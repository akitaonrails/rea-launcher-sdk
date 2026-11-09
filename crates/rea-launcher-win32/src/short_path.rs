//! Windows' short (8.3) names, for old games that read a folder path into
//! a small fixed-size buffer (`rea_launcher::paths::fit_path` picks which
//! path to hand them).

use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::GetShortPathNameW;

/// The short name of an existing `path`, when its volume keeps 8.3 names;
/// `None` otherwise (or when `path` doesn't exist).
pub fn short_path(path: &Path) -> Option<String> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut buf = vec![0u16; 1024];
    // SAFETY: a NUL-terminated input and an output buffer of the given size.
    let len = unsafe { GetShortPathNameW(wide.as_ptr(), buf.as_mut_ptr(), buf.len() as u32) };
    if len == 0 || len as usize >= buf.len() {
        return None;
    }
    OsString::from_wide(&buf[..len as usize]).into_string().ok()
}

/// [`short_path`] for a folder that may not exist yet (a `--check` before
/// the first start): the short name of its deepest existing parent, then
/// the rest of the path as it is.
pub fn short_path_ahead(path: &Path) -> Option<String> {
    let mut existing = path;
    let mut rest = Vec::new();
    while !existing.exists() {
        rest.push(existing.file_name()?.to_string_lossy().into_owned());
        existing = existing.parent()?;
    }
    let mut short = short_path(existing)?;
    for part in rest.iter().rev() {
        if !short.ends_with('\\') {
            short.push('\\');
        }
        short.push_str(part);
    }
    Some(short)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_folder_has_a_shorter_name_for_the_same_place() {
        let dir = std::env::temp_dir().join(format!(
            "rea-short-path-{}-a-folder-name-long-enough-to-need-a-short-one",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        // Short names may be off on a volume: then there is nothing to test.
        if let Some(short) = short_path(&dir) {
            assert!(short.len() < dir.display().to_string().len(), "{short}");
            assert!(short.is_ascii(), "{short}");
            assert!(Path::new(&short).is_dir(), "{short} names the same folder");
            let ahead = short_path_ahead(&dir.join("not-yet").join("Rally")).unwrap();
            assert_eq!(ahead, format!(r"{short}\not-yet\Rally"));
        }
        assert_eq!(short_path(&dir.join("missing")), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
