# Control plane, config, and CLI

Back to [architecture overview](overview.md).

Evidence-first deep dive for the native control surface. Authority is code;
`docs/control-plane.md` and `docs/configuration.md` are summaries, not the
spec. All paths below are relative to the workspace root.

Sources: `crates/eggchaos-protocol/src/` (wire DTO + operation-inventory
authority: `lib.rs`, `routes.rs`, `stream.rs`, `scenario_v2.rs`,
`common.rs`), `api/openapi/eggchaos-v1.yaml` (mechanically drift-checked
contract, 36 operations), `crates/eggchaos-server/src/admin.rs`,
`crates/eggchaos-server/src/native.rs` (server adapters + compatibility
re-exports), `crates/eggchaos-server/src/native_v2.rs` (V2 re-exports only),
`crates/eggchaos-server/src/runtime/` (`mod.rs` composition/re-exports,
`control.rs` single `ControlState` authority, `model.rs` error/evidence
types, `datagram/` UDP authority), `crates/eggchaos-server/src/config.rs`,
`crates/eggchaos-server/src/lib.rs`,
`crates/eggchaos-cli/src/main.rs`,
`crates/eggchaos-cli/Cargo.toml`,
`crates/eggchaos-cli/tests/cli_e2e.rs`,
`docs/control-plane.md`, `docs/configuration.md`,
`qualification/release/eggchaos.toml`.

## 1. Native `/v1` admin API

### 1.1 Substrate

`crates/eggchaos-server/src/admin.rs:1-137` owns only listener setup, auth,
bounds, and route dispatch. Business state lives in `ControlState`
(`crates/eggchaos-server/src/runtime/control.rs`); `admin.rs` never stores
proxies, faults, or connections itself.

- HTTP substrate: `eggserve_primitives::{Request, RequestBodyPolicy,
  Response, ResponseBody, StatusCode}` plus
  `eggserve_server::{service_fn_with_policy, RuntimeConfig, Server,
  ServerHandle, ServiceError}`.
- Startup (`NativeAdmin::start`, `admin.rs:95-137`):
  `TcpListener::bind(config.bind)` then
  `Server::builder().runtime(runtime).from_listener(listener).build()` then
  `start_with_service(service)`. Returns `AdminHandle { local_addr, server }`
  with `shutdown()` / `wait()` (`admin.rs:71-93`).
- Body/connection bounds (`admin.rs:106-122,184-187`):
  - `RuntimeConfig { max_request_body_bytes: 1 MiB, max_connections: 128, .. }`.
  - `RequestBodyPolicy::Buffer { max_bytes: 1 MiB }` (mirrors
    `MAX_REQUEST_BODY_BYTES`, `eggchaos-protocol/src/common.rs:16`).
  - `body.read_all()` failure maps to `ServiceError::rejected(413, ...)`.
- The EggServe leaf H1 runtime owns parsing, body bounds, and connection
  lifecycle; eggchaos owns route dispatch + typed JSON conversion
  (`docs/control-plane.md:13-16`).

### 1.2 Auth, loopback policy

`AdminConfig` (`admin.rs:15-22`, `Default` at `37-45`; `Debug` redacts the
token at `24-35`):

```rust
pub struct AdminConfig {
    pub bind: SocketAddr,        // default 127.0.0.1:8475
    pub public_admin: bool,      // default false
    pub auth_token: Option<String>, // default None
}
```

Gate (`admin.rs:101-104`):

```rust
if !config.bind.ip().is_loopback() && (!config.public_admin || config.auth_token.is_none()) {
    return Err(AdminError::InsecurePublicBind);
}
```

- Loopback binds need no token. Non-loopback requires **both**
  `public_admin=true` and `auth_token=Some(...)`, else
  `AdminError::InsecurePublicBind` (`admin.rs:47-65`).
- When a token is configured, `handle_request` (`admin.rs:162-181`) requires
  `Authorization: Bearer <token>` with constant-time comparison
  (`constant_time_equal`, `admin.rs:737-743`). Mismatch/missing returns
  `403` with `{"error":{"code":"unauthorized","message":"authorization required"}}`.
  The token is never echoed (unit test `admin.rs:795-804`; CLI auth
  round-trip `cli_e2e.rs:219-281`).
- The auth gate runs before route dispatch, so it covers every route below
  including `GET /metrics` when a token is configured.

### 1.3 Error envelope and status mapping

Envelope (`admin.rs:140-142`, shared `ErrorEnvelopeV1` owned by
`eggchaos-protocol/src/common.rs:18-51`):

```json
{"error": {"code": "<snake_case>", "message": "<human detail>"}}
```

All JSON responses use `content-type: application/json`.

`control_status` (`admin.rs:144-153`) + `ControlError::code`
(`runtime/model.rs:307-317`; enum at `runtime/model.rs:278-305`):

| `ControlError` | `code` | HTTP |
| --- | --- | --- |
| `NotFound(_)` | `not_found` | `404` |
| `Conflict(_)` | `conflict` | `409` |
| `BindFailed{..}` | `bind_failed` | `409` |
| `Invalid(_)` | `invalid` | `400` |
| `RestartFailed{..}` | `restart_failed` | `500` |

Datagram failures arrive as `DatagramRuntimeError` and are translated at
the `ControlState` boundary (`runtime/control.rs:1-17`): `Invalid`/`NotFound`
pass through, `Bind` becomes `BindFailed` (create) or `RestartFailed`
(restart-class update), capacity `Conflict` passes through.

Additional envelope codes from `admin.rs:189-229`:

| Case | HTTP | `code` |
| --- | --- | --- |
| Malformed JSON body | `400` | `invalid_json` |
| Malformed percent-escape / non-UTF8 fault ID | `400` | `invalid` (`invalid path component`) |
| Unknown route | `404` | `not_found` |
| Auth failure | `403` | `unauthorized` |
| Body over bound | `413` | via `ServiceError::rejected` |
| Serialization fallback | — | `serialization` |

`GET /metrics` is the exception: `200 text/plain; version=0.0.4`
(`text_response`, `admin.rs:765-772`; `METRICS_CONTENT_TYPE` in
`common.rs:14`). Note a contract divergence: the OpenAPI document marks
`GET /metrics` with `security: []` (`eggchaos-v1.yaml:46`), but the runtime
auth gate (`admin.rs:162-181`) runs before dispatch, so a configured bearer
token is still required for `/metrics`. The protocol inventory
(`routes.rs:9-12`) states the runtime behavior (every operation requires the
token when configured).

### 1.4 Route inventory

Dispatch is `route()` (`admin.rs:272-735`). `segments` are `/`-split,
empty-filtered; proxy names travel as single path segments, fault IDs as
single segments decoded exactly once (`decode_path_component`,
`admin.rs:231-259`; only the fault-`{id}` position of the two fault
families is decoded, `admin.rs:189-216`). This is the full 36-operation
inventory (`NATIVE_OPERATIONS`, `eggchaos-protocol/src/routes.rs:39-256`).

Stream/proxy family:

| Method | Route | Success | Notes |
| --- | --- | --- | --- |
| `GET` | `/v1/health` | `200 {"running":true,"generation":N}` | generation from `ControlState::generation()` (`control.rs:618`) |
| `GET` | `/v1/version` | `200 {"version":"<cargo>","api":"v1"}` | `env!("CARGO_PKG_VERSION")` |
| `GET` | `/metrics` | `200` Prometheus text | no `/v1` prefix; see §1.3 auth note |
| `POST` | `/v1/reset` | `200 ResetReport` | resets TCP + datagram families; see §4 |
| `GET` | `/v1/proxies` | `200 NativeProxyViewV1[]` | `state.list()` (`control.rs:627`) converted to native DTOs |
| `POST` | `/v1/proxies` | `201 {"proxy":view,"generation":N}` | body `NativeProxyRequestV1`; bind-before-register |
| `GET` | `/v1/proxies/{name}` | `200 NativeProxyViewV1` / `404` | |
| `PATCH` | `/v1/proxies/{name}` | `200 {"proxy":view,"generation":N}` | body `NativeProxyPatchV1`; restart-class for listen/upstream |
| `DELETE` | `/v1/proxies/{name}` | `200 {"generation":N,"deleted":true}` | stops listener, terminates conns |
| `GET` | `/v1/proxies/{name}/faults` | `200 {"upstream":[...],"downstream":[...]}` | arrays of `NativeFaultViewV1` from live snapshots |
| `POST` | `/v1/proxies/{name}/faults` | `201 {"direction":..,"fault":..,"generation":N}` | body `FaultUpsertV1`; kebab-case typed `kind.type` |
| `GET` | `/v1/proxies/{name}/faults/{id}` | `200 {"direction":..,"fault":..}` | upstream searched first (`control.rs:1249-1262`) |
| `PATCH` | `/v1/proxies/{name}/faults/{id}` | `200 {"direction":..,"fault":..,"generation":N}` | body `FaultPatchV1`; same kind DTO as create |
| `DELETE` | `/v1/proxies/{name}/faults/{id}` | `200 {"generation":N,"deleted":true}` | either direction |
| `GET` | `/v1/connections` | `200 ConnectionSnapshot[]` | live evidence merged at read time (`control.rs:651`) |
| `GET` | `/v1/connections/{id}` | `200` / `400` non-integer / `404` | `connection id must be an integer` |
| `DELETE` | `/v1/connections/{id}` | `200 {"id":N,"terminated":true}` / `404` | `kill()` (`control.rs:675`); level-triggered cancel |
| `GET` | `/v1/history` | `200 ClosedConnection[]` | bounded, newest last (`control.rs:668`) |

Scenario family (version-aware; semantics belong to
`scenario-observability.md`, this file records only the route/status
contract):

| Method | Route | Success | Notes |
| --- | --- | --- | --- |
| `POST` | `/v1/scenarios/apply` | `202 ScenarioRunV1` / `202 ScheduleRunV2` | `version_of` peek (`admin.rs:749-752`): `version: 2` → `ScenarioScheduleV2Dto`, else `ScenarioV1`; invalid doc maps to `ControlError::Invalid`, unparseable JSON to `invalid_json` |
| `POST` | `/v1/scenarios/validate` | `200 ScheduleValidateV2` | body `ScenarioScheduleV2Dto`; fingerprint/identity only, creates no run |
| `POST` | `/v1/scenarios/compile` | `200 ScheduleCompileV2` | body `ScenarioScheduleV2Dto`; normalized tape + fingerprint, creates no run |
| `GET` | `/v1/scenarios/{run_id}` | `200 ScenarioRunV1` / `200 ScheduleRunV2` / `400` / `404` | serves both versions; run IDs share one namespace; lowercase status values |
| `DELETE` | `/v1/scenarios/{run_id}` | `200 ScenarioRunV1` / `200 ScheduleRunV2` / `400` / `404` | cancels both versions |

Datagram family (`admin.rs:287-467`; M022 operator surface, see §8):

| Method | Route | Success | Notes |
| --- | --- | --- | --- |
| `GET` | `/v1/datagram-proxies` | `200 NativeDatagramProxyViewV1[]` | `state.datagram_proxies()` (`control.rs:308`) |
| `POST` | `/v1/datagram-proxies` | `201 {"proxy":view,"generation":N}` | body `NativeDatagramProxyRequestV1` via `datagram_proxy_request_into_spec` (`native.rs:184-208`) |
| `GET` | `/v1/datagram-proxies/{name}` | `200` / `404` | |
| `PATCH` | `/v1/datagram-proxies/{name}` | `200 {"proxy":view,"generation":N}` / `400` empty patch | body `NativeDatagramProxyPatchV1`; empty patch → `Invalid("empty datagram proxy patch")` (`admin.rs:325-334`) |
| `DELETE` | `/v1/datagram-proxies/{name}` | `200 {"generation":N,"deleted":true}` | |
| `GET` | `/v1/datagram-proxies/{name}/faults` | `200 {"upstream":{generation,seed_namespace,faults},"downstream":{...}}` | per-direction object shape (unlike the stream arrays) from `get_datagram_plan` (`control.rs:400`) |
| `POST` | `/v1/datagram-proxies/{name}/faults` | `201 {"direction":..,"fault":..,"generation":N}` | body `DatagramFaultUpsertV1`; six explicit `DatagramFaultKindV1` types |
| `GET` | `/v1/datagram-proxies/{name}/faults/{id}` | `200 {"direction":..,"fault":..}` | fault IDs unique across directions (`stream.rs:1087-1094`) |
| `PATCH` | `/v1/datagram-proxies/{name}/faults/{id}` | `200 {"direction":..,"fault":..,"generation":N}` | body `DatagramFaultPatchV1` (probability and/or kind) |
| `DELETE` | `/v1/datagram-proxies/{name}/faults/{id}` | `200 {"generation":N,"deleted":true}` | either direction |
| `GET` | `/v1/datagram-associations` | `200 NativeDatagramAssociationViewV1[]` | `state.datagram_associations()` (`control.rs:603`) |
| `GET` | `/v1/datagram-associations/{id}` | `200` / `400` non-integer / `404` | `datagram association id must be an integer` |
| `DELETE` | `/v1/datagram-associations/{id}` | `200 {"id":N,"terminated":true}` / `404` | administrative termination (`control.rs:613`) |

`docs/control-plane.md:18-30` carries the same inventory in prose.

## 2. Schema-v1 TOML config

Types in `crates/eggchaos-server/src/config.rs` and the `eggchaos-protocol`
crate (`stream.rs`, `scenario_v2.rs`, `common.rs`, `routes.rs`); re-exported by
`crates/eggchaos-server/src/lib.rs:13-38` for source compatibility.

### 2.1 `NativeConfig`

```toml
version = 1
seed = 7

[admin]
bind = "127.0.0.1:18475"

[[proxy]]
name = "smoke"
listen = "127.0.0.1:0"
upstream = "127.0.0.1:1"
enabled = true
```

Real example: `qualification/release/eggchaos.toml:1-17` (the stream stanza
above verbatim, plus a `[[datagram_proxies]]` `udp-smoke` entry with
`max_associations = 8`).

- `version: u32` — must be `1`, else `NativeConfigError::Version`
  (`config.rs:186-188`).
- `seed: u64` — `#[serde(default)]`, i.e. `0` when omitted. Feeds
  `ServiceBuilder::new(seed)` (`main.rs:839`; builder at
  `runtime/mod.rs:412-552`).
- `admin: AdminFileConfig`, optional `runtime: RuntimeConfigV1`, and
  `proxies: Vec<ProxyFileConfig>` under `[[proxy]]`, plus optional
  `datagram_proxies: Vec<DatagramProxyFileConfig>` (§2.6).

### 2.2 `AdminFileConfig` (`config.rs:38-47`)

| Field | Type | Default |
| --- | --- | --- |
| `bind` | `SocketAddr` | `127.0.0.1:8475` via `default_admin_bind()` (`config.rs:74-76`) |
| `public_admin` | `bool` | `false` |
| `auth_token` | `Option<String>` | `None` |

Loopback-by-default is enforced at `NativeAdmin::start`, not at parse time.
Prefer env/file indirection for the token in deployment (field comment,
`config.rs:45`). `Debug` redacts the token (`config.rs:49-60`).

### 2.3 `ProxyFileConfig` (`config.rs:90-111`)

| Field | Type | Default / notes |
| --- | --- | --- |
| `name` | `String` | required; `1..=128` bytes, `[A-Za-z0-9._-]+` (`validate_proxy_name`, `stream.rs:112-123`, mirroring `ProxySpec::validate`, `runtime/mod.rs:127`) |
| `listen` | `SocketAddr` | required; port `0` = ephemeral |
| `upstream` | `SocketAddr` | required; fixed target |
| `connect_timeout_ms` | `u64` milliseconds | default `5000` (`NATIVE_DEFAULT_PROXY_TIMEOUT_MS`) |
| `seed` | `u64` | default `0` |
| `enabled` | `bool` | default `true` (`default_true`) |
| `max_connections` | `Option<usize>` | default `None` (no per-proxy cap) |
| `fault` | `Vec<FaultFileConfig>` | `[[proxy.fault]]`, default empty |

Schema-v1 TOML and native API/CLI use the same `connect_timeout_ms` spelling
and millisecond unit. Omitted config values retain the former 5-second
default.

### 2.4 `FaultFileConfig` (`config.rs:123-159`)

Human-friendly units; `direction: Direction` (`upstream` = client→target,
`downstream` = target→client), `type: String` (native name), plus:

| TOML key | Unit/type | Default |
| --- | --- | --- |
| `probability` | `f64` finite `0..=1` | `1.0` |
| `delay` | duration string `"<n>ms\|us\|s"` | `ZERO` (absent) |
| `jitter` | duration string | `ZERO` |
| `max_buffer_bytes` | `u64`, `1..=67108864` | `65536` |
| `bytes_per_second` | `u64`, non-zero | `1` |
| `burst_bytes` | `u64` | `64*1024` |
| `bytes` | `u64`, non-zero | `1` |
| `average_size` | `u64`, non-zero | `1024` |
| `variation` | `u64` | `0` |
| `hard_reset` | `bool` | `false` |
| `loss_rate` | `f64` finite `[0,1]` | required for `stream-loss`, else unused |
| `correlation` | `f64` finite `[0,1]` | required for `stream-loss`, else unused |

The service half-close policy is not configurable in schema v1; the runtime
continues to use its existing `Drain` setting until the policy has a stable
operator contract.

### 2.5 `RuntimeConfigV1` (+ `[runtime.datagram]`)

`RuntimeConfigV1` (`stream.rs:468-487`) — optional `[runtime]` bounds preserve
existing defaults when omitted:

| TOML key | Unit | Default | Accepted range |
| --- | --- | --- | --- |
| `global_connections` | active connections | `1024` | `1..=1000000` |
| `history` | retained closed records | `256` | `0..=1000000` |
| `relay_buffer_bytes` | bytes | `65536` | `1..=16777216` |
| `termination_grace_ms` | milliseconds | `5000` | `0..=300000` |

`[runtime.datagram]` (`DatagramRuntimeConfigV1`, `stream.rs:489-531`;
assembled by `runtime_datagram_limits`, `native.rs:116-128`):

| TOML key | Default |
| --- | --- |
| `max_proxies` | `128` |
| `max_associations` | `4096` |
| `history` | `1024` |
| `ingress_per_association` | `16` |
| `max_ingress_queue_bytes` | `67108864` (64 MiB) |

Native HTTP fault durations use integer nanoseconds. Fault `kind.type` spellings
are kebab-case and independent of core enum Serde. The pre-release API
migration from the prior unpublished Rust enum tags/`Duration` objects and
proxy-create `connect_timeout` spelling is documented in
`docs/control-plane.md`.

Duration parser (`config.rs:514-533`): trims, requires `ms`/`us`/`s`
suffix, `u64` number, checked `nanos` multiply. Anything else is
`NativeConfigError::Field`. Nanosecond conversion overflow is a field
error (`to_ns`, `config.rs:426-431`).

`type` mapping (`config.rs:432-496`):

- `latency` → `Latency{delay, jitter, max_buffer_bytes: 64 KiB fixed}`.
- `bandwidth` → `Bandwidth{bytes_per_second!, burst_bytes}`.
- `blackhole`\|`timeout` → `Blackhole{close_after: delay-as-duration?}`.
- `limit_data`\|`limit-data` → `LimitData{bytes!}`.
- `slow_close`\|`slow-close` → `SlowClose{delay}`.
- `slicer`\|`slice` → `Slice{average_size!, variation, delay}`.
- `disconnect`\|`reset_peer` → `Disconnect{after: delay, hard_reset}`.
- `stream-loss` → `StreamLoss{loss_rate, correlation}` (ADR 007; both
  required and must be finite `[0, 1]`, `config.rs:476-489`, validated by
  `FaultPlan`).
- else `Field{fault.type, unsupported type ...}`.

`!` = `None`/zero rejected as `Field`. Core range rules (bounded IDs,
finite probability, `variation < average_size`, non-zero capacities) come
from `eggchaos-core::FaultPlan` validation; adapters must not re-implement
it (`docs/configuration.md:3-17`).

### 2.6 `DatagramProxyFileConfig` (`config.rs:302-325`)

Optional `[[datagram_proxies]]` collection (M022); absence preserves existing
behavior. `#[serde(deny_unknown_fields)]`; directional faults are inline
TOML arrays of the native datagram DTO (`upstream_faults` /
`downstream_faults: Vec<DatagramFaultSpecV1>`), compiled through the same
`datagram_proxy_request_into_spec` authority as HTTP (`config.rs:343-365`).

| TOML key | Default |
| --- | --- |
| `name` / `listen` / `upstream` | required (same name rule as §2.3) |
| `max_associations` | `256` |
| `association_idle_timeout_ms` | `60000` |
| `max_queued_datagrams` | `1024` |
| `max_queued_bytes` | `4194304` (4 MiB) |
| `max_datagram_size` | `65507` |
| `seed` | `0` |

Wire bounds: idle timeout `1..=86400000` ms, non-zero queue limits validated
by `DatagramQueueLimits`, fault IDs unique across directions
(`stream.rs:1060-1114`).

### 2.7 `parse()` / `compile_*()` validation path

`NativeConfig::parse` (`config.rs:184-266`):

1. `toml::from_str` → `Toml` error on bad syntax.
2. `version != 1` → `Version`.
3. `proxies.len() > 1024` → `Field{proxy, too many proxies}`.
4. `datagram_proxies.len() > 128` → `Field{datagram_proxies, ...}`, plus
   count-vs-`runtime.datagram.max_proxies` cap (`config.rs:227-232`).
5. `runtime` admission/relay/grace/datagram bound validation
   (`config.rs:201-226`).
6. Duplicate `proxy.name` → `Field{proxy.name, duplicate proxy ...}`;
   duplicate datagram names likewise.
7. `proxy.compile()?` / datagram `compile()?` + global-association
   validation for every proxy (fail-fast, no partial result).

`compile_proxies()` (`config.rs:272-274`) maps `ProxyFileConfig::compile`
(`config.rs:369-401`): partition faults by `direction`, `FaultFileConfig::compile`
each (`FaultId::new`, `Probability::new`, kind match), then
`FaultPlan::new(upstream/downstream)` — core duplicate-ID / semantic
validation surfaces as `Field{fault, ...}`. `compile_datagram_proxies()`
(`config.rs:276-299`) is the datagram sibling with the same fail-fast shape.
`NativeConfig::load` (`config.rs:268-270`) is `read_to_string` + `parse`.

Unit coverage: `config.rs:536-719` (versioned TOML compiles; datagram
defaults/faults; invalid datagram bounds/addresses rejected; duplicate names
rejected; zero burst is a field error without panicking; token redaction;
runtime/proxy omission defaults; zero buffers/timeout rejected).

## 3. CLI (`eggchaos` binary)

Crate: `crates/eggchaos-cli/Cargo.toml:1-25` — `eggchaos-cli` depends on
`eggchaos-server` (path), `eggfetch-core` (workspace), `clap`,
`serde_json`, `tokio`, `toml`; binary name `eggchaos` at `src/main.rs`.
`#![forbid(unsafe_code)]` (`main.rs:1`).

Global flags (`main.rs:13-31`):

- `--admin <url>`, default `http://127.0.0.1:8475`.
- `--admin-token <token>` overrides `EGGCHAOS_ADMIN_TOKEN` (clap `env`);
  omitted means no Authorization header.
- `--json`: emit one machine-readable JSON document; also switches error
  shape (see §3.3).

### 3.1 Command matrix

`Command` enum (`main.rs:33-73`); dispatch in `dispatch()` (`main.rs:355-722`).

| CLI | Admin call | Flags / body |
| --- | --- | --- |
| `serve --config <path>` (default `eggchaos.toml`) | no HTTP; local boot (§3.4) | |
| `version` | none (local `CARGO_PKG_VERSION`) | prints `{"version":..,"api":"v1"}` |
| `reset` | `POST /v1/reset` | |
| `proxy list` | `GET /v1/proxies` | |
| `proxy get <name>` | `GET /v1/proxies/{name}` | |
| `proxy add <name> --listen --upstream [--max-connections] [--connect-timeout-ms] [--seed] [--disabled]` | `POST /v1/proxies` | `listen`, `upstream` required `SocketAddr`; body uses `connect_timeout_ms` (default 5000), optional `max_connections` and `seed`; `--disabled` registers without starting |
| `proxy set <name> [--listen] [--upstream] [--max-connections \| --clear-max-connections] [--connect-timeout-ms] [--enable \| --disable]` | `PATCH /v1/proxies/{name}` | at least one field required else `proxy set requires at least one field to change`; `enable`/`disable` map to `{"enabled":bool}`; `clear_max_connections` sends `"max_connections":null`; timeout key is `connect_timeout_ms` (`main.rs:482-529`) |
| `proxy remove <name>` | `DELETE /v1/proxies/{name}` | |
| `proxy enable <name>` | `PATCH ... {"enabled":true}` | |
| `proxy disable <name>` | `PATCH ... {"enabled":false}` | |
| `fault list <proxy>` | `GET /v1/proxies/{proxy}/faults` | |
| `fault get <proxy> <id>` | `GET .../faults/{id}` | ID percent-encoded (`encode_path_component`, `main.rs:763-774`) |
| `fault add <proxy> <id> [--direction] [--probability] --kind + kind params` | `POST .../faults` | `direction` default `downstream`, validated to `upstream\|downstream` (`check_direction`, `main.rs:756-761`); `probability` default `1.0`; `kind` **required** else `fault add requires --kind` (`main.rs:594`) |
| `fault set <proxy> <id> [--probability] [--kind + params]` | `PATCH .../faults/{id}` | requires `--probability` and/or `--kind` else `fault set requires --probability and/or --kind` (`main.rs:617-619`); only supplied fields sent |
| `fault remove <proxy> <id>` | `DELETE .../faults/{id}` | ID percent-encoded |
| `datagram proxy list` | `GET /v1/datagram-proxies` | (`main.rs:676-695`) |
| `datagram proxy get <name>` | `GET /v1/datagram-proxies/{name}` | |
| `datagram proxy add <name> --listen --upstream [--max-associations] [--association-idle-timeout-ms] [--max-queued-datagrams] [--max-queued-bytes] [--max-datagram-size] [--seed]` | `POST /v1/datagram-proxies` | defaults `256` / `60000` / `1024` / `4194304` / `65507` / `0` (`main.rs:220-238,680`) |
| `datagram proxy set <name> [--listen] [--upstream] [--max-associations] [--association-idle-timeout-ms] [--enable \| --disable]` | `PATCH /v1/datagram-proxies/{name}` | queue-size fields are create-only (no patch keys); empty patch → `datagram proxy set requires at least one field` (`main.rs:681-691`) |
| `datagram proxy enable/disable/remove <name>` | `PATCH {"enabled":true/false}` / `DELETE` | |
| `datagram fault list <proxy>` | `GET /v1/datagram-proxies/{proxy}/faults` | (`main.rs:696-714`) |
| `datagram fault get <proxy> <id>` | `GET .../faults/{id}` | ID percent-encoded |
| `datagram fault add <proxy> <id> [--direction] [--probability] --kind + kind params` | `POST .../faults` | `direction` default `upstream` (unlike stream `fault add`, which defaults `downstream`); `kind` is a required closed set (see below) |
| `datagram fault set <proxy> <id> --probability <f64>` | `PATCH .../faults/{id}` | probability-only patch (`main.rs:298-303`); no kind replacement, unlike stream `fault set` |
| `datagram fault remove <proxy> <id>` | `DELETE .../faults/{id}` | ID percent-encoded |
| `datagram association list` | `GET /v1/datagram-associations` | (`main.rs:715-719`) |
| `datagram association get <id:u64>` | `GET /v1/datagram-associations/{id}` | |
| `datagram association kill <id:u64>` | `DELETE /v1/datagram-associations/{id}` | |
| `connection list` | `GET /v1/connections` | |
| `connection get <id:u64>` | `GET /v1/connections/{id}` | |
| `connection kill <id:u64>` | `DELETE /v1/connections/{id}` | |
| `scenario apply <file>` | `POST /v1/scenarios/apply` | v1 JSON `ScenarioV1` or v2 JSON/TOML schedule (`.toml` extension selects TOML → shared DTO → JSON), at most 1 MiB (`read_scenario_document`, `main.rs:731-754`); CLI never expands phases or derives fingerprints |
| `scenario validate <file>` | `POST /v1/scenarios/validate` | v2 JSON/TOML file; fingerprint/identity only, creates no run |
| `scenario compile <file>` | `POST /v1/scenarios/compile` | v2 JSON/TOML file; normalized tape + fingerprint, creates no run |
| `scenario get <run_id:u64>` | `GET /v1/scenarios/{run_id}` | serves v1 and v2 runs |
| `scenario cancel <run_id:u64>` | `DELETE /v1/scenarios/{run_id}` | cancels v1 and v2 runs |
| `history` | `GET /v1/history` | |
| `metrics` | `GET /metrics` | raw text in human mode; `{"body":"..."}` in JSON mode |

`FaultParams` (`main.rs:124-156`): `--kind`, `--delay-ms`, `--jitter-ms`,
`--max-buffer-bytes`, `--bytes-per-second`, `--burst-bytes`,
`--close-after-ms`, `--bytes`, `--average-size`, `--variation`,
`--after-ms`, `--hard-reset` (bool flag), `--loss-rate`, `--correlation`.

`build_kind` (`main.rs:786-832`):

- `latency` requires `--delay-ms`; optional `--jitter-ms` (0),
  `--max-buffer-bytes` (65536). Emits `{"type":"latency","delay_ns":...,"jitter_ns":...,"max_buffer_bytes":...}`.
- `bandwidth`\|`bw` requires `--bytes-per-second`, `--burst-bytes`.
- `blackhole`\|`hole` optional `--close-after-ms` → nullable `close_after_ns`.
- `limit-data`\|`limit` requires `--bytes`.
- `slow-close`\|`slowclose` requires `--delay-ms`.
- `slice` requires `--average-size`; optional `--variation` (0),
  `--delay-ms` (0).
- `disconnect` optional `--after-ms` (0), `--hard-reset`.
- `stream-loss` requires `--loss-rate` and `--correlation` (both finite
  `[0, 1]`; deterministic userspace stream-chunk loss in fixed 32 KiB
  logical grains — explicitly not IP/TCP packet loss).
- else `unknown --kind ...`.
- All native fault durations are encoded as integer nanoseconds
  (`duration_ns`, `main.rs:776-780`). The
  `FaultKindV1` DTO owns the explicit lowercase `type` spelling and is reused
  by config conversion and native HTTP create/patch/response conversion.
- Missing required param → `--kind requires --<name>` (`required`,
  `main.rs:782-784`).

`datagram fault add` kinds (`main.rs:701-713`; DTO `DatagramFaultKindV1`,
`stream.rs:880-902`): `delay` (requires `--delay-ns`, optional
`--jitter-ns`), `loss`, `duplicate` (requires `--additional-copies`),
`reorder` (requires `--hold-ns`), `payload-corrupt` (requires `--bytes`),
`bandwidth` (requires `--bytes-per-second` + `--burst-bytes`); anything else
→ `kind must be delay, loss, duplicate, reorder, payload-corrupt, or
bandwidth`. Durations are already nanoseconds on this path (no ms scaling).

TOML accepts legacy `limit_data`, `slow_close`, `slicer`, `timeout`, and
`reset_peer` aliases at its parser edge (`config.rs:446-475`); the native
kebab-case forms (`limit-data`, `slow-close`, `slice`, `blackhole`,
`disconnect`) are canonical. Native HTTP always uses the explicit
kebab-case `FaultKindV1` schema; the CLI converts to this schema and never
serializes the core `FaultKind` enum.

### 3.2 `eggfetch-core` HTTP client path

`request()` (`main.rs:880-916`):

1. `Client::builder().build()`.
2. `client.get/post/patch/delete(&format!("{base}{path}"))?`; optional
   `.json(&body)?` for POST/PATCH.
3. If a token is configured, attach lowercase `authorization: Bearer <token>`
   (`main.rs:899-901`); explicit `--admin-token` overrides
   `EGGCHAOS_ADMIN_TOKEN` via clap.
4. `send().await`, `status()`, `bytes().await`. Transport errors are reported
   generically (`admin request failed`) so malformed credentials cannot appear
   in diagnostics.
5. Non-success status returns an error before printing; top-level dispatch
   emits one bounded CLI error document in JSON mode.
6. On success, `--json` emits compact JSON; human mode prints text/pretty JSON.
   Prometheus text becomes `{"body":"..."}` in JSON mode (lossy-UTF8 fallback
   for any non-JSON body, `main.rs:908-914`).

The CLI is a thin translator; like the Toxiproxy adapter it holds no proxy
state — every mutation goes through `ControlState` over HTTP.

### 3.3 `--json` contract and error shape

- Every JSON command emits **one** JSON document on stdout. `version`/`reset`/
  CRUD/scenario/history/metrics follow this; metrics wraps raw text. E2E asserts
  parseable single docs (the `cli()` helper forces `--json`,
  `cli_e2e.rs:9-22`; exercised across all four tests, `cli_e2e.rs:64-483`).
- Human mode prints pretty JSON for JSON responses and raw Prometheus text for
  metrics; `--json` prints compact JSON (`print_value`, `main.rs:918-924`).
- Top-level dispatch error (`main.rs:342-353`): `--json` prints
  `{"error":{"code":"request_failed","message":"..."}}` on stdout and exits
  `1`; otherwise `eprintln!("eggchaos: {error}")` and exits `1`.
- Server-side error payloads are not printed a second time on failure. The CLI
  wraps HTTP-status/transport failures as `request_failed`; nonzero exit on
  any failure (`docs/control-plane.md:80-82`).

### 3.4 `serve` boot sequence

`serve()` (`main.rs:834-878`):

```text
load → compile (stream + datagram) → ServiceBuilder → start → NativeAdmin → ctrl_c → shutdown/wait
```

1. `NativeConfig::load(path).await?` (TOML parse + §2.7 validation).
2. `config.compile_proxies()?` → `Vec<ProxySpec>` and
   `config.compile_datagram_proxies()?` → `Vec<DatagramProxySpec>`.
3. `ServiceBuilder::new(config.seed).proxy_all(...).datagram_proxy_all(...)`
   `.limits(...)` `.relay_buffer(...)` `.termination_grace(...)`
   `.datagram_limits(...).build()?` — duplicate-name + `ProxySpec::validate`
   + non-zero global limit (`runtime/mod.rs:412-552`: `proxy_all` 443,
   `datagram_proxy_all` 453, `limits` 461, `relay_buffer` 471,
   `termination_grace` 476, `datagram_limits` 481, `build` 486).
4. `service.start().await?` (`runtime/mod.rs:552`) — `ControlState::with_params`
   (`control.rs:52`), then `start_all` (`control.rs:721`): enabled →
   `create_proxy` (bind + supervise), disabled → `import_definition`
   (stored, `running:false`, `control.rs:689`); datagram mirrors via
   `create_datagram_proxy` (`control.rs:295`).
5. `NativeAdmin::start(AdminConfig{bind, public_admin, auth_token from file},
   handle.control_state()).await?`.
6. `eprintln!("eggchaos listening; admin={}", admin.local_addr())`.
7. `tokio::signal::ctrl_c().await?`, then `handle.shutdown();
   admin.shutdown(); handle.wait().await; admin.wait().await;`.

Shutdown cascades via cancellation tokens; `wait` joins supervised
listeners and scenario tasks.

## 4. Failure semantics

Typed in `ControlError` (`runtime/model.rs:278-317`); HTTP mapping in §1.3;
CLI pre-validation in `main.rs`.

- **Bind failures.** `create_proxy` (`control.rs:749`) binds **before**
  registering; failure returns `BindFailed{proxy, reason}` (`409`) and leaves
  no proxy. `start_stored` has the same guarantee (`control.rs:834`).
  Creation race after bind cancels the orphan supervisor and returns
  `Conflict`. Datagram creation (`control.rs:295`) shares the
  bind-before-register shape via the `DatagramRuntimeError::Bind →
  BindFailed` mapping (`control.rs:1-17`).
- **Restart-class updates.** `PATCH` with changed `listen`/`upstream` on a
  running proxy (`update_proxy`, `control.rs:954`) pre-binds the replacement
  first. Pre-bind failure keeps the old listener serving with spec untouched
  and returns `RestartFailed` (`500`) with a rollback note; the datagram
  sibling (`update_datagram_proxy`, `control.rs:352`) maps replacement bind
  failure to `RestartFailed` the same way (`control.rs:379`).
  `max_connections`/`connect_timeout` are live fields (apply to new
  connections); `connect_timeout_ms == 0` is `Invalid`.
- **Conflict / not-found / invalid.**
  - Duplicate proxy name or fault ID → `Conflict` (`409`)
    (`control.rs:749-...`; datagram `control.rs:295-...,440-...`).
  - Unknown proxy/fault/connection/scenario/association → `NotFound` (`404`).
  - Bad names/IDs (`1..=128`, charset), non-finite/out-of-range
    probability, zero timeouts, invalid plans → `Invalid` (`400`)
    (`stream.rs:112-123`; `ProxySpec::validate`, `runtime/mod.rs:127`).
  - Non-integer connection/scenario/association IDs → `Invalid` (`400`)
    (`admin.rs:594-619,689-732,440-467`).
  - Empty datagram proxy patch → `Invalid` (`400`) before any state access
    (`admin.rs:325-334`).
  - Stale-generation publications (scenario path) → `Conflict`, never silent
    overwrite (`publish_plans_expected`, `control.rs:1321`;
    `docs/control-plane.md:107-109`).
- **Patch validation (CLI side).**
  - `proxy set` with no field → local error `proxy set requires at least one
    field to change`, exit `1`, no request sent (`main.rs:482-491`).
  - `datagram proxy set` with no field → `datagram proxy set requires at
    least one field` (`main.rs:689`).
  - `fault add` without `--kind` → `fault add requires --kind`
    (`main.rs:594`); bad direction → `direction must be upstream or
    downstream, got ...` (`check_direction`, `main.rs:756-761`).
  - `fault set` without `--probability` and `--kind` → `fault set requires
    --probability and/or --kind` (`main.rs:617-619`).
- **Reset.** `reset()` (`control.rs:1553`): retains definitions +
  listen/upstream, enables every proxy, replaces all plans with empty
  (seed namespaces retained), terminates active connections/associations,
  exactly one generation. Proxies whose re-bind fails stay disabled and appear
  in `failed_enables: Vec<String>` / `failed_datagram_enables: Vec<String>`
  (`ResetReport`, `runtime/model.rs:263-274`) — still `200` with
  `{"generation":.., "reset":true, "failed_enables":[...],
  "failed_datagram_enables":[...]}`. Warns per proxy
  (`tracing::warn!`).
- **Enable/disable.** Disable stops the listener + terminates connections
  but retains definition and plans; enable rebinds via `start_stored`
  (`control.rs:834`) / `import_definition` (`control.rs:689`).
  Idempotent when already in the target state (returns current generation).

## 5. Review checklist

When reviewing this surface, confirm:

1. No state outside `ControlState` (+ its owned `DatagramRuntime`):
   admin/CLI/config/scenario paths are translators; no parallel listener
   registry.
2. Loopback default preserved; any non-loopback example or test sets both
   `public_admin` and `auth_token`, and auth failures return bounded JSON
   without echoing the token (`admin.rs:24-35,162-181`;
   `config.rs:49-60`).
3. All bounds present: 1 MiB request body (`admin.rs:106-122`,
   `common.rs:16`), 128 admin connections, bounded file proxy counts
   (`1024` stream / `128` datagram, `config.rs:189-200`), `max_proxies`
   count cap (`config.rs:227-232`), bounded histories and metric tables
   (TCP + datagram; association/client addresses never labels —
   asserted `admin.rs:973-982`), non-zero timeouts/rates/sizes/queue
   limits.
4. Status-code discipline: `201` only for `POST` create (stream +
   datagram proxies/faults), `202` only for scenario apply, `409` for
   bind/conflict, `500` only for restart-failed; unknown routes are
   `404 not_found`, bad JSON is `400 invalid_json`.
5. `GET` views pair plans with the generation/namespace from the same atomic
   snapshot (stream `control.rs:627-668`; datagram plan views
   `control.rs:400`); manual updates retain namespaces.
6. CLI `--json` emits exactly one parseable document per invocation and exits
   nonzero on failure; human mode never breaks the JSON shape, only
   whitespace.
7. `serve` follows load→compile→build→start→admin→`ctrl_c`→shutdown/wait; no
   `eggress-admin` HTTP dependency, no `eggress-outbound` MVP dependency,
   `eggfetch-core` minimal client only.
8. Datagram parity: no shadow registries — config (`config.rs:276-299`),
   HTTP (`admin.rs:287-467`), scenarios (v1 `set-datagram-plan` /
   `remove-datagram-fault`; v2 same actions), and CLI (`main.rs:676-720`)
   all convert into the shared `DatagramRuntime` authority.
9. No `unsafe`; `#![forbid(unsafe_code)]` (`main.rs:1`) /
   `#![deny(unsafe_code)]` (`lib.rs:3`, protocol `lib.rs:25`).

## 6. Verification

- Unit + e2e:
  ```sh
  cargo test -p eggchaos-cli
  ```
  Covers all four tests in `crates/eggchaos-cli/tests/cli_e2e.rs`:
  `cli_json_create_fault_kill_reset_end_to_end` (`:64-217`, boots
  `NativeAdmin` on `127.0.0.1:0` + echo origin, then via the built
  binary (`CARGO_BIN_EXE_eggchaos`) with `--admin <addr> --json`: `proxy
  add` → `fault add --kind latency --delay-ms 50` → hold TCP connection →
  `connection kill` → history appears → `metrics` wraps `{"body":...}` →
  `scenario apply/get/cancel` (v1 shape carries no v2 identity fields) →
  `reset` empties faults → `proxy get missing` exits nonzero; multi-thread
  runtime so blocking child waits do not freeze the admin under test);
  `cli_authenticates_and_round_trips_opaque_fault_ids` (`:219-281`, token
  failures stay single-JSON with no secret leakage, `part/percent%? ü`
  fault ID round-trips through percent-encoding);
  `datagram_cli_is_json_first_and_uses_native_routes` (`:283-346`,
  `datagram proxy add` → `datagram fault add --kind loss` → fault list →
  disable → remove, all one-JSON-doc);
  `cli_scenario_v2_validate_compile_apply_json_and_toml` (`:348-483`,
  v2 validate/compile/apply over JSON + TOML to the same fingerprint,
  run completion with scheduled/applied timing and `restore-initial`
  cleanup evidence).
- Admin unit: path-decode unit (`admin.rs:787-793`), token-redaction unit
  (`admin.rs:795-804`), health route via `eggfetch-core` client
  (`admin.rs:806-829`), datagram HTTP resources mutating the shared runtime
  incl. fault patch validation, metrics label bounds, and global reset
  (`admin.rs:831-1008`); config parse/duplicate/bounds tests
  (`config.rs:536-719`); native adapter parity tests (`native.rs:282-397`).
- Manual checks:
  ```sh
  eggchaos --admin http://127.0.0.1:8475 --json proxy list
  eggchaos --admin http://127.0.0.1:8475 --json fault list <proxy>
  eggchaos --admin http://127.0.0.1:8475 --json datagram proxy list
  curl -s http://127.0.0.1:8475/v1/health
  curl -s http://127.0.0.1:8475/metrics
  curl -s -X POST http://127.0.0.1:8475/v1/proxies -H 'content-type: application/json' -d '{"name":"p","listen":"127.0.0.1:0","upstream":"127.0.0.1:1"}'
  ```
  Expect single JSON docs, `201` on create, `{"error":{"code":...}}` +
  nonzero CLI exit on `missing` names.
- Lints/format once the workspace exists:
  ```sh
  cargo clippy --workspace --all-targets --all-features -- -D warnings
  cargo fmt --check
  cargo test --workspace --all-features
  ```
- Record any un-runnable oracle/platform as incomplete evidence; do not
  substitute inspection for execution (per `AGENTS.md` verification
  discipline).

## 7. Remote control SDKs (M033)

`bindings/python-client/` (stdlib-only `eggchaos_client`: sync `Client`
plus `AsyncClient` sharing one model/transport layer via
`asyncio.to_thread`) and `bindings/typescript-client/`
(`@eggstack/eggchaos-client`: `EggchaosClient` over injectable `fetch`
with `AbortSignal` support) cover all 36 `NATIVE_OPERATIONS`. Both ship
a generated operation/method table (`eggchaos_client/_generated.py`,
`src/generated.ts`) produced by `scripts/sync_sdk_contract.py` from the
M032 OpenAPI document: the script reads
`api/openapi/eggchaos-v1.yaml`, snapshots the shared
`bindings/_contract/operations.json`, and emits both tables from one
`OPERATION_METHODS` map (operationId → method name, e.g.
`getMetrics` → `metrics_text`); output is deterministic and CI fails on
regeneration diffs. Contract tests assert table equality with the shared
snapshot and that every operation resolves to a real client method.
`bindings/_contract/cross_language_fixtures.json` proves equivalent inputs
serialize to identical native JSON in both languages. Qualification:
`scripts/check_python_client.sh`, `scripts/check_typescript_client.sh`
(regeneration drift + unit, no server),
`scripts/qualify_language_clients.sh` (loopback server: equivalent
sync/async/TS flows incl. auth failure/token redaction, then
`python -m build` sdist/wheel and `npm pack` artifacts without
publication). No FFI, no daemon lifecycle management, no implicit mutation
retries; metrics stay text; bearer tokens never enter repr/errors/models.

## 8. Datagram v1 operator surface (M022)

Datagrams are explicit sibling resources, not `transport` fields on TCP
proxies. The full route inventory is §1.4's datagram family
(`GET/POST /v1/datagram-proxies`, `GET/PATCH/DELETE
/v1/datagram-proxies/{name}`, directional fault CRUD below `/faults`, and
`GET /v1/datagram-associations[/{id}]` plus association `DELETE` for
administrative termination); the CLI matrix is §3.1's `datagram` rows
(`proxy list|get|add|set|enable|disable|remove`,
`fault list|get|add|set|remove` with a probability-only `set`,
`association list|get|kill`). Native DTOs (`DatagramFaultKindV1`'s six
kinds, `stream.rs:869-902`; `NativeDatagramProxyRequestV1` defaults,
`stream.rs:992-1031`) convert into the M021 `DatagramRuntime`
(`runtime/datagram/`: `model.rs` limits/views, `registry.rs` lifecycle
authority, `association.rs` per-client upstream sockets); config
(`config.rs:276-299,302-365`), HTTP (`admin.rs:287-467`), scenario (v1 +
v2 `set-datagram-plan` / `remove-datagram-fault`), and CLI
(`main.rs:198-315,676-720`) retain no shadow registries. Schema v1 adds
the optional `datagram_proxies` collection (§2.6) and the
`[runtime.datagram]` global bounds (§2.5). `POST /v1/reset` resets both
resource families and reports TCP/datagram re-enable failures separately
(`failed_enables` + `failed_datagram_enables`, `model.rs:263-274`).
Metrics expose UDP association and evidence gauges with bounded `proxy`,
`direction`, `kind`, and fault-type labels; IDs and peer addresses are
never labels (asserted `admin.rs:973-982`). The `eggchaos datagram` CLI
stays an Eggfetch-backed HTTP adapter. Toxiproxy v2.12 remains unchanged
and TCP-only.
