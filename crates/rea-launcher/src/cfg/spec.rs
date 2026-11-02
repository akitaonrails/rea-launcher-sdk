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
        }
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

    /// The command-line flag.
    pub fn flag_name(&self, style: KeyStyle) -> String {
        self.flag
            .map_or_else(|| style.flag(self.key), str::to_owned)
    }

    /// What the option takes, for messages: `on or off`, `a, b or c`,
    /// `WIDTHxHEIGHT or original`, `30 to 150`, `a folder`, `text`.
    pub fn needs(&self) -> String {
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
            Kind::Range { min, max } => format!("{min} to {max}"),
            Kind::Path => "a folder or file".to_owned(),
            Kind::Text => "text".to_owned(),
        }
    }

    /// The value part of the `--help` line: `on|off`, `a|b|c`, `<WxH>`,
    /// `<30-150>`, `<folder>`, `<text>`.
    pub fn placeholder(&self) -> String {
        match self.kind {
            Kind::Toggle => "on|off".to_owned(),
            Kind::Choice(table) => table.iter().map(|(v, _)| *v).collect::<Vec<_>>().join("|"),
            Kind::Size { .. } => "<WxH>".to_owned(),
            Kind::Range { min, max } => format!("<{min}-{max}>"),
            Kind::Path => "<folder>".to_owned(),
            Kind::Text => "<text>".to_owned(),
        }
    }

    /// `value` checked against the row and parsed.
    pub fn parse<'a>(&self, value: &'a str) -> Result<Value<'a>, Rejected> {
        let parsed = match self.kind {
            Kind::Toggle => parse_bool(value).map(Value::Toggle),
            Kind::Choice(table) => table
                .iter()
                .find(|(v, _)| v.eq_ignore_ascii_case(value.trim()))
                .map(|(v, _)| Value::Choice(v)),
            Kind::Size { min, max, aliases } => {
                parse_resolution(value, min, max, aliases).map(Value::Size)
            }
            Kind::Range { min, max } => super::parse_range(value, min..=max).map(Value::Number),
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
    apply(spec.key, spec.parse(value)?)
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
/// [`OptionSpec::flag_name`] is its flag, values are checked before
/// `apply`, and `switches` (value-less launcher flags) come back in order.
pub fn parse_args<I: IntoIterator<Item = String>>(
    args: I,
    style: KeyStyle,
    specs: &[OptionSpec],
    switches: &[&str],
    mut apply: impl FnMut(&str, Value) -> Result<(), Rejected>,
) -> Result<Vec<String>, ArgError> {
    let mut seen = Vec::new();
    let mut args = args.into_iter();
    while let Some(flag) = args.next() {
        if matches!(flag.as_str(), "--help" | "-h" | "/?") {
            return Err(ArgError::Help(flag));
        }
        if switches.contains(&flag.as_str()) {
            seen.push(flag);
            continue;
        }
        let Some(spec) = specs.iter().find(|s| s.takes_flag(&flag, style)) else {
            return Err(ArgError::Unknown(flag));
        };
        let Some(value) = args.next() else {
            return Err(ArgError::MissingValue(flag));
        };
        match dispatch(specs, spec.key, &value, &mut apply) {
            Ok(()) => {}
            Err(Rejected::Unknown) => return Err(ArgError::Unknown(flag)),
            Err(Rejected::Needs(needs)) => return Err(ArgError::Bad { flag, value, needs }),
        }
    }
    Ok(seen)
}

/// The option lines of `--help`: `  --flag <value>  label`, aligned;
/// helper-owned options end in `(Linux helper)`.
/// The launcher adds its own heading and switch lines around them.
pub fn usage(specs: &[OptionSpec], style: KeyStyle) -> String {
    let left: Vec<String> = specs
        .iter()
        .map(|s| format!("{} {}", s.flag_name(style), s.placeholder()))
        .collect();
    let width = left.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    left.iter()
        .zip(specs)
        .map(|(l, s)| {
            let helper = if s.helper { " (Linux helper)" } else { "" };
            format!("  {l:width$}  {}{helper}\n", s.label)
        })
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
    for spec in specs {
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
        if !specs.iter().any(|s| s.key == *key) {
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
    }
}
