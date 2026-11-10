# Changelog

All notable changes to rea-launcher-sdk. Versions are git tags; launchers
pin one (`tag = "vX.Y.Z"` in Cargo.toml, `@vX.Y.Z` for the reusable
workflows, `sdk/VERSION` for the vendored files). Every change a launcher
must act on is a bullet in its release's `### Migration` section (first in
the release), saying what to do and how to verify it; `sdk-bump` and the
`sdk-update` pull requests copy that section.

## [Unreleased]

### Migration

- **Sega Rally Revo: drop `clear_stale_binding`** at the next SDK bump
  (`crates/srr-launcher/src/registry.rs`): `pe::edit::add_import` (and so
  `rea_close_hook::import_into`) now clears the stale binding itself.
  Verify: `import_into` on the bytes as read still gives both exes the
  import; the `a_stale_binding_like_the_games_takes_the_import` test keeps
  passing without the local function.

### Added

- `reg::delete_tree(hive, path)` (Windows): deletes a key with its values
  and subkeys (`RegDeleteTreeW`, in the 32-bit view for `HKLM\SOFTWARE`),
  `Ok(false)` when absent; refuses a hive root or a top-level key
  (`reg::deletable_path`). For registry tests and cleanup (CMR4's Wine
  test called `RegDeleteTreeW` directly).
- Unpack window: `Unpacking::with_credit("Photo: name, CC BY 2.0")` shows a
  photo credit small on a plate in the photo strip's lower left (a label
  for screen readers), for a CC BY photo (CMR2005's old window had one).
  New `widgets::photo_credit` draws it.
- `installer::begin_again(installer, dest)`: a fresh work folder and
  innoextract's arguments even when `dest` is marked as this installer's
  unpack (see Fixed).

### Fixed

- `pe::edit::add_import` / `rea_close_hook::import_into` accept an image
  whose import descriptors are marked bound (time stamp `0xFFFFFFFF`) with
  no name table, when the binding is stale: no bound import directory and
  the address table still holds names or ordinals (both SEGA Rally Revo
  exes; Windows ignores such a binding). The stamp and forwarder chain are
  cleared; a genuinely bound image (bound import directory, or addresses
  in the table) is still refused with `BoundWithoutNames`.
- `scripts/release`: a failure after the files were staged (`git commit`
  refused) left `CHANGELOG.md`, `Cargo.toml` and `Cargo.lock` staged and
  changed; they are now unstaged and restored on any failure. A missing
  git identity (`user.name`/`user.email`, as in a fresh distrobox) is
  refused before anything changes.
- Installer unpack: a folder with a valid `.rea-unpacked` marker whose game
  check fails (the player deleted files) is the launcher's own, so it is
  unpacked again instead of failing the check (`installer::unpack`,
  `rea_launcher_win32::innoextract::unpack`, which logs the reason).
- `scripts/clean`: a `.work/.task-created` line with a comment after the
  path (`.work/probe-1  # why`) was read as part of the path, so the entry
  was never removed. The rest of a line from a blank followed by `#` is now
  a comment (documented in the script and `packaging/README.md`).

## [0.6.0] - 2026-11-10

A game that reads its key from `HKLM` gets it from `HKCU` through the
close hook DLL, so a launcher never depends on the VirtualStore, which the
game may never see (rea-colin-mcrae#6: CMR1 crashed on a player's
Windows 11 while `--check` said ready).

### Migration

- **CMR4, CMR2005, Sega Rally Revo: stop relying on the VirtualStore.**
  `launcher-check` warns (rule `virtual-store`) on
  `reg::Key::machine_or_virtual_store`, now deprecated: the game reads
  the VirtualStore copy only while UAC registry virtualization is on for
  it, which Windows doesn't promise, and never for an exe with a
  `requestedExecutionLevel` manifest. Steps
  (`docs/playbooks/new-launcher.md`, "Registry"):
  1. Write the values with `reg::write_verified(Hive::CurrentUser,
     "Software\…", &values)` instead; game-owned values `When::IfMissing`,
     carried over once from `Key::read_machine(<old HKLM path>)`.
  2. Set `rea_close_hook::registry::ENV` on the game to the rule (one
     path per game, from the audit):
     - CMR4: `HKLM\SOFTWARE\Codemasters\Colin McRae Rally 04 ->
       HKCU\Software\Codemasters\Colin McRae Rally 04` (it reads
       INSTALL_PATH, WIDTH/HEIGHT, LANGUAGE, ADAPTER_VID/PID through
       `RegCreateKeyExA` of the full path). Its runtime exe already imports
       `rea_close_hook.dll`; replace the local `win32.rs` copy of the
       fallback.
     - CMR2005: `HKLM\SOFTWARE\CODEMASTERS\Colin McRae Rally 2005 ->
       HKCU\Software\Codemasters\Colin McRae Rally 2005` (INSTALL_PATH,
       SAVEGAME_PATH, WIDTH/HEIGHT, SELECTION, which the game writes back:
       it now lands in HKCU). The DLL is already imported.
     - SRR: `HKLM\SOFTWARE\Sega\SEGA Rally -> HKCU\Software\Sega\SEGA
       Rally` (`language`, `sku`). SRR has no hook DLL yet: ship
       `rea_close_hook.dll`, import it into the runtime copies of
       `SEGA Rally.exe` and `SEGA Rally_SSE1.exe` with
       `rea_close_hook::import_into`; without `REA_CLOSE_HOOK_ATTACH` it
       only redirects the registry. Its exes' `asInvoker` manifest means
       the VirtualStore never reached it on Windows.
  3. Play and `--check` fail with the `write_verified` mismatches; `--check`
     also fails when the DLL or its import is missing, or the rules don't
     parse (`registry::Config::parse`).
  4. Verify: `sdk/scripts/wine-test` with no key in HKLM; a golden
     snapshot of the env (`REA_REGISTRY`) and the HKCU values; in the real
     game under Wine with the HKLM key removed, the game starts and reads
     its folders; record the Windows check (unelevated) in
     `RUNTIME_NOTES.md`.
- CMR1 needs nothing: its `cmrhooks.dll` already redirects (v0.10.5). It
  may move to `walk HKLM\SOFTWARE\Codemasters\Rally\1.00 ->
  HKCU\Software\Codemasters\Rally\1.00` later.

### Added

- `rea-launcher-close-hook`: a registry redirect configured by
  `REA_REGISTRY` (`registry::{ENV, Root, Key, Rule, Target, Config::parse,
  Config::env_value, Config::map}`). Loaded through the game's import
  table, the DLL wraps the main exe's `RegOpenKey[Ex]A/W`,
  `RegCreateKey[Ex]A/W` and `RegCloseKey` imports when it loads: `FROM ->
  TO` rules send opens of a key and its subkeys elsewhere; `walk` rules
  follow games that open `SOFTWARE` and go down a level at a time, and send
  a step off the rule's path back to `HKLM`. Unmatched opens call the
  original import. Off without the variable. Tested under Wine with a
  stand-in game (reads, writes through a redirected handle, the walk, the
  control).
- `rea_launcher_win32::reg::write_verified(hive, path, &[Value])`: write,
  reopen read-only, compare; `Err(Vec<Mismatch>)` names each wrong value.
  Pure `reg::verify`, `Data`, `When`, `Value`, `Mismatch`, `Problem`.
- `rea_launcher_win32::reg::Key::machine(path)`: `HKLM` with no fallback.
- `launcher-check` rule `virtual-store` (warn).

### Deprecated

- `rea_launcher_win32::reg::Key::machine_or_virtual_store`: see Migration.

### Changed

- STANDARDS, "No admin rights": write HKCU only, never count on the
  VirtualStore; the playbook's new "Registry" pattern.

## [0.5.0] - 2026-11-09

A player who points a launcher at the game's installer gets the game
unpacked, once, without running the installer (CMR2005's code, moved into
the SDK for CMR3, CMR4 and CMR2005).

### Migration

- Nothing required: the installer support is opt-in. To adopt it, follow
  `docs/playbooks/installers.md`: an `InstallerBuild` table,
  `installer::detect` after the installed-game lookup,
  `innoextract::unpack` with the `unpack::show` window, and
  `innoextract_bundle` in `package.sh`. CMR2005 moves its `source.rs`,
  `InnoJob` and unpack window to these at its next bump; its marker file
  becomes `.rea-unpacked` (a folder with the old `.cmr2005-unpacked`
  marker is not the SDK's to replace: rename the marker, or unpack again
  into a new folder).

### Added

- `rea_launcher::installer`: identify an Inno Setup installer by its setup
  program's SHA-256 and its data parts' sizes (an unknown one, or any other
  Inno Setup `.exe`, is left alone and reported with its digest), unpack
  it into a folder of the launcher's own (`begin`/`finish`/`abandon`, the
  `.rea-unpacked` marker: only such a folder is ever replaced or removed),
  resolve innoextract's file collisions with a per-installer list of
  preferred components (CMR4's repack writes `p206.big` twice), and remove
  the files a repack adds that the launcher bundles itself (`drop`:
  CMR3's `dinput8.dll` and `SilentPatchCMR3.asi`, CMR4's dgVoodoo files).
  `check_space`, `unpacked_so_far`, `UnpackJob`.
- `rea_launcher_win32::innoextract`: innoextract.exe started hidden, with
  progress, Cancel (stops only that process and removes its work folder),
  a plain wait when no window can open, and `unpack`, the whole flow.
- `rea_launcher_ui::unpack`: the progress window (the game's title, which
  installer, sizes and time left, "Stop unpacking"; keyboard and pad).
- `packaging/innoextract.sh`: the verified innoextract 1.9 pin lines and
  `innoextract_bundle` for the Windows zip and the AppImage.
- `launcher-check` rule `innoextract`: a launcher that runs
  innoextract.exe pins it and lists it in `THIRD_PARTY.md`.
- `docs/playbooks/installers.md`.

## [0.4.5] - 2026-11-09

From the launchers' 2026-11-09 issues (CMR1 #5, OutRun #1-#3, SRR #3,
Screamer #1).

### Migration

- Nothing required. A launcher whose game reads a folder path into a
  fixed-size buffer (registry `…Path` values) can hand it a path that fits
  with `rea_launcher::paths::fit_path` and
  `rea_launcher_win32::short_path::{short_path, short_path_ahead}`
  (CMR1 0.10.4 has its own copy; move to these at its next bump).
- After `sdk-bump`, the unknown-edition issue form is refreshed only if
  you copy it from `templates/launcher/dot-github/ISSUE_TEMPLATE/` again:
  it now asks for the file size and says what a SHA-256 looks like.

### Added

- `rea_launcher::paths::fit_path` and `rea_launcher_win32::short_path`:
  Windows' short (8.3) name for a folder, for old games with small path
  buffers. CMR1 crashed at `0x49893a` when its runtime folder's path passed
  99 characters (rea-colin-mcrae#5).
- `play.log` and `settings.log` start with a line naming the helper's SDK
  version, the launcher and, when Wine runs directly (the AppImage), its
  version. An OutRun report claimed v0.3.4 while running an older
  AppImage's Wine 11.20 (rea-outrun-2006#2).
- `scripts/tests/version.sh`: the SDK's version strings must agree.

### Fixed

- `rea-play.sh` reported SDK 0.4.3 in v0.4.4 (`REA_SDK_VERSION` wasn't
  bumped with Cargo.toml); the new test catches that.
- The launcher template's unknown-edition form no longer promises that a
  SHA-256 is enough to add a build (Screamer #1: builds that need a byte
  fix need the file read), says a SHA-256 is 64 characters of 0-9 and a-f
  (OutRun #3 posted 20 digits) and asks for the file's size.

### Docs

- AGENTS.md: how to run the toolchain in the `gaming` box when its view of
  `/home` is read-only.

## [0.4.4] - 2026-11-07

The AppImage's portable Wine moves to 11.21: Wine 11.20 crashed at startup
on machines that cannot reserve ~64 GiB of address space (16 GB handhelds
such as the Legion Go S). Wine 11.21 fixes the address-space probe.

### Migration

- Launchers with a portable-Wine AppImage (CMR1, CMR3, CMR4, CMR2005, DiRT,
  OutRun 2006, Sega Rally Revo): bump the SDK tag and re-run
  `scripts/sdk-sync`; `packaging/appimage/tools.pins` now pins Wine 11.21
  and `packaging/appimage/wine/SOURCE.txt` describes it. Verify the built
  AppImage's `wine --version` says 11.21 and the launcher starts on a 16 GB
  machine (or with an 8 GiB address-space cap); the SDK's
  `packaging/tests/appimage-wine.sh` covers the pin/SOURCE match.

### Fixed

- The AppImage's portable Wine is 11.21. Wine 11.20's host address-space
  probe called `mmap()` without `MAP_FIXED_NOREPLACE`, so the hint was not
  honoured and `get_host_addr_space_limit()` sized its page-protection
  table from 1<<63: ~64 GiB of address space per Wine process. Where that
  reservation cannot be made, `calloc()` returns NULL, the wrong assert
  passes, and the first `create_view()` dereferences it — SIGSEGV in
  `ntdll.so+0x787c9`, before any Windows code runs. 11.21 reserves ~8 GiB
  (Wine-Bug 60528, commit 3c49b97b).

## [0.4.3] - 2026-11-05

Fewer antivirus false positives: a player's antivirus flagged CMRLauncher
0.9.0 on Windows (Quiver feedback, 2026-11-04). It injected its hook DLL
into the running game, and like every launcher it had no version
information or icon.

### Migration

- Add a `build.rs` calling `rea_launcher_build::Resources` (product,
  description, company, file name, the AppImage PNG as icon) and
  `rea-launcher-build` in `[build-dependencies]`. The exe then carries
  version information, an icon and a manifest. Verify:
  `i686-w64-mingw32-windres -i <exe> -O rc` lists them. `launcher-check`
  rule `exe-resources` (error from v0.4.3).
- Launchers with the close hook (CMR4, CMR2005): replace
  `Running::message_hook` with `rea_close_hook::import_into` on the
  runtime copy's executable, the DLL next to it, and
  `REA_CLOSE_HOOK_ATTACH=1` in the game's environment (crate README).
  Verify Alt+F4 and window close in the real game. Rule `remote-injection`
  (error from v0.4.3).
- A launcher that injects a hook DLL or writes into the running game
  (CMR1): move the fixes into the runtime copy's file with
  `pe::edit::add_section` and the DLL into its imports with
  `pe::edit::add_import` (rule `remote-injection`).

### Added

- `rea_launcher::pe::edit`: `add_section`, `add_import` (the address table
  optionally inside a fix section), `imports`, `fix_base`,
  `next_section_rva`. Checked by the Wine loader (`rea-launcher-win32`
  tests `pe_import`) and by CMR1 in the real game (race, HD art, Alt+F4).
  When bytes sit right after the section table (Rally.exe's build stamp),
  the table moves to free header space and every other byte stays put.
- `rea_close_hook::import_into`, `DLL`, `ATTACH_EXPORT`, `ATTACH_ENV`:
  the hook DLL loaded with the game and attaching itself, nothing crossing
  processes (tests `attach`).
- `rea-launcher-build`: version information, icon (from a PNG) and manifest
  for a launcher's exe, version information for hook DLLs. The close hook
  DLL carries its own.
- The template has the `build.rs`; STANDARDS has an "Antivirus" section;
  the binary-patching playbook says never to write into the running game.
- `launcher-check` rules `exe-resources` and `remote-injection`.

## [0.4.2] - 2026-11-03

From issue #1 (a native Windows 10 test of the eight Windows launchers)
and CMR3's Xidi 5 and vkBasalt updates. Migration first.

### Migration

- Windows command line (issue #1, a native Windows 10 test of the eight
  Windows launchers): report a command line that fails to parse with
  `report::audience(report::Run::bad_command_line(&args)).mode()` and exit
  `report::BAD_COMMAND_LINE` (2); pass
  `instance::is_action(&args, &[own actions])` as the second start's
  `action`, `--check` included; build the Windows program with
  `windows_subsystem = "windows"`. `launcher-check` rules
  `bad-command-line` and `action-flags` (errors from v0.4.2) and
  `windows-subsystem` check these. Verify: `Launcher.exe --bogus-flag`
  from PowerShell prints the usage and exits 2 with no dialog; `--setup`
  or `--check` with the window open exits 5.
- `launcher-check` has five more new rules; run it after bumping. Errors:
  `release-notes` (no `notes: generate` in build.yml), `watch-pins` (a
  watched component pinned to anything but its release's uploaded file),
  `process-listing` (scripts that print other processes' command lines or
  environments), `real-game-gated` (a test that runs the real game without
  an opt-in variable, or with a visible gamescope). Warning:
  `unwatched-pins` (a pinned GitHub release with no weekly watch; add it
  to `packaging/upstream-watch.txt` and copy the template's
  `.github/workflows/watch-upstream.yml`). Of the released launchers, CMR4,
  CMR2005, DiRT and Sega Rally Revo get that warning; none gets an error.

### Added

- `report::Run::bad_command_line` and `report::BAD_COMMAND_LINE`: a parse
  error is reported like a command (the terminal; a dialog only when
  nobody sees it) and exits 2 in every launcher.
- `instance::ACTION_FLAGS` and `instance::is_action`: the family's action
  flags (`--check`, `--setup`, `--download-*`) in one place.
- The README playbook and template: a SmartScreen sentence for the
  unsigned Windows build, and every registry entry in "Where things live".
- `Ledger::known_with_earlier(rel, current, earlier_digests)`: after a
  component update, a file an earlier release installed before the ledger
  still counts as the launcher's own, so it is replaced instead of refused
  (from CMR3's Xidi 4.3.1 → 5.0.0 update).
- The launcher template has a weekly `watch-upstream` workflow and an
  empty `packaging/upstream-watch.txt`.
- STANDARDS: real-game tests are opt-in; other processes' command lines
  are secrets like environments. The components playbook says which file
  to pin and lists the steps for moving a pin to a new version.

### Fixed

- `upstream-watch.yml` no longer reports a component as updated every
  week when its pin is a commit or tag archive rather than the file the
  release uploads (CMR3's vkBasalt); it skips such a pin with a warning in
  the run summary.

## [0.4.1] - 2026-11-02

Fixes found by v0.4.0's CI. Launchers bumping from v0.3.x go straight to
v0.4.1 (v0.4.0's Migration section applies).

### Added

- `scripts/wine-test` prints what each failing test printed (its panic
  message and output, at most 40 lines), not only its name, so a CI log
  says why; `--details-from LOG` reads a saved log.

### Fixed

- The close hook's `close_on = alt_f4_state` also requires the game's
  window to be visible before it acts: a hidden window is never what the
  player looks at, whatever the system reports as the foreground window.
- The close hook's Alt+F4 test no longer fails on Ubuntu's Wine (CI
  runners), which ends the hidden stand-in game by itself with exit 0;
  the test checks that the hook didn't act (its exit code), and the
  in-front case says SKIPPED with the reason where that Wine can't run it.
- The SDK's scaffold job runs `launcher-check` on the scaffold as
  generated, before its SDK dependencies point at the checkout.

## [0.4.0] - 2026-11-02

Mistakes the family made, turned into SDK functions a launcher can't get
wrong and a conformance check (`scripts/launcher-check`) that fails CI when
a launcher repeats one. Native-Linux launchers get an error window, a
single-instance lock and pad navigation.

### Migration

- **Who sees a message: `report::audience`** (CMR1, CMR3, CMR4, DiRT,
  CMR2005, every new launcher). Replace any hand-made `interactive`
  (`!no_window`, `!linux`, a local `audience.rs`) with
  `rea_launcher_win32::report::audience(Run { command, no_window,
  settings_only, linux }).mode()` filled from the parsed flags (or
  `Run::from_args(&args, &["--controllers"])`), and delete the local copy.
  What players notice: under Wine (`<id>-play`, the AppImage, Proton) a
  wrong game folder, an unknown build or "already running" in the game run
  now shows a dialog instead of looking like a game that never started.
  `--settings --linux` and `--check --linux` show none (the window or the
  helper reports). Verify: the launcher's tests call
  `report::Audience::of` for its helper's command lines.
- **DLL overrides: only ours are removed** (SRR, DiRT, every launcher
  calling `wine::app_dll_overrides`). No code change. Values written from
  v0.4.0 on are recorded under
  `HKCU\Software\rea-launcher\DllOverrides\<exe>`;
  `remove_app_dll_overrides` deletes only recorded values. Values written by
  v0.3.3 have no record: they are now reported in the new
  `Applied::matching` field and kept when the component is turned off (with
  the component's DLL gone from the runtime a leftover `native,builtin`
  falls back to Wine's builtin). Code building an `Applied` literal adds
  `matching`; log it next to `ours`.
- **Second start: `instance::AlreadyRunning::second_start`** (SRR, DiRT,
  CMR4, CMR2005, Screamer). Replace local `instance.rs` `Second`/
  `SecondStart` and local window lookups with
  `running.second_start(action, Window::Own(TITLE))`, exit with
  `exit_code()` (0, or `instance::ALREADY_RUNNING` = 5) and report
  `message(game)` with the run's audience. Before Play, refuse with
  `SecondStart::Busy` when `process::find_running(&runtime_exe)` is not
  empty. CMR4 (1) and CMR2005 (0) change their refused-second-start exit
  code to 5. A `match` on `instance::Window` needs arms for `Own` and
  `ClassTitle`.
- **Fresh prefix, settings window: `gpu::prefer_glx_or_restart`**
  (CMR2005, the template, every eframe launcher under Wine). Replace
  `gpu::prefer_glx()` before the window with
  `match gpu::prefer_glx_or_restart(guard) { Glx::Restarted { exit_code } =>
  exit(exit_code as i32), Glx::Ready { instance, warning } => … }`.
- **Close hook `Requests` literal**: has a new field `alt_f4_state`; code
  building `Requests { … }` adds it. `Config::parse` users: nothing.
- CMR2005: delete `palette::light_widgets` and its calls; keep the palette
  test. Dark palettes: no visible change (egui's dark fills kept).
- Every launcher with a disabled chip row/switch outside spec rows: one
  more note line under it. OR: its `chip_row`/`bool_row` write
  `disabled.unwrap_or(row.description)` — now the reason shows twice;
  pass `row.description` or move to `spec_row_with`. Screamer: its
  `pad_off_reason` note under a disabled row duplicates too. SRR/CMR3
  (hover-only reasons) gain the visible note; nothing to change.
- Disabled switches show the saved value; tests reading `checked` on a
  disabled switch see it.
- Disabled look/game cards show the reason instead of the note/subtitle.
- Stepped rows (`RowOptions::steps`): a saved value between steps gets
  its own chip; a spec with `unset_label` gets the unset chip first.
- DiRT Rally: replace `jspad` with `pad::joystick::Joysticks`; route
  `linux::run`'s errors through `report::fatal` (`Mode::Play
  { interactive: !no_window }`, the unknown-edition issue form as `link`);
  `--help`/`--check` with `Mode::Command`.
- Optional: SRR `option_row`, OR `chip_row`, CMR3 `option_chips` for
  resolution → `RowOptions::sizes`; CMR3 FOVS + `with_saved` →
  `steps` + `unset_label`; CMR3 `xidi::note` → `RowOptions::notes`;
  OR's hand-drawn caption plate → `photo_caption_plated`.
- Play helper: `--play` / a plain start while the game runs exits 5, not
  0 (CMR1's and CMR3's `linux/tests/supervision.sh` only compare the
  output text, so they still pass; a test under `set -e` would not).
  CMR1: drop the local `rea_wineserver_wait`/`rea_hook_before_settings`
  override (83dacbb) once its launcher exits 5 (or keep exiting 3: also
  handled).
- Launchers with a `*_look_size` hook (CMR1, CMR2005, CMR4, OR) replace it
  with `REA_LOOK_SIZES` (and drop `look resolution` from REA_HELPER_KEYS
  if only listed for that).
- Snapshot tests: replace the local `check()` with `rea_launcher::snapshot!`
  (or `snapshots!().update_var("<ID>_UPDATE_SNAPSHOTS")`), drop the
  manual `.replace(env!("CARGO_PKG_VERSION"), "<version>")`. A stored
  snapshot with the literal version now fails: regenerate it.
- DiRT Rally: `copy.rs`'s measuring thread → `sync_with_progress` +
  `ProgressCounter`; native start → `instance::single` and exit
  `running.exit_code()`.
- launcher-ci now runs a `secrets` job by default (`secret-scan: false` to
  opt out); a repo with a false positive adds `.gitleaksignore` or
  `.gitleaks.toml`.
- CMR1: mark `cmr-hooks` with `[package.metadata.rea] wine-test = false`
  and drop `--exclude cmr-hooks` from `wine-test-args` (both work).
- Hosted pins in a release other than `thirdparty` add `tag=<release>`.
- Run `sdk/scripts/launcher-check` and fix every error; CI runs it now
  (`conformance: true`). Rules marked "from v0.4.0" turn from warnings into
  errors with the bump.
- Owner-approved DRM exceptions need a waiver line: CMR3 (DRM-free 1.1 via
  SilentPatch's patcher), Sega Rally Revo (NoDVD build), OutRun 2006
  (OutRun2006Tweaks' DRM-free exe), DiRT Rally (pre-activated copies):
  `drm-exception: owner decision <date>, <why> (<where recorded>)`.
- A native-Linux launcher with two `launcher-ci` jobs passes
  `conformance: false` to the Windows-tests job (one run is enough).

### Added

- `report::Run`, `report::Audience` (`Audience::of(run, wine)`,
  `.mode()`), `report::audience(run)` (Windows), `report::COMMAND_FLAGS`,
  `Run::from_args(args, extra_commands)`: who may be watching a run, from
  the family flags; tested for all 32 combinations of the four flags and
  Wine.
- `instance::Window::Own(title)` (a visible window with the title in a
  process running this launcher's exe: eframe windows all share winit's
  class, and a title alone matched Explorer folders and the game's
  window) and `Window::ClassTitle(class, title)`.
- `instance::SecondStart::{Focused, Busy, WindowOpen}` with `decide`,
  `exit_code`, `message(game)`, `log_line`; `instance::ALREADY_RUNNING`
  (5); `AlreadyRunning::second_start(action, window)`; pure
  `instance::is_own_window`, `instance::same_image`.
- `process::find_running(image) -> Vec<u32>` (Toolhelp32 +
  `QueryFullProcessImageNameW`) and `process::image_path(pid)`: a launcher
  killed while its game ran no longer lets a second game start from the
  same runtime copy.
- `gpu::prefer_glx_or_restart(Option<Guard>) -> Glx`,
  `gpu::GLX_RESTARTED_ENV`, `gpu::needs_restart`: on a fresh Wine prefix
  the launcher runs itself again once so the settings window draws through
  GLX on the first start.
- Close hook: `close_on = alt_f4_state` — for games with an exclusive
  DirectInput keyboard (CMR2005), the hook reads Alt+F4 from the keyboard
  state on any message while the game's own window is in front, once per
  press. Off by default. `rea_close_hook::Chord`.
- `wine::owned_key`, `wine::step` → `wine::Step`, `wine::removable`,
  `Applied::matching`.
- `theme::visuals(&Palette)` (the egui style the theme installs) and
  `Palette::is_light()`.
- `report` (native Linux messages): `report::native_audience(Mode, Native) ->
  NativeAudience { Terminal, Window, LogOnly }`, `Native { stdin_terminal,
  stderr_terminal, display }` + `Native::here()` / `Native::desktop()`,
  `Message { title, text, link: Option<Link { label, url }>, details }`,
  `report::fatal(&Palette, &Metrics, &Message, Mode) -> NativeAudience`,
  `report::show` / `show_with(…, buttons)`, `report::message_ui` +
  `MessageState` + `Action`, `report::pad_closes`, `report::WINDOW_SIZE`.
  A double-clicked native launcher that fails before its window now shows
  the error (with the issue link, Copy details and OK) instead of exiting
  silently.
- `pad::joystick` (Linux): `Joysticks::new()`, `Joysticks::buttons() -> u16`
  (XInput bits for `PadNav`), `Joysticks::with_scan(every, scan)`,
  `Joysticks::count()`, `State`, `devices(dir)`, `RESCAN`.
- Spec rows: `RowOptions::notes` (per-value notes by cfg text),
  `RowOptions::sizes: Option<SizeChips { screen }>` (resolution chips),
  `spec::size_choices`, `spec::SizeChoice`, `spec::COMMON_SIZES`,
  `spec::step_choices`.
- `widgets::photo_caption_plated` (wrapped caption on a solid `panel`
  plate), `widgets::disabled_reasons`, `widgets::join_sentences`.
- `testing::click_in_view(&mut harness, role, label)`.
- `rea_launcher::testing`: golden snapshots that survive a release bump.
  `rea_launcher::snapshot!("help.txt", text)` compares with the calling
  crate's `tests/snapshots/`, stores its `CARGO_PKG_VERSION` as
  `<version>`, refuses a stored snapshot that still holds the version,
  treats CRLF as LF, and names the first differing line.
  `REA_UPDATE_SNAPSHOTS=1 cargo test` writes them;
  `rea_launcher::snapshots!().update_var("CMR3_UPDATE_SNAPSHOTS")` keeps a
  launcher's own variable. (A help snapshot with the version broke the
  CMR3, CMR4 and OR tag builds.)
- `runtime::sync_with_progress(game, runtime, policy, manifest, &mut |p| …)`
  with `runtime::Progress { files_done, files_total, bytes_done,
  bytes_total }` (`fraction()`, `finished()`) and `runtime::ProgressCounter`
  (`new`, `set`, `get`; clones share) for a window showing "12 of 40 GB".
- `rea_launcher::instance::single(name) -> Result<Guard, AlreadyRunning>`:
  a `flock` lock for native Linux launchers (`$XDG_RUNTIME_DIR/
  rea-launcher-<name>.lock`), released by the kernel however the process
  ends; `single_at(path)`, `lock_path(name)`, `Guard::is_held/path`,
  `AlreadyRunning::exit_code()`, `instance::ALREADY_RUNNING = 5` (same as
  the win32 lane's).
- Play helper: `REA_LOOK_SIZES="original=640x480 steam_deck=1280x800
  enhanced=screen"` (+ `REA_LOOK_KEY`, default `look`) sizes gamescope
  from `--look` / `look =` as the launchers resolve it (this run's
  `--resolution`, its `--look`, the file's `resolution`, its look;
  `resolution = auto` = the look, else the screen). Look/resolution flags
  are read without listing them in `REA_HELPER_KEYS`.
- Play helper test seam `REA_GAMESCOPE_HEADLESS=1` (only with
  `REA_TEST=1`): gamescope `--backend headless`.
- Secret scanning: launcher-ci.yml `secrets` job (input `secret-scan`,
  default true) and the SDK's ci.yml `secrets` job, gitleaks 8.30.1
  (SHA-256 pinned), whole history + tree, every push and PR; findings show
  file, line, commit and rule, never the match. `scripts/prepush-scan
  [--all | --range A..B] [--no-tree] [repo]` scans what a push would
  publish (`@{upstream}..HEAD`) and the tree at HEAD the same way (exit 0
  clean, 1 findings, 2 tool error; `REA_GITLEAKS`, `REA_NO_FETCH`).
  Without a `.gitleaks.toml` the SDK allowlist skips play-script `*_KEY=`
  settings (the only false positive found; every launcher scans clean).
  dev-check runs it when gitleaks is installed; sdk-sync vendors it.
- `scripts/release [--date D] [--no-test] VERSION [repo]` (vendored as
  `sdk/scripts/release`): bumps the crate launcher-release's `package:`
  names (never the first crate; Cargo.toml or `[workspace.package]`),
  dates the Unreleased CHANGELOG section in the repo's own heading style
  (Keep a Changelog links moved on) or dates an undated `## X`, runs
  `cargo test --locked --workspace` after the bump, checks
  release-notes.sh, commits `chore: release vX`, annotated tag `vX`. Never
  pushes; any failure restores the files; refuses a dirty tree, an older
  or existing version, no/several/unknown package.
- `packaging/release-notes.sh --since vX VERSION`: every section after vX
  up to VERSION, with headings (fix-forward releases); launcher-release.yml
  input `notes-since` (a tag or `latest-release`).
- pins.sh: `REA_NO_FETCH=1` offline mode (destination, cache, `file://`
  only; says which pin is missing); a pin's `tag=RELEASE` word names its
  own mirror release (`pin_tag NAME`; `pin_mirror_url` uses it before
  `PIN_MIRROR_TAG`); mirror.yml uploads one release per tag.
- sdk-sync vendors `scripts/launcher-check` (guard lane's conformance
  check; launcher-ci runs `sdk/scripts/launcher-check --github`), checked
  by `--check` like every vendored file.
- wine-test: crates marked `[package.metadata.rea] wine-test = false` are
  excluded from `--workspace` runs; launcher-ci input `wine-test-exclude`.
- `scripts/launcher-check [--github] [--explain RULE|all] [--list] [repo]`:
  checks a launcher repository against the family rules a file can show,
  one line per problem (`error[rule] file:line: …` + `fix: …`), exit 1 on
  errors. 33 rules, from the mistakes v0.3 launchers shipped: golden
  snapshots (missing, holding the crate version, not using
  `rea_launcher::snapshot!`), hand-made audiences (`!no_window`, `!linux`,
  `console::output`), the Windows instance guard and the native-Linux
  lock, window lookup by title/class, native-Linux error window and pads,
  `prefer_glx` without restart, hand-made widget fills, bundled Microsoft
  DLLs without a Wine override, own fixes without a draft in
  CONTRIBUTIONS.md, README outline/order/Contributing link/issue links/
  versioned links, AGENTS.md pointer and copied STANDARDS, SDK workflow
  calls and one SDK tag across Cargo, `sdk/VERSION` and workflows, lld per
  Windows target, `wine-tests`, `--look`, a test naming every option, quit
  paths in RUNTIME_NOTES, the unknown-edition and bug-report forms (with
  "What worked"), `sdk-update.yml`, DRM exceptions. Rules that need a
  v0.4.0 API are warnings for launchers still on v0.3.x and errors from
  v0.4.0. Owner exceptions: `.rea-check-waivers` lines `rule: reason`
  (shown with the reason, never failing; unknown rules, empty reasons and
  stale waivers are reported).
- `launcher-ci.yml` input `conformance` (default true): runs the vendored
  `sdk/scripts/launcher-check --github` after the `sdk/` drift check;
  errors fail the job, warnings are annotations.
- Scaffold (`scripts/new-launcher`, `templates/launcher`): golden snapshots
  (`tests/snapshots.rs` + `tests/snapshots/`: `--help`, the `launcher.cfg`
  template, the first run, every look on four screens) through
  `rea_launcher::snapshot!`; who sees a message through
  `report::audience` (`Invocation::run()`, `audience(wine)`, test of the
  helper's command lines); one instance through `instance::single` +
  `second_start(action, Window::Own(WINDOW_TITLE))` and
  `process::find_running` before Play; `gpu::prefer_glx_or_restart`;
  `WINDOW_TITLE` = "<Game> Launcher" (never the game's title); the weekly
  `sdk-update.yml` caller; a "What worked?" checklist in the bug report;
  CONTRIBUTIONS.md with an id column, RESEARCH.md "Our own (draft C1)"
  and "Owner decisions", RUNTIME_NOTES.md "Quitting and leftovers" table.
  A fresh scaffold passes `launcher-check` with no errors (tested in
  `scripts/tests/new-launcher.sh`).
- Docs: STANDARDS "What a machine can check, it checks", "Every fix of our
  own gets a draft", "DRM exceptions", audience/instance APIs named,
  window title rule, snapshot helper; new-launcher playbook "Patterns"
  (native Linux recipe, old native Linux ports, installer source, portrait
  monitor, saves via registry, test notes); ci-and-releases: native-Linux
  two-job shape, `conformance`, pre-push scan, release via
  `scripts/release`, gentle polling + `gh release view`; readme playbook:
  headless screenshots, DRM wording; contribute-back and sdk-change: the
  draft rule, "When a launcher repeats a mistake". Skills `rea-new-launcher`
  and `rea-sdk-change`: run launcher-check before every commit and before
  a release; its errors block.
- `launcher-check` rule `look-sizes`: a Linux helper whose launcher has
  looks must declare `REA_LOOK_SIZES`; the scaffold declares its three
  looks' sizes.

### Changed

- **Play helper exit codes**: a start while our game runs (`--play`, and
  the plain/settings start) exits **5** (`REA_EXIT_RUNNING`) instead of 0.
  The settings window's 5 (the launchers' `ALREADY_RUNNING`) prints
  "another launcher (or its game) is already running; nothing started"
  and the helper exits 5. After a 5 or a 3, no `wineserver -w` (CMR1
  83dacbb's workaround is now built in). The inner script exits 5 without
  waiting for, or `wineserver -k`-ing, another launcher's game when the
  launcher refuses a play run with 5.
- `REA_FLAG_SWITCHES` default adds `--help --version`.
- `rea_game_alive` ignores a process whose environment names another
  `WINEPREFIX`/`WINECONFIGDIR` (only that value is read, nothing printed;
  unreadable or prefix-less environments still count, so the wait never
  ends early). The inner script gets `REA_GAME_PREFIX`.
- wine-test exports `LANG=C.UTF-8 LC_ALL=C.UTF-8`.
- appimage-wine.sh reads the Wine version from the archive's top folder
  first (a `file://` pin named by its checksum builds offline).
- `scripts/new-launcher` stamps `sdk/VERSION` with the tag it pins
  (`sdk-sync --tag`), also from an untagged SDK checkout.
- Template: the `game` option's description no longer names the game (so
  the `launcher.cfg` snapshot doesn't depend on the title's length); the
  program tests run with `--linux` (under Wine a player's run shows
  dialogs, which would hang a headless test); a bad command line is
  reported as a command (terminal, or a dialog when nobody sees one).

### Fixed

- `wine::remove_app_dll_overrides` deleted a value the player had set when
  it equalled the launcher's (DiRT review). Ownership is now recorded when
  the launcher writes, and only recorded, unchanged values are removed; a
  value of ours the player changed becomes theirs.
- Light palettes (CMR2005) got dark-grey egui widgets under dark text
  (~1.5:1) that `contrast_problems` could not see. Light bases now start
  from `Visuals::light()` with widgets filled from `panel` (hover +6 % ink,
  pressed +12 %), ink strokes and links; `Palette::pairs` checks ink on
  every egui widget fill and state, tooltips, and the link colour.
- Disabled controls write their reason on the window (pad players never
  hover): under a chip row, under a switch's label, in place of a card's
  note/subtitle, in place of a spec row's description.
- A disabled switch shows its saved value (it looked off).
- A look card's description no longer doubles the full stop when the note
  ends with "." and a reason is appended (SCR); spec-row descriptions are
  `description. reason` (were `description reason`).
- sdk-bump no longer clones into `~/.cache`: `$REA_SDK_CACHE`, else the
  launcher's ignored `.work/sdk-checkout`, else a temp folder it removes;
  it no longer rewrites Cargo.toml files under `.work/`, `target/` or other
  ignored folders (SCR).
- sdk-sync `--check` with an SDK checkout at the recorded commit but with
  uncommitted changes compared with the dirty tree (false drift); it now
  goes through a clone of the recorded commit.

### Documentation

- Wine replaces a DLL whose version resource names Microsoft as the vendor
  with its builtin even from the program folder; bundled redistributables
  (`d3dx9_*`, `d3dcompiler_*`, `xinput1_3`, `msvcp140`, …) need
  `app_dll_overrides`. Other vendors' DLLs (DXVK, dgVoodoo, Xidi) win by
  default (Sega Rally Revo's finding). In the win32 README and the `wine`
  module docs.

## [0.3.3] - 2026-11-02

Wine DLL overrides that actually reach the game, found in a real-game run.

### Migration

- **Bundled DLLs under plain Proton/Steam** (every Wine launcher that
  ships a DLL Wine also has: d3d9/dxgi for DXVK, d3dx9_*, dinput8 for Xidi
  or OutRun2006Tweaks, dsound…): `WINEDLLOVERRIDES` set in the game's
  environment block by `Launch::env` is ignored by Wine's loader. Before
  starting the game, under Wine, call
  `rea_launcher_win32::wine::app_dll_overrides(game_exe_name, &[(dll,
  DllMode::NativeThenBuiltin), …])` (it writes
  `HKCU\Software\Wine\AppDefaults\<exe>\DllOverrides` only where no value
  exists), and `remove_app_dll_overrides` when the component is turned
  off. Keep the environment value too (harmless). Verify: start the
  launcher's exe directly with `wine` (no `<id>-play`, no
  `WINEDLLOVERRIDES` exported) and check the game loads the bundled DLL
  (`WINEDEBUG=+loaddll` shows `native` for it).

### Fixed

- `process::Launch`: documented that Wine ignores `WINEDLLOVERRIDES` in
  the child's environment block (proved by a Wine test with a native
  stand-in DLL: the default loads the program folder's copy, the block
  changes nothing, the AppDefaults registry key is honoured).

### Added

- `wine::app_dll_overrides` / `remove_app_dll_overrides` (per-program
  `DllOverrides`, the player's values kept, ours removed only while
  unchanged), `wine::DllMode`, `app_defaults_key`, `plan`;
  `reg::Key::delete_value`.

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

[0.6.0]: https://github.com/akitaonrails/rea-launcher-sdk/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/akitaonrails/rea-launcher-sdk/compare/v0.4.5...v0.5.0
[0.4.5]: https://github.com/akitaonrails/rea-launcher-sdk/compare/v0.4.4...v0.4.5
[0.4.4]: https://github.com/akitaonrails/rea-launcher-sdk/compare/v0.4.3...v0.4.4
[0.4.3]: https://github.com/akitaonrails/rea-launcher-sdk/compare/v0.4.2...v0.4.3
[0.4.2]: https://github.com/akitaonrails/rea-launcher-sdk/compare/v0.4.1...v0.4.2
[0.4.1]: https://github.com/akitaonrails/rea-launcher-sdk/compare/v0.4.0...v0.4.1
[0.4.0]: https://github.com/akitaonrails/rea-launcher-sdk/compare/v0.3.3...v0.4.0
[0.3.3]: https://github.com/akitaonrails/rea-launcher-sdk/compare/v0.3.2...v0.3.3
[0.3.2]: https://github.com/akitaonrails/rea-launcher-sdk/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/akitaonrails/rea-launcher-sdk/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/akitaonrails/rea-launcher-sdk/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/akitaonrails/rea-launcher-sdk/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/akitaonrails/rea-launcher-sdk/releases/tag/v0.1.0
