# M010 — Runtime and Control Authority Corrective

Status: ready  
Depends on: M003/M004 historical implementation  
Parallel with: M009  
Successor gate: M011

## Objective

Make the native runtime—not an independent control-plane map—the single authority for proxy lifecycle, listener ownership, connection cancellation, reset behavior, and operator CRUD.

The current implementation successfully starts static configured listeners, but runtime HTTP mutations can change `ControlState` without changing the actual listener/task graph. M010 repairs that split-brain condition and completes the native API/CLI surface promised by M004.

## User-visible outcome

After M010:

- creating a proxy through the native API actually binds and supervises its listener;
- deleting/disabling a proxy actually stops its listener and handles active connections according to documented semantics;
- changing listen/upstream performs a controlled restart-class transition;
- fault CRUD updates the live policy and canonical proxy definition together;
- native reset actually resets the service;
- connection kill is durable and cannot be lost before the relay begins waiting;
- connection records/counters are cleaned up deterministically;
- the CLI exposes the registered native proxy/fault/connection operations instead of only list/get.

## Baseline findings

The audit found:

1. `EggchaosService::start()` creates listeners from the initial `self.proxies` only.
2. `ControlState::insert()` and `remove()` mutate a `BTreeMap` but do not start/stop listener tasks.
3. `POST /v1/proxies` can therefore report success for a proxy that has no listening socket.
4. Toxiproxy compatibility inherits the same problem.
5. Native `POST /v1/reset` currently returns success without resetting fault plans/listeners.
6. Native HTTP lacks much of the planned proxy/fault CRUD surface.
7. CLI currently exposes only serve/version/reset and proxy list/get.
8. Connection cancellation is based on `Notify::notify_waiters()`, which is not a durable cancellation state if notification precedes the task's wait.
9. `finish()` uses `try_write()` for registry cleanup, so transient contention can leave stale entries.
10. The configured history bound is not currently an implemented closed-connection history authority.

Historical M003/M004 records remain preserved; M010 is their corrective successor.

## Scope

Primary affected surfaces:

```text
crates/eggchaos-server/src/runtime.rs
crates/eggchaos-server/src/admin.rs
crates/eggchaos-server/src/config.rs
crates/eggchaos-cli/src/main.rs
crates/eggchaos-server/Cargo.toml
docs/control-plane.md
docs/configuration.md
README.md
```

M010 may add `tokio-util::sync::CancellationToken` if useful for durable cancellation.

## Non-goals

Do not:

- reimplement fault state machines from M009;
- implement Toxiproxy-specific routes;
- expand scenario/evidence semantics beyond what is needed for canonical runtime control;
- add persistence/database state;
- add general forward-proxy behavior;
- add UDP.

## Single authority design

Refactor toward one runtime supervisor/command authority.

A suitable shape is:

```text
NativeAdmin / CLI / Toxiproxy adapter
               |
               v
        RuntimeControlHandle
               |
        bounded command channel
               |
               v
        service supervisor
          /    |     \
      proxy  proxy   proxy supervisors
      task   task    task
```

The exact implementation may instead use carefully locked shared state, but these invariants are mandatory:

- a successful control mutation reflects actual runtime state;
- listener ownership and proxy definition cannot diverge;
- state changes are serialized into unique generations;
- read snapshots come from the same authority that executes mutations;
- compatibility/native callers do not maintain parallel listener registries.

Avoid a central mutex on every data-plane byte. Control-plane serialization is acceptable.

## Proxy lifecycle semantics

### Create

A create operation must:

1. validate full proxy definition;
2. reserve/validate unique name;
3. bind listener before committing visible success;
4. if bind fails, leave no proxy registered;
5. publish initial fault policies;
6. spawn and register a supervised listener;
7. return actual bound address, including resolved port 0;
8. increment generation exactly once.

### Delete

Delete must:

- stop accepting immediately;
- apply documented active-connection policy (default: terminate/drain boundedly; choose one and test it);
- await/own listener termination;
- remove proxy from canonical registry;
- remove/cancel relevant active connection state;
- increment generation once;
- return success only when the runtime transition has committed.

### Enable / disable

Disabled means no active listener.

Enabling binds/spawns through the same create/start machinery while retaining definition/fault plan.

Disabling stops listener according to the same active-connection policy but retains definition.

### Update

Classify fields:

- fault-only change: live policy update, no listener restart;
- max-connection/connect-timeout change: define whether it applies live/new connections and document;
- upstream change: restart-class or explicitly new-connections-only; the M004 roadmap selected restart-class, so prefer restart;
- listen change: restart-class;
- enabled change: enable/disable lifecycle.

For restart-class updates, bind the replacement listener before destroying the old one when possible, but do not allow two contradictory canonical states. If same-port rebinding prevents transactional replacement, document the stop/bind/rollback sequence and test failure behavior.

## Native reset semantics

Define reset as a real operation.

Recommended native behavior, aligned with Toxiproxy usefulness:

- retain proxy definitions/listen/upstream;
- enable all registered proxies;
- replace upstream/downstream fault plans with empty plans;
- terminate or transition active connections according to live-fault semantics;
- increment generation once for the reset transaction.

If a different native reset contract is selected, document it and ensure M012 translates Toxiproxy reset exactly.

A reset response must not say success without state change.

## Fault CRUD authority

Complete native routes:

```text
GET    /v1/proxies/{name}/faults
POST   /v1/proxies/{name}/faults
GET    /v1/proxies/{name}/faults/{id}
PATCH  /v1/proxies/{name}/faults/{id}
DELETE /v1/proxies/{name}/faults/{id}
```

Use one typed mutation method that updates:

- canonical stored `FaultPlan`;
- shared `LivePolicy`;
- generation.

Do not update only the `ArcSwap` while leaving `ProxySpec.*_faults` stale.

Fault IDs remain unique within direction/proxy according to the native model.

## Proxy API authority

Complete:

```text
GET    /v1/proxies
POST   /v1/proxies
GET    /v1/proxies/{name}
PATCH  /v1/proxies/{name}
DELETE /v1/proxies/{name}
```

Return actual bound listener state/address, not only requested configuration.

Use stable JSON error codes for bind conflict, invalid update, not found, conflict, and restart failure.

## CLI completion

Add thin Eggfetch-backed commands matching the native API:

```text
eggchaos proxy list
eggchaos proxy get <name>
eggchaos proxy add ...
eggchaos proxy set <name> ...
eggchaos proxy remove <name>
eggchaos proxy enable <name>
eggchaos proxy disable <name>

eggchaos fault list <proxy>
eggchaos fault get <proxy> <id>
eggchaos fault add <proxy> ...
eggchaos fault set <proxy> <id> ...
eggchaos fault remove <proxy> <id>

eggchaos connection list
eggchaos connection get <id>
eggchaos connection kill <id>

eggchaos reset
```

Exact argument ergonomics may vary. `--json` remains one valid JSON document with nonzero exit on failure.

Do not introduce a direct in-process CLI mutation path separate from the admin API.

## Durable cancellation

Replace notification-only cancellation with level-triggered state.

Preferred: `CancellationToken` per connection/proxy/service, or an atomic cancelled flag paired with notification.

Requirements:

- cancelling before the relay select begins is still observed;
- service shutdown cascades to listener and connection children;
- proxy deletion/disable can cancel its own children without cancelling unrelated proxies;
- connection kill returns success only if the ID was active and cancellation was issued;
- repeated cancellation is idempotent.

## Structured task ownership

Do not use detached tasks for listeners or connections.

Each proxy supervisor owns its connection `JoinSet` or equivalent and drains it during shutdown.

The root service owns every proxy supervisor.

A mutation returning “deleted/disabled” should not leave an untracked listener accepting traffic.

## Connection cleanup

Replace `try_write()` cleanup with an approach that guarantees registry removal.

Possible approaches:

- async finalizer called after relay/cancellation completes;
- RAII accounting guard plus awaited registry cleanup;
- supervisor receives connection-completed message and serially removes it.

Counters must not underflow or leak.

If closed history is kept, move the final snapshot into a bounded ring/deque honoring `AdmissionLimits.history`. Active and history registries must be distinct.

## M009 termination integration

M010 must consume the durable termination signal introduced by M009.

Graceful request:

- stop the relay deterministically and perform appropriate half/full close.

Hard-reset request:

- at the concrete `TcpStream` edge, attempt a true reset only through safe supported APIs;
- expose `applied`, `unsupported`, or `failed` evidence;
- do not call an ordinary FIN a reset.

If M009 is not yet closed, M010 may implement the runtime/supervisor portions in parallel but cannot close until integration tests against the final M009 termination contract pass.

## Ordered work packages

1. **WP1 — Supervisor/control architecture:** create a single runtime mutation authority and migrate read snapshots/generation ownership to it.
2. **WP2 — Dynamic proxy lifecycle:** implement create/delete/enable/disable/restart-class updates with real listener/task changes and rollback behavior.
3. **WP3 — Durable cancellation/cleanup:** replace notification races and `try_write` cleanup; implement bounded closed history if retained.
4. **WP4 — Fault/proxy native CRUD:** complete typed runtime mutation methods and versioned API routes, keeping canonical plans and live policy synchronized.
5. **WP5 — Real reset:** implement native reset transaction and tests.
6. **WP6 — CLI completion:** expose all native operations through the Eggfetch control client and stable JSON output.
7. **WP7 — M009 termination integration:** act on graceful/hard-reset signals at the TCP runtime edge and qualify platform behavior.
8. **WP8 — Closure:** end-to-end dynamic lifecycle, CLI/API, shutdown, and exact-commit evidence.

## Required tests

At minimum:

- API create with port 0 -> actual bound address -> successful relay;
- create bind conflict -> no registered ghost proxy;
- delete -> listener no longer accepts;
- disable -> listener stops; enable -> listener returns;
- upstream update directs new/restarted traffic to the new target;
- failed restart leaves a documented consistent state;
- concurrent create/update/delete operations serialize into unique generations;
- fault add/update/delete reflected both in GET and active policy;
- reset empties faults and enables proxies;
- kill issued during upstream connect is not lost;
- kill issued during relay terminates it;
- service shutdown with many active connections leaves zero active registry entries/tasks;
- connection finish under registry contention still removes the record;
- history bound never exceeds configured size;
- CLI JSON create/fault/kill/reset end-to-end;
- hard-reset request reports truthful platform outcome.

## Verification commands

```sh
cargo fmt --all -- --check
cargo clippy -p eggchaos-server -p eggchaos-cli --all-targets --all-features -- -D warnings
cargo test -p eggchaos-server --all-features
cargo test -p eggchaos-cli --all-features
cargo test --workspace --all-features
```

Run dynamic lifecycle tests on Linux, macOS, and Windows CI before closure where platform behavior matters.

## Acceptance criteria

M010 closes only when:

- runtime and control-plane state cannot report a listener that does not exist;
- proxy CRUD mutates actual supervised listeners;
- native fault CRUD is complete and canonical/live state remain synchronized;
- native reset changes real state;
- CLI exposes the registered control surface;
- cancellation is level-triggered/durable;
- active registry cleanup is guaranteed rather than best-effort;
- all tasks remain structurally owned;
- M009 termination requests are consumed truthfully at the TCP edge;
- exact-commit closure evidence exists.

## Stop/rejection conditions

Stop and revise if:

- control API still mutates a registry independently of listener ownership;
- create can succeed before bind;
- delete can succeed while an untracked listener remains alive;
- connection cancellation still depends on a one-shot notify race;
- runtime reset is only a response payload;
- canonical fault state and live policy can diverge;
- hard reset is faked with normal shutdown;
- dynamic lifecycle requires turning eggchaos into a general forward proxy.

## Follow-on activation

M010 may proceed in parallel with M009.

M011 becomes ready only after both M009 and M010 close.
