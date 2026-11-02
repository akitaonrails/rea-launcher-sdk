//! One table for the window and the command line.
//!
//! Each [`OptionSpec`] row names a setting once: its `launcher.cfg` key,
//! its flag, what values it takes, and the words the window and `--help`
//! show. From the table come [`usage`] (the option lines of `--help`),
//! [`template`] (a commented `launcher.cfg`), [`parse_file`] and
//! [`parse_args`] (values checked against the row, then handed to the
//! launcher as a typed [`Value`]), and [`assert_parity`] for a test that
//! the window shows every option the command line has, and the reverse.
//! The launcher keeps its own typed settings struct and `apply`.

use super::{
    Aliases, ArgError, KeyStyle, ON_OFF, Rejected, Resolution, parse_bool, parse_resolution,
};

/// What values an option takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `on`/`off` (and the other [`parse_bool`] words).
    Toggle,
    /// One of these `(value, label)` pairs; values match without case.
    Choice(&'static [(&'static str, &'static str)]),
    /// `WIDTHxHEIGHT` within `min..=max`, or one of the `aliases` words.
    Size {
        min: (u32, u32),
        max: (u32, u32),
        aliases: &'static Aliases,
    },
    /// A whole number in `min..=max`.
    Range { min: u32, max: u32 },
    /// A folder or file.
    Path,
    /// Free text.
    Text,
}

/// One option: a `launcher.cfg` key, a flag and a window control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OptionSpec {
    /// The `launcher.cfg` key.
    pub key: &'static str,
    /// The flag when it is not the one [`KeyStyle`] derives from `key`.
    pub flag: Option<&'static str>,
    pub kind: Kind,
    /// Short name: the window's control label and the `--help` line.
    pub label: &'static str,
    /// What it does, for the template and the window's note.
    pub description: &'static str,
    /// A value shown in the template (`#key = example`).
    pub example: Option<&'static str>,
    /// Has no window control on purpose (it does not change the game:
    /// the runtime folder, a log switch for bug reports).
    pub launcher_only: bool,
    /// Old names still read from `launcher.cfg` and as flags, silently
    /// (a renamed key); never written, listed or shown.
    pub aliases: &'static [&'static str],
    /// Read by the Linux play helper, not the game's exe (the gamescope
    /// scaler, vkBasalt post-processing). It still needs a window control
    /// and a key; the helper must honour the one-run flag passed to it as
    /// well as `launcher.cfg`.
    pub helper: bool,
    /// For `Toggle` and `Choice`: the window also offers leaving the value
    /// unset, under this label (e.g. "As the game"); unset means the key
    /// is cleared from `launcher.cfg` and the game keeps its own value.
    pub unset: Option<&'static str>,
    /// Replaces the generated [`OptionSpec::needs`] text
    /// ("10–240 or unlimited").
    pub needs_text: Option<&'static str>,
    /// Replaces the generated [`OptionSpec::placeholder`] (`<10-240|unlimited>`).
    pub placeholder_text: Option<&'static str>,
    /// `(word, canonical value)`: other spellings read as a value, silently
    /// (an old `true` for `on`, `unlimited` for `0`). A canonical value
    /// named here is accepted even outside a `Range` (a special value).
    pub value_aliases: &'static [(&'static str, &'static str)],
    /// A key the launcher no longer has (see [`OptionSpec::removed`]), with
    /// a note on why.
    pub removed: Option<&'static str>,
}

impl OptionSpec {
    pub const fn new(
        key: &'static str,
        kind: Kind,
        label: &'static str,
        description: &'static str,
    ) -> OptionSpec {
        OptionSpec {
            key,
            flag: None,
            kind,
            label,
            description,
            example: None,
            launcher_only: false,
            aliases: &[],
            helper: false,
            unset: None,
            needs_text: None,
            placeholder_text: None,
            value_aliases: &[],
            removed: None,
        }
    }

    /// A key the launcher removed (CMR4's `widescreen`): still accepted in
    /// `launcher.cfg` and as a flag with any value and ignored, never
    /// listed by [`usage`]/[`template`] or asked of the window, and
    /// commented out by [`retire_removed`] when the file is saved. `why` is
    /// for the launcher's log or docs.
    pub const fn removed(key: &'static str, why: &'static str) -> OptionSpec {
        let mut spec = OptionSpec::new(key, Kind::Text, "", why);
        spec.launcher_only = true;
        spec.removed = Some(why);
        spec
    }

    /// Uses `text` for what the option takes, in messages and the template.
    pub const fn with_needs(mut self, text: &'static str) -> OptionSpec {
        self.needs_text = Some(text);
        self
    }

    /// Uses `text` as the `--help` value placeholder.
    pub const fn with_placeholder(mut self, text: &'static str) -> OptionSpec {
        self.placeholder_text = Some(text);
        self
    }

    /// Reads other spellings as values (see the field).
    pub const fn with_value_aliases(
        mut self,
        aliases: &'static [(&'static str, &'static str)],
    ) -> OptionSpec {
        self.value_aliases = aliases;
        self
    }

    /// Offers "unset" in the window under `label` (see the field).
    pub const fn unset_label(mut self, label: &'static str) -> OptionSpec {
        self.unset = Some(label);
        self
    }

    /// Uses `flag` instead of the derived one.
    pub const fn with_flag(mut self, flag: &'static str) -> OptionSpec {
        self.flag = Some(flag);
        self
    }

    /// Shows `value` in the template.
    pub const fn with_example(mut self, value: &'static str) -> OptionSpec {
        self.example = Some(value);
        self
    }

    /// Marks an option with no window control (see the field).
    pub const fn launcher_only(mut self) -> OptionSpec {
        self.launcher_only = true;
        self
    }

    /// Marks an option the Linux play helper reads (see the field).
    pub const fn helper_owned(mut self) -> OptionSpec {
        self.helper = true;
        self
    }

    /// Also reads these old key names (SRR's `frame-limit` after the
    /// rename to `frame_limit`), in the file and as flags.
    pub const fn with_aliases(mut self, aliases: &'static [&'static str]) -> OptionSpec {
        self.aliases = aliases;
        self
    }

    /// True for the key or one of its aliases.
    pub fn answers_to(&self, key: &str) -> bool {
        self.key == key || self.aliases.contains(&key)
    }

    /// True for the flag or an alias's flag (in `style`).
    fn takes_flag(&self, flag: &str, style: KeyStyle) -> bool {
        self.flag_name(style) == flag || self.aliases.iter().any(|a| style.flag(a) == flag)
    }

    /// [`OptionSpec::takes_flag`] with `_` read as `-` (`--display_mode`).
    fn takes_flag_loosely(&self, flag: &str, style: KeyStyle) -> bool {
        let loose = |f: &str| f.replace('_', "-");
        let flag = loose(flag);
        loose(&self.flag_name(style)) == flag
            || self.aliases.iter().any(|a| loose(&style.flag(a)) == flag)
    }

    /// The command-line flag.
    pub fn flag_name(&self, style: KeyStyle) -> String {
        self.flag
            .map_or_else(|| style.flag(self.key), str::to_owned)
    }

    /// What the option takes, for messages: `on or off`, `a, b or c`,
    /// `WIDTHxHEIGHT or original`, `30 to 150`, `a folder`, `text`.
    pub fn needs(&self) -> String {
        if let Some(text) = self.needs_text {
            return text.to_owned();
        }
        match self.kind {
            Kind::Toggle => ON_OFF.to_owned(),
            Kind::Choice(table) => super::one_of(table),
            Kind::Size { aliases, .. } => {
                let mut words: Vec<&str> = vec!["WIDTHxHEIGHT"];
                for (word, _) in aliases {
                    if !words.contains(word) {
                        words.push(word);
                    }
                }
                join_or(&words)
            }
            Kind::Range { min, max } => {
                let mut words = vec![format!("{min} to {max}")];
                words.extend(self.special_words().map(str::to_owned));
                join_or(&words.iter().map(String::as_str).collect::<Vec<_>>())
            }
            Kind::Path => "a folder or file".to_owned(),
            Kind::Text => "text".to_owned(),
        }
    }

    /// The value part of the `--help` line: `on|off`, `a|b|c`, `<WxH>`,
    /// `<30-150>`, `<folder>`, `<text>`.
    pub fn placeholder(&self) -> String {
        if let Some(text) = self.placeholder_text {
            return text.to_owned();
        }
        match self.kind {
            Kind::Toggle => "on|off".to_owned(),
            Kind::Choice(table) => table.iter().map(|(v, _)| *v).collect::<Vec<_>>().join("|"),
            Kind::Size { .. } => "<WxH>".to_owned(),
            Kind::Range { min, max } => {
                let mut parts = vec![format!("{min}-{max}")];
                parts.extend(self.special_words().map(str::to_owned));
                format!("<{}>", parts.join("|"))
            }
            Kind::Path => "<folder>".to_owned(),
            Kind::Text => "<text>".to_owned(),
        }
    }

    /// Value-alias words naming a number outside a `Range` (`off` for 0).
    fn special_words(&self) -> impl Iterator<Item = &'static str> + '_ {
        let range = match self.kind {
            Kind::Range { min, max } => Some(min..=max),
            _ => None,
        };
        self.value_aliases
            .iter()
            .filter_map(move |(word, canonical)| {
                let n: u32 = canonical.trim().parse().ok()?;
                range.as_ref().filter(|r| !r.contains(&n)).map(|_| *word)
            })
    }

    /// `value` checked against the row and parsed.
    pub fn parse<'a>(&self, value: &'a str) -> Result<Value<'a>, Rejected> {
        let value = self
            .value_aliases
            .iter()
            .find(|(word, _)| word.eq_ignore_ascii_case(value.trim()))
            .map_or(value, |(_, canonical)| *canonical);
        let special = |n: &u32| {
            self.value_aliases
                .iter()
                .any(|(_, canonical)| canonical.trim().parse() == Ok(*n))
        };
        let parsed = match self.kind {
            Kind::Toggle => parse_bool(value).map(Value::Toggle),
            Kind::Choice(table) => table
                .iter()
                .find(|(v, _)| v.eq_ignore_ascii_case(value.trim()))
                .map(|(v, _)| Value::Choice(v)),
            Kind::Size { min, max, aliases } => {
                parse_resolution(value, min, max, aliases).map(Value::Size)
            }
            Kind::Range { min, max } => value
                .trim()
                .parse()
                .ok()
                .filter(|n| (min..=max).contains(n) || special(n))
                .map(Value::Number),
            Kind::Path | Kind::Text => Some(Value::Text(value.trim())),
        };
        parsed.ok_or_else(|| Rejected::needs(self.needs()))
    }
}

/// A value checked against its [`OptionSpec`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Value<'a> {
    Toggle(bool),
    /// The table's own spelling of the value.
    Choice(&'static str),
    Size(Resolution),
    Number(u32),
    /// A path or text, trimmed.
    Text(&'a str),
}

fn join_or(words: &[&str]) -> String {
    match words.split_last() {
        None => String::new(),
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
    }
}

/// The row for `key` or one of its aliases.
pub fn find<'s>(specs: &'s [OptionSpec], key: &str) -> Option<&'s OptionSpec> {
    specs.iter().find(|s| s.answers_to(key))
}

/// [`super::set_key`] for a row: writes the canonical key and turns any
/// alias lines into comments, so the old name no longer overrides it.
pub fn set_key(text: &str, spec: &OptionSpec, value: &str) -> String {
    let mut out = super::set_key(text, spec.key, value);
    for alias in spec.aliases {
        out = super::clear_key(&out, alias);
    }
    out
}

/// Checks `value` against `key`'s row and hands the parsed value to
/// `apply`; a key without a row is [`Rejected::Unknown`].
pub fn dispatch(
    specs: &[OptionSpec],
    key: &str,
    value: &str,
    apply: &mut dyn FnMut(&str, Value) -> Result<(), Rejected>,
) -> Result<(), Rejected> {
    let spec = find(specs, key).ok_or(Rejected::Unknown)?;
    if spec.removed.is_some() {
        return Ok(());
    }
    apply(spec.key, spec.parse(value)?)
}

/// Comments out the lines of removed keys ([`OptionSpec::removed`]), for
/// the launcher to call when it saves `launcher.cfg`.
pub fn retire_removed(text: &str, specs: &[OptionSpec]) -> String {
    specs
        .iter()
        .filter(|s| s.removed.is_some())
        .fold(text.to_owned(), |text, s| {
            let text = super::clear_key(&text, s.key);
            s.aliases.iter().fold(text, |t, a| super::clear_key(&t, a))
        })
}

/// The rows a window or listing shows: all but removed keys.
pub fn active(specs: &[OptionSpec]) -> impl Iterator<Item = &OptionSpec> {
    specs.iter().filter(|s| s.removed.is_none())
}

/// [`super::parse`] with every value checked against `specs` first.
pub fn parse_file(
    text: &str,
    file_name: &str,
    specs: &[OptionSpec],
    mut apply: impl FnMut(&str, Value) -> Result<(), Rejected>,
) -> Vec<String> {
    super::parse(text, file_name, |key, value| {
        dispatch(specs, key, value, &mut apply)
    })
}

/// [`super::parse_args_with`] for a spec table: each row's
/// [`OptionSpec::flag_name`] is its flag (`--flag value` or
/// `--flag=value`; `_` read as `-` as a fallback), values are checked
/// before `apply`, and `switches` (value-less launcher flags, never with
/// `=value`) come back in order.
pub fn parse_args<I: IntoIterator<Item = String>>(
    args: I,
    style: KeyStyle,
    specs: &[OptionSpec],
    switches: &[&str],
    mut apply: impl FnMut(&str, Value) -> Result<(), Rejected>,
) -> Result<Vec<String>, ArgError> {
    super::each_arg(
        args,
        switches,
        |flag| {
            specs
                .iter()
                .find(|s| s.takes_flag(flag, style))
                .or_else(|| specs.iter().find(|s| s.takes_flag_loosely(flag, style)))
                .map(|s| s.key.to_owned())
        },
        |key, value| dispatch(specs, key, value, &mut apply),
    )
}

/// The option lines of `--help`: `  --flag <value>  label`, aligned;
/// helper-owned options end in `(Linux helper)`; removed keys are left
/// out. The launcher adds its own heading around them; see
/// [`usage_with`] for its value-less switches.
pub fn usage(specs: &[OptionSpec], style: KeyStyle) -> String {
    usage_with(specs, style, &[])
}

/// [`usage`] followed by the launcher's value-less switches, given as
/// `(flag, description)` (`("--no-window", "play with the saved
/// settings")`), all in one aligned column.
pub fn usage_with(specs: &[OptionSpec], style: KeyStyle, switches: &[(&str, &str)]) -> String {
    let mut rows: Vec<(String, String)> = active(specs)
        .map(|s| {
            let helper = if s.helper { " (Linux helper)" } else { "" };
            (
                format!("{} {}", s.flag_name(style), s.placeholder()),
                format!("{}{helper}", s.label),
            )
        })
        .collect();
    rows.extend(
        switches
            .iter()
            .map(|(f, d)| ((*f).to_owned(), (*d).to_owned())),
    );
    let width = rows
        .iter()
        .map(|(l, _)| l.chars().count())
        .max()
        .unwrap_or(0);
    rows.iter()
        .map(|(l, d)| format!("  {l:width$}  {d}\n"))
        .collect()
}

/// Wraps `text` into `# ` comment lines of at most 76 characters.
fn comment(text: &str, out: &mut String) {
    let mut line = String::from("#");
    for word in text.split_whitespace() {
        if line.len() > 1 && line.len() + 1 + word.len() > 76 {
            out.push_str(&line);
            out.push('\n');
            line = String::from("#");
        }
        line.push(' ');
        line.push_str(word);
    }
    if line.len() > 1 {
        out.push_str(&line);
        out.push('\n');
    }
}

/// A commented `launcher.cfg`: `heading` as comments, then per row its
/// description, the values it takes and a `#key =` placeholder (with the
/// example), which [`super::set_key`] fills in place. Parses to nothing.
pub fn template(heading: &str, specs: &[OptionSpec]) -> String {
    let mut out = String::new();
    for line in heading.lines() {
        if line.trim().is_empty() {
            out.push_str("#\n");
        } else {
            comment(line, &mut out);
        }
    }
    for spec in active(specs) {
        out.push('\n');
        comment(spec.description, &mut out);
        comment(&format!("Values: {}.", spec.needs()), &mut out);
        match spec.example {
            Some(example) => out.push_str(&format!("#{} = {example}\n", spec.key)),
            None => out.push_str(&format!("#{} =\n", spec.key)),
        }
    }
    out
}

/// What is wrong with a table and a window's controls: duplicate keys or
/// flags, an alias naming another row, an empty choice, examples the row would refuse, rows (not
/// `launcher_only`) with no window control, and window controls with no
/// row. Empty when all is well.
pub fn parity_problems(specs: &[OptionSpec], style: KeyStyle, window_keys: &[&str]) -> Vec<String> {
    let mut problems = Vec::new();
    for (i, spec) in specs.iter().enumerate() {
        for other in &specs[i + 1..] {
            if spec.key == other.key {
                problems.push(format!("key {} is listed twice", spec.key));
            }
            let shared = spec
                .aliases
                .iter()
                .chain([&spec.key])
                .find(|name| other.answers_to(name));
            if let Some(name) = shared.filter(|_| spec.key != other.key) {
                problems.push(format!("{name} names both {} and {}", spec.key, other.key));
            }
            if spec.flag_name(style) == other.flag_name(style) {
                problems.push(format!("flag {} is listed twice", spec.flag_name(style)));
            }
        }
        if matches!(spec.kind, Kind::Choice([])) {
            problems.push(format!("{} has no choices", spec.key));
        }
        if let Some(example) = spec.example
            && spec.parse(example).is_err()
        {
            problems.push(format!(
                "{} example {example:?} is not a valid value",
                spec.key
            ));
        }
        let shown = window_keys.contains(&spec.key);
        if !spec.launcher_only && !shown {
            problems.push(format!(
                "{} ({}) has no window control",
                spec.key,
                spec.flag_name(style)
            ));
        }
    }
    for key in window_keys {
        if !active(specs).any(|s| s.key == *key) {
            problems.push(format!(
                "window control {key} has no option row (no flag, no launcher.cfg key)"
            ));
        }
    }
    problems
}

/// [`parity_problems`] as one message for [`assert_parity`]; `Ok` when
/// there are none.
pub fn check_parity(
    specs: &[OptionSpec],
    style: KeyStyle,
    window_keys: &[&str],
) -> Result<(), String> {
    let problems = parity_problems(specs, style, window_keys);
    if problems.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "window and command line differ:\n  {}",
            problems.join("\n  ")
        ))
    }
}

/// For a launcher's tests: panics listing every [`parity_problems`] item
/// (flags as [`KeyStyle::Underscore`] derives them; see [`check_parity`]
/// for another style). `window_keys` are the keys the settings window
/// has controls for.
#[track_caller]
pub fn assert_parity(specs: &[OptionSpec], window_keys: &[&str]) {
    if let Err(message) = check_parity(specs, KeyStyle::Underscore, window_keys) {
        panic!("{message}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfg::ORIGINAL;

    const MODES: &[(&str, &str)] = &[
        ("fullscreen", "Fullscreen"),
        ("windowed", "Windowed"),
        ("borderless", "Borderless"),
    ];

    const SPECS: &[OptionSpec] = &[
        OptionSpec::new(
            "game",
            Kind::Path,
            "Game folder",
            "The folder with your installed game. It is only read.",
        )
        .launcher_only(),
        OptionSpec::new(
            "resolution",
            Kind::Size {
                min: (640, 480),
                max: (7680, 4320),
                aliases: ORIGINAL,
            },
            "Resolution",
            "Display resolution, or original for the game's own size.",
        )
        .with_example("1920x1080"),
        OptionSpec::new(
            "display_mode",
            Kind::Choice(MODES),
            "Display mode",
            "How the game fills the screen.",
        ),
        OptionSpec::new(
            "vsync",
            Kind::Toggle,
            "VSync",
            "Wait for the screen's refresh before each frame.",
        )
        .with_example("on"),
        OptionSpec::new(
            "fov",
            Kind::Range { min: 30, max: 150 },
            "Field of view",
            "Degrees, both cameras.",
        ),
        OptionSpec::new(
            "gamepad_log",
            Kind::Toggle,
            "Gamepad log",
            "Xidi's log, for bug reports.",
        )
        .with_flag("--pad-log")
        .launcher_only(),
    ];

    #[derive(Debug, Default, PartialEq)]
    struct Settings {
        game: Option<String>,
        resolution: Option<Resolution>,
        mode: Option<&'static str>,
        vsync: Option<bool>,
        fov: Option<u32>,
        pad_log: Option<bool>,
    }

    impl Settings {
        fn apply(&mut self, key: &str, value: Value) -> Result<(), Rejected> {
            match (key, value) {
                ("game", Value::Text(t)) => self.game = Some(t.to_owned()),
                ("resolution", Value::Size(r)) => self.resolution = Some(r),
                ("display_mode", Value::Choice(c)) => self.mode = Some(c),
                ("vsync", Value::Toggle(b)) => self.vsync = Some(b),
                ("fov", Value::Number(n)) => self.fov = Some(n - n % 5),
                ("gamepad_log", Value::Toggle(b)) => self.pad_log = Some(b),
                _ => return Err(Rejected::Unknown),
            }
            Ok(())
        }
    }

    fn args(a: &[&str]) -> Result<(Settings, Vec<String>), ArgError> {
        let mut s = Settings::default();
        let seen = parse_args(
            a.iter().map(|x| x.to_string()),
            KeyStyle::Underscore,
            SPECS,
            &["--no-window"],
            |k, v| s.apply(k, v),
        )?;
        Ok((s, seen))
    }

    #[test]
    fn the_file_and_the_flags_go_through_the_table() {
        let mut s = Settings::default();
        let warnings = parse_file(
            "game = C:\\G\nresolution = 2560x1440\ndisplay_mode = Windowed\nvsync = off\nfov = 77\n",
            "launcher.cfg",
            SPECS,
            |k, v| s.apply(k, v),
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(s.game.as_deref(), Some("C:\\G"));
        assert_eq!(s.resolution, Some(Resolution::Size(2560, 1440)));
        assert_eq!(s.mode, Some("windowed"), "the table's spelling");
        assert_eq!((s.vsync, s.fov), (Some(false), Some(75)));

        let (s, seen) = args(&[
            "--no-window",
            "--display-mode",
            "borderless",
            "--pad-log",
            "on",
        ])
        .unwrap();
        assert_eq!(seen, ["--no-window"]);
        assert_eq!((s.mode, s.pad_log), (Some("borderless"), Some(true)));
        assert_eq!(
            args(&["--fov", "200"]).unwrap_err().to_string(),
            "--fov needs 30 to 150, not 200"
        );
        assert_eq!(
            args(&["--display-mode", "wide"]).unwrap_err().to_string(),
            "--display-mode needs fullscreen, windowed or borderless, not wide"
        );
        assert_eq!(
            args(&["--gamepad-log", "on"]).unwrap_err(),
            ArgError::Unknown("--gamepad-log".into())
        );
        assert_eq!(
            args(&["--vsync"]).unwrap_err(),
            ArgError::MissingValue("--vsync".into())
        );
        assert_eq!(args(&["-h"]).unwrap_err(), ArgError::Help("-h".into()));
    }

    #[test]
    fn spec_flags_take_equals_and_underscores() {
        let (s, seen) =
            args(&["--display_mode=borderless", "--pad-log=on", "--no-window"]).unwrap();
        assert_eq!((s.mode, s.pad_log), (Some("borderless"), Some(true)));
        assert_eq!(seen, ["--no-window"]);
        assert!(matches!(
            args(&["--no-window=yes"]),
            Err(ArgError::Bad { .. })
        ));
        assert_eq!(
            args(&["--fov=200"]).unwrap_err().to_string(),
            "--fov needs 30 to 150, not 200"
        );
    }

    #[test]
    fn bad_file_values_are_reported_from_the_table() {
        let mut s = Settings::default();
        let warnings = parse_file(
            "resolution = 100x100\nbogus = 1\n",
            "launcher.cfg",
            SPECS,
            |k, v| s.apply(k, v),
        );
        assert_eq!(warnings.len(), 2, "{warnings:?}");
        assert!(warnings[0].contains("resolution needs WIDTHxHEIGHT or original"));
        assert!(warnings[1].contains("unknown setting \"bogus\""));
    }

    #[test]
    fn usage_lists_every_flag_aligned() {
        let text = usage(SPECS, KeyStyle::Underscore);
        assert_eq!(text.lines().count(), SPECS.len());
        assert!(text.contains("  --display-mode fullscreen|windowed|borderless  Display mode\n"));
        assert!(text.contains("  --pad-log on|off "));
        assert!(text.contains("  --fov <30-150> "));
        let columns: Vec<usize> = text.lines().map(|l| l.rfind("  ").unwrap()).collect();
        assert!(columns.windows(2).all(|w| w[0] == w[1]), "{text}");
    }

    #[test]
    fn the_template_parses_to_nothing_and_set_key_fills_it_in_place() {
        let text = template("Example launcher settings.\n\nOne setting per line.", SPECS);
        assert!(text.starts_with("# Example launcher settings.\n#\n# One setting per line.\n"));
        assert!(text.contains("#resolution = 1920x1080\n"));
        assert!(text.contains("# Values: fullscreen, windowed or borderless.\n#display_mode =\n"));
        let mut s = Settings::default();
        let warnings = parse_file(&text, "launcher.cfg", SPECS, |k, v| s.apply(k, v));
        assert!(
            warnings.is_empty() && s == Settings::default(),
            "{warnings:?}"
        );
        let filled = crate::cfg::set_key(&text, "vsync", "off");
        assert!(filled.contains("before each frame.\n# Values: on or off.\nvsync = off\n"));
        assert!(text.lines().all(|l| l.chars().count() <= 78));
    }

    #[test]
    fn parity_lists_what_the_window_or_the_table_lacks() {
        assert_parity(SPECS, &["resolution", "display_mode", "vsync", "fov"]);
        let problems = parity_problems(
            SPECS,
            KeyStyle::Underscore,
            &["resolution", "vsync", "fov", "shadows"],
        );
        assert_eq!(
            problems,
            [
                "display_mode (--display-mode) has no window control",
                "window control shadows has no option row (no flag, no launcher.cfg key)",
            ]
        );
        const BAD: &[OptionSpec] = &[
            OptionSpec::new("a", Kind::Choice(&[]), "A", "").launcher_only(),
            OptionSpec::new("a", Kind::Toggle, "A", "")
                .with_example("maybe")
                .launcher_only(),
        ];
        let problems = parity_problems(BAD, KeyStyle::Underscore, &[]);
        assert_eq!(problems.len(), 4, "{problems:?}");
    }

    #[test]
    fn the_parity_message_lists_every_problem() {
        // (A panic aborts the whole test binary under Wine, so the message
        // assert_parity panics with is tested here.)
        let err = check_parity(
            SPECS,
            KeyStyle::Underscore,
            &["resolution", "display_mode", "vsync"],
        )
        .unwrap_err();
        assert_eq!(
            err,
            "window and command line differ:\n  fov (--fov) has no window control"
        );
        assert_eq!(
            check_parity(
                SPECS,
                KeyStyle::Underscore,
                &["resolution", "display_mode", "vsync", "fov"]
            ),
            Ok(())
        );
    }

    #[test]
    fn custom_texts_and_value_aliases() {
        const FPS: OptionSpec = OptionSpec::new(
            "fps_limit",
            Kind::Range { min: 10, max: 240 },
            "Frame limit",
            "Frames per second.",
        )
        .with_needs("10–240 or unlimited")
        .with_placeholder("<10-240|unlimited>")
        .with_value_aliases(&[("unlimited", "0"), ("off", "0")])
        .with_example("unlimited");
        assert_eq!(FPS.parse("Unlimited"), Ok(Value::Number(0)));
        assert_eq!(FPS.parse("0"), Ok(Value::Number(0)), "a special value");
        assert_eq!(FPS.parse("60"), Ok(Value::Number(60)));
        assert_eq!(FPS.parse("5"), Err(Rejected::needs("10–240 or unlimited")));
        assert!(usage(&[FPS], KeyStyle::Underscore).contains("--fps-limit <10-240|unlimited>"));
        let text = template("x", &[FPS]);
        assert!(text.contains("# Values: 10–240 or unlimited.\n#fps_limit = unlimited\n"));
        assert_eq!(
            parity_problems(&[FPS], KeyStyle::Underscore, &["fps_limit"]),
            Vec::<String>::new()
        );
        // Old spellings of a toggle and a choice.
        const VSYNC: OptionSpec = OptionSpec::new(
            "vsync",
            Kind::Choice(&[("on", "On"), ("off", "Off"), ("adaptive", "Adaptive")]),
            "VSync",
            "",
        )
        .with_value_aliases(&[("true", "on"), ("half", "adaptive")]);
        assert_eq!(VSYNC.parse("TRUE"), Ok(Value::Choice("on")));
        assert_eq!(VSYNC.parse("half"), Ok(Value::Choice("adaptive")));
        assert_eq!(VSYNC.needs(), "on, off or adaptive", "aliases stay silent");
    }

    #[test]
    fn an_off_word_on_a_range_is_listed() {
        const FPS: OptionSpec = OptionSpec::new(
            "fps_limit",
            Kind::Range { min: 1, max: 1000 },
            "FPS limit",
            "",
        )
        .with_value_aliases(&[("off", "0"), ("max", "1000")]);
        assert_eq!(FPS.parse("off"), Ok(Value::Number(0)));
        assert_eq!(FPS.parse("max"), Ok(Value::Number(1000)));
        assert_eq!(
            FPS.needs(),
            "1 to 1000 or off",
            "an alias inside the range stays silent"
        );
        assert_eq!(FPS.placeholder(), "<1-1000|off>");
    }

    #[test]
    fn removed_keys_are_read_and_ignored_then_retired() {
        const TABLE: &[OptionSpec] = &[
            OptionSpec::new("vsync", Kind::Toggle, "VSync", "Wait for the refresh."),
            OptionSpec::removed("widescreen", "the 16:9 patch is gone; dgVoodoo scales"),
        ];
        let mut seen = Vec::new();
        let warnings = parse_file(
            "widescreen = on\nvsync = on\n",
            "launcher.cfg",
            TABLE,
            |k, _| {
                seen.push(k.to_owned());
                Ok(())
            },
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(seen, ["vsync"]);
        let switches = parse_args(
            ["--widescreen", "anything", "--vsync", "off"].map(String::from),
            KeyStyle::Underscore,
            TABLE,
            &[],
            |k, _| {
                seen.push(k.to_owned());
                Ok(())
            },
        )
        .unwrap();
        assert!(switches.is_empty());
        assert_eq!(seen, ["vsync", "vsync"]);
        assert_eq!(
            retire_removed("widescreen = on\nvsync = on\n", TABLE),
            "#widescreen =\nvsync = on\n"
        );
        assert!(!usage(TABLE, KeyStyle::Underscore).contains("widescreen"));
        assert!(!template("x", TABLE).contains("widescreen"));
        assert_eq!(
            check_parity(TABLE, KeyStyle::Underscore, &["vsync"]),
            Ok(())
        );
        assert!(check_parity(TABLE, KeyStyle::Underscore, &["vsync", "widescreen"]).is_err());
        assert_eq!(active(TABLE).count(), 1);
    }

    #[test]
    fn usage_lists_switches_in_the_same_column() {
        const ONE: &[OptionSpec] = &[OptionSpec::new("vsync", Kind::Toggle, "VSync", "")];
        let text = usage_with(
            ONE,
            KeyStyle::Underscore,
            &[("--no-window", "play with the saved settings")],
        );
        assert_eq!(
            text,
            "  --vsync on|off  VSync\n  --no-window     play with the saved settings\n"
        );
    }

    #[test]
    fn helper_options_are_marked_and_still_need_a_control() {
        const SCALERS: &[(&str, &str)] = &[("fsr", "FSR"), ("linear", "Smooth")];
        const LINUX: &[OptionSpec] = &[OptionSpec::new(
            "scaler",
            Kind::Choice(SCALERS),
            "Scaler",
            "How gamescope enlarges the picture.",
        )
        .helper_owned()];
        assert!(LINUX[0].helper);
        assert!(usage(LINUX, KeyStyle::Underscore).ends_with("Scaler (Linux helper)\n"));
        assert!(check_parity(LINUX, KeyStyle::Underscore, &[]).is_err());
        assert_eq!(
            check_parity(LINUX, KeyStyle::Underscore, &["scaler"]),
            Ok(())
        );
    }

    #[test]
    fn renamed_keys_still_read_their_old_names() {
        const RENAMED: &[OptionSpec] = &[OptionSpec::new(
            "frame_limit",
            Kind::Range { min: 0, max: 240 },
            "Frame limit",
            "Frames per second; 0 is unlimited.",
        )
        .with_aliases(&["frame-limit"])];
        let mut got = Vec::new();
        let warnings = parse_file("frame-limit = 60\n", "launcher.cfg", RENAMED, |k, v| {
            got.push((k.to_owned(), v == Value::Number(60)));
            Ok(())
        });
        assert!(warnings.is_empty(), "silent: {warnings:?}");
        for flag in ["--frame-limit", "--frame_limit"] {
            parse_args(
                [flag.to_string(), "60".into()],
                KeyStyle::Hyphen,
                RENAMED,
                &[],
                |k, v| {
                    got.push((k.to_owned(), v == Value::Number(60)));
                    Ok(())
                },
            )
            .unwrap();
        }
        assert_eq!(
            got,
            vec![("frame_limit".to_string(), true); 3],
            "always the canonical key"
        );
        // Saving writes the new name and retires the old line.
        assert_eq!(
            set_key("# fps\nframe-limit = 60\n", &RENAMED[0], "30"),
            "# fps\n#frame-limit =\nframe_limit = 30\n"
        );
        // Only the canonical name is listed, and parity ignores aliases.
        assert!(!usage(RENAMED, KeyStyle::Hyphen).contains("--frame-limit "));
        assert!(!template("x", RENAMED).contains("frame-limit"));
        assert_eq!(
            check_parity(RENAMED, KeyStyle::Hyphen, &["frame_limit"]),
            Ok(())
        );
        assert!(check_parity(RENAMED, KeyStyle::Hyphen, &["frame-limit"]).is_err());
        const CLASH: &[OptionSpec] = &[
            OptionSpec::new("a", Kind::Toggle, "A", "").launcher_only(),
            OptionSpec::new("b", Kind::Toggle, "B", "")
                .with_aliases(&["a"])
                .launcher_only(),
        ];
        assert_eq!(
            parity_problems(CLASH, KeyStyle::Underscore, &[]),
            ["a names both a and b"]
        );
    }

    #[test]
    fn hyphen_style_flags_and_needs() {
        const SRR: &[OptionSpec] = &[OptionSpec::new(
            "frame-limit",
            Kind::Range { min: 0, max: 240 },
            "Frame limit",
            "",
        )];
        assert_eq!(SRR[0].flag_name(KeyStyle::Hyphen), "--frame-limit");
        let mut got = None;
        parse_args(
            ["--frame-limit".to_string(), "60".to_string()],
            KeyStyle::Hyphen,
            SRR,
            &[],
            |k, v| {
                got = Some((k.to_owned(), v == Value::Number(60)));
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(got, Some(("frame-limit".to_string(), true)));
        let scr = OptionSpec::new(
            "resolution",
            Kind::Size {
                min: (320, 200),
                max: (7680, 4320),
                aliases: crate::cfg::ORIGINAL_OR_FULLSCREEN,
            },
            "",
            "",
        );
        assert_eq!(
            scr.needs(),
            "WIDTHxHEIGHT, original, off, fullscreen, full or desktop"
        );
        assert_eq!(
            scr.parse("desktop"),
            Ok(Value::Size(Resolution::Fullscreen))
        );
        assert_eq!(
            OptionSpec::new("p", Kind::Path, "", "").placeholder(),
            "<folder>"
        );
        assert_eq!(OptionSpec::new("t", Kind::Text, "", "").needs(), "text");
        let program = OptionSpec::new("dosbox", Kind::Path, "", "").with_placeholder("<program>");
        assert_eq!(program.placeholder(), "<program>");
        assert_eq!(
            program.parse(" /usr/bin/dosbox "),
            Ok(Value::Text("/usr/bin/dosbox"))
        );
    }
}
