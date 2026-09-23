# M013 — Corrective Requalification Gate

Status: closed
Depends on: M009, M010, M011, M012
Successor: M008 resumes

## Historical closure note

M013 closed with a clean verdict at candidate `9904490`; see `plans/closure/M013-corrective-requalification-gate-closure.md`. Status retained as completed history; M014 reconciles planning state and M015 is the final pre-tag authority.

## Objective

Independently requalify the corrected implementation before release work resumes.

M013 exists because M009–M012 repair behavior that was previously recorded as closed under M002–M006. The historical closure records remain useful evidence of prior work, but they are no longer sufficient for a release candidate after the audit findings and corrective changes.

M013 is not a release/publishing milestone. It decides whether the implementation is again coherent enough for M008 to continue with packaging, target artifacts, performance baselines, and external release evidence.

## User-visible outcome

A single candidate commit has reproducible evidence that:

- every release-baseline fault executes correctly;
- dynamic native proxy/fault CRUD controls real listeners and live state;
- live mutation/scenario generation/seed semantics are coherent;
- Toxiproxy v2.12 compatibility is backed by a meaningful oracle corpus;
- Eggfetch integration still works after core/runtime changes;
- CI/security/fuzz/property tests are green;
- documentation and registry no longer overstate implementation state.

## Preconditions

M009, M010, M011, and M012 each have their own closure records.

Do not begin by assuming those closures compose. M013 reruns cross-layer interactions on one exact commit.

## Scope

M013 owns qualification and narrow corrective fixes only.

If qualification reveals a substantial new design/correctness issue, stop and create M014+ corrective plans rather than hiding broad implementation inside M013.

Affected evidence surfaces may include:

```text
qualification/
benchmarks/
fuzz/
scripts/
plans/reference/verification-matrix.md
plans/reference/toxiproxy-parity.md
plans/closure/
README.md
docs/
plans/registry.md
```

## Non-goals

Do not:

- publish crates;
- tag a release;
- create release artifacts as final release evidence;
- add UDP/proxy chains/new faults;
- expand beyond Toxiproxy v2.12;
- introduce new product features.

Those remain M008 or post-v1 work.

## Requalification model

Treat historical M002–M006 closure records as historical implementation evidence, not current release verdicts.

M013 closure should contain a reconciliation table:

| Historical area | Corrective plan | Current exact-commit verdict |
| --- | --- | --- |
| M002 core faults | M009 | pass/fail + evidence |
| M003/M004 runtime/control | M010 | pass/fail + evidence |
| M005 live/scenario | M011 | pass/fail + evidence |
| M006 Toxiproxy | M012 | pass/fail + evidence |
| M007 Eggfetch | impacted by M009/M011 | pass/fail + evidence |

Do not rewrite old closure files to pretend they were based on the corrected code.

## Full workspace gate

Run:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
cargo build --workspace --release
cargo audit --deny warnings
cargo deny check advisories licenses bans sources
```

Also build/test at Rust 1.89.

CI must execute the supported host matrix at least Linux/macOS/Windows x86_64 host runners. Architecture artifact cross-build remains M008 unless ordinary CI already covers it.

## Core fault matrix

Re-run every release-baseline row in `plans/reference/verification-matrix.md`.

Required independent cross-checks include:

- multi-write latency queueing;
- bounded backpressure;
- token-bucket rate/burst;
- slicer variation/delay;
- blackhole finite/indefinite;
- exact limit-data termination;
- graceful disconnect;
- hard-reset request/application evidence;
- probability deterministic 0/1/intermediate;
- live mutation with queued data;
- flush/shutdown/cancellation.

Add a qualification test that combines representative faults in order rather than only testing one at a time.

## Runtime/control matrix

On one service process:

1. create proxy via API;
2. verify actual listener and traffic;
3. add fault;
4. verify live traffic behavior;
5. update fault;
6. update upstream;
7. disable/enable;
8. inspect/kill connection;
9. reset;
10. delete proxy;
11. verify listener disappearance;
12. clean shutdown.

Repeat key operations via CLI `--json` rather than only direct Rust calls.

Exercise bind conflicts and failed updates to verify rollback consistency.

## Live/scenario matrix

Run deterministic fixture scenarios with:

- same seed repeated;
- different seed;
- active buffered connection;
- manual mutation interleaved with scenario event;
- cancellation;
- service shutdown.

Compare evidence records and ensure no stale plan rollback.

Metrics should reconcile with the same fixture.

## Toxiproxy gate

Run the pinned v2.12 differential corpus from M012.

The M013 record must include:

- oracle tag/version;
- checksum;
- platform;
- exact case count;
- pass/fail/divergence count;
- client smoke versions.

No “differential incomplete” wording is acceptable for M013 closure if the missing cases cover claimed release-baseline toxics/routes. Narrow platform-specific reset limitations may remain explicitly classified.

## Eggfetch regression gate

Because M009/M011 alter stream/policy semantics, rerun and expand Eggfetch qualification.

At minimum:

- H1 no-fault;
- H1 keep-alive live update;
- HTTPS valid test trust;
- invalid certificate rejection under normal policy;
- H2 concurrent streams over one physical connection;
- blackhole/read timeout interaction;
- mid-response termination;
- downstream bandwidth;
- retry creating a new physical connection;
- error/redaction assertions.

Physical-stream semantics remain the contract.

## Fuzz/property gate

Run meaningful bounded fuzz/property work against the corrected code.

At minimum:

- plan/config parsing;
- arbitrary fault-plan validation;
- partial write/read fragmentation;
- live generation transition sequences;
- evidence serialization;
- Toxiproxy DTO attributes.

Record iteration/time counts rather than merely saying “fuzz passed.”

## Performance sanity

M013 does not freeze final M008 budgets, but it must catch gross regressions introduced by corrective work.

Record at least:

- bare/empty stream baseline;
- latency queue overhead excluding intentional delay;
- bandwidth limiter overhead;
- standalone no-fault relay;
- Eggfetch empty-policy path.

If no-fault overhead regresses materially versus the existing recorded baseline, investigate before M013 closure.

## Documentation/registry reconciliation

Before closure:

- README capabilities match actual implementation;
- native API docs list actual routes;
- CLI docs list actual commands;
- fault semantics match M009;
- scenario/replay docs match M011;
- Toxiproxy matrix matches M012;
- M008 blocked record is marked historical/stale where corrective work supersedes assumptions;
- registry identifies M013 as the gate immediately before M008.

## Ordered work packages

1. **WP1 — Candidate freeze:** select one exact commit containing closed M009–M012 and do not mix evidence from moving commits.
2. **WP2 — Workspace/security gate:** run format/clippy/tests/docs/release/MSRV/audit/deny.
3. **WP3 — Core/runtime/live qualification:** execute full fault, dynamic lifecycle, cancellation, scenario, evidence, and metrics matrices.
4. **WP4 — External compatibility/integration:** run pinned Toxiproxy corpus/client smokes and expanded Eggfetch H1/HTTPS/H2 regression.
5. **WP5 — Fuzz/property/performance sanity:** record bounded fuzz counts and comparative performance.
6. **WP6 — Documentation/state census:** compare every implemented public surface against README/docs/registry/reference files.
7. **WP7 — Narrow fixes only:** repair qualification defects that do not require new architecture; otherwise stop and create a new plan.
8. **WP8 — Closure verdict:** write `plans/closure/M013-corrective-requalification-gate-closure.md` and activate M008 only on a clean verdict.

## Acceptance criteria

M013 closes only when:

- one exact candidate commit passes the full workspace gate;
- core fault semantics satisfy M009 across integration/property tests;
- dynamic native proxy/fault operations correspond to actual runtime state;
- live policy/scenario seeds/generations/evidence are coherent;
- Toxiproxy v2.12 claimed surface has complete pinned-oracle differential evidence;
- Eggfetch integration passes the expanded regression matrix;
- fuzz/property run counts are recorded and nontrivial;
- no gross no-fault performance regression is unexplained;
- docs/reference/registry match reality;
- no unresolved correctness finding is being deferred merely because M008 exists.

## Stop/rejection conditions

Do not close M013 if:

- a release-baseline fault remains type-only or partially executed;
- proxy CRUD can still diverge from listener state;
- reset remains a no-op;
- scenario seed remains metadata-only;
- Toxiproxy declared route/toxic coverage lacks oracle evidence;
- Eggfetch pooled/H2 behavior regresses;
- cross-layer tests pass only when individual components are tested in isolation;
- qualification evidence comes from different candidate commits without clear justification.

## Follow-on activation

On successful M013 closure:

- M013 -> `closed`;
- M008 -> `ready`;
- update the M008 note to state that implementation correctness was requalified and remaining work is release/package/target/performance evidence.

If M013 fails on a substantial issue, keep M008 blocked and create the next numbered corrective plan instead.
