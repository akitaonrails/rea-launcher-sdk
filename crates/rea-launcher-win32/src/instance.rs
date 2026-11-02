//! One launcher at a time (docs/STANDARDS.md, "Clean exit, single
//! instance"): a named mutex held for the launcher's lifetime. A second
//! start finds it, brings the first launcher's window forward and exits.
//!
//! ```no_run
//! # #[cfg(windows)] fn main() {
//! use rea_launcher_win32::instance::{self, Window};
//! let _instance = match instance::single("rea-outrun-2006") {
//!     Ok(guard) => guard,
//!     Err(running) => {
//!         running.bring_to_front(Window::Title("OutRun 2006"));
//!         return;
//!     }
//! };
//! // ... the launcher; the guard drops at exit
//! # }
//! # #[cfg(not(windows))] fn main() {}
//! ```
//!
//! The mutex lives in the `Local\` namespace: one instance per user
//! session. Under Wine that is per prefix (wineserver). On Linux the play
//! helper's session lock does the same job.

/// The kernel object name for `name`: `Local\rea-launcher-<name>`, with
/// characters a name can't hold (`\`) replaced.
pub fn mutex_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .map(|c| if c == '\\' || c.is_control() { '_' } else { c })
        .collect();
    format!(r"Local\rea-launcher-{clean}")
}

/// How to find the running launcher's window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window<'a> {
    /// Its title (eframe sets the title passed to `run_native`).
    Title(&'a str),
    /// Its window class.
    Class(&'a str),
}

#[cfg(windows)]
pub use windows::*;

#[cfg(windows)]
mod windows {
    use std::ptr::null;

    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, IsIconic, SW_RESTORE, SetForegroundWindow, ShowWindow,
    };

    use super::Window;
    use crate::wide::wide;

    /// Held while this launcher runs; dropping it lets the next one start.
    #[derive(Debug)]
    #[must_use = "the instance is released when this is dropped"]
    pub struct Guard(HANDLE);

    impl Drop for Guard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: the handle came from CreateMutexW and is closed once.
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }

    /// Another launcher with the same name is running.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct AlreadyRunning;

    impl std::fmt::Display for AlreadyRunning {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("the launcher is already running")
        }
    }

    impl std::error::Error for AlreadyRunning {}

    impl AlreadyRunning {
        /// Restores and focuses the running launcher's window; false when
        /// no such window was found (it may be the game that is running).
        pub fn bring_to_front(&self, window: Window<'_>) -> bool {
            let (class, title) = match window {
                Window::Title(title) => (None, Some(wide(title))),
                Window::Class(class) => (Some(wide(class)), None),
            };
            // SAFETY: NUL-terminated strings (or null) for FindWindowW; the
            // other calls take the window handle it returned.
            unsafe {
                let hwnd = FindWindowW(
                    class.as_ref().map_or(null(), |c| c.as_ptr()),
                    title.as_ref().map_or(null(), |t| t.as_ptr()),
                );
                if hwnd.is_null() {
                    return false;
                }
                if IsIconic(hwnd) != 0 {
                    ShowWindow(hwnd, SW_RESTORE);
                }
                SetForegroundWindow(hwnd);
            }
            true
        }
    }

    /// Takes the instance `name` (see [`super::mutex_name`]), or reports
    /// that another launcher holds it. When the mutex can't be created at
    /// all the launcher runs unguarded rather than refusing to start.
    pub fn single(name: &str) -> Result<Guard, AlreadyRunning> {
        match create(name) {
            Ok(guard) => Ok(guard),
            Err(Some(running)) => Err(running),
            // Could not create the object at all: run unguarded rather
            // than refuse to start.
            Err(None) => Ok(Guard(std::ptr::null_mut())),
        }
    }

    fn create(name: &str) -> Result<Guard, Option<AlreadyRunning>> {
        let name = wide(super::mutex_name(name));
        // SAFETY: a NUL-terminated name and default security; the handle
        // is owned by the Guard (or closed right away when not ours).
        unsafe {
            let handle = CreateMutexW(null(), 0, name.as_ptr());
            if handle.is_null() {
                return Err(None);
            }
            if GetLastError() == ERROR_ALREADY_EXISTS {
                CloseHandle(handle);
                return Err(Some(AlreadyRunning));
            }
            Ok(Guard(handle))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_local_and_clean() {
        assert_eq!(mutex_name("rea-cmr4"), r"Local\rea-launcher-rea-cmr4");
        assert_eq!(mutex_name(r"a\b"), r"Local\rea-launcher-a_b");
    }

    /// Windows/Wine: a second acquisition fails until the first is dropped.
    #[cfg(windows)]
    #[test]
    fn one_instance_at_a_time() {
        let name = format!("test-{}", std::process::id());
        let first = single(&name).expect("first instance");
        assert_eq!(single(&name).unwrap_err(), AlreadyRunning);
        // Also from another thread (a second handle).
        let from_thread = {
            let name = name.clone();
            std::thread::spawn(move || single(&name).is_err())
                .join()
                .unwrap()
        };
        assert!(from_thread);
        drop(first);
        let again = single(&name).expect("free after the first exits");
        drop(again);
        assert!(!AlreadyRunning.bring_to_front(Window::Title("no such window rea-test")));
    }
}
