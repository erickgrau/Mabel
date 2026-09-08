use std::path::PathBuf;
use tauri::{AppHandle, Emitter};
use tauri_plugin_shell::ShellExt;

/// Whisper model sizes we accept. Anything else is rejected before it can
/// flow into a file path or download URL.
///
/// `large-v3` is the quantized Q5_0 multilingual checkpoint
/// (`ggml-large-v3-q5_0.bin`, ~1.1 GB). There is no official English-only
/// large-v3 Q5 on the whisper.cpp Hugging Face repo, so en/multi share one file.
const ALLOWED_MODELS: &[&str] = &["small", "medium", "large-v3"];
const ALLOWED_LANGUAGES: &[&str] = &["en", "multi", "auto"];
const VAD_MODEL_FILENAME: &str = "ggml-silero-v6.2.0.bin";
const VAD_MODEL_URL: &str =
    "https://huggingface.co/ggml-org/whisper-vad/resolve/main/ggml-silero-v6.2.0.bin";

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptSegment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    pub confidence: Option<f64>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalTranscription {
    pub text: String,
    pub language: String,
    pub confidence: Option<f64>,
    pub duration_ms: u64,
    pub segments: Vec<TranscriptSegment>,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TranscriptionQuality {
    confidence: f64,
    low_confidence: bool,
}

/// Recommended default for new Apple Silicon installs.
pub fn recommended_model_size() -> &'static str {
    "large-v3"
}

/// Every (size, language) pair the downloader / first-run gate should check.
/// large-v3 maps en and multi to the same file; listing both keeps the UI
/// scan consistent with small/medium.
pub fn known_download_variants() -> &'static [(&'static str, &'static str)] {
    &[
        ("large-v3", "en"),
        ("large-v3", "multi"),
        ("small", "en"),
        ("small", "multi"),
        ("medium", "en"),
        ("medium", "multi"),
    ]
}

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

/// Maps persisted language setting to the whisper.cpp decoder flag.
/// - en -> en (force English decoder)
/// - multi -> auto (allow language auto-detection)
pub fn whisper_decode_language(language: &str) -> Result<&'static str, String> {
    match validate_language(language)? {
        "en" => Ok("en"),
        _ => Ok("auto"),
    }
}

pub fn vad_model_filename() -> &'static str {
    VAD_MODEL_FILENAME
}

pub fn vad_model_url() -> &'static str {
    VAD_MODEL_URL
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

pub async fn transcribe_local_detailed(
    app: &AppHandle,
    model_path: &PathBuf,
    audio_path: &PathBuf,
    whisper_language: &str,
    dictionary: &[String],
) -> Result<LocalTranscription, String> {
    if !model_path.exists() {
        return Err("Whisper model not found. Please download a model first.".to_string());
    }

    println!("[Mabel] Running whisper.cpp sidecar with model {:?}", model_path);

    let prompt = build_prompt(dictionary);
    let decode_language = whisper_decode_language(whisper_language)?;

    let output_stem = std::env::temp_dir().join(format!(
        "mabel-transcript-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
    ));
    let output_json = output_stem.with_extension("json");

    // NOTE: We are NOT setting `-t` (thread count) right now. Bumping it to 6
    // caused a regression where streaming chunks came back blank or with the
    // classic "Thanks for watching." Whisper-on-silence hallucination. Letting
    // whisper.cpp pick its own thread count restored normal behavior. If we
    // revisit this, gate it behind a setting and benchmark per-machine.
    let mut args = vec![
        "-m".to_string(),
        model_path.to_string_lossy().into_owned(),
        "-f".to_string(),
        audio_path.to_string_lossy().into_owned(),
        "--output-json-full".to_string(),
        "--output-file".to_string(),
        output_stem.to_string_lossy().into_owned(),
        "--no-prints".to_string(),
        "-l".to_string(),
        decode_language.to_string(),
        "--no-speech-thold".to_string(),
        "0.6".to_string(),
        "--suppress-nst".to_string(),
        // Isolate each window. Without this, whisper.cpp feeds the previous
        // segment text back in as the next prompt and long toggle takes
        // (~15 min) cascade into invented salad.
        "--no-context".to_string(),
        "--prompt".to_string(),
        prompt,
    ];

    if let Some(parent) = model_path.parent() {
        let vad_path = parent.join(VAD_MODEL_FILENAME);
        if vad_path.exists() {
            args.extend([
                "--vad".to_string(),
                "--vad-model".to_string(),
                vad_path.to_string_lossy().into_owned(),
                "--vad-threshold".to_string(),
                "0.5".to_string(),
                "--vad-min-speech-duration-ms".to_string(),
                "120".to_string(),
                "--vad-min-silence-duration-ms".to_string(),
                "300".to_string(),
                "--vad-speech-pad-ms".to_string(),
                "80".to_string(),
            ]);
        }
    }

    let output = app
        .shell()
        .sidecar("whisper-cpp")
        .map_err(|e| format!("Failed to create sidecar command: {}", e))?
        .args(args)
        .output()
        .await
        .map_err(|e| {
            let _ = std::fs::remove_file(&output_json);
            format!("Failed to run whisper.cpp: {}", e)
        })?;

    if output.status.code() != Some(0) {
        let _ = std::fs::remove_file(&output_json);
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("whisper.cpp failed: {}", stderr));
    }

    let json = std::fs::read_to_string(&output_json)
        .map_err(|e| format!("Whisper did not produce structured output: {}", e));
    let _ = std::fs::remove_file(&output_json);
    let result = parse_whisper_json(&json?)?;

    if let Some(confidence) = result.confidence {
        let _ = app.emit(
            "transcription-quality",
            TranscriptionQuality {
                confidence,
                low_confidence: confidence < 0.55,
            },
        );
    }

    Ok(result)
}

pub async fn transcribe_local(
    app: &AppHandle,
    model_path: &PathBuf,
    audio_path: &PathBuf,
    whisper_language: &str,
    dictionary: &[String],
) -> Result<String, String> {
    Ok(
        transcribe_local_detailed(app, model_path, audio_path, whisper_language, dictionary)
            .await?
            .text,
    )
}

fn parse_whisper_json(raw: &str) -> Result<LocalTranscription, String> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("Invalid Whisper JSON: {}", e))?;
    let language = value
        .pointer("/result/language")
        .and_then(|value| value.as_str())
        .unwrap_or("unknown")
        .to_string();
    let items = value
        .get("transcription")
        .and_then(|value| value.as_array())
        .ok_or_else(|| "Whisper JSON did not contain transcription segments".to_string())?;

    let mut segments = Vec::with_capacity(items.len());
    let mut confidence_sum = 0.0;
    let mut confidence_count = 0_u64;

    for item in items {
        let text = item
            .get("text")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .trim()
            .to_string();
        if text.is_empty() {
            continue;
        }
        let start_ms = item
            .pointer("/offsets/from")
            .and_then(|value| value.as_u64())
            .unwrap_or_default();
        let end_ms = item
            .pointer("/offsets/to")
            .and_then(|value| value.as_u64())
            .unwrap_or(start_ms);
        let probabilities: Vec<f64> = item
            .get("tokens")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .filter(|token| {
                !token
                    .get("text")
                    .and_then(|value| value.as_str())
                    .is_some_and(|text| text.starts_with("<|"))
            })
            .filter_map(|token| token.get("p").and_then(|value| value.as_f64()))
            .collect();
        let confidence = if probabilities.is_empty() {
            None
        } else {
            Some(probabilities.iter().sum::<f64>() / probabilities.len() as f64)
        };
        if crate::transcript_integrity::is_low_confidence(confidence) {
            continue;
        }
        if crate::transcript_integrity::accept_asr_or_fail_closed(&text).is_none() {
            continue;
        }
        if !probabilities.is_empty() {
            confidence_sum += probabilities.iter().sum::<f64>();
            confidence_count += probabilities.len() as u64;
        }
        segments.push(TranscriptSegment {
            start_ms,
            end_ms,
            text,
            confidence,
        });
    }

    let text = segments
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let duration_ms = segments.last().map(|segment| segment.end_ms).unwrap_or_default();
    let confidence =
        (confidence_count > 0).then_some(confidence_sum / confidence_count as f64);

    Ok(LocalTranscription {
        text,
        language,
        confidence,
        duration_ms,
        segments,
    })
}

pub fn transcript_txt(result: &LocalTranscription) -> String {
    format!("{}\n", result.text.trim())
}

pub fn transcript_json(result: &LocalTranscription) -> Result<String, String> {
    serde_json::to_string_pretty(result).map_err(|e| e.to_string())
}

pub fn transcript_srt(result: &LocalTranscription) -> String {
    result
        .segments
        .iter()
        .enumerate()
        .map(|(index, segment)| {
            format!(
                "{}\n{} --> {}\n{}\n",
                index + 1,
                format_timestamp(segment.start_ms, ','),
                format_timestamp(segment.end_ms, ','),
                segment.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn transcript_vtt(result: &LocalTranscription) -> String {
    let cues = result
        .segments
        .iter()
        .map(|segment| {
            format!(
                "{} --> {}\n{}\n",
                format_timestamp(segment.start_ms, '.'),
                format_timestamp(segment.end_ms, '.'),
                segment.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("WEBVTT\n\n{}", cues)
}

fn format_timestamp(milliseconds: u64, separator: char) -> String {
    let hours = milliseconds / 3_600_000;
    let minutes = (milliseconds / 60_000) % 60;
    let seconds = (milliseconds / 1_000) % 60;
    let millis = milliseconds % 1_000;
    format!(
        "{:02}:{:02}:{:02}{}{:03}",
        hours, minutes, seconds, separator, millis
    )
}

/// Disk filename for a (size, language) pair. English-only models are
/// the same size on disk but trained harder on English — better accuracy
/// when the user only ever speaks English. large-v3 Q5 has no `.en`
/// variant, so both language settings share `ggml-large-v3-q5_0.bin`.
pub fn model_filename(model_size: &str, language: &str) -> Result<String, String> {
    let size = validate_model_size(model_size)?;
    let lang = validate_language(language)?;
    Ok(match size {
        "large-v3" => "ggml-large-v3-q5_0.bin".to_string(),
        _ => match lang {
            "en" => format!("ggml-{}.en.bin", size),
            _ => format!("ggml-{}.bin", size),
        },
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
        assert_eq!(
            model_filename("large-v3", "en").unwrap(),
            "ggml-large-v3-q5_0.bin"
        );
        assert_eq!(
            model_filename("large-v3", "multi").unwrap(),
            "ggml-large-v3-q5_0.bin"
        );
    }

    #[test]
    fn test_recommended_model_is_large_v3_q5() {
        assert_eq!(recommended_model_size(), "large-v3");
        assert!(ALLOWED_MODELS.contains(&recommended_model_size()));
        assert!(known_download_variants()
            .iter()
            .any(|(size, _)| *size == "large-v3"));
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
        assert_eq!(
            model_download_url("large-v3", "en").unwrap(),
            "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-q5_0.bin"
        );
    }

    #[test]
    fn test_model_download_url_rejects_unknown() {
        assert!(model_download_url("../../evil", "en").is_err());
        assert!(model_download_url("small", "../bad").is_err());
    }

    #[test]
    fn test_whisper_decode_language_mapping() {
        assert_eq!(whisper_decode_language("en").unwrap(), "en");
        assert_eq!(whisper_decode_language("multi").unwrap(), "auto");
        assert!(whisper_decode_language("fr").is_err());
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

    #[test]
    fn test_structured_transcript_and_exports() {
        let raw = serde_json::json!({
            "result": { "language": "en" },
            "transcription": [
                {
                    "offsets": { "from": 0, "to": 1250 },
                    "text": "Hello there.",
                    "tokens": [{ "p": 0.9 }, { "p": 0.7 }]
                },
                {
                    "offsets": { "from": 1250, "to": 2500 },
                    "text": "Welcome to Mabel.",
                    "tokens": [{ "p": 0.8 }]
                }
            ]
        });
        let transcript = parse_whisper_json(&raw.to_string()).unwrap();

        assert_eq!(transcript.text, "Hello there. Welcome to Mabel.");
        assert_eq!(transcript.language, "en");
        assert_eq!(transcript.duration_ms, 2500);
        assert!((transcript.confidence.unwrap() - 0.8).abs() < f64::EPSILON * 2.0);
        assert!(transcript_srt(&transcript).contains("00:00:01,250 --> 00:00:02,500"));
        assert!(transcript_vtt(&transcript).starts_with("WEBVTT\n\n"));
        assert!(transcript_json(&transcript).unwrap().contains("\"startMs\""));
    }

    #[test]
    fn whisper_cpp_isolates_decoder_context() {
        let src = include_str!("transcribe_local.rs");
        assert!(
            src.contains("\"--no-context\""),
            "long takes must not condition the next window on previous text"
        );
    }

    #[test]
    fn low_confidence_and_silence_hallucination_segments_fail_closed() {
        let raw = serde_json::json!({
            "result": { "language": "en" },
            "transcription": [
                {
                    "offsets": { "from": 0, "to": 1000 },
                    "text": "Save the metrics.",
                    "tokens": [{ "p": 0.9 }]
                },
                {
                    "offsets": { "from": 1000, "to": 2000 },
                    "text": "Holy Send May Burns and Burnity",
                    "tokens": [{ "p": 0.12 }, { "p": 0.08 }]
                },
                {
                    "offsets": { "from": 2000, "to": 2500 },
                    "text": "Thanks for watching.",
                    "tokens": [{ "p": 0.88 }]
                }
            ]
        });
        let transcript = parse_whisper_json(&raw.to_string()).unwrap();
        assert_eq!(transcript.text, "Save the metrics.");
        assert_eq!(transcript.segments.len(), 1);
    }

    #[test]
    fn test_vad_model_points_at_silero_ggml() {
        assert_eq!(vad_model_filename(), "ggml-silero-v6.2.0.bin");
        assert!(vad_model_url().contains("whisper-vad"));
    }
}
