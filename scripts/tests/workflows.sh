#!/bin/sh
# Runs launcher-ci.yml's shell steps as GitHub does (sh -e) against fixture
# trees, so a check that cannot fail is caught: sh scripts/tests/workflows.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd -P)
WF=$HERE/../../.github/workflows
T=$(mktemp -d "${TMPDIR:-/tmp}/rea-workflows-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }

# Prints the `run: |` block of the step named $2 in workflow $1, unindented.
extract() {
  awk -v want="$2" '
    function indent(s) { match(s, /^ */); return RLENGTH }
    !found { t = $0; sub(/^ *- name: /, "", t); if ($0 ~ /^ *- name: / && t == want) found = 1; next }
    found && !inrun { if ($0 ~ /^ *- (name|uses):/) exit; if ($0 ~ /^ *run: \|$/) { inrun = 1; ri = indent($0) }; next }
    inrun { if ($0 !~ /^ *$/ && indent($0) <= ri) exit; print substr($0, ri + 3) }
  ' "$1"
}
step() { # workflow step-name -> $T/<slug>.sh, non-empty
  f=$T/$(printf '%s' "$2" | tr -c 'A-Za-z0-9' '_').sh
  extract "$WF/$1" "$2" > "$f"
  [ -s "$f" ] || { echo "no run block for '$2' in $1" >&2; return 1; }
  echo "$f"
}
passes() { if (cd "$T/w" && sh -e "$S" >"$T/out" 2>&1); then ok "$1"; else bad "$1: $(cat "$T/out")"; fi; }
fails_() { if (cd "$T/w" && sh -e "$S" >"$T/out" 2>&1); then bad "$1 (step passed)"; else ok "$1"; fi; }

mkdir -p "$T/w/linux/tests" "$T/w/packaging"
printf '#!/bin/sh\necho fine\n' > "$T/w/linux/game-play"
printf '#!/bin/sh\nexit 0\n' > "$T/w/packaging/build.sh"

S=$(step launcher-ci.yml "Shell syntax") || exit 1
export GLOBS='linux/*-play packaging/*.sh missing/*.sh'
passes "shell syntax: valid scripts pass, unmatched globs skipped"
printf 'if then fi (\n' > "$T/w/packaging/broken.sh"
fails_ "shell syntax: a syntax error fails the step"
rm "$T/w/packaging/broken.sh"

S=$(step launcher-ci.yml "Shell tests") || exit 1
printf '#!/bin/sh\necho ok\n' > "$T/w/linux/tests/unit.sh"
printf '#!/bin/sh\necho "needs a dev kit" >&2; exit 2\n' > "$T/w/linux/tests/e2e.sh"
export GLOBS='linux/tests/*.sh packaging/tests/*.sh' SKIP_PATTERNS='*e2e*'
passes "shell tests: *e2e* skipped by default"
grep -q '^skip linux/tests/e2e.sh$' "$T/out" && ok "shell tests: the skip is reported" || bad "shell tests: the skip is reported"
export SKIP_PATTERNS=''
fails_ "shell tests: a failing test fails the step"
export SKIP_PATTERNS='*e2e*'
printf '#!/bin/sh\nexit 1\n' > "$T/w/linux/tests/unit.sh"
fails_ "shell tests: a failing unit test fails the step"
# The skip patterns must not glob against the working folder.
printf '#!/bin/sh\necho ok\n' > "$T/w/linux/tests/unit.sh"; : > "$T/w/xe2ex"
passes "shell tests: skip patterns are not expanded as file globs"

S=$(step launcher-ci.yml "Dependency advisories, licences and sources") || exit 1
rm -f "$T/w/deny.toml"
if (cd "$T/w" && sh -e "$S" >"$T/out" 2>&1); then bad "audit without deny.toml fails"
elif grep -q 'templates/deny.toml' "$T/out"; then ok "audit without deny.toml fails with the template hint"
else bad "audit without deny.toml: $(cat "$T/out")"; fi

# No status-swallowing "check && echo" anywhere in the workflows.
if grep -n '&& echo' "$WF"/*.yml; then bad "no '&& echo' checks in workflows"; else ok "no '&& echo' checks in workflows"; fi

[ "$fails" = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
