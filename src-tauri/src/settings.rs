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
    #[serde(default = "default_streaming")]
    pub streaming: bool,
    /// Tracks "is a Groq key in the keychain?" without ever actually reading the
    /// keychain, so the UI can show "Saved" status without prompting.
    #[serde(rename = "groqKeyConfigured", default)]
    pub groq_key_configured: bool,
    #[serde(rename = "launchAtLogin", default)]
    pub launch_at_login: bool,
    #[serde(rename = "showInDock", default = "default_true")]
    pub show_in_dock: bool,
    #[serde(rename = "dictationSounds", default = "default_true")]
    pub dictation_sounds: bool,
    #[serde(rename = "pressEnterCommand", default)]
    pub press_enter_command: bool,
}

fn default_streaming() -> bool { true }
fn default_true() -> bool { true }

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
    #[serde(default = "default_streaming")]
    streaming: bool,
    #[serde(rename = "groqKeyConfigured", default)]
    groq_key_configured: bool,
    #[serde(rename = "launchAtLogin", default)]
    launch_at_login: bool,
    #[serde(rename = "showInDock", default = "default_true")]
    show_in_dock: bool,
    #[serde(rename = "dictationSounds", default = "default_true")]
    dictation_sounds: bool,
    #[serde(rename = "pressEnterCommand", default)]
    press_enter_command: bool,
}

impl From<&Settings> for DiskSettings {
    fn from(s: &Settings) -> Self {
        Self {
            microphone: s.microphone.clone(),
            engine: s.engine.clone(),
            whisper_model: s.whisper_model.clone(),
            recording_mode: s.recording_mode.clone(),
            hotkey: s.hotkey.clone(),
            streaming: s.streaming,
            groq_key_configured: s.groq_key_configured,
            launch_at_login: s.launch_at_login,
            show_in_dock: s.show_in_dock,
            dictation_sounds: s.dictation_sounds,
            press_enter_command: s.press_enter_command,
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
            streaming: true,
            groq_key_configured: false,
            launch_at_login: false,
            show_in_dock: true,
            dictation_sounds: true,
            press_enter_command: false,
        }
    }
}

impl Settings {
    pub fn config_path(app_dir: &PathBuf) -> PathBuf {
        app_dir.join("config.json")
    }

    pub fn load(app_dir: &PathBuf) -> Self {
        let path = Self::config_path(app_dir);
        let (settings, needs_migration) = match fs::read_to_string(&path) {
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
                        streaming: d.streaming,
                        groq_key_configured: d.groq_key_configured,
                        launch_at_login: d.launch_at_login,
                        show_in_dock: d.show_in_dock,
                        dictation_sounds: d.dictation_sounds,
                        press_enter_command: d.press_enter_command,
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

        // Don't proactively read the Groq key from the keychain on startup.
        // Unsigned dev builds get a new binary signature on every rebuild, which
        // causes macOS to prompt for keychain access repeatedly and can hang the
        // launch. The key is fetched on-demand at cloud transcription time, and
        // the UI shows a "key is set" status without exposing the value.
        if needs_migration {
            let _ = settings.save(app_dir);
        }

        settings
    }


    pub fn save(&self, app_dir: &PathBuf) -> Result<(), String> {
        let path = Self::config_path(app_dir);
        fs::create_dir_all(app_dir).map_err(|e| e.to_string())?;
        // If a non-empty key came in, flip the configured flag so the UI can
        // show "Saved" without ever reading the keychain back.
        let mut to_disk = self.clone();
        if !to_disk.groq_api_key.is_empty() {
            to_disk.groq_key_configured = true;
        }
        let disk: DiskSettings = (&to_disk).into();
        let json = serde_json::to_string_pretty(&disk).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| e.to_string())?;
        // Only touch the keychain if the caller actually provided a non-empty
        // key. Settings saves from the UI ship an empty groq_api_key on every
        // change (mic, mode, hotkey, etc) and we don't want each one to trigger
        // a keychain prompt on unsigned dev builds.
        if !self.groq_api_key.is_empty() {
            secrets::set_groq_key(&self.groq_api_key)?;
        }
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
