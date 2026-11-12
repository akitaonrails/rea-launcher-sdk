//! Wheels, pedals and pads through SDL2's joystick, game-controller and
//! haptic APIs (DirectInput/XInput on Windows, evdev on Linux). From
//! rea-rad-mobile `input/{sdl,haptic}.rs`, with three fixes (study §5):
//! a pad from a wheel vendor keeps its pad view ([`wheel::classify`]), a
//! plug event inside the rescan throttle is not lost, and a device that
//! refuses force feedback is not reopened every tick.
//!
//! Everything here lives on one thread, the one that called
//! [`Sdl::init`]: [`Sdl`], [`Devices`] and [`Device`] are neither `Send`
//! nor `Sync`, and `Devices` borrows `Sdl`, so SDL is never shut down
//! under an open device.
//!
//! ```no_run
//! use rea_launcher_native::sdl::{Devices, Sdl};
//! use rea_launcher_native::policy::HapticPolicy;
//! let sdl = Sdl::init(&[]).expect("SDL");
//! let mut devices = Devices::new(&sdl, HapticPolicy::default());
//! loop {
//!     if devices.pump() {
//!         for d in devices.list() {
//!             println!("{:?} {} {:04x}:{:04x}", d.kind, d.id.name, d.id.vid, d.id.pid);
//!         }
//!     }
//!     std::thread::sleep(std::time::Duration::from_millis(4));
//! }
//! ```

mod haptic;

pub use crate::policy::{EffectKind, EffectParams, HapticPolicy, effect_params};
pub use haptic::Haptic;

use std::ffi::{CStr, CString, c_char};
use std::marker::PhantomData;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use rea_launcher::ffb::FfbOutput;
use rea_launcher::wheel::{
    self, DeviceFacts, DeviceId, DeviceShape, DeviceState, Kind, PadState, pad_map,
};
use sdl2::sys;

use crate::policy::{
    RESCAN_EVERY, RUMBLE_EVERY, RUMBLE_HOLD_MS, Rescan, Retry, RumbleLimit, rumble_level,
};

/// The hints [`Sdl::init`] always sets:
/// - background events: the emulator's window has the focus while the
///   launcher reads the wheel;
/// - no RawInput on Windows (DirectInput carries wheels and their force
///   feedback);
/// - no signal handlers: SDL would turn SIGINT/SIGTERM into a quit event
///   nobody reads.
///
/// rea-rad-mobile also set `SDL_JOYSTICK_HIDAPI_WHEELS`, which no SDL2
/// release defines (it did nothing); it is left out.
pub const HINTS: [(&str, &str); 3] = [
    ("SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS", "1"),
    ("SDL_JOYSTICK_RAWINPUT", "0"),
    ("SDL_NO_SIGNAL_HANDLERS", "1"),
];

static INITIALISED: AtomicBool = AtomicBool::new(false);

/// SDL with the joystick, game-controller, haptic and event subsystems,
/// no video. One at a time per process; dropping it shuts SDL down.
pub struct Sdl {
    /// Not `Send`/`Sync`: SDL's input lives on one thread.
    thread_bound: PhantomData<*const ()>,
}

impl Sdl {
    /// Sets [`HINTS`] and `extra_hints`, then starts SDL.
    pub fn init(extra_hints: &[(&str, &str)]) -> Result<Sdl, String> {
        if INITIALISED.swap(true, Ordering::SeqCst) {
            return Err("SDL is already started in this process".into());
        }
        let started = (|| {
            let c_string = |s: &str| CString::new(s).map_err(|e| format!("hint {s:?}: {e}"));
            for &(k, v) in HINTS.iter().chain(extra_hints) {
                let (k, v) = (c_string(k)?, c_string(v)?);
                // SAFETY: valid C strings; SDL copies them.
                unsafe { sys::SDL_SetHint(k.as_ptr(), v.as_ptr()) };
            }
            let flags = sys::SDL_INIT_JOYSTICK
                | sys::SDL_INIT_GAMECONTROLLER
                | sys::SDL_INIT_HAPTIC
                | sys::SDL_INIT_EVENTS;
            // SAFETY: a plain init call.
            if unsafe { sys::SDL_Init(flags) } != 0 {
                return Err(format!("SDL_Init: {}", error()));
            }
            Ok(())
        })();
        match started {
            Ok(()) => Ok(Sdl {
                thread_bound: PhantomData,
            }),
            Err(e) => {
                INITIALISED.store(false, Ordering::SeqCst);
                Err(e)
            }
        }
    }

    /// How many joysticks SDL sees.
    fn device_count(&self) -> usize {
        // SAFETY: SDL is started (self exists).
        unsafe { sys::SDL_NumJoysticks().max(0) as usize }
    }

    /// Handles pending events and refreshes every device's state; true
    /// when a device was added or removed.
    fn pump_events(&self) -> bool {
        let mut changed = false;
        // SAFETY: SDL is started; the event is plain data SDL fills.
        unsafe {
            sys::SDL_PumpEvents();
            let mut ev = std::mem::zeroed::<sys::SDL_Event>();
            while sys::SDL_PollEvent(&mut ev) != 0 {
                let t = ev.type_;
                if t == sys::SDL_EventType::SDL_JOYDEVICEADDED as u32
                    || t == sys::SDL_EventType::SDL_JOYDEVICEREMOVED as u32
                {
                    changed = true;
                }
            }
            sys::SDL_JoystickUpdate();
            sys::SDL_GameControllerUpdate();
        }
        changed
    }
}

impl Drop for Sdl {
    fn drop(&mut self) {
        // SAFETY: every Device and Haptic borrows from this Sdl's lifetime
        // through Devices, so they are closed already.
        unsafe { sys::SDL_Quit() };
        INITIALISED.store(false, Ordering::SeqCst);
    }
}

/// SDL's last error message.
pub fn error() -> String {
    // SAFETY: SDL_GetError returns a valid C string.
    cstr(unsafe { sys::SDL_GetError() })
}

fn cstr(p: *const c_char) -> String {
    if p.is_null() {
        String::new()
    } else {
        // SAFETY: SDL returns a valid C string or null.
        unsafe { CStr::from_ptr(p).to_string_lossy().into_owned() }
    }
}

/// An opened joystick, and its game-controller view when it is a pad.
pub struct Device {
    joy: NonNull<sys::SDL_Joystick>,
    controller: Option<NonNull<sys::_SDL_GameController>>,
    rumble: RumbleLimit,
    pub instance: i32,
    pub id: DeviceId,
    /// The OS device path: tells two collections of one wheel apart.
    pub path: String,
    /// It takes force feedback.
    pub haptic: bool,
    pub kind: Kind,
    /// The axes when SDL opened it (pedals at rest).
    pub initial: DeviceState,
    pub state: DeviceState,
    /// The game-controller view of a pad.
    pub pad: Option<PadState>,
    /// The first hat, (x, y) each −1, 0 or 1, y down.
    pub hat: (i8, i8),
}

impl Device {
    /// Opens joystick `index` (0..device count).
    fn open(index: usize) -> Option<Device> {
        let index = index as i32;
        // SAFETY: SDL is started (Devices borrows Sdl); index is in range
        // and every null is handled.
        unsafe {
            let joy = NonNull::new(sys::SDL_JoystickOpen(index))?;
            let j = joy.as_ptr();
            let mut guid = [0 as c_char; 64];
            sys::SDL_JoystickGetGUIDString(
                sys::SDL_JoystickGetGUID(j),
                guid.as_mut_ptr(),
                guid.len() as i32,
            );
            let id = DeviceId {
                name: cstr(sys::SDL_JoystickName(j)),
                vid: sys::SDL_JoystickGetVendor(j),
                pid: sys::SDL_JoystickGetProduct(j),
                guid: cstr(guid.as_ptr()),
            };
            let axes = sys::SDL_JoystickNumAxes(j).max(0);
            let kind = wheel::classify(&DeviceFacts {
                vid: id.vid,
                pid: id.pid,
                sdl_wheel_type: sys::SDL_JoystickGetType(j)
                    == sys::SDL_JoystickType::SDL_JOYSTICK_TYPE_WHEEL,
                game_controller: sys::SDL_IsGameController(index) == sys::SDL_bool::SDL_TRUE,
                axes: axes as usize,
            });
            let controller = if kind == Kind::Pad {
                NonNull::new(sys::SDL_GameControllerOpen(index))
            } else {
                None
            };
            let mut initial = DeviceState {
                axes: Vec::with_capacity(axes as usize),
                buttons: vec![false; sys::SDL_JoystickNumButtons(j).max(0) as usize],
            };
            for a in 0..axes {
                let mut v = 0i16;
                let has =
                    sys::SDL_JoystickGetAxisInitialState(j, a, &mut v) == sys::SDL_bool::SDL_TRUE;
                initial.axes.push(if has {
                    v
                } else {
                    sys::SDL_JoystickGetAxis(j, a)
                });
            }
            let mut dev = Device {
                joy,
                controller,
                rumble: RumbleLimit::new(RUMBLE_EVERY),
                instance: sys::SDL_JoystickInstanceID(j),
                id,
                path: cstr(sys::SDL_JoystickPath(j)),
                haptic: sys::SDL_JoystickIsHaptic(j) == 1,
                kind: if kind == Kind::Pad && controller.is_none() {
                    Kind::Other
                } else {
                    kind
                },
                state: initial.clone(),
                initial,
                pad: None,
                hat: (0, 0),
            };
            dev.read();
            Some(dev)
        }
    }

    fn raw(&self) -> *mut sys::SDL_Joystick {
        self.joy.as_ptr()
    }

    pub fn is_attached(&self) -> bool {
        // SAFETY: the joystick is open while self lives.
        unsafe { sys::SDL_JoystickGetAttached(self.raw()) == sys::SDL_bool::SDL_TRUE }
    }

    /// Copies the current readings (after a pump).
    fn read(&mut self) {
        let j = self.raw();
        // SAFETY: the joystick and controller are open while self lives.
        unsafe {
            for (i, v) in self.state.axes.iter_mut().enumerate() {
                *v = sys::SDL_JoystickGetAxis(j, i as i32);
            }
            for (i, b) in self.state.buttons.iter_mut().enumerate() {
                *b = sys::SDL_JoystickGetButton(j, i as i32) != 0;
            }
            let hat = if sys::SDL_JoystickNumHats(j) > 0 {
                u32::from(sys::SDL_JoystickGetHat(j, 0))
            } else {
                0
            };
            let dir = |neg: u32, pos: u32| -> i8 {
                if hat & neg != 0 {
                    -1
                } else if hat & pos != 0 {
                    1
                } else {
                    0
                }
            };
            self.hat = (
                dir(sys::SDL_HAT_LEFT, sys::SDL_HAT_RIGHT),
                dir(sys::SDL_HAT_UP, sys::SDL_HAT_DOWN),
            );
            if let Some(pad) = self.controller {
                use sys::SDL_GameControllerAxis as A;
                use sys::SDL_GameControllerButton as B;
                let p = pad.as_ptr();
                let axis = |a: A| sys::SDL_GameControllerGetAxis(p, a);
                let button = |b: B| sys::SDL_GameControllerGetButton(p, b) != 0;
                self.pad = Some(PadState {
                    left_x: axis(A::SDL_CONTROLLER_AXIS_LEFTX),
                    left_y: axis(A::SDL_CONTROLLER_AXIS_LEFTY),
                    left_trigger: axis(A::SDL_CONTROLLER_AXIS_TRIGGERLEFT),
                    right_trigger: axis(A::SDL_CONTROLLER_AXIS_TRIGGERRIGHT),
                    a: button(B::SDL_CONTROLLER_BUTTON_A),
                    b: button(B::SDL_CONTROLLER_BUTTON_B),
                    x: button(B::SDL_CONTROLLER_BUTTON_X),
                    y: button(B::SDL_CONTROLLER_BUTTON_Y),
                    start: button(B::SDL_CONTROLLER_BUTTON_START),
                    back: button(B::SDL_CONTROLLER_BUTTON_BACK),
                    guide: button(B::SDL_CONTROLLER_BUTTON_GUIDE),
                    left_shoulder: button(B::SDL_CONTROLLER_BUTTON_LEFTSHOULDER),
                    right_shoulder: button(B::SDL_CONTROLLER_BUTTON_RIGHTSHOULDER),
                    left_stick: button(B::SDL_CONTROLLER_BUTTON_LEFTSTICK),
                    right_stick: button(B::SDL_CONTROLLER_BUTTON_RIGHTSTICK),
                    dpad_left: button(B::SDL_CONTROLLER_BUTTON_DPAD_LEFT),
                    dpad_right: button(B::SDL_CONTROLLER_BUTTON_DPAD_RIGHT),
                    dpad_up: button(B::SDL_CONTROLLER_BUTTON_DPAD_UP),
                    dpad_down: button(B::SDL_CONTROLLER_BUTTON_DPAD_DOWN),
                });
            }
        }
    }

    /// Menu navigation in XInput bits ([`rea_launcher::pad`]): the pad's
    /// buttons and stick, and the hat (a wheel's d-pad).
    pub fn nav_bits(&self) -> u16 {
        self.pad.map_or(0, |p| p.nav_bits()) | pad_map::hat_bits(self.hat)
    }

    /// Pad rumble, both motors at `strength` (0..1). Called every tick, it
    /// reaches the pad only on a start, a stop, or every
    /// [`RUMBLE_EVERY`](crate::policy::RUMBLE_EVERY).
    pub fn rumble(&mut self, strength: f32) {
        let level = rumble_level(strength);
        if let Some(pad) = self.controller
            && self.rumble.due(level, Instant::now())
        {
            // SAFETY: the controller is open while self lives.
            unsafe { sys::SDL_GameControllerRumble(pad.as_ptr(), level, level, RUMBLE_HOLD_MS) };
        }
    }

    fn shape(&self) -> DeviceShape {
        DeviceShape {
            vid: self.id.vid,
            pid: self.id.pid,
            kind: self.kind,
            axes: self.state.axes.len(),
        }
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // SAFETY: we opened these and close each once.
        unsafe {
            if let Some(pad) = self.controller {
                sys::SDL_GameControllerClose(pad.as_ptr());
            }
            sys::SDL_JoystickClose(self.raw());
        }
    }
}

/// Every device SDL sees (spare HID interfaces left out), kept current by
/// [`Devices::pump`], and the force feedback of the one wheel in use.
pub struct Devices<'sdl> {
    sdl: &'sdl Sdl,
    /// Declared first: dropped (closed) before the devices it came from.
    haptic: Option<Haptic>,
    retry: Retry,
    policy: HapticPolicy,
    list: Vec<Device>,
    skipped: Vec<DeviceId>,
    rescan: Rescan,
}

impl<'sdl> Devices<'sdl> {
    /// Opens every device now.
    pub fn new(sdl: &'sdl Sdl, policy: HapticPolicy) -> Devices<'sdl> {
        let mut devices = Devices {
            sdl,
            haptic: None,
            retry: Retry::default(),
            policy,
            list: Vec::new(),
            skipped: Vec::new(),
            rescan: Rescan::new(RESCAN_EVERY),
        };
        sdl.pump_events();
        devices.scan();
        devices.rescan.done(Instant::now());
        devices
    }

    /// Handles SDL's events and reads every device; true when the list
    /// changed (a rescan at most every [`RESCAN_EVERY`], and an event
    /// inside that window is kept for the next one).
    pub fn pump(&mut self) -> bool {
        let changed = self.sdl.pump_events();
        let lost = self.list.iter().any(|d| !d.is_attached());
        self.rescan.note(changed || lost);
        let rescanned = self.rescan.due(Instant::now());
        if rescanned {
            self.scan();
        }
        for d in &mut self.list {
            d.read();
        }
        rescanned
    }

    fn scan(&mut self) {
        // force feedback first: SDL wants it closed before its joystick
        self.haptic = None;
        self.retry.clear();
        self.list.clear();
        let all: Vec<Device> = (0..self.sdl.device_count())
            .filter_map(Device::open)
            .collect();
        let shapes: Vec<DeviceShape> = all.iter().map(Device::shape).collect();
        self.skipped.clear();
        for (i, d) in all.into_iter().enumerate() {
            if wheel::is_spare_interface(&shapes, i) {
                self.skipped.push(d.id.clone());
            } else {
                self.list.push(d);
            }
        }
    }

    pub fn list(&self) -> &[Device] {
        &self.list
    }

    /// Devices left out as spare interfaces of another one (for the log).
    pub fn skipped(&self) -> &[DeviceId] {
        &self.skipped
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut Device> {
        self.list.get_mut(index)
    }

    /// Drives the force feedback of the wheel at `index`, opening it the
    /// first time (or after a rescan, or [`HapticPolicy::retry_after`]
    /// after it refused). Returns a line for the log when it opened or
    /// was refused.
    pub fn force_feedback(&mut self, index: usize, out: &FfbOutput) -> Option<String> {
        let device = self.list.get(index)?;
        let mut note = None;
        if self
            .haptic
            .as_ref()
            .is_none_or(|h| h.instance() != device.instance)
        {
            self.haptic = None;
            let now = Instant::now();
            if !self
                .retry
                .may_try(device.instance, now, self.policy.retry_after)
            {
                return None;
            }
            match Haptic::open(device, self.policy) {
                Ok(h) => {
                    note = Some(format!(
                        "ffb: {} on \"{}\"",
                        h.description(),
                        device.id.name
                    ));
                    self.retry.clear();
                    self.haptic = Some(h);
                }
                Err(e) => {
                    self.retry.failed(device.instance, now);
                    return Some(format!("ffb: \"{}\": {e}", device.id.name));
                }
            }
        }
        if let Some(h) = &mut self.haptic {
            h.apply(out);
        }
        note
    }

    /// Releases the wheel: no force, effects stopped and closed (a pad
    /// took over, the game ended).
    pub fn release_force_feedback(&mut self) {
        if let Some(mut h) = self.haptic.take() {
            h.stop();
        }
    }

    /// The effects running on the device at `index`, if it is the one
    /// with force feedback open.
    pub fn force_feedback_description(&self, index: usize) -> Option<&str> {
        let device = self.list.get(index)?;
        self.haptic
            .as_ref()
            .filter(|h| h.instance() == device.instance)
            .map(Haptic::description)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SDL starts without a display (no video subsystem), once per
    /// process; device lists are whatever this machine has, so only the
    /// shape is checked.
    #[test]
    fn sdl_starts_headless_once_and_lists_devices() {
        let sdl = Sdl::init(&[("SDL_JOYSTICK_HIDAPI", "0")]).expect("SDL starts without a display");
        assert!(Sdl::init(&[]).is_err(), "a second SDL is refused");
        let mut devices = Devices::new(&sdl, HapticPolicy::default());
        devices.pump();
        for (i, d) in devices.list().iter().enumerate() {
            assert!(d.is_attached());
            assert_eq!(d.state.axes.len(), d.initial.axes.len());
            assert_eq!(d.pad.is_some(), d.kind == Kind::Pad);
            assert!(
                devices.force_feedback_description(i).is_none(),
                "nothing opened yet"
            );
        }
        assert!(
            devices
                .force_feedback(usize::MAX, &FfbOutput::default())
                .is_none()
        );
        devices.release_force_feedback();
        drop(devices);
        drop(sdl);
        let again = Sdl::init(&[]).expect("SDL starts again after a drop");
        drop(again);
        assert!(Sdl::init(&[("bad\0hint", "1")]).is_err());
        assert!(Sdl::init(&[]).is_ok(), "a refused start leaves SDL free");
    }
}
