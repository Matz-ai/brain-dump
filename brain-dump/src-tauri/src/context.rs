use serde_json::{json, Value};

#[cfg(target_os = "windows")]
pub fn capture_active_context() -> Value {
    use windows::Win32::Foundation::{CloseHandle, HWND, MAX_PATH};
    use windows::Win32::System::ProcessStatus::GetModuleBaseNameW;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
    };

    unsafe {
        let hwnd: HWND = GetForegroundWindow();
        if hwnd.0.is_null() {
            return json!({ "os": "windows" });
        }

        // Titre de la fenêtre
        let title_len = GetWindowTextLengthW(hwnd);
        let window_title = if title_len > 0 {
            let mut buf = vec![0u16; (title_len + 1) as usize];
            let n = GetWindowTextW(hwnd, &mut buf);
            String::from_utf16_lossy(&buf[..n as usize])
        } else {
            String::new()
        };

        // Nom du process (ex: "Cursor.exe")
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));

        let app_name = if pid != 0 {
            match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, false, pid) {
                Ok(handle) => {
                    let mut name_buf = vec![0u16; MAX_PATH as usize];
                    let n = GetModuleBaseNameW(handle, None, &mut name_buf);
                    let _ = CloseHandle(handle);
                    if n > 0 {
                        String::from_utf16_lossy(&name_buf[..n as usize])
                    } else {
                        String::new()
                    }
                }
                Err(_) => String::new(),
            }
        } else {
            String::new()
        };

        json!({
            "os": "windows",
            "app": app_name,
            "window": window_title,
        })
    }
}

#[cfg(not(target_os = "windows"))]
pub fn capture_active_context() -> Value {
    json!({ "os": "other" })
}
