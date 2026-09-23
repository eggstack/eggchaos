# Eggchaos Plan Registry

Last reconciled: 2026-09-23 (M015 closure)

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
| M015 | `015-final-exact-head-release-requalification.md` | closed | M014 | Clean verdict at `cd88b22`; evidence in `plans/closure/M015-final-exact-head-release-requalification-closure.md` (ordinary CI + dedicated release workflow green on the exact candidate, 5/5 artifacts with checksums, 47/47 Toxiproxy differential, Go/Python smokes, fuzz/security/package gates, perf within budget). Tag/publish/release remain owner decisions. |

## Execution state

The corrective implementation chain is complete:

`(M009 || M010) -> M011 -> M012 -> M013 -> M008`

Historical M002–M006 closure records remain preserved as evidence of the earlier implementation state. M009–M013 are the corrective successors that requalified that implementation, and M008 subsequently closed the first release-qualification milestone.

The pre-tag sequence is complete:

`M000 -> ... -> M015` (all closed)

M014 reconciled planning/release state and the post-M008 candidate lineage. M015 qualified the exact HEAD `cd88b22` with the dedicated release workflow and artifact matrix. Tagging, crates.io publication, and GitHub release creation remain explicit owner decisions.

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

Completed: M000–M015 and M008 are closed.

Ready now: none in the pre-tag sequence.

Blocked: none in the pre-tag sequence.

There is no further planned pre-tag implementation work; tagging/publishing/release creation remain explicit owner decisions.

## Closure requirements

A milestone becomes `closed` only when:

1. implementation is present on the target branch;
2. the plan's explicit tests/commands have been run on the exact candidate commit where practical;
3. required external/differential evidence is present rather than inferred;
4. documentation and registry state match the implementation;
5. unresolved medium-or-higher findings are either fixed or explicitly move the milestone back to `active`/`blocked`;
6. a closure note under `plans/closure/` identifies the candidate commit, evidence, limitations, and successor activation.

Do not use `closed` for “code written,” “tests likely pass,” or “source inspection looks complete.”
