#!/bin/sh
# The game command: gamescope size, output size and filter from
# launcher.cfg, the cases that skip gamescope, and the environment the
# session gets (prefix, SDL, DLL overrides, game-only env):
# sh shell/tests/gamescope.sh
# (merged from CMR3/OR scaler.sh, CMR4 gamescope.sh and SCR scaler.sh).
. "$(dirname "$(readlink -f "$0")")/common.sh"
REA_APP=test-launcher REA_PREFIX=$T/prefix REA_SYSFS=$T/nosys
: > "$T/TestLauncher.exe"
REA_EXE=$T/TestLauncher.exe REA_CFG=$T/launcher.cfg
init_lib
make_fake_run
REA_RUN=$T/fake-run REA_GPU=
C=$REA_CFG
ran() { cat "$T/ran"; }
run_with() { # cfg-text
  printf '%b' "$1" > "$C"
  rm -f "$T/ran"
  rea_start > "$T/out"
}

echo "# filter (-F) from the scaler"
for scaler in nearest pixel fsr linear nis; do
  run_with "resolution = 1920x1080\nscaler = $scaler\n"
  contains "scaler $scaler" "$(ran)" "gamescope -W 1920 -H 1080 -w 1920 -h 1080 -f -S fit -F $scaler -- sh $REA_INNER $REA_EXE --no-window"
done
run_with 'resolution = 1920x1080\n'
contains "default scaler linear (CMR3, OR)" "$(ran)" "-F linear --"
run_with 'scaler = FSR \r\n'
contains "scaler spellings: case, spaces, CRLF" "$(ran)" "-F fsr --"
run_with 'scaler = sharpest\n'
contains "an unknown scaler is the default, never passed to gamescope" "$(ran)" "-F linear --"
run_with 'scaler = bilinear\n'
contains "bilinear is linear" "$(ran)" "-F linear --"
REA_DEFAULT_SCALER=fsr
run_with 'resolution = 1920x1080\n'
contains "REA_DEFAULT_SCALER (SCR fsr)" "$(ran)" "-F fsr --"
REA_SCALER_KEY= REA_DEFAULT_SCALER=nis
run_with 'scaler = fsr\n'
contains "no scaler key: a fixed filter (CMR4 nis)" "$(ran)" "-F nis --"
REA_SCALER_KEY=scaler REA_DEFAULT_SCALER=linear

echo "# size (-w/-h and the output -W/-H)"
run_with 'resolution = 3840x2160\n'
contains "4K nested and output" "$(ran)" "gamescope -W 3840 -H 2160 -w 3840 -h 2160 -f"
run_with '  resolution = 2560X1440 \r\n'
contains "launcher spellings" "$(ran)" "-W 2560 -H 1440 -w 2560 -h 1440 "
run_with 'resolution = 1280x800\n# resolution = 640x480\n'
contains "a commented resolution is ignored" "$(ran)" "-w 1280 -h 800 "
for bad in huge 4kx x1080 1920x1080x2 0x0 '$(touch pwned)x1'; do
  run_with "resolution = $bad\n"
  contains "invalid '$bad' is the default" "$(ran)" "-W 1920 -H 1080 -w 1920 -h 1080 "
done
[ -e pwned ] && fail "a resolution was evaluated"
rm -f "$C"; rm -f "$T/ran"; rea_start > /dev/null
contains "no launcher.cfg: default" "$(ran)" "-w 1920 -h 1080 "
REA_DEFAULT_SIZE=1024x768
run_with 'resolution = original\n'
contains "CMR4: original and unset are 1024x768" "$(ran)" "gamescope -W 1024 -H 768 -w 1024 -h 768 -f -S fit"
REA_DEFAULT_SIZE=1920x1080 REA_ORIGINAL_SIZE=640x480
run_with 'resolution = original\n'
contains "CMR3/OR: original is 640x480" "$(ran)" "-w 640 -h 480 -f"
REA_ORIGINAL_SIZE=
REA_GAMESCOPE_OUTPUT=0 REA_FULLSCREEN_SIZE=screen REA_SCREEN=2560x1600
run_with 'resolution = fullscreen\n'
contains "SCR: no -W/-H, fullscreen is the screen" "$(ran)" "gamescope -w 2560 -h 1600 -f -S fit"
lacks "SCR: no output size" "$(ran)" "-W "
REA_GAMESCOPE_OUTPUT=1 REA_FULLSCREEN_SIZE= REA_SCREEN=
REA_GAMESCOPE_EXTRA="--adaptive-sync"
run_with 'resolution = 1920x1080\n'
contains "REA_GAMESCOPE_EXTRA before --" "$(ran)" "-F linear --adaptive-sync -- sh"
REA_GAMESCOPE_EXTRA=
contains "the size is reported" "$(cat "$T/out")" "starting at 1920x1080, scaler linear"

echo "# without a gamescope of our own"
REA_WINDOWED_KEY=display_mode
for m in windowed borderless Borderless; do
  run_with "resolution = 1920x1080\ndisplay_mode = $m\n"
  lacks "CMR3: $m skips gamescope" "$(ran)" "gamescope"
  contains "CMR3: $m runs the inner script" "$(ran)" "sh $REA_INNER $REA_EXE --no-window"
done
run_with 'display_mode = fullscreen\n'
contains "CMR3: fullscreen keeps gamescope" "$(ran)" "gamescope -W"
REA_WINDOWED_KEY=
run_with 'display_mode = borderless\n'
contains "OR: no windowed key, borderless stays in gamescope" "$(ran)" "gamescope -W"
GAMESCOPE_WAYLAND_DISPLAY=fake run_with 'resolution = 1920x1080\n'
lacks "already inside gamescope: never nested" "$(ran)" "gamescope"
contains "the note says so" "$(cat "$T/out")" "starting without a gamescope of our own"
: > "$T/no-gamescope"
run_with 'resolution = 1920x1080\n'
lacks "no gamescope installed: direct" "$(ran)" "gamescope"
rm "$T/no-gamescope"
REA_GAMESCOPE=off REA_DIRECT_NOTE="starting (isolated Wine desktop)"
run_with 'resolution = 1920x1080\n'
lacks "SRR: REA_GAMESCOPE=off" "$(ran)" "gamescope"
contains "REA_DIRECT_NOTE" "$(cat "$T/out")" "starting (isolated Wine desktop)"
REA_GAMESCOPE=auto REA_DIRECT_NOTE=
fake_gs() { return 1; }
REA_GAMESCOPE_CHECK=fake_gs
run_with 'resolution = 1920x1080\n'
lacks "REA_GAMESCOPE_CHECK hook" "$(ran)" "gamescope"
REA_GAMESCOPE_CHECK=

echo "# environment"
run_with 'resolution = 1920x1080\n'
contains "the private prefix" "$(ran)" "env WINEPREFIX=$T/prefix "
contains "SDL background events default on" "$(ran)" "SDL_GAMECONTROLLER_ALLOW_BACKGROUND_EVENTS=1 SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS=1"
SDL_GAMECONTROLLER_ALLOW_BACKGROUND_EVENTS=0 SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS=0 run_with ''
contains "the caller's SDL choices win" "$(ran)" "SDL_GAMECONTROLLER_ALLOW_BACKGROUND_EVENTS=0 SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS=0"
REA_SDL_ENV="SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS=1 SDL_JOYSTICK_HIDAPI=0 SDL_JOYSTICK_DEVICE=/dev/input/js0"
run_with ''
contains "CMR3's SDL set" "$(ran)" "SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS=1 SDL_JOYSTICK_HIDAPI=0"
lacks "SDL_JOYSTICK_DEVICE is never set" "$(ran)" "SDL_JOYSTICK_DEVICE"
lacks "only the listed SDL variables" "$(ran)" "SDL_GAMECONTROLLER"
REA_SDL_ENV=
run_with ''
lacks "REA_SDL_ENV empty: none (SCR)" "$(ran)" "SDL_"
REA_SDL_ENV="SDL_GAMECONTROLLER_ALLOW_BACKGROUND_EVENTS=1 SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS=1"
lacks "no DLL overrides unless the game sets them" "$(ran)" "WINEDLLOVERRIDES"
REA_DLL_OVERRIDES='dinput8,dsound,d3d9=n,b' REA_WINEDBG_OFF=1
WINEDLLOVERRIDES='mscoree,mshtml=' run_with ''
contains "the game's overrides before the AppImage's" "$(ran)" "WINEDLLOVERRIDES=dinput8,dsound,d3d9=n,b;winedbg=d;mscoree,mshtml= "
check "the caller's WINEDLLOVERRIDES is not changed" "${WINEDLLOVERRIDES:-unset}" "unset"
run_with ''
contains "repeated starts do not stack overrides" "$(ran)" "WINEDLLOVERRIDES=dinput8,dsound,d3d9=n,b;winedbg=d "
REA_DLL_OVERRIDES= REA_WINEDBG_OFF=0
REA_GPU="DRI_PRIME=1"
run_with ''
contains "the GPU choice" "$(ran)" "WINEPREFIX=$T/prefix DRI_PRIME=1 "
REA_GPU=
rea_hook_before_start() {
  rea_game_env VK_INSTANCE_LAYERS "VK_LAYER_VKBASALT_post_processing:VK_LAYER_VKBASALT_post_processing32"
  rea_game_env VKBASALT_CONFIG_FILE "$T/a dir/vkBasalt.conf"
}
run_with 'resolution = 1920x1080\n'
lacks "game-only env never reaches gamescope's environment" "$(ran)" "VK_INSTANCE_LAYERS"
contains "it is handed to the inner script as a file" "$(ran)" "REA_GAME_ENV_FILE=$REA_STATE/game.env"
check "the file holds the assignments" "$(cat "$REA_STATE/game.env")" \
  "VK_INSTANCE_LAYERS=VK_LAYER_VKBASALT_post_processing:VK_LAYER_VKBASALT_post_processing32
VKBASALT_CONFIG_FILE=$T/a dir/vkBasalt.conf"
run_with 'resolution = 1920x1080\n'
check "repeated starts rewrite it" "$(grep -c VK_INSTANCE_LAYERS "$REA_STATE/game.env")" 1
if rea_game_env 'BAD NAME' x 2>/dev/null; then fail "a bad name accepted"; else ok "rea_game_env rejects a bad name"; fi
if rea_game_env GOOD "a
b" 2>/dev/null; then fail "a newline accepted"; else ok "rea_game_env rejects a newline"; fi
rea_hook_before_start() { :; }
run_with ''
check "no game env: an empty file" "$(cat "$REA_STATE/game.env")" ""

echo "# arguments"
run_with ''
rea_start --which rally > /dev/null
check "extra start arguments follow REA_PLAY_ARGS" "$(tail -3 "$T/args" | tr '\n' ' ')" "<--no-window> <--which> <rally> "
REA_PLAY_ARGS="--no-window --linux"
rea_start > /dev/null
check "SRR's play flags" "$(tail -2 "$T/args" | tr '\n' ' ')" "<--no-window> <--linux> "
REA_PLAY_ARGS=--no-window

echo "# one-run flags for the keys the helper reads (REA_HELPER_KEYS)"
REA_HELPER_KEYS="scaler resolution display_mode postfx dxvk" REA_WINDOWED_KEY=display_mode
printf 'resolution = 1920x1080\nscaler = linear\ndisplay_mode = fullscreen\n' > "$C"
rm -f "$T/ran"; rea_start --scaler fsr --resolution 1280x720 > /dev/null
contains "--scaler/--resolution override launcher.cfg for the run" "$(ran)" "gamescope -W 1280 -H 720 -w 1280 -h 720 -f -S fit -F fsr --"
check "the flags still reach the launcher" "$(tail -4 "$T/args" | tr '\n' ' ')" "<--scaler> <fsr> <--resolution> <1280x720> "
check "launcher.cfg is untouched" "$(cat "$C")" "resolution = 1920x1080
scaler = linear
display_mode = fullscreen"
rm -f "$T/ran"; rea_start > /dev/null
contains "the next run reads launcher.cfg again" "$(ran)" "-w 1920 -h 1080 -f -S fit -F linear --"
check "nothing left over after a run" "${REA_ONE_RUN:-}" ""
rm -f "$T/ran"; rea_start --display-mode windowed > /dev/null
lacks "--display-mode (key display_mode) skips gamescope for the run" "$(ran)" "gamescope"
rm -f "$T/ran"; rea_start --scaler=nearest --resolution=2560x1440 > /dev/null
contains "--key=value spelling" "$(ran)" "-w 2560 -h 1440 -f -S fit -F nearest --"
rm -f "$T/ran"; rea_start --no-window --scaler fsr --check --scaler pixel > /dev/null
contains "switches take no value; the last flag wins" "$(ran)" "-F pixel --"
rm -f "$T/ran"; rea_start --game 'C:\\x' --resolution 3840x2160 --runtime /r > /dev/null
contains "other flags' values are skipped, not read as flags" "$(ran)" "-w 3840 -h 2160 "
rm -f "$T/ran"; rea_start --resolution > /dev/null
contains "a flag without its value changes nothing" "$(ran)" "-w 1920 -h 1080 "
rm -f "$T/ran"; rea_start --fps 60 --resolution junk > /dev/null
contains "an invalid one-run size is the default, as from the file" "$(ran)" "-w 1920 -h 1080 "
rm -f "$T/ran"; rea_start --unrelated 1280x720 1280x720 > /dev/null
contains "positional words are not values" "$(ran)" "-w 1920 -h 1080 "
rea_hook_before_start() { rea_get_key postfx > "$T/postfx"; rea_get_key postfx "$C" > "$T/postfx-file"; }
printf 'postfx = off\n' >> "$C"
rea_start --postfx color > /dev/null
check "hooks see the one-run value (CMR3 postfx)" "$(cat "$T/postfx")" color
check "an explicit FILE is read as it is" "$(cat "$T/postfx-file")" off
rea_hook_before_start() { :; }
REA_HELPER_KEYS=
rm -f "$T/ran"; rea_start --scaler fsr > /dev/null
contains "no REA_HELPER_KEYS: flags only go to the launcher" "$(ran)" "-F linear --"
REA_WINDOWED_KEY=
rea_cmd_play_test() { REA_HELPER_KEYS=scaler; rm -f "$T/ran"; rea_main --play --scaler nis > /dev/null; }
rea_cmd_play_test
contains "through rea_main --play" "$(ran)" "-F nis --"
REA_HELPER_KEYS=

echo "# player values never glob"
mkdir -p "$T/globdir"; : > "$T/globdir/AAA"; : > "$T/globdir/BBB"
cd "$T/globdir"
REA_PLAY_ARGS='--no-window *' REA_GAMESCOPE_EXTRA='--x *' REA_GPU='GLOB=*'
SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS='*'
run_with 'resolution = 1920x1080\n'
lacks "no file names in the command" "$(ran)" "AAA"
contains "REA_PLAY_ARGS keeps a literal *" "$(tail -1 "$T/args")" "<*>"
contains "REA_GAMESCOPE_EXTRA keeps a literal *" "$(ran)" "--x * --"
contains "REA_GPU keeps a literal *" "$(ran)" "GLOB=* "
contains "a caller's SDL value keeps a literal *" "$(ran)" "SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS=* "
echo 0 > "$T/rc"; rea_settings > /dev/null
lacks "the window's command never globs" "$(cat "$T/settings")" "AAA"
case $- in *f*) fail "globbing left off after a start" ;; *) ok "globbing is back on after a start" ;; esac
set -f; run_with ''; case $- in *f*) ok "a caller's own set -f is kept" ;; *) fail "set -f was dropped" ;; esac; set +f
unset SDL_JOYSTICK_ALLOW_BACKGROUND_EVENTS
REA_PLAY_ARGS=--no-window REA_GAMESCOPE_EXTRA= REA_GPU=
cd "$T"

trap 'rm -rf "$T"' EXIT
finish
