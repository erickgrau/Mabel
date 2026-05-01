pub mod settings;
pub mod audio;
pub mod transcribe_local;
pub mod transcribe_groq;
pub mod cleanup;
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
