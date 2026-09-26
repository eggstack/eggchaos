# M041 — Stream-Loss Metrics and Closure Hygiene Corrective Closure

Status: closed

Exact implementation candidate: `724b967da04579282dd8bfc7a81dc4fe55d034a2`

Closure date: 2026-09-26

Depends on: M040 historical closure record + ADR 007

## Outcome

M041 corrected the three narrow defects registered after M040 without
reopening any ADR 007 stream-loss semantics:

1. **Stream-loss Prometheus exposition**: the per-proxy/direction
   render path in `ControlState::metrics_text()` previously emitted
   the same three stream-loss metric samples twice, with the second
   copy ending in a literal `\\n` (backslash plus `n`) instead of a
   real newline. The duplicate block is removed; the three families
   (`eggchaos_stream_loss_chunks_evaluated_total`,
   `eggchaos_stream_loss_chunks_dropped_total`,
   `eggchaos_stream_loss_bytes_discarded_total`) now render exactly
   once per retained proxy per direction through a single helper,
   using real `\n` line terminators. The bounded `_overflow` proxy
   uses the same helper. The legacy seven-slot activation arrays,
   `FAULT_TYPE_NAMES`, the bounded proxy cardinality, and the
   existing `metrics_use_only_bounded_label_keys` invariants are
   unchanged.

2. **Post-v2.12 fetcher stdout contract**: `scripts/fetch_toxiproxy_post_v2_12.sh`
   now restores the documented shell-substitution behavior. The
   default mode and the explicit `--path-only` flag both print
   exactly one executable path on stdout; `--json` prints the single
   JSON metadata record (`requested_toolchain`,
   `resolved_go_version`, `resolved_gotoolchain`, `source_commit`,
   `source_sha256`, `oracle_path`, `oracle_version`); `--help` exits
   0 with usage on stderr; unknown flags exit 2 with a diagnostic on
   stderr. Diagnostics never leak onto stdout. The qualifier
   `scripts/qualify_toxiproxy_post_v2_12.sh` consumes the path via
   an explicit `--path-only` fetch mode so the documented
   `TOXIPROXY_POST_V2_12_SERVER="$(...)"` substitution and the
   mandatory qualifier both remain unambiguous.

3. **Planning/document drift**: `AGENTS.md` and `docs/toxiproxy.md`
   reflect the corrected fetcher contract. The M040 numbered plan
   remains historically `closed`; `plans/registry.md`,
   `plans/README.md`, `plans/roadmap.md`, and `AGENTS.md` identify
   M041 as the latest ADR 007 repository-level authority. No
   historical M036–M040 closure evidence was rewritten.

A new focused regression
`scripts/tests/test_fetch_toxiproxy_post_v2_12_contract.sh` is wired
into the `language-clients` CI job to enforce the fetcher stdout
contract end-to-end whenever a cached oracle exists, alongside the
static-pattern and argument-parsing checks that always run.

## Local exact-candidate evidence

All results below were run on candidate
`724b967da04579282dd8bfc7a81dc4fe55d034a2`:

- `./scripts/check.sh` passed: `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets --all-features -- -D
  warnings`, `cargo test --workspace --all-features`, and
  `cargo doc --workspace --all-features --no-deps`.
- The new M041 regression
  `runtime::tests::stream_loss_prometheus_exposition_is_unique_and_well_formed`
  passed and proves:
  - no literal `\n` byte sequence exists in the rendered `GET
    /metrics` text;
  - each stream-loss `(metric, proxy, direction)` tuple appears
    exactly once for the live proxy and exactly once for the
    bounded `_overflow` proxy;
  - every expected `(metric, proxy, direction)` tuple is present;
  - existing label-key allowlist and zero/full loss counter
    invariants remain unchanged.
- `sh scripts/tests/test_fetch_toxiproxy_post_v2_12_contract.sh`
  passed (cached oracle present in `$TMPDIR`):
  - default mode and `--path-only` each print exactly one
    executable path with no JSON markers;
  - `--json` parses with `python3 -m json.tool` and contains all
    required oracle-identity fields;
  - `--json` produces no stderr diagnostics;
  - `--help` exits 0 with usage on stderr;
  - `--bogus` exits 2 with a diagnostic on stderr;
  - pattern checks confirm `mode="--path-only"` is the default and
    the historical `mode="${1:-}"` parsing is gone;
  - `qualify_toxiproxy_post_v2_12.sh` continues to consume
    `--path-only` explicitly.
- `TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)"
  EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1
  ./scripts/qualify_toxiproxy_v2_12.sh` passed **50/50** against
  pinned Toxiproxy v2.12.0 (oracle SHA-256
  `aa299966b52f16a8594f1cd0d1e9049dc2e8fe2c04a90c19860e2719b2b95d15`).
- `EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1
  ./scripts/qualify_toxiproxy_post_v2_12.sh` passed; the
  post-v2.12 oracle was built from commit
  `40f7fd31bee529d824116bd2a11a9e3425e904ec`, archive SHA-256
  `26351cc70792f1c3391bdd1d376974a79a756b349ec1abfa8b8f1524c042b13d`,
  under requested/resolved `go1.23.0` with `GOTOOLCHAIN=go1.23.0`.
- `./scripts/check_openapi.sh` passed (21 paths, 36 operations).
- `./scripts/release-smoke.sh` and `./scripts/release-artifact-smoke.sh`
  passed.

## Exact-head hosted qualification

GitHub Actions run
[36219464594](https://github.com/eggstack/eggchaos/actions/runs/36219464594)
completed with conclusion `success` on the exact candidate SHA
`724b967da04579282dd8bfc7a81dc4fe55d034a2`. Every job concluded
`success`:

- Rust check matrix: `check (ubuntu-latest)`,
  `check (macos-latest)`, `check (windows-latest)` (including
  hosted audit/advisory and license/dependency checks in the
  workflow).
- Native Python: `python-native (ubuntu-latest, 3.12)` and
  `python-native (macos-latest, 3.12)`.
- Language clients: Ubuntu Python 3.11/3.12 × Node 20/22 and macOS
  Python 3.11/3.12 × Node 20/22. Each language-client job runs the
  new `scripts/tests/test_fetch_toxiproxy_post_v2_12_contract.sh`
  alongside the existing cleanup-trap regression.

## Planning and successor disposition

`plans/registry.md`, `plans/README.md`, `plans/roadmap.md`,
`AGENTS.md`, and `docs/toxiproxy.md` now identify M041 as closed
and the latest ADR 007 repository-level authority. Historical
M036–M040 closure evidence is preserved with the additive reference
that M041 supersedes only the narrow metrics/tooling closure
hygiene layer; ADR 007 stream-loss semantics and the M040
qualification corpus remain authoritative.

No future numbered plan is currently blocked on M041, and no plan
needs to be promoted from `blocked` or `ready` as a consequence of
this closure. M041 activates no automatic successor. Any future
tagged Shopify Toxiproxy release requires a separately registered
plan that compares the tag with the pinned `40f7fd31` snapshot
before changing profile names or compatibility claims.

No unresolved medium-or-higher correctness or security finding
remains from the M041 scope. This closure does not change ADR 007
semantics, datagram semantics, or historical M036–M040 evidence.
