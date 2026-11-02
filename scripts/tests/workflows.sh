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

# mirror.yml: only pins marked mirror, verified, named by the pin; the
# release is created when missing, never deleted, assets clobbered.
M=$T/m
mkdir -p "$M/up" "$M/repo/sdk" "$M/bin" "$M/rt"
cp -R "$HERE/../../packaging" "$M/repo/sdk/packaging"
echo xidi > "$M/up/Xidi-v4.3.1.zip"; echo dxvk > "$M/up/dxvk.tar.gz"; echo hd > "$M/up/hd.zip"
sha() { sha256sum "$1" | cut -d' ' -f1; }
cat > "$M/repo/pins.txt" <<PINS
xidi file://$M/up/Xidi-v4.3.1.zip $(sha "$M/up/Xidi-v4.3.1.zip") mirror https://github.com/me/game/releases/download/thirdparty/Xidi-v4.3.1.zip
dxvk file://$M/up/dxvk.tar.gz $(sha "$M/up/dxvk.tar.gz") mirror
hd file://$M/up/hd.zip $(sha "$M/up/hd.zip")
PINS
cat > "$M/bin/gh" <<'GH'
#!/bin/sh
echo "gh $*" >> "$GH_LOG"
[ "$1 $2" = "release view" ] && [ -z "${GH_HAS_RELEASE:-}" ] && exit 1
exit 0
GH
chmod +x "$M/bin/gh"
S=$(step mirror.yml "Fetch and verify every mirrorable pin") || exit 1
S2=$(step mirror.yml "Upload to the mirror release") || exit 1
mirror_env() { export PINS=pins.txt TAG=thirdparty SDK_DIR=sdk RUNNER_TEMP="$M/rt" GITHUB_REPOSITORY=me/game GH_LOG="$M/gh.log" PATH="$M/bin:$PATH"; }
if (cd "$M/repo" && mirror_env && sh -e "$S") >"$T/out" 2>&1; then ok "mirror: fetch step passes"; else bad "mirror: fetch step: $(cat "$T/out")"; fi
[ "$(ls "$M/rt/mirror" | LC_ALL=C sort | tr '\n' ' ')" = "Xidi-v4.3.1.zip dxvk.tar.gz " ] && ok "mirror: only marked pins, under the pin's file name" || bad "mirror: files: $(ls "$M/rt/mirror")"
grep -q 'warning.*dxvk: add the mirror url as a fallback: https://github.com/me/game/releases/download/thirdparty/dxvk.tar.gz' "$T/out" \
  && ok "mirror: warns when a pin lacks its mirror url" || bad "mirror: missing-fallback warning"
if grep -q 'xidi: add the mirror url' "$T/out"; then bad "mirror: no warning when the fallback is listed"; else ok "mirror: no warning when the fallback is listed"; fi
if (cd "$M/repo" && mirror_env && sh -e "$S2") >"$T/out" 2>&1; then ok "mirror: upload step passes"; else bad "mirror: upload step: $(cat "$T/out")"; fi
grep -q '^gh release create thirdparty --repo me/game --latest=false' "$M/gh.log" && ok "mirror: creates the missing release, not as latest" || bad "mirror: create: $(cat "$M/gh.log")"
up=$(grep '^gh release upload thirdparty ' "$M/gh.log")
case $up in *" --repo me/game --clobber") ;; *) up='' ;; esac
case $up in *"$M/rt/mirror/Xidi-v4.3.1.zip"*) ;; *) up='' ;; esac
case $up in *"$M/rt/mirror/dxvk.tar.gz"*) ;; *) up='' ;; esac
[ -n "$up" ] && ok "mirror: uploads with --clobber" || bad "mirror: upload: $(cat "$M/gh.log")"
: > "$M/gh.log"
(cd "$M/repo" && mirror_env && GH_HAS_RELEASE=1 && export GH_HAS_RELEASE && sh -e "$S2") >/dev/null 2>&1
if grep -q 'release create' "$M/gh.log"; then bad "mirror: existing release reused"; else ok "mirror: existing release reused"; fi
if grep -q 'delete' "$M/gh.log"; then bad "mirror: never deletes"; else ok "mirror: never deletes"; fi
# A file that no longer matches its pin is never uploaded.
echo tampered > "$M/up/dxvk.tar.gz"; rm -rf "$M/rt/mirror"
if (cd "$M/repo" && mirror_env && sh -e "$S") >"$T/out" 2>&1; then bad "mirror: a sha256 mismatch fails"; else ok "mirror: a sha256 mismatch fails"; fi
echo dxvk > "$M/up/dxvk.tar.gz"
# Two pins sharing an asset name are refused.
echo "dup file://$M/up/dup/Xidi-v4.3.1.zip $(sha "$M/up/Xidi-v4.3.1.zip") mirror" >> "$M/repo/pins.txt"
if (cd "$M/repo" && mirror_env && sh -e "$S") >"$T/out" 2>&1; then bad "mirror: duplicate asset name refused"
elif grep -q 'is used by another pin' "$T/out"; then ok "mirror: duplicate asset name refused"; else bad "mirror: duplicate: $(cat "$T/out")"; fi

# No status-swallowing "check && echo" anywhere in the workflows.
if grep -n '&& echo' "$WF"/*.yml; then bad "no '&& echo' checks in workflows"; else ok "no '&& echo' checks in workflows"; fi

[ "$fails" = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
