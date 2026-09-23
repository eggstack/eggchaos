# Eggchaos architecture overview

Bird's-eye view of the eggchaos workspace: a Rust-native, fixed-target chaos
proxy and embeddable bounded byte-stream fault engine. This document is the
index for systematic review. Each section below summarizes one discrete
module/component and links to its deep dive in this directory.

Pre-release `0.1.0`. Canonical planning surface is `plans/` (see `AGENTS.md`);
user-facing implementation boundaries live in `docs/architecture.md`,
`docs/configuration.md`, `docs/control-plane.md`, `docs/toxiproxy.md`, and
`docs/eggfetch.md`.

## Workspace map

```text
eggchaos-cli -> eggchaos-server -> eggchaos-core
eggchaos-toxiproxy -----------^        |
eggchaos-eggfetch ------------^        +-> Tokio byte streams
```

| Crate | Path | Role |
| --- | --- | --- |
| `eggchaos-core` | `crates/eggchaos-core/` | Protocol-neutral deterministic fault engine: typed plans, `DirectionEngine` state machines, `ChaosStream<T>` / `BidirectionalChaosStream`, `LivePolicy`, SplitMix64-v1 identity-scoped RNG. No HTTP, listeners, CLI, Toxiproxy, or Eggfetch knowledge. |
| `eggchaos-server` | `crates/eggchaos-server/` | Fixed-target TCP runtime, bounded connection registry, native control authority (`ControlState`), native admin HTTP (`admin.rs`), schema-v1 TOML config (`config.rs`), deterministic scenario driver (`scenario.rs`). Uses `eggress-relay` for bidirectional copy / half-close; never forks its semantics. |
| `eggchaos-cli` | `crates/eggchaos-cli/` | `eggchaos` binary: `serve`, `proxy`, `fault`, `connection`, `version`, `reset`. Thin JSON client over the native admin API via `eggfetch-core`; `serve` boots `ServiceBuilder` + `NativeAdmin` from TOML. Machine-readable JSON is a first-class contract. |
| `eggchaos-toxiproxy` | `crates/eggchaos-toxiproxy/` | Toxiproxy v2.12 compatibility adapter. Holds no state; every view derives from `ControlState` snapshots and every mutation goes through native control authority. Oracle-exact routes, status codes, and error envelopes; divergences classified in `plans/reference/toxiproxy-parity.md`. |
| `eggchaos-eggfetch` | `crates/eggchaos-eggfetch/` | In-process `eggfetch-core::Dialer` adapter (`ChaosDialer`). Owns only raw direct TCP dial + physical-stream fault policy; Eggfetch remains authority for HTTP framing, pooling, TLS, SNI, cert verification. |

Supporting workspace members: `benchmarks/` (no-fault throughput/latency vs bare
`eggress-relay`), `fuzz/` (`plan_json` target), `qualification/` (perf snapshots,
release TOML, pinned v2.12 oracle baseline + Go/Python client smokes),
`scripts/` (check, benchmark, qualify, release smoke), `.github/workflows/`
(`ci.yml`, `release.yml`), `dist/` (release artifacts), `plans/` + `docs/`
(governance and contracts).

## How everything fits together

1. Operator defines typed fault intent once in `eggchaos-core::FaultPlan`
   (`crates/eggchaos-core/src/plan.rs`): bounded IDs, finite `Probability`,
   7 `FaultKind`s (latency, bandwidth, blackhole/timeout, limit-data,
   slow-close, slice, disconnect).
2. `LivePolicy` (`crates/eggchaos-core/src/policy.rs`) publishes immutable
   `(plan, generation, seed_namespace)` snapshots via `ArcSwap`. Manual updates
   retain the namespace; scenario runs derive namespaces from
   `(scenario seed, run id, event index)`.
3. Per-connection `DirectionEngine` (`crates/eggchaos-core/src/engine.rs`) +
   `ChaosStream<T>` (`crates/eggchaos-core/src/stream.rs`) enforce the plan on
   Tokio byte streams. `poll_write` may report acceptance once the bounded queue
   owns the bytes; `poll_flush` is the delivery barrier. Termination is a durable
   level-triggered `TerminationHandle` (first request wins; survives generation
   swaps); the runtime edge maps it to shutdown vs hard reset.
4. Determinism comes from `rng.rs`: `derive_seed(run_seed, proxy,
   connection_key, direction, fault)` and `derive_policy_seed(...)` feed
   fault-local SplitMix64-v1 streams. No process-global or scheduler-order RNG.
5. `eggchaos-server` embeds the engine in fixed-target TCP listeners
   (`runtime.rs`): `ServiceBuilder` -> `EggchaosService` -> `ServiceHandle` +
   `ControlState`. `eggress-relay` does the byte relay; eggchaos wraps each
   direction in a chaos stream. Admission limits, connection registry,
   snapshots/evidence, metrics tables, `ResettableTcpStream`, and generation
   transitions (old-generation preserving bytes drain before swap) live here.
6. Control surfaces converge on one authority: `ControlState` (M010). Native
   admin HTTP (`admin.rs` on `eggserve-server` + `eggserve-primitives`),
   file config (`config.rs` schema-v1 TOML), CLI (`eggchaos-cli`), Toxiproxy
   adapter, and scenario driver are all translators into that authority — never
   alternate stores. The native HTTP contract is explicit in `native.rs` and
   does not serialize internal fault enum layout as its mutation schema.
7. Observability is evidence-first: `StreamEvidence` / `EngineEvidence` /
   `ConnectionEvidence` / `RngEvidence`, `ConnectionSnapshot`, `MetricsCounters`,
   bounded scenario run records. No payload capture; histories and queues are
   bounded.
8. Loopback-by-default everywhere for admin/compat listeners; non-loopback
   requires explicit `public_admin` opt-in + auth policy. All buffers, queues,
   connection counts, body sizes, and fault buffers are bounded. `unsafe_code =
   "forbid"`.

## Project invariants (must not drift)

- Core is protocol-neutral; HTTP semantics do not belong in the stream engine.
- Proxy is fixed-target; not a general forward proxy / CONNECT/SOCKS router.
- Stream-chunk dropping is user-space byte-stream behavior, not IP/TCP packet
  loss (Toxiproxy compat may retain upstream naming with documented distinction).
- `eggress-relay` remains relay authority; `eggserve-server`/`eggserve-primitives`
  is the admin substrate; `eggfetch-core` is the CLI/Dialer substrate;
  `eggress-outbound` is optional/future only.
- No `unsafe` without separate ADR + audit. No unbounded state. Seeded
  reproducibility. JSON-first CLI/control contract.

## Deep-dive index

Each file below is a focused review handoff for one discrete area. Read this
overview first, then go component by component.

1. [Core fault engine](core-fault-engine.md) — `eggchaos-core`: `plan.rs`,
   `engine.rs`, `stream.rs`, `policy.rs`, `rng.rs`; 7 fault semantics,
   write/flush contract, termination handle, generation swaps, golden vectors.
2. [Fixed-target server runtime](server-runtime.md) — `eggchaos-server/runtime.rs`:
   listeners, `eggress-relay` embedding, connection registry, admission limits,
   reset semantics, metrics tables, `ControlState` authority.
3. [Control plane, config, and CLI](control-plane-cli.md) — `admin.rs`,
   `config.rs`, `eggchaos-cli/src/main.rs`: native `/v1` API, schema-v1 TOML,
   CLI command matrix, auth/loopback policy, JSON contract.
4. [Scenarios and observability](scenario-observability.md) — `scenario.rs`,
   evidence/snapshot/metrics types: deterministic scenario driver, generation
   barriers, run records, connection inspection, Prometheus surface.
5. [Toxiproxy v2.12 compatibility](toxiproxy-compat.md) —
   `eggchaos-toxiproxy/src/lib.rs`: toxic↔fault translation, oracle-exact routes
   / errors, parity divergences, differential + client-smoke evidence.
6. [Eggfetch in-process integration](eggfetch-integration.md) —
   `eggchaos-eggfetch/src/lib.rs`: `ChaosDialer`, live-policy publishing,
   ownership split (dial vs HTTP/TLS), H1/H2 coverage.
7. [Verification and qualification](verification-qualification.md) — unit /
   property / half-close / backpressure / JSON round-trip / differential tests,
   `fuzz/`, `benchmarks/`, `qualification/`, tolerance policy for wall-clock
   assertions.
8. [Tooling, release, and repo governance](tooling-distribution.md) —
   `scripts/`, `.github/workflows/`, `dist/`, `deny.toml`, `plans/` +
   `docs/` authority, status vocabulary, closure-evidence discipline.

## Canonical references

- `docs/architecture.md` — dependency direction + M009 fault-semantics baseline.
- `docs/configuration.md`, `docs/control-plane.md`, `docs/eggfetch.md`,
  `docs/toxiproxy.md` — per-surface contracts.
- `plans/roadmap.md`, `plans/registry.md` — sequencing, invariants, milestone
  closure (M000–M015 + M008 closed; tag/publish remain owner decisions).
- `plans/adrs/001-stream-fault-engine-boundary.md`,
  `plans/adrs/002-determinism-and-live-mutation.md` — durable boundaries.
- `plans/reference/toxiproxy-parity.md`,
  `plans/reference/verification-matrix.md` — parity + verification contracts.
- `qualification/toxiproxy-v2-12/oracle-baseline-v2.12.0.md` — pinned oracle.
