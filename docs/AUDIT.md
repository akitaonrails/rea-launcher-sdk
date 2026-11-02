# Cross-launcher audit (2026-11-02)

Survey of the five sibling launchers, done before normalising them and
extracting this SDK:

| Short | Repo | Game | Runs through |
|---|---|---|---|
| CMR3 | `rea-colin-mcrae-3` | Colin McRae Rally 3 (2003) | Win32 exe; SilentPatch, Xidi, DXVK; Wine/Proton on Linux |
| CMR4 | `rea-colin-mcrae-4` | Colin McRae Rally 04 | Win32 exe; dgVoodoo2, Xidi, DXVK; byte patch; close-hook DLL |
| SCR | `rea-screamer` (GitHub: rea-screamer-trilogy) | Screamer trilogy (DOS) | native Linux + Windows; patched dosbox-staging + hooks |
| SRR | `rea-sega-rally-revo` | Sega Rally Revo (2007) | Win32 exe; byte patches; DXVK |
| OR | `rea-outrun-2006` | OutRun 2006 C2C | Win32 exe; OutRun2006Tweaks; DXVK; HD pack download |

All are Rust 2024, eframe 0.37 (glow) + windows-sys 0.61, a settings window
with the same look (Barlow fonts, portrait art, looks/presets, tabs, Play
plate), a `launcher.cfg` key=value file, a private runtime folder built from
the read-only game folder, and a POSIX `linux/<x>-play` helper plus an
AppImage with portable Wine.

Siblings outside this audit with the same shape: `rea-colin-mcrae` (CMR1),
`rea-dirt`.

## Best implementation per shared feature

| Feature | Reference | Why |
|---|---|---|
| Xidi.ini generation | CMR3 `xidi.rs` | presets + notes, log toggle, golden fixture, DLL hash pinned |
| Proxy DLL install/remove | CMR4 `components.rs` | SHA-256 ownership ledger; restores the game's own DLL; never deletes a user's file |
| Wine winebus registry | OR `win32.rs` | done in-launcher (works under Proton), correct key names, only-if-absent |
| Wine DLL overrides | CMR4 `wine_overrides` + env block | deterministic merge with the user's, works without the shell helper |
| Win32 helpers / registry | CMR4 `win32.rs` | RAII handles, VirtualStore fallback, `is_wine` env fallback |
| Monitor detection | CMR4 `display.rs` + `win32::monitors` | all monitors, physical modes, portrait-primary aware |
| Settings window sizing | CMR4 `ui.rs` | final size at creation under Wine (XWayland), DPI-aware fit on Windows |
| Config file + CLI | CMR3 `config.rs` | one `apply(key,value)` for file and flags, comment-preserving `set_key`/`clear_key`, unset = keep game value |
| Looks/presets | CMR3 model + OR masked compare | stored values, look derived, Custom remembered |
| Pad navigation in window | SCR | visible focus, left/right stepping, B/back |
| Rumble test | SRR / OR | — |
| Exe identification | SCR / SRR | SHA-256 table, unknown reports digest; SRR records patched hashes |
| Byte patching | SCR `prepare` + SRR cave checks; CMR4 signature toggle | expected-or-patched bytes, all-or-nothing, idempotent |
| Runtime sync | CMR3 link+manifest engine | whole-file swaps never write through links |
| Runtime/game overlap check | CMR4 `resolve` | canonicalises nearest existing parent |
| Save mirroring | CMR3 `mirror_file` | newer wins; survives a crash before write-back |
| INI editing | OR `ini.rs` (+ CMR3 case-insensitive keys) | preserves comments, unknown keys, newline shape |
| Pinned downloads | OR `hdpack.rs` | resumable, streaming SHA-256, free-space, zip-slip guard, atomic |
| Linux session/lock/stop | CMR4 `cmr4-play` | pid+start-time lock, own session, group kill; tested |
| Linux inner script | CMR4 | quoted heredoc + positional args (no injection) |
| gamescope wrapper | CMR3 `cmr3-play` | scaler from cfg, windowed bypass, nesting guard, -W/-H + -w/-h |
| GPU env | SCR (test hooks) | same function in CMR3/CMR4/OR |
| CI release integrity | SCR | `--locked`, SHA-pinned actions, tag/version check, SHA256SUMS, attestation |
| CI checks | SRR | cargo audit + deny, `sh -n`, import resolution test |
| Package allowlist | CMR4 `smoke-package.sh` | rejects unlisted files and game assets |
| Pin file format | CMR3 `third-party.txt` + OR `pins.sh` | fallback mirrors + clean loader |

## Bugs found (status tracked in each repo's CHANGELOG)

CMR3
- DRM-free xdelta conversion could never run: `xdelta_args` repeats the
  program path as argv[1]; xdelta3 then fails "too many filenames"
  (reproduced with xdelta3 on the host).
- `runtime::sync` sets mtimes on freshly hard-linked files and
  `make_writable` clears read-only before delete — both touch the user's
  original through the link.
- HD UI overlay cannot be switched off once copied.
- Wine DLL overrides only set by `cmr3-play`; plain Proton loads builtins.
- Xidi "Off" deletes any `dinput8.dll`, including a user's.
- `mirror.yml` uploads `SilentPatch.zip` but the fallback URL expects
  `SilentPatchCMR3.zip`; watch-upstream echoes step output into shell.
- `sp::set` with a missing section and no trailing newline glues the header.

CMR4
- AppImage runtime pinned by checksum to the moving `continuous` URL.
- THIRD_PARTY lists Xidi as MIT (it is BSD-3-Clause).
- `Settings::render` rewrites launcher.cfg wholesale (drops comments) and
  persists CLI overrides.

SCR
- Linux runtime symlinks top-level files and whole directories to the
  user's folder — the game can write into it.
- `resolution = fullscreen` reaches gamescope as `-w fullscreen`; anything
  above 1920×1080 is silently turned into fullscreen.
- Save sync overwrites without newer-wins; no runtime/game overlap check.
- No window → fatal instead of playing with saved settings.

SRR
- `srr-play` writes `"Disable Hidraw"`/`"Disable Input"` (Wine reads
  `DisableHidraw`/`DisableInput`).
- `gpu_env` pins the NVIDIA Vulkan ICD whenever its json exists, even
  without an NVIDIA device; `mem_info_vrm_total` typo.
- Custom look never stays selected; "Recommended" hard-coded.
- `config.ini` key matching does not trim (`Width = 1280` → duplicate).
- Issue template copied from DiRT.

OR
- Save mirror copies the player folder over the runtime with no
  newer-wins check.
- Settings window resized after creation under Wine (XWayland bug).
- `or-play stop` kills the helper shell, not the game session.

All Wine launchers
- Play-script lock is pid-only (pid reuse); CMR3/SRR/OR interpolate the
  launcher path into a generated script (quote injection).
- First run detected by `text == TEMPLATE`.
- `GetSystemMetrics` used without DPI awareness on the `--no-window` path.
