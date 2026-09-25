# M030 — Consumer-Neutral Experiment Harness and Coordinated Start

Status: closed  
Depends on: M029 (closed), ADR 005  
Closed at: `0bc45f03e1a944a957cd80edabcf4489c9f4afbc`; evidence in `plans/closure/M030-consumer-neutral-experiment-harness-and-coordinated-start-closure.md`.
Role: reusable deterministic experiment orchestration boundary

## Objective

Create a narrow, consumer-neutral experiment layer that lets Rust test
harnesses coordinate an Eggchaos Scenario V2 schedule and a caller workload
from one process-local monotonic epoch, without depending on the standalone
admin server and without introducing EggReplay/EggProbe semantics.

M030 must reuse the already-qualified Scenario V2 compiler, fingerprint,
isolation, cleanup, and generation-publication model. It may extract pure
Scenario V2 semantics from `eggchaos-server` into a narrower crate when that
is required to avoid making every embedded consumer depend on the full
server/admin stack.

The result should be a small library surface suitable for later EggReplay
regression tests and EggProbe controlled-impairment experiments.

## Baseline and dependencies

M026–M028 froze and qualified:

- ScenarioScheduleV2 source semantics;
- deterministic bounded compilation;
- SHA-256 schedule fingerprinting;
- run_id-independent policy namespace derivation;
- absolute monotonic deadlines;
- strict/live isolation;
- restore-initial/leave cleanup;
- bounded run/event evidence;
- stream/datagram explicit actions.

Those semantics currently execute through `eggchaos-server::ControlState`.
M029 adds a composable EggFetch physical-stream adapter, caller-controlled
connection identity, and externally consumable transport evidence.

ADR 005 requires downstream products to consume these authorities rather than
creating product-specific schedulers.

## Architectural target

Prefer a dependency shape equivalent to:

    eggchaos-core
          ^
          |
    eggchaos-experiment (or equivalently narrow crate)
       ^            ^
       |            |
eggchaos-server   downstream Rust harnesses
       |
   ControlState adapter

    eggchaos-eggfetch
          ^
          |
 downstream transport composition

The exact crate name may differ, but the reusable experiment layer must not
depend on `eggchaos-server`, EggServe, CLI parsing, Toxiproxy, EggReplay, or
EggProbe.

If moving the existing Scenario V2 pure types/compiler is too disruptive for
one milestone, a temporary internal decomposition is acceptable only if the
published downstream harness does not force the full server dependency.
Preserve existing `eggchaos-server` public re-exports for source
compatibility.

## Scope

### In scope

- A narrow reusable crate/module for Scenario V2 semantic types and/or
  experiment execution.
- Preservation of all existing Scenario V2 compiler fingerprints and golden
  vectors.
- Compatibility re-exports from `eggchaos-server` for currently public
  Scenario V2 items.
- A consumer-neutral policy target abstraction over named directional stream
  and datagram resources.
- A `ControlState` adapter implementing that abstraction without creating a
  second state store.
- An in-process stream-policy adapter suitable for the M029 EggFetch chaos
  adapter.
- Explicit capability/unsupported handling for transport families a target
  does not implement.
- Preparation before execution: compile, validate target capabilities, resolve
  touched resources, and capture required initial state without starting time.
- One shared process-local Tokio monotonic experiment epoch.
- A bounded start gate/barrier or equivalent that makes the same captured epoch
  visible to the schedule driver and caller workload.
- Absolute `epoch + offset` schedule deadlines identical to M027 semantics.
- Strict/live ownership and restore/leave cleanup equivalent to the server
  runtime.
- Bounded experiment/schedule evidence suitable for caller correlation.
- Cancellation and shutdown semantics with no detached tasks.
- Paused-time tests for shared-epoch behavior and schedule drift.
- Documentation of what is deterministic and what remains observed live timing.

### Non-goals

- No EggReplay fixture, flow, regression, or replay-server API.
- No EggProbe plan/report/probe/assertion API.
- No standalone daemon replacement.
- No second admin HTTP API.
- No cross-process clock synchronization.
- No wall-clock scheduled start, cron, calendar, timezone, or persistence.
- No new Scenario V2 syntax.
- No new fault kinds.
- No arbitrary callbacks or shell execution from schedule documents.
- No generic "run any closure from a schedule action" feature.
- No implicit datagram-to-stream translation for targets without datagram
  support.
- No automatic HTTP pooling policy changes.

## Required reusable semantic boundary

M030 must leave one authority for Scenario V2 semantic compilation.

If the current `eggchaos-server::scenario_v2` pure types/compiler move to a
new crate, preserve exact:

- `SCHEDULE_SCHEMA_VERSION`;
- `COMPILER_SEMANTICS_VERSION`;
- canonical fingerprint bytes and golden digests;
- compiled event ordering/indices/offsets;
- repeat/expansion limits;
- `derive_schedule_policy_seed` input contract;
- isolation and cleanup spellings;
- stream/datagram action separation.

The server's native DTO layer may remain in `eggchaos-server`, converting to
the shared semantic types.

Existing public server imports should continue via re-export when practical so
M030 is an extraction, not a gratuitous user migration.

## Policy target abstraction

Define one narrow target contract, trait, or equivalent over the operations the
Scenario V2 runtime actually requires.

Conceptually it must support:

    capabilities()
    snapshot(resource)
    publish(resource, expected_generation, plan, seed_namespace)
    current_generation(resource)

where `resource` distinguishes at least:

    stream { name, direction }
    datagram { name, direction }

The exact Rust form may split stream/datagram methods to keep type safety.

Required properties:

- no arbitrary user callback appears in a schedule;
- expected-generation publication remains mandatory;
- target errors have bounded stable categories such as missing resource,
  unsupported capability, generation conflict, validation, and internal;
- the target abstraction does not expose payload bytes;
- no lock is held across a schedule sleep;
- the server adapter delegates to existing ControlState publication methods;
- the in-process stream adapter delegates to the same LivePolicy authorities
  used by M029 rather than copying plans into a second store.

A stream-only target presented with a datagram action must fail during prepare
if the unsupported requirement can be known then. It must never silently skip
the event.

## Prepare/arm/start lifecycle

The experiment lifecycle must make "not started yet" explicit.

A preferred conceptual state machine is:

    source
      -> compiled
      -> prepared
      -> armed
      -> running
      -> completed | failed | cancelled
      -> cleanup complete

Preparation performs all failure checks that can be completed without advancing
the experiment clock:

- source validation/compile;
- expansion/bounds validation;
- target capability validation;
- touched-resource resolution;
- initial state snapshots needed for strict ownership/restore cleanup.

No policy event is published and no schedule deadline begins during prepare.

## Shared monotonic epoch

The harness must provide a single captured `tokio::time::Instant` that is
observed by both schedule execution and the caller.

An implementation may use a one-shot/watch gate, start token, or equivalent,
but it must satisfy:

1. the epoch is captured exactly once;
2. the schedule driver cannot execute an event before the epoch is released;
3. the caller can await/obtain the same epoch value;
4. every schedule deadline remains `epoch + compiled_offset`;
5. no SystemTime conversion participates in scheduling.

A convenient high-level helper may run a caller future after the common epoch
is released, but the low-level gate must remain usable when a consumer needs to
prepare its own tasks/connections first.

The contract is synchronization of the schedule clock, not synchronization of
kernel packet transmission. Documentation must state this explicitly.

## Isolation and cleanup reuse

Do not implement a weaker "test harness" ownership model.

Strict mode must retain:

- initial touched-resource snapshot;
- expected generation = last generation owned by the run;
- immediate failure on an external move before the next conflicting action.

Live mode must retain:

- current target snapshot at event fire time;
- expected-generation publication;
- no silent overwrite of a concurrent move.

Restore-initial cleanup must remain generation guarded and non-clobbering.
Cleanup conflicts are evidence, not permission to force overwrite.

A shared driver should be used by server and embedded harness if practical. If
separate small adapters remain, executable conformance tests must prove their
observable schedule semantics are identical.

## In-process EggFetch target

Provide an adapter or construction pattern that maps one logical experiment
resource name to the M029 adapter's upstream/downstream `LivePolicy` pair.

Required behavior:

- stream Scenario V2 set/remove actions work through expected generation;
- stable schedule namespaces reach the physical connection engines on live
  transition;
- current/open pooled physical streams observe the same publication semantics
  as M029;
- datagram actions return typed unsupported;
- no new dial path is created;
- the target does not own HTTP/TLS/pooling.

The experiment resource name is an eggchaos identity, not an HTTP origin or
EggReplay/EggProbe identifier.

## Evidence contract

Expose bounded experiment evidence sufficient to correlate:

- schedule/compiler semantics version;
- schedule fingerprint;
- seed and execution_key;
- caller integration identity if configured;
- captured monotonic epoch in process-local/debug form only;
- scheduled/applied event offsets and lateness;
- target resource and resulting generation;
- run outcome;
- cleanup outcome/conflicts;
- connection evidence handles/IDs supplied by M029 where the caller chooses to
  correlate them.

Do not serialize raw `tokio::time::Instant` as a portable timestamp. Portable
reports should use relative offsets and stable identity fields.

No payload capture.

## Cancellation and lifecycle

The harness must not hide background work.

Required behavior:

- driver task/future is owned by the returned harness/run handle or directly by
  the caller;
- cancellation before start prevents event publication and performs only
  preparation cleanup that is actually needed;
- cancellation while sleeping wakes promptly;
- cancellation after publications runs selected cleanup;
- dropping a handle either cancels synchronously through owned RAII state or is
  explicitly documented as requiring an awaited shutdown method; silent
  detached continuation is forbidden;
- server shutdown semantics remain equivalent after any shared-driver
  extraction.

## Ordered work packages

### WP1 — Freeze extraction boundary

Identify pure Scenario V2 model/compiler/driver concepts versus
server-native DTO and ControlState concerns. Select the narrow crate/module
layout and document compatibility re-exports before moving code.

### WP2 — Extract/reuse Scenario V2 semantics

Move or share the pure compiler/fingerprint/schedule types without changing
golden outputs. Keep the server wire DTO and admin routing where they belong.

### WP3 — Policy target contract

Implement the resource/capability/snapshot/expected-publish abstraction and
typed target errors.

### WP4 — ControlState adapter

Wire the standalone server through the reusable target/driver authority with no
second generation store and no API behavior regression.

### WP5 — Shared epoch gate

Implement prepare/arm/start semantics and one captured Tokio Instant shared by
schedule driver and caller. Add paused-time proofs.

### WP6 — In-process stream target

Adapt the M029 LivePolicy pair to the experiment target contract. Reject
unsupported datagram actions deterministically.

### WP7 — Isolation/cleanup conformance

Run strict/live, restore/leave, manual-conflict, cancellation, and failure tests
through both ControlState and in-process targets.

### WP8 — Evidence and lifecycle

Expose bounded experiment evidence and explicit owned cancellation/shutdown.
Prove no detached tasks/history growth.

### WP9 — Documentation and examples

Document an embedded consumer example using a custom workload future and the
composable EggFetch adapter. Do not add EggReplay/EggProbe-specific commands.

## Invariants and failure semantics

- Scenario V2 compile/fingerprint/namespace golden vectors do not change.
- One shared semantic compiler exists after M030.
- One process-local epoch is captured exactly once per started experiment.
- Deadlines remain absolute from that epoch.
- Prepare performs no schedule publication.
- Unsupported target capabilities fail explicitly.
- Strict/live and cleanup semantics match ADR 004.
- Server ControlState remains the server's single state authority.
- In-process stream targets use M029 LivePolicy state directly.
- No payload or consumer product model enters experiment evidence.
- No background schedule task can outlive its owner untracked.

## Required tests

At minimum:

- all existing M026/M028 golden corpus tests pass byte-for-byte;
- existing server Scenario V2 API/CLI tests pass after extraction;
- server re-export/source-compat smoke for moved public symbols;
- prepare of valid schedule changes no generation;
- prepare rejects a stream-only target when a datagram resource is required;
- start gate returns the same captured epoch to caller and driver;
- paused time: 1s/2s/3s events fire at epoch-relative deadlines with no drift;
- equal-deadline ordering matches compiled index;
- caller workload started from the gate can record the same epoch identity;
- strict external mutation conflicts on the next affected event;
- live mode observes current state according to ADR 004;
- restore cleanup does not overwrite an external generation;
- cancel before start publishes nothing;
- cancel while sleeping is prompt;
- cancel/failure after publication performs bounded cleanup;
- in-process stream target updates an already-open M029 pooled connection;
- unsupported datagram action is typed failure, never ignored;
- ControlState and in-process target conformance fixtures produce equivalent
  event-generation outcomes for the same supported stream schedule.

## Verification

Minimum:

    ./scripts/check.sh
    cargo test -p eggchaos-core --all-features
    cargo test -p eggchaos-server --all-features
    cargo test -p eggchaos-eggfetch --all-features
    cargo test --workspace --all-features
    cargo doc --workspace --all-features --no-deps

If a new publishable crate is introduced, add it to release/package smoke and
verify workspace dependency versions/publish order. Run:

    ./scripts/release-smoke.sh

Run focused paused-time and property/fuzz tests for the extracted schedule
surface. M031 remains the exact-candidate qualification authority.

## Acceptance criteria

M030 closes only when:

- downstream Rust harnesses can use Scenario V2 execution without depending on
  the full eggchaos server/admin runtime;
- existing server Scenario V2 public behavior and fingerprint corpus remain
  compatible;
- a consumer-neutral expected-generation target abstraction exists;
- ControlState and M029 LivePolicy-backed stream targets both use the shared
  schedule semantics;
- one process-local monotonic epoch is shared with the caller workload;
- strict/live/cleanup behavior matches ADR 004 through both targets;
- unsupported datagram requirements fail explicitly on stream-only targets;
- cancellation/shutdown leaves no detached task or silent stale state;
- evidence is bounded and consumer-neutral;
- no EggReplay/EggProbe production dependency is introduced;
- required tests pass on one exact candidate;
- closure evidence records any API moves/re-exports and residual limitations.

Create
`plans/closure/M030-consumer-neutral-experiment-harness-and-coordinated-start-closure.md`.

## Stop/rejection conditions

Do not close if:

- Scenario V2 fingerprints or namespace vectors change merely because code was
  extracted;
- server and embedded harness maintain separate compilers or subtly different
  scheduling semantics;
- the harness depends on the full server crate as its public minimum
  dependency;
- prepare publishes policy state;
- caller and driver use independently captured epochs;
- cross-process wall-clock synchronization is advertised as deterministic;
- unsupported actions are skipped;
- cleanup can overwrite externally owned generations;
- the implementation introduces arbitrary schedule callbacks or product-
  specific hooks;
- any schedule task can continue after its owner is dropped/shutdown without an
  explicit retained owner.

## Follow-on activation

On clean M030 closure, M031 becomes ready.

EggReplay and EggProbe downstream implementation plans remain separate work.
They may begin against the M029/M030 public seams only after those seams are
closure-backed; M030 itself must not modify either sibling repository.
