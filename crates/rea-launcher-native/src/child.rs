//! A child process that never outlives the launcher (docs/STANDARDS.md,
//! "Native launchers"): if the launcher is killed, its emulator goes too,
//! instead of running on without input or a window to return to. From
//! rea-rad-mobile `child.rs`.
//!
//! - **Linux:** `PR_SET_PDEATHSIG(SIGKILL)` in the child before `exec`. It
//!   fires when the *thread* that spawned the child exits, so spawn from a
//!   thread that outlives the child (the main thread, or one that waits for
//!   it). A launcher that died before the call is caught too: the child
//!   checks its parent and refuses to start.
//! - **Windows:** the child joins a job object with
//!   `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, one per launcher process, never
//!   closed: Windows closes it, and kills the job, when the launcher ends.
//! - Elsewhere it is a plain spawn.
//!
//! Failing to tie the child never fails the spawn on Windows (the safety
//! net is lost, the game still runs).

use std::io;
use std::process::{Child, Command};

/// Spawns `cmd` so that its process dies with this one.
pub fn spawn_tied(cmd: &mut Command) -> io::Result<Child> {
    imp::spawn_tied(cmd)
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use std::os::unix::process::CommandExt;

    pub fn spawn_tied(cmd: &mut Command) -> io::Result<Child> {
        let parent = std::process::id();
        // SAFETY: the closure runs between fork and exec and only makes
        // async-signal-safe calls (prctl, getppid) on plain integers.
        unsafe {
            cmd.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(io::Error::last_os_error());
                }
                // The launcher may have died before prctl took effect.
                if libc::getppid() as u32 != parent {
                    return Err(io::Error::other(
                        "the launcher exited before its child started",
                    ));
                }
                Ok(())
            });
        }
        cmd.spawn()
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::os::windows::io::AsRawHandle;
    use std::sync::OnceLock;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    /// The launcher's kill-on-close job: a handle kept for the process's
    /// lifetime (as an address, so it can live in a static).
    struct Job(usize);

    impl Job {
        fn create() -> Option<Job> {
            // SAFETY: plain Win32 calls; the struct is zeroed plain data
            // and outlives the call that reads it.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return None;
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    (&raw const info).cast(),
                    size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                // A job without the limit would only keep the child: use none.
                (ok != 0).then_some(Job(job as usize))
            }
        }

        fn assign(&self, child: &Child) -> bool {
            // SAFETY: the job handle is open for the process's lifetime and
            // the child's handle is open while `child` lives.
            unsafe { AssignProcessToJobObject(self.0 as _, child.as_raw_handle() as _) != 0 }
        }
    }

    pub fn spawn_tied(cmd: &mut Command) -> io::Result<Child> {
        static JOB: OnceLock<Option<Job>> = OnceLock::new();
        let child = cmd.spawn()?;
        if let Some(job) = JOB.get_or_init(Job::create) {
            job.assign(&child);
        }
        Ok(child)
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
mod imp {
    use super::*;

    pub fn spawn_tied(cmd: &mut Command) -> io::Result<Child> {
        cmd.spawn()
    }
}
