# Eggchaos Plan Registry

Last reconciled: 2026-09-23 (M017 closed; M018 activated)

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
| M018 | `018-runtime-modularization-and-dependency-hygiene.md` | active | M017 | Activated after M017. Decompose the monolithic server runtime without changing authority/semantics and remove unused direct dependencies. |
| M019 | `019-qualification-expansion-and-final-corrective-requalification.md` | blocked | M016, M017, M018 | Final pre-tag successor gate: strict pinned-oracle release qualification, expanded fuzz/differential evidence, full exact-HEAD CI/artifacts/security/package/performance requalification. |

## Execution state

The earlier corrective implementation chain is complete:

`(M009 || M010) -> M011 -> M012 -> M013 -> M008`

Historical M002–M006 closure records remain preserved as evidence of the earlier implementation state. M009–M013 are the corrective successors that requalified that implementation, and M008 subsequently closed the first release-qualification milestone.

The historical pre-tag sequence through M015 is complete. M016 has now closed cleanly and activated the remaining release-blocking successor chain:

`M016 -> M017 -> M018 -> M019`

M015 qualified `cd88b22` and remains valid historical evidence. Because M016–M018 will change release-relevant code and M019 strengthens/re-runs qualification, `cd88b22` is no longer the final tag candidate. Tagging, crates.io publication, and GitHub release creation are deferred until M019 closes.

## Future roadmap items not yet activated

These remain post-release or separately planned work and must not be pulled into the corrective sequence.

| Area | State | Gate |
| --- | --- | --- |
| UDP/datagram impairment engine | future | M008 closed; separate datagram semantics ADR required. |
| Optional `eggress-outbound` chained upstreams | future | M008 closed; prove demand without turning eggchaos into a second proxy framework. |
| eggreplay timing/fault integration | future | eggreplay stable flow model + M008. |
| eggprobe controlled impairment experiments | future | eggprobe stable diagnostics contract + M008. |
| Python/FFI bindings | future | stable Rust API after first release; no parallel networking implementation. |
| richer scenario scheduler / time-varying fault scripts | future | M011 corrected scenario model proven and M008 closed. |
| current-Toxiproxy post-2.12 extensions such as stream-chunk `packet_loss` | future | M012 v2.12 parity requalified and M008 closed. |

## Dependency-ready view

Completed historical work: M000–M015 and M008 are closed.

Active: M018.

Blocked: M019 on M016/M017/M018.

Current pre-tag execution order: `M016 (closed) -> M017 -> M018 -> M019`. Do not tag/publish/create the release before M019 closes.

## Closure requirements

A milestone becomes `closed` only when:

1. implementation is present on the target branch;
2. the plan's explicit tests/commands have been run on the exact candidate commit where practical;
3. required external/differential evidence is present rather than inferred;
4. documentation and registry state match the implementation;
5. unresolved medium-or-higher findings are either fixed or explicitly move the milestone back to `active`/`blocked`;
6. a closure note under `plans/closure/` identifies the candidate commit, evidence, limitations, and successor activation.

Do not use `closed` for “code written,” “tests likely pass,” or “source inspection looks complete.”
