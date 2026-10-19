use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tauri::{AppHandle, Emitter};

use crate::audio::AudioRecorder;
use crate::cleanup::cleanup_text;
use crate::command_groq;
use crate::context;
use crate::correct_groq;
use crate::history;
use crate::paste::paste_text;
use crate::quota::{self, CheckResult};
use crate::selection;
use crate::settings::Settings;
use crate::snippets;
use crate::stats;
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
    Command,   // instruction vocale appliquée à la sélection, PAS de DB
}

impl TranscribeMode {
    fn source_label(&self) -> &'static str {
        match self {
            TranscribeMode::PasteOnly => "desktop_paste_only",
            TranscribeMode::DbPaste => "desktop_db_paste",
            TranscribeMode::Command => "desktop_command",
        }
    }

    fn save_to_db(&self) -> bool {
        matches!(self, TranscribeMode::DbPaste)
    }
}

/// The stop key decides between the two dictation modes (as before), but a command started
/// with the command key stays a command, and a dictation is never turned into one.
fn effective_mode(started: Option<TranscribeMode>, requested: TranscribeMode) -> TranscribeMode {
    match started {
        Some(TranscribeMode::Command) => TranscribeMode::Command,
        Some(started) if requested == TranscribeMode::Command => started,
        _ => requested,
    }
}

pub struct Recorder {
    state: Arc<Mutex<RecordingState>>,
    audio_recorder: Arc<Mutex<AudioRecorder>>,
    started_at: Mutex<Option<Instant>>,
    started_mode: Mutex<Option<TranscribeMode>>,
}

impl Recorder {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(RecordingState::Ready)),
            audio_recorder: Arc::new(Mutex::new(AudioRecorder::new())),
            started_at: Mutex::new(None),
            started_mode: Mutex::new(None),
        }
    }

    pub fn get_state(&self) -> RecordingState {
        self.state.lock().unwrap().clone()
    }

    pub fn start_recording(
        &self,
        app: &AppHandle,
        mic_name: &str,
        mode: TranscribeMode,
    ) -> Result<(), String> {
        let mut state = self.state.lock().unwrap();
        if *state != RecordingState::Ready {
            return Err("Already recording or transcribing".to_string());
        }

        let mut recorder = self.audio_recorder.lock().unwrap();
        recorder.start(mic_name)?;
        *self.started_at.lock().unwrap() = Some(Instant::now());
        *self.started_mode.lock().unwrap() = Some(mode);

        // Le voyant affiche « Commande… » au lieu de « J'écoute… »
        let kind = if mode == TranscribeMode::Command { "command" } else { "dictation" };
        let _ = app.emit("recording-mode", kind);
        *state = RecordingState::Recording;
        let _ = app.emit("recording-state", RecordingState::Recording);

        // Niveau micro en direct pour le voyant, tant que l'enregistrement dure
        let (state_ref, audio, app) = (self.state.clone(), self.audio_recorder.clone(), app.clone());
        std::thread::spawn(move || {
            while *state_ref.lock().unwrap() == RecordingState::Recording {
                let level = audio.lock().unwrap().level();
                let _ = app.emit("mic-level", level);
                std::thread::sleep(std::time::Duration::from_millis(60));
            }
        });
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
        let mode = effective_mode(self.started_mode.lock().unwrap().take(), mode);

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

        if mode == TranscribeMode::Command {
            let result = self.run_command(app, settings, &temp_path).await;
            let _ = std::fs::remove_file(&temp_path);
            reset_state();
            return result;
        }

        let raw_text = transcribe_groq::transcribe_groq(
            &settings.groq_api_key,
            &temp_path,
            &settings.language,
            &settings.whisper_model,
            &vocab::whisper_prompt(&settings.vocabulary),
        )
        .await;

        let raw_text = match raw_text {
            Ok(t) => {
                let _ = std::fs::remove_file(&temp_path);
                t
            }
            Err(e) => {
                // L'audio est gardé pour « Réessayer » dans l'historique au lieu d'être perdu
                let kept = std::fs::rename(&temp_path, failed_recording_path(app_dir)).is_ok();
                let _ = app.emit("transcription-failed", kept);
                reset_state();
                return Err(e);
            }
        };

        let cleaned = finish_text(settings, &raw_text).await;

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

        // Paste — toujours (les 2 modes pastent). Le presse-papiers d'origine est rendu ensuite ;
        // si le collage échoue, la dictée y reste pour un Ctrl+V à la main.
        let backup = selection::backup_clipboard();
        match paste_text(&cleaned) {
            Ok(_) => {
                let _ = app.emit("paste-done", &cleaned);
                let pasted = cleaned.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    // L'app cible lit le presse-papiers après le Ctrl+V : on attend avant de le restaurer
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    selection::restore_clipboard_if_holds(backup, &pasted);
                });
            }
            Err(e) => {
                eprintln!("[brain-dump] Paste failed: {}", e);
                let _ = app.emit("paste-failed", &e);
            }
        }

        // Historique et stats hors du chemin critique : le collage est déjà fait
        record_stats(app, app_dir, cleaned.clone(), duration_ms);
        if settings.keep_history {
            save_history(app, app_dir, cleaned.clone(), raw_text, duration_ms, settings.capture_context);
        }

        reset_state();
        Ok(cleaned)
    }

    /// Retranscrit la dernière dictée dont la transcription a échoué. On ne sait plus où la
    /// coller : le texte est copié dans le presse-papiers et ajouté à l'historique.
    /// En cas de nouvel échec, l'audio reste disponible.
    pub async fn retry_failed(
        &self,
        app: &AppHandle,
        settings: &Settings,
        app_dir: &PathBuf,
    ) -> Result<String, String> {
        let path = failed_recording_path(app_dir);
        if !path.exists() {
            return Err("Aucune dictée en attente".to_string());
        }
        if let CheckResult::Blocked(status) = quota::check_and_increment(app_dir) {
            return Err(format!(
                "Groq free tier épuisé pour aujourd'hui ({}/{}). Reset à minuit UTC.",
                status.used, status.limit
            ));
        }

        let raw_text = transcribe_groq::transcribe_groq(
            &settings.groq_api_key,
            &path,
            &settings.language,
            &settings.whisper_model,
            &vocab::whisper_prompt(&settings.vocabulary),
        )
        .await?;
        let _ = std::fs::remove_file(&path);

        let cleaned = finish_text(settings, &raw_text).await;
        if cleaned.is_empty() {
            return Ok(String::new());
        }
        arboard::Clipboard::new()
            .and_then(|mut c| c.set_text(cleaned.clone()))
            .map_err(|e| e.to_string())?;
        if settings.keep_history {
            save_history(app, app_dir, cleaned.clone(), raw_text, 0, false);
        }
        Ok(cleaned)
    }

    /// Mode commande : la sélection est copiée pendant que l'instruction est transcrite (sans
    /// correction IA), puis remplacée par la réponse du LLM. Le presse-papiers d'origine est
    /// toujours restauré ; en cas d'échec le texte sélectionné n'est pas touché.
    async fn run_command(
        &self,
        app: &AppHandle,
        settings: &Settings,
        wav_path: &PathBuf,
    ) -> Result<String, String> {
        let copy = tauri::async_runtime::spawn_blocking(|| {
            let backup = selection::backup_clipboard();
            (backup, selection::copy_selection())
        });
        let instruction = transcribe_groq::transcribe_groq(
            &settings.groq_api_key,
            wav_path,
            &settings.language,
            &settings.whisper_model,
            &vocab::whisper_prompt(&settings.vocabulary),
        )
        .await;
        let (backup, selected) = copy.await.map_err(|e| e.to_string())?;

        let outcome = async {
            let instruction = instruction.map_err(|e| {
                eprintln!("[brain-dump] Command transcription failed: {}", e);
                "Transcription échouée".to_string()
            })?;
            let instruction = vocab::apply_replacements(&instruction, &settings.replacements);
            if instruction.trim().is_empty() {
                return Ok(None);
            }
            let selected = selected.map_err(|e| {
                eprintln!("[brain-dump] Copy selection failed: {}", e);
                "Impossible de lire la sélection".to_string()
            })?;
            println!(
                "[brain-dump] Command: {:?} on {} chars",
                instruction,
                selected.chars().count()
            );
            command_groq::run_command(&settings.groq_api_key, &instruction, &selected)
                .await
                .map(Some)
        }
        .await;

        match outcome {
            Ok(Some(result)) => {
                let pasted = result.clone();
                let paste = tauri::async_runtime::spawn_blocking(move || {
                    let res = paste_text(&pasted);
                    // L'app cible lit le presse-papiers après le Ctrl+V : on attend avant de le restaurer
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    selection::restore_clipboard(backup);
                    res
                })
                .await
                .map_err(|e| e.to_string())?;
                match paste {
                    Ok(_) => {
                        let _ = app.emit("paste-done", &result);
                        Ok(result)
                    }
                    Err(e) => {
                        let _ = app.emit("paste-failed", &e);
                        Err(e)
                    }
                }
            }
            Ok(None) => {
                selection::restore_clipboard(backup);
                let _ = app.emit("nothing-heard", ());
                Ok(String::new())
            }
            Err(e) => {
                selection::restore_clipboard(backup);
                let _ = app.emit("command-failed", &e);
                Err(e)
            }
        }
    }
}

pub fn failed_recording_path(app_dir: &PathBuf) -> PathBuf {
    app_dir.join("failed_recording.wav")
}

/// Corrections apprises, correction IA optionnelle, cleanup puis raccourcis vocaux.
async fn finish_text(settings: &Settings, raw_text: &str) -> String {
    // Corrections apprises (« super base » → « Supabase »), sans appel réseau
    let text = vocab::apply_replacements(raw_text, &settings.replacements);

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
    snippets::expand(&cleanup_text(&text), &settings.snippets)
}

/// Écrit l'entrée d'historique en tâche de fond et prévient l'UI.
fn record_stats(app: &AppHandle, app_dir: &PathBuf, text: String, duration_ms: u64) {
    let (app, app_dir) = (app.clone(), app_dir.clone());
    tauri::async_runtime::spawn_blocking(move || {
        let day = chrono::Local::now().format("%Y-%m-%d").to_string();
        match stats::record(&app_dir, &day, &text, duration_ms) {
            Ok(stats) => {
                let _ = app.emit("stats-updated", &stats);
            }
            Err(e) => eprintln!("[brain-dump] Stats save failed: {}", e),
        }
    });
}

fn save_history(
    app: &AppHandle,
    app_dir: &PathBuf,
    text: String,
    raw_text: String,
    duration_ms: u64,
    capture_context: bool,
) {
    let (app, app_dir) = (app.clone(), app_dir.clone());
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
        assert_eq!(TranscribeMode::Command.source_label(), "desktop_command");
    }

    #[test]
    fn test_mode_save_to_db() {
        assert!(!TranscribeMode::PasteOnly.save_to_db());
        assert!(TranscribeMode::DbPaste.save_to_db());
        assert!(!TranscribeMode::Command.save_to_db());
    }

    #[test]
    fn test_effective_mode() {
        use TranscribeMode::*;
        // Comportement existant : la touche d'arrêt choisit entre les deux dictées
        assert_eq!(effective_mode(Some(PasteOnly), DbPaste), DbPaste);
        assert_eq!(effective_mode(Some(DbPaste), PasteOnly), PasteOnly);
        // Une commande reste une commande, une dictée ne devient jamais une commande
        assert_eq!(effective_mode(Some(Command), PasteOnly), Command);
        assert_eq!(effective_mode(Some(DbPaste), Command), DbPaste);
        assert_eq!(effective_mode(None, Command), Command);
    }
}
