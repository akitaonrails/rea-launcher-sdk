//! The registry redirect inside the game ([`crate::registry`]).
//!
//! When the DLL loads (the loader has filled the game's import table by
//! then), [`install`] reads [`crate::registry::ENV`] and points the main
//! executable's import slots for the registry open and create calls, found
//! by name, at the wrappers below. A wrapper asks [`Config::map`] where the
//! open goes: unmatched opens call the function the slot held, with the
//! game's own arguments; matched ones open the target key through the
//! wide-character call. Values are then read and written through the handle
//! the game got, so `RegQueryValueEx`/`RegSetValueEx` need no wrapper. Only
//! the game's executable is changed; other DLLs' registry calls are its own.

use std::ffi::CStr;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::Globalization::{CP_ACP, MultiByteToWideChar};
use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleHandleA, GetModuleHandleW, GetProcAddress,
};
use windows_sys::Win32::System::Memory::{PAGE_PROTECTION_FLAGS, PAGE_READWRITE, VirtualProtect};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RegCreateKeyExW, RegCreateKeyW, RegOpenKeyExW,
    RegOpenKeyW,
};
use windows_sys::core::{PCSTR, PCWSTR};

use crate::registry::{Config, Key, Root};

/// The rules, set once by [`install`].
static CONFIG: OnceLock<Config> = OnceLock::new();

/// Handles a `walk` rule opened (an ancestor of its key), with the key the
/// game asked for: the next open relative to one is matched again.
static WALKS: Mutex<Vec<(usize, Key)>> = Mutex::new(Vec::new());

/// The imports wrapped, in the order of [`ORIGINAL`] and [`wrappers`].
const NAMES: [&CStr; 9] = [
    c"RegOpenKeyA",
    c"RegOpenKeyW",
    c"RegOpenKeyExA",
    c"RegOpenKeyExW",
    c"RegCreateKeyA",
    c"RegCreateKeyW",
    c"RegCreateKeyExA",
    c"RegCreateKeyExW",
    c"RegCloseKey",
];
const OPEN_A: usize = 0;
const OPEN_W: usize = 1;
const OPEN_EX_A: usize = 2;
const OPEN_EX_W: usize = 3;
const CREATE_A: usize = 4;
const CREATE_W: usize = 5;
const CREATE_EX_A: usize = 6;
const CREATE_EX_W: usize = 7;
const CLOSE: usize = 8;

/// What each wrapped slot held before: the function an unmatched call goes
/// to (ADVAPI32's, or a compatibility shim's wrapper of it).
static ORIGINAL: [AtomicUsize; 9] = [const { AtomicUsize::new(0) }; 9];

type Open = unsafe extern "system" fn(HKEY, *const u8, *mut HKEY) -> u32;
type OpenEx = unsafe extern "system" fn(HKEY, *const u8, u32, u32, *mut HKEY) -> u32;
type CreateEx = unsafe extern "system" fn(
    HKEY,
    *const u8,
    u32,
    *const u8,
    u32,
    u32,
    *const SECURITY_ATTRIBUTES,
    *mut HKEY,
    *mut u32,
) -> u32;
type Close = unsafe extern "system" fn(HKEY) -> u32;

/// The function slot `which` held. The `A` and `W` forms share a type here:
/// the name pointer is passed through untouched.
///
/// # Safety
/// `F` is the function type of `NAMES[which]`, and the slot was wrapped
/// (only then is its wrapper reachable).
unsafe fn original<F: Copy>(which: usize) -> F {
    let address = ORIGINAL[which].load(Ordering::Acquire);
    // SAFETY: a non-null function address of the type the caller names.
    unsafe { std::mem::transmute_copy(&address) }
}

/// Reads the rules from the environment and wraps the game's imports.
/// Called from `DllMain`; without the variable, or with rules that don't
/// parse, the game is left alone.
pub fn install() {
    let Some(config) = std::env::var(crate::registry::ENV)
        .ok()
        .and_then(|text| Config::parse(&text).ok())
        .filter(|config| !config.rules.is_empty())
    else {
        return;
    };
    if CONFIG.set(config).is_err() {
        return;
    }
    // SAFETY: the main executable's mapped image, its headers read as the
    // PE format lays them out.
    unsafe { wrap_imports(GetModuleHandleW(null()).cast()) }
}

fn wrappers() -> [usize; 9] {
    [
        reg_open_key_a as *const () as usize,
        reg_open_key_w as *const () as usize,
        reg_open_key_ex_a as *const () as usize,
        reg_open_key_ex_w as *const () as usize,
        reg_create_key_a as *const () as usize,
        reg_create_key_w as *const () as usize,
        reg_create_key_ex_a as *const () as usize,
        reg_create_key_ex_w as *const () as usize,
        reg_close_key as *const () as usize,
    ]
}

/// Points the import slots of `base` (a mapped PE image) named in
/// [`NAMES`] at [`wrappers`]. Slots are found by the import's name, or,
/// when the image has no name table, by the address its DLL exports.
///
/// # Safety
/// `base` is a module mapped by the loader, whose imports are resolved.
unsafe fn wrap_imports(base: *const u8) {
    const ORDINAL: usize = 1 << (usize::BITS - 1);
    let thunk = size_of::<usize>();
    let wrappers = wrappers();
    // SAFETY: offsets inside the image's headers and import directory,
    // as the loader itself read them.
    unsafe {
        let u16_at = |offset: usize| base.add(offset).cast::<u16>().read_unaligned();
        let u32_at = |offset: usize| base.add(offset).cast::<u32>().read_unaligned() as usize;
        if base.is_null() || u16_at(0) != 0x5A4D {
            return;
        }
        let nt = u32_at(0x3C);
        if u32_at(nt) != 0x4550 {
            return;
        }
        let optional = nt + 24;
        let directories = match u16_at(optional) {
            0x10B => optional + 96,
            0x20B => optional + 112,
            _ => return,
        };
        let imports = u32_at(directories + 8);
        if imports == 0 {
            return;
        }
        for descriptor in (imports..).step_by(20) {
            let (names, dll, slots) = (
                u32_at(descriptor),
                u32_at(descriptor + 12),
                u32_at(descriptor + 16),
            );
            if dll == 0 || slots == 0 {
                break;
            }
            let module = GetModuleHandleA(base.add(dll));
            for i in 0.. {
                let slot = base.add(slots + i * thunk).cast::<usize>().cast_mut();
                let current = slot.read();
                if current == 0 {
                    break;
                }
                let which = if names != 0 {
                    let entry = base.add(names + i * thunk).cast::<usize>().read();
                    (entry & ORDINAL == 0)
                        .then(|| CStr::from_ptr(base.add(entry + 2).cast()))
                        .and_then(|name| NAMES.iter().position(|n| *n == name))
                } else if !module.is_null() {
                    NAMES.iter().position(|n| {
                        GetProcAddress(module, n.as_ptr().cast())
                            .is_some_and(|f| f as usize == current)
                    })
                } else {
                    None
                };
                if let Some(which) = which {
                    let mut old: PAGE_PROTECTION_FLAGS = 0;
                    if VirtualProtect(slot.cast(), thunk, PAGE_READWRITE, &mut old) != 0 {
                        ORIGINAL[which].store(current, Ordering::Release);
                        slot.write(wrappers[which]);
                        VirtualProtect(slot.cast(), thunk, old, &mut old);
                    }
                }
            }
        }
    }
}

fn root_of(key: HKEY) -> Option<Root> {
    if key == HKEY_LOCAL_MACHINE {
        Some(Root::LocalMachine)
    } else if key == HKEY_CURRENT_USER {
        Some(Root::CurrentUser)
    } else {
        None
    }
}

fn hkey(root: Root) -> HKEY {
    match root {
        Root::LocalMachine => HKEY_LOCAL_MACHINE,
        Root::CurrentUser => HKEY_CURRENT_USER,
    }
}

fn walked(key: HKEY) -> Option<Key> {
    let walks = WALKS.lock().ok()?;
    walks
        .iter()
        .find(|(handle, _)| *handle == key as usize)
        .map(|(_, key)| key.clone())
}

/// What an open of `subkey` under `root` does.
enum Plan {
    /// The game's own call, untouched.
    Pass,
    /// Open `path` (NUL-terminated) under `root` instead; remember the
    /// handle with `walk` when set.
    Open {
        root: Root,
        path: Vec<u16>,
        walk: Option<Key>,
    },
}

fn plan(root: HKEY, subkey: impl FnOnce() -> String) -> Plan {
    let Some(config) = CONFIG.get() else {
        return Plan::Pass;
    };
    let (base, from_walk) = match root_of(root) {
        Some(root) => (Key::new(root, ""), false),
        None => match walked(root) {
            Some(key) => (key, true),
            None => return Plan::Pass,
        },
    };
    let key = base.join(&subkey());
    let wide = |path: &str| path.encode_utf16().chain([0]).collect();
    match config.map(&key) {
        Some(target) => Plan::Open {
            root: target.key.root,
            path: wide(&target.key.path),
            walk: target.walk.then_some(key),
        },
        // A step off a walked rule's path: the key the game asked for.
        None if from_walk => Plan::Open {
            root: key.root,
            path: wide(&key.path),
            walk: None,
        },
        None => Plan::Pass,
    }
}

/// Runs an open: `pass` (the game's call) or `open(root, path)` as
/// [`plan`] says, remembering a walk handle written to `out`.
///
/// # Safety
/// `out` is the game's result pointer, written by a successful open.
unsafe fn open_with(
    root: HKEY,
    subkey: impl FnOnce() -> String,
    out: *mut HKEY,
    pass: impl FnOnce() -> u32,
    open: impl FnOnce(HKEY, PCWSTR) -> u32,
) -> u32 {
    match plan(root, subkey) {
        Plan::Pass => pass(),
        Plan::Open { root, path, walk } => {
            let code = open(hkey(root), path.as_ptr());
            if code == ERROR_SUCCESS
                && !out.is_null()
                && let Some(key) = walk
                && let Ok(mut walks) = WALKS.lock()
            {
                // SAFETY: written by the successful open.
                walks.push((unsafe { *out } as usize, key));
            }
            code
        }
    }
}

/// An ANSI name the game passed, decoded with the system code page.
///
/// # Safety
/// `text` is null or NUL-terminated.
unsafe fn ansi(text: PCSTR) -> String {
    if text.is_null() {
        return String::new();
    }
    // SAFETY: a NUL-terminated string (-1) into a buffer of the length
    // the first call reports.
    unsafe {
        let len = MultiByteToWideChar(CP_ACP, 0, text, -1, null_mut(), 0);
        if len <= 0 {
            return CStr::from_ptr(text.cast()).to_string_lossy().into_owned();
        }
        let mut wide = vec![0u16; len as usize];
        MultiByteToWideChar(CP_ACP, 0, text, -1, wide.as_mut_ptr(), len);
        String::from_utf16_lossy(&wide[..len as usize - 1])
    }
}

/// A wide name the game passed.
///
/// # Safety
/// `text` is null or NUL-terminated.
unsafe fn utf16(text: PCWSTR) -> String {
    if text.is_null() {
        return String::new();
    }
    // SAFETY: reads up to the terminating NUL.
    unsafe {
        let len = (0..).take_while(|&i| *text.add(i) != 0).count();
        String::from_utf16_lossy(std::slice::from_raw_parts(text, len))
    }
}

unsafe extern "system" fn reg_open_key_a(root: HKEY, subkey: PCSTR, out: *mut HKEY) -> u32 {
    // SAFETY: the game's arguments, passed on; the slot was wrapped.
    unsafe {
        open_with(
            root,
            || ansi(subkey),
            out,
            || original::<Open>(OPEN_A)(root, subkey, out),
            |root, path| RegOpenKeyW(root, path, out),
        )
    }
}

unsafe extern "system" fn reg_open_key_w(root: HKEY, subkey: PCWSTR, out: *mut HKEY) -> u32 {
    // SAFETY: as in `reg_open_key_a`.
    unsafe {
        open_with(
            root,
            || utf16(subkey),
            out,
            || original::<Open>(OPEN_W)(root, subkey.cast(), out),
            |root, path| RegOpenKeyW(root, path, out),
        )
    }
}

unsafe extern "system" fn reg_open_key_ex_a(
    root: HKEY,
    subkey: PCSTR,
    options: u32,
    access: u32,
    out: *mut HKEY,
) -> u32 {
    // SAFETY: as in `reg_open_key_a`.
    unsafe {
        open_with(
            root,
            || ansi(subkey),
            out,
            || original::<OpenEx>(OPEN_EX_A)(root, subkey, options, access, out),
            |root, path| RegOpenKeyExW(root, path, options, access, out),
        )
    }
}

unsafe extern "system" fn reg_open_key_ex_w(
    root: HKEY,
    subkey: PCWSTR,
    options: u32,
    access: u32,
    out: *mut HKEY,
) -> u32 {
    // SAFETY: as in `reg_open_key_a`.
    unsafe {
        open_with(
            root,
            || utf16(subkey),
            out,
            || original::<OpenEx>(OPEN_EX_W)(root, subkey.cast(), options, access, out),
            |root, path| RegOpenKeyExW(root, path, options, access, out),
        )
    }
}

unsafe extern "system" fn reg_create_key_a(root: HKEY, subkey: PCSTR, out: *mut HKEY) -> u32 {
    // SAFETY: as in `reg_open_key_a`.
    unsafe {
        open_with(
            root,
            || ansi(subkey),
            out,
            || original::<Open>(CREATE_A)(root, subkey, out),
            |root, path| RegCreateKeyW(root, path, out),
        )
    }
}

unsafe extern "system" fn reg_create_key_w(root: HKEY, subkey: PCWSTR, out: *mut HKEY) -> u32 {
    // SAFETY: as in `reg_open_key_a`.
    unsafe {
        open_with(
            root,
            || utf16(subkey),
            out,
            || original::<Open>(CREATE_W)(root, subkey.cast(), out),
            |root, path| RegCreateKeyW(root, path, out),
        )
    }
}

#[allow(clippy::too_many_arguments)]
unsafe extern "system" fn reg_create_key_ex_a(
    root: HKEY,
    subkey: PCSTR,
    reserved: u32,
    class: PCSTR,
    options: u32,
    access: u32,
    security: *const SECURITY_ATTRIBUTES,
    out: *mut HKEY,
    disposition: *mut u32,
) -> u32 {
    // SAFETY: as in `reg_open_key_a`; the class is decoded like the name.
    unsafe {
        open_with(
            root,
            || ansi(subkey),
            out,
            || {
                original::<CreateEx>(CREATE_EX_A)(
                    root,
                    subkey,
                    reserved,
                    class,
                    options,
                    access,
                    security,
                    out,
                    disposition,
                )
            },
            |root, path| {
                let class: Option<Vec<u16>> =
                    (!class.is_null()).then(|| ansi(class).encode_utf16().chain([0]).collect());
                RegCreateKeyExW(
                    root,
                    path,
                    reserved,
                    class.as_ref().map_or(null(), |c| c.as_ptr()),
                    options,
                    access,
                    security,
                    out,
                    disposition,
                )
            },
        )
    }
}

#[allow(clippy::too_many_arguments)]
unsafe extern "system" fn reg_create_key_ex_w(
    root: HKEY,
    subkey: PCWSTR,
    reserved: u32,
    class: PCWSTR,
    options: u32,
    access: u32,
    security: *const SECURITY_ATTRIBUTES,
    out: *mut HKEY,
    disposition: *mut u32,
) -> u32 {
    // SAFETY: as in `reg_open_key_a`.
    unsafe {
        open_with(
            root,
            || utf16(subkey),
            out,
            || {
                original::<CreateEx>(CREATE_EX_W)(
                    root,
                    subkey.cast(),
                    reserved,
                    class.cast(),
                    options,
                    access,
                    security,
                    out,
                    disposition,
                )
            },
            |root, path| {
                RegCreateKeyExW(
                    root,
                    path,
                    reserved,
                    class,
                    options,
                    access,
                    security,
                    out,
                    disposition,
                )
            },
        )
    }
}

/// Forgets a walk handle before it closes: Windows may hand its value out
/// again for another key.
unsafe extern "system" fn reg_close_key(key: HKEY) -> u32 {
    if let Ok(mut walks) = WALKS.lock() {
        walks.retain(|(handle, _)| *handle != key as usize);
    }
    // SAFETY: the game's handle, passed on; the slot was wrapped.
    unsafe { original::<Close>(CLOSE)(key) }
}
