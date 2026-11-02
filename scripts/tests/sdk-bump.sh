#!/bin/sh
# Tests scripts/sdk-bump with a local SDK git repository (tags v0.1.0,
# v0.2.0, v0.3.0), a fake launcher repository and a fake cargo (no
# network): sh scripts/tests/sdk-bump.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd -P)
ROOT=$(cd "$HERE/../.." && pwd -P)
T=$(mktemp -d "${TMPDIR:-/tmp}/rea-sdk-bump-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }
check() { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1: want '$2', got '$3'"; fi; }
git() { command git -c user.name=test -c user.email=test@example.com -c init.defaultBranch=main "$@"; }
export GIT_CONFIG_NOSYSTEM=1 REA_SDK_CACHE="$T/cache"
unset REA_SDK_URL

# The SDK: three tagged versions with Migration items (one wrapped).
S=$T/sdk-src
mkdir -p "$S/scripts" "$S/shell"
cp -R "$ROOT/packaging" "$S/packaging"
cp "$ROOT/scripts/sdk-sync" "$ROOT/scripts/sdk-bump" "$ROOT/scripts/wine-test" "$S/scripts/"
echo '# play v1' > "$S/shell/rea-play.sh"
changelog() { # versions... newest first, each "ver:style:migration text"
  # style: section (### Migration, the current format; an inline mention
  # elsewhere is then ignored), mid (a **Migration:** note inside an Added
  # bullet), start (a bullet that starts with **Migration:**).
  echo '# Changelog'
  echo
  echo '## [Unreleased]'
  echo
  echo '### Migration'
  echo
  echo '- unreleased, never shown'
  for v; do
    ver=${v%%:*} rest=${v#*:}; style=${rest%%:*} text=${rest#*:}
    echo
    echo "## [$ver] - 2026-11-02"
    echo
    case $style in
      section)
        printf '### Migration\n\n- %s\n  wrapped second line\n- second item\n\n' "$text"
        printf '### Added\n\n- a feature. **Migration:** not repeated from here\n' ;;
      mid)
        printf '### Added\n\n- a feature\n  more. **Migration:** %s\n  verify it\n- not a migration\n' "$text" ;;
      start)
        printf '### Changed\n\n- **Migration:** %s\n  wrapped second line\n- not a migration\n' "$text" ;;
    esac
  done
}
changelog "0.1.0:start:first" > "$S/CHANGELOG.md"
(cd "$S" && git init -q && git add -A && git commit -qm v1 && git tag v0.1.0) || { echo "git setup failed"; exit 1; }
echo '# play v2' > "$S/shell/rea-play.sh"
changelog "0.2.0:mid:rename Palette.line" "0.1.0:start:first" > "$S/CHANGELOG.md"
(cd "$S" && git commit -qam v2 && git tag v0.2.0)
echo '# play v3' > "$S/shell/rea-play.sh"
changelog "0.3.0:section:WindowFit opt-in" "0.2.0:mid:rename Palette.line" "0.1.0:start:first" > "$S/CHANGELOG.md"
(cd "$S" && git commit -qam v3 && git tag v0.3.0)
V3=$(git -C "$S" rev-parse v0.3.0)
BUMP=$S/scripts/sdk-bump

# The launcher at v0.1.0.
L=$T/launcher
mkdir -p "$L/crates/game" "$L/.github/workflows"
cat > "$L/Cargo.toml" <<'EOF'
[workspace]
members = ["crates/game"]

[workspace.dependencies]
rea-launcher = { git = "https://github.com/akitaonrails/rea-launcher-sdk", tag = "v0.1.0" }
rea-launcher-ui = { git="https://github.com/akitaonrails/rea-launcher-sdk", tag="v0.1.0", features = ["testing"] }
other = { git = "https://github.com/someone/other", tag = "v0.1.0" }
EOF
cat > "$L/crates/game/Cargo.toml" <<'EOF'
[package]
name = "game"
version = "1.0.0"

[dependencies.rea-launcher-win32]
git = "https://github.com/akitaonrails/rea-launcher-sdk"
tag = "v0.1.0"

[dependencies.unrelated]
version = "1"
tag = "keep-me"
EOF
printf '[[package]]\nname = "rea-launcher"\n\n[[package]]\nname = "rea-launcher-ui"\n\n[[package]]\nname = "game"\n' > "$L/Cargo.lock"
cat > "$L/.github/workflows/build.yml" <<'EOF'
jobs:
  ci:
    uses: akitaonrails/rea-launcher-sdk/.github/workflows/launcher-ci.yml@v0.1.0
  release:
    uses: akitaonrails/rea-launcher-sdk/.github/workflows/launcher-release.yml@v0.1.0
  other:
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
EOF
(cd "$S" && git checkout -q v0.1.0) && sh "$S/scripts/sdk-sync" "$L" >/dev/null && (cd "$S" && git checkout -q main)
(cd "$L" && git init -q && git add -A && git commit -qm launcher) || { echo "launcher setup failed"; exit 1; }

# Fake cargo: records its arguments.
mkdir -p "$T/bin"
printf '#!/bin/sh\necho "$PWD: $*" >> "%s"\n' "$T/cargo.log" > "$T/bin/cargo"
chmod +x "$T/bin/cargo"
export PATH="$T/bin:$PATH"

# 1. The SDK checkout itself is at v0.3.0 and clean: used directly.
if sh "$BUMP" --notes "$T/notes.md" v0.3.0 "$L" >"$T/out" 2>&1; then ok "bump succeeds"; else bad "bump: $(cat "$T/out")"; fi
grep -q "SDK v0.3.0 from $S\$" "$T/out" && ok "uses the SDK checkout at the tag" || bad "uses the SDK checkout: $(grep 'from' "$T/out")"
check "inline tag moved" 'rea-launcher = { git = "https://github.com/akitaonrails/rea-launcher-sdk", tag = "v0.3.0" }' "$(grep '^rea-launcher =' "$L/Cargo.toml")"
check "inline tag moved, other keys kept" 'rea-launcher-ui = { git="https://github.com/akitaonrails/rea-launcher-sdk", tag = "v0.3.0", features = ["testing"] }' "$(grep '^rea-launcher-ui' "$L/Cargo.toml")"
check "other git dependency untouched" 'other = { git = "https://github.com/someone/other", tag = "v0.1.0" }' "$(grep '^other' "$L/Cargo.toml")"
check "table-form tag moved" 'tag = "v0.3.0"' "$(sed -n '/dependencies.rea-launcher-win32/,/^$/p' "$L/crates/game/Cargo.toml" | grep '^tag')"
check "unrelated table untouched" 'tag = "keep-me"' "$(sed -n '/dependencies.unrelated/,$p' "$L/crates/game/Cargo.toml" | grep '^tag')"
check "cargo update for the SDK crates in Cargo.lock" "$L: update -p rea-launcher -p rea-launcher-ui" "$(cat "$T/cargo.log")"
grep -qx 'tag=v0.3.0' "$L/sdk/VERSION" && grep -qx "commit=$V3" "$L/sdk/VERSION" && ok "sdk/ re-vendored at v0.3.0" || bad "sdk/VERSION: $(cat "$L/sdk/VERSION")"
check "vendored files from v0.3.0" "# play v3" "$(cat "$L/sdk/rea-play.sh")"
[ -x "$L/sdk/scripts/sdk-bump" ] && ok "sdk-bump vendored" || bad "sdk-bump vendored"
check "workflow refs moved" "2" "$(grep -c 'rea-launcher-sdk/.github/workflows/[a-z-]*\.yml@v0.3.0$' "$L/.github/workflows/build.yml")"
grep -q 'actions/checkout@3d3c42e5' "$L/.github/workflows/build.yml" && ok "other actions untouched" || bad "other actions untouched"
check "Migration section (0.3.0) and a note inside a bullet (0.2.0), newest first" "### v0.3.0
- WindowFit opt-in
  wrapped second line
- second item

### v0.2.0
- a feature
  more. **Migration:** rename Palette.line
  verify it" "$(cat "$T/notes.md")"
grep -q '^- WindowFit opt-in' "$T/out" && ok "Migration items printed" || bad "Migration items printed"
if grep -q 'not repeated from here' "$T/notes.md"; then bad "a Migration section wins over inline notes"; else ok "a Migration section wins over inline notes"; fi

# Idempotent; a dirty tree is refused.
if sh "$BUMP" v0.3.0 "$L" >"$T/out" 2>&1; then bad "dirty tree refused"; else grep -q 'uncommitted changes' "$T/out" && ok "dirty tree refused" || bad "dirty tree: $(cat "$T/out")"; fi
(cd "$L" && git add -A && git commit -qm bump)
: > "$T/cargo.log"
if sh "$BUMP" v0.3.0 "$L" >"$T/out" 2>&1; then ok "second run succeeds"; else bad "second run: $(cat "$T/out")"; fi
check "second run changes nothing" "" "$(git -C "$L" status --porcelain)"
grep -q 'No Migration items between v0.3.0 and v0.3.0' "$T/out" && ok "no Migration items for the same version" || bad "same-version notes: $(tail -5 "$T/out")"

if sh "$BUMP" --since v0.1.0 --notes "$T/since.md" v0.3.0 "$L" >/dev/null 2>&1 && grep -q 'rename Palette.line' "$T/since.md"; then
  ok "--since lists items after that version"
else
  bad "--since: $(cat "$T/since.md" 2>/dev/null)"
fi
check "--since run changes nothing" "" "$(git -C "$L" status --porcelain)"

# Downgrade and unknown tags.
if sh "$BUMP" v0.2.0 "$L" >"$T/out" 2>&1; then bad "downgrade refused"; else grep -q 'older than' "$T/out" && ok "downgrade refused" || bad "downgrade: $(cat "$T/out")"; fi
if sh "$BUMP" v9.9.9 "$L" >"$T/out" 2>&1; then bad "unknown tag fails"; else ok "unknown tag fails"; fi
check "a failed bump leaves the tree clean" "" "$(git -C "$L" status --porcelain)"
if sh "$BUMP" 0.3 "$L" >/dev/null 2>&1; then bad "malformed tag refused"; else ok "malformed tag refused"; fi

# 2. From the vendored copy (no SDK checkout around): clone into the cache
# from REA_SDK_URL. Start the launcher back at v0.2.0 by a forced downgrade.
if sh "$BUMP" --force --no-cargo v0.2.0 "$L" >"$T/out" 2>&1; then ok "forced downgrade"; else bad "forced downgrade: $(cat "$T/out")"; fi
(cd "$L" && git add -A && git commit -qm down)
rm -rf "$T/cache"
if REA_SDK_URL=$S sh "$L/sdk/scripts/sdk-bump" --no-cargo v0.3.0 "$L" >"$T/out" 2>&1; then ok "vendored sdk-bump works"; else bad "vendored sdk-bump: $(cat "$T/out")"; fi
grep -q "SDK v0.3.0 from $T/cache\$" "$T/out" && ok "vendored run clones into the cache" || bad "cache: $(grep from "$T/out")"
grep -qx "commit=$V3" "$L/sdk/VERSION" && ok "cache checkout at the tag" || bad "cache checkout at the tag"
check "only v0.3.0's items after 0.2.0" "### v0.3.0
- WindowFit opt-in
  wrapped second line
- second item" "$(sed -n '/^### v/,$p' "$T/out" | sed '/^$/,$d')"

# 3. An SDK checkout not at the tag: its local tags feed the cache, no network.
(cd "$L" && git add -A && git commit -qm up)
(cd "$S" && git checkout -q v0.2.0)
if sh "$BUMP" --force --no-cargo v0.2.0 "$L" >"$T/out" 2>&1; then :; fi
(cd "$L" && git add -A && git commit -qm down2)
(cd "$S" && git checkout -q main && echo wip >> CHANGELOG.md)
if sh "$BUMP" --no-cargo v0.3.0 "$L" >"$T/out" 2>&1 && grep -q "from $T/cache\$" "$T/out"; then
  ok "a dirty SDK checkout is not used; its tags are"
else
  bad "dirty SDK checkout: $(cat "$T/out")"
fi
grep -qx "commit=$V3" "$L/sdk/VERSION" && ok "stamped with the tag's commit, not the work tree" || bad "stamp: $(cat "$L/sdk/VERSION")"

# The SDK's own CHANGELOG: every v0.2.0 and v0.3.0 Migration bullet is
# found (the CMR4 bump missed the ones inside Added bullets).
awk_prog=$(sed -n "/^  awk -v old=/,/^  ' \"\$CO\/CHANGELOG.md\"/p" "$ROOT/scripts/sdk-bump" | sed '1d;$d')
real=$(awk -v old=0.1.0 -v new=0.3.0 "$awk_prog" "$ROOT/CHANGELOG.md")
for want in 'Pin::name()' 'REA_HELPER_KEYS=' 'link-arg=-fuse-ld=lld' 'folder_field' 'LookCard' 'Palette` has a new' 'with_native_fit' 'ArgError::Help' 'contrast_problems' 'wine-test` keeps'; do
  case $real in *"$want"*) ok "real CHANGELOG: $want listed" ;; *) bad "real CHANGELOG: $want missing" ;; esac
done

[ "$fails" = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
