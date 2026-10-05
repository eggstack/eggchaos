#!/usr/bin/env python3
"""Planning-state drift guard (M053).

This is the sole authority for the compact, mechanically-checked
"current planning state" block in
AGENTS.md / plans/README.md / plans/roadmap.md / architecture/overview.md.

`plans/registry.md` remains the only hand-maintained source of truth
for milestone state. This script:

- parses the registry table with stdlib only;
- validates that every numbered active plan has exactly one
  registry row, that milestone numbers/plan filenames are unique
  and ordered, that statuses come from the documented vocabulary,
  and that dependencies reference known milestones or explicit
  external/historical prerequisites;
- validates (M061) that every registered numbered plan's own
  top-level `Status:` header agrees with its registry row, so the
  two cannot silently diverge again;
- generates a deterministic current-state block
  (`<!-- BEGIN eggchaos:planning-state -->` ... `<!-- END -->`) for
  the four documents that have one;
- checks (`--check`) that every block matches the current registry
  snapshot, exiting 1 on drift;
- writes (`--write`) the block back into the document.

The script is stdlib-only on purpose. CI is expected to call
`--check` from `scripts/check.sh` and from one existing CI job.
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Dict, List, Optional, Tuple

# --- Configuration --------------------------------------------------------

REPO_ROOT = Path(__file__).resolve().parent.parent

REGISTRY_PATH = REPO_ROOT / "plans" / "registry.md"
PLANS_DIR = REPO_ROOT / "plans"

# Documents that carry a generated current-state block.
TARGETS: List[Tuple[Path, str]] = [
    (
        REPO_ROOT / "AGENTS.md",
        "AGENTS",
    ),
    (
        REPO_ROOT / "plans" / "README.md",
        "README",
    ),
    (
        REPO_ROOT / "plans" / "roadmap.md",
        "ROADMAP",
    ),
    (
        REPO_ROOT / "architecture" / "overview.md",
        "OVERVIEW",
    ),
]

# Documented status vocabulary. `closed` and `blocked` are the historical
# anchors. `ready` marks a milestone whose predecessor closed and that
# has no further successor in flight. `active` is reserved for
# implementation work in flight. The vocabulary is intentionally
# narrow so drift is obvious.
VALID_STATUSES = {"closed", "ready", "blocked", "active"}

# Top-level plan metadata key carrying the milestone status. Only a
# line that starts at column 0 inside the plan header block counts;
# see `plan_header_lines` for why the search is bounded.
PLAN_STATUS_PREFIX = "Status:"

# Begin / end markers delimiting the generated block in each target.
BEGIN_MARKER = "<!-- BEGIN eggchaos:planning-state -->"
END_MARKER = "<!-- END eggchaos:planning-state -->"

# --- Parsing --------------------------------------------------------------


@dataclass(frozen=True)
class Row:
    milestone: str
    plan: str
    status: str
    depends_on: Tuple[str, ...]
    note: str


def _strip_inline_code(text: str) -> str:
    # Strip both inline-code backticks and inline-emphasis asterisks so
    # the table cells can carry either `closed` or **closed** without
    # breaking the parser.
    return text.replace("`", "").replace("**", "").strip()


def _parse_dependencies(raw: str) -> Tuple[str, ...]:
    if raw.strip() in {"", "—", "-"}:
        return ()
    parts = [p.strip() for p in raw.split(",")]
    return tuple(p for p in parts if p)


def parse_registry(text: str) -> List[Row]:
    """Parse the registry markdown table.

    The table is a five-column pipe table; the header row begins
    `| Milestone | Plan | Status | Depends on | Activation / closure note |`
    and rows follow. Anything between the table rows is treated as
    prose and ignored.
    """
    rows: List[Row] = []
    in_table = False
    seen_header = False
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("|") and stripped.endswith("|"):
            cells = [c.strip() for c in stripped.strip("|").split("|")]
            if not seen_header:
                if cells and cells[0].lower().startswith("milestone"):
                    in_table = True
                    seen_header = True
                continue
            if not in_table:
                continue
            if len(cells) < 5:
                continue
            milestone = _strip_inline_code(cells[0])
            plan = _strip_inline_code(cells[1])
            status = _strip_inline_code(cells[2])
            deps = _parse_dependencies(_strip_inline_code(cells[3]))
            note = cells[4]
            if not re.fullmatch(r"M\d{3}", milestone):
                continue
            if not plan.endswith(".md"):
                continue
            rows.append(
                Row(
                    milestone=milestone,
                    plan=plan,
                    status=status,
                    depends_on=deps,
                    note=note,
                )
            )
    return rows


# --- Plan-header status metadata (M061) -----------------------------------


def plan_header_lines(text: str) -> List[str]:
    """Return a plan file's top-level header block.

    The header block is everything before the first level-2 markdown
    heading. Plan metadata (`Status:`, `Depends on:`, `Role:`, ...)
    lives there and body prose does not. Bounding the search this way
    keeps a body sentence that happens to begin with `Status:` from
    being mistaken for plan metadata.
    """
    lines: List[str] = []
    for line in text.splitlines():
        if line.startswith("## "):
            break
        lines.append(line)
    return lines


def parse_plan_status(text: str) -> Tuple[Optional[str], Optional[str], str]:
    """Return `(status token, problem kind, problem detail)` for a plan.

    The leading token carries the status; anything after it is an
    explanatory suffix and is ignored, so all of these parse as
    `closed`:

        Status: closed
        Status: closed (candidate `ca46801`; evidence in `...`)
        Status: **closed**

    Missing, duplicated, empty, and out-of-vocabulary declarations are
    reported as problems rather than silently skipped, so a plan can
    never opt out of the agreement check by removing or mangling its
    own metadata.

    Authority rule: `plans/registry.md` is canonical. This function
    only *reads* plan metadata to prove it follows the registry; the
    registry status is never inferred from a plan file.
    """
    declarations = [
        line.strip()
        for line in plan_header_lines(text)
        if line.strip().startswith(PLAN_STATUS_PREFIX)
    ]
    if not declarations:
        return (
            None,
            "missing",
            f"no top-level `{PLAN_STATUS_PREFIX}` declaration in the plan header",
        )
    if len(declarations) > 1:
        return (
            None,
            "duplicate",
            f"{len(declarations)} top-level `{PLAN_STATUS_PREFIX}` declarations "
            f"({'; '.join(declarations)})",
        )
    rest = _strip_inline_code(declarations[0][len(PLAN_STATUS_PREFIX) :])
    words = rest.split()
    token = words[0] if words else ""
    if token not in VALID_STATUSES:
        return (
            None,
            "unparseable",
            f"{declarations[0]!r} does not lead with a status token "
            f"from {sorted(VALID_STATUSES)}",
        )
    return token, None, ""


# --- Validation -----------------------------------------------------------


class RegistryError:
    def __init__(self, kind: str, detail: str) -> None:
        self.kind = kind
        self.detail = detail

    def __str__(self) -> str:
        return f"{self.kind}: {self.detail}"


def validate(rows: List[Row], plans_dir: Optional[Path] = None) -> List[RegistryError]:
    errors: List[RegistryError] = []
    seen_milestones: Dict[str, Row] = {}
    seen_plans: Dict[str, Row] = {}
    plans_dir = plans_dir or PLANS_DIR

    for row in rows:
        if row.milestone in seen_milestones:
            errors.append(
                RegistryError(
                    "duplicate-milestone",
                    f"{row.milestone} appears more than once",
                )
            )
        else:
            seen_milestones[row.milestone] = row
        if row.plan in seen_plans:
            errors.append(
                RegistryError(
                    "duplicate-plan",
                    f"{row.plan} is registered for more than one milestone",
                )
            )
        else:
            seen_plans[row.plan] = row
        if row.status not in VALID_STATUSES:
            errors.append(
                RegistryError(
                    "invalid-status",
                    f"{row.milestone} status {row.status!r} not in {sorted(VALID_STATUSES)}",
                )
            )
        plan_path = plans_dir / row.plan
        if not plan_path.exists():
            errors.append(
                RegistryError(
                    "missing-plan",
                    f"{row.milestone} references missing file {row.plan}",
                )
            )
            continue

        # Registry ↔ plan-header status agreement (M061). The registry
        # stays canonical; the plan header is checked metadata.
        token, kind, detail = parse_plan_status(
            plan_path.read_text(encoding="utf-8")
        )
        if kind == "missing":
            errors.append(
                RegistryError(
                    "plan-status-missing",
                    f"{row.milestone} plan {row.plan} {detail}",
                )
            )
        elif kind == "duplicate":
            errors.append(
                RegistryError(
                    "plan-status-duplicate",
                    f"{row.milestone} plan {row.plan} has {detail}",
                )
            )
        elif kind == "unparseable":
            errors.append(
                RegistryError(
                    "plan-status-unparseable",
                    f"{row.milestone} plan {row.plan} {detail}",
                )
            )
        elif token != row.status:
            errors.append(
                RegistryError(
                    "plan-status-mismatch",
                    f"{row.milestone} registry status {row.status!r} disagrees with "
                    f"plan header {token!r} in {row.plan}",
                )
            )

    # Strict numeric ordering for milestones in the table.
    last_num = -1
    for row in rows:
        try:
            num = int(row.milestone[1:])
        except ValueError:
            continue
        if num <= last_num:
            errors.append(
                RegistryError(
                    "out-of-order",
                    f"{row.milestone} is not strictly greater than the previous milestone",
                )
            )
        last_num = num

    # Dependency resolution: every M### token mentioned in a Depends-on
    # cell must reference a registered milestone. Cells may also carry
    # explicit external/historical prerequisites alongside (e.g.
    # "M019 + ADR 003", "M002 historical implementation",
    # "M049-M053"); those non-milestone words are allowed, but every
    # embedded M### token must resolve.
    known_milestones = set(seen_milestones.keys())
    for row in rows:
        for dep in row.depends_on:
            tokens = re.findall(r"M\d{3}", dep)
            if not tokens:
                continue
            for token in tokens:
                if token not in known_milestones:
                    errors.append(
                        RegistryError(
                            "missing-dependency",
                            f"{row.milestone} depends on unknown {token} (in {dep!r})",
                        )
                    )

    # Reverse check: every numbered plan file must have exactly one
    # registry row. ("Exactly one" is enforced jointly with the
    # duplicate-plan check above.) Only files matching NNN-*.md count;
    # README.md / roadmap.md / registry.md / adrs / closure / archive /
    # reference are not milestone plans.
    try:
        numbered = sorted(
            p.name
            for p in plans_dir.iterdir()
            if p.is_file() and re.fullmatch(r"\d{3}-.*\.md", p.name)
        )
    except FileNotFoundError:
        errors.append(
            RegistryError("missing-plans-dir", f"plans directory not found: {plans_dir}")
        )
        return errors
    for name in numbered:
        if name not in seen_plans:
            errors.append(
                RegistryError(
                    "unregistered-plan",
                    f"plan file {name} has no registry row",
                )
            )
    return errors


# --- Generation -----------------------------------------------------------


def _format_block(rows: List[Row]) -> str:
    """Render the current-state block deterministically."""
    closed = [r for r in rows if r.status == "closed"]
    ready = [r for r in rows if r.status == "ready"]
    active = [r for r in rows if r.status == "active"]
    blocked = [r for r in rows if r.status == "blocked"]

    lines: List[str] = []
    lines.append(BEGIN_MARKER)
    lines.append("<!--")
    lines.append("  Generated by scripts/check_planning_state.py --write.")
    lines.append("  Do not hand-edit between the markers; this script rewrites")
    lines.append("  the block from plans/registry.md, the only hand-maintained")
    lines.append("  source of truth for milestone state.")
    lines.append("-->")
    lines.append("")
    lines.append("## Current planning state")
    lines.append("")
    if active:
        lines.append("**Active (in flight):**")
        for r in active:
            lines.append(f"- `{r.milestone}` ({r.status}; plan `{r.plan}`)")
        lines.append("")
    if ready:
        lines.append("**Ready (next milestone):**")
        for r in ready:
            lines.append(f"- `{r.milestone}` ({r.status}; plan `{r.plan}`)")
        lines.append("")
    if blocked:
        lines.append("**Blocked (waiting on a predecessor closure):**")
        chain: List[str] = []
        for r in blocked:
            chain.append(r.milestone)
        lines.append("- " + " -> ".join(chain))
        lines.append("")
    if closed:
        highest = max(closed, key=lambda r: int(r.milestone[1:]))
        lines.append(
            f"**Highest closed milestone:** `{highest.milestone}` "
            f"(see registry for closure evidence)."
        )
        lines.append("")
    if active or ready:
        ready_for_registration = active + ready
        order = ", ".join(f"`{r.milestone}`" for r in ready_for_registration)
        lines.append(f"**Execution order:** {order}")
    else:
        lines.append("**Execution order:** no active or ready milestones.")
    lines.append("")
    lines.append(END_MARKER)
    return "\n".join(lines) + "\n"


def generate_block(rows: List[Row]) -> str:
    return _format_block(rows)


# --- Check / write --------------------------------------------------------


def _extract_existing_block(text: str) -> Optional[Tuple[int, int, str]]:
    begin = text.find(BEGIN_MARKER)
    if begin < 0:
        return None
    end = text.find(END_MARKER, begin)
    if end < 0:
        return None
    end += len(END_MARKER)
    # include trailing newline if present
    while end < len(text) and text[end] == "\n":
        end += 1
    return begin, end, text[begin:end]


def check_target(path: Path, expected: str) -> bool:
    if not path.exists():
        print(f"FAIL: {path} does not exist", file=sys.stderr)
        return False
    text = path.read_text(encoding="utf-8")
    existing = _extract_existing_block(text)
    if existing is None:
        print(
            f"FAIL: {path} has no {BEGIN_MARKER} ... {END_MARKER} block",
            file=sys.stderr,
        )
        return False
    begin, end, content = existing
    if content.strip() != expected.strip():
        print(
            f"FAIL: {path} planning-state block is stale (drift detected)",
            file=sys.stderr,
        )
        # Show a one-line hint for the first divergent byte to keep
        # the message cheap.
        for i, (a, b) in enumerate(zip(content, expected)):
            if a != b:
                print(
                    f"  first divergence at offset {i}: {a!r} vs {b!r}",
                    file=sys.stderr,
                )
                break
        else:
            print(
                f"  trailing content differs (lengths {len(content)} vs {len(expected)})",
                file=sys.stderr,
            )
        return False
    return True


def write_target(path: Path, expected: str) -> bool:
    if not path.exists():
        print(f"FAIL: {path} does not exist", file=sys.stderr)
        return False
    text = path.read_text(encoding="utf-8")
    existing = _extract_existing_block(text)
    if existing is None:
        # Append the block at end of file with a blank-line separator.
        if not text.endswith("\n"):
            text += "\n"
        text += "\n" + expected
    else:
        begin, end, _ = existing
        text = text[:begin] + expected + text[end:]
    path.write_text(text, encoding="utf-8")
    print(f"OK: wrote planning-state block into {path}")
    return True


# --- Fixture coverage -----------------------------------------------------

# A small synthetic registry used by the fixture test. Kept inline so
# the test does not need a fixture file on disk. The fixture checks
# the validator's behavior in isolation.


def _fixture_table() -> str:
    return (
        "| Milestone | Plan | Status | Depends on | Activation / closure note |\n"
        "| --- | --- | --- | --- | --- |\n"
        "| M000 | `000-foo.md` | closed | — | seed |\n"
        "| M001 | `001-bar.md` | closed | M000 | seeded |\n"
        "| M002 | `002-baz.md` | ready | M001 | next |\n"
    )


def _fixture_plan(header: str) -> str:
    """Render a synthetic plan whose header block carries `header`."""
    return f"# fixture plan\n\n{header}\n\n## Objective\n\nfixture body.\n"


def _run_fixture() -> bool:
    import tempfile

    def write_plan(plans: Path, name: str, header: str) -> None:
        (plans / name).write_text(_fixture_plan(header), encoding="utf-8")

    with tempfile.TemporaryDirectory() as tmpdir:
        plans = Path(tmpdir)
        write_plan(plans, "000-foo.md", "Status: closed")
        write_plan(plans, "001-bar.md", "Status: closed")
        write_plan(plans, "002-baz.md", "Status: ready")
        rows = parse_registry(_fixture_table())
        if len(rows) != 3:
            print(
                f"FAIL: fixture expected 3 rows, got {len(rows)}",
                file=sys.stderr,
            )
            return False
        errors = validate(rows, plans_dir=plans)
        if errors:
            print(
                f"FAIL: fixture expected no validation errors, got {errors}",
                file=sys.stderr,
            )
            return False
        # Tamper: invalid status.
        tampered = _fixture_table().replace("| ready ", "| unknown ")
        bad = validate(parse_registry(tampered), plans_dir=plans)
        if not any(e.kind == "invalid-status" for e in bad):
            print(
                "FAIL: fixture expected an invalid-status error after tampering",
                file=sys.stderr,
            )
            return False
        # Tamper: duplicate plan.
        dup = _fixture_table().replace("001-bar.md", "000-foo.md")
        bad = validate(parse_registry(dup), plans_dir=plans)
        if not any(e.kind == "duplicate-plan" for e in bad):
            print(
                "FAIL: fixture expected a duplicate-plan error after tampering",
                file=sys.stderr,
            )
            return False
        # Tamper: out-of-order.
        ooo = re.sub(
            r"\| M000 \| `000-foo.md` \| closed \| — \| seed \|",
            "| M000 | `000-foo.md` | closed | — | seed |\n"
            "| M002 | `002-baz.md` | closed | M000 | reordered |\n"
            "| M001 | `001-bar.md` | closed | M000 | reordered |\n",
            _fixture_table(),
        )
        bad = validate(parse_registry(ooo), plans_dir=plans)
        if not any(e.kind == "out-of-order" for e in bad):
            print(
                "FAIL: fixture expected an out-of-order error after tampering",
                file=sys.stderr,
            )
            return False
        # Tamper: unknown milestone dependency.
        dep = _fixture_table().replace("| M001 | next |", "| M099 | next |")
        bad = validate(parse_registry(dep), plans_dir=plans)
        if not any(e.kind == "missing-dependency" for e in bad):
            print(
                "FAIL: fixture expected a missing-dependency error after tampering",
                file=sys.stderr,
            )
            return False
        # Tamper: numbered plan file with no registry row.
        write_plan(plans, "003-qux.md", "Status: closed")
        bad = validate(parse_registry(_fixture_table()), plans_dir=plans)
        if not any(
            e.kind == "unregistered-plan" and "003-qux.md" in e.detail for e in bad
        ):
            print(
                "FAIL: fixture expected an unregistered-plan error for 003-qux.md",
                file=sys.stderr,
            )
            return False
        (plans / "003-qux.md").unlink()

        # --- Registry <-> plan-header status agreement (M061) ---
        #
        # The registry is canonical; the plan header is checked
        # metadata. Each case below drives one plan file's own
        # `Status:` line and asserts the named failure kind.

        def status_errors(name: str, header: str) -> List[RegistryError]:
            write_plan(plans, name, header)
            return validate(parse_registry(_fixture_table()), plans_dir=plans)

        # (a) Matching statuses are accepted, including the
        #     explanatory-suffix syntax already used by real plans
        #     (M024/M025/M058 carry a parenthesised suffix).
        suffix_errors = status_errors(
            "001-bar.md",
            "Status: closed (candidate `deadbee`; evidence in "
            "`plans/closure/M001-closure.md`)",
        )
        if suffix_errors:
            print(
                "FAIL: fixture expected an explanatory status suffix to be accepted, "
                f"got {suffix_errors}",
                file=sys.stderr,
            )
            return False
        emphasised = status_errors("002-baz.md", "Status: **ready**")
        if emphasised:
            print(
                "FAIL: fixture expected an emphasised status token to be accepted, "
                f"got {emphasised}",
                file=sys.stderr,
            )
            return False

        # (b) Registry `closed` + plan `ready` fails.
        bad = status_errors("001-bar.md", "Status: ready")
        if not any(e.kind == "plan-status-mismatch" and "M001" in e.detail for e in bad):
            print(
                "FAIL: fixture expected a plan-status-mismatch for M001 "
                "(registry closed / plan ready)",
                file=sys.stderr,
            )
            return False

        # (c) Registry `ready` + plan `closed` fails.
        bad = status_errors("002-baz.md", "Status: closed")
        if not any(e.kind == "plan-status-mismatch" and "M002" in e.detail for e in bad):
            print(
                "FAIL: fixture expected a plan-status-mismatch for M002 "
                "(registry ready / plan closed)",
                file=sys.stderr,
            )
            return False

        # (d) A missing `Status:` fails closed rather than being skipped.
        bad = status_errors("002-baz.md", "Depends on: M001")
        if not any(e.kind == "plan-status-missing" and "002-baz.md" in e.detail for e in bad):
            print(
                "FAIL: fixture expected a plan-status-missing for 002-baz.md",
                file=sys.stderr,
            )
            return False

        # (e) Duplicate top-level `Status:` declarations fail.
        bad = status_errors(
            "002-baz.md", "Status: ready\nStatus: closed\nRole: fixture"
        )
        if not any(e.kind == "plan-status-duplicate" and "002-baz.md" in e.detail for e in bad):
            print(
                "FAIL: fixture expected a plan-status-duplicate for 002-baz.md",
                file=sys.stderr,
            )
            return False

        # (f) An out-of-vocabulary status token fails as unparseable.
        bad = status_errors("002-baz.md", "Status: ~~frozen~~")
        if not any(
            e.kind == "plan-status-unparseable" and "002-baz.md" in e.detail for e in bad
        ):
            print(
                "FAIL: fixture expected a plan-status-unparseable for 002-baz.md",
                file=sys.stderr,
            )
            return False

        # (g) A `Status:` line in the plan body is not plan metadata, so
        #     a body sentence can neither satisfy nor duplicate the
        #     header declaration.
        (plans / "002-baz.md").write_text(
            "# fixture plan\n\nDepends on: M001\n\n## Objective\n\n"
            "Status: closed is what a stale body note might say.\n",
            encoding="utf-8",
        )
        bad = validate(parse_registry(_fixture_table()), plans_dir=plans)
        if not any(e.kind == "plan-status-missing" for e in bad):
            print(
                "FAIL: fixture expected a body-only `Status:` line to read as missing",
                file=sys.stderr,
            )
            return False

        # Restore the clean fixture state for the determinism/block checks.
        write_plan(plans, "000-foo.md", "Status: closed")
        write_plan(plans, "001-bar.md", "Status: closed")
        write_plan(plans, "002-baz.md", "Status: ready")
        # Determinism: the generated block must be byte-identical
        # across repeated renders of the same rows.
        block_a = generate_block(parse_registry(_fixture_table()))
        block_b = generate_block(parse_registry(_fixture_table()))
        if block_a != block_b:
            print("FAIL: generated block is not deterministic", file=sys.stderr)
            return False
        # Stale-block detection on temp copies only (never the real repo).
        # Negative cases print FAIL lines by design; silence them here so
        # --fixture output stays a single OK on success.
        import contextlib
        import io

        doc = plans / "doc.md"
        doc.write_text("# title\n\n" + block_a, encoding="utf-8")
        if not check_target(doc, block_a):
            print("FAIL: fresh block should verify", file=sys.stderr)
            return False
        stale = block_a.replace("M002", "M099")
        doc.write_text("# title\n\n" + stale, encoding="utf-8")
        with contextlib.redirect_stderr(io.StringIO()):
            stale_ok = check_target(doc, block_a)
        if stale_ok:
            print("FAIL: stale block should not verify", file=sys.stderr)
            return False
        doc.write_text("# title with no block\n", encoding="utf-8")
        with contextlib.redirect_stderr(io.StringIO()):
            missing_ok = check_target(doc, block_a)
        if missing_ok:
            print("FAIL: missing block should not verify", file=sys.stderr)
            return False
        print("OK: fixture tests pass")
        return True


# --- Entry point ----------------------------------------------------------


def main(argv: List[str]) -> int:
    parser = argparse.ArgumentParser(
        description="Planning-state drift guard (M053)."
    )
    group = parser.add_mutually_exclusive_group()
    group.add_argument(
        "--check",
        action="store_true",
        help="Verify all current-state blocks match the registry snapshot.",
    )
    group.add_argument(
        "--write",
        action="store_true",
        help="Rewrite the current-state block in every target document.",
    )
    group.add_argument(
        "--fixture",
        action="store_true",
        help="Run the inline fixture self-test.",
    )
    args = parser.parse_args(argv)

    if args.fixture:
        return 0 if _run_fixture() else 1

    text = REGISTRY_PATH.read_text(encoding="utf-8")
    rows = parse_registry(text)
    errors = validate(rows)
    if errors:
        for e in errors:
            print(f"FAIL: {e}", file=sys.stderr)
        return 1

    block = generate_block(rows)

    if args.write:
        ok = True
        for path, _ in TARGETS:
            if not write_target(path, block):
                ok = False
        return 0 if ok else 1

    # default + --check
    ok = True
    for path, _ in TARGETS:
        if not check_target(path, block):
            ok = False
    if not ok:
        return 1
    print(
        f"OK: {len(rows)} milestones, {len(TARGETS)} target documents match"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
