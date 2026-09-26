# M042 — Performance Measurement Authority and Baseline Correction

Status: ready

Role: post-M041 performance prerequisite and benchmark-authority corrective

Depends on: M041 closed at `724b967da04579282dd8bfc7a81dc4fe55d034a2`

## Objective

Repair the performance harness where its semantics no longer match the current
fault surface, then establish reproducible stream and datagram baselines for the
next optimization tranche before changing production hot paths.

This milestone is measurement-first. It must distinguish measured material
costs from source-level hypotheses and freeze the evidence needed by M043
(stream hot path) and M044 (datagram runtime scaling). It does not optimize the
production runtime itself.

## Baseline and investigated findings

Current `main` at plan registration is
`1e26579c58159fe286db5f7dbc6c2a8143f6ba04`.

The existing benchmark/qualification surface is useful but incomplete for the
current implementation:

1. `benchmarks/src/main.rs` now includes destructive
   `stream_loss_mid`/`stream_loss_full` cases, but `run_chaos()` still
   asserts that the target receives the complete input byte count. That
   assertion is correct only for preserving faults. Full stream loss is
   defined to forward zero bytes, so the harness cannot be the authority for
   destructive stream-loss performance until the case model is corrected.
2. The historical TCP empty-plan benchmark constructs a static
   `ChaosStream::new(... FaultPlan::empty())`. Production TCP proxy
   connections use `ChaosStream::new_live(...)`, whose steady-state I/O path
   checks the published generation. Static empty-plan performance therefore
   does not isolate the live-policy overhead used by the server and Eggfetch
   adapter.
3. Existing TCP cases are dominated by large sequential writes. They do not
   expose the cost profile of many smaller polls or non-empty vectored writes,
   where live-policy checks, plan scanning, evidence mirroring, timer
   re-arming, and iovec joining are more likely to matter.
4. M024 already corrected the major datagram scheduler pathology and froze a
   topology-matched bare fixed-target baseline. Do not reopen the heap
   scheduler without new evidence.
5. The current UDP listener clones a complete `DatagramProxySpec` on every
   client datagram before association resolution, and every active ingress
   datagram traverses the association-map synchronization path. The existing
   1/8-client benchmark does not isolate scaling at hundreds or thousands of
   live associations.
6. The idle reaper scans active associations every 10 ms while the default
   association idle timeout is 60 s. Source inspection suggests a possible
   high-cardinality idle cost, but there is no current benchmark proving it is
   material.

The existing M008/M023/M024 budgets and raw artifacts remain historical
regression authorities. M042 may add measurement dimensions; it must not
weaken or rewrite those budgets.

## Scope

Affected surfaces:

- `benchmarks/src/main.rs`
- `benchmarks/src/bin/datagram.rs`
- `scripts/benchmark.sh`
- `scripts/benchmark_datagram.sh`
- `qualification/performance/`
- performance/verification documentation where the benchmark contract is
  described

Test-only/internal probe support may be added when necessary, but M042 must not
change public Rust API, native HTTP/OpenAPI, config/TOML, CLI, SDK, Toxiproxy,
fault, deterministic RNG, queue, connection, or association semantics.

## Non-goals

- no production data-plane optimization;
- no new fault kind or fault semantic;
- no new public benchmark API in `eggchaos-core` or `eggchaos-server`;
- no new runtime dependency solely for benchmarking;
- no replacement of M024's datagram heap scheduler;
- no invented performance target before a target-class baseline exists;
- no claim that one favorable local run proves an optimization target.

## Work packages

### WP1 — Correct destructive stream benchmark semantics

Refactor the TCP benchmark case description so every case declares whether
bytes are preserving or destructive.

The harness must report, at minimum, for each stream case:

- input bytes presented by the benchmark client;
- caller-visible bytes accepted by the chaos stream;
- bytes forwarded to the target;
- bytes intentionally discarded by the engine when applicable;
- elapsed time / throughput;
- raw per-round samples.

For preserving cases, retain exact byte conservation at the target.

For stream loss:

- zero-loss must preserve the complete input;
- full-loss must accept the input and forward zero payload bytes;
- intermediate-loss cases must prove
  `accepted == forwarded + intentionally_discarded` after the final
  delivery barrier/close;
- no destructive case may hang waiting for an impossible full target byte
  count.

Do not weaken production semantics to make the benchmark easier.

### WP2 — Add production-representative stream baselines

Add explicit cases separating:

1. bare `eggress-relay`;
2. static `ChaosStream` with empty plan;
3. live-policy `ChaosStream::new_live` with empty plan and no publications;
4. live-policy preserving fault plans whose deliberate delay is zero or
   otherwise excluded from pure overhead measurement;
5. corrected stream-loss zero/intermediate/full cases;
6. one controlled live-generation transition case.

Retain the existing deliberate latency/bandwidth/slice workload cases for
semantic/performance context.

Add at least two write profiles:

- existing large-buffer throughput;
- a many-small-write profile that generates materially more
  `poll_write`/generation-check/evidence activity.

Add a vectored-write profile for a non-empty preserving plan if the benchmark
transport supports a stable `AsyncWrite::poll_write_vectored` exercise.
This profile exists to measure the current full-iovec join behavior, not to
assume it is a material bottleneck.

Every case name and JSON field must be stable enough for before/after
comparison in M043/M045.

### WP3 — Add stream hot-path probes without public API expansion

Add narrow benchmark/probe coverage for costs that cannot be inferred reliably
from end-to-end throughput alone. Prefer existing public crate seams and
benchmark-local helpers. Test-only/internal instrumentation is acceptable if
it does not alter release behavior.

Measure where practical:

- repeated live-policy steady-state I/O with no publication;
- engine construction/transition cost as plan stage count increases;
- repeated deadline re-arm behavior for latency/slice/bandwidth-like queues;
- faulted writes that do and do not change mirrored evidence;
- preserving vectored writes where total iovec size greatly exceeds available
  bounded ownership;
- stream-loss logical-grain processing at zero/intermediate/full drop rates.

If a probe cannot be made stable/reproducible, record it as inconclusive
instead of freezing a numeric target.

### WP4 — Expand datagram steady-state and scale measurement

Preserve the M024 direct/bare/eggchaos triad, sequential RTT, windowed
throughput, scheduler-depth probes, and all existing budgets.

Add pre-warmed active-association workloads at representative cardinalities.
At minimum attempt:

- 1 association;
- 8 associations;
- 256 associations;
- 1,024 associations;
- 4,096 associations when host limits permit.

The workload must distinguish setup cost from steady-state lookup/forwarding
cost. Create associations before timing the steady-state interval.

Capture both a hot single/few-client path in the presence of many idle
associations and a distributed multi-client path.

Add an idle-association observation that can reveal whether the 10 ms reaper
scan materially affects CPU/latency as cardinality rises. Prefer end-to-end
observable measurements over exposing private registries. If the host cannot
support a requested cardinality, record the capability limit and continue
with the highest successful level.

### WP5 — Capture exact baseline artifacts and classify targets

Run the corrected harnesses on one exact pre-optimization candidate and retain
the raw output under `qualification/performance/` with:

- exact candidate SHA;
- OS and architecture;
- CPU/model where available;
- rustc version;
- payload/write profile;
- round count/window/concurrency;
- host-load caveats.

For each candidate M043/M044 target, classify it:

- **proven-material** — measured enough to justify production work;
- **low-cost-cleanup** — semantics-preserving change with direct allocation/
  synchronization removal but no claim of major throughput benefit;
- **inconclusive** — measurement too noisy or unavailable;
- **not-material** — measured cost does not justify implementation complexity.

M043/M044 may not turn an inconclusive/not-material hypothesis into a complex
production rewrite merely because the source looks inefficient.

### WP6 — Freeze optimization acceptance thresholds from evidence

Do not invent percentages in this plan.

After the baseline exists, document for each proven optimization dimension:

- the before metric;
- observed run-to-run variance;
- the minimum improvement or maximum regression threshold that will count as
  successful implementation;
- which historical M008/M023/M024 budgets remain independently mandatory.

Thresholds should be conservative enough to tolerate host noise while still
rejecting a no-op optimization.

## Required invariants

- public Rust API surface is unchanged;
- native wire/OpenAPI/config/CLI/SDK surfaces are unchanged;
- `FaultPlan`, datagram-plan, RNG, stream-loss grain/correlation, queue and
  termination semantics are unchanged;
- no fault is reclassified merely to make a benchmark pass;
- M023 direct-UDP and M024 topology-matched budgets remain intact;
- bare benchmark relays remain benchmark-only and never become production
  runtime paths;
- raw unsuccessful/noisy runs are retained or described rather than silently
  cherry-picked away.

## Verification

Minimum commands on the exact M042 candidate:

```sh
./scripts/check.sh
./scripts/benchmark.sh
./scripts/benchmark_datagram.sh
cargo test -p eggchaos-core --all-features
cargo test -p eggchaos-server --all-features
```

Also run the focused benchmark case selectors introduced by this milestone and
record the exact command lines in closure evidence.

If benchmark changes touch Eggfetch-facing stream construction, run:

```sh
./scripts/qualify_eggfetch.sh
```

## Acceptance criteria

M042 can close only when:

- destructive stream-loss benchmark cases have semantically correct
  conservation assertions;
- static-empty and production-representative live-empty stream cases are
  separately measured;
- small-write and, where stable, vectored-write stream profiles exist;
- datagram steady-state scaling includes pre-warmed high-association cases or
  explicitly records host capability limits;
- one exact pre-optimization baseline is preserved under
  `qualification/performance/`;
- every proposed M043/M044 optimization target has a measured classification;
- M008/M023/M024 historical budgets still pass;
- no production capability/API/fault semantic changed;
- planning/registry/verification docs describe the corrected benchmark
  authority.

## Rejection / stop conditions

Move M042 back to active rather than closing if:

- stream-loss throughput is reported while the benchmark still waits for all
  original payload bytes at the target;
- live-policy overhead is inferred from the static empty-plan case;
- datagram scale results include association setup inside the timed interval
  without labeling it;
- thresholds are chosen after viewing an optimization candidate rather than
  from the baseline;
- a noisy/failing baseline run is discarded without retaining/explaining it;
- a public or wire API is added only to make a private performance probe easy.

## Closure evidence

Create:

`plans/closure/M042-performance-measurement-authority-and-baseline-correction-closure.md`

The closure must identify the exact candidate, raw artifact paths, host/toolchain
metadata, corrected case semantics, target classifications, frozen
implementation thresholds, limitations, and successor activation.

## Successor activation

On clean closure:

- M043 becomes `ready` for the stream targets classified
  proven-material/low-cost-cleanup;
- M044 becomes `ready` for the datagram targets classified
  proven-material/low-cost-cleanup;
- targets classified inconclusive/not-material remain explicitly out of the
  implementation handoff;
- M045 remains blocked until both M043 and M044 are closed (or one is closed
  as an evidenced no-op because M042 proved no justified production change).
