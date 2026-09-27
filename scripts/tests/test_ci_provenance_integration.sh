#!/usr/bin/env sh
# M048 WP6 regression: structural guard against accidental CI de-integration
# of the M047 provenance tests.
#
# Proves `.github/workflows/ci.yml` still references both:
# - `test_bench_provenance.sh` (Tier A);
# - `test_bench_provenance_artifacts.sh` (Tier B).
#
# and that the placement respects M048's cost model:
# - Tier A appears in a non-Windows-only context (Linux + macOS only);
# - Tier B is not multiplied across a matrix that would build release
#   benchmarks on every entry.
#
# This is intentionally a static pattern check — no YAML parser
# dependency, no workflow execution. It runs locally in seconds and
# also runs in the language-clients CI job (cheap; same pattern as the
# existing test_cleanup_traps.sh).
set -eu

fail() { echo "FAIL: $1" >&2; exit 1; }

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
WORKFLOW="$REPO_ROOT/.github/workflows/ci.yml"
[ -f "$WORKFLOW" ] || fail "missing workflow: $WORKFLOW"

assert_pattern() {
  file="$1"
  pattern="$2"
  grep -q "$pattern" "$file" || fail "$file missing pattern: $pattern"
}

# 1. Tier A: cheap Git-state contract must be wired into CI on Linux and
#    macOS only (M048 explicitly does not claim Windows POSIX-shell
#    coverage).
assert_pattern "$WORKFLOW" "test_bench_provenance.sh"
# Tier A must be guarded so Windows is intentionally skipped, not just
# left to a silent shell-resolution accident.
if ! grep -B2 "test_bench_provenance.sh" "$WORKFLOW" | grep -q "runner.os"; then
  fail "Tier A must be guarded with a runner.os condition so Windows is explicit"
fi

# 2. Tier B: shortened artifact qualification must be wired into CI
#    exactly once on Linux (M048 cost model forbids matrix
#    multiplication of release benchmark builds).
assert_pattern "$WORKFLOW" "test_bench_provenance_artifacts.sh"
# Tier B must live in a dedicated job, not inside the `check` matrix.
if awk '
  /^  [a-zA-Z][a-zA-Z0-9_-]*:$/ { current=$1; in_check=(current=="check:") }
  /test_bench_provenance_artifacts.sh/ { if (in_check) { print "in_check"; exit 0 } }
' "$WORKFLOW" | grep -q "in_check"; then
  fail "Tier B must not live inside the check matrix"
fi

# 3. The dedicated Tier B job must exist (the preferred placement from
#    M048 WP3) and must be Linux only.
if ! grep -q "^  performance-provenance:" "$WORKFLOW"; then
  fail "expected a dedicated 'performance-provenance:' job in the workflow"
fi
if ! awk '
  /^  performance-provenance:$/ { in_job=1; next }
  /^  [a-zA-Z][a-zA-Z0-9_-]*:$/ { in_job=0 }
  in_job && /runs-on: ubuntu-latest/ { found=1; exit 0 }
  END { exit (found ? 0 : 1) }
' "$WORKFLOW"; then
  fail "performance-provenance job must run on ubuntu-latest only"
fi

# 4. The Tier B job must declare its own timeout (M048 forbids absorbing
#    the work into the 25-minute check bound without an explicit bound).
if ! awk '
  /^  performance-provenance:$/ { in_job=1; next }
  /^  [a-zA-Z][a-zA-Z0-9_-]*:$/ { in_job=0 }
  in_job && /timeout-minutes:/ { found=1; exit 0 }
  END { exit (found ? 0 : 1) }
' "$WORKFLOW"; then
  fail "performance-provenance job must declare its own timeout-minutes"
fi

echo '{"ci_provenance_integration":"pass"}'
