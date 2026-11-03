//! The hook itself: runs inside the game's main thread.

use std::path::PathBuf;
use std::ptr::null_mut;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use std::sync::Mutex;
use windows_sys::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::{
    GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
    GetModuleFileNameW, GetModuleHandleExW,
};
use windows_sys::Win32::System::Threading::{ExitProcess, GetCurrentProcess, TerminateProcess};

use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GA_ROOT, GetAncestor, GetClassNameW, GetForegroundWindow,
    GetWindowThreadProcessId, HC_ACTION, MSG, PM_REMOVE, PostMessageW, PostQuitMessage, WM_KEYDOWN,
    WM_KEYUP, WM_NULL, WM_QUIT,
};

use crate::{Chord, Config, Step, Strategy, msg};

/// The configuration: `REA_CLOSE_HOOK`, else `<dll>.ini` next to the DLL,
/// else the defaults. `None` when the text doesn't parse: a broken
/// configuration leaves the game alone rather than ending it wrongly.
fn config() -> Option<&'static Config> {
    static CONFIG: OnceLock<Option<Config>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let text = std::env::var(crate::ENV)
                .ok()
                .filter(|text| !text.trim().is_empty())
                .or_else(|| std::fs::read_to_string(ini_path()?).ok());
            text.map_or_else(|| Some(Config::default()), |text| Config::parse(&text).ok())
        })
        .as_ref()
}

/// `<this DLL without extension>.ini`.
fn ini_path() -> Option<PathBuf> {
    let mut module: HMODULE = null_mut();
    let mut name = [0u16; 1024];
    // SAFETY: looks up the module containing this function (no reference
    // taken) and copies its path into a local buffer of the given size.
    let len = unsafe {
        if GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
            close_hook as *const () as *const u16,
            &mut module,
        ) == 0
        {
            return None;
        }
        GetModuleFileNameW(module, name.as_mut_ptr(), name.len() as u32)
    };
    (len > 0).then(|| {
        PathBuf::from(String::from_utf16_lossy(&name[..len as usize])).with_extension("ini")
    })
}

/// The class name of `hwnd`'s top-level window.
fn root_class(hwnd: HWND) -> String {
    let mut name = [0u16; 256];
    // SAFETY: plain window queries into a local buffer of the given size.
    let len = unsafe {
        let root = GetAncestor(hwnd, GA_ROOT);
        GetClassNameW(
            if root.is_null() { hwnd } else { root },
            name.as_mut_ptr(),
            name.len() as i32,
        )
    };
    String::from_utf16_lossy(&name[..len.max(0) as usize])
}

/// Presses and releases `vk` through `SendInput`.
fn send_key(vk: u16) {
    let input = |flags| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [input(0), input(KEYEVENTF_KEYUP)];
    // SAFETY: two initialised keyboard INPUTs and their size.
    unsafe {
        SendInput(2, inputs.as_ptr(), size_of::<INPUT>() as i32);
    }
}

/// Set while a key sequence plays: a second close request then is
/// swallowed instead of starting another one.
static PLAYING: AtomicBool = AtomicBool::new(false);

/// Plays `steps` on a thread of its own, so the game's loop keeps running
/// (and its menu can react) during the pauses. Another request can start
/// it again once it is done (the game may have been in a submenu).
fn play(steps: Vec<Step>, window: Option<usize>) {
    if PLAYING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        for step in steps {
            match step {
                Step::Pause(ms) => std::thread::sleep(Duration::from_millis(u64::from(ms))),
                Step::Key(vk) => match window {
                    // SAFETY: posting to the game's window; a stale handle
                    // just fails.
                    Some(hwnd) => unsafe {
                        PostMessageW(hwnd as HWND, WM_KEYDOWN, usize::from(vk), 1);
                        PostMessageW(
                            hwnd as HWND,
                            WM_KEYUP,
                            usize::from(vk),
                            0xC000_0001u32 as isize,
                        );
                    },
                    None => send_key(vk),
                },
            }
        }
        PLAYING.store(false, Ordering::SeqCst);
    });
}

/// Ends the game as configured. Returns once the quit was started.
fn quit(config: &Config, hwnd: HWND) {
    // SAFETY: process-wide exit calls on this process, and posts to the
    // game's own window or thread.
    unsafe {
        match &config.strategy {
            Strategy::Terminate => {
                TerminateProcess(GetCurrentProcess(), config.code);
            }
            Strategy::Exit => ExitProcess(config.code),
            Strategy::Quit => PostQuitMessage(config.code as i32),
            Strategy::PostQuit => {
                PostMessageW(hwnd, WM_QUIT, config.code as usize, 0);
            }
            Strategy::Keys(steps) => play(steps.clone(), None),
            Strategy::PostKeys(steps) => play(steps.clone(), Some(hwnd as usize)),
        }
    }
}

/// Whether a virtual key is held down now (the keyboard's state, which
/// DirectInput's exclusive mode doesn't hide).
fn key_down(vk: usize) -> bool {
    // SAFETY: a plain keyboard-state query.
    (unsafe { GetAsyncKeyState(vk as i32) } as u16 & 0x8000) != 0
}

/// `alt_f4_state`: the game's window in front (this process's, of the
/// configured class) and a new Alt+F4 press; that window. Alt+F4 pressed
/// in another program never counts.
fn alt_f4_pressed(config: &Config) -> Option<HWND> {
    static CHORD: Mutex<Chord> = Mutex::new(Chord { held: false });
    let (alt, f4) = (key_down(msg::VK_MENU), key_down(msg::VK_F4));
    let mut chord = CHORD.lock().ok()?;
    if !chord.press(alt, f4) {
        return None;
    }
    // SAFETY: plain window queries.
    let front = unsafe { GetForegroundWindow() };
    if front.is_null() {
        return None;
    }
    let mut pid = 0u32;
    // SAFETY: as above, into a local.
    unsafe { GetWindowThreadProcessId(front, &mut pid) };
    // SAFETY: a plain process query.
    let ours = pid == unsafe { GetCurrentProcessId() };
    (ours && config.is_game_class(&root_class(front))).then_some(front)
}

/// The `WH_GETMESSAGE` hook the launcher installs.
///
/// # Safety
/// Called by Windows as a `WH_GETMESSAGE` hook procedure; for `HC_ACTION`,
/// `lparam` points to the `MSG` being retrieved.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn close_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: for HC_ACTION, lparam is the MSG being removed from the
    // queue, valid and writable for the duration of this call.
    unsafe {
        if code == HC_ACTION as i32
            && wparam == PM_REMOVE as usize
            && let Some(config) = config()
        {
            let msg = &mut *(lparam as *mut MSG);
            if config.requests.matches(msg.message, msg.wParam, msg.lParam)
                && config.is_game_class(&root_class(msg.hwnd))
            {
                // The request goes no further (Alt+F4's default handling
                // included).
                msg.message = WM_NULL;
                quit(config, msg.hwnd);
            } else if config.requests.alt_f4_state
                && let Some(window) = alt_f4_pressed(config)
            {
                // Any message will do: the keyboard's state says Alt+F4.
                quit(config, window);
            }
        }
        CallNextHookEx(null_mut(), code, wparam, lparam)
    }
}
