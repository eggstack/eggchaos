# Toxiproxy v2.12 compatibility adapter

Part of [Eggchaos architecture overview](overview.md) (§5:
`eggchaos-toxiproxy`). For the user-facing contract see
`docs/toxiproxy.md`; for the qualification contract see
`plans/reference/toxiproxy-parity.md`.

Crate: `crates/eggchaos-toxiproxy/` (`Cargo.toml`: depends on
`eggchaos-core`, `eggchaos-server`, `eggserve-primitives`,
`eggserve-server`; dev-depends on `eggfetch-core` for tests).
Implementation: `crates/eggchaos-toxiproxy/src/lib.rs` (1976 lines at HEAD).
Standalone server: `crates/eggchaos-toxiproxy/examples/compat_server.rs`
(48 lines; bind + optional profile arg).
Differential corpora: `crates/eggchaos-toxiproxy/tests/differential.rs`
(strict v2.12) + `crates/eggchaos-toxiproxy/tests/post_v212_differential.rs`
(snapshot `packet_loss`).
Oracle baseline: `qualification/toxiproxy-v2-12/oracle-baseline-v2.12.0.md`.
Qualification runners: `scripts/qualify_toxiproxy_v2_12.sh` (strict) +
`scripts/qualify_toxiproxy_post_v2_12.sh` (snapshot, consumes the fetcher
via explicit `--path-only`).
Fetchers: `scripts/fetch_toxiproxy_v2_12.sh` +
`scripts/fetch_toxiproxy_post_v2_12.sh` (M041 stdout contract).
Latest corrective authority: M041 (`724b967`, closure
`plans/closure/M041-stream-loss-metrics-and-closure-hygiene-corrective-closure.md`);
M040 (`48fe0dd`) is the historical stream-loss requalification.

## 1. Adapter stance: no state, native authority only

- `ToxiproxyAdapter { state: ControlState, profile: CompatProfile }`
  (`crates/eggchaos-toxiproxy/src/lib.rs:646-649`) is a **facade with no
  proxy or toxic definitions of its own**. Module doc (`lib.rs:1-14`) and
  struct doc (`lib.rs:642-644`) state the invariant explicitly: views derive
  from `ControlState` snapshots, mutations go through the M010 control
  authority. The only adapter-owned value is the selected
  `CompatProfile` (`lib.rs:1039-1072`).
- Construction:
  - `new(state)` → strict-v2.12 default (`lib.rs:652-655`).
  - `with_profile(state, profile)` → explicit profile (`lib.rs:657-659`).
  - `profile()` accessor (`lib.rs:661-663`); `control_state()` borrow
    (`lib.rs:666-668`).
- Reads (all from live native snapshots plus actual bound addresses):
  - `list_views()` → `ControlState::list()` (`lib.rs:671-673`).
  - `list_json()` → name-keyed `BTreeMap<String, Value>` of
    `proxy_json(&view, profile)` (`lib.rs:676-684`).
  - `proxy_json(name)` / `list_toxics` / `get_toxic` → live snapshots
    (`lib.rs:687-693`, `lib.rs:922-954`).
  - `proxy_json(view, profile)` renders `bound_addr.unwrap_or(listen)` plus
    reverse-translated toxics (`lib.rs:617-640`).
- Writes (all through native authority, never a shadow store):
  - `create` → `create_proxy` (enabled) or `import_definition` (disabled)
    (`lib.rs:730-756`).
  - `update` → `set_enabled` + `update_proxy` restart-class machinery
    (`lib.rs:760-804`; error mapping `lib.rs:806-814`).
  - `delete` → `delete_proxy` (`lib.rs:817-823`).
  - `populate` / `populate_entry` → `get` + `delete_proxy` +
    `create_proxy`/`import_definition` (`lib.rs:831-874`).
  - `add_toxic` → `add_fault` (`lib.rs:878-918`).
  - `update_toxic` → `update_fault` (`lib.rs:958-998`).
  - `remove_toxic` → `remove_fault` (`lib.rs:1001-1013`).
  - `reset` → `ControlState::reset` (re-enable all, clear all fault plans)
    (`lib.rs:1017-1023`).
  - `version()` is profile-aware: strict → `"2.12.0"`, snapshot →
    `"git"` (`lib.rs:1031-1036`).
  - Classified in `plans/reference/toxiproxy-parity.md:94-101` ("State
    authority"): `create_proxy`, `import_definition`, `update_proxy`,
    `set_enabled`, `delete_proxy`, `add/update/remove_fault`, `reset`.
    Native `/v1` mutations are immediately visible through reverse
    `FaultSpec -> Toxic` translation.
- Compatibility profiles (`lib.rs:1039-1072`):
  - `StrictV2_12` (`"strict-v2.12"`, default): frozen v2.12 toxic surface;
    rejects `packet_loss`.
  - `PostV2_12_2026_09_25` (`"post-v2.12-2026-09-25"`): opt-in pinned
    post-v2.12 snapshot (`accepts_packet_loss() == true`); adds
    `packet_loss` with `loss_rate`/`correlation` while keeping the v2.12
    route family unchanged.
- HTTP hosting:
  - `ToxiproxyHttp::start(bind, adapter)` (`lib.rs:1102-1142`) binds a Tokio
    `TcpListener`, builds an `eggserve-server` service via
    `service_fn_with_policy` with `RequestBodyPolicy::Buffer { max_bytes:
    1 MiB }`, `RuntimeConfig { max_request_body_bytes: 1 MiB,
    max_connections: 128 }`, and returns `ToxiproxyHttpHandle`.
  - `ToxiproxyHttpHandle { local_addr, server }` (`lib.rs:1078-1100`):
    `local_addr()`, `shutdown()`, `wait()`.
  - `crates/eggchaos-toxiproxy/examples/compat_server.rs:1-48` is the
    standalone smoke server: `ToxiproxyAdapter::with_profile(
    ControlState::default(), profile)` + `ToxiproxyHttp::start`,
    loopback `127.0.0.1:8474` by default; optional second arg selects
    `strict-v2.12` (default) or `post-v2.12-2026-09-25`
    (`compat_server.rs:32-37`).

## 2. Toxic ↔ fault translation

Forward: `Toxic::to_fault()` (strict default, `lib.rs:515-517`),
`to_fault_with_profile()` (`lib.rs:521-568`),
`kind_from_attrs()` (`lib.rs:274-329`). Reverse: `attrs_from_kind()`
(`lib.rs:337-417`), `attributes_object()` (`lib.rs:421-439`),
`fault_to_toxic()` (`lib.rs:447-460`). Update merge:
`merge_attributes()` (`lib.rs:465-507`). Proxy translation:
`translate_proxy()` (strict, `lib.rs:574-576`) /
`translate_proxy_with_profile()` (`lib.rs:579-608`).

### 2.1 Type table

| v2.12 toxic | Native `FaultKind` | Attribute units | Mapping (`lib.rs`) |
| --- | --- | --- | --- |
| `latency` | `Latency(LatencyConfig)` | `latency` ms, `jitter` ms | `delay = ms(latency)`, `jitter = ms(jitter)`, `max_buffer_bytes = 64 KiB` (`lib.rs:281-285`) |
| `bandwidth` | `Bandwidth(BandwidthConfig)` | `rate` **KiB/s** | `bytes_per_second = max(rate,1) * 1024`, `burst_bytes = 64 KiB` (`lib.rs:286-290`); reverse `rate = bytes_per_second / 1024` (`lib.rs:350-356`) |
| `timeout` | `Blackhole(BlackholeConfig)` | `timeout` ms | `close_after = timeout>0 ? Some(ms) : None`; `timeout=0` is indefinite blackhole (`lib.rs:291-296`); reverse `None → 0` (`lib.rs:357-368`) |
| `slow_close` | `SlowClose(SlowCloseConfig)` | `delay` ms | `delay = ms(delay)` (`lib.rs:297-299`); reverse exact (`lib.rs:376-382`) |
| `reset_peer` | `Disconnect(DisconnectConfig)` | `timeout` ms | `after = ms(timeout)`, `hard_reset = true` (`lib.rs:300-303`); reverse `timeout = after.ms` (`lib.rs:392-398`) |
| `slicer` | `Slice(SliceConfig)` | `average_size` bytes, `size_variation` bytes, `delay` **µs** | `average_size = NonZero(max(v,1))`, `variation`, `delay = µs(delay)` (`lib.rs:304-309`); reverse echoes all three (`lib.rs:383-391`) |
| `limit_data` | `LimitData(LimitDataConfig)` | `bytes` bytes | `bytes = NonZero(max(v,1))` (`lib.rs:310-313`); reverse exact (`lib.rs:369-375`) |
| `packet_loss` (snapshot only) | `StreamLoss(StreamLossConfig)` | `loss_rate` [0,1], `correlation` [0,1] (finite; out-of-range clamped) | `loss_rate/correlation = clamp(attr,0,1)` (`lib.rs:314-326`); strict profile rejects with `invalid_type`; reverse snapshot-only (`lib.rs:399-415`), strict reverse errors `invalid_type` |
| anything else | — | — | `CompatError::invalid_type()` → 400 `"invalid toxic type"` (`lib.rs:327`, `lib.rs:70-72`) |

Notes:

- Slicer is **stream segmentation, not IP/TCP packet loss**
  (`docs/toxiproxy.md:35-36`). Stream-chunk dropping is never described as
  real packet loss in native APIs; Toxiproxy compat retains upstream naming
  with the documented distinction (project invariant, `AGENTS.md`).
- `reset_peer` maps to delayed `Disconnect { hard_reset: true }`; RST vs FIN
  is platform-dependent (see §4).
- `packet_loss` is available only under the opt-in
  `post-v2.12-2026-09-25` profile pinned to Shopify/Toxiproxy commit
  `40f7fd31bee529d824116bd2a11a9e3425e904ec`; strict v2.12 remains the
  default and rejects it (forward `lib.rs:314-316`, reverse
  `lib.rs:399-415`, HTTP test `lib.rs:1593-1647`). See §5.1. The profile
  models userspace stream-chunk loss (`StreamLoss`), not IP/TCP packet loss.
  Out-of-range finite `loss_rate`/`correlation` are clamped into `[0,1]`
  (recorded divergence; test `lib.rs:1711-1734`).

### 2.2 Defaults, names, toxicity, validation precedence, update merge

- **Stream default `downstream`** (`lib.rs:161-163`, `Toxic.stream`
  `default_stream()`); **toxicity default `1.0`** (`lib.rs:164-166`,
  `default_toxicity()`); **omitted attributes zero-fill per type**,
  differential-verified (`plans/reference/toxiproxy-parity.md:69-71`;
  oracle zero shapes in
  `qualification/toxiproxy-v2-12/oracle-baseline-v2.12.0.md:50-58`;
  sample case `qualification/toxiproxy-v2-12/cases/default-latency.json`).
  Reverse rendering always emits the full per-type object via
  `attributes_object()` (`lib.rs:421-439`).
- **Name defaulting**: absent or empty `name` → `<type>_<stream>` with the
  **normalized lowercase** stream (`lib.rs:549-553` in `to_fault()` and
  `lib.rs:887-891` in `add_toxic`; empty-string case matches the pinned Go
  client serializing unset names as `""`, `lib.rs:885-886`). Example:
  `latency` + `Upstream` → `latency_upstream` (unit test
  `lib.rs:1470-1489`).
- **Toxicity clamping**: `clamp_toxicity()` (`lib.rs:254-260`) clamps finite
  values into `[0.0, 1.0]`; non-finite → 400 `"toxicity must be finite"`.
  `Probability::new` then enforces the range
  (`crates/eggchaos-core/src/plan.rs`). Recorded divergence, see §4.
- **Zero-numeric coalescing**: `bandwidth.rate`, `slicer.average_size`,
  `limit_data.bytes` use `unwrap_or(1).max(1)` into `NonZeroU64`
  (`lib.rs:287`, `lib.rs:305`, `lib.rs:311`); unit test
  `lib.rs:1516-1550`. Recorded divergence, see §4.
- **Stream validation-before-type precedence**: `Toxic::to_fault_with_profile()`
  checks stream before the type match (`lib.rs:525-546`); `add_toxic()` calls
  `parse_stream()` (`lib.rs:240-248`) at `lib.rs:882` before
  `kind_from_attrs()` at `lib.rs:884`. Unit test
  `rejects_invalid_stream_before_type` (`lib.rs:1454-1467`) asserts
  `stream=sideways, type=nope` reports the stream error. `parse_stream`
  accepts any ASCII case (`eq_ignore_ascii_case`, `lib.rs:241-244`).
- **Same-type-only update merge**: `ToxicUpdate.type/stream` are accepted and
  **ignored** (`lib.rs:172-186`, `lib.rs:956-998`); only `toxicity` and keys
  belonging to the toxic's own type apply via `merge_attributes()`
  (`lib.rs:465-507`, `pick = incoming.or(current)` per type, plus `pick_f`
  for `packet_loss`). Cross-type payloads leave the toxic unchanged (unit
  test `lib.rs:1553-1568`; differential cases
  `toxic-update-ignores-type-stream` in `tests/differential.rs:892-938`).

## 3. HTTP surface (oracle-exact)

Dispatcher: `compatibility_request()` (`lib.rs:1206-1335`) over
`eggserve-primitives` + `eggserve-server`. Helpers: `status()`
(`lib.rs:1145-1156`), `json_value()` (`lib.rs:1159-1167`),
`error_response()` (`lib.rs:1171-1180`),
`empty_response()` (`lib.rs:1183-1188`),
`unknown_route()` (`lib.rs:1191-1200`), `parse_json()` (`lib.rs:1202-1204`).

| Method + path | Success | Notes |
| --- | --- | --- |
| `GET /version` | 200 `{"version":"2.12.0"}` strict / `{"version":"git"}` snapshot | Profile-aware (`lib.rs:1031-1036`, `lib.rs:1219-1228`); `content-type: application/json;charset=utf-8` (oracle suffix verbatim, `lib.rs:1224`) |
| `GET /proxies` | 200 name-keyed map | `{"p1": {...}}`, empty → `{}` (`lib.rs:1229-1238`) |
| `POST /proxies` | 201 proxy object | Duplicate → 409; missing name/upstream → 400 oracle-verbatim; malformed → 400 (text differs, status exact); bind conflict → 500 (shape matches, OS text differs); `create` (`lib.rs:730-756`) |
| `POST /populate` | 201 always | Empty list → `{"proxies":null}` (`lib.rs:1257-1258`); whitespace-only body → 400 `bad request body: EOF` (`lib.rs:1250-1252`); missing entry name → 400 shape below; otherwise keep/replace/create/skip (see below) |
| `GET /proxies/{proxy}` | 200 proxy object | Missing → 404 envelope (`lib.rs:1275-1278`) |
| `POST /proxies/{proxy}`, `PATCH /proxies/{proxy}` | 200 proxy object | `enabled` lifecycle + listen/upstream restart-class; invalid addresses → 500 per oracle (`lib.rs:1279-1288`, `lib.rs:760-814`) |
| `DELETE /proxies/{proxy}` | 204 empty | Missing → 404 envelope (`lib.rs:1289-1292`) |
| `GET /proxies/{proxy}/toxics` | 200 array | From live snapshots, upstream faults then downstream (`lib.rs:1293-1296`, `lib.rs:922-939`) |
| `POST /proxies/{proxy}/toxics` | 200 toxic | Duplicate → 409; bad type/stream → 400 oracle-verbatim; missing proxy → 404 (`lib.rs:1297-1306`, `lib.rs:878-918`); strict `packet_loss` → 400 `invalid toxic type` |
| `GET /proxies/{proxy}/toxics/{toxic}` | 200 toxic | Missing proxy → 404 `proxy not found`; missing toxic → 404 `toxic not found` (`lib.rs:1307-1310`, `lib.rs:942-954`) |
| `POST /proxies/{proxy}/toxics/{toxic}`, `PATCH /proxies/{proxy}/toxics/{toxic}` | 200 toxic | Toxicity + same-type attributes only; type/stream ignored (`lib.rs:1311-1321`, `lib.rs:958-998`); PATCH exists because the pinned Go client uses it |
| `DELETE /proxies/{proxy}/toxics/{toxic}` | 204 empty | Missing → 404 envelope with proxy/toxic distinction (`lib.rs:1322-1327`, `lib.rs:1001-1013`) |
| `POST /reset` | 204 empty | Re-enables every proxy, removes every toxic (`lib.rs:1328-1331`, `lib.rs:1017-1023`) |
| `GET /metrics` | 404 plain-text | Matches oracle without metrics flags; no fabricated counters (`plans/reference/toxiproxy-parity.md:50`) |
| unknown routes | 404 plain-text | `404 page not found\n` (`lib.rs:1191-1200`) |

Error envelopes and content types (oracle-exact):

- Compat errors: JSON body `{"error": "...", "status": NNN}` served as
  **`text/plain; charset=utf-8`**, matching the oracle's error content type
  (`lib.rs:30-31`, `lib.rs:1171-1180`; parity
  `plans/reference/toxiproxy-parity.md:53-55`).
- Success bodies: `application/json` (version carries the oracle `charset`
  suffix verbatim, `lib.rs:1224`).
- Unknown routes / metrics-off: plain-text `404 page not found`
  (`lib.rs:1191-1200`; baseline
  `qualification/toxiproxy-v2-12/oracle-baseline-v2.12.0.md:77-81`).
- Verbatim messages (`lib.rs:50-80`): `proxy not found` (404),
  `toxic not found` (404), `proxy already exists` (409),
  `toxic already exists` (409), `invalid toxic type` (400),
  `stream was invalid, can be either upstream or downstream` (400),
  `missing required field: name` / `missing required field: upstream` (400
  via `resolve_addrs`, `lib.rs:698-726`), `bad request body: ...` (400,
  prefix mirrors oracle, suffix is local parse text, `lib.rs:84-86`).
- Populate missing-name shape: entry index is **1-based**,
  `{"error":"missing required field: name at proxy N","status":400,"proxies":null}`
  served as **`application/json`** (oracle-exact, `lib.rs:833-839`,
  `lib.rs:1263-1268`), unlike normal errors.
- Malformed-body and bind-conflict **message text differs** (Go/OS-specific);
  statuses and envelope shapes are exact
  (`plans/reference/toxiproxy-parity.md:37`, `88-89`).

Rendering and lifecycle details:

- **Bound-address rendering**: `proxy_json()` uses
  `view.bound_addr.unwrap_or(view.listen)` (`lib.rs:617-618`), so port `0`
  resolves to the actual ephemeral port. Differential tests normalize `listen`
  to `LISTEN` and assert concrete binds per server via `assert_listen`
  (`tests/differential.rs:141-158`, `238-246`).
- **`Logger` field**: every proxy object carries `"Logger":{}` verbatim
  (`lib.rs:637`), matching the oracle baseline
  (`oracle-baseline-v2.12.0.md:18-21`).
- **Enabled / import semantics**: create with `enabled=true` (default)
  binds via `create_proxy`; `enabled=false` imports the definition without
  binding, echoing the configured listen (`lib.rs:730-756`; HTTP test
  `lib.rs:1863-1872`). Updates apply `enabled` via `set_enabled` and
  listen/upstream via `update_proxy`; address parse failures on update report
  **500** per the oracle (`lib.rs:760-814`).
- **Populate replace/skip/201 behavior** (`lib.rs:831-874`,
  `lib.rs:1249-1274`): same `listen`+`upstream` → return proxy **untouched**
  (enabled + toxics preserved; snapshot profile preserves `packet_loss`,
  test `lib.rs:1422-1451`); changed addresses → delete + recreate
  (**toxics drop**); unknown names → created (honoring `enabled:false`);
  bind failures → **skipped** (`None`); proxies absent from input are **not**
  deleted; empty input echoes `{"proxies":null}`; always **201** on success.

## 4. Recorded divergences (intent-compatible / not supported / incomplete)

Full matrix: `plans/reference/toxiproxy-parity.md:73-92`. Summary in
`docs/toxiproxy.md:18-33`. Module doc pointer: `lib.rs:9-14`.

| Divergence | Level | Rationale / evidence |
| --- | --- | --- |
| Toxicity outside `[0,1]` **clamped** (oracle echoes verbatim) | intent compatible | `clamp_toxicity` (`lib.rs:254-260`); runtime effect preserved (always/never applies); exact clamped values asserted in corpus (`tests/differential.rs:979-989`, `normalize_recorded` `192-234`) |
| Degenerate zero `rate` / `average_size` / `bytes` **coalesce to 1** | intent compatible | Native `NonZero` bounds (`lib.rs:287`, `lib.rs:305`, `lib.rs:311`); asserted in `lib.rs:1516-1550`; normalized in differential (`tests/differential.rs:211-222`) |
| `packet_loss` out-of-range finite `loss_rate`/`correlation` **clamped** (oracle echoes verbatim) | intent compatible | Snapshot-only clamp (`lib.rs:279`, `lib.rs:318-319`); normalized in post-v2.12 corpus (`tests/post_v212_differential.rs:219-245`); asserted in `lib.rs:1711-1734` |
| Stream echo **lowercase** (oracle preserves exotic input case) | intent compatible | Native `Direction` round-trip (`lib.rs:447-460`); parity `73-92` |
| Missing `listen` binds **ephemeral loopback** (`127.0.0.1:0`); oracle binds ephemeral wildcard | intent compatible (native invariant wins) | `resolve_addrs` (`lib.rs:716-724`); loopback-by-default invariant (`AGENTS.md`) |
| Non-socket `upstream` → clear **400** (oracle stores arbitrary strings) | not supported (fails clearly) | Native fixed-target invariant (`lib.rs:711-715`); parity `83-85` |
| Proxy names outside `[A-Za-z0-9._-]` → clear **400** (oracle laxer) | not supported (fails clearly) | Native path-segment invariant `crates/eggchaos-server/src/runtime.rs`; parity `86-87`; surfaces as 400 via `CompatError` mapping (`lib.rs:228-236`, `lib.rs:740-744`) |
| Malformed-body / bind-conflict message text differs; status + shape exact | exact status/shape | `lib.rs:84-86`, `lib.rs:740-743`; parity `88-89`; differential checks status-only for malformed (`tests/differential.rs:758-762`) |
| Toxic presentation order **upstream faults then downstream**; creation-interleaved order does not round-trip | intent compatible | `proxy_json` (`lib.rs:619-631`), `list_toxics` (`lib.rs:927-937`); differential comment `lib.rs:920-921` |
| `reset_peer` termination **platform-qualified** (RST vs FIN not asserted) | intent compatible | Observed termination on darwin/arm64 in differential data-plane (`tests/differential.rs:669-695`); parity `65`, `docs/toxiproxy.md:27` |
| `bandwidth` data-plane pacing **intent compatible** (not scheduler-exact) | intent compatible | 1 MiB preserved, 1–10 s window, ≤3× oracle/native ratio (`tests/differential.rs:428-476`); parity `62` (Darwin ARM64 sample oracle 4.156 s, native 1.898 s); `docs/toxiproxy.md:28-33` |
| `slow_close` data-plane delay **intent compatible** | intent compatible | 300 ms close delay, exact echo bytes, ≥200 ms floor (`tests/differential.rs:487-538`); parity `63` (oracle 302.432 ms, native 302.184 ms) |
| `slicer` data-plane pacing **intent compatible** (not Go-sequence-exact) | intent compatible | 64 KiB preserved, ≥20 ms floor, ≤3× ratio (`tests/differential.rs:540-590`); parity `66` (oracle 320.382 ms, native 256.933 ms) |
| `GET /metrics` plain-text 404 | exact | Matches oracle without metrics flags (`plans/reference/toxiproxy-parity.md:50`) |

Version policy: eggchaos targets pinned **v2.12.0 only**, not moving
`main` (`plans/reference/toxiproxy-parity.md:17-20`). The opt-in
post-v2.12 snapshot is pinned to `40f7fd31` and never claims equivalence
with a moving upstream `main` (`docs/toxiproxy.md:42-47`).

## 5. Evidence

- **Oracle baseline** `qualification/toxiproxy-v2-12/oracle-baseline-v2.12.0.md`:
  live `curl` probes against pinned `toxiproxy-server version 2.12.0`
  (Darwin arm64 SHA-256
  `aa299966b52f16a8594f1cd0d1e9049dc2e8fe2c04a90c19860e2719b2b95d15`;
  full per-OS SHAs in baseline `:8-10` and
  `plans/reference/toxiproxy-parity.md:3-6`): proxy object shape with
  `Logger:{}` + `toxics:[]`, 201/409/404/400/500 cases, seven toxic types
  with zero-filled defaults, `<type>_<stream>` auto-naming, unvalidated
  toxicity, ignored type-on-update, PATCH mirroring, `POST /reset` 204
  re-enable + clear, `POST /populate` keep semantics, `404 page not found`
  for metrics/unknown.
- **Representative case**
  `qualification/toxiproxy-v2-12/cases/default-latency.json`: latency
  `{"latency":100,"jitter":10}` translation fixture shape.
- **Differential corpus** `crates/eggchaos-toxiproxy/tests/differential.rs`:
  The strict corpus now has **50/50 comparisons pass** against the live pinned oracle
  (`TOXIPROXY_SERVER=/path/to/v2.12.0` + `EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1`);
  without the binary (or on version mismatch) the test reports `incomplete`
  and passes without asserting parity (`differential.rs:699-712`). Corpus
  compares status + content-type class + normalized bodies (`Corpus::check`,
  `differential.rs:273-307`), with declared normalizations only: `listen` →
  `LISTEN` (disjoint binds, concrete ports asserted per server), JSON numbers
  canonicalized to f64 (Go `1` vs `serde_json` `1.0`), toxicity clamped, zero
  numerics coalesced (`differential.rs:138-246`, summary `1128-1141` prints
  `DIFFERENTIAL_SUMMARY` with `"failed":0`). Covers version, list/get/
  create/dup/update/disable/enable/delete proxies, all seven toxic defaults
  + auto-name + list/get/update (POST+PATCH, type/stream ignored) + dup/bad
  type/bad stream/missing proxy + toxicity clamp + delete, populate
  empty/new/missing-name/keep, reset + get-after-reset, metrics/unknown 404,
  plus data-plane (`differential.rs:367-696`): latency byte-preservation +
  ≥150 ms delay on 200 ms config, `bandwidth` 1 MiB + 1–10 s + ≤3× ratio,
  `slow_close` exact echo + ≥200 ms close delay, `slicer` 64 KiB + ≥20 ms +
  ≤3× ratio, `limit_data` exact 100/1000 boundary, `timeout=0` blocking +
  post-removal flow, `reset_peer` termination.
- **Client smokes** `qualification/toxiproxy-v2-12/client-smoke/`:
  pinned Go client `github.com/Shopify/toxiproxy/v2@v2.12.0` —
  `go/go_results.json` 13/13 steps pass
  (version/create/populate/add-toxic/add-toxic-auto-name/update/list/
  remove×2/disable/enable/reset/delete); independent Python-stdlib client
  `py_smoke.py` → `py_results.json` 12/12 steps pass (version/create/
  populate/add/update-POST/update-PATCH/list/remove/disable/enable/reset/
  delete). Parity header (`plans/reference/toxiproxy-parity.md:7-11`)
  records both as 13/13 (Go toolchain go1.27.1); file-observed counts are
  Go 13, Python 12 (PATCH update covers POST+PATCH in one Python step).
- **Unit/HTTP translation suite** in `lib.rs:1337-1976`: all-seven mapping +
  downstream default, stream-before-type precedence, name/stream/toxicity
  defaults + clamp, `timeout=0` indefinite round-trip, zero coalescing,
  cross-type merge isolation, version route, create/list/toxic shared-state,
  oracle shapes/errors, reset re-enable + clear; plus snapshot-profile
  tests: `packet_loss` snapshot-only round-trip (`lib.rs:1394-1419`),
  snapshot populate-keep preserves `packet_loss` (`lib.rs:1422-1451`),
  strict rejects `packet_loss` 400 (`lib.rs:1593-1647`), snapshot accepts +
  clamps out-of-range (`lib.rs:1650-1737`), strict native `StreamLoss`
  reverse-maps to `invalid toxic type` (`lib.rs:1740-1776`).

### 5.1 Post-v2.12 snapshot profile (M040 qualified, M041 corrective)

The adapter profile (`lib.rs:1039-1072`) is the single source for
proxy/toxic translation and rendering, including populate keep/recreate
responses. `to_fault()` and `translate_proxy()` remain strict by default;
their profile-aware variants (`to_fault_with_profile`,
`translate_proxy_with_profile`, `proxy_json(view, profile)`,
`fault_to_toxic(direction, fault, profile)`) accept `packet_loss` only for
the snapshot profile. Snapshot reverse mapping emits `packet_loss`; strict
reverse mapping reports `invalid toxic type` (`lib.rs:399-415`).

The mandatory source-built oracle uses commit
`40f7fd31bee529d824116bd2a11a9e3425e904ec`, archive SHA-256
`26351cc70792f1c3391bdd1d376974a79a756b349ec1abfa8b8f1524c042b13d`, and
exact Go toolchain `go1.23.0` (`GOTOOLCHAIN=go1.23.0`;
`scripts/fetch_toxiproxy_post_v2_12.sh:21-22`, M041 closure `:96-101`).
The corrective corpus (`tests/post_v212_differential.rs`) separates 12 exact
API comparisons (version/create/defaults/typical/edges/out-of-range/
mixed-int/bad-type/empty-name/get/update-correlation/list), two isolated
exact data-plane edges (zero-loss preserves all 131,072 payload bytes;
full-loss forwards zero bytes), a 256-probe `loss_rate=0.25` comparator with
frozen `0.10..=0.40` drop-fraction bounds, and a 512-probe `loss_rate=0.20`,
`correlation=0.50` conditional-gap comparator with 50-observation minimum
conditioning buckets and a `0.20` gap floor. The M040 mandatory run recorded
oracle/Eggchaos intermediate drops of 71/256 and 69/256, and conditional gaps
of 0.4720/0.5136. These are intent-compatible stochastic results, not exact
RNG/chunk-sequence equivalence. See the M040 closure for the exact candidate
(`48fe0dd`) and raw qualification output.

M041 (exact candidate `724b967`, closure
`plans/closure/M041-stream-loss-metrics-and-closure-hygiene-corrective-closure.md`)
is the latest corrective successor and changes no ADR 007 semantics:

- **Stream-loss Prometheus exposition**: per-proxy/direction samples for
  `eggchaos_stream_loss_chunks_evaluated_total`,
  `eggchaos_stream_loss_chunks_dropped_total`,
  `eggchaos_stream_loss_bytes_discarded_total` render exactly once with real
  `\n` terminators via a single helper in
  `ControlState::metrics_text()`
  (`crates/eggchaos-server/src/runtime/control.rs:62-...`); the bounded
  `_overflow` proxy uses the same helper. Regression
  `stream_loss_prometheus_exposition_is_unique_and_well_formed`
  (`crates/eggchaos-server/src/runtime/tests.rs:1848`) asserts no literal
  `\n` sequence, exactly-once `(metric, proxy, direction)` tuples, and
  unchanged label-key/zero-loss invariants.
- **Fetcher stdout contract**: default and `--path-only` print exactly one
  executable path on stdout; `--json` prints one metadata record
  (`requested_toolchain`, `resolved_go_version`, `resolved_gotoolchain`,
  `source_commit`, `source_sha256`, `oracle_path`, `oracle_version`);
  `--help` exits 0 (usage on stderr); unknown flags exit 2 (diagnostic on
  stderr); diagnostics never leak onto stdout
  (`scripts/fetch_toxiproxy_post_v2_12.sh:11-18`, `:118-128`). The qualifier
  consumes the path via explicit `--path-only`
  (`scripts/qualify_toxiproxy_post_v2_12.sh:23`). Regression
  `scripts/tests/test_fetch_toxiproxy_post_v2_12_contract.sh` (wired into the
  `language-clients` CI job) enforces the contract end-to-end when a cached
  oracle exists.

## 6. Review checklist

For a systematic reviewer of this adapter:

1. Confirm `ToxiproxyAdapter` holds **no** proxy/toxic maps — only
   `ControlState` + `CompatProfile` — and every read/write path in §1
   delegates to the native authority.
2. Confirm the default profile is strict v2.12 and `packet_loss` is rejected
   (forward + reverse) outside `PostV2_12_2026_09_25`; check
   `version()` (`2.12.0` vs `git`) and `compat_server.rs` second-arg
   handling.
3. Walk the §2.1 table row by row against `kind_from_attrs` /
   `attrs_from_kind`: units (ms vs µs vs KiB/s vs [0,1] loss/correlation),
   `NonZero` coalescing, `timeout=0 → close_after: None`,
   `reset_peer → hard_reset: true`, 64 KiB latency/bandwidth buffers,
   `loss_rate`/`correlation` clamp.
4. Check defaults: `default_stream`, `default_toxicity`, zero-filled
   `attributes_object`, `<type>_<stream>` naming incl. `""` input.
5. Check `clamp_toxicity` finiteness + `[0,1]` clamp vs oracle-verbatim
   baseline; confirm differential `normalize_recorded` (+
   `normalize_packet_loss` for the snapshot corpus) and explicit clamp
   assertions cover the gap.
6. Check stream-before-type order in both `to_fault_with_profile` and
   `add_toxic`, and ASCII-case-insensitive accept with lowercase echo.
7. Check `merge_attributes` only picks own-type keys (incl. `packet_loss`
   `pick_f`); confirm update ignores `type`/`stream` on both POST and PATCH.
8. Walk every route in §3 against `compatibility_request`: method+path,
   status code, content type, envelope shape; especially version charset
   suffix + profile-aware version, populate empty/whitespace/missing-name
   shapes, 204 empties, 404 plain-text, proxy-vs-toxic 404 distinction.
9. Confirm bound-address rendering (`bound_addr.unwrap_or(listen)`) and
   `Logger:{}` on every proxy render.
10. Confirm enabled/import split (`create_proxy` vs `import_definition`) and
   update-path 500s on bad addresses.
11. Confirm populate keep (same addrs untouched, snapshot toxics preserved) /
   replace (changed addrs drop toxics) / create / skip-on-bind-failure /
   never-delete / always-201.
12. Confirm each §4 divergence is classified in
   `plans/reference/toxiproxy-parity.md` and has either a normalization +
   explicit assertion or an `incomplete`/`not supported` label — never silent.
   Bandwidth/slow_close/slicer are measured intent-compatible comparators,
   not incomplete.
13. Confirm evidence is execution-based (§5): differential `failed:0` logs
   (strict 50/50; snapshot 12 + 2 + 256/512), Go + Python smoke JSON, oracle
   baseline SHAs/commits/toolchains, M041 metrics/fetcher regressions — not
   source inspection.
14. Confirm loopback-by-default (`compat_server.rs`, `127.0.0.1:0` fallback)
   and 1 MiB body / 128-connection bounds in `ToxiproxyHttp::start`.
15. Confirm M041 narrow scope: stream-loss samples once-only with real
   newlines; fetcher default/`--path-only` = one path, `--json` = one
   record; no ADR 007 semantic change; M036–M040 history untouched.

## 7. Verification

```sh
cargo test -p eggchaos-toxiproxy --all-features
TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 ./scripts/qualify_toxiproxy_v2_12.sh
TOXIPROXY_POST_V2_12_SERVER="$(./scripts/fetch_toxiproxy_post_v2_12.sh --path-only)" EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1 ./scripts/qualify_toxiproxy_post_v2_12.sh
sh scripts/tests/test_fetch_toxiproxy_post_v2_12_contract.sh
```

- `cargo test -p eggchaos-toxiproxy --all-features` runs the translation +
  HTTP suites (`lib.rs:1337-1976`).
- `scripts/qualify_toxiproxy_v2_12.sh` runs
  `cargo test -p eggchaos-toxiproxy --all-features`, then — only if a pinned
  `2.12.0` oracle binary is present and checksum/version-verified — the
  strict differential corpus and greps for `"failed":0`; without the oracle
  it reports `differential: incomplete` (exit 0) rather than treating source
  inspection as proof. Mandatory mode is `TOXIPROXY_SERVER=<pinned binary>`
  + `EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1` (fail-closed, exit 1 without a
  verified oracle). Developer mode without a verified oracle reporting
  `differential:incomplete` (exit 0) is not a pass.
- Differential directly:
  `TOXIPROXY_SERVER=/path/to/pinned/v2.12.0 cargo test -p eggchaos-toxiproxy --test differential -- --nocapture`
  (usage documented in `tests/differential.rs:1-10`).
- Snapshot differential directly:
  `TOXIPROXY_POST_V2_12_SERVER=/path/to/pinned/40f7fd31 EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1 cargo test -p eggchaos-toxiproxy --test post_v212_differential -- --nocapture`
  (usage in `tests/post_v212_differential.rs:1-14`). The qualifier itself
  uses `./scripts/fetch_toxiproxy_post_v2_12.sh --path-only` internally so
  the fetcher stdout contract is unambiguous (M041).
- Fetcher contract (M041):
  `TOXIPROXY_POST_V2_12_SERVER="$(./scripts/fetch_toxiproxy_post_v2_12.sh)"`
  and `--path-only` print exactly one executable path;
  `./scripts/fetch_toxiproxy_post_v2_12.sh --json` prints one metadata
  record. Enforced by
  `sh scripts/tests/test_fetch_toxiproxy_post_v2_12_contract.sh`.
- Manual smoke:
  `cargo run -p eggchaos-toxiproxy --example compat_server -- 127.0.0.1:8474`
  (strict default; `examples/compat_server.rs:3-7`), snapshot profile via
  `cargo run -p eggchaos-toxiproxy --example compat_server -- 127.0.0.1:8475 post-v2.12-2026-09-25`,
  then the Go/Python smokes in
  `qualification/toxiproxy-v2-12/client-smoke/`.
- Repo-wide gates per `AGENTS.md`:
  `./scripts/check.sh`
  (= `cargo fmt --all -- --check` + `cargo clippy --workspace --all-targets --all-features -- -D warnings` + `cargo test --workspace --all-features` + `cargo doc --workspace --all-features --no-deps`).
