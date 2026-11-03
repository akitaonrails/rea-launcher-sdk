#!/bin/sh
# Tests scripts/prepush-scan in temp git repositories: with a fake gitleaks
# (how findings are reported: paths and commit ids, never the matched text)
# and, when gitleaks is installed, the real one (a planted token is found,
# the rea-play.sh *_KEY settings are not). No download: REA_NO_FETCH=1.
#   sh scripts/tests/prepush-scan.sh
set -u
HERE=$(cd "$(dirname "$0")" && pwd -P)
SCAN=$HERE/../prepush-scan
T=$(mktemp -d "${TMPDIR:-/tmp}/rea-prepush-test.XXXXXX")
trap 'rm -rf "$T"' EXIT
fails=0
ok() { echo "ok   $1"; }
bad() { echo "FAIL $1"; fails=$((fails + 1)); }
git() { command git -c user.name=t -c user.email=t@example.com -c init.defaultBranch=main "$@"; }
export GIT_CONFIG_NOSYSTEM=1 REA_NO_FETCH=1
# A token-shaped value built at run time, so this file holds none.
SECRET=$(printf 'ghp_%s%s' 'Zq8xW3rT7yU1iO5pA9sD2fG6hJ4kL0' 'mN3bV7')

repo() { # name: a repository with one clean commit and a remote branch
  rm -rf "${T:?}/${1:?}" "$T/$1.git"
  mkdir -p "$T/$1"
  (
    cd "$T/$1" && git init -q &&
      printf 'REA_SCALER_KEY= REA_DEFAULT_SCALER=nis\n' > play &&
      git add -A && git commit -qm clean &&
      git init -q --bare "$T/$1.git" && git remote add origin "$T/$1.git" &&
      git push -q origin main 2>/dev/null && git branch -q -u origin/main
  )
}
run() { # repo args...
  r=$1; shift
  (cd "$T/$r" && sh "$SCAN" "$@") > "$T/out" 2> "$T/err"
}

# --- a fake gitleaks: prints the secret itself, reports one finding ----------
mkdir -p "$T/bin"
cat > "$T/bin/gitleaks" <<'FAKE'
#!/bin/sh
echo "$*" >> "$FAKE_LOG"
report='' template=''
while [ $# -gt 0 ]; do
  case $1 in
    --report-path) report=$2; shift ;;
    --report-template) template=$2; shift ;;
  esac
  shift
done
echo "Finding: token=$FAKE_SECRET Secret: $FAKE_SECRET"
grep -q 'RuleID' "$template" || exit 3
case $FAKE_MODE in
  find) printf 'github-pat|7|0123456789abcdef0123|src/conf.rs\n' > "$report" ;;
  tree) printf 'github-pat|3||%s/tree/a|b.txt\n' "$FAKE_TREE" > "$report" ;;
  fail) echo "fatal: bad revision" >&2; exit 2 ;;
esac
exit 0
FAKE
chmod +x "$T/bin/gitleaks"
export FAKE_LOG="$T/fake.log" FAKE_SECRET="$SECRET"
repo fake
(cd "$T/fake" && echo y > g && git add g && git commit -qm unpushed)
FAKE_MODE=find REA_GITLEAKS=$T/bin/gitleaks run fake
rc=$?
[ "$rc" = 1 ] && ok "a finding exits 1" || bad "a finding exits $rc: $(cat "$T/err")"
grep -q 'possible secret: src/conf.rs:7 (commit 0123456789ab, rule github-pat)' "$T/out" \
  && ok "the path, line, short commit id and rule" || bad "report: $(cat "$T/out")"
if grep -q "$SECRET" "$T/out" "$T/err"; then bad "the secret was printed"; else ok "the secret is never printed (not even gitleaks' own output)"; fi
grep -q -- '--redact' "$FAKE_LOG" && ok "gitleaks runs with --redact" || bad "no --redact: $(cat "$FAKE_LOG")"
grep -q -- '--log-opts=@{upstream}..HEAD' "$FAKE_LOG" && ok "the commits a push publishes (@{upstream}..HEAD)" || bad "range: $(cat "$FAKE_LOG")"
grep -q ' dir ' "$FAKE_LOG" && ok "and the tree at HEAD" || bad "tree scan: $(cat "$FAKE_LOG")"
grep -q -- "-c .*gitleaks.toml" "$FAKE_LOG" && ok "the SDK allowlist when the repo has none" || bad "config: $(cat "$FAKE_LOG")"

: > "$FAKE_LOG"
(cd "$T/fake" && echo x > f && git add f && git commit -qm one)
FAKE_MODE=none REA_GITLEAKS=$T/bin/gitleaks run fake --no-tree
rc=$?
[ "$rc" = 0 ] && ok "nothing found exits 0" || bad "clean exits $rc: $(cat "$T/out" "$T/err")"
grep -q '2 commit(s) in @{upstream}..HEAD' "$T/out" && ok "counts the unpushed commits" || bad "count: $(cat "$T/out")"
grep -q ' dir ' "$FAKE_LOG" && bad "--no-tree scanned the tree" || ok "--no-tree skips the tree"

: > "$FAKE_LOG"
FAKE_MODE=none REA_GITLEAKS=$T/bin/gitleaks run fake --all --no-tree
grep -q -- '--log-opts' "$FAKE_LOG" && bad "--all limited the range" || ok "--all scans the whole history"
FAKE_MODE=none REA_GITLEAKS=$T/bin/gitleaks run fake --range 'HEAD~1..HEAD' --no-tree
grep -q -- '--log-opts=HEAD~1..HEAD' "$FAKE_LOG" && ok "--range" || bad "--range: $(cat "$FAKE_LOG")"
REA_GITLEAKS=$T/bin/gitleaks run fake --range 'a;rm -rf x'
[ $? = 2 ] && ok "an unsafe --range is refused" || bad "unsafe range accepted"

FAKE_TREE=$(cd "${TMPDIR:-/tmp}" && pwd -P)
export FAKE_TREE
FAKE_MODE=fail REA_GITLEAKS=$T/bin/gitleaks run fake
rc=$?
[ "$rc" = 2 ] && ok "gitleaks failing exits 2" || bad "gitleaks failure exits $rc"
if grep -q "$SECRET" "$T/out" "$T/err"; then bad "a failure printed the secret"; else ok "a failure prints only error lines"; fi

printf '[extend]\nuseDefault = true\n' > "$T/fake/.gitleaks.toml"
: > "$FAKE_LOG"
FAKE_MODE=none REA_GITLEAKS=$T/bin/gitleaks run fake --no-tree
grep -q -- "-c $T/fake/.gitleaks.toml" "$FAKE_LOG" && ok "a repository's .gitleaks.toml wins" || bad "repo config: $(cat "$FAKE_LOG")"

REA_GITLEAKS=$T/bin/missing run fake
[ $? = 2 ] && grep -q 'is not executable' "$T/err" && ok "a missing REA_GITLEAKS: exit 2" || bad "missing gitleaks: $(cat "$T/err")"
mkdir -p "$T/plain"
sh "$SCAN" "$T/plain" > /dev/null 2>&1
[ $? = 2 ] && ok "not a git repository: exit 2" || bad "not a repo"

# --- the real gitleaks -----------------------------------------------------------
REAL=${REA_GITLEAKS:-$(command -v gitleaks 2>/dev/null || true)}
case $REAL in "$T"/*) REAL='' ;; esac
if [ -z "$REAL" ]; then
  echo "skip real gitleaks: not installed"
else
  repo real
  run real
  [ $? = 0 ] && ok "real: the *_KEY settings of a play script are no secret" || bad "real clean: $(cat "$T/out" "$T/err")"
  (cd "$T/real" && printf 'const TOKEN: &str = "%s";\n' "$SECRET" > token.rs && git add token.rs && git commit -qm token)
  id=$(git -C "$T/real" rev-parse HEAD | cut -c1-12)
  REA_GITLEAKS=$REAL run real
  [ $? = 1 ] && ok "real: a planted token is found" || bad "real: not found: $(cat "$T/out" "$T/err")"
  grep -q "possible secret: token.rs:1 (commit $id" "$T/out" && ok "real: in the unpushed commit" || bad "real commit: $(cat "$T/out")"
  grep -q "possible secret: token.rs:1 (tree at HEAD" "$T/out" && ok "real: and in the tree, with the repo's path" || bad "real tree: $(cat "$T/out")"
  if grep -q "$SECRET" "$T/out" "$T/err"; then bad "real: the secret was printed"; else ok "real: the secret is never printed"; fi
  (cd "$T/real" && git push -q origin main 2>/dev/null)
  REA_GITLEAKS=$REAL run real --no-tree
  [ $? = 0 ] && ok "real: once pushed, only new commits count (--no-tree)" || bad "real pushed: $(cat "$T/out")"
fi

[ "$fails" = 0 ] && echo "all passed" || { echo "$fails failed"; exit 1; }
