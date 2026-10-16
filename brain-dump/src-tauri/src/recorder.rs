use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter};

use crate::audio::AudioRecorder;
use crate::cleanup::cleanup_text;
use crate::context;
use crate::correct_groq;
use crate::history;
use crate::paste::paste_text;
use crate::quota::{self, CheckResult};
use crate::settings::Settings;
use crate::snippets;
use crate::supabase;
use crate::transcribe_groq;
use crate::vocab;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub enum RecordingState {
    Ready,
    Recording,
    Transcribing,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TranscribeMode {
    PasteOnly, // paste dans l'app active, PAS de DB (info sensible)
    DbPaste,   // DB + paste dans l'app active
}

impl TranscribeMode {
    fn source_label(&self) -> &'static str {
        match self {
            TranscribeMode::PasteOnly => "desktop_paste_only",
            TranscribeMode::DbPaste => "desktop_db_paste",
        }
    }

    fn save_to_db(&self) -> bool {
        matches!(self, TranscribeMode::DbPaste)
    }
}

pub struct Recorder {
    state: Arc<Mutex<RecordingState>>,
    audio_recorder: Arc<Mutex<AudioRecorder>>,
    started_at: Mutex<Option<Instant>>,
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RecordingState::Ready)),
            audio_recorder: Arc::new(Mutex::new(AudioRecorder::new())),
            started_at: Mutex::new(None),
        }
    }

    pub fn get_state(&self) -> RecordingState {
        self.state.lock().unwrap().clone()
    }

    pub fn start_recording(&self, app: &AppHandle, mic_name: &str) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        if *state != RecordingState::Ready {
            return Err("Already recording or transcribing".to_string());
        }

        let mut recorder = self.audio_recorder.lock().unwrap();
        recorder.start(mic_name)?;
        *self.started_at.lock().unwrap() = Some(Instant::now());

        *state = RecordingState::Recording;
        let _ = app.emit("recording-state", RecordingState::Recording);
        Ok(())
    }

    pub async fn stop_and_transcribe(
        &self,
        app: &AppHandle,
        settings: &Settings,
        app_dir: &PathBuf,
        mode: TranscribeMode,
    ) -> Result<String, String> {
        // Transition state → Transcribing
        {
            let mut state = self.state.lock().unwrap();
            if *state != RecordingState::Recording {
                return Err("Not currently recording".to_string());
            }
            *state = RecordingState::Transcribing;
            let _ = app.emit("recording-state", RecordingState::Transcribing);
        }

        let temp_path = app_dir.join("temp_recording.wav");

        let reset_state = || {
            let mut state = self.state.lock().unwrap();
            *state = RecordingState::Ready;
            let _ = app.emit("recording-state", RecordingState::Ready);
        };

        let duration_ms = self
            .started_at
            .lock()
            .unwrap()
            .take()
            .map_or(0, |t| t.elapsed().as_millis() as u64);

        // Sauvegarde du WAV (silences coupés). Rien de dit → pas d'appel API.
        let saved = self.audio_recorder.lock().unwrap().stop_and_save(&temp_path);
        match saved {
            Ok(true) => {}
            Ok(false) => {
                let _ = app.emit("nothing-heard", ());
                reset_state();
                return Ok(String::new());
            }
            Err(e) => {
                reset_state();
                return Err(e);
            }
        }

        // Quota check AVANT l'appel API pour éviter une requête perdue
        match quota::check_and_increment(app_dir) {
            CheckResult::Ok => {}
            CheckResult::WarnCrossed(status) => {
                let _ = app.emit("quota-warning", &status);
                eprintln!(
                    "[brain-dump] Groq quota 3/4 atteint : {}/{}",
                    status.used, status.limit
                );
            }
            CheckResult::Blocked(status) => {
                let _ = app.emit("quota-blocked", &status);
                let _ = std::fs::remove_file(&temp_path);
                reset_state();
                return Err(format!(
                    "Groq free tier épuisé pour aujourd'hui ({}/{}). Reset à minuit UTC.",
                    status.used, status.limit
                ));
            }
        }

        let raw_text = transcribe_groq::transcribe_groq(
            &settings.groq_api_key,
            &temp_path,
            &settings.language,
            &settings.whisper_model,
            &vocab::whisper_prompt(&settings.vocabulary),
        )
        .await;

        let _ = std::fs::remove_file(&temp_path);

        let raw_text = match raw_text {
            Ok(t) => t,
            Err(e) => {
                reset_state();
                return Err(e);
            }
        };

        // Corrections apprises (« super base » → « Supabase »), sans appel réseau
        let text = vocab::apply_replacements(&raw_text, &settings.replacements);

        // Correction IA optionnelle : en cas d'échec on garde la transcription brute
        let text = if settings.ai_correction && !text.trim().is_empty() {
            match correct_groq::correct_text(&settings.groq_api_key, &text, &settings.vocabulary).await {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("[brain-dump] AI correction skipped: {}", e);
                    text
                }
            }
        } else {
            text
        };

        // Raccourcis vocaux après le cleanup : le texte du snippet est collé tel quel
        let cleaned = snippets::expand(&cleanup_text(&text), &settings.snippets);

        if cleaned.is_empty() {
            let _ = app.emit("nothing-heard", ());
            reset_state();
            return Ok(String::new());
        }

        // Insert Supabase — uniquement en mode DbPaste, non-bloquant
        if mode.save_to_db() {
            let ctx = if settings.capture_context {
                Some(context::capture_active_context())
            } else {
                None
            };
            if let Err(e) = supabase::insert_note(
                &settings.supabase_url,
                &settings.supabase_anon_key,
                &cleaned,
                mode.source_label(),
                ctx,
            )
            .await
            {
                eprintln!("[brain-dump] Supabase insert failed: {}", e);
            }
        }

        // Paste — toujours (les 2 modes pastent)
        match paste_text(&cleaned) {
            Ok(_) => {
                let _ = app.emit("paste-done", &cleaned);
            }
            Err(e) => {
                eprintln!("[brain-dump] Paste failed: {}", e);
                let _ = app.emit("paste-failed", &e);
            }
        }

        // Historique hors du chemin critique : le collage est déjà fait
        if settings.keep_history {
            let (app, app_dir, text) = (app.clone(), app_dir.clone(), cleaned.clone());
            let capture_context = settings.capture_context;
            tauri::async_runtime::spawn_blocking(move || {
                let target_app = if capture_context {
                    context::capture_active_context()["app"]
                        .as_str()
                        .filter(|a| !a.is_empty())
                        .map(str::to_string)
                } else {
                    None
                };
                let now = chrono::Local::now();
                let entry = history::Entry {
                    id: now.timestamp_millis(),
                    date: now.to_rfc3339(),
                    text,
                    edited: None,
                    raw: raw_text,
                    app: target_app,
                    duration_ms,
                };
                match history::add(&app_dir, entry.clone()) {
                    Ok(_) => {
                        let _ = app.emit("history-added", &entry);
                    }
                    Err(e) => eprintln!("[brain-dump] History save failed: {}", e),
                }
            });
        }

        reset_state();
        Ok(cleaned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state_is_ready() {
        let recorder = Recorder::new();
        assert_eq!(recorder.get_state(), RecordingState::Ready);
    }

    #[test]
    fn test_mode_source_label() {
        assert_eq!(TranscribeMode::PasteOnly.source_label(), "desktop_paste_only");
        assert_eq!(TranscribeMode::DbPaste.source_label(), "desktop_db_paste");
    }

    #[test]
    fn test_mode_save_to_db() {
        assert!(!TranscribeMode::PasteOnly.save_to_db());
        assert!(TranscribeMode::DbPaste.save_to_db());
    }
}
