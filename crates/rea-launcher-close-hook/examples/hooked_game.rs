//! A stand-in for an old game, for the close hook's tests: a hidden window
//! of class `rea_hooked_game` that ignores WM_CLOSE (as CMR04 does), quits
//! with code 4 on Escape, and exits with its message loop's quit code.
//! Hidden unless `REA_HOOKED_GAME_SHOW` is set: then it is shown and put
//! in front (still headless under Wine's null graphics driver), for the
//! `alt_f4_state` test.

#[cfg(windows)]
fn main() {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, MSG, PostQuitMessage,
        RegisterClassW, SW_SHOW, SetForegroundWindow, ShowWindow, TranslateMessage, WM_CLOSE,
        WM_KEYDOWN, WM_SYSCOMMAND, WNDCLASSW,
    };
    let show = std::env::var_os("REA_HOOKED_GAME_SHOW").is_some();

    unsafe extern "system" fn proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        // SAFETY: a window procedure called by the system for our window.
        unsafe {
            match msg {
                // The game swallows every close request.
                WM_CLOSE | WM_SYSCOMMAND => 0,
                WM_KEYDOWN if w == 0x1B => {
                    PostQuitMessage(4);
                    0
                }
                _ => DefWindowProcW(hwnd, msg, w, l),
            }
        }
    }

    let class: Vec<u16> = "rea_hooked_game\0".encode_utf16().collect();
    // SAFETY: a window class and window on this thread, then its message
    // loop; every pointer refers to a live local.
    let code = unsafe {
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
        if show {
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        msg.wParam as i32
    };
    std::process::exit(code);
}

#[cfg(not(windows))]
fn main() {}
