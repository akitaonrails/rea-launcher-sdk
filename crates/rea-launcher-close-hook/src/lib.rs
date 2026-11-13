//! A close hook for games that ignore close requests.
//!
//! Generalised from CMR4's `close-hook`: CMR04's window procedure swallows
//! `WM_CLOSE` and its loop ignores `WM_QUIT`, so neither Alt+F4 nor a
//! compositor close (which Wine turns into `SC_CLOSE`) ended it. The
//! launcher installs this DLL's `close_hook` export as a `WH_GETMESSAGE`
//! hook on the game's main thread
//! (`rea_launcher_win32::process::Running::message_hook`); Windows loads
//! the DLL into the game, and the hook turns a close request for the
//! game's window into the game's quit path.
//!
//! Configured per game without recompiling, from the `REA_CLOSE_HOOK`
//! environment variable (the launcher sets it on the game:
//! `Launch::env`), or else from `<dll name>.ini` next to the DLL
//! (`rea_close_hook.ini`). Both use the same `key = value` lines (`;`
//! separates them in the variable, `#` starts a comment in the file):
//!
//! ```text
//! class    = PC_CMR4              # the game window's class; empty: any window
//! close_on = sc_close, alt_f4     # also: wm_close, alt_f4_state (DirectInput)
//! strategy = terminate            # see Strategy
//! code     = 0                    # exit / quit code
//! ```
//!
//! The launcher can check its configuration with the same parser:
//! `rea_close_hook::Config::parse(text)` (this crate is also an rlib).
//!
//! The same DLL, loaded through the game's import table, can also answer
//! the game's `HKLM` key from `HKCU` ([`registry`], `REA_REGISTRY`), so a
//! launcher never depends on the VirtualStore.

use std::fmt;

/// The environment variable the hook reads first.
pub const ENV: &str = "REA_CLOSE_HOOK";
/// The export the launcher installs as the hook.
pub const EXPORT: &str = "close_hook";
/// The DLL's file name, next to the game's executable.
pub const DLL: &str = "rea_close_hook.dll";
/// The export a game's runtime copy imports so the loader maps the DLL at
/// start-up ([`import_into`]).
pub const ATTACH_EXPORT: &str = "rea_close_hook_attach";
/// `1` in the game's environment: the DLL, loaded through the import
/// table, installs the hook on the game's window thread itself.
pub const ATTACH_ENV: &str = "REA_CLOSE_HOOK_ATTACH";

/// The game executable `bytes` (the launcher's runtime copy, never the
/// player's file) with [`DLL`] in its import table, so Windows loads the
/// hook when the game starts. With [`ATTACH_ENV`]=1 in the game's
/// environment and the DLL next to the executable, the hook then installs
/// itself; the launcher never touches the running game. Antivirus
/// heuristics take a launcher that installs a hook into another process
/// for a keylogger; this way nothing crosses processes. Unchanged when the
/// import is already there (a runtime copy from an earlier start).
pub fn import_into(bytes: &[u8]) -> Result<Vec<u8>, rea_launcher::pe::edit::EditError> {
    use rea_launcher::pe::edit;
    if edit::imports(bytes)?
        .iter()
        .any(|import| import.dll.eq_ignore_ascii_case(DLL))
    {
        return Ok(bytes.to_vec());
    }
    edit::add_import(bytes, ".reahook", DLL, &[ATTACH_EXPORT], None)
}

/// Window messages and constants the pure parts need.
pub mod msg {
    pub const WM_CLOSE: u32 = 0x0010;
    pub const WM_SYSKEYDOWN: u32 = 0x0104;
    pub const WM_SYSCOMMAND: u32 = 0x0112;
    pub const SC_CLOSE: usize = 0xF060;
    pub const VK_F4: usize = 0x73;
    /// Alt.
    pub const VK_MENU: usize = 0x12;
    /// `WM_SYSKEYDOWN` lParam bit 29: Alt was held.
    pub const ALT_DOWN: isize = 1 << 29;
}

/// Which close requests the hook acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Requests {
    /// The window menu's / compositor's close (`WM_SYSCOMMAND SC_CLOSE`).
    pub sc_close: bool,
    /// Alt+F4 (`WM_SYSKEYDOWN VK_F4` with Alt).
    pub alt_f4: bool,
    /// A posted `WM_CLOSE`. Off by default: CMR4 closes its intro-movie
    /// window with `WM_CLOSE` on the same thread.
    pub wm_close: bool,
    /// Alt+F4 read from the keyboard state (`GetAsyncKeyState`) whenever
    /// the hook sees a message while the game's window is in front, for
    /// games that take the keyboard through DirectInput in exclusive
    /// mode: their window never gets the `WM_SYSKEYDOWN` (CMR2005). Off by
    /// default.
    pub alt_f4_state: bool,
}

impl Default for Requests {
    fn default() -> Requests {
        Requests {
            sc_close: true,
            alt_f4: true,
            wm_close: false,
            alt_f4_state: false,
        }
    }
}

/// Alt+F4 held down, seen as one press: [`Chord::press`] is true once
/// when both keys go down together, and again only after one was let go.
/// A held chord (the hook sees many messages while the player holds the
/// keys) closes the game once, not on every message.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Chord {
    held: bool,
}

impl Chord {
    /// Feeds the keys' state (`alt`, `f4`: held down now); true for a new
    /// press of the chord.
    pub fn press(&mut self, alt: bool, f4: bool) -> bool {
        let both = alt && f4;
        let new = both && !self.held;
        self.held = both;
        new
    }
}

impl Requests {
    /// True for the requests this configuration acts on.
    pub fn matches(&self, message: u32, wparam: usize, lparam: isize) -> bool {
        match message {
            msg::WM_SYSCOMMAND => self.sc_close && wparam & 0xFFF0 == msg::SC_CLOSE,
            msg::WM_SYSKEYDOWN => {
                self.alt_f4 && wparam == msg::VK_F4 && lparam & msg::ALT_DOWN != 0
            }
            msg::WM_CLOSE => self.wm_close,
            _ => false,
        }
    }
}

/// One step of a keystroke sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Press and release a virtual key.
    Key(u16),
    /// Wait this many milliseconds (the game's menu catching up).
    Pause(u32),
}

/// How the hook ends the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Strategy {
    /// `TerminateProcess` at once (CMR4: the game has no shutdown path).
    Terminate,
    /// `ExitProcess`: the C runtime's exit handlers and every DLL's
    /// detach run first, so what the game flushes there is saved.
    Exit,
    /// `PostQuitMessage`: the game's message loop ends as if it quit.
    Quit,
    /// `WM_QUIT` posted to the game's window.
    PostQuit,
    /// Keystrokes through `SendInput` (reaches DirectInput games, which
    /// read the keyboard, not messages), e.g. the game's Quit menu:
    /// `keys: escape, 300, down, return`.
    Keys(Vec<Step>),
    /// Keystrokes posted to the game's window as `WM_KEYDOWN`/`WM_KEYUP`
    /// (games that read their message queue).
    PostKeys(Vec<Step>),
}

/// The hook's configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// The game window's class (case-insensitive); `None`: any top-level
    /// window of the game.
    pub class: Option<String>,
    pub requests: Requests,
    pub strategy: Strategy,
    /// The exit code (`Terminate`, `Exit`) or quit code (`Quit`,
    /// `PostQuit`).
    pub code: u32,
}

impl Default for Config {
    /// CMR4's behaviour: any window, SC_CLOSE and Alt+F4, terminate.
    fn default() -> Config {
        Config {
            class: None,
            requests: Requests::default(),
            strategy: Strategy::Terminate,
            code: 0,
        }
    }
}

/// A configuration line the parser refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "close hook configuration: {}", self.0)
    }
}

impl std::error::Error for ConfigError {}

/// The virtual-key code for a key name: `escape`, `return`/`enter`,
/// `space`, `tab`, `up`/`down`/`left`/`right`, `f1`–`f12`, a letter or
/// digit, or `0xNN`.
pub fn virtual_key(name: &str) -> Option<u16> {
    let name = name.trim().to_ascii_lowercase();
    let named = match name.as_str() {
        "escape" | "esc" => Some(0x1B),
        "return" | "enter" => Some(0x0D),
        "space" => Some(0x20),
        "tab" => Some(0x09),
        "backspace" => Some(0x08),
        "left" => Some(0x25),
        "up" => Some(0x26),
        "right" => Some(0x27),
        "down" => Some(0x28),
        _ => None,
    };
    if named.is_some() {
        return named;
    }
    if let Some(hex) = name.strip_prefix("0x") {
        return u16::from_str_radix(hex, 16)
            .ok()
            .filter(|&vk| vk > 0 && vk < 0xFF);
    }
    if let Some(n) = name.strip_prefix('f').and_then(|n| n.parse::<u16>().ok())
        && (1..=12).contains(&n)
    {
        return Some(0x6F + n);
    }
    match name.as_bytes() {
        [c] if c.is_ascii_alphanumeric() => Some(u16::from(c.to_ascii_uppercase())),
        _ => None,
    }
}

/// `escape, 300, down, return`: key names and pauses in milliseconds.
pub fn parse_steps(text: &str) -> Result<Vec<Step>, ConfigError> {
    let steps = text
        .split(',')
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| match token.parse::<u32>() {
            Ok(ms) => Ok(Step::Pause(ms)),
            Err(_) => virtual_key(token)
                .map(Step::Key)
                .ok_or_else(|| ConfigError(format!("unknown key {token:?}"))),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if steps.iter().any(|s| matches!(s, Step::Key(_))) {
        Ok(steps)
    } else {
        Err(ConfigError("a key sequence needs at least one key".into()))
    }
}

impl Config {
    /// Parses `key = value` lines (or `;`-separated pairs). Unset keys keep
    /// the defaults; unknown keys and values are errors.
    pub fn parse(text: &str) -> Result<Config, ConfigError> {
        let mut config = Config::default();
        for item in text.split(['\n', ';']) {
            let item = item.split('#').next().unwrap_or("").trim();
            if item.is_empty() || item.starts_with('[') {
                continue;
            }
            let (key, value) = item
                .split_once('=')
                .ok_or_else(|| ConfigError(format!("not key = value: {item:?}")))?;
            let (key, value) = (key.trim().to_ascii_lowercase(), value.trim());
            match key.as_str() {
                "class" => config.class = (!value.is_empty()).then(|| value.to_owned()),
                "close_on" => {
                    let mut requests = Requests {
                        sc_close: false,
                        alt_f4: false,
                        wm_close: false,
                        alt_f4_state: false,
                    };
                    for name in value.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                        match name.to_ascii_lowercase().as_str() {
                            "sc_close" => requests.sc_close = true,
                            "alt_f4" => requests.alt_f4 = true,
                            "wm_close" => requests.wm_close = true,
                            "alt_f4_state" => requests.alt_f4_state = true,
                            other => {
                                return Err(ConfigError(format!("unknown request {other:?}")));
                            }
                        }
                    }
                    config.requests = requests;
                }
                "strategy" => config.strategy = parse_strategy(value)?,
                "code" => {
                    config.code = value
                        .parse()
                        .map_err(|_| ConfigError(format!("code is not a number: {value:?}")))?;
                }
                other => return Err(ConfigError(format!("unknown key {other:?}"))),
            }
        }
        Ok(config)
    }

    /// Whether a window of class `class` (its root's) is the game's.
    pub fn is_game_class(&self, class: &str) -> bool {
        self.class
            .as_deref()
            .is_none_or(|wanted| wanted.eq_ignore_ascii_case(class))
    }
}

fn parse_strategy(value: &str) -> Result<Strategy, ConfigError> {
    let (name, rest) = value.split_once(':').unwrap_or((value, ""));
    match name.trim().to_ascii_lowercase().as_str() {
        "terminate" => Ok(Strategy::Terminate),
        "exit" => Ok(Strategy::Exit),
        "quit" => Ok(Strategy::Quit),
        "post_quit" => Ok(Strategy::PostQuit),
        "keys" => parse_steps(rest).map(Strategy::Keys),
        "post_keys" => parse_steps(rest).map(Strategy::PostKeys),
        other => Err(ConfigError(format!("unknown strategy {other:?}"))),
    }
}

pub mod registry;

#[cfg(all(windows, feature = "dll"))]
mod hook;
#[cfg(all(windows, feature = "dll"))]
mod registry_hook;

#[cfg(test)]
mod tests {
    use super::msg::*;
    use super::*;

    #[test]
    fn close_requests_are_recognized() {
        let r = Requests::default();
        assert!(r.matches(WM_SYSCOMMAND, SC_CLOSE, 0));
        // Low four bits of SC_* are reserved and may be set.
        assert!(r.matches(WM_SYSCOMMAND, SC_CLOSE | 0x2, 0));
        assert!(r.matches(WM_SYSKEYDOWN, VK_F4, ALT_DOWN | 1));
        // Not by default: CMR4's movie window is closed with WM_CLOSE.
        assert!(!r.matches(WM_CLOSE, 0, 0));
        assert!(
            Requests {
                wm_close: true,
                ..r
            }
            .matches(WM_CLOSE, 0, 0)
        );
    }

    #[test]
    fn other_input_is_left_alone() {
        let r = Requests::default();
        assert!(!r.matches(WM_SYSKEYDOWN, VK_F4, 1)); // F4 without Alt
        assert!(!r.matches(WM_SYSKEYDOWN, 0x09, ALT_DOWN)); // Alt+Tab
        assert!(!r.matches(WM_SYSCOMMAND, 0xF140, 0)); // SC_SCREENSAVE
        assert!(!r.matches(0x0100, VK_F4, 0)); // WM_KEYDOWN F4
        let only_alt = Requests {
            sc_close: false,
            ..r
        };
        assert!(!only_alt.matches(WM_SYSCOMMAND, SC_CLOSE, 0));
    }

    #[test]
    fn the_default_is_cmr4s_hook() {
        assert_eq!(Config::parse("").unwrap(), Config::default());
        assert_eq!(
            Config::parse("class = PC_CMR4").unwrap(),
            Config {
                class: Some("PC_CMR4".into()),
                ..Config::default()
            }
        );
    }

    #[test]
    fn file_and_variable_forms_parse_alike() {
        let file = "[close_hook]\n# OutRun-style\nclass = Game\nclose_on = alt_f4, wm_close\nstrategy = keys: escape, 300, down, return  # the Quit menu\ncode = 3\n";
        let env = "class=Game;close_on=alt_f4,wm_close;strategy=keys:escape,300,down,return;code=3";
        let expected = Config {
            class: Some("Game".into()),
            requests: Requests {
                sc_close: false,
                alt_f4: true,
                wm_close: true,
                alt_f4_state: false,
            },
            strategy: Strategy::Keys(vec![
                Step::Key(0x1B),
                Step::Pause(300),
                Step::Key(0x28),
                Step::Key(0x0D),
            ]),
            code: 3,
        };
        assert_eq!(Config::parse(file).unwrap(), expected);
        assert_eq!(Config::parse(env).unwrap(), expected);
        for (text, strategy) in [
            ("strategy = exit", Strategy::Exit),
            ("strategy = QUIT", Strategy::Quit),
            ("strategy = post_quit", Strategy::PostQuit),
            (
                "strategy = post_keys: q",
                Strategy::PostKeys(vec![Step::Key(0x51)]),
            ),
        ] {
            assert_eq!(Config::parse(text).unwrap().strategy, strategy, "{text}");
        }
    }

    #[test]
    fn alt_f4_state_is_opt_in() {
        // Existing configurations read as before.
        for text in [
            "",
            "close_on = sc_close, alt_f4",
            "class = PC_CMR4;strategy=terminate",
        ] {
            assert!(
                !Config::parse(text).unwrap().requests.alt_f4_state,
                "{text}"
            );
        }
        let config = Config::parse("class=PC_CMR5;close_on=sc_close,alt_f4_state").unwrap();
        assert_eq!(
            config.requests,
            Requests {
                sc_close: true,
                alt_f4: false,
                wm_close: false,
                alt_f4_state: true,
            }
        );
        let file = "close_on = ALT_F4_STATE  # DirectInput keyboard\n";
        assert!(Config::parse(file).unwrap().requests.alt_f4_state);
        // The message matcher never fires for it: the state is read
        // separately.
        let state_only = Config::parse("close_on = alt_f4_state").unwrap().requests;
        assert!(!state_only.matches(WM_SYSKEYDOWN, VK_F4, ALT_DOWN));
        assert!(!state_only.matches(WM_SYSCOMMAND, SC_CLOSE, 0));
    }

    #[test]
    fn a_held_chord_is_one_press() {
        let mut chord = Chord::default();
        assert!(!chord.press(false, false));
        assert!(!chord.press(true, false)); // Alt alone
        assert!(chord.press(true, true)); // + F4
        // Held across many messages: once.
        assert!(!chord.press(true, true));
        assert!(!chord.press(true, true));
        // F4 let go and pressed again with Alt still down: again.
        assert!(!chord.press(true, false));
        assert!(chord.press(true, true));
        // F4 alone never.
        let mut chord = Chord::default();
        assert!(!chord.press(false, true));
        assert!(!chord.press(false, true));
    }

    #[test]
    fn mistakes_are_reported() {
        for text in [
            "strategy = explode",
            "strategy = keys: 300",
            "strategy = keys: hyperspace",
            "close_on = double_click",
            "close_on = alt_f4_status",
            "code = soon",
            "colour = red",
            "class",
        ] {
            assert!(Config::parse(text).is_err(), "{text}");
        }
        let error = Config::parse("strategy = keys: hyperspace").unwrap_err();
        assert_eq!(
            error.to_string(),
            "close hook configuration: unknown key \"hyperspace\""
        );
    }

    #[test]
    fn key_names() {
        assert_eq!(virtual_key("Escape"), Some(0x1B));
        assert_eq!(virtual_key("enter"), Some(0x0D));
        assert_eq!(virtual_key("f1"), Some(0x70));
        assert_eq!(virtual_key("F12"), Some(0x7B));
        assert_eq!(virtual_key("y"), Some(0x59));
        assert_eq!(virtual_key("7"), Some(0x37));
        assert_eq!(virtual_key("0x51"), Some(0x51));
        assert_eq!(virtual_key("f13"), None);
        assert_eq!(virtual_key("0x00"), None);
        assert_eq!(virtual_key("enterprise"), None);
    }

    #[test]
    fn window_classes_compare_without_case() {
        let config = Config::parse("class = PC_CMR4").unwrap();
        assert!(config.is_game_class("pc_cmr4"));
        assert!(!config.is_game_class("PC_CMR4_MOVIE"));
        assert!(Config::default().is_game_class("anything"));
    }
}
