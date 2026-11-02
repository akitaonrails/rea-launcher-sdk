# Changelog

All notable changes to rea-launcher-sdk. Versions are git tags; launchers
pin one (`tag = "vX.Y.Z"` in Cargo.toml, `@vX.Y.Z` for the reusable
workflows, `sdk/VERSION` for the vendored files). Behaviour changes a
launcher must act on are marked **Migration**.

## [0.2.0] - 2026-11-02

The gaps found while moving the five launchers onto v0.1.0, and the
owner's standards made into code. Minor version bump: some APIs changed
(marked **Migration**).

### Added

- `rea_launcher::cfg::spec`: one option table (key, flag, kind, label,
  description, launcher-only) that generates `--help`, the `launcher.cfg`
  template and file/flag parsing, plus `assert_parity` for GUI/CLI parity.
  `OptionSpec::with_aliases` keeps a renamed key's old name (file key and
  flag) readable without warnings; `spec::set_key` writes the new name
  and comments out the old lines. `OptionSpec::helper_owned` marks options the Linux
  play helper reads (`usage` says "(Linux helper)"; parity still needs a
  window control).
- `rea_launcher::ini::{Owned, set_owned, release}`: a record of the INI
  values the launcher wrote; unsetting an option restores the earlier
  value or removes the key only if the file still holds ours, so a choice
  the player made in the game's menu is kept (CMR3 wiped it).
- `rea_launcher::pe`: PE32/PE32+ sections, VA ↔ file offset.
- `rea_launcher::patch`: declarative `PatchSet` tables applied from pristine
  bytes with per-edition result hashes, `unpatch`, `describe` →
  Markdown/PCGamingWiki tables, `patch::testing` golden-byte and
  `real_game` helpers.
- `rea_launcher::download` (from OutRun's HD pack code): `Fetch`, `Pin`,
  resumable verified downloads, safe unzip behind the `zip` feature,
  cancellable `Job` with progress.
- Core: `paths::canonical_existing`, `cfg::KeyStyle` (hyphenated keys),
  `display::landscape_from`, `fsx::remove_files`,
  `fsx::remove_stale_temps_here`, `ledger::take_out_with`,
  `log::Log::without_limit`.
- `rea-launcher-ui`: `Metrics` (per-launcher widget sizes and button text
  role), `game_card`, `spec_row` (window rows from the option table, with
  rendered-key tracking), `download_row`, a `testing` feature with
  `egui_kittest` harness helpers, accessible names/roles/states on every
  widget, `theme::contrast_problems` over every pair the widgets draw,
  re-exports of `eframe`/`egui`.
- `rea-launcher-ui`: `theme::size` (every text size the widgets draw;
  body ≥ 13 pt, nothing under 11 pt), `Metrics::text_sizes` and
  `theme::text_size_problems`; `DownloadState::from_phase` for the core
  download job.
- `rea-launcher-win32`: `console::attach_parent` (a GUI-subsystem launcher
  prints `--help`/`--check` to the terminal it was started from) with
  `console::output` to choose terminal, dialog or nothing.
- `rea-launcher-win32`: `http::WinHttp` (`download::Fetch`),
  `display::displays()`/`adapters()` (refresh rate, device and adapter
  names), `Key::open_read`/`read_machine`, `Place::describe` labels.
- Shell: the `REA_FUNCTIONS_ONLY` / `REA_PLAY_HERE` test convention,
  `REA_ORIGINAL_WORDS`.
- Docs: `docs/STANDARDS.md` and playbooks for new launchers, binary
  patching, community components, SDK changes and contributing back;
  `AGENTS.md`/`CLAUDE.md`, Claude Code skills and `scripts/install-skills`;
  AI-first README.

- `scripts/wine-test --target x86_64-pc-windows-gnu` (or
  `REA_WINE_TARGET`), for launchers that ship a 64-bit build (Screamer).
- `scripts/wine-test` names every failing test, also when a test binary
  exits normally with failures (cargo's `failures:` lists), not only after
  an abort; `--failures-from LOG` reads a saved log. It runs with
  `--no-fail-fast`, so every test binary reports.
- `scripts/wine-test` is vendored by `sdk-sync` as `sdk/scripts/wine-test`.
- `launcher-ci.yml` inputs `wine-tests` and `wine-test-args`: install Wine
  and the matching MinGW and run the launcher's Windows-target tests,
  headless.
- SDK CI job running `scripts/wine-test --locked --workspace` (i686) on
  Ubuntu with Wine.
- Reusable `mirror.yml`: copies the pins marked `mirror` into the calling
  repository's `thirdparty` release, verified by sha256 and named by the pin
  (fixes CMR3's mirror: no delete-then-recreate, no asset/fallback name
  mismatch). `pins.sh`: the `mirror` pin word, `pin_file`,
  `pin_mirrorable`, `pin_mirror_url`.
- `scripts/dev-check`: every SDK check in one command; checks whose tool is
  missing (dash, actionlint, shellcheck, Wine, MinGW, cargo-deny) are
  skipped with a warning.
- `CONTRIBUTING.md` and this changelog.
- `docs/STANDARDS.md`: launcher standards (community fixes first,
  contributing back, settings window and CLI rules, headless testing).

### Changed

- **Migration:** `Palette` has a new `selection` field.
- **Migration:** `WindowFit` resizes to fit on native Windows/Linux only
  with `.with_native_fit(true)`.
- **Migration:** `cfg::ArgError::Help` carries the flag typed (`-h`,
  `--help`, `/?`).
- Outline/folder buttons default to the action colour (as the launchers
  drew them); a hovered outline button shows ink text for contrast.
- **Migration:** `theme::contrast_problems` checks control borders:
  `line` needs 3:1 against `base` and `panel` (WCAG non-text contrast).
  All five launchers' `line` fails; the least lighter colour that passes
  is in the `rea-launcher-ui` README. `line` is now a border colour only:
  hovered chips and buttons and the not-ready Play plate fill with
  `panel` instead.
- **Migration:** `scripts/wine-test` keeps its Wine prefix in
  `<workspace>/target/wine/prefix` (was `~/.cache/rea-wine-test`) and
  resolves every path to an absolute one, so it behaves the same inside a
  distrobox whose home differs. `REA_WINE_TEST_PREFIX` still overrides it.

### Fixed

- `widgets::tabs_bar` never drew the current tab in ink.
- `download::remove` of a single-file pin tried to delete a folder.
- `paths::find_ignoring_case` returned the requested spelling on
  case-insensitive disks (Windows); it now returns the on-disk name, and the
  runtime/game overlap check compares paths case-insensitively on Windows.

## [0.1.0] - 2026-11-02

The first release: the shared code of CMR3, CMR4, Screamer, Sega Rally Revo
and OutRun 2006, each piece the reviewed best version across them
(`docs/AUDIT.md`).

### Added

- `rea-launcher`: runtime folder sync with whole-file swaps (never through
  a hard link), atomic installs, save mirroring (newer wins), component
  ownership ledger, `launcher.cfg` and INI editing that keep comments,
  edition hashing, byte patching, Wine DLL override merging, display and
  pad logic, logging.
- `rea-launcher-win32`: registry (read-only access never creates keys),
  process launch with Wine overrides, XInput, monitors and DPI, dialogs,
  GPU preference.
- `rea-launcher-ui`: the settings-window theme, fonts, widgets, Wine-safe
  window sizing and gamepad navigation.
- `shell/rea-play.sh`: the Linux helper library (session lock and stop,
  GPU environment, gamescope, Wine prefix), vendored as `sdk/rea-play.sh`.
- `packaging/`: pin files with mirror fallback (`pins.sh`), the AppImage
  builder with portable Wine or native (`appimage-wine.sh`), the package
  allowlist check, release notes from the CHANGELOG.
- Reusable workflows `launcher-ci.yml`, `launcher-release.yml` and
  `upstream-watch.yml`; `scripts/sdk-sync` to vendor and drift-check the
  shell and packaging files; `templates/deny.toml`.

[Unreleased]: https://github.com/akitaonrails/rea-launcher-sdk/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/akitaonrails/rea-launcher-sdk/releases/tag/v0.1.0
