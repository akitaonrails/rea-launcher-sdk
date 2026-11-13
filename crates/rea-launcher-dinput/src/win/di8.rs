//! The game's `IDirectInput8A`, wrapped: `EnumDevices` lists the virtual
//! wheel first and leaves out the devices it is made of, `CreateDevice`
//! makes it; everything else goes to the DirectInput the game would have
//! had.

use std::ffi::c_void;
use std::ptr::null_mut;

use rea_launcher::wheel::rig::Device;
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    DI8DEVCLASS_ALL, DI8DEVCLASS_GAMECTRL, DI8DEVTYPE_DRIVING, DIDEVICEINSTANCEA, DIENUM_CONTINUE,
    DIENUM_STOP, LPDIENUMDEVICESCALLBACKA,
};
use windows_sys::Win32::Foundation::{HINSTANCE, HWND};
use windows_sys::core::{BOOL, GUID, HRESULT};

use super::com::{
    Com, DeviceVtbl, DirectInputVtbl, IID_DIRECTINPUT8A, IID_UNKNOWN, Object, This, UnknownVtbl,
    same,
};
use super::device::{self, INSTANCE, Rig};
use super::real::{self, Attached, Real};
use super::{config, fake, guard, guid_text, hr, log};
use crate::devices::{self, Merge, Plan};
use rea_launcher::wheel::dinput::Mode;

/// The wrapper's data: the game's DirectInput and what was found when it
/// was made.
pub(super) struct Wrapper {
    inner: Com<DirectInputVtbl>,
    attached: Vec<Attached>,
    plan: Plan,
}

type Wrapped = Object<DirectInputVtbl, Wrapper>;

/// The wrapper for `inner`, the `IDirectInput8A` the game asked for;
/// `None` (the game keeps `inner`) when the layer is off or (`auto`)
/// there is no wheel. The wrapper takes a reference of its own.
///
/// # Safety
/// `inner` is a live `IDirectInput8A`.
pub(super) unsafe fn wrap(inner: This) -> Option<This> {
    let config = config();
    if config.mode == Mode::Off {
        log!("off: the game gets its DirectInput untouched");
        return None;
    }
    // SAFETY: the caller's contract.
    let inner = unsafe { Com::<DirectInputVtbl>::add_ref(inner) }?;
    // SAFETY: a live DirectInput.
    let mut attached = unsafe { real::attached(&inner) };
    if let Some(kind) = config.fake {
        attached.extend(fake::install(kind));
    }
    let devices: Vec<Device> = attached.iter().map(|a| a.device.clone()).collect();
    for d in &devices {
        log!(
            "found {} ({:04x}:{:04x}) {} {:?}, {} axes, {} buttons{}",
            d.id.name,
            d.id.vid,
            d.id.pid,
            d.id.guid,
            d.kind,
            d.axes,
            d.buttons,
            if d.ffb { ", force feedback" } else { "" }
        );
    }
    let plan = devices::plan(
        &devices,
        config.mode,
        &config.setup,
        config.profile.as_ref(),
    );
    if !plan.show {
        log!("auto and no wheel: the game gets its DirectInput untouched");
        return None;
    }
    log!(
        "virtual wheel from {} and {}; hidden: {:?}",
        plan.wheel
            .as_ref()
            .map_or("no wheel", |d| d.id.name.as_str()),
        plan.pedals
            .as_ref()
            .map_or("the wheel's pedals", |d| d.id.name.as_str()),
        plan.hidden
    );
    Some(Wrapped::create(
        &VTBL,
        Wrapper {
            inner,
            attached,
            plan,
        },
    ))
}

impl Wrapper {
    /// Opens the attached device `wanted` for the virtual wheel.
    fn open(&self, wanted: Option<&Device>) -> Option<Real> {
        let attached = self
            .attached
            .iter()
            .find(|a| Some(&a.device.id.guid) == wanted.map(|d| &d.id.guid))?;
        let com = match attached.fake {
            Some(index) => fake::open(index)?,
            None => {
                let mut ptr = null_mut();
                // SAFETY: a live DirectInput; the out pointer takes one
                // reference, which `Com` releases.
                unsafe {
                    let code = (self.inner.vtbl().create_device)(
                        self.inner.raw(),
                        &attached.instance,
                        &mut ptr,
                        null_mut(),
                    );
                    if !hr::ok(code) {
                        log!("{}: not opened ({code:#x})", attached.device.id.name);
                        return None;
                    }
                    Com::<DeviceVtbl>::from_raw(ptr)?
                }
            }
        };
        Real::open(com, attached.device.clone())
    }

    /// A new virtual wheel on the planned devices.
    fn virtual_wheel(&self) -> This {
        let config = config();
        let wheel = self.open(self.plan.wheel.as_ref());
        let pedals = self.open(self.plan.pedals.as_ref());
        let merge = wheel.as_ref().map(|w| {
            Merge::new(
                config.setup.clone(),
                config.profile.clone(),
                w.device.clone(),
                pedals.as_ref().map(|p| p.device.clone()),
            )
        });
        device::create(Rig::new(wheel, pedals, merge, config.setup.ffb))
    }

    fn hides(&self, guid: &GUID) -> bool {
        let text = guid_text(guid);
        self.plan
            .hidden
            .iter()
            .any(|h| h.eq_ignore_ascii_case(&text))
    }
}

fn is_virtual(guid: &GUID) -> bool {
    same(guid, &INSTANCE) || same(guid, &device::product())
}

static VTBL: DirectInputVtbl = DirectInputVtbl {
    base: UnknownVtbl {
        query_interface,
        add_ref,
        release,
    },
    create_device,
    enum_devices,
    get_device_status,
    run_control_panel,
    initialize,
    find_device,
    enum_devices_by_semantics,
    configure_devices,
};

/// # Safety
/// `this` is one of this module's wrappers.
unsafe fn wrapper<'a>(this: This) -> &'a Wrapper {
    // SAFETY: the caller's contract.
    unsafe { &Wrapped::get(this).value }
}

unsafe extern "system" fn query_interface(
    this: This,
    riid: *const GUID,
    out: *mut This,
) -> HRESULT {
    guard(hr::GENERIC, || {
        // SAFETY: a call on our object, as COM makes it.
        let code = unsafe {
            super::com::query::<DirectInputVtbl, Wrapper>(
                this,
                riid,
                out,
                &[IID_UNKNOWN, IID_DIRECTINPUT8A],
            )
        };
        if hr::ok(code) || riid.is_null() || out.is_null() {
            return code;
        }
        // Another interface (the wide one): the game's own.
        // SAFETY: as above; the inner object answers for itself.
        unsafe {
            let inner = &wrapper(this).inner;
            (inner.vtbl().base.query_interface)(inner.raw(), riid, out)
        }
    })
}

unsafe extern "system" fn add_ref(this: This) -> u32 {
    // SAFETY: a call on our object.
    guard(1, || unsafe { Wrapped::get(this) }.add_ref())
}

unsafe extern "system" fn release(this: This) -> u32 {
    // SAFETY: a call on our object; not used after its last release.
    guard(1, || unsafe { Wrapped::release(this) })
}

unsafe extern "system" fn create_device(
    this: This,
    guid: *const GUID,
    out: *mut This,
    outer: This,
) -> HRESULT {
    guard(hr::GENERIC, || {
        // SAFETY: a call on our object; the game's GUID and out pointer.
        unsafe {
            let w = wrapper(this);
            if guid.is_null() || out.is_null() {
                return hr::INVALIDPARAM;
            }
            if !is_virtual(&*guid) {
                return (w.inner.vtbl().create_device)(w.inner.raw(), guid, out, outer);
            }
            if !outer.is_null() {
                return hr::INVALIDPARAM;
            }
            log!("CreateDevice: the virtual wheel");
            *out = w.virtual_wheel();
            hr::OK
        }
    })
}

/// The game's callback and what to leave out, for [`each_device`].
struct Listing<'a> {
    callback: unsafe extern "system" fn(*mut DIDEVICEINSTANCEA, *mut c_void) -> BOOL,
    context: *mut c_void,
    wrapper: &'a Wrapper,
}

unsafe extern "system" fn each_device(
    instance: *mut DIDEVICEINSTANCEA,
    listing: *mut c_void,
) -> BOOL {
    guard(DIENUM_STOP as BOOL, || {
        // SAFETY: `listing` is the Listing of enum_devices, alive across
        // the inner call; `instance` is DirectInput's.
        unsafe {
            let listing = &*listing.cast::<Listing>();
            if instance
                .as_ref()
                .is_some_and(|i| listing.wrapper.hides(&i.guidInstance))
            {
                return DIENUM_CONTINUE as BOOL;
            }
            (listing.callback)(instance, listing.context)
        }
    })
}

unsafe extern "system" fn enum_devices(
    this: This,
    kind: u32,
    callback: LPDIENUMDEVICESCALLBACKA,
    context: *mut c_void,
    flags: u32,
) -> HRESULT {
    let Some(callback) = callback else {
        return hr::INVALIDPARAM;
    };
    guard(hr::GENERIC, || {
        // SAFETY: a call on our object.
        let w = unsafe { wrapper(this) };
        if matches!(
            kind,
            DI8DEVCLASS_ALL | DI8DEVCLASS_GAMECTRL | DI8DEVTYPE_DRIVING
        ) {
            let mut info = device::instance();
            // SAFETY: the game's callback with a structure that lives
            // across the call.
            if unsafe { callback(&mut info, context) } == DIENUM_STOP as BOOL {
                return hr::OK;
            }
        }
        let mut listing = Listing {
            callback,
            context,
            wrapper: w,
        };
        // SAFETY: the game's DirectInput with our filter in front of its
        // callback; `listing` outlives the call.
        unsafe {
            (w.inner.vtbl().enum_devices)(
                w.inner.raw(),
                kind,
                Some(each_device),
                (&raw mut listing).cast(),
                flags,
            )
        }
    })
}

unsafe extern "system" fn get_device_status(this: This, guid: *const GUID) -> HRESULT {
    guard(hr::GENERIC, || {
        // SAFETY: a call on our object; the game's GUID.
        unsafe {
            if guid.as_ref().is_some_and(is_virtual) {
                return hr::OK;
            }
            let w = wrapper(this);
            (w.inner.vtbl().get_device_status)(w.inner.raw(), guid)
        }
    })
}

unsafe extern "system" fn run_control_panel(this: This, window: HWND, flags: u32) -> HRESULT {
    // SAFETY: a call on our object, passed on.
    guard(hr::GENERIC, || unsafe {
        let w = wrapper(this);
        (w.inner.vtbl().run_control_panel)(w.inner.raw(), window, flags)
    })
}

unsafe extern "system" fn initialize(this: This, instance: HINSTANCE, version: u32) -> HRESULT {
    // SAFETY: a call on our object, passed on.
    guard(hr::GENERIC, || unsafe {
        let w = wrapper(this);
        (w.inner.vtbl().initialize)(w.inner.raw(), instance, version)
    })
}

unsafe extern "system" fn find_device(
    this: This,
    class: *const GUID,
    name: *const u8,
    out: *mut GUID,
) -> HRESULT {
    // SAFETY: a call on our object, passed on.
    guard(hr::GENERIC, || unsafe {
        let w = wrapper(this);
        (w.inner.vtbl().find_device)(w.inner.raw(), class, name, out)
    })
}

unsafe extern "system" fn enum_devices_by_semantics(
    this: This,
    user: *const u8,
    format: *mut c_void,
    callback: *mut c_void,
    context: *mut c_void,
    flags: u32,
) -> HRESULT {
    // SAFETY: a call on our object, passed on (action maps don't know the
    // virtual wheel).
    guard(hr::GENERIC, || unsafe {
        let w = wrapper(this);
        (w.inner.vtbl().enum_devices_by_semantics)(
            w.inner.raw(),
            user,
            format,
            callback,
            context,
            flags,
        )
    })
}

unsafe extern "system" fn configure_devices(
    this: This,
    callback: *mut c_void,
    params: *mut c_void,
    flags: u32,
    data: *mut c_void,
) -> HRESULT {
    // SAFETY: a call on our object, passed on.
    guard(hr::GENERIC, || unsafe {
        let w = wrapper(this);
        (w.inner.vtbl().configure_devices)(w.inner.raw(), callback, params, flags, data)
    })
}
