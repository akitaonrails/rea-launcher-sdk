//! Golden snapshots for launcher tests (docs/STANDARDS.md, "Golden
//! snapshots"), with the mistakes every launcher made once built out.
//!
//! A help text that printed the crate version went into `help.txt`
//! verbatim; the release commit bumped the version and the tag's own CI
//! failed (CMR3, CMR4, OR, nearly DiRT). Here the caller's version is
//! always replaced by `<version>` before comparing or writing, and a stored
//! snapshot that still holds the version is refused, so that can't happen
//! again. Line endings are compared as `\n` (a Windows checkout with
//! `core.autocrlf` turns them into CRLF).
//!
//! ```no_run
//! // tests/snapshots.rs of a launcher crate
//! # fn usage() -> String { String::new() }
//! #[test]
//! fn help() {
//!     // Compares with tests/snapshots/help.txt; `<version>` stands for
//!     // this crate's CARGO_PKG_VERSION.
//!     rea_launcher::snapshot!("help.txt", usage());
//! }
//! ```
//!
//! `REA_UPDATE_SNAPSHOTS=1 cargo test` writes the snapshots instead of
//! comparing them; review the diff like code. A launcher that documented
//! its own variable keeps it with [`Snapshots::update_var`]:
//!
//! ```no_run
//! # fn template() -> String { String::new() }
//! let snapshots = rea_launcher::snapshots!().update_var("CMR3_UPDATE_SNAPSHOTS");
//! snapshots.assert("template.cfg", &template());
//! ```

use std::fmt;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// The environment variable that makes [`Snapshots`] write instead of
/// compare (any non-empty value but `0`).
pub const UPDATE_VAR: &str = "REA_UPDATE_SNAPSHOTS";

/// What the caller's version becomes in a snapshot.
pub const VERSION_MARK: &str = "<version>";

/// [`Snapshots`] for the calling crate: `tests/snapshots/` in its
/// manifest folder, and its `CARGO_PKG_VERSION`.
#[macro_export]
macro_rules! snapshots {
    () => {
        $crate::testing::Snapshots::new(
            ::std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots"),
            env!("CARGO_PKG_VERSION"),
        )
    };
}

/// Compares `actual` with the calling crate's `tests/snapshots/<name>`
/// ([`snapshots!`]), panicking with the difference.
#[macro_export]
macro_rules! snapshot {
    ($name:expr, $actual:expr $(,)?) => {
        $crate::snapshots!().assert($name, ::core::convert::AsRef::<str>::as_ref(&$actual))
    };
}

/// A folder of golden snapshots and the version they must never embed.
#[derive(Debug, Clone)]
pub struct Snapshots {
    dir: PathBuf,
    version: String,
    update_vars: Vec<String>,
}

/// What [`Snapshots::check`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The snapshot matched.
    Matched,
    /// Update mode: the snapshot was written (or already held this text).
    Written,
}

/// Why a snapshot check failed.
#[derive(Debug)]
pub enum SnapshotError {
    /// A name that is empty, absolute or climbs out of the folder.
    BadName(String),
    /// No snapshot file yet.
    Missing {
        path: PathBuf,
        update_var: String,
    },
    /// The stored snapshot holds the crate version: it breaks at the next
    /// version bump. `line` is 1-based.
    EmbedsVersion {
        path: PathBuf,
        version: String,
        line: usize,
    },
    /// The text differs. `line` is the first differing line (1-based).
    Changed {
        path: PathBuf,
        line: usize,
        expected: String,
        actual: String,
        only_versions: bool,
        update_var: String,
    },
    Io {
        path: PathBuf,
        error: io::Error,
    },
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotError::BadName(name) => write!(f, "bad snapshot name {name:?}"),
            SnapshotError::Missing { path, update_var } => write!(
                f,
                "no snapshot {}: run the test with {update_var}=1 to write it, then review it",
                path.display()
            ),
            SnapshotError::EmbedsVersion {
                path,
                version,
                line,
            } => write!(
                f,
                "snapshot {}:{line} holds the crate version {version}: it would fail at the \
                 next version bump (the tag's CI). Write {VERSION_MARK} there (the helper \
                 replaces the version with it), or regenerate the snapshot",
                path.display()
            ),
            SnapshotError::Changed {
                path,
                line,
                expected,
                actual,
                only_versions,
                update_var,
            } => {
                write!(
                    f,
                    "snapshot {} changed at line {line}:\n  expected: {expected:?}\n  actual:   {actual:?}\n\
                     Review it like code; if the change is intended, run with {update_var}=1",
                    path.display()
                )?;
                if *only_versions {
                    write!(
                        f,
                        "\n  (only version numbers differ: is a component version or a \
                         date in the output? keep varying values out of snapshots)"
                    )?;
                }
                Ok(())
            }
            SnapshotError::Io { path, error } => write!(f, "{}: {error}", path.display()),
        }
    }
}

impl std::error::Error for SnapshotError {}

impl Snapshots {
    /// Snapshots in `dir`; `version` (the crate's `CARGO_PKG_VERSION`) is
    /// written as [`VERSION_MARK`]. [`snapshots!`] fills both in.
    pub fn new(dir: impl Into<PathBuf>, version: &str) -> Snapshots {
        Snapshots {
            dir: dir.into(),
            version: version.to_owned(),
            update_vars: vec![UPDATE_VAR.to_owned()],
        }
    }

    /// Also update when `var` is set (a launcher's own documented name,
    /// e.g. `CMR3_UPDATE_SNAPSHOTS`). [`UPDATE_VAR`] always works.
    pub fn update_var(mut self, var: &str) -> Snapshots {
        self.update_vars.push(var.to_owned());
        self
    }

    /// The snapshot folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The text as stored: `\n` line endings, the version as
    /// [`VERSION_MARK`].
    pub fn normalize(&self, text: &str) -> String {
        replace_version(&text.replace("\r\n", "\n"), &self.version, VERSION_MARK)
    }

    fn updating(&self) -> bool {
        self.update_vars
            .iter()
            .any(|var| std::env::var_os(var).is_some_and(|value| !value.is_empty() && value != "0"))
    }

    fn path(&self, name: &str) -> Result<PathBuf, SnapshotError> {
        let rel = Path::new(name);
        let clean = !name.is_empty() && rel.components().all(|c| matches!(c, Component::Normal(_)));
        if clean {
            Ok(self.dir.join(rel))
        } else {
            Err(SnapshotError::BadName(name.to_owned()))
        }
    }

    /// Compares `actual` (normalized) with the stored snapshot `name`, or
    /// writes it in update mode ([`UPDATE_VAR`]). A stored snapshot that
    /// holds the version is refused either way.
    pub fn check(&self, name: &str, actual: &str) -> Result<Outcome, SnapshotError> {
        let path = self.path(name)?;
        let actual = self.normalize(actual);
        if self.updating() {
            if fs::read_to_string(&path).ok().as_deref() != Some(actual.as_str()) {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent).map_err(|error| SnapshotError::Io {
                        path: parent.to_owned(),
                        error,
                    })?;
                }
                fs::write(&path, &actual).map_err(|error| SnapshotError::Io {
                    path: path.clone(),
                    error,
                })?;
            }
            return Ok(Outcome::Written);
        }
        let stored = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(SnapshotError::Missing {
                    path,
                    update_var: UPDATE_VAR.to_owned(),
                });
            }
            Err(error) => return Err(SnapshotError::Io { path, error }),
        };
        let expected = stored.replace("\r\n", "\n");
        if let Some(line) = version_line(&expected, &self.version) {
            return Err(SnapshotError::EmbedsVersion {
                path,
                version: self.version.clone(),
                line,
            });
        }
        if expected == actual {
            return Ok(Outcome::Matched);
        }
        let (line, want, got) = first_difference(&expected, &actual);
        Err(SnapshotError::Changed {
            path,
            line,
            expected: want,
            actual: got,
            only_versions: mask_versions(&expected) == mask_versions(&actual),
            update_var: UPDATE_VAR.to_owned(),
        })
    }

    /// [`Snapshots::check`], panicking with the reason (for `#[test]`s).
    #[track_caller]
    pub fn assert(&self, name: &str, actual: &str) {
        if let Err(error) = self.check(name, actual) {
            panic!("{error}");
        }
    }
}

/// Whether `text[at..]` starts a whole version token: not inside a longer
/// number or dotted version (`10.3.3`, `0.3.30`, `0.3.3.1`).
fn version_at(text: &str, at: usize, version: &str) -> bool {
    if version.is_empty() || !text[at..].starts_with(version) {
        return false;
    }
    let before = text[..at].chars().next_back();
    let rest = &text[at + version.len()..];
    let mut after = rest.chars();
    let next = after.next();
    let digit_or_dot = |c: Option<char>| c.is_some_and(|c| c.is_ascii_digit() || c == '.');
    let joined_after = match next {
        Some(c) if c.is_ascii_digit() => true,
        Some('.') => after.next().is_some_and(|c| c.is_ascii_digit()),
        _ => false,
    };
    !digit_or_dot(before) && !joined_after
}

fn replace_version(text: &str, version: &str, mark: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < text.len() {
        if version_at(text, i, version) {
            out.push_str(mark);
            i += version.len();
        } else {
            let c = text[i..].chars().next().expect("i is a char boundary");
            out.push(c);
            i += c.len_utf8();
        }
    }
    out
}

fn version_line(text: &str, version: &str) -> Option<usize> {
    text.lines()
        .position(|line| {
            (0..line.len()).any(|i| line.is_char_boundary(i) && version_at(line, i, version))
        })
        .map(|i| i + 1)
}

fn first_difference(expected: &str, actual: &str) -> (usize, String, String) {
    let (mut want, mut got) = (expected.split('\n'), actual.split('\n'));
    let mut line = 1;
    loop {
        match (want.next(), got.next()) {
            (Some(a), Some(b)) if a == b => line += 1,
            (a, b) => {
                let show =
                    |s: Option<&str>| s.map_or_else(|| "<end of file>".to_owned(), str::to_owned);
                return (line, show(a), show(b));
            }
        }
    }
}

/// Every `N.N.N…` token as `#`, to tell a difference in versions only.
fn mask_versions(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].is_ascii_digit() {
            let start = i;
            let mut dots = 0;
            while i < chars.len()
                && (chars[i].is_ascii_digit()
                    || (chars[i] == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit)))
            {
                dots += usize::from(chars[i] == '.');
                i += 1;
            }
            if dots >= 2 {
                out.push('#');
            } else {
                out.extend(&chars[start..i]);
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_dir;

    fn snaps(name: &str) -> Snapshots {
        // Never in update mode, whatever the developer's environment says:
        // the update variables are names nobody sets.
        let mut s = Snapshots::new(test_dir(&format!("snap-{name}")), "0.3.3");
        s.update_vars = vec![format!("REA_TEST_NEVER_SET_{}", std::process::id())];
        s
    }

    fn updating(s: &Snapshots) -> Snapshots {
        let mut s = s.clone();
        // PATH is always set and non-empty: update mode without touching
        // the process environment (tests run in parallel).
        s.update_vars = vec!["PATH".to_owned()];
        s
    }

    #[test]
    fn the_version_becomes_a_mark_only_as_a_whole_token() {
        let s = snaps("version-token");
        assert_eq!(
            s.normalize("cmr3-launcher 0.3.3 (v0.3.3).\n"),
            "cmr3-launcher <version> (v<version>).\n"
        );
        for kept in ["10.3.3", "0.3.30", "0.3.3.1", "1.0.3.3"] {
            assert_eq!(s.normalize(kept), kept, "{kept}");
        }
        assert_eq!(s.normalize("a\r\nb\r\n"), "a\nb\n");
        assert_eq!(s.normalize("Ação 0.3.3"), "Ação <version>");
    }

    #[test]
    fn a_help_snapshot_survives_the_release_bump() {
        // Written at 0.3.3, checked after the release commit made it 0.3.4.
        let before = snaps("bump");
        updating(&before)
            .check("help.txt", "launcher 0.3.3\n")
            .unwrap();
        assert_eq!(
            fs::read_to_string(before.dir().join("help.txt")).unwrap(),
            "launcher <version>\n"
        );
        let mut after = before.clone();
        after.version = "0.3.4".to_owned();
        assert_eq!(
            after.check("help.txt", "launcher 0.3.4\n").unwrap(),
            Outcome::Matched
        );
    }

    #[test]
    fn a_stored_snapshot_with_the_version_is_refused() {
        let s = snaps("embeds");
        fs::write(s.dir().join("help.txt"), "usage\nlauncher 0.3.3\n").unwrap();
        let err = s.check("help.txt", "usage\nlauncher 0.3.3\n").unwrap_err();
        assert!(
            matches!(err, SnapshotError::EmbedsVersion { line: 2, .. }),
            "{err}"
        );
        assert!(err.to_string().contains("next version bump"));
    }

    #[test]
    fn crlf_checkouts_match() {
        let s = snaps("crlf");
        fs::write(s.dir().join("t.cfg"), "a = 1\r\nb = 2\r\n").unwrap();
        assert_eq!(
            s.check("t.cfg", "a = 1\nb = 2\n").unwrap(),
            Outcome::Matched
        );
        assert_eq!(
            s.check("t.cfg", "a = 1\r\nb = 2\r\n").unwrap(),
            Outcome::Matched
        );
    }

    #[test]
    fn a_difference_names_the_first_line_and_how_to_update() {
        let s = snaps("changed");
        fs::write(s.dir().join("plan.txt"), "same\nold\n").unwrap();
        let err = s.check("plan.txt", "same\nnew\n").unwrap_err();
        let SnapshotError::Changed {
            line,
            expected,
            actual,
            only_versions,
            ..
        } = &err
        else {
            panic!("{err}")
        };
        assert_eq!(
            (*line, expected.as_str(), actual.as_str()),
            (2, "old", "new")
        );
        assert!(!only_versions);
        assert!(err.to_string().contains("REA_UPDATE_SNAPSHOTS=1"));

        let err = s.check("plan.txt", "same\nold\nmore\n").unwrap_err();
        assert!(
            matches!(&err, SnapshotError::Changed { line: 3, expected, .. } if expected.is_empty()),
            "{err}"
        );

        fs::write(s.dir().join("dxvk.txt"), "dxvk 2.6.1\n").unwrap();
        let err = s.check("dxvk.txt", "dxvk 2.7.0\n").unwrap_err();
        assert!(
            matches!(
                err,
                SnapshotError::Changed {
                    only_versions: true,
                    ..
                }
            ),
            "{err}"
        );
    }

    #[test]
    fn missing_snapshots_say_how_to_write_them() {
        let s = snaps("missing");
        let err = s.check("none.txt", "x").unwrap_err();
        assert!(matches!(err, SnapshotError::Missing { .. }));
        assert!(err.to_string().contains("REA_UPDATE_SNAPSHOTS=1"));
    }

    #[test]
    fn update_mode_writes_subfolders_and_is_idempotent() {
        let s = updating(&snaps("update"));
        assert_eq!(
            s.check("looks/original.txt", "x\r\n").unwrap(),
            Outcome::Written
        );
        assert_eq!(
            s.check("looks/original.txt", "x\n").unwrap(),
            Outcome::Written
        );
        assert_eq!(
            fs::read_to_string(s.dir().join("looks/original.txt")).unwrap(),
            "x\n"
        );
    }

    #[test]
    fn names_stay_inside_the_folder() {
        let s = snaps("names");
        for bad in ["", "../x", "/etc/passwd", "a/../../b", "./a"] {
            assert!(
                matches!(s.check(bad, ""), Err(SnapshotError::BadName(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn update_vars_need_a_real_value() {
        let mut s = snaps("vars");
        assert!(!s.updating());
        s.update_vars.push("PATH".to_owned());
        assert!(s.updating());
        let custom = Snapshots::new(s.dir(), "1.0.0").update_var("CMR3_UPDATE_SNAPSHOTS");
        assert_eq!(custom.update_vars, [UPDATE_VAR, "CMR3_UPDATE_SNAPSHOTS"]);
    }

    #[test]
    fn the_macros_use_the_calling_crate() {
        let s = crate::snapshots!();
        assert!(s.dir().ends_with("tests/snapshots"));
        assert_eq!(s.normalize(env!("CARGO_PKG_VERSION")), VERSION_MARK);
    }
}

/// The folder that holds this crate's built examples (`<profile>/examples`),
/// found from the running test binary.
///
/// Cargo 1.100 moved test binaries out of `<profile>/deps` into
/// `<profile>/build/<package>/<hash>/`, so "two folders up from the test"
/// no longer reaches the profile folder (the SDK's Wine tests broke on the
/// day GitHub's runners took 1.100). This walks up from the test binary to
/// the first folder whose `examples/<name>` exists, and so works with both
/// layouts.
///
/// ```no_run
/// let probe = rea_launcher::testing::example("dll_probe.exe");
/// let examples = probe.parent().unwrap();
/// ```
///
/// # Panics
///
/// When no such file is found: build the examples first (`cargo test`
/// builds a package's examples with its tests).
pub fn example(name: &str) -> std::path::PathBuf {
    let exe = std::env::current_exe().expect("the test binary's path");
    find_example(&exe, name).unwrap_or_else(|| {
        panic!(
            "no examples/{name} above {}: build the examples first",
            exe.display()
        )
    })
}

fn find_example(exe: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    exe.ancestors()
        .skip(1)
        .map(|dir| dir.join("examples").join(name))
        .find(|path| path.is_file())
}

/// A file this build produced (the crate's own `cdylib`, say), found from
/// the running test binary: the newest of `<dir>/<name>`, `<dir>/deps/<name>`
/// and, for Cargo 1.100's layout, `<dir>/build/*/*/out/<name>`, for every
/// folder above the test.
///
/// Cargo 1.100 no longer copies a package's own DLL next to its tests;
/// each build unit keeps its output in `build/<package>/<hash>/out/`. The
/// newest copy is the one this `cargo test` just built (an older layout's
/// copy can still lie in `deps/`).
///
/// # Panics
///
/// When no copy is found.
pub fn built(name: &str) -> std::path::PathBuf {
    let exe = std::env::current_exe().expect("the test binary's path");
    find_built(&exe, name)
        .unwrap_or_else(|| panic!("{name} was not built (looked above {})", exe.display()))
}

fn find_built(exe: &std::path::Path, name: &str) -> Option<std::path::PathBuf> {
    let mut found = Vec::new();
    for dir in exe.ancestors().skip(1) {
        found.push(dir.join(name));
        found.push(dir.join("deps").join(name));
        let units = std::fs::read_dir(dir.join("build"))
            .into_iter()
            .flatten()
            .flatten();
        for package in units {
            let hashes = std::fs::read_dir(package.path())
                .into_iter()
                .flatten()
                .flatten();
            found.extend(hashes.map(|hash| hash.path().join("out").join(name)));
        }
    }
    found
        .into_iter()
        .filter_map(|path| Some((std::fs::metadata(&path).ok()?.modified().ok()?, path)))
        .max_by_key(|(modified, _)| *modified)
        .map(|(_, path)| path)
}

#[cfg(test)]
mod example_tests {
    use super::{find_built, find_example};

    #[test]
    fn examples_are_found_from_either_cargo_layout() {
        let root = crate::test_dir("find-example");
        let profile = root.join("i686-pc-windows-gnu").join("debug");
        std::fs::create_dir_all(profile.join("examples")).unwrap();
        std::fs::write(profile.join("examples").join("probe.exe"), b"MZ").unwrap();
        let want = profile.join("examples").join("probe.exe");
        // Cargo up to 1.99: <profile>/deps/<test>.exe
        let old = profile.join("deps").join("tests-1234.exe");
        assert_eq!(find_example(&old, "probe.exe"), Some(want.clone()));
        // Cargo 1.100: <profile>/build/<package>/<hash>/<test>.exe
        let new = profile
            .join("build")
            .join("pkg")
            .join("29f2")
            .join("deps")
            .join("tests.exe");
        assert_eq!(find_example(&new, "probe.exe"), Some(want));
        assert_eq!(find_example(&old, "missing.exe"), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_newest_build_output_wins_in_either_layout() {
        let root = crate::test_dir("find-built");
        let profile = root.join("debug");
        let stale = profile.join("deps").join("hook.dll");
        let fresh_dir = profile.join("build").join("hook").join("0071").join("out");
        std::fs::create_dir_all(stale.parent().unwrap()).unwrap();
        std::fs::create_dir_all(&fresh_dir).unwrap();
        std::fs::write(&stale, b"old").unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        std::fs::File::options()
            .write(true)
            .open(&stale)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let exe = profile
            .join("build")
            .join("hook")
            .join("557e")
            .join("out")
            .join("t.exe");
        assert_eq!(
            find_built(&exe, "hook.dll"),
            Some(stale.clone()),
            "only the old layout"
        );
        std::fs::write(fresh_dir.join("hook.dll"), b"new").unwrap();
        assert_eq!(
            find_built(&exe, "hook.dll"),
            Some(fresh_dir.join("hook.dll"))
        );
        assert_eq!(find_built(&exe, "none.dll"), None);
        std::fs::remove_dir_all(root).unwrap();
    }
}
