//! One launcher at a time for a launcher run directly on Linux
//! (docs/STANDARDS.md, "Clean exit, single instance"): an advisory file
//! lock (`flock`) held for the launcher's lifetime. The play helper's
//! session lock only guards starts that go through it; DiRT Rally's native
//! launcher started straight from a menu had no guard at all.
//!
//! The same shape as `rea_launcher_win32::instance::single` (a named mutex
//! on Windows): `Ok(Guard)` to keep for the launcher's lifetime, or
//! `Err(AlreadyRunning)`. A native window can't be brought forward from
//! here, so the second start says the launcher (or its game) is running and
//! exits; an action flag exits non-zero, as on Windows.
//!
//! ```no_run
//! use rea_launcher::instance;
//! # let action = false;
//! let _instance = match instance::single("rea-dirt-rally") {
//!     Ok(guard) => guard,
//!     Err(running) => {
//!         eprintln!("DiRT Rally: {running}");
//!         std::process::exit(if action { 3 } else { 0 });
//!     }
//! };
//! // ... the launcher, waiting for the game; the guard drops at exit
//! ```
//!
//! The kernel releases the lock when the process ends, however it ends:
//! a killed launcher never leaves a stale lock. The lock file stays (removing
//! it would race a starting launcher). The file is opened close-on-exec, so
//! the game does not inherit it: keep the guard while the launcher waits for
//! the game.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};

/// Held while this launcher runs; dropping it lets the next one start.
#[derive(Debug)]
#[must_use = "the instance is released when this is dropped"]
pub struct Guard {
    file: Option<File>,
    path: PathBuf,
}

impl Guard {
    /// The lock file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// False when no lock could be taken at all (no writable folder) and
    /// the launcher runs unguarded rather than refusing to start.
    pub fn is_held(&self) -> bool {
        self.file.is_some()
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

/// Where [`single`] keeps the lock for `name`: `$XDG_RUNTIME_DIR` (per
/// user, cleared at logout), else the temp folder with the user's name in
/// the file name. Characters a file name can't hold become `_`.
pub fn lock_path(name: &str) -> PathBuf {
    let clean = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if c == '/' || c == '\\' || c.is_control() {
                    '_'
                } else {
                    c
                }
            })
            .collect()
    };
    let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute() && dir.is_dir());
    match runtime_dir {
        Some(dir) => dir.join(format!("rea-launcher-{}.lock", clean(name))),
        None => {
            let user = std::env::var("USER").unwrap_or_default();
            std::env::temp_dir().join(format!(
                "rea-launcher-{}-{}.lock",
                clean(&user),
                clean(name)
            ))
        }
    }
}

/// Takes the instance `name` (lock file at [`lock_path`]), or reports that
/// another launcher holds it. When the lock can't be taken at all (no
/// writable folder, a file system without locks) the launcher runs
/// unguarded rather than refusing to start.
pub fn single(name: &str) -> Result<Guard, AlreadyRunning> {
    single_at(&lock_path(name))
}

/// [`single`] with an explicit lock file.
pub fn single_at(path: &Path) -> Result<Guard, AlreadyRunning> {
    match take(path) {
        Ok(file) => Ok(Guard {
            file: Some(file),
            path: path.to_owned(),
        }),
        Err(Taken::ByOther) => Err(AlreadyRunning),
        Err(Taken::Unavailable) => Ok(Guard {
            file: None,
            path: path.to_owned(),
        }),
    }
}

enum Taken {
    ByOther,
    Unavailable,
}

fn take(path: &Path) -> Result<File, Taken> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|_| Taken::Unavailable)?;
    }
    // Never truncated: the file holds nothing, and a second start must not
    // change what the first one opened.
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|_| Taken::Unavailable)?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(fs::TryLockError::WouldBlock) => Err(Taken::ByOther),
        Err(fs::TryLockError::Error(_)) => Err(Taken::Unavailable),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_dir;

    #[test]
    fn a_second_start_is_refused_until_the_first_ends() {
        let dir = test_dir("instance");
        let path = dir.join("rea-launcher-x.lock");
        let first = single_at(&path).unwrap();
        assert!(first.is_held());
        assert_eq!(first.path(), path);
        let second = single_at(&path).unwrap_err();
        assert_eq!(second, AlreadyRunning);
        assert_eq!(second.to_string(), "the launcher is already running");
        drop(first);
        let third = single_at(&path).unwrap();
        assert!(third.is_held());
        assert!(path.is_file(), "the lock file stays");
    }

    #[test]
    fn a_lock_left_by_a_killed_launcher_is_no_lock() {
        // A leftover file without a holder (the process ended).
        let dir = test_dir("instance-stale");
        let path = dir.join("left.lock");
        fs::write(&path, "4242\n").unwrap();
        assert!(single_at(&path).unwrap().is_held());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "4242\n",
            "never truncated"
        );
    }

    #[test]
    fn no_writable_folder_runs_unguarded() {
        let dir = test_dir("instance-nowhere");
        let blocker = dir.join("file");
        fs::write(&blocker, "").unwrap();
        // A folder below a file can't exist.
        let guard = single_at(&blocker.join("sub/x.lock")).unwrap();
        assert!(!guard.is_held());
    }

    #[test]
    fn lock_names_are_clean() {
        let path = lock_path("a/b\\c\nd");
        let name = path.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("rea-launcher-"), "{name}");
        assert!(name.ends_with("a_b_c_d.lock"), "{name}");
    }
}
