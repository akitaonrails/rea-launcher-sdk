//! Portable core of the rea launchers: pure Rust and SHA-256, no GUI, no
//! Win32 calls, every module tested on the host. Game knowledge (which
//! files are static or game-written, edition tables, patch sites, ini
//! keys) comes in as data or trait implementations. See the README for a
//! module list with examples.

#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

pub mod args;
pub mod cfg;
pub mod display;
pub mod download;
pub mod env;
pub mod fsx;
pub mod identify;
pub mod ini;
pub mod installer;
pub mod instance;
pub mod ledger;
pub mod log;
pub mod pad;
pub mod patch;
pub mod paths;
pub mod pe;
pub mod runtime;
pub mod saves;
pub mod testing;
pub mod wheel;
pub mod wine;

/// A fresh, empty folder for one test.
#[cfg(test)]
pub(crate) fn test_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("rea-launcher-test-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
