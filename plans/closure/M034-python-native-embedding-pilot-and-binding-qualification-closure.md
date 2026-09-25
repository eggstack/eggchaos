# M034 — Python Native Embedding Pilot and Binding Qualification — Closure

Status: closed
Exact qualification candidate: `991818b1ef2afcfcf3bcc659a8edcd89cafd9550`
Depends on: M033 (closed at `429d459`), M032 (closed at `ed05f68`), ADR 006 (accepted)

## Objective verdict

M034 adds the safe `eggchaos-embed` Rust facade over the existing
server/control/experiment authorities and qualifies a PyO3/maturin
Python native pilot (`eggchaos-native`) that runs eggchaos in-process
with managed objects and snapshot/value models. Deterministic
semantics, validation, bounds, and lifecycle guarantees match the
qualified remote surfaces; remote/native conformance passes on shared
fixtures. No generic C ABI was created. No stop condition fired. The
ADR 006 chain is now complete with no automatic successor.

## WP1 — Embedding API and runtime ownership

`EmbeddedService::start(EmbedOptions)` owns one private multi-thread
Tokio runtime plus a `ControlState` built through the public
`ServiceBuilder` path (the same authority `eggchaos serve` uses). Every
method blocks the calling thread; foreign callers never observe Rust
futures, `Arc`s, borrows, or `Instant`s. Concurrent calls from multiple
OS threads are safe (proven by test); calling from inside an async
context on the embedded runtime is documented misuse. `shutdown()` is
idempotent and joins supervised tasks; `Drop` initiates shutdown
without joining and the owned runtime release aborts (never detaches)
any remainder. Errors are `EmbedError` categories
(validation/not-found/conflict/unsupported/lifecycle/bind/internal)
aligned with the remote client, carrying bounded details and no
secrets.

## WP2 — `eggchaos-embed` implementation

New publishable workspace crate depending only on
core/experiment/protocol/server. It assembles requests through the now
public server adapters (`proxy_request_into_spec`,
`fault_upsert_into_runtime`, `fault_patch_into_runtime`,
`scenario_v1_into_runtime`, `datagram_proxy_request_into_spec`,
`datagram_fault_patch_into_parts`, limit adapters) and maps views
through the protocol DTOs. Datagram fault CRUD replicates the admin
route flow (generation-guarded publish, cross-direction uniqueness)
over public `ControlState` methods — the only logic duplication in the
facade, pinned by the HTTP conformance test. No second
listener/runtime/state authority exists. Unsafe remains denied.

## WP3 — Rust conformance against the native protocol

`crates/eggchaos-embed/tests/facade.rs` (9 tests):
`facade_and_http_admin_agree_on_protocol_views` mounts `NativeAdmin`
on the facade's own `ControlState` and asserts byte-identical proxy and
fault views between the facade and the HTTP path. All other tests cover
start/health/version/shutdown, double close, drop-without-close,
repeated start/shutdown with independent state, 8-thread concurrent
use, stream/datagram CRUD round trips, conflict/missing/validation
mapping, Scenario V1/V2 validate/compile/apply/get/cancel, and empty
connection/association/kill paths.

## WP4 — PyO3 binding crate/package

`bindings/python-native/` is a standalone maturin crate (outside the
workspace: plain cargo cannot link a macOS extension-module cdylib;
`Cargo.lock` committed). `Service`/`Fault` classes plus 7 typed
`Native*Error` subclasses of `NativeError`. Methods are coarse and
bounded, take/return plain dicts/lists/strings/numbers, release the GIL
around every blocking facade call (`py.detach`), and convert
`EmbedError` categories one-to-one. Context-manager and explicit
`close()` lifecycle; use-after-close raises `NativeLifecycleError`.
`python/eggchaos_native/__init__.py` documents remote-vs-native
selection. Pinned: `pyo3 0.29.2` (abi3-py311, extension-module),
`maturin 1.9.5`. abi3 was usable with no API loss — no fallback reason
to record.

## WP5 — Async ergonomics

No PyO3 async integration: Python-level `asyncio.to_thread` offload is
documented in the README (mirroring the M033 `AsyncClient` rationale).
No Rust future crosses the boundary; no Python event-loop coupling.

## WP6 — Lifecycle and failure qualification

`tests/test_native.py` (10 tests): import smoke, context-manager close
on normal return and on exception, explicit + double close, repeated
independent instances, stream/datagram CRUD, connections/history/reset/
metrics-text/version, V2 validate/compile/apply/get/cancel plus V1
apply/get, all 7 error categories incl. conflict and bad-direction
validation, 4-thread parallel independent instances, and interpreter
shutdown without close exiting 0 in a subprocess.

## WP7 — Wheel/package matrix

`scripts/build_python_native_artifacts.sh` builds abi3
(`cp311-abi3`) wheels for both locally supported macOS targets plus an
sdist, and import-smokes the interpreter-arch wheel:

- `macosx_10_12_x86_64`: built AND import/runtime-smoked (this
  machine's interpreter).
- `macosx_11_0_arm64`: built; import smoke not runnable here (no
  arm64 interpreter on this host) — claimed as built-only.
- Linux x86_64/aarch64, Windows x86_64: not built here — recorded
  incomplete, not claimed. No publication to PyPI (owner decision).

## WP8 — Remote/native conformance

`tests/test_conformance.py` runs the same representative scenario
(stream proxy+fault, V2 validate/compile with fingerprint, proxy fault
views) through the M033 remote client (loopback daemon) and the native
module: proxy seeds, fault views, schedule fingerprint
(`21/22` test schedule), compiled action, and downstream fault lists
are timing-independently equal. Passes in
`scripts/qualify_python_native.sh`.

## WP9 — Performance and safety review

Measured on the candidate (darwin x86_64, fastest runs,
`scripts/qualify_python_native.sh`):

- native startup+shutdown: mean 0.7 ms;
- native proxy create+delete: mean 0.18 ms;
- native fault add+delete: mean 0.05 ms vs remote 0.45 ms (~9x control
  overhead reduction — the expected HTTP-hop saving, not a data-plane
  claim);
- native V2 validate: mean 0.06 ms.

No numerical budget is frozen (single-host measurement). Python never
enters the byte/datagram hot path by construction (control-only
facade); the existing datagram benchmark on the candidate passes both
budgets (`datagram_budget: pass`, `matched_budget: pass`), so no
data-plane regression exists. Unsafe audit (`check_python_native.sh`):
no `unsafe {`/fn/impl/trait/extern in binding sources; the
crate-local `allow(unsafe_code)` covers PyO3 0.29 macro-generated FFI
glue only. `cargo audit` clean on both lockfiles; workspace `cargo
deny` clean with one documented allowance (`Apache-2.0 WITH
LLVM-exception` for `target-lexicon`, transitive via
pyo3-build-config — permissive, no copyleft effect).

## WP10 — Documentation and closure

`bindings/python-native/README.md` (usage, lifecycle rules,
remote-vs-native selection, safety boundary),
`docs/control-plane.md` (native embedding paragraph),
`architecture/tooling-distribution.md` (3 new scripts, 7-crate package
list, publish order with `embed`), `AGENTS.md` (layout, embed bullet,
publish order, native pilot note). No C ABI created.

## Evidence (candidate `991818b`, darwin x86_64, rustc 1.89.0,
## Python 3.14.2, Node v26.8.2, PyO3 0.29.2, maturin 1.9.5)

- `./scripts/check.sh`: pass (fmt, clippy `-D warnings`, workspace
  tests, doc).
- `./scripts/check_openapi.sh`: `{"openapi":"pass","paths":21,
  "operations":36}`.
- `./scripts/release-smoke.sh`: pass incl. artifact smoke and order
  proof `core->experiment/eggfetch->protocol->server/toxiproxy/cli->embed`.
- Pinned Toxiproxy oracle: translation+differential pass
  (toxiproxy-server 2.12.0, checksum verified).
- `./scripts/qualify_eggfetch.sh`: pass (9/9 suites).
- `./scripts/check_python_client.sh` / `check_typescript_client.sh` /
  `qualify_language_clients.sh`: pass (M033 regressions green).
- `./scripts/check_python_native.sh`: `{"python_native":"pass"}`
  (facade tests, binding-crate tests, unsafe audit, binding audit,
  abi3 wheel, 10 Python tests).
- `./scripts/qualify_python_native.sh`:
  `{"python_native_qualify":"pass"}` (11 tests incl. conformance +
  measurements above).
- `./scripts/build_python_native_artifacts.sh`:
  `{"python_native_artifacts":"pass"}` (2 wheels + sdist + smoke).
- `./scripts/benchmark_datagram.sh`: `datagram_budget: pass`,
  `matched_budget: pass`.
- Existing stream/datagram/Scenario V2 golden traces and
  fingerprints: unchanged and green.

## C ABI go/no-go decision (required)

NO-GO for a generic C ABI at this time. Evidence: exactly one concrete
native consumer exists (Python via PyO3, which needs no C ABI), and no
second embedding requirement (Node/JNI/.NET/Go) has been demonstrated.
Per ADR 006 Decision 6, a future C ABI requires a separate ADR plus a
second concrete consumer. Remote Go/Java/.NET clients should reuse the
M032 OpenAPI contract. Node native, JNI, P/Invoke, cgo, UniFFI, and
WASM remain demand-driven and unactivated.

## Limitations

- Wheel import smoke covers macOS x86_64 only; arm64 built-only;
  Linux/Windows unbuilt (incomplete, not claimed).
- `AsyncClient`-style native async is offload-only (documented).
- Fuzz targets were not re-soaked for M034 (no parser/state-machine
  semantics changed; native control JSON paths are covered by contract
  + e2e suites).
- `eggchaos-native` is not in the crates.io publish order (standalone
  pilot, unpublished); PyPI publication is an owner decision.
