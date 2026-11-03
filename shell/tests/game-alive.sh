#!/bin/sh
# rea_game_alive: a zombie or vanished game process is not running:
# sh shell/tests/game-alive.sh (from SRR game-alive.sh).
. "$(dirname "$(readlink -f "$0")")/common.sh"
REA_APP=test-launcher REA_PREFIX=$T/prefix
init_lib
mkdir -p "$T/bin" "$T/proc/4242"
cat > "$T/bin/pgrep" <<'FAKE'
#!/bin/sh
[ "$2" = "SEGA Rally_SSE1" ] && echo 4242
exit 0
FAKE
chmod +x "$T/bin/pgrep"
PATH=$T/bin:$PATH REA_PROC=$T/proc REA_GAME_PROCS='SEGA Rally.exe|SEGA Rally_SSE1'
alive() { if rea_game_alive; then echo alive; else echo gone; fi; }
echo '4242 (SEGA Rally_SSE1) Z 1 4242' > "$T/proc/4242/stat"
check "a zombie is not running" "$(alive)" gone
echo '4242 (SEGA Rally_SSE1) S 1 4242' > "$T/proc/4242/stat"
check "a sleeping game process is" "$(alive)" alive
echo '4242 (a) b) (c) R 1 4242' > "$T/proc/4242/stat"
check "a name with ') ' still finds the state" "$(alive)" alive
rm "$T/proc/4242/stat"
check "a vanished process is not" "$(alive)" gone
REA_GAME_PROCS=
check "no names: nothing to wait for" "$(alive)" gone

echo "# only this prefix's game (CMR3: another install's copy is not ours)"
REA_GAME_PROCS='SEGA Rally_SSE1'
echo '4242 (SEGA Rally_SSE1) S 1 4242' > "$T/proc/4242/stat"
environ() { printf '%s\000' "$@" > "$T/proc/4242/environ"; }
mkdir -p "$T/other"; ln -s "$T/prefix" "$T/prefix-link"
check "the prefix is canonical" "$REA_GAME_PREFIX" "$T/prefix"
environ HOME=/h "WINEPREFIX=$T/prefix" SECRET=do-not-print
check "our prefix" "$(alive)" alive
environ "WINEPREFIX=$T/prefix/"
check "our prefix with a trailing slash" "$(alive)" alive
environ "WINEPREFIX=$T/prefix-link"
check "our prefix through a link" "$(alive)" alive
environ "WINEPREFIX=$T/other" HOME=/h
check "another prefix's copy is not ours" "$(alive)" gone
environ "WINECONFIGDIR=$T/prefix"
check "Wine's own WINECONFIGDIR counts" "$(alive)" alive
environ "WINEPREFIX=$T/other" "WINECONFIGDIR=$T/prefix"
check "either names our prefix" "$(alive)" alive
environ HOME=/h
check "no prefix in the environment: still counted (as before)" "$(alive)" alive
rm "$T/proc/4242/environ"
check "an unreadable environment: still counted" "$(alive)" alive
environ "WINEPREFIX=$T/other"
out=$(rea_game_alive 2>&1)
check "nothing printed, not even the environment" "$out" ""
_pfx=$REA_GAME_PREFIX REA_GAME_PREFIX=
check "native mode (no prefix): every process counts" "$(alive)" alive
REA_GAME_PREFIX=$_pfx
echo "# the inner script carries the same check and the prefix"
rea_write_inner
contains "inner script has rea_game_ours" "$(cat "$REA_INNER")" "rea_game_ours()"
finish
