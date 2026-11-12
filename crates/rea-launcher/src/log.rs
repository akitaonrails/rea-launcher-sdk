//! `launcher.log`: an append-only log with a size cap.
//!
//! From SCR edb692f: when no window can be made (no display, no OpenGL)
//! the launcher plays with saved settings, and warnings must still reach
//! the player somewhere. The file starts afresh once it passes the cap
//! ([`DEFAULT_LIMIT`]; [`Log::with_limit`] changes it, [`Log::without_limit`]
//! removes it), and logging never fails a launch. Levels: [`Log::info`]
//! (log only), [`Log::warn`] and [`Log::error`] (log and stderr), or a
//! plain [`Log::line`].
//!
//! Two opt-in options from rea-rad-mobile's `log.rs`: [`Log::per_run`]
//! starts each run's log afresh and keeps the previous run's as
//! `<name>.old` (so the first line is always this run's version, which
//! support advice relies on), and [`Log::timestamps`] puts `[secs.millis]`
//! in front of every line.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// The usual cap: 1 MiB.
pub const DEFAULT_LIMIT: u64 = 1 << 20;

/// A log file and its cap. Clones share the [`Log::per_run`] state.
#[derive(Debug, Clone)]
pub struct Log {
    path: PathBuf,
    /// `None`: never started afresh.
    limit: Option<u64>,
    /// `Some`: this run's first line moves the old file aside; the flag
    /// says it happened.
    per_run: Option<Arc<AtomicBool>>,
    timestamps: bool,
}

impl PartialEq for Log {
    fn eq(&self, other: &Log) -> bool {
        self.path == other.path
            && self.limit == other.limit
            && self.per_run.is_some() == other.per_run.is_some()
            && self.timestamps == other.timestamps
    }
}

impl Eq for Log {}

impl Log {
    /// `path` with the [`DEFAULT_LIMIT`].
    pub fn new(path: impl Into<PathBuf>) -> Log {
        Log {
            path: path.into(),
            limit: Some(DEFAULT_LIMIT),
            per_run: None,
            timestamps: false,
        }
    }

    /// One file per run: the first line this `Log` (or a clone) writes
    /// moves an existing file to `<name>.old`, replacing the one before,
    /// and starts afresh. Make the `Log` once per run and clone it for
    /// threads; the cap still applies within a run.
    pub fn per_run(mut self) -> Log {
        self.per_run = Some(Arc::new(AtomicBool::new(false)));
        self
    }

    /// Puts `[secs.millis] ` (seconds since the Unix epoch) in front of
    /// every line, or not.
    pub fn timestamps(mut self, on: bool) -> Log {
        self.timestamps = on;
        self
    }

    /// Where [`Log::per_run`] keeps the previous run: `launcher.log.old`.
    pub fn old_path(&self) -> PathBuf {
        let mut name = self.path.file_name().unwrap_or_default().to_os_string();
        name.push(".old");
        self.path.with_file_name(name)
    }

    /// Starts the file afresh once it is larger than `bytes`.
    pub fn with_limit(mut self, bytes: u64) -> Log {
        self.limit = Some(bytes);
        self
    }

    /// Never starts the file afresh: it grows without a cap.
    pub fn without_limit(mut self) -> Log {
        self.limit = None;
        self
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Appends one line (folders created; a file past the cap is started
    /// afresh). Errors are ignored.
    pub fn line(&self, line: &str) {
        if let Some(dir) = self.path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let new_run = self
            .per_run
            .as_ref()
            .is_some_and(|started| !started.swap(true, Ordering::SeqCst));
        if new_run && self.path.is_file() {
            let _ = fs::rename(&self.path, self.old_path());
        }
        let fresh = new_run
            || self
                .limit
                .is_some_and(|limit| fs::metadata(&self.path).is_ok_and(|m| m.len() > limit));
        if let Ok(mut f) = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(!fresh)
            .truncate(fresh)
            .open(&self.path)
        {
            if self.timestamps {
                let secs = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0.0, |d| d.as_secs_f64());
                let _ = writeln!(f, "[{secs:.3}] {line}");
            } else {
                let _ = writeln!(f, "{line}");
            }
        }
    }

    /// `info: …` to the log only: what the launcher did (copied,
    /// patched, chose), for a bug report, without cluttering the console.
    pub fn info(&self, message: &str) {
        self.line(&format!("info: {message}"));
    }

    /// `warning: …` to the log and stderr.
    pub fn warn(&self, message: &str) {
        eprintln!("warning: {message}");
        self.line(&format!("warning: {message}"));
    }

    /// `error: …` to the log and stderr.
    pub fn error(&self, message: &str) {
        eprintln!("{message}");
        self.line(&format!("error: {message}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_dir;

    #[test]
    fn appends_and_starts_afresh_when_too_big() {
        let dir = test_dir("log");
        let log = Log::new(dir.join("sub/launcher.log")).with_limit(16);
        log.line("one");
        log.line("two");
        assert_eq!(fs::read_to_string(log.path()).unwrap(), "one\ntwo\n");
        fs::write(log.path(), vec![b'x'; 17]).unwrap();
        log.warn("three");
        assert_eq!(fs::read_to_string(log.path()).unwrap(), "warning: three\n");
        log.info("synced 3 files");
        assert!(
            fs::read_to_string(log.path())
                .unwrap()
                .ends_with("warning: three\ninfo: synced 3 files\n")
        );
        // Without a cap it only grows.
        let uncapped = Log::new(log.path()).with_limit(1).without_limit();
        uncapped.line("four");
        assert_eq!(
            fs::read_to_string(log.path()).unwrap(),
            "warning: three\ninfo: synced 3 files\nfour\n"
        );
        // A log that cannot be written is silently skipped.
        Log::new(dir.join("sub")).line("into a folder");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn each_run_starts_afresh_and_keeps_the_last_one() {
        let dir = test_dir("log-per-run");
        let path = dir.join("launcher.log");
        fs::write(&path, "an older run\n").unwrap();
        // run 1: a clone for the input thread shares the run
        let run1 = Log::new(&path).per_run();
        let thread = run1.clone();
        run1.line("Launcher 1.0");
        thread.info("input thread");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Launcher 1.0\ninfo: input thread\n"
        );
        assert_eq!(
            fs::read_to_string(run1.old_path()).unwrap(),
            "an older run\n"
        );
        // run 2: the first line is its own version; run 1 is the .old
        let run2 = Log::new(&path).per_run();
        run2.line("Launcher 1.1");
        assert_eq!(fs::read_to_string(&path).unwrap(), "Launcher 1.1\n");
        assert_eq!(
            fs::read_to_string(dir.join("launcher.log.old")).unwrap(),
            "Launcher 1.0\ninfo: input thread\n"
        );
        // the default still appends
        Log::new(&path).line("appended");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "Launcher 1.1\nappended\n"
        );
        assert_ne!(Log::new(&path), run2);
        assert_eq!(run2, Log::new(&path).per_run());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn timestamps_are_seconds_and_millis() {
        let dir = test_dir("log-stamps");
        let log = Log::new(dir.join("launcher.log")).timestamps(true);
        log.warn("slow");
        let text = fs::read_to_string(log.path()).unwrap();
        let (stamp, rest) = text.split_once("] ").unwrap();
        let secs = stamp.strip_prefix('[').unwrap();
        let (whole, millis) = secs.split_once('.').unwrap();
        assert!(whole.parse::<u64>().unwrap() > 1_700_000_000);
        assert_eq!(millis.len(), 3);
        assert_eq!(rest, "warning: slow\n");
        fs::remove_dir_all(dir).unwrap();
    }
}
