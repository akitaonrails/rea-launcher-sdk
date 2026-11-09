#!/bin/sh
# shellcheck disable=SC2034 # PIN_* settings are read by the sourced pins.sh
# Tests packaging/innoextract.sh with a fake zip over file:// (no network):
#   sh packaging/tests/innoextract.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd -P)
T=$(mktemp -d "${TMPDIR:-/tmp}/rea-innoextract-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
. "$HERE/../pins.sh"
. "$HERE/../innoextract.sh"
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }
check() { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1: want '$2', got '$3'"; fi; }
sha() { sha256sum "$1" | cut -d' ' -f1; }

# The shipped pins are well formed and name the verified files.
innoextract_pins > "$T/real.pins"
PIN_FILE=$T/real.pins
check "the zip's pin" 6989342c9b026a00a72a38f23b62a8e6a22cc5de69805cf47d68ac2fec993065 "$(pin_sha innoextract)"
check "the exe's pin" 946bd06d8b3722a791fea4d84f496139eef6faef61640924cb72f5f1998a68e1 "$(pin_sha innoextract-exe)"
check "the zip has a fallback" 2 "$(pin_urls innoextract | wc -l | tr -d ' ')"
if pin_mirrorable innoextract; then ok "zlib: mirrorable"; else bad "zlib: mirrorable"; fi

if ! command -v zip >/dev/null 2>&1 || ! command -v unzip >/dev/null 2>&1; then
  echo "SKIP bundling: zip/unzip not installed"
  [ "$fails" -eq 0 ] || exit 1
  exit 0
fi

mkdir -p "$T/src/license" "$T/up"
printf 'not really innoextract\n' > "$T/src/innoextract.exe"
printf 'zlib\n' > "$T/src/license/innoextract.LICENSE.txt"
printf 'boost\n' > "$T/src/license/Boost.LICENSE_1_0.txt"
printf 'docs\n' > "$T/src/README.md"
(cd "$T/src" && zip -q -r "$T/up/innoextract-1.9-windows.zip" .)
cat > "$T/fake.pins" <<PINS
innoextract file://$T/up/innoextract-1.9-windows.zip $(sha "$T/up/innoextract-1.9-windows.zip")
innoextract-exe - $(sha "$T/src/innoextract.exe")
PINS
PIN_FILE=$T/fake.pins PIN_CACHE='' PIN_RETRY=0
pkg="$T/pkg with space's"
if innoextract_bundle "$pkg" "$T/work" 2>"$T/err"; then ok "bundles"; else bad "bundles: $(cat "$T/err")"; fi
check "the exe goes to components/" "not really innoextract" "$(cat "$pkg/components/innoextract.exe")"
check "the licences go to licenses/innoextract/" "Boost.LICENSE_1_0.txt innoextract.LICENSE.txt" \
  "$(ls "$pkg/licenses/innoextract" | tr '\n' ' ' | sed 's/ $//')"
check "nothing else is copied" "components licenses" "$(ls "$pkg" | tr '\n' ' ' | sed 's/ $//')"
if [ -e "$T/work/innoextract-unzip" ]; then bad "scratch removed"; else ok "scratch removed"; fi

# A zip whose exe isn't the pinned one is refused, and nothing is bundled.
printf 'innoextract-exe - %s\n' "$(printf x | sha256sum | cut -d' ' -f1)" > "$T/wrong.pins"
grep '^innoextract ' "$T/fake.pins" >> "$T/wrong.pins"
PIN_FILE=$T/wrong.pins
if innoextract_bundle "$T/pkg2" "$T/work2" 2>/dev/null; then bad "a wrong exe is refused"; else ok "a wrong exe is refused"; fi
if [ -e "$T/pkg2/components/innoextract.exe" ]; then bad "nothing bundled"; else ok "nothing bundled"; fi
if innoextract_bundle "$T/pkg2" 2>/dev/null; then bad "usage"; else ok "usage"; fi

[ "$fails" -eq 0 ] || { echo "$fails failure(s)"; exit 1; }
