#!/bin/sh
# rea_main: the commands every helper understands, and the hook a game
# script extends them with: sh shell/tests/dispatch.sh
. "$(dirname "$(readlink -f "$0")")/common.sh"
REA_APP=test-launcher REA_PREFIX=$T/prefix REA_SYSFS=$T/nosys
: > "$T/TestLauncher.exe"
REA_EXE=$T/TestLauncher.exe REA_CFG=$T/launcher.cfg REA_EXE_NAME=TestLauncher.exe REA_EXE_HINT=TEST_LAUNCHER
init_lib
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

echo "# the hook sees every command first (screamer-play's CLI, kept exactly)"
mkdir -p "$T/scr/linux"
cp "$LIB" "$T/scr/linux/rea-play.sh"
cat > "$T/scr/ScreamerLauncher" <<'FAKE'
#!/bin/sh
case "$*" in *--settings*--linux*) echo "window $*" >> "$T/scr.log"; exit 0 ;; esac
echo "launcher $*" >> "$T/scr.log"
FAKE
chmod +x "$T/scr/ScreamerLauncher"
# The Screamer recipe from README.md.
cat > "$T/scr/linux/screamer-play" <<'SCR'
#!/bin/sh
set -u
_fonly=${REA_FUNCTIONS_ONLY:-}
_here=$(dirname "$(readlink -f "$0")")
[ "$_fonly" = 1 ] && [ -n "${REA_PLAY_HERE:-}" ] && _here=$REA_PLAY_HERE
if [ -f "$_here/rea-play.sh" ]; then . "$_here/rea-play.sh"
elif [ -f "$_here/../sdk/rea-play.sh" ]; then . "$_here/../sdk/rea-play.sh"
else echo "rea-play.sh not found next to $0 or in ../sdk" >&2; exit 1; fi
REA_HERE=$_here REA_NAME=screamer-play
REA_MODE=native REA_APP=screamer-launcher REA_EXE_NAME=ScreamerLauncher
REA_EXE_HINT=SCREAMER_LAUNCHER REA_EXE=${SCREAMER_LAUNCHER:-}
REA_EXE_SEARCH="../target/release/ScreamerLauncher ../target/debug/ScreamerLauncher"
REA_STATE_BASE=${XDG_CONFIG_HOME:-$HOME/.config}
REA_GAMESCOPE_OUTPUT=0 REA_DEFAULT_SCALER=fsr REA_FULLSCREEN_SIZE=screen
REA_SCREEN=${SCREAMER_SCREEN:-} REA_SDL_ENV= REA_WAIT=${SCREAMER_WAIT:-}
rea_hook_command() {
  case "${1:-}" in
    --play|start|status|stop) return 127 ;;
    1|2|3|s1|s2|sr|screamer|screamer1|screamer2|rally|screamer-rally) rea_cmd_default "$@" ;;
    *) rea_require_exe || exit 1; exec "$REA_EXE" "$@" ;;
  esac
}
rea_init
[ "$_fonly" = 1 ] && return 0
rea_main "$@"
SCR
scr() { rm -f "$T/scr.log"; PATH=$T/scr:$PATH sh "$T/scr/linux/screamer-play" "$@" > "$T/out" 2>&1; echo $?; }
printf '#!/bin/sh\nexit 1\n' > "$T/scr/gamescope-absent"
for cmd in --help -h help --settings settings play --status --stop --check --controllers; do
  scr "$cmd" > /dev/null
  check "screamer-play $cmd goes to ScreamerLauncher" "$(cat "$T/scr.log" 2>/dev/null)" "launcher $cmd"
done
check "screamer-play status is the helper's" "$(scr status; cat "$T/out")" "0
idle"
check "screamer-play stop is the helper's" "$(scr stop; cat "$T/out")" "0
stopped"
scr rally > /dev/null
contains "screamer-play rally: the window with the game picked" "$(cat "$T/scr.log")" "window --settings --linux rally"
contains "...then the game, without the selector (as today)" "$(cat "$T/scr.log")" "launcher --no-window"
scr --play 2 > /dev/null
check "screamer-play --play 2" "$(cat "$T/scr.log")" "launcher --no-window 2"
XDG_CONFIG_HOME=$T/xdg scr status > /dev/null
[ -d "$T/xdg/screamer-launcher" ] && ok "the state folder follows XDG_CONFIG_HOME for Screamer" || fail "Screamer state not under XDG"
mkdir -p "$T/scr2/linux" "$T/scr2/target/release"
cp "$T/scr/linux/screamer-play" "$T/scr/linux/rea-play.sh" "$T/scr2/linux/"
cp "$T/scr/ScreamerLauncher" "$T/scr2/target/release/"
rm -f "$T/scr.log"; (cd / && sh "$T/scr2/linux/screamer-play" --check > /dev/null 2>&1)
check "a relative REA_EXE_SEARCH works from any cwd" "$(cat "$T/scr.log" 2>/dev/null)" "launcher --check"
got=$(cd / && REA_FUNCTIONS_ONLY=1 REA_PLAY_HERE=$T/scr2/linux sh -c '. "$1"; echo "$REA_EXE"' sh "$T/scr2/linux/screamer-play")
check "the recipe sourced by a test finds its launcher" "$got" "$T/scr2/target/release/ScreamerLauncher"
rm "$T/scr/ScreamerLauncher"
check "no launcher for a pass-through: exit 1" "$(scr --check)" 1
contains "with the reason" "$(cat "$T/out")" "ScreamerLauncher not found next to screamer-play (or set SCREAMER_LAUNCHER)"

trap 'rm -rf "$T"' EXIT
finish
