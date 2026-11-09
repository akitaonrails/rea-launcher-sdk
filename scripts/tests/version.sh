#!/bin/sh
# The SDK's version is written in more than one place; a release must move
# them together. v0.4.4 bumped Cargo.toml and left rea-play.sh at 0.4.3, so
# every launcher's Linux helper reported the wrong SDK.
#   sh scripts/tests/version.sh
set -u
ROOT=$(cd "$(dirname "$0")/../.." && pwd -P)
cargo=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"/\1/p' "$ROOT/Cargo.toml")
shell=$(sed -n 's/^REA_SDK_VERSION=//p' "$ROOT/shell/rea-play.sh")
fails=0
if [ -n "$cargo" ] && [ "$cargo" = "$shell" ]; then
  echo "ok   rea-play.sh's REA_SDK_VERSION matches Cargo.toml ($cargo)"
else
  echo "FAIL rea-play.sh says '$shell', Cargo.toml's workspace version is '$cargo'"
  fails=1
fi
for dep in rea-launcher rea-launcher-build; do
  pinned=$(sed -n "s/^$dep = { path = \"[^\"]*\", version = \"\\([^\"]*\\)\" }/\\1/p" "$ROOT/Cargo.toml")
  if [ "$pinned" = "$cargo" ]; then
    echo "ok   the $dep workspace dependency is $cargo"
  else
    echo "FAIL the $dep workspace dependency is '$pinned', the workspace is '$cargo'"
    fails=1
  fi
done
[ "$fails" = 0 ] && echo "all passed" || { echo "failed"; exit 1; }
