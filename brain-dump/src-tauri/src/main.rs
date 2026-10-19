#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::Mutex;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, PhysicalPosition, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_updater::UpdaterExt;

use brain_dump_lib::audio;
use brain_dump_lib::history;
use brain_dump_lib::quota;
use brain_dump_lib::recorder::{failed_recording_path, Recorder, RecordingState, TranscribeMode};
use brain_dump_lib::settings::Settings;
use brain_dump_lib::stats;
use brain_dump_lib::vocab;

struct AppState {
    recorder: Recorder,
    settings: Mutex<Settings>,
    app_dir: PathBuf,
}

const OVERLAY_MARGIN: i32 = 12;

fn overlay_pos_path(app_dir: &PathBuf) -> PathBuf {
    app_dir.join("overlay_pos.json")
}

fn load_overlay_pos(app_dir: &PathBuf) -> Option<(i32, i32)> {
    let raw = std::fs::read_to_string(overlay_pos_path(app_dir)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    Some((v["x"].as_i64()? as i32, v["y"].as_i64()? as i32))
}

fn save_overlay_pos(app_dir: &PathBuf, x: i32, y: i32) {
    let _ = std::fs::write(
        overlay_pos_path(app_dir),
        serde_json::json!({ "x": x, "y": y }).to_string(),
    );
}

/// Physical rect (x, y, w, h) of a monitor.
type Rect = (i32, i32, i32, i32);

fn center_is_on_a_monitor(pos: (i32, i32), size: (i32, i32), monitors: &[Rect]) -> bool {
    let (cx, cy) = (pos.0 + size.0 / 2, pos.1 + size.1 / 2);
    monitors
        .iter()
        .any(|&(x, y, w, h)| cx >= x && cx < x + w && cy >= y && cy < y + h)
}

/// Puts the overlay back in the top-right corner of the primary screen if it ended up
/// off every connected screen (monitor unplugged, resolution change, saved position stale).
fn ensure_overlay_on_screen(overlay: &tauri::WebviewWindow) {
    let (Ok(pos), Ok(size), Ok(monitors)) = (
        overlay.outer_position(),
        overlay.outer_size(),
        overlay.available_monitors(),
    ) else {
        return;
    };
    let rects: Vec<Rect> = monitors
        .iter()
        .map(|m| {
            let (p, s) = (m.position(), m.size());
            (p.x, p.y, s.width as i32, s.height as i32)
        })
        .collect();
    let size = (size.width as i32, size.height as i32);
    if rects.is_empty() || center_is_on_a_monitor((pos.x, pos.y), size, &rects) {
        return;
    }
    println!("[brain-dump] Overlay off-screen at {:?}, moving it back", pos);
    move_overlay_to_default(overlay);
}

/// Top-right corner of the primary screen, fully inside it.
fn move_overlay_to_default(overlay: &tauri::WebviewWindow) {
    let (Ok(Some(m)), Ok(size)) = (overlay.primary_monitor(), overlay.outer_size()) else {
        return;
    };
    let (p, s) = (m.position(), m.size());
    let x = p.x + s.width as i32 - size.width as i32 - OVERLAY_MARGIN;
    let _ = overlay.set_position(PhysicalPosition::new(x, p.y + OVERLAY_MARGIN));
}

const UPDATE_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(6 * 3600);

/// Downloads a newer release from GitHub (if any) and installs it once nobody is dictating.
/// User data lives in the config dir, untouched by the installer, so nothing is lost.
async fn update_if_available(app: &tauri::AppHandle) -> Result<(), String> {
    let Some(update) = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    println!("[brain-dump] Update {} found, downloading", update.version);
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;

    let state = app.state::<AppState>();
    while state.recorder.get_state() != RecordingState::Ready {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    }
    println!("[brain-dump] Installing update {}", update.version);
    update.install(bytes).map_err(|e| e.to_string())?;
    app.restart();
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
    do_toggle_recording(&app, &state, TranscribeMode::DbPaste).await
}

#[tauri::command]
fn get_quota_status(state: State<AppState>) -> quota::QuotaStatus {
    quota::load(&state.app_dir)
}

#[tauri::command]
fn get_history(state: State<AppState>) -> Vec<history::Entry> {
    history::load(&state.app_dir)
}

#[tauri::command]
fn get_stats(state: State<AppState>) -> stats::Stats {
    stats::load(&state.app_dir)
}

#[tauri::command]
fn clear_history(state: State<AppState>) -> Result<(), String> {
    history::clear(&state.app_dir)
}

#[tauri::command]
fn copy_text(text: String) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text))
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn has_failed_recording(state: State<AppState>) -> bool {
    failed_recording_path(&state.app_dir).exists()
}

#[tauri::command]
async fn retry_failed_recording(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let settings = state.settings.lock().unwrap().clone();
    state.recorder.retry_failed(&app, &settings, &state.app_dir).await
}

#[tauri::command]
fn discard_failed_recording(state: State<AppState>) {
    let _ = std::fs::remove_file(failed_recording_path(&state.app_dir));
}

#[derive(serde::Serialize)]
struct EditResult {
    entry: history::Entry,
    suggestions: Vec<vocab::Suggestion>,
}

/// Enregistre la version éditée et propose les mots à ajouter au vocabulaire.
#[tauri::command]
fn edit_history_entry(state: State<AppState>, id: i64, text: String) -> Result<EditResult, String> {
    let entry = history::edit(&state.app_dir, id, &text)?;
    let settings = state.settings.lock().unwrap();
    let known: Vec<String> = settings
        .vocabulary
        .split([',', '\n'])
        .chain(settings.ignored_suggestions.iter().map(String::as_str))
        .map(|w| w.trim().to_lowercase())
        .collect();
    let suggestions = vocab::suggestions(&entry.text, &text)
        .into_iter()
        .filter(|s| !known.contains(&s.to.to_lowercase()))
        .collect();
    Ok(EditResult { entry, suggestions })
}

#[tauri::command]
fn get_overlay_visible(app: tauri::AppHandle) -> bool {
    app.get_webview_window("overlay")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

#[tauri::command]
fn set_overlay_visible(app: tauri::AppHandle, visible: bool) -> Result<(), String> {
    let w = app
        .get_webview_window("overlay")
        .ok_or("Overlay window not found")?;
    if visible {
        ensure_overlay_on_screen(&w);
        w.show().map_err(|e| e.to_string())?;
    } else {
        w.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn reposition_overlay(app: tauri::AppHandle) -> Result<(), String> {
    let w = app
        .get_webview_window("overlay")
        .ok_or("Overlay window not found")?;
    ensure_overlay_on_screen(&w);
    w.show().map_err(|e| e.to_string())?;
    w.emit("start-reposition", ()).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

/// Passed by the Windows startup entry: the app starts silently in the tray.
const AUTOSTART_FLAG: &str = "--autostart";

#[tauri::command]
fn get_autostart(app: tauri::AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
fn set_autostart(app: tauri::AppHandle, enabled: bool) -> Result<(), String> {
    let launcher = app.autolaunch();
    if enabled { launcher.enable() } else { launcher.disable() }.map_err(|e| e.to_string())
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

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
        "command" => TranscribeMode::Command,
        _ => return Err(format!("Unknown slot: {}", slot)),
    };

    let old_accelerator = {
        let settings = state.settings.lock().unwrap();
        match mode {
            TranscribeMode::PasteOnly => settings.hotkey_paste_only.clone(),
            TranscribeMode::DbPaste => settings.hotkey_db_paste.clone(),
            TranscribeMode::Command => settings.hotkey_command.clone(),
        }
    };

    let _ = app.global_shortcut().unregister(old_accelerator.as_str());

    register_hotkey_handle(&app, &accelerator, mode)
        .map_err(|e| format!("Failed to register hotkey: {}", e))?;

    {
        let mut settings = state.settings.lock().unwrap();
        match mode {
            TranscribeMode::PasteOnly => settings.hotkey_paste_only = accelerator,
            TranscribeMode::DbPaste => settings.hotkey_db_paste = accelerator,
            TranscribeMode::Command => settings.hotkey_command = accelerator,
        }
        settings.save(&state.app_dir)?;
    }

    Ok(())
}

async fn do_toggle_recording(
    app: &tauri::AppHandle,
    state: &AppState,
    mode: TranscribeMode,
) -> Result<String, String> {
    let current_state = state.recorder.get_state();
    match current_state {
        RecordingState::Ready => {
            let mic = state.settings.lock().unwrap().microphone.clone();
            state.recorder.start_recording(app, &mic, mode)?;
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
        RecordingState::Transcribing => Err("Currently transcribing, please wait".to_string()),
    }
}

fn main() {
    let app_dir = get_app_dir();
    let settings = Settings::load(&app_dir);
    let hotkey_paste_only = settings.hotkey_paste_only.clone();
    let hotkey_db_paste = settings.hotkey_db_paste.clone();
    let hotkey_command = settings.hotkey_command.clone();

    tauri::Builder::default()
        // Relancer Brain Dump alors qu'il tourne déjà dans la zone de notification rouvre la fenêtre
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main_window(app)))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![AUTOSTART_FLAG]),
        ))
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
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
            get_history,
            get_stats,
            clear_history,
            copy_text,
            edit_history_entry,
            has_failed_recording,
            retry_failed_recording,
            discard_failed_recording,
            update_hotkey,
            get_overlay_visible,
            set_overlay_visible,
            reposition_overlay,
            quit_app,
            get_autostart,
            set_autostart,
        ])
        .setup(move |app| {
            // ---- Overlay window ----
            let overlay = WebviewWindowBuilder::new(
                app,
                "overlay",
                WebviewUrl::App("src/overlay.html".into()),
            )
            .title("")
            .inner_size(160.0, 50.0)
            .resizable(false)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .focused(false)
            .shadow(false)
            .visible(false)
            .build();

            match overlay {
                Ok(overlay) => {
                    let app_dir = app.state::<AppState>().app_dir.clone();
                    match load_overlay_pos(&app_dir) {
                        Some((x, y)) => {
                            let _ = overlay.set_position(PhysicalPosition::new(x, y));
                            ensure_overlay_on_screen(&overlay);
                        }
                        None => move_overlay_to_default(&overlay),
                    }
                    let _ = overlay.show();

                    overlay.on_window_event(move |event| {
                        if let WindowEvent::Moved(p) = event {
                            save_overlay_pos(&app_dir, p.x, p.y);
                        }
                    });

                    // Screens can be unplugged at any time: re-check every few seconds.
                    let watched = overlay.clone();
                    std::thread::spawn(move || loop {
                        std::thread::sleep(std::time::Duration::from_secs(3));
                        if watched.is_visible().unwrap_or(false) {
                            ensure_overlay_on_screen(&watched);
                        }
                    });
                    println!("[brain-dump] Overlay window created");
                }
                Err(e) => eprintln!("[brain-dump] Failed to create overlay: {}", e),
            }

            // ---- Main window : X = hide to the tray (keeps overlay + hotkeys alive) ----
            if let Some(main_win) = app.get_webview_window("main") {
                let main_clone = main_win.clone();
                main_win.on_window_event(move |event| {
                    if let WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = main_clone.hide();
                    }
                });
                // Au démarrage de Windows, on reste discret dans la zone de notification
                if !std::env::args().any(|a| a == AUTOSTART_FLAG) {
                    let _ = main_win.show();
                }
            }

            // ---- Tray icon ----
            let open = MenuItem::with_id(app, "open", "Ouvrir Brain Dump", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quitter", true, None::<&str>)?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("Brain Dump")
                .menu(&Menu::with_items(app, &[&open, &quit])?)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "open" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                })
                .build(app)?;

            // ---- Auto-update (release builds only) ----
            if !cfg!(debug_assertions) {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        if let Err(e) = update_if_available(&handle).await {
                            eprintln!("[brain-dump] Update check failed: {}", e);
                        }
                        tokio::time::sleep(UPDATE_CHECK_INTERVAL).await;
                    }
                });
            }

            // ---- Hotkeys ----
            println!(
                "[brain-dump] Registering hotkey (paste-only): {}",
                hotkey_paste_only
            );
            if let Err(e) = register_hotkey_app(app, &hotkey_paste_only, TranscribeMode::PasteOnly)
            {
                eprintln!("[brain-dump] ERROR registering paste-only hotkey: {}", e);
            }

            println!(
                "[brain-dump] Registering hotkey (db+paste): {}",
                hotkey_db_paste
            );
            if let Err(e) = register_hotkey_app(app, &hotkey_db_paste, TranscribeMode::DbPaste) {
                eprintln!("[brain-dump] ERROR registering db+paste hotkey: {}", e);
            }

            println!("[brain-dump] Registering hotkey (command): {}", hotkey_command);
            if let Err(e) = register_hotkey_app(app, &hotkey_command, TranscribeMode::Command) {
                eprintln!("[brain-dump] ERROR registering command hotkey: {}", e);
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
    handle
        .global_shortcut()
        .on_shortcut(accelerator, move |_app, shortcut, event| {
            println!(
                "[brain-dump] Hotkey event ({:?}): {:?} state={:?}",
                mode, shortcut, event.state
            );
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
                                    if let Err(e) = state.recorder.start_recording(&handle, &mic, mode) {
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

#[cfg(test)]
mod tests {
    use super::*;

    const SCREENS: [Rect; 2] = [(0, 0, 1920, 1080), (1920, 0, 2560, 1440)];

    #[test]
    fn test_overlay_on_primary_or_secondary() {
        assert!(center_is_on_a_monitor((1700, 12), (240, 75), &SCREENS));
        assert!(center_is_on_a_monitor((4000, 12), (240, 75), &SCREENS));
    }

    #[test]
    fn test_overlay_on_unplugged_screen() {
        // Saved on the 2nd screen, which is now gone
        assert!(!center_is_on_a_monitor((4000, 12), (240, 75), &SCREENS[..1]));
        // Screen to the left of the primary, now gone
        assert!(!center_is_on_a_monitor((-1500, 300), (240, 75), &SCREENS));
    }
}
