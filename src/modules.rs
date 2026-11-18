//! Read-only sampled module inventories. No injection, hooks in targets or memory reads.
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io::{self, Write}};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModuleEvent {
    pub kind: String,
    pub utc_ms: u64,
    pub elapsed_ms: u64,
    pub pid: u32,
    pub process: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_win32: Option<i32>,
}

pub fn print_event(mut output: impl Write, event: &ModuleEvent) -> io::Result<()> {
    writeln!(output, "+{}.{:03}s  {} pid={} process={:?} module={:?} error_win32={:?}",
        event.elapsed_ms / 1000, event.elapsed_ms % 1000, event.kind,
        event.pid, event.process, event.module_path, event.error_win32)
}

#[derive(Default)]
pub struct ModuleSummary {
    counts: BTreeMap<(u32, String), usize>,
}
impl ModuleSummary {
    pub fn record(&mut self, event: &ModuleEvent) {
        *self.counts.entry((event.pid, event.kind.clone())).or_default() += 1;
    }
    pub fn print(&self, mut output: impl Write) -> io::Result<()> {
        for ((pid, kind), count) in &self.counts {
            writeln!(output, "DLL pid={pid} {kind}={count}")?;
        }
        Ok(())
    }
}

#[cfg(any(windows, test))]
fn changes(previous: Option<&BTreeMap<String, String>>, current: &BTreeMap<String, String>) -> Vec<(&'static str, String)> {
    let mut result = Vec::new();
    for (key, path) in current {
        if previous.is_none_or(|p| !p.contains_key(key)) {
            result.push((if previous.is_none() { "module_baseline" } else { "module_added" }, path.clone()));
        }
    }
    if let Some(previous) = previous {
        for (key, path) in previous {
            if !current.contains_key(key) { result.push(("module_removed", path.clone())); }
        }
    }
    result
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::{mem::size_of, time::{Duration, Instant}};
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, ERROR_BAD_LENGTH, ERROR_NO_MORE_FILES, WAIT_OBJECT_0},
        System::{Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Module32FirstW, Module32NextW, MODULEENTRY32W, TH32CS_SNAPMODULE, TH32CS_SNAPMODULE32},
            Threading::{OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE}},
    };
    struct Handle(HANDLE);
    impl Drop for Handle { fn drop(&mut self) { unsafe { CloseHandle(self.0); } } }

    fn snapshot(pid: u32) -> io::Result<BTreeMap<String, String>> {
        let mut raw = INVALID_HANDLE_VALUE;
        for _ in 0..3 {
            raw = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPMODULE | TH32CS_SNAPMODULE32, pid) };
            if raw != INVALID_HANDLE_VALUE { break; }
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(ERROR_BAD_LENGTH as i32) { return Err(error); }
        }
        if raw == INVALID_HANDLE_VALUE { return Err(io::Error::last_os_error()); }
        let snapshot = Handle(raw);
        let mut entry = MODULEENTRY32W { dwSize: size_of::<MODULEENTRY32W>() as u32, ..unsafe { std::mem::zeroed() } };
        if unsafe { Module32FirstW(snapshot.0, &mut entry) } == 0 { return Err(io::Error::last_os_error()); }
        let mut paths = BTreeMap::new();
        loop {
            let len = entry.szExePath.iter().position(|c| *c == 0).unwrap_or(entry.szExePath.len());
            let path = String::from_utf16_lossy(&entry.szExePath[..len]);
            // Inventory DLLs only, including both native and WOW64 modules.
            if path.to_ascii_lowercase().ends_with(".dll") { paths.insert(path.to_lowercase(), path); }
            if unsafe { Module32NextW(snapshot.0, &mut entry) } == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() != Some(ERROR_NO_MORE_FILES as i32) { return Err(error); }
                break;
            }
        }
        Ok(paths)
    }

    struct Target {
        handle: Handle,
        process: Option<String>,
        previous: Option<BTreeMap<String, String>>,
        error: Option<i32>,
    }
    pub struct Tracker {
        targets: BTreeMap<u32, Target>,
        attempted: std::collections::BTreeSet<u32>,
        next_poll: Instant,
        pub summary: ModuleSummary,
    }
    impl Tracker {
        pub fn new() -> Self {
            Self { targets: BTreeMap::new(), attempted: Default::default(), next_poll: Instant::now(), summary: Default::default() }
        }
        fn emit(&mut self, event: ModuleEvent, output: &mut impl Write, screen: &mut impl Write) -> io::Result<()> {
            serde_json::to_writer(&mut *output, &event)?; writeln!(output)?;
            print_event(screen, &event)?; self.summary.record(&event); Ok(())
        }
        pub fn add(&mut self, pid: u32, process: Option<String>, utc_ms: u64, elapsed_ms: u64,
            output: &mut impl Write, screen: &mut impl Write) -> io::Result<()> {
            if !self.attempted.insert(pid) { return Ok(()); }
            // Bound work in sessions containing many short-lived candidate windows.
            if self.attempted.len() > 32 {
                return self.emit(ModuleEvent { kind: "module_target_limit".into(), utc_ms, elapsed_ms, pid, process, module_path: None, error_win32: None }, output, screen);
            }
            let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, 0, pid) };
            if handle.is_null() {
                let code = io::Error::last_os_error().raw_os_error();
                return self.emit(ModuleEvent { kind: "module_scan_error".into(), utc_ms, elapsed_ms, pid, process, module_path: None, error_win32: code }, output, screen);
            }
            self.targets.insert(pid, Target { handle: Handle(handle), process, previous: None, error: None });
            // Sample a newly observed process immediately rather than waiting for the next poll.
            self.next_poll = Instant::now();
            Ok(())
        }
        pub fn poll(&mut self, utc_ms: u64, elapsed_ms: u64, output: &mut impl Write, screen: &mut impl Write) -> io::Result<()> {
            if Instant::now() < self.next_poll { return Ok(()); }
            self.next_poll = Instant::now() + Duration::from_millis(500);
            let mut events = Vec::new(); let mut exited = Vec::new();
            for (&pid, target) in &mut self.targets {
                let make = |kind: &str, path, error| ModuleEvent { kind: kind.into(), utc_ms, elapsed_ms, pid, process: target.process.clone(), module_path: path, error_win32: error };
                if unsafe { WaitForSingleObject(target.handle.0, 0) } == WAIT_OBJECT_0 {
                    events.push(make("module_process_exited", None, None)); exited.push(pid); continue;
                }
                match snapshot(pid) {
                    Ok(current) => {
                        for (kind, path) in changes(target.previous.as_ref(), &current) { events.push(make(kind, Some(path), None)); }
                        if target.error.is_some() { events.push(make("module_scan_resumed", None, None)); }
                        target.previous = Some(current); target.error = None;
                    }
                    Err(error) => {
                        let code = error.raw_os_error().unwrap_or(-1);
                        if target.error != Some(code) { events.push(make("module_scan_error", None, Some(code))); }
                        // Keep the last successful inventory; a failed scan is not an unload.
                        target.error = Some(code);
                    }
                }
            }
            for pid in exited { self.targets.remove(&pid); }
            for event in events { self.emit(event, output, screen)?; }
            Ok(())
        }
    }
}
#[cfg(windows)]
pub use windows::Tracker;

#[cfg(test)]
mod tests {
    use super::*;
    fn map(names: &[&str]) -> BTreeMap<String, String> { names.iter().map(|s| (s.to_lowercase(), s.to_string())).collect() }
    #[test]
    fn baseline_then_real_additions_and_removals() {
        let old = map(&["C:\\Windows\\a.dll", "C:\\Vendor\\b.dll"]);
        assert!(changes(None, &old).iter().all(|c| c.0 == "module_baseline"));
        let new = map(&["c:\\windows\\A.DLL", "C:\\Vendor\\c.dll"]);
        let delta = changes(Some(&old), &new);
        assert_eq!(delta.len(), 2);
        assert!(delta.contains(&("module_added", "C:\\Vendor\\c.dll".into())));
        assert!(delta.contains(&("module_removed", "C:\\Vendor\\b.dll".into())));
        assert!(changes(Some(&new), &new).is_empty());
    }
}
