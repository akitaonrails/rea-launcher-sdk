# rea-launcher-sdk packaging/pins.sh: pinned, checksum-verified downloads.
# POSIX sh library; source it, don't run it.
#
# A pin file holds one download per line (blank lines and # comments are
# skipped):
#
#   name url sha256 [fallback-url ...]
#
# The first url whose download matches sha256 wins; later urls are mirrors
# for the day an upstream moves or disappears. A url of "-" records a
# checksum only (e.g. one file inside an archive): pin_sha works, pin_fetch
# refuses. Per-name files also work: $PIN_DIR/<name>.txt with one line
# "url sha256 [fallback-url ...]".
#
# Settings, read on every call:
#   PIN_FILE   pin file, looked up first
#   PIN_DIR    folder of <name>.txt pins, looked up second
#   PIN_CACHE  download cache keyed by sha256 (default
#              ${XDG_CACHE_HOME:-~/.cache}/rea-launcher-sdk/pins; empty: none)
#   PIN_RETRY  curl --retry count (default 3)
#
# Functions return non-zero with a message on stderr; they never exit.
#   pin_sha NAME            the sha256
#   pin_url NAME            the primary url
#   pin_urls NAME           every url, primary first, one per line
#   pin_names               every name in PIN_FILE and PIN_DIR
#   pin_fetch NAME DEST_DIR downloads into DEST_DIR, prints the file's path
#   pin_verify FILE SHA256  checks a file's digest

# Prints "sha256" then each url (primary first; "-" when checksum-only).
_pin_record() {
  case $1 in
    '' | *[!A-Za-z0-9._-]*) echo "pins: bad pin name '$1'" >&2; return 1 ;;
  esac
  _pin_src=
  _pin_rec=
  if [ -n "${PIN_FILE:-}" ]; then
    [ -f "$PIN_FILE" ] || { echo "pins: no pin file $PIN_FILE" >&2; return 1; }
    _pin_rec=$(awk -v n="$1" '
      $1 == n { c++; l = $2; for (i = 3; i <= NF; i++) l = l " " $i }
      END { if (c > 1) exit 3; if (c) print l }' "$PIN_FILE") \
      || { echo "pins: $1 is pinned more than once in $PIN_FILE" >&2; return 1; }
    _pin_src=$PIN_FILE
  fi
  if [ -z "$_pin_rec" ] && [ -n "${PIN_DIR:-}" ] && [ -f "$PIN_DIR/$1.txt" ]; then
    _pin_rec=$(awk '!/^[ \t]*(#|$)/ { print; exit }' "$PIN_DIR/$1.txt")
    _pin_src=$PIN_DIR/$1.txt
  fi
  [ -n "$_pin_rec" ] || { echo "pins: no pin named $1" >&2; return 1; }
  # Validate: url (or -), 64-hex sha256, then only urls.
  printf '%s\n' "$_pin_rec" | awk '{
    if (NF < 2 || $2 !~ /^[0-9a-f]+$/ || length($2) != 64) exit 1
    if ($1 != "-" && $1 !~ /:\/\//) exit 1
    for (i = 3; i <= NF; i++) if ($i !~ /:\/\//) exit 1
    print $2; print $1
    for (i = 3; i <= NF; i++) print $i
  }' || {
    echo "pins: malformed pin $1 in $_pin_src (want: url sha256 [fallback-url ...])" >&2
    return 1
  }
}

pin_sha() {
  _pin_r=$(_pin_record "$1") || return 1
  printf '%s\n' "$_pin_r" | sed -n 1p
}

pin_url() {
  _pin_r=$(_pin_record "$1") || return 1
  printf '%s\n' "$_pin_r" | sed -n 2p
}

pin_urls() {
  _pin_r=$(_pin_record "$1") || return 1
  printf '%s\n' "$_pin_r" | sed -n '2,$p' | grep -v '^-$' || true
}

pin_names() {
  if [ -n "${PIN_FILE:-}" ] && [ -f "$PIN_FILE" ]; then
    awk '!/^[ \t]*(#|$)/ { print $1 }' "$PIN_FILE"
  fi
  if [ -n "${PIN_DIR:-}" ] && [ -d "$PIN_DIR" ]; then
    for _pin_f in "$PIN_DIR"/*.txt; do
      [ -f "$_pin_f" ] || continue
      _pin_f=${_pin_f##*/}
      printf '%s\n' "${_pin_f%.txt}"
    done
  fi
}

_pin_sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

pin_verify() {
  [ -f "$1" ] || { echo "pins: no file $1" >&2; return 1; }
  _pin_got=$(_pin_sha256 "$1") || return 1
  [ "$_pin_got" = "$2" ] && return 0
  echo "pins: sha256 mismatch for $1: want $2, got $_pin_got" >&2
  return 1
}

pin_fetch() {
  [ $# -eq 2 ] || { echo "usage: pin_fetch NAME DEST_DIR" >&2; return 2; }
  _pin_r=$(_pin_record "$1") || return 1
  _pin_sha=$(printf '%s\n' "$_pin_r" | sed -n 1p)
  _pin_primary=$(printf '%s\n' "$_pin_r" | sed -n 2p)
  [ "$_pin_primary" != - ] || { echo "pins: $1 is checksum-only; nothing to fetch" >&2; return 1; }
  # The file keeps the primary url's name whichever mirror served it.
  _pin_file=${_pin_primary%%[?#]*}
  _pin_file=${_pin_file##*/}
  case $_pin_file in '' | . | .. | *[!A-Za-z0-9._+-]*) _pin_file=$1 ;; esac
  mkdir -p "$2" || return 1
  _pin_dest=$2/$_pin_file
  _pin_cache=${PIN_CACHE-${XDG_CACHE_HOME:-$HOME/.cache}/rea-launcher-sdk/pins}

  if [ -f "$_pin_dest" ] && [ "$(_pin_sha256 "$_pin_dest")" = "$_pin_sha" ]; then
    printf '%s\n' "$_pin_dest"; return 0
  fi
  if [ -n "$_pin_cache" ] && [ -f "$_pin_cache/$_pin_sha" ]; then
    if [ "$(_pin_sha256 "$_pin_cache/$_pin_sha")" = "$_pin_sha" ]; then
      rm -f "$_pin_dest"
      ln "$_pin_cache/$_pin_sha" "$_pin_dest" 2>/dev/null \
        || cp "$_pin_cache/$_pin_sha" "$_pin_dest" || return 1
      echo "pins: $1 from cache" >&2
      printf '%s\n' "$_pin_dest"; return 0
    fi
    echo "pins: dropping corrupt cache entry for $1" >&2
    rm -f "$_pin_cache/$_pin_sha"
  fi

  _pin_part=$2/.$_pin_file.part.$$
  while IFS= read -r _pin_u; do
    [ "$_pin_u" != - ] || continue
    echo "pins: $1 <- $_pin_u" >&2
    rm -f "$_pin_part"
    if curl -fsSL --proto '=https,file' --proto-redir '=https' \
      --retry "${PIN_RETRY:-3}" --retry-delay 2 -o "$_pin_part" "$_pin_u" \
      && pin_verify "$_pin_part" "$_pin_sha"; then
      mv -f "$_pin_part" "$_pin_dest" || { rm -f "$_pin_part"; return 1; }
      if [ -n "$_pin_cache" ] && mkdir -p "$_pin_cache" 2>/dev/null; then
        cp "$_pin_dest" "$_pin_cache/$_pin_sha.$$" 2>/dev/null \
          && mv -f "$_pin_cache/$_pin_sha.$$" "$_pin_cache/$_pin_sha" \
          || rm -f "$_pin_cache/$_pin_sha.$$"
      fi
      printf '%s\n' "$_pin_dest"; return 0
    fi
    echo "pins: unavailable or wrong checksum: $_pin_u" >&2
  done <<EOF
$(printf '%s\n' "$_pin_r" | sed -n '2,$p')
EOF
  rm -f "$_pin_part"
  echo "pins: no source for $1 matched sha256 $_pin_sha" >&2
  return 1
}
