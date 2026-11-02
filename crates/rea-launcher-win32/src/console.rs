//! Terminal output for a GUI-subsystem launcher.
//!
//! The launchers are built with `windows_subsystem = "windows"` (no console
//! window flashes on double-click), so `--help`, `--check` and
//! `--no-window` output would go nowhere. `attach_parent` attaches to the
//! console of the terminal that started the launcher and points stdout and
//! stderr at it, so `println!` reaches that terminal. The pattern:
//!
//! ```no_run
//! # #[cfg(windows)] {
//! use rea_launcher_win32::{console, dialog};
//! let usage = "Usage: MyLauncher [--help] [--check]";
//! let terminal = console::attach_parent();
//! match console::output(terminal, console::stdout_usable(), true) {
//!     console::Output::Terminal => println!("{usage}"),
//!     console::Output::Dialog => dialog::message("MyLauncher", usage, dialog::Icon::Info),
//!     console::Output::Nowhere => {}
//! }
//! # }
//! ```
//!
//! Attach for `--help`, `--check` and `--no-window`; show a dialog only
//! when there is no console (and no redirected output) and the run is
//! interactive (a double-click, not a script).

/// Where a launcher's text output should go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    /// Print: a console is attached, or stdout is redirected to a file or
    /// pipe.
    Terminal,
    /// No console: a message box (interactive runs only).
    Dialog,
    /// No console and not interactive: say nothing (a log may still).
    Nowhere,
}

/// The [`Output`] for a run: `attached` from `attach_parent`, `usable`
/// from `stdout_usable` (true when redirected), `interactive` when a
/// person started it (not `--no-window` from a script).
pub fn output(attached: bool, usable: bool, interactive: bool) -> Output {
    if attached || usable {
        Output::Terminal
    } else if interactive {
        Output::Dialog
    } else {
        Output::Nowhere
    }
}

#[cfg(windows)]
pub use windows::*;

#[cfg(windows)]
mod windows {
    use std::ptr::{null, null_mut};

    use windows_sys::Win32::Foundation::{
        ERROR_ACCESS_DENIED, GENERIC_READ, GENERIC_WRITE, GetLastError, HANDLE,
        INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileType, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_ERROR_HANDLE, STD_HANDLE,
        STD_OUTPUT_HANDLE, SetStdHandle,
    };

    use crate::wide::wide;

    fn usable(handle: HANDLE) -> bool {
        // SAFETY: GetFileType only inspects the handle; 0 is
        // FILE_TYPE_UNKNOWN (no handle, or a closed one).
        !handle.is_null() && handle != INVALID_HANDLE_VALUE && unsafe { GetFileType(handle) } != 0
    }

    /// Whether stdout leads somewhere: a console, a file or a pipe (a
    /// GUI-subsystem process started by a double-click has none).
    pub fn stdout_usable() -> bool {
        // SAFETY: a plain query of the process's standard handle.
        usable(unsafe { GetStdHandle(STD_OUTPUT_HANDLE) })
    }

    /// Attaches to the console of the process that started this one (the
    /// terminal) and points stdout and stderr at it, unless they already
    /// lead somewhere (redirected to a file or pipe, which is kept). True
    /// when a console is attached (also when one already was).
    pub fn attach_parent() -> bool {
        // SAFETY: AttachConsole takes a process id; ATTACH_PARENT_PROCESS
        // means the parent's console.
        let attached = unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } != 0;
        // ERROR_ACCESS_DENIED: this process already has a console.
        let already = !attached && unsafe { GetLastError() } == ERROR_ACCESS_DENIED;
        if attached {
            for which in [STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
                rebind(which);
            }
        }
        attached || already
    }

    /// Points `which` at the console's output buffer when it leads nowhere.
    fn rebind(which: STD_HANDLE) {
        // SAFETY: standard-handle queries and a CreateFileW of the
        // console device with a NUL-terminated name; the new handle is
        // handed to the process as its standard handle and lives as long.
        unsafe {
            if usable(GetStdHandle(which)) {
                return;
            }
            let console = CreateFileW(
                wide("CONOUT$").as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                null(),
                OPEN_EXISTING,
                0,
                null_mut(),
            );
            if console != INVALID_HANDLE_VALUE {
                SetStdHandle(which, console);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_goes_to_the_terminal_a_dialog_or_nowhere() {
        assert_eq!(output(true, false, true), Output::Terminal);
        // Redirected (`--help > help.txt`): print even without a console.
        assert_eq!(output(false, true, true), Output::Terminal);
        assert_eq!(output(false, false, true), Output::Dialog);
        assert_eq!(output(false, false, false), Output::Nowhere);
    }

    /// Windows/Wine: the test binary's stdout is the harness's pipe; it
    /// stays usable and printing still works after attaching.
    #[cfg(windows)]
    #[test]
    fn attaching_keeps_redirected_output() {
        let usable_before = stdout_usable();
        let attached = attach_parent();
        if usable_before {
            assert!(stdout_usable(), "a redirected stdout is kept");
        }
        assert_eq!(
            output(attached, stdout_usable(), true),
            Output::Terminal,
            "the harness can always print"
        );
        use std::io::Write;
        writeln!(std::io::stdout(), "console attach test").unwrap();
    }
}
