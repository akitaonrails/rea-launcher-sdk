# rea-launcher

The portable core of the rea retro-game launchers: pure Rust plus `sha2`,
no GUI, no Win32 calls, `#![forbid(unsafe_code)]`. Every module is tested
on a Linux host. Game knowledge (which files are static or game-written,
edition tables, patch sites, ini keys) is passed in as data or as a trait
implementation.

Each module's doc comment names the launcher it came from and the bug that
made that copy the reference.

| Module | What it does |
|---|---|
| `fsx` | Whole-file swaps (temp + rename) that never write through a hard link; stale temp cleanup |
| `paths` | `resolve`, the runtime/game overlap check, `native`, `\\?\` stripping; `data_dir`, a portable-first data folder |
| `runtime` | The private runtime copy: list, plan, sync (with progress), restore, manifest |
| `saves` | Save mirroring by recorded state, for one file or a folder |
| `ledger` | SHA-256 record of the components the launcher installed |
| `ini` | Section-aware INI editing that keeps comments, case, CRLF, BOM, bytes |
| `cfg` | `launcher.cfg` reading, line editing, value parsers, shared CLI flags; `cfg::spec` option tables |
| `wine` | `WINEDLLOVERRIDES` merge, Wine detection from the environment, winebus names |
| `download` | Pinned, resumable, verified downloads; unpacking with the `zip` feature |
| `env` | Child environments and the `CreateProcessW` environment block |
| `identify` | SHA-256 tables, hex, PE32 check |
| `installer` | A game's Inno Setup installer instead of an installed game: `detect` by SHA-256 and part sizes, `begin`/`finish`/`abandon` into a folder of the launcher's own (`begin_again` when a marked folder fails the game check: unpacked again), collisions (`prefer`), repack files the launcher bundles (`drop`), the `.rea-unpacked` marker; `UnpackJob` for the progress window (docs/playbooks/installers.md) |
| `patch` | Declarative patch sets (`PatchSet`, `apply_sets`, `describe`), byte sites, signatures, caves, result hashes |
| `pe` | PE32/PE32+ headers and sections, VA ↔ file offset |
| `display` | Monitor choice, landscape sizes, Steam Deck, the largest 16:9 size |
| `pad` | XInput state parsing and settings-window navigation by edges; `PadNav::with_repeat` for list menus |
| `wheel` | Wheels, pedals and pads as racing controls: known wheels, `classify`, spare interfaces, `pick_wheel`, steering lock, curves, pedals, `WheelProfile<B>` (`wheel.ini`), the setup `wizard`, `pad_map`; `dinput` (the in-game DirectInput layer's `rea_wheel.ini`, `HEADER`; `dinput::options`: the wheel settings as launcher options, `OPTIONS`, `Setup::apply`/`value`/`to_launcher_cfg`) and `rig` (the devices behind its virtual wheel: `pick`, `profile`, `virtual_controls`, `shows_virtual_wheel`, `calibrate`, `save`, `WheelDevices`) (docs/playbooks/wheels-and-ffb.md) |
| `ffb` | Synthetic force feedback from a normalised `CarState` (`SyntheticFfb`, `Tuning::ARCADE`), the menu's `test_pattern` |
| `log` | `launcher.log` with a size cap; opt-in `per_run` (previous run kept as `.old`) and `timestamps` |
| `args` | Windows command-line quoting |
| `instance` | One launcher at a time without the play helper (native Linux): `instance::single`, a `flock` |
| `testing` | Golden snapshots for launcher tests: `snapshot!`, version as `<version>`, CRLF-proof |

## Examples

### fsx

```rust
use rea_launcher::fsx;
# fn main() -> std::io::Result<()> {
# let runtime = std::env::temp_dir().join("rea-doc-fsx");
// The runtime file may be a hard link to the player's original: this
// replaces the link, never the original.
fsx::install(&runtime.join("dgVoodoo.conf"), "[General]\n")?;
fsx::write_atomic(&runtime.join(".manifest"), "flushed before the rename\n")?;
# std::fs::remove_dir_all(runtime)?;
# Ok(()) }
```

### paths

```rust
use rea_launcher::paths;
# fn main() -> Result<(), String> {
# let game = std::env::temp_dir();
// A runtime folder inside the game folder is refused, even before it
// exists or when it is spelled through `..` or a symlink.
assert!(paths::check_separate(&game, &game.join("runtime")).is_err());
# Ok(()) }
```

### runtime

```rust
use rea_launcher::runtime::{self, Policy, Stamp};

struct Cmr3;

impl Policy for Cmr3 {
    fn link(&self, rel: &str) -> bool {
        rel.starts_with("Data/") || rel.ends_with(".exe")
    }
    fn game_written(&self, rel: &str, _source: Stamp) -> bool {
        rel.eq_ignore_ascii_case("CMR3")
    }
}

# fn main() -> std::io::Result<()> {
# let (game, rt) = (std::env::temp_dir().join("rea-doc-game"), std::env::temp_dir().join("rea-doc-rt"));
# std::fs::create_dir_all(&game)?;
let report = runtime::sync(&game, &rt, &Cmr3, ".cmr3-manifest")?;
println!("{} brought, {} linked", report.brought, report.linked);
for warning in &report.warnings {
    eprintln!("{warning}");
}
# std::fs::remove_dir_all(game)?; std::fs::remove_dir_all(rt)?;
# Ok(()) }
```

### saves

```rust
use rea_launcher::saves::{FirstRun, Mirror};
# fn main() -> std::io::Result<()> {
# let (rt, saves) = (std::env::temp_dir().join("rea-doc-rt2"), std::env::temp_dir().join("rea-doc-saves"));
# std::fs::create_dir_all(&rt)?;
let state = rt.join(".cmr3-profile-state");
let (rt_profile, saved) = (rt.join("CMR3"), saves.join("CMR3"));
let mirror = Mirror::file(&rt_profile, &saved, &state, FirstRun::NewerWins);
let moved = mirror.before()?; // a changed saves copy wins, else a changed runtime copy
// … the game runs …
let copied = mirror.after()?; // runtime → saves where the bytes differ
# let _ = (moved, copied); std::fs::remove_dir_all(rt)?; let _ = std::fs::remove_dir_all(saves);
# Ok(()) }
```

`Mirror::folder(…)` does the same for every file below a folder;
`.scan(&["", "CONFIG"])` looks only directly inside some subfolders,
`.only(&filter)` limits it to some names, and `.deleted(Deleted::Stays)`
leaves a save deleted on one side gone. Names match without regard to
case between the two sides.

### ledger

```rust
use rea_launcher::ledger::{Ledger, Outcome};
# fn main() -> std::io::Result<()> {
# let (game, rt) = (std::env::temp_dir().join("rea-doc-lg"), std::env::temp_dir().join("rea-doc-lr"));
# std::fs::create_dir_all(&game)?; std::fs::create_dir_all(&rt)?;
# let xidi: &[u8] = b"xidi";
let mut ledger = Ledger::load(&game, &rt, ".cmr3-launcher-components");
// Refused (nothing written) over a player's own dinput8.dll.
if ledger.install("dinput8.dll", xidi, &[])? == Outcome::NotOurs {
    eprintln!("dinput8.dll is not ours; Xidi left out");
}
// Off: the game's own file comes back; a player's file stays.
ledger.take_out("dinput8.dll", &[xidi])?;
# std::fs::remove_dir_all(game)?; std::fs::remove_dir_all(rt)?;
# Ok(()) }
```

After a component update, a runtime from before the ledger may still hold
the old version's file. `known_with_earlier` adds it to `known` when its
SHA-256 is one an earlier release installed, so the update replaces it
instead of refusing it as the player's:

```rust
# use rea_launcher::ledger::Ledger;
# fn main() -> std::io::Result<()> {
# let (game, rt) = (std::env::temp_dir().join("rea-doc-eg"), std::env::temp_dir().join("rea-doc-er"));
# std::fs::create_dir_all(&game)?; std::fs::create_dir_all(&rt)?;
# let xidi5: &[u8] = b"xidi 5";
/// Xidi 4.3.1's Win32 dinput8.dll, installed by releases up to 0.3.0.
const EARLIER: &[&str] = &["dedbf4aa0d1cc3b0c036feb1a089d181345c03ef07a118886b66654f04395efd"];
let mut ledger = Ledger::load(&game, &rt, ".cmr3-launcher-components");
let known = ledger.known_with_earlier("dinput8.dll", vec![xidi5.to_vec()], EARLIER)?;
let known: Vec<&[u8]> = known.iter().map(Vec::as_slice).collect();
ledger.install("dinput8.dll", xidi5, &known)?;
# std::fs::remove_dir_all(game)?; std::fs::remove_dir_all(rt)?;
# Ok(()) }
```

### ini

`ini::set_owned` / `ini::release` with an `ini::Owned` record (saved
beside the runtime) undo only values the launcher wrote and the player
has not changed since in the game's menu.

```rust
use rea_launcher::ini::Ini;
let bytes = b"[GameController]\r\nName=Manette \xE9tendue\r\n";
let mut ini = Ini::from_bytes(bytes).tight(); // not UTF-8: byte-faithful, `key=value`
ini.set("Graphics", "VSync", "1");
assert!(ini.to_bytes().starts_with(bytes));
assert!(ini.to_text().is_none()); // only to_bytes can write this file back
assert_eq!(ini.get("graphics", "vsync"), Some("1"));
```

### cfg

```rust
use rea_launcher::cfg::{self, Rejected};

#[derive(Default)]
struct Settings { vsync: Option<bool> }

impl Settings {
    fn apply(&mut self, key: &str, value: &str) -> Result<(), Rejected> {
        match key {
            "vsync" => self.vsync = Some(cfg::need(cfg::parse_bool(value), cfg::ON_OFF)?),
            _ => return Err(Rejected::Unknown),
        }
        Ok(())
    }
}

let mut s = Settings::default();
let warnings = cfg::parse("vsync = on\n", "launcher.cfg", |k, v| s.apply(k, v));
assert!(warnings.is_empty());
// The same apply serves `--vsync off`.
let switches = cfg::parse_args(
    ["--no-window".into(), "--vsync".into(), "off".into()],
    &["vsync"],
    &["--no-window"],
    |k, v| s.apply(k, v),
)
.unwrap();
assert_eq!((s.vsync, switches), (Some(false), vec!["--no-window".to_string()]));
// Only the launcher's own line changes; comments stay.
assert_eq!(cfg::set_key("# Picture\n#vsync =\n", "vsync", "on"), "# Picture\nvsync = on\n");
```

### wine

```rust
use rea_launcher::wine;
let merged = wine::merge_dll_overrides("dinput8,d3d9=n,b", Some("D3D9.dll=b;xinput1_3=n"));
assert_eq!(merged, "dinput8,d3d9=n,b;xinput1_3=n");
// Or straight from the environment the game inherits (any name case):
let env: Vec<_> = std::env::vars_os().collect();
let for_game = wine::dll_overrides_for("d3d9=n,b", &env);
# let _ = for_game;
let seed = wine::SDL_ONLY; // under wine::WINEBUS_KEY, only where absent
# let _ = (seed, wine::is_wine_env());
```

### env

```rust
use rea_launcher::env;
let vars = env::merge_env(std::env::vars_os(), &[("WINEDLLOVERRIDES".into(), Some("d3d9=n,b".into()))]);
let block: Vec<u16> = env::environment_block(&vars); // for CreateProcessW
# let _ = block;
```

### identify

```rust
use rea_launcher::identify::{self, Known};

static EDITIONS: [Known<&str>; 1] = [Known::new(
    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    "empty test build",
)];

match identify::identify(b"some exe", &EDITIONS) {
    Ok(name) => println!("{name}"),
    Err(digest) => println!("unknown build, SHA-256 {}", identify::hex(&digest)),
}
```

### pe::edit (a runtime copy's sections and imports)

`pe::edit::add_section` appends a section (a fix's code and data at a fixed
address) and `pe::edit::add_import` adds a DLL to the import table, its
address table optionally inside that section, so the loader maps a hook
DLL and fills in its functions before the game runs. Only for the runtime
copy. It replaces starting the game suspended and injecting
(`VirtualAllocEx`/`WriteProcessMemory`/`CreateRemoteThread`), which
antivirus heuristics flag. `pe::edit::fix_base` clears `DYNAMIC_BASE` so a
fix section's absolute addresses hold; `pe::edit::imports` reads the table
back. Bound imports are unbound; a stale binding (descriptors stamped
bound with no bound import directory, the address table still holding
names, as in SEGA Rally Revo's exes) is cleared too, and only a genuinely
bound descriptor without a name table is refused (`BoundWithoutNames`).
`REA_SRR_GAME_DIR=<folder>` runs the test on the owner's SEGA Rally Revo
exes, read only.

```rust
use rea_launcher::pe::edit;
# fn demo(game: &[u8]) -> Result<Vec<u8>, edit::EditError> {
let (with_fix, fix) = edit::add_section(game, ".rea", &[0u8; 0x400], edit::SCN_CODE_RWX)?;
// The loader writes hooks.dll's two functions at fix.rva + 0x100.
edit::add_import(&with_fix, ".reaimp", "hooks.dll", &["one", "two"], Some(fix.rva + 0x100))
# }
```

### patch

```rust
use rea_launcher::patch::{self, Site};
let mut exe = vec![0x90, 0x55, 0x8b];
let sites = [Site { name: "hook", offset: 1, expected: &[0x55, 0x8b], patched: &[0xc3, 0x90] }];
assert_eq!(patch::apply(&mut exe, &sites), Ok(1));
assert_eq!(patch::apply(&mut exe, &sites), Ok(0)); // idempotent
```

### display

```rust
use rea_launcher::display::{self, Monitor};
let monitors = [
    Monitor { width: 2160, height: 3840, primary: true },
    Monitor { width: 3840, height: 2160, primary: false },
];
assert_eq!(display::game_monitor(&monitors), Some(1)); // None: keep the primary
assert!(display::is_steam_deck(Some((800, 1280))));
assert_eq!(display::largest_16_9(Some((1280, 800)), (3840, 2160), (1920, 1080)), (1280, 720));
```

### pad

```rust
use rea_launcher::pad::{self, Nav, PadNav};
let mut nav = PadNav::default();
assert_eq!(nav.update(pad::DPAD_DOWN | pad::A), [Nav::Down, Nav::Press]);
assert_eq!(nav.update(pad::DPAD_DOWN | pad::A), []); // held: nothing more
```

### log

```rust
use rea_launcher::log::Log;
let log = Log::new(std::env::temp_dir().join("rea-doc-launcher.log"));
log.warn("no window could be made; playing with the saved settings");
# let _ = std::fs::remove_file(log.path());
```

### args

```rust
use rea_launcher::args;
use std::ffi::OsStr;
assert_eq!(args::quote_windows(OsStr::new(r"C:\Games\")), r#""C:\Games\\""#);
assert_eq!(args::command_line(OsStr::new("a.exe"), &["-d"]), r#""a.exe" "-d""#);
```

### Golden snapshots (testing)

Every launcher's `tests/snapshots.rs` compares `--help`, the
`launcher.cfg` template and each look's plan with files under
`tests/snapshots/` (docs/STANDARDS.md). Use the SDK's helper, not a local
`check()`: a help text that printed the crate version broke the tag builds
of CMR3, CMR4 and OR when the release commit bumped it.

```rust,no_run
# fn usage() -> String { String::new() }
#[test]
fn help() {
    // tests/snapshots/help.txt; this crate's version is stored as <version>.
    rea_launcher::snapshot!("help.txt", usage());
}
```

- `REA_UPDATE_SNAPSHOTS=1 cargo test` writes them; review the diff.
  `rea_launcher::snapshots!().update_var("CMR3_UPDATE_SNAPSHOTS")` keeps
  a launcher's documented variable working too.
- The caller's `CARGO_PKG_VERSION` becomes `<version>` (whole tokens only:
  `10.3.3` stays). A stored snapshot that still holds the version fails
  with "would fail at the next version bump".
- CRLF and LF compare equal; a failure names the first differing line and
  says when only version numbers differ.

### Copy progress (runtime)

`runtime::sync_with_progress` calls back with `Progress { files_done,
files_total, bytes_done, bytes_total }` once the plan is known and after
each file; `ProgressCounter` hands it from the worker thread to the
window. No second thread measuring the runtime (DiRT Rally's 40 GB copy).

### instance (native Linux)

```rust,no_run
let _instance = match rea_launcher::instance::single("rea-dirt-rally") {
    Ok(guard) => guard,
    Err(running) => {
        eprintln!("{running}"); // "the launcher is already running"
        std::process::exit(running.exit_code()); // ALREADY_RUNNING = 5
    }
};
```

The same `Result<Guard, AlreadyRunning>` as `rea_launcher_win32::instance`
(use that one on Windows: it can bring the window forward), and the same
exit code, `instance::ALREADY_RUNNING` (5), which `rea-play.sh` reads as
"nothing started". The lock is
`$XDG_RUNTIME_DIR/rea-launcher-<name>.lock`; the kernel drops it when the
process ends, so a killed launcher leaves no stale lock.

### wheel and ffb (a launcher that owns the input)

The launcher's buttons implement `wheel::Control` (key, label, bit); the
module's own docs have a full example. A frame of an emulator wrapper:

```rust,no_run
use rea_launcher::ffb::{CarState, FfbSettings, SyntheticFfb, Tuning};
use rea_launcher::wheel::{self, Control, DeviceState, WheelProfile, WheelSettings};
# fn frame<B: Control>(profile: &WheelProfile<B>, wheel: &DeviceState, pedals: Option<&DeviceState>,
#                      pads: &[(wheel::PadSettings<B>, wheel::PadState)], car: Option<CarState>,
#                      ffb: &mut SyntheticFfb) {
let settings = WheelSettings::default(); // 270° lock
let mut controls = profile.read(&settings, wheel, pedals);
for (pad_settings, pad) in pads {
    controls = controls.merge(wheel::read_pad(pad_settings, pad, None));
}
// …hand `controls` to the emulator, then the force for this frame:
let force = ffb.update(0.004, controls.steer, car, &FfbSettings::default());
# let _ = force; }
# let _ = SyntheticFfb::new(Tuning::ARCADE);
```

### data_dir and a per-run log

```rust,no_run
use rea_launcher::{log::Log, paths};
# let exe_dir = std::path::Path::new(".");
let names = paths::DataDir { portable: "data", windows: "RadMobile", xdg: "rad-mobile" };
let data = paths::data_dir(exe_dir, None, names, &|k| std::env::var_os(k));
// The first line is always this run's; the previous run is launcher.log.old.
let log = Log::new(data.join("launcher.log")).per_run().timestamps(true);
log.line(concat!("Rad Mobile launcher ", env!("CARGO_PKG_VERSION")));
```

## One table for the window and the command line

The rule: every window option has a flag and a `launcher.cfg` key, every
flag that changes the game has a window control, the game runs fully with
`--no-window`, and the window can set everything. `cfg::spec` keeps the
rows in one `const` table; the launcher keeps its typed settings and an
`apply(key, Value)`.

```rust
use rea_launcher::cfg::{self, spec, KeyStyle, Kind, OptionSpec, Rejected, Value};

const MODES: &[(&str, &str)] = &[("fullscreen", "Fullscreen"), ("windowed", "Windowed")];

const OPTIONS: &[OptionSpec] = &[
    OptionSpec::new("game", Kind::Path, "Game folder", "The installed game; only read.")
        .launcher_only(),
    OptionSpec::new(
        "resolution",
        Kind::Size { min: (640, 480), max: (7680, 4320), aliases: cfg::ORIGINAL },
        "Resolution",
        "Display resolution, or original.",
    )
    .with_example("1920x1080"),
    OptionSpec::new("display_mode", Kind::Choice(MODES), "Display mode", "How the game fills the screen."),
    OptionSpec::new("vsync", Kind::Toggle, "VSync", "Wait for the screen's refresh."),
];

#[derive(Default)]
struct Settings { mode: Option<&'static str>, vsync: Option<bool> }

impl Settings {
    fn apply(&mut self, key: &str, value: Value) -> Result<(), Rejected> {
        match (key, value) {
            ("display_mode", Value::Choice(mode)) => self.mode = Some(mode),
            ("vsync", Value::Toggle(on)) => self.vsync = Some(on),
            ("game" | "resolution", _) => {}
            _ => return Err(Rejected::Unknown),
        }
        Ok(())
    }
}

// launcher.cfg and the command line, checked against the same rows.
let mut s = Settings::default();
let warnings = spec::parse_file("vsync = on\n", "launcher.cfg", OPTIONS, |k, v| s.apply(k, v));
let argv = ["--no-window", "--display-mode", "windowed"].map(String::from);
let switches = spec::parse_args(argv, KeyStyle::Underscore, OPTIONS, &["--no-window"], |k, v| s.apply(k, v));
assert!(warnings.is_empty() && switches.is_ok());
assert_eq!((s.mode, s.vsync), (Some("windowed"), Some(true)));

// --help lines and the first-run template come from the rows.
let help = spec::usage(OPTIONS, KeyStyle::Underscore);
let template = spec::template("Example launcher settings.", OPTIONS);
assert!(help.contains("--display-mode fullscreen|windowed"));
assert!(template.contains("#resolution = 1920x1080"));

// In the launcher's tests: the window's controls match the table.
spec::assert_parity(OPTIONS, &["resolution", "display_mode", "vsync"]);
```

`launcher_only` marks rows with no window control on purpose (the game
and runtime folders, a log switch for bug reports). `assert_parity`
panics with every row the window lacks, every window control without a
row, duplicate keys or flags, empty choices and invalid examples.
`OptionSpec::parse` checks one value, for a window's own text fields. A renamed key keeps its old name readable with
`.with_aliases(&["old-name"])` (file and flags, no warning);
`spec::set_key(text, &row, value)` writes the new name and comments out
the old lines. `.helper_owned()` marks an option the Linux play
helper reads rather than the game (gamescope scaler, vkBasalt): it still
has a key and a window control, `usage` marks it "(Linux helper)", and the
helper must honour the one-run flag passed to it as well as
`launcher.cfg`. `.with_needs("10–240 or unlimited")` and
`.with_placeholder("<program>")` replace the generated texts;
`.with_value_aliases(&[("off", "0")])` reads other spellings (a number
outside a `Range` named this way is a special value, listed in `needs`).
`OptionSpec::removed("widescreen", "why")` keeps an old key harmless:
ignored when read, left out of help and the window, and commented out by
`spec::retire_removed` on save. `spec::usage_with` adds the launcher's
value-less switches to the help column.

## Binary patches as one table

A game's fixes are a `const` table of `PatchSet`s (see
`docs/playbooks/binary-patching.md`). `apply_sets` identifies the build by
SHA-256, resolves virtual addresses through the PE sections, refuses a site
holding other bytes (all or nothing), accepts sites already patched,
checks caves (zero, executable, not shared) and verifies each set's
result hash. `describe` gives the offset table for docs and PCGamingWiki.

```rust
use rea_launcher::identify::hex32;
use rea_launcher::patch::{self, At, PatchSet, PatchSite};
use rea_launcher::pe::Pe;

const RETAIL: [u8; 32] = hex32("9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa");

const ASPECT: PatchSet = PatchSet {
    name: "aspect",
    summary: "Ultra-wide screens without stretching",
    editions: &[RETAIL],
    sites: &[PatchSite {
        at: At::Va(0x0068_3fa4),
        original: &[0x39, 0x8e, 0xe3, 0x3f],
        patched: &[0x26, 0xb4, 0x17, 0x40],
        note: "16:9 constant to 21:9",
    }],
    caves: &[],
    results: &[], // (RETAIL, hash after) once measured on the real build
};

# fn main() -> Result<(), rea_launcher::patch::PatchError> {
# let pristine = rea_launcher::pe::synthetic::image(
#     rea_launcher::pe::Format::Pe32, 0x40_0000,
#     &[(".rdata", 0x28_3000, &[0x39, 0x8e, 0xe3, 0x3f].repeat(0x400), rea_launcher::pe::synthetic::DATA)]);
let pe = Pe::parse(&pristine).map_err(patch::PatchError::Pe)?;
let rows = patch::describe(&ASPECT, Some(&pe))?;
println!("{}", patch::to_markdown(&rows)); // docs/README
println!("{}", patch::to_wikitext(&rows)); // PCGamingWiki fix box
// The launcher: patched = patch::apply_sets(&pristine, &[&ASPECT])?;
# Ok(()) }
```

In the launcher's tests, `patch::testing::assert_golden(&SET, &synthetic)`
checks every site on a small synthetic file (golden bytes, idempotent,
reversible, all-or-nothing), and
`patch::testing::assert_real_game("GAME_DIR", &[&SET, …])` checks every set
on the player's real build when the variable names its folder (and does
nothing otherwise). `pe::synthetic::image` builds the synthetic PE.

## Downloads on request

`download` fetches a pinned file the player asked for (never on its own):
free-space check, resume with an HTTP range (a server that sends the whole
file again starts over), SHA-256 while streaming, then with the `zip`
feature an unpack into a temporary folder renamed into place, a
`.complete` marker and older versions removed. The platform supplies the
HTTP GET as a `Fetch`; a window polls `Job::phase()` from another thread.

```rust
use std::path::Path;
use rea_launcher::download::{self, Fetch, Job, Phase, Pin};

struct Offline;

impl Fetch for Offline {
    fn fetch(&self, _url: &str, _from: u64, _start: &mut dyn FnMut(bool),
             _chunk: &mut dyn FnMut(&[u8]) -> bool) -> Result<(), String> {
        Err("no network in this example".into())
    }
}

let pin = Pin::parse(&format!("https://example.org/r/Pack-v2.zip {} 1000 5000", "0".repeat(64))).unwrap();
let root = std::env::temp_dir().join("rea-doc-download");
let job = Job::default(); // Arc<Job> shared with the window in a launcher
let free = |_: &Path| None; // the platform's free-space query
let phase = download::download(&pin, &root, &free, &Offline, &job);
assert!(matches!(phase, Phase::Failed(_)));
assert_eq!(job.phase(), phase);
# let _ = std::fs::remove_dir_all(root);
```

With `features = ["zip"]`, `download::install(&pin, &root, &free, &fetch,
&job)` also unpacks and returns `Phase::Done(folder)`;
`download::installed(&pin, &root)` finds a complete version later. For an archive the SDK can't open (`.cab`, an installer), pass
the launcher's own extractor to `download::install_with`; it reports
progress with `job.set_phase(Phase::Unpacking { done, total })`. Give
each component its own `root`: older versions there are removed.

## Migration

What changes for each launcher when it swaps its own copy for the SDK's.
Behaviour not listed here is the same.

### Every launcher

- **v0.4.0:** golden snapshot tests use `rea_launcher::snapshot!(name,
  text)` (or `snapshots!()` with `.update_var("<ID>_UPDATE_SNAPSHOTS")` to
  keep the documented variable) in place of a local `check()`; drop the
  local `.replace(env!("CARGO_PKG_VERSION"), "<version>")`. Stored
  snapshots that hold the version literally now fail: write `<version>`.
- **v0.4.0:** a launcher that measured its runtime copy from a second
  thread (DiRT Rally `copy.rs`) calls `runtime::sync_with_progress` with a
  `ProgressCounter` instead. `runtime::sync` is unchanged.
- **v0.4.0:** a native Linux launcher takes `rea_launcher::instance::single`
  at start (DiRT Rally had no guard when run without the play helper).
- **Temp files** are `.name.rea-new` (CMR3 used `.name.cmr3-new`, CMR4
  `.name.cmr4-new`, OR `name.tmp`, SRR `name.srr-tmp`). Remove old
  leftovers once with `fsx::remove_stale_temps(runtime, &[".cmr3-new"])`;
  it only unlinks, so a leftover hard link to an original is harmless.
  Pass suffixes like `.tmp` only for folders where the game has no `.tmp`
  files of its own; `fsx::remove_stale_temps_here` looks in one folder
  only, and `fsx::remove_files(&[path])` removes exact names (OR's
  `.or-manifest.tmp`).
- **A refused replace** on Windows now moves the old file to
  `.name.rea-old`, renames the new one in and removes the backup after;
  the older launchers deleted the old file first, which could lose both.
- **Runtime presence**: a symbolic link in the runtime counts as missing
  and is replaced by a real file (`Policy::present`). Call
  `runtime::remove_legacy_links` once to also drop links to folders.
- **`Policy::owned` and `game_written`** receive the source `Stamp`.
- **Errors** from `runtime::sync`, `saves` and `fsx` name the path.
- **`launcher.cfg`**:
  - `set_key` rewrites every active line of the key (CMR3, SCR and OR
    rewrote the first; a later duplicate then won when parsed). CMR4 and
    SRR already rewrote all.
  - With no active line, `set_key` turns the first `#key =` placeholder
    into the line, under its comment (all five appended at the end; OR's
    test `set_key("#game = x\n", …)` expected `#game = x\ngame = y`).
    Prose comments that mention the key are not placeholders.
  - A line starting with `;` is a comment (only `#` was).
  - Unknown keys produce a warning ("ignored, kept in the file"), as CMR4
    did; CMR3, SCR, SRR and OR ignored them silently.
  - `read_file` reads non-UTF-8 as Latin-1 (CMR4 16edc74); only NotFound
    is a first run. A UTF-8 BOM stays in the text and is kept by
    `set_key`/`clear_key`, so the file is saved with it (v0.1.0 dropped
    it).
  - `parse_args` takes the launcher's value-less switches (`--no-window`,
    `--check`, `--settings`, `--linux`) in the same pass and returns them;
    a switch name in a value position is a value.
  - `parse_args_with(…, KeyStyle::Hyphen, …)` keeps hyphens, for SRR's
    `frame-limit` style keys (`--frame-limit` ↔ `frame-limit`).
  - **v0.2.0, breaking:** `ArgError::Help(flag)` carries the flag given
    (`--help`, `-h` or `/?`); match `ArgError::Help(_)`.
- **`resolution`** words come from a table passed to
  `cfg::parse_resolution`; each launcher's current spellings:

  | Launcher | Table | Sizes |
  |---|---|---|
  | CMR3 | `ORIGINAL` (`original` = 640×480) | 640×480 – 7680×4320 |
  | CMR4 | `ORIGINAL` | 320×240 – 7680×4320 |
  | OR | `ORIGINAL_OR_OFF` (`off` too) | 640×480 – 7680×4320 |
  | SCR | `ORIGINAL_OR_FULLSCREEN` (`off`; `fullscreen`, `full`, `desktop`) | 320×200 – 7680×4320 |
  | SRR | `SIZES_ONLY` | 640×480 – 7680×4320 |

  `×` and `X` are accepted as the separator everywhere (only `x` was).
- **`display::landscape`** offers sizes larger than 640×480 (CMR3, CMR4,
  OR); SRR's copy also offered exactly 640×480: use
  `display::landscape_from(screen, (640, 480))`.
- **`paths::canonical_existing(path, cwd)`** is CMR4's
  `canonical_existing` (the game folder must exist).
- **`log::Log`** caps at 1 MiB by default; `.with_limit(bytes)` changes
  the cap and `.without_limit()` removes it.
- **Binary patches**: `patch::apply`, `signature_toggle` and `check_caves`
  stay; new games describe fixes as `PatchSet` tables. SCR's `FixSet` /
  `prepare` map to sites at `At::Offset` (its LE parser stays local, which
  turns its VAs into offsets); SRR's pacer becomes a set with three caves
  and a `results` hash, `pacer_verified` becomes `apply_sets`, and its
  pattern-found aspect slots stay a launcher function (no fixed address);
  CMR4's widescreen toggle stays `signature_toggle`.
  `identify::validate_pe32` now reads the headers through `pe`.
- **`Ledger::take_out_keep_game(rel, known)`** removes our file and does
  not bring the game folder's copy back (OR's DXVK `d3d9.dll`, where the
  repack's SpecialK must stay out); OR's DXVK marker file can go.
- **`Ledger::take_out_with(rel, known, &policy)`** brings the game's file
  back the way a sync would (hard-linked where the policy links);
  `take_out` copies.
- **`display::game_monitor`** returns `Some(index)` only when a monitor
  other than the primary should be used (CMR4's `choose().output` was
  `index + 1`).

### CMR3

- Profile mirroring: `Mirror::file(…, FirstRun::NewerWins)`. The old
  `.cmr3-profile-state` (`size\tmtime`) is read; it is rewritten in the
  five-field format.
- `runtime::restore(game, runtime, rel, &policy)` takes the policy (for
  linking and the runtime name) instead of using `is_static` itself.
- The component ledger (`.cmr3-launcher-components`) keeps its format.
  `Ledger::install` now refuses (`Outcome::NotOurs`) to replace a file
  that is neither ours nor an untouched copy of the game's; CMR3's
  `put_owned` overwrote it.

### CMR4

- `components.rs` → `Ledger`: `install_wrapper`'s refusal becomes
  `Outcome::NotOurs` instead of an error; turn it into one if wanted.
- `runtime::list` skips loops, unreadable folders and non-Unicode names
  with warnings (CMR4 failed on the first unreadable entry).
- `check_separate` no longer requires the game folder to exist.

### SCR

- `stage_source_tree` → `runtime::sync` with a policy whose
  `runtime_rel` upper-cases, `owned` covers the generated files, CD images
  and the sub-50 KB `GLIDE2X.OVL` (by its `Stamp`), and `game_written`
  the saves. CD-image filtering stays in the launcher.
- Saves: `Mirror::folder(runtime, saves, state, FirstRun::NewerWins)
  .scan(&["", "CONFIG", "REPLAYS"]).only(&is_save)`. `.screamer-saves`
  is read (its upper-case names match any case now). The state file may
  stay in the runtime folder; it is never mirrored itself.
- `launcher.log` → `log::Log` (same 1 MiB cap and truncation).

### SRR

- Count `rel.srr-disabled` as present with `Policy::present`.
- `config.ini` → `Ini::from_bytes(…).tight()` and `to_bytes()`. A valid
  UTF-8 file is now edited as UTF-8 text (SRR always used Latin-1 per
  byte, which wrote UTF-8 names back unchanged too, but would have
  written a new non-ASCII value as Latin-1 into a UTF-8 file).

### rea-rad-mobile (v0.8.0)

What changes when it moves its own copies onto the SDK:

- `wheel/*`, `pad_map.rs`, the generic half of `ffb.rs`, `NavTracker`,
  `is_spare_interface`, `find_aux` → `rea_launcher::wheel`, `::ffb`,
  `pad::PadNav::with_repeat`. `Button` implements `wheel::Control` with
  `bit()` in today's order (the bridge's and MAME's button order: never
  renumber it). `Cabinet` → `wheel::Controls` (`to_frame` stays local).
  `WheelProfile::guess(id, initial, Some((Light, Wiper)))`; the wizard
  takes the step list; `PadMap::with(…)` is the default layout;
  `read_pad(…, Some(Exit { button: Exit, suppress: &[Start, Coin] }))`.
- `wheel.ini` and the `pad_*` keys of `settings.ini`: the same keys, read
  in any case and in the old `key=value` form; written as `key = value`
  through `cfg::set_key`, keeping the player's comments (it rewrote the
  whole file). Keys the profile no longer has are commented out.
- `FfbEngine` → `SyntheticFfb::new(Tuning::ARCADE)`, fed
  `CarState { racing: t.playing, speed: t.speed / 430.0, contact:
  t.touching() }`; the hit threshold is the same (600/430 top speeds/s).
- Behaviour that changes for players: a Logitech or Thrustmaster pad is a
  pad (it was a wheel); an armed wizard pedal times out 10 s after it
  armed; the menu's test pattern uses at least 25 % and at most 100 %
  strength.
- `paths.rs`'s data folder → `paths::data_dir` (same order; an empty
  `XDG_DATA_HOME`/`LOCALAPPDATA` counts as unset). `log.rs` →
  `Log::new(..).per_run().timestamps(true)`: `info` lines go to the file
  only (it printed every line to stderr too), and lines get `info:`/
  `warning:` prefixes from `info`/`warn`.

### OR

- Saves: `Mirror::folder(…, FirstRun::SavesWin)`, plus
  `.deleted(Deleted::Stays)` to keep OR's launch rule: a save missing on
  one side while the other is unchanged is not copied back at launch.
  OR's own record dropped such a save, so it came back one launch later;
  the SDK remembers it as gone. `.or-saves` (`size\tmtime\trel`) is read.
- `ini.rs` → `Ini::parse` (spaced `key = value`, as before).
- `hdpack.rs` → `download` with `features = ["zip"]`: `Pack` is `Pin`
  (`sha256` is bytes; `unpacked` is `unpacked_size`), `install` takes a
  free-space callback instead of `Option<u64>`, the marker is written
  before the folder is renamed into place, and messages no longer name
  the Download button (the window adds that).
- `save_dir` → `paths::find_ignoring_case(runtime, "SaveGame")` (v0.2.0
  reports the on-disk spelling on case-insensitive disks too).
- Owned files: `Policy::owned(rel, stamp)`.
