//! Telling the player something: a message box, the terminal/log, or both.
//!
//! The rule (docs/STANDARDS.md, the CLI and settings-window rules):
//!
//! - **Script-facing commands** (`--help`, `--check`, `--list`…,
//!   [`Mode::Command`]): the terminal. A dialog only when nobody sees the
//!   terminal and a person started the launcher (a double-clicked
//!   shortcut), never in a script.
//! - **Play-time messages** (unknown edition, game folder not found,
//!   already running, a failed install…, [`Mode::Play`]): ALWAYS a dialog
//!   when a person may be watching, AND printed to stdout, which the play
//!   helper, the AppImage or Proton capture in the log.
//!
//! "Nobody sees the terminal": no console attached and stdout either goes
//! nowhere or (under Wine) is redirected — `<id>-play`, the AppImage and
//! Proton always send it to a log. On Windows a redirected stdout
//! (`--help > help.txt`) is the person's own choice and counts as seen.
//! v0.3.0's `console::output` picked the terminal for every redirected
//! stdout, so under Wine play-time errors only reached the log; use this
//! module instead.
//!
//! **Who is watching** is decided by [`Audience`], from the parsed command
//! line ([`Run`]) and Wine. Don't compute `interactive` yourself:
//! `interactive = !no_window` hid play-time errors in CMR3, CMR4 and DiRT
//! (v0.3.x), because the Linux helper always starts the game run with
//! `--no-window` for a player who is watching.
//!
//! ```no_run
//! # #[cfg(windows)] {
//! use rea_launcher_win32::report::{self, Run};
//! // From the launcher's parsed flags (or `Run::from_args`).
//! let run = Run { no_window: true, linux: true, ..Run::default() };
//! let audience = report::audience(run);
//! report::fatal("OutRun 2006", "The game folder was not found.", audience.mode());
//! # }
//! ```

/// The family flags that say what kind of run this is (docs/STANDARDS.md,
/// "Family consistency"). Fill it from the launcher's parsed command line,
/// or with [`Run::from_args`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Run {
    /// A script-facing command whose output is its result: `--help`,
    /// `--check`, `--list`, `--controllers`…
    pub command: bool,
    /// `--no-window`: play with the saved settings, no settings window.
    pub no_window: bool,
    /// `--settings`: only the settings window (exit codes 0/3/4 for the
    /// Linux helper), never the game.
    pub settings_only: bool,
    /// `--linux`: started by the Linux helper (`<id>-play`, the AppImage).
    pub linux: bool,
}

/// The command flags every launcher has; [`Run::from_args`] takes the
/// launcher's own on top (`--list`, `--controllers`…).
pub const COMMAND_FLAGS: &[&str] = &["--help", "-h", "/?", "--check"];

/// The exit code of a command line the launcher can't parse (an unknown
/// flag, a bad value): docs/STANDARDS.md, "Family consistency". 1 is a
/// refusal, 2 a bad command line, in every launcher.
pub const BAD_COMMAND_LINE: i32 = 2;

impl Run {
    /// The run `args` ask for (the launcher's arguments, program name
    /// excluded): [`COMMAND_FLAGS`] or one of `commands` make it a
    /// command; `--no-window`, `--settings` and `--linux` as named. It
    /// looks at every argument, so a flag's value spelled like a flag
    /// counts too: prefer filling [`Run`] from the launcher's parsed
    /// command line when there is one.
    pub fn from_args<S: AsRef<str>>(args: &[S], commands: &[&str]) -> Run {
        let has = |flag: &str| args.iter().any(|arg| arg.as_ref() == flag);
        Run {
            command: COMMAND_FLAGS.iter().chain(commands).any(|flag| has(flag)),
            no_window: has("--no-window"),
            settings_only: has("--settings"),
            linux: has("--linux"),
        }
    }

    /// The run of a command line that failed to parse: report the error
    /// and the usage like a command (the terminal; a dialog only when
    /// nobody sees it), never like a play run, whose dialog is modal and
    /// would hang a script with a typo (a Windows 10 test of CMR, CMR4,
    /// DiRT and SRR, 2026-11-03). Exit [`BAD_COMMAND_LINE`] afterwards.
    pub fn bad_command_line<S: AsRef<str>>(args: &[S]) -> Run {
        Run {
            command: true,
            ..Run::from_args(args, &[])
        }
    }
}

/// Who may see a run's messages: whether its output is a command's result
/// (`command`), and whether a person may be watching, so a dialog may be
/// shown (`interactive`). Build it with [`Audience::of`] (or `audience`
/// on Windows) and hand [`Audience::mode`] to `fatal`/`note`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Audience {
    pub command: bool,
    pub interactive: bool,
}

impl Audience {
    /// The rule (docs/STANDARDS.md, "Messages reach the player"):
    ///
    /// - **A play run** (the window then the game, or `--no-window`): a
    ///   person is watching unless it is `--no-window` on native Windows
    ///   without `--linux`, which is a script. Under Wine `<id>-play`, the
    ///   AppImage and Proton start the game run with `--no-window` for a
    ///   player in front of the screen, and stdout goes to a log nobody
    ///   reads; `--linux` says the same on any system.
    /// - **A command** (`--help`, `--check`…): its output goes to the
    ///   terminal ([`Mode::Command`]); a dialog only when nobody sees the
    ///   terminal and a person started it: not with `--no-window` on
    ///   Windows, and never with `--linux` (the helper reads the output and
    ///   the exit code).
    /// - **The Linux helper's settings window** (`--settings --linux`, as
    ///   CMR2005 and every `<id>-play` start it): the window shows its own
    ///   messages, and when it can't open the helper reads exit code 4 and
    ///   starts the game run, which reports. No dialog.
    pub fn of(run: Run, wine: bool) -> Audience {
        let interactive = if run.command {
            !run.linux && (wine || !run.no_window)
        } else if run.settings_only && run.linux && !run.no_window {
            false
        } else {
            wine || run.linux || !run.no_window
        };
        Audience {
            command: run.command,
            interactive,
        }
    }

    /// The [`Mode`] for this run's messages.
    pub fn mode(self) -> Mode {
        let interactive = self.interactive;
        if self.command {
            Mode::Command { interactive }
        } else {
            Mode::Play { interactive }
        }
    }
}

/// What kind of message it is, and whether a person may be watching
/// (`interactive`). Take it from [`Audience::mode`] rather than building
/// it by hand.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Output of a script-facing command: `--help`, `--check`.
    Command { interactive: bool },
    /// A message while playing or preparing to play.
    Play { interactive: bool },
}

/// What the process can tell about its terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Terminal {
    /// A console is attached (`console::attach_parent`).
    pub attached: bool,
    /// stdout leads to a file or pipe (`console::stdout_usable`).
    pub redirected: bool,
    /// Running under Wine (`wine::is_wine`).
    pub wine: bool,
}

impl Terminal {
    /// Whether a person sees what is printed.
    pub fn seen(&self) -> bool {
        self.attached || (self.redirected && !self.wine)
    }

    /// Whether printing reaches anything (a console, a file, a log).
    pub fn printable(&self) -> bool {
        self.attached || self.redirected
    }
}

/// Where one message goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Route {
    pub print: bool,
    pub dialog: bool,
}

/// The decision table of the module docs.
pub fn route(mode: Mode, terminal: Terminal) -> Route {
    match mode {
        Mode::Command { interactive } => Route {
            print: terminal.printable(),
            dialog: interactive && !terminal.seen(),
        },
        Mode::Play { interactive } => Route {
            print: terminal.printable(),
            dialog: interactive,
        },
    }
}

#[cfg(windows)]
pub use windows::*;

#[cfg(windows)]
mod windows {
    use std::io::Write;

    use super::{Mode, Route, Terminal, route};
    use crate::dialog::{self, Icon};

    /// The [`Audience`](super::Audience) of `run` in this process
    /// (`wine::is_wine`).
    pub fn audience(run: super::Run) -> super::Audience {
        super::Audience::of(run, crate::wine::is_wine())
    }

    /// This process's terminal (attaching to the parent console once).
    pub fn terminal() -> Terminal {
        static ATTACHED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let attached = *ATTACHED.get_or_init(crate::console::attach_parent);
        Terminal {
            attached,
            redirected: crate::console::stdout_usable(),
            wine: crate::wine::is_wine(),
        }
    }

    /// Where a message in `mode` goes in this process.
    pub fn route_here(mode: Mode) -> Route {
        route(mode, terminal())
    }

    fn show(title: &str, text: &str, mode: Mode, icon: Icon) -> Route {
        let route = route_here(mode);
        if route.print {
            let mut out = std::io::stdout();
            let _ = writeln!(out, "{title}: {text}");
            let _ = out.flush();
        }
        if route.dialog {
            dialog::message(title, text, icon);
        }
        route
    }

    /// An error the launcher stops on; returns where it went.
    pub fn fatal(title: &str, text: &str, mode: Mode) -> Route {
        show(title, text, mode, Icon::Error)
    }

    /// Information (a warning when `warning`); returns where it went.
    pub fn note(title: &str, text: &str, mode: Mode, warning: bool) -> Route {
        show(
            title,
            text,
            mode,
            if warning { Icon::Warning } else { Icon::Info },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn t(attached: bool, redirected: bool, wine: bool) -> Terminal {
        Terminal {
            attached,
            redirected,
            wine,
        }
    }

    fn r(print: bool, dialog: bool) -> Route {
        Route { print, dialog }
    }

    #[test]
    fn commands_go_to_the_terminal_and_a_dialog_only_when_unseen() {
        let help = Mode::Command { interactive: true };
        let script = Mode::Command { interactive: false };
        // Started from a terminal.
        assert_eq!(route(help, t(true, true, false)), r(true, false));
        assert_eq!(route(help, t(true, false, true)), r(true, false));
        // `--help > help.txt` on Windows: the person's choice, no dialog.
        assert_eq!(route(help, t(false, true, false)), r(true, false));
        // Under Wine stdout goes to <id>-play's log: nobody sees it.
        assert_eq!(route(help, t(false, true, true)), r(true, true));
        // A double-clicked `--help` shortcut: nothing printable.
        assert_eq!(route(help, t(false, false, false)), r(false, true));
        // Scripts never get a dialog.
        assert_eq!(route(script, t(false, true, true)), r(true, false));
        assert_eq!(route(script, t(false, false, false)), r(false, false));
    }

    #[test]
    fn play_time_errors_always_reach_a_watching_person() {
        let play = Mode::Play { interactive: true };
        // The v0.3.0 regression: Wine, stdout logged, no console.
        assert_eq!(route(play, t(false, true, true)), r(true, true));
        // Also from a terminal, and with nothing printable.
        assert_eq!(route(play, t(true, true, false)), r(true, true));
        assert_eq!(route(play, t(false, false, false)), r(false, true));
        // `--no-window` from a script: the log only.
        let unattended = Mode::Play { interactive: false };
        assert_eq!(route(unattended, t(false, true, true)), r(true, false));
    }

    /// Every combination of the four family flags, with and without Wine:
    /// `(command, no_window, settings_only, linux, wine) -> interactive`.
    #[test]
    fn the_audience_of_every_flag_combination() {
        #[rustfmt::skip]
        let table: [(bool, bool, bool, bool, bool, bool); 32] = [
            // command no_win settings linux  wine   interactive
            // Play runs: the window, then the game.
            (false, false, false, false, false, true),  // a double-click on Windows
            (false, false, false, false, true,  true),  // Proton, no arguments
            (false, false, false, true,  false, true),
            (false, false, false, true,  true,  true),  // the helper's window run, then play
            // --no-window: the game run.
            (false, true,  false, false, false, false), // a Windows script
            (false, true,  false, false, true,  true),  // <id>-play's game run (CMR3, CMR2005)
            (false, true,  false, true,  false, true),  // DiRT's `--no-window --linux`
            (false, true,  false, true,  true,  true),  // the regression: CMR3/CMR4/DiRT
            // --settings: only the window.
            (false, false, true,  false, false, true),  // a "Settings" shortcut
            (false, false, true,  false, true,  true),
            (false, false, true,  true,  false, false), // the helper's settings window
            (false, false, true,  true,  true,  false), // CMR2005's `--settings --linux`
            // --settings --no-window makes no window: a play-like run.
            (false, true,  true,  false, false, false),
            (false, true,  true,  false, true,  true),
            (false, true,  true,  true,  false, true),
            (false, true,  true,  true,  true,  true),
            // Commands: --help, --check…
            (true,  false, false, false, false, true),  // from a terminal or a shortcut
            (true,  false, false, false, true,  true),  // under Wine stdout is a log
            (true,  false, false, true,  false, false), // the helper reads the output
            (true,  false, false, true,  true,  false),
            (true,  true,  false, false, false, false), // `--check --no-window` script
            (true,  true,  false, false, true,  true),
            (true,  true,  false, true,  false, false),
            (true,  true,  false, true,  true,  false),
            (true,  false, true,  false, false, true),
            (true,  false, true,  false, true,  true),
            (true,  false, true,  true,  false, false),
            (true,  false, true,  true,  true,  false),
            (true,  true,  true,  false, false, false),
            (true,  true,  true,  false, true,  true),
            (true,  true,  true,  true,  false, false),
            (true,  true,  true,  true,  true,  false),
        ];
        for (command, no_window, settings_only, linux, wine, interactive) in table {
            let run = Run {
                command,
                no_window,
                settings_only,
                linux,
            };
            let audience = Audience::of(run, wine);
            assert_eq!(
                audience,
                Audience {
                    command,
                    interactive
                },
                "{run:?}, wine {wine}"
            );
            let expected = if command {
                Mode::Command { interactive }
            } else {
                Mode::Play { interactive }
            };
            assert_eq!(audience.mode(), expected);
        }
    }

    /// `<id>-play`, the AppImage and Proton: no console, stdout to a log.
    const UNDER_HELPER: Terminal = t(false, true, true);

    #[test]
    fn a_play_error_under_the_linux_helper_is_a_dialog() {
        for args in [
            &["--no-window"][..],
            &["--no-window", "--linux"],
            &["--no-window", "--scaler", "fsr"],
            &["--linux"],
            &[],
        ] {
            let mode = Audience::of(Run::from_args(args, &[]), true).mode();
            assert_eq!(route(mode, UNDER_HELPER), r(true, true), "{args:?}");
        }
        // Also a native Linux launcher started by its helper.
        let native = Audience::of(Run::from_args(&["--no-window", "--linux"], &[]), false);
        assert!(native.interactive);
    }

    #[test]
    fn the_helpers_settings_window_shows_its_own_messages() {
        // CMR2005's helper passes --linux only to the settings window.
        let window = Audience::of(Run::from_args(&["--settings", "--linux"], &[]), true);
        assert_eq!(route(window.mode(), UNDER_HELPER), r(true, false));
        // Its game run that follows is watched.
        let game = Audience::of(Run::from_args(&["--no-window"], &[]), true);
        assert_eq!(route(game.mode(), UNDER_HELPER), r(true, true));
    }

    #[test]
    fn a_bad_command_line_is_reported_like_a_command() {
        let mode = |args: &[&str], wine| Audience::of(Run::bad_command_line(args), wine).mode();
        let terminal = t(true, false, false);
        let shortcut = t(false, false, false);
        // From PowerShell: printed, no modal box to wait on.
        let typo = mode(&["--bogus-flag"], false);
        assert_eq!(route(typo, terminal), r(true, false));
        assert_eq!(
            route(mode(&["--look", "invalid"], false), terminal),
            r(true, false)
        );
        // A shortcut with a bad argument: nobody sees a terminal, so a box.
        assert_eq!(route(typo, shortcut), r(false, true));
        // A script (--no-window on Windows) and the Linux helper: never a box.
        assert_eq!(
            route(mode(&["--no-window", "--bogus"], false), shortcut),
            r(false, false)
        );
        let helper = t(false, true, true);
        assert_eq!(
            route(mode(&["--linux", "--bogus"], true), helper),
            r(true, false)
        );
        // A play run with the same flags would have shown a box from a terminal.
        let play = Audience::of(Run::from_args(&["--bogus-flag"], &[]), false).mode();
        assert_eq!(route(play, terminal), r(true, true));
        assert_eq!(BAD_COMMAND_LINE, 2);
    }

    #[test]
    fn commands_go_to_the_terminal() {
        for args in [&["--check"][..], &["--help"], &["-h"], &["/?"], &["--list"]] {
            assert!(Run::from_args(args, &["--list"]).command, "{args:?}");
        }
        assert!(!Run::from_args(&["--list"], &[]).command);
        let from_terminal = t(true, false, false);
        let check = Audience::of(Run::from_args(&["--check"], &[]), false);
        assert_eq!(route(check.mode(), from_terminal), r(true, false));
        // The helper's `--check --linux`: the log, never a dialog.
        let check = Audience::of(Run::from_args(&["--check", "--linux"], &[]), true);
        assert_eq!(route(check.mode(), UNDER_HELPER), r(true, false));
        // A Windows script: the terminal only.
        let check = Audience::of(Run::from_args(&["--check", "--no-window"], &[]), false);
        assert_eq!(route(check.mode(), t(false, true, false)), r(true, false));
    }

    #[test]
    fn a_windows_script_with_no_window_never_gets_a_dialog() {
        let script = Audience::of(
            Run::from_args(&["--no-window", "--game", r"C:\Games\CMR"], &[]),
            false,
        );
        assert!(!script.interactive);
        assert!(!route(script.mode(), t(false, true, false)).dialog);
        // A double-clicked shortcut is a person.
        assert!(Audience::of(Run::from_args::<&str>(&[], &[]), false).interactive);
    }

    #[test]
    fn who_sees_the_terminal() {
        assert!(t(true, false, true).seen());
        assert!(t(false, true, false).seen());
        assert!(!t(false, true, true).seen());
        assert!(!t(false, false, false).seen());
        assert!(t(false, true, true).printable());
    }

    /// Windows/Wine: `scripts/wine-test` pipes stdout to cargo and runs
    /// under Wine, the cmr*-play situation; no dialog is shown here.
    #[cfg(windows)]
    #[test]
    fn under_wine_test_play_errors_would_show_a_dialog() {
        let here = terminal();
        if here.wine {
            let game_run = audience(Run {
                no_window: true,
                ..Run::default()
            });
            assert!(route_here(game_run.mode()).dialog, "the helper's game run");
        }
        assert!(here.redirected, "the harness pipes stdout");
        let play = route_here(Mode::Play { interactive: true });
        assert!(play.print && play.dialog);
        let script = route_here(Mode::Command { interactive: false });
        assert!(!script.dialog);
        if here.wine && !here.attached {
            assert!(route_here(Mode::Command { interactive: true }).dialog);
        }
    }
}
