# M012 — Toxiproxy v2.12 Parity Corrective

Status: closed
Depends on: M009, M010, M011
Successor: M013

## Historical closure note

M012 closed at candidate `a040ed7`; see `plans/closure/M012-toxiproxy-v2-12-parity-corrective-closure.md` (47/47 differential vs pinned v2.12.0 oracle, Go + Python client smokes). Status retained as completed history; M014 reconciles planning state and M015 is the final pre-tag authority.

## Objective

Complete and requalify the declared Toxiproxy v2.12.0 compatibility surface against the pinned official oracle.

The historical M006 implementation established useful DTO translation and a partial HTTP facade, but it does not yet implement the full route/default/mutation behavior claimed by the original compatibility plan. M012 closes that gap without making Toxiproxy vocabulary authoritative inside eggchaos.

## User-visible outcome

Existing Toxiproxy v2.12 clients can use eggchaos for the explicitly documented compatibility surface:

- proxy CRUD/update;
- populate;
- all seven v2.12 toxics with create/get/update/delete;
- enable/disable/reset semantics;
- version;
- compatibility metrics where truthfully supportable;
- active toxic mutation backed by native live policy;
- differential byte/timing behavior for the declared compatibility level.

Any remaining reset-platform divergence is clearly classified as intent-compatible rather than silently treated as exact.

## Baseline findings

The audit found current compatibility HTTP is partial:

- no `POST /populate`;
- no proxy update route;
- no individual toxic GET;
- no toxic update route;
- no toxic delete route;
- no compatibility `/metrics`;
- `POST /reset` is currently a no-op response;
- default toxic name is effectively type-only rather than `<type>_<stream>`;
- invalid stream values fall through to downstream instead of being rejected;
- `/version` returns `2.12.0-eggchaos` rather than the oracle's exact `2.12.0` payload;
- timeout=0 translation does not currently preserve the oracle's indefinite-blackhole behavior;
- reset_peer timeout is not mapped into executable delayed reset semantics;
- compatibility presentation state is held in a separate mutable `BTreeMap`, creating divergence risk from native state.

M009–M011 must close first so M012 maps onto corrected native behavior.

## Scope

Primary surfaces:

```text
crates/eggchaos-toxiproxy/src/
crates/eggchaos-toxiproxy/Cargo.toml
qualification/toxiproxy-v2-12/
scripts/qualify_toxiproxy_v2_12.sh
docs/toxiproxy.md
plans/reference/toxiproxy-parity.md
```

Native runtime changes should be exceptional; use the M010/M011 control authority rather than adding compatibility-only execution state.

## Non-goals

Do not:

- target moving Toxiproxy `main`;
- add current-main `packet_loss`;
- reproduce Go implementation internals;
- weaken native security defaults;
- create a second listener/fault execution registry;
- claim exact TCP RST behavior where platform evidence cannot support it.

## Compatibility state authority

Eliminate the compatibility adapter's independent mutable proxy definition as an execution/presentation authority.

Preferred approach:

- canonical proxy/listener/fault state lives in the M010 runtime control authority;
- compatibility responses are derived from canonical native state plus narrowly scoped compatibility metadata only if a field cannot round-trip;
- any metadata is keyed to the canonical proxy/fault identity + generation and cannot survive deletion/replacement incorrectly.

Because all seven v2.12 toxics have typed native representations after M009, implement reverse translation `FaultSpec -> Toxic` where possible and use it for list/get responses.

Do not let native mutation leave Toxiproxy GET/list permanently stale.

## Exact v2.12 route family

Implement and differential-test:

```text
GET    /proxies
POST   /proxies
POST   /populate
GET    /proxies/{proxy}
POST   /proxies/{proxy}
DELETE /proxies/{proxy}

GET    /proxies/{proxy}/toxics
POST   /proxies/{proxy}/toxics
GET    /proxies/{proxy}/toxics/{toxic}
POST   /proxies/{proxy}/toxics/{toxic}
DELETE /proxies/{proxy}/toxics/{toxic}

POST   /reset
GET    /version
GET    /metrics
```

Use the pinned v2.12.0 oracle to determine:

- status codes;
- response body/empty-body behavior;
- content type;
- defaults;
- duplicate/not-found semantics;
- malformed body behavior;
- update replacement behavior.

Do not infer undocumented details if the oracle can be run.

## Proxy lifecycle translation

Compatibility create/update/delete/enable/disable must call the M010 runtime lifecycle authority so returned state corresponds to an actual listener.

Port 0 must return the actual bound port in compatibility JSON.

Proxy update semantics for listen/upstream/enabled must match the oracle at the API level while using native restart-class operations internally.

## Toxic defaults and validation

Capture and reproduce v2.12 rules.

At minimum:

- stream defaults to `downstream`;
- only `upstream` / `downstream` accepted;
- toxicity defaults to 1.0 and validates supported range;
- omitted name defaults to `<type>_<stream>` if oracle confirms;
- toxic names are unique within proxy according to oracle behavior;
- attributes validate required/default edge values exactly.

Unknown stream must not silently become downstream.

## Seven toxic mappings

### latency

Map milliseconds/jitter into corrected M009 latency semantics.

Differential timing uses numeric tolerance windows, not exact scheduler timestamps.

### bandwidth

Determine v2.12 rate unit from source/oracle and document it. Map onto M009 sustained rate + compatibility burst policy.

Differential-test sustained throughput and initial burst behavior. If exact Go chunk scheduling differs, classify the deviation explicitly.

### timeout

Oracle timeout=0 means indefinite blockage until toxic removal/change. Map exactly to native `Blackhole { close_after: None }`.

Positive timeout maps to finite blackhole then connection termination as corrected by M009.

### slow_close

Map delay and differential-test close timing without applying delay to normal writes.

### reset_peer

Map timeout into M009 delayed disconnect with `hard_reset=true`.

At M010 TCP edge, record whether an actual reset is applied. Compatibility table may be behaviorally compatible on platforms where observed RST matches oracle and intent-compatible elsewhere.

### slicer

Map average size, variation, delay. Use corrected deterministic native slicer.

Exact random sequence need not match Go unless clients depend on it; compare externally visible size/rate/timing bounds.

### limit_data

Map exact byte boundary and verify the connection terminates rather than silently discarding the suffix.

## Populate

Build an oracle corpus specifically for:

- empty list;
- first population;
- identical population repeated;
- proxy added;
- proxy removed;
- upstream changed;
- listen changed;
- enabled changed;
- existing toxics interaction;
- one invalid entry among valid entries.

Implement the observed atomicity/idempotence semantics through M010 control transactions.

## Reset

Toxiproxy reset must perform actual oracle-equivalent state change.

Expected behavior from the initial plan: re-enable all proxies and remove all toxics. Verify against v2.12.0 and implement through one native transaction where practical.

Do not return a success-only placeholder.

## Version

If oracle returns:

```json
{"version":"2.12.0"}
```

then compatibility `/version` should match that exact version field. Eggchaos identity belongs in native `/v1/version`, headers, logs, or documentation, not by mutating a compatibility field clients may parse.

## Metrics

Oracle-test `/metrics`.

Expose Toxiproxy-compatible names/labels only where backed by truthful native counters. It is acceptable to omit or classify unsupported metric families if exact data is unavailable, but the compatibility matrix must state this.

Do not fabricate byte totals.

## Differential corpus

Expand `qualification/toxiproxy-v2-12/` into a real oracle comparison corpus.

Required groups:

1. proxy CRUD/defaults/errors;
2. populate;
3. toxic CRUD/defaults/errors for every type;
4. upstream/downstream direction isolation;
5. active toxic add/update/remove;
6. byte behavior for preserving/destructive toxics;
7. latency/slow-close timing tolerances;
8. bandwidth throughput tolerance;
9. timeout 0/positive;
10. reset_peer observed transport result;
11. limit-data exact boundary;
12. metrics/version;
13. port 0 actual bind address.

The qualification command must fail on an unexpected divergence and emit a machine-readable summary.

## Existing client smoke

Run at least:

- pinned Toxiproxy Go client corresponding to v2.12.0 where practical;
- one independent maintained client, such as Rust/Python/Java.

Smoke sequence:

- create/populate;
- add latency;
- update latency;
- remove toxic;
- disable/enable;
- reset;
- delete.

Record exact client versions.

## Ordered work packages

1. **WP1 — Oracle capture:** pin v2.12.0 binary/checksum and capture route/default/error/populate/reset behaviors before changing adapter semantics.
2. **WP2 — Canonical-state adapter:** remove independent mutable compatibility authority and add native<->Toxic reverse translation.
3. **WP3 — Route completion:** implement all required proxy/toxic/populate/reset/version/metrics routes through M010 control methods.
4. **WP4 — Toxic semantic corrections:** strict defaults/validation and corrected M009 timeout/reset/slicer/bandwidth/limit mappings.
5. **WP5 — Differential data-plane corpus:** exercise all seven toxics in both directions including active updates.
6. **WP6 — Client smoke:** run pinned Go plus one independent client and record versions/results.
7. **WP7 — Compatibility matrix/docs:** update parity classifications and list any platform-specific intent-only behavior.
8. **WP8 — Closure:** exact-oracle exact-commit qualification and M012 closure record.

## Required verification

At minimum:

```sh
cargo fmt --all -- --check
cargo clippy -p eggchaos-toxiproxy --all-targets --all-features -- -D warnings
cargo test -p eggchaos-toxiproxy --all-features
cargo test --workspace --all-features
TOXIPROXY_SERVER=/path/to/pinned/v2.12.0 ./scripts/qualify_toxiproxy_v2_12.sh
```

Record oracle checksum and platform.

## Acceptance criteria

M012 closes only when:

- required v2.12 routes are implemented;
- create/update/delete correspond to real native listener state;
- populate and reset are real operations;
- toxic default names/streams/toxicity match oracle;
- invalid stream is rejected;
- timeout=0 is indefinite;
- reset_peer timeout reaches the corrected disconnect/reset path;
- compatibility version field matches oracle;
- adapter presentation cannot drift from native state;
- differential corpus passes within declared exact/tolerance comparators;
- client smokes pass for the claimed surface;
- compatibility matrix is truthful and updated;
- exact-commit closure evidence exists.

## Stop/rejection conditions

Stop/revise if:

- a required behavior cannot be observed from the pinned oracle;
- compatibility requires a second execution registry;
- API comparison normalizes away meaningful status/body differences;
- reset_peer is called behaviorally compatible without observing reset behavior;
- current-main features are pulled into the v2.12 claim;
- native semantics are weakened solely to mimic a compatibility quirk that can live in translation.

## Follow-on activation

On M012 closure, M013 becomes ready.
