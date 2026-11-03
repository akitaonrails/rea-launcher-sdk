#!/bin/sh
# Tests scripts/release in temp launcher repositories with a fake cargo (no
# network, nothing pushed): sh scripts/tests/release.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd -P)
RELEASE=$HERE/../release
T=$(mktemp -d "${TMPDIR:-/tmp}/rea-release-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }
check() { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1: want '$2', got '$3'"; fi; }
git() { command git -c user.name=t -c user.email=t@example.com -c init.defaultBranch=main -c tag.gpgSign=false -c commit.gpgSign=false "$@"; }
export GIT_CONFIG_NOSYSTEM=1 GIT_AUTHOR_NAME=t GIT_AUTHOR_EMAIL=t@example.com GIT_COMMITTER_NAME=t GIT_COMMITTER_EMAIL=t@example.com

# Fake cargo: `update` records, `test` records the launcher's version at
# that moment and fails when $T/test-fails exists.
mkdir -p "$T/bin"
cat > "$T/bin/cargo" <<'FAKE'
#!/bin/sh
echo "$*" >> "$FAKE_DIR/cargo.log"
case $1 in
  test) grep -h '^version' Cargo.toml crates/*/Cargo.toml > "$FAKE_DIR/version-at-test" 2>/dev/null
        [ ! -e "$FAKE_DIR/test-fails" ] ;;
esac
FAKE
chmod +x "$T/bin/cargo"
export PATH="$T/bin:$PATH" FAKE_DIR="$T"

# launcher NAME CHANGELOG-STYLE [workspace]: a repo whose first crate
# (alphabetically) is a hook DLL, and whose build.yml releases the launcher.
launcher() {
  L=$T/$1
  rm -rf "$L" "$L.git"
  mkdir -p "$L/crates/aaa-hooks" "$L/crates/game-launcher" "$L/.github/workflows" "$L/sdk/packaging"
  cp "$HERE/../../packaging/release-notes.sh" "$L/sdk/packaging/"
  if [ "${3:-}" = workspace ]; then
    printf '[workspace]\nmembers = ["crates/*"]\n\n[workspace.package]\nversion = "0.3.1"\nedition = "2024"\n' > "$L/Cargo.toml"
    lv='version.workspace = true'
  else
    printf '[workspace]\nmembers = ["crates/*"]\n' > "$L/Cargo.toml"
    lv='version = "0.3.1"'
  fi
  printf '[package]\nname = "aaa-hooks"\nversion = "9.9.9"\n\n[dependencies]\nx = { version = "1.0.0" }\n' > "$L/crates/aaa-hooks/Cargo.toml"
  printf '[package]\nname = "game-launcher"\n%s\n\n[dependencies]\nserde = { version = "1.0.0" }\n' "$lv" > "$L/crates/game-launcher/Cargo.toml"
  printf '# lock\n' > "$L/Cargo.lock"
  cat > "$L/.github/workflows/build.yml" <<'EOF'
jobs:
  ci:
    uses: akitaonrails/rea-launcher-sdk/.github/workflows/launcher-ci.yml@v0.4.0
    with:
      package-command: x
  release:
    if: startsWith(github.ref, 'refs/tags/v')
    uses: akitaonrails/rea-launcher-sdk/.github/workflows/launcher-release.yml@v0.4.0
    with:
      # the crate the tag must match
      package: game-launcher
      artifact-pattern: game-*
EOF
  case $2 in
    kac) cat > "$L/CHANGELOG.md" <<'EOF'
# Changelog

## [Unreleased]

### Fixed

- a fix

## [0.3.1] - 2026-11-02

- older

[Unreleased]: https://github.com/o/game/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/o/game/compare/v0.3.0...v0.3.1
EOF
    ;;
    v) printf '# Changelog\n\n## Unreleased\n\n- srr fix\n\n## v0.3.1 - 2026-11-02\n\n- older\n' > "$L/CHANGELOG.md" ;;
    plain) printf '# Changelog\n\n## Unreleased\n\n- screamer fix\n\n## 0.3.1 - 2026-11-02\n\n- older\n' > "$L/CHANGELOG.md" ;;
    own) printf '# Changelog\n\n## 0.4.0\n\n- written ahead\n\n## 0.3.1 - 2026-11-02\n\n- older\n' > "$L/CHANGELOG.md" ;;
    empty) printf '# Changelog\n\n## [Unreleased]\n\n## [0.3.1] - 2026-11-02\n\n- older\n' > "$L/CHANGELOG.md" ;;
    none) printf '# Changelog\n\n## [0.3.1] - 2026-11-02\n\n- older\n' > "$L/CHANGELOG.md" ;;
  esac
  (cd "$L" && git init -q && git add -A && git commit -qm init && git tag v0.3.1 &&
    git init -q --bare "$L.git" && git remote add origin "$L.git" && git push -q origin main v0.3.1 2>/dev/null)
  : > "$T/cargo.log"
}
release() { (cd "$L" && sh "$RELEASE" --date 2026-12-24 "$@") > "$T/out" 2>&1; }
clean_tree() { check "$1" "" "$(git -C "$L" status --porcelain)"; }

# --- Keep a Changelog, the crate's own version ----------------------------------
launcher kac kac
if release 0.4.0; then ok "release succeeds"; else bad "release: $(cat "$T/out")"; fi
check "the crate launcher-release names is bumped" 'version = "0.4.0"' "$(grep '^version' "$L/crates/game-launcher/Cargo.toml")"
check "not the first crate found" 'version = "9.9.9"' "$(grep '^version' "$L/crates/aaa-hooks/Cargo.toml")"
check "dependency versions untouched" 'serde = { version = "1.0.0" }' "$(grep '^serde' "$L/crates/game-launcher/Cargo.toml")"
check "Cargo.lock refreshed for the workspace" "update --workspace --offline" "$(head -n 1 "$T/cargo.log")"
check "cargo test --locked --workspace after the bump" "test --locked --workspace" "$(sed -n 2p "$T/cargo.log")"
grep -q 'game-launcher.*0.4.0\|^version = "0.4.0"' "$T/version-at-test" && ok "the test saw the new version" || bad "version at test: $(cat "$T/version-at-test")"
check "the CHANGELOG dated in its own style, a new Unreleased kept" "# Changelog

## [Unreleased]

## [0.4.0] - 2026-12-24

### Fixed

- a fix

## [0.3.1] - 2026-11-02

- older

[Unreleased]: https://github.com/o/game/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/o/game/compare/v0.3.1...v0.4.0
[0.3.1]: https://github.com/o/game/compare/v0.3.0...v0.3.1" "$(cat "$L/CHANGELOG.md")"
check "committed" "chore: release v0.4.0" "$(git -C "$L" log -1 --format=%s)"
check "an annotated tag on that commit" "tag $(git -C "$L" rev-parse HEAD)" "$(git -C "$L" cat-file -t v0.4.0) $(git -C "$L" rev-parse 'v0.4.0^{commit}')"
clean_tree "nothing left uncommitted"
check "nothing pushed" "$(git -C "$L" rev-parse HEAD~1)" "$(git -C "$L.git" rev-parse main)"
git -C "$L.git" rev-parse -q --verify refs/tags/v0.4.0 >/dev/null && bad "the tag was pushed" || ok "the tag was not pushed"
grep -q 'git push origin v0.4.0' "$T/out" && ok "prints the push commands for the owner" || bad "push hint: $(cat "$T/out")"
grep -q '| ### Fixed' "$T/out" && ok "shows the release notes" || bad "notes shown: $(cat "$T/out")"

if release 0.4.0; then bad "an existing tag is refused"; else grep -q 'already exists' "$T/out" && ok "an existing tag is refused" || bad "existing tag: $(cat "$T/out")"; fi
if release 0.3.9; then bad "an older version is refused"; else grep -q 'not newer' "$T/out" && ok "an older version is refused" || bad "older: $(cat "$T/out")"; fi
echo x >> "$L/README"
if release 0.5.0; then bad "a dirty tree is refused"; else grep -q 'uncommitted' "$T/out" && ok "a dirty tree is refused" || bad "dirty: $(cat "$T/out")"; fi
rm "$L/README"
for v in 1.2 v1.2.3.4 'x;y' 1.2.3-rc1; do
  if release "$v"; then bad "malformed $v refused"; else ok "malformed $v refused"; fi
done

# --- workspace version, other heading styles ----------------------------------
launcher ws v workspace
if release v0.3.2; then ok "workspace: succeeds (vX spelling)"; else bad "workspace: $(cat "$T/out")"; fi
check "workspace: [workspace.package] version" 'version = "0.3.2"' "$(grep '^version' "$L/Cargo.toml")"
check "workspace: the crate keeps version.workspace" 'version.workspace = true' "$(grep '^version' "$L/crates/game-launcher/Cargo.toml")"
check "## vX - date style" "## Unreleased

## v0.3.2 - 2026-12-24

- srr fix" "$(sed -n '3,7p' "$L/CHANGELOG.md")"

launcher plain plain
release 0.3.2 && ok "## X style succeeds" || bad "plain: $(cat "$T/out")"
check "## X - date style" "## 0.3.2 - 2026-12-24" "$(sed -n 5p "$L/CHANGELOG.md")"

launcher own own
release 0.4.0 && ok "an undated heading of the version succeeds" || bad "own: $(cat "$T/out")"
check "the undated heading gets the date" "## 0.4.0 - 2026-12-24" "$(sed -n 3p "$L/CHANGELOG.md")"
check "no Unreleased heading added where the repo has none" 0 "$(grep -ci unreleased "$L/CHANGELOG.md")"

# --- refusals that leave everything as it was -------------------------------------
launcher empty empty
if release 0.4.0; then bad "an empty Unreleased is refused"; else grep -q 'is empty' "$T/out" && ok "an empty Unreleased is refused" || bad "empty: $(cat "$T/out")"; fi
clean_tree "empty: files put back"
launcher none none
if release 0.4.0; then bad "no Unreleased section is refused"; else grep -q 'no Unreleased section' "$T/out" && ok "no Unreleased section is refused" || bad "none: $(cat "$T/out")"; fi
clean_tree "none: files put back"

launcher failing kac
: > "$T/test-fails"
if release 0.4.0; then bad "a failing cargo test stops the release"; else grep -q 'cargo test failed after the bump' "$T/out" && ok "a failing cargo test stops the release" || bad "test fails: $(cat "$T/out")"; fi
rm "$T/test-fails"
clean_tree "failing test: files put back"
git -C "$L" rev-parse -q --verify refs/tags/v0.4.0 >/dev/null && bad "failing test: tagged" || ok "failing test: no tag, no commit"
check "failing test: no commit" "init" "$(git -C "$L" log -1 --format=%s)"
: > "$T/cargo.log"
release --no-test 0.4.0 && ok "--no-test skips cargo test" || bad "--no-test: $(cat "$T/out")"
grep -q '^test' "$T/cargo.log" && bad "--no-test ran the tests" || ok "--no-test ran no tests"

launcher nopkg kac
sed -i.bak '/package: game-launcher/d' "$L/.github/workflows/build.yml" && rm "$L/.github/workflows/build.yml.bak"
(cd "$L" && git commit -qam nopkg)
if release 0.4.0; then bad "no package: input is refused"; else grep -q 'no launcher-release.yml call with a package' "$T/out" && ok "no package: input is refused" || bad "nopkg: $(cat "$T/out")"; fi
launcher twopkg kac
printf '  release2:\n    uses: o/rea-launcher-sdk/.github/workflows/launcher-release.yml@v1\n    with:\n      package: aaa-hooks\n' >> "$L/.github/workflows/build.yml"
(cd "$L" && git commit -qam two)
if release 0.4.0; then bad "two packages are refused"; else grep -q 'different packages' "$T/out" && ok "two packages are refused" || bad "twopkg: $(cat "$T/out")"; fi
launcher wrongpkg kac
sed -i.bak 's/package: game-launcher/package: other-launcher/' "$L/.github/workflows/build.yml" && rm "$L/.github/workflows/build.yml.bak"
(cd "$L" && git commit -qam wrong)
if release 0.4.0; then bad "a package with no crate is refused"; else grep -q 'no Cargo.toml with \[package\] name = "other-launcher"' "$T/out" && ok "a package with no crate is refused" || bad "wrongpkg: $(cat "$T/out")"; fi
clean_tree "refusals change nothing"

[ "$fails" = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
