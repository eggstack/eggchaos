# Eggchaos Plan Registry

Last reconciled: 2026-09-22

This file is the compact source of truth for active milestone state. Detailed scope lives in the numbered plans. Historical closure evidence belongs in `plans/closure/`.

| Milestone | Plan | Status | Depends on | Activation / closure note |
| --- | --- | --- | --- | --- |
| M000 | `000-architecture-and-scope-baseline.md` | closed | — | Initial investigation, architecture, references, and handoff sequence registered. |
| M001 | `001-workspace-bootstrap-and-core-contracts.md` | closed | M000 | Closed in `plans/closure/M001-workspace-bootstrap-and-core-contracts-closure.md` at `309aeff8da9b1d91b36aecd55c190e12d56e537d`. |
| M002 | `002-deterministic-stream-fault-engine.md` | active | M001 | Deterministic engine, bounded buffering, live policy foundations, and core evidence are implemented; qualification is in progress. |
| M003 | `003-fixed-target-proxy-runtime.md` | blocked | M002 | Requires proven directional fault engine and byte-preserving no-fault path. |
| M004 | `004-control-plane-cli-and-config.md` | blocked | M003 | Requires stable runtime handles/registry and lifecycle semantics. |
| M005 | `005-live-mutation-observability-and-scenarios.md` | blocked | M004 | Requires native API/control model to be stable enough for generation changes and active-connection control. |
| M006 | `006-toxiproxy-v2-12-compatibility.md` | blocked | M005 | Toxiproxy adapter must map onto a stable native engine, not define it. |
| M007 | `007-eggfetch-inprocess-integration.md` | blocked | M005 | Requires stable live policy handles so pooled physical connections can observe fault updates correctly. |
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
