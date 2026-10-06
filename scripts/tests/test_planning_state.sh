#!/usr/bin/env sh
# M053 regression: planning-state drift guard.
# M061 regression: registry <-> plan-header status agreement.
#
# Proves `scripts/check_planning_state.py` still enforces
# `plans/registry.md` as the sole milestone-status authority:
# - inline fixture passes (malformed rows, plan-header status
#   mismatch / missing / duplicate / unparseable classes, stale
#   blocks, determinism);
# - real-tree --check passes (every registered plan header agrees
#   with its registry row, and all four generated blocks match);
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

# 1. Inline fixture: malformed registry rows, plan-header status
#    agreement classes, unregistered plan files, unknown
#    dependencies, determinism, stale/missing block detection.
python3 "$GUARD" --fixture || fail "fixture self-test failed"

# 2. Real-tree check: registry parses clean, every registered plan
#    header agrees with its registry row, and all four generated
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
    # Corrupt the highest-closed milestone token actually rendered in
    # the block (state-independent: hardcoding one milestone ID breaks
    # as soon as that milestone leaves the rendered state).
    highest = [r.milestone for r in rows if r.status == "closed"][-1]
    assert highest in expected, "highest closed milestone must be rendered"
    doc.write_text("# t\n\n" + expected.replace(highest, "M099", 1), encoding="utf-8")
    with contextlib.redirect_stderr(io.StringIO()):
        assert not cps.check_target(doc, expected), "stale block must fail"
print("deliberate-drift negative test ok")
PY

# 3b. Plan-header status agreement against the real registry (M061).
#     Copies the live plans directory into a temp tree so the real
#     checkout is never modified, proves the live tree is already
#     clean, and then drives one real plan's `Status:` line into
#     each failure class to prove the guard fails closed.
python3 - "$GUARD" <<'PY' || exit 1
import re
import shutil
import sys
import tempfile
from pathlib import Path

guard = Path(sys.argv[1]).resolve()
sys.path.insert(0, str(guard.parent))
import check_planning_state as cps

rows = cps.parse_registry(cps.REGISTRY_PATH.read_text(encoding="utf-8"))
assert rows, "registry must parse"

with tempfile.TemporaryDirectory() as tmp:
    plans = Path(tmp) / "plans"
    plans.mkdir()
    for p in cps.PLANS_DIR.iterdir():
        if p.is_file():
            shutil.copy2(p, plans / p.name)

    # The live tree must already agree with the registry.
    live = cps.validate(rows, plans_dir=plans)
    assert not live, f"live tree plan-header drift: {live}"

    # Drive one real plan header through each failure class.
    target = next(r for r in rows if r.status == "closed" and r.plan.endswith(".md"))
    victim = plans / target.plan
    original = victim.read_text(encoding="utf-8")

    def run():
        return cps.validate(rows, plans_dir=plans)

    def expect(kind, header, label):
        patched = re.sub(r"^Status:.*$", header, original, count=1, flags=re.M)
        assert patched != original, "status line not found in " + target.plan
        victim.write_text(patched, encoding="utf-8")
        try:
            errs = run()
        finally:
            victim.write_text(original, encoding="utf-8")
        hit = [e for e in errs if e.kind == kind and target.milestone in e.detail]
        assert hit, f"{label}: expected {kind} for {target.milestone}, got {errs}"
        assert not run(), f"{label}: guard must be clean once the header is restored"

    # An explanatory suffix after a valid status token is accepted;
    # real plans already use this form.
    suffixed = re.sub(
        r"^Status:.*$",
        "Status: closed (evidence in `plans/closure/example-closure.md`)",
        original,
        count=1,
        flags=re.M,
    )
    victim.write_text(suffixed, encoding="utf-8")
    try:
        errs = run()
    finally:
        victim.write_text(original, encoding="utf-8")
    assert not errs, f"explanatory status suffix must be accepted, got {errs}"

    expect("plan-status-mismatch", "Status: ready", "registry closed / plan ready")
    expect("plan-status-mismatch", "Status: blocked", "registry closed / plan blocked")
    expect("plan-status-missing", "Depends on: M000", "missing status header")
    expect(
        "plan-status-duplicate",
        "Status: closed\nStatus: ready",
        "duplicate status headers",
    )
    expect("plan-status-unparseable", "Status: ~~frozen~~", "unparseable status header")

print("plan-header status agreement negative tests ok")
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
