#!/usr/bin/env python3
"""M055 version-coherence authority (stdlib only, no network, no mutation).

Derives the intended repository development package version from the Rust
workspace (`[workspace.package].version` in the root `Cargo.toml`) and
verifies that every first-party manifest agrees with it:

- every workspace member crate inherits or resolves the workspace version;
- every intra-workspace path dependency requirement equals it;
- the Python remote-client version equals it;
- the TypeScript remote-client version (plus lockfile) equals it;
- the Python-native Python and Rust package versions equal it;
- the Python-native local eggchaos dependency requirements equal it;
- the internal benchmarks crate (separate workspace, `publish = false`)
  package version and local eggchaos requirements equal it.

Historical `v0.1.0` prose, provenance records, OpenAPI `1.0.0`, config
schema v1, RNG v1, and compiler-semantics versions are out of scope and
must not be rejected merely for containing an old number.

Usage:
    python3 scripts/check_version_coherence.py [--check] [--root DIR]
    python3 scripts/check_version_coherence.py --fixture   # inline self-test

Exit 0 with `{"version_coherence":"pass",...}` on success; exit 1 with
`FAIL:` lines on any disagreement.
"""

import json
import sys
import tomllib
from pathlib import Path

SCRIPT_DIR = Path(__file__).resolve().parent
DEFAULT_ROOT = SCRIPT_DIR.parent

WORKSPACE_MEMBERS_KEY = ("workspace", "members")
WORKSPACE_VERSION_KEY = ("workspace", "package", "version")


def load_toml(path: Path):
    try:
        with open(path, "rb") as fh:
            return tomllib.load(fh)
    except FileNotFoundError:
        return None
    except tomllib.TOMLDecodeError as exc:
        raise ValueError(f"{path}: invalid TOML: {exc}") from exc


def check_tree(root: Path):
    """Return a list of failure strings (empty means coherent)."""
    failures = []

    root_manifest = root / "Cargo.toml"
    root_doc = load_toml(root_manifest)
    if root_doc is None:
        return [f"missing workspace manifest: {root_manifest}"]
    try:
        ws_version = root_doc["workspace"]["package"]["version"]
        members = root_doc["workspace"]["members"]
    except KeyError as exc:
        return [f"{root_manifest}: missing workspace key: {exc}"]
    if not isinstance(ws_version, str) or not ws_version:
        return [f"{root_manifest}: workspace version must be a non-empty string"]
    if not isinstance(members, list) or not members:
        return [f"{root_manifest}: workspace members must be a non-empty list"]

    checked_crates = 0

    def check_cargo_manifest(path: Path, *, member: bool):
        nonlocal checked_crates
        doc = load_toml(path)
        if doc is None:
            failures.append(f"missing manifest: {path}")
            return
        pkg = doc.get("package", {})
        name = pkg.get("name", path.parent.name)
        version = pkg.get("version")
        if isinstance(version, dict) and version.get("workspace") is True:
            resolved = ws_version
        elif isinstance(version, str):
            resolved = version
            if member and version != ws_version:
                failures.append(
                    f"{path}: package {name!r} version {version!r} != "
                    f"workspace version {ws_version!r}"
                )
            elif not member and version != ws_version:
                failures.append(
                    f"{path}: package {name!r} version {version!r} != "
                    f"workspace version {ws_version!r}"
                )
        else:
            failures.append(f"{path}: package {name!r} has no resolvable version")
            resolved = None
        if member:
            checked_crates += 1
        # Intra-workspace path dependency requirements must equal the
        # workspace version (plain `path` deps without a version, as in
        # fuzz/, carry nothing to check).
        dep_sections = []
        for section in ("dependencies", "dev-dependencies", "build-dependencies"):
            section_doc = doc.get(section)
            if isinstance(section_doc, dict):
                dep_sections.append((section, section_doc))
        target = doc.get("target")
        if isinstance(target, dict):
            for _triple, triple_doc in target.items():
                if isinstance(triple_doc, dict):
                    for section in (
                        "dependencies",
                        "dev-dependencies",
                        "build-dependencies",
                    ):
                        section_doc = triple_doc.get(section)
                        if isinstance(section_doc, dict):
                            dep_sections.append((f"target.{section}", section_doc))
        for section, section_doc in dep_sections:
            for dep_name, dep_spec in section_doc.items():
                if not dep_name.startswith("eggchaos-"):
                    continue
                if not isinstance(dep_spec, dict) or "path" not in dep_spec:
                    continue
                req = dep_spec.get("version")
                if req != ws_version:
                    failures.append(
                        f"{path}: [{section}] {dep_name!r} requirement "
                        f"{req!r} != workspace version {ws_version!r}"
                    )
        return resolved

    for member in members:
        check_cargo_manifest(root / member / "Cargo.toml", member=True)

    # Internal benchmarks crate: separate workspace, publish = false, but
    # its local eggchaos requirements must track the workspace version or
    # the harness stops building against current main.
    check_cargo_manifest(root / "benchmarks" / "Cargo.toml", member=False)

    # Standalone maturin crate: own package version plus local requirements.
    check_cargo_manifest(root / "bindings" / "python-native" / "Cargo.toml", member=False)

    # First-party language package metadata.
    py_client = root / "bindings" / "python-client" / "pyproject.toml"
    py_doc = load_toml(py_client)
    if py_doc is None:
        failures.append(f"missing manifest: {py_client}")
    else:
        try:
            py_version = py_doc["project"]["version"]
        except KeyError:
            py_version = None
        if py_version != ws_version:
            failures.append(
                f"{py_client}: version {py_version!r} != "
                f"workspace version {ws_version!r}"
            )

    ts_pkg = root / "bindings" / "typescript-client" / "package.json"
    try:
        ts_doc = json.loads(ts_pkg.read_text(encoding="utf-8"))
    except FileNotFoundError:
        failures.append(f"missing manifest: {ts_pkg}")
        ts_doc = None
    except json.JSONDecodeError as exc:
        failures.append(f"{ts_pkg}: invalid JSON: {exc}")
        ts_doc = None
    if ts_doc is not None:
        if ts_doc.get("version") != ws_version:
            failures.append(
                f"{ts_pkg}: version {ts_doc.get('version')!r} != "
                f"workspace version {ws_version!r}"
            )
    ts_lock = root / "bindings" / "typescript-client" / "package-lock.json"
    try:
        lock_doc = json.loads(ts_lock.read_text(encoding="utf-8"))
    except FileNotFoundError:
        failures.append(f"missing manifest: {ts_lock}")
        lock_doc = None
    except json.JSONDecodeError as exc:
        failures.append(f"{ts_lock}: invalid JSON: {exc}")
        lock_doc = None
    if lock_doc is not None:
        if lock_doc.get("version") != ws_version:
            failures.append(
                f"{ts_lock}: top-level version {lock_doc.get('version')!r} != "
                f"workspace version {ws_version!r}"
            )
        root_entry = lock_doc.get("packages", {}).get("", {})
        if root_entry.get("version") != ws_version:
            failures.append(
                f"{ts_lock}: packages[\"\"].version {root_entry.get('version')!r} "
                f"!= workspace version {ws_version!r}"
            )

    native_py = root / "bindings" / "python-native" / "pyproject.toml"
    native_doc = load_toml(native_py)
    if native_doc is None:
        failures.append(f"missing manifest: {native_py}")
    else:
        try:
            native_version = native_doc["project"]["version"]
        except KeyError:
            native_version = None
        if native_version != ws_version:
            failures.append(
                f"{native_py}: version {native_version!r} != "
                f"workspace version {ws_version!r}"
            )

    return failures, ws_version, checked_crates


def run_fixture():
    """Inline self-test: every mismatch class must fail on a temp tree."""
    import tempfile

    def write_tree(base: Path, *, ws_version="0.2.0", member_version=None,
                   dep_req=None, py_version=None, ts_version=None,
                   native_py_version=None, native_rs_version=None,
                   native_dep_req=None, bench_version=None, bench_dep_req=None):
        mv = member_version if member_version is not None else {"workspace": True}
        dq = dep_req if dep_req is not None else ws_version
        (base / "crates" / "demo").mkdir(parents=True, exist_ok=True)
        (base / "bindings" / "python-client").mkdir(parents=True, exist_ok=True)
        (base / "bindings" / "typescript-client").mkdir(parents=True, exist_ok=True)
        (base / "bindings" / "python-native").mkdir(parents=True, exist_ok=True)
        (base / "benchmarks").mkdir(parents=True, exist_ok=True)
        (base / "Cargo.toml").write_text(
            "[workspace]\nmembers = [\"crates/demo\"]\n"
            f"[workspace.package]\nversion = \"{ws_version}\"\n",
            encoding="utf-8",
        )
        if isinstance(mv, dict):
            pkg_version_line = 'version.workspace = true'
        else:
            pkg_version_line = f'version = "{mv}"'
        (base / "crates" / "demo" / "Cargo.toml").write_text(
            "[package]\nname = \"eggchaos-demo\"\n" + pkg_version_line + "\n"
            "[dependencies]\n"
            f"eggchaos-core = {{ path = \"../eggchaos-core\", version = \"{dq}\" }}\n",
            encoding="utf-8",
        )
        pv = py_version if py_version is not None else ws_version
        (base / "bindings" / "python-client" / "pyproject.toml").write_text(
            f"[project]\nname = \"eggchaos-client\"\nversion = \"{pv}\"\n",
            encoding="utf-8",
        )
        tv = ts_version if ts_version is not None else ws_version
        (base / "bindings" / "typescript-client" / "package.json").write_text(
            json.dumps({"name": "@eggstack/eggchaos-client", "version": tv}),
            encoding="utf-8",
        )
        (base / "bindings" / "typescript-client" / "package-lock.json").write_text(
            json.dumps(
                {
                    "name": "@eggstack/eggchaos-client",
                    "version": tv,
                    "packages": {"": {"version": tv}},
                }
            ),
            encoding="utf-8",
        )
        npv = native_py_version if native_py_version is not None else ws_version
        (base / "bindings" / "python-native" / "pyproject.toml").write_text(
            f"[project]\nname = \"eggchaos-native\"\nversion = \"{npv}\"\n",
            encoding="utf-8",
        )
        nrv = native_rs_version if native_rs_version is not None else ws_version
        ndr = native_dep_req if native_dep_req is not None else ws_version
        (base / "bindings" / "python-native" / "Cargo.toml").write_text(
            "[workspace]\n[package]\nname = \"eggchaos-native\"\n"
            f"version = \"{nrv}\"\n[dependencies]\n"
            f"eggchaos-embed = {{ path = \"../../crates/eggchaos-embed\", "
            f"version = \"{ndr}\" }}\n",
            encoding="utf-8",
        )
        bv = bench_version if bench_version is not None else ws_version
        bdr = bench_dep_req if bench_dep_req is not None else ws_version
        (base / "benchmarks" / "Cargo.toml").write_text(
            "[workspace]\n[package]\nname = \"eggchaos-benchmarks\"\n"
            f"version = \"{bv}\"\n[dependencies]\n"
            f"eggchaos-core = {{ path = \"../crates/eggchaos-core\", "
            f"version = \"{bdr}\" }}\n",
            encoding="utf-8",
        )

    cases = [
        ("clean tree passes", {}, True),
        ("stale member version fails", {"member_version": "0.1.0"}, False),
        ("stale intra-workspace req fails", {"dep_req": "0.1.0"}, False),
        ("stale python-client fails", {"py_version": "0.1.0"}, False),
        ("stale typescript-client fails", {"ts_version": "0.1.0"}, False),
        ("stale python-native pyproject fails", {"native_py_version": "0.1.0"}, False),
        ("stale python-native crate fails", {"native_rs_version": "0.1.0"}, False),
        ("stale python-native dep req fails", {"native_dep_req": "0.1.0"}, False),
        ("stale benchmarks package fails", {"bench_version": "0.1.0"}, False),
        ("stale benchmarks dep req fails", {"bench_dep_req": "0.1.0"}, False),
    ]
    failed = False
    for label, kwargs, expect_pass in cases:
        with tempfile.TemporaryDirectory() as tmp:
            base = Path(tmp)
            write_tree(base, **kwargs)
            failures, _ws, _n = check_tree(base)
            ok = (not failures) == expect_pass
            print(("ok" if ok else "FAIL") + f": fixture: {label}")
            if not ok:
                failed = True
                for line in failures:
                    print(f"    unexpected: {line}")
    if failed:
        raise SystemExit(1)
    print("version-coherence fixture self-test ok")


def main(argv):
    if "--fixture" in argv:
        run_fixture()
        return 0
    root = DEFAULT_ROOT
    if "--root" in argv:
        idx = argv.index("--root")
        try:
            root = Path(argv[idx + 1]).resolve()
        except IndexError:
            print("FAIL: --root requires a directory", file=sys.stderr)
            return 1
    failures, ws_version, checked_crates = check_tree(root)
    if failures:
        for line in failures:
            print(f"FAIL: {line}", file=sys.stderr)
        print(
            json.dumps(
                {
                    "version_coherence": "fail",
                    "workspace_version": ws_version,
                    "failures": len(failures),
                }
            )
        )
        return 1
    print(
        json.dumps(
            {
                "version_coherence": "pass",
                "workspace_version": ws_version,
                "workspace_crates": checked_crates,
            }
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
