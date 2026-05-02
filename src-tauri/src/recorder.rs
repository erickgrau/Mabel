use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager};

use crate::audio::AudioRecorder;
use crate::cleanup::cleanup_text;
use crate::paste::{extract_press_enter_command, paste_text, press_return};
use crate::settings::Settings;
use crate::stats::StatsStore;
use crate::streaming::{self, StreamingHandle};
use crate::system_ui;
use crate::transcribe_groq;
use crate::transcribe_local;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum RecordingState {
    Ready,
    Recording,
    Transcribing,
}

fn update_overlay(app: &AppHandle, state: &RecordingState) {
    if let Some(overlay) = app.get_webview_window("overlay") {
        let class = match state {
            RecordingState::Ready => "mic",
            RecordingState::Recording => "mic recording",
            RecordingState::Transcribing => "mic transcribing",
        };
        let js = format!("document.getElementById('mic').className = '{}';", class);
        let _ = overlay.eval(&js);
    }
}

pub struct Recorder {
    state: Arc<Mutex<RecordingState>>,
    audio_recorder: Arc<Mutex<AudioRecorder>>,
    streaming_handle: Arc<Mutex<Option<StreamingHandle>>>,
    streaming_words: Arc<AtomicU64>,
    started_at: Arc<Mutex<Option<Instant>>>,
    stats: Arc<StatsStore>,
}

impl Recorder {
    pub fn new(stats: Arc<StatsStore>) -> Self {
        Self {
            state: Arc::new(Mutex::new(RecordingState::Ready)),
            audio_recorder: Arc::new(Mutex::new(AudioRecorder::new())),
            streaming_handle: Arc::new(Mutex::new(None)),
            streaming_words: Arc::new(AtomicU64::new(0)),
            started_at: Arc::new(Mutex::new(None)),
            stats,
        }
    }

    pub fn get_state(&self) -> RecordingState {
        self.state.lock().unwrap().clone()
    }

    pub fn start_recording(
        &self,
        app: &AppHandle,
        mic_name: &str,
        settings: &Settings,
        app_dir: &PathBuf,
    ) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        if *state != RecordingState::Ready {
            return Err("Already recording or transcribing".to_string());
        }

        {
            let mut recorder = self.audio_recorder.lock().unwrap();
            recorder.start(app, mic_name)?;
        }

        self.streaming_words.store(0, Ordering::Relaxed);
        *self.started_at.lock().unwrap() = Some(Instant::now());

        // Skip streaming when AI cleanup is on. With streaming, each chunk is
        // pasted independently with only the rules-based cleanup pass — the LLM
        // never sees the full utterance, so the user gets a worse result than
        // rules-only mode. In LLM mode we want the full utterance transcribed
        // once, sent to the LLM once, and pasted once. The cost is losing the
        // live-transcription feel; the benefit is the cleanup actually working.
        if settings.streaming && settings.cleanup_mode != "llm" {
            let handle = streaming::spawn_vad_worker(
                app.clone(),
                self.audio_recorder.clone(),
                settings.clone(),
                app_dir.clone(),
                self.streaming_words.clone(),
            );
            *self.streaming_handle.lock().unwrap() = Some(handle);
        }

        *state = RecordingState::Recording;
        let _ = app.emit("recording-state", RecordingState::Recording);
        update_overlay(app, &RecordingState::Recording);

        // Intentionally NOT playing a start sound here. Spawning afplay right
        // after the cpal input stream opens triggers an audio session
        // reconfiguration on Apple Silicon and the mic gain drops to near-zero,
        // which makes Whisper see only ambient noise. Bug-fixed post v1.0.0.

        Ok(())
    }

    pub async fn stop_and_transcribe(
        &self,
        app: &AppHandle,
        settings: &Settings,
        app_dir: &PathBuf,
    ) -> Result<String, String> {
        {
            let mut state = self.state.lock().unwrap();
            if *state != RecordingState::Recording {
                return Err("Not currently recording".to_string());
            }
            *state = RecordingState::Transcribing;
            let _ = app.emit("recording-state", RecordingState::Transcribing);
            update_overlay(app, &RecordingState::Transcribing);
        }

        let elapsed_seconds = self
            .started_at
            .lock()
            .unwrap()
            .take()
            .map(|t| t.elapsed().as_secs_f64())
            .unwrap_or(0.0);

        let streaming_handle = self.streaming_handle.lock().unwrap().take();
        if let Some(handle) = streaming_handle {
            handle.stop().await;
            {
                let mut recorder = self.audio_recorder.lock().unwrap();
                recorder.stop_stream_only();
            }
            streaming::flush_final_chunk(
                app,
                self.audio_recorder.clone(),
                settings,
                app_dir,
                self.streaming_words.clone(),
            )
            .await;

            let words = self.streaming_words.load(Ordering::Relaxed);
            if words > 0 {
                self.stats.record(words, elapsed_seconds);
            }

            *self.state.lock().unwrap() = RecordingState::Ready;
            let _ = app.emit("recording-state", RecordingState::Ready);
            let _ = app.emit("stats-updated", ());
            update_overlay(app, &RecordingState::Ready);
            if settings.dictation_sounds {
                system_ui::play_sound("Pop");
            }
            return Ok(String::new());
        }

        let temp_path = app_dir.join("temp_recording.wav");
        {
            let mut recorder = self.audio_recorder.lock().unwrap();
            recorder.stop_and_save(&temp_path)?;
        }

        let raw_text = match settings.engine.as_str() {
            "local" => {
                let model_file = transcribe_local::model_filename(
                    &settings.whisper_model,
                    &settings.whisper_language,
                )?;
                let model_path = app_dir.join(model_file);
                transcribe_local::transcribe_local(
                    app,
                    &model_path,
                    &temp_path,
                    &settings.dictionary,
                )
                .await?
            }
            "cloud" => {
                let key = crate::secrets::get_groq_key()?;
                transcribe_groq::transcribe_groq(&key, &temp_path).await?
            }
            _ => return Err(format!("Unknown engine: {}", settings.engine)),
        };

        let debug_path = app_dir.join("last_recording.wav");
        let _ = std::fs::copy(&temp_path, &debug_path);
        let _ = std::fs::remove_file(&temp_path);

        let rule_cleaned = cleanup_text(&raw_text);
        let cleaned = if settings.cleanup_mode == "llm" && !rule_cleaned.is_empty() {
            println!("[Mabel] LLM cleanup input: {:?}", rule_cleaned);
            let t0 = std::time::Instant::now();
            match crate::llm::cleanup_with_llm(&rule_cleaned).await {
                Ok(s) if !s.is_empty() => {
                    println!("[Mabel] LLM cleanup output ({:?}): {:?}", t0.elapsed(), s);
                    s
                }
                Ok(empty) => {
                    println!("[Mabel] LLM returned empty ({:?}, raw={:?}); falling back to rules", t0.elapsed(), empty);
                    rule_cleaned
                }
                Err(e) => {
                    eprintln!("[Mabel] LLM cleanup failed ({:?}), using rules: {}", t0.elapsed(), e);
                    rule_cleaned
                }
            }
        } else {
            rule_cleaned
        };
        println!("[Mabel] About to paste: {:?}", cleaned);
        let (to_paste, press_enter) =
            extract_press_enter_command(&cleaned, settings.press_enter_command);
        if !to_paste.is_empty() {
            paste_text(&to_paste)?;
            let words = to_paste.split_whitespace().count() as u64;
            if words > 0 {
                self.stats.record(words, elapsed_seconds);
            }
        }
        if press_enter {
            std::thread::sleep(std::time::Duration::from_millis(50));
            let _ = press_return();
        }

        {
            let mut state = self.state.lock().unwrap();
            *state = RecordingState::Ready;
            let _ = app.emit("recording-state", RecordingState::Ready);
            let _ = app.emit("stats-updated", ());
            update_overlay(app, &RecordingState::Ready);
        }

        if settings.dictation_sounds {
            system_ui::play_sound("Pop");
        }

        Ok(to_paste)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state_is_ready() {
        let stats = Arc::new(StatsStore::load(&PathBuf::from("/tmp/mabel-test-recorder")));
        let recorder = Recorder::new(stats);
        assert_eq!(recorder.get_state(), RecordingState::Ready);
    }
}
