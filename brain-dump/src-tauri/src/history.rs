use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const MAX_ENTRIES: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Entry {
    pub id: i64,
    pub date: String,
    /// Texte collé
    pub text: String,
    /// Version corrigée à la main dans l'historique (sert au dictionnaire qui apprend)
    pub edited: Option<String>,
    /// Transcription Whisper avant correction IA / cleanup
    pub raw: String,
    pub app: Option<String>,
    #[serde(rename = "durationMs")]
    pub duration_ms: u64,
}

impl Default for Entry {
    fn default() -> Self {
        Self {
            id: 0,
            date: String::new(),
            text: String::new(),
            edited: None,
            raw: String::new(),
            app: None,
            duration_ms: 0,
        }
    }
}

/// Sérialise les lectures-écritures du fichier : une dictée peut arriver pendant une édition.
static FILE_LOCK: Mutex<()> = Mutex::new(());

fn path(app_dir: &Path) -> PathBuf {
    app_dir.join("history.json")
}

/// Plus récente en premier. Fichier absent ou corrompu → historique vide.
pub fn load(app_dir: &Path) -> Vec<Entry> {
    let _guard = FILE_LOCK.lock().unwrap();
    read(app_dir)
}

fn read(app_dir: &Path) -> Vec<Entry> {
    fs::read_to_string(path(app_dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn write(app_dir: &Path, entries: &[Entry]) -> Result<(), String> {
    fs::create_dir_all(app_dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string(entries).map_err(|e| e.to_string())?;
    fs::write(path(app_dir), json).map_err(|e| e.to_string())
}

pub fn add(app_dir: &Path, entry: Entry) -> Result<(), String> {
    let _guard = FILE_LOCK.lock().unwrap();
    let mut entries = read(app_dir);
    entries.insert(0, entry);
    entries.truncate(MAX_ENTRIES);
    write(app_dir, &entries)
}

/// Enregistre la version éditée (ou l'efface si on revient au texte d'origine) et renvoie l'entrée.
pub fn edit(app_dir: &Path, id: i64, text: &str) -> Result<Entry, String> {
    let _guard = FILE_LOCK.lock().unwrap();
    let mut entries = read(app_dir);
    let entry = entries
        .iter_mut()
        .find(|e| e.id == id)
        .ok_or("Dictée introuvable dans l'historique")?;
    entry.edited = if text == entry.text { None } else { Some(text.to_string()) };
    let updated = entry.clone();
    write(app_dir, &entries)?;
    Ok(updated)
}

pub fn clear(app_dir: &Path) -> Result<(), String> {
    let _guard = FILE_LOCK.lock().unwrap();
    match fs::remove_file(path(app_dir)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    fn dir(name: &str) -> PathBuf {
        let d = temp_dir().join(name);
        let _ = fs::remove_dir_all(&d);
        d
    }

    fn entry(id: i64) -> Entry {
        Entry { id, text: format!("dictée {}", id), ..Default::default() }
    }

    #[test]
    fn test_rotation_keeps_100_most_recent() {
        let d = dir("brain_dump_test_history_rotation");
        for id in 1..=105 {
            add(&d, entry(id)).unwrap();
        }
        let entries = load(&d);
        assert_eq!(entries.len(), MAX_ENTRIES);
        assert_eq!(entries[0].id, 105);
        assert_eq!(entries[MAX_ENTRIES - 1].id, 6);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn test_missing_file_is_empty() {
        let d = dir("brain_dump_test_history_missing");
        assert!(load(&d).is_empty());
        assert!(clear(&d).is_ok());
    }

    #[test]
    fn test_corrupt_file_is_empty_and_recovers() {
        let d = dir("brain_dump_test_history_corrupt");
        fs::create_dir_all(&d).unwrap();
        fs::write(path(&d), "{ pas du json").unwrap();
        assert!(load(&d).is_empty());
        add(&d, entry(1)).unwrap();
        assert_eq!(load(&d).len(), 1);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn test_edit_keeps_original() {
        let d = dir("brain_dump_test_history_edit");
        add(&d, entry(1)).unwrap();
        let e = edit(&d, 1, "dictée corrigée").unwrap();
        assert_eq!(e.text, "dictée 1");
        assert_eq!(e.edited.as_deref(), Some("dictée corrigée"));
        // Revenir au texte d'origine efface la version éditée
        assert_eq!(edit(&d, 1, "dictée 1").unwrap().edited, None);
        assert!(edit(&d, 42, "x").is_err());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn test_clear() {
        let d = dir("brain_dump_test_history_clear");
        add(&d, entry(1)).unwrap();
        clear(&d).unwrap();
        assert!(load(&d).is_empty());
        let _ = fs::remove_dir_all(&d);
    }
}
