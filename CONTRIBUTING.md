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
3. Run everything with `scripts/dev-check` from the repository root. It
   skips checks whose tool is missing and says so; CI runs them all.
4. Docs: the crate or folder README; **Migration** notes (in the crate
   README's MIGRATION section and the CHANGELOG entry) whenever a launcher
   must change something to adopt the release; an entry under
   `## [Unreleased]` in `CHANGELOG.md`.
5. Commits: conventional style (`fix(core): …`, `feat(scripts): …`).

## Release process

1. Move the `## [Unreleased]` entries in `CHANGELOG.md` to
   `## [X.Y.Z] - YYYY-MM-DD` and update the link references.
2. Bump `version` in `[workspace.package]` (and the `rea-launcher`
   workspace dependency version) in `Cargo.toml`; `cargo update -w`.
3. `scripts/dev-check` clean (and CI green on the release commit).
4. Tag and push: `git tag -a vX.Y.Z -m "rea-launcher-sdk vX.Y.Z"`,
   `git push origin main vX.Y.Z`.
5. Each launcher (`docs/playbooks/sdk-change.md`):
   - change `tag = "vX.Y.Z"` on every `rea-launcher*` dependency and
     `cargo update -p rea-launcher -p rea-launcher-win32 -p rea-launcher-ui`;
   - from an SDK checkout at the tag, `scripts/sdk-sync <launcher>` (the
     launcher's CI fails on drift from `sdk/VERSION`);
   - move `@vX.Y.Z` on the reusable workflows in `.github/workflows/`;
   - apply the Migration notes of every skipped version and drop local
     workarounds the release made unnecessary;
   - run the launcher's checks, `sdk/scripts/wine-test` and
     `scripts/sdk-sync --check <launcher>`; add a CHANGELOG entry; commit
     `build: bump rea-launcher-sdk to vX.Y.Z`.

Licensed under MIT or Apache-2.0, at your option; contributions are
accepted under the same terms.
