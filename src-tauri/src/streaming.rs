use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::AppHandle;
use tokio::task::JoinHandle;

use crate::audio::AudioRecorder;
use crate::cleanup::cleanup_text;
use crate::paste::{extract_press_enter_command, paste_text, press_return};
use crate::settings::Settings;
use crate::transcribe_groq;
use crate::transcribe_local;

const TICK: Duration = Duration::from_millis(33);
const SPEECH_RMS: f32 = 0.020;
const SILENCE_RMS: f32 = 0.008;
const SILENCE_FOR_END: Duration = Duration::from_millis(700);
const MIN_CHUNK_SPEECH: Duration = Duration::from_millis(400);
const MAX_CHUNK: Duration = Duration::from_secs(10);
const SPEECH_DEBOUNCE_TICKS: u32 = 3;
const MIN_CHUNK_RMS: f32 = 0.012;

pub struct StreamingHandle {
    running: Arc<AtomicBool>,
    task: Option<JoinHandle<()>>,
}

impl StreamingHandle {
    pub async fn stop(mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(t) = self.task.take() {
            let _ = t.await;
        }
    }
}

pub fn spawn_vad_worker(
    app: AppHandle,
    recorder: Arc<Mutex<AudioRecorder>>,
    settings: Settings,
    app_dir: PathBuf,
    words_counter: Arc<AtomicU64>,
) -> StreamingHandle {
    let running = Arc::new(AtomicBool::new(true));
    let running_handle = running.clone();
    let chunk_seq = Arc::new(AtomicU64::new(0));

    let task = tokio::spawn(async move {
        let mut speech_started: Option<Instant> = None;
        let mut last_speech: Option<Instant> = None;
        let mut consecutive_loud: u32 = 0;

        while running_handle.load(Ordering::Relaxed) {
            tokio::time::sleep(TICK).await;

            let level = {
                let r = recorder.lock().unwrap();
                r.current_level()
            };
            let now = Instant::now();

            if level > SPEECH_RMS {
                consecutive_loud = consecutive_loud.saturating_add(1);
                if consecutive_loud >= SPEECH_DEBOUNCE_TICKS {
                    if speech_started.is_none() {
                        speech_started = Some(now);
                    }
                    last_speech = Some(now);
                }
            } else {
                consecutive_loud = 0;
            }

            let should_chunk = if let (Some(start), Some(last)) = (speech_started, last_speech) {
                let total_speech = now.duration_since(start);
                let elapsed_since_speech = now.duration_since(last);
                let long_enough = total_speech > MIN_CHUNK_SPEECH;
                let trailing_silence = level < SILENCE_RMS && elapsed_since_speech > SILENCE_FOR_END;
                let force_flush = total_speech > MAX_CHUNK;
                long_enough && (trailing_silence || force_flush)
            } else {
                false
            };

            if should_chunk {
                speech_started = None;
                last_speech = None;
                consecutive_loud = 0;

                let path = chunk_path(&app_dir, &chunk_seq);
                let saved = {
                    let mut r = recorder.lock().unwrap();
                    r.drain_to_wav(&path)
                };

                match saved {
                    Ok(Some(rms)) if rms >= MIN_CHUNK_RMS => {
                        let app2 = app.clone();
                        let settings2 = settings.clone();
                        let app_dir2 = app_dir.clone();
                        let counter2 = words_counter.clone();
                        tokio::spawn(async move {
                            transcribe_and_paste(app2, path, settings2, app_dir2, counter2, false).await;
                        });
                    }
                    Ok(Some(rms)) => {
                        let _ = std::fs::remove_file(&path);
                        println!("[Mabel] dropped silent chunk (rms={:.4})", rms);
                    }
                    Ok(None) => {}
                    Err(e) => eprintln!("[Mabel] streaming chunk write failed: {}", e),
                }
            }
        }
    });

    StreamingHandle {
        running,
        task: Some(task),
    }
}

pub async fn flush_final_chunk(
    app: &AppHandle,
    recorder: Arc<Mutex<AudioRecorder>>,
    settings: &Settings,
    app_dir: &PathBuf,
    words_counter: Arc<AtomicU64>,
) {
    let path = app_dir.join("chunk_final.wav");
    let saved = {
        let mut r = recorder.lock().unwrap();
        r.drain_to_wav(&path)
    };
    match saved {
        Ok(Some(rms)) if rms >= MIN_CHUNK_RMS => {
            transcribe_and_paste(app.clone(), path, settings.clone(), app_dir.clone(), words_counter, true).await;
        }
        Ok(Some(rms)) => {
            let _ = std::fs::remove_file(&path);
            println!("[Mabel] dropped silent final chunk (rms={:.4})", rms);
        }
        Ok(None) => {}
        Err(e) => eprintln!("[Mabel] streaming final chunk write failed: {}", e),
    }
}

fn chunk_path(app_dir: &PathBuf, seq: &Arc<AtomicU64>) -> PathBuf {
    let n = seq.fetch_add(1, Ordering::Relaxed);
    app_dir.join(format!("chunk_{}.wav", n))
}

async fn transcribe_and_paste(
    app: AppHandle,
    path: PathBuf,
    settings: Settings,
    app_dir: PathBuf,
    words_counter: Arc<AtomicU64>,
    is_final: bool,
) {
    let raw = match settings.engine.as_str() {
        "local" => match transcribe_local::model_filename(
            &settings.whisper_model,
            &settings.whisper_language,
        ) {
            Ok(model_file) => {
                let model_path = app_dir.join(model_file);
                transcribe_local::transcribe_local(
                    &app,
                    &model_path,
                    &path,
                    &settings.dictionary,
                )
                .await
            }
            Err(e) => Err(format!("invalid model: {}", e)),
        },
        "cloud" => match crate::secrets::get_groq_key() {
            Ok(key) => transcribe_groq::transcribe_groq(&key, &path).await,
            Err(e) => Err(e),
        },
        _ => Err(format!("unknown engine: {}", settings.engine)),
    };

    let _ = std::fs::remove_file(&path);

    match raw {
        Ok(text) => {
            let rule_cleaned = cleanup_text(&text);
            if rule_cleaned.is_empty() {
                return;
            }
            // LLM cleanup only on the final chunk in streaming mode. Per-chunk
            // LLM passes would add 300-500ms to every paste and break the
            // "live transcription" feel.
            let cleaned = if is_final && settings.cleanup_mode == "llm" {
                match crate::llm::cleanup_with_llm(&rule_cleaned).await {
                    Ok(s) if !s.is_empty() => s,
                    Ok(_) => rule_cleaned,
                    Err(e) => {
                        eprintln!("[Mabel] LLM cleanup failed, using rules: {}", e);
                        rule_cleaned
                    }
                }
            } else {
                rule_cleaned
            };
            // Only the final chunk can carry a "press enter" command — otherwise
            // we'd fire Return mid-sentence.
            let (to_paste, press_enter) = if is_final {
                extract_press_enter_command(&cleaned, settings.press_enter_command)
            } else {
                (cleaned.clone(), false)
            };
            if !to_paste.is_empty() {
                if let Err(e) = paste_text(&to_paste) {
                    eprintln!("[Mabel] paste failed: {}", e);
                    return;
                }
                let words = to_paste.split_whitespace().count() as u64;
                words_counter.fetch_add(words, Ordering::Relaxed);
            }
            if press_enter {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let _ = press_return();
            }
        }
        Err(e) => eprintln!("[Mabel] streaming transcription failed: {}", e),
    }
}
