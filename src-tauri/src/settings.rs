use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::secrets;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub microphone: String,
    pub engine: String,
    #[serde(rename = "whisperModel")]
    pub whisper_model: String,
    #[serde(rename = "groqApiKey")]
    pub groq_api_key: String,
    #[serde(rename = "recordingMode")]
    pub recording_mode: String,
    pub hotkey: String,
}

/// What we actually serialize to disk. Excludes the Groq API key, which lives
/// in the OS keychain. Keeps the same JSON shape the UI expects, minus the key.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct DiskSettings {
    microphone: String,
    engine: String,
    #[serde(rename = "whisperModel")]
    whisper_model: String,
    #[serde(rename = "recordingMode")]
    recording_mode: String,
    hotkey: String,
}

impl From<&Settings> for DiskSettings {
    fn from(s: &Settings) -> Self {
        Self {
            microphone: s.microphone.clone(),
            engine: s.engine.clone(),
            whisper_model: s.whisper_model.clone(),
            recording_mode: s.recording_mode.clone(),
            hotkey: s.hotkey.clone(),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            microphone: "default".to_string(),
            engine: "local".to_string(),
            whisper_model: "small".to_string(),
            groq_api_key: String::new(),
            recording_mode: "toggle".to_string(),
            hotkey: "CmdOrCtrl+Shift+Space".to_string(),
        }
    }
}

impl Settings {
    pub fn config_path(app_dir: &PathBuf) -> PathBuf {
        app_dir.join("config.json")
    }

    pub fn load(app_dir: &PathBuf) -> Self {
        let path = Self::config_path(app_dir);
        let (mut settings, needs_migration) = match fs::read_to_string(&path) {
            Ok(contents) => {
                // Detect plaintext groqApiKey in old config.json and stage it
                // for migration into the keychain.
                let legacy_key = serde_json::from_str::<serde_json::Value>(&contents)
                    .ok()
                    .and_then(|v| {
                        v.get("groqApiKey")
                            .and_then(|x| x.as_str())
                            .filter(|s| !s.is_empty())
                            .map(|s| s.to_string())
                    });

                let parsed = serde_json::from_str::<DiskSettings>(&contents)
                    .map(|d| Settings {
                        microphone: d.microphone,
                        engine: d.engine,
                        whisper_model: d.whisper_model,
                        groq_api_key: String::new(),
                        recording_mode: d.recording_mode,
                        hotkey: d.hotkey,
                    })
                    .unwrap_or_default();

                if let Some(key) = legacy_key {
                    let _ = secrets::set_groq_key(&key);
                    (parsed, true)
                } else {
                    (parsed, false)
                }
            }
            Err(_) => (Self::default(), false),
        };

        settings.groq_api_key = secrets::get_groq_key().unwrap_or_default();

        // Only rewrite config.json if we just stripped a plaintext key out of it.
        if needs_migration {
            let _ = settings.save(app_dir);
        }

        settings
    }

    pub fn save(&self, app_dir: &PathBuf) -> Result<(), String> {
        let path = Self::config_path(app_dir);
        fs::create_dir_all(app_dir).map_err(|e| e.to_string())?;
        let disk: DiskSettings = self.into();
        let json = serde_json::to_string_pretty(&disk).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| e.to_string())?;
        secrets::set_groq_key(&self.groq_api_key)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_settings() {
        let settings = Settings::default();
        assert_eq!(settings.microphone, "default");
        assert_eq!(settings.engine, "local");
        assert_eq!(settings.whisper_model, "small");
        assert_eq!(settings.groq_api_key, "");
        assert_eq!(settings.recording_mode, "toggle");
        assert_eq!(settings.hotkey, "CmdOrCtrl+Shift+Space");
    }

    #[test]
    fn test_disk_settings_excludes_groq_key() {
        let settings = Settings {
            groq_api_key: "secret-key".to_string(),
            ..Settings::default()
        };
        let disk: DiskSettings = (&settings).into();
        let json = serde_json::to_string(&disk).unwrap();
        assert!(!json.contains("groqApiKey"));
        assert!(!json.contains("secret-key"));
    }

    #[test]
    fn test_disk_round_trip_preserves_non_secret_fields() {
        let mut settings = Settings::default();
        settings.engine = "cloud".to_string();
        settings.whisper_model = "medium".to_string();
        settings.recording_mode = "push-to-talk".to_string();

        let disk: DiskSettings = (&settings).into();
        let json = serde_json::to_string(&disk).unwrap();
        let parsed: DiskSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, disk);
    }
}
