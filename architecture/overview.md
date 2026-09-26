# Eggchaos architecture overview

Bird's-eye view of the eggchaos workspace: a Rust-native, fixed-target chaos
proxy with embeddable deterministic stream and whole-datagram fault engines.
This document summarizes each discrete module/component in one place and is
the index for systematic review — each section links to its deep dive in
this directory. Read this file first, then go component by component.

Pre-release `0.1.0`. Milestones M000–M041 are closed; `M041` (closed at
`724b967`) is the latest corrective authority and `M019` (`ca527db`) remains
the final pre-tag authority — later tranches do not rewrite it.
Canonical planning surface is `plans/` (see `AGENTS.md`); user-facing
implementation boundaries live in `docs/architecture.md`,
`docs/configuration.md`, `docs/control-plane.md`, `docs/toxiproxy.md`, and
`docs/eggfetch.md`.

## Workspace map

Dependency direction is inward (from `AGENTS.md`):

```text
eggchaos-cli -> eggchaos-server -> eggchaos-protocol -> eggchaos-experiment -> eggchaos-core
eggchaos-toxiproxy -> server/core (adapter, no separate state store)
eggchaos-eggfetch -> core (implements `eggfetch_core::Dialer`)
eggchaos-embed -> server/protocol/experiment (safe coarse facade, owns lifecycle)
eggchaos-native -> embed (PyO3 pilot, standalone maturin crate outside the workspace)
```

| Crate / package | Path | Role | Deep dive |
| --- | --- | --- | --- |
| `eggchaos-core` | `crates/eggchaos-core/` | Protocol-neutral deterministic fault substrate: typed stream `FaultPlan`, `DirectionEngine` state machines, `ChaosStream<T>` / `BidirectionalChaosStream` write-side adapters, `LivePolicy` publication, SplitMix64-v1 identity-scoped RNG, plus the sibling bounded whole-datagram scheduler. Knows nothing about HTTP, listeners, CLI, Toxiproxy, or Eggfetch. Empty stream plan delegates without allocating queue/timer. | [Core fault engine](core-fault-engine.md) |
| `eggchaos-experiment` | `crates/eggchaos-experiment/` | Consumer-neutral Scenario V2 authority: schedule source language, deterministic compiler, canonical SHA-256 fingerprint, run_id-independent namespaces, shared expected-generation schedule driver, prepare/arm/start lifecycle with one monotonic epoch gate, in-process `StreamPolicyTarget`. Depends only on core. | [Scenarios and observability](scenario-observability.md) |
| `eggchaos-protocol` | `crates/eggchaos-protocol/` | Stable native `/v1` wire DTOs + `NATIVE_OPERATIONS` inventory, drift-checked against `api/openapi/eggchaos-v1.yaml`. Kebab-case fault discriminators, integer-nanosecond durations, unknown-field rejection. Depends on core/experiment only. | [Protocol contract](protocol-contract.md) |
| `eggchaos-server` | `crates/eggchaos-server/` | Fixed-target TCP and UDP runtimes (never a forward proxy), bounded connection/association registries, single `ControlState` / `DatagramRuntime` mutation authorities, native admin HTTP (`admin.rs` on `eggserve-server` + `eggserve-primitives`), schema-v1 TOML config (`config.rs`), deterministic scenario drivers (V1 + V2). `eggress-relay` owns TCP bidirectional copy + half-close. | [Server runtime](server-runtime.md), [Control plane, config, and CLI](control-plane-cli.md) |
| `eggchaos-cli` | `crates/eggchaos-cli/` | `eggchaos` binary: `serve`, TCP `proxy`/`fault`/`connection`, UDP `datagram proxy`/`fault`/`association`, `scenario` (V1 + V2 incl. validate/compile), `version`, `reset`, `history`, `metrics`. Thin JSON client over native admin via `eggfetch-core`; every command emits one JSON doc with `--json` and exits nonzero on failure. | [Control plane, config, and CLI](control-plane-cli.md) |
| `eggchaos-toxiproxy` | `crates/eggchaos-toxiproxy/` | Toxiproxy v2.12 REST adapter over native `ControlState` (strict v2.12 default, frozen) plus the opt-in pinned post-v2.12 `packet_loss` snapshot profile. Holds no state; every view derives from snapshots, every mutation goes through native authority. | [Toxiproxy compatibility](toxiproxy-compat.md) |
| `eggchaos-eggfetch` | `crates/eggchaos-eggfetch/` | Composable in-process `eggfetch-core::Dialer` adapter (`ChaosDialer<D>` over any caller-selected inner dialer, direct-TCP convenience included): live physical-stream fault policy, caller-controlled deterministic connection identity, bounded out-of-band transport evidence. Eggfetch keeps HTTP/TLS/SNI/pooling. Per-request chaos is out of scope. | [Eggfetch integration](eggfetch-integration.md) |
| `eggchaos-embed` | `crates/eggchaos-embed/` | Safe coarse embedding facade over server/control/experiment authorities; owns lifecycle on a private Tokio runtime with blocking (never future-exposing) methods. Shared datagram mutation authority with HTTP admin (M035). | [Embedding and native bindings](embedding-native.md) |
| `eggchaos-native` | `bindings/python-native/` | PyO3/maturin pilot over `eggchaos-embed` (abi3 wheel). Standalone crate outside the workspace; maturin owns its build. Host-aware qualification (Apple targets only on Darwin). No generic C ABI (requires a separate ADR). | [Embedding and native bindings](embedding-native.md) |
| Remote SDKs | `bindings/python-client/`, `bindings/typescript-client/` | Stdlib-only Python (`Client` + `AsyncClient`) and zero-dep TypeScript (`EggchaosClient` over injectable `fetch`) control clients covering all `NATIVE_OPERATIONS`. Generated operation tables drift-checked against the OpenAPI contract. No FFI, no daemon lifecycle. | [Control plane, config, and CLI](control-plane-cli.md) |

Supporting members and tools:

| Area | Path | Role | Deep dive |
| --- | --- | --- | --- |
| OpenAPI contract | `api/openapi/eggchaos-v1.yaml` | Mechanically drift-checked native contract (36 operations). Authority is the `eggchaos-protocol` crate. | [Protocol contract](protocol-contract.md) |
| Benchmarks | `benchmarks/` (separate crate) | No-fault throughput/latency vs bare `eggress-relay` (TCP) and topology-matched bare UDP relay (sequential RTT + windowed throughput). | [Verification and qualification](verification-qualification.md) |
| Fuzz | `fuzz/` (separate workspace) | `plan_json` + datagram/transition/DTO/config targets via `cargo-fuzz` (`--sanitizer none` under pinned 1.89). | [Verification and qualification](verification-qualification.md) |
| Qualification | `qualification/` | Perf snapshots, release TOML fixture, pinned v2.12 oracle baseline + Go/Python client smokes, post-v2.12 evidence. | [Verification and qualification](verification-qualification.md) |
| Scripts | `scripts/` | Canonical command surface: `check`, `benchmark(_datagram)`, `qualify_*`, `check_*`, fetcher/qualifier pairs, `release-smoke`, `release-artifact-smoke`. | [Tooling and distribution](tooling-distribution.md) |
| CI / release | `.github/workflows/` (`ci.yml`, `release.yml`) | 3-OS fmt/clippy/test/doc/audit/deny + language-client + python-native jobs; release qualify + 5-target artifact matrix. | [Tooling and distribution](tooling-distribution.md) |
| Governance | `plans/` + `docs/` | `plans/roadmap.md` (architecture authority), `plans/registry.md` (status), `plans/adrs/`, `plans/reference/` (parity/verification contracts, not status), `plans/closure/` (evidence); `docs/` (user contracts). | [Tooling and distribution](tooling-distribution.md) |

## How everything fits together

1. Operator defines typed fault intent once in `eggchaos-core` (`plan.rs` for
   streams, `datagram.rs` for whole datagrams): bounded IDs, finite
   `Probability`, 8 stream `FaultKind`s (latency, bandwidth,
   blackhole/timeout, limit-data, slow-close, slice, disconnect, plus
   `stream-loss` per ADR 007) and 6 datagram kinds (delay, loss, duplication,
   reorder, corruption, bandwidth).
2. `LivePolicy` (`policy.rs`) publishes immutable
   `(plan, generation, seed_namespace)` snapshots via `ArcSwap`. Manual
   updates retain the namespace; Scenario V1 derives namespaces from
   `(scenario seed, run id, event index)` and Scenario V2 from
   `(scenario seed, execution key, schedule fingerprint, compiled event
   index)` with no run_id input.
3. Per-connection `DirectionEngine` (`engine.rs`) + `ChaosStream<T>`
   (`stream.rs`) enforce the plan on Tokio byte streams. `poll_write`
   success means the bounded queue owns the bytes; `poll_flush` is the
   delivery barrier. Termination is a durable level-triggered
   `TerminationHandle` (first request wins; survives generation swaps); the
   runtime edge maps it to shutdown vs hard reset. The datagram sibling
   (`DatagramDirectionEngine::admit` → `Immediate` / `Queued`, heap-drained
   by `take_ready`) enforces whole-message decisions with no byte-stream
   contract.
4. Determinism comes from `rng.rs`: SplitMix64-v1 sub-seeds derived from
   `(seed namespace, proxy identity, connection key, direction, fault id)`
   (plus a domain-separated chunk stream for stream-loss and a separate
   datagram domain). No process-global or scheduler-order RNG. Replay is
   exact for policy/per-key decisions, not live timing (connection keys
   depend on accept order).
5. `eggchaos-server` embeds the engines in fixed-target listeners
   (`runtime/` for TCP, `runtime/datagram/` for UDP): `ServiceBuilder` →
   `EggchaosService` → `ServiceHandle` + `ControlState` (+ `DatagramRuntime`
   for UDP). `eggress-relay` does the TCP byte relay; eggchaos wraps each
   direction in a chaos stream. Admission limits, connection/association
   registries, snapshots/evidence, metrics tables, `ResettableTcpStream`,
   and barrier generation transitions (old-generation preserving bytes
   drain before swap) live here.
6. Wire shape is owned by `eggchaos-protocol` (DTOs + `NATIVE_OPERATIONS`,
   drift-checked against `api/openapi/eggchaos-v1.yaml`): kebab-case
   `kind.type`, integer-nanosecond durations, explicit required fields,
   unknown-field rejection. The server (`native.rs`, `native_v2.rs`) only
   adapts DTOs into `ControlState` calls plus compatibility re-exports.
7. Control surfaces converge on one authority: `ControlState` (streams) and
   `DatagramRuntime` (datagrams). Native admin HTTP (`admin.rs` on
   `eggserve-server` + `eggserve-primitives`), file config (`config.rs`
   schema-v1 TOML), CLI (`eggchaos-cli`), Toxiproxy adapter, scenario
   drivers, remote SDKs, and `eggchaos-embed` are all translators into that
   authority — never alternate stores.
8. Scenarios drive generations over time: V1 (`scenario.rs`, `at_ms` events)
   and V2 (`eggchaos-experiment` compiler/fingerprint + shared
   epoch-anchored driver, `scenario_v2/` server wiring) publish through
   expected-generation guards, so a concurrent manual move fails the run
   instead of silently overwriting. The consumer-neutral `PolicyTarget` +
   `EpochGate` let in-process harnesses share one monotonic start epoch
   between schedule and caller workload.
9. Observability is evidence-first: `StreamEvidence` / `EngineEvidence` /
   `ConnectionEvidence` / `RngEvidence`, `ConnectionSnapshot` /
   association views, `MetricsCounters`, bounded scenario run records. No
   payload capture; histories, queues, tables, and label vocabularies are
   bounded. `GET /metrics` is Prometheus text without a `/v1` prefix.
10. Adapters extend reach without forking authority: `eggchaos-toxiproxy`
    translates toxics↔faults with oracle-exact routes/errors (strict v2.12
    frozen default; post-v2.12 `packet_loss` only under the pinned snapshot
    profile); `eggchaos-eggfetch::ChaosDialer` decorates any Eggfetch
    `Dialer` at the physical-stream seam; `eggchaos-embed` + remote SDKs +
    `eggchaos-native` expose the same native operations in-process,
    over HTTP, and in Python without new state stores.

## Project invariants (must not drift)

- Core is protocol-neutral; HTTP semantics do not belong in the stream or
  datagram engines.
- Proxy is fixed-target; not a general forward proxy / CONNECT/SOCKS router.
- Native userspace TCP byte-chunk dropping is `stream-loss`; only the
  Toxiproxy compatibility presentation may say `packet_loss`. It is not
  IP/TCP packet loss and never reuses ADR 003 datagram-loss semantics.
- `eggress-relay` remains TCP relay authority; `eggserve-server` /
  `eggserve-primitives` is the admin substrate; `eggfetch-core` (minimal
  features) is the CLI/Dialer substrate; `eggress-outbound` is
  optional/future only.
- `reset_peer`/hard-reset is best-effort and platform-qualified (RST vs FIN
  not asserted); ordinary `poll_shutdown` is never advertised as TCP RST.
- Directions are `upstream` (client→target) and `downstream`
  (target→client). Faults wrap destination writes; reads stay pass-through.
- No `unsafe` without separate ADR + audit (`unsafe_code = "forbid"`).
  No unbounded state. Seeded reproducibility. JSON-first CLI/control
  contract. Loopback-by-default admin/compat listeners; non-loopback needs
  explicit opt-in + bearer token (never echoed).

## Deep-dive index

Each file below is a focused review handoff for one discrete area. Read this
overview first, then go component by component.

1. [Core fault engine](core-fault-engine.md) — `eggchaos-core`: `plan.rs`,
   `engine.rs`, `stream.rs`, `policy.rs`, `rng.rs`, and `datagram.rs`;
   stream write/flush contract, `stream-loss` (ADR 007) grain/correlation
   rules, and the sibling bounded datagram scheduler with its six fault
   semantics.
2. [Fixed-target server runtime](server-runtime.md) — `eggchaos-server/runtime/`
   (plus `runtime/datagram/`): TCP listeners and `eggress-relay` embedding,
   the connection registry and `ControlState`, plus the independent bounded
   UDP `DatagramRuntime` with per-client connected upstream associations
   (M021/M024/M025 lifecycle).
3. [Control plane, config, and CLI](control-plane-cli.md) — `admin.rs`,
   `config.rs`, `native.rs` adapters, `eggchaos-cli/src/main.rs`: native
   `/v1` route inventory, schema-v1 TOML, CLI command matrix, auth/loopback
   policy, JSON contract, and the remote Python/TypeScript SDKs (M033).
4. [Protocol contract](protocol-contract.md) — `eggchaos-protocol` +
   `api/openapi/eggchaos-v1.yaml`: wire DTO ownership, `NATIVE_OPERATIONS`
   inventory, drift-check discipline, units/discriminators/bounds (M032).
5. [Scenarios and observability](scenario-observability.md) —
   `eggchaos-experiment` + `scenario.rs` + `scenario_v2/` + evidence /
   snapshot / metrics types: Scenario V1 driver, Scenario V2 compiler /
   fingerprint / epoch-anchored driver / isolation / cleanup, generation
   barriers, run records, connection inspection, Prometheus surface.
6. [Toxiproxy v2.12 compatibility](toxiproxy-compat.md) —
   `eggchaos-toxiproxy/src/lib.rs`: toxic↔fault translation, oracle-exact
   routes / errors, strict-vs-snapshot profiles, parity divergences,
   differential + client-smoke evidence (M036–M041).
7. [Eggfetch in-process integration](eggfetch-integration.md) —
   `eggchaos-eggfetch/src/lib.rs`: composable `ChaosDialer<D>`,
   caller-controlled connection identity, live-policy publishing, ownership
   split (dial vs HTTP/TLS), H1/`http2` profiles, regression map
   (M029/M031).
8. [Embedding and native bindings](embedding-native.md) — `eggchaos-embed`
   coarse facade (lifecycle, blocking contract, shared datagram mutation
   authority) and the `eggchaos-native` PyO3/maturin pilot plus its
   host-aware qualification (M034/M035).
9. [Verification and qualification](verification-qualification.md) — unit /
   property / half-close / backpressure / JSON round-trip / differential /
   cross-language / native-conformance tests, `fuzz/`, `benchmarks/`,
   `qualification/`, tolerance policy for wall-clock assertions,
   incomplete-evidence rule.
10. [Tooling, release, and repo governance](tooling-distribution.md) —
    `scripts/`, `.github/workflows/`, `dist/`, `deny.toml`, `plans/` +
    `docs/` authority, publish order, status vocabulary,
    closure-evidence discipline.

## Canonical references

- `docs/architecture.md` — dependency direction + M009 fault-semantics baseline.
- `docs/configuration.md`, `docs/control-plane.md`, `docs/eggfetch.md`,
  `docs/toxiproxy.md` — per-surface contracts.
- `plans/roadmap.md`, `plans/registry.md` — sequencing, invariants, milestone
  closure (M000–M041 closed; M019 final tag authority, M041 latest
  corrective authority).
- `plans/adrs/` — durable boundaries (001 stream engine, 002 determinism,
  003 datagrams, 004 schedules, 005 integration, 006 cross-language,
  007 stream-loss).
- `plans/reference/toxiproxy-parity.md`,
  `plans/reference/verification-matrix.md` — parity + verification contracts.
- `qualification/toxiproxy-v2-12/oracle-baseline-v2.12.0.md` — pinned oracle.
