//! GPU preferences, each written only when the player has none.
//!
//! - [`prefer_fast_gpu`]: Windows' per-program "High performance" choice
//!   (`GpuPreference=2;` under `UserGpuPreferences`), for laptops with two
//!   GPUs. Same in CMR3/CMR4/SRR/OR; under Wine the Linux helper's GPU
//!   environment does this instead.
//! - [`prefer_glx`]: Wine `UseEGL = N`. Wine 11 draws OpenGL through EGL by
//!   default, and where the driver has no EGL for X11 (NVIDIA without
//!   egl-x11) that falls back to software; GLX reaches the GPU. The settings
//!   window is the only OpenGL in these launchers (CMR3's note).
//!   Wine reads the setting when the process starts, so a fresh prefix's
//!   first start still uses EGL, and the window's OpenGL setup fails
//!   there. Call [`prefer_glx_or_restart`] before opening the window: when
//!   it had to write the value it runs the launcher again, which then
//!   draws through GLX (CMR2005's restart, made one).

use std::path::Path;

use crate::reg::{Error, Hive, Key};

/// Selects the high-performance GPU for `exe` unless a choice exists;
/// true when it wrote.
pub fn prefer_fast_gpu(exe: &Path) -> Result<bool, Error> {
    Key::create(
        Hive::CurrentUser,
        r"Software\Microsoft\DirectX\UserGpuPreferences",
    )?
    .set_string_if_missing(&exe.display().to_string(), "GpuPreference=2;")
}

/// Under Wine: OpenGL through GLX unless `UseEGL` is set; true when it wrote.
pub fn prefer_glx() -> Result<bool, Error> {
    Key::create(Hive::CurrentUser, r"Software\Wine\X11 Driver")?
        .set_string_if_missing("UseEGL", "N")
}

/// The variable [`prefer_glx_or_restart`] sets on the process it starts,
/// so that one never starts another.
pub const GLX_RESTARTED_ENV: &str = "REA_GLX_RESTARTED";

/// Whether the launcher must run itself again: under Wine, the value was
/// written by this process (`wrote`), and this process is not already the
/// restarted one.
pub fn needs_restart(wine: bool, wrote: bool, restarted: bool) -> bool {
    wine && wrote && !restarted
}

/// What [`prefer_glx_or_restart`] did.
#[derive(Debug)]
pub enum Glx {
    /// Go on in this process and open the window: GLX was already chosen
    /// (or `UseEGL` is the player's), native Windows, or this is the
    /// restarted process. `instance` is the guard passed in; `warning` a
    /// line for `launcher.log` when something failed (the window is tried
    /// anyway).
    Ready {
        instance: Option<crate::instance::Guard>,
        warning: Option<String>,
    },
    /// The launcher ran itself again with GLX in effect and that run has
    /// ended: exit with its code. The instance guard was released for it.
    Restarted { exit_code: u32 },
}

/// Under Wine, before the settings window: sets `UseEGL = N` unless set
/// ([`prefer_glx`]); when this call wrote it, releases `instance` (the
/// next process takes it), runs this launcher again with the same
/// arguments, waits for it and returns its exit code. Otherwise hands the
/// guard back. The restarted process is marked ([`GLX_RESTARTED_ENV`]) and
/// never restarts again. Native Windows: `Ready` at once.
///
/// ```no_run
/// # fn main() -> Result<(), String> {
/// use rea_launcher_win32::gpu::{self, Glx};
/// let guard = rea_launcher_win32::instance::single("rea-game").ok();
/// let _instance = match gpu::prefer_glx_or_restart(guard) {
///     Glx::Restarted { exit_code } => std::process::exit(exit_code as i32),
///     Glx::Ready { instance, warning } => {
///         if let Some(warning) = warning {
///             eprintln!("warning: {warning}"); // launcher.log
///         }
///         instance
///     }
/// };
/// // ... open the settings window
/// # Ok(()) }
/// ```
pub fn prefer_glx_or_restart(instance: Option<crate::instance::Guard>) -> Glx {
    glx_or_restart(instance, std::env::args_os().skip(1).collect())
}

fn glx_or_restart(instance: Option<crate::instance::Guard>, args: Vec<std::ffi::OsString>) -> Glx {
    let ready = |instance, warning| Glx::Ready { instance, warning };
    let wine = crate::wine::is_wine();
    if !wine {
        return ready(instance, None);
    }
    let wrote = match prefer_glx() {
        Ok(wrote) => wrote,
        Err(error) => return ready(instance, Some(format!("OpenGL through GLX: {error}"))),
    };
    let restarted = std::env::var_os(GLX_RESTARTED_ENV).is_some();
    if !needs_restart(wine, wrote, restarted) {
        return ready(instance, None);
    }
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(error) => {
            return ready(
                instance,
                Some(format!("OpenGL through GLX from the next start: {error}")),
            );
        }
    };
    // The next process takes the single-instance lock.
    drop(instance);
    match crate::process::Launch::new(&exe)
        .args(args)
        .env(GLX_RESTARTED_ENV, "1")
        .run()
    {
        Ok(exit_code) => Glx::Restarted { exit_code },
        Err(error) => ready(
            None,
            Some(format!("starting again for OpenGL through GLX: {error}")),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_first_start_under_wine_restarts() {
        assert!(needs_restart(true, true, false));
        // GLX already set (or the player's UseEGL): go on.
        assert!(!needs_restart(true, false, false));
        // The restarted process never restarts again.
        assert!(!needs_restart(true, true, true));
        // Native Windows never.
        assert!(!needs_restart(false, true, false));
    }

    /// Under Wine: with `UseEGL` absent, the call writes it and runs this
    /// test binary again (only this test); that run finds it set and is
    /// marked, so it goes on with `Ready`; the first sees its exit code.
    /// The test prefix's `UseEGL` is put back as it was.
    #[test]
    fn a_fresh_prefix_restarts_the_launcher_once() {
        use crate::reg::{Hive, Key};
        if !crate::wine::is_wine() {
            return;
        }
        let name = "gpu::tests::a_fresh_prefix_restarts_the_launcher_once";
        if std::env::var_os(GLX_RESTARTED_ENV).is_some() {
            // The restarted run.
            match glx_or_restart(None, vec![]) {
                Glx::Ready { warning: None, .. } => return,
                other => panic!("the restarted run went on: {other:?}"),
            }
        }
        let key = Key::create(Hive::CurrentUser, r"Software\Wine\X11 Driver").unwrap();
        let before = key.string("UseEGL").unwrap();
        key.delete_value("UseEGL").unwrap();
        let args = [name, "--exact", "--test-threads=1"]
            .map(Into::into)
            .to_vec();
        let outcome = glx_or_restart(None, args);
        assert_eq!(key.string("UseEGL").unwrap().as_deref(), Some("N"));
        // Set now: no restart.
        assert!(matches!(
            glx_or_restart(None, vec![]),
            Glx::Ready { warning: None, .. }
        ));
        match before {
            Some(value) => key.set_string("UseEGL", &value).unwrap(),
            None => {
                key.delete_value("UseEGL").unwrap();
            }
        }
        assert!(
            matches!(outcome, Glx::Restarted { exit_code: 0 }),
            "{outcome:?}"
        );
    }
}
