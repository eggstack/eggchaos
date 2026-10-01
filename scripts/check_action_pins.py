#!/usr/bin/env python3
"""M059 structural guard: immutable actions and least-privilege policy.

Every external `uses:` in owned workflows must reference a full
40-character commit SHA (never a branch, moving major tag, or short
SHA), every workflow file must declare an explicit top-level
`permissions:` block, and every checkout step must disable credential
persistence (no authenticated Git operation follows checkout).

Stdlib only. Usage:
  python3 scripts/check_action_pins.py --check    # real tree
  python3 scripts/check_action_pins.py --fixture  # inline self-test
"""

import re
import sys
import tempfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
WORKFLOWS_DIR = REPO_ROOT / ".github" / "workflows"

SHA_RE = re.compile(r"^[0-9a-f]{40}$")


def check_file(path: Path) -> list:
    """Return error strings for one workflow file."""
    errors = []
    text = path.read_text(encoding="utf-8")
    if not re.search(r"^permissions:\s*$", text, re.M):
        errors.append(f"{path.name}: missing top-level permissions: block")
    in_checkout = False
    checkout_has_flag = False
    checkout_line = 0
    for lineno, line in enumerate(text.splitlines(), 1):
        m = re.match(r"\s*- uses:\s*(\S+)\s*(#.*)?$", line)
        if m:
            if in_checkout and not checkout_has_flag:
                errors.append(
                    f"{path.name}:{checkout_line}: checkout step "
                    "must set persist-credentials: false"
                )
            ref = m.group(1)
            in_checkout = ref.startswith("actions/checkout@")
            checkout_has_flag = False
            checkout_line = lineno
            if ref.startswith("./"):
                continue  # local composite action, nothing to pin
            if "@" not in ref:
                errors.append(f"{path.name}:{lineno}: uses: without a ref: {ref}")
                continue
            revision = ref.split("@", 1)[1]
            if not SHA_RE.match(revision):
                errors.append(
                    f"{path.name}:{lineno}: uses: must pin a full "
                    f"40-character SHA, got: {ref}"
                )
            continue
        if in_checkout and re.search(r"persist-credentials:\s*false", line):
            checkout_has_flag = True
    if in_checkout and not checkout_has_flag:
        errors.append(
            f"{path.name}:{checkout_line}: checkout step "
            "must set persist-credentials: false"
        )
    return errors


def check_tree(root: Path) -> list:
    """Return error strings for all workflow files."""
    workflows = root / ".github" / "workflows"
    files = sorted(workflows.glob("*.yml"))
    if not files:
        return ["no workflow files found"]
    errors = []
    for path in files:
        errors.extend(check_file(path))
    return errors


def run_fixture() -> None:
    """Inline self-test: pinned passes, moving tags fail."""
    pinned = """name: ok
permissions:
  contents: read
on: [push]
jobs:
  x:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1
        with:
          persist-credentials: false
      - uses: ./local
"""
    loose = """name: bad
on: [push]
jobs:
  x:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: some/action@abcd123
"""
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        (root / ".github" / "workflows").mkdir(parents=True)
        (root / ".github" / "workflows" / "ok.yml").write_text(pinned)
        assert check_tree(root) == [], "pinned layout must pass"
        (root / ".github" / "workflows" / "ok.yml").write_text(loose)
        errors = check_tree(root)
        assert len(errors) == 4, errors
    print("action-pins fixture ok")


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
        print('{"action_pins":"pass"}')
        return 0
    print("usage: check_action_pins.py (--check | --fixture)", file=sys.stderr)
    return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
