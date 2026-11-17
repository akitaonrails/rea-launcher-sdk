//! Linux: a process group, the session's tag, pidfds.

use std::fs;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::process::{Child, Command, ExitStatus};

use rea_launcher::procs;

use super::{Finished, Member, Pipes, Tag};

pub struct Group {
    child: Child,
    tag: Tag,
    /// The leader's status once read (it stays unreaped until `finish`).
    status: Option<ExitStatus>,
    reaped: bool,
}

impl Group {
    pub fn spawn(cmd: &mut Command, tag: &Tag) -> io::Result<Group> {
        cmd.env(tag.var, &tag.value).process_group(0);
        crate::child::tie(cmd);
        Ok(Group {
            child: cmd.spawn()?,
            tag: tag.clone(),
            status: None,
            reaped: false,
        })
    }

    pub fn id(&self) -> u32 {
        self.child.id()
    }

    pub fn warning(&self) -> Option<&str> {
        None
    }

    pub fn take_pipes(&mut self) -> Pipes {
        Pipes {
            stdin: self.child.stdin.take(),
            stdout: self.child.stdout.take(),
            stderr: self.child.stderr.take(),
        }
    }

    pub fn exited(&mut self) -> io::Result<Option<ExitStatus>> {
        if self.status.is_none() {
            self.status = peek_exit(self.id(), false)?;
        }
        Ok(self.status)
    }

    pub fn wait_exit(&mut self) -> io::Result<ExitStatus> {
        if let Some(st) = self.status {
            return Ok(st);
        }
        let st =
            peek_exit(self.id(), true)?.ok_or_else(|| io::Error::other("waitid: no status"))?;
        self.status = Some(st);
        Ok(st)
    }

    /// The group's id is the leader's pid, which can't be reused while the
    /// leader is unreaped: `&self` exists only until `finish` reaps it.
    fn signal_group(&self, sig: i32) {
        // SAFETY: a plain syscall on integers.
        unsafe { libc::kill(-(self.id() as i32), sig) };
    }

    pub fn terminate(&self) {
        self.signal_group(libc::SIGTERM);
    }

    pub fn terminate_one(&self, pid: u32) -> bool {
        self.tagged()
            .into_iter()
            .find(|p| p.pid == pid)
            .is_some_and(|p| p.signal(libc::SIGTERM))
    }

    pub fn kill(&self) {
        self.signal_group(libc::SIGKILL);
        for p in self.tagged() {
            p.signal(libc::SIGKILL);
        }
    }

    pub fn any_left(&self) -> bool {
        !self.tagged().is_empty()
    }

    pub fn members(&self) -> Vec<Member> {
        self.tagged()
            .into_iter()
            .map(|p| Member {
                argv: fs::read(format!("/proc/{}/cmdline", p.pid))
                    .map(|raw| procs::cmdline(&raw))
                    .unwrap_or_default(),
                pid: p.pid,
            })
            .collect()
    }

    pub fn finish(mut self) -> io::Result<Finished> {
        let status = self.child.wait()?;
        self.reaped = true;
        let left = self.tagged();
        for p in &left {
            p.signal(libc::SIGKILL);
        }
        Ok(Finished {
            status,
            leftovers: left.len(),
        })
    }

    /// The running processes (not zombies) whose environment carries the
    /// session's tag, besides this one, each held by a pidfd opened before
    /// its environment was read.
    fn tagged(&self) -> Vec<Tagged> {
        let Ok(dir) = fs::read_dir("/proc") else {
            return Vec::new();
        };
        let me = std::process::id();
        dir.flatten()
            .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
            .filter(|&pid| pid != me)
            .filter(|&pid| self.is_tagged(pid))
            // held first, then checked again: the pid may have been reused
            .filter_map(|pid| Tagged::open(pid).filter(|_| self.is_tagged(pid)))
            .collect()
    }

    fn is_tagged(&self, pid: u32) -> bool {
        let read = |what| fs::read(format!("/proc/{pid}/{what}")).unwrap_or_default();
        procs::environ_has(&read("environ"), self.tag.var, &self.tag.value)
            && !procs::is_zombie(&String::from_utf8_lossy(&read("stat")))
    }
}

/// A group dropped without `finish` (an error mid-run) takes its session
/// along, as a closed job does on Windows.
impl Drop for Group {
    fn drop(&mut self) {
        if !self.reaped {
            self.kill();
            let _ = self.child.wait();
        }
    }
}

/// A process held by its pidfd: a signal reaches it, or nothing if it has
/// ended, never a process that took its pid since.
struct Tagged {
    pid: u32,
    fd: OwnedFd,
}

impl Tagged {
    fn open(pid: u32) -> Option<Tagged> {
        // SAFETY: a plain syscall; a non-negative result is a new fd we own.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0) };
        (fd >= 0).then(|| Tagged {
            pid,
            // SAFETY: just opened above, owned by nothing else.
            fd: unsafe { OwnedFd::from_raw_fd(fd as i32) },
        })
    }

    fn signal(&self, sig: i32) -> bool {
        // SAFETY: a plain syscall on an fd we own; no siginfo.
        let r = unsafe {
            libc::syscall(
                libc::SYS_pidfd_send_signal,
                self.fd.as_raw_fd(),
                sig,
                std::ptr::null::<libc::siginfo_t>(),
                0,
            )
        };
        r == 0
    }
}

/// The exit status of our child `pid` once it has ended, left unreaped
/// (`WNOWAIT`); `block` waits for it.
fn peek_exit(pid: u32, block: bool) -> io::Result<Option<ExitStatus>> {
    let flags = libc::WEXITED | libc::WNOWAIT | if block { 0 } else { libc::WNOHANG };
    loop {
        // SAFETY: zeroed plain data the call fills in.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        // SAFETY: `info` outlives the call.
        let r = unsafe { libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, flags) };
        if r == -1 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(e);
        }
        // SAFETY: waitid filled in a SIGCHLD siginfo (or left it zeroed).
        let (who, status) = unsafe { (info.si_pid(), info.si_status()) };
        if who == 0 {
            return Ok(None);
        }
        return Ok(Some(wait_status(info.si_code, status)));
    }
}

/// `waitpid`'s status word for what `waitid` reported.
fn wait_status(code: i32, status: i32) -> ExitStatus {
    ExitStatus::from_raw(match code {
        libc::CLD_EXITED => (status & 0xff) << 8,
        libc::CLD_DUMPED => (status & 0x7f) | 0x80,
        _ => status & 0x7f,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waitid_codes_become_exit_statuses() {
        let exited = wait_status(libc::CLD_EXITED, 3);
        assert_eq!(exited.code(), Some(3));
        let killed = wait_status(libc::CLD_KILLED, libc::SIGKILL);
        assert_eq!(
            (killed.code(), killed.signal()),
            (None, Some(libc::SIGKILL))
        );
        let dumped = wait_status(libc::CLD_DUMPED, libc::SIGSEGV);
        assert_eq!(dumped.signal(), Some(libc::SIGSEGV));
        assert!(dumped.core_dumped());
    }
}
