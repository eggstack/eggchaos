# M050 — Scenario Run Lifecycle Authority Consolidation Corrective Closure

Status: closed

Exact M050 implementation/evidence candidate:
`b2891a37a52e27189f5cf294e341710cadd8f100`

Activation baseline (per plan): `bbe3b43` (M049 closure).
M050 is the only commit on top of the M049 closure candidate.

Closure date: 2026-09-28

Depends on: M049 closed on exact candidate `bbe3b43`.

## Outcome

M050 consolidates the duplicated Scenario V1 and V2 run-registry,
cancellation-token, admission, pruning, lookup, and update bookkeeping
into a single private `runtime::scenario_registry::ScenarioRegistry`
authority. The pre-M050 per-family maps
(`scenario_runs`/`scenario_tokens`/`schedule_v2_runs`/`schedule_v2_tokens`)
are removed. The single `next_run_id` atomic and the single
`scenario_tasks` `JoinSet` remain the only run-ID and task authorities;
V1 and V2 share both, preserving the pre-M050 globally-unique
monotonic run ID allocator and the single supervisor.

Scenario V1 and V2 remain semantically distinct. Their compilers,
fingerprinting, namespace derivation, strict/live isolation, cleanup,
event accounting, and driver semantics are unchanged. The record
shapes returned by every existing public method (and every wire route
that projects them) are unchanged. No native route, DTO, OpenAPI
operation, CLI command, or Toxiproxy mapping changes.

The consolidated registry preserves the pre-M050 effective capability:
32 V1 retained/active runs AND 32 V2 retained/active runs concurrently,
with family-local oldest-finished pruning. V1 and V2 are not collapsed
into a single global quota.

## What changed on `b2891a3`

`git show --stat b2891a3`:

```text
 crates/eggchaos-server/src/runtime/control.rs     | 132 +++++++----------
 crates/eggchaos-server/src/runtime/mod.rs         |  19 +--
 crates/eggchaos-server/src/runtime/scenario_registry.rs | 327 +++++++++++++++++++++++++++++
 crates/eggchaos-server/src/runtime/tests.rs       | 257 +++++++++++++++++++++
```

### New module: `runtime/scenario_registry.rs`

`ScenarioRegistry` is a private `pub(crate)` struct that owns the V1
and V2 run records (`ScenarioRunRecord`, `ScenarioScheduleRunRecord`),
their cancellation tokens, the family-specific active counts, and the
admission/pruning helpers. The two record types stay in their original
crates; the registry holds them in two maps and exposes typed
admit/get/update/token methods per family so the public `ControlState`
API can stay verbatim.

The registry exposes:

- `admit_v1`, `admit_v2`: family-tagged admission. Each family enforces
  its own `MAX_SCENARIO_RUNS = 32` cap independently. When the family
  reaches the retention cap, the registry evicts the oldest finished
  entry (BTreeMap iteration order = ascending run ID) to make room.
- `active_v1_count`, `active_v2_count`: per-family active counts
  (`Pending | Running | Cancelling`).
- `get_v1`, `get_v2`: cloned record lookup.
- `update_v1`, `update_v2`: typed mutation helpers. Unknown IDs are
  silent no-ops, matching the pre-M050 `update_scenario_run` /
  `update_schedule_v2_run` contract.
- `insert_token_v1`, `insert_token_v2`, `take_token_v1`, `take_token_v2`,
  `drop_token_v1`, `drop_token_v2`: typed token management. `take_*`
  returns the token (used by cancel paths) and removes the map entry;
  `drop_*` removes silently.
- `cancel_all`: cascades the cancellation request to every active token
  in both families. Used at `shutdown_and_join` so the cascade
  reaches scenario driver tasks without waiting for the parent token
  alone.

### Public `ControlState` methods

The public `start_scenario` / `get_scenario` / `cancel_scenario` /
`update_scenario_run` / `remove_scenario_token` and the v2-equivalents
(`start_schedule_v2` / `get_schedule_v2` / `cancel_schedule_v2` /
`update_schedule_v2_run` / `append_schedule_v2_event` /
`remove_schedule_v2_token`) retain their pre-M050 signatures and
behavior. Each is now a thin wrapper that holds the registry lock and
delegates to the typed registry method. No signature change, no
observable behavior change.

The `MAX_SCENARIO_RUNS = 32` constant is preserved; admission still
returns the same typed `EggchaosError::Control(ControlError::Conflict("too many active scenario runs"))`
when the active cap rejects a new run.

`shutdown_and_join` now also calls `ScenarioRegistry::cancel_all()`
before joining the `JoinSet`, so a scenario driver task that only
watches the child token observes the cascade deterministically.

### Tests

`runtime/scenario_registry.rs::tests` adds 7 unit tests covering:

- admit-then-active-count grows;
- active capacity is per-family (32 V1 + at least one V2 still admits);
- mixed-family full capacity (32 V1 + 32 V2 concurrent);
- retention pruning only drops finished runs in the relevant family;
- take_token removes and returns;
- cancel_all signals both families;
- unknown run lookups return `None` and updates are silent no-ops.

`runtime/tests.rs` adds 5 integration tests covering the full
`ControlState` surface:

- `m050_run_ids_are_globally_monotonic_across_families` — V1, V1,
  V2, V2 admissions produce strictly-monotonic `run_id` values across
  families, proving the single `next_run_id` allocator still works.
- `m050_mixed_family_full_capacity_is_supported` — 32 V1 runs admit
  serially, the 33rd V1 admission succeeds by evicting the oldest
  finished V1 entry (family-local pruning), and a V2 admission then
  works independently.
- `m050_active_capacity_admission_is_per_family` — when 32 V1 runs
  are simultaneously active, the 33rd V1 active admission is rejected
  with the typed conflict, V2 admission is unaffected, and cancelling
  the V1 runs reopens V1 capacity.
- `m050_unknown_get_and_cancel_are_no_ops` — `get`/`cancel`/`update`
  on unknown IDs return `None` and are silent no-ops, matching the
  pre-M050 contract.
- `m050_shutdown_cancels_both_families` — after `shutdown_and_join`,
  every active V1 and V2 run's final record reads `Cancelled`; no
  detached task is left behind.

## Invariant non-regression

- Frozen public symbol set: every public `ControlState` method
  retained its name, signature, and observable behavior. No public
  item was added or removed.
- Frozen public record shapes: `ScenarioRunRecord` and
  `ScenarioScheduleRunRecord` are unchanged. JSON / wire DTOs are
  unchanged (no test or schema required editing).
- Frozen scenario semantics: V1 namespace derivation remains
  `(scenario seed, run id, event index)`; V2 namespace derivation
  remains `(scenario seed, execution key, fingerprint, compiled event
  index)`. V1 event timing and V2 strict/live isolation/cleanup
  semantics are untouched.
- Frozen shared authorities: `next_run_id` and `scenario_tasks`
  remain the single run-ID allocator and single supervisor; V1 and V2
  share both.
- Frozen effective capacity: V1 holds 32 retained/active entries;
  V2 holds 32 retained/active entries; both concurrently. The
  registry does not collapse to a single 32-cap.
- Frozen compatibility surfaces: native `/v1` DTOs, OpenAPI shape,
  TOML configuration, CLI behavior, Toxiproxy mappings, SDK behavior,
  Python-native behavior are all unchanged.

## Required verification (results on `b2891a3`)

| Command | Result |
| --- | --- |
| `cargo test -p eggchaos-server --all-features` | green (149 lib + 1 route inventory + 2 schedule corpus) |
| `cargo test -p eggchaos-experiment --all-features` | green |
| `cargo test -p eggchaos-protocol --all-features` | green |
| `cargo test --workspace --all-features` | green |
| `./scripts/check_openapi.sh` | `{"openapi":"pass","paths":21,"operations":36}` |
| `./scripts/check.sh` (fmt/clippy/test/doc) | exit 0 |
| `sh scripts/tests/test_bench_provenance.sh` | `{"bench_provenance":"pass"}` |

The TypeScript/Python/Eggfetch/Toxiproxy/python-native gates are not
required by M050 itself; the local OpenAPI drift guard plus the
M050 mixed-family integration tests are the protocol boundary M050
was explicitly forbidden from changing.

## Successor activation

Closing M050 activates M051 (currently `blocked` in `plans/registry.md`).
M051 may begin from this exact M050 closure candidate `b2891a3` as its
activation baseline. M051 will register its own exact implementation
candidate when it closes.
