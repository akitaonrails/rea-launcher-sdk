# rea-launcher-dinput

The in-game DirectInput wheel layer, for old games that read wheels
through DirectInput 8 themselves (first users: Colin McRae Rally 3, 04
and 2005, all 32-bit, all `dinput8!DirectInput8Create` with
`IDirectInput8A`). A DLL in the game shows it **one virtual wheel**,
"rea wheel", made from the player's wheel and, when they are a separate
USB device, their pedals:

- steering after the steering lock, deadzone and curve, so a 900–1080°
  wheel turns like the lock the player picked;
- throttle and brake on axes of their own, and combined on one axis for
  games that bind one;
- the wheel's buttons, then the pedal device's, and the wheel's hat;
- force feedback: the game's effects are made on the real wheel at the
  player's strength, and come back when the wheel does.

The game sees one device with a layout that never changes, whatever
wheel the player has, so a launcher can seed the game's control settings
once per game instead of once per wheel model. The games can only use
one device per player: separate pedals work only through this merge.

## How it works

The launcher adds `rea_dinput.dll` to the import table of its runtime
copy of the game (`rea_dinput::import_into`). When the DLL loads it
points the executable's `dinput8!DirectInput8Create` import at its own
(the loader has resolved the import by then; verified under Wine).
That calls the function the import held — the system's DirectInput, or
Xidi's `dinput8.dll` in the game's folder — and wraps the
`IDirectInput8A` it made:

- `EnumDevices` lists the virtual wheel **first** (for all devices, game
  controllers and driving devices) and leaves out the devices it is made
  of: the wheel, the pedals, the wheel's twin HID collections without
  force feedback (Fanatec on Windows), and spare interfaces of either.
- `CreateDevice` of the virtual wheel's GUID makes it; every other GUID,
  and every other call, goes to the wrapped DirectInput.

The wheel and pedals are chosen, read and turned into the game's values
by `rea_launcher::wheel::rig`, the same functions the launcher's Wheel
tab uses, so the tab's live check is what the game gets. Real devices
are read as the launcher reads them: `c_dfDIJoystick2`, axes in
`dinput::REAL_AXES` order at `REAL_RANGE`.

The wheel and pedals are opened once, when the game creates
DirectInput, and shared by every virtual device it makes from it and by
their effects. The layer acquires them itself: the wheel exclusive and
in the background on the game's window (force feedback needs exclusive
access), pedals shared. A wheel another program holds exclusively is
acquired shared: it steers, without force feedback. A lost wheel
(unplugged) steers centred while separate pedals still drive, and is
acquired again every 500 ms. With `mode = on` and no wheel at the
start, the devices are listed again every 2 s until one is plugged in.

**Off** without a settings file, with `mode = off`, (`auto`) without a
wheel attached when the game starts, or when the picked wheel can't be
opened: the game gets its DirectInput untouched, the same object it
would have had, and nothing is hidden. A broken file keeps the defaults
it can't read. A panic inside the layer becomes an error code for that
one call, never a crash in the game — **when the DLL is built to
unwind** (`--profile dll`, see "Ship and enable it").

## Settings: `rea_wheel.ini`

Next to the DLL; written by the launcher's Wheel tab
(`rea_launcher::wheel::dinput::Setup` and the calibration,
`dinput::Profile`, in one file).

| Key | Values | Default |
|---|---|---|
| `mode` | `auto` (when a wheel is attached), `on` (always; a centred wheel without one), `off` | `auto` |
| `lock` | steering lock in degrees (180, 270, 360, 540, 900) or 0 for the wheel's whole rotation | 270 |
| `deadzone` | steering deadzone, percent, 0..25 | 0 |
| `linearity` | steering curve, 0 (linear) .. 100 (cubic) | 0 |
| `invert_accel`, `invert_brake` | pedal reads the other way | off |
| `device` | the wheel picked by hand (instance GUID); empty picks it | |
| `pedals` | the pedal device (instance GUID); empty: the calibrated one, else on the wheel | |
| `ffb_strength` | force feedback, percent | 50 |
| `ffb_invert` | force feedback the other way (last resort) | off |
| `log` | write `rea_wheel.log` next to the DLL (the game's calls, devices, effects, errors; 1 MiB at most) | off |
| `wheel_*`, `aux_*`, `steer_axis`, `rotation`, `gas_*`, `brake_*` | the calibration (`WheelProfile`) | guessed |

`REA_WHEEL_MODE=on|off` in the game's environment overrides `mode`
(`rea_dinput::MODE_ENV`): the launcher decides `auto` itself before it
starts the game (`rig::shows_virtual_wheel`), seeds the game's controls
only when the wheel will be there, and passes its decision, so both
sides agree. Anything else in the variable leaves the file's mode.

Without a calibration for the picked wheel, pedals are guessed from where
their axes rest during the first second the game reads the wheel (a
device's first reading can be all zeros). The launcher's calibration
(the Wheel tab's pedal wizard) is the reliable path.

`fake = wheel` or `fake = wheel+pedals` adds **test-only** synthetic
devices (see Tests); launchers never write it.

## The virtual wheel

Frozen (`rea_launcher::wheel::dinput::VIRTUAL`, `Axis`): launchers seed
game profiles that bind it.

| | |
|---|---|
| Name | `rea wheel` |
| VID/PID | 1209:5245 (pid.codes), in `guidProduct` (`Data1` = PID << 16 \| VID, "PIDVID") and `DIPROP_VIDPID` |
| Instance GUID | `{7265615f-7768-6565-6c00-000000000001}`, fixed |
| Type | `DI8DEVTYPE_DRIVING`, `DI8DEVTYPEDRIVING_DUALPEDALS`, HID (`dwDevType` 0x10316) |
| Axes (`EnumObjects` order, instances 0–3) | X steering (the force-feedback actuator), Y throttle − brake (centred; throttle towards the maximum), Z throttle, Rz brake (both released at the minimum) |
| Buttons | 32: the wheel's, then the pedal device's |
| Hat | 1, the wheel's first |

`GetCapabilities` reports 4 axes, 32 buttons and 1 hat. Every axis
takes `DIPROP_RANGE` (by device, offset, object ID or usage),
`DIPROP_DEADZONE` and `DIPROP_SATURATION`; the default range is
0..65535. **Centre:** a centred axis rests halfway between the ends,
and when that falls between two integers it takes the lower: **127** of
the CMR games' 0..255 (CMR04 reads 128 as one step right), −1 of
−32768..32767.

The data format the game sets (`c_dfDIJoystick2` in the CMR games, but
any) is translated the way DirectInput maps one: each entry, in order,
gets the first unused object that fits its GUID, type, instance, aspect
and actuator flag; hats the wheel doesn't have read centred.
`GetDeviceData` works once the game sets `DIPROP_BUFFERSIZE`.

## Force feedback

`EnumEffects` offers the standard effects (constant, ramp, the five
periodic ones, spring, damper, inertia, friction; no custom force). A
game's effect becomes a one-axis effect on the wheel's X:

- its force is the share of its direction along X (cartesian, polar or
  spherical; CMR uses X and Y), flipped by `ffb_invert` (a periodic
  effect flips its phase);
- a condition (spring, damper…) takes X's own condition; `ffb_invert`
  mirrors it (centre negated, its two sides swapped) and never makes a
  coefficient negative, which would drive the wheel to full lock;
- trigger buttons are not passed on.

**Strength:** the wheel's `DIPROP_FFGAIN` is the game's gain ×
`ffb_strength`, so the wheel's own autocentre spring is at the player's
strength too, and effects go as the game made them. A wheel that
refuses the gain gets the strength in each effect instead (magnitudes,
coefficients, saturations and envelope levels).

The virtual wheel claims force feedback (capabilities,
`DIEDFL_FORCEFEEDBACK`) only when its wheel takes it. `SetParameters`
goes to the real effect; when the driver refuses a `DIEP_NORESTART`
update (an old MOMO report) it is sent again without it, and from then
on without asking. An effect started before it was complete, or after
`DISFFC_RESET`, is made and plays once it can. `DIPROP_FFGAIN` and
`DIPROP_AUTOCENTER` answer (CMR04 makes no effects when the gain can't
be read). `SendForceFeedbackCommand` is passed on.

**Kept alive:** when the wheel is acquired again (the game's own
re-acquire, or after it was lost) its autocentre and gain are set again
and every effect is made, downloaded and, if the game had it playing,
started again — drivers drop a device's effects then, and the game never
knows. Effects that play forever and that the wheel stopped on its own
are started again (checked every 250 ms; Wanszai's revive,
docs/playbooks/wheels-and-ffb.md).

## Ship and enable it

**Build the in-game DLLs** (`rea_dinput.dll`, `rea_close_hook.dll`) from
the launcher's workspace:

1. Depend on the DLL crates with `default-features = false`. Their `dll`
   feature holds each DLL's `DllMain` and exports; an exe that links both
   crates with it on fails (`duplicate symbol: _DllMain@12`).
2. Give the launcher a feature that turns it on for the DLLs it ships:
   `layer-dlls = ["rea-launcher-close-hook/dll", "rea-launcher-dinput/dll"]`
   (Cargo refuses `--features rea-launcher-dinput/dll` for a package
   outside the workspace).
3. Add a profile for them to the workspace's `Cargo.toml`:

   ```toml
   [profile.dll]
   inherits = "release"
   panic = "unwind"
   ```

   A launcher's release profile has `panic = "abort"`: in a DLL inside
   the game that turns any panic into the game ending. Built to unwind,
   the DLL catches its own panics and answers that one call with an
   error. `launcher-check` (rule `dll-unwind`) checks the profile and the
   build.
4. Build them with it, for the game's architecture:
   `cargo build --profile dll --lib -p <launcher> -p rea-launcher-close-hook -p rea-launcher-dinput --features layer-dlls --target i686-pc-windows-gnu`
   (`--lib` keeps the launcher's exe out of this build; in the reusable
   workflow, the `extra-build` input). The DLLs land in
   `target/i686-pc-windows-gnu/dll/`; ship them next to the launcher and
   add them to the package allowlist.

Then enable it:

1. Write the runtime copy of the game's executable through
   `rea_dinput::import_into` (never the player's file), copy the DLL and
   `rea_wheel.ini` next to it, and start the game with `REA_WHEEL_MODE`:

```rust
use rea_launcher::wheel::{dinput, rig};
use rea_launcher_win32::process::Launch;

let exe = runtime.join("cmr4.exe");
let bytes = std::fs::read(&exe)?;
std::fs::write(&exe, rea_dinput::import_into(&bytes)?)?;
std::fs::copy(launcher_dir.join(rea_dinput::DLL), runtime.join(rea_dinput::DLL))?;
std::fs::write(runtime.join(dinput::FILE), &wheel_ini)?;
let show = rig::shows_virtual_wheel(&setup, &devices);
// … seed the game's controls for the virtual wheel when `show` …
let code = Launch::new(&exe)
    .cwd(&runtime)
    .env(rea_dinput::MODE_ENV, if show { "on" } else { "off" })
    .run()?;
```

Both this DLL and the close hook (`rea_close_hook::import_into`) go in
one runtime copy, each with its own import descriptor: CMR04's headless
real-game run loaded both, and Alt+F4 still worked.

## Tests

- Host: `cargo test -p rea-launcher-dinput` (format translation, axis
  units at any range, device choice and hiding, calibration, effects,
  settings, the log).
- Windows-target, headless: `scripts/wine-test -p rea-launcher-dinput`.
  `examples/wheel_game.rs` is a stand-in CMR game that imports the DLL
  through its import table and does what the games do (EnumDevices,
  `c_dfDIJoystick2`, 0..255 by object ID, exclusive foreground on a
  hidden window, Spring, Damper, Sine, Square and Constant effects,
  `DIEP_NORESTART` updates). `fake = …` gives it a wheel (and pedals)
  that the layer opens, reads and gives force feedback to like real
  ones; the game moves them and reads what reached them through the
  DLL's `rea_dinput_fake_*` exports (they do nothing without the key).
  Words after the set give the fake wheel a driver's quirks:
  `unplugged` (not there at the start), `busy` (held by another
  program), `nogain`, `norestart`, `broken` (can't be opened),
  `lockedprops` (autocentre and gain only while unacquired).
  `examples/dinput8.rs` is a `dinput8.dll` stand-in in Xidi's place: the
  layer chains in front of it.

**Not verified headless:** real wheels. Wine has no wheel in the tests,
so the force direction, its feel at a given strength, exclusive
background access with each vendor's driver and Wine's hidraw/SDL
backends, and condition effects (spring, damper) on each wheel family
need the hardware pass (phase 3 of the wheel plan).

## Limits

- `IDirectInput8A` only (the CMR games): a game asking for the wide
  interface, or creating DirectInput by another path, gets it untouched.
- The devices are chosen when the game creates DirectInput; a different
  wheel plugged in later is used only with `mode = on` and no wheel at
  the start (a replug of the same wheel is fine).
- A spring's centre and stiffness are the real wheel's, over its whole
  rotation: the steering lock narrows only what the game reads, so a
  spring feels as on the wheel's own range, not the locked one.
- Alt-tab: the wheel is held exclusive in the background, so effects
  keep playing while the game is behind another window until the game
  unacquires the virtual wheel (whether the CMR games do on focus loss
  is not verified). The game's foreground flag is not
  followed: acquiring the real wheel in the foreground would make every
  focus change drop and revive its effects.
- Twin collections: the wheel's same-model devices without force
  feedback are hidden, so a second wheel of the same model without force
  feedback is hidden too.
- The CMR games read 256 steps of steering; that stays.
- Xidi lists its virtual pad (IDs 0000:0000) even with no XInput pad
  connected; it is a pad, never picked, and stays in the game's list.
