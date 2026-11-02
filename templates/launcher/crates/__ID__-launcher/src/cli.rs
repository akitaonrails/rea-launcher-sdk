//! What a command line asks for (docs/STANDARDS.md "Family consistency":
//! `--game`, `--settings`, `--no-window`, `--check`, `--linux`, `--look`).

use std::path::{Path, PathBuf};

use rea_launcher::cfg::ArgError;
use rea_launcher::paths;

use crate::config::{self, Settings};
use crate::edition::{self, Found};

/// Exit codes. The Linux helper reads the `--settings` ones.
pub const OK: i32 = 0;
/// Refused: an unknown edition, no game folder, a failed `--check`.
pub const REFUSED: i32 = 1;
/// A bad command line.
pub const USAGE_ERROR: i32 = 2;
/// `--settings`: the window was closed without Play.
pub const WINDOW_CLOSED: i32 = 3;
/// `--settings`: no window could be made (the helper then plays anyway).
pub const WINDOW_UNAVAILABLE: i32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// `--help`, `-h`, `/?`.
    Help,
    /// The settings window; then the game, unless `settings_only`
    /// (`--settings`).
    Window { settings_only: bool },
    /// `--check`: identify the game folder and report, never launch.
    Check,
    /// `--no-window`, or any option flag: play with the saved settings and
    /// the flags on top.
    Play,
}

/// A parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub mode: Mode,
    /// Started by the Linux helper (`--linux`): never show a dialog.
    pub linux: bool,
    /// The settings the flags set, to go on top of `launcher.cfg`.
    pub flags: Settings,
}

impl Invocation {
    /// Whether output goes to the terminal that started the launcher
    /// (`--help`, `--check`, `--no-window`); the window shows its own.
    pub fn wants_console(&self) -> bool {
        !matches!(self.mode, Mode::Window { .. })
    }

    /// Whether a person may be watching: messages may become dialogs when
    /// there is no terminal. Never for the Linux helper's runs.
    pub fn interactive(&self) -> bool {
        !self.linux
    }
}

/// Reads the command line (without the program name). The error is the
/// message for the player.
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Invocation, String> {
    let args: Vec<String> = args.into_iter().collect();
    let (flags, switches) = match config::parse_args(args) {
        Ok(parsed) => parsed,
        Err(ArgError::Help(_)) => {
            return Ok(Invocation {
                mode: Mode::Help,
                linux: false,
                flags: Settings::default(),
            });
        }
        Err(error) => {
            return Err(format!(
                "{error}\n\nRun @LAUNCHER_EXE@ --help for the options."
            ));
        }
    };
    let has = |switch: &str| switches.iter().any(|s| s == switch);
    let options = flags != Settings::default();
    let mode = if has("--check") {
        Mode::Check
    } else if has("--settings") {
        if has("--no-window") {
            return Err("--settings and --no-window cannot be used together.".into());
        }
        Mode::Window {
            settings_only: true,
        }
    } else if has("--no-window") || options {
        Mode::Play
    } else {
        Mode::Window {
            settings_only: false,
        }
    };
    Ok(Invocation {
        mode,
        linux: has("--linux"),
        flags,
    })
}

/// The game folder from the settings, made absolute against `cwd`.
pub fn game_folder(settings: &Settings, cwd: &Path) -> Result<PathBuf, String> {
    let game = settings.game.as_deref().ok_or_else(|| {
        format!(
            "No game folder yet. Choose it in the settings window, or run:\n\n\
             @LAUNCHER_EXE@ --game <the folder with {}>",
            edition::GAME_EXE
        )
    })?;
    paths::resolve(game, cwd)
}

/// `--check`: the game folder and its edition, or why it cannot be played.
pub fn check(settings: &Settings, cwd: &Path) -> Result<(PathBuf, Found), String> {
    let game = game_folder(settings, cwd)?;
    let found = edition::find(&game)?;
    Ok((game, found))
}

/// The `--check` report for a playable folder.
pub fn check_report(game: &Path, found: &Found) -> String {
    format!(
        "Ready to play: {} ({}) in {}",
        edition::GAME_EXE,
        found.edition.name,
        game.display()
    )
}
