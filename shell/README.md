# rea-play.sh — the shared Linux play helper

`rea-play.sh` is a POSIX sh library (it runs under dash, busybox ash and bash's
sh mode) that each launcher's `linux/<x>-play` sources. The game script keeps
the configuration and the game's own knowledge, about 40–80 lines. The library
does the rest: launcher and `launcher.cfg` discovery, GPU choice, the Wine
prefix, gamescope, the supervised session (lock, stop, status), the settings
window and command dispatch.

`example-play` is a complete game script for a hypothetical Wine game.

## Where the library lives

- In a launcher repo, `scripts/sdk-sync` vendors it as `sdk/rea-play.sh`.
- In a package (zip, AppImage), the builder copies it next to the helper.

A game script finds it like this (the first three lines are the test
convention, see "Tests for a game script" below):

```sh
_fonly=${REA_FUNCTIONS_ONLY:-}
_here=$(dirname "$(readlink -f "$0")")
[ "$_fonly" = 1 ] && [ -n "${REA_PLAY_HERE:-}" ] && _here=$REA_PLAY_HERE
if [ -f "$_here/rea-play.sh" ]; then
  . "$_here/rea-play.sh"
elif [ -f "$_here/../sdk/rea-play.sh" ]; then
  . "$_here/../sdk/rea-play.sh"
else
  echo "rea-play.sh not found next to $0 or in ../sdk (run scripts/sdk-sync)" >&2
  exit 1
fi
REA_HERE=$_here REA_NAME=<x>-play
```

The SDK's `packaging/appimage/AppRun.in` exports `<PREFIX>_LAUNCHER`,
`<PREFIX>_WAIT=1` and Wine's `PATH`/`WINEDLLOVERRIDES`. It does not export
`WINEPREFIX`. The game script maps these to `REA_EXE`, `REA_WAIT` and
`REA_PREFIX`, as shown below. The prefix default
(`~/.local/share/$REA_APP/prefix`) matches the one AppRun.in documents when
`REA_APP=<app>-launcher`.

## The shape of a game script

```sh
#!/bin/sh
set -u
<library lookup above>                   # 1. source (drops inherited REA_*)
REA_APP=cmr4-launcher                    # 2. configure
REA_EXE_NAME=CMR4Launcher.exe
REA_EXE_HINT=CMR4_LAUNCHER
REA_EXE=${CMR4_LAUNCHER:-}
REA_PREFIX=${CMR4_PREFIX:-}
REA_RUN=${CMR4_RUN:-}
REA_WAIT=${CMR4_WAIT:-}
REA_CFG_APPDATA=CMR4Launcher
# ...game choices (see the tables below)...
rea_hook_before_start() { ...; }         # 3. only the hooks it needs
rea_init || exit 1                       # 4. defaults, paths, state folder, prefix
[ "$_fonly" = 1 ] && return 0            # tests: loaded, not run
rea_main "$@"                            # 5. (calls rea_init itself if needed)
```

Source first, then configure. Sourcing only defines functions and **unsets
every `REA_*` variable the library reads**, so a value inherited from the
player's environment never configures a game: only what the game script
assigns after sourcing counts. The player-facing interface stays each
game's own variables, as today (`CMR3_PREFIX`, `SRR_RUN`, ...), mapped by
the script. A game that wants to let players override something else opts
in under its own name, for example:

```sh
[ -n "${CMR3_GPU+set}" ] && REA_GPU=$CMR3_GPU   # CMR3_GPU= means "no GPU choice"
```

(`REA_GPU` set, even empty, skips detection, so map it only when the
player's variable is set; `REA_GPU=${CMR3_GPU-}` would always skip it.)

`rea_init` applies the defaults, resolves paths and creates the state
folder and prefix. Nothing runs Wine, gamescope or the game before
`rea_main`. The library uses the `REA_` prefix for its state and `_rea_`
for scratch variables, and it works under `set -u`. Player values that are
word-split (`REA_RUN`, `REA_GPU`, `REA_PLAY_ARGS`, `REA_SETTINGS_ARGS`,
`REA_GAMESCOPE_EXTRA`, `REA_EXE_SEARCH`, `REA_SDL_ENV` and the caller's SDL
values) are split with globbing off, so a `*` never expands; a caller's
own `set -f` is kept.

## Commands (`rea_main`)

| Command | What it does |
|---|---|
| *(none)*, `--settings`, `settings` | the settings window, then the game. Arguments go to the window. Already running: says so and exits 0 |
| `--play`, `play`, `start` | the game with the saved settings, no window. Arguments go to the launcher after `REA_PLAY_ARGS` |
| `stop`, `--stop` | ends the session this helper started, then `wineserver -k` in the prefix (Wine mode). Prints `stopped` |
| `status`, `--status` | `running` or `idle` |
| `-h`, `--help`, `help` | usage, plus `rea_hook_usage` |
| anything else | usage on stderr, exit code 2 |

Every non-empty command line goes to `rea_hook_command "$@"` **first**. A
return of 127 (the default) means "not mine" and the table above applies;
anything else is the exit status. A game can therefore keep its own
meaning for any word, the built-in aliases included (Screamer passes
`--help`, `--settings`, `settings`, `play`, ... to its launcher, as today).

Settings-window exit codes: `0` Play, then start. `4` no window could open,
so it plays with the saved settings and says so. `3` closed without Play:
it says so and nothing starts. Anything else counts as a failure and points
at `settings.log`. The window's stderr always goes to
`$REA_STATE/settings.log` (the SRR behaviour). In Wine mode,
`wineserver -w` runs after the window, so the game cannot join the
desktop's wineserver outside gamescope.

## Configuration variables

Only unset variables get a default. Variables marked † keep an explicitly
empty value (empty means "none").

### Paths and mode

| Variable | Default | Meaning |
|---|---|---|
| `REA_APP` | — (required) | state folder and prefix name, e.g. `cmr3-launcher` |
| `REA_MODE` | `wine` | `wine` or `native` (no Wine: SCR) |
| `REA_EXE` | found | the launcher program. When empty, the library looks for `$REA_HERE/$REA_EXE_NAME`, then `$REA_HERE/../$REA_EXE_NAME`, then each path in `REA_EXE_SEARCH`. Relative entries are taken from `REA_HERE` (the script's folder), never the current directory. Native mode needs it executable |
| `REA_EXE_NAME`, `REA_EXE_HINT` | —, `REA_EXE` | the file name, and the variable named in the "not found" message (stderr, exit 1) |
| `REA_HERE`, `REA_NAME` | dir and name of `$0` | the script's folder and name |
| `REA_PREFIX` | `~/.local/share/$REA_APP/prefix` | the private Wine prefix |
| `REA_STATE_BASE` | `$HOME/.config` | where the state folder lives. The Wine launchers keep `~/.config` as today; Screamer sets `${XDG_CONFIG_HOME:-$HOME/.config}`, as its script and launcher do |
| `REA_STATE` | `$REA_STATE_BASE/$REA_APP` | holds `running.pid`, `play.log`, `settings.log`, `inner.sh`, `game.env` |
| `REA_RUN` | empty | wrapper for wine, gamescope and lookups, e.g. `distrobox enter gaming --` (word-split) |
| `REA_CFG` | discovered | `launcher.cfg`. Discovery order: next to `REA_EXE` (a portable install wins, as in every launcher's own `settings_path`), then the last `drive_c/users/*/AppData/Local/$REA_CFG_APPDATA` that is not `Public` (falling back to `$USER`'s), or `$REA_STATE` in native mode or when `REA_CFG_APPDATA` is empty. `rea_start` discovers it again when the file does not exist yet |
| `REA_CFG_NAME`, `REA_CFG_APPDATA` | `launcher.cfg`, empty | |

### Session

| Variable | Default | Meaning |
|---|---|---|
| `REA_ALWAYS_WAIT` | `1` | `1`: every launch waits for the game. `0`: waits only with `REA_WAIT`, otherwise returns at once with the lock kept |
| `REA_WAIT` | empty | the AppImage sets it (`<PREFIX>_WAIT=1`) |
| `REA_LEGACY_LOCK` | `signal` | a pid-only lock from an older script with a live pid counts as running. `signal`: stop TERMs that pid. `keep`: stop never signals it. `off`: such locks are ignored |
| `REA_INTERRUPT_WINESERVER` | `1` | when the waiting helper is interrupted, it also runs `wineserver -k` in the prefix (under distrobox the game can live outside the session group) |
| `REA_PLAY_ARGS` † | `--no-window` | launcher flags for a game run |
| `REA_SETTINGS_ARGS` † | `--settings --linux` | launcher flags for the window |
| `REA_GAME_PROCS` | empty | the game's process names, separated by `\|` (a Windows name cannot contain `\|`). When set, the inner script waits `REA_GAME_SETTLE` seconds after the launcher returns, then until none is alive. Zombies do not count. Names are matched with `pgrep -x`, so a name is at most 15 characters |
| `REA_GAME_SETTLE` | `3` | |
| `REA_DIRECT_NOTE` | `starting without a gamescope of our own` | the message for a run without gamescope |

### One-run flags

| Variable | Default | Meaning |
|---|---|---|
| `REA_HELPER_KEYS` † | empty | `launcher.cfg` keys the helper reads itself that a play run may override, e.g. `scaler resolution display_mode postfx dxvk` (CMR3). The flag is the key with `-` for `_` (`display_mode` → `--display-mode`) |
| `REA_FLAG_SWITCHES` † | `--no-window --settings --linux --check` | launcher flags that take no value; every other `--flag` takes the next word |

`x-play --play --scaler fsr --resolution 1280x720` passes the flags to the
launcher unchanged (it applies them for one run, without saving them).
`rea_start` also reads `--<key> value` and `--<key>=value` for each key in
`REA_HELPER_KEYS` into `REA_ONE_RUN`. For that run `rea_get_key KEY` returns
the flag's value over `launcher.cfg`: gamescope's size, filter and
windowed skip, and any hook such as CMR3's postfx. The last flag wins, as in
the launchers' parser. An invalid value falls back like an invalid file
value. `launcher.cfg` is never written, and the next run reads it again.
`rea_get_key KEY FILE`, with an explicit file, always reads the file. Only
play runs (`--play`, `play`, `start`) take one-run flags; arguments to the
settings window go to the window, which saves what the player picks.

### Size and gamescope

| Variable | Default | Meaning |
|---|---|---|
| `REA_GAMESCOPE` | `auto` | `off`: never use gamescope (SRR) |
| `REA_GAMESCOPE_OUTPUT` | `1` | pass the size as `-W/-H` as well as `-w/-h`. `0`: only `-w/-h` (SCR) |
| `REA_GAMESCOPE_EXTRA` † | empty | extra flags, placed before `--` |
| `REA_RES_KEY` | `resolution` | the cfg key that holds the size |
| `REA_DEFAULT_SIZE` | `1920x1080` | used when the value is unset, invalid or unknown |
| `REA_ORIGINAL_SIZE` | empty | what `original` means. Empty: the default size |
| `REA_ORIGINAL_WORDS` | `original` | the resolution words that mean `REA_ORIGINAL_SIZE` (case-insensitive; only these). OR's Rust config treats `off` as original too (`ORIGINAL_OR_OFF`), but or-play has always given gamescope the default 1920x1080 for `off`; OR keeps the default, and a game that wants both sets `REA_ORIGINAL_WORDS="original off"` |
| `REA_FULLSCREEN_SIZE` | empty | `screen`: `fullscreen`, `full` and `desktop` mean the screen's size (`REA_SCREEN`, else xrandr's current mode) |
| `REA_SCALER_KEY` | `scaler` | the cfg key for gamescope's `-F`. Empty: always use `REA_DEFAULT_SCALER` |
| `REA_DEFAULT_SCALER` | `linear` | |
| `REA_WINDOWED_KEY` | empty | e.g. `display_mode`. When its value is in `REA_WINDOWED_VALUES` (`windowed borderless`), gamescope is skipped. Empty: never skip |

Gamescope is never nested (`GAMESCOPE_WAYLAND_DISPLAY` set) and is skipped
when it is not installed. The command is
`gamescope -W W -H H -w W -h H -f -S fit -F <filter> [extra] -- sh inner.sh <launcher> <flags>`.
Filter mapping: `linear`, `nearest`, `fsr`, `nis` and `pixel` pass as
written (case-insensitive). `bilinear` becomes `linear`. Anything else
becomes the default, so gamescope never receives a filter it would refuse.

### Environment

| Variable | Default | Meaning |
|---|---|---|
| `REA_SDL_ENV` † | `SDL_GAMECONTROLLER_ALLOW_BACKGROUND_EVENTS=1 SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS=1` | `NAME=DEFAULT` words. The caller's non-empty value wins. `SDL_JOYSTICK_DEVICE` is always dropped: SDL already finds every pad, and naming one adds it a second time (CMR3) |
| `REA_SDL_SETTINGS` | `1` | give the window the same SDL variables |
| `REA_GPU_ICD_PIN` | `0` | `1`: the SRR Vulkan ICD rule instead of NVIDIA's OpenGL offload (see `rea_gpu_env`) |
| `REA_GPU` | computed | the GPU choice as `NAME=VALUE` words. Preset it (even empty) to skip detection |
| `REA_DLL_OVERRIDES` | empty | the game's `WINEDLLOVERRIDES`, placed before the caller's (the AppImage's, the player's) |
| `REA_WINEDBG_OFF` | `0` | `1`: add `winedbg=d`, so a fault ends the game instead of freezing it in a hidden debugger (CMR3) |
| `REA_WINEBUS_MARKER`, `REA_WINEBUS_VALUES` | `.rea-winebus`, `DisableHidraw\|DisableInput\|Enable SDL\|Map Controllers` | |

### Test seams (only with `REA_TEST=1`)

`REA_SYSFS` (replaces `/sys`), `REA_ICD_DIRS` (replaces
`/etc/vulkan/icd.d /usr/share/vulkan/icd.d`), `REA_NV_LIB_CHECK` (a command
run instead of the `libGLX_nvidia` lookup), `REA_GAMESCOPE_CHECK`,
`REA_LAUNCH_CMD` (replaces the whole supervised launch), `REA_PROC`
(replaces `/proc` for `rea_game_alive`). They are honoured only when a test
sets `REA_TEST=1` after sourcing; like every `REA_*` variable they are
dropped from the environment at source time, and the inner script receives
the library's own `REA_TEST` (empty outside tests). `REA_SCREEN` is a
normal setting (Screamer maps `SCREAMER_SCREEN` to it).

## Hooks (override after sourcing)

| Hook | Called | Use |
|---|---|---|
| `rea_hook_before_settings` | before the window | prefix bootstrap the window needs (SRR's winebus) |
| `rea_hook_before_start` | in `rea_start`, before the inner script is written | prefix prep (`rea_wine_no_crash_dialog`, `rea_seed_winebus`, `rea_once`), game-only env (`rea_game_env`). `REA_GAME_ENV` is reset just before this hook, so call `rea_game_env` here |
| `rea_hook_command "$@"` | for every non-empty command line, before the built-ins | return 127 when the command is not handled. It may call `rea_cmd_default`, `rea_cmd_play` or `exec "$REA_EXE" "$@"`. Use `${1:-}` under `set -u` |
| `rea_hook_usage` | after the usage lines | extra lines |

## Functions

- Plumbing: `rea_defaults` (called by `rea_init`), `rea_seam NAME` (a
  test seam's value under `REA_TEST=1`), `rea_run CMD...` (through
  `REA_RUN`, split without globbing), `rea_noglob`/`rea_glob`,
  `rea_save_traps`/`rea_restore_traps`.
- Config: `rea_init`, `rea_find_exe`, `rea_require_exe`, `rea_find_cfg`,
  `rea_get_key KEY [FILE]`. In `rea_get_key`, the last active line wins,
  matching the launchers' parser, which applies lines in order and so
  matches `set_key`'s file. Keys and values are trimmed of spaces, tabs and
  CR. `#` lines are comments. The key is compared as text, never used as a
  pattern.
- Sizes: `rea_parse_size TEXT` prints `WxH`. It accepts `1920X1080`,
  spaces, CRLF and leading zeros. It rejects `0`, half sizes, extra parts,
  letters, `$()` and more than 5 digits. Also `rea_screen_size`,
  `rea_game_size` (sets `REA_W` and `REA_H`) and `rea_scaler` (sets
  `REA_F`).
- GPU: `rea_gpu_env`, `rea_gpu_init`, `rea_has_nv_lib`,
  `rea_nvidia_present`. The caller's `DRI_PRIME` or
  `__NV_PRIME_RENDER_OFFLOAD` always wins; in pin mode, so do
  `VK_DRIVER_FILES` and `VK_ICD_FILENAMES`. Pin mode pins NVIDIA's ICD
  json(s) as both `VK_DRIVER_FILES` and `VK_ICD_FILENAMES`, and only when
  three things hold: an NVIDIA ICD exists, another vendor's ICD exists,
  and an NVIDIA device is present (PCI vendor `0x10de` or the `nvidia`
  module). Otherwise, with NVIDIA's driver and `libGLX_nvidia`, it sets
  NVIDIA's render offload (not in pin mode). With several Mesa GPUs where
  the screen's one is integrated, it sets `DRI_PRIME=1`.
- Environment: `rea_sdl_env`, `rea_dll_overrides`,
  `rea_game_env NAME VALUE`. `rea_game_env` sets a variable for the game
  only: the inner script reads `game.env` as data, so gamescope never
  loads a vkBasalt layer.
- Wine: `rea_wine` (`wine` in the prefix, with the GPU choice),
  `rea_wineserver`, `rea_wineserver_wait`, `rea_once MARKER CMD...` (runs
  CMD until it succeeds, then never again; the marker lives in the prefix,
  or in the state folder in native mode), `rea_wine_no_crash_dialog` (once:
  checks `user.reg`, then waits for the wineserver), `rea_seed_winebus`
  (values written only when absent, under the names `winebus.sys` reads,
  once per prefix, then `wineserver -w`).
- Session: `rea_proc_stat`, `rea_read_lock`, `rea_session_valid` (pid,
  session leader and start time must all match), `rea_legacy_lock`,
  `rea_running`, `rea_launch [NAME=VALUE...] CMD...` (setsid, lock, wait),
  `rea_launch_interrupted` (TERMs the group, runs `wineserver -k`, waits,
  removes the lock, exits 143), `rea_stop`, `rea_status`,
  `rea_game_alive`.
- Inner script: `rea_write_inner` writes a quoted heredoc with no
  interpolation. The launcher is `$1` and its flags follow. Its settings
  arrive as environment (`REA_INNER_MODE`, `REA_GAME_ENV_FILE`,
  `REA_GAME_PROCS`, `REA_GAME_SETTLE`). In Wine mode it runs `wine "$@"`,
  optionally waits for the game, then runs `wineserver -k` (also on
  INT/TERM/HUP). In native mode it runs `exec "$@"`.
- Gamescope: `rea_has_gamescope`, `rea_windowed`, `rea_use_gamescope`.
- Steps and commands: `rea_settings`, `rea_start`, `rea_cmd_default`,
  `rea_cmd_play`, `rea_usage`, `rea_main`.

While `rea_launch` waits, its own EXIT/HUP/INT/TERM traps replace the
caller's. The caller's traps are saved first and put back when the launch
returns, and also before an interrupted launch exits 143, so the caller's
EXIT trap still runs. Two limits: `trap` output is read through a file
(dash prints nothing for `$(trap)`), and only a top-level shell gets its
traps back. Inside a `( subshell )`, bash reports the parent's traps,
which are not active there, so re-arming them would be wrong; a subshell
gets the defaults back, as the launchers' `launch` did everywhere.

## Game differences kept as settings

| Difference | CMR3 | CMR4 | SRR | OR | SCR |
|---|---|---|---|---|---|
| `REA_MODE` | wine | wine | wine | wine | **native** |
| `REA_STATE_BASE` | `~/.config` | `~/.config` | `~/.config` | `~/.config` | **`${XDG_CONFIG_HOME:-~/.config}`** |
| wait (`REA_ALWAYS_WAIT`) | **0** + `CMR3_WAIT` | 1 | **0** + `SRR_WAIT` | 1 | 1 |
| gamescope (`REA_GAMESCOPE`) | auto | auto | **off** (DXVK lands on the iGPU inside gamescope on hybrid hosts) | auto | auto |
| `REA_DEFAULT_SIZE` | 1920x1080 | **1024x768** | — | 1920x1080 | 1920x1080 |
| `REA_ORIGINAL_SIZE` | 640x480 | *(empty → default)* | — | 640x480 | *(empty → default)* |
| `REA_ORIGINAL_WORDS` | original | original | — | original (`off` stays 1920x1080, as today) | original |
| `REA_FULLSCREEN_SIZE` | — | — | — | — | **screen** (+ `REA_SCREEN=${SCREAMER_SCREEN:-}`) |
| scaler | `scaler`, linear | **fixed nis** (`REA_SCALER_KEY=`) | — | `scaler`, linear | `scaler`, **fsr** |
| `REA_GAMESCOPE_OUTPUT` | 1 | 1 | — | 1 | **0** |
| `REA_WINDOWED_KEY` | **display_mode** | — | — | — (OR has no desktop window mode, so it never skips) | — |
| `REA_SDL_ENV` | `SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS=1 SDL_JOYSTICK_HIDAPI=0` | default | `SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS=1` | default | **empty** |
| `REA_SDL_SETTINGS` | 0 | 1 | 0 | 1 | — |
| `REA_GPU_ICD_PIN` | 0 | 0 | **1** | 0 | 0 |
| `REA_DLL_OVERRIDES` / `REA_WINEDBG_OFF` | `dinput8,dsound,d3d9=n,b` / 1 | — | — | — | — |
| `REA_PLAY_ARGS` | `--no-window` | `--no-window` | `--no-window --linux` | `--no-window` | `--no-window` |
| `REA_GAME_PROCS` | `Rally_3PC.exe` | — | `SEGA Rally.exe\|SEGA Rally_SSE1` | — (waiting on the game's process raced Wine start-up and killed the launcher) | — |
| `REA_LEGACY_LOCK` | signal | **off** (never shipped a pid-only lock) | signal | **keep** (its old lock held the helper's own pid) | signal |
| hooks | before_start: crash dialog + postfx (`rea_game_env`) | — | before_settings + before_start: winebus (`REA_WINEBUS_MARKER=.srr-winebus-v2`) + winetricks | — | command: game selectors, pass-through to the launcher |

## Deliberate changes from the five scripts

Each of these is either a fix one launcher already had, or something that
only changes invalid input:

- **Config next to the exe**: only the exe's own folder, first match wins.
  CMR3 also checked the parent folder, and that copy won; no launcher reads
  it there. CMR4, SRR and OR never checked next to the exe, although their
  launchers prefer that file. SCR checked only the state folder, although
  its launcher checks next to the exe first.
- **Environment**: inherited `REA_*` values are ignored (they did not
  exist before); test seams need `REA_TEST=1`.
- **Sizes**: `0x0`, `0x1080`, more than 5 digits and leading zeros are now
  handled everywhere, not only in SCR. All launchers now lowercase the
  value, not only OR and SCR.
- **Unknown scaler values** become the default instead of reaching
  gamescope.
- **Interrupt**: OR's extra `wineserver -k` now runs for every Wine game
  (`REA_INTERRUPT_WINESERVER=0` turns it off). Every launcher waits for
  the session before it exits.
- **`settings.log`** and the exit-code messages (SRR) apply to every
  launcher; the others discarded stderr.
- **CMR3's process wait** is zombie-aware (SRR's `game_alive`).
- **Game-only environment** goes through a data file rather than CMR3's
  `CMR3_VK_*` relay variables. `WINEDLLOVERRIDES` is passed to the game
  session only, never exported into the helper's shell.
- `GPU: …` is printed only for runs that start something, not for `stop`
  or `status`. "Launcher not found" goes to stderr.
- The direct and gamescope paths both run `sh inner.sh …`. Native mode
  also goes through the inner script, which `exec`s the launcher, so
  native games can use `rea_game_env` too.

## Migration guide

### Function map

| Launcher function / variable | Library |
|---|---|
| `HERE`, `PREFIX`, `LAUNCHER`, `RUN`, `STATE`, `CFG`, `LOCK`, `GPU`, `pid` | `REA_HERE`, `REA_PREFIX`, `REA_EXE`, `REA_RUN`, `REA_STATE`, `REA_CFG`, `REA_LOCK`, `REA_GPU`, `REA_PID` |
| launcher discovery loop, CFG discovery loop | `REA_EXE_NAME` (+ `REA_EXE_SEARCH`), `REA_CFG_APPDATA` |
| `get_key` | `rea_get_key` |
| `proc_stat`, `read_lock`, `session_valid` | `rea_proc_stat`, `rea_read_lock`, `rea_session_valid` |
| `legacy_pid` / `legacy_session` / `old_lock_alive` / `legacy_lock_live` | `rea_legacy_lock` + `REA_LEGACY_LOCK` |
| `running` | `rea_running` |
| `gpu_env`, `has_nv_lib`, `nvidia_present` | `rea_gpu_env`, `rea_has_nv_lib`, `rea_nvidia_present` |
| `has_gamescope`, `screen_size` | `rea_has_gamescope`, `rea_screen_size` |
| `launch`, `launch_interrupted` | `rea_launch`, `rea_launch_interrupted` |
| `stop_session` | `rea_stop` |
| `start` (size parsing, inner heredoc, gamescope choice) | `rea_start` + the size/scaler/gamescope variables |
| `settings` | `rea_settings` |
| the `case "${1:-}"` dispatch | `rea_main` (+ `rea_hook_command`) |
| CMR3 `dll_overrides` | `REA_DLL_OVERRIDES` + `REA_WINEDBG_OFF` (`rea_dll_overrides`) |
| CMR3 `prepare_prefix` | `rea_wine_no_crash_dialog` in `rea_hook_before_start` |
| CMR3 `display_mode` | `REA_WINDOWED_KEY=display_mode` |
| CMR3 `rea_hook_command` + `one_run_cfg` (one-run `--scaler/--postfx/--resolution/--display-mode/--dxvk`) | delete both; set `REA_HELPER_KEYS="scaler postfx resolution display_mode dxvk"`. `--play`/`play`/`start` then reach `rea_cmd_play` directly, and `one-run.cfg` is no longer written |
| CMR3 `postfx_env`, `grade` | stay in cmr3-play, called from `rea_hook_before_start`. Replace the three `export CMR3_VK_*` lines with `rea_game_env VK_LAYER_PATH …`, `rea_game_env VK_INSTANCE_LAYERS …`, `rea_game_env VKBASALT_CONFIG_FILE …`. `$HERE` becomes `$REA_HERE`, `$STATE` becomes `$REA_STATE`, `$LAUNCHER` becomes `$REA_EXE` |
| SRR `GAME_ALIVE` / `game_alive`, `SRR_PROC` | `REA_GAME_PROCS`, `rea_game_alive`, `REA_PROC` |
| SRR `prefix_wine` | `rea_wine` |
| SRR `seed_winebus` | `rea_seed_winebus` with `REA_WINEBUS_MARKER=.srr-winebus-v2` (existing prefixes are not seeded again) |
| SRR `seed_winetricks`, `bootstrap` | keep `seed_winetricks` as a game function under `rea_once .srr-tricks seed_winetricks`. It must return non-zero when it skips (winetricks missing, `SRR_WINETRICKS=off`), so it runs again next time, and 0 after winetricks ran, as before. Call `rea_seed_winebus` and it from both `rea_hook_before_settings` and `rea_hook_before_start` |
| SCR `SCREAMER_NV_LIB_CHECK`, `SCREAMER_GAMESCOPE_CHECK`, `SCREAMER_LAUNCH_CMD`, `SCREAMER_SYSFS`, `SCREAMER_SCREEN` | `REA_NV_LIB_CHECK`, `REA_GAMESCOPE_CHECK`, `REA_LAUNCH_CMD`, `REA_SYSFS`, `REA_SCREEN` (map them in the script if the old names must keep working) |
| `<X>_SYSFS` (CMR3/CMR4/OR/SRR) | `REA_SYSFS` |

### Per launcher

**CMR3** (`cmr3-play`):
`REA_APP=cmr3-launcher REA_EXE_NAME=CMR3Launcher.exe REA_CFG_APPDATA=CMR3Launcher REA_ALWAYS_WAIT=0 REA_WAIT=${CMR3_WAIT:-} REA_ORIGINAL_SIZE=640x480 REA_WINDOWED_KEY=display_mode REA_SDL_ENV="SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS=1 SDL_JOYSTICK_HIDAPI=0" REA_SDL_SETTINGS=0 REA_DLL_OVERRIDES='dinput8,dsound,d3d9=n,b' REA_WINEDBG_OFF=1 REA_GAME_PROCS=Rally_3PC.exe`.
Hook: `rea_hook_before_start() { rea_wine_no_crash_dialog; postfx_env; }`.
Keep `postfx_env`, `grade` and `tests/postfx.sh` in the launcher.

**CMR4** (`cmr4-play`):
`REA_APP=cmr4-launcher REA_EXE_NAME=CMR4Launcher.exe REA_CFG_APPDATA=CMR4Launcher REA_DEFAULT_SIZE=1024x768 REA_SCALER_KEY= REA_DEFAULT_SCALER=nis REA_LEGACY_LOCK=off`.
No hooks. `CMR4_WAIT` maps to `REA_WAIT`, which has no effect because every launch waits.

**SRR** (`srr-play`):
`REA_APP=srr-launcher REA_EXE_NAME=SRRLauncher.exe REA_GAMESCOPE=off REA_GPU_ICD_PIN=1 REA_PLAY_ARGS="--no-window --linux" REA_GAME_PROCS='SEGA Rally.exe|SEGA Rally_SSE1' REA_ALWAYS_WAIT=0 REA_WAIT=${SRR_WAIT:-} REA_SDL_ENV=SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS=1 REA_SDL_SETTINGS=0 REA_WINEBUS_MARKER=.srr-winebus-v2 REA_DIRECT_NOTE="starting (isolated Wine desktop, frame limit via DXVK)"`.
Hooks: winebus + winetricks before settings and before start.

**OR** (`or-play`):
`REA_APP=or-launcher REA_EXE_NAME=OutRunLauncher.exe REA_CFG_APPDATA=OutRunLauncher REA_ORIGINAL_SIZE=640x480 REA_LEGACY_LOCK=keep`.
No hooks. Never set `REA_WINDOWED_KEY` or `REA_GAME_PROCS`.

**SCR** (`screamer-play`), keeping today's CLI exactly (tested in
`tests/dispatch.sh`):

```sh
#!/bin/sh
set -u
_fonly=${REA_FUNCTIONS_ONLY:-}
_here=$(dirname "$(readlink -f "$0")")
[ "$_fonly" = 1 ] && [ -n "${REA_PLAY_HERE:-}" ] && _here=$REA_PLAY_HERE
if [ -f "$_here/rea-play.sh" ]; then . "$_here/rea-play.sh"
elif [ -f "$_here/../sdk/rea-play.sh" ]; then . "$_here/../sdk/rea-play.sh"
else echo "rea-play.sh not found next to $0 or in ../sdk" >&2; exit 1; fi
REA_HERE=$_here REA_NAME=screamer-play
REA_MODE=native REA_APP=screamer-launcher REA_EXE_NAME=ScreamerLauncher
REA_EXE_HINT=SCREAMER_LAUNCHER REA_EXE=${SCREAMER_LAUNCHER:-}
REA_EXE_SEARCH="../target/release/ScreamerLauncher ../target/debug/ScreamerLauncher"
REA_STATE_BASE=${XDG_CONFIG_HOME:-$HOME/.config}
REA_GAMESCOPE_OUTPUT=0 REA_DEFAULT_SCALER=fsr REA_FULLSCREEN_SIZE=screen
REA_SCREEN=${SCREAMER_SCREEN:-} REA_SDL_ENV= REA_WAIT=${SCREAMER_WAIT:-}
rea_hook_command() {
  case "${1:-}" in
    --play|start|status|stop) return 127 ;;   # the helper's own commands
    1|2|3|s1|s2|sr|screamer|screamer1|screamer2|rally|screamer-rally) rea_cmd_default "$@" ;;
    *) rea_require_exe || exit 1; exec "$REA_EXE" "$@" ;;   # --help, --check, ...
  esac
}
rea_init || exit 1
[ "$_fonly" = 1 ] && return 0
rea_main "$@"
```

The `REA_EXE_SEARCH` entries are relative, so they are taken from the
script's folder. Test seams map as `SCREAMER_NV_LIB_CHECK` →
`REA_NV_LIB_CHECK` and so on, under `REA_TEST=1`.

`SCREAMER_DOSBOX` and `SCREAMER_GAME_DIR` stay plain environment for the
launcher.

### Tests for a game script

One convention for every launcher's `linux/tests/*.sh`:

- `REA_FUNCTIONS_ONLY=1` makes the game script load (library, config,
  hooks, `rea_init`) and `return` before `rea_main`.
- `REA_PLAY_HERE=<the script's folder>` replaces `dirname "$0"` for the
  library lookup and `REA_HERE`, because `$0` is the test when a test sources
  the script. It counts only together with `REA_FUNCTIONS_ONLY=1`; a
  normal run ignores it.
- The library drops both once sourced (like every `REA_*` variable), so
  they never reach the game's processes. Never set them outside tests: with
  `REA_FUNCTIONS_ONLY=1` the helper does nothing.

```sh
# linux/tests/<x>.sh
HERE=$(dirname "$(readlink -f "$0")")
REA_FUNCTIONS_ONLY=1 REA_PLAY_HERE=$HERE/.. . "$HERE/../<x>-play"
REA_TEST=1 REA_SYSFS=$T/nosys REA_RUN=$T/fake-run REA_GPU=   # seams + fakes
rea_start > /dev/null
```

In a repo checkout this finds `linux/../sdk/rea-play.sh`; in a package,
`<dir>/rea-play.sh`. `shell/tests/example.sh` checks the convention in
both layouts, and `shell/tests/dispatch.sh` checks it for the Screamer
recipe.

To adopt it, each launcher replaces its own hook:

1. In `linux/<x>-play`, replace the lines before the library lookup with
   the three convention lines from "Where the library lives". Remove
   CMR3's `CMR3_PLAY_HERE` branch, Screamer's `${SCREAMER_PLAY_DIR:-…}` and
   SRR's `../../sdk` fallback.
2. Right after the lookup, add `REA_HERE=$_here REA_NAME=<x>-play`.
   Without it, a sourced script's `REA_HERE` would be `linux/tests`, and
   launcher discovery and messages would be wrong.
3. Replace `[ -n "${<X>_PLAY_FUNCTIONS_ONLY:-}" ] && return 0` with
   `[ "$_fonly" = 1 ] && return 0`. It stays after `rea_init` and the hooks.
4. In `linux/tests/*.sh`, replace `<X>_PLAY_FUNCTIONS_ONLY=1` and
   `CMR3_PLAY_HERE=…` / `SCREAMER_PLAY_DIR=…` with
   `REA_FUNCTIONS_ONLY=1 REA_PLAY_HERE=$HERE/..` on the `.` line. Then set
   `REA_TEST=1` before using any seam.
5. CMR4 runs `sh -c` with `CMR4_PLAY_FUNCTIONS_ONLY=1`; OR re-sources `"$0"`
   with `OR_PLAY_FUNCTIONS_ONLY=1`. Both use the same pair:
   `REA_FUNCTIONS_ONLY=1 REA_PLAY_HERE=<linux dir> sh -c '. "$1"; …' sh <script>`.
   Tests that run the script normally (`sh <x>-play status`) need neither.
6. Re-run `scripts/sdk-sync` to vendor the new `sdk/rea-play.sh`, and grep the
   repo for `_PLAY_FUNCTIONS_ONLY`, `_PLAY_HERE` and `_PLAY_DIR`; nothing
   should remain.

The library's tests cover the shared behaviour. A launcher keeps tests only
for its own configuration (one `gamescope`/`--play` command assertion is
enough to catch a wrong variable) and for its game hooks (CMR3 `postfx.sh`,
SRR `winetricks`).

## Tests

```sh
sh shell/tests/run-all.sh                      # sh, plus dash/busybox when installed
REA_TEST_SHELLS="dash" dash shell/tests/run-all.sh
```

Suites: `config.sh` (discovery, `rea_get_key`, sizes, screen),
`gpu-env.sh`, `gamescope.sh` (filter, size, skips, environment, game-only
env), `supervision.sh` (lock, legacy locks, stop, background and waiting
launches, interrupt, inner script, settings exit codes, native mode),
`prefix.sh` (crash dialog, DLL overrides, `rea_once`, winebus),
`game-alive.sh`, `dispatch.sh` (`rea_main` and the hooks), `example.sh`
(`example-play` in both layouts). Wine, gamescope and the game are always
fakes on `PATH` or `REA_RUN`. Only harmless `sleep`s run.
