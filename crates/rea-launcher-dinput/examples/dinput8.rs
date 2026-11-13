//! A stand-in for a `dinput8.dll` in the game's folder (Xidi's place),
//! for the layer's tests: its `DirectInput8Create` counts its calls
//! (`stand_in_calls`) and passes them to the system's DirectInput, so a
//! test sees the layer chain in front of it.

#[cfg(windows)]
mod stand_in {
    use std::ffi::c_void;
    use std::sync::atomic::{AtomicU32, Ordering};

    use windows_sys::Win32::Foundation::HMODULE;
    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
    use windows_sys::core::{GUID, HRESULT};

    static CALLS: AtomicU32 = AtomicU32::new(0);

    type Create = unsafe extern "system" fn(
        HMODULE,
        u32,
        *const GUID,
        *mut *mut c_void,
        *mut c_void,
    ) -> HRESULT;

    /// The system's `DirectInput8Create`.
    fn system() -> Option<Create> {
        let mut dir = [0u16; 260];
        // SAFETY: a local buffer of the given size.
        let len = unsafe { GetSystemDirectoryW(dir.as_mut_ptr(), dir.len() as u32) } as usize;
        let path: Vec<u16> = dir[..len]
            .iter()
            .copied()
            .chain("\\dinput8.dll\0".encode_utf16())
            .collect();
        // SAFETY: a NUL-terminated path; the export has this type.
        unsafe {
            let module = LoadLibraryW(path.as_ptr());
            if module.is_null() {
                return None;
            }
            GetProcAddress(module, c"DirectInput8Create".as_ptr().cast())
                .map(|f| std::mem::transmute::<unsafe extern "system" fn() -> isize, Create>(f))
        }
    }

    /// # Safety
    /// DirectInput's contract.
    #[unsafe(no_mangle)]
    pub unsafe extern "system" fn DirectInput8Create(
        instance: HMODULE,
        version: u32,
        riid: *const GUID,
        out: *mut *mut c_void,
        outer: *mut c_void,
    ) -> HRESULT {
        CALLS.fetch_add(1, Ordering::Relaxed);
        match system() {
            // SAFETY: the caller's arguments, passed on.
            Some(create) => unsafe { create(instance, version, riid, out, outer) },
            None => 0x8000_4005_u32 as i32,
        }
    }

    #[unsafe(no_mangle)]
    pub extern "system" fn stand_in_calls() -> u32 {
        CALLS.load(Ordering::Relaxed)
    }
}
