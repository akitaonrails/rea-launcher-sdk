//! `launcher.cfg`, the command line and `--help`, from one option table
//! (`rea_launcher::cfg::spec`, docs/STANDARDS.md "Settings window and CLI").
//!
//! [`OPTIONS`] names every setting once: its key, its flag (`--vsync` for
//! `vsync`), the values it takes, and the label and description the window
//! and `--help` show. The parser, `--help`, the first-run template and the
//! window rows all come from it, and the window's parity test fails when a
//! row has no control or a control has no row.
//!
//! The options below are EXAMPLES that show the wiring (a look, a size, a
//! toggle). Replace them with options whose effect was seen in the real
//! game (STANDARDS "No unverified options"), and give each one an
//! end-to-end test: window control → key → the game file it changes.

use std::path::PathBuf;

use rea_launcher::cfg::{self, ArgError, KeyStyle, Kind, OptionSpec, Rejected, Resolution, Value, spec};
use rea_launcher::display;

/// The settings file, next to the exe (portable) or in the launcher's
/// AppData folder.
pub const FILE_NAME: &str = "launcher.cfg";

/// Flags spell keys with hyphens: `--display-mode` is `display_mode`.
pub const STYLE: KeyStyle = KeyStyle::Underscore;

/// The launcher's own switches: they take no value and never reach
/// `launcher.cfg`.
pub const SWITCHES: &[&str] = &["--settings", "--no-window", "--check", "--linux"];

/// Looks, as `(value, label)` for the option table.
pub const LOOKS: &[(&str, &str)] = &[
    ("original", "Original"),
    ("recommended", "Recommended"),
    ("steam_deck", "Steam Deck"),
];

/// The smallest and largest sizes the game is offered.
pub const MIN_SIZE: (u32, u32) = (640, 480);
pub const MAX_SIZE: (u32, u32) = (7680, 4320);

/// Every option: `launcher.cfg` key, flag and window control.
pub const OPTIONS: &[OptionSpec] = &[
    OptionSpec::new(
        "game",
        Kind::Path,
        "Game folder",
        "The folder with your installed @GAME@ (@GAME_EXE@). It is only read, never changed.",
    ),
    OptionSpec::new(
        "look",
        Kind::Choice(LOOKS),
        "Look",
        "Sets the picture options at once: as the game shipped, recommended for your \
         screen, or for the Steam Deck. Options set after it win.",
    ),
    OptionSpec::new(
        "resolution",
        Kind::Size {
            min: MIN_SIZE,
            max: MAX_SIZE,
            aliases: cfg::ORIGINAL,
        },
        "Resolution",
        "The game's screen size, or original for the size it shipped with.",
    )
    .with_example("1920x1080"),
    OptionSpec::new(
        "vsync",
        Kind::Toggle,
        "VSync",
        "Waits for the screen's refresh: no tearing, slightly more input delay.",
    ),
    OptionSpec::new(
        "runtime",
        Kind::Path,
        "Runtime folder",
        "Where the launcher keeps its private copy of the game. Default: a runtime \
         folder beside launcher.log.",
    )
    .launcher_only(),
];

/// Comment lines at the top of a new `launcher.cfg`.
const HEADING: &str = "Settings for @LAUNCHER_EXE@ (@GAME@).\n\
    Every key is also a command-line flag: resolution = 1920x1080 is --resolution 1920x1080.\n\
    A key left commented out keeps the game's own value.";

/// A look: a named set of option values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Look {
    /// The game as it shipped.
    Original,
    /// The launcher's pick for the player's screen.
    Recommended,
    SteamDeck,
}

impl Look {
    pub const ALL: [Look; 3] = [Look::Original, Look::Recommended, Look::SteamDeck];

    /// The `launcher.cfg` value.
    pub fn key(self) -> &'static str {
        match self {
            Look::Original => "original",
            Look::Recommended => "recommended",
            Look::SteamDeck => "steam_deck",
        }
    }

    pub fn from_key(key: &str) -> Option<Look> {
        Look::ALL.into_iter().find(|look| look.key() == key)
    }

    /// The window's card title (the option table's label for the value).
    pub fn label(self) -> &'static str {
        LOOKS
            .iter()
            .find(|(key, _)| *key == self.key())
            .map_or("", |(_, label)| label)
    }

    /// The look recommended on `screen`: Steam Deck on a Deck's screen.
    pub fn recommended(screen: Option<(u32, u32)>) -> Look {
        if display::is_steam_deck(screen) {
            Look::SteamDeck
        } else {
            Look::Recommended
        }
    }

    /// The look's values on `screen`. EXAMPLE values: each one must be
    /// verified in the real game before it ships.
    pub fn picture(self, screen: Option<(u32, u32)>) -> Picture {
        let resolution = match self {
            Look::Original => Resolution::Original,
            Look::Recommended => {
                let (w, h) = screen.and_then(display::landscape).unwrap_or((1920, 1080));
                Resolution::Size(w, h)
            }
            Look::SteamDeck => {
                let (w, h) = display::STEAM_DECK_SCREEN;
                Resolution::Size(w, h)
            }
        };
        Picture {
            resolution: Some(resolution),
            vsync: Some(true),
        }
    }
}

/// The picture options for one run. `None` keeps the game's own value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Picture {
    pub resolution: Option<Resolution>,
    pub vsync: Option<bool>,
}

impl Picture {
    /// The values as `(key, launcher.cfg text)`, for the window and the log.
    pub fn values(&self) -> [(&'static str, Option<String>); 2] {
        [
            ("resolution", self.resolution.map(|r| r.to_string())),
            ("vsync", self.vsync.map(|on| cfg::on_off(on).to_owned())),
        ]
    }

    /// One line for `launcher.log`.
    pub fn describe(&self) -> String {
        self.values()
            .iter()
            .map(|(key, value)| format!("{key} {}", value.as_deref().unwrap_or("(game's own)")))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The settings from `launcher.cfg` and the command line. `None` is unset.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    pub game: Option<PathBuf>,
    pub look: Option<Look>,
    pub resolution: Option<Resolution>,
    pub vsync: Option<bool>,
    pub runtime: Option<PathBuf>,
}

impl Settings {
    /// One checked value from the file or a flag.
    pub fn apply(&mut self, key: &str, value: Value) -> Result<(), Rejected> {
        let path = |text: &str| (!text.is_empty()).then(|| PathBuf::from(text));
        match (key, value) {
            ("game", Value::Text(text)) => self.game = path(text),
            ("runtime", Value::Text(text)) => self.runtime = path(text),
            ("look", Value::Choice(choice)) => {
                let look = Look::from_key(choice).ok_or(Rejected::Unknown)?;
                // A look sets all of its values; options after it win.
                self.look = Some(look);
                self.resolution = None;
                self.vsync = None;
            }
            ("resolution", Value::Size(resolution)) => self.resolution = Some(resolution),
            ("vsync", Value::Toggle(on)) => self.vsync = Some(on),
            _ => return Err(Rejected::Unknown),
        }
        Ok(())
    }

    /// These settings with the command line's on top: a flag wins over the
    /// file, and `--look` replaces the file's picture options.
    pub fn merged(mut self, flags: Settings) -> Settings {
        if flags.look.is_some() {
            self.look = flags.look;
            self.resolution = None;
            self.vsync = None;
        }
        self.game = flags.game.or(self.game);
        self.runtime = flags.runtime.or(self.runtime);
        self.resolution = flags.resolution.or(self.resolution);
        self.vsync = flags.vsync.or(self.vsync);
        self
    }

    /// The picture for this run: the look's values, then the options set
    /// on their own.
    pub fn picture(&self, screen: Option<(u32, u32)>) -> Picture {
        let base = self.look.map(|look| look.picture(screen)).unwrap_or_default();
        Picture {
            resolution: self.resolution.or(base.resolution),
            vsync: self.vsync.or(base.vsync),
        }
    }
}

/// Reads `launcher.cfg` text; the warnings name each line it could not use.
pub fn parse_file(text: &str) -> (Settings, Vec<String>) {
    let mut settings = Settings::default();
    let warnings = spec::parse_file(text, FILE_NAME, OPTIONS, |key, value| {
        settings.apply(key, value)
    });
    (settings, warnings)
}

/// Reads the command line: the settings its flags set, and the switches
/// (`--no-window`, …) in the order given.
pub fn parse_args(
    args: impl IntoIterator<Item = String>,
) -> Result<(Settings, Vec<String>), ArgError> {
    let mut settings = Settings::default();
    let switches = spec::parse_args(args, STYLE, OPTIONS, SWITCHES, |key, value| {
        settings.apply(key, value)
    })?;
    Ok((settings, switches))
}

/// A new `launcher.cfg`: every option commented out, with its description.
pub fn template() -> String {
    spec::template(HEADING, OPTIONS)
}

/// The first run's `launcher.cfg`: the template with the recommended look's
/// values saved explicitly, which is how the default becomes the
/// recommended experience (STANDARDS "The game's own settings stay the
/// game's").
pub fn first_run_text(screen: Option<(u32, u32)>) -> String {
    let look = Look::recommended(screen);
    let mut text = set(&template(), "look", look.key());
    for (key, value) in look.picture(screen).values() {
        if let Some(value) = value {
            text = set(&text, key, &value);
        }
    }
    text
}

/// [`spec::set_key`] for the row of `key`.
pub fn set(text: &str, key: &str, value: &str) -> String {
    match spec::find(OPTIONS, key) {
        Some(row) => spec::set_key(text, row, value),
        None => cfg::set_key(text, key, value),
    }
}

/// `--help`.
pub fn usage() -> String {
    format!(
        "@LAUNCHER_EXE@ {version}: plays your own copy of @GAME@ on modern Windows and Linux.\n\
         \n\
         Usage: @LAUNCHER_EXE@ [options]\n\
         \n\
         Without options the settings window opens; Play saves them and starts the game.\n\
         \n\
         Options (each is also a {FILE_NAME} key):\n\
         {options}\n\
         Launcher switches:\n  \
           --settings   only the settings window (exit 0 after Play, 3 when closed)\n  \
           --no-window  play with the saved settings and these options, no window\n  \
           --check      check the game folder and its edition, then exit (see launcher.log)\n  \
           --linux      started by the Linux helper (linux/@ID@-play)\n  \
           --help       this text\n",
        version = env!("CARGO_PKG_VERSION"),
        options = spec::usage(OPTIONS, STYLE),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_template_parses_to_nothing_and_lists_every_option() {
        let text = template();
        assert_eq!(parse_file(&text), (Settings::default(), Vec::new()));
        for row in OPTIONS {
            assert!(text.contains(&format!("#{} =", row.key)), "{}", row.key);
        }
    }

    #[test]
    fn the_first_run_saves_the_recommended_look_explicitly() {
        let (settings, warnings) = parse_file(&first_run_text(Some((2560, 1440))));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(settings.look, Some(Look::Recommended));
        assert_eq!(settings.resolution, Some(Resolution::Size(2560, 1440)));
        assert_eq!(settings.vsync, Some(true));
        let deck = parse_file(&first_run_text(Some((800, 1280)))).0;
        assert_eq!(deck.look, Some(Look::SteamDeck));
        assert_eq!(deck.resolution, Some(Resolution::Size(1280, 800)));
    }

    #[test]
    fn a_look_sets_its_values_and_later_options_win() {
        let (settings, _) = parse_file("resolution = 800x600\nlook = original\nvsync = off\n");
        assert_eq!(
            settings.picture(None),
            Picture {
                resolution: Some(Resolution::Original),
                vsync: Some(false),
            }
        );
        let unset = Settings::default().picture(None);
        assert_eq!(unset, Picture::default(), "unset keeps the game's values");
    }

    #[test]
    fn flags_win_over_the_file_and_a_look_flag_replaces_its_values() {
        let (file, _) = parse_file("game = C:\\Games\\Old\nresolution = 800x600\nvsync = on\n");
        let (flags, _) = parse_args(["--look".into(), "steam_deck".into()]).unwrap();
        let merged = file.clone().merged(flags);
        assert_eq!(merged.game, file.game);
        assert_eq!(merged.picture(None).resolution, Some(Resolution::Size(1280, 800)));
        let (flags, _) = parse_args(["--vsync".into(), "off".into()]).unwrap();
        let merged = file.merged(flags);
        assert_eq!(merged.vsync, Some(false));
        assert_eq!(merged.resolution, Some(Resolution::Size(800, 600)));
    }

    #[test]
    fn bad_lines_are_reported_and_kept() {
        let (settings, warnings) = parse_file("vsync = maybe\nfps = 60\nresolution = 1280x720\n");
        assert_eq!(settings.resolution, Some(Resolution::Size(1280, 720)));
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings[0].contains("on or off"), "{warnings:?}");
        assert!(warnings[1].contains("unknown setting"), "{warnings:?}");
    }
}
