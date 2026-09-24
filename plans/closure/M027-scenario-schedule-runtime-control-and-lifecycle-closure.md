# M027 — Scenario Schedule Runtime, Control, and Lifecycle — Closure

Status: closed  
Exact implementation candidate: `5d85d15648e558df589c2d058ec93335bf63dcf0`  
Implementation commit: `5d85d15648e558df589c2d058ec93335bf63dcf0`  
Closure commit: this file's commit; the implementation candidate above is the qualified code head  
Depends on: M026 (closed at `e0507d1`)

## What M027 delivered

M027 wires the M026 compiled `CompiledScenarioV2` event tape into the
existing owned scenario supervisor and native operator surface. One
`ControlState` remains the publication authority; ScenarioV1
compatibility, stream/datagram live-mutation semantics, and bounded
evidence are preserved. No new fault kind, no data-plane clock
branch, no second state store, no cron/persisted scheduler, no
continue-on-error mode.

### Supervisor integration (WP1)

`ControlState::start_schedule_v2` (`runtime/control.rs`) compiles the
source entirely upfront — a compile failure creates no run and
consumes no run ID — then registers a `ScenarioScheduleRunRecord`,
stores a child of the service shutdown token in a v2 token map, and
spawns `drive_schedule_v2_run` on the **shared** `scenario_tasks`
JoinSet. V1 and v2 run IDs share the single `next_run_id` namespace.
The 32-record `MAX_SCENARIO_RUNS` bound applies to the v2 record map
exactly as it does to v1 (oldest finished pruned first, active cap
fails fast). `cancel_schedule_v2` mirrors `cancel_scenario`;
shutdown joins v2 tasks together with v1 tasks, so no schedule task
outlives the service untracked.

### Absolute-deadline driver (WP2)

`scenario_v2/runtime.rs` captures one `tokio::time::Instant` epoch at
task entry and waits for `deadline = epoch + event.offset_ns` with
`sleep_until` raced against cancellation in a `biased` select.
Already-due deadlines skip the sleep and apply immediately in
compiled-index order, so slow event application never shifts later
deadlines. Actual elapsed time is measured from the same epoch and
`late_by_ns = max(applied - scheduled, 0)` is recorded per event.
`SystemTime` never participates in schedule authority.

### Strict/live generation ownership (WP3)

Before the first event the driver snapshots every touched directional
policy (plan + generation for stream via `snapshot_policies`,
plan + generation for datagram via `get_datagram_plan`); a missing
proxy fails the run before anything publishes. Strict mode publishes
each event against the generation last owned by the run — an external
manual/scenario move makes the next event's expected-generation
guard fail, so the run fails fast instead of incorporating or
overwriting external state. Live mode re-reads the live generation
at fire time (a completed manual update may become the base) while a
concurrent move during publication still conflicts. Both modes reuse
`publish_direction_expected` / `publish_datagram_plan`; per-event v2
namespaces derive from `(seed, execution_key, fingerprint,
compiled_index)` with no `run_id` or lateness input.

### Cleanup lifecycle (WP4)

After the terminal outcome (Completed, Failed, or Cancelled) the
driver runs cleanup once. `leave` records `NotRequested` per
resource and publishes nothing. `restore-initial` republishes each
touched resource's snapshotted initial plan only while the live
generation still equals the generation last owned by the run;
otherwise it records `Conflict` and leaves external state intact
(`Missing` when the proxy is gone). Cleanup of one conflicted
resource never blocks bounded cleanup of the others, and the
original run outcome is retained alongside the cleanup outcome.
Cleanup performs in-process publications only — it never waits on
data-plane drain.

### Native DTO/routes (WP5)

`POST /v1/scenarios/apply` is version-aware: it peeks the numeric
`version` field and routes `2` to `ScenarioScheduleV2Dto →
start_schedule_v2 → 202 ScheduleRunV2`, everything else to the
unchanged v1 path. New additive endpoints `POST
/v1/scenarios/validate` (fingerprint/identity, no run) and `POST
/v1/scenarios/compile` (normalized tape + fingerprint, no run)
accept v2 schedules with the same body/auth/bound conventions.
`GET`/`DELETE /v1/scenarios/{run_id}` serve and cancel both
versions. V1 fixtures are byte/behavior compatible.

### CLI/TOML authoring (WP6)

`scenario validate|compile|apply <file>` accept v2 JSON or TOML;
`.toml` files parse through the shared server `ScenarioScheduleV2Toml`
DTO and are forwarded as semantic JSON to the server authority. The
CLI never expands phases or derives fingerprints. V1 `apply`
behavior is unchanged; JSON mode still emits one document and exits
nonzero on failure.

### Observability and metrics (WP7)

Run evidence carries fingerprint (hex), execution key, compiler
version, isolation/cleanup policy, per-event compiled index, phase
(`top/{i}` / `repeat/{iter}/{i}`), scheduled/applied/late
nanoseconds, action summary, proxy/direction/transport, resulting
generations, and the cleanup outcome — no payload bytes. Three
coarse counters (`eggchaos_schedule_v2_runs_total`,
`eggchaos_schedule_v2_events_total`,
`eggchaos_schedule_v2_late_events_total`) carry no run,
fingerprint, phase, key, or peer labels.

### Documentation (WP9)

`docs/control-plane.md` (§Scenarios, §CLI inventory, §Metrics),
`architecture/control-plane-cli.md` (route + CLI tables), and
`architecture/scenario-observability.md` (§1.8.4 resolved, new §1.9
driver/isolation/cleanup/evidence/replay-limits) document the
implemented runtime and state the exact replay boundary:
policy/event identity is exact across daemon restarts and run
ordering; live connection/datagram arrival timing is not replayed.

## Verification on the exact candidate

All commands below ran against
`5d85d15648e558df589c2d058ec93335bf63dcf0`.

### Local gates

- `./scripts/check.sh` — pass: workspace fmt, Clippy with `-D warnings`,
  all workspace tests, and `cargo doc --no-deps` all clean.
- `cargo test --workspace --all-features` — pass: 131 server tests
  (17 new v2 runtime tests, 31 v2 compiler tests, 3 v2 property
  tests, 5 v2 wire DTO tests), 57 core, 4 CLI (including the new v2
  validate/compile/apply/get/cancel e2e over JSON and TOML), plus
  toxiproxy/eggfetch suites. No v1 scenario/runtime regression.
- `cargo test -p eggchaos-cli --all-features` — pass, including
  `cli_scenario_v2_validate_compile_apply_json_and_toml` against a
  live admin listener.
- Paused-time authority: `paused_time_events_fire_at_epoch_offsets_without_drift`
  (1s/2s/3s offsets exact with unrelated control work between
  events), `same_deadline_events_execute_in_compiled_order`,
  `already_late_events_apply_immediately_with_lateness`
  (deterministic 0/400ms lateness). Repeated 3× stable, plus 6× for
  the property suite after a `prop_assume!` fix for generator
  outputs that fail plan validation.
- Concurrency/cleanup matrix (real-time, bounded): strict stream
  conflict fail-fast + cleanup conflict without overwrite, strict
  datagram conflict, live-mode current-state absorption, cancel
  while sleeping with restore, failure-after-events with restore,
  leave keeps state, two-resource conflict isolation (A conflict
  keeps external state while B restores), shutdown cancels/joins,
  invalid schedule creates no run, run-ID-independent namespaces
  pinned against `derive_schedule_policy_seed`, TOML/JSON identity,
  evidence contract (timing math, no `payload` in JSON), metrics
  label discipline.

### Fuzz

- `EGGCHAOS_FUZZ_RUNS=2000 ./scripts/qualify_fuzz.sh` — pass for all
  nine targets, including the extended `native_control_json`
  target (now round-trips `ScenarioScheduleV2Dto` and compiles
  parsed schedules) and `scenario_v2`. No panic. (Full 10k-run
  qualification is M028's gate.)

### Security and dependency gates

- `cargo audit --deny warnings` — pass.
- `cargo deny check advisories licenses bans sources` — pass. No new
  dependency was added in M027.

## Invariants and unresolved findings

- One `ControlState` is the publication authority; no second
  registry, no detached task, no global run-duration lock.
- Deadlines anchor to one epoch; slow application never shifts
  later deadlines (paused-time proven).
- Strict never silently incorporates or overwrites external
  generations; restore never clobbers externally-owned state.
- V2 randomness never depends on `run_id` or scheduler lateness
  (cross-run-ID namespace identity pinned).
- Stream buffered bytes / datagram queued candidates keep their
  existing generation semantics (engines untouched).
- Cancellation cannot leave an untracked task (shared JoinSet +
  shutdown join proven).
- V1 documents keep their schema and lifecycle (existing fixtures
  green, e2e v1 path untouched).
- All retained state and error text remain bounded (32-run maps,
  1024-event tapes, bounded messages).

No unresolved medium-or-higher finding remains for this milestone.
One flaky proptest (`fingerprint_changes_with_seed` assuming a
compilable generator output) was fixed with `prop_assume!` and
verified stable across 6 consecutive runs.

## Acceptance verdict and planning transition

M027 closes cleanly. `plans/registry.md` moves M027 from `ready` to
`closed`. M028 moves from `blocked` to `ready`: the v2 runtime,
control, lifecycle, evidence, and operator surface it must qualify
are implemented on this candidate, with the M026 compiler,
fingerprint, and namespace contracts unchanged underneath it.

`unsafe_code = "forbid"` remains. The tranche adds no new
data-plane fault behavior and no new dependency.

## Limitations carried forward

- Full exact-candidate qualification (golden corpus freeze,
  paused-time timing corpus, full race matrix under qualification
  tooling, pinned Toxiproxy/Eggfetch/security/package/performance
  gates, 10k fuzz runs) is M028's scope on its own candidate.
- The `metrics_use_only_bounded_label_keys` pin covers the existing
  label-key set; the three new counter names are label-free and
  covered by the v2 metrics test, with full reconciliation in M028.
- Wall-clock scheduling precision (timer granularity, publish
  latency under load) is observed as lateness evidence, not asserted
  as exact in real-time tests; paused-time tests own the exactness
  claims.
