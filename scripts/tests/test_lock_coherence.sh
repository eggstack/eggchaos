#!/usr/bin/env sh
# M059 regression: first-party lockfile-coherence guard.
#
# Proves `scripts/check_lock_coherence.py` still enforces a single
# resolved version per first-party Eggstack runtime crate across the
# four committed Cargo lockfiles:
# - inline fixture passes (coherent passes, drifted fails);
# - real-tree --check passes;
# - wiring is intact: `scripts/check.sh` invokes this test script and
#   the `language-clients` CI job invokes the bare --check, so
#   removing either still trips the other.
#
# Stdlib Python + POSIX sh only; well under one second.
set -eu

fail() { echo "FAIL: $1" >&2; exit 1; }

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
GUARD="$REPO_ROOT/scripts/check_lock_coherence.py"
WORKFLOW="$REPO_ROOT/.github/workflows/ci.yml"
CHECK_SH="$REPO_ROOT/scripts/check.sh"
[ -f "$GUARD" ] || fail "missing guard: $GUARD"

# 1. Inline fixture: coherent layout passes, drifted layout fails.
python3 "$GUARD" --fixture || fail "fixture self-test failed"

# 2. Real-tree check: all four lockfiles agree on first-party versions.
python3 "$GUARD" --check || fail "real-tree --check failed"

# 3. Wiring: this test script must run from scripts/check.sh ...
grep -q "test_lock_coherence\.sh" "$CHECK_SH" \
  || fail "scripts/check.sh must invoke test_lock_coherence.sh"
# ... and the bare --check independently in one existing CI job
# (language-clients), so removing the local check still trips CI.
[ -f "$WORKFLOW" ] || fail "missing workflow: $WORKFLOW"
if ! awk '
  /^  language-clients:$/ { in_job=1; next }
  /^  [a-zA-Z][a-zA-Z0-9_-]*:$/ { in_job=0 }
  in_job && /check_lock_coherence\.py --check/ { found=1; exit 0 }
  END { exit (found ? 0 : 1) }
' "$WORKFLOW"; then
  fail "language-clients job must invoke check_lock_coherence.py --check"
fi
# No dedicated CI job for lock coherence (one authority, no new job).
if grep -q "^  lock-coherence:" "$WORKFLOW"; then
  fail "must not add a dedicated lock-coherence CI job"
fi

echo '{"lock_coherence":"pass"}'
