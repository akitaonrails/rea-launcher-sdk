//! The real devices behind the virtual wheel, opened once per wrapped
//! DirectInput and shared by every virtual device the game makes from it
//! and by their effects ([`Rig`]), and where they come from ([`Source`]).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
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
use super::{config, fake, guard, hr, lock, log};
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
    /// Devices whose failure to open was logged (once each).
    told: Vec<String>,
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
        Source {
            inner,
            told: Vec::new(),
        }
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
    pub fn open_plan(&mut self, attached: &[Attached], mode: Mode) -> Option<Opened> {
        let config = config();
        let devices: Vec<Device> = attached.iter().map(|a| a.device.clone()).collect();
        let plan = devices::plan(&devices, mode, &config.setup, config.profile.as_ref());
        if !plan.show {
            return None;
        }
        let wheel = match &plan.wheel {
            Some(device) => {
                Some(self.open(attached, device, "the game gets its DirectInput untouched")?)
            }
            None => None,
        };
        let pedals = plan
            .pedals
            .as_ref()
            .and_then(|device| self.open(attached, device, "left to the game"));
        let mut hidden = plan.hidden;
        if let (Some(wanted), None) = (&plan.pedals, &pedals) {
            hidden.retain(|g| !rig::same_guid(g, &wanted.id.guid));
        }
        Some(Opened {
            wheel,
            pedals,
            hidden,
        })
    }

    /// Opens the attached `device`; a failure is logged once per device,
    /// with what follows from it.
    fn open(&mut self, attached: &[Attached], device: &Device, then: &str) -> Option<Real> {
        let found = attached
            .iter()
            .find(|a| rig::same_guid(&a.device.id.guid, &device.id.guid))?;
        match self.open_attached(found) {
            Ok(real) => Some(real),
            Err(code) => {
                if !self.told.contains(&device.id.guid) {
                    log!("{}: can't be opened ({code:#x}): {then}", device.id.name);
                    self.told.push(device.id.guid.clone());
                }
                None
            }
        }
    }

    fn open_attached(&self, attached: &Attached) -> Result<Real, HRESULT> {
        let com = match attached.fake {
            Some(index) => fake::open(index).ok_or(hr::GENERIC)?,
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
                        return Err(code);
                    }
                    Com::<DeviceVtbl>::from_raw(ptr).ok_or(hr::GENERIC)?
                }
            }
        };
        Real::open(com, attached.device.clone())
    }
}

/// The devices listed again on a thread of its own every [`RESCAN`] until
/// a wheel can be opened (`on` with none at the start), so the game's
/// input thread never waits on a listing; it only takes the result.
/// DirectInput may be called from any thread.
struct Rescan {
    found: Arc<Mutex<Option<Opened>>>,
    stop: Arc<AtomicBool>,
}

impl Rescan {
    fn start(inner: Com<DirectInputVtbl>) -> Rescan {
        let found = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let (out, quit) = (Arc::clone(&found), Arc::clone(&stop));
        std::thread::spawn(move || {
            guard((), || {
                let mut source = Source::new(inner);
                while !quit.load(Ordering::Acquire) {
                    let wait = Instant::now() + RESCAN;
                    while Instant::now() < wait {
                        if quit.load(Ordering::Acquire) {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    let attached = source.attached();
                    if let Some(opened) = source
                        .open_plan(&attached, Mode::On)
                        .filter(|o| o.wheel.is_some())
                    {
                        *lock(&out) = Some(opened);
                        return;
                    }
                }
            });
        });
        Rescan { found, stop }
    }

    fn take(&self) -> Option<Opened> {
        lock(&self.found).take()
    }
}

impl Drop for Rescan {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

/// The real devices, the force feedback state the game set on the
/// virtual wheel, and its effects.
pub(super) struct Rig {
    /// Looking for a wheel (`on` without one).
    rescan: Option<Rescan>,
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
    window: usize,
    cooperated: bool,
    /// Virtual devices the game holds acquired.
    holders: u32,
    keep_alive_at: Instant,
}

impl Rig {
    /// The rig of `opened`; without a wheel it keeps looking for one
    /// through `source`.
    pub fn new(source: Source, opened: Opened) -> Rig {
        let rescan = opened.wheel.is_none().then(|| Rescan::start(source.inner));
        let mut rig = Rig {
            rescan,
            wheel: None,
            pedals: None,
            merge: None,
            hidden: Vec::new(),
            effects: Effects::default(),
            ffb: config().setup.ffb,
            autocenter: 1,
            gain: FULL,
            window: 0,
            cooperated: false,
            holders: 0,
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

    pub fn effects_mut(&mut self) -> &mut Effects {
        &mut self.effects
    }

    /// The effects, with the wheel and settings they are made for.
    pub fn effects_with_wheel(&mut self) -> (&mut Effects, Option<&Real>, FfbSettings) {
        (&mut self.effects, self.wheel.as_ref(), self.ffb)
    }

    /// The game's window: the real devices cooperate on it. Another
    /// virtual device on the same window changes nothing; a new window
    /// while the devices are held takes them again on it.
    pub fn set_window(&mut self, window: HWND) {
        if self.cooperated && self.window == window as usize {
            return;
        }
        self.window = window as usize;
        self.cooperate();
        if self.holders > 0 {
            self.acquire_reals();
        }
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

    /// The virtual wheel's reading now. Without a wheel, one the
    /// [`Rescan`] found is taken first.
    pub fn read(&mut self, now: Instant) -> Reading {
        if let Some(opened) = self.rescan.as_ref().and_then(Rescan::take) {
            self.rescan = None;
            self.found(opened);
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

    /// Drives with a wheel plugged in after the start.
    fn found(&mut self, opened: Opened) {
        if let Some(wheel) = &opened.wheel {
            log!(
                "{} plugged in: the virtual wheel drives with it",
                wheel.device.id.name
            );
        }
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
        let (effects, wheel, ffb) = self.effects_with_wheel();
        effects.revive(wheel, &ffb);
        if let Some(wheel) = self.wheel.as_mut() {
            wheel.settled();
        }
    }

    /// The game's gain to the wheel, as the game set it. The player's
    /// strength goes into each effect instead: a driver may take the gain
    /// and ignore it (Logitech with "allow game to adjust" off, Wine
    /// backends without it), which would lose the strength unseen.
    fn push_gain(&mut self) {
        let gain = self.gain;
        if let Some(wheel) = self.wheel.as_mut().filter(|w| w.exclusive()) {
            let code = wheel.set_dword(PROP_FFGAIN, gain);
            if !hr::ok(code) {
                log!("the wheel refused gain {gain} ({code:#x})");
            }
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
pub(super) type Shared = Arc<Mutex<Rig>>;
