# M020 — Deterministic Datagram Fault Engine

Status: ready
Depends on: M019, ADR 003
Role: post-release datagram semantics foundation

## Objective

Add a protocol-neutral deterministic datagram impairment subsystem to `eggchaos-core` without changing the existing byte-stream fault engine.

M020 defines the native semantics that every later UDP runtime/control surface must consume. It intentionally contains no sockets, listeners, HTTP/admin routes, CLI commands, Toxiproxy changes, or Eggress routing integration.

## Baseline and dependencies

The current `FaultPlan` / `DirectionEngine` / `ChaosStream` path is correct for ordered byte streams but carries stream-specific contracts: per-connection fault activation, `AsyncWrite` ownership, flush/shutdown barriers, FIFO preserving queues, stream termination, and generation-drain transitions.

ADR 003 establishes a sibling datagram model because loss, duplication, reordering, atomic message boundaries, and UDP overflow behavior cannot be expressed truthfully through those contracts.

Reusable existing primitives include:

- `Direction`, `FaultId`, `Probability`, and `RngVersion` where their meanings remain identical;
- SplitMix64-v1 implementation/golden-vector discipline;
- immutable generation-published policy snapshots;
- bounded evidence/no-payload-capture posture;
- Tokio monotonic time and paused-time test discipline.

## Scope

### In scope

- `DatagramPlan`, `DatagramFaultSpec`, `DatagramFaultKind`, validation, and stable fault-type names.
- A direction-local datagram engine that accepts and emits whole `Bytes` values.
- Initial ordered primitives:
  - delay with symmetric jitter;
  - loss;
  - duplication;
  - reorder-by-hold;
  - payload corruption;
  - whole-datagram bandwidth throttling.
- Per-datagram/candidate probability.
- Stable candidate identity `(ingress_ordinal, copy_index)`.
- Domain-separated deterministic RNG.
- Bounded deadline scheduling by datagram count and bytes.
- Explicit drop-newest queue-overflow semantics.
- Datagram-specific live policy/generation snapshots.
- Datagram evidence and property/fuzz tests.

### Non-goals

- No UDP sockets/listeners or client association registry.
- No admin/config/CLI/scenario routes.
- No Toxiproxy UDP extension.
- No stream fault behavior changes.
- No TCP stream-chunk feature described as packet loss.
- No IP fragmentation, MTU/path-MTU, checksum-error, ECN, TTL, ICMP, NIC/qdisc, multicast, or raw-IP model.
- No correlated/Gilbert-Elliott loss or non-uniform latency distributions in this milestone.
- No task-per-datagram scheduler.

## Affected surfaces

- `crates/eggchaos-core/src/` — new datagram plan/engine/policy/evidence modules and public exports.
- `fuzz/` — focused datagram plan/transition targets.
- `architecture/core-fault-engine.md` and `docs/architecture.md`.
- ADR 003 is the semantic authority and must not be weakened silently.

## Required public model

Exact filenames/types may vary for cohesion, but the public contract must preserve these concepts:

```text
DatagramPlan
DatagramFaultSpec
DatagramFaultKind
DatagramQueueLimits
DatagramDirectionEngine
DatagramLivePolicy
DatagramEvidence
```

A `ChaosDatagram` facade may be added if it improves embedding, but the core must remain independent of `UdpSocket`, addresses, DNS, listener lifecycle, HTTP, CLI, SOCKS, QUIC, and Toxiproxy vocabulary.

## Fault semantics

### Delay

For each candidate, compute base delay plus deterministic symmetric jitter clipped at zero. Scheduling may naturally reorder datagrams when later candidates receive earlier deadlines.

### Loss

A selected candidate is intentionally discarded. This increments configured-loss evidence and never enters the scheduler.

### Duplicate

A selected candidate produces a bounded number of additional copies. Original identity uses `copy_index = 0`; duplicates use monotonically increasing indices. Every copy continues through subsequent ordered stages and consumes independent queue count/byte budget.

### Reorder

A selected candidate receives an additional deterministic hold delay. Reordering is the consequence of deadline scheduling, not arbitrary in-place array shuffling.

### Payload corruption

Deterministically mutate a bounded number of payload bytes/bits before emission. The payload length remains unchanged unless a later ADR explicitly adds truncation. This models application payload mutation; the OS will normally compute a valid UDP checksum over the modified bytes.

### Bandwidth

Token-bucket scheduling applies to complete datagrams. A datagram is never split to satisfy a rate. Burst and rate arithmetic must be bounded/overflow-safe and use Tokio monotonic time.

## Determinism contract

Datagram RNG substreams must be domain-separated from stream RNG substreams and derived from stable dimensions equivalent to:

```text
seed namespace
+ proxy identity
+ association key
+ direction
+ fault identity
+ rng version
+ datagram domain separator
-> fault-local deterministic stream
```

Candidates pass one direction engine in deterministic ingress order. Per-fault probability is evaluated per candidate at that stage, not once per association.

No process-global RNG, OS entropy, wall clock, hash-map iteration order, or Tokio task scheduling may affect a replay decision. Commit golden vectors for seed derivation and representative fault traces.

## Queue and overflow contract

A direction-local scheduler is bounded by both:

- `max_queued_datagrams`;
- `max_queued_bytes`.

Delay, reorder, duplication, and bandwidth share this resource budget. Equal deadlines have a stable tie-break equivalent to:

```text
(release_at, ingress_ordinal, copy_index)
```

When a candidate cannot be admitted, drop that newest candidate and increment a distinct `queue_overflow` counter. Do not stop socket reads or call overflow configured network loss; M021 depends on this distinction.

No queue/history is unbounded.

## Live mutation contract

At admission, a datagram snapshots the complete `(plan, generation, seed_namespace)` that determines its decisions. A newer publication applies immediately to subsequently admitted datagrams. Already queued candidates retain their decisions until emitted or explicitly discarded.

Old- and new-generation datagrams may coexist and depart out of generation order. Do not copy the stream engine's drain-before-swap transition machine.

## Evidence requirements

At minimum, direction evidence must distinguish:

- datagrams/candidate bytes admitted;
- emitted datagrams/bytes;
- configured loss;
- queue-overflow drops;
- duplicated copies;
- corrupted candidates;
- reorder activations;
- current/high-water queued datagrams and bytes;
- configured/injected delay and bandwidth delay;
- fault-type activation counts;
- generation/seed namespace/RNG version needed for replay.

No payload capture.

## Ordered work packages

### WP1 — Types and validation

Add datagram plan/config types with bounded numeric fields, stable names, duplicate-ID rejection, probability validation, and explicit queue limits.

### WP2 — Domain-separated RNG and candidate identity

Add datagram seed derivation/golden vectors and stable ordinal/copy identity. Prove stream golden vectors are unchanged.

### WP3 — Bounded deadline scheduler

Implement one deterministic scheduler with count/byte accounting, stable deadline ordering, timer re-arming, explicit overflow, and no task-per-datagram design.

### WP4 — Preserving timing/rate faults

Implement delay/jitter, reorder hold, and whole-datagram bandwidth. Use paused Tokio time for exact ordering/timing tests.

### WP5 — Destructive/fan-out faults

Implement loss, duplication, and payload corruption with exact accounting and ordered-composition tests such as duplicate→loss versus loss→duplicate.

### WP6 — Datagram live policy

Add immutable generation snapshots and admission-time policy capture. Test mutation while old-generation datagrams remain queued.

### WP7 — Evidence, fuzz/property tests, docs

Add bounded evidence, serialization/round-trip tests where public, hostile plan/config fuzzing, queue-accounting properties, architecture docs, and module map updates.

## Verification

Minimum:

```sh
cargo fmt --all -- --check
cargo clippy -p eggchaos-core --all-targets --all-features -- -D warnings
cargo test -p eggchaos-core --all-features
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
cargo audit --deny warnings
cargo deny check advisories licenses bans sources
```

Required focused evidence includes:

- 0% and 100% loss exactness;
- deterministic intermediate loss traces;
- duplicate count/copy identity;
- equal-deadline stable ordering;
- delay jitter producing natural reorder;
- explicit reorder overtaking;
- corruption determinism and unchanged length;
- whole-datagram bandwidth timing;
- queue count/byte limit exhaustion and drop-newest accounting;
- plan-order differences;
- generation mutation with queued old-generation candidates;
- cancellation/drop leaves no unbounded state;
- existing stream RNG/behavior regressions stay green.

## Acceptance criteria

M020 closes only when:

- ADR 003 concepts exist as an independent datagram core surface;
- all six initial fault kinds have deterministic tested semantics;
- probability is per candidate/datagram rather than association;
- count and byte queue bounds are enforced with explicit overflow evidence;
- live policy uses admission-time snapshots;
- no UDP/runtime vocabulary leaks into the core;
- existing stream public behavior/golden vectors remain unchanged;
- full workspace checks are green;
- architecture/docs describe the implemented contract exactly;
- closure evidence names the exact candidate SHA and test results.

Create `plans/closure/M020-deterministic-datagram-fault-engine-closure.md`.

## Stop/rejection conditions

Do not close if:

- any datagram fault is implemented by converting datagrams into a byte stream;
- queue overflow becomes implicit kernel loss/backpressure;
- RNG draws depend on task scheduling;
- duplicates or queued bytes escape configured bounds;
- live mutation silently reevaluates already admitted candidates;
- payload corruption is documented as checksum/IP corruption;
- implementation alters stream semantics to make datagrams fit;
- required tests are inferred rather than run.

## Follow-on activation

On clean closure, M021 becomes `ready`.

If M021 discovers that a generic Eggress fixed-target UDP primitive is required but not available in an aligned published version, leave M021 blocked and register the narrow upstream Eggress extraction work rather than coupling eggchaos to routing-heavy APIs.
