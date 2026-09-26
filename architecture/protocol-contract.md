# Protocol contract

Back to [architecture overview](overview.md).

Evidence-first deep dive for the native wire contract. Authority is code;
`docs/control-plane.md` is a summary, not the spec. All paths below are
relative to the workspace root. HEAD at write time: `e050fa2` (post-M041;
M032 `ed05f68` origin).

Sources: `crates/eggchaos-protocol/src/` (`lib.rs`, `common.rs`,
`stream.rs`, `scenario_v2.rs`, `routes.rs`),
`crates/eggchaos-protocol/Cargo.toml`,
`crates/eggchaos-protocol/tests/contract_drift.rs`,
`crates/eggchaos-protocol/tests/golden.rs`,
`crates/eggchaos-protocol/tests/fixtures/*.json`,
`api/openapi/eggchaos-v1.yaml`,
`scripts/check_openapi.sh`, `scripts/sync_sdk_contract.py`,
`bindings/_contract/operations.json`,
`bindings/_contract/cross_language_fixtures.json`,
`crates/eggchaos-server/src/native.rs` (adapters + re-exports),
`crates/eggchaos-server/src/native_v2.rs` (pure re-exports),
`crates/eggchaos-server/tests/native_route_inventory.rs`,
`docs/control-plane.md`,
`plans/closure/M032-native-protocol-contract-extraction-and-openapi-foundation-closure.md`.

## 1. Purpose and boundary

`eggchaos-protocol` owns the stable native `/v1` wire DTOs, defaults,
units, bounds, discriminators, unknown-field rejection, the
`NATIVE_OPERATIONS` inventory, and `OPENAPI_CONTRACT_VERSION`
(`crates/eggchaos-protocol/src/lib.rs:1-25`):

```text
eggchaos-core
      ^
      |
eggchaos-experiment
      ^
      |
eggchaos-protocol (this crate: wire DTOs + operation inventory)
      ^
      |
eggchaos-server (runtime/control authority, compatibility re-exports)
```

- Depends only on `eggchaos-core`, `eggchaos-experiment`, `serde`,
  `serde_json`, `toml` (plus dev-only `serde_yaml` for the drift test);
  no sockets, no Tokio runtime/tasks, no EggServe, no live policy/state,
  no CLI presentation, no Toxiproxy DTOs, no EggReplay/EggProbe
  (`Cargo.toml:15-23`, `lib.rs:1-8`). `#![deny(unsafe_code)]`
  (`lib.rs:25`).
- Conversions land only on `eggchaos-core` (`FaultSpec`, `FaultPlan`,
  `DatagramFaultSpec`, `DatagramPlan`, `Direction`, queue limits) or
  `eggchaos-experiment` (`ScenarioAction`, `ScenarioScheduleV2`,
  compiled/fingerprint types) semantic types. Runtime assembly
  (`ProxySpec`, live policies, listener ownership, `AdmissionLimits`,
  view assembly) stays in `eggchaos-server` (`lib.rs:22-24`,
  `native.rs:1-10,30-43,106-128,179-208`).
- Server compat: `native.rs` re-exports every protocol DTO for source
  compatibility (`native.rs:13-23`) and adds exactly one assembly per DTO
  family; `native_v2.rs` is pure re-exports with no logic
  (`native_v2.rs:1-11`). There is one DTO definition per wire type and
  one runtime assembly per family — no second state store.
- What protocol does **not** own (still true at HEAD, per M032
  limitations): `ConnectionSnapshot` / `ClosedConnection` / `ResetReport`
  remain server-owned runtime views serialized directly; the OpenAPI
  document describes their observed shapes. Externally-tagged
  `ConnectionOutcome` / `ResetResult` are documented descriptively, not
  as exhaustive `oneOf` schemas; drift pins only the fully-owned DTO
  discriminators.

## 2. Module inventory

### 2.1 `common.rs` — envelope, health/version, constants

- Constants (`common.rs:9-16`): `NATIVE_API_VERSION = "v1"`,
  `NATIVE_JSON_CONTENT_TYPE = "application/json"`,
  `METRICS_CONTENT_TYPE = "text/plain; version=0.0.4"`,
  `MAX_REQUEST_BODY_BYTES = 1 MiB` (declared here, enforced by the
  server admin runtime).
- `ErrorEnvelopeV1 { error: ErrorBodyV1 { code, message } }`
  (`common.rs:18-51`): `{"error":{"code":"...","message":"..."}}`, both
  structs `deny_unknown_fields`. Stable codes: `not_found`, `conflict`,
  `invalid`, `invalid_json`, `unauthorized`, `bind_failed`,
  `restart_failed`, `serialization`. Messages are bounded human detail
  and never echo bearer tokens. Exact-shape test
  (`common.rs:73-91`).
- `HealthV1 { running: bool, generation: u64 }` and
  `VersionV1 { version: String, api: "v1" }` (`common.rs:53-71`), both
  `deny_unknown_fields`.

### 2.2 `stream.rs` — proxy / fault / datagram / scenario-v1 DTOs

Field spellings, defaults, units, bounds, and core conversions live
here; the server only assembles (`stream.rs:1-7`).

Defaults and shared bound constants (`stream.rs:21-67`):
`NATIVE_DEFAULT_BUFFER_BYTES = 65536`,
`NATIVE_DEFAULT_PROXY_TIMEOUT_MS = 5000`,
`NATIVE_DEFAULT_BANDWIDTH_BYTES_PER_SECOND = 1`,
`NATIVE_DEFAULT_BANDWIDTH_BURST_BYTES = 65536`,
`NATIVE_DEFAULT_LIMIT_BYTES = 1`,
`NATIVE_DEFAULT_SLICE_AVERAGE_SIZE = 1024`;
`MAX_TIMEOUT_MS = 300000`, `MAX_CONNECTION_LIMIT = 1000000`,
`MAX_HISTORY_LIMIT = 1000000`,
`MAX_RELAY_BUFFER_BYTES = 16 MiB`,
`MAX_LATENCY_BUFFER_BYTES = 64 MiB`.

- `NativeProxyRequestV1` (`stream.rs:69-106`): `name`, `listen`,
  `upstream` required; `enabled` default `true`,
  `max_connections: Option<usize>` default `None`,
  `connect_timeout_ms` default `5000`, `seed` default `0`.
  `validate()` checks the shared name rule plus
  `connect_timeout_ms in 1..=300000`.
- `validate_proxy_name` (`stream.rs:108-123`): `1..=128` bytes,
  `[A-Za-z0-9._-]+` (URL single-segment rule). Parity with
  `ProxySpec::validate` is pinned by a shared corpus
  (`native.rs:287-350`).
- `NativeProxyPatchV1` (`stream.rs:125-155`): all-`Option` patch;
  `max_connections: Option<Option<usize>>` so `null` clears;
  `validate()` bounds only a supplied timeout.
- `FaultKindV1` (`stream.rs:157-214`): 8 variants, explicit wire schema,
  `#[serde(tag = "type", rename_all = "kebab-case",
  deny_unknown_fields)]` with `limit-data` / `slow-close` renames and
  the literal `stream-loss` (ADR 007; fixed 32 KiB logical-chunk
  userspace loss, `correlation` raises post-drop probability capped at
  1 — explicitly not IP/TCP packet loss). `validate()` builds a
  one-fault `FaultPlan` (`stream.rs:216-230`); `into_core` /
  `from_core` map to `eggchaos-core::FaultKind` with
  `NonZeroU64` / finite-`[0,1]` checks (`stream.rs:231-338`). Required
  vs defaulted per `docs/control-plane.md:42-50`: `latency.delay_ns`
  required (`jitter_ns` 0, `max_buffer_bytes` 65536);
  `bandwidth` all-defaulted (1 / 65536); `blackhole.close_after_ns`
  nullable; `limit-data.bytes` default 1 (non-zero);
  `slow-close.delay_ns` required; `slice` all-defaulted (1024 / 0 / 0);
  `disconnect.after_ns` 0 + `hard_reset` false;
  `stream-loss.loss_rate` + `correlation` both required, finite
  `[0,1]`. Exact wire fixtures for all 8
  (`stream.rs:1280-1361`).
- `FaultUpsertV1 { direction, id, probability = 1.0, kind }`
  (`stream.rs:340-376`) and `FaultPatchV1 { probability?, kind? }`
  using the identical create schema (`stream.rs:378-407`); both
  `deny_unknown_fields`; `validate`/`into_core` go through
  `FaultId`/`Probability`/core plan rules. Create/patch/response share
  one `kind` JSON shape (`stream.rs:1251-1278`).
- `NativeFaultViewV1` / `NativeProxyViewV1` (`stream.rs:409-463`):
  explicit native fault views (never core-enum Serde); proxy views carry
  both fault arrays plus `upstream/downstream_generation` and
  `upstream/downstream_seed_namespace` from the same atomic snapshot,
  `bound_addr`, `max_connections`, `connect_timeout_ms`, `seed`.
  Assembled by the server (`native.rs:74-104`).
- `RuntimeConfigV1` + `DatagramRuntimeConfigV1`
  (`stream.rs:468-596`): stream `global_connections` 1024 /
  `history` 256 / `relay_buffer_bytes` 65536 / `termination_grace_ms`
  5000; datagram `max_proxies` 128 / `max_associations` 4096 /
  `history` 1024 / `ingress_per_association` 16 /
  `max_ingress_queue_bytes` 64 MiB. `validate()` /
  `relay_buffer()` / `termination_grace()` enforce
  `1..=1000000`, `<=1000000`, non-zero `<=16 MiB`, `<=300000 ms`;
  the server limit adapters reuse these exact bounds
  (`native.rs:106-128`, parity test `native.rs:367-385`).
- Scenario V1 (`stream.rs:598-793`): `ScenarioV1 { version: 1, seed,
  events }` (max 1024 events, `at_ms` non-decreasing),
  `ScenarioEventV1 { at_ms, action }`, `ScenarioActionV1` with 4
  kebab-case tags (`set-plan`, `remove-fault`, `set-datagram-plan`,
  `remove-datagram-fault`), `ScenarioFaultV1` in the CRUD vocabulary,
  `into_core_actions()` to `eggchaos_experiment::ScenarioAction`
  (server wraps in its runtime `Scenario` via
  `scenario_v1_into_runtime`, `native.rs:130-143`).
- `ScenarioStatusV1` lowercase (`pending…failed`,
  `stream.rs:795-805`) and run/event evidence
  (`stream.rs:807-867`); server maps its run record through
  `from_parts` (`native.rs:145-177`).
- Datagram family (`stream.rs:869-1145`): `DatagramFaultKindV1` with 6
  kebab-case tags (`delay`, `loss`, `duplicate`, `reorder`,
  `payload-corrupt`, `bandwidth`); `DatagramFaultSpecV1`,
  `DatagramFaultUpsertV1 { direction, #[flatten] fault }`,
  `DatagramFaultPatchV1`; `NativeDatagramProxyRequestV1` (required
  `name/listen/upstream`; defaults 256 associations / 60000 ms idle /
  1024 queued / 4 MiB queued bytes / 65507 max datagram / seed 0 /
  empty fault lists) validated through `into_core_parts`
  (idle `1..=86400000`, non-zero queue limits + core validate,
  cross-direction fault-ID uniqueness, per-direction `DatagramPlan`
  check); `DatagramProxyCoreParts` handoff;
  `NativeDatagramProxyPatchV1`; evidence/association views
  (`stream.rs:1147-1245`, payloads never captured), assembled by the
  server (`native.rs:234-280`).

### 2.3 `scenario_v2.rs` — schedule DTOs, TOML/JSON parse, canonical JSON

Additive v2 family; v1 wire format unchanged
(`scenario_v2.rs:1-15`).

- `ScenarioScheduleV2Dto { version, seed, execution_key, isolation,
  cleanup, phases, repeat? }`, `ScenarioPhaseV2Dto { name?,
  duration_ns, actions }`, `ScenarioRepeatV2Dto { count, phases }`
  (`scenario_v2.rs:24-56`); `ScenarioActionDto` mirrors
  `ScenarioActionV1` tags (`scenario_v2.rs:58-87`); all
  `deny_unknown_fields`.
- `into_internal` conversions land on `ScenarioScheduleV2` /
  `SchedulePhaseV2` / `ScheduleRepeatV2` / `ScenarioAction` with
  `FaultId`/`Probability`/plan validation
  (`scenario_v2.rs:89-159,262-325`); version gate is
  `SCHEDULE_SCHEMA_VERSION` (`scenario_v2.rs:273-276,374-376`).
- TOML authoring DTOs (`ScenarioScheduleV2Toml` family,
  `scenario_v2.rs:327-428`) accept the same shape with integer-ns
  `duration_ns` — deterministic by construction; a human-duration
  extension is an explicitly separate future plan. JSON and TOML parse
  to the same internal source and fingerprint
  (`scenario_v2.rs:800-847`).
- `to_canonical_json` round-trips the internal source through the wire
  DTO (`scenario_v2.rs:430-436`); re-parse recompiles to the same
  fingerprint (`golden.rs:98-115`).
- Response shapes, all server-assembled from experiment records:
  `ScheduleValidateV2` (fingerprint, compiler version, event count),
  `ScheduleCompileV2` + `ScheduleCompiledEventV2` (normalized tape
  with `compiled_index`, `top/{i}` / `repeat/{iter}/{i}` phase
  identity, `offset_ns`, action/proxy/direction/transport),
  `ScheduleRunEventV2` (scheduled/applied/`late_by_ns` + generations),
  `ScheduleCleanupV2` (`restored` / `conflict` / `missing` /
  `not_requested`), `ScheduleRunV2` (additive to `ScenarioRunV1`;
  lowercase status strings) (`scenario_v2.rs:438-580,582-591,634-755`).

### 2.4 `routes.rs` — `NATIVE_OPERATIONS` table, 36 ops

Single machine-readable route authority (`routes.rs:1-13`):
`NativeOperation { method, path, operation_id, summary }`
(`routes.rs:22-33`); `OPENAPI_CONTRACT_VERSION = "1.0.0"` — bump only
with an explicit versioned contract decision, never for a DTO move
alone (`routes.rs:16-20`).

`NATIVE_OPERATIONS` (`routes.rs:39-256`) holds exactly 36 entries;
uniqueness of `operation_id` and `method+path` is asserted
(`routes.rs:258-276`). Grouped count (matches YAML §4 path list):

| Family | Ops |
| --- | --- |
| health / version / metrics / reset | 4 (`getHealth`, `getVersion`, `getMetrics`, `resetService`) |
| stream proxies | 5 (`list/create/get/patch/delete…Proxy`) |
| stream faults | 5 (`list/add/get/patch/delete…Fault`) |
| connections + history | 4 (`list/get/kill…Connection`, `getHistory`) |
| scenarios (v1+v2) | 5 (`applyScenario`, `validateSchedule`, `compileSchedule`, `getScenario`, `cancelScenario`) |
| datagram proxies | 5 (`list/create/get/patch/delete…DatagramProxy`) |
| datagram faults | 5 (`list/add/get/patch/delete…DatagramFault`) |
| datagram associations | 3 (`list/get/kill…DatagramAssociation`) |

Path templates use `{name}` / `{id}` placeholders matching the OpenAPI
document; auth is uniform (bearer required iff the admin listener is
token-configured; loopback without a token accepts unauthenticated,
`routes.rs:8-12`).

## 3. Wire rules

- Kebab-case `kind.type` / action `type` everywhere:
  `FaultKindV1` (`latency`, `bandwidth`, `blackhole`, `limit-data`,
  `slow-close`, `slice`, `disconnect`, `stream-loss`),
  `DatagramFaultKindV1` (`delay`, `loss`, `duplicate`, `reorder`,
  `payload-corrupt`, `bandwidth`), `ScenarioActionV1` /
  `ScenarioActionDto` (`set-plan`, `remove-fault`,
  `set-datagram-plan`, `remove-datagram-fault`). Native userspace TCP
  byte-chunk dropping is `stream-loss`; only the Toxiproxy
  compatibility presentation may say `packet_loss`.
- Durations: `*_ns` fields are unsigned integer nanoseconds
  (`delay_ns`, `jitter_ns`, `close_after_ns?`, `after_ns`,
  `hold_ns`, `duration_ns`, `offset_ns`, `scheduled/applied/late_by_ns`,
  injected/bandwidth delay nanos). Fields ending in `_ms` are
  milliseconds (`connect_timeout_ms`, v1 `at_ms`,
  `association_idle_timeout_ms`, `age_ms`/`idle_ms`,
  `termination_grace_ms`, runtime bounds). TOML config keeps human
  duration strings at its own parser edge; native HTTP never does.
- Addresses are `SocketAddr` strings (`"127.0.0.1:6379"`) for
  `listen`, `upstream`, `bound_addr?`, `peer`, `client`
  (YAML `SocketAddr` schema; `ProxyName` pattern
  `^[A-Za-z0-9._-]+$`, `1..=128`).
- Required fields are explicit: proxy create
  (`name, listen, upstream`); fault upserts
  (`direction, id, kind`; `probability` defaults to `1.0`);
  `ScenarioV1` (`version, seed, events`); `ScenarioScheduleV2Dto`
  (`version, seed, execution_key`); envelope (`error` →
  `code, message`). Drift asserts exactly these sets
  (`contract_drift.rs:240-263`). IDs are `u64` numerics for
  connections/associations/scenarios (`400 invalid` on non-integer),
  opaque strings (fault IDs percent-encoded, decoded exactly once) for
  faults.
- Unknown-field rejection: every request DTO plus the envelope and
  health/version carry `deny_unknown_fields`; malformed discriminators
  fail parse. Negative coverage: `golden.rs:64-85`,
  `scenario_v2.rs:849-876`, `stream.rs:1450,1468-1477`.
- Error envelope is the only JSON error shape (except metrics):
  `{"error":{"code":"<snake_case>","message":"<bounded detail>"}}`
  with `application/json` content type.
- `METRICS_CONTENT_TYPE` exception: `GET /metrics` has no `/v1`
  prefix and returns `text/plain; version=0.0.4` Prometheus text.
- `MAX_REQUEST_BODY_BYTES` (1 MiB): declared in protocol
  (`common.rs:15-16`), enforced by the server admin runtime
  (`RuntimeConfig` + `RequestBodyPolicy::Buffer`, 413 on overflow).

## 4. OpenAPI drift discipline

- Gate (`scripts/check_openapi.sh:1-21`): (1)
  `cargo test -p eggchaos-protocol --all-features`, (2)
  `cargo test -p eggchaos-server --all-features --test
  native_route_inventory`, (3) YAML shape assert — `openapi: 3.0.3`,
  21 `paths`, 36 `get/post/patch/delete` operations, printing
  `{"openapi":"pass","paths":21,"operations":36}`.
- `contract_drift.rs` (single-authority option 3): `info.version`
  equals `OPENAPI_CONTRACT_VERSION` (`51-63`); operation-set **and**
  `operationId` equality between YAML and `NATIVE_OPERATIONS`
  (`66-95`); `bearerAuth: http/bearer` scheme (`98-113`); discriminator
  `mapping` equality for `StreamFaultKind` (8 tags),
  `DatagramFaultKind` (6), `ScenarioActionV1` (4)
  (`198-224`); required-field equality for the 5 schemas above
  (`240-263`); per-variant wire round-trip + `into_core`
  (`266-342`).
- `golden.rs`: byte-identical fixture round-trips —
  `stream_faults.json`, `datagram_faults.json`, `proxy.json`
  (stream + datagram create), `scenario_v1.json` (→ 4 core actions),
  `scenario_v2.json` (JSON↔TOML fingerprint equality + canonical
  re-parse), `errors.json` (5 envelopes) (`golden.rs:18-131`).
- `sync_sdk_contract.py` derives SDK artifacts from the YAML authority
  (never reconstructed independently): sorted `operations.json`
  snapshot (`openapi_version`, 36 `{method, path, operation_id}`,
  `stream/datagram/scenario_action_tags`), plus
  `bindings/python-client/…/_generated.py` and
  `bindings/typescript-client/src/generated.ts` via the reviewed 36-entry
  `OPERATION_METHODS` hand-off (`sync_sdk_contract.py:32-79,81-199`).
  CI fails on regeneration diffs. `cross_language_fixtures.json` (7
  cases incl. `stream-loss`, bandwidth defaults, datagram `loss`, v1
  apply, v2 validate) proves identical native JSON in both languages.
- Live proof (`native_route_inventory.rs:1-80…`): every
  `NATIVE_OPERATIONS` entry resolves against a real loopback admin;
  neither a missing route nor a missing resource may answer
  `"route not found"`, and every 4xx uses the shared envelope.

## 5. Review checklist

When reviewing this surface, confirm:

1. No sockets, runtime, listeners, CLI, Toxiproxy, or Eggfetch in
   `eggchaos-protocol` (`Cargo.toml:15-23`); no `unsafe`
   (`lib.rs:25`); conversions target core/experiment types only.
2. No second DTO definition per wire type (server does re-export +
   assembly only); no parallel state — `ControlState` /
   `DatagramRuntime` remain the single authorities.
3. All 8 stream + 6 datagram + 4 action discriminator tags present in
   code, YAML `mapping`, `operations.json`, and generated SDK tables.
4. Integer-ns vs `_ms` units preserved; `SocketAddr` stays a string;
   `connect_timeout_ms` spelling (not `connect_timeout`) on create and
   patch; proxy-name charset/length rule identical on both sides of the
   adapter.
5. `deny_unknown_fields` on every request DTO; required-field sets
   match §3; error envelope is the only JSON error shape and never
   echoes tokens.
6. `NATIVE_OPERATIONS` (36) ≡ YAML paths (21) / operations (36) ≡
   `operations.json` (36) ≡ both generated SDK tables (36 methods);
   `OPENAPI_CONTRACT_VERSION` ≡ `info.version` (`1.0.0`).
7. `ConnectionSnapshot` / `ClosedConnection` / `ResetReport` still
   server-owned; externally-tagged outcome enums still descriptive-only.
8. `GET /metrics` still unprefixed Prometheus text; 1 MiB body bound
   still declared in protocol and enforced in admin.

## 6. Verification

```sh
cargo test -p eggchaos-protocol --all-features
./scripts/check_openapi.sh
```

`check_openapi.sh` additionally runs the live 36-operation inventory
proof (`--test native_route_inventory`) and the YAML shape assert
(`openapi 3.0.3`, 21 paths, 36 operations). SDK derivation check:

```sh
python3 scripts/sync_sdk_contract.py  # deterministic; CI fails on diff
git diff --exit-code -- bindings/_contract/operations.json \
  bindings/python-client/eggchaos_client/_generated.py \
  bindings/typescript-client/src/generated.ts
```

Record any un-runnable oracle/platform as incomplete evidence; do not
substitute inspection for execution.

## 7. Discrepancies found at HEAD (not fixed; this file only records)

1. Stale milestone ref in code comment: `stream.rs:206` says
   `(ADR 007 / M037)` on `StreamLoss`, but no M037 exists — the
   stream-loss chain is ADR 007 / M036–M041 (M041 latest corrective).
   Comment should say ADR 007 (or M037-free wording).
2. Golden corpus lags the 8th stream fault:
   `tests/fixtures/stream_faults.json` holds 7 entries (no
   `stream-loss`) and `golden.rs:22` asserts `len() == 7`, while
   `FaultKindV1`, `contract_drift.rs`, the YAML
   (`StreamFaultStreamLoss`), and `operations.json`
   (`stream_fault_tags`, 8 entries) all cover `stream-loss`.
   Per-variant coverage exists in `stream.rs:1280-1361` and
   `contract_drift.rs:266-310`, but the golden file predates ADR 007
   (accurate at M032 closure: "7/7") and was never extended.
3. Placeholder-name cosmetic drift: `routes.rs:168,173` and the YAML
   use `/v1/scenarios/{id}`, while `docs/control-plane.md` and
   `control-plane-cli.md` write `/v1/scenarios/{run_id}`. Runtime
   dispatch splits segments so behavior is identical, but the reviewed
   inventory string differs from the prose.
4. Structural asymmetry (by design, worth knowing):
   `DatagramFaultUpsertV1` nests its fault via `#[flatten]`
   (`stream.rs:976-980`) while `FaultUpsertV1` is flat-declared;
   both serialize to the same flat `{direction, id, probability,
   kind}` shape the YAML requires, but `deny_unknown_fields` +
   `flatten` interact differently at the serde layer than a flat
   struct.
5. Working tree was already dirty at write time (`git status` shows
   `M architecture/{core-fault-engine,eggfetch-integration,overview,
   scenario-observability,server-runtime,toxiproxy-compat}.md`);
   this file was created without touching any of them.
