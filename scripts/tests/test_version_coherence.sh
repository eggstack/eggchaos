#!/usr/bin/env sh
# M055 regression: version-coherence guard.
#
# Proves `scripts/check_version_coherence.py` still enforces one
# repository development version derived from the Rust workspace:
# - inline fixture passes (clean tree) and every stale-manifest class
#   fails (member version, intra-workspace req, Python/TypeScript
#   clients, Python-native Python/Rust manifests, benchmarks);
# - real-tree --check passes (all manifests coherent at 0.2.0);
# - deliberate mismatch fails (a temp fixture copy with one stale
#   requirement does not verify), without touching the real checkout;
# - wiring is intact: `scripts/check.sh` invokes this test and the
#   `language-clients` CI job invokes the bare `--check` independently,
#   so removing either still trips the other.
#
# Stdlib Python + POSIX sh only; well under one second.
set -eu

fail() { echo "FAIL: $1" >&2; exit 1; }

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
GUARD="$REPO_ROOT/scripts/check_version_coherence.py"
WORKFLOW="$REPO_ROOT/.github/workflows/ci.yml"
CHECK_SH="$REPO_ROOT/scripts/check.sh"
[ -f "$GUARD" ] || fail "missing guard: $GUARD"

# 1. Inline fixture: clean tree passes, every stale class fails.
python3 "$GUARD" --fixture || fail "fixture self-test failed"

# 2. Real-tree check: all first-party manifests coherent.
python3 "$GUARD" --check || fail "real-tree --check failed"

# 3. Deliberate mismatch fails: stage a temp fixture tree with one
#    stale intra-workspace requirement and prove the checker rejects
#    it, without modifying the real repository.
python3 - "$GUARD" <<'PY' || exit 1
import json
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(sys.argv[1]).parent))
import check_version_coherence as cvc

with tempfile.TemporaryDirectory() as tmp:
    base = Path(tmp)
    (base / "crates" / "demo").mkdir(parents=True)
    (base / "bindings" / "python-client").mkdir(parents=True)
    (base / "bindings" / "typescript-client").mkdir(parents=True)
    (base / "bindings" / "python-native").mkdir(parents=True)
    (base / "benchmarks").mkdir(parents=True)
    (base / "Cargo.toml").write_text(
        '[workspace]\nmembers = ["crates/demo"]\n'
        '[workspace.package]\nversion = "0.2.0"\n',
        encoding="utf-8",
    )
    (base / "crates" / "demo" / "Cargo.toml").write_text(
        '[package]\nname = "eggchaos-demo"\nversion.workspace = true\n'
        '[dependencies]\n'
        'eggchaos-core = { path = "../eggchaos-core", version = "0.1.0" }\n',
        encoding="utf-8",
    )
    (base / "bindings" / "python-client" / "pyproject.toml").write_text(
        '[project]\nname = "eggchaos-client"\nversion = "0.2.0"\n',
        encoding="utf-8",
    )
    (base / "bindings" / "typescript-client" / "package.json").write_text(
        json.dumps({"version": "0.2.0"}), encoding="utf-8",
    )
    (base / "bindings" / "typescript-client" / "package-lock.json").write_text(
        json.dumps({"version": "0.2.0", "packages": {"": {"version": "0.2.0"}}}),
        encoding="utf-8",
    )
    (base / "bindings" / "python-native" / "pyproject.toml").write_text(
        '[project]\nname = "eggchaos-native"\nversion = "0.2.0"\n',
        encoding="utf-8",
    )
    (base / "bindings" / "python-native" / "Cargo.toml").write_text(
        '[workspace]\n[package]\nname = "eggchaos-native"\nversion = "0.2.0"\n'
        '[dependencies]\n'
        'eggchaos-embed = { path = "../../crates/eggchaos-embed", '
        'version = "0.2.0" }\n',
        encoding="utf-8",
    )
    (base / "benchmarks" / "Cargo.toml").write_text(
        '[workspace]\n[package]\nname = "eggchaos-benchmarks"\n'
        'version = "0.2.0"\n[dependencies]\n'
        'eggchaos-core = { path = "../crates/eggchaos-core", '
        'version = "0.2.0" }\n',
        encoding="utf-8",
    )
    failures, ws, _ = cvc.check_tree(base)
    assert ws == "0.2.0", ws
    assert failures, "stale intra-workspace req must fail"
    assert any("eggchaos-core" in f for f in failures), failures
print("deliberate-mismatch negative test ok")
PY

# 4. Wiring: the cheap guard must live in scripts/check.sh (via this
# test script, which itself runs --fixture, --check, the
# deliberate-mismatch negative test, and these wiring assertions) ...
grep -q "test_version_coherence\.sh" "$CHECK_SH" \
  || fail "scripts/check.sh must invoke test_version_coherence.sh"
# ... and the bare --check independently in one existing CI job
# (language-clients), so removing the local check still trips CI.
[ -f "$WORKFLOW" ] || fail "missing workflow: $WORKFLOW"
if ! awk '
  /^  language-clients:$/ { in_job=1; next }
  /^  [a-zA-Z][a-zA-Z0-9_-]*:$/ { in_job=0 }
  in_job && /check_version_coherence\.py --check/ { found=1; exit 0 }
  END { exit (found ? 0 : 1) }
' "$WORKFLOW"; then
  fail "language-clients job must invoke check_version_coherence.py --check"
fi
# No dedicated CI job for version coherence (no new job per M055 WP5).
if grep -q "^  version-coherence:" "$WORKFLOW"; then
  fail "must not add a dedicated version-coherence CI job"
fi

echo '{"version_coherence_guard":"pass"}'
