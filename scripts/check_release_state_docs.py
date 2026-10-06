#!/usr/bin/env python3
"""M060 release-state/current-documentation drift guard.

`plans/registry.md` remains the sole hand-maintained milestone-status
authority. This guard does not create a second one: it derives
whether the published release milestone is `closed` from the registry
table and then checks a small, named set of live documents for
concrete claims that contradict that derivation.

The guard is intentionally narrow (per the M060 plan WP5 guard
design). It checks explicit positive/negative fixtures so a stale
phrase fails and historical context does not. It scans only the named
current-state documents, never historical numbered plans, ADRs,
closures, or retained evidence. It is stdlib-only, no network access,
no Git state mutation.

Usage:
    python3 scripts/check_release_state_docs.py [--check]

Exit 0 on success with `{"release_state_docs":"pass",...}`. Exit 1 on
drift with `FAIL:` lines.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path
from typing import Iterable, List, Tuple

REPO_ROOT = Path(__file__).resolve().parent.parent
REGISTRY_PATH = REPO_ROOT / "plans" / "registry.md"

# Milestones whose `closed` claim is asserted by this guard. Other
# closed milestones are still authoritative (per registry), but they
# are not part of the named drift-detector set.
PUBLISHED_RELEASE_MILESTONES = ("M058",)
PRE_PUBLICATION_HARDENING_MILESTONES = ("M059",)
POST_RELEASE_CLEANUP_MILESTONES = ("M060",)


def parse_registry_milestone_status(text: str) -> dict:
    """Return `{milestone: status}` for every registry table row."""
    statuses = {}
    in_table = False
    seen_header = False
    for line in text.splitlines():
        stripped = line.strip()
        if not (stripped.startswith("|") and stripped.endswith("|")):
            continue
        cells = [c.strip() for c in stripped.strip("|").split("|")]
        if not seen_header:
            if cells and cells[0].lower().startswith("milestone"):
                in_table = True
                seen_header = True
            continue
        if not in_table or len(cells) < 5:
            continue
        milestone = cells[0].replace("`", "").replace("**", "").strip()
        if not re.fullmatch(r"M\d{3}", milestone):
            continue
        status = cells[2].replace("`", "").replace("**", "").strip()
        statuses[milestone] = status
    return statuses


# A claim is `(regex, doc_path, friendly label)`. The regex must match
# a substring that contradicts the registered state, OR (for negative
# anchors) must NOT match when the registered state implies that it
# should match. Each claim is encoded as a tuple
# `(kind, milestone, regex, doc_path, label)`:
#
#   - kind "must_be_absent": the substring must not appear in the doc
#     while the milestone is in the claimed state.
#   - kind "must_be_present": the substring must appear in the doc
#     while the milestone is in the claimed state.
#   - kind "forbidden_phrase": the phrase must not appear in the doc
#     regardless of milestone state when it represents a known stale
#     form (e.g. "do not tag", "publication requires M058/M059").
#
# Each claim is paired with a `kind` of condition. The guard fails if
# the condition is violated on the live documents.
CLAIMS = [
    # --- M058 is closed (v0.2.0 is the current published release) ---
    # SECURITY.md must show v0.2.x as the current supported line and
    # must NOT treat v0.2.0 as future publication.
    (
        "must_be_present",
        "M058",
        re.compile(r"\|\s*v0\.2\.x\s*\|\s*Supported"),
        "SECURITY.md",
        "SECURITY.md must list v0.2.x as the supported current line",
    ),
    (
        "must_be_absent",
        "M058",
        re.compile(r"Supported until v0\.2\.0 is published"),
        "SECURITY.md",
        "SECURITY.md must not retain a pre-publication v0.1.x row",
    ),
    (
        "must_be_absent",
        "M058",
        re.compile(r"Supported once published \(pre-release on `main`\)"),
        "SECURITY.md",
        "SECURITY.md must not describe v0.2.x as pre-release",
    ),
    # Release notes for v0.2.0 must say M058 closed / v0.2.0 published.
    (
        "must_be_present",
        "M058",
        re.compile(r"M058 closed (the |on `b6a277d`|the publication)|`v0\.2\.0` is (the |now )?(current |Eggchaos |currentEggchaos )?release"),
        "docs/release-notes-v0.2.0.md",
        "v0.2.0 release notes must record the published status",
    ),
    (
        "must_be_absent",
        "M058",
        re.compile(r"M058 is ready\. Publication requires an"),
        "docs/release-notes-v0.2.0.md",
        "v0.2.0 release notes must not claim publication is still future",
    ),
    # architecture/overview.md: must NOT describe M058 as blocked on M059
    # or M059 as the pre-publication hardening that still gates publication.
    (
        "must_be_absent",
        "M058",
        re.compile(r"M058 publication blocked on M059|M059 ready as the pre-v0\.2\.0"),
        "architecture/overview.md",
        "overview must not claim M058 is blocked on M059",
    ),
    (
        "must_be_present",
        "M058",
        re.compile(r"M058 closed"),
        "architecture/overview.md",
        "overview must record M058 closed",
    ),
    # architecture/tooling-distribution.md: registry narrative must not
    # say "M058 is `ready`".
    (
        "must_be_absent",
        "M058",
        re.compile(r"M058 is `ready`"),
        "architecture/tooling-distribution.md",
        "tooling-distribution must not describe M058 as ready",
    ),
    # --- M059 is closed (pre-publication hardening authority) ---
    (
        "must_be_absent",
        "M059",
        re.compile(r"M059 ready as the pre-v0\.2\.0|M059.+ready.+M058 pre-publication"),
        "architecture/overview.md",
        "overview must not describe M059 as ready",
    ),
    (
        "must_be_present",
        "M059",
        re.compile(r"M059 closed"),
        "architecture/overview.md",
        "overview must record M059 closed",
    ),
    (
        "must_be_absent",
        "M059",
        re.compile(r"M059 ready as the pre-v0\.2\.0|M059.+ready"),
        "architecture/tooling-distribution.md",
        "tooling-distribution must not describe M059 as ready",
    ),
    (
        "must_be_absent",
        "M059",
        re.compile(r"M058 stays blocked until M059 closes|M058 remains blocked until M059 closes"),
        ".github/workflows/release.yml",
        "release.yml must not retain a stale M058-blocked comment",
    ),
    # --- Tooling claim: npm ci is the install path ---
    (
        "must_be_absent",
        "M058",
        re.compile(r"`npm install` if needed"),
        "architecture/tooling-distribution.md",
        "tooling-distribution must describe the lockfile-install path",
    ),
    (
        "must_be_present",
        "M058",
        re.compile(r"npm ci --ignore-scripts --no-audit --no-fund"),
        "architecture/tooling-distribution.md",
        "tooling-distribution must reference the actual install command",
    ),
    # --- Tooling claim: Python-native crate root denies unsafe, the
    #     allowance is module-scoped to the macro-facing bridge.
    (
        "must_be_absent",
        "M058",
        re.compile(r"asserts `allow\(unsafe_code\)` in `lib\.rs`"),
        "architecture/tooling-distribution.md",
        "tooling-distribution must not claim a crate-root allowance",
    ),
    (
        "must_be_present",
        "M058",
        re.compile(r"deny\(unsafe_code\)"),
        "architecture/tooling-distribution.md",
        "tooling-distribution must describe the deny boundary",
    ),
]


def evaluate(statuses: dict, *, root: Path = REPO_ROOT) -> List[Tuple[str, str, str]]:
    """Return a list of `FAIL:` tuples for any guard violation."""
    failures: List[Tuple[str, str, str]] = []

    # Derive the milestone states the guard cares about.
    for ms in PUBLISHED_RELEASE_MILESTONES:
        if statuses.get(ms) != "closed":
            failures.append(
                (
                    "registry-state",
                    "plans/registry.md",
                    f"registry must list {ms} as closed (got {statuses.get(ms)!r}); drift guard requires the published release milestone to be closed",
                )
            )
    for ms in PRE_PUBLICATION_HARDENING_MILESTONES:
        if statuses.get(ms) != "closed":
            failures.append(
                (
                    "registry-state",
                    "plans/registry.md",
                    f"registry must list {ms} as closed (got {statuses.get(ms)!r})",
                )
            )

    # Apply claim conditions.
    for kind, milestone, regex, doc_path, label in CLAIMS:
        ms_status = statuses.get(milestone)
        # The condition only applies when the milestone is in the
        # claimed state. If the registry has not closed that milestone
        # yet, the claim is dormant — its textual assertions are not
        # applicable. (The registry-state failures above still trip.)
        if milestone in PUBLISHED_RELEASE_MILESTONES + PRE_PUBLICATION_HARDENING_MILESTONES:
            if ms_status != "closed":
                continue

        path = root / doc_path
        if not path.exists():
            failures.append(("missing-doc", doc_path, f"required document missing: {doc_path}"))
            continue
        text = path.read_text(encoding="utf-8")
        if kind == "must_be_absent" and regex.search(text):
            failures.append(("drift", doc_path, f"{label}: stale phrase still present: {regex.pattern!r}"))
        elif kind == "must_be_present" and not regex.search(text):
            failures.append(("drift", doc_path, f"{label}: required phrase missing: {regex.pattern!r}"))

    return failures


def run_fixture() -> bool:
    """Inline self-test: pass on a synthetic tree, fail on every drift class."""
    import json
    import tempfile

    def write_doc(rel: str, body: str, base: Path) -> None:
        target = base / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(body, encoding="utf-8")

    ok_total = True

    # Case 1: clean registry + clean docs passes.
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        (base / "plans").mkdir()
        (base / "docs").mkdir()
        (base / "architecture").mkdir()
        (base / ".github" / "workflows").mkdir(parents=True, exist_ok=True)
        registry = (
            "| Milestone | Plan | Status | Depends on | Note |\n"
            "| --- | --- | --- | --- | --- |\n"
            "| M058 | `058-publication.md` | closed | — | done |\n"
            "| M059 | `059-hardening.md` | closed | — | done |\n"
            "| M060 | `060-cleanup.md` | ready | — | next |\n"
        )
        write_doc("plans/registry.md", registry, base)
        write_doc(
            "SECURITY.md",
            "| Line    | Status                                              |\n"
            "| ------- | --------------------------------------------------- |\n"
            "| v0.2.x  | Supported (current published release line; `v0.2.0` published 2026-10-02) |\n",
            base,
        )
        write_doc(
            "docs/release-notes-v0.2.0.md",
            "Status: **published**. `v0.2.0` is the current Eggchaos release; M058 closed the v0.2.0 publication on `b6a277d`.\n",
            base,
        )
        write_doc(
            "architecture/overview.md",
            "M059 closed on `1409d0f` (hosted CI 15/15). M058 closed the v0.2.0 publication on `b6a277d`.\n",
            base,
        )
        write_doc(
            "architecture/tooling-distribution.md",
            "Unconditional lockfile install: npm ci --ignore-scripts --no-audit --no-fund. "
            "Python-native crate root deny(unsafe_code) plus item-scoped allow only on the macro-facing bridge.\n",
            base,
        )
        write_doc(
            ".github/workflows/release.yml",
            "# comment only\npermissions:\n  contents: read\n",
            base,
        )

        statuses = parse_registry_milestone_status(registry)
        fails = evaluate(statuses, root=base)
        if fails:
            ok_total = False
            print("FAIL: clean fixture must pass, got:")
            for f in fails:
                print(f"    {f}")

    # Case 2: drift on SECURITY.md (pre-publication row still says "until v0.2.0 is published").
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        (base / "plans").mkdir()
        (base / "docs").mkdir()
        (base / "architecture").mkdir()
        (base / ".github" / "workflows").mkdir(parents=True, exist_ok=True)
        registry = (
            "| Milestone | Plan | Status | Depends on | Note |\n"
            "| --- | --- | --- | --- | --- |\n"
            "| M058 | `058-publication.md` | closed | — | done |\n"
            "| M059 | `059-hardening.md` | closed | — | done |\n"
        )
        write_doc("plans/registry.md", registry, base)
        write_doc(
            "SECURITY.md",
            "| v0.1.x  | Supported until v0.2.0 is published |\n",
            base,
        )
        write_doc("docs/release-notes-v0.2.0.md", "", base)
        write_doc("architecture/overview.md", "", base)
        write_doc("architecture/tooling-distribution.md", "", base)
        write_doc(".github/workflows/release.yml", "", base)
        statuses = parse_registry_milestone_status(registry)
        fails = evaluate(statuses, root=base)
        if not any(f[0] == "drift" and "SECURITY.md" in f[1] for f in fails):
            ok_total = False
            print("FAIL: SECURITY.md pre-publication drift must fail")
            for f in fails:
                print(f"    {f}")

    # Case 3: drift on release notes (M058 still described as ready).
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        (base / "plans").mkdir()
        (base / "docs").mkdir()
        (base / "architecture").mkdir()
        (base / ".github" / "workflows").mkdir(parents=True, exist_ok=True)
        registry = (
            "| Milestone | Plan | Status | Depends on | Note |\n"
            "| --- | --- | --- | --- | --- |\n"
            "| M058 | `058-publication.md` | closed | — | done |\n"
            "| M059 | `059-hardening.md` | closed | — | done |\n"
        )
        write_doc("plans/registry.md", registry, base)
        write_doc(
            "SECURITY.md",
            "| v0.2.x  | Supported (current published release line) |\n",
            base,
        )
        write_doc(
            "docs/release-notes-v0.2.0.md",
            "M058 is ready. Publication requires an annotated `v0.2.0` tag.\n",
            base,
        )
        write_doc("architecture/overview.md", "", base)
        write_doc("architecture/tooling-distribution.md", "", base)
        write_doc(".github/workflows/release.yml", "", base)
        statuses = parse_registry_milestone_status(registry)
        fails = evaluate(statuses, root=base)
        if not any(f[0] == "drift" and "release-notes-v0.2.0.md" in f[1] for f in fails):
            ok_total = False
            print("FAIL: release-notes pre-publication drift must fail")
            for f in fails:
                print(f"    {f}")

    # Case 4: drift on tooling-distribution (`npm install` / crate-root `allow`).
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        (base / "plans").mkdir()
        (base / "docs").mkdir()
        (base / "architecture").mkdir()
        (base / ".github" / "workflows").mkdir(parents=True, exist_ok=True)
        registry = (
            "| Milestone | Plan | Status | Depends on | Note |\n"
            "| --- | --- | --- | --- | --- |\n"
            "| M058 | `058-publication.md` | closed | — | done |\n"
            "| M059 | `059-hardening.md` | closed | — | done |\n"
        )
        write_doc("plans/registry.md", registry, base)
        write_doc(
            "SECURITY.md",
            "| v0.2.x  | Supported (current published release line) |\n",
            base,
        )
        write_doc("docs/release-notes-v0.2.0.md", "M058 closed the v0.2.0 publication on `b6a277d`.\n", base)
        write_doc("architecture/overview.md", "M059 closed on `1409d0f`. M058 closed on `b6a277d`.\n", base)
        write_doc(
            "architecture/tooling-distribution.md",
            "`npm install` if needed. The crate root asserts `allow(unsafe_code)` in `lib.rs`.\n",
            base,
        )
        write_doc(".github/workflows/release.yml", "", base)
        statuses = parse_registry_milestone_status(registry)
        fails = evaluate(statuses, root=base)
        if not any(f[0] == "drift" and "tooling-distribution.md" in f[1] for f in fails):
            ok_total = False
            print("FAIL: tooling-distribution drift must fail")
            for f in fails:
                print(f"    {f}")

    # Case 5: drift on overview (M059 ready / M058 blocked).
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        (base / "plans").mkdir()
        (base / "docs").mkdir()
        (base / "architecture").mkdir()
        (base / ".github" / "workflows").mkdir(parents=True, exist_ok=True)
        registry = (
            "| Milestone | Plan | Status | Depends on | Note |\n"
            "| --- | --- | --- | --- | --- |\n"
            "| M058 | `058-publication.md` | closed | — | done |\n"
            "| M059 | `059-hardening.md` | closed | — | done |\n"
        )
        write_doc("plans/registry.md", registry, base)
        write_doc(
            "SECURITY.md",
            "| v0.2.x  | Supported (current published release line) |\n",
            base,
        )
        write_doc("docs/release-notes-v0.2.0.md", "M058 closed the v0.2.0 publication on `b6a277d`.\n", base)
        write_doc(
            "architecture/overview.md",
            "M059 ready as the pre-v0.2.0 hardening corrective; M058 publication blocked on M059.\n",
            base,
        )
        write_doc("architecture/tooling-distribution.md", "", base)
        write_doc(".github/workflows/release.yml", "", base)
        statuses = parse_registry_milestone_status(registry)
        fails = evaluate(statuses, root=base)
        if not any(f[0] == "drift" and "overview.md" in f[1] for f in fails):
            ok_total = False
            print("FAIL: overview drift must fail")
            for f in fails:
                print(f"    {f}")

    # Case 6: drift on release.yml (M058-blocked comment).
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        (base / "plans").mkdir()
        (base / "docs").mkdir()
        (base / "architecture").mkdir()
        (base / ".github" / "workflows").mkdir(parents=True, exist_ok=True)
        registry = (
            "| Milestone | Plan | Status | Depends on | Note |\n"
            "| --- | --- | --- | --- | --- |\n"
            "| M058 | `058-publication.md` | closed | — | done |\n"
            "| M059 | `059-hardening.md` | closed | — | done |\n"
        )
        write_doc("plans/registry.md", registry, base)
        write_doc(
            "SECURITY.md",
            "| v0.2.x  | Supported (current published release line) |\n",
            base,
        )
        write_doc("docs/release-notes-v0.2.0.md", "M058 closed the v0.2.0 publication on `b6a277d`.\n", base)
        write_doc("architecture/overview.md", "M059 closed on `1409d0f`. M058 closed on `b6a277d`.\n", base)
        write_doc(
            "architecture/tooling-distribution.md",
            "npm ci --ignore-scripts --no-audit --no-fund. Python-native deny(unsafe_code) plus item-scoped allow.\n",
            base,
        )
        write_doc(
            ".github/workflows/release.yml",
            "name: release\n# M058 stays blocked until M059 closes\npermissions: contents: read\n",
            base,
        )
        statuses = parse_registry_milestone_status(registry)
        fails = evaluate(statuses, root=base)
        if not any(f[0] == "drift" and "release.yml" in f[1] for f in fails):
            ok_total = False
            print("FAIL: release.yml drift must fail")
            for f in fails:
                print(f"    {f}")

    # Case 7: registry not closed -> registry-state failure.
    with tempfile.TemporaryDirectory() as tmp:
        base = Path(tmp)
        (base / "plans").mkdir()
        registry = (
            "| Milestone | Plan | Status | Depends on | Note |\n"
            "| --- | --- | --- | --- | --- |\n"
            "| M058 | `058-publication.md` | ready | — | next |\n"
            "| M059 | `059-hardening.md` | closed | — | done |\n"
        )
        write_doc("plans/registry.md", registry, base)
        statuses = parse_registry_milestone_status(registry)
        fails = evaluate(statuses, root=base)
        if not any(f[0] == "registry-state" for f in fails):
            ok_total = False
            print("FAIL: unclosed M058 must produce registry-state failure")
            for f in fails:
                print(f"    {f}")

    if ok_total:
        print("OK: release-state-docs fixture self-test passed")
    return ok_total


def main(argv: List[str]) -> int:
    parser = argparse.ArgumentParser(
        description="M060 release-state/current-doc drift guard."
    )
    group = parser.add_mutually_exclusive_group()
    group.add_argument(
        "--check",
        action="store_true",
        help="Verify all live documents match the claimed milestone states.",
    )
    group.add_argument(
        "--fixture",
        action="store_true",
        help="Run the inline fixture self-test.",
    )
    args = parser.parse_args(argv)

    if args.fixture:
        return 0 if run_fixture() else 1

    text = REGISTRY_PATH.read_text(encoding="utf-8")
    statuses = parse_registry_milestone_status(text)
    failures = evaluate(statuses)
    if failures:
        for kind, doc_path, detail in failures:
            print(f"FAIL: {kind} {doc_path}: {detail}", file=sys.stderr)
        print(
            f"{{\"release_state_docs\":\"fail\",\"failures\":{len(failures)}}}",
            file=sys.stderr,
        )
        return 1
    print("{\"release_state_docs\":\"pass\"}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))