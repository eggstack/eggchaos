#!/usr/bin/env sh
# M047 WP6 regression: shared provenance collector Git-state handling.
#
# Builds disposable Git repositories (never touches the real checkout) and
# covers: clean tree, tracked-unstaged edit, staged edit, untracked source
# file, ignored/generated-only files, excluded output artifact, detached
# HEAD, subdirectory invocation, missing-Git failure in authoritative mode,
# fingerprint determinism + content sensitivity, no Git mutation, no absolute
# paths, and single-authority wiring (both benchmark wrappers delegate to
# scripts/bench_provenance.py and stamp no HEAD of their own).
set -eu

fail() { echo "FAIL: $1" >&2; exit 1; }

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
COLLECTOR="$REPO_ROOT/scripts/bench_provenance.py"

field() {
  # field <json-file> <key> -> raw value (empty for JSON null)
  python3 -c "import json,sys; v=json.load(open(sys.argv[1]))['provenance'][sys.argv[2]]; print('' if v is None else (str(v).lower() if isinstance(v,bool) else str(v)))" "$1" "$2"
}

collect() {
  # collect <dir> [extra args...] -> prints envelope path (kept outside
  # the repo so the envelope itself never dirties the fixture).
  dir="$1"; shift
  out="$(mktemp)"
  (cd "$dir" && python3 "$COLLECTOR" --json "$@" >"$out")
  echo "$out"
}

new_repo() {
  dir="$(mktemp -d)"
  (cd "$dir" && git init -q && git config user.email "m047@test" && git config user.name "m047" \
    && echo "one" >a.txt && git add a.txt && git commit -qm init)
  echo "$dir"
}

# --- single authority wiring ---------------------------------------------
grep -q "bench_provenance.py" "$REPO_ROOT/scripts/benchmark.sh" \
  || fail "benchmark.sh does not delegate to bench_provenance.py"
grep -q "bench_provenance.py" "$REPO_ROOT/scripts/benchmark_datagram.sh" \
  || fail "benchmark_datagram.sh does not delegate to bench_provenance.py"
if grep -n "rev-parse.*HEAD" "$REPO_ROOT/scripts/benchmark.sh" | grep -q .; then
  fail "benchmark.sh still stamps HEAD directly"
fi
if grep -n "rev-parse.*HEAD" "$REPO_ROOT/scripts/benchmark_datagram.sh" | grep -q .; then
  fail "benchmark_datagram.sh still stamps HEAD directly"
fi
echo "single-authority wiring ok"

# --- 1. clean tree ---------------------------------------------------------
D=$(new_repo)
P=$(collect "$D")
[ "$(field "$P" worktree)" = "clean" ] || fail "clean: worktree=$(field "$P" worktree)"
[ "$(field "$P" authoritative)" = "true" ] || fail "clean must be authoritative"
[ "$(field "$P" source_fingerprint)" = "" ] || fail "clean fingerprint must be null"
HEAD_SHA=$(field "$P" head_sha)
case "$HEAD_SHA" in ????????*) ;; *) fail "clean head_sha malformed: $HEAD_SHA" ;; esac
[ "${#HEAD_SHA}" = "40" ] || fail "clean head_sha not 40-hex"
[ "$(field "$P" schema)" = "1" ] || fail "schema must be 1"
[ "$(field "$P" index_dirty)" = "false" ] || fail "clean index_dirty"
[ "$(field "$P" tracked_dirty)" = "false" ] || fail "clean tracked_dirty"
[ "$(field "$P" untracked_source)" = "false" ] || fail "clean untracked_source"
if grep -q "$D" "$P"; then fail "provenance leaks absolute repo path"; fi
(cd "$D" && python3 "$COLLECTOR" --json --require-clean >/dev/null) \
  || fail "clean --require-clean must exit 0"
echo "clean tree ok"

# --- 2. tracked unstaged edit ----------------------------------------------
echo "two" >>"$D/a.txt"
P=$(collect "$D")
[ "$(field "$P" worktree)" = "dirty" ] || fail "unstaged: not dirty"
[ "$(field "$P" authoritative)" = "false" ] || fail "unstaged must not be authoritative"
[ "$(field "$P" tracked_dirty)" = "true" ] || fail "unstaged tracked_dirty"
[ "$(field "$P" index_dirty)" = "false" ] || fail "unstaged index_dirty must be false"
FP1=$(field "$P" source_fingerprint)
[ "${#FP1}" = "64" ] || fail "dirty fingerprint must be 64-hex"
(cd "$D" && python3 "$COLLECTOR" --json --require-clean >/dev/null 2>&1) \
  && fail "dirty --require-clean must exit nonzero"
echo "tracked-unstaged ok"

# --- fingerprint determinism + content sensitivity --------------------------
P=$(collect "$D"); FP2=$(field "$P" source_fingerprint)
[ "$FP1" = "$FP2" ] || fail "fingerprint not deterministic"
echo "three" >>"$D/a.txt"
P=$(collect "$D"); FP3=$(field "$P" source_fingerprint)
[ "$FP3" != "$FP1" ] || fail "fingerprint insensitive to content change"
echo "fingerprint determinism/sensitivity ok"

# --- 3. staged edit ----------------------------------------------------------
(cd "$D" && git checkout -q -- a.txt && echo "staged" >>a.txt && git add a.txt)
P=$(collect "$D")
[ "$(field "$P" worktree)" = "dirty" ] || fail "staged: not dirty"
[ "$(field "$P" index_dirty)" = "true" ] || fail "staged index_dirty"
[ "$(field "$P" authoritative)" = "false" ] || fail "staged must not be authoritative"
[ -n "$(field "$P" source_fingerprint)" ] || fail "staged needs fingerprint"
echo "staged ok"

# --- 4. untracked source file -------------------------------------------------
D2=$(new_repo)
echo "fn main() {}" >"$D2/new_source.rs"
P=$(collect "$D2")
[ "$(field "$P" worktree)" = "dirty" ] || fail "untracked: not dirty"
[ "$(field "$P" untracked_source)" = "true" ] || fail "untracked_source flag"
[ "$(field "$P" authoritative)" = "false" ] || fail "untracked must not be authoritative"
[ -n "$(field "$P" source_fingerprint)" ] || fail "untracked needs fingerprint"
echo "untracked source ok"

# --- 5. ignored file only ------------------------------------------------------
(cd "$D2" && rm new_source.rs && echo "*.log" >.gitignore && git add .gitignore && git commit -qm ignore \
  && echo "noise" >debug.log)
P=$(collect "$D2")
[ "$(field "$P" worktree)" = "clean" ] || fail "ignored-only file must stay clean"
echo "ignored-only ok"

# --- 5b. generated target/ + performance JSON only ----------------------------
mkdir -p "$D2/target/debug" "$D2/qualification/performance"
echo "binary" >"$D2/target/debug/foo.o"
echo '{"x":1}' >"$D2/qualification/performance/some-report.json"
P=$(collect "$D2")
[ "$(field "$P" worktree)" = "clean" ] || fail "generated-only files must stay clean"
echo "generated-only ok"

# --- 6. output artifact excluded -----------------------------------------------
echo "draft" >"$D2/report-draft.md"
P=$(collect "$D2" --exclude "$D2/report-draft.md")
[ "$(field "$P" worktree)" = "clean" ] || fail "--exclude output must stay clean"
P=$(collect "$D2")
[ "$(field "$P" worktree)" = "dirty" ] || fail "same file without --exclude must be dirty"
echo "output-exclusion ok"

# --- 7. detached HEAD ------------------------------------------------------------
D3=$(new_repo)
(cd "$D3" && git checkout -q --detach HEAD)
P=$(collect "$D3")
[ "$(field "$P" worktree)" = "clean" ] || fail "detached clean tree must be clean"
[ "$(field "$P" authoritative)" = "true" ] || fail "detached clean must be authoritative"
DETACHED_SHA=$(field "$P" head_sha)
[ "${#DETACHED_SHA}" = "40" ] || fail "detached head_sha malformed"
echo "detached HEAD ok"

# --- 8. subdirectory invocation --------------------------------------------------
mkdir -p "$D3/sub/dir"
P=$(collect "$D3/sub/dir")
[ "$(field "$P" worktree)" = "clean" ] || fail "subdir invocation must be clean"
[ "$(field "$P" head_sha)" = "$(cd "$D3" && git rev-parse HEAD)" ] || fail "subdir head mismatch"
echo "subdirectory ok"

# --- 9. missing Git repository ----------------------------------------------------
D4="$(mktemp -d)"
if (cd "$D4" && python3 "$COLLECTOR" --json >/dev/null 2>&1); then
  fail "non-git dir --json must exit nonzero"
fi
if (cd "$D4" && python3 "$COLLECTOR" --json --require-clean >/dev/null 2>&1); then
  fail "non-git dir --require-clean must exit nonzero"
fi
echo "missing-git failure ok"

# --- 10. collection never mutates Git state ----------------------------------------
D5=$(new_repo)
echo "edit" >>"$D5/a.txt"
BEFORE_HEAD=$(cd "$D5" && git rev-parse HEAD)
BEFORE_STATUS=$(cd "$D5" && git status --porcelain=v1 -uall)
(cd "$D5" && python3 "$COLLECTOR" --json >/dev/null)
AFTER_HEAD=$(cd "$D5" && git rev-parse HEAD)
AFTER_STATUS=$(cd "$D5" && git status --porcelain=v1 -uall)
[ "$BEFORE_HEAD" = "$AFTER_HEAD" ] || fail "collector moved HEAD"
[ "$BEFORE_STATUS" = "$AFTER_STATUS" ] || fail "collector changed worktree status"
echo "no-mutation ok"

rm -rf "$D" "$D2" "$D3" "$D4" "$D5"
echo '{"bench_provenance":"pass"}'
