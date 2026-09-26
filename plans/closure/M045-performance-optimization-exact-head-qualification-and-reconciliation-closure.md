# M045 — Performance Optimization Exact-Head Qualification and Reconciliation Closure

Status: closed

Exact candidates:

- `e8ff753d08acfc2eb6dd1a452bb1a887d4fa41d4` — combined production
  head (M043 `67ce4ab` + M044 `7a6dbb8` plus their closure docs).
  All local performance/determinism/oracle evidence below was
  gathered on this SHA.
- `a27a67a0577e3a92a2189d5188a63bf8a0a94dc0` — evidence candidate
  (M045 artifacts + WP7 doc reconciliation; `git diff
  e8ff753..a27a67a` is docs and `qualification/performance` JSON
  only, zero production delta). Hosted CI run `36255454409` is
  green 13/13 on this SHA.

Closure date: 2026-09-26

Depends on: M042 (closed), M043 (closed at `67ce4ab`), M044 (closed
at `7a6dbb8`)

## Outcome

M045 adds no optimization of its own. It qualifies the combined
M043/M044 tree on one exact head, confirms every M042 frozen
threshold is either met or covered by an M043/M044-accepted no-op
disposition, and reconciles planning state. The post-M041
performance tranche is now closed end to end:

`M042 (closed) -> M043 (closed) -> M044 (closed) -> M045 (closed)`

No automatic successor is activated. Further performance work
requires a new measured finding and numbered plan.

## WP1 — Exact head reconciliation

- M043 (`67ce4ab`) and M044 (`7a6dbb8`) closure candidates are both
  reachable from `main`.
- `git diff 7a6dbb8..e8ff753 --stat` is M044 closure docs + registry
  only; no unregistered production change exists after either
  optimization closure.
- `git diff e8ff753..a27a67a --stat` is M045 artifacts + WP7 docs
  only; the hosted matrix therefore evaluated identical production
  code to the local evidence.

## WP2 — Corrected performance authority on the exact candidate

Raw artifacts (all `2026-09-26-macos-arm64-m045-*` under
`qualification/performance/`):

- `m045-stream.json` (17 cases, 1 MiB, 5 rounds) + redeeming
  controlled-rerun history (see Limitations);
- `m045-stream-probes.json` (microprobes);
- `m045-datagram.json` (full triad + scale matrix; second run
  retained, first superseded as load-limited — see Limitations).

Stream same-profile table (M043 ran the same 1 MiB harness; M042
ran 8 MiB and is not the like-for-like comparator):

| case | M043 | M045 | delta |
| --- | --- | --- | --- |
| `bare_eggress_relay` | 4 635 | 5 319 | +14.8 % |
| `eggchaos_static_empty_plan` | 4 679 | 5 417 | +15.8 % |
| `eggchaos_live_empty_plan` | 6 382 | 5 300 | −17.0 % |
| `eggchaos_static_vectored_writes_preserving_plan` | 4 471 | 4 095 | −8.4 % |
| `eggchaos_static_stream_loss_full` | 5 527 | 4 458 | −19.3 % |
| `eggchaos_static_latency_1ms` | 28 | 28 | +1.0 % |

Byte-conservation invariants hold on all 16 byte-bearing cases
(0 violations; the `eggfetch_adapter` case carries no byte fields
by design). Microprobes reproduce M042 floors:
`live_policy_snapshot_load_full` 4.5 ns (≤10 ns),
`engine_build_stages_0` 100.6 ns, `engine_build_stages_4_latency`
304.7 ns. The M043 WP7 no-op disposition stands (path intact,
no ≥10 % claim).

Datagram scale table (M042 file baseline → M045 retained run,
medians):

| case | M042 dps/p50 | M045 dps/p50 | file delta |
| --- | --- | --- | --- |
| `scale_warm_1` | 22 460/42 µs | 20 000/44 µs | −11.0 % |
| `scale_warm_8` | 22 007/42 µs | 19 558/44 µs | −11.1 % |
| `scale_warm_256` | 17 337/44 µs | 16 890/44 µs | −2.6 % |
| `scale_warm_1024` | 16 769/47 µs | 15 134/48 µs | −9.8 % |
| `scale_warm_4096` | 11 311/73 µs | 9 166/84 µs | −19.0 % |
| `hot_with_idle_4096` | 11 505/67 µs | 9 718/78 µs | −15.5 % |

Historical budgets pass unchanged on the exact candidate:
`{"datagram_budget":"pass","matched_budget":"pass",
"empty_plan_median_datagrams_s":19728,
"empty_plan_throughput_ratio":0.6065,"p95_latency_ratio":1.3333,
"matched_empty_over_bare_throughput":1.175,
"matched_empty_over_bare_p95":0.8067,
"matched_windowed_empty_over_bare":0.9964}` (empty-plan triad
beats the M042 file value). M042 latency tightness holds
(`scale_warm_1` p50 44 µs ≤ 60; `scale_warm_4096` p50 84 µs ≤
100). Hot-with-idle p50 parity holds at every cardinality
(worst +2 % at 1024; hot faster than scale at 4096).

M042 thresholds were NOT retuned: the ≥10 % scale-gain targets
remain recorded as M044 no-op dispositions, and the controlling
no-regression evidence is the same-host A/B parity (identical
code measured ±12 % minutes apart; pre/post-M044 A/B within
±5 %), not the cross-session file deltas.

## WP3 — Determinism, fault, lifecycle regression

- `cargo test -p eggchaos-core --all-features` + full workspace
  suite via `./scripts/check.sh`: pass on the exact candidate.
- Stream RNG golden vectors, stream-loss
  fragmentation/correlation/goldens: pass (core suite green).
- ADR 003 datagram golden traces: pass (core + server datagram
  suites green, no drift).
- Association setup/waiter/capacity/idle/kill/drain race suite
  (137 server tests): pass against the M044 sync registry.
- Live-policy generation/transition/drain/termination and
  Scenario V1/V2 publication/ownership tests: pass.

## WP4 — Public/cross-surface capability regression

- `./scripts/check.sh` (fmt + clippy `-D warnings` + workspace
  `--all-targets --all-features` tests + doc): pass.
- `./scripts/check_openapi.sh`
  (`{"openapi":"pass","paths":21,"operations":36}`): pass.
- `./scripts/check_python_client.sh`
  (`{"python_client":"pass"}`),
  `./scripts/check_typescript_client.sh`
  (`{"typescript_client":"pass"}`): pass.
- `./scripts/check_python_native.sh`
  (`{"python_native":"pass"}`) and
  `./scripts/qualify_python_native.sh`
  (`{"python_native_qualify":"pass"}`): pass.
- `./scripts/qualify_eggfetch.sh`: pass (exit 0).
- Strict v2.12 mandatory differential
  (`EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1`, checksum-verified
  `toxiproxy-server 2.12.0`):
  `{"translation":"pass","differential":"pass"}`.
- Pinned post-v2.12 mandatory qualification
  (`EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1`, `40f7fd31`):
  `{"translation":"pass","differential":"pass"}`.
- No route, DTO, config key, CLI operation, binding, fault kind,
  or adapter capability disappeared; strict v2.12 remains
  default/frozen, post-v2.12 remains opt-in/pinned.

## WP5 — Security, fuzz, package, artifact regression

- `./scripts/release-smoke.sh`: pass, including
  `{"order_proof":"pass","order":"core->experiment/eggfetch->protocol->server/toxiproxy/cli->embed"}`
  and `{"artifact_smoke":"pass"}`.
- `EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh`:
  `{"fuzz":"pass","targets":9}` on the exact candidate.
- audit/deny run through the canonical smoke path: pass.
- `./scripts/release-artifact-smoke.sh`:
  `{"artifact_smoke":"pass"}`.
- No new `unsafe` (workspace `unsafe_code = "forbid"` intact;
  M042–M044 registered no ADR for any exception).

## WP6 — Hosted exact-head qualification

Pushed `a27a67a`; hosted CI run `36255454409` completed
**success 13/13** on that SHA:

- `check` on ubuntu/macOS/windows: success;
- `language-clients` matrix (macOS + ubuntu × py 3.11/3.12 ×
  node 20/22): success;
- `python-native` on ubuntu + macOS: success.

No hosted evidence is borrowed from an earlier SHA.

## WP7 — Documentation and planning reconciliation

- `plans/registry.md`: M045 active → closed; tranche order
  recorded as closed end to end.
- `plans/README.md`, `plans/roadmap.md`: status lines moved from
  M045-ready to M045-closed with the evidence candidate and hosted
  run ID.
- `architecture/` deep dives: no change required (none
  references the M043/M044-changed internals).
- `qualification/performance/README.md`: M043/M044 evidence
  section added with the M045 combined-head artifacts.
- `AGENTS.md` planning state: M042–M044 closed, M045 active
  (updated to closed with this commit).
- M042 baseline/threshold artifacts untouched; no historical
  closure rewritten.

## Limitations

- All local performance evidence is single-host (macOS arm64,
  Apple M4 Pro, rustc 1.89.0, uptime 21 days, load averages
  8–15 during the sessions). Host load is the dominant noise
  source: one M045 stream run was discarded as invalid when its
  no-code-change control (`bare_eggress_relay` 338 MiB/s vs the
  4 635 M043 value) proved load limitation, and the rerun was
  accepted only after the control recovered (5 319). The first
  M045 datagram run was likewise superseded (bare relay −26 %)
  by the retained run (bare −18 %, empty-plan triad above
  baseline).
- File-baseline deltas conflate host drift across sessions; the
  controlling regression evidence is same-host A/B parity plus
  in-run controls (bare relay, budget gates, latency budgets).
- `scale_warm_4096` pre-warm sleeps up to 500 ms between
  warm-up opens (unchanged M042 harness bookkeeping).
- No release/tag action was taken (explicit non-goal).

## Test commands retained for re-qualification

```sh
./scripts/check.sh
./scripts/benchmark.sh
./scripts/benchmark_datagram.sh
./scripts/check_openapi.sh
./scripts/check_python_client.sh
./scripts/check_typescript_client.sh
./scripts/qualify_eggfetch.sh
EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh
TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" \
  EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 \
  ./scripts/qualify_toxiproxy_v2_12.sh
TOXIPROXY_POST_V2_12_SERVER="$(./scripts/fetch_toxiproxy_post_v2_12.sh)" \
  EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1 \
  ./scripts/qualify_toxiproxy_post_v2_12.sh
./scripts/release-smoke.sh
./scripts/release-artifact-smoke.sh
```
