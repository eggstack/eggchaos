# M011 closure — Live State, Scenario, and Observability Corrective

Milestone: M011 (`011-live-state-scenario-observability-corrective.md`)
Candidate commit: `acd0883`
Implementation commits: code commit + this closure record
Commands executed: see Verification
Platforms: macOS arm64 (developer host)
External oracle/version: none required for M011 (no Toxiproxy oracle surface)
Evidence artifacts: `cargo test --workspace --all-features` (86 tests total),
`cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets --all-features -- -D warnings`
Known limitations: listed below
Acceptance criteria verdict: pass
Registry transition: M011 `active` -> `closed`
Next milestone activated: M012 (`ready`; M009, M010, M011 closed)

## Work package disposition (WP1–WP8)

1. WP1 — Atomic policy snapshot: `LivePolicy` publishes one immutable
   `PublishedPolicy { generation, plan, seed_namespace }` per generation
   via a single `ArcSwap` snapshot load/store, plus `publish_expected`
   CAS. Canonical proxy state reads from the same snapshots: views report
   each plan with its generation/namespace from one load; fault CRUD,
   `publish_plans`, reset, and init paths derive bases from snapshots and
   refresh config mirrors from the published snapshot under the same
   write lock. Policies initialize from configured plans at
   create/import/construct. Manual updates retain namespaces.
2. WP2 — Connection evidence: `ChaosStream` owns `Arc<StreamEvidence>`
   (observed/pending generations, seed namespace, transitions, byte
   counters, per-type activations, bounded active fault identities,
   high-water). `ConnectionSnapshot` carries accept-time generations plus
   live fields merged at read/close. No payloads. Streams compile from
   the accepted snapshot.
3. WP3 — History/metrics: `record_close` and purge merge final evidence,
   aggregate reconciling metrics (8 outcome classes, graceful/hard-reset
   requests, reset applied/unsupported/failed, byte flows, transitions,
   per-proxy tables, per-type activations; bounded with overflow), then
   bounded history (0 disables). `/metrics` adds outcome, request, reset,
   byte, transition, per-proxy, activation, active/policy/queue gauges
   with a fixed label vocabulary (proxy, direction, flow, outcome,
   request, result, fault_type).
4. WP4 — Owned scenarios: `POST /v1/scenarios/apply` validates upfront,
   returns `{run_id, seed, status}`; `GET` reports
   status/applied/failure/trail; `DELETE` cancels. Tasks in a
   service-owned `JoinSet` on shutdown-child tokens; shutdown joins them.
   Max 32 run records. Events apply fail-fast to live bases via
   directional expected-generation publish; concurrent publications fail
   the run instead of silent rollback.
5. WP5 — Seed-effective scenarios: `derive_policy_seed(seed, run_id,
   event_index)` (golden-tested) namespaces each event publication;
   engines compile RNGs from the namespace, so seeds change decisions
   and identical inputs replay regardless of scheduling.
6. WP6 — Tests: 14 new server + 7 new core tests covering all 14
   required behaviors (generation agreement, stale conflict, B-derived
   removal, monotonic concurrency, pending drain, accepted/current
   tracking, seed sensitivity/replay, bounded cancel, shutdown cancel,
   observable failure, history 0/N, exact metrics, label vocabulary,
   payload-free evidence). Server suite passes repeated runs.
7. WP7 — Docs: `docs/control-plane.md` (generations, snapshots, scenario
   API, replay limits, metrics contract) and `docs/configuration.md`
   (namespace retention/derivation).
8. WP8 — Closure: this record.

## Core refinements found by M011 tests

- Empty-engine direct-path bytes bypassed all evidence (no-fault legs
  reconciled as zero). Transparent bytes now count via `note_direct`.
- `new_live` (both stream types) loaded plan+generation in two steps
  and ignored namespaces; both compile from one atomic snapshot now.
- `EngineEvidence` gains per-fault-type activation counters with a
  documented engagement definition, surfaced via summaries, handles,
  and metrics.

## Verification (exact candidate tree)

```text
cargo fmt --all -- --check                                             PASS
cargo clippy --workspace --all-targets --all-features -- -D warnings   PASS
cargo test -p eggchaos-core --all-features                             PASS (42 tests)
cargo test -p eggchaos-server --all-features                           PASS (37 tests, repeated runs)
cargo test -p eggchaos-cli --all-features                              PASS (1 end-to-end test)
cargo test -p eggchaos-eggfetch --all-features                         PASS (3 tests)
cargo test -p eggchaos-toxiproxy --all-features                        PASS (3 tests)
cargo test --workspace --all-features                                  PASS (86 tests total)
cargo check --manifest-path fuzz/Cargo.toml                            PASS
```

## Limitations (explicitly not M011 work)

- Only macOS arm64 execution evidence; Linux/Windows CI via CI/M013.
- No wall-clock throughput/latency regression vs bare `eggress-relay`;
  M013 owns the performance gate.
- Scenario runs are API-only (no CLI subcommands).
- Metric tables cap at 1024 proxies / 8192 activation series with
  `_overflow` buckets (totals stay exact).
- Connection keys derive from accept ordinals, so live timing/order is
  not replayable — only policy state and per-key decisions are.
