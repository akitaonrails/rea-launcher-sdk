#!/bin/sh
# shellcheck disable=SC2034 # PIN_* settings are read by the sourced pins.sh
# Tests packaging/pins.sh with file:// urls (no network): sh packaging/tests/pins.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd -P)
T=$(mktemp -d "${TMPDIR:-/tmp}/rea-pins-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
. "$HERE/../pins.sh"
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }
check() { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1: want '$2', got '$3'"; fi; }
sha() { sha256sum "$1" | cut -d' ' -f1; }

mkdir -p "$T/up" "$T/mirror" "$T/pins.d"
echo "real payload" > "$T/up/tool.tar.gz"
echo "real payload" > "$T/mirror/tool-mirror.tar.gz"
echo "tampered" > "$T/up/evil.zip"
GOOD=$(sha "$T/up/tool.tar.gz")
EVIL_URL=file://$T/up/evil.zip
cat > "$T/pins" <<EOF
# a comment
tool file://$T/up/tool.tar.gz $GOOD file://$T/mirror/tool-mirror.tar.gz

moved file://$T/up/gone.tar.gz $GOOD file://$T/missing/also-gone.tar.gz file://$T/mirror/tool-mirror.tar.gz
tampered $EVIL_URL $GOOD file://$T/mirror/tool-mirror.tar.gz
nowhere file://$T/up/gone.tar.gz $GOOD
inner - $GOOD
dup file://a/x $GOOD
dup file://a/y $GOOD
shortsha file://$T/up/tool.tar.gz abc123
junkfield file://$T/up/tool.tar.gz $GOOD 325966152
EOF
echo "file://$T/up/tool.tar.gz $GOOD" > "$T/pins.d/perfile.txt"
printf '# just a comment\n' > "$T/pins.d/empty.txt"

# shellcheck disable=SC2034 # read by pins.sh
PIN_FILE=$T/pins PIN_DIR=$T/pins.d PIN_CACHE=$T/cache PIN_RETRY=0

check "pin_sha" "$GOOD" "$(pin_sha tool)"
check "pin_url is the primary" "file://$T/up/tool.tar.gz" "$(pin_url tool)"
check "pin_urls lists mirrors in order" "file://$T/up/tool.tar.gz
file://$T/mirror/tool-mirror.tar.gz" "$(pin_urls tool)"
check "checksum-only pin has a sha" "$GOOD" "$(pin_sha inner)"
check "checksum-only pin has no urls" "" "$(pin_urls inner)"
check "per-name file (PIN_DIR)" "$GOOD" "$(pin_sha perfile)"
check "pin_names" "tool moved tampered nowhere inner dup dup shortsha junkfield empty perfile" \
  "$(pin_names | tr '\n' ' ' | sed 's/ $//')"

if pin_sha missing 2>/dev/null; then bad "unknown name fails"; else ok "unknown name fails"; fi
if pin_sha empty 2>/dev/null; then bad "empty per-name file fails"; else ok "empty per-name file fails"; fi
if pin_sha dup 2>/dev/null; then bad "duplicate name fails"; else ok "duplicate name fails"; fi
if pin_sha shortsha 2>/dev/null; then bad "short sha fails"; else ok "short sha fails"; fi
if pin_sha junkfield 2>/dev/null; then bad "non-url trailing field fails"; else ok "non-url trailing field fails"; fi
if pin_sha 'a b' 2>/dev/null; then bad "unsafe name fails"; else ok "unsafe name fails"; fi
if pin_sha '../x' 2>/dev/null; then bad "path name fails"; else ok "path name fails"; fi

f=$(pin_fetch tool "$T/dl" 2>/dev/null)
check "fetch from the primary url" "$T/dl/tool.tar.gz" "$f"
check "fetched content" "real payload" "$(cat "$f")"
[ -f "$T/cache/$GOOD" ] && ok "fetch fills the cache" || bad "fetch fills the cache"

rm -rf "$T/dl"
f=$(PIN_CACHE='' pin_fetch moved "$T/dl" 2>"$T/log")
check "fallback keeps the primary file name" "$T/dl/gone.tar.gz" "$f"
check "fallback content" "real payload" "$(cat "$f" 2>/dev/null)"
grep -q 'also-gone' "$T/log" && ok "mirrors tried in order" || bad "mirrors tried in order"

f=$(PIN_CACHE='' pin_fetch tampered "$T/dl" 2>/dev/null)
check "a wrong checksum falls through to the mirror" "real payload" "$(cat "$f" 2>/dev/null)"
[ -z "$(ls -A "$T/dl" | grep '\.part' || true)" ] && ok "no .part leftovers" || bad "no .part leftovers"

if (PIN_CACHE=''; pin_fetch nowhere "$T/dl2") >/dev/null 2>&1; then bad "no source fails"; else ok "no source fails"; fi
[ ! -e "$T/dl2/gone.tar.gz" ] && ok "a failed fetch leaves no file" || bad "a failed fetch leaves no file"
if pin_fetch inner "$T/dl2" >/dev/null 2>&1; then bad "checksum-only fetch fails"; else ok "checksum-only fetch fails"; fi

# Served from the cache once upstream and mirror are gone.
mv "$T/up" "$T/up.off"; mv "$T/mirror" "$T/mirror.off"
f=$(pin_fetch tool "$T/dl3" 2>/dev/null)
check "cache serves an offline fetch" "real payload" "$(cat "$f" 2>/dev/null)"
# A corrupted cache entry is dropped, never used.
rm -rf "$T/dl3"; echo corrupt > "$T/cache/$GOOD"
if pin_fetch tool "$T/dl3" >/dev/null 2>&1; then bad "corrupt cache is not trusted"; else ok "corrupt cache is not trusted"; fi
[ ! -e "$T/cache/$GOOD" ] && ok "corrupt cache entry removed" || bad "corrupt cache entry removed"
mv "$T/up.off" "$T/up"; mv "$T/mirror.off" "$T/mirror"

# An already verified destination file is reused; a stale one is replaced.
echo stale > "$T/dl4.tar.gz"; mkdir -p "$T/dl4"; cp "$T/dl4.tar.gz" "$T/dl4/tool.tar.gz"
f=$(PIN_CACHE='' pin_fetch tool "$T/dl4" 2>/dev/null)
check "stale destination replaced" "real payload" "$(cat "$f" 2>/dev/null)"

pin_verify "$T/up/tool.tar.gz" "$GOOD" && ok "pin_verify accepts" || bad "pin_verify accepts"
if pin_verify "$T/up/evil.zip" "$GOOD" 2>/dev/null; then bad "pin_verify rejects"; else ok "pin_verify rejects"; fi

# Mirror marking and the mirror url.
cat > "$T/mpins" <<EOF
xidi https://github.com/samuelgr/Xidi/releases/download/v4.3.1/Xidi-v4.3.1.zip $GOOD mirror https://github.com/me/game/releases/download/thirdparty/Xidi-v4.3.1.zip
plain https://example.com/a/plain.tar.gz?raw=1 $GOOD
summed - $GOOD mirror
weird https://example.com/get/?id=7 $GOOD mirror
dgvoodoo https://example.com/dgVoodoo2_80.zip $GOOD hosted https://github.com/me/game/releases/download/thirdparty/dgVoodoo2_80.zip
both https://example.com/x.zip $GOOD mirror hosted
summedhosted - $GOOD hosted
EOF
(
  PIN_FILE=$T/mpins PIN_DIR='' GITHUB_REPOSITORY=me/game
  pin_mirrorable xidi && ok "mirror word marks a pin" || bad "mirror word marks a pin"
  if pin_mirrorable plain; then bad "unmarked pin is not mirrorable"; else ok "unmarked pin is not mirrorable"; fi
  check "mirror word is not a url" "https://github.com/samuelgr/Xidi/releases/download/v4.3.1/Xidi-v4.3.1.zip
https://github.com/me/game/releases/download/thirdparty/Xidi-v4.3.1.zip" "$(pin_urls xidi)"
  check "pin_sha with mirror word" "$GOOD" "$(pin_sha xidi)"
  check "pin_file: primary url's name" "Xidi-v4.3.1.zip" "$(pin_file xidi)"
  check "pin_file drops the query" "plain.tar.gz" "$(pin_file plain)"
  check "pin_file falls back to the pin name" "weird" "$(pin_file weird)"
  check "mirror url from GITHUB_REPOSITORY" "https://github.com/me/game/releases/download/thirdparty/Xidi-v4.3.1.zip" "$(pin_mirror_url xidi)"
  check "mirror url, explicit repo and tag" "https://github.com/o/r/releases/download/mirror-1/plain.tar.gz" "$(pin_mirror_url plain o/r mirror-1)"
  check "the listed fallback is exactly the mirror url" "$(pin_mirror_url xidi)" "$(pin_urls xidi | grep '/releases/download/thirdparty/')"
  if pin_sha summed 2>/dev/null; then bad "checksum-only pin cannot be mirrored"; else ok "checksum-only pin cannot be mirrored"; fi
  if pin_mirror_url xidi 'a/b/c' 2>/dev/null; then bad "bad repository refused"; else ok "bad repository refused"; fi
  if pin_mirror_url xidi o/r 'x y' 2>/dev/null; then bad "bad tag refused"; else ok "bad tag refused"; fi
  if (unset GITHUB_REPOSITORY PIN_MIRROR_REPO; pin_mirror_url xidi) 2>/dev/null; then bad "no repository refused"; else ok "no repository refused"; fi
  pin_hosted dgvoodoo && ok "hosted word marks a pin" || bad "hosted word marks a pin"
  if pin_mirrorable dgvoodoo; then bad "hosted is not mirrorable"; else ok "hosted is not mirrorable"; fi
  if pin_hosted xidi; then bad "mirror is not hosted"; else ok "mirror is not hosted"; fi
  if pin_hosted plain; then bad "unmarked pin is not hosted"; else ok "unmarked pin is not hosted"; fi
  check "hosted word is not a url" "https://example.com/dgVoodoo2_80.zip
https://github.com/me/game/releases/download/thirdparty/dgVoodoo2_80.zip" "$(pin_urls dgvoodoo)"
  check "hosted pin has a mirror url" "https://github.com/me/game/releases/download/thirdparty/dgVoodoo2_80.zip" "$(pin_mirror_url dgvoodoo)"
  if pin_sha both 2>/dev/null; then bad "mirror and hosted together refused"; else ok "mirror and hosted together refused"; fi
  if pin_sha summedhosted 2>/dev/null; then bad "checksum-only hosted pin refused"; else ok "checksum-only hosted pin refused"; fi
  echo "fails=$fails" > "$T/sub"
)
. "$T/sub"

# Per-pin mirror release: tag=RELEASE (CMR1's HD pack in hd-pack-2).
cat > "$T/tpins" <<EOF
hdpack https://example.com/hd-pack-2.zip $GOOD hosted tag=hd-pack-2
plain2 https://example.com/p.zip $GOOD mirror
twotags https://example.com/x.zip $GOOD tag=a tag=b
badtag https://example.com/x.zip $GOOD tag=a/b
emptytag https://example.com/x.zip $GOOD tag=
EOF
(
  PIN_FILE=$T/tpins PIN_DIR='' GITHUB_REPOSITORY=me/game
  check "pin_tag" "hd-pack-2" "$(pin_tag hdpack)"
  check "pin_tag: none" "" "$(pin_tag plain2)"
  check "tag= is not a url" "https://example.com/hd-pack-2.zip" "$(pin_urls hdpack)"
  check "the mirror url uses the pin's own release" "https://github.com/me/game/releases/download/hd-pack-2/hd-pack-2.zip" "$(pin_mirror_url hdpack)"
  check "...over PIN_MIRROR_TAG (mirror.yml's default)" "https://github.com/me/game/releases/download/hd-pack-2/hd-pack-2.zip" "$(PIN_MIRROR_TAG=thirdparty pin_mirror_url hdpack)"
  check "an explicit tag still wins" "https://github.com/me/game/releases/download/x/hd-pack-2.zip" "$(pin_mirror_url hdpack me/game x)"
  check "no tag=: PIN_MIRROR_TAG, else thirdparty" "https://github.com/me/game/releases/download/thirdparty/p.zip" "$(pin_mirror_url plain2)"
  pin_hosted hdpack && ok "tag= keeps the hosted mark" || bad "tag= keeps the hosted mark"
  for b in twotags badtag emptytag; do
    if pin_sha "$b" 2>/dev/null; then bad "malformed tag= refused ($b)"; else ok "malformed tag= refused ($b)"; fi
  done
  echo "fails=$fails" > "$T/sub"
)
. "$T/sub"

# Offline (REA_NO_FETCH=1): never the network; the cache and file:// urls
# still work.
mkdir -p "$T/fakebin"
printf '#!/bin/sh\nfor a; do case $a in http*) echo "$a" >> "%s/net.log" ;; esac; done\nexec %s "$@"\n' "$T" "$(command -v curl)" > "$T/fakebin/curl"
chmod +x "$T/fakebin/curl"
cat > "$T/opins" <<EOF
net https://example.invalid/tool.tar.gz $GOOD
mixed https://example.invalid/tool.tar.gz $GOOD file://$T/mirror/tool-mirror.tar.gz
EOF
(
  PATH=$T/fakebin:$PATH PIN_FILE=$T/opins PIN_DIR='' REA_NO_FETCH=1 PIN_CACHE=$T/ocache
  if f=$(pin_fetch net "$T/odl" 2>"$T/olog"); then bad "offline: a network-only pin fails"; else ok "offline: a network-only pin fails"; fi
  grep -q 'offline (REA_NO_FETCH=1): net is not in' "$T/olog" && ok "offline: says why" || bad "offline message: $(cat "$T/olog")"
  f=$(pin_fetch mixed "$T/odl" 2>/dev/null) && check "offline: a file:// fallback works" "real payload" "$(cat "$f")" || bad "offline: file:// fallback"
  rm -rf "$T/odl"
  f=$(pin_fetch net "$T/odl" 2>/dev/null) && ok "offline: the cache works (mixed filled it)" || bad "offline: from the cache"
  [ -e "$T/net.log" ] && bad "offline: the network was used: $(cat "$T/net.log")" || ok "offline: never the network"
  echo "fails=$fails" > "$T/sub"
)
. "$T/sub"

# The SDK's own tool pins parse.
PIN_FILE=$HERE/../appimage/tools.pins PIN_DIR=
for n in wine appimagetool appimage-runtime; do
  pin_sha "$n" >/dev/null && ok "tools.pins: $n" || bad "tools.pins: $n"
done
if pin_urls appimage-runtime | grep -q /continuous/; then bad "runtime pinned to a dated release"; else ok "runtime pinned to a dated release"; fi

[ "$fails" = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
