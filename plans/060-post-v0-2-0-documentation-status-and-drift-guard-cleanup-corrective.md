# M060 — Post-v0.2.0 Documentation, Release-State, and Drift-Guard Cleanup Corrective

Status: closed (header reconciled to the registry status by M061; as written at registration: `ready`; evidence in `plans/closure/M060-post-v0-2-0-documentation-status-and-drift-guard-cleanup-corrective-closure.md`; the hosted exact-head CI gap on `e897cc4` is reconciled by M061 (`plans/061-m060-closure-evidence-and-plan-status-consistency-corrective.md` and its closure record))
Depends on: M058 closed; M059 closed
Role: post-release documentation/governance corrective; production behavior frozen
Registration baseline: `c5a151df069f93faf421c0ded9728d1dc3a347ee`

## Objective

Reconcile the repository's hand-written current-state documentation with the
actual M059 closure and M058 `v0.2.0` publication, correct stale security and
tooling descriptions, and add a narrow mechanical guard for future
release-state/documentation drift.

This milestone is intentionally non-feature work. It must not alter runtime
semantics, public Rust APIs, native HTTP/OpenAPI contracts, CLI behavior,
fault determinism, compatibility profiles, package versions, dependency
selection, release artifacts, or the published `v0.2.0` lineage.

## Research baseline

At registration baseline
`c5a151df069f93faf421c0ded9728d1dc3a347ee`:

- M059 is closed on exact hardening candidate `1409d0fa11dddfeff3f680ef453bf830e1339606`
  with hosted CI `36922662654` green 15/15 and release qualification
  `36922672946` green 7/7.
- M058 is closed. The frozen release candidate is
  `b6a277d5ad4267bd602bc15a4333b14322057b90`; annotated tag `v0.2.0`
  peels to that commit; pre-tag CI/dispatch and tag-triggered CI/release runs
  are all green; all eight Rust crates are published at `0.2.0`; the GitHub
  Release carries the five qualified binaries plus SHA-256 sidecars and
  Sigstore attestations.
- Current `main` is one commit ahead of the frozen release candidate, and
  that post-tag commit changes only documentation/planning/closure files.
  There is no post-`v0.2.0` production-code delta at registration.
- `plans/registry.md` and the generated planning-state blocks already say
  M058/M059 are closed, but several hand-written current-state passages still
  describe the pre-publication state.
- `AGENTS.md` still says M059 is ready, M058 is blocked, and publication
  must wait for M059.
- `plans/README.md` carries the same stale M059-ready/M058-blocked summary.
- `plans/059-pre-v0-2-0-security-dependency-and-maintenance-hardening-corrective.md`
  still has a `Status: ready` header even though its closure record and
  registry row are closed.
- `SECURITY.md` still describes `v0.1.x` as supported until v0.2.0 is
  published, `v0.2.x` as supported only once published, and `main` as
  pre-release. That conflicts with its stated policy that security fixes are
  provided on the latest published release line.
- `docs/release-notes-v0.2.0.md` still says M058 is ready and publication
  remains to happen, despite being the final published release notes.
- `architecture/embedding-native.md` and
  `architecture/verification-qualification.md` still identify M057-era
  HEAD/evidence as the current authority rather than distinguishing the M059
  hardening candidate from the M058 published release candidate.
- `architecture/overview.md` is correct at the top, but later current-state
  reference/index prose still contains M057/M059/M058 pre-publication claims.
- `architecture/tooling-distribution.md` still documents
  `check_typescript_client.sh` as using `npm install`, although the script
  now unconditionally uses
  `npm ci --ignore-scripts --no-audit --no-fund`; it also describes the
  old Python-native crate-root `allow(unsafe_code)` audit even though the
  crate root now denies unsafe and the allowance is scoped to the PyO3
  macro-facing layer.
- `architecture/tooling-distribution.md` and other current-state prose still
  call M055 the "unreleased 0.2.0" baseline without clearly marking that as a
  historical development baseline.
- `.github/workflows/release.yml` contains a stale M059-era comment saying
  M058 remains blocked. The workflow behavior itself is already correct and
  qualified.
- `plans/registry.md` contains historical current-prose saying the owner may
  proceed with the v0.1.0 release even though v0.1.0 and v0.2.0 are already
  published. Historical milestone rows/closure records remain valid and must
  not be rewritten to erase their original chronology.

The defect class is therefore documentation/status drift after a successful
release, not a production or release-workflow correctness failure.

## Scope

### In scope

1. reconcile all non-historical current-state planning prose with M058/M059
   closure and the published `v0.2.0` state;
2. update the M059 plan header to closed with a pointer to its closure
   evidence, without altering the historical implementation instructions;
3. correct the security support table and `main` wording to match the
   repository's existing "latest published release line" policy;
4. convert the v0.2.0 release notes from pre-publication wording to a final
   published-state record without rewriting their substantive feature claims;
5. update architecture/evidence authority prose so M057 remains historical,
   M059 is the pre-publication hardening authority, and M058 is the published
   release authority;
6. reconcile tooling documentation with the actual TypeScript lockfile install
   path and current Python-native unsafe boundary;
7. remove or rewrite stale workflow comments while preserving workflow YAML
   behavior byte-for-byte except for comments or an explicitly approved cheap
   documentation guard step;
8. reconcile stale release-state prose in `plans/registry.md` outside the
   milestone table while preserving the table as the status authority;
9. add a stdlib-only release-state/current-documentation drift check plus
   focused regression tests;
10. wire that cheap guard into the normal repository check and one independent
    hosted CI path, following the M053 dual-wiring pattern.

### Non-goals

- no Rust production-source change;
- no public Rust API or crate feature change;
- no native `/v1`, OpenAPI, config, CLI, SDK, binding, Toxiproxy, Eggfetch,
  scenario, RNG, provenance-schema, or fault-semantic change;
- no Cargo manifest, lockfile, package-version, dependency-version, MSRV, or
  feature-selection change;
- no release-workflow DAG, trigger, permission, action pin, artifact,
  attestation, checksum, or publication behavior change;
- no new tag, crate publication, GitHub Release, PyPI/npm publication, or
  language-package namespace decision;
- no rewrite of M058/M059 closure evidence or earlier historical closure
  records;
- no policy expansion to support multiple release lines; this milestone only
  makes `SECURITY.md` consistent with its existing latest-published-line
  policy;
- no broad prose/style rewrite unrelated to the identified state drift.

If implementing a documentation guard would require production code,
dependency changes, privileged workflow behavior, or release-DAG changes,
stop and register a separate corrective rather than expanding M060.

## Affected surfaces

Expected edits are limited to:

- `plans/060-post-v0-2-0-documentation-status-and-drift-guard-cleanup-corrective.md`;
- `plans/registry.md`;
- `plans/059-pre-v0-2-0-security-dependency-and-maintenance-hardening-corrective.md`;
- `AGENTS.md`;
- `plans/README.md`;
- `plans/roadmap.md`;
- `architecture/overview.md`;
- `architecture/embedding-native.md`;
- `architecture/verification-qualification.md`;
- `architecture/tooling-distribution.md`;
- `docs/release-notes-v0.2.0.md`;
- `SECURITY.md`;
- comment-only cleanup in `.github/workflows/release.yml`;
- a focused checker under `scripts/` and regression under `scripts/tests/`;
- `scripts/check.sh` and one existing CI job for cheap guard wiring;
- `plans/closure/M060-post-v0-2-0-documentation-status-and-drift-guard-cleanup-corrective-closure.md`
  at closure.

No other file should change without a recorded reason.

## Ordered work packages

### WP1 — Freeze authoritative post-release facts and complete the drift census

Before editing prose:

1. record registration HEAD `c5a151d`;
2. verify the M058 and M059 registry rows and closure files;
3. verify the M058 frozen candidate/tag relationship and the four green hosted
   run IDs already recorded by closure;
4. compare `b6a277d..HEAD` and confirm there is no post-release production
   source/manifests/workflow-behavior delta;
5. search the current-state documentation set for stale forms including:
   - M059 "ready";
   - M058 "ready" or "blocked";
   - "do not tag/publish until M059";
   - `0.2.0` described as unreleased/pre-publication;
   - M057 described as the current/final release authority rather than
     historical authority;
   - `npm install` as the TypeScript qualification install path;
   - crate-root `allow(unsafe_code)` as the current Python-native boundary;
   - future-tense v0.1/v0.2 publication instructions outside historical
     numbered plans/closures.
6. classify every match as either historical context that must stay verbatim
   or current-state prose that must be reconciled.

The census must be recorded in closure evidence so cleanup is auditable rather
than an open-ended prose sweep.

### WP2 — Reconcile planning and release-state prose

Update current-state planning surfaces so they agree with the registry:

- M058: closed, published `v0.2.0` authority on `b6a277d`;
- M059: closed pre-publication security/dependency/maintenance authority on
  `1409d0f`;
- M060: current ready cleanup milestone until closure;
- no active/blocked release milestone after M058/M059.

Specifically:

1. change the M059 plan's top-level status line to closed and point to its
   closure record without editing the historical work-package body;
2. replace stale hand-written M059-ready/M058-blocked prose in `AGENTS.md`
   and `plans/README.md`;
3. reconcile the current-state header/prose in `plans/roadmap.md` and
   `architecture/overview.md`;
4. remove stale "owner may proceed with v0.1.0 publication" current prose from
   the registry's narrative section while retaining historical milestone rows;
5. regenerate the four M053 current-planning-state blocks from the registry;
6. do not hand-edit generated block bodies.

### WP3 — Correct SECURITY.md and the final v0.2.0 release notes

`SECURITY.md` must reflect the policy it already states:

- `v0.2.x`: supported current published release line;
- `v0.1.x`: unsupported after v0.2.0 publication unless the owner explicitly
  adopts a multi-line support policy in a separate decision;
- older lines: unsupported;
- `main`: unreleased development after `v0.2.0`, not itself a supported
  release line.

Preserve private vulnerability-reporting instructions and do not invent a new
contact mechanism.

`docs/release-notes-v0.2.0.md` must become a final published-state record:

- say M058 is closed and v0.2.0 was published from `b6a277d`;
- retain the exact required/published surface, language-registry deferrals,
  compatibility notes, checksums/provenance instructions, and feature
  descriptions;
- remove future-tense "publication requires" wording where it claims the
  release has not happened;
- do not rewrite implementation history merely for style.

### WP4 — Reconcile architecture/tooling evidence authorities

Update the architecture deep dives to distinguish authority layers clearly:

- M057: historical hosted qualification for the M056 release-contract DAG;
- M059: exact pre-publication security/dependency/API/provenance hardening
  candidate `1409d0f`;
- M058: exact published v0.2.0 candidate `b6a277d`, tag/release/publication
  authority;
- current `main`: post-tag documentation/planning lineage unless production
  changes appear during implementation.

At minimum:

1. update `architecture/embedding-native.md` evidence header and any current
   M057-only claims;
2. update `architecture/verification-qualification.md` top-level exact-head
   authority and latest closure pointers;
3. sweep `architecture/overview.md` lower index/reference sections for
   pre-publication statements missed by its already-correct top paragraph;
4. update `architecture/tooling-distribution.md`:
   - TypeScript gate = unconditional
     `npm ci --ignore-scripts --no-audit --no-fund`;
   - Python-native crate root denies unsafe;
   - PyO3 macro-facing bridge owns the scoped unsafe allowance;
   - handwritten unsafe remains forbidden by the audit;
   - M055 is a historical development-version baseline, not evidence that
     0.2.0 remains unreleased;
   - current release authority is M058, with M059 as the hardening authority.
5. remove the obsolete M058-blocked comment from `release.yml` without
   changing YAML behavior.

### WP5 — Add a bounded release-state/current-doc drift guard

Add a stdlib-only checker, preferably
`scripts/check_release_state_docs.py`, with a focused regression wrapper
under `scripts/tests/`.

The checker must derive milestone status from `plans/registry.md`; it must
not create a second hand-maintained milestone authority.

When M058 and M059 are closed, it should at minimum detect reintroduction of
the known contradictory current-state claims in the selected live
documentation set. It should also verify a small number of source-backed
tooling claims whose previous drift was concrete:

- the TypeScript check documentation agrees that the install is `npm ci`
  with lifecycle scripts disabled;
- Python-native documentation agrees that the crate root denies unsafe and
  only the macro-facing boundary is allowed;
- `SECURITY.md` identifies v0.2.x as the supported current line and does not
  describe publication as future;
- release notes identify v0.2.0 as published/closed rather than ready.

Guard design requirements:

- target only named current-state documents; do not scan historical numbered
  plans, ADRs, closures, or retained evidence indiscriminately;
- use explicit positive/negative fixtures so a stale phrase fails and
  historical context does not;
- emit actionable file/claim diagnostics;
- no network access;
- no dependency beyond Python stdlib/POSIX shell;
- keep runtime negligible relative to the existing check;
- wire into `scripts/check.sh` and one independent existing CI job so local
  aggregate removal is detectable.

If a generic semantic checker becomes brittle, keep the rules narrow to the
specific release-state/tooling invariants above rather than building a prose
linter.

### WP6 — Exact-head cleanup qualification and closure

Required local gate on the exact M060 candidate:

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

- `git diff b6a277d..HEAD --` contains no Rust production source, Cargo
  manifest/lockfile, OpenAPI, generated SDK contract, release artifact, or
  package-version change introduced by M060;
- any `.github/workflows/*.yml` M060 delta is comment-only except the
  explicitly planned cheap current-doc guard invocation;
- all action pins, permissions, release DAG edges, artifact targets, and
  attestation steps are unchanged;
- the M058/M059 closure files are byte-identical to their pre-M060 forms;
- the current-state drift census from WP1 has no unresolved contradictory
  matches.

Hosted CI is required on the exact candidate because M060 adds/changes a CI
guard. A release `workflow_dispatch` rerun is **not** required for
comment/doc-only cleanup unless implementation changes release-workflow
behavior beyond comments. If that stop condition is crossed, M060 must not
close under this plan.

## Invariants

M060 must preserve:

- published tag `v0.2.0` and its target;
- all eight published Rust crate `0.2.0` artifacts;
- workspace version `0.2.0` and MSRV 1.89;
- all public Rust APIs and crate features;
- native `/v1` contract (21 paths / 36 operations), OpenAPI, config schema
  v1, CLI inventory, SDK operation tables, and Python-native visible API;
- RNG v1, provenance schema v1, scenario V1/V2 semantics, stream/datagram
  fault semantics, evidence bounds, and Toxiproxy compatibility profiles;
- M059 dependency/action/security/provenance hardening;
- release workflow contract, five binary targets, SHA-256 sidecars, and
  Sigstore attestations;
- all historical closure records and evidence.

## Acceptance criteria

M060 may close only when:

1. the complete stale-current-state census is recorded and classified;
2. M059's plan header is closed and points to its closure record;
3. AGENTS/plans README/roadmap/architecture overview agree with the registry;
4. generated M053 planning-state blocks are regenerated, not hand-maintained;
5. SECURITY.md says v0.2.x is the supported current release line and no
   longer treats v0.2.0 publication as future;
6. release notes record v0.2.0 as published and M058 as closed;
7. embedding-native and verification architecture docs no longer present
   M057 as the current release authority;
8. tooling-distribution accurately documents npm-ci and the narrowed PyO3
   unsafe boundary;
9. M055 "unreleased 0.2.0" wording is historicalized where retained;
10. stale future v0.1/v0.2 publication prose is removed from live registry
    narrative;
11. release.yml's stale M058-blocked comment is removed/reconciled without
    behavioral YAML changes;
12. the new release-state/current-doc checker and its positive/negative
    fixtures pass;
13. the new guard is wired locally and independently in CI;
14. planning/version/tag guards remain green;
15. `./scripts/check.sh` is green on the exact candidate;
16. hosted CI is green on the exact candidate;
17. no production source, dependency graph, public contract, package version,
    release artifact, or release-DAG behavior changed;
18. M058/M059 closure records are untouched;
19. a closure record identifies the exact M060 candidate and CI run;
20. the registry is updated from `ready` to `closed` only after the above
    evidence exists.

## Stop conditions

Stop and register separate work if:

- a stale statement exposes an actual runtime/release defect rather than a
  documentation mismatch;
- correcting the claim would require changing a public API or support policy;
- dependency/manifests/lockfiles need alteration;
- release-workflow triggers, DAG, permissions, action pins, artifact
  generation, attestation, or publication behavior need alteration;
- M058/M059 closure evidence is found internally inconsistent;
- the proposed drift guard requires network access, privileged workflow
  permissions, or a broad historical-doc rewrite.

## Closure evidence

At closure create:

`plans/closure/M060-post-v0-2-0-documentation-status-and-drift-guard-cleanup-corrective-closure.md`

It must record:

- registration and exact closure SHAs;
- the WP1 drift census and disposition;
- changed-file inventory;
- proof that the production/release artifact surface did not change;
- local gate outputs;
- hosted CI run ID and job result;
- release-workflow-delta classification;
- confirmation that M058/M059 closure records are unchanged;
- final registry/current-state result.

## Follow-on

M060 activates no automatic feature milestone. After closure, Eggchaos is in a
clean post-`v0.2.0` maintenance state. Any patch release, new feature tranche,
language-registry publication, EggServe compatibility-line migration, or other
post-release work requires a fresh evidence-backed plan.
