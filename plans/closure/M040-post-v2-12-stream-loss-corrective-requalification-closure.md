# M040 — Post-v2.12 Stream-Loss Corrective Requalification Closure

Status: closed

Exact implementation candidate: `48fe0dd8dfc1f995c53a3b1661fe704dbdc5bce0`

Closure date: 2026-09-26

Depends on: M039 historical closure record + ADR 007

## Outcome

M040 corrected the bounded profile propagation, data-plane qualification,
metrics, oracle-toolchain, and current-state documentation defects registered
after M039. Strict Toxiproxy v2.12 remains the default/frozen profile. The
post-v2.12 `packet_loss` behavior remains opt-in and pinned to Shopify commit
`40f7fd31bee529d824116bd2a11a9e3425e904ec`.

The public profile-aware toxic conversion and proxy translation paths now
honor the selected compatibility profile; strict defaults continue to reject
`packet_loss`. Snapshot `/populate` keep responses preserve the existing toxic.
The post-v2.12 corpus separates API checks from isolated data-plane fixtures,
asserts exact zero/full loss, and runs predeclared intermediate-loss and
conditional-correlation comparators. Its 16 counted results are 12 exact API,
2 exact data-plane, 1 stochastic, and 1 correlation pass; each count is backed
by its corresponding assertion or comparator.

Named stream-loss Prometheus counters now report evaluated logical chunks,
dropped chunks, and discarded bytes per bounded proxy/direction. They use the
existing proxy overflow strategy and do not change `FAULT_TYPE_NAMES` or the
seven legacy activation slots. Final connection evidence drives these counters
once, avoiding duplicate byte accounting across multiple faults.

## Local exact-candidate evidence

All results below were run on candidate
`48fe0dd8dfc1f995c53a3b1661fe704dbdc5bce0`:

- `./scripts/check.sh` passed: workspace format, all-target/all-feature
  clippy, all-feature workspace tests, and docs. The local test process used
  `ulimit -n 2048` after the default macOS file limit caused an environmental
  `Too many open files` failure; the full gate then passed.
- Focused OpenAPI, Python client, TypeScript client, language-client, native
  Python, EggFetch, release smoke, and release artifact smoke scripts passed.
- `EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh` passed all nine fuzz
  targets.
- Strict mandatory oracle qualification passed **50/50** against pinned
  Toxiproxy v2.12.0.
- Mandatory post-v2.12 qualification passed with exact counts:
  `exact_api_passed=12`, `exact_data_plane_passed=2`,
  `stochastic_loss_passed=1`, `correlation_passed=1`, `failed=0`.
  Zero loss preserved all 131,072 payload bytes; full loss forwarded zero
  bytes. In 256 intermediate probes, the oracle dropped 71 and Eggchaos
  dropped 69, both inside the frozen `0.10..=0.40` interval. For the
  512-probe correlation comparator, conditional drop gaps were 0.4720 for the
  oracle and 0.5136 for Eggchaos, both above the frozen 0.20 floor. Minimum
  predecessor buckets were met for both implementations.
- The oracle was built from commit
  `40f7fd31bee529d824116bd2a11a9e3425e904ec`, source archive SHA-256
  `26351cc70792f1c3391bdd1d376974a79a756b349ec1abfa8b8f1524c042b13d`,
  using requested/resolved `go1.23.0`; the recorded compiler was
  `go version go1.23.0 darwin/arm64`, with `GOTOOLCHAIN=go1.23.0`.
- Stream and datagram benchmark raw output is retained under
  `qualification/m040/`. The exact-candidate datagram benchmark passed all
  thresholds on its third attempt (Apple M4 Pro, macOS, aarch64, Rust 1.89.0):
  direct/empty throughput ratio `0.4883` (minimum `0.45`), p95 ratio `2.1304`
  (maximum `2.5`), matched throughput `0.8675` (minimum `0.7`), matched p95
  `1.1575` (maximum `1.6`), and matched windowed throughput `1.2297` (minimum
  `0.7`). Two earlier attempts failed the absolute ratios while unrelated
  cargo builds/tests were concurrently saturating the host; they are retained
  and labelled as contaminated attempts, not accepted qualification results.

Raw qualification artifacts:

- `qualification/m040/post-v2-12-exact-48fe0dd.log.gz`
- `qualification/m040/post-v2-12-oracle-record-48fe0dd.json`
- `qualification/m040/stream-benchmark-48fe0dd.json.gz`
- `qualification/m040/datagram-benchmark-48fe0dd-attempt-03.json`
- `qualification/m040/datagram-benchmark-load-contaminated-01.json`
- `qualification/m040/datagram-benchmark-load-contaminated-02.json`

The strict oracle's 50/50 summary and the other local scripts' successful
exit statuses are recorded above; their full console logs were not retained.

## Exact-head hosted qualification

GitHub Actions run [36214657871](https://github.com/eggstack/eggchaos/actions/runs/36214657871)
completed with conclusion `success` on the exact candidate SHA
`48fe0dd8dfc1f995c53a3b1661fe704dbdc5bce0`. Every job concluded `success`:

- Rust check matrix: `check (ubuntu-latest)`, `check (macos-latest)`,
  `check (windows-latest)` (including hosted audit/advisory and license/dependency
  checks in the workflow).
- Native Python: `python-native (ubuntu-latest, 3.12)` and
  `python-native (macos-latest, 3.12)`.
- Language clients: Ubuntu Python 3.11/3.12 × Node 20/22 and macOS Python
  3.11/3.12 × Node 20/22.

## Planning and successor disposition

`plans/registry.md`, `plans/README.md`, `plans/roadmap.md`, `AGENTS.md`, and
the current compatibility/verification deep dives now identify M040 as closed
and the final repository-level ADR 007 authority. Historical M036–M039 closure
evidence was preserved with additive correction references. No future numbered
plan is currently blocked on M040, and no plan needs to be promoted from
`blocked` or `ready` as a consequence of this closure. M040 activates no
automatic successor. Any future Toxiproxy tagged-release promotion requires a
separately registered plan that compares the release with the pinned
`40f7fd31` snapshot.

No unresolved medium-or-higher correctness or security finding remains from
the M040 scope. This closure does not change ADR 007 semantics, datagram
semantics, or historical M036–M039 evidence.
