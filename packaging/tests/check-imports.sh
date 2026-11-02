#!/bin/sh
# Tests packaging/check-imports.sh on tiny synthetic PE32 and PE32+ images:
# one import descriptor, an IAT that is complete, or holds a named orphan
# (what GNU ld 2.47 produces), or an ordinal orphan:
#   sh packaging/tests/check-imports.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd -P)
CHECK=$HERE/../check-imports.sh
T=$(mktemp -d "${TMPDIR:-/tmp}/rea-imports-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }

# put FILE OFFSET VALUE WIDTH: little-endian integer; puts FILE OFFSET "text".
put() {
  v=$3 bytes=''
  i=0
  while [ "$i" -lt "$4" ]; do
    bytes="$bytes\\$(printf '%03o' $((v % 256)))"
    v=$((v / 256)); i=$((i + 1))
  done
  # shellcheck disable=SC2059 # the octal escapes are the format
  printf "$bytes" | dd of="$1" bs=1 seek="$2" conv=notrunc 2>/dev/null
}
puts() { printf '%s' "$3" | dd of="$1" bs=1 seek="$2" conv=notrunc 2>/dev/null; }

# image FILE WIDTH(4|8) IAT_SIZE SLOT2: section .idata at RVA 0x1000, file
# 0x400. Descriptor at 0x1000 (FirstThunk 0x1100); IAT slot 0 imports
# GetModuleHandleW, slot 1 ends the descriptor's run, slot 2 is SLOT2
# (0 = none, "name" = ExitProcess by name, "ordinal" = by ordinal).
image() {
  f=$1 w=$2
  dd if=/dev/zero of="$f" bs=4096 count=2 2>/dev/null
  puts "$f" 0 MZ
  put "$f" 60 128 4
  puts "$f" 128 PE
  put "$f" 134 1 2                           # one section
  opt=152
  if [ "$w" = 4 ]; then put "$f" 148 224 2; put "$f" $opt 267 2; dirs=$((opt + 96))
  else put "$f" 148 240 2; put "$f" $opt 523 2; dirs=$((opt + 112)); fi
  put "$f" $((dirs + 8)) 4096 4              # import directory RVA
  put "$f" $((dirs + 96)) 4352 4             # IAT RVA 0x1100
  put "$f" $((dirs + 100)) "$3" 4            # IAT size
  table=$((opt + (w == 4 ? 224 : 240)))
  puts "$f" "$table" .idata
  put "$f" $((table + 8)) 4096 4             # virtual size
  put "$f" $((table + 12)) 4096 4            # RVA 0x1000
  put "$f" $((table + 16)) 2048 4            # raw size
  put "$f" $((table + 20)) 1024 4            # raw offset 0x400
  off() { echo $(($1 - 4096 + 1024)); }
  put "$f" "$(off 4096)" 4608 4              # descriptor: OriginalFirstThunk (nonzero)
  put "$f" $(($(off 4096) + 12)) 4864 4      # Name
  put "$f" $(($(off 4096) + 16)) 4352 4      # FirstThunk 0x1100
  put "$f" "$(off 4352)" 4864 "$w"           # slot 0 -> hint/name at 0x1300
  case $4 in
    name) put "$f" $(($(off 4352) + 2 * w)) 4896 "$w" ;;    # slot 2 -> 0x1320
    ordinal) put "$f" $(($(off 4352) + 2 * w)) 5 "$w"
             put "$f" $(($(off 4352) + 3 * w - 1)) 128 1 ;; # high bit: by ordinal
  esac
  puts "$f" $(($(off 4864) + 2)) GetModuleHandleW
  puts "$f" $(($(off 4896) + 2)) ExitProcess
}

for w in 4 8; do
  kind=$([ "$w" = 4 ] && echo PE32 || echo PE32+)
  image "$T/good.exe" "$w" $((2 * w)) 0
  if sh "$CHECK" "$T/good.exe" >"$T/out" 2>&1; then ok "$kind: complete IAT passes"; else bad "$kind: complete IAT: $(cat "$T/out")"; fi
  image "$T/bad.exe" "$w" $((4 * w)) name
  if sh "$CHECK" "$T/bad.exe" >"$T/out" 2>&1; then bad "$kind: orphaned import fails"
  elif grep -qx '  ExitProcess' "$T/out" && ! grep -q 'GetModuleHandleW' "$T/out"; then ok "$kind: orphaned import fails, named"
  else bad "$kind: orphan: $(cat "$T/out")"; fi
  grep -q 'fuse-ld=lld' "$T/out" && ok "$kind: points at lld" || bad "$kind: points at lld"
  image "$T/ord.exe" "$w" $((4 * w)) ordinal
  if sh "$CHECK" "$T/ord.exe" >"$T/out" 2>&1; then ok "$kind: ordinal-only orphan ignored"; else bad "$kind: ordinal: $(cat "$T/out")"; fi
done
image "$T/good.exe" 4 8 0
if sh "$CHECK" "$T/good.exe" "$T/bad.exe" >/dev/null 2>&1; then bad "any bad file fails the run"; else ok "any bad file fails the run"; fi
echo 'not a program' > "$T/text.exe"
sh "$CHECK" "$T/text.exe" >/dev/null 2>&1; [ $? = 2 ] && ok "non-PE file: exit 2" || bad "non-PE file: exit 2"
if sh "$CHECK" >/dev/null 2>&1; then bad "no argument refused"; else ok "no argument refused"; fi

[ "$fails" = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
