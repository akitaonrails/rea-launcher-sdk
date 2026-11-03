#!/bin/sh
# Runs launcher-ci.yml's shell steps as GitHub does (sh -e) against fixture
# trees, so a check that cannot fail is caught: sh scripts/tests/workflows.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd -P)
WF=$HERE/../../.github/workflows
T=$(mktemp -d "${TMPDIR:-/tmp}/rea-workflows-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }

# Prints the `run: |` block of the step named $2 in workflow $1, unindented.
extract() {
  awk -v want="$2" '
    function indent(s) { match(s, /^ */); return RLENGTH }
    !found { t = $0; sub(/^ *- name: /, "", t); if ($0 ~ /^ *- name: / && t == want) found = 1; next }
    found && !inrun { if ($0 ~ /^ *- (name|uses):/) exit; if ($0 ~ /^ *run: \|$/) { inrun = 1; ri = indent($0) }; next }
    inrun { if ($0 !~ /^ *$/ && indent($0) <= ri) exit; print substr($0, ri + 3) }
  ' "$1"
}
step() { # workflow step-name -> $T/<slug>.sh, non-empty
  f=$T/$(printf '%s' "$2" | tr -c 'A-Za-z0-9' '_').sh
  extract "$WF/$1" "$2" > "$f"
  [ -s "$f" ] || { echo "no run block for '$2' in $1" >&2; return 1; }
  echo "$f"
}
passes() { if (cd "$T/w" && sh -e "$S" >"$T/out" 2>&1); then ok "$1"; else bad "$1: $(cat "$T/out")"; fi; }
fails_() { if (cd "$T/w" && sh -e "$S" >"$T/out" 2>&1); then bad "$1 (step passed)"; else ok "$1"; fi; }

mkdir -p "$T/w/linux/tests" "$T/w/packaging"
printf '#!/bin/sh\necho fine\n' > "$T/w/linux/game-play"
printf '#!/bin/sh\nexit 0\n' > "$T/w/packaging/build.sh"

S=$(step launcher-ci.yml "Shell syntax") || exit 1
export GLOBS='linux/*-play packaging/*.sh missing/*.sh'
passes "shell syntax: valid scripts pass, unmatched globs skipped"
printf 'if then fi (\n' > "$T/w/packaging/broken.sh"
fails_ "shell syntax: a syntax error fails the step"
rm "$T/w/packaging/broken.sh"

S=$(step launcher-ci.yml "Shell tests") || exit 1
printf '#!/bin/sh\necho ok\n' > "$T/w/linux/tests/unit.sh"
printf '#!/bin/sh\necho "needs a dev kit" >&2; exit 2\n' > "$T/w/linux/tests/e2e.sh"
export GLOBS='linux/tests/*.sh packaging/tests/*.sh' SKIP_PATTERNS='*e2e*'
passes "shell tests: *e2e* skipped by default"
grep -q '^skip linux/tests/e2e.sh$' "$T/out" && ok "shell tests: the skip is reported" || bad "shell tests: the skip is reported"
export SKIP_PATTERNS=''
fails_ "shell tests: a failing test fails the step"
export SKIP_PATTERNS='*e2e*'
printf '#!/bin/sh\nexit 1\n' > "$T/w/linux/tests/unit.sh"
fails_ "shell tests: a failing unit test fails the step"
# The skip patterns must not glob against the working folder.
printf '#!/bin/sh\necho ok\n' > "$T/w/linux/tests/unit.sh"; : > "$T/w/xe2ex"
passes "shell tests: skip patterns are not expanded as file globs"

S=$(step launcher-ci.yml "Dependency advisories, licences and sources") || exit 1
rm -f "$T/w/deny.toml"
if (cd "$T/w" && sh -e "$S" >"$T/out" 2>&1); then bad "audit without deny.toml fails"
elif grep -q 'templates/deny.toml' "$T/out"; then ok "audit without deny.toml fails with the template hint"
else bad "audit without deny.toml: $(cat "$T/out")"; fi

# mirror.yml: only pins marked mirror, verified, named by the pin; the
# release is created when missing, never deleted, assets clobbered.
M=$T/m
mkdir -p "$M/up" "$M/repo/sdk" "$M/bin" "$M/rt"
cp -R "$HERE/../../packaging" "$M/repo/sdk/packaging"
echo xidi > "$M/up/Xidi-v4.3.1.zip"; echo dxvk > "$M/up/dxvk.tar.gz"; echo hd > "$M/up/hd.zip"
sha() { sha256sum "$1" | cut -d' ' -f1; }
cat > "$M/repo/pins.txt" <<PINS
xidi file://$M/up/Xidi-v4.3.1.zip $(sha "$M/up/Xidi-v4.3.1.zip") mirror https://github.com/me/game/releases/download/thirdparty/Xidi-v4.3.1.zip
dxvk file://$M/up/dxvk.tar.gz $(sha "$M/up/dxvk.tar.gz") mirror
hd file://$M/up/hd.zip $(sha "$M/up/hd.zip")
PINS
cat > "$M/bin/gh" <<'GH'
#!/bin/sh
echo "gh $*" >> "$GH_LOG"
[ "$1 $2" = "release view" ] && [ -z "${GH_HAS_RELEASE:-}" ] && exit 1
exit 0
GH
chmod +x "$M/bin/gh"
S=$(step mirror.yml "Fetch and verify every mirrorable pin") || exit 1
S2=$(step mirror.yml "Upload to the mirror release") || exit 1
mirror_env() { export PINS=pins.txt TAG=thirdparty SDK_DIR=sdk RUNNER_TEMP="$M/rt" GITHUB_REPOSITORY=me/game GH_LOG="$M/gh.log" PATH="$M/bin:$PATH"; }
if (cd "$M/repo" && mirror_env && sh -e "$S") >"$T/out" 2>&1; then ok "mirror: fetch step passes"; else bad "mirror: fetch step: $(cat "$T/out")"; fi
[ "$(ls "$M/rt/mirror" | LC_ALL=C sort | tr '\n' ' ')" = "Xidi-v4.3.1.zip dxvk.tar.gz " ] && ok "mirror: only marked pins, under the pin's file name" || bad "mirror: files: $(ls "$M/rt/mirror")"
grep -q 'warning.*dxvk: add the mirror url as a fallback: https://github.com/me/game/releases/download/thirdparty/dxvk.tar.gz' "$T/out" \
  && ok "mirror: warns when a pin lacks its mirror url" || bad "mirror: missing-fallback warning"
if grep -q 'xidi: add the mirror url' "$T/out"; then bad "mirror: no warning when the fallback is listed"; else ok "mirror: no warning when the fallback is listed"; fi
if (cd "$M/repo" && mirror_env && sh -e "$S2") >"$T/out" 2>&1; then ok "mirror: upload step passes"; else bad "mirror: upload step: $(cat "$T/out")"; fi
grep -q '^gh release create thirdparty --repo me/game --latest=false' "$M/gh.log" && ok "mirror: creates the missing release, not as latest" || bad "mirror: create: $(cat "$M/gh.log")"
up=$(grep '^gh release upload thirdparty ' "$M/gh.log")
case $up in *" --repo me/game --clobber") ;; *) up='' ;; esac
case $up in *"$M/rt/mirror/Xidi-v4.3.1.zip"*) ;; *) up='' ;; esac
case $up in *"$M/rt/mirror/dxvk.tar.gz"*) ;; *) up='' ;; esac
[ -n "$up" ] && ok "mirror: uploads with --clobber" || bad "mirror: upload: $(cat "$M/gh.log")"
: > "$M/gh.log"
(cd "$M/repo" && mirror_env && GH_HAS_RELEASE=1 && export GH_HAS_RELEASE && sh -e "$S2") >/dev/null 2>&1
if grep -q 'release create' "$M/gh.log"; then bad "mirror: existing release reused"; else ok "mirror: existing release reused"; fi
if grep -q 'delete' "$M/gh.log"; then bad "mirror: never deletes"; else ok "mirror: never deletes"; fi
# A file that no longer matches its pin is never uploaded.
echo tampered > "$M/up/dxvk.tar.gz"; rm -rf "$M/rt/mirror"
if (cd "$M/repo" && mirror_env && sh -e "$S") >"$T/out" 2>&1; then bad "mirror: a sha256 mismatch fails"; else ok "mirror: a sha256 mismatch fails"; fi
echo dxvk > "$M/up/dxvk.tar.gz"
# Two pins sharing an asset name are refused.
echo "dup file://$M/up/dup/Xidi-v4.3.1.zip $(sha "$M/up/Xidi-v4.3.1.zip") mirror" >> "$M/repo/pins.txt"
if (cd "$M/repo" && mirror_env && sh -e "$S") >"$T/out" 2>&1; then bad "mirror: duplicate asset name refused"
elif grep -q 'is used by another pin' "$T/out"; then ok "mirror: duplicate asset name refused"; else bad "mirror: duplicate: $(cat "$T/out")"; fi
sed -i '/^dup /d' "$M/repo/pins.txt"
# hosted: skipped unless include-hosted, then uploaded like a mirror.
echo dgv > "$M/up/dgVoodoo2_80.zip"
echo "dgvoodoo file://$M/up/dgVoodoo2_80.zip $(sha "$M/up/dgVoodoo2_80.zip") hosted" >> "$M/repo/pins.txt"
rm -rf "$M/rt/mirror"
(cd "$M/repo" && mirror_env && sh -e "$S") >"$T/out" 2>&1
[ ! -e "$M/rt/mirror/dgVoodoo2_80.zip" ] && grep -q 'dgvoodoo is marked hosted (no licence): skipped' "$T/out" \
  && ok "mirror: hosted pin skipped without include-hosted" || bad "mirror: hosted skip: $(cat "$T/out")"
rm -rf "$M/rt/mirror"
if (cd "$M/repo" && mirror_env && INCLUDE_HOSTED=true && export INCLUDE_HOSTED && sh -e "$S") >"$T/out" 2>&1 \
  && [ -f "$M/rt/mirror/dgVoodoo2_80.zip" ]; then ok "mirror: hosted pin uploaded with include-hosted"; else bad "mirror: include-hosted: $(cat "$T/out")"; fi
grep -q "hosted by the owner's decision" "$T/out" && ok "mirror: hosted upload is announced" || bad "mirror: hosted notice"
# One-pin-per-file folders (OutRun's packaging/<name>.txt), alone, as a
# glob, and mixed with a pin file; unmarked and malformed files are skipped.
mkdir -p "$M/repo/pd"
echo vc > "$M/up/vc_redist.x86.exe"
printf '# VC++ runtime\nfile://%s %s mirror\n' "$M/up/vc_redist.x86.exe" "$(sha "$M/up/vc_redist.x86.exe")" > "$M/repo/pd/vcredist.txt"
printf 'file://%s %s\n' "$M/up/hd.zip" "$(sha "$M/up/hd.zip")" > "$M/repo/pd/wine.txt"
printf 'https://example.com/hd.zip %s 325966152 3168683238\n' "$(sha "$M/up/hd.zip")" > "$M/repo/pd/hd-pack.txt"
for pins in pd 'pd/*.txt' 'pd/vcredist.txt'; do
  rm -rf "$M/rt/mirror"
  if (cd "$M/repo" && mirror_env && PINS=$pins && export PINS && sh -e "$S") >"$T/out" 2>&1 \
    && [ "$(ls "$M/rt/mirror")" = vc_redist.x86.exe ]; then ok "mirror: pins '$pins' (one pin per file)"; else bad "mirror: pins '$pins': $(ls "$M/rt/mirror" 2>&1) $(cat "$T/out")"; fi
done
rm -rf "$M/rt/mirror"
(cd "$M/repo" && mirror_env && PINS='pins.txt pd' && export PINS && sh -e "$S") >"$T/out" 2>&1
[ "$(ls "$M/rt/mirror" | LC_ALL=C sort | tr '\n' ' ')" = "Xidi-v4.3.1.zip dxvk.tar.gz vc_redist.x86.exe " ] \
  && ok "mirror: a pin file and a folder together" || bad "mirror: mixed: $(ls "$M/rt/mirror")"
if (cd "$M/repo" && mirror_env && PINS=nowhere && export PINS && sh -e "$S") >"$T/out" 2>&1; then bad "mirror: a missing source fails"; else grep -q 'no pin file or folder nowhere' "$T/out" && ok "mirror: a missing source fails" || bad "mirror: missing: $(cat "$T/out")"; fi

# compat.yml: the launcher's own cargo config is kept, the SDK crates that
# exist are patched in, sdk/ is re-vendored, a second patch is refused.
C=$T/compat
mkdir -p "$C/sdk-src/crates/rea-launcher" "$C/sdk-src/crates/rea-launcher-ui" "$C/sdk-src/scripts" "$C/launcher/.cargo"
cp -R "$HERE/../../packaging" "$C/sdk-src/packaging"
cp "$HERE/../sdk-sync" "$C/sdk-src/scripts/"
: > "$C/sdk-src/crates/rea-launcher/Cargo.toml"; : > "$C/sdk-src/crates/rea-launcher-ui/Cargo.toml"
printf '[target.i686-pc-windows-gnu]\nlinker = "i686-w64-mingw32-gcc"\n' > "$C/launcher/.cargo/config.toml"
S=$(step compat.yml "Point the launcher at this SDK") || exit 1
export SDK="$C/sdk-src" LAUNCHER="$C/launcher"
if sh -e "$S" >"$T/out" 2>&1; then ok "compat: patch step passes"; else bad "compat: patch step: $(cat "$T/out")"; fi
cfg=$C/launcher/.cargo/config.toml
grep -qx 'linker = "i686-w64-mingw32-gcc"' "$cfg" && ok "compat: launcher's cargo config kept" || bad "compat: config kept"
grep -qx '\[patch\."https://github.com/akitaonrails/rea-launcher-sdk"\]' "$cfg" && ok "compat: patch table added" || bad "compat: patch table: $(cat "$cfg")"
grep -qx "rea-launcher = { path = \"$C/sdk-src/crates/rea-launcher\" }" "$cfg" && ok "compat: crate patched to the SDK checkout" || bad "compat: crate path: $(cat "$cfg")"
if grep -q 'rea-launcher-win32' "$cfg"; then bad "compat: absent crates not patched"; else ok "compat: absent crates not patched"; fi
grep -qx 'tag=canary' "$C/launcher/sdk/VERSION" && ok "compat: sdk/ re-vendored from the checkout" || bad "compat: sdk/VERSION"
if sh -e "$S" >"$T/out" 2>&1; then bad "compat: second patch refused"; else grep -q 'already patches' "$T/out" && ok "compat: second patch refused" || bad "compat: second: $(cat "$T/out")"; fi
unset SDK LAUNCHER

# launcher-ci: the imports check covers every .exe/.dll in the packages
# (folders and zips, any letter case), not only the workspace build.
P=$T/pkgimports
mkdir -p "$P/sdk/packaging" "$P/dist/Game-v1/components" "$P/zipsrc/Other-v1" "$P/rt"
cat > "$P/sdk/packaging/check-imports.sh" <<'STUB'
#!/bin/sh
for f; do echo "$f" >> "$CHECKED"; case $f in *bad*) exit 1 ;; esac; done
STUB
for f in Game.exe components/D3D9.DLL components/xidi.dll README.md; do echo x > "$P/dist/Game-v1/$f"; done
echo x > "$P/zipsrc/Other-v1/tool.EXE"
S=$(step launcher-ci.yml "Every import resolves (packages)") || exit 1
pkg_env() { export SDK_DIR=sdk RUST_TARGET=i686-pc-windows-gnu RUNNER_TEMP="$P/rt" CHECKED="$P/checked" PACKAGES="$1"; }
: > "$P/checked"
if (cd "$P" && pkg_env 'dist/Game-*/' && sh -e "$S") >"$T/out" 2>&1; then ok "imports (packages): passes"; else bad "imports (packages): $(cat "$T/out")"; fi
check_n() { [ "$(grep -c . "$P/checked")" = "$2" ] && ok "$1" || bad "$1: $(cat "$P/checked")"; }
check_n "imports (packages): every .exe/.dll, any case" 3
grep -q README "$P/checked" && bad "imports (packages): other files skipped" || ok "imports (packages): other files skipped"
if command -v zip >/dev/null 2>&1 && command -v unzip >/dev/null 2>&1; then
  (cd "$P/zipsrc" && zip -qr "$P/dist/Other-v1.zip" Other-v1)
  : > "$P/checked"
  (cd "$P" && pkg_env 'dist/Game-*/ dist/*.zip' && sh -e "$S") >"$T/out" 2>&1
  grep -q 'Other-v1/tool.EXE$' "$P/checked" && ok "imports (packages): inside zips too" || bad "imports (packages): zip: $(cat "$P/checked")"
fi
echo x > "$P/dist/Game-v1/components/bad.dll"; : > "$P/checked"
if (cd "$P" && pkg_env 'dist/Game-*/' && sh -e "$S") >"$T/out" 2>&1; then bad "imports (packages): a bad DLL fails"; else ok "imports (packages): a bad DLL fails"; fi
check_n "imports (packages): the others are still checked" 4
: > "$P/checked"
(cd "$P" && pkg_env 'dist/Game-*/' && RUST_TARGET=x86_64-unknown-linux-gnu && sh -e "$S") >/dev/null 2>&1
check_n "imports (packages): not for a native target" 0

# Secret scan: the launcher-ci step against a fake gitleaks, and its pin in
# step with scripts/prepush-scan.
pin() { sed -n "s/^$1=//p" "$HERE/../prepush-scan"; }
grep -q "GITLEAKS_VERSION: $(pin GITLEAKS_VERSION)\$" "$WF/launcher-ci.yml" && ok "secret scan: same gitleaks version as prepush-scan" || bad "secret scan: version differs from prepush-scan"
grep -q "GITLEAKS_SHA256: $(pin GITLEAKS_SHA256_x86_64)\$" "$WF/launcher-ci.yml" && ok "secret scan: same SHA-256 as prepush-scan" || bad "secret scan: SHA-256 differs from prepush-scan"
grep -q 'bREA_\[A-Z0-9_\]\*KEY=' "$WF/launcher-ci.yml" && grep -q 'bREA_\[A-Z0-9_\]\*KEY=' "$HERE/../prepush-scan" && ok "secret scan: same allowlist" || bad "secret scan: allowlists differ"
grep -q 'sh scripts/prepush-scan --all' "$WF/ci.yml" && ok "secret scan: the SDK's ci.yml runs prepush-scan --all" || bad "secret scan: ci.yml"
G=$T/gl
mkdir -p "$G/bin" "$G/w" "$G/rt"
cat > "$G/bin/gitleaks" <<'FAKE'
#!/bin/sh
echo "$*" >> "$GL_LOG"
while [ $# -gt 0 ]; do [ "$1" = --report-path ] && r=$2; shift; done
echo "Secret: $GL_SECRET"
if [ -n "${GL_FIND:-}" ]; then printf 'github-pat|4|abcdef0123456789|conf.rs\ngithub-pat|4||%s/scan/tree/conf.rs\n' "$RUNNER_TEMP" > "$r"; else : > "$r"; fi
FAKE
chmod +x "$G/bin/gitleaks"
(cd "$G/w" && git init -q && echo x > f && git -c user.name=t -c user.email=t@e commit -qm x --allow-empty && git add f && git -c user.name=t -c user.email=t@e commit -qm f) >/dev/null 2>&1
S=$(step launcher-ci.yml "Scan the history and the tree (paths and commit ids only)") || exit 1
scan() { (cd "$G/w" && rm -rf "$G/rt/scan" && PATH="$G/bin:$PATH" RUNNER_TEMP="$G/rt" GL_LOG="$G/log" GL_SECRET=hunter2-not-real sh -e "$S") > "$T/out" 2>&1; }
if scan; then ok "secret scan: nothing found passes"; else bad "secret scan: clean: $(cat "$T/out")"; fi
grep -q -- '--redact' "$G/log" && grep -q '^git ' "$G/log" && grep -q '^dir ' "$G/log" && ok "secret scan: history and tree, redacted" || bad "secret scan: modes: $(cat "$G/log")"
grep -q -- "-c $G/rt/scan/gitleaks.toml" "$G/log" && ok "secret scan: the SDK allowlist without a .gitleaks.toml" || bad "secret scan: config: $(cat "$G/log")"
if GL_FIND=1 scan; then bad "secret scan: a finding fails"; else ok "secret scan: a finding fails"; fi
grep -q '::error file=conf.rs,line=4::possible secret (commit abcdef012345, rule github-pat)' "$T/out" && ok "secret scan: annotation with path and commit" || bad "secret scan: annotation: $(cat "$T/out")"
grep -q '::error file=conf.rs,line=4::possible secret (tree at HEAD' "$T/out" && ok "secret scan: tree paths are the repo's" || bad "secret scan: tree path: $(cat "$T/out")"
grep -q hunter2 "$T/out" && bad "secret scan: gitleaks' output was shown" || ok "secret scan: gitleaks' own output is never shown"
echo '[extend]' > "$G/w/.gitleaks.toml"; : > "$G/log"; scan
grep -q -- "-c $G/w/.gitleaks.toml" "$G/log" && ok "secret scan: a repository's .gitleaks.toml wins" || bad "secret scan: repo config: $(cat "$G/log")"

# No status-swallowing "check && echo" anywhere in the workflows.
if grep -n '&& echo' "$WF"/*.yml; then bad "no '&& echo' checks in workflows"; else ok "no '&& echo' checks in workflows"; fi

[ "$fails" = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
