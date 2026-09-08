pub mod settings;
pub mod audio;
pub mod dictation_error;
pub mod mic_permission;
pub mod local_engine;
pub mod transcribe_local;
pub mod transcribe_native;
pub mod transcribe_groq;
pub mod cleanup;
pub mod polish;
pub mod transcript_integrity;
pub mod stiki;
pub mod connectors;
pub mod llm;
pub mod companion;
pub mod paste;
pub mod recorder;
pub mod downloader;
pub mod secrets;
pub mod stats;
pub mod streaming;
pub mod system_ui;
pub mod overlay_macos;
pub mod debug_log;
pub mod storekit;
pub mod stiki_session;
pub mod storage;
pub mod teams;
pub mod pro_features;
pub mod dictionary;
pub mod snippets;
pub mod style;
pub mod transforms;
pub mod scratchpad;
pub mod insights;
pub mod clipboard_history;
#[cfg(target_os = "macos")]
pub mod pasteboard_macos;

pub const MABEL_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const MABEL_GIT_HASH: &str = env!("MABEL_GIT_HASH");
pub const MABEL_GIT_DIRTY: &str = env!("MABEL_GIT_DIRTY");

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
