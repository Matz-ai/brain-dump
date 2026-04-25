use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

use crate::audio::AudioRecorder;
use crate::cleanup::cleanup_text;
use crate::context;
use crate::paste::paste_text;
use crate::quota::{self, CheckResult};
use crate::settings::Settings;
use crate::supabase;
use crate::transcribe_groq;

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
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RecordingState::Ready)),
            audio_recorder: Arc::new(Mutex::new(AudioRecorder::new())),
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

        // Sauvegarde du WAV
        {
            let mut recorder = self.audio_recorder.lock().unwrap();
            recorder.stop_and_save(&temp_path)?;
        }

        let reset_state = || {
            let mut state = self.state.lock().unwrap();
            *state = RecordingState::Ready;
            let _ = app.emit("recording-state", RecordingState::Ready);
        };

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
            &settings.vocabulary,
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

        let cleaned = cleanup_text(&raw_text);

        if cleaned.is_empty() {
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
