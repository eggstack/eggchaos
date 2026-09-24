# M024 — Datagram Hot-Path Performance and Runtime Maintainability

Status: ready
Depends on: M023
Role: post-release performance/maintenance hardening

## Objective

Reduce avoidable overhead in the UDP/datagram hot path and make the datagram runtime easier to maintain without changing ADR 003 semantics, native wire contracts, deterministic traces, fixed-target association behavior, or Toxiproxy/Eggfetch compatibility.

M024 begins with measurement rather than an assumed optimization target. The existing M023 direct-UDP comparison is an end-to-end regression signal, but it includes the unavoidable cost of an additional proxy hop and therefore cannot by itself identify chaos-engine overhead.

## Baseline

M023 closed cleanly on exact candidate `ae2ab733b2be199d7693e40cdc558df01ee9a9de`.

On the recorded Apple M4 Pro / macOS arm64 run:

- direct UDP echo median: 43,786.71 datagrams/s, p95 42 µs;
- fixed-target empty-plan median: 24,107.89 datagrams/s, p95 66 µs;
- end-to-end throughput ratio: 0.5506;
- end-to-end p95 ratio: 1.5714;
- existing M023 gate: throughput ratio >= 0.45 and p95 ratio <= 2.5.

Those figures remain valid regression evidence but are not a topology-matched engine-overhead measurement. A fixed-target proxy necessarily adds another pair of socket receive/send operations.

Source inspection also identifies several concrete scaling/maintenance candidates:

1. `DatagramDirectionEngine.queue` is a `Vec<Candidate>`.
2. `next_deadline()` scans the complete queue.
3. `take_ready()` sorts the complete queue before every ready drain.
4. `admit()` allocates temporary candidate vectors even when an empty plan can emit immediately.
5. The runtime copies each received payload into owned `Bytes` before handing it to an association.
6. Per-association ingress uses an MPSC channel and live evidence updates use a mutex-protected record.
7. Association creation holds the association-map async mutex while awaiting UDP bind/connect.
8. `runtime/datagram.rs` has grown into a large combined model/registry/supervisor/association/test surface.

These are investigation targets, not pre-judged bottlenecks. Do not trade deterministic semantics or lifecycle correctness for a microbenchmark result.

## Scope

### In scope

- Improve datagram benchmark methodology so proxy-hop cost and chaos-engine cost are distinguishable.
- Capture a before-change profile and queue-depth scaling evidence.
- Add a topology-matched bare fixed-target UDP relay benchmark local to the benchmark harness.
- Separate sequential RTT measurement from windowed/saturated throughput measurement.
- Optimize the M020 datagram scheduler while preserving exact deterministic ordering/evidence.
- Add an empty-plan/immediate-emission fast path if profiling confirms value.
- Reduce avoidable candidate/scheduler allocations where this does not complicate semantics.
- Remove association-registry lock ownership across UDP socket setup awaits.
- Reduce hot-path evidence/record synchronization only where snapshots remain exact and bounded.
- Split the datagram runtime into cohesive internal modules while preserving one runtime authority and public re-exports.
- Rerun the complete M023 regression/qualification surface on one exact candidate.
- Record before/after measurements and freeze any new topology-matched performance budget only from measured data.

### Non-goals

- No new datagram fault kinds.
- No changes to ADR 003 probability, RNG, generation, overflow, corruption, or ordering semantics.
- No native API/config/CLI/schema changes.
- No change to per-client connected upstream socket ownership.
- No routing, SOCKS, QUIC parsing, raw-IP, MTU, or lower-layer network modeling.
- No new production benchmarking/profiling dependency solely for M024.
- No unsafe code or OS-specific fast path.
- No replacement of Tokio.
- No optimization justified only by source intuition if profiling disproves material impact.
- No tightening of the existing M023 budget before a measured baseline exists.

## Affected surfaces

Expected implementation areas:

- `crates/eggchaos-core/src/datagram.rs`;
- `crates/eggchaos-server/src/runtime/datagram.rs` and new private submodules;
- `benchmarks/src/bin/datagram.rs`;
- `scripts/benchmark_datagram.sh`;
- `qualification/performance/`;
- `architecture/core-fault-engine.md`;
- `architecture/server-runtime.md`;
- `architecture/verification-qualification.md`;
- `architecture/tooling-distribution.md`;
- `AGENTS.md` if runtime module paths change.

Do not change the native v1 DTO/resource layout unless a correctness defect is discovered. Such a defect requires explicit scope reconciliation before continuing.

## Ordered work packages

### WP1 — Establish topology-matched measurement

Before changing production hot-path code, extend the benchmark harness to report three distinct baselines:

1. direct UDP echo;
2. benchmark-local bare fixed-target UDP relay with the same client -> proxy -> fixed target -> proxy -> client socket topology but no `DatagramDirectionEngine`;
3. eggchaos fixed-target UDP with empty plans.

The bare relay belongs in the benchmark crate only. It must not become a second production runtime.

Separate two workload modes:

- **sequential RTT**: one logical datagram outstanding per client, used primarily for p50/p95 latency;
- **windowed throughput**: bounded multiple outstanding datagrams carrying deterministic sequence IDs, used for sustained datagrams/s and MiB/s without serial RTT dominating the result.

Windowed measurement must account for duplicates/loss explicitly and must not hang indefinitely if a candidate is dropped.

Retain the existing M023 direct-vs-eggchaos metrics so historical evidence remains comparable.

### WP2 — Capture pre-change profile and freeze optimization targets

On the same host/session class used for implementation evidence:

- run enough rounds/datagrams to quantify variance;
- record sequential and windowed direct/bare/empty-plan ratios;
- add core-only scheduler microbenchmarks or timing probes at representative ready-queue depths such as 1, 32, 256, and 1024;
- collect a sampling profile with an available host profiler (for example Instruments/xctrace on macOS or perf on Linux) without adding a production dependency;
- record the top material hot paths in the M024 closure evidence.

Only after this evidence exists, freeze a topology-matched empty-plan budget in `qualification/performance/README.md` / the benchmark script. The budget must be justified by observed variance and the measured bare-relay baseline.

If the topology-matched bare relay shows that the chaos engine contributes little overhead, do not force a large rewrite to improve the direct-UDP ratio. Preserve the finding and focus on queue scaling and maintainability.

### WP3 — Replace full-queue scan/sort scheduling

Replace the `Vec<Candidate>` scheduling strategy with a structure that provides:

- O(1) or O(log n) next-deadline inspection;
- O(log n) insertion;
- O(k log n) removal of k ready candidates;
- exact stable ordering by `(release_at, ingress_ordinal, copy_index)`;
- exact count/byte accounting;
- bounded capacity behavior identical to ADR 003;
- no task-per-datagram design.

A min-heap or equivalent is acceptable. Payload/generation fields must not make equal-key behavior nondeterministic. The existing golden trace fixture must remain byte-for-byte valid.

### WP4 — Empty-plan and immediate-emission fast path

If WP2 confirms material engine overhead, avoid scheduler work when a datagram is immediately deliverable.

The design may add an admission result equivalent to:

```text
Consumed        # configured loss or overflow
Immediate(item) # no release delay; runtime may send now
Queued          # scheduler owns one or more candidates
```

Exact API naming is implementation detail.

Requirements:

- evidence counters remain identical to the ordinary path;
- ingress ordinal still advances exactly once per original datagram;
- generation and seed evidence remain correct;
- an empty plan must not enter the deadline heap merely to be drained on the next loop turn;
- duplicate/fault combinations that produce multiple candidates retain deterministic copy order;
- no public socket/runtime concept leaks into `eggchaos-core`.

If an equivalent lower-risk design achieves the same result, use it instead.

### WP5 — Reduce proven allocation/synchronization overhead

Use WP2 profile evidence to address only material costs. Candidate techniques include:

- reusing engine scratch candidate buffers instead of allocating fresh vectors per admission;
- avoiding redundant plan/fault lookups;
- batching or atomically maintaining hot counters while retaining exact snapshots;
- reducing redundant record-lock acquisitions in one association-loop iteration;
- moving evidence snapshots to defined observation points rather than cloning large structures unnecessarily.

Do not add `SmallVec`, lock-free maps, object pools, custom allocators, or other dependencies unless measured benefit clearly exceeds complexity and the dependency is justified in closure evidence.

Payload copy reduction is optional and must be evidence-driven. A copy required to transfer ownership away from a reusable UDP receive buffer is acceptable.

### WP6 — Remove association-map lock across socket setup await

Refactor new-client association creation so the association registry lock is not held while awaiting `UdpSocket::bind` / `connect`.

The solution must preserve:

- at most one active association per client address;
- exact global/per-proxy capacity accounting;
- no leaked reservation on bind/connect failure;
- no duplicate long-lived upstream sockets after races;
- deterministic association ID assignment/evidence within documented guarantees;
- bounded memory and task ownership.

A small explicit starting/reserved state is preferable to broad new synchronization machinery. Add a concurrent-first-datagram stress test for one client and many clients.

### WP7 — Datagam runtime internal modularization

Split `runtime/datagram.rs` along cohesive private boundaries without changing public paths or authority. A reasonable target shape is:

```text
runtime/datagram/mod.rs
runtime/datagram/model.rs
runtime/datagram/registry.rs
runtime/datagram/association.rs
runtime/datagram/supervisor.rs
runtime/datagram/tests.rs
```

Exact filenames may vary.

Preserve one `DatagramRuntime` authority and the existing `ControlState` integration. Do not duplicate registries, metrics, cancellation, or policy state merely to obtain smaller files.

Move tests out of the production implementation file where this improves navigation. Keep visibility as narrow as possible.

Do not turn M024 into a broad `native.rs` or admin refactor; those surfaces remain out of scope unless directly required by a datagram runtime move.

### WP8 — Rebenchmark, qualify, and document

On one frozen candidate:

- rerun the enhanced datagram benchmark on the same host class as the pre-change baseline;
- record raw before/after reports;
- confirm the new topology-matched budget;
- retain and pass the M023 direct-vs-eggchaos floor;
- run deterministic golden traces unchanged;
- run all datagram runtime and race/lifecycle tests;
- run full workspace, fuzz, security, Eggfetch, strict Toxiproxy, release smoke, and hosted cross-platform CI;
- update architecture/performance docs and create exact-candidate closure evidence.

## Performance evidence requirements

The closure note must report, for before and after:

- direct sequential p50/p95;
- bare-relay sequential p50/p95;
- empty-plan sequential p50/p95;
- direct/bare/empty-plan windowed throughput;
- eggchaos empty-plan / bare-relay throughput ratio;
- eggchaos empty-plan / bare-relay latency ratio;
- retained eggchaos / direct M023 ratios;
- queue-depth scheduler measurements at the frozen representative depths;
- host CPU, OS, architecture, Rust version, payload size, window size, rounds, and sample count;
- profiler hotspots before and after.

Do not claim improvement from one favorable sample. Use medians over repeated same-session runs and retain raw JSON.

## Invariants

- ADR 003 deterministic behavior is unchanged.
- `datagram_golden_traces.json` remains unchanged unless a separately documented correctness bug requires a versioned semantic change; such a change is outside ordinary M024 optimization scope.
- Stream RNG and stream fault semantics remain unchanged.
- Configured loss, queue overflow, oversize, send error, and administrative discard remain distinct.
- Queue count/byte hard bounds are never weakened.
- Immediate emission must not bypass evidence or cancellation semantics.
- Per-client upstream socket ownership and response isolation remain unchanged.
- No association/task/socket is detached.
- No registry/state lock is held across external/network I/O after WP6.
- Public native JSON/TOML/CLI/Toxiproxy contracts remain unchanged.
- `unsafe_code = "forbid"` remains true.

## Verification

Minimum local exact-candidate gates:

```sh
./scripts/check.sh
./scripts/benchmark_datagram.sh
EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh
./scripts/qualify_eggfetch.sh
TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 ./scripts/qualify_toxiproxy_v2_12.sh
./scripts/release-smoke.sh
cargo audit --deny warnings
cargo deny check advisories licenses bans sources
```

Also dispatch ordinary hosted CI on Ubuntu/macOS/Windows. If the release qualification workflow remains the canonical way to exercise benchmark/fuzz/oracle/artifact gates together, run it against the exact candidate as M023 did.

Required focused tests include:

- unchanged deterministic 14-case datagram golden corpus;
- scheduler ordering at equal and mixed deadlines;
- count/byte overflow after scheduler replacement;
- empty-plan evidence equality between immediate and queued/reference paths;
- live generation mutation with old queued candidates;
- duplicate/loss/reorder composition unchanged;
- high queue-depth drain ordering and boundedness;
- concurrent first datagrams for one client create one association;
- simultaneous new clients respect global/per-proxy caps;
- bind/connect setup failure releases capacity and starting state;
- kill/delete/disable/shutdown leave no starting/active association task behind;
- multi-client/multiple-response/unsolicited-response behavior remains correct;
- IPv4/IPv6 host qualification remains green.

## Acceptance criteria

M024 closes only when:

- WP1 establishes direct, topology-matched bare-relay, and eggchaos benchmark baselines with separate sequential and windowed modes;
- WP2 records a pre-change profile and freezes any new performance threshold from measured evidence rather than assumption;
- the scheduler no longer rescans/sorts the entire queue on every ready check and preserves exact golden ordering;
- any empty-plan fast path preserves complete evidence/generation semantics;
- association setup no longer holds the association registry lock across UDP socket setup awaits;
- datagram runtime implementation is decomposed into cohesive private modules without duplicating authority;
- the existing M023 performance floor still passes;
- the newly frozen topology-matched performance budget passes;
- before/after results demonstrate a repeatable improvement in at least one measured avoidable-cost dimension (empty-plan engine overhead and/or queue-depth scaling) with no material regression in the other dimensions;
- all deterministic, runtime, fuzz, security, Toxiproxy, Eggfetch, release-smoke, and cross-platform CI gates are green;
- no unresolved medium-or-higher correctness/performance-maintainability finding remains;
- closure evidence identifies the exact implementation candidate and raw performance artifacts.

Create `plans/closure/M024-datagram-hot-path-performance-and-runtime-maintainability-closure.md`.

## Stop/rejection conditions

Do not close if:

- a faster result changes deterministic trace output or fault semantics;
- the benchmark still compares only direct UDP to a proxied path and calls the difference engine overhead;
- a microbenchmark improvement regresses end-to-end M023 bounds;
- queue optimization weakens hard bounds or stable tie ordering;
- an empty-plan bypass skips evidence or generation tracking;
- association setup races can create duplicate live associations or leak global capacity;
- modularization creates a second runtime/registry authority;
- a new dependency is added without measured justification;
- payload-copy elimination introduces unsafe code or fragile buffer lifetime coupling;
- one favorable benchmark run is used as closure evidence;
- release or regression gates are skipped because the work is "performance only."

## Follow-on activation

A clean M024 closes this optimization/maintenance pass. It does not automatically activate richer datagram semantics.

If profiling shows a larger architectural bottleneck that cannot be corrected without changing ADR 003 or public contracts, stop M024 at the narrow safe improvements and register a separate successor plan/ADR rather than widening this milestone.
