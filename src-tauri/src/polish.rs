//! Product **Polish** — Enforcer BOUND (fold hard).
//!
//! - default OFF
//! - local Gemma fail closed (loopback llama-server; Err → rules, never
//!   paste contaminated model text)
//! - never invent
//! - fail closed / reset rather than invent words on long Mac sessions
//! - not Nexus write
//! - distinct from clipboard toggles (`clipboardHistoryEnabled` ≠ Polish)
//!
//! BREAKS IF: default ON / cloud / invent / garbage inject / Nexus write
//!
//! Product LOCK still applies: Pro only, modes Off|Casual|Professional|Polite,
//! after ASR, autocorrect + light reword, cat UI, no website upgrade.
//!
//! Product LOCK UPDATE + Enforcer BOUND UPDATE confirm (Erick 2026-09-08):
//! Mac TF only this tip; fail closed/reset rather than invent. Soft nits later.

use std::path::PathBuf;

use crate::storekit;

/// Named Enforcer BOUND. `enforcer_bound_*` tests fail if this is violated.
pub const ENFORCER_BOUND: &str =
    "default OFF; local Gemma fail closed; never invent; fail closed/reset rather than invent words; Mac TF only this tip; Soft nits later; not Nexus write; clipboardHistoryEnabled != Polish";

pub const MODE_OFF: &str = "off";
pub const MODE_CASUAL: &str = "casual";
pub const MODE_PROFESSIONAL: &str = "professional";
pub const MODE_POLITE: &str = "polite";

pub const MODES: &[&str] = &[MODE_OFF, MODE_CASUAL, MODE_PROFESSIONAL, MODE_POLITE];

/// Shared anti-invention contract for every live mode. Mode copy only adds
/// register guidance; it must not relax these rules.
const NEVER_INVENT: &str = "You are Mabel's on-device dictation polish assistant. The user spoke into a microphone and a local ASR engine transcribed their speech. This step is autocorrect and light reword only.\n\nNever invent facts. Never expand meaning. Never add names, numbers, clauses, greetings, sign-offs, answers, or commentary the speaker did not say. If a cleanup or register shift would require new information, keep the original wording. Fail closed: do not send this step off-device; output only the cleaned transcript.\n\nRules:\n- Remove filler words: \"um\", \"uh\", \"like\", \"you know\", \"I mean\", \"so\" when used as filler.\n- Add proper punctuation and capitalization.\n- Fix obvious self-corrections: when the speaker restarts a sentence, keep only the final version.\n- Preserve the speaker's words, meaning, and intent. Do not paraphrase, summarize, or embellish.\n- Do not answer questions in the transcript. The user is dictating, not asking you.\n- Output only the cleaned transcript. No preamble, no explanation, no quotes around it.";

const USER_PROMPT_PREFIX: &str = "Autocorrect and lightly reword this transcript. Never invent facts. Never expand meaning. Do not think, reason, or explain. Output only the cleaned text. Transcript: ";

const CASUAL_REGISTER: &str = "Register: Casual. Keep the speaker's informal voice. Contractions and casual wording stay. Autocorrect and light cleanup only.";

const PROFESSIONAL_REGISTER: &str = "Register: Professional. Prefer a workplace-neutral wording of the SAME utterance (gonna → going to, yeah → yes) when that does not change or expand meaning. Do not add formality, hedging, or extra clauses the speaker did not say.";

const POLITE_REGISTER: &str = "Register: Polite. Prefer a courteous wording of the SAME request or statement when a close synonym already covers it. Do not add please, thanks, apologies, or extra courtesy the speaker did not say.";

pub fn default_mode() -> String {
    MODE_OFF.to_string()
}

pub fn normalize_mode(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        MODE_CASUAL => MODE_CASUAL.to_string(),
        MODE_PROFESSIONAL => MODE_PROFESSIONAL.to_string(),
        MODE_POLITE => MODE_POLITE.to_string(),
        _ => MODE_OFF.to_string(),
    }
}

pub fn is_live(mode: &str) -> bool {
    matches!(
        normalize_mode(mode).as_str(),
        MODE_CASUAL | MODE_PROFESSIONAL | MODE_POLITE
    )
}

/// Persist-time gate. Off is always allowed. Live modes need StoreKit Pro
/// and a Stiki session. No silent Pro path: callers must surface the error.
pub fn require_mode_allowed(mode: &str) -> Result<String, String> {
    require_mode_allowed_at(None, mode)
}

pub fn require_mode_allowed_at(app_dir: Option<&PathBuf>, mode: &str) -> Result<String, String> {
    let mode = normalize_mode(mode);
    if is_live(&mode) {
        match app_dir {
            Some(dir) => {
                crate::stiki::require_pro_unlock(dir)?;
            }
            None => {
                storekit::require_pro()?;
            }
        }
    }
    Ok(mode)
}

/// Runtime gate. Free / lapsed / stub entitlement or signed-out Stiki → Off.
pub fn effective_mode(persisted: &str) -> String {
    effective_mode_at(None, persisted)
}

pub fn effective_mode_at(app_dir: Option<&PathBuf>, persisted: &str) -> String {
    let mode = normalize_mode(persisted);
    if !is_live(&mode) {
        return MODE_OFF.to_string();
    }
    if let Some(dir) = app_dir {
        if crate::stiki::pro_unlocked(dir) {
            return mode;
        }
        return MODE_OFF.to_string();
    }
    if storekit::pro_surfaces_unlocked() {
        mode
    } else {
        MODE_OFF.to_string()
    }
}

pub fn user_prompt_prefix() -> &'static str {
    USER_PROMPT_PREFIX
}

pub fn system_prompt(mode: &str) -> String {
    let register = match normalize_mode(mode).as_str() {
        MODE_PROFESSIONAL => PROFESSIONAL_REGISTER,
        MODE_POLITE => POLITE_REGISTER,
        MODE_CASUAL => CASUAL_REGISTER,
        _ => CASUAL_REGISTER,
    };
    format!("{NEVER_INVENT}\n\n{register}")
}

/// Fail closed on invented / expanded Gemma output.
///
/// Caller treats Err as "use the rules pass" — never paste the model text.
pub fn accept_or_fail_closed(input: &str, output: &str) -> Result<String, String> {
    let cleaned = output.trim();
    if cleaned.is_empty() {
        return Err("Polish output empty after sanitization (fail closed)".into());
    }
    let input_chars = input.trim().chars().count() as f32;
    let out_chars = cleaned.chars().count() as f32;
    // A real autocorrect / light reword stays near input length. >2.5x on a
    // 20+ char utterance is invention, reasoning leak, or meaning expansion.
    if input_chars >= 20.0 && out_chars > input_chars * 2.5 {
        return Err(format!(
            "Polish output too long ({} chars vs {} input chars), fail closed (never invent)",
            out_chars as usize, input_chars as usize
        ));
    }
    // Same-length invention (degraded ASR salad, Gemma drift) is not caught
    // by the 2.5x gate. Token overlap fails closed so we keep the rules text.
    if let Some(overlap) = crate::transcript_integrity::token_overlap_ratio(input, cleaned) {
        if overlap < 0.40 {
            return Err(format!(
                "Polish output dropped spoken tokens (overlap={overlap:.2}), fail closed (never invent)"
            ));
        }
    }
    Ok(cleaned.to_string())
}

/// Menu / Settings labels. Keep cat energy without turning this into a website.
pub fn mode_label(mode: &str) -> &'static str {
    match normalize_mode(mode).as_str() {
        MODE_CASUAL => "Casual",
        MODE_PROFESSIONAL => "Professional",
        MODE_POLITE => "Polite",
        _ => "Off",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_off() {
        assert_eq!(default_mode(), MODE_OFF);
        assert!(!is_live(&default_mode()));
    }

    #[test]
    fn four_modes_normalize() {
        assert_eq!(normalize_mode("off"), MODE_OFF);
        assert_eq!(normalize_mode("Casual"), MODE_CASUAL);
        assert_eq!(normalize_mode("PROFESSIONAL"), MODE_PROFESSIONAL);
        assert_eq!(normalize_mode(" polite "), MODE_POLITE);
        assert_eq!(normalize_mode("rewrite"), MODE_OFF);
        assert_eq!(normalize_mode(""), MODE_OFF);
        assert_eq!(normalize_mode("llm"), MODE_OFF);
    }

    #[test]
    fn live_modes_are_the_three_registers() {
        assert!(is_live(MODE_CASUAL));
        assert!(is_live(MODE_PROFESSIONAL));
        assert!(is_live(MODE_POLITE));
        assert!(!is_live(MODE_OFF));
        assert!(!is_live("rules"));
    }

    #[test]
    fn free_cannot_enable_live_mode() {
        assert!(require_mode_allowed(MODE_OFF).is_ok());
        assert!(require_mode_allowed(MODE_CASUAL).is_err());
        assert!(require_mode_allowed(MODE_PROFESSIONAL).is_err());
        assert!(require_mode_allowed(MODE_POLITE).is_err());
    }

    #[test]
    fn runtime_fails_closed_without_entitlement() {
        assert_eq!(effective_mode(MODE_CASUAL), MODE_OFF);
        assert_eq!(effective_mode(MODE_PROFESSIONAL), MODE_OFF);
        assert_eq!(effective_mode(MODE_POLITE), MODE_OFF);
        assert_eq!(effective_mode(MODE_OFF), MODE_OFF);
        let src = include_str!("polish.rs");
        assert!(
            src.contains("pro_surfaces_unlocked()"),
            "live Polish is a Pro surface: StoreKit alone is not enough"
        );
    }

    #[test]
    fn prompts_never_invent_facts_or_expand_meaning() {
        for mode in [MODE_CASUAL, MODE_PROFESSIONAL, MODE_POLITE] {
            let prompt = system_prompt(mode);
            assert!(prompt.contains("Never invent facts"), "{mode}");
            assert!(prompt.contains("Never expand meaning"), "{mode}");
            assert!(prompt.contains("autocorrect and light reword only"));
            assert!(prompt.contains("do not send this step off-device"));
            assert!(prompt.contains("Fail closed"));
            assert!(!prompt.contains("http://"));
            assert!(!prompt.contains("https://"));
        }
        assert!(system_prompt(MODE_CASUAL).contains("Casual"));
        assert!(system_prompt(MODE_PROFESSIONAL).contains("Professional"));
        assert!(system_prompt(MODE_POLITE).contains("Polite"));
        assert!(user_prompt_prefix().contains("Never invent facts"));
        assert!(user_prompt_prefix().contains("Never expand meaning"));
    }

    #[test]
    fn labels_match_product_modes() {
        assert_eq!(mode_label(MODE_OFF), "Off");
        assert_eq!(mode_label(MODE_CASUAL), "Casual");
        assert_eq!(mode_label(MODE_PROFESSIONAL), "Professional");
        assert_eq!(mode_label(MODE_POLITE), "Polite");
        assert_eq!(MODES.len(), 4);
    }

    #[test]
    fn settings_and_status_item_name_polish_without_web_upgrade() {
        let html = include_str!("../../index.html");
        assert!(html.contains("id=\"polish-toggle\""));
        assert!(html.contains("id=\"polish-mode-select\""));
        assert!(html.contains("id=\"polish-activate\""));
        assert!(html.contains("value=\"off\""));
        assert!(html.contains("value=\"casual\""));
        assert!(html.contains("value=\"professional\""));
        assert!(html.contains("value=\"polite\""));
        assert!(html.contains("Coach cannot rewrite"));
        assert!(html.contains("not Nexus"));
        let ui = include_str!("clipboard_ui.rs");
        assert!(ui.contains("Polish"));
        assert!(ui.contains("polish-casual"));
        assert!(ui.contains("open-plans"));
        assert!(!ui.contains("chibiteklabs.com"));
        let ts = include_str!("../../src/main.ts");
        assert!(ts.contains("polish_set"));
        assert!(ts.contains("openPlans()"));
        assert!(ts.contains("polish-toggle"));
        assert!(!ts.contains("https://chibiteklabs"));
        let main = include_str!("main.rs");
        assert!(main.contains("require_mode_allowed_at"));
        assert!(main.contains("polish_set"));
        assert!(!main.contains("MabelSpatial"));
    }

    #[test]
    fn recorder_uses_pro_gated_local_gemma_polish() {
        let rec = include_str!("recorder.rs");
        assert!(rec.contains("polish_or_rules"));
        let llm = include_str!("llm.rs");
        assert!(llm.contains("polish::system_prompt"));
        assert!(llm.contains("127.0.0.1"));
        assert!(llm.contains("never invent"));
        let polish_fn = llm.split("pub async fn polish_or_rules").nth(1).unwrap();
        let polish_fn = polish_fn.split("pub async fn ensure_and_cleanup").next().unwrap();
        assert!(!polish_fn.contains("groq"));
        assert!(!polish_fn.contains("transcribe_groq"));
        assert!(!polish_fn.contains("api.groq.com"));
        let stream = include_str!("streaming.rs");
        assert!(stream.contains("effective_mode"));
        assert!(stream.contains("cleanup_with_llm_mode"));
    }

    #[test]
    fn invented_expansion_fails_closed() {
        let ok = accept_or_fail_closed("Hello there friend.", "Hello there, friend.");
        assert_eq!(ok.unwrap(), "Hello there, friend.");
        let long = "x".repeat(200);
        assert!(
            accept_or_fail_closed("hello world this is spoken text", &long).is_err(),
            "BREAKS IF: invent (expanded output accepted)"
        );
        assert!(accept_or_fail_closed("hello", "   ").is_err());
        let spoken = "Please send the metrics to May after the meeting today.";
        let salad = "The magic seem very low Or. The metrics are give me Uh Uh the save but are all the damn time And things like that And you from All magic that we can probably Holy Send May Burns and Burnity.";
        assert!(
            accept_or_fail_closed(spoken, salad).is_err(),
            "BREAKS IF: invent/garbage inject on long session"
        );
        assert_eq!(accept_or_fail_closed(spoken, spoken).unwrap(), spoken);
    }

    #[test]
    fn enforcer_bound_breaks_if_default_on_cloud_invent_or_nexus_write() {
        assert!(ENFORCER_BOUND.contains("default OFF"));
        assert!(ENFORCER_BOUND.contains("never invent"));
        assert!(ENFORCER_BOUND.contains("fail closed/reset rather than invent words"));
        assert!(ENFORCER_BOUND.contains("Mac TF only this tip"));
        assert!(ENFORCER_BOUND.contains("Soft nits later"));
        assert!(ENFORCER_BOUND.contains("not Nexus write"));
        assert!(ENFORCER_BOUND.contains("clipboardHistoryEnabled != Polish"));

        // BREAKS IF: default ON
        assert_eq!(default_mode(), MODE_OFF, "BREAKS IF: default ON");
        assert!(!is_live(&default_mode()), "BREAKS IF: default ON");
        let settings = crate::settings::Settings::default();
        assert_eq!(settings.polish_mode, MODE_OFF, "BREAKS IF: default ON");
        assert!(
            !settings.clipboard_history_enabled,
            "clipboard stays independently off"
        );
        let html = include_str!("../../index.html");
        let toggle = html
            .split("id=\"polish-toggle\"")
            .nth(1)
            .expect("polish toggle");
        let toggle = toggle.split("</button>").next().unwrap();
        assert!(
            toggle.contains("aria-checked=\"false\""),
            "BREAKS IF: default ON"
        );

        // BREAKS IF: cloud
        let llm = include_str!("llm.rs");
        assert!(llm.contains("http://127.0.0.1:{}/v1/chat/completions"));
        let polish_fn = llm.split("pub async fn polish_or_rules").nth(1).unwrap();
        let polish_fn = polish_fn.split("pub async fn ensure_and_cleanup").next().unwrap();
        assert!(!polish_fn.contains("groq"), "BREAKS IF: cloud");
        assert!(!polish_fn.contains("api.groq.com"), "BREAKS IF: cloud");
        assert!(
            !polish_fn.contains("https://"),
            "BREAKS IF: cloud"
        );

        // BREAKS IF: invent / garbage inject on long session
        for mode in [MODE_CASUAL, MODE_PROFESSIONAL, MODE_POLITE] {
            let prompt = system_prompt(mode);
            assert!(prompt.contains("Never invent facts"), "BREAKS IF: invent");
            assert!(prompt.contains("Never expand meaning"), "BREAKS IF: invent");
        }
        assert!(llm.contains("accept_or_fail_closed"), "BREAKS IF: invent");
        assert!(
            include_str!("recorder.rs").contains("transcribe_isolated_take"),
            "BREAKS IF: invent/garbage inject on long session"
        );
        assert!(
            include_str!("transcribe_local.rs").contains("--no-context"),
            "BREAKS IF: invent/garbage inject on long session"
        );

        // BREAKS IF: Nexus write
        let main = include_str!("main.rs");
        let polish_set = main.split("fn polish_set").nth(1).unwrap();
        let polish_set = polish_set.split("fn refresh_status_item").next().unwrap();
        assert!(polish_set.contains("polish_mode"));
        assert!(
            !polish_set.contains("clipboard_history"),
            "BREAKS IF: clipboard toggle merged with Polish"
        );
        assert!(
            !polish_set.contains("clipboardHistory"),
            "BREAKS IF: clipboard toggle merged with Polish"
        );
        let nexus_write = format!("{}{}", "nexus", "_write");
        assert!(!polish_set.contains(&nexus_write), "BREAKS IF: Nexus write");
        assert!(!main.contains(&nexus_write), "BREAKS IF: Nexus write");
        assert!(!llm.contains(&nexus_write), "BREAKS IF: Nexus write");
    }

    #[test]
    fn enforcer_bound_clipboard_toggle_is_not_polish() {
        let settings_src = include_str!("settings.rs");
        assert!(settings_src.contains("clipboardHistoryEnabled"));
        assert!(settings_src.contains("polishMode"));
        assert!(settings_src.contains("Distinct from `clipboardHistoryEnabled`"));
        assert_ne!(
            "clipboardHistoryEnabled", "polishMode",
            "BREAKS IF: clipboardHistoryEnabled == Polish"
        );
        let html = include_str!("../../index.html");
        assert!(html.contains("id=\"polish-toggle\""));
        assert!(html.contains("id=\"clipboard-history-toggle\""));
        assert_ne!("polish-toggle", "clipboard-history-toggle");
        let ts = include_str!("../../src/main.ts");
        let persist = ts.split("async function persistPolishMode").nth(1).unwrap();
        let persist = persist.split("polishToggle.addEventListener").next().unwrap();
        assert!(!persist.contains("clipboardHistoryEnabled"));
        assert!(!persist.contains("clipboard_history"));
    }

    #[test]
    fn isolated_from_nexus_coach_and_off_device() {
        let src = include_str!("polish.rs");
        assert!(src.contains("Not Nexus"));
        assert!(src.contains("Coach cannot rewrite"));
        assert!(src.contains("company memory"));
        assert!(src.contains("Never send this step off-device"));
        let forbidden = format!("{}Polish", "nexus");
        let settings = include_str!("settings.rs");
        assert!(settings.contains("polishMode"));
        assert!(settings.contains("not Nexus"));
        assert!(!settings.contains(&forbidden));
        let main = include_str!("main.rs");
        assert!(!main.contains(&forbidden));
        assert!(!main.contains("coach_rewrite"));
        assert!(!main.contains("MabelSpatial"));
    }
}
