# M027 — Scenario Schedule Runtime, Control, and Lifecycle

Status: closed
Depends on: M026
Role: runtime/control integration for deterministic scenario-v2 schedules

## Objective

Wire the M026 compiled ScenarioScheduleV2 model into the existing owned scenario supervisor and native operator surface while preserving one ControlState authority, ScenarioV1 compatibility, stream/datagram live-mutation semantics, and bounded evidence.

M027 is where richer schedules become executable. It must not reimplement fault behavior or create a second state store.

## Baseline and dependencies

M026 must first close with a frozen compiler, schedule fingerprint, event ordering, expansion bounds, and run_id-independent v2 namespace derivation.

The runtime already provides:

- supervised scenario tasks and CancellationToken ownership;
- bounded retained run records;
- expected-generation publication for stream plans;
- expected-generation publication for datagram plans;
- live stream barrier transitions and datagram admission-time snapshots;
- native JSON DTO boundaries and authenticated control plane;
- thin Eggfetch-backed CLI.

Current ScenarioV1 behavior remains supported. The v2 runtime is additive and shares the same authoritative publication methods.

## Scope

### In scope

- Apply CompiledScenarioV2 through the owned scenario supervisor.
- One monotonic epoch plus absolute sleep_until deadlines.
- Correct the v1 driver to use its documented at_ms-from-start meaning if fixture review confirms this is compatibility-safe; otherwise leave v1 execution untouched and use the corrected scheduler only for v2.
- Strict and live isolation modes from ADR 004.
- Per-resource ownership tracking for stream/datagram directional generations.
- restore-initial and leave cleanup policies.
- Cleanup on success, cancellation, and failure with non-clobbering CAS semantics.
- V2 run evidence: fingerprint, execution key, compiler version, scheduled/applied timing, lateness, isolation, cleanup outcome.
- Additive native validate/compile/apply operations without breaking ScenarioV1 fixtures.
- CLI validate/compile/apply support for JSON and TOML schedule files.
- Existing GET/cancel run lifecycle integration and bounded retention.
- Low-cardinality schedule metrics where useful.
- Concurrent manual/scenario mutation tests.

### Non-goals

- No new fault kinds.
- No continuous data-plane clock interpolation.
- No cron/calendar scheduler or persisted jobs across daemon restart.
- No continue-on-error mode.
- No connection-ID/association-ID targeting.
- No arbitrary conditions, branches, shell commands, callbacks, or HTTP hooks.
- No proxy/listener enable/disable actions in the initial v2 action set.
- No Toxiproxy scenario extension.
- No second runtime/state store for schedules.

## Affected surfaces

- crates/eggchaos-server/src/scenario.rs or decomposed scenario module.
- crates/eggchaos-server/src/runtime/control.rs and scenario task/run-record ownership.
- crates/eggchaos-server/src/native.rs and admin routing.
- crates/eggchaos-cli/src/main.rs.
- docs/control-plane.md, docs/configuration.md if TOML schedule examples live there, docs/architecture.md.
- architecture/scenario-observability.md and architecture/control-plane-cli.md.
- fuzz/native control fixtures as appropriate.

## Deadline scheduler semantics

At task entry, capture epoch = tokio::time::Instant::now() before the first v2 event. For every compiled event:

    deadline = epoch + event.offset

Wait with sleep_until(deadline) raced against cancellation. If deadline is already due, do not sleep; apply the event immediately in compiled-index order.

Record actual elapsed application time from the same monotonic epoch and lateness = max(actual_elapsed - scheduled_offset, 0). Do not use SystemTime for schedule authority.

Same-deadline events execute serially in compiled order. Event application latency must not shift later deadlines.

## Strict isolation

Before a strict run begins, resolve and snapshot every directional policy touched by the compiled schedule. Record initial plan, generation, and seed namespace for cleanup, and initialize an owned-generation map.

For each event:

- expected generation is the last generation successfully owned by this run for that resource;
- construct the next plan according to the compiled action;
- publish through existing expected-generation authority with the v2 event namespace;
- on success update the owned generation;
- on conflict/missing resource fail the run immediately.

Do not hold a global lock for the run duration. Ownership is logical generation ownership, not mutex ownership.

## Live isolation

For live mode, preserve the v1 interactive rule: snapshot the current target plan/generation at fire time, build the action from that state, and publish with an expected-generation guard. A manual update that completed before the event snapshot may become the base. A concurrent move during publication conflicts.

Run evidence must state the isolation mode so these semantics are not confused.

## Cleanup

restore-initial captures initial plans for every touched directional policy before event execution. Cleanup runs once after Completed, Failed, or Cancelled outcome is determined.

For each touched resource, restoration is attempted only if the current generation equals the generation last owned by the schedule. Restoration itself publishes through the authoritative control path and advances generation normally.

If the generation moved externally:

- do not overwrite it;
- record cleanup conflict for that resource;
- continue bounded cleanup of other resources;
- retain the original run outcome plus cleanup outcome.

Cleanup is cancellation-shielded only for a bounded in-process publication pass; it must not wait indefinitely on data-plane drain. Existing live-policy transition semantics own any subsequent drain.

leave performs no restore and records not-needed.

## Run record and evidence

Extend retained run evidence additively. V1 response fixtures must keep all existing fields/values. V2 adds a bounded schedule block equivalent to:

    schedule_fingerprint
    compiler_semantics_version
    execution_key
    isolation
    cleanup_policy
    cleanup_status/details

Per-event evidence adds:

    compiled_index
    optional phase identity
    scheduled_offset_ns
    applied_elapsed_ns
    late_by_ns
    resulting generations

No payload bytes. Failure/cleanup messages remain bounded.

The existing MAX_SCENARIO_RUNS bound remains authoritative unless M027 demonstrates and documents a reason to change it. Per-run event trails remain bounded by M026 compiled-event limits.

## Native API

Keep POST /v1/scenarios/apply backward compatible for version-1 documents. Add version-aware parsing so a version-2 schedule can be validated/compiled server-side and applied through the same owned run supervisor.

Add additive endpoints equivalent to:

    POST /v1/scenarios/validate
    POST /v1/scenarios/compile

validate returns bounded semantic validation/fingerprint information and creates no run. compile returns the normalized compiled event tape plus fingerprint and creates no run.

Do not add a separate schedule daemon or shadow registry. GET/DELETE /v1/scenarios/{run_id} continue to inspect/cancel both versions.

Exact DTO names/response envelopes must follow native v1 error/body/auth conventions and be frozen by JSON fixtures.

## CLI

Extend the existing scenario namespace rather than creating a second scheduler:

    eggchaos scenario validate <file>
    eggchaos scenario compile <file>
    eggchaos scenario apply <file>
    eggchaos scenario get <run_id>
    eggchaos scenario cancel <run_id>

Apply continues to accept existing v1 JSON. V2 accepts JSON or TOML, with format selected explicitly or unambiguously from file extension. The CLI may deserialize TOML into the shared v2 DTO, but it must send the semantic document to server-side validation/compiler authority; it must not expand phases or derive fingerprints itself.

JSON mode emits exactly one machine-readable document and exits nonzero on failure.

## Metrics

Only low-cardinality additions are permitted, for example total v2 runs/events by coarse outcome and a scheduler lateness histogram without run/event/phase labels.

Do not label by run ID, schedule fingerprint, phase name, proxy peer, arbitrary fault ID, execution key, or source file.

## Ordered work packages

### WP1 — Scenario supervisor integration

Introduce a version-aware run program that reuses the existing JoinSet/token ownership and bounded run registry. No detached tasks.

### WP2 — Absolute-deadline driver

Implement epoch + offset scheduling with sleep_until, cancellation race, stable due-event order, and lateness evidence. Add paused-time tests proving no cumulative drift.

### WP3 — Strict/live generation ownership

Implement touched-resource discovery, initial snapshots, per-resource expected-generation tracking, strict conflict behavior, and live-mode current-state behavior for both stream and datagram plans.

### WP4 — Cleanup lifecycle

Implement restore-initial/leave across completion/failure/cancellation, CAS-safe non-clobber behavior, bounded cleanup evidence, and shutdown interaction.

### WP5 — Native DTO/routes

Add version-aware apply plus validate/compile endpoints, bounded bodies/errors, auth parity, exact JSON fixtures, and no ScenarioV1 regression.

### WP6 — CLI/TOML authoring

Add validate/compile and v2 TOML/JSON file support while keeping CLI semantics thin and server-authoritative.

### WP7 — Observability and metrics

Expose schedule fingerprint/execution identity, scheduled versus actual timing, cleanup outcome, and bounded low-cardinality counters/histograms.

### WP8 — Concurrent mutation and lifecycle tests

Exercise manual updates, another scenario, cancellation, shutdown, stream transition drains, and queued datagram generations under both isolation modes.

### WP9 — Documentation

Update control-plane, architecture, replay-limit, CLI, and schedule-file documentation. Clearly distinguish deterministic policy/event identity from nondeterministic OS/application traffic timing.

## Invariants and failure semantics

- One ControlState remains the publication authority.
- V2 randomness never depends on run_id or scheduler lateness.
- Later deadlines are anchored to one epoch and never drift because an earlier event was slow.
- Strict mode never silently incorporates or overwrites an external generation.
- Restore cleanup never clobbers externally-owned state.
- Stream buffered bytes and datagram queued candidates retain their existing generation semantics.
- Cancellation cannot leave an untracked scenario task.
- V1 documents remain accepted with their existing schema and run lifecycle.
- All retained state and error text remain bounded.

## Required tests

At minimum:

- paused time: events at 1s/2s/3s fire at those epoch offsets even when simulated application work occurs between them;
- same-time events execute in compiled order;
- already-late events execute immediately and record deterministic nonnegative lateness fields;
- same v2 schedule under different run IDs publishes identical seed namespaces;
- strict stream manual mutation causes fail-fast conflict;
- strict datagram manual mutation causes fail-fast conflict;
- live mode uses current state according to documented semantics;
- cancel while sleeping transitions promptly and performs selected cleanup;
- failure after several events performs selected cleanup;
- restore-initial succeeds only while generations remain owned;
- external mutation before cleanup causes cleanup conflict and is not overwritten;
- cleanup of one conflicted resource does not prevent bounded cleanup of another;
- service shutdown cancels/joins v2 tasks;
- stream transition evidence remains correct when a scheduled generation arrives while old bytes drain;
- datagram old/new queued generations remain admission-stable across scheduled publications;
- validate/compile create no run;
- v1 JSON fixture/apply/get/cancel behavior remains green;
- TOML and JSON v2 apply compile to the same fingerprint.

## Verification

Minimum:

    ./scripts/check.sh
    cargo test -p eggchaos-server --all-features
    cargo test -p eggchaos-cli --all-features
    cargo test --workspace --all-features

Focused paused-time/concurrency tests must be ordinary deterministic integration/unit tests, not wall-clock-only evidence.

## Acceptance criteria

M027 closes only when:

- M026 is closed;
- v2 schedules execute through the existing owned supervisor/ControlState authority;
- deadline scheduling is epoch-anchored and drift-free in paused-time tests;
- strict/live isolation semantics match ADR 004 for stream and datagram policies;
- restore-initial cleanup is CAS-safe and non-clobbering;
- run evidence contains stable schedule identity plus scheduled/actual timing without payload capture;
- validate/compile/apply native surfaces are bounded, authenticated consistently, and fixture-tested;
- CLI supports v2 JSON/TOML without implementing its own compiler;
- ScenarioV1 compatibility tests remain green;
- no data-plane fault semantics were changed to accommodate scheduling;
- exact-candidate closure evidence exists.

Create plans/closure/M027-scenario-schedule-runtime-control-and-lifecycle-closure.md.

## Stop/rejection conditions

Do not close if:

- event deadlines are chained from prior event completion;
- strict mode uses unconditional publication or silently rebases after external mutation;
- cleanup overwrites a generation the run no longer owns;
- v2 run_id affects seed namespace;
- a schedule task can outlive the service untracked;
- CLI and server have separate expansion/fingerprint implementations;
- metrics gain unbounded labels;
- stream/datagram engines gain schedule-clock branches;
- v1 wire compatibility is broken without an explicit versioned migration.

## Follow-on activation

On clean M027 closure, M028 becomes ready. Any newly discovered need for branches/predicates, proxy lifecycle actions, continuous ramps, or persisted cron scheduling is separate follow-on work and must not be pulled into M027.