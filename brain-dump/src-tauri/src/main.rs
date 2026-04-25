#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use brain_dump_lib::audio;
use brain_dump_lib::quota;
use brain_dump_lib::recorder::{Recorder, RecordingState, TranscribeMode};
use brain_dump_lib::settings::Settings;

struct AppState {
    recorder: Recorder,
    settings: Mutex<Settings>,
    app_dir: PathBuf,
}

fn get_app_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("com.brain-dump.app")
}

#[tauri::command]
fn get_settings(state: State<AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
fn save_settings(state: State<AppState>, settings: Settings) -> Result<(), String> {
    settings.save(&state.app_dir)?;
    *state.settings.lock().unwrap() = settings;
    Ok(())
}

#[tauri::command]
fn list_microphones() -> Vec<audio::MicDevice> {
    audio::list_microphones()
}

#[tauri::command]
fn get_recording_state(state: State<AppState>) -> RecordingState {
    state.recorder.get_state()
}

#[tauri::command]
async fn toggle_recording(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    // Command déclenché depuis l'UI = mode DbPaste par défaut
    do_toggle_recording(&app, &state, TranscribeMode::DbPaste).await
}

#[tauri::command]
fn get_quota_status(state: State<AppState>) -> quota::QuotaStatus {
    quota::load(&state.app_dir)
}

/// Réassigne un hotkey : unregister l'ancien, register le nouveau, persist.
/// `slot` : "paste_only" ou "db_paste"
#[tauri::command]
fn update_hotkey(
    app: tauri::AppHandle,
    state: State<AppState>,
    slot: String,
    accelerator: String,
) -> Result<(), String> {
    let mode = match slot.as_str() {
        "paste_only" => TranscribeMode::PasteOnly,
        "db_paste" => TranscribeMode::DbPaste,
        _ => return Err(format!("Unknown slot: {}", slot)),
    };

    // Récup l'ancien accelerator pour ce slot
    let old_accelerator = {
        let settings = state.settings.lock().unwrap();
        match mode {
            TranscribeMode::PasteOnly => settings.hotkey_paste_only.clone(),
            TranscribeMode::DbPaste => settings.hotkey_db_paste.clone(),
        }
    };

    // Unregister l'ancien (ignore l'erreur si pas trouvé)
    let _ = app.global_shortcut().unregister(old_accelerator.as_str());

    // Register le nouveau
    register_hotkey_handle(&app, &accelerator, mode)
        .map_err(|e| format!("Failed to register hotkey: {}", e))?;

    // Persist
    {
        let mut settings = state.settings.lock().unwrap();
        match mode {
            TranscribeMode::PasteOnly => settings.hotkey_paste_only = accelerator,
            TranscribeMode::DbPaste => settings.hotkey_db_paste = accelerator,
        }
        settings.save(&state.app_dir)?;
    }

    Ok(())
}

/// Shared logic pour le toggle recording, utilisé par la command Tauri et les hotkey handlers.
async fn do_toggle_recording(
    app: &tauri::AppHandle,
    state: &AppState,
    mode: TranscribeMode,
) -> Result<String, String> {
    let current_state = state.recorder.get_state();
    match current_state {
        RecordingState::Ready => {
            let mic = state.settings.lock().unwrap().microphone.clone();
            state.recorder.start_recording(app, &mic)?;
            Ok("recording".to_string())
        }
        RecordingState::Recording => {
            let settings = state.settings.lock().unwrap().clone();
            let result = state
                .recorder
                .stop_and_transcribe(app, &settings, &state.app_dir, mode)
                .await?;
            Ok(result)
        }
        RecordingState::Transcribing => {
            Err("Currently transcribing, please wait".to_string())
        }
    }
}

fn main() {
    let app_dir = get_app_dir();
    let settings = Settings::load(&app_dir);
    let hotkey_paste_only = settings.hotkey_paste_only.clone();
    let hotkey_db_paste = settings.hotkey_db_paste.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .manage(AppState {
            recorder: Recorder::new(),
            settings: Mutex::new(settings),
            app_dir,
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            list_microphones,
            get_recording_state,
            toggle_recording,
            get_quota_status,
            update_hotkey,
        ])
        .setup(move |app| {
            // Overlay window (icône mic flottante, top-right)
            let monitor = app.primary_monitor().ok().flatten();
            let (x, y) = if let Some(m) = monitor {
                let size = m.size();
                let scale = m.scale_factor();
                let logical_w = size.width as f64 / scale;
                ((logical_w - 80.0) as i32, 10_i32)
            } else {
                (1380, 10)
            };

            let overlay = WebviewWindowBuilder::new(
                app,
                "overlay",
                WebviewUrl::App("src/overlay.html".into()),
            )
            .title("")
            .inner_size(160.0, 50.0)
            .position(x as f64, y as f64)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .shadow(false)
            .build();

            match overlay {
                Ok(_) => println!("[brain-dump] Overlay window created"),
                Err(e) => eprintln!("[brain-dump] Failed to create overlay: {}", e),
            }

            println!("[brain-dump] Registering hotkey (paste-only): {}", hotkey_paste_only);
            if let Err(e) = register_hotkey_app(app, &hotkey_paste_only, TranscribeMode::PasteOnly) {
                eprintln!("[brain-dump] ERROR registering paste-only hotkey: {}", e);
            }

            println!("[brain-dump] Registering hotkey (db+paste): {}", hotkey_db_paste);
            if let Err(e) = register_hotkey_app(app, &hotkey_db_paste, TranscribeMode::DbPaste) {
                eprintln!("[brain-dump] ERROR registering db+paste hotkey: {}", e);
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn register_hotkey_app(
    app: &tauri::App,
    accelerator: &str,
    mode: TranscribeMode,
) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();
    register_hotkey_handle(&handle, accelerator, mode)
}

fn register_hotkey_handle(
    handle: &tauri::AppHandle,
    accelerator: &str,
    mode: TranscribeMode,
) -> Result<(), Box<dyn std::error::Error>> {
    let handle_for_callback = handle.clone();
    handle.global_shortcut().on_shortcut(accelerator, move |_app, shortcut, event| {
        println!("[brain-dump] Hotkey event ({:?}): {:?} state={:?}", mode, shortcut, event.state);
        let handle = handle_for_callback.clone();
        let state = handle.state::<AppState>();
        let recording_mode = state.settings.lock().unwrap().recording_mode.clone();

        match event.state {
            ShortcutState::Pressed => {
                tauri::async_runtime::spawn(async move {
                    let state = handle.state::<AppState>();
                    match recording_mode.as_str() {
                        "toggle" => {
                            match do_toggle_recording(&handle, state.inner(), mode).await {
                                Ok(result) => println!("[brain-dump] Toggle result: {}", result),
                                Err(e) => eprintln!("[brain-dump] Toggle error: {}", e),
                            }
                        }
                        "push-to-talk" => {
                            let current = state.recorder.get_state();
                            if current == RecordingState::Ready {
                                let mic = state.settings.lock().unwrap().microphone.clone();
                                if let Err(e) = state.recorder.start_recording(&handle, &mic) {
                                    eprintln!("[brain-dump] PTT start error: {}", e);
                                }
                            }
                        }
                        _ => {}
                    }
                });
            }
            ShortcutState::Released => {
                if recording_mode == "push-to-talk" {
                    tauri::async_runtime::spawn(async move {
                        let state = handle.state::<AppState>();
                        let current = state.recorder.get_state();
                        if current == RecordingState::Recording {
                            let settings = state.settings.lock().unwrap().clone();
                            match state
                                .recorder
                                .stop_and_transcribe(&handle, &settings, &state.app_dir, mode)
                                .await
                            {
                                Ok(result) => println!("[brain-dump] Transcription: {}", result),
                                Err(e) => eprintln!("[brain-dump] Transcription error: {}", e),
                            }
                        }
                    });
                }
            }
        }
    })?;
    Ok(())
}
