#!/bin/sh
# packaging/package.sh builds a zip folder that passes packaging/package.allow
# (no AppImage, no network; a stand-in exe): sh packaging/tests/package.sh
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd -P)
T=$(mktemp -d "${TMPDIR:-/tmp}/@ID@-package-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
fails=0
ok() { echo "ok   $1"; }
fail() { echo "FAIL $1"; fails=$((fails + 1)); }

mkdir -p "$T/release"
printf 'MZ stand-in\n' > "$T/release/@LAUNCHER_EXE@"
if REA_RELEASE_DIR=$T/release sh "$ROOT/packaging/package.sh" --no-appimage \
  "$T/dist/@LAUNCHER_NAME@-v0.0.0" > "$T/out" 2>&1; then
  ok "package.sh builds the folder and zip"
else
  fail "package.sh: $(cat "$T/out")"
fi
for p in "$T/dist/@LAUNCHER_NAME@-v0.0.0" "$T/dist/@LAUNCHER_NAME@-v0.0.0.zip"; do
  if sh "$ROOT/sdk/packaging/smoke-package.sh" "$p" "$ROOT/packaging/package.allow" > "$T/out" 2>&1; then
    ok "allowlist passes: $(basename "$p")"
  else
    fail "allowlist: $(basename "$p"): $(cat "$T/out")"
  fi
done
if REA_RELEASE_DIR=$T/nowhere sh "$ROOT/packaging/package.sh" --no-appimage "$T/dist/x" > /dev/null 2>&1; then
  fail "a missing exe must fail"
else
  ok "a missing exe fails"
fi

if [ "$fails" = 0 ]; then echo "all passed"; exit 0; fi
echo "$fails failed"; exit 1
