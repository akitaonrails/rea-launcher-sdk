#!/bin/sh
# linux/@ID@-play end to end with a recording wrapper instead of Wine and
# gamescope, in the repository layout (library in ../sdk) and the package
# layout (library beside the script): sh linux/tests/play.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd -P)
ROOT=$(cd "$HERE/../.." && pwd -P)
T=$(mktemp -d "${TMPDIR:-/tmp}/@ID@-play-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
export HOME=$T
unset XDG_CONFIG_HOME GAMESCOPE_WAYLAND_DISPLAY DRI_PRIME __NV_PRIME_RENDER_OFFLOAD \
  WINEDLLOVERRIDES @PREFIX@_LAUNCHER @PREFIX@_WAIT @PREFIX@_GPU 2>/dev/null
fails=0
ok() { echo "ok   $1"; }
fail() { echo "FAIL $1"; fails=$((fails + 1)); }
check() { if [ "$2" = "$3" ]; then ok "$1"; else fail "$1: got '$2', want '$3'"; fi; }
contains() { case "$2" in *"$3"*) ok "$1" ;; *) fail "$1: '$2' lacks '$3'" ;; esac; }

# The wrapper: gamescope is "installed"; the settings window exits with
# $T/rc; everything else is recorded in $T/ran.
cat > "$T/fake-run" <<'FAKE'
#!/bin/sh
case "$*" in
  *"command -v gamescope"*) exit 0 ;;
  *"wineserver"*) exit 0 ;;
  *"--settings"*) echo "$*" > "$T/settings"; exit "$(cat "$T/rc" 2>/dev/null || echo 0)" ;;
esac
echo "$*" > "$T/ran"
FAKE
chmod +x "$T/fake-run"
export T @PREFIX@_RUN=$T/fake-run @PREFIX@_PREFIX=$T/prefix @PREFIX@_GPU=

mkdir -p "$T/repo/linux" "$T/repo/sdk" "$T/pkg/linux"
cp "$ROOT/linux/@ID@-play" "$T/repo/linux/"
cp "$ROOT/sdk/rea-play.sh" "$T/repo/sdk/"
cp "$ROOT/linux/@ID@-play" "$ROOT/sdk/rea-play.sh" "$T/pkg/linux/"
: > "$T/repo/@LAUNCHER_EXE@"
: > "$T/pkg/@LAUNCHER_EXE@"

for layout in repo pkg; do
  play=$T/$layout/linux/@ID@-play
  check "[$layout] status" "$(sh "$play" status)" idle
  rm -f "$T/ran" "$T/settings"
  echo 0 > "$T/rc"
  sh "$play" > "$T/out" 2>&1; rc=$?
  check "[$layout] window, then the game: exit 0" "$rc" 0
  contains "[$layout] the window opens through the helper" "$(cat "$T/settings" 2>/dev/null)" "@LAUNCHER_EXE@ --settings --linux"
  contains "[$layout] the game runs in gamescope" "$(cat "$T/ran" 2>/dev/null)" "gamescope -W 1920 -H 1080"
  contains "[$layout] without the window" "$(cat "$T/ran" 2>/dev/null)" "@LAUNCHER_EXE@ --no-window"
  rm -f "$T/ran"
  echo 3 > "$T/rc"
  sh "$play" > "$T/out" 2>&1; rc=$?
  check "[$layout] window closed without Play: nothing starts" "$(cat "$T/ran" 2>/dev/null)" ""
  check "[$layout] stop" "$(sh "$play" stop)" stopped
done

echo "# tests source the script: REA_FUNCTIONS_ONLY=1 + REA_PLAY_HERE"
got=$(REA_FUNCTIONS_ONLY=1 REA_PLAY_HERE=$T/repo/linux sh -c '
  . "$1"
  echo "app=$REA_APP exe=$REA_EXE cfg_appdata=$REA_CFG_APPDATA"
  echo "dropped=${REA_FUNCTIONS_ONLY-unset}/${REA_PLAY_HERE-unset}"' sh "$T/repo/linux/@ID@-play" 2>&1)
contains "sourced: configured" "$got" "app=@ID@-launcher exe=$T/repo/@LAUNCHER_EXE@ cfg_appdata=@LAUNCHER_NAME@"
contains "sourced: both variables dropped once loaded" "$got" "dropped=unset/unset"

rm -f "$T/repo/@LAUNCHER_EXE@"
out=$(sh "$T/repo/linux/@ID@-play" --play 2>&1); rc=$?
check "no launcher: exit 1" "$rc" 1
contains "no launcher: names the variable" "$out" "@PREFIX@_LAUNCHER"

if [ "$fails" = 0 ]; then echo "all passed"; exit 0; fi
echo "$fails failed"; exit 1
