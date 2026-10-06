# M061 — M060 Closure-Evidence and Plan-Status Consistency Corrective

Status: closed (closed on exact candidate `97913754c125864dbbccb4a269e5ee51102c3de8`; hosted run `37366364200` green 15/15; evidence in `plans/closure/M061-m060-closure-evidence-and-plan-status-consistency-corrective-closure.md`; as written at registration: `ready`)
Depends on: M060 historical closure; post-M060 documentation audit at `3ebee30cd33ea141a680acd5e6569a453fcdf124`
Role: additive planning/evidence corrective; production and published release surfaces frozen
Registration baseline: `3ebee30cd33ea141a680acd5e6569a453fcdf124`

## Objective

Close the remaining M060 closure-hygiene defects without reopening or rewriting
the M060 implementation tranche:

1. reconcile the M060 plan header with the registry's closed status;
2. correct the M060 closure record's exact-head hosted-evidence claim without
   pretending a later CI run qualified an earlier SHA;
3. strengthen the planning-state guard so registry status and numbered-plan
   `Status:` headers cannot silently diverge again;
4. obtain hosted CI on the exact M061 corrective candidate and record that
   evidence as the additive closure authority for this gap.

M060 remains a historical closed milestone. M061 is the evidence/status
successor, analogous to prior additive qualification correctives: it does not
re-run the v0.2.0 release or change runtime/release behavior.

## Research baseline

At registration baseline
`3ebee30cd33ea141a680acd5e6569a453fcdf124`:

- `plans/registry.md` records M060 as **closed**.
- The generated planning-state blocks in `AGENTS.md`, `plans/README.md`,
  `plans/roadmap.md`, and `architecture/overview.md` also project M060 as
  the highest closed milestone with no active/ready successor.
- `plans/060-post-v0-2-0-documentation-status-and-drift-guard-cleanup-corrective.md`
  still says `Status: ready`. The M053 planning guard currently verifies
  registry rows and generated blocks but does not compare registry status to
  each numbered plan's own status header.
- The M060 closure record names
  `e897cc4767f0bb2c10feaae46232ab2e52666d90` as its "Exact closure
  candidate" and says hosted CI is a hard gate whose run ID would be appended.
  GitHub has no Actions run for `e897cc4`.
- The first later green hosted CI found in this line is run
  `37028684309` on `474220662ed1daff406b6b2639884580768ea9c3`
  (15/15 jobs). That run is useful descendant evidence but must **not** be
  described as exact-head qualification of `e897cc4`.
- The current registration baseline `3ebee30` has its own green CI run
  `37266737795`. The commit is documentation-only and re-audited the
  architecture/tooling deep dives, including correction of the M060
  `check.sh` catalog. This is a healthy current baseline but likewise does
  not retroactively qualify `e897cc4`.
- The October 5 documentation audit already fixed the previously observed
  stale `architecture/tooling-distribution.md` `check.sh` inventory and
  broader overview/deep-dive factual drift. M061 must preserve those fixes
  rather than duplicate or revert them.
- M058 remains the published `v0.2.0` authority on `b6a277d`; M059 remains
  the pre-publication hardening authority on `1409d0f`. M061 has no release
  publication role.

The remaining defect class is planning/evidence consistency, not runtime,
security, dependency, API, or release correctness.

## Scope

### In scope

1. correct the M060 plan's top-level status metadata to `closed` and add a
   closure pointer/evidence note consistent with the registry;
2. add an explicit M061 reconciliation note to the M060 closure record that:
   - preserves the historical local qualification narrative;
   - states that `e897cc4` had no hosted CI run;
   - distinguishes later descendant green runs from exact-SHA evidence;
   - points to M061 as the additive exact-head hosted reconciliation;
3. update the M060 registry closure note after M061 closes so it accurately
   distinguishes implementation/local evidence from the later M061 hosted
   reconciliation;
4. extend `scripts/check_planning_state.py` so each registered numbered
   plan's top-level `Status:` token must agree with the registry row;
5. extend `scripts/tests/test_planning_state.sh` fixtures/negative tests to
   prove plan-header/registry mismatches fail;
6. keep `plans/registry.md` as the only hand-maintained status authority;
   plan headers are checked projections/metadata, not a second authority;
7. re-run the existing M060 release-state-doc guard to prove the corrective
   does not regress the post-v0.2.0 documentation state;
8. obtain normal hosted CI on the exact final M061 implementation candidate;
9. write an M061 closure record with the exact candidate SHA and exact hosted
   run ID/job result;
10. reconcile the canonical planning-state projections after M061 closes.

### Non-goals

- no Rust production-source change;
- no public Rust API, feature, wire, CLI, SDK, binding, config, fault,
  scenario, RNG, evidence, Toxiproxy, or Eggfetch semantic change;
- no Cargo manifest/lockfile/dependency/MSRV/package-version change;
- no security-policy change;
- no release workflow trigger, DAG, permission, action-pin, artifact,
  attestation, checksum, or publication change;
- no tag or registry publication;
- no `v0.2.0` requalification or release `workflow_dispatch` merely to
  repair planning evidence;
- no rewriting of M058/M059 closure evidence;
- no false assertion that run `37028684309` or `37266737795` executed on
  `e897cc4`;
- no broad documentation refactor already handled by the `3ebee30`
  architecture audit.

## Affected surfaces

Expected implementation surfaces are limited to:

- `plans/061-m060-closure-evidence-and-plan-status-consistency-corrective.md`;
- `plans/registry.md`;
- `plans/060-post-v0-2-0-documentation-status-and-drift-guard-cleanup-corrective.md`;
- `plans/closure/M060-post-v0-2-0-documentation-status-and-drift-guard-cleanup-corrective-closure.md`;
- `scripts/check_planning_state.py`;
- `scripts/tests/test_planning_state.sh`;
- canonical planning projections/current-state prose in:
  - `AGENTS.md`;
  - `plans/README.md`;
  - `plans/roadmap.md`;
  - `architecture/overview.md`;
- `plans/closure/M061-m060-closure-evidence-and-plan-status-consistency-corrective-closure.md`
  at closure.

No other file should change without an explicit recorded reason.

## Ordered work packages

### WP1 — Freeze the evidence lineage

Before edits:

1. record registration baseline `3ebee30`;
2. verify:
   - M060 registry row = closed;
   - M060 plan header = ready;
   - M060 closure record = `e897cc4` exact candidate;
   - no Actions run exists for `e897cc4`;
   - run `37028684309` is green 15/15 on `4742206`;
   - run `37266737795` is green on `3ebee30`;
3. record the distinction:
   - `e897cc4`: M060 implementation/local qualification candidate;
   - `4742206`: later planning/closure descendant with green hosted CI;
   - `3ebee30`: current docs-audited registration baseline with green hosted
     CI;
   - final M061 candidate: future exact hosted reconciliation authority;
4. verify M058/M059 closure records are unchanged.

This lineage must be copied into M061 closure evidence verbatim enough that a
reviewer cannot mistake descendant CI for exact-SHA CI.

### WP2 — Reconcile M060 status metadata additively

Update the M060 plan header from:

`Status: ready`

to a closed form consistent with the repository conventions, with a pointer
to its existing M060 closure record and an explicit note that the hosted
exact-head evidence gap is reconciled by M061.

Do not alter the historical M060 work-package body except where a current
status pointer is necessary to avoid contradiction.

Update the M060 closure record additively:

- preserve the original implementation/local test evidence;
- do not erase the fact that the original record expected hosted CI later;
- add a clearly labeled M061 reconciliation/erratum section;
- state that no hosted workflow run exists for `e897cc4`;
- classify `37028684309@4742206` and
  `37266737795@3ebee30` as later descendant evidence only;
- state that M061 exact-head CI is the authoritative hosted closure evidence
  for the planning/evidence corrective.

Do not claim that M060's implementation candidate changed after the fact.
M061 supplies additive evidence; it does not rewrite Git history.

### WP3 — Make registry ↔ plan-header status consistency mechanical

Extend `scripts/check_planning_state.py` using Python stdlib only.

For every numbered plan referenced by the registry:

1. read the plan file;
2. locate the top-level status declaration near the plan header;
3. parse the leading status token from a form such as:
   - `Status: closed`
   - `Status: closed (...explanation...)`
   - `Status: ready`
4. normalize only the recognized vocabulary
   `active|blocked|closed|ready`;
5. require the parsed plan status to equal the registry row status;
6. emit an actionable `plan-status-mismatch` error naming milestone,
   registry status, plan status, and path;
7. fail distinctly for missing, duplicate, or unparseable top-level status
   metadata rather than silently skipping it.

Authority rule: the registry remains canonical. The guard checks that the
plan metadata follows it; it must never infer registry status from plan files.

Before enabling the rule on the live tree, audit all registered numbered
plans. If historical plan files use a parseable explanatory suffix, support
that syntax. If a genuinely malformed historical plan would require broad
historical rewriting, stop and document the conflict rather than weakening the
guard into a current-M060 special case.

### WP4 — Strengthen planning-state regression coverage

Extend `scripts/tests/test_planning_state.sh` / the guard's inline fixture to
prove at least:

1. matching registry/plan status passes;
2. registry `closed` + plan `ready` fails;
3. registry `ready` + plan `closed` fails;
4. missing `Status:` fails;
5. duplicate top-level `Status:` declarations fail;
6. explanatory suffix after a valid status token is accepted if that syntax
   exists in current plans;
7. the real tree passes after the M060 header correction;
8. existing registry uniqueness/order/dependency/generated-block tests remain
   intact;
9. existing dual wiring remains intact:
   - `scripts/check.sh` via `test_planning_state.sh`;
   - bare `check_planning_state.py --check` in the
     `language-clients` CI job.

Do not create a second planning-status checker or dedicated CI job.

### WP5 — Reconcile registry/current planning projections

Register M061 as `ready` while implementing; after the exact candidate is
qualified:

- mark M061 `closed`;
- update the M060 row with an additive note that its hosted exact-head
  evidence gap is reconciled by M061;
- preserve M060's historical implementation candidate `e897cc4`;
- regenerate the four planning-state blocks with
  `python3 scripts/check_planning_state.py --write`;
- current state after closure must read:
  - highest closed milestone M061;
  - no active/ready milestones;
  - M058/M059/M060 historical authority roles preserved.

The October 5 `3ebee30` architecture/tooling corrections must remain intact.

### WP6 — Exact-head qualification and closure

Required local gate on the exact implementation candidate:

```sh
python3 scripts/check_planning_state.py --check
sh scripts/tests/test_planning_state.sh
python3 scripts/check_release_state_docs.py --check
sh scripts/tests/test_release_state_docs.sh
python3 scripts/check_version_coherence.py --check
sh scripts/tests/test_release_tag_version.sh
./scripts/check.sh
git diff --check
```

Additionally prove:

- no Rust production source changed;
- no Cargo manifest/lockfile changed;
- no OpenAPI/generated SDK contract changed;
- no package version/MSRV changed;
- no release workflow behavior changed;
- M058/M059 closure records are byte-identical;
- M060 closure changes are limited to transparent additive evidence
  reconciliation;
- `architecture/tooling-distribution.md` still records the eight-guard
  `check.sh` inventory introduced/fixed at `3ebee30`;
- the release-state documentation guard remains green.

Hosted evidence:

- require ordinary CI on the **exact** M061 candidate;
- record the run ID, exact `head_sha`, conclusion, and job census;
- the run must include the `language-clients` legs that execute the bare
  planning-state guard independently;
- do not use a CI run from an ancestor or descendant as exact-candidate
  evidence.

A release `workflow_dispatch` run is not required because M061 must not
change the release workflow behavior. If implementation requires any release
workflow behavioral change, stop and register separate work.

## Invariants

M061 must preserve:

- published `v0.2.0` tag/release and `b6a277d` target;
- all eight published Rust crate `0.2.0` artifacts;
- M059 hardening state and dependency graph;
- workspace version `0.2.0`, MSRV 1.89;
- all public APIs/contracts/runtime semantics;
- M060 release-state documentation guard;
- October 5 architecture/tooling corrections at `3ebee30`;
- registry as sole status authority;
- historical closure lineage: factual correction is additive and explicit,
  never silent retroactive relabeling.

## Acceptance criteria

M061 may close only when:

1. M060's plan header agrees with its closed registry row;
2. the M060 plan points to its closure record and M061 reconciliation;
3. the M060 closure record explicitly states that `e897cc4` had no hosted CI
   run;
4. descendant runs `37028684309` and `37266737795` are labeled as
   descendant evidence, not exact qualification of `e897cc4`;
5. the planning guard validates registry ↔ plan-header status consistency for
   registered numbered plans;
6. missing/duplicate/unparseable status metadata fails closed;
7. mismatch negative fixtures pass;
8. existing planning guard behavior/fixtures remain green;
9. the live tree has no registry/plan-header status mismatch;
10. M060's registry row preserves its implementation candidate/history and
    points to M061 for hosted closure-evidence reconciliation after closure;
11. M058/M059 closure evidence is untouched;
12. M060 release-state-doc guard remains green;
13. `./scripts/check.sh` and `git diff --check` are green on the exact
    candidate;
14. no production/dependency/package/API/release behavior changed;
15. hosted ordinary CI is green on the **exact M061 candidate**;
16. the M061 closure record contains the exact SHA, run ID, and job census;
17. canonical planning-state blocks are regenerated from the registry;
18. M061 is marked closed only after exact-head hosted evidence exists;
19. final registry state has no active/ready/blocked milestone unless a new
    independent plan has been explicitly registered;
20. no statement implies that later CI retroactively ran on `e897cc4`.

## Stop conditions

Stop and register separate work if:

- plan-status auditing reveals broad historical corruption requiring a large
  rewrite rather than bounded normalization;
- M060's implementation evidence is found materially false beyond the hosted
  exact-head gap already identified;
- a runtime, API, dependency, security, or release-workflow defect is found;
- release workflow behavior must change;
- exact-head hosted CI cannot be obtained;
- correcting evidence would require deleting or obscuring historical facts.

## Closure evidence

At closure create:

`plans/closure/M061-m060-closure-evidence-and-plan-status-consistency-corrective-closure.md`

It must record:

- registration baseline `3ebee30`;
- exact implementation/closure candidate SHA;
- the M060 evidence-lineage table
  (`e897cc4`, `4742206`, `3ebee30`, final M061 candidate);
- plan-status census result;
- changed-file inventory;
- local gate results;
- exact hosted CI run ID + exact head SHA + job census;
- proof of no production/release behavior delta;
- confirmation M058/M059 closure records are untouched;
- final registry/planning projection state.

## Follow-on

M061 activates no automatic successor. Once closed, the M060 documentation
cleanup line is considered fully reconciled and exact-head hosted evidence is
restored for the planning/evidence layer. Any further feature, patch release,
dependency migration, language publication, or runtime work requires a new
evidence-backed milestone.
