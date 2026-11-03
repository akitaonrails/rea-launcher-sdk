#!/bin/sh
# Runs the real game: only with FX_E2E=1, headless.
if [ "${FX_E2E:-}" != 1 ]; then echo "skipped: set FX_E2E=1"; exit 0; fi
pid=$(pgrep -x fx.exe)
ps -o pid,stat,comm -p "$pid"
grep -qz "WINEPREFIX=$HOME/prefix" "/proc/$pid/environ" && echo ours
env -i PATH=/usr/bin:/bin distrobox enter gaming -- gamescope --backend headless -- fx-play
