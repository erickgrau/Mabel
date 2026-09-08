use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager};

use crate::audio::AudioRecorder;
use crate::cleanup::cleanup_text;
use crate::dictation_error::{self, UserError};
use crate::llm::LlmServer;
use crate::local_engine;
use crate::mic_permission;
use crate::paste::{extract_press_enter_command, paste_text, press_return};
use crate::settings::Settings;
use crate::stats::StatsStore;
use crate::streaming::{self, StreamingHandle};
use crate::system_ui;
use crate::transcribe_groq;
use crate::transcribe_native;

pub const TEMP_RECORDING_WAV: &str = "temp_recording.wav";
pub const LEGACY_LAST_RECORDING_WAV: &str = "last_recording.wav";

/// Global-shortcut backends can deliver a second Pressed (key-repeat or a
/// doubled CGEvent) before Released. Toggle mode used to treat that as
/// start-then-stop and feed Parakeet a ~50–250ms clip.
pub fn hotkey_pressed_is_new(already_held: bool) -> bool {
    !already_held
}

/// Delete leftover dictation audio in App Support. Called after a successful
/// transcribe and on launch so a leftover `last_recording.wav` from older
/// builds cannot linger.
pub fn wipe_audio_artifacts(app_dir: &PathBuf) {
    for name in [TEMP_RECORDING_WAV, LEGACY_LAST_RECORDING_WAV] {
        let path = app_dir.join(name);
        if path.exists() {
            match std::fs::remove_file(&path) {
                Ok(()) => println!("[Mabel] wiped leftover audio {:?}", path),
                Err(e) => eprintln!("[Mabel] failed to wipe {:?}: {}", path, e),
            }
        }
    }
}

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
    llm_server: Arc<LlmServer>,
}

impl Recorder {
    pub fn new(stats: Arc<StatsStore>, llm_server: Arc<LlmServer>) -> Self {
        Self {
            state: Arc::new(Mutex::new(RecordingState::Ready)),
            audio_recorder: Arc::new(Mutex::new(AudioRecorder::new())),
            streaming_handle: Arc::new(Mutex::new(None)),
            streaming_words: Arc::new(AtomicU64::new(0)),
            started_at: Arc::new(Mutex::new(None)),
            stats,
            llm_server,
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
    ) -> Result<(), UserError> {
        // Claim Recording before the slow TCC / engine / cpal work. The May
        // hotkey-stop deadlock fix released this lock too early, so a second
        // Pressed (key-repeat or a doubled global-shortcut event) still saw
        // Ready and either started a second stream or immediately stopped a
        // ~50–250ms tap. Parakeet then rejected the short WAV.
        {
            let mut state = self.state.lock().unwrap();
            if *state != RecordingState::Ready {
                return Err(UserError::generic("Already recording or transcribing"));
            }
            *state = RecordingState::Recording;
        }

        let revert_ready = || {
            *self.state.lock().unwrap() = RecordingState::Ready;
        };

        // Fail closed before the overlay goes to Listening. MAS/TF can open a
        // CoreAudio stream and show the UI while TCC is denied and the model
        // is missing; that is the 1.3.0 build 1302 smoke failure.
        if let Err(err) = mic_permission::ensure_granted() {
            revert_ready();
            return Err(err);
        }
        if let Err(err) = ensure_engine_ready(settings, app_dir) {
            revert_ready();
            return Err(err);
        }

        {
            let mut recorder = self.audio_recorder.lock().unwrap();
            if let Err(e) = recorder.start(app, mic_name) {
                revert_ready();
                return Err(UserError::new(dictation_error::TITLE_CAPTURE, e));
            }
        }

        self.streaming_words.store(0, Ordering::Relaxed);
        *self.started_at.lock().unwrap() = Some(Instant::now());

        // Temporary safety switch: disable streaming worker until we resolve
        // a shutdown hang seen in stop_and_transcribe on some machines.
        // This keeps dictation reliable by always using full-utterance
        // transcription on stop. Do NOT unpark live streaming from this tip.
        if settings.streaming && settings.cleanup_mode != "llm" {
            crate::debug_log::append(app_dir, "streaming requested but temporarily disabled");
            *self.streaming_handle.lock().unwrap() = None;
        }

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
        crate::debug_log::append(app_dir, "stop_and_transcribe entered");
        crate::debug_log::append(
            app_dir,
            &format!(
                "settings snapshot: engine={} streaming={} cleanup_mode={} polish_mode={}",
                settings.engine, settings.streaming, settings.cleanup_mode, settings.polish_mode
            ),
        );
        println!("[Mabel] stop_and_transcribe entered");
        {
            crate::debug_log::append(app_dir, "transitioning recorder state to Transcribing");
            let mut state = self.state.lock().unwrap();
            if *state != RecordingState::Recording {
                crate::debug_log::append(
                    app_dir,
                    &format!("stop ignored: recorder state was {:?}", *state),
                );
                return Err("Not currently recording".to_string());
            }
            *state = RecordingState::Transcribing;
            let _ = app.emit("recording-state", RecordingState::Transcribing);
            update_overlay(app, &RecordingState::Transcribing);
        }
        crate::debug_log::append(app_dir, "recorder state is Transcribing");

        let elapsed = self
            .started_at
            .lock()
            .unwrap()
            .take()
            .map(|t| t.elapsed())
            .unwrap_or_default();
        let elapsed_seconds = elapsed.as_secs_f64();
        let elapsed_ms = elapsed.as_millis() as u64;

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

        let temp_path = app_dir.join(TEMP_RECORDING_WAV);
        let stop_and_save_result = {
            let mut recorder = self.audio_recorder.lock().unwrap();
            recorder.stop_and_save(&temp_path)
        };
        let captured_rms = match stop_and_save_result {
            Ok((_path, rms)) => rms,
            Err(err) => {
                let user_err = err.to_user_error(elapsed_ms);
                crate::debug_log::append(
                    app_dir,
                    &format!("stop_and_save failed: {}", user_err.message),
                );
                eprintln!("[Mabel] stop_and_save failed: {}", user_err.message);
                let _ = std::fs::remove_file(&temp_path);

                {
                    let mut state = self.state.lock().unwrap();
                    *state = RecordingState::Ready;
                    let _ = app.emit("recording-state", RecordingState::Ready);
                    let _ = app.emit("stats-updated", ());
                    update_overlay(app, &RecordingState::Ready);
                }

                dictation_error::emit(app, &user_err);
                return Err(user_err.message);
            }
        };
        if settings.engine == "local"
            && crate::audio::native_engine_needs_min_duration(&settings.local_engine)
        {
            match crate::audio::pad_pcm16_mono_wav(&temp_path, crate::audio::NATIVE_ASR_MIN_SAMPLES)
            {
                Ok(samples) => {
                    crate::debug_log::append(
                        app_dir,
                        &format!("padded native wav to {} samples", samples),
                    );
                }
                Err(err) => {
                    crate::debug_log::append(app_dir, &format!("native wav pad skipped: {err}"));
                }
            }
        }
        if let Ok(meta) = std::fs::metadata(&temp_path) {
            crate::debug_log::append(
                app_dir,
                &format!(
                    "captured temp wav {} bytes rms={:.6}",
                    meta.len(),
                    captured_rms
                ),
            );
            println!(
                "[Mabel] Captured temp WAV: {} bytes rms={:.6}",
                meta.len(),
                captured_rms
            );
        } else {
            crate::debug_log::append(app_dir, "captured temp wav metadata unavailable");
            println!("[Mabel] Captured temp WAV: metadata unavailable");
        }

        let result: Result<String, String> = async {
            crate::debug_log::append(
                app_dir,
                &format!("transcription engine={}", settings.engine),
            );
            println!("[Mabel] Transcription engine: {}", settings.engine);
            let raw_text =
                transcribe_isolated_take(app, settings, app_dir, &temp_path).await?;
            println!(
                "[Mabel] Transcription returned (chars={})",
                raw_text.chars().count()
            );
            crate::debug_log::append(
                app_dir,
                &format!("transcription returned chars={}", raw_text.chars().count()),
            );
            if raw_text.trim().is_empty() {
                crate::debug_log::append(
                    app_dir,
                    "ASR returned empty transcript on a captured take (not overlay-mapped silence)",
                );
            }

            let rule_cleaned = cleanup_text(&raw_text);
            let cleaned =
                crate::llm::polish_or_rules(app, &self.llm_server, settings, app_dir, rule_cleaned)
                    .await;
            let cleaned = crate::dictionary::apply_replacements(&cleaned, &settings.dictionary);
            let cleaned = crate::snippets::apply_expansions_for_dir(&cleaned, app_dir);
            let cleaned = crate::style::apply_register_for_dir(&cleaned, app_dir);

            let (to_paste, press_enter) =
                extract_press_enter_command(&cleaned, settings.press_enter_command);
            println!(
                "[Mabel] Final text prepared (chars={}, press_enter={})",
                to_paste.chars().count(),
                press_enter
            );
            crate::debug_log::append(
                app_dir,
                &format!(
                    "final text chars={} press_enter={}",
                    to_paste.chars().count(),
                    press_enter
                ),
            );
            if to_paste.is_empty() {
                return Err(dictation_error::nothing_recognized().message);
            }
            crate::debug_log::append(app_dir, "pasting text");
            println!("[Mabel] Pasting text");
            paste_text(&to_paste)?;
            crate::debug_log::append(app_dir, "paste command completed");
            let words = to_paste.split_whitespace().count() as u64;
            if words > 0 {
                self.stats.record(words, elapsed_seconds);
            }
            if press_enter {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let _ = press_return();
            }

            Ok(to_paste)
        }
        .await;

        let _ = std::fs::remove_file(&temp_path);

        {
            let mut state = self.state.lock().unwrap();
            *state = RecordingState::Ready;
            let _ = app.emit("recording-state", RecordingState::Ready);
            let _ = app.emit("stats-updated", ());
            update_overlay(app, &RecordingState::Ready);
        }

        match result {
            Ok(text) => {
                crate::debug_log::append(app_dir, "stop_and_transcribe succeeded");
                println!("[Mabel] stop_and_transcribe succeeded");
                wipe_audio_artifacts(app_dir);
                if settings.dictation_sounds {
                    system_ui::play_sound("Pop");
                }
                Ok(text)
            }
            Err(err) => {
                crate::debug_log::append(
                    app_dir,
                    &format!("transcription pipeline failed: {}", err),
                );
                eprintln!("[Mabel] Transcription pipeline failed: {}", err);
                let user_err = classify_pipeline_error(&err);
                dictation_error::emit(app, &user_err);
                Err(user_err.message)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state_is_ready() {
        let stats = Arc::new(StatsStore::load(&PathBuf::from("/tmp/mabel-test-recorder")));
        let recorder = Recorder::new(stats, Arc::new(LlmServer::new()));
        assert_eq!(recorder.get_state(), RecordingState::Ready);
    }

    #[test]
    fn hotkey_repeat_pressed_is_ignored_until_release() {
        assert!(hotkey_pressed_is_new(false));
        assert!(!hotkey_pressed_is_new(true));
    }

    #[test]
    fn wipe_audio_artifacts_deletes_temp_and_legacy_wav() {
        let dir = std::env::temp_dir().join(format!(
            "mabel-wav-wipe-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let temp = dir.join(TEMP_RECORDING_WAV);
        let legacy = dir.join(LEGACY_LAST_RECORDING_WAV);
        std::fs::write(&temp, b"temp").unwrap();
        std::fs::write(&legacy, b"legacy").unwrap();
        wipe_audio_artifacts(&dir);
        assert!(!temp.exists());
        assert!(!legacy.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn engine_ready_rejects_missing_whisper_cpp_on_mas_flavor() {
        if local_engine::whisper_cpp_sidecar_compiled() {
            return;
        }
        let settings = Settings {
            engine: "local".into(),
            local_engine: local_engine::WHISPER_CPP.to_string(),
            ..Settings::default()
        };
        let err = ensure_engine_ready(&settings, &PathBuf::from("/tmp")).unwrap_err();
        assert_eq!(err.title, dictation_error::TITLE_MODEL);
        assert!(err.message.contains("whisper.cpp"), "{}", err.message);
    }

    #[test]
    fn missing_parakeet_fails_closed_when_not_ready() {
        if transcribe_native::engine_ready(local_engine::PARAKEET, "en", &PathBuf::from("/tmp")) {
            return;
        }
        let settings = Settings::default();
        let err = ensure_engine_ready(&settings, &PathBuf::from("/tmp")).unwrap_err();
        assert_eq!(err.title, dictation_error::TITLE_MODEL);
        assert!(err.message.contains("Parakeet"), "{}", err.message);
    }

    #[test]
    fn classify_maps_empty_and_paste_and_model() {
        let empty = classify_pipeline_error(&dictation_error::nothing_recognized().message);
        assert_eq!(empty.title, dictation_error::TITLE_EMPTY);
        let paste = classify_pipeline_error("AppleScript paste failed (status 1): not authorized");
        assert_eq!(paste.title, dictation_error::TITLE_PASTE);
        let model = classify_pipeline_error(
            "Native CoreML engines are not linked in this binary. On a Mac with Xcode 16+",
        );
        assert_eq!(model.title, dictation_error::TITLE_MODEL);
        let short = classify_pipeline_error("Parakeet transcribe failed: invalid audio data");
        assert_eq!(short.title, dictation_error::TITLE_CAPTURE);
        assert_ne!(short.title, dictation_error::TITLE_MODEL);
        assert!(short.message.contains("too short"), "{}", short.message);
    }

    #[test]
    fn classify_cloud_keychain_and_groq_auth_are_not_empty_takes() {
        let keychain =
            classify_pipeline_error("Keychain read error: default keychain could not be found");
        assert_eq!(keychain.title, dictation_error::TITLE_CLOUD);
        assert_ne!(keychain.title, dictation_error::TITLE_EMPTY);

        let missing =
            classify_pipeline_error("Groq API key not set. Please enter your API key in settings.");
        assert_eq!(missing.title, dictation_error::TITLE_CLOUD);
        assert_ne!(missing.title, dictation_error::TITLE_EMPTY);

        let unauthorized = classify_pipeline_error("Groq API error (401): invalid api key");
        assert_eq!(unauthorized.title, dictation_error::TITLE_CLOUD);
        assert_ne!(unauthorized.title, dictation_error::TITLE_EMPTY);
    }

    #[test]
    fn cloud_ready_and_transcribe_fail_closed_on_keychain() {
        let src = include_str!("recorder.rs");
        let ensure = src
            .split("fn ensure_engine_ready")
            .nth(1)
            .expect("ensure_engine_ready");
        assert!(
            ensure.contains("get_groq_key"),
            "cloud ready must read the key fail-closed, not only has_groq_key"
        );
        assert!(
            ensure.contains("cloud_unavailable"),
            "keychain errors at start must surface as Cloud engine failed"
        );
        let stop = src
            .split("pub async fn stop_and_transcribe")
            .nth(1)
            .expect("stop_and_transcribe");
        assert!(
            src.contains("cloud key read failed") && stop.contains("transcribe_isolated_take"),
            "cloud keychain fail must be logged, not swallowed as empty ASR"
        );
        assert!(
            src.contains("is_cloud_auth_or_keychain_error"),
            "pipeline must classify keychain/Groq auth before Nothing recognized"
        );
    }

    #[test]
    fn start_recording_claims_state_before_capture() {
        let src = include_str!("recorder.rs");
        let start = src
            .split("pub fn start_recording")
            .nth(1)
            .expect("start_recording");
        let start = start
            .split("pub async fn stop_and_transcribe")
            .next()
            .unwrap();
        let ready_idx = start.find("RecordingState::Ready").expect("Ready check");
        let claim_idx = start
            .find("*state = RecordingState::Recording")
            .expect("claim Recording under the same lock");
        let mic_idx = start
            .find("mic_permission::ensure_granted")
            .expect("mic gate");
        assert!(
            ready_idx < claim_idx && claim_idx < mic_idx,
            "Ready check must claim Recording before TCC/cpal so a second hotkey cannot also start"
        );
        assert!(
            start.contains("revert_ready"),
            "failed start must put the recorder back to Ready"
        );
        assert!(
            !start.contains("spawn_vad_worker"),
            "this tip must not unpark live streaming"
        );
    }

    #[test]
    fn stop_does_not_teardown_asr_or_unpark_streaming() {
        let src = include_str!("recorder.rs");
        let stop = src
            .split("pub async fn stop_and_transcribe")
            .nth(1)
            .expect("stop_and_transcribe")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        assert!(
            !stop.contains("spawn_vad_worker"),
            "this tip must not unpark live streaming"
        );
        assert!(
            !stop.contains("mabel_asr") && !stop.contains(".cleanup("),
            "stop must not teardown the in-process ASR session"
        );
        assert!(
            stop.contains("transcribe_isolated_take"),
            "long Mac takes must isolate ASR context on stop"
        );
        assert!(
            src.contains("accept_asr_or_fail_closed"),
            "degraded ASR must fail closed rather than inject garbage"
        );
        assert!(
            stop.contains("to_paste.is_empty()"),
            "empty ASR text is the Nothing recognized path, not a silent Ready"
        );
    }
}

/// Isolate long toggle takes into ≤20s windows so Whisper/cloud cannot
/// condition later speech on earlier invented text. Soft reset only —
/// does not unload the warm CoreML session.
async fn transcribe_isolated_take(
    app: &AppHandle,
    settings: &Settings,
    app_dir: &PathBuf,
    audio_path: &PathBuf,
) -> Result<String, String> {
    let chunks = crate::transcript_integrity::plan_chunks(audio_path)?;
    crate::debug_log::append(
        app_dir,
        &format!(
            "isolated ASR chunks={} window_secs={}",
            chunks.len(),
            crate::transcript_integrity::ISOLATED_CHUNK_SECS
        ),
    );
    let mut accepted = Vec::new();
    let mut dropped = 0_u32;
    for chunk in &chunks {
        if settings.engine == "local"
            && crate::audio::native_engine_needs_min_duration(&settings.local_engine)
            && chunk != audio_path
        {
            let _ = crate::audio::pad_pcm16_mono_wav(chunk, crate::audio::NATIVE_ASR_MIN_SAMPLES);
        }
        let raw = transcribe_one_chunk(app, settings, app_dir, chunk).await?;
        match crate::transcript_integrity::accept_asr_or_fail_closed(&raw) {
            Some(text) => accepted.push(text),
            None => {
                dropped += 1;
                crate::debug_log::append(app_dir, "dropped degraded ASR chunk (fail closed)");
            }
        }
    }
    crate::transcript_integrity::finish_chunks(audio_path, &chunks);
    if dropped > 0 {
        crate::debug_log::append(
            app_dir,
            &format!("isolated ASR dropped {dropped} garbage chunk(s)"),
        );
    }
    Ok(accepted.join(" "))
}

async fn transcribe_one_chunk(
    app: &AppHandle,
    settings: &Settings,
    app_dir: &PathBuf,
    audio_path: &PathBuf,
) -> Result<String, String> {
    match settings.engine.as_str() {
        "local" => {
            crate::debug_log::append(
                app_dir,
                &format!("local transcription start engine={}", settings.local_engine),
            );
            println!(
                "[Mabel] Local transcription starting ({})",
                settings.local_engine
            );
            transcribe_native::transcribe_local_engine(app, app_dir, audio_path, settings).await
        }
        "cloud" => {
            crate::debug_log::append(app_dir, "cloud transcription start");
            println!("[Mabel] Cloud transcription starting");
            let key = crate::secrets::get_groq_key().map_err(|e| {
                crate::debug_log::append(app_dir, &format!("cloud key read failed: {e}"));
                e
            })?;
            transcribe_groq::transcribe_groq(&key, audio_path, &settings.whisper_language).await
        }
        other => Err(format!("Unknown engine: {other}")),
    }
}

fn ensure_engine_ready(settings: &Settings, app_dir: &PathBuf) -> Result<(), UserError> {
    match settings.engine.as_str() {
        "local" => {
            let engine =
                local_engine::validate(&settings.local_engine).map_err(UserError::generic)?;
            match engine {
                local_engine::PARAKEET | local_engine::WHISPERKIT => {
                    if !transcribe_native::engine_ready(engine, &settings.whisper_language, app_dir)
                    {
                        let label = if engine == local_engine::PARAKEET {
                            "Parakeet"
                        } else {
                            "WhisperKit"
                        };
                        return Err(dictation_error::model_missing(label));
                    }
                }
                local_engine::WHISPER_CPP => {
                    if !local_engine::whisper_cpp_sidecar_compiled() {
                        return Err(dictation_error::whisper_cpp_excluded());
                    }
                    let model_file = crate::transcribe_local::model_filename(
                        &settings.whisper_model,
                        &settings.whisper_language,
                    )
                    .map_err(UserError::generic)?;
                    if !app_dir.join(model_file).exists() {
                        return Err(dictation_error::model_missing("whisper.cpp"));
                    }
                }
                other => {
                    return Err(UserError::generic(format!("Unknown local engine: {other}")));
                }
            }
        }
        "cloud" => match crate::secrets::get_groq_key() {
            Ok(_) => {}
            Err(e) if e.to_lowercase().contains("keychain") => {
                return Err(dictation_error::cloud_unavailable(&e));
            }
            Err(_) => return Err(dictation_error::cloud_key_missing()),
        },
        other => return Err(UserError::generic(format!("Unknown engine: {other}"))),
    }
    Ok(())
}

fn is_cloud_auth_or_keychain_error(lower: &str) -> bool {
    lower.contains("keychain")
        || lower.contains("groq api key")
        || lower.contains("api key not set")
        || lower.contains("groq api error")
        || lower.contains("groq api request failed")
}

fn classify_pipeline_error(err: &str) -> UserError {
    let lower = err.to_lowercase();
    // Auth / keychain must not look like a clean empty recognition.
    if is_cloud_auth_or_keychain_error(&lower) {
        return dictation_error::cloud_unavailable(err);
    }
    if err.contains(&dictation_error::nothing_recognized().message)
        || lower.contains("returned no text")
        || lower.contains("nothing was recognized")
    {
        return dictation_error::nothing_recognized();
    }
    if lower.contains("paste")
        || lower.contains("system events")
        || lower.contains("applescript")
        || lower.contains("accessibility")
    {
        return dictation_error::paste_failed(err);
    }
    if lower.contains("invalid audio")
        || lower.contains("too short")
        || lower.contains("invalidaudiodata")
    {
        return UserError::new(
            dictation_error::TITLE_CAPTURE,
            "Recording was too short for the on-device model. Hold the hotkey, speak a full phrase, then stop.",
        );
    }
    // Engine names appear on every native failure ("Parakeet transcribe
    // failed: …"). Only treat actual readiness failures as Model not ready.
    if lower.contains("not downloaded")
        || lower.contains("not linked")
        || lower.contains("model not found")
        || lower.contains("model is not")
        || lower.contains("is not in this flavor")
        || lower.contains("switch the local engine")
    {
        let title_engine = if lower.contains("whisperkit") {
            "WhisperKit"
        } else if lower.contains("whisper.cpp") {
            "whisper.cpp"
        } else {
            "Parakeet"
        };
        return dictation_error::model_missing(title_engine);
    }
    if lower.contains("microphone") || lower.contains("no audio") || lower.contains("silence") {
        return UserError::new(dictation_error::TITLE_CAPTURE, err);
    }
    UserError::generic(err)
}
