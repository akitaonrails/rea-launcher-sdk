//! What a launcher tells the in-game DirectInput wheel layer
//! (`rea-launcher-dinput`), for games that read controllers through
//! DirectInput themselves (CMR3, CMR04, CMR2005).
//!
//! The layer shows the game one virtual wheel ([`VIRTUAL`]) made from the
//! player's wheel and, when they are a separate USB device, their pedals,
//! after the steering lock, deadzone and curve below; force feedback the
//! game makes goes to the real wheel at [`Setup::ffb`]'s strength. The
//! game binds the virtual wheel, whose layout never changes, so a
//! launcher can seed the game's own control settings once per game
//! instead of once per wheel model.
//!
//! Everything lives in one file, [`FILE`], next to the layer's DLL: the
//! [`Setup`] keys the launcher's wheel tab saves, and the
//! [`WheelProfile`] (`wheel_*`, `aux_*`, pedal and axis keys) its
//! calibration found. Both writers keep each other's keys.
//!
//! ```
//! use rea_launcher::wheel::dinput::{Mode, Setup};
//!
//! let mut setup = Setup::default();
//! setup.mode = Mode::On;
//! setup.ffb.strength = 70;
//! let text = setup.to_cfg("", "rea_wheel.ini - written by the launcher");
//! assert_eq!(Setup::from_cfg(&text), setup);
//! ```

use super::{Control, Keys, SteeringLock, WheelProfile, WheelSettings, with_header};
use crate::cfg;
use crate::ffb::FfbSettings;

pub mod options;

/// The layer's settings file, next to its DLL in the game's folder.
pub const FILE: &str = "rea_wheel.ini";

/// The comment a new [`FILE`] starts with.
pub const HEADER: &str = "rea_wheel.ini - the wheel settings, written by the launcher";

/// When the layer shows the game the virtual wheel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// When a wheel is attached at the game's start; otherwise the game
    /// sees its controllers unchanged.
    #[default]
    Auto,
    /// Always, even before a wheel is found (the game sees a centred
    /// wheel until one is plugged in).
    On,
    /// Never: the layer passes everything through untouched.
    Off,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Auto, Mode::On, Mode::Off];

    pub fn key(self) -> &'static str {
        match self {
            Mode::Auto => "auto",
            Mode::On => "on",
            Mode::Off => "off",
        }
    }

    /// `auto`, `on`/`off` and the SDK's other boolean spellings.
    pub fn parse(v: &str) -> Option<Self> {
        match v.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Mode::Auto),
            "on" | "true" | "yes" | "1" => Some(Mode::On),
            "off" | "false" | "no" | "0" => Some(Mode::Off),
            _ => None,
        }
    }
}

/// The virtual wheel's layout. Never change it: launchers seed game
/// profiles that bind these numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VirtualWheel {
    pub name: &'static str,
    /// pid.codes' open-source vendor; the product is the SDK's own.
    pub vid: u16,
    pub pid: u16,
    /// Buttons: the wheel's, then the pedal device's, up to this many.
    pub buttons: usize,
}

/// The one virtual wheel.
pub const VIRTUAL: VirtualWheel = VirtualWheel {
    name: "rea wheel",
    vid: 0x1209,
    pid: 0x5245,
    buttons: 32,
};

/// The virtual wheel's axes in DirectInput object order (`EnumObjects`
/// lists them in this order, so games number them this way).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// Steering, centred.
    X,
    /// Throttle minus brake, centred: for games that bind one pedal axis.
    Y,
    /// Throttle: released at the axis minimum.
    Z,
    /// Brake: released at the axis minimum.
    Rz,
}

impl Axis {
    pub const ALL: [Axis; 4] = [Axis::X, Axis::Y, Axis::Z, Axis::Rz];
}

/// A real device's axes in [`super::DeviceState::axes`] order, as both the
/// launcher (calibration, live check) and the in-game layer read them:
/// `DIJOYSTATE2`'s `lX`, `lY`, `lZ`, `lRx`, `lRy`, `lRz`, `rglSlider[0]`,
/// `rglSlider[1]`, with each axis's `DIPROP_RANGE` set to
/// [`REAL_RANGE`]. A [`Profile`]'s axis numbers index this list.
pub const REAL_AXES: [&str; 8] = ["X", "Y", "Z", "Rx", "Ry", "Rz", "Slider 1", "Slider 2"];

/// The range both sides set on a real device's axes, so a reading is a
/// [`super::DeviceState`] value as it is.
pub const REAL_RANGE: (i32, i32) = (-32_768, 32_767);

/// A [`Control`] with no buttons: the layer passes the wheel's buttons
/// through by number and gives none of them a launcher meaning, so its
/// profile is a `WheelProfile<NoButtons>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NoButtons {}

impl Control for NoButtons {
    const ALL: &'static [Self] = &[];
    fn key(self) -> &'static str {
        match self {}
    }
    fn label(self) -> &'static str {
        match self {}
    }
    fn bit(self) -> u32 {
        match self {}
    }
}

/// The calibration the layer reads, as [`WheelProfile::from_cfg`] of the
/// same file.
pub type Profile = WheelProfile<NoButtons>;

/// The wheel tab's settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setup {
    pub mode: Mode,
    /// Steering lock, pedal inversion and the wheel picked by hand
    /// (`device`: the DirectInput instance GUID; empty picks it).
    pub wheel: WheelSettings,
    /// The pedal device picked by hand (instance GUID); empty: the
    /// calibrated `aux_*` device, else pedals on the wheel.
    pub pedals: String,
    /// Steering deadzone, percent 0..=[`Setup::MAX_DEADZONE`].
    pub deadzone: u8,
    /// Steering curve, 0 (linear) ..= 100 ([`super::curve`]).
    pub linearity: u8,
    /// Force feedback strength and direction (`rumble` is unused).
    pub ffb: FfbSettings,
    /// Write `rea_wheel.log` next to the DLL.
    pub log: bool,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            mode: Mode::Auto,
            wheel: WheelSettings::default(),
            pedals: String::new(),
            deadzone: 0,
            linearity: 0,
            ffb: FfbSettings::default(),
            log: false,
        }
    }
}

impl Setup {
    pub const MAX_DEADZONE: u8 = 25;

    /// The keys from `text`; a missing or bad value keeps its default.
    pub fn from_cfg(text: &str) -> Self {
        let keys = Keys::of(text);
        let d = Setup::default();
        let flag =
            |k: &str, default: bool| keys.get(k).and_then(cfg::parse_bool).unwrap_or(default);
        let percent = |k: &str, max: u8, default: u8| {
            keys.parsed::<u8>(k)
                .filter(|v| *v <= max)
                .unwrap_or(default)
        };
        Setup {
            mode: keys.get("mode").and_then(Mode::parse).unwrap_or(d.mode),
            wheel: WheelSettings {
                lock: keys
                    .parsed::<i64>("lock")
                    .map_or(d.wheel.lock, SteeringLock::from_setting),
                invert_accel: flag("invert_accel", d.wheel.invert_accel),
                invert_brake: flag("invert_brake", d.wheel.invert_brake),
                device: keys.get("device").unwrap_or_default().trim().to_owned(),
            },
            pedals: keys.get("pedals").unwrap_or_default().trim().to_owned(),
            deadzone: percent("deadzone", Self::MAX_DEADZONE, d.deadzone),
            linearity: percent("linearity", 100, d.linearity),
            ffb: FfbSettings {
                strength: percent("ffb_strength", 100, d.ffb.strength),
                invert: flag("ffb_invert", d.ffb.invert),
                rumble: d.ffb.rumble,
            },
            log: flag("log", d.log),
        }
    }

    /// `existing` with these keys set and every other line kept (the
    /// profile's keys, comments); `header` starts a new file.
    pub fn to_cfg(&self, existing: &str, header: &str) -> String {
        let on_off = |b: bool| if b { "on" } else { "off" }.to_owned();
        let pairs = [
            ("mode", self.mode.key().to_owned()),
            ("lock", self.wheel.lock.to_setting().to_string()),
            ("invert_accel", on_off(self.wheel.invert_accel)),
            ("invert_brake", on_off(self.wheel.invert_brake)),
            ("device", self.wheel.device.clone()),
            ("pedals", self.pedals.clone()),
            ("deadzone", self.deadzone.to_string()),
            ("linearity", self.linearity.to_string()),
            ("ffb_strength", self.ffb.strength.to_string()),
            ("ffb_invert", on_off(self.ffb.invert)),
            ("log", on_off(self.log)),
        ];
        pairs
            .into_iter()
            .fold(with_header(existing, header), |text, (k, v)| {
                cfg::set_key(&text, k, &v)
            })
    }

    /// The steering deadzone as a fraction, for [`super::shape`].
    pub fn deadzone_fraction(&self) -> f32 {
        f32::from(self.deadzone.min(Self::MAX_DEADZONE)) / 100.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wheel::{DeviceId, DeviceState};

    #[test]
    fn a_setup_survives_its_file() {
        let setup = Setup {
            mode: Mode::Off,
            wheel: WheelSettings {
                lock: SteeringLock::from_setting(540),
                invert_accel: true,
                invert_brake: false,
                device: "{6f1d2b61-d5a0-11cf-bfc7-444553540000}".into(),
            },
            pedals: "{1234}".into(),
            deadzone: 3,
            linearity: 40,
            ffb: FfbSettings {
                strength: 65,
                invert: true,
                ..FfbSettings::default()
            },
            log: true,
        };
        assert_eq!(Setup::from_cfg(&setup.to_cfg("", "hello")), setup);
    }

    #[test]
    fn missing_or_bad_values_keep_their_defaults() {
        let s =
            Setup::from_cfg("mode = sideways\ndeadzone = 90\nlinearity = x\nffb_strength = 101\n");
        assert_eq!(s, Setup::default());
        assert_eq!(Setup::from_cfg(""), Setup::default());
    }

    #[test]
    fn mode_takes_the_sdk_boolean_spellings() {
        assert_eq!(Mode::parse(" Yes "), Some(Mode::On));
        assert_eq!(Mode::parse("0"), Some(Mode::Off));
        assert_eq!(Mode::parse("AUTO"), Some(Mode::Auto));
        assert_eq!(Mode::parse(""), None);
        for m in Mode::ALL {
            assert_eq!(Mode::parse(m.key()), Some(m));
        }
    }

    #[test]
    fn setup_and_profile_share_one_file() {
        let wheel = DeviceId {
            name: "FANATEC CSL DD".into(),
            vid: 0x0eb7,
            pid: 0x0020,
            guid: "{a}".into(),
        };
        let resting = DeviceState {
            axes: vec![0, 32_767, 32_767],
            buttons: vec![false; 8],
        };
        let profile = Profile::guess(wheel, &resting, None);
        let setup = Setup {
            mode: Mode::On,
            ..Setup::default()
        };
        let text = profile.to_cfg("", "both");
        let text = setup.to_cfg(&text, "");
        let text = profile.to_cfg(&text, "");
        assert_eq!(Setup::from_cfg(&text), setup);
        assert_eq!(Profile::from_cfg(&text), Some(profile));
        assert!(text.starts_with("# both\n"));
    }

    #[test]
    fn the_virtual_layout_is_frozen() {
        assert_eq!(
            (VIRTUAL.vid, VIRTUAL.pid, VIRTUAL.buttons),
            (0x1209, 0x5245, 32)
        );
        assert_eq!(Axis::ALL, [Axis::X, Axis::Y, Axis::Z, Axis::Rz]);
    }
}
