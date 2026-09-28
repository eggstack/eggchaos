# M051 — Native Control Operation Authority Consolidation Corrective Closure

Status: closed

Exact M051 implementation/evidence candidate:
`68cc957d2a21ad7638a20a7efa9251ca0ddcdbb8`

Activation baseline (per plan): `b2891a3` (M050 closure).
M051 is the only commit on top of the M050 closure candidate.

Closure date: 2026-09-28

Depends on: M050 closed on exact candidate `b2891a3`.

## Outcome

M051 collapses the duplicated conversion/state-call sequences that
`crates/eggchaos-server/src/admin.rs` and `crates/eggchaos-embed/src/lib.rs`
were each maintaining independently. The new
`crates/eggchaos-server/src/operations.rs` is a typed native
application operation authority that operates on already-parsed
protocol DTOs and returns typed `Result<T, ControlError>` values; both
`admin.rs` and `eggchaos-embed` now delegate to it.

The authority is not a second mutable state store: every method
delegates to the single `ControlState` / `DatagramRuntime` authority.
It contains no HTTP types, no PyO3 types, no body or status selection,
and no metrics text handling. The 36 existing `NATIVE_OPERATIONS` are
all unchanged in HTTP method, path, status, and JSON body shape.

Existing public helpers (`proxy_request_into_spec`,
`fault_upsert_into_runtime`, `datagram_proxy_request_into_spec`, etc.)
remain available as thin wrappers around the same conversion
authority; they are not removed because they are part of the
backwards-compatible `eggchaos_server` public surface.

## What changed on `68cc957`

`git show --stat 68cc957`:

```text
 crates/eggchaos-embed/src/lib.rs               | 292 ++++++------------
 crates/eggchaos-server/src/admin.rs            | 238 +++++-----------
 crates/eggchaos-server/src/lib.rs              |  11 ++
 crates/eggchaos-server/src/operations.rs       | 446 ++++++++++++++++++++++
 crates/eggchaos-server/src/runtime/tests.rs    |  18 +-
```

### New module: `operations.rs`

A typed native application operation authority that exposes one
method per shared operation. Each method takes the parsed protocol
DTO and the `ControlState`, runs the existing conversion
(`proxy_request_into_spec`, `fault_upsert_into_runtime`, etc.) once,
calls the appropriate `ControlState` method, and returns a typed
result plus the canonical `ControlError` category.

Methods exposed:

- `apply_proxy_request`, `apply_proxy_patch`
- `apply_datagram_proxy_request`, `apply_datagram_proxy_patch`
- `apply_stream_fault_upsert`, `apply_stream_fault_patch`
- `apply_datagram_fault_upsert`, `apply_datagram_fault_patch`
- `apply_scenario_v1`, `apply_scenario_v2_dto`
- `validate_scenario_v2_dto`, `compile_scenario_v2_dto`
- `get_scenario_run`, `cancel_scenario_run` (V1/V2-dispatched)
- `reset_service`
- `kill_connection`, `kill_datagram_association`, `get_connection`
- `apply_runtime_config` (typed `RuntimeConfigV1` → `RuntimeLimitApply`)

The V1/V2 scenario dispatch (`get_scenario_run` / `cancel_scenario_run`)
goes through the same `ControlState` methods, but the family-by-family
lookup is now a single typed method the transport calls once.

### Typed outcome structs

The authority returns typed outcome structs:

- `ProxyApplyOutcome` (`{ proxy, generation }`)
- `StreamFaultApplyOutcome` (`{ direction, fault, generation }`)
- `DatagramFaultApplyOutcome` (`{ direction, fault, generation }`)
- `DatagramProxyApplyOutcome` (`{ proxy, generation }`)
- `ScenarioRunLookup` (`V1 | V2`)
- `ScenarioApplyOutcome` (`V1 | V2`)
- `RuntimeLimitApply` (`{ admission, datagram, relay_buffer, termination_grace }`)

These typed outcomes are `Serialize` so HTTP and embed can re-emit the
same JSON shape the legacy hand-rolled code produced; both layers
serialize them in the same envelope structure as before, so wire
compatibility is preserved.

### `admin.rs` refactor

`admin.rs::route` now uses `crate::operations` for every shared
operation: proxy create/patch, datagram proxy create/patch, stream
fault upsert/patch, datagram fault upsert/patch, scenario apply
(V1/V2-dispatched), scenario validate/compile, scenario get/cancel
(V1/V2-dispatched), reset, connection get/kill, datagram association
kill. The route handler now parses JSON, calls the facade, and emits
the response or the `ControlError` envelope. The route's path matching,
status codes, body envelopes, headers, and request parsing are all
unchanged.

### `eggchaos-embed` refactor

`eggchaos-embed/src/lib.rs::EmbeddedService` retains every public
method signature and the `EmbedError` category set. Every state-
mutating method now delegates to the corresponding
`eggchaos_server::operations` method. The `start`/`shutdown`/
`control_state`/blocking/lifecycle contract is preserved verbatim;
the per-method body shrinks from a duplicate of the admin.rs sequence
to a single typed facade call.

### Tests

`operations.rs::tests` adds 3 unit tests covering:

- `runtime_config_apply_rejects_zero_proxies` — the typed
  `apply_runtime_config` returns the same rejection as
  `runtime_datagram_limits` alone.
- `stream_fault_patch_empty_is_rejected` — the typed
  `apply_stream_fault_patch` rejects empty patches the same way the
  legacy embed code did.
- `stream_fault_patch_requires_valid_fault_id_on_real_call` — the
  facade forwards the typed `ControlError::NotFound` from the
  underlying `ControlState` unchanged.

All pre-M051 embed tests continue to pass; the `EmbeddedService`
public API and behavior are unchanged.

## Invariant non-regression

- Frozen public symbol set: every `EmbeddedService` public method
  retains its name, signature, and behavior. `EmbedError` variants are
  unchanged. `ControlState` public methods are unchanged. The new
  `operations` module is additive (`pub use` re-exports it from
  `eggchaos_server`).
- Frozen route contract: all 36 native routes (21 paths) keep their
  pre-M051 method, path, status code, and JSON body shape. The route
  inventory test (`tests/native_route_inventory.rs`) passes without
  modification.
- Frozen CLI/SDK/Python-native behavior: the change is internal; no
  external surface moves.
- Frozen scenario semantics: V1 and V2 semantic authorities stay
  separate. The M050 scenario registry is now reached through the
  facade, but the registry itself is unchanged.
- Frozen compatibility surfaces: Toxiproxy behavior, OpenAPI shape,
  runtime crates, determinism, and benchmark workloads are unchanged.

## Required verification (results on `68cc957`)

| Command | Result |
| --- | --- |
| `cargo test -p eggchaos-server --all-features` | green (152 lib + 1 route inventory + 2 schedule corpus) |
| `cargo test -p eggchaos-embed --all-features` | green (10 embed tests) |
| `cargo test -p eggchaos-protocol --all-features` | green |
| `cargo test --workspace --all-features` | green |
| `./scripts/check_openapi.sh` | `{"openapi":"pass","paths":21,"operations":36}` |
| `./scripts/check.sh` (fmt/clippy/test/doc) | exit 0 |
| `sh scripts/tests/test_bench_provenance.sh` | `{"bench_provenance":"pass"}` |

## Successor activation

Closing M051 activates M052 (currently `blocked` in `plans/registry.md`).
M052 may begin from this exact M051 closure candidate `68cc957` as its
activation baseline. M052 will register its own exact implementation
candidate when it closes.
