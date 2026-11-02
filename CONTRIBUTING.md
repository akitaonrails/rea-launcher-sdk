# Contributing

rea-launcher-sdk holds the code the rea launchers share. Read
`docs/DESIGN.md` (what the SDK is) and `docs/STANDARDS.md` (the rules every
launcher follows) first.

## SDK or launcher?

| Belongs in the SDK | Stays in the launcher |
|---|---|
| Mechanisms every launcher needs: runtime sync, installs, save mirroring, config/INI editing, Win32 helpers, the settings-window kit, `rea-play.sh`, packaging scripts, CI workflows | Game knowledge: edition tables, patch sites, config generators (SilentPatch, Tweaks, dgVoodoo, DOSBox), profile layouts, which files the game writes |
| Generic widgets and options, opt-in | The game's tabs, looks, presets and their values |
| The AppImage builder, pin loader, allowlist checker | Pin files, allowlists, desktop entries, icons, which archive members ship |
| Reusable workflows | Game-specific jobs (mirrors, upstream tweak updates, native DOSBox builds) |

Game knowledge reaches the SDK as data or a trait implementation, never as
a hard-coded case. A behaviour a game must verify (a new option, a changed
default) is opt-in until every launcher using it has verified it.

## Where to report

- A bug or gap in shared code: an issue or pull request **here**, naming the
  launcher, what it needs and where (file:line). Don't copy SDK code into a
  launcher and never edit a launcher's vendored `sdk/`; keep the launcher's
  local code until the SDK release that replaces it.
- A game-specific problem: the launcher's own repository.

## Proposing a change

1. Make it generic (see above) and keep the rules in `docs/DESIGN.md`:
   pure logic in `rea-launcher` with host tests, `unsafe`/FFI only in
   `rea-launcher-win32`, whole-file swaps, unset settings mean "keep the
   game's value".
2. Tests:
   - Rust: host tests, plus the Windows-target tests under Wine for
     anything touching Win32 or paths: `scripts/wine-test --locked
     --workspace` (add `--target x86_64-pc-windows-gnu` for 64-bit code).
   - Shell: `shell/tests/` for `rea-play.sh`, `packaging/tests/` and
     `scripts/tests/` for the scripts; POSIX sh only, checked under dash
     with mawk.
   - Workflows: actionlint; shell steps are exercised by
     `scripts/tests/workflows.sh`.
   - Tests run headless: nothing opens a window on your desktop.
3. Run everything with `scripts/dev-check` from the repository root; for a
   change that touches launcher-facing API, run the canary
   (`compat.yml`, also on every PR) and read any red launcher job. It
   skips checks whose tool is missing and says so; CI runs them all.
4. Docs: the crate or folder README; whenever a launcher must change
   something to adopt the release, a bullet under `### Migration` in the
   `## [Unreleased]` section of `CHANGELOG.md` (and the crate README's
   MIGRATION section); the other entries under `### Added`/`Changed`/`Fixed`.
5. Commits: conventional style (`fix(core): …`, `feat(scripts): …`).

## Before tagging

Tag only when every box holds:

- [ ] `scripts/dev-check` clean, and CI green on the release commit.
- [ ] **Canary** (`.github/workflows/compat.yml`) green for all five
      launchers, or each red/allowed-to-fail job explained: a launcher still
      on an older SDK API may fail, but you know which change breaks it and
      its CHANGELOG Migration item says how to fix it. Never tag over a red
      job you haven't read. An allowed failure (`REA_CANARY_ALLOW_FAILURE`)
      is removed once that launcher has moved.
- [ ] `CHANGELOG.md` complete for the version, with a `### Migration`
      section first in the release: every change a launcher must act on is
      its own bullet there (never only a sentence inside an Added or Changed
      bullet) that says **what to do** (the new call, field or input) and
      **how to verify** it (the test, check or screen that shows it
      worked). `sdk-bump` and `sdk-update.yml` copy that section into each
      launcher's bump PR, so each bullet must stand alone; a release with
      nothing to do has no Migration section. Check with
      `scripts/sdk-bump --no-cargo --since v<previous> v<this> <a launcher>`
      on a scratch clone of a launcher.
- [ ] Docs (crate READMEs, `packaging/README.md`, playbooks) match the code.

## Release process

1. Move the `## [Unreleased]` entries in `CHANGELOG.md` to
   `## [X.Y.Z] - YYYY-MM-DD` and update the link references.
2. Bump `version` in `[workspace.package]` (and the `rea-launcher`
   workspace dependency version) in `Cargo.toml`; `cargo update -w`.
3. Go through "Before tagging".
4. Tag and push: `git tag -a vX.Y.Z -m "rea-launcher-sdk vX.Y.Z"`,
   `git push origin main vX.Y.Z`.
5. Each launcher (`docs/playbooks/sdk-change.md`) moves by a pull request,
   never directly:
   - its weekly `sdk-update` workflow (`packaging/README.md`, "Staying
     current") opens `sdk-update/vX.Y.Z` with the bump and the Migration
     items; or by hand, from the launcher's root:
     `sdk/scripts/sdk-bump vX.Y.Z` (or `<sdk>/scripts/sdk-bump vX.Y.Z
     <launcher>`), which moves the Cargo tags, runs `cargo update`,
     re-vendors `sdk/`, moves `@vX.Y.Z` on the reusable workflows and prints
     the Migration items;
   - apply each Migration item and verify it as the item says; drop local
     workarounds the release made unnecessary;
   - run the launcher's checks, `sdk/scripts/wine-test` and
     `scripts/sdk-sync --check <launcher>`; add a CHANGELOG entry; merge as
     `build: bump rea-launcher-sdk to vX.Y.Z`.

Licensed under MIT or Apache-2.0, at your option; contributions are
accepted under the same terms.
