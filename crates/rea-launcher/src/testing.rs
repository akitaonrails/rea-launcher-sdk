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
