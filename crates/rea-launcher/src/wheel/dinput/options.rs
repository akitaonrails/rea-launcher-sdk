//! [`Setup`] as launcher options: one [`OptionSpec`] row per setting, so
//! the Wheel tab, `launcher.cfg` and the command line are equals
//! (docs/STANDARDS.md). `launcher.cfg` holds the setup; the calibration
//! ([`super::Profile`]) stays in its own file, and the launcher writes the
//! layer's [`super::FILE`] from both at each start
//! (`rig::save("", &setup, profile, HEADER)`).
//!
//! A launcher lists [`OPTIONS`]' rows in its own table (each is also a
//! `pub const`), hands every wheel key to [`Setup::apply`] from its own
//! `apply`, and saves with [`Setup::to_launcher_cfg`]. Defaults are the
//! launcher's: it starts from its own `Setup` (the CMR rally games:
//! `Setup::default()` with a 540° lock) and `apply` changes only the
//! keys `launcher.cfg` and the flags set.
//!
//! ```
//! use rea_launcher::cfg::spec;
//! use rea_launcher::wheel::dinput::Setup;
//! use rea_launcher::wheel::dinput::options::OPTIONS;
//!
//! let mut setup = Setup::default();
//! let problems = spec::parse_file("wheel_lock = 540\nwheel_ffb = 70\n", "launcher.cfg", &OPTIONS, |key, value| {
//!     setup.apply(key, value);
//!     Ok(())
//! });
//! assert!(problems.is_empty());
//! let text = setup.to_launcher_cfg("");
//! assert!(text.contains("wheel_lock = 540") && text.contains("wheel_ffb = 70"));
//! ```

use super::{Mode, Setup};
use crate::cfg::spec::{self, Kind, OptionSpec, Value};
use crate::wheel::SteeringLock;

pub const WHEEL: OptionSpec = OptionSpec::new(
    "wheel",
    Kind::Choice(&[("auto", "Auto"), ("on", "On"), ("off", "Off")]),
    "Wheel support",
    "Show the game one wheel made from your wheel and pedals: auto when a wheel is plugged in as the game starts.",
);

pub const WHEEL_DEVICE: OptionSpec = OptionSpec::new(
    "wheel_device",
    Kind::Text,
    "Wheel",
    "The wheel to drive with, by its DirectInput instance GUID; empty picks one.",
)
.with_placeholder("<guid>");

pub const WHEEL_PEDALS: OptionSpec = OptionSpec::new(
    "wheel_pedals",
    Kind::Text,
    "Pedals",
    "Separate pedals, by their DirectInput instance GUID; empty uses the calibrated pedals or the wheel's own.",
)
.with_placeholder("<guid>");

pub const WHEEL_LOCK: OptionSpec = OptionSpec::new(
    "wheel_lock",
    Kind::Choice(&[
        ("180", "180°"),
        ("270", "270°"),
        ("360", "360°"),
        ("540", "540°"),
        ("900", "900°"),
        ("full", "Full"),
    ]),
    "Steering lock",
    "How far you turn the wheel for full lock in the game. Full is the wheel's whole rotation.",
)
.with_value_aliases(&[("0", "full")]);

pub const WHEEL_DEADZONE: OptionSpec = OptionSpec::new(
    "wheel_deadzone",
    Kind::Range {
        min: 0,
        max: Setup::MAX_DEADZONE as u32,
    },
    "Deadzone",
    "A small band in the centre that does nothing (percent), for a wheel that drifts.",
);

pub const WHEEL_CURVE: OptionSpec = OptionSpec::new(
    "wheel_curve",
    Kind::Range { min: 0, max: 100 },
    "Curve",
    "0 steers as much as you turn; up to 100 softens small turns near the centre.",
);

pub const WHEEL_INVERT_THROTTLE: OptionSpec = OptionSpec::new(
    "wheel_invert_throttle",
    Kind::Toggle,
    "Invert throttle",
    "For a throttle that reads backwards.",
);

pub const WHEEL_INVERT_BRAKE: OptionSpec = OptionSpec::new(
    "wheel_invert_brake",
    Kind::Toggle,
    "Invert brake",
    "For a brake that reads backwards.",
);

pub const WHEEL_FFB: OptionSpec = OptionSpec::new(
    "wheel_ffb",
    Kind::Range { min: 0, max: 100 },
    "FFB strength",
    "How hard the wheel pushes back in the game (percent; 0 is off). Go lower on a direct-drive wheel.",
);

pub const WHEEL_FFB_INVERT: OptionSpec = OptionSpec::new(
    "wheel_ffb_invert",
    Kind::Toggle,
    "Reverse force feedback",
    "Only for a wheel that pulls the wrong way.",
);

pub const WHEEL_LOG: OptionSpec = OptionSpec::new(
    "wheel_log",
    Kind::Toggle,
    "Wheel log",
    "Write rea_wheel.log next to the game, for bug reports.",
)
.launcher_only();

/// Every row, in the Wheel tab's order.
pub const OPTIONS: [OptionSpec; 11] = [
    WHEEL,
    WHEEL_DEVICE,
    WHEEL_PEDALS,
    WHEEL_LOCK,
    WHEEL_DEADZONE,
    WHEEL_CURVE,
    WHEEL_INVERT_THROTTLE,
    WHEEL_INVERT_BRAKE,
    WHEEL_FFB,
    WHEEL_FFB_INVERT,
    WHEEL_LOG,
];

fn on_off(on: bool) -> String {
    if on { "on" } else { "off" }.to_owned()
}

impl Setup {
    /// Takes a value [`OPTIONS`] checked (`spec::parse_file`,
    /// `spec::parse_args`); false for a key that isn't a wheel key, so a
    /// launcher's `apply` can try it first.
    pub fn apply(&mut self, key: &str, value: Value) -> bool {
        let percent = |n: u32, max: u8| u8::try_from(n).unwrap_or(max).min(max);
        match (key, value) {
            ("wheel", Value::Choice(v)) => self.mode = Mode::parse(v).unwrap_or(self.mode),
            ("wheel_device", Value::Text(v)) => self.wheel.device = super::guid(Some(v)),
            ("wheel_pedals", Value::Text(v)) => self.pedals = super::guid(Some(v)),
            ("wheel_lock", Value::Choice(v)) => {
                self.wheel.lock = SteeringLock::from_setting(v.parse().unwrap_or(0));
            }
            ("wheel_deadzone", Value::Number(n)) => self.deadzone = percent(n, Self::MAX_DEADZONE),
            ("wheel_curve", Value::Number(n)) => self.linearity = percent(n, 100),
            ("wheel_invert_throttle", Value::Toggle(on)) => self.wheel.invert_accel = on,
            ("wheel_invert_brake", Value::Toggle(on)) => self.wheel.invert_brake = on,
            ("wheel_ffb", Value::Number(n)) => self.ffb.strength = percent(n, 100),
            ("wheel_ffb_invert", Value::Toggle(on)) => self.ffb.invert = on,
            ("wheel_log", Value::Toggle(on)) => self.log = on,
            _ => return false,
        }
        true
    }

    /// The `launcher.cfg` value of a wheel key (`None` for another key).
    pub fn value(&self, key: &str) -> Option<String> {
        Some(match key {
            "wheel" => self.mode.key().to_owned(),
            "wheel_device" => self.wheel.device.clone(),
            "wheel_pedals" => self.pedals.clone(),
            "wheel_lock" => match self.wheel.lock {
                SteeringLock::Full => "full".to_owned(),
                SteeringLock::Degrees(d) => d.to_string(),
            },
            "wheel_deadzone" => self.deadzone.to_string(),
            "wheel_curve" => self.linearity.to_string(),
            "wheel_invert_throttle" => on_off(self.wheel.invert_accel),
            "wheel_invert_brake" => on_off(self.wheel.invert_brake),
            "wheel_ffb" => self.ffb.strength.to_string(),
            "wheel_ffb_invert" => on_off(self.ffb.invert),
            "wheel_log" => on_off(self.log),
            _ => return None,
        })
    }

    /// `text` (a `launcher.cfg`) with every wheel key set, every other line
    /// kept.
    pub fn to_launcher_cfg(&self, text: &str) -> String {
        OPTIONS.iter().fold(text.to_owned(), |text, row| {
            let value = self.value(row.key).unwrap_or_default();
            spec::set_key(&text, row, &value)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfg::Rejected;
    use crate::ffb::FfbSettings;
    use crate::wheel::WheelSettings;

    fn read(text: &str) -> (Setup, Vec<String>) {
        let mut setup = Setup::default();
        let problems = spec::parse_file(text, "launcher.cfg", &OPTIONS, |key, value| {
            assert!(setup.apply(key, value), "{key}");
            Ok(())
        });
        (setup, problems)
    }

    fn every_value_changed() -> Setup {
        Setup {
            mode: Mode::Off,
            wheel: WheelSettings {
                lock: SteeringLock::Full,
                invert_accel: true,
                invert_brake: true,
                device: "{6f1d2b61-d5a0-11cf-bfc7-444553540000}".into(),
            },
            pedals: "{1234}".into(),
            deadzone: 25,
            linearity: 100,
            ffb: FfbSettings {
                strength: 0,
                invert: true,
                ..FfbSettings::default()
            },
            log: true,
        }
    }

    #[test]
    fn a_setup_survives_launcher_cfg() {
        for setup in [Setup::default(), every_value_changed()] {
            let text = setup.to_launcher_cfg("# mine\n");
            assert!(text.starts_with("# mine\n"));
            let (back, problems) = read(&text);
            assert_eq!(problems, Vec::<String>::new());
            assert_eq!(back, setup);
            assert_eq!(
                back.to_launcher_cfg(&text),
                text,
                "saving again changes nothing"
            );
        }
    }

    #[test]
    fn flags_take_the_same_values() {
        let mut setup = Setup::default();
        let args = ["--wheel", "on", "--wheel_lock=900", "--wheel_ffb", "35"];
        spec::parse_args(
            args.map(String::from),
            crate::cfg::KeyStyle::Underscore,
            &OPTIONS,
            &[],
            |key, value| {
                setup.apply(key, value);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(setup.mode, Mode::On);
        assert_eq!(setup.wheel.lock, SteeringLock::Degrees(900));
        assert_eq!(setup.ffb.strength, 35);
    }

    #[test]
    fn bad_values_are_refused_and_change_nothing() {
        for (key, bad) in [
            ("wheel", "sometimes"),
            ("wheel_lock", "450"),
            ("wheel_deadzone", "26"),
            ("wheel_curve", "-1"),
            ("wheel_ffb", "101"),
            ("wheel_ffb_invert", "maybe"),
        ] {
            let (setup, problems) = read(&format!("{key} = {bad}\n"));
            assert_eq!(setup, Setup::default(), "{key} = {bad}");
            assert_eq!(problems.len(), 1, "{key} = {bad}: {problems:?}");
        }
        let row = spec::find(&OPTIONS, "wheel_lock").unwrap();
        assert_eq!(row.parse("0"), Ok(Value::Choice("full")));
        assert!(matches!(WHEEL_FFB.parse("x"), Err(Rejected::Needs(_))));
    }

    /// A launcher's own default (540° for the CMR rally games) stays
    /// until `launcher.cfg` or a flag says otherwise.
    #[test]
    fn a_launchers_defaults_are_kept_for_keys_it_doesnt_set() {
        let mut setup = Setup::default();
        setup.wheel.lock = SteeringLock::Degrees(540);
        spec::parse_file(
            "wheel_ffb = 30\n",
            "launcher.cfg",
            &OPTIONS,
            |key, value| {
                setup.apply(key, value);
                Ok(())
            },
        );
        assert_eq!(setup.wheel.lock, SteeringLock::Degrees(540));
        assert_eq!(setup.ffb.strength, 30);
        assert_eq!(WHEEL_LOCK.example, None, "the template claims no default");
    }

    #[test]
    fn other_keys_are_left_to_the_launcher() {
        let mut setup = Setup::default();
        assert!(!setup.apply("fps", Value::Number(60)));
        assert_eq!(setup.value("fps"), None);
        assert_eq!(setup, Setup::default());
    }

    #[test]
    fn the_rows_pass_the_parity_check_with_a_window_that_shows_them() {
        let shown: Vec<&str> = OPTIONS
            .iter()
            .filter(|row| !row.launcher_only)
            .map(|row| row.key)
            .collect();
        assert_eq!(shown.len(), 10);
        spec::assert_parity(&OPTIONS, &shown);
        for row in &OPTIONS {
            assert!(Setup::default().value(row.key).is_some(), "{}", row.key);
        }
    }
}
