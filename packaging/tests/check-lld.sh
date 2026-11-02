#!/bin/sh
# Tests packaging/check-lld.sh: sh packaging/tests/check-lld.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd -P)
CHECK=$HERE/../check-lld.sh
T=$(mktemp -d "${TMPDIR:-/tmp}/rea-lld-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }
passes() { if sh "$CHECK" "$@" >"$T/out" 2>&1; then ok "$name"; else bad "$name: $(cat "$T/out")"; fi; }
fails_() { if sh "$CHECK" "$@" >"$T/out" 2>&1; then bad "$name (passed)"; else ok "$name"; fi; }
unset CARGO_TARGET_I686_PC_WINDOWS_GNU_RUSTFLAGS CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS

cat > "$T/both.toml" <<'TOML'
[target.i686-pc-windows-gnu]
linker = "i686-w64-mingw32-gcc"
rustflags = ["-C", "link-arg=-fuse-ld=lld"]

[target.x86_64-pc-windows-gnu]
linker = "x86_64-w64-mingw32-gcc"
rustflags = [ "-C", "link-arg=-fuse-ld=lld" ]
TOML
# The bug the Screamer pass found: lld for i686 only.
cat > "$T/i686.toml" <<'TOML'
[target.i686-pc-windows-gnu]
rustflags = ["-C", "link-arg=-fuse-ld=lld"]

[target.x86_64-pc-windows-gnu]
linker = "x86_64-w64-mingw32-gcc"
# rustflags = ["-C", "link-arg=-fuse-ld=lld"]   (commented out)
TOML
printf '[build]\nrustflags = ["-C", "link-arg=-fuse-ld=lld"]\n' > "$T/build.toml"

name="both targets set"; passes --config "$T/both.toml" i686-pc-windows-gnu x86_64-pc-windows-gnu
name="x86_64 missing fails"; fails_ --config "$T/i686.toml" x86_64-pc-windows-gnu
grep -q 'FAIL x86_64-pc-windows-gnu' "$T/out" && grep -q 'rustflags = \["-C", "link-arg=-fuse-ld=lld"\]' "$T/out" \
  && ok "the failure shows the fix" || bad "the failure shows the fix: $(cat "$T/out")"
name="i686 in the same file passes"; passes --config "$T/i686.toml" i686-pc-windows-gnu
name="a commented-out line does not count"; fails_ --config "$T/i686.toml" x86_64-pc-windows-gnu
name="lld under [build] only does not count for a target"; fails_ --config "$T/build.toml" i686-pc-windows-gnu
name="no config file fails"; fails_ --config "$T/none.toml" i686-pc-windows-gnu
name="the environment counts"; CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS='-C link-arg=-fuse-ld=lld' passes --config "$T/none.toml" x86_64-pc-windows-gnu
name="non-Windows targets pass"; passes --config "$T/none.toml" x86_64-unknown-linux-gnu
name="default config path"; (mkdir -p "$T/repo/.cargo" && cp "$T/both.toml" "$T/repo/.cargo/config.toml" && cd "$T/repo" && sh "$CHECK" i686-pc-windows-gnu) >/dev/null 2>&1 && ok "$name" || bad "$name"
name="the launcher template sets both targets"; passes --config "$HERE/../../templates/launcher/dot-cargo/config.toml" i686-pc-windows-gnu x86_64-pc-windows-gnu

[ "$fails" = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
