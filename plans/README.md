# Eggchaos Planning

This directory is the canonical planning record for eggchaos.

Eggchaos is intended to be a small Rust-native successor to the useful core of Toxiproxy: a fixed-target TCP fault-injection proxy with deterministic, directional stream impairment. Its differentiator inside Eggstack is not another proxy stack. It is a reusable fault engine that composes with existing Eggstack networking substrates.

## Planning layout

| Path | Purpose |
| --- | --- |
| `roadmap.md` | Long-term architecture, sequencing, release stages, and future extensions. |
| `registry.md` | Current milestone status and dependency source of truth. |
| `000-architecture-and-scope-baseline.md` | Investigated baseline, reuse decisions, scope boundaries, and initial dependency graph. |
| `001-*.md` ... `019-*.md` | Bounded implementation, corrective, cleanup, and qualification handoffs in execution order. |
| `adrs/` | Durable architecture decisions that should not be silently changed by implementation. |
| `reference/toxiproxy-parity.md` | Compatibility target and semantic mapping. |
| `reference/verification-matrix.md` | Required evidence across faults, platforms, APIs, and performance. |
| `closure/` | Closure evidence created after milestones are implemented. |
| `archive/` | Superseded plans retained for historical traceability. |

## Current execution order

M000–M015 and M008 remain closed historical work. A 2026-09-23 post-M015 repository audit found additional correctness, secure-control, contract-maintenance, and qualification gaps, so a new release-blocking chain is registered:

`M016 correctness/security -> M017 native contract/operator surface -> M018 runtime modularization -> M019 final corrective requalification`

M016 is ready. M017–M019 remain blocked until their prerequisites close. Tagging, crates.io publication, and GitHub release creation are deferred until M019 closes.

Historical closure records remain preserved at their real candidate commits. M015 remains valid qualification evidence for `cd88b22`; M019 will become the final current release-candidate authority only after the new chain is implemented and independently qualified.

## Status rules

The registry uses the following states: `ready`, `blocked`, `active`, `implemented-awaiting-evidence`, `closed`, and `superseded`.

Only `ready` work should be handed to an implementation agent unless the purpose of the handoff is explicitly diagnostic. A blocked plan must remain blocked until its named prerequisites are evidenced.

Implementation does not close a milestone. Closure requires the plan's acceptance criteria plus reproducible evidence. Create a closure record under `plans/closure/` and update `registry.md`.

## Plan-writing conventions

Numbered plans are executable handoffs. Each plan must specify objective, baseline, dependencies, scope/non-goals, affected surfaces, ordered work packages, semantics, tests, acceptance criteria, rejection conditions, closure evidence, and successor activation.

Plans should prefer exact public crate seams and named behavior over speculative internal implementation. If a dependency surface changes before implementation, update the plan and registry rather than silently adapting and leaving stale planning behind.

## Baseline decisions

The initial architecture is deliberately narrow:

- `eggchaos-core`: reusable protocol-neutral directional fault engine over Tokio `AsyncRead + AsyncWrite`.
- `eggchaos-server`: fixed-target TCP listener/runtime and registry, using `eggress-relay` for bidirectional relay semantics.
- `eggchaos-cli`: operator CLI with JSON-first output and a local HTTP control client.
- `eggchaos-toxiproxy`: optional Toxiproxy v2.12.0 compatibility adapter.
- `eggchaos-eggfetch`: optional `eggfetch_core::Dialer` adapter for in-process HTTP fault injection.

The native admin plane should use the leaf `eggserve-server` + `eggserve-primitives` H1 substrate. The CLI should use a minimal `eggfetch-core` HTTP profile to call it. `eggress-admin` is not used because its state model is specific to Eggress routing, UDP, metrics, and reverse-proxy administration.

The first release is TCP byte-stream focused. UDP/datagram chaos, arbitrary outbound proxy chains, language bindings, and cross-project integration with eggreplay/eggprobe are roadmap items rather than MVP scope.

## Research baseline

Initial planning was researched on 2026-09-22 against:

- Shopify Toxiproxy latest release v2.12.0 and current `main`;
- Toxiproxy REST API and toxic implementation guidance;
- `eggstack/eggress` current workspace, especially `eggress-relay`, `eggress-testkit`, and `eggress-outbound`;
- `eggstack/eggfetch` `eggfetch-core` 0.2.0 and its public `Dialer` API;
- `eggstack/eggserve` 0.2.0 leaf generic H1 server/primitives;
- contemporary Rust stream-adapter chaos designs such as Trixter/tokio-netem for comparison, not as dependencies.

See the baseline and reference documents for exact conclusions.
