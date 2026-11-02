//! Free disk space (OR's, for its download size check).

use std::path::Path;
use std::ptr::null_mut;

use crate::wide::wide;

/// Free bytes for this user on the drive holding `dir`, or its nearest
/// existing parent.
pub fn free_space(dir: &Path) -> Option<u64> {
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let existing = dir.ancestors().find(|path| path.exists())?;
    let mut free = 0u64;
    // SAFETY: a NUL-terminated path and a local output; the totals are not
    // asked for.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide(existing.as_os_str()).as_ptr(),
            &mut free,
            null_mut(),
            null_mut(),
        )
    };
    (ok != 0).then_some(free)
}
