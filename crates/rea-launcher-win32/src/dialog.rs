//! Message boxes, the folder picker and opening a web page.
//!
//! The same code in every Wine launcher (SRR/OR/CMR3/CMR4); this copy adds
//! SAFETY notes and balances `CoInitializeEx`. The picker is the classic
//! `SHBrowseForFolderW`, which Wine draws reliably (SCR's PowerShell picker
//! does not exist under Wine).

use std::path::PathBuf;
use std::ptr::{null, null_mut};

use crate::wide::{from_wide, wide};

/// The icon of a [`message`] box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Info,
    Warning,
    Error,
}

/// A modal message box with an OK button and no owner window.
pub fn message(title: &str, text: &str, icon: Icon) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MB_ICONERROR, MB_ICONINFORMATION, MB_ICONWARNING, MB_OK, MessageBoxW,
    };
    let icon = match icon {
        Icon::Info => MB_ICONINFORMATION,
        Icon::Warning => MB_ICONWARNING,
        Icon::Error => MB_ICONERROR,
    };
    // SAFETY: NUL-terminated strings that outlive the modal call.
    unsafe {
        MessageBoxW(
            null_mut(),
            wide(text).as_ptr(),
            wide(title).as_ptr(),
            MB_OK | icon,
        );
    }
}

/// Asks for a folder with the system's folder picker; `None` when cancelled.
pub fn pick_folder(title: &str) -> Option<PathBuf> {
    use windows_sys::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, CoInitializeEx, CoTaskMemFree, CoUninitialize,
    };
    use windows_sys::Win32::UI::Shell::{
        BIF_NEWDIALOGSTYLE, BIF_RETURNONLYFSDIRS, BROWSEINFOW, SHBrowseForFolderW,
        SHGetPathFromIDListW,
    };
    let title = wide(title);
    let mut name = [0u16; 260];
    let mut path = [0u16; 1024];
    // SAFETY: the structure and buffers live across the modal call; the
    // returned item list is freed with the COM allocator, and COM is
    // uninitialised only when this call initialised it (S_OK or S_FALSE).
    unsafe {
        let com = CoInitializeEx(null(), COINIT_APARTMENTTHREADED as u32);
        let info = BROWSEINFOW {
            hwndOwner: null_mut(),
            pidlRoot: null_mut(),
            pszDisplayName: name.as_mut_ptr(),
            lpszTitle: title.as_ptr(),
            ulFlags: BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE,
            lpfn: None,
            lParam: 0,
            iImage: 0,
        };
        let list = SHBrowseForFolderW(&info);
        let ok = !list.is_null() && SHGetPathFromIDListW(list, path.as_mut_ptr()) != 0;
        if !list.is_null() {
            CoTaskMemFree(list as *const _);
        }
        if com >= 0 {
            CoUninitialize();
        }
        let path = from_wide(&path);
        (ok && !path.is_empty()).then(|| path.into())
    }
}

/// Opens `url` in the default browser (under Wine, the desktop's); false
/// when the shell refused.
pub fn open_url(url: &str) -> bool {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    // SAFETY: NUL-terminated strings; no owner window.
    let result = unsafe {
        ShellExecuteW(
            null_mut(),
            wide("open").as_ptr(),
            wide(url).as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        )
    };
    // ShellExecuteW reports success as a value above 32.
    result as usize > 32
}
