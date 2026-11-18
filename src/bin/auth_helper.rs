//! Opt-in foreground credential helper. Requires Office and an exact target marker
//! in the foreground window title. Does not submit credentials.
#[cfg(not(windows))]
fn main() { eprintln!("Windows only"); }

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{collections::HashSet, env, thread, time::Duration};
    use windows_sys::Win32::{
        Foundation::CloseHandle,
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Process32FirstW, Process32NextW,
            PROCESSENTRY32W, TH32CS_SNAPPROCESS,
        },
        UI::{
            WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW},
            Input::KeyboardAndMouse::{
                SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
                KEYEVENTF_UNICODE, KEYEVENTF_KEYUP, VK_TAB,
            },
        },
    };

    fn office_running() -> bool {
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE { return false; }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            let mut found = false;
            if Process32FirstW(snapshot, &mut entry) != 0 {
                loop {
                    let end = entry.szExeFile.iter().position(|&c| c == 0).unwrap_or(entry.szExeFile.len());
                    let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
                    if ["EXCEL.EXE", "WINWORD.EXE", "POWERPNT.EXE"]
                        .iter().any(|x| name.eq_ignore_ascii_case(x)) {
                        found = true;
                        break;
                    }
                    if Process32NextW(snapshot, &mut entry) == 0 { break; }
                }
            }
            CloseHandle(snapshot);
            found
        }
    }
    fn send_unicode(text: &str) -> Result<(), Box<dyn std::error::Error>> {
        for unit in text.encode_utf16() {
            let make = |flags| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 { ki: KEYBDINPUT {
                    wVk: 0, wScan: unit, dwFlags: flags, time: 0, dwExtraInfo: 0
                }},
            };
            let inputs = [make(KEYEVENTF_UNICODE), make(KEYEVENTF_UNICODE | KEYEVENTF_KEYUP)];
            let sent = unsafe { SendInput(2, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
            if sent != 2 { return Err(std::io::Error::last_os_error().into()); }
        }
        Ok(())
    }
    fn tab() -> Result<(), Box<dyn std::error::Error>> {
        let make = |flags| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT {
                wVk: VK_TAB, wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0
            }},
        };
        let inputs = [make(0), make(KEYEVENTF_KEYUP)];
        let sent = unsafe { SendInput(2, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
        if sent != 2 { return Err(std::io::Error::last_os_error().into()); }
        Ok(())
    }
    let username = env::var("OFFICE_AUTH_USER")?;
    let password = env::var("OFFICE_AUTH_PASSWORD")?;
    if username.is_empty() || password.is_empty() { return Err("Credentials are empty".into()); }
    let target = env::var("OFFICE_AUTH_TARGET")
        .unwrap_or_else(|_| "scadmin.standardchartered.co.kr".to_string());
    if target.is_empty() { return Err("Target must not be empty".into()); }
    let mut filled: HashSet<isize> = HashSet::new();
    let mut last_window: isize = 0;
    println!("Watching Office + verified foreground credential dialog; Ctrl+C to stop.");
    loop {
        if !office_running() {
            filled.clear();
            last_window = 0;
            thread::sleep(Duration::from_millis(750));
            continue;
        }
        let hwnd = unsafe { GetForegroundWindow() };
        let id = hwnd as isize;
        if id != last_window {
            filled.clear();
            last_window = id;
        }
        if !hwnd.is_null() && !filled.contains(&id) {
            let mut buf = [0u16; 1024];
            let len = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
            let title = String::from_utf16_lossy(&buf[..len.max(0) as usize]);
            let is_security = title.contains("Windows Security") || title.contains("Windows 보안");
            // Deliberately fail closed if the server cannot be established from the title.
            if is_security && title.to_lowercase().contains(&target.to_lowercase()) {
                // Requires the username field to already have keyboard focus.
                // Does not click OK or press Enter.
                send_unicode(&username)?;
                tab()?;
                send_unicode(&password)?;
                filled.insert(id);
                println!("Fields filled. Click Confirm manually.");
            }
        }
        thread::sleep(Duration::from_millis(500));
    }
}
