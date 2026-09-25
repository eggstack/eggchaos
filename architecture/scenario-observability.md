# Scenarios and observability

Deep dive for the deterministic scenario driver and the evidence-first
observability surface. See [architecture overview](overview.md) for the
workspace map; this document covers `scenario.rs` plus the
evidence/snapshot/metrics types it publishes through.

> Evidence-first: every claim below names the file that owns it. No payload
> bytes are captured anywhere in this surface.

## 1. Scenario model

Owner: `crates/eggchaos-server/src/scenario.rs`
(v1), `crates/eggchaos-experiment/` (v2 semantics, shared driver,
experiment harness), and `crates/eggchaos-server/src/scenario_v2/`
(v2 server wiring: `ControlState` target adapter, run records,
compat re-exports).

### 1.1 Document types

- `Scenario { version, seed, events }` (`scenario.rs:11-19`): deterministic,
  bounded scenario document.
  - `version` must be `1`; anything else is rejected in `validate_scenario`.
  - `seed: u64` is recorded in the run record and mixed into every published
    policy seed namespace (see §1.5).
  - `events: Vec<ScenarioEvent>` is capped at 1024 entries.
- `ScenarioEvent { at_ms, action }` (`scenario.rs:23-28`): one
  monotonic-time event. `at_ms` is milliseconds from scenario start.
- `ScenarioAction` (authority: `eggchaos-experiment/src/action.rs`,
  re-exported from `scenario.rs` for compatibility): the shared
  stream/datagram actions:
  - `SetPlan { proxy, direction, faults }` — replace one directional plan
    as a barrier generation.
  - `RemoveFault { proxy, direction, id }` — remove one fault from a
    directional plan.
  - `SetDatagramPlan { proxy, direction, faults }` /
    `RemoveDatagramFault { proxy, direction, id }` — datagram twins.

The `/v1/scenarios/apply` body uses `ScenarioV1` from `native.rs`, not the
internal Serde layout. Each event has `at_ms` plus a nested `action` object
tagged by kebab-case `type` (`set-plan` or `remove-fault`). `set-plan` faults
use the explicit `FaultKindV1` schema shared with fault CRUD; duration
attributes are integer nanoseconds. Scenario responses use `ScenarioRunV1`
and lowercase status values.

### 1.2 Validation (`validate_scenario`, `scenario.rs:103-128`)

The document validates entirely before a run begins; the run task never
starts for an invalid document:

1. `version == 1`, else `InvalidProxy("unsupported scenario version")`.
2. `events.len() <= 1024`, else `InvalidProxy("too many scenario events")`.
3. `at_ms` non-decreasing in document order (`event.at_ms < previous` fails
   with `"scenario events must be ordered"`). Equal timestamps are allowed;
   the wait between them is zero.
4. Per-action `validate_action` (`scenario.rs:130-171`):
   - `SetPlan`: `FaultPlan::new(faults)` must succeed (native plan
     invariants), proxy must exist via `snapshot_policies`.
   - `RemoveFault`: `FaultId::new(id)` must parse, proxy must exist, and the
     named fault must already be present on the target direction's base plan,
     else `"scenario fault {id} not present on {proxy}"`.

### 1.3 Driver (`drive_scenario_run`, `scenario.rs:178-311`)

`ControlState::start_scenario` (`runtime.rs:1941-2009`) validates, assigns
`run_id` from `next_run_id` (starting at 1), inserts a `Pending` record,
creates a `shutdown_token.child_token()`, and spawns `drive_scenario_run`
on the supervised `scenario_tasks` JoinSet — never detached. The driver:

1. Marks the run `Running`.
2. For each `(index, event)` in order, waits
   `event.at_ms.saturating_sub(previous)` via `tokio::time::sleep`, raced
   against `token.cancelled()` in a `biased` `select!`. Cancellation wins
   immediately, marks `Cancelled`, drops the token, and returns
   (`scenario.rs:192-204`).
3. Snapshots the **currently live** plans at fire time
   (`snapshot_policies`, `scenario.rs:214`), clones them, and applies the
   action to the in-memory copy — never a stale snapshot. `RemoveFault`
   re-checks presence at fire time; a fault removed manually since
   validation fails the run here.
4. Publishes **only the target direction** with an expected-generation guard
   (`publish_direction_expected`, `scenario.rs:262-276`; authority at
   `runtime.rs:1888-1919`). The other direction keeps its plan, generation,
   and namespace untouched.
5. On success appends a `ScenarioEventResult` to the trail and increments
   `applied`. On any failure calls `fail_run` and returns — fail-fast, see
   §1.6.
6. After all events, marks `Completed` and drops the token.

`previous` tracks `at_ms`, so waits are deltas, not absolute sleeps.

### 1.4 Barrier generations and conflict semantics

Each event is a barrier generation in the `LivePolicy` sense:

- `ChaosStream::update_live` (`stream.rs:497-536`) drains already-accepted
  preserving bytes before swapping the engine; already-discarded blackhole
  bytes stay discarded. The `pending_generation` marker is visible to
  evidence readers while draining. Termination handles are shared across the
  swap (first request wins).
- Publish paths are compare-and-swap on the expected generation.
  `publish_direction_expected` maps `PublishError::Conflict` to
  `ControlError::Conflict` with `conflict_message`
  (`runtime.rs:2660-2665`): `"proxy {p} policy moved from generation {e}
  to {f} during the operation; retry from current state"`.
- Consequence: a concurrent manual publication between scenario events does
  **not** get silently overwritten. The scenario event's expected base is
  stale, the publish returns `Conflict`, and the run fails fast with
  `"scenario event failed: conflict: ..."` instead of rolling state back.
  This is the documented contract in `docs/control-plane.md` (§Scenarios)
  and the driver header (`scenario.rs:173-177`).
- `GET` proxy views never mix generations: plan, generation, and seed
  namespace come from one atomic `LivePolicy::snapshot()` per direction
  (`runtime.rs:908-931`).

### 1.5 Derived seed namespaces (`derive_policy_seed`)

Owner: `crates/eggchaos-core/src/rng.rs:54-63` (v1) and `rng.rs` v2
namespace helper `derive_schedule_policy_seed` (v2).

Each v1 event derives its namespace purely as
`derive_policy_seed(scenario.seed, run_id, index)` (`scenario.rs:261`).
Manual control updates retain the current namespace; only scenario events
(and explicit publish callers) set a new one. The published namespace feeds
fault-local RNG compilation for every connection that observes the policy,
so the scenario seed participates in deterministic engine decisions.

Covered by `scenario_seed_drives_published_namespaces` and
`scenario_namespaces_replay_independent_of_scheduling`
(`runtime.rs:4285-4408`): different seeds publish different namespaces for
the same event identity; same seed + identity under different `at_ms`
timing publishes the same namespaces.

Scenario v2 namespaces are derived as
`derive_schedule_policy_seed(scenario.seed, execution_key,
schedule_fingerprint, compiled_event_index)`. Daemon `run_id` is **not**
an input. The v2 helper lives in
`crates/eggchaos-core/src/rng.rs` next to the v1 helper so any audit
can compare the contracts side-by-side. The schedule fingerprint itself
is the SHA-256 digest produced by `compiled_fingerprint` in
`crates/eggchaos-server/src/scenario_v2/fingerprint.rs`; see
§1.8 below for the canonical encoding.

### 1.6 Run lifecycle, fail-fast, cancellation

### 1.6 Run lifecycle, fail-fast, cancellation

`ScenarioRunStatus` (`scenario.rs:49-62`):

```text
Pending -> Running -> Completed
               |  \-> Failed (fail-fast, failure: Some(_))
               \---> Cancelling -> Cancelled
```

- `Pending`: validated, waiting for the first event (initial record in
  `start_scenario`).
- `Running`: set by the driver on entry.
- `Cancelling`: set by `cancel_scenario` (`runtime.rs:2024-2040`) when a
  token exists and the record is `Pending`/`Running`. Cancelling a finished
  run returns its final record unchanged; unknown IDs return `None`.
- `Cancelled`: set by the driver when the token fires before the next
  event. `remove_scenario_token` cleans up.
- `Completed`: all events applied.
- `Failed`: stopped early by the first failed event. `fail_run`
  (`scenario.rs:313-321`) records `failure: Some(message)` and drops the
  token. Failure sources: proxy deleted since validation
  (`"scenario proxy {p} not found"`), plan invalid at fire time, fault
  absent at fire time, publish conflict/invalid.

Fail-fast means the trail stops at the failure: `applied` counts only
successful events, no rollback of already-published generations, no skipped
events. Observable via `scenario_failure_is_observable_not_discarded`
(`runtime.rs:4488-4523`): delete the proxy mid-run, the run reports
`Failed` with the proxy name in `failure`.

Cancellation bounds:

- `cancel_scenario` cancels the per-run child token; the driver's biased
  select returns without sleeping out the remaining `at_ms`.
  `scenario_cancel_during_sleep_returns_boundedly` uses `at_ms: 30_000`
  and asserts termination well under 10 s (`runtime.rs:4411-4452`).
- Service shutdown cascades: run tokens are children of `shutdown_token`,
  and `shutdown_and_join` joins `scenario_tasks`, so no run outlives the
  service. `service_shutdown_cancels_active_scenarios`
  (`runtime.rs:4455-4485`) asserts a sleeping run ends `Cancelled`.
- At most 32 run records are retained (`MAX_SCENARIO_RUNS`,
  `runtime.rs:1881`). `start_scenario` fails fast with
  `Conflict("too many active scenario runs")` at the active cap, and
  otherwise prunes oldest *finished* runs first; active runs are never
  evicted to make room.

### 1.7 Run records (no payloads, bounded)

`ScenarioEventResult` (`scenario.rs:66-83`): per-event evidence —
`index`, `at_ms`, `action` (`"set-plan"` / `"remove-fault"` summary string),
`proxy`, `direction`, `global_generation`, `upstream_generation`,
`downstream_generation`. No fault bodies, no byte payloads.

`ScenarioRunRecord` (`scenario.rs:87-100`): `run_id`, `seed`, `status`,
`applied`, `failure: Option<String>` (short detail only), `trail:
Vec<ScenarioEventResult>`. The trail is bounded by the 1024-event document
cap; runs are bounded by the 32-record retention cap. Serialization never
contains payload bytes (`connection_evidence_contains_no_payload_bytes`,
`runtime.rs:4714-4734`, asserts the same for connection/history JSON).

### 1.8 Scenario v2 source language and compiler (M026; extracted to `eggchaos-experiment` in M030)

Owner: `crates/eggchaos-experiment/src/{source,compiler,fingerprint,run,action}.rs`.
The server keeps source-compatible re-exports under
`crates/eggchaos-server/src/scenario_v2/mod.rs`. Compiler,
fingerprint bytes, namespace vectors, and golden corpus are unchanged
by the extraction (proven by `schedule_corpus` and `scenario_v2`
suites running against the re-exports).

`ScenarioScheduleV2` is the bounded piecewise-constant source language
ADR 004 introduced. A schedule is a `version: 2` document with explicit
`seed`, `execution_key`, `isolation` (`strict` or `live`), `cleanup`
(`restore-initial` or `leave`), an ordered `phases` list, and an
optional one-level `repeat` block. Each phase carries an optional
bounded name (presentation-only, not in the fingerprint), an integer
nanosecond `duration_ns`, and a non-empty list of actions. The four
v1 scenario actions remain the only actions in scope; v2 adds the
source-level naming, durations, repetition, isolation, and cleanup
without introducing new fault kinds.

The compiler
(`compile_schedule`, `scenario_v2/compiler.rs`) is a pure function of
`source × COMPILER_SEMANTICS_VERSION`. It rejects:

- unsupported `version` values;
- empty schedules, empty phases, oversize phase names;
- more than `MAX_PHASES = 256` phases, more than `MAX_REPEAT_COUNT = 64`
  iterations, more than `MAX_PHASE_ACTIONS = 64` actions per phase;
- duration arithmetic that would overflow `u64`;
- `FaultPlan` / `DatagramPlan` validation failures inside actions.

The compiled output is `CompiledScenarioV2`: an immutable
`events: Vec<CompiledEventV2>` where every entry carries its stable
`compiled_index`, a `CompiledPhaseIdentity`
(`Top { index }` or `Repeat { iteration, index }`), an absolute
`offset_ns` from the run epoch, and the action. Equal-offset events
preserve source order, so the compiled tape is unique for a given
semantic source. The compile ceiling (`MAX_COMPILED_EVENTS = 1024`)
matches the v1 cap; expansion is bounded before a run task is
created, and there is no partial compile followed by truncation.

#### 1.8.1 Canonical SHA-256 fingerprint

`compiled_fingerprint` (`scenario_v2/fingerprint.rs`) produces a
32-byte SHA-256 digest. The SHA-256 input is the domain/version
prefix `eggchaos/scenario-v2/fingerprint/v1/compiler-semantics=`
followed by the 4-byte big-endian `COMPILER_SEMANTICS_VERSION` and
then the explicit semantic encoding
`encode_compiled_for_fingerprint`. The encoding writes one line per
axis (`isolation=`, `cleanup=`, `seed=`, `execution_key=`,
`event_count=`) followed by one `\nevent[i] phase=... offset_ns=...
action=...` line per compiled event. The action block lists
`(proxy, direction, fault id, probability, kind)` pairs in source
order. Optional phase names do not participate; nor do daemon
`run_id`, source file path, or timestamps. The encoding is hand
written instead of using `serde_json::Value` so that hash-map
iteration order, serde formatter whitespace drift, and Serde
defaults cannot leak into the digest.

Two inputs produce the same fingerprint iff they compile to identical
event tapes. The fingerprint is diagnostic/replay identity, not
authentication; it lives only in run evidence and the v2 namespace
derivation.

#### 1.8.2 V2 namespace derivation

`derive_schedule_policy_seed` (`crates/eggchaos-core/src/rng.rs`)
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
`run_id`, task scheduling, wall-clock time, or hash-map order. The
golden vector in
`rng.rs::tests::schedule_policy_seed_derivation_is_stable_and_sensitive`
pins the byte-fold. A new version requires a registered version bump
in the v2 domain prefix and a regenerated golden vector set; the v1
`derive_policy_seed` vectors must remain byte-identical.

#### 1.8.3 Source parsing and round trips

The wire DTOs in `crates/eggchaos-server/src/native_v2.rs` preserve a
clean conversion into the internal `ScenarioScheduleV2`. JSON is
parsed via `ScenarioScheduleV2Dto::from_json_str`; TOML is parsed via
`ScenarioScheduleV2Toml::from_toml_str`. Both paths reject unknown
fields. The wire action vocabulary (`set-plan`, `remove-fault`,
`set-datagram-plan`, `remove-datagram-fault`) is identical to the v1
kebab-case tags so the runtime can keep its existing authority. TOML
durations are integer nanoseconds; human duration strings are a
separate plan/ADR and deliberately not part of M026 because silent
rounding at the parser edge is a stop condition.

#### 1.8.4 What M026 does not introduce

M026 implements only the compiler, fingerprint, and namespace helpers.
It deliberately ships:

- no new HTTP route and no native `apply` for v2 schedules;
- no CLI subcommand for v2 schedules;
- no run supervisor task for v2 schedules;
- no change to stream or datagram engine semantics.

M027 (below) wires the compiled tape into the owned scenario
supervisor via `ControlState` and the existing publication paths.
M028 qualifies the combined surface on an exact candidate.

### 1.9 Scenario v2 runtime, isolation, and lifecycle (M027; driver shared via `eggchaos-experiment` in M030)

Owner: `crates/eggchaos-experiment/src/driver.rs` (shared driver),
`crates/eggchaos-server/src/scenario_v2/runtime.rs` (`ControlState`
target adapter + run-record sink), plus
`ControlState::start_schedule_v2` and friends
(`runtime/control.rs`), routes (`admin.rs`), and CLI (`main.rs`).
Server behavior is unchanged: the same driver executes through the
same `ControlState` publication methods, and the run record remains
the single source of truth (proven by unchanged `runtime_tests` and
the new `conformance_tests`).

`ControlState::start_schedule_v2` compiles the source entirely
upfront — a compile failure creates no run and consumes no run ID —
then registers a `ScenarioScheduleRunRecord` (run ID from the shared
`next_run_id`, so v1/v2 IDs share one namespace), stores a child of
the service shutdown token, and spawns `drive_schedule_v2_run` on
the **shared** `scenario_tasks` JoinSet. There is no second
supervisor registry; shutdown joins v2 tasks with v1 tasks, and the
32-record `MAX_SCENARIO_RUNS` bound applies to the v2 map as it does
to v1. `cancel_schedule_v2` mirrors `cancel_scenario`, and
`GET`/`DELETE /v1/scenarios/{run_id}` serve both versions.

#### 1.9.1 Epoch-anchored driver

At task entry the driver captures `epoch =
tokio::time::Instant::now()` once, then for each compiled event waits
for `deadline = epoch + event.offset_ns` with `sleep_until` raced
against cancellation (`biased` select so cancellation wins). An
already-due deadline skips the sleep and applies immediately in
compiled-index order, so slow event application never shifts later
deadlines. Actual elapsed time is measured from the same epoch and
`late_by_ns = max(applied_elapsed - scheduled_offset, 0)` is
recorded per event. `SystemTime` never participates in schedule
authority.

#### 1.9.2 Strict/live generation ownership

Before the first event the driver snapshots every touched
directional policy (plan + generation for stream, plan/generation
for datagram); a missing proxy fails the run before anything
publishes. Strict mode (default) publishes each event against the
generation last owned by the run — an external manual/scenario move
makes the next event's expected-generation guard fail, so the run
fails fast instead of incorporating or overwriting external state.
Live mode re-reads the live generation at fire time, so a completed
manual update may become the base, while a concurrent move during
publication still conflicts. Both modes publish through the existing
`publish_direction_expected` / `publish_datagram_plan` authority;
the v2 namespace for each event is
`derive_schedule_policy_seed(seed, execution_key, fingerprint,
compiled_index)` and never involves `run_id` or lateness.

#### 1.9.3 CAS-safe cleanup

After the terminal outcome (Completed, Failed, or Cancelled) the
driver runs cleanup once. `leave` records `NotRequested` per
resource and publishes nothing. `restore-initial` republishes each
touched resource's snapshotted initial plan only when the live
generation still equals the generation last owned by the run;
otherwise it records `Conflict` and leaves external state intact.
Cleanup of one conflicted resource never blocks bounded cleanup of
the others, and the original run outcome is retained alongside the
cleanup outcome. Cleanup performs in-process publications only — it
never waits on data-plane drain.

#### 1.9.4 Run evidence and operator surface

`ScenarioScheduleRunRecord` (`run.rs`) carries `run_id`, `seed`,
`execution_key`, 32-byte `schedule_fingerprint`, compiler version,
isolation/cleanup policy, status, applied count, bounded failure
detail, the per-event trail (`compiled_index`, phase identity,
scheduled/applied/late nanoseconds, action summary, proxy/direction/
transport, resulting generations), and the cleanup outcome. No
payload bytes. The native responses (`ScheduleRunV2`,
`ScheduleValidateV2`, `ScheduleCompileV2` in `native_v2.rs`) render
the fingerprint as lowercase hex and the phase as `top/{i}` or
`repeat/{iter}/{i}`. Metrics add three coarse counters
(`eggchaos_schedule_v2_runs_total`,
`eggchaos_schedule_v2_events_total`,
`eggchaos_schedule_v2_late_events_total`) with no run, fingerprint,
phase, or key labels.

#### 1.9.5 Replay limits for v2

Policy/event identity is exact across daemon restarts and run
ordering: the same schedule document replays identical seed
namespaces and therefore identical fault decisions for the same
connection keys. Live connection timing (accept order → connection
keys), datagram arrival timing, wall-clock event-application cost,
and scheduler lateness are not replayed; lateness is diagnostic
evidence recorded per event, never an RNG input. Strict-mode
conflict/cleanup-conflict evidence distinguishes "the world moved"
from "the schedule is wrong".

### 1.10 Consumer-neutral experiment harness and coordinated start (M030)

Owner: `crates/eggchaos-experiment/` (`target.rs`, `driver.rs`,
`gate.rs`, `experiment.rs`, `stream_target.rs`); server adapter in
`crates/eggchaos-server/src/scenario_v2/runtime.rs`; conformance in
`scenario_v2/conformance_tests.rs`.

`PolicyTarget` is the one narrow publication contract the shared
driver requires: `capabilities()`, `snapshot(resource)`,
`current_generation(resource)`, expected-generation
`publish(resource, expected, plan, seed_namespace)`, and
`global_generation()`. `TargetError` has bounded stable categories
(`MissingResource`, `UnsupportedCapability`,
`GenerationConflict { expected, found }`, `Validation`, `Internal`);
no payload or consumer product model enters the contract, and no
lock is held across schedule sleeps.

`PreparedExperiment::prepare` compiles, validates target
capabilities, resolves touched resources, and snapshots initial
state — publishing nothing and starting no clock. `EpochGate`
captures one Tokio monotonic `Instant` exactly once and releases it
to both the schedule driver and the caller workload
(`run`/`run_from_epoch`/`run_with_workload`); every deadline stays
`epoch + compiled_offset`. Cancellation before start publishes
nothing; cancellation while sleeping wakes promptly; cancellation
after publications runs the selected cleanup. The driver future is
caller-owned; dropping a prepared experiment publishes nothing and
spawns nothing. Evidence (`ExperimentEvidence`) carries compiler
version, fingerprint hex, seed, execution key, optional caller
integration identity (≤128 bytes), applied count, outcome, and
cleanup — portable reports use relative offsets only, never the raw
epoch.

`StreamPolicyTarget` maps one logical resource name to an M029
`LivePolicy` upstream/downstream pair (register by name, stream-only:
datagram actions are typed `UnsupportedCapability`, never silently
translated). The server adapts `ControlState` without a second state
store; `conformance_tests` proves the two targets produce equivalent
event/generation outcomes for the supported stream subset. Downstream
EggReplay/EggProbe adoption remains downstream work after M031.

## 2. Observability

### 2.1 Stream / engine / RNG evidence

Owners: `crates/eggchaos-core/src/stream.rs`, `engine.rs`, `rng.rs`.

- `StreamEvidence` (`stream.rs:65-168`): lock-shared live evidence per
  direction, read by the runtime without locking the stream. Atomics for
  `observed_generation`, `pending_generation` (0 = none),
  `seed_namespace`, byte counts, high-water mark, `transitions`,
  per-fault-type `activations[7]`; a `Mutex<(Vec<ActiveFault>, bool)>` for
  fault identities. `refresh_policy` stores the published snapshot's
  generation/namespace/identities; `mirror_engine` copies `DirectionEngine`
  counters after each drive; `note_direct` counts no-fault direct-path
  bytes immediately so evidence never lags a completed write.
- `EngineEvidence` (`engine.rs:128-163`): direction-local counters —
  `bytes_accepted/forwarded/discarded`, `segments`, `slices`,
  `buffered_bytes`, `high_water_bytes`, `injected_delay_ms`,
  `throttled_delay_ms`, `termination`, `activations[7]`, `rng_version`.
  Activation semantics are documented per stage (preserving stages count
  per engaged `accept`; blackhole per discarding call; termination stages
  once on first publish; slow-close once on enforced positive delay).
- `RngEvidence { version, seed }` (`rng.rs:9-14`): replay metadata for a
  connection-local deterministic RNG. `RngVersion` is surfaced in
  `DirectionSummary` and `ConnectionSnapshot.rng_version`.
- `ActiveFault { id, fault_type }` (`stream.rs:54-59`): connection-active
  fault identity for evidence only. `fault_type` uses the low-cardinality
  `FAULT_TYPE_NAMES` spelling. Retained list is capped at
  `MAX_EVIDENCE_FAULTS = 128` (`stream.rs:50`) with a `truncated` flag —
  the `(Vec<ActiveFault>, bool)` shape.
- `DirectionSummary` (`stream.rs:21-47`): serializable per-direction rollup
  — byte counters (including transparent direct-path bytes), segment/slice
  counts, buffer/high-water, delay totals, `activations[7]`, `termination`,
  `rng_version`. `evidence_serializes_without_payloads`
  (`stream.rs:1459+`) pins the no-payload contract.

### 2.2 Connection snapshots and history

Owner: `crates/eggchaos-server/src/runtime.rs:163-317`.

- `ConnectionSnapshot` (`runtime.rs:181-238`): safe operational summary.
  Accept-time identity (`id`, `proxy`, `ordinal`, `peer`, `upstream`,
  `connection_key`, `seed = service_seed ^ proxy_seed`, `generation`,
  `accepted_*_generation/seed`) is frozen at registration
  (`accept_connection`, `runtime.rs:2349-2444`); `observed_*` /
  `pending_*` / counters / fault lists report live stream state merged at
  read time via `merge_evidence` (`runtime.rs:2670-2720`), or final state in
  history records. `DirectionBytes { accepted, forwarded, discarded }`
  per direction; `*_faults_truncated` mirrors the 128-entry evidence bound.
- `ConnectionEvidence { upstream, downstream: Arc<StreamEvidence> }`
  (`runtime.rs:253-258`): registered before relaying starts
  (`runtime.rs:2824-2830`) so snapshots report from the first read.
  `ControlState::connections()` / `get_connection()`
  (`runtime.rs:1146-1159`) merge live evidence at read time; connections
  without registered evidence report accept-time values.
- `ConnectionOutcome` (`runtime.rs:275-302`): final classification —
  `RelayCompleted`, `ConnectFailed(String)`, `KilledByOperator`,
  `ServiceShutdown`, `ProxyRemoved`, `GracefulTermination { drained }`,
  `HardReset { client, upstream: ResetResult }`, `RelayError(String)`.
  Coarse class order is pinned by `OUTCOME_CLASS_NAMES`
  (`runtime.rs:661-670`) for the `outcomes[8]` metrics array.
  `ResetResult` (`runtime.rs:262-271`): `Applied` / `Failed(String)` /
  `Unsupported(String)` — wire-level RST observation stays
  platform-dependent; the recorded value is the socket-API outcome.
- `ClosedConnection { snapshot (state == Closed), outcome,
  upstream_termination, downstream_termination, detail }`
  (`runtime.rs:306-317`): bounded retained final record. `detail` is a
  short machine-safe string (relay report, failure, truncation note).
  Retention is bounded by `AdmissionLimits.history` (default 256,
  `runtime.rs:148-161`); `history == 0` disables retention but keeps
  metrics (`history_bound_zero_disables_retention_but_keeps_metrics`,
  `runtime.rs:4526-4562`). Eviction is oldest-first (`record_close`,
  `runtime.rs:2464-2507`); `take_connection` guarantees exactly-once
  accounting across concurrent finish/purge paths.

### 2.3 Per-proxy metrics and Prometheus surface

Owners: `runtime.rs:627-760` (counters/tables), `runtime.rs:1007-1114`
(`metrics_text`), `docs/control-plane.md` (§Metrics).

- `MetricsCounters` (`runtime.rs:629-658`): `accepted / completed /
  rejected` totals, `outcomes[8]`, `graceful_requests /
  hard_reset_requests`, `reset_applied / reset_unsupported / reset_failed`,
  `bytes_accepted / forwarded / discarded`, `transitions`,
  `schedule_v2_runs / schedule_v2_events / schedule_v2_late_events`,
  plus `tables: StdMutex<MetricTables>`. Totals derive from final stream
  evidence in `aggregate_close_metrics` (`runtime.rs:2512-2596`), so
  counters always agree with the history records they summarize. The
  three v2 schedule counters are coarse run/event/late totals with no
  labels at all.
- `MetricTables` (`runtime.rs:692-746`): bounded low-cardinality tables
  with overflow buckets. `MAX_METRIC_PROXIES = 1024`,
  `MAX_METRIC_ACTIVATIONS = 8192` (`runtime.rs:673-675`). Per-proxy
  `PerProxyMetrics { accepted, completed, bytes[2][3] }`; activations keyed
  by `(proxy, direction, fault_type)`. Overflow proxies fold into a
  `_overflow` series; overflow activations fold into
  `{proxy="_overflow",direction="_overflow",fault_type="_overflow"}`.
- `metrics_text()` renders Prometheus text exposition:
  - `eggchaos_config_generation` (gauge), `eggchaos_connections_accepted /
    completed / rejected_total`, `eggchaos_connections_active` (live gauge).
  - `eggchaos_connection_outcomes_total{outcome}` (8 coarse classes),
    `eggchaos_termination_requests_total{request="graceful|hard_reset"}`,
    `eggchaos_reset_results_total{result="applied|unsupported|failed"}`,
    `eggchaos_bytes_total{flow}`, `eggchaos_policy_transitions_total`.
  - Per-proxy `eggchaos_proxy_connections_accepted/completed_total{proxy}`,
    `eggchaos_proxy_bytes_total{proxy,direction,flow}`,
    `eggchaos_fault_activations_total{proxy,direction,fault_type}`.
  - Live gauges from policies + live evidence:
    `eggchaos_proxy_connections_active{proxy}`,
    `eggchaos_proxy_policy_generation{proxy,direction}`,
    `eggchaos_proxy_queue_bytes{proxy,direction}` (accepted minus
    forwarded+discarded, summed over live evidence).
- Label discipline: the only label keys are `proxy`, `direction`, `flow`,
  `outcome`, `request`, `result`, `fault_type` — pinned by
  `metrics_use_only_bounded_label_keys` (`runtime.rs:4669-4711`). Labels
  never carry connection IDs, peer addresses, scenario run IDs, arbitrary
  fault IDs, or hostnames. Directions and fault types are fixed
  vocabularies; proxy names come from the capped table.
- Reconciliation is exact in the deterministic fixture
  (`metrics_reconcile_with_deterministic_fixture`,
  `runtime.rs:4565-4666`): 3 graceful echoes + 1 operator kill assert
  accepted/completed/outcome/termination/byte/activation series verbatim.

### 2.4 Admin inspection routes

Owner: `crates/eggchaos-server/src/admin.rs` + `native.rs`; contract summary in
`docs/control-plane.md` (§Route inventory, §Scenarios).

| Method + path | Handler | Payload |
| --- | --- | --- |
| `GET /v1/connections` | `state.connections().await` | `Vec<ConnectionSnapshot>` with live evidence merged |
| `GET /v1/connections/{id}` | `state.get_connection(id)` | one snapshot, or `not_found`; non-integer ID is `invalid` |
| `DELETE /v1/connections/{id}` | `state.kill(id)` | `{id, terminated: true}` or `not_found` |
| `GET /v1/history` | `state.history().await` | bounded `Vec<ClosedConnection>` |
| `POST /v1/scenarios/apply` | parse `ScenarioV1`, then `state.start_scenario` | `202 ScenarioRunV1`; invalid body is a bounded JSON error |
| `GET /v1/scenarios/{run_id}` | `state.get_scenario(run_id)` | full `ScenarioRunV1` (lowercase status, applied, failure, trail) or `not_found` |
| `DELETE /v1/scenarios/{run_id}` | `state.cancel_scenario(run_id)` | latest `ScenarioRunV1` (active status moves to `cancelling`) |
| `GET /metrics` | `state.metrics_text().await` | Prometheus text, no `/v1` prefix, `text/plain; version=0.0.4` |
| `GET /v1/health`, `GET /v1/version`, `POST /v1/reset` | service routes | liveness/generation, build version, reset report |

Request bodies are capped at 1 MiB (`admin.rs:92-105`); the EggServe H1
runtime owns parsing/body bounds/connection lifecycle while eggchaos owns
only route dispatch and typed JSON conversion (`docs/control-plane.md:9-12`).

## 3. Replay limits

From `docs/control-plane.md` (§Scenarios) and the `rng.rs` / `scenario.rs`
headers — what is and is not reproducible:

- **Reproducible: policy state and per-key decisions.** The same document
  (`version`, `seed`, ordered `at_ms` + actions) reproduces the same seed
  namespaces, therefore the same fault decisions for the same connection
  keys. Derivation is a pure function of `(scenario seed, run id, event
  index)` (`rng.rs:46-63`); it never depends on task scheduling, wall time,
  or connection order. Golden vectors pin the derivation
  (`rng.rs:118-124`: `derive_policy_seed(7,1,0)`,
  `derive_policy_seed(7,1,1)`, sensitivity to each component).
- **Not reproducible: live connection timing.** Connection keys are
  proxy-local accept ordinals (`ordinal`, `runtime.rs:2375`), so they
  depend on accept order. Scheduling interleavings, wall-clock sleeps
  between events, dial latency, relay pump timing, and which connections
  are alive when an event fires are not part of the seed. Re-running a
  document replays namespaces exactly but not live timing — replay is exact
  for policy state, not for connection-timing behavior.
- **Manual mutations move the base.** Events apply against live plans at
  fire time (`scenario_remove_builds_on_current_state`,
  `runtime.rs:4024-4081`: removal builds on current B-state). A manual
  change between events changes what the next event applies to; a
  conflicting concurrent publication fails the run (conflict, §1.4) instead
  of rolling state back.

## 4. Review checklist

For any change touching `scenario.rs`, evidence types, `merge_evidence`,
`aggregate_close_metrics`, `metrics_text`, or the admin inspection routes:

- [ ] Scenario document bounds preserved: `version == 1`, `<= 1024` events,
  monotonic `at_ms`, per-action validation (proxy exists, plan valid,
  removed fault present). No new action without a version bump plan.
- [ ] Driver still applies to **live** plans with a single-direction
  expected-generation publish; the untouched direction's plan/generation/
  namespace is unchanged in the trail assertion.
- [ ] Seed participation intact: namespace is exactly
  `derive_policy_seed(seed, run_id, index)`; manual publishes retain the
  namespace; golden vectors in `rng.rs` still pass.
- [ ] Lifecycle complete: `Pending → Running → Completed/Failed`, `Cancelling
  → Cancelled`, token removed on every terminal path, shutdown joins runs.
  No detached scenario task.
- [ ] Fail-fast, not rollback: first failure sets `Failed` + `failure` and
  stops; concurrent manual publish surfaces `conflict`, never silent
  overwrite.
- [ ] Bounds on all retained state: `MAX_SCENARIO_RUNS (32)`,
  `MAX_EVIDENCE_FAULTS (128)` + truncated flags, `MetricTables` caps +
  overflow buckets, `AdmissionLimits.history` honored.
- [ ] No payloads in evidence/metrics/history: identities, counters,
  generations, namespaces, coarse outcomes only. Label keys stay within the
  pinned set; no connection/peer/run/fault-ID/hostname labels.
- [ ] Metrics reconcile: global counters agree with closed-connection
  evidence; per-proxy/activation tables agree; live gauges (active,
  policy generation, queue bytes) read from policies + live evidence.
- [ ] Barrier semantics hold: old-generation preserving bytes drain before
  swap; `pending_*` is observable mid-drain; termination survives the swap
  (first wins).

## 5. Verification

Minimum for closure (see `docs/control-plane.md` and the `AGENTS.md`
verification discipline):

```sh
cargo test -p eggchaos-server scenario
cargo test -p eggchaos-server metric
cargo test -p eggchaos-server --lib
cargo fmt --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Relevant tests (all in `crates/eggchaos-server/src/runtime.rs` unless
noted):

- Scenario driver: `scenario_remove_builds_on_current_state`,
  `scenario_seed_drives_published_namespaces`,
  `scenario_namespaces_replay_independent_of_scheduling`,
  `scenario_cancel_during_sleep_returns_boundedly`,
  `service_shutdown_cancels_active_scenarios`,
  `scenario_failure_is_observable_not_discarded`,
  `stale_base_publication_conflicts_instead_of_overwriting`,
  `transition_pending_while_buffer_drains`,
  `accepted_and_current_generations_update_on_traffic`.
- Metrics/evidence: `metrics_reconcile_with_deterministic_fixture`,
  `metrics_use_only_bounded_label_keys`,
  `history_bound_zero_disables_retention_but_keeps_metrics`,
  `connection_evidence_contains_no_payload_bytes`,
  `closed_history_honors_configured_bound`; engine-side
  `evidence_serializes_without_payloads` (`stream.rs`), RNG golden vectors
  (`rng.rs:106-124`).
- Deterministic Tokio-time policy: prefer `#[tokio::test(start_paused =
  true)]` with `tokio::time::advance` (as the `stream.rs` latency/bandwidth
  tests do). Any wall-clock timing assertion must carry a justified
  tolerance window and must not be the sole evidence for correctness.
  Tests that poll live state use bounded `tokio::time::timeout` loops
  (e.g. `wait_scenario`, `runtime.rs:4736-4756`), never unbounded waits.
- Record any platform or external-oracle gap as incomplete evidence; do
  not substitute source inspection for execution.

## Datagram scenario and evidence additions (M022)

Scenario v1 has two explicit datagram actions: `set-datagram-plan` and
`remove-datagram-fault`. They address the sibling datagram proxy registry and
validate against the current directional `DatagramPlan`. At fire time they
read the current plan/generation, apply only that direction, derive the seed
namespace from `(scenario seed, run id, event index)`, and publish with an
expected-generation guard. A concurrent manual datagram publication therefore
fails the run instead of overwriting newer state. Datagram policy publication
uses admission-time snapshots, so queued datagrams retain their decisions.

`GET /v1/datagram-associations` includes live and retained association
summaries; `GET` by ID reads active or retained evidence and `DELETE` is an
administrative kill. `/metrics` adds datagram active-association gauges,
per-proxy drop observations, directional datagram evidence/queue/high-water
gauges, and fixed-vocabulary activation series. Those summaries are explicitly
current-plus-retained evidence; no association, client, run, hostname, or fault
identity enters metric labels, and no payload bytes are recorded. Global reset
clears datagram plans, cancels associations, and restarts stored datagram
listeners alongside the existing stream reset.
