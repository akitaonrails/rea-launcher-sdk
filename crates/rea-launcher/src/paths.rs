//! Path resolution and the runtime/game overlap check.
//!
//! `resolve` is CMR4's (canonicalise the nearest existing ancestor, so a
//! runtime folder that does not exist yet, is relative, or is reached
//! through `..` or a symlink is still compared by its real place) with the
//! lexical fallback all five adopted after "Incorrect function" on Windows
//! RAM disks and `subst` drives (CMR3 3d63927). `check_separate` adds CMR3's
//! second, lexical comparison as written: when only one side canonicalises,
//! the two spellings differ (`\\?\` prefixes, resolved aliases).
//! `simplify_extended_length` is OR's/CMR4's.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Component, Path, PathBuf};

type Canonicalize<'a> = &'a dyn Fn(&Path) -> io::Result<PathBuf>;

/// The native path of a `/`-separated relative path below `root`.
pub fn native(root: &Path, rel: &str) -> PathBuf {
    rel.split('/')
        .filter(|part| !part.is_empty())
        .fold(root.to_path_buf(), |p, part| p.join(part))
}

/// The file or folder at `/`-separated `rel` below `root`, matching each
/// part without regard to ASCII case (a Windows game's `DINPUT8.DLL` for
/// `dinput8.dll` on a case-sensitive Linux disk), spelled as it is on
/// disk. An exact match wins. On a case-insensitive volume (Windows) the
/// file opens under any spelling, but the folder listing still gives its
/// own, which is what callers report and write back.
pub fn find_ignoring_case(root: &Path, rel: &str) -> Option<PathBuf> {
    let mut path = root.to_path_buf();
    for part in rel.split('/').filter(|p| !p.is_empty()) {
        let names: Vec<OsString> = std::fs::read_dir(&path)
            .ok()?
            .flatten()
            .map(|e| e.file_name())
            .collect();
        let found = names
            .iter()
            .find(|name| name.as_os_str() == part)
            .or_else(|| {
                names
                    .iter()
                    .find(|name| name.to_str().is_some_and(|n| n.eq_ignore_ascii_case(part)))
            });
        match found {
            Some(name) => path.push(name),
            // Not listed under any spelling, yet there (a Windows 8.3 name).
            None if std::fs::symlink_metadata(path.join(part)).is_ok() => path.push(part),
            None => return None,
        }
    }
    Some(path)
}

/// Drops `.` and folds `..` into the component before it, as text.
pub fn normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() && !normalized.has_root() {
                    normalized.push("..");
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn absolute(path: &Path, cwd: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

/// The absolute, real form of `path`, even when its last components do not
/// exist yet: the nearest existing ancestor is canonicalised (symlinks,
/// Windows aliases), the rest appended with `.`/`..` handled as text.
/// Relative paths start at `cwd`. Where the volume cannot canonicalise at
/// all (any error but NotFound), the whole path is normalised as text.
pub fn resolve(path: &Path, cwd: &Path) -> Result<PathBuf, String> {
    resolve_with(path, cwd, &|p| std::fs::canonicalize(p))
}

fn resolve_with(path: &Path, cwd: &Path, canonicalize: Canonicalize) -> Result<PathBuf, String> {
    let absolute = absolute(path, cwd);
    let mut ancestor = absolute.as_path();
    let mut suffix = Vec::new();
    loop {
        match canonicalize(ancestor) {
            Ok(real) => {
                let mut resolved = simplify_extended_length(&real);
                for component in suffix.iter().rev() {
                    resolved.push(component);
                }
                return Ok(normalize(&resolved));
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let Some(component) = ancestor.components().next_back() else {
                    return Err(format!("cannot resolve {}", path.display()));
                };
                match component {
                    Component::Normal(name) => suffix.push(name.to_os_string()),
                    Component::ParentDir => suffix.push("..".into()),
                    Component::CurDir => {}
                    _ => return Ok(normalize(&absolute)),
                }
                let Some(parent) = ancestor.parent() else {
                    return Ok(normalize(&absolute));
                };
                ancestor = parent;
            }
            Err(_) => return Ok(normalize(&absolute)),
        }
    }
}

/// The real form of a folder that must exist (the game folder): relative
/// paths start at `cwd`; canonicalised, `\\?\` dropped; on a volume that
/// cannot canonicalise, normalised as text if it exists. A missing path is
/// a NotFound error (CMR4's `canonical_existing`).
pub fn canonical_existing(path: &Path, cwd: &Path) -> io::Result<PathBuf> {
    canonical_existing_with(path, cwd, &|p| std::fs::canonicalize(p))
}

fn canonical_existing_with(
    path: &Path,
    cwd: &Path,
    canonicalize: Canonicalize,
) -> io::Result<PathBuf> {
    let absolute = absolute(path, cwd);
    match canonicalize(&absolute) {
        Ok(real) => Ok(simplify_extended_length(&real)),
        Err(e) if e.kind() != io::ErrorKind::NotFound && absolute.exists() => {
            Ok(normalize(&absolute))
        }
        Err(e) => Err(io::Error::new(
            e.kind(),
            format!("{}: {e}", absolute.display()),
        )),
    }
}

/// Refuses a folder the launcher writes (`runtime`, or a saves folder)
/// that overlaps the game folder, including one that does not exist yet
/// or reaches it through `..` or a symlink. Nothing is created.
pub fn check_separate(game: &Path, runtime: &Path) -> Result<(), String> {
    let cwd = std::env::current_dir().map_err(|e| format!("current directory: {e}"))?;
    check_separate_with(game, runtime, &cwd, &|p| std::fs::canonicalize(p))
}

fn check_separate_with(
    game: &Path,
    runtime: &Path,
    cwd: &Path,
    canonicalize: Canonicalize,
) -> Result<(), String> {
    let overlap = |a: &Path, b: &Path| contains(a, b) || contains(b, a);
    let g = resolve_with(game, cwd, canonicalize)?;
    let r = resolve_with(runtime, cwd, canonicalize)?;
    let lexical = |p: &Path| normalize(&absolute(p, cwd));
    if overlap(&r, &g) || overlap(&lexical(runtime), &lexical(game)) {
        return Err(format!(
            "{} must be outside the game folder {}",
            runtime.display(),
            game.display()
        ));
    }
    Ok(())
}

/// True when `inner` is `outer` or below it. Windows volumes ignore case,
/// so there the parts compare without it (`C:\Games\CMR3` holds
/// `c:\games\cmr3\rt`); elsewhere they compare exactly.
fn contains(inner: &Path, outer: &Path) -> bool {
    if !cfg!(windows) {
        return inner.starts_with(outer);
    }
    let parts = |p: &Path| -> Vec<String> {
        p.components()
            .map(|c| c.as_os_str().to_string_lossy().to_lowercase())
            .collect()
    };
    parts(inner).starts_with(&parts(outer))
}

#[cfg(windows)]
fn strip_prefix_os(value: &OsStr, prefix: &str) -> Option<OsString> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    let wide: Vec<u16> = value.encode_wide().collect();
    let prefix: Vec<u16> = prefix.encode_utf16().collect();
    wide.starts_with(&prefix)
        .then(|| OsString::from_wide(&wide[prefix.len()..]))
}

#[cfg(unix)]
fn strip_prefix_os(value: &OsStr, prefix: &str) -> Option<OsString> {
    use std::os::unix::ffi::OsStrExt;
    let rest = value.as_bytes().strip_prefix(prefix.as_bytes())?;
    Some(OsStr::from_bytes(rest).to_os_string())
}

#[cfg(not(any(unix, windows)))]
fn strip_prefix_os(value: &OsStr, prefix: &str) -> Option<OsString> {
    value.to_str()?.strip_prefix(prefix).map(OsString::from)
}

/// Turns Win32 extended-length paths (`\\?\C:\…`, `\\?\UNC\server\…`, what
/// `canonicalize` returns on Windows) back into drive and UNC paths, so
/// they compare with paths that were only normalised and old games accept
/// The path to hand an old game that reads it into a fixed-size buffer of
/// `max_chars` characters (its terminating NUL not counted): `long` when it
/// fits, else `short` (Windows' 8.3 name for the same folder, which is also
/// ASCII-only) when that fits; `None` when neither does. A `long` path with
/// non-ASCII characters prefers the short name when there is one, since such
/// games read paths through the ANSI code page. Trailing backslashes are
/// dropped. CMR1 reads `HDPath` into 100 bytes and crashed at `0x49893a`
/// with a longer runtime folder (rea-colin-mcrae#5).
pub fn fit_path(long: &str, short: Option<&str>, max_chars: usize) -> Option<String> {
    let long = long.trim_end_matches('\\');
    let short = short
        .map(|s| s.trim_end_matches('\\'))
        .filter(|s| s.is_ascii() && s.chars().count() <= max_chars);
    let fits = long.chars().count() <= max_chars;
    match (fits, short) {
        (true, Some(short)) if !long.is_ascii() => Some(short.to_owned()),
        (true, _) => Some(long.to_owned()),
        (false, short) => short.map(str::to_owned),
    }
}

/// them. Other paths are returned unchanged.
pub fn simplify_extended_length(path: &Path) -> PathBuf {
    if let Some(rest) = strip_prefix_os(path.as_os_str(), r"\\?\") {
        if let Some(unc) = strip_prefix_os(&rest, r"UNC\") {
            let mut simplified = OsString::from(r"\\");
            simplified.push(unc);
            return PathBuf::from(simplified);
        }
        return PathBuf::from(rest);
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_is_fitted_to_a_games_buffer() {
        let long = r"C:\Users\player\AppData\Local\CMRLauncher\Rally";
        assert_eq!(fit_path(long, None, 99).as_deref(), Some(long));
        assert_eq!(
            fit_path(&format!("{long}\\"), Some(r"C:\X~1"), 99).as_deref(),
            Some(long)
        );
        let at = format!(r"C:\{}", "a".repeat(96));
        assert_eq!(fit_path(&at, None, 99).as_deref(), Some(at.as_str()));
        let too_long = format!(r"C:\{}", "a".repeat(97));
        assert_eq!(
            fit_path(&too_long, Some(r"C:\AAAAAA~1"), 99).as_deref(),
            Some(r"C:\AAAAAA~1")
        );
        assert_eq!(fit_path(&too_long, None, 99), None);
        assert_eq!(fit_path(&too_long, Some(&too_long), 99), None);
        let accented = r"C:\Users\José\Rally";
        assert_eq!(
            fit_path(accented, Some(r"C:\Users\JOS~1\Rally"), 99).as_deref(),
            Some(r"C:\Users\JOS~1\Rally")
        );
        assert_eq!(fit_path(accented, None, 99).as_deref(), Some(accented));
    }
    use crate::test_dir;

    #[test]
    fn native_joins_slash_separated_parts() {
        let p = native(Path::new("root"), "Data/Tracks/a.dat");
        assert_eq!(
            p,
            Path::new("root").join("Data").join("Tracks").join("a.dat")
        );
        assert_eq!(native(Path::new("root"), ""), Path::new("root"));
    }

    #[test]
    fn files_are_found_whatever_their_case() {
        let root = test_dir("paths-case");
        std::fs::create_dir_all(root.join("DATA")).unwrap();
        std::fs::write(root.join("DATA/DInput8.DLL"), b"x").unwrap();
        assert_eq!(
            find_ignoring_case(&root, "data/dinput8.dll"),
            Some(root.join("DATA/DInput8.DLL"))
        );
        assert_eq!(find_ignoring_case(&root, "data/missing.dll"), None);
        // The disk's spelling, even where any spelling opens the file.
        let found = find_ignoring_case(&root, "data/dinput8.dll").unwrap();
        let names: Vec<_> = found.components().rev().take(2).collect();
        assert_eq!(names[0].as_os_str(), "DInput8.DLL");
        assert_eq!(names[1].as_os_str(), "DATA");
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn existing_folders_are_canonicalised_and_missing_ones_refused() {
        let game = test_dir("paths-existing");
        let real = canonical_existing(Path::new("."), &game).unwrap();
        assert_eq!(
            real,
            simplify_extended_length(&std::fs::canonicalize(&game).unwrap())
        );
        let err = canonical_existing(Path::new("missing"), &game).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(err.to_string().contains("missing"), "{err}");
        // A volume that cannot canonicalise: the existing path, as text.
        let unsupported =
            |_: &Path| -> io::Result<PathBuf> { Err(io::Error::other("Incorrect function")) };
        std::fs::create_dir_all(game.join("sub")).unwrap();
        let lexical = canonical_existing_with(Path::new("./sub/.."), &game, &unsupported).unwrap();
        assert_eq!(lexical, normalize(&game));
        assert!(canonical_existing_with(Path::new("gone"), &game, &unsupported).is_err());
        std::fs::remove_dir_all(&game).unwrap();
    }

    #[test]
    fn a_save_folder_is_found_in_any_spelling() {
        // OR: the game writes `SaveGame`, a Linux copy may hold `SAVEGAME`.
        let rt = test_dir("paths-savegame");
        std::fs::create_dir_all(rt.join("SAVEGAME")).unwrap();
        let found = find_ignoring_case(&rt, "SaveGame").unwrap();
        assert_eq!(found.file_name().unwrap(), "SAVEGAME");
        std::fs::remove_dir_all(&rt).unwrap();
    }

    #[test]
    fn overlapping_folders_are_refused() {
        let game = test_dir("paths-overlap");
        assert!(check_separate(&game, &game.join("rt")).is_err());
        assert!(check_separate(&game.join("inner"), &game).is_err());
        assert!(check_separate(&game, &test_dir("paths-elsewhere")).is_ok());
        std::fs::remove_dir_all(&game).unwrap();
    }

    #[test]
    fn missing_relative_and_dotdot_runtimes_are_resolved() {
        let canon = |p: &Path| std::fs::canonicalize(p);
        let game = test_dir("paths-relative");
        let outside = resolve(Path::new("../outside-runtime"), &game).unwrap();
        // Windows canonicalises to `\\?\C:\…`, which resolve simplifies.
        let parent = std::fs::canonicalize(game.parent().unwrap()).unwrap();
        assert_eq!(
            outside,
            simplify_extended_length(&parent).join("outside-runtime")
        );
        assert!(check_separate_with(&game, Path::new("child"), &game, &canon).is_err());
        assert!(check_separate_with(&game, Path::new("missing/../child"), &game, &canon).is_err());
        assert!(check_separate_with(&game, Path::new("../outside"), &game, &canon).is_ok());
        assert!(!game.join("child").exists(), "nothing is created");
        std::fs::remove_dir_all(&game).unwrap();
    }

    #[test]
    fn volumes_that_cannot_canonicalize_are_compared_lexically() {
        let unsupported =
            |_: &Path| -> io::Result<PathBuf> { Err(io::Error::other("Incorrect function")) };
        let cwd = Path::new("/r/games");
        assert_eq!(
            resolve_with(Path::new("cmr3/./x/../rt"), cwd, &unsupported).unwrap(),
            PathBuf::from("/r/games/cmr3/rt")
        );
        let game = Path::new("/r/games/cmr3");
        assert!(check_separate_with(game, Path::new("cmr3/rt"), cwd, &unsupported).is_err());
        assert!(check_separate_with(game, Path::new("../rt"), cwd, &unsupported).is_ok());
        // Only the game canonicalises (to another spelling): still refused.
        let game_only = move |p: &Path| -> io::Result<PathBuf> {
            if p == game {
                Ok(PathBuf::from("/canonical/cmr3"))
            } else {
                Err(io::Error::other("Incorrect function"))
            }
        };
        assert!(check_separate_with(game, Path::new("/r/games/cmr3/rt"), cwd, &game_only).is_err());
    }

    #[test]
    fn overlap_ignores_case_only_on_windows() {
        let unsupported =
            |_: &Path| -> io::Result<PathBuf> { Err(io::Error::other("Incorrect function")) };
        let cwd = Path::new("/r");
        let refused = check_separate_with(
            Path::new("/r/Games/CMR3"),
            Path::new("/r/games/cmr3/rt"),
            cwd,
            &unsupported,
        )
        .is_err();
        assert_eq!(refused, cfg!(windows));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_parent_is_resolved_before_the_check() {
        let game = test_dir("paths-alias-game");
        let alias = test_dir("paths-alias-link");
        std::fs::remove_dir(&alias).unwrap();
        std::os::unix::fs::symlink(&game, &alias).unwrap();
        assert!(check_separate(&game, &alias.join("new-runtime")).is_err());
        std::fs::remove_file(&alias).unwrap();
        std::fs::remove_dir_all(&game).unwrap();
    }

    #[test]
    fn extended_length_paths_become_drive_and_unc_paths() {
        assert_eq!(
            simplify_extended_length(Path::new(r"\\?\C:\Games\OutRun")),
            PathBuf::from(r"C:\Games\OutRun")
        );
        assert_eq!(
            simplify_extended_length(Path::new(r"\\?\UNC\server\share\OutRun")),
            PathBuf::from(r"\\server\share\OutRun")
        );
        assert_eq!(
            simplify_extended_length(Path::new(r"C:\Games")),
            PathBuf::from(r"C:\Games")
        );
    }
}
