# M044 — Datagram Steady-State Synchronization and Association-Scale Optimization

Status: blocked

Role: semantics-preserving ADR 003 runtime scaling optimization

Depends on: M042

## Objective

Reduce measured steady-state UDP/datagram overhead at moderate/high
association counts without reopening the already-closed M024 scheduler work or
changing ADR 003 fault, association, ordering, evidence, and bounded-resource
semantics.

M042 is the measurement authority. Every nontrivial synchronization or
lifecycle change in this plan is conditional on a proven material target.

## Baseline and research findings

The M024 heap/immediate-path pass remains successful and must not be
reimplemented.

Current source inspection identifies additional candidates outside that closed
scheduler scope:

1. `receive_client_datagram()` clones the complete
   `DatagramProxySpec` for every client datagram even though steady-state
   ingress primarily needs the max-datagram bound and one upstream policy
   snapshot.
2. Every client datagram calls `resolve_association()`, which enters the
   async mutex protecting `HashMap<SocketAddr, AssociationSlot>` even for an
   already-active association.
3. M024 already guarantees UDP bind/connect occurs outside the association
   registry lock. Current registry critical sections inspected for this plan
   perform bounded in-memory lookup/update work before any later await, making
   a read-optimized/synchronous registry strategy plausible if M042 proves
   contention/lock overhead.
4. Association accounting is improved relative to pre-M024 code, but a normal
   exchange still performs multiple `AssociationRecord` lock acquisitions
   across listener ingress, worker activity, egress, and evidence publication.
5. Datagram duplicate stages under-reserve the next candidate vector relative
   to their known bounded amplification.
6. Payload corruption always copies `Bytes` into a `Vec<u8>`. The pinned
   `bytes` 1.12.1 API supports `Bytes::try_into_mut()`, which can avoid
   copying when the candidate uniquely owns the entire backing buffer and can
   safely fall back when shared.
7. The listener idle reaper wakes every 10 ms and scans all associations. This
   may become material at high idle cardinality; M042 must prove it before any
   deadline-queue redesign.

## Scope

Primary:

- `crates/eggchaos-server/src/runtime/datagram/supervisor.rs`
- `crates/eggchaos-server/src/runtime/datagram/association.rs`
- `crates/eggchaos-server/src/runtime/datagram/registry.rs`
- `crates/eggchaos-server/src/runtime/datagram/model.rs`
- `crates/eggchaos-core/src/datagram.rs`

Qualification:

- `benchmarks/src/bin/datagram.rs`
- `qualification/performance/`
- ADR 003 deterministic/golden/runtime tests

## Non-goals

- no change to datagram fault kinds or composition order;
- no change to per-client connected upstream socket ownership;
- no change to association identity/capacity/setup waiter semantics;
- no change to M024 heap ordering `(release_at, ingress_ordinal, copy_index)`;
- no queue-bound increase or silent overflow behavior;
- no general-purpose concurrent-map dependency by default;
- no zero-copy receive-buffer lifetime scheme unless M042 proves ownership
  copying dominates and a separate safe bounded design is justified;
- no `unsafe`;
- no weakening of M023/M024 budgets.

## Work packages

### WP1 — Consume the M042 datagram target matrix

Before production edits, record:

- exact M042 pre-optimization candidate/artifacts;
- active-association cardinalities successfully measured;
- which spec-clone/registry/record/reaper/candidate/corruption targets are
  proven-material, low-cost-cleanup, inconclusive, or not-material;
- M042's frozen implementation thresholds.

Complex work on registry/reaper design is forbidden when the corresponding
target is inconclusive/not-material.

### WP2 — Remove whole-spec cloning from steady-state ingress

This is the preferred first low-risk production change when M042 permits it.

In `receive_client_datagram()`, obtain only the data required for the
current datagram while holding the short standard RwLock read guard:

- max datagram size / queue limit fields needed before association dispatch;
- one atomic upstream `PublishedDatagramPolicy` snapshot.

Drop the spec guard before any await.

Do not clone the proxy name, listener/upstream addresses, downstream policy, or
other lifecycle settings on every active ingress datagram.

New-association setup may continue to clone a complete immutable setup spec
because the worker needs a stable configuration snapshot and setup is not the
steady-state packet path.

Policy generation/namespace must still be one atomic snapshot attached to each
ingress item.

### WP3 — Optimize active-association lookup synchronization if measured

If M042 proves the async association-map mutex is material, replace or split
the registry synchronization without changing its authority.

The preferred candidate is a standard-library read/write lock or another
dependency-free short-critical-section design because:

- M024 already moved UDP bind/connect outside the registry guard;
- inspected registry critical sections perform in-memory
  lookup/insert/remove/drain only;
- waiting for setup completion occurs after the guard is dropped.

Before conversion, audit every `state.associations` use again on the exact
implementation base. No synchronous guard may survive an await or external
socket operation.

Required race semantics:

- exactly one setup owner for a new client;
- retained event-driven waiters observe Published/Abandoned transitions;
- global/per-proxy capacity leases release exactly once;
- delete/disable/drain can remove Starting/Active entries without task/socket
  leaks;
- idle reaping removes only the exact association instance it inspected;
- update_proxy's association-count/spec transition remains atomic enough to
  preserve its current validation guarantees.

Do not add DashMap/lock-free map/object pool unless the dependency-free
candidate is measured insufficient and closure evidence justifies the extra
complexity/dependency.

### WP4 — Reduce redundant association-record synchronization

Implement only to the extent M042 shows record-lock overhead is material.

Prefer batching within existing ownership boundaries:

- accumulate worker-loop activity/egress/error/evidence deltas and commit them
  in one record lock where semantics permit;
- avoid a second last-activity lock in the same worker iteration when the
  first record commit can carry it;
- retain exact listener ingress accounting and coherent
  `DatagramAssociationSnapshot` creation.

Do not make public snapshot counters eventual merely to remove a mutex. If a
conversion to atomics is considered, first document which multi-field
coherence properties are currently relied on and prove they are retained or
explicitly non-contractual.

### WP5 — Bound candidate allocation more accurately

For duplicate stages, reserve the next vector based on the validated bounded
amplification:

`candidates.len() * (additional_copies + 1)`

using checked/saturating arithmetic consistent with the existing 4,096
candidate hard bound.

For non-duplicate stages, preserve or reuse capacity where doing so avoids
allocation without retaining unbounded memory.

A two-buffer scratch/ping-pong design is optional only if M042/M044 local
probes show repeated allocation remains material after exact capacity
reservation. Any retained scratch capacity must remain bounded by existing
plan amplification limits.

Candidate order must remain byte-for-byte identical.

### WP6 — Avoid corruption copies when Bytes ownership is unique

If M042 proves corruption copying material, attempt mutation through
`Bytes::try_into_mut()`:

- success: mutate the unique `BytesMut` using the exact current selected
  indices/bit flips, then freeze back to `Bytes`;
- failure (shared/static/non-unique): fall back to the current copy-and-mutate
  behavior.

Do not change RNG draw count/order, candidate length, copy identity, or
duplicate sharing semantics.

Add explicit tests for unique and shared candidate backing storage and verify
identical corrupted bytes/evidence under the same seed.

### WP7 — Replace fixed-interval full idle scans only if M042 proves material

Do not implement this package on source intuition alone.

If high-cardinality idle measurement shows the 10 ms scan is material, design
a deadline-driven reaper using existing dependencies (for example a Tokio
deadline heap/DelayQueue) without changing the idle contract.

Required properties:

- never expire before `last_activity + association_idle_timeout`;
- queued upstream/downstream candidates or pending ingress still defer
  expiration as today;
- activity efficiently advances/reschedules the deadline;
- delete/disable/update/drain invalidates stale deadline entries safely;
- no detached per-association sleeper task;
- memory remains bounded by association limits;
- association identity prevents an old expiry event removing a new
  association for the same client.

If these properties require disproportionate complexity relative to measured
benefit, retain the current scanner and record the no-change disposition.

### WP8 — Before/after datagram qualification

Rerun the complete M042 datagram matrix on the exact M044 candidate:

- M023 direct/empty historical ratios;
- M024 topology-matched sequential p95/throughput and windowed throughput;
- scheduler probes to ensure closed M024 behavior did not regress;
- 1/8/256/1,024/4,096 pre-warmed association scale where supported;
- hot-client-with-many-idle-associations;
- distributed multi-client workload;
- corruption workload if WP6 lands;
- idle-cardinality observation if WP7 lands.

Retain raw before/after artifacts.

## Required invariants

- ADR 003 golden traces remain byte-for-byte unchanged;
- ingress ordinal/copy-index/generation ordering remains unchanged;
- per-datagram policy snapshot remains atomic;
- queue count/byte bounds and oversize/overflow distinctions remain unchanged;
- loss/duplication/reorder/corruption/bandwidth RNG behavior remains exact;
- one connected upstream socket per client association remains the response
  isolation boundary;
- Starting/Active setup semantics and capacity leases remain exact;
- no registry/spec/record guard is held across socket/network awaits;
- public Rust/native/config/CLI/SDK surfaces remain unchanged;
- no new production dependency without measured justification.

## Verification

Minimum:

```sh
./scripts/check.sh
./scripts/benchmark_datagram.sh
cargo test -p eggchaos-core --all-features
cargo test -p eggchaos-server --all-features
EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh
```

Run all M042 datagram scale case selectors/probes on the exact M044 candidate.

Focused concurrency tests must include:

- concurrent first datagrams -> one association;
- setup owner cancellation/failure and waiter takeover;
- delete/disable/update during setup;
- simultaneous capacity pressure;
- idle reap racing new association for same client;
- kill/drain with queued datagrams;
- multi-client unsolicited/multi-response isolation;
- exact unique/shared corruption output if WP6 lands.

## Acceptance criteria

M044 can close only when:

- each nontrivial production change is justified by M042 measurement;
- whole-spec cloning is removed from steady-state active ingress if classified
  for implementation;
- any registry synchronization change passes the complete setup/drain race
  suite with no guard across await;
- any reaper redesign is supported by high-cardinality evidence and preserves
  all deferral/identity semantics;
- M023 and M024 existing budgets pass unchanged;
- M042's frozen high-cardinality regression/improvement thresholds pass;
- ADR 003 golden traces remain unchanged;
- no public/wire/capability regression exists;
- raw exact-candidate before/after evidence is retained.

## Rejection / stop conditions

Do not close M044 if:

- the M024 heap scheduler is changed without a newly measured scheduler
  regression;
- a synchronous registry lock is held across await/network I/O;
- setup waiter/capacity behavior becomes polling-based again;
- corruption optimization changes deterministic output or duplicates;
- idle expiry can remove a replacement association from a stale event;
- queue/evidence/accounting is weakened for throughput;
- high-cardinality optimization misses its M042 threshold and remains without
  maintainability justification;
- an existing M023/M024 budget is weakened.

## Closure evidence

Create:

`plans/closure/M044-datagram-steady-state-synchronization-and-association-scale-optimization-closure.md`

Record exact candidate SHA, target disposition, before/after scale matrix,
historical-budget results, race/golden evidence, host limits, and any omitted
conditional work packages.

## Successor activation

M044 closure alone does not activate M045. M045 becomes ready only after both
M043 and M044 are closed or one closes as an evidenced no-op under M042.
