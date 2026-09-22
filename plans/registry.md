# Eggchaos Plan Registry

Last reconciled: 2026-09-22

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
| M008 | `008-qualification-release-and-distribution.md` | blocked | M006, M007, M013 | Release qualification is paused until M013 requalifies the corrective implementation. The earlier blocked record also names package/target/performance/consumer evidence that remains M008 work after correctness is restored. |
| M009 | `009-core-fault-semantics-corrective.md` | closed | M002 historical implementation | Closed in `plans/closure/M009-core-fault-semantics-corrective-closure.md` at `8c1373e90129e68e96379fc3277fade8ce087abd`. |
| M010 | `010-runtime-control-authority-corrective.md` | closed | M003/M004 historical implementation | Closed in `plans/closure/M010-runtime-control-authority-corrective-closure.md` at `3961e968e98948cfab1d0c99d3503ba1624e2e6`. |
| M011 | `011-live-state-scenario-observability-corrective.md` | closed | M009, M010 | Closed in `plans/closure/M011-live-state-scenario-observability-corrective-closure.md`. |
| M012 | `012-toxiproxy-v2-12-parity-corrective.md` | active | M009, M010, M011 | M009, M010, M011 closed; implementation in progress, starting with oracle capture. |
| M013 | `013-corrective-requalification-gate.md` | blocked | M009, M010, M011, M012 | Cross-layer corrective qualification gate. Only a clean M013 verdict may reactivate M008. |

## Corrective execution order

The active corrective graph is:

`(M009 || M010) -> M011 -> M012 -> M013 -> M008`

M009 and M010 are independently ready and may be implemented in parallel. M011 is the first join point because live-state qualification depends on both correct fault semantics and a real runtime mutation authority.

Historical M002–M006 closure records are retained as evidence of the earlier implementation state. They are not rewritten or deleted. M009–M013 are corrective successors discovered by a later source/behavior audit.

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

Ready now:

- M009 — core fault semantics corrective.
- M010 — runtime/control authority corrective.

Blocked:

- M011 on M009 + M010.
- M012 on M009 + M010 + M011.
- M013 on M009–M012.
- M008 on M013, after which its remaining release/package/target/performance evidence can resume.

Do not hand M008 to an implementation/release agent while M013 is open.

## Closure requirements

A milestone becomes `closed` only when:

1. implementation is present on the target branch;
2. the plan's explicit tests/commands have been run on the exact candidate commit where practical;
3. required external/differential evidence is present rather than inferred;
4. documentation and registry state match the implementation;
5. unresolved medium-or-higher findings are either fixed or explicitly move the milestone back to `active`/`blocked`;
6. a closure note under `plans/closure/` identifies the candidate commit, evidence, limitations, and successor activation.

Do not use `closed` for “code written,” “tests likely pass,” or “source inspection looks complete.”
