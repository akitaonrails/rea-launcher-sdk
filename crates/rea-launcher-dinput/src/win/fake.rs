//! The test-only fake devices of [`crate::fake`] as `IDirectInputDevice8A`
//! objects: the layer opens, reads and gives force feedback to them
//! exactly as to real ones. A test (the stand-in game) moves their axes
//! and unplugs them through the `rea_dinput_fake_*` exports, and reads
//! what reached them (each call the layer made, one line each) from
//! [`rea_dinput_fake_events`]. With no `fake` key the exports do nothing.

use std::ffi::c_void;
use std::sync::Mutex;

use rea_launcher::wheel::dinput::Axis;
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    DIDATAFORMAT, DIDC_ATTACHED, DIDC_FORCEFEEDBACK, DIDEVCAPS, DIDEVICEINSTANCEA,
    DIDEVICEOBJECTDATA, DIDEVICEOBJECTINSTANCEA, DIEFFECT, DIEFFECTINFOA, DIEFFESCAPE,
    DIEGES_PLAYING, DIEP_ALLPARAMS, DIGFFS_ACTUATORSON, DIGFFS_POWERON, DIGFFS_STOPPED,
    DIJOYSTATE2, DIPROPDWORD, DIPROPHEADER, DIPROPRANGE, DISCL_EXCLUSIVE,
    LPDIENUMCREATEDEFFECTOBJECTSCALLBACK, LPDIENUMDEVICEOBJECTSCALLBACKA, LPDIENUMEFFECTSCALLBACKA,
};
use windows_sys::Win32::Foundation::{HANDLE, HINSTANCE, HWND};
use windows_sys::core::{GUID, HRESULT};

use super::com::{
    Com, DeviceVtbl, EffectVtbl, IID_DEVICE8A, IID_EFFECT, IID_UNKNOWN, Object, This, UnknownVtbl,
    query,
};
use super::real::{Attached, PROP_AUTOCENTER, PROP_FFGAIN, PROP_RANGE};
use super::{effect, guard, hr, lock};
use crate::effect::Effect;
use crate::fake::Fake;
use crate::format::{JOYSTATE2_SIZE, Object as Part};

/// One fake device's state.
struct State {
    spec: crate::fake::FakeDevice,
    axes: [i32; 8],
    buttons: u32,
    pov: u32,
    acquired: bool,
    unplugged: bool,
    /// Counts unplugs: an effect started before the last one stopped.
    epoch: u32,
}

/// The fake devices and the calls that reached them.
struct Fakes {
    devices: Vec<State>,
    events: Vec<String>,
}

static FAKES: Mutex<Option<Fakes>> = Mutex::new(None);

fn record(fakes: &mut Fakes, device: usize, text: String) {
    fakes.events.push(format!("{device}: {text}"));
}

/// Adds the fake devices; their `Attached` entries.
pub(super) fn install(kind: Fake) -> Vec<Attached> {
    let specs = kind.devices();
    let attached = specs
        .iter()
        .enumerate()
        .map(|(i, spec)| Attached {
            device: spec.device.clone(),
            instance: GUID::from_u128(0x7265612d_6661_6b65_0000_000000000001 + i as u128),
            fake: Some(i),
        })
        .collect();
    let devices = specs
        .into_iter()
        .map(|spec| State {
            axes: spec.rest,
            spec,
            buttons: 0,
            pov: u32::MAX,
            acquired: false,
            unplugged: false,
            epoch: 0,
        })
        .collect();
    *lock(&FAKES) = Some(Fakes {
        devices,
        events: Vec::new(),
    });
    attached
}

/// A new object for fake device `index`.
pub(super) fn open(index: usize) -> Option<Com<DeviceVtbl>> {
    // SAFETY: a new object with one reference, whose vtable is a
    // DeviceVtbl.
    unsafe { Com::from_raw(DeviceObject::create(&DEVICE, index)) }
}

/// Runs `f` on the fakes and device `index`.
fn with<R: Copy>(index: usize, failed: R, f: impl FnOnce(&mut Fakes, usize) -> R) -> R {
    guard(failed, || match lock(&FAKES).as_mut() {
        Some(fakes) if index < fakes.devices.len() => f(fakes, index),
        _ => failed,
    })
}

/// Moves fake device `device`: its eight axes (at the real range), its
/// buttons (bit n: button n) and its hat. 0 when it worked.
///
/// # Safety
/// `axes` points to eight values.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn rea_dinput_fake_input(
    device: u32,
    axes: *const i32,
    buttons: u32,
    pov: u32,
) -> i32 {
    if axes.is_null() {
        return -1;
    }
    // SAFETY: the caller's contract.
    let axes = unsafe { std::ptr::read_unaligned(axes.cast::<[i32; 8]>()) };
    with(device as usize, -1, |fakes, i| {
        let d = &mut fakes.devices[i];
        (d.axes, d.buttons, d.pov) = (axes, buttons, pov);
        0
    })
}

/// Unplugs fake device `device` (`unplugged` ≠ 0) or plugs it back:
/// unplugged it can't be acquired or read, and its effects stop.
#[unsafe(no_mangle)]
pub extern "system" fn rea_dinput_fake_unplug(device: u32, unplugged: i32) -> i32 {
    with(device as usize, -1, |fakes, i| {
        let d = &mut fakes.devices[i];
        d.unplugged = unplugged != 0;
        if d.unplugged {
            d.acquired = false;
            d.epoch += 1;
        }
        record(
            fakes,
            i,
            if unplugged != 0 {
                "unplugged"
            } else {
                "plugged"
            }
            .into(),
        );
        0
    })
}

/// The calls that reached the fakes since the last time, one per line,
/// into `buffer` (`size` bytes, no NUL). Lines that don't fit stay for
/// the next call. The bytes written.
///
/// # Safety
/// `buffer` is writable for `size` bytes.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn rea_dinput_fake_events(buffer: *mut u8, size: u32) -> u32 {
    if buffer.is_null() {
        return 0;
    }
    guard(0, || {
        let mut guard = lock(&FAKES);
        let Some(fakes) = guard.as_mut() else {
            return 0;
        };
        let mut text = String::new();
        let mut taken = 0;
        for line in &fakes.events {
            if text.len() + line.len() + 1 > size as usize {
                break;
            }
            text.push_str(line);
            text.push('\n');
            taken += 1;
        }
        fakes.events.drain(..taken);
        // SAFETY: the caller's contract; text fits in `size`.
        unsafe { std::ptr::copy_nonoverlapping(text.as_ptr(), buffer, text.len()) };
        text.len() as u32
    })
}

type DeviceObject = Object<DeviceVtbl, usize>;

/// # Safety
/// `this` is one of this module's devices.
unsafe fn index(this: This) -> usize {
    // SAFETY: the caller's contract.
    unsafe { DeviceObject::get(this).value }
}

static DEVICE: DeviceVtbl = DeviceVtbl {
    base: UnknownVtbl {
        query_interface: device_query,
        add_ref: device_add_ref,
        release: device_release,
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

unsafe extern "system" fn device_query(this: This, riid: *const GUID, out: *mut This) -> HRESULT {
    // SAFETY: a call on our object.
    unsafe { query::<DeviceVtbl, usize>(this, riid, out, &[IID_UNKNOWN, IID_DEVICE8A]) }
}

unsafe extern "system" fn device_add_ref(this: This) -> u32 {
    // SAFETY: a call on our object.
    unsafe { DeviceObject::get(this) }.add_ref()
}

unsafe extern "system" fn device_release(this: This) -> u32 {
    // SAFETY: a call on our object.
    unsafe { DeviceObject::release(this) }
}

unsafe extern "system" fn get_capabilities(this: This, caps: *mut DIDEVCAPS) -> HRESULT {
    // SAFETY: a call on our object; the layer's structure.
    unsafe {
        with(index(this), hr::GENERIC, |fakes, i| {
            let device = &fakes.devices[i].spec.device;
            *caps = DIDEVCAPS {
                dwSize: size_of::<DIDEVCAPS>() as u32,
                dwFlags: DIDC_ATTACHED | if device.ffb { DIDC_FORCEFEEDBACK } else { 0 },
                dwDevType: fakes.devices[i].spec.dev_type,
                dwAxes: device.axes as u32,
                dwButtons: device.buttons as u32,
                dwPOVs: 1,
                ..DIDEVCAPS::default()
            };
            hr::OK
        })
    }
}

unsafe extern "system" fn get_property(_: This, _: *const GUID, _: *mut DIPROPHEADER) -> HRESULT {
    hr::UNSUPPORTED
}

unsafe extern "system" fn set_property(
    this: This,
    id: *const GUID,
    header: *const DIPROPHEADER,
) -> HRESULT {
    // SAFETY: a call on our object; the layer's property structure.
    unsafe {
        with(index(this), hr::GENERIC, |fakes, i| {
            let text = match id as usize {
                PROP_RANGE => {
                    let range = &*header.cast::<DIPROPRANGE>();
                    format!("range {} {}", range.lMin, range.lMax)
                }
                PROP_AUTOCENTER => format!("autocenter {}", (*header.cast::<DIPROPDWORD>()).dwData),
                PROP_FFGAIN => format!("gain {}", (*header.cast::<DIPROPDWORD>()).dwData),
                _ => return hr::UNSUPPORTED,
            };
            record(fakes, i, text);
            hr::OK
        })
    }
}

unsafe extern "system" fn acquire(this: This) -> HRESULT {
    // SAFETY: a call on our object.
    with(unsafe { index(this) }, hr::GENERIC, |fakes, i| {
        let d = &mut fakes.devices[i];
        if d.unplugged {
            return hr::INPUTLOST;
        }
        d.acquired = true;
        record(fakes, i, "acquire".into());
        hr::OK
    })
}

unsafe extern "system" fn unacquire(this: This) -> HRESULT {
    // SAFETY: a call on our object.
    with(unsafe { index(this) }, hr::GENERIC, |fakes, i| {
        fakes.devices[i].acquired = false;
        hr::OK
    })
}

unsafe extern "system" fn get_device_state(this: This, size: u32, data: *mut c_void) -> HRESULT {
    // SAFETY: a call on our object; the layer's DIJOYSTATE2.
    with(unsafe { index(this) }, hr::GENERIC, |fakes, i| {
        let d = &fakes.devices[i];
        if d.unplugged {
            return hr::INPUTLOST;
        }
        if !d.acquired {
            return hr::NOTACQUIRED;
        }
        if size != JOYSTATE2_SIZE {
            return hr::INVALIDPARAM;
        }
        // SAFETY: a zeroed DIJOYSTATE2 is valid.
        let mut state: DIJOYSTATE2 = unsafe { std::mem::zeroed() };
        let [x, y, z, rx, ry, rz, s0, s1] = d.axes;
        (
            state.lX, state.lY, state.lZ, state.lRx, state.lRy, state.lRz,
        ) = (x, y, z, rx, ry, rz);
        state.rglSlider = [s0, s1];
        state.rgdwPOV = [d.pov, u32::MAX, u32::MAX, u32::MAX];
        for (n, button) in state
            .rgbButtons
            .iter_mut()
            .enumerate()
            .take(d.spec.device.buttons)
        {
            *button = if d.buttons & (1 << n) != 0 { 0x80 } else { 0 };
        }
        // SAFETY: the layer's buffer of JOYSTATE2_SIZE bytes.
        unsafe { data.cast::<DIJOYSTATE2>().write_unaligned(state) };
        hr::OK
    })
}

unsafe extern "system" fn set_data_format(this: This, format: *const DIDATAFORMAT) -> HRESULT {
    // SAFETY: a call on our object; the layer's format.
    unsafe {
        with(index(this), hr::GENERIC, |fakes, i| {
            let df = &*format;
            if df.dwDataSize != JOYSTATE2_SIZE {
                return hr::INVALIDPARAM;
            }
            record(fakes, i, format!("format {} objects", df.dwNumObjs));
            hr::OK
        })
    }
}

unsafe extern "system" fn set_cooperative_level(this: This, _: HWND, flags: u32) -> HRESULT {
    // SAFETY: a call on our object.
    with(unsafe { index(this) }, hr::GENERIC, |fakes, i| {
        let exclusive = flags & DISCL_EXCLUSIVE != 0;
        record(
            fakes,
            i,
            format!(
                "cooperate {}",
                if exclusive { "exclusive" } else { "shared" }
            ),
        );
        hr::OK
    })
}

unsafe extern "system" fn poll(this: This) -> HRESULT {
    // SAFETY: a call on our object.
    with(unsafe { index(this) }, hr::GENERIC, |fakes, i| {
        if fakes.devices[i].acquired {
            hr::OK
        } else {
            hr::NOTACQUIRED
        }
    })
}

unsafe extern "system" fn get_force_feedback_state(_: This, state: *mut u32) -> HRESULT {
    // SAFETY: the layer's out value.
    unsafe { *state = DIGFFS_STOPPED | DIGFFS_ACTUATORSON | DIGFFS_POWERON };
    hr::OK
}

unsafe extern "system" fn send_force_feedback_command(this: This, command: u32) -> HRESULT {
    // SAFETY: a call on our object.
    with(unsafe { index(this) }, hr::GENERIC, |fakes, i| {
        record(fakes, i, format!("command {command:#x}"));
        hr::OK
    })
}

/// An axis of an effect the layer made: `lX` of `c_dfDIJoystick2`.
fn joystate2_axis(by_id: bool, value: u32) -> Option<Axis> {
    let part = if by_id {
        Part::by_id(value)
    } else {
        Part::at_joystate2(value)
    };
    match part {
        Some(Part::Axis(axis)) => Some(axis),
        _ => None,
    }
}

unsafe extern "system" fn create_effect(
    this: This,
    guid: *const GUID,
    params: *const DIEFFECT,
    out: *mut This,
    _: This,
) -> HRESULT {
    // SAFETY: a call on our object with the layer's GUID, parameters and
    // out pointer.
    unsafe {
        with(index(this), hr::GENERIC, |fakes, i| {
            if !fakes.devices[i].spec.device.ffb {
                return hr::UNSUPPORTED;
            }
            let Some(kind) = effect::kind_of(&*guid) else {
                return hr::DEVICENOTREG;
            };
            let made =
                match effect::read(&Effect::new(kind), params, DIEP_ALLPARAMS, joystate2_axis) {
                    Ok(made) => made,
                    Err(code) => return code,
                };
            record(fakes, i, format!("create {}", made.describe()));
            *out = EffectObject::create(
                &EFFECT,
                FakeEffect {
                    device: i,
                    effect: Mutex::new(made),
                    started: Mutex::new(None),
                },
            );
            hr::OK
        })
    }
}

macro_rules! unsupported {
    ($($name:ident($($arg:ty),*);)*) => {
        $(unsafe extern "system" fn $name(_: This, $(_: $arg),*) -> HRESULT {
            hr::UNSUPPORTED
        })*
    };
}

unsupported! {
    enum_objects(LPDIENUMDEVICEOBJECTSCALLBACKA, *mut c_void, u32);
    get_device_data(u32, *mut DIDEVICEOBJECTDATA, *mut u32, u32);
    set_event_notification(HANDLE);
    get_object_info(*mut DIDEVICEOBJECTINSTANCEA, u32, u32);
    get_device_info(*mut DIDEVICEINSTANCEA);
    run_control_panel(HWND, u32);
    initialize(HINSTANCE, u32, *const GUID);
    enum_effects(LPDIENUMEFFECTSCALLBACKA, *mut c_void, u32);
    get_effect_info(*mut DIEFFECTINFOA, *const GUID);
    enum_created_effect_objects(LPDIENUMCREATEDEFFECTOBJECTSCALLBACK, *mut c_void, u32);
    escape(*mut DIEFFESCAPE);
    send_device_data(u32, *const DIDEVICEOBJECTDATA, *mut u32, u32);
    enum_effects_in_file(*const u8, *mut c_void, *mut c_void, u32);
    write_effect_to_file(*const u8, u32, *mut c_void, u32);
    build_action_map(*mut c_void, *const u8, u32);
    set_action_map(*mut c_void, *const u8, u32);
    get_image_info(*mut c_void);
    effect_initialize(HINSTANCE, u32, *const GUID);
    effect_get_guid(*mut GUID);
    effect_get_parameters(*mut DIEFFECT, u32);
    effect_escape(*mut DIEFFESCAPE);
}

/// A fake device's effect: what the layer set, and the device epoch it
/// was started in.
struct FakeEffect {
    device: usize,
    effect: Mutex<Effect>,
    started: Mutex<Option<u32>>,
}

type EffectObject = Object<EffectVtbl, FakeEffect>;

static EFFECT: EffectVtbl = EffectVtbl {
    base: UnknownVtbl {
        query_interface: effect_query,
        add_ref: effect_add_ref,
        release: effect_release,
    },
    initialize: effect_initialize,
    get_effect_guid: effect_get_guid,
    get_parameters: effect_get_parameters,
    set_parameters: effect_set_parameters,
    start: effect_start,
    stop: effect_stop,
    get_effect_status: effect_status,
    download: effect_download,
    unload: effect_unload,
    escape: effect_escape,
};

/// # Safety
/// `this` is one of this module's effects.
unsafe fn fake_effect<'a>(this: This) -> &'a FakeEffect {
    // SAFETY: the caller's contract.
    unsafe { &EffectObject::get(this).value }
}

unsafe extern "system" fn effect_query(this: This, riid: *const GUID, out: *mut This) -> HRESULT {
    // SAFETY: a call on our object.
    unsafe { query::<EffectVtbl, FakeEffect>(this, riid, out, &[IID_UNKNOWN, IID_EFFECT]) }
}

unsafe extern "system" fn effect_add_ref(this: This) -> u32 {
    // SAFETY: a call on our object.
    unsafe { EffectObject::get(this) }.add_ref()
}

unsafe extern "system" fn effect_release(this: This) -> u32 {
    // SAFETY: a call on our object.
    unsafe { EffectObject::release(this) }
}

unsafe extern "system" fn effect_set_parameters(
    this: This,
    params: *const DIEFFECT,
    flags: u32,
) -> HRESULT {
    // SAFETY: a call on our object with the layer's parameters.
    unsafe {
        let e = fake_effect(this);
        let mut current = lock(&e.effect);
        match effect::read(&current, params, flags, joystate2_axis) {
            Ok(made) => {
                *current = made;
                with(e.device, hr::GENERIC, |fakes, i| {
                    record(fakes, i, format!("set {flags:#x} {}", current.describe()));
                    hr::OK
                })
            }
            Err(code) => code,
        }
    }
}

/// Records `what` for the effect's device, noting a start in its epoch.
///
/// # Safety
/// `this` is one of this module's effects.
unsafe fn effect_event(this: This, what: &str, start: Option<bool>) -> HRESULT {
    // SAFETY: the caller's contract.
    let e = unsafe { fake_effect(this) };
    let kind = lock(&e.effect).kind.name();
    with(e.device, hr::GENERIC, |fakes, i| {
        if fakes.devices[i].unplugged {
            return hr::INPUTLOST;
        }
        match start {
            Some(true) => *lock(&e.started) = Some(fakes.devices[i].epoch),
            Some(false) => *lock(&e.started) = None,
            None => {}
        }
        record(fakes, i, format!("{what} {kind}"));
        hr::OK
    })
}

unsafe extern "system" fn effect_start(this: This, iterations: u32, _: u32) -> HRESULT {
    // SAFETY: a call on our object.
    unsafe { effect_event(this, &format!("start {iterations}"), Some(true)) }
}

unsafe extern "system" fn effect_stop(this: This) -> HRESULT {
    // SAFETY: a call on our object.
    unsafe { effect_event(this, "stop", Some(false)) }
}

unsafe extern "system" fn effect_download(this: This) -> HRESULT {
    // SAFETY: a call on our object.
    unsafe { effect_event(this, "download", None) }
}

unsafe extern "system" fn effect_unload(this: This) -> HRESULT {
    // SAFETY: a call on our object.
    unsafe { effect_event(this, "unload", Some(false)) }
}

unsafe extern "system" fn effect_status(this: This, status: *mut u32) -> HRESULT {
    // SAFETY: a call on our object; the layer's out value.
    unsafe {
        let e = fake_effect(this);
        let started = *lock(&e.started);
        let epoch = with(e.device, None, |fakes, i| Some(fakes.devices[i].epoch));
        *status = if started.is_some() && started == epoch {
            DIEGES_PLAYING
        } else {
            0
        };
    }
    hr::OK
}
