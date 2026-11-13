//! The player's real devices behind the virtual wheel of [`super::dinput`]:
//! which wheel and pedals drive it, and what the game is shown for their
//! readings. The launcher's Wheel tab (live check, calibration) and the
//! in-game layer call the same functions here, so what the tab shows is
//! what the game gets.
//!
//! ```
//! use rea_launcher::wheel::dinput::Setup;
//! use rea_launcher::wheel::rig::{self, Device};
//! use rea_launcher::wheel::{DeviceId, DeviceState, Kind};
//!
//! let resting = DeviceState { axes: vec![0, 32_767, 32_767], buttons: vec![false; 8] };
//! let csl = Device {
//!     id: DeviceId { name: "FANATEC CSL DD".into(), vid: 0x0eb7, pid: 0x0020, guid: "{a}".into() },
//!     kind: Kind::Wheel,
//!     axes: 3,
//!     buttons: 8,
//!     ffb: true,
//!     initial: resting.clone(),
//!     state: resting,
//! };
//! let devices = [csl];
//! let setup = Setup::default();
//! let rig = rig::pick(&devices, &setup, None).expect("a wheel");
//! let profile = rig::profile(None, &devices, rig);
//! let controls = rig::virtual_controls(&setup, &profile, &devices[0].state, None);
//! assert_eq!((controls.steer, controls.accel), (0.0, 0.0));
//! ```

use super::dinput::{Mode, NoButtons, Profile, Setup};
use super::wizard::Captures;
use super::{
    Controls, DeviceId, DeviceState, Kind, PedalAxis, Source, WheelCandidate, find_aux, pick_wheel,
    shape,
};
use crate::ffb::FfbSettings;

/// A game controller as DirectInput lists it, with its readings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// `guid` is the DirectInput instance GUID, as [`guid_text`] writes it.
    pub id: DeviceId,
    pub kind: Kind,
    /// How many axes, buttons it has (its state always has every axis of
    /// [`super::dinput::REAL_AXES`]).
    pub axes: usize,
    pub buttons: usize,
    /// It takes force feedback.
    pub ffb: bool,
    /// The first reading after it was opened (pedals at rest).
    pub initial: DeviceState,
    /// The latest reading.
    pub state: DeviceState,
}

/// A GUID the way [`Setup`]'s `device` and `pedals` keys hold it:
/// `{6f1d2b61-d5a0-11cf-bfc7-444553540000}`, lower case.
pub fn guid_text(data1: u32, data2: u16, data3: u16, data4: [u8; 8]) -> String {
    let [a, b, c, d, e, f, g, h] = data4;
    format!(
        "{{{data1:08x}-{data2:04x}-{data3:04x}-{a:02x}{b:02x}-{c:02x}{d:02x}{e:02x}{f:02x}{g:02x}{h:02x}}}"
    )
}

/// `DI8DEVTYPE_GAMEPAD` and `DI8DEVTYPE_DRIVING`: a `dwDevType`'s low byte.
pub const DEVTYPE_GAMEPAD: u32 = 0x15;
pub const DEVTYPE_DRIVING: u32 = 0x16;

/// What a DirectInput device is: [`super::classify`] of its USB ids, with
/// `DI8DEVTYPE_DRIVING` as a wheel type and `DI8DEVTYPE_GAMEPAD` as a game
/// controller.
pub fn kind(vid: u16, pid: u16, dev_type: u32, axes: usize) -> Kind {
    let class = dev_type & 0xff;
    super::classify(&super::DeviceFacts {
        vid,
        pid,
        sdl_wheel_type: class == DEVTYPE_DRIVING,
        game_controller: class == DEVTYPE_GAMEPAD,
        axes,
    })
}

/// The USB ids in a DirectInput product GUID, when DirectInput put them
/// there (`Data1` = PID << 16 | VID, `Data4` ending in "PIDVID"); else
/// 0, 0.
pub fn product_ids(data1: u32, data4: [u8; 8]) -> (u16, u16) {
    if &data4[2..] == b"PIDVID" {
        (data1 as u16, (data1 >> 16) as u16)
    } else {
        (0, 0)
    }
}

/// A `DIJOYSTATE2` as a [`DeviceState`]: `axes` are its `lX`, `lY`, `lZ`,
/// `lRx`, `lRy`, `lRz` and `rglSlider` read at
/// [`super::dinput::REAL_RANGE`] (clamped to `i16`), then the first
/// `count` of `buttons` (`rgbButtons`).
pub fn reading(axes: [i32; 8], buttons: &[u8], count: usize) -> DeviceState {
    DeviceState {
        axes: axes
            .iter()
            .map(|&v| v.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16)
            .collect(),
        buttons: buttons.iter().take(count).map(|&b| b & 0x80 != 0).collect(),
    }
}

/// The devices that drive the virtual wheel, as indices into the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rig {
    pub wheel: usize,
    /// Pedals on their own USB device; `None`: on the wheel.
    pub pedals: Option<usize>,
}

/// The wheel and pedals to drive with: the wheel by [`pick_wheel`] among
/// the wheels and the device picked by hand (`setup.wheel.device`, which
/// may be one [`super::classify`] doesn't call a wheel); the pedals picked
/// by hand (`setup.pedals`; `None` while they are unplugged), else the
/// saved profile's `aux` device ([`find_aux`]), else on the wheel. `None`
/// without a wheel.
pub fn pick(devices: &[Device], setup: &Setup, saved: Option<&Profile>) -> Option<Rig> {
    let chosen = setup.wheel.device.as_str();
    let pool: Vec<usize> = (0..devices.len())
        .filter(|&i| devices[i].kind == Kind::Wheel || devices[i].id.guid == chosen)
        .collect();
    let candidates: Vec<WheelCandidate> = pool
        .iter()
        .map(|&i| WheelCandidate {
            vid: devices[i].id.vid,
            pid: devices[i].id.pid,
            path: &devices[i].id.guid,
            haptic: devices[i].ffb,
        })
        .collect();
    let wheel = pool[pick_wheel(&candidates, saved.map(|p| &p.wheel), chosen)?];
    let pedals = if setup.pedals.is_empty() {
        let ids: Vec<DeviceId> = devices.iter().map(|d| d.id.clone()).collect();
        saved
            .and_then(|p| p.aux.as_ref())
            .and_then(|aux| find_aux(&ids, aux, wheel))
    } else {
        (0..devices.len()).find(|&i| i != wheel && devices[i].id.guid == setup.pedals)
    };
    Some(Rig { wheel, pedals })
}

/// The calibration to read `rig` with: `saved` when it is for the same
/// wheel model, else a guess from the wheel's resting axes; and when the
/// pedals are a device the profile doesn't know, their axes guessed from
/// where they rest (pedals rest at one end of their axis).
pub fn profile(saved: Option<&Profile>, devices: &[Device], rig: Rig) -> Profile {
    let wheel = &devices[rig.wheel];
    let same_model = |id: &DeviceId| (id.vid, id.pid) == (wheel.id.vid, wheel.id.pid);
    let mut profile = match saved {
        Some(p) if same_model(&p.wheel) => p.clone(),
        _ => Profile::guess(wheel.id.clone(), &wheel.initial, None),
    };
    if let Some(pedals) = rig.pedals.map(|i| &devices[i])
        && profile
            .aux
            .as_ref()
            .is_none_or(|aux| (aux.vid, aux.pid) != (pedals.id.vid, pedals.id.pid))
    {
        profile.forget_aux();
        profile.aux = Some(pedals.id.clone());
        let (gas, brake) = resting_pedals(&pedals.initial);
        profile.gas = gas.map(|p| (Source::Aux, p)).or(profile.gas);
        profile.brake = brake.map(|p| (Source::Aux, p)).or(profile.brake);
    }
    profile
}

/// The first two axes of a pedal device resting beyond ±24000, as
/// accelerator and brake ([`super::guess_pedals`] without the wheel's
/// steering axis to skip).
fn resting_pedals(state: &DeviceState) -> (Option<PedalAxis>, Option<PedalAxis>) {
    let mut resting = state
        .axes
        .iter()
        .enumerate()
        .filter(|(_, v)| v.unsigned_abs() > 24_000)
        .map(|(axis, &v)| PedalAxis {
            axis,
            rest: if v > 0 { 32_767 } else { -32_768 },
            peak: if v > 0 { -32_768 } else { 32_767 },
        });
    (resting.next(), resting.next())
}

/// What the virtual wheel shows the game for these readings:
/// [`Profile::read`] (steering lock, pedal calibration and inversion),
/// then the steering deadzone and curve ([`shape`]). `aux` is the pedal
/// device's reading, `None` while it is unplugged or there is none.
pub fn virtual_controls(
    setup: &Setup,
    profile: &Profile,
    wheel: &DeviceState,
    aux: Option<&DeviceState>,
) -> Controls {
    let mut controls = profile.read(&setup.wheel, wheel, aux);
    controls.steer = shape(controls.steer, setup.deadzone_fraction(), setup.linearity);
    controls
}

/// Whether the in-game layer shows the game the virtual wheel, for these
/// attached devices: never when `Off`, always when `On`, and when `Auto`
/// only with a wheel to drive it ([`pick`]: a wheel, or the device picked
/// by hand). The launcher asks before it starts the game (to seed the
/// game's controls only then); the layer asks as the game starts.
pub fn shows_virtual_wheel(setup: &Setup, devices: &[Device]) -> bool {
    match setup.mode {
        Mode::Off => false,
        Mode::On => true,
        Mode::Auto => pick(devices, setup, None).is_some(),
    }
}

/// Takes what the pedal wizard captured into `profile`, `order` being the
/// devices it watched (the wheel first, as [`super::wizard`] wants), and
/// points `setup.pedals` at the pedals' device (empty: on the wheel), so
/// [`pick`] finds the pedals that were calibrated.
pub fn calibrate(
    profile: &mut Profile,
    setup: &mut Setup,
    captures: &Captures<NoButtons>,
    order: &[DeviceId],
) {
    profile.apply(captures, order);
    let on_aux = [profile.gas, profile.brake]
        .iter()
        .any(|p| p.is_some_and(|(src, _)| src == Source::Aux));
    setup.pedals = match (&profile.aux, on_aux) {
        (Some(aux), true) => aux.guid.clone(),
        _ => String::new(),
    };
}

/// The new [`super::dinput::FILE`] text: `existing` with the setup's keys
/// and, when there is one, the calibration's; `header` starts a new file.
pub fn save(existing: &str, setup: &Setup, profile: Option<&Profile>, header: &str) -> String {
    let text = setup.to_cfg(existing, header);
    match profile {
        Some(p) => p.to_cfg(&text, ""),
        None => text,
    }
}

/// The progress of a force-feedback test.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum FfbTest {
    #[default]
    Idle,
    Playing,
    Done,
    /// What went wrong, for the player.
    Failed(String),
}

/// The launcher's view of the devices: DirectInput on Windows and Wine
/// (`rea_launcher_win32::dinput`), a [`Fixed`] list in tests. Calls never
/// block for long: the device work runs elsewhere.
pub trait WheelDevices {
    /// The controllers attached now, with their latest readings.
    fn devices(&mut self) -> Vec<Device>;
    /// List the devices again now instead of at the next rescan.
    fn refresh(&mut self);
    /// Play [`crate::ffb::test_pattern`] on the device with this instance
    /// GUID, at `settings`' strength and direction.
    fn ffb_test(&mut self, guid: &str, settings: FfbSettings);
    fn ffb_status(&mut self) -> FfbTest;
}

/// A device list that changes only when a test changes it, recording what
/// was asked of it.
#[derive(Debug, Clone, Default)]
pub struct Fixed {
    pub devices: Vec<Device>,
    pub refreshes: usize,
    /// Every force-feedback test asked for.
    pub tests: Vec<(String, FfbSettings)>,
    pub status: FfbTest,
}

impl WheelDevices for Fixed {
    fn devices(&mut self) -> Vec<Device> {
        self.devices.clone()
    }

    fn refresh(&mut self) {
        self.refreshes += 1;
    }

    fn ffb_test(&mut self, guid: &str, settings: FfbSettings) {
        self.tests.push((guid.to_owned(), settings));
        self.status = FfbTest::Playing;
    }

    fn ffb_status(&mut self) -> FfbTest {
        self.status.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wheel::wizard::{Status, Step, Wizard};
    use crate::wheel::{SteeringLock, apply_lock, known};

    fn device(name: &str, vid: u16, pid: u16, guid: &str, kind: Kind, axes: Vec<i16>) -> Device {
        let state = DeviceState {
            axes,
            buttons: vec![false; 12],
        };
        Device {
            id: DeviceId {
                name: name.into(),
                vid,
                pid,
                guid: guid.into(),
            },
            kind,
            axes: state.axes.len(),
            buttons: 12,
            ffb: kind == Kind::Wheel,
            initial: state.clone(),
            state,
        }
    }

    /// A CSL DD (pedals into the base on axes 1 and 2, resting high), a
    /// pad, and separate pedals resting low on axes 0 and 1.
    fn rig() -> Vec<Device> {
        vec![
            device("Xbox pad", 0x045e, 0x028e, "{pad}", Kind::Pad, vec![0; 8]),
            device(
                "FANATEC CSL DD",
                0x0eb7,
                0x0020,
                "{csl}",
                Kind::Wheel,
                vec![0, 32_767, 32_767, 0, 0, 0, 0, 0],
            ),
            device(
                "Heusinkveld Sprint",
                0x30b7,
                0x1001,
                "{pedals}",
                Kind::Other,
                vec![-32_768, -32_768, 0, 0, 0, 0, 0, 0],
            ),
        ]
    }

    #[test]
    fn guids_are_written_as_the_contract_keeps_them() {
        let text = guid_text(
            0x6f1d2b61,
            0xd5a0,
            0x11cf,
            [0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00],
        );
        assert_eq!(text, "{6f1d2b61-d5a0-11cf-bfc7-444553540000}");
        assert_eq!(
            guid_text(1, 2, 3, [0; 8]),
            "{00000001-0002-0003-0000-000000000000}"
        );
    }

    #[test]
    fn directinput_facts_become_kinds_ids_and_readings() {
        assert_eq!(kind(0x0eb7, 0x0020, 0x0001_0216, 6), Kind::Wheel);
        assert_eq!(kind(0x1209, 0x0001, DEVTYPE_DRIVING, 3), Kind::Wheel);
        assert_eq!(kind(0x045e, 0x028e, DEVTYPE_GAMEPAD, 4), Kind::Pad);
        assert_eq!(kind(0x1209, 0x0001, 0x14, 2), Kind::Other);
        let pidvid = [0, 0, b'P', b'I', b'D', b'V', b'I', b'D'];
        assert_eq!(product_ids(0x0020_0eb7, pidvid), (0x0eb7, 0x0020));
        assert_eq!(product_ids(0x0020_0eb7, [0; 8]), (0, 0));
        let r = reading(
            [-40_000, 0, 0, 0, 0, 0, 5, 70_000],
            &[0, 0x80, 0, 0, 0, 0x80],
            4,
        );
        assert_eq!(r.axes, vec![-32_768, 0, 0, 0, 0, 0, 5, 32_767]);
        assert_eq!(r.buttons, vec![false, true, false, false]);
    }

    #[test]
    fn the_wheel_is_picked_and_the_pedals_follow_the_setup() {
        let devices = rig();
        let mut setup = Setup::default();
        assert_eq!(
            pick(&devices, &setup, None),
            Some(Rig {
                wheel: 1,
                pedals: None
            })
        );
        setup.pedals = "{pedals}".into();
        assert_eq!(pick(&devices, &setup, None).unwrap().pedals, Some(2));
        setup.pedals = "{unplugged}".into();
        assert_eq!(pick(&devices, &setup, None).unwrap().pedals, None);
        // the wheel itself is never its own pedals
        setup.pedals = "{csl}".into();
        assert_eq!(pick(&devices, &setup, None).unwrap().pedals, None);
    }

    #[test]
    fn a_device_picked_by_hand_drives_even_when_it_isnt_called_a_wheel() {
        let devices = rig();
        let mut setup = Setup::default();
        setup.wheel.device = "{pedals}".into();
        assert_eq!(pick(&devices, &setup, None).unwrap().wheel, 2);
        setup.wheel.device = "{gone}".into();
        assert_eq!(
            pick(&devices, &setup, None).unwrap().wheel,
            1,
            "unplugged: automatic"
        );
    }

    #[test]
    fn the_virtual_wheel_shows_by_mode_and_by_what_is_attached() {
        let devices = rig();
        let pad_only = &devices[..1];
        let mut setup = Setup::default();
        assert!(shows_virtual_wheel(&setup, &devices));
        assert!(!shows_virtual_wheel(&setup, pad_only));
        assert!(!shows_virtual_wheel(&setup, &[]));
        setup.wheel.device = "{pad}".into();
        assert!(shows_virtual_wheel(&setup, pad_only), "picked by hand");
        setup.mode = Mode::On;
        assert!(shows_virtual_wheel(&setup, &[]));
        setup.mode = Mode::Off;
        assert!(!shows_virtual_wheel(&setup, &devices));
    }

    #[test]
    fn no_wheel_no_rig() {
        let devices = vec![rig().remove(0)];
        assert_eq!(pick(&devices, &Setup::default(), None), None);
        assert_eq!(pick(&[], &Setup::default(), None), None);
    }

    #[test]
    fn calibrated_aux_pedals_are_found_without_a_hand_pick() {
        let devices = rig();
        let mut saved = profile(
            None,
            &devices,
            Rig {
                wheel: 1,
                pedals: Some(2),
            },
        );
        saved.aux.as_mut().unwrap().guid = "{other port}".into();
        let rig = pick(&devices, &Setup::default(), Some(&saved)).unwrap();
        assert_eq!(rig.pedals, Some(2), "same model on another port");
    }

    #[test]
    fn a_profile_is_guessed_or_taken_from_the_saved_one() {
        let devices = rig();
        let guessed = profile(
            None,
            &devices,
            Rig {
                wheel: 1,
                pedals: None,
            },
        );
        assert_eq!(guessed.rotation, 1080);
        assert_eq!(
            guessed.gas.map(|(s, p)| (s, p.axis)),
            Some((Source::Wheel, 1))
        );
        assert_eq!(
            guessed.brake.map(|(s, p)| (s, p.axis)),
            Some((Source::Wheel, 2))
        );

        let mut saved = guessed.clone();
        saved.rotation = 900;
        assert_eq!(
            profile(
                Some(&saved),
                &devices,
                Rig {
                    wheel: 1,
                    pedals: None
                }
            ),
            saved
        );
        let mut other = saved.clone();
        other.wheel.pid = 0x0e03;
        assert_eq!(
            profile(
                Some(&other),
                &devices,
                Rig {
                    wheel: 1,
                    pedals: None
                }
            ),
            guessed,
            "another model's profile is not used"
        );
    }

    #[test]
    fn new_separate_pedals_are_guessed_from_where_they_rest() {
        let devices = rig();
        let p = profile(
            None,
            &devices,
            Rig {
                wheel: 1,
                pedals: Some(2),
            },
        );
        assert_eq!(p.aux.as_ref().map(|a| a.guid.as_str()), Some("{pedals}"));
        let gas = p.gas.unwrap();
        assert_eq!((gas.0, gas.1.axis, gas.1.rest), (Source::Aux, 0, -32_768));
        assert_eq!(p.brake.map(|(s, a)| (s, a.axis)), Some((Source::Aux, 1)));
    }

    /// The live check and the game see the same numbers: the lock, then
    /// the deadzone and curve, for the same readings.
    #[test]
    fn virtual_controls_compose_lock_deadzone_and_curve() {
        let devices = rig();
        let setup = Setup {
            mode: Mode::On,
            deadzone: 10,
            linearity: 50,
            ..Setup::default()
        };
        let p = profile(
            None,
            &devices,
            Rig {
                wheel: 1,
                pedals: None,
            },
        );
        let mut wheel = devices[1].state.clone();
        wheel.axes[0] = 4_000;
        wheel.axes[1] = 0; // throttle half way
        let c = virtual_controls(&setup, &p, &wheel, None);
        let locked = apply_lock(4_000.0 / 32_767.0, 1080, SteeringLock::DEFAULT);
        let expected = shape(locked, 0.10, 50);
        assert!(
            (c.steer - expected).abs() < 1e-6,
            "{} vs {expected}",
            c.steer
        );
        assert!(c.steer > 0.0 && c.steer < locked);
        assert!((c.accel - 0.49).abs() < 0.02, "{}", c.accel);
        assert_eq!(c.brake, 0.0);

        wheel.axes[0] = 500; // inside the deadzone after the lock
        assert_eq!(virtual_controls(&setup, &p, &wheel, None).steer, 0.0);
        assert_eq!(known::lookup(0x0eb7, 0x0020).unwrap().rotation, 1080);
    }

    #[test]
    fn unplugged_pedals_read_as_released() {
        let devices = rig();
        let p = profile(
            None,
            &devices,
            Rig {
                wheel: 1,
                pedals: Some(2),
            },
        );
        let mut aux = devices[2].state.clone();
        aux.axes[0] = 32_767;
        let setup = Setup::default();
        assert!(virtual_controls(&setup, &p, &devices[1].state, Some(&aux)).accel > 0.99);
        assert_eq!(
            virtual_controls(&setup, &p, &devices[1].state, None).accel,
            0.0
        );
    }

    /// The wizard on the separate pedals, through to the saved file and
    /// back.
    #[test]
    fn a_wizard_run_becomes_the_saved_calibration() {
        let devices = rig();
        let mut setup = Setup::default();
        let mut p = profile(
            None,
            &devices,
            Rig {
                wheel: 1,
                pedals: None,
            },
        );
        let order = [devices[1].id.clone(), devices[2].id.clone()];
        let mut states = [devices[1].state.clone(), devices[2].state.clone()];
        let mut wizard = Wizard::<NoButtons>::new(vec![Step::Accelerator, Step::Brake], 0);
        let mut now = 0;
        let mut tick = |states: &[DeviceState]| {
            now += 16;
            wizard.tick(now, states)
        };
        for _ in 0..12 {
            tick(&states);
        }
        for (axis, peak) in [(1, 30_000), (0, 31_000)] {
            states[1].axes[axis] = peak;
            tick(&states);
            states[1].axes[axis] = -32_768;
            tick(&states);
        }
        let Status::Done(captures) = tick(&states) else {
            panic!("the wizard is done");
        };
        calibrate(&mut p, &mut setup, &captures, &order);
        assert_eq!(setup.pedals, "{pedals}");
        let gas = p.gas.unwrap();
        assert_eq!((gas.0, gas.1.axis, gas.1.peak), (Source::Aux, 1, 30_000));
        assert_eq!(p.brake.map(|(s, a)| (s, a.axis)), Some((Source::Aux, 0)));

        let text = save("", &setup, Some(&p), "rea_wheel.ini");
        assert_eq!(Setup::from_cfg(&text), setup);
        assert_eq!(Profile::from_cfg(&text), Some(p.clone()));
        assert_eq!(pick(&devices, &setup, Some(&p)).unwrap().pedals, Some(2));
        // saved again over itself: nothing changes
        assert_eq!(save(&text, &setup, Some(&p), "x"), text);
    }

    #[test]
    fn pedals_calibrated_on_the_wheel_clear_the_hand_pick() {
        let devices = rig();
        let mut setup = Setup {
            pedals: "{pedals}".into(),
            ..Setup::default()
        };
        let mut p = profile(
            None,
            &devices,
            Rig {
                wheel: 1,
                pedals: Some(2),
            },
        );
        let captures = Captures {
            gas: Some((
                0,
                PedalAxis {
                    axis: 1,
                    rest: 32_767,
                    peak: -32_768,
                },
            )),
            brake: Some((
                0,
                PedalAxis {
                    axis: 2,
                    rest: 32_767,
                    peak: -32_768,
                },
            )),
            ..Captures::default()
        };
        calibrate(
            &mut p,
            &mut setup,
            &captures,
            &[devices[1].id.clone(), devices[2].id.clone()],
        );
        assert_eq!(setup.pedals, "");
        assert_eq!(p.gas.map(|(s, _)| s), Some(Source::Wheel));
    }

    #[test]
    fn a_setup_alone_saves_without_profile_keys() {
        let text = save("# mine\n", &Setup::default(), None, "x");
        assert!(text.starts_with("# mine\n"));
        assert!(!text.contains("wheel_vid"));
    }

    #[test]
    fn the_fixed_list_records_what_was_asked() {
        let mut fixed = Fixed {
            devices: rig(),
            ..Fixed::default()
        };
        assert_eq!(fixed.devices().len(), 3);
        fixed.refresh();
        fixed.ffb_test("{csl}", FfbSettings::default());
        assert_eq!(fixed.refreshes, 1);
        assert_eq!(
            fixed.tests,
            vec![("{csl}".to_owned(), FfbSettings::default())]
        );
        assert_eq!(fixed.ffb_status(), FfbTest::Playing);
    }
}
