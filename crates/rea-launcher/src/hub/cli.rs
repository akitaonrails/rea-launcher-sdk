//! A hub launcher's command line, parsed without side effects so every
//! flag can be tested (Sega's `cli.rs`, made generic). The per-game
//! options come from the launcher's option table, so `--help`, the
//! settings files and the window agree.
//!
//! ```text
//! <program>                          the window
//! <program> --library <folder>       add a library folder (repeatable)
//! <program> --<path field> <path>    the launcher's other paths
//! <program> --list | --check [<game>]
//! <program> --play <game> [options] [test options]
//! <program> --set <game> <options>   (also --game)
//! ```
//!
//! Exit codes (docs/STANDARDS.md, "Family consistency"): [`OK`],
//! [`REFUSED`], [`BAD_COMMAND_LINE`].

use std::path::PathBuf;

use crate::cfg::{KeyStyle, OptionSpec, Value};

use super::config::PathField;

pub const OK: u8 = 0;
pub const REFUSED: u8 = 1;
pub const BAD_COMMAND_LINE: u8 = 2;

/// What a launcher's command line knows.
#[derive(Debug, Clone, Copy)]
pub struct CliSpec<'a> {
    /// `initiald-launcher`.
    pub program: &'a str,
    /// The first line of `--help`, after the program: "the Initial D arcade series".
    pub about: &'a str,
    /// The games' ids, as `--play` takes them.
    pub games: &'a [&'a str],
    /// The per-game options.
    pub options: &'a [OptionSpec],
    pub style: KeyStyle,
    pub paths: &'a [PathField],
    /// Whether game `id` takes option `key` (the rows its window offers);
    /// `None`: every game takes every option.
    pub offers: Option<fn(id: &str, key: &str) -> bool>,
    /// The launcher's word on the test options of `--play`: its own help
    /// text, and the games that ignore one. `&[]`: the SDK's help, every
    /// game takes every test option.
    pub test_options: &'a [TestOption<'a>],
}

/// A test option of `--play` as a launcher qualifies it.
#[derive(Debug, Clone, Copy)]
pub struct TestOption<'a> {
    /// `--headless`, `--no-input`, `--input-script` or `--seconds`.
    pub flag: &'a str,
    /// Its `--help` text: "timed inputs (windowsarcadeloader games)".
    pub help: &'a str,
    /// Why game `id` doesn't take it (refused on the command line), or
    /// `None` when it does; `None`: every game takes it.
    pub refuses: Option<fn(id: &str) -> Option<&'static str>>,
}

/// The test options of `--play`: flag, its value, the SDK's help.
const TEST_OPTIONS: [(&str, &str, &str); 4] = [
    ("--headless", "", "run in an invisible display"),
    ("--no-input", "", "no controller input"),
    ("--input-script", " <file>", "timed inputs"),
    ("--seconds", " <n>", "stop the game after n seconds"),
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// The window (no arguments).
    Window,
    Help,
    /// List the games found and their state.
    List,
    /// Check that a game (or every game found) can start, without starting it.
    Check(Option<String>),
    /// Save options for a game without starting it.
    Set(String),
    /// Start a game without the window.
    Play(String),
    /// Only library folders or paths were given: save them.
    Configure,
}

/// The test options of `--play`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Test {
    pub headless: bool,
    pub no_input: bool,
    pub script: Option<PathBuf>,
    pub seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub mode: Mode,
    pub libraries: Vec<PathBuf>,
    /// One per [`PathField`], as given.
    pub paths: Vec<Option<PathBuf>>,
    /// `(key, value)` per option flag, in order, values already checked.
    pub options: Vec<(&'static str, String)>,
    pub test: Test,
    /// `--linux`: started by the Linux helper or the AppImage.
    pub linux: bool,
}

impl Invocation {
    /// A script-facing command whose output is its result (`report::Run::command`).
    pub fn is_command(&self) -> bool {
        matches!(self.mode, Mode::Help | Mode::List | Mode::Check(_))
    }

    /// A run without the window (`report::Run::no_window`).
    pub fn no_window(&self) -> bool {
        matches!(self.mode, Mode::Play(_) | Mode::Set(_) | Mode::Configure)
    }

    /// An action that must not run while another launcher is open
    /// (`instance::single`). `--help` and `--list` only read: run them
    /// without the lock, so they work while the window is open.
    pub fn is_action(&self) -> bool {
        matches!(
            self.mode,
            Mode::Check(_) | Mode::Set(_) | Mode::Play(_) | Mode::Configure
        )
    }

    /// Feeds the options to `apply` as typed values: the keys in `first`
    /// before the others (a look, so the flags after it win), then the rest
    /// in command-line order.
    pub fn apply_options(
        &self,
        specs: &[OptionSpec],
        first: &[&str],
        mut apply: impl FnMut(&OptionSpec, Value<'_>),
    ) {
        let firsts = self.options.iter().filter(|(k, _)| first.contains(k));
        let rest = self.options.iter().filter(|(k, _)| !first.contains(k));
        for (key, value) in firsts.chain(rest) {
            if let Some(spec) = crate::cfg::spec::find(specs, key)
                && let Ok(v) = spec.parse(value)
            {
                apply(spec, v);
            }
        }
    }
}

/// `--help`.
pub fn usage(spec: &CliSpec) -> String {
    let p = spec.program;
    let mut lines: Vec<(String, String)> = vec![
        (
            String::new(),
            "open the window (games, settings, controls)".into(),
        ),
        (
            "--library <folder>".into(),
            "add a folder with your games (repeatable)".into(),
        ),
    ];
    for f in spec.paths {
        lines.push((format!("{} <{}>", f.flag, f.kind.noun()), f.help.into()));
    }
    lines.extend([
        (
            "--list".into(),
            "list the games found and their state".into(),
        ),
        (
            "--check [<game>]".into(),
            "check a game (or all) can start, without starting".into(),
        ),
        (
            "--play <game> [options]".into(),
            format!("start a game ({})", spec.games.join(", ")),
        ),
        (
            "--set <game> <options>".into(),
            "save options for a game without starting it (also --game)".into(),
        ),
    ]);
    let width = lines.iter().map(|(l, _)| l.len()).max().unwrap_or(0) + p.len() + 1;
    let line = |(flags, what): &(String, String)| {
        let call = format!("{p} {flags}");
        format!("  {:width$}  {what}\n", call.trim_end())
    };
    let mut out = format!("{p} — {}\n\n", spec.about);
    lines.iter().for_each(|l| out.push_str(&line(l)));
    out.push_str("\nOptions (saved in that game's settings):\n");
    out.push_str(&crate::cfg::spec::usage(spec.options, spec.style));
    out.push_str("\nTest options for --play:\n");
    for (flag, value, help) in TEST_OPTIONS {
        let help = test_option(spec, flag).map_or(help, |t| t.help);
        out.push_str(&format!("  {:25}{help}\n", format!("{flag}{value}")));
    }
    out.push('\n');
    out.push_str(&line(&("--help".into(), "this text".into())));
    out
}

fn game_id(spec: &CliSpec, v: &str) -> Result<String, String> {
    spec.games
        .iter()
        .find(|g| **g == v)
        .map(|g| g.to_string())
        .ok_or_else(|| format!("unknown game {v}; the games are {}", spec.games.join(", ")))
}

/// Records the one mode a command line asks for; a second, different one is an error.
fn set_mode(mode: &mut Option<Mode>, m: Mode) -> Result<(), String> {
    match mode {
        Some(old) if *old != m => Err("give one of --list, --check, --play or --set".into()),
        _ => {
            *mode = Some(m);
            Ok(())
        }
    }
}

/// The invocation `args` ask for (program name excluded), or what is
/// wrong with them (exit [`BAD_COMMAND_LINE`]).
pub fn parse(spec: &CliSpec, args: impl IntoIterator<Item = String>) -> Result<Invocation, String> {
    let mut it = args.into_iter().peekable();
    let mut mode = None;
    let mut inv = Invocation {
        mode: Mode::Window,
        libraries: Vec::new(),
        paths: vec![None; spec.paths.len()],
        options: Vec::new(),
        test: Test::default(),
        linux: false,
    };
    while let Some(arg) = it.next() {
        let (flag, inline) = match arg.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f.to_string(), Some(v.to_string())),
            _ => (arg.clone(), None),
        };
        let mut value = |what: &str| -> Result<String, String> {
            inline
                .clone()
                .or_else(|| it.next())
                .ok_or_else(|| format!("{flag} needs {what}"))
        };
        let path_field = spec
            .paths
            .iter()
            .position(|f| f.flag == flag || f.flag_aliases.contains(&flag.as_str()));
        if let Some(n) = path_field {
            let noun = spec.paths[n].kind.noun();
            inv.paths[n] = Some(value(&format!("a {noun}"))?.into());
            continue;
        }
        match flag.as_str() {
            "--help" | "-h" => {
                return Ok(Invocation {
                    mode: Mode::Help,
                    ..inv
                });
            }
            "--library" => inv.libraries.push(value("a folder")?.into()),
            "--list" => set_mode(&mut mode, Mode::List)?,
            "--check" => {
                let game = match it.peek() {
                    Some(next) if !next.starts_with("--") => {
                        Some(game_id(spec, &it.next().unwrap_or_default())?)
                    }
                    _ => None,
                };
                set_mode(&mut mode, Mode::Check(game))?;
            }
            "--play" => set_mode(&mut mode, Mode::Play(game_id(spec, &value("a game")?)?))?,
            "--set" | "--game" => {
                set_mode(&mut mode, Mode::Set(game_id(spec, &value("a game")?)?))?
            }
            "--linux" => inv.linux = true,
            "--headless" => inv.test.headless = true,
            "--no-input" => inv.test.no_input = true,
            "--input-script" => inv.test.script = Some(value("a file")?.into()),
            "--seconds" => {
                inv.test.seconds = Some(
                    value("a number")?
                        .parse()
                        .map_err(|_| "--seconds needs a number".to_string())?,
                )
            }
            f => {
                let option = crate::cfg::spec::active(spec.options)
                    .find(|s| {
                        s.flag_name(spec.style) == f
                            || s.aliases.iter().any(|a| spec.style.flag(a) == f)
                    })
                    .ok_or_else(|| format!("unknown option {f}"))?;
                let v = value(&option.placeholder())?;
                option
                    .parse(&v)
                    .map_err(|_| format!("bad value for {f}: {v} (it takes {})", option.needs()))?;
                inv.options.push((option.key, v));
            }
        }
    }
    inv.mode = match mode {
        Some(Mode::Set(game)) if inv.options.is_empty() => {
            return Err(format!(
                "--set {game} needs options to save, e.g. --set {game} --{} <value>",
                spec.options.first().map_or("option", |o| o.key)
            ));
        }
        Some(m) => m,
        None if !inv.options.is_empty() => {
            return Err("options need --play <game> or --set <game>".into());
        }
        None if !inv.libraries.is_empty() || inv.paths.iter().any(Option::is_some) => {
            Mode::Configure
        }
        None => Mode::Window,
    };
    if let (Mode::Play(game) | Mode::Set(game), Some(offers)) = (&inv.mode, spec.offers)
        && let Some((key, _)) = inv.options.iter().find(|(k, _)| !offers(game, k))
    {
        return Err(format!(
            "{game} doesn't take {}: it has no such setting",
            spec.style.flag(key)
        ));
    }
    if inv.test != Test::default() && !matches!(inv.mode, Mode::Play(_)) {
        return Err("the test options go with --play <game>".into());
    }
    if let Mode::Play(game) = &inv.mode
        && let Some((flag, why)) = given(&inv.test).find_map(|flag| {
            let refuses = test_option(spec, flag)?.refuses?;
            Some((flag, refuses(game)?))
        })
    {
        return Err(format!("{game} doesn't take {flag}: {why}"));
    }
    Ok(inv)
}

fn test_option<'a>(spec: &CliSpec<'a>, flag: &str) -> Option<&'a TestOption<'a>> {
    spec.test_options.iter().find(|t| t.flag == flag)
}

/// The test options `test` was given, by flag.
fn given(test: &Test) -> impl Iterator<Item = &'static str> {
    [
        (test.headless, "--headless"),
        (test.no_input, "--no-input"),
        (test.script.is_some(), "--input-script"),
        (test.seconds.is_some(), "--seconds"),
    ]
    .into_iter()
    .filter_map(|(set, flag)| set.then_some(flag))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cfg::Kind;
    use crate::hub::config::tests::BIOS;

    const OPTIONS: &[OptionSpec] = &[
        OptionSpec::new(
            "upscaler",
            Kind::Choice(&[("fsr", "FSR 1"), ("nis", "NIS")]),
            "Upscaler",
            "How the picture is stretched.",
        )
        .helper_owned(),
        OptionSpec::new(
            "deadzone",
            Kind::Range { min: 0, max: 30 },
            "Dead zone",
            "How far the stick moves first.",
        )
        .with_aliases(&["dead_zone"]),
        OptionSpec::new(
            "look",
            Kind::Choice(&[("original", "Original")]),
            "Look",
            "All at once.",
        ),
    ];
    const SPEC: CliSpec = CliSpec {
        program: "sega-launcher",
        about: "the Sega arcade racers",
        games: &["sr3", "or2sdx"],
        options: OPTIONS,
        style: KeyStyle::Underscore,
        paths: &[BIOS],
        offers: Some(|game, key| !(game == "or2sdx" && key == "upscaler")),
        test_options: &[],
    };

    fn parse_v(v: &[&str]) -> Result<Invocation, String> {
        parse(&SPEC, v.iter().map(|s| s.to_string()))
    }

    #[test]
    fn no_arguments_open_the_window() {
        assert_eq!(parse_v(&[]).unwrap().mode, Mode::Window);
        assert_eq!(parse_v(&["--help"]).unwrap().mode, Mode::Help);
        assert_eq!(parse_v(&["-h"]).unwrap().mode, Mode::Help);
    }

    #[test]
    fn every_option_flag_reaches_its_key_in_order_with_firsts_first() {
        let inv = parse_v(&[
            "--set",
            "sr3",
            "--upscaler",
            "nis",
            "--deadzone=9",
            "--look",
            "original",
            "--dead-zone",
            "3",
        ])
        .unwrap();
        assert_eq!(inv.mode, Mode::Set("sr3".into()));
        let mut seen = vec![];
        inv.apply_options(OPTIONS, &["look"], |spec, v| {
            seen.push((spec.key, format!("{v:?}")))
        });
        assert_eq!(
            seen,
            [
                ("look", "Choice(\"original\")".to_string()),
                ("upscaler", "Choice(\"nis\")".into()),
                ("deadzone", "Number(9)".into()),
                ("deadzone", "Number(3)".into()),
            ]
        );
    }

    #[test]
    fn a_decimal_option_is_checked_on_the_command_line() {
        const STICK: &[OptionSpec] = &[OptionSpec::new(
            "stick_deadzone",
            Kind::Decimal {
                min: 0,
                max: 30,
                places: 2,
            },
            "Dead zone",
            "How far the stick moves first.",
        )];
        let spec = CliSpec {
            options: STICK,
            ..SPEC
        };
        let parse_d = |v: &[&str]| parse(&spec, v.iter().map(|s| s.to_string()));
        assert!(parse_d(&["--stick-deadzone", "0.05", "--play", "sr3"]).is_ok());
        let e = parse_d(&["--stick-deadzone", "0.9", "--play", "sr3"]).unwrap_err();
        assert!(e.contains("bad value for --stick-deadzone: 0.9"), "{e}");
    }

    #[test]
    fn game_is_the_old_name_of_set_and_needs_options() {
        let inv = parse_v(&["--game", "or2sdx", "--deadzone", "4"]).unwrap();
        assert_eq!(inv.mode, Mode::Set("or2sdx".into()));
        let e = parse_v(&["--game", "or2sdx"]).unwrap_err();
        assert!(e.contains("--set or2sdx needs options to save"), "{e}");
    }

    #[test]
    fn bad_command_lines_say_what_is_wrong() {
        for (args, words) in [
            (
                &["--play", "id9"][..],
                "unknown game id9; the games are sr3, or2sdx",
            ),
            (
                &["--deadzone", "333", "--play", "sr3"][..],
                "bad value for --deadzone: 333",
            ),
            (&["--frobnicate"][..], "unknown option --frobnicate"),
            (&["--upscaler", "fsr"][..], "options need --play"),
            (&["--headless"][..], "the test options go with --play"),
            (&["--play"][..], "--play needs a game"),
            (&["--bios"][..], "--bios needs a folder"),
            (&["--list", "--play", "sr3"][..], "give one of"),
            (
                &["--play", "sr3", "--seconds", "x"][..],
                "--seconds needs a number",
            ),
        ] {
            let e = parse_v(args).unwrap_err();
            assert!(e.contains(words), "{args:?}: {e}");
        }
    }

    #[test]
    fn an_option_the_game_doesnt_take_is_refused() {
        let e = parse_v(&["--upscaler", "nis", "--play", "or2sdx"]).unwrap_err();
        assert_eq!(e, "or2sdx doesn't take --upscaler: it has no such setting");
        assert!(parse_v(&["--set", "or2sdx", "--deadzone", "3"]).is_ok());
        assert!(parse_v(&["--set", "sr3", "--upscaler", "nis"]).is_ok());
    }

    #[test]
    fn library_and_paths_alone_configure_and_old_flags_still_work() {
        let inv = parse_v(&["--library", "/games", "--bios", "/bios"]).unwrap();
        assert_eq!(inv.mode, Mode::Configure);
        assert_eq!(inv.paths, [Some(PathBuf::from("/bios"))]);
        assert_eq!(inv.libraries, [PathBuf::from("/games")]);
        assert!(inv.is_action() && inv.no_window() && !inv.is_command());
        let old = parse_v(&["--chihiro-bios=/bios/chihiro"]).unwrap();
        assert_eq!(old.paths, [Some(PathBuf::from("/bios/chihiro"))]);
    }

    #[test]
    fn check_takes_an_optional_game_and_is_a_command() {
        assert_eq!(parse_v(&["--check"]).unwrap().mode, Mode::Check(None));
        let inv = parse_v(&["--check", "or2sdx", "--linux"]).unwrap();
        assert_eq!(inv.mode, Mode::Check(Some("or2sdx".into())));
        assert!(inv.is_command() && inv.linux && !inv.no_window());
    }

    #[test]
    fn play_takes_the_test_options() {
        let inv = parse_v(&[
            "--play",
            "sr3",
            "--headless",
            "--no-input",
            "--input-script",
            "s.txt",
            "--seconds",
            "5",
        ])
        .unwrap();
        assert_eq!(
            inv.test,
            Test {
                headless: true,
                no_input: true,
                script: Some("s.txt".into()),
                seconds: Some(5)
            }
        );
        assert!(inv.no_window() && !inv.is_command());
    }

    #[test]
    fn the_usage_lists_every_game_path_and_option() {
        let u = usage(&SPEC);
        assert!(
            u.starts_with("sega-launcher — the Sega arcade racers\n"),
            "{u}"
        );
        for g in SPEC.games {
            assert!(u.contains(g));
        }
        assert!(u.contains("sega-launcher --bios <folder>") && u.contains("your BIOS folder"));
        for spec in OPTIONS {
            assert!(
                u.contains(&spec.flag_name(KeyStyle::Underscore)),
                "{}",
                spec.key
            );
        }
        assert!(u.contains("--game") && u.contains("--seconds <n>"));
    }

    #[test]
    fn a_launcher_qualifies_a_test_option_and_refuses_it_where_ignored() {
        let spec = CliSpec {
            test_options: &[TestOption {
                flag: "--input-script",
                help: "timed inputs (windowsarcadeloader games)",
                refuses: Some(|game| {
                    (game == "or2sdx").then_some("its loader reads no input lines")
                }),
            }],
            ..SPEC
        };
        let u = usage(&spec);
        assert!(
            u.contains("  --input-script <file>    timed inputs (windowsarcadeloader games)\n"),
            "{u}"
        );
        assert!(u.contains("  --seconds <n>            stop the game after n seconds\n"));
        let parse_t = |v: &[&str]| parse(&spec, v.iter().map(|s| s.to_string()));
        let e = parse_t(&["--play", "or2sdx", "--input-script", "s.txt"]).unwrap_err();
        assert_eq!(
            e,
            "or2sdx doesn't take --input-script: its loader reads no input lines"
        );
        assert!(parse_t(&["--play", "sr3", "--input-script", "s.txt"]).is_ok());
        assert!(parse_t(&["--play", "or2sdx", "--seconds", "5"]).is_ok());
    }
}
