use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// Groq free tier : 2000 requêtes Whisper par jour, reset à minuit UTC.
pub const DAILY_LIMIT: u32 = 2000;
pub const WARN_THRESHOLD: u32 = 1500; // 3/4 du quota

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct QuotaStatus {
    pub date: String,       // YYYY-MM-DD (UTC)
    pub used: u32,
    pub limit: u32,
    pub warned: bool,
}

impl Default for QuotaStatus {
    fn default() -> Self {
        Self {
            date: today_utc(),
            used: 0,
            limit: DAILY_LIMIT,
            warned: false,
        }
    }
}

fn today_utc() -> String {
    Utc::now().format("%Y-%m-%d").to_string()
}

fn quota_path(app_dir: &PathBuf) -> PathBuf {
    app_dir.join("groq_quota.json")
}

pub fn load(app_dir: &PathBuf) -> QuotaStatus {
    let path = quota_path(app_dir);
    let status: QuotaStatus = match fs::read_to_string(&path) {
        Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
        Err(_) => QuotaStatus::default(),
    };

    // Rollover si on a changé de jour UTC
    if status.date != today_utc() {
        QuotaStatus::default()
    } else {
        status
    }
}

fn save(app_dir: &PathBuf, status: &QuotaStatus) -> Result<(), String> {
    fs::create_dir_all(app_dir).map_err(|e| e.to_string())?;
    let json = serde_json::to_string_pretty(status).map_err(|e| e.to_string())?;
    fs::write(quota_path(app_dir), json).map_err(|e| e.to_string())
}

pub enum CheckResult {
    Ok,                       // < WARN_THRESHOLD
    WarnCrossed(QuotaStatus), // vient de dépasser WARN_THRESHOLD (une seule fois par jour)
    Blocked(QuotaStatus),     // >= DAILY_LIMIT
}

/// À appeler avant chaque requête Groq. Renvoie Blocked si on a atteint la limite,
/// WarnCrossed la première fois qu'on franchit 1500 aujourd'hui, sinon Ok.
/// Incrémente le compteur si non-bloqué.
pub fn check_and_increment(app_dir: &PathBuf) -> CheckResult {
    let mut status = load(app_dir);

    if status.used >= DAILY_LIMIT {
        return CheckResult::Blocked(status);
    }

    status.used += 1;
    let just_crossed_warn = !status.warned && status.used >= WARN_THRESHOLD;
    if just_crossed_warn {
        status.warned = true;
    }

    if let Err(e) = save(app_dir, &status) {
        eprintln!("[brain-dump] Failed to save quota: {}", e);
    }

    if just_crossed_warn {
        CheckResult::WarnCrossed(status)
    } else {
        CheckResult::Ok
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env::temp_dir;

    #[test]
    fn test_default_is_today_zero() {
        let q = QuotaStatus::default();
        assert_eq!(q.used, 0);
        assert_eq!(q.limit, DAILY_LIMIT);
        assert!(!q.warned);
    }

    #[test]
    fn test_increment_and_load() {
        let dir = temp_dir().join("brain_dump_test_quota");
        let _ = fs::remove_dir_all(&dir);

        match check_and_increment(&dir) {
            CheckResult::Ok => {}
            _ => panic!("first call should be Ok"),
        }

        let status = load(&dir);
        assert_eq!(status.used, 1);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_blocked_at_limit() {
        let dir = temp_dir().join("brain_dump_test_quota_blocked");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let mut status = QuotaStatus::default();
        status.used = DAILY_LIMIT;
        save(&dir, &status).unwrap();

        match check_and_increment(&dir) {
            CheckResult::Blocked(_) => {}
            _ => panic!("should be blocked at limit"),
        }

        let _ = fs::remove_dir_all(&dir);
    }
}
