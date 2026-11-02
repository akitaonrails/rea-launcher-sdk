# Sourced by every test: a scratch HOME, ok/fail counters, helpers, and the
# library itself. Each test then sets its REA_* configuration (as a game
# script does, after sourcing) and calls init_lib.
set -u
TESTS=$(dirname "$(readlink -f "$0")")
LIB=$TESTS/../rea-play.sh
T=$(mktemp -d)
trap 'rm -rf "$T"' EXIT
export T HOME=$T
unset XDG_CONFIG_HOME GAMESCOPE_WAYLAND_DISPLAY DRI_PRIME __NV_PRIME_RENDER_OFFLOAD \
  __GLX_VENDOR_LIBRARY_NAME VK_DRIVER_FILES VK_ICD_FILENAMES VK_INSTANCE_LAYERS \
  VK_LAYER_PATH WINEDLLOVERRIDES SDL_GAMECONTROLLER_ALLOW_BACKGROUND_EVENTS \
  SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS SDL_JOYSTICK_HIDAPI REA_WAIT REA_SCREEN 2>/dev/null
fails=0
ok() { echo "ok   $1"; }
fail() { echo "FAIL $1"; fails=$((fails + 1)); }
check() { # name got want
  if [ "$2" = "$3" ]; then ok "$1"; else fail "$1: got '$2', want '$3'"; fi
}
contains() { # name haystack needle
  case "$2" in *"$3"*) ok "$1" ;; *) fail "$1: '$2' lacks '$3'" ;; esac
}
lacks() { # name haystack needle
  case "$2" in *"$3"*) fail "$1: '$2' has '$3'" ;; *) ok "$1" ;; esac
}
finish() {
  if [ "$fails" = 0 ]; then echo "all passed"; exit 0; fi
  echo "$fails failed"; exit 1
}
# Kills a helper only while it is still the process we started.
stop_helper() { # pid start
  [ -n "$1" ] || return 0
  _st=$(rea_proc_stat "$1") || { wait "$1" 2>/dev/null; return 0; }
  if [ "${_st#* }" = "$2" ]; then kill "$1" 2>/dev/null || true; fi
  wait "$1" 2>/dev/null || true
}
init_lib() {
  REA_TEST=1
  REA_HERE=${REA_HERE:-$T}
  REA_NAME=${REA_NAME:-x-play}
  rea_init
}
# fresh 'ASSIGNMENTS' 'COMMANDS': a new shell that sources the library,
# runs ASSIGNMENTS as the game script would, rea_init, then COMMANDS.
fresh() {
  sh -c '. "$1"; eval "$2"; REA_TEST=1; rea_init && eval "$3"' sh "$LIB" "$1" "$2"
}
# A recording wrapper (a program: setsid cannot run a shell function).
# gamescope is "installed" unless $T/no-gamescope exists; a settings run
# exits with $T/rc; everything else is recorded in $T/ran and $T/args.
make_fake_run() {
  cat > "$T/fake-run" <<'FAKE'
#!/bin/sh
case "$*" in
  *"command -v gamescope"*) [ ! -e "$T/no-gamescope" ]; exit ;;
  *"wineserver"*) echo "$*" >> "$T/wineserver"; exit 0 ;;
  *"--settings"*) echo "$*" > "$T/settings"; exit "$(cat "$T/rc" 2>/dev/null || echo 0)" ;;
esac
echo "$*" > "$T/ran"
printf '<%s>\n' "$@" > "$T/args"
FAKE
  chmod +x "$T/fake-run"
}
. "$LIB"
