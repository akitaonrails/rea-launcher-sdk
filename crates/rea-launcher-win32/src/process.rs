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

use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::System::Threading::{
    CREATE_NO_WINDOW, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, GetExitCodeProcess, INFINITE,
    PROCESS_INFORMATION, STARTUPINFOW, WaitForSingleObject,
};

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
