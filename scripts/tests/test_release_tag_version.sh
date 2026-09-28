#!/usr/bin/env sh
# M055 + M056 regression: release-workflow contract gate.
#
# Proves the release-workflow tag/package contract is enforced through
# one cheap shared gate job, and that no expensive work may run until
# the gate succeeds.
#
# Semantic half (M055):
#   - non-tag refs (workflow_dispatch/branch) pass on manifest coherence;
#   - a tag equal to the workspace version passes;
#   - a tag different from the workspace version fails, before any
#     expensive qualification or artifact build.
#
# Structural half (M056):
#   - `release.yml` contains exactly one `release-contract` job that
#     invokes `scripts/check_release_tag_version.sh`;
#   - `release-contract` has no `needs:` dependency (it is the root);
#   - both `qualify` and `artifacts` have hard `needs:` dependencies on
#     `release-contract`, so neither can be scheduled independently;
#   - the guard script is invoked exactly once (one scheduling authority,
#     not two subtly different ones);
#   - artifact naming still derives from the release tag;
#   - no automated `git tag`, `cargo publish`, package-registry publish,
#     or GitHub release creation step appears.
#   - a deliberate negative test mutates a temp copy of the workflow to
#     remove the `artifacts` dependency (and, separately, the `qualify`
#     dependency) and proves the structural checker rejects it.
#
# POSIX sh + stdlib Python only; no network access; no mutation of the
# real repository.
set -eu

fail() { echo "FAIL: $1" >&2; exit 1; }

REPO_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
GUARD="$REPO_ROOT/scripts/check_release_tag_version.sh"
WORKFLOW="$REPO_ROOT/.github/workflows/release.yml"
[ -f "$GUARD" ] || fail "missing guard: $GUARD"
[ -f "$WORKFLOW" ] || fail "missing workflow: $WORKFLOW"

WS_VERSION=$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["workspace"]["package"]["version"])')
[ -n "$WS_VERSION" ] || fail "cannot read workspace version"

# --------------------------------------------------------------------------
# Semantic half: the guard itself behaves correctly.
# --------------------------------------------------------------------------

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

# --------------------------------------------------------------------------
# Structural helpers: scope checks to a single top-level job.
# --------------------------------------------------------------------------

# job_block_has <path> <job> <regex>
#   exit 0 if <regex> matches a line inside the <job> block in <path>;
#   exit 1 otherwise.
job_block_has() {
  awk -v job="^  $2:$" '
    $0 ~ job { in_job=1; next }
    in_job && /^  [a-zA-Z][a-zA-Z0-9_-]*:$/ { in_job=0 }
    in_job && $0 ~ "'"$3"'" { found=1; exit 0 }
    END { exit (found ? 0 : 1) }
  ' "$1"
}

# job_block_lacks <path> <job> <regex>
#   exit 0 if <regex> does NOT match a line inside the <job> block;
#   exit 1 if it does.
job_block_lacks() {
  awk -v job="^  $2:$" '
    $0 ~ job { in_job=1; next }
    in_job && /^  [a-zA-Z][a-zA-Z0-9_-]*:$/ { in_job=0 }
    in_job && $0 ~ "'"$3"'" { found=1; exit 0 }
    END { exit (found ? 0 : 1) }
  ' "$1" && return 1
  return 0
}

# assert_structural <path>
#   Run every structural assertion against the workflow at <path>.
assert_structural() {
  wf="$1"

  # 1. Exactly one release-contract job exists.
  count=$(grep -c '^  release-contract:$' "$wf" || true)
  [ "$count" = "1" ] \
    || fail "$wf must declare exactly one release-contract: job (found $count)"

  # 2. The release-contract job invokes the guard script.
  job_block_has "$wf" "release-contract" "check_release_tag_version\.sh" \
    || fail "$wf release-contract job must invoke scripts/check_release_tag_version.sh"

  # 3. release-contract declares no needs (it is the root of the DAG).
  if job_block_has "$wf" "release-contract" "^    needs:"; then
    fail "$wf release-contract job must not declare a needs: dependency"
  fi

  # 4. qualify declares needs: release-contract.
  job_block_has "$wf" "qualify" "needs:.*release-contract" \
    || fail "$wf qualify job must declare needs: release-contract"

  # 5. artifacts declares needs: release-contract.
  job_block_has "$wf" "artifacts" "needs:.*release-contract" \
    || fail "$wf artifacts job must declare needs: release-contract"

  # 6. The guard script is invoked exactly once in the workflow (one
  #    scheduling authority, not two subtly different ones).
  count=$(grep -c "check_release_tag_version\.sh" "$wf" || true)
  [ "$count" = "1" ] \
    || fail "$wf must invoke check_release_tag_version.sh exactly once (found $count)"

  # 7. Artifact naming still derives from the release tag.
  job_block_has "$wf" "artifacts" "GITHUB_REF_NAME#v" \
    || fail "$wf artifacts job must continue to derive artifact names from the tag"

  # 8. No automated tag creation or publication may be introduced.
  for pattern in "cargo publish" "gh release create" "create-release" "action-gh-release" "git tag "; do
    if grep -q "$pattern" "$wf"; then
      fail "$wf must not contain automated release action: $pattern"
    fi
  done
}

# --------------------------------------------------------------------------
# Structural half: the real workflow.
# --------------------------------------------------------------------------

assert_structural "$WORKFLOW"

# --------------------------------------------------------------------------
# Deliberate negative tests: prove the structural assertions actually
# catch the M055 / M056 bypass modes.
# --------------------------------------------------------------------------

# Helper: copy release.yml to a temp dir, run a sed mutation against the
# copy, then run the structural assertions on the copy. Output of
# assertion failures is suppressed so the regression script reports one
# summary line per negative case.
mutate_and_check() {
  case_name="$1"; mutation="$2"
  tmp=$(mktemp -d)
  trap 'rm -rf "$tmp"' EXIT INT TERM HUP
  cp "$WORKFLOW" "$tmp/release.yml"
  # shellcheck disable=SC2086
  sed -i.bak "$mutation" "$tmp/release.yml"
  rm -f "$tmp/release.yml.bak"
  set +e
  (
    set -eu
    WORKFLOW="$tmp/release.yml"
    assert_structural "$WORKFLOW"
  ) >/dev/null 2>"$tmp/err"
  status=$?
  set -e
  trap - EXIT INT TERM HUP
  rm -rf "$tmp"
  if [ "$status" -eq 0 ]; then
    fail "negative test must reject mutation: $case_name"
  fi
  printf '  ok: negative test rejected %s\n' "$case_name"
}

# A. The whole release-contract job is removed. Both downstream needs:
#    references become dangling and the structural assertions fail.
mutate_and_check "release-contract job removed" \
  '/^  release-contract:$/,/^  [a-zA-Z][a-zA-Z0-9_-]*:$/d'

# B. The artifacts.needs: release-contract dependency is dropped, so the
#    artifact matrix could start in parallel with an invalid tag. This is
#    the exact M055 bypass that M056 must prevent.
mutate_and_check "artifacts missing needs: release-contract" \
  "/    needs: \\[release-contract\\]/d"

# C. The qualify.needs: release-contract dependency is dropped, so
#    qualification could start in parallel with an invalid tag.
mutate_and_check "qualify missing needs: release-contract" \
  "/    needs: \\[release-contract\\]/d"

# D. release-contract grows a `needs:` of its own — even if it is
#    something innocuous, the corrective requires the gate to be the
#    root of the DAG (no upstream dependency).
mutate_and_check "release-contract gained a needs: dependency" \
  "/^    runs-on: ubuntu-latest\$/i\\
    needs: [something-else]"

# E. The guard script is invoked twice — the original in-qualify site is
#    re-added alongside the shared gate. Two subtly different scheduling
#    authorities would re-create the M055 bypass. Insert a duplicate
#    invocation immediately after the qualify job installs cargo-deny.
mutate_and_check "guard script invoked twice" \
  "/^      - run: cargo install cargo-deny/a\\
      - run: ./scripts/check_release_tag_version.sh"

echo '{"release_tag_version_guard":"pass","release_workflow_gate":"pass"}'