#!/bin/sh
# Runs sdk-update.yml's shell steps as GitHub does (sh -e) against a local
# SDK repository, a launcher cloned from a local bare "origin", and fake gh
# and cargo (no network): sh scripts/tests/sdk-update.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd -P)
ROOT=$(cd "$HERE/../.." && pwd -P)
WF=$ROOT/.github/workflows/sdk-update.yml
T=$(mktemp -d "${TMPDIR:-/tmp}/rea-sdk-update-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }
check() { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1: want '$2', got '$3'"; fi; }
git() { command git -c user.name=test -c user.email=test@example.com -c init.defaultBranch=main "$@"; }
export GIT_CONFIG_NOSYSTEM=1 GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@example.com GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@example.com

# Prints the `run: |` block of step $1, unindented (as scripts/tests/workflows.sh).
step() {
  f=$T/$(printf '%s' "$1" | tr -c 'A-Za-z0-9' '_').sh
  awk -v want="$1" '
    function indent(s) { match(s, /^ */); return RLENGTH }
    !found { t = $0; sub(/^ *- name: /, "", t); if ($0 ~ /^ *- name: / && t == want) found = 1; next }
    found && !inrun { if ($0 ~ /^ *- (name|uses):/) exit; if ($0 ~ /^ *run: \|$/) { inrun = 1; ri = indent($0) }; next }
    inrun { if ($0 !~ /^ *$/ && indent($0) <= ri) exit; print substr($0, ri + 3) }
  ' "$WF" > "$f"
  [ -s "$f" ] || { echo "no run block for '$1'" >&2; exit 1; }
  echo "$f"
}
FIND=$(step "Find the SDK version")
BUMP=$(step "Bump on the update branch")
PR=$(step "Open or update the pull request")

# SDK with v0.1.0, v0.2.0 and v0.10.0 (numeric order, not text order).
S=$T/sdk
mkdir -p "$S/scripts" "$S/shell"
cp -R "$ROOT/packaging" "$S/packaging"
cp "$ROOT/scripts/sdk-sync" "$ROOT/scripts/sdk-bump" "$ROOT/scripts/wine-test" "$S/scripts/"
cl() { printf '# Changelog\n'; for v; do printf '\n## [%s] - 2026-11-02\n\n- **Migration:** item of %s\n' "$v" "$v"; done; }
echo v1 > "$S/shell/rea-play.sh"; cl 0.1.0 > "$S/CHANGELOG.md"
(cd "$S" && git init -q && git add -A && git commit -qm 1 && git tag v0.1.0) || exit 1
echo v2 > "$S/shell/rea-play.sh"; cl 0.2.0 0.1.0 > "$S/CHANGELOG.md"
(cd "$S" && git commit -qam 2 && git tag v0.2.0)
echo v10 > "$S/shell/rea-play.sh"; cl 0.10.0 0.2.0 0.1.0 > "$S/CHANGELOG.md"
(cd "$S" && git commit -qam 10 && git tag v0.10.0 && git tag not-a-version)

# Launcher at v0.1.0, pushed to a bare origin, cloned as the runner would.
W=$T/work
mkdir -p "$W/.github/workflows"
printf '[dependencies]\nrea-launcher = { git = "https://github.com/akitaonrails/rea-launcher-sdk", tag = "v0.1.0" }\n' > "$W/Cargo.toml"
printf 'jobs:\n  ci:\n    uses: akitaonrails/rea-launcher-sdk/.github/workflows/launcher-ci.yml@v0.1.0\n' > "$W/.github/workflows/build.yml"
(cd "$S" && git checkout -q v0.1.0) && sh "$S/scripts/sdk-sync" "$W" >/dev/null && (cd "$S" && git checkout -q main)
(cd "$W" && git init -q && git add -A && git commit -qm init) || exit 1
git clone -q --bare "$W" "$T/origin.git"
git clone -q "$T/origin.git" "$T/runner"
L=$T/runner

# Fakes.
mkdir -p "$T/bin" "$T/rt"
cat > "$T/bin/gh" <<'EOF'
#!/bin/sh
echo "gh $*" >> "$GH_LOG"
case "$1 $2" in
  "api repos/"*) echo main ;;
  "pr list") echo "${GH_PR:-}" ;;
  "pr create" | "pr edit") while [ $# -gt 0 ]; do [ "$1" = --body-file ] && cp "$2" "$GH_BODY"; shift; done ;;
esac
EOF
printf '#!/bin/sh\nexit 0\n' > "$T/bin/cargo"
chmod +x "$T/bin/gh" "$T/bin/cargo"
export PATH="$T/bin:$PATH" GH_LOG="$T/gh.log" GH_BODY="$T/body.md" RUNNER_TEMP="$T/rt" \
  GITHUB_REPOSITORY=me/game SDK_URL="$S" SDK_DIR=sdk BASE_INPUT='' VERSION_INPUT='' REA_SDK_CACHE="$T/cache"
out() { sed -n "s/^$1=//p" "$GITHUB_OUTPUT" | tail -n 1; }
run() { (cd "$L" && sh -e "$1") >"$T/out" 2>&1; }

export GITHUB_OUTPUT="$T/out1"; : > "$GITHUB_OUTPUT"
if run "$FIND"; then ok "find: runs"; else bad "find: $(cat "$T/out")"; fi
check "find: newest by number (v0.10.0 > v0.2.0), not text" "v0.10.0" "$(out latest)"
check "find: current from sdk/VERSION" "v0.1.0" "$(out current)"
check "find: newer" "1" "$(out newer)"

export LATEST=v0.10.0 CURRENT=v0.1.0 GITHUB_OUTPUT="$T/out2"; : > "$GITHUB_OUTPUT"
if run "$BUMP"; then ok "bump: runs"; else bad "bump: $(cat "$T/out")"; fi
check "bump: branch output" "sdk-update/v0.10.0" "$(out branch)"
pushed=$(git -C "$T/origin.git" log -1 --format=%s refs/heads/sdk-update/v0.10.0 2>/dev/null)
check "bump: commit pushed to sdk-update/v0.10.0" "build: bump rea-launcher-sdk to v0.10.0" "$pushed"
check "bump: base branch untouched" "init" "$(git -C "$T/origin.git" log -1 --format=%s refs/heads/main)"
git -C "$T/origin.git" show refs/heads/sdk-update/v0.10.0:sdk/VERSION | grep -qx 'tag=v0.10.0' && ok "bump: sdk/ re-vendored" || bad "bump: sdk/ re-vendored"
git -C "$T/origin.git" show refs/heads/sdk-update/v0.10.0:Cargo.toml | grep -q 'tag = "v0.10.0"' && ok "bump: Cargo tag moved" || bad "bump: Cargo tag moved"
git -C "$T/origin.git" show refs/heads/sdk-update/v0.10.0:.github/workflows/build.yml | grep -q 'launcher-ci.yml@v0.10.0' && ok "bump: workflow ref moved" || bad "bump: workflow ref moved"

export BRANCH=sdk-update/v0.10.0
if run "$PR"; then ok "pr: runs"; else bad "pr: $(cat "$T/out")"; fi
grep -q '^gh pr create --base main --head sdk-update/v0.10.0 --title build: bump rea-launcher-sdk to v0.10.0 --body-file ' "$GH_LOG" \
  && ok "pr: opened against the default branch" || bad "pr: $(cat "$GH_LOG")"
grep -q 'item of 0.2.0' "$GH_BODY" && grep -q 'item of 0.10.0' "$GH_BODY" && ! grep -q 'item of 0.1.0' "$GH_BODY" \
  && ok "pr: body has the Migration items after v0.1.0" || bad "pr body: $(cat "$GH_BODY")"
if grep -q 'merge' "$GH_LOG"; then bad "pr: never merges"; else ok "pr: never merges"; fi

# A second run (the branch exists): nothing new pushed, PR body updated with
# the same items (counted from the base's version, not the branch's).
: > "$GH_LOG"; rm -f "$GH_BODY"
before=$(git -C "$T/origin.git" rev-parse refs/heads/sdk-update/v0.10.0)
(cd "$L" && git checkout -q main)
export GITHUB_OUTPUT="$T/out3"; : > "$GITHUB_OUTPUT"
if run "$BUMP" && grep -q 'already up to date' "$T/out"; then ok "rerun: branch already up to date"; else bad "rerun: $(cat "$T/out")"; fi
check "rerun: nothing pushed" "$before" "$(git -C "$T/origin.git" rev-parse refs/heads/sdk-update/v0.10.0)"
GH_PR=7 run "$PR"
grep -q '^gh pr edit 7 --body-file' "$GH_LOG" && ! grep -q 'pr create' "$GH_LOG" && ok "rerun: existing PR updated" || bad "rerun PR: $(cat "$GH_LOG")"
grep -q 'item of 0.2.0' "$GH_BODY" && ok "rerun: Migration items kept" || bad "rerun body: $(cat "$GH_BODY" 2>/dev/null)"

# Up to date: nothing to do. A pinned version wins over the newest.
(cd "$L" && git checkout -q sdk-update/v0.10.0)
export GITHUB_OUTPUT="$T/out4"; : > "$GITHUB_OUTPUT"
run "$FIND"; check "up to date: not newer" "0" "$(out newer)"
(cd "$L" && git checkout -q main)
export GITHUB_OUTPUT="$T/out5"; : > "$GITHUB_OUTPUT"
VERSION_INPUT=v0.2.0 run "$FIND"; check "version input wins" "v0.2.0" "$(out latest)"
export GITHUB_OUTPUT="$T/out6"; : > "$GITHUB_OUTPUT"
if VERSION_INPUT='v1;rm' run "$FIND"; then bad "unsafe version refused"; else ok "unsafe version refused"; fi

[ "$fails" = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
