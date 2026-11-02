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
finish
