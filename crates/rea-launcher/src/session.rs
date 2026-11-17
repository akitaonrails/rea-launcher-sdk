//! Small helpers for one game run of a hub launcher: telling a start that
//! failed from a game the player quit, noticing a closed game window, and
//! reading what loaders print.
//!
//! From the TeknoParrot hub launchers' `session.rs`: [`early_end`] is
//! Initial D's (Nintendo and Sega said nothing when Windows refused to start
//! a program), [`WindowWatch`] and [`strip_ansi`] are Sega's (F355's board
//! window, Flycast's coloured log), [`default_interface`] is the Initial D
//! and Sega copy (lindbergh-loader's `NIC_NAME`).

use std::time::{Duration, Instant};

/// A game that closes by itself this soon after its start didn't start.
pub const EARLY_END: Duration = Duration::from_secs(20);

/// A run that ended by itself `after` its start, with `status`: when that
/// was too soon to have started, why (the end, then the last four error
/// lines of its session `log`, where the loaders write theirs).
pub fn early_end(after: Duration, status: &str, log: &str) -> Option<String> {
    if after >= EARLY_END {
        return None;
    }
    let errors: Vec<&str> = log
        .lines()
        .filter(|l| {
            let l = l.to_ascii_lowercase();
            l.starts_with("error") || l.contains("missing") || l.contains("failed")
        })
        .collect();
    let mut why = format!(
        "the game closed by itself {} s after starting ({status})",
        after.as_secs()
    );
    if !errors.is_empty() {
        why.push_str(":\n");
        why.push_str(&errors[errors.len().saturating_sub(4)..].join("\n"));
    }
    Some(why)
}

/// How often [`WindowWatch::look`] looks.
const LOOK_EVERY: Duration = Duration::from_millis(200);
/// How long an open window must stay missing to count as closed.
const GONE_AFTER: Duration = Duration::from_secs(1);

/// The game's window, looked at every 200 ms: once it has been open, it
/// counts as closed after a second without it (the player closed it), so a
/// window the game recreates doesn't end the game.
#[derive(Debug, Default)]
pub struct WindowWatch {
    seen: bool,
    last_look: Option<Instant>,
    missing_since: Option<Instant>,
    gone: bool,
}

impl WindowWatch {
    /// One look at `now` (at most every 200 ms; `shown` finds the window).
    /// Returns the window the first time it is seen, to give it the focus.
    pub fn look<W>(&mut self, now: Instant, shown: impl FnOnce() -> Option<W>) -> Option<W> {
        if self.gone || self.last_look.is_some_and(|t| now - t < LOOK_EVERY) {
            return None;
        }
        self.last_look = Some(now);
        match shown() {
            Some(w) => {
                self.missing_since = None;
                (!std::mem::replace(&mut self.seen, true)).then_some(w)
            }
            None => {
                if self.seen {
                    let since = *self.missing_since.get_or_insert(now);
                    self.gone = now - since >= GONE_AFTER;
                }
                None
            }
        }
    }

    /// The window was open and has now been missing for a second.
    pub fn gone(&self) -> bool {
        self.gone
    }
}

/// Text without its terminal escape sequences (colours: `ESC [ … m`).
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// How this machine reaches the network, from `/proc/net/route`'s text:
/// the default route's interface, else loopback (`lo`).
pub fn default_interface(proc_net_route: &str) -> String {
    proc_net_route
        .lines()
        .skip(1)
        .find_map(|l| {
            let mut f = l.split_whitespace();
            let (name, dest) = (f.next()?, f.next()?);
            (dest == "00000000" && name != "lo").then(|| name.to_string())
        })
        .unwrap_or_else(|| "lo".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_game_that_closes_at_once_did_not_start() {
        let log = "# arcade-launcher.exe run ...\n\
            winhost: xaudio2_7.dll missing from C:\\Windows\\SysWOW64 (xact): install ...\n\
            game: Initial D Arcade Stage 8 Infinity\n\
            error: starting the game: The application has failed to start\n";
        let why = early_end(Duration::from_secs(3), "exit code: 1", log).unwrap();
        assert!(why.starts_with("the game closed by itself 3 s after starting (exit code: 1):\n"));
        assert!(why.contains("xaudio2_7.dll missing"));
        assert!(why.ends_with("error: starting the game: The application has failed to start"));
        assert!(!why.contains("game: Initial D"));
        // nothing in the log: the end alone
        assert_eq!(
            early_end(Duration::from_secs(1), "exit code: 0", "").unwrap(),
            "the game closed by itself 1 s after starting (exit code: 0)"
        );
        // a game that ran: a normal end
        assert_eq!(early_end(EARLY_END, "exit code: 1", log), None);
    }

    #[test]
    fn only_the_last_four_errors_are_kept() {
        let log: String = (1..=6).map(|i| format!("error {i}\n")).collect();
        let why = early_end(Duration::ZERO, "x", &log).unwrap();
        assert!(
            why.ends_with(":\nerror 3\nerror 4\nerror 5\nerror 6"),
            "{why}"
        );
    }

    #[test]
    fn the_game_window_is_focused_once_and_closed_after_a_second_away() {
        let t0 = Instant::now();
        let ms = |n: u64| t0 + Duration::from_millis(n);
        let mut w = WindowWatch::default();
        // the game starts: no window yet, for as long as it takes
        assert_eq!(w.look(ms(0), || None::<u8>), None);
        assert_eq!(w.look(ms(5000), || None::<u8>), None);
        assert!(!w.gone());
        // it opens: focused once
        assert_eq!(w.look(ms(5300), || Some(7)), Some(7));
        assert_eq!(w.look(ms(5500), || Some(7)), None);
        // too soon to look again
        assert_eq!(
            w.look(ms(5600), || panic!("looked within 200 ms")),
            None::<u8>
        );
        // a blink (a window recreated) doesn't end the game
        w.look(ms(5800), || None::<u8>);
        w.look(ms(6200), || Some(7));
        w.look(ms(6900), || None::<u8>);
        assert!(!w.gone());
        // closed for a second: the game ends
        w.look(ms(7500), || None::<u8>);
        assert!(!w.gone());
        w.look(ms(7900), || None::<u8>);
        assert!(w.gone());
        assert_eq!(w.look(ms(9000), || panic!("no look once gone")), None::<u8>);
    }

    #[test]
    fn coloured_log_lines_lose_their_colours() {
        assert_eq!(
            strip_ansi("\x1b[93m00:50:954 sh4_opcodes.cpp:929 W[INTERPRETER]: TRAP #50\n\x1b[0m"),
            "00:50:954 sh4_opcodes.cpp:929 W[INTERPRETER]: TRAP #50\n"
        );
        assert_eq!(strip_ansi("plain"), "plain");
    }

    #[test]
    fn the_default_route_interface_is_picked() {
        let route = "Iface\tDestination\tGateway\n\
            lo\t00000000\t00000000\n\
            docker0\t0000FEA9\t00000000\n\
            eth0\t00000000\t0102A8C0\n";
        assert_eq!(default_interface(route), "eth0");
        assert_eq!(default_interface("Iface\tDestination\n"), "lo");
        assert_eq!(default_interface(""), "lo");
    }
}
