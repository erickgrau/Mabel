#[allow(unused_imports)]
use std::ffi::{CStr, CString};
#[allow(unused_imports)]
use std::os::raw::{c_char, c_void};
use std::path::Path;
#[allow(dead_code)]
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

use crate::downloader::DownloadProgress;
use crate::local_engine;

#[cfg(all(target_os = "macos", mabel_native_asr))]
mod ffi {
    use super::*;

    pub type ProgressCb = extern "C" fn(f64, *mut c_void);

    unsafe extern "C" {
        pub fn mabel_asr_parakeet_ready(version: *const c_char) -> i32;
        pub fn mabel_asr_parakeet_download(
            version: *const c_char,
            cb: Option<ProgressCb>,
            user: *mut c_void,
        ) -> i32;
        pub fn mabel_asr_parakeet_transcribe(
            version: *const c_char,
            wav_path: *const c_char,
            language: *const c_char,
            out_text: *mut *mut c_char,
        ) -> i32;
        pub fn mabel_asr_whisperkit_ready(cache_dir: *const c_char) -> i32;
        pub fn mabel_asr_whisperkit_download(
            cache_dir: *const c_char,
            cb: Option<ProgressCb>,
            user: *mut c_void,
        ) -> i32;
        pub fn mabel_asr_whisperkit_transcribe(
            cache_dir: *const c_char,
            wav_path: *const c_char,
            language: *const c_char,
            out_text: *mut *mut c_char,
        ) -> i32;
        pub fn mabel_asr_string_free(s: *mut c_char);
        pub fn mabel_asr_last_error() -> *const c_char;
    }

    pub fn last_error() -> String {
        unsafe {
            let ptr = mabel_asr_last_error();
            if ptr.is_null() {
                "native ASR error".to_string()
            } else {
                CStr::from_ptr(ptr).to_string_lossy().into_owned()
            }
        }
    }

    pub fn take_string(ptr: *mut c_char) -> String {
        if ptr.is_null() {
            return String::new();
        }
        unsafe {
            let text = CStr::from_ptr(ptr).to_string_lossy().into_owned();
            mabel_asr_string_free(ptr);
            text
        }
    }
}

#[allow(dead_code)]
fn macos_only() -> Result<(), String> {
    Err("Parakeet and WhisperKit run in-process via CoreML and are only available on macOS.".into())
}

#[allow(dead_code)]
fn native_unlinked() -> Result<(), String> {
    Err(
        "Native CoreML engines are not linked in this binary. On a Mac with Xcode 16+, run `npm run vendor-asr` and rebuild. The Developer ID DMG can still use whisper.cpp."
            .into(),
    )
}

#[allow(dead_code)]
struct ProgressUser {
    app: AppHandle,
}

#[allow(dead_code)]
extern "C" fn progress_cb(percent: f64, user: *mut c_void) {
    if user.is_null() {
        return;
    }
    let ctx = unsafe { &*(user as *const ProgressUser) };
    let _ = ctx.app.emit(
        "download-progress",
        DownloadProgress {
            downloaded: 0,
            total: 0,
            percent,
        },
    );
}

#[allow(dead_code)]
static TRANSCRIBE_LOCK: Mutex<()> = Mutex::new(());

#[allow(dead_code)]
fn with_progress<F, T>(app: &AppHandle, f: F) -> T
where
    F: FnOnce(*mut c_void) -> T,
{
    let mut ctx = ProgressUser { app: app.clone() };
    f((&mut ctx as *mut ProgressUser) as *mut c_void)
}

pub fn parakeet_ready(language: &str) -> bool {
    #[cfg(all(target_os = "macos", mabel_native_asr))]
    {
        let version = CString::new(local_engine::parakeet_version(language)).unwrap_or_default();
        unsafe { ffi::mabel_asr_parakeet_ready(version.as_ptr()) == 1 }
    }
    #[cfg(not(all(target_os = "macos", mabel_native_asr)))]
    {
        let _ = language;
        false
    }
}

pub fn whisperkit_ready(app_dir: &Path) -> bool {
    #[cfg(all(target_os = "macos", mabel_native_asr))]
    {
        let cache = CString::new(app_dir.to_string_lossy().as_ref()).unwrap_or_default();
        unsafe { ffi::mabel_asr_whisperkit_ready(cache.as_ptr()) == 1 }
    }
    #[cfg(not(all(target_os = "macos", mabel_native_asr)))]
    {
        let _ = app_dir;
        false
    }
}

pub fn engine_ready(engine: &str, language: &str, app_dir: &Path) -> bool {
    match engine {
        local_engine::PARAKEET => parakeet_ready(language),
        local_engine::WHISPERKIT => whisperkit_ready(app_dir),
        _ => false,
    }
}

pub async fn download_parakeet(app: AppHandle, language: &str) -> Result<(), String> {
    #[cfg(all(target_os = "macos", mabel_native_asr))]
    {
        let version = CString::new(local_engine::parakeet_version(language))
            .map_err(|e| e.to_string())?;
        let code = tokio::task::spawn_blocking(move || {
            with_progress(&app, |user| unsafe {
                ffi::mabel_asr_parakeet_download(version.as_ptr(), Some(progress_cb), user)
            })
        })
        .await
        .map_err(|e| e.to_string())?;
        if code != 0 {
            return Err(ffi::last_error());
        }
        Ok(())
    }
    #[cfg(all(target_os = "macos", not(mabel_native_asr)))]
    {
        let _ = (app, language);
        native_unlinked()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, language);
        macos_only()
    }
}

pub async fn download_whisperkit(app: AppHandle, app_dir: &Path) -> Result<(), String> {
    #[cfg(all(target_os = "macos", mabel_native_asr))]
    {
        let cache = CString::new(app_dir.to_string_lossy().as_ref()).map_err(|e| e.to_string())?;
        let code = tokio::task::spawn_blocking(move || {
            with_progress(&app, |user| unsafe {
                ffi::mabel_asr_whisperkit_download(cache.as_ptr(), Some(progress_cb), user)
            })
        })
        .await
        .map_err(|e| e.to_string())?;
        if code != 0 {
            return Err(ffi::last_error());
        }
        Ok(())
    }
    #[cfg(all(target_os = "macos", not(mabel_native_asr)))]
    {
        let _ = (app, app_dir);
        native_unlinked()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, app_dir);
        macos_only()
    }
}

pub async fn transcribe_parakeet(
    wav_path: &Path,
    language: &str,
) -> Result<String, String> {
    #[cfg(all(target_os = "macos", mabel_native_asr))]
    {
        let version = CString::new(local_engine::parakeet_version(language))
            .map_err(|e| e.to_string())?;
        let wav = CString::new(wav_path.to_string_lossy().as_ref()).map_err(|e| e.to_string())?;
        let lang = CString::new(language).map_err(|e| e.to_string())?;
        let text = tokio::task::spawn_blocking(move || {
            let _guard = TRANSCRIBE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let mut out: *mut c_char = std::ptr::null_mut();
            let code = unsafe {
                ffi::mabel_asr_parakeet_transcribe(
                    version.as_ptr(),
                    wav.as_ptr(),
                    lang.as_ptr(),
                    &mut out,
                )
            };
            if code != 0 {
                Err(ffi::last_error())
            } else {
                Ok(ffi::take_string(out))
            }
        })
        .await
        .map_err(|e| e.to_string())??;
        Ok(text)
    }
    #[cfg(all(target_os = "macos", not(mabel_native_asr)))]
    {
        let _ = (wav_path, language);
        native_unlinked()?;
        unreachable!()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (wav_path, language);
        macos_only()?;
        unreachable!()
    }
}

pub async fn transcribe_whisperkit(
    app_dir: &Path,
    wav_path: &Path,
    language: &str,
) -> Result<String, String> {
    #[cfg(all(target_os = "macos", mabel_native_asr))]
    {
        let cache = CString::new(app_dir.to_string_lossy().as_ref()).map_err(|e| e.to_string())?;
        let wav = CString::new(wav_path.to_string_lossy().as_ref()).map_err(|e| e.to_string())?;
        let lang = CString::new(language).map_err(|e| e.to_string())?;
        let text = tokio::task::spawn_blocking(move || {
            let _guard = TRANSCRIBE_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let mut out: *mut c_char = std::ptr::null_mut();
            let code = unsafe {
                ffi::mabel_asr_whisperkit_transcribe(
                    cache.as_ptr(),
                    wav.as_ptr(),
                    lang.as_ptr(),
                    &mut out,
                )
            };
            if code != 0 {
                Err(ffi::last_error())
            } else {
                Ok(ffi::take_string(out))
            }
        })
        .await
        .map_err(|e| e.to_string())??;
        Ok(text)
    }
    #[cfg(all(target_os = "macos", not(mabel_native_asr)))]
    {
        let _ = (app_dir, wav_path, language);
        native_unlinked()?;
        unreachable!()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app_dir, wav_path, language);
        macos_only()?;
        unreachable!()
    }
}

/// Route a local transcription to Parakeet, WhisperKit, or whisper.cpp.
pub async fn transcribe_local_engine(
    app: &AppHandle,
    app_dir: &Path,
    audio_path: &Path,
    settings: &crate::settings::Settings,
) -> Result<String, String> {
    let engine = local_engine::validate(&settings.local_engine)?;
    match engine {
        local_engine::PARAKEET => {
            println!("[Mabel] Local engine=parakeet (in-process CoreML)");
            transcribe_parakeet(audio_path, &settings.whisper_language).await
        }
        local_engine::WHISPERKIT => {
            println!("[Mabel] Local engine=whisperkit large-v3-turbo (in-process CoreML)");
            transcribe_whisperkit(app_dir, audio_path, &settings.whisper_language).await
        }
        local_engine::WHISPER_CPP => {
            if !local_engine::whisper_cpp_sidecar_compiled() {
                return Err(
                    "whisper.cpp is not in this flavor. The Mac App Store build uses Parakeet or WhisperKit. Switch the local engine in Settings."
                        .into(),
                );
            }
            let model_file = crate::transcribe_local::model_filename(
                &settings.whisper_model,
                &settings.whisper_language,
            )?;
            let model_path = app_dir.join(model_file);
            let dictionary = crate::dictionary::effective_terms(&settings.dictionary);
            crate::transcribe_local::transcribe_local(
                app,
                &model_path,
                &audio_path.to_path_buf(),
                &settings.whisper_language,
                &dictionary,
            )
            .await
        }
        _ => Err(format!("Unknown local engine: {}", engine)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn engine_ready_is_false_without_native_link() {
        assert!(!engine_ready(local_engine::PARAKEET, "en", &PathBuf::from("/tmp")));
        assert!(!engine_ready(local_engine::WHISPERKIT, "en", &PathBuf::from("/tmp")));
    }

    #[test]
    fn native_bridge_keeps_asr_session_warm_across_takes() {
        let swift = include_str!("../../native/MabelASR/Sources/MabelASR/MabelASR.swift");
        assert!(
            swift.contains("actor ParakeetWarmSession"),
            "take 2+ must reuse the warm Parakeet manager, not loadModels again"
        );
        assert!(
            !swift.contains("await manager.cleanup()"),
            "must not unload CoreML between dictation takes"
        );
        let transcribe_fn = swift
            .split("@_cdecl(\"mabel_asr_parakeet_transcribe\")")
            .nth(1)
            .and_then(|rest| rest.split("@_cdecl(\"mabel_asr_whisperkit_ready\")").next())
            .expect("parakeet transcribe cdecl");
        assert!(
            transcribe_fn.contains("parakeetWarm.transcribe"),
            "C ABI must call the warm session, not construct AsrManager per take"
        );
        assert!(
            !transcribe_fn.contains("AsrManager(config"),
            "constructing AsrManager in the C ABI is the sticky empty-take bug"
        );
        assert!(
            !transcribe_fn.contains("downloadAndLoad"),
            "downloadAndLoad on every take rebinds the process-wide CoreML cache"
        );
        assert!(
            swift.contains("usePrefillCache = false"),
            "WhisperKit must soft-reset decoder cache so long takes do not invent"
        );
        assert!(
            swift.contains("promptTokens = nil") && swift.contains("prefixTokens = nil"),
            "WhisperKit must not condition a take on leftover previous text"
        );
    }
}
