# Eggchaos architecture overview

Bird's-eye view of the eggchaos workspace: a Rust-native, **fixed-target**
chaos proxy with embeddable deterministic stream and whole-datagram fault
engines. This file is both the summary of each discrete module/component and
the index for systematic review — every component section links to its deep
dive in this directory. Read this first, then go component by component.

> **Verification status.** Every numeric and structural claim below was
> re-checked against the source at the current HEAD. Claims that could not be
> confirmed were removed rather than softened. Deep dives are the review
> handoffs; this file never restates their detail.

## What the system is

One sentence: **eggchaos injects deterministic, seeded, byte-level network
impairment into a TCP/UDP relay that is always bound to a known target, and
exposes exactly one control authority that every surface — HTTP, CLI, file
config, Toxiproxy-compatible REST, in-process embedding, and remote SDKs —
translates into.**

Three properties shape every design decision:

1. **Fixed-target, never a forward proxy.** Each proxy is configured with a
   concrete `upstream`. There is no CONNECT/SOCKS routing, no
   request-parsing, no open relay.
2. **Deterministic given a seed.** Every probabilistic decision derives from
   a versioned SplitMix64-v1 sub-seed computed from stable identity inputs
   (see [Determinism](core-fault-engine.md#6-determinism)). Replay is
   exact for policy and per-key decisions; it is not exact for live timing,
   because connection keys depend on accept order.
3. **Bounded everything.** Queues, connections, bodies, histories, metrics
   cardinality, run records. Default overflow is backpressure, never silent
   loss unless the fault documents discard.

## Release status

Published `v0.2.0` is the current release: annotated tag `v0.2.0`
(`15746dc…` peeling to `b6a277d`) with all eight Rust workspace crates at
`0.2.0` on crates.io. Published `v0.1.0` is preserved as immutable history.
The native `/v1` contract, config schema v1, RNG v1, and provenance schema v1
are unchanged by the release.

The authority layers below matter when reading the deep dives, because several
milestones remain the named authority for a specific concern even after later
work supersedes them:

- **M058 closed** the `v0.2.0` publication (frozen candidate `b6a277d`; tag CI
  `36935298504` 15/15, tag release `36935298484` 7/7). It is the publication
  authority.
- **M059 closed** as the pre-publication security/dependency/maintenance
  corrective (exact candidate `1409d0f`): workspace lints, first-party
  dependency reconciliation, action SHA pinning, the Rust API gate, and
  Sigstore attestation. It introduced the four M059 guards and the `api-gate`
  job.
- **M057 closed** as the additive exact-head hosted qualification authority for
  the shared release-contract DAG (candidate `818e567`; hosted run `36490497114`
  14/14, dispatch `36630812771`). M056 remains the *implementation* authority
  for that DAG — it closed without hosted CI on its exact implementation SHA,
  which is exactly why M057 exists.
- **M061 is ready** as the additive M060 closure-evidence/plan-status consistency corrective. It changes planning/evidence guards only and must obtain exact-head hosted CI; production and release surfaces stay frozen.
- **M060 closed** the post-release documentation/status and drift-guard cleanup
  corrective (candidate `e897cc4`). It changed no Rust source, manifest,
  lockfile, OpenAPI, generated contract, or package version; it added
  `scripts/check_release_state_docs.py` to keep published status from drifting
  back into pre-publication prose.

## Workspace map

Dependency direction is inward — outer surfaces depend on the protocol-neutral
core, never the reverse.

```text
eggchaos-cli ──────► eggchaos-server ──► eggchaos-protocol ──► eggchaos-experiment ──► eggchaos-core
                                          ▲          ▲                                    ▲     ▲
eggchaos-toxiproxy ───────────────────────┘          │                                    │     │
eggchaos-embed ───────────────────────────────────────┘                                    │     │
eggchaos-eggfetch ───────────────────────────────────────────────────────────────────────────┘     │
eggchaos-native ──► eggchaos-embed (standalone maturin crate, outside the workspace)                      │
```

| Crate | Path | Scale | Role |
| --- | --- | --- | --- |
| `eggchaos-core` | `crates/eggchaos-core/` | 7 files, ~7.0k lines | Protocol-neutral deterministic fault substrate. Knows nothing about HTTP, listeners, CLI, Toxiproxy, or Eggfetch. |
| `eggchaos-experiment` | `crates/eggchaos-experiment/` | 13 files, ~3.2k | Consumer-neutral Scenario V2 authority: source language, compiler, fingerprint, shared schedule driver. |
| `eggchaos-protocol` | `crates/eggchaos-protocol/` | 5 files, ~2.9k | Stable native `/v1` wire DTOs + `NATIVE_OPERATIONS` (36 operations). |
| `eggchaos-server` | `crates/eggchaos-server/` | 29 files, ~16.7k | Fixed-target TCP + UDP runtimes, the single control authority, admin HTTP, config, scenario drivers. |
| `eggchaos-cli` | `crates/eggchaos-cli/` | 1 file, ~1.0k | Thin JSON client; 10 top-level commands. Never a second state path. |
| `eggchaos-toxiproxy` | `crates/eggchaos-toxiproxy/` | 1 file, ~2.0k | Toxiproxy v2.12 REST adapter over native `ControlState`. Holds no state. |
| `eggchaos-eggfetch` | `crates/eggchaos-eggfetch/` | 2 files, ~1.4k | Physical-stream `Dialer` adapter. Eggfetch keeps HTTP/TLS/SNI/pooling. |
| `eggchaos-embed` | `crates/eggchaos-embed/` | 1 file, ~0.7k | Safe coarse embedding facade owning lifecycle on a private Tokio runtime. |
| `eggchaos-native` | `bindings/python-native/` | standalone | PyO3/maturin pilot over `eggchaos-embed` (abi3 wheel). Outside the workspace. |

Note the deliberate dependency edges: `eggchaos-eggfetch` depends on
`eggchaos-core` only for its **runtime** deps (its `eggchaos-experiment` entry
is a dev-dependency for integration tests), and `eggchaos-experiment` depends
on `eggchaos-core` alone. That is what keeps the fault engine free of any
consumer's concepts.

Supporting members and tools:

| Area | Path | Role |
| --- | --- | --- |
| OpenAPI contract | `api/openapi/eggchaos-v1.yaml` | Mechanically drift-checked contract: **21 paths / 36 operations**. Authority is the `eggchaos-protocol` crate. |
| Remote SDKs | `bindings/python-client/`, `bindings/typescript-client/` | Stdlib-only Python (`Client` + `AsyncClient`) and zero-dep TypeScript (`EggchaosClient`) control clients over all 36 operations. No FFI, no daemon lifecycle. |
| Generated tables | `bindings/_contract/operations.json` + two generated tables | Regenerated by `scripts/sync_sdk_contract.py`; drift-checked by `git diff --exit-code`. |
| Benchmarks | `benchmarks/` (separate crate) | No-fault throughput/latency vs bare `eggress-relay` — 16 named stream cases (1 `bare_eggress_relay` baseline + 15 eggchaos cases, including 4 `stream-loss` cases) plus microprobes, and a topology-matched bare UDP relay. |
| Fuzz | `fuzz/` (separate workspace) | **9 targets** via `cargo-fuzz` (`--sanitizer none` under the pinned toolchain). |
| Qualification | `qualification/` | Perf snapshots + release TOML fixture + pinned v2.12 oracle baseline and post-v2.12 evidence. |
| Scripts | `scripts/` | Canonical command surface — see [Tooling](#tooling-and-distribution) below. |
| CI | `.github/workflows/ci.yml` | **5 jobs** — see below. |
| ADRs | `plans/adrs/` | **7** durable boundaries (001 stream engine, 002 determinism, 003 datagrams, 004 schedules, 005 integration, 006 cross-language, 007 stream-loss). |
| Governance | `plans/` + `docs/` | `plans/roadmap.md` (architecture authority), `plans/registry.md` (status), `plans/reference/` (parity/verification contracts, not status), `docs/` (user contracts). |

## The fault vocabulary

Both engines are seeded and bounded; they differ only in the unit of decision
(stream byte-chunk vs whole datagram). This table is the fastest way to see
the whole capability surface.

**Stream faults — 8 variants** (`eggchaos-core/src/plan.rs:214`, wire form
kebab-case via `#[serde(tag = "type", rename_all = "kebab-case", deny_unknown_fields)]`):

| Kind | Effect |
| --- | --- |
| `latency` | Delay accepted segments. |
| `bandwidth` | Rate-limit accepted segments. |
| `blackhole` | Intentionally discard accepted bytes. |
| `limit-data` | Stop forwarding after a byte boundary. |
| `slow-close` | Delay shutdown. |
| `slice` | Split writes into slices. |
| `disconnect` | Request connection termination. |
| `stream-loss` | Deterministically discard fixed-grain logical stream chunks (ADR 007). |

> **Reviewer trap.** `FAULT_TYPE_NAMES` has **7** entries for those **8**
> variants. That is intentional and test-locked
> (`crates/eggchaos-core/tests/stream_loss.rs:145` asserts the length): the
> metrics/evidence label table deliberately excludes `stream-loss`, which
> carries its own `STREAM_LOSS_TYPE_NAME`. The array is a stable index for
> counters and its order is part of the evidence contract — do not "fix" the
> mismatch by appending an entry.

**Datagram faults — 6 kinds** (`eggchaos-core/src/datagram.rs:74`, stable
names at `:17`): `delay`, `loss`, `duplicate`, `reorder`, `payload-corrupt`,
`bandwidth`. The whole-datagram scheduler decides
`Immediate` vs `Queued` per message and is heap-drained by `take_ready`; it
never exposes a byte-stream contract.

Naming discipline: native userspace TCP byte-chunk dropping is `stream-loss`
everywhere. Only the Toxiproxy compatibility presentation may say
`packet_loss`. It is not IP/TCP packet loss and never reuses ADR 003
datagram-loss semantics.

## How everything fits together

1. **Operator defines typed intent once** in `eggchaos-core` — `plan.rs` for
   streams, `datagram.rs` for whole datagrams — with bounded IDs, finite
   `Probability`, and validated configs.
2. **`LivePolicy` publishes immutable snapshots** (`policy.rs`) via `ArcSwap`
   as `(plan, generation, seed_namespace)`. Manual updates retain the
   namespace; scenario runs derive theirs (below). A live swap never erases a
   termination that is already due.
3. **Per-connection `DirectionEngine` + `ChaosStream<T>`** (`engine.rs`,
   `stream.rs`) enforce the plan on Tokio byte streams. `poll_write` success
   means the bounded queue owns the bytes; `poll_flush` is the delivery
   barrier — the engine drains its queue and *then* flushes the inner writer
   (`engine.rs:1547`). Termination is a durable, level-triggered
   `TerminationHandle` (`engine.rs:49`): publication survives a late waiter
   and the first request wins, so a policy transition cannot erase it. An
   empty plan delegates without allocating a queue or timer.
4. **Determinism is a contract, not luck.** `rng.rs` derives SplitMix64-v1
   sub-seeds from `(seed namespace, proxy identity, connection key, direction,
   fault id)`, with a domain-separated chunk stream for `stream-loss` and a
   separate datagram domain. There is no process-global and no
   scheduler-order RNG.
5. **The server embeds the engines in fixed-target listeners.** `runtime/`
   owns TCP (`ServiceBuilder` → `EggchaosService` → `ServiceHandle` +
   `ControlState`); `runtime/datagram/` owns UDP
   (`DatagramRuntime` with per-client connected upstream associations).
   `eggress-relay` owns the TCP bidirectional copy and half-close — eggchaos
   wraps each direction rather than reimplementing the relay. `ControlState`
   is defined at `runtime/mod.rs:425` and its behavior lives in
   `runtime/control.rs`; it is the **single** mutation authority for streams,
   with `DatagramRuntime` the single authority for datagrams. There is no
   second state store — the typed `operations.rs` layer only delegates.
6. **The wire shape is owned by `eggchaos-protocol`** — DTOs plus the
   `NATIVE_OPERATIONS` inventory (36 operations / 21 paths), drift-checked
   against the OpenAPI document. `./scripts/check_openapi.sh` currently
   reports `{"openapi":"pass","paths":21,"operations":36}`. The server's
   `native.rs` / `native_v2.rs` only adapt DTOs into `ControlState` calls.
7. **Every control surface converges on one authority.** Native admin HTTP
   (`admin.rs` on `eggserve-server` + `eggserve-primitives`), schema-v1 TOML
   (`config.rs`), the CLI, the Toxiproxy adapter, the scenario drivers, the
   remote SDKs, and `eggchaos-embed` are all *translators* into
   `ControlState` / `DatagramRuntime` — never alternate stores.
8. **Scenarios drive generations over time** and publish through
   expected-generation guards, so a concurrent manual move fails the run
   instead of silently overwriting. Scenario V1 namespaces derive from
   `(scenario seed, run id, event index)`; Scenario V2 derives from
   `(scenario seed, execution key, schedule fingerprint, compiled event
   index)` with **no run_id input** — which is what makes a fingerprint
   reusable. The fingerprint is a domain-separated SHA-256 over canonical
   encoded bytes (`fingerprint.rs`), explicitly excluding map iteration order
   and timing. A single monotonic epoch gate orders prepare/arm/start so an
   in-process harness can share one start epoch with its workload.
   Admission is bounded per family (`SCENARIO_FAMILY_CAPACITY = 32` for V1
   and V2 independently, `ControlState::MAX_SCENARIO_RUNS = 32`).
9. **Observability is evidence-first** — `StreamEvidence` /
   `EngineEvidence` / `ConnectionEvidence` / `RngEvidence`, connection
   snapshots, association views, `MetricsCounters`, and bounded run records.
   No payload capture. Histories, queues, tables, and label vocabularies are
   all bounded. `GET /metrics` is Prometheus text with **no** `/v1` prefix.
10. **Adapters extend reach without forking authority.**
    `eggchaos-toxiproxy` translates toxics↔faults — 7 toxics in the frozen
    strict v2.12 profile (`latency`, `bandwidth`, `timeout`, `limit_data`,
    `slow_close`, `slicer`, `reset_peer`), plus `packet_loss` as an 8th that
    the adapter emits **only** under the opt-in pinned post-v2.12 profile; in
    strict mode a native `stream-loss` returns `invalid_type` rather than
    silently becoming a different toxic. `ChaosDialer` decorates any Eggfetch
    `Dialer` at the physical-stream seam; `eggchaos-embed` + the remote SDKs
    + `eggchaos-native` expose the same operations in-process, over HTTP, and
    in Python — all without new state stores.

## Tooling and distribution

`./scripts/check.sh` is the local gate: **eight** cheap stdlib/shell guards,
then fmt → clippy → test → doc. The guards are `test_bench_provenance.sh`
(M048 Tier A), `test_planning_state.sh` (M053), `test_version_coherence.sh`
and `test_release_tag_version.sh` (M055/M056), `test_lint_inheritance.sh`,
`test_lock_coherence.sh`, and `test_action_pins.sh` (M059), and
`test_release_state_docs.sh` (M060). `cargo audit` and `cargo deny` live in
CI and `release-smoke.sh`; release-benchmark Tier B stays CI-only. Each guard
introduced after M055 is *also* wired independently into the `language-clients`
CI job, so deleting the local wiring still trips CI.

CI (`.github/workflows/ci.yml`) has **5 jobs**:

| Job | Runner / timeout | Role |
| --- | --- | --- |
| `check` | ubuntu + macos + windows, 25 min, `RUST_TEST_THREADS: 4` | Main gate: guards, fmt, clippy, workspace tests, IPv6-loopback datagram test, doc, audit, deny. |
| `performance-provenance` | ubuntu, 12 min | M048 Tier B release-benchmark provenance qualification, isolated from the OS/language matrices. |
| `language-clients` | matrix, 25 min | Python/TypeScript SDK drift + cross-language qualification, plus independent wiring of the M053/M055/M059/M060 guards. |
| `python-native` | matrix, 25 min | `eggchaos-embed` + `eggchaos-native` PyO3 checks and host-aware qualification. |
| `api-gate` | ubuntu, 25 min, **stable** toolchain | M059 Rust public-API regression gate (`cargo-semver-checks`); deliberately not on the pinned MSRV, and requires full Git history because its baselines are git revisions. |

Supply-chain discipline: every external `uses:` is a full 40-character commit
SHA, every workflow declares explicit top-level `permissions:`, and every
checkout sets `persist-credentials: false` — all three enforced by
`scripts/check_action_pins.py`, not by convention.

## Project invariants (must not drift)

- Core is protocol-neutral; HTTP semantics do not belong in the stream or
  datagram engines.
- The proxy is fixed-target; not a general forward proxy or CONNECT/SOCKS
  router.
- `stream-loss` is the native name; only the Toxiproxy presentation may say
  `packet_loss`, and it never reuses ADR 003 datagram-loss semantics.
- `eggress-relay` remains the TCP relay authority; `eggserve-server` /
  `eggserve-primitives` is the admin substrate; `eggfetch-core` (minimal
  features) is the CLI/Dialer substrate; `eggress-outbound` is optional/future
  only. Don't fork a primitive a published Eggstack crate already provides.
- `reset_peer`/hard-reset is best-effort and platform-qualified (RST vs FIN is
  not asserted); ordinary `poll_shutdown` is never advertised as TCP RST.
- Directions are `upstream` (client→target) and `downstream` (target→client).
  Faults wrap destination writes; reads stay pass-through.
- No `unsafe` without a separate ADR + audit (`unsafe_code = "forbid"` at
  workspace level). No unbounded state. Seeded reproducibility. JSON-first
  CLI/control contract.
- Admin binds loopback by default (`127.0.0.1:8475`). Non-loopback requires
  explicit `public_admin` opt-in **and** a bearer token, compared in
  constant time and never echoed (the `Debug` impl redacts it). A non-loopback
  bind without both fails with a bounded JSON error.

## Deep-dive index

Each file is a focused review handoff for one discrete area. All ten were
re-verified against the current source.

| # | Deep dive | Covers |
| --- | --- | --- |
| 1 | [Core fault engine](core-fault-engine.md) | `eggchaos-core`: `plan.rs`, `engine.rs`, `stream.rs`, `policy.rs`, `rng.rs`, `datagram.rs` — write/flush contract, `stream-loss` grain/correlation rules (ADR 007), and the sibling datagram scheduler. |
| 2 | [Fixed-target server runtime](server-runtime.md) | `eggchaos-server/runtime/` + `runtime/datagram/`: listeners, `eggress-relay` embedding, connection registry, `ControlState`, the `operations.rs` delegation layer, `ScenarioRegistry`, and the bounded UDP `DatagramRuntime`. |
| 3 | [Control plane, config, and CLI](control-plane-cli.md) | `admin.rs`, `config.rs`, `native.rs` adapters, the 36-route/21-path inventory, schema-v1 TOML, the CLI command matrix, auth/loopback policy, and the remote Python/TypeScript SDKs. |
| 4 | [Protocol contract](protocol-contract.md) | `eggchaos-protocol` + `api/openapi/eggchaos-v1.yaml`: wire DTO ownership, `NATIVE_OPERATIONS`, drift-check discipline, units/discriminators/bounds, and a live list of recorded discrepancies at HEAD. |
| 5 | [Scenarios and observability](scenario-observability.md) | `eggchaos-experiment` + `scenario.rs` + `scenario_v2/`: V1 driver, V2 compiler/fingerprint/epoch-anchored driver, generation barriers, run records, and the Prometheus surface. |
| 6 | [Toxiproxy v2.12 compatibility](toxiproxy-compat.md) | `eggchaos-toxiproxy/src/lib.rs`: toxic↔fault translation, oracle-exact routes/errors, strict-vs-snapshot profiles, parity divergences, differential + client-smoke evidence. |
| 7 | [Eggfetch in-process integration](eggfetch-integration.md) | `eggchaos-eggfetch/src/lib.rs`: composable `ChaosDialer<D>`, caller-controlled connection identity, live-policy publishing, ownership split (dial vs HTTP/TLS). |
| 8 | [Embedding and native bindings](embedding-native.md) | `eggchaos-embed` coarse facade (lifecycle, blocking contract, shared datagram authority) and the `eggchaos-native` PyO3/maturin pilot. |
| 9 | [Verification and qualification](verification-qualification.md) | Test layers, `fuzz/` (9 targets), `benchmarks/`, `qualification/` snapshots, the Tokio-time and wall-clock tolerance policy, and the incomplete-evidence rule. |
| 10 | [Tooling, release, and repo governance](tooling-distribution.md) | The full `scripts/` catalog, the 5-job CI matrix, the release `release-contract` DAG, publish order, and the planning/version dual-wiring guards. |

## Canonical references

- `docs/architecture.md` — dependency direction + M009 fault-semantics baseline.
- `docs/configuration.md`, `docs/control-plane.md`, `docs/eggfetch.md`,
  `docs/toxiproxy.md` — per-surface user contracts.
- `plans/roadmap.md` (architecture authority), `plans/registry.md` (the sole
  hand-maintained milestone-status authority), `plans/closure/` (evidence).
- `plans/adrs/` — the 7 durable boundaries listed in the workspace map.
- `plans/reference/toxiproxy-parity.md`, `plans/reference/verification-matrix.md`
  — parity and verification contracts (contracts, not status).
- `qualification/toxiproxy-v2-12/oracle-baseline-v2.12.0.md` — the pinned oracle.

## Focused verification commands

```sh
./scripts/check.sh                    # full local gate (8 guards + fmt/clippy/test/doc)
./scripts/check_openapi.sh            # protocol/OpenAPI drift — reports paths + operations
cargo test -p eggchaos-core --all-features
cargo test -p eggchaos-toxiproxy --all-features
cargo test -p eggchaos-eggfetch --all-features   # add --features http2 for the H2 profile
sh scripts/tests/test_bench_provenance.sh       # M047/M048 Tier A (cheap)
python3 scripts/check_planning_state.py --check  # M053 planning-state drift (cheap)
python3 scripts/check_version_coherence.py --check  # M055 version coherence (cheap)
```

<!-- BEGIN eggchaos:planning-state -->
<!--
  Generated by scripts/check_planning_state.py --write.
  Do not hand-edit between the markers; this script rewrites
  the block from plans/registry.md, the only hand-maintained
  source of truth for milestone state.
-->

## Current planning state

**Ready (next milestone):**
- `M061` (ready; plan `061-m060-closure-evidence-and-plan-status-consistency-corrective.md`)

**Highest closed milestone:** `M060` (see registry for closure evidence).

**Execution order:** `M061`

<!-- END eggchaos:planning-state -->
