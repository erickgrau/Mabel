pub fn paste_text(text: &str) -> Result<(), String> {
    // Set clipboard (arboard is thread-safe)
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.set_text(text).map_err(|e| e.to_string())?;

    // Small delay to ensure clipboard is set before the keystroke fires
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Simulate Cmd+V via osascript (works from any thread, unlike enigo which
    // calls TSMGetInputSourceProperty requiring the main thread)
    std::process::Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events" to keystroke "v" using command down"#,
        ])
        .output()
        .map_err(|e| format!("Failed to simulate paste: {}", e))?;

    // Give the paste a moment to land in the focused app, then wipe the
    // clipboard so other apps and clipboard managers don't retain the text.
    std::thread::sleep(std::time::Duration::from_millis(150));
    let _ = clipboard.clear();

    Ok(())
}
