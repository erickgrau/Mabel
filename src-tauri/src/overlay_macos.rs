//! macOS-only NSPanel conversion for the floating dictation overlay.
//!
//! Using tauri-nspanel turns the overlay's NSWindow into a non-activating
//! NSPanel — the same primitive Spotlight and Raycast use. That gets us:
//!   - Floats above all other windows including fullscreen apps.
//!   - Stays visible across Spaces without manual re-pinning.
//!   - Never steals key/main status, so the user's focused app keeps focus.

#[cfg(target_os = "macos")]
use tauri_nspanel::{cocoa::appkit::NSWindowCollectionBehavior, WebviewWindowExt};

// NSWindowStyleMaskNonactivatingPanel — keeps the panel from becoming key.
#[cfg(target_os = "macos")]
const NS_WINDOW_STYLE_MASK_NONACTIVATING_PANEL: i32 = 1 << 7;

#[cfg(target_os = "macos")]
pub fn apply_overlay_behavior(window: &tauri::WebviewWindow) {
    let panel = match window.to_panel() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[Mabel] Could not convert overlay to NSPanel: {:?}", e);
            return;
        }
    };

    panel.set_style_mask(NS_WINDOW_STYLE_MASK_NONACTIVATING_PANEL);
    panel.set_collection_behaviour(
        NSWindowCollectionBehavior::NSWindowCollectionBehaviorCanJoinAllSpaces
            | NSWindowCollectionBehavior::NSWindowCollectionBehaviorStationary
            | NSWindowCollectionBehavior::NSWindowCollectionBehaviorFullScreenAuxiliary
            | NSWindowCollectionBehavior::NSWindowCollectionBehaviorIgnoresCycle,
    );
    panel.set_floating_panel(true);
    panel.set_hides_on_deactivate(false);
}

#[cfg(not(target_os = "macos"))]
pub fn apply_overlay_behavior(_window: &tauri::WebviewWindow) {}
