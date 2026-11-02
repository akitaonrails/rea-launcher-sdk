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
| `fsx` | Whole-file swaps (temp + rename) that never write through a hard link |
| `paths` | `resolve`, the runtime/game overlap check, `native`, `\\?\` stripping |
| `runtime` | The private runtime copy: list, plan, sync, restore, manifest |
| `saves` | Save mirroring by recorded state, for one file or a folder |
| `ledger` | SHA-256 record of the components the launcher installed |
| `ini` | Section-aware INI editing that keeps comments, case, CRLF, BOM, bytes |
| `cfg` | `launcher.cfg` reading, line editing, value parsers, shared CLI flags |
| `wine` | `WINEDLLOVERRIDES` merge, Wine detection from the environment, winebus names |
| `env` | Child environments and the `CreateProcessW` environment block |
| `identify` | SHA-256 tables, hex, PE32 check |
| `patch` | All-or-nothing byte sites, one-match signatures, caves, result hashes |
| `display` | Monitor choice, landscape sizes, Steam Deck, the largest 16:9 size |
| `pad` | XInput state parsing and settings-window navigation by edges |
| `log` | `launcher.log` with a size cap |
| `args` | Windows command-line quoting |

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
use rea_launcher::runtime::{self, Policy};

struct Cmr3;

impl Policy for Cmr3 {
    fn link(&self, rel: &str) -> bool {
        rel.starts_with("Data/") || rel.ends_with(".exe")
    }
    fn game_written(&self, rel: &str) -> bool {
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
let mirror = Mirror::file(&rt_profile, &saved, &state).first_run(FirstRun::NewerWins);
let moved = mirror.before()?; // a changed saves copy wins, else a changed runtime copy
// … the game runs …
let copied = mirror.after()?; // runtime → saves where the bytes differ
# let _ = (moved, copied); std::fs::remove_dir_all(rt)?; let _ = std::fs::remove_dir_all(saves);
# Ok(()) }
```

`Mirror::folder(…)` does the same for every file below a folder, and
`.only(&filter)` limits it to some names.

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

### ini

```rust
use rea_launcher::ini::Ini;
let bytes = b"[GameController]\r\nName=Manette \xE9tendue\r\n";
let mut ini = Ini::from_bytes(bytes).tight(); // byte-faithful, `key=value`
ini.set("Graphics", "VSync", "1");
assert!(ini.to_bytes().starts_with(bytes));
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
cfg::parse_args(["--vsync".into(), "off".into()], &["vsync"], |k, v| s.apply(k, v)).unwrap();
assert_eq!(s.vsync, Some(false));
// Only the launcher's own line changes; comments stay.
assert_eq!(cfg::set_key("# Picture\n#vsync =\n", "vsync", "on"), "# Picture\nvsync = on\n");
```

### wine

```rust
use rea_launcher::wine;
let merged = wine::merge_dll_overrides("dinput8,d3d9=n,b", Some("D3D9.dll=b;xinput1_3=n"));
assert_eq!(merged, "dinput8,d3d9=n,b;xinput1_3=n");
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
assert_eq!(display::game_monitor(&monitors).map(|(i, _)| i), Some(1));
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
