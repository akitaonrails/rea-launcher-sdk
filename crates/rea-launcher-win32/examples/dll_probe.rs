//! Loads d3dx9_35.dll the way a game does (by name, from its own folder)
//! and exits with which copy loaded: 10 the native stand-in next to it,
//! 11 Wine's builtin (or the system's), 12 none.

#[cfg(windows)]
fn main() {
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    let name: Vec<u16> = "d3dx9_35.dll\0".encode_utf16().collect();
    // SAFETY: a NUL-terminated name; the module stays loaded until exit.
    let code = unsafe {
        let module = LoadLibraryW(name.as_ptr());
        if module.is_null() {
            12
        } else if GetProcAddress(module, c"rea_native_marker".as_ptr().cast()).is_some() {
            10
        } else {
            11
        }
    };
    std::process::exit(code);
}

#[cfg(not(windows))]
fn main() {}
