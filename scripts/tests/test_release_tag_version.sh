#!/usr/bin/env sh
# M055 regression: release tag/package version agreement guard.
#
# Proves `scripts/check_release_tag_version.sh` enforces the WP6 contract:
# - non-tag refs (workflow_dispatch/branch) pass on manifest coherence;
# - a tag equal to the workspace version passes;
# - a tag different from the workspace version fails, before any
#   expensive qualification or artifact build;
# - `release.yml` invokes the guard before `release-smoke.sh`, still
#   derives artifact names from the tag, and introduces no tag
#   creation or publication step.
#
# POSIX sh + stdlib Python only; no network access; no mutation.
set -eu

fail() { echo "FAIL: $1" >&2; exit 1; }

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
GUARD="$REPO_ROOT/scripts/check_release_tag_version.sh"
WORKFLOW="$REPO_ROOT/.github/workflows/release.yml"
[ -f "$GUARD" ] || fail "missing guard: $GUARD"
[ -f "$WORKFLOW" ] || fail "missing workflow: $WORKFLOW"

WS_VERSION=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["workspace"]["package"]["version"])')
[ -n "$WS_VERSION" ] || fail "cannot read workspace version"

# 1. Non-tag ref (dispatch/branch): coherence only, must pass.
env -u GITHUB_REF_TYPE -u GITHUB_REF_NAME sh "$GUARD" >/dev/null \
  || fail "non-tag run must pass on a coherent tree"

# 2. Matching tag passes.
GITHUB_REF_TYPE=tag GITHUB_REF_NAME="v$WS_VERSION" sh "$GUARD" >/dev/null \
  || fail "matching tag v$WS_VERSION must pass"

# 3. Mismatched tag fails.
if GITHUB_REF_TYPE=tag GITHUB_REF_NAME="v9.9.9" sh "$GUARD" >/dev/null 2>&1; then
  fail "mismatched tag v9.9.9 must fail against workspace $WS_VERSION"
fi

# 4. Mismatched tag without a leading `v` also fails.
if GITHUB_REF_TYPE=tag GITHUB_REF_NAME="9.9.9" sh "$GUARD" >/dev/null 2>&1; then
  fail "mismatched bare tag 9.9.9 must fail against workspace $WS_VERSION"
fi

# 5. Structural: release.yml must invoke the guard ...
grep -q "check_release_tag_version\.sh" "$WORKFLOW" \
  || fail "release.yml must invoke check_release_tag_version.sh"
# ... before the first expensive qualification step ...
guard_line=$(grep -n "check_release_tag_version\.sh" "$WORKFLOW" | head -1 | cut -d: -f1)
smoke_line=$(grep -n "release-smoke\.sh" "$WORKFLOW" | head -1 | cut -d: -f1)
[ -n "$guard_line" ] && [ -n "$smoke_line" ] || fail "cannot locate guard/smoke lines"
[ "$guard_line" -lt "$smoke_line" ] \
  || fail "tag/version guard (line $guard_line) must precede release-smoke.sh (line $smoke_line)"
# ... while artifact naming still derives from the release tag.
grep -q 'GITHUB_REF_NAME#v' "$WORKFLOW" \
  || fail "artifact naming must continue to derive from the release tag"
# 6. No automated tag creation or publication may be introduced.
for pattern in "cargo publish" "gh release create" "create-release" "action-gh-release" "git tag "; do
  if grep -q "$pattern" "$WORKFLOW"; then
    fail "release.yml must not contain automated release action: $pattern"
  fi
done

echo '{"release_tag_version_guard":"pass"}'
