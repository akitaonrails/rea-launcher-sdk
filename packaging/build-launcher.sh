#!/bin/sh
# rea-launcher-sdk packaging/build-launcher.sh: builds a native launcher's
# packages for Linux, or for x86_64 Windows with the SDK's pinned SDL2
# MinGW package (rea-launcher-native's `sdl` feature links SDL2).
#
#   build-launcher.sh [--root DIR] linux PACKAGE...    target/release/PACKAGE
#   build-launcher.sh [--root DIR] windows PACKAGE...  target/x86_64-pc-windows-gnu/release/PACKAGE.exe
#                                                      (+ dist/sdl2/SDL2.dll)
#   build-launcher.sh [--root DIR] sdl2                only fetch SDL2 into dist/sdl2
#                                                      (for the Windows-target tests)
#
# DIR is the launcher's workspace (default: the current folder); target/
# is $CARGO_TARGET_DIR when set. SDL2 comes through pins.sh (checked by
# SHA-256, `.part` downloads, mirrors, the pin cache, REA_NO_FETCH) from
# sdl2.pins beside this script, or $SDL2_PINS. The Windows build needs
# MinGW-w64 and LLD; lld is passed here because CARGO_ENCODED_RUSTFLAGS
# (one flag per field, so a folder with spaces stays one argument)
# replaces .cargo/config.toml's rustflags. Prints each built file's path.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd -P)
. "$HERE/pins.sh"
die() { echo "build-launcher: $*" >&2; exit 1; }
usage() { die "usage: build-launcher.sh [--root DIR] linux|windows|sdl2 [PACKAGE...]"; }

root=$(pwd -P)
if [ "${1:-}" = --root ]; then
  [ $# -ge 2 ] || usage
  root=$(cd "$2" && pwd -P) || die "no folder $2"
  shift 2
fi
[ $# -ge 1 ] || usage
what=$1; shift
target_dir=${CARGO_TARGET_DIR:-$root/target}
TRIPLE=x86_64-pc-windows-gnu

# Fetches and unpacks the pinned SDL2 MinGW package into dist/sdl2; prints
# its x86_64 folder (bin/SDL2.dll, lib/).
fetch_sdl2() {
  dir=$root/dist/sdl2
  archive=$(PIN_FILE=${SDL2_PINS:-$HERE/sdl2.pins} PIN_DIR='' pin_fetch sdl2-mingw "$dir") \
    || die "SDL2 could not be fetched (see above)"
  # SDL2-devel-2.32.10-mingw.tar.gz unpacks to SDL2-2.32.10/
  version=$(basename "$archive" | sed -n 's/^SDL2-devel-\(.*\)-mingw\.tar\.gz$/\1/p')
  [ -n "$version" ] || die "unexpected SDL2 package name $(basename "$archive")"
  pkg=$dir/SDL2-$version/x86_64-w64-mingw32
  [ -f "$pkg/bin/SDL2.dll" ] || tar xzf "$archive" -C "$dir" || die "unpacking $archive"
  [ -f "$pkg/bin/SDL2.dll" ] || die "no x86_64 SDL2.dll in $archive"
  cp "$pkg/bin/SDL2.dll" "$dir/SDL2.dll"
  printf '%s\n' "$pkg"
}

pkgs=''
for p; do
  case $p in '' | -* | *[!A-Za-z0-9_-]*) die "bad package name '$p'" ;; esac
  pkgs="$pkgs -p $p"
done

case $what in
sdl2)
  [ $# -eq 0 ] || usage
  fetch_sdl2 >/dev/null
  echo "build-launcher: $root/dist/sdl2/SDL2.dll"
  ;;
linux)
  [ $# -ge 1 ] || usage
  # shellcheck disable=SC2086 # package names checked above
  cargo build --release --locked $pkgs --manifest-path "$root/Cargo.toml"
  for p; do echo "build-launcher: $target_dir/release/$p"; done
  ;;
windows)
  [ $# -ge 1 ] || usage
  command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1 || die "x86_64-w64-mingw32-gcc not found (MinGW-w64)"
  sdl=$(fetch_sdl2)/lib
  # with --target, these reach only the Windows build, not build scripts
  CARGO_ENCODED_RUSTFLAGS=$(printf '%s\037' -C link-arg=-fuse-ld=lld -L "$sdl")
  CARGO_ENCODED_RUSTFLAGS=${CARGO_ENCODED_RUSTFLAGS%?}
  export CARGO_ENCODED_RUSTFLAGS
  # shellcheck disable=SC2086 # package names checked above
  cargo build --release --locked $pkgs --target "$TRIPLE" --manifest-path "$root/Cargo.toml"
  for p; do echo "build-launcher: $target_dir/$TRIPLE/release/$p.exe"; done
  ;;
*)
  die "unknown target '$what' (linux, windows or sdl2)"
  ;;
esac
