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

### ini

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

## Migration

What changes for each launcher when it swaps its own copy for the SDK's.
Behaviour not listed here is the same.

### Every launcher

- **Temp files** are `.name.rea-new` (CMR3 used `.name.cmr3-new`, CMR4
  `.name.cmr4-new`, OR `name.tmp`, SRR `name.srr-tmp`). Remove old
  leftovers once with `fsx::remove_stale_temps(runtime, &[".cmr3-new"])`;
  it only unlinks, so a leftover hard link to an original is harmless.
  Pass suffixes like `.tmp` only for folders where the game has no `.tmp`
  files of its own.
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
  - `read_file` drops a UTF-8 BOM and reads non-UTF-8 as Latin-1 (CMR4
    16edc74); only NotFound is a first run.
  - `parse_args` takes the launcher's value-less switches (`--no-window`,
    `--check`, `--settings`, `--linux`) in the same pass and returns them;
    a switch name in a value position is a value.
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
  OR); SRR's copy also offered exactly 640×480.
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

### OR

- Saves: `Mirror::folder(…, FirstRun::SavesWin)`, plus
  `.deleted(Deleted::Stays)` to keep OR's launch rule: a save missing on
  one side while the other is unchanged is not copied back at launch.
  OR's own record dropped such a save, so it came back one launch later;
  the SDK remembers it as gone. `.or-saves` (`size\tmtime\trel`) is read.
- `ini.rs` → `Ini::parse` (spaced `key = value`, as before).
- Owned files: `Policy::owned(rel, stamp)`.
