#!/usr/bin/env python3
"""Single benchmark-provenance authority for M047.

Collects one shared provenance object used by stream, stream-probe, and
datagram benchmark reports. Stdlib-only, portable across macOS/Linux.

Provenance schema (version 1)::

    {
        "schema": 1,
        "head_sha": "<40-hex HEAD>" | null,
        "worktree": "clean" | "dirty" | "unknown",
        "authoritative": bool,
        "source_fingerprint": "<64-hex sha256>" | null,
        "index_dirty": bool,
        "tracked_dirty": bool,
        "untracked_source": bool,
        "git_describe": str | null,
        "collector": "bench_provenance.py v1"
    }

Rules (frozen by M047 WP1, see qualification/performance/README.md):

* ``head_sha`` is the base HEAD from ``git rev-parse HEAD``. It is NOT
  proof of a clean tree by itself.
* ``authoritative`` is true only when ``worktree == "clean"`` and
  ``head_sha`` is a 40-hex SHA. A dirty tree can never be authoritative.
* ``source_fingerprint`` is null on clean trees and a stable
  ``sha256`` hex digest of the dirty source delta otherwise. It covers
  byte content of source-relevant dirty/untracked files (sorted
  ``status + relpath + content-sha`` manifest), never the output
  artifact itself.
* Source-relevant means: tracked modifications (staged or unstaged) and
  non-ignored untracked files, minus documented generated/output paths:
  any path with a ``target`` component, any ``*.json`` under
  ``qualification/performance/``, and caller-supplied ``--exclude``
  paths (e.g. the report destination being written).
* No absolute local paths, usernames, tokens, environment dumps, or
  full ``git status`` text are emitted.
* Collection never mutates Git state (no add/stash/commit).

Usage::

    python3 scripts/bench_provenance.py --json [--exclude PATH ...]
    python3 scripts/bench_provenance.py --json --require-clean [--exclude PATH ...]
"""

import argparse
import hashlib
import os
import subprocess
import sys

COLLECTOR_VERSION = "bench_provenance.py v1"
SCHEMA_VERSION = 1


def run_git(args, cwd):
    # Strip only trailing newlines: `git status --porcelain` encodes the
    # index state in the first column, so a leading space of the first
    # line is significant and must survive.
    return subprocess.check_output(
        ["git"] + args, cwd=cwd, text=True, stderr=subprocess.DEVNULL
    ).rstrip("\n")


def repo_root(start):
    return run_git(["rev-parse", "--show-toplevel"], start)


def is_excluded(relpath, extra_excludes):
    # relpath uses forward slashes, relative to repo root.
    parts = relpath.split("/")
    if "target" in parts:
        return True
    if "__pycache__" in parts:
        return True
    if relpath.endswith(".pyc"):
        return True
    if relpath.startswith("qualification/performance/") and relpath.endswith(".json"):
        return True
    for extra in extra_excludes:
        if relpath == extra or relpath.startswith(extra.rstrip("/") + "/"):
            return True
    return False


def normalize_extra_excludes(raw_excludes, root):
    # Resolve symlinks (e.g. macOS /var -> /private/var TMPDIR) before
    # relativizing so caller-supplied absolute paths match status paths.
    real_root = os.path.realpath(root)
    out = []
    for raw in raw_excludes or []:
        path = os.path.expanduser(raw)
        if os.path.isabs(path):
            try:
                rel = os.path.relpath(os.path.realpath(path), real_root)
            except ValueError:
                continue
        else:
            # Interpret relative excludes against the repo root, but also
            # accept cwd-relative paths by resolving them first.
            candidate = os.path.join(os.getcwd(), raw)
            if os.path.exists(candidate):
                try:
                    rel = os.path.relpath(os.path.realpath(candidate), real_root)
                except ValueError:
                    rel = raw
            else:
                rel = raw
        rel = rel.replace(os.sep, "/").strip("/")
        if rel in ("", ".", "..") or rel.startswith("../"):
            continue
        out.append(rel)
    return out


def collect(start_cwd, extra_excludes):
    root = repo_root(start_cwd)
    head_sha = run_git(["rev-parse", "HEAD"], root)
    try:
        describe = run_git(["describe", "--always", "--dirty", "--long"], root)
    except (OSError, subprocess.CalledProcessError):
        describe = None
    status = run_git(["status", "--porcelain=v1", "-uall"], root)
    excludes = normalize_extra_excludes(extra_excludes, root)

    index_dirty = False
    tracked_dirty = False
    untracked_found = False
    dirty_entries = []  # (status_tag, relpath)

    for line in status.splitlines():
        if not line:
            continue
        if len(line) < 4:
            continue
        x = line[0]
        y = line[1]
        relpath = line[3:].strip()
        # Handle rename/copy "old -> new": fingerprint the new path.
        if " -> " in relpath:
            relpath = relpath.split(" -> ")[-1].strip().strip('"')
        relpath = relpath.strip('"')
        if not relpath or relpath.startswith("../"):
            continue
        if is_excluded(relpath, excludes):
            continue
        if x == "?" and y == "?":
            untracked_found = True
            dirty_entries.append(("untracked", relpath))
        elif x == "!" and y == "!":
            # Ignored; --porcelain without --ignored should not emit these,
            # but skip defensively.
            continue
        else:
            if x not in (" ", "?", "!"):
                index_dirty = True
            if y not in (" ", "?", "!"):
                tracked_dirty = True
            tag = "staged" if (x not in (" ", "?", "!") and y in (" ", "?", "!")) else "modified"
            if x not in (" ", "?", "!") and y not in (" ", "?", "!"):
                tag = "staged+unstaged"
            dirty_entries.append((tag, relpath))

    untracked_source = untracked_found
    dirty = index_dirty or tracked_dirty or untracked_source

    fingerprint = None
    if dirty:
        manifest_lines = []
        for tag, relpath in sorted(dirty_entries):
            abspath = os.path.join(root, *relpath.split("/"))
            try:
                with open(abspath, "rb") as handle:
                    content_sha = hashlib.sha256(handle.read()).hexdigest()
            except (OSError, IsADirectoryError):
                # Deleted files, submodules, or unreadable entries: hash
                # the tag+path so the fingerprint still changes.
                content_sha = hashlib.sha256(b"<unreadable>").hexdigest()
            manifest_lines.append(f"{tag}\0{relpath}\0{content_sha}\n")
        fingerprint = hashlib.sha256(
            "".join(manifest_lines).encode("utf-8")
        ).hexdigest()

    if not _is_hex_sha(head_sha):
        worktree = "unknown"
        authoritative = False
        head_out = None
    elif dirty:
        worktree = "dirty"
        authoritative = False
        head_out = head_sha
    else:
        worktree = "clean"
        authoritative = True
        head_out = head_sha

    return {
        "schema": SCHEMA_VERSION,
        "head_sha": head_out,
        "worktree": worktree,
        "authoritative": authoritative,
        "source_fingerprint": fingerprint,
        "index_dirty": index_dirty,
        "tracked_dirty": tracked_dirty,
        "untracked_source": untracked_source,
        "git_describe": describe,
        "collector": COLLECTOR_VERSION,
    }


def _is_hex_sha(value):
    if not isinstance(value, str) or len(value) != 40:
        return False
    try:
        int(value, 16)
    except ValueError:
        return False
    return True


def main(argv=None):
    parser = argparse.ArgumentParser(description="M047 benchmark provenance collector")
    parser.add_argument("--json", action="store_true", help="print provenance JSON")
    parser.add_argument(
        "--require-clean",
        action="store_true",
        help="exit 2 when the source-relevant worktree is dirty",
    )
    parser.add_argument(
        "--exclude",
        action="append",
        default=[],
        help="extra repo-relative or absolute path to exclude from dirty detection "
        "(repeatable; e.g. the report output file)",
    )
    args = parser.parse_args(argv)

    try:
        provenance = collect(os.getcwd(), args.exclude)
    except (OSError, subprocess.CalledProcessError) as exc:
        print(f"bench_provenance: cannot determine Git provenance: {exc}", file=sys.stderr)
        return 1

    if args.require_clean and provenance["worktree"] != "clean":
        print(
            "bench_provenance: source-relevant worktree is dirty "
            f"(head={provenance['head_sha']}, "
            f"fingerprint={provenance['source_fingerprint']}); "
            "canonical retained evidence requires a clean tree",
            file=sys.stderr,
        )
        if args.json:
            import json

            print(json.dumps({"provenance": provenance}, sort_keys=True))
        return 2

    if args.json or not args.require_clean:
        import json

        print(json.dumps({"provenance": provenance}, sort_keys=True))
        return 0
    return 0


if __name__ == "__main__":
    sys.exit(main())
