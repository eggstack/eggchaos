#!/usr/bin/env sh
# M060 regression: release-state / current-doc drift guard.
#
# Proves `scripts/check_release_state_docs.py` still detects the named
# drift classes while keeping the live repository clean:
# - inline fixture passes (clean tree, every drift class fails when
#   its specific phrase is reintroduced, unclosed registry produces
#   a registry-state failure);
# - real-tree --check passes (M058/M059 closed in registry and every
#   named live document records the published/historical authority
#   without pre-publication prose);
# - deliberate drift fails on a temp copy, without touching the real
#   checkout (one named drift class per case);
# - wiring is intact: `scripts/check.sh` invokes this test script
#   and the `language-clients` CI job invokes the bare `--check`
#   independently, so removing either still trips the other.
#
# Stdlib Python + POSIX sh only; well under one second.
set -eu

fail() { echo "FAIL: $1" >&2; exit 1; }

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
GUARD="$REPO_ROOT/scripts/check_release_state_docs.py"
WORKFLOW="$REPO_ROOT/.github/workflows/ci.yml"
CHECK_SH="$REPO_ROOT/scripts/check.sh"
[ -f "$GUARD" ] || fail "missing guard: $GUARD"

# 1. Inline fixture: clean tree passes, every drift class fails.
python3 "$GUARD" --fixture || fail "fixture self-test failed"

# 2. Real-tree check: registry closed for M058/M059 and every named
#    live document records the published/historical authority.
python3 "$GUARD" --check || fail "real-tree --check failed"

# 3. Deliberate drift fails on a temp copy: corrupt one named
#    (file, phrase) pair and prove --check rejects it on the temp
#    tree without modifying the real checkout. We exercise one drift
#    class per case so a single failure points at the regression.
python3 - "$GUARD" <<'PY' || exit 1
import sys
from pathlib import Path
sys.path.insert(0, str(Path(sys.argv[1]).parent))
import check_release_state_docs as csrd
import tempfile

def run_with(text_overrides):
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        # Mirror the docs the guard reads.
        for rel in (
            "plans/registry.md",
            "SECURITY.md",
            "docs/release-notes-v0.2.0.md",
            "architecture/overview.md",
            "architecture/tooling-distribution.md",
            ".github/workflows/release.yml",
        ):
            target = base / rel
            target.parent.mkdir(parents=True, exist_ok=True)
            if rel in text_overrides:
                target.write_text(text_overrides[rel], encoding="utf-8")
            else:
                # Use the real-repo copy so must_be_present checks have
                # a baseline to compare against; the override injects the
                # specific drift class.
                src = csrd.REPO_ROOT / rel
                target.write_text(src.read_text(encoding="utf-8"), encoding="utf-8")
        statuses = csrd.parse_registry_milestone_status(
            (base / "plans" / "registry.md").read_text(encoding="utf-8")
        )
        return csrd.evaluate(statuses, root=base)

cases = [
    ("SECURITY.md pre-publication row reintroduced",
     lambda text: text.__setitem__("SECURITY.md", "| v0.1.x  | Supported until v0.2.0 is published |\n")
                  or text.__setitem__("SECURITY.md", "| v0.1.x  | Supported until v0.2.0 is published |\n"),
     "SECURITY.md"),
    ("release notes still say M058 is ready",
     lambda text: text.__setitem__("docs/release-notes-v0.2.0.md",
        "M058 is ready. Publication requires an annotated `v0.2.0` tag.\n"),
     "docs/release-notes-v0.2.0.md"),
    ("overview still says M058 blocked on M059",
     lambda text: text.__setitem__("architecture/overview.md",
        "M059 ready as the pre-v0.2.0 hardening corrective; M058 publication blocked on M059.\n"),
     "architecture/overview.md"),
    ("tooling-distribution reverted to `npm install`",
     lambda text: text.__setitem__("architecture/tooling-distribution.md",
        "Unconditional `npm install` if needed.\n"),
     "architecture/tooling-distribution.md"),
    ("tooling-distribution reverted to crate-root allow(unsafe_code)",
     lambda text: text.__setitem__("architecture/tooling-distribution.md",
        "Asserts `allow(unsafe_code)` in `lib.rs`.\n"),
     "architecture/tooling-distribution.md"),
    ("release.yml retains the M058-blocked comment",
     lambda text: text.__setitem__(".github/workflows/release.yml",
        "name: release\n# M058 stays blocked until M059 closes\npermissions: contents: read\n"),
     ".github/workflows/release.yml"),
]

for label, mutate_fn, doc in cases:
    overrides = {}
    mutate_fn(overrides)
    fails = run_with(overrides)
    drift_hits = [f for f in fails if f[0] == "drift" and doc in f[1]]
    assert drift_hits, f"{label}: expected drift hit on {doc}, got: {fails}"
print("deliberate-drift negative tests ok")
PY

# 4. Wiring: the cheap guard must live in scripts/check.sh ...
grep -q "test_release_state_docs\.sh" "$CHECK_SH" \
    || fail "scripts/check.sh must invoke test_release_state_docs.sh"
# ... and independently in one existing CI job (language-clients),
# so removing the local check still trips CI.
[ -f "$WORKFLOW" ] || fail "missing workflow: $WORKFLOW"
if ! awk '
  /^  language-clients:$/ { in_job=1; next }
  /^  [a-zA-Z][a-zA-Z0-9_-]*:$/ { in_job=0 }
  in_job && /check_release_state_docs\.py --check/ { found=1; exit 0 }
  END { exit (found ? 0 : 1) }
' "$WORKFLOW"; then
  fail "language-clients job must invoke check_release_state_docs.py --check"
fi
# No dedicated CI job for release-state docs (M060 forbids a second
# authority and a new job).
if grep -q "^  release-state-docs:" "$WORKFLOW"; then
  fail "must not add a dedicated release-state-docs CI job"
fi

echo '{"release_state_docs":"pass"}'