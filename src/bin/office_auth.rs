//! Portable executable entrypoint for the UI Automation credential watcher.
//! Embeds the script at compile time: no external .ps1 file is required.
//! Windows PowerShell 5.1 remains a runtime dependency.
#[cfg(not(windows))]
fn main() { eprintln!("office_auth.exe requires Windows."); }

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{env, process::{Command, Stdio}};
    let user = env::var("OFFICE_AUTH_USER")?;
    let password = env::var("OFFICE_AUTH_PASSWORD")?;
    if user.is_empty() || password.is_empty() {
        return Err("OFFICE_AUTH_USER and OFFICE_AUTH_PASSWORD must be set".into());
    }
    let script = include_str!("../../scripts/office-auth-uia.ps1");
    // PowerShell -EncodedCommand accepts base64-encoded UTF-16LE.
    let utf16: Vec<u8> = script.encode_utf16()
        .flat_map(|unit| unit.to_le_bytes()).collect();
    fn b64(bytes: &[u8]) -> String {
        const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let a = chunk[0]; let b = *chunk.get(1).unwrap_or(&0);
            let c = *chunk.get(2).unwrap_or(&0);
            out.push(ABC[(a >> 2) as usize] as char);
            out.push(ABC[(((a & 3) << 4) | (b >> 4)) as usize] as char);
            out.push(if chunk.len() > 1 { ABC[(((b & 15) << 2) | (c >> 6)) as usize] as char } else { '=' });
            out.push(if chunk.len() > 2 { ABC[(c & 63) as usize] as char } else { '=' });
        }
        out
    }
    println!("Office credential watcher started. Press Ctrl+C to stop.");
    let status = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand", &b64(&utf16)])
        .stdin(Stdio::null())
        .status()?;
    if !status.success() { return Err(format!("Watcher exited: {status}").into()); }
    Ok(())
}
