#!/bin/sh
# Wine prefix preparation: the crash dialog, DLL overrides, one-time
# markers and the winebus seed: sh shell/tests/prefix.sh
# (merged from CMR3 prefix.sh and SRR winebus.sh).
. "$(dirname "$(readlink -f "$0")")/common.sh"
REA_APP=test-launcher REA_PREFIX=$T/prefix REA_SYSFS=$T/nosys
init_lib
REA_GPU=
# A stand-in wine that records its calls and writes what reg add would.
mkdir -p "$T/bin"
cat > "$T/bin/wine" <<'WINE'
#!/bin/sh
echo "wine $*" >> "$T/calls"
case "$*" in *ShowCrashDialog*) echo '"ShowCrashDialog"=dword:00000000' >> "$WINEPREFIX/user.reg" ;; esac
WINE
printf '#!/bin/sh\necho "wineserver $* $WINEPREFIX" >> "$T/calls"\n' > "$T/bin/wineserver"
chmod +x "$T/bin/wine" "$T/bin/wineserver"
PATH=$T/bin:$PATH

echo "# crash dialog (CMR3)"
rea_wine_no_crash_dialog
rea_wine_no_crash_dialog
check "turned off once" "$(grep -c 'reg add' "$T/calls")" 1
contains "under HKCU\\Software\\Wine\\WineDbg" "$(cat "$T/calls")" 'reg add HKCU\Software\Wine\WineDbg /v ShowCrashDialog /t REG_DWORD /d 0 /f'
check "its wineserver awaited, in our prefix" "$(grep -c "wineserver -w $T/prefix" "$T/calls")" 1

echo "# DLL overrides"
unset WINEDLLOVERRIDES
check "none by default" "$(rea_dll_overrides)" ""
REA_DLL_OVERRIDES='dinput8,dsound,d3d9=n,b' REA_WINEDBG_OFF=1
check "the game's own, winedbg off" "$(rea_dll_overrides)" "dinput8,dsound,d3d9=n,b;winedbg=d"
WINEDLLOVERRIDES="mscoree,mshtml="
check "kept beside the AppImage's" "$(rea_dll_overrides)" "dinput8,dsound,d3d9=n,b;winedbg=d;mscoree,mshtml="
REA_DLL_OVERRIDES= REA_WINEDBG_OFF=0
check "only the caller's" "$(rea_dll_overrides)" "mscoree,mshtml="
unset WINEDLLOVERRIDES

echo "# rea_once"
step() { echo step >> "$T/steps"; return "${STEP_RC:-0}"; }
STEP_RC=1 rea_once .test-marker step
[ -f "$T/prefix/.test-marker" ] && fail "marker made after a failure" || ok "a failed step makes no marker"
rea_once .test-marker step; rea_once .test-marker step
check "it runs until it succeeds, then never" "$(wc -l < "$T/steps" | tr -d ' ')" 2
[ -f "$T/prefix/.test-marker" ] && ok "the marker lives in the prefix" || fail "no marker"

echo "# winebus (SRR)"
cat > "$T/run" <<'FAKE'
#!/bin/sh
printf '%s|' "$@" >> "$T/log"; echo >> "$T/log"
case "$*" in *"reg query"*"Enable SDL"*) exit 0 ;; *"reg query"*) exit 1 ;; esac
exit 0
FAKE
chmod +x "$T/run"
REA_RUN=$T/run
rea_seed_winebus > "$T/out"
for name in DisableHidraw DisableInput "Map Controllers"; do
  if grep -q "|reg|add|HKLM.System.CurrentControlSet.Services.winebus|/v|$name|/t|REG_DWORD|/d|1|/f|" "$T/log"; then
    ok "$name seeded"
  else
    fail "$name not seeded: $(cat "$T/log")"
  fi
done
grep -q "Disable Hidraw\|Disable Input" "$T/log" && fail "misspelled names (Wine ignores them)" || ok "the names winebus.sys reads"
grep -q "|reg|add|.*|Enable SDL|" "$T/log" && fail "overwrote the player's Enable SDL" || ok "an existing value is never overwritten"
grep -q "wineserver|-w|" "$T/log" && ok "the prefix shuts down after seeding" || fail "no wineserver -w"
grep -q "WINEPREFIX=$T/prefix|" "$T/log" && ok "the private prefix" || fail "not the private prefix"
contains "it says so once" "$(cat "$T/out")" "configuring Wine's controller bridge (once)"
[ -f "$T/prefix/.rea-winebus" ] && ok "default marker .rea-winebus" || fail "no marker"
: > "$T/log"
rea_seed_winebus > /dev/null
[ -s "$T/log" ] && fail "seeded twice" || ok "seeded once per prefix"
REA_WINEBUS_MARKER=.srr-winebus-v2
: > "$T/prefix/.srr-winebus-v2"
REA_PREFIX=$T/prefix rea_seed_winebus > /dev/null
[ -s "$T/log" ] && fail "the game's existing marker ignored" || ok "REA_WINEBUS_MARKER keeps SRR's marker"
finish
