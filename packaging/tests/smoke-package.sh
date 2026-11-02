#!/bin/sh
# Tests packaging/smoke-package.sh (no network): sh packaging/tests/smoke-package.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd -P)
SMOKE=$HERE/../smoke-package.sh
T=$(mktemp -d "${TMPDIR:-/tmp}/rea-smoke-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }
passes() { if sh "$SMOKE" "$PKG" "$T/allow" >/dev/null 2>"$T/err"; then ok "$1"; else bad "$1: $(cat "$T/err")"; fi; }
fails_with() { # name, expected stderr fragment
  if sh "$SMOKE" "$PKG" "$T/allow" >/dev/null 2>"$T/err"; then bad "$1 (passed)"
  elif grep -q "$2" "$T/err"; then ok "$1"
  else bad "$1: wrong error: $(cat "$T/err")"; fi
}

cat > "$T/allow" <<'LIST'
# Launcher package allowlist.
Launcher.exe
README.md
linux/game-play x
linux/README.md
docs/images/*.png
licenses/*
NOTES.txt empty
extras/optional.dll optional
LIST
PKG=$T/pkg/Launcher-v1
mkdir -p "$PKG/linux" "$PKG/docs/images" "$PKG/licenses/DXVK"
for f in Launcher.exe README.md linux/game-play linux/README.md docs/images/a.png \
  docs/images/b.png licenses/DXVK/LICENSE; do echo fixture > "$PKG/$f"; done
: > "$PKG/NOTES.txt"
chmod +x "$PKG/linux/game-play"
passes "complete allowlisted package passes"

mkdir -p "$PKG/Game"; echo asset > "$PKG/Game/track.dat"
fails_with "unlisted game data fails" "Game/track.dat"
rm -r "$PKG/Game"
echo x > "$PKG/game.exe"
fails_with "unlisted executable fails" "game.exe"
rm "$PKG/game.exe"
ln -s /etc/passwd "$PKG/docs/link"
fails_with "unlisted symlink fails" "docs/link"
rm "$PKG/docs/link"
mv "$PKG/README.md" "$PKG/README.bak"; ln -s README.bak "$PKG/README.md"
fails_with "a listed path that is a link fails" "not a regular file: README.md"
rm "$PKG/README.md"; mv "$PKG/README.bak" "$PKG/README.md"
echo x > "$PKG/docs/images/c.jpg"
fails_with "a pattern only allows what it matches" "docs/images/c.jpg"
rm "$PKG/docs/images/c.jpg"
rm "$PKG/Launcher.exe"
fails_with "missing required file fails" "missing: Launcher.exe"
: > "$PKG/Launcher.exe"
fails_with "empty required file fails" "empty: Launcher.exe"
echo fixture > "$PKG/Launcher.exe"
chmod -x "$PKG/linux/game-play"
fails_with "non-executable helper fails" "not executable: linux/game-play"
chmod +x "$PKG/linux/game-play"
mv "$PKG/docs/images" "$PKG/docs/imgs"
fails_with "pattern matching nothing fails" "nothing matches docs/images"
mv "$PKG/docs/imgs" "$PKG/docs/images"
mkdir -p "$PKG/extras"; echo x > "$PKG/extras/optional.dll"
passes "optional file allowed when present"
rm -r "$PKG/extras"

(cd "$T/pkg" && zip -qr "$T/Launcher-v1.zip" Launcher-v1) 2>/dev/null
if [ -f "$T/Launcher-v1.zip" ]; then
  PKG=$T/Launcher-v1.zip; passes "a zip with one top folder is checked"
  echo x > "$T/stray"; (cd "$T" && zip -q "$T/Launcher-v1.zip" stray)
  fails_with "a zip with a stray top-level file fails" "one top-level folder"
  PKG=$T/pkg/Launcher-v1
else
  echo "skip zip tests (no zip)"
fi

echo "../escape" > "$T/allow"
fails_with "allowlist escaping the package fails" "escapes"
echo "README.md exe" > "$T/allow"
fails_with "unknown flag fails" "unknown allowlist flag"

[ "$fails" = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
