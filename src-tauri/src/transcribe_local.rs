use std::path::PathBuf;
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

/// Whisper model sizes we accept. Anything else is rejected before it can
/// flow into a file path or download URL.
const ALLOWED_MODELS: &[&str] = &["small", "medium"];
const ALLOWED_LANGUAGES: &[&str] = &["en", "multi"];

pub fn validate_model_size(size: &str) -> Result<&str, String> {
    if ALLOWED_MODELS.contains(&size) {
        Ok(size)
    } else {
        Err(format!("Invalid model size: {}", size))
    }
}

pub fn validate_language(lang: &str) -> Result<&str, String> {
    if ALLOWED_LANGUAGES.contains(&lang) {
        Ok(lang)
    } else {
        Err(format!("Invalid whisper language: {}", lang))
    }
}

/// Build the `--prompt` argument. Whisper conditions on this when decoding,
/// which improves spelling for proper nouns, acronyms, and jargon. We always
/// include the "Dictation transcript:" steer; user dictionary words are
/// appended as a comma-separated list when present.
fn build_prompt(dictionary: &[String]) -> String {
    let mut p = String::from("Dictation transcript:");
    let words: Vec<&str> = dictionary
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if !words.is_empty() {
        p.push(' ');
        p.push_str(&words.join(", "));
        p.push('.');
    }
    p
}

pub async fn transcribe_local(
    app: &AppHandle,
    model_path: &PathBuf,
    audio_path: &PathBuf,
    dictionary: &[String],
) -> Result<String, String> {
    if !model_path.exists() {
        return Err("Whisper model not found. Please download a model first.".to_string());
    }

    println!("[Mabel] Running whisper.cpp sidecar with model {:?}", model_path);

    let prompt = build_prompt(dictionary);

    // NOTE: We are NOT setting `-t` (thread count) right now. Bumping it to 6
    // caused a regression where streaming chunks came back blank or with the
    // classic "Thanks for watching." Whisper-on-silence hallucination. Letting
    // whisper.cpp pick its own thread count restored normal behavior. If we
    // revisit this, gate it behind a setting and benchmark per-machine.
    let output = app
        .shell()
        .sidecar("whisper-cpp")
        .map_err(|e| format!("Failed to create sidecar command: {}", e))?
        .args([
            "-m",
            model_path.to_str().unwrap(),
            "-f",
            audio_path.to_str().unwrap(),
            "--no-timestamps",
            // Default to English. Auto-detect (-l auto) breaks accuracy on
            // short utterances because Whisper occasionally picks the wrong
            // language from a brief sample and forces English-sounding garbage.
            "-l",
            "en",
            // Default 0.6 threshold. Our VAD already drops silent chunks, so
            // anything that reaches Whisper should plausibly contain speech;
            // we don't need to be permissive here and risk hallucinations.
            "--no-speech-thold",
            "0.6",
            // Suppress non-speech tokens like "(music)" / "[BLANK_AUDIO]".
            "--suppress-nst",
            // Initial prompt. Steers Whisper toward dictation and seeds it
            // with the user's custom dictionary when present.
            "--prompt",
            &prompt,
        ])
        .output()
        .await
        .map_err(|e| format!("Failed to run whisper.cpp: {}", e))?;

    if output.status.code() != Some(0) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("whisper.cpp failed: {}", stderr));
    }

    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    println!("[Mabel DEBUG] Whisper raw output: {:?}", text);
    Ok(text)
}

/// Disk filename for a (size, language) pair. English-only models are
/// the same size on disk but trained harder on English — better accuracy
/// when the user only ever speaks English.
pub fn model_filename(model_size: &str, language: &str) -> Result<String, String> {
    let size = validate_model_size(model_size)?;
    let lang = validate_language(language)?;
    Ok(match lang {
        "en" => format!("ggml-{}.en.bin", size),
        _ => format!("ggml-{}.bin", size),
    })
}

pub fn model_download_url(model_size: &str, language: &str) -> Result<String, String> {
    let name = model_filename(model_size, language)?;
    Ok(format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}",
        name
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_filename_allowed() {
        assert_eq!(model_filename("small", "multi").unwrap(), "ggml-small.bin");
        assert_eq!(model_filename("medium", "multi").unwrap(), "ggml-medium.bin");
        assert_eq!(model_filename("small", "en").unwrap(), "ggml-small.en.bin");
        assert_eq!(model_filename("medium", "en").unwrap(), "ggml-medium.en.bin");
    }

    #[test]
    fn test_model_filename_rejects_unknown() {
        assert!(model_filename("../etc/passwd", "en").is_err());
        assert!(model_filename("large", "en").is_err());
        assert!(model_filename("", "en").is_err());
        assert!(model_filename("small", "fr").is_err());
        assert!(model_filename("small", "../bad").is_err());
    }

    #[test]
    fn test_model_download_url_allowed() {
        assert_eq!(
            model_download_url("small", "multi").unwrap(),
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin"
        );
        assert_eq!(
            model_download_url("medium", "en").unwrap(),
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.en.bin"
        );
    }

    #[test]
    fn test_model_download_url_rejects_unknown() {
        assert!(model_download_url("../../evil", "en").is_err());
        assert!(model_download_url("small", "../bad").is_err());
    }

    #[test]
    fn test_build_prompt_empty_dictionary() {
        let p = build_prompt(&[]);
        assert_eq!(p, "Dictation transcript:");
    }

    #[test]
    fn test_build_prompt_with_dictionary() {
        let words = vec!["Chibitek".to_string(), "Mochi".to_string(), "GGUF".to_string()];
        let p = build_prompt(&words);
        assert_eq!(p, "Dictation transcript: Chibitek, Mochi, GGUF.");
    }

    #[test]
    fn test_build_prompt_skips_blank_entries() {
        let words = vec!["".to_string(), "  ".to_string(), "Mabel".to_string()];
        let p = build_prompt(&words);
        assert_eq!(p, "Dictation transcript: Mabel.");
    }

}
