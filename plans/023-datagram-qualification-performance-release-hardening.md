# M023 — Datagram Qualification, Performance, and Release Hardening

Status: closed
Depends on: M022
Role: exact-candidate qualification gate for the first datagram tranche

## Objective

Qualify M020–M022 as one reproducible post-release UDP/datagram feature tranche on an exact candidate commit, establish the first measured no-fault datagram performance budget, and reconcile all user/planning documentation before declaring the tranche complete.

M023 is not a retroactive v0.1.0 qualification gate. M019 remains the historical first-release authority.

## Baseline

M020 defines deterministic datagram semantics, M021 supplies the fixed-target per-client UDP runtime, and M022 exposes native control/config/CLI/scenario/observability.

The risk profile differs from the established stream subsystem: datagram scheduling can reorder, duplication amplifies bounded state, UDP receive overflow can become host-dependent, per-client socket ownership controls reply isolation, and cross-platform UDP behavior must be tested rather than inferred.

Linux `tc netem` may be used as a semantic/reference comparison for overlapping delay/loss/duplicate/reorder concepts, but eggchaos is a cross-platform user-space datagram engine and must not claim kernel/qdisc equivalence.

## Scope

### In scope

- One exact-candidate qualification SHA.
- Golden deterministic datagram traces.
- Live multi-client fixed-target UDP behavior.
- Cross-platform Ubuntu/macOS/Windows CI.
- IPv4 plus capability-qualified IPv6.
- Fuzz/property/bounds/security evidence.
- No-fault and representative-fault performance measurements.
- First measured datagram performance regression budget.
- Existing TCP/Toxiproxy/Eggfetch regression gates.
- Package/artifact checks for changed publishable crates/binary.
- Documentation/planning/architecture reconciliation.
- Exact closure evidence.

### Non-goals

- No new fault types.
- No correlated loss/distributions.
- No raw-IP/kernel equivalence claim.
- No production use of `tc netem`.
- No weakening of M019 stream qualification gates.
- No arbitrary upstream routing/chains.
- No performance target invented before measurement.

## Evidence layers

### Layer A — pure deterministic engine

Commit a stable corpus of numbered logical datagrams and exact expected outcomes for combinations of:

- delay/jitter;
- loss;
- duplication;
- reorder;
- corruption;
- bandwidth;
- ordered combinations;
- queue exhaustion;
- generation mutation.

The corpus must record seed/RNG version, plan, ingress ordinals/copy indices, emissions/discards, and evidence counters without payload capture beyond small synthetic fixture bytes required to prove corruption.

### Layer B — live UDP runtime

Exercise real sockets with:

- concurrent clients;
- echo;
- multiple responses;
- unsolicited responses;
- upstream and downstream impairments;
- idle expiry;
- capacity/overflow;
- delete/disable/shutdown;
- oversized input;
- IPv4/IPv6 family behavior.

### Layer C — semantic reference

Document where M020 semantics intentionally align with or differ from common network-emulation terminology. An optional Linux `netem` comparison may validate broad phenomena but must not be used to assert exact timing/randomness equivalence.

## Golden corpus requirements

At minimum freeze cases for:

- loss 0.0 and 1.0;
- one intermediate seeded loss trace;
- duplication with stable copy indices;
- duplicate→loss and loss→duplicate distinct traces;
- deterministic corruption positions;
- delay jitter causing overtaking without explicit reorder;
- explicit reorder hold causing overtaking;
- stable equal-deadline ordering;
- bandwidth burst/refill timing for whole datagrams;
- max queued datagram exhaustion;
- max queued byte exhaustion;
- administrative discard distinct from configured loss;
- old/new generation coexistence after live mutation.

Changing an expected trace requires either a bug-fix explanation or a versioned semantic/RNG change, never silent fixture churn.

## Cross-platform matrix

Required hosted/native CI where available:

- Ubuntu x86_64;
- macOS;
- Windows.

Run real UDP runtime tests on each host rather than compiling only. IPv6 tests may capability-skip only when the host genuinely lacks IPv6 loopback; the skip must be visible, not counted as a pass for IPv6 behavior.

Platform differences in socket errors/ICMP behavior must be normalized only where they do not alter the documented eggchaos contract.

## Fuzz/property and boundedness gate

Expand fuzz/property coverage over:

- datagram plan JSON/DTO parsing;
- TOML datagram config;
- queue count/byte accounting;
- deadline/order arithmetic;
- duplication amplification;
- corruption index calculations;
- bandwidth arithmetic;
- generation publication/conflict sequences;
- association snapshot/evidence serialization.

Invariants include:

- queue counters never exceed configured bounds;
- emitted/discarded/queued accounting reconciles with admitted candidates and duplicates;
- preserving plans do not mutate/drop data;
- destructive classes are distinguished;
- no panic from hostile bounded numeric input;
- no unbounded allocation proportional to attacker-controlled counts;
- cancellation drains task ownership.

## Performance qualification

First measure before freezing a budget.

Benchmark in the same session/environment:

1. direct UDP echo baseline without eggchaos;
2. fixed-target eggchaos datagram proxy with empty plan;
3. each individual fault under representative datagram sizes/rates;
4. a realistic combined impairment plan;
5. multi-client association scaling.

Capture at least datagrams/second, payload throughput, median/tail latency where harness reliability permits, CPU usage if readily available, and allocation/queue high-water indicators where useful.

After collecting a repeatable baseline, freeze a defensible empty-plan regression threshold in qualification docs/scripts. Do not invent the threshold in this plan.

If Eggbench has a stable production driver surface by implementation time, prefer delegating experiment orchestration there while keeping eggchaos-specific workloads/fixtures local. Do not block M023 solely to migrate a working bounded benchmark harness.

## Security/reliability gate

Verify:

- public listener safety rules remain explicit;
- admin loopback/auth policy remains unchanged;
- configured datagram/queue/association/history limits are enforced;
- oversized inputs do not truncate into valid traffic;
- no payload contents leak to logs/metrics/history;
- malformed control/config input is bounded;
- no stale association/task remains after kill/delete/shutdown;
- no unexpected Eggress dependency graph expansion.

## Regression gates

The datagram tranche must not weaken the stream product. On the frozen candidate run:

```sh
./scripts/check.sh
cargo audit --deny warnings
cargo deny check advisories licenses bans sources
./scripts/qualify_eggfetch.sh
TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 ./scripts/qualify_toxiproxy_v2_12.sh
./scripts/release-smoke.sh
```

Add/extend dedicated datagram qualification and benchmark scripts as needed. Fuzz execution must follow the repository's established stable-toolchain workaround documented by M019 if still necessary.

If publishable crate dependencies changed, rerun package/publish-order proof and artifact builds for the supported target matrix. Build-only versus runtime-smoked status must be recorded truthfully.

## Ordered work packages

### WP1 — Freeze exact candidate and qualification matrix

Name the candidate SHA, required host jobs, tool versions, datagram fixture corpus, and expected regression gates. Do not combine evidence from moving SHAs without explicit classification.

### WP2 — Golden deterministic corpus

Run/freeze M020 pure-engine traces and verify seed/generation/evidence replay.

### WP3 — Live runtime qualification

Run M021 multi-client, multi-response, unsolicited-response, lifecycle, queue/capacity, and oversized-input tests against real sockets.

### WP4 — Cross-platform network matrix

Run datagram tests on Ubuntu/macOS/Windows and record IPv6 capability/results separately.

### WP5 — Fuzz/security/bounds

Run expanded targets, hostile config/control cases, dependency/security checks, and resource-reconciliation tests.

### WP6 — Performance baseline and budget

Measure direct versus empty-plan plus representative faults, freeze the first justified budget, and make regressions visible in scripts/docs.

### WP7 — Existing product regressions and artifacts

Run full workspace, Eggfetch, strict pinned Toxiproxy, package/release smoke, and affected artifact lanes on the same candidate.

### WP8 — Documentation/planning reconciliation

Update:

- `plans/registry.md`;
- `plans/roadmap.md`;
- `plans/README.md`;
- `AGENTS.md`;
- architecture deep dives;
- `docs/architecture.md`;
- `docs/configuration.md`;
- `docs/control-plane.md`;
- CLI examples/help references;
- verification matrix with datagram coverage and measured performance budget.

Create one closure note with a clean/not-clean verdict.

## Acceptance criteria

M023 closes only when:

- M020–M022 are closed;
- one exact candidate SHA is frozen;
- deterministic golden traces pass unchanged;
- real multi-client UDP runtime evidence proves no cross-delivery and correct multiple/unsolicited response ownership;
- queue/association/resource bounds reconcile under stress;
- Ubuntu/macOS/Windows datagram jobs are green;
- IPv6 evidence is present or explicitly capability-unavailable per host;
- fuzz/property/security gates are clean;
- a measured no-fault datagram performance budget is frozen and the candidate satisfies it;
- Eggfetch and strict pinned Toxiproxy regressions pass;
- release/package/artifact checks for affected crates pass;
- docs accurately distinguish UDP datagram impairment from lower-layer packet/qdisc behavior;
- no unresolved medium-or-higher correctness/security/release finding remains;
- closure evidence identifies the candidate and all limitations.

Create `plans/closure/M023-datagram-qualification-performance-release-hardening-closure.md`.

## Stop/rejection conditions

Do not close if:

- deterministic traces depend on Tokio scheduling or wall-clock coincidence;
- a multi-client test can cross-route responses;
- a required host only compiles instead of running UDP tests without explicit limitation;
- queue overflow or administrative discard is counted as configured loss;
- oversized datagrams can be silently truncated into accepted traffic;
- a fuzz crash or resource leak is waived;
- a performance budget is chosen before measuring a baseline;
- stream/Toxiproxy/Eggfetch regressions are skipped because the change is "UDP only";
- kernel `netem` behavior is presented as exact eggchaos equivalence;
- release evidence comes from different moving commits without justification.

## Follow-on activation

A clean M023 closes the initial UDP/datagram roadmap tranche.

Further work such as correlated/Gilbert-Elliott loss, non-uniform delay distributions, NAT rebinding, MTU/path-MTU/ICMP simulation, multicast/broadcast, raw-IP/lower-layer impairment, datagram embedding adapters, or upstream proxy chains requires separate planning and, where semantics change, a new/amended ADR.
