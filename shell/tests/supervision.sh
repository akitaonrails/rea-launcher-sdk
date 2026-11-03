#!/bin/sh
# Owned-session lock, stop, interruption, the inner script and the settings
# exit codes; only harmless sleeps and fake programs run:
# sh shell/tests/supervision.sh
# (merged from the CMR3/CMR4/SRR/OR/SCR supervision tests).
. "$(dirname "$(readlink -f "$0")")/common.sh"
REA_APP=test-launcher REA_PREFIX=$T/prefix REA_SYSFS=$T/nosys
: > "$T/TestLauncher.exe"
REA_EXE=$T/TestLauncher.exe REA_CFG=$T/launcher.cfg
init_lib
make_fake_run
REA_RUN=$T/fake-run REA_GPU=
STALE_PID= STALE_START= OTHER_PID= OTHER_START=
cleanup() {
  stop_helper "$STALE_PID" "$STALE_START"
  stop_helper "$OTHER_PID" "$OTHER_START"
  rm -rf "$T"
}
trap cleanup EXIT HUP INT TERM
# Starts a setsid'd sleep (a session leader) and records it in the lock.
own_session() {
  setsid sleep 30 & OTHER_PID=$!
  i=0; while [ "$i" -lt 40 ]; do
    st=$(rea_proc_stat "$OTHER_PID") && [ "${st%% *}" = "$OTHER_PID" ] && break
    sleep 0.05; i=$((i + 1))
  done
  OTHER_START=${st#* }
}

echo "# the lock"
sleep 30 & STALE_PID=$!
st=$(rea_proc_stat "$STALE_PID"); STALE_START=${st#* }
printf '%s %s\n' "$STALE_PID" "$((STALE_START + 1))" > "$REA_LOCK"
if rea_running; then fail "stale identity reported running"; else ok "a stale/reused pid identity is refused"; fi
rea_stop
if kill -0 "$STALE_PID" 2>/dev/null; then ok "stop does not signal a reused pid"; else fail "stale helper signalled"; fi
[ -f "$REA_LOCK" ] && fail "stop left the lock" || ok "stop removes the lock"
contains "stop ends the private prefix's wineserver" "$(cat "$T/wineserver")" "env WINEPREFIX=$T/prefix wineserver -k"
stop_helper "$STALE_PID" "$STALE_START"; STALE_PID= STALE_START=

# A program named like the game (a script: busybox cannot run a renamed applet).
printf '#!/bin/sh\ntrap '"'"'kill $! 2>/dev/null; exit 143'"'"' TERM\nsleep "$1" & wait\n' > "$T/Game.exe"
chmod +x "$T/Game.exe"
"$T/Game.exe" 30 & OTHER_PID=$!
st=$(rea_proc_stat "$OTHER_PID"); OTHER_START=${st#* }
rm -f "$REA_LOCK"
if rea_running; then fail "an unrelated Game.exe blocked the launch"; else ok "an unrelated Game.exe does not block"; fi
rea_stop
if kill -0 "$OTHER_PID" 2>/dev/null; then ok "an unrelated Game.exe is not signalled"; else fail "unrelated game signalled"; fi
stop_helper "$OTHER_PID" "$OTHER_START"; OTHER_PID= OTHER_START=

printf 'garbage here\n' > "$REA_LOCK"
if rea_running; then fail "a garbage lock is running"; else ok "a garbage lock is idle"; fi

own_session
printf '%s %s\n' "$OTHER_PID" "$OTHER_START" > "$REA_LOCK"
if rea_running; then ok "our own session is running"; else fail "own session not seen"; fi
check "status says running" "$(rea_status)" running
: > "$T/wineserver"
rea_stop
wait "$OTHER_PID"; rc=$?
check "stop TERMs the owned session group" "$rc" 143
check "then wineserver -k in the prefix" "$(cat "$T/wineserver")" "env WINEPREFIX=$T/prefix wineserver -k"
check "status says idle" "$(rea_status)" idle
OTHER_PID= OTHER_START=

echo "# pid-only locks from older scripts"
own_session
echo "$OTHER_PID" > "$REA_LOCK"
if rea_running; then ok "signal: a live pid-only lock is running"; else fail "old lock ignored"; fi
rea_stop
wait "$OTHER_PID"; rc=$?
check "signal: stop TERMs the old pid (CMR3/SRR/SCR)" "$rc" 143
OTHER_PID= OTHER_START=
echo 999999999 > "$REA_LOCK"
if rea_running; then fail "a dead pid-only lock is running"; else ok "a dead pid-only lock is idle"; fi
REA_LEGACY_LOCK=keep
sleep 30 & OTHER_PID=$!; st=$(rea_proc_stat "$OTHER_PID"); OTHER_START=${st#* }
echo "$OTHER_PID" > "$REA_LOCK"
if rea_running; then ok "keep: a live pid-only lock is running (OR)"; else fail "keep: ignored"; fi
rea_stop
if kill -0 "$OTHER_PID" 2>/dev/null; then ok "keep: stop never signals that pid"; else fail "keep: signalled"; fi
REA_LEGACY_LOCK=off
echo "$OTHER_PID" > "$REA_LOCK"
if rea_running; then fail "off: pid-only lock counted"; else ok "off: a pid-only lock is ignored (CMR4)"; fi
rea_stop
if kill -0 "$OTHER_PID" 2>/dev/null; then ok "off: stop never signals it"; else fail "off: signalled"; fi
stop_helper "$OTHER_PID" "$OTHER_START"; OTHER_PID= OTHER_START=
REA_LEGACY_LOCK=signal

echo "# launches"
rea_start > /dev/null
[ -f "$REA_LOCK" ] && fail "a natural exit left the lock" || ok "a natural exit removes the lock"

# Background mode (CMR3/SRR without REA_WAIT): returns at once, lock kept.
cat > "$T/slow-run" <<'FAKE'
#!/bin/sh
case "$*" in *inner.sh*) exec sleep 30 ;; esac
exit 0
FAKE
chmod +x "$T/slow-run"
REA_RUN=$T/slow-run REA_ALWAYS_WAIT=0
rea_start > /dev/null
if rea_running; then ok "a background launch is running"; else fail "background launch not running"; fi
rea_read_lock; SESSION=$REA_LOCK_PID
rea_stop
wait "$SESSION" 2>/dev/null
if kill -0 "$SESSION" 2>/dev/null; then fail "stop left the background session"; else ok "stop ends the background session"; fi
REA_RUN=$T/fake-run REA_WAIT=1
rea_start > /dev/null
[ -f "$REA_LOCK" ] && fail "REA_WAIT launch left the lock" || ok "REA_WAIT makes a background-mode launch wait"
REA_WAIT= REA_ALWAYS_WAIT=1
if ( rea_proc_stat() { return 1; }; REA_ALWAYS_WAIT=0; rea_start > /dev/null ); then
  ok "a launch without a /proc record still succeeds"
else
  fail "launch without a record failed"
fi

# Interrupted while waiting: the session is ended and waited for, the
# prefix's wineserver ended, the lock removed, exit 143.
cat > "$T/slow-run" <<'FAKE'
#!/bin/sh
case "$*" in *wineserver*) echo "$*" >> "$T/wineserver"; exit 0 ;; *inner.sh*) ;; *) exit 0 ;; esac
trap 'sleep 0.5; touch "$T/ended"; exit 0' TERM
sleep 30 & wait
FAKE
REA_RUN=$T/slow-run
: > "$T/wineserver"
( rea_start > /dev/null ) & WAITER=$!
i=0; while ! rea_session_valid && [ "$i" -lt 60 ]; do sleep 0.05; i=$((i + 1)); done
kill -TERM "$WAITER"
wait "$WAITER"; rc=$?
check "an interrupted launch exits 143" "$rc" 143
[ -f "$T/ended" ] && ok "an interrupted launch waits for its session" || fail "returned before its session ended"
[ -f "$REA_LOCK" ] && fail "an interrupted launch left the lock" || ok "an interrupted launch removes the lock"
contains "an interrupted launch ends the prefix's wineserver (OR)" "$(cat "$T/wineserver")" "WINEPREFIX=$T/prefix wineserver -k"
REA_INTERRUPT_WINESERVER=0; rm -f "$T/ended"; : > "$T/wineserver"
( rea_start > /dev/null ) & WAITER=$!
i=0; while ! rea_session_valid && [ "$i" -lt 60 ]; do sleep 0.05; i=$((i + 1)); done
kill -TERM "$WAITER"; wait "$WAITER"
check "REA_INTERRUPT_WINESERVER=0 leaves it to the inner script" "$(cat "$T/wineserver")" ""
REA_INTERRUPT_WINESERVER=1 REA_RUN=$T/fake-run

echo "# the inner script"
mkdir -p "$T/launcher it's \$(here)"
REA_EXE=$T/"launcher it's \$(here)"/TestLauncher.exe; : > "$REA_EXE"
rea_start > /dev/null
check "an awkward launcher path (space, quote, \$()) is one argument" "$(tail -2 "$T/args" | head -1)" "<$REA_EXE>"
inner=$(cat "$REA_INNER")
lacks "the launcher path is absent from the script" "$inner" "$REA_EXE"
contains "the script runs wine \"\$@\"" "$inner" 'wine "$@"'
sh -n "$REA_INNER" && ok "the inner script parses" || fail "inner syntax"
case "$(head -1 "$REA_INNER")" in '#!/bin/sh') ok "inner has a shebang" ;; *) fail "inner shebang" ;; esac

mkdir -p "$T/bin"
cat > "$T/bin/wine" <<'FAKE'
#!/bin/sh
echo "layers=${VK_INSTANCE_LAYERS:-} conf=${VKBASALT_CONFIG_FILE:-} args=$*" > "$T/wine-env"
FAKE
printf '#!/bin/sh\necho "wineserver $*" >> "$T/inner-ws"\n' > "$T/bin/wineserver"
printf '#!/bin/sh\nexit 0\n' > "$T/bin/sleep"
printf '#!/bin/sh\n[ -e "$T/alive" ] && echo 4242\nexit 0\n' > "$T/bin/pgrep"
chmod +x "$T/bin/"*
mkdir -p "$T/proc/4242"; echo '4242 (Game.exe) S 1' > "$T/proc/4242/stat"
printf 'VK_INSTANCE_LAYERS=L1:L2\nVKBASALT_CONFIG_FILE=%s\nbad name=x\n$(touch %s)=y\n' "$T/a b/\$(x).conf" "$T/pwned" > "$T/game.env"
PATH=$T/bin:$PATH REA_GAME_ENV_FILE=$T/game.env sh "$REA_INNER" "$REA_EXE" --no-window
check "game-only env reaches wine, values untouched" "$(cat "$T/wine-env")" \
  "layers=L1:L2 conf=$T/a b/\$(x).conf args=$REA_EXE --no-window"
[ -e "$T/pwned" ] && fail "game.env was evaluated" || ok "game.env is data, never evaluated"
check "the inner script ends the prefix's wineserver" "$(cat "$T/inner-ws")" "wineserver -k"
PATH=$T/bin:$PATH REA_GAME_ENV_FILE= sh "$REA_INNER" "$REA_EXE"
check "no game env file: nothing set" "$(cat "$T/wine-env")" "layers= conf= args=$REA_EXE"
# Waiting for the game's own processes (CMR3, SRR): a fake game that dies
# after a few checks.
cat > "$T/bin/pgrep" <<'FAKE'
#!/bin/sh
echo "$2" >> "$T/pgrep-names"
n=$(cat "$T/polls" 2>/dev/null || echo 0); n=$((n + 1)); echo "$n" > "$T/polls"
[ "$n" -le 4 ] && [ "$2" = "SEGA Rally_SSE1" ] && echo 4242
exit 0
FAKE
chmod +x "$T/bin/pgrep"
PATH=$T/bin:$PATH REA_TEST=1 REA_PROC=$T/proc REA_GAME_PROCS='SEGA Rally.exe|SEGA Rally_SSE1' sh "$REA_INNER" "$REA_EXE"
contains "it waits for each named process" "$(sort -u "$T/pgrep-names" | tr '\n' '|')" "SEGA Rally.exe|SEGA Rally_SSE1|"
[ "$(cat "$T/polls")" -gt 4 ] && ok "it polls until the game is gone" || fail "polls: $(cat "$T/polls")"
rm -f "$T/polls" "$T/pgrep-names"
PATH=$T/bin:$PATH sh "$REA_INNER" "$REA_EXE"
[ -e "$T/polls" ] && fail "polled without REA_GAME_PROCS" || ok "no REA_GAME_PROCS: no wait (OR, CMR4)"
printf '#!/bin/sh\necho "native $*" > "$T/native"\n' > "$T/native-game"; chmod +x "$T/native-game"
: > "$T/inner-ws"
PATH=$T/bin:$PATH REA_INNER_MODE=native REA_GAME_ENV_FILE=$T/game.env sh "$REA_INNER" "$T/native-game" --no-window 2
check "native mode runs the launcher itself" "$(cat "$T/native")" "native --no-window 2"
check "native mode never calls wineserver" "$(cat "$T/inner-ws")" ""

rm -f "$T/polls"
PATH=$T/bin:$PATH REA_PROC=$T/proc REA_GAME_PROCS='SEGA Rally_SSE1' sh "$REA_INNER" "$REA_EXE"
check "the inner script ignores REA_PROC without REA_TEST=1" "$(cat "$T/polls")" 1
rm -f "$T/polls"

echo "# the caller's traps"
# A top-level script (as a game's -play is): its traps come back after a
# launch, and its EXIT trap still runs when the launch is interrupted.
cat > "$T/caller.sh" <<'CALLER'
. "$1"
REA_APP=trap-test REA_PREFIX=$T/prefix2 REA_EXE=$T/TestLauncher.exe REA_GPU= REA_RUN=$2
REA_TEST=1 REA_SYSFS=$T/nosys REA_GAMESCOPE=off
rea_init
trap 'echo caller-exit >> "$T/caller-exit"' EXIT
trap 'echo caller-usr1' USR1
trap 'echo caller-int' INT
rea_start > /dev/null
trap > "$T/caller-traps"
CALLER
sh "$T/caller.sh" "$LIB" "$T/fake-run"
traps=$(cat "$T/caller-traps")
contains "the caller's EXIT trap is back after a launch" "$traps" "caller-exit"
contains "the caller's INT trap is back" "$traps" "caller-int"
contains "an untouched trap is kept" "$traps" "caller-usr1"
lacks "the launch's own traps are gone" "$traps" "rea_launch_interrupted"
check "the caller's EXIT trap ran once at its exit" "$(cat "$T/caller-exit")" caller-exit
rm -f "$T/caller-exit" "$T/ended"
sh "$T/caller.sh" "$LIB" "$T/slow-run" & CALLER=$!
i=0; while [ ! -s "$T/.config/trap-test/running.pid" ] && [ "$i" -lt 60 ]; do sleep 0.05; i=$((i + 1)); done
sleep 0.2
kill -TERM "$CALLER"; wait "$CALLER"; rc=$?
check "interrupted top-level caller exits 143" "$rc" 143
[ -f "$T/ended" ] && ok "it waited for the session" || fail "did not wait"
check "the caller's EXIT trap runs after an interrupted launch" "$(cat "$T/caller-exit" 2>/dev/null)" caller-exit

echo "# the settings window"
REA_EXE=$T/TestLauncher.exe
for case in 0:0 4:0 3:1 1:1 139:1; do
  echo "${case%:*}" > "$T/rc"
  : > "$T/wineserver"
  if rea_settings > "$T/out"; then got=0; else got=1; fi
  check "settings exit ${case%:*} -> $got" "$got" "${case#*:}"
  if [ "${case%:*}" = 3 ]; then
    # Exit 3 is also a second launcher refused while another one (or its
    # game) holds the prefix: waiting for the prefix's wineserver would
    # wait for that game (CMR1 83dacbb). Nothing follows a 3.
    lacks "settings exit 3: no wait for the prefix's wineserver" "$(cat "$T/wineserver")" "wineserver -w"
  else
    contains "settings exit ${case%:*}: the prefix's wineserver is awaited" "$(cat "$T/wineserver")" "wineserver -w"
  fi
done
contains "a failed window points at settings.log" "$(cat "$T/out")" "see $REA_STATE/settings.log"
echo 3 > "$T/rc"; rea_settings > "$T/out"
contains "closed without Play is said" "$(cat "$T/out")" "closed without Play"
check "settings: prefix, SDL, flags" "$(cat "$T/settings")" \
  "env SDL_GAMECONTROLLER_ALLOW_BACKGROUND_EVENTS=1 SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS=1 WINEPREFIX=$T/prefix wine $REA_EXE --settings --linux"
REA_SDL_SETTINGS=0; echo 0 > "$T/rc"; rea_settings > /dev/null
check "REA_SDL_SETTINGS=0 (CMR3, SRR)" "$(cat "$T/settings")" "env WINEPREFIX=$T/prefix wine $REA_EXE --settings --linux"
REA_SDL_SETTINGS=1
rea_settings 2 > /dev/null
contains "settings arguments go to the window" "$(cat "$T/settings")" "--settings --linux 2"
printf '#!/bin/sh\necho oops >&2\nexit 1\n' > "$T/err-run"; chmod +x "$T/err-run"
REA_RUN=$T/err-run rea_settings > /dev/null
check "the window's stderr goes to settings.log" "$(cat "$REA_STATE/settings.log")" oops

echo "# native mode (SCR)"
REA_MODE=native
cat > "$T/Native" <<'FAKE'
#!/bin/sh
case "$*" in *--settings*) exit "$(cat "$T/rc")" ;; esac
sleep 30 & echo $! > "$T/child.pid"
trap 'sleep 0.3; echo saved > "$T/saved"; kill $(cat "$T/child.pid"); exit 0' TERM
wait
FAKE
chmod +x "$T/Native"
REA_EXE=$T/Native REA_RUN= REA_GAMESCOPE_CHECK=false REA_SDL_ENV=
for case in 0:0 4:0 3:1; do
  echo "${case%:*}" > "$T/rc"
  if rea_settings > /dev/null; then got=0; else got=1; fi
  check "native settings exit ${case%:*} -> $got" "$got" "${case#*:}"
done
( rea_start > /dev/null ) & PLAY=$!
i=0; while [ "$i" -lt 60 ] && ! { rea_session_valid && [ -s "$T/child.pid" ]; }; do sleep 0.05; i=$((i + 1)); done
if rea_running; then ok "native: running while the game plays"; else fail "native launch not recorded"; fi
: > "$T/wineserver"
rea_stop
wait "$PLAY" 2>/dev/null
sleep 0.4
if kill -0 "$(cat "$T/child.pid")" 2>/dev/null; then fail "native stop left the game"; else ok "native stop ends the launcher and the game"; fi
[ -s "$T/saved" ] && ok "native stop lets the launcher finish (saves)" || fail "no save on stop"
[ -f "$REA_LOCK" ] && fail "native stop left the lock" || ok "native stop removes the lock"
check "native mode never runs wineserver" "$(cat "$T/wineserver")" ""
rm -f "$T/saved" "$T/child.pid"
( rea_start > /dev/null ) & PLAY=$!
i=0; while [ "$i" -lt 60 ] && ! { rea_session_valid && [ -s "$T/child.pid" ]; }; do sleep 0.05; i=$((i + 1)); done
kill -TERM "$PLAY"; wait "$PLAY"
[ -s "$T/saved" ] && ok "native interrupt waits for the game to finish" || fail "native interrupt did not wait"
[ -f "$REA_LOCK" ] && fail "native interrupt left the lock" || ok "native interrupt removes the lock"
fake_launch() { echo "$*" > "$T/launched"; }
REA_LAUNCH_CMD=fake_launch REA_GAMESCOPE_CHECK=true REA_GAMESCOPE_OUTPUT=0 REA_DEFAULT_SCALER=fsr
printf 'resolution = 1920x1080\n' > "$REA_CFG"
rea_start 2 > /dev/null
contains "REA_LAUNCH_CMD hook (SCR): gamescope without -W/-H" "$(cat "$T/launched")" \
  "gamescope -w 1920 -h 1080 -f -S fit -F fsr -- sh $REA_INNER $T/Native --no-window 2"
lacks "native launch has no WINEPREFIX" "$(cat "$T/launched")" "WINEPREFIX"
trap cleanup EXIT HUP INT TERM
finish
