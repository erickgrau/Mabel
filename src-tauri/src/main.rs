#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_autostart::{ManagerExt as AutostartManagerExt, MacosLauncher};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutEvent, ShortcutState};

use mabel_lib::audio;
use mabel_lib::downloader;
use mabel_lib::llm::LlmServer;
use mabel_lib::recorder::{Recorder, RecordingState};
use mabel_lib::settings::Settings;
use mabel_lib::stats::{StatsStore, StatsSummary};
use mabel_lib::system_ui;
use mabel_lib::transcribe_local;

struct AppState {
    recorder: Recorder,
    settings: Mutex<Settings>,
    app_dir: PathBuf,
    stats: Arc<StatsStore>,
    llm_server: Arc<LlmServer>,
}

#[derive(serde::Serialize)]
struct VersionInfo {
    version: &'static str,
    #[serde(rename = "gitHash")]
    git_hash: &'static str,
    dirty: bool,
}

#[tauri::command]
fn get_version() -> VersionInfo {
    VersionInfo {
        version: mabel_lib::MABEL_VERSION,
        git_hash: mabel_lib::MABEL_GIT_HASH,
        dirty: mabel_lib::MABEL_GIT_DIRTY == "1",
    }
}

#[tauri::command]
fn get_stats(state: State<AppState>) -> StatsSummary {
    state.stats.summary()
}

#[tauri::command]
fn set_launch_at_login(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let mgr = app.autolaunch();
    if enabled {
        mgr.enable().map_err(|e| e.to_string())
    } else {
        mgr.disable().map_err(|e| e.to_string())
    }
}

#[tauri::command]
fn set_show_in_dock(app: tauri::AppHandle, show: bool) {
    system_ui::set_dock_visibility(&app, show);
}

#[tauri::command]
fn check_accessibility() -> bool {
    system_ui::is_accessibility_trusted(false)
}

/// Triggers macOS's Accessibility-required system dialog if not yet granted.
/// Returns whether trust was already in place. The dialog has an "Open System
/// Settings" button that takes the user to the right pane with Mabel
/// pre-listed.
#[tauri::command]
fn request_accessibility() -> bool {
    let already_trusted = system_ui::is_accessibility_trusted(false);
    if !already_trusted {
        // Trigger the prompt and also open settings as a belt-and-suspenders.
        system_ui::is_accessibility_trusted(true);
        system_ui::open_accessibility_settings();
    }
    already_trusted
}

/// Fires a benign AppleScript so macOS shows the "Mabel wants to send Apple
/// events to System Events" prompt during setup, not on first paste.
#[tauri::command]
fn request_apple_events_permission() {
    system_ui::prime_apple_events_permission();
}

fn get_app_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("com.mabel.app")
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings {
    let mut s = state.settings.lock().unwrap().clone();
    // Reconcile against the keychain: a previous install may have stored a Groq
    // key, then the config.json got deleted (or the user is on a fresh install
    // of a new version). The key is still in the keychain — surface it so the
    // UI shows "Saved" instead of asking for the key again.
    //
    // This call may trigger the macOS keychain access prompt the first time
    // after install. That's the right moment for it: the user is in Settings,
    // expects keychain interaction, and grants once-and-forever per signature.
    if !s.groq_key_configured && mabel_lib::secrets::has_groq_key() {
        s.groq_key_configured = true;
        // Persist the corrected flag so subsequent loads don't re-probe.
        let mut held = state.settings.lock().unwrap();
        held.groq_key_configured = true;
        let _ = held.save(&state.app_dir);
    }
    s
}

#[tauri::command]
fn save_settings(state: State<AppState>, settings: Settings) -> Result<(), String> {
    settings.save(&state.app_dir)?;
    *state.settings.lock().unwrap() = settings;
    Ok(())
}

#[tauri::command]
fn list_microphones() -> Vec<audio::MicDevice> {
    audio::list_microphones()
}

#[tauri::command]
fn get_recording_state(state: State<AppState>) -> RecordingState {
    state.recorder.get_state()
}

#[tauri::command]
fn check_model_downloaded(state: State<AppState>, model_size: String) -> bool {
    match transcribe_local::model_filename(&model_size) {
        Ok(model_file) => state.app_dir.join(&model_file).exists(),
        Err(_) => false,
    }
}

#[tauri::command]
async fn download_model(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    model_size: String,
) -> Result<(), String> {
    let url = transcribe_local::model_download_url(&model_size)?;
    let model_file = transcribe_local::model_filename(&model_size)?;
    let dest = state.app_dir.join(&model_file);
    downloader::download_model(app, &url, &dest).await
}

#[tauri::command]
fn check_llm_model_downloaded(state: State<AppState>, model: String) -> bool {
    match mabel_lib::llm::model_filename(&model) {
        Ok(name) => state.app_dir.join(&name).exists(),
        Err(_) => false,
    }
}

#[tauri::command]
async fn download_llm_model(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    model: String,
) -> Result<(), String> {
    let url = mabel_lib::llm::model_download_url(&model)?;
    let name = mabel_lib::llm::model_filename(&model)?;
    let dest = state.app_dir.join(&name);
    downloader::download_model(app, &url, &dest).await
}

/// Starts (or confirms running) the llama-server with the configured LLM model.
/// Idempotent: if already running with the right model, returns immediately.
/// The frontend can call this when the user enables LLM cleanup so the first
/// dictation doesn't pay the cold-start cost.
#[tauri::command]
async fn ensure_llm_started(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let (model, app_dir, server) = {
        let settings = state.settings.lock().unwrap();
        let model = settings.llm_model.clone();
        (model, state.app_dir.clone(), state.llm_server.clone())
    };
    let name = mabel_lib::llm::model_filename(&model)?;
    let path = app_dir.join(&name);
    server.start(&app, &model, &path).await
}

#[tauri::command]
async fn toggle_recording(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    do_toggle_recording(&app, &state).await
}

#[tauri::command]
fn update_hotkey(
    app: tauri::AppHandle,
    state: State<AppState>,
    hotkey: String,
) -> Result<(), String> {
    let trimmed = hotkey.trim().to_string();
    if trimmed.is_empty() {
        return Err("Hotkey cannot be empty".to_string());
    }

    let old_hotkey = state.settings.lock().unwrap().hotkey.clone();
    if trimmed == old_hotkey {
        return Ok(());
    }

    // Unregister the old binding before attempting the new one. If the old
    // hotkey was never successfully registered (e.g. on first launch with a
    // bogus value), unregister will fail silently and we proceed.
    let _ = app.global_shortcut().unregister(old_hotkey.as_str());

    match app
        .global_shortcut()
        .on_shortcut(trimmed.as_str(), build_shortcut_handler(app.clone()))
    {
        Ok(_) => {
            let mut settings = state.settings.lock().unwrap();
            settings.hotkey = trimmed;
            settings.save(&state.app_dir)?;
            Ok(())
        }
        Err(e) => {
            // Restore the old binding so the app stays functional.
            let _ = app
                .global_shortcut()
                .on_shortcut(old_hotkey.as_str(), build_shortcut_handler(app.clone()));
            Err(format!("Could not register that combination: {}", e))
        }
    }
}

fn build_shortcut_handler(
    handle: tauri::AppHandle,
) -> impl Fn(&tauri::AppHandle, &tauri_plugin_global_shortcut::Shortcut, ShortcutEvent)
       + Send
       + Sync
       + 'static {
    move |_app, shortcut, event| {
        println!("[Mabel] Hotkey event: {:?} state={:?}", shortcut, event.state);
        let handle = handle.clone();
        let state = handle.state::<AppState>();
        let mode = state.settings.lock().unwrap().recording_mode.clone();
        println!("[Mabel] Recording mode: {}", mode);

        match event.state {
            ShortcutState::Pressed => {
                tauri::async_runtime::spawn(async move {
                    let state = handle.state::<AppState>();
                    match mode.as_str() {
                        "toggle" => {
                            println!("[Mabel] Toggle mode: calling do_toggle_recording");
                            match do_toggle_recording(&handle, state.inner()).await {
                                Ok(_) => println!("[Mabel] Toggle complete"),
                                Err(e) => eprintln!("[Mabel] Toggle error: {}", e),
                            }
                        }
                        "push-to-talk" => {
                            let current = state.recorder.get_state();
                            println!("[Mabel] PTT mode, current state: {:?}", current);
                            if current == RecordingState::Ready {
                                let (mic, settings) = {
                                    let s = state.settings.lock().unwrap();
                                    (s.microphone.clone(), s.clone())
                                };
                                match state.recorder.start_recording(&handle, &mic, &settings, &state.app_dir) {
                                    Ok(_) => println!("[Mabel] Recording started"),
                                    Err(e) => eprintln!("[Mabel] Start recording error: {}", e),
                                }
                            }
                        }
                        _ => {}
                    }
                });
            }
            ShortcutState::Released => {
                if mode == "push-to-talk" {
                    tauri::async_runtime::spawn(async move {
                        let state = handle.state::<AppState>();
                        let current = state.recorder.get_state();
                        if current == RecordingState::Recording {
                            let settings = state.settings.lock().unwrap().clone();
                            match state
                                .recorder
                                .stop_and_transcribe(&handle, &settings, &state.app_dir)
                                .await
                            {
                                Ok(_) => println!("[Mabel] Transcription complete"),
                                Err(e) => eprintln!("[Mabel] Transcription error: {}", e),
                            }
                        }
                    });
                }
            }
        }
    }
}

/// Shared logic for toggle recording, used by both the Tauri command and hotkey handler.
async fn do_toggle_recording(
    app: &tauri::AppHandle,
    state: &AppState,
) -> Result<String, String> {
    let current_state = state.recorder.get_state();
    match current_state {
        RecordingState::Ready => {
            let (mic, settings) = {
                let s = state.settings.lock().unwrap();
                (s.microphone.clone(), s.clone())
            };
            state.recorder.start_recording(app, &mic, &settings, &state.app_dir)?;
            Ok("recording".to_string())
        }
        RecordingState::Recording => {
            let settings = state.settings.lock().unwrap().clone();
            let result = state
                .recorder
                .stop_and_transcribe(app, &settings, &state.app_dir)
                .await?;
            Ok(result)
        }
        RecordingState::Transcribing => {
            Err("Currently transcribing, please wait".to_string())
        }
    }
}

fn main() {
    let app_dir = get_app_dir();
    let settings = Settings::load(&app_dir);
    let initial_hotkey = settings.hotkey.clone();

    let stats = Arc::new(StatsStore::load(&app_dir));
    let recorder = Recorder::new(stats.clone());
    let llm_server = Arc::new(LlmServer::new());
    let initial_show_in_dock = settings.show_in_dock;
    let initial_cleanup_mode = settings.cleanup_mode.clone();
    let initial_llm_model = settings.llm_model.clone();

    tauri::Builder::default()
        // Single-instance MUST be the first plugin registered. When a second
        // copy launches (e.g. user double-clicks the dock icon while the
        // LaunchAgent already has Mabel running, or a dev build starts on top
        // of the installed one) it exits immediately and the original instance
        // gets the callback. Two instances would otherwise fight over the
        // global hotkey and the shared config dir, which silently breaks paste.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(overlay) = app.get_webview_window("overlay") {
                let _ = overlay.show();
            }
        }))
        .plugin(tauri_nspanel::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            recorder,
            settings: Mutex::new(settings),
            app_dir: app_dir.clone(),
            stats,
            llm_server: llm_server.clone(),
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            list_microphones,
            get_recording_state,
            check_model_downloaded,
            download_model,
            toggle_recording,
            update_hotkey,
            get_version,
            get_stats,
            set_launch_at_login,
            set_show_in_dock,
            check_accessibility,
            request_accessibility,
            request_apple_events_permission,
            check_llm_model_downloaded,
            download_llm_model,
            ensure_llm_started,
        ])
        .setup(move |app| {
            // Create the overlay window (small mic icon, top-right, always on top)
            // Default position: top center of the primary screen.
            let monitor = app.primary_monitor().ok().flatten();
            let overlay_w: f64 = 360.0;
            let (x, y) = if let Some(m) = monitor {
                let size = m.size();
                let scale = m.scale_factor();
                let logical_w = size.width as f64 / scale;
                (((logical_w - overlay_w) / 2.0) as i32, 12_i32)
            } else {
                (480, 12)
            };

            let overlay = WebviewWindowBuilder::new(
                app,
                "overlay",
                WebviewUrl::App("src/overlay.html".into()),
            )
            .title("")
            .inner_size(360.0, 60.0)
            .position(x as f64, y as f64)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .shadow(false)
            .build();

            match overlay {
                Ok(w) => {
                    // Don't call Tauri's set_visible_on_all_workspaces; it dispatches
                    // an async task on the main thread that can clobber our
                    // collectionBehavior write. Set every NSWindow flag we need
                    // directly via objc, synchronously, on the main thread.
                    // Defensive: a panic in objc-land here would propagate
                    // into AppKit's did_finish_launching and abort the app.
                    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        mabel_lib::overlay_macos::apply_overlay_behavior(&w);
                    }));
                    if result.is_err() {
                        eprintln!("[Mabel] apply_overlay_behavior panicked; overlay behavior not applied");
                    }
                    println!("[Mabel] Overlay window created");
                }
                Err(e) => eprintln!("[Mabel] Failed to create overlay: {}", e),
            }

            let handle = app.handle().clone();

            // Apply persisted dock visibility preference.
            system_ui::set_dock_visibility(&handle, initial_show_in_dock);

            println!("[Mabel] Registering global shortcut: {}", initial_hotkey);

            match app.global_shortcut().on_shortcut(
                initial_hotkey.as_str(),
                build_shortcut_handler(handle.clone()),
            ) {
                Ok(_) => println!("[Mabel] Global shortcut registered successfully"),
                Err(e) => eprintln!("[Mabel] ERROR: Failed to register global shortcut: {}", e),
            }

            // If the user has LLM cleanup configured and the model is on disk,
            // warm the server now so the first dictation doesn't block on a
            // 1–3s cold start. Best effort only — failure here just means the
            // first cleanup pays the load cost (or falls back to rules).
            if initial_cleanup_mode == "llm" {
                if let Ok(name) = mabel_lib::llm::model_filename(&initial_llm_model) {
                    let model_path = app_dir.join(&name);
                    if model_path.exists() {
                        let server = llm_server.clone();
                        let model = initial_llm_model.clone();
                        let warm_handle = handle.clone();
                        tauri::async_runtime::spawn(async move {
                            if let Err(e) = server.start(&warm_handle, &model, &model_path).await {
                                eprintln!("[Mabel] LLM warm-start failed: {}", e);
                            }
                        });
                    }
                }
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // Stop the LLM server when the main window closes (full app exit).
            // Tauri kills sidecars on app exit anyway, but doing this explicitly
            // avoids any race where the killed process holds the port and the
            // next launch can't bind.
            if matches!(event, tauri::WindowEvent::Destroyed) {
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    state.llm_server.stop();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

