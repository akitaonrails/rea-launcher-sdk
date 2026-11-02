# rea-launcher-sdk

Shared code for the rea retro-game launchers: small packages that make the
player's own copy of an old PC game run on modern Windows and Linux
(Proton/Wine, AppImage) with a settings window, sensible defaults and no
manual patching. Each game lives in its own repository; this SDK holds what
they have in common.

| Launcher | Game |
|---|---|
| [rea-colin-mcrae-3](https://github.com/akitaonrails/rea-colin-mcrae-3) | Colin McRae Rally 3 |
| [rea-colin-mcrae-4](https://github.com/akitaonrails/rea-colin-mcrae-4) | Colin McRae Rally 04 |
| rea-screamer | Screamer trilogy (DOS, via dosbox-staging) |
| [rea-sega-rally-revo](https://github.com/akitaonrails/rea-sega-rally-revo) | Sega Rally Revo |
| [rea-outrun-2006](https://github.com/akitaonrails/rea-outrun-2006) | OutRun 2006: Coast 2 Coast |

## What's in it

| Path | What | Used as |
|---|---|---|
| `crates/rea-launcher` | Portable core: runtime folder sync, atomic installs, save mirroring, component ownership ledger, `launcher.cfg` and INI editing, edition hashing, byte patching, Wine override merging, display and pad logic, logging | Cargo git dependency |
| `crates/rea-launcher-win32` | Windows helpers: registry, process launch with Wine overrides, XInput, monitors and DPI, dialogs, GPU preference | Cargo git dependency |
| `crates/rea-launcher-ui` | Settings window: theme and fonts, widgets, Wine-safe window sizing, gamepad navigation | Cargo git dependency |
| `shell/rea-play.sh` | Linux helper library: session lock and stop, GPU env, gamescope, Wine prefix setup | vendored as `sdk/rea-play.sh` |
| `packaging/` | Pin files, AppImage builder (portable Wine or native), package allowlist check, release notes | vendored under `sdk/packaging/` |
| `.github/workflows/launcher-*.yml` | Reusable CI and release workflows | `uses:` by tag |
| `scripts/sdk-sync` | Vendors `shell/` and `packaging/` into a launcher, `--check` fails on drift | run from a launcher |
| `.claude/skills/` | How to build and maintain a launcher with this SDK | Claude Code skills |

## Using it from a launcher

```toml
[dependencies]
rea-launcher = { git = "https://github.com/akitaonrails/rea-launcher-sdk", tag = "v0.1.0" }

[target.'cfg(windows)'.dependencies]
rea-launcher-win32 = { git = "https://github.com/akitaonrails/rea-launcher-sdk", tag = "v0.1.0" }
rea-launcher-ui = { git = "https://github.com/akitaonrails/rea-launcher-sdk", tag = "v0.1.0" }
```

```sh
../rea-launcher-sdk/scripts/sdk-sync .          # vendor sdk/ at the SDK's current tag
../rea-launcher-sdk/scripts/sdk-sync --check .  # what CI runs
```

Each crate and folder has its own README with the API and examples.
`docs/DESIGN.md` explains what belongs in the SDK and what stays in a game;
`docs/AUDIT.md` records the cross-launcher survey the SDK was extracted from.

## Rules

- The player's game folder is read-only. Everything the launcher changes
  lives in a private runtime copy, and every write there is a whole-file
  swap that never goes through a hard link.
- Never commit or ship game files. Third-party components are fetched from
  pinned URLs and checked by SHA-256.
- The SDK provides mechanisms; a game opts in. Nothing that changes how a
  game behaves becomes a default here until that game has verified it.

## Checks

```sh
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --target i686-pc-windows-gnu --workspace --all-targets -- -D warnings
sh shell/tests/run-all.sh
for t in packaging/tests/*.sh scripts/tests/*.sh; do sh "$t"; done
```

Licensed under MIT or Apache-2.0, at your option.
