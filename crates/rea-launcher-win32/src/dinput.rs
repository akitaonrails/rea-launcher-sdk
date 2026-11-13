//! DirectInput 8 in the launcher (Windows only): the game controllers a
//! DirectInput game sees, their live readings and a force-feedback test,
//! for the settings window's Wheel tab (`rea_launcher_ui::wheel`).
//!
//! [`DirectInput::start`] moves every DirectInput call to a worker thread,
//! so the window never waits on a device: [`WheelDevices`] calls only copy
//! the worker's latest results. While the tab reads them (a
//! [`WheelDevices::devices`] call in the last [`WATCHED`]) the worker
//! reads every device each [`TICK`] and lists them again every
//! [`RESCAN`] (hot-plug); otherwise it only waits.
//!
//! Devices are read the way the in-game layer reads them: `DIJOYSTATE2`'s
//! eight axes in `rea_launcher::wheel::dinput::REAL_AXES` order with each
//! axis's range set to `REAL_RANGE`, non-exclusive and in the background.
//! The force-feedback test takes the wheel exclusively for
//! `ffb::TEST_PATTERN_MS` through a hidden top-level window of the worker
//! (never shown), then gives it back. The launcher process never loads the
//! in-game layer.

use std::ffi::c_void;
use std::mem::{offset_of, size_of};
use std::ptr::{NonNull, null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    DI_FFNOMINALMAX, DI8DEVCLASS_GAMECTRL, DI8DEVTYPE_DRIVING, DI8DEVTYPE_GAMEPAD, DICONSTANTFORCE,
    DIDATAFORMAT, DIDC_FORCEFEEDBACK, DIDEVCAPS, DIDEVICEINSTANCEW, DIDF_ABSAXIS,
    DIDFT_ANYINSTANCE, DIDFT_AXIS, DIDFT_BUTTON, DIDFT_POV, DIDOI_ASPECTPOSITION, DIEB_NOTRIGGER,
    DIEDFL_ATTACHEDONLY, DIEFF_CARTESIAN, DIEFF_OBJECTOFFSETS, DIEFFECT, DIENUM_CONTINUE,
    DIEP_TYPESPECIFICPARAMS, DIJOYSTATE2, DIOBJECTDATAFORMAT, DIPH_DEVICE, DIPROPAUTOCENTER_OFF,
    DIPROPAUTOCENTER_ON, DIPROPDWORD, DIPROPHEADER, DIPROPRANGE, DISCL_BACKGROUND, DISCL_EXCLUSIVE,
    DISCL_NONEXCLUSIVE, DirectInput8Create, GUID_ConstantForce, GUID_POV, GUID_RxAxis, GUID_RyAxis,
    GUID_RzAxis, GUID_Slider, GUID_XAxis, GUID_YAxis, GUID_ZAxis,
};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW,
    TranslateMessage, WS_OVERLAPPED,
};
use windows_sys::core::{BOOL, GUID, HRESULT};

use crate::wide::wide;
use rea_launcher::ffb::{FfbSettings, test_pattern};
use rea_launcher::wheel::dinput::REAL_RANGE;
use rea_launcher::wheel::rig::{Device, FfbTest, WheelDevices, guid_text};
use rea_launcher::wheel::{self, DeviceFacts, DeviceId, DeviceShape, DeviceState};

/// How often the worker reads the devices while they are watched.
pub const TICK: Duration = Duration::from_millis(10);
/// How often it lists them again (hot-plug) while they are watched.
pub const RESCAN: Duration = Duration::from_secs(2);
/// How long after the last [`WheelDevices::devices`] call they count as
/// watched.
pub const WATCHED: Duration = Duration::from_secs(1);

const DIRECTINPUT_VERSION: u32 = 0x0800;
/// IID_IDirectInput8W.
const IID_IDIRECTINPUT8W: GUID = GUID::from_u128(0xbf798031_483a_4da2_aa99_5d64ed369700);
/// `DIPROP_*` are `MAKEDIPROP(n)`: the number itself, cast to a GUID
/// pointer (windows-sys spells them as GUIDs, which DirectInput would
/// read as an address).
const DIPROP_RANGE: usize = 4;
const DIPROP_AUTOCENTER: usize = 9;
const DIDFT_OPTIONAL: u32 = 0x8000_0000;
const INFINITE: u32 = u32::MAX;
/// What `guidProduct.Data4` ends with when Data1 holds the USB ids.
const PIDVID: &[u8; 6] = b"PIDVID";

// ---------------------------------------------------------------- COM

/// The first three entries of every COM vtable.
#[repr(C)]
struct UnknownVtbl {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
struct DirectInputVtbl {
    base: UnknownVtbl,
    create_device: unsafe extern "system" fn(
        *mut c_void,
        *const GUID,
        *mut *mut c_void,
        *mut c_void,
    ) -> HRESULT,
    enum_devices: unsafe extern "system" fn(
        *mut c_void,
        u32,
        Option<unsafe extern "system" fn(*mut DIDEVICEINSTANCEW, *mut c_void) -> BOOL>,
        *mut c_void,
        u32,
    ) -> HRESULT,
}

#[repr(C)]
struct DeviceVtbl {
    base: UnknownVtbl,
    get_capabilities: unsafe extern "system" fn(*mut c_void, *mut DIDEVCAPS) -> HRESULT,
    enum_objects: usize,
    get_property: usize,
    set_property:
        unsafe extern "system" fn(*mut c_void, *const GUID, *const DIPROPHEADER) -> HRESULT,
    acquire: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    unacquire: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    get_device_state: unsafe extern "system" fn(*mut c_void, u32, *mut c_void) -> HRESULT,
    get_device_data: usize,
    set_data_format: unsafe extern "system" fn(*mut c_void, *const DIDATAFORMAT) -> HRESULT,
    set_event_notification: usize,
    set_cooperative_level: unsafe extern "system" fn(*mut c_void, HWND, u32) -> HRESULT,
    get_object_info: usize,
    get_device_info: usize,
    run_control_panel: usize,
    initialize: usize,
    create_effect: unsafe extern "system" fn(
        *mut c_void,
        *const GUID,
        *const DIEFFECT,
        *mut *mut c_void,
        *mut c_void,
    ) -> HRESULT,
    enum_effects: usize,
    get_effect_info: usize,
    get_force_feedback_state: usize,
    send_force_feedback_command: usize,
    enum_created_effect_objects: usize,
    escape: usize,
    poll: unsafe extern "system" fn(*mut c_void) -> HRESULT,
}

#[repr(C)]
struct EffectVtbl {
    base: UnknownVtbl,
    initialize: usize,
    get_effect_guid: usize,
    get_parameters: usize,
    set_parameters: unsafe extern "system" fn(*mut c_void, *const DIEFFECT, u32) -> HRESULT,
    start: unsafe extern "system" fn(*mut c_void, u32, u32) -> HRESULT,
    stop: unsafe extern "system" fn(*mut c_void) -> HRESULT,
}

/// A COM object we hold one reference to, released on drop. `V` is its
/// vtable, which starts with [`UnknownVtbl`].
struct Com<V> {
    ptr: NonNull<c_void>,
    vtbl: std::marker::PhantomData<V>,
}

impl<V> Com<V> {
    /// SAFETY: `ptr` is null or a live COM object whose vtable is `V`,
    /// with a reference that becomes this value's.
    unsafe fn new(ptr: *mut c_void) -> Option<Self> {
        NonNull::new(ptr).map(|ptr| Com {
            ptr,
            vtbl: std::marker::PhantomData,
        })
    }

    fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    fn vtbl(&self) -> &V {
        // SAFETY: a COM object starts with its vtable pointer; `new`'s
        // caller vouched that the vtable is a `V`, and it outlives the
        // object.
        unsafe { &**self.ptr.as_ptr().cast::<*const V>() }
    }
}

impl<V> Drop for Com<V> {
    fn drop(&mut self) {
        // SAFETY: every vtable starts with IUnknown's; this value owns one
        // reference.
        unsafe {
            let base = &**self.ptr.as_ptr().cast::<*const UnknownVtbl>();
            (base.release)(self.raw());
        }
    }
}

fn ok(hr: HRESULT) -> bool {
    hr >= 0
}

// ---------------------------------------------------------------- pure parts

/// The USB ids in a product GUID, when DirectInput put them there
/// (`Data1` = PID << 16 | VID, `Data4` ending in "PIDVID"); else 0, 0.
fn product_ids(product: &GUID) -> (u16, u16) {
    if &product.data4[2..] == PIDVID {
        (product.data1 as u16, (product.data1 >> 16) as u16)
    } else {
        (0, 0)
    }
}

/// A NUL-terminated UTF-16 buffer as text.
fn text(buffer: &[u16]) -> String {
    let end = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
    String::from_utf16_lossy(&buffer[..end]).trim().to_owned()
}

/// A `DIJOYSTATE2` as a [`DeviceState`]: the eight axes of [`rea_launcher::wheel::dinput::REAL_AXES`]
/// (clamped to `i16`) and the first `buttons` buttons.
fn reading(state: &DIJOYSTATE2, buttons: usize) -> DeviceState {
    let [s0, s1] = state.rglSlider;
    let axes = [
        state.lX, state.lY, state.lZ, state.lRx, state.lRy, state.lRz, s0, s1,
    ];
    DeviceState {
        axes: axes
            .iter()
            .map(|&v| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16)
            .collect(),
        buttons: state.rgbButtons[..buttons.min(128)]
            .iter()
            .map(|&b| b & 0x80 != 0)
            .collect(),
    }
}

/// `c_dfDIJoystick2` without the velocity, acceleration and force axes:
/// the eight axes, four hats and 128 buttons the in-game layer reads, at
/// their `DIJOYSTATE2` offsets. Holds the GUIDs the entries point to.
struct Format {
    /// Never read: the objects point into it.
    _guids: Box<[GUID; 8]>,
    objects: Vec<DIOBJECTDATAFORMAT>,
}

impl Format {
    fn new() -> Format {
        let guids = Box::new([
            GUID_XAxis,
            GUID_YAxis,
            GUID_ZAxis,
            GUID_RxAxis,
            GUID_RyAxis,
            GUID_RzAxis,
            GUID_Slider,
            GUID_POV,
        ]);
        let slider = offset_of!(DIJOYSTATE2, rglSlider);
        let axes = [
            offset_of!(DIJOYSTATE2, lX),
            offset_of!(DIJOYSTATE2, lY),
            offset_of!(DIJOYSTATE2, lZ),
            offset_of!(DIJOYSTATE2, lRx),
            offset_of!(DIJOYSTATE2, lRy),
            offset_of!(DIJOYSTATE2, lRz),
            slider,
            slider + 4,
        ];
        let any = DIDFT_OPTIONAL | DIDFT_ANYINSTANCE;
        let object = |guid: *const GUID, offset: usize, kind: u32, flags: u32| DIOBJECTDATAFORMAT {
            pguid: guid,
            dwOfs: offset as u32,
            dwType: kind | any,
            dwFlags: flags,
        };
        let mut objects: Vec<DIOBJECTDATAFORMAT> = axes
            .iter()
            .enumerate()
            .map(|(i, &offset)| object(&guids[i.min(6)], offset, DIDFT_AXIS, DIDOI_ASPECTPOSITION))
            .collect();
        let pov = offset_of!(DIJOYSTATE2, rgdwPOV);
        objects.extend((0..4).map(|i| object(&guids[7], pov + 4 * i, DIDFT_POV, 0)));
        let buttons = offset_of!(DIJOYSTATE2, rgbButtons);
        objects.extend((0..128).map(|i| object(null(), buttons + i, DIDFT_BUTTON, 0)));
        Format {
            _guids: guids,
            objects,
        }
    }

    /// Valid while `self` lives (DirectInput reads it during
    /// `SetDataFormat`).
    fn data_format(&self) -> DIDATAFORMAT {
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

fn header<T>() -> DIPROPHEADER {
    DIPROPHEADER {
        dwSize: size_of::<T>() as u32,
        dwHeaderSize: size_of::<DIPROPHEADER>() as u32,
        dwObj: 0,
        dwHow: DIPH_DEVICE,
    }
}

// ---------------------------------------------------------------- devices

/// An opened device.
struct Open {
    com: Com<DeviceVtbl>,
    info: Device,
}

impl Open {
    /// Opens `instance` for background reading: the data format, the
    /// range on every axis, then a first reading.
    fn new(instance: &DIDEVICEINSTANCEW, s: &Session) -> Option<Open> {
        let di = &s.di;
        let mut ptr = null_mut();
        // SAFETY: a live DirectInput; the out pointer receives a device
        // with one reference, which `Com` takes.
        let com: Com<DeviceVtbl> = unsafe {
            let hr =
                (di.vtbl().create_device)(di.raw(), &instance.guidInstance, &mut ptr, null_mut());
            if !ok(hr) {
                return None;
            }
            Com::new(ptr)?
        };
        let v = com.vtbl();
        let format = s.format.data_format();
        let range = DIPROPRANGE {
            diph: header::<DIPROPRANGE>(),
            lMin: REAL_RANGE.0,
            lMax: REAL_RANGE.1,
        };
        let mut caps = DIDEVCAPS {
            dwSize: size_of::<DIDEVCAPS>() as u32,
            ..DIDEVCAPS::default()
        };
        // SAFETY: a live device; every structure is sized and lives across
        // its call. A failed range keeps the device's own (it may have no
        // axes at all).
        unsafe {
            if !ok((v.set_data_format)(com.raw(), &format))
                || !ok((v.set_cooperative_level)(
                    com.raw(),
                    s.window,
                    DISCL_NONEXCLUSIVE | DISCL_BACKGROUND,
                ))
                || !ok((v.get_capabilities)(com.raw(), &mut caps))
            {
                return None;
            }
            (v.set_property)(com.raw(), DIPROP_RANGE as *const GUID, &range.diph);
            (v.acquire)(com.raw());
        }
        let (vid, pid) = product_ids(&instance.guidProduct);
        let id = DeviceId {
            name: text(&instance.tszProductName),
            vid,
            pid,
            guid: guid_text(
                instance.guidInstance.data1,
                instance.guidInstance.data2,
                instance.guidInstance.data3,
                instance.guidInstance.data4,
            ),
        };
        let devtype = instance.dwDevType & 0xff;
        let kind = wheel::classify(&DeviceFacts {
            vid,
            pid,
            sdl_wheel_type: devtype == DI8DEVTYPE_DRIVING,
            game_controller: devtype == DI8DEVTYPE_GAMEPAD,
            axes: caps.dwAxes as usize,
        });
        let mut open = Open {
            com,
            info: Device {
                id,
                kind,
                axes: caps.dwAxes as usize,
                buttons: (caps.dwButtons as usize).min(128),
                ffb: caps.dwFlags & DIDC_FORCEFEEDBACK != 0,
                initial: DeviceState::default(),
                state: DeviceState::default(),
            },
        };
        open.read();
        open.info.initial = open.info.state.clone();
        Some(open)
    }

    /// Takes the current reading; re-acquires a device that lost it.
    fn read(&mut self) {
        let v = self.com.vtbl();
        // SAFETY: a zeroed DIJOYSTATE2 is valid; the device's data format
        // is DIJOYSTATE2-sized.
        let mut state: DIJOYSTATE2 = unsafe { std::mem::zeroed() };
        let mut get = || unsafe {
            (v.poll)(self.com.raw());
            (v.get_device_state)(
                self.com.raw(),
                size_of::<DIJOYSTATE2>() as u32,
                (&raw mut state).cast(),
            )
        };
        let mut hr = get();
        if !ok(hr) {
            // SAFETY: a live device.
            unsafe { (v.acquire)(self.com.raw()) };
            hr = get();
        }
        if ok(hr) {
            self.info.state = reading(&state, self.info.buttons);
        }
    }

    fn shape(&self) -> DeviceShape {
        DeviceShape {
            vid: self.info.id.vid,
            pid: self.info.id.pid,
            kind: self.info.kind,
            axes: self.info.axes,
        }
    }

    /// Back to background reading with the autocentre on, after a test.
    fn release_exclusive(&self, window: HWND) {
        let v = self.com.vtbl();
        let centre = DIPROPDWORD {
            diph: header::<DIPROPDWORD>(),
            dwData: DIPROPAUTOCENTER_ON,
        };
        // SAFETY: a live device and a sized property.
        unsafe {
            (v.set_property)(
                self.com.raw(),
                DIPROP_AUTOCENTER as *const GUID,
                &centre.diph,
            );
            (v.unacquire)(self.com.raw());
            (v.set_cooperative_level)(
                self.com.raw(),
                window,
                DISCL_NONEXCLUSIVE | DISCL_BACKGROUND,
            );
            (v.acquire)(self.com.raw());
        }
    }
}

/// A running force-feedback test.
struct Test {
    /// Declared first: released before the device is given back.
    effect: Com<EffectVtbl>,
    device: usize,
    settings: FfbSettings,
    started: Instant,
    magnitude: i32,
}

/// A constant force on the X axis, from the west: a positive magnitude
/// (up to 10000) pushes right, as `rea_launcher_native`'s SDL effects
/// do. Owns what [`ConstantForce::effect`]'s pointers point to.
struct ConstantForce {
    axes: [u32; 1],
    direction: [i32; 1],
    force: DICONSTANTFORCE,
}

impl ConstantForce {
    fn new(magnitude: i32) -> ConstantForce {
        ConstantForce {
            axes: [offset_of!(DIJOYSTATE2, lX) as u32],
            direction: [-1],
            force: DICONSTANTFORCE {
                lMagnitude: magnitude,
            },
        }
    }

    /// The effect's parameters, valid while `self` is neither moved nor
    /// dropped.
    fn effect(&mut self) -> DIEFFECT {
        DIEFFECT {
            dwSize: size_of::<DIEFFECT>() as u32,
            dwFlags: DIEFF_CARTESIAN | DIEFF_OBJECTOFFSETS,
            dwDuration: INFINITE,
            dwSamplePeriod: 0,
            dwGain: DI_FFNOMINALMAX,
            dwTriggerButton: DIEB_NOTRIGGER,
            dwTriggerRepeatInterval: 0,
            cAxes: 1,
            rgdwAxes: self.axes.as_mut_ptr(),
            rglDirection: self.direction.as_mut_ptr(),
            lpEnvelope: null_mut(),
            cbTypeSpecificParams: size_of::<DICONSTANTFORCE>() as u32,
            lpvTypeSpecificParams: (&raw mut self.force).cast(),
            dwStartDelay: 0,
        }
    }
}

/// The force of [`test_pattern`] at `elapsed`, in DirectInput units;
/// `None` once it is over.
fn test_magnitude(elapsed: Duration, settings: &FfbSettings) -> Option<i32> {
    let out = test_pattern(elapsed.as_millis() as u64, settings)?;
    let sign = if settings.invert { -1.0 } else { 1.0 };
    Some((out.constant * sign * DI_FFNOMINALMAX as f32) as i32)
}

/// Everything DirectInput on the worker thread. Fields drop in order: the
/// test's effect, the devices, DirectInput, then the window.
struct Session {
    test: Option<Test>,
    devices: Vec<Open>,
    di: Com<DirectInputVtbl>,
    format: Format,
    window: HWND,
}

impl Session {
    fn new() -> Result<Session, String> {
        // SAFETY: this module's handle; a hidden STATIC window (no class to
        // register), never shown; DirectInput8Create hands back one
        // reference, which `Com` takes.
        unsafe {
            let module = GetModuleHandleW(null());
            let class = wide("STATIC");
            let title = wide("rea wheel");
            let window = CreateWindowExW(
                0,
                class.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                null_mut(),
                null_mut(),
                module,
                null(),
            );
            let mut ptr = null_mut();
            let hr = DirectInput8Create(
                module,
                DIRECTINPUT_VERSION,
                &IID_IDIRECTINPUT8W,
                &mut ptr,
                null_mut(),
            );
            let di = if ok(hr) { Com::new(ptr) } else { None };
            let Some(di) = di else {
                if !window.is_null() {
                    DestroyWindow(window);
                }
                return Err(format!("DirectInput8Create failed ({hr:#010x})"));
            };
            Ok(Session {
                test: None,
                devices: Vec::new(),
                di,
                format: Format::new(),
                window,
            })
        }
    }

    /// Lists the game controllers again: devices still attached stay
    /// open, new ones are opened, gone ones closed. Not during a test.
    fn scan(&mut self) {
        if self.test.is_some() {
            return;
        }
        unsafe extern "system" fn collect(
            instance: *mut DIDEVICEINSTANCEW,
            list: *mut c_void,
        ) -> BOOL {
            // SAFETY: `list` is the Vec passed below; DirectInput gives a
            // valid instance for the callback's duration.
            unsafe { (*list.cast::<Vec<DIDEVICEINSTANCEW>>()).push(*instance) };
            DIENUM_CONTINUE as BOOL
        }
        let mut found: Vec<DIDEVICEINSTANCEW> = Vec::new();
        // SAFETY: a live DirectInput; the callback only pushes to `found`,
        // which outlives the call.
        let hr = unsafe {
            (self.di.vtbl().enum_devices)(
                self.di.raw(),
                DI8DEVCLASS_GAMECTRL,
                Some(collect),
                (&raw mut found).cast(),
                DIEDFL_ATTACHEDONLY,
            )
        };
        if !ok(hr) {
            return;
        }
        let guid_of = |i: &DIDEVICEINSTANCEW| {
            let g = &i.guidInstance;
            guid_text(g.data1, g.data2, g.data3, g.data4)
        };
        let mut kept: Vec<Option<Open>> = std::mem::take(&mut self.devices)
            .into_iter()
            .map(Some)
            .collect();
        for instance in &found {
            let guid = guid_of(instance);
            let old = kept
                .iter_mut()
                .find(|o| o.as_ref().is_some_and(|o| o.info.id.guid == guid))
                .and_then(Option::take);
            if let Some(open) = old.or_else(|| Open::new(instance, self)) {
                self.devices.push(open);
            }
        }
    }

    /// The devices a game would bind (spare HID interfaces left out).
    fn listed(&self) -> Vec<Device> {
        let shapes: Vec<DeviceShape> = self.devices.iter().map(Open::shape).collect();
        self.devices
            .iter()
            .enumerate()
            .filter(|&(i, _)| !wheel::is_spare_interface(&shapes, i))
            .map(|(_, d)| d.info.clone())
            .collect()
    }

    fn read_all(&mut self) {
        for d in &mut self.devices {
            d.read();
        }
    }

    /// Takes the wheel with `guid` exclusively and starts the test force.
    fn start_test(&mut self, guid: &str, settings: FfbSettings) -> Result<(), String> {
        self.end_test();
        let device = self
            .devices
            .iter()
            .position(|d| d.info.id.guid == guid)
            .ok_or("The wheel isn't connected.")?;
        let open = &self.devices[device];
        if !open.info.ffb {
            return Err("This device has no force feedback.".into());
        }
        let v = open.com.vtbl();
        let raw = open.com.raw();
        let centre = DIPROPDWORD {
            diph: header::<DIPROPDWORD>(),
            dwData: DIPROPAUTOCENTER_OFF,
        };
        let mut force = ConstantForce::new(0);
        let effect = force.effect();
        let mut ptr = null_mut();
        // SAFETY: a live device and the worker's own top-level window; the
        // effect's arrays live across CreateEffect, which copies them; the
        // new effect's reference goes to `Com`.
        let created = unsafe {
            (v.unacquire)(raw);
            let taken = ok((v.set_cooperative_level)(
                raw,
                self.window,
                DISCL_EXCLUSIVE | DISCL_BACKGROUND,
            )) && ok((v.acquire)(raw));
            if taken {
                (v.set_property)(raw, DIPROP_AUTOCENTER as *const GUID, &centre.diph);
                let hr = (v.create_effect)(raw, &GUID_ConstantForce, &effect, &mut ptr, null_mut());
                if ok(hr) {
                    Com::<EffectVtbl>::new(ptr)
                } else {
                    None
                }
            } else {
                None
            }
        };
        let Some(effect) = created else {
            open.release_exclusive(self.window);
            return Err("The wheel refused the test force. Close other programs that use it, then try again.".into());
        };
        // SAFETY: a live effect.
        unsafe { (effect.vtbl().start)(effect.raw(), 1, 0) };
        self.test = Some(Test {
            effect,
            device,
            settings,
            started: Instant::now(),
            magnitude: 0,
        });
        Ok(())
    }

    /// Moves the test force along the pattern; true once it is over.
    fn tick_test(&mut self) -> bool {
        let Some(test) = &mut self.test else {
            return false;
        };
        let Some(magnitude) = test_magnitude(test.started.elapsed(), &test.settings) else {
            self.end_test();
            return true;
        };
        if magnitude != test.magnitude {
            let mut force = ConstantForce::new(magnitude);
            let effect = force.effect();
            // SAFETY: a live effect; the parameters live across the call.
            unsafe {
                (test.effect.vtbl().set_parameters)(
                    test.effect.raw(),
                    &effect,
                    DIEP_TYPESPECIFICPARAMS,
                )
            };
            test.magnitude = magnitude;
        }
        false
    }

    /// Stops a running test and gives the wheel back.
    fn end_test(&mut self) {
        if let Some(test) = self.test.take() {
            // SAFETY: a live effect, released (dropped) right after.
            unsafe { (test.effect.vtbl().stop)(test.effect.raw()) };
            drop(test.effect);
            self.devices[test.device].release_exclusive(self.window);
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.end_test();
        self.devices.clear();
        if !self.window.is_null() {
            // SAFETY: the worker's own window, destroyed once.
            unsafe { DestroyWindow(self.window) };
        }
    }
}

// ---------------------------------------------------------------- worker

/// What the worker and the window share.
#[derive(Default)]
struct Shared {
    devices: Vec<Device>,
    ffb: FfbTest,
    refresh: bool,
    test: Option<(String, FfbSettings)>,
    watched: Option<Instant>,
}

/// DirectInput on its own thread, for the Wheel tab. Dropping it stops
/// the thread (and any test force) within a [`TICK`].
pub struct DirectInput {
    shared: Arc<Mutex<Shared>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl DirectInput {
    /// Starts the worker; the first list follows within a [`TICK`] or so
    /// of the first [`WheelDevices::devices`] call.
    pub fn start() -> DirectInput {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let (shared, stop) = (shared.clone(), stop.clone());
            std::thread::Builder::new()
                .name("rea-dinput".into())
                .spawn(move || work(&shared, &stop))
                .ok()
        };
        DirectInput {
            shared,
            stop,
            worker,
        }
    }

    fn shared(&self) -> MutexGuard<'_, Shared> {
        lock(&self.shared)
    }
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl WheelDevices for DirectInput {
    fn devices(&mut self) -> Vec<Device> {
        let mut shared = self.shared();
        shared.watched = Some(Instant::now());
        shared.devices.clone()
    }

    fn refresh(&mut self) {
        self.shared().refresh = true;
    }

    fn ffb_test(&mut self, guid: &str, settings: FfbSettings) {
        let mut shared = self.shared();
        shared.test = Some((guid.to_owned(), settings));
        shared.ffb = FfbTest::Playing;
    }

    fn ffb_status(&mut self) -> FfbTest {
        self.shared().ffb.clone()
    }
}

impl Drop for DirectInput {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Answers the hidden window's messages, so a broadcast never waits on it.
fn pump_messages() {
    // SAFETY: a zeroed MSG is valid; this thread's own queue.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn work(shared: &Mutex<Shared>, stop: &AtomicBool) {
    let mut session = match Session::new() {
        Ok(session) => session,
        Err(why) => {
            lock(shared).ffb = FfbTest::Failed(format!("DirectInput isn't available: {why}"));
            return;
        }
    };
    let mut scanned: Option<Instant> = None;
    while !stop.load(Ordering::SeqCst) {
        pump_messages();
        let (watched, refresh, test) = {
            let mut s = lock(shared);
            let watched = s.watched.is_some_and(|t| t.elapsed() < WATCHED);
            (watched, std::mem::take(&mut s.refresh), s.test.take())
        };
        if refresh || (watched && scanned.is_none_or(|t| t.elapsed() >= RESCAN)) {
            session.scan();
            scanned = Some(Instant::now());
        }
        if let Some((guid, settings)) = test
            && let Err(why) = session.start_test(&guid, settings)
        {
            lock(shared).ffb = FfbTest::Failed(why);
        }
        if session.tick_test() {
            lock(shared).ffb = FfbTest::Done;
        }
        if watched {
            session.read_all();
            lock(shared).devices = session.listed();
        }
        std::thread::sleep(TICK);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rea_launcher::wheel::dinput::REAL_AXES;

    #[test]
    fn the_format_reads_the_axes_in_the_contracts_order() {
        let format = Format::new();
        assert_eq!(format.objects.len(), 8 + 4 + 128);
        assert_eq!(REAL_AXES.len(), 8);
        // the axes come first, at DIJOYSTATE2's lX.. rglSlider[1]
        for (i, object) in format.objects[..8].iter().enumerate() {
            assert_eq!(object.dwOfs as usize, 4 * i, "{}", REAL_AXES[i]);
            assert_eq!(object.dwType & DIDFT_AXIS, DIDFT_AXIS);
        }
        assert!(std::ptr::eq(format.objects[6].pguid, &format._guids[6]));
        assert!(
            std::ptr::eq(format.objects[7].pguid, &format._guids[6]),
            "two sliders"
        );
        let df = format.data_format();
        assert_eq!(df.dwDataSize as usize, size_of::<DIJOYSTATE2>());
        assert_eq!(df.dwNumObjs, 140);
    }

    #[test]
    fn a_reading_keeps_eight_axes_and_the_devices_buttons() {
        // SAFETY: a zeroed DIJOYSTATE2 is valid.
        let mut state: DIJOYSTATE2 = unsafe { std::mem::zeroed() };
        state.lX = -40_000;
        state.rglSlider = [5, 70_000];
        state.rgbButtons[1] = 0x80;
        state.rgbButtons[5] = 0x80;
        let r = reading(&state, 4);
        assert_eq!(r.axes, vec![-32_768, 0, 0, 0, 0, 0, 5, 32_767]);
        assert_eq!(r.buttons, vec![false, true, false, false]);
    }

    #[test]
    fn usb_ids_come_from_a_pidvid_product_guid() {
        let csl = GUID {
            data1: 0x0020_0eb7,
            data2: 0,
            data3: 0,
            data4: [0, 0, b'P', b'I', b'D', b'V', b'I', b'D'],
        };
        assert_eq!(product_ids(&csl), (0x0eb7, 0x0020));
        assert_eq!(
            product_ids(&GUID {
                data4: [0; 8],
                ..csl
            }),
            (0, 0)
        );
        assert_eq!(text(&[0x52, 0x65, 0x61, 0, 0x41]), "Rea");
    }

    #[test]
    fn the_test_force_pushes_left_then_right_then_stops() {
        let s = FfbSettings {
            strength: 100,
            ..FfbSettings::default()
        };
        let at = |ms| test_magnitude(Duration::from_millis(ms), &s);
        assert_eq!(at(100), Some(-6_000));
        assert_eq!(at(700), Some(6_000));
        assert_eq!(at(1_200), Some(0));
        assert_eq!(at(1_500), None);
        let inverted = FfbSettings { invert: true, ..s };
        assert_eq!(
            test_magnitude(Duration::from_millis(100), &inverted),
            Some(6_000)
        );
    }

    /// Wine has no wheel: the list must still come (empty, or Wine's own
    /// virtual devices, each with every axis), a test on a missing wheel
    /// says so, and dropping stops the worker.
    #[test]
    fn the_worker_lists_tests_and_stops_without_a_wheel() {
        let mut di = DirectInput::start();
        let start = Instant::now();
        let mut devices = di.devices();
        while start.elapsed() < Duration::from_millis(500) {
            std::thread::sleep(Duration::from_millis(20));
            devices = di.devices();
        }
        for d in &devices {
            assert_eq!(d.state.axes.len(), REAL_AXES.len(), "{}", d.id.name);
            assert!(
                d.id.guid.starts_with('{') && d.id.guid.len() == 38,
                "{}",
                d.id.guid
            );
        }
        di.refresh();
        di.ffb_test("{not-a-wheel}", FfbSettings::default());
        let mut status = di.ffb_status();
        while status == FfbTest::Playing && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
            status = di.ffb_status();
        }
        assert_eq!(status, FfbTest::Failed("The wheel isn't connected.".into()));
        let stopping = Instant::now();
        drop(di);
        assert!(stopping.elapsed() < Duration::from_secs(1));
    }
}
