# Fixed-target server runtime

Back to [architecture overview](overview.md).

`eggchaos-server` is the fixed-target TCP runtime. It owns listeners,
upstream dials, admission, the bounded connection registry, `eggress-relay`
embedding, reset semantics, metrics tables, and the single control mutation
authority (`ControlState`, M010). The fault state machines themselves live in
`eggchaos-core`; this crate only embeds `ChaosStream<T>` at the transport
edge. User-facing dependency direction and fault-semantics baseline live in
`docs/architecture.md` and `docs/control-plane.md`.

Sources (evidence-first): `crates/eggchaos-server/src/lib.rs`,
`crates/eggchaos-server/src/runtime/` (modular runtime implementation),
`crates/eggchaos-server/Cargo.toml`, `docs/architecture.md`,
`docs/control-plane.md`. Supporting context: `crates/eggchaos-server/src/admin.rs`,
`crates/eggchaos-server/src/config.rs`, `crates/eggchaos-server/src/scenario.rs`.

The runtime module map is: `runtime/mod.rs` composes `RuntimeInner`, service
handles, shared helpers, and public re-exports; `control.rs` implements the
single `ControlState` authority; `connection.rs` owns connection finalization,
evidence merge, history, and accounting; `supervisor.rs` owns listener accept
and admission; `transport.rs` owns reset-capable TCP wrappers; `metrics.rs`
owns bounded tables/counters; `model.rs` owns public runtime DTOs; and
`tests.rs` contains the runtime regression suite. The module moves preserve
one `RuntimeInner` and one `ControlState` store.

## Role and construction chain

| Type | Path | Role |
| --- | --- | --- |
| `ServiceBuilder` | `runtime.rs:2153–2229` | Validated builder: seed, proxy list, `AdmissionLimits`, `HalfClosePolicy`, `relay_buffer`, `term_grace`. `build()` rejects duplicate names, invalid proxies/plans, and `global_connections == 0`. |
| `EggchaosService` | `runtime.rs:2232–2255` | Configured-but-not-started service. `start()` creates a `ControlState::with_params`, then `create_proxy` for enabled proxies and `import_definition` for disabled ones. |
| `ServiceHandle` | `runtime.rs:2261–2291` | Running-service handle: `bound_addresses()`, `connections()`, `history()`, `control_state()`, level-triggered `shutdown()` + `wait()` (`shutdown_and_join`). No listener/connection task is ever detached. |
| `ControlState` | `runtime.rs:936–2150` | Cloneable M010 single mutation authority over `Arc<RuntimeInner>`. Native admin, CLI-via-HTTP, Toxiproxy adapter, scenario driver, and `eggchaos-embed` all mutate through these typed methods. Datagram fault add/get/list/patch/remove is one HTTP-independent authority here (M035: duplicate/cross-direction conflicts, patch non-emptiness, plan reconstruction, generation-guarded publication); HTTP and embed only convert DTOs and map errors. `lib.rs:15–22` re-exports the full surface. |
| `RuntimeInner` | `runtime.rs:854–881` | Single store: `params`, `proxies: RwLock<BTreeMap<String, ManagedProxy>>`, `generation`, `next_conn_id`, `active`, `connections`, `cancellations`, `evidence`, `history: VecDeque<ClosedConnection>`, `metrics`, shutdown token/flag, owned `root` supervisor handles, owned scenario `JoinSet`/records/tokens. |
| `RuntimeParams` | `runtime.rs:764–788` | Per-service tuning: `seed`, `limits: AdmissionLimits`, `half_close: HalfClosePolicy` (default `Drain`), `relay_buffer` (default 64 KiB), `term_grace` (default 5 s). |
| `AdmissionLimits` | `runtime.rs:147–161` | `global_connections` (default 1024), `history` (default 256). |

Dependencies (`crates/eggchaos-server/Cargo.toml:11–29`, workspace `Cargo.toml`):
`eggchaos-core` (path), `eggress-relay` 1.0.7 (`egress-relay` crate name),
`eggserve-server` + `eggserve-primitives` 0.2.0 (admin substrate only),
`tokio` 1 + `tokio-util` 0.7 + `bytes`, `socket2` (stable-API abortive close),
`serde`/`serde_json`, `tracing`, `thiserror`, `toml`. `#![deny(unsafe_code)]`
(`lib.rs:3`). No `eggress-outbound` dependency (optional/future only).

## Proxy model

- `ProxySpec` (`runtime.rs:34–64`): `name`, `listen` (port 0 = ephemeral),
  `upstream` (fixed target), `upstream_faults` / `downstream_faults`
  (`FaultPlan`), live `upstream_policy` / `downstream_policy` (`LivePolicy`,
  `serde(skip)`), `enabled` (default true), `max_connections: Option<usize>`,
  `connect_timeout: Duration` (millis serde, default 5 s via `new()`),
  `seed: u64`. `new()` / `with_max_connections()` at `runtime.rs:82–103`.
- `ProxySpec::validate` (`runtime.rs:104–133`): name 1..=128 bytes and
  `[A-Za-z0-9._-]+` (single URL path segment), both plans valid, connect
  timeout non-zero.
- `init_policies(service_seed)` (`runtime.rs:138–142`): namespace =
  `service_seed ^ proxy.seed`; both `LivePolicy::new(plan, namespace)`.
  Canonical state is the policy snapshot; plan fields are config-origin
  mirrors refreshed from published snapshots under the same lock.
- `ProxyView` (`runtime.rs:508–541`): operator-visible definition + actual
  runtime state: `listen`, `upstream`, `bound_addr: Option<SocketAddr>`,
  `running`, `enabled`, both plans **read from the same atomic snapshot as
  the reported generation/namespace** (`RuntimeInner::view_of`,
  `runtime.rs:908–931`), `max_connections`, `connect_timeout_ms`, `seed`.
- `ProxyPatch` (`runtime.rs:462–474`): `listen` / `upstream` (restart-class),
  `enabled`, `max_connections: Option<Option<usize>>` (`None` = no change,
  `Some(None)` = clear cap), `connect_timeout_ms`. Fault plans are out of
  scope here — use fault CRUD.
- `FaultUpsert` (`runtime.rs:478–488`): `direction: Direction`, `id`
  (1..=128 bytes), `probability` (default 1.0), `kind: FaultKind`.
- `FaultPatch` (`runtime.rs:497–502`): optional `probability` / `kind` only;
  identity and direction are fixed (cross-direction move = delete + add).
- Errors: `ControlError` (`runtime.rs:557–597`) with stable codes
  `not_found` / `conflict` / `invalid` / `bind_failed` / `restart_failed`;
  `EggchaosError` (`runtime.rs:601–625`) for build/bind/lifecycle/IO.

## Listener lifecycle

All paths go through `ControlState`; success reflects an actual listener,
never a map entry alone.

- `import_definition` (`runtime.rs:1188–1217`): validate, `init_policies`,
  store with `running: false`, `bound_addr: None`. Duplicate name =
  `Conflict`. Returns new global generation.
- `create_proxy` (`runtime.rs:1248–1320`): validate, duplicate check, **bind
  before visible success** (`TcpListener::bind`), resolve `local_addr`
  (ephemeral port 0 resolution), `init_policies`, force `enabled = true`,
  `spawn_supervisor`, insert. Bind failure = `BindFailed`, no ghost proxy
  (tested by `create_bind_conflict_leaves_no_ghost_proxy`). Creation race
  after bind cancels the orphan supervisor, joins its done-flag, untracks,
  returns `Conflict`.
- `start_all` (`runtime.rs:1220–1242`) / `start_stored`
  (`runtime.rs:1333–1367`): bind + `local_addr` + supervisor for every
  enabled-but-not-running stored definition. Vanished-while-binding path
  stops the orphan and returns `NotFound`.
- `set_enabled` (`runtime.rs:1408–1445`): disable stops the listener,
  terminates active connections (old scope cancellation), retains definition
  and fault plans (`running = false`, `bound_addr = None`,
  `enabled = false`, fresh child token); enable rebinds via `start_stored`.
  Idempotent when already in the desired state.
- `delete_proxy` (`runtime.rs:1389–1403`): remove definition, cancel scope,
  join supervisor done-flag, untrack. Only after the transition commits is
  success reported. Supervisor purges stragglers with `ProxyRemoved` (or
  `ServiceShutdown`) before its done-flag resolves.
- `update_proxy` (`runtime.rs:1453–1588`): applies `enabled` first, validates
  `connect_timeout_ms != 0`, then splits:
  - live/non-restart (or stopped proxy): swap spec in place, **preserving
    live policies** (fault plans untouched), new global generation;
  - restart-class (`listen` / `upstream` changed on a running proxy):
    **pre-bind the replacement**; bind failure keeps the old listener
    serving with spec untouched and returns `RestartFailed`. Then stop old,
    join, untrack, spawn supervisor on the prebound listener, preserve live
    policies, `running = true`, `bound_addr = Some(new)`.
  - `max_connections` / `connect_timeout` apply to **new** connections only.
- Supervision: `spawn_supervisor` (`runtime.rs:1369–1384`) pushes
  `(name, JoinHandle)` onto owned `root`; `untrack` (`runtime.rs:1325–1331`)
  drops it only after the done-flag confirms completion. `SupervisorDone` +
  `DoneGuard` (`runtime.rs:790–823`) is level-triggered: flag set on drop so
  even a panicking supervisor releases waiters. `proxy_supervisor`
  (`runtime.rs:2302–2346`) re-reads the entry every accept (updates apply to
  new connections), selects over scope/service cancellation vs
  `listener.accept()`, then `children.shutdown()` + `purge_proxy_connections`
  before resolving the done-flag. `ManagedProxy` (`runtime.rs:825–847`) holds
  spec, running/bound state, cancel token, done-flag, per-proxy `active` and
  `ordinal` counters.
- `bound_addresses()` (`runtime.rs:2141–2149`, `ServiceHandle::bound_addresses`
  at `2267–2269`): actual bound addresses for running proxies; port-0
  listeners report their resolved port (tested by
  `create_with_port_zero_reports_actual_bound_address`).

## Upstream dial and admission

- `run_connection` (`runtime.rs:2723–2885`): dial is
  `timeout(connect_timeout, TcpStream::connect(upstream))` with durable
  cancellation — a kill issued before the relay waits is still observed.
  Dial error → `ConnectFailed("dial: …")`; timeout → `ConnectFailed("dial
  timed out after …")`; cancel during connect → `KilledByOperator` /
  `ProxyRemoved` / `ServiceShutdown` via `classify_cancel`, recorded with
  detail `"cancelled during upstream connect"`. On success the snapshot flips
  `Connecting → Relaying` (`runtime.rs:2769–2771`).
- `accept_connection` (`runtime.rs:2349–2444`): increments global + per-proxy
  active counters, then rejects when `current > limits.global_connections`
  or `current_proxy > spec.max_connections`. Rejections decrement both
  counters and bump `metrics.rejected` — no record, no ID consumed. Accepted
  connections bump `metrics.accepted` + `record_accept`, consume
  `next_conn_id` / per-proxy `ordinal` (deterministic `connection_key`),
  snapshot accept-time policy generations/namespaces from **one atomic
  snapshot per direction**, register `Connecting` + cancellation token, and
  spawn `run_connection` as a child of the proxy supervisor.
- `kill(id)` (`runtime.rs:1174–1182`): removes and cancels the per-connection
  token; level-triggered so a pre-relay kill still fires. Returns true only
  when the ID was active; repeat kills report absence.
- Shutdown cascade: `initiate_shutdown` (`runtime.rs:2116–2121`) cancels the
  service token (child of every proxy/connection/scenario scope);
  `shutdown_and_join` (`runtime.rs:2127–2138`) joins all tracked supervisors
  then all scenario tasks. Idempotent.

## `eggress-relay` embedding

`eggress-relay` remains the bidirectional relay authority; eggchaos never
forks its half-close/copy semantics (`docs/architecture.md:17–20`).

- Import: `use egress_relay::{HalfClosePolicy, RelayOptions}`
  (`runtime.rs:18`); options built per connection (`runtime.rs:2831–2834`)
  from `RuntimeParams.half_close` (default `Drain`) and bounded
  `relay_buffer` (default 64 KiB).
- Chaos wrapping (`runtime.rs:2772–2830`): each accepted socket is first
  wrapped in `ResettableTcpStream` (resettable edge), then in
  `ChaosStream::new_live(wrapped, policy.clone(), proxy_name, key,
  direction)` — client side compiles the **downstream** policy, upstream
  side the **upstream** policy, from the accepted atomic snapshots so seed
  namespaces and plans agree with the accepted generation. Engine-compile
  failure → `RelayError("… engine: …")`. Live `StreamEvidence` handles are
  registered in `evidence` **before** relaying so snapshots report observed
  generations/counters/active faults from the start.
- Relay future: `egress_relay::relay_with_options(client_chaos,
  upstream_chaos, options)` (`runtime.rs:2835–2839`). The connection task
  selects (biased) over `conn_token.cancelled()`, relay completion,
  `term_upstream.terminated()`, `term_downstream.terminated()`
  (`runtime.rs:2841–2884`).
- Termination mapping (`handle_termination`, `runtime.rs:2888–2989`):
  - `Graceful`: wait `timeout(term_grace, relay)` so the accepted prefix
    drains; success or eggchaos-terminated relay error → `GracefulTermination
    { drained: true }`; unrelated relay failure → `RelayError("graceful
    termination due, then relay failed: …")`; grace expiry → abort with
    `GracefulTermination { drained: false }` (conservative abort note).
    Graceful termination fires **even with no further application writes**
    because the core handle is level-triggered (see `docs/architecture.md:
    53–68` for blackhole/limit-data/disconnect baselines).
  - `HardReset`: flag **both** sockets via `TcpResetHandle::request_reset`,
    drop the relay (wrappers apply abortive close on release), read truthful
    per-socket outcomes → `HardReset { client, upstream }` with
    `fault_id` detail.
- Relay-completed path records `RelayCompleted` with the relay report
  (`termination`, byte counts); eggchaos-terminated relay errors consult the
  durable handle: `HardReset` request but sockets already gone →
  `HardReset { Unsupported, Unsupported }` with explanatory detail; otherwise
  `GracefulTermination { drained: true }`. Unrelated IO → `RelayError`.
  `is_eggchaos_termination` (`runtime.rs:2654–2657`) matches
  `ConnectionAborted` + `"eggchaos stream terminated"` prefix.
- Abstract request vs platform capability: core publishes an abstract
  `TerminationRequest::HardReset`; only this edge maps it to TCP RST.
  Ordinary `poll_shutdown` is never advertised as RST
  (`docs/architecture.md:24–26`).

## Connection registry, snapshots, and evidence

- `ConnectionState` (`runtime.rs:165–172`): `Connecting` / `Relaying` /
  `Closed`.
- `ConnectionSnapshot` (`runtime.rs:181–238`): frozen accept-time identity
  (`id`, `proxy`, `ordinal`, `peer`, `upstream`, `connection_key`,
  `seed = service_seed ^ proxy_seed`, global `generation`,
  `accepted_*_generation/seed`) merged at read time with live stream state
  (`observed_*`, `pending_*`, namespaces, transitions, `DirectionBytes`,
  bounded `ActiveFault` lists + truncation flags, `rng_version`). No payload
  bytes anywhere (`docs/control-plane.md:50–55`).
- `DirectionBytes` (`runtime.rs:242–249`): `accepted` / `forwarded` /
  `discarded` per direction.
- `ConnectionEvidence` (`runtime.rs:253–258`): lock-shared `Arc<StreamEvidence>`
  pair (`upstream`, `downstream`) updated by the streams.
- `merge_evidence` (`runtime.rs:2670–2720`): preserves accept-time fields;
  live generations/namespaces/transitions/counters/faults come from the
  streams (`pending == 0` → `None`). `connections()` / `get_connection()`
  (`runtime.rs:1150–1164`) merge at read time; unregistered evidence falls
  back to accept-time values.
- `ConnectionOutcome` (`runtime.rs:275–302`) → `ClosedConnection`
  (`runtime.rs:306–317`): final `state == Closed` snapshot + outcome +
  per-direction `TerminationInfo` + short machine-safe `detail`.
  Eight outcomes: `RelayCompleted`, `ConnectFailed(String)`,
  `KilledByOperator`, `ServiceShutdown`, `ProxyRemoved`,
  `GracefulTermination { drained }`, `HardReset { client, upstream }`,
  `RelayError(String)`.
- Close accounting: `take_connection` (`runtime.rs:2450–2462`) removes
  record + token + evidence exactly once so concurrent finish/purge paths
  cannot double-count or underflow; `record_close` (`runtime.rs:2464–2507`)
  decrements global + per-proxy counters, merges final evidence **before**
  metrics/history, bumps `completed`, then FIFO-pushes history honoring the
  bound (skips retention when `history == 0`). `purge_proxy_connections`
  (`runtime.rs:2602–2652`) applies the same exactly-once path with
  `ProxyRemoved`/`ServiceShutdown` after supervisor abort.
- Metrics reconciliation: `aggregate_close_metrics`
  (`runtime.rs:2512–2596`) derives totals from final evidence so counters
  agree with history; `metrics_text` (`runtime.rs:1007–1114`) renders
  Prometheus text plus live gauges (active connections, policy generations,
  queued bytes summed from live `byte_counts`).

## Resettable transport edge

- `TcpResetHandle` (`runtime.rs:328–342`): shared `reset_on_close` flag +
  outcome slot; `request_reset()` / `outcome()`.
- `ResettableTcpStream` (`runtime.rs:346–456`): owns `Option<TcpStream>` +
  shared handle; `AsyncRead`/`AsyncWrite` delegate to the inner stream.
  `Drop` (`runtime.rs:365–395`): ordinary drop closes gracefully (FIN);
  when reset was requested, converts via `into_std()` → `socket2::Socket`
  → `set_linger(Some(ZERO))` before close (stable-API-only abortive close,
  no `unsafe`, no unstable `set_linger`). Records `Applied` / `Failed(reason)`.
- `ResetResult` (`runtime.rs:262–271`): `Applied` (abortive close initiated;
  wire-level RST observation remains platform-dependent, M013), `Failed(String)`,
  `Unsupported(String)` (no transport when the request resolved, e.g. relay
  already failed and sockets were gone).
- `ResetReport` (`runtime.rs:545–553`): `generation`, `reset: true`,
  `failed_enables: Vec<String>` (bind-failed proxies stay disabled with
  definitions retained).

## Metrics tables

- `MetricsCounters` (`runtime.rs:629–658`): `accepted` / `completed` /
  `rejected`, `outcomes: [AtomicU64; 8]`, `graceful_requests` /
  `hard_reset_requests`, `reset_applied` / `reset_unsupported` /
  `reset_failed`, `bytes_accepted` / `bytes_forwarded` / `bytes_discarded`,
  `transitions`, bounded `tables: StdMutex<MetricTables>`.
- `OUTCOME_CLASS_NAMES` (`runtime.rs:661–670`): eight coarse classes in
  counter order; `outcome_class()` (`runtime.rs:749–760`) maps each
  `ConnectionOutcome` to its index.
- `MAX_METRIC_PROXIES = 1024`, `MAX_METRIC_ACTIVATIONS = 8192`
  (`runtime.rs:673–675`).
- `PerProxyMetrics` (`runtime.rs:679–687`): accepted/completed + byte triples
  per direction (`[accepted, forwarded, discarded]`, 0 = upstream).
- `MetricTables` (`runtime.rs:692–746`): `proxies` + `overflow_proxy`,
  `activations[(proxy, direction, fault_type)]` + `overflow_activations`
  (saturating adds). Overflow series render as `_overflow` labels
  (`runtime.rs:1054–1072`). Labels never carry connection IDs, peer
  addresses, run IDs, arbitrary fault IDs, or hostnames
  (`runtime.rs:1003–1006`, `docs/control-plane.md:77–86`).

## `ControlState` authority, generations, conflicts, reset

`RuntimeInner` doc (`runtime.rs:849–853`): proxy definitions, listener
ownership, registries, bounded history, metrics, and supervision all live in
one authority; adapters never keep a parallel registry.

- Proxy mutations: `create_proxy`, `import_definition`, `start_all`,
  `set_enabled`, `delete_proxy`, `update_proxy` (above). Every committed
  mutation bumps the global generation (`next_generation`,
  `runtime.rs:1121–1123`; starts at 1, `generation()` at `1117–1119`).
  Concurrent creations/updates serialize into unique generations (tested by
  `concurrent_mutations_serialize_into_unique_generations`).
- Fault mutations (canonical plan + live policy updated together under one
  write lock; mirrors refresh from published snapshots, never locally built
  plans): `add_fault` (`runtime.rs:1623–1668`, per-direction unique IDs,
  `Conflict` on duplicate), `update_fault` (`runtime.rs:1672–1724`, order
  preserving, upstream-then-downstream search), `remove_fault`
  (`runtime.rs:1727–1754`), `get_fault` / `list_faults`
  (`runtime.rs:1758–1787`, live-snapshot reads), `publish_plans`
  (`runtime.rs:1792–1823`, seed namespaces retained). Fault ID validation in
  `build_fault_spec` (`runtime.rs:1590–1618`): 1..=128 bytes,
  `[A-Za-z0-9._-]+`, finite probability in `[0, 1]`.
- Generation tracking: global `generation` (config publication counter) plus
  per-direction `LivePolicy` generations. `ProxyView` pairs each plan with
  its generation + seed namespace from the same atomic snapshot
  (`runtime.rs:908–931`); connection snapshots carry accepted vs observed
  generations, namespaces, pending transition targets, and transition counts
  (`docs/control-plane.md:36–55`).
- `ExpectedPublish` (`runtime.rs:943–956`): both replacement plans + both
  seed namespaces + both expected generations. `publish_plans_expected`
  (`runtime.rs:1830–1878`) and `publish_direction_expected`
  (`runtime.rs:1888–1919`, single-direction variant so an event touches
  exactly its target) fail fast with `Conflict` on stale base via
  `conflict_message` (`runtime.rs:2660–2665`:
  `"proxy {p} policy moved from generation {e} to {f}…"`) instead of silently
  overwriting. Scenario events use these paths; manual publications use the
  unconditional paths and retain namespaces. `snapshot_policies`
  (`runtime.rs:1923–1936`) atomically snapshots both directions as the event
  base.
- `reset()` (`runtime.rs:2062–2111`): retains definitions and
  listen/upstream addresses, enables every proxy, replaces **all** fault
  plans with empty plans (seed namespaces retained), cancels active
  connections via level-triggered tokens, rebinds stopped proxies via
  `start_stored`, reports bind failures in `failed_enables`. Exactly one
  global generation covers the transaction
  (`report.generation == before + 1`, tested by
  `reset_empties_faults_and_enables_proxies`).
- Scenario supervision (kept here because `ControlState` owns it):
  `MAX_SCENARIO_RUNS = 32` (`runtime.rs:1881`); `start_scenario`
  (`runtime.rs:1941–2009`) validates upfront, fails fast at the active cap,
  prunes oldest **finished** runs FIFO, spawns into the owned `JoinSet` with
  a service-child token; `cancel_scenario` / `update_scenario_run` /
  `remove_scenario_token` (`runtime.rs:2024–2056`); shutdown cancels and
  joins all runs (`runtime.rs:2127–2138`).

## Bounded-ness (every limit and where enforced)

| Bound | Value | Enforcement |
| --- | --- | --- |
| Proxy name | 1..=128 bytes, `[A-Za-z0-9._-]+` | `ProxySpec::validate` (`runtime.rs:104–120`) |
| Fault ID | 1..=128 bytes, `[A-Za-z0-9._-]+` | `build_fault_spec` (`runtime.rs:1591–1605`); probability finite in `[0,1]` (`1596–1618`, `1679–1684`) |
| Global active connections | `limits.global_connections`, default 1024, non-zero required at build | `accept_connection` over-global check + `rejected` counter (`runtime.rs:2358–2368`); `ServiceBuilder::build` (`2213–2217`) |
| Per-proxy active connections | `spec.max_connections: Option<usize>` | `accept_connection` over-proxy check (`runtime.rs:2359–2368`); patchable live, applies to new connections |
| Closed-connection history | `limits.history`, default 256; `0` disables retention | `record_close` + `purge_proxy_connections` FIFO `pop_front` while `len >= limit` (`runtime.rs:2493–2506`, `2637–2651`) |
| Metric proxy table | `MAX_METRIC_PROXIES = 1024` | `MetricTables::proxy_entry` spills to `overflow_proxy` (`runtime.rs:699–708`) |
| Metric activation series | `MAX_METRIC_ACTIVATIONS = 8192` | `record_activations` spills counts to `overflow_activations` (`runtime.rs:725–745`) |
| Scenario runs retained | `MAX_SCENARIO_RUNS = 32` | Active cap fails fast; finished runs pruned FIFO (`runtime.rs:1959–1992`) |
| Scenario events per doc | ≤ 1024 | `validate_scenario` (`scenario.rs:112–116`) |
| Config proxies per file | ≤ 1024 | `NativeConfig::parse` (`config.rs:138–143`) |
| Admin request body | 1 MiB (`max_request_body_bytes` + `Buffer{max_bytes}`) | `NativeAdmin::start` (`admin.rs:91–107`); `docs/control-plane.md:3–7` |
| Admin connections | 128 | `RuntimeConfig{ max_connections: 128 }` (`admin.rs:91–95`) |
| Relay copy buffer | 64 KiB default (`NonZeroUsize`) | `RuntimeParams::default` + `ServiceBuilder::new` (`runtime.rs:784`, `2170`); passed as `RelayOptions.buffer_size` per connection |
| Graceful-drain grace | 5 s default (`term_grace`) | `RuntimeParams::default` / `termination_grace()` (`runtime.rs:785`, `2200–2203`); bounds `handle_termination` wait; expiry records `drained: false` |
| Upstream connect timeout | 5 s default, non-zero required | `ProxySpec::new` / `validate` (`runtime.rs:95`, `127–131`); `ProxyPatch::connect_timeout_ms` rejects 0 (`1461–1467`); enforced by `timeout()` in `run_connection` |
| Fault buffers (config path) | latency `max_buffer_bytes` 64 KiB; bandwidth `burst_bytes` 64 KiB default; slicer `average_size` 1024 default | `ProxyFileConfig`/`FaultFileConfig::compile` (`config.rs:220–264`) |
| Evidence fault lists | Bounded + truncation flags | `upstream/downstream_faults_truncated` (`runtime.rs:230–235`); core `active_faults()` bound merged in `merge_evidence` (`2715–2717`) |
| Metric labels | Fixed vocabularies only | No conn ID / peer / run ID / fault ID / hostname labels (`runtime.rs:1003–1006`); proxy/direction/fault-type tables bounded above |
| Payload capture | None anywhere | Snapshots/evidence/metrics/history record counters, identities, generations — never payload bytes (`runtime.rs:179`, `docs/control-plane.md:50–55`) |

Core queue bounds (latency/bandwidth `max_buffer_bytes`, per-write caps) are
owned by `eggchaos-core` and covered in [core fault engine](core-fault-engine.md).

## Review checklist (file by file)

- `crates/eggchaos-server/src/lib.rs` (26 lines): re-export surface matches
  the required contract (`ProxySpec/View/Patch`, `AdmissionLimits`,
  `RuntimeParams`, registry, reset, metrics, `ServiceBuilder/Service/Handle`,
  `ControlState`, `ExpectedPublish`, fault patch types, `MAX_METRIC_*`,
  `OUTCOME_CLASS_NAMES`, `VERSION`); `deny(unsafe_code)`; module split
  `admin` / `config` / `runtime` / `scenario`.
- `crates/eggchaos-server/src/runtime.rs`:
  - Types: `ProxySpec` + validate + `init_policies`; `ProxyView` snapshot
    atomicity; `ProxyPatch` restart-class docs; `FaultUpsert/FaultPatch`;
    `AdmissionLimits/RuntimeParams` defaults; `ConnectionSnapshot/State`,
    `DirectionBytes`, `ConnectionEvidence`, `ConnectionOutcome`,
    `ClosedConnection`; `ResettableTcpStream/TcpResetHandle/ResetReport/
    ResetResult`; `MetricTables/MetricsCounters/PerProxyMetrics`,
    `OUTCOME_CLASS_NAMES`, `MAX_METRIC_*`, `outcome_class`.
  - Lifecycle: bind-before-success, ephemeral resolution, orphan-supervisor
    cleanup, restart pre-bind + rollback, enable/disable retention,
    done-flag + untrack discipline, per-accept entry re-read.
  - Data path: admission counters, dial timeout + durable cancellation,
    chaos wrapping order, evidence registration, biased select, graceful vs
    hard-reset mapping, exactly-once close accounting, FIFO history,
    reconciling metrics.
  - Authority: single-lock plan+policy publication, global + per-direction
    generations, `ExpectedPublish` conflicts, one-generation `reset`.
  - Tests (`runtime.rs:2991–4757`): ephemeral relay/drain, per-proxy vs
    global limits, bound-address resolution, bind-conflict ghost check,
    delete/disconnect lifecycle, disable/enable plan retention, upstream
    redirect, failed-restart rollback, generation uniqueness, fault CRUD
    sync, live-traffic fault engage/remove, reset semantics, kill cleanup,
    history bound, hard-reset outcomes, metrics label boundedness. Timing
    assertions use Tokio time with tolerance windows, not bare wall-clock
    equality.
- `crates/eggchaos-server/Cargo.toml`: minimal Eggstack picks —
  `eggress-relay` (not `eggress-embed`) for byte relay,
  `eggserve-server + eggserve-primitives` (not `eggserve-core`) for H1 admin;
  `socket2` for stable linger; no `eggress-outbound`; dev-deps
  `eggfetch-core` + `proptest` only.
- `docs/architecture.md`: dependency direction, bounded-release statement,
  abstract-reset vs shutdown distinction, M009 fault baselines (latency burst
  drain, token-bucket bandwidth, blackhole/limit-data graceful termination
  without further writes, disconnect `hard_reset` flag, generation-swap drain
  semantics) — runtime behavior must match these.
- `docs/control-plane.md`: loopback default + public opt-in/auth, 1 MiB body
  cap, route inventory, generation/snapshot pairing rule, stale-base
  conflict, scenario seed-namespace + replay limits, low-cardinality metrics
  vocabulary — all implemented in `runtime.rs`/`admin.rs` above.

## Verification

```sh
cargo test -p eggchaos-server
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --check
```

Full workspace closure additionally expects `cargo test --workspace
--all-features` plus fmt/clippy clean and dependency/security checks per
`AGENTS.md`; timing-sensitive tests must keep justified tolerance windows,
and any platform/oracle evidence that cannot run (e.g. Linux/macOS/Windows
reset semantics, Toxiproxy differential) must be recorded as incomplete, not
replaced by inspection.

## Canonical references

- [Architecture overview](overview.md) (index; this file is §2 server runtime).
- `docs/architecture.md` — dependency direction, M009 semantics.
- `docs/control-plane.md` — generations, snapshots, scenarios, metrics.
- `plans/adrs/001-stream-fault-engine-boundary.md`,
  `plans/adrs/002-determinism-and-live-mutation.md` — relay authority and
  determinism boundaries.
- `plans/reference/verification-matrix.md`,
  `plans/reference/toxiproxy-parity.md` — verification + parity contracts.

## Fixed-target UDP runtime (M021, M024, M025)

`runtime/datagram/` (`mod.rs` composition/re-exports; `model.rs` validated
configuration, views, and evidence; `registry.rs` the single
`DatagramRuntime` control authority; `association.rs` setup, worker loop,
and teardown; `supervisor.rs` listener loop and admission; `tests.rs` the
runtime regression suite) is an independent Tokio UDP owner alongside the
TCP `ControlState`; it does not branch the stream registry or relay. A
`DatagramRuntime` binds before publishing a proxy and maps each client socket
address to one bounded association with its own connected upstream socket,
upstream/downstream `DatagramDirectionEngine`s, cancellation token, and
evidence record. This preserves reply ownership across multiple responses and
unsolicited target pushes. Listener disable/delete/shutdown joins the listener
and association tasks; explicit administrative cancellation records queued
datagram discards. Idle expiry checks both direction queues and pre-engine
ingress, so a delayed item keeps its association alive.

The UDP listener reads into a 65,536-byte buffer before applying the configured
logical datagram bound, making oversized input an observable drop instead of
an accepted truncated prefix. Association counts, ingress channel slots,
globally reserved ingress bytes, per-direction scheduler queues, and completed
history are bounded. IPv4 and IPv6 upstream sockets bind to the matching
unspecified family. M021 reuses no Eggress production dependency: the audited
published `eggress-udp` surface is routing/SOCKS-oriented and does not expose
the required generic fixed-target association owner.

M024 setup no longer holds the association registry lock across UDP
bind/connect. M025 makes the setup state machine explicit: each client
address is `Absent`, `Starting(reservation)`, or `Active(association)`, and a
reservation carries a monotonic identity, a retained/versioned Tokio `watch`
transition, and an idempotent global/per-proxy capacity lease. Exactly one
racing creator owns setup; bind/connect runs without the registry lock. A
waiter subscribes to the reservation's watch receiver while still holding
that lock, then awaits a terminal `Published` or `Abandoned` state with
`wait_for`. Publication, setup failure, and administrative drain change the
slot and publish the terminal state under the same lock. If a transition wins
before subscription, the map is no longer `Starting`; if it wins after
subscription, the retained watch state makes `wait_for` return immediately or
wakes the receiver. This is the no-lost-wakeup ordering, with no polling or
retry bound.

Retryable abandonment wakes same-client waiters so one can reserve the absent
slot with a new identity; terminal drains wake them to a conflict. A stale
setup owner can publish or release only its own reservation, never a newer
slot, and each capacity lease is released exactly once on failure, drain,
abandonment, or active teardown. The registry lock is not held across worker
joins either, and unpublished workers are cancellation-safe. Idle reaping
removes a slot only if it still holds the same expired association.

The association worker forwards `Immediate` admissions without entering the
deadline heap, batches egress accounting into one record update per direction
per drain, and refreshes direction evidence snapshots only when admission,
emission, or error state changed. Egress/send-error/administrative-discard
categories are unchanged.
