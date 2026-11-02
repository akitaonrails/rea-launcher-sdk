# Changelog

All notable changes to rea-launcher-sdk. Versions are git tags; launchers
pin one (`tag = "vX.Y.Z"` in Cargo.toml, `@vX.Y.Z` for the reusable
workflows, `sdk/VERSION` for the vendored files). Every change a launcher
must act on is a bullet in its release's `### Migration` section (first in
the release), saying what to do and how to verify it; `sdk-bump` and the
`sdk-update` pull requests copy that section.

## [0.3.2] - 2026-11-02

Fixes from the regression reviews of the five launchers on v0.3.x.

### Migration

- **Errors reach the player under Wine** (CMR3, CMR4, every launcher that
  reports errors): replace `console::output` + `dialog::message` for
  player-facing errors with `report::fatal(title, text, Mode::Play {
  interactive: !no_window })`; keep `Mode::Command` for `--help`/`--check`
  output. Verify under `<id>-play` with a wrong game folder: a dialog
  appears and `launcher.log` has the line.
- **Helper value comparisons** (CMR3, and any game hook that compares a
  `launcher.cfg` value): replace `$(rea_get_key postfx)` comparisons with
  `$(rea_get_value postfx)` and `[ "$(rea_get_key dxvk)" = off ]` with
  `rea_is_off dxvk`. Verify with `cmr3-play --play --postfx Color`
  (play.log shows the vkBasalt layer) and `--play --dxvk False --postfx
  color` (the "needs DXVK" note).
- **Action flags while another instance is open** (`--download-*`): report
  and exit non-zero instead of exiting 0 silently
  (`AlreadyRunning::is_window_found`). Verify by running the flag with the
  settings window open.

### Fixed

- `cfg::parse_args`, `cfg::parse_args_with` and `cfg::spec::parse_args`
  accept `--key=value` as well as `--key value`, and read an underscore
  spelling of a hyphenated flag (`--display_mode`) as a fallback — both
  as SDK v0.1 did, and launchers' scripts relied on them. A value-less
  switch given `=value` is refused.
- `fsx::install`, `fsx::write_atomic` and `fsx::install_copy` leave a file
  that already holds exactly those bytes alone: no temp file, no rename
  (which can fail over a DLL the game has loaded), no flush. `install_copy`
  still replaces a hard link to its source, or a read-only copy, with a
  writable copy. SRR rewrote about 70 MB of DLLs every launch.
- Play-time errors under Wine never reached the player since v0.3.0's
  console pattern: `<id>-play`, the AppImage and Proton redirect stdout to
  a log, so `console::output` chose the terminal. New
  `rea_launcher_win32::report::{fatal, note}(title, text, Mode)`:
  `Mode::Command` (`--help`, `--check`) prints and shows a dialog only when
  nobody sees the terminal (under Wine a redirected stdout counts as
  unseen); `Mode::Play` always shows a dialog when interactive and prints
  too. `report::route` is the tested decision table.

### Changed

- `rea-play.sh` compares values the way the launchers parse them: the
  resolution, `REA_ORIGINAL_WORDS`, the scaler and `REA_WINDOWED_KEY`
  against `REA_WINDOWED_VALUES` match in any case, so
  `--display-mode Windowed` (or `display_mode = Borderless`) now skips
  gamescope. A `--flag=value` argument reaches the launcher as
  `--flag value`, which every launcher's parser reads.

### Added

- `ledger::Ledger::batch(|ledger| …)`: installs and take-outs inside save
  the ledger once at the end (also when the work fails part way), instead
  of once per file.
- `instance::AlreadyRunning::is_window_found(window)`, and the documented
  pattern for action flags (`--download-*`, `--check`) while another
  instance is open.
- `rea-play.sh`: `rea_get_value KEY [FILE]` (the value lowercased, for
  comparisons), `rea_is_on`/`rea_is_off KEY [FILE]` (`on|true|yes|1` /
  `off|false|no|0`, any case; unset is neither).

## [0.3.1] - 2026-11-02

### Added

- `launcher-ci.yml` input `extra-build`: commands after the workspace
  release build (e.g. a dependency's DLL such as `rea-launcher-close-hook`).
- `sdk-sync --check --against-head`: compare with this SDK work tree.

### Changed

- `CHANGELOG.md`: each release lists every change a launcher must act on
  as its own bullet in a `### Migration` section (v0.2.0 and v0.3.0 moved
  there, wording unchanged); CONTRIBUTING's "Before tagging" requires it.
- `launcher-ci.yml` `imports-check` also checks every `.exe`/`.dll` in
  `package-dir` (folders and zips, third-party DLLs included).

### Fixed

- `sdk-bump` (and so the `sdk-update` pull requests) missed Migration
  notes written inside Added bullets (the lld rule, `Pin::name`,
  `REA_HELPER_KEYS`): it reads each release's `### Migration` section, and
  for older releases every bullet that mentions **Migration**.
- `sdk-sync --check` compared with this SDK checkout's HEAD instead of the
  version recorded in the launcher's `sdk/VERSION`, so a launcher pinned to
  an older tag showed false drift; it now runs that version's own
  `sdk-sync` from a temporary `git clone --shared`.

## [0.3.0] - 2026-11-02

### Migration

- Every launcher sets
  `rustflags = ["-C", "link-arg=-fuse-ld=lld"]` under each
  `[target.*-pc-windows-gnu]` it builds (x86_64 too); verify with
  `sdk/packaging/check-lld.sh <target>` and, after a release build,
  `sdk/packaging/check-imports.sh target/<target>/release/*.exe` ("ok").
- `widgets::folder_field(ui, value, label, hint, pick)` takes
  the field's label: it is the text field's accessible name (it was the
  placeholder hint, which is now the description). Pass the heading shown
  above the field (e.g. `"Game folder"`); verify with a kittest
  `get_by_role_and_label(Role::TextInput, "Game folder")`.
- `LookCard` and `GameCard` have a `disabled` field: struct
  literals without `..Default::default()` add `disabled: None`.
- `Pin::name()` (and so
  `Pin::dir`) now drops any short extension, not only `.zip`
  (`setup.exe` → `setup`; `tool-v1.2` stays), so a non-zip download's
  folder is no longer the file itself.
- (CMR3): delete the local
  `rea_hook_command`/`one_run_cfg` and set
  `REA_HELPER_KEYS="scaler postfx resolution display_mode dxvk"`; verify
  with `cmr3-play --play --scaler fsr` (play.log shows `-F fsr`) and a
  plain `--play` afterwards (back to launcher.cfg's scaler).

### Added

- `cfg::spec::OptionSpec`: `with_needs`/`with_placeholder` replace the
  generated value texts (e.g. "10–240 or unlimited", `<program>` for a
  path); `with_value_aliases(&[("true", "on"), ("off", "0")])` reads other
  spellings silently, and an alias naming a number outside a `Range` is a
  special value (the off word: `fps_limit = off` → 0, listed by `needs` and
  the placeholder); `OptionSpec::removed(key, why)` declares a key the
  launcher dropped: accepted and ignored in the file and as a flag, left
  out of `usage`/`template`/parity, and commented out by
  `spec::retire_removed` on save (`spec::active` lists the other rows);
  `spec::usage_with(specs, style, &[(switch, description)])` lists the
  value-less launcher switches in the same aligned column.
- `ledger::Ledger::take_out_keep_game`: removes our file without bringing
  the game folder's own copy back (OR: a repack's SpecialK `d3d9.dll`).
- `log::Log::info`: log-only lines.
- `download::install_with(pin, root, free, fetch, job, extract)`: the
  launcher's own extractor (a `.cab`, an installer) runs into a temporary
  folder that is marked complete and renamed into place like a zip;
  `Job::set_phase` lets it report progress; `download::unzip` is the zip
  extractor `install` uses.
- `rea-play.sh` one-run flags: `REA_HELPER_KEYS` lists the `launcher.cfg`
  keys the helper reads itself (e.g. `scaler resolution display_mode
  postfx dxvk`). `x-play --play --scaler fsr --resolution 1280x720` still
  passes the flags to the launcher, and for that run `rea_get_key` returns
  them over `launcher.cfg` (gamescope size/filter/windowed skip, hooks such
  as postfx); the file is never written. `REA_FLAG_SWITCHES` names the
  value-less flags.
- `scripts/sdk-bump vX.Y.Z [repo]` (vendored as `sdk/scripts/sdk-bump`):
  moves a launcher to an SDK tag in one step (Cargo tags and `cargo
  update`, `sdk/` re-vendored, reusable workflow refs) and prints the
  Migration items in between; idempotent, refuses a dirty tree or a
  downgrade without `--force`.
- Reusable `sdk-update.yml`: a launcher's scheduled workflow opens or
  updates a PR `sdk-update/vX.Y.Z` with the bump and its Migration items;
  never merges.
- `compat.yml`: the SDK canary builds and tests all five launchers against
  the SDK under test (Cargo `[patch]`, re-vendored `sdk/`, fmt, tests,
  clippy, linux/tests, Wine), with an allow-failure list.
- CONTRIBUTING.md: "Before tagging" checklist (canary, Migration items that
  say what to do and how to verify).
- `packaging/check-imports.sh FILE…`: every import of a built PE32/PE32+
  program must belong to an import descriptor (GNU ld 2.47 leaves some out,
  for i686 and x86_64); names the orphans. `packaging/check-lld.sh TARGET…`:
  `.cargo/config.toml` links each Windows target with lld. `launcher-ci`
  runs both (`imports-check`, default on) and always installs lld for a
  Windows target.
- SDK CI `scaffold` job: generates a launcher with `scripts/new-launcher`
  and runs its checks (fmt, tests, clippy host and i686, linux and package
  tests, lld and import checks, Wine tests) against this checkout.
- `scripts/clean` (vendored as `sdk/scripts/clean`): dry-run list of
  `target/`, `dist/` and the `.work/` entries in `.work/.task-created` with
  sizes; `--yes` removes exactly those (inside the repo, ignored, untracked,
  no symlinks out; `--keep`, `--work-all`).
- `sdk-sync` vendors `docs/STANDARDS.md` and `docs/playbooks/*.md` to
  `sdk/docs/`; the launcher template's `CLAUDE.md` imports
  `@sdk/docs/STANDARDS.md`, and its `AGENTS.md` points there instead of
  copying the rules.
- `pins.sh`: the `hosted` pin word (no licence; the owner decided to host
  it) and `pin_hosted NAME`; `mirror.yml` uploads `hosted` pins only with
  `include-hosted: true`, and its `pins` input takes folders of one-pin
  `<name>.txt` files and globs as well as pin files.
- `scripts/wine-test`: the private prefix uses Wine's null graphics driver
  (`HKCU\Software\Wine\Drivers` `Graphics=null`), so tests that create
  windows run headless without setting it; an existing prefix gets it once.
- `rea_launcher_win32::instance::single(name)`: one launcher per user
  session (named mutex in `Local\`); `AlreadyRunning::bring_to_front`
  focuses the running launcher's window.
- `process::Running::close_gracefully(timeout)` (`WM_CLOSE` to the child's
  top-level windows, then `TerminateProcess` as a last resort; reports
  `Closed::{AlreadyExited, Closed, Terminated}`), `Running::is_alive`,
  `Running::exit_code`, `process::top_level_windows(pid)`.
- New crate `rea-launcher-close-hook`: CMR4's close hook made generic, a
  `WH_GETMESSAGE` hook DLL configured per game without recompiling
  (`REA_CLOSE_HOOK` or `rea_close_hook.ini`: window class, which requests,
  and the strategy: terminate, exit, quit, post_quit, keys, post_keys).
- `OptionSpec::unset_label("As the game")` and in `rea-launcher-ui`'s
  `spec_row` a first chip for it on `Toggle`/`Choice` rows (a toggle then
  becomes tri-state chips); choosing it writes the empty text, which the
  launcher clears from `launcher.cfg`. `spec::choices(spec)` lists a row's
  chips.
  A removed row (`OptionSpec::removed`) draws nothing and is not recorded
  for parity; draw rows from `spec::active(specs)`. The refused-value line
  uses the row's `needs` (its `with_needs` text when set).
- `spec::spec_row_with(.., RowOptions { disabled, steps })`: a disabled
  reason on any row (hover text and accessible description), and `Range`
  rows drawn as chips with given steps instead of a slider.
- `widgets::photo_caption(ui, strip, alt, lines)`: the photo strip's
  caption (title, text and credit lines), exposed to AccessKit as an image
  named by `alt` plus one label with every line.
- `widgets::name_control(response, label, description)`: names any control
  for screen readers (a chip row's group label and description).
- `LookCard::disabled` / `GameCard::disabled`: a reason; the card is
  dimmed, ignores clicks, and is disabled with the reason for screen
  readers.
- `tabs_bar`: Ctrl+Tab / Ctrl+Shift+Tab and Ctrl+PageDown / Ctrl+PageUp
  switch tabs (wrapping); the tabs are a tab list for screen readers.
- `footer`: while Play is not ready, the hint is the plate's accessible
  description (why it is disabled).
- `testing::harness_sized(.., size, ..)` for launchers whose window is not
  880×640 (OR 880×760, Screamer 980×700); the docs show
  `scroll_to_me()` for widgets below a scrolling tab's fold.

## [0.2.0] - 2026-11-02

The gaps found while moving the five launchers onto v0.1.0, and the
owner's standards made into code. Minor version bump: some APIs changed
(see Migration).

### Migration

- `Palette` has a new `selection` field.
- `WindowFit` resizes to fit on native Windows/Linux only
  with `.with_native_fit(true)`.
- `cfg::ArgError::Help` carries the flag typed (`-h`,
  `--help`, `/?`).
- `theme::contrast_problems` checks control borders:
  `line` needs 3:1 against `base` and `panel` (WCAG non-text contrast).
  All five launchers' `line` fails; the least lighter colour that passes
  is in the `rea-launcher-ui` README. `line` is now a border colour only:
  hovered chips and buttons and the not-ready Play plate fill with
  `panel` instead.
- `scripts/wine-test` keeps its Wine prefix in
  `<workspace>/target/wine/prefix` (was `~/.cache/rea-wine-test`) and
  resolves every path to an absolute one, so it behaves the same inside a
  distrobox whose home differs. `REA_WINE_TEST_PREFIX` still overrides it.

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

- Outline/folder buttons default to the action colour (as the launchers
  drew them); a hovered outline button shows ink text for contrast.

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

[Unreleased]: https://github.com/akitaonrails/rea-launcher-sdk/compare/v0.3.0...HEAD
[0.3.2]: https://github.com/akitaonrails/rea-launcher-sdk/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/akitaonrails/rea-launcher-sdk/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/akitaonrails/rea-launcher-sdk/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/akitaonrails/rea-launcher-sdk/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/akitaonrails/rea-launcher-sdk/releases/tag/v0.1.0
