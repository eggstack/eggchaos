# M028 — Deterministic Schedule Qualification and Hardening

Status: ready
Depends on: M027
Role: exact-candidate qualification gate for the first richer-scenario tranche

## Objective

Qualify the M026/M027 scenario-v2 compiler and runtime as one reproducible post-release feature tranche on an exact candidate commit. Prove deterministic compilation/replay identity, drift-free deadline scheduling, safe concurrent mutation/cleanup, bounded hostile-input behavior, and no regression to stream, datagram, Toxiproxy, Eggfetch, security, package, or performance contracts.

M028 adds no new schedule language feature unless a qualification defect requires a narrow corrective change.

## Baseline

M026 freezes the source language, compiler, canonical fingerprint, expansion bound, and v2 namespace derivation. M027 wires that IR to the authoritative runtime with strict/live isolation, cleanup, API/CLI, and evidence.

The main qualification risks are control-plane rather than data-plane:

- compiler instability across equivalent source encodings;
- accidental reuse of daemon run_id in deterministic identity;
- scheduler drift under slow event application;
- conflict/cleanup races that overwrite external state;
- cancellation/shutdown leaving tasks or fault state behind;
- repeat expansion causing resource amplification;
- timing evidence being mistaken for exact traffic replay;
- additive API changes breaking ScenarioV1 automation.

## Scope

### In scope

- One exact-candidate qualification SHA.
- Golden schedule compile/fingerprint/namespace corpus.
- Paused-time scheduler timing corpus.
- Strict/live concurrent mutation matrix for stream and datagram resources.
- Success/failure/cancel/shutdown cleanup matrix.
- JSON/TOML equivalence and native DTO/CLI contract fixtures.
- Fuzz/property tests for schedule parsing/expansion/control input.
- Resource/bounds/adversarial tests at the 1024-event ceiling.
- Cross-platform workspace/CLI/runtime CI through the repository's supported host matrix.
- Existing stream and datagram deterministic golden regressions.
- Strict pinned Toxiproxy v2.12 differential regression.
- Eggfetch integration regression.
- Security/dependency/package/release smoke.
- Scenario compiler/dispatcher performance measurements and preservation of existing M024 datagram/no-fault budgets.
- Documentation/planning reconciliation and exact closure evidence.

### Non-goals

- No new schedule syntax after candidate freeze except bug correction.
- No cron/persistent job scheduler.
- No new fault kinds or continuous data-plane interpolation.
- No loosening of compile/run bounds to make a stress test pass.
- No retroactive rewrite of M019 or M023/M024 closure evidence.

## Golden schedule corpus

Commit small canonical fixtures covering at least:

- minimal one-phase stream schedule;
- minimal one-phase datagram schedule;
- mixed stream/datagram phases;
- equal-deadline multiple actions;
- finite repeat expansion;
- strict versus live metadata;
- restore-initial versus leave metadata;
- JSON/TOML semantic equivalence;
- seed sensitivity;
- execution_key sensitivity;
- event/action/offset sensitivity;
- 1024-event exact-bound case;
- malformed/overflow/1025-event rejection.

For each valid case freeze the canonical compiled representation, SHA-256 fingerprint, compiled indices/offsets, and v2 policy namespace vectors. Fixture changes require a documented compiler-semantics version change or bug-fix rationale.

## Scheduler timing corpus

Use Tokio paused time as the primary timing authority.

Required cases:

- 0-offset first event;
- sparse events with long gaps;
- multiple equal-offset events;
- simulated slow publication/application between events;
- deadline already passed before poll;
- cancellation just before/at/after a deadline;
- large but bounded final offset;
- timer arithmetic near checked representation limits.

The proof target is event scheduling semantics, not OS traffic timing. Wall-clock integration tests may use tolerances but cannot be the sole evidence.

## Concurrency and cleanup matrix

Exercise both stream and datagram directions with:

- no external mutation;
- manual mutation before the next strict event;
- manual mutation racing publication;
- another scenario mutation;
- cancellation while sleeping;
- cancellation immediately after publication;
- target deletion/failure;
- service shutdown;
- external mutation before restore cleanup;
- cleanup of multiple touched resources with one conflict.

Assert that strict conflicts fail without overwrite, live mode matches documented current-state semantics, and cleanup never clobbers externally-owned generations.

For stream resources, include a case where old buffered preserving bytes are draining at a scheduled publication. For datagrams, include queued old-generation candidates while newer scheduled plans are published.

## Fuzz/property/bounds gate

Add or extend targets over:

- ScenarioScheduleV2 JSON;
- TOML source parsing if practical in fuzz harness;
- phase/repeat expansion;
- duration arithmetic;
- canonical fingerprint serialization;
- version-aware native apply/validate/compile bodies;
- cleanup/resource ownership transition sequences.

Invariants include:

- expansion never exceeds configured event/nesting bounds;
- no panic or unbounded allocation from hostile counts/durations/names;
- compile output is deterministic for the same semantic input;
- same-deadline order is stable;
- run_id cannot affect v2 namespace derivation;
- cleanup cannot publish over an unexpected generation;
- run/history retention stays bounded;
- no payload appears in schedule/run evidence.

## Performance qualification

This feature must not add per-byte/per-datagram work when no scenario is running. Existing stream and M024 datagram no-fault budgets remain regression gates.

Measure, in a repeatable local/CI-friendly harness where possible:

1. compile/validate a small schedule;
2. compile the 1024-event bound case;
3. dispatch 1024 same-deadline no-op-equivalent policy publications under paused/synthetic time where meaningful;
4. dispatch a representative spread schedule;
5. compare normal proxy no-fault benchmarks before/after to ensure scheduler support did not enter hot paths.

Freeze a compiler/dispatcher regression threshold only if measurement is stable enough to justify one. Do not invent a number before collecting baseline evidence.

## Security and control-plane gate

Verify:

- 1 MiB native request body bound still applies;
- malformed version/format/unknown fields return bounded errors;
- non-loopback admin auth behavior is unchanged;
- TOML/JSON errors do not echo secrets or unbounded source content;
- schedule/phase names and failure messages are bounded;
- no path traversal/file loading exists server-side;
- CLI reads only the explicitly supplied local file and server never accepts a filesystem path as schedule content;
- no shell/callback/plugin surface was introduced.

## Regression gates

On the exact candidate run at minimum:

    ./scripts/check.sh
    cargo audit --deny warnings
    cargo deny check advisories licenses bans sources
    ./scripts/qualify_eggfetch.sh
    TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 ./scripts/qualify_toxiproxy_v2_12.sh
    ./scripts/release-smoke.sh
    ./scripts/benchmark_datagram.sh

Run focused schedule fuzzing/property targets with the repository's documented stable-toolchain cargo-fuzz workaround if still required.

If publishable dependencies changed (for example the fingerprint implementation), rerun package/publish-order and affected artifact smoke evidence.

## Ordered work packages

### WP1 — Freeze candidate and matrix

Name one candidate SHA, tool versions, schedule fixture corpus, host jobs, and all inherited regression gates.

### WP2 — Golden compiler/replay corpus

Run/freeze exact compile representations, fingerprints, and v2 namespace vectors including run_id-independence proof.

### WP3 — Paused-time scheduler qualification

Prove epoch anchoring, equal-deadline order, no cumulative drift, cancellation behavior, and lateness evidence.

### WP4 — Isolation/cleanup race qualification

Run the full strict/live and restore/leave matrix across stream/datagram resources with controlled concurrency.

### WP5 — Fuzz/security/bounds

Run hostile source/control inputs, expansion/property targets, body/auth checks, bounded evidence/history assertions, and dependency security gates.

### WP6 — API/CLI compatibility

Freeze version-aware JSON fixtures, TOML/JSON equivalence, CLI one-document JSON behavior, and ScenarioV1 regression fixtures.

### WP7 — Product regressions/performance

Run workspace, Eggfetch, pinned Toxiproxy, stream/datagram golden traces, M024 performance budget, package/release smoke, and scheduler compile/dispatch measurements.

### WP8 — Documentation/planning reconciliation

Update plans/registry.md, plans/roadmap.md, plans/README.md, AGENTS.md, architecture/scenario-observability.md, architecture/control-plane-cli.md, docs/control-plane.md, docs/architecture.md, examples/help, and verification matrix. Record exact limitations around traffic timing replay.

### WP9 — Closure

Create one closure record with exact candidate, clean/not-clean verdict, all commands/evidence, measured performance notes, incomplete external evidence if any, and follow-on activation.

## Acceptance criteria

M028 closes only when:

- M026 and M027 are closed;
- one exact candidate is used for the declared qualification verdict;
- golden compiled schedules/fingerprints/namespaces pass unchanged;
- v2 replay identity is proven independent of daemon run_id;
- paused-time tests prove one-epoch deadlines and no cumulative drift;
- strict/live conflict semantics and restore/leave cleanup are race-tested for stream and datagram resources;
- cancellation/shutdown leave no untracked task or silent stale restoration;
- 1024-event and hostile expansion bounds hold without panic/unbounded allocation;
- JSON/TOML/API/CLI fixtures pass and ScenarioV1 remains compatible;
- security/dependency gates are clean;
- Eggfetch and strict pinned Toxiproxy regressions pass;
- existing stream/datagram deterministic traces and M024 performance budget remain green;
- scheduler compiler/dispatch performance is measured and any threshold is evidence-based;
- docs state that policy/event replay is deterministic while live connection/datagram arrival timing is not;
- no unresolved medium-or-higher correctness/security finding remains;
- closure evidence identifies the exact candidate and limitations.

Create plans/closure/M028-deterministic-schedule-qualification-and-hardening-closure.md.

## Stop/rejection conditions

Do not close if:

- golden fingerprints/namespaces vary between equivalent runs without a versioned reason;
- event timing relies on wall-clock sleeps as sole proof;
- strict or cleanup races can overwrite external state;
- run_id or Tokio scheduling affects v2 random realization;
- source expansion can exceed bounds or partially execute after validation failure;
- ScenarioV1 automation breaks;
- a required pinned Toxiproxy or Eggfetch regression is skipped without being recorded as incomplete;
- existing data-plane performance budgets are weakened merely because the feature is control-plane oriented;
- documentation implies exact replay of OS/application traffic timing.

## Follow-on activation

A clean M028 closes the first richer deterministic-scenario/time-varying schedule tranche. Later work such as smooth ramp sugar, predicates/branches, lifecycle actions, replay integration with eggreplay, diagnostic orchestration with eggprobe, or persisted cron scheduling requires separate planning and must compile to or compose with this bounded model rather than bypass it.