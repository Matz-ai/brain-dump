use std::time::{Duration, Instant};

/// What the clipboard held before command mode borrowed it.
pub enum ClipboardBackup {
    Text(String),
    Image(arboard::ImageData<'static>),
    Empty,
}

pub fn backup_clipboard() -> ClipboardBackup {
    let Ok(mut clipboard) = arboard::Clipboard::new() else {
        return ClipboardBackup::Empty;
    };
    if let Ok(text) = clipboard.get_text() {
        return ClipboardBackup::Text(text);
    }
    match clipboard.get_image() {
        Ok(image) => ClipboardBackup::Image(image),
        Err(_) => ClipboardBackup::Empty,
    }
}

pub fn restore_clipboard(backup: ClipboardBackup) {
    let Ok(mut clipboard) = arboard::Clipboard::new() else {
        return;
    };
    let result = match backup {
        ClipboardBackup::Text(text) => clipboard.set_text(text),
        ClipboardBackup::Image(image) => clipboard.set_image(image),
        ClipboardBackup::Empty => clipboard.clear(),
    };
    if let Err(e) = result {
        eprintln!("[brain-dump] Clipboard restore failed: {}", e);
    }
}

/// Restores the backup only if the clipboard still holds what we pasted: anything else means
/// the user copied something meanwhile, and that wins.
pub fn restore_clipboard_if_holds(backup: ClipboardBackup, pasted: &str) {
    let holds_pasted = arboard::Clipboard::new()
        .and_then(|mut c| c.get_text())
        .map_or(false, |text| text == pasted);
    if holds_pasted {
        restore_clipboard(backup);
    }
}

/// Copies the current selection of the active app (simulated Ctrl+C) and returns it,
/// or an empty string when nothing is selected. Overwrites the clipboard: back it up first.
pub fn copy_selection() -> Result<String, String> {
    // Ctrl+C with Shift still held down would be Ctrl+Shift+C (devtools in browsers).
    wait_for_modifiers_release(Duration::from_millis(1500));

    let mut clipboard = arboard::Clipboard::new().map_err(|e| e.to_string())?;
    clipboard.clear().map_err(|e| e.to_string())?;
    send_copy()?;

    // The app fills the clipboard asynchronously; nothing after a short wait means no selection.
    let deadline = Instant::now() + Duration::from_millis(400);
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(25));
        if let Ok(text) = clipboard.get_text() {
            return Ok(text);
        }
    }
    Ok(String::new())
}

#[cfg(target_os = "windows")]
fn wait_for_modifiers_release(max: Duration) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    let deadline = Instant::now() + max;
    while Instant::now() < deadline {
        let held = [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
            .iter()
            .any(|k| unsafe { GetAsyncKeyState(k.0 as i32) } as u16 & 0x8000 != 0);
        if !held {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(not(target_os = "windows"))]
fn wait_for_modifiers_release(max: Duration) {
    std::thread::sleep(max.min(Duration::from_millis(300)));
}

fn send_copy() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("osascript")
            .args(["-e", r#"tell application "System Events" to keystroke "c" using command down"#])
            .output()
            .map_err(|e| format!("Failed to simulate copy: {}", e))?;
    }

    #[cfg(target_os = "windows")]
    {
        use enigo::{Direction, Enigo, Key, Keyboard, Settings};
        let mut enigo = Enigo::new(&Settings::default()).map_err(|e| e.to_string())?;
        enigo.key(Key::Control, Direction::Press).map_err(|e| e.to_string())?;
        enigo.key(Key::Unicode('c'), Direction::Click).map_err(|e| e.to_string())?;
        enigo.key(Key::Control, Direction::Release).map_err(|e| e.to_string())?;
    }

    Ok(())
}
