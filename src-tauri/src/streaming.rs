use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::AppHandle;
use tokio::task::JoinHandle;

use crate::audio::AudioRecorder;
use crate::cleanup::cleanup_text;
use crate::paste::paste_text;
use crate::settings::Settings;
use crate::transcribe_groq;
use crate::transcribe_local;

const TICK: Duration = Duration::from_millis(33);
// Tightened thresholds to keep brief noise from being misread as speech.
// Speech onset requires a sustained burst above SPEECH_RMS; a single spike
// won't open a chunk. Chunks that end up averaging below MIN_CHUNK_RMS are
// dropped entirely so Whisper never sees near-silence (which it hallucinates on).
const SPEECH_RMS: f32 = 0.020;
const SILENCE_RMS: f32 = 0.008;
const SILENCE_FOR_END: Duration = Duration::from_millis(700);
const MIN_CHUNK_SPEECH: Duration = Duration::from_millis(400);
const MAX_CHUNK: Duration = Duration::from_secs(10);
const SPEECH_DEBOUNCE_TICKS: u32 = 3; // ~100 ms above threshold before "speech"
const MIN_CHUNK_RMS: f32 = 0.012; // chunks averaging below this don't go to Whisper

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

/// Spawn a VAD-driven worker that flushes speech chunks to disk and transcribes
/// them while recording continues. Each chunk is processed in its own task so
/// long inferences don't block the next chunk's detection.
pub fn spawn_vad_worker(
    app: AppHandle,
    recorder: Arc<Mutex<AudioRecorder>>,
    settings: Settings,
    app_dir: PathBuf,
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

            // Debounce: require N consecutive ticks above threshold before
            // we treat this as actual speech onset. Keeps single-tick noise
            // spikes from opening empty chunks.
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

            // Decide whether to chunk:
            //  - we've heard enough speech to be worth transcribing, AND
            //  - either there's been enough trailing silence, OR
            //  - the chunk has run too long.
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
                        tokio::spawn(async move {
                            transcribe_and_paste(app2, path, settings2, app_dir2).await;
                        });
                    }
                    Ok(Some(rms)) => {
                        // Chunk was mostly silence. Skip Whisper.
                        let _ = std::fs::remove_file(&path);
                        println!("[Mabel] dropped silent chunk (rms={:.4})", rms);
                    }
                    Ok(None) => {} // nothing buffered, skip
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

/// Drain whatever is currently buffered as a final chunk after the user stops.
/// Awaits the transcription so the caller knows it landed.
pub async fn flush_final_chunk(
    app: &AppHandle,
    recorder: Arc<Mutex<AudioRecorder>>,
    settings: &Settings,
    app_dir: &PathBuf,
) {
    let path = app_dir.join("chunk_final.wav");
    let saved = {
        let mut r = recorder.lock().unwrap();
        r.drain_to_wav(&path)
    };
    match saved {
        Ok(Some(rms)) if rms >= MIN_CHUNK_RMS => {
            transcribe_and_paste(app.clone(), path, settings.clone(), app_dir.clone()).await;
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

async fn transcribe_and_paste(app: AppHandle, path: PathBuf, settings: Settings, app_dir: PathBuf) {
    let raw = match settings.engine.as_str() {
        "local" => match transcribe_local::model_filename(&settings.whisper_model) {
            Ok(model_file) => {
                let model_path = app_dir.join(model_file);
                transcribe_local::transcribe_local(&app, &model_path, &path).await
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
            let cleaned = cleanup_text(&text);
            if !cleaned.is_empty() {
                if let Err(e) = paste_text(&cleaned) {
                    eprintln!("[Mabel] paste failed: {}", e);
                }
            }
        }
        Err(e) => eprintln!("[Mabel] streaming transcription failed: {}", e),
    }
}
