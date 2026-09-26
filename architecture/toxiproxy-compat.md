# Toxiproxy v2.12 compatibility adapter

Part of [Eggchaos architecture overview](overview.md) (§5:
`eggchaos-toxiproxy`). For the user-facing contract see
`docs/toxiproxy.md`; for the qualification contract see
`plans/reference/toxiproxy-parity.md`.

Crate: `crates/eggchaos-toxiproxy/` (`Cargo.toml`: depends on
`eggchaos-core`, `eggchaos-server`, `eggserve-primitives`,
`eggserve-server`; dev-depends on `eggfetch-core` for tests).
Implementation: `crates/eggchaos-toxiproxy/src/lib.rs` (1562 lines).
Standalone server: `crates/eggchaos-toxiproxy/examples/compat_server.rs`.
Differential corpus: `crates/eggchaos-toxiproxy/tests/differential.rs`.
Oracle baseline: `qualification/toxiproxy-v2-12/oracle-baseline-v2.12.0.md`.
Qualification runner: `scripts/qualify_toxiproxy_v2_12.sh`.

## 1. Adapter stance: no state, native authority only

- `ToxiproxyAdapter { state: ControlState }`
  (`crates/eggchaos-toxiproxy/src/lib.rs:544-558`) is a **facade with no
  proxy or toxic definitions of its own**. Doc comment on the module
  (`lib.rs:1-14`) and on the struct (`lib.rs:541-543`) state the invariant
  explicitly: views derive from `ControlState` snapshots, mutations go
  through the M010 control authority.
- Reads:
  - `list_views()` → `ControlState::list()` (`lib.rs:561-563`).
  - `list_json()` → name-keyed `BTreeMap<String, Value>` of
    `proxy_json(&view)` (`lib.rs:566-573`).
  - `proxy_json(name)` / `list_toxics` / `get_toxic` → live native snapshots
    plus actual bound addresses (`lib.rs:575-578`, `lib.rs:803-831`).
  - `proxy_json(view)` renders `bound_addr.unwrap_or(listen)` plus
    reverse-translated toxics (see §3).
- Writes (all through native authority, never a shadow store):
  - `create` → `create_proxy` (enabled) or `import_definition` (disabled)
    (`lib.rs:615-640`).
  - `update` → `set_enabled` + `update_proxy` restart-class machinery
    (`lib.rs:644-698`).
  - `delete` → `delete_proxy` (`lib.rs:701-707`).
  - `populate` / `populate_entry` → `get` + `delete_proxy` +
    `create_proxy`/`import_definition` (`lib.rs:715-758`).
  - `add_toxic` → `add_fault` (`lib.rs:762-801`).
  - `update_toxic` → `update_fault` (`lib.rs:835-874`).
  - `remove_toxic` → `remove_fault` (`lib.rs:877-889`).
  - `reset` → `ControlState::reset` (re-enable all, clear all fault plans)
    (`lib.rs:891-899`).
  - Classified in `plans/reference/toxiproxy-parity.md:85-92` ("State
    authority"): `create_proxy`, `import_definition`, `update_proxy`,
    `set_enabled`, `delete_proxy`, `add/update/remove_fault`, `reset`.
    Native `/v1` mutations are immediately visible through reverse
    `FaultSpec -> Toxic` translation.
- HTTP hosting:
  - `ToxiproxyHttp::start(bind, adapter)` (`lib.rs:936-976`) binds a Tokio
    `TcpListener`, builds an `eggserve-server` service via
    `service_fn_with_policy` with `RequestBodyPolicy::Buffer { max_bytes:
    1 MiB }`, `RuntimeConfig { max_request_body_bytes: 1 MiB,
    max_connections: 128 }`, and returns `ToxiproxyHttpHandle`.
  - `ToxiproxyHttpHandle { local_addr, server }` (`lib.rs:912-934`):
    `local_addr()`, `shutdown()`, `wait()`.
  - `crates/eggchaos-toxiproxy/examples/compat_server.rs:1-24` is the
    standalone smoke server: `ToxiproxyAdapter::new(ControlState::default())`
    + `ToxiproxyHttp::start`, loopback `127.0.0.1:8474` by default.

## 2. Toxic ↔ fault translation

Forward: `Toxic::to_fault()` (`lib.rs:438-486`), `kind_from_attrs()`
(`lib.rs:263-300`). Reverse: `attrs_from_kind()` (`lib.rs:304-364`),
`attributes_object()` (`lib.rs:368-381`), `fault_to_toxic()` (`lib.rs:386-395`).
Update merge: `merge_attributes()` (`lib.rs:400-436`).

### 2.1 Type table

| v2.12 toxic | Native `FaultKind` | Attribute units | Mapping (`lib.rs`) |
| --- | --- | --- | --- |
| `latency` | `Latency(LatencyConfig)` | `latency` ms, `jitter` ms | `delay = ms(latency)`, `jitter = ms(jitter)`, `max_buffer_bytes = 64 KiB` (`lib.rs:265-269`) |
| `bandwidth` | `Bandwidth(BandwidthConfig)` | `rate` **KiB/s** | `bytes_per_second = max(rate,1) * 1024`, `burst_bytes = 64 KiB` (`lib.rs:270-274`); reverse `rate = bytes_per_second / 1024` (`lib.rs:314-319`) |
| `timeout` | `Blackhole(BlackholeConfig)` | `timeout` ms | `close_after = timeout>0 ? Some(ms) : None`; `timeout=0` is indefinite blackhole (`lib.rs:275-280`); reverse `None → 0` (`lib.rs:321-331`) |
| `slow_close` | `SlowClose(SlowCloseConfig)` | `delay` ms | `delay = ms(delay)` (`lib.rs:281-283`) |
| `reset_peer` | `Disconnect(DisconnectConfig)` | `timeout` ms | `after = ms(timeout)`, `hard_reset = true` (`lib.rs:284-287`); reverse `timeout = after.ms` (`lib.rs:356-361`) |
| `slicer` | `Slice(SliceConfig)` | `average_size` bytes, `size_variation` bytes, `delay` **µs** | `average_size = NonZero(max(v,1))`, `variation`, `delay = µs(delay)` (`lib.rs:288-293`); reverse echoes all three (`lib.rs:347-354`) |
| `limit_data` | `LimitData(LimitDataConfig)` | `bytes` bytes | `bytes = NonZero(max(v,1))` (`lib.rs:294-297`); reverse exact (`lib.rs:333-339`) |
| anything else | — | — | `CompatError::invalid_type()` → 400 `"invalid toxic type"` (`lib.rs:298`, `lib.rs:68-71`) |

Notes:

- Slicer is **stream segmentation, not IP/TCP packet loss**
  (`docs/toxiproxy.md:31-33`). Stream-chunk dropping is never described as
  real packet loss in native APIs; Toxiproxy compat retains upstream naming
  with the documented distinction (project invariant, `AGENTS.md`).
- `reset_peer` maps to delayed `Disconnect { hard_reset: true }`; RST vs FIN
  is platform-dependent (see §4).
- `packet_loss` is available only under the opt-in
  `post-v2.12-2026-09-25` profile pinned to Shopify/Toxiproxy commit
  `40f7fd31bee529d824116bd2a11a9e3425e904ec`; strict v2.12 remains the
  default and rejects it. See §5. The profile models userspace stream-chunk
  loss, not IP/TCP packet loss.

### 2.2 Defaults, names, toxicity, validation precedence, update merge

- **Stream default `downstream`** (`lib.rs:160-162`, `Toxic.stream`
  `default_stream()`); **toxicity default `1.0`** (`lib.rs:163-165`,
  `default_toxicity()`); **omitted attributes zero-fill per type**,
  differential-verified (`plans/reference/toxiproxy-parity.md:60-62`;
  oracle zero shapes in
  `qualification/toxiproxy-v2-12/oracle-baseline-v2.12.0.md:49-56`;
  sample case `qualification/toxiproxy-v2-12/cases/default-latency.json`).
  Reverse rendering always emits the full per-type object via
  `attributes_object()` (`lib.rs:368-381`).
- **Name defaulting**: absent or empty `name` → `<type>_<stream>` with the
  **normalized lowercase** stream (`lib.rs:466-470` in `to_fault()` and
  `lib.rs:770-774` in `add_toxic`; empty-string case matches the pinned Go
  client serializing unset names as `""`, `lib.rs:769-770`). Example:
  `latency` + `Upstream` → `latency_upstream` (unit test
  `lib.rs:1244-1262`).
- **Toxicity clamping**: `clamp_toxicity()` (`lib.rs:249-255`) clamps finite
  values into `[0.0, 1.0]`; non-finite → 400 `"toxicity must be finite"`.
  `Probability::new` then enforces the range
  (`crates/eggchaos-core/src/plan.rs:44-52`). Recorded divergence, see §4.
- **Zero-numeric coalescing**: `bandwidth.rate`, `slicer.average_size`,
  `limit_data.bytes` use `unwrap_or(1).max(1)` into `NonZeroU64`
  (`lib.rs:271`, `lib.rs:289`, `lib.rs:295`); unit test
  `lib.rs:1288-1322`. Recorded divergence, see §4.
- **Stream validation-before-type precedence**: `Toxic::to_fault()` checks
  stream before the type match (`lib.rs:443-463`); `add_toxic()` calls
  `parse_stream()` (`lib.rs:235-243`) before `kind_from_attrs()`
  (`lib.rs:766-767`). Unit test `rejects_invalid_stream_before_type`
  (`lib.rs:1228-1241`) asserts `stream=sideways, type=nope` reports the
  stream error. `parse_stream` accepts any ASCII case
  (`eq_ignore_ascii_case`, `lib.rs:236-242`).
- **Same-type-only update merge**: `ToxicUpdate.type/stream` are accepted and
  **ignored** (`lib.rs:167-185`, `lib.rs:833-846`); only `toxicity` and keys
  belonging to the toxic's own type apply via `merge_attributes()`
  (`lib.rs:400-436`, `pick = incoming.or(current)` per type). Cross-type
  payloads leave the toxic unchanged (unit test
  `lib.rs:1325-1340`; differential cases
  `toxic-update-ignores-type-stream` in `tests/differential.rs:709-755`).

## 3. HTTP surface (oracle-exact)

Dispatcher: `compatibility_request()` (`lib.rs:1040-1169`) over
`eggserve-primitives` + `eggserve-server`. Helpers: `status()` (`lib.rs:979-990`),
`json_value()` (`lib.rs:993-1001`), `error_response()` (`lib.rs:1005-1014`),
`empty_response()` (`lib.rs:1017-1022`), `unknown_route()` (`lib.rs:1025-1034`),
`parse_json()` (`lib.rs:1036-1038`).

| Method + path | Success | Notes |
| --- | --- | --- |
| `GET /version` | 200 `{"version":"2.12.0"}` | Verbatim; `content-type: application/json;charset=utf-8` (oracle suffix verbatim, `lib.rs:1053-1062`); `ToxiproxyAdapter::version()` (`lib.rs:901-905`) |
| `GET /proxies` | 200 name-keyed map | `{"p1": {...}}`, empty → `{}` (`lib.rs:1063-1072`) |
| `POST /proxies` | 201 proxy object | Duplicate → 409; missing name/upstream → 400 oracle-verbatim; malformed → 400 (text differs, status exact); bind conflict → 500 (shape matches, OS text differs) |
| `POST /populate` | 201 always | Empty list → `{"proxies":null}` (`lib.rs:1091-1093`); missing entry name → 400 shape below; otherwise keep/replace/create/skip (see below) |
| `GET /proxies/{proxy}` | 200 proxy object | Missing → 404 envelope |
| `POST /proxies/{proxy}`, `PATCH /proxies/{proxy}` | 200 proxy object | `enabled` lifecycle + listen/upstream restart-class; invalid addresses → 500 per oracle (`lib.rs:1113-1122`, `lib.rs:642-698`) |
| `DELETE /proxies/{proxy}` | 204 empty | Missing → 404 envelope (`lib.rs:1123-1126`) |
| `GET /proxies/{proxy}/toxics` | 200 array | From live snapshots, upstream faults then downstream (`lib.rs:1127-1130`, `lib.rs:803-817`) |
| `POST /proxies/{proxy}/toxics` | 200 toxic | Duplicate → 409; bad type/stream → 400 oracle-verbatim; missing proxy → 404 (`lib.rs:1131-1140`) |
| `GET /proxies/{proxy}/toxics/{toxic}` | 200 toxic | Missing → 404 envelope (`lib.rs:1141-1144`) |
| `POST /proxies/{proxy}/toxics/{toxic}`, `PATCH /proxies/{proxy}/toxics/{toxic}` | 200 toxic | Toxicity + same-type attributes only; type/stream ignored (`lib.rs:1145-1155`); PATCH exists because the pinned Go client uses it |
| `DELETE /proxies/{proxy}/toxics/{toxic}` | 204 empty | Missing → 404 envelope (`lib.rs:1156-1161`) |
| `POST /reset` | 204 empty | Re-enables every proxy, removes every toxic (`lib.rs:1162-1165`) |
| `GET /metrics` | 404 plain-text | Matches oracle without metrics flags; no fabricated counters (`plans/reference/toxiproxy-parity.md:41`) |
| unknown routes | 404 plain-text | `404 page not found\n` (`lib.rs:1025-1034`) |

Error envelopes and content types (oracle-exact):

- Compat errors: JSON body `{"error": "...", "status": NNN}` served as
  **`text/plain; charset=utf-8`**, matching the oracle's error content type
  (`lib.rs:29-30`, `lib.rs:1003-1014`; parity
  `plans/reference/toxiproxy-parity.md:44-46`).
- Success bodies: `application/json` (version carries the oracle `charset`
  suffix verbatim, `lib.rs:1058`).
- Unknown routes / metrics-off: plain-text `404 page not found`
  (`lib.rs:1025-1034`; baseline
  `qualification/toxiproxy-v2-12/oracle-baseline-v2.12.0.md:75-79`).
- Verbatim messages (`lib.rs:48-79`): `proxy not found` (404),
  `toxic not found` (404), `proxy already exists` (409),
  `toxic already exists` (409), `invalid toxic type` (400),
  `stream was invalid, can be either upstream or downstream` (400),
  `missing required field: name` / `missing required field: upstream` (400
  via `resolve_addrs`, `lib.rs:586-611`), `bad request body: ...` (400,
  prefix mirrors oracle, suffix is local parse text, `lib.rs:83-85`).
- Populate missing-name shape: entry index is **1-based**,
  `{"error":"missing required field: name at proxy N","status":400,"proxies":null}`
  served as **`application/json`** (oracle-exact, `lib.rs:715-723`,
  `lib.rs:1097-1102`), unlike normal errors.
- Malformed-body and bind-conflict **message text differs** (Go/OS-specific);
  statuses and envelope shapes are exact
  (`plans/reference/toxiproxy-parity.md:28`, `77-80`).

Rendering and lifecycle details:

- **Bound-address rendering**: `proxy_json()` uses
  `view.bound_addr.unwrap_or(view.listen)` (`lib.rs:522-539`), so port `0`
  resolves to the actual ephemeral port. Differential tests normalize `listen`
  to `LISTEN` and assert concrete binds per server via `assert_listen`
  (`tests/differential.rs:141-158`, `236-246`).
- **`Logger` field**: every proxy object carries `"Logger":{}` verbatim
  (`lib.rs:531-537`), matching the oracle baseline
  (`oracle-baseline-v2.12.0.md:16-19`).
- **Enabled / import semantics**: create with `enabled=true` (default)
  binds via `create_proxy`; `enabled=false` imports the definition without
  binding, echoing the configured listen (`lib.rs:615-640`; HTTP test
  `lib.rs:1449-1458`). Updates apply `enabled` via `set_enabled` and
  listen/upstream via `update_proxy`; address parse failures on update report
  **500** per the oracle (`lib.rs:644-698`).
- **Populate replace/skip/201 behavior** (`lib.rs:709-758`,
  `lib.rs:1083-1108`): same `listen`+`upstream` → return proxy **untouched**
  (enabled + toxics preserved); changed addresses → delete + recreate
  (**toxics drop**); unknown names → created (honoring `enabled:false`);
  bind failures → **skipped** (`None`); proxies absent from input are **not**
  deleted; empty input echoes `{"proxies":null}`; always **201** on success.

## 4. Recorded divergences (intent-compatible / not supported / incomplete)

Full matrix: `plans/reference/toxiproxy-parity.md:64-83`. Summary in
`docs/toxiproxy.md:18-29`. Module doc pointer: `lib.rs:9-14`.

| Divergence | Level | Rationale / evidence |
| --- | --- | --- |
| Toxicity outside `[0,1]` **clamped** (oracle echoes verbatim) | intent compatible | `clamp_toxicity` (`lib.rs:249-255`); runtime effect preserved (always/never applies); exact clamped values asserted in corpus (`tests/differential.rs:796-806`, `normalize_recorded` `184-234`) |
| Degenerate zero `rate` / `average_size` / `bytes` **coalesce to 1** | intent compatible | Native `NonZero` bounds (`lib.rs:257-262`, `270-297`); asserted in `lib.rs:1288-1322`; normalized in differential (`tests/differential.rs:205-233`) |
| Stream echo **lowercase** (oracle preserves exotic input case) | intent compatible | Native `Direction` round-trip (`lib.rs:383-395`); parity `64-83` |
| Missing `listen` binds **ephemeral loopback** (`127.0.0.1:0`); oracle binds ephemeral wildcard | intent compatible (native invariant wins) | `resolve_addrs` (`lib.rs:601-609`); loopback-by-default invariant (`AGENTS.md`) |
| Non-socket `upstream` → clear **400** (oracle stores arbitrary strings) | not supported (fails clearly) | Native fixed-target invariant (`lib.rs:596-600`); parity `74-76` |
| Proxy names outside `[A-Za-z0-9._-]` → clear **400** (oracle laxer) | not supported (fails clearly) | Native path-segment invariant `crates/eggchaos-server/src/runtime.rs:104-120`; parity `77-78`; surfaces as 400 via `CompatError` mapping (`lib.rs:223-231`, `lib.rs:628`, `lib.rs:637`) |
| Malformed-body / bind-conflict message text differs; status + shape exact | exact status/shape | `lib.rs:83-85`, `lib.rs:622-627`; parity `77-80`; differential checks status-only for malformed (`tests/differential.rs:574-579`) |
| Toxic presentation order **upstream faults then downstream**; creation-interleaved order does not round-trip | intent compatible | `proxy_json` (`lib.rs:524-530`), `list_toxics` (`lib.rs:805-817`); differential sorts by name (comment `lib.rs:803-804`) |
| `reset_peer` termination **platform-qualified** (RST vs FIN not asserted) | intent compatible | Observed termination on darwin/arm64 in differential data-plane (`tests/differential.rs:485-513`); parity `56`, `docs/toxiproxy.md:26` |
| `bandwidth` / `slicer` / `slow_close` data-plane **timing differential incomplete** | incomplete (never claimed) | Parity `52-58`; `docs/toxiproxy.md:27-28`; only `latency` delay+bytes, `timeout=0` blocking, `limit_data` boundary, `reset_peer` termination are differential-verified (`tests/differential.rs:365-513`) |
| `GET /metrics` plain-text 404 | exact | Matches oracle without metrics flags (`plans/reference/toxiproxy-parity.md:41`) |

Version policy: eggchaos targets pinned **v2.12.0 only**, not moving
`main` (`plans/reference/toxiproxy-parity.md:17-20`).

## 5. Evidence

- **Oracle baseline** `qualification/toxiproxy-v2-12/oracle-baseline-v2.12.0.md`:
  live `curl` probes against pinned `toxiproxy-server version 2.12.0`
  (SHA-256 `aa299966b52f16a8594f1cd0d1e9049dc2e8fe2c04a90c19860e2719b2b95d15`
  per `plans/reference/toxiproxy-parity.md:4-6`): proxy object shape with
  `Logger:{}` + `toxics:[]`, 201/409/404/400/500 cases, seven toxic types
  with zero-filled defaults, `<type>_<stream>` auto-naming, unvalidated
  toxicity, ignored type-on-update, PATCH зеркалирование, `POST /reset` 204
  re-enable + clear, `POST /populate` keep semantics, `404 page not found`
  for metrics/unknown.
- **Representative case**
  `qualification/toxiproxy-v2-12/cases/default-latency.json`: latency
  `{"latency":100,"jitter":10}` translation fixture shape.
- **Differential corpus** `crates/eggchaos-toxiproxy/tests/differential.rs`:
  The strict corpus now has **50/50 comparisons pass** against the live pinned oracle
  (`TOXIPROXY_SERVER=/path/to/v2.12.0`); without the binary (or on version
  mismatch) the test reports `incomplete` and passes without asserting
  parity (`differential.rs:515-530`). Corpus compares status + content-type
  class + normalized bodies (`Corpus::check`, `differential.rs:272-317`),
  with declared normalizations only: `listen` → `LISTEN` (disjoint binds,
  concrete ports asserted per server), JSON numbers canonicalized to f64
  (Go `1` vs `serde_json` `1.0`), toxicity clamped, zero numerics coalesced
  (`differential.rs:141-234`, summary `945-957` prints
  `DIFFERENTIAL_SUMMARY` with `"failed":0`). Covers version, list/get/
  create/dup/update/disable/enable/delete proxies, all seven toxic defaults
  + auto-name + list/get/update (POST+PATCH, type/stream ignored) + dup/bad
  type/bad stream/missing proxy + toxicity clamp + delete, populate
  empty/new/missing-name/keep, reset + get-after-reset, metrics/unknown 404,
  plus data-plane: latency byte-preservation + ≥150 ms delay on 200 ms
  config, `limit_data` exact 100/1000 boundary, `timeout=0` blocking +
  post-removal flow, `reset_peer` termination (`differential.rs:365-513`).
- **Client smokes** `qualification/toxiproxy-v2-12/client-smoke/`:
  pinned Go client `github.com/Shopify/toxiproxy/v2@v2.12.0` —
  `go/go_results.json` 13/13 steps pass
  (version/create/populate/add-toxic/add-toxic-auto-name/update/list/
  remove×2/disable/enable/reset/delete); independent Python-stdlib client
  `py_smoke.py` → `py_results.json` 12/12 steps pass (version/create/
  populate/add/update-POST/update-PATCH/list/remove/disable/enable/reset/
  delete). Both recorded as 13/13 in
  `plans/reference/toxiproxy-parity.md:8-11` (Go toolchain go1.27.1).
- **Unit/HTTP translation suite** in `lib.rs:1171-1562`: all-seven mapping +
  downstream default, stream-before-type precedence, name/stream/toxicity
  defaults + clamp, `timeout=0` indefinite round-trip, zero coalescing,
  cross-type merge isolation, version route, create/list/toxic shared-state,
  oracle shapes/errors, reset re-enable + clear.

### 5.1 Post-v2.12 snapshot profile (M040 corrective qualification)

The adapter profile is the single source for proxy/toxic translation and
rendering, including populate keep/recreate responses. `to_fault()` and
`translate_proxy()` remain strict by default; their profile-aware variants
accept `packet_loss` only for the snapshot profile. Snapshot reverse mapping
emits `packet_loss`; strict reverse mapping reports an invalid toxic type.

The mandatory source-built oracle uses commit
`40f7fd31bee529d824116bd2a11a9e3425e904ec`, archive SHA-256
`26351cc70792f1c3391bdd1d376974a79a756b349ec1abfa8b8f1524c042b13d`, and
exact Go toolchain `go1.23.0` (the pinned upstream module declares Go 1.23.0).
The corrective corpus separates 12 exact API comparisons, two isolated exact
data-plane edges, a 256-probe `loss_rate=0.25` comparator with frozen
`0.10..=0.40` drop-fraction bounds, and a 512-probe `loss_rate=0.20`,
`correlation=0.50` conditional-gap comparator with 50-observation minimum
conditioning buckets and a `0.20` gap floor. The latest mandatory run recorded
oracle/Eggchaos intermediate drops of 54/256 and 69/256, and conditional gaps
of 0.6766/0.5204. These are intent-compatible stochastic results, not exact
RNG/chunk-sequence equivalence. See the M040 closure for the exact candidate
and raw qualification output.

## 6. Review checklist

For a systematic reviewer of this adapter:

1. Confirm `ToxiproxyAdapter` holds **no** proxy/toxic maps — only
   `ControlState` — and every read/write path in §1 delegates to it.
2. Walk the §2.1 table row by row against `kind_from_attrs` /
   `attrs_from_kind`: units (ms vs µs vs KiB/s), `NonZero` coalescing,
   `timeout=0 → close_after: None`, `reset_peer → hard_reset: true`,
   64 KiB latency/bandwidth buffers.
3. Check defaults: `default_stream`, `default_toxicity`, zero-filled
   `attributes_object`, `<type>_<stream>` naming incl. `""` input.
4. Check `clamp_toxicity` finiteness + `[0,1]` clamp vs oracle-verbatim
   baseline; confirm differential `normalize_recorded` + explicit clamp
   assertions cover the gap.
5. Check stream-before-type order in both `to_fault` and `add_toxic`, and
   ASCII-case-insensitive accept with lowercase echo.
6. Check `merge_attributes` only picks own-type keys; confirm update ignores
   `type`/`stream` on both POST and PATCH.
7. Walk every route in §3 against `compatibility_request`: method+path,
   status code, content type, envelope shape; especially version charset
   suffix, populate empty/missing-name shapes, 204 empties, 404 plain-text.
8. Confirm bound-address rendering (`bound_addr.unwrap_or(listen)`) and
   `Logger:{}` on every proxy render.
9. Confirm enabled/import split (`create_proxy` vs `import_definition`) and
   update-path 500s on bad addresses.
10. Confirm populate keep (same addrs untouched) / replace (changed addrs
    drop toxics) / create / skip-on-bind-failure / never-delete / always-201.
11. Confirm each §4 divergence is classified in
    `plans/reference/toxiproxy-parity.md` and has either a normalization +
    explicit assertion or an `incomplete`/`not supported` label — never silent.
12. Confirm evidence is execution-based (§5): differential `failed:0` log, Go +
    Python smoke JSON, oracle baseline SHA/version — not source inspection.
13. Confirm loopback-by-default (`compat_server.rs`, `127.0.0.1:0` fallback)
    and 1 MiB body / 128-connection bounds in `ToxiproxyHttp::start`.

## 7. Verification

```sh
cargo test -p eggchaos-toxiproxy
TOXIPROXY_SERVER=/path/to/pinned/v2.12.0 ./scripts/qualify_toxiproxy_v2_12.sh
```

- `cargo test -p eggchaos-toxiproxy` runs the translation + HTTP suites
  (`lib.rs:1171-1562`).
- `scripts/qualify_toxiproxy_v2_12.sh:1-30` runs
  `cargo test -p eggchaos-toxiproxy --all-features`, then — only if a pinned
  `2.12.0` oracle binary is present — the differential corpus and greps for
  `"failed":0`; without the oracle it reports `differential: incomplete`
  (exit 0) rather than treating source inspection as proof.
- Differential directly:
  `TOXIPROXY_SERVER=/path/to/pinned/v2.12.0 cargo test -p eggchaos-toxiproxy --test differential -- --nocapture`
  (usage documented in `tests/differential.rs:1-10`).
- Manual smoke:
  `cargo run -p eggchaos-toxiproxy --example compat_server -- 127.0.0.1:8474`
  (`examples/compat_server.rs:3`), then the Go/Python smokes in
  `qualification/toxiproxy-v2-12/client-smoke/`.
- Repo-wide gates per `AGENTS.md` (once the workspace exists):
  `cargo fmt --check`,
  `cargo clippy --workspace --all-targets --all-features -- -D warnings`,
  `cargo test --workspace --all-features`.
