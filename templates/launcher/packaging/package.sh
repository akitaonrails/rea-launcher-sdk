#!/bin/sh
# packaging/package.sh [--no-appimage] <package-folder>
# Builds the zip folder (e.g. dist/@LAUNCHER_NAME@-v0.1.0/), its .zip beside it,
# and the AppImage (portable Wine, sdk/packaging/appimage-wine.sh) in the
# same parent folder. CI calls it from launcher-ci.yml's package-command
# (rea-launcher-sdk docs/playbooks/ci-and-releases.md).
#
# Environment: REA_RELEASE_DIR holds @LAUNCHER_EXE@ (CI sets it; default
# target/i686-pc-windows-gnu/release); REA_CACHE caches pinned downloads.
# Everything the folder holds must be listed in packaging/package.allow.
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd -P)
appimage=1
if [ "${1:-}" = --no-appimage ]; then appimage='' && shift; fi
[ $# -eq 1 ] || { echo "usage: $0 [--no-appimage] <package-folder>" >&2; exit 2; }
pkg=$1
name=$(basename "$pkg")
case $name in '' | *[!A-Za-z0-9._-]*) echo "package: bad package name '$name'" >&2; exit 2 ;; esac
dist=$(dirname "$pkg")
release=${REA_RELEASE_DIR:-$ROOT/target/i686-pc-windows-gnu/release}
[ -f "$release/@LAUNCHER_EXE@" ] || { echo "package: no $release/@LAUNCHER_EXE@ (cargo build --release --target i686-pc-windows-gnu)" >&2; exit 1; }

rm -rf "$pkg"
mkdir -p "$pkg/linux" "$pkg/licenses"
cp "$release/@LAUNCHER_EXE@" "$pkg/"
cp "$ROOT/README.md" "$ROOT/THIRD_PARTY.md" "$ROOT/CHANGELOG.md" \
  "$ROOT/LICENSE-MIT" "$ROOT/LICENSE-APACHE" "$pkg/"
cp "$ROOT/linux/@ID@-play" "$ROOT/linux/README.md" "$pkg/linux/"
# The helper finds the SDK play library beside itself.
cp "$ROOT/sdk/rea-play.sh" "$pkg/linux/rea-play.sh"
chmod +x "$pkg/linux/@ID@-play"
cp "$ROOT"/packaging/licenses/* "$pkg/licenses/"

# Third-party components: fetch each from packaging/third-party.txt
# (checked by SHA-256) and unpack what the game needs, e.g.
#   . "$ROOT/sdk/packaging/pins.sh"
#   PIN_FILE=$ROOT/packaging/third-party.txt
#   zip=$(pin_fetch dxvk "$work")
# and list every file in package.allow and THIRD_PARTY.md.

(cd "$dist" && rm -f "$name.zip" && zip -qr "$name.zip" "$name")

if [ -n "$appimage" ]; then
  sh "$ROOT/sdk/packaging/appimage-wine.sh" --app @ID@ --exe @LAUNCHER_EXE@ \
    --env-prefix @PREFIX@ \
    --icon "$ROOT/packaging/appimage/@ID@-launcher.png" \
    --desktop "$ROOT/packaging/appimage/@ID@-launcher.desktop" \
    --doc licenses \
    ${REA_CACHE:+--cache "$REA_CACHE"} \
    --name "$name" "$pkg" "$dist"
fi
