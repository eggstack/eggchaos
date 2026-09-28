#!/usr/bin/env sh
# M055 release-only guard: tag/package version agreement.
#
# On tag-triggered release runs (`GITHUB_REF_TYPE=tag`), strip the leading
# `v` from `GITHUB_REF_NAME` and require it to equal the canonical Rust
# workspace package version. A mismatch fails before any expensive
# qualification or artifact build, so a tag such as `v0.2.0` can never
# build/package a workspace whose metadata disagrees.
#
# On `workflow_dispatch` (or any non-tag ref) the tag comparison is
# skipped, but ordinary manifest coherence still runs, keeping manual
# qualification usable without a tag.
#
# Artifact naming continues to derive from the release tag on tag runs;
# this guard introduces no tag creation or publication step.
#
# POSIX sh + stdlib Python only; no network access; no mutation.
set -eu

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)

fail() { echo "FAIL: $1" >&2; exit 1; }

# Ordinary manifest coherence always applies, tag or not.
python3 "$REPO_ROOT/scripts/check_version_coherence.py" --check \
  || fail "manifest version coherence failed"

ws_version=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["workspace"]["package"]["version"])' ) || fail "cannot read workspace version"
[ -n "$ws_version" ] || fail "empty workspace version"

ref_type="${GITHUB_REF_TYPE:-}"
ref_name="${GITHUB_REF_NAME:-}"

if [ "$ref_type" = "tag" ]; then
  tag_version=$(printf '%s' "$ref_name" | sed 's/^v//')
  if [ "$tag_version" != "$ws_version" ]; then
    fail "tag/package version mismatch: tag ${ref_name} (version ${tag_version}) != workspace version ${ws_version}"
  fi
  printf '%s\n' "{\"tag_version_check\":\"pass\",\"tag\":\"$ref_name\",\"workspace_version\":\"$ws_version\"}"
else
  printf '%s\n' "{\"tag_version_check\":\"skip-no-tag\",\"ref_type\":\"$ref_type\",\"ref_name\":\"$ref_name\",\"workspace_version\":\"$ws_version\"}"
fi
