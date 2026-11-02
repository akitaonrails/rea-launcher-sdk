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

Tests: `sh packaging/tests/<name>.sh` and `sh scripts/tests/sdk-sync.sh`
(no network: downloads come from `file://` urls).

## Vendoring into a launcher

From an SDK checkout at the tag you want:

```sh
scripts/sdk-sync ../rea-colin-mcrae-4          # writes ../rea-colin-mcrae-4/sdk/
scripts/sdk-sync --check ../rea-colin-mcrae-4  # exit 1 on drift
```

Layout in the launcher: `sdk/rea-play.sh` (the play library, from the SDK's
`shell/`), `sdk/packaging/...` (as here). Helpers look for `rea-play.sh`
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

Fetched files keep the primary url's file name whichever mirror served them.
Only `https://` and `file://` are allowed (redirects: `https` only).

## AppImage

```sh
sdk/packaging/appimage-wine.sh --app cmr4 --exe CMR4Launcher.exe \
  --icon packaging/appimage/cmr4-launcher.png \
  --desktop packaging/appimage/cmr4-launcher.desktop \
  --payload components --doc licenses --doc docs \
  --name "CMR4Launcher-$VERSION" dist/CMR4Launcher-$VERSION dist
```

Layout: the launcher, `linux/<app>-play` with the SDK play library beside
it (`usr/lib/<app>/rea-play.sh`, taken from the vendored `sdk/rea-play.sh`
or `--play-lib FILE`), and every `--payload` path go to `usr/lib/<app>/` at the same
relative paths; README, THIRD_PARTY, both licences, CHANGELOG,
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

Inputs of `launcher-ci.yml` (all optional): `runs-on`, `rust-toolchain`
(stable), `rust-target` (i686-pc-windows-gnu; empty for a native build),
`apt-packages`, `submodules`, `shell-syntax` and `shell-tests` (globs),
`audit` (cargo-audit 0.22.1 + cargo-deny 0.19.8), `sdk-check`, `sdk-dir`,
`imports-test`, `package-command`, `package-dir`, `package-allowlist`,
`artifact-name`, `artifact-paths`. Commands run in bash with
`REA_VERSION_NAME`, `REA_RELEASE_DIR`, `REA_CACHE` and `REA_SDK` set. SRR's
import check, for example:
`imports-test: SRR_LAUNCHER_EXE="$REA_RELEASE_DIR/SRRLauncher.exe" cargo test --locked --test imports -- --ignored`.

`launcher-release.yml` takes `package` (the Cargo package the tag must
match), `artifact-pattern`, and optionally `changelog`, `notes`
(`changelog` or `generate`), `attest` and `sdk-dir`. Any extra job that
uploads release files (Screamer's Windows and source archives) only needs
an artifact name matching the pattern and a place in `needs:`.

A scheduled pin watch (as CMR3's):

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

## Left in each launcher

Game knowledge and one-off jobs stay with the game: which archive members
to unpack and where (CMR3's thirdparty step, CMR4's
`install-components.sh`, OutRun's `package.sh`, SRR's DXVK step), the pin
and allowlist contents, desktop entries and icons, the play helper and its
tests, CMR3's vkBasalt build and `mirror.yml`, Screamer's DOSBox build,
library bundling, Windows (MSYS2) and source-archive jobs, and OutRun's
`tweaks-update.yml`.
