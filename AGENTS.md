# AGENTS.md

This repository uses an evidence-first planning and implementation workflow.

## Planning authority

The canonical planning surface is `plans/`.

- `plans/roadmap.md` defines long-term architecture, sequencing, invariants, non-goals, and release gates.
- `plans/registry.md` is the compact source of truth for milestone status, dependencies, activation, and closure.
- `plans/000-architecture-and-scope-baseline.md` records the initial investigated baseline and boundaries.
- Numbered implementation plans are executable handoffs. Their filename prefix is the milestone sequence number and must not be reused.
- `plans/adrs/` records durable architectural decisions.
- `plans/reference/` contains parity matrices and verification contracts, not implementation status.
- `plans/closure/` is reserved for independent closure evidence after implementation.
- `plans/archive/` is for superseded planning material only; do not delete historical evidence to make the active state look cleaner.

When implementation begins, update `plans/registry.md` in the same change that activates, blocks, closes, or supersedes a milestone. The registry must not claim closure from source inspection alone.

## Status vocabulary

Use only these milestone states unless a plan explicitly defines a narrower sub-state:

- `ready`: dependencies are satisfied and the plan can be handed to an implementer.
- `blocked`: a named dependency or evidence gate is unresolved.
- `active`: implementation is in progress.
- `implemented-awaiting-evidence`: code appears complete but closure evidence is incomplete.
- `closed`: acceptance criteria and required verification evidence are satisfied.
- `superseded`: replaced by a named successor plan.

A blocked milestone must name the blocking milestone or evidence gap. A closed milestone must point to a closure record or exact verification evidence.

## Handoff plan requirements

Every numbered plan must contain:

1. objective and user-visible outcome;
2. baseline and dependencies;
3. scope and explicit non-goals;
4. affected crates/modules or expected files;
5. ordered implementation work packages;
6. behavioral invariants and failure semantics;
7. test and verification commands;
8. acceptance criteria;
9. rejection/stop conditions;
10. evidence required for closure;
11. follow-on activation rules.

Do not collapse a broad milestone into “implement feature X.” An implementation agent should be able to execute the plan without rediscovering the architecture.

## Project invariants

Eggchaos is a small Rust-native fault-injection substrate and fixed-target chaos proxy. Preserve these boundaries unless an ADR changes them:

- The core fault engine is protocol-neutral and operates on Tokio-compatible byte streams.
- The standalone proxy is fixed-target. It is not a general forward proxy.
- HTTP semantics do not belong in the stream-fault engine.
- `eggress-relay` remains the bidirectional relay authority; eggchaos must not fork its half-close/copy semantics.
- `eggress-testkit` should be reused for echo, half-close, fragmentation, and transport fixtures where practical.
- `eggserve-server` / `eggserve-primitives` are the preferred native admin HTTP substrate; do not depend on `eggress-admin` merely to obtain an HTTP server.
- `eggfetch-core` is the preferred HTTP client for the CLI control path and the public `Dialer` seam for in-process HTTP chaos integration.
- `eggress-outbound` is optional/future integration for chained upstream routes; it is not an MVP dependency.
- No unsafe Rust is introduced without a separate explicit ADR and narrow audit.
- All queues, connection counts, body sizes, and fault buffers are bounded.
- Randomized chaos is reproducible from explicit, versioned seeds. Do not use process-global or scheduler-order RNG state.
- Stream-chunk dropping is not described as real IP/TCP packet loss in native APIs. Toxiproxy compatibility may retain upstream naming while documenting the semantic distinction.
- Native control/admin listeners bind to loopback by default. Non-loopback admin exposure requires an explicit opt-in and authentication policy.
- Machine-readable JSON is a first-class CLI/control-plane contract.

## Dependency discipline

Target Rust 1.89+ and edition 2021 initially to align with current Eggstack networking crates.

Prefer the smallest direct Eggstack dependency:

- `eggress-relay` rather than `eggress-embed` for plain byte relay;
- `eggserve-server` + `eggserve-primitives` rather than `eggserve-core` when H1 application serving is sufficient;
- `eggfetch-core` with minimal features rather than Python/CLI adapters;
- `eggress-outbound` only behind an explicit optional feature when a non-direct upstream route is required.

Do not copy implementation code from sibling repositories when a stable published crate surface already provides the needed primitive.

## Verification discipline

Use deterministic Tokio-time tests where possible. Any timing assertion against wall clock must use a justified tolerance window and must not be the sole evidence for correctness.

At minimum, closure work should consider:

- unit tests for every fault state machine;
- property tests for byte conservation where the configured fault should preserve bytes;
- half-close and shutdown tests;
- bounded-buffer/backpressure tests;
- deterministic RNG golden vectors;
- exact JSON/config round trips;
- differential tests against Toxiproxy v2.12.0 for declared compatibility;
- no-fault throughput/latency regression measurement against bare `eggress-relay`;
- Linux/macOS/Windows CI for supported behavior, with platform-specific reset semantics called out;
- `cargo fmt --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`, `cargo test --workspace --all-features`, and dependency/security checks once the workspace exists.

When a platform or external oracle cannot be run, record that as incomplete evidence. Do not replace missing execution with an assertion that the code looks correct.
