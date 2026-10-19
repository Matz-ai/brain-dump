use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::history;

/// Totaux d'une journée. Seuls des compteurs sont gardés, jamais le texte : les stats survivent
/// à « Tout effacer » et à l'historique désactivé.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Day {
    pub words: u64,
    #[serde(rename = "durationMs")]
    pub duration_ms: u64,
    pub count: u64,
}

/// Clé : date locale "AAAA-MM-JJ"
pub type Stats = BTreeMap<String, Day>;

static FILE_LOCK: Mutex<()> = Mutex::new(());

fn path(app_dir: &Path) -> PathBuf {
    app_dir.join("stats.json")
}

pub fn word_count(text: &str) -> u64 {
    text.split_whitespace()
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .count() as u64
}

/// Fichier absent (première version avec les stats) → reconstruit depuis l'historique existant.
pub fn load(app_dir: &Path) -> Stats {
    let _guard = FILE_LOCK.lock().unwrap();
    read(app_dir)
}

fn read(app_dir: &Path) -> Stats {
    match fs::read_to_string(path(app_dir)) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => from_history(&history::load(app_dir)),
    }
}

fn from_history(entries: &[history::Entry]) -> Stats {
    let mut stats = Stats::new();
    for e in entries.iter().filter(|e| e.duration_ms > 0) {
        if let Some(day) = e.date.get(..10) {
            add_to(&mut stats, day, &e.text, e.duration_ms);
        }
    }
    stats
}

fn add_to(stats: &mut Stats, day: &str, text: &str, duration_ms: u64) {
    let d = stats.entry(day.to_string()).or_default();
    d.words += word_count(text);
    d.duration_ms += duration_ms;
    d.count += 1;
}

pub fn record(app_dir: &Path, day: &str, text: &str, duration_ms: u64) -> Result<Stats, String> {
    let _guard = FILE_LOCK.lock().unwrap();
    let mut stats = read(app_dir);
    add_to(&mut stats, day, text, duration_ms);
    fs::create_dir_all(app_dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string(&stats).map_err(|e| e.to_string())?;
    fs::write(path(app_dir), json).map_err(|e| e.to_string())?;
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    #[test]
    fn test_word_count_ignores_punctuation() {
        assert_eq!(word_count("Bonjour, ça va ? Oui !"), 4);
        assert_eq!(word_count("  "), 0);
        assert_eq!(word_count("l'équipe - 0.4.0"), 2);
    }

    #[test]
    fn test_seeded_from_history_then_recorded() {
        let dir = temp_dir().join("brain_dump_test_stats");
        let _ = fs::remove_dir_all(&dir);
        let entry = |date: &str, text: &str, ms| history::Entry {
            date: date.to_string(),
            text: text.to_string(),
            duration_ms: ms,
            ..Default::default()
        };
        history::add(&dir, entry("2026-10-16T21:37:57+02:00", "un deux trois", 3000)).unwrap();
        history::add(&dir, entry("2026-10-16T22:00:00+02:00", "quatre", 1000)).unwrap();
        // Dictée réessayée : pas de durée, exclue (fausserait la vitesse)
        history::add(&dir, entry("2026-10-17T09:00:00+02:00", "cinq", 0)).unwrap();

        let seeded = load(&dir);
        assert_eq!(seeded.len(), 1);
        assert_eq!(seeded["2026-10-16"], Day { words: 4, duration_ms: 4000, count: 2 });

        let stats = record(&dir, "2026-10-18", "six sept", 2000).unwrap();
        assert_eq!(stats["2026-10-18"].words, 2);
        // Une fois le fichier créé, vider l'historique ne touche plus aux stats
        history::clear(&dir).unwrap();
        assert_eq!(load(&dir).len(), 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_corrupted_file_gives_empty_stats() {
        let dir = temp_dir().join("brain_dump_test_stats_corrupt");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(path(&dir), "{oops").unwrap();
        assert!(load(&dir).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
