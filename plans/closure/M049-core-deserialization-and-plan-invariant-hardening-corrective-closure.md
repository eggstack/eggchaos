# M049 — Core Deserialization and Plan-Invariant Hardening Corrective Closure

Status: closed

Exact M049 implementation/evidence candidate:
`bbe3b434bdf91ffe6367b81dd96adc808b69c070`

Implementation commit: `2305991 M049: route core plan/invariant serde through canonical validators`
Fuzz / corpus commit:   `bbe3b43 M049: extend plan_json fuzz target with invalid-construction coverage`

Activation baseline (per plan): `4febd52e60f8098e9adfffa876052831fb2be56a` (M048 closure).

Closure date: 2026-09-28

Depends on: M048 closed on exact candidate `ab61ac7`.

## Outcome

M049 closes the audit-baseline structural asymmetry between the
`DatagramPlan` and `FaultPlan` invariants: every safe Rust construction
path for `FaultId`, `Probability`, and `FaultPlan` now reaches the same
authoritative validator that the documented constructors already used.
The serialized representation, public symbol names, fault semantics,
deterministic RNG contract, native `/v1` DTOs, OpenAPI shape, CLI
behavior, TOML configuration shape, Toxiproxy mappings, SDK behavior,
Python-native behavior, and the seven-slot legacy activation-array
contract are all unchanged.

The end-to-end invariant ("no invalid plan can reach a published
policy") is now strictly stronger than the pre-M049 revalidation
contract, because invalid scalar values are rejected at the
deserialization boundary instead of only at the publication barrier.

## What changed on `bbe3b43`

`git show --stat 2305991..bbe3b43`:

```text
 architecture/core-fault-engine.md              | 41 ++---
 architecture/verification-qualification.md     |  8 +-
 crates/eggchaos-core/src/datagram.rs           | 16 ++-
 crates/eggchaos-core/src/plan.rs               | 369 +++++++++++++++++++-
 crates/eggchaos-core/tests/stream_loss.rs      | 19 +-
 fuzz/fuzz_targets/plan_json.rs                 | 25 ++-
```

### Scalar deserialization through the constructor

`FaultId` and `Probability` lost the derived `serde::Deserialize`
implementation. Their custom `Serialize`/`Deserialize` impls route
through `FaultId::new` / `Probability::new` so an empty or oversized
fault id, an NaN, an infinity, or an out-of-range probability is
rejected at the deserialization boundary with a `serde::de::Error`.
The serialized representation (a JSON string for `FaultId`, a JSON
number for `Probability`) is unchanged.

### Plan deserialization through the constructor

`FaultPlan` lost the derived `serde::Deserialize` implementation. Its
custom `Serialize`/`Deserialize` impls keep the pre-M049 wire shape
(`{"faults": [...]}`) on the serialize path and route deserialization
through `FaultPlan::new`, which is the canonical authority for the
plan-level invariants (duplicate fault IDs, per-fault scalar validity,
and every kind-specific bound). A document that fails any of those
checks cannot construct a `FaultPlan` at all.

### Complete `FaultPlan::validate`

`FaultPlan::validate` is now the single plan-level invariant authority
matching the existing `DatagramPlan::validate` contract:

- duplicate fault IDs (previously only checked in `FaultPlan::new`);
- defensive scalar re-validation via `Probability::new` for every
  fault stage;
- every kind-specific bound.

`FaultPlan::new`, `FaultPlan::validate`, `with_fault`, and the
publication path all reach the same invariant set.

### Tests

`crates/eggchaos-core/src/plan.rs::tests` adds 11 unit tests covering:

- empty / oversized `FaultId` rejection at serde, boundary lengths
  accepted;
- out-of-range `Probability` rejection at serde, boundary values
  (`0.0`, `1.0`) accepted;
- `Probability::new` rejects NaN, +∞, -∞ on the safe Rust path
  (verifying the constructor is the only authority);
- empty `FaultPlan` serializes as `{"faults":[]}` and round-trips;
- duplicate-ID `FaultPlan` documents rejected at serde;
- oversized / empty fault IDs rejected at serde inside `FaultPlan`;
- out-of-range probability rejected at serde inside `FaultPlan`;
- kind-specific bound (`Slice.variation >= average_size`) rejected at
  serde inside `FaultPlan`;
- round-trip across all eight fault kinds preserves every stage;
- `FaultPlan::validate` agrees with the constructor against duplicate-ID
  and out-of-range-probability mutations that bypass the constructor;
- `with_fault` rejects duplicates;
- `LivePolicy::publish` still rejects a hand-built duplicate-ID plan
  as a defensive belt-and-braces check.

The existing `stream_loss_validation_rejects_out_of_range_probabilities`
test was tightened to assert the new M049 deserialization behavior
(`from_value::<FaultPlan>` returns `Err`) and to confirm that a
hand-built invalid plan that bypasses deserialization is still caught
by `FaultPlan::validate`.

The existing `deserialized_fault_ids_are_revalidated_before_policy_publication`
datagram test was tightened to assert that an empty fault ID is now
rejected at deserialization (the end-to-end invariant is even
stronger), and a valid datagram plan still publishes cleanly.

### Fuzz target

`fuzz/fuzz_targets/plan_json.rs` now exercises scalar
`FaultId` / `Probability` deserialization as explicit targets (the
M049 invariant: invalid scalars are rejected). Local fuzz run
(`cargo fuzz run --sanitizer none plan_json corpus/plan_json
-max_total_time=30`) completed cleanly with `9_239_583` runs and
zero crashes; the pre-existing `corpus/plan_json/` plus four
M049 explicit corpus seeds (`m049_empty_id`, `m049_dup_id`,
`m049_bad_prob`, `m049_oversized_id`) were used.

## Invariant non-regression

- Frozen public symbol set: unchanged (`FaultId`, `Probability`,
  `FaultPlan`, `FaultSpec`, `FaultKind`, all config structs, the
  `ValidationError` variants). No new public item, no removed public
  item, no signature change.
- Frozen serialized shapes: empty plan is `{"faults":[]}`; populated
  plan is `{"faults":[...]}`; `FaultId` is a JSON string;
  `Probability` is a JSON number; `FaultKind` variant tags remain
  PascalCase; `Duration` remains the default `{"secs","nanos"}`
  struct; `NonZeroU64` and the seven-slot legacy activation arrays
  are untouched.
- Frozen RNG / fault semantics: `LivePolicy::new`, `publish`,
  `publish_expected` are unchanged; `STREAM_LOSS_GRAIN_BYTES = 32 *
  1024` is unchanged; SplitMix64-v1 is unchanged; scenario namespaces
  are unchanged.
- Frozen compatibility surfaces: native `/v1` DTOs, OpenAPI shape,
  TOML configuration, CLI behavior, Toxiproxy mappings, SDK behavior,
  Python-native behavior are all unchanged (no test file or schema
  required editing for this milestone).

## Required verification (results on `bbe3b43`)

| Command | Result |
| --- | --- |
| `cargo test -p eggchaos-core --all-features` | green (76 + 1 + 29 = 106 tests) |
| `cargo test -p eggchaos-protocol --all-features` | green |
| `cargo test --workspace --all-features` | green |
| `./scripts/check_openapi.sh` | `{"openapi":"pass","paths":21,"operations":36}` |
| `./scripts/check.sh` (fmt/clippy/test/doc) | exit 0 |
| `cargo fuzz run --sanitizer none plan_json corpus/plan_json -- -max_total_time=30` | 9.24 M runs, 0 crashes |

The TypeScript/Python/Eggfetch/Toxiproxy/python-native gates are not
required by M049 itself (they are M051/M054 cross-language concerns);
the local OpenAPI drift guard is the protocol boundary M049 was
explicitly forbidden from changing.

## Public-API and deterministic-semantics non-regression statement

Every public Rust symbol exported by `eggchaos-core` retains its
pre-M049 name, signature, and behavior. `LivePolicy::new`,
`LivePolicy::publish`, and `LivePolicy::publish_expected` all retain
their infallible/`Result` surface; no caller anywhere in the workspace
needed updating. The deterministic replay identity is unchanged
because (a) `SplitMix64-v1` and all seed derivations are untouched,
(b) the seven-slot activation arrays are untouched, (c) the
`STREAM_LOSS_GRAIN_BYTES = 32 * 1024` semantic metadata is unchanged,
and (d) every valid pre-M049 serialized document round-trips byte
for byte at the semantic level (validated in
`fault_plan_round_trip_preserves_every_fault_kind`).

## Successor activation

Closing M049 activates M050 (currently `blocked` in `plans/registry.md`).
M050 may begin from this exact M049 closure candidate `bbe3b43` as its
activation baseline. M050 will register its own exact implementation
candidate when it closes.
