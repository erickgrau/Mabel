//! Small system-integration helpers: dock visibility, system sounds,
//! Accessibility permission request.

#[cfg(target_os = "macos")]
use objc2::msg_send;
#[cfg(target_os = "macos")]
use objc2::runtime::AnyObject;
#[cfg(target_os = "macos")]
use std::ffi::c_void;

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFDictionaryCreate(
        allocator: *const c_void,
        keys: *const *const c_void,
        values: *const *const c_void,
        num_values: isize,
        key_callbacks: *const c_void,
        value_callbacks: *const c_void,
    ) -> *const c_void;
    fn CFRelease(cf: *const c_void);
    fn CFStringCreateWithCString(
        allocator: *const c_void,
        c_str: *const u8,
        encoding: u32,
    ) -> *const c_void;
    static kCFTypeDictionaryKeyCallBacks: c_void;
    static kCFTypeDictionaryValueCallBacks: c_void;
    static kCFBooleanTrue: *const c_void;
}

/// Checks whether Mabel has Accessibility permission. When `prompt` is true
/// and the app is not trusted, macOS shows the standard Accessibility-required
/// system dialog with a button that opens System Settings to the right pane
/// (the app gets added to the Accessibility list automatically).
#[cfg(target_os = "macos")]
pub fn is_accessibility_trusted(prompt: bool) -> bool {
    unsafe {
        if !prompt {
            return AXIsProcessTrustedWithOptions(std::ptr::null());
        }
        let key_cstr = b"AXTrustedCheckOptionPrompt\0".as_ptr();
        // kCFStringEncodingUTF8 = 0x08000100
        let key = CFStringCreateWithCString(std::ptr::null(), key_cstr, 0x08000100);
        if key.is_null() {
            return AXIsProcessTrustedWithOptions(std::ptr::null());
        }
        let keys = [key];
        let values = [kCFBooleanTrue];
        let dict = CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr() as *const *const c_void,
            values.as_ptr() as *const *const c_void,
            1,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        );
        let trusted = AXIsProcessTrustedWithOptions(dict);
        if !dict.is_null() {
            CFRelease(dict);
        }
        CFRelease(key);
        trusted
    }
}

#[cfg(not(target_os = "macos"))]
pub fn is_accessibility_trusted(_prompt: bool) -> bool { true }

/// Fires a no-op AppleScript against System Events so macOS shows the
/// "Mabel wants to send Apple events to System Events" permission prompt.
/// Without this, the prompt only appears the first time we try to paste
/// (Cmd+V via System Events keystroke), which is too late — by then the user
/// has already lost a dictation. We call this during first-run setup so the
/// prompt appears alongside the Accessibility one, before the user dictates.
///
/// Does not block on the prompt: macOS shows it asynchronously. If the user
/// denies, paste will still fail later with a permission error.
pub fn prime_apple_events_permission() {
    let _ = std::process::Command::new("osascript")
        .args(["-e", r#"tell application "System Events" to return true"#])
        .spawn();
}

/// Open System Settings to the Automation privacy pane (where AppleEvents
/// permissions are listed). Useful as a follow-up if the user denied the prompt.
pub fn open_automation_settings() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Automation")
        .spawn();
}

/// Open System Settings to the Accessibility privacy pane.
pub fn open_accessibility_settings() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .spawn();
}

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
