# M038 — Pinned Post-v2.12 Toxiproxy packet_loss Profile

Status: blocked  
Depends on: M037  
Role: compatibility adapter + pinned upstream oracle

## Objective

Add an explicit opt-in Toxiproxy compatibility profile for the researched
post-v2.12 upstream snapshot, translating upstream `packet_loss` into the
M036/M037 native `StreamLoss` primitive while preserving the existing strict
v2.12 profile unchanged.

Create a reproducible source-built oracle and differential corpus for the
extension instead of tracking moving Toxiproxy `main`.

## Upstream baseline

Research baseline:

- released strict oracle: Shopify Toxiproxy v2.12.0;
- upstream feature commit:
  `7c01129a8c232bf01aaebaca8a87429fd16f69b2`;
- pinned researched `main` snapshot:
  `40f7fd31bee529d824116bd2a11a9e3425e904ec`;
- `v2.12.0...40f7fd31`: 86 commits;
- only new server data-plane toxic in that diff: `packet_loss`.

At the pinned snapshot, upstream `packet_loss` has:

```text
type = "packet_loss"

attributes:
  loss_rate:   float64
  correlation: float64
```

Upstream execution uses per-connection mutable state with a previous-drop bit
and per-connection RNG. It clamps both attributes into [0,1] for execution.
The oracle research in this milestone must freeze exact create/update/echo
behavior, including out-of-range attributes, omitted fields, names, direction,
toxicity, and live update state.

Do not rely solely on source inspection for API parity; execute the pinned
oracle.

## Scope

### In scope

- Introduce an explicit compatibility-profile type/configuration.
- Keep strict v2.12 as the default.
- Add one opt-in pinned profile for the 2026-09-25 post-v2.12 snapshot.
- Accept/render `packet_loss` only under that profile.
- Add `loss_rate` and `correlation` to compatibility toxic attributes.
- Map:
  - `toxicity` -> native `FaultSpec::probability`;
  - `loss_rate` -> native `StreamLossConfig.loss_rate`;
  - `correlation` -> native `StreamLossConfig.correlation`.
- Reverse-map native `StreamLoss` into `packet_loss` only when the
  post-v2.12 profile is selected.
- Preserve the adapter's no-separate-state rule.
- Add a pinned source-fetch/build/checksum script for the upstream snapshot.
- Record an executable oracle baseline under `qualification/`.
- Extend differential cases for API/default/live/data-plane behavior.
- Add Go-client/generic-client smoke as appropriate.
- Document exact, behaviorally compatible, intent-compatible, and unsupported
  aspects separately from the v2.12 parity table.

### Non-goals

- No claim of a fictional Toxiproxy 2.13 release.
- No moving-`main` compatibility claim.
- No change to strict v2.12 `GET /version`, toxic set, or oracle corpus.
- No IP/TCP packet-loss claim.
- No UDP mapping.
- No native semantic change to StreamLoss.
- No state migration across Eggchaos policy generations.
- No incorporation of unrelated unreleased upstream CLI/features/PRs.
- No upstream slicer behavior change unless it is already present in the
  pinned snapshot and demonstrably changes the oracle cases needed here.
- No package/release version decision beyond compatibility profile metadata.

## Compatibility profile

Use a stable explicit enum/value. Exact public spelling may be refined before
implementation, but it must distinguish:

```text
strict-v2.12
post-v2.12-2026-09-25
```

or equivalent names with the same semantics.

Default remains strict v2.12.

The profile must be selectable in the compatibility server/example/test
harness without changing the native admin API.

If the adapter exposes a builder:

```text
CompatProfile::V2_12
CompatProfile::PostV2_12_2026_09_25
```

is preferred to a free-form string internally.

Unknown profiles fail clearly.

## /version behavior

M038 must execute the pinned oracle built from the source snapshot and record
its real `GET /version` response under the exact build command.

Strict v2.12 keeps its existing exact response.

The post-v2.12 profile should match the pinned source oracle's version response
when practical. Do not invent `2.13.0`. If source-build version metadata is
build-system dependent, expose/record that limitation and keep profile identity
as Eggchaos configuration metadata rather than claiming a released upstream
version.

## Toxic translation

### Create/defaults

Research and freeze:

- omitted `loss_rate`;
- omitted `correlation`;
- omitted/empty `name`;
- omitted `stream`;
- omitted `toxicity`;
- mixed integer/float JSON forms;
- unknown attributes;
- out-of-range finite values;
- wrong JSON types.

Expected zero-value behavior from source is
`loss_rate=0.0, correlation=0.0`, but the live oracle is authoritative.

### Update

Research exact behavior for:

- toxicity-only update;
- loss-rate-only update;
- correlation-only update;
- both attributes;
- supplied `type`/`stream` changes (existing Toxiproxy update semantics
  normally ignore them);
- cross-type attribute keys;
- out-of-range values.

Translate updates through the same native fault mutation authority used by
strict toxics. No adapter-local mutable PacketLoss state is allowed.

### Reverse presentation

Under the snapshot profile, a native `StreamLoss` must render:

```json
{
  "name": "...",
  "type": "packet_loss",
  "stream": "upstream|downstream",
  "toxicity": ...,
  "attributes": {
    "loss_rate": ...,
    "correlation": ...
  }
}
```

Under strict v2.12, the same native fault is not representable as a v2.12 toxic.
The strict adapter must fail/filter according to one explicit documented rule
rather than falsely calling it another toxic. Prefer failing a mutation that
would introduce an unrepresentable native fault through strict compatibility;
native-created unrepresentable faults need an explicit view policy tested by
M038.

Do not silently map native stream loss to datagram loss or timeout.

## Data-plane comparator

### Exact edge cases

Require exact/structural behavior for:

- `loss_rate=0, correlation=0`: bytes preserved;
- `loss_rate=1`: affected traffic discarded/blocked from destination;
- toxicity 0: toxic inactive;
- toxicity 1: toxic active;
- upstream and downstream directions;
- add/remove before a connection;
- add/remove/update on an existing connection, with divergence notes where
  state continuity differs.

### Intermediate stochastic cases

Do not compare exact byte positions.

For representative rates/correlation, execute enough independent
connections/chunks to establish that:

- both implementations exhibit nonzero/non-total loss for mid-range rates;
- increased `loss_rate` increases observed loss over a bounded sample;
- positive correlation produces measurably more adjacent/burst drop behavior
  than correlation zero under a statistical comparator chosen before running
  the final corpus.

The comparator must have explicit sample sizes and tolerances and must not be
tuned after seeing one candidate's output.

Because Eggchaos uses deterministic fixed logical grains while upstream uses
runtime-dependent StreamChunks/RNG, classify intermediate data-plane behavior
as `intent compatible`.

## Live update divergence

Upstream `packet_loss` is a `StatefulToxic`; its state object can survive a
toxic goroutine restart/update on an existing link.

Eggchaos retains its existing generation barrier and recompiles engine state on
policy publication.

M038 must include a live update case that documents this difference rather
than pretending state continuity is exact. It must still prove:

- already accepted preserving bytes are not corrupted by update;
- the new loss configuration becomes active after the generation barrier;
- remove unblocks future preserving traffic according to native semantics.

## Pinned oracle infrastructure

Add a script analogous in discipline to the v2.12 fetcher, for example:

`scripts/fetch_toxiproxy_post_v2_12.sh`

It must:

- fetch source for exact commit
  `40f7fd31bee529d824116bd2a11a9e3425e904ec`;
- verify archive/content identity with a committed checksum or equivalent;
- build with an explicitly recorded Go toolchain;
- return the exact executable path;
- cache only by commit/toolchain identity;
- never fall back to current `main`.

Add qualification entry point, for example:

`scripts/qualify_toxiproxy_post_v2_12.sh`

Release/mandatory mode must fail if the verified oracle is unavailable.
Developer mode may report incomplete only if that behavior is explicit and
cannot be confused with a pass.

Do not replace the existing v2.12 scripts.

## Ordered work packages

### WP1 — Pin and execute the upstream snapshot

Build the exact oracle, capture `/version`, route behavior, toxic defaults,
attribute edge cases, and data-plane samples. Commit a normalized baseline.

### WP2 — Add compatibility profile plumbing

Add the typed profile and strict default. Prove v2.12 behavior is unchanged
when no profile is supplied.

### WP3 — packet_loss DTO translation

Extend compatibility attributes/create/update/reverse mapping through native
StreamLoss.

### WP4 — Strict-profile isolation

Add negative tests proving strict v2.12 still rejects `packet_loss` and does
not mis-render native StreamLoss.

### WP5 — Differential corpus

Add exact edge/API comparisons and predeclared statistical/intent comparators
for intermediate loss/correlation.

### WP6 — Client smoke

Exercise the snapshot profile with the official Go client using generic toxic
creation or another independent HTTP client if the official typed helpers have
not added packet_loss APIs. Record exact client/toolchain versions.

### WP7 — Documentation/reference matrix

Create/update a post-v2.12 compatibility reference that is separate from
`plans/reference/toxiproxy-parity.md`. Update `docs/toxiproxy.md` with
profile selection and semantic caveats.

### WP8 — Exact-candidate closure

Run M038 gates on one exact candidate and create closure evidence.

## Required tests

At minimum:

- strict profile rejects `packet_loss`;
- snapshot profile accepts it;
- omitted attributes/default name/default stream/default toxicity;
- create/get/list/update/delete;
- duplicate name;
- invalid stream;
- wrong toxic type;
- loss_rate/correlation 0 and 1;
- out-of-range behavior compared to oracle and documented;
- upstream/downstream;
- toxicity 0/1;
- add/remove/update before connection;
- add/remove/update during existing connection;
- loss_rate=0 exact bytes;
- loss_rate=1 exact no-forward behavior within bounded timeout;
- intermediate loss-rate statistical comparator;
- correlation statistical/burst comparator;
- strict v2.12 existing differential corpus remains fully green;
- native fault created through /v1 is visible through snapshot adapter by the
  documented reverse mapping;
- strict adapter behavior for native StreamLoss is explicit and tested;
- no adapter-local proxy/toxic state is introduced.

## Verification

Minimum:

```sh
./scripts/check.sh

TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" \
  EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 \
  ./scripts/qualify_toxiproxy_v2_12.sh

TOXIPROXY_POST_V2_12_SERVER="$(./scripts/fetch_toxiproxy_post_v2_12.sh)" \
  EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1 \
  ./scripts/qualify_toxiproxy_post_v2_12.sh

cargo test -p eggchaos-toxiproxy --all-features
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
```

Exact environment variable/script names may differ, but mandatory mode and
pinned identity are required.

## Acceptance criteria

M038 closes only when:

- strict v2.12 remains the default and passes its existing pinned oracle gate;
- the snapshot profile is explicit and tied to
  `40f7fd31bee529d824116bd2a11a9e3425e904ec`;
- `packet_loss` translates only to native StreamLoss;
- API/default/update behavior has live-oracle evidence;
- exact edge cases and intent-compatible stochastic cases have committed
  comparators;
- live-update state divergence is documented/tested;
- no moving-main fetch exists;
- no fictional released version is claimed;
- docs/reference matrices distinguish strict v2.12 from snapshot support;
- all M038 tests pass on one exact candidate;
- closure evidence records oracle build identity, toolchain, corpus, and
  limitations.

Create
`plans/closure/M038-pinned-post-v2-12-toxiproxy-packet-loss-profile-closure.md`.

## Stop/rejection conditions

Do not close if:

- the implementation changes strict v2.12 toxic acceptance;
- oracle source is fetched from an unpinned branch;
- intermediate random sequences are asserted byte-for-byte against upstream;
- the adapter owns an independent RNG/data plane/state store;
- packet_loss is described as true TCP/IP packet loss;
- a new Toxiproxy version number is invented;
- statistical tolerances are chosen after seeing the candidate result.

## Follow-on activation

A clean M038 closure makes M039 ready.

M039 is the tranche-level exact-candidate authority and must rerun both the
strict v2.12 and pinned snapshot oracles together with native/SDK/binding
regressions.
