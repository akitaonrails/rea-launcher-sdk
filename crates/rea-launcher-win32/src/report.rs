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
//! ```no_run
//! # #[cfg(windows)] {
//! use rea_launcher_win32::report::{self, Mode};
//! let interactive = true; // false for --no-window from a script
//! report::fatal("OutRun 2006", "The game folder was not found.", Mode::Play { interactive });
//! # }
//! ```

/// What kind of message it is, and whether a person started the launcher
/// (`interactive`: false for `--no-window` or other runs from scripts).
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
