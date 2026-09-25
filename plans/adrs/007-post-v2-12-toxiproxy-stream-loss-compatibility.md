# ADR 007 — Post-v2.12 Toxiproxy Stream-Loss Compatibility

Status: accepted  
Date: 2026-09-25  
Owners: eggchaos maintainers  
Activates: M036 -> M037 -> M038 -> M039

## Context

Eggchaos deliberately froze its compatibility claim against Shopify
Toxiproxy v2.12.0. M012 and the later exact-candidate gates qualify that
seven-toxic surface against a pinned v2.12.0 oracle. The adapter is not a
moving-`main` compatibility promise.

On 2026-09-25, Shopify Toxiproxy `main` is at
`40f7fd31bee529d824116bd2a11a9e3425e904ec`, 86 commits ahead of the
v2.12.0 tag. The only new server data-plane toxic in that diff is
`packet_loss`, introduced by upstream commit
`7c01129a8c232bf01aaebaca8a87429fd16f69b2`.

Upstream `packet_loss` operates on Toxiproxy's userspace `StreamChunk`
objects, not IP/TCP packets. It has two attributes:

- `loss_rate`: baseline probability in [0,1] that a stream chunk is dropped;
- `correlation`: additional probability after the previous chunk was dropped,
  capped at 1.

Toxiproxy keeps per-connection mutable state containing the previous-drop bit
and an RNG. This is a byte-stream corruption model. It does not model TCP
retransmission, congestion response, checksums, or IP packet boundaries.

Eggchaos already has real whole-datagram loss under ADR 003. Reusing
`DatagramFaultKind::Loss` for TCP compatibility would conflate two different
transport abstractions and violate the existing stream/datagram boundary.

The current stream engine also has stronger determinism requirements than
Toxiproxy. Arbitrary Tokio `poll_write` boundaries, kernel read sizes, or
scheduler ordering must not change a replay trace.

## Decision

### 1. Preserve strict v2.12 as a frozen profile

The existing Toxiproxy v2.12 behavior remains a distinct strict compatibility
profile and remains the default.

Strict v2.12:

- exposes exactly the v2.12 route/toxic contract already qualified by M012;
- rejects `packet_loss` as an invalid toxic, matching the pinned v2.12 oracle;
- keeps `GET /version` behavior unchanged;
- continues to use the existing pinned v2.12 qualification script/oracle.

Post-v2.12 support is opt-in and must never silently broaden the strict
v2.12 claim.

### 2. Add a native deterministic stream-loss primitive

The semantic primitive belongs in `eggchaos-core`, not in
`eggchaos-toxiproxy`.

Its native name is `stream-loss` / `StreamLoss`. Native documentation must
not call it packet loss except when describing the Toxiproxy compatibility
spelling.

The initial config contains only:

- `loss_rate`: finite probability [0,1];
- `correlation`: finite probability [0,1].

The native logical loss grain is fixed at 32 KiB for this tranche. The constant
is public/documented and versioned as part of stream-loss semantics; it is not
a user-configurable field in M036-M039. A later configurable grain requires
separate planning because it would affect replay identity and reverse
Toxiproxy presentation.

### 3. Loss decisions are keyed to absolute stream byte position

A logical loss chunk is determined by the accepted byte-stream offset, not by a
caller write or Tokio poll boundary.

For grain size `G = 32768`, byte offset `n` belongs to logical chunk
`floor(n / G)`. A chunk's loss decision is selected once when that logical
chunk is first encountered and applies to every accepted byte in that chunk,
including bytes arriving in later writes.

For one active stream-loss fault:

```text
p(drop[0]) = loss_rate

p(drop[n]) =
    loss_rate                              if drop[n-1] == false
    min(1, loss_rate + correlation)        if drop[n-1] == true
```

Each fault has its own deterministic RNG/state derived from the existing
connection/fault seed domain. No process-global RNG, wall clock, task ordering,
socket read size, or caller write fragmentation participates.

Equivalent byte streams under the same connection identity, policy seed,
direction, and fault plan therefore produce the same loss decisions even when
write fragmentation differs.

### 4. Connection-level toxicity remains separate

`FaultSpec::probability` retains its current meaning: deterministic
per-connection activation probability.

For the Toxiproxy adapter:

- Toxiproxy `toxicity` maps to `FaultSpec::probability`;
- `attributes.loss_rate` maps to native `StreamLoss.loss_rate`;
- `attributes.correlation` maps to native `StreamLoss.correlation`.

These are separate probability layers and must not be collapsed.

### 5. Stream loss is destructive but bounded

Dropped bytes are accepted from the caller and counted as intentionally
discarded. They require no retained payload buffer.

For mixed dropped/preserved input, `poll_write` may accept only the largest
prefix for which every byte is either:

- deliberately discarded by stream loss/another destructive fault; or
- owned by the existing bounded preserving queue.

The implementation must never skip an unbufferable preserving prefix in order
to accept later dropped bytes.

`poll_flush` remains the delivery barrier for preserving accepted bytes.
Dropped bytes are already resolved and do not delay flush.

### 6. Existing fault composition invariants win over incidental upstream chunking

The existing stream engine remains one heterogeneous deterministic state
machine. M036 must define and test composition explicitly.

Required rules:

- blackhole retains its current dominant discard behavior; stream-loss RNG
  does not need to advance while blackhole owns the accepted prefix;
- limit-data continues to bound the caller-visible accepted prefix before
  termination; loss within that prefix still counts toward the limit;
- slicer remains a preserving output segmentation/pacing primitive and does
  not define stream-loss grain boundaries;
- latency/bandwidth apply only to preserving bytes that survive loss;
- multiple active stream-loss faults may be composed, but their state/RNG is
  fault-local and deterministic. The implementation must freeze one
  documented rule for how their decisions combine and golden-test it.

The preferred multi-loss rule is ordered evaluation on the same absolute
logical-grain identity with fault-local state; a byte range discarded by any
active loss stage is discarded once. If implementation discovers that this
cannot preserve the existing engine ownership contract cleanly, stop and amend
this ADR rather than deriving behavior from runtime fragmentation.

### 7. Existing seven-slot evidence arrays remain frozen

The existing `activations: [u64; 7]` arrays and their index order are an
evidence contract. M036 must not resize or reorder them.

Stream loss gets additive named evidence, at minimum:

- logical stream-loss chunks evaluated;
- logical chunks dropped;
- bytes discarded by stream loss;
- stream-loss activation/decision counters sufficient for diagnosis.

Existing aggregate `bytes_discarded` continues to include all intentional
stream discards.

The implementation may refactor internal fault-type lookup so
`FaultKind::type_name()` no longer requires every kind to have a legacy array
index. Existing seven indexes and metric meanings must remain byte-for-byte
stable.

### 8. Live updates keep Eggchaos generation semantics

A live policy change still uses the existing drain/barrier and engine
replacement semantics.

M036-M039 do not add state migration across engine generations solely to mimic
Toxiproxy's `StatefulToxic` implementation. A stream-loss fault's current
logical-chunk/correlation/RNG state may restart when a new policy generation is
compiled.

This is a documented post-v2.12 compatibility divergence and must be tested.
Changing the general live-generation contract requires a separate ADR.

### 9. Post-v2.12 Toxiproxy support uses a pinned snapshot profile

M038 introduces an explicit opt-in compatibility profile tied to the researched
upstream snapshot `40f7fd31bee529d824116bd2a11a9e3425e904ec`.

The profile name must communicate that it is a pinned post-v2.12 snapshot, not
an invented Toxiproxy release number. A spelling such as
`post-v2.12-2026-09-25` is acceptable.

The profile:

- retains the v2.12 route family;
- adds `packet_loss` with `loss_rate` and `correlation`;
- uses the pinned source snapshot as its oracle;
- records exact API/default behavior and intent-compatible data-plane
  comparison;
- does not claim compatibility with future moving `main`.

If Shopify publishes a tagged successor, a later milestone must diff that tag
against this snapshot before renaming/promoting the profile.

### 10. Compatibility level for packet_loss

Exact compatibility is required where deterministic comparison is meaningful:

- toxic type spelling;
- attribute names and zero defaults;
- route/status/error behavior;
- toxic CRUD/update/delete behavior;
- direction/default naming behavior;
- `loss_rate=0` preserves bytes;
- `loss_rate=1` discards all affected bytes;
- toxicity 0/1 behavior.

Intermediate stochastic loss and burst correlation are `intent compatible`,
not byte-sequence exact, because upstream chunk boundaries and RNG are
runtime-dependent and nondeterministic.

Out-of-range upstream attribute handling must be oracle-researched in M038.
Native values remain finite [0,1]. If the upstream oracle stores/echoes values
outside that range while clamping only at execution, the adapter may clamp into
native representable state and must record the echo divergence rather than
weakening native validation.

## Consequences

Positive:

- strict v2.12 compatibility remains truthful and reproducible;
- Eggchaos gains a useful deterministic destructive stream primitive;
- TCP stream loss and UDP datagram loss remain distinct;
- loss traces are independent of write fragmentation;
- the adapter continues to have no separate data-plane/state authority;
- future tagged Toxiproxy releases can be promoted from a pinned evidence base.

Costs:

- a new native fault kind must propagate through protocol/OpenAPI/SDK/embed
  surfaces before the adapter can consume it cleanly;
- stream-loss evidence needs additive fields without resizing legacy arrays;
- the post-v2.12 oracle requires source-build/pinning infrastructure in
  addition to the binary v2.12 oracle;
- live update state continuity differs from upstream and must be documented.

## Rejected alternatives

### Reuse datagram loss for TCP

Rejected. Whole UDP datagrams and userspace TCP byte chunks have different
transport semantics and replay identities.

### Implement packet_loss only inside eggchaos-toxiproxy

Rejected. That would create a second data-plane/state authority and break the
M010/M012 ownership model.

### Use Tokio poll_write calls as packet boundaries

Rejected. Replay would depend on caller fragmentation and runtime scheduling.

### Track Toxiproxy main continuously

Rejected. Compatibility needs a reproducible oracle. Moving `main` is a
research source, not a versioned contract.

### Replace the seven-slot activation arrays with eight slots

Rejected for this tranche. Their order/length is already frozen evidence.
Stream loss uses additive named evidence.

### Preserve packet-loss RNG state across all live generations

Rejected for this tranche. It would alter the existing generation replacement
contract for one unreleased upstream toxic.

## Implementation sequence

`M036`
: deterministic stream-loss primitive and evidence semantics in core.

`M037`
: native wire/config/CLI/scenario/OpenAPI/SDK/embed propagation.

`M038`
: opt-in pinned post-v2.12 Toxiproxy snapshot profile and differential oracle.

`M039`
: exact-candidate qualification, regression proof, documentation, and closure.

M037 is blocked until M036 closes; M038 is blocked until M037 closes; M039 is
blocked until M038 closes.
