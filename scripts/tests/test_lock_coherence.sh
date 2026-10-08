#!/usr/bin/env sh
# M059 regression: lockfile-coherence guard.
#
# Proves `scripts/check_lock_coherence.py` still enforces both
# invariants across the four committed Cargo lockfiles:
# - inline fixture passes (coherent passes; first-party drift fails;
#   inherited drift fails for every sibling; directly-declared and
#   absent crates do not false-positive);
# - a realistic drift derived from the real manifests is caught by the
#   inherited-drift check, and the same tree without the drift passes;
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

# 1. Inline fixture: every invariant fires, and only where it should.
python3 "$GUARD" --fixture || fail "fixture self-test failed"

# 2. Real-tree check: all four lockfiles agree.
python3 "$GUARD" --check || fail "real-tree --check failed"

# 3. Realistic negative: downgrade one workspace-inherited crate in a
#    sibling lockfile of a copy of the real tree. The guard must fail
#    closed, and the untouched copy must pass.
python3 - "$GUARD" "$REPO_ROOT" <<'PY' || fail "inherited-drift negative test misfired"
import importlib.util
import re
import sys
import tempfile
from pathlib import Path

spec = importlib.util.spec_from_file_location("lockc", sys.argv[1])
guard = importlib.util.module_from_spec(spec)
spec.loader.exec_module(guard)
repo = Path(sys.argv[2])

with tempfile.TemporaryDirectory() as tmp:
    tree = Path(tmp)
    (tree / guard.ROOT_MANIFEST).write_text(
        (repo / guard.ROOT_MANIFEST).read_text(encoding="utf-8"), encoding="utf-8"
    )
    for manifest_rel, lock_rel in guard.SIBLING_WORKSPACES:
        (tree / manifest_rel).parent.mkdir(parents=True, exist_ok=True)
        shutil_copy = (repo / manifest_rel).read_text(encoding="utf-8")
        (tree / manifest_rel).write_text(shutil_copy, encoding="utf-8")
        (tree / lock_rel).parent.mkdir(parents=True, exist_ok=True)
        (tree / lock_rel).write_text(
            (repo / lock_rel).read_text(encoding="utf-8"), encoding="utf-8"
        )
    (tree / guard.ROOT_LOCK).write_text(
        (repo / guard.ROOT_LOCK).read_text(encoding="utf-8"), encoding="utf-8"
    )

    # The unmodified copy of the real tree must be coherent.
    assert guard.check_tree(tree) == [], "copied real tree must pass"

    # Pick a crate benchmarks inherits rather than declaring itself.
    declared = guard.workspace_declared_crates(
        (repo / guard.ROOT_MANIFEST).read_text(encoding="utf-8")
    )
    own = guard.directly_declared_crates(
        (repo / "benchmarks" / "Cargo.toml").read_text(encoding="utf-8")
    )
    inherited = sorted(declared - own - set(guard.CHECKED_CRATES))
    assert inherited, "expected at least one workspace-inherited crate"
    target = inherited[0]

    lock_path = tree / "benchmarks" / "Cargo.lock"
    text = lock_path.read_text(encoding="utf-8")
    patched, count = re.subn(
        r'(\[\[package\]\]\nname = "%s"\nversion = ")[^"]+(")' % re.escape(target),
        r"\g<1>0.0.0\g<2>",
        text,
        count=1,
    )
    assert count == 1, f"{target} not present in benchmarks/Cargo.lock"
    lock_path.write_text(patched, encoding="utf-8")

    errors = guard.check_tree(tree)
    assert any(
        "workspace-inherited drift" in error and target in error for error in errors
    ), errors
print("inherited-drift negative test ok")
PY

# 4. Wiring: this test script must run from scripts/check.sh ...
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