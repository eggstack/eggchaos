# Eggchaos Long-Term Roadmap

Status: M008 and M009–M039 are historical closed work. M019 remains the final pre-tag authority at `ca527db`. ADR 003 datagram, ADR 004 scenario, ADR 005 integration, and ADR 006 cross-language tranches are complete. ADR 007's M036–M039 implementation chain landed, and M040 is the active corrective requalification handoff for bounded compatibility/qualification/closure defects. Strict Toxiproxy v2.12 remains frozen/default.

## 1. Mission

Eggchaos is a Rust-native network fault-injection toolkit built around a small reusable stream impairment engine.

The project should cover the practical core use case that made Toxiproxy useful—put a fixed-target proxy in front of a dependency and change network conditions during tests—while also being useful without a standalone proxy. The reusable core should embed directly into Rust test harnesses and, through `eggfetch_core::Dialer`, into Eggstack HTTP clients.

The success criterion is not feature-count parity with a general proxy. It is precise, deterministic, diagnosable fault semantics with low no-fault overhead and clean composition with existing Eggstack networking crates.

## 2. Architectural shape

The intended dependency direction is:

```text
                         +----------------------+
                         | eggchaos-toxiproxy   |
                         | v2.12 REST adapter   |
                         +----------+-----------+
                                    |
+----------------+        +---------v---------+        +-------------------+
| eggchaos-cli  |------->| eggchaos-server  |<-------| native admin API  |
| eggfetch HTTP |        | runtime/registry  |        | eggserve-server   |
+----------------+        +---------+---------+        +-------------------+
                                    |
                           +--------v--------+
                           | eggchaos-core   |
                           | fault engine    |
                           +--------+--------+
                                    |
                         wraps destination writes
                                    |
                           +--------v--------+
                           | eggress-relay   |
                           | relay authority |
                           +-----------------+

+--------------------+
| eggchaos-eggfetch  |---- implements eggfetch_core::Dialer
+---------+----------+
          |
          +------------------> eggchaos-core
```

The fault engine itself must not depend on HTTP, CLI parsing, Toxiproxy vocabulary, or a listener runtime.

## 3. Direction model

Eggchaos uses the same directional vocabulary expected by Toxiproxy clients:

- upstream: client -> target;
- downstream: target -> client.

The preferred transport composition is write-side impairment:

- wrap the upstream destination stream's `AsyncWrite` behavior with the upstream fault plan;
- wrap the client destination stream's `AsyncWrite` behavior with the downstream fault plan;
- leave reads as transparent pass-through unless a future fault explicitly requires read-side semantics;
- pass the two full-duplex wrapped streams to `eggress_relay::relay_with_options`.

This keeps half-close and bidirectional copy ownership in Eggress while giving eggchaos a single directional impairment authority.

## 4. Fault semantics roadmap

### Release baseline faults

The first release should support:

- latency with optional symmetric jitter;
- bandwidth throttling with an explicit burst policy;
- timeout/blackhole;
- byte limit / truncate-after-N;
- slow close;
- slicing/chunking with optional per-slice delay;
- graceful disconnect;
- best-effort hard reset where the concrete transport supports it;
- per-fault probability (“toxicity”) selected deterministically per connection.

Toxiproxy v2.12.0 compatibility maps its seven toxics—latency, bandwidth, slow_close, timeout, reset_peer, slicer, and limit_data—onto these native primitives.

### Post-release faults

ADR 007 activates deterministic userspace stream loss as the next native stream
fault. The upstream motivation is Shopify Toxiproxy `packet_loss`, introduced
after v2.12.0 and present in the researched snapshot
`40f7fd31bee529d824116bd2a11a9e3425e904ec`.

Native semantics deliberately use the name `stream-loss`. The first version
uses a fixed 32 KiB logical grain keyed to absolute accepted stream byte
position, deterministic fault-local RNG, and a simplified burst-correlation
rule compatible in intent with upstream `loss_rate` + `correlation`.
Caller write/Tokio poll boundaries are not loss boundaries.

Execution order is:

`M036 core/evidence -> M037 native/cross-language propagation -> M038 pinned Toxiproxy snapshot profile -> M039 exact-candidate qualification`.

Strict Toxiproxy v2.12 remains a separate frozen/default compatibility profile.
The post-v2.12 adapter spelling `packet_loss` is opt-in and pinned; it is not a
claim against moving upstream `main`.

Real packet loss/reordering belongs in a datagram or lower-layer impairment subsystem. The user-space UDP/datagram line is implemented under ADR 003 and M020–M023; lower-layer IP/qdisc phenomena remain out of scope.

## 5. Determinism

Reproduction is a product requirement.

The engine must define a versioned RNG algorithm and seed-derivation contract. Do not use one global RNG whose draw order depends on Tokio scheduling.

Recommended v1 design:

```text
run_seed
  + stable proxy identity
  + accepted connection ordinal / explicit embed connection key
  + direction
  + stable fault identity
  -> versioned deterministic sub-seed
```

A small explicitly specified non-cryptographic algorithm such as SplitMix64-v1 is acceptable if golden vectors are committed and the version is included in diagnostic/replay metadata. The algorithm must never be described as security-sensitive randomness.

A connection evidence record should be sufficient to replay every probabilistic decision made for that connection.

## 6. Buffering and backpressure

Latency cannot be implemented as “sleep before reading the next bytes” because that couples latency to throughput. The engine needs a bounded timestamped queue so multiple chunks can be in flight while waiting for their release time.

Every buffering fault must define:

- maximum buffered bytes and/or segments;
- behavior on reaching the bound;
- whether pressure is propagated by returning `Pending`;
- flush semantics;
- shutdown semantics;
- mutation behavior while buffered data exists.

The default overflow policy is backpressure, never unbounded allocation and never silent byte loss unless the configured fault explicitly requests loss/corruption.

## 7. Live mutation

Native semantics should be generation-based and explicit.

Phase 1 may snapshot a plan when a connection is accepted. M005 then adds safe active-connection mutation. Structural plan changes must transition at a well-defined boundary after already accepted bytes are either delivered or deliberately discarded by a fault that documents discard semantics.

Configuration updates must never accidentally lose in-flight data merely because a fault was edited.

Toxiproxy compatibility is layered on this mechanism. Toxiproxy's implementation interrupts and replaces toxic pipelines, with explicit guidance that in-flight data must be flushed during interruption. Eggchaos should preserve the externally important property—no accidental stream corruption during an administrative update—without copying the Go channel architecture.

## 8. Admin/control plane

Native API is versioned under `/v1`. Initial route family:

```text
GET    /v1/health
GET    /v1/version
GET    /v1/proxies
POST   /v1/proxies
GET    /v1/proxies/{name}
PATCH  /v1/proxies/{name}
DELETE /v1/proxies/{name}
GET    /v1/proxies/{name}/faults
POST   /v1/proxies/{name}/faults
GET    /v1/proxies/{name}/faults/{id}
PATCH  /v1/proxies/{name}/faults/{id}
DELETE /v1/proxies/{name}/faults/{id}
GET    /v1/connections
GET    /v1/connections/{id}
DELETE /v1/connections/{id}
POST   /v1/reset
POST   /v1/scenarios/apply
GET    /metrics
```

Use `eggserve-server` and `eggserve-primitives` as the generic H1 runtime. Do not depend on `eggress-admin`; that crate intentionally owns Eggress-specific routing, UDP, reverse-server, and metrics state.

Admin binds to loopback by default. A non-loopback bind requires an explicit public-admin opt-in and authenticated requests. Compatibility mode may expose Toxiproxy's unauthenticated API only on loopback by default.

## 9. CLI and configuration

The CLI is a thin adapter. It should never implement a second networking or state-management path.

Illustrative native commands:

```text
eggchaos serve --config eggchaos.toml
eggchaos proxy list --json
eggchaos proxy add redis --listen 127.0.0.1:0 --upstream 127.0.0.1:6379
eggchaos fault add redis latency --direction downstream --delay 1s --jitter 100ms
eggchaos fault set redis latency_downstream --delay 500ms
eggchaos fault remove redis latency_downstream
eggchaos connection list --json
eggchaos connection kill <id>
eggchaos reset
```

The control client should use `eggfetch-core` with a minimal H1 feature profile. Human tables are presentation; JSON is the stable automation surface.

Configuration starts at schema version 1 and should be TOML for native service configuration. The control API remains JSON.

## 10. Eggstack reuse roadmap

### Required for MVP

`eggress-relay`
: canonical bidirectional copying, byte accounting, directional error reporting, and half-close policy.

`eggress-testkit`
: dev/test echo server, half-close server, fragmentation/slow-I/O fixtures where directly reusable.

`eggserve-server` + `eggserve-primitives`
: generic H1 admin API runtime and lifecycle.

`eggfetch-core`
: CLI HTTP control client and later `Dialer` adapter.

### Optional after MVP

`eggress-outbound`
: listener-free outbound proxy-chain connector. Add only behind an optional feature after the direct fixed-target path is proven.

Avoid using `eggress-embed` as a convenience umbrella because eggchaos does not need the full protocol/routing/runtime graph.

## 11. Milestone sequence

### M001 — workspace/bootstrap and public contracts

Create the Rust workspace, crate topology, error/config/domain types, CI, dependency policy, no-fault pass-through, and plan/architecture documentation alignment.

Exit: core and server skeletons compile; the empty fault path has a deterministic unit test and public types are documented.

### M002 — deterministic directional fault engine

Implement the core fault pipeline and release-baseline impairment primitives, bounded buffering, deterministic RNG, paused-time tests, and byte-conservation properties.

Exit: core can wrap arbitrary Tokio streams without a listener and every fault has deterministic evidence.

### M003 — fixed-target TCP runtime

Build the standalone listener/service around fixed upstreams, `eggress-relay`, connection lifecycle, limits, graceful shutdown, reset capability handling, and embeddable runtime handles.

Exit: multiple proxies can run concurrently; half-close/error/limit behavior is evidenced.

### M004 — native control plane, CLI, and config

Add versioned H1 admin API via EggServe, TOML schema v1, JSON-first CLI via Eggfetch, secure bind defaults, atomic configuration registry, and lifecycle operations.

Exit: operators and tests can create/update/remove proxies/faults without editing process state directly.

### M005 — live mutation, observability, and scenarios

Add generation-aware active-connection updates, connection inspection/control, Prometheus metrics, event/evidence records, deterministic replay metadata, and a minimal scenario application model.

Exit: live updates do not corrupt unaffected bytes; a failed CI scenario is diagnosable and reproducible.

### M006 — Toxiproxy v2.12.0 compatibility

Implement compatibility routes and field mappings, exact v2.12 toxic set, populate/reset/version behavior, client-library smoke tests, and differential oracle cases.

Exit: supported Toxiproxy clients can drive eggchaos for the declared subset with documented intentional differences.

### M007 — eggfetch in-process integration

Implement an `eggfetch_core::Dialer` adapter that returns fault-wrapped physical streams while leaving HTTP/TLS/SNI semantics owned by Eggfetch.

Exit: H1 and H2 resilience tests can inject faults without a standalone listener; pooled physical connections observe supported live updates.

### M008 — qualification and first release

Cross-platform CI, fuzz/property suites, benchmark/regression budgets, security review, package metadata, binaries/checksums/installer path, docs, and closure census.

Exit: first release candidate has reproducible evidence and no planning/documentation state claims unsupported closure.

## 11A. Completed corrective sequence discovered during pre-release audit

A later implementation audit after M001–M007 found that several historical milestone closures overstated behavioral completeness. The architecture remained valid and the bounded M009–M013 corrective sequence subsequently completed, followed by M008 release qualification.

Completed graph (historical):

```text
M009 core fault semantics ──┐
                            ├─> M011 live state/scenario/observability
M010 runtime/control ───────┘
                                  |
                                  v
                         M012 Toxiproxy v2.12 parity
                                  |
                                  v
                         M013 corrective requalification
                                  |
                                  v
                              M008 resumes
```

M009 repairs release-baseline stream semantics: bounded asynchronous latency buffering, token-bucket bandwidth/burst behavior, slicer variation/delay, finite/indefinite blackhole behavior, exact limit-data termination, disconnect execution, and durable termination signaling.

M010 makes listener/task runtime state authoritative for native proxy/fault CRUD, reset, CLI operations, cancellation, cleanup, and concrete TCP termination/reset handling.

M011 reconciles canonical and live policy state, atomically publishes generation + seed namespace, makes scenario seeds effective, structurally owns scenario runs, expands connection evidence/metrics, and enforces bounded history.

M012 completes the declared Toxiproxy v2.12 route/default/populate/reset/toxic surface and replaces partial smoke evidence with a pinned-oracle differential corpus.

M013 reran the corrected stack on one exact commit, including Eggfetch regression qualification, and M008 then completed release/package/target/performance qualification.

Historical closure records are retained and should not be rewritten. Dependency/test/workflow fixes that landed after the M008 candidate were reconciled by the closed M014 and requalified by the closed M015 at `cd88b22`.

## 11B. Post-M015 corrective/hardening sequence

A 2026-09-23 repository audit after M015 found additional issues that warrant a bounded successor chain before tagging. This does not invalidate M015's historical evidence; it means the release-relevant tree will change and must be qualified again.

```text
M016 correctness + secure control
  -> M017 native contract + operator surface
  -> M018 runtime modularization + dependency hygiene
  -> M019 expanded qualification + final exact-HEAD gate
```

M016 repairs concrete release blockers: panic-free user configuration validation, authenticated CLI access to secured admin endpoints, secret redaction, and reliable native identifier path round trips.

M017 makes the native `/v1` wire contract explicit rather than coupling CLI/API behavior to internal Rust Serde layout, consolidates native fault input conversion, exposes important existing runtime bounds through schema-v1 configuration, and adds CLI access to existing scenario/history/metrics operations.

M018 is maintenance-only structural hardening: preserve one `RuntimeInner`/`ControlState` authority while decomposing the oversized server runtime into cohesive modules and removing unused direct dependencies.

M019 is the final pre-tag authority. It makes the pinned Toxiproxy oracle mandatory in release mode, expands fuzz/differential evidence, and reruns Eggfetch/security/package/artifact/performance gates. It closed on exact candidate `ca527db`; no further corrective milestone is blocked.

## 11C. Activated post-release UDP/datagram sequence

ADR 003 activates a separate datagram impairment subsystem rather than extending stream `FaultKind`/`DirectionEngine` with UDP-specific branches.

```text
M020 deterministic datagram fault engine (closed at 56c8925)
  -> M021 fixed-target UDP runtime + per-client associations (closed at 686838b)
  -> M022 native control/config/CLI/scenarios/observability (closed at 8c4e3fb)
  -> M023 cross-platform qualification + measured performance budget (closed at ae2ab73)
```

M020 defines ordered whole-datagram delay/jitter, loss, duplication, reorder-by-hold, payload corruption, and bandwidth semantics with per-datagram probability, domain-separated deterministic RNG, bounded deadline scheduling, explicit drop-newest overflow, and admission-time generation snapshots.

M021 adds a fixed-target UDP listener whose client `SocketAddr` maps to a bounded association with its own connected upstream UDP socket. That model is required to prevent reply cross-delivery and to support multiple or unsolicited target responses. It must audit the exact published Eggress seam/version before choosing production reuse; routing/SOCKS-heavy UDP APIs are not imported merely for convenience.

M022 (closed at `8c4e3fb`) adds sibling native resources under `/v1/datagram-proxies` and `/v1/datagram-associations`, plus TOML, CLI, explicit datagram scenario actions, metrics, and bounded evidence. Toxiproxy v2.12 remains TCP/stream-only.

M023 (closed on `ae2ab733b2be199d7693e40cdc558df01ee9a9de`) froze exact deterministic traces, ran real multi-client UDP tests on supported host OSes, expanded fuzz/bounds/security evidence, measured and froze the first no-fault datagram performance regression budget, and reran existing stream/Toxiproxy/Eggfetch release regressions. Closure evidence is in `plans/closure/M023-datagram-qualification-performance-release-hardening-closure.md`.

This tranche is post-release work. It does not rewrite M019 closure evidence and is not part of the historical v0.1.0 qualification gate.

## 11D. Completed datagram performance/runtime-maintainability pass

M024 (closed at `ca46801`) was a maintenance successor to the completed ADR
003 feature tranche. It added no new datagram semantics.

Its first job is measurement correction: the M023 direct-UDP comparison is retained as an end-to-end regression gate, but a fixed-target proxy necessarily adds another socket hop. M024 therefore adds a benchmark-local bare fixed-target relay with the same socket topology, separates sequential RTT from windowed throughput, profiles before changing production code, and freezes any new topology-matched budget only from measured evidence.

The implementation then targets only demonstrated avoidable cost. The known candidates are the current full-queue scan/sort scheduler, immediate empty-plan scheduling, candidate allocation/synchronization, and association creation that currently holds the association registry lock while awaiting UDP socket setup. Exact ADR 003 traces, queue bounds, evidence classes, generation behavior, per-client connected upstream sockets, and native contracts must remain unchanged.

M024 also decomposed the large datagram runtime into cohesive private modules while retaining one `DatagramRuntime` authority and the existing `ControlState` integration. It reran the complete M023 regression surface and recorded before/after raw performance artifacts before closure.

## 11E. Completed datagram association setup/closure hygiene

M025 (closed at `55911f6`) was a narrow concurrency and planning-hygiene
successor to M024. It added no new datagram semantics.

M024 removed the association-registry lock from UDP bind/connect by introducing
an explicit `Starting` slot. M025 replaced bounded `yield_now()` polling with a
retained, versioned Tokio `watch` transition. Waiters subscribe while holding
the registry lock; publication, setup failure, and administrative drain publish
their terminal state under that same lock, and `wait_for` observes retained
state if a transition wins before suspension. Explicit reservation identity
and idempotent global/per-proxy capacity leases prevent stale setup owners from
publishing over or releasing a successor. Worker cleanup and administrative
drain remain cancellation-safe, and the registry lock is never held across UDP
setup or worker joins.

The pass reconciled stale planning language after M024 closure. It preserved
M024 topology-matched performance budgets, ADR 003 golden traces, per-client
connected upstream sockets, native contracts, and the one `DatagramRuntime`
authority. M025 activated no successor; richer datagram semantics still require
separate planning and an ADR where applicable.

## 11F. Activated richer deterministic-scenario and schedule sequence

ADR 004 activates the next post-release semantic tranche above the already-proven stream/datagram publication machinery:

    M026 deterministic scenario schedule model + compiler
      -> M027 schedule runtime/control/lifecycle
      -> M028 exact-candidate schedule qualification/hardening

M026 is closed. It defines a bounded ScenarioScheduleV2 source language with named phases, finite repetition, strict/live isolation metadata, restore/leave cleanup metadata, deterministic expansion into an inspectable event tape, a canonical SHA-256 schedule fingerprint, and a v2 seed namespace independent of daemon run_id. The initial compiled-event ceiling remains 1024. ScenarioV1 stays supported.

M027 is closed. It executes compiled schedules through the existing owned scenario supervisor and ControlState publication authority. V2 timing is anchored to one Tokio monotonic epoch with absolute sleep_until deadlines so event-application latency cannot accumulate into later deadlines. Strict mode detects external generation movement; restore-initial cleanup uses generation guards and never clobbers state the schedule no longer owns. Stream and datagram actions remain transport-explicit.

M028 is closed. It froze golden compiler/fingerprint/namespace fixtures, proved deadline behavior with paused Tokio time, exercised strict/live and cleanup races across stream/datagram resources, fuzzed bounded schedule expansion/control input, and reran Eggfetch, strict pinned Toxiproxy, security/package, deterministic trace, and existing performance gates on one exact candidate. See its closure record for the candidate SHA, measured compiler/dispatcher performance, and follow-on activation.

The v2 scheduler remains an intra-run deterministic control system. It does not add cron/calendar persistence, arbitrary branches/predicates, callbacks/shell execution, continuous per-packet clock interpolation, or a second data-plane scheduler. Later eggreplay/eggprobe integration should consume this compiled/evidence model rather than bypassing it.

## 11G. Activated cross-project integration-boundary and experiment-harness sequence

ADR 005 activates a consumer-neutral integration tranche before any EggReplay or EggProbe product adapter is implemented:

    M029 composable transport chaos adapter + evidence
      -> M030 consumer-neutral experiment harness + coordinated start
      -> M031 exact-candidate qualification + downstream handoff

M029 is closed (`add40b0`). It refactored the EggFetch physical-stream integration so eggchaos can decorate an arbitrary caller-selected Dialer rather than owning direct resolution/routing, added caller-controlled deterministic physical connection identity, and exposed bounded bidirectional connection evidence outside the type-erased EggFetch stream. The physical connection remains the fault unit: keep-alive and HTTP/2 multiplexing do not acquire per-request chaos identities.

M030 is closed (`0bc45f0`). It moved the pure Scenario V2 semantic/compiler authority into the narrow consumer-neutral `eggchaos-experiment` crate, preserved server public re-exports, added expected-generation policy-target adapters, and provided an embedded prepare/arm/start lifecycle with one shared Tokio monotonic epoch for schedule and caller workload correlation. This synchronizes the schedule clock; it does not claim deterministic kernel/application traffic timing or cross-process clock synchronization.

M031 is closed (`fa189b9`) as the exact-candidate qualification gate. It froze dependency direction, public seam compatibility, Dialer composition/error provenance, connection identity/evidence behavior, shared-epoch schedule conformance, performance/security/package regressions, and a closure-backed handoff table for downstream EggReplay/EggProbe planning.

The dependency rule is strict: eggchaos must not depend on `eggreplay-*` or `eggprobe-*`. Route identity and impairment identity remain orthogonal. EggReplay continues to own `.eggr`, semantic replay timing, and regression models; EggProbe continues to own route/probe/report/assertion semantics. Their product integrations begin only after M031 closes.

## 11H. Activated cross-language contract and binding sequence

ADR 006 activates a contract-first language-binding tranche:

    M032 native protocol contract extraction + OpenAPI foundation
      -> M033 Python + TypeScript native-control SDKs
      -> M034 Python native embedding pilot + binding qualification

M032 is closed at `ed05f68`. It extracted the explicit
native `/v1` wire DTOs from `eggchaos-server` into a narrow
`eggchaos-protocol` crate, preserved compatibility re-exports, and added a
checked-in OpenAPI contract whose drift against the real server/protocol
authority is mechanically detected. It added no foreign SDK or FFI.

M033 is closed at `429d459`. It built complete Python (stdlib-only
sync/async) and TypeScript/Node (zero-dependency `fetch`) remote control
SDKs from the exact M032 contract, covering stream and datagram
resources, Scenario V1/V2, evidence/control views, auth, errors, reset/history,
and metrics. The SDKs do not manage the daemon lifecycle and contain no native
extension.

M034 is closed at `991818b`. It added a coarse safe Rust embedding facade
(preferred crate `eggchaos-embed`) over existing server/control/experiment
authorities and qualified the PyO3/maturin Python native package
(`eggchaos-native`, abi3) with lifecycle, conformance, wheel-matrix, and
control-overhead evidence. Rust/Tokio stream traits, futures, borrows,
`Arc`, and monotonic `Instant` values did not become foreign ABI
concepts. The closure records a no-go for a generic C ABI pending a
separate ADR and a second concrete consumer.

This tranche explicitly does not activate a generic C ABI, Node native addon,
JNI, P/Invoke, cgo, UniFFI, or WASM. A generic C ABI requires a separate ADR
after M034 and evidence for at least one additional concrete native consumer
beyond Python (or equivalently strong demand). Remote Go/Java/.NET clients
should normally reuse the M032 OpenAPI contract.

All normal Eggchaos Rust crates retain the safe-Rust boundary. No handwritten
unsafe code is authorized by this tranche; any binding-framework generated FFI
exception must remain isolated to the binding crate and be recorded/audited by
M034.

### Post-tranche corrective qualification — M035

A post-M034 audit found four bounded closure defects, not a new binding feature:

- the hosted remote-SDK qualification completes successfully and prints its
  pass marker, then exits 143 because the EXIT trap waits on intentionally
  SIGTERM-terminated child servers under `set -e`;
- native Python qualification scripts retain local macOS/x86_64 target
  assumptions and are not protected by a dedicated hosted CI job;
- datagram fault add/get/patch/remove/path-conflict semantics are duplicated in
  the HTTP admin path and `eggchaos-embed`;
- current-state planning/handoff text is inconsistent with the already-closed
  M032–M034 rows.

M035 is closed on `a710cd6` and is the corrective handoff record. It preserved the ADR 006
architecture, consolidated datagram mutation semantics below HTTP/embed,
established truthful hosted native-Python qualification, reconciled planning, and
obtained a green exact-head hosted matrix (13/13 jobs). M032–M034 historical
closure records remain immutable evidence of their original candidates.

## 11I. Post-v2.12 stream-loss compatibility and corrective closure

ADR 007's implementation sequence has landed:

```text
M036 deterministic stream-loss core + additive evidence
  -> M037 native/config/CLI/Scenario/OpenAPI/SDK/embed propagation
  -> M038 pinned post-v2.12 Toxiproxy packet_loss profile
  -> M039 initial tranche qualification/closure record
  -> M040 corrective requalification (active)
```

M036–M037 established the core/native implementation: fragmentation-independent
32 KiB logical-grain stream loss, additive evidence without changing the frozen
seven-slot activation arrays, and propagation through native/config/CLI/
Scenario/OpenAPI/Python/TypeScript/embed/Python-native.

M038 added the explicit opt-in profile pinned to Shopify/Toxiproxy
`40f7fd31bee529d824116bd2a11a9e3425e904ec`, where `packet_loss`
translates to native `StreamLoss`. Strict v2.12 remains the default/frozen
profile.

M039 attempted combined qualification. A later audit found bounded defects:
snapshot-profile `populate`/public conversion paths can bypass the active
profile; post-v2.12 zero/full data-plane checks are observational and share
residual toxics; intermediate loss/correlation statistics are absent; named
stream-loss Prometheus metrics are incomplete; the source oracle uses an
ambient unrecorded Go toolchain; exact-head hosted Linux/macOS/Windows evidence
was not rerun; and closure/current-state documents drifted.

M040 is the sole active corrective authority. It must fix those defects,
re-run both mandatory Toxiproxy oracles and the complete local/hosted
qualification on one exact candidate, and then become the final repository-level
closure authority for ADR 007. Historical M036–M039 evidence remains preserved.

This line still does not model IP/TCP retransmission or lower-layer packet loss,
does not reuse ADR 003 datagram-loss semantics, and does not track moving
Toxiproxy `main`.

## 12. Performance targets

No-fault overhead is a first-class regression metric.

Qualification should compare:

1. bare `eggress-relay`;
2. relay through `ChaosStream` with an empty plan;
3. each individual fault under a representative throughput/connection workload;
4. combinations likely in real tests.

For datagrams, retain M023's direct-UDP end-to-end ratio for historical regression comparison, but use M024's topology-matched bare fixed-target relay to isolate avoidable chaos/runtime overhead. Keep sequential RTT and windowed throughput as separate measurements.

Set numeric budgets only after a target-class baseline is measured. Do not invent a percentage before measurement. M008/M023 froze their historical budgets; M024 froze the topology-matched datagram budget, and M025 retained it without weakening.

## 13. Security and operational posture

- loopback admin by default;
- explicit opt-in for public data listeners where a configuration would otherwise bind wildcard/non-loopback;
- authenticated non-loopback native admin;
- secret redaction in config/debug/errors;
- bounded request bodies and JSON documents;
- bounded connection registry retention;
- no shell execution;
- no arbitrary plugin loading in the initial design;
- no hidden MITM/TLS interception;
- no claim that stream corruption reproduces lower-layer packet behavior.

## 14. Post-v1 directions

After M008 closes, reassess rather than automatically expanding scope.

Potential next lines:

- datagram/UDP impairment is implemented under ADR 003 + M020–M023, hardened by the closed M024 performance/runtime-maintainability pass, and closed through M025 association-setup/closure hygiene; follow-on datagram models require separate planning;
- optional upstream chains via `eggress-outbound`;
- cross-project integration substrate/harness is implemented under ADR 005 + M029–M031 (closed at `fa189b9`); it remains consumer-neutral and dependency-inward;
- eggreplay transport-chaos integration is downstream work after M031 so recorded/regression flows can be exercised under deterministic transport conditions without moving `.eggr` semantics into eggchaos;
- eggprobe controlled-impairment integration is downstream work after M031, initially for transport-bearing TLS/HTTP paths while route/probe/report semantics remain EggProbe-owned;
- cross-language bindings are implemented under ADR 006 + M032–M034 and correctively qualified under M035 (closed at `a710cd6`). A generic C ABI remains deferred pending a separate ADR and demand;
- richer time-varying scenarios and deterministic schedule files are activated under ADR 004 + M026–M028;
- post-v2.12 Toxiproxy stream-loss/`packet_loss` compatibility is implemented by ADR 007 + M036–M039 and is undergoing bounded corrective requalification under active M040; later upstream extensions or a tagged successor require separate reconciliation against the pinned snapshot;
- target-class SBC qualification and service-management integration through Eggstack shared updater/service machinery if operational demand exists.

None of these may weaken the fixed-target, protocol-neutral core boundary.
