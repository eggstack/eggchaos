# AGENTS.md

Rust workspace (edition 2021, `rust-toolchain.toml` pins `1.89.0`, `rust-version = "1.89"`). `unsafe_code = "forbid"` at workspace level — no new `unsafe` without an explicit ADR.

## Layout — dependency direction is inward

```text
eggchaos-cli -> eggchaos-server -> eggchaos-protocol -> eggchaos-experiment -> eggchaos-core
eggchaos-toxiproxy -> server/core (adapter, no separate state store)
eggchaos-eggfetch -> core (implements `eggfetch_core::Dialer`)
eggchaos-embed -> server/protocol/experiment (safe coarse facade, owns lifecycle)
eggchaos-native -> embed (PyO3 pilot, standalone maturin crate outside the workspace)
```

- `eggchaos-core`: protocol-neutral stream `FaultPlan` + `ChaosStream<T>` write-side state machine and deterministic whole-datagram engine. Knows nothing about HTTP, listeners, CLI, Toxiproxy, or Eggfetch. Empty stream plan delegates without allocating queue/timer.
- `eggchaos-experiment`: consumer-neutral Scenario V2 authority (source/compiler/fingerprint/run, shared schedule driver, prepare/arm/start with one monotonic epoch gate, in-process `StreamPolicyTarget`). Depends only on core; never on server, EggServe, CLI, Toxiproxy, EggReplay, or EggProbe.
- `eggchaos-protocol`: stable native `/v1` wire DTOs + `NATIVE_OPERATIONS`, drift-checked against `api/openapi/eggchaos-v1.yaml`. Depends on core/experiment only.
- `eggchaos-server`: fixed-target TCP and UDP listeners (never a forward proxy). `eggress-relay` owns TCP bidirectional copy + half-close — do not fork its semantics. UDP associations own per-client connected upstream sockets. Admin H1 runtime is `eggserve-server` + `eggserve-primitives`; `native.rs` owns DTO adapters + compatibility re-exports. `src/runtime/`: single `ControlState` authority (`control.rs`) + single `DatagramRuntime` authority (`datagram/registry.rs`); never a second state store.
- `eggchaos-cli`: thin adapter; control HTTP via `eggfetch-core` (minimal features). Never a second networking/state path.
- `eggchaos-toxiproxy`: v2.12 REST adapter over native `ControlState`. `eggchaos-eggfetch`: physical-stream `Dialer`; Eggfetch keeps HTTP/TLS/SNI/pooling. Per-request chaos is out of scope.
- `benchmarks/` and `fuzz/` are separate crates/workspaces with their own manifests — don't run them via workspace commands. `bindings/python-native/` is a standalone maturin crate (plain cargo can't link the macOS extension cdylib); `bindings/python-client/` + `bindings/typescript-client/` are stdlib-only / zero-dep remote SDKs with generated tables drift-checked from the OpenAPI contract.

## Where to look (read the overview first)

`architecture/overview.md` is the index; its deep dives are the review handoffs (no `.skills/` directory): `core-fault-engine`, `server-runtime`, `control-plane-cli`, `protocol-contract`, `scenario-observability`, `toxiproxy-compat`, `eggfetch-integration`, `embedding-native`, `verification-qualification`, `tooling-distribution`.
User contracts: `docs/architecture.md`, `docs/configuration.md`, `docs/control-plane.md`, `docs/toxiproxy.md`, `docs/eggfetch.md`. Parity/verification contracts (not status): `plans/reference/`. Status: `plans/registry.md`.

## Commands (trust these, not guesses)

Full gate: `./scripts/check.sh` (= `fmt --check` + `clippy --workspace --all-targets --all-features -- -D warnings` + `cargo test --workspace --all-features` + `cargo doc --workspace --all-features --no-deps`). CI (`ci.yml`, 25-min timeout, `RUST_TEST_THREADS=4`, ubuntu/macos/windows) additionally runs `cargo audit --deny warnings` and `cargo deny check advisories licenses bans sources`, plus `language-clients` and `python-native` jobs.

Focused runs:

```sh
cargo test -p eggchaos-core --all-features
cargo test -p eggchaos-toxiproxy --all-features
cargo test -p eggchaos-eggfetch --all-features          # add --features http2 for the H2 profile
cargo run --manifest-path benchmarks/Cargo.toml --release
./scripts/benchmark_datagram.sh
./scripts/check_openapi.sh                             # protocol/OpenAPI drift (36 ops)
```

Qualification (release workflow): `release-smoke.sh` (fmt/clippy/test/doc/audit/deny/package + publish-order proof + artifact smoke), `qualify_fuzz.sh`, `qualify_toxiproxy_v2_12.sh`, `qualify_toxiproxy_post_v2_12.sh`, `qualify_eggfetch.sh`, `qualify_language_clients.sh`, `qualify_python_native.sh`, `release-artifact-smoke.sh`.

## Gotchas agents actually hit

- Toxiproxy differential needs the pinned oracle: `TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 ./scripts/qualify_toxiproxy_v2_12.sh`. Developer mode without a verified oracle reports `differential:incomplete` (exit 0) — that is not a pass. Compat server: `cargo run -p eggchaos-toxiproxy --example compat_server -- 127.0.0.1:8474`. Strict v2.12 is default/frozen; post-v2.12 `packet_loss` is opt-in pinned to `40f7fd31`.
- Post-v2.12 fetcher stdout contract: default and `--path-only` print exactly one executable path; `--json` prints one metadata record. `TOXIPROXY_POST_V2_12_SERVER="$(./scripts/fetch_toxiproxy_post_v2_12.sh)"` works as command substitution. Mandatory mode: `EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1 ./scripts/qualify_toxiproxy_post_v2_12.sh`.
- Fuzz: `cargo-fuzz 0.13.2` cannot build under pinned 1.89 (transitive `cargo-platform` needs rustc 1.91). Release workflow installs it with `RUSTUP_TOOLCHAIN=stable`; the fuzz target itself still builds under 1.89 with `--sanitizer none`. See `release.yml`.
- Publish order matters (intra-workspace deps use `version = "0.1.0"` registry reqs): `core -> experiment/eggfetch -> protocol -> server/toxiproxy/cli -> embed`. `scripts/release-smoke.sh` asserts this order-proof. `eggchaos-native` stands outside the workspace; maturin owns its build.
- Directions are `upstream` (client→target) and `downstream` (target→client). Faults wrap destination writes; reads stay pass-through.
- Native control is versioned under `/v1` except `GET /metrics` (Prometheus text, no prefix). Request bodies capped at 1 MiB. CLI: `eggchaos --admin <url> [--json] <command>`; every command emits one JSON doc with `--json` and exits nonzero on failure. Route/CLI inventory: `docs/control-plane.md`.
- Admin binds loopback by default. Non-loopback requires explicit public-admin opt-in + bearer token; failures return bounded JSON without echoing the token.
- All limits are bounded (queues, connections, bodies, histories, metrics cardinality). Default overflow is backpressure (`Pending`), never silent loss unless the fault documents discard. `poll_flush` is the delivery barrier; `poll_write` success only means the engine owns the bytes.
- Determinism: SplitMix64-v1, versioned seeds from `(seed namespace, proxy identity, connection key, direction, fault id)`. No process-global or scheduler-order RNG. ScenarioV1 namespaces derive from `(scenario seed, run id, event index)`; scenario-v2 from `(scenario seed, execution key, schedule fingerprint, compiled event index)` with no run_id; replay is exact for policy/per-key decisions, not live timing (connection keys depend on accept order).
- Native userspace TCP byte-chunk dropping is named `stream-loss`; only the Toxiproxy compatibility presentation may call the post-v2.12 spelling `packet_loss`. It is not IP/TCP packet loss and must never reuse ADR 003 datagram-loss semantics. `reset_peer`/hard-reset is best-effort and platform-qualified (RST vs FIN not asserted); ordinary `poll_shutdown` is never advertised as TCP RST.
- Dep discipline: `eggress-relay` not `eggress-embed`; `eggserve-server`+`eggserve-primitives` not `eggress-admin`/`eggserve-core`; `eggfetch-core` minimal features; `eggress-outbound` only behind an optional feature with proven demand. Don't copy sibling-repo code when a published Eggstack crate provides the primitive.
- SDK drift: regenerate via `scripts/sync_sdk_contract.py`, then assert `git diff --exit-code` on `bindings/_contract/operations.json` + both generated tables (`check_python_client.sh` / `check_typescript_client.sh` enforce this).

## Planning state

M000–M041 are closed. M019 (`ca527db`) remains the final pre-tag authority; M041 (`724b967`, hosted run `36219464594` 13/13) is the latest ADR 007 corrective authority. Performance implementation tranche M042–M045 is closed; M046 is the closed authority for reconstructing historical performance evidence; M047 is `ready` as the sole performance-artifact provenance hardening handoff. M047 must use one provenance definition across stream/probe/datagram reports, allow dirty exploratory runs only as non-authoritative with a source fingerprint, and require a clean source-relevant tree for exact-candidate retained evidence. It must not retune thresholds or rewrite historical artifacts. Do not call a performance artifact "exact-candidate evidence" unless its `provenance.authoritative == true` and its `provenance.head_sha` is the candidate under discussion (M047 schema; pre-M047 artifacts use the M046 provenance map). Do not rewrite `plans/archive/` history. Do not start a generic C ABI, Node native addon, JNI, P/Invoke, cgo, UniFFI, or WASM without a separate ADR after demonstrated multi-consumer demand; do not add `eggreplay-*`/`eggprobe-*` production dependencies (downstream adoption only).

If the owner asks for new work: `plans/roadmap.md` is the architecture authority, `plans/reference/` holds parity/verification contracts (not status), ADRs live in `plans/adrs/`. Any new numbered plan needs objective, baseline/deps, scope + non-goals, affected crates, ordered work packages, invariants/failure semantics, test commands, acceptance criteria, stop conditions, closure evidence, and follow-on rules — and must update `plans/registry.md` in the same change. Never mark `closed` from source inspection; closure requires running the plan's tests on the exact candidate plus external/differential evidence where declared.

## Verification

Prefer deterministic Tokio-time tests; wall-clock assertions need justified tolerances and must not be sole evidence. Cover: fault state machines, byte-conservation where faults preserve bytes, half-close/shutdown, bounded-buffer/backpressure, RNG golden vectors, exact JSON/TOML round trips, OpenAPI drift (21 paths / 36 ops), Toxiproxy differential (strict corpus 50/50 vs pinned v2.12.0), no-fault throughput/latency vs bare `eggress-relay`, exact datagram traces and the measured fixed-target UDP budget. Record un-runnable oracles/platforms as incomplete evidence. Authoritative semantics: `docs/architecture.md`, `docs/configuration.md`, `docs/control-plane.md`, `docs/toxiproxy.md`, `docs/eggfetch.md`.
