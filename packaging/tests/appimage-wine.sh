#!/bin/sh
# Tests packaging/appimage-wine.sh with a fake Wine archive and a fake
# appimagetool served over file:// (no network, no FUSE):
#   sh packaging/tests/appimage-wine.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd -P)
BUILD=$HERE/../appimage-wine.sh
T=$(mktemp -d "${TMPDIR:-/tmp}/rea-appimage-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }
check() { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1: want '$2', got '$3'"; fi; }
has() { if [ -e "$2" ]; then ok "$1"; else bad "$1: no $2"; fi; }
lacks() { if [ -e "$2" ]; then bad "$1: $2 exists"; else ok "$1"; fi; }
sha() { sha256sum "$1" | cut -d' ' -f1; }

# Fake downloads: a Wine tree with dev tools, appimagetool that copies the
# AppDir next to its output, and a runtime blob.
W=$T/src/wine-11.20-amd64-wow64
mkdir -p "$W/bin" "$W/include" "$W/share/man/man1" "$W/lib/wine"
for t in wine wineserver winegcc widl winebuild wrc; do printf '#!/bin/sh\n' > "$W/bin/$t"; chmod +x "$W/bin/$t"; done
echo h > "$W/include/windows.h"; echo m > "$W/share/man/man1/wine.1"; echo d > "$W/lib/wine/kernel32.dll"
(cd "$T/src" && tar -cJf "$T/src/wine-11.20-amd64-wow64.tar.xz" wine-11.20-amd64-wow64)
cat > "$T/src/appimagetool" <<'EOF'
#!/bin/sh
# Fake appimagetool: records its arguments, keeps the AppDir for inspection.
printf '%s\n' "$@" > "$T_LOG"
for a; do prev2=$prev; prev=$a; done
[ "$ARCH" = x86_64 ] || exit 3
cp -R "$prev2" "$prev.d" && echo appimage > "$prev"
EOF
echo runtime > "$T/src/runtime-x86_64"
cat > "$T/tools.pins" <<EOF
wine file://$T/src/wine-11.20-amd64-wow64.tar.xz $(sha "$T/src/wine-11.20-amd64-wow64.tar.xz")
appimagetool file://$T/src/appimagetool $(sha "$T/src/appimagetool")
appimage-runtime file://$T/src/runtime-x86_64 $(sha "$T/src/runtime-x86_64")
EOF
# Another Wine (its archive's folder says 11.21), and the 11.20 archive
# under a name without a version (an offline file:// copy named by its
# checksum): the archive decides.
mkdir -p "$T/src/o" && cp -R "$W" "$T/src/o/wine-11.21-amd64-wow64"
(cd "$T/src/o" && tar -cJf "$T/src/wine-11.21-amd64-wow64.tar.xz" wine-11.21-amd64-wow64)
sed "s|^wine .*|wine file://$T/src/wine-11.21-amd64-wow64.tar.xz $(sha "$T/src/wine-11.21-amd64-wow64.tar.xz")|" \
  "$T/tools.pins" > "$T/other-wine.pins"
WSHA=$(sha "$T/src/wine-11.20-amd64-wow64.tar.xz")
cp "$T/src/wine-11.20-amd64-wow64.tar.xz" "$T/src/$WSHA"
sed "s|^wine .*|wine file://$T/src/$WSHA $WSHA|" "$T/tools.pins" > "$T/sha-named.pins"
export T_LOG="$T/tool.log" REA_CACHE="$T/cache"

# A Wine launcher's unpacked package (CMR3-like: thirdparty/ payload).
P=$T/pkg
mkdir -p "$P/linux" "$P/thirdparty/dxvk" "$P/docs"
for f in GameLauncher.exe README.md THIRD_PARTY.md LICENSE-MIT LICENSE-APACHE CHANGELOG.md \
  linux/README.md thirdparty/dxvk/d3d9.dll docs/GUIDE.md; do echo "$f" > "$P/$f"; done
cat > "$P/linux/game-play" <<'EOF'
#!/bin/sh
echo "launcher=$GAME_LAUNCHER wait=$GAME_WAIT tp=${GAME_THIRDPARTY:-} dll=$WINEDLLOVERRIDES debug=$WINEDEBUG args=$*"
echo "path=$PATH"
EOF
# A decoy in the package: the library must come from the vendored SDK.
echo '# stale package copy' > "$P/linux/rea-play.sh"
# The builder as a launcher runs it: vendored under sdk/, library at sdk/rea-play.sh.
V=$T/repo/sdk
mkdir -p "$V"
cp -R "$HERE/.." "$V/packaging"
echo '# vendored play library' > "$V/rea-play.sh"
# Without any library, and as an SDK checkout (library in shell/).
BARE=$T/bare
mkdir -p "$BARE"; cp -R "$HERE/.." "$BARE/packaging"
CO=$T/checkout
mkdir -p "$CO/shell"; cp -R "$HERE/.." "$CO/packaging"
echo '# checkout play library' > "$CO/shell/rea-play.sh"
printf 'png' > "$T/game-launcher.png"

run() { sh "$BUILD" "$@" >"$T/out" 2>&1; }
BUILD=$V/packaging/appimage-wine.sh
if run --app game --exe GameLauncher.exe --pins "$T/tools.pins" --icon "$T/game-launcher.png" \
  --title "A Game" --comment "Play it" --payload thirdparty --payload linux --doc docs \
  --env-path GAME_THIRDPARTY=thirdparty "$P" "$T/out1"; then
  ok "wine build succeeds"
else
  bad "wine build succeeds: $(cat "$T/out")"
fi
BUILD=$HERE/../appimage-wine.sh
A=$T/out1/GameLauncher-x86_64.AppImage.d
has "AppImage written with the exe stem" "$T/out1/GameLauncher-x86_64.AppImage"
grep -qx -- --runtime-file "$T/tool.log" && grep -q '/runtime-x86_64$' "$T/tool.log" \
  && ok "pinned runtime passed to appimagetool" || bad "pinned runtime passed to appimagetool"
has "Wine bin" "$A/usr/lib/wine/bin/wine"
lacks "winegcc stripped" "$A/usr/lib/wine/bin/winegcc"
lacks "widl stripped" "$A/usr/lib/wine/bin/widl"
lacks "Wine headers stripped" "$A/usr/lib/wine/include"
lacks "Wine man pages stripped" "$A/usr/lib/wine/share/man"
for f in COPYING.LIB LICENSE SOURCE.txt; do
  if cmp -s "$HERE/../appimage/wine/$f" "$A/usr/share/doc/wine/$f"; then ok "Wine $f shipped"; else bad "Wine $f shipped"; fi
done
for f in README.md THIRD_PARTY.md LICENSE-MIT LICENSE-APACHE CHANGELOG.md linux/README.md docs/GUIDE.md; do
  has "doc $f" "$A/usr/share/doc/game/$f"
done
has "exe" "$A/usr/lib/game/GameLauncher.exe"
has "payload" "$A/usr/lib/game/thirdparty/dxvk/d3d9.dll"
has "helper" "$A/usr/lib/game/game-play"
check "vendored sdk/rea-play.sh beside the helper" "# vendored play library" "$(cat "$A/usr/lib/game/rea-play.sh" 2>/dev/null)"
lacks "no duplicate helper in linux/" "$A/usr/lib/game/linux/game-play"
grep -qx 'Icon=game-launcher' "$A/game-launcher.desktop" && grep -qx 'Name=A Game' "$A/game-launcher.desktop" \
  && ok "desktop entry generated" || bad "desktop entry generated"
has "icon" "$A/game-launcher.png"
if grep -q '@[A-Z]*@' "$A/AppRun"; then bad "AppRun fully rendered"; else ok "AppRun fully rendered"; fi
if grep -q LD_LIBRARY_PATH "$A/AppRun"; then bad "no native lines in Wine AppRun"; else ok "no native lines in Wine AppRun"; fi
o=$(unset WINEDLLOVERRIDES WINEDEBUG; sh "$A/AppRun" --play 2>&1 | sed -n 1p)
check "AppRun environment and args" "launcher=$A/usr/lib/game/GameLauncher.exe wait=1 tp=$A/usr/lib/game/thirdparty dll=mscoree,mshtml= debug=-all args=--play" "$o"
o=$(WINEDLLOVERRIDES='d3d9=n' WINEDEBUG=+seh sh "$A/AppRun" 2>&1 | sed -n 1p)
case $o in *"dll=mscoree,mshtml=;d3d9=n debug=+seh "*) ok "player overrides kept" ;; *) bad "player overrides kept: $o" ;; esac
o=$(sh "$A/AppRun" 2>&1 | sed -n 2p)
case $o in "path=$A/usr/lib/wine/bin:"*) ok "bundled Wine first on PATH" ;; *) bad "bundled Wine first on PATH: $o" ;; esac

# Native mode (Screamer-like): no Wine, lib/ on LD_LIBRARY_PATH.
N=$T/native
mkdir -p "$N/linux" "$N/lib" "$N/resources"
for f in NativeLauncher dosbox README.md THIRD_PARTY.md LICENSE-MIT LICENSE-APACHE lib/libSDL2.so resources/a.txt; do echo "$f" > "$N/$f"; done
printf '#!/bin/sh\necho "l=$SCR_LAUNCHER db=$SCR_DOSBOX ld=$LD_LIBRARY_PATH"\n' > "$N/linux/native-play"
printf '[Desktop Entry]\nType=Application\nName=Native\nExec=AppRun\nIcon=game-launcher\nCategories=Game;\n' > "$T/native.desktop"
BUILD=$BARE/packaging/appimage-wine.sh
if run --native --app native --exe NativeLauncher --env-prefix SCR --pins "$T/tools.pins" \
  --icon "$T/game-launcher.png" --desktop "$T/native.desktop" --payload dosbox --payload lib \
  --optional-payload resources --optional-payload not-there --env-path SCR_DOSBOX=dosbox "$N" "$T/out2"; then
  ok "native build succeeds"
else
  bad "native build succeeds: $(cat "$T/out")"
fi
B=$T/out2/NativeLauncher-x86_64.AppImage.d
lacks "no play library when none exists and the helper needs none" "$B/usr/lib/native/rea-play.sh"
lacks "no Wine in native mode" "$B/usr/lib/wine"
lacks "no Wine docs in native mode" "$B/usr/share/doc/wine"
[ -x "$B/usr/lib/native/NativeLauncher" ] && ok "native exe executable" || bad "native exe executable"
has "optional payload present" "$B/usr/lib/native/resources/a.txt"
lacks "optional payload missing is skipped" "$B/usr/lib/native/not-there"
o=$(unset LD_LIBRARY_PATH; sh "$B/AppRun" 2>&1)
check "native AppRun" "l=$B/usr/lib/native/NativeLauncher db=$B/usr/lib/native/dosbox ld=$B/usr/lib/native/lib" "$o"
if grep -q 'WINE' "$B/AppRun"; then bad "no Wine lines in native AppRun"; else ok "no Wine lines in native AppRun"; fi
check "desktop copied as given" "$(cat "$T/native.desktop")" "$(cat "$B/game-launcher.desktop")"

# A helper that sources rea-play.sh never ships without it.
cp "$N/linux/native-play" "$T/native-play.orig"
printf '#!/bin/sh\n. "$(dirname "$0")/rea-play.sh"\n' > "$N/linux/native-play"
if run --native --app native --exe NativeLauncher --env-prefix SCR --pins "$T/tools.pins" \
  --icon "$T/game-launcher.png" --title N "$N" "$T/out4"; then
  bad "helper needing rea-play.sh without a library fails (built)"
elif grep -q 'sources rea-play.sh but none was found' "$T/out"; then
  ok "helper needing rea-play.sh without a library fails"
else
  bad "helper needing rea-play.sh without a library fails: $(cat "$T/out")"
fi
# An SDK checkout supplies shell/rea-play.sh.
BUILD=$CO/packaging/appimage-wine.sh
if run --native --app native --exe NativeLauncher --env-prefix SCR --pins "$T/tools.pins" \
  --icon "$T/game-launcher.png" --title N "$N" "$T/out5"; then
  check "SDK checkout's shell/rea-play.sh beside the helper" "# checkout play library" \
    "$(cat "$T/out5/NativeLauncher-x86_64.AppImage.d/usr/lib/native/rea-play.sh" 2>/dev/null)"
else
  bad "SDK checkout build: $(cat "$T/out")"
fi
cp "$T/native-play.orig" "$N/linux/native-play"
BUILD=$HERE/../appimage-wine.sh

# An explicit --play-lib wins.
echo '# explicit' > "$T/explicit.sh"
if run --native --app native --exe NativeLauncher --env-prefix SCR --pins "$T/tools.pins" \
  --icon "$T/game-launcher.png" --title N --play-lib "$T/explicit.sh" "$N" "$T/out3"; then
  check "--play-lib copied beside the helper" "# explicit" "$(cat "$T/out3/NativeLauncher-x86_64.AppImage.d/usr/lib/native/rea-play.sh" 2>/dev/null)"
else
  bad "--play-lib build: $(cat "$T/out")"
fi

# Argument checks.
refuse() { # name, expected fragment, args...
  n=$1; e=$2; shift 2
  if run "$@"; then bad "$n (built)"
  elif grep -q -- "$e" "$T/out"; then ok "$n"
  else bad "$n: $(cat "$T/out")"; fi
}
COMMON="--app game --exe GameLauncher.exe --pins $T/tools.pins --icon $T/game-launcher.png --title Game"
# shellcheck disable=SC2086
{
refuse "missing package file" "package lacks" $COMMON --payload nope "$P" "$T/o"
refuse "path escape rejected" "unsafe path" $COMMON --payload ../etc "$P" "$T/o"
refuse "absolute path rejected" "unsafe path" $COMMON --doc /etc/passwd "$P" "$T/o"
refuse "bad app id" "--app must be" --app 'Bad App' --exe GameLauncher.exe --icon "$T/game-launcher.png" --title G "$P" "$T/o"
refuse "bad env prefix" "--env-prefix" $COMMON --env-prefix 'x;rm' "$P" "$T/o"
refuse "bad env-path variable" "bad --env-path" $COMMON --env-path 'a$b=thirdparty' "$P" "$T/o"
refuse "env-path must exist in the AppImage" "is not in the AppImage" $COMMON --env-path GAME_X=nothing "$P" "$T/o"
refuse "desktop or title required" "--desktop FILE or --title" --app game --exe GameLauncher.exe --icon "$T/game-launcher.png" "$P" "$T/o"
refuse "missing --play-lib fails" "no play library" $COMMON --play-lib "$T/nope.sh" "$P" "$T/o"
refuse "unknown option" "unknown option" $COMMON --frobnicate "$P" "$T/o"
refuse "wrong argument count" "appimage-wine.sh" $COMMON "$P"
refuse "payload listed twice" "listed twice" $COMMON --payload thirdparty --payload thirdparty "$P" "$T/o"
refuse "SOURCE.txt must match the Wine pin" "does not describe Wine 11.21" --app game --exe GameLauncher.exe --pins "$T/other-wine.pins" --icon "$T/game-launcher.png" --title G "$P" "$T/o"
}
if run --app game --exe GameLauncher.exe --pins "$T/sha-named.pins" --icon "$T/game-launcher.png" --title G "$P" "$T/o-sha"; then
  ok "a Wine pin named by its checksum builds (the version comes from the archive)"
else bad "checksum-named Wine pin: $(cat "$T/out")"; fi
rm -rf "$T/o-sha"
# Offline: everything from file:// pins, never the network.
if REA_NO_FETCH=1 run --app game --exe GameLauncher.exe --pins "$T/tools.pins" --icon "$T/game-launcher.png" --title G "$P" "$T/o-off"; then
  ok "offline (REA_NO_FETCH=1) with file:// pins builds"
else bad "offline build: $(cat "$T/out")"; fi
rm -rf "$T/o-off"
printf 'png' > "$T/other.png"
refuse "desktop Icon must match the icon" "needs Icon=other" --app game --exe GameLauncher.exe --icon "$T/other.png" --desktop "$T/native.desktop" "$P" "$T/o"
mv "$T/src/appimagetool" "$T/src/appimagetool.off"; rm -rf "$REA_CACHE"
refuse "a missing tool fails the build" "cannot fetch appimagetool" $COMMON "$P" "$T/o"
mv "$T/src/appimagetool.off" "$T/src/appimagetool"
[ -z "$(ls -A "${TMPDIR:-/tmp}" 2>/dev/null | grep '^rea-appimage\.' || true)" ] \
  && ok "work folders cleaned up" || echo "note: rea-appimage.* folders exist in ${TMPDIR:-/tmp} (maybe another run)"

[ "$fails" = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
