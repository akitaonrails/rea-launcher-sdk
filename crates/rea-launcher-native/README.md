# rea-launcher-native

FFI that only **native** launchers need: emulator wrappers (a bundled MAME
or DOSBox), native ports and the TeknoParrot hubs (loaders and emulators
in gamescope). The Wine-run launchers (i686) don't use it; they keep
`rea-launcher-win32`. From rea-rad-mobile's `child.rs` and
`input/{sdl,haptic}.rs`, and the hubs' `proc.rs`, `win32.rs`, `session.rs`
and `input.rs`.

`unsafe` is allowed here (docs/STANDARDS.md, "Code"), and every `unsafe`
block sits inside a safe type. The decisions around the FFI are pure and
tested on any host in `policy`.

| Module | Feature | What it does |
|---|---|---|
| `child` | `child` (default) | `spawn_tied`: the child dies with the launcher (`PR_SET_PDEATHSIG` on Linux, a kill-on-close job object on Windows) |
| `sdl` | `sdl` | `Sdl` (hints, no video), `Devices` (rescan throttle that keeps plug events, spare interfaces skipped, force feedback of the wheel in use), `Device` (axes, buttons, hat, the pad view of a pad, `nav_bits`, rate-limited `rumble`), `Haptic` (constant + damper + friction, keepalive, revive) |
| `group` | `group` | A game session's processes as one: `Group::spawn(cmd, &Tag)`, `terminate` (Linux `SIGTERM` to the group, Windows `WM_CLOSE` to the leader's windows), `kill`, `any_left`, `exited`/`wait_exit` (no reaping), `finish` (reap, kill and count the leftovers); Linux `terminate_one`, `members`; Windows `windows` (`Window::hide`/`focus`/`close`), `kill_under`, `Tag::cpus`, `logical_processor_information` |
| `supervise` | `group` | `run(group, feed, watcher, Options)`: one game run, 10 ms poll, output to the log and a `Watcher`, stop → polite stop → grace → kill, leftovers; `Feed` and `Hold` (input kept open), `Outcome::ended_early` |
| `feeder` | `feeder` (`sdl` + `group`) | The hubs' input thread: `InputService::start(pad_map, paddles)`, `Command::{Attach, Output, Detach}`, `Wire` (a game's line format), `Ffb`, `Tuning`, `Snapshot`, `find_wheel`; feeds `Feeding` and `Scripted` |
| `policy` | always | `Rescan`, `Retry`, `RumbleLimit`, `HapticPolicy`, `effect_params` ("from the west") |

The pure halves live in `rea-launcher`: `wheel` (known wheels,
`classify`, `pick_wheel`, profiles, the setup wizard, pad maps, `Script`),
`ffb` (synthetic force feedback) and `procs` (finding a session's
processes). See docs/playbooks/wheels-and-ffb.md.

## Use it

```toml
[dependencies]
rea-launcher-native = { git = "https://github.com/akitaonrails/rea-launcher-sdk", tag = "v0.8.0" }
# or, for a TeknoParrot-style hub: features = ["group"] (processes) or ["feeder"] (also SDL input)
# or, with SDL (needs libsdl2-dev to build on Linux, SDL2.dll beside the exe on Windows):
# rea-launcher-native = { git = "…", tag = "v0.8.0", features = ["sdl"] }
```

## Examples

### child

```rust,no_run
use std::process::Command;
# fn main() -> std::io::Result<()> {
// Spawn from a thread that outlives the child (PDEATHSIG follows the
// spawning thread on Linux): here, the main thread, which waits.
let mut mame = Command::new("mame/radmobile");
let status = rea_launcher_native::child::spawn_tied(&mut mame)?.wait()?;
# let _ = status; Ok(()) }
```

### group and supervise

A session the way the TeknoParrot hubs run one: gamescope as the leader,
the session's tag on every process, the loader's input held open until
the stop, F355-style game knowledge in a `Watcher` of the launcher's own.

```rust,no_run
# #[cfg(feature = "group")] fn main() -> std::io::Result<()> {
use std::process::{Command, Stdio};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use rea_launcher_native::group::{Group, Tag};
use rea_launcher_native::supervise::{self, Hold, Options};

let mut cmd = Command::new("gamescope");
cmd.args(["--", "arcade-launcher", "run"])
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
// spawned on the thread that runs the session (Linux ties it to this thread)
let group = Group::spawn(&mut cmd, &Tag::new("SEGA_SESSION", "/data/runtimes/f355"))?;
let stop = AtomicBool::new(false); // the window's Stop sets it
let log = Arc::new(Mutex::new(std::fs::File::create("session.log")?));
let opts = Options { stop: &stop, log, grace: Duration::from_secs(10), sink: None };
let out = supervise::run(group, &mut Hold::default(), &mut (), opts)?;
if out.ended_early(Duration::from_secs(20)) {
    eprintln!("the game closed by itself: see session.log");
}
# Ok(()) }
# #[cfg(not(feature = "group"))] fn main() {}
```

### feeder

```rust,no_run
# #[cfg(feature = "feeder")] fn main() {
use rea_launcher::wheel::{Control, PadMap};
use rea_launcher_native::feeder::{Feeding, Frame, InputService, Tuning, Wire};

# #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)] enum Button { Start }
# impl Control for Button {
#     const ALL: &'static [Button] = &[Button::Start];
#     fn key(self) -> &'static str { "start" }
#     fn label(self) -> &'static str { "Start" }
#     fn bit(self) -> u32 { 1 }
# }
/// windowsarcadeloader's stick line (the launcher's button table).
struct Wal;
impl Wire for Wal {
    fn encode(&mut self, f: &Frame) -> String {
        let c = f.controls;
        format!("p1 {:x} {} 0 0 0 {} {}\n", c.buttons, c.steer, c.accel, c.brake)
    }
    fn quit(&self) -> Option<&'static str> { Some("quit") }
}
let input = InputService::start(PadMap::<Button>::default(), None);
// one run's feed, handed to supervise::run
let feed = Feeding::new(input.sender(), Box::new(Wal), Tuning::default());
# let _ = feed; }
# #[cfg(not(feature = "feeder"))] fn main() {}
```

### policy

```rust
use rea_launcher_native::policy::{EffectKind, effect_params};
// SDL gives the direction a force comes from: a push to the right comes
// from the west. rea-rad-mobile 0.4.0 shipped the mirror image.
let push_right = effect_params(EffectKind::Constant, 0.5);
assert!(push_right.direction[0] < 0 && push_right.level > 0);
```

## Testing

- `cargo test -p rea-launcher-native` (the `child` test kills a parent and
  checks its child is gone; an untied child is the control).
- `cargo test -p rea-launcher-native --features sdl` starts SDL headless.
- `cargo test -p rea-launcher-native --features group` (Linux): a stand-in
  gamescope (a shell script) starts its game in a session of its own; the
  stop finds and kills it, a killed launcher takes the session with it,
  and decoy processes the test owns (untagged, or tagged for another
  session) are never signalled. `supervise` runs shell scripts as games:
  the log, the feed, the input held open, the grace, the watcher.
- `cargo test -p rea-launcher-native --features feeder`: the input thread
  with and without SDL (its device loop only on a machine without
  controllers: it would play force feedback on a real wheel), a broken
  input let go, the two feeds.
- `scripts/wine-test --target x86_64-pc-windows-gnu -p rea-launcher-native
  --features group` runs the job-object tests under Wine: the leader's
  first child is held by the job, `finish` ends what the leader left,
  `kill_under` spares a process started before the session, and the polite
  stop closes a window that saves first (`terminate`, `supervise::run`).

## Migration

v0.10.0: `group`, `supervise` and `feeder` are new, behind features of
their own; nothing changes for a launcher that doesn't turn them on. A
TeknoParrot hub replaces its `proc.rs`, `win32.rs`, the run loop of
`session.rs` and `input.rs` with them; what its players see change: a
killed launcher takes its Linux game along, a stop on Windows asks
Flycast and Dolphin to close their windows (they save) instead of waiting
8 s and ending them, `--no-input` keeps windowsarcadeloader's input open
(it quit at once), the force feedback is released when the game's input
breaks, a failed job attach is a `# …` warning in the session log instead
of a failed start, the leftover count leaves out zombies, a script's
line is written every 200 ms too, not only on a change, and a run without
a feed logs windowsarcadeloader's `out` lines.

New in v0.8.0; nothing to migrate. rea-rad-mobile moves onto it by
replacing `child.rs` and `input/{sdl,haptic}.rs`. Behaviour that changes
for its players: a Logitech or Thrustmaster *pad* is a pad now (it was a
900° wheel), pad rumble is refreshed every 50 ms instead of every tick, a
device that refuses force feedback is retried every 5 s instead of every
tick, `Device::rumble` takes `&mut self` and no duration, and the hint
`SDL_JOYSTICK_HIDAPI_WHEELS` (not an SDL2 hint) is no longer set.
