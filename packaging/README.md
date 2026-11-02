# Packaging and CI

Shared packaging scripts and reusable GitHub workflows for the rea
launchers. Everything here is POSIX sh (tested with dash and mawk, and with
busybox) and is vendored into each launcher by `scripts/sdk-sync`.

| File | What it does |
|---|---|
| `pins.sh` | Library: pin files, checksum-verified downloads with mirror fallback and a cache |
| `appimage-wine.sh` | Builds the AppImage: portable Wine (or `--native`), AppRun from `appimage/AppRun.in`, docs and licences inside |
| `appimage/tools.pins` | Pins for Wine, appimagetool and the dated type2 runtime |
| `appimage/wine/` | Wine's licence and source notes, shipped in every Wine AppImage |
| `smoke-package.sh` | Checks a package folder or zip against an allowlist |
| `release-notes.sh` | Prints a version's CHANGELOG section |
| `../.github/workflows/launcher-ci.yml` | Reusable: checks, build, package, allowlist, upload |
| `../.github/workflows/launcher-release.yml` | Reusable: tag/version check, SHA256SUMS, attestation, release |
| `../.github/workflows/upstream-watch.yml` | Reusable: opens an issue when an upstream release no longer matches its pin |
| `../.github/workflows/mirror.yml` | Reusable: copies pins marked `mirror` into the repository's own `thirdparty` release |

Tests: `sh packaging/tests/<name>.sh` and `sh scripts/tests/sdk-sync.sh`
(no network: downloads come from `file://` urls).

## Vendoring into a launcher

From an SDK checkout at the tag you want:

```sh
scripts/sdk-sync ../rea-colin-mcrae-4          # writes ../rea-colin-mcrae-4/sdk/
scripts/sdk-sync --check ../rea-colin-mcrae-4  # exit 1 on drift
```

Layout in the launcher: `sdk/rea-play.sh` (the play library, from the SDK's
`shell/`), `sdk/scripts/wine-test` (Windows-target tests under Wine),
`sdk/packaging/...` (as here). Helpers look for `rea-play.sh`
beside themselves first, then at `../sdk/rea-play.sh`; a zip that ships the
helper ships the library beside it too.

`sdk/` is owned by the script: `sdk/VERSION` records the SDK tag and commit,
and `launcher-ci.yml` fetches that exact SDK version and fails when `sdk/`
differs from it (edited, missing or extra files, exec bits). Commit `sdk/`
with the launcher; never edit it by hand.

## Pin files

```
# name url sha256 [fallback-url ...]
xidi https://github.com/samuelgr/Xidi/releases/download/v4.3.1/Xidi-v4.3.1.zip 78120f44…17ff https://github.com/akitaonrails/rea-colin-mcrae-3/releases/download/thirdparty/Xidi-v4.3.1.zip
dxvk-d3d9-sha256 - 265888c3…97fa
```

The first url that downloads and matches wins; `-` is a checksum-only pin.
Per-name files (`$PIN_DIR/<name>.txt`, one `url sha256 [fallback…]` line)
are read when `PIN_FILE` has no such name, so OutRun's `packaging/*.txt`
work unchanged.

```sh
. sdk/packaging/pins.sh
PIN_FILE=packaging/third-party.txt     # or PIN_DIR=packaging
PIN_CACHE=${REA_CACHE:-$HOME/.cache/rea-launcher-sdk/pins}
zip=$(pin_fetch xidi "$WORK")          # prints the verified file's path
sha=$(pin_sha dxvk-d3d9-sha256)
pin_verify thirdparty/dxvk/d3d9.dll "$sha"
```

Fetched files keep the primary url's file name whichever mirror served them
(`pin_file NAME`). Only `https://` and `file://` are allowed (redirects:
`https` only).

### Mirroring

When a component's licence allows republishing, add the word `mirror` to its
pin and list our mirror url as the last fallback:

```
xidi https://github.com/samuelgr/Xidi/releases/download/v4.3.1/Xidi-v4.3.1.zip 78120f44…17ff mirror https://github.com/akitaonrails/rea-colin-mcrae-3/releases/download/thirdparty/Xidi-v4.3.1.zip
```

`pin_mirror_url xidi [owner/repo [tag]]` prints that url (repository from
`PIN_MIRROR_REPO` or `GITHUB_REPOSITORY`, tag `thirdparty`); the asset is
always named by `pin_file`, so the fallback and the upload can't disagree.
The mirror workflow (`mirror.yml`) fetches every pin marked `mirror`,
checks its sha256, creates the release if it is missing (never as "latest")
and uploads with `--clobber`. It never deletes the release, so the fallback
urls stay valid while it runs, and it warns about a marked pin whose
fallbacks lack the mirror url. Run it by hand after adopting a new pin:

```yaml
name: mirror
on: workflow_dispatch
jobs:
  mirror:
    permissions: {contents: write}
    uses: akitaonrails/rea-launcher-sdk/.github/workflows/mirror.yml@v0.2.0
    with:
      pins: packaging/third-party.txt
```

Never mark a pin `mirror` without a licence that allows redistribution
(record it in `THIRD_PARTY.md`); a licence-less download stays a download
from its author.

## AppImage

```sh
sdk/packaging/appimage-wine.sh --app cmr4 --exe CMR4Launcher.exe \
  --icon packaging/appimage/cmr4-launcher.png \
  --desktop packaging/appimage/cmr4-launcher.desktop \
  --payload components --doc licenses --doc docs \
  --name "CMR4Launcher-$VERSION" dist/CMR4Launcher-$VERSION dist
```

Layout: the launcher, `linux/<app>-play` with the SDK play library beside
it (`usr/lib/<app>/rea-play.sh`: `--play-lib FILE`, else the vendored
`sdk/rea-play.sh`, else `shell/rea-play.sh` when run from an SDK checkout),
and every `--payload` path go to `usr/lib/<app>/` at the same relative
paths. A helper that mentions `rea-play.sh` fails the build when no library
is found, so an AppImage never ships without it; README, THIRD_PARTY, both licences, CHANGELOG,
`linux/README.md` and every `--doc` go to `usr/share/doc/<app>/`; Wine to
`usr/lib/wine` (development tools stripped) with its licence files in
`usr/share/doc/wine`. AppRun exports `<PREFIX>_LAUNCHER`, `<PREFIX>_WAIT=1`,
any `--env-path VAR=PATH`, and (Wine) `WINEDLLOVERRIDES=mscoree,mshtml=`
ahead of the player's own and `WINEDEBUG=-all` unless set, then runs the
helper. The helper keeps choosing the prefix (`<PREFIX>_PREFIX`).

Per launcher:

| Launcher | Arguments |
|---|---|
| CMR3 | `--app cmr3 --exe CMR3Launcher.exe --payload thirdparty --env-path CMR3_THIRDPARTY=thirdparty` |
| CMR4 | `--app cmr4 --exe CMR4Launcher.exe --payload components --doc licenses --doc docs` |
| SRR | `--app srr --exe SRRLauncher.exe --payload linux/d3d9.dll` (licence via `--doc linux/dxvk-LICENSE`) |
| OutRun | `--app or --exe OutRunLauncher.exe --payload linux --optional-payload tweaks --optional-payload vc --optional-payload soundtrack` |
| Screamer | `--native --app screamer --exe ScreamerLauncher --payload dosbox --payload lib --optional-payload resources --env-path SCREAMER_DOSBOX=dosbox` |

plus `--icon` and `--desktop` (or `--title`/`--comment` to generate the
entry). Moving the Wine pin means updating `appimage/wine/SOURCE.txt`; the
builder refuses notes that name another Wine version.

## Allowlist

`packaging/package.allow` in the launcher, checked by
`smoke-package.sh <folder|zip> packaging/package.allow`:

```
CMR4Launcher.exe
README.md
linux/cmr4-play x
docs/images/*.png
licenses/Xidi/ThirdParty/*
components/cmr4close.dll
```

Flags after a path: `x` (executable), `empty` (may be empty), `optional`.
A path with `*` is a pattern. Anything not listed fails.

A zip that ships the helper must ship the play library beside it: the
package script copies `sdk/rea-play.sh` to `linux/rea-play.sh`, and the
allowlist lists `linux/rea-play.sh`.

## A launcher's build.yml

```yaml
name: build

on:
  push:
    branches: [main]
    tags: ["v*"]
  pull_request:
  workflow_dispatch:

concurrency:
  group: build-${{ github.ref }}
  cancel-in-progress: ${{ !startsWith(github.ref, 'refs/tags/') }}

permissions:
  contents: read

jobs:
  ci:
    uses: akitaonrails/rea-launcher-sdk/.github/workflows/launcher-ci.yml@v0.1.0
    with:
      apt-packages: gcc-mingw-w64-i686 lld zip unzip
      shell-tests: linux/tests/*.sh packaging/tests/*.sh
      package-command: |
        packaging/build-windows.sh "$REA_VERSION_NAME" dist "$REA_CACHE"
        "$REA_SDK/packaging/appimage-wine.sh" --app cmr4 --exe CMR4Launcher.exe \
          --icon packaging/appimage/cmr4-launcher.png \
          --desktop packaging/appimage/cmr4-launcher.desktop \
          --payload components --doc licenses --doc docs \
          --name "CMR4Launcher-$REA_VERSION_NAME" \
          "dist/CMR4Launcher-$REA_VERSION_NAME" dist
      package-dir: dist/CMR4Launcher-*/
      package-allowlist: packaging/package.allow
      artifact-name: CMR4Launcher-packages
      artifact-paths: |
        dist/*.zip
        dist/*.AppImage

  release:
    if: startsWith(github.ref, 'refs/tags/v')
    needs: ci
    permissions:
      contents: write
      id-token: write
      attestations: write
    uses: akitaonrails/rea-launcher-sdk/.github/workflows/launcher-release.yml@v0.1.0
    with:
      package: cmr4-launcher
      artifact-pattern: CMR4Launcher-*
```

Inputs of `launcher-ci.yml` (all optional):

| Input | Default | Notes |
|---|---|---|
| `runs-on` | `ubuntu-24.04` | Screamer: `ubuntu-22.04` (older glibc for its native binaries) |
| `concurrency-key` | empty | set a distinct value per call when one workflow calls launcher-ci twice, or the calls cancel each other |
| `rust-toolchain` | `stable` | |
| `rust-target` | `i686-pc-windows-gnu` | empty builds for the host (Screamer) |
| `clippy-args` | `--all-targets` | CMR3/SRR may pass `""` (library and binaries only) |
| `apt-packages` | `gcc-mingw-w64-i686 lld zip unzip` | |
| `apt-i386` | `false` | `true` runs `dpkg --add-architecture i386` first (CMR3's `:i386` packages) |
| `submodules` | `false` | Screamer: `recursive` |
| `shell-syntax` | `linux/*-play packaging/*.sh …` | globs checked with `sh -n`; any error fails |
| `shell-tests` | `linux/tests/*.sh packaging/tests/*.sh` | globs; each match runs with `sh` (dash) and must exit 0; unmatched globs are skipped |
| `shell-tests-skip` | `*e2e*` | patterns of tests not to run (CMR3's `e2e.sh` needs a dev kit) |
| `wine-tests` | `false` | `true` installs Wine and the target's MinGW and runs `sdk/scripts/wine-test --target <rust-target>` headless |
| `wine-test-args` | `--locked --workspace` | |
| `audit` | `true` | cargo-audit 0.22.1 + cargo-deny 0.19.8; fails without a `deny.toml` |
| `sdk-check`, `sdk-dir`, `sdk-repository` | `true`, `sdk`, this repo | drift check against `sdk/VERSION` |
| `imports-test`, `package-command` | empty | bash commands |
| `package-dir`, `package-allowlist` | empty | glob of package folders/zips and the allowlist |
| `artifact-name`, `artifact-paths` | empty | upload |

Commands run in bash with Commands run in bash with
`REA_VERSION_NAME`, `REA_RELEASE_DIR`, `REA_CACHE` and `REA_SDK` set. SRR's
import check, for example:
`imports-test: SRR_LAUNCHER_EXE="$REA_RELEASE_DIR/SRRLauncher.exe" cargo test --locked --test imports -- --ignored`.
Every check fails its step on error: `scripts/tests/workflows.sh` runs the
shell steps as GitHub does (`sh -e`) against fixtures.

`launcher-release.yml` takes `package` (the Cargo package the tag must
match), `artifact-pattern`, and optionally `changelog`, `notes`
(`changelog` or `generate`), `attest` and `sdk-dir`. It runs three jobs:
`check` (tag, version, notes), `attest` (only when `attest` is true, and the
only job with `id-token`/`attestations` write) and `release`
(`contents: write`). Any extra job that
uploads release files (Screamer's Windows and source archives) only needs
an artifact name matching the pattern and a place in `needs:`.

A scheduled pin watch (as CMR3's). Names without a pin and unreachable
repositories are warnings in the run summary; only a changed checksum (or a
missing or undownloadable asset) opens an issue:

```yaml
on:
  schedule: [{cron: "23 4 * * 1"}]
  workflow_dispatch:
jobs:
  watch:
    permissions: {contents: read, issues: write}
    uses: akitaonrails/rea-launcher-sdk/.github/workflows/upstream-watch.yml@v0.1.0
    with:
      pins: packaging/third-party.txt
      watch-list: packaging/upstream-watch.txt
```

## Migrating a launcher

1. Vendor the SDK: `scripts/sdk-sync <launcher>` and commit `sdk/`.
2. **deny.toml:** copy `templates/deny.toml` to the launcher root (CMR3,
   CMR4 and OutRun have none; SRR's and Screamer's reject the SDK's git
   source through `unknown-git = "deny"`). The template allows
   `https://github.com/akitaonrails/rea-launcher-sdk`. Without a
   `deny.toml`, `launcher-ci` fails when `audit` is on.
3. **Release permissions:** the job that calls `launcher-release.yml` must
   grant `contents: write`, `id-token: write` and `attestations: write`,
   **even with `attest: false`**. GitHub checks a called workflow's job
   permissions when it loads it, before evaluating `if:`, so a missing
   grant fails the whole release workflow at startup.
4. Ship the play library: copy `sdk/rea-play.sh` beside the helper in the
   zip (`linux/rea-play.sh`) and list it in the allowlist; the AppImage
   builder places its own copy.
5. Replace `packaging/appimage/build.sh` and `AppRun` with a call to
   `sdk/packaging/appimage-wine.sh` (arguments in the table above), and the
   wine licence files with the SDK's (`sdk/packaging/appimage/wine/`).
6. Replace build.yml with the example above; keep game-specific jobs.

## Left in each launcher

Game knowledge and one-off jobs stay with the game: which archive members
to unpack and where (CMR3's thirdparty step, CMR4's
`install-components.sh`, OutRun's `package.sh`, SRR's DXVK step), the pin
and allowlist contents, desktop entries and icons, the play helper and its
tests, CMR3's vkBasalt build and `mirror.yml`, Screamer's DOSBox build,
library bundling, Windows (MSYS2) and source-archive jobs, and OutRun's
`tweaks-update.yml`.
