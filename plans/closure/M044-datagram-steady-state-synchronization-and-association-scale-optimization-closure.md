# M044 — Datagram Steady-State Synchronization and Association-Scale Optimization Closure

Status: closed

Exact implementation candidate: `7a6dbb8c699e1017aa2a7e335c0fd75a224b8a84`
(with `fb2c8fc` M042 as the measurement baseline)

Closure date: 2026-09-26

Depends on: M042 closed at `fb2c8fc`; M043 closed at `67ce4ab`

## Outcome

M044 implemented the two M042-classified proven-material datagram
targets plus one low-cost-cleanup, with the remaining cleanups and the
reaper redesign deliberately skipped per the M042 target matrix:

1. **WP1 — Consume the M042 datagram target matrix**: Done. WP2
   (whole-spec clone) and WP3 (association-map async mutex) entered as
   **proven-material**; WP4 (record batching), WP5 (candidate
   reservation), WP6 (corruption copy) as **low-cost-cleanup**; WP7
   (deadline reaper) as **not-material**. Thresholds frozen in the M042
   closure govern every disposition below.

2. **WP2 — Remove whole-spec cloning from steady-state ingress
   (proven-material, retained as cleanup)**: Done in
   `receive_client_datagram()`. The per-datagram
   `state.spec.read()…clone()` is replaced by two short read guards:
   one copying the `Copy` max-datagram bound before association
   dispatch, one taking the atomic upstream-policy snapshot after the
   oversize early-return. Neither guard crosses the awaiting
   `resolve_association` future. New-association setup still clones a
   complete immutable spec because the worker needs a stable
   configuration snapshot and setup is not the steady-state path.

3. **WP3 — Active-association lookup synchronization (proven-material,
   recorded as no-op disposition)**: Done.
   `ProxyState.associations` is now `std::sync::RwLock<HashMap<…>>`
   (was `tokio::sync::Mutex`) and `policy_mutation` is now
   `std::sync::Mutex<()>` (was async). `resolve_association` takes an
   optimistic read-locked fast path for the common already-active
   case and re-checks under the write guard on miss, so the fast path
   cannot admit a second setup owner. Every other use was audited:
   reads use `read()`, inserts/removes/drains use `write()`, and no
   synchronous guard crosses an await or socket operation
   (`update_proxy` scopes its validation guard before
   `drain_associations().await`; the sync read guard is `!Send`).

   Artifact comparison
   `2026-09-26-macos-arm64-m042-datagram.json` →
   `2026-09-26-macos-arm64-m044-datagram.json` (2 000 datagrams ×
   3 rounds, medians): `scale_warm_1024` 16 769 → 16 840 (+0.4 %),
   `scale_warm_4096` 11 311 → 11 332 (+0.2 %). **No ≥10 % gain on
   either M042 scale threshold** — steady-state throughput at all
   cardinalities sits inside host noise (see Limitations). Same-host
   A/B runs confirm parity rather than regression (`scale_warm_1`
   pre-M044 ≈ 21 000 vs M044 ≈ 20 500; `scale_warm_4096` pre-M044 ≈
   11 800 vs M044 ≈ 11 900). The conversion is retained as a
   maintainability cleanup: it removes async-scheduler involvement
   from a pure in-memory lookup/insert/remove critical section and
   gives readers a shared lock. Recorded as a no-op disposition
   against both M042 throughput thresholds.

4. **WP5 — Bound duplicate candidate allocation (low-cost-cleanup)**:
   Done in `eggchaos-core/src/datagram.rs`. The next-stage vector
   for a `Duplicate` stage now reserves `candidates.len() *
   (additional_copies + 1)` with saturating arithmetic; non-duplicate
   stages keep the existing `len().max(1)` reservation. Candidate
   order, the 4 096 hard bound, and RNG behavior are unchanged.

5. **WP4 (record batching, M042: low-cost-cleanup)**: Not
   implemented. The worker loops already batch egress/evidence
   commits; the remaining acquisitions are listener-ingress recording
   and lifecycle transitions, and an atomics conversion would weaken
   multi-field snapshot coherence for unmeasured gain.

6. **WP6 (unique-`Bytes` corruption fast path, M042: inconclusive)**:
   Not implemented. Corruption copying runs only for active
   `PayloadCorrupt` stages; the `try_into_mut` branch plus the
   required unique/shared output-equivalence tests add branching to
   an unmeasured conditional path.

7. **WP7 (deadline-driven reaper, M042: not-material)**: Not
   implemented, as required. `hot_with_idle_<N>` p50 stays within
   +5 % of `scale_warm_<N>` p50 at every cardinality (4096: 70 µs vs
   69 µs), confirming the 10 ms scan is not material.

## Public-surface invariants preserved

- No public Rust/native/config/CLI/SDK change. Touched items are
  `pub(crate)` (`ProxyState` fields, `resolve_association`,
  engine internals).
- No new production dependency; no `unsafe`.
- ADR 003 fault kinds, composition order, per-client connected
  upstream sockets, association identity/capacity/setup-waiter
  semantics, M024 heap ordering, and queue bounds are unchanged.

## Deterministic and golden behaviour

- `cargo test -p eggchaos-core --all-features`: pass (datagram
  filter 13 + 1 pass; full suite green via `./scripts/check.sh`).
- `cargo test -p eggchaos-server --all-features`: 137 + 1 + 2 pass,
  including the setup/drain/capacity race suite against the new
  sync registry.
- `./scripts/check.sh` (fmt + clippy `-D warnings` + workspace
  tests + doc): pass on the exact candidate.
- `./scripts/check_openapi.sh`
  (`{"openapi":"pass","paths":21,"operations":36}`): pass.
- `EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh`
  (`{"fuzz":"pass","targets":9}`): pass.
- ADR 003 golden traces unchanged (core + server datagram suites
  pass without drift).

## Before/after measurements

`benchmarks/src/bin/datagram.rs` on the exact M044 candidate
(`7a6dbb8`), same harness and selectors as M042:

- M023/M024 budgets pass unchanged:
  `{"datagram_budget":"pass","matched_budget":"pass",
  "empty_plan_median_datagrams_s":17083.64,
  "empty_plan_throughput_ratio":0.5409,"p95_latency_ratio":1.6604,
  "matched_empty_over_bare_throughput":1.0222,
  "matched_empty_over_bare_p95":1.0115,
  "matched_windowed_empty_over_bare":0.9387}`.
- Scale medians (dps / p50): `scale_warm_1` 19 066 / 45 µs,
  `scale_warm_8` 20 725 / 43 µs, `scale_warm_256` 17 263 / 44 µs,
  `scale_warm_1024` 16 840 / 46 µs, `scale_warm_4096` 11 332 /
  69 µs.
- Latency tightness holds: `scale_warm_1` p50 45 µs ≤ 60 µs;
  `scale_warm_4096` p50 69 µs ≤ 100 µs.
- Hot-with-idle parity holds at every cardinality (worst +4.3 %
  at 1024: 48 µs vs 46 µs).
- Throughput thresholds recorded as no-op dispositions (see WP3):
  `scale_warm_1024` +0.4 %, `scale_warm_4096` +0.2 % vs the M042
  file baselines; same-host A/B shows parity.

Raw artifact:

- `qualification/performance/2026-09-26-macos-arm64-m044-datagram.json`

## No regression findings

- ADR 003 golden traces remain byte-for-byte (core + server
  datagram suites green).
- M023 direct-UDP floor and M024 topology-matched floor pass on
  the exact candidate.
- Per-datagram policy snapshots remain atomic (snapshot taken
  under the short spec guard, attached to each ingress item).

## Out-of-scope / explicit no-op dispositions

- WP3 throughput thresholds: no-op (parity retained as cleanup).
- WP4, WP6: no-op per above; implementation status: none.
- WP7: not-material; implementation status: none (per M042, must
  NOT be implemented).
- The pre-existing clippy `unusual_byte_groupings` warnings on
  `benchmarks/src/bin/datagram.rs:373/407` (`0x23_0b_1`) remain
  unchanged.

## Limitations

- Measured on one macOS arm64 host (Apple M4 Pro, rustc 1.89.0,
  2 000 datagrams per UDP case, 3 rounds). Host variance dominates
  single-digit deltas: ad-hoc `scale_warm_4096` runs on both the
  pre- and post-M044 candidates spanned ~10 000–14 000 dps with
  occasional catastrophic-drop runs (~1 500–3 000 dps) on either
  candidate, consistent with environmental noise rather than code
  behavior.
- File-baseline deltas (M042 artifact vs M044 artifact) conflate
  host drift across sessions; the same-host A/B parity runs are
  the controlling regression evidence.
- `scale_warm_4096` pre-warm sleeps up to 500 ms between warm-up
  opens (harness bookkeeping, unchanged from M042).

## Successor activation

M044 closure plus M043 closure jointly satisfy M045's gate. M045
(`045-performance-optimization-exact-head-qualification-and-reconciliation.md`)
is now `ready`: combined exact-head
performance/API/determinism/oracle/fuzz/security/package/hosted
qualification over the M042 thresholds with M008/M023/M024
budgets retained and no new optimization scope.

## Test commands retained for re-qualification

```sh
./scripts/check.sh
./scripts/benchmark_datagram.sh
cargo test -p eggchaos-core --all-features
cargo test -p eggchaos-server --all-features
EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh
```
