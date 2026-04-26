use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

use crate::audio::AudioRecorder;
use crate::cleanup::cleanup_text;
use crate::paste::paste_text;
use crate::settings::Settings;
use crate::streaming::{self, StreamingHandle};
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
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RecordingState::Ready)),
            audio_recorder: Arc::new(Mutex::new(AudioRecorder::new())),
            streaming_handle: Arc::new(Mutex::new(None)),
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

        // If streaming is enabled, fire up the VAD worker. It pulls from the
        // shared AudioRecorder buffer and ships chunks to whisper as they arrive.
        if settings.streaming {
            let handle = streaming::spawn_vad_worker(
                app.clone(),
                self.audio_recorder.clone(),
                settings.clone(),
                app_dir.clone(),
            );
            *self.streaming_handle.lock().unwrap() = Some(handle);
        }

        *state = RecordingState::Recording;
        let _ = app.emit("recording-state", RecordingState::Recording);
        update_overlay(app, &RecordingState::Recording);

        Ok(())
    }

    pub async fn stop_and_transcribe(
        &self,
        app: &AppHandle,
        settings: &Settings,
        app_dir: &PathBuf,
    ) -> Result<String, String> {
        // Stop recording
        {
            let mut state = self.state.lock().unwrap();
            if *state != RecordingState::Recording {
                return Err("Not currently recording".to_string());
            }
            *state = RecordingState::Transcribing;
            let _ = app.emit("recording-state", RecordingState::Transcribing);
            update_overlay(app, &RecordingState::Transcribing);
        }

        // If streaming was running, shut down the VAD worker, stop the cpal
        // stream, and flush any tail audio as one last chunk.
        let streaming_handle = self.streaming_handle.lock().unwrap().take();
        if let Some(handle) = streaming_handle {
            handle.stop().await;
            {
                let mut recorder = self.audio_recorder.lock().unwrap();
                recorder.stop_stream_only();
            }
            streaming::flush_final_chunk(app, self.audio_recorder.clone(), settings, app_dir).await;

            *self.state.lock().unwrap() = RecordingState::Ready;
            let _ = app.emit("recording-state", RecordingState::Ready);
            update_overlay(app, &RecordingState::Ready);
            return Ok(String::new());
        }

        // Non-streaming: existing one-shot path. Save WAV, transcribe whole thing,
        // paste once.
        let temp_path = app_dir.join("temp_recording.wav");
        {
            let mut recorder = self.audio_recorder.lock().unwrap();
            recorder.stop_and_save(&temp_path)?;
        }

        let raw_text = match settings.engine.as_str() {
            "local" => {
                let model_file = transcribe_local::model_filename(&settings.whisper_model)?;
                let model_path = app_dir.join(model_file);
                transcribe_local::transcribe_local(app, &model_path, &temp_path).await?
            }
            "cloud" => {
                let key = crate::secrets::get_groq_key()?;
                transcribe_groq::transcribe_groq(&key, &temp_path).await?
            }
            _ => return Err(format!("Unknown engine: {}", settings.engine)),
        };

        // Keep a copy for debugging, then drop the temp.
        let debug_path = app_dir.join("last_recording.wav");
        let _ = std::fs::copy(&temp_path, &debug_path);
        let _ = std::fs::remove_file(&temp_path);

        let cleaned = cleanup_text(&raw_text);
        if !cleaned.is_empty() {
            paste_text(&cleaned)?;
        }

        {
            let mut state = self.state.lock().unwrap();
            *state = RecordingState::Ready;
            let _ = app.emit("recording-state", RecordingState::Ready);
            update_overlay(app, &RecordingState::Ready);
        }

        Ok(cleaned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state_is_ready() {
        let recorder = Recorder::new();
        assert_eq!(recorder.get_state(), RecordingState::Ready);
    }
}
