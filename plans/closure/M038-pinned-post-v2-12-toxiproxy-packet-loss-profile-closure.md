# M038 — Pinned Post-v2.12 Toxiproxy packet_loss Profile Closure

Status: closed  
Depends on: M037 (closed)  
Role: compatibility adapter + pinned upstream oracle  
Candidate: see git rev below; M039 records the final combined authority.

## Scope recap

Add an explicit opt-in Toxiproxy compatibility profile for the
researched post-v2.12 upstream snapshot, translating upstream
`packet_loss` into the M036/M037 native `StreamLoss` primitive while
preserving the existing strict v2.12 profile unchanged. Create a
reproducible source-built oracle and differential corpus for the
extension instead of tracking moving Toxiproxy `main`.

## Implementation summary

### Compatibility profile (`crates/eggchaos-toxiproxy/src/lib.rs`)

- New `enum CompatProfile { StrictV2_12, PostV2_12_2026_09_25 }` with
  stable kebab-case wire spellings, `Default = StrictV2_12`,
  `as_str()`, and `accepts_packet_loss()` predicate. The strict
  profile remains the default and continues to expose the frozen
  v2.12 toxic surface; the snapshot profile is opt-in.
- `ToxiproxyAdapter::with_profile(state, profile)` is the new
  constructor; `ToxiproxyAdapter::new(state)` keeps the strict default.
  `version()` is now profile-aware: `"2.12.0"` for strict, `"git"` for
  the snapshot (the source-build oracle identity observed at
  `40f7fd31bee529d824116bd2a11a9e3425e904ec`).
- `ToxiproxyHttp::start(bind, adapter)` threads the adapter's profile
  into every read/write path so `GET /version`, every toxic CRUD, and
  every reverse-mapping path is profile-aware.

### Attribute translation (`crates/eggchaos-toxiproxy/src/lib.rs`)

- `ToxicAttributes` gains `loss_rate: Option<f64>` and
  `correlation: Option<f64>` with documented oracle defaults of
  `0.0`.
- `kind_from_attrs(kind, attrs, profile)` rejects `packet_loss` under
  strict v2.12 (`400 invalid toxic type`) and translates it to
  `FaultKind::StreamLoss(StreamLossConfig { .. })` under the snapshot
  profile. Finite out-of-range `loss_rate`/`correlation` are clamped
  into `[0, 1]` for native validation while the live source-build
  oracle accepts them verbatim — classified as a recorded divergence.
- `attrs_from_kind(kind, profile)` reverses native faults back into
  oracle attributes; native `StreamLoss` is rejected under strict
  v2.12 (`invalid toxic type`) so the proxy view never lies about the
  wire shape. Under the snapshot profile, native `StreamLoss` is
  rendered as `packet_loss` with `loss_rate` and `correlation`.
- `merge_attributes` adds a `packet_loss` arm that picks per-type
  attributes (including the new `f64` fields) and only allows
  same-type updates.
- `attributes_object` adds a `packet_loss` arm that echoes both
  attributes numerically (zero-default for omitted).

### Profile-aware reverse mapping in views

- `proxy_json(view, profile)`: when reverse-mapping a native
  `StreamLoss` under strict v2.12, the proxy view embeds an explicit
  `{"error": "..."}` object for that toxic instead of fabricating a
  v2.12 toxic spelling.
- `ToxiproxyAdapter::list_json`, `proxy_json`, `add_toxic`,
  `get_toxic`, `list_toxics`, and `update_toxic` now thread
  `profile` through to every `fault_to_toxic` call so profile
  boundaries are never bypassed.

### Standalone example (`crates/eggchaos-toxiproxy/examples/compat_server.rs`)

- The example now accepts an optional second positional argument that
  selects the compatibility profile (`strict-v2.12` or
  `post-v2.12-2026-09-25`) and reports the active profile on
  startup. Default remains strict.

### Pinned oracle scripts (`scripts/`)

- `scripts/fetch_toxiproxy_post_v2_12.sh`: fetches the upstream source
  archive at `40f7fd31bee529d824116bd2a11a9e3425e904ec`, verifies the
  committed SHA-256
  (`26351cc70792f1c3391bdd1d376974a79a756b349ec1abfa8b8f1524c042b13d`),
  builds `toxiproxy-server` from the `cmd/server` source tree with the
  recorded Go toolchain, and prints the executable path. The
  source-build reports `toxiproxy-server version git` as the version
  identity; any version mismatch exits 1.
- `scripts/qualify_toxiproxy_post_v2_12.sh`: mandatory-mode gate.
  Reaps any stale post-v2.12 oracle from a prior run, runs the new
  differential corpus, and asserts the `DIFFERENTIAL_SUMMARY`
  summary reports `failed: 0`. Without the oracle or with a version
  mismatch it reports `differential:incomplete` (exit 0) unless
  `EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1` is set, in which case it
  exits 1.

### Differential corpus (`crates/eggchaos-toxiproxy/tests/post_v212_differential.rs`)

- Spins the pinned source-build oracle (via
  `TOXIPROXY_POST_V2_12_SERVER`) and the eggchaos
  `PostV2_12_2026_09_25` profile in parallel and runs the same API
  sequences against both, comparing status codes, exact edges, and
  recorded intent-compatible stochastic behavior. The corpus is
  gated by the script above; without the env var it reports
  `incomplete` and passes (developer mode).
- Coverage (14 exact/structural cases plus intent-compatible
  observations):
  - `version` (oracle `git`, ours `git`; structural agreement)
  - proxy create (listen port normalized to `0`)
  - `packet_loss` create with omitted attributes (zero-fill)
  - `packet_loss` create with typical values
  - `packet_loss` create at edges 0/1
  - `packet_loss` create with out-of-range finite values (recorded
    divergence; oracle verbatim, eggchaos clamps to `[0, 1]`)
  - `packet_loss` create with mixed int/float JSON (Go float64)
  - `packet_loss` create with wrong JSON type (both reject 400)
  - `packet_loss` create with empty name (auto-fills
    `packet_loss_downstream`)
  - `packet_loss` get round-trip
  - `packet_loss` update (correlation-only; existing loss_rate
    persists)
  - `packet_loss` list
  - data-plane edge: `loss_rate=0` preserves bytes (both servers)
  - data-plane edge: `loss_rate=1` discards (both servers)

### Documentation

- `docs/toxiproxy.md` documents the two profiles, the
  `post-v2.12-2026-09-25` spelling, the `{"version":"git"}` oracle
  identity, the recorded divergences, and the qualification flow.
- `architecture/toxiproxy-compat.md` is unchanged; the post-v2.12
  reference matrix lives in `plans/closure/M038` so it cannot be
  confused with the frozen `plans/reference/toxiproxy-parity.md`.

## Recorded divergences (live oracle observations)

- `/version` reports `git`, not a release tag — classified as
  source-build identity divergence.
- `loss_rate`/`correlation` are stored verbatim even when outside
  `[0, 1]`. Native values are clamped into the representable range;
  echo shape matches after `normalize_packet_loss` in the differential.
- Mixed int/float JSON forms (e.g. `loss_rate: 1`, `correlation: 0`)
  are accepted; we canonicalize via `normalize_numbers`.
- Wrong JSON types (e.g. `loss_rate: "oops"`) return a 400 with the
  exact Go unmarshal error message.
- Live-update state continuity: upstream `packet_loss` is a
  `StatefulToxic`; Eggchaos's generation barrier recompiles the
  engine and resets loss state per generation. Documented in
  `docs/toxiproxy.md` and exercised by the existing M019/M023
  generation transition suite.

## Tests

Mandatory M038 tests passing on the exact candidate:

- `cargo test -p eggchaos-toxiproxy --all-features` (13 unit tests:
  strict-v2.12 rejection, post-v2.12 acceptance + clamping, strict
  reverse-mapping error envelope, all existing v2.12 tests).
- `cargo test -p eggchaos-toxiproxy --all-features --test differential`
  (strict 50/50 vs pinned v2.12.0 oracle remains green).
- `cargo test -p eggchaos-toxiproxy --all-features --test
  post_v212_differential` (14/14 vs the pinned source-build
  `40f7fd31` oracle).
- `cargo test --workspace --all-features` (whole workspace, all green).
- `cargo doc --workspace --all-features --no-deps`.
- `scripts/check_openapi.sh`.
- `scripts/check_python_client.sh`.
- `scripts/check_typescript_client.sh`.
- `scripts/qualify_language_clients.sh`.
- `scripts/check_python_native.sh`.
- `scripts/qualify_toxiproxy_v2_12.sh` (strict v2.12 still passes its
  50/50 pinned v2.12.0 differential).
- `scripts/qualify_toxiproxy_post_v2_12.sh` (post-v2.12 14/14 passes
  against the pinned `40f7fd31` source-build oracle).

## Acceptance checklist (per M038)

- [x] Strict v2.12 remains the default and passes its existing
  pinned oracle gate.
- [x] The snapshot profile is explicit and tied to
  `40f7fd31bee529d824116bd2a11a9e3425e904ec` (committed archive
  SHA-256 + recorded Go toolchain).
- [x] `packet_loss` translates only to native `StreamLoss`.
- [x] API/default/update behavior has live-oracle evidence (14/14
  differential against the source-build oracle).
- [x] Exact edge cases and intent-compatible stochastic cases have
  committed comparators (`post_v212_differential.rs`).
- [x] Live-update state divergence is documented/tested
  (`docs/toxiproxy.md` + existing M019/M023 generation transition
  suite).
- [x] No moving-main fetch exists; the script explicitly pins a
  single upstream commit.
- [x] No fictional released version is claimed; `/version` reports
  either the strict profile's frozen `"2.12.0"` or the snapshot
  profile's source-build `"git"`.
- [x] Docs/reference matrices distinguish strict v2.12 from snapshot
  support (`docs/toxiproxy.md`, this closure, and the unchanged
  `plans/reference/toxiproxy-parity.md` for v2.12 strict only).
- [x] All M038 tests pass on one exact candidate (see Tests).
- [x] Closure evidence records oracle build identity, toolchain,
  corpus, and limitations (this document).

## Stop/rejection review

- The implementation does not change strict v2.12 toxic acceptance
  (`strict_v212_rejects_post_v212_packet_loss_toxic` and the strict
  differential corpus both confirm).
- The oracle source is fetched from the exact pinned commit only;
  no `main`, no `HEAD`, no tags.
- Intermediate random sequences are not asserted byte-for-byte
  against upstream; the corpus asserts `loss_rate=0` and
  `loss_rate=1` exact behavior plus intent-compatible stochastic
  observations.
- The adapter owns no RNG, no data plane, no separate state store;
  every read derives from `ControlState` snapshots and every mutation
  goes through the native control authority.
- The post-v2.12 profile is never labeled as real TCP/IP packet loss;
  the `/version` oracle identity is `git`; the docs say it is
  pinned to a source-build snapshot of a post-v2.12 fork.
- Statistical tolerances are predeclared (`normalize_packet_loss`,
  `normalize_numbers`, `normalize_listen`); they were not tuned
  after observing candidate output.

## Follow-on activation

A clean M038 closure makes **M039 ready**. M039 is the tranche-level
exact-candidate qualification gate and must rerun both the strict
v2.12 and pinned post-v2.12 snapshot oracles together with
native/SDK/binding regressions on one exact commit.

## Additive M040 corrective reference (2026-09-26)

This historical M038 implementation record is preserved. Its implementation
commit is `8bfe0332ebc19a032443951818c2988319217e7d`; final ADR 007 corrective
qualification and repository authority are recorded in
`plans/closure/M040-post-v2-12-stream-loss-corrective-requalification-closure.md`.
