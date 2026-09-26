# Fixed-target server runtime

Back to [architecture overview](overview.md) (§2 server runtime).

`eggchaos-server` is the fixed-target TCP + UDP runtime. It owns TCP
listeners, UDP listeners/associations, upstream dials, admission, the bounded
connection/association registries, `eggress-relay` embedding, reset semantics,
metrics tables, and the single control mutation authorities (`ControlState`
for TCP + datagram orchestration, `DatagramRuntime` for UDP lifecycle). The
fault state machines themselves live in `eggchaos-core`; this crate only
embeds `ChaosStream<T>` (TCP) and `DatagramDirectionEngine` (UDP) at the
transport edge. User-facing dependency direction and fault-semantics baseline
live in `docs/architecture.md` and `docs/control-plane.md`.

Verified at HEAD `e050fa2` (M041 close at `724b967`; TCP/UDP split-module
layout, not a `runtime.rs` monolith).

Sources (evidence-first): `crates/eggchaos-server/src/lib.rs` (60 lines),
`crates/eggchaos-server/src/runtime/mod.rs` (620 lines: composition +
`ProxySpec`/`RuntimeInner`/`ControlState`/`ServiceBuilder`/`ServiceHandle`),
`crates/eggchaos-server/src/runtime/model.rs` (360 lines: views, snapshots,
errors), `crates/eggchaos-server/src/runtime/control.rs` (1874 lines:
`ControlState` authority), `crates/eggchaos-server/src/runtime/connection.rs`
(581 lines: finalization, evidence merge, relay data path),
`crates/eggchaos-server/src/runtime/supervisor.rs` (151 lines: accept +
admission), `crates/eggchaos-server/src/runtime/transport.rs` (162 lines:
reset edge), `crates/eggchaos-server/src/runtime/metrics.rs` (167 lines:
tables/counters), `crates/eggchaos-server/src/runtime/tests.rs` (2051 lines:
TCP/runtime regression suite), `crates/eggchaos-server/src/runtime/datagram/`
(`mod.rs` 32 lines, `model.rs` 302 lines, `registry.rs` 613 lines,
`association.rs` 931 lines, `supervisor.rs` 193 lines, `tests.rs` 919 lines),
`crates/eggchaos-server/Cargo.toml`, `docs/architecture.md`,
`docs/control-plane.md`.

## Runtime module map (split-module reality)

There is no `runtime.rs` monolith. Composition lives in `runtime/mod.rs`:

- `runtime/mod.rs:28–46` — declares `connection` / `control` / `datagram` /
  `metrics` / `model` / `supervisor` / `transport`, re-exports the datagram,
  metrics, model, and transport surfaces.
- `runtime/mod.rs:55–171` — `ProxySpec` + `validate` + `init_policies`;
  `mod.rs:175–189` `AdmissionLimits`; `mod.rs:193–221` `RuntimeParams`
  (now includes `datagram_limits`); `mod.rs:50–53` hard caps
  (`MAX_CONNECTION_LIMIT`, `MAX_HISTORY_LIMIT`, `MAX_RELAY_BUFFER_BYTES`,
  `MAX_CONTROL_TIMEOUT_MS`).
- `runtime/mod.rs:228–280` — `SupervisorDone` / `DoneGuard`
  (level-triggered, drop-sets-flag) + `ManagedProxy`.
- `runtime/mod.rs:287–378` — `RuntimeInner` (single TCP store + owned
  `DatagramRuntime`) + `view_of` (atomic snapshot pairing).
- `runtime/mod.rs:382–410` — `ControlState` + `ExpectedPublish` + `Default`.
- `runtime/mod.rs:412–572` — `ServiceBuilder` → `EggchaosService::start` →
  `ServiceHandle`.
- `runtime/model.rs` — public DTOs/errors: `ConnectionState`,
  `ConnectionSnapshot`, `DirectionBytes`, `ConnectionEvidence`,
  `ConnectionOutcome`, `ClosedConnection`, `ProxyPatch`, `FaultUpsert`,
  `FaultPatch`, `DatagramFaultPatch`, `ProxyView`, `ResetReport`,
  `ControlError`, `EggchaosError`, `outcome_class`.
- `runtime/control.rs` — the entire `ControlState` authority: metrics text,
  TCP proxy lifecycle, TCP fault CRUD, generation guards, scenario V1 + V2
  supervision, reset, shutdown, plus datagram orchestration that delegates
  listener/association ownership to `DatagramRuntime`.
- `runtime/connection.rs` — connection finalization (`take_connection`,
  `record_close`, `purge_proxy_connections`), `aggregate_close_metrics`,
  `merge_evidence`, `run_connection` dial/relay/select, `handle_termination`.
- `runtime/supervisor.rs` — `proxy_supervisor` accept loop +
  `accept_connection` admission/registration.
- `runtime/transport.rs` — `TcpResetHandle`, `ResettableTcpStream`,
  `ResetResult`.
- `runtime/metrics.rs` — `MetricsCounters`, `OUTCOME_CLASS_NAMES`,
  `MAX_METRIC_*`, `PerProxyMetrics`, `MetricTables`.
- `runtime/tests.rs` — TCP/runtime regression suite (see checklist).
- `runtime/datagram/mod.rs` — UDP split overview + shared constants
  (`UDP_RECEIVE_BUFFER_BYTES = 65_536`, `MAX_DATAGRAM_PROXIES = 1024`,
  `MAX_DATAGRAM_ASSOCIATIONS = 65_536`, `MAX_ASSOCIATION_HISTORY = 65_536`,
  `MAX_INGRESS_QUEUE = 1024`, `MAX_INGRESS_BUFFER_BYTES = 1 GiB`,
  `MAX_IDLE_TIMEOUT = 24 h`).
- `runtime/datagram/model.rs` — `DatagramRuntimeLimits`,
  `DatagramProxySpec`, `DatagramProxyView`, `DatagramAssociationSnapshot`,
  `DatagramRuntimeError`, `ProxyCounters`, `AssociationRecord`.
- `runtime/datagram/registry.rs` — single `DatagramRuntime` authority
  (proxy/listener/association registry, fault-plan publication, history).
- `runtime/datagram/association.rs` — setup reservation/publication, worker
  loop, batched egress accounting, teardown.
- `runtime/datagram/supervisor.rs` — listener loop, client-datagram
  admission, idle reaping.
- `runtime/datagram/tests.rs` — UDP regression suite (see checklist).

The split preserves one `RuntimeInner` and one `ControlState` store; UDP adds
one `DatagramRuntime` owned by `RuntimeInner` (`mod.rs:289`).

## Role and construction chain

| Type | Path | Role |
| --- | --- | --- |
| `ServiceBuilder` | `runtime/mod.rs:412–536` | Validated builder: seed, TCP proxy list, UDP proxy list (`datagram_proxy`, `datagram_proxy_all` at `mod.rs:448–459`), `AdmissionLimits`, `HalfClosePolicy`, `relay_buffer`, `term_grace`, `datagram_limits`. `build()` rejects duplicate TCP names, duplicate UDP names, invalid proxies/plans, `global_connections == 0` or over `MAX_CONNECTION_LIMIT`, `history` over `MAX_HISTORY_LIMIT`, `relay_buffer` over 16 MiB, `term_grace` over 300 s, invalid datagram limits, and datagram definition count over `max_proxies` (`mod.rs:486–535`). |
| `EggchaosService` | `runtime/mod.rs:539–572` | Configured-but-not-started service. `start()` creates `ControlState::with_params`, then `create_proxy` for enabled TCP proxies and `import_definition` for disabled ones, then `create_datagram_proxy` for UDP definitions (UDP failure triggers `shutdown_and_join` and returns `EggchaosError::Io`). |
| `ServiceHandle` | `runtime/mod.rs:578–608` | Running-service handle: `bound_addresses()`, `connections()`, `history()`, `control_state()`, level-triggered `shutdown()` + `wait()` (`shutdown_and_join`). No listener/connection task is ever detached. |
| `ControlState` | `runtime/mod.rs:382–384`, `runtime/control.rs` (full impl) | Cloneable single mutation authority over `Arc<RuntimeInner>`. Native admin, CLI-via-HTTP, Toxiproxy adapter, scenario drivers, and `eggchaos-embed` all mutate through these typed methods. Stream fault add/get/list/patch/remove plus datagram fault add/get/list/patch/remove are HTTP-independent authorities here (duplicate/cross-direction conflicts, patch non-emptiness, plan reconstruction, generation-guarded publication); HTTP and embed only convert DTOs and map errors. `lib.rs:39–48` re-exports the full surface. |
| `RuntimeInner` | `runtime/mod.rs:287–352` | Single store: `params`, owned `datagrams: DatagramRuntime`, `proxies: RwLock<BTreeMap<String, ManagedProxy>>`, `generation` (starts at 1), `next_conn_id`, `active`, `connections`, `cancellations`, `evidence`, `history: VecDeque<ClosedConnection>`, `metrics`, shutdown token/flag, owned `root` supervisor handles, owned scenario `JoinSet`/records/tokens (V1 + V2 share the same `JoinSet`; V2 has a separate record/token map with the same `MAX_SCENARIO_RUNS` bound). |
| `RuntimeParams` | `runtime/mod.rs:193–221` | Per-service tuning: `seed`, `limits: AdmissionLimits`, `half_close: HalfClosePolicy` (default `Drain`), `relay_buffer` (default `NATIVE_DEFAULT_BUFFER_BYTES`), `term_grace` (default `NATIVE_DEFAULT_PROXY_TIMEOUT_MS`), `datagram_limits: DatagramRuntimeLimits`. |
| `AdmissionLimits` | `runtime/mod.rs:175–189` | `global_connections` (default 1024), `history` (default 256). |

Dependencies (`crates/eggchaos-server/Cargo.toml:15–31`, workspace
`Cargo.toml`): `eggchaos-core` + `eggchaos-experiment` + `eggchaos-protocol`
(path), `eggress-relay` (workspace; byte-relay authority),
`eggserve-server` + `eggserve-primitives` (admin substrate only),
`tokio` + `tokio-util` + `bytes`, `socket2` (stable-API abortive close),
`serde`/`serde_json`, `sha2`, `thiserror`, `toml`, `tracing`.
`#![deny(unsafe_code)]` (`lib.rs:3`). No `eggress-outbound` dependency
(optional/future only). Dev-deps are `eggfetch-core` + `proptest` only
(`Cargo.toml:33–35`).

## Proxy model

- `ProxySpec` (`mod.rs:57–87`): `name`, `listen` (port 0 = ephemeral),
  `upstream` (fixed target), `upstream_faults` / `downstream_faults`
  (`FaultPlan`), live `upstream_policy` / `downstream_policy` (`LivePolicy`,
  `serde(skip)`), `enabled` (default true), `max_connections: Option<usize>`,
  `connect_timeout: Duration` (millis serde, default
  `NATIVE_DEFAULT_PROXY_TIMEOUT_MS` via `new()` at `mod.rs:107–121`),
  `seed: u64`. `with_max_connections()` at `mod.rs:123–126`.
- `ProxySpec::validate` (`mod.rs:127–161`): name 1..=128 bytes and
  `[A-Za-z0-9._-]+` (single URL path segment), both plans valid, connect
  timeout non-zero and at most 300 s (`MAX_CONTROL_TIMEOUT_MS`).
- `init_policies(service_seed)` (`mod.rs:166–170`): namespace =
  `service_seed ^ proxy.seed`; both `LivePolicy::new(plan, namespace)`.
  Canonical state is the policy snapshot; plan fields are config-origin
  mirrors refreshed from published snapshots under the same lock.
- `ProxyView` (`model.rs:226–259`): operator-visible definition + actual
  runtime state: `listen`, `upstream`, `bound_addr: Option<SocketAddr>`,
  `running`, `enabled`, both plans **read from the same atomic snapshot as
  the reported generation/namespace** (`RuntimeInner::view_of`,
  `mod.rs:354–377`), `max_connections`, `connect_timeout_ms`, `seed`.
- `ProxyPatch` (`model.rs:169–181`): `listen` / `upstream` (restart-class),
  `enabled`, `max_connections: Option<Option<usize>>` (`None` = no change,
  `Some(None)` = clear cap), `connect_timeout_ms`. Fault plans are out of
  scope here — use fault CRUD.
- `FaultUpsert` (`model.rs:185–195`): `direction: Direction`, `id`
  (1..=128 bytes), `probability` (default 1.0), `kind: FaultKind`.
- `FaultPatch` (`model.rs:204–209`): optional `probability` / `kind` only;
  identity and direction are fixed (cross-direction move = delete + add).
- `DatagramFaultPatch` (`model.rs:215–220`): same shape for datagram kinds.
- Errors: `ControlError` (`model.rs:278–318`) with stable codes
  `not_found` / `conflict` / `invalid` / `bind_failed` / `restart_failed`;
  `EggchaosError` (`model.rs:322–346`) for build/bind/lifecycle/IO.

## Listener lifecycle

All paths go through `ControlState`; success reflects an actual listener,
never a map entry alone.

- `import_definition` (`control.rs:689–718`): validate, `init_policies`,
  store with `running: false`, `bound_addr: None`. Duplicate name =
  `Conflict`. Returns new global generation.
- `create_proxy` (`control.rs:749–821`): validate, duplicate check, **bind
  before visible success** (`TcpListener::bind`), resolve `local_addr`
  (ephemeral port 0 resolution), `init_policies`, force `enabled = true`,
  `spawn_supervisor`, insert. Bind failure = `BindFailed`, no ghost proxy
  (tested by `create_bind_conflict_leaves_no_ghost_proxy` at
  `tests.rs:236`). Creation race after bind cancels the orphan supervisor,
  joins its done-flag, untracks, returns `Conflict`.
- `start_all` (`control.rs:721–743`) / `start_stored`
  (`control.rs:834–868`): bind + `local_addr` + supervisor for every
  enabled-but-not-running stored definition. Vanished-while-binding path
  stops the orphan and returns `NotFound`.
- `set_enabled` (`control.rs:909–946`): disable stops the listener,
  terminates active connections (old scope cancellation), retains definition
  and fault plans (`running = false`, `bound_addr = None`,
  `enabled = false`, fresh child token); enable rebinds via `start_stored`.
  Idempotent when already in the desired state.
- `delete_proxy` (`control.rs:890–904`): remove definition, cancel scope,
  join supervisor done-flag, untrack. Only after the transition commits is
  success reported. Supervisor purges stragglers with `ProxyRemoved` (or
  `ServiceShutdown`) before its done-flag resolves.
- `update_proxy` (`control.rs:954–1089`): applies `enabled` first, validates
  `connect_timeout_ms != 0`, then splits:
  - live/non-restart (or stopped proxy): swap spec in place, **preserving
    live policies** (fault plans untouched), new global generation;
  - restart-class (`listen` / `upstream` changed on a running proxy):
    **pre-bind the replacement**; bind failure keeps the old listener
    serving with spec untouched and returns `RestartFailed`. Then stop old,
    join, untrack, spawn supervisor on the prebound listener, preserve live
    policies, `running = true`, `bound_addr = Some(new)`.
  - `max_connections` / `connect_timeout` apply to **new** connections only.
- Supervision: `spawn_supervisor` (`control.rs:870–885`) pushes
  `(name, JoinHandle)` onto owned `root`; `untrack` (`control.rs:826–832`)
  drops it only after the done-flag confirms completion. `SupervisorDone` +
  `DoneGuard` (`mod.rs:228–256`) is level-triggered: flag set on drop so
  even a panicking supervisor releases waiters. `proxy_supervisor`
  (`supervisor.rs:3–47`) re-reads the entry every accept (updates apply to
  new connections), selects over scope/service cancellation vs
  `listener.accept()`, then `children.shutdown()` + `purge_proxy_connections`
  before resolving the done-flag. `ManagedProxy` (`mod.rs:258–280`) holds
  spec, running/bound state, cancel token, done-flag, per-proxy `active` and
  `ordinal` counters.
- `bound_addresses()` (`control.rs:1865–1873`, `ServiceHandle::bound_addresses`
  at `mod.rs:584–586`): actual bound addresses for running proxies; port-0
  listeners report their resolved port (tested by
  `create_with_port_zero_reports_actual_bound_address` at `tests.rs:215`).

## Upstream dial and admission

- `run_connection` (`connection.rs:315–477`): dial is
  `timeout(connect_timeout, TcpStream::connect(upstream))` with durable
  cancellation — a kill issued before the relay waits is still observed.
  Dial error → `ConnectFailed("dial: …")`; timeout → `ConnectFailed("dial
  timed out after …")`; cancel during connect → `KilledByOperator` /
  `ProxyRemoved` / `ServiceShutdown` via `classify_cancel`, recorded with
  detail `"cancelled during upstream connect"`. On success the snapshot flips
  `Connecting → Relaying` (`connection.rs:361–363`).
- `accept_connection` (`supervisor.rs:50–151`): increments global + per-proxy
  active counters, then rejects when `current > limits.global_connections`
  or `current_proxy > spec.max_connections`. Rejections decrement both
  counters and bump `metrics.rejected` — no record, no ID consumed. Accepted
  connections bump `metrics.accepted` + `record_accept`, consume
  `next_conn_id` / per-proxy `ordinal` (deterministic `connection_key`),
  snapshot accept-time policy generations/namespaces from **one atomic
  snapshot per direction**, register `Connecting` + cancellation token, and
  spawn `run_connection` as a child of the proxy supervisor.
- `kill(id)` (`control.rs:675–683`): removes and cancels the per-connection
  token; level-triggered so a pre-relay kill still fires. Returns true only
  when the ID was active; repeat kills report absence.
- Shutdown cascade: `initiate_shutdown` (`control.rs:1657–1662`) cancels the
  service token (child of every proxy/connection/scenario scope);
  `shutdown_and_join` (`control.rs:1668–1680`) joins all tracked supervisors
  then all scenario tasks, then `datagrams.shutdown()`. Idempotent.

## `eggress-relay` embedding

`eggress-relay` remains the bidirectional relay authority; eggchaos never
forks its half-close/copy semantics (`docs/architecture.md:25–31`).

- Import: `use egress_relay::{HalfClosePolicy, RelayOptions}`
  (`mod.rs:17`); options built per connection (`connection.rs:423–426`)
  from `RuntimeParams.half_close` (default `Drain`) and bounded
  `relay_buffer` (default `NATIVE_DEFAULT_BUFFER_BYTES`).
- Chaos wrapping (`connection.rs:364–422`): each accepted socket is first
  wrapped in `ResettableTcpStream` (resettable edge), then in
  `ChaosStream::new_live(wrapped, policy.clone(), proxy_name, key,
  direction)` — client side compiles the **downstream** policy, upstream
  side the **upstream** policy, from the accepted atomic snapshots so seed
  namespaces and plans agree with the accepted generation. Engine-compile
  failure → `RelayError("… engine: …")`. Live `StreamEvidence` handles are
  registered in `evidence` **before** relaying so snapshots report observed
  generations/counters/active faults from the start.
- Relay future: `egress_relay::relay_with_options(client_chaos,
  upstream_chaos, options)` (`connection.rs:427–431`). The connection task
  selects (biased) over `conn_token.cancelled()`, relay completion,
  `term_upstream.terminated()`, `term_downstream.terminated()`
  (`connection.rs:433–476`).
- Termination mapping (`handle_termination`, `connection.rs:480–581`):
  - `Graceful`: wait `timeout(term_grace, relay)` so the accepted prefix
    drains; success or eggchaos-terminated relay error → `GracefulTermination
    { drained: true }`; unrelated relay failure → `RelayError("graceful
    termination due, then relay failed: …")`; grace expiry → abort with
    `GracefulTermination { drained: false }` (conservative abort note).
    Graceful termination fires **even with no further application writes**
    because the core handle is level-triggered (see `docs/architecture.md:
    62–77` for blackhole/limit-data/disconnect baselines).
  - `HardReset`: flag **both** sockets via `TcpResetHandle::request_reset`,
    drop the relay (wrappers apply abortive close on release), read truthful
    per-socket outcomes → `HardReset { client, upstream }` with
    `fault_id` detail.
- Relay-completed path records `RelayCompleted` with the relay report
  (`termination`, byte counts); eggchaos-terminated relay errors consult the
  durable handle: `HardReset` request but sockets already gone →
  `HardReset { Unsupported, Unsupported }` with explanatory detail; otherwise
  `GracefulTermination { drained: true }`. Unrelated IO → `RelayError`.
  `is_eggchaos_termination` (`connection.rs:224–227`) matches
  `ConnectionAborted` + `"eggchaos stream terminated"` prefix.
- Abstract request vs platform capability: core publishes an abstract
  `TerminationRequest::HardReset`; only this edge maps it to TCP RST.
  Ordinary `poll_shutdown` is never advertised as RST
  (`docs/architecture.md:33–37`).

## Connection registry, snapshots, and evidence

- `ConnectionState` (`model.rs:5–12`): `Connecting` / `Relaying` /
  `Closed`.
- `ConnectionSnapshot` (`model.rs:21–97`): frozen accept-time identity
  (`id`, `proxy`, `ordinal`, `peer`, `upstream`, `connection_key`,
  `seed = service_seed ^ proxy_seed`, global `generation`,
  `accepted_*_generation/seed`) merged at read time with live stream state
  (`observed_*`, `pending_*`, namespaces, transitions, `DirectionBytes`,
  bounded `ActiveFault` lists + truncation flags, six additive
  stream-loss counters, `rng_version`). No payload bytes anywhere
  (`docs/control-plane.md:111–115`).
- `DirectionBytes` (`model.rs:101–108`): `accepted` / `forwarded` /
  `discarded` per direction.
- `ConnectionEvidence` (`model.rs:112–117`): lock-shared `Arc<StreamEvidence>`
  pair (`upstream`, `downstream`) updated by the streams.
- `merge_evidence` (`connection.rs:240–312`): preserves accept-time fields;
  live generations/namespaces/transitions/counters/faults/stream-loss come
  from the streams (`pending == 0` → `None`). `connections()` /
  `get_connection()` (`control.rs:651–665`) merge at read time; unregistered
  evidence falls back to accept-time values.
- `ConnectionOutcome` (`model.rs:121–148`) → `ClosedConnection`
  (`model.rs:152–163`): final `state == Closed` snapshot + outcome +
  per-direction `TerminationInfo` + short machine-safe `detail`.
  Eight outcomes: `RelayCompleted`, `ConnectFailed(String)`,
  `KilledByOperator`, `ServiceShutdown`, `ProxyRemoved`,
  `GracefulTermination { drained }`, `HardReset { client, upstream }`,
  `RelayError(String)`.
- Close accounting: `take_connection` (`connection.rs:7–19`) removes
  record + token + evidence exactly once so concurrent finish/purge paths
  cannot double-count or underflow; `record_close` (`connection.rs:21–64`)
  decrements global + per-proxy counters, merges final evidence **before**
  metrics/history, bumps `completed`, then FIFO-pushes history honoring the
  bound (skips retention when `history == 0`). `purge_proxy_connections`
  (`connection.rs:172–222`) applies the same exactly-once path with
  `ProxyRemoved`/`ServiceShutdown` after supervisor abort.
- Metrics reconciliation: `aggregate_close_metrics`
  (`connection.rs:69–166`) derives totals from final evidence so counters
  agree with history, including one `record_stream_loss` per close;
  `metrics_text` (`control.rs:62–292`) renders Prometheus text plus live
  gauges (active connections, policy generations, queued bytes summed from
  live `byte_counts`).

## Resettable transport edge

- `TcpResetHandle` (`transport.rs:34–48`): shared `reset_on_close` flag +
  outcome slot; `request_reset()` / `outcome()`.
- `ResettableTcpStream` (`transport.rs:52–69`): owns `Option<TcpStream>` +
  shared handle; `AsyncRead`/`AsyncWrite` delegate to the inner stream
  (`transport.rs:110–162`).
  `Drop` (`transport.rs:71–102`): ordinary drop closes gracefully (FIN);
  when reset was requested, converts via `into_std()` → `socket2::Socket`
  → `set_linger(Some(ZERO))` before close (stable-API-only abortive close,
  no `unsafe`, no unstable `set_linger`). Records `Applied` / `Failed(reason)`.
- `ResetResult` (`transport.rs:14–23`): `Applied` (abortive close initiated;
  wire-level RST observation remains platform-dependent), `Failed(String)`,
  `Unsupported(String)` (no transport when the request resolved, e.g. relay
  already failed and sockets were gone).
- `ResetReport` (`model.rs:263–274`): `generation`, `reset: true`,
  `failed_enables: Vec<String>` (bind-failed TCP proxies stay disabled with
  definitions retained), `failed_datagram_enables: Vec<String>` (UDP
  equivalent, defaulted for backward-compatible deserialization).

## Metrics tables (incl. M041 stream-loss closure)

- `MetricsCounters` (`metrics.rs:9–44`): `accepted` / `completed` /
  `rejected`, `outcomes: [AtomicU64; 8]`, `graceful_requests` /
  `hard_reset_requests`, `reset_applied` / `reset_unsupported` /
  `reset_failed`, `bytes_accepted` / `bytes_forwarded` / `bytes_discarded`,
  `transitions`, `schedule_v2_runs` / `schedule_v2_events` /
  `schedule_v2_late_events`, bounded `tables: StdMutex<MetricTables>`.
- `OUTCOME_CLASS_NAMES` (`metrics.rs:47–56`): eight coarse classes in
  counter order; `outcome_class()` (`model.rs:350–360`) maps each
  `ConnectionOutcome` to its index.
- `MAX_METRIC_PROXIES = 1024`, `MAX_METRIC_ACTIVATIONS = 8192`
  (`metrics.rs:59–61`).
- `PerProxyMetrics` (`metrics.rs:65–75`): accepted/completed + byte triples
  per direction (`[accepted, forwarded, discarded]`, 0 = upstream) +
  stream-loss triples per direction (`[evaluated, dropped, bytes]`).
- `MetricTables` (`metrics.rs:80–144`): `proxies` + `overflow_proxy`,
  `activations[(proxy, direction, fault_type)]` + `overflow_activations`
  (saturating adds). `record_stream_loss` (`metrics.rs:113–121`) is additive
  and disjoint from the frozen seven-slot `record_activations` vocabulary.
  Overflow series render as `_overflow` labels
  (`control.rs:139–162`). Labels never carry connection IDs, peer
  addresses, run IDs, arbitrary fault IDs, or hostnames
  (`control.rs:58–61`, `docs/control-plane.md:177–188`).
- M041 exposition (`control.rs:95–162`): the three
  `eggchaos_stream_loss_*` HELP/TYPE headers plus per-proxy/direction
  samples are emitted exactly once with real newlines (also for the
  `_overflow` bucket); unit-covered by
  `stream_loss_prometheus_metrics_count_final_evidence_once` and
  `stream_loss_prometheus_exposition_is_unique_and_well_formed`
  (`tests.rs:1799`, `tests.rs:1848`).

## `ControlState` authority, generations, conflicts, reset

`RuntimeInner` doc (`mod.rs:282–286`): proxy definitions, listener
ownership, registries, bounded history, metrics, and supervision all live in
one authority; adapters never keep a parallel registry.

- Proxy mutations: `create_proxy`, `import_definition`, `start_all`,
  `set_enabled`, `delete_proxy`, `update_proxy` (above). Every committed
  mutation bumps the global generation (`next_generation`,
  `control.rs:622–624`; starts at 1, `generation()` at `618–620`).
  Concurrent creations/updates serialize into unique generations (tested by
  `concurrent_mutations_serialize_into_unique_generations` at
  `tests.rs:393`).
- Stream fault mutations (canonical plan + live policy updated together
  under one write lock; mirrors refresh from published snapshots, never
  locally built plans): `add_fault` (`control.rs:1114–1159`, per-direction
  unique IDs, `Conflict` on duplicate), `update_fault`
  (`control.rs:1163–1215`, order preserving, upstream-then-downstream
  search), `remove_fault` (`control.rs:1218–1245`), `get_fault` /
  `list_faults` (`control.rs:1249–1278`, live-snapshot reads),
  `publish_plans` (`control.rs:1283–1314`, seed namespaces retained).
  Fault ID validation in `build_fault_spec` (`control.rs:1091–1109`):
  1..=128 bytes, finite probability in `[0, 1]` (charset enforced by core
  `FaultId`).
- Datagram fault authority (same `ControlState`, HTTP-independent, M035
  shared with embed): `add_datagram_fault` (`control.rs:440–478`,
  same-direction duplicate + cross-direction uniqueness for path lookup),
  `get_datagram_fault` / `list_datagram_faults` (`control.rs:482–514`,
  upstream-then-downstream live reads), `update_datagram_fault`
  (`control.rs:519–571`, empty-patch rejection + probability range at this
  semantic layer), `remove_datagram_fault` (`control.rs:575–600`).
  Generation-guarded publication flows through `publish_datagram_plan`
  (`control.rs:414–430`) into `DatagramRuntime::publish_fault_plan`.
- Generation tracking: global `generation` (config publication counter) plus
  per-direction `LivePolicy` generations. `ProxyView` pairs each plan with
  its generation + seed namespace from the same atomic snapshot
  (`mod.rs:354–377`); connection snapshots carry accepted vs observed
  generations, namespaces, pending transition targets, and transition counts
  (`docs/control-plane.md:96–115`).
- `ExpectedPublish` (`mod.rs:389–402`): both replacement plans + both
  seed namespaces + both expected generations. `publish_plans_expected`
  (`control.rs:1321–1369`) and `publish_direction_expected`
  (`control.rs:1379–1410`, single-direction variant so an event touches
  exactly its target) fail fast with `Conflict` on stale base via
  `conflict_message` (`connection.rs:230–235`:
  `"proxy {p} policy moved from generation {e} to {f}…"`) instead of silently
  overwriting. Scenario events use these paths; manual publications use the
  unconditional paths and retain namespaces. `snapshot_policies`
  (`control.rs:1414–1427`) atomically snapshots both directions as the event
  base.
- `reset()` (`control.rs:1553–1652`): retains definitions and
  listen/upstream addresses, enables every TCP proxy, replaces **all** TCP
  fault plans with empty plans (seed namespaces retained), cancels active
  connections via level-triggered tokens, rebinds stopped proxies via
  `start_stored`, reports TCP bind failures in `failed_enables`; then
  terminates every datagram association, clears both directional datagram
  plans (generation-guarded), re-enables stopped datagram listeners, and
  reports UDP bind failures in `failed_datagram_enables`. Exactly one
  global generation covers the transaction (tested by
  `reset_empties_faults_and_enables_proxies` at `tests.rs:584`).
- Scenario supervision (kept here because `ControlState` owns it):
  `MAX_SCENARIO_RUNS = 32` (`control.rs:1372`, shared cap for V1 and V2);
  `start_scenario` (`control.rs:1432–1500`) validates upfront, fails fast at
  the active cap, prunes oldest **finished** runs FIFO, spawns into the owned
  `JoinSet` with a service-child token; `cancel_scenario` /
  `update_scenario_run` / `remove_scenario_token`
  (`control.rs:1515–1547`); `start_schedule_v2` / `get_schedule_v2` /
  `cancel_schedule_v2` / `append_schedule_v2_event`
  (`control.rs:1709–1862`, same cap/JoinSet/token discipline, coarse
  `schedule_v2_*` counters without run/fingerprint labels); shutdown cancels
  and joins all runs (`control.rs:1668–1680`).

## Bounded-ness (every limit and where enforced)

| Bound | Value | Enforcement |
| --- | --- | --- |
| Proxy name | 1..=128 bytes, `[A-Za-z0-9._-]+` | `ProxySpec::validate` (`mod.rs:127–143`); datagram equivalent `DatagramProxySpec::validate` (`datagram/model.rs:130–141`) |
| Fault ID | 1..=128 bytes | `build_fault_spec` (`control.rs:1091–1109`); probability finite in `[0,1]` (`control.rs:1097–1100`, `1169–1174`); datagram patch non-emptiness + range at `control.rs:525–536` |
| Global active connections | `limits.global_connections`, default 1024, `1..=1_000_000` required at build | `accept_connection` over-global check + `rejected` counter (`supervisor.rs:59–68`); `ServiceBuilder::build` (`mod.rs:500–507`) |
| Per-proxy active connections | `spec.max_connections: Option<usize>` | `accept_connection` over-proxy check (`supervisor.rs:60–68`); patchable live, applies to new connections |
| Closed-connection history | `limits.history`, default 256, `0..=1_000_000` (`0` disables retention) | `record_close` + `purge_proxy_connections` FIFO `pop_front` while `len >= limit` (`connection.rs:50–63`, `210–220`); retention skip keeps metrics (`tests.rs` `history_bound_zero_*`) |
| Metric proxy table | `MAX_METRIC_PROXIES = 1024` | `MetricTables::proxy_entry` spills to `overflow_proxy` (`metrics.rs:88–96`) |
| Metric activation series | `MAX_METRIC_ACTIVATIONS = 8192` | `record_activations` spills counts to `overflow_activations` (`metrics.rs:123–143`) |
| Scenario runs retained | `MAX_SCENARIO_RUNS = 32` (V1 + V2) | Active cap fails fast; finished runs pruned FIFO (`control.rs:1446–1483`, `1732–1766`) |
| Scenario events per doc | ≤ 1024 | `validate_scenario` (`scenario.rs:112–116`) |
| Config proxies per file | ≤ 1024 | `NativeConfig::parse` (`config.rs:138–143`) |
| Admin request body | 1 MiB (`max_request_body_bytes` + `Buffer{max_bytes}`) | `NativeAdmin::start` (`admin.rs:91–107`); `docs/control-plane.md:3–7` |
| Admin connections | 128 | `RuntimeConfig{ max_connections: 128 }` (`admin.rs:91–95`) |
| Relay copy buffer | 64 KiB default (`NonZeroUsize`), ≤ 16 MiB | `RuntimeParams::default` + `ServiceBuilder::new` (`mod.rs:215–216`, `431–432`); `build` cap at `mod.rs:508–514`; passed as `RelayOptions.buffer_size` per connection |
| Graceful-drain grace | 5 s default (`term_grace`), ≤ 300 s | `RuntimeParams::default` / `termination_grace()` (`mod.rs:217`, `477–479`); bounds `handle_termination` wait (`connection.rs:497`); expiry records `drained: false` |
| Upstream connect timeout | 5 s default, non-zero and ≤ 300 s | `ProxySpec::new` / `validate` (`mod.rs:118`, `150–159`); `ProxyPatch::connect_timeout_ms` rejects 0 (`control.rs:962–968`); enforced by `timeout()` in `run_connection` |
| Fault buffers (config path) | latency `max_buffer_bytes` 64 KiB; bandwidth `burst_bytes` 64 KiB default; slicer `average_size` 1024 default | `ProxyFileConfig`/`FaultFileConfig::compile` (`config.rs:220–264`) |
| Evidence fault lists | Bounded + truncation flags | `upstream/downstream_faults_truncated` (`model.rs:89–94`); core `active_faults()` bound merged in `merge_evidence` (`connection.rs:303–305`) |
| Metric labels | Fixed vocabularies only | No conn ID / peer / run ID / fault ID / hostname labels (`control.rs:58–61`); proxy/direction/fault-type tables bounded above |
| Payload capture | None anywhere | Snapshots/evidence/metrics/history record counters, identities, generations — never payload bytes (`model.rs:14–19`, `docs/control-plane.md:111–115`) |
| UDP proxies | `1..=1024` (`max_proxies`, default 128) | `DatagramRuntimeLimits::validate` (`datagram/model.rs:49–78`); `create_proxy` duplicate + cap checks (`datagram/registry.rs:117–128`, `152–163`); builder cap at `mod.rs:515–522` |
| UDP associations | global `1..=65_536` (default 4096); per-proxy nonzero and ≤ global (default 256) | `DatagramProxySpec::validate` (`datagram/model.rs:142–147`); `CapacityLease::try_acquire` (`datagram/association.rs:141–165`); setup-storm covered by `delete_during_setup_storm_*` |
| UDP history | `0..=65_536` (default 1024; `0` disables) | `stop_association` FIFO (`datagram/association.rs:924–930`) |
| UDP ingress | per-association `1..=1024` slots (default 16); global `1..=1 GiB` bytes (default 64 MiB) | `DatagramRuntimeLimits::validate` (`datagram/model.rs:65–78`); `reserve_ingress` CAS (`datagram/registry.rs:602–613`); `try_send` overflow path (`datagram/supervisor.rs:125–145`) |
| UDP idle timeout | 1 ms..=24 h (default 60 s) | `DatagramProxySpec::validate` (`datagram/model.rs:148–154`); `update_proxy` range at `control.rs:361–365`; reaper at `datagram/supervisor.rs:147–193` |
| UDP datagram size | `max_datagram_bytes` via `DatagramQueueLimits` | Oversize classified after full 64 KiB receive (`datagram/supervisor.rs:68–85`, `datagram/association.rs:733–736`) |

Core queue bounds (latency/bandwidth `max_buffer_bytes`, per-write caps) are
owned by `eggchaos-core` and covered in [core fault engine](core-fault-engine.md).

## Review checklist (file by file)

- `crates/eggchaos-server/src/lib.rs` (60 lines): re-export surface matches
  the required contract (`ProxySpec/View/Patch`, `AdmissionLimits`,
  `RuntimeParams`, registry, reset, metrics, `ServiceBuilder/Service/Handle`,
  `ControlState`, `ExpectedPublish`, fault patch types incl.
  `DatagramFaultPatch`, datagram views/runtime/limits, `MAX_METRIC_*`,
  `OUTCOME_CLASS_NAMES`, `VERSION` at `lib.rs:39–48`); `deny(unsafe_code)`
  at `lib.rs:3`; module split `admin` / `config` / `native` / `native_v2` /
  `runtime` / `scenario` / `scenario_v2` (`lib.rs:5–11`).
- `runtime/mod.rs`: `ProxySpec` + validate + `init_policies` (`55–171`);
  `AdmissionLimits`/`RuntimeParams` defaults incl. `datagram_limits`
  (`175–221`); hard caps (`50–53`); `SupervisorDone`/`DoneGuard`/`ManagedProxy`
  (`228–280`); `RuntimeInner` + `view_of` atomicity (`287–378`);
  `ControlState`/`ExpectedPublish` (`382–402`); `ServiceBuilder` validation
  incl. UDP + buffer/grace caps (`412–536`); `EggchaosService::start` with
  UDP rollback (`545–572`); `ServiceHandle` ownership (`578–608`).
- `runtime/model.rs`: `ConnectionState/Snapshot` incl. stream-loss additive
  counters (`5–97`); `DirectionBytes` (`101–108`); `ConnectionEvidence`
  (`112–117`); `ConnectionOutcome` eight variants (`121–148`);
  `ClosedConnection` (`152–163`); `ProxyPatch` restart-class docs
  (`169–181`); `FaultUpsert/FaultPatch/DatagramFaultPatch` (`185–220`);
  `ProxyView` snapshot atomicity (`226–259`); `ResetReport` with both
  failed-enable lists (`263–274`); `ControlError` codes (`278–318`);
  `EggchaosError` (`322–346`); `outcome_class` (`350–360`).
- `runtime/control.rs`: bind-before-success + ephemeral resolution +
  orphan-supervisor cleanup (`749–821`, `834–868`); restart pre-bind +
  rollback (`1035–1089`); enable/disable retention (`909–946`); done-flag +
  untrack discipline (`826–885`); fault CRUD sync (`1114–1278`);
  `ExpectedPublish` conflicts (`1321–1410`); one-generation `reset` for TCP +
  UDP (`1553–1652`); V1 + V2 supervision with shared cap/JoinSet
  (`1372`, `1432–1500`, `1709–1862`); shutdown cascade (`1657–1680`);
  metrics text incl. M041 stream-loss uniqueness (`62–292`); datagram
  orchestration (`294–615`).
- `runtime/connection.rs`: dial timeout + durable cancellation
  (`315–363`); chaos wrapping order + evidence registration
  (`364–422`); biased select + graceful vs hard-reset mapping
  (`433–581`); exactly-once close accounting + FIFO history
  (`7–64`, `172–222`); reconciling metrics incl. stream-loss
  (`69–166`); evidence merge incl. stream-loss (`240–312`).
- `runtime/supervisor.rs`: per-accept entry re-read, scope/service
  cancellation, accept-error tolerance, `children.shutdown()` + purge before
  done-flag (`3–47`); admission counters, atomic snapshot capture,
  `Connecting` registration, child spawn (`50–151`).
- `runtime/transport.rs`: `ResetResult` platform honesty (`14–23`);
  `TcpResetHandle` (`34–48`); `ResettableTcpStream` + stable-API
  `SO_LINGER=0` drop (`52–102`); read/write delegation (`110–162`).
- `runtime/metrics.rs`: `MetricsCounters` incl. V2 counters (`9–44`);
  `OUTCOME_CLASS_NAMES` (`47–56`); `MAX_METRIC_*` (`59–61`);
  `PerProxyMetrics` incl. `stream_loss` triples (`65–75`);
  `MetricTables` bounds + overflow + disjoint stream-loss
  (`80–144`); bound test (`146–167`).
- `runtime/tests.rs` (2051 lines): ephemeral relay/drain, per-proxy vs
  global limits, bound-address resolution (`215`), bind-conflict ghost check
  (`236`), delete/disconnect lifecycle, disable/enable plan retention,
  upstream redirect, failed-restart rollback, generation uniqueness
  (`393`), fault CRUD sync, live-traffic fault engage/remove, reset semantics
  (`584`), kill cleanup (incl. pre-relay kill), history bound (incl.
  zero-retention), hard-reset outcomes, metrics label boundedness +
  reconciliation (`1640`), stream-loss counting/exposition (`1799`,
  `1848`), generation/namespace pairing, scenario seed/replay/cancel. Timing
  assertions use Tokio time with tolerance windows, not bare wall-clock
  equality.
- `runtime/datagram/*`: `mod.rs` constants; `model.rs` limits/spec/views/
  errors; `registry.rs` bind-before-publish + disable/enable/delete/update
  (pre-bind) + plan publication + association views/kill/shutdown;
  `association.rs` reservation/watch setup + worker/evidence/accounting;
  `supervisor.rs` listener/admission/reaping; `tests.rs` (919 lines)
  isolation, multi-client, multi-response/unsolicited routing, kill/disable
  accounting, capacity/oversize classification, idle expiry (`331`),
  rollback (`409`), setup races (`515`, `686`+). See UDP section below.
- `crates/eggchaos-server/Cargo.toml`: minimal Eggstack picks —
  `eggress-relay` (not `eggress-embed`) for byte relay,
  `eggserve-server + eggserve-primitives` (not `eggserve-core`) for H1 admin;
  `socket2` for stable linger; no `eggress-outbound`; dev-deps
  `eggfetch-core` + `proptest` only.
- `docs/architecture.md`: dependency direction, bounded-release statement,
  abstract-reset vs shutdown distinction, M009 fault baselines (latency burst
  drain, token-bucket bandwidth, blackhole/limit-data graceful termination
  without further writes, disconnect `hard_reset` flag, generation-swap drain
  semantics) plus datagram sibling (`103–114`) — runtime behavior must match
  these.
- `docs/control-plane.md`: loopback default + public opt-in/auth, 1 MiB body
  cap, route inventory (`18–30`), generation/snapshot pairing rule, stale-base
  conflict, scenario seed-namespace + replay limits, low-cardinality metrics
  vocabulary (`175–188`), datagram resources + reset duality (`190–196`) —
  all implemented in `runtime/` + `admin.rs` above.

## Verification

```sh
cargo test -p eggchaos-server --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Full workspace closure additionally expects `cargo test --workspace
--all-features` plus fmt/clippy/doc clean and dependency/security checks per
`AGENTS.md` (`./scripts/check.sh`); timing-sensitive tests must keep
justified tolerance windows, and any platform/oracle evidence that cannot run
(e.g. Linux/macOS/Windows reset semantics, Toxiproxy differential) must be
recorded as incomplete, not replaced by inspection.

## Canonical references

- [Architecture overview](overview.md) (index; this file is §2 server runtime).
- `docs/architecture.md` — dependency direction, M009 semantics, datagram sibling.
- `docs/control-plane.md` — generations, snapshots, scenarios, metrics, datagram resources.
- `plans/adrs/001-stream-fault-engine-boundary.md`,
  `plans/adrs/002-determinism-and-live-mutation.md` — relay authority and
  determinism boundaries.
- `plans/reference/verification-matrix.md`,
  `plans/reference/toxiproxy-parity.md` — verification + parity contracts.

## Fixed-target UDP runtime (M021, M024, M025)

`runtime/datagram/` (`mod.rs` composition/re-exports + constants;
`model.rs` validated configuration, views, and evidence; `registry.rs` the
single `DatagramRuntime` control authority; `association.rs` setup, worker
loop, and teardown; `supervisor.rs` listener loop and admission; `tests.rs`
the runtime regression suite) is an independent Tokio UDP owner alongside the
TCP `ControlState`; it does not branch the stream registry or relay. UDP
shares the global `ControlState` generation counter (each datagram proxy/fault
mutation returns `next_generation`) but listener/association ownership stays
in `DatagramRuntime`, owned by `RuntimeInner` (`mod.rs:289`).

A `DatagramRuntime` binds before publishing a proxy and maps each client
socket address to one bounded association with its own connected upstream
socket, upstream/downstream `DatagramDirectionEngine`s, cancellation token,
and evidence record. This preserves reply ownership across multiple responses
and unsolicited target pushes (tested by
`association_routes_multiple_and_unsolicited_replies_and_keeps_source_port`).
Listener disable/delete/shutdown joins the listener and association tasks;
explicit administrative cancellation records queued datagram discards.
Idle expiry checks both direction queues and pre-engine ingress, so a delayed
item keeps its association alive (`supervisor.rs:147–193`,
`delayed_datagram_prevents_idle_association_expiry` at
`datagram/tests.rs:331`).

Lifecycle (`registry.rs:113–257`, `295–409`):

- `create_proxy` (`113–172`) validates, checks duplicate + `max_proxies`,
  binds `UdpSocket::bind(listen)` before publishing, resolves the bound
  address (port 0), spawns the listener, then re-checks duplicate/cap under
  lock (orphan listener cancelled + joined on race).
- `disable_proxy` (`175–191`) takes the listener owner, cancels + joins,
  then drains associations as administrative.
- `enable_proxy` (`194–242`) is idempotent when running; otherwise binds
  before changing visible state, with orphan cleanup on race.
- `delete_proxy` (`245–257`) removes the definition, cancels + joins the
  listener, drains associations as administrative.
- `update_proxy` (`295–409`) validates the merged spec, rejects
  `max_associations` below the current active count, pre-binds a replacement
  when enabling/starting or when `listen` changed (bind failure leaves the
  current listener serving — tested by
  `restart_patch_bind_failure_keeps_old_udp_listener_serving` at
  `datagram/tests.rs:409`), stops the old listener, swaps the spec, drains
  associations on `listen` change, and drains + cancels on `upstream` change
  so only future associations use the new fixed target.
- `ControlState::update_datagram_proxy` (`control.rs:352–383`) adds the
  `association_idle_timeout_ms` 1..=86 400 000 range check and maps bind
  failures to `RestartFailed` with the rollback note.

The UDP listener reads into a 65 536-byte buffer
(`UDP_RECEIVE_BUFFER_BYTES` at `datagram/mod.rs:26`) before applying the
configured `max_datagram_bytes` bound, making oversized input an observable
drop instead of an accepted truncated prefix
(`supervisor.rs:41–85`; downstream oversize in
`association.rs:733–736`). Association counts, ingress channel slots
(`ingress_per_association`), globally reserved ingress bytes
(`max_ingress_queue_bytes` via CAS `reserve_ingress` at
`registry.rs:602–613`), per-direction scheduler queues, and completed
history are bounded. IPv4 and IPv6 upstream sockets bind to the matching
unspecified family (`association.rs:529–532`). M021 reuses no Eggress
production dependency: the audited published `eggress-udp` surface is
routing/SOCKS-oriented and does not expose the required generic fixed-target
association owner.

M025 setup state machine (`association.rs:31–132`, `284–304`, `438–632`):
each client address is `Absent`, `Starting(reservation)`, or
`Active(association)`, and a reservation carries a monotonic identity
(`next_reservation`), a retained/versioned Tokio `watch` transition
(`SetupTransition { reservation_id, version, outcome }`), an idempotent
global/per-proxy capacity lease (`CapacityLease::try_acquire` at
`association.rs:141–165`, `release` exactly once via `released` flag +
`Drop` at `167–179`), and a `SetupOwnerGuard` (`379–423`) that abandons as
retryable if the owner dies before publication. Exactly one racing creator
owns setup; bind/connect runs without the registry lock
(`publish_association` at `508–618`). A waiter subscribes to the
reservation's watch receiver while still holding the association-map lock
(`resolve_association` at `438–506`), then awaits a terminal `Published` or
`Abandoned` state with `wait_for`. Publication, setup failure, and
administrative drain change the slot and publish the terminal state under
that same lock. If a transition wins before subscription, the map is no
longer `Starting`; if it wins after subscription, the retained watch state
makes `wait_for` return immediately or wakes the receiver. This is the
no-lost-wakeup ordering, with no polling or retry bound.

Retryable abandonment wakes same-client waiters so one can reserve the absent
slot with a new identity; terminal drains (`drain_associations` with
`retryable: false` at `634–659`) wake them to a conflict. A stale setup owner
can publish or release only its own reservation (`same_reservation_slot` at
`368–377`, checked under lock at `596–604` and `626–632`), never a newer
slot, and each capacity lease is released exactly once on failure, drain,
abandonment, or active teardown (`stop_association` at `904–931`). The
registry lock is not held across worker joins either, and unpublished workers
are cancellation-safe (`609–618`). Idle reaping removes a slot only if it
still holds the same expired association (`supervisor.rs:174–192`).
Covered by `concurrent_first_datagrams_for_one_client_create_one_association`
(`datagram/tests.rs:515`), `starting_publication_wakes_all_waiters_*`,
`starting_failure_wakes_waiters_*`, `starting_transition_is_retained_*`,
`update_drain_releases_reservation_*`, `disable_drains_an_unpublished_*`,
`delete_drain_releases_capacity_*`, and
`delete_during_setup_storm_leaves_no_leaked_capacity`.

Data path (`association.rs:660–902`, `supervisor.rs:61–146`):

- The association worker forwards `Immediate` admissions without entering the
  deadline heap (`admit` → `send_upstream`/`send_downstream` at
  `association.rs:717–745`), batches egress accounting into one record update
  per direction per drain (`send_upstream` at `808–846`, `send_downstream`
  at `854–892`), drains both heap schedulers via `take_ready` (`emit_ready`
  at `786–802`), and refreshes direction evidence snapshots only when
  admission, emission, or error state changed (`evidence_dirty` at
  `758–760`).
- Client ingress reserves global bytes first, then `try_send`s into the
  bounded per-association channel; full/closed paths bump
  `ingress_overflow` vs silent drop respectively
  (`supervisor.rs:104–145`). Egress/send-error/administrative-discard
  categories are unchanged; teardown attributes queued + ingress work to
  `administrative_discards` only under administrative cancel
  (`association.rs:762–771`).
- Fault plans publish per direction with optional expected-generation guard
  (`registry.rs:432–465`); associations read the upstream policy snapshot at
  receive time and compile the downstream snapshot per worker spec, so plan
  updates apply to new/future datagrams without touching the socket mapping.
- `associations()` omits in-setup reservations (no evidence yet);
  `all_associations()` returns retained history + active sorted by ID
  (`registry.rs:470–502`); `kill_association` removes exactly the live slot
  by stable ID and stops it administratively (`registry.rs:536–553`; setup
  races resolve on retry).
