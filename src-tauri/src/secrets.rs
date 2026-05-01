use keyring::Entry;
use std::sync::RwLock;

const SERVICE: &str = "com.mabel.app";
const GROQ_KEY_ACCOUNT: &str = "groq_api_key";

/// In-process cache of the Groq key. First access touches the keychain (and on
/// dev builds with changing signatures, prompts the user). Subsequent calls
/// read from this cache, so a dictation flurry doesn't trigger a flurry of
/// keychain prompts. Updated whenever set_groq_key writes a new value.
static CACHED: RwLock<Option<String>> = RwLock::new(None);

fn entry() -> Result<Entry, String> {
    Entry::new(SERVICE, GROQ_KEY_ACCOUNT).map_err(|e| format!("Keychain entry error: {}", e))
}

pub fn get_groq_key() -> Result<String, String> {
    // Dev override: setting MABEL_GROQ_KEY in the shell skips the keychain
    // entirely. Useful when running `npm run tauri dev` because Rust edits
    // change the binary signature each rebuild, which makes macOS treat every
    // dev session as a new app and re-prompt for keychain access. In a signed
    // production install the keychain prompt fires once per machine and
    // "Always Allow" persists forever.
    if let Ok(env_key) = std::env::var("MABEL_GROQ_KEY") {
        if !env_key.is_empty() {
            return Ok(env_key);
        }
    }
    if let Some(cached) = CACHED.read().unwrap().clone() {
        return Ok(cached);
    }
    match entry()?.get_password() {
        Ok(s) => {
            *CACHED.write().unwrap() = Some(s.clone());
            Ok(s)
        }
        Err(keyring::Error::NoEntry) => Ok(String::new()),
        Err(e) => Err(format!("Keychain read error: {}", e)),
    }
}

/// Probe whether a key is stored without holding it in cache. Used by the
/// Settings UI to decide whether to show "Saved". On the first call after
/// install this still triggers the macOS keychain prompt (unavoidable —
/// macOS requires consent before disclosing any keychain item, including its
/// existence in the form we have here).
pub fn has_groq_key() -> bool {
    matches!(entry().and_then(|e| e.get_password().map_err(|err| err.to_string())), Ok(s) if !s.is_empty())
}

pub fn set_groq_key(value: &str) -> Result<(), String> {
    let e = entry()?;
    if value.is_empty() {
        let res = match e.delete_credential() {
            Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(format!("Keychain delete error: {}", err)),
        };
        if res.is_ok() {
            *CACHED.write().unwrap() = None;
        }
        res
    } else {
        let res = e
            .set_password(value)
            .map_err(|err| format!("Keychain write error: {}", err));
        if res.is_ok() {
            *CACHED.write().unwrap() = Some(value.to_string());
        }
        res
    }
}
