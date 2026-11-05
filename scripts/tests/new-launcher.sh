#!/bin/sh
# Tests scripts/new-launcher: generates launchers into a temp folder and
# checks the file set, the filled placeholders, the refusals, and the
# generated repository's own shell tests. No network, no builds (--no-lock):
#   sh scripts/tests/new-launcher.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd -P)
ROOT=$(cd "$HERE/../.." && pwd -P)
NEW=$ROOT/scripts/new-launcher
T=$(mktemp -d "${TMPDIR:-/tmp}/rea-new-launcher-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }
has() { # name file text
  if grep -qF -- "$3" "$2" 2>/dev/null; then ok "$1"; else bad "$1: $2 lacks '$3'"; fi
}
export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.com \
  GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.com
VERSION=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version *= *"\(.*\)"/\1/p' "$ROOT/Cargo.toml")
TAG=$(git -C "$ROOT" describe --tags --exact-match HEAD 2>/dev/null || echo "v$VERSION")

L=$T/rea-example-rally
if sh "$NEW" --id exr --prefix EXR_ --game "Example Rally: Driver's Cut" --exe "Example Rally.exe" \
  --author "Jane Doe" --repo someone/rea-exr --no-lock --out "$L" > "$T/out" 2>&1; then
  ok "generates a launcher"
else
  bad "generates a launcher: $(cat "$T/out")"
fi

# --- the file set --------------------------------------------------------------
cat > "$T/want" <<'EOF'
.cargo/config.toml
.github/ISSUE_TEMPLATE/bug-report.yml
.github/ISSUE_TEMPLATE/config.yml
.github/ISSUE_TEMPLATE/unknown-edition.yml
.github/workflows/build.yml
.github/workflows/sdk-update.yml
.github/workflows/watch-upstream.yml
.gitignore
AGENTS.md
CHANGELOG.md
CLAUDE.md
Cargo.toml
LICENSE-APACHE
LICENSE-MIT
README.md
THIRD_PARTY.md
crates/exr-launcher/Cargo.toml
crates/exr-launcher/build.rs
crates/exr-launcher/assets/README.md
crates/exr-launcher/src/cli.rs
crates/exr-launcher/src/config.rs
crates/exr-launcher/src/edition.rs
crates/exr-launcher/src/form.rs
crates/exr-launcher/src/lib.rs
crates/exr-launcher/src/main.rs
crates/exr-launcher/src/palette.rs
crates/exr-launcher/src/runtime.rs
crates/exr-launcher/src/ui.rs
crates/exr-launcher/tests/cli.rs
crates/exr-launcher/tests/edition.rs
crates/exr-launcher/tests/snapshots.rs
crates/exr-launcher/tests/snapshots/first-run-2560x1440.cfg.txt
crates/exr-launcher/tests/snapshots/help.txt
crates/exr-launcher/tests/snapshots/launcher.cfg.txt
crates/exr-launcher/tests/snapshots/looks.txt
deny.toml
docs/CONTRIBUTIONS.md
docs/RESEARCH.md
docs/RUNTIME_NOTES.md
linux/README.md
linux/exr-play
linux/tests/play.sh
packaging/appimage/exr-launcher.desktop
packaging/appimage/exr-launcher.png
packaging/licenses/Barlow-OFL.txt
packaging/package.allow
packaging/package.sh
packaging/tests/package.sh
packaging/third-party.txt
packaging/upstream-watch.txt
EOF
(cd "$L" && find . -path ./sdk -prune -o -type f -print | sed 's|^\./||' | LC_ALL=C sort) > "$T/have"
if LC_ALL=C sort "$T/want" | cmp -s - "$T/have"; then
  ok "the repository shape (STANDARDS) and nothing else"
else
  bad "file set differs: $(LC_ALL=C sort "$T/want" | diff - "$T/have" | grep '^[<>]' | tr '\n' ' ')"
fi
[ -f "$L/sdk/VERSION" ] && [ -f "$L/sdk/rea-play.sh" ] && [ -x "$L/sdk/scripts/wine-test" ] \
  && ok "sdk/ vendored" || bad "sdk/ vendored"
if sh "$ROOT/scripts/sdk-sync" --check "$L" >/dev/null 2>&1; then ok "sdk-sync --check passes"; else bad "sdk-sync --check"; fi
[ ! -e "$L/Cargo.lock" ] && ok "--no-lock: no Cargo.lock" || bad "--no-lock: no Cargo.lock"
[ ! -e "$L/.git" ] && ok "no git repository without --git" || bad "no git repository without --git"

# --- placeholders -----------------------------------------------------------------
left=$(cd "$L" && find . -path ./sdk -prune -o -type f ! -name '*.png' -exec grep -lE '@[A-Z_]+@' {} + || true)
[ -z "$left" ] && ok "every placeholder filled" || bad "placeholders left in: $left"
odd=$(cd "$L" && find . -name '*__ID__*' -o -name 'dot-*' -o -name '*.tmpl')
[ -z "$odd" ] && ok "template names translated" || bad "template names left: $odd"
for f in linux/exr-play packaging/package.sh linux/tests/play.sh packaging/tests/package.sh; do
  [ -x "$L/$f" ] && ok "executable: $f" || bad "executable: $f"
done
has "SDK crates pinned to the checkout's tag" "$L/Cargo.toml" \
  "rea-launcher-ui = { git = \"https://github.com/akitaonrails/rea-launcher-sdk\", tag = \"$TAG\" }"
has "crate name" "$L/crates/exr-launcher/Cargo.toml" 'name = "exr-launcher"'
has "launcher program" "$L/crates/exr-launcher/Cargo.toml" 'name = "EXRLauncher"'
has "game title" "$L/crates/exr-launcher/src/ui.rs" "Example Rally: Driver's Cut"
has "game exe" "$L/crates/exr-launcher/src/edition.rs" 'pub const GAME_EXE: &str = "Example Rally.exe";'
has "unknown-edition issue link from --repo" "$L/crates/exr-launcher/src/edition.rs" \
  "https://github.com/someone/rea-exr/issues/new?template=unknown-edition.yml"
has "lib name used in main.rs" "$L/crates/exr-launcher/src/main.rs" "use exr_launcher::"
has "CI workflow at the tag" "$L/.github/workflows/build.yml" \
  "uses: akitaonrails/rea-launcher-sdk/.github/workflows/launcher-ci.yml@$TAG"
has "release workflow at the tag" "$L/.github/workflows/build.yml" \
  "uses: akitaonrails/rea-launcher-sdk/.github/workflows/launcher-release.yml@$TAG"
has "Wine tests on" "$L/.github/workflows/build.yml" "wine-tests: true"
has "release package" "$L/.github/workflows/build.yml" "package: exr-launcher"
has "licence owner" "$L/LICENSE-MIT" "Jane Doe and contributors"
has "deny.toml from the SDK template" "$L/deny.toml" 'allow-git = ["https://github.com/akitaonrails/rea-launcher-sdk"]'
has "helper app name" "$L/linux/exr-play" "REA_APP=exr-launcher"
has "helper env prefix (trailing _ dropped)" "$L/linux/exr-play" 'REA_EXE=${EXR_LAUNCHER:-}'
has "helper follows the test convention" "$L/linux/exr-play" '[ "$_fonly" = 1 ] && return 0'
has "AppImage env prefix" "$L/packaging/package.sh" "--env-prefix EXR"
has "allowlist" "$L/packaging/package.allow" "linux/exr-play x"
has "issue template asks for the SHA-256" "$L/.github/ISSUE_TEMPLATE/bug-report.yml" "SHA-256 of Example Rally.exe"
has "AGENTS points at the vendored standards" "$L/AGENTS.md" "Read \`sdk/docs/STANDARDS.md\` before any work"
has "AGENTS points at the vendored playbooks" "$L/AGENTS.md" "sdk/docs/playbooks/"
has "AGENTS keeps the real-game test pointer" "$L/AGENTS.md" '"Running the real'
has "AGENTS: everything outside the repository is read-only" "$L/AGENTS.md" "Everything outside this repository is the user's"
if grep -q '^## Non-negotiables\|^## The owner' "$L/AGENTS.md"; then bad "AGENTS has no copied rule sections"; else ok "AGENTS has no copied rule sections"; fi
n=$(awk '/^## Always/{f=1;next} f&&/^## /{exit} f&&/^[-] |^  /{n++} END{print n+0}' "$L/AGENTS.md")
[ "$n" -le 12 ] && ok "Always block is short ($n lines)" || bad "Always block has $n lines"
[ "$(cat "$L/CLAUDE.md")" = "$(printf '@AGENTS.md\n@sdk/docs/STANDARDS.md')" ] && ok "CLAUDE.md is exactly the two imports" || bad "CLAUDE.md: $(cat "$L/CLAUDE.md")"
[ -s "$L/sdk/docs/STANDARDS.md" ] && [ -n "$(ls "$L"/sdk/docs/playbooks/*.md 2>/dev/null)" ] \
  && ok "STANDARDS and playbooks vendored under sdk/docs" || bad "STANDARDS and playbooks vendored under sdk/docs"
has "README routes shared code to the SDK" "$L/README.md" "https://github.com/akitaonrails/rea-launcher-sdk/issues"
has "weekly SDK update at the tag" "$L/.github/workflows/sdk-update.yml" \
  "uses: akitaonrails/rea-launcher-sdk/.github/workflows/sdk-update.yml@$TAG"
has "sdk/VERSION names the pinned tag" "$L/sdk/VERSION" "tag=$TAG"
has "the window title is not the game's" "$L/crates/exr-launcher/src/lib.rs" \
  "pub const WINDOW_TITLE: &str = \"Example Rally: Driver's Cut Launcher\";"
# launcher-ci runs the vendored copy (sdk/scripts/launcher-check).
if [ -x "$L/sdk/scripts/launcher-check" ]; then ok "launcher-check is vendored and executable"; else bad "sdk/scripts/launcher-check missing or not executable"; fi
# The family's rules (scripts/launcher-check): a fresh scaffold has no errors.
if sh "$ROOT/scripts/launcher-check" "$L" > "$T/check" 2>&1; then
  ok "launcher-check: no errors in a fresh scaffold"
else
  bad "launcher-check on a fresh scaffold: $(grep '^error' "$T/check" | tr '\n' ' ')"
fi

# --- the generated repository's own shell tests --------------------------------
for t in "$L"/linux/tests/*.sh "$L"/packaging/tests/*.sh; do
  if sh "$t" > "$T/out" 2>&1; then ok "generated test ${t#"$L"/} passes"; else bad "generated ${t#"$L"/}: $(tail -5 "$T/out")"; fi
done
for f in "$L/linux/exr-play" "$L/packaging/package.sh"; do
  if sh -n "$f"; then ok "sh -n ${f#"$L"/}"; else bad "sh -n ${f#"$L"/}"; fi
done

# --- defaults ---------------------------------------------------------------------
if sh "$NEW" --id screamer-two --prefix SCR2 --game "Screamer 2" --exe SCREAMER.EXE --no-lock \
  --author "Jane Doe" --git --out "$T/rea-screamer-2" > "$T/out" 2>&1; then
  ok "a second launcher, with --git"
else
  bad "second launcher: $(cat "$T/out")"
fi
has "long ids: capitalised launcher name" "$T/rea-screamer-2/crates/screamer-two-launcher/Cargo.toml" 'name = "ScreamerTwoLauncher"'
has "default --repo: SDK owner / folder name" "$T/rea-screamer-2/crates/screamer-two-launcher/src/edition.rs" \
  "https://github.com/akitaonrails/rea-screamer-2/issues"
if [ "$(git -C "$T/rea-screamer-2" log --format=%s 2>/dev/null)" = "chore: scaffold from rea-launcher-sdk $TAG" ] \
  && [ -z "$(git -C "$T/rea-screamer-2" status --porcelain)" ]; then
  ok "--git commits everything"
else
  bad "--git commits everything"
fi
if sh "$NEW" --id cmr3 --prefix CMR3 --game "Colin McRae Rally 3" --exe Rally_3PC.exe --no-lock \
  --author A --launcher-exe RallyLauncher.exe --out "$T/c" > /dev/null 2>&1; then
  has "--launcher-exe" "$T/c/linux/cmr3-play" "REA_EXE_NAME=RallyLauncher.exe"
else
  bad "--launcher-exe"
fi

# --- refusals --------------------------------------------------------------------
refuse() { # name expected-message args...
  name=$1 want=$2; shift 2
  if sh "$NEW" "$@" > "$T/out" 2>&1; then
    bad "$name: accepted"
  elif grep -qF -- "$want" "$T/out"; then
    ok "$name"
  else
    bad "$name: $(cat "$T/out")"
  fi
}
base='--prefix X --game Game --exe G.exe --author A --no-lock'
# shellcheck disable=SC2086
{
  refuse "an upper-case id" "--id must start" --id Bad $base --out "$T/r1"
  refuse "a bad id" "--id takes" --id 'a_b' $base --out "$T/r2"
  refuse "a bad prefix" "--prefix" --id ok --prefix 'x-y' --game G --exe G.exe --author A --out "$T/r3"
  refuse "a quote in the title" "--game may not contain" --id ok --prefix X --game 'Say "hi"' --exe G.exe --author A --out "$T/r4"
  refuse "an exe that is not .exe" "--exe must name a .exe" --id ok --prefix X --game G --exe game.bat --author A --out "$T/r5"
  refuse "a bad repo" "--repo must be OWNER/NAME" --id ok $base --repo nope --out "$T/r6"
  refuse "missing arguments" "are required" --id ok
  refuse "an unknown flag" "unknown argument" --id ok $base --frob --out "$T/r7"
  refuse "a non-empty folder" "is not empty" --id exr $base --out "$L"
  refuse "inside the SDK" "inside the SDK" --id ok $base --out "$ROOT/templates/should-not-exist"
}
[ ! -e "$ROOT/templates/should-not-exist" ] || { rm -rf "$ROOT/templates/should-not-exist"; bad "left a folder in the SDK"; }
if sh "$NEW" --help | grep -q -- "--out DIR"; then ok "--help"; else bad "--help"; fi

if [ "$fails" = 0 ]; then echo "all passed"; exit 0; fi
echo "$fails failed"; exit 1
