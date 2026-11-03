//! The command line end to end: every flag, `--help`, bad values and their
//! messages, `--check`; on Windows (and under `sdk/scripts/wine-test`) the
//! real @LAUNCHER_EXE@ too.

use std::path::{Path, PathBuf};

use @CRATE_SNAKE@::cli::{self, Invocation, Mode};
use @CRATE_SNAKE@::config::{self, OPTIONS, Settings};
use @CRATE_SNAKE@::edition;
use rea_launcher::cfg::{Kind, Resolution};
use rea_launcher::identify;

fn parse(args: &[&str]) -> Result<Invocation, String> {
    cli::parse(args.iter().map(|arg| (*arg).to_owned()))
}

fn mode(args: &[&str]) -> Mode {
    parse(args).unwrap().mode
}

/// A scratch folder with a hostile name (spaces, apostrophe, accents).
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir()
        .join(format!("@ID@-cli-{name}-{}", std::process::id()))
        .join("Jogos d'Ação");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn modes_follow_the_family_flags() {
    assert_eq!(mode(&[]), Mode::Window { settings_only: false });
    assert_eq!(mode(&["--settings"]), Mode::Window { settings_only: true });
    assert_eq!(mode(&["--no-window"]), Mode::Play);
    assert_eq!(mode(&["--vsync", "off"]), Mode::Play, "an option flag plays");
    assert_eq!(mode(&["--check", "--game", "C:\\Games"]), Mode::Check);
    for help in ["--help", "-h", "/?"] {
        assert_eq!(mode(&[help]), Mode::Help);
    }
    let helper = parse(&["--settings", "--linux"]).unwrap();
    assert!(helper.linux && !helper.wants_console());
    assert!(parse(&["--check"]).unwrap().wants_console());
}

/// Who sees a message comes from the SDK's rule (report::Audience), never
/// from `--no-window` or `--linux` alone (STANDARDS "Messages reach the
/// player").
#[test]
fn the_player_sees_the_errors_of_the_helpers_game_run() {
    let watching = |args: &[&str], wine: bool| parse(args).unwrap().audience(wine).interactive;
    // `<id>-play` and the AppImage start the game run like this, for a
    // player in front of the screen: errors must reach them as dialogs.
    assert!(watching(&["--no-window", "--linux"], true));
    assert!(watching(&["--no-window"], true), "under Wine nobody reads stdout");
    assert!(watching(&[], false), "a double-clicked launcher");
    assert!(!watching(&["--no-window"], false), "a script on Windows");
    // The helper's settings window shows its own messages.
    assert!(!watching(&["--settings", "--linux"], true));
    // The helper reads --check's output and exit code itself.
    assert!(!watching(&["--check", "--linux"], true));
    assert!(parse(&["--check"]).unwrap().is_action());
    assert!(!parse(&["--no-window"]).unwrap().is_action());
}

#[test]
fn every_option_has_a_working_flag() {
    for row in OPTIONS {
        let number;
        let value = match row.kind {
            Kind::Toggle => "off",
            Kind::Choice(choices) => choices[0].0,
            Kind::Size { .. } => "1280x720",
            Kind::Range { min, .. } => {
                number = min.to_string();
                &number
            }
            Kind::Path | Kind::Text => "C:\\Games\\Some Game",
        };
        let flag = row.flag_name(config::STYLE);
        let parsed = parse(&[&flag, value]).unwrap_or_else(|e| panic!("{flag} {value}: {e}"));
        assert_ne!(parsed.flags, Settings::default(), "{flag} {value} set nothing");
        assert!(config::usage().contains(&flag), "--help lacks {flag}");
    }
    let flags = parse(&["--look", "original", "--resolution", "800X600"]).unwrap().flags;
    assert_eq!(flags.picture(None).resolution, Some(Resolution::Size(800, 600)));
}

#[test]
fn bad_command_lines_say_what_is_wrong() {
    let message = |args: &[&str]| parse(args).unwrap_err();
    assert!(message(&["--vsync", "maybe"]).contains("--vsync needs on or off, not maybe"));
    assert!(message(&["--look", "shiny"]).contains("original, recommended or steam_deck"));
    assert!(message(&["--resolution", "10x10"]).contains("WIDTHxHEIGHT or original"));
    assert!(message(&["--resolution"]).contains("--resolution needs a value"));
    assert!(message(&["--frobnicate"]).contains("unknown argument --frobnicate"));
    assert!(message(&["--frobnicate"]).contains("--help"));
    assert!(message(&["--settings", "--no-window"]).contains("cannot be used together"));
}

#[test]
fn help_lists_every_option_and_switch() {
    let usage = config::usage();
    for switch in config::SWITCHES.iter().chain(&["--help"]) {
        assert!(usage.contains(switch), "{switch}");
    }
    assert!(usage.contains("--vsync on|off"));
    assert!(usage.contains("--look original|recommended|steam_deck"));
}

#[test]
fn check_needs_a_game_folder_with_a_known_edition() {
    let cwd = Path::new("/");
    let none = cli::check(&Settings::default(), cwd).unwrap_err();
    assert!(none.contains("--game"), "{none}");

    let dir = scratch("check");
    let game = parse(&["--check", "--game", &dir.display().to_string()]).unwrap().flags;
    let missing = cli::check(&game, cwd).unwrap_err();
    assert!(missing.contains("There is no"), "{missing}");

    std::fs::write(dir.join(edition::GAME_EXE), b"some other build").unwrap();
    let unknown = cli::check(&game, cwd).unwrap_err();
    let digest = identify::hex(&identify::sha256(b"some other build"));
    assert!(unknown.contains(&digest), "{unknown}");
    assert!(unknown.contains(edition::UNKNOWN_EDITION_ISSUE), "{unknown}");
}

/// The real program, as the Linux helper runs it. `--linux` because under
/// Wine (`sdk/scripts/wine-test`) a run without it is a player's: its
/// messages are dialogs, and a dialog would wait forever in a headless
/// test. The SDK's audience rule is tested above.
#[cfg(windows)]
mod program {
    use super::*;
    use std::process::{Command, Output};

    fn run(name: &str, args: &[&str]) -> (Output, PathBuf) {
        let home = scratch(name).join("AppData");
        let output = Command::new(env!("CARGO_BIN_EXE_@LAUNCHER_NAME@"))
            .args(args)
            .arg("--linux")
            .env("LOCALAPPDATA", &home)
            .output()
            .unwrap();
        (output, home.join("@LAUNCHER_NAME@").join("launcher.log"))
    }

    fn text(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }

    #[test]
    fn help_goes_to_the_terminal() {
        let (output, _) = run("help", &["--help"]);
        assert_eq!(output.status.code(), Some(cli::OK));
        assert!(text(&output.stdout).contains("--vsync on|off"));
    }

    #[test]
    fn a_bad_flag_exits_2_with_its_message() {
        let (output, log) = run("bad", &["--vsync", "maybe"]);
        assert_eq!(output.status.code(), Some(cli::USAGE_ERROR));
        assert!(text(&output.stdout).contains("--vsync needs on or off"));
        assert!(std::fs::read_to_string(log).unwrap().contains("--vsync needs on or off"));
    }

    #[test]
    fn check_refuses_an_unknown_edition_with_its_digest() {
        let dir = scratch("unknown-game");
        std::fs::write(dir.join(edition::GAME_EXE), b"some other build").unwrap();
        let game = dir.display().to_string();
        let (output, log) = run("unknown", &["--check", "--game", &game]);
        assert_eq!(output.status.code(), Some(cli::REFUSED));
        let digest = identify::hex(&identify::sha256(b"some other build"));
        assert!(text(&output.stdout).contains(&digest));
        let log = std::fs::read_to_string(log).unwrap();
        assert!(log.contains(&digest) && log.contains(edition::UNKNOWN_EDITION_ISSUE), "{log}");
    }
}

#[cfg(not(windows))]
#[test]
fn the_program_says_it_is_for_windows() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_@LAUNCHER_NAME@"))
        .arg("--help")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(cli::USAGE_ERROR));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Windows program"));
}
