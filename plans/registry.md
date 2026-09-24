# Eggchaos Plan Registry

Last reconciled: 2026-09-24 (ADR 004 accepted; M026–M028 richer deterministic-scenario tranche registered)

This file is the compact source of truth for active milestone state. Detailed scope lives in the numbered plans. Historical closure evidence belongs in `plans/closure/`.

| Milestone | Plan | Status | Depends on | Activation / closure note |
| --- | --- | --- | --- | --- |
| M000 | `000-architecture-and-scope-baseline.md` | closed | — | Initial investigation, architecture, references, and handoff sequence registered. |
| M001 | `001-workspace-bootstrap-and-core-contracts.md` | closed | M000 | Closed in `plans/closure/M001-workspace-bootstrap-and-core-contracts-closure.md` at `309aeff8da9b1d91b36aecd55c190e12d56e537d`. |
| M002 | `002-deterministic-stream-fault-engine.md` | closed | M001 | Historical closure at `d85d25402af4f7c63a45ebb4ebc73b8de727d2e5`; later implementation audit found semantic gaps now assigned to M009. Preserve the historical record rather than rewriting it. |
| M003 | `003-fixed-target-proxy-runtime.md` | closed | M002 | Historical closure at `5dd41027948e7413b595c77f11d7a9e0b31f3785`; runtime/control lifecycle corrections are assigned to M010. |
| M004 | `004-control-plane-cli-and-config.md` | closed | M003 | Historical closure at `14791042aad47dc11d57f84677dcb69ef055d690`; incomplete native runtime/API/CLI authority is assigned to M010. |
| M005 | `005-live-mutation-observability-and-scenarios.md` | closed | M004 | Historical closure at `250494fc7d408c3f86933f44437d454864519984`; state/generation/scenario evidence corrections are assigned to M011. |
| M006 | `006-toxiproxy-v2-12-compatibility.md` | closed | M005 | Historical closure at `e3d1d1faaf390f7d2b8b134f8650dc5a385a0110`; route/semantic parity completion is assigned to M012. |
| M007 | `007-eggfetch-inprocess-integration.md` | closed | M005 | Closed at `eb52ecd2a2a28e06171bbdf96c3ef4947b8d3eb8`; M013 will requalify it after core/live-policy corrections. |
| M008 | `008-qualification-release-and-distribution.md` | closed | — | Closed at `645a761`; evidence in `plans/closure/M008-qualification-release-and-distribution-closure.md`. Tag/crates.io publication/GitHub release remain owner decisions. |
| M009 | `009-core-fault-semantics-corrective.md` | closed | M002 historical implementation | Closed in `plans/closure/M009-core-fault-semantics-corrective-closure.md` at `8c1373e90129e68e96379fc3277fade8ce087abd`. |
| M010 | `010-runtime-control-authority-corrective.md` | closed | M003/M004 historical implementation | Closed in `plans/closure/M010-runtime-control-authority-corrective-closure.md` at `3961e968e98948cfab1d0c99d3503ba1624e2e6`. |
| M011 | `011-live-state-scenario-observability-corrective.md` | closed | M009, M010 | Closed in `plans/closure/M011-live-state-scenario-observability-corrective-closure.md`. |
| M012 | `012-toxiproxy-v2-12-parity-corrective.md` | closed | M009, M010, M011 | Closed at `a040ed7`; evidence in `plans/closure/M012-toxiproxy-v2-12-parity-corrective-closure.md` (47/47 differential vs pinned v2.12.0 oracle, Go + Python client smokes). |
| M013 | `013-corrective-requalification-gate.md` | closed | M009, M010, M011, M012 | Clean verdict at `9904490`; evidence in `plans/closure/M013-corrective-requalification-gate-closure.md`. M008 was subsequently completed. |
| M014 | `014-release-state-and-planning-reconciliation.md` | closed | M008, M013 | Closed with reconciliation commit family; evidence in `plans/closure/M014-release-state-and-planning-reconciliation-closure.md`. Post-M008 lineage recorded; M015 is the final exact-HEAD authority. |
| M015 | `015-final-exact-head-release-requalification.md` | closed | M014 | Clean historical verdict at `cd88b22`; evidence in `plans/closure/M015-final-exact-head-release-requalification-closure.md`. A later audit activated M016–M019, so M015 is no longer the final tag authority. |
| M016 | `016-pre-release-correctness-and-secure-control-hardening.md` | closed | M015 | Clean closure at `5bb1f81`; evidence in `plans/closure/M016-pre-release-correctness-and-secure-control-hardening-closure.md`. |
| M017 | `017-native-control-contract-and-operator-surface-consolidation.md` | closed | M016 | Closed at `58c4345`; evidence in `plans/closure/M017-native-control-contract-and-operator-surface-consolidation-closure.md`. Pinned Toxiproxy differential remains incomplete and is an explicit M019 gate. |
| M018 | `018-runtime-modularization-and-dependency-hygiene.md` | closed | M017 | Closed at `7e33d03`; evidence in `plans/closure/M018-runtime-modularization-and-dependency-hygiene-closure.md`. Runtime authority and public root exports preserved; six unused direct deps removed. |
| M019 | `019-qualification-expansion-and-final-corrective-requalification.md` | closed | M016, M017, M018 | Closed at `ca527db`; evidence in `plans/closure/M019-qualification-expansion-and-final-corrective-requalification-closure.md`. Final pre-tag gate passed on exact candidate across local/remote CI, pinned oracle, fuzz, Eggfetch, artifacts, security, package, and performance evidence. |
| M020 | `020-deterministic-datagram-fault-engine.md` | closed | M019 + ADR 003 | Closed on exact candidate `56c8925`; evidence in `plans/closure/M020-deterministic-datagram-fault-engine-closure.md`. |
| M021 | `021-fixed-target-udp-runtime-and-association-lifecycle.md` | closed | M020 | Closed on exact candidate `686838b`; evidence in `plans/closure/M021-fixed-target-udp-runtime-and-association-lifecycle-closure.md`. Local Tokio UDP ownership; no narrow published Eggress fixed-target seam. |
| M022 | `022-datagram-native-control-scenarios-cli-observability.md` | closed | M021 | Closed on exact candidate `8c4e3fb`; evidence in `plans/closure/M022-datagram-native-control-scenarios-cli-observability-closure.md`. |
| M023 | `023-datagram-qualification-performance-release-hardening.md` | closed | M022 | Closed cleanly on exact candidate `ae2ab733b2be199d7693e40cdc558df01ee9a9de`; evidence in `plans/closure/M023-datagram-qualification-performance-release-hardening-closure.md`. |
| M024 | `024-datagram-hot-path-performance-and-runtime-maintainability.md` | closed | M023 | Closed on exact candidate `ca46801`; evidence in `plans/closure/M024-datagram-hot-path-performance-and-runtime-maintainability-closure.md`. |
| M025 | `025-datagram-association-setup-waiter-and-closure-hygiene.md` | closed | M024 | Closed on exact candidate `55911f6`; evidence in `plans/closure/M025-datagram-association-setup-waiter-and-closure-hygiene-closure.md`. |
| M026 | `026-deterministic-scenario-schedule-model-and-compiler.md` | closed | M025 + ADR 004 | Closed on exact candidate `e0507d1`; evidence in `plans/closure/M026-deterministic-scenario-schedule-model-and-compiler-closure.md`. ScenarioScheduleV2 source language, deterministic compiler, SHA-256 fingerprint, and run_id-independent v2 namespace helper are frozen; wire DTOs round-trip JSON+ TOML to identical compiled tapes. |
| M027 | `027-scenario-schedule-runtime-control-and-lifecycle.md` | closed | M026 | Closed on exact candidate `5d85d15`; evidence in `plans/closure/M027-scenario-schedule-runtime-control-and-lifecycle-closure.md`. V2 schedules execute through the owned supervisor with epoch-anchored deadlines, strict/live ownership, CAS-safe cleanup, version-aware native routes, and CLI JSON/TOML support. |
| M028 | `028-deterministic-schedule-qualification-and-hardening.md` | closed | M027 | Closed cleanly on exact candidate `ceb3bae`; evidence in `plans/closure/M028-deterministic-schedule-qualification-and-hardening-closure.md`. Golden corpus frozen, paused-time/race/fuzz/security/API/CLI/regression/performance gates green; no new language feature. |

## Execution state

The earlier corrective implementation chain is complete:

`(M009 || M010) -> M011 -> M012 -> M013 -> M008`

Historical M002–M006 closure records remain preserved as evidence of the earlier implementation state. M009–M013 are the corrective successors that requalified that implementation, and M008 subsequently closed the first release-qualification milestone.

The historical pre-tag sequence through M015 is complete. M016 has now closed cleanly and activated the remaining release-blocking successor chain:

`M016 -> M017 -> M018 -> M019`

M015 qualified `cd88b22` and remains valid historical evidence. M016–M019 changed/requalified release-relevant code, so `cd88b22` is not the final candidate. M019 closed at `ca527db`; the owner may proceed with tagging, crates.io publication, and GitHub release creation as separate actions.

A post-release UDP/datagram tranche is registered and complete under ADR 003:

`M020 (closed) -> M021 (closed) -> M022 (closed) -> M023 (closed)`

This work does not rewrite M019 closure evidence and does not make datagram support part of the historical v0.1.0 qualification candidate. M020 closed on `56c8925`; M021 closed on `686838b`; M022 closed on `8c4e3fb`; M023 qualified `ae2ab733b2be199d7693e40cdc558df01ee9a9de`.

A bounded semantics-preserving performance/maintainability successor is now
complete:

`M024 (closed)`

M024 does not reopen M020–M023. It added topology-matched performance
measurement, scheduler/hot-path optimization where profiling justified it,
association-setup locking cleanup, and internal datagram runtime
modularization.

A narrow post-M024 concurrency/planning hygiene successor is complete:

`M025 (closed)`

M025 preserved ADR 003 and M024 semantics while replacing bounded
`yield_now()` polling from the `Starting` association path with a retained,
no-lost-wakeup event-driven transition. It proved setup/drain/capacity races
and reconciled planning-state language.

ADR 004's bounded post-release tranche is complete:

`M026 (closed) -> M027 (closed) -> M028 (closed)`

This work added a scenario-v2 schedule/compiler layer above the existing stream
and datagram policy publication machinery. M026 closed on `e0507d1` with
frozen compiler, fingerprint, and namespace semantics; M027 closed on
`5d85d15` with the runtime, control, lifecycle, evidence, and operator
surface implemented; M028 closed on `ceb3bae` with exact-candidate
qualification. ScenarioV1 remains a compatibility surface throughout.

## Post-release roadmap state

The completed UDP/datagram tranche is listed below; the remaining items are post-release or separately planned work and must not be pulled into another milestone implicitly.

| Area | State | Gate |
| --- | --- | --- |
| UDP/datagram impairment engine | completed / maintenance complete | ADR 003 semantics and M020–M025 feature/performance/hygiene work closed; follow-on datagram models require separate planning. |
| Optional `eggress-outbound` chained upstreams | future | M008 closed; prove demand without turning eggchaos into a second proxy framework. |
| eggreplay timing/fault integration | future | eggreplay stable flow model + M008. |
| eggprobe controlled impairment experiments | future | eggprobe stable diagnostics contract + M008. |
| Python/FFI bindings | future | stable Rust API after first release; no parallel networking implementation. |
| richer deterministic scenarios / time-varying schedule files | completed | ADR 004 tranche M026–M028 closed and qualified at `ceb3bae`; ScenarioV1 remains a compatibility surface. Follow-on schedule work requires separate planning. |
| current-Toxiproxy post-2.12 extensions such as stream-chunk `packet_loss` | future | M012 v2.12 parity requalified and M008 closed. |

## Dependency-ready view

Completed work: M000–M028 and M008 are closed.

Ready: none.

Active: none.

Blocked: none.

Historical pre-tag execution order: `M016 (closed) -> M017 (closed) -> M018 (closed) -> M019 (closed)`. The owner may proceed with the v0.1.0 tag, crates.io publication, and GitHub release as separate release actions.

Completed post-release feature execution order: `M020 (closed) -> M021 (closed) -> M022 (closed) -> M023 (closed)`.

Completed post-release performance/maintenance handoff: `M024 (closed)`.

Completed post-release concurrency/planning hygiene handoff: `M025 (closed)`. Richer datagram semantics still require a separate plan/ADR.

Closed scenario-v2 compiler foundation: `M026 (closed at e0507d1)`.

Closed scenario-v2 runtime/control handoff: `M027 (closed at 5d85d15)`.

Closed scenario-v2 qualification gate: `M028 (closed at ceb3bae)`.

Completed richer-scenario execution order: `ADR 004 -> M026 (closed) -> M027 (closed) -> M028 (closed)`. No successor is activated; later schedule work requires a separately registered numbered plan (and an ADR for semantic expansion).

## Closure requirements

A milestone becomes `closed` only when:

1. implementation is present on the target branch;
2. the plan's explicit tests/commands have been run on the exact candidate commit where practical;
3. required external/differential evidence is present rather than inferred;
4. documentation and registry state match the implementation;
5. unresolved medium-or-higher findings are either fixed or explicitly move the milestone back to `active`/`blocked`;
6. a closure note under `plans/closure/` identifies the candidate commit, evidence, limitations, and successor activation.

Do not use `closed` for “code written,” “tests likely pass,” or “source inspection looks complete.”
