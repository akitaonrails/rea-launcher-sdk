//! A game's picture through gamescope on Linux, and the command line a
//! hub launcher starts there.
//!
//! From the TeknoParrot hub launchers (`settings.rs`, `launch.rs` and
//! `session.rs` of Initial D, Nintendo, Sega and Wangan, which carried four
//! copies): the picture's sizes and upscaler, Initial D's and Nintendo's CRT
//! look (a ReShade effect gamescope loads from a private `HOME`), Sega's
//! [`fit`], the gamescope arguments ([`Picture::args`]; a headless run, for
//! tests in the background, always scales `linear`: FSR and NIS need a
//! display) and the wrapped command line ([`CommandLine`]): gamescope, then
//! bubblewrap binding every library folder read-only so no loader can
//! write to the player's dumps, then `taskset` when the game must stay on
//! some CPUs (Sega's F355, [`crate::cpus`]), then the game.

use std::fmt;
use std::path::{Path, PathBuf};

/// The smallest and largest sizes a setting accepts (for
/// [`crate::cfg::spec::Kind::Size`]).
pub const SIZE_MIN: (u32, u32) = (320, 240);
pub const SIZE_MAX: (u32, u32) = (15360, 8640);

/// A width × height.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Size {
    pub w: u32,
    pub h: u32,
}

impl Size {
    pub const fn new(w: u32, h: u32) -> Size {
        Size { w, h }
    }

    /// `1920x1080` (or `X`, spaces allowed) within [`SIZE_MIN`]..[`SIZE_MAX`].
    pub fn parse(s: &str) -> Option<Size> {
        let (w, h) = s.trim().split_once(['x', 'X'])?;
        let (w, h): (u32, u32) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
        let fits = (SIZE_MIN.0..=SIZE_MAX.0).contains(&w) && (SIZE_MIN.1..=SIZE_MAX.1).contains(&h);
        fits.then_some(Size { w, h })
    }
}

impl fmt::Display for Size {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.w, self.h)
    }
}

/// The largest size with `native`'s shape that fits in `screen`.
pub fn fit(native: Size, screen: Size) -> Size {
    let by_width = Size::new(screen.w, screen.w * native.h / native.w);
    if by_width.h <= screen.h {
        by_width
    } else {
        Size::new(screen.h * native.w / native.h, screen.h)
    }
}

/// How gamescope scales the game's picture to the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Upscaler {
    /// Bilinear, gamescope's default.
    Linear,
    /// AMD FidelityFX Super Resolution 1 (spatial; any GPU).
    Fsr,
    /// NVIDIA Image Scaling (spatial; any GPU).
    Nis,
    /// Sharp pixels.
    Nearest,
}

/// Every upscaler's key and label, in menu order (a
/// [`crate::cfg::spec::Kind::Choice`] table).
pub const UPSCALERS: &[(&str, &str)] = &[
    ("fsr", "FSR 1 (sharp upscale)"),
    ("nis", "NVIDIA Image Scaling"),
    ("linear", "Smooth"),
    ("nearest", "Sharp pixels"),
];

impl Upscaler {
    pub const ALL: [Upscaler; 4] = [
        Upscaler::Fsr,
        Upscaler::Nis,
        Upscaler::Linear,
        Upscaler::Nearest,
    ];

    /// The settings value, which is also gamescope's `-F` filter.
    pub fn key(self) -> &'static str {
        match self {
            Upscaler::Linear => "linear",
            Upscaler::Fsr => "fsr",
            Upscaler::Nis => "nis",
            Upscaler::Nearest => "nearest",
        }
    }

    pub fn label(self) -> &'static str {
        label(UPSCALERS, self.key())
    }

    /// Any case, spaces around ignored.
    pub fn parse(s: &str) -> Option<Upscaler> {
        Self::ALL
            .into_iter()
            .find(|u| u.key().eq_ignore_ascii_case(s.trim()))
    }

    /// FSR and NIS take a sharpness.
    pub fn sharpens(self) -> bool {
        matches!(self, Upscaler::Fsr | Upscaler::Nis)
    }
}

/// A CRT look drawn over the game by gamescope (a ReShade effect).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Crt {
    Off,
    /// Timothy Lottes' CRT shader: scanlines, shadow mask, slight curvature.
    Lottes,
}

/// Every CRT look's key and label, in menu order.
pub const CRTS: &[(&str, &str)] = &[("off", "Off"), ("lottes", "CRT (scanlines and mask)")];

impl Crt {
    pub const ALL: [Crt; 2] = [Crt::Off, Crt::Lottes];

    pub fn key(self) -> &'static str {
        match self {
            Crt::Off => "off",
            Crt::Lottes => "lottes",
        }
    }

    pub fn label(self) -> &'static str {
        label(CRTS, self.key())
    }

    /// Any case, spaces around ignored.
    pub fn parse(s: &str) -> Option<Crt> {
        Self::ALL
            .into_iter()
            .find(|c| c.key().eq_ignore_ascii_case(s.trim()))
    }

    /// The effect's file name, in the launcher's shaders and in
    /// gamescope's effect folder ([`effect_link`]).
    pub fn effect(self) -> Option<&'static str> {
        match self {
            Crt::Off => None,
            Crt::Lottes => Some("crt-lottes.fx"),
        }
    }
}

fn label(table: &[(&str, &'static str)], key: &str) -> &'static str {
    table.iter().find(|(k, _)| *k == key).map_or("", |(_, l)| l)
}

/// The `HOME` gamescope runs with when it loads an effect: gamescope looks
/// effects up by name only, under `$HOME/.local/share/gamescope/reshade/
/// Shaders` (it ignores `XDG_DATA_HOME` and absolute paths), so the game's
/// runtime folder holds such a home. The game gets the player's own `HOME`
/// back ([`CommandLine::homes`]).
pub fn effect_home(runtime: &Path) -> PathBuf {
    runtime.join("gamescope-home")
}

/// The link to make for `effect` in `home` ([`effect_home`]): (where the
/// link goes, the launcher's shader it points at).
pub fn effect_link(home: &Path, shaders: &Path, effect: &str) -> (PathBuf, PathBuf) {
    (
        home.join(".local/share/gamescope/reshade/Shaders")
            .join(effect),
        shaders.join(effect),
    )
}

/// What gamescope shows: the game's own size, the size on screen and how
/// one becomes the other.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Picture {
    /// The size the game draws at.
    pub game: Size,
    /// The size gamescope shows on the screen.
    pub output: Size,
    pub upscaler: Upscaler,
    /// 0–20, FSR's and NIS's sharpness (0 = sharpest).
    pub sharpness: u8,
    pub fullscreen: bool,
    pub crt: Crt,
}

impl Picture {
    /// gamescope's arguments (before `--`). A headless run scales
    /// `linear`, with no sharpness, and has no full screen.
    pub fn args(&self, headless: bool) -> Vec<String> {
        let filter = if headless {
            Upscaler::Linear
        } else {
            self.upscaler
        };
        let mut a: Vec<String> = [
            "-w",
            &self.game.w.to_string(),
            "-h",
            &self.game.h.to_string(),
            "-W",
            &self.output.w.to_string(),
            "-H",
            &self.output.h.to_string(),
            "-r",
            "60",
            "-F",
            filter.key(),
        ]
        .map(String::from)
        .into();
        if filter.sharpens() {
            a.extend(["--sharpness".into(), self.sharpness.to_string()]);
        }
        if let Some(effect) = self.crt.effect() {
            a.extend(["--reshade-effect".into(), effect.into()]);
        }
        if headless {
            a.extend(["--backend".into(), "headless".into()]);
        } else if self.fullscreen {
            a.push("-f".into());
        }
        a
    }
}

/// The Linux command line around a game.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CommandLine {
    /// gamescope's arguments ([`Picture::args`]).
    pub gamescope: Vec<String>,
    /// gamescope's `HOME` ([`effect_home`]) and the player's, which the
    /// game gets back; `None`: both keep the launcher's.
    pub homes: Option<(PathBuf, String)>,
    /// Folders bubblewrap binds read-only around the game, each only if
    /// it is there (an unplugged drive in the library list doesn't stop
    /// the game); `None`: no bubblewrap (it is not installed).
    pub read_only: Option<Vec<PathBuf>>,
    /// Folders the session writes (the launcher's data, runtime, prefix):
    /// one inside a read-only folder (`--library ~`) is bound back
    /// writable after them, so it must exist before the start.
    pub writable: Vec<PathBuf>,
    /// `taskset -c` CPU list (`8,9,10,11`).
    pub cpus: Option<String>,
}

impl CommandLine {
    /// gamescope `ARGS -- [env HOME=…] [bwrap …] [taskset -c …] program args`.
    pub fn argv(&self, program: &Path, args: &[String]) -> Vec<String> {
        let mut v = Vec::new();
        if let Some((home, _)) = &self.homes {
            v.extend(["env".into(), format!("HOME={}", home.display())]);
        }
        v.push("gamescope".into());
        v.extend(self.gamescope.iter().cloned());
        v.push("--".into());
        if let Some((_, player)) = &self.homes {
            v.extend(["env".into(), format!("HOME={player}")]);
        }
        if let Some(folders) = &self.read_only {
            v.extend(["bwrap", "--dev-bind", "/", "/"].map(String::from));
            let mut bind = |how: &str, f: &Path| {
                let f = f.to_string_lossy().into_owned();
                v.extend([how.into(), f.clone(), f]);
            };
            for f in folders {
                bind("--ro-bind-try", f);
            }
            for w in &self.writable {
                if folders.iter().any(|f| w.starts_with(f)) {
                    bind("--bind", w);
                }
            }
        }
        if let Some(cpus) = &self.cpus {
            v.extend(["taskset".into(), "-c".into(), cpus.clone()]);
        }
        v.push(program.to_string_lossy().into_owned());
        v.extend(args.iter().cloned());
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture() -> Picture {
        Picture {
            game: Size::new(1360, 768),
            output: Size::new(3840, 2160),
            upscaler: Upscaler::Fsr,
            sharpness: 5,
            fullscreen: true,
            crt: Crt::Off,
        }
    }

    fn after(args: &[String], flag: &str) -> Option<String> {
        let i = args.iter().position(|a| a == flag)?;
        args.get(i + 1).cloned()
    }

    #[test]
    fn sizes_parse_within_their_bounds_and_print_back() {
        assert_eq!(Size::parse(" 1920 X 1080 "), Some(Size::new(1920, 1080)));
        assert_eq!(Size::new(640, 480).to_string(), "640x480");
        for bad in ["319x240", "320x239", "15361x8640", "1920", "axb", ""] {
            assert_eq!(Size::parse(bad), None, "{bad}");
        }
        assert_eq!(Size::parse("15360x8640"), Some(Size::new(15360, 8640)));
    }

    #[test]
    fn fit_keeps_the_games_shape_inside_the_screen() {
        let deck = Size::new(1280, 800);
        assert_eq!(fit(Size::new(1360, 768), deck), Size::new(1280, 722));
        assert_eq!(
            fit(Size::new(640, 480), Size::new(1920, 1080)),
            Size::new(1440, 1080)
        );
        assert_eq!(
            fit(Size::new(1920, 1080), Size::new(3840, 2160)),
            Size::new(3840, 2160)
        );
    }

    #[test]
    fn choices_parse_their_keys_in_any_case_and_have_labels() {
        for u in Upscaler::ALL {
            assert_eq!(
                Upscaler::parse(&format!(" {} ", u.key().to_uppercase())),
                Some(u)
            );
            assert!(!u.label().is_empty());
        }
        let keys: Vec<_> = Upscaler::ALL.iter().map(|u| u.key()).collect();
        let table: Vec<_> = UPSCALERS.iter().map(|(k, _)| *k).collect();
        assert_eq!(keys, table);
        assert_eq!(Upscaler::Linear.label(), "Smooth");
        assert_eq!(Upscaler::parse("bicubic"), None);
        for c in Crt::ALL {
            assert_eq!(Crt::parse(c.key()), Some(c));
        }
        assert_eq!(Crt::Lottes.label(), "CRT (scanlines and mask)");
        assert_eq!(Crt::Off.effect(), None);
    }

    #[test]
    fn the_upscaler_sharpness_and_full_screen_reach_gamescope() {
        let a = picture().args(false);
        assert_eq!(
            a.join(" "),
            "-w 1360 -h 768 -W 3840 -H 2160 -r 60 -F fsr --sharpness 5 -f"
        );
        let smooth = Picture {
            upscaler: Upscaler::Linear,
            fullscreen: false,
            ..picture()
        };
        let a = smooth.args(false);
        assert_eq!(after(&a, "-F").as_deref(), Some("linear"));
        assert!(!a.contains(&"--sharpness".into()) && !a.contains(&"-f".into()));
        assert_eq!(
            after(
                &Picture {
                    upscaler: Upscaler::Nis,
                    ..picture()
                }
                .args(false),
                "--sharpness"
            )
            .as_deref(),
            Some("5")
        );
    }

    #[test]
    fn a_headless_run_scales_linear_without_a_screen() {
        let a = picture().args(true);
        assert_eq!(after(&a, "-F").as_deref(), Some("linear"));
        assert_eq!(after(&a, "--backend").as_deref(), Some("headless"));
        assert!(!a.contains(&"--sharpness".into()) && !a.contains(&"-f".into()));
    }

    #[test]
    fn the_crt_look_loads_its_effect_from_the_runtimes_home() {
        let crt = Picture {
            crt: Crt::Lottes,
            ..picture()
        };
        for headless in [false, true] {
            let a = crt.args(headless);
            assert_eq!(
                after(&a, "--reshade-effect").as_deref(),
                Some("crt-lottes.fx")
            );
        }
        let home = effect_home(Path::new("/d/rt/id6"));
        assert_eq!(home, Path::new("/d/rt/id6/gamescope-home"));
        assert_eq!(
            effect_link(&home, Path::new("/c/shaders"), "crt-lottes.fx"),
            (
                PathBuf::from(
                    "/d/rt/id6/gamescope-home/.local/share/gamescope/reshade/Shaders/crt-lottes.fx"
                ),
                PathBuf::from("/c/shaders/crt-lottes.fx")
            )
        );
    }

    #[test]
    fn the_library_is_bound_read_only_around_the_game() {
        let program = Path::new("/c/wal/arcade-launcher");
        let args = ["run".to_string(), "/d/rt".into()];
        let mut line = CommandLine {
            gamescope: picture().args(false),
            homes: None,
            read_only: Some(vec!["/lib".into(), "/naomi".into()]),
            writable: vec!["/d/rt".into()],
            cpus: None,
        };
        let s = line.argv(program, &args).join(" ");
        assert!(s.starts_with("gamescope -w 1360 -h 768 "), "{s}");
        assert!(
            s.ends_with(" -f -- bwrap --dev-bind / / --ro-bind-try /lib /lib --ro-bind-try /naomi /naomi /c/wal/arcade-launcher run /d/rt"),
            "{s}"
        );
        line.read_only = None;
        assert!(!line.argv(program, &args).join(" ").contains("bwrap"));
        // F355's boards on one cache domain
        line.cpus = Some("8,9,10,11".into());
        let s = line.argv(program, &args).join(" ");
        assert!(
            s.contains(" -- taskset -c 8,9,10,11 /c/wal/arcade-launcher run "),
            "{s}"
        );
        // the CRT effect: gamescope with the runtime's HOME, the game with the player's
        let line = CommandLine {
            homes: Some(("/d/rt/gamescope-home".into(), "/home/p".into())),
            read_only: Some(vec!["/lib".into()]),
            ..line
        };
        let s = line.argv(program, &args).join(" ");
        assert!(
            s.starts_with("env HOME=/d/rt/gamescope-home gamescope "),
            "{s}"
        );
        assert!(s.contains(" -- env HOME=/home/p bwrap "), "{s}");
    }

    #[test]
    fn a_writable_folder_inside_a_read_only_one_is_bound_back() {
        // `--library ~`: the launcher's data folder is inside it
        let line = CommandLine {
            read_only: Some(vec!["/home/p".into(), "/gone".into()]),
            writable: vec!["/home/p/.local/share/rea".into(), "/d/rt".into()],
            ..CommandLine::default()
        };
        let s = line.argv(Path::new("game"), &[]).join(" ");
        assert!(
            s.ends_with(
                " -- bwrap --dev-bind / / --ro-bind-try /home/p /home/p --ro-bind-try /gone /gone \
                 --bind /home/p/.local/share/rea /home/p/.local/share/rea game"
            ),
            "{s}"
        );
        // without bubblewrap, nothing to bind back
        let line = CommandLine {
            read_only: None,
            ..line
        };
        assert!(!line.argv(Path::new("game"), &[]).join(" ").contains("bind"));
    }

    /// The real bubblewrap, when it is installed (STANDARDS "Test
    /// behaviour"): a missing library folder doesn't stop the game, a
    /// read-only one can't be written, the session's folder inside it can.
    #[test]
    #[cfg(target_os = "linux")]
    fn bubblewrap_runs_with_a_missing_folder_and_a_writable_one_inside() {
        let t = crate::test_dir("gamescope-bwrap");
        let (lib, rw) = (t.join("lib"), t.join("lib/data"));
        std::fs::create_dir_all(&rw).unwrap();
        let line = CommandLine {
            read_only: Some(vec![t.join("unplugged"), lib.clone()]),
            writable: vec![rw.clone()],
            ..CommandLine::default()
        };
        let script = format!(
            "touch '{}/saved' && ! touch '{}/dump' 2>/dev/null",
            rw.display(),
            lib.display()
        );
        let argv = line.argv(Path::new("sh"), &["-c".into(), script]);
        let bwrap = &argv[argv.iter().position(|a| a == "--").unwrap() + 1..];
        match std::process::Command::new(&bwrap[0])
            .args(&bwrap[1..])
            .output()
        {
            Ok(o) => assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr)),
            Err(_) => eprintln!("bwrap not installed: skipped"),
        }
        if rw.join("saved").exists() {
            assert!(!lib.join("dump").exists());
        }
        std::fs::remove_dir_all(t).unwrap();
    }
}
