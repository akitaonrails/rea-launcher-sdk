//! The DirectInput 8 COM interfaces as vtables (windows-sys has the
//! structures but not the interfaces), a smart pointer for the ones the
//! layer calls, and the object header for the ones it implements.

use std::ffi::c_void;
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, Ordering};

use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    DIDATAFORMAT, DIDEVCAPS, DIDEVICEINSTANCEA, DIDEVICEOBJECTDATA, DIDEVICEOBJECTINSTANCEA,
    DIEFFECT, DIEFFECTINFOA, DIEFFESCAPE, DIPROPHEADER, LPDIENUMCREATEDEFFECTOBJECTSCALLBACK,
    LPDIENUMDEVICEOBJECTSCALLBACKA, LPDIENUMDEVICESCALLBACKA, LPDIENUMEFFECTSCALLBACKA,
};
use windows_sys::Win32::Foundation::{HANDLE, HINSTANCE, HWND};
use windows_sys::core::{GUID, HRESULT};

pub type This = *mut c_void;

/// `IID_IUnknown`.
pub const IID_UNKNOWN: GUID = GUID::from_u128(0x00000000_0000_0000_c000_000000000046);
/// `IID_IDirectInput8A`.
pub const IID_DIRECTINPUT8A: GUID = GUID::from_u128(0xbf798030_483a_4da2_aa99_5d64ed369700);
/// `IID_IDirectInputDevice8A`.
pub const IID_DEVICE8A: GUID = GUID::from_u128(0x54d41080_dc15_4833_a41b_748f73a38179);
/// `IID_IDirectInputEffect`.
pub const IID_EFFECT: GUID = GUID::from_u128(0xe7e1f7c0_88d2_11d0_9ad0_00a0c9a06e35);

pub fn same(a: &GUID, b: &GUID) -> bool {
    (a.data1, a.data2, a.data3, a.data4) == (b.data1, b.data2, b.data3, b.data4)
}

#[repr(C)]
pub struct UnknownVtbl {
    pub query_interface: unsafe extern "system" fn(This, *const GUID, *mut This) -> HRESULT,
    pub add_ref: unsafe extern "system" fn(This) -> u32,
    pub release: unsafe extern "system" fn(This) -> u32,
}

#[repr(C)]
pub struct DirectInputVtbl {
    pub base: UnknownVtbl,
    pub create_device: unsafe extern "system" fn(This, *const GUID, *mut This, This) -> HRESULT,
    pub enum_devices:
        unsafe extern "system" fn(This, u32, LPDIENUMDEVICESCALLBACKA, *mut c_void, u32) -> HRESULT,
    pub get_device_status: unsafe extern "system" fn(This, *const GUID) -> HRESULT,
    pub run_control_panel: unsafe extern "system" fn(This, HWND, u32) -> HRESULT,
    pub initialize: unsafe extern "system" fn(This, HINSTANCE, u32) -> HRESULT,
    pub find_device: unsafe extern "system" fn(This, *const GUID, *const u8, *mut GUID) -> HRESULT,
    /// The action-map calls pass through untouched: their pointers as
    /// they come.
    pub enum_devices_by_semantics: unsafe extern "system" fn(
        This,
        *const u8,
        *mut c_void,
        *mut c_void,
        *mut c_void,
        u32,
    ) -> HRESULT,
    pub configure_devices:
        unsafe extern "system" fn(This, *mut c_void, *mut c_void, u32, *mut c_void) -> HRESULT,
}

#[repr(C)]
pub struct DeviceVtbl {
    pub base: UnknownVtbl,
    pub get_capabilities: unsafe extern "system" fn(This, *mut DIDEVCAPS) -> HRESULT,
    pub enum_objects: unsafe extern "system" fn(
        This,
        LPDIENUMDEVICEOBJECTSCALLBACKA,
        *mut c_void,
        u32,
    ) -> HRESULT,
    pub get_property: unsafe extern "system" fn(This, *const GUID, *mut DIPROPHEADER) -> HRESULT,
    pub set_property: unsafe extern "system" fn(This, *const GUID, *const DIPROPHEADER) -> HRESULT,
    pub acquire: unsafe extern "system" fn(This) -> HRESULT,
    pub unacquire: unsafe extern "system" fn(This) -> HRESULT,
    pub get_device_state: unsafe extern "system" fn(This, u32, *mut c_void) -> HRESULT,
    pub get_device_data:
        unsafe extern "system" fn(This, u32, *mut DIDEVICEOBJECTDATA, *mut u32, u32) -> HRESULT,
    pub set_data_format: unsafe extern "system" fn(This, *const DIDATAFORMAT) -> HRESULT,
    pub set_event_notification: unsafe extern "system" fn(This, HANDLE) -> HRESULT,
    pub set_cooperative_level: unsafe extern "system" fn(This, HWND, u32) -> HRESULT,
    pub get_object_info:
        unsafe extern "system" fn(This, *mut DIDEVICEOBJECTINSTANCEA, u32, u32) -> HRESULT,
    pub get_device_info: unsafe extern "system" fn(This, *mut DIDEVICEINSTANCEA) -> HRESULT,
    pub run_control_panel: unsafe extern "system" fn(This, HWND, u32) -> HRESULT,
    pub initialize: unsafe extern "system" fn(This, HINSTANCE, u32, *const GUID) -> HRESULT,
    pub create_effect:
        unsafe extern "system" fn(This, *const GUID, *const DIEFFECT, *mut This, This) -> HRESULT,
    pub enum_effects:
        unsafe extern "system" fn(This, LPDIENUMEFFECTSCALLBACKA, *mut c_void, u32) -> HRESULT,
    pub get_effect_info:
        unsafe extern "system" fn(This, *mut DIEFFECTINFOA, *const GUID) -> HRESULT,
    pub get_force_feedback_state: unsafe extern "system" fn(This, *mut u32) -> HRESULT,
    pub send_force_feedback_command: unsafe extern "system" fn(This, u32) -> HRESULT,
    pub enum_created_effect_objects: unsafe extern "system" fn(
        This,
        LPDIENUMCREATEDEFFECTOBJECTSCALLBACK,
        *mut c_void,
        u32,
    ) -> HRESULT,
    pub escape: unsafe extern "system" fn(This, *mut DIEFFESCAPE) -> HRESULT,
    pub poll: unsafe extern "system" fn(This) -> HRESULT,
    pub send_device_data:
        unsafe extern "system" fn(This, u32, *const DIDEVICEOBJECTDATA, *mut u32, u32) -> HRESULT,
    pub enum_effects_in_file:
        unsafe extern "system" fn(This, *const u8, *mut c_void, *mut c_void, u32) -> HRESULT,
    pub write_effect_to_file:
        unsafe extern "system" fn(This, *const u8, u32, *mut c_void, u32) -> HRESULT,
    pub build_action_map: unsafe extern "system" fn(This, *mut c_void, *const u8, u32) -> HRESULT,
    pub set_action_map: unsafe extern "system" fn(This, *mut c_void, *const u8, u32) -> HRESULT,
    pub get_image_info: unsafe extern "system" fn(This, *mut c_void) -> HRESULT,
}

#[repr(C)]
pub struct EffectVtbl {
    pub base: UnknownVtbl,
    pub initialize: unsafe extern "system" fn(This, HINSTANCE, u32, *const GUID) -> HRESULT,
    pub get_effect_guid: unsafe extern "system" fn(This, *mut GUID) -> HRESULT,
    pub get_parameters: unsafe extern "system" fn(This, *mut DIEFFECT, u32) -> HRESULT,
    pub set_parameters: unsafe extern "system" fn(This, *const DIEFFECT, u32) -> HRESULT,
    pub start: unsafe extern "system" fn(This, u32, u32) -> HRESULT,
    pub stop: unsafe extern "system" fn(This) -> HRESULT,
    pub get_effect_status: unsafe extern "system" fn(This, *mut u32) -> HRESULT,
    pub download: unsafe extern "system" fn(This) -> HRESULT,
    pub unload: unsafe extern "system" fn(This) -> HRESULT,
    pub escape: unsafe extern "system" fn(This, *mut DIEFFESCAPE) -> HRESULT,
}

/// One reference to a COM object whose vtable is `V` (which starts with
/// [`UnknownVtbl`]); released on drop.
pub struct Com<V> {
    ptr: NonNull<c_void>,
    _vtbl: PhantomData<*const V>,
}

// SAFETY: DirectInput's objects may be called from any thread; the layer
// serialises its own calls behind a mutex.
unsafe impl<V> Send for Com<V> {}

impl<V> Com<V> {
    /// Takes over the reference `ptr` holds.
    ///
    /// # Safety
    /// `ptr` is null or a live COM object whose vtable is a `V`.
    pub unsafe fn from_raw(ptr: This) -> Option<Self> {
        NonNull::new(ptr).map(|ptr| Com {
            ptr,
            _vtbl: PhantomData,
        })
    }

    /// A new reference to `ptr`.
    ///
    /// # Safety
    /// As [`Com::from_raw`].
    pub unsafe fn add_ref(ptr: This) -> Option<Self> {
        // SAFETY: the caller's contract.
        let com = unsafe { Self::from_raw(ptr) }?;
        // SAFETY: a live object; the new reference is the one returned.
        unsafe { (com.unknown().add_ref)(com.raw()) };
        Some(com)
    }

    pub fn raw(&self) -> This {
        self.ptr.as_ptr()
    }

    pub fn vtbl(&self) -> &V {
        // SAFETY: a COM object starts with its vtable pointer (from_raw's
        // contract), which lives as long as the object.
        unsafe { &**self.ptr.as_ptr().cast::<*const V>() }
    }

    fn unknown(&self) -> &UnknownVtbl {
        // SAFETY: every `V` here starts with an `UnknownVtbl`.
        unsafe { &**self.ptr.as_ptr().cast::<*const UnknownVtbl>() }
    }
}

impl<V> Drop for Com<V> {
    fn drop(&mut self) {
        // SAFETY: this reference was ours to release.
        unsafe { (self.unknown().release)(self.raw()) };
    }
}

/// The start of every object the layer implements: its vtable and
/// reference count, then its own data.
#[repr(C)]
pub struct Object<V: 'static, T> {
    vtbl: &'static V,
    refs: AtomicU32,
    pub value: T,
}

impl<V: 'static, T> Object<V, T> {
    /// A new object with one reference, as a COM pointer.
    pub fn create(vtbl: &'static V, value: T) -> This {
        Box::into_raw(Box::new(Object {
            vtbl,
            refs: AtomicU32::new(1),
            value,
        }))
        .cast()
    }

    /// # Safety
    /// `this` came from [`Object::create`] with the same `V` and `T` and
    /// still has a reference.
    pub unsafe fn get<'a>(this: This) -> &'a Object<V, T> {
        // SAFETY: the caller's contract.
        unsafe { &*this.cast::<Object<V, T>>() }
    }

    pub fn add_ref(&self) -> u32 {
        self.refs.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Drops a reference; the last one frees the object.
    ///
    /// # Safety
    /// As [`Object::get`]; `this` is not used after its last release.
    pub unsafe fn release(this: This) -> u32 {
        // SAFETY: the caller's contract.
        let left = unsafe { Self::get(this) }
            .refs
            .fetch_sub(1, Ordering::AcqRel)
            - 1;
        if left == 0 {
            // SAFETY: the last reference: nobody else holds the box.
            drop(unsafe { Box::from_raw(this.cast::<Object<V, T>>()) });
        }
        left
    }
}

/// `QueryInterface` for an object that answers `iids`: itself, with a new
/// reference, or `E_NOINTERFACE`.
///
/// # Safety
/// `this` is an [`Object`] of `V` and `T`; `out` is null or writable.
pub unsafe fn query<V: 'static, T>(
    this: This,
    riid: *const GUID,
    out: *mut This,
    iids: &[GUID],
) -> HRESULT {
    const E_NOINTERFACE: HRESULT = 0x8000_4002_u32 as i32;
    const E_POINTER: HRESULT = 0x8000_4003_u32 as i32;
    if out.is_null() || riid.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller's contract; riid is a readable GUID.
    unsafe {
        if iids.iter().any(|iid| same(iid, &*riid)) {
            Object::<V, T>::get(this).add_ref();
            *out = this;
            0
        } else {
            *out = std::ptr::null_mut();
            E_NOINTERFACE
        }
    }
}
