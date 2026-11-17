use crate::report::{self, Event, Kind, Relation, Summary};
use std::{
    cell::RefCell,
    collections::HashMap,
    fs::File,
    io::{self, Write},
    path::Path,
    ptr::null_mut,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HWND},
    System::{
        Console::{SetConsoleCtrlHandler, CTRL_BREAK_EVENT, CTRL_C_EVENT},
        Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION},
    },
    UI::{
        Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK},
        WindowsAndMessaging::{
            DispatchMessageW, GetAncestor, GetClassNameW, GetForegroundWindow, GetWindow,
            GetWindowThreadProcessId, PeekMessageW, EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE,
            EVENT_OBJECT_SHOW, EVENT_SYSTEM_DIALOGEND, EVENT_SYSTEM_DIALOGSTART,
            EVENT_SYSTEM_FOREGROUND, GA_ROOT, GW_OWNER, MSG, PM_REMOVE, WINEVENT_OUTOFCONTEXT,
            WINEVENT_SKIPOWNPROCESS, WM_QUIT,
        },
    },
};

const OFFICE_CONTEXT_MS: u64 = 15_000;
static STOP: AtomicBool = AtomicBool::new(false);

thread_local! {
    static CAPTURE: RefCell<Option<Capture>> = const { RefCell::new(None) };
}

struct Capture {
    start: Instant,
    utc_start_ms: u64,
    foreground: Option<usize>,
    office_foreground: bool,
    last_office_ms: Option<u64>,
    next_window_id: u64,
    dialogs: HashMap<usize, Event>,
    pending: Vec<Event>,
}

impl Capture {
    fn observe(&mut self, action: u32, hwnd: HWND) {
        let elapsed_ms = self.start.elapsed().as_millis() as u64;
        let handle = hwnd as usize;
        if matches!(
            action,
            EVENT_SYSTEM_DIALOGEND | EVENT_OBJECT_HIDE | EVENT_OBJECT_DESTROY
        ) {
            if let Some(mut event) = self.dialogs.remove(&handle) {
                event.kind = Kind::DialogClosed;
                event.elapsed_ms = elapsed_ms;
                event.utc_ms = self.utc_start_ms + elapsed_ms;
                self.pending.push(event);
            }
            return;
        }

        let foreground = action == EVENT_SYSTEM_FOREGROUND;
        if foreground {
            if self.foreground == Some(handle) {
                return;
            }
            if self.office_foreground {
                self.last_office_ms = Some(elapsed_ms);
            }
            self.foreground = Some(handle);
            self.office_foreground = false;
        } else if self.dialogs.contains_key(&handle) {
            return;
        }
        // Read top-level window metadata only. Ignore accessibility controls.
        if unsafe { GetAncestor(hwnd, GA_ROOT) } != hwnd {
            return;
        }
        let mut class = [0; 256];
        let class_len = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };
        if class_len <= 0 {
            return;
        }
        let class = String::from_utf16_lossy(&class[..class_len as usize]);
        if action == EVENT_OBJECT_SHOW && class != "#32770" {
            return;
        }

        let Some(pid) = process_id(hwnd) else {
            return;
        };
        let process = process_name(pid);
        let owner_pid = process_id(unsafe { GetWindow(hwnd, GW_OWNER) });
        let owner_process = match owner_pid {
            Some(owner) if owner == pid => process.clone(),
            Some(owner) => process_name(owner),
            None => None,
        };
        let is_office = is_office_process(process.as_deref());
        let relation = if is_office {
            Relation::OfficeProcess
        } else if is_office_process(owner_process.as_deref()) {
            Relation::OfficeOwner
        } else if self.office_foreground
            || self
                .last_office_ms
                .is_some_and(|last| elapsed_ms.saturating_sub(last) <= OFFICE_CONTEXT_MS)
        {
            Relation::Temporal
        } else {
            return;
        };

        let kind = if foreground {
            if is_office {
                self.office_foreground = true;
                self.last_office_ms = Some(elapsed_ms);
                Kind::OfficeForeground
            } else if self.dialogs.contains_key(&handle) {
                return;
            } else {
                Kind::ForegroundCandidate
            }
        } else {
            Kind::DialogOpened
        };
        self.next_window_id += 1;
        let event = Event {
            utc_ms: self.utc_start_ms + elapsed_ms,
            elapsed_ms,
            window_id: self.next_window_id,
            kind,
            pid,
            process,
            class,
            owner_pid,
            owner_process,
            relation,
        };
        if kind == Kind::DialogOpened {
            self.dialogs.insert(handle, event.clone());
        }
        self.pending.push(event);
    }
}

fn is_office_process(name: Option<&str>) -> bool {
    name.is_some_and(|name| {
        ["excel.exe", "winword.exe", "powerpnt.exe"]
            .iter()
            .any(|office| name.eq_ignore_ascii_case(office))
    })
}

fn process_id(hwnd: HWND) -> Option<u32> {
    if hwnd.is_null() {
        return None;
    }
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
    (pid != 0).then_some(pid)
}

fn process_name(pid: u32) -> Option<String> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return None;
    }
    let mut path = [0; 32_768];
    let mut len = path.len() as u32;
    let ok = unsafe {
        let ok = QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &mut len);
        CloseHandle(process);
        ok
    };
    if ok == 0 {
        return None;
    }
    let basename = path[..len as usize]
        .rsplit(|ch| *ch == u16::from(b'\\') || *ch == u16::from(b'/'))
        .next()?;
    Some(String::from_utf16_lossy(basename))
}

unsafe extern "system" fn on_event(
    _hook: HWINEVENTHOOK,
    action: u32,
    hwnd: HWND,
    object: i32,
    child: i32,
    _thread: u32,
    _event_time: u32,
) {
    if hwnd.is_null() || object != 0 || child != 0 {
        return;
    }
    CAPTURE.with(|state| {
        if let Ok(mut state) = state.try_borrow_mut() {
            if let Some(capture) = state.as_mut() {
                capture.observe(action, hwnd);
            }
        }
    });
}

unsafe extern "system" fn on_console_control(event: u32) -> i32 {
    if matches!(event, CTRL_C_EVENT | CTRL_BREAK_EVENT) {
        STOP.store(true, Ordering::Relaxed);
        return 1;
    }
    0
}

struct Registration {
    hooks: Vec<HWINEVENTHOOK>,
    console_handler: bool,
}

impl Drop for Registration {
    fn drop(&mut self) {
        for hook in self.hooks.drain(..) {
            unsafe { UnhookWinEvent(hook) };
        }
        if self.console_handler {
            unsafe { SetConsoleCtrlHandler(Some(on_console_control), 0) };
        }
        CAPTURE.with(|state| *state.borrow_mut() = None);
    }
}

pub fn watch(duration: Duration, path: &Path) -> io::Result<()> {
    let start = Instant::now();
    let utc_start_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_millis() as u64;
    STOP.store(false, Ordering::Relaxed);
    let mut registration = Registration {
        hooks: Vec::new(),
        console_handler: false,
    };
    CAPTURE.with(|state| {
        *state.borrow_mut() = Some(Capture {
            start,
            utc_start_ms,
            foreground: None,
            office_foreground: false,
            last_office_ms: None,
            next_window_id: 0,
            dialogs: HashMap::new(),
            pending: Vec::new(),
        });
    });
    for (first, last) in [
        (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
        (EVENT_SYSTEM_DIALOGSTART, EVENT_SYSTEM_DIALOGEND),
        (EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE),
    ] {
        let hook = unsafe {
            SetWinEventHook(
                first,
                last,
                null_mut(),
                Some(on_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        };
        if hook.is_null() {
            return Err(io::Error::last_os_error());
        }
        registration.hooks.push(hook);
    }
    if unsafe { SetConsoleCtrlHandler(Some(on_console_control), 1) } == 0 {
        return Err(io::Error::last_os_error());
    }
    registration.console_handler = true;

    let mut output = File::create_new(path)?;
    let mut screen = io::stdout().lock();
    let mut summary = Summary::default();
    let initial = unsafe { GetForegroundWindow() };
    if !initial.is_null() {
        CAPTURE.with(|state| {
            if let Some(capture) = state.borrow_mut().as_mut() {
                capture.observe(EVENT_SYSTEM_FOREGROUND, initial);
            }
        });
    }
    writeln!(
        screen,
        "Watching for {} seconds. Reproduce the prompt now. Ctrl+C stops capture.",
        duration.as_secs()
    )?;
    loop {
        let mut message = MSG::default();
        while unsafe { PeekMessageW(&mut message, null_mut(), 0, 0, PM_REMOVE) } != 0 {
            if message.message == WM_QUIT {
                STOP.store(true, Ordering::Relaxed);
                break;
            }
            unsafe { DispatchMessageW(&message) };
            if start.elapsed() >= duration || STOP.load(Ordering::Relaxed) {
                break;
            }
        }
        let pending = CAPTURE.with(|state| {
            let mut state = state.borrow_mut();
            std::mem::take(&mut state.as_mut().expect("capture is registered").pending)
        });
        for event in pending {
            report::write_event(&mut output, &event)?;
            report::print_event(&mut screen, &event)?;
            summary.record(&event);
        }
        if start.elapsed() >= duration || STOP.load(Ordering::Relaxed) {
            break;
        }
        thread::sleep(Duration::from_millis(15));
    }
    writeln!(screen, "\nSaved to {}", path.display())?;
    summary.print(screen)?;
    Ok(())
}
