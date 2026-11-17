#!/bin/sh
# Tests packaging/build-launcher.sh with a fake cargo and MinGW and a
# file:// SDL2 package (no network): sh packaging/tests/build-launcher.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd -P)
BUILD=$HERE/../build-launcher.sh
T=$(mktemp -d "${TMPDIR:-/tmp}/rea-build-launcher-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }
has() { if grep -qF -- "$2" "$3"; then ok "$1"; else bad "$1: no '$2' in $(cat "$3")"; fi; }

# The fake tools: cargo writes down its arguments and its rustflags.
mkdir -p "$T/bin" "$T/up/SDL2-2.32.10/x86_64-w64-mingw32/bin" "$T/up/SDL2-2.32.10/x86_64-w64-mingw32/lib" "$T/repo"
cat > "$T/bin/cargo" <<SH
#!/bin/sh
echo "\$*" > "$T/cargo.args"
echo "\${CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS:-}" > "$T/cargo.flags"
SH
printf '#!/bin/sh\n' > "$T/bin/x86_64-w64-mingw32-gcc"
chmod +x "$T/bin/cargo" "$T/bin/x86_64-w64-mingw32-gcc"
echo dll > "$T/up/SDL2-2.32.10/x86_64-w64-mingw32/bin/SDL2.dll"
tar czf "$T/up/SDL2-devel-2.32.10-mingw.tar.gz" -C "$T/up" SDL2-2.32.10
sum=$(sha256sum "$T/up/SDL2-devel-2.32.10-mingw.tar.gz" | cut -d' ' -f1)
echo "sdl2-mingw file://$T/up/SDL2-devel-2.32.10-mingw.tar.gz $sum" > "$T/sdl2.pins"
export PATH="$T/bin:$PATH" SDL2_PINS="$T/sdl2.pins" REA_NO_FETCH=1 PIN_CACHE=''
unset CARGO_TARGET_DIR CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS

out=$(sh "$BUILD" --root "$T/repo" linux initiald-launcher 2>&1) || bad "linux build: $out"
has "linux: release build of the package" "build --release --locked -p initiald-launcher --manifest-path $T/repo/Cargo.toml" "$T/cargo.args"
[ "$out" = "build-launcher: $T/repo/target/release/initiald-launcher" ] && ok "linux: prints the binary" || bad "linux: prints the binary: $out"

out=$(cd "$T/repo" && CARGO_TARGET_DIR=$T/tgt sh "$BUILD" windows wangan-launcher wangan-io 2>&1) || bad "windows build: $out"
has "windows: both packages for x86_64" "-p wangan-launcher -p wangan-io --target x86_64-pc-windows-gnu" "$T/cargo.args"
has "windows: lld and SDL2's import library" "-C link-arg=-fuse-ld=lld -L $T/repo/dist/sdl2/SDL2-2.32.10/x86_64-w64-mingw32/lib" "$T/cargo.flags"
[ "$(cat "$T/repo/dist/sdl2/SDL2.dll" 2>/dev/null)" = dll ] && ok "windows: SDL2.dll in dist/sdl2" || bad "windows: SDL2.dll in dist/sdl2"
case $out in *"build-launcher: $T/tgt/x86_64-pc-windows-gnu/release/wangan-io.exe"*) ok "windows: prints each exe under CARGO_TARGET_DIR" ;; *) bad "windows: prints each exe: $out" ;; esac

# A download cut short last time wedges nothing: only the checked file counts.
rm -rf "$T/repo/dist"
mkdir -p "$T/repo/dist/sdl2"
echo partial > "$T/repo/dist/sdl2/SDL2-devel-2.32.10-mingw.tar.gz"
sh "$BUILD" --root "$T/repo" sdl2 >/dev/null 2>&1 && [ -f "$T/repo/dist/sdl2/SDL2.dll" ] \
  && ok "a partial download is fetched again" || bad "a partial download is fetched again"

echo "sdl2-mingw file://$T/up/SDL2-devel-2.32.10-mingw.tar.gz 0000000000000000000000000000000000000000000000000000000000000000" > "$T/bad.pins"
rm -rf "$T/repo/dist"
if SDL2_PINS=$T/bad.pins sh "$BUILD" --root "$T/repo" sdl2 >/dev/null 2>&1; then bad "a wrong checksum fails"; else ok "a wrong checksum fails"; fi
[ ! -e "$T/repo/dist/sdl2/SDL2.dll" ] && ok "nothing unchecked is unpacked" || bad "nothing unchecked is unpacked"

if sh "$BUILD" --root "$T/repo" linux 'x;rm' >/dev/null 2>&1; then bad "an odd package name fails"; else ok "an odd package name fails"; fi
if sh "$BUILD" --root "$T/repo" linux >/dev/null 2>&1; then bad "no package fails"; else ok "no package fails"; fi
if sh "$BUILD" --root "$T/repo" mac x >/dev/null 2>&1; then bad "an unknown target fails"; else ok "an unknown target fails"; fi
# a PATH with only what the script runs before it looks for MinGW
mkdir "$T/min" && ln -s "$(command -v dirname)" "$T/min/dirname"
SH=$(command -v sh)
if PATH=$T/min "$SH" "$BUILD" --root "$T/repo" windows x >"$T/out" 2>&1; then bad "windows without MinGW fails"; else has "windows without MinGW fails" "MinGW" "$T/out"; fi

# The SDK's own pin is well formed (what launchers fetch).
(. "$HERE/../pins.sh"; PIN_FILE=$HERE/../sdl2.pins PIN_DIR='' pin_file sdl2-mingw) | grep -qx 'SDL2-devel-2.32.10-mingw.tar.gz' \
  && ok "the SDK's SDL2 pin" || bad "the SDK's SDL2 pin"

[ "$fails" = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
