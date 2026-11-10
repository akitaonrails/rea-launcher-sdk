#!/bin/sh
# Tests scripts/clean in a temp git repository (ignored, tracked, not
# ignored, symlinks out, manifest entries, --keep, --work-all):
#   sh scripts/tests/clean.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd -P)
CLEAN=$HERE/../clean
T=$(mktemp -d "${TMPDIR:-/tmp}/rea-clean-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }
exists() { if [ -e "$2" ] || [ -L "$2" ]; then ok "$1"; else bad "$1: $2 is gone"; fi; }
gone() { if [ -e "$2" ] || [ -L "$2" ]; then bad "$1: $2 still exists"; else ok "$1"; fi; }
git() { command git -c user.name=t -c user.email=t@example.com -c init.defaultBranch=main "$@"; }

# A fresh repository: target/, dist/, .work/ with task scratch and evidence.
setup() {
  rm -rf "$T/r" "$T/outside"
  mkdir -p "$T/r" "$T/outside"
  echo precious > "$T/outside/keep.txt"
  cd "$T/r" || exit 1
  git init -q
  printf '/target/\n/dist/\n/.work/\n' > .gitignore
  echo src > lib.rs
  git add -A && git commit -qm init
  mkdir -p target/debug target/wine/prefix dist .work/probe-1 .work/evidence
  head -c 20000 /dev/zero > target/debug/big.bin
  echo p > target/wine/prefix/system.reg
  echo z > dist/pkg.zip
  echo s > .work/probe-1/scratch.txt
  echo e > .work/evidence/snapshot.rea
  printf '# task scratch\n.work/probe-1\n' > .work/.task-created
}
run() { (cd "$T/r" && sh "$CLEAN" "$@") >"$T/out" 2>"$T/err"; }

setup
if run; then ok "dry run exits 0"; else bad "dry run: $(cat "$T/err")"; fi
grep -q '^would remove' "$T/out" && ok "dry run says so" || bad "dry run header: $(cat "$T/out")"
for p in target dist .work/probe-1; do grep -q "  $p\$" "$T/out" && ok "lists $p" || bad "lists $p: $(cat "$T/out")"; done
grep -q 'evidence' "$T/out" && bad "evidence not listed" || ok "evidence not listed"
grep -q ' total$' "$T/out" && ok "prints a total" || bad "prints a total"
exists "dry run removes nothing" "$T/r/target/debug/big.bin"

if run --yes; then ok "--yes exits 0"; else bad "--yes: $(cat "$T/err")"; fi
gone "target removed" "$T/r/target"
gone "dist removed" "$T/r/dist"
gone "manifest entry removed" "$T/r/.work/probe-1"
exists "evidence kept" "$T/r/.work/evidence/snapshot.rea"
gone "emptied manifest removed" "$T/r/.work/.task-created"
exists "tracked source kept" "$T/r/lib.rs"
if run; then grep -q 'nothing to clean' "$T/out" && ok "second run: nothing to clean" || bad "second run: $(cat "$T/out")"; else bad "second run fails"; fi

# A trailing comment after the path (`path  # why`) is not part of it; a
# `#` inside a name is.
setup
mkdir -p '.work/probe#2' .work/probe-3
printf '.work/probe-1  # the wine probe\n.work/probe#2\n.work/probe-3\t# tab, then a comment\n' > .work/.task-created
if run --yes; then ok "commented entries: --yes exits 0"; else bad "commented entries: $(cat "$T/err")"; fi
gone "an entry with a trailing comment is removed" "$T/r/.work/probe-1"
gone "a # inside a name is part of it" "$T/r/.work/probe#2"
gone "a tab before the comment" "$T/r/.work/probe-3"
grep -q 'already gone' "$T/err" && bad "comment read as part of the path: $(cat "$T/err")" || ok "no comment read as a path"
gone "the manifest drops the commented entries" "$T/r/.work/.task-created"

# Tracked files inside a candidate: refused, nothing removed.
setup
git add -f dist/pkg.zip && git commit -qm tracked
if run --yes; then bad "tracked dist refused"; else grep -q 'refused: dist: Git tracks' "$T/err" && ok "tracked dist refused" || bad "tracked: $(cat "$T/err")"; fi
exists "a refusal removes nothing (target)" "$T/r/target/debug/big.bin"
exists "a refusal removes nothing (dist)" "$T/r/dist/pkg.zip"

# Not ignored: refused.
setup
printf '/dist/\n/.work/\n' > .gitignore
if run --yes; then bad "unignored target refused"; else grep -q 'refused: target: not ignored' "$T/err" && ok "unignored target refused" || bad "unignored: $(cat "$T/err")"; fi
exists "unignored target kept" "$T/r/target/debug/big.bin"

# A symlink out of the repository: refused, its target untouched.
setup
rm -rf target && ln -s "$T/outside" target
if run --yes; then bad "symlink out refused"; else grep -q 'refused: target: resolves outside' "$T/err" && ok "symlink out refused" || bad "symlink: $(cat "$T/err")"; fi
exists "outside folder untouched" "$T/outside/keep.txt"
setup
ln -s "$T/outside" .work/out-link && echo .work/out-link >> .work/.task-created
if run --yes; then bad "manifest symlink out refused"; else ok "manifest symlink out refused"; fi
exists "outside folder untouched (manifest)" "$T/outside/keep.txt"

# Manifest entries outside .work/ are refused.
for evil in lib.rs ../outside .work/../lib.rs .work/.task-created; do
  setup
  echo "$evil" >> .work/.task-created
  if run --yes; then bad "manifest entry '$evil' refused"; else ok "manifest entry '$evil' refused"; fi
  exists "nothing removed for '$evil'" "$T/r/target/debug/big.bin"
done
exists "outside folder still there" "$T/outside/keep.txt"

# --keep.
setup
if run --yes --keep target/wine/prefix; then ok "--keep run"; else bad "--keep: $(cat "$T/err")"; fi
exists "kept path survives (its parent skipped)" "$T/r/target/wine/prefix/system.reg"
gone "other candidates still removed" "$T/r/dist"

# --work-all warns and includes .work/.
setup
if run --work-all; then ok "--work-all dry run"; else bad "--work-all: $(cat "$T/err")"; fi
grep -q 'may hold investigation evidence' "$T/err" && ok "--work-all warns" || bad "--work-all warns"
grep -q '  \.work$' "$T/out" && ok "--work-all lists .work" || bad "--work-all lists: $(cat "$T/out")"

# Not a git repository.
mkdir -p "$T/plain"
if (cd "$T/plain" && sh "$CLEAN") >/dev/null 2>&1; then bad "non-git folder refused"; else ok "non-git folder refused"; fi
# Never git clean.
grep -q 'git clean' "$CLEAN" && ! grep -q '^[^#]*git clean' "$CLEAN" && ok "never runs git clean" || bad "never runs git clean"

[ "$fails" = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
