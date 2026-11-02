//! `launcher.cfg` documents: plain `key = value` lines with `#` comments,
//! and the command-line flags that share their keys.
//!
//! CMR3's `config.rs` pattern (the audit's pick): one `apply(key, value)`
//! serves the file and `--some-key value` flags alike; the launcher edits
//! only the lines it owns ([`set_key`], [`clear_key`]), keeping comments
//! and keys it does not know (CMR4 3d9780b: rewriting the file wholesale
//! dropped them). CMR4's line editing (every active line, CRLF kept) and
//! [`read_file`] (CMR4 16edc74: a Notepad ANSI file stopped every launch;
//! only NotFound is a first run, so an unreadable file is never replaced).
//! A toolkit, not a derive: the launcher keeps its own `Settings`.

use std::io;
use std::ops::RangeInclusive;
use std::path::Path;

/// Reads a settings file: `None` when it does not exist (a first run).
/// See [`decode`]. Any other error is returned, so the caller never
/// overwrites a file it could not read.
pub fn read_file(path: &Path) -> io::Result<Option<String>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(decode(bytes))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Text from a settings file's bytes: UTF-8 (a leading BOM dropped), else
/// Latin-1, which keeps the accented letters of Western code pages
/// instead of turning a folder name into U+FFFD. Saved back as UTF-8.
pub fn decode(bytes: Vec<u8>) -> String {
    match String::from_utf8(bytes) {
        Ok(text) => match text.strip_prefix('\u{feff}') {
            Some(rest) => rest.to_owned(),
            None => text,
        },
        Err(e) => e.into_bytes().iter().map(|&b| char::from(b)).collect(),
    }
}

/// One active setting line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry<'a> {
    pub key: &'a str,
    pub value: &'a str,
    /// 1-based line number, for messages.
    pub line: usize,
}

/// Splits an active `key = value` line (both trimmed); comments (`#`,
/// `;`) and lines without `=` are `None`.
fn split_line(line: &str) -> Option<(&str, &str)> {
    let t = line.trim();
    if t.starts_with(['#', ';']) {
        return None;
    }
    t.split_once('=').map(|(k, v)| (k.trim(), v.trim()))
}

/// The active settings in `text`, in order. Blank values (`game =`) mean
/// "unset" and are skipped.
pub fn entries(text: &str) -> impl Iterator<Item = Entry<'_>> {
    text.lines().enumerate().filter_map(|(n, line)| {
        let (key, value) = split_line(line)?;
        (!value.is_empty() && !key.is_empty()).then_some(Entry {
            key,
            value,
            line: n + 1,
        })
    })
}

/// Why `apply` did not take a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejected {
    /// Not a key this launcher knows.
    Unknown,
    /// What the key takes ("on or off"), for the message.
    Needs(String),
}

impl Rejected {
    pub fn needs(what: impl Into<String>) -> Rejected {
        Rejected::Needs(what.into())
    }
}

/// `Some(v)`, or [`Rejected::Needs`] with `what`.
pub fn need<T>(value: Option<T>, what: &str) -> Result<T, Rejected> {
    value.ok_or_else(|| Rejected::needs(what))
}

/// Feeds every entry of `text` to `apply`. Returns a warning per line it
/// could not use (`file_name line n: …`); unknown keys are ignored but
/// reported, and stay in the file.
pub fn parse(
    text: &str,
    file_name: &str,
    mut apply: impl FnMut(&str, &str) -> Result<(), Rejected>,
) -> Vec<String> {
    let mut warnings = Vec::new();
    for e in entries(text) {
        match apply(e.key, e.value) {
            Ok(()) => {}
            Err(Rejected::Unknown) => warnings.push(format!(
                "{file_name} line {}: unknown setting {:?} (ignored, kept in the file)",
                e.line, e.key
            )),
            Err(Rejected::Needs(what)) => warnings.push(format!(
                "{file_name} line {}: {} needs {what}, not {:?}",
                e.line, e.key, e.value
            )),
        }
    }
    warnings
}

/// The newline `text` uses: CRLF when it has any (Notepad), else LF.
fn newline(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

fn is_key_line(line: &str, key: &str) -> bool {
    split_line(line).is_some_and(|(k, _)| k == key)
}

/// A commented-out placeholder for `key`: `#key =` or `#key = example`
/// (what templates and [`clear_key`] write), not prose.
fn is_placeholder(line: &str, key: &str) -> bool {
    line.trim()
        .strip_prefix('#')
        .and_then(|rest| rest.strip_prefix(key))
        .is_some_and(|rest| rest.trim_start().starts_with('='))
}

/// Sets `key` in a settings file, keeping every other line as it was:
/// replaces each active `key =` line; else the first `#key =` placeholder
/// becomes the active line (so it stays under its comment); else appends.
pub fn set_key(text: &str, key: &str, value: &str) -> String {
    let nl = newline(text);
    let entry = format!("{key} = {value}");
    let mut lines: Vec<&str> = text.lines().collect();
    let active = lines.iter().any(|l| is_key_line(l, key));
    let placeholder = (!active)
        .then(|| lines.iter().position(|l| is_placeholder(l, key)))
        .flatten();
    let mut out = String::with_capacity(text.len() + entry.len() + 2);
    if !active && placeholder.is_none() {
        lines.push("");
    }
    let last = lines.len() - 1;
    for (n, line) in lines.iter().enumerate() {
        let replace = is_key_line(line, key)
            || Some(n) == placeholder
            || (!active && placeholder.is_none() && n == last);
        out.push_str(if replace { &entry } else { line });
        out.push_str(nl);
    }
    out
}

/// Unsets `key`: each active line becomes `#key =`, so the default
/// applies; other lines are kept.
pub fn clear_key(text: &str, key: &str) -> String {
    let nl = newline(text);
    let mut out = String::with_capacity(text.len() + 2);
    for line in text.lines() {
        if is_key_line(line, key) {
            out.push_str(&format!("#{key} ="));
        } else {
            out.push_str(line);
        }
        out.push_str(nl);
    }
    out
}

/// The file key of a `--flag-name`: `flag_name`; `None` for anything that
/// is not a long flag.
pub fn flag_key(flag: &str) -> Option<String> {
    let name = flag.strip_prefix("--").filter(|n| !n.is_empty())?;
    Some(name.replace('-', "_"))
}

/// The `--flag-name` of a file key.
pub fn key_flag(key: &str) -> String {
    format!("--{}", key.replace('_', "-"))
}

/// Why the command line was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgError {
    /// `--help`, `-h` or `/?`: print the usage.
    Help,
    Unknown(String),
    MissingValue(String),
    Bad {
        flag: String,
        value: String,
        needs: String,
    },
}

impl std::fmt::Display for ArgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ArgError::Help => f.write_str("usage requested"),
            ArgError::Unknown(flag) => write!(f, "unknown argument {flag}"),
            ArgError::MissingValue(flag) => write!(f, "{flag} needs a value"),
            ArgError::Bad { flag, value, needs } => {
                write!(f, "{flag} needs {needs}, not {value}")
            }
        }
    }
}

impl std::error::Error for ArgError {}

/// Applies `--flag value` pairs through the same `apply` as the file, in
/// one pass. `keys` lists the settings keys, so a known flag without its
/// value is told apart from an unknown one; `switches` lists the
/// launcher's own flags that take no value (`--no-window`, `--check`),
/// returned in order, so a value is never mistaken for a switch.
pub fn parse_args<I: IntoIterator<Item = String>>(
    args: I,
    keys: &[&str],
    switches: &[&str],
    mut apply: impl FnMut(&str, &str) -> Result<(), Rejected>,
) -> Result<Vec<String>, ArgError> {
    let mut seen = Vec::new();
    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        if matches!(flag.as_str(), "--help" | "-h" | "/?") {
            return Err(ArgError::Help);
        }
        if switches.contains(&flag.as_str()) {
            seen.push(flag);
            continue;
        }
        let Some(key) = flag_key(&flag).filter(|k| keys.contains(&k.as_str())) else {
            return Err(ArgError::Unknown(flag));
        };
        let Some(value) = args.next() else {
            return Err(ArgError::MissingValue(flag));
        };
        match apply(&key, &value) {
            Ok(()) => {}
            Err(Rejected::Unknown) => return Err(ArgError::Unknown(flag)),
            Err(Rejected::Needs(needs)) => return Err(ArgError::Bad { flag, value, needs }),
        }
    }
    Ok(seen)
}

/// What [`parse_bool`] takes, for messages.
pub const ON_OFF: &str = "on or off";

/// `on/true/yes/1` or `off/false/no/0`, any case.
pub fn parse_bool(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "on" | "true" | "yes" | "1" => Some(true),
        "off" | "false" | "no" | "0" => Some(false),
        _ => None,
    }
}

pub fn on_off(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

/// A display resolution setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    /// The size the game shipped with (`original`).
    Original,
    /// The whole screen at its own size (SCR's `fullscreen`).
    Fullscreen,
    Size(u32, u32),
}

impl std::fmt::Display for Resolution {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Resolution::Original => f.write_str("original"),
            Resolution::Fullscreen => f.write_str("fullscreen"),
            Resolution::Size(w, h) => write!(f, "{w}x{h}"),
        }
    }
}

/// Words a `resolution` value may be instead of a size, per launcher (see
/// the README's migration notes).
pub type Aliases = [(&'static str, Resolution)];

/// No words: sizes only (SRR).
pub const SIZES_ONLY: &Aliases = &[];
/// `original` (CMR3, CMR4).
pub const ORIGINAL: &Aliases = &[("original", Resolution::Original)];
/// `original` or `off` (OR).
pub const ORIGINAL_OR_OFF: &Aliases = &[
    ("original", Resolution::Original),
    ("off", Resolution::Original),
];
/// `original`/`off`, and `fullscreen`/`full`/`desktop` (SCR).
pub const ORIGINAL_OR_FULLSCREEN: &Aliases = &[
    ("original", Resolution::Original),
    ("off", Resolution::Original),
    ("fullscreen", Resolution::Fullscreen),
    ("full", Resolution::Fullscreen),
    ("desktop", Resolution::Fullscreen),
];

/// `WIDTHxHEIGHT` (`x`, `X` or `×`) within `min..=max` on both sides.
pub fn parse_size(value: &str, min: (u32, u32), max: (u32, u32)) -> Option<(u32, u32)> {
    let value = value.trim().to_ascii_lowercase();
    let (w, h) = value.split_once(['x', '×'])?;
    let w: u32 = w.trim().parse().ok()?;
    let h: u32 = h.trim().parse().ok()?;
    ((min.0..=max.0).contains(&w) && (min.1..=max.1).contains(&h)).then_some((w, h))
}

/// [`parse_size`], or one of the `aliases` words (any case).
pub fn parse_resolution(
    value: &str,
    min: (u32, u32),
    max: (u32, u32),
    aliases: &Aliases,
) -> Option<Resolution> {
    parse_enum(value, aliases)
        .or_else(|| parse_size(value, min, max).map(|(w, h)| Resolution::Size(w, h)))
}

/// The value whose name matches, any case.
pub fn parse_enum<T: Copy>(value: &str, table: &[(&str, T)]) -> Option<T> {
    let value = value.trim();
    table
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(value))
        .map(|&(_, v)| v)
}

/// The names of a table for messages: `a, b or c`.
pub fn one_of<T>(table: &[(&str, T)]) -> String {
    let names: Vec<&str> = table.iter().map(|(n, _)| *n).collect();
    match names.split_last() {
        None => String::new(),
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
    }
}

/// A whole number inside `range`.
pub fn parse_range(value: &str, range: RangeInclusive<u32>) -> Option<u32> {
    value.trim().parse().ok().filter(|n| range.contains(n))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_dir;

    const TEMPLATE: &str = "# Launcher settings.\n\n# The game folder.\ngame =\n\n# Private copy.\n#runtime =\n\n# Resolution, or original.\n#resolution = 3840x2160\n";

    #[derive(Debug, Default, PartialEq)]
    struct Settings {
        game: Option<String>,
        resolution: Option<Resolution>,
        vsync: Option<bool>,
        mode: Option<u8>,
        fov: Option<u32>,
    }

    const MODES: [(&str, u8); 3] = [("fullscreen", 0), ("windowed", 1), ("borderless", 2)];
    const KEYS: [&str; 5] = ["game", "resolution", "vsync", "display_mode", "fov"];

    impl Settings {
        fn apply(&mut self, key: &str, value: &str) -> Result<(), Rejected> {
            match key {
                "game" => self.game = Some(value.into()),
                "resolution" => {
                    let r = parse_resolution(value, (640, 480), (7680, 4320), ORIGINAL);
                    self.resolution = Some(need(r, "WIDTHxHEIGHT or original")?);
                }
                "vsync" => self.vsync = Some(need(parse_bool(value), ON_OFF)?),
                "display_mode" => {
                    self.mode = Some(need(parse_enum(value, &MODES), &one_of(&MODES))?)
                }
                "fov" => self.fov = Some(need(parse_range(value, 30..=150), "30 to 150")?),
                _ => return Err(Rejected::Unknown),
            }
            Ok(())
        }
    }

    fn file(text: &str) -> (Settings, Vec<String>) {
        let mut s = Settings::default();
        let warnings = parse(text, "launcher.cfg", |k, v| s.apply(k, v));
        (s, warnings)
    }

    fn args(a: &[&str]) -> Result<Settings, ArgError> {
        let mut s = Settings::default();
        parse_args(a.iter().map(|s| s.to_string()), &KEYS, &[], |k, v| {
            s.apply(k, v)
        })?;
        Ok(s)
    }

    #[test]
    fn parses_settings_and_reports_bad_lines() {
        let (s, warnings) = file(
            "game = C:\\G\nresolution = 3840x2160\nvsync = off\n; note\ndisplay_mode = Borderless\nfov = 90\n",
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(s.game.as_deref(), Some("C:\\G"));
        assert_eq!(s.resolution, Some(Resolution::Size(3840, 2160)));
        assert_eq!((s.vsync, s.mode, s.fov), (Some(false), Some(2), Some(90)));
        let (_, warnings) =
            file("resolution = 8000x100\nfov = 200\ndisplay_mode = wide\nfuture = 1\n");
        assert_eq!(warnings.len(), 4, "{warnings:?}");
        assert_eq!(
            warnings[2],
            "launcher.cfg line 3: display_mode needs fullscreen, windowed or borderless, not \"wide\""
        );
        assert!(warnings[3].contains("unknown setting \"future\""));
    }

    #[test]
    fn template_parses_clean() {
        let (s, warnings) = file(TEMPLATE);
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(s, Settings::default(), "blank and commented keys are unset");
    }

    #[test]
    fn flags_share_the_file_keys() {
        let s = args(&[
            "--game",
            "D:\\G",
            "--display-mode",
            "windowed",
            "--vsync",
            "on",
        ])
        .unwrap();
        assert_eq!(s.game.as_deref(), Some("D:\\G"));
        assert_eq!((s.mode, s.vsync), (Some(1), Some(true)));
        assert_eq!(
            args(&["--vsync", "maybe"]).unwrap_err().to_string(),
            "--vsync needs on or off, not maybe"
        );
        assert_eq!(
            args(&["--fov"]).unwrap_err(),
            ArgError::MissingValue("--fov".into())
        );
        assert_eq!(
            args(&["--bogus"]).unwrap_err(),
            ArgError::Unknown("--bogus".into())
        );
        assert_eq!(
            args(&["game"]).unwrap_err(),
            ArgError::Unknown("game".into())
        );
        assert_eq!(args(&["--help"]).unwrap_err(), ArgError::Help);
        assert_eq!(flag_key("--hd-ui").as_deref(), Some("hd_ui"));
        assert_eq!(flag_key("-x"), None);
        assert_eq!(flag_key("--"), None);
        assert_eq!(key_flag("gamepad_log"), "--gamepad-log");
        for key in KEYS {
            assert_eq!(flag_key(&key_flag(key)).as_deref(), Some(key));
        }
    }

    #[test]
    fn launcher_switches_are_taken_in_the_same_pass() {
        let mut s = Settings::default();
        let argv = ["--no-window", "--game", "--check", "--settings", "--linux"];
        let seen = parse_args(
            argv.iter().map(|a| a.to_string()),
            &KEYS,
            &["--no-window", "--check", "--settings", "--linux"],
            |k, v| s.apply(k, v),
        )
        .unwrap();
        // `--check` after `--game` is the game folder's value, not a switch.
        assert_eq!(seen, ["--no-window", "--settings", "--linux"]);
        assert_eq!(s.game.as_deref(), Some("--check"));
    }

    #[test]
    fn set_key_keeps_the_rest_of_the_file() {
        let filled = set_key(TEMPLATE, "game", r"D:\CMR3");
        assert!(filled.contains("\n# The game folder.\ngame = D:\\CMR3\n"));
        assert!(filled.contains("#runtime =\n"));
        let again = set_key(&filled, "game", r"E:\CMR3");
        assert_eq!(again.matches("game =").count(), 1);
        let cleared = clear_key(&filled, "game");
        assert!(cleared.contains("#game ="));
        assert!(!cleared.contains("\ngame ="));
        // A placeholder becomes the line, under its comment.
        let set = set_key(TEMPLATE, "resolution", "1920x1080");
        assert!(set.contains("# Resolution, or original.\nresolution = 1920x1080\n"));
        assert!(!set.contains("#resolution"));
        // Set, clear, set: in place every time.
        let round = set_key(&clear_key(&set, "resolution"), "resolution", "original");
        assert_eq!(round, set.replace("1920x1080", "original"));
        // Unknown keys are appended; prose that mentions a key is not a
        // placeholder.
        let added = set_key("# game is where it lives\n", "game", "X");
        assert_eq!(added, "# game is where it lives\ngame = X\n");
        assert_eq!(set_key("", "game", "X"), "game = X\n");
    }

    #[test]
    fn duplicates_and_crlf() {
        // Every active line: the last one would win when parsed.
        let text = "vsync = on\r\nother = 1\r\nvsync = off\r\n";
        assert_eq!(
            set_key(text, "vsync", "off"),
            "vsync = off\r\nother = 1\r\nvsync = off\r\n"
        );
        assert_eq!(
            clear_key(text, "vsync"),
            "#vsync =\r\nother = 1\r\n#vsync =\r\n"
        );
        assert_eq!(set_key("a = 1", "b", "2"), "a = 1\nb = 2\n");
    }

    #[test]
    fn value_parsers() {
        assert_eq!(parse_bool(" YES "), Some(true));
        assert_eq!(parse_bool("0"), Some(false));
        assert_eq!(parse_bool("maybe"), None);
        assert_eq!(on_off(true), "on");
        let r = |v| parse_resolution(v, (640, 480), (7680, 4320), ORIGINAL);
        assert_eq!(r("ORIGINAL"), Some(Resolution::Original));
        assert_eq!(r("2560 x 1440"), Some(Resolution::Size(2560, 1440)));
        assert_eq!(r("1920×1080"), Some(Resolution::Size(1920, 1080)));
        assert_eq!(r("320x240"), None);
        assert_eq!(r("wide"), None);
        assert_eq!(Resolution::Size(1280, 800).to_string(), "1280x800");
        // Every launcher's spellings are expressible.
        let scr = |v| parse_resolution(v, (320, 200), (7680, 4320), ORIGINAL_OR_FULLSCREEN);
        assert_eq!(scr("Off"), Some(Resolution::Original));
        assert_eq!(scr("desktop"), Some(Resolution::Fullscreen));
        assert_eq!(scr("320x200"), Some(Resolution::Size(320, 200)));
        let or = |v| parse_resolution(v, (640, 480), (7680, 4320), ORIGINAL_OR_OFF);
        assert_eq!(or("off"), Some(Resolution::Original));
        assert_eq!(or("full"), None);
        let srr = |v| parse_resolution(v, (640, 480), (7680, 4320), SIZES_ONLY);
        assert_eq!(srr("original"), None);
        assert_eq!(Resolution::Fullscreen.to_string(), "fullscreen");
        assert_eq!(parse_enum("WINDOWED", &MODES), Some(1));
        assert_eq!(one_of(&MODES), "fullscreen, windowed or borderless");
        assert_eq!(one_of(&MODES[..1]), "fullscreen");
        assert_eq!(parse_range("150", 30..=150), Some(150));
        assert_eq!(parse_range("29", 30..=150), None);
    }

    #[test]
    fn settings_files_in_utf8_or_ansi_read_and_only_missing_is_a_first_run() {
        let dir = test_dir("cfg-read");
        let path = dir.join("launcher.cfg");
        assert!(read_file(&path).unwrap().is_none());
        // Notepad ANSI (Windows-1252): "C:\Jogos\Edição".
        let mut ansi = b"game = C:\\Jogos\\Edi".to_vec();
        ansi.extend([0xE7, 0xE3]);
        ansi.extend(b"o\r\n");
        std::fs::write(&path, &ansi).unwrap();
        let text = read_file(&path).unwrap().unwrap();
        assert_eq!(file(&text).0.game.as_deref(), Some("C:\\Jogos\\Edição"));
        std::fs::write(&path, "\u{feff}game = D:\\Edição\n").unwrap();
        let text = read_file(&path).unwrap().unwrap();
        assert_eq!(
            file(&text).0.game.as_deref(),
            Some("D:\\Edição"),
            "BOM dropped"
        );
        // Not a file: a real error, so the caller won't overwrite it.
        assert!(read_file(&dir).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
