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

/// Builds `path`'s new content with `make` at [`temp_beside`], then renames
/// it over `path`. Parent folders are created. Nothing is ever written
/// through `path`, which may be a hard link to the player's original.
pub fn put_in_place(path: &Path, make: impl FnOnce(&Path) -> io::Result<()>) -> io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    let tmp = temp_beside(path);
    let _ = fs::remove_file(&tmp);
    let result = make(&tmp).and_then(|()| {
        match retry_denied(RENAME_TRIES, RENAME_WAIT, || fs::rename(&tmp, path)) {
            // Windows refuses to replace a read-only file: remove it (std
            // ignores the attribute and never changes it), then rename.
            Err(e) if e.kind() == io::ErrorKind::PermissionDenied && exists(path) => {
                fs::remove_file(path)?;
                fs::rename(&tmp, path)
            }
            other => other,
        }
    });
    // Gone after a rename, except when `path` already was the same file
    // (renaming a hard link onto itself does nothing).
    let _ = fs::remove_file(&tmp);
    result
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
    put_in_place(path, |tmp| fs::write(tmp, bytes.as_ref()))
}

/// Like [`install`], but flushed to disk before the rename: for settings,
/// manifests and ledgers, where a crash must leave the old file or the new
/// one, never a torn one.
pub fn write_atomic(path: &Path, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    put_in_place(path, |tmp| {
        let mut file = fs::File::create(tmp)?;
        io::Write::write_all(&mut file, bytes.as_ref())?;
        file.sync_all()
    })
}

/// Copies `from` over `path` as a fresh file its owner can write (a copy
/// of a read-only original would be read-only too).
pub fn install_copy(from: &Path, path: &Path) -> io::Result<()> {
    put_in_place(path, |tmp| {
        fs::copy(from, tmp)?;
        make_writable(tmp)
    })
}

/// Removes a file (or a symbolic link, dangling or not) when it exists.
/// The file's attributes are never changed first: it may be a hard link to
/// the player's original, which shares them.
pub fn remove(path: &Path) -> io::Result<()> {
    if exists(path) {
        fs::remove_file(path)?;
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
