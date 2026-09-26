# M045 — Performance Optimization Exact-Head Qualification and Reconciliation

Status: blocked

Role: combined exact-candidate qualification/closure gate for M042–M044

Depends on: M043, M044

## Objective

Qualify the combined stream/datagram optimization tree on one exact candidate,
prove that the public/capability surface and deterministic contracts did not
regress, reconcile performance evidence and planning state, and establish the
post-M041 performance authority.

M045 adds no optimization of its own except narrowly necessary qualification
or documentation corrections. If qualification reveals a production defect,
move the responsible implementation milestone back to active or register a
bounded corrective successor rather than hiding the issue in closure text.

## Inputs

M045 consumes:

- M042 corrected benchmark authority, raw pre-optimization baselines,
  classifications and frozen success/regression thresholds;
- M043 exact stream optimization closure and before/after evidence;
- M044 exact datagram scaling closure and before/after evidence;
- historical M008 TCP, M023 direct-UDP and M024 topology-matched performance
  budgets;
- current strict v2.12 and pinned post-v2.12 Toxiproxy authorities;
- ADR 001/002/003/004/005/006/007 deterministic/integration boundaries.

## Scope

Qualification/reconciliation only:

- repository-wide tests/lints/docs/security/package checks;
- corrected stream/datagram performance harnesses;
- deterministic/golden/fuzz evidence;
- strict and post-v2.12 Toxiproxy regressions;
- Eggfetch and language/native control regressions where shared core/server
  code is touched;
- planning/architecture/performance documentation;
- exact hosted CI evidence.

## Non-goals

- no new fault;
- no public API redesign;
- no new benchmark budget invented from the post-optimization candidate;
- no additional optimization unrelated to a qualification defect;
- no historical closure rewrite;
- no release/tag action.

## Work packages

### WP1 — Reconcile exact implementation head

Before qualification:

- ensure M043 and M044 closure candidates are both reachable from the target
  branch;
- record the exact combined candidate SHA;
- confirm there are no unregistered production changes after either
  optimization closure;
- reconcile any documentation-only follow-up before beginning final evidence.

If code changes after qualification begins, restart exact-candidate evidence.

### WP2 — Run corrected performance authority

Run M042's full corrected stream and datagram harnesses on the exact combined
candidate.

For every targeted optimization, report:

- M042 baseline metric;
- M043/M044 local after metric;
- M045 combined-head metric;
- frozen threshold;
- pass/fail/inconclusive status.

The combined candidate must pass every historical budget:

- M008 no-fault stream budget;
- M023 direct UDP budget;
- M024 topology-matched sequential throughput/p95 and windowed throughput.

Do not retune M042 thresholds from M045 output.

Retain raw M045 artifacts under `qualification/performance/`.

### WP3 — Determinism, fault, and lifecycle regression

Run and record:

- `eggchaos-core` complete tests/properties;
- stream RNG golden vectors;
- stream-loss fragmentation/correlation/golden behavior;
- ADR 003 datagram golden traces;
- datagram association setup/waiter/capacity/idle/kill/drain race suite;
- live policy generation transition/drain/termination tests;
- Scenario V1/V2 deterministic publication/ownership tests.

Any changed golden trace/vector is a release-blocking finding for this tranche
unless a separately registered semantic change explicitly authorizes it.

### WP4 — Public/cross-surface capability regression

Prove that internal optimization did not narrow capability.

At minimum:

- full workspace all-feature test/doc/clippy gate;
- native OpenAPI/operation drift check;
- Python/TypeScript remote SDK checks;
- embed/native-Python checks where supported by host;
- Eggfetch qualification;
- strict Toxiproxy v2.12 mandatory differential;
- pinned post-v2.12 mandatory qualification.

No native route, DTO, config key, CLI operation, public binding, fault kind, or
adapter capability may disappear.

### WP5 — Security, fuzz, package, artifact regression

Run the current canonical gates rather than reconstructing them manually:

- `./scripts/release-smoke.sh`;
- 10,000-run fuzz qualification (or the repository's then-current mandatory
  release count if higher);
- audit/deny through the canonical scripts/workflow;
- release artifact smoke.

Optimization-specific unsafe code remains forbidden unless a separate ADR was
registered before implementation; M042–M044 do not authorize one.

### WP6 — Hosted exact-head qualification

Push the exact M045 candidate and require the ordinary hosted matrix to finish
green on that SHA:

- Rust check on Linux/macOS/Windows;
- language-client matrix;
- native-Python supported hosts.

If the release workflow is the current authority for performance/fuzz/oracle
qualification, run it on the same candidate and record the run IDs/results.

A run on an earlier implementation SHA is historical context, not M045
closure evidence.

### WP7 — Reconcile documentation and planning

Update current-state documents to describe only optimizations that actually
landed and measured results that actually exist.

At minimum reconcile:

- `plans/registry.md`;
- `plans/README.md`;
- `plans/roadmap.md`;
- relevant `architecture/` performance/runtime deep dives;
- `qualification/performance/README.md`;
- `AGENTS.md` current planning state if its status summary references this
  tranche.

Do not rewrite M008/M023/M024 historical closure records.

## Required invariants

- public API/capability surface is not reduced;
- deterministic RNG/golden traces are unchanged;
- fault composition and queue/termination semantics are unchanged;
- strict Toxiproxy v2.12 remains default/frozen;
- post-v2.12 stream-loss profile remains opt-in/pinned;
- fixed-target proxy boundary remains intact;
- Eggress remains TCP relay authority;
- no new state store or alternate runtime path is introduced;
- no historical performance budget is weakened;
- M042 baseline/threshold artifacts remain immutable historical evidence.

## Minimum qualification commands

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

Also run `./scripts/check_python_native.sh` and
`./scripts/qualify_python_native.sh` on supported hosts, or rely on the
hosted native-Python gate and record why local execution was unavailable.

## Acceptance criteria

M045 can close only when:

- the full qualification is tied to one exact combined candidate;
- every M042 frozen optimization/regression threshold passes or has an
  explicitly evidenced no-op disposition already accepted by M043/M044;
- M008/M023/M024 performance budgets pass unchanged;
- corrected destructive stream-loss benchmark semantics remain valid;
- deterministic stream/datagram golden behavior is unchanged;
- full public/control/config/SDK/embed/Toxiproxy/Eggfetch capability remains
  present;
- fuzz/security/package/artifact gates pass;
- required hosted jobs are green on the exact candidate;
- documentation/registry state matches the implementation;
- no unresolved medium-or-higher correctness/security/performance-regression
  finding remains.

## Rejection / stop conditions

Do not close M045 if:

- a benchmark threshold is relaxed after seeing the optimized candidate;
- a historical budget fails and is waved as host noise without a controlled
  rerun/evidence;
- a public API/capability regression is accepted in exchange for throughput;
- an RNG/golden difference is dismissed as "internal";
- a mandatory oracle is unavailable but reported as a pass;
- hosted evidence belongs to a different code SHA;
- M045 itself accumulates unrelated production optimization work.

## Closure evidence

Create:

`plans/closure/M045-performance-optimization-exact-head-qualification-and-reconciliation-closure.md`

The closure must record the exact candidate, local and hosted run IDs, complete
before/after table, historical budget results, target disposition, API/
determinism/oracle/fuzz/security/package evidence, limitations, and successor
activation.

## Successor activation

A clean M045 closes the registered post-M041 performance tranche and activates
no automatic successor.

Further performance work requires a new measured finding and numbered plan;
do not keep an open-ended "optimization" milestone alive indefinitely.
