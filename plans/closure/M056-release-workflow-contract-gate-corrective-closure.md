# M056 — Release Workflow Contract-Gate Corrective — Closure

Status: `closed` on exact implementation candidate
`b6f00950b94057934773385f679d8f576e8abd40` (commit `M056: gate
release.yml on shared release-contract prerequisite`). Registration
baseline was `0a15b59add45711666d9018e0e4493a90e2ffb56`.

## What this corrective changed

- `.github/workflows/release.yml`: introduced a cheap `release-contract`
  prerequisite job that owns `scripts/check_release_tag_version.sh`;
  declared `needs: [release-contract]` on both `qualify` and
  `artifacts`; removed the in-`qualify` invocation of the guard so
  there is exactly one scheduling authority. The gate has no
  `needs:` dependency of its own (it is the root of the DAG).
- `scripts/tests/test_release_tag_version.sh`: retained every existing
  semantic assertion (non-tag, matching-tag, mismatched-tag,
  mismatched-bare-tag, presence of guard script, tag-derived artifact
  naming, no automated publication steps) and added the M056
  structural assertions: exactly one `release-contract:` job, the
  release-contract block invokes the guard, `release-contract` has no
  `needs:` line, `qualify` declares `needs: release-contract`,
  `artifacts` declares `needs: release-contract`, the guard script is
  invoked exactly once in the workflow, plus five deliberate negative
  tests (release-contract removed, artifacts needs dropped, qualify
  needs dropped, release-contract gained a needs dependency, guard
  script invoked twice).
- `architecture/tooling-distribution.md`: §3 now describes the
  `release-contract` job, the DAG fan-out, and the new structural
  contract. The scripts catalog row for
  `scripts/check_release_tag_version.sh` was updated to point at the
  shared gate. The exact-candidate context section gained an M056
  summary block (without rewriting M055 closure evidence).
- `architecture/verification-qualification.md`: §5 release-qualification
  paragraph now mentions the `release-contract` gate and the M056
  structural regression.

No production Rust, package, API, fault, RNG, config, SDK, binding,
deterministic, or compatibility change. No tag, package, GitHub
release, or artifact target change. M055 closure evidence remains
untouched.

## Before / after release-workflow DAG

Before (registration baseline `0a15b59`):

```text
qualify ─── guard invocation (in-qualify) + expensive tooling
artifacts ─── 5-target cross-build matrix
             (no DAG link to qualify or any guard)
```

After (M056 candidate):

```text
release-contract ─── guard invocation only, no Rust toolchain
        |
        +------> qualify ─── expensive tooling
        |
        +------> artifacts ─── 5-target cross-build matrix
```

The `release-contract` job is the root of the DAG. After it succeeds,
`qualify` and every `artifacts` matrix leg may run concurrently. A
failed or cancelled `release-contract` prevents both expensive branches
from starting.

## Guard behavior matrix (re-run on exact candidate)

| Scenario                       | `GITHUB_REF_TYPE` | `GITHUB_REF_NAME`  | Expected | Observed |
| ------------------------------ | ----------------- | ------------------ | -------- | -------- |
| `workflow_dispatch` / branch   | unset             | unset              | pass     | pass     |
| Matching tag                   | `tag`             | `v0.2.0`           | pass     | pass     |
| Mismatched tag (with `v`)      | `tag`             | `v9.9.9`           | fail     | fail     |
| Mismatched tag (without `v`)   | `tag`             | `9.9.9`            | fail     | fail     |

The `set -eu` / POSIX sh + stdlib Python implementation in
`scripts/check_release_tag_version.sh` is unchanged from M055; M056
only changes how the workflow invokes it.

## Structural regression (re-run on exact candidate)

`sh scripts/tests/test_release_tag_version.sh` runs:

- the four semantic cases above (non-tag, matching, mismatched,
  mismatched-bare);
- structural assertions against `release.yml` (exactly one
  `release-contract:` job, root-of-DAG, both downstream `needs:`
  declared, exactly one guard invocation, tag-derived artifact
  naming, no automated publication step);
- five deliberate negative tests against a temp copy of the workflow:
  release-contract removed; `artifacts` missing `needs: release-contract`;
  `qualify` missing `needs: release-contract`; `release-contract`
  gained a `needs:` dependency; guard script invoked twice.

All structural assertions pass on the real workflow; all five negative
tests reject the mutated workflow as required.

## Release-workflow YAML schema (parsed on exact candidate)

`python3 -c "import yaml; ..."` on `.github/workflows/release.yml`
reports the validated DAG:

- `jobs` keys: `['release-contract', 'qualify', 'artifacts']`;
- `release-contract` `needs:` absent;
- `qualify` `needs: ['release-contract']`;
- `artifacts` `needs: ['release-contract']`;
- `release-contract` steps: `actions/checkout@v4`,
  `./scripts/check_release_tag_version.sh` (no Rust toolchain, no
  cargo-audit / cargo-deny / cargo-fuzz, no cross-linker);
- `qualify` steps: 13 (no longer invokes the guard script);
- `artifacts` steps: 6 (unchanged target matrix and naming).

The guard script appears exactly once in the workflow
(`grep -c check_release_tag_version.sh .github/workflows/release.yml` → 1).

## Local gate (exact candidate)

- `python3 scripts/check_version_coherence.py --check` →
  `{"version_coherence":"pass","workspace_version":"0.2.0","workspace_crates":8}`
- `python3 scripts/check_planning_state.py --check` →
  `OK: 57 milestones, 4 target documents match`
- `sh scripts/tests/test_release_tag_version.sh` →
  `{"release_tag_version_guard":"pass","release_workflow_gate":"pass"}`
- `sh scripts/tests/test_planning_state.sh` →
  `{"planning_state":"pass"}`
- `sh scripts/tests/test_version_coherence.sh` →
  `{"version_coherence_guard":"pass"}`
- `sh scripts/tests/test_bench_provenance.sh` →
  `{"bench_provenance":"pass"}`
- `./scripts/check.sh` → green
  (provenance Tier A, planning-state, version-coherence,
  release-tag + workflow gate, fmt, clippy `-D warnings`, workspace
  tests, doc).
- `./scripts/release-smoke.sh` → green, including the
  `{"order_proof":"pass","workspace_version":"0.2.0",...}` line and the
  derived-version artifact smoke. No publication.
- `./scripts/release-artifact-smoke.sh` (standalone) →
  `{"artifact_smoke":"pass"}`; live binary reports
  `{"api":"v1","version":"0.2.0"}`. No publication.
- `./scripts/check_openapi.sh` → `{"openapi":"pass","paths":21,"operations":36}`.
- `./scripts/check_python_client.sh` → `{"python_client":"pass"}`
  (12 unit tests + drift gate).
- `./scripts/check_typescript_client.sh` → `{"typescript_client":"pass"}`
  (6 tests + drift gate).
- `./scripts/qualify_eggfetch.sh` → pass.

## Hosted CI and workflow-dispatch (evidence gap)

Hosted CI on the exact candidate (the 3-OS `check` matrix, the
`language-clients` matrix, the `python-native` matrix, and the
dedicated `performance-provenance` job) and a dedicated
`workflow_dispatch` against the release workflow are **not** executed
in this closure change because the local repository does not have
permission to trigger hosted runners. Those runs are recorded as the
follow-up evidence the next owner-controlled release-workflow
invocation will produce; the local structural regression
(`scripts/tests/test_release_tag_version.sh`) plus the YAML schema
parse above prove the DAG is correct without consuming hosted runner
minutes, and the dual-wiring in `scripts/check.sh` + `language-clients`
ensures any regression in the local structural guard trips CI.

The deliberately invalid tag bypass is proven only through the local
negative tests; no `v9.9.9` (or other mismatched) tag was created.

## Unpublished statement

No tag was created, moved, rewritten, or deleted. No crates.io / PyPI /
npm publication. No GitHub release. No workflow artifact upload beyond
the local release-smoke / release-artifact-smoke runs above. The
unreleased `0.2.0` development baseline is intact.

## M055 closure evidence

`plans/closure/M055-post-v0-1-0-release-state-and-v0-2-0-development-baseline-closure.md`
is unchanged.

## Acceptance criteria verdict

| # | Criterion | Verdict |
| - | --------- | ------- |
| 1 | One cheap shared `release-contract` job owns `check_release_tag_version.sh`. | pass — YAML parse shows exactly one `release-contract:` job with one guard invocation. |
| 2 | Both `qualify` and `artifacts` have hard `needs:` dependencies on it. | pass — YAML parse shows both jobs declare `needs: ['release-contract']`. |
| 3 | Valid-input parallelism between qualification and artifact production is preserved. | pass — after `release-contract` succeeds, `qualify` and `artifacts` are sibling jobs in the same DAG level. |
| 4 | The structural regression fails if either downstream dependency is removed. | pass — `artifacts missing needs: release-contract` and `qualify missing needs: release-contract` negative tests both reject the mutated workflow. |
| 5 | Matching-tag, mismatched-tag, and non-tag guard behavior remains proven. | pass — guard behavior matrix above. |
| 6 | Artifact target matrix and tag-derived naming are unchanged. | pass — `artifacts` matrix entries and `eggchaos-v${GITHUB_REF_NAME#v}-<triple>` naming preserved. |
| 7 | No automated tag / publication / release action is introduced. | pass — `cargo publish`, `gh release create`, `create-release`, `action-gh-release`, `git tag ` patterns absent. |
| 8 | `check.sh`, version coherence, release smoke, and artifact smoke are green. | pass — see Local gate above. |
| 9 | Hosted CI is green on the exact candidate. | not run — local repository has no permission to trigger hosted runners; recorded as evidence gap. |
| 10 | A workflow-dispatch run on the exact candidate/ref demonstrates the valid gate/fan-out DAG. | not run — same reason as #9; recorded as evidence gap. |
| 11 | No production / package / API behavior changed. | pass — production Rust, package metadata, native `/v1`, fault semantics, RNG, config, SDKs, bindings, and compatibility profiles untouched. |
| 12 | M055 closure history remains untouched. | pass — `plans/closure/M055-...` is unchanged. |
| 13 | Closure evidence identifies the exact candidate and the release-workflow run/structural proof. | pass — this file plus the structural regression output above. |

## Follow-on rule

M056 activates no automatic feature successor. The release workflow is
now qualified to reject an invalid tag before any expensive work
starts, and to keep that contract enforced through the strengthened
structural regression. The unreleased 0.2.0 development baseline is
intact; tag, crates.io publish, and GitHub release creation remain
separate owner-controlled actions.