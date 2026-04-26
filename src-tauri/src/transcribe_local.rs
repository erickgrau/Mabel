use std::path::PathBuf;
use tauri::AppHandle;
use tauri_plugin_shell::ShellExt;

/// Whisper model sizes we accept. Anything else is rejected before it can
/// flow into a file path or download URL.
const ALLOWED_MODELS: &[&str] = &["small", "medium"];

pub fn validate_model_size(size: &str) -> Result<&str, String> {
    if ALLOWED_MODELS.contains(&size) {
        Ok(size)
    } else {
        Err(format!("Invalid model size: {}", size))
    }
}

pub async fn transcribe_local(
    app: &AppHandle,
    model_path: &PathBuf,
    audio_path: &PathBuf,
) -> Result<String, String> {
    if !model_path.exists() {
        return Err("Whisper model not found. Please download a model first.".to_string());
    }

    println!("[Mabel] Running whisper.cpp sidecar with model {:?}", model_path);

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
            "-l",
            "en",
            // Default 0.6 threshold. Our VAD already drops silent chunks, so
            // anything that reaches Whisper should plausibly contain speech;
            // we don't need to be permissive here and risk hallucinations.
            "--no-speech-thold",
            "0.6",
            // Suppress non-speech tokens like "(music)" / "[BLANK_AUDIO]".
            "--suppress-nst",
            // Steer Whisper toward dictation.
            "--prompt",
            "Dictation transcript:",
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

pub fn model_filename(model_size: &str) -> Result<String, String> {
    Ok(format!("ggml-{}.bin", validate_model_size(model_size)?))
}

pub fn model_download_url(model_size: &str) -> Result<String, String> {
    Ok(format!(
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-{}.bin",
        validate_model_size(model_size)?
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_filename_allowed() {
        assert_eq!(model_filename("small").unwrap(), "ggml-small.bin");
        assert_eq!(model_filename("medium").unwrap(), "ggml-medium.bin");
    }

    #[test]
    fn test_model_filename_rejects_unknown() {
        assert!(model_filename("../etc/passwd").is_err());
        assert!(model_filename("large").is_err());
        assert!(model_filename("").is_err());
    }

    #[test]
    fn test_model_download_url_allowed() {
        assert_eq!(
            model_download_url("small").unwrap(),
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin"
        );
    }

    #[test]
    fn test_model_download_url_rejects_unknown() {
        assert!(model_download_url("../../evil").is_err());
    }
}
