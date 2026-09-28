#!/usr/bin/env sh
# M053 regression: planning-state drift guard.
#
# Proves `scripts/check_planning_state.py` still enforces
# `plans/registry.md` as the sole milestone-status authority:
# - inline fixture passes (malformed rows, stale blocks, determinism);
# - real-tree --check passes (all four generated blocks match);
# - deliberate drift fails (a temp copy with a corrupted block does
#   not verify), without touching the real checkout;
# - wiring is intact: `scripts/check.sh` invokes the guard and the
#   `language-clients` CI job invokes it independently, so removing
#   either still trips the other.
#
# Stdlib Python + POSIX sh only; well under one second.
set -eu

fail() { echo "FAIL: $1" >&2; exit 1; }

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
GUARD="$REPO_ROOT/scripts/check_planning_state.py"
WORKFLOW="$REPO_ROOT/.github/workflows/ci.yml"
CHECK_SH="$REPO_ROOT/scripts/check.sh"
[ -f "$GUARD" ] || fail "missing guard: $GUARD"

# 1. Inline fixture: malformed registry rows, unregistered plan files,
#    unknown dependencies, determinism, stale/missing block detection.
python3 "$GUARD" --fixture || fail "fixture self-test failed"

# 2. Real-tree check: registry parses clean and all four generated
#    blocks match.
python3 "$GUARD" --check || fail "real-tree --check failed"

# 3. Deliberate drift fails: corrupt one generated block on a temp
#    copy and prove check_target rejects it, without modifying the
#    real repository.
python3 - "$GUARD" <<'PY' || exit 1
import sys
from pathlib import Path
sys.path.insert(0, str(Path(sys.argv[1]).parent))
import check_planning_state as cps
import tempfile

text = cps.REGISTRY_PATH.read_text(encoding="utf-8")
rows = cps.parse_registry(text)
errs = cps.validate(rows)
assert not errs, errs
expected = cps.generate_block(rows)
with tempfile.TemporaryDirectory() as tmp:
    doc = Path(tmp) / "doc.md"
    doc.write_text("# t\n\n" + expected, encoding="utf-8")
    assert cps.check_target(doc, expected), "fresh block must verify"
    import contextlib, io
    doc.write_text("# t\n\n" + expected.replace("M053", "M099"), encoding="utf-8")
    with contextlib.redirect_stderr(io.StringIO()):
        assert not cps.check_target(doc, expected), "stale block must fail"
print("deliberate-drift negative test ok")
PY

# 4. Wiring: the cheap guard must live in scripts/check.sh (via this
# test script, which itself runs --fixture, --check, the
# deliberate-drift negative test, and these wiring assertions) ...
grep -q "test_planning_state\.sh" "$CHECK_SH" \
  || fail "scripts/check.sh must invoke test_planning_state.sh"
# ... and independently in one existing CI job (language-clients), so
# removing the local check still trips CI.
[ -f "$WORKFLOW" ] || fail "missing workflow: $WORKFLOW"
if ! awk '
  /^  language-clients:$/ { in_job=1; next }
  /^  [a-zA-Z][a-zA-Z0-9_-]*:$/ { in_job=0 }
  in_job && /check_planning_state\.py --check/ { found=1; exit 0 }
  END { exit (found ? 0 : 1) }
' "$WORKFLOW"; then
  fail "language-clients job must invoke check_planning_state.py --check"
fi
# No dedicated CI job for planning state (M053 forbids a second
# authority and a new job).
if grep -q "^  planning-state:" "$WORKFLOW"; then
  fail "must not add a dedicated planning-state CI job"
fi

echo '{"planning_state":"pass"}'
