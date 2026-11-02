#!/bin/sh
# shell/example-play end to end with a recording wrapper, in both layouts:
# the library next to the script (zip/AppImage) and in ../sdk (a launcher
# repo): sh shell/tests/example.sh
. "$(dirname "$(readlink -f "$0")")/common.sh"
make_fake_run
export EXR_RUN=$T/fake-run EXR_PREFIX=$T/prefix REA_SYSFS=$T/nosys
mkdir -p "$T/pkg" "$T/repo/linux" "$T/repo/sdk" "$T/bare"
cp "$TESTS/../example-play" "$TESTS/../rea-play.sh" "$T/pkg/"
cp "$TESTS/../example-play" "$T/repo/linux/"; cp "$TESTS/../rea-play.sh" "$T/repo/sdk/"
cp "$TESTS/../example-play" "$T/bare/"
: > "$T/pkg/EXRLauncher.exe"; : > "$T/repo/EXRLauncher.exe"
for layout in pkg repo/linux; do
  play=$T/$layout/example-play
  check "[$layout] status" "$(sh "$play" status)" idle
  contains "[$layout] help lists the game's command" "$(sh "$play" --help)" "--fix-pads"
  rm -f "$T/ran" "$T/wineserver"
  echo 0 > "$T/rc"
  sh "$play" > "$T/out" 2>&1; rc=$?
  check "[$layout] window then game exits 0" "$rc" 0
  contains "[$layout] the game runs in gamescope" "$(cat "$T/ran")" "gamescope -W 1920 -H 1080"
  contains "[$layout] with the game's DLL overrides" "$(cat "$T/ran")" "WINEDLLOVERRIDES=dinput8,d3d9=n,b;winedbg=d"
  contains "[$layout] and its process to wait for" "$(cat "$T/ran")" "REA_GAME_PROCS=ExampleRally.exe"
  contains "[$layout] launcher found" "$(cat "$T/ran")" "EXRLauncher.exe --no-window"
  check "[$layout] stop" "$(sh "$play" stop)" stopped
done
out=$(sh "$T/bare/example-play" status 2>&1); rc=$?
check "no library: exit 1" "$rc" 1
contains "no library: a clear message" "$out" "rea-play.sh not found"
check "a bad command exits 2" "$(sh "$T/pkg/example-play" nope >/dev/null 2>&1; echo $?)" 2
finish
