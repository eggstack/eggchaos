#!/usr/bin/env python3
"""M059 structural guard: first-party lockfile coherence.

Two independent invariants over the four committed Cargo lockfiles
(root, python-native, benchmarks, fuzz):

1. First-party convergence. The Eggstack runtime crates M059
   intentionally synchronizes must resolve to one version everywhere.
2. Workspace-inherited convergence. A crate declared in the root
   `[workspace.dependencies]` table is inherited by every workspace
   member through `foo.workspace = true`. The sibling workspaces reach
   those members by path, so a root bump silently re-resolves each
   sibling lockfile. If a sibling resolves a different version than the
   root lock, the bump was applied to only one manifest.

Invariant 2 is scoped per sibling: a sibling that declares a crate
directly owns that crate's resolution and is exempt. Without that
carve-out the check would false-positive on legitimate independent
resolution (fuzz pins its own `toml`; benchmarks pins its own `tokio`),
and requiring whole-lockfile identity would be wrong across workspaces
with different roles.

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

ROOT_MANIFEST = "Cargo.toml"
ROOT_LOCK = "Cargo.lock"

LOCKFILES = (
    "Cargo.lock",
    "bindings/python-native/Cargo.lock",
    "benchmarks/Cargo.lock",
    "fuzz/Cargo.lock",
)

# Sibling workspace manifest -> its committed lockfile. Each reaches the
# root workspace crates by path dependency.
SIBLING_WORKSPACES = (
    ("benchmarks/Cargo.toml", "benchmarks/Cargo.lock"),
    ("fuzz/Cargo.toml", "fuzz/Cargo.lock"),
    ("bindings/python-native/Cargo.toml", "bindings/python-native/Cargo.lock"),
)

DEPENDENCY_TABLES = ("dependencies", "dev-dependencies", "build-dependencies")


def _table_body(manifest_text: str, header: str) -> str:
    """Return the body of a `[header]` table, or "" when absent."""
    match = re.search(
        rf"^\[{re.escape(header)}\]\s*$(.*?)(?=^\[|\Z)",
        manifest_text,
        re.MULTILINE | re.DOTALL,
    )
    return match.group(1) if match else ""


def _key_names(body: str) -> set:
    """Dependency keys declared in one table body (`name = ...` lines)."""
    names = set()
    for line in body.splitlines():
        # Strip a trailing comment before matching the key.
        candidate = re.match(
            r"^([A-Za-z0-9_-]+)\s*=", line.split("#", 1)[0].strip()
        )
        if candidate:
            names.add(candidate.group(1))
    return names


def workspace_declared_crates(manifest_text: str) -> set:
    """Crate names in the root `[workspace.dependencies]` table.

    These are the crates a member inherits with `foo.workspace = true`;
    their root bump propagates into every sibling lockfile by path.
    """
    return _key_names(_table_body(manifest_text, "workspace.dependencies"))


def directly_declared_crates(manifest_text: str) -> set:
    """Crate names a manifest declares itself, across all dep tables."""
    names = set()
    for table in DEPENDENCY_TABLES:
        names |= _key_names(_table_body(manifest_text, table))
    return names


def locked_versions(lock_text: str) -> dict:
    """Map crate name -> sorted version list from one lockfile."""
    found: dict = {}
    for name, version in re.findall(
        r'\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"', lock_text
    ):
        found.setdefault(name, []).append(version)
    return found


def check_first_party(per_lock: dict) -> list:
    """First-party Eggstack runtime crates must converge everywhere."""
    errors = []
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


def check_workspace_inherited(root: Path, per_lock: dict) -> list:
    """Root `[workspace.dependencies]` bumps must reach sibling lockfiles.

    A sibling that declares a crate directly owns its resolution and is
    exempt for that crate; only crates it inherits are compared.
    """
    manifest_path = root / ROOT_MANIFEST
    if not manifest_path.is_file():
        return [f"missing root manifest: {ROOT_MANIFEST}"]

    declared = workspace_declared_crates(manifest_path.read_text(encoding="utf-8"))
    if not declared:
        return []  # nothing to enforce without a workspace dependency table

    root_locked = per_lock.get(ROOT_LOCK, {})
    errors = []
    for manifest_rel, lock_rel in SIBLING_WORKSPACES:
        sibling_manifest = root / manifest_rel
        sibling_lock = per_lock.get(lock_rel)
        if not sibling_manifest.is_file() or sibling_lock is None:
            continue  # absence is already reported by the lockfile sweep
        own = directly_declared_crates(
            sibling_manifest.read_text(encoding="utf-8")
        )
        # First-party crates are also workspace-declared, so invariant 1
        # already reports them with the more precise message. Exclude
        # them here so a single root cause yields a single failure.
        inherited = declared - own - set(CHECKED_CRATES)
        for crate in sorted(inherited):
            root_versions = set(root_locked.get(crate, []))
            sibling_versions = set(sibling_lock.get(crate, []))
            if not root_versions or not sibling_versions:
                continue  # crate absent from one side: not a version drift
            if root_versions != sibling_versions:
                errors.append(
                    f"workspace-inherited drift for {crate}: "
                    f"{sorted(root_versions)} in [{ROOT_LOCK}]; "
                    f"{sorted(sibling_versions)} in [{lock_rel}]"
                )
    return errors


def check_tree(root: Path) -> list:
    """Return error strings for every coherence violation."""
    errors = []
    per_lock = {}
    for rel in LOCKFILES:
        path = root / rel
        if not path.is_file():
            errors.append(f"missing committed lockfile: {rel}")
            continue
        per_lock[rel] = locked_versions(path.read_text(encoding="utf-8"))
    if not per_lock:
        return errors
    errors.extend(check_first_party(per_lock))
    errors.extend(check_workspace_inherited(root, per_lock))
    return errors


def _lock(*entries: tuple) -> str:
    return "".join(
        f'[[package]]\nname = "{name}"\nversion = "{version}"\n'
        for name, version in entries
    )


def _write_tree(root: Path, *, root_manifest: str, sibling_manifests: dict,
                lock_versions: dict) -> None:
    """Materialize a miniature four-lockfile repo for the fixture."""
    (root / ROOT_MANIFEST).write_text(root_manifest, encoding="utf-8")
    for manifest_rel, lock_rel in SIBLING_WORKSPACES:
        manifest_path = root / manifest_rel
        manifest_path.parent.mkdir(parents=True, exist_ok=True)
        manifest_path.write_text(sibling_manifests[manifest_rel], encoding="utf-8")
        lock_path = root / lock_rel
        lock_path.parent.mkdir(parents=True, exist_ok=True)
        lock_path.write_text(_lock(*lock_versions[lock_rel]), encoding="utf-8")
    (root / ROOT_LOCK).write_text(
        _lock(*lock_versions[ROOT_LOCK]), encoding="utf-8"
    )


def run_fixture() -> None:
    """Inline self-test: every invariant fires, and only where it should."""
    root_manifest = (
        "[workspace]\nmembers = []\n\n"
        "[workspace.dependencies]\n"
        'sha2 = "0.11"\n'
        "toml = \"1.1\"\n"
        "serde = \"1\"\n"
        'eggress-relay = { version = "1.0.11" }\n'
    )
    # fuzz owns `toml` and `serde` directly; the others are inherited.
    sibling_manifests = {
        "benchmarks/Cargo.toml": (
            "[dependencies]\neggchaos-server = { path = \"../crates/eggchaos-server\" }\n"
            "serde_json = \"1\"\n"
        ),
        "fuzz/Cargo.toml": "[dependencies]\ntoml = \"1.1\"\nserde = \"1\"\nserde_json = \"1\"\n",
        "bindings/python-native/Cargo.toml": (
            "[dependencies]\nserde = \"1\"\nserde_json = \"1\"\n"
        ),
    }
    good_locks = {
        ROOT_LOCK: (("sha2", "0.11.0"), ("toml", "1.1.6"), ("serde", "1.0.229"),
                    ("eggress-relay", "1.0.11")),
        "benchmarks/Cargo.lock": (("sha2", "0.11.0"), ("toml", "1.1.6"),
                                  ("serde", "1.0.229"), ("eggress-relay", "1.0.11")),
        "fuzz/Cargo.lock": (("sha2", "0.11.0"), ("toml", "1.1.6"),
                            ("serde", "1.0.229"), ("eggress-relay", "1.0.11")),
        "bindings/python-native/Cargo.lock": (("sha2", "0.11.0"), ("toml", "1.1.6"),
                                              ("serde", "1.0.229"),
                                              ("eggress-relay", "1.0.11")),
    }

    def build(mutate=None):
        locks = {rel: list(v) for rel, v in good_locks.items()}
        if mutate:
            mutate(locks)
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            _write_tree(root, root_manifest=root_manifest,
                        sibling_manifests=sibling_manifests, lock_versions=locks)
            return check_tree(root)

    # 1. Coherent layout passes.
    assert build() == [], "coherent layout must pass"

    # 2. First-party drift is still caught.
    errors = build(lambda l: l.__setitem__(
        "benchmarks/Cargo.lock",
        [(n, "1.0.7" if n == "eggress-relay" else v) for n, v in l["benchmarks/Cargo.lock"]]))
    assert len(errors) == 1 and "first-party drift" in errors[0], errors

    # 3. A root bump that never reached a sibling lockfile is caught.
    #    This is the Dependabot single-manifest failure mode.
    def stale_sha2(locks):
        locks["benchmarks/Cargo.lock"] = [
            (n, "0.10.9" if n == "sha2" else v) for n, v in locks["benchmarks/Cargo.lock"]
        ]

    errors = build(stale_sha2)
    assert len(errors) == 1, errors
    assert "workspace-inherited drift for sha2" in errors[0], errors
    assert "benchmarks/Cargo.lock" in errors[0], errors

    # 4. Every sibling can be caught, not just benchmarks.
    for stale in ("fuzz/Cargo.lock", "bindings/python-native/Cargo.lock"):
        errors = build(lambda l, s=stale: l.__setitem__(
            s, [(n, "0.10.9" if n == "sha2" else v) for n, v in l[s]]))
        assert len(errors) == 1 and stale in errors[0], (stale, errors)

    # 5. No false positive: a sibling may pin an inherited-named crate at
    #    its own version, and must not be reported.
    def own_toml_version(locks):
        locks["fuzz/Cargo.lock"] = [
            (n, "1.0.0" if n == "toml" else v) for n, v in locks["fuzz/Cargo.lock"]
        ]

    assert build(own_toml_version) == [], "directly declared crate must be exempt"

    # 6. No false positive for crates absent from one lockfile.
    def drop_from_benchmarks(locks):
        locks["benchmarks/Cargo.lock"] = [
            (n, v) for n, v in locks["benchmarks/Cargo.lock"] if n != "toml"
        ]

    assert build(drop_from_benchmarks) == [], "absent crate is not a version drift"

    # 7. A missing sibling manifest must not crash the guard.
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        _write_tree(root, root_manifest=root_manifest,
                    sibling_manifests=sibling_manifests,
                    lock_versions={rel: list(v) for rel, v in good_locks.items()})
        (root / "fuzz/Cargo.toml").unlink()
        assert check_tree(root) == [], "absent sibling manifest is not a drift"

    # 8. A missing root manifest is reported, not silently skipped.
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        _write_tree(root, root_manifest=root_manifest,
                    sibling_manifests=sibling_manifests,
                    lock_versions={rel: list(v) for rel, v in good_locks.items()})
        (root / ROOT_MANIFEST).unlink()
        errors = check_tree(root)
        assert errors == ["missing root manifest: Cargo.toml"], errors

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