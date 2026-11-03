#!/bin/sh
# Tests scripts/wine-test with fake cargo and wine on PATH (no Wine, no
# MinGW needed): sh scripts/tests/wine-test.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd -P)
WT=$HERE/../wine-test
T=$(mktemp -d "${TMPDIR:-/tmp}/rea-wine-test-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }
check() { if [ "$2" = "$3" ]; then ok "$1"; else bad "$1: want '$2', got '$3'"; fi; }

# Canned cargo output: one binary with normal failures, one that aborted
# (path with a space), one that passed. CRLF as Wine may print it.
EXE_ABORT="$T/target dir/deps/rea_launcher_win32-abc.exe"
printf '%s\r\n' \
  'running 3 tests' \
  'test cfg::parses ... ok' \
  'test ini::keeps_bom ... FAILED' \
  'test ini::trims_keys ... FAILED' \
  '' \
  'failures:' \
  '' \
  '---- ini::keeps_bom stdout ----' \
  "thread 'ini::keeps_bom' panicked at src/ini.rs:10:5:" \
  '    left: 1' \
  '' \
  'failures:' \
  '    ini::keeps_bom' \
  '    ini::trims_keys' \
  '' \
  'test result: FAILED. 1 passed; 2 failed; 0 ignored' \
  'error: test failed, to rerun pass `-p rea-launcher --lib`' \
  '' \
  'Caused by:' \
  "  process didn't exit successfully: \`wine $T/t/deps/rea_launcher-123.exe --test-threads=1\` (exit status: 101)" \
  'running 2 tests' \
  "  process didn't exit successfully: \`wine $EXE_ABORT --test-threads=1\` (exit code: 0xc0000409, STATUS_STACK_BUFFER_OVERRUN)" \
  'test result: ok. 4 passed; 0 failed' > "$T/canned.log"

check "names failures listed by a binary that exited normally" "FAIL ini::keeps_bom
FAIL ini::trims_keys
ABORTED $EXE_ABORT" "$(sh "$WT" --failures-from "$T/canned.log")"

# Fakes.
mkdir -p "$T/bin" "$T/ws"
cat > "$T/bin/cargo" <<'F'
#!/bin/sh
{ printf 'args:'; printf ' [%s]' "$@"; echo
  echo "runner_i686=${CARGO_TARGET_I686_PC_WINDOWS_GNU_RUNNER:-}"
  echo "runner_x86_64=${CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUNNER:-}"
  echo "prefix=$WINEPREFIX"; echo "target_dir=$CARGO_TARGET_DIR"
  echo "display=${DISPLAY:-}"; echo "lang=${LANG:-} lc_all=${LC_ALL:-}"; } > "$FAKE_RECORD"
case $FAKE_MODE in
  pass) echo 'test result: ok. 3 passed; 0 failed'; exit 0 ;;
  fail) cat "$FAKE_LOG"; exit 101 ;;
esac
F
cat > "$T/bin/wine" <<'F'
#!/bin/sh
echo "wine $*" >> "$WINE_LOG"
case $2 in
  --list) printf 'abi::one: test\r\nabi::two: test\r\nabi::bench: benchmark\r\n' ;;
  --exact) [ "$3" != abi::two ] ;;
  *) exit 0 ;;
esac
F
printf '#!/bin/sh\nexit 0\n' > "$T/bin/wineboot"; cp "$T/bin/wineboot" "$T/bin/wineserver"
chmod +x "$T/bin/"*
export PATH="$T/bin:$PATH" FAKE_RECORD="$T/record" FAKE_LOG="$T/canned.log" DISPLAY=:9 WINE_LOG="$T/wine.log"
unset REA_WINE_TARGET REA_WINE_TEST_PREFIX CARGO_TARGET_DIR

run() { (cd "$T/ws" && FAKE_MODE=$1 && export FAKE_MODE && shift && sh "$WT" "$@") >"$T/out" 2>"$T/err"; }
if run pass --locked --workspace; then ok "passing run exits 0"; else bad "passing run exits 0: $(cat "$T/err")"; fi
check "default target, no-fail-fast, one thread" \
  "args: [test] [--target] [i686-pc-windows-gnu] [--no-fail-fast] [--locked] [--workspace] [--] [--test-threads=1]" \
  "$(sed -n 1p "$T/record")"
grep -qx 'runner_i686=wine' "$T/record" && ok "i686 runner is wine" || bad "i686 runner is wine"
grep -qx "prefix=$T/ws/target/wine/prefix" "$T/record" && ok "prefix inside the workspace, absolute" || bad "prefix: $(grep prefix "$T/record")"
grep -qx "target_dir=$T/ws/target/wine" "$T/record" && ok "target dir absolute" || bad "target dir: $(grep target_dir "$T/record")"
grep -qx 'display=' "$T/record" && ok "headless: DISPLAY cleared" || bad "headless: DISPLAY cleared"
[ -f "$T/ws/target/wine/prefix/.rea-headless" ] && ok "prefix prepared once" || bad "prefix prepared once"
grep -qF 'wine reg add HKCU\Software\Wine\Drivers /v Graphics /t REG_SZ /d null /f' "$T/wine.log" \
  && ok "new prefix uses the null graphics driver" || bad "null driver: $(cat "$T/wine.log")"
: > "$T/wine.log"
run pass --workspace
if grep -q 'reg add' "$T/wine.log"; then bad "registry set only once per prefix"; else ok "registry set only once per prefix"; fi
# A prefix from an older wine-test (empty marker) gets the null driver once.
mkdir -p "$T/old/prefix"; : > "$T/old/prefix/.rea-headless"; : > "$T/wine.log"
(cd "$T/ws" && FAKE_MODE=pass REA_WINE_TEST_PREFIX="$T/old/prefix" sh "$WT" >/dev/null 2>&1)
grep -q 'Drivers /v Graphics' "$T/wine.log" && ! grep -q 'WineDbg' "$T/wine.log" \
  && ok "older prefix upgraded with the null driver only" || bad "older prefix: $(cat "$T/wine.log")"
grep -qx 'graphics=null' "$T/old/prefix/.rea-headless" && ok "marker records the upgrade" || bad "marker records the upgrade"

run pass --target x86_64-pc-windows-gnu -p rea-launcher -- --nocapture
check "--target x86_64, test args kept after --" \
  "args: [test] [--target] [x86_64-pc-windows-gnu] [--no-fail-fast] [-p] [rea-launcher] [--] [--nocapture] [--test-threads=1]" \
  "$(sed -n 1p "$T/record")"
grep -qx 'runner_x86_64=wine' "$T/record" && ok "x86_64 runner is wine" || bad "x86_64 runner is wine"
(cd "$T/ws" && FAKE_MODE=pass REA_WINE_TARGET=x86_64-pc-windows-gnu sh "$WT" >/dev/null 2>&1)
grep -q 'x86_64-pc-windows-gnu' "$T/record" && ok "REA_WINE_TARGET picks the target" || bad "REA_WINE_TARGET picks the target"
(cd "$T/ws" && FAKE_MODE=pass REA_WINE_TEST_PREFIX=rel/prefix sh "$WT" >/dev/null 2>&1)
grep -qx "prefix=$T/ws/rel/prefix" "$T/record" && ok "relative prefix made absolute" || bad "relative prefix made absolute"
if run pass --target aarch64-pc-windows-msvc; then bad "unsupported target refused"; else ok "unsupported target refused"; fi
if run pass --target; then bad "--target without a value refused"; else ok "--target without a value refused"; fi

if run fail --workspace; then bad "failing run exits non-zero"; else ok "failing run exits non-zero"; fi
check "every failing test named (listed and from the aborted binary)" "FAIL ini::keeps_bom
FAIL ini::trims_keys
FAIL abi::two" "$(grep '^FAIL' "$T/out")"

# The locale is C.UTF-8 whatever the caller's.
(cd "$T/ws" && FAKE_MODE=pass LANG='' LC_ALL=POSIX sh "$WT" --workspace >/dev/null 2>&1)
grep -qx 'lang=C.UTF-8 lc_all=C.UTF-8' "$T/record" && ok "LANG and LC_ALL are C.UTF-8" || bad "locale: $(grep lang "$T/record")"

# Host-only crates opt out with [package.metadata.rea] wine-test = false.
mkdir -p "$T/ws/crates/hooks" "$T/ws/crates/game" "$T/ws/target/x" "$T/ws/.work/y"
printf '[package]\nname = "cmr-hooks"\n\n[package.metadata.rea]\nwine-test = false\n' > "$T/ws/crates/hooks/Cargo.toml"
printf '[package]\nname = "game"\n\n[package.metadata.rea]\nwine-test = true\n' > "$T/ws/crates/game/Cargo.toml"
printf '[package]\nname = "stale"\n[package.metadata.rea]\nwine-test = false\n' > "$T/ws/target/x/Cargo.toml"
cp "$T/ws/target/x/Cargo.toml" "$T/ws/.work/y/Cargo.toml"
run pass --locked --workspace
check "an opted-out crate is excluded from --workspace" \
  "args: [test] [--target] [i686-pc-windows-gnu] [--no-fail-fast] [--locked] [--workspace] [--exclude] [cmr-hooks] [--] [--test-threads=1]" \
  "$(sed -n 1p "$T/record")"
grep -q 'leaving out cmr-hooks' "$T/err" && ok "and says so" || bad "no note: $(cat "$T/err")"
run pass --workspace -- --nocapture
check "...also with test arguments" \
  "args: [test] [--target] [i686-pc-windows-gnu] [--no-fail-fast] [--workspace] [--exclude] [cmr-hooks] [--] [--nocapture] [--test-threads=1]" \
  "$(sed -n 1p "$T/record")"
run pass -p game
check "without --workspace nothing is excluded" \
  "args: [test] [--target] [i686-pc-windows-gnu] [--no-fail-fast] [-p] [game] [--] [--test-threads=1]" \
  "$(sed -n 1p "$T/record")"
run pass --workspace --exclude other
check "an explicit --exclude is kept" \
  "args: [test] [--target] [i686-pc-windows-gnu] [--no-fail-fast] [--workspace] [--exclude] [other] [--exclude] [cmr-hooks] [--] [--test-threads=1]" \
  "$(sed -n 1p "$T/record")"
rm -rf "$T/ws/crates" "$T/ws/.work"

[ "$fails" = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
