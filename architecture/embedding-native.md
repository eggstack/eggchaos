# Embedding and native bindings

Back to [architecture overview](overview.md) (§8, workspace-map rows
`eggchaos-embed` and `eggchaos-native`).

Evidence-first deep dive for the in-process embedding surface. Authority is
code at HEAD (M041; M034/M035 origin); `docs/control-plane.md:210-217` and
`bindings/python-native/README.md` are summaries, not the spec. All paths
below are relative to the workspace root.

Sources: `crates/eggchaos-embed/src/lib.rs`,
`crates/eggchaos-embed/Cargo.toml`,
`crates/eggchaos-embed/tests/facade.rs`,
`bindings/python-native/` (`Cargo.toml`, `src/lib.rs`,
`python/eggchaos_native/__init__.py`, `pyproject.toml`, `README.md`,
`tests/test_native.py`, `tests/test_conformance.py`),
`scripts/check_python_native.sh`, `scripts/qualify_python_native.sh`,
`scripts/build_python_native_artifacts.sh`,
`plans/closure/M034-python-native-embedding-pilot-and-binding-qualification-closure.md`,
`plans/closure/M035-cross-language-qualification-and-closure-corrective-closure.md`.

## 1. Purpose and boundary

`eggchaos-embed` is the safe coarse embedding facade over the existing
authorities. It owns lifecycle on a private Tokio runtime and delegates every
state change to server / control / experiment. It never re-implements
networking, policy publication, RNG derivation, or schedule compilation
(`lib.rs:1-27` header).

| Concern | Owner | Embed role |
| --- | --- | --- |
| TCP/UDP listeners, relay, associations | `eggchaos-server` runtime + `eggress-relay` | none; built via public `ServiceBuilder` path |
| Proxy/fault/connection/scenario state | `ControlState` / `DatagramRuntime` | caller; converts DTOs, maps errors |
| Wire shapes, operation inventory | `eggchaos-protocol` DTOs | request/response types only |
| Scenario V2 compile/fingerprint/epoch driver | `eggchaos-experiment` (via server re-export `compile_schedule`) | `validate/compile` call it directly; `apply` goes through `ControlState` |
| RNG, engines, policy snapshots | `eggchaos-core` | none |

`eggchaos-native` (`bindings/python-native/`) is the PyO3/maturin pilot over
`eggchaos-embed` (abi3 wheel). It is a standalone crate outside the workspace;
maturin owns its build. There is no generic C ABI — per the M034 closure
go/no-go decision and M035 non-claims, a generic C ABI (and Node addon, JNI,
P/Invoke, cgo, UniFFI, WASM) requires a separate ADR plus demonstrated
multi-consumer demand.

Blocking contract (`lib.rs:15-27`):

- Every `EmbeddedService` method blocks the calling thread. Foreign callers
  never observe Rust futures, `Arc`s, borrowed references, or
  `tokio::time::Instant`.
- Do not call from inside an async context running on the embedded runtime.
  The runtime is private, so this only happens if the caller blocks its own
  executor thread while another task on the same thread awaits the facade —
  use a dedicated thread instead.
- Concurrent calls from multiple OS threads are safe (pinned by the 8-thread
  test, `facade.rs:58-75`).
- Python adds: all blocking control releases the GIL (`py.detach`); async
  harnesses use `asyncio.to_thread`; the binding holds no Python event-loop
  state (`README.md:28-30`).

## 2. Crate layout and dependencies

`crates/eggchaos-embed/Cargo.toml:15-23`:

```toml
eggchaos-core = { path = "../eggchaos-core", version = "0.1.0" }
eggchaos-experiment = { path = "../eggchaos-experiment", version = "0.1.0" }
eggchaos-protocol = { path = "../eggchaos-protocol", version = "0.1.0" }
eggchaos-server = { path = "../eggchaos-server", version = "0.1.0" }
serde, serde_json, thiserror, tokio
```

Dev-dependency: `eggfetch-core` (workspace) — used only by the facade/HTTP
conformance tests to drive `NativeAdmin` over the facade's own
`ControlState` (`facade.rs:231-272,300-313`). `#![deny(unsafe_code)]`
(`lib.rs:28`).

`EmbeddedService::start` (`lib.rs:124-159`) builds the service through the
same authority `eggchaos serve` uses:

1. `tokio::runtime::Builder::new_multi_thread().enable_all()`
   `.thread_name("eggchaos-embed")` — private owned runtime.
2. `ServiceBuilder::new(options.seed)` + `runtime_admission_limits`,
   `relay_buffer()`, `termination_grace()`, `runtime_datagram_limits`
   from `EmbedOptions.runtime: RuntimeConfigV1` (bounds only; listeners come
   from proxies). Limit-adapter failures map to `EmbedError::Validation`;
   `ServiceBuilder::build` failures map to `Validation`.
3. `runtime.block_on(service.start())` — start failures map to
   `EmbedError::Lifecycle` — then `.control_state()`.

`EmbedOptions` (`lib.rs:91-98`): `{ seed: u64, runtime: RuntimeConfigV1 }`,
`Default` is seed `0` plus default bounds. `ScenarioRunView`
(`lib.rs:100-108`): `#[serde(tag = "family")]` enum `V1(ScenarioRunV1)` /
`V2(ScheduleRunV2)`.

## 3. Facade inventory

### 3.1 Lifecycle and snapshots

| Method (`lib.rs`) | Semantics |
| --- | --- |
| `start(EmbedOptions)` | create private runtime + `ControlState`; no proxies |
| `shutdown(&self)` (`191-197`) | idempotent: `swap(closed)` → `initiate_shutdown()` → `block_on(shutdown_and_join())` |
| `is_closed()` | `AtomicBool` load; `true` after shutdown requested/completed |
| `generation()` (`177-179`) | synchronous `control.generation()` snapshot read (no `block_on`) |
| `control_state()` (`186-188`) | clone the underlying `ControlState` handle (advanced Rust escape hatch; aliases service state; prefer coarse methods) |
| `health()` / `version()` | `{running:true, generation}` / `{version: eggchaos_server::VERSION, api:"v1"}` via `block_on` |
| `metrics_text()` | Prometheus text, never JSON (same `control.metrics_text()`) |
| `reset()` | `control.reset()` for stream + datagram state; returns `ResetReport` |
| `error_envelope(&EmbedError)` (`723-734`) | render shared native `ErrorEnvelopeV1` (see §5) |
| `Drop` (`737-746`) | best-effort: signal shutdown without joining; owned-runtime release aborts (never detaches) any remainder — always prefer explicit `shutdown()` |

Private `block_on` (`161-169`) rejects with
`Lifecycle("service is closed")` once `closed` is set, so post-shutdown calls
fail fast without touching the runtime. Proven by
`facade.rs:start_health_version_shutdown_and_double_close`
(`list_proxies` errors after close),
`drop_without_close_still_shuts_down`, and
`repeated_construction_has_independent_state`.

### 3.2 Stream proxies / faults / connections / history

Thin convert-then-delegate wrappers; every arm is `block_on(async { convert;
control.<op>.await.map_err(EmbedError::from); convert view })`:

- Proxies (`231-296`): `create_proxy(NativeProxyRequestV1) →
  (NativeProxyViewV1, generation)` via `proxy_request_into_spec`;
  `list_proxies`, `get_proxy` (`NotFound` when absent),
  `patch_proxy` (calls `patch.validate()` first), `delete_proxy →
  generation`.
- Faults (`300-382`): `add_fault(proxy, FaultUpsertV1) →
  (Direction, NativeFaultViewV1, generation)` via
  `fault_upsert_into_runtime`; `list_faults → (upstream[], downstream[])`
  (`NotFound` for unknown proxy); `get_fault → (Direction, view)`;
  `patch_fault` via `fault_patch_into_runtime` plus an explicit
  empty-patch guard (`probability.is_none() && kind.is_none()` →
  `Validation`, matching the HTTP path — `facade.rs:109-116`);
  `remove_fault → generation`.
- Connections/history (`387-412`): `connections()`, `get_connection(id)`
  (`NotFound("connection {id}")`), `kill_connection(id) → bool`, `history()`.
  No payload capture; snapshots come straight from `ControlState`.

### 3.3 Scenarios V1 + V2

| Method | Path |
| --- | --- |
| `apply_scenario_v1(ScenarioV1)` (`417-427`) | `scenario_v1_into_runtime` → `control.start_scenario`; errors → `Validation` |
| `validate_schedule_v2(ScenarioScheduleV2Dto)` (`430-438`) | **synchronous** (no `block_on`): `into_internal()` → `compile_schedule()` → `ScheduleValidateV2::from_compiled` |
| `compile_schedule_v2` (`442-449`) | same synchronous path → `ScheduleCompileV2::from_compiled` |
| `apply_schedule_v2` (`452-465`) | `into_internal()` → `control.start_schedule_v2`; errors → `Validation` |
| `get_scenario(run_id)` (`468-478`) | try V1 record, then V2 record → `ScenarioRunView::V1/V2`; else `NotFound("scenario run {id}")` |
| `cancel_scenario(run_id)` (`481-491`) | try V1 cancel, then V2 cancel; same `NotFound` |

V1 and V2 runs share one run-id namespace on lookup/cancel. Pinned by
`facade.rs:scenario_v1_and_v2_lifecycle` (validate `event_count == 1`,
compile `events.len() == 1`, get/cancel return the `V2` family).

### 3.4 Datagram proxies / faults / associations

- Proxies (`496-574`): `create_datagram_proxy` via
  `datagram_proxy_request_into_spec`; `list_datagram_proxies`,
  `get_datagram_proxy` (`NotFound`), `patch_datagram_proxy` with an explicit
  empty-patch guard over all five fields (`enabled/listen/upstream/
  max_associations/association_idle_timeout_ms` → `Validation("empty
  datagram proxy patch")`), `delete_datagram_proxy → generation`.
- Faults (`582-700`): `add_datagram_fault` via
  `datagram_fault_upsert_into_runtime` → `control.add_datagram_fault` →
  `(Direction, DatagramFaultSpecV1, generation)`; `get_datagram_fault`;
  `list_datagram_faults → (upstream[], downstream[])` (added in M035);
  `patch_datagram_fault` via `datagram_fault_patch_into_runtime` →
  `control.update_datagram_fault`; `remove_datagram_fault → generation`
  (discards the direction, returns the next generation). Mutation semantics
  live in `ControlState` — see §6.
- Associations (`702-720`): `datagram_associations()`,
  `kill_datagram_association(id) → bool`.

## 4. Error-envelope mapping

`EmbedError` (`lib.rs:49-73`) mirrors the remote client categories; details
are bounded and carry no secrets:

| `EmbedError` | `error_envelope` code | From `ControlError` (`75-89`) |
| --- | --- | --- |
| `Validation(_)` | `invalid` | `Invalid(_)` |
| `NotFound(_)` | `not_found` | `NotFound(_)` |
| `Conflict(_)` | `conflict` | `Conflict(_)` |
| `Unsupported(_)` | `unsupported` | — (facade-only capability gate) |
| `Lifecycle(_)` | `lifecycle` | `RestartFailed{..}` (`"{proxy}: {reason}"`) |
| `Bind(_)` | `bind_failed` | `BindFailed{..}` (`"{proxy}: {reason}"`) |
| `Internal(_)` | `internal` | — (never a panic, never a secret) |

Note the deliberate narrow divergence from HTTP status mapping: the server
maps `RestartFailed` to HTTP `500 restart_failed`, while embed folds it into
`Lifecycle`. HTTP-only codes (`invalid_json`, `unauthorized`,
`serialization`, body `413`) never arise in-process. Pinned by
`facade.rs:invalid_config_and_missing_resources_map_to_categories` (bad name
→ `Validation`, absent proxy/run → `NotFound`).

## 5. Shared datagram mutation authority (M035)

M034 shipped the only logic duplication in the facade (datagram fault CRUD
replicating the admin route flow). M035 removed it: `ControlState` now owns
`add/get/list/update/remove_datagram_fault` in core/runtime types
(`crates/eggchaos-server/src/runtime/control.rs:440,482,498,519,575`) —
same-direction duplicate, cross-direction ID uniqueness, patch non-emptiness
after DTO conversion, plan reconstruction, generation-guarded publication.
`NativeAdmin` (`crates/eggchaos-server/src/admin.rs:381,410`) and
`eggchaos-embed` (`lib.rs:582-700`) only convert DTOs
(`datagram_fault_upsert_into_runtime`, `datagram_fault_patch_into_runtime`
via `datagram_fault_patch_into_parts` in
`crates/eggchaos-server/src/native.rs:210-232`) and map errors; neither
contains plan reconstruction or publication logic anymore.

Conformance proof is
`facade.rs:datagram_facade_and_http_admin_agree_on_mutation_and_conflicts`
(11 steps on one shared `ControlState` with `NativeAdmin` mounted on it):
proxy-create view equality; embed-add then HTTP get/list equality
(`direction` + `fault` object); embed `list_datagram_faults` vs HTTP
`upstream/downstream` lists; same-direction and cross-direction duplicate
adds are `Conflict` on embed and `409` on HTTP; HTTP patch visible via
embed; embed patch visible via HTTP; empty patches rejected on both
(`Validation` / `400`); HTTP delete then `NotFound` on both surfaces;
proxy delete then `404` on HTTP. The stream sibling
`facade_and_http_admin_agree_on_protocol_views` asserts byte-identical proxy
and fault views between facade and HTTP. Any reintroduced semantic split
fails here by construction. Test hygiene note: embed calls stay outside the
test's own `block_on` — the facade owns a private runtime and cannot block
from inside another runtime (`facade.rs:338-339`).

## 6. Native binding (`eggchaos-native`)

### 6.1 Crate layout

Standalone maturin crate outside the workspace
(`bindings/python-native/Cargo.toml:1-6` — plain cargo cannot link a macOS
extension-module cdylib, so an empty `[workspace]` detaches it; maturin owns
the link step):

```toml
[lib] name = "eggchaos_native" crate-type = ["cdylib"]
eggchaos-embed = { path = "../../crates/eggchaos-embed", version = "0.1.0" }
eggchaos-protocol = { path = "../../crates/eggchaos-protocol", version = "0.1.0" }
pyo3 = { version = "0.29", features = ["extension-module", "abi3-py311"] }
serde 1 (derive), serde_json 1
```

`pyproject.toml`: `maturin==1.9.5`, `bindings = "pyo3"`,
`module-name = "eggchaos_native"`, `python-source = "python"`,
`requires-python = ">=3.11"`. `python/eggchaos_native/__init__.py`
re-exports `Fault`, `Service`, and the 7 typed errors with remote-vs-native
selection docs.

### 6.2 Managed-objects-only rule

`Service` / `Fault` pyclasses plus plain dicts/lists/strings/numbers only —
no Tokio streams, futures, borrows, or `Instant` cross the boundary
(`src/lib.rs:346-353` header; `README.md:5-6`). Conversion is a JSON
round-trip: Rust values serialize via `serde_json::to_value` then convert
element-wise (`to_python`/`value_to_python`, `77-81,42-75`); Python inputs
convert back with strict rules (`python_to_value`, `825-881`: `bool` before
`int`, `u64`-then-`i64` ints, finite floats only, string mapping keys only,
lists/tuples/dicts recursed, anything else `PyValueError`). No Python
callback runs on data-plane hot paths; Python never executes per-packet code
(`README.md:49-50`).

`Fault` (`84-344`) is a value object `{direction, id, probability, kind}` with
14 static factories: stream `latency`, `bandwidth`, `blackhole`,
`limit_data`, `slow_close`, `slice`, `disconnect`, `stream_loss`
(kebab-case `stream-loss` DTO), plus datagram `datagram_delay`,
`datagram_loss`, `datagram_duplicate`, `datagram_reorder`,
`datagram_corrupt` (`payload-corrupt` DTO), `datagram_bandwidth`.

`Service` (`355-816`) maps embed 1:1 except where narrowed (see §6.5):
`new(*, seed=0)`, context-manager `__enter__`/`__exit__` + idempotent
`close()` (takes the inner service, shuts down) + `closed()`;
`health`, `version`, `metrics_text` (raw text), `reset`;
`create_proxy(*, name, listen, upstream, enabled=true, max_connections?,
connect_timeout_ms?, seed?)`, `list_proxies`, `get_proxy`, `patch_proxy`,
`delete_proxy`; `set_fault` (from a `Fault`), `list_faults`,
`get_fault`, `patch_fault(*, probability?, kind?)`, `remove_fault`;
`connections`, `kill_connection`, `history`;
`create_datagram_proxy(*, name, listen, upstream, max_associations?,
association_idle_timeout_ms?, max_queued_datagrams?, max_queued_bytes?,
max_datagram_size?, seed?)`, `list_datagram_proxies`, `get_datagram_proxy`,
`delete_datagram_proxy`, `set_datagram_fault`, `get_datagram_fault`,
`remove_datagram_fault`, `datagram_associations`;
`scenario_apply` (dispatches on `document["version"] == 2` → V2 schedule else
V1), `schedule_validate`, `schedule_compile`, `scenario_get`,
`scenario_cancel`.

Every blocking facade call is wrapped in `py.detach(...)` (GIL release);
the CPU-only `schedule_validate`/`schedule_compile` call the facade directly
(`783-799`). Use-after-close (or use of a closed handle) raises
`NativeLifecycleError` via `require()` (`818-823`).

Errors map one-to-one (`convert_error`, `21-31`): `Validation` →
`NativeValidationError`, `NotFound` → `NativeNotFoundError`, `Conflict` →
`NativeConflictError`, `Unsupported` → `NativeUnsupportedError`, `Lifecycle`
→ `NativeLifecycleError`, `Bind` → `NativeBindError`, `Internal` →
`NativeInternalError` — all subclasses of `NativeError` (`33-40`).

### 6.3 Handwritten-unsafe audit

The workspace forbids handwritten `unsafe` in normal crates; the binding
crate carries a crate-local `#![allow(unsafe_code)]` for one reason only:
PyO3 `#[pymodule]`/`#[pyclass]`/`#[pymethods]` macros expand to
framework-owned FFI glue containing `unsafe` blocks. There is no handwritten
`unsafe` in `src/lib.rs` (`1-13` header; M034 closure audit).
`scripts/check_python_native.sh:11-14` enforces it:

```sh
grep -rnE "unsafe[[:space:]]*(\{|\(|fn|impl|trait|extern)" bindings/python-native/src  # must be empty
grep -n "allow(unsafe_code)" bindings/python-native/src/lib.rs                          # must be present
```

### 6.4 abi3 wheel inspection and host-aware target selection

`check_python_native.sh:21-38` derives the target from OS + architecture:
only a Darwin host may select an Apple target (`x86_64` → `--target
x86_64-apple-darwin`; Darwin/arm64 falls through to native-host); Linux and
Windows always use native-host builds. `EGGCHAOS_NATIVE_TARGET` overrides for
intentional cross builds, which are never runtime-qualified without a
matching import. The gate then `maturin build`s, asserts the wheel contains
a `.abi3.so` (`zipfile` check), installs with `pip install --target` +
`--no-deps`, and runs the server-independent Python tests.
`qualify_python_native.sh:21-36` repeats the same selection; the inline bench
step rebuilds the wheel the same way. `build_python_native_artifacts.sh`
builds the host-native abi3 wheel plus an sdist, cross-builds Apple wheels
only on a Darwin host with the target installed, then import-smokes only the
wheel matching the interpreter arch (`x86_64`/`arm64`/`universal2`).

### 6.5 Import/runtime smoke, conformance, overhead

- `tests/test_native.py` (10 tests): import smoke, context-manager close on
  normal return and on exception, explicit + double close, repeated
  independent instances, stream/datagram CRUD, connections/history/reset/
  metrics-text/version, V2 validate/compile/apply/get/cancel plus V1
  apply/get, all error categories incl. conflict and bad-direction
  validation, 4-thread parallel independent instances, interpreter shutdown
  without close exiting 0 in a subprocess.
- `tests/test_conformance.py`: same representative scenario (stream
  proxy+fault, V2 seed `21` / execution key `22` validate/compile with
  fingerprint, proxy fault views) through the M033 remote client (loopback
  daemon at `EGGCHAOS_ADMIN_URL`) and the native module: proxy seeds, fault
  views, schedule fingerprint, compiled action, and downstream fault lists
  are timing-independently equal.
- `qualify_python_native.sh:62-125` additionally records (not budgets)
  `native_startup_shutdown`, `native_create_delete_proxy`,
  `native_fault_mutation` vs `remote_fault_mutation` (M034 candidate:
  0.05 ms vs 0.45 ms, ~9× control-overhead reduction from removing the HTTP
  hop — a control-plane observation, not a data-plane claim), and
  `native_schedule_validate` into `bench-native.json`. Python never enters
  the byte/datagram hot path by construction (control-only facade).

Native coverage is a coarse subset of embed by design: it exposes no
`patch_datagram_proxy`, no `list_datagram_faults`, no `patch_datagram_fault`,
no `get_connection`, and no `control_state()` escape hatch. Anything needing
those stays on the Rust facade or the remote client.

### 6.6 Platform support tiers (`README.md:52-69`)

- Hosted runtime-qualified: Linux x86_64 (`ubuntu-latest`, Python 3.12) and
  macOS native architecture (`macos-latest`, Python 3.12), via the dedicated
  `python-native` CI job (`check_python_native.sh` +
  `qualify_python_native.sh`: embed/binding tests, unsafe audit, abi3
  inspection, import/runtime smoke, remote/native conformance). No claim is
  inferred from the Rust CLI artifact matrix.
- Built-only: macOS cross-arch wheels from `build_python_native_artifacts.sh`
  on a Darwin host (import-smoked only when a matching interpreter is
  present).
- Unqualified: Windows and every other platform (no hosted native-Python
  gate, no import/runtime smoke — no support claimed).
- Remote-control users who need portability without a compiler prefer
  `eggchaos-client` (`bindings/python-client/README.md`, stdlib-only
  `Client` + `AsyncClient`); TypeScript users use `@eggstack/eggchaos-client`
  (zero-dep, injectable `fetch`). Neither remote SDK manages daemon
  lifecycle or uses FFI.

## 7. Review checklist

For any change touching `crates/eggchaos-embed/` or
`bindings/python-native/`:

1. **Boundary.** Does the diff add networking, policy publication, RNG, or
   schedule compilation to embed? It belongs in server/experiment/core
   (`lib.rs:1-27`). Does the binding add a second state store, per-packet
   Python, or an event-loop-coupled async API? Reject — control-only facade
   plus `asyncio.to_thread` offload (`README.md:28-30`).
2. **Blocking contract.** Do new facade methods block without exposing
   futures/`Arc`s/borrows/`Instant`? Do new binding methods wrap blocking
   calls in `py.detach`? Is the never-call-from-inside-the-embedded-runtime
   rule still documented?
3. **Lifecycle.** Is `shutdown` still idempotent with `Drop` as
   initiate-without-join (abort, never detach)? Do new tests cover
   double-close, drop-without-close, and independent instances?
4. **Errors.** Do new failure modes map to the 7 existing categories with
   bounded, secret-free details? Does `error_envelope` still agree with the
   native envelope codes? Use-after-close must stay `Lifecycle`.
5. **Datagram authority.** Does any datagram mutation logic (uniqueness,
   reconstruction, publication) leak back into HTTP or embed adapters? Both
   must stay convert-and-map over `ControlState`; the 11-step conformance
   test must keep passing.
6. **Native subset discipline.** New embed methods are not automatically
   binding methods; each addition needs a factory/shape, error, and test
   entry. No `patch_datagram_proxy` / `list_datagram_faults` /
   `patch_datagram_fault` / `get_connection` / `control_state` in Python
   unless deliberately added with tests.
7. **Unsafe.** Does `grep -rnE "unsafe..."` stay empty apart from the one
   `allow(unsafe_code)` line? Any handwritten `unsafe` stops the milestone
   and needs a new ADR.
8. **Wheels/host-awareness.** Do scripts still select Apple targets only on
   Darwin, keep `EGGCHAOS_NATIVE_TARGET` as override-only, abi3-inspect the
   wheel, and import-smoke only matching-arch wheels? Are platform claims
   still limited to import-smoked interpreters?
9. **Docs.** Do `bindings/python-native/README.md` (tiers), `docs/control-plane.md`
   (remote-vs-native selection), and this handoff still describe the
   implemented subset? No generic C ABI language may creep in.

## 8. Verification

```sh
cargo test -p eggchaos-embed --all-features
cargo test --manifest-path bindings/python-native/Cargo.toml --all-features
sh scripts/check_python_native.sh      # facade + binding tests, unsafe + binding audits, abi3 build, server-independent Python tests
sh scripts/qualify_python_native.sh    # loopback daemon + remote/native conformance + overhead table
sh scripts/build_python_native_artifacts.sh  # host abi3 wheel + sdist (+ Apple cross-arch on Darwin) + matching-arch import smoke
```

`check_python_native.sh` prints `{"python_native":"pass"}`;
`qualify_python_native.sh` prints `{"python_native_qualify":"pass"}`;
`build_python_native_artifacts.sh` prints
`{"python_native_artifacts":"pass"}`. Wall-clock bench figures are recorded
observations, not frozen budgets. Hosted qualification is the `python-native`
CI job (`[ubuntu-latest, macos-latest] × python 3.12`, pinned
`maturin==1.9.5`); local runs additionally need the workspace gate
(`scripts/check.sh`). Record un-runnable interpreters/platforms as
incomplete evidence, never as passes.

## 9. References

- `crates/eggchaos-embed/src/lib.rs` — facade authority (options, service,
  error mapping, `control_state` hatch, `Drop`).
- `crates/eggchaos-embed/tests/facade.rs` — 10 tests: lifecycle/threads/Rust
  CRUD plus stream and datagram HTTP/embed conformance.
- `crates/eggchaos-server/src/runtime/control.rs:440-575` — shared datagram
  fault mutation authority.
- `crates/eggchaos-server/src/native.rs:210-232` — datagram DTO converters;
  `admin.rs:381,410` — HTTP convert-and-map call sites.
- `bindings/python-native/src/lib.rs` — PyO3 `Service`/`Fault`, GIL release,
  JSON conversion, error mapping, unsafe-boundary header.
- `bindings/python-native/tests/test_native.py`, `test_conformance.py` —
  lifecycle/control/errors plus remote/native timing-independent equality.
- `bindings/python-native/README.md` — usage, lifecycle rules, safety
  boundary, platform tiers.
- `docs/control-plane.md:198-217` — remote SDKs vs in-process embedding
  selection.
- `plans/closure/M034-python-native-embedding-pilot-and-binding-qualification-closure.md`,
  `plans/closure/M035-cross-language-qualification-and-closure-corrective-closure.md` —
  pilot implementation and corrective qualification evidence.
- `architecture/overview.md` — workspace index; this file is its §8 deep
  dive.
