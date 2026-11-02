//! A tiny child process for the close tests (`process::Running`):
//!
//!   close_child window   a hidden window; quits on WM_CLOSE (exit 0)
//!   close_child deaf     a hidden window that ignores WM_CLOSE
//!   close_child console  no window; sleeps
//!
//! Headless: the window is never shown.

#[cfg(windows)]
fn main() {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, MSG,
        PostQuitMessage, RegisterClassW, TranslateMessage, WM_CLOSE, WM_DESTROY, WNDCLASSW,
    };

    use std::sync::atomic::{AtomicBool, Ordering};
    static DEAF: AtomicBool = AtomicBool::new(false);

    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        // SAFETY: a window procedure called by the system for our window.
        unsafe {
            match msg {
                WM_CLOSE if DEAF.load(Ordering::Relaxed) => 0,
                WM_CLOSE => {
                    DestroyWindow(hwnd);
                    0
                }
                WM_DESTROY => {
                    PostQuitMessage(0);
                    0
                }
                _ => DefWindowProcW(hwnd, msg, w, l),
            }
        }
    }

    let mode = std::env::args().nth(1).unwrap_or_default();
    if mode == "console" {
        std::thread::sleep(std::time::Duration::from_secs(60));
        return;
    }
    DEAF.store(mode == "deaf", Ordering::Relaxed);
    let class: Vec<u16> = "rea_close_child\0".encode_utf16().collect();
    // SAFETY: a plain window class and window on this thread, then its
    // message loop; every pointer refers to a live local.
    unsafe {
        let instance = GetModuleHandleW(null());
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            class.as_ptr(),
            0,
            0,
            0,
            100,
            100,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            std::process::exit(2);
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(not(windows))]
fn main() {}
