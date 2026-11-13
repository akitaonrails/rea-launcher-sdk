//! `DllMain`: the game's `dinput8!DirectInput8Create` import pointed at
//! [`direct_input8_create`], which calls the function the import held
//! (the system's DirectInput, or Xidi's) and wraps what it made.
//!
//! The executable's imports are resolved by the time this `DllMain` runs,
//! whether the DLL is in the import table (`import_into`) or loaded later
//! by an ASI loader, so the slot already holds the function the game
//! would have called. Only the executable's own import is changed: other
//! DLLs keep theirs.

use std::ffi::{CStr, c_void};
use std::ptr::null;
use std::sync::atomic::{AtomicUsize, Ordering};

use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleHandleA, GetModuleHandleW, GetProcAddress,
};
use windows_sys::Win32::System::Memory::{PAGE_PROTECTION_FLAGS, PAGE_READWRITE, VirtualProtect};
use windows_sys::Win32::System::SystemServices::DLL_PROCESS_ATTACH;
use windows_sys::core::{GUID, HRESULT};

use super::com::{IID_DIRECTINPUT8A, same};
use super::{di8, guard, hr, log};

const DLL: &CStr = c"dinput8.dll";
const NAME: &CStr = c"DirectInput8Create";

type Create =
    unsafe extern "system" fn(HMODULE, u32, *const GUID, *mut *mut c_void, *mut c_void) -> HRESULT;

/// The function the game's import held.
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);

/// What the game's runtime copy imports so the loader maps this DLL
/// ([`crate::LOAD_EXPORT`]).
#[unsafe(no_mangle)]
pub extern "system" fn rea_dinput_load() -> u32 {
    1
}

/// Points the game's `DirectInput8Create` import at the wrapper. Never
/// fails the load: without the import the game is left alone.
///
/// # Safety
/// Called by the loader.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllMain(
    _module: HMODULE,
    reason: u32,
    _reserved: *mut c_void,
) -> i32 {
    if reason == DLL_PROCESS_ATTACH {
        // SAFETY: the main executable's mapped image, its imports resolved.
        guard((), || unsafe {
            wrap_import(GetModuleHandleW(null()).cast())
        });
    }
    1
}

/// Replaces the executable's `dinput8!DirectInput8Create` import slot
/// (found by name, or by address when the image has no name table).
///
/// # Safety
/// `base` is a module mapped by the loader, whose imports are resolved.
unsafe fn wrap_import(base: *const u8) {
    const ORDINAL: usize = 1 << (usize::BITS - 1);
    let thunk = size_of::<usize>();
    // SAFETY: offsets inside the image's headers and import directory, as
    // the loader itself read them.
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
            if !CStr::from_ptr(base.add(dll).cast())
                .to_bytes()
                .eq_ignore_ascii_case(DLL.to_bytes())
            {
                continue;
            }
            let module = GetModuleHandleA(DLL.as_ptr().cast());
            let exported = (!module.is_null())
                .then(|| GetProcAddress(module, NAME.as_ptr().cast()))
                .flatten()
                .map(|f| f as usize);
            for i in 0.. {
                let slot = base.add(slots + i * thunk).cast::<usize>().cast_mut();
                let current = slot.read();
                if current == 0 {
                    break;
                }
                let named = if names != 0 {
                    let entry = base.add(names + i * thunk).cast::<usize>().read();
                    entry & ORDINAL == 0 && CStr::from_ptr(base.add(entry + 2).cast()) == NAME
                } else {
                    exported == Some(current)
                };
                if named {
                    replace(slot, current);
                }
            }
        }
    }
}

/// # Safety
/// `slot` is an import slot of the mapped executable holding `current`.
unsafe fn replace(slot: *mut usize, current: usize) {
    let wrapper = direct_input8_create as *const () as usize;
    if current == wrapper {
        return;
    }
    let mut old: PAGE_PROTECTION_FLAGS = 0;
    // SAFETY: the caller's contract; the page's protection is restored.
    unsafe {
        if VirtualProtect(slot.cast(), size_of::<usize>(), PAGE_READWRITE, &mut old) != 0 {
            ORIGINAL.store(current, Ordering::Release);
            slot.write(wrapper);
            VirtualProtect(slot.cast(), size_of::<usize>(), old, &mut old);
        }
    }
}

/// The game's `DirectInput8Create`: the original's result, wrapped when
/// the game asked for `IDirectInput8A` and the layer is on.
unsafe extern "system" fn direct_input8_create(
    instance: HMODULE,
    version: u32,
    riid: *const GUID,
    out: *mut *mut c_void,
    outer: *mut c_void,
) -> HRESULT {
    let original = ORIGINAL.load(Ordering::Acquire);
    if original == 0 {
        return hr::GENERIC;
    }
    // SAFETY: the function the import held, of this type.
    let original: Create = unsafe { std::mem::transmute::<usize, Create>(original) };
    // SAFETY: the game's own arguments, passed on.
    let code = unsafe { original(instance, version, riid, out, outer) };
    // SAFETY: the game's IID and out pointer, checked before use.
    let ansi = unsafe { riid.as_ref() }.is_some_and(|iid| same(iid, &IID_DIRECTINPUT8A));
    if !hr::ok(code) || out.is_null() || !outer.is_null() || !ansi {
        log!("DirectInput8Create {version:#x}: passed through ({code:#x})");
        return code;
    }
    // SAFETY: a new IDirectInput8A, which the game's reference keeps
    // alive until the wrapper (with a reference of its own) replaces it.
    unsafe {
        let made = *out;
        if let Some(wrapper) = guard(None, || di8::wrap(made)) {
            *out = wrapper;
            let unknown = &**made.cast::<*const super::com::UnknownVtbl>();
            (unknown.release)(made);
        }
    }
    code
}
