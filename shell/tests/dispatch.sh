#!/bin/sh
# rea_main: the commands every helper understands, and the hook a game
# script extends them with: sh shell/tests/dispatch.sh
. "$(dirname "$(readlink -f "$0")")/common.sh"
REA_APP=test-launcher REA_PREFIX=$T/prefix REA_SYSFS=$T/nosys
: > "$T/TestLauncher.exe"
REA_EXE=$T/TestLauncher.exe REA_CFG=$T/launcher.cfg REA_EXE_NAME=TestLauncher.exe REA_EXE_HINT=TEST_LAUNCHER
load_lib
make_fake_run
REA_RUN=$T/fake-run REA_GPU=
main() { ( rea_main "$@" ) > "$T/out" 2>&1; echo $?; }

check "status when idle" "$(main status; cat "$T/out")" "0
idle"
check "stop" "$(main stop; cat "$T/out")" "0
stopped"
contains "stop ends the prefix's wineserver" "$(cat "$T/wineserver")" "wineserver -k"
check "--help exits 0" "$(main --help)" 0
contains "--help shows usage" "$(cat "$T/out")" "usage: x-play"
check "an unknown command exits 2" "$(main --bogus)" 2
contains "with usage on stderr" "$(cat "$T/out")" "usage: x-play"

rm -f "$T/settings"
for cmd in --play play start; do
  rm -f "$T/ran"
  main "$cmd" > /dev/null
  contains "$cmd starts the game, no window" "$(cat "$T/ran" 2>/dev/null)" "inner.sh $REA_EXE --no-window"
done
[ -e "$T/settings" ] && fail "--play opened the window" || ok "--play never opens the window"
main --play 2 > /dev/null
contains "--play arguments go to the launcher" "$(cat "$T/ran")" "--no-window 2"

for cmd in "" --settings settings; do
  echo 0 > "$T/rc"; rm -f "$T/ran" "$T/settings"
  main $cmd > /dev/null
  [ -e "$T/settings" ] && [ -e "$T/ran" ] && ok "'$cmd': window, then the game" || fail "'$cmd' flow"
done
echo 3 > "$T/rc"; rm -f "$T/ran"
check "window closed: exit 0, no game" "$(main)" 0
[ -e "$T/ran" ] && fail "closed window started the game" || ok "closed window starts nothing"
echo 4 > "$T/rc"; rm -f "$T/ran"
main > /dev/null
[ -e "$T/ran" ] && ok "no window possible: the game with saved settings" || fail "exit 4 did not play"

own() { setsid sleep 30 & S=$!; sleep 0.2; st=$(rea_proc_stat "$S"); printf '%s %s\n' "$S" "${st#* }" > "$REA_LOCK"; }
own
check "already running: no second window" "$(main; cat "$T/out")" "0
already running"
check "already running: no second --play" "$(main --play; cat "$T/out")" "0
already running"
check "status when running" "$(main status; cat "$T/out")" "0
running"
rea_stop > /dev/null; wait "$S" 2>/dev/null

REA_EXE=$T/missing.exe
check "no launcher: exit 1" "$(main)" 1
contains "and why" "$(cat "$T/out")" "TestLauncher.exe not found next to x-play (or set TEST_LAUNCHER)"
check "no launcher for --play: exit 1" "$(main --play)" 1
REA_EXE=$T/TestLauncher.exe

# A game script's own commands (SCR: game selectors, pass-through).
rea_hook_command() {
  case "$1" in
    1|2|rally) rea_cmd_default "$@" ;;
    --check) echo "checked $*" ;;
    *) return 127 ;;
  esac
}
rea_hook_usage() { echo "  1|2|rally           pick a game"; }
echo 0 > "$T/rc"; rm -f "$T/ran"
main rally > /dev/null
contains "hook: selector opens the window with it" "$(cat "$T/settings")" "--settings --linux rally"
[ -e "$T/ran" ] && ok "hook: then plays" || fail "hook did not play"
check "hook: own command" "$(main --check now; cat "$T/out")" "0
checked --check now"
check "hook: unknown still exits 2" "$(main nope)" 2
main --help > /dev/null
contains "hook: usage extended" "$(cat "$T/out")" "pick a game"
trap 'rm -rf "$T"' EXIT
finish
