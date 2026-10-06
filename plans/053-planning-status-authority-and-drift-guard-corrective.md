# M053 — Planning Status Authority and Drift-Guard Corrective

Status: closed (header reconciled to the registry status by M061; as written at registration: `blocked`; evidence in `plans/closure/M053-planning-status-authority-and-drift-guard-corrective-closure.md`)
Depends on: M052 closed
Role: repository-governance corrective; plans/registry.md remains the sole milestone-status authority
Activation baseline: exact M052 closure candidate

## Objective

Eliminate recurring planning/status drift by making plans/registry.md mechanically authoritative for current milestone state and validating the small set of current-state summaries elsewhere in the repository.

The audit found a concrete example: architecture/overview.md still described M041 as the latest corrective authority after M048 had closed. M046-M048 hardened performance evidence provenance, but milestone-status prose remains manually duplicated across AGENTS.md, plans/README.md, plans/roadmap.md, and architecture/overview.md.

M053 must reduce that maintenance burden without rewriting historical plans or introducing a competing status database.

## Authority decision

plans/registry.md remains the canonical milestone/status/dependency source of truth.

Do not create registry.json, status.toml, or another hand-maintained status manifest.

A small stdlib-only script may parse the registry table and generate or check clearly delimited "current planning state" blocks in other documents. Generated summaries are projections; they are never independent authorities.

## Scope

### In scope

- reconcile the known stale current-state wording;
- add a stdlib-only planning-state checker/generator;
- verify every numbered active plan file has exactly one registry row;
- verify registry statuses are from the documented vocabulary;
- verify milestone numbers/plan filenames are unique and ordered;
- verify dependencies reference known milestones or explicit external/historical prerequisites;
- maintain delimited generated/current-state summary blocks in:
  - AGENTS.md;
  - plans/README.md;
  - plans/roadmap.md;
  - architecture/overview.md;
- wire cheap check mode into scripts/check.sh and hosted CI through an existing job;
- document how an implementation agent registers/activates/closes a milestone.

### Non-goals

- no rewrite of plans/archive;
- no mutation of historical closure records;
- no attempt to generate long-form roadmap architecture prose;
- no GitHub Issues/milestones as a second authority;
- no production Rust change;
- no release automation;
- no performance-provenance schema change.

## Generated-block principle

Only compact current-state facts should be generated or mechanically checked, for example:

- highest closed milestone;
- currently ready milestone(s);
- blocked successor chain;
- final pre-tag historical authority;
- currently active corrective/feature chain.

Long-form rationale remains hand-written.

The checker must fail when a generated block is stale, but it must not rewrite files during ordinary check mode.

## Ordered work packages

### WP1 — Define the registry parser

Parse only the stable Markdown table in plans/registry.md with Python stdlib.

Fail closed on:

- duplicate milestone IDs;
- duplicate plan paths;
- malformed status;
- missing plan file;
- out-of-order numeric milestone IDs;
- multiple rows for one numbered plan.

Do not attempt a general Markdown parser.

### WP2 — Add current-state block generation/checking

Define explicit begin/end markers in the four summary documents.

Provide:

- a write/update mode used intentionally when registering/closing work;
- a check mode used by CI/developers.

The generated block must be deterministic.

### WP3 — Reconcile current docs

Correct the stale M041/M048 statements and install the generated blocks without altering historical claims inside numbered plans or closure evidence.

### WP4 — Integrate the cheap guard

Add the check to scripts/check.sh and one existing CI path. It must require only Python stdlib and repository files and should complete in well under one second on normal hardware.

Do not add a new dedicated CI job.

### WP5 — Document planning workflow

Update AGENTS.md planning instructions so future agents:

1. add/update the numbered plan;
2. update registry.md;
3. regenerate current-state blocks;
4. run the drift guard;
5. never mark closed without exact-candidate evidence.

## Required verification

At minimum:

    python3 scripts/check_planning_state.py --check
    ./scripts/check.sh
    cargo test --workspace --all-features

Add fixture/unit coverage for malformed registry rows and stale generated blocks without modifying the real repository during tests.

## Acceptance criteria

M053 may close only when:

1. registry.md remains the sole hand-maintained status authority;
2. stale summary text is corrected;
3. the checker detects missing/duplicate/malformed plan registrations;
4. generated current-state blocks are deterministic;
5. scripts/check.sh fails on deliberate fixture drift;
6. no historical plan/archive/closure evidence is rewritten;
7. hosted CI includes the cheap guard without a new job;
8. exact-candidate closure evidence is recorded.

## Rejection / stop conditions

Stop and simplify if the checker requires a general Markdown dependency, a database, network access, or significant CI time.

Do not convert all planning prose into generated content.

## Closure evidence

Create plans/closure/M053-planning-status-authority-and-drift-guard-corrective-closure.md with exact candidate, fixture results, current-state block diff, and hosted CI result.

## Successor activation

Closing M053 activates M054.
