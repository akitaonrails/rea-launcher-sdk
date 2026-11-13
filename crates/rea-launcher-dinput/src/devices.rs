//! Which real devices make the virtual wheel, and how their readings
//! become its [`Reading`]. The choices are [`rig`]'s, the same functions
//! the launcher's Wheel tab calls, so the game gets what the tab showed.

use std::time::{Duration, Instant};

use rea_launcher::wheel::dinput::{Mode, Profile, Setup, VIRTUAL};
use rea_launcher::wheel::rig::{self, Device, Rig};
use rea_launcher::wheel::{self, DeviceShape, DeviceState};

use crate::state::Reading;

/// The environment variable the launcher sets on the game to `on` or
/// `off`, having decided `auto` itself (it seeds the game's controls only
/// when the virtual wheel will be there). It overrides the file's
/// `mode`; anything else leaves the file's.
pub const MODE_ENV: &str = "REA_WHEEL_MODE";

/// The mode: [`MODE_ENV`]'s `on` or `off` (`env`), else the file's.
pub fn mode(file: Mode, env: Option<&str>) -> Mode {
    match env.and_then(Mode::parse) {
        Some(Mode::Auto) | None => file,
        Some(decided) => decided,
    }
}

/// What the layer does with the attached devices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// The game gets the virtual wheel ([`rig::shows_virtual_wheel`]).
    pub show: bool,
    pub wheel: Option<Device>,
    pub pedals: Option<Device>,
    /// Instance GUIDs the game no longer sees: the wheel and pedals, the
    /// wheel's twin collections without force feedback, and spare
    /// interfaces of either.
    pub hidden: Vec<String>,
}

/// The plan for `all`, every game controller attached, in `mode` (after
/// [`MODE_ENV`]). The wheel and pedals are picked among the devices the
/// launcher lists (spare interfaces left out).
pub fn plan(all: &[Device], mode: Mode, setup: &Setup, saved: Option<&Profile>) -> Plan {
    let shapes: Vec<DeviceShape> = all.iter().map(shape).collect();
    let spare = |i: usize| wheel::is_spare_interface(&shapes, i);
    let listed: Vec<Device> = (0..all.len())
        .filter(|&i| !spare(i))
        .map(|i| all[i].clone())
        .collect();
    let decided = Setup {
        mode,
        ..setup.clone()
    };
    let show = rig::shows_virtual_wheel(&decided, &listed);
    let picked = rig::pick(&listed, setup, saved).filter(|_| show);
    let wheel = picked.map(|r| listed[r.wheel].clone());
    let pedals = picked.and_then(|r| r.pedals).map(|i| listed[i].clone());
    let model = |d: &Device| (d.id.vid, d.id.pid);
    let merged: Vec<&Device> = wheel.iter().chain(&pedals).collect();
    let hidden = (0..all.len())
        .filter(|&i| {
            let d = &all[i];
            merged.iter().any(|m| m.id.guid == d.id.guid)
                || wheel
                    .as_ref()
                    .is_some_and(|w| model(w) == model(d) && !d.ffb)
                || (spare(i) && merged.iter().any(|m| model(m) == model(d)))
        })
        .map(|i| all[i].id.guid.clone())
        .collect();
    Plan {
        show,
        wheel,
        pedals,
        hidden,
    }
}

fn shape(d: &Device) -> DeviceShape {
    DeviceShape {
        vid: d.id.vid,
        pid: d.id.pid,
        kind: d.kind,
        axes: d.axes,
    }
}

/// How long a guessed calibration follows the readings: a device's first
/// reading after it is acquired can be all zeros, and nobody presses a
/// pedal in the first second the game holds the wheel.
pub const GUESS_WINDOW: Duration = Duration::from_secs(1);

/// The settings and calibration the virtual wheel is read with.
#[derive(Debug, Clone, PartialEq)]
pub struct Merge {
    pub setup: Setup,
    saved: Option<Profile>,
    /// The wheel, then the pedal device if there is one.
    devices: Vec<Device>,
    profile: Option<Profile>,
    since: Option<Instant>,
}

impl Merge {
    pub fn new(
        setup: Setup,
        saved: Option<Profile>,
        wheel: Device,
        pedals: Option<Device>,
    ) -> Merge {
        Merge {
            setup,
            saved,
            devices: [wheel].into_iter().chain(pedals).collect(),
            profile: None,
            since: None,
        }
    }

    /// The calibration ([`rig::profile`]), its resting readings taken
    /// from the readings during the [`GUESS_WINDOW`] after the first.
    pub fn profile(
        &mut self,
        now: Instant,
        wheel: &DeviceState,
        pedals: Option<&DeviceState>,
    ) -> &Profile {
        let since = *self.since.get_or_insert(now);
        if now.duration_since(since) < GUESS_WINDOW {
            self.profile = None;
        }
        let (devices, saved) = (&mut self.devices, &self.saved);
        self.profile.get_or_insert_with(|| {
            devices[0].initial = wheel.clone();
            if let (Some(device), Some(state)) = (devices.get_mut(1), pedals) {
                device.initial = state.clone();
            }
            let rig = Rig {
                wheel: 0,
                pedals: (devices.len() > 1).then_some(1),
            };
            rig::profile(saved.as_ref(), devices, rig)
        })
    }

    /// The virtual wheel's reading from the wheel's (with its first hat,
    /// `pov`) and the pedal device's; neutral while the wheel can't be
    /// read.
    pub fn reading(
        &mut self,
        now: Instant,
        wheel: Option<(&DeviceState, u32)>,
        pedals: Option<&DeviceState>,
    ) -> Reading {
        let Some((wheel, pov)) = wheel else {
            return Reading::NEUTRAL;
        };
        let profile = self.profile(now, wheel, pedals).clone();
        let controls = rig::virtual_controls(&self.setup, &profile, wheel, pedals);
        let buttons = wheel
            .buttons
            .iter()
            .chain(pedals.into_iter().flat_map(|a| a.buttons.iter()))
            .take(VIRTUAL.buttons)
            .enumerate()
            .fold(0u32, |bits, (i, &down)| bits | (u32::from(down) << i));
        Reading::new(&controls, buttons, pov)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rea_launcher::wheel::{DeviceId, Kind, SteeringLock};

    fn device(name: &str, vid: u16, pid: u16, dev_type: u32, axes: usize, ffb: bool) -> Device {
        let state = DeviceState {
            axes: vec![0; 8],
            buttons: vec![false; 4],
        };
        Device {
            id: DeviceId {
                name: name.into(),
                vid,
                pid,
                guid: format!("{{{name}}}"),
            },
            kind: rig::kind(vid, pid, dev_type, axes),
            axes,
            buttons: 4,
            ffb,
            initial: state.clone(),
            state,
        }
    }

    fn csl() -> Device {
        device("csl", 0x0eb7, 0x0020, rig::DEVTYPE_DRIVING, 6, true)
    }

    fn csl_twin() -> Device {
        device("csl-twin", 0x0eb7, 0x0020, rig::DEVTYPE_DRIVING, 6, false)
    }

    fn pedals() -> Device {
        device("pedals", 0x1209, 0x0001, 0x14, 3, false)
    }

    fn pad() -> Device {
        device("pad", 0x045e, 0x028e, rig::DEVTYPE_GAMEPAD, 4, true)
    }

    #[test]
    fn the_launchers_decision_overrides_the_file() {
        for file in Mode::ALL {
            assert_eq!(mode(file, Some("on")), Mode::On);
            assert_eq!(mode(file, Some("OFF")), Mode::Off);
            assert_eq!(mode(file, None), file);
            assert_eq!(mode(file, Some("sideways")), file);
            assert_eq!(mode(file, Some("auto")), file);
            assert_eq!(mode(file, Some("")), file);
        }
    }

    #[test]
    fn the_wheel_with_force_feedback_drives_and_its_twin_is_hidden() {
        let all = [pad(), csl_twin(), csl()];
        let plan = plan(&all, Mode::Auto, &Setup::default(), None);
        assert!(plan.show);
        assert_eq!(plan.wheel, Some(csl()));
        assert_eq!(plan.pedals, None);
        assert_eq!(plan.hidden, ["{csl-twin}", "{csl}"], "the pad stays");
    }

    #[test]
    fn auto_needs_a_wheel_on_and_off_do_not_ask() {
        let pads = [pad()];
        let auto = plan(&pads, Mode::Auto, &Setup::default(), None);
        assert_eq!((auto.show, auto.wheel, auto.hidden.len()), (false, None, 0));
        let on = plan(&pads, Mode::On, &Setup::default(), None);
        assert_eq!((on.show, on.wheel), (true, None));
        let off = plan(&[csl()], Mode::Off, &Setup::default(), None);
        assert_eq!((off.show, off.wheel, off.hidden.len()), (false, None, 0));
    }

    #[test]
    fn separate_pedals_and_spare_interfaces_of_merged_models_are_hidden() {
        let pedals_spare = device("pedals-spare", 0x1209, 0x0001, 0x14, 0, false);
        let pad_spare = device("pad-spare", 0x045e, 0x028e, 0x14, 0, false);
        let all = [csl(), pedals(), pedals_spare, pad(), pad_spare];
        let setup = Setup {
            pedals: "{pedals}".into(),
            ..Setup::default()
        };
        let plan = plan(&all, Mode::Auto, &setup, None);
        assert_eq!(plan.pedals, Some(pedals()));
        assert_eq!(plan.hidden, ["{csl}", "{pedals}", "{pedals-spare}"]);
    }

    fn state(axes: &[i16], buttons: &[bool]) -> DeviceState {
        DeviceState {
            axes: axes.to_vec(),
            buttons: buttons.to_vec(),
        }
    }

    fn start() -> Instant {
        static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *START.get_or_init(Instant::now)
    }

    /// Past the guessing window.
    fn later() -> Instant {
        start() + 2 * GUESS_WINDOW
    }

    /// The wheel (no pedals on it) and separate pedals resting high, read
    /// at rest first.
    fn merge(lock: SteeringLock, deadzone: u8, linearity: u8) -> Merge {
        let mut setup = Setup::default();
        setup.wheel.lock = lock;
        setup.deadzone = deadzone;
        setup.linearity = linearity;
        let mut m = Merge::new(setup, None, csl(), Some(pedals()));
        let rest = state(&[32_767, 32_767], &[]);
        m.reading(start(), Some((&state(&[0, 0, 0], &[]), 0)), Some(&rest));
        m
    }

    #[test]
    fn steering_is_rigs_lock_deadzone_and_curve() {
        // A quarter turn right of a 1080° wheel: full lock at 540°.
        let wheel = state(&[8_192, 0, 0], &[]);
        let read = |m: &mut Merge| m.reading(later(), Some((&wheel, 0)), None).axes[0];
        assert!((read(&mut merge(SteeringLock::Degrees(540), 0, 0)) - 0.5).abs() < 1e-3);
        assert!((read(&mut merge(SteeringLock::Full, 0, 0)) - 0.25).abs() < 1e-3);
        let m: f32 = (0.5 - 0.1) / 0.9;
        assert!((read(&mut merge(SteeringLock::Degrees(540), 10, 100)) - m * m * m).abs() < 1e-3);
    }

    #[test]
    fn pedals_from_their_own_device_make_separate_and_combined_axes() {
        let mut m = merge(SteeringLock::Full, 0, 0);
        let wheel = state(&[0, 0, 0], &[true, false]);
        // Throttle floored (rest +32767 → −32768), brake untouched.
        let pedals = state(&[-32_768, 32_767], &[true]);
        let r = m.reading(later(), Some((&wheel, 9000)), Some(&pedals));
        assert_eq!(r.axes, [0.0, 1.0, 1.0, -1.0]);
        // The wheel's two buttons, then the pedal device's.
        assert_eq!((r.buttons, r.pov), (0b101, 9000));
        // Pedals unplugged: released.
        let r = m.reading(later(), Some((&wheel, 0)), None);
        assert_eq!(r.axes[1..], [0.0, -1.0, -1.0]);
        assert_eq!(m.reading(later(), None, Some(&pedals)), Reading::NEUTRAL);
    }

    #[test]
    fn buttons_stop_at_the_virtual_wheels_32() {
        let mut m = merge(SteeringLock::Full, 0, 0);
        let wheel = state(&[0], &[true; 30]);
        let pedals = state(&[32_767, 32_767], &[true; 8]);
        assert_eq!(
            m.reading(later(), Some((&wheel, 0)), Some(&pedals)).buttons,
            u32::MAX
        );
    }

    #[test]
    fn a_guess_follows_the_first_second_then_holds() {
        let mut m = Merge::new(Setup::default(), None, csl(), Some(pedals()));
        let wheel = state(&[0, 0, 0], &[]);
        let zeros = state(&[0, 0], &[]);
        let rest = state(&[32_767, 32_767], &[]);
        // A first reading of all zeros finds no pedals there; the next does.
        assert_eq!(m.profile(start(), &wheel, Some(&zeros)).gas, None);
        let found = m.profile(start(), &wheel, Some(&rest)).clone();
        assert_eq!(found.gas.map(|(_, p)| p.axis), Some(0));
        // Past the window a pressed pedal changes nothing.
        let pressed = state(&[-32_768, 32_767], &[]);
        assert_eq!(m.profile(later(), &wheel, Some(&pressed)), &found);
        // A saved calibration for this wheel and these pedals is kept.
        let mut saved = found.clone();
        saved.rotation = 540;
        let mut m = Merge::new(Setup::default(), Some(saved.clone()), csl(), Some(pedals()));
        assert_eq!(m.profile(start(), &wheel, Some(&zeros)), &saved);
    }

    #[test]
    fn devices_are_kinds_by_rig() {
        assert_eq!(csl().kind, Kind::Wheel);
        assert_eq!(pedals().kind, Kind::Other);
    }
}
