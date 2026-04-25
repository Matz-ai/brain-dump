use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub microphone: String,
    #[serde(rename = "groqApiKey")]
    pub groq_api_key: String,
    #[serde(rename = "recordingMode")]
    pub recording_mode: String,

    #[serde(rename = "hotkeyPasteOnly")]
    pub hotkey_paste_only: String,
    #[serde(rename = "hotkeyDbPaste")]
    pub hotkey_db_paste: String,

    pub language: String,

    #[serde(rename = "supabaseUrl")]
    pub supabase_url: String,
    #[serde(rename = "supabaseAnonKey")]
    pub supabase_anon_key: String,

    #[serde(rename = "captureContext")]
    pub capture_context: bool,

    #[serde(rename = "whisperModel")]
    pub whisper_model: String,

    pub vocabulary: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            microphone: "default".to_string(),
            groq_api_key: String::new(),
            recording_mode: "toggle".to_string(),
            hotkey_paste_only: "CmdOrCtrl+Shift+Space".to_string(),
            hotkey_db_paste: "CmdOrCtrl+Shift+V".to_string(),
            language: "fr".to_string(),
            supabase_url: String::new(),
            supabase_anon_key: String::new(),
            capture_context: true,
            whisper_model: "whisper-large-v3-turbo".to_string(),
            vocabulary: String::new(),
        }
    }
}

impl Settings {
    pub fn config_path(app_dir: &PathBuf) -> PathBuf {
        app_dir.join("config.json")
    }

    pub fn load(app_dir: &PathBuf) -> Self {
        let path = Self::config_path(app_dir);
        match fs::read_to_string(&path) {
            Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, app_dir: &PathBuf) -> Result<(), String> {
        let path = Self::config_path(app_dir);
        fs::create_dir_all(app_dir).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, json).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    #[test]
    fn test_default_settings() {
        let settings = Settings::default();
        assert_eq!(settings.language, "fr");
        assert_eq!(settings.hotkey_paste_only, "CmdOrCtrl+Shift+Space");
        assert_eq!(settings.hotkey_db_paste, "CmdOrCtrl+Shift+V");
        assert!(settings.capture_context);
    }

    #[test]
    fn test_save_and_load() {
        let dir = temp_dir().join("brain_dump_test_settings");
        let _ = fs::remove_dir_all(&dir);

        let mut settings = Settings::default();
        settings.supabase_url = "https://test.supabase.co".to_string();
        settings.supabase_anon_key = "eyJ...".to_string();

        settings.save(&dir).unwrap();
        let loaded = Settings::load(&dir);

        assert_eq!(loaded.supabase_url, "https://test.supabase.co");
        assert_eq!(loaded.supabase_anon_key, "eyJ...");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_load_missing_file_returns_default() {
        let dir = temp_dir().join("brain_dump_test_missing");
        let _ = fs::remove_dir_all(&dir);
        let settings = Settings::load(&dir);
        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn test_partial_config_preserves_existing_fields() {
        // Simule un config.json plus ancien : seuls quelques champs sont présents.
        // Les nouveaux champs (vocabulary, whisperModel, etc.) doivent fall back sur les defaults
        // SANS écraser les champs existants.
        let dir = temp_dir().join("brain_dump_test_partial");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let partial = r#"{
            "microphone": "MyMic",
            "groqApiKey": "gsk_secret",
            "supabaseUrl": "https://x.supabase.co",
            "supabaseAnonKey": "sb_publishable_xxx",
            "language": "en"
        }"#;
        fs::write(Settings::config_path(&dir), partial).unwrap();

        let loaded = Settings::load(&dir);

        // Champs présents dans le JSON : préservés
        assert_eq!(loaded.microphone, "MyMic");
        assert_eq!(loaded.groq_api_key, "gsk_secret");
        assert_eq!(loaded.supabase_url, "https://x.supabase.co");
        assert_eq!(loaded.supabase_anon_key, "sb_publishable_xxx");
        assert_eq!(loaded.language, "en");

        // Nouveaux champs absents du JSON : defaults
        assert_eq!(loaded.whisper_model, "whisper-large-v3-turbo");
        assert_eq!(loaded.vocabulary, "");
        assert_eq!(loaded.hotkey_paste_only, "CmdOrCtrl+Shift+Space");
        assert_eq!(loaded.hotkey_db_paste, "CmdOrCtrl+Shift+V");
        assert!(loaded.capture_context);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_unknown_fields_ignored() {
        // Si le JSON contient des anciens champs supprimés (engine, hotkeyNote, etc.),
        // le load doit ignorer ces clés inconnues sans crash et préserver le reste.
        let dir = temp_dir().join("brain_dump_test_unknown");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let with_old_fields = r#"{
            "microphone": "MyMic",
            "groqApiKey": "gsk_keep",
            "engine": "cloud",
            "hotkeyNote": "Ctrl+Shift+N",
            "hotkeyInline": "Ctrl+Shift+I",
            "whisperModel": "whisper-large-v3"
        }"#;
        fs::write(Settings::config_path(&dir), with_old_fields).unwrap();

        let loaded = Settings::load(&dir);
        assert_eq!(loaded.microphone, "MyMic");
        assert_eq!(loaded.groq_api_key, "gsk_keep");
        assert_eq!(loaded.whisper_model, "whisper-large-v3"); // toujours valide

        let _ = fs::remove_dir_all(&dir);
    }
}
