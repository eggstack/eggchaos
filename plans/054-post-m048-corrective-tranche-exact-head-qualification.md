# M054 — Post-M048 Corrective Tranche Exact-Head Qualification

Status: closed (header reconciled to the registry status by M061; as written at registration: `blocked`; evidence in `plans/closure/M054-post-m048-corrective-tranche-exact-head-qualification-closure.md`)
Depends on: M049, M050, M051, M052, M053 closed
Role: exact-head qualification and closure gate for the maintenance/correctness tranche
Activation baseline: exact M053 closure candidate

## Objective

Qualify the combined M049-M053 corrective tranche on one exact candidate and reconcile all current-state documentation before declaring the repository ready for a new feature line.

M054 contains no planned production feature implementation. Its job is to prove that invariant hardening, lifecycle consolidation, operation-authority consolidation, private modularization, and planning drift protection did not regress API surface, deterministic behavior, Toxiproxy compatibility, cross-language clients/bindings, or performance-sensitive runtime behavior.

## Scope

### In scope

- exact-head local full gate;
- OpenAPI/SDK drift checks;
- mandatory pinned Toxiproxy v2.12 differential;
- mandatory pinned post-v2.12 snapshot differential;
- Eggfetch integration qualification;
- language-client qualification;
- Python-native qualification;
- bounded fuzz qualification;
- release/package smoke;
- hosted CI exact-head evidence;
- targeted performance smoke sufficient to detect accidental no-fault/datagram hot-path regression from refactoring;
- public-surface census against the M048 baseline;
- planning/architecture reconciliation;
- closure records for M049-M054.

### Non-goals

- no new feature;
- no threshold retuning;
- no benchmark workload redesign;
- no release/tag/publication action;
- no API cleanup/removal;
- no new compatibility claim.

## Qualification matrix

### Tier 1 — deterministic local gate

Run:

    ./scripts/check.sh
    ./scripts/check_openapi.sh
    ./scripts/check_python_client.sh
    ./scripts/check_typescript_client.sh

### Tier 2 — compatibility/integration

Run mandatory oracle-backed qualification:

    TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 ./scripts/qualify_toxiproxy_v2_12.sh
    TOXIPROXY_POST_V2_12_SERVER="$(./scripts/fetch_toxiproxy_post_v2_12.sh)" EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1 ./scripts/qualify_toxiproxy_post_v2_12.sh

Also run:

    ./scripts/qualify_eggfetch.sh
    ./scripts/qualify_language_clients.sh
    ./scripts/qualify_python_native.sh

If a host cannot execute a required native-Python platform case, record it as incomplete and rely on the declared hosted matrix; do not relabel incomplete evidence as pass.

### Tier 3 — fuzz/security/package

Run the existing bounded fuzz suite according to release guidance, then:

    ./scripts/release-smoke.sh
    ./scripts/release-artifact-smoke.sh

Retain cargo audit/deny evidence through the ordinary hosted CI jobs.

### Tier 4 — performance non-regression

This tranche is not a performance optimization milestone, but M052 moves code around runtime/control/adapter boundaries. Run the existing stream and datagram benchmark smoke in the same topology and compare against the latest authoritative M047/M048-era evidence or the most recent valid baseline available at activation.

Do not retune budgets. Investigate material regression rather than accepting a new threshold.

### Tier 5 — hosted exact-head

Require one hosted run on the exact M054 candidate with all ordinary jobs green, including the M048 performance-provenance job and M053 planning-state guard.

Record run ID, job count, candidate SHA, and any platform-specific skips.

## Public/capability non-regression census

Explicitly verify:

- workspace crate list unchanged unless an additive private-support crate was separately justified;
- existing root exports remain;
- all 36 native operations remain;
- CLI command inventory remains;
- strict Toxiproxy v2.12 remains default;
- pinned post-v2.12 packet_loss profile remains opt-in;
- Scenario V1 and V2 response shapes remain;
- current mixed-family scenario capacity is not reduced;
- EmbeddedService signatures remain;
- Python/TypeScript/Python-native operation coverage remains;
- Eggfetch Dialer adapter behavior remains;
- no fixed-target boundary expansion;
- no generic C ABI or downstream EggReplay/EggProbe dependency appears.

## Ordered work packages

### WP1 — Candidate freeze

Record exact candidate SHA and confirm clean worktree/provenance state before qualification.

No production edits after qualification begins. Any fix creates a new candidate and restarts the affected evidence set.

### WP2 — Run local/integration qualification

Execute Tiers 1-4 and record commands/results.

### WP3 — Run hosted qualification

Push the exact candidate, wait for the ordinary CI matrix, and record hosted evidence.

### WP4 — Reconcile planning/docs

Update:

- plans/registry.md;
- plans/README.md;
- plans/roadmap.md;
- AGENTS.md;
- architecture/overview.md and affected deep dives;
- M049-M054 closure records.

The M053 drift guard must pass after reconciliation.

### WP5 — Closure verdict

Close M054 only if every declared required gate is green or a plan explicitly permits an incomplete platform case backed by hosted evidence.

Do not activate an automatic feature successor. Further work begins from a new observed need.

## Acceptance criteria

M054 may close only when:

1. M049-M053 each have exact-candidate closure evidence;
2. full local check is green;
3. both mandatory Toxiproxy oracle qualifications are complete and green;
4. Eggfetch/language-client/Python-native qualification is green or platform incompleteness is explicitly covered by hosted evidence;
5. bounded fuzz and release/package smoke are green;
6. hosted exact-head CI is green;
7. no material performance regression is accepted without separate corrective work;
8. public/capability census shows no regression;
9. planning-state guard is green on the final reconciled tree;
10. one final M054 closure record identifies the authoritative exact candidate.

## Rejection / stop conditions

Any of the following blocks closure:

- public route/symbol/command removal;
- deterministic replay change not explicitly planned;
- reduced scenario capacity;
- Toxiproxy differential failure;
- cross-language conformance failure;
- unexplained material performance regression;
- stale planning state after M053;
- qualification evidence generated from a different candidate.

## Closure evidence

Create plans/closure/M054-post-m048-corrective-tranche-exact-head-qualification-closure.md containing the full qualification matrix, hosted run, public-surface census, performance disposition, and final verdict.

## Follow-on rule

M054 activates no automatic successor. Scenario enumeration, egress chaining, new fault models, additional language bindings, and downstream EggReplay/EggProbe adapters remain separate feature decisions and must be planned independently.
