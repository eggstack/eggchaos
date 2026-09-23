# M011 — Live State, Scenario, and Observability Corrective

Status: closed
Depends on: M009, M010
Successor: M012

## Historical closure note

M011 closed at candidate `acd0883`; see `plans/closure/M011-live-state-scenario-observability-corrective-closure.md`. Status retained as completed history; M014 reconciles planning state and M015 is the final pre-tag authority.

## Objective

Reconcile live policy generations, canonical proxy state, connection evidence, scenarios, metrics, and lifecycle ownership so the M005 promises become true end-to-end rather than existing as partially independent mechanisms.

M011 is deliberately after M009/M010 because live mutation cannot be qualified until fault semantics and runtime authority are correct.

## User-visible outcome

After M011:

- `GET` state always reflects the same fault plans active connections can observe;
- every policy publication has one coherent generation and seed namespace;
- active connection snapshots report accepted/current policy generations truthfully;
- scenarios are owned, cancellable runs with observable status/evidence;
- a scenario seed actually participates in the deterministic decisions it claims to reproduce;
- connection kill/shutdown and scenario cancellation are durable;
- metrics and evidence reconcile with actual runtime outcomes;
- closed connection history is bounded.

## Baseline findings

The audit found:

1. `ControlState::publish_plans()` currently publishes to `LivePolicy` but leaves `ProxySpec.upstream_faults` / `downstream_faults` stale.
2. Scenario actions call `get()`, so later events can rebuild from stale plans and overwrite prior live mutations.
3. `Scenario.seed` is returned in metadata but does not currently influence engine RNG state.
4. The HTTP scenario endpoint uses an untracked `tokio::spawn` and discards the result.
5. Connection snapshots contain only an accept-time generation constant and do not report current live generation/transitions.
6. Native metrics are currently only aggregate accepted/completed/config-generation counters, well below the planned diagnostic surface.
7. Historical M005 closure therefore needs corrective qualification before release.

## Scope

Primary surfaces:

```text
crates/eggchaos-core/src/policy.rs
crates/eggchaos-core/src/stream.rs
crates/eggchaos-server/src/runtime.rs
crates/eggchaos-server/src/admin.rs
crates/eggchaos-server/src/scenario.rs
docs/control-plane.md
docs/configuration.md
plans/reference/verification-matrix.md
```

## Non-goals

Do not:

- reimplement M009 fault algorithms;
- rework listener CRUD already owned by M010;
- add arbitrary scenario scripting;
- add distributed scenario coordination;
- store payload contents;
- add high-cardinality Prometheus labels;
- implement Toxiproxy translation (M012).

## Canonical published policy

Refactor the live policy snapshot so a generation carries all deterministic inputs required to compile a direction engine.

A suitable conceptual type is:

```rust
pub struct PublishedPolicy {
    pub generation: u64,
    pub plan: Arc<FaultPlan>,
    pub seed_namespace: u64,
}
```

Exact fields may differ.

Requirements:

- plan + generation + seed namespace are published atomically as one immutable snapshot;
- canonical proxy state references the same published snapshot/plan;
- `get/list` cannot return a plan older than the policy generation they report;
- active streams can compare one observed generation value and transition safely.

Avoid a separate atomic generation increment that can temporarily disagree with the stored plan.

## Generation model

Define and document:

- global service/config generation;
- per-proxy upstream/downstream policy generation;
- accepted generation recorded when connection starts;
- current observed generation per direction;
- pending transition generation when a barrier is draining.

Global generation increments once per committed control transaction.

Policy generation increments once per successful plan publication.

Connection evidence distinguishes them instead of storing an ambiguous single `generation: 1`.

## Seed namespace model

Scenario replay requires the scenario seed to affect scenario-controlled fault decisions.

Recommended design:

- service/proxy initial policies use the configured service/proxy seed namespace;
- manual control updates retain the policy's current seed namespace unless the API explicitly changes it;
- scenario-run publications derive a policy seed namespace from `scenario.seed + scenario run id/hash + event identity` using the versioned deterministic derivation function;
- an active connection transitioning to that policy recompiles its fault-local RNGs using that published seed namespace and its stable connection key.

The exact derivation must be documented and golden-tested.

Do not make randomness depend on scenario task scheduling.

## Connection evidence

Expand active/final connection records to include:

- connection ID;
- proxy;
- connection key/ordinal;
- peer/upstream;
- accepted global generation;
- observed upstream policy generation;
- observed downstream policy generation;
- pending transition generation if any;
- RNG version;
- safe seed namespace/reference;
- selected active fault IDs per direction;
- bytes accepted/forwarded/discarded per direction;
- injected termination request/outcome;
- reset applied/unsupported/failed where applicable;
- final close classification.

Do not record payload bytes or request contents.

## Closed history

Honor `AdmissionLimits.history` with a bounded FIFO/ring of final connection summaries.

Requirements:

- active map contains only active/connecting sessions;
- finalizer removes active entry exactly once;
- history zero disables retention;
- history never exceeds bound;
- API pagination/limit is bounded if history is exposed.

## Metrics

Expand low-cardinality metrics to a useful minimum:

- connections accepted/completed/active by proxy where proxy count itself is bounded;
- connection outcomes by coarse outcome class;
- bytes accepted/forwarded/discarded by proxy + direction;
- fault activation counts by proxy + direction + fault type, not arbitrary fault ID;
- injected graceful/hard-reset request counts;
- reset applied/unsupported/failed;
- policy generation gauges;
- transition counts/outcomes;
- queue high-water/current bytes where aggregation is practical.

Do not label by connection ID, peer IP, scenario UUID, arbitrary fault ID, or unbounded hostname.

Metric totals must reconcile against deterministic fixture evidence.

## Scenario runtime

Replace fire-and-forget scenario execution with an owned run supervisor.

Suggested API:

```text
POST   /v1/scenarios/apply          -> { run_id, seed, status }
GET    /v1/scenarios/{run_id}
DELETE /v1/scenarios/{run_id}       -> cancel
```

A scenario run record is bounded and may be retained in a small history.

The root service/control supervisor owns scenario tasks and cancels them during service shutdown.

No detached `tokio::spawn` whose result is discarded.

## Scenario semantics

Keep v1 small:

- ordered relative-time events;
- set/replace directional plan;
- remove fault;
- optional proxy enable/disable only if M010 lifecycle semantics make it safe.

Requirements:

- events validated entirely before run begins where possible;
- relative time is monotonic;
- run cancellation interrupts sleeps promptly;
- each applied event records resulting global/policy generations;
- failures stop or continue according to one documented policy; prefer fail-fast for v1;
- final report records applied count, failure/cancellation, seed, and generation trail.

Do not add loops, branches, shell commands, HTTP callbacks, or arbitrary code.

## Live transition evidence

M009 provides byte-safe barrier transitions. M011 must expose their state.

An active connection should be able to say:

- currently on upstream generation N;
- transition to N+1 pending because old buffered bytes remain;
- transitioned to N+1;
- termination prevented transition.

This may be implemented with shared stream evidence handles updated by `ChaosStream`.

Avoid requiring the admin API to lock each stream directly.

## Ordered work packages

1. **WP1 — Atomic policy snapshot:** unify plan/generation/seed namespace publication and canonical proxy reads.
2. **WP2 — Connection evidence handles:** expose observed/pending generations, fault activation, counters, and termination outcome without payload capture.
3. **WP3 — Final connection history/metrics:** implement bounded history and low-cardinality metric reconciliation.
4. **WP4 — Owned scenario supervisor:** replace fire-and-forget tasks with run IDs, cancellation, bounded status/history, and service ownership.
5. **WP5 — Seed-effective scenarios:** make scenario seeds participate in published policy RNG namespaces and commit golden replay fixtures.
6. **WP6 — Concurrent mutation tests:** exercise manual updates plus scenario events under active buffered connections and verify no stale-plan rollback.
7. **WP7 — API/docs:** expose connection/scenario evidence and update control-plane/reproducibility documentation.
8. **WP8 — Closure:** exact-commit live-state/scenario/metrics qualification.

## Required tests

At minimum:

- publish fault A, GET returns A, active stream observes same generation;
- publish A then B; scenario remove from current state yields B-derived result, never stale pre-A/B state;
- concurrent publications produce unique monotonic generations and one canonical winner/order;
- active connection reports transition pending while M009 buffer drains;
- accepted/current upstream/downstream generations update correctly;
- scenario seed difference changes deterministic probabilistic/jitter/slice decisions under same connection key;
- same scenario seed and event identities reproduce the same decisions despite scheduler noise;
- scenario cancel during sleep returns boundedly;
- service shutdown cancels active scenarios;
- scenario failure is observable, not discarded;
- history bound 0/N;
- metrics reconcile accepted/completed/bytes/discard/reset totals;
- no high-cardinality metric labels;
- connection evidence contains no payload bytes.

## Verification commands

```sh
cargo fmt --all -- --check
cargo clippy -p eggchaos-core -p eggchaos-server --all-targets --all-features -- -D warnings
cargo test -p eggchaos-core --all-features
cargo test -p eggchaos-server --all-features
cargo test --workspace --all-features
```

Add deterministic scenario fixture replay to a checked-in qualification command or ordinary integration suite.

## Acceptance criteria

M011 closes only when:

- canonical stored plans and live policy snapshots cannot diverge;
- policy publication is atomic across plan/generation/seed namespace;
- scenario seed affects deterministic engine decisions;
- scenarios are structured, owned, cancellable, and observable;
- active/final connection evidence reports real live-generation state;
- history is bounded;
- metrics materially cover injected behavior and reconcile with fixtures;
- stale-state rollback tests pass;
- docs accurately describe replay limits;
- closure evidence exists.

## Stop/rejection conditions

Stop/revise if:

- a plan can be published while GET still returns an older canonical fault list;
- scenario determinism still ignores its seed;
- scenario tasks can outlive the service untracked;
- connection evidence requires payload inspection;
- metric labels are unbounded;
- fixing generation consistency requires a data-plane global mutex;
- live transition evidence can disagree with the engine's actual generation.

## Follow-on activation

On M011 closure, M012 becomes ready.
