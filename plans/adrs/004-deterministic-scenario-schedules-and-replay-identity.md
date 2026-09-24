# ADR 004 — Deterministic Scenario Schedules and Replay Identity

Status: accepted  
Date: 2026-09-24

## Context

Scenario v1 is intentionally small: an ordered list of relative-time events applies complete stream or datagram directional plans, or removes one fault. M011 made those runs owned, cancellable, observable, generation-guarded, and seed-effective; M022 added explicit datagram actions without collapsing transport semantics.

The roadmap now calls for richer deterministic scenarios and time-varying fault schedules. Expanding the current event loop directly would create several correctness problems:

- v1 waits event-to-event, so time spent applying an earlier event can accumulate into later schedule drift even though at_ms is defined from scenario start;
- the current scenario policy namespace includes daemon-assigned run_id, so the same portable file may produce a different random realization merely because unrelated runs happened first;
- long-running schedules need an explicit answer for concurrent manual mutation, cancellation, failure, and restoration of the pre-run state;
- continuous clock-dependent fault parameters would push scheduling semantics into the stream and datagram engines and make replay depend on admission timing;
- source conveniences such as named phases and finite repetition need hard expansion bounds before they reach the runtime.

ADR 002 remains authoritative for fault-local deterministic RNG and byte-safe stream live mutation. ADR 003 remains authoritative for datagram admission-time snapshots and whole-datagram semantics. This ADR defines only the higher-level scenario schedule boundary.

## Decision: compile richer schedules into the existing publication machinery

Scenario v2 is a control-plane scheduling layer above both fault engines. It does not add a second impairment engine and it does not make eggchaos-core wall-clock aware.

The conceptual path is:

    ScenarioScheduleV2 source
        -> validate and deterministically expand
        -> CompiledScenarioV2 ordered event tape
        -> owned scenario supervisor
        -> existing stream/datagram generation publication
        -> existing fault engines

ScenarioV1 remains a supported compatibility surface. Its JSON shape, actions, run lookup/cancel routes, and current seed derivation remain versioned historical behavior unless a separately documented compatibility-safe correction is made. The v2 schedule path gets a distinct replay identity so portable schedules do not inherit daemon run ordering.

## Schedule-time authority

A v2 run captures one Tokio monotonic Instant epoch when the owned driver starts. Every compiled event has an absolute offset from that epoch.

The driver waits for:

    deadline(event) = epoch + event.offset

using sleep_until semantics. It must never implement v2 by sleeping the delta from the previous event completion.

If application work makes a deadline already due, the event executes immediately in stable event order and records lateness. Scheduler lateness is diagnostic evidence, not an input to random decisions.

Absolute wall-clock time, time zones, cron rules, DST, and persistent calendar scheduling are not part of this subsystem.

## Source model and deterministic expansion

ScenarioScheduleV2 is a bounded source language for piecewise-constant fault conditions. The initial language supports:

- named sequential phases with explicit durations;
- one or more existing stream/datagram scenario actions at a phase boundary;
- finite repetition of a bounded phase group;
- explicit seed and execution_key;
- isolation policy and cleanup policy.

Continuous per-packet/per-write interpolation is not part of v2. A time-varying profile is represented by deterministic keyframes/phase boundaries that publish ordinary plans. Smooth-ramp sugar may be added later only if it expands before execution into the same bounded compiled event tape.

The compiler must preserve document order for equal deadlines. Every expanded event receives a stable zero-based compiled index. Expansion arithmetic is checked and bounded. The initial compiled-event ceiling is 1024, matching the existing v1 event bound; any future increase requires measured memory/runtime evidence rather than silent expansion.

Source nesting/repetition must also have a small explicit structural bound and must fail validation before a run task is created if expansion would exceed limits.

## Portable replay identity

V2 separates observation identity from deterministic execution identity:

- run_id is daemon-assigned and exists only to inspect/cancel a live or retained run;
- execution_key is part of the schedule document or explicit apply request and selects a repeatable realization;
- schedule_fingerprint identifies the canonical compiled schedule semantics;
- compiled_event_index identifies one event inside that schedule.

V2 policy seed namespaces are derived from stable inputs equivalent to:

    scenario seed
    + execution key
    + schedule fingerprint/domain
    + compiled event index
    + derivation version

and never from run_id, task scheduling, wall-clock timestamps, or hash-map iteration order.

M026 must freeze the exact canonical fingerprint algorithm, schedule seed-derivation encoding, and golden vectors before implementation closure. The fingerprint should be a stable SHA-256 digest over a versioned canonical compiled representation; it is diagnostic/replay identity, not authentication.

Changing seed or execution_key intentionally selects another deterministic realization. Running the same schedule after unrelated runs must not.

## Isolation semantics

V2 has two explicit isolation modes.

strict is the default for deterministic schedule files. At run start, every touched directional policy is snapshotted with its generation. Each later publication must compare against the generation last owned by that schedule. An external manual/scenario publication moves the resource and causes the next conflicting v2 action to fail rather than silently incorporating or overwriting external state.

live retains the v1-style interactive model: each event builds from the currently live plan at fire time and uses an expected-generation guard for the publication attempt. External mutation may therefore become the base for a later action, but a concurrent move during the publication still conflicts.

Neither mode introduces a global data-plane mutex.

## Cleanup semantics

V2 defines an explicit cleanup policy:

- restore-initial is the default;
- leave preserves the last successfully published state.

For restore-initial, the run snapshots the initial state of each touched directional policy before the first event. Completion, cancellation, or failure attempts restoration through the same generation/CAS authority. Cleanup must never overwrite a generation that the run no longer owns. A cleanup conflict is retained as evidence and leaves the externally-owned state intact.

Already accepted stream bytes remain governed by ADR 002 barrier-transition semantics. Already admitted datagrams remain governed by ADR 003 generation snapshots. Cleanup is a policy publication, not retroactive reevaluation of in-flight data.

## Failure semantics

V2 remains fail-fast in its initial form.

- validation/expansion failure creates no run;
- a publication conflict or missing target fails the run;
- cancellation interrupts deadline waits promptly;
- already successful events are not rolled back by rewriting history;
- cleanup then executes according to the selected cleanup policy;
- cleanup failure/conflict is separately observable from the original run failure.

There is no continue-on-error mode in the initial v2 language.

## Transport boundary

Compiled events keep stream and datagram actions explicit. A stream plan always contains FaultSpec semantics; a datagram plan always contains DatagramFaultSpec semantics. The compiler may share schedule timing and lifecycle types, but it must not introduce a transport-generic fault union that weakens ADR 003.

The initial v2 action vocabulary is the four already-proven scenario mutations: set stream plan, remove stream fault, set datagram plan, and remove datagram fault. Proxy/listener lifecycle actions, connection-ID actions, shell commands, callbacks, predicates, arbitrary code, and protocol-aware behavior are outside the initial tranche.

## Control and artifact boundary

The native HTTP API remains JSON-first. ScenarioV1 remains accepted by the existing apply route. M027 may add additive validate/compile operations and accept a version-2 schedule through the same versioned scenario family, provided v1 fixtures remain byte/behavior compatible.

The CLI may accept JSON and TOML source files, but parsing is the only CLI-side responsibility. Validation, expansion, fingerprinting, and execution semantics must live in reusable server/library code so the CLI never becomes a second scheduler.

A compile operation must be able to return the normalized event tape and schedule fingerprint without executing it.

## Evidence contract

V2 run evidence must be sufficient to identify what was scheduled and what actually happened without capturing payloads. At minimum retain:

- run_id, seed, execution_key, schedule fingerprint, and schedule/compiler semantics version;
- isolation and cleanup policy;
- compiled event index and optional phase identity;
- scheduled offset;
- actual monotonic elapsed application time and lateness;
- action summary and target resource;
- resulting global/policy generations;
- cleanup requested/result, including conflict or failure.

Run IDs, schedule fingerprints, event IDs, phase names, peer addresses, and arbitrary fault IDs must not become Prometheus labels.

## Consequences

Positive:

- richer schedules reuse the already-qualified fault engines and publication barriers;
- portable replay no longer depends on daemon run ordering;
- schedule drift does not accumulate from event execution time;
- cancellation/failure has an explicit non-clobbering restoration policy;
- compiled artifacts are inspectable and golden-testable;
- stream and datagram semantics remain separate.

Costs:

- v1 and v2 scenario semantics coexist;
- the server gains a compiler plus more run evidence/state;
- strict ownership requires per-resource expected-generation tracking;
- source conveniences are constrained by deterministic expansion bounds.

These costs are accepted because moving scheduling into either data-plane engine would make correctness and replay materially harder.

## Rejected alternatives

### Put time-varying parameters directly in eggchaos-core

Rejected. Per-write/per-datagram clock evaluation would couple random/fault decisions to runtime timing and duplicate scheduling logic across stream and datagram engines.

### Keep run_id in v2 seed derivation

Rejected. run_id is an observation handle whose value depends on daemon history, not portable experiment identity.

### Sleep deltas between completed events

Rejected. Event application latency accumulates schedule drift.

### Allow unbounded loops or general expressions

Rejected. Expansion/work must be known and bounded before execution.

### Roll back by unconditional state replacement

Rejected. It can overwrite legitimate external/manual mutations after the schedule started.

### Add cron/calendar scheduling

Rejected from this subsystem. Recurring operational job scheduling has timezone, persistence, restart, and DST semantics that are distinct from deterministic intra-run timing.