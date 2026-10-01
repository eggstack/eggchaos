#!/usr/bin/env python3
"""M059 structural guard: workspace lint inheritance.

Every ordinary workspace member must opt into the root lint policy
with `[lints] workspace = true` (Cargo workspace lints are opt-in per
package; assuming implicit inheritance is a documented common error).
Enumerates members from the root `[workspace] members` table so a
future ordinary member that omits the stanza fails this check.

Stdlib only. Usage:
  python3 scripts/check_lint_inheritance.py --check    # real tree
  python3 scripts/check_lint_inheritance.py --fixture  # inline self-test
"""

import re
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
ROOT_MANIFEST = REPO_ROOT / "Cargo.toml"


def workspace_members(root_text: str) -> list:
    """Extract the root [workspace] members list (quoted paths)."""
    m = re.search(r"\[workspace\]\s*\n(?:[^\[]*?)members\s*=\s*\[(.*?)\]", root_text, re.S)
    if not m:
        raise ValueError("root manifest has no [workspace] members table")
    return re.findall(r'"([^"]+)"', m.group(1))


def manifest_inherits_lints(manifest_text: str) -> bool:
    """True when the manifest contains `[lints]` with `workspace = true`."""
    for section in re.finditer(r"^\[lints\]\s*$(.*?)(?=^\[|\Z)", manifest_text, re.M | re.S):
        if re.search(r"^\s*workspace\s*=\s*true\s*$", section.group(1), re.M):
            return True
    return False


def check_tree(root: Path) -> list:
    """Return error strings for members missing lint inheritance."""
    members = workspace_members((root / "Cargo.toml").read_text(encoding="utf-8"))
    errors = []
    if not members:
        return ["root [workspace] members table is empty"]
    for member in members:
        manifest = root / member / "Cargo.toml"
        if not manifest.is_file():
            errors.append(f"member has no manifest: {member}")
            continue
        if not manifest_inherits_lints(manifest.read_text(encoding="utf-8")):
            errors.append(f"member omits [lints] workspace = true: {member}")
    return errors


def run_fixture() -> None:
    """Inline self-test: passing and failing member layouts."""
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / "good").mkdir()
        (root / "bad").mkdir()
        (root / "Cargo.toml").write_text(
            '[workspace]\nmembers = [\n    "good",\n    "bad",\n]\n', encoding="utf-8"
        )
        (root / "good" / "Cargo.toml").write_text(
            '[package]\nname = "good"\nversion = "0.1.0"\n\n[lints]\nworkspace = true\n',
            encoding="utf-8",
        )
        (root / "bad" / "Cargo.toml").write_text(
            '[package]\nname = "bad"\nversion = "0.1.0"\n', encoding="utf-8"
        )
        errors = check_tree(root)
        assert errors == ["member omits [lints] workspace = true: bad"], errors
        (root / "bad" / "Cargo.toml").write_text(
            '[package]\nname = "bad"\nversion = "0.1.0"\n\n[lints]\nworkspace = true\n',
            encoding="utf-8",
        )
        assert check_tree(root) == [], "fixed layout must pass"
    print("lint-inheritance fixture ok")


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
        print('{"lint_inheritance":"pass"}')
        return 0
    print("usage: check_lint_inheritance.py (--check | --fixture)", file=sys.stderr)
    return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
