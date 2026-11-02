//! UTF-16 strings for Win32 calls.

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::{OsStrExt, OsStringExt};

/// `value` as a NUL-terminated UTF-16 string.
pub fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
    value.as_ref().encode_wide().chain(Some(0)).collect()
}

/// The text of a UTF-16 buffer up to its first NUL (or its end).
pub fn from_wide(buffer: &[u16]) -> OsString {
    let len = buffer
        .iter()
        .position(|&unit| unit == 0)
        .unwrap_or(buffer.len());
    OsString::from_wide(&buffer[..len])
}

pub(crate) fn last_error() -> std::io::Error {
    std::io::Error::last_os_error()
}
