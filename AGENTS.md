# AGENTS.md

Rust workspace (edition 2021, pinned `1.89.0` in `rust-toolchain.toml` + CI). `unsafe_code = "forbid"` at workspace level — no new `unsafe` without an explicit ADR.

## Layout — dependency direction is inward

```text
eggchaos-cli -> eggchaos-server -> eggchaos-core (-> Tokio byte streams)
eggchaos-toxiproxy -> server/core (adapter, no separate state store)
eggchaos-eggfetch -> core (implements `eggfetch_core::Dialer`)
```

- `eggchaos-core` (`crates/eggchaos-core/src/`): protocol-neutral `FaultPlan` + `ChaosStream<T>` write-side state machine. Knows nothing about HTTP, listeners, CLI, Toxiproxy, Eggfetch. Empty plan delegates without allocating queue/timer.
- `eggchaos-server`: fixed-target TCP listeners only (never a forward proxy). `eggress-relay` owns bidirectional copy + half-close — do not fork its semantics. Admin H1 runtime is `eggserve-server` + `eggserve-primitives`; `native.rs` owns the explicit `/v1` DTO/conversion boundary.
- `eggchaos-cli`: thin adapter; control HTTP via `eggfetch-core` (minimal features). Never a second networking/state path.
- `eggchaos-toxiproxy`: v2.12 REST adapter over native `ControlState`. `eggchaos-eggfetch`: physical-stream `Dialer`; Eggfetch keeps HTTP/TLS/SNI/pooling. Per-request chaos is out of scope.
- `benchmarks/` and `fuzz/` are separate crates/workspaces with their own manifests — don't run them via workspace commands.

## Where to look (read the overview first)

`architecture/overview.md` is the index; each file below is the review handoff for one area (no `.skills/` directory exists — these deep dives serve that role):

- `architecture/core-fault-engine.md` — plan/engine/stream/policy/rng semantics, write/flush contract, golden vectors.
- `architecture/server-runtime.md` — listeners, `eggress-relay` embedding, `ControlState` authority, full bounds table.
- `architecture/control-plane-cli.md` — `/v1` route inventory, schema-v1 TOML, CLI command matrix, auth/bounds.
- `architecture/scenario-observability.md` — scenario driver, evidence/snapshot/metrics types, replay limits.
- `architecture/toxiproxy-compat.md` — toxic↔fault table, defaults/naming precedence, divergences, 47/47 evidence.
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
```

Qualification scripts (release workflow): `scripts/qualify_fuzz.sh`, `scripts/qualify_toxiproxy_v2_12.sh`, `scripts/qualify_eggfetch.sh`, `scripts/release-smoke.sh`, `scripts/release-artifact-smoke.sh`.

## Gotchas agents actually hit

- Toxiproxy differential needs the pinned oracle: `TOXIPROXY_SERVER=/path/to/v2.12.0 ./scripts/qualify_toxiproxy_v2_12.sh`. Without a `2.12.0` binary it reports `differential:incomplete` (exit 0) — that is not a pass. Compat server: `cargo run -p eggchaos-toxiproxy --example compat_server -- 127.0.0.1:8474`.
- Fuzz: `cargo-fuzz 0.13.2` cannot build under pinned 1.89 (transitive `cargo-platform` needs rustc 1.91). Release workflow installs it with `RUSTUP_TOOLCHAIN=stable`; the fuzz target itself still builds under 1.89 with `--sanitizer none`. See `release.yml`.
- Publish order matters (intra-workspace deps use `version = "0.1.0"` registry reqs): `core -> eggfetch -> server/toxiproxy/cli`. `scripts/release-smoke.sh` asserts this order-proof.
- Directions are `upstream` (client→target) and `downstream` (target→client). Faults wrap destination writes; reads stay pass-through.
- Native control is versioned under `/v1` except `GET /metrics` (Prometheus text, no prefix). Request bodies capped at 1 MiB. CLI: `eggchaos --admin <url> [--json] <command>`; every command emits one JSON doc with `--json` and exits nonzero on failure. Route/CLI inventory: `docs/control-plane.md`.
- Admin binds loopback by default. Non-loopback requires explicit public-admin opt-in + bearer token; failures return bounded JSON without echoing the token.
- All limits are bounded (queues, connections, bodies, histories, metrics cardinality). Default overflow is backpressure (`Pending`), never silent loss unless the fault documents discard. `poll_flush` is the delivery barrier; `poll_write` success only means the engine owns the bytes.
- Determinism: SplitMix64-v1, versioned seeds derived from `(seed namespace, proxy identity, connection key, direction, fault id)`. No process-global or scheduler-order RNG. Scenario namespaces derive from `(scenario seed, run id, event index)`; replay is exact for policy/per-key decisions, not live timing (connection keys depend on accept order).
- Never call stream-chunk dropping "packet loss" in native APIs/docs. `reset_peer`/hard-reset is best-effort and platform-qualified (RST vs FIN not asserted); ordinary `poll_shutdown` is never advertised as TCP RST.
- Dep discipline: `eggress-relay` not `eggress-embed`; `eggserve-server`+`eggserve-primitives` not `eggress-admin`/`eggserve-core`; `eggfetch-core` minimal features; `eggress-outbound` only behind an optional feature with proven demand. Don't copy sibling-repo code when a published Eggstack crate provides the primitive.

## Planning state

M000–M017 and M008 are closed milestones. The 2026-09-23 post-M015 corrective chain is `M016 (closed) -> M017 (closed) -> M018 (active) -> M019 (blocked)`. M015 remains historical qualification for `cd88b22`, but it is no longer the final tag authority. No tag, crates.io publish, or GitHub release should occur until M019 closes. Do not rewrite `plans/closure/` / `plans/archive/` history.

If the owner asks for new work: `plans/roadmap.md` is the architecture authority, `plans/reference/` holds parity/verification contracts (not status), ADRs live in `plans/adrs/`. Any new numbered plan needs objective, baseline/deps, scope + non-goals, affected crates, ordered work packages, invariants/failure semantics, test commands, acceptance criteria, stop conditions, closure evidence, and follow-on rules — and must update `plans/registry.md` in the same change. Never mark `closed` from source inspection; closure requires running the plan's tests on the exact candidate plus external/differential evidence where declared.

## Verification

Prefer deterministic Tokio-time tests; wall-clock assertions need justified tolerances and must not be sole evidence. Cover: fault state machines, byte-conservation properties where faults preserve bytes, half-close/shutdown, bounded-buffer/backpressure, RNG golden vectors, exact JSON/TOML round trips, Toxiproxy differential (47/47 vs pinned v2.12.0), no-fault throughput/latency vs bare `eggress-relay`. Record un-runnable oracles/platforms as incomplete evidence. Authoritative semantics: `docs/architecture.md`, `docs/configuration.md`, `docs/control-plane.md`, `docs/toxiproxy.md`, `docs/eggfetch.md`.
