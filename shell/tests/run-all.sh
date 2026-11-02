#!/bin/sh
# Runs every library test under each shell available: sh, dash, busybox sh
# (or the shells named in REA_TEST_SHELLS), plus `sh -n` on the library.
#   sh shell/tests/run-all.sh
set -u
TESTS=$(dirname "$(readlink -f "$0")")
shells=${REA_TEST_SHELLS:-}
if [ -z "$shells" ]; then
  shells=sh
  command -v dash >/dev/null 2>&1 && shells="$shells dash"
  command -v busybox >/dev/null 2>&1 && shells="$shells busybox-sh"
fi
failed=0
for s in $shells; do
  case $s in busybox-sh) run="busybox sh" ;; *) run=$s ;; esac
  if $run -n "$TESTS/../rea-play.sh"; then echo "ok   [$s] rea-play.sh parses"; else echo "FAIL [$s] rea-play.sh syntax"; failed=$((failed + 1)); fi
  for t in "$TESTS"/*.sh; do
    case ${t##*/} in common.sh|run-all.sh) continue ;; esac
    if out=$($run "$t" 2>&1); then
      echo "ok   [$s] ${t##*/} ($(printf '%s\n' "$out" | grep -c '^ok') checks)"
    else
      echo "FAIL [$s] ${t##*/}"; printf '%s\n' "$out" | grep -v '^ok' | sed 's/^/     /'
      failed=$((failed + 1))
    fi
  done
done
[ "$failed" = 0 ] && echo "all suites passed under: $shells" || { echo "$failed suite(s) failed"; exit 1; }
