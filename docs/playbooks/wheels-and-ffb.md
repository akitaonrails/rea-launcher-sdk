# Playbook: wheels, pedals and force feedback

For any launcher that **owns the wheel** (an emulator wrapper such as
rea-rad-mobile, where the launcher feeds the emulator's input), and as a
reference for the PC racers where the **game** owns it.

Most of what follows was learnt from Wanszai's arcade wrappers (Sega
Rally, Scud Race, Ridge Racer, Virtua Racing), studied for their
behaviour while building rea-rad-mobile; nothing of theirs is copied. The
code is in the SDK:

| What | Where |
|---|---|
| Known wheels, `classify`, spare interfaces, `pick_wheel`, `find_aux`, steering lock, curves, pedals, `WheelProfile<B>`, the wizard, pad maps | `rea_launcher::wheel` (pure) |
| Synthetic force feedback from a `CarState`, the test pattern | `rea_launcher::ffb` (pure) |
| Key repeat for list menus | `rea_launcher::pad::PadNav::with_repeat` |
| DirectInput games: the devices behind the virtual wheel, the launcher's Wheel tab | `rea_launcher::wheel::rig`, `rea_launcher_win32::dinput`, `rea_launcher_ui::wheel` |
| DirectInput games: the virtual wheel inside the game | `rea-launcher-dinput` (`rea_dinput.dll`) |
| SDL devices, haptics, rumble; rescans; retries | `rea_launcher_native::sdl` (feature `sdl`), `rea_launcher_native::policy` |

The launcher brings the game knowledge: its buttons (`impl Control`), the
wizard's steps and prompts, which buttons the shift paddles become, the
default pad layout, the default lock, and how its telemetry maps to a
`CarState`.

## 1. Who owns the wheel

Wanszai's model: the launcher reads every device and hands the game
finished values (steering after lock and curve, pedals after calibration,
buttons). Device numbering, deadzones and platform quirks never reach the
game. This works when we control the emulator's input (MAME through the
bridge, DOSBox's joystick). A PC game that reads DirectInput or XInput
itself (Colin McRae, DiRT, OutRun 2006) owns the wheel; the launcher can
only diagnose and advise (§ "PC racers" below).

## 2. Known wheels

`wheel::known::KNOWN` holds 18 VID/PID rows from Wanszai's table: name,
rotation and the shift paddles' button numbers on Windows. Only facts that
hold on every platform are kept. Axis numbers differ between DirectInput
and Linux's evdev drivers, so the steer/gas/brake axis columns of
Wanszai's table are not used: pedals are found by where they rest.
To add a row, ask the player for the launcher's `--list-devices` output
(VID, PID, name, axes, buttons, force feedback) and the wheel's rotation
from its manual.

## 3. What a device is

`wheel::classify`: a known row, or SDL's wheel type, makes a wheel; then a
game controller is a pad; then a device with axes from a wheel vendor
(`known::wheel_vendor`) is a wheel; anything else is "other" (pedals on
their own, shifters, button boxes). The vendor hint comes last because
Logitech and Thrustmaster make pads too: rea-rad-mobile 0.5.0 asked it
first and drove a Logitech F310 as a 900° wheel.

Skip spare HID interfaces (`is_spare_interface`): a device with no axes,
neither wheel nor pad, beside one of the same model with axes. An 8BitDo
pad shows up three times on Windows.

## 4. Twin collections

A Fanatec CSL DD appears twice on Windows (two HID collections) and only
one reports input. `pick_wheel` takes the one picked by hand (a device
path, from the menu), else among the saved profile's model the first that
takes force feedback, else the first. Offer the manual pick in the window.

## 5. Steering lock

`s = clamp(raw × (rotation/2) / (lock/2), −1, 1)` (`apply_lock`). Steps
180/270/360/540/900/Full; default 270° for an arcade cabinet
(`SteeringLock::DEFAULT`); an unknown rotation counts as 900°. A 1080°
wheel at 270° reaches full lock a quarter of the way to its stop. For a PC
game, find the original's value in research, never by guessing.

## 6. Pedals

`PedalAxis { axis, rest, peak }` reads 0..1 with a 2 % deadzone at the
top; a span under 1000 (uncalibrated) reads as released. `guess_pedals`
takes the first two non-steering axes resting beyond ±24000. Pedals on a
second USB device are pinned by GUID, then VID/PID (`find_aux`), and read
as released while unplugged, never stuck. A combined pedal axis (gas one
way, brake the other) is allowed; the same axis the same way is not.

## 7. Gamepad curves

Deadzone first, then `L·m³ + (1−L)·m` (`shape`, `curve`), with Wanszai's
labels Linear/Slight/Mild/Medium/Heavy/Cubic (`linearity_label`).
`wheel::read_pad` steers from the left stick or the d-pad; triggers are
pedals through their own deadzone and curve, buttons floor them. Back +
Start together is the launcher's exit (`pad_map::Exit`), and suppresses
the buttons it would otherwise press.

## 8. Setup wizard

`wheel::wizard::Wizard<B>`: 10 warm-up reads, then a baseline; an axis
arms past 16000 from rest, follows its peak, and is captured when it comes
back within 8000. Pedal steps time out after 10 s, button steps after 5 s,
and an armed pedal that never comes back times out 10 s after it armed
(rea-rad-mobile waited forever on a sticky pedal). Up to three extra
devices are watched; the first one used becomes the aux device
(`WheelProfile::apply`). The same machine remaps one control
(`Wizard::single`). `PadCapture` waits for the opening press to be let go,
times out after 6 s; ignore menu input for 400 ms after a capture.

Keep the wizard's device order fixed while it runs: a rescan mid-wizard
reorders devices (rea-rad-mobile fed it the wrong ones). Restart the
wizard when the device list changes.

`WheelProfile::to_cfg`/`from_cfg` keep rea-rad-mobile's `wheel.ini` keys,
read its `key=value` form and any key case, and keep the player's comments
and hand edits.

## 9. Force feedback on SDL

`rea_launcher_native::sdl::Devices::force_feedback`: autocentre off, gain
100 (the launcher scales strength itself), a constant force plus damper and
friction (no spring), parameters sent only on change (1/512), the constant
re-run every 150 ms (Fanatec and Thrustmaster firmwares drop an idle
effect), stopped conditions revived every 250 ms (`HapticPolicy`). A
device that refuses force feedback is retried after 5 s or a rescan, not
every tick.

**SDL directions say where a force comes from.** A push to the right comes
"from the west": `[-1, 0, 0]` (`policy::effect_params`). Rad Mobile 0.4.0
shipped the mirror image, and centring pushed away from the centre on a
real CSL DD. Linux frees effects when the fd closes; stop them all on exit
anyway (`Devices::release_force_feedback`).

## 10. Synthetic force feedback

For games that had no motor, `rea_launcher::ffb::SyntheticFfb` with
`Tuning::ARCADE`: a centring spring stiffening with speed (0.25 → 1.0,
0.2 outside a race), damper 0.05 → 0.30, friction 0.04, a 120 ms slew, a
wall scrape (pushed away 0.3 plus a 14 Hz buzz 0.2, scaled by speed), and
crash jolts from smoothed deceleration (50 ms window, 600/430 top speeds
per second), `(size·0.5+0.5)·0.8` held four frames, armed only after one
second of racing so a new race doesn't jolt. The game maps its telemetry
(RAM → emulator outputs, `arcade-mame.md`) to `CarState { racing, speed,
contact }`. `ffb::test_pattern` is the menu's "test force feedback".

## 11. Rumble

Pads get rumble from the same model (scrape × 0.6, jolts).
`Device::rumble` sends a start or stop at once and refreshes a running
rumble every 50 ms (rea-rad-mobile sent 250 calls a second, an output
report each on Bluetooth pads).

## 12. Hot-plug

Fall back to the pad while the wheel is gone, and back when it returns.
`Devices::pump` rescans at most every 500 ms and keeps a plug event that
arrives inside that window for the next rescan (rea-rad-mobile lost it, so
pedals plugged right after the wheel stayed invisible).

## 13. Stick assists

Games where the wheel sets a turn rate (Rad Mobile, Power Drift) are hard
on a stick. rea-rad-mobile's assist turns the stick into a target angle to
the road (its `STEERING_ASSIST.md`): learn the bend from telemetry, don't
steer into the walls, use the emulator's frame counter (never a heartbeat
that ticks twice a frame), check the player's ROM version first. Merge the
pads before the assist runs, and run it once per tick (two pads each
running it fought over its state). Game code; stays in the launcher.

## 14. Testing without hardware

- Host tests drive the pure parts with synthetic `DeviceState`s; tests
  move over with a test `Control` enum.
- rea-rad-mobile's `scripts/dev/fake-wheel.py` and `fake-pad.py` create
  uinput devices (an FF wheel that logs every uploaded effect as JSON). A
  test checks the **direction** of the force, not only that one was sent.
  They need write access to `/dev/uinput`; headless, no window. A Rust
  port in the SDK is a later step (no Python in what ships or builds).
- Fakes at the boundary (STANDARDS, "Tests that catch regressions").

## 15. What players report

`--list-devices` (on Windows only with `console::attach_parent`: a GUI
exe prints nothing otherwise), the version line at the top of
`launcher.log` (`Log::per_run` keeps it first; the previous run is
`launcher.log.old`), and "invert force feedback" only as a last resort,
never as the fix.

## PC racers (the game owns the wheel)

Colin McRae, Sega Rally Revo, DiRT, DiRT Rally and OutRun 2006 read the
wheel themselves under Wine, so no lock or synthetic force from the
launcher. What still helps:

- **Diagnostics:** list devices and flag twin collections and spare
  interfaces; a DirectInput game that binds "the first wheel" may pick the
  inert Fanatec collection.
- **Seeding the game's controller config**, where it keeps one, from the
  known rotation and pedals found by rest position (per launcher, after
  research).
- **Lock advice:** "set your wheel to N° in its software" in the README
  and the window.
- **Wine/Proton FFB:** which winebus path carries it (SDL or hidraw) is
  game- and wheel-specific; record it in `RUNTIME_NOTES.md`.
  `wine::SDL_ONLY` has not been verified with a wheel.

## DirectInput games: the launcher's side

For CMR3, CMR04 and CMR2005 an in-game layer shows the game one virtual
wheel made from the player's wheel and pedals (`rea_wheel.ini`, the
contract in `rea_launcher::wheel::dinput`). This part is what the
launcher does; the layer itself has its own part.

- **One set of rules on both sides.** `wheel::rig` decides which wheel
  and pedals drive (`pick`: `pick_wheel` among the wheels and the device
  picked by hand, the pedals picked by hand, else the calibrated `aux_*`
  device, else on the wheel), how they are read (`profile`: the saved
  calibration for that wheel model, else a guess from where the axes
  rest), what the game gets (`virtual_controls`: `Profile::read` with the
  lock, then `shape` with the deadzone and curve) and whether the layer
  shows the wheel at all (`shows_virtual_wheel`). The tab's live check
  and the layer call the same functions, so the check is the game.
- **Reading devices.** `rea_launcher_win32::dinput::DirectInput` lists
  `DI8DEVCLASS_GAMECTRL` devices (spare HID interfaces left out), keys
  each by its instance GUID (`rig::guid_text`, lower case with braces)
  and reads `DIJOYSTATE2`'s eight axes in `dinput::REAL_AXES` order with
  `DIPROP_RANGE` = `REAL_RANGE`, non-exclusive, in the background, on a
  worker thread: the window only copies results. The kind comes from
  `classify` with `DI8DEVTYPE_DRIVING` as the wheel type and
  `DI8DEVTYPE_GAMEPAD` as a pad; USB ids from `guidProduct` ("PIDVID").
- **Hot-plug.** While the tab reads devices the worker rescans every 2 s;
  "Look for devices again" rescans now. Devices still attached stay open.
- **Force-feedback test.** `ffb::test_pattern` as a constant force on X,
  from the west (positive pushes right, as the SDL path), at the player's
  strength and direction. Exclusive access needs a top-level window: the
  worker makes a hidden one of its own, so the launcher passes no window
  handle. Autocentre is turned off for the test and back on after.
- **Options and files.** The settings are launcher options
  (`wheel::dinput::options`: window, flag and `launcher.cfg` key alike),
  starting from the launcher's own `Setup` (the CMR rally games default to
  540°). The calibration lives in a `rea_wheel.ini` in the launcher's
  config folder. At each start the launcher decides with
  `rig::shows_virtual_wheel` over `dinput::list()` (one-shot, no worker)
  and writes the layer's `rea_wheel.ini` next to its DLL with
  `rig::save("", &setup, profile, HEADER)`.
- **Calibration.** The tab runs the SDK wizard (accelerator, brake) over
  the wheel and every other non-pad device, then `rig::calibrate` saves
  the profile and points `pedals` at the device the pedals were found on.
- **Wine** has no wheel in tests: the module's tests run under
  `scripts/wine-test` with zero devices (list, a refused test, a clean
  stop); the tab's tests use `rig::Fixed`.
- **Unverified on hardware:** the force direction through DirectInput
  (taken from SDL's DirectInput backend), exclusive background access on
  every driver, and whether Wine's instance GUIDs stay the same across
  replugging. Phase 3 of the wheel plan checks them.

## DirectInput games (the game reads the wheel itself): the layer

`rea-launcher-dinput` (crate README for the details) is the in-game half:
`rea_dinput.dll` in the runtime copy's import table
(`rea_dinput::import_into`) wraps the game's `DirectInput8Create`, in
front of Xidi or the system's DirectInput, and shows the game one
virtual wheel ("rea wheel", 1209:5245, the frozen layout of
`wheel::dinput::VIRTUAL`). What we learnt building it:

- **One device per player.** The CMR games bind one controller: a wheel
  base and a pedal box on its own USB can only drive together as one
  merged device. The layer hides what it merged (the wheel, the pedals,
  the wheel's Fanatec twin collection, spare interfaces), so slots in the
  game's own list don't change meaning.
- **Same choices as the tab.** The layer calls `rig::pick`,
  `rig::profile` and `rig::virtual_controls` over the devices it lists
  (spare interfaces left out, as the launcher lists them), and reads them
  at `REAL_AXES`/`REAL_RANGE`, so the calibration means the same.
  `auto` is not decided twice: the launcher passes its
  `shows_virtual_wheel` decision as `REA_WHEEL_MODE=on|off`.
- **The game's units.** CMR sets every axis to 0..255 by object ID;
  any `DIPROP_RANGE` (device, offset, ID, usage), deadzone and saturation
  work. A centred axis rests at 127 of 0..255 (the lower of the two
  middle values; CMR04 reads 128 as one step right). The game's data
  format is translated the way DirectInput maps one, not assumed to be
  `c_dfDIJoystick2`.
- **Force feedback.** Effects become one-axis effects on the wheel's X
  (the X share of the game's direction, × strength, flipped on invert;
  conditions keep their own X coefficients). `DIPROP_FFGAIN` must answer
  (CMR04 makes no effects otherwise). A refused `DIEP_NORESTART` update is
  sent again without it. When the wheel is acquired again its effects are
  made, downloaded and restarted, and forever-effects the firmware
  stopped are restarted every 250 ms: the game never knows the driver
  dropped them (Wanszai's revive).
- **Calibration without the launcher.** Uncalibrated pedals are guessed
  from where the axes rest during the first second the game reads the
  wheel (a device's first reading after acquire can be all zeros).
- **Building it.** The DLL crates as dependencies with
  `default-features = false`, a launcher feature `layer-dlls =
  ["rea-launcher-close-hook/dll", "rea-launcher-dinput/dll"]`, and a
  workspace `[profile.dll]` (`inherits = "release"`, `panic = "unwind"`):
  `cargo build --profile dll --lib -p <launcher> -p
  rea-launcher-close-hook -p rea-launcher-dinput --features layer-dlls
  --target i686-pc-windows-gnu`, DLLs in `target/<triple>/dll/`. With a
  launcher's `panic = "abort"` the layer's own panic guards can't work
  and any panic ends the game (`launcher-check` rule `dll-unwind`; the
  close hook has no guard, so the rule doesn't ask it).
- **Games with an ASI loader that checks the exe** (SilentPatch, CMR3):
  it refuses an exe whose import table changed, so ship the DLL renamed
  `rea_dinput.asi` instead of `import_into` and leave the exe untouched.
  The loader must run before the game's `DirectInput8Create`; the layer
  patches the already-bound import from its `DllMain` either way.
- **Strength** scales each effect; the wheel gets the game's own gain
  (a driver may take a gain and ignore it). The wheel's autocentre
  can't be scaled: the seeds turn it off in the car, the vendor software
  sets it for menus.
- **Testing.** `fake = wheel` / `wheel+pedals` in `rea_wheel.ini`
  (test-only) adds devices the layer opens, reads and gives force
  feedback to like real ones; the stand-in game drives them through the
  `rea_dinput_fake_*` exports and checks what reached them, also behind
  a stand-in `dinput8.dll` in Xidi's place, under `scripts/wine-test`.
- **Unverified on hardware:** force direction and feel per wheel family,
  exclusive background acquisition on each driver and Wine backend, and
  spring/damper support per wheel. Phase 3 of the wheel plan.
