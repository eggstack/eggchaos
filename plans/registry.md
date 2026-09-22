# Eggchaos Plan Registry

Last reconciled: 2026-09-22

This file is the compact source of truth for active milestone state. Detailed scope lives in the numbered plans. Historical closure evidence belongs in `plans/closure/`.

| Milestone | Plan | Status | Depends on | Activation / closure note |
| --- | --- | --- | --- | --- |
| M000 | `000-architecture-and-scope-baseline.md` | closed | — | Initial investigation, architecture, references, and handoff sequence registered. |
| M001 | `001-workspace-bootstrap-and-core-contracts.md` | closed | M000 | Closed in `plans/closure/M001-workspace-bootstrap-and-core-contracts-closure.md` at `309aeff8da9b1d91b36aecd55c190e12d56e537d`. |
| M002 | `002-deterministic-stream-fault-engine.md` | closed | M001 | Closed in `plans/closure/M002-deterministic-stream-fault-engine-closure.md` at `d85d25402af4f7c63a45ebb4ebc73b8de727d2e5`. |
| M003 | `003-fixed-target-proxy-runtime.md` | closed | M002 | Closed in `plans/closure/M003-fixed-target-proxy-runtime-closure.md` at `5dd41027948e7413b595c77f11d7a9e0b31f3785`. |
| M004 | `004-control-plane-cli-and-config.md` | closed | M003 | Closed in `plans/closure/M004-control-plane-cli-and-config-closure.md` at `14791042aad47dc11d57f84677dcb69ef055d690`. |
| M005 | `005-live-mutation-observability-and-scenarios.md` | closed | M004 | Closed in `plans/closure/M005-live-mutation-observability-and-scenarios-closure.md` at `250494fc7d408c3f86933f44437d454864519984`. |
| M006 | `006-toxiproxy-v2-12-compatibility.md` | ready | M005 | Native generation/live-policy, connection control, evidence snapshots, metrics, and scenario v1 are closed. |
| M007 | `007-eggfetch-inprocess-integration.md` | ready | M005 | Native generation/live-policy handles and physical connection identity are closed. |
| M008 | `008-qualification-release-and-distribution.md` | blocked | M006, M007 | First release gate: compatibility, integration, cross-platform, performance, packaging, and evidence. |

## Future roadmap items not yet activated

These are deliberately not assigned executable milestone files yet. They require post-M008 evidence and a new planning pass.

| Area | State | Gate |
| --- | --- | --- |
| UDP/datagram impairment engine | future | M008 closed; separate datagram semantics ADR required. |
| Optional `eggress-outbound` chained upstreams | future | M008 closed; prove demand without turning eggchaos into a second proxy framework. |
| eggreplay timing/fault integration | future | eggreplay stable flow model + M008. |
| eggprobe controlled impairment experiments | future | eggprobe stable diagnostics contract + M008. |
| Python/FFI bindings | future | stable Rust API after first release; no parallel networking implementation. |
| richer scenario scheduler / time-varying fault scripts | future | M005 scenario model proven in real tests. |
| current-Toxiproxy post-2.12 extensions such as stream-chunk `packet_loss` | future | M006 v2.12 parity closed; semantics documented as chunk loss, not real packet loss. |

## Dependency-ready view

Only M001 is ready for handoff.

When M001 closes, update this registry so M002 becomes `ready`; do not pre-mark later plans ready. The same rule applies transitively.

## Closure requirements

A milestone becomes `closed` only when:

1. implementation is present on the target branch;
2. the plan's explicit tests/commands have been run on the exact candidate commit where practical;
3. required external/differential evidence is present rather than inferred;
4. documentation and registry state match the implementation;
5. unresolved medium-or-higher findings are either fixed or explicitly move the milestone back to `active`/`blocked`;
6. a closure note under `plans/closure/` identifies the candidate commit, evidence, limitations, and successor activation.

Do not use `closed` for “code written,” “tests likely pass,” or “source inspection looks complete.”
