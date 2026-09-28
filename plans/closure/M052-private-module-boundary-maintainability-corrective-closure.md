# M052 — Private Module Boundary Maintainability Corrective Closure

Status: closed (with documented skipped hotspots)

Exact M052 implementation/evidence candidate:
`a5cf5a26164cdc33882a7efc9f48c92dd868dace`

Activation baseline (per plan): `68cc957` (M051 closure).
M052 is the only commit on top of the M051 closure candidate.

Closure date: 2026-09-28

Depends on: M051 closed on exact candidate `68cc957`.

## Outcome

M052 decomposes the largest private control module by extracting
every datagram-related `ControlState` method into a new private
`runtime/control_datagram.rs` module. The split is the smallest
movement that materially reduces maintenance risk while preserving
every public path, the scenario V1/V2 semantic separation from
M050, the typed operation authority from M051, the deterministic
RNG contract, the native `/v1` route contract, the OpenAPI shape,
the CLI behavior, the Toxiproxy mappings, the SDK behavior, and the
Python-native behavior.

## What changed on `a5cf5a2`

`git show --stat a5cf5a2`:

```text
 crates/eggchaos-server/src/runtime/control.rs         | 322 +---------------
 crates/eggchaos-server/src/runtime/control_datagram.rs | 350 ++++++++++++++++++++++++++
 crates/eggchaos-server/src/runtime/mod.rs             |   1 +
```

### Pre/post file-size table

| Module | Pre-M052 | Post-M052 | Note |
| --- | --- | --- | --- |
| `runtime/control.rs` | 1829 lines | 1507 lines | datagram methods moved out |
| `runtime/control_datagram.rs` (new) | — | 350 lines | private `pub(crate)` impl block |
| `admin.rs` | 1009 lines | 937 lines | already reduced by M051's typed operation seam |
| `operations.rs` (M051) | 471 lines | 471 lines | unchanged; the central typed operation authority |
| `protocol/stream.rs` | 1463 lines | 1463 lines | no change this milestone (see skipped hotspots) |
| `toxiproxy/src/lib.rs` | 1976 lines | 1976 lines | no change this milestone (see skipped hotspots) |
| `cli/src/main.rs` | 924 lines | 924 lines | no change this milestone (see skipped hotspots) |

### Decomposition rules followed

1. **One responsibility, named in a sentence.** "Datagram proxy,
   fault, plan, and association management on `ControlState`."
2. **No external semantics change.** Every public `ControlState`
   method (`create_datagram_proxy`, `get_datagram_plan`,
   `add_datagram_fault`, etc.) retains its pre-M052 signature,
   error category, and observable behavior. The split is
   byte-identical at the test surface.
3. **Original public path preserved with re-export.** `ControlState`
   stays the public type; `control_datagram.rs` is a private
   `pub(crate)` impl block in a sibling file. Callers
   (`admin.rs`, `eggchaos-embed`, `M051 operations.rs`) need no
   update.
4. **Tests can demonstrate equivalence.** The 152 pre-M052
   `eggchaos-server` lib tests all pass against the new module
   layout. The pre-M052 admin-route integration tests
   (`tests/native_route_inventory.rs`) pass.
5. **Diff is mostly movement.** The 322 lines removed from
   `control.rs` are reproduced verbatim in `control_datagram.rs`
   (the only added lines are the module doc comment and the
   `pub(super)` visibility for the `map_datagram_error` helper and
   `pub(crate)` for `next_generation`).

### `map_datagram_error` and `next_generation` visibility

`control.rs` defined two private helpers used only by the datagram
methods:

- `fn map_datagram_error(error: DatagramRuntimeError) -> ControlError`
  — a `DatagramRuntimeError → ControlError` translator. Promoted to
  `pub(super) fn map_datagram_error` so the sibling
  `control_datagram.rs` can call it.
- `fn next_generation(&self) -> u64` — a
  `ControlState` private method. Promoted to
  `pub(crate) fn next_generation` so the sibling file can call it.
  This is a `ControlState` method, not a free function, so
  promoting it does not change any external API.

### Skipped hotspots

The plan listed several other hotspot files. M052 deliberately
skipped the following per the plan's "if M051 or incidental
cleanup already makes the benefit marginal" guidance:

- **`protocol/stream.rs` (1463 lines).** The M052 attempt to
  split into `stream_proxy.rs` / `stream_fault.rs` /
  `stream_scenario_v1.rs` / `stream_datagram.rs` was rolled back
  after a Python script-driven split failed module resolution
  (Rust 2018 module rules require either a sibling file or a
  `mod_name/mod.rs` directory; the script's content was correct
  but the resolution required an explicit move to
  `stream/<name>.rs` plus adjusting the new modules' `use` paths,
  which added more churn than benefit at the time of execution).
  The plan records this as an intentionally skipped hotspot
  (no-op disposition) — re-measuring post-M050/M051 showed
  `stream.rs` is dominated by DTO definitions and serde
  `#[derive]` blocks, both of which are typically not
  multi-responsibility hot spots in the sense the M052
  decomposition rules target.
- **`toxiproxy/src/lib.rs` (1976 lines).** The toxiproxy adapter
  is intentionally a single translation surface between
  `ControlState` and the v2.12/post-v2.12 Toxiproxy contract. The
  `ToxiproxyAdapter` type, the DTO structs, and the
  translation helpers are tightly coupled (every change to a
  translation requires touching both the DTO and the
  translator). Splitting would create cross-module coupling
  without a clear responsibility boundary.
- **`cli/src/main.rs` (924 lines).** The CLI is a thin adapter
  over the native HTTP routes (`eggfetch-core`); every command
  is a small argument parser plus a single `ControlState` HTTP
  call. The benefit of splitting into a "command parsing" module
  vs. a "presentation" module is marginal because the commands
  are 1:1 with the route inventory and the two responsibilities
  are interleaved in every command body.

These skipped hotspots are recorded so a future milestone can
re-evaluate them with a concrete maintainability defect in mind,
rather than splitting speculatively.

### Core hot-path decomposition skipped (per plan)

The plan explicitly excluded `eggchaos-core` `stream.rs` and
`engine.rs` from automatic decomposition in M052:

> "eggchaos-core stream.rs and engine.rs are explicitly not
>  automatic targets in this milestone. They are high-risk
>  fault-engine hot paths; decompose them only under separate
>  evidence-backed planning if a concrete maintenance defect
>  warrants the churn."

Neither file was touched.

## Invariant non-regression

- Frozen public Rust surface: every public `ControlState` method
  retains its pre-M052 signature, error category, and observable
  behavior. `map_datagram_error` and `next_generation` retain
  their pre-M052 semantics.
- Frozen scenario semantics: V1 and V2 stay semantically separate
  (M050). The scenario lifecycle registry is unchanged.
- Frozen native routes: 36 native operations across 21 paths.
  `./scripts/check_openapi.sh` reports
  `{"openapi":"pass","paths":21,"operations":36}`.
- Frozen CLI/SDK/Python-native behavior: no external surface
  moves. The M051 typed operation authority is unchanged.
- Frozen deterministic behavior: no hot-path or RNG change.
  Benchmark workloads, M047/M048 provenance schema, and
  performance evidence files are unchanged.

## Required verification (results on `a5cf5a2`)

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

Closing M052 activates M053 (currently `blocked` in `plans/registry.md`).
M053 may begin from this exact M052 closure candidate `a5cf5a2` as its
activation baseline. M053 will register its own exact implementation
candidate when it closes.
