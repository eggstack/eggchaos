# Scenarios and observability

Deep dive for the deterministic scenario drivers (V1 + V2) and the
evidence-first observability surface. See [architecture
overview](overview.md) for the workspace map; this document covers
`crates/eggchaos-server/src/scenario.rs` (V1),
`crates/eggchaos-experiment/` (V2 semantic authority, shared schedule
driver, consumer-neutral experiment harness — ADR 004/005, M026–M031),
`crates/eggchaos-server/src/scenario_v2/` (V2 server wiring:
`ControlState` target adapter, run records, compat re-exports),
`crates/eggchaos-server/src/runtime/` (run supervision, run-record
maps, `metrics_text`), and the evidence/snapshot/metrics types they
publish through.

> Evidence-first: every claim below names the file that owns it. No payload
> bytes are captured anywhere in this surface.
>
> Verified at HEAD against M041 (`724b967`, corrective successor to M040)
> including the M026–M031 ADR 004/005 tranche and the M041 stream-loss
> metrics/tooling closure. Stale pre-split `runtime.rs` line refs were
> re-pointed at `runtime/{mod,control,connection,model,metrics}.rs`;
> stale `scenario_v2/*.rs` semantic refs were re-pointed at
> `eggchaos-experiment`.

## 1. Scenario model

Owner: `crates/eggchaos-server/src/scenario.rs`
(V1), `crates/eggchaos-experiment/` (V2 semantics, shared driver,
experiment harness), `crates/eggchaos-server/src/scenario_v2/`
(V2 server wiring), `crates/eggchaos-server/src/runtime/control.rs`
(run supervision: `start_scenario`, `start_schedule_v2`, cancel/get
accessors), and `crates/eggchaos-protocol/` (wire DTO authority,
re-exported for source compatibility by `native.rs` / `native_v2.rs`).

### 1.1 Document types

- `Scenario { version, seed, events }` (`scenario.rs:10-19`): deterministic,
  bounded scenario document.
  - `version` must be `1`; anything else is rejected in `validate_scenario`.
  - `seed: u64` is recorded in the run record and mixed into every published
    policy seed namespace (see §1.5).
  - `events: Vec<ScenarioEvent>` is capped at 1024 entries.
- `ScenarioEvent { at_ms, action }` (`scenario.rs:22-28`): one
  monotonic-time event. `at_ms` is milliseconds from scenario start.
- `ScenarioAction` (authority: `eggchaos-experiment/src/action.rs:13-50`,
  re-exported from `scenario.rs:30-34` for compatibility): the shared
  stream/datagram actions:
  - `SetPlan { proxy, direction, faults }` — replace one directional plan
    as a barrier generation.
  - `RemoveFault { proxy, direction, id }` — remove one fault from a
    directional plan.
  - `SetDatagramPlan { proxy, direction, faults }` /
    `RemoveDatagramFault { proxy, direction, id }` — datagram twins.

The `/v1/scenarios/apply` body uses `ScenarioV1` from the protocol DTO
authority (via `native.rs`), not the internal Serde layout. Each event has
`at_ms` plus a nested `action` object tagged by kebab-case `type`
(`set-plan`, `remove-fault`, `set-datagram-plan`,
`remove-datagram-fault`). `set-plan` faults use the explicit `FaultKindV1`
schema shared with fault CRUD; duration attributes are integer
nanoseconds. Scenario responses use `ScenarioRunV1` and lowercase status
values. V2 wire DTOs (`ScenarioScheduleV2Dto`,
`ScenarioScheduleV2Toml`, `ScheduleRunV2`, `ScheduleValidateV2`,
`ScheduleCompileV2`, `ScenarioActionDto`) live in `eggchaos-protocol`;
`crates/eggchaos-server/src/native_v2.rs:1-11` is only a compat
re-export — it owns no schema.

### 1.2 Validation (`validate_scenario`, `scenario.rs:92-117`)

The document validates entirely before a run begins; the run task never
starts for an invalid document:

1. `version == 1`, else `InvalidProxy("unsupported scenario version")`.
2. `events.len() <= 1024`, else `InvalidProxy("too many scenario events")`.
3. `at_ms` non-decreasing in document order (`event.at_ms < previous` fails
   with `"scenario events must be ordered"`). Equal timestamps are allowed;
   the wait between them is zero.
4. Per-action `validate_action` (`scenario.rs:119-215`):
   - `SetPlan`: `FaultPlan::new(faults)` must succeed (native plan
     invariants), proxy must exist via `snapshot_policies`, else
     `"scenario proxy not found"`.
   - `RemoveFault`: `FaultId::new(id)` must parse, proxy must exist, and the
     named fault must already be present on the target direction's base plan,
     else `"scenario fault {id} not present on {proxy}"`.
   - `SetDatagramPlan`: `DatagramPlan::new(faults)` must succeed, the
     datagram proxy must exist (else `"scenario datagram proxy not
     found"`), and replacement fault IDs must be unique **across both
     directions** — a collision with the untouched direction fails with
     `"datagram fault IDs must be unique across directions"`.
   - `RemoveDatagramFault`: ID must parse and the fault must already be
     present on the target direction's datagram plan, else
     `"scenario datagram fault {id} not present on {proxy}"`.

### 1.3 Driver (`drive_scenario_run`, `scenario.rs:222-499`)

`ControlState::start_scenario` (`runtime/control.rs:1432-1500`) validates,
assigns `run_id` from the **shared** `next_run_id` (starts at 1,
`runtime/mod.rs:321,350`; shared with V2, so V1/V2 IDs share one
namespace), inserts a `Pending` record, creates a
`shutdown_token.child_token()`, and spawns `drive_scenario_run` on the
shared `scenario_tasks` JoinSet (`runtime/mod.rs:305-309`) — never
detached. The driver:

1. Marks the run `Running`.
2. For each `(index, event)` in order, waits
   `event.at_ms.saturating_sub(previous)` via `tokio::time::sleep`, raced
   against `token.cancelled()` in a `biased` `select!`. Cancellation wins
   immediately, marks `Cancelled`, drops the token, and returns
   (`scenario.rs:234-249`).
3. Dispatches on transport. Stream events snapshot the **currently live**
   plans at fire time (`snapshot_policies`, `scenario.rs:399`), clone them,
   and apply the action to the in-memory copy — never a stale snapshot.
   `RemoveFault` re-checks presence at fire time; a fault removed manually
   since validation fails the run here. Datagram events take the sibling
   branch (`scenario.rs:264-398`): read the live datagram plan/generation
   (`get_datagram_plan`), re-validate (`DatagramPlan::new`, cross-direction
   uniqueness for `set-datagram-plan`, presence for
   `remove-datagram-fault`), and publish with an expected-generation guard
   (`publish_datagram_plan(…, Some(expected))`, `scenario.rs:358-361`).
4. Publishes **only the target direction** with an expected-generation guard
   (`publish_direction_expected`, `scenario.rs:450-464`; authority at
   `runtime/control.rs:1379-1410`). The other direction keeps its plan,
   generation, and namespace untouched.
5. On success appends a `ScenarioEventResult` to the trail and increments
   `applied`. On any failure calls `fail_run` (`scenario.rs:501-509`) and
   returns — fail-fast, see §1.6.
6. After all events, marks `Completed` and drops the token.

`previous` tracks `at_ms`, so waits are deltas, not absolute sleeps.

### 1.4 Barrier generations and conflict semantics

Each event is a barrier generation in the `LivePolicy` sense:

- Barrier transition (`stream.rs:614-639`): already-accepted preserving
  bytes drain before the engine swaps; already-discarded blackhole bytes
  stay discarded. The `pending_generation` marker is visible to evidence
  readers while draining (0 = none; `StreamEvidence::pending_generation`,
  `stream.rs:107-109`). Termination handles are shared across the swap
  (first request wins).
- Publish paths are compare-and-swap on the expected generation.
  `publish_direction_expected` maps `PublishError::Conflict` to
  `ControlError::Conflict` with `conflict_message`
  (`runtime/connection.rs:230`): `"proxy {p} policy moved from generation
  {e} to {f} during the operation; retry from current state"`.
- Consequence: a concurrent manual publication between scenario events does
  **not** get silently overwritten. The scenario event's expected base is
  stale, the publish returns `Conflict`, and the run fails fast with
  `"scenario event failed: conflict: ..."` instead of rolling state back.
  This is the documented contract in `docs/control-plane.md` (§Scenarios)
  and the driver header (`scenario.rs:217-221`).
- `GET` proxy views never mix generations: plan, generation, and seed
  namespace come from one atomic `LivePolicy::snapshot()` per direction
  (`ProxyView` construction, `runtime/mod.rs:354-369`).

### 1.5 Derived seed namespaces (`derive_policy_seed`)

Owner: `crates/eggchaos-core/src/rng.rs:54-63` (v1) and
`derive_schedule_policy_seed` at `rng.rs:87-112` (v2).

Each v1 event derives its namespace purely as
`derive_policy_seed(scenario.seed, run_id, index)` (`scenario.rs:358,449`;
stream and datagram branches alike). Manual control updates retain the
current namespace; only scenario events (and explicit publish callers) set
a new one. The published namespace feeds fault-local RNG compilation for
every connection that observes the policy, so the scenario seed
participates in deterministic engine decisions.

Covered by `scenario_seed_drives_published_namespaces` and
`scenario_namespaces_replay_independent_of_scheduling`
(`runtime/tests.rs:1360,1426`): different seeds publish different
namespaces for the same event identity; same seed + identity under
different `at_ms` timing publishes the same namespaces.

Scenario v2 namespaces are derived as
`derive_schedule_policy_seed(scenario.seed, execution_key,
schedule_fingerprint, compiled_event_index)` (`driver.rs:250-255`).
Daemon `run_id` is **not** an input. The v2 helper lives in
`crates/eggchaos-core/src/rng.rs` next to the v1 helper so any audit
can compare the contracts side-by-side. The schedule fingerprint itself
is the SHA-256 digest produced by `compiled_fingerprint` in
`crates/eggchaos-experiment/src/fingerprint.rs`; see
§1.8.1 below for the canonical encoding.

### 1.6 Run lifecycle, fail-fast, cancellation

`ScenarioRunStatus` (`scenario.rs:37-51`):

```text
Pending -> Running -> Completed
               |  \-> Failed (fail-fast, failure: Some(_))
               \---> Cancelling -> Cancelled
```

- `Pending`: validated, waiting for the first event (initial record in
  `start_scenario`).
- `Running`: set by the driver on entry.
- `Cancelling`: set by `cancel_scenario`
  (`runtime/control.rs:1515-1531`) when a token exists and the record is
  `Pending`/`Running`. Cancelling a finished run returns its final record
  unchanged; unknown IDs return `None`.
- `Cancelled`: set by the driver when the token fires before the next
  event. `remove_scenario_token` cleans up.
- `Completed`: all events applied.
- `Failed`: stopped early by the first failed event. `fail_run`
  (`scenario.rs:501-509`) records `failure: Some(message)` and drops the
  token. Failure sources: proxy deleted since validation
  (`"scenario proxy {p} not found"` / `"scenario datagram proxy {p} not
  found: …"`), plan invalid at fire time, fault absent at fire time,
  cross-direction datagram ID collision, publish conflict/invalid.

Fail-fast means the trail stops at the failure: `applied` counts only
successful events, no rollback of already-published generations, no skipped
events. Observable via `scenario_failure_is_observable_not_discarded`
(`runtime/tests.rs:1563`): delete the proxy mid-run, the run reports
`Failed` with the proxy name in `failure`.

Cancellation bounds:

- `cancel_scenario` cancels the per-run child token; the driver's biased
  select returns without sleeping out the remaining `at_ms`.
  `scenario_cancel_during_sleep_returns_boundedly` uses `at_ms: 30_000`
  and asserts termination well under 10 s (`runtime/tests.rs:1486`).
- Service shutdown cascades: run tokens are children of `shutdown_token`,
  and `shutdown_and_join` joins `scenario_tasks`, so no run outlives the
  service. `service_shutdown_cancels_active_scenarios`
  (`runtime/tests.rs:1530`) asserts a sleeping run ends `Cancelled`.
- At most 32 run records are retained (`MAX_SCENARIO_RUNS`,
  `runtime/control.rs:1372`). `start_scenario` fails fast with
  `Conflict("too many active scenario runs")` at the active cap, and
  otherwise prunes oldest *finished* runs first; active runs are never
  evicted to make room. The same constant bounds the V2 map (§1.9).

### 1.7 Run records (no payloads, bounded)

`ScenarioEventResult` (`scenario.rs:54-72`): per-event evidence —
`index`, `at_ms`, `action` summary string (`"set-plan"` /
`"remove-fault"` / `"set-datagram-plan"` / `"remove-datagram-fault"`),
`proxy`, `direction`, `global_generation`, `upstream_generation`,
`downstream_generation`. No fault bodies, no byte payloads.

`ScenarioRunRecord` (`scenario.rs:75-89`): `run_id`, `seed`, `status`,
`applied`, `failure: Option<String>` (short detail only), `trail:
Vec<ScenarioEventResult>`. The trail is bounded by the 1024-event document
cap; runs are bounded by the 32-record retention cap. Serialization never
contains payload bytes (`connection_evidence_contains_no_payload_bytes`,
`runtime/tests.rs:2012`, asserts the same for connection/history JSON).

### 1.8 Scenario v2 source language and compiler (M026; extracted to `eggchaos-experiment` in M030)

Owner: `crates/eggchaos-experiment/src/{source,compiler,fingerprint,run,action}.rs`.
The server keeps source-compatible re-exports under
`crates/eggchaos-server/src/scenario_v2/mod.rs:26-34`. Compiler,
fingerprint bytes, namespace vectors, and golden corpus are unchanged
by the extraction (proven by `schedule_corpus` and `scenario_v2`
suites running against the re-exports).

`ScenarioScheduleV2` (`experiment/src/source.rs:104-125`) is the bounded
piecewise-constant source language ADR 004 introduced. A schedule is a
`version: 2` document (`SCHEDULE_SCHEMA_VERSION`, `source.rs:26`) with
explicit `seed`, `execution_key`, `isolation` (`strict` or `live`,
`source.rs:28-47`, default `Strict`), `cleanup` (`restore-initial` or
`leave`, `source.rs:49-63`, default `RestoreInitial`), an ordered
`phases` list, and an optional one-level `repeat` block. Each phase
carries an optional bounded name (presentation-only, not in the
fingerprint), an integer nanosecond `duration_ns`, and a non-empty list
of actions. The four v1 scenario actions remain the only actions in
scope; v2 adds the source-level naming, durations, repetition, isolation,
and cleanup without introducing new fault kinds.

The compiler (`compile_schedule`, `experiment/src/compiler.rs:96-177`)
is a pure function of `source × COMPILER_SEMANTICS_VERSION`
(`compiler.rs:25`, currently `1`). It rejects:

- unsupported `version` values (`UnsupportedVersion`);
- empty schedules (`EmptySchedule`, incl. empty repeat blocks and
  top-level-empty with no repeat), empty phases (`EmptyPhase`), empty or
  oversize phase names (`InvalidPhaseName`; empty string and
  `> MAX_PHASE_NAME_BYTES = 128` both rejected, `source.rs:162,182`);
- more than `MAX_PHASES = 256` phases (top-level or repeat block),
  `TooManyRepeatPhases` for oversize repeat blocks, more than
  `MAX_REPEAT_COUNT = 64` iterations (`RepeatOutOfRange`, and `0`
  rejected), more than `MAX_PHASE_ACTIONS = 64` actions per phase;
- duration arithmetic that would overflow `u64` (`OffsetOverflow`,
  via `checked_add` in `phase_target_offset`, `compiler.rs:84-89`);
- `FaultPlan` / `DatagramPlan` validation failures inside actions
  (`InvalidStreamPlan` / `InvalidDatagramPlan`).

The compiled output is `CompiledScenarioV2` (`compiler.rs:60-73`): an
immutable `events: Vec<CompiledEventV2>` where every entry carries its
stable `compiled_index`, a `CompiledPhaseIdentity` (`Top { index }` or
`Repeat { iteration /* 1-based */, index }`, `compiler.rs:33-40`), an
absolute `offset_ns` from the run epoch, and the action. Equal-offset
events preserve source order, so the compiled tape is unique for a given
semantic source. The compile ceiling (`MAX_COMPILED_EVENTS = 1024`,
`source.rs:15`) matches the v1 cap; expansion is count-checked before
allocation (`compiler.rs:100-109`) and re-checked after
(`compiler.rs:165-167`): there is no partial compile followed by
truncation.

#### 1.8.1 Canonical SHA-256 fingerprint

`compiled_fingerprint` (`experiment/src/fingerprint.rs:39-49`) produces a
32-byte SHA-256 digest. The SHA-256 input is the domain/version prefix
`eggchaos/scenario-v2/fingerprint/v1/compiler-semantics=`
(`FINGERPRINT_DOMAIN`, `fingerprint.rs:23`) followed by the 4-byte
big-endian `COMPILER_SEMANTICS_VERSION` and then the explicit semantic
encoding `encode_compiled_for_fingerprint` (`fingerprint.rs:66-82`). The
encoding writes one line per axis (`isolation=`, `cleanup=`, `seed=`,
`execution_key=`, `event_count=`) followed by one `\nevent[i] phase=...
offset_ns=... action=...` line per compiled event (`write_event`,
`fingerprint.rs:278-287`). The action block lists `(proxy, direction,
fault id, probability, kind)` tuples in source order with probabilities
rendered at fixed `{:.17}` precision; stream `stream-loss` faults encode
as `stream-loss;loss_rate=…;correlation=…` (`fingerprint.rs:232-241`,
M036/M037 propagation of the ADR 007 primitive). Optional phase names do
not participate; nor do daemon `run_id`, source file path, or
timestamps. The encoding is hand written instead of using
`serde_json::Value` so that hash-map iteration order, serde formatter
whitespace drift, and Serde defaults cannot leak into the digest.

Two inputs produce the same fingerprint iff they compile to identical
event tapes. The fingerprint is diagnostic/replay identity, not
authentication; it lives only in run evidence and the v2 namespace
derivation.

#### 1.8.2 V2 namespace derivation

`derive_schedule_policy_seed` (`crates/eggchaos-core/src/rng.rs:87-112`)
derives a portable v2 policy namespace as

```text
derive_schedule_policy_seed(
    scenario_seed,
    execution_key,
    schedule_fingerprint, // 32 bytes
    compiled_event_index,
)
```

The helper lives in `eggchaos-core` next to `derive_policy_seed` so the
two contracts are auditable side-by-side, and it never depends on
`run_id`, task scheduling, wall-clock time, or hash-map order. The golden
vectors in
`rng.rs::tests::schedule_policy_seed_derivation_is_stable_and_sensitive`
(`rng.rs:196-242`) pin the byte-fold; sensitivity to `execution_key`,
fingerprint bytes, and `compiled_event_index` is asserted alongside the
v1 vectors (`rng.rs:240-241`), which must remain byte-identical. A new
version requires a registered version bump in the v2 domain prefix and a
regenerated golden vector set.

#### 1.8.3 Source parsing and round trips

The wire DTOs (authority: `eggchaos-protocol`, re-exported by
`native_v2.rs`) convert into the internal `ScenarioScheduleV2` at the
admin edge (`admin.rs:624-636,656-688`). JSON arrives as
`ScenarioScheduleV2Dto`; the CLI forwards TOML schedules by translating
to the semantic JSON document server-side authority-side (`cli
main.rs:724-738`; TOML selected by extension, 1 MiB cap enforced
client-side too). Both paths reject unknown fields. The wire action
vocabulary (`set-plan`, `remove-fault`, `set-datagram-plan`,
`remove-datagram-fault`) is identical to the v1 kebab-case tags so the
runtime can keep its existing authority. TOML durations are integer
nanoseconds; human duration strings are a separate plan/ADR and
deliberately not part of M026 because silent rounding at the parser edge
is a stop condition. JSON↔TOML semantic equivalence is pinned by
`json_and_toml_source_form_yield_identical_fingerprint` and
`toml_and_json_v2_apply_compile_to_the_same_fingerprint`
(`scenario_v2/tests.rs:606-696`, `runtime_tests.rs:945-998`).

#### 1.8.4 What M026 does not introduce

M026 implements only the compiler, fingerprint, and namespace helpers.
It deliberately ships:

- no new HTTP route and no native `apply` for v2 schedules;
- no CLI subcommand for v2 schedules;
- no run supervisor task for v2 schedules;
- no change to stream or datagram engine semantics.

M027 (below) wires the compiled tape into the owned scenario
supervisor via `ControlState` and the existing publication paths.
M028 qualifies the combined surface on an exact candidate. (Historical
scope note — M027/M028 are closed; the routes, CLI, and supervisor
described in §1.9 exist.)

### 1.9 Scenario v2 runtime, isolation, and lifecycle (M027; driver shared via `eggchaos-experiment` in M030)

Owner: `crates/eggchaos-experiment/src/driver.rs` (shared driver),
`crates/eggchaos-server/src/scenario_v2/runtime.rs` (`ControlState`
target adapter + run-record sink), plus
`ControlState::start_schedule_v2` and friends
(`runtime/control.rs:1709-1862`), routes (`admin.rs:623-732`), and CLI
(`cli/main.rs:319-426,724-738`). Server behavior is unchanged by the
M030 extraction: the same driver executes through the same
`ControlState` publication methods, and the run record remains the
single source of truth (proven by unchanged `runtime_tests` and the new
`conformance_tests`).

`ControlState::start_schedule_v2` (`runtime/control.rs:1709-1783`)
compiles the source entirely upfront — a compile failure creates no run
and consumes no run ID (`control.rs:1713-1714`) — then registers a
`ScenarioScheduleRunRecord` (run ID from the shared `next_run_id`, so
v1/v2 IDs share one namespace), bumps the coarse
`schedule_v2_runs` counter, stores a child of the service shutdown
token, and spawns `drive_schedule_v2_run` on the **shared**
`scenario_tasks` JoinSet. There is no second supervisor registry;
shutdown joins v2 tasks with v1 tasks (`shutdown_and_join`,
`control.rs:1668-1680`), and the 32-record `MAX_SCENARIO_RUNS` bound
applies to the v2 map as it does to v1. `cancel_schedule_v2`
(`control.rs:1801-1820`) mirrors `cancel_scenario`, and
`GET`/`DELETE /v1/scenarios/{run_id}` serve both versions (v1 map first,
then v2; `admin.rs:689-732`).

At task entry the server path captures the epoch via
`EpochGate::started()` (`scenario_v2/runtime.rs:256-257`), preserving
the historical run-entry timing anchor, then runs
`PreparedExperiment::prepare_compiled` (compile already done; capability
check + initial snapshots, publishing nothing) and
`run_from_epoch`. Preparation failure ends the run `Failed` with an
empty `CleanupOutcome` over the schedule's own cleanup policy
(`scenario_v2/runtime.rs:233-254`); per-event evidence accumulates via
`append_schedule_v2_event` (`control.rs:1837-1857`), which also bumps the
coarse `schedule_v2_events` / `schedule_v2_late_events` counters.

#### 1.9.1 Epoch-anchored driver

The driver (`driver.rs:144-206`) takes one captured epoch; every
deadline is `epoch + offset_ns`, waited with `sleep_until` raced against
cancellation in a `biased` select so cancellation wins
(`wait_for_deadline`, `driver.rs:216-236`). An already-due deadline
skips the sleep and applies immediately in compiled-index order, so slow
event application never shifts later deadlines. A pre-cancelled token
short-circuits before any publication (`driver.rs:162-165`). An
unrepresentable deadline (`epoch.checked_add` overflow near the `u64`
limit) waits `pending` — never panics — until cancellation arrives
(`driver.rs:221-227`; pinned by
`unrepresentable_deadline_near_u64_limit_cancels_without_panic`,
`runtime_tests.rs:1081-1110`). Actual elapsed time is measured from the
same epoch and `late_by_ns = max(applied_elapsed - scheduled_offset, 0)`
is recorded per event. `SystemTime` never participates in schedule
authority. Paused-time anchoring (no drift across sparse multi-hour gaps)
is pinned by `paused_time_events_fire_at_epoch_offsets_without_drift`
and `sparse_events_with_long_gaps_stay_epoch_anchored`
(`runtime_tests.rs:179-235,1036-1078`).

#### 1.9.2 Strict/live generation ownership

During preparation, `prepare_initial` (`driver.rs:82-133`) snapshots
every touched directional policy (plan + generation for stream,
plan/generation for datagram) and checks target capabilities; a missing
proxy or an unsupported transport fails preparation before anything
publishes. Strict mode (default) publishes each event against the
generation last owned by the run (`expected_generation`,
`driver.rs:439-452`) — an external manual/scenario move makes the next
event's expected-generation guard fail, so the run fails fast instead of
incorporating or overwriting external state. Live mode re-reads the live
generation at fire time (`current_generation`), so a completed manual
update may become the base, while a concurrent move during publication
still conflicts. Both modes publish through the existing
`publish_direction_expected` / `publish_datagram_plan` authority via the
`ControlStateTarget` adapter (`scenario_v2/runtime.rs:31-161`,
`map_publish_error` re-reads the live generation for an exact
`expected/found` pair, `runtime.rs:166-188`); the v2 namespace for each
event is `derive_schedule_policy_seed(seed, execution_key, fingerprint,
compiled_index)` and never involves `run_id` or lateness. Concurrent
strict schedules conflict without overwriting each other
(`concurrent_strict_schedules_conflict_without_overwrite`,
`runtime_tests.rs:1152-1230`).

#### 1.9.3 CAS-safe cleanup

After the terminal outcome (Completed, Failed, or Cancelled) the driver
runs cleanup once (`run_cleanup`, `driver.rs:506-526`).
`leave` records `NotRequested` per resource and publishes nothing.
`restore-initial` republishes each touched resource's snapshotted
initial plan only when the live generation still equals the generation
last owned by the run (`restore_target`, `driver.rs:531-550`);
otherwise it records `Conflict` and leaves external state intact, or
`Missing` when the resource is gone. Cleanup publishes with seed
namespace `0` (`driver.rs:544`) — it restores plans, not scenario RNG
participation. Cleanup of one conflicted resource never blocks bounded
cleanup of the others
(`cleanup_conflict_on_one_resource_does_not_block_another`,
`runtime_tests.rs:742-815`), and the original run outcome is retained
alongside the cleanup outcome. Cleanup performs in-process publications
only — it never waits on data-plane drain.

#### 1.9.4 Run evidence and operator surface

`ScenarioScheduleRunRecord` (`experiment/src/run.rs:132-158`) carries
`run_id`, `seed`, `execution_key`, 32-byte `schedule_fingerprint`,
compiler version, isolation/cleanup policy, status, applied count,
bounded failure detail, the per-event trail (`compiled_index`, phase
identity, scheduled/applied/late nanoseconds, action summary,
proxy/direction/transport, resulting generations), and the cleanup
outcome. No payload bytes. The native responses (`ScheduleRunV2`,
`ScheduleValidateV2`, `ScheduleCompileV2` in `eggchaos-protocol`,
rendered at `admin.rs:627-629,659-662,676-679`) render the fingerprint
as lowercase hex and the phase as `top/{i}` or `repeat/{iter}/{i}`
(`phase_identity_string`). Metrics add three coarse counters
(`eggchaos_schedule_v2_runs_total`,
`eggchaos_schedule_v2_events_total`,
`eggchaos_schedule_v2_late_events_total`, `runtime/control.rs:78-94`)
with no run, fingerprint, phase, or key labels
(`v2_metrics_count_runs_events_and_late_events`,
`runtime_tests.rs:1001-1033`).

Known evidence limitation: datagram event records do not read the
sibling direction — `set-datagram-plan` records the published generation
for the target direction and `0` for the other (`driver.rs:362-365`),
and `remove-datagram-fault` records `0` for both (`driver.rs:415-426`).
Stream events always pair the receipt with the sibling's live generation
(`directional_gens`, `driver.rs:459-474`).

#### 1.9.5 Replay limits for v2

Policy/event identity is exact across daemon restarts and run ordering:
the same schedule document replays identical seed namespaces and
therefore identical fault decisions for the same connection keys
(`same_schedule_under_different_run_ids_publishes_identical_namespaces`,
`runtime_tests.rs:311-361`). Live connection timing (accept order →
connection keys), datagram arrival timing, wall-clock
event-application cost, and scheduler lateness are not replayed;
lateness is diagnostic evidence recorded per event, never an RNG input.
Strict-mode conflict/cleanup-conflict evidence distinguishes "the world
moved" from "the schedule is wrong".

### 1.10 Consumer-neutral experiment harness and coordinated start (M030)

Owner: `crates/eggchaos-experiment/` (`target.rs`, `driver.rs`,
`gate.rs`, `experiment.rs`, `stream_target.rs`); server adapter in
`crates/eggchaos-server/src/scenario_v2/runtime.rs`; conformance in
`scenario_v2/conformance_tests.rs`.

`PolicyTarget` (`experiment/src/target.rs:123-151`) is the one narrow
publication contract the shared driver requires: `capabilities()`,
`snapshot(resource)`, `current_generation(resource)`,
expected-generation `publish(resource, expected, plan, seed_namespace)`,
and `global_generation()`. `TargetError` has bounded stable categories
(`MissingResource`, `UnsupportedCapability`, `GenerationConflict {
expected, found }`, `Validation`, `Internal`;
`target.rs:60-116`, details truncated to `MAX_TARGET_LABEL_BYTES = 256`
bytes on a character boundary); no payload or consumer product model
enters the contract, and no lock is held across schedule sleeps
(`target.rs:118-122`).

`PreparedExperiment::prepare` (`experiment.rs:142-145`) compiles,
validates target capabilities, resolves touched resources, and snapshots
initial state — publishing nothing and starting no clock.
`prepare_compiled` (`experiment.rs:150-162`) skips compilation for the
server path. `EpochGate` (`gate.rs:30-80`) captures one Tokio monotonic
`Instant` exactly once (`start` is idempotent — extra calls return the
retained epoch) and releases it to both the schedule driver and the
caller workload (`run` awaits the waiter, `experiment.rs:189-197`;
`run_from_epoch` takes an already-captured epoch,
`experiment.rs:200-234`; `run_with_workload` starts one gate for
schedule + workload closure, `experiment.rs:243-259`); every deadline
stays `epoch + compiled_offset`. Cancellation before start publishes
nothing; cancellation while sleeping wakes promptly; cancellation after
publications runs the selected cleanup. The driver future is
caller-owned; dropping a prepared experiment publishes nothing and
spawns nothing. Evidence (`ExperimentEvidence`,
`experiment.rs:60-79`) carries compiler version, fingerprint hex, seed,
execution key, optional caller integration identity
(`≤ MAX_EXPERIMENT_IDENTITY_BYTES = 128` bytes,
`experiment.rs:36,170-180`), applied count, outcome, and cleanup —
portable reports use relative offsets only, never the raw epoch (the
`Instant` is process-local and debug-only).

`StreamPolicyTarget` (`experiment/src/stream_target.rs:40-43`) maps one
logical resource name to an M029 `LivePolicy` upstream/downstream pair
(`register` by name, `stream_target.rs:68-87`; names bounded by
`MAX_RESOURCE_NAME_BYTES = 128`, sorted listing via `names()`):
typically a chaos-dialer policy pair, so stream Scenario V2
publications reach the authorities already-open pooled physical
connections observe. Stream-only: datagram actions return typed
`UnsupportedCapability` (`stream_target.rs:102-116,157-161`), never
silently translated. Its `global_generation` is a local publish counter
(`stream_target.rs:182-184`), a different authority from the server
global by design. The server adapts `ControlState` (stream + datagram
capabilities) without a second state store;
`conformance_tests` proves the two targets produce equivalent
event/generation outcomes for the supported stream subset
(`assert_events_conform` compares everything except the
target-wide global generation,
`conformance_tests.rs:152-162`; strict, live-with-prior-publish, and
datagram-rejection cases at `conformance_tests.rs:164-297`).
Downstream EggReplay/EggProbe adoption remains downstream work after
M031.

## 2. Observability

### 2.1 Stream / engine / RNG evidence

Owners: `crates/eggchaos-core/src/stream.rs`, `engine.rs`, `rng.rs`.

- `StreamEvidence` (`stream.rs:76+`): lock-shared live evidence per
  direction, read by the runtime without locking the stream. Atomics for
  `observed_generation`, `pending_generation` (0 = none,
  `stream.rs:107-109`), `seed_namespace`, byte counts
  (`byte_counts`, `stream.rs:120`), high-water mark, `transitions`,
  per-fault-type `activations[7]` (`stream.rs:190`); fault identities in
  a bounded structure. `refresh_policy` stores the published snapshot's
  generation/namespace/identities; engine counters are mirrored after
  each drive; the direct (no-fault) path counts bytes immediately so
  evidence never lags a completed write.
- `EngineEvidence` (`engine.rs:128+`): direction-local counters —
  `bytes_accepted/forwarded/discarded`, `segments`, `slices`,
  `buffered_bytes`, `high_water_bytes`, `injected_delay_ms`,
  `throttled_delay_ms`, `termination`, `activations[7]`, `rng_version`.
  Activation semantics are documented per stage (preserving stages count
  per engaged `accept`; blackhole per discarding call; termination stages
  once on first publish; slow-close once on enforced positive delay).
- `RngEvidence { version, seed }` (`rng.rs:9-14`): replay metadata for a
  connection-local deterministic RNG. `RngVersion` is surfaced in
  `DirectionSummary` and `ConnectionSnapshot.rng_version`.
- `ActiveFault { id, fault_type }` (`stream.rs:65+`): connection-active
  fault identity for evidence only. `fault_type` uses the low-cardinality
  `FAULT_TYPE_NAMES` spelling (`plan.rs:190-198`). Retained list is capped
  at `MAX_EVIDENCE_FAULTS = 128` (`stream.rs:61`) with a `truncated` flag.
- `DirectionSummary` (`stream.rs:21+`): serializable per-direction rollup
  — byte counters (including transparent direct-path bytes), segment/slice
  counts, buffer/high-water, delay totals, `activations[7]`, `termination`,
  `rng_version`. `evidence_serializes_without_payloads`
  (`stream.rs:1810`) pins the no-payload contract.
- `stream-loss` (ADR 007, M036–M041) deliberately owns **no**
  legacy activation slot: `FaultKind::type_index` returns `None` for
  `StreamLoss` (`plan.rs:224-235`) so the frozen seven-slot arrays never
  resize or reorder. Its decisions are counted in additive named evidence
  instead — per-direction `stream_loss_chunks_evaluated`,
  `stream_loss_chunks_dropped`, `stream_loss_bytes_discarded`
  (`ConnectionSnapshot` fields, `runtime/model.rs:68-86`; bytes
  discarded are counted once and included in `DirectionBytes.discarded`).
  Chunking is fragmentation-independent: fixed 32 KiB logical grains
  (`STREAM_LOSS_GRAIN_BYTES`, `plan.rs:158`) with a domain-separated
  chunk seed (`derive_stream_loss_seed`, `rng.rs:122-131`).

### 2.2 Connection snapshots and history

Owners: `crates/eggchaos-server/src/runtime/model.rs` (snapshot/history
types), `connection.rs` (merge/record/metrics), `supervisor.rs`
(accept), `control.rs` (readers), `mod.rs` (`AdmissionLimits`).

- `ConnectionSnapshot` (`runtime/model.rs:21-97`): safe operational summary.
  Accept-time identity (`id`, `proxy`, `ordinal`, `peer`, `upstream`,
  `connection_key`, `seed = service_seed ^ proxy_seed`, `generation`,
  `accepted_*_generation/seed`) is frozen at registration
  (`accept_connection`, `runtime/supervisor.rs:50`); `observed_*` /
  `pending_*` / counters / fault lists / additive stream-loss counters
  report live stream state merged at read time via `merge_evidence`
  (`runtime/connection.rs:240`), or final state in history records.
  `DirectionBytes { accepted, forwarded, discarded }`
  (`model.rs:101-108`) per direction; `*_faults_truncated` mirrors the
  128-entry evidence bound.
- `ConnectionEvidence { upstream, downstream: Arc<StreamEvidence> }`
  (`runtime/model.rs:112-117`): registered before relaying starts so
  snapshots report from the first read. `ControlState::connections()` /
  `get_connection()` (`runtime/control.rs:651-666`) merge live evidence
  at read time; connections without registered evidence report
  accept-time values.
- `ConnectionOutcome` (`runtime/model.rs:121-148`): final classification —
  `RelayCompleted`, `ConnectFailed(String)`, `KilledByOperator`,
  `ServiceShutdown`, `ProxyRemoved`, `GracefulTermination { drained }`,
  `HardReset { client, upstream: ResetResult }`, `RelayError(String)`.
  Coarse class order is pinned by `OUTCOME_CLASS_NAMES`
  (`runtime/metrics.rs:47-56`) for the `outcomes[8]` metrics array.
  `ResetResult` (`runtime/transport.rs:14`): `Applied` /
  `Failed(String)` / `Unsupported(String)` — wire-level RST observation
  stays platform-dependent; the recorded value is the socket-API outcome.
- `ClosedConnection { snapshot (state == Closed), outcome,
  upstream_termination, downstream_termination, detail }`
  (`runtime/model.rs:152-163`): bounded retained final record. `detail`
  is `Option<String>` short machine-safe detail (relay report, failure,
  truncation note). Retention is bounded by `AdmissionLimits.history`
  (default 256, `runtime/mod.rs:175-186`); `history == 0` disables
  retention but keeps metrics
  (`history_bound_zero_disables_retention_but_keeps_metrics`,
  `runtime/tests.rs:1601`). Eviction is oldest-first (`record_close`,
  `runtime/connection.rs:21`); `take_connection`
  (`runtime/connection.rs:7`) guarantees exactly-once accounting across
  concurrent finish/purge paths.

### 2.3 Per-proxy metrics and Prometheus surface

Owners: `runtime/metrics.rs` (counters/tables), `runtime/control.rs:62-292`
(`metrics_text`), `docs/control-plane.md` (§Metrics).

- `MetricsCounters` (`runtime/metrics.rs:9-44`): `accepted / completed /
  rejected` totals, `outcomes[8]`, `graceful_requests /
  hard_reset_requests`, `reset_applied / reset_unsupported / reset_failed`,
  `bytes_accepted / forwarded / discarded`, `transitions`,
  `schedule_v2_runs / schedule_v2_events / schedule_v2_late_events`,
  plus `tables: StdMutex<MetricTables>`. Totals derive from final stream
  evidence in `aggregate_close_metrics`
  (`runtime/connection.rs:69`), so counters always agree with the history
  records they summarize. The three v2 schedule counters are coarse
  run/event/late totals with no labels at all.
- `MetricTables` (`runtime/metrics.rs:80-85`): bounded low-cardinality
  tables with overflow buckets. `MAX_METRIC_PROXIES = 1024`,
  `MAX_METRIC_ACTIVATIONS = 8192` (`runtime/metrics.rs:59-61`).
  Per-proxy `PerProxyMetrics { accepted, completed, bytes[2][3],
  stream_loss[2][3] }` (`metrics.rs:65-75`); legacy activations keyed by
  `(proxy, direction, fault_type)` over the frozen 7-type vocabulary
  (`record_activations`, `metrics.rs:123-143`); stream-loss recorded
  separately via `record_stream_loss` (`metrics.rs:113-121`) so legacy
  activation series never gain an eighth member
  (`stream_loss_metrics_are_bounded_and_do_not_extend_legacy_activations`,
  `metrics.rs:151-166`). Overflow proxies fold into a `_overflow`
  series; overflow activations fold into
  `{proxy="_overflow",direction="_overflow",fault_type="_overflow"}`;
  overflow stream-loss folds into `_overflow` per-direction samples.
- `metrics_text()` renders Prometheus text exposition
  (`runtime/control.rs:62-163`):
  - `eggchaos_config_generation` (gauge), `eggchaos_connections_accepted /
    completed / rejected_total`, `eggchaos_connections_active` (live gauge).
  - `eggchaos_connection_outcomes_total{outcome}` (8 coarse classes),
    `eggchaos_termination_requests_total{request="graceful|hard_reset"}`,
    `eggchaos_reset_results_total{result="applied|unsupported|failed"}`,
    `eggchaos_bytes_total{flow}`, `eggchaos_policy_transitions_total`.
  - Per-proxy `eggchaos_proxy_connections_accepted/completed_total{proxy}`,
    `eggchaos_proxy_bytes_total{proxy,direction,flow}`,
    `eggchaos_fault_activations_total{proxy,direction,fault_type}`
    (legacy 7-type vocabulary only).
  - Stream-loss (M041, `control.rs:95-149`): exactly one sample per
    proxy × direction × family —
    `eggchaos_stream_loss_chunks_evaluated_total`,
    `eggchaos_stream_loss_chunks_dropped_total`,
    `eggchaos_stream_loss_bytes_discarded_total` — emitted with a real
    newline, including `_overflow` samples when the proxy table overflows.
    Uniqueness and well-formedness are pinned by
    `stream_loss_prometheus_metrics_count_final_evidence_once` and
    `stream_loss_prometheus_exposition_is_unique_and_well_formed`
    (`runtime/tests.rs:1799,1848`): each labelset occurs exactly once
    with the final-evidence value counted once.
  - Live gauges from policies + live evidence (`control.rs:164-202`):
    `eggchaos_proxy_connections_active{proxy}`,
    `eggchaos_proxy_policy_generation{proxy,direction}`,
    `eggchaos_proxy_queue_bytes{proxy,direction}` (accepted minus
    forwarded+discarded, summed over live evidence).
  - Datagram gauges (`control.rs:203-290`): per-proxy
    `eggchaos_datagram_associations_active`, fixed-`kind` drop
    observations (`oversize`, `capacity_rejection`,
    `ingress_queue_overflow`, `association_setup_failure`),
    `eggchaos_datagram_administrative_discards`, 12-`kind`
    `eggchaos_datagram_evidence` series (current + retained), and
    `eggchaos_datagram_fault_activations` over
    `DATAGRAM_FAULT_TYPE_NAMES`.
- Label discipline: the only label keys are `proxy`, `direction`, `flow`,
  `outcome`, `request`, `result`, `fault_type`, `kind` (datagram only) —
  pinned by `metrics_use_only_bounded_label_keys`
  (`runtime/tests.rs:1745`). Labels never carry connection IDs, peer
  addresses, scenario run IDs, arbitrary fault IDs, hostnames, schedule
  fingerprints, execution keys, or phase names. Directions and fault
  types are fixed vocabularies; proxy names come from the capped table.
- Reconciliation is exact in the deterministic fixture
  (`metrics_reconcile_with_deterministic_fixture`,
  `runtime/tests.rs:1640`): 3 graceful echoes + 1 operator kill assert
  accepted/completed/outcome/termination/byte/activation series verbatim.

### 2.4 Admin inspection routes

Owner: `crates/eggchaos-server/src/admin.rs` + `native.rs` /
`native_v2.rs` (protocol re-exports); contract summary in
`docs/control-plane.md` (§Route inventory, §Scenarios). `POST
/v1/scenarios/apply` is version-aware: the `version` field is peeked
without full parsing (`version_of`, `admin.rs:749-752`) so `version: 2`
schedules route to `start_schedule_v2` and anything else to the v1 path
(`admin.rs:623-654`).

| Method + path | Handler | Payload |
| --- | --- | --- |
| `GET /v1/connections` | `state.connections().await` | `Vec<ConnectionSnapshot>` with live evidence merged |
| `GET /v1/connections/{id}` | `state.get_connection(id)` | one snapshot, or `not_found`; non-integer ID is `invalid` |
| `DELETE /v1/connections/{id}` | `state.kill(id)` | `{id, terminated: true}` or `not_found` |
| `GET /v1/history` | `state.history().await` | bounded `Vec<ClosedConnection>` |
| `POST /v1/scenarios/apply` | version-aware: `ScenarioScheduleV2Dto → start_schedule_v2` else `ScenarioV1 → start_scenario` | `202 ScheduleRunV2` or `202 ScenarioRunV1`; invalid body is a bounded JSON error |
| `POST /v1/scenarios/validate` | `compile_schedule` (no run, no ID) | `ScheduleValidateV2`: fingerprint hex, compiler version, event count, schedule identity |
| `POST /v1/scenarios/compile` | `compile_schedule` (no run, no ID) | `ScheduleCompileV2`: normalized compiled event tape + fingerprint |
| `GET /v1/scenarios/{run_id}` | `state.get_scenario` then `state.get_schedule_v2` | full `ScenarioRunV1` or `ScheduleRunV2` (lowercase status, applied, failure, trail/events) or `not_found` |
| `DELETE /v1/scenarios/{run_id}` | `state.cancel_scenario` then `state.cancel_schedule_v2` | latest run record (active status moves to `cancelling`) |
| `GET /v1/datagram-associations` | datagram runtime associations | live + retained association summaries |
| `GET /v1/datagram-associations/{id}` | active or retained lookup | one association summary or `not_found` |
| `DELETE /v1/datagram-associations/{id}` | administrative kill | kill report |
| `GET /metrics` | `state.metrics_text().await` | Prometheus text, no `/v1` prefix, `text/plain; version=0.0.4` |
| `GET /v1/health`, `GET /v1/version`, `POST /v1/reset` | service routes | liveness/generation, build version, reset report (TCP + datagram) |

Request bodies are capped at 1 MiB (`max_request_body_bytes` and
`RequestBodyPolicy::Buffer { max_bytes: 1024 * 1024 }`,
`admin.rs:107,119-121`); the EggServe H1 runtime owns parsing/body
bounds/connection lifecycle while eggchaos owns only route dispatch and
typed JSON conversion (`docs/control-plane.md:13-16`). The CLI mirrors
the scenario surface (`scenario apply|validate|compile|get|cancel`,
`cli/main.rs:319-426`) and forwards the semantic document to the server
authority without expanding phases or deriving fingerprints
(`read_scenario_document`, `cli/main.rs:724-738`).

## 3. Replay limits

From `docs/control-plane.md` (§Scenarios) and the `rng.rs` / `scenario.rs`
headers — what is and is not reproducible:

- **Reproducible: policy state and per-key decisions.** The same document
  (`version`, `seed`, ordered `at_ms` + actions) reproduces the same seed
  namespaces, therefore the same fault decisions for the same connection
  keys. Derivation is a pure function of `(scenario seed, run id, event
  index)` (`rng.rs:46-63`); it never depends on task scheduling, wall time,
  or connection order. Golden vectors pin the derivation
  (`rng.rs:187-193`: `derive_policy_seed(7,1,0)`,
  `derive_policy_seed(7,1,1)`, sensitivity to the seed axis).
- **Not reproducible: live connection timing.** Connection keys are
  proxy-local accept ordinals (`ordinal`, `supervisor.rs:50`), so they
  depend on accept order. Scheduling interleavings, wall-clock sleeps
  between events, dial latency, relay pump timing, and which connections
  are alive when an event fires are not part of the seed. Re-running a
  document replays namespaces exactly but not live timing — replay is exact
  for policy state, not for connection-timing behavior.
- **V2 is run_id-independent.** Same schedule → same
  `(seed, execution_key, fingerprint, compiled_index)` namespaces
  regardless of daemon run order (`rng.rs:87-112`,
  `runtime_tests.rs:311-361`). Scheduler lateness is diagnostic evidence,
  never an RNG input.
- **Manual mutations move the base.** Events apply against live plans at
  fire time (`scenario_remove_builds_on_current_state`,
  `runtime/tests.rs:1099`: removal builds on current B-state). A manual
  change between events changes what the next event applies to; a
  conflicting concurrent publication fails the run (conflict, §1.4) instead
  of rolling state back.

## 4. Review checklist

For any change touching `scenario.rs`, `scenario_v2/`,
`eggchaos-experiment`, evidence types, `merge_evidence`,
`aggregate_close_metrics`, `metrics_text`, or the admin inspection routes:

- [ ] Scenario document bounds preserved: V1 `version == 1`, `<= 1024`
  events, monotonic `at_ms`, per-action validation (proxy exists, plan
  valid, removed fault present, datagram cross-direction ID uniqueness).
  No new action without a version bump plan.
- [ ] V2 source/compiler bounds preserved: `version == 2`,
  `MAX_PHASES` (256), `MAX_REPEAT_COUNT` (64), `MAX_PHASE_ACTIONS` (64),
  `MAX_COMPILED_EVENTS` (1024), `MAX_PHASE_NAME_BYTES` (128),
  `COMPILER_SEMANTICS_VERSION` (1); fingerprint encoding unchanged
  (phase names excluded, `run_id` excluded, fixed float precision).
  No new fault kind without a fingerprint migration plan.
- [ ] Driver still applies to **live** plans with a single-direction
  expected-generation publish; the untouched direction's plan/generation/
  namespace is unchanged in the trail assertion. V2 strict still fails
  fast on external moves; live still re-reads at fire time.
- [ ] Seed participation intact: V1 namespace is exactly
  `derive_policy_seed(seed, run_id, index)`; V2 namespace is exactly
  `derive_schedule_policy_seed(seed, execution_key, fingerprint,
  compiled_index)`; manual publishes retain the namespace (V2 cleanup
  restores with namespace `0` by design); golden vectors in `rng.rs`
  still pass.
- [ ] Lifecycle complete: `Pending → Running → Completed/Failed`, `Cancelling
  → Cancelled`, token removed on every terminal path, shutdown joins runs
  on the shared JoinSet. No detached scenario task. V1/V2 run IDs share
  one `next_run_id` namespace; both maps honor `MAX_SCENARIO_RUNS` (32).
- [ ] Fail-fast, not rollback: first failure sets `Failed` + `failure` and
  stops; concurrent manual publish surfaces `conflict`, never silent
  overwrite. Cleanup conflicts (`Conflict`/`Missing`) never overwrite
  external state and never change the terminal run outcome.
- [ ] Bounds on all retained state: `MAX_SCENARIO_RUNS` (32),
  `MAX_EVIDENCE_FAULTS` (128) + truncated flags, `MetricTables` caps +
  overflow buckets (incl. stream-loss overflow), `AdmissionLimits.history`
  honored, `MAX_TARGET_LABEL_BYTES` (256) / `MAX_RESOURCE_NAME_BYTES`
  (128) / `MAX_EXPERIMENT_IDENTITY_BYTES` (128) honored.
- [ ] No payloads in evidence/metrics/history: identities, counters,
  generations, namespaces, coarse outcomes only. Label keys stay within the
  pinned set (plus datagram `kind`); no connection/peer/run/fault-ID/
  hostname/fingerprint/key/phase labels. Legacy 7-slot activation arrays
  never gain a `stream-loss` member.
- [ ] Metrics reconcile: global counters agree with closed-connection
  evidence; per-proxy/activation tables agree; stream-loss samples occur
  exactly once per proxy × direction × family with final-evidence values;
  live gauges (active, policy generation, queue bytes) read from policies
  + live evidence; v2 counters carry no labels.
- [ ] Barrier semantics hold: old-generation preserving bytes drain before
  swap; `pending_*` is observable mid-drain; termination survives the swap
  (first wins).

## 5. Verification

Minimum for closure (see `docs/control-plane.md` and the `AGENTS.md`
verification discipline):

```sh
cargo test -p eggchaos-server scenario
cargo test -p eggchaos-server schedule
cargo test -p eggchaos-experiment --all-features
cargo test -p eggchaos-server metric
cargo test -p eggchaos-server stream_loss
cargo test -p eggchaos-server --lib
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Relevant tests (V1 + metrics in
`crates/eggchaos-server/src/runtime/tests.rs` unless noted):

- Scenario driver: `scenario_remove_builds_on_current_state` (1099),
  `scenario_seed_drives_published_namespaces` (1360),
  `scenario_namespaces_replay_independent_of_scheduling` (1426),
  `scenario_cancel_during_sleep_returns_boundedly` (1486),
  `service_shutdown_cancels_active_scenarios` (1530),
  `scenario_failure_is_observable_not_discarded` (1563),
  `stale_base_publication_conflicts_instead_of_overwriting` (1047),
  `transition_pending_while_buffer_drains` (1214),
  `accepted_and_current_generations_update_on_traffic` (1306).
- V2 compiler/fingerprint (`scenario_v2/tests.rs`): minimal expansion,
  absolute/monotonic offsets, source-order equal offsets, repeat
  expansion, ceiling-exact + ceiling+1 rejection, per-phase action cap,
  stream/datagram plan validation, exact-version gate, fingerprint
  stability/sensitivity, `canonical_encoding_excludes_run_id_and_source_path`,
  JSON↔TOML fingerprint identity, run_id-independent namespaces,
  `phase_name_length_is_bounded`, golden corpus
  (`isolated_fingerprint_variants_pinned_by_golden_corpus`).
- V2 properties (`scenario_v2/property_tests.rs`, proptest, 64 cases):
  `compile_is_pure`, `fingerprint_changes_with_seed`,
  `compiler_emits_expected_count`.
- V2 runtime (`scenario_v2/runtime_tests.rs`): paused-time epoch
  anchoring, same-deadline compiled order, late-apply with lateness,
  run_id-independent namespaces, strict stream/datagram conflicts,
  live-mode absorption, cancel-while-sleeping, failure cleanup,
  leave semantics, per-resource cleanup isolation, shutdown join,
  invalid-schedule-no-run, evidence identity/timing/cleanup, TOML/JSON
  apply equivalence, v2 metrics counters + label exclusion, sparse-gap
  anchoring, unrepresentable-deadline cancel, concurrent strict
  conflict, mid-run deletion, live-traffic publication (stream +
  datagram).
- V2 conformance (`scenario_v2/conformance_tests.rs`):
  `control_and_stream_target_conform_on_stream_schedule`,
  `live_mode_conforms_with_prior_external_publish`,
  `stream_target_rejects_datagram_while_server_applies_it`.
- Metrics/evidence: `metrics_reconcile_with_deterministic_fixture`
  (1640), `metrics_use_only_bounded_label_keys` (1745),
  `stream_loss_prometheus_metrics_count_final_evidence_once` (1799),
  `stream_loss_prometheus_exposition_is_unique_and_well_formed` (1848),
  `history_bound_zero_disables_retention_but_keeps_metrics` (1601),
  `connection_evidence_contains_no_payload_bytes` (2012),
  `closed_history_honors_configured_bound` (853); engine-side
  `evidence_serializes_without_payloads` (`stream.rs:1810`), RNG golden
  vectors (`rng.rs:175-242`), `record_stream_loss` bound test
  (`metrics.rs:151-166`).
- Deterministic Tokio-time policy: prefer `#[tokio::test(start_paused =
  true)]` with `tokio::time::advance` (as the `stream.rs` latency/bandwidth
  tests and the V2 paused-time tests do). Any wall-clock timing assertion
  must carry a justified tolerance window and must not be the sole evidence
  for correctness. Tests that poll live state use bounded
  `tokio::time::timeout` loops (e.g. `wait_scenario`,
  `runtime/tests.rs:2034`; `wait_schedule_v2`,
  `runtime_tests.rs:143-163`), never unbounded waits.
- Record any platform or external-oracle gap as incomplete evidence; do
  not substitute source inspection for execution.

## Datagram scenario and evidence additions (M022)

Scenario v1 has two explicit datagram actions: `set-datagram-plan` and
`remove-datagram-fault` (`scenario.rs:124-172` validation,
`scenario.rs:264-398` driver branch). They address the sibling datagram
proxy registry and validate against the current directional
`DatagramPlan` (including the cross-direction ID-uniqueness rule on
`set-datagram-plan`). At fire time they read the current plan/generation,
apply only that direction, derive the seed namespace from `(scenario
seed, run id, event index)`, and publish with an expected-generation
guard (`Some(expected)`). A concurrent manual datagram publication
therefore fails the run instead of overwriting newer state. Datagram
policy publication uses admission-time snapshots, so queued datagrams
retain their decisions.

`GET /v1/datagram-associations` (`admin.rs:431`) includes live and retained
association summaries; `GET` by ID reads active or retained evidence
(`admin.rs:440`) and `DELETE` is an administrative kill (`admin.rs:454`).
`/metrics` adds `eggchaos_datagram_associations_active` gauges,
`eggchaos_datagram_proxy_drops{kind}` observations (fixed 4-`kind`
vocabulary), `eggchaos_datagram_evidence` (12 fixed `kind`s, current +
retained), `eggchaos_datagram_fault_activations` (fixed 6-type
vocabulary), and `eggchaos_datagram_administrative_discards`
(`control.rs:203-290`). Those summaries are explicitly current-plus-retained
evidence; no association, client, run, hostname, or fault identity enters
metric labels, and no payload bytes are recorded. Global reset clears
datagram plans, cancels associations, and restarts stored datagram
listeners alongside the existing stream reset (`control.rs:1598-1651`).

## Stream-loss observability additions (ADR 007, M036–M041)

`stream-loss` is deterministic userspace TCP byte-chunk dropping in fixed
32 KiB logical grains (`STREAM_LOSS_GRAIN_BYTES`, `plan.rs:158`) —
explicitly not IP/TCP packet loss, and it never reuses ADR 003
datagram-loss semantics. Only the Toxiproxy compatibility presentation
may call it `packet_loss`. It propagates through the full scenario
surface: `FaultPlan` validation (V1 events and V2 compiler alike),
fingerprint encoding (`fingerprint.rs:232-241`), and per-event
namespaces. Per-direction additive evidence
(`*_stream_loss_chunks_evaluated/dropped/bytes_discarded`,
`model.rs:68-86`) merges into snapshots/history and reconciles into the
per-proxy `stream_loss[2][3]` table (`metrics.rs:65-75,113-121`) without
touching the frozen 7-slot activation arrays. Prometheus exposes exactly
one sample per proxy × direction × family with a real newline
(`control.rs:95-149`), pinned by the M041 uniqueness tests
(`tests.rs:1848-2003`).
