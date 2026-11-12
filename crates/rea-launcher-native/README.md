# rea-launcher-native

FFI that only **native** launchers need: emulator wrappers (a bundled MAME
or DOSBox) and native ports. The Wine-run launchers (i686) don't use it;
they keep `rea-launcher-win32`. From rea-rad-mobile's `child.rs` and
`input/{sdl,haptic}.rs`.

`unsafe` is allowed here (docs/STANDARDS.md, "Code"), and every `unsafe`
block sits inside a safe type. The decisions around the FFI are pure and
tested on any host in `policy`.

| Module | Feature | What it does |
|---|---|---|
| `child` | `child` (default) | `spawn_tied`: the child dies with the launcher (`PR_SET_PDEATHSIG` on Linux, a kill-on-close job object on Windows) |
| `sdl` | `sdl` | `Sdl` (hints, no video), `Devices` (rescan throttle that keeps plug events, spare interfaces skipped, force feedback of the wheel in use), `Device` (axes, buttons, hat, the pad view of a pad, `nav_bits`, rate-limited `rumble`), `Haptic` (constant + damper + friction, keepalive, revive) |
| `policy` | always | `Rescan`, `Retry`, `RumbleLimit`, `HapticPolicy`, `effect_params` ("from the west") |

The pure halves live in `rea-launcher`: `wheel` (known wheels,
`classify`, `pick_wheel`, profiles, the setup wizard, pad maps) and `ffb`
(synthetic force feedback). See docs/playbooks/wheels-and-ffb.md.

## Use it

```toml
[dependencies]
rea-launcher-native = { git = "https://github.com/akitaonrails/rea-launcher-sdk", tag = "v0.8.0" }
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
- `scripts/wine-test --target x86_64-pc-windows-gnu -p rea-launcher-native`
  runs the job-object test under Wine.

## Migration

New in v0.8.0; nothing to migrate. rea-rad-mobile moves onto it by
replacing `child.rs` and `input/{sdl,haptic}.rs`. Behaviour that changes
for its players: a Logitech or Thrustmaster *pad* is a pad now (it was a
900° wheel), pad rumble is refreshed every 50 ms instead of every tick, a
device that refuses force feedback is retried every 5 s instead of every
tick, `Device::rumble` takes `&mut self` and no duration, and the hint
`SDL_JOYSTICK_HIDAPI_WHEELS` (not an SDL2 hint) is no longer set.
