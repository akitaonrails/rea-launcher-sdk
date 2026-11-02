//! Starting the game (or a helper) with `CreateProcessW`.
//!
//! From CMR4's `run_game`: RAII handles, environment changes in the child's
//! own block (so `WINEDLLOVERRIDES` works without the Linux helper), and
//! the main thread id, which CMR4 needs to hook the game's message loop
//! with a close-hook DLL. CMR3 adds arguments (quoted by
//! `rea_launcher::args::command_line`) and hidden helpers (`CREATE_NO_WINDOW`).
//! `std::process::Command` covers neither the thread id nor the hook.
//!
//! ```no_run
//! # use rea_launcher::wine::merge_dll_overrides;
//! # use rea_launcher_win32::process::Launch;
//! # fn main() -> Result<(), String> {
//! let ours = "d3d9,dxgi=n,b";
//! let overrides = merge_dll_overrides(ours, std::env::var("WINEDLLOVERRIDES").ok().as_deref());
//! let game = Launch::new(r"C:\rt\game.exe")
//!     .cwd(r"C:\rt")
//!     .env("WINEDLLOVERRIDES", overrides)
//!     .spawn()?;
//! let _hook = game.message_hook(std::path::Path::new(r"C:\rt\hook.dll"), c"close_hook");
//! let code = game.wait()?;
//! # let _ = code; Ok(()) }
//! ```

use std::ffi::{CStr, OsString};
use std::path::{Path, PathBuf};
use std::ptr::null;

use std::time::Duration;

use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::TerminateProcess;
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, GetExitCodeProcess, INFINITE,
    PROCESS_INFORMATION, STARTUPINFOW, WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
};
use windows_sys::core::BOOL;

use crate::wide::{last_error, wide};

/// What to start. Build with the methods, then [`Launch::spawn`] or
/// [`Launch::run`].
#[derive(Debug, Clone, Default)]
pub struct Launch {
    exe: PathBuf,
    args: Vec<OsString>,
    cwd: Option<PathBuf>,
    env: Vec<(OsString, Option<OsString>)>,
    hidden: bool,
}

impl Launch {
    pub fn new(exe: impl Into<PathBuf>) -> Launch {
        Launch {
            exe: exe.into(),
            ..Launch::default()
        }
    }

    pub fn arg(mut self, arg: impl Into<OsString>) -> Launch {
        self.args.push(arg.into());
        self
    }

    pub fn args<A: Into<OsString>>(mut self, args: impl IntoIterator<Item = A>) -> Launch {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// The working directory (default: the launcher's).
    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Launch {
        self.cwd = Some(dir.into());
        self
    }

    /// Sets a variable in the child's environment (the rest is inherited).
    pub fn env(mut self, name: impl Into<OsString>, value: impl Into<OsString>) -> Launch {
        self.env.push((name.into(), Some(value.into())));
        self
    }

    /// Removes a variable from the child's environment.
    pub fn env_remove(mut self, name: impl Into<OsString>) -> Launch {
        self.env.push((name.into(), None));
        self
    }

    /// Starts without a console window (a bundled command-line tool).
    pub fn hidden(mut self, hidden: bool) -> Launch {
        self.hidden = hidden;
        self
    }

    /// Starts the process.
    pub fn spawn(&self) -> Result<Running, String> {
        let application = wide(&self.exe);
        let mut command = wide(rea_launcher::args::command_line(
            self.exe.as_os_str(),
            &self.args,
        ));
        let directory = self.cwd.as_deref().map(wide);
        let environment = (!self.env.is_empty()).then(|| {
            rea_launcher::env::environment_block(&rea_launcher::env::merge_env(
                std::env::vars_os(),
                &self.env,
            ))
        });
        let mut flags = 0;
        if self.hidden {
            flags |= CREATE_NO_WINDOW;
        }
        if environment.is_some() {
            flags |= CREATE_UNICODE_ENVIRONMENT;
        }
        // SAFETY: zeroed STARTUPINFOW/PROCESS_INFORMATION are valid inputs
        // (cb set below).
        let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
        startup.cb = size_of::<STARTUPINFOW>() as u32;
        let mut info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: every buffer outlives the call and `command` is writable,
        // as CreateProcessW requires; the returned handles are owned by
        // `Running` from here on.
        let created = unsafe {
            CreateProcessW(
                application.as_ptr(),
                command.as_mut_ptr(),
                null(),
                null(),
                0,
                flags,
                environment
                    .as_ref()
                    .map_or(null(), |block| block.as_ptr().cast()),
                directory.as_ref().map_or(null(), |dir| dir.as_ptr()),
                &startup,
                &mut info,
            )
        };
        if created == 0 {
            return Err(format!(
                "cannot start {}: {}",
                self.exe.display(),
                last_error()
            ));
        }
        Ok(Running(info))
    }

    /// Starts the process and waits for it; its exit code.
    pub fn run(&self) -> Result<u32, String> {
        self.spawn()?.wait()
    }
}

/// A started process; its handles close on drop.
pub struct Running(PROCESS_INFORMATION);

impl std::fmt::Debug for Running {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Running")
            .field("pid", &self.pid())
            .field("thread_id", &self.thread_id())
            .finish()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        // SAFETY: both handles came from CreateProcessW and are closed once.
        unsafe {
            CloseHandle(self.0.hThread);
            CloseHandle(self.0.hProcess);
        }
    }
}

impl Running {
    pub fn pid(&self) -> u32 {
        self.0.dwProcessId
    }

    /// The main thread's id.
    pub fn thread_id(&self) -> u32 {
        self.0.dwThreadId
    }

    /// Whether the process is still running.
    pub fn is_alive(&self) -> bool {
        // SAFETY: a zero-timeout wait on the owned process handle.
        unsafe { WaitForSingleObject(self.0.hProcess, 0) == WAIT_TIMEOUT }
    }

    /// The exit code once the process has ended; `None` while it runs.
    pub fn exit_code(&self) -> Option<u32> {
        if self.is_alive() {
            return None;
        }
        let mut code = 0u32;
        // SAFETY: the owned process handle and a local output.
        (unsafe { GetExitCodeProcess(self.0.hProcess, &mut code) } != 0).then_some(code)
    }

    /// Asks the process to quit the way a player would: `WM_CLOSE` to every
    /// top-level window it owns. Waits up to `timeout`, then terminates it
    /// as a last resort (exit code 1). Says which path ended it.
    pub fn close_gracefully(&self, timeout: Duration) -> Result<Closed, String> {
        if let Some(code) = self.exit_code() {
            return Ok(Closed::AlreadyExited(code));
        }
        let windows = top_level_windows(self.pid());
        for &hwnd in &windows {
            // SAFETY: posting to a window handle; a stale one just fails.
            unsafe {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            }
        }
        let wait = if windows.is_empty() {
            0
        } else {
            u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1)
        };
        // SAFETY: waits on the owned process handle.
        if unsafe { WaitForSingleObject(self.0.hProcess, wait) } != WAIT_TIMEOUT {
            return Ok(Closed::Closed {
                windows: windows.len(),
                code: self.exit_code().unwrap_or(0),
            });
        }
        // SAFETY: the owned process handle; then a bounded wait for it.
        unsafe {
            if TerminateProcess(self.0.hProcess, 1) == 0 {
                return Err(format!(
                    "cannot end process {}: {}",
                    self.pid(),
                    last_error()
                ));
            }
            WaitForSingleObject(self.0.hProcess, 5_000);
        }
        Ok(Closed::Terminated {
            windows: windows.len(),
        })
    }

    /// Loads `dll` here and installs its `export` as a `WH_GETMESSAGE` hook
    /// on the child's main thread (Windows then loads the DLL into the
    /// child). CMR4 uses it to end a game that ignores `WM_CLOSE`. Keep the
    /// returned hook alive until [`Running::wait`] returns.
    ///
    /// The child is already running when the hook goes in, so messages it
    /// handles before that are not seen — as in CMR4, where the hook only
    /// has to catch a later close request. Starting the child suspended
    /// would close that gap, but hooking a thread that has not yet made a
    /// window call is not reliable (and untested under Wine), so it isn't
    /// done.
    pub fn message_hook(&self, dll: &Path, export: &CStr) -> Result<MessageHook, String> {
        use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
        use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowsHookExW, WH_GETMESSAGE};
        // SAFETY: NUL-terminated names; the module stays loaded for the
        // launcher's lifetime, which outlives the hook. The export must be
        // a HOOKPROC — that is the DLL's contract with its caller.
        unsafe {
            let module = LoadLibraryW(wide(dll).as_ptr());
            if module.is_null() {
                return Err(format!("cannot load {}: {}", dll.display(), last_error()));
            }
            let Some(proc) = GetProcAddress(module, export.as_ptr().cast()) else {
                return Err(format!(
                    "{} has no {} export",
                    dll.display(),
                    export.to_string_lossy()
                ));
            };
            let hook = SetWindowsHookExW(
                WH_GETMESSAGE,
                Some(std::mem::transmute::<
                    unsafe extern "system" fn() -> isize,
                    unsafe extern "system" fn(i32, usize, isize) -> isize,
                >(proc)),
                module,
                self.thread_id(),
            );
            if hook.is_null() {
                return Err(format!("cannot install the hook: {}", last_error()));
            }
            Ok(MessageHook(hook))
        }
    }

    /// Waits for the process to end; its exit code.
    pub fn wait(self) -> Result<u32, String> {
        let mut code = 0u32;
        // SAFETY: the process handle stays valid until `self` drops.
        unsafe {
            WaitForSingleObject(self.0.hProcess, INFINITE);
            if GetExitCodeProcess(self.0.hProcess, &mut code) == 0 {
                return Err(format!("cannot read the exit status: {}", last_error()));
            }
        }
        Ok(code)
    }
}

/// How [`Running::close_gracefully`] ended the process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closed {
    /// It had already exited, with this code.
    AlreadyExited(u32),
    /// It quit after `WM_CLOSE` to its `windows`, with this exit code.
    Closed { windows: usize, code: u32 },
    /// It ignored the request (or had no window) and was terminated.
    Terminated { windows: usize },
}

/// The system's per-thread input-method windows, which every GUI thread
/// owns and which are not the program's.
const IME_CLASSES: [&str; 2] = ["IME", "MSCTFIME UI"];

/// The visible or hidden top-level windows owned by process `pid`, without
/// the system's input-method windows.
pub fn top_level_windows(pid: u32) -> Vec<HWND> {
    struct Search {
        pid: u32,
        found: Vec<HWND>,
    }
    unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> BOOL {
        // SAFETY: `data` is the Search below, alive for the enumeration.
        unsafe {
            let search = &mut *(data as *mut Search);
            let mut owner = 0u32;
            GetWindowThreadProcessId(hwnd, &mut owner);
            if owner == search.pid {
                let mut class = [0u16; 64];
                let len = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32);
                let class = String::from_utf16_lossy(&class[..len.max(0) as usize]);
                if !IME_CLASSES.contains(&class.as_str()) {
                    search.found.push(hwnd);
                }
            }
        }
        1
    }
    let mut search = Search {
        pid,
        found: Vec::new(),
    };
    // SAFETY: the callback only touches `search` during this call.
    unsafe {
        EnumWindows(Some(visit), (&raw mut search) as LPARAM);
    }
    search.found
}

/// An installed hook, removed on drop: bind it (`let _hook = …`), since
/// `let _ = …` would remove it at once.
#[derive(Debug)]
#[must_use = "the hook is removed when this is dropped"]
pub struct MessageHook(windows_sys::Win32::UI::WindowsAndMessaging::HHOOK);

impl Drop for MessageHook {
    fn drop(&mut self) {
        // SAFETY: the handle came from SetWindowsHookExW and is released once.
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::UnhookWindowsHookEx(self.0);
        }
    }
}

/// Run on Windows or under Wine (`scripts/wine-test`), with the
/// `close_child` example as the child.
#[cfg(test)]
mod tests {
    use super::*;

    /// Headless Wine (no display, as `scripts/wine-test` runs) can only
    /// create windows through its null graphics driver; select it in the
    /// private test prefix when nothing chose a driver.
    pub(crate) fn headless_wine_windows() {
        let headless = ["DISPLAY", "WAYLAND_DISPLAY"]
            .iter()
            .all(|name| std::env::var_os(name).is_none());
        if crate::wine::is_wine() && headless {
            crate::reg::Key::create(crate::reg::Hive::CurrentUser, r"Software\Wine\Drivers")
                .and_then(|key| key.set_string_if_missing("Graphics", "null"))
                .unwrap();
        }
    }

    fn child(mode: &str) -> Running {
        headless_wine_windows();
        let exe = std::env::current_exe().unwrap();
        let examples = exe.parent().unwrap().parent().unwrap().join("examples");
        let running = Launch::new(examples.join("close_child.exe"))
            .arg(mode)
            .spawn()
            .unwrap();
        if mode != "console" {
            // Wait for its window.
            for _ in 0..200 {
                if !top_level_windows(running.pid()).is_empty() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
        }
        running
    }

    #[test]
    fn a_window_that_honours_close_is_closed() {
        let game = child("window");
        assert!(game.is_alive());
        let closed = game.close_gracefully(Duration::from_secs(10)).unwrap();
        assert!(
            matches!(
                closed,
                Closed::Closed {
                    windows: 1..,
                    code: 0
                }
            ),
            "{closed:?}"
        );
        assert!(!game.is_alive());
        assert_eq!(
            game.close_gracefully(Duration::ZERO).unwrap(),
            Closed::AlreadyExited(0)
        );
    }

    #[test]
    fn a_deaf_window_or_no_window_is_terminated() {
        let deaf = child("deaf");
        let closed = deaf.close_gracefully(Duration::from_millis(300)).unwrap();
        assert_eq!(closed, Closed::Terminated { windows: 1 });
        assert_eq!(deaf.exit_code(), Some(1));

        let console = child("console");
        assert_eq!(
            console.close_gracefully(Duration::from_secs(10)).unwrap(),
            Closed::Terminated { windows: 0 }
        );
        assert!(!console.is_alive());
        assert_eq!(console.wait().unwrap(), 1);
    }
}
