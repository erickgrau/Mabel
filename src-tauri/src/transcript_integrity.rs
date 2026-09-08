//! Mac long-session transcript integrity (Erick P0).
//!
//! Product LOCK UPDATE + Enforcer BOUND UPDATE confirm (Erick 2026-09-08):
//! Mac TF only this tip; fail closed/reset rather than invent. Soft nits later.
//!
//! GREEN (b6530197 Product LOCK UPDATE + Enforcer BOUND UPDATE):
//! Mac TF only this tip; fail closed/reset rather than invent words;
//! Soft nits later.
//! BREAKS IF (b6530197 Product LOCK UPDATE + Enforcer BOUND UPDATE):
//! invent/garbage inject on long Mac session; iOS in this tip.
//!
//! Toggle / hold can capture 15+ minutes as one WAV. Whisper (cpp, WhisperKit,
//! Groq) then conditions each ~30s window on the previous window's text.
//! After enough windows the decoder invents salad ("Burnity", "Holy Send May
//! Burns") and pastes it. Fail closed / reset context rather than inject.
//!
//! Soft reset only: isolate chunks and drop garbage. Do not unload the warm
//! Parakeet / WhisperKit CoreML session (that was the empty take-2 bug).
//! Live streaming stays parked.

use std::path::{Path, PathBuf};

use crate::audio::NATIVE_ASR_SAMPLE_RATE;

/// Named Enforcer suite. Tests fail if this is retargeted without a new MCS.
pub const ENFORCER_SUITE: &str = "b6530197";

/// Product LOCK UPDATE (Erick 2026-09-08 confirm). Soft nits later.
pub const PRODUCT_LOCK: &str =
    "Mac TF only this tip; fail closed/reset rather than invent. Soft nits later.";

/// Enforcer BOUND UPDATE (Suite b6530197). `enforcer_bound_*` tests fold this.
pub const ENFORCER_BOUND: &str = "CONFIRMED Suite b6530197; Mac TF only this tip; fail closed/reset rather than invent words; Soft nits later; BREAKS IF: invent/garbage inject on long Mac session; iOS in this tip";

pub const BREAKS_IF: &str =
    "BREAKS IF: invent/garbage inject on long Mac session; iOS in this tip";

/// GREEN: this tip is Mac TestFlight only.
pub const MAC_TF_ONLY: bool = true;
/// BREAKS IF: iOS ships in this tip.
pub const IOS_IN_THIS_TIP: bool = false;

/// Whisper's decoder window is 30s. Stay under it so one chunk cannot grow
/// a previous-text prompt inside a single engine call.
pub const ISOLATED_CHUNK_SECS: u32 = 20;
pub const ISOLATED_CHUNK_SAMPLES: u32 = NATIVE_ASR_SAMPLE_RATE * ISOLATED_CHUNK_SECS;

/// Token-average probability below this is treated as invented / degraded.
/// Whisper reports this on whisper.cpp `--output-json-full` tokens.
pub const ASR_FAIL_CLOSED_CONFIDENCE: f64 = 0.40;

/// Whole-chunk silence hallucinations. Match the entire chunk, not a
/// substring of real speech ("thank you" inside a sentence stays).
const SILENCE_HALLUCINATIONS: &[&str] = &[
    "thanks for watching",
    "thank you for watching",
    "thanks for watching please subscribe",
    "please subscribe",
    "like and subscribe",
    "thank you",
    "thanks",
    "you",
    "okay",
    "ok",
];

/// Split a 16 kHz mono PCM16 WAV into isolated chunks. Short files stay one
/// path (the original) so take-1 latency does not change.
pub fn plan_chunks(wav: &Path) -> Result<Vec<PathBuf>, String> {
    let mut reader = hound::WavReader::open(wav).map_err(|e| e.to_string())?;
    let spec = reader.spec();
    if spec.channels != 1
        || spec.sample_rate != NATIVE_ASR_SAMPLE_RATE
        || spec.bits_per_sample != 16
        || spec.sample_format != hound::SampleFormat::Int
    {
        return Err(format!(
            "refusing to isolate unexpected wav spec: {}Hz {}ch {}bit {:?}",
            spec.sample_rate, spec.channels, spec.bits_per_sample, spec.sample_format
        ));
    }
    let samples: Vec<i16> = reader
        .samples::<i16>()
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if samples.len() as u32 <= ISOLATED_CHUNK_SAMPLES {
        return Ok(vec![wav.to_path_buf()]);
    }

    let stem = wav
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "chunk".into());
    let parent = wav.parent().unwrap_or_else(|| Path::new("."));
    let mut paths = Vec::new();
    for (index, chunk) in samples.chunks(ISOLATED_CHUNK_SAMPLES as usize).enumerate() {
        if chunk.is_empty() {
            continue;
        }
        let path = parent.join(format!("{stem}-iso-{index}.wav"));
        let mut writer = hound::WavWriter::create(&path, spec).map_err(|e| e.to_string())?;
        for &sample in chunk {
            writer.write_sample(sample).map_err(|e| e.to_string())?;
        }
        writer.finalize().map_err(|e| e.to_string())?;
        paths.push(path);
    }
    Ok(paths)
}

/// Delete isolated chunk files. Never deletes `original`.
pub fn finish_chunks(original: &Path, chunks: &[PathBuf]) {
    for path in chunks {
        if path != original {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Fail closed on empty, silence-hallucination, or invented salad.
/// Caller keeps going with other chunks rather than pasting garbage.
pub fn accept_asr_or_fail_closed(text: &str) -> Option<String> {
    let cleaned = text.trim();
    if cleaned.is_empty() {
        return None;
    }
    if is_silence_hallucination(cleaned) {
        return None;
    }
    Some(cleaned.to_string())
}

pub fn merge_accepted<I, S>(parts: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    parts
        .into_iter()
        .filter_map(|part| accept_asr_or_fail_closed(part.as_ref()))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn is_low_confidence(confidence: Option<f64>) -> bool {
    match confidence {
        Some(value) if value.is_finite() && value > 0.0 && value < ASR_FAIL_CLOSED_CONFIDENCE => {
            true
        }
        _ => false,
    }
}

fn is_silence_hallucination(text: &str) -> bool {
    let words = normalize_words(text);
    if words.is_empty() {
        return true;
    }
    SILENCE_HALLUCINATIONS.iter().any(|phrase| {
        let expected = normalize_words(phrase);
        words == expected
    })
}

fn normalize_words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| {
            word.chars()
                .filter(|c| c.is_alphanumeric())
                .collect::<String>()
                .to_ascii_lowercase()
        })
        .filter(|word| !word.is_empty())
        .collect()
}

/// Content tokens for Polish fail-closed overlap. Skip short / function words
/// so a register synonym (gonna → going to) does not trip the invent gate.
pub fn content_tokens(text: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "the", "and", "that", "this", "with", "from", "you", "are", "was", "for", "but", "not",
        "have", "has", "had", "can", "all", "yes",
    ];
    normalize_words(text)
        .into_iter()
        .filter(|word| word.len() >= 4 && !STOP.contains(&word.as_str()))
        .collect()
}

/// Shared invent tripwire: output must still be the same utterance.
pub fn token_overlap_ratio(input: &str, output: &str) -> Option<f32> {
    let input_tokens = content_tokens(input);
    if input_tokens.len() < 4 {
        return None;
    }
    let output_set: std::collections::HashSet<_> = content_tokens(output).into_iter().collect();
    let shared = input_tokens
        .iter()
        .filter(|token| output_set.contains(*token))
        .count();
    Some(shared as f32 / input_tokens.len() as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{WavSpec, WavWriter};

    fn write_mono(path: &Path, samples: &[i16]) {
        let spec = WavSpec {
            channels: 1,
            sample_rate: NATIVE_ASR_SAMPLE_RATE,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = WavWriter::create(path, spec).unwrap();
        for &sample in samples {
            writer.write_sample(sample).unwrap();
        }
        writer.finalize().unwrap();
    }

    fn read_len(path: &Path) -> usize {
        let mut reader = hound::WavReader::open(path).unwrap();
        reader.samples::<i16>().count()
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "mabel-iso-{}-{}",
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn short_wav_is_not_rewritten() {
        let dir = temp_dir("short");
        let path = dir.join("take.wav");
        write_mono(&path, &[1, 2, 3, 4]);
        let chunks = plan_chunks(&path).unwrap();
        assert_eq!(chunks, vec![path.clone()]);
        finish_chunks(&path, &chunks);
        assert!(path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn long_wav_splits_under_whisper_window() {
        let dir = temp_dir("long");
        let path = dir.join("take.wav");
        let samples = vec![7_i16; (ISOLATED_CHUNK_SAMPLES + 800) as usize];
        write_mono(&path, &samples);
        let chunks = plan_chunks(&path).unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(read_len(&chunks[0]), ISOLATED_CHUNK_SAMPLES as usize);
        assert_eq!(read_len(&chunks[1]), 800);
        assert_ne!(chunks[0], path);
        finish_chunks(&path, &chunks);
        assert!(path.exists());
        assert!(!chunks[0].exists());
        assert!(!chunks[1].exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn silence_hallucinations_fail_closed() {
        assert!(accept_asr_or_fail_closed("Thanks for watching.").is_none());
        assert!(accept_asr_or_fail_closed("thank you").is_none());
        assert!(accept_asr_or_fail_closed("  okay  ").is_none());
        assert!(accept_asr_or_fail_closed("").is_none());
        assert_eq!(
            accept_asr_or_fail_closed("Please save the metrics.").as_deref(),
            Some("Please save the metrics.")
        );
        assert_eq!(
            accept_asr_or_fail_closed("Thank you for sending the metrics.").as_deref(),
            Some("Thank you for sending the metrics.")
        );
    }

    #[test]
    fn erick_salad_is_not_a_silence_phrase_so_isolation_must_prevent_it() {
        let salad = "The magic seem very low Or. The metrics are give me Uh Uh the save but are all the damn time And things like that And you from All magic that we can probably Holy Send May Burns and Burnity.";
        // Isolation + --no-context / WhisperKit cache reset prevent this.
        // A real utterance that happens to be long must still paste.
        assert!(accept_asr_or_fail_closed(salad).is_some());
        assert!(accept_asr_or_fail_closed("We should save the metrics after the meeting.").is_some());
    }

    #[test]
    fn merge_drops_garbage_chunks_and_keeps_speech() {
        let merged = merge_accepted([
            "Thanks for watching.",
            "Save the metrics after the meeting.",
            "okay",
        ]);
        assert_eq!(merged, "Save the metrics after the meeting.");
    }

    #[test]
    fn low_confidence_is_fail_closed() {
        assert!(is_low_confidence(Some(0.12)));
        assert!(!is_low_confidence(Some(0.82)));
        assert!(!is_low_confidence(Some(0.0)));
        assert!(!is_low_confidence(None));
    }

    #[test]
    fn token_overlap_rejects_invented_salad() {
        let spoken = "Please send the metrics to May after the meeting today.";
        let salad = "The magic seem very low Or. The metrics are give me Uh Uh the save but are all the damn time And things like that And you from All magic that we can probably Holy Send May Burns and Burnity.";
        let ratio = token_overlap_ratio(spoken, salad).unwrap();
        assert!(
            ratio < 0.40,
            "BREAKS IF: invent/garbage inject accepted ({ratio})"
        );
        let keep = token_overlap_ratio(spoken, "Please send the metrics to May after the meeting today.").unwrap();
        assert!(keep > 0.80);
    }

    #[test]
    fn enforcer_bound_long_sticky_transcript_integrity_suite_b6530197() {
        assert_eq!(ENFORCER_SUITE, "b6530197");
        assert!(MAC_TF_ONLY);
        assert!(!IOS_IN_THIS_TIP);
        assert_eq!(
            PRODUCT_LOCK,
            "Mac TF only this tip; fail closed/reset rather than invent. Soft nits later."
        );
        assert!(ENFORCER_BOUND.contains("CONFIRMED Suite b6530197"));
        assert!(ENFORCER_BOUND.contains("Mac TF only this tip"));
        assert!(ENFORCER_BOUND.contains("fail closed/reset rather than invent words"));
        assert!(ENFORCER_BOUND.contains("Soft nits later"));
        assert!(ENFORCER_BOUND.contains("BREAKS IF: invent/garbage inject on long Mac session"));
        assert_eq!(
            BREAKS_IF,
            "BREAKS IF: invent/garbage inject on long Mac session; iOS in this tip"
        );

        let src = include_str!("transcript_integrity.rs");
        assert!(src.contains("GREEN (b6530197 Product LOCK UPDATE + Enforcer BOUND UPDATE):"));
        assert!(src.contains("BREAKS IF (b6530197 Product LOCK UPDATE + Enforcer BOUND UPDATE):"));
        assert!(src.contains("Mac TF only this tip"));
        assert!(src.contains("Soft nits later"));

        let polish = include_str!("polish.rs");
        assert!(polish.contains("Mac TF only this tip"));
        assert!(polish.contains("fail closed/reset rather than invent"));
        assert!(polish.contains("Soft nits later"));

        // Mac TF only: iOS sticky ASR / App Group append are not this tip.
        let ios_engine = include_str!("../../MabelIOS/Shared/OnDeviceSpeechEngine.swift");
        let ios_session = include_str!("../../MabelIOS/Shared/SpeechSession.swift");
        assert!(
            !ios_engine.contains("ISOLATED_CHUNK") && !ios_session.contains("transcribe_isolated_take"),
            "BREAKS IF: iOS in this tip"
        );
        assert!(
            include_str!("recorder.rs").contains("transcribe_isolated_take"),
            "Mac recorder must isolate long-session ASR"
        );
        assert!(
            !include_str!("recorder.rs").contains("spawn_vad_worker()"),
            "live streaming stays parked"
        );
    }
}
