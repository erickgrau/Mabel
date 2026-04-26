pub fn paste_text(text: &str) -> Result<(), String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.set_text(text).map_err(|e| e.to_string())?;

    std::thread::sleep(std::time::Duration::from_millis(50));

    std::process::Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events" to keystroke "v" using command down"#,
        ])
        .output()
        .map_err(|e| format!("Failed to simulate paste: {}", e))?;

    std::thread::sleep(std::time::Duration::from_millis(150));
    let _ = clipboard.clear();

    Ok(())
}

pub fn press_return() -> Result<(), String> {
    std::process::Command::new("osascript")
        .args([
            "-e",
            r#"tell application "System Events" to key code 36"#,
        ])
        .output()
        .map_err(|e| format!("Failed to press Return: {}", e))?;
    Ok(())
}

/// If `press_enter` is on and the transcription ends with a "press enter" /
/// "new line" command phrase, strip the phrase and return (cleaned_text, true).
/// Otherwise returns (text_unchanged, false).
pub fn extract_press_enter_command(text: &str, enabled: bool) -> (String, bool) {
    if !enabled {
        return (text.to_string(), false);
    }
    let trimmed = text.trim_end_matches(|c: char| c.is_whitespace() || matches!(c, '.' | '!' | '?' | ','));
    let lower = trimmed.to_lowercase();
    for phrase in ["press enter", "press return", "new line", "newline"] {
        if let Some(idx) = lower.rfind(phrase) {
            // Phrase must be at the end (allowing for trailing punctuation we already stripped).
            if idx + phrase.len() == lower.len() {
                let cleaned = trimmed[..idx].trim_end().to_string();
                return (cleaned, true);
            }
        }
    }
    (text.to_string(), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_press_enter() {
        let (out, hit) = extract_press_enter_command("send the email press enter", true);
        assert!(hit);
        assert_eq!(out, "send the email");
    }

    #[test]
    fn handles_trailing_punctuation() {
        let (out, hit) = extract_press_enter_command("ok press enter.", true);
        assert!(hit);
        assert_eq!(out, "ok");
    }

    #[test]
    fn ignores_when_disabled() {
        let (out, hit) = extract_press_enter_command("hello press enter", false);
        assert!(!hit);
        assert_eq!(out, "hello press enter");
    }

    #[test]
    fn ignores_phrase_in_middle() {
        let (out, hit) = extract_press_enter_command("press enter to confirm please", true);
        assert!(!hit);
        assert_eq!(out, "press enter to confirm please");
    }
}
