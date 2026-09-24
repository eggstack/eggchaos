# Eggchaos Long-Term Roadmap

Status: M008 and M009–M021 are closed work. M019 passed final pre-tag qualification at `ca527db`; the owner may proceed with the v0.1.0 tag and separate publication/release actions. ADR 003 is accepted; M020 closed on `56c8925` and M021 on `686838b`, activating M022.

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

Current Toxiproxy `main` includes `packet_loss`, which was not present in the v2.12.0 release tag. Eggchaos may add a compatibility spelling after M006, but the native model should call this stream-chunk loss or byte-stream corruption. Dropping user-space TCP stream chunks is not equivalent to IP/TCP packet loss because it bypasses retransmission semantics.

Real packet loss/reordering belongs in a datagram or lower-layer impairment subsystem. The user-space UDP/datagram line is activated by ADR 003 and M020–M023; lower-layer IP/qdisc phenomena remain out of scope.

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
  -> M022 native control/config/CLI/scenarios/observability (ready)
  -> M023 cross-platform qualification + measured performance budget (blocked)
```

M020 defines ordered whole-datagram delay/jitter, loss, duplication, reorder-by-hold, payload corruption, and bandwidth semantics with per-datagram probability, domain-separated deterministic RNG, bounded deadline scheduling, explicit drop-newest overflow, and admission-time generation snapshots.

M021 adds a fixed-target UDP listener whose client `SocketAddr` maps to a bounded association with its own connected upstream UDP socket. That model is required to prevent reply cross-delivery and to support multiple or unsolicited target responses. It must audit the exact published Eggress seam/version before choosing production reuse; routing/SOCKS-heavy UDP APIs are not imported merely for convenience.

M022 adds sibling native resources under `/v1/datagram-proxies` and `/v1/datagram-associations`, plus TOML, CLI, explicit datagram scenario actions, metrics, and bounded evidence. Toxiproxy v2.12 remains TCP/stream-only.

M023 freezes exact deterministic traces, runs real multi-client UDP tests on supported host OSes, expands fuzz/bounds/security evidence, measures the first no-fault datagram performance baseline and only then freezes a regression budget, and reruns existing stream/Toxiproxy/Eggfetch release regressions.

This tranche is post-release work. It does not rewrite M019 closure evidence and is not part of the historical v0.1.0 qualification gate.

## 12. Performance targets

No-fault overhead is a first-class regression metric.

Qualification should compare:

1. bare `eggress-relay`;
2. relay through `ChaosStream` with an empty plan;
3. each individual fault under a representative throughput/connection workload;
4. combinations likely in real tests.

Set numeric budgets only after M001/M002 establish a target-class baseline. Do not invent a percentage before measurement. M008 may freeze a budget from observed data.

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

- datagram/UDP impairment is activated as ADR 003 + M020–M023; follow-on datagram models require separate planning;
- optional upstream chains via `eggress-outbound`;
- eggreplay integration so recorded flows can replay with timing/failure profiles;
- eggprobe integration for controlled diagnostic experiments;
- language bindings around the stable Rust engine;
- richer time-varying scenarios and deterministic schedule files;
- post-v2.12 Toxiproxy extensions where useful;
- target-class SBC qualification and service-management integration through Eggstack shared updater/service machinery if operational demand exists.

None of these may weaken the fixed-target, protocol-neutral core boundary.
