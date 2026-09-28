# M057 — M056 Closure-Evidence Reconciliation and Exact-Head Requalification

Status: ready
Depends on: M056 closed
Role: qualification/evidence corrective
Registration baseline: c2a721cd35b4383be99d4ba744a59d810ebcafe1

## Objective

Reconcile the remaining evidence defect in M056 without changing the release
workflow implementation.

M056's implementation candidate
`b6f00950b94057934773385f679d8f576e8abd40` correctly introduced the shared
`release-contract` prerequisite and the strengthened structural regression.
However, the M056 plan required hosted CI to be green on that **exact
candidate**, while the M056 closure record explicitly states that hosted CI
was not run on `b6f0095`.

The subsequent closure/documentation commit
`c2a721cd35b4383be99d4ba744a59d810ebcafe1` changes no workflow,
test, production, package, or API implementation and has a green hosted CI run
(`36487117317`, 14/14 jobs). That is strong production-equivalence evidence,
but it is not retroactive exact-candidate evidence for `b6f0095`.

M057 therefore becomes the additive qualification authority for the M056
release-workflow state. It must close only on a new exact candidate that has
hosted CI evidence attached to that exact SHA. It must not rewrite M056
closure history to pretend the missing run existed.

## Baseline facts

At registration:

- M056 implementation candidate: `b6f00950b94057934773385f679d8f576e8abd40`;
- M056 closure commit/current pre-M057 head:
  `c2a721cd35b4383be99d4ba744a59d810ebcafe1`;
- diff `b6f0095..c2a721c` is documentation/planning/closure evidence only;
- release workflow implementation is unchanged between those commits;
- current release DAG is:
  ```text
  release-contract
         |
         +------> qualify
         |
         +------> artifacts (5-target matrix)
  ```
- current branch-tip CI run `36487117317` is green 14/14;
- no CI run exists for exact M056 implementation SHA `b6f0095`;
- no v0.2.0 tag, package publication, or GitHub release exists;
- M055 closure evidence remains historical and untouched.

## Scope

### In scope

- exact-head requalification of the already-implemented M056 workflow state;
- additive evidence clarifying the M056 exact-candidate hosted-CI gap;
- registry/roadmap/overview wording that distinguishes:
  - M056 implementation authority;
  - M056 historical closure evidence and its explicit gap;
  - M057 final evidence/qualification authority for the corrected release DAG;
- hosted CI on the exact M057 candidate;
- re-running the release-workflow structural regression and version-coherence
  guards on that same candidate;
- closure evidence under `plans/closure/`.

### Non-goals

- no release-workflow YAML behavior change unless requalification uncovers a
  new defect;
- no change to `scripts/check_release_tag_version.sh`;
- no package/version change from unreleased `0.2.0`;
- no runtime/API/CLI/config/fault/RNG/SDK/binding/compatibility change;
- no new release target or artifact format;
- no tag creation or publication;
- no branch-protection/ruleset work;
- no rewrite of M055 or M056 closure history.

## Authority model

M057 must preserve the historical record rather than erase it.

- M056 remains the implementation milestone for the shared
  `release-contract` DAG.
- The M056 closure file remains accurate historical evidence that hosted CI
  was unavailable on `b6f0095` at closure time.
- M057 becomes the **final exact-head qualification authority** for that
  release-workflow corrective once it closes cleanly.
- Registry/roadmap language may point from M056 to M057 for final hosted
  qualification, but must not claim M056 itself had exact-candidate hosted CI.

## Ordered work packages

### WP1 — Freeze implementation equivalence

Verify and record that no release-workflow or test implementation changed
between `b6f0095` and the M057 candidate except evidence/planning
reconciliation.

At minimum compare:

- `.github/workflows/release.yml`;
- `scripts/check_release_tag_version.sh`;
- `scripts/tests/test_release_tag_version.sh`;
- `scripts/check_version_coherence.py`;
- package manifests/lockfiles.

If any of those change after registration, M057 is no longer
qualification-only and must either explain the change explicitly or stop and
re-plan.

### WP2 — Re-run local structural and release guards

On the exact M057 candidate run:

```sh
python3 scripts/check_planning_state.py --check
python3 scripts/check_version_coherence.py --check
sh scripts/tests/test_release_tag_version.sh
sh scripts/tests/test_version_coherence.sh
./scripts/check.sh
./scripts/release-smoke.sh
./scripts/release-artifact-smoke.sh
```

Required structural facts:

- exactly one `release-contract` job;
- `release-contract` has no `needs`;
- `qualify` needs `release-contract`;
- `artifacts` needs `release-contract`;
- the release tag/version guard is invoked exactly once;
- all deliberate DAG-negative tests reject their mutated fixtures;
- artifact naming/target matrix remain unchanged;
- no automated publication action appears.

### WP3 — Obtain exact-candidate hosted CI

Hosted CI is mandatory for M057 closure.

The exact M057 candidate SHA must have a completed successful CI run with the
normal matrix:

- 3 `check` jobs;
- 1 `performance-provenance` job;
- 8 `language-clients` jobs;
- 2 `python-native` jobs.

Record the run ID and exact SHA in the closure evidence.

A green run on an ancestor or documentation successor is supporting evidence
only and cannot satisfy this criterion.

If hosted CI does not run automatically on the candidate, M057 remains
`ready` or becomes `blocked`; do not close it by substituting a
production-equivalent SHA.

### WP4 — Record release-workflow dispatch evidence honestly

A dedicated `workflow_dispatch` of the release workflow is useful but is not
required if the available repository connection cannot trigger it.

If a dispatch can be executed, record that:

- `release-contract` completes first;
- `qualify` and artifact matrix legs start only after the gate succeeds;
- both expensive branches may overlap after the gate;
- no publication action occurs.

If dispatch cannot be executed, record the limitation explicitly and rely on:

- the structural DAG regression;
- YAML structure;
- exact-head normal hosted CI.

Do not create a deliberately invalid tag.

### WP5 — Reconcile planning authority

At closure:

- keep M056 marked `closed`;
- preserve its closure file verbatim;
- update the M056 registry note to identify M057 as the hosted exact-head
  qualification successor;
- mark M057 closed only after WP2/WP3 succeed;
- describe M057 as the final qualification authority for the M056 DAG;
- regenerate all planning-state projections.

## Invariants

M057 must not change:

- unreleased development version `0.2.0`;
- published v0.1.0 history;
- native `/v1`;
- config/RNG/provenance schema versions;
- fault or deterministic semantics;
- 36-operation native control surface;
- five release artifact targets;
- artifact filename convention;
- owner-controlled release/publication policy;
- M055/M056 historical closure files.

## Acceptance criteria

M057 may close only when:

1. the M056 release DAG implementation is unchanged from the qualified state;
2. release structural regression passes on the exact candidate;
3. version/planning coherence checks pass;
4. `check.sh`, release smoke, and artifact smoke pass;
5. normal hosted CI is green on the exact M057 candidate SHA;
6. the closure record names the exact CI run and 14-job result;
7. no ancestor/successor SHA is substituted for exact-candidate evidence;
8. M056 closure history remains unchanged and its original hosted-CI gap is
   preserved as historical fact;
9. registry/roadmap identify M057—not M056—as the final hosted qualification
   authority for the release-workflow contract-gate corrective;
10. no tag, registry publication, GitHub release, or production/API change
    occurs.

## Stop conditions

Stop and re-plan if:

- hosted CI exposes a new workflow/test defect;
- any release workflow, guard, package manifest, or production file must
  change to obtain a clean run;
- the exact candidate cannot receive hosted CI;
- the repository state diverges from the M056 implementation beyond
  documentation/planning evidence.

## Closure evidence

Create:

`plans/closure/M057-m056-closure-evidence-reconciliation-and-exact-head-requalification-closure.md`

Record:

- exact candidate SHA;
- exact hosted CI run ID and 14/14 job census;
- comparison proving the release workflow/guards remain M056-equivalent;
- local structural negative-test results;
- planning/version coherence results;
- release-smoke and artifact-smoke results;
- workflow-dispatch evidence or explicit unavailability;
- explicit statement that M055/M056 closure files were not rewritten;
- explicit statement that M057 supersedes only the **final hosted
  qualification authority**, not M056's implementation ownership.

## Follow-on rule

M057 activates no automatic feature successor.

After clean closure, the M055/M056/M057 line reads:

`M055 release-state/version baseline -> M056 release-contract implementation -> M057 exact-head qualification authority`.

The repository remains an unreleased 0.2.0 development baseline until the
owner separately authorizes publication.
