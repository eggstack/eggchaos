# M043 — Stream Hot-Path Allocation, Policy, Timer, and Evidence Optimization

Status: blocked

Role: semantics-preserving TCP/stream hot-path optimization

Depends on: M042

## Objective

Reduce measured avoidable overhead in the stream fault path while preserving
the complete public API, deterministic fault behavior, live-generation
barriers, evidence semantics, and Eggress/Eggfetch integration contracts.

Implementation is evidence-gated by M042. Source-level candidates below are
not permission to implement complexity that M042 classifies
`inconclusive` or `not-material`.

## Baseline and research findings

Current source inspection identified these candidate costs:

1. `ChaosStream::update_live()` clones `LivePolicy` and obtains an owned
   `Arc<PublishedPolicy>` via `load_full()` on every relevant I/O poll,
   even when the generation has not changed.
2. A published policy already owns `Arc<FaultPlan>`, but
   `ChaosStream::new_live()` and generation transitions clone the complete
   `FaultPlan` before `DirectionEngine` stores it.
3. `ArmedTimer` allocates a new boxed Tokio `Sleep` whenever a queue/
   termination/shutdown deadline changes.
4. Active latency/slice/stream-loss work repeatedly scans the ordered complete
   plan during accepted chunks, even though connection activation is frozen at
   engine construction.
5. `StreamEvidence::mirror_engine()` rewrites a broad set of atomics after
   many drive points, including calls where the engine state may not have
   changed.
6. Non-empty `poll_write_vectored()` joins every iovec into a fresh
   `Vec<u8>` before the bounded engine decides how much prefix it can own.

Version-specific implementation research confirms safe primitives exist:

- ArcSwap's borrowed `load()` is intended for short-lived local reads and
  avoids the owned-pointer work of `load_full()`; its guard must not be held
  over async yield points.
- Tokio `Sleep::reset()` changes a pinned sleep's deadline without creating
  new associated timer state.
- No new dependency is required for either mechanism.

## Scope

Primary:

- `crates/eggchaos-core/src/policy.rs`
- `crates/eggchaos-core/src/engine.rs`
- `crates/eggchaos-core/src/stream.rs`

Qualification/regression:

- `benchmarks/`
- `qualification/performance/`
- `crates/eggchaos-server` stream runtime tests
- `crates/eggchaos-eggfetch` qualification

## Non-goals

- no public API removal, rename, signature break, or semantic reinterpretation;
- no change to `PublishedPolicy` atomic snapshot semantics;
- no relaxation of bounded ownership/backpressure;
- no RNG algorithm/seed/domain/order change;
- no change to stream-loss 32 KiB logical grain or fragmentation independence;
- no custom allocator/object pool;
- no `unsafe`;
- no task-per-chunk/timer-per-chunk design;
- no replacement of `eggress-relay`;
- no optimization accepted solely because a microbenchmark improves while
  end-to-end regression evidence worsens.

## Work packages

### WP1 — Consume the M042 target matrix

Before production edits, copy into the M043 implementation/closure notes:

- exact M042 baseline candidate;
- stream cases/probes classified proven-material or low-cost-cleanup;
- frozen success/regression thresholds;
- targets explicitly excluded by M042.

If M042 proves a listed candidate is not material, omit it or record a
minimal/no-op disposition. Do not broaden M043 to compensate.

### WP2 — Cheap steady-state live-policy generation check

Optimize the no-publication path without changing publication/transition
semantics.

Preferred shape:

- add a crate-private `LivePolicy` helper that uses a short-lived
  `ArcSwap::load()` guard only to read the current generation;
- when the generation equals `ChaosStream::observed_generation`, drop the
  guard and return immediately;
- obtain an owned atomic snapshot only when a transition is actually required;
- never retain an ArcSwap guard in `ChaosStream`, `DirectionEngine`, a
  future, or across an inner transport poll that may return `Pending`.

Race semantics must remain equivalent to the existing polling boundary:
a publication racing after an equal-generation check may be observed on the
next poll, just as a publication racing after the current snapshot load is
not retroactively applied to an already-running poll.

Add controlled tests for publication before/after the fast check and for
drain-pending barriers.

Do not expose ArcSwap/guard types through the public API.

### WP3 — Share immutable plan ownership internally

Eliminate per-connection/per-transition deep plan clones where M042 confirms
construction/transition cost is material or classifies this as low-cost
cleanup.

Introduce an internal shared-plan constructor so live streams can pass the
already-published `Arc<FaultPlan>` into `DirectionEngine`.

Preserve existing public constructors:

- `DirectionEngine::new(FaultPlan, ...)`;
- `DirectionEngine::new_with_termination(FaultPlan, ...)`;
- `DirectionEngine::plan() -> &FaultPlan`;
- `ChaosStream::new(...)`.

Those may wrap an internal `Arc<FaultPlan>`. Callers must observe no
ownership/API change.

Validation remains at policy publication/public construction boundaries.

### WP4 — Compile active execution metadata once per engine generation

If M042 shows repeated plan interpretation is material, compile private
execution metadata during `DirectionEngine` construction.

Candidates include:

- first active slicer position/RNG source;
- ordered active latency stage indices;
- frozen effective slice-delay inputs;
- a boolean/compact ordered set for active stream-loss stages;
- active bandwidth/limit flags needed for activation accounting.

Hard requirement: preserve original plan order and fault-local RNG draw order.
Do not sort or combine stages in a way that changes deterministic output.

For stream loss, any compact representation must preserve each active fault's
independent RNG state, previous-drop correlation state, and exact original
plan order. Existing golden/property tests must remain byte-identical.

Activation counters and evidence must remain semantically identical.

### WP5 — Reuse pinned timer state

Replace repeated `Box::pin(tokio::time::sleep(...))` construction with a
reusable pinned `Sleep` where the M042 timer probe justifies it.

Required behavior:

- one cached sleep allocation may be created lazily;
- changed deadlines use `Pin<&mut Sleep>::reset(deadline)`;
- after readiness, clear the logical armed deadline but retain reusable timer
  storage;
- queue-head, finite-blackhole, disconnect, and slow-close deadlines remain
  independently keyed exactly as today;
- paused Tokio-time tests must pass unchanged.

Prefer `sleep_until(deadline)`/absolute reset semantics so recalculation
cannot introduce elapsed-duration drift.

### WP6 — Reduce redundant evidence mirroring without weakening immediacy

Only implement this work if M042 measures meaningful evidence overhead.

Preferred conservative mechanism:

- add an engine-local evidence revision/dirty generation that advances whenever
  observable engine evidence changes;
- let each `ChaosStream` remember the last mirrored revision;
- skip `mirror_engine()` atomic rewrites when the revision is unchanged;
- if still material, compare the last mirrored `EngineEvidence` and update
  only changed atomic fields.

Do not make evidence eventual. After a completed direct/faulted write, drain,
discard, transition, or termination publication, currently visible evidence
must still reflect that completed operation before returning to the caller.

Do not change the external snapshot structs or counter definitions.

### WP7 — Bound preserving-plan vectored-write copying

Implement only if the corrected M042 vectored profile proves material cost.

The current full-iovec join is the semantic reference.

An optimization may copy only the maximum prefix the current preserving engine
can own when that bound can be computed without consuming RNG or changing
fault decisions. At minimum:

- latency/bandwidth/slice/ordinary preserving plans may use the optimized path
  when exact ownership capacity/limit bounds are known;
- destructive blackhole/stream-loss compositions must retain the reference
  fallback unless an equivalence proof/test demonstrates identical accepted
  prefix, RNG progression, segmentation and evidence.

Never call `accept()` independently for each `IoSlice` if doing so changes
logical segmentation, jitter/slice RNG draws, activation counts, or
limit/termination boundaries.

Preserve caller-visible prefix semantics and `poll_write_vectored`
ordering.

### WP8 — Before/after stream qualification

Run the exact M042 corrected stream harness before and after production changes
on the same host/session class when practical.

Record:

- bare/static-empty/live-empty ratios;
- small-write live-policy profile;
- each implemented microprobe;
- preserving vectored profile if WP7 lands;
- stream-loss zero/intermediate/full evidence and throughput;
- transition cost if plan sharing/live fast check changes it.

Retain raw artifacts with exact SHAs.

## Required invariants

- every current public Rust item/signature remains source compatible;
- `PublishedPolicy` still publishes plan/generation/seed namespace as one
  atomic snapshot;
- generation transitions still drain already-owned preserving bytes before
  engine replacement;
- durable termination survives generation swaps;
- empty direct path still observes live generations before bypassing the
  engine;
- `poll_write` success still means bounded engine ownership, and
  `poll_flush` remains the preserving-byte delivery barrier;
- deterministic RNG golden vectors and stream-loss fragmentation independence
  remain exact;
- active fault identity/evidence semantics remain exact;
- no new production dependency without measured justification.

## Verification

Minimum:

```sh
./scripts/check.sh
./scripts/benchmark.sh
cargo test -p eggchaos-core --all-features
cargo test -p eggchaos-server --all-features
./scripts/qualify_eggfetch.sh
```

Run all M042 stream-focused case selectors/probes on the exact M043 candidate.

Also run focused tests covering:

- live empty -> faulted -> empty transitions;
- publication races at the transition boundary;
- old-generation queue drain before replacement;
- termination handle survival;
- latency/slice/bandwidth paused-time behavior;
- stream-loss golden/property/fragmentation tests;
- vectored-vs-scalar equivalence for every optimized plan class;
- evidence snapshots immediately after write/drain/transition.

## Acceptance criteria

M043 can close only when:

- each production change corresponds to an M042 proven-material or
  low-cost-cleanup target;
- all declared M042 stream regression budgets pass;
- at least one targeted avoidable-cost dimension meets the improvement
  threshold frozen by M042, unless M042 explicitly authorized a low-cost
  cleanup with no numeric gain requirement;
- static/live empty-plan behavior and all fault semantics remain identical;
- public API and native/wire surfaces are unchanged;
- RNG/golden/replay behavior is unchanged;
- Eggfetch qualification passes;
- no unresolved medium-or-higher correctness/performance finding remains;
- exact before/after raw evidence and closure metadata are retained.

## Rejection / stop conditions

Do not close M043 if:

- an ArcSwap guard survives across an await/yield or is stored long-term;
- plan sharing exposes `Arc<FaultPlan>` where callers previously owned
  `FaultPlan`;
- compiled stage metadata reorders RNG draws or fault composition;
- timer reuse releases bytes before the prior exact deadline contract;
- evidence becomes eventually consistent;
- vectored optimization changes accepted-prefix behavior for a supported plan;
- an optimization misses its M042 threshold and is retained without a clear
  maintainability justification;
- a historical performance budget is weakened to make the candidate pass.

## Closure evidence

Create:

`plans/closure/M043-stream-hot-path-allocation-policy-timer-and-evidence-optimization-closure.md`

Record exact candidate SHA, implemented/omitted target disposition, raw
before/after artifacts, public-surface invariants, test commands, host
limitations, and any follow-on debt.

## Successor activation

M043 closure alone does not activate M045. M045 becomes ready only after M043
and M044 are both closed (including evidenced no-op closure when M042 proves a
side has no justified production changes).
