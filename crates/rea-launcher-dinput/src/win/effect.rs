//! Effects: `DIEFFECT` read into [`Effect`] and written back, the
//! game's effect objects, and the real effects behind them.
//!
//! Each game effect owns at most one real effect on the wheel, made from
//! [`Effect::for_wheel`]. Real effects are made when the game's effect is
//! complete, updated as the game updates it, and made again, downloaded
//! and restarted when the wheel comes back ([`Effects::revive`]).

use std::ffi::c_void;
use std::ptr::null_mut;
use std::sync::{Arc, Mutex};

use rea_launcher::ffb::FfbSettings;
use rea_launcher::wheel::dinput::Axis;
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    DICONDITION, DICONSTANTFORCE, DIEB_NOTRIGGER, DIEFF_CARTESIAN, DIEFF_OBJECTIDS,
    DIEFF_OBJECTOFFSETS, DIEFF_POLAR, DIEFF_SPHERICAL, DIEFFECT, DIEFFESCAPE, DIEGES_PLAYING,
    DIENVELOPE, DIEP_AXES, DIEP_DIRECTION, DIEP_DURATION, DIEP_ENVELOPE, DIEP_GAIN,
    DIEP_NODOWNLOAD, DIEP_NORESTART, DIEP_SAMPLEPERIOD, DIEP_START, DIEP_STARTDELAY,
    DIEP_TYPESPECIFICPARAMS, DIPERIODIC, DIRAMPFORCE,
};
use windows_sys::Win32::Foundation::HINSTANCE;
use windows_sys::core::{GUID, HRESULT};

use super::com::{EffectVtbl, IID_EFFECT, IID_UNKNOWN, Object, This, query};
use super::device::View;
use super::real::{Real, RealEffect};
use super::rig::Rig;
use super::{guard, hr, lock, log};
use crate::effect::{Condition, Coordinates, Effect, Envelope, Family, Kind, Params};

/// The rest of every standard effect's GUID.
const EFFECT_GUID_TAIL: (u16, u16, [u8; 8]) = (
    0x8e33,
    0x11d0,
    [0x9a, 0xd0, 0x00, 0xa0, 0xc9, 0xa0, 0x6e, 0x35],
);

pub(super) fn kind_of(guid: &GUID) -> Option<Kind> {
    ((guid.data2, guid.data3, guid.data4) == EFFECT_GUID_TAIL)
        .then(|| Kind::ALL.into_iter().find(|k| k.data1() == guid.data1))
        .flatten()
}

pub(super) fn guid_of(kind: Kind) -> GUID {
    let (data2, data3, data4) = EFFECT_GUID_TAIL;
    GUID {
        data1: kind.data1(),
        data2,
        data3,
        data4,
    }
}

/// `DIEFFECT` before DirectX 6 had no start delay.
const DIEFFECT_DX5_SIZE: u32 = 52;

/// The parameters `flags` names, read from `p` over `base`. `axis` names
/// an axis from its `rgdwAxes` value (`true`: an object ID, `false`: an
/// offset in the device's data format).
///
/// # Safety
/// `p` is null or a `DIEFFECT` whose pointers are valid for the sizes it
/// gives, as DirectInput requires of its callers.
pub(super) unsafe fn read(
    base: &Effect,
    p: *const DIEFFECT,
    flags: u32,
    axis: impl Fn(bool, u32) -> Option<Axis>,
) -> Result<Effect, HRESULT> {
    // SAFETY: the caller's contract.
    let e = unsafe { p.as_ref() }.ok_or(hr::INVALIDPARAM)?;
    if e.dwSize != size_of::<DIEFFECT>() as u32 && e.dwSize != DIEFFECT_DX5_SIZE {
        return Err(hr::INVALIDPARAM);
    }
    let mut out = base.clone();
    if flags & DIEP_DURATION != 0 {
        out.duration = e.dwDuration;
    }
    if flags & DIEP_SAMPLEPERIOD != 0 {
        out.sample_period = e.dwSamplePeriod;
    }
    if flags & DIEP_GAIN != 0 {
        out.gain = e.dwGain;
    }
    if flags & DIEP_STARTDELAY != 0 && e.dwSize >= size_of::<DIEFFECT>() as u32 {
        out.start_delay = e.dwStartDelay;
    }
    let count = e.cAxes as usize;
    if flags & DIEP_AXES != 0 {
        let by_id = match e.dwFlags & (DIEFF_OBJECTIDS | DIEFF_OBJECTOFFSETS) {
            DIEFF_OBJECTIDS => true,
            DIEFF_OBJECTOFFSETS => false,
            _ => return Err(hr::INVALIDPARAM),
        };
        if count == 0 || count > 8 || e.rgdwAxes.is_null() {
            return Err(hr::INVALIDPARAM);
        }
        // SAFETY: rgdwAxes holds cAxes entries (the caller's contract).
        let axes = unsafe { std::slice::from_raw_parts(e.rgdwAxes, count) };
        out.axes = axes.iter().map(|&v| axis(by_id, v)).collect();
    }
    if flags & DIEP_DIRECTION != 0 {
        out.coordinates = match e.dwFlags & (DIEFF_CARTESIAN | DIEFF_POLAR | DIEFF_SPHERICAL) {
            DIEFF_CARTESIAN => Coordinates::Cartesian,
            DIEFF_POLAR => Coordinates::Polar,
            DIEFF_SPHERICAL => Coordinates::Spherical,
            _ => return Err(hr::INVALIDPARAM),
        };
        if count == 0 || count > 8 || e.rglDirection.is_null() {
            return Err(hr::INVALIDPARAM);
        }
        // SAFETY: rglDirection holds cAxes entries (the caller's contract).
        out.direction = unsafe { std::slice::from_raw_parts(e.rglDirection, count) }.to_vec();
    }
    if flags & DIEP_ENVELOPE != 0 {
        // SAFETY: null or a DIENVELOPE (the caller's contract).
        out.envelope = unsafe { e.lpEnvelope.as_ref() }.map(|v| Envelope {
            attack_level: v.dwAttackLevel,
            attack_time: v.dwAttackTime,
            fade_level: v.dwFadeLevel,
            fade_time: v.dwFadeTime,
        });
    }
    if flags & DIEP_TYPESPECIFICPARAMS != 0 {
        // SAFETY: lpvTypeSpecificParams holds cbTypeSpecificParams bytes.
        out.params =
            unsafe { read_params(out.kind, e.lpvTypeSpecificParams, e.cbTypeSpecificParams) }?;
    }
    Ok(out)
}

/// # Safety
/// `data` is null or holds `size` readable bytes.
unsafe fn read_params(kind: Kind, data: *const c_void, size: u32) -> Result<Params, HRESULT> {
    fn at<T: Copy>(data: *const c_void, size: u32) -> Result<T, HRESULT> {
        if data.is_null() || (size as usize) < size_of::<T>() {
            return Err(hr::INVALIDPARAM);
        }
        // SAFETY: at least size_of::<T>() bytes (checked); T is plain data.
        Ok(unsafe { data.cast::<T>().read_unaligned() })
    }
    Ok(match kind.family() {
        Family::Constant => Params::Constant {
            magnitude: at::<DICONSTANTFORCE>(data, size)?.lMagnitude,
        },
        Family::Ramp => {
            let r = at::<DIRAMPFORCE>(data, size)?;
            Params::Ramp {
                start: r.lStart,
                end: r.lEnd,
            }
        }
        Family::Periodic => {
            let p = at::<DIPERIODIC>(data, size)?;
            Params::Periodic {
                magnitude: p.dwMagnitude,
                offset: p.lOffset,
                phase: p.dwPhase,
                period: p.dwPeriod,
            }
        }
        Family::Condition => {
            let one = size_of::<DICONDITION>();
            let n = size as usize / one;
            if data.is_null() || n == 0 || !(size as usize).is_multiple_of(one) || n > 8 {
                return Err(hr::INVALIDPARAM);
            }
            // SAFETY: n whole DICONDITIONs (checked above).
            let all = unsafe { std::slice::from_raw_parts(data.cast::<DICONDITION>(), n) };
            Params::Condition(
                all.iter()
                    .map(|c| Condition {
                        offset: c.lOffset,
                        positive_coefficient: c.lPositiveCoefficient,
                        negative_coefficient: c.lNegativeCoefficient,
                        positive_saturation: c.dwPositiveSaturation,
                        negative_saturation: c.dwNegativeSaturation,
                        deadband: c.lDeadBand,
                    })
                    .collect(),
            )
        }
        Family::Custom => Params::None,
    })
}

/// The type-specific structure of an [`Effect`], ready to point to.
enum Raw {
    None,
    Constant(DICONSTANTFORCE),
    Ramp(DIRAMPFORCE),
    Periodic(DIPERIODIC),
    Condition(Vec<DICONDITION>),
}

impl Raw {
    fn of(params: &Params) -> Raw {
        match params {
            Params::None => Raw::None,
            Params::Constant { magnitude } => Raw::Constant(DICONSTANTFORCE {
                lMagnitude: *magnitude,
            }),
            Params::Ramp { start, end } => Raw::Ramp(DIRAMPFORCE {
                lStart: *start,
                lEnd: *end,
            }),
            Params::Periodic {
                magnitude,
                offset,
                phase,
                period,
            } => Raw::Periodic(DIPERIODIC {
                dwMagnitude: *magnitude,
                lOffset: *offset,
                dwPhase: *phase,
                dwPeriod: *period,
            }),
            Params::Condition(all) => Raw::Condition(
                all.iter()
                    .map(|c| DICONDITION {
                        lOffset: c.offset,
                        lPositiveCoefficient: c.positive_coefficient,
                        lNegativeCoefficient: c.negative_coefficient,
                        dwPositiveSaturation: c.positive_saturation,
                        dwNegativeSaturation: c.negative_saturation,
                        lDeadBand: c.deadband,
                    })
                    .collect(),
            ),
        }
    }

    fn bytes(&self) -> (*const c_void, u32) {
        fn of<T>(v: &T) -> (*const c_void, u32) {
            ((v as *const T).cast(), size_of::<T>() as u32)
        }
        match self {
            Raw::None => (std::ptr::null(), 0),
            Raw::Constant(v) => of(v),
            Raw::Ramp(v) => of(v),
            Raw::Periodic(v) => of(v),
            Raw::Condition(v) => (v.as_ptr().cast(), size_of_val(v.as_slice()) as u32),
        }
    }
}

/// `f` with `effect` (a wheel effect: one axis at offset 0, `lX` of
/// `c_dfDIJoystick2`) as a `DIEFFECT`, valid during the call.
pub(super) fn with_raw<R>(effect: &Effect, f: impl FnOnce(&DIEFFECT) -> R) -> R {
    let mut axes = [0u32];
    let mut direction = [1i32];
    let mut envelope = effect.envelope.map(|e| DIENVELOPE {
        dwSize: size_of::<DIENVELOPE>() as u32,
        dwAttackLevel: e.attack_level,
        dwAttackTime: e.attack_time,
        dwFadeLevel: e.fade_level,
        dwFadeTime: e.fade_time,
    });
    let raw = Raw::of(&effect.params);
    let (params, size) = raw.bytes();
    let dieffect = DIEFFECT {
        dwSize: size_of::<DIEFFECT>() as u32,
        dwFlags: DIEFF_CARTESIAN | DIEFF_OBJECTOFFSETS,
        dwDuration: effect.duration,
        dwSamplePeriod: effect.sample_period,
        dwGain: effect.gain,
        dwTriggerButton: DIEB_NOTRIGGER,
        dwTriggerRepeatInterval: 0,
        cAxes: 1,
        rgdwAxes: axes.as_mut_ptr(),
        rglDirection: direction.as_mut_ptr(),
        lpEnvelope: envelope
            .as_mut()
            .map_or(null_mut(), |e| e as *mut DIENVELOPE),
        cbTypeSpecificParams: size,
        lpvTypeSpecificParams: params.cast_mut(),
        dwStartDelay: effect.start_delay,
    };
    f(&dieffect)
}

/// Writes the parameters `flags` names into the game's `p`; `axis_value`
/// gives an axis's `rgdwAxes` value (`true`: as an object ID).
///
/// # Safety
/// `p` is null or a writable `DIEFFECT` whose pointers are writable for
/// the sizes it gives.
pub(super) unsafe fn write(
    effect: &Effect,
    p: *mut DIEFFECT,
    flags: u32,
    axis_value: impl Fn(bool, Option<Axis>) -> u32,
) -> HRESULT {
    // SAFETY: the caller's contract.
    let Some(e) = (unsafe { p.as_mut() }) else {
        return hr::INVALIDPARAM;
    };
    if e.dwSize != size_of::<DIEFFECT>() as u32 && e.dwSize != DIEFFECT_DX5_SIZE {
        return hr::INVALIDPARAM;
    }
    if flags & DIEP_DURATION != 0 {
        e.dwDuration = effect.duration;
    }
    if flags & DIEP_SAMPLEPERIOD != 0 {
        e.dwSamplePeriod = effect.sample_period;
    }
    if flags & DIEP_GAIN != 0 {
        e.dwGain = effect.gain;
    }
    if flags & DIEP_STARTDELAY != 0 && e.dwSize >= size_of::<DIEFFECT>() as u32 {
        e.dwStartDelay = effect.start_delay;
    }
    let n = effect.axes.len();
    if flags & (DIEP_AXES | DIEP_DIRECTION) != 0 && (e.cAxes as usize) < n {
        e.cAxes = n as u32;
        return hr::MOREDATA;
    }
    if flags & DIEP_AXES != 0 {
        if e.rgdwAxes.is_null() {
            return hr::INVALIDPARAM;
        }
        let by_id = e.dwFlags & DIEFF_OBJECTIDS != 0;
        for (i, axis) in effect.axes.iter().enumerate() {
            // SAFETY: rgdwAxes holds cAxes ≥ n entries.
            unsafe { *e.rgdwAxes.add(i) = axis_value(by_id, *axis) };
        }
        e.cAxes = n as u32;
    }
    if flags & DIEP_DIRECTION != 0 {
        if e.rglDirection.is_null() {
            return hr::INVALIDPARAM;
        }
        for i in 0..n {
            // SAFETY: rglDirection holds cAxes ≥ n entries.
            unsafe { *e.rglDirection.add(i) = effect.direction.get(i).copied().unwrap_or(0) };
        }
        let coordinates = match effect.coordinates {
            Coordinates::Cartesian => DIEFF_CARTESIAN,
            Coordinates::Polar => DIEFF_POLAR,
            Coordinates::Spherical => DIEFF_SPHERICAL,
        };
        e.dwFlags = (e.dwFlags & !(DIEFF_CARTESIAN | DIEFF_POLAR | DIEFF_SPHERICAL)) | coordinates;
        e.cAxes = n as u32;
    }
    if flags & DIEP_ENVELOPE != 0 {
        match (effect.envelope, e.lpEnvelope.is_null()) {
            (None, _) => e.lpEnvelope = null_mut(),
            (Some(_), true) => return hr::INVALIDPARAM,
            (Some(v), false) => {
                // SAFETY: a writable DIENVELOPE (the caller's contract).
                unsafe {
                    *e.lpEnvelope = DIENVELOPE {
                        dwSize: size_of::<DIENVELOPE>() as u32,
                        dwAttackLevel: v.attack_level,
                        dwAttackTime: v.attack_time,
                        dwFadeLevel: v.fade_level,
                        dwFadeTime: v.fade_time,
                    }
                };
            }
        }
    }
    if flags & DIEP_TYPESPECIFICPARAMS != 0 {
        let raw = Raw::of(&effect.params);
        let (data, size) = raw.bytes();
        if e.cbTypeSpecificParams < size {
            e.cbTypeSpecificParams = size;
            return hr::MOREDATA;
        }
        if size > 0 {
            if e.lpvTypeSpecificParams.is_null() {
                return hr::INVALIDPARAM;
            }
            // SAFETY: the game's buffer holds cbTypeSpecificParams ≥ size
            // bytes; `data` holds size bytes.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    data.cast::<u8>(),
                    e.lpvTypeSpecificParams.cast(),
                    size as usize,
                )
            };
        }
        e.cbTypeSpecificParams = size;
    }
    hr::OK
}

/// The parameter flags a game's change means for the real effect: its
/// direction and axes become the force's sign and size.
fn real_flags(flags: u32) -> u32 {
    const PASSED: u32 = DIEP_DURATION
        | DIEP_SAMPLEPERIOD
        | DIEP_GAIN
        | DIEP_STARTDELAY
        | DIEP_ENVELOPE
        | DIEP_TYPESPECIFICPARAMS
        | DIEP_START
        | DIEP_NORESTART
        | DIEP_NODOWNLOAD;
    let moved = if flags & (DIEP_AXES | DIEP_DIRECTION) != 0 {
        DIEP_TYPESPECIFICPARAMS | DIEP_ENVELOPE
    } else {
        0
    };
    (flags & PASSED) | moved
}

/// `INFINITE`, for durations and iterations.
const INFINITE: u32 = u32::MAX;

/// Every parameter a real effect is sent again with.
const ALL_PARAMS: u32 = DIEP_DURATION
    | DIEP_SAMPLEPERIOD
    | DIEP_GAIN
    | DIEP_STARTDELAY
    | DIEP_ENVELOPE
    | DIEP_TYPESPECIFICPARAMS;

/// One effect the game made.
struct Slot {
    id: u64,
    /// The game's object, for `EnumCreatedEffectObjects`.
    object: usize,
    game: Effect,
    real: Option<RealEffect>,
    /// The real effect couldn't be made; tried again on [`Effects::revive`].
    failed: bool,
    /// Started by the game (with these iterations) and not stopped.
    playing: Option<u32>,
}

impl Slot {
    /// Complete enough for the real wheel: axes and its own parameters.
    fn complete(&self) -> bool {
        !self.game.axes.is_empty() && self.game.params != Params::None
    }

    /// Makes the real effect when there is none yet and it can be made
    /// (a wheel held exclusively, a complete effect), and starts it when
    /// the game has it playing: an effect started before it was complete,
    /// or after a reset, plays once it can.
    fn ensure_real(&mut self, wheel: Option<&Real>, ffb: &FfbSettings) {
        let Some(wheel) = wheel.filter(|w| w.device.ffb && w.exclusive()) else {
            return;
        };
        if self.real.is_some() || self.failed || !self.complete() {
            return;
        }
        let made = self.game.for_wheel(ffb);
        match wheel.create_effect(&made) {
            Ok(real) => {
                log!("effect {}: made on the wheel: {}", self.id, made.describe());
                if let Some(iterations) = self.playing {
                    real.start(iterations, 0);
                }
                self.real = Some(real);
            }
            Err(code) => {
                log!(
                    "effect {}: the wheel refused {} ({code:#x})",
                    self.id,
                    made.describe()
                );
                self.failed = true;
            }
        }
    }
}

/// The game's effects.
#[derive(Default)]
pub(super) struct Effects {
    slots: Vec<Slot>,
    next: u64,
    /// The wheel's driver refused a `DIEP_NORESTART` update: it isn't
    /// asked again (every frame in CMR3).
    no_restart_refused: bool,
}

impl Effects {
    fn slot(&mut self, id: u64) -> Option<&mut Slot> {
        self.slots.iter_mut().find(|s| s.id == id)
    }

    pub fn add(
        &mut self,
        object: This,
        game: Effect,
        wheel: Option<&Real>,
        ffb: &FfbSettings,
    ) -> u64 {
        self.next += 1;
        let mut slot = Slot {
            id: self.next,
            object: object as usize,
            game,
            real: None,
            failed: false,
            playing: None,
        };
        slot.ensure_real(wheel, ffb);
        self.slots.push(slot);
        self.next
    }

    pub fn remove(&mut self, id: u64) {
        self.slots.retain(|s| s.id != id);
    }

    pub fn game(&self, id: u64) -> Option<&Effect> {
        self.slots.iter().find(|s| s.id == id).map(|s| &s.game)
    }

    pub fn objects(&self) -> Vec<This> {
        self.slots.iter().map(|s| s.object as This).collect()
    }

    /// The game's new parameters (`game`, as changed by `flags`).
    pub fn set(
        &mut self,
        id: u64,
        game: Effect,
        flags: u32,
        wheel: Option<&Real>,
        ffb: &FfbSettings,
    ) -> HRESULT {
        let Some(i) = self.slots.iter().position(|s| s.id == id) else {
            return hr::NOTFOUND;
        };
        let slot = &mut self.slots[i];
        slot.game = game;
        if flags & DIEP_START != 0 {
            slot.playing = Some(1);
        }
        let Some(real) = &slot.real else {
            // Made now, with every parameter, and started if playing.
            slot.ensure_real(wheel, ffb);
            return hr::OK;
        };
        let made = slot.game.for_wheel(ffb);
        let mut flags = real_flags(flags);
        if self.no_restart_refused {
            flags &= !DIEP_NORESTART;
        }
        let mut code = real.set(&made, flags);
        if !hr::ok(code) && flags & DIEP_NORESTART != 0 {
            // Some drivers refuse to update a playing effect in place.
            code = real.set(&made, flags & !DIEP_NORESTART);
            if hr::ok(code) {
                log!(
                    "the wheel refuses DIEP_NORESTART updates: they restart the effect from now on"
                );
                self.no_restart_refused = true;
            }
        }
        if !hr::ok(code) {
            log!(
                "effect {id}: the wheel refused new parameters ({code:#x}): {}",
                made.describe()
            );
        }
        hr::OK
    }

    pub fn start(
        &mut self,
        id: u64,
        iterations: u32,
        flags: u32,
        wheel: Option<&Real>,
        ffb: &FfbSettings,
    ) -> HRESULT {
        let Some(slot) = self.slot(id) else {
            return hr::NOTFOUND;
        };
        slot.playing = Some(iterations);
        match &slot.real {
            Some(real) => {
                let code = real.start(iterations, flags);
                if !hr::ok(code) {
                    log!("effect {id}: the wheel didn't start it ({code:#x})");
                }
            }
            None => slot.ensure_real(wheel, ffb),
        }
        hr::OK
    }

    pub fn stop(&mut self, id: u64) -> HRESULT {
        let Some(slot) = self.slot(id) else {
            return hr::NOTFOUND;
        };
        slot.playing = None;
        if let Some(real) = &slot.real {
            real.stop();
        }
        hr::OK
    }

    /// `DIEGES_*`: the wheel's answer, else whether the game started it.
    pub fn status(&mut self, id: u64) -> u32 {
        let Some(slot) = self.slot(id) else {
            return 0;
        };
        match slot.real.as_ref().and_then(RealEffect::status) {
            Some(status) => status,
            None if slot.playing.is_some() => DIEGES_PLAYING,
            None => 0,
        }
    }

    pub fn download(&mut self, id: u64, wheel: Option<&Real>, ffb: &FfbSettings) -> HRESULT {
        let Some(slot) = self.slot(id) else {
            return hr::NOTFOUND;
        };
        if !slot.complete() {
            return hr::INVALIDPARAM;
        }
        slot.ensure_real(wheel, ffb);
        if let Some(real) = &slot.real {
            real.download();
        }
        hr::OK
    }

    pub fn unload(&mut self, id: u64) -> HRESULT {
        let Some(slot) = self.slot(id) else {
            return hr::NOTFOUND;
        };
        slot.playing = None;
        if let Some(real) = &slot.real {
            real.unload();
        }
        hr::OK
    }

    /// `DISFFC_STOPALL` or `DISFFC_RESET`: nothing plays; after a reset
    /// the real effects are gone and made again when next needed.
    pub fn stop_all(&mut self, reset: bool) {
        for slot in &mut self.slots {
            slot.playing = None;
            if reset {
                slot.real = None;
                slot.failed = false;
            }
        }
    }

    /// Another wheel drives: the effects are made on it when it is
    /// acquired ([`Effects::revive`]).
    pub fn forget_wheel(&mut self) {
        for slot in &mut self.slots {
            slot.real = None;
            slot.failed = false;
        }
        self.no_restart_refused = false;
    }

    /// After the wheel was (re-)acquired: every effect made, downloaded
    /// and, if the game had it playing, started again. Drivers drop a
    /// device's effects when it is lost or unacquired; the game doesn't
    /// know and never restarts them.
    pub fn revive(&mut self, wheel: Option<&Real>, ffb: &FfbSettings) {
        for slot in &mut self.slots {
            slot.failed = false;
            match &slot.real {
                Some(real) => {
                    real.set(&slot.game.for_wheel(ffb), ALL_PARAMS);
                    real.download();
                    if let Some(iterations) = slot.playing {
                        real.start(iterations, 0);
                        log!("effect {}: restarted on the wheel", slot.id);
                    }
                }
                None => slot.ensure_real(wheel, ffb),
            }
        }
    }

    /// Effects that play forever and that the wheel stopped on its own
    /// (firmwares drop idle effects): started again. Wanszai's revive.
    pub fn keep_alive(&mut self) {
        for slot in &mut self.slots {
            let forever = slot.game.duration == INFINITE || slot.playing == Some(INFINITE);
            if let (Some(iterations), Some(real), true) = (slot.playing, &slot.real, forever)
                && real.status().is_some_and(|s| s & DIEGES_PLAYING == 0)
            {
                real.start(iterations, 0);
                log!("effect {}: stopped by the wheel, started again", slot.id);
            }
        }
    }
}

/// A game's effect object: the virtual device that made it (whose data
/// format names its axes) and its slot on the rig.
pub(super) struct GameEffect {
    view: Arc<Mutex<View>>,
    id: u64,
    kind: Kind,
}

type EffectObject = Object<EffectVtbl, GameEffect>;

/// A new effect object for the game, made by `view`, its slot on `rig`
/// made with `game`.
pub(super) fn create(view: &Arc<Mutex<View>>, rig: &mut Rig, game: Effect) -> This {
    let kind = game.kind;
    let object = EffectObject::create(
        &VTBL,
        GameEffect {
            view: Arc::clone(view),
            id: 0,
            kind,
        },
    );
    let (effects, wheel, ffb) = rig.effects_with_wheel();
    let id = effects.add(object, game, wheel, &ffb);
    // SAFETY: the object was just made and nobody else has it yet.
    unsafe { (*object.cast::<EffectObject>()).value.id = id };
    object
}

static VTBL: EffectVtbl = EffectVtbl {
    base: super::com::UnknownVtbl {
        query_interface,
        add_ref,
        release,
    },
    initialize,
    get_effect_guid,
    get_parameters,
    set_parameters,
    start,
    stop,
    get_effect_status,
    download,
    unload,
    escape,
};

/// Runs `f` on the effect's device and rig (locked in that order, as
/// everywhere), inside [`guard`].
///
/// # Safety
/// `this` is one of this module's effect objects.
unsafe fn with<R>(this: This, failed: R, f: impl FnOnce(&GameEffect, &View, &mut Rig) -> R) -> R {
    guard(failed, || {
        // SAFETY: the caller's contract.
        let effect = unsafe { &EffectObject::get(this).value };
        let view = lock(&effect.view);
        let rig = view.rig();
        let mut rig = lock(&rig);
        f(effect, &view, &mut rig)
    })
}

unsafe extern "system" fn query_interface(
    this: This,
    riid: *const GUID,
    out: *mut This,
) -> HRESULT {
    // SAFETY: a call on our object, as COM makes it.
    guard(hr::GENERIC, || unsafe {
        query::<EffectVtbl, GameEffect>(this, riid, out, &[IID_UNKNOWN, IID_EFFECT])
    })
}

unsafe extern "system" fn add_ref(this: This) -> u32 {
    // SAFETY: a call on our object.
    guard(1, || unsafe { EffectObject::get(this) }.add_ref())
}

unsafe extern "system" fn release(this: This) -> u32 {
    guard(1, || {
        // SAFETY: a call on our object; read before the last release
        // frees it.
        let (view, id) = unsafe {
            let value = &EffectObject::get(this).value;
            (Arc::clone(&value.view), value.id)
        };
        // SAFETY: a call on our object, which isn't used afterwards.
        let left = unsafe { EffectObject::release(this) };
        if left == 0 {
            let rig = lock(&view).rig();
            lock(&rig).effects_mut().remove(id);
        }
        left
    })
}

unsafe extern "system" fn initialize(_: This, _: HINSTANCE, _: u32, _: *const GUID) -> HRESULT {
    hr::OK
}

unsafe extern "system" fn get_effect_guid(this: This, out: *mut GUID) -> HRESULT {
    // SAFETY: a call on our object; `out` is the game's GUID or null.
    unsafe {
        with(this, hr::GENERIC, |effect, _, _| match out.as_mut() {
            Some(out) => {
                *out = guid_of(effect.kind);
                hr::OK
            }
            None => hr::INVALIDPARAM,
        })
    }
}

unsafe extern "system" fn get_parameters(this: This, p: *mut DIEFFECT, flags: u32) -> HRESULT {
    // SAFETY: a call on our object; `p` is the game's DIEFFECT.
    unsafe {
        with(this, hr::GENERIC, |effect, view, rig| {
            let Some(game) = rig.effects_mut().game(effect.id) else {
                return hr::NOTFOUND;
            };
            write(game, p, flags, |by_id, axis| view.axis_value(by_id, axis))
        })
    }
}

unsafe extern "system" fn set_parameters(this: This, p: *const DIEFFECT, flags: u32) -> HRESULT {
    // SAFETY: a call on our object; `p` is the game's DIEFFECT.
    unsafe {
        with(this, hr::GENERIC, |effect, view, rig| {
            let (effects, wheel, ffb) = rig.effects_with_wheel();
            let Some(game) = effects.game(effect.id) else {
                return hr::NOTFOUND;
            };
            match read(game, p, flags, |by_id, v| view.axis_named(by_id, v)) {
                Ok(game) => effects.set(effect.id, game, flags, wheel, &ffb),
                Err(code) => code,
            }
        })
    }
}

unsafe extern "system" fn start(this: This, iterations: u32, flags: u32) -> HRESULT {
    // SAFETY: a call on our object.
    unsafe {
        with(this, hr::GENERIC, |effect, _, rig| {
            let (effects, wheel, ffb) = rig.effects_with_wheel();
            effects.start(effect.id, iterations, flags, wheel, &ffb)
        })
    }
}

unsafe extern "system" fn stop(this: This) -> HRESULT {
    // SAFETY: a call on our object.
    unsafe {
        with(this, hr::GENERIC, |effect, _, rig| {
            rig.effects_mut().stop(effect.id)
        })
    }
}

unsafe extern "system" fn get_effect_status(this: This, out: *mut u32) -> HRESULT {
    // SAFETY: a call on our object; `out` is the game's or null.
    unsafe {
        with(this, hr::GENERIC, |effect, _, rig| match out.as_mut() {
            Some(out) => {
                *out = rig.effects_mut().status(effect.id);
                hr::OK
            }
            None => hr::INVALIDPARAM,
        })
    }
}

unsafe extern "system" fn download(this: This) -> HRESULT {
    // SAFETY: a call on our object.
    unsafe {
        with(this, hr::GENERIC, |effect, _, rig| {
            let (effects, wheel, ffb) = rig.effects_with_wheel();
            effects.download(effect.id, wheel, &ffb)
        })
    }
}

unsafe extern "system" fn unload(this: This) -> HRESULT {
    // SAFETY: a call on our object.
    unsafe {
        with(this, hr::GENERIC, |effect, _, rig| {
            rig.effects_mut().unload(effect.id)
        })
    }
}

unsafe extern "system" fn escape(_: This, _: *mut DIEFFESCAPE) -> HRESULT {
    hr::UNSUPPORTED
}
