# M056 — Release Workflow Contract-Gate Corrective

Status: ready
Depends on: M055 closed
Role: release-orchestration correctness / qualification corrective
Registration baseline: 0a15b59add45711666d9018e0e4493a90e2ffb56

## Objective

Close the bounded release-workflow gap discovered after M055: the
tag/package-version guard is currently executed only inside the `qualify`
job, while the five-target `artifacts` matrix is an independent job with no
dependency on that validation.

On a mismatched release tag, `qualify` fails early as intended, but the
artifact matrix may still start in parallel, consume runners, build release
binaries, and upload workflow artifacts named from the invalid tag. This does
not publish a GitHub release or registry package, but it violates the M055
contract that tag/version mismatch must stop expensive qualification **and
artifact builds** before they begin.

M056 establishes one cheap release-contract gate as a prerequisite for every
expensive release-workflow branch and strengthens the structural regression so
the bypass cannot silently return.

M056 is orchestration-only. It must not change package versions, public APIs,
runtime behavior, release contents, artifact target coverage, or publication
policy.

## Baseline defect

At registration baseline
`0a15b59add45711666d9018e0e4493a90e2ffb56`:

- `scripts/check_release_tag_version.sh` correctly:
  - always runs manifest/version coherence;
  - accepts non-tag/workflow-dispatch runs after coherence;
  - requires `GITHUB_REF_NAME` (minus leading `v`) to equal the canonical
    workspace package version for tag runs;
  - rejects a mismatched tag.
- `.github/workflows/release.yml` invokes that guard only in `jobs.qualify`.
- `jobs.artifacts` has no `needs:` dependency on `qualify` or any shared
  validation job.
- therefore GitHub Actions may schedule `qualify` and the five artifact
  matrix legs concurrently; guard failure in `qualify` does not prevent
  artifact work from starting.
- `scripts/tests/test_release_tag_version.sh` proves guard behavior and
  guard-before-`release-smoke.sh` ordering, but does not prove that the
  artifact matrix is downstream of the same guard.

M055's closure remains historical evidence and must not be rewritten. M056 is
the additive corrective record for this post-closure finding.

## Preferred architecture

Use a single lightweight workflow job, named `release-contract` unless an
equally clear name is justified:

```text
release-contract
       |
       +------> qualify
       |
       +------> artifacts (5-target matrix)
```

The gate job should do only the minimum required to establish the contract:

1. checkout the exact workflow ref;
2. run `./scripts/check_release_tag_version.sh`.

It must not install Rust, cargo-audit, cargo-deny, cargo-fuzz, cross linkers, or
other expensive release tooling.

Both expensive jobs must declare the gate as a hard GitHub Actions dependency:

- `qualify: needs: release-contract`;
- `artifacts: needs: release-contract`.

A failed or cancelled contract job must therefore prevent both downstream jobs
from starting.

Do not solve this by making `artifacts` depend on the full `qualify` job:
that would serialize long qualification and multi-target artifact production
unnecessarily. The intended shape validates once, then permits the two
expensive branches to fan out in parallel on valid input.

## Scope

### In scope

- `.github/workflows/release.yml` DAG correction;
- reuse of `scripts/check_release_tag_version.sh` as the single release
  contract authority;
- strengthening `scripts/tests/test_release_tag_version.sh` or adding one
  focused release-workflow structure test;
- CI wiring needed to keep the structural regression blocking;
- release/tooling documentation reconciliation where it currently implies
  the M055 guard already gates artifact scheduling;
- exact-candidate qualification and closure evidence.

### Non-goals

- no package-version change from unreleased `0.2.0`;
- no changes to `scripts/check_version_coherence.py` semantics unless a
  narrowly necessary bug is discovered;
- no new release targets or artifact formats;
- no artifact-name change;
- no crates.io/PyPI/npm publication;
- no GitHub release creation;
- no tag creation, movement, deletion, or rewrite;
- no branch-protection/ruleset work;
- no runtime, fault, API, CLI, config, SDK, binding, deterministic, or
  compatibility change;
- no unrelated CI optimization/refactor.

## Affected files

Expected:

- `.github/workflows/release.yml`;
- `scripts/tests/test_release_tag_version.sh` and/or one focused companion
  structural test;
- `architecture/tooling-distribution.md`;
- planning registry/current-state projections;
- `plans/closure/M056-release-workflow-contract-gate-corrective-closure.md`
  at closure.

Avoid production Rust changes.

## Ordered work packages

### WP1 — Freeze current release-workflow behavior

Record the current workflow DAG and guard semantics before editing:

- trigger modes: `workflow_dispatch` and `push.tags = v*.*.*`;
- `qualify` timeout/environment/tool installation and qualification sequence;
- five artifact targets;
- tag-derived artifact naming;
- no automated tag creation/publication;
- current M055 guard behavior for matching, mismatching, and non-tag refs.

This baseline is needed to prove the corrective changes scheduling only.

### WP2 — Introduce the cheap shared release-contract gate

Add one lightweight job that:

- runs on every release-workflow invocation;
- checks out the workflow ref;
- executes `./scripts/check_release_tag_version.sh`;
- performs no expensive compiler/tool installation;
- has a short bounded timeout appropriate for a stdlib/shell manifest check.

The guard script remains the semantic authority. Do not duplicate its
tag/version comparison logic in YAML expressions.

If checkout plus the existing script cannot run on a supported hosted image
without extra setup, add only the minimum setup required and document why.

### WP3 — Gate both expensive branches

Make both existing expensive jobs depend directly on the shared gate:

- `qualify.needs = release-contract`;
- `artifacts.needs = release-contract`.

Preserve valid-input parallelism: after `release-contract` succeeds,
`qualify` and all artifact matrix legs may run concurrently.

Remove a redundant in-`qualify` guard invocation if and only if the shared
gate fully dominates `qualify`; there should be one scheduling authority, not
two subtly different ones.

### WP4 — Strengthen the release-workflow structural regression

Extend the existing regression so it proves all of the following:

1. the guard script itself:
   - passes for a matching tag;
   - fails for a mismatched tag;
   - passes coherence-only for a non-tag ref;
2. `release.yml` contains exactly one intended release-contract gate job
   invoking `check_release_tag_version.sh`;
3. `qualify` has a hard dependency on that gate;
4. `artifacts` has a hard dependency on that same gate;
5. the gate does not depend on either expensive job;
6. neither expensive job can be scheduled independently of the gate under the
   workflow's normal DAG;
7. artifact naming still derives from `GITHUB_REF_NAME`;
8. no automated `git tag`, `cargo publish`, package-registry publish, or
   GitHub release creation step appears.

Include a deliberate structural negative test: mutate/copy the workflow in a
temporary fixture to remove the artifact dependency (and, ideally, the qualify
dependency in a second case) and prove the structural checker rejects it.

Do not rely only on line-order comparison; the regression must reason about
job ownership/dependencies sufficiently to catch the actual M055 bypass.

### WP5 — Reconcile release/tooling documentation

Update release/tooling documentation to state the corrected contract:

- manifest/tag validation is a prerequisite job for **all** expensive release
  work;
- on valid input, qualification and artifact builds fan out after the gate;
- workflow dispatch skips only the tag-equality comparison but still requires
  manifest coherence;
- the workflow still builds/uploads CI artifacts only; publication/tag/release
  actions remain owner-controlled.

Do not rewrite M055 closure evidence. Refer to M056 as the corrective for the
post-closure orchestration gap.

### WP6 — Exact-candidate qualification

On the exact M056 implementation candidate run at minimum:

```sh
sh scripts/tests/test_release_tag_version.sh
./scripts/check.sh
./scripts/release-smoke.sh
./scripts/release-artifact-smoke.sh
```

Also verify:

- `python3 scripts/check_version_coherence.py --check` is green;
- planning-state guard is green;
- normal hosted CI is green on the exact candidate.

Then exercise the **valid** release-workflow DAG with
`workflow_dispatch` on the exact candidate/ref when repository permissions
permit it. Record evidence that:

- `release-contract` runs first and succeeds;
- `qualify` starts only after it;
- every artifact matrix leg starts only after it;
- qualification and artifact branches may overlap after the gate;
- all expected jobs complete successfully;
- no publication/release action occurs.

Do **not** create a deliberately invalid tag merely to test failure scheduling.
The local/CI structural negative test is the required proof for the invalid-tag
DAG.

## Invariants

M056 must preserve:

- workspace/first-party development version `0.2.0`;
- historical published `v0.1.0` release/tag;
- native `/v1`, config schema v1, RNG/determinism contracts;
- all 36 native operations;
- strict Toxiproxy v2.12 default and opt-in post-v2.12 profile;
- five release artifact targets and existing filename convention;
- release qualification commands and semantic gates except for moving the
  cheap tag/version guard into the shared prerequisite;
- owner-only tag/package/GitHub-release publication policy.

## Failure semantics

A release-contract failure is terminal for that workflow invocation's
expensive work.

If `release-contract` fails or is cancelled:

- `qualify` must not start;
- no artifact matrix leg may start;
- no release binary may be built or uploaded by the release workflow.

For `workflow_dispatch`, absence of a tag is not failure: manifest coherence
must pass, then both expensive branches may proceed.

## Acceptance criteria

M056 may close only when:

1. one cheap shared release-contract job owns
   `check_release_tag_version.sh`;
2. both `qualify` and `artifacts` have hard `needs` dependencies on it;
3. valid-input parallelism between qualification and artifact production is
   preserved after the gate;
4. the structural regression fails if either downstream dependency is removed;
5. matching-tag, mismatched-tag, and non-tag guard behavior remains proven;
6. artifact target matrix and tag-derived naming are unchanged;
7. no automated tag/publication/release action is introduced;
8. `check.sh`, version coherence, release smoke, and artifact smoke are green;
9. hosted CI is green on the exact candidate;
10. a workflow-dispatch run on the exact candidate/ref demonstrates the valid
    gate/fan-out DAG when repository permissions permit; if permissions make
    dispatch impossible, closure must explicitly record that evidence gap
    rather than claiming it ran;
11. no production/package/API behavior changed;
12. M055 closure history remains untouched;
13. closure evidence identifies the exact candidate and the release-workflow
    run/structural proof.

## Stop conditions

Stop and re-plan if:

- GitHub Actions semantics require serializing artifact production behind the
  full qualification job to guarantee gating;
- the guard needs network access or broad GitHub permissions;
- the fix requires changing package versions or release target coverage;
- a reusable-workflow extraction becomes necessary and materially broadens
  scope;
- correcting this reveals an actual publication side effect rather than only
  workflow-artifact generation.

## Closure evidence

Create:

`plans/closure/M056-release-workflow-contract-gate-corrective-closure.md`

Record:

- exact candidate;
- before/after release DAG;
- structural negative-test results;
- guard behavior matrix;
- `check.sh`, release-smoke, artifact-smoke, version-coherence results;
- hosted CI run;
- workflow-dispatch release-qualification run and job dependency evidence, or
  the explicit reason it could not be executed;
- confirmation that no tag/package/GitHub release was created;
- confirmation that M055 closure evidence was not rewritten.

## Follow-on rule

M056 activates no automatic feature successor.

After closure, the unreleased 0.2.0 development baseline remains intact and
the release workflow is qualified to reject an invalid tag before any
expensive release work starts. Actual v0.2.0 publication remains a separate
owner-controlled action.
