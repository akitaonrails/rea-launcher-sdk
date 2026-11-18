//! A game session's processes, started and stopped as one group, tied to
//! the launcher. From the TeknoParrot hubs' `proc.rs` (sega's, the most
//! complete), with their process bugs fixed.
//!
//! - **Linux:** the session's first process (the leader, usually
//!   gamescope) starts in a process group of its own, dies with the
//!   launcher (`PR_SET_PDEATHSIG`, as [`crate::child::spawn_tied`]) and
//!   carries the session's [`Tag`] in its environment, which every process
//!   it starts inherits: Wine and bubblewrap move processes out of the
//!   group, the tag still finds them. The group is signalled only while
//!   its leader is unreaped (its exit is read with `WNOWAIT`), so its id
//!   can't have been reused; the leader is reaped in [`Group::finish`],
//!   which consumes the group. A tagged process is signalled through a
//!   pidfd opened *before* its tag is read, so a reused pid is never hit.
//!   Zombies are never counted or signalled. A killed launcher takes the
//!   whole session along: a small sentinel shell, started with the
//!   session, kills every tagged process once the launcher is gone (see
//!   [`Group::warning`] when it couldn't start).
//! - **Windows:** the leader starts suspended and joins a kill-on-close job
//!   before its first instruction, so none of its children escape; a
//!   failed join only loses that net (see [`Group::warning`]) and the game
//!   runs. Wine's jobs don't always hold a process's children, so they are
//!   also found by parent and creation time
//!   ([`rea_launcher::procs::descendants_at`]), each ended only while its
//!   handle shows that same creation time. [`Group::terminate`] asks
//!   politely: `WM_CLOSE` to the leader's windows and to the visible
//!   windows of the session's other processes (Flycast and Dolphin save,
//!   then quit; windowsarcadeloader's and OpenParrotLoader64's game window
//!   is their child's; a console loader has none and ends when its input
//!   closes).
//!
//! Spawn from the thread that waits for the session (Linux ties the leader
//! to the spawning thread). What is signalled is the session's: its group
//! and tagged processes on Linux, its job and descendants on Windows, plus
//! what the caller asks for by name ([`Group::terminate_one`], and
//! [`Group::kill_under`]: any program inside a folder, started after the
//! leader). The one look-then-kill without a held handle is the Linux
//! sentinel's, after the launcher died: it reads each pid's tag again right
//! before the kill, and a pid reused within those few milliseconds could
//! be hit.

use std::io;
use std::process::{ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as imp;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows as imp;

#[cfg(windows)]
pub use windows::{Window, logical_processor_information};

/// What marks a session's processes: on Linux the environment variable
/// `var` set to `value` (a launcher's own name, e.g. `SEGA_SESSION`, and
/// something unique to the session). Every process carrying the same tag
/// is treated as this session's and killed with it, so `value` must be
/// unique per session: the hubs use the session's runtime folder, and the
/// launcher's instance lock ([`rea_launcher::instance`]) is what keeps two
/// sessions with that folder from running at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    var: &'static str,
    value: String,
    #[cfg(windows)]
    cpus: Vec<u32>,
}

impl Tag {
    pub fn new(var: &'static str, value: impl Into<String>) -> Tag {
        Tag {
            var,
            value: value.into(),
            #[cfg(windows)]
            cpus: Vec::new(),
        }
    }

    /// Windows: keep the session on these logical CPUs (processor group
    /// 0; `rea_launcher::cpus::windows_cpus` of
    /// [`logical_processor_information`], then `uniform_cpus`), set before
    /// its first instruction, so every process it starts inherits them. On
    /// Linux put `taskset` in the command line instead
    /// (`cpus::taskset_list`: it pins the game without pinning gamescope).
    #[cfg(windows)]
    pub fn cpus(mut self, cpus: &[u32]) -> Tag {
        self.cpus = cpus.to_vec();
        self
    }
}

/// The pipes of the session's leader, as `cmd` asked for them.
pub struct Pipes {
    pub stdin: Option<ChildStdin>,
    pub stdout: Option<ChildStdout>,
    pub stderr: Option<ChildStderr>,
}

/// A process of the session (Linux): its id and arguments.
#[cfg(target_os = "linux")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub pid: u32,
    pub argv: Vec<String>,
}

/// How a session ended.
#[derive(Debug)]
pub struct Finished {
    /// The leader's exit status.
    pub status: ExitStatus,
    /// The session's processes still running after the leader was reaped,
    /// now killed.
    pub leftovers: usize,
}

/// A running session. Dropping it without [`Group::finish`] (an early
/// return, a panic) kills it: on Linux the group and its tagged processes,
/// on Windows the job and the leader's descendants.
pub struct Group(imp::Group);

impl Group {
    /// Starts `cmd` as a new session marked with `tag`. On Windows it sets
    /// `cmd`'s creation flags (suspended, no console window of its own: the
    /// loaders are console programs).
    /// A program that isn't there is named: "gamescope is not installed
    /// (or not on PATH)".
    pub fn spawn(cmd: &mut Command, tag: &Tag) -> io::Result<Group> {
        imp::Group::spawn(cmd, tag).map(Group).map_err(|e| {
            if e.kind() != io::ErrorKind::NotFound {
                return e;
            }
            let program = cmd.get_program().to_string_lossy().into_owned();
            io::Error::new(
                e.kind(),
                format!("{program} is not installed (or not on PATH)"),
            )
        })
    }

    /// The leader's process id.
    pub fn id(&self) -> u32 {
        self.0.id()
    }

    /// Why the session is less tied than it should be (Windows: the job
    /// couldn't take the leader; Linux: no sentinel, or it ended early,
    /// noticed by [`Group::exited`]), for the session log.
    pub fn warning(&self) -> Option<&str> {
        self.0.warning()
    }

    /// The leader's pipes (each once).
    pub fn take_pipes(&mut self) -> Pipes {
        self.0.take_pipes()
    }

    /// The leader's exit status once it has ended, without reaping it.
    pub fn exited(&mut self) -> io::Result<Option<ExitStatus>> {
        self.0.exited()
    }

    /// Waits for the leader to end, without reaping it.
    pub fn wait_exit(&mut self) -> io::Result<ExitStatus> {
        self.0.wait_exit()
    }

    /// Asks the session to end: Linux `SIGTERM` to the group, Windows
    /// `WM_CLOSE` to the leader's windows (hidden ones too) and to the
    /// visible windows of the session's other processes.
    pub fn terminate(&self) {
        self.0.terminate()
    }

    /// Kills the session: the group, its tagged processes (Linux), the job
    /// and the leader's descendants (Windows).
    pub fn kill(&self) {
        self.0.kill()
    }

    /// Whether any process of the session still runs (the leader too, until
    /// it has ended).
    pub fn any_left(&self) -> bool {
        self.0.any_left()
    }

    /// Asks one process of the session to end (`SIGTERM`; F355's master
    /// board, which saves before it lets the slave go). `false`, and no
    /// signal, when `pid` is not a running process of this session.
    #[cfg(target_os = "linux")]
    pub fn terminate_one(&self, pid: u32) -> bool {
        self.0.terminate_one(pid)
    }

    /// The session's running processes (zombies left out), the leader too.
    #[cfg(target_os = "linux")]
    pub fn members(&self) -> Vec<Member> {
        self.0.members()
    }

    /// The top-level windows of the session's processes (the leader, its
    /// descendants, the job's), hidden ones too.
    #[cfg(windows)]
    pub fn windows(&self) -> Vec<Window> {
        self.0.windows()
    }

    /// Ends every process whose program lies inside `dir` (the session's
    /// runtime folder) and that started after the leader: what a loader
    /// started outside both the job and its own descendants (OpenParrot's
    /// auth daemon). How many.
    #[cfg(windows)]
    pub fn kill_under(&self, dir: &std::path::Path) -> usize {
        self.0.kill_under(dir)
    }

    /// Waits for the leader to end (call it once it has, or after
    /// [`Group::kill`]), reaps it, and kills what is left of the session.
    pub fn finish(self) -> io::Result<Finished> {
        self.0.finish()
    }
}
