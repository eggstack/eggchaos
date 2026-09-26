# AGENTS.md

Rust workspace (edition 2021, pinned `1.89.0` in `rust-toolchain.toml` + CI). `unsafe_code = "forbid"` at workspace level — no new `unsafe` without an explicit ADR.

## Layout — dependency direction is inward

```text
eggchaos-cli -> eggchaos-server -> eggchaos-core (Tokio byte streams + datagrams)
eggchaos-toxiproxy -> server/core (adapter, no separate state store)
eggchaos-eggfetch -> core (implements `eggfetch_core::Dialer`)
eggchaos-embed -> server/protocol/experiment (safe coarse facade, owns lifecycle)
eggchaos-native -> embed (PyO3 pilot, standalone maturin crate outside the workspace)
```

- `eggchaos-core` (`crates/eggchaos-core/src/`): protocol-neutral stream `FaultPlan` + `ChaosStream<T>` write-side state machine and deterministic whole-datagram engine. Knows nothing about HTTP, listeners, CLI, Toxiproxy, or Eggfetch. Empty stream plan delegates without allocating queue/timer.
- `eggchaos-experiment` (`crates/eggchaos-experiment/src/`): consumer-neutral Scenario V2 semantic authority (source/compiler/fingerprint/run), shared expected-generation schedule driver, prepare/arm/start lifecycle with one monotonic epoch gate, and the in-process `StreamPolicyTarget`. Depends only on core; never on server, EggServe, CLI, Toxiproxy, EggReplay, or EggProbe.
- `eggchaos-protocol` (`crates/eggchaos-protocol/src/`): stable native `/v1` wire DTOs + `NATIVE_OPERATIONS` inventory, drift-checked against `api/openapi/eggchaos-v1.yaml`. Depends on core/experiment only; never on server, EggServe, CLI, Toxiproxy, or Eggfetch.
- `eggchaos-embed` (`crates/eggchaos-embed/src/`): safe coarse embedding facade over server/control/experiment authorities; owns lifecycle on a private Tokio runtime. Depends on server/protocol/experiment/core; never on listeners beyond what it owns, CLI, Toxiproxy, or Eggfetch.
- `eggchaos-server`: fixed-target TCP and UDP listeners (never a forward proxy). `eggress-relay` owns TCP bidirectional copy + half-close — do not fork its semantics. UDP associations own per-client connected upstream sockets. Admin H1 runtime is `eggserve-server` + `eggserve-primitives`; `native.rs` owns server-side DTO adapters + compatibility re-exports.
- `eggchaos-server/src/runtime/`: `mod.rs` composition/re-exports; `control.rs` single `ControlState` authority; `connection.rs` evidence/finalization; `supervisor.rs` listener admission; `transport.rs` reset wrapper; `metrics.rs` bounded metrics; `model.rs` runtime views; `datagram/` (`mod.rs` composition/re-exports, `model.rs`, `registry.rs` single `DatagramRuntime` authority, `association.rs`, `supervisor.rs`, `tests.rs`) owns UDP proxy/association lifecycle; `tests.rs` holds the TCP/runtime regression suite.
- `eggchaos-cli`: thin adapter; control HTTP via `eggfetch-core` (minimal features). Never a second networking/state path.
- `eggchaos-toxiproxy`: v2.12 REST adapter over native `ControlState`. `eggchaos-eggfetch`: physical-stream `Dialer`; Eggfetch keeps HTTP/TLS/SNI/pooling. Per-request chaos is out of scope.
- `benchmarks/` and `fuzz/` are separate crates/workspaces with their own manifests — don't run them via workspace commands.

## Where to look (read the overview first)

`architecture/overview.md` is the index; each file below is the review handoff for one area (no `.skills/` directory exists — these deep dives serve that role):

- `architecture/core-fault-engine.md` — plan/engine/stream/policy/rng semantics, write/flush contract, golden vectors.
- `architecture/server-runtime.md` — listeners, `eggress-relay` embedding, `ControlState` authority, full bounds table.
- `architecture/control-plane-cli.md` — `/v1` route inventory, schema-v1 TOML, CLI command matrix, auth/bounds.
- `architecture/scenario-observability.md` — scenario driver, evidence/snapshot/metrics types, replay limits.
- `architecture/toxiproxy-compat.md` — toxic↔fault table, defaults/naming precedence, strict/snapshot divergences and qualification evidence; M040 owns current corrective reconciliation.
- `architecture/eggfetch-integration.md` — `ChaosDialer`, ownership split, H1/`http2` profiles, regression map.
- `architecture/verification-qualification.md` — test layers, exact gate commands, incomplete-evidence rule.
- `architecture/tooling-distribution.md` — scripts catalog, CI/release workflows, dep policy, plan governance.
- User contracts: `docs/architecture.md`, `docs/configuration.md`, `docs/control-plane.md`, `docs/toxiproxy.md`, `docs/eggfetch.md`. Parity/verification contracts (not status): `plans/reference/`. Status: `plans/registry.md`.

## Commands (trust these, not guesses)

Full gate: `./scripts/check.sh` (= `cargo fmt --all -- --check` + `cargo clippy --workspace --all-targets --all-features -- -D warnings` + `cargo test --workspace --all-features` + `cargo doc --workspace --all-features --no-deps`). CI (`ci.yml`) additionally runs `cargo audit --deny warnings` and `cargo deny check advisories licenses bans sources` on ubuntu/macos/windows with a 25-min timeout.

Focused runs:

```sh
cargo test -p eggchaos-core --all-features
cargo test -p eggchaos-toxiproxy --all-features
cargo test -p eggchaos-eggfetch --all-features          # add --features http2 for the H2 qualification profile
cargo run --manifest-path benchmarks/Cargo.toml --release
./scripts/benchmark_datagram.sh
```

Qualification scripts (release workflow): `scripts/qualify_fuzz.sh`, `scripts/qualify_toxiproxy_v2_12.sh`, `scripts/qualify_eggfetch.sh`, `scripts/release-smoke.sh`, `scripts/release-artifact-smoke.sh`.

## Gotchas agents actually hit

- Toxiproxy differential needs the pinned oracle: `TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 ./scripts/qualify_toxiproxy_v2_12.sh`. Developer mode without a verified oracle reports `differential:incomplete` (exit 0) — that is not a pass. Compat server: `cargo run -p eggchaos-toxiproxy --example compat_server -- 127.0.0.1:8474`.
- Fuzz: `cargo-fuzz 0.13.2` cannot build under pinned 1.89 (transitive `cargo-platform` needs rustc 1.91). Release workflow installs it with `RUSTUP_TOOLCHAIN=stable`; the fuzz target itself still builds under 1.89 with `--sanitizer none`. See `release.yml`.
- Publish order matters (intra-workspace deps use `version = "0.1.0"` registry reqs): `core -> experiment/eggfetch -> protocol -> server/toxiproxy/cli -> embed`. `scripts/release-smoke.sh` asserts this order-proof. The `eggchaos-native` PyO3 pilot stands outside the workspace (plain cargo cannot link a macOS extension-module cdylib); maturin owns its build.
- Directions are `upstream` (client→target) and `downstream` (target→client). Faults wrap destination writes; reads stay pass-through.
- Native control is versioned under `/v1` except `GET /metrics` (Prometheus text, no prefix). Request bodies capped at 1 MiB. CLI: `eggchaos --admin <url> [--json] <command>`; every command emits one JSON doc with `--json` and exits nonzero on failure. Route/CLI inventory: `docs/control-plane.md`.
- Admin binds loopback by default. Non-loopback requires explicit public-admin opt-in + bearer token; failures return bounded JSON without echoing the token.
- All limits are bounded (queues, connections, bodies, histories, metrics cardinality). Default overflow is backpressure (`Pending`), never silent loss unless the fault documents discard. `poll_flush` is the delivery barrier; `poll_write` success only means the engine owns the bytes.
- Determinism: SplitMix64-v1, versioned seeds derived from `(seed namespace, proxy identity, connection key, direction, fault id)`. No process-global or scheduler-order RNG. ScenarioV1 namespaces derive from `(scenario seed, run id, event index)`; scenario-v2 namespaces derive from `(scenario seed, execution key, schedule fingerprint, compiled event index)` with no run_id; replay is exact for policy/per-key decisions, not live timing (connection keys depend on accept order).
- Native userspace TCP byte-chunk dropping is named `stream-loss`; only the Toxiproxy compatibility presentation may call the post-v2.12 spelling `packet_loss`. It is not IP/TCP packet loss and must never reuse ADR 003 datagram-loss semantics. `reset_peer`/hard-reset is best-effort and platform-qualified (RST vs FIN not asserted); ordinary `poll_shutdown` is never advertised as TCP RST.
- Dep discipline: `eggress-relay` not `eggress-embed`; `eggserve-server`+`eggserve-primitives` not `eggress-admin`/`eggserve-core`; `eggfetch-core` minimal features; `eggress-outbound` only behind an optional feature with proven demand. Don't copy sibling-repo code when a published Eggstack crate provides the primitive.

## Planning state

M000–M025 and M008 are closed milestones. The 2026-09-23 post-M015 corrective chain `M016 -> M017 -> M018 -> M019` closed, with M019's final candidate `ca527db`. M015 remains historical qualification for `cd88b22`, but M019 is the final tag authority. The owner may proceed with tag/publication/release actions; do not rewrite `plans/closure/` / `plans/archive/` history.

Post-release UDP/datagram feature work registered under ADR 003 is complete: M020 closed at `56c8925`, M021 at `686838b`, M022 at `8c4e3fb`, and M023 qualified exact candidate `ae2ab733b2be199d7693e40cdc558df01ee9a9de`. Closure evidence is in `plans/closure/M020-deterministic-datagram-fault-engine-closure.md`, `plans/closure/M021-fixed-target-udp-runtime-and-association-lifecycle-closure.md`, `plans/closure/M022-datagram-native-control-scenarios-cli-observability-closure.md`, and `plans/closure/M023-datagram-qualification-performance-release-hardening-closure.md`. This tranche does not rewrite M019 or make UDP part of the historical v0.1.0 qualification.

M024 is closed at `ca46801` and is fully implemented, measured, and qualified. It is semantics-preserving: topology-matched bare UDP relay benchmark with separate sequential RTT and windowed throughput, heap scheduler replacing full-queue scan/sort, empty-plan immediate emission, batched egress accounting, association-registry lock removed from UDP setup awaits, and the datagram runtime split internally.

M025 is closed at `55911f6` with evidence in
`plans/closure/M025-datagram-association-setup-waiter-and-closure-hygiene-closure.md`.
The final association setup rule is a per-reservation retained/versioned Tokio
`watch` transition: waiters subscribe while holding the association-map lock,
then use `wait_for`; publication, failure, and administrative drain publish
terminal state under that same lock, so a transition cannot be lost. Explicit
reservation identity and idempotent global/per-proxy capacity leases prevent
stale owners from publishing over or releasing successors. Do not change ADR
003, datagram golden traces, native contracts, M024 performance budgets, or
per-client upstream socket ownership without a new datagram plan/ADR.

ADR 004's execution chain is complete: `M026 (closed) -> M027 (closed) ->
M028 (closed)`. The bounded scenario-v2 compiler, canonical fingerprint,
run_id-independent namespace vectors, runtime/control wiring, and
exact-candidate qualification are implemented and qualified. ScenarioV1
remains supported; do not implement v2 by adding clock/schedule branches to
eggchaos-core.

ADR 005's execution chain is complete: `M029 (closed) ->
M030 (closed) -> M031 (closed)`. M029 made the EggFetch physical-stream
chaos integration composable over arbitrary Dialers, added
caller-controlled deterministic physical connection identity, and exposed
bounded bidirectional evidence. M030 added the consumer-neutral Scenario
V2 experiment harness/shared monotonic start epoch behind
`eggchaos-experiment`. M031 qualified the tranche on exact candidate
`fa189b9` with the downstream handoff table in its closure. Do not add
`eggreplay-*` or `eggprobe-*` production dependencies to eggchaos;
product-specific adoption remains downstream.

ADR 006's feature chain is complete: `M032 (closed at ed05f68) ->
M033 (closed at 429d459) -> M034 (closed at 991818b)`. The post-closure
corrective `M035 (closed at a710cd6)` fixed the hosted language-client
cleanup false failure, made native-Python qualification host-aware and
protected by hosted CI, consolidated datagram fault mutation semantics
below HTTP/embed, reconciled current-state planning, and requalified the
exact head with a green hosted matrix. Do not rewrite M032–M035 closure
history. Do not start a generic C ABI, Node native addon, JNI, P/Invoke,
cgo, UniFFI, or WASM under M035; a generic C ABI still requires
a separate ADR after demonstrated multi-consumer demand.

ADR 007's M036–M039 implementation chain is historical. A post-M039
audit registered `M040 (ready)` as the sole corrective handoff. Do not
reimplement M036/M037 core/native stream loss. M040 owns the snapshot-profile
`populate`/conversion fixes, executable isolated packet_loss data-plane edge
tests, intermediate/correlation statistical qualification, named stream-loss
Prometheus metrics, exact Go oracle-toolchain recording, exact-head hosted
Linux/macOS/Windows + language/native-Python requalification, and planning/
closure reconciliation. Strict v2.12 remains the default/frozen profile and
must keep its pinned oracle behavior. Preserve historical M036–M039 closure
evidence; M040 closure becomes the final ADR 007 repository-level authority.

If the owner asks for new work: `plans/roadmap.md` is the architecture authority, `plans/reference/` holds parity/verification contracts (not status), ADRs live in `plans/adrs/`. Any new numbered plan needs objective, baseline/deps, scope + non-goals, affected crates, ordered work packages, invariants/failure semantics, test commands, acceptance criteria, stop conditions, closure evidence, and follow-on rules — and must update `plans/registry.md` in the same change. Never mark `closed` from source inspection; closure requires running the plan's tests on the exact candidate plus external/differential evidence where declared.

## Verification

Prefer deterministic Tokio-time tests; wall-clock assertions need justified tolerances and must not be sole evidence. Cover: fault state machines, byte-conservation properties where faults preserve bytes, half-close/shutdown, bounded-buffer/backpressure, RNG golden vectors, exact JSON/TOML round trips, Toxiproxy differential (47/47 vs pinned v2.12.0), no-fault throughput/latency vs bare `eggress-relay`, exact datagram traces and the measured fixed-target UDP budget. Record un-runnable oracles/platforms as incomplete evidence. Authoritative semantics: `docs/architecture.md`, `docs/configuration.md`, `docs/control-plane.md`, `docs/toxiproxy.md`, `docs/eggfetch.md`.
