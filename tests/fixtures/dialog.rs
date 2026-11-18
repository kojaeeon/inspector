use std::{ffi::c_void, ptr::null_mut, thread, time::Duration};

type Hwnd = *mut c_void;
type TimerProc = unsafe extern "system" fn(Hwnd, u32, usize, u32);

#[link(name = "user32")]
extern "system" {
    fn SetTimer(window: Hwnd, id: usize, interval: u32, callback: Option<TimerProc>) -> usize;
    fn KillTimer(window: Hwnd, id: usize) -> i32;
    fn FindWindowW(class: *const u16, title: *const u16) -> Hwnd;
    fn PostMessageW(window: Hwnd, message: u32, wparam: usize, lparam: isize) -> i32;
    fn MessageBoxW(window: Hwnd, text: *const u16, title: *const u16, style: u32) -> i32;
}

const TITLE: &str = "Inspector synthetic dialog";
const WM_CLOSE: u32 = 0x0010;

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(Some(0)).collect()
}

unsafe extern "system" fn dismiss(_: Hwnd, _: u32, timer: usize, _: u32) {
    let window = FindWindowW(wide("#32770").as_ptr(), wide(TITLE).as_ptr());
    if !window.is_null() && PostMessageW(window, WM_CLOSE, 0, 0) != 0 {
        KillTimer(null_mut(), timer);
    }
}

#[link(name = "kernel32")]
extern "system" {
    fn LoadLibraryW(path: *const u16) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
}

fn main() {
    for index in 0..2 {
        unsafe {
            let timer = SetTimer(null_mut(), 0, 600, Some(dismiss));
            assert_ne!(timer, 0);
            assert_ne!(
                MessageBoxW(
                    null_mut(),
                    wide("Synthetic test window").as_ptr(),
                    wide(TITLE).as_ptr(),
                    0,
                ),
                0
            );
            KillTimer(null_mut(), timer);
        }
        thread::sleep(Duration::from_millis(250));
        if index == 0 {
            if let Some(path) = std::env::args().nth(1) {
                unsafe {
                    let module = LoadLibraryW(wide(&path).as_ptr());
                    assert!(!module.is_null());
                    thread::sleep(Duration::from_millis(1100));
                    assert_ne!(FreeLibrary(module), 0);
                    thread::sleep(Duration::from_millis(1100));
                }
            }
        }
    }
}
