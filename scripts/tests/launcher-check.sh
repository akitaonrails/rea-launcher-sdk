#!/bin/sh
# Tests scripts/launcher-check: a fixture launcher that follows every rule
# (scripts/tests/fixtures/launcher-check/good), then one copy per mistake
# the family made, each of which must fail with its rule. Also: waivers,
# --explain/--list, GitHub annotations, the version gate, read-only.
#   sh scripts/tests/launcher-check.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd -P)
ROOT=$(cd "$HERE/../.." && pwd -P)
CHECK=$ROOT/scripts/launcher-check
GOOD=$HERE/fixtures/launcher-check/good
T=$(mktemp -d "${TMPDIR:-/tmp}/rea-launcher-check-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
fails=0 n=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }

fresh() { # -> a copy of the good fixture in $L
  n=$((n + 1)); L=$T/l$n
  cp -R "$GOOD" "$L"
}
run() { # args -> $out, $rc
  out=$(sh "$CHECK" "$@" 2>&1); rc=$?
}
expect() { # name level rule: the check fails (error) or passes with a warning
  name=$1 level=$2 rule=$3
  run "$L"
  case $level in
    error) want_rc=1 ;;
    *) want_rc=0 ;;
  esac
  if [ "$rc" -eq "$want_rc" ] && printf '%s\n' "$out" | grep -q "^$level\[$rule\] "; then
    ok "$name"
  else
    bad "$name (rc $rc, want ${level}[$rule]): $out"
  fi
}
replace() { # file old new: literal, first match per line
  awk -v o="$2" -v r="$3" '{ if ((p = index($0, o)) > 0) $0 = substr($0, 1, p - 1) r substr($0, p + length(o)); print }' "$1" > "$1.new" && mv "$1.new" "$1"
}
drop_line() { # file text
  grep -vF -- "$2" "$1" > "$1.new"; mv "$1.new" "$1"
}

# --- the good fixture ---------------------------------------------------------------
run "$GOOD"
if [ "$rc" -eq 0 ] && printf '%s\n' "$out" | grep -q '^launcher-check: 0 error(s), 0 warning(s)'; then
  ok "a launcher that follows every rule passes clean"
else
  bad "good fixture: $out"
fi
before=$(cd "$GOOD" && find . -type f -exec cksum {} + | LC_ALL=C sort)
run "$GOOD"
after=$(cd "$GOOD" && find . -type f -exec cksum {} + | LC_ALL=C sort)
[ "$before" = "$after" ] && [ -z "$(cd "$GOOD" && find . -newer "$CHECK" -name '*.tmp')" ] \
  && ok "read-only: the repository is unchanged" || bad "the check changed the repository"

# --- golden snapshots -----------------------------------------------------------------
fresh; rm "$L/crates/fx-launcher/tests/snapshots.rs"
expect "no tests/snapshots.rs (DiRT Rally)" error snapshots
fresh; printf 'FXLauncher 0.7.3\n' > "$L/crates/fx-launcher/tests/snapshots/help.txt"
expect "a snapshot that holds the crate version" error snapshot-version
fresh; printf 'FXLauncher 10.7.3 and 0.7.30\n' > "$L/crates/fx-launcher/tests/snapshots/help.txt"
run "$L"
printf '%s\n' "$out" | grep -q 'snapshot-version' && bad "a longer version is not the crate's: $out" \
  || ok "a longer version (10.7.3, 0.7.30) is not the crate's"
fresh; replace "$L/crates/fx-launcher/tests/snapshots.rs" 'rea_launcher::snapshot!("help.txt", String::new());' 'assert_eq!(1, 1);'
expect "snapshots compared by hand" warn snapshot-helper

# --- who sees a message ---------------------------------------------------------------
fresh; cat >> "$L/crates/fx-launcher/src/main.rs" <<'EOF'

fn interactive() -> bool {
    !std::env::args().any(|arg| arg == "--no-window")
}
EOF
expect "interactive = !no_window (CMR3, CMR4, DiRT)" error audience
fresh; cat >> "$L/crates/fx-launcher/src/main.rs" <<'EOF'

impl Invocation {
    pub fn interactive(&self) -> bool {
        !self.linux
    }
}
EOF
expect "interactive = !self.linux (CMR2005, DiRT Rally)" error audience
fresh; replace "$L/crates/fx-launcher/src/main.rs" 'report::audience(Run::from_args(&args, &[]))' 'Mode::Play { interactive: !no_window }'
expect "Mode built by hand from --no-window" error audience
fresh; printf 'fn say() { let _ = console::output(attached, true, interactive); }\n' >> "$L/crates/fx-launcher/src/main.rs"
expect "console::output for player messages" error audience
fresh; replace "$L/crates/fx-launcher/src/main.rs" 'use rea_launcher_win32::report::{self, Run};' 'use std::fmt;'
replace "$L/crates/fx-launcher/src/main.rs" 'report::audience(Run::from_args(&[] as &[&str], &[]))' '0'
expect "no SDK audience at all" error audience
fresh; printf 'tag=v0.3.3\ncommit=0123456789abcdef0123456789abcdef01234567\n' > "$L/sdk/VERSION"
for f in Cargo.toml .github/workflows/build.yml .github/workflows/sdk-update.yml .github/workflows/watch-upstream.yml; do replace "$L/$f" v0.4.0 v0.3.3; replace "$L/$f" v0.4.0 v0.3.3; done
printf '\nfn interactive() -> bool {\n    !no_window()\n}\n' >> "$L/crates/fx-launcher/src/main.rs"
expect "before the SDK has the helper, the same mistake only warns" warn audience
printf '%s\n' "$out" | grep -q 'an error once the launcher is on SDK v0.4.0' \
  && ok "the warning says when it becomes an error" || bad "no version note: $out"

# --- one instance -----------------------------------------------------------------------
fresh; replace "$L/crates/fx-launcher/src/main.rs" 'instance::single("rea-fx")' 'Ok::<(), ()>(())'
replace "$L/crates/fx-launcher/src/main.rs" 'use rea_launcher_win32::instance::{self, Window};' ''
expect "no single-instance guard" error instance
fresh; cat > "$L/.github/workflows/build.yml" <<'EOF'
jobs:
  linux-packages:
    uses: akitaonrails/rea-launcher-sdk/.github/workflows/launcher-ci.yml@v0.4.0
    with:
      rust-target: ""
      package-command: packaging/package.sh dist/FX
  windows-tests:
    uses: akitaonrails/rea-launcher-sdk/.github/workflows/launcher-ci.yml@v0.4.0
    with:
      rust-target: i686-pc-windows-gnu
      wine-tests: true
  release:
    uses: akitaonrails/rea-launcher-sdk/.github/workflows/launcher-release.yml@v0.4.0
    with:
      package: fx-launcher
EOF
expect "native Linux build without the unix lock (DiRT Rally)" error instance-unix
printf '%s\n' "$out" | grep -q 'error\[native-report\]' && ok "... and without the native error window" \
  || bad "native-report not reported: $out"
printf '%s\n' "$out" | grep -q '\[instance\]' && bad "Windows tests only: no Windows program ships, no mutex needed: $out" \
  || ok "a Windows-tests-only job ships no Windows program"
fresh; printf 'fn f() { running.bring_to_front(Window::Title(TITLE)); }\n' >> "$L/crates/fx-launcher/src/main.rs"
expect "second start by window title (CMR4)" error window-lookup

# --- small family rules -------------------------------------------------------------------
fresh; printf 'fn f() { let _ = std::fs::File::open("/dev/input/js0"); }\n' >> "$L/crates/fx-launcher/src/main.rs"
expect "a local /dev/input/js reader" warn native-pad
fresh; printf 'fn f() { let _ = gpu::prefer_glx(); }\n' >> "$L/crates/fx-launcher/src/main.rs"
expect "prefer_glx without the restart" warn glx-restart
fresh; printf 'fn light_widgets(v: &mut Visuals) { v.widgets.inactive.weak_bg_fill = PANEL; }\n' >> "$L/crates/fx-launcher/src/main.rs"
expect "a hand-made light widget fill (CMR2005)" warn light-widgets
fresh; printf 'd3dx9_43.dll x\n' >> "$L/packaging/package.allow"
expect "a bundled Microsoft DLL without its override" warn ms-dll-overrides
printf 'const OVERRIDES: &[&str] = &["d3dx9_43"];\n' >> "$L/crates/fx-launcher/src/main.rs"
run "$L"; printf '%s\n' "$out" | grep -q ms-dll-overrides && bad "an overridden DLL still reported" || ok "an overridden DLL passes"

# --- own fixes have drafts -------------------------------------------------------------------
fresh; replace "$L/docs/RESEARCH.md" '(draft C1)' 'LD_PRELOAD shim'
expect "an own fix that cites no draft (DiRT Rally)" error own-fix-draft
fresh; printf '| Id | Finding | Target | Evidence | Status |\n|---|---|---|---|---|\n| | | | | draft / posted (link) |\n' > "$L/docs/CONTRIBUTIONS.md"
expect "own fixes and an empty drafts table" error own-fix-draft
printf '%s\n' "$out" | grep -q 'drafts table is empty' && ok "... says the table is empty" || bad "empty table not named: $out"
fresh; replace "$L/docs/RESEARCH.md" '(draft C1)' '(no draft: the owner keeps it private)'
printf '# Contributions back\n' > "$L/docs/CONTRIBUTIONS.md"
run "$L"; [ "$rc" -eq 0 ] && ok "\"no draft: reason\" is accepted" || bad "no draft: $out"
fresh; printf '| 2 | PCGamingWiki | The patch | RESEARCH.md #2 | draft |\n' > "$L/docs/c"; cat "$L/docs/c" >> "$L/docs/CONTRIBUTIONS.md"
replace "$L/docs/RESEARCH.md" '(draft C1)' ''
replace "$L/docs/CONTRIBUTIONS.md" '| C1 |' '| x |'
run "$L"; [ "$rc" -eq 0 ] && ok "a draft that cites RESEARCH.md #2 covers row 2" || bad "RESEARCH.md #N: $out"
fresh; printf '[package]\nname = "fx-hooks"\nversion = "0.1.0"\n[lib]\ncrate-type = ["cdylib"]\n' > "$L/crates/fx-launcher/Hooks.toml"
mkdir -p "$L/crates/fx-hooks"; mv "$L/crates/fx-launcher/Hooks.toml" "$L/crates/fx-hooks/Cargo.toml"
printf '# Contributions back\n' > "$L/docs/CONTRIBUTIONS.md"
replace "$L/docs/RESEARCH.md" '**Our own** byte patch (draft C1)' 'Bundle'
expect "a hook library of our own and no drafts (CMR1)" error own-fix-draft

# --- README, AGENTS --------------------------------------------------------------------------
fresh; drop_line "$L/README.md" '## Known limitations'; drop_line "$L/README.md" '## Built on rea-launcher-sdk'
expect "README sections missing" error readme-sections
printf '%s\n' "$out" | grep -q 'Known limitations, ## Built on rea-launcher-sdk' && ok "... names each one" || bad "sections not named: $out"
fresh; replace "$L/README.md" '(https://github.com/akitaonrails/rea-launcher-sdk/issues)' '(https://example.com)'
expect "Contributing does not route shared code to the SDK" error readme-contributing
fresh; replace "$L/README.md" 'template=unknown-edition.yml' 'template=bug.yml'
expect "an issue link to a template that doesn't exist" error readme-links
fresh; printf '\n[zip](https://github.com/x/fx/releases/download/v0.7.3/FX.zip)\n' >> "$L/README.md"
expect "a versioned download link" warn readme-versions
fresh; awk '/^## Screenshots/ { hold = 1 } hold && /^## What you need/ { hold = 0 } !hold' "$GOOD/README.md" > "$L/README.md"
printf '## Screenshots\n\nLate.\n' >> "$L/README.md"
expect "sections out of order" warn readme-order
fresh; printf '# Fixture\n\nRead the standards.\n' > "$L/AGENTS.md"
expect "AGENTS.md without the sdk/docs pointer" error agents-pointer
fresh; cp "$ROOT/docs/STANDARDS.md" "$L/sdk/docs/STANDARDS.md"; sed -n '6,60p' "$ROOT/docs/STANDARDS.md" >> "$L/AGENTS.md"
expect "AGENTS.md copies STANDARDS" error agents-duplicate

# --- pins, workflows, linker -------------------------------------------------------------------
fresh; replace "$L/.github/workflows/build.yml" 'launcher-ci.yml@v0.4.0' 'launcher-ci.yml@v0.3.3'
expect "a workflow at another tag than sdk/VERSION" error sdk-tag
fresh; replace "$L/Cargo.toml" 'tag = "v0.4.0"' 'tag = "v0.3.0"'
expect "a Cargo tag other than sdk/VERSION" error sdk-tag
fresh; printf 'tag=untagged\ncommit=abc\n' > "$L/sdk/VERSION"
expect "sdk/ vendored from an untagged checkout" error sdk-tag
fresh; replace "$L/.github/workflows/build.yml" 'akitaonrails/rea-launcher-sdk/.github/workflows/launcher-release.yml@v0.4.0' './.github/workflows/release.yml'
expect "no SDK release workflow" error sdk-workflows
fresh; printf '[target.i686-pc-windows-gnu]\nlinker = "i686-w64-mingw32-gcc"\n' > "$L/.cargo/config.toml"
expect "i686 without lld (Screamer, SRR)" error lld
fresh; replace "$L/.github/workflows/build.yml" 'rust-target: i686-pc-windows-gnu' 'rust-target: x86_64-pc-windows-gnu'
expect "x86_64 target not in config.toml" error lld
fresh; drop_line "$L/.github/workflows/build.yml" 'wine-tests: true'
expect "Windows code never run under Wine" error wine-tests

# --- options -------------------------------------------------------------------------------------
fresh; replace "$L/crates/fx-launcher/src/config.rs" '"look",' '"preset",'
expect "no --look (CMR3, Screamer)" error look-flag
fresh; drop_line "$L/linux/fx-play" 'REA_LOOK_SIZES='
expect "looks don't size gamescope (CMR1, CMR4, OR, CMR2005)" error look-sizes
fresh; replace "$L/crates/fx-launcher/tests/snapshots.rs" '"--frame-cap"' '"--other"'
expect "an option no test names" error option-tests
printf '%s\n' "$out" | grep -q 'frame_cap' && ok "... names the option" || bad "option not named: $out"

# --- runtime notes, GitHub files -------------------------------------------------------------------
fresh; drop_line "$L/docs/RUNTIME_NOTES.md" 'Alt+F4'; drop_line "$L/docs/RUNTIME_NOTES.md" 'Leftover'
expect "quit paths not recorded" warn quit-paths
printf '%s\n' "$out" | grep -q 'window close / Alt+F4, no leftover processes' && ok "... names the missing ones" || bad "quit paths: $out"
fresh; rm "$L/.github/ISSUE_TEMPLATE/unknown-edition.yml"
expect "no unknown-edition form" error issue-unknown-edition
fresh; printf 'name: Bug report\nbody: []\n' > "$L/.github/ISSUE_TEMPLATE/bug-report.yml"
expect "bug report without a What worked checklist" warn issue-bug-report
fresh; rm "$L/.github/workflows/sdk-update.yml"
expect "no weekly SDK update" warn sdk-update

# --- Windows command line ---------------------------------------------------------------------------
fresh; drop_line "$L/crates/fx-launcher/src/main.rs" 'windows_subsystem'
expect "a console-subsystem Windows build (ScreamerLauncher 0.8.0)" error windows-subsystem
on_v042() { printf 'tag=v0.4.2\ncommit=0123456789abcdef0123456789abcdef01234567\n' > "$L/sdk/VERSION"
  for f in Cargo.toml .github/workflows/build.yml .github/workflows/sdk-update.yml .github/workflows/watch-upstream.yml; do replace "$L/$f" v0.4.0 v0.4.2; replace "$L/$f" v0.4.0 v0.4.2; done; }
fresh; on_v042; replace "$L/crates/fx-launcher/src/main.rs" 'Run::bad_command_line(&args)' 'Run::from_args(&args, &[])'
expect "a parse error reported as a play run (a modal box from PowerShell: CMR, CMR4, DiRT, SRR)" error bad-command-line
fresh; replace "$L/crates/fx-launcher/src/main.rs" 'Run::bad_command_line(&args)' 'Run::from_args(&args, &[])'
expect "on an older SDK the same mistake only warns" warn bad-command-line
fresh; on_v042; replace "$L/crates/fx-launcher/src/main.rs" 'second_start(action,' 'second_start(false,'
expect "a literal action at the second start (SRR's --setup exited 0)" error action-flags
fresh; on_v042; run "$L"
[ "$rc" -eq 0 ] && ok "the good fixture on v0.4.2 passes" || bad "good on v0.4.2: $out"

# --- antivirus: resources, no injection -------------------------------------------------------------
on_v043() { printf 'tag=v0.4.3\ncommit=0123456789abcdef0123456789abcdef01234567\n' > "$L/sdk/VERSION"
  for f in Cargo.toml .github/workflows/build.yml .github/workflows/sdk-update.yml .github/workflows/watch-upstream.yml; do replace "$L/$f" v0.4.0 v0.4.3; replace "$L/$f" v0.4.0 v0.4.3; done; }
fresh; on_v043; rm "$L/crates/fx-launcher/build.rs"
expect "a Windows exe without version info, icon or manifest (CMRLauncher 0.9.0)" error exe-resources
fresh; rm "$L/crates/fx-launcher/build.rs"
expect "on an older SDK the missing resources only warn" warn exe-resources
for call in 'CreateRemoteThread(process, null(), 0, start, arg, 0, null_mut());' 'WriteProcessMemory(process, at, bytes, len, &mut done);' 'VirtualAllocEx(process, null(), size, MEM_COMMIT, PAGE_EXECUTE_READWRITE);' 'let _hook = game.message_hook(&dll, c"close_hook");'; do
  fresh; on_v043; printf 'fn inject() { unsafe { %s } }\n' "$call" >> "$L/crates/fx-launcher/src/main.rs"
  expect "injecting into the running game: ${call%%(*}" error remote-injection
done
fresh; on_v043; printf '// WriteProcessMemory is never used here\n' >> "$L/crates/fx-launcher/src/main.rs"
run "$L"; printf '%s\n' "$out" | grep -q 'remote-injection' && bad "a comment naming the call is not injection: $out" || ok "a comment naming the call is not injection"
fresh; on_v043; run "$L"
[ "$rc" -eq 0 ] && ok "the good fixture on v0.4.3 passes" || bad "good on v0.4.3: $out"

# --- registry: never the VirtualStore ----------------------------------------------------------------
fresh; printf 'fn registry() { let (key, place) = reg::Key::machine_or_virtual_store(r"SOFTWARE\\Sega\\SEGA Rally").unwrap(); }\n' >> "$L/crates/fx-launcher/src/main.rs"
expect "the game's values through the VirtualStore (CMR1, rea-colin-mcrae#6)" warn virtual-store
printf '%s\n' "$out" | grep -q 'REA_REGISTRY' && ok "the warning names the redirect" || bad "virtual-store names the redirect: $out"
fresh; printf '// machine_or_virtual_store() was here before v0.6.0\n' >> "$L/crates/fx-launcher/src/main.rs"
run "$L"; printf '%s\n' "$out" | grep -q 'virtual-store' && bad "a comment is not a call: $out" || ok "a comment naming machine_or_virtual_store is not a call"

# --- release notes, third-party pins ----------------------------------------------------------------
fresh; printf '      notes: generate\n' >> "$L/.github/workflows/build.yml"
expect "notes: generate (CMR4 v0.4.0 had only GitHub's commit list)" error release-notes
fresh; replace "$L/packaging/third-party.txt" 'https://github.com/upstream/fixer/releases/download/v1.0/fixer-1.0.zip' 'https://github.com/upstream/fixer/archive/0123abcd.tar.gz'
expect "a watched pin of a commit archive (CMR3's vkBasalt)" error watch-pins
fresh; printf 'other upstream/other other-\n' >> "$L/packaging/upstream-watch.txt"
expect "a watched name with no pin" error watch-pins
fresh; printf 'xidi https://github.com/samuelgr/Xidi/releases/download/v4.3.1/Xidi-v4.3.1.zip %064d\n' 0 >> "$L/packaging/third-party.txt"
expect "a pinned GitHub release nobody watches (CMR4's Xidi)" warn unwatched-pins
fresh; rm "$L/.github/workflows/watch-upstream.yml"
expect "pins without a watch workflow" warn unwatched-pins

# --- innoextract: pinned and credited --------------------------------------------------------------
inno_code() { # a launcher that runs the bundled innoextract
  printf '\nconst INNOEXTRACT: &str = "innoextract.exe";\n' >> "$L/crates/fx-launcher/src/main.rs"
}
inno_pins() {
  sh -c '. "$1/packaging/pins.sh"; . "$1/packaging/innoextract.sh"; innoextract_pins' sh "$ROOT" >> "$L/packaging/third-party.txt"
  printf 'innoextract dscharrer/innoextract innoextract-\n' >> "$L/packaging/upstream-watch.txt"
}
fresh; inno_code
expect "runs innoextract.exe with no pins and no credit" error innoextract
printf '%s\n' "$out" | grep -q 'no pin file pins innoextract-exe' && printf '%s\n' "$out" | grep -q "THIRD_PARTY.md doesn't list it" \
  && ok "each missing piece is named" || bad "each missing piece is named: $out"
fresh; inno_code; inno_pins
expect "pinned but not in THIRD_PARTY.md" error innoextract
fresh; inno_code; inno_pins; printf '| innoextract | 1.9 | zlib |\n' >> "$L/THIRD_PARTY.md"
run "$L"; [ "$rc" -eq 0 ] && ! printf '%s\n' "$out" | grep -q 'innoextract\]' \
  && ok "pinned and credited: clean" || bad "pinned and credited: $out"
fresh; printf '// innoextract.exe is not used yet\n' >> "$L/crates/fx-launcher/src/main.rs"
run "$L"; printf '%s\n' "$out" | grep -q 'innoextract\]' && bad "a comment is code here too? $out" || ok "no innoextract in the code: no rule"

# --- secrets: process listings ---------------------------------------------------------------------
for line in 'ps -eo pid,stat,args | grep wineserver' 'ps aux | grep -i rally' 'ps -ef' 'ps -o pid=,cmd= -p "$pid"' \
  'pgrep -af wineserver' 'env | sort' 'x=$(printenv)' 'cat "/proc/$pid/cmdline"' 'tr "\0" "\n" < /proc/$p/environ'; do
  fresh; printf '%s\n' "$line" >> "$L/linux/tests/e2e.sh"
  expect "prints other processes' command lines or environments: $line" error process-listing
done
fresh; printf '%s\n' '# ps aux is never used here' 'pgrep -x wineserver' 'ps -o pid,stat,comm' 'env -i PATH=/usr/bin:/bin sh -c true' \
  'v=$(tr "\0" "\n" < "/proc/$p/environ" | sed -n "s/^WINEPREFIX=//p")' 'printf "X=1\0" > "$T/proc/4242/environ"' >> "$L/linux/tests/e2e.sh"
mkdir -p "$L/.work/old" "$L/vendor/dosbox/scripts"
printf 'pgrep -a wine\n' > "$L/.work/old/harness.sh"; printf 'env\n' > "$L/vendor/dosbox/scripts/log-env.sh"
run "$L"
printf '%s\n' "$out" | grep -q 'process-listing' && bad "safe listings, task scratch and vendored code pass: $out" \
  || ok "pgrep -x, ps comm, env -i, a value read from environ, a fake environ, .work/ and vendor/ pass"

# --- real-game tests ------------------------------------------------------------------------------------
fresh; drop_line "$L/linux/tests/e2e.sh" 'FX_E2E'
expect "a real-game test with no opt-in (CMR3's e2e.sh, 2026-11-02)" error real-game-gated
fresh; replace "$L/linux/tests/e2e.sh" 'gamescope --backend headless' 'gamescope -W 3840 -H 2160'
expect "a real-game test with a visible gamescope" error real-game-gated
fresh; printf '#!/bin/sh\n# a stand-in gamescope\ngamescope() { :; }\ngamescope -W 640 -- true\n' > "$L/linux/tests/unit.sh"
run "$L"
printf '%s\n' "$out" | grep -q 'real-game-gated' && bad "a test with fakes only is not a real-game test: $out" \
  || ok "a test with a stand-in gamescope and no real game passes"

# --- DRM and waivers -------------------------------------------------------------------------------
fresh; printf -- '- **Editions:** the pre-activated v1.1 executable.\n' >> "$L/AGENTS.md"
expect "a pre-activated build without a recorded decision (DiRT Rally)" error drm-exception
printf 'drm-exception: owner decision 2026-11-02, delisted game (AGENTS.md)\n' > "$L/.rea-check-waivers"
run "$L"
if [ "$rc" -eq 0 ] && printf '%s\n' "$out" | grep -q '^waived\[drm-exception\]' \
  && printf '%s\n' "$out" | grep -q 'reason: owner decision 2026-11-02'; then
  ok "a waiver with a reason passes and is shown"
else
  bad "waiver: $out"
fi
fresh; printf -- '- We never ship a crack or a no-CD exe, and refuse unknown ones.\n' >> "$L/AGENTS.md"
run "$L"; [ "$rc" -eq 0 ] && ok "saying we never crack is not an exception" || bad "negation flagged: $out"
fresh; printf 'drm-exception:\n' > "$L/.rea-check-waivers"
expect "a waiver without a reason" error waivers
fresh; printf '# the owner said so\nno-such-rule: because\n' > "$L/.rea-check-waivers"
expect "a waiver for a rule that doesn't exist" error waivers
fresh; printf 'lld: owner decision\n' > "$L/.rea-check-waivers"
run "$L"; printf '%s\n' "$out" | grep -q "waives lld, which found nothing" && ok "a stale waiver is pointed out" || bad "stale waiver: $out"
fresh; printf 'waivers: no\n' > "$L/.rea-check-waivers"
expect "the waivers rule can't be waived" error waivers

# --- output and options -------------------------------------------------------------------------------
fresh; rm "$L/crates/fx-launcher/tests/snapshots.rs"
run --github "$L"
printf '%s\n' "$out" | grep -q '^::error file=tests,title=launcher-check snapshots::' \
  && ok "--github annotations" || bad "--github: $out"
printf '%s\n' "$out" | grep -q '^  fix: ' && ok "every problem says how to fix it" || bad "no fix line: $out"
fresh; drop_line "$L/.github/workflows/build.yml" 'wine-tests: true'
run --github "$L"
printf '%s\n' "$out" | grep -q '^::error file=.github/workflows/build.yml,title=launcher-check wine-tests::' \
  && ok "--github: the file of the problem" || bad "--github file: $out"
run --explain audience
printf '%s\n' "$out" | grep -q '^  why: ' && printf '%s\n' "$out" | grep -q '^  fix: ' \
  && ok "--explain says why and how" || bad "--explain: $out"
run --explain all
[ "$(printf '%s\n' "$out" | grep -c '^  why: ')" -ge 25 ] && ok "--explain all" || bad "--explain all: $out"
run --explain nope; [ "$rc" -eq 2 ] && ok "--explain an unknown rule exits 2" || bad "--explain nope: $rc"
run --list; printf '%s\n' "$out" | grep -q '^drm-exception  *error' && ok "--list" || bad "--list: $out"
run --frob; [ "$rc" -eq 2 ] && ok "an unknown option exits 2" || bad "--frob: $rc"
run "$T/missing"; [ "$rc" -eq 2 ] && ok "a missing folder exits 2" || bad "missing folder: $rc"
for r in $(sh "$CHECK" --list | cut -d' ' -f1); do
  grep -F -- "$r" "$HERE/launcher-check.sh" | grep -qvE '^[ \t]*#' || bad "rule $r has no test here"
done

if [ "$fails" = 0 ]; then echo "all passed"; exit 0; fi
echo "$fails failed"; exit 1
