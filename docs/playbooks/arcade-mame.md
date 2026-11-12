# Playbook: an arcade game on a bundled MAME

How rea-rad-mobile runs the player's own Rad Mobile ROMs on a MAME it
bundles, written down for the next arcade launcher. The shape follows
Wanszai's arcade wrappers, studied for their behaviour (front end, wheel
handling, operator settings, CRT look); nothing of theirs is copied.

Today only `child::spawn_tied` and the wheel/force-feedback layer are in
the SDK (`rea-launcher-native`, `rea_launcher::wheel`, `rea_launcher::ffb`;
see `wheels-and-ffb.md`). The MAME parts below (bridge, romset finder,
command line, controller file, layouts, snapshots, the generic patch
hunks) move into `rea-launcher-native`'s `mame` feature and an SDK
`mame/` folder when the second MAME launcher starts, because the patch
has to be rebased on that launcher's MAME version anyway. Until then, read
rea-rad-mobile's code for them.

## 1. Bundle a pinned MAME

A shallow submodule at a release tag, built with `SUBTARGET` limited to
the one driver, patched by a single patch file regenerated with
`git diff > patches/…`. Ship the GPL corresponding source with every
release. Build Windows with MinGW and lld in the `gaming` box, and a
portable Linux build in Debian bookworm (an old glibc).

## 2. The launcher owns MAME's configuration

`-noreadconfig` and every directory private to the launcher (`-rompath`,
`-cfg_directory`, `-nvram_directory`, `-ctrlrpath`, `-artpath`,
`-snapshot_directory`, `-bgfx_path`), as absolute paths because MAME runs
from its own folder. `-skip_gameinfo`, `-noconfirm_quit`,
`-joystick_deadzone 0 -joystick_saturation 1` (the launcher shapes the
input). Full volume passes no `-volume` at all: a Windows build went
silent at −2 dB (rea-rad-mobile #1).

## 3. The bridge

A 4 KiB shared file (`REA_BRIDGE` in MAME's environment) carries the axes
and buttons in and the outputs back: `-joystickprovider reabridge` (one
virtual joystick) and `-output reabridge` (lamps and `rea_*` telemetry).
MAME's heartbeat `mame_seq` ticks twice a frame, so never use it as a frame
clock: publish `rea_frame` from vblank. Telemetry older than 500 ms means
MAME is gone. Keep a test that parses the C header's offsets against the
Rust side (rea-rad-mobile's test compares the Rust constants with
themselves).

## 4. The controller file

A `default` section maps input types to the bridge's joycodes beside the
keyboard keys. **Fields with a driver `PORT_CODE` ignore the default
section**: they need a per-set section by port tag and mask. Rad Mobile's
lights and wiper were keyboard-only until 0.3.0 for that reason. Dump each
field's `input_seq("standard")` from Lua to check. The button order is
the bridge's and MAME's `JOYCODE_1_BUTTONn`: never renumber the launcher's
button enum.

## 5. ROMs identified by CRC

Read only the zip central directories and match (CRC32, size) under any
file name. Pick the preferred complete set; for the closest incomplete one,
report the missing files. Rebuild the set under the bundled MAME version's
names in the private folder, after checking the real CRCs, written to a
partial file and renamed. Folders with more than 200 zips get a name
prefix filter. Files added after old dumps (factory EEPROMs) are made
optional in the patch. Never ship ROMs; tests use synthetic sets.

## 6. Operator settings in EEPROM/NVRAM

Like Wanszai's Sega Rally seed: map the operator settings by changing one
test-menu item at a time and diffing the NVRAM. Rewrite the
launcher-owned bytes before every run, keep the game's bookkeeping while
the image is valid, recompute the checksum. Force what MAME can't
emulate (Upright instead of Deluxe, which reports "Motor trouble").

## 7. Telemetry from RAM

Find addresses with headless runs driven through the bridge plus Lua RAM
dumps; keep a `RAM_MAP.md` with the evidence per address. Publish them
from the driver: MAME 0.289's Lua can no longer create outputs. The game
maps them to `ffb::CarState`.

## 8. True widescreen

Only where the video hardware can draw wider (System 32's 416-pixel mode).
Widen the visible area; shift the tilemaps, bitmap and sprites by the
margin; stretch clip windows that touch the edge; keep the HUD inside the
original width. The margin comes from the pixel aspect
(`(A·H/par − W)/2`, even, capped), plus a layout view at the real aspect.
Label it experimental: things pop in at the margins.

## 9. CRT shaders

Wanszai's Scud Race CRT look (Lottes-derived: quarter-resolution bloom,
9-tap Gaussian, a beam per virtual scanline, aperture or shadow mask,
curvature 0.12, vignette 0.25) as our own bgfx port, with the compiled
`.bin` for every backend committed. dx9/dx11 need Microsoft's
`d3dcompiler_47.dll` under Wine to compile; it is never committed.
x11grab under Xvfb can't capture MAME's GL window: check the look on a
real display (and say so before opening one).

## 10. No modal boxes

The launcher is the front end: skip MAME's start-up screens
(`REA_SKIP_STARTUP_SCREENS` in the patch) and the optional-ROM warning;
Esc (UI_CANCEL) leaves without a confirmation.

## 11. Menu art from the player's own ROM

A hidden `-video none -sound none -nothrottle` run with a Lua snapshot at
chosen frames, a 90 s timeout, blank frames rejected. Nothing from the
game ships. Run it on its own thread; never join it from the UI thread
(rea-rad-mobile's menu could freeze for the whole timeout).

## 12. MAME dies with the launcher

`rea_launcher_native::child::spawn_tied`. On Linux PDEATHSIG fires when
the *spawning thread* exits, so spawn from a thread that waits for MAME.
Take the single-instance guard first: a second launcher would truncate the
running game's bridge file and start a second MAME.
