//! `launcher.log`: an append-only log with a size cap.
//!
//! From SCR edb692f: when no window can be made (no display, no OpenGL)
//! the launcher plays with saved settings, and warnings must still reach
//! the player somewhere. The file starts afresh once it passes the cap
//! ([`DEFAULT_LIMIT`]; [`Log::with_limit`] changes it, [`Log::without_limit`]
//! removes it), and logging never fails a launch. Levels: [`Log::info`]
//! (log only), [`Log::warn`] and [`Log::error`] (log and stderr), or a
//! plain [`Log::line`].

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// The usual cap: 1 MiB.
pub const DEFAULT_LIMIT: u64 = 1 << 20;

/// A log file and its cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Log {
    path: PathBuf,
    /// `None`: never started afresh.
    limit: Option<u64>,
}

impl Log {
    /// `path` with the [`DEFAULT_LIMIT`].
    pub fn new(path: impl Into<PathBuf>) -> Log {
        Log {
            path: path.into(),
            limit: Some(DEFAULT_LIMIT),
        }
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
        let fresh = self
            .limit
            .is_some_and(|limit| fs::metadata(&self.path).is_ok_and(|m| m.len() > limit));
        if let Ok(mut f) = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(!fresh)
            .truncate(fresh)
            .open(&self.path)
        {
            let _ = writeln!(f, "{line}");
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
}
