# Eggchaos Planning

This directory is the canonical planning record for eggchaos.

Eggchaos is intended to be a small Rust-native successor to the useful core of Toxiproxy: a fixed-target TCP fault-injection proxy with deterministic, directional stream impairment. Its differentiator inside Eggstack is not another proxy stack. It is a reusable fault engine that composes with existing Eggstack networking substrates.

## Planning layout

| Path | Purpose |
| --- | --- |
| `roadmap.md` | Long-term architecture, sequencing, release stages, and future extensions. |
| `registry.md` | Current milestone status and dependency source of truth. |
| `000-architecture-and-scope-baseline.md` | Investigated baseline, reuse decisions, scope boundaries, and initial dependency graph. |
| `001-*.md` ... `041-*.md` | Bounded implementation, corrective, cleanup, feature, qualification, maintenance, richer-scenario, integration-boundary, language-binding, post-v2.12 compatibility, and closure handoffs in execution order. |
| `adrs/` | Durable architecture decisions that should not be silently changed by implementation. |
| `reference/toxiproxy-parity.md` | Compatibility target and semantic mapping. |
| `reference/verification-matrix.md` | Required evidence across faults, platforms, APIs, and performance. |
| `closure/` | Closure evidence created after milestones are implemented. |
| `archive/` | Superseded plans retained for historical traceability. |

## Current execution order

M000–M015 and M008 remain closed historical work. A 2026-09-23 post-M015 repository audit found additional correctness, secure-control, contract-maintenance, and qualification gaps, so a new release-blocking chain is registered:

`M016 correctness/security -> M017 native contract/operator surface -> M018 runtime modularization -> M019 final corrective requalification`

M016–M019 are closed. M019 completed the pinned Toxiproxy oracle gate and is the final exact-candidate release qualification authority. The owner may proceed with the v0.1.0 tag, crates.io publication, and GitHub release as separate release actions.

The first activated post-release feature tranche was UDP/datagram impairment under ADR 003: `M020 -> M021 -> M022 -> M023`. All four plans are formally closed with exact-candidate evidence in `plans/closure/`; M023 qualified candidate `ae2ab733b2be199d7693e40cdc558df01ee9a9de`. This does not alter M019's historical v0.1.0 authority.

M024 is closed after a semantics-preserving datagram hot-path performance and runtime-maintainability pass. It added the topology-matched bare relay benchmark, heap scheduler, immediate empty-plan emission, association setup outside the registry lock, and internal datagram runtime decomposition.

M025 is closed on exact candidate `55911f6`; evidence is in
`plans/closure/M025-datagram-association-setup-waiter-and-closure-hygiene-closure.md`.
It replaced the `Starting` association path's bounded `yield_now()` retry loop
with retained/event-driven waiting, proved setup/drain/capacity races with
controlled tests, and reconciled closure/status wording.

ADR 004's richer deterministic-scenario/time-varying schedule tranche is
complete: `M026 (closed) -> M027 (closed) -> M028 (closed)`. M026 froze the
bounded scenario-v2 source/compiler and portable replay identity; M027 wired it
to the existing owned scenario/ControlState runtime; M028 qualified the
combined surface on an exact candidate. ScenarioV1 remains a compatibility
surface, and no schedule logic was moved into the stream/datagram fault
engines. Later schedule work (ramps, predicates, lifecycle actions, cron)
requires separate planning and must compile to or compose with this bounded
model rather than bypass it.

ADR 005's cross-project integration-boundary/harness tranche is complete:
`M029 (closed) -> M030 (closed) -> M031 (closed)`. M031 qualified exact
candidate `fa189b9` and recorded the closure-backed EggReplay/EggProbe
handoff seams. No EggReplay/EggProbe production dependency belongs in
eggchaos.

ADR 006's language-binding feature tranche is complete:
`M032 (closed at ed05f68) -> M033 (closed at 429d459) -> M034 (closed at
991818b)`. The protocol/OpenAPI authority, Python + TypeScript remote SDKs,
safe `eggchaos-embed` facade, and PyO3/maturin Python-native pilot are
implemented. A generic C ABI remains explicitly deferred.

A post-closure audit registered one bounded corrective successor, now closed:
`M035 (closed at a710cd6)`. It fixed the hosted
language-client false-red cleanup exit, added portable hosted native-Python
qualification, consolidated duplicated datagram fault mutation semantics between
HTTP and embed, reconciled current-state planning, and obtained a green exact-head
hosted qualification result. M035 does not rewrite M032–M034 history.

ADR 007's M036–M041 implementation chain and M040 corrective
qualification are historical. M040 closed on exact candidate
`48fe0dd8dfc1f995c53a3b1661fe704dbdc5bce0` with green local/oracle/hosted
evidence. A post-closure audit found one narrow operator-facing regression
in the M040 metrics renderer (duplicate stream-loss series plus a literal
`\\n` separator), along with oracle-fetch stdout-contract and planning
hygiene drift. M041 closed on exact candidate
`724b967da04579282dd8bfc7a81dc4fe55d034a2` with green local/oracle/hosted
evidence (hosted run `36219464594`, 13/13 jobs).

M041 fixes only those metrics/tooling/closure issues and is the latest ADR
007 repository-level authority. M040 remains preserved as historical evidence.
M015 remains valid qualification evidence for `cd88b22`; M019 is the final
pre-tag release-candidate authority at `ca527db`.

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

The first release is TCP byte-stream focused. UDP/datagram chaos is implemented as the post-release tranche under ADR 003 (M020–M023), with M024/M025 performance and setup-hygiene successors closed, and remains outside the first-release/M019 historical scope. Richer deterministic scenarios are complete under ADR 004 + M026–M028. The consumer-neutral cross-project integration substrate is complete under ADR 005 + M029–M031; EggReplay/EggProbe product-specific adoption remains downstream work. Cross-language control/embedding is implemented under ADR 006 + M032–M034 and correctively qualified under M035 (closed at `a710cd6`: hosted SDK/native-Python qualification, shared datagram mutation authority, exact-head closure). ADR 007 + M036–M039 implement the post-v2.12 deterministic stream-loss/`packet_loss` line; M040 closed at `48fe0dd` as its corrective qualification authority. Native stream loss remains distinct from ADR 003 datagram loss and strict v2.12 remains frozen. A generic C ABI remains deferred. Arbitrary outbound proxy chains remain a later roadmap item.

## Research baseline

Initial planning was researched on 2026-09-22 against:

- Shopify Toxiproxy latest release v2.12.0 and current `main`;
- Toxiproxy REST API and toxic implementation guidance;
- `eggstack/eggress` current workspace, especially `eggress-relay`, `eggress-testkit`, and `eggress-outbound`;
- `eggstack/eggfetch` `eggfetch-core` 0.2.0 and its public `Dialer` API;
- `eggstack/eggserve` 0.2.0 leaf generic H1 server/primitives;
- contemporary Rust stream-adapter chaos designs such as Trixter/tokio-netem for comparison, not as dependencies.

See the baseline and reference documents for exact conclusions.


## Datagram research update

On 2026-09-24 the UDP/datagram roadmap item was re-researched against current eggchaos `main`, current Eggress UDP/runtime surfaces (especially `eggress-udp` and its fixed-target compatibility behavior), Tokio UDP receive semantics, and Linux `tc netem` as a semantic reference rather than an implementation dependency. ADR 003 and M020–M023 encode the resulting boundary and execution order.

## Language binding research update

On 2026-09-25 the post-v1 language-binding direction was reviewed against the closure-backed M017/M026–M031 boundaries and current Rust/Python binding tooling. ADR 006 + M032–M034 encode the resulting order: native `/v1` protocol/OpenAPI authority first, Python + TypeScript remote SDKs second, and a coarse safe Rust facade + PyO3/maturin Python embedding pilot third. Direct binding of Tokio stream internals and a generic C ABI are intentionally deferred.


## Post-v2.12 Toxiproxy research update

On 2026-09-25 the post-v2.12 roadmap item was re-researched against
Shopify/Toxiproxy v2.12.0 and current `main` at
`40f7fd31bee529d824116bd2a11a9e3425e904ec`. The compared branch is 86
commits ahead of v2.12.0; the substantive new server data-plane toxic is
`packet_loss`, introduced at
`7c01129a8c232bf01aaebaca8a87429fd16f69b2`. ADR 007 freezes the
Eggchaos boundary: deterministic userspace stream loss is a native core
primitive, not real TCP/IP packet loss and not ADR 003 datagram loss; strict
v2.12 remains a distinct default profile; post-v2.12 compatibility uses a
pinned snapshot oracle rather than moving `main`.
