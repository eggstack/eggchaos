# M036 — Deterministic Stream-Loss Core and Evidence

Status: ready  
Depends on: M035 (closed), ADR 007  
Role: post-v2.12 stream-loss semantic foundation

## Objective

Add the deterministic destructive TCP byte-stream loss primitive defined by
ADR 007 to `eggchaos-core`, including fragmentation-independent logical
chunking, burst correlation, bounded ownership behavior, additive evidence, and
golden/property tests.

M036 is intentionally core-first. It must make the semantic primitive correct
and reproducible before native HTTP/config/CLI/SDK or Toxiproxy presentation is
added.

## Baseline

Current exact planning baseline is the post-M035 repository head. The stream
engine has seven `FaultKind` variants and fixed
`activations: [u64; 7]` evidence arrays whose ordering is documented as a
contract.

The engine already provides:

- deterministic fault-local RNG derived from explicit connection identity;
- per-connection activation through `FaultSpec::probability`;
- bounded preserving queues;
- explicit destructive `Blackhole` behavior;
- exact caller-prefix ownership semantics;
- generation drain/barrier replacement;
- additive accepted/forwarded/discarded byte evidence;
- no-fault direct path.

ADR 007 freezes the new stream-loss semantics. The upstream motivation is
Shopify Toxiproxy `packet_loss`, introduced at
`7c01129a8c232bf01aaebaca8a87429fd16f69b2`; the researched `main`
snapshot is `40f7fd31bee529d824116bd2a11a9e3425e904ec`.

## Scope

### In scope

- Add a native `StreamLossConfig` with finite [0,1] `loss_rate` and
  `correlation`.
- Add `FaultKind::StreamLoss` with stable native spelling `stream-loss`.
- Freeze `STREAM_LOSS_GRAIN_BYTES = 32 * 1024` as the v1 logical grain.
- Add per-fault connection state sufficient to track absolute logical chunk
  index, current chunk decision, previous-drop state, and deterministic RNG.
- Make loss decisions independent of caller/Tokio write fragmentation.
- Support mixed discard/preserve prefixes without violating `AsyncWrite`
  prefix ownership.
- Preserve bounded buffering: only preserving survivor bytes consume the
  existing queue bound.
- Add deterministic composition with existing latency, bandwidth, blackhole,
  limit-data, slicer, slow-close, and disconnect behavior.
- Define deterministic behavior for multiple active stream-loss faults.
- Add additive named stream-loss evidence while preserving every existing
  seven-slot activation array unchanged.
- Extend active-fault type presentation/metrics without renumbering legacy
  fault indexes.
- Add exact golden vectors and fragmentation/property tests.
- Update core architecture documentation for the implemented semantics.

### Non-goals

- No native HTTP route/schema/config/CLI exposure; that is M037.
- No OpenAPI/SDK/binding changes; that is M037.
- No Toxiproxy `packet_loss` spelling; that is M038.
- No UDP/datagram change.
- No IP-layer packet loss, qdisc/netem integration, TCP retransmission model,
  or packet capture.
- No configurable stream-loss grain.
- No new RNG version unless implementation proves the existing domain cannot
  express fault-local chunk decisions without ambiguity.
- No migration of stream-loss RNG/correlation state across live policy
  generations.
- No resizing/reordering of `activations: [u64; 7]`.
- No rewrite of the stream engine into nested generic wrappers.

## Affected surfaces

Expected implementation surfaces:

- `crates/eggchaos-core/src/plan.rs`;
- `crates/eggchaos-core/src/engine.rs`;
- `crates/eggchaos-core/src/stream.rs`;
- `crates/eggchaos-core/src/rng.rs` only if a named domain-separated helper
  makes chunk-decision derivation clearer without changing existing vectors;
- `crates/eggchaos-core/src/lib.rs`;
- core unit/property/golden tests;
- `architecture/core-fault-engine.md`;
- `docs/architecture.md` only for core semantics.

Do not touch `eggchaos-protocol`, HTTP routing, SDKs, or
`eggchaos-toxiproxy` in this milestone except compile-only fixtures if a
public exhaustive match requires a temporary explicit unsupported arm.

## Required semantic model

### Logical chunk identity

The loss grain is fixed:

```text
STREAM_LOSS_GRAIN_BYTES = 32768
logical_chunk_index = accepted_stream_byte_offset / 32768
```

A caller write may begin/end inside a logical chunk. The chunk decision is made
once and reused until the accepted stream offset crosses the next grain
boundary.

Two runs with identical:

- seed namespace;
- proxy identity;
- connection key;
- direction;
- fault ID/order/config;
- accepted byte stream;

must produce identical stream-loss decisions even when the same bytes are
presented using different write fragmentation.

### Burst correlation

For each active `StreamLoss` fault independently:

```text
p = loss_rate
if previous_logical_chunk_was_dropped:
    p = min(1, loss_rate + correlation)
drop = rng.bernoulli(p)
```

The first chunk starts with `previous_dropped = false`.

The connection-level `FaultSpec::probability` draw happens first using the
existing compile semantics. If the fault is inactive, no per-chunk loss state
affects traffic.

### Multiple stream-loss faults

Freeze one deterministic composition rule in code and docs.

Preferred rule:

1. every active stream-loss fault evaluates each original logical ingress
   chunk in plan order using its own state/RNG;
2. a logical byte range is discarded if any active stream-loss fault selects
   drop;
3. each fault advances its own previous-drop state from its own decision,
   independent of another loss fault;
4. payload bytes are discarded once and evidence reconciles without double
   counting aggregate discarded bytes.

This preserves deterministic plan-order diagnostics while avoiding
scheduler/write-boundary dependence.

If this rule conflicts with existing engine invariants in a way that requires a
second buffering pipeline, stop and amend ADR 007 before implementing another
model.

### Prefix ownership and queue bounds

`DirectionEngine::accept` must return only a caller prefix that is fully
resolved:

- dropped subranges are resolved immediately and count as accepted/discarded;
- preserving subranges are accepted only when the bounded queue can own them;
- if the next preserving byte cannot be owned, acceptance stops at that byte;
- later dropped ranges may not be skipped to increase the reported prefix.

No payload copy should be retained for a dropped range.

### Existing-fault composition

At minimum freeze/test:

- **blackhole**: existing blackhole behavior remains dominant while active;
  stream-loss decisions need not advance for bytes blackholed before reaching
  ordinary preservation logic;
- **limit-data**: the limit counts the caller-visible accepted prefix, including
  bytes later discarded by stream loss; exact N-byte termination remains;
- **latency/bandwidth**: survivor bytes only are delayed/throttled;
- **slicer**: survivor bytes may be sliced for output, but slicer boundaries do
  not change stream-loss logical chunk identity;
- **disconnect/slow-close**: termination/shutdown semantics remain unchanged.

## Evidence contract

The existing `EngineEvidence.activations: [u64; 7]`,
`DirectionSummary.activations: [u64; 7]`, and corresponding snapshot/metrics
index order remain unchanged.

Add named evidence rather than an eighth legacy array slot. Exact names may be
adjusted for consistency, but the information must include:

- `stream_loss_chunks_evaluated`;
- `stream_loss_chunks_dropped`;
- `stream_loss_bytes_discarded`.

If multiple stream-loss faults are active, aggregate counters must have a
documented non-double-counting rule for bytes and an explicit rule for
per-fault decision counts.

`bytes_discarded` remains the aggregate intentional-discard byte authority.

`ActiveFault` must be able to report `stream-loss` without requiring a
legacy activation-array index. Refactor internal type-name/index coupling
carefully; indexes 0..6 retain their current exact meanings.

## Ordered work packages

### WP1 — Freeze types, constants, and validation

Add `StreamLossConfig`, the new `FaultKind`, fixed grain constant, finite
probability validation, stable native type name, and unit tests. Preserve
existing enum spellings and seven legacy indexes.

### WP2 — Implement fault-local chunk state

Add deterministic per-active-fault stream-loss state. Ensure state is keyed by
absolute accepted stream offset rather than `poll_write` call count.

### WP3 — Integrate destructive prefix processing

Refactor `accept` only as far as needed to classify/drop/preserve logical
subranges while retaining queue bounds, limit behavior, timers, and the direct
empty-plan fast path.

### WP4 — Composition semantics

Implement and test the ADR 007 rules for multiple loss faults and interactions
with every existing fault family.

### WP5 — Evidence and metrics internals

Add named counters, preserve legacy activation arrays, update live evidence
mirroring/snapshots as needed, and ensure aggregate accounting reconciles:

```text
accepted = forwarded + discarded + currently_buffered
```

at every stable observation boundary where the existing engine already makes
that relation meaningful.

### WP6 — Determinism/property corpus

Commit golden vectors for chunk decisions and property tests proving equivalent
traffic fragmentation produces identical loss outcomes/evidence.

### WP7 — Architecture documentation and closure

Update the core deep dive and create M036 closure evidence on one exact
candidate.

## Required tests

At minimum:

- `loss_rate=0`: exact byte preservation;
- `loss_rate=1`: all accepted bytes discarded with no retained payload;
- `correlation=0`: deterministic independent baseline sequence;
- `correlation=1`: deterministic burst continuation after a drop;
- exact fixed-grain boundary cases at 32767/32768/32769 bytes;
- one 96 KiB write versus equivalent fragmented writes yields identical
  drop/survive ranges and evidence;
- byte-at-a-time writes versus large writes yield identical decisions;
- connection-level probability 0 never activates and 1 always activates;
- unrelated fault addition does not perturb another fault's deterministic RNG
  stream beyond already documented plan/seed rules;
- two active stream-loss faults follow the frozen composition rule;
- stream loss + latency;
- stream loss + bandwidth;
- stream loss + slicer;
- stream loss + limit-data at boundaries inside dropped and preserved chunks;
- stream loss + blackhole;
- stream loss + graceful/hard disconnect;
- flush after mixed drop/preserve traffic;
- shutdown/half-close with survivor bytes queued;
- queue saturation stops at the first unowned preserving prefix;
- live generation replacement drains old survivor bytes and compiles a fresh
  stream-loss state;
- legacy seven-entry activation arrays retain exact prior fixtures;
- existing RNG golden vectors remain unchanged;
- no-fault direct-path tests remain unchanged.

Property tests should include randomized fragmentation of a fixed byte stream
and assert identical forwarded/discarded output for a fixed identity/config.

## Verification

Minimum exact-candidate commands:

```sh
cargo fmt --all -- --check
cargo clippy -p eggchaos-core --all-targets --all-features -- -D warnings
cargo test -p eggchaos-core --all-features
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
./scripts/check.sh
```

Run any core fuzz/property target changed by this work. If a new fuzz target is
added, use the repository's existing pinned fuzz-toolchain policy.

M036 does not require a post-v2.12 external oracle; that begins in M038.

## Acceptance criteria

M036 closes only when:

- `StreamLoss` exists as a protocol-neutral core primitive;
- logical loss decisions are independent of caller write fragmentation;
- the 32 KiB grain and burst-correlation contract are golden-tested;
- dropped bytes are never retained in an unbounded buffer;
- mixed preserve/drop writes obey Tokio prefix ownership;
- all existing fault interactions have explicit tests;
- legacy seven-slot activation evidence is byte/shape stable;
- additive stream-loss evidence reconciles with aggregate discard accounting;
- existing stream/datagram RNG golden corpora remain unchanged;
- no native HTTP/CLI/Toxiproxy compatibility claim is made prematurely;
- all required tests pass on one exact candidate;
- a closure note records the candidate and any residual limitations.

Create
`plans/closure/M036-deterministic-stream-loss-core-and-evidence-closure.md`.

## Stop/rejection conditions

Do not close if:

- loss decisions depend on `poll_write` fragmentation, socket read size,
  Tokio scheduling, wall clock, or global RNG;
- the implementation calls native stream loss IP/TCP packet loss;
- `DatagramFaultKind::Loss` is reused for stream traffic;
- legacy activation arrays are resized/reordered;
- dropped payloads are retained merely to satisfy accounting;
- a preserving byte can be skipped so a later dropped byte is reported
  accepted;
- existing limit/flush/shutdown semantics regress;
- a second stream execution engine is introduced.

## Follow-on activation

A clean M036 closure makes M037 ready.

M037 must expose the already-proven core primitive through native/versioned
control surfaces. It must not redefine stream-loss semantics.
