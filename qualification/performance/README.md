# Performance qualification

Run `./scripts/benchmark.sh` on the release candidate. The harness measures a
64 KiB-buffered bare `eggress-relay`, an empty-plan eggchaos relay (both the
static `ChaosStream::new` constructor and the production-representative live
`ChaosStream::new_live` constructor), latency/bandwidth/slice/combined fault
plans, the destructive stream-loss zero/intermediate/full/latency cases, and
the Eggfetch adapter. Each preserving case asserts `bytes_accepted == input &&
bytes_discarded == 0 && bytes_received_target == input`; each destructive
case asserts `bytes_accepted == input && bytes_forwarded + bytes_discarded ==
bytes_accepted && bytes_received_target == bytes_forwarded`. Three write
profiles are exercised per plan shape: large (8 MiB single-shot), small
(1 KiB × N), and vectored (4 KiB iovecs joined by the chaos engine). Set
`EGGCHAOS_BENCH_BYTES` and `EGGCHAOS_BENCH_ROUNDS` to repeat a larger or
longer sample set; `EGGCHAOS_BENCH_CASE=<name>` selects one case for focused
runs.

The TCP harness also emits benchmark-only probes to stderr (`format: 2`) for
costs that cannot be inferred from end-to-end throughput alone: full
`LivePolicy::snapshot` round-trip vs generation-only read,
`DirectionEngine::new` build cost at representative plan shapes, and
stream-loss engine build cost at zero/full loss.

Record the raw JSON together with CPU/model, OS, Rust version, release profile,
payload size, rounds, throughput, and any scheduler/power-management caveats.
The bare relay is the comparison baseline; deliberate fault delay is not
included in the no-fault overhead budget.

The sibling UDP harness is `benchmarks/src/bin/datagram.rs`; run it with
`./scripts/benchmark_datagram.sh`. It reports three baselines at 1200-byte
payloads — direct UDP echo, a benchmark-local bare fixed-target relay with
the same client -> proxy -> fixed target -> proxy -> client socket topology
but no `DatagramDirectionEngine`, and the eggchaos fixed-target runtime —
in two workload modes: sequential RTT (one datagram outstanding, for p50/p95
latency) and windowed throughput (32 outstanding sequence-tagged datagrams,
for sustained datagrams/s without serial RTT dominating). The bare relay is
benchmark-only and must not become a second production runtime. The harness
also emits core-only scheduler scaling probes (`scheduler` array: admit and
`next_deadline`/`take_ready` cost at ready-queue depths 1/32/256/1024, no
sockets involved) and measures delay/loss/duplicate/reorder/
corruption/bandwidth, a combined plan, and eight concurrent client
associations. The script records raw samples plus candidate SHA, CPU,
OS, architecture, and Rust version when `EGGCHAOS_DATAGRAM_BENCH_OUTPUT` is
set. `EGGCHAOS_DATAGRAM_BENCH_DATAGRAMS` and
`EGGCHAOS_DATAGRAM_BENCH_ROUNDS` adjust workload size.
`EGGCHAOS_DATAGRAM_BENCH_CASE` selects one case (e.g. `empty_plan_windowed`
or a `scheduler_depth_*` probe source) for focused runs.

M042 added pre-warmed high-association scale workloads to the UDP harness:
`scale_warm_<N>` and `hot_with_idle_<N>` for `N ∈ {1, 8, 256, 1_024,
4_096}`. Each pre-warms `N` associations via one warmup packet per distinct
client before timing a single hot client's RTT throughput and latency, so the
timed interval excludes the listener-side setup path; the harness records
`achieved_cardinality` separately from `target_cardinality` so OS port/FD
limits can be distinguished from genuine registry/reaper regressions.

M023's first direct-UDP baseline selected a same-session empty-plan budget of
at least 45% of direct datagrams/second median and empty-plan p95 latency no
more than 2.5 times direct p95 latency. The script checks both ratios. These
relative thresholds tolerate host speed changes while keeping a visible
regression limit; absolute results remain host-specific. Deliberate timer-based
fault cases are reported but excluded from the empty-plan budget.

Exact candidate `ae2ab733b2be199d7693e40cdc558df01ee9a9de` was measured on
2026-09-24 with rustc 1.89.0 on an Apple M4 Pro (macOS arm64), 1200-byte
payloads, 2000 logical datagrams per case, and three rounds. The direct UDP
echo median was 43,786.71 datagrams/s (p95 42 µs); the fixed-target empty-plan
median was 24,107.89 datagrams/s (p95 66 µs), a 0.5506 throughput ratio and
1.5714 p95 ratio. The checked raw 30-sample report is
`2026-09-24-macos-arm64-m023.json`. Fault cases and eight-client throughput
are in that report; scheduler timer granularity dominates the 20 µs delay and
reorder cases on this host.

M024 added the topology-matched bare-relay baseline and froze a second
budget from measured evidence: same-session empty-plan/bare-relay sequential
throughput ≥ 0.7, sequential p95 ratio ≤ 1.6, and windowed empty/bare
throughput ≥ 0.7. The pre-change production tree (`3c3a7ba`, measured with
the new harness before any hot-path change) showed matched medians of 0.9479
sequential throughput, 1.025 sequential p95, and 0.8559 windowed — the 0.7 /
1.6 thresholds sit well below observed behavior with headroom for host
variance, while the retained M023 direct floor still guards end-to-end
regressions. Pre-change scheduler probes confirmed the scaling cost being
removed: `next_deadline` peek 1 ns at depth 1 rising to 1374 ns at depth
1024 (full-queue scan), and not-ready `take_ready` 10 ns rising to 711 ns
(full-queue sort on every check). Raw reports are
`2026-09-24-macos-arm64-m024-before.json` and
`2026-09-24-macos-arm64-m024-after.json` (1200-byte payloads, 2000 datagrams
per case, 3 rounds, window 32).

The M024 candidate `ca46801bb5f12a9ecf9232f33d8840bd0c09afad` was measured
in the same host class (Apple M4 Pro, macOS arm64, rustc 1.89.0) with the
same workload. Before → after medians: direct sequential 36,831 → 39,149
datagrams/s (p95 50 → 43 µs); bare relay 21,108 → 21,494 (p95 80 → 71 µs);
empty-plan 20,007 → 20,566 (p95 82 → 79 µs); direct windowed 135,357 →
140,911; bare windowed 89,091 → 82,137; empty windowed 76,257 → 82,731.
Topology-matched ratios moved 0.9479 → 0.9568 (sequential throughput), 1.025
→ 1.1127 (sequential p95), and 0.8559 → 1.0072 (windowed throughput): a
repeatable windowed improvement with no material sequential change, and the
retained M023 floor passes (0.5253 ≥ 0.45, 1.8372 ≤ 2.5). Scheduler probes
after the change are flat with depth: `next_deadline` peek ~0.5 ns and
not-ready `take_ready` ~2.7 ns at all depths (was 1374 ns / 711 ns at depth
1024); per-item drain cost rose from ~5 ns to ~40 ns (heap pops instead of
one memmove drain) and is negligible next to socket I/O. A host sampling
profiler was attempted (`xctrace` requires full Xcode, absent; `sample`
produced unsymbolized stacks), so profiling evidence rests on these built-in
timing probes plus the end-to-end triad, which measure the exact suspected
hot paths.

## M042 measurement authority and baseline

M042 corrected destructive stream-loss benchmark semantics (the prior harness
implicitly asserted the target would receive the full input byte count, which
is only true for preserving faults) and added `static-empty` vs
`live-empty`, small-write, and vectored-write stream profiles so the public
`LivePolicy` fast path and the iovec join can be measured instead of inferred.
It also added high-association datagram scale (`scale_warm_<N>` /
`hot_with_idle_<N>`) so M043/M044 optimization candidates have a measured
baseline rather than a source-level hypothesis. M042 baseline artifacts are
`2026-09-26-macos-arm64-m042-stream.json` (cases + raw samples),
`2026-09-26-macos-arm64-m042-stream-probes.json` (stderr-emitted microprobes),
and `2026-09-26-macos-arm64-m042-datagram.json` (existing triad plus the new
scale cases).

## M043/M044 optimization evidence (M045 combined head)

M043 implemented shared `Arc<FaultPlan>` engine ownership (WP3) and a
bounded preserving-plan vectored prefix copy (WP7, no-op disposition
against the ≥10 % threshold); M042-classified WP2/WP4/WP5/WP6 were
skipped. M044 removed the whole-`DatagramProxySpec` per-datagram clone
(WP2), converted the association map to a short-critical-section std
`RwLock` with a read fast path (WP3, no-op disposition against both
≥10 % scale thresholds), and sized duplicate-stage candidate buffers
from bounded amplification (WP5); WP4/WP6 were skipped and WP7 was
not implemented per the M042 matrix. Combined-head artifacts on the
M045 candidate are
`2026-09-26-macos-arm64-m045-stream.json`,
`2026-09-26-macos-arm64-m045-stream-probes.json`, and
`2026-09-26-macos-arm64-m045-datagram.json`. Stream byte-conservation
invariants hold on every case; M023/M024 budgets, M042 latency
tightness, and hot-with-idle parity hold; same-host A/B runs show
parity where file-baseline deltas reflect host load (a load-limited
stream run was discarded and re-evidence under a controlled rerun —
bare-relay parity is the validity check).

## M042–M045 provenance map (M046 authority)

M046 freezes six distinct concepts per artifact family; do not collapse
them into one generic "candidate" when they differ. `candidate_sha`
is stamped by `scripts/benchmark_datagram.sh` as `git rev-parse HEAD`
at run time, so a dirty-worktree run records its **base HEAD**, not a
content-addressed harness tree. The stream harness emits no
`candidate_sha`; stream/probe rows below therefore infer the base HEAD
from the co-committed datagram session and the parent commit, marked
as inferred. `unknown / not reconstructable` is written where history
cannot prove more.

| Artifact family | `candidate_sha` embedded | Production revision measured | Benchmark-harness revision (clean commit, if any) | Execution base HEAD | Artifact evidence commit | Artifact refresh commit | Hosted qualification revision |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `m042-stream.json` / `m042-stream-probes.json` | none (stream harness emits none) | `2ce0c232`-based dirty worktree incl. uncommitted M042 harness (inferred; exact dirty content not recoverable) | `fb2c8fc` (harness first committed there) | `2ce0c232` (inferred from co-committed datagram session) | `fb2c8fc` | none | n/a (local only) |
| `m042-datagram.json` | `2ce0c232` | `2ce0c232`-based dirty worktree incl. uncommitted M042 harness (production code unchanged by M042; harness is the delta) | `fb2c8fc` | `2ce0c232` (proven: embedded SHA + `2ce0c232..fb2c8fc` harness diff) | `fb2c8fc` | none | n/a (local only) |
| `m043-stream.json` / `m043-stream-probes.json` | none | `fb2c8fc`-based dirty worktree whose prod content equals `67ce4ab` (M043 stream-only prod delta) | `67ce4ab` (stream files committed with prod) | `fb2c8fc` (inferred: parent of `67ce4ab`; JSON proves nothing alone) | `67ce4ab` | none | n/a (local only) |
| `m043-datagram.json` | `fb2c8fc` | datagram-path production identical at `fb2c8fc` and `67ce4ab` (M043 prod delta is stream-only: `engine.rs`/`stream.rs`); measured tree's datagram production revision is `fb2c8fc`-equivalent | `fb2c8fc` (no datagram-harness change `fb2c8fc..df81ea4`) | `fb2c8fc` (proven embedded; file first stored one commit later in `df81ea4`) | `df81ea4` (M043 closure commit) | none | n/a (local only) |
| `m044-datagram.json` | `df81ea4` | `df81ea4`-based dirty worktree whose prod content equals `7a6dbb8` (M044 datagram prod delta committed there) | `7a6dbb8` | `df81ea4` (proven embedded) | `7a6dbb8` | none | n/a (local only) |
| `m045-stream.json` / `m045-stream-probes.json` | none | `e8ff753` (== `a27a67a` production; `e8ff753..a27a67a` is docs + JSON only) | unchanged from M042–M044 harness | `e8ff753` (inferred from parent + co-committed generation-1 datagram) | `a27a67a` | none (stream files untouched by `37acc6b`) | `a27a67a` via run `36255454409` (13/13 green) |
| `m045-datagram.json` generation 1 | `e8ff753` | `e8ff753` (== `a27a67a` production) | unchanged | `e8ff753` (proven embedded) | `a27a67a` | superseded by generation 2 in `37acc6b` | `a27a67a` via run `36255454409` (13/13 green) |
| `m045-datagram.json` generation 2 (current canonical) | `a27a67a` | `a27a67a` (production-identical to `e8ff753`) | unchanged | `a27a67a` | `37acc6b` | n/a (latest) | `a27a67a` via run `36255454409` (13/13 green) |

Preserved immutable copies (byte-for-byte, embedded metadata untouched):

- `2026-09-26-macos-arm64-m045-datagram-e8ff753.json` — copy of the
  `a27a67a` blob (generation 1, `candidate_sha = e8ff753…`).
- `2026-09-26-macos-arm64-m045-datagram-a27a67a.json` — copy of the
  `37acc6b`/HEAD blob (generation 2, `candidate_sha = a27a67a…`).

Naming rule for future refreshes: never overwrite a retained raw file
to change its embedded `candidate_sha`. Add a new file suffixed with
the execution SHA (`-datagram-<short-sha>.json`) copied byte-for-byte
from the source blob, and update this table to say which generation
the canonical filename points to. The M042 closure's original
non-resolving `2ce0c238…` SHA is corrected additively in the M042
closure (transcription error; base HEAD `2ce0c232`, evidence commit
`fb2c8fc`) — see the M046 closure for the before/after record.
