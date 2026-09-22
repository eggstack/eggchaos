# M001 — Workspace Bootstrap and Core Contracts

Status: implemented-awaiting-evidence  
Depends on: M000  
Successor: M002

## Objective

Turn the empty repository into a compiling Rust workspace whose crate boundaries, public domain types, dependency policy, and no-fault stream path are stable enough for the actual impairment engine to be implemented without re-litigating architecture.

This milestone is intentionally infrastructure-heavy and behavior-light. It should establish the smallest credible skeleton and prove that the proposed Eggstack seams compile together.

## User-visible outcome

A source checkout can:

- build and test the complete workspace;
- instantiate an empty `FaultPlan`;
- wrap an arbitrary Tokio full-duplex stream in `ChaosStream`;
- move bytes through that wrapper without modification;
- construct validated proxy/fault configuration values;
- build the server/CLI/compatibility/integration crate skeletons without circular dependencies.

No actual latency, throttling, blackhole, or other network impairment is required yet.

## Baseline

The repository currently contains planning documents only.

Research baseline:

- Rust MSRV target: 1.89;
- edition: 2021;
- `eggress-relay` current workspace version inspected: 1.0.7;
- `eggfetch-core` current inspected version: 0.2.0;
- `eggserve-server` and `eggserve-primitives` current inspected version: 0.2.0;
- core design is ADR 001;
- deterministic/live-mutation contract is ADR 002.

Dependency versions must be rechecked against crates.io or the current sibling repository manifests during implementation. Do not silently substitute Git path dependencies just because a required published version is unavailable; record the dependency decision explicitly.

## Scope

Create the workspace and initial crates:

```text
crates/
  eggchaos-core/
  eggchaos-server/
  eggchaos-cli/
  eggchaos-toxiproxy/
  eggchaos-eggfetch/
```

Recommended package/binary naming:

- package `eggchaos-core`, library `eggchaos_core`;
- package `eggchaos-server`, library `eggchaos_server`;
- package `eggchaos-cli`, binary `eggchaos`;
- package `eggchaos-toxiproxy`, library `eggchaos_toxiproxy`;
- package `eggchaos-eggfetch`, library `eggchaos_eggfetch`.

The latter two may remain minimal compile-only adapters in M001. Their actual behavior belongs to M006/M007.

Also establish:

- root workspace manifest;
- workspace lint policy;
- CI entry point;
- basic architecture documentation;
- public type documentation;
- exact initial dependency/feature inventory.

## Non-goals

Do not implement:

- substantive fault behavior;
- listener accept loops;
- HTTP admin endpoints;
- Toxiproxy routes;
- Eggfetch dialing behavior;
- Prometheus metrics;
- release packaging;
- Python/FFI bindings;
- UDP;
- proxy-chain support.

Do not add “temporary” duplicate implementations of relay or HTTP serving just to get the skeleton compiling.

## Expected root/workspace files

At minimum:

```text
Cargo.toml
Cargo.lock
README.md
LICENSE or dual-license files
rust-toolchain.toml or documented MSRV policy
.github/workflows/ci.yml
docs/architecture.md
docs/configuration.md
crates/...
plans/...
```

Use the repository's intended license consistently across package manifests.

If the license has not been explicitly chosen by the repository owner, stop before publishing crates and record the blocker; local workspace implementation may continue using a clearly marked provisional manifest only if that does not create misleading package metadata.

## Crate dependency rules

### eggchaos-core

Production dependencies should be minimal and protocol-neutral. Expected candidates:

- `tokio` with only needed I/O/time/sync features;
- `tokio-util` only if cancellation/utilities are genuinely used;
- `bytes`;
- `thiserror`;
- `serde` if the canonical domain model is serializable here;
- a snapshot primitive such as `arc-swap` only if needed for the core contract.

It must not depend on:

- Hyper;
- EggServe;
- Eggfetch;
- Eggress server/runtime crates;
- Clap;
- Prometheus;
- TOML;
- Toxiproxy compatibility types.

### eggchaos-server

May depend on:

- `eggchaos-core`;
- `eggress-relay`;
- Tokio/Tokio-util;
- Serde/TOML as needed;
- tracing;
- socket utilities if justified.

It should not yet depend on the full `eggress-embed`.

EggServe admin dependencies may be introduced here now as optional/unused scaffolding or deferred to M004. Prefer deferral if it keeps M001 smaller.

### eggchaos-cli

Keep thin. It may depend on Clap and serialization/output helpers. Network control behavior belongs to M004.

### eggchaos-toxiproxy

Must depend “inward” on native eggchaos types/runtime, never the reverse. Keep empty/minimal until M006.

### eggchaos-eggfetch

May declare the smallest `eggfetch-core` feature slice needed to name `Dialer`, but no behavior is required until M007. If doing so materially bloats ordinary workspace builds, feature-gate the adapter crate's Eggfetch integration and document the command that exercises it.

## Canonical domain types

M001 should establish a deliberately small public model in `eggchaos-core`.

Names can be adjusted for Rust ergonomics, but there should be exactly one authority for these concepts.

### Identity and direction

- `Direction::{Upstream, Downstream}`;
- `FaultId` — stable opaque/string identifier suitable for evidence and config;
- `ProxyId` should remain server-owned unless the core genuinely requires it;
- `RngVersion` — reserve the versioned RNG contract even before M002 implements it.

### Fault definition

A canonical ordered representation such as:

```rust
pub struct FaultPlan {
    faults: Vec<FaultSpec>,
}

pub struct FaultSpec {
    id: FaultId,
    probability: Probability,
    kind: FaultKind,
}

pub enum FaultKind {
    Latency(LatencyConfig),
    Bandwidth(BandwidthConfig),
    Blackhole(BlackholeConfig),
    LimitData(LimitDataConfig),
    SlowClose(SlowCloseConfig),
    Slice(SliceConfig),
    Disconnect(DisconnectConfig),
}
```

The exact enum nesting is not mandated. What is mandated:

- ordered faults;
- stable fault identity;
- validated probability;
- typed durations/rates/byte counts rather than raw unvalidated JSON maps;
- no Toxiproxy field names in the native core unless the name is independently appropriate.

### Validation

Provide one validation authority that rejects impossible or nonsensical values before a runtime engine is built.

Examples:

- probability outside 0..=1;
- zero buffer capacity where a fault requires storage;
- slice variation greater than/equal to average if the chosen semantics require a positive lower bound;
- duration/rate arithmetic that can overflow internal units;
- invalid combinations whose behavior is undefined.

Prefer constructor/newtype validation so invalid states are difficult to construct through public safe APIs.

### Stream capability vocabulary

Reserve a transport-neutral capability result such as:

```text
graceful_shutdown
half_close
hard_reset
```

The core must not pretend that all `AsyncWrite` streams can produce a TCP reset.

## ChaosStream empty-plan contract

Implement enough of `ChaosStream<T>` and the internal direction engine to prove the abstraction.

For an empty plan:

- `AsyncRead` is transparent;
- `AsyncWrite::poll_write` delegates correctly;
- vectored writes either delegate correctly or explicitly use the scalar fallback according to Tokio's contract;
- `poll_flush` delegates;
- `poll_shutdown` delegates;
- no timer/task is spawned per poll;
- no byte is buffered by eggchaos;
- no semantic change is introduced to EOF/half-close.

The empty-plan code path should be explicit and cheap.

Do not implement fake fault behavior in this milestone.

## Workspace lint and quality baseline

Recommended root policy:

```toml
[workspace.lints.rust]
unsafe_code = "deny"

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
```

Allow narrowly noisy lints explicitly rather than turning off broad lint families.

Public library crates should warn/deny missing docs according to the existing Eggstack convention chosen for the workspace.

## CI baseline

Initial CI should cover the supported host-level workspace on at least Linux, macOS, and Windows if runner setup is straightforward.

Required routine commands:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
```

If all-features combines deliberately incompatible feature profiles, define explicit supported feature jobs instead of weakening the gate. Document that matrix.

Add a lockfile policy appropriate for a binary-containing workspace.

A lightweight `scripts/check.sh` may aggregate the local CI-equivalent commands, following other Eggstack repositories.

## Dependency evidence

Before closing M001, capture:

- `cargo tree -p eggchaos-core`;
- `cargo tree -p eggchaos-server`;
- `cargo tree -p eggchaos-cli`;
- feature trees for the Eggfetch/EggServe integrations if declared.

Check that:

- core does not accidentally pull Hyper/Clap/Eggfetch/EggServe;
- server does not pull `eggress-embed`;
- the Eggfetch adapter uses the intended public `Dialer` feature slice;
- duplicate Tokio/Hyper major versions are not introduced unnecessarily.

## Tests

Minimum M001 tests:

1. Empty `FaultPlan` validates and preserves order.
2. Probability boundary validation accepts 0 and 1 and rejects out-of-range values.
3. Invalid initial fault values fail with typed errors.
4. `ChaosStream` over `tokio::io::duplex` preserves arbitrary byte payloads.
5. Empty-plan flush is observed by the inner stream.
6. Empty-plan shutdown preserves EOF/half-close behavior.
7. No-fault large payload does not duplicate or truncate bytes under partial reads/writes.
8. Serde round-trip of the canonical plan model if serialization is part of core.
9. Compile-time/sendability assertions for the intended `ChaosStream<T>` use in spawned tasks.
10. Workspace crate topology tests/gates, if a script is used, reject reverse dependencies from core to adapters.

## Documentation

Create/update:

- root README with current pre-release scope and explicit non-goals;
- `docs/architecture.md` with the crate dependency graph and ownership;
- `docs/configuration.md` with the native typed model, marked pre-M004 for file/API syntax;
- links back to ADR 001/002 and the roadmap.

Do not claim implemented fault support yet.

## Ordered work packages

Execute in this order unless a stop condition fires:

1. **WP1 — Workspace authority:** create root manifest/toolchain/license/README/lint policy and the five initial crate manifests with one-way dependency direction.
2. **WP2 — Canonical core domain model:** implement identity, direction, ordered fault definitions, validated newtypes/configs, RNG-version placeholder, capabilities, and typed validation errors.
3. **WP3 — Empty transport seam:** implement the no-fault `ChaosStream<T>` / direction-engine skeleton and prove Tokio read/write/flush/shutdown correctness without timers or queues.
4. **WP4 — Adapter/runtime skeletons:** make server, CLI, Toxiproxy, and Eggfetch crates compile while containing no duplicate network/fault authority.
5. **WP5 — Dependency/feature qualification:** run and document Cargo feature trees; reduce accidental umbrella/default dependencies.
6. **WP6 — CI and topology gates:** add routine checks, MSRV/supported-platform jobs as practical, and dependency-direction/topology checks.
7. **WP7 — Documentation reconciliation:** write root/architecture/config docs that describe only implemented M001 behavior.
8. **WP8 — Closure pass:** rerun the full M001 command set on one candidate commit, write closure evidence, and only then activate M002.

## Acceptance criteria

M001 closes only when:

- the workspace builds on the declared MSRV or the documented current toolchain if MSRV CI is separately configured;
- every initial crate exists with the intended one-way dependency direction;
- core contains the canonical typed/validated ordered plan model;
- empty-plan `ChaosStream` correctly implements the relevant Tokio I/O contracts;
- no relay implementation has been copied from Eggress;
- dependency/feature trees confirm the intended minimal seams;
- CI/local verification commands pass;
- docs do not overstate current capability;
- a closure record identifies the exact candidate commit and evidence.

## Stop/rejection conditions

Stop and revise the architecture rather than pushing through if:

- `eggress-relay` cannot be consumed without pulling an unexpectedly broad graph;
- Eggfetch's public `Dialer` cannot be named from a reasonable feature slice;
- the proposed `ChaosStream` write-side composition cannot preserve half-close semantics with `eggress-relay`;
- core needs HTTP/server-specific types to express basic faults;
- a circular dependency appears between native runtime and compatibility/integration crates;
- the empty-plan wrapper measurably changes semantics in basic duplex/half-close tests.

Any such finding should move M001 to `blocked` and produce a narrow corrective/ADR rather than silently changing the boundary.

## Closure evidence

Create `plans/closure/M001-workspace-bootstrap-and-core-contracts-closure.md` containing:

- candidate SHA;
- dependency tree excerpts or artifact paths;
- exact commands and results;
- platforms/runner matrix;
- empty-path test evidence;
- deviations from this plan;
- verdict for each acceptance criterion.

On successful closure, update `plans/registry.md`:

- M001 -> `closed`;
- M002 -> `ready`.

Do not activate M003 yet.
