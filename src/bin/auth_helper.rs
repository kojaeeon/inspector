//! Explicitly confirmed, foreground-only credential entry for a Windows Security dialog.
//! No hooks, persistence, logging of secrets, or bypass of server authentication.
#[cfg(not(windows))]
fn main() { eprintln!("This tool requires Windows."); }

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{env, io::{self, Write}, thread, time::Duration};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowTextW};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
        KEYEVENTF_UNICODE, KEYEVENTF_KEYUP, VK_TAB, VK_RETURN,
    };

    fn key(scan: u16, flags: u32) -> INPUT {
        INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: 0, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 }
        }}
    }
    fn send(inputs: &[INPUT]) -> io::Result<()> {
        let count = unsafe {
            SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32)
        };
        if count != inputs.len() as u32 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
    fn type_text(s: &str) -> io::Result<()> {
        for unit in s.encode_utf16() {
            send(&[key(unit, KEYEVENTF_UNICODE), key(unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP)])?;
        }
        Ok(())
    }
    fn virtual_key(vk: u16) -> io::Result<()> {
        let mut down = key(0, 0);
        down.Anonymous.ki.wVk = vk;
        let mut up = key(0, KEYEVENTF_KEYUP);
        up.Anonymous.ki.wVk = vk;
        send(&[down, up])
    }

    let user = env::var("OFFICE_AUTH_USER")?;
    let password = env::var("OFFICE_AUTH_PASSWORD")?;
    if user.is_empty() || password.is_empty() { return Err("Empty credentials".into()); }

    println!("After confirming, you have 5 seconds to activate the intended Windows Security dialog.");
    println!("Make sure the USERNAME field is focused and empty.");
    print!("Type YES to proceed: ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if answer.trim() != "YES" { return Ok(()); }
    thread::sleep(Duration::from_secs(5));

    let hwnd = unsafe { GetForegroundWindow() };
    let mut buf = [0u16; 256];
    let n = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
    let title = String::from_utf16_lossy(&buf[..(n.max(0) as usize)]);
    if !title.contains("Windows Security") && !title.contains("Windows 보안") {
        return Err(format!("Unexpected foreground window: {title:?}").into());
    }
    type_text(&user)?;
    virtual_key(VK_TAB)?;
    type_text(&password)?;
    virtual_key(VK_RETURN)?;
    println!("Credentials submitted. Windows and the server determine whether authentication succeeds.");
    Ok(())
}
