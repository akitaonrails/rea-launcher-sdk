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
| `wine` | `is_wine` (export + env fallback), `seed_winebus` | CMR4 / OR |
| `reg` | pure: `Error`, `seed_missing`, `virtual_store_path`, `uses_32_bit_view`; RAII `Key`: `open_read` and `Key::read_machine` (read-only, never create; VirtualStore value first, as the game sees it); `dword`/`string`/`has`/`set_*`/`*_if_missing`/`seed_dwords`; `Key::machine_or_virtual_store` (HKLM — 32-bit view under `SOFTWARE` only — or HKCU VirtualStore on access denied; `Place::describe` gives the launchers' log labels "HKLM 32-bit view" / "HKCU VirtualStore") | CMR4 |
| `process` | `Launch` builder (args, cwd, env, hidden) → `Running` (`wait`, `is_alive`, `exit_code`, `close_gracefully(timeout)` → `Closed::{AlreadyExited, Closed, Terminated}`, `message_hook` for a close-hook DLL such as `rea-launcher-close-hook`; `#[must_use]` hook); `top_level_windows(pid)` | CMR4 + CMR3 |
| `instance` | `single(name) -> Result<Guard, AlreadyRunning>` (named mutex in `Local\`), `AlreadyRunning::bring_to_front(Window::Title/Class)`, `is_window_found`, `mutex_name` | new |
| `xinput` | cached lookup; `pads` (as `rea_launcher::pad::Pad`), `buttons`, `connected`, `set_rumble`/`test_rumble`/`stop_rumble` (latest call wins), `ids` (USB vid/pid) | CMR4 + SRR/OR |
| `display` | `monitors` (physical modes, as `rea_launcher::display::Monitor`), `displays` (+ refresh rate, `\\.\DISPLAYn` device, adapter name), `adapters` (GPU names, attached/primary), `screen_size`, `mode_supported`, `wine_dpi_scale` (Wine only; 1.0 and untouched DPI awareness on Windows) | CMR4 + SCR + SRR + OR |
| `gpu` | `prefer_fast_gpu` (`GpuPreference=2;`), `prefer_glx` (Wine `UseEGL=N`) | all four |
| `dialog` | `message`, `pick_folder`, `open_url` | all four |
| `report` | `fatal`/`note(title, text, Mode::{Command, Play})`: print and/or dialog by the rule below; `route`, `route_here`, `Terminal` | CMR3/CMR4 regression |
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

`report::route(mode, Terminal { attached, redirected, wine })` is the
decision table (pure, tested on the host); `report::route_here(mode)`
applies it to this process. Don't use `console::output` for player-facing
messages: it treats Wine's redirected stdout as a terminal someone reads,
which hid CMR3's and CMR4's play-time errors in v0.3.0.

**One instance and action flags.** A second start with an action flag
(`--download-*`, `--check`) while a launcher is open must not exit 0 with
nothing done: report it (`Mode::Command`) and exit non-zero (3). A plain
second start brings the window forward (`AlreadyRunning::bring_to_front`);
when no launcher window exists (`is_window_found` is false) it is the game
that runs, and the player is told so (`Mode::Play`). See the `instance`
module docs.

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
    use rea_launcher_win32::{gpu, process::Launch, reg, wine};

    let exe = runtime.join("game.exe");
    let mut launch = Launch::new(&exe).cwd(runtime);
    if wine::is_wine() {
        wine::seed_winebus(&[("Map Controllers", 1)])?;
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
