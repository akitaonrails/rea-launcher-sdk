//! Whole-file swaps that never write through a hard link.
//!
//! From CMR3 `runtime.rs` (`put_in_place`, 265b9ca/bc654e6/3dda08b/8f459a2):
//! the runtime folder hard-links the player's originals, so writing into an
//! existing runtime file, or changing its time or attributes, would change
//! the player's file through the link. Every write here builds the new
//! content at a temp name beside the target and renames it over, so the old
//! file stays whole until the new one is complete. On Windows an antivirus
//! or OneDrive briefly holds a fresh file, so a refused rename is retried
//! (5 × 100 ms, CMR4 2d072a9 / OR 68d81e6) before the read-only fallback.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Suffix of the temp file a replacement is built in (`.name.rea-new`).
pub const TEMP_SUFFIX: &str = ".rea-new";

const RENAME_TRIES: u32 = if cfg!(windows) { 5 } else { 1 };
const RENAME_WAIT: Duration = Duration::from_millis(100);

/// Where a replacement for `path` is prepared: beside it (same volume, so
/// the final rename is one step), hidden, with [`TEMP_SUFFIX`].
pub fn temp_beside(path: &Path) -> PathBuf {
    let mut name = OsString::from(".");
    name.push(path.file_name().unwrap_or_default());
    name.push(TEMP_SUFFIX);
    path.with_file_name(name)
}

/// Where [`put_in_place`] keeps the old file while a refused replacement
/// is retried (`.name.rea-old`).
pub const BACKUP_SUFFIX: &str = ".rea-old";

fn backup_beside(path: &Path) -> PathBuf {
    let mut name = OsString::from(".");
    name.push(path.file_name().unwrap_or_default());
    name.push(BACKUP_SUFFIX);
    path.with_file_name(name)
}

/// `e` with `path` in its message (same kind).
pub(crate) fn at(path: &Path) -> impl FnOnce(io::Error) -> io::Error + '_ {
    move |e| io::Error::new(e.kind(), format!("{}: {e}", path.display()))
}

/// Builds `path`'s new content with `make` at [`temp_beside`], then renames
/// it over `path`. Parent folders are created. Nothing is ever written
/// through `path`, which may be a hard link to the player's original.
///
/// When the rename keeps being refused (Windows will not replace a
/// read-only file), the old file is moved aside to a backup name, the new
/// one renamed in, and the backup removed only after that succeeded; on
/// failure the old file is moved back and the temp file, holding the new
/// content, is left for the next attempt. Errors name the path.
pub fn put_in_place(path: &Path, make: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir).map_err(at(dir))?;
    }
    let backup = backup_beside(path);
    if !exists(path) && exists(&backup) {
        // An earlier swap failed after moving the old file aside.
        let _ = fs::rename(&backup, path);
    }
    let tmp = temp_beside(path);
    let _ = fs::remove_file(&tmp);
    if let Err(e) = make(&tmp) {
        let _ = fs::remove_file(&tmp);
        return Err(at(path)(e));
    }
    swap_in(
        &tmp,
        path,
        &mut |from, to| fs::rename(from, to),
        RENAME_WAIT,
    )
    .map_err(at(path))?;
    // Gone after a rename, except when `path` already was the same file
    // (renaming a hard link onto itself does nothing).
    let _ = fs::remove_file(&tmp);
    Ok(())
}

/// Renames `tmp` over `path` (see [`put_in_place`]); `rename` is
/// injectable so the Windows fallback is tested on any host.
fn swap_in(
    tmp: &Path,
    path: &Path,
    rename: &mut dyn FnMut(&Path, &Path) -> io::Result<()>,
    wait: Duration,
) -> io::Result<()> {
    match retry_denied(RENAME_TRIES, wait, || rename(tmp, path)) {
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied && exists(path) => {
            let backup = backup_beside(path);
            let _ = fs::remove_file(&backup);
            rename(path, &backup)?;
            match retry_denied(RENAME_TRIES, wait, || rename(tmp, path)) {
                Ok(()) => {
                    // Unlinked only: it may be a hard link to an original.
                    let _ = fs::remove_file(&backup);
                    Ok(())
                }
                Err(e) => {
                    let _ = rename(&backup, path);
                    Err(e)
                }
            }
        }
        other => other,
    }
}

/// Removes (unlinks, never writes) every file below `dir` whose name ends
/// with one of `suffixes` and is longer than it: temp files older
/// launchers left (`.cmr3-new`, `.cmr4-new`, `.srr-tmp`; ours are
/// [`TEMP_SUFFIX`]). A leftover may be a hard link to the player's
/// original, which unlinking leaves alone. Symbolic links to folders are
/// not followed. Returns what was removed.
pub fn remove_stale_temps(dir: &Path, suffixes: &[&str]) -> io::Result<Vec<PathBuf>> {
    stale_temps(dir, suffixes, true)
}

/// [`remove_stale_temps`] for the files directly in `dir` only.
pub fn remove_stale_temps_here(dir: &Path, suffixes: &[&str]) -> io::Result<Vec<PathBuf>> {
    stale_temps(dir, suffixes, false)
}

/// Unlinks each of `paths` that exists (OR's `.or-manifest.tmp`); a
/// folder is left alone. Returns what was removed.
pub fn remove_files(paths: &[&Path]) -> io::Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    for path in paths {
        if fs::symlink_metadata(path).is_ok_and(|m| !m.is_dir()) {
            fs::remove_file(path).map_err(at(path))?;
            removed.push(path.to_path_buf());
        }
    }
    Ok(removed)
}

fn stale_temps(dir: &Path, suffixes: &[&str], deep: bool) -> io::Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    for entry in fs::read_dir(dir).map_err(at(dir))? {
        let path = entry.map_err(at(dir))?.path();
        let meta = fs::symlink_metadata(&path).map_err(at(&path))?;
        if meta.is_dir() {
            if deep {
                removed.extend(stale_temps(&path, suffixes, true)?);
            }
            continue;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy();
        if suffixes
            .iter()
            .any(|s| !s.is_empty() && name.len() > s.len() && name.ends_with(s))
        {
            fs::remove_file(&path).map_err(at(&path))?;
            removed.push(path);
        }
    }
    Ok(removed)
}

/// Runs `op` up to `tries` times while it fails with PermissionDenied,
/// waiting `wait` in between; any other result is returned at once.
pub(crate) fn retry_denied(
    tries: u32,
    wait: Duration,
    mut op: impl FnMut() -> io::Result<()>,
) -> io::Result<()> {
    let mut left = tries.max(1);
    loop {
        left -= 1;
        match op() {
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied && left > 0 => {
                std::thread::sleep(wait);
            }
            other => return other,
        }
    }
}

/// Replaces `path` with `bytes` as a fresh file (never through a link).
pub fn install(path: &Path, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    if holds(path, bytes.as_ref()) {
        return Ok(());
    }
    put_in_place(path, |tmp| fs::write(tmp, bytes.as_ref()))
}

/// Like [`install`], but flushed to disk before the rename: for settings,
/// manifests and ledgers, where a crash must leave the old file or the new
/// one, never a torn one.
pub fn write_atomic(path: &Path, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    if holds(path, bytes.as_ref()) {
        return Ok(());
    }
    put_in_place(path, |tmp| {
        let mut file = fs::File::create(tmp)?;
        io::Write::write_all(&mut file, bytes.as_ref())?;
        file.sync_all()
    })
}

/// Copies `from` over `path` as a fresh file its owner can write (a copy
/// of a read-only original would be read-only too).
pub fn install_copy(from: &Path, path: &Path) -> io::Result<()> {
    // Already a writable copy with these bytes (not a hard link to
    // `from`, which the caller wants replaced by a copy): nothing to do.
    if fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && !m.permissions().readonly())
        && !same_file(from, path)
        && fs::read(from).is_ok_and(|bytes| holds(path, &bytes))
    {
        return Ok(());
    }
    put_in_place(path, |tmp| {
        fs::copy(from, tmp)?;
        make_writable(tmp)
    })
}

/// True when `path` is a regular file (not a link) holding exactly
/// `bytes`: [`install`], [`write_atomic`] and [`install_copy`] then leave
/// it alone — no temp file, no rename (which can fail over a DLL a game
/// has loaded), no flush. Nothing is written either way.
fn holds(path: &Path, bytes: &[u8]) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_file() && m.len() == bytes.len() as u64)
        && fs::read(path).is_ok_and(|current| current == bytes)
}

/// True when `a` and `b` are the same file (hard links included).
fn same_file(a: &Path, b: &Path) -> bool {
    let (Ok(a), Ok(b)) = (fs::metadata(a), fs::metadata(b)) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        a.dev() == b.dev() && a.ino() == b.ino()
    }
    #[cfg(not(unix))]
    {
        // Hard links share one record: size and all three times. A copy
        // gets its own creation time.
        a.len() == b.len()
            && a.created()
                .ok()
                .is_some_and(|t| b.created().ok() == Some(t))
            && a.modified().ok() == b.modified().ok()
            && a.accessed().ok() == b.accessed().ok()
    }
}

/// Removes a file (or a symbolic link, dangling or not) when it exists.
/// The file's attributes are never changed first: it may be a hard link to
/// the player's original, which shares them.
pub fn remove(path: &Path) -> io::Result<()> {
    if exists(path) {
        fs::remove_file(path).map_err(at(path))?;
    }
    Ok(())
}

/// Lets the owner write `path`: only the owner's write bit on Unix
/// (`set_readonly(false)` would make it writable by everyone, CMR3 8f459a2),
/// the read-only attribute cleared on Windows. Only for files the launcher
/// just created: an existing runtime file may be a hard link.
pub fn make_writable(path: &Path) -> io::Result<()> {
    let mut perm = fs::metadata(path)?.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perm.set_mode(perm.mode() | 0o200);
    }
    #[cfg(not(unix))]
    {
        #[allow(clippy::permissions_set_readonly_false)]
        perm.set_readonly(false);
    }
    fs::set_permissions(path, perm)
}

/// True when something (a file, folder or link, even a dangling one) is
/// at `path`.
fn exists(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_dir;

    #[test]
    fn installs_swap_whole_files_and_leave_no_temp() {
        let dir = test_dir("fsx-atomic");
        let ini = dir.join("Settings.ini");
        install(&ini, b"one").unwrap();
        // A temp left by an interrupted run is replaced, not in the way.
        fs::write(temp_beside(&ini), b"stale").unwrap();
        install(&ini, b"two").unwrap();
        assert_eq!(fs::read(&ini).unwrap(), b"two");
        write_atomic(&ini, b"three").unwrap();
        assert_eq!(fs::read(&ini).unwrap(), b"three");
        // A failed copy leaves the old file whole.
        assert!(install_copy(&dir.join("missing"), &ini).is_err());
        assert_eq!(fs::read(&ini).unwrap(), b"three");
        let names: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["Settings.ini"]);
        fs::remove_dir_all(dir).unwrap();
    }

    /// A rename that refuses to replace `locked` (Windows and a read-only
    /// file), and with `held` refuses to move the temp file at all (an
    /// antivirus holding it).
    fn refusing(locked: PathBuf, held: bool) -> impl FnMut(&Path, &Path) -> io::Result<()> {
        move |from: &Path, to: &Path| {
            let from_temp = from.to_string_lossy().ends_with(TEMP_SUFFIX);
            if (held && from_temp) || (to == locked && to.exists()) {
                return Err(io::Error::from(io::ErrorKind::PermissionDenied));
            }
            fs::rename(from, to)
        }
    }

    #[test]
    fn a_refused_replace_moves_the_old_file_aside_and_back() {
        let dir = test_dir("fsx-swap");
        let path = dir.join("Rally_3PC.exe");
        let tmp = temp_beside(&path);
        fs::write(&path, b"old").unwrap();
        fs::write(&tmp, b"new").unwrap();
        let mut rename = refusing(path.clone(), false);
        swap_in(&tmp, &path, &mut rename, Duration::ZERO).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert!(
            !backup_beside(&path).exists(),
            "the backup goes after success"
        );

        // The new file cannot go in either: the old one comes back, and
        // the temp keeps the new content.
        fs::write(&tmp, b"newer").unwrap();
        let mut rename = refusing(path.clone(), true);
        assert!(swap_in(&tmp, &path, &mut rename, Duration::ZERO).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"new");
        assert_eq!(fs::read(&tmp).unwrap(), b"newer");
        assert!(!backup_beside(&path).exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_backup_left_by_a_failed_swap_is_recovered() {
        let dir = test_dir("fsx-recover");
        let path = dir.join("a.ini");
        fs::write(backup_beside(&path), b"old").unwrap();
        // The make fails: nothing replaces it, but the old file is back.
        assert!(put_in_place(&path, |_| Err(io::Error::other("full disk"))).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"old");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn errors_name_the_path() {
        let dir = test_dir("fsx-context");
        let err = install_copy(&dir.join("missing"), &dir.join("x.dll")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
        assert!(err.to_string().contains("x.dll"), "{err}");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stale_temps_are_unlinked() {
        let dir = test_dir("fsx-stale");
        fs::create_dir_all(dir.join("Data")).unwrap();
        fs::write(dir.join(".CMR3.cmr3-new"), b"x").unwrap();
        fs::write(dir.join("Data/.a.dat.rea-new"), b"x").unwrap();
        fs::write(dir.join("keep.dat"), b"x").unwrap();
        fs::write(dir.join(".cmr3-new"), b"exactly the suffix").unwrap();
        let mut removed = remove_stale_temps(&dir, &[".cmr3-new", TEMP_SUFFIX]).unwrap();
        removed.sort();
        assert_eq!(
            removed,
            [dir.join(".CMR3.cmr3-new"), dir.join("Data/.a.dat.rea-new")]
        );
        assert!(dir.join("keep.dat").exists() && dir.join(".cmr3-new").exists());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stale_temps_by_exact_name_or_in_one_folder() {
        let dir = test_dir("fsx-stale-exact");
        fs::create_dir_all(dir.join("SaveGame")).unwrap();
        fs::write(dir.join(".or-manifest.tmp"), b"x").unwrap();
        fs::write(dir.join("a.tmp"), b"x").unwrap();
        fs::write(dir.join("SaveGame/b.tmp"), b"game's own").unwrap();
        let removed = remove_files(&[
            &dir.join(".or-manifest.tmp"),
            &dir.join("missing.tmp"),
            &dir.join("SaveGame"),
        ])
        .unwrap();
        assert_eq!(removed, [dir.join(".or-manifest.tmp")]);
        assert!(dir.join("SaveGame").is_dir(), "folders stay");
        assert_eq!(
            remove_stale_temps_here(&dir, &[".tmp"]).unwrap(),
            [dir.join("a.tmp")]
        );
        assert!(dir.join("SaveGame/b.tmp").exists(), "not below");
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn stale_temps_that_are_links_leave_the_original() {
        let dir = test_dir("fsx-stale-link");
        let game = test_dir("fsx-stale-link-game");
        fs::write(game.join("a.dat"), b"original").unwrap();
        fs::hard_link(game.join("a.dat"), dir.join(".a.dat.cmr3-new")).unwrap();
        std::os::unix::fs::symlink(&game, dir.join("linked")).unwrap();
        fs::write(game.join(".b.cmr3-new"), b"not ours to follow").unwrap();
        remove_stale_temps(&dir, &[".cmr3-new"]).unwrap();
        assert_eq!(fs::read(game.join("a.dat")).unwrap(), b"original");
        assert!(
            game.join(".b.cmr3-new").exists(),
            "symlinked folders are not entered"
        );
        fs::remove_dir_all(dir).unwrap();
        fs::remove_dir_all(game).unwrap();
    }

    #[test]
    fn the_same_bytes_are_not_written_again() {
        let dir = test_dir("fsx-same");
        let dll = dir.join("d3d9.dll");
        install(&dll, b"dxvk").unwrap();
        let when = std::time::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
        fs::File::options()
            .write(true)
            .open(&dll)
            .unwrap()
            .set_modified(when)
            .unwrap();
        install(&dll, b"dxvk").unwrap();
        write_atomic(&dll, b"dxvk").unwrap();
        assert_eq!(
            fs::metadata(&dll).unwrap().modified().unwrap(),
            when,
            "untouched"
        );
        let bundle = dir.join("bundle.dll");
        fs::write(&bundle, b"dxvk").unwrap();
        install_copy(&bundle, &dll).unwrap();
        assert_eq!(fs::metadata(&dll).unwrap().modified().unwrap(), when);
        // Different bytes are still swapped in.
        install(&dll, b"dxvk 2").unwrap();
        assert_eq!(fs::read(&dll).unwrap(), b"dxvk 2");
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_hard_link_to_the_source_is_still_replaced_by_a_copy() {
        use std::os::unix::fs::MetadataExt;
        let dir = test_dir("fsx-same-link");
        let original = dir.join("original.dat");
        fs::write(&original, b"data").unwrap();
        let link = dir.join("link.dat");
        fs::hard_link(&original, &link).unwrap();
        install_copy(&original, &link).unwrap();
        let ino = |p: &Path| fs::metadata(p).unwrap().ino();
        assert_ne!(ino(&original), ino(&link), "a copy now, not the original");
        assert_eq!(fs::read(&link).unwrap(), b"data");
        // A read-only copy is replaced by a writable one.
        use std::os::unix::fs::PermissionsExt;
        let ro = dir.join("ro.dat");
        fs::write(&ro, b"data").unwrap();
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o444)).unwrap();
        install_copy(&original, &ro).unwrap();
        assert!(!fs::metadata(&ro).unwrap().permissions().readonly());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn installs_create_missing_folders() {
        let dir = test_dir("fsx-folders");
        install(&dir.join("a/b/c.txt"), "deep").unwrap();
        assert_eq!(fs::read(dir.join("a/b/c.txt")).unwrap(), b"deep");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn refused_renames_are_retried_a_few_times() {
        let denied = || Err(io::Error::from(io::ErrorKind::PermissionDenied));
        let mut calls = 0;
        let r = retry_denied(5, Duration::ZERO, || {
            calls += 1;
            if calls < 3 { denied() } else { Ok(()) }
        });
        assert!(r.is_ok());
        assert_eq!(calls, 3, "held briefly, then free");
        calls = 0;
        assert!(
            retry_denied(5, Duration::ZERO, || {
                calls += 1;
                denied()
            })
            .is_err()
        );
        assert_eq!(calls, 5);
        calls = 0;
        let _ = retry_denied(5, Duration::ZERO, || {
            calls += 1;
            Err(io::Error::from(io::ErrorKind::NotFound))
        });
        assert_eq!(calls, 1, "other errors are final");
        calls = 0;
        let _ = retry_denied(0, Duration::ZERO, || {
            calls += 1;
            denied()
        });
        assert_eq!(calls, 1, "always tried once");
    }

    #[cfg(unix)]
    #[test]
    fn writes_never_touch_a_hard_linked_original() {
        use std::os::unix::fs::PermissionsExt;
        let dir = test_dir("fsx-link");
        let original = dir.join("original.dat");
        fs::write(&original, b"data").unwrap();
        fs::set_permissions(&original, fs::Permissions::from_mode(0o444)).unwrap();
        let link = dir.join("link.dat");
        fs::hard_link(&original, &link).unwrap();
        install(&link, b"replaced").unwrap();
        assert_eq!(fs::read(&link).unwrap(), b"replaced");
        assert_eq!(fs::read(&original).unwrap(), b"data");
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&original), 0o444, "installing over the link");
        fs::hard_link(&original, dir.join("again.dat")).unwrap();
        remove(&dir.join("again.dat")).unwrap();
        assert_eq!(mode(&original), 0o444, "removing the link");
        fs::set_permissions(&original, fs::Permissions::from_mode(0o644)).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn copies_get_only_the_owner_write_bit() {
        use std::os::unix::fs::PermissionsExt;
        let dir = test_dir("fsx-mode");
        let from = dir.join("ro.dat");
        fs::write(&from, b"x").unwrap();
        fs::set_permissions(&from, fs::Permissions::from_mode(0o444)).unwrap();
        install_copy(&from, &dir.join("copy.dat")).unwrap();
        let mode = fs::metadata(dir.join("copy.dat"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o644);
        fs::set_permissions(&from, fs::Permissions::from_mode(0o644)).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn remove_takes_dangling_links_and_ignores_missing_files() {
        let dir = test_dir("fsx-remove");
        std::os::unix::fs::symlink("nowhere", dir.join("dangling")).unwrap();
        remove(&dir.join("dangling")).unwrap();
        assert!(fs::symlink_metadata(dir.join("dangling")).is_err());
        remove(&dir.join("never-there")).unwrap();
        fs::remove_dir_all(dir).unwrap();
    }
}
