#!/bin/sh
# Tests scripts/dev-check in a fake SDK tree with a restricted PATH (it never
# runs the real suites, so no recursion): sh scripts/tests/dev-check.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd -P)
T=$(mktemp -d "${TMPDIR:-/tmp}/rea-dev-check-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }
has_line() { if grep -qxF -- "$2" "$3"; then ok "$1"; else bad "$1: no '$2' in $(cat "$3")"; fi; }

SH=$(command -v sh)
F=$T/sdk
mkdir -p "$F/scripts/tests" "$F/shell/tests" "$F/packaging/tests" "$T/min" "$T/full"
cp "$HERE/../dev-check" "$F/scripts/"
printf 'exit 0\n' > "$F/shell/tests/run-all.sh"
printf 'exit 0\n' > "$F/packaging/tests/good.sh"
printf 'exit 1\n' > "$F/packaging/tests/broken.sh"
# Minimal PATH: the shell, a few utilities, and a cargo that always passes.
for u in sh dirname grep; do ln -s "$(command -v "$u")" "$T/min/$u"; done
printf '#!%s\nexit 0\n' "$SH" > "$T/min/cargo"
chmod +x "$T/min/cargo"
# Full PATH: plus every optional tool, stubbed.
cp -P "$T/min/"* "$T/full/"
for u in dash wine i686-w64-mingw32-gcc ld.lld cargo-deny cargo-audit shellcheck actionlint gitleaks; do
  printf '#!%s\nexit 0\n' "$SH" > "$T/full/$u"; chmod +x "$T/full/$u"
done

PATH=$T/min "$SH" "$F/scripts/dev-check" --list >"$T/out" 2>"$T/err"
has_line "runs fmt" "RUN  cargo fmt" "$T/out"
has_line "runs the shell suites" "RUN  shell suites (shell/tests/run-all.sh)" "$T/out"
has_line "skips i686 clippy without MinGW" "SKIP cargo clippy (i686 Windows): i686-w64-mingw32-gcc missing" "$T/err"
has_line "skips dash runs without dash" "SKIP packaging and scripts tests (dash): dash not installed" "$T/err"
has_line "skips wine-test without Wine" "SKIP wine-test: wine not installed" "$T/err"
has_line "skips actionlint" "SKIP actionlint: actionlint not installed" "$T/err"
has_line "skips cargo deny" "SKIP cargo deny: cargo-deny not installed" "$T/err"
has_line "skips the secret scan without gitleaks" "SKIP secret scan: gitleaks not installed (CI runs the pinned one)" "$T/err"

PATH=$T/full "$SH" "$F/scripts/dev-check" --list >"$T/out" 2>"$T/err"
for c in "secret scan (prepush-scan)" "cargo clippy (i686 Windows)" "cargo deny" "packaging and scripts tests (dash)" "shellcheck" "actionlint" "wine-test (i686)"; do
  has_line "runs $c when its tools exist" "RUN  $c" "$T/out"
done
[ ! -s "$T/err" ] && ok "nothing skipped with every tool" || bad "nothing skipped with every tool: $(cat "$T/err")"

# A real run: a failing check fails the run, skips do not.
if PATH=$T/min "$SH" "$F/scripts/dev-check" >"$T/out" 2>"$T/err"; then bad "a failing check exits 1"; else ok "a failing check exits 1"; fi
has_line "the failure is in the summary" "  packaging and scripts tests (sh)" "$T/out"
grep -q '^skipped' "$T/out" && ok "skips are in the summary" || bad "skips are in the summary"
rm "$F/packaging/tests/broken.sh"
if PATH=$T/min "$SH" "$F/scripts/dev-check" >"$T/out" 2>"$T/err"; then ok "only skips: exits 0"; else bad "only skips: exits 0: $(cat "$T/out")"; fi
if "$SH" "$F/scripts/dev-check" --bogus >/dev/null 2>&1; then bad "unknown option refused"; else ok "unknown option refused"; fi

[ "$fails" = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
