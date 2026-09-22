# M005 — Live Mutation, Observability, and Scenarios

Status: closed  
Depends on: M004  
Successors: M006, M007

## Objective

Upgrade eggchaos from a configuration-driven fixed-target proxy into a useful chaos-testing control system: fault changes can safely affect active connections, connection state is inspectable, metrics/evidence explain what happened, and deterministic scenarios can be applied/replayed.

This milestone closes the largest semantic gap between a static impairment proxy and a Toxiproxy-class testing tool.

## User-visible outcome

A running connection can receive supported fault changes without reconnecting, while bytes already accepted by preserving fault stages are neither duplicated nor accidentally lost.

Operators can:

- inspect active connections;
- see the config generation each connection accepted and currently observes;
- kill an active connection;
- change faults and know whether the change was live, barrier-applied, or restart-class;
- collect Prometheus metrics;
- capture deterministic execution evidence;
- apply a bounded scenario containing time-relative control events.

## Preconditions

M004 is closed with:

- one native mutation authority;
- generation-aware proxy/fault registry;
- native API;
- JSON CLI;
- bounded configuration/control plane;
- stable runtime handles.

If M004 only applies fault changes to new connections, preserve that as an explicit baseline until this milestone replaces it with proven active semantics.

## Core design requirement

Follow ADR 002.

An active structural update may not simply replace an `Arc<FaultPlan>` when the old engine owns accepted bytes.

Each direction needs a safe generation transition mechanism with an equivalent of:

```text
Active(generation N)
  -> Quiescing(N, pending N+1)
  -> Active(N+1)
```

or another design that proves the same property.

Already accepted preserving bytes remain owned by their original generation until delivered. A destructive fault may intentionally discard bytes only according to its documented semantics and accounting.

## Mutation classes

Expose these classes in native operation results/evidence.

### Parameter-live

The runtime can change a parameter in place while retaining relevant state.

Candidate examples:

- bandwidth rate, while preserving token state according to documented rule;
- probability only for future generation activations, not retroactively changing an already selected connection unless explicitly specified.

Do not label a field parameter-live merely because it is stored in an atomic. Semantics must be well-defined.

### Barrier-transition

The runtime stops admitting new bytes into an old fault pipeline, drains/resolves its accepted state, then activates a new compiled generation.

Expected for:

- fault add/remove;
- fault reordering;
- latency buffer/policy changes;
- slice structure changes;
- changes where old queued bytes would otherwise become ambiguous.

The control API may return `pending`/transition metadata until the barrier completes if necessary.

### Connection-restart

Cannot safely apply to an existing physical connection.

Examples:

- upstream target change;
- listen address change;
- fundamental transport setting changes.

Document which active connections are terminated/restarted.

## Shared live-policy handle

M005 should introduce the stable policy mechanism later reused by `eggchaos-eggfetch`.

Requirements:

- a fault-wrapped physical stream can observe supported generation changes without needing the outer caller to recreate the wrapper;
- updates are scoped to the intended proxy/policy identity;
- readers do not take a global mutex on every byte;
- immutable compiled generations are preferred for read-mostly access;
- old generations stay alive while a connection transition still references them;
- publication order is explicit.

`ArcSwap` or an equivalent snapshot primitive is a reasonable registry publication tool, but it does not by itself solve buffered-state transition. Separate plan publication from connection-local transition state.

## Active connection control

Extend native API/CLI:

```text
GET    /v1/connections
GET    /v1/connections/{id}
DELETE /v1/connections/{id}
```

Deletion means explicit connection termination, not removing history only.

Optional query filters may include proxy, state, and generation, but keep cardinality/complexity bounded.

Connection details should expose:

- stable ID;
- proxy identity;
- accept ordinal / deterministic connection key;
- peer/local/upstream safe addresses;
- accepted/current generation;
- selected faults;
- active/pending transition state;
- byte accounting;
- injected/discarded counts;
- duration;
- last classified outcome once closed if retained;
- RNG version/seed reference needed for replay.

Retained closed history must have a hard entry/time bound. It may be disabled by default if not needed.

## Metrics

Use `prometheus-client` directly unless another genuinely generic Eggstack metrics crate exists by implementation time.

Do not pull `eggress-metrics` just for exposition; its current model is coupled to Eggress server/UDP/protocol state.

### Minimum metric families

Low-cardinality candidates:

- accepted connections total by proxy;
- active connections by proxy;
- completed connections by proxy/outcome;
- upstream/downstream bytes accepted;
- bytes forwarded;
- bytes intentionally discarded;
- fault activation total by proxy/direction/fault type;
- injected termination total by proxy/mode;
- transition total/result;
- latency queue current/high-water bytes;
- throttle wait duration/cumulative events;
- config generation gauge;
- admin request counts only if useful.

Avoid connection ID, peer IP, fault arbitrary ID, raw upstream host, or scenario UUID as Prometheus labels when that creates unbounded cardinality.

Per-connection rich detail belongs in the connection/evidence endpoint, not metric labels.

### Exposition

Native `GET /metrics` returns Prometheus text.

If M006 later needs Toxiproxy metric aliases, add them in the compatibility adapter without making them the native storage authority.

## Evidence/event model

Define a bounded, serializable execution summary.

A connection evidence record should include enough to answer:

- what policy generation did it start under?
- which probabilistic faults activated?
- what RNG version/connection key was used?
- which generation transitions occurred?
- how many bytes were accepted/forwarded/discarded?
- how much delay/throttle was injected?
- was termination natural, remote, local, injected, reset-requested, reset-applied, or shutdown-driven?
- what scenario/operator action caused a transition?

Do not record payload contents.

If an event stream is implemented, it must have bounded fan-out/buffer semantics and explicit drop/backpressure policy. An event stream is optional for M005; a final evidence record is required.

## Scenario model v1

Keep the first scenario format deliberately small.

A scenario is a deterministic sequence of control actions relative to scenario start.

Example conceptual form:

```toml
version = 1
seed = 42

[[event]]
at = "0s"
action = "set_fault"
proxy = "redis"
fault = { ... latency ... }

[[event]]
at = "5s"
action = "set_fault"
proxy = "redis"
fault = { ... blackhole ... }

[[event]]
at = "8s"
action = "remove_fault"
proxy = "redis"
fault_id = "outage"
```

Required initial actions:

- add/update/remove fault;
- enable/disable proxy if semantics are stable;
- terminate a connection only if addressed deterministically enough for replay;
- reset to baseline if useful.

Do not add a programming language, loops, conditions, distributed coordination, or arbitrary shell commands.

### Scenario timing

Use monotonic time relative to start.

The scenario runner owns cancellation and must stop cleanly on service shutdown.

When omitted, a run seed may be generated once, but it must be returned/logged in machine-readable evidence before random decisions matter.

## Replay evidence

Add a command/API or documented method to export scenario execution metadata sufficient to rerun:

- scenario version/hash;
- resolved run seed;
- RNG version;
- event schedule;
- resulting generations;
- connection keys encountered;
- injected outcomes.

M005 does not need to reproduce nondeterminism outside eggchaos, such as application timing or DNS changes. Documentation must distinguish “replay the eggchaos decisions” from “bit-for-bit replay the entire distributed system.”

## Live mutation test strategy

Use slow/blocked inner writers and paused time to create known bytes resident in a fault stage.

Required examples:

1. Latency queue contains A/B, latency fault is removed, A/B still arrive exactly once before/at the defined barrier, future C follows new generation.
2. Slice fault is changed mid-segment; accepted original bytes are not lost/duplicated.
3. Bandwidth rate changes while token state is nonzero; observed burst matches documented transition rule.
4. Blackhole removal unblocks future behavior according to contract; intentionally discarded bytes remain counted as discarded.
5. Fault reorder while traffic is continuous reaches a transition barrier and exposes pending/current generations truthfully.
6. Concurrent admin updates serialize or conflict deterministically; they do not create a mixed partial generation.
7. Service shutdown while a transition is pending terminates without orphan state.

Property tests should generate update sequences against preserving fault combinations and assert byte conservation.

## Concurrency control

Choose and document one update ordering model per proxy, such as a serial command queue or generation compare-and-swap.

If optimistic generation preconditions are exposed:

- clients can submit expected generation;
- stale updates fail with a stable conflict response;
- successful mutation yields exactly one next generation.

Do not permit two concurrent requests to produce two different states with the same generation number.

## API additions

Native API may add:

```text
DELETE /v1/connections/{id}
POST   /v1/scenarios/apply
GET    /v1/scenarios/{run_id}       # optional if bounded run records exist
GET    /v1/evidence/connections/{id} # optional if distinct from connection GET
GET    /metrics
```

Exact path choices may be adjusted, but keep versioned native routes separate from M006 compatibility routes.

Mutation responses should include:

- resulting generation;
- mutation class;
- whether active connections are already on it, transitioning, or restart-required;
- any affected/terminated count where safe.

## CLI additions

Suggested:

```text
eggchaos connection kill <id>
eggchaos scenario apply <file> --json
eggchaos metrics
```

A raw evidence export command is useful if the API supports it.

## Tests

Beyond live mutation tests:

- metrics totals/gauges reconcile with connection evidence for deterministic fixtures;
- repeated metric render does not mutate totals;
- high-cardinality fields are absent from labels;
- bounded history evicts according to documented rule;
- scenario cancellation;
- scenario same seed/same connection keys -> same fault decisions;
- unrelated scheduler noise does not change decisions;
- concurrent mutation conflict/order behavior;
- JSON evidence schema round-trip;
- no payload bytes in diagnostics.

## Performance

Measure the hot path with:

- no live updates;
- a policy generation pointer present but unchanged;
- repeated generation reads;
- transition under traffic.

The stable data path should not require a central async mutex for every read/write.

## Documentation

Update architecture/config/API docs with:

- generation semantics;
- mutation classes;
- transition guarantees;
- metrics;
- evidence;
- scenario v1;
- reproducibility limits.

Document which changes are truly live and which restart connections.

## Ordered work packages

Execute in this order:

1. **WP1 — Live policy publication:** implement immutable compiled generations and a low-contention shared policy handle without yet allowing unsafe structural swaps.
2. **WP2 — Connection-local transition machine:** implement parameter-live, barrier-transition, and restart-class semantics so old generations retain ownership of accepted bytes.
3. **WP3 — Active connection control:** expose bounded connection inspection, generation/transition state, and explicit operator termination through native service/API/CLI.
4. **WP4 — Observability authority:** add low-cardinality Prometheus metrics and bounded payload-free connection evidence with accounting reconciliation.
5. **WP5 — Scenario v1:** add bounded monotonic-time scenario scheduling, deterministic seed resolution, cancellation, and replay metadata—no loops/shell/code execution.
6. **WP6 — Concurrency/property qualification:** stress concurrent mutations and transition barriers under queued data; prove byte conservation for preserving faults.
7. **WP7 — Hot-path/performance check:** verify ordinary traffic does not acquire a global admin mutex and record unchanged-generation/transition overhead.
8. **WP8 — Documentation/closure:** reconcile mutation classes/evidence/scenarios, close M005, and mark M006 and M007 ready in parallel.

## Acceptance criteria

M005 closes only when:

- active preserving streams can transition generations without accidental byte loss/duplication;
- mutation classes are explicit in code/API/docs;
- updates are serialized/versioned deterministically;
- active connection inspection/control works;
- metrics and evidence are bounded and tested;
- deterministic scenario v1 can be applied/cancelled/replayed at the eggchaos-decision level;
- shared live-policy handle is suitable for the upcoming Eggfetch physical-stream adapter;
- no hot-path global mutex is introduced;
- concurrency/property/integration tests pass;
- closure evidence is committed.

## Stop/rejection conditions

Stop/revise if:

- structural live mutation can only be implemented by discarding unknown buffered bytes;
- the engine needs to pause the entire process/runtime for a proxy update;
- per-byte hot path acquires a central admin/registry lock;
- metrics use unbounded labels;
- connection/evidence history is unbounded;
- scenario runner executes arbitrary commands/code;
- reproducibility still depends on Tokio task scheduling.

## Closure evidence

Create `plans/closure/M005-live-mutation-observability-and-scenarios-closure.md` with:

- candidate SHA;
- live-transition byte-conservation evidence;
- concurrent update evidence;
- deterministic scenario/evidence fixtures;
- metrics reconciliation;
- performance measurements;
- acceptance verdict.

Then update registry:

- M005 -> `closed`;
- M006 -> `ready`;
- M007 -> `ready`.

M006 and M007 may proceed in parallel.
