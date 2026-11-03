#!/bin/sh
# Tests packaging/release-notes.sh: sh packaging/tests/release-notes.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd -P)
NOTES=$HERE/../release-notes.sh
T=$(mktemp -d "${TMPDIR:-/tmp}/rea-notes-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
fails=0
check() { # name expected actual
  if [ "$2" = "$3" ]; then echo "ok   $1"; else echo "FAIL $1: got '$3'"; fails=$((fails + 1)); fi
}
refuses() { # name args...
  n=$1; shift
  if sh "$NOTES" "$@" >/dev/null 2>&1; then echo "FAIL $n"; fails=$((fails + 1)); else echo "ok   $n"; fi
}
# Keep a Changelog style (CMR3, CMR4, OR).
cat > "$T/kac.md" <<'MD'
# Changelog

## [Unreleased]

### Fixed

- unreleased fix

## [0.2.0] - 2026-12-01

### Fixed

- two
  with a wrapped line

### Notes

## [0.1.0] - 2026-11-02

The first release.

- one

[Unreleased]: https://example.com/compare/v0.2.0...HEAD
[0.2.0]: https://example.com/v0.2.0
MD
# "## vX" (SRR) and "## X" (Screamer) styles, CRLF-free, no trailing links.
cat > "$T/plain.md" <<'MD'
# Changelog

## Unreleased

- soon

## v0.1.1 - 2026-11-02

- srr style

## 0.6.0 - 2026-11-01

- screamer style


## 0.5.0

- bare heading
MD
printf '## [0.3.0] - 2027-01-01\n\n\n## [0.2.0] - x\n\n- y\n' > "$T/empty.md"
check "the tag's section only" "$(printf '### Fixed\n\n- two\n  with a wrapped line\n\n### Notes')" "$(sh "$NOTES" v0.2.0 "$T/kac.md")"
check "without the v" "$(printf 'The first release.\n\n- one')" "$(sh "$NOTES" 0.1.0 "$T/kac.md")"
check "stops at link references" "$(printf 'The first release.\n\n- one')" "$(sh "$NOTES" v0.1.0 "$T/kac.md")"
check "## vX headings" "- srr style" "$(sh "$NOTES" v0.1.1 "$T/plain.md")"
check "## X - date headings" "- screamer style" "$(sh "$NOTES" v0.6.0 "$T/plain.md")"
check "## X bare heading, last section" "- bare heading" "$(sh "$NOTES" 0.5.0 "$T/plain.md")"
check "prefix of a version is not a match" "" "$(sh "$NOTES" v0.1 "$T/kac.md" 2>/dev/null)"
refuses "a missing version fails" v0.9.0 "$T/kac.md"
refuses "an empty section fails" v0.3.0 "$T/empty.md"
refuses "a missing changelog fails" v0.1.0 "$T/none.md"
refuses "no arguments fails"
# --since: a fix-forward release (0.2.1 tagged, never published; 0.2.2).
cat > "$T/ff.md" <<'MD'
# Changelog

## [Unreleased]

- next

## [0.2.2] - 2026-12-03

### Fixed

- the release build

## [0.2.1] - 2026-12-02

### Added

- a feature

## [0.2.0] - 2026-12-01

- older

[0.2.2]: https://example.com
MD
check "--since: every unpublished section, newest first, with headings" "$(printf '## [0.2.2] - 2026-12-03\n\n### Fixed\n\n- the release build\n\n## [0.2.1] - 2026-12-02\n\n### Added\n\n- a feature')" "$(sh "$NOTES" --since v0.2.0 v0.2.2 "$T/ff.md")"
check "--since the previous version: only the version, with its heading" "$(printf '## [0.2.2] - 2026-12-03\n\n### Fixed\n\n- the release build')" "$(sh "$NOTES" --since 0.2.1 v0.2.2 "$T/ff.md")"
check "--since an older version than any: up to the oldest" "3" "$(sh "$NOTES" --since v0.0.1 v0.2.2 "$T/ff.md" | grep -c '^## ')"
check "without --since: unchanged" "$(printf '### Fixed\n\n- the release build')" "$(sh "$NOTES" v0.2.2 "$T/ff.md")"
refuses "--since still needs the version's own section" --since v0.2.0 v0.2.3 "$T/ff.md"
refuses "--since without a value" --since
[ "$fails" = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
