# Control plane, config, and CLI

Back to [architecture overview](overview.md).

Evidence-first deep dive for the native control surface. Authority is code;
`docs/control-plane.md` and `docs/configuration.md` are summaries, not the
spec. All paths below are relative to the workspace root.

Sources: `crates/eggchaos-protocol/src/` (wire DTO + operation-inventory
authority), `api/openapi/eggchaos-v1.yaml` (mechanically drift-checked
contract), `crates/eggchaos-server/src/admin.rs`,
`crates/eggchaos-server/src/native.rs` (server adapters + compatibility
re-exports), `crates/eggchaos-server/src/native_v2.rs` (V2 re-exports),
`crates/eggchaos-server/src/runtime.rs`,
`crates/eggchaos-server/src/config.rs`,
`crates/eggchaos-server/src/lib.rs`,
`crates/eggchaos-cli/src/main.rs`,
`crates/eggchaos-cli/Cargo.toml`,
`crates/eggchaos-cli/tests/cli_e2e.rs`,
`docs/control-plane.md`, `docs/configuration.md`,
`qualification/release/eggchaos.toml`.

## 1. Native `/v1` admin API

### 1.1 Substrate

`crates/eggchaos-server/src/admin.rs:1-122` owns only listener setup, auth,
bounds, and route dispatch. Business state lives in `ControlState`
(`crates/eggchaos-server/src/runtime.rs`); `admin.rs` never stores proxies,
faults, or connections itself.

- HTTP substrate: `eggserve_primitives::{Request, RequestBodyPolicy,
  Response, ResponseBody, StatusCode}` plus
  `eggserve_server::{service_fn_with_policy, RuntimeConfig, Server,
  ServerHandle, ServiceError}`.
- Startup (`NativeAdmin::start`): `TcpListener::bind(config.bind)` then
  `Server::builder().runtime(runtime).from_listener(listener).build()` then
  `start_with_service(service)`. Returns `AdminHandle { local_addr, server }`
  with `shutdown()` / `wait()` (`admin.rs:56-78,80-122`).
- Body/connection bounds (`admin.rs:91-107,184-187`):
  - `RuntimeConfig { max_request_body_bytes: 1 MiB, max_connections: 128, .. }`.
  - `RequestBodyPolicy::Buffer { max_bytes: 1 MiB }`.
  - `body.read_all()` failure maps to `ServiceError::rejected(413, ...)`.
- The EggServe leaf H1 runtime owns parsing, body bounds, and connection
  lifecycle; eggchaos owns route dispatch + typed JSON conversion
  (`docs/control-plane.md:9-12`).

### 1.2 Auth, loopback policy

`AdminConfig` (`admin.rs:11-30`):

```rust
pub struct AdminConfig {
    pub bind: SocketAddr,        // default 127.0.0.1:8475
    pub public_admin: bool,      // default false
    pub auth_token: Option<String>, // default None
}
```

Gate (`admin.rs:86-89`):

```rust
if !config.bind.ip().is_loopback() && (!config.public_admin || config.auth_token.is_none()) {
    return Err(AdminError::InsecurePublicBind);
}
```

- Loopback binds need no token. Non-loopback requires **both**
  `public_admin=true` and `auth_token=Some(...)`, else
  `AdminError::InsecurePublicBind` (`admin.rs:32-50`).
- When a token is configured, `handle_request` (`admin.rs:157-181`) requires
  `Authorization: Bearer <token>` with constant-time comparison
  (`constant_time_equal`, `admin.rs:415-421`). Mismatch/missing returns
  `403` with `{"error":{"code":"unauthorized","message":"authorization required"}}`.
  The token is never echoed.

### 1.3 Error envelope and status mapping

Envelope (`admin.rs:124-132,423-432`):

```json
{"error": {"code": "<snake_case>", "message": "<human detail>"}}
```

All JSON responses use `content-type: application/json`.

`control_status` (`admin.rs:134-143`) + `ControlError::code`
(`runtime.rs:586-597`):

| `ControlError` | `code` | HTTP |
| --- | --- | --- |
| `NotFound(_)` | `not_found` | `404` |
| `Conflict(_)` | `conflict` | `409` |
| `BindFailed{..}` | `bind_failed` | `409` |
| `Invalid(_)` | `invalid` | `400` |
| `RestartFailed{..}` | `restart_failed` | `500` |

Additional envelope codes from `admin.rs:145-212`:

| Case | HTTP | `code` |
| --- | --- | --- |
| Malformed JSON body | `400` | `invalid_json` |
| Unknown route | `404` | `not_found` |
| Auth failure | `403` | `unauthorized` |
| Body over bound | `413` | via `ServiceError::rejected` |
| Serialization fallback | — | `serialization` |

`GET /metrics` is the exception: `200 text/plain; version=0.0.4`
(`text_response`, `admin.rs:434-441`).

### 1.4 Route inventory

Dispatch is `route()` (`admin.rs:225-413`). `segments` are `/`-split,
empty-filtered; proxy/fault names travel as single path segments.

| Method | Route | Success | Notes |
| --- | --- | --- | --- |
| `GET` | `/v1/health` | `200 {"running":true,"generation":N}` | generation from `ControlState::generation()` |
| `GET` | `/v1/version` | `200 {"version":"<cargo>","api":"v1"}` | `env!("CARGO_PKG_VERSION")` |
| `GET` | `/metrics` | `200` Prometheus text | no `/v1` prefix |
| `POST` | `/v1/reset` | `200 ResetReport` | see §4 |
| `GET` | `/v1/proxies` | `200 NativeProxyViewV1[]` | `state.list()` converted to native DTOs |
| `POST` | `/v1/proxies` | `201 {"proxy":view,"generation":N}` | body `NativeProxyRequestV1`; bind-before-register |
| `GET` | `/v1/proxies/{name}` | `200 NativeProxyViewV1` / `404` | |
| `PATCH` | `/v1/proxies/{name}` | `200 {"proxy":view,"generation":N}` | body `NativeProxyPatchV1`; restart-class for listen/upstream |
| `DELETE` | `/v1/proxies/{name}` | `200 {"generation":N,"deleted":true}` | stops listener, terminates conns |
| `GET` | `/v1/proxies/{name}/faults` | `200 {"upstream":[...],"downstream":[...]}` | from live snapshots |
| `POST` | `/v1/proxies/{name}/faults` | `201 {"direction":..,"fault":..,"generation":N}` | body `FaultUpsertV1`; lowercase typed `kind.type` |
| `GET` | `/v1/proxies/{name}/faults/{id}` | `200 {"direction":..,"fault":..}` | upstream searched first |
| `PATCH` | `/v1/proxies/{name}/faults/{id}` | `200 {"direction":..,"fault":..,"generation":N}` | body `FaultPatchV1`; same kind DTO as create |
| `DELETE` | `/v1/proxies/{name}/faults/{id}` | `200 {"generation":N,"deleted":true}` | either direction |
| `GET` | `/v1/connections` | `200 ConnectionSnapshot[]` | live evidence merged at read time |
| `GET` | `/v1/connections/{id}` | `200` / `400` non-integer / `404` | `connection id must be an integer` |
| `DELETE` | `/v1/connections/{id}` | `200 {"id":N,"terminated":true}` / `404` | `kill()`; level-triggered cancel |
| `GET` | `/v1/history` | `200 ClosedConnection[]` | bounded, newest last |
| `POST` | `/v1/scenarios/apply` | `202 ScenarioRunV1` / `202 ScheduleRunV2` | version-aware body: `version: 1` → `ScenarioV1`, `version: 2` → `ScenarioScheduleV2Dto`; invalid doc maps to `ControlError::Invalid` |
| `POST` | `/v1/scenarios/validate` | `200 ScheduleValidateV2` | body `ScenarioScheduleV2Dto`; fingerprint/identity only, creates no run |
| `POST` | `/v1/scenarios/compile` | `200 ScheduleCompileV2` | body `ScenarioScheduleV2Dto`; normalized tape + fingerprint, creates no run |
| `GET` | `/v1/scenarios/{run_id}` | `200 ScenarioRunV1` / `200 ScheduleRunV2` / `400` / `404` | serves both versions; run IDs share one namespace; lowercase status values |
| `DELETE` | `/v1/scenarios/{run_id}` | `200 ScenarioRunV1` / `200 ScheduleRunV2` / `400` / `404` | cancels both versions |

Scenario/metrics semantics belong to
`scenario-observability.md`; this file records only the route/status
contract. `docs/control-plane.md:14-26` carries the same inventory in prose.

## 2. Schema-v1 TOML config

Types in `crates/eggchaos-server/src/config.rs` and the `eggchaos-protocol`
crate (`stream.rs`, `scenario_v2.rs`, `common.rs`, `routes.rs`); re-exported by
`crates/eggchaos-server/src/lib.rs:11-13` for source compatibility.

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

Real example: `qualification/release/eggchaos.toml:1-11` (above, verbatim
modulo proxy addresses).

- `version: u32` — must be `1`, else `NativeConfigError::Version`
  (`config.rs:135-137`).
- `seed: u64` — `#[serde(default)]`, i.e. `0` when omitted. Feeds
  `ServiceBuilder::new(seed)` and the `(service seed XOR proxy seed)`
  namespace (`runtime.rs:138-142`).
- `admin: AdminFileConfig`, optional `runtime: RuntimeConfigV1`, and
  `proxies: Vec<ProxyFileConfig>` under `[[proxy]]`.

### 2.2 `AdminFileConfig` (`config.rs:28-53`)

| Field | Type | Default |
| --- | --- | --- |
| `bind` | `SocketAddr` | `127.0.0.1:8475` via `default_admin_bind()` |
| `public_admin` | `bool` | `false` |
| `auth_token` | `Option<String>` | `None` |

Loopback-by-default is enforced at `NativeAdmin::start`, not at parse time.
Prefer env/file indirection for the token in deployment (field comment,
`config.rs:37`).

### 2.3 `ProxyFileConfig` (`config.rs:55-76`)

| Field | Type | Default / notes |
| --- | --- | --- |
| `name` | `String` | required; `1..=128` bytes, `[A-Za-z0-9._-]+` (validated in `ProxySpec::validate`, `runtime.rs:104-120`) |
| `listen` | `SocketAddr` | required; port `0` = ephemeral |
| `upstream` | `SocketAddr` | required; fixed target |
| `enabled` | `bool` | default `true` (`default_true`) |
| `max_connections` | `Option<usize>` | default `None` (no per-proxy cap) |
| `connect_timeout_ms` | `u64` milliseconds | default `5000`, range `1..=300000` |
| `seed` | `u64` | default `0` |
| `fault` | `Vec<FaultFileConfig>` | `[[proxy.fault]]`, default empty |

Schema-v1 TOML and native API/CLI use the same `connect_timeout_ms` spelling
and millisecond unit. Omitted config values retain the former 5-second
default.

### 2.4 `FaultFileConfig` (`config.rs:78-112`)

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

The service half-close policy is not configurable in schema v1; the runtime
continues to use its existing `Drain` setting until the policy has a stable
operator contract.

### 2.5 `RuntimeConfigV1`

Optional `[runtime]` bounds preserve existing defaults when omitted:

| TOML key | Unit | Default | Accepted range |
| --- | --- | --- | --- |
| `global_connections` | active connections | `1024` | `1..=1000000` |
| `history` | retained closed records | `256` | `0..=1000000` |
| `relay_buffer_bytes` | bytes | `65536` | `1..=16777216` |
| `termination_grace_ms` | milliseconds | `5000` | `0..=300000` |

Native HTTP fault durations use integer nanoseconds. Fault `kind.type` spellings
are kebab-case and independent of core enum Serde. The pre-release API
migration from the prior unpublished Rust enum tags/`Duration` objects and
proxy-create `connect_timeout` spelling is documented in
`docs/control-plane.md`.

Duration parser (`config.rs:285-304`): trims, requires `ms`/`us`/`s`
suffix, `u64` number, checked `nanos` multiply. Anything else is
`NativeConfigError::Field`.

`type` mapping (`config.rs:219-276`):

- `latency` → `Latency{delay, jitter, max_buffer_bytes: 64 KiB fixed}`.
- `bandwidth` → `Bandwidth{bytes_per_second!, burst_bytes}`.
- `blackhole`\|`timeout` → `Blackhole{close_after: delay-as-duration?}`.
- `limit_data` → `LimitData{bytes!}`.
- `slow_close` → `SlowClose{delay}`.
- `slicer`\|`slice` → `Slice{average_size!, variation, delay}`.
- `disconnect`\|`reset_peer` → `Disconnect{after: delay, hard_reset}`.
- else `Field{fault.type, unsupported type ...}`.

`!` = `None`/zero rejected as `Field`. Core range rules (bounded IDs,
finite probability, `variation < average_size`, non-zero capacities) come
from `eggchaos-core::FaultPlan` validation; adapters must not re-implement
it (`docs/configuration.md:3-12`).

### 2.5 `parse()` / `compile_proxies()` validation path

`NativeConfig::parse` (`config.rs:131-155`):

1. `toml::from_str` → `Toml` error on bad syntax.
2. `version != 1` → `Version`.
3. `proxies.len() > 1024` → `Field{proxy, too many proxies}`.
4. Duplicate `proxy.name` → `Field{proxy.name, duplicate proxy ...}`.
5. `proxy.compile()?` for every proxy (fail-fast, no partial result).

`compile_proxies()` (`config.rs:161-163`) maps `ProxyFileConfig::compile`
(`config.rs:166-195`): partition faults by `direction`, `FaultFileConfig::compile`
each (`FaultId::new`, `Probability::new`, kind match), then
`FaultPlan::new(upstream/downstream)` — core duplicate-ID / semantic
validation surfaces as `Field{fault, ...}`. `NativeConfig::load`
(`config.rs:157-159`) is `read_to_string` + `parse`.

Unit coverage: `config.rs:306-350` (versioned TOML compiles; duplicate names
rejected).

## 3. CLI (`eggchaos` binary)

Crate: `crates/eggchaos-cli/Cargo.toml:1-21` — `eggchaos-cli` depends on
`eggchaos-server` (path), `eggfetch-core`, `clap`, `serde_json`, `tokio`,
`toml`; binary name `eggchaos` at `src/main.rs`. `#![forbid(unsafe_code)]`
(`main.rs:1`).

Global flags (`main.rs:9-24`):

- `--admin <url>`, default `http://127.0.0.1:8475`.
- `--admin-token <token>` overrides `EGGCHAOS_ADMIN_TOKEN`; omitted means no
  Authorization header.
- `--json`: emit one machine-readable JSON document; also switches error
  shape (see §3.3).

### 3.1 Command matrix

`Command` enum (`main.rs:26-52`); dispatch in `dispatch()` (`main.rs:188-406`).

| CLI | Admin call | Flags / body |
| --- | --- | --- |
| `serve --config <path>` (default `eggchaos.toml`) | no HTTP; local boot (§3.4) | |
| `version` | none (local `CARGO_PKG_VERSION`) | prints `{"version":..,"api":"v1"}` |
| `reset` | `POST /v1/reset` | |
| `proxy list` | `GET /v1/proxies` | |
| `proxy get <name>` | `GET /v1/proxies/{name}` | |
| `proxy add <name> --listen --upstream [--max-connections] [--connect-timeout-ms] [--seed] [--disabled]` | `POST /v1/proxies` | `listen`, `upstream` required `SocketAddr`; body uses `connect_timeout_ms` (default 5000), optional `max_connections` and `seed` |
| `proxy set <name> [--listen] [--upstream] [--max-connections \| --clear-max-connections] [--connect-timeout-ms] [--enable \| --disable]` | `PATCH /v1/proxies/{name}` | at least one field required else `proxy set requires at least one field to change`; `enable`/`disable` map to `{"enabled":bool}`; `clear_max_connections` sends `"max_connections":null`; timeout key is `connect_timeout_ms` (`main.rs:228-284`) |
| `proxy remove <name>` | `DELETE /v1/proxies/{name}` | |
| `proxy enable <name>` | `PATCH ... {"enabled":true}` | |
| `proxy disable <name>` | `PATCH ... {"enabled":false}` | |
| `fault list <proxy>` | `GET /v1/proxies/{proxy}/faults` | |
| `fault get <proxy> <id>` | `GET .../faults/{id}` | |
| `fault add <proxy> <id> [--direction] [--probability] --kind + kind params` | `POST .../faults` | `direction` default `downstream`, validated to `upstream\|downstream`; `probability` default `1.0`; `kind` **required** else `fault add requires --kind` (`main.rs:330-352`) |
| `fault set <proxy> <id> [--probability] [--kind + params]` | `PATCH .../faults/{id}` | requires `--probability` and/or `--kind` else `fault set requires --probability and/or --kind`; only supplied fields sent (`main.rs:353-377`) |
| `fault remove <proxy> <id>` | `DELETE .../faults/{id}` | |
| `connection list` | `GET /v1/connections` | |
| `connection get <id:u64>` | `GET /v1/connections/{id}` | |
| `connection kill <id:u64>` | `DELETE /v1/connections/{id}` | |
| `scenario apply <file>` | `POST /v1/scenarios/apply` | v1 JSON `ScenarioV1` or v2 JSON/TOML schedule (`.toml` extension selects TOML → shared DTO → JSON), at most 1 MiB; CLI never expands phases or derives fingerprints |
| `scenario validate <file>` | `POST /v1/scenarios/validate` | v2 JSON/TOML file; fingerprint/identity only, creates no run |
| `scenario compile <file>` | `POST /v1/scenarios/compile` | v2 JSON/TOML file; normalized tape + fingerprint, creates no run |
| `scenario get <run_id:u64>` | `GET /v1/scenarios/{run_id}` | serves v1 and v2 runs |
| `scenario cancel <run_id:u64>` | `DELETE /v1/scenarios/{run_id}` | cancels v1 and v2 runs |
| `history` | `GET /v1/history` | |
| `metrics` | `GET /metrics` | raw text in human mode; `{"body":"..."}` in JSON mode |

`FaultParams` (`main.rs:101-129`): `--kind`, `--delay-ms`, `--jitter-ms`,
`--max-buffer-bytes`, `--bytes-per-second`, `--burst-bytes`,
`--close-after-ms`, `--bytes`, `--average-size`, `--variation`,
`--after-ms`, `--hard-reset` (bool flag).

`build_kind` (`main.rs`):

- `latency` requires `--delay-ms`; optional `--jitter-ms` (0),
  `--max-buffer-bytes` (65536). Emits `{"type":"latency","delay_ns":...,"jitter_ns":...,"max_buffer_bytes":...}`.
- `bandwidth`\|`bw` requires `--bytes-per-second`, `--burst-bytes`.
- `blackhole`\|`hole` optional `--close-after-ms` → nullable `close_after_ns`.
- `limit-data`\|`limit` requires `--bytes`.
- `slow-close`\|`slowclose` requires `--delay-ms`.
- `slice` requires `--average-size`; optional `--variation` (0),
  `--delay-ms` (0).
- `disconnect` optional `--after-ms` (0), `--hard-reset`.
- else `unknown --kind ...`.
- All native fault durations are encoded as integer nanoseconds. The
  `FaultKindV1` DTO owns the explicit lowercase `type` spelling and is reused
  by config conversion and native HTTP create/patch/response conversion.
- Missing required param → `--kind requires --<name>` (`required`,
  `main.rs:419-421`).

TOML accepts legacy `limit_data`/`slow_close`, `slicer`, `timeout`, and
`reset_peer` aliases at its parser edge. Native HTTP always uses the explicit
kebab-case `FaultKindV1` schema; the CLI converts to this schema and never
serializes the core `FaultKind` enum.

### 3.2 `eggfetch-core` HTTP client path

`request()` (`main.rs:487-519`):

1. `Client::builder().build()`.
2. `client.get/post/patch/delete(&format!("{base}{path}"))?`; optional
   `.json(&body)?` for POST/PATCH.
3. If a token is configured, attach `Authorization: Bearer <token>`; explicit
   `--admin-token` overrides `EGGCHAOS_ADMIN_TOKEN`.
4. `send().await`, `status()`, `bytes().await`. Transport errors are reported
   generically so malformed credentials cannot appear in diagnostics.
5. Non-success status returns an error before printing; top-level dispatch
   emits one bounded CLI error document in JSON mode.
6. On success, `--json` emits compact JSON; human mode prints text/pretty JSON.
   Prometheus text becomes `{"body":"..."}` in JSON mode.

The CLI is a thin translator; like the Toxiproxy adapter it holds no proxy
state — every mutation goes through `ControlState` over HTTP.

### 3.3 `--json` contract and error shape

- Every JSON command emits **one** JSON document on stdout. `version`/`reset`/
  CRUD/scenario/history/metrics follow this; metrics wraps raw text. E2E asserts
  parseable single docs (`cli_e2e.rs:9-21,99-143`).
- Human mode prints pretty JSON for JSON responses and raw Prometheus text for
  metrics; `--json` prints compact JSON.
- Top-level dispatch error (`main.rs:172-186`): `--json` prints
  `{"error":{"code":"request_failed","message":"..."}}` on stdout and exits
  `1`; otherwise `eprintln!("eggchaos: {error}")` and exits `1`.
- Server-side error payloads are not printed a second time on failure. The CLI
  wraps HTTP-status/transport failures as `request_failed`; nonzero exit on
  any failure (`docs/control-plane.md`).

### 3.4 `serve` boot sequence

`serve()` (`main.rs:462-485`):

```text
load → compile → ServiceBuilder → start → NativeAdmin → ctrl_c → shutdown/wait
```

1. `NativeConfig::load(path).await?` (TOML parse + §2.5 validation).
2. `config.compile_proxies()?` → `Vec<ProxySpec>`.
3. `ServiceBuilder::new(config.seed).proxy_all(proxies).build()?`
   — duplicate-name + `ProxySpec::validate` + non-zero global limit
   (`runtime.rs:2205-2229`).
4. `service.start().await?` — `ControlState::with_params`, then per proxy:
   enabled → `create_proxy` (bind + supervise), disabled →
   `import_definition` (stored, `running:false`) (`runtime.rs:2244-2255`).
5. `NativeAdmin::start(AdminConfig{bind, public_admin, auth_token from file},
   handle.control_state()).await?`.
6. `eprintln!("eggchaos listening; admin={}", admin.local_addr())`.
7. `tokio::signal::ctrl_c().await?`, then `handle.shutdown();
   admin.shutdown(); handle.wait().await; admin.wait().await;`.

Shutdown cascades via cancellation tokens; `wait` joins supervised
listeners and scenario tasks (`runtime.rs:2113-2138`).

## 4. Failure semantics

Typed in `ControlError` (`runtime.rs:555-597`); HTTP mapping in §1.3;
CLI pre-validation in `main.rs`.

- **Bind failures.** `create_proxy` binds **before** registering; failure
  returns `BindFailed{proxy, reason}` (`409`) and leaves no proxy
  (`runtime.rs:1248-1280`). `start_stored` has the same guarantee
  (`runtime.rs:1333-1346`). Creation race after bind cancels the orphan
  supervisor and returns `Conflict` (`runtime.rs:1301-1319`).
- **Restart-class updates.** `PATCH` with changed `listen`/`upstream` on a
  running proxy pre-binds the replacement first. Pre-bind failure keeps the
  old listener serving with spec untouched and returns `RestartFailed`
  (`500`) with `replacement bind failed (...); old listener still serving`
  (`runtime.rs:1531-1550`). `max_connections`/`connect_timeout` are live
  fields (apply to new connections); `connect_timeout_ms == 0` is `Invalid`.
- **Conflict / not-found / invalid.**
  - Duplicate proxy name or fault ID → `Conflict` (`409`):
    `proxy {name} already exists`, `fault {id} already exists on {proxy}/{dir}`
    (`runtime.rs:1260-1266,1646-1653`).
  - Unknown proxy/fault/connection/scenario → `NotFound` (`404`).
  - Bad names/IDs (`1..=128`, charset), non-finite/out-of-range
    probability, zero timeouts, invalid plans → `Invalid` (`400`)
    (`runtime.rs:104-133,1590-1618,1670-1684`).
  - Non-integer connection/scenario IDs → `Invalid` (`400`)
    (`admin.rs:342-367,389-410`).
  - Stale-generation publications (scenario path) → `Conflict`, never silent
    overwrite (`runtime.rs:1830-1878`; `docs/control-plane.md:47-49`).
- **Patch validation (CLI side).**
  - `proxy set` with no field → local error `proxy set requires at least one
    field to change`, exit `1`, no request sent (`main.rs:238-247`).
  - `fault add` without `--kind` → `fault add requires --kind`
    (`main.rs:337`); bad direction → `direction must be upstream or
    downstream` (`main.rs:408-413`).
  - `fault set` without `--probability` and `--kind` → `fault set requires
    --probability and/or --kind` (`main.rs:359-361`).
- **Reset.** `reset()` (`runtime.rs:2058-2111`): retains definitions +
  listen/upstream, enables every proxy, replaces all plans with empty
  (seed namespaces retained), terminates active connections, exactly one
  generation. Proxies whose re-bind fails stay disabled and appear in
  `failed_enables: Vec<String>` — still `200` with `{"generation":..,
  "reset":true, "failed_enables":[...]}`. Warns per proxy
  (`tracing::warn!`).
- **Enable/disable.** Disable stops the listener + terminates connections
  but retains definition and plans; enable rebinds via `start_stored`.
  Idempotent when already in the target state (returns current generation)
  (`runtime.rs:1405-1445`).

## 5. Review checklist

When reviewing this surface, confirm:

1. No state outside `ControlState`: admin/CLI/config/scenario paths are
   translators; no parallel listener registry (`runtime.rs:849-853`).
2. Loopback default preserved; any non-loopback example or test sets both
   `public_admin` and `auth_token`, and auth failures return bounded JSON
   without echoing the token.
3. All bounds present: 1 MiB request body, 128 admin connections, bounded
   proxy counts (`1024` file / `MAX_METRIC_PROXIES`), bounded histories and
   metric tables, non-zero timeouts/rates/sizes.
4. Status-code discipline: `201` only for `POST` create, `202` only for
   scenario apply, `409` for bind/conflict, `500` only for restart-failed;
   unknown routes are `404 not_found`, bad JSON is `400 invalid_json`.
5. `GET` views pair plans with the generation/namespace from the same atomic
   snapshot (`RuntimeInner::view_of`, `runtime.rs:908-931`); manual updates
   retain namespaces.
6. CLI `--json` emits exactly one parseable document per invocation and exits
   nonzero on failure; human mode never breaks the JSON shape, only
   whitespace.
7. `serve` follows load→compile→build→start→admin→`ctrl_c`→shutdown/wait; no
   `eggress-admin` HTTP dependency, no `eggress-outbound` MVP dependency,
   `eggfetch-core` minimal client only.
8. No `unsafe`; `#![forbid(unsafe_code)]` (`main.rs:1`) /
   `#![deny(unsafe_code)]` (`lib.rs:3`).

## 6. Verification

- Unit + e2e:
  ```sh
  cargo test -p eggchaos-cli
  ```
  Covers `crates/eggchaos-cli/tests/cli_e2e.rs:46-151`:
  boots `NativeAdmin` on `127.0.0.1:0` + echo origin, then via the built
  binary (`CARGO_BIN_EXE_eggchaos`) with `--admin <addr> --json`: `proxy
  add` → `fault add --kind latency --delay-ms 50` → hold TCP connection →
  `connection kill` → `reset` empties faults → `proxy get missing` exits
  nonzero. Multi-thread runtime so blocking child waits do not freeze the
  admin under test.
- Admin unit: health route via `eggfetch-core` client
  (`admin.rs:450-479`); config parse/duplicate tests
  (`config.rs:306-350`).
- Manual checks:
  ```sh
  eggchaos --admin http://127.0.0.1:8475 --json proxy list
  eggchaos --admin http://127.0.0.1:8475 --json fault list <proxy>
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

## Datagram v1 operator surface (M022)

Datagrams are explicit sibling resources, not `transport` fields on TCP
proxies. The route inventory is `GET/POST /v1/datagram-proxies`,
`GET/PATCH/DELETE /v1/datagram-proxies/{name}`, directional fault CRUD below
`/faults`, and `GET /v1/datagram-associations[/{id}]` plus association `DELETE`
for administrative termination. Native DTOs describe the six M020 fault
kinds and convert into the M021 `DatagramRuntime`; config, HTTP, scenario, and
CLI do not retain shadow registries. Schema v1 adds the optional
`datagram_proxies` collection and `[runtime.datagram]` global bounds.
`POST /v1/reset` now resets both resource families and reports TCP/datagram
re-enable failures separately. Metrics expose UDP association and evidence
gauges with bounded `proxy`, `direction`, `kind`, and fault-type labels; IDs
and peer addresses are never labels. The `eggchaos datagram` CLI stays an
Eggfetch-backed HTTP adapter. Toxiproxy v2.12 remains unchanged and TCP-only.
