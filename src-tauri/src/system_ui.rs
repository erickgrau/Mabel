//! Small system-integration helpers: dock visibility, system sounds.

#[cfg(target_os = "macos")]
use objc2::msg_send;
#[cfg(target_os = "macos")]
use objc2::runtime::AnyObject;

/// Play a short macOS system sound. Spawns afplay so the call doesn't block
/// the recording state machine. Errors are intentionally swallowed; sound is
/// cosmetic.
pub fn play_sound(name: &str) {
    let path = format!("/System/Library/Sounds/{}.aiff", name);
    let _ = std::process::Command::new("afplay").arg(&path).spawn();
}

#[cfg(target_os = "macos")]
pub fn set_dock_visibility(app: &tauri::AppHandle, show_in_dock: bool) {
    // NSApplicationActivationPolicyRegular = 0, Accessory = 1.
    let policy: i64 = if show_in_dock { 0 } else { 1 };
    let _ = app.run_on_main_thread(move || {
        unsafe {
            let cls = objc2::class!(NSApplication);
            let app: *mut AnyObject = msg_send![cls, sharedApplication];
            if !app.is_null() {
                let _: () = msg_send![app, setActivationPolicy: policy];
            }
        }
    });
}

#[cfg(not(target_os = "macos"))]
pub fn set_dock_visibility(_app: &tauri::AppHandle, _show_in_dock: bool) {}
