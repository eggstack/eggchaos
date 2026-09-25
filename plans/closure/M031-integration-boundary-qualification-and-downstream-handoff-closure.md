# M031 — Integration Boundary Qualification and Downstream Handoff — Closure

Status: closed
Exact qualification candidate: `fa189b916761594b3d863a33e314a6799cf07b64`
Depends on: M030 (closed at `0bc45f0`), M029 (closed at `add40b0`)

## Objective verdict

M031 qualifies M029/M030 as one stable cross-project integration
substrate on the exact candidate above. Arbitrary-inner-Dialer
composition without route duplication, deterministic physical
connection identity and evidence, the reusable Scenario V2 experiment
harness with shared-epoch semantics, and downstream adoption without
eggchaos depending on consumer product models are all proven below.
No EggReplay/EggProbe product feature was added. No stop condition
fired; the tranche closes clean and hands downstream planning its
closure-backed seams.

## WP1 — Candidate and public API inventory

Candidate SHA: `fa189b916761594b3d863a33e314a6799cf07b64` (clean
tree at gate time). Toolchain: pinned rustc/cargo 1.89.0.
Crate versions: all workspace crates `0.1.0`. Feature flags
unchanged (`eggchaos-eggfetch/http2` only). Host: macOS arm64 local;
hosted CI covers the normal OS matrix as an owner release-step action.

New/changed public surface since M028:

- `eggchaos-core`: `StreamEvidence` gains delay/segment/buffered/
  termination mirroring + `snapshot()`; new `DirectionEvidenceSnapshot`,
  `BidirectionalEvidenceSnapshot`, `LiveBidirectionalEvidence`;
  `BidirectionalChaosStream` gains connection-key, observed/pending
  generation, live-evidence, snapshot, and inner-stream accessors.
- `eggchaos-eggfetch`: generic `ChaosDialer<D = DirectDialer>` plus
  `DirectDialer`, `ConnectionKeyContext`, `ConnectionKeyProvider`,
  `DefaultConnectionKeyProvider`, `KeyError`, `ConnectionReport`,
  `ConnectionObserver`, `RecordingObserver`, `ConnectionRecord`,
  `ChaosConfigError`, and bound constants. `ChaosDialer::new` /
  `with_policies` remain source-compatible (existing
  `tests/regression.rs` unmodified and green).
- `eggchaos-experiment` (new): full Scenario V2 semantic authority
  plus `PolicyTarget`, `TargetError`, `TargetCapabilities`,
  `TargetPlan`, `TargetSnapshot`, `PublishReceipt`, `EventSink`,
  `drive`, `prepare_initial`, `EpochGate`/`EpochWaiter`,
  `PreparedExperiment`, `ExperimentError`/`ExperimentEvidence`/
  `ExperimentOutcome`, `StreamPolicyTarget`.
- `eggchaos-server`: `ScenarioAction` and all Scenario V2 pure
  symbols re-exported unchanged (`scenario_v2` compat module);
  `ControlState` behavior and routes unchanged.

Dependency graph (verified via `cargo tree` + `Cargo.lock` +
release order proof): `core -> experiment/eggfetch ->
server/toxiproxy/cli`; no `eggreplay-*` or `eggprobe-*` dependency
anywhere; the experiment crate sees only core + serde/sha2/
thiserror/tokio/tokio-util.

## WP2/WP4 — Dialer, HTTP/TLS, and harness matrices

- Eggfetch lib 20 tests + integration 10 + correlation 1 + overhead
  3: direct/inner/routed/duplex fixtures, all five `DialErrorKind`s
  preserved, exactly one inner attempt, H1 keep-alive reuse (one
  key), forced separate H1 (distinct keys), H2 multiplexing (one
  key), TLS handshake under impairment, live policy engagement,
  cancellation, evidence observer enabled/disabled.
- Experiment 23 tests: paused-time 1s/2s/3s deadlines with zero drift
  and compiled-order preservation, shared-epoch proofs, prepare
  purity, strict/live/cleanup conformance, prompt cancellation at all
  lifecycle points, unsupported-capability and bounds behavior.
- Server conformance 3 tests: `ControlState` adapter and
  `StreamPolicyTarget` produce equivalent event/generation outcomes
  for the stream subset; datagram diverges explicitly (server
  applies, stream target fails at prepare with nothing published).

## WP3 — Identity and evidence corpus

- Default ordinal sequence `1, 2, …` over successful dials only;
  failed dials consume no ordinal and create no evidence.
- Caller integration-identity sensitivity (derivation input +
  report/evidence field), explicit constant-provider collision
  semantics, determinism across repeated inputs, no dependence on
  wall clock, UUIDs, task scheduling, or logical request IDs.
- Live/final evidence verified against controlled fixtures:
  empty-plan byte counts both directions, generation/transition
  tracking, bounded active-fault lists with explicit truncation,
  preserving/loss byte reconciliation, directional delay totals,
  graceful termination with fault identity, exactly-once observer
  delivery, post-drop handle readability, payload-free snapshots,
  and oldest-first eviction in the bounded collector.

## WP5 — Cross-layer correlation fixture

`crates/eggchaos-eggfetch/tests/correlation.rs` demonstrates the
intended downstream contract end to end: custom workload → EggFetch
→ route-authoritative inner Dialer → M029 chaos adapter → local
service, with a Scenario V2 schedule driving live publications from
the same shared epoch. Final assertion correlates schedule
fingerprint (`seed 2026`, `execution_key 31`), event generations,
physical connection key, active fault evidence (`inject`), and the
workload-observed `ok` bodies — proving correlation, not exact
kernel/network timing replay.

## WP6 — Performance, security, dependency gates

Measured on the candidate (macOS arm64, in-memory duplex harness):

- Empty-plan adapter throughput at/above the same-topology bare
  baseline (`plain_ratio 1.384`, `observed_ratio 1.298`; functional
  bound `> 0.25`).
- Wrap latency 200 dials: plain `1.1 ms`, observer-enabled
  `0.9 ms` (bound: `< 4× + 500 ms`).
- Compile/prepare/start: small schedule `~0.2 ms`; ceiling 1024-event
  schedule prepare `~5.2 ms` with all 1024 events applied (bounds
  5 s / 10 s).
- No new numerical budget is frozen; existing M008/M023/M024
  budgets all still pass (`benchmark_datagram.sh`: both budgets
  `pass`; release benchmarks unchanged).

Security/robustness:

- `DialTarget` carries host/port only by construction; observer
  reports and evidence retain no credentials, headers, URLs,
  payloads, or consumer blobs.
- Provider failure is a bounded typed dial error; provider panic
  propagates before Eggfetch handoff with no evidence recorded
  (tested; no recovery inside networking internals, as specified).
- Integration/target labels bounded (128/256 bytes, tested);
  unsupported capabilities are typed failures; cancellation leaks
  no tasks or retained ownership.
- `cargo audit --deny warnings` clean; `cargo deny check
  advisories licenses bans sources` clean.

## WP7 — Existing product regressions (exact candidate)

- `./scripts/check.sh` — exit 0 (18 `test result: ok` suites, zero
  failures): fmt, workspace clippy `-D warnings`, all workspace
  tests (core 57 + golden, eggfetch 20+10+1+3, experiment 23,
  server 141 + corpus, toxiproxy 10, cli 4), doc.
- `TOXIPROXY_SERVER=… EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1
  ./scripts/qualify_toxiproxy_v2_12.sh` — `translation:pass`,
  `oracle:toxiproxy-server 2.12.0 (checksum verified)`,
  `differential:pass` (50 observations, 0 failed).
- `./scripts/qualify_eggfetch.sh` — green.
- `./scripts/release-smoke.sh` — exit 0, `artifact_smoke:pass`,
  `order_proof:pass` (`core->experiment/eggfetch->
  server/toxiproxy/cli`).
- `./scripts/benchmark_datagram.sh` — both budgets `pass`.
- Scenario V2 golden corpus and stream/datagram exact traces green.

## WP8 — Documentation/planning reconciliation

Updated to the qualified behavior: `README.md` (composed dialer +
experiment harness), `docs/architecture.md` (dependency direction +
new authorities), `docs/eggfetch.md` (composable model, identity,
evidence), `architecture/overview.md` (workspace map + crate roles),
`architecture/eggfetch-integration.md` (§2–§4, §7–§8),
`architecture/scenario-observability.md` (§1.8–§1.10, Owner),
`architecture/verification-qualification.md` (eggfetch + new
integration-boundary sections), `AGENTS.md` (layout, publish order,
planning state), `plans/roadmap.md` (§11G, §14 direction),
`plans/registry.md`, and plan status headers.

## Downstream handoff contract

Stable, closure-backed eggchaos seams only — no aspirational APIs,
no modifications to downstream repositories.

### For EggReplay

| Stable seam | Location | Notes |
| --- | --- | --- |
| Arbitrary-inner-Dialer wrapper | `eggchaos-eggfetch::ChaosDialer<D>` | Route stays EggFetch/caller-owned; exactly one inner attempt; errors pass through |
| Caller-controlled physical connection identity | `ConnectionKeyProvider`, `ConnectionKeyContext`, `DefaultConnectionKeyProvider` | Deterministic over `(ordinal, DialTarget, integration_id)`; collisions only when caller-selected |
| Evidence observer/snapshots | `ConnectionObserver`, `RecordingObserver`, `LiveBidirectionalEvidence` | Bounded, payload-free, readable after drop |
| Scenario V2 compile/fingerprint identity | `eggchaos-experiment::{compile_schedule, compiled_fingerprint, fingerprint_hex}` | Frozen semantics v1; golden corpus |
| Prepared shared-epoch experiment start | `PreparedExperiment`, `EpochGate`/`EpochWaiter` | Schedule-clock sync only |
| Stream `LivePolicy` target adapter | `StreamPolicyTarget` | Expected-generation publications reach pooled connections |
| Pooling/multiplexing caveat | docs + M029/M031 evidence | One key per physical connection; per-request chaos is out of scope |
| `.eggr`/semantic timing ownership | — | Remains EggReplay-owned; eggchaos asserts nothing about it |

### For EggProbe

| Stable seam | Location | Notes |
| --- | --- | --- |
| Route-versus-impairment orthogonality | ADR 005 + M029 evidence | Impairment decorates; never routes |
| Arbitrary-inner-Dialer wrapper for HTTP/TLS paths | `ChaosDialer<D>` | Initial applicability: transport-bearing TLS/HTTP paths |
| Transport evidence correlation | `LiveBidirectionalEvidence`, observer reports | Bounded transport-level counters + identities |
| Scenario V2 experiment identity/start | `eggchaos-experiment` | Same seams as EggReplay rows above |
| Stream-only initial applicability | `StreamPolicyTarget` | Datagram actions fail explicitly |
| Unsupported status | — | DNS/route/ICMP/traceroute/PMTU and any not-yet-adopted UDP path are explicitly unsupported; no translation |
| Probe/report schema ownership | — | Remains EggProbe-owned; no probe/report type enters eggchaos |

Explicit non-guarantees: no cross-process clock synchronization; no
per-request fault isolation on pooled/multiplexed connections; no
promise that hard reset maps identically through every inner
transport; live traffic timing is observational, only
policy/event/schedule identity is deterministic.

Remaining downstream work (separate repositories/plans, may begin
now): EggReplay transport-chaos regression integration is the
preferred first adopter; EggProbe controlled impairment follows
against its stable diagnostic/native work, initially for
transport-bearing TLS/HTTP paths and only later for UDP.

## Acceptance check

All M031 acceptance criteria hold: M029/M030 closed; one exact
candidate; ADR 005 dependency direction; composition without route
duplication; deterministic pooling-aware identity; bounded correct
payload-free evidence; unchanged golden fingerprints/namespaces;
paused-time embedded harness contract; server/embedded conformance;
explicit unsupported failures; clean cancellation/cleanup;
consumer-neutral cross-layer correlation; measured overhead with no
material regression; clean security/dependency/package gates;
EggFetch + pinned Toxiproxy + stream/datagram regressions green;
honest timing documentation; stable handoff seams without touching
downstream repositories; no unresolved medium-or-higher findings.

## Follow-on activation

A clean M031 closes the eggchaos integration-boundary/harness
tranche (`M029 -> M030 -> M031`). Downstream repositories may now
register implementation milestones against these closure-backed
seams.
