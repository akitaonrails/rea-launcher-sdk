//! The virtual wheel: an `IDirectInputDevice8A` with the frozen layout of
//! [`VIRTUAL`] whose state comes from the real wheel and pedals.
//!
//! Each device the game makes is a [`View`] (its data format, axis
//! properties, buffer) over the one [`Rig`](super::rig::Rig) of its DirectInput (the real
//! devices and effects). Locks are always taken view first, then rig.
//! Game callbacks (`EnumObjects`, `EnumEffects`, …) run with both
//! released, so a callback may call the device again.

use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use rea_launcher::wheel::dinput::{Axis, VIRTUAL};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    DI8DEVTYPE_DRIVING, DI8DEVTYPEDRIVING_DUALPEDALS, DIDATAFORMAT, DIDC_ATTACHED, DIDC_DEADBAND,
    DIDC_FFATTACK, DIDC_FFFADE, DIDC_FORCEFEEDBACK, DIDC_POLLEDDEVICE, DIDC_POSNEGCOEFFICIENTS,
    DIDC_POSNEGSATURATION, DIDC_SATURATION, DIDC_STARTDELAY, DIDEVCAPS, DIDEVICEINSTANCEA,
    DIDEVICEOBJECTDATA, DIDEVICEOBJECTINSTANCEA, DIDEVTYPE_HID, DIEFFECT, DIEFFECTINFOA,
    DIEFFESCAPE, DIEFT_DEADBAND, DIEFT_FFATTACK, DIEFT_FFFADE, DIEFT_POSNEGCOEFFICIENTS,
    DIEFT_POSNEGSATURATION, DIEFT_SATURATION, DIEFT_STARTDELAY, DIENUM_STOP, DIEP_ALLPARAMS,
    DIEP_ALLPARAMS_DX5, DIGDD_PEEK, DIOBJECTDATAFORMAT, DIPH_BYID, DIPH_BYOFFSET, DIPH_BYUSAGE,
    DIPH_DEVICE, DIPROPDWORD, DIPROPHEADER, DIPROPRANGE, DIPROPSTRING, DISCL_BACKGROUND,
    DISCL_EXCLUSIVE, DISCL_FOREGROUND, DISCL_NONEXCLUSIVE, LPDIENUMCREATEDEFFECTOBJECTSCALLBACK,
    LPDIENUMDEVICEOBJECTSCALLBACKA, LPDIENUMEFFECTSCALLBACKA,
};
use windows_sys::Win32::Foundation::{HANDLE, HINSTANCE, HWND};
use windows_sys::Win32::System::SystemInformation::GetTickCount;
use windows_sys::Win32::System::Threading::SetEvent;
use windows_sys::Win32::UI::WindowsAndMessaging::{GA_ROOT, GetAncestor};
use windows_sys::core::{GUID, HRESULT};

use super::com::{DeviceVtbl, IID_DEVICE8A, IID_UNKNOWN, Object, This, UnknownVtbl, query};
use super::effect;
use super::real::{guid_kind, guid_ptr};
use super::rig::Shared;
use super::{ansi, guard, hr, lock, log};
use crate::effect::{Effect, Family, Kind};
use crate::format::{Entry, Layout, Object as Part, Values};
use crate::state::{AxisProps, FULL};

/// The virtual wheel's instance GUID: fixed, so a game that saves it
/// finds the wheel again.
pub(super) const INSTANCE: GUID = GUID::from_u128(0x7265615f_7768_6565_6c00_000000000001);

/// Its product GUID, as DirectInput makes them for USB devices:
/// `Data1` = PID << 16 | VID, then "PIDVID". The CMR games read the
/// VID and PID from it.
pub(super) fn product() -> GUID {
    GUID {
        data1: (u32::from(VIRTUAL.pid) << 16) | u32::from(VIRTUAL.vid),
        data2: 0,
        data3: 0,
        data4: *b"\0\0PIDVID",
    }
}

const DEV_TYPE: u32 = DI8DEVTYPE_DRIVING | (DI8DEVTYPEDRIVING_DUALPEDALS << 8) | DIDEVTYPE_HID;

/// The virtual wheel's `DIDEVICEINSTANCEA`, as `EnumDevices` and
/// `GetDeviceInfo` give it.
pub(super) fn instance() -> DIDEVICEINSTANCEA {
    let mut info = DIDEVICEINSTANCEA {
        dwSize: size_of::<DIDEVICEINSTANCEA>() as u32,
        guidInstance: INSTANCE,
        guidProduct: product(),
        dwDevType: DEV_TYPE,
        wUsagePage: 1,
        wUsage: 4,
        ..DIDEVICEINSTANCEA::default()
    };
    ansi(VIRTUAL.name, &mut info.tszInstanceName);
    ansi(VIRTUAL.name, &mut info.tszProductName);
    info
}

/// `DIDEVICEINSTANCEA` before DirectX 5 (no FF driver or usage).
const INSTANCE_DX3_SIZE: u32 = 560;
/// `DIDEVCAPS` before DirectX 5.
const CAPS_DX3_SIZE: u32 = 24;
/// `DIDEVICEOBJECTINSTANCEA` before DirectX 5.
const OBJECT_DX3_SIZE: u32 = 292;
/// `DIDEVICEOBJECTDATA` before DirectX 8 (no `uAppData`).
const OBJECT_DATA_DX7_SIZE: u32 = 16;

/// `DIPROP_*` the device answers.
mod prop {
    pub const BUFFERSIZE: usize = 1;
    pub const AXISMODE: usize = 2;
    pub const GRANULARITY: usize = 3;
    pub const RANGE: usize = 4;
    pub const DEADZONE: usize = 5;
    pub const SATURATION: usize = 6;
    pub const FFGAIN: usize = 7;
    pub const AUTOCENTER: usize = 9;
    pub const CALIBRATIONMODE: usize = 10;
    pub const INSTANCENAME: usize = 13;
    pub const PRODUCTNAME: usize = 14;
    pub const VIDPID: usize = 24;
}

/// What a game read with buffered data (`DIPROP_BUFFERSIZE`).
#[derive(Default)]
struct Buffer {
    size: u32,
    events: VecDeque<DIDEVICEOBJECTDATA>,
    overflow: bool,
    sequence: u32,
}

impl Buffer {
    fn push(&mut self, offset: u32, data: u32) {
        if self.size == 0 {
            return;
        }
        if self.events.len() >= self.size as usize {
            self.events.pop_front();
            self.overflow = true;
        }
        self.sequence = self.sequence.wrapping_add(1);
        self.events.push_back(DIDEVICEOBJECTDATA {
            dwOfs: offset,
            dwData: data,
            // SAFETY: a plain system query.
            dwTimeStamp: unsafe { GetTickCount() },
            dwSequence: self.sequence,
            uAppData: usize::MAX,
        });
    }
}

/// One virtual device the game made: its own settings of the wheel,
/// over the rig it shares.
pub(super) struct View {
    rig: Shared,
    layout: Option<Layout>,
    props: [AxisProps; 4],
    buffer: Buffer,
    acquired: bool,
    event: usize,
    values: Option<Values>,
}

impl View {
    fn new(rig: Shared) -> View {
        View {
            rig,
            layout: None,
            props: [AxisProps::default(); 4],
            buffer: Buffer::default(),
            acquired: false,
            event: 0,
            values: None,
        }
    }

    pub fn rig(&self) -> Shared {
        Arc::clone(&self.rig)
    }

    /// An axis named in an effect: by object ID or by its offset in the
    /// game's data format (`c_dfDIJoystick2`'s before it set one).
    pub fn axis_named(&self, by_id: bool, value: u32) -> Option<Axis> {
        let part = if by_id {
            Part::by_id(value)
        } else {
            self.object_at(value)
        };
        match part {
            Some(Part::Axis(axis)) => Some(axis),
            _ => None,
        }
    }

    /// An axis as an effect's `rgdwAxes` gives it back.
    pub fn axis_value(&self, by_id: bool, axis: Option<Axis>) -> u32 {
        let Some(axis) = axis else { return 0 };
        let part = Part::Axis(axis);
        if by_id {
            part.type_id()
        } else {
            self.offset_of(part)
        }
    }

    fn object_at(&self, offset: u32) -> Option<Part> {
        match &self.layout {
            Some(layout) => layout.object_at(offset),
            None => Part::at_joystate2(offset),
        }
    }

    fn offset_of(&self, part: Part) -> u32 {
        self.layout
            .as_ref()
            .map_or(Some(part.joystate2_offset()), |l| l.offset_of(part))
            .unwrap_or(u32::MAX)
    }

    /// Reads the real devices into the virtual wheel's values.
    fn update(&mut self, now: Instant) {
        let values = lock(&self.rig).read(now).values(&self.props);
        if let (Some(old), Some(layout)) = (self.values, &self.layout)
            && old != values
        {
            for (offset, data) in layout.changes(&old, &values) {
                self.buffer.push(offset, data);
            }
            if self.event != 0 {
                // SAFETY: the game's event handle (SetEventNotification).
                unsafe { SetEvent(self.event as HANDLE) };
            }
        }
        self.values = Some(values);
    }

    fn acquire(&mut self) -> HRESULT {
        if self.layout.is_none() {
            return hr::INVALIDPARAM;
        }
        if self.acquired {
            return hr::FALSE;
        }
        lock(&self.rig).acquire();
        self.acquired = true;
        self.values = None;
        self.update(Instant::now());
        hr::OK
    }

    fn unacquire(&mut self) -> HRESULT {
        if !self.acquired {
            return hr::FALSE;
        }
        self.acquired = false;
        lock(&self.rig).unacquire();
        hr::OK
    }

    /// What a property header addresses: the device (`None`) or one of
    /// its objects.
    fn target(&self, how: u32, obj: u32) -> Result<Option<Part>, HRESULT> {
        match how {
            DIPH_DEVICE if obj == 0 => Ok(None),
            DIPH_DEVICE => Err(hr::INVALIDPARAM),
            DIPH_BYOFFSET => self
                .layout
                .as_ref()
                .ok_or(hr::NOTFOUND)?
                .object_at(obj)
                .map(Some)
                .ok_or(hr::NOTFOUND),
            DIPH_BYID => Part::by_id(obj).map(Some).ok_or(hr::NOTFOUND),
            DIPH_BYUSAGE => Part::by_usage((obj >> 16) as u16, obj as u16)
                .map(Some)
                .ok_or(hr::NOTFOUND),
            _ => Err(hr::INVALIDPARAM),
        }
    }

    /// The axes a property applies to: the one addressed, or all of them
    /// for the device.
    fn axes_of(&self, target: Option<Part>) -> Result<Vec<usize>, HRESULT> {
        match target {
            None => Ok((0..4).collect()),
            Some(Part::Axis(a)) => Ok(vec![Axis::ALL.iter().position(|x| *x == a).unwrap_or(0)]),
            Some(_) => Err(hr::INVALIDPARAM),
        }
    }

    /// # Safety
    /// `header` heads a property structure of `header.dwSize` bytes.
    unsafe fn get_property(&mut self, id: usize, header: *mut DIPROPHEADER) -> HRESULT {
        // SAFETY: the caller's contract.
        let Some(h) = (unsafe { header.as_ref() }) else {
            return hr::INVALIDPARAM;
        };
        if h.dwHeaderSize != size_of::<DIPROPHEADER>() as u32 {
            return hr::INVALIDPARAM;
        }
        let target = match self.target(h.dwHow, h.dwObj) {
            Ok(target) => target,
            Err(code) => return code,
        };
        let dword = |value: u32| -> HRESULT {
            if h.dwSize != size_of::<DIPROPDWORD>() as u32 {
                return hr::INVALIDPARAM;
            }
            // SAFETY: a DIPROPDWORD (checked size, the caller's contract).
            unsafe { (*header.cast::<DIPROPDWORD>()).dwData = value };
            hr::OK
        };
        let axis = || self.axes_of(target).map(|a| self.props[a[0]]);
        match id {
            prop::RANGE => {
                let Ok(props) = axis() else {
                    return hr::INVALIDPARAM;
                };
                if h.dwSize != size_of::<DIPROPRANGE>() as u32 {
                    return hr::INVALIDPARAM;
                }
                // SAFETY: a DIPROPRANGE (checked size).
                unsafe {
                    let range = &mut *header.cast::<DIPROPRANGE>();
                    range.lMin = props.min;
                    range.lMax = props.max;
                }
                hr::OK
            }
            prop::DEADZONE => axis().map_or(hr::INVALIDPARAM, |p| dword(p.deadzone)),
            prop::SATURATION => axis().map_or(hr::INVALIDPARAM, |p| dword(p.saturation)),
            prop::GRANULARITY => dword(1),
            prop::AXISMODE | prop::CALIBRATIONMODE => dword(0),
            prop::BUFFERSIZE => dword(self.buffer.size),
            prop::FFGAIN => dword(lock(&self.rig).gain()),
            prop::AUTOCENTER => dword(lock(&self.rig).autocenter()),
            prop::VIDPID => dword((u32::from(VIRTUAL.pid) << 16) | u32::from(VIRTUAL.vid)),
            prop::PRODUCTNAME | prop::INSTANCENAME => {
                if h.dwSize != size_of::<DIPROPSTRING>() as u32 {
                    return hr::INVALIDPARAM;
                }
                // SAFETY: a DIPROPSTRING (checked size).
                let text = unsafe { &mut (*header.cast::<DIPROPSTRING>()).wsz };
                text.fill(0);
                for (dst, src) in text.iter_mut().zip(VIRTUAL.name.encode_utf16()) {
                    *dst = src;
                }
                hr::OK
            }
            _ => hr::UNSUPPORTED,
        }
    }

    /// # Safety
    /// `header` heads a property structure of `header.dwSize` bytes.
    unsafe fn set_property(&mut self, id: usize, header: *const DIPROPHEADER) -> HRESULT {
        // SAFETY: the caller's contract.
        let Some(h) = (unsafe { header.as_ref() }) else {
            return hr::INVALIDPARAM;
        };
        if h.dwHeaderSize != size_of::<DIPROPHEADER>() as u32 {
            return hr::INVALIDPARAM;
        }
        let target = match self.target(h.dwHow, h.dwObj) {
            Ok(target) => target,
            Err(code) => return code,
        };
        let dword = || -> Result<u32, HRESULT> {
            if h.dwSize != size_of::<DIPROPDWORD>() as u32 {
                return Err(hr::INVALIDPARAM);
            }
            // SAFETY: a DIPROPDWORD (checked size).
            Ok(unsafe { (*header.cast::<DIPROPDWORD>()).dwData })
        };
        let code = match id {
            prop::RANGE => {
                if h.dwSize != size_of::<DIPROPRANGE>() as u32 {
                    return hr::INVALIDPARAM;
                }
                // SAFETY: a DIPROPRANGE (checked size).
                let range = unsafe { &*header.cast::<DIPROPRANGE>() };
                if range.lMin >= range.lMax {
                    return hr::INVALIDPARAM;
                }
                self.each_axis(target, |p| {
                    p.min = range.lMin;
                    p.max = range.lMax;
                })
            }
            prop::DEADZONE | prop::SATURATION => match dword() {
                Ok(v) if v <= FULL => self.each_axis(target, |p| {
                    if id == prop::DEADZONE {
                        p.deadzone = v;
                    } else {
                        p.saturation = v;
                    }
                }),
                Ok(_) => hr::INVALIDPARAM,
                Err(code) => code,
            },
            prop::AXISMODE => match dword() {
                Ok(0) => hr::OK,
                Ok(_) => hr::UNSUPPORTED,
                Err(code) => code,
            },
            prop::CALIBRATIONMODE => dword().map_or_else(|c| c, |_| hr::OK),
            prop::BUFFERSIZE => dword().map_or_else(
                |c| c,
                |v| {
                    self.buffer.size = v;
                    self.buffer.events.truncate(v as usize);
                    hr::OK
                },
            ),
            prop::FFGAIN | prop::AUTOCENTER if target.is_some() => hr::INVALIDPARAM,
            prop::FFGAIN => match dword() {
                Ok(v) if v <= FULL => {
                    lock(&self.rig).set_gain(v);
                    hr::OK
                }
                Ok(_) => hr::INVALIDPARAM,
                Err(code) => code,
            },
            prop::AUTOCENTER => match dword() {
                Ok(v @ (0 | 1)) => {
                    lock(&self.rig).set_autocenter(v);
                    hr::OK
                }
                Ok(_) => hr::INVALIDPARAM,
                Err(code) => code,
            },
            _ => hr::UNSUPPORTED,
        };
        log!(
            "SetProperty {id} on {target:?} (how {}, obj {:#x}): {code:#x}",
            h.dwHow,
            h.dwObj
        );
        code
    }

    fn each_axis(&mut self, target: Option<Part>, mut f: impl FnMut(&mut AxisProps)) -> HRESULT {
        match self.axes_of(target) {
            Ok(axes) => {
                for a in axes {
                    f(&mut self.props[a]);
                }
                hr::OK
            }
            Err(code) => code,
        }
    }

    /// The game's `DIDATAFORMAT`.
    ///
    /// # Safety
    /// `format` is null or a `DIDATAFORMAT` whose objects are readable.
    unsafe fn set_data_format(&mut self, format: *const DIDATAFORMAT) -> HRESULT {
        // SAFETY: the caller's contract.
        let Some(df) = (unsafe { format.as_ref() }) else {
            return hr::INVALIDPARAM;
        };
        if self.acquired {
            return hr::ACQUIRED;
        }
        if df.dwSize != size_of::<DIDATAFORMAT>() as u32
            || df.dwObjSize != size_of::<DIOBJECTDATAFORMAT>() as u32
            || (df.dwNumObjs > 0 && df.rgodf.is_null())
        {
            return hr::INVALIDPARAM;
        }
        // SAFETY: dwNumObjs readable entries (the caller's contract).
        let objects = unsafe { std::slice::from_raw_parts(df.rgodf, df.dwNumObjs as usize) };
        let entries: Vec<Entry> = objects
            .iter()
            .map(|o| Entry {
                // SAFETY: a GUID the game points to, or null.
                guid: unsafe { o.pguid.as_ref() }.map(guid_kind),
                offset: o.dwOfs,
                kind: o.dwType,
                flags: o.dwFlags,
            })
            .collect();
        match Layout::translate(df.dwDataSize, &entries) {
            Ok(layout) => {
                log!(
                    "SetDataFormat: {} bytes, {} entries: {:?}",
                    df.dwDataSize,
                    entries.len(),
                    layout.slots()
                );
                self.layout = Some(layout);
                self.values = None;
                hr::OK
            }
            Err(error) => {
                log!("SetDataFormat refused: {error:?}");
                hr::INVALIDPARAM
            }
        }
    }

    fn object_instance(&self, part: Part) -> DIDEVICEOBJECTINSTANCEA {
        let (page, usage) = part.usage();
        let mut info = DIDEVICEOBJECTINSTANCEA {
            dwSize: size_of::<DIDEVICEOBJECTINSTANCEA>() as u32,
            // SAFETY: the GUIDs `guid_ptr` points to are statics; every
            // part of the wheel has one.
            guidType: unsafe { *guid_ptr(part.guid()) },
            dwOfs: self.offset_of(part),
            dwType: part.type_id(),
            dwFlags: part.flags(),
            wUsagePage: page,
            wUsage: usage,
            ..DIDEVICEOBJECTINSTANCEA::default()
        };
        if part == Part::Axis(Axis::X) {
            info.dwFFMaxForce = 10_000;
            info.dwFFForceResolution = 1;
        }
        ansi(&part.name(), &mut info.tszName);
        info
    }
}

/// The device object: its view.
pub(super) struct VirtualDevice {
    view: Arc<Mutex<View>>,
}

type DeviceObject = Object<DeviceVtbl, VirtualDevice>;

/// A new virtual wheel for the game, on `rig`.
pub(super) fn create(rig: Shared) -> This {
    DeviceObject::create(
        &VTBL,
        VirtualDevice {
            view: Arc::new(Mutex::new(View::new(rig))),
        },
    )
}

static VTBL: DeviceVtbl = DeviceVtbl {
    base: UnknownVtbl {
        query_interface,
        add_ref,
        release,
    },
    get_capabilities,
    enum_objects,
    get_property,
    set_property,
    acquire,
    unacquire,
    get_device_state,
    get_device_data,
    set_data_format,
    set_event_notification,
    set_cooperative_level,
    get_object_info,
    get_device_info,
    run_control_panel,
    initialize,
    create_effect,
    enum_effects,
    get_effect_info,
    get_force_feedback_state,
    send_force_feedback_command,
    enum_created_effect_objects,
    escape,
    poll,
    send_device_data,
    enum_effects_in_file,
    write_effect_to_file,
    build_action_map,
    set_action_map,
    get_image_info,
};

/// The device object behind `this`.
///
/// # Safety
/// `this` is one of this module's device objects.
unsafe fn device<'a>(this: This) -> &'a VirtualDevice {
    // SAFETY: the caller's contract.
    unsafe { &DeviceObject::get(this).value }
}

/// Runs `f` on the device's view, inside [`guard`].
///
/// # Safety
/// As [`device`].
unsafe fn with<R>(this: This, failed: R, f: impl FnOnce(&mut View) -> R) -> R {
    // SAFETY: the caller's contract.
    guard(failed, || f(&mut lock(&unsafe { device(this) }.view)))
}

/// Copies `full` into the game's structure of `size` bytes (an older
/// DirectX's smaller one), keeping its `dwSize`.
///
/// # Safety
/// `dst` is writable for `size` bytes; `size` is at most `T`'s.
unsafe fn copy_sized<T>(full: &T, dst: *mut T, size: u32) {
    // SAFETY: the caller's contract; the first four bytes, dwSize, stay.
    unsafe {
        std::ptr::copy_nonoverlapping(
            (full as *const T).cast::<u8>().add(4),
            dst.cast::<u8>().add(4),
            size as usize - 4,
        );
    }
}

unsafe extern "system" fn query_interface(
    this: This,
    riid: *const GUID,
    out: *mut This,
) -> HRESULT {
    // SAFETY: a call on our object, as COM makes it.
    guard(hr::GENERIC, || unsafe {
        query::<DeviceVtbl, VirtualDevice>(this, riid, out, &[IID_UNKNOWN, IID_DEVICE8A])
    })
}

unsafe extern "system" fn add_ref(this: This) -> u32 {
    // SAFETY: a call on our object.
    guard(1, || unsafe { DeviceObject::get(this) }.add_ref())
}

unsafe extern "system" fn release(this: This) -> u32 {
    guard(1, || {
        // SAFETY: a call on our object; its view is taken before the last
        // release frees it.
        let view = unsafe { Arc::clone(&device(this).view) };
        // SAFETY: a call on our object, not used after its last release.
        let left = unsafe { DeviceObject::release(this) };
        if left == 0 {
            // Released while acquired: its hold on the wheel goes too.
            lock(&view).unacquire();
        }
        left
    })
}

unsafe extern "system" fn get_capabilities(this: This, caps: *mut DIDEVCAPS) -> HRESULT {
    // SAFETY: a call on our object; `caps` is the game's or null.
    unsafe {
        with(this, hr::GENERIC, |view| {
            let Some(size) = caps.as_ref().map(|c| c.dwSize) else {
                return hr::INVALIDPARAM;
            };
            if size != size_of::<DIDEVCAPS>() as u32 && size != CAPS_DX3_SIZE {
                return hr::INVALIDPARAM;
            }
            // Force feedback only through a wheel that takes it.
            let ffb = if lock(&view.rig).has_ffb() {
                DIDC_FORCEFEEDBACK
                    | DIDC_FFATTACK
                    | DIDC_FFFADE
                    | DIDC_SATURATION
                    | DIDC_POSNEGCOEFFICIENTS
                    | DIDC_POSNEGSATURATION
                    | DIDC_DEADBAND
                    | DIDC_STARTDELAY
            } else {
                0
            };
            let full = DIDEVCAPS {
                dwSize: size,
                dwFlags: DIDC_ATTACHED | DIDC_POLLEDDEVICE | ffb,
                dwDevType: DEV_TYPE,
                dwAxes: 4,
                dwButtons: VIRTUAL.buttons as u32,
                dwPOVs: 1,
                dwFFSamplePeriod: 1_000,
                dwFFMinTimeResolution: 1_000,
                dwFirmwareRevision: 1,
                dwHardwareRevision: 1,
                dwFFDriverVersion: 1,
            };
            copy_sized(&full, caps, size);
            hr::OK
        })
    }
}

unsafe extern "system" fn enum_objects(
    this: This,
    callback: LPDIENUMDEVICEOBJECTSCALLBACKA,
    context: *mut c_void,
    filter: u32,
) -> HRESULT {
    // SAFETY: a call on our object.
    let objects = unsafe {
        with(this, None, |view| {
            Some(
                Part::all()
                    .filter(|p| p.in_filter(filter))
                    .map(|p| view.object_instance(p))
                    .collect::<Vec<_>>(),
            )
        })
    };
    let (Some(callback), Some(mut objects)) = (callback, objects) else {
        return hr::INVALIDPARAM;
    };
    guard(hr::GENERIC, || {
        for object in &mut objects {
            // SAFETY: the game's callback, with an object that lives
            // across the call (the rig is unlocked: it may call back).
            if unsafe { callback(object, context) } == DIENUM_STOP as i32 {
                break;
            }
        }
        hr::OK
    })
}

unsafe extern "system" fn get_property(
    this: This,
    id: *const GUID,
    header: *mut DIPROPHEADER,
) -> HRESULT {
    // SAFETY: a call on our object; the property as the game passes it.
    unsafe {
        with(this, hr::GENERIC, |view| {
            view.get_property(id as usize, header)
        })
    }
}

unsafe extern "system" fn set_property(
    this: This,
    id: *const GUID,
    header: *const DIPROPHEADER,
) -> HRESULT {
    // SAFETY: a call on our object; the property as the game passes it.
    unsafe {
        with(this, hr::GENERIC, |view| {
            view.set_property(id as usize, header)
        })
    }
}

unsafe extern "system" fn acquire(this: This) -> HRESULT {
    // SAFETY: a call on our object.
    unsafe {
        with(this, hr::GENERIC, |view| {
            let code = view.acquire();
            log!("Acquire: {code:#x}");
            code
        })
    }
}

unsafe extern "system" fn unacquire(this: This) -> HRESULT {
    // SAFETY: a call on our object.
    unsafe { with(this, hr::GENERIC, View::unacquire) }
}

unsafe extern "system" fn get_device_state(this: This, size: u32, data: *mut c_void) -> HRESULT {
    // SAFETY: a call on our object; `data` holds `size` bytes.
    unsafe {
        with(this, hr::GENERIC, |view| {
            if !view.acquired {
                return hr::NOTACQUIRED;
            }
            let Some(layout) = &view.layout else {
                return hr::INVALIDPARAM;
            };
            if data.is_null() || size != layout.size {
                return hr::INVALIDPARAM;
            }
            view.update(Instant::now());
            let (Some(layout), Some(values)) = (&view.layout, &view.values) else {
                return hr::GENERIC;
            };
            layout.write(
                values,
                std::slice::from_raw_parts_mut(data.cast::<u8>(), size as usize),
            );
            hr::OK
        })
    }
}

unsafe extern "system" fn get_device_data(
    this: This,
    object_size: u32,
    data: *mut DIDEVICEOBJECTDATA,
    count: *mut u32,
    flags: u32,
) -> HRESULT {
    // SAFETY: a call on our object; `data` holds `*count` entries of
    // `object_size` bytes, or is null.
    unsafe {
        with(this, hr::GENERIC, |view| {
            if view.buffer.size == 0 {
                return hr::NOTBUFFERED;
            }
            if !view.acquired {
                return hr::NOTACQUIRED;
            }
            let fits = object_size == size_of::<DIDEVICEOBJECTDATA>() as u32
                || object_size == OBJECT_DATA_DX7_SIZE;
            let Some(wanted) = count.as_mut() else {
                return hr::INVALIDPARAM;
            };
            if !fits {
                return hr::INVALIDPARAM;
            }
            view.update(Instant::now());
            let take = (*wanted as usize).min(view.buffer.events.len());
            if !data.is_null() {
                for (i, event) in view.buffer.events.iter().take(take).enumerate() {
                    let dst = data.cast::<u8>().add(i * object_size as usize);
                    std::ptr::copy_nonoverlapping(
                        (event as *const DIDEVICEOBJECTDATA).cast::<u8>(),
                        dst,
                        object_size as usize,
                    );
                }
            }
            if flags & DIGDD_PEEK == 0 {
                view.buffer.events.drain(..take);
            }
            *wanted = take as u32;
            if std::mem::take(&mut view.buffer.overflow) {
                hr::FALSE
            } else {
                hr::OK
            }
        })
    }
}

unsafe extern "system" fn set_data_format(this: This, format: *const DIDATAFORMAT) -> HRESULT {
    // SAFETY: a call on our object; the game's format.
    unsafe { with(this, hr::GENERIC, |view| view.set_data_format(format)) }
}

unsafe extern "system" fn set_event_notification(this: This, event: HANDLE) -> HRESULT {
    // SAFETY: a call on our object.
    unsafe {
        with(this, hr::GENERIC, |view| {
            if view.acquired {
                return hr::ACQUIRED;
            }
            view.event = event as usize;
            hr::OK
        })
    }
}

unsafe extern "system" fn set_cooperative_level(this: This, window: HWND, flags: u32) -> HRESULT {
    let one_of = |a: u32, b: u32| (flags & a != 0) != (flags & b != 0);
    if !one_of(DISCL_EXCLUSIVE, DISCL_NONEXCLUSIVE) || !one_of(DISCL_FOREGROUND, DISCL_BACKGROUND) {
        return hr::INVALIDPARAM;
    }
    // SAFETY: a call on our object; a window query on the game's handle.
    unsafe {
        with(this, hr::GENERIC, |view| {
            if view.acquired {
                return hr::ACQUIRED;
            }
            let root = if window.is_null() {
                window
            } else {
                GetAncestor(window, GA_ROOT)
            };
            log!("SetCooperativeLevel {flags:#x}");
            lock(&view.rig).set_window(if root.is_null() { window } else { root });
            hr::OK
        })
    }
}

unsafe extern "system" fn get_object_info(
    this: This,
    info: *mut DIDEVICEOBJECTINSTANCEA,
    obj: u32,
    how: u32,
) -> HRESULT {
    // SAFETY: a call on our object; `info` is the game's or null.
    unsafe {
        with(this, hr::GENERIC, |view| {
            let Some(size) = info.as_ref().map(|i| i.dwSize) else {
                return hr::INVALIDPARAM;
            };
            if size != size_of::<DIDEVICEOBJECTINSTANCEA>() as u32 && size != OBJECT_DX3_SIZE {
                return hr::INVALIDPARAM;
            }
            match view.target(how, obj) {
                Ok(Some(part)) => {
                    copy_sized(&view.object_instance(part), info, size);
                    hr::OK
                }
                Ok(None) => hr::INVALIDPARAM,
                Err(code) => code,
            }
        })
    }
}

unsafe extern "system" fn get_device_info(this: This, info: *mut DIDEVICEINSTANCEA) -> HRESULT {
    // SAFETY: a call on our object; `info` is the game's or null.
    unsafe {
        with(this, hr::GENERIC, |_| {
            let Some(size) = info.as_ref().map(|i| i.dwSize) else {
                return hr::INVALIDPARAM;
            };
            if size != size_of::<DIDEVICEINSTANCEA>() as u32 && size != INSTANCE_DX3_SIZE {
                return hr::INVALIDPARAM;
            }
            copy_sized(&instance(), info, size);
            hr::OK
        })
    }
}

unsafe extern "system" fn run_control_panel(_: This, _: HWND, _: u32) -> HRESULT {
    hr::OK
}

unsafe extern "system" fn initialize(_: This, _: HINSTANCE, _: u32, _: *const GUID) -> HRESULT {
    hr::OK
}

unsafe extern "system" fn create_effect(
    this: This,
    guid: *const GUID,
    params: *const DIEFFECT,
    out: *mut This,
    outer: This,
) -> HRESULT {
    // SAFETY: a call on our object; the game's GUID, parameters and out
    // pointer.
    unsafe {
        guard(hr::GENERIC, || {
            if out.is_null() || guid.is_null() || !outer.is_null() {
                return hr::INVALIDPARAM;
            }
            *out = std::ptr::null_mut();
            let Some(kind) = effect::kind_of(&*guid).filter(|k| *k != Kind::CustomForce) else {
                return hr::DEVICENOTREG;
            };
            let device = device(this);
            let view = lock(&device.view);
            let mut game = Effect::new(kind);
            if !params.is_null() {
                let all = if (*params).dwSize >= size_of::<DIEFFECT>() as u32 {
                    DIEP_ALLPARAMS
                } else {
                    DIEP_ALLPARAMS_DX5
                };
                game = match effect::read(&game, params, all, |by_id, v| view.axis_named(by_id, v))
                {
                    Ok(game) => game,
                    Err(code) => return code,
                };
            }
            log!("CreateEffect {}", game.describe());
            *out = effect::create(&device.view, &mut lock(&view.rig), game);
            hr::OK
        })
    }
}

/// `DIEFFECTINFOA` of a standard effect.
fn effect_info(kind: Kind) -> DIEFFECTINFOA {
    let shaped = DIEFT_FFATTACK | DIEFT_FFFADE | DIEFT_STARTDELAY;
    let condition = DIEFT_SATURATION
        | DIEFT_POSNEGCOEFFICIENTS
        | DIEFT_POSNEGSATURATION
        | DIEFT_DEADBAND
        | DIEFT_STARTDELAY;
    let family = family_code(kind.family());
    let mut info = DIEFFECTINFOA {
        dwSize: size_of::<DIEFFECTINFOA>() as u32,
        guid: effect::guid_of(kind),
        dwEffType: family
            | if kind.family() == Family::Condition {
                condition
            } else {
                shaped
            },
        dwStaticParams: DIEP_ALLPARAMS,
        dwDynamicParams: DIEP_ALLPARAMS,
        ..DIEFFECTINFOA::default()
    };
    ansi(kind.name(), &mut info.tszName);
    info
}

/// `DIEFT_*` type of a family.
fn family_code(family: Family) -> u32 {
    match family {
        Family::Constant => 1,
        Family::Ramp => 2,
        Family::Periodic => 3,
        Family::Condition => 4,
        Family::Custom => 5,
    }
}

/// The effects the virtual wheel offers: the standard ones the layer can
/// pass to a wheel (all but a custom force).
fn offered() -> impl Iterator<Item = Kind> {
    Kind::ALL.into_iter().filter(|k| *k != Kind::CustomForce)
}

unsafe extern "system" fn enum_effects(
    _: This,
    callback: LPDIENUMEFFECTSCALLBACKA,
    context: *mut c_void,
    kind: u32,
) -> HRESULT {
    let Some(callback) = callback else {
        return hr::INVALIDPARAM;
    };
    let wanted = kind & 0xFF;
    guard(hr::GENERIC, || {
        for k in offered().filter(|k| wanted == 0 || family_code(k.family()) == wanted) {
            let mut info = effect_info(k);
            // SAFETY: the game's callback with a structure that lives
            // across the call.
            if unsafe { callback(&mut info, context) } == DIENUM_STOP as i32 {
                break;
            }
        }
        hr::OK
    })
}

unsafe extern "system" fn get_effect_info(
    _: This,
    info: *mut DIEFFECTINFOA,
    guid: *const GUID,
) -> HRESULT {
    // SAFETY: the game's structure and GUID, or null.
    unsafe {
        guard(hr::GENERIC, || {
            let (Some(out), Some(guid)) = (info.as_mut(), guid.as_ref()) else {
                return hr::INVALIDPARAM;
            };
            if out.dwSize != size_of::<DIEFFECTINFOA>() as u32 {
                return hr::INVALIDPARAM;
            }
            match effect::kind_of(guid).filter(|k| offered().any(|o| o == *k)) {
                Some(kind) => {
                    *out = effect_info(kind);
                    hr::OK
                }
                None => hr::DEVICENOTREG,
            }
        })
    }
}

unsafe extern "system" fn get_force_feedback_state(this: This, state: *mut u32) -> HRESULT {
    // SAFETY: a call on our object; `state` is the game's or null.
    unsafe {
        with(this, hr::GENERIC, |view| {
            let Some(state) = state.as_mut() else {
                return hr::INVALIDPARAM;
            };
            *state = lock(&view.rig).ffb_state();
            hr::OK
        })
    }
}

unsafe extern "system" fn send_force_feedback_command(this: This, command: u32) -> HRESULT {
    // SAFETY: a call on our object.
    unsafe {
        with(this, hr::GENERIC, |view| {
            if !matches!(command, 1 | 2 | 4 | 8 | 0x10 | 0x20) {
                return hr::INVALIDPARAM;
            }
            log!("SendForceFeedbackCommand {command:#x}");
            lock(&view.rig).command(command)
        })
    }
}

unsafe extern "system" fn enum_created_effect_objects(
    this: This,
    callback: LPDIENUMCREATEDEFFECTOBJECTSCALLBACK,
    context: *mut c_void,
    flags: u32,
) -> HRESULT {
    let Some(callback) = callback.filter(|_| flags == 0) else {
        return hr::INVALIDPARAM;
    };
    // SAFETY: a call on our object.
    let objects = unsafe {
        with(this, Vec::new(), |view| {
            lock(&view.rig).effects_mut().objects()
        })
    };
    guard(hr::GENERIC, || {
        for object in objects {
            // SAFETY: the game's callback with one of its effects, which
            // it holds (the rig is unlocked: it may call back).
            if unsafe { callback(object, context) } == DIENUM_STOP as i32 {
                break;
            }
        }
        hr::OK
    })
}

unsafe extern "system" fn escape(_: This, _: *mut DIEFFESCAPE) -> HRESULT {
    hr::UNSUPPORTED
}

unsafe extern "system" fn poll(this: This) -> HRESULT {
    // SAFETY: a call on our object.
    unsafe {
        with(this, hr::GENERIC, |view| {
            if !view.acquired {
                return hr::NOTACQUIRED;
            }
            view.update(Instant::now());
            hr::OK
        })
    }
}

unsafe extern "system" fn send_device_data(
    _: This,
    _: u32,
    _: *const DIDEVICEOBJECTDATA,
    _: *mut u32,
    _: u32,
) -> HRESULT {
    hr::UNSUPPORTED
}

unsafe extern "system" fn enum_effects_in_file(
    _: This,
    _: *const u8,
    _: *mut c_void,
    _: *mut c_void,
    _: u32,
) -> HRESULT {
    hr::UNSUPPORTED
}

unsafe extern "system" fn write_effect_to_file(
    _: This,
    _: *const u8,
    _: u32,
    _: *mut c_void,
    _: u32,
) -> HRESULT {
    hr::UNSUPPORTED
}

unsafe extern "system" fn build_action_map(
    _: This,
    _: *mut c_void,
    _: *const u8,
    _: u32,
) -> HRESULT {
    hr::UNSUPPORTED
}

unsafe extern "system" fn set_action_map(_: This, _: *mut c_void, _: *const u8, _: u32) -> HRESULT {
    hr::UNSUPPORTED
}

unsafe extern "system" fn get_image_info(_: This, _: *mut c_void) -> HRESULT {
    hr::UNSUPPORTED
}
