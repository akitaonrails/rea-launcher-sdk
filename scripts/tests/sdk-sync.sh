#!/bin/sh
# Tests scripts/sdk-sync against temp repositories: sh scripts/tests/sdk-sync.sh
# Runs a committed, tagged copy of the SDK files so the stamp is real.
set -u
HERE=$(cd "$(dirname "$0")" && pwd -P)
ROOT=$(cd "$HERE/../.." && pwd -P)
T=$(mktemp -d "${TMPDIR:-/tmp}/rea-sdk-sync-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }
git() { command git -c user.name=test -c user.email=test@example.com -c init.defaultBranch=main "$@"; }

S=$T/sdk-src
mkdir -p "$S/scripts" "$S/shell"
cp -R "$ROOT/packaging" "$S/packaging"
mkdir -p "$S/docs/playbooks"
echo '# Standards' > "$S/docs/STANDARDS.md"
echo '# Playbook A' > "$S/docs/playbooks/a.md"
echo '# Playbook B' > "$S/docs/playbooks/b.md"
echo '# Design, not vendored' > "$S/docs/DESIGN.md"
cp "$ROOT/scripts/sdk-sync" "$ROOT/scripts/wine-test" "$ROOT/scripts/sdk-bump" "$ROOT/scripts/clean" "$S/scripts/"
printf '# play library\n' > "$S/shell/rea-play.sh"
(cd "$S" && git init -q && git add -A && git commit -qm sdk && git tag v9.9.9) || { echo "git setup failed"; exit 1; }
SYNC=$S/scripts/sdk-sync
COMMIT=$(git -C "$S" rev-parse HEAD)

L=$T/launcher
mkdir -p "$L"
if sh "$SYNC" "$L" >/dev/null 2>"$T/err"; then ok "sync into a fresh launcher"; else bad "sync: $(cat "$T/err")"; fi
grep -qx 'tag=v9.9.9' "$L/sdk/VERSION" && grep -qx "commit=$COMMIT" "$L/sdk/VERSION" \
  && ok "VERSION has tag and commit" || bad "VERSION: $(cat "$L/sdk/VERSION")"
if cmp -s "$S/shell/rea-play.sh" "$L/sdk/rea-play.sh"; then ok "play library vendored as sdk/rea-play.sh"; else bad "play library vendored as sdk/rea-play.sh"; fi
[ ! -e "$L/sdk/shell" ] && ok "no sdk/shell folder" || bad "no sdk/shell folder"
[ -x "$L/sdk/scripts/wine-test" ] && cmp -s "$S/scripts/wine-test" "$L/sdk/scripts/wine-test" \
  && ok "vendored scripts/wine-test (executable)" || bad "vendored scripts/wine-test (executable)"
[ -x "$L/sdk/scripts/sdk-bump" ] && ok "vendored scripts/sdk-bump" || bad "vendored scripts/sdk-bump"
[ -x "$L/sdk/scripts/clean" ] && cmp -s "$S/scripts/clean" "$L/sdk/scripts/clean" && ok "vendored scripts/clean" || bad "vendored scripts/clean"
[ ! -e "$L/sdk/scripts/sdk-sync" ] && ok "sdk-sync itself not vendored" || bad "sdk-sync itself not vendored"
for f in packaging/pins.sh packaging/appimage-wine.sh packaging/smoke-package.sh \
  packaging/release-notes.sh packaging/appimage/AppRun.in packaging/appimage/tools.pins \
  packaging/appimage/wine/COPYING.LIB packaging/appimage/wine/LICENSE packaging/appimage/wine/SOURCE.txt; do
  if cmp -s "$S/$f" "$L/sdk/$f"; then ok "vendored $f"; else bad "vendored $f"; fi
done
for f in docs/STANDARDS.md docs/playbooks/a.md docs/playbooks/b.md; do
  if cmp -s "$S/$f" "$L/sdk/$f"; then ok "vendored $f"; else bad "vendored $f"; fi
done
[ ! -e "$L/sdk/docs/DESIGN.md" ] && ok "other SDK docs not vendored" || bad "other SDK docs not vendored"
[ ! -e "$L/sdk/packaging/tests" ] && ok "tests not vendored" || bad "tests not vendored"
[ -x "$L/sdk/packaging/appimage-wine.sh" ] && ok "exec bit kept" || bad "exec bit kept"
[ ! -x "$L/sdk/packaging/pins.sh" ] && ok "library stays non-executable" || bad "library stays non-executable"

check_ok() { if sh "$SYNC" --check "$L" >/dev/null 2>"$T/err"; then ok "$1"; else bad "$1: $(cat "$T/err")"; fi; }
check_drift() { # name, expected fragment
  if sh "$SYNC" --check "$L" >/dev/null 2>"$T/err"; then bad "$1 (no drift reported)"
  elif grep -q "$2" "$T/err"; then ok "$1"
  else bad "$1: $(cat "$T/err")"; fi
}
check_ok "fresh sync has no drift"
echo "# local hack" >> "$L/sdk/packaging/pins.sh"
check_drift "edited file is drift" "changed: sdk/packaging/pins.sh"
sh "$SYNC" "$L" >/dev/null 2>&1; check_ok "re-sync repairs it"
echo extra > "$L/sdk/packaging/mine.sh"
check_drift "extra file is drift" "not from the SDK: sdk/packaging/mine.sh"
rm "$L/sdk/packaging/mine.sh"
rm "$L/sdk/packaging/release-notes.sh"
check_drift "missing file is drift" "missing: sdk/packaging/release-notes.sh"
sh "$SYNC" "$L" >/dev/null 2>&1
echo "# local rule" >> "$L/sdk/docs/STANDARDS.md"
check_drift "edited vendored STANDARDS is drift" "changed: sdk/docs/STANDARDS.md"
sh "$SYNC" "$L" >/dev/null 2>&1
rm "$L/sdk/docs/playbooks/b.md"
check_drift "missing vendored playbook is drift" "missing: sdk/docs/playbooks/b.md"
sh "$SYNC" "$L" >/dev/null 2>&1
echo "# local" >> "$L/sdk/rea-play.sh"
check_drift "edited play library is drift" "changed: sdk/rea-play.sh"
sh "$SYNC" "$L" >/dev/null 2>&1
chmod -x "$L/sdk/packaging/smoke-package.sh"
check_drift "lost exec bit is drift" "mode differs: sdk/packaging/smoke-package.sh"
sh "$SYNC" "$L" >/dev/null 2>&1
# A newer SDK commit makes the launcher's copy stale.
echo "# newer" >> "$S/packaging/release-notes.sh"
(cd "$S" && git commit -qam newer && git tag v9.9.10)
check_drift "newer SDK version is drift" "changed: sdk/VERSION"
sh "$SYNC" "$L" >/dev/null 2>&1
grep -qx 'tag=v9.9.10' "$L/sdk/VERSION" && ok "re-sync stamps the new tag" || bad "re-sync stamps the new tag"
# Uncommitted SDK changes are stamped dirty.
echo "# wip" >> "$S/packaging/pins.sh"
sh "$SYNC" "$L" >/dev/null 2>&1
grep -q '^commit=.*-dirty$' "$L/sdk/VERSION" && ok "dirty SDK is stamped" || bad "dirty SDK is stamped"
(cd "$S" && git checkout -q -- packaging/pins.sh)
# --tag overrides describe.
sh "$SYNC" --tag v1.2.3 "$L" >/dev/null 2>&1
grep -qx 'tag=v1.2.3' "$L/sdk/VERSION" && ok "--tag is recorded" || bad "--tag is recorded"
if sh "$SYNC" --tag 'v1;rm' "$L" >/dev/null 2>&1; then bad "unsafe tag refused"; else ok "unsafe tag refused"; fi
# A sync killed mid-swap (sdk/ moved aside, new copy partial) recovers.
mv "$L/sdk" "$L/.sdk-sync.old.111"
mkdir -p "$L/.sdk-sync.new.111/packaging"; echo partial > "$L/.sdk-sync.new.111/packaging/pins.sh"
if sh "$SYNC" --check "$L" >/dev/null 2>"$T/err"; then bad "check reports an interrupted sync"
elif grep -q 'interrupted sync' "$T/err"; then ok "check reports an interrupted sync"
else bad "check reports an interrupted sync: $(cat "$T/err")"; fi
if sh "$SYNC" "$L" >/dev/null 2>"$T/err" && grep -q 'restored' "$T/err"; then ok "sync restores sdk/ after an interrupted swap"
else bad "sync restores sdk/ after an interrupted swap: $(cat "$T/err")"; fi
check_ok "recovered sdk/ has no drift"
[ -z "$(ls -A "$L" | grep '^\.sdk-sync' || true)" ] && ok "stray swap folders removed" || bad "stray swap folders removed"
# Leftovers next to an intact sdk/ are dropped, sdk/ kept.
mkdir -p "$L/.sdk-sync.old.222" "$L/.sdk-sync.new.333"
sh "$SYNC" "$L" >/dev/null 2>&1
[ -f "$L/sdk/VERSION" ] && [ -z "$(ls -A "$L" | grep '^\.sdk-sync' || true)" ] \
  && ok "leftovers beside an intact sdk/ cleaned" || bad "leftovers beside an intact sdk/ cleaned"

# A foreign sdk/ folder is never replaced.
F=$T/foreign; mkdir -p "$F/sdk"; echo mine > "$F/sdk/notes.txt"
if sh "$SYNC" "$F" >/dev/null 2>&1; then bad "foreign sdk/ refused"; else ok "foreign sdk/ refused"; fi
[ -f "$F/sdk/notes.txt" ] && ok "foreign sdk/ untouched" || bad "foreign sdk/ untouched"
if sh "$SYNC" --check "$T/nowhere" >/dev/null 2>&1; then bad "missing repo fails"; else ok "missing repo fails"; fi
if sh "$SYNC" --check "$F" >/dev/null 2>&1; then bad "check of a foreign sdk/ fails"; else ok "check of a foreign sdk/ fails"; fi
if sh "$SYNC" "$S" >/dev/null 2>&1; then bad "sync into the SDK refused"; else ok "sync into the SDK refused"; fi
[ -z "$(ls -A "$L" | grep '^\.sdk-sync' || true)" ] && ok "no temp folders left" || bad "no temp folders left"

[ "$fails" = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
