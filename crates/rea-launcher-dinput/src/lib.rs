//! The in-game DirectInput wheel layer: for old games that read wheels
//! through DirectInput 8 themselves (Colin McRae Rally 3, 04 and 2005),
//! a DLL that shows the game **one virtual wheel** ("rea wheel",
//! [`rea_launcher::wheel::dinput::VIRTUAL`]) made from the player's wheel
//! and, when they are a separate USB device, their pedals:
//!
//! - steering after the steering lock, deadzone and curve, so a 900–1080°
//!   wheel turns like the lock the player picked;
//! - throttle and brake on axes of their own (Z, Rz), and combined on Y;
//! - the wheel's buttons, then the pedal device's, and its hat;
//! - force feedback: the game's effects are made on the real wheel, at
//!   the player's strength, kept alive when the wheel is re-acquired.
//!
//! The launcher adds [`DLL`] to the import table of its runtime copy of
//! the game ([`import_into`]) and writes the settings,
//! [`rea_launcher::wheel::dinput::FILE`], next to it. When the DLL loads
//! it points the game's `dinput8!DirectInput8Create` import at its own,
//! which wraps whatever DirectInput the game would have had (the
//! system's, or Xidi's `dinput8.dll` in the game's folder). With the mode
//! off, no settings file, or (`auto`) no wheel attached, the game gets
//! that DirectInput untouched.
//!
//! The pure parts (data formats, axis units, device choice, effects) are
//! here and host-tested; the COM layer is Windows-only.

use rea_launcher::wheel::dinput::{Mode, Profile, Setup};

pub mod devices;
pub mod effect;
pub mod fake;
pub mod format;
pub mod log;
pub mod state;

#[cfg(windows)]
mod win;

pub use devices::MODE_ENV;

/// The DLL's file name, next to the game's executable.
pub const DLL: &str = "rea_dinput.dll";

/// The export a game's runtime copy imports so the loader maps the DLL at
/// start-up ([`import_into`]); the work is done in `DllMain`.
pub const LOAD_EXPORT: &str = "rea_dinput_load";

/// The game executable `bytes` (the launcher's runtime copy, never the
/// player's file) with [`DLL`] in its import table. Unchanged when the
/// import is already there (a runtime copy from an earlier start).
pub fn import_into(bytes: &[u8]) -> Result<Vec<u8>, rea_launcher::pe::edit::EditError> {
    use rea_launcher::pe::edit;
    if edit::imports(bytes)?
        .iter()
        .any(|import| import.dll.eq_ignore_ascii_case(DLL))
    {
        return Ok(bytes.to_vec());
    }
    edit::add_import(bytes, ".readi", DLL, &[LOAD_EXPORT], None)
}

/// What the layer reads at the game's start.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// The mode after [`MODE_ENV`].
    pub mode: Mode,
    pub setup: Setup,
    pub profile: Option<Profile>,
    pub fake: Option<fake::Fake>,
}

impl Config {
    /// From the settings file's text (`None`: missing or unreadable, which
    /// turns the layer off unless the launcher said `on`) and the value of
    /// [`MODE_ENV`].
    pub fn read(text: Option<&str>, env: Option<&str>) -> Config {
        let text = text.unwrap_or_default();
        let setup = Setup::from_cfg(text);
        let file_mode = if text.trim().is_empty() {
            Mode::Off
        } else {
            setup.mode
        };
        Config {
            mode: devices::mode(file_mode, env),
            setup,
            profile: Profile::from_cfg(text),
            fake: fake::Fake::from_cfg(text),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_settings_is_off_unless_the_launcher_says_on() {
        assert_eq!(Config::read(None, None).mode, Mode::Off);
        assert_eq!(Config::read(Some("  \n"), None).mode, Mode::Off);
        assert_eq!(Config::read(None, Some("on")).mode, Mode::On);
    }

    #[test]
    fn the_file_gives_the_mode_setup_profile_and_fake() {
        let mut setup = Setup::default();
        setup.ffb.strength = 70;
        let text = setup.to_cfg("", "rea_wheel.ini");
        let config = Config::read(Some(&text), None);
        assert_eq!(
            (config.mode, config.setup, config.profile, config.fake),
            (Mode::Auto, setup, None, None)
        );
        let config = Config::read(Some("mode = off\nfake = wheel\n"), Some("on"));
        assert_eq!(
            (config.mode, config.fake),
            (Mode::On, Some(fake::Fake::Wheel))
        );
        // A broken file keeps the defaults it can't read.
        let config = Config::read(Some("mode = ???\n\u{0}garbage\nffb_strength = lots"), None);
        assert_eq!((config.mode, config.setup.ffb.strength), (Mode::Auto, 50));
    }
}
