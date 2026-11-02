//! Wine detection and Wine's bus-driver settings (Windows only).
//!
//! `is_wine` is the version all launchers converged on (CMR4/OR): the
//! `wine_get_version` export, plus the environment Wine, Proton and the
//! play helpers set (`rea_launcher::wine::wine_env`), for Wine builds that
//! hide the export. Seeding the winebus key only where values are absent is
//! OR's (done in the launcher so it also works under plain Proton); the
//! values are game knowledge (`rea_launcher::wine` has the names).

use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

use crate::reg::{Error, Hive, Key};
use crate::wide::wide;

/// True under Wine or Proton.
pub fn is_wine() -> bool {
    // SAFETY: read-only loader queries with NUL-terminated names.
    let exported = unsafe {
        let ntdll = GetModuleHandleW(wide("ntdll.dll").as_ptr());
        !ntdll.is_null() && GetProcAddress(ntdll, c"wine_get_version".as_ptr().cast()).is_some()
    };
    exported || rea_launcher::wine::is_wine_env()
}

/// Seeds the winebus values the prefix lacks; a value already there
/// (the player's choice) is kept. True when anything was written.
pub fn seed_winebus(values: &[(&str, u32)]) -> Result<bool, Error> {
    Key::create(Hive::LocalMachine, rea_launcher::wine::WINEBUS_KEY)?.seed_dwords(values)
}
