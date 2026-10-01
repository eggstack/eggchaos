#!/usr/bin/env python3
"""M059 structural guard: first-party lockfile coherence.

The four committed Cargo lockfiles (root, python-native, benchmarks,
fuzz) must resolve the same versions of the first-party Eggstack
runtime crates M059 intentionally synchronizes. A lockfile that does
not depend on a given crate at all is skipped for that crate; the
checked set stays narrow on purpose and does not require
whole-lockfile identity across workspaces with different roles.

Stdlib only. Usage:
  python3 scripts/check_lock_coherence.py --check    # real tree
  python3 scripts/check_lock_coherence.py --fixture  # inline self-test
"""

import re
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent

# First-party Eggstack runtime crates synchronized by M059.
CHECKED_CRATES = (
    "eggress-relay",
    "eggfetch-core",
    "eggserve-primitives",
    "eggserve-server",
)

LOCKFILES = (
    "Cargo.lock",
    "bindings/python-native/Cargo.lock",
    "benchmarks/Cargo.lock",
    "fuzz/Cargo.lock",
)


def locked_versions(lock_text: str) -> dict:
    """Map crate name -> sorted version list from one lockfile."""
    found: dict = {}
    for name, version in re.findall(
        r'\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"', lock_text
    ):
        found.setdefault(name, []).append(version)
    return found


def check_tree(root: Path) -> list:
    """Return error strings for first-party version drift."""
    errors = []
    per_lock = {}
    for rel in LOCKFILES:
        path = root / rel
        if not path.is_file():
            errors.append(f"missing committed lockfile: {rel}")
            continue
        per_lock[rel] = locked_versions(path.read_text(encoding="utf-8"))
    for crate in CHECKED_CRATES:
        seen = {}
        for rel, locked in per_lock.items():
            versions = locked.get(crate, [])
            if not versions:
                continue  # lockfile does not depend on this crate
            if len(set(versions)) > 1:
                errors.append(f"{rel} resolves {crate} twice: {sorted(set(versions))}")
            seen.setdefault(versions[0], []).append(rel)
        if len(seen) > 1:
            detail = ", ".join(f"{v} in {sorted(r)}" for v, r in sorted(seen.items()))
            errors.append(f"first-party drift for {crate}: {detail}")
    return errors


def run_fixture() -> None:
    """Inline self-test: coherent passes, drifted fails."""
    good = '[[package]]\nname = "eggress-relay"\nversion = "1.0.11"\n'
    bad = '[[package]]\nname = "eggress-relay"\nversion = "1.0.7"\n'
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        for rel in LOCKFILES:
            path = root / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(good, encoding="utf-8")
        assert check_tree(root) == [], "coherent layout must pass"
        (root / LOCKFILES[0]).write_text(bad, encoding="utf-8")
        errors = check_tree(root)
        assert len(errors) == 1 and "eggress-relay" in errors[0], errors
    print("lock-coherence fixture ok")


def main(argv: list) -> int:
    if "--fixture" in argv:
        run_fixture()
        return 0
    if "--check" in argv:
        errors = check_tree(REPO_ROOT)
        if errors:
            for error in errors:
                print(f"FAIL: {error}", file=sys.stderr)
            return 1
        print('{"lock_coherence":"pass"}')
        return 0
    print("usage: check_lock_coherence.py (--check | --fixture)", file=sys.stderr)
    return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
