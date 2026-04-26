use keyring::Entry;

const SERVICE: &str = "com.mabel.app";
const GROQ_KEY_ACCOUNT: &str = "groq_api_key";

fn entry() -> Result<Entry, String> {
    Entry::new(SERVICE, GROQ_KEY_ACCOUNT).map_err(|e| format!("Keychain entry error: {}", e))
}

pub fn get_groq_key() -> Result<String, String> {
    match entry()?.get_password() {
        Ok(s) => Ok(s),
        Err(keyring::Error::NoEntry) => Ok(String::new()),
        Err(e) => Err(format!("Keychain read error: {}", e)),
    }
}

/// Fast non-throwing check for "is a key stored?" — used to render UI status
/// without prompting the user for keychain access.
pub fn has_groq_key() -> bool {
    matches!(entry().and_then(|e| e.get_password().map_err(|err| err.to_string())), Ok(s) if !s.is_empty())
}

pub fn set_groq_key(value: &str) -> Result<(), String> {
    let e = entry()?;
    if value.is_empty() {
        match e.delete_credential() {
            Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(format!("Keychain delete error: {}", err)),
        }
    } else {
        e.set_password(value)
            .map_err(|err| format!("Keychain write error: {}", err))
    }
}
