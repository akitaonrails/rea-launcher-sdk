//! The real devices behind the virtual wheel, opened once per wrapped
//! DirectInput and shared by every virtual device the game makes from it
//! and by their effects ([`Rig`]), and where they come from ([`Source`]).

use std::sync::Arc;
use std::time::{Duration, Instant};

use rea_launcher::ffb::FfbSettings;
use rea_launcher::wheel::dinput::Mode;
use rea_launcher::wheel::rig::{self, Device};
use windows_sys::Win32::Devices::HumanInterfaceDevice::{
    DIGFFS_ACTUATORSON, DIGFFS_EMPTY, DIGFFS_POWERON, DIGFFS_STOPPED, DISFFC_RESET, DISFFC_STOPALL,
};
use windows_sys::Win32::Foundation::HWND;
use windows_sys::core::HRESULT;

use super::com::{Com, DeviceVtbl, DirectInputVtbl};
use super::effect::Effects;
use super::real::{self, Attached, PROP_AUTOCENTER, PROP_FFGAIN, Read, Real};
use super::{config, fake, hr, log};
use crate::devices::{self, Merge};
use crate::state::{FULL, Reading};

/// How often effects that play forever are checked on the wheel.
const KEEP_ALIVE: Duration = Duration::from_millis(250);

/// How often the devices are listed again while there is no wheel (`on`
/// with none plugged in at the start).
const RESCAN: Duration = Duration::from_secs(2);

/// The game's DirectInput, for listing and opening real devices.
pub(super) struct Source {
    inner: Com<DirectInputVtbl>,
}

/// The devices a plan opened.
pub(super) struct Opened {
    pub wheel: Option<Real>,
    pub pedals: Option<Real>,
    /// Instance GUIDs the game no longer sees.
    pub hidden: Vec<String>,
}

impl Source {
    pub fn new(inner: Com<DirectInputVtbl>) -> Source {
        Source { inner }
    }

    /// The game controllers attached now, and the test-only fakes.
    pub fn attached(&self) -> Vec<Attached> {
        // SAFETY: a live DirectInput.
        let mut attached = unsafe { real::attached(&self.inner) };
        if let Some(kind) = config().fake {
            attached.extend(fake::attached(kind));
        }
        attached
    }

    /// The devices of the plan for `attached` in `mode`, opened. `None`
    /// when the game gets its DirectInput untouched: no virtual wheel, or
    /// the picked wheel can't be opened (then nothing is hidden).
    pub fn open_plan(&self, attached: &[Attached], mode: Mode) -> Option<Opened> {
        let config = config();
        let devices: Vec<Device> = attached.iter().map(|a| a.device.clone()).collect();
        let plan = devices::plan(&devices, mode, &config.setup, config.profile.as_ref());
        if !plan.show {
            return None;
        }
        let open = |device: &Device| {
            let found = attached
                .iter()
                .find(|a| rig::same_guid(&a.device.id.guid, &device.id.guid))?;
            self.open(found)
        };
        let wheel = match &plan.wheel {
            Some(device) => Some(open(device).or_else(|| {
                log!(
                    "{}: can't be opened: the game gets its DirectInput untouched",
                    device.id.name
                );
                None
            })?),
            None => None,
        };
        let pedals = plan.pedals.as_ref().and_then(open);
        let mut hidden = plan.hidden;
        if let (Some(wanted), None) = (&plan.pedals, &pedals) {
            log!("{}: can't be opened: left to the game", wanted.id.name);
            hidden.retain(|g| !rig::same_guid(g, &wanted.id.guid));
        }
        Some(Opened {
            wheel,
            pedals,
            hidden,
        })
    }

    fn open(&self, attached: &Attached) -> Option<Real> {
        let com = match attached.fake {
            Some(index) => fake::open(index)?,
            None => {
                let mut ptr = std::ptr::null_mut();
                // SAFETY: a live DirectInput; the out pointer takes one
                // reference, which `Com` releases.
                unsafe {
                    let code = (self.inner.vtbl().create_device)(
                        self.inner.raw(),
                        &attached.instance,
                        &mut ptr,
                        std::ptr::null_mut(),
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
}

/// The real devices, the force feedback state the game set on the
/// virtual wheel, and its effects.
pub(super) struct Rig {
    source: Source,
    wheel: Option<Real>,
    pedals: Option<Real>,
    /// `None` without a wheel: the wheel reads neutral.
    merge: Option<Merge>,
    hidden: Vec<String>,
    effects: Effects,
    ffb: FfbSettings,
    /// The game's `DIPROP_AUTOCENTER` and `DIPROP_FFGAIN`.
    autocenter: u32,
    gain: u32,
    /// The wheel took the gain with the player's strength in it, so the
    /// effects go at full strength (else each effect is scaled).
    gain_carries_strength: bool,
    window: usize,
    cooperated: bool,
    /// Virtual devices the game holds acquired.
    holders: u32,
    rescan_at: Instant,
    keep_alive_at: Instant,
}

impl Rig {
    pub fn new(source: Source, opened: Opened) -> Rig {
        let mut rig = Rig {
            source,
            wheel: None,
            pedals: None,
            merge: None,
            hidden: Vec::new(),
            effects: Effects::default(),
            ffb: config().setup.ffb,
            autocenter: 1,
            gain: FULL,
            gain_carries_strength: false,
            window: 0,
            cooperated: false,
            holders: 0,
            rescan_at: Instant::now() + RESCAN,
            keep_alive_at: Instant::now(),
        };
        rig.take(opened);
        rig
    }

    /// Drives with the devices `opened`.
    fn take(&mut self, opened: Opened) {
        let setup = &config().setup;
        self.merge = opened.wheel.as_ref().map(|w| {
            let pedals = opened.pedals.as_ref().map(|p| p.device.clone());
            Merge::new(
                setup.clone(),
                config().profile.clone(),
                w.device.clone(),
                pedals,
            )
        });
        self.wheel = opened.wheel;
        self.pedals = opened.pedals;
        self.hidden = opened.hidden;
        self.effects.forget_wheel();
    }

    pub fn hidden(&self) -> &[String] {
        &self.hidden
    }

    /// The virtual wheel takes force feedback: its wheel does.
    pub fn has_ffb(&self) -> bool {
        self.wheel.as_ref().is_some_and(|w| w.device.ffb)
    }

    /// The settings effects are made with: full strength when the wheel's
    /// gain carries the player's.
    fn effect_ffb(&self) -> FfbSettings {
        FfbSettings {
            strength: if self.gain_carries_strength {
                100
            } else {
                self.ffb.strength
            },
            ..self.ffb
        }
    }

    /// The effects, with the wheel and settings they are made for.
    pub fn effects(&mut self) -> (&mut Effects, Option<&Real>, FfbSettings) {
        let ffb = self.effect_ffb();
        (&mut self.effects, self.wheel.as_ref(), ffb)
    }

    /// The game's window: the real devices cooperate on it.
    pub fn set_window(&mut self, window: HWND) {
        self.window = window as usize;
        self.cooperate();
    }

    fn cooperate(&mut self) {
        let window = self.window as HWND;
        for real in self.wheel.iter_mut().chain(self.pedals.iter_mut()) {
            real.cooperate(window);
        }
        self.cooperated = true;
    }

    /// A virtual device acquired: the first one acquires the real ones.
    pub fn acquire(&mut self) {
        self.holders += 1;
        if self.holders == 1 {
            self.acquire_reals();
        }
    }

    fn acquire_reals(&mut self) {
        if !self.cooperated {
            self.cooperate();
        }
        if let Some(pedals) = self.pedals.as_mut() {
            pedals.acquire();
        }
        if self.wheel.as_mut().is_some_and(Real::acquire) {
            self.wheel_back();
        }
    }

    /// A virtual device unacquired: the last one lets the real ones go.
    pub fn unacquire(&mut self) {
        self.holders = self.holders.saturating_sub(1);
        if self.holders == 0 {
            for real in self.wheel.iter_mut().chain(self.pedals.iter_mut()) {
                real.unacquire();
            }
        }
    }

    /// The virtual wheel's reading now. Without a wheel the devices are
    /// listed again every [`RESCAN`] until one is plugged in.
    pub fn read(&mut self, now: Instant) -> Reading {
        if self.wheel.is_none() && now >= self.rescan_at {
            self.rescan_at = now + RESCAN;
            self.rescan();
        }
        let wheel = self.wheel.as_mut().map(|w| w.read(now));
        let pedals = self.pedals.as_mut().map(|p| p.read(now));
        if matches!(wheel, Some(Read::Back(..))) {
            self.wheel_back();
        }
        let wheel = match wheel {
            Some(Read::State(s, pov) | Read::Back(s, pov)) => Some((s, pov)),
            _ => None,
        };
        let pedals = match pedals {
            Some(Read::State(s, _) | Read::Back(s, _)) => Some(s),
            _ => None,
        };
        let reading = match &mut self.merge {
            Some(merge) => merge.reading(
                now,
                wheel.as_ref().map(|(s, pov)| (s, *pov)),
                pedals.as_ref(),
            ),
            None => Reading::NEUTRAL,
        };
        if now >= self.keep_alive_at {
            self.effects.keep_alive();
            self.keep_alive_at = now + KEEP_ALIVE;
        }
        reading
    }

    /// Lists the devices again; drives with a wheel that is there now.
    fn rescan(&mut self) {
        let attached = self.source.attached();
        let Some(opened) = self.source.open_plan(&attached, Mode::On) else {
            return;
        };
        let Some(name) = opened.wheel.as_ref().map(|w| w.device.id.name.clone()) else {
            return;
        };
        log!("{name} plugged in: the virtual wheel drives with it");
        self.take(opened);
        if self.cooperated {
            self.cooperate();
        }
        if self.holders > 0 {
            self.acquire_reals();
        }
    }

    /// The wheel was acquired (again): its autocentre and gain, and the
    /// game's effects, back on it.
    fn wheel_back(&mut self) {
        let Some(wheel) = self.wheel.as_mut().filter(|w| w.exclusive()) else {
            return;
        };
        wheel.set_dword(PROP_AUTOCENTER, self.autocenter);
        self.push_gain();
        let (effects, wheel, ffb) = self.effects();
        effects.revive(wheel, &ffb);
        if let Some(wheel) = self.wheel.as_mut() {
            wheel.settled();
        }
    }

    /// The game's gain times the player's strength to the wheel; when the
    /// wheel takes it, its autocentre spring is at the player's strength
    /// too, and the effects go at full strength.
    fn push_gain(&mut self) {
        let gain = self.gain * u32::from(self.ffb.strength.min(100)) / 100;
        let carries = match self.wheel.as_mut().filter(|w| w.exclusive()) {
            Some(wheel) => {
                let code = wheel.set_dword(PROP_FFGAIN, gain);
                if !hr::ok(code) {
                    log!(
                        "the wheel refused gain {gain} ({code:#x}): the strength goes into each effect"
                    );
                }
                hr::ok(code)
            }
            None => false,
        };
        if carries != self.gain_carries_strength {
            self.gain_carries_strength = carries;
            let (effects, _, ffb) = self.effects();
            effects.refresh(&ffb);
        }
    }

    pub fn gain(&self) -> u32 {
        self.gain
    }

    pub fn autocenter(&self) -> u32 {
        self.autocenter
    }

    pub fn set_gain(&mut self, gain: u32) {
        self.gain = gain;
        self.push_gain();
    }

    pub fn set_autocenter(&mut self, on: u32) {
        self.autocenter = on;
        if let Some(wheel) = self.wheel.as_mut().filter(|w| w.exclusive()) {
            let code = wheel.set_dword(PROP_AUTOCENTER, on);
            if !hr::ok(code) {
                log!("the wheel refused autocentre {on} ({code:#x})");
            }
        }
    }

    /// `DIGFFS_*`: the wheel's, else a stopped, empty device.
    pub fn ffb_state(&self) -> u32 {
        self.wheel
            .as_ref()
            .filter(|w| w.exclusive())
            .and_then(Real::ffb_state)
            .unwrap_or(DIGFFS_EMPTY | DIGFFS_STOPPED | DIGFFS_ACTUATORSON | DIGFFS_POWERON)
    }

    /// `SendForceFeedbackCommand`, passed to the wheel.
    pub fn command(&mut self, command: u32) -> HRESULT {
        if let Some(wheel) = self.wheel.as_ref().filter(|w| w.exclusive()) {
            wheel.command(command);
        }
        if command & (DISFFC_RESET | DISFFC_STOPALL) != 0 {
            self.effects.stop_all(command == DISFFC_RESET);
        }
        hr::OK
    }
}

/// A rig shared by the virtual devices and effects of one DirectInput.
pub(super) type Shared = Arc<std::sync::Mutex<Rig>>;
