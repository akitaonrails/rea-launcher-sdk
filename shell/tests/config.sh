#!/bin/sh
# launcher.cfg discovery, rea_get_key, rea_parse_size, the screen probe and
# the game size: sh shell/tests/config.sh
. "$(dirname "$(readlink -f "$0")")/common.sh"
export REA_APP=test-launcher REA_EXE_NAME=TestLauncher.exe REA_CFG_APPDATA=TestLauncher
mkdir -p "$T/kit/linux" "$T/prefix/drive_c/users/me/AppData/Local/TestLauncher" \
  "$T/prefix/drive_c/users/Public/AppData/Local/TestLauncher"
: > "$T/kit/TestLauncher.exe"
REA_HERE=$T/kit/linux REA_PREFIX=$T/prefix
load_lib

echo "# paths"
check "exe found one folder up from the script" "$REA_EXE" "$T/kit/TestLauncher.exe"
check "state folder under ~/.config" "$REA_STATE" "$T/.config/test-launcher"
check "cfg in the prefix user's AppData (Public skipped)" "$REA_CFG" \
  "$T/prefix/drive_c/users/me/AppData/Local/TestLauncher/launcher.cfg"
: > "$T/kit/launcher.cfg"
rea_find_cfg
check "a launcher.cfg next to the exe wins (portable install)" "$REA_CFG" "$T/kit/launcher.cfg"
rm "$T/kit/launcher.cfg"
got=$(XDG_CONFIG_HOME=$T/xdg REA_STATE= REA_PREFIX=$T/prefix sh -c '. "$1"; echo "$REA_STATE|$REA_LOCK"' sh "$LIB")
check "XDG_CONFIG_HOME is honoured" "$got" "$T/xdg/test-launcher|$T/xdg/test-launcher/running.pid"
got=$(REA_PREFIX= sh -c '. "$1"; echo "$REA_PREFIX"' sh "$LIB")
check "default prefix" "$got" "$T/.local/share/test-launcher/prefix"
got=$(REA_MODE=native REA_PREFIX= REA_EXE_NAME= sh -c '. "$1"; echo "$REA_CFG|$REA_PREFIX"' sh "$LIB")
check "native mode: cfg in the state folder, no prefix" "$got" "$T/.config/test-launcher/launcher.cfg|"
got=$(REA_APP= sh -c '. "$1"' sh "$LIB" 2>&1)
contains "REA_APP is required" "$got" "REA_APP is not set"
mkdir -p "$T/native"; : > "$T/native/Game"
got=$(REA_MODE=native REA_HERE=$T/native REA_EXE_NAME=Game sh -c '. "$1"; echo "[$REA_EXE]"' sh "$LIB")
check "native mode needs an executable launcher" "$got" "[]"
chmod +x "$T/native/Game"
got=$(REA_MODE=native REA_HERE=$T/native REA_EXE_NAME=Game sh -c '. "$1"; echo "[$REA_EXE]"' sh "$LIB")
check "native executable found" "$got" "[$T/native/Game]"
mkdir -p "$T/native/target/release"; printf '#!/bin/sh\n' > "$T/native/target/release/Other"; chmod +x "$T/native/target/release/Other"
got=$(REA_MODE=native REA_HERE=$T/native REA_EXE_NAME=Other REA_EXE_SEARCH="$T/native/target/release/Other" sh -c '. "$1"; echo "[$REA_EXE]"' sh "$LIB")
check "REA_EXE_SEARCH candidates" "$got" "[$T/native/target/release/Other]"
REA_EXE=$T/missing.exe
contains "a missing launcher is reported with its variable" "$(rea_require_exe 2>&1)" "TestLauncher.exe not found next to x-play (or set REA_EXE)"
REA_EXE=$T/kit/TestLauncher.exe

echo "# rea_get_key"
C=$T/k.cfg
printf 'resolution = 1280x720\n# resolution = 640x480\n  resolution=  2560X1440 \r\nscaler=fsr\n' > "$C"
check "last active line wins, trimmed, CR dropped" "$(rea_get_key resolution "$C")" "2560X1440"
check "no spaces around =" "$(rea_get_key scaler "$C")" "fsr"
check "a missing key is empty" "$(rea_get_key nothing "$C")" ""
check "a missing file is empty" "$(rea_get_key resolution "$T/none.cfg")" ""
printf 'game = C:\\Games\\a=b\n' > "$C"
check "a value may hold = and backslashes" "$(rea_get_key game "$C")" 'C:\Games\a=b'
printf 'res.lution = 1x1\nresolution = 2x2\n' > "$C"
check "the key is text, not a pattern" "$(rea_get_key 'res.lution' "$C")" "1x1"
printf 'resolution = 1x1\nresolution =\n' > "$C"
check "an empty last line unsets" "$(rea_get_key resolution "$C")" ""
printf '#resolution = 3x3\n\t# scaler = x\nresolutionx = 4x4\n' > "$C"
check "commented and longer keys ignored" "$(rea_get_key resolution "$C")" ""
REA_CFG=$C; printf 'scaler = nis\n' > "$C"
check "REA_CFG is the default file" "$(rea_get_key scaler)" "nis"

echo "# rea_parse_size"
for ok_case in "1920x1080:1920x1080" "1920X1080:1920x1080" " 2560 x 1440 :2560x1440" \
    "1280x800\r:1280x800" "01280x0800:1280x800" "	640x480	:640x480"; do
  in=$(printf "${ok_case%:*}"); want=${ok_case##*:}
  check "accepts '${ok_case%:*}'" "$(rea_parse_size "$in")" "$want"
done
for bad in "" 0x0 0x1080 1920x0 x1080 1920x 1920x1080x2 huge 4kx 19a0x1080 '$(touch pwned)x1' \
    -1920x1080 1920 123456x1 fullscreen original; do
  if rea_parse_size "$bad" > /dev/null; then fail "rejects '$bad'"; else ok "rejects '$bad'"; fi
done
[ -e pwned ] && fail "a size was evaluated"

echo "# rea_screen_size"
check "REA_SCREEN overrides" "$(REA_SCREEN=2560x1600 rea_screen_size)" "2560x1600"
if REA_SCREEN=bogus rea_screen_size > /dev/null; then fail "bogus REA_SCREEN"; else ok "a bogus REA_SCREEN is no size"; fi
mkdir -p "$T/bin"
printf '#!/bin/sh\necho "Screen 0: minimum 8 x 8"\necho "HDMI-1 connected"\necho "   3440x1440     59.97*+"\n' > "$T/bin/xrandr"
chmod +x "$T/bin/xrandr"
check "xrandr's current mode" "$(PATH=$T/bin:$PATH rea_screen_size)" "3440x1440"

echo "# rea_game_size"
size() { # name cfg-line expected [screen]
  printf '%b' "$2" > "$C"
  REA_SCREEN=${4:-} rea_game_size
  check "$1" "${REA_W}x$REA_H" "$3"
}
size "configured size" 'resolution = 3840x2160\n' 3840x2160
size "unset resolution: default" 'resolution =\n' 1920x1080
size "garbage: default" 'resolution = big\n' 1920x1080
size "original without REA_ORIGINAL_SIZE: default (CMR4, SCR)" 'resolution = original\n' 1920x1080
REA_ORIGINAL_SIZE=640x480
size "original with REA_ORIGINAL_SIZE (CMR3, OR)" 'resolution = Original\n' 640x480
size "fullscreen without REA_FULLSCREEN_SIZE: default" 'resolution = fullscreen\n' 1920x1080 2560x1600
REA_FULLSCREEN_SIZE=screen
size "fullscreen is the screen's size (SCR)" 'resolution = fullscreen\n' 2560x1600 2560x1600
size "a bogus screen size: default" 'resolution = fullscreen\n' 1920x1080 0x0
REA_DEFAULT_SIZE=1024x768
size "REA_DEFAULT_SIZE (CMR4 1024x768)" 'game = C:\\x\n' 1024x768
rm -f "$C"
size "no launcher.cfg: default" '' 1024x768
finish
