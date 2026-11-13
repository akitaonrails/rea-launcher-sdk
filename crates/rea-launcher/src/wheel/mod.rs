//! Wheels, pedals and gamepads turned into finished controls, the way
//! Wanszai's arcade wrappers do it: the launcher reads every device and
//! hands the game steering, pedals and buttons after steering lock, curves
//! and calibration (docs/playbooks/wheels-and-ffb.md).
//!
//! From rea-rad-mobile (`radmobile-core/src/wheel/{mod,known,wizard}.rs`,
//! `pad_map.rs`, the pure helpers of `radmobile-launcher/src/input/mod.rs`),
//! made generic: the launcher's own buttons come in through the [`Control`]
//! trait, so the game knowledge (its buttons, their keys and labels, which
//! button the shift paddles become, the wizard's steps, the default pad
//! layout) stays in the launcher. Two fixes on the way: a gamepad from a
//! wheel vendor (a Logitech F310) is a pad, not a wheel ([`classify`]), and
//! the wizard's armed pedal times out too ([`wizard`]).
//!
//! The device layer (SDL, haptics) is `rea-launcher-native`'s `sdl`
//! feature; everything here is pure and host-tested.
//!
//! ```
//! use rea_launcher::wheel::{self, Control, DeviceId, DeviceState, WheelProfile, WheelSettings};
//!
//! #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
//! enum Button { ShiftUp, ShiftDown, Start }
//!
//! impl Control for Button {
//!     const ALL: &'static [Button] = &[Button::ShiftUp, Button::ShiftDown, Button::Start];
//!     fn key(self) -> &'static str {
//!         match self { Button::ShiftUp => "shift_up", Button::ShiftDown => "shift_down", Button::Start => "start" }
//!     }
//!     fn label(self) -> &'static str {
//!         match self { Button::ShiftUp => "Shift up", Button::ShiftDown => "Shift down", Button::Start => "Start" }
//!     }
//!     fn bit(self) -> u32 { 1 << (self as u32) }
//! }
//!
//! let csl = DeviceId { name: "FANATEC CSL DD".into(), vid: 0x0eb7, pid: 0x0020, guid: String::new() };
//! let resting = DeviceState { axes: vec![0, 32_767, 32_767], buttons: vec![false; 8] };
//! // Known rotation, pedals found where they rest, paddles as the gearbox.
//! let profile = WheelProfile::guess(csl, &resting, Some((Button::ShiftUp, Button::ShiftDown)));
//! let mut now = resting.clone();
//! now.axes[0] = 8_192; // a quarter turn right of a 1080° wheel
//! let controls = profile.read(&WheelSettings::default(), &now, None);
//! assert!((controls.steer - 1.0).abs() < 1e-3); // full lock at 270°
//! let saved = profile.to_cfg("", "wheel.ini - written by the launcher");
//! assert_eq!(WheelProfile::<Button>::from_cfg(&saved), Some(profile));
//! ```

pub mod dinput;
pub mod known;
pub mod pad_map;
pub mod wizard;

pub use pad_map::{PadInput, PadMap, PadSettings, PadState, read_pad};

use crate::cfg;

/// Raw axis full scale, as SDL reports it.
pub const RAW_MAX: f32 = 32767.0;

/// A device's raw state: axes in −32768..=32767, buttons pressed or not.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceState {
    pub axes: Vec<i16>,
    pub buttons: Vec<bool>,
}

impl DeviceState {
    pub fn axis(&self, i: usize) -> Option<i16> {
        self.axes.get(i).copied()
    }

    pub fn button(&self, i: usize) -> bool {
        self.buttons.get(i).copied().unwrap_or(false)
    }
}

/// What a device is, as far as a launcher cares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Wheel,
    Pad,
    /// Pedals on their own, shifters, button boxes, flight sticks.
    Other,
}

/// What the device layer reads about a device, for [`classify`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeviceFacts {
    pub vid: u16,
    pub pid: u16,
    /// SDL says it is a wheel (`SDL_JOYSTICK_TYPE_WHEEL`).
    pub sdl_wheel_type: bool,
    /// SDL has a game-controller mapping for it (`SDL_IsGameController`).
    pub game_controller: bool,
    pub axes: usize,
}

/// What a device is: a [`known`] wheel or one SDL calls a wheel, else a
/// game controller is a pad, else a device with axes from a wheel vendor
/// is a wheel (a model new to the table), else something else.
///
/// rea-rad-mobile asked the vendor first, so a Logitech F310 or a
/// Thrustmaster pad became a 900° wheel without its pad layout.
pub fn classify(f: &DeviceFacts) -> Kind {
    if known::lookup(f.vid, f.pid).is_some() || f.sdl_wheel_type {
        Kind::Wheel
    } else if f.game_controller {
        Kind::Pad
    } else if known::wheel_vendor(f.vid) && f.axes > 0 {
        Kind::Wheel
    } else {
        Kind::Other
    }
}

/// What [`is_spare_interface`] looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceShape {
    pub vid: u16,
    pub pid: u16,
    pub kind: Kind,
    pub axes: usize,
}

/// A device with no axes that is neither a wheel nor a pad, beside another
/// device of the same model that has axes: one of a gamepad's extra HID
/// interfaces (an 8BitDo pad shows up three times on Windows, twice as
/// such). Skip it; a lone button box of its own stays.
pub fn is_spare_interface(devices: &[DeviceShape], i: usize) -> bool {
    let Some(d) = devices.get(i) else {
        return false;
    };
    d.kind == Kind::Other
        && d.axes == 0
        && devices
            .iter()
            .enumerate()
            .any(|(j, o)| j != i && (o.vid, o.pid) == (d.vid, d.pid) && o.axes > 0)
}

/// A device's identity, for pinning it across runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceId {
    pub name: String,
    pub vid: u16,
    pub pid: u16,
    /// SDL's GUID string (stable for a model on a platform).
    pub guid: String,
}

/// The profile's second device (pedals, a button box) among `devices`,
/// never the wheel at `wheel`: the same GUID and model first, then the
/// same model.
pub fn find_aux(devices: &[DeviceId], aux: &DeviceId, wheel: usize) -> Option<usize> {
    let candidates = || devices.iter().enumerate().filter(|(i, _)| *i != wheel);
    let model = |d: &DeviceId| (d.vid, d.pid) == (aux.vid, aux.pid);
    candidates()
        .find(|(_, d)| model(d) && d.guid == aux.guid)
        .or_else(|| candidates().find(|(_, d)| model(d)))
        .map(|(i, _)| i)
}

/// A connected wheel, as [`pick_wheel`] weighs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WheelCandidate<'a> {
    pub vid: u16,
    pub pid: u16,
    /// The OS device path, which tells two HID collections apart.
    pub path: &'a str,
    /// It takes force feedback.
    pub haptic: bool,
}

/// Which wheel to drive with, as an index into `wheels`:
///
/// 1. the one picked by hand (`chosen`, a device path), when connected;
/// 2. otherwise among the saved profile's model (or every wheel), the
///    first that takes force feedback, then the first at all.
///
/// Some wheels show up twice, once per HID collection, and only the one
/// that takes force feedback reports input (Fanatec CSL DD on Windows).
pub fn pick_wheel(
    wheels: &[WheelCandidate],
    saved: Option<&DeviceId>,
    chosen: &str,
) -> Option<usize> {
    if !chosen.is_empty()
        && let Some(i) = wheels.iter().position(|w| w.path == chosen)
    {
        return Some(i);
    }
    let same_model = |w: &WheelCandidate| saved.is_some_and(|s| (s.vid, s.pid) == (w.vid, w.pid));
    let pool: Vec<usize> = if wheels.iter().any(same_model) {
        (0..wheels.len())
            .filter(|&i| same_model(&wheels[i]))
            .collect()
    } else {
        (0..wheels.len()).collect()
    };
    pool.iter()
        .copied()
        .find(|&i| wheels[i].haptic)
        .or(pool.first().copied())
}

/// Steering lock, in degrees of wheel rotation lock to lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SteeringLock {
    Degrees(u16),
    /// The wheel's whole rotation.
    Full,
}

impl SteeringLock {
    /// Wanszai's steps, in the order a menu cycles them.
    pub const STEPS: [SteeringLock; 6] = [
        SteeringLock::Degrees(180),
        SteeringLock::Degrees(270),
        SteeringLock::Degrees(360),
        SteeringLock::Degrees(540),
        SteeringLock::Degrees(900),
        SteeringLock::Full,
    ];

    /// 270°, an arcade cabinet's feel.
    pub const DEFAULT: SteeringLock = SteeringLock::Degrees(270);

    /// "270°" or "Full".
    pub fn label(self) -> String {
        match self {
            SteeringLock::Degrees(d) => format!("{d}°"),
            SteeringLock::Full => "Full".into(),
        }
    }

    /// Stored as degrees, 0 for Full; other values snap to the nearest step.
    pub fn from_setting(v: i64) -> Self {
        if v <= 0 {
            return SteeringLock::Full;
        }
        Self::STEPS
            .into_iter()
            .filter_map(|s| match s {
                SteeringLock::Degrees(d) => Some((s, (i64::from(d) - v).abs())),
                SteeringLock::Full => None,
            })
            .min_by_key(|&(_, dist)| dist)
            .map_or(Self::DEFAULT, |(s, _)| s)
    }

    pub fn to_setting(self) -> i64 {
        match self {
            SteeringLock::Degrees(d) => i64::from(d),
            SteeringLock::Full => 0,
        }
    }

    /// `delta` steps along [`Self::STEPS`], wrapping around.
    pub fn step(self, delta: i32) -> Self {
        let i = Self::STEPS.iter().position(|s| *s == self).unwrap_or(1) as i32;
        Self::STEPS[(i + delta).rem_euclid(Self::STEPS.len() as i32) as usize]
    }
}

/// Wheel position (−1..1 over the wheel's full rotation) to steering
/// (−1..1 over the steering lock): `raw × (rotation/2) / (lock/2)`,
/// clamped. A rotation of 0 or 1 is taken as 900°.
pub fn apply_lock(raw: f32, rotation_deg: u16, lock: SteeringLock) -> f32 {
    let raw = raw.clamp(-1.0, 1.0);
    match lock {
        SteeringLock::Full => raw,
        SteeringLock::Degrees(lock) => {
            let half_rotation = if rotation_deg <= 1 {
                f32::from(known::GENERIC_ROTATION) / 2.0
            } else {
                f32::from(rotation_deg) / 2.0
            };
            (raw * half_rotation / (f32::from(lock) / 2.0)).clamp(-1.0, 1.0)
        }
    }
}

/// Linearity 0..=100: 0 is linear, 100 a full cubic (`L·m³ + (1−L)·m`).
pub fn curve(m: f32, linearity: u8) -> f32 {
    let l = f32::from(linearity.min(100)) / 100.0;
    l * m * m * m + (1.0 - l) * m
}

/// Deadzone (a fraction of full scale) then [`curve`], keeping the sign.
pub fn shape(v: f32, deadzone: f32, linearity: u8) -> f32 {
    let v = v.clamp(-1.0, 1.0);
    let dz = deadzone.clamp(0.0, 0.99);
    let m = if v.abs() <= dz {
        0.0
    } else {
        ((v.abs() - dz) / (1.0 - dz)).min(1.0)
    };
    curve(m, linearity).copysign(v)
}

/// Wanszai's menu name of a linearity value.
pub fn linearity_label(l: u8) -> &'static str {
    match l {
        0 => "Linear",
        1..=20 => "Slight",
        21..=40 => "Mild",
        41..=60 => "Medium",
        61..=80 => "Heavy",
        _ => "Cubic",
    }
}

/// How one pedal is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PedalAxis {
    pub axis: usize,
    /// Raw value with the pedal up.
    pub rest: i16,
    /// Raw value with the pedal floored.
    pub peak: i16,
}

/// Pedal deadzone at the top of the travel (Wanszai's 2 %).
pub const PEDAL_DEADZONE: f32 = 0.02;

impl PedalAxis {
    /// 0 up … 1 floored. An uncalibrated span (under 1000) reads as up.
    pub fn read(&self, raw: i16) -> f32 {
        let span = f32::from(self.peak) - f32::from(self.rest);
        if span.abs() < 1000.0 {
            return 0.0;
        }
        let p = ((f32::from(raw) - f32::from(self.rest)) / span).clamp(0.0, 1.0);
        ((p - PEDAL_DEADZONE) / (1.0 - PEDAL_DEADZONE)).clamp(0.0, 1.0)
    }

    /// The same axis with its direction flipped (a menu's invert toggle).
    pub fn inverted(self) -> Self {
        Self {
            rest: self.peak,
            peak: self.rest,
            ..self
        }
    }
}

/// Pedals rest at one end of their axis while the steering rests near the
/// middle, so the first two axes after the steering (axis 0) resting
/// beyond ±24000 are taken as accelerator and brake.
pub fn guess_pedals(initial: &DeviceState) -> (Option<PedalAxis>, Option<PedalAxis>) {
    let mut resting = initial
        .axes
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, v)| v.unsigned_abs() > 24_000)
        .map(|(axis, &v)| PedalAxis {
            axis,
            rest: if v > 0 { 32_767 } else { -32_768 },
            peak: if v > 0 { -32_768 } else { 32_767 },
        });
    (resting.next(), resting.next())
}

/// The launcher's own buttons (Rad Mobile's lights and wiper, a gearbox
/// game's shift up and down). The game knowledge stays in the launcher.
pub trait Control: Copy + Eq + Ord + core::fmt::Debug + 'static {
    /// Every button; the order is [`Action`]'s index order (and a
    /// [`PadMap`]'s slots), so never reorder it.
    const ALL: &'static [Self];
    /// The name in settings files (`btn_<key>`, `pad_<key>`).
    fn key(self) -> &'static str;
    /// The name in the launcher's menu.
    fn label(self) -> &'static str;
    /// The button's bit in [`Controls::buttons`].
    fn bit(self) -> u32;
}

/// Something a player binds to a pad or a wheel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action<B> {
    Accelerate,
    Brake,
    Press(B),
}

impl<B: Control> Action<B> {
    /// The accelerator, the brake, then [`Control::ALL`].
    pub fn all() -> impl Iterator<Item = Action<B>> {
        [Action::Accelerate, Action::Brake]
            .into_iter()
            .chain(B::ALL.iter().map(|&b| Action::Press(b)))
    }

    /// How many actions there are.
    pub fn count() -> usize {
        2 + B::ALL.len()
    }

    /// Position in [`Self::all`].
    pub fn index(self) -> usize {
        match self {
            Action::Accelerate => 0,
            Action::Brake => 1,
            Action::Press(b) => {
                2 + B::ALL
                    .iter()
                    .position(|&x| x == b)
                    .expect("Control::ALL lists every button")
            }
        }
    }

    /// The name in settings files: `accel`, `brake` or the button's key.
    pub fn key(self) -> &'static str {
        match self {
            Action::Accelerate => "accel",
            Action::Brake => "brake",
            Action::Press(b) => b.key(),
        }
    }
}

/// Steering, pedals and buttons after lock, curves and calibration.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Controls {
    /// −1 full left … +1 full right.
    pub steer: f32,
    /// 0 released … 1 floored.
    pub accel: f32,
    pub brake: f32,
    /// [`Control::bit`] flags.
    pub buttons: u32,
}

impl Controls {
    pub fn press<B: Control>(&mut self, button: B) {
        self.buttons |= button.bit();
    }

    pub fn is_pressed<B: Control>(&self, button: B) -> bool {
        self.buttons & button.bit() != 0
    }

    /// Two sources together (a wheel and a pad): the larger steering
    /// deflection and pedal travel win, buttons add up.
    pub fn merge(self, other: Controls) -> Controls {
        Controls {
            steer: if other.steer.abs() > self.steer.abs() {
                other.steer
            } else {
                self.steer
            },
            accel: self.accel.max(other.accel),
            brake: self.brake.max(other.brake),
            buttons: self.buttons | other.buttons,
        }
    }
}

/// Where a pedal or button lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Wheel,
    /// A second device (separate pedals or a button box).
    Aux,
}

impl Source {
    fn key(self) -> &'static str {
        match self {
            Source::Wheel => "wheel",
            Source::Aux => "aux",
        }
    }
}

/// A button on the wheel or the aux device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub source: Source,
    pub button: usize,
}

impl Binding {
    /// `wheel:4` or `aux:0`.
    fn parse(s: &str) -> Option<Self> {
        let (src, n) = s.split_once(':')?;
        let source = match src.trim() {
            "wheel" => Source::Wheel,
            "aux" => Source::Aux,
            _ => return None,
        };
        Some(Binding {
            source,
            button: n.trim().parse().ok()?,
        })
    }

    fn format(self) -> String {
        format!("{}:{}", self.source.key(), self.button)
    }
}

/// The wheel options a launcher's menu offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WheelSettings {
    pub lock: SteeringLock,
    pub invert_accel: bool,
    pub invert_brake: bool,
    /// The wheel picked by hand, by device path; empty picks automatically.
    pub device: String,
}

impl Default for WheelSettings {
    fn default() -> Self {
        Self {
            lock: SteeringLock::DEFAULT,
            invert_accel: false,
            invert_brake: false,
            device: String::new(),
        }
    }
}

/// The keys of a settings file, lower-cased (rea-rad-mobile's `Ini` read
/// `Rotation=` as `rotation`); a later line wins. Blank values are unset.
pub(crate) struct Keys(Vec<(String, String)>);

impl Keys {
    pub(crate) fn of(text: &str) -> Keys {
        Keys(
            cfg::entries(text)
                .map(|e| (e.key.to_ascii_lowercase(), e.value.to_owned()))
                .collect(),
        )
    }

    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    fn parsed<T: std::str::FromStr>(&self, key: &str) -> Option<T> {
        self.get(key).and_then(|v| v.parse().ok())
    }
}

/// `existing` with `header` as `#` comments in front when it has no lines
/// yet, so a new file says who wrote it.
fn with_header(existing: &str, header: &str) -> String {
    if !existing.trim().is_empty() || header.is_empty() {
        return existing.to_owned();
    }
    header.lines().map(|l| format!("# {l}\n")).collect()
}

/// Everything a wheel setup learns about a wheel; saved as `wheel.ini`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WheelProfile<B> {
    pub wheel: DeviceId,
    /// Pedals or buttons on a second USB device.
    pub aux: Option<DeviceId>,
    pub steer_axis: usize,
    /// Lock to lock, degrees.
    pub rotation: u16,
    pub gas: Option<(Source, PedalAxis)>,
    pub brake: Option<(Source, PedalAxis)>,
    /// Sorted by button.
    pub buttons: Vec<(B, Binding)>,
}

impl<B: Control> WheelProfile<B> {
    /// A first guess for a wheel nobody has set up: steering on axis 0,
    /// pedals from where they rest, and the shift paddles (from the table,
    /// or buttons 0 and 1 for a wheel it does not list) as the `paddles`
    /// buttons (up, down) when the launcher gives them a use.
    pub fn guess(wheel: DeviceId, initial: &DeviceState, paddles: Option<(B, B)>) -> Self {
        let known = known::lookup(wheel.vid, wheel.pid);
        let rotation = known.map_or(known::GENERIC_ROTATION, |k| k.rotation);
        let (gas, brake) = guess_pedals(initial);
        let mut profile = Self {
            wheel,
            aux: None,
            steer_axis: 0,
            rotation,
            gas: gas.map(|p| (Source::Wheel, p)),
            brake: brake.map(|p| (Source::Wheel, p)),
            buttons: Vec::new(),
        };
        let numbers = known.map_or(Some(known::GENERIC_PADDLES), |k| k.paddles);
        if let (Some((up, down)), Some((b_up, b_down))) = (numbers, paddles) {
            for (button, n) in [(b_up, up), (b_down, down)] {
                profile.assign(
                    button,
                    Binding {
                        source: Source::Wheel,
                        button: usize::from(n),
                    },
                );
            }
        }
        profile
    }

    pub fn binding(&self, button: B) -> Option<Binding> {
        self.buttons
            .iter()
            .find(|(b, _)| *b == button)
            .map(|(_, bind)| *bind)
    }

    /// Bind `button` to `binding` alone, taking that binding away from any
    /// other button so one press never does two things.
    pub fn assign(&mut self, button: B, binding: Binding) {
        self.buttons
            .retain(|(b, bind)| *b != button && *bind != binding);
        self.buttons.push((button, binding));
        self.buttons.sort_by_key(|(b, _)| *b);
    }

    /// Drop the second device and everything bound to it.
    pub fn forget_aux(&mut self) {
        self.aux = None;
        self.buttons
            .retain(|(_, bind)| bind.source == Source::Wheel);
        for pedal in [&mut self.gas, &mut self.brake] {
            if pedal.is_some_and(|(src, _)| src == Source::Aux) {
                *pedal = None;
            }
        }
    }

    /// Take what the setup wizard captured. `devices[0]` is the wheel, the
    /// others are the devices it also watched (wizard device order). The
    /// first other device used becomes the aux device; whatever was bound
    /// to a different aux device before is dropped.
    pub fn apply(&mut self, captures: &wizard::Captures<B>, devices: &[DeviceId]) {
        let other = captures
            .gas
            .map(|(d, _)| d)
            .into_iter()
            .chain(captures.brake.map(|(d, _)| d))
            .chain(captures.buttons.iter().map(|(_, d, _)| *d))
            .find(|d| *d != 0 && *d < devices.len());
        if let Some(d) = other
            && self.aux.as_ref() != Some(&devices[d])
        {
            self.forget_aux();
            self.aux = Some(devices[d].clone());
        }
        // the wheel, the new aux device, or (any third device) nothing
        let source = |d: usize| match d {
            0 => Some(Source::Wheel),
            _ if Some(d) == other => Some(Source::Aux),
            _ => None,
        };
        if let Some(pedal) = captures.gas.and_then(|(d, p)| Some((source(d)?, p))) {
            self.gas = Some(pedal);
            if same_pedal(self.brake, pedal) {
                self.brake = None;
            }
        }
        if let Some(pedal) = captures.brake.and_then(|(d, p)| Some((source(d)?, p))) {
            self.brake = Some(pedal);
            if same_pedal(self.gas, pedal) {
                self.gas = None;
            }
        }
        for &(button, d, n) in &captures.buttons {
            if let Some(source) = source(d) {
                self.assign(button, Binding { source, button: n });
            }
        }
    }

    /// What a menu shows for a control: "AXIS 2", "AUX BUTTON 5" (counted
    /// from 1, as wheel software shows them), or "-".
    pub fn describe(&self, action: Action<B>) -> String {
        let prefix = |src: Source| if src == Source::Aux { "AUX " } else { "" };
        let pedal = |p: &Option<(Source, PedalAxis)>| {
            p.map_or("-".to_string(), |(src, axis)| {
                format!("{}AXIS {}", prefix(src), axis.axis + 1)
            })
        };
        match action {
            Action::Accelerate => pedal(&self.gas),
            Action::Brake => pedal(&self.brake),
            Action::Press(button) => self.binding(button).map_or("-".to_string(), |b| {
                format!("{}BUTTON {}", prefix(b.source), b.button + 1)
            }),
        }
    }

    /// The controls for the current readings; `aux` is `None` while the
    /// second device is unplugged (its pedals read as released).
    pub fn read(
        &self,
        settings: &WheelSettings,
        wheel: &DeviceState,
        aux: Option<&DeviceState>,
    ) -> Controls {
        let device = |src: Source| match src {
            Source::Wheel => Some(wheel),
            Source::Aux => aux,
        };
        let pedal = |p: &Option<(Source, PedalAxis)>, invert: bool| {
            p.and_then(|(src, axis)| {
                let axis = if invert { axis.inverted() } else { axis };
                device(src)
                    .and_then(|d| d.axis(axis.axis))
                    .map(|raw| axis.read(raw))
            })
            .unwrap_or(0.0)
        };
        let raw_steer = wheel
            .axis(self.steer_axis)
            .map_or(0.0, |v| f32::from(v) / RAW_MAX);
        let mut controls = Controls {
            steer: apply_lock(raw_steer, self.rotation, settings.lock),
            accel: pedal(&self.gas, settings.invert_accel),
            brake: pedal(&self.brake, settings.invert_brake),
            buttons: 0,
        };
        for (button, bind) in &self.buttons {
            if device(bind.source).is_some_and(|d| d.button(bind.button)) {
                controls.press(*button);
            }
        }
        controls
    }

    /// The profile written into `existing` (a `wheel.ini`, or "" for a new
    /// one, which gets `header` as comments), with rea-rad-mobile's keys:
    /// `wheel_{name,vid,pid,guid}`, `aux_…`, `steer_axis`, `rotation`,
    /// `{gas,brake}_{device,axis,rest,peak}`, `btn_<key>` = `wheel:N` or
    /// `aux:N`. Every other line stays (hand edits and comments); keys
    /// this profile no longer has are commented out.
    pub fn to_cfg(&self, existing: &str, header: &str) -> String {
        let mut text = with_header(existing, header);
        let mut set = |key: &str, value: Option<String>| {
            text = match value {
                Some(v) => cfg::set_key(&text, key, &v),
                None => cfg::clear_key(&text, key),
            };
        };
        for (prefix, id) in [("wheel_", Some(&self.wheel)), ("aux_", self.aux.as_ref())] {
            set(&format!("{prefix}name"), id.map(|i| i.name.clone()));
            set(
                &format!("{prefix}vid"),
                id.map(|i| format!("{:04x}", i.vid)),
            );
            set(
                &format!("{prefix}pid"),
                id.map(|i| format!("{:04x}", i.pid)),
            );
            set(&format!("{prefix}guid"), id.map(|i| i.guid.clone()));
        }
        set("steer_axis", Some(self.steer_axis.to_string()));
        set("rotation", Some(self.rotation.to_string()));
        for (key, pedal) in [("gas", &self.gas), ("brake", &self.brake)] {
            set(
                &format!("{key}_device"),
                pedal.map(|(s, _)| s.key().to_owned()),
            );
            set(
                &format!("{key}_axis"),
                pedal.map(|(_, p)| p.axis.to_string()),
            );
            set(
                &format!("{key}_rest"),
                pedal.map(|(_, p)| p.rest.to_string()),
            );
            set(
                &format!("{key}_peak"),
                pedal.map(|(_, p)| p.peak.to_string()),
            );
        }
        for &button in B::ALL {
            set(
                &format!("btn_{}", button.key()),
                self.binding(button).map(Binding::format),
            );
        }
        text
    }

    /// A profile from a `wheel.ini` (this SDK's or rea-rad-mobile's
    /// `key=value` form; keys in any case); `None` without the wheel's
    /// `wheel_vid` and `wheel_pid`. The rotation is clamped to 90..=3600.
    pub fn from_cfg(text: &str) -> Option<Self> {
        let keys = Keys::of(text);
        let id = |prefix: &str| -> Option<DeviceId> {
            let hex = |k: &str| u16::from_str_radix(keys.get(&format!("{prefix}{k}"))?, 16).ok();
            let text = |k: &str| {
                keys.get(&format!("{prefix}{k}"))
                    .unwrap_or_default()
                    .to_owned()
            };
            Some(DeviceId {
                name: text("name"),
                vid: hex("vid")?,
                pid: hex("pid")?,
                guid: text("guid"),
            })
        };
        let pedal = |key: &str| -> Option<(Source, PedalAxis)> {
            let src = match keys.get(&format!("{key}_device")) {
                Some("aux") => Source::Aux,
                _ => Source::Wheel,
            };
            Some((
                src,
                PedalAxis {
                    axis: keys.parsed(&format!("{key}_axis"))?,
                    rest: keys.parsed(&format!("{key}_rest"))?,
                    peak: keys.parsed(&format!("{key}_peak"))?,
                },
            ))
        };
        let buttons = B::ALL
            .iter()
            .filter_map(|&b| {
                let bind = keys
                    .get(&format!("btn_{}", b.key()))
                    .and_then(Binding::parse)?;
                Some((b, bind))
            })
            .collect();
        Some(Self {
            wheel: id("wheel_")?,
            aux: id("aux_"),
            steer_axis: keys.parsed("steer_axis").unwrap_or(0),
            rotation: keys
                .parsed("rotation")
                .unwrap_or(known::GENERIC_ROTATION)
                .clamp(90, 3600),
            gas: pedal("gas"),
            brake: pedal("brake"),
            buttons,
        })
    }
}

/// Whether `pedal` reads the same axis the same way as `other`. Two pedals
/// on one axis in opposite directions are a combined pedal axis and fine.
fn same_pedal(pedal: Option<(Source, PedalAxis)>, other: (Source, PedalAxis)) -> bool {
    let direction = |p: PedalAxis| (i32::from(p.peak) - i32::from(p.rest)).signum();
    pedal.is_some_and(|(src, p)| {
        src == other.0 && p.axis == other.1.axis && direction(p) == direction(other.1)
    })
}

/// The buttons the tests bind, in the shape of rea-rad-mobile's cabinet.
#[cfg(test)]
pub(crate) mod test_controls {
    use super::Control;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub enum Button {
        Light,
        Wiper,
        Start,
        Coin,
        Service,
        Test,
        Pause,
        Exit,
    }

    impl Control for Button {
        const ALL: &'static [Button] = &[
            Button::Light,
            Button::Wiper,
            Button::Start,
            Button::Coin,
            Button::Service,
            Button::Test,
            Button::Pause,
            Button::Exit,
        ];

        fn key(self) -> &'static str {
            match self {
                Button::Light => "light",
                Button::Wiper => "wiper",
                Button::Start => "start",
                Button::Coin => "coin",
                Button::Service => "service",
                Button::Test => "test",
                Button::Pause => "pause",
                Button::Exit => "exit",
            }
        }

        fn label(self) -> &'static str {
            match self {
                Button::Light => "Lights",
                Button::Wiper => "Wiper",
                Button::Start => "Start",
                Button::Coin => "Coin",
                Button::Service => "Service",
                Button::Test => "Test menu",
                Button::Pause => "Pause",
                Button::Exit => "Back to menu",
            }
        }

        fn bit(self) -> u32 {
            1 << (self as u32)
        }
    }

    /// Lights and wiper on the shift paddles, as Rad Mobile.
    pub const PADDLES: Option<(Button, Button)> = Some((Button::Light, Button::Wiper));
}

#[cfg(test)]
mod tests {
    use super::test_controls::{Button, PADDLES};
    use super::*;

    type Profile = WheelProfile<Button>;

    fn approx(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    fn facts(
        vid: u16,
        pid: u16,
        sdl_wheel_type: bool,
        game_controller: bool,
        axes: usize,
    ) -> DeviceFacts {
        DeviceFacts {
            vid,
            pid,
            sdl_wheel_type,
            game_controller,
            axes,
        }
    }

    #[test]
    fn classify_puts_known_wheels_first_and_pads_before_the_vendor_hint() {
        let table = [
            // a gamepad from a wheel vendor is a pad (rea-rad-mobile made
            // it a 900° wheel)
            (
                facts(0x046d, 0xc21d, false, true, 6),
                Kind::Pad,
                "Logitech F310",
            ),
            (
                facts(0x046d, 0xc21f, false, true, 6),
                Kind::Pad,
                "Logitech F710",
            ),
            (
                facts(0x044f, 0xd00e, false, true, 6),
                Kind::Pad,
                "Thrustmaster eSwap",
            ),
            // a known wheel row wins even with a controller mapping
            (
                facts(0x046d, 0xc262, false, true, 4),
                Kind::Wheel,
                "Logitech G920",
            ),
            (
                facts(0x0eb7, 0x0020, false, false, 8),
                Kind::Wheel,
                "CSL DD",
            ),
            (
                facts(0x1234, 0x0001, true, false, 3),
                Kind::Wheel,
                "SDL wheel type",
            ),
            // a model new to the table, from a wheel vendor
            (
                facts(0x0eb7, 0x0e04, false, false, 3),
                Kind::Wheel,
                "new Fanatec",
            ),
            (facts(0x045e, 0x0b12, false, true, 6), Kind::Pad, "Xbox pad"),
            (
                facts(0x2dc8, 0x310b, false, false, 0),
                Kind::Other,
                "8BitDo spare",
            ),
            (
                facts(0x0eb7, 0x1839, false, false, 0),
                Kind::Other,
                "button box, no axes",
            ),
        ];
        for (f, kind, name) in table {
            assert_eq!(classify(&f), kind, "{name}");
        }
    }

    fn shape_of(pid: u16, kind: Kind, axes: usize) -> DeviceShape {
        DeviceShape {
            vid: 0x2dc8,
            pid,
            kind,
            axes,
        }
    }

    #[test]
    fn a_pads_spare_interfaces_are_skipped_but_lone_button_boxes_stay() {
        let devices = [
            shape_of(0x310b, Kind::Pad, 6),
            shape_of(0x310b, Kind::Other, 0),
            shape_of(0x310b, Kind::Other, 0),
            shape_of(0x1234, Kind::Other, 0), // a button box of its own
        ];
        let spare: Vec<bool> = (0..devices.len())
            .map(|i| is_spare_interface(&devices, i))
            .collect();
        assert_eq!(spare, [false, true, true, false]);
        assert!(!is_spare_interface(&devices, 9));
    }

    #[test]
    fn the_aux_device_is_found_by_guid_then_model_never_the_wheel() {
        let id = |pid, guid: &str| DeviceId {
            name: String::new(),
            vid: 0x0eb7,
            pid,
            guid: guid.into(),
        };
        let pedals = id(0x6204, "b");
        let devices = [id(0x6204, "a"), id(0x0020, "w"), id(0x6204, "b")];
        assert_eq!(find_aux(&devices, &pedals, 1), Some(2));
        assert_eq!(find_aux(&devices, &id(0x6204, "gone"), 1), Some(0));
        assert_eq!(find_aux(&devices, &id(0x6204, "a"), 0), Some(2));
        assert_eq!(find_aux(&devices, &id(0x9999, "b"), 1), None);
    }

    fn cand(pid: u16, path: &str, haptic: bool) -> WheelCandidate<'_> {
        WheelCandidate {
            vid: 0x0eb7,
            pid,
            path,
            haptic,
        }
    }

    #[test]
    fn the_collection_that_takes_force_feedback_wins() {
        // the inert collection comes first, as on the reporter's Windows
        let wheels = [cand(0x20, "col02", false), cand(0x20, "col01", true)];
        assert_eq!(pick_wheel(&wheels, None, ""), Some(1));
        let only_inert = [cand(0x20, "col02", false)];
        assert_eq!(pick_wheel(&only_inert, None, ""), Some(0));
        assert_eq!(pick_wheel(&[], None, ""), None);
    }

    #[test]
    fn a_wheel_picked_by_hand_wins_while_connected() {
        let wheels = [cand(0x20, "a", true), cand(0x20, "b", false)];
        assert_eq!(pick_wheel(&wheels, None, "b"), Some(1));
        assert_eq!(pick_wheel(&wheels, None, "gone"), Some(0));
    }

    #[test]
    fn the_saved_model_comes_before_force_feedback_elsewhere() {
        let saved = DeviceId {
            vid: 0x0eb7,
            pid: 0x20,
            ..DeviceId::default()
        };
        let wheels = [cand(0x05, "other", true), cand(0x20, "mine", false)];
        assert_eq!(pick_wheel(&wheels, Some(&saved), ""), Some(1));
    }

    #[test]
    fn steering_lock_scales_the_wheel_rotation() {
        // a 1080° wheel at 270° lock: a quarter of the half-rotation is full lock
        assert!(approx(
            apply_lock(0.25, 1080, SteeringLock::Degrees(270)),
            1.0
        ));
        assert!(approx(
            apply_lock(0.125, 1080, SteeringLock::Degrees(270)),
            0.5
        ));
        assert!(approx(
            apply_lock(-0.9, 1080, SteeringLock::Degrees(270)),
            -1.0
        ));
        assert!(approx(apply_lock(0.4, 900, SteeringLock::Full), 0.4));
        // a nonsense rotation falls back to 900°
        assert!(approx(apply_lock(0.1, 0, SteeringLock::Degrees(180)), 0.5));
    }

    #[test]
    fn lock_steps_cycle_and_snap() {
        assert_eq!(SteeringLock::DEFAULT.step(1), SteeringLock::Degrees(360));
        assert_eq!(SteeringLock::Full.step(1), SteeringLock::Degrees(180));
        assert_eq!(SteeringLock::Degrees(180).step(-1), SteeringLock::Full);
        assert_eq!(SteeringLock::from_setting(300), SteeringLock::Degrees(270));
        assert_eq!(SteeringLock::from_setting(0), SteeringLock::Full);
        assert_eq!(
            SteeringLock::from_setting(SteeringLock::Degrees(540).to_setting()),
            SteeringLock::Degrees(540)
        );
        assert_eq!(SteeringLock::DEFAULT.label(), "270°");
        assert_eq!(SteeringLock::Full.label(), "Full");
    }

    #[test]
    fn curves_follow_the_cubic_blend() {
        assert!(approx(curve(0.5, 0), 0.5));
        assert!(approx(curve(0.5, 100), 0.125));
        assert!(approx(curve(0.5, 50), 0.3125));
        assert!(approx(shape(0.05, 0.1, 0), 0.0));
        assert!(approx(shape(-0.55, 0.1, 0), -0.5));
        assert!(approx(shape(1.0, 0.1, 100), 1.0));
        assert_eq!(linearity_label(0), "Linear");
        assert_eq!(linearity_label(30), "Mild");
        assert_eq!(linearity_label(100), "Cubic");
    }

    #[test]
    fn pedals_read_with_calibration_and_deadzone() {
        let gas = PedalAxis {
            axis: 1,
            rest: 32_767,
            peak: -32_768,
        };
        assert_eq!(gas.read(32_767), 0.0);
        assert!(approx(gas.read(-32_768), 1.0));
        assert!(approx(
            gas.read(0),
            (0.5 - PEDAL_DEADZONE) / (1.0 - PEDAL_DEADZONE)
        ));
        assert_eq!(gas.read(32_000), 0.0, "inside the top deadzone");
        assert!(approx(gas.inverted().read(32_767), 1.0));
        // an uncalibrated axis reads as released
        let flat = PedalAxis {
            axis: 1,
            rest: 0,
            peak: 10,
        };
        assert_eq!(flat.read(10), 0.0);
    }

    #[test]
    fn pedals_are_guessed_from_where_they_rest() {
        let initial = DeviceState {
            axes: vec![-120, 32_767, 5, -32_768, 32_767],
            buttons: vec![],
        };
        let (gas, brake) = guess_pedals(&initial);
        assert_eq!(
            gas,
            Some(PedalAxis {
                axis: 1,
                rest: 32_767,
                peak: -32_768
            })
        );
        assert_eq!(
            brake,
            Some(PedalAxis {
                axis: 3,
                rest: -32_768,
                peak: 32_767
            })
        );
        let none = guess_pedals(&DeviceState {
            axes: vec![0, 0],
            buttons: vec![],
        });
        assert_eq!(none, (None, None));
    }

    fn csl() -> DeviceId {
        DeviceId {
            name: "FANATEC CSL DD".into(),
            vid: 0x0eb7,
            pid: 0x0020,
            guid: "030000b70e00002000".into(),
        }
    }

    #[test]
    fn a_guessed_profile_drives_the_controls() {
        let rest = DeviceState {
            axes: vec![0, 32_767, 32_767],
            buttons: vec![false; 16],
        };
        let profile = Profile::guess(csl(), &rest, PADDLES);
        assert_eq!(profile.rotation, 1080);
        assert_eq!(
            profile.binding(Button::Light),
            Some(Binding {
                source: Source::Wheel,
                button: 0
            })
        );

        let mut now = rest.clone();
        now.axes[0] = 8_192; // a quarter of the way to the right of a 1080° wheel = 135°
        now.axes[1] = -32_768;
        now.buttons[1] = true;
        let c = profile.read(&WheelSettings::default(), &now, None);
        assert!(approx(c.steer, 1.0));
        assert!(approx(c.accel, 1.0));
        assert_eq!(c.brake, 0.0);
        assert!(c.is_pressed(Button::Wiper));

        let inverted = WheelSettings {
            invert_brake: true,
            ..Default::default()
        };
        assert!(approx(profile.read(&inverted, &rest, None).brake, 1.0));
        // a launcher without a use for the paddles binds nothing
        assert!(Profile::guess(csl(), &rest, None).buttons.is_empty());
    }

    #[test]
    fn pedals_on_a_separate_device() {
        let mut profile = Profile::guess(
            csl(),
            &DeviceState {
                axes: vec![0],
                buttons: vec![],
            },
            PADDLES,
        );
        profile.aux = Some(DeviceId {
            name: "Pedals".into(),
            vid: 0x0eb7,
            pid: 0x183b,
            guid: String::new(),
        });
        profile.gas = Some((
            Source::Aux,
            PedalAxis {
                axis: 0,
                rest: -32_768,
                peak: 32_767,
            },
        ));
        let wheel = DeviceState {
            axes: vec![0],
            buttons: vec![],
        };
        let pedals = DeviceState {
            axes: vec![32_767],
            buttons: vec![],
        };
        let settings = WheelSettings::default();
        assert!(approx(
            profile.read(&settings, &wheel, Some(&pedals)).accel,
            1.0
        ));
        // pedals unplugged: released, not stuck
        assert_eq!(profile.read(&settings, &wheel, None).accel, 0.0);
    }

    fn full_profile() -> Profile {
        let mut profile = Profile::guess(
            csl(),
            &DeviceState {
                axes: vec![0, 32_767, -32_768],
                buttons: vec![],
            },
            PADDLES,
        );
        profile.aux = Some(DeviceId {
            name: "CSL Pedals".into(),
            vid: 0x0eb7,
            pid: 0x6204,
            guid: "abc".into(),
        });
        profile.assign(
            Button::Exit,
            Binding {
                source: Source::Aux,
                button: 3,
            },
        );
        profile
    }

    #[test]
    fn profiles_round_trip_through_cfg() {
        let profile = full_profile();
        let text = profile.to_cfg("", "wheel.ini - written by a test\nhand edits stay");
        assert!(text.starts_with("# wheel.ini - written by a test\n# hand edits stay\n"));
        assert_eq!(Profile::from_cfg(&text), Some(profile.clone()));
        assert!(Profile::from_cfg("steer_axis=1").is_none());

        // forgetting the aux device comments its keys out, so they don't
        // come back on the next read; hand edits stay
        let edited = format!("{text}# my note\nextra = kept\n");
        let mut moved = profile.clone();
        moved.forget_aux();
        let again = moved.to_cfg(&edited, "ignored: the file has lines");
        assert!(again.contains("# my note\nextra = kept\n"));
        assert!(again.contains("#aux_vid =\n"));
        assert!(!again.contains("ignored"));
        assert_eq!(Profile::from_cfg(&again), Some(moved));
    }

    /// A `wheel.ini` exactly as rea-rad-mobile 0.5.0 writes it
    /// (`WheelProfile::to_ini`: `key=value`, `#` comments).
    const RAD_MOBILE_WHEEL_INI: &str = include_str!("../../tests/fixtures/rad-mobile-wheel.ini");

    #[test]
    fn rad_mobiles_wheel_ini_still_reads_and_reads_ours() {
        let profile = Profile::from_cfg(RAD_MOBILE_WHEEL_INI).unwrap();
        assert_eq!(profile, full_profile());
        // Notepad's CRLF, upper-case keys and a [section] line too
        let notepad = RAD_MOBILE_WHEEL_INI
            .replace("rotation=", "Rotation=")
            .replace('\n', "\r\n")
            .replacen("# hand", "[wheel]\r\n# hand", 1);
        assert_eq!(Profile::from_cfg(&notepad), Some(full_profile()));

        // what we write, read the way rea-rad-mobile's Ini reads it
        // (trimmed `key=value`, keys lower-cased, a later line wins)
        let ours = profile.to_cfg(RAD_MOBILE_WHEEL_INI, "");
        let rad_read = |text: &str| {
            let mut map = std::collections::BTreeMap::new();
            for line in text.lines().map(str::trim) {
                if line.starts_with(['#', ';', '[']) {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    map.insert(k.trim().to_ascii_lowercase(), v.trim().to_owned());
                }
            }
            map
        };
        assert_eq!(rad_read(&ours), rad_read(RAD_MOBILE_WHEEL_INI));
    }

    #[test]
    fn unknown_wheels_get_the_generic_paddles() {
        let id = DeviceId {
            name: "Some Wheel".into(),
            vid: 0x1234,
            pid: 0x0001,
            guid: String::new(),
        };
        let profile = Profile::guess(id, &DeviceState::default(), PADDLES);
        assert_eq!(profile.describe(Action::Press(Button::Light)), "BUTTON 1");
        assert_eq!(profile.describe(Action::Press(Button::Wiper)), "BUTTON 2");
        assert_eq!(profile.describe(Action::Press(Button::Start)), "-");
    }

    #[test]
    fn a_remapped_wheel_button_leaves_its_old_control() {
        let mut profile = Profile::guess(csl(), &DeviceState::default(), PADDLES);
        let paddle = profile.binding(Button::Light).unwrap();
        profile.assign(Button::Start, paddle);
        assert_eq!(profile.binding(Button::Start), Some(paddle));
        assert_eq!(profile.binding(Button::Light), None);
    }

    #[test]
    fn captures_from_a_second_device_make_it_the_aux() {
        let pedals = DeviceId {
            name: "Pedals".into(),
            vid: 0x0eb7,
            pid: 0x6204,
            guid: "p".into(),
        };
        let mut profile = Profile::guess(csl(), &DeviceState::default(), PADDLES);
        let gas = PedalAxis {
            axis: 1,
            rest: 32_767,
            peak: -32_768,
        };
        let captures = wizard::Captures {
            gas: Some((1, gas)),
            buttons: vec![(Button::Coin, 0, 7)],
            ..Default::default()
        };
        profile.apply(&captures, &[csl(), pedals.clone()]);
        assert_eq!(profile.aux, Some(pedals));
        assert_eq!(profile.describe(Action::Accelerate), "AUX AXIS 2");
        assert_eq!(profile.describe(Action::Press(Button::Coin)), "BUTTON 8");

        // pedals moved back onto the wheel: the old aux and its pedal go
        let mut moved = profile.clone();
        moved.forget_aux();
        assert_eq!(moved.aux, None);
        assert_eq!(moved.describe(Action::Accelerate), "-");
        assert_eq!(moved.describe(Action::Press(Button::Coin)), "BUTTON 8");
    }

    #[test]
    fn a_pedal_moved_onto_the_other_pedals_axis_takes_it_over() {
        let at_top = |axis| PedalAxis {
            axis,
            rest: 32_767,
            peak: -32_768,
        };
        let mut profile = Profile::guess(csl(), &DeviceState::default(), PADDLES);
        profile.gas = Some((Source::Wheel, at_top(1)));
        profile.brake = Some((Source::Wheel, at_top(2)));
        let captures = wizard::Captures {
            gas: Some((0, at_top(2))),
            ..Default::default()
        };
        profile.apply(&captures, &[csl()]);
        assert_eq!(profile.describe(Action::Accelerate), "AXIS 3");
        assert_eq!(profile.describe(Action::Brake), "-");

        // one axis, opposite directions: a combined pedal axis keeps both
        let mut combined = Profile::guess(csl(), &DeviceState::default(), PADDLES);
        combined.brake = Some((
            Source::Wheel,
            PedalAxis {
                axis: 1,
                rest: 0,
                peak: 32_767,
            },
        ));
        let captures = wizard::Captures {
            gas: Some((
                0,
                PedalAxis {
                    axis: 1,
                    rest: 0,
                    peak: -32_768,
                },
            )),
            ..Default::default()
        };
        combined.apply(&captures, &[csl()]);
        assert_eq!(combined.describe(Action::Brake), "AXIS 2");
    }

    #[test]
    fn actions_index_in_order_and_controls_merge() {
        let all: Vec<Action<Button>> = Action::all().collect();
        assert_eq!(all.len(), Action::<Button>::count());
        for (i, a) in all.iter().enumerate() {
            assert_eq!(a.index(), i);
        }
        assert_eq!(Action::<Button>::Accelerate.key(), "accel");
        assert_eq!(Action::Press(Button::Exit).key(), "exit");

        let mut wheel = Controls {
            steer: 0.2,
            accel: 0.9,
            ..Default::default()
        };
        wheel.press(Button::Light);
        let mut pad = Controls {
            steer: -0.6,
            accel: 0.1,
            brake: 0.3,
            buttons: 0,
        };
        pad.press(Button::Start);
        let both = wheel.merge(pad);
        assert_eq!(both.steer, -0.6);
        assert_eq!(both.accel, 0.9);
        assert_eq!(both.brake, 0.3);
        assert!(both.is_pressed(Button::Light) && both.is_pressed(Button::Start));
        assert!(!both.is_pressed(Button::Exit));
        assert_eq!(both.buttons, 0b101);
    }
}
