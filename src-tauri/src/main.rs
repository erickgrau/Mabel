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
    // Wrapped in Arc so background tasks (the companion scheduler) can hold a
    // shared reference and read the latest settings on each tick.
    settings: Arc<Mutex<Settings>>,
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

/// docs/whatsnew.md is bundled into the binary at compile time so the popup
/// never depends on the file being present at runtime. Updated each release
/// (rule lives in Claude memory `feedback_release_changelog.md`).
const WHATSNEW_MD: &str = include_str!("../../docs/whatsnew.md");

#[derive(serde::Serialize)]
struct WhatsNewEntry {
    version: String,
    body: String,
}

/// Returns the changelog entry for the running version, if one exists.
/// Frontend uses this to populate the "What's New" popup that fires on the
/// first launch after an update.
#[tauri::command]
fn get_whats_new() -> Option<WhatsNewEntry> {
    let target_header = format!("## v{}", mabel_lib::MABEL_VERSION);
    let mut lines = WHATSNEW_MD.lines();
    while let Some(line) = lines.next() {
        if line.trim_start().starts_with(&target_header) {
            // Capture from the line after the header until the next "## v"
            // header or end of file.
            let mut body = String::new();
            for next in &mut lines {
                if next.trim_start().starts_with("## v") {
                    break;
                }
                body.push_str(next);
                body.push('\n');
            }
            return Some(WhatsNewEntry {
                version: mabel_lib::MABEL_VERSION.to_string(),
                body: body.trim().to_string(),
            });
        }
    }
    None
}

#[tauri::command]
fn mark_version_seen(state: State<AppState>) -> Result<(), String> {
    let mut held = state.settings.lock().unwrap();
    held.last_seen_version = mabel_lib::MABEL_VERSION.to_string();
    held.save(&state.app_dir)
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
    // Cheap and read-only — no keychain access, no disk writes. This command
    // gets called on every recording state poll, so any side effect here will
    // flap the user (e.g. dev builds re-prompt the keychain because their
    // signature changes per rebuild). The keychain reconciliation lives in
    // `reconcile_groq_keychain` instead, called only when the Settings pane
    // opens.
    state.settings.lock().unwrap().clone()
}

/// One-shot probe of the macOS keychain for a stored Groq key. If found and not
/// already reflected in the on-disk settings, flip the configured flag and
/// persist. Called only when the Settings panel opens, so the keychain prompt
/// happens at a moment the user expects (not on every dictation).
#[tauri::command]
fn reconcile_groq_keychain(state: State<AppState>) -> bool {
    let already = {
        let s = state.settings.lock().unwrap();
        s.groq_key_configured
    };
    if already {
        return true;
    }
    if mabel_lib::secrets::has_groq_key() {
        let mut held = state.settings.lock().unwrap();
        held.groq_key_configured = true;
        let _ = held.save(&state.app_dir);
        true
    } else {
        false
    }
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
fn check_model_downloaded(
    state: State<AppState>,
    model_size: String,
    language: Option<String>,
) -> bool {
    let lang = language.unwrap_or_else(|| "multi".to_string());
    match transcribe_local::model_filename(&model_size, &lang) {
        Ok(model_file) => state.app_dir.join(&model_file).exists(),
        Err(_) => false,
    }
}

#[tauri::command]
async fn download_model(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    model_size: String,
    language: Option<String>,
) -> Result<(), String> {
    let lang = language.unwrap_or_else(|| "multi".to_string());
    let url = transcribe_local::model_download_url(&model_size, &lang)?;
    let model_file = transcribe_local::model_filename(&model_size, &lang)?;
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
fn llm_runtime_available() -> bool {
    mabel_lib::llm::runtime_available()
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

/// Toggle one companion visit. If a visit is currently in flight, cancel it
/// (cat parks off-screen). Otherwise start a new one. Used by the Settings
/// "Show now" button so repeat clicks don't stack visits.
#[tauri::command]
async fn companion_visit_now(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    if mabel_lib::companion::is_visiting() {
        mabel_lib::companion::cancel_visit();
        return Ok(());
    }
    let snapshot = state.settings.lock().unwrap().clone();
    mabel_lib::companion::run_visit(&app, &snapshot).await;
    Ok(())
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
    mabel_lib::debug_log::append(
        &state.app_dir,
        &format!("do_toggle_recording called, state={:?}", current_state),
    );
    println!("[Mabel] do_toggle_recording called, state={:?}", current_state);
    match current_state {
        RecordingState::Ready => {
            let (mic, settings) = {
                let s = state.settings.lock().unwrap();
                (s.microphone.clone(), s.clone())
            };
            mabel_lib::debug_log::append(
                &state.app_dir,
                &format!(
                    "start_recording mic={} engine={} model={} lang={}",
                    mic, settings.engine, settings.whisper_model, settings.whisper_language
                ),
            );
            println!(
                "[Mabel] Starting recording (mic={}, engine={}, model={}, lang={})",
                mic, settings.engine, settings.whisper_model, settings.whisper_language
            );
            state.recorder.start_recording(app, &mic, &settings, &state.app_dir)?;
            mabel_lib::debug_log::append(&state.app_dir, "recording started successfully");
            println!("[Mabel] Recording started successfully");
            Ok("recording".to_string())
        }
        RecordingState::Recording => {
            let settings = state.settings.lock().unwrap().clone();
            mabel_lib::debug_log::append(&state.app_dir, "stop requested, beginning transcription");
            println!("[Mabel] Stopping recording and starting transcription");
            let result = state
                .recorder
                .stop_and_transcribe(app, &settings, &state.app_dir)
                .await?;
            mabel_lib::debug_log::append(
                &state.app_dir,
                &format!("stop_and_transcribe completed (chars={})", result.chars().count()),
            );
            println!("[Mabel] stop_and_transcribe completed");
            Ok(result)
        }
        RecordingState::Transcribing => {
            mabel_lib::debug_log::append(&state.app_dir, "toggle ignored: currently transcribing");
            println!("[Mabel] toggle ignored because state is Transcribing");
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
    let settings_handle = Arc::new(Mutex::new(settings.clone()));
    let initial_show_in_dock = settings.show_in_dock;
    let initial_cleanup_mode = settings.cleanup_mode.clone();
    let initial_llm_model = settings.llm_model.clone();
    let initial_companion_enabled = settings.companion_enabled;

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
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(AppState {
            recorder,
            settings: settings_handle.clone(),
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
            llm_runtime_available,
            download_llm_model,
            ensure_llm_started,
            companion_visit_now,
            reconcile_groq_keychain,
            get_whats_new,
            mark_version_seen,
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

            // Create the companion (animated cat) window. Plain transparent
            // always-on-top NSWindow — deliberately NOT converted to NSPanel
            // (the overlay does that for floating-across-Spaces behavior, but
            // for the companion we need a regular window that reliably shows
            // and hides on demand). Starts visible at the builder layer; we
            // immediately hide it in code so it doesn't flash.
            // Companion window. Never hidden — we just park it off-screen when
            // not in a visit. macOS's hide/show dance on transparent windows is
            // flaky (show after hide doesn't always re-render), so we sidestep
            // it entirely by teleporting the window in and out of visible
            // bounds. 1px off the visible region is enough.
            let companion = WebviewWindowBuilder::new(
                app,
                "companion",
                WebviewUrl::App("src/companion.html".into()),
            )
            .title("")
            .inner_size(265.0, 265.0)
            .position(-9999.0, -9999.0)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .shadow(false)
            .build();
            match companion {
                Ok(cw) => {
                    let _ = cw.show();
                    println!("[Mabel] Companion window created (parked off-screen)");
                }
                Err(e) => eprintln!("[Mabel] Failed to create companion window: {}", e),
            }

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

            // Spawn the desktop companion scheduler. The loop runs forever and
            // re-reads settings each tick, so toggling the feature on/off in the
            // UI takes effect on the next interval. We always spawn — the
            // scheduler itself respects companion_enabled.
            let _ = initial_companion_enabled; // kept for symmetry / future use
            mabel_lib::companion::spawn_scheduler(handle.clone(), settings_handle.clone());

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
