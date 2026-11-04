# rea-launcher-win32

The Win32 side of the rea launchers, in one place with every `unsafe` block
explained. Everything that calls Windows is `#[cfg(windows)]`. The pure
helpers it builds on — argument quoting (`rea_launcher::args`), environment
blocks (`rea_launcher::env`), DLL-override merging and Wine constants
(`rea_launcher::wine`), monitor choice (`rea_launcher::display`), XInput
state parsing and button bits (`rea_launcher::pad`) — live in the portable
`rea-launcher` crate and are tested on the host.

| Module | What | From |
|---|---|---|
| `wine` | `is_wine` (export + env fallback), `seed_winebus`, `app_dll_overrides`/`remove_app_dll_overrides(exe, &[(dll, DllMode)])` → `Applied { written, ours, matching, player }`, ownership recorded under `owned_key(exe)`; pure `DllMode`, `app_defaults_key`, `owned_key`, `step` → `Step`, `removable` (`plan` kept for old callers) | CMR4 / OR / SRR / DiRT |
| `reg` | pure: `Error`, `seed_missing`, `virtual_store_path`, `uses_32_bit_view`; RAII `Key`: `open_read` and `Key::read_machine` (read-only, never create; VirtualStore value first, as the game sees it); `dword`/`string`/`has`/`set_*`/`*_if_missing`/`seed_dwords`; `Key::machine_or_virtual_store` (HKLM — 32-bit view under `SOFTWARE` only — or HKCU VirtualStore on access denied; `Place::describe` gives the launchers' log labels "HKLM 32-bit view" / "HKCU VirtualStore") | CMR4 |
| `process` | `Launch` builder (args, cwd, env, hidden) → `Running` (`wait`, `is_alive`, `exit_code`, `close_gracefully(timeout)` → `Closed::{AlreadyExited, Closed, Terminated}`, `message_hook` for a close-hook DLL such as `rea-launcher-close-hook`; `#[must_use]` hook); `top_level_windows(pid)`; `image_path(pid)`; `find_running(image)` (Toolhelp32 + `QueryFullProcessImageNameW`: the pids running that exact exe) | CMR4 + CMR3 + OR |
| `instance` | `single(name) -> Result<Guard, AlreadyRunning>` (named mutex in `Local\`), `AlreadyRunning::second_start(action, window)` → `SecondStart::{Focused, Busy, WindowOpen}` (`exit_code`: 0 or `ALREADY_RUNNING` = 5, `message(game)`, `log_line`; pure `SecondStart::decide`), `bring_to_front`/`is_window_found(Window::Own/Title/Class/ClassTitle)`; pure `is_own_window`, `same_image`, `mutex_name` | SRR + DiRT + CMR4 + Screamer |
| `xinput` | cached lookup; `pads` (as `rea_launcher::pad::Pad`), `buttons`, `connected`, `set_rumble`/`test_rumble`/`stop_rumble` (latest call wins), `ids` (USB vid/pid) | CMR4 + SRR/OR |
| `display` | `monitors` (physical modes, as `rea_launcher::display::Monitor`), `displays` (+ refresh rate, `\\.\DISPLAYn` device, adapter name), `adapters` (GPU names, attached/primary), `screen_size`, `mode_supported`, `wine_dpi_scale` (Wine only; 1.0 and untouched DPI awareness on Windows) | CMR4 + SCR + SRR + OR |
| `gpu` | `prefer_fast_gpu` (`GpuPreference=2;`), `prefer_glx` (Wine `UseEGL=N`), `prefer_glx_or_restart(Option<Guard>)` → `Glx::{Ready { instance, warning }, Restarted { exit_code }}` (runs the launcher again once when it had to write `UseEGL`; `GLX_RESTARTED_ENV`, pure `needs_restart`) | all four + CMR2005 |
| `dialog` | `message`, `pick_folder`, `open_url` | all four |
| `report` | `audience(Run { command, no_window, settings_only, linux })` → `Audience { command, interactive }` and `.mode()` (who is watching; pure `Audience::of(run, wine)`, `Run::from_args(args, extra_commands)`, `COMMAND_FLAGS`); `fatal`/`note(title, text, Mode::{Command, Play})`: print and/or dialog by the rule below; `route`, `route_here`, `Terminal` | CMR3/CMR4/DiRT regression, CMR1 `audience.rs` |
| `console` | `attach_parent()` (attach to the starting terminal's console and point stdout/stderr at it, keeping redirections), `stdout_usable()`, `output(attached, usable, interactive)` → Terminal / Dialog / Nowhere | new |
| `disk` | `free_space` | OR |
| `http` | `WinHttp::new(agent)`: `impl rea_launcher::download::Fetch` (HTTPS through WinHTTP, ranged resume, redirects, streamed) — pass it to `download::install`; pure `split_https`, `range_header`, `body_starts_at_from` | OR |

The Windows-only code has tests too; run them under Wine with
`scripts/wine-test --workspace` (registry round trips in the test prefix,
display enumeration).

Registry errors are `reg::Error`, which converts into `String` for `?` in
launcher code that reports strings.

## Messages: terminal, log or dialog (`report`)

The launchers are GUI-subsystem executables: their output has no console
by default, and under Wine (`<id>-play`, the AppImage, Proton) stdout is
always redirected to a log nobody watches. Send every message through
`report::fatal(title, text, mode)` / `report::note(title, text, mode,
warning)`:

| Mode | Printed (stdout, the log) | Dialog |
|---|---|---|
| `Mode::Command { interactive }` — `--help`, `--check`, `--list`… | when there is a console or a redirect | only when nobody sees the terminal (no console, and stdout goes nowhere or, under Wine, to a log) and `interactive` |
| `Mode::Play { interactive }` — unknown edition, game folder not found, already running, a failed install… | when there is a console or a redirect | always when `interactive` (not `--no-window` from a script) |

**Never compute `interactive` yourself.** Take the mode from the run's
flags: `report::audience(run).mode()`, where `run` is a `report::Run`
filled from the parsed command line (or `Run::from_args`). The rule it
applies:

| Run | A person may be watching (`interactive`) |
|---|---|
| the window, then the game; any run under Wine; any `--linux` play run (`--no-window --linux`) | yes |
| `--no-window` on native Windows without `--linux` | no: a script |
| `--settings --linux` (the helper's settings window) | no: the window shows its own messages; exit code 4 makes the helper play, and that run reports |
| a command (`--help`, `--check`…) | yes, unless `--no-window` on native Windows or `--linux` (the helper reads the output) |

`interactive = !no_window` was the v0.3.x mistake in CMR3, CMR4 and DiRT:
the Linux helper always starts the game run with `--no-window`, so a wrong
game folder or "already running" under Wine looked like a game that never
started. `Audience::of` is tested for all 32 combinations of the four
flags and Wine.

`report::route(mode, Terminal { attached, redirected, wine })` is the
decision table (pure, tested on the host); `report::route_here(mode)`
applies it to this process. Don't use `console::output` for player-facing
messages: it treats Wine's redirected stdout as a terminal someone reads,
which hid CMR3's and CMR4's play-time errors in v0.3.0.

**One instance and action flags.** On `Err(running)` from
`instance::single`, call `running.second_start(action, Window::Own(TITLE))`:
a plain start brings the launcher's window forward and exits 0
(`SecondStart::Focused`); with no window it is the game that runs, and the
player is told so and the run exits `ALREADY_RUNNING` (`Busy`); an action
flag (`--download-*`, `--setup`) never brings the window forward and never
exits 0 (`WindowOpen`/`Busy`). Use `Window::Own`: eframe windows all share
winit's class, and a title alone also matches an Explorer folder or the
game's window. **Before Play**, check `process::find_running(&runtime_exe)`:
a launcher killed while the game ran released the mutex, and a second game
must not start from the same runtime copy. See the `instance` module docs.

## Wine DLL overrides: the registry, not the environment block

Under Wine, `WINEDLLOVERRIDES` in the child's environment block
(`Launch::env`) is **ignored** by Wine's loader in the game; only the Unix
environment it inherited (from `<id>-play`, the AppImage or Proton) and the
registry count. So under plain Proton/Steam, with no helper script, a
bundled DLL Wine also has (d3d9, dinput8, d3dx9_*…) may silently load as
Wine's. Before starting the game, under Wine, call
`wine::app_dll_overrides(exe_file_name, &[(dll, DllMode::…)])`: it writes
`HKCU\Software\Wine\AppDefaults\<exe>\DllOverrides` in the current prefix
(only where no value exists; the player's values are kept) and is a no-op
on Windows. When a component is turned off,
`wine::remove_app_dll_overrides` removes only the values the launcher
wrote.

**Ownership is recorded, not guessed.** Each value `app_dll_overrides`
writes is also recorded, with the exact text, under
`HKCU\Software\rea-launcher\DllOverrides\<exe>` (`wine::owned_key`), in
the same prefix. Removal deletes a value only while it still equals the
record; a value the player set — even one equal to ours — has no record and
is never deleted, and a value of ours the player changed loses its record.
Before v0.4.0 a player's `d3d9=native,builtin` was removed with ours
because the two looked the same (DiRT review). A Wine test proves every
case (`wine::tests`).

**Microsoft's DLLs need the override; others don't.** With no override,
Wine's loader replaces a DLL whose version resource names Microsoft as the
vendor with its own builtin, *even when the DLL sits in the program
folder* (`WINEDEBUG=+module` logs `version_heuristics got default vendor
"Microsoft Corporation"`, then `find_builtin_dll`). So a bundled
Microsoft redistributable — `d3dx9_*`, `d3dcompiler_*`, `xinput1_3`,
`msvcp140`, `vcruntime140`, `xaudio2_*` — loads as Wine's unless
`app_dll_overrides` sets it to `native` / `native,builtin`. A DLL from
anyone else (DXVK's `d3d9`/`dxgi`, dgVoodoo, Xidi's `dinput8`, an ASI
loader) wins from the program folder by default; set its override anyway
when the launcher depends on it (the player may run under a prefix that
overrides it). Found by Sega Rally Revo (its `d3dx9_35` loaded as
builtin; `docs/RUNTIME_NOTES.md` there, 2026-11-02).

## Use

```toml
[dependencies]
rea-launcher = { git = "https://github.com/akitaonrails/rea-launcher-sdk", tag = "v0.1.0" }
rea-launcher-win32 = { git = "https://github.com/akitaonrails/rea-launcher-sdk", tag = "v0.1.0" }
```

```rust
#[cfg(windows)]
fn play(runtime: &std::path::Path) -> Result<u32, String> {
    use rea_launcher::wine::merge_dll_overrides;
    use rea_launcher_win32::{gpu, process::Launch, reg, wine::{self, DllMode}};

    let exe = runtime.join("game.exe");
    let mut launch = Launch::new(&exe).cwd(runtime);
    if wine::is_wine() {
        wine::seed_winebus(&[("Map Controllers", 1)])?;
        // What Wine's loader reads for the game, in any prefix.
        wine::app_dll_overrides(
            "game.exe",
            &[("d3d9", DllMode::NativeThenBuiltin), ("dxgi", DllMode::NativeThenBuiltin)],
        )?;
        // Kept for the log; Wine ignores it in the child's block.
        let existing = std::env::var("WINEDLLOVERRIDES").ok();
        launch = launch.env(
            "WINEDLLOVERRIDES",
            merge_dll_overrides("d3d9,dxgi=n,b", existing.as_deref()),
        );
    } else {
        gpu::prefer_fast_gpu(&exe)?;
    }
    let (key, place) = reg::Key::machine_or_virtual_store(r"SOFTWARE\Studio\Game")?;
    key.set_string("INSTALL_PATH", &runtime.display().to_string())?;
    key.set_dword_if_missing("Width", 1920)?;
    let _ = place; // log where it went: place.describe()
    launch.run()
}
```

Game knowledge (which registry values, which DLLs to override, winebus
values) stays in the launcher; this crate provides the mechanisms.

## Migration

What changes for a launcher moving to this version of the crate.

### v0.4.2

- **A bad command line** — when the command line fails to parse, report
  it with `report::audience(Run::bad_command_line(&args)).mode()` and exit
  `report::BAD_COMMAND_LINE` (2). A play-run mode showed a modal box from
  PowerShell and the process waited for OK (CMR, CMR4, DiRT, SRR). Delete
  a local `Run { command: true, .. }` for parse errors (CMR3, DiRT Rally).
- **Action flags** — pass `instance::is_action(&args, &[own actions])` as
  `second_start`'s `action` (it knows `--check`, `--setup`, `--download-*`)
  and take the instance for `--check` too, so `--check` while the window
  is open exits 5 like `--setup` and the downloads.

### v0.4.0

- **`report::Audience`** — replace any hand-made `interactive`
  (`!no_window`, `!linux`, a local `audience.rs`) with
  `report::audience(Run { command, no_window, settings_only, linux }).mode()`
  from the parsed flags, and delete the local copy (CMR1, CMR3, DiRT have
  one). Behaviour changes only where the local rule was wrong: a
  `--no-window` game run under Wine or with `--linux` now gets its
  play-time dialog; `--settings --linux` and `--check --linux` don't.
  Verify: the launcher's tests call `report::Audience::of` for its
  helper's command lines (`--no-window`, `--no-window --linux`,
  `--settings --linux`).
- **`wine::app_dll_overrides` ownership** — no code change. From now on
  the values it writes are recorded under `owned_key(exe)`, and
  `remove_app_dll_overrides` deletes only recorded ones. Values written by
  a v0.3.x launcher have no record: they are reported in
  `Applied::matching` and are no longer removed when the component is
  turned off (with the component's DLL gone from the runtime, a leftover
  `native,builtin` falls back to Wine's builtin, which is what "off"
  meant). A launcher that constructs `Applied` by hand adds `matching`.
  Log `matching` next to `ours`.
- **`instance`** — replace a local second-start module (SRR's and DiRT's
  `instance.rs` `Second`, CMR4's `SecondStart`, Screamer's and CMR4's
  window lookups in `win32.rs`) with `AlreadyRunning::second_start(action,
  Window::Own(TITLE))`, `SecondStart::exit_code()` (0 or 5) and
  `message(game)` (or keep the launcher's own wording). Before Play,
  refuse with `SecondStart::Busy` when `process::find_running(&runtime_exe)`
  is not empty. `Window` has two new variants (`Own`, `ClassTitle`): a
  `match` on it needs arms for them. Exit codes: SRR and DiRT already use
  5; CMR4 exits 1 for a refused second start and CMR2005 0 — moving to the
  helper makes them 5 (tell the Linux helper: 5 means "another launcher
  holds this prefix").
- **`gpu::prefer_glx_or_restart`** — before the settings window, under
  Wine, replace `gpu::prefer_glx()` (and CMR2005's own re-exec) with
  `match gpu::prefer_glx_or_restart(guard) { Glx::Restarted { exit_code }
  => exit(exit_code), Glx::Ready { instance, warning } => … }`. On a
  fresh prefix the first start used to open the window through EGL
  (software, or no window at all on NVIDIA without egl-x11) until the next
  start; now it runs again at once. Verify: delete `UseEGL` from
  `HKCU\Software\Wine\X11 Driver` in a test prefix and start the window:
  `launcher.log` shows two starts and the window draws.
