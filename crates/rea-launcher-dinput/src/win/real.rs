//! The player's real devices, through the DirectInput the game would have
//! had (the system's, or Xidi's): listed when the game starts, then
//! opened, read and given force feedback for the virtual wheel.
//!
//! They are read the way the launcher reads them (`c_dfDIJoystick2`'s
//! axes in [`REAL_AXES`] order at [`REAL_RANGE`]), so the calibration it
//! saved means the same here.
//!
//! [`REAL_AXES`]: rea_launcher::wheel::dinput::REAL_AXES

use std::ffi::c_void;
use std::ptr::null_mut;
use std::time::{Duration, Instant};

use rea_launcher::wheel::DeviceState;
use rea_launcher::wheel::dinput::REAL_RANGE;
use rea_launcher::wheel::rig::{self, Device};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    DI8DEVCLASS_GAMECTRL, DIDATAFORMAT, DIDC_FORCEFEEDBACK, DIDEVCAPS, DIDEVICEINSTANCEA,
    DIDF_ABSAXIS, DIEDFL_ATTACHEDONLY, DIENUM_CONTINUE, DIJOYSTATE2, DIOBJECTDATAFORMAT,
    DIPH_DEVICE, DIPROPDWORD, DIPROPHEADER, DIPROPRANGE, DISCL_BACKGROUND, DISCL_EXCLUSIVE,
    DISCL_NONEXCLUSIVE, GUID_Button, GUID_POV, GUID_RxAxis, GUID_RyAxis, GUID_RzAxis, GUID_Slider,
    GUID_XAxis, GUID_YAxis, GUID_ZAxis,
};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::core::{BOOL, GUID, HRESULT};

use super::com::{Com, DeviceVtbl, DirectInputVtbl, EffectVtbl, This};
use super::{effect, guid_text, hr, log};
use crate::effect::Effect;
use crate::format::{self, Guid};

/// A game controller attached when the game started.
pub(super) struct Attached {
    pub device: Device,
    pub instance: GUID,
    /// A test-only fake ([`super::fake`]) at this index.
    pub fake: Option<usize>,
}

/// A GUID for each [`Guid`] a format entry can name.
pub(super) fn guid_ptr(guid: Guid) -> *const GUID {
    match guid {
        Guid::XAxis => &GUID_XAxis,
        Guid::YAxis => &GUID_YAxis,
        Guid::ZAxis => &GUID_ZAxis,
        Guid::RxAxis => &GUID_RxAxis,
        Guid::RyAxis => &GUID_RyAxis,
        Guid::RzAxis => &GUID_RzAxis,
        Guid::Slider => &GUID_Slider,
        Guid::Button => &GUID_Button,
        Guid::Pov => &GUID_POV,
        Guid::Key | Guid::Other => std::ptr::null(),
    }
}

/// What `guid` (a format entry's `pguid`) names.
pub(super) fn guid_kind(guid: &GUID) -> Guid {
    use super::com::same;
    use windows_sys::Win32::Devices::HumanInterfaceDevice::GUID_Key;
    [
        (Guid::XAxis, &GUID_XAxis),
        (Guid::YAxis, &GUID_YAxis),
        (Guid::ZAxis, &GUID_ZAxis),
        (Guid::RxAxis, &GUID_RxAxis),
        (Guid::RyAxis, &GUID_RyAxis),
        (Guid::RzAxis, &GUID_RzAxis),
        (Guid::Slider, &GUID_Slider),
        (Guid::Button, &GUID_Button),
        (Guid::Key, &GUID_Key),
        (Guid::Pov, &GUID_POV),
    ]
    .into_iter()
    .find(|(_, g)| same(g, guid))
    .map_or(Guid::Other, |(kind, _)| kind)
}

/// `c_dfDIJoystick2`, built from [`format::joystate2`].
pub(super) struct Joystate2 {
    objects: Vec<DIOBJECTDATAFORMAT>,
}

impl Joystate2 {
    pub fn new() -> Joystate2 {
        let objects = format::joystate2()
            .iter()
            .map(|e| DIOBJECTDATAFORMAT {
                pguid: e.guid.map_or(std::ptr::null(), guid_ptr),
                dwOfs: e.offset,
                dwType: e.kind,
                dwFlags: e.flags,
            })
            .collect();
        Joystate2 { objects }
    }

    /// Valid while `self` lives.
    pub fn data_format(&self) -> DIDATAFORMAT {
        DIDATAFORMAT {
            dwSize: size_of::<DIDATAFORMAT>() as u32,
            dwObjSize: size_of::<DIOBJECTDATAFORMAT>() as u32,
            dwFlags: DIDF_ABSAXIS,
            dwDataSize: size_of::<DIJOYSTATE2>() as u32,
            dwNumObjs: self.objects.len() as u32,
            rgodf: self.objects.as_ptr().cast_mut(),
        }
    }
}

pub(super) fn header<T>(how: u32, obj: u32) -> DIPROPHEADER {
    DIPROPHEADER {
        dwSize: size_of::<T>() as u32,
        dwHeaderSize: size_of::<DIPROPHEADER>() as u32,
        dwObj: obj,
        dwHow: how,
    }
}

/// `DIPROP_*` are small integers passed as GUID pointers.
pub(super) fn prop(id: usize) -> *const GUID {
    id as *const GUID
}

pub(super) const PROP_RANGE: usize = 4;
pub(super) const PROP_FFGAIN: usize = 7;
pub(super) const PROP_AUTOCENTER: usize = 9;

/// The capabilities of the device `guid` (opened and closed again).
///
/// # Safety
/// `di` is a live `IDirectInput8A`.
unsafe fn capabilities(di: &Com<DirectInputVtbl>, guid: &GUID) -> Option<DIDEVCAPS> {
    let mut ptr = null_mut();
    // SAFETY: a live DirectInput; the out pointer takes one reference,
    // which `Com` releases.
    let device: Com<DeviceVtbl> = unsafe {
        if !hr::ok((di.vtbl().create_device)(
            di.raw(),
            guid,
            &mut ptr,
            null_mut(),
        )) {
            return None;
        }
        Com::from_raw(ptr)?
    };
    let mut caps = DIDEVCAPS {
        dwSize: size_of::<DIDEVCAPS>() as u32,
        ..DIDEVCAPS::default()
    };
    // SAFETY: a live device and a sized structure.
    hr::ok(unsafe { (device.vtbl().get_capabilities)(device.raw(), &mut caps) }).then_some(caps)
}

/// The game controllers attached now, as the launcher lists them.
///
/// # Safety
/// `di` is a live `IDirectInput8A`.
pub(super) unsafe fn attached(di: &Com<DirectInputVtbl>) -> Vec<Attached> {
    unsafe extern "system" fn collect(instance: *mut DIDEVICEINSTANCEA, list: *mut c_void) -> BOOL {
        // SAFETY: `list` is the Vec below; DirectInput passes a valid
        // instance for the call.
        unsafe { (*list.cast::<Vec<DIDEVICEINSTANCEA>>()).push(*instance) };
        DIENUM_CONTINUE as BOOL
    }
    let mut found: Vec<DIDEVICEINSTANCEA> = Vec::new();
    // SAFETY: a live DirectInput; the callback only pushes to `found`.
    let code = unsafe {
        (di.vtbl().enum_devices)(
            di.raw(),
            DI8DEVCLASS_GAMECTRL,
            Some(collect),
            (&raw mut found).cast(),
            DIEDFL_ATTACHEDONLY,
        )
    };
    if !hr::ok(code) {
        log!("listing the controllers failed ({code:#x})");
    }
    found
        .iter()
        .filter_map(|instance| {
            // SAFETY: the caller's contract.
            let caps = unsafe { capabilities(di, &instance.guidInstance) }?;
            let product = &instance.guidProduct;
            let (vid, pid) = rig::product_ids(product.data1, product.data4);
            let axes = caps.dwAxes as usize;
            let device = Device {
                id: rea_launcher::wheel::DeviceId {
                    name: super::from_ansi(&instance.tszProductName),
                    vid,
                    pid,
                    guid: guid_text(&instance.guidInstance),
                },
                kind: rig::kind(vid, pid, instance.dwDevType, axes),
                axes,
                buttons: (caps.dwButtons as usize).min(128),
                ffb: caps.dwFlags & DIDC_FORCEFEEDBACK != 0,
                initial: DeviceState::default(),
                state: DeviceState::default(),
            };
            Some(Attached {
                device,
                instance: instance.guidInstance,
                fake: None,
            })
        })
        .collect()
}

/// How often a lost device is acquired again.
const RETRY: Duration = Duration::from_millis(500);

/// What a reading found.
pub(super) enum Read {
    State(DeviceState, u32),
    /// Lost or not acquired (unplugged, or another program took it).
    Lost,
    /// Acquired again after it was lost: the drivers dropped its effects.
    Back(DeviceState, u32),
}

/// An opened real device.
pub(super) struct Real {
    com: Com<DeviceVtbl>,
    pub device: Device,
    window: usize,
    exclusive: bool,
    lost: bool,
    /// Acquired again outside [`Real::read`] (a property that needed it):
    /// the next reading says [`Read::Back`].
    regained: bool,
    retry_at: Instant,
}

impl Real {
    /// `com` (a new device) set up to read `c_dfDIJoystick2` at the real
    /// range.
    pub fn open(com: Com<DeviceVtbl>, device: Device) -> Option<Real> {
        let format = Joystate2::new();
        let df = format.data_format();
        let range = DIPROPRANGE {
            diph: header::<DIPROPRANGE>(DIPH_DEVICE, 0),
            lMin: REAL_RANGE.0,
            lMax: REAL_RANGE.1,
        };
        // SAFETY: a live device; the format and range live across the
        // calls. A device without axes refuses the range: kept.
        unsafe {
            let code = (com.vtbl().set_data_format)(com.raw(), &df);
            if !hr::ok(code) {
                log!(
                    "{}: the data format was refused ({code:#x})",
                    device.id.name
                );
                return None;
            }
            (com.vtbl().set_property)(com.raw(), prop(PROP_RANGE), &range.diph);
        }
        Some(Real {
            com,
            device,
            window: 0,
            exclusive: false,
            lost: true,
            regained: false,
            retry_at: Instant::now(),
        })
    }

    /// Exclusive and in the background on `hwnd` (the game's window) for
    /// force feedback, else shared in the background.
    pub fn cooperate(&mut self, hwnd: HWND) {
        // SAFETY: a live device.
        unsafe { (self.com.vtbl().unacquire)(self.com.raw()) };
        self.window = hwnd as usize;
        self.exclusive = self.device.ffb && !hwnd.is_null() && self.set_level(true);
        if !self.exclusive {
            self.set_level(false);
        }
        self.lost = true;
        log!(
            "{}: {} in the background",
            self.device.id.name,
            if self.exclusive {
                "exclusive"
            } else {
                "shared"
            }
        );
    }

    /// The cooperative level on the game's window, in the background.
    fn set_level(&self, exclusive: bool) -> bool {
        let flags = (if exclusive {
            DISCL_EXCLUSIVE
        } else {
            DISCL_NONEXCLUSIVE
        }) | DISCL_BACKGROUND;
        // SAFETY: a live device; DirectInput checks the window.
        hr::ok(unsafe {
            (self.com.vtbl().set_cooperative_level)(self.com.raw(), self.window as HWND, flags)
        })
    }

    /// Acquires the device. When another program holds the wheel
    /// exclusively it is acquired shared instead: it steers, without force
    /// feedback.
    pub fn acquire(&mut self) -> bool {
        let try_acquire = |real: &Real| {
            // SAFETY: a live device.
            unsafe { (real.com.vtbl().acquire)(real.com.raw()) }
        };
        let mut code = try_acquire(self);
        if !hr::ok(code) && self.exclusive && self.set_level(false) {
            code = try_acquire(self);
            if hr::ok(code) {
                self.exclusive = false;
                log!(
                    "{}: taken by another program: shared, no force feedback",
                    self.device.id.name
                );
            } else {
                self.set_level(true);
            }
        }
        self.lost = !hr::ok(code);
        self.retry_at = Instant::now() + RETRY;
        if self.lost {
            log!("{}: not acquired ({code:#x})", self.device.id.name);
        }
        !self.lost
    }

    pub fn unacquire(&mut self) {
        // SAFETY: a live device.
        unsafe { (self.com.vtbl().unacquire)(self.com.raw()) };
        self.lost = true;
    }

    /// Polls and reads the device; a lost one is acquired again at most
    /// every [`RETRY`].
    pub fn read(&mut self, now: Instant) -> Read {
        let mut back = std::mem::take(&mut self.regained);
        if self.lost {
            if now < self.retry_at || !self.acquire() {
                return Read::Lost;
            }
            back = true;
        }
        // SAFETY: a zeroed DIJOYSTATE2 is valid.
        let mut state: DIJOYSTATE2 = unsafe { std::mem::zeroed() };
        // SAFETY: a live device whose format is DIJOYSTATE2-sized.
        let code = unsafe {
            (self.com.vtbl().poll)(self.com.raw());
            (self.com.vtbl().get_device_state)(
                self.com.raw(),
                size_of::<DIJOYSTATE2>() as u32,
                (&raw mut state).cast(),
            )
        };
        if !hr::ok(code) {
            log!("{}: lost ({code:#x})", self.device.id.name);
            self.lost = true;
            self.retry_at = now + RETRY;
            return Read::Lost;
        }
        // SAFETY: a DIJOYSTATE2 is plain data; its bytes are read only.
        let bytes = unsafe {
            std::slice::from_raw_parts((&raw const state).cast::<u8>(), size_of::<DIJOYSTATE2>())
        };
        let Some((reading, pov)) = rig::joystate2(bytes, self.device.buttons) else {
            return Read::Lost;
        };
        if back {
            Read::Back(reading, pov)
        } else {
            Read::State(reading, pov)
        }
    }

    /// Sets a `DIPROPDWORD` device property; one that can't change while
    /// the device is acquired is set between an unacquire and an acquire.
    pub fn set_dword(&mut self, id: usize, value: u32) -> HRESULT {
        let data = DIPROPDWORD {
            diph: header::<DIPROPDWORD>(DIPH_DEVICE, 0),
            dwData: value,
        };
        let set = |real: &Real| {
            // SAFETY: a live device; `data` lives across the call.
            unsafe { (real.com.vtbl().set_property)(real.com.raw(), prop(id), &data.diph) }
        };
        let mut code = set(self);
        if code == hr::ACQUIRED {
            self.unacquire();
            code = set(self);
            self.regained = self.acquire();
        }
        code
    }

    /// Its settings and effects are back on it: a re-acquire for a
    /// property since then is no news.
    pub fn settled(&mut self) {
        self.regained = false;
    }

    pub fn exclusive(&self) -> bool {
        self.exclusive
    }

    pub fn command(&self, command: u32) -> HRESULT {
        // SAFETY: a live device.
        unsafe { (self.com.vtbl().send_force_feedback_command)(self.com.raw(), command) }
    }

    /// `DIGFFS_*`, when the device answers.
    pub fn ffb_state(&self) -> Option<u32> {
        let mut state = 0;
        // SAFETY: a live device and a local out value.
        let code =
            unsafe { (self.com.vtbl().get_force_feedback_state)(self.com.raw(), &mut state) };
        hr::ok(code).then_some(state)
    }

    /// A real effect for `made` (a wheel effect).
    pub fn create_effect(&self, made: &Effect) -> Result<RealEffect, HRESULT> {
        let guid = effect::guid_of(made.kind);
        let mut ptr: This = null_mut();
        // SAFETY: a live device; the DIEFFECT and the GUID live across the
        // call; the out pointer takes one reference.
        let code = effect::with_raw(made, |raw| unsafe {
            (self.com.vtbl().create_effect)(self.com.raw(), &guid, raw, &mut ptr, null_mut())
        });
        if !hr::ok(code) {
            return Err(code);
        }
        // SAFETY: an effect DirectInput just made.
        unsafe { Com::from_raw(ptr) }
            .map(|com| RealEffect { com })
            .ok_or(hr::GENERIC)
    }
}

/// A real effect on the wheel.
pub(super) struct RealEffect {
    com: Com<EffectVtbl>,
}

impl RealEffect {
    pub fn set(&self, made: &Effect, flags: u32) -> HRESULT {
        // SAFETY: a live effect; the DIEFFECT lives across the call.
        effect::with_raw(made, |raw| unsafe {
            (self.com.vtbl().set_parameters)(self.com.raw(), raw, flags)
        })
    }

    pub fn start(&self, iterations: u32, flags: u32) -> HRESULT {
        // SAFETY: a live effect.
        let code = unsafe { (self.com.vtbl().start)(self.com.raw(), iterations, flags) };
        if code == hr::NOTDOWNLOADED {
            self.download();
            // SAFETY: as above.
            return unsafe { (self.com.vtbl().start)(self.com.raw(), iterations, flags) };
        }
        code
    }

    pub fn stop(&self) -> HRESULT {
        // SAFETY: a live effect.
        unsafe { (self.com.vtbl().stop)(self.com.raw()) }
    }

    /// `DIEGES_*`, when the device answers.
    pub fn status(&self) -> Option<u32> {
        let mut status = 0;
        // SAFETY: a live effect and a local out value.
        let code = unsafe { (self.com.vtbl().get_effect_status)(self.com.raw(), &mut status) };
        hr::ok(code).then_some(status)
    }

    pub fn download(&self) -> HRESULT {
        // SAFETY: a live effect.
        unsafe { (self.com.vtbl().download)(self.com.raw()) }
    }

    pub fn unload(&self) -> HRESULT {
        // SAFETY: a live effect.
        unsafe { (self.com.vtbl().unload)(self.com.raw()) }
    }
}
