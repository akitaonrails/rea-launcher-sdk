# SDK design

## Is it worth extracting?

Yes. After normalisation the five launchers carry the same code five times:

| Area | Copies | Lines each (approx.) |
|---|---|---|
| Runtime folder sync, overlap check, atomic installs, save mirror | 5 | 300 + 250 tests |
| Win32 boilerplate (wide strings, message box, folder picker, registry, process launch, XInput, `is_wine`, GPU/GLX prefs) | 4 | 250 |
| Settings-window theme, fonts, widgets, sizing, pad navigation | 5 | 500 |
| `launcher.cfg` editing and value parsing | 5 | 150–200 |
| INI editing | 3 | 120 |
| Edition hashing / hex | 5 | 45 |
| Component ownership ledger, Wine DLL override merge | 4 | 200 |
| Linux play helper (lock, session, gpu env, gamescope, prefix) | 5 | 130 |
| AppImage + pin fetching + CI | 5 | 200 |

About 2,000 lines per launcher are shared. Every bug found in the audit
(`AUDIT.md`) existed in one or more copies and was already fixed in another —
the shared code is where the drift costs the most.

rea-rad-mobile (a MAME wrapper) added the wheel and force-feedback layer
in v0.8.0: about 2,000 lines of pure, tested code with Wanszai-derived
hardware knowledge, generic once the game's buttons became a trait. Its
MAME pieces (bridge, romset finder, command line) wait for a second MAME
launcher; `docs/playbooks/arcade-mame.md` records them meanwhile.

What we do **not** extract: game knowledge. Edition tables, patch sites,
config generators for SilentPatch/Tweaks/dgVoodoo/DOSBox, profile byte
layouts, which files a game writes, its look values, its tabs. The SDK takes
these as data or trait implementations.

## Layout

```
crates/
  rea-launcher        portable, host-tested core (no GUI, no Win32)
  rea-launcher-win32  Windows-only helpers (compiles to nothing elsewhere)
  rea-launcher-ui     eframe/egui theme, fonts, widgets, window shell
  rea-launcher-native native launchers only: tied children, SDL wheels/pads/FFB
shell/
  rea-play.sh         POSIX library sourced by each linux/<x>-play
  tests/              shell tests for the library
packaging/
  pins.sh             pin-file loader + verified fetch with mirror fallback
  appimage-wine.sh    AppImage builder with portable Wine (+ native mode)
  smoke-package.sh    allowlist check for a built package
  appimage/           AppRun template, Wine licence files
.github/workflows/
  launcher-ci.yml     reusable (workflow_call) checks + Windows build
  launcher-release.yml reusable release: SHA256SUMS, notes from CHANGELOG
scripts/
  sdk-sync            vendor shell/ + packaging/ into a launcher, stamped
templates/            skeleton for a new launcher
.claude/skills/       how to build a launcher with this SDK
```

## How launchers consume it

- **Rust crates:** Cargo git dependency pinned to a tag:
  `rea-launcher = { git = "https://github.com/akitaonrails/rea-launcher-sdk", tag = "v0.1.0" }`.
  `deny.toml` allows that git source.
- **Shell and packaging files:** vendored. The shipped AppImage and zip must
  contain the play library, so a player never needs a checkout.
  `scripts/sdk-sync <launcher>` copies them under `sdk/` in the launcher with
  an `sdk/VERSION` stamp; CI re-runs the sync and fails on drift.
- **CI:** launchers call the reusable workflows by tag.

## Rules for SDK code

- Pure logic lives in `rea-launcher` and is unit-tested on the host.
- `unsafe` and FFI live only in `rea-launcher-win32` (what the Wine-run
  launchers need) and `rea-launcher-native` (what only native launchers
  need: a child tied to the launcher, SDL devices and haptics). The second
  is a separate crate with features, not a feature of `rea-launcher-ui`:
  an i686 Wine launcher never owns a wheel, and `sdl2-sys` in every
  launcher's lockfile and `cargo deny` would cost them for nothing. The
  pure halves (wheel maths, the setup wizard, synthetic force feedback)
  stay in `rea-launcher`, where every launcher can test against them.
- Never write through a hard link; every install is a whole-file swap
  (temp file + rename).
- A setting the launcher does not own is never rewritten. Unset means "keep
  the game's value".
- Behaviour a game must verify (a new option) never ships as an SDK default;
  the SDK provides mechanisms, games opt in.
