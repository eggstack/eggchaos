# Eggchaos Plan Registry

Last reconciled: 2026-09-26 (M040 implementation is undergoing exact-candidate local/hosted qualification)

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
| M029 | `029-composable-transport-chaos-adapter-and-evidence.md` | closed | M028 + ADR 005 | Closed on exact candidate `add40b0`; evidence in `plans/closure/M029-composable-transport-chaos-adapter-and-evidence-closure.md`. Arbitrary-inner EggFetch Dialer composition, caller-controlled physical connection identity, and bounded bidirectional evidence. No EggReplay/EggProbe dependency. |
| M030 | `030-consumer-neutral-experiment-harness-and-coordinated-start.md` | closed | M029 | Closed on exact candidate `0bc45f0`; evidence in `plans/closure/M030-consumer-neutral-experiment-harness-and-coordinated-start-closure.md`. Consumer-neutral Scenario V2 semantic/execution boundary in `eggchaos-experiment`, expected-generation target adapters, shared Tokio monotonic start epoch. |
| M031 | `031-integration-boundary-qualification-and-downstream-handoff.md` | closed | M030 | Closed on exact candidate `fa189b9`; evidence in `plans/closure/M031-integration-boundary-qualification-and-downstream-handoff-closure.md`. Cross-project substrate qualified with downstream handoff table; no EggReplay/EggProbe product work. |
| M032 | `032-native-protocol-contract-extraction-and-openapi-foundation.md` | **closed** | M031 + ADR 006 | Closed on exact candidate `ed05f68`; evidence in `plans/closure/M032-native-protocol-contract-extraction-and-openapi-foundation-closure.md`. `eggchaos-protocol` owns wire DTOs + `NATIVE_OPERATIONS`; OpenAPI drift-checked; server behavior preserved. |
| M033 | `033-python-and-typescript-native-control-sdks.md` | **closed** | M032 | Closed on exact candidate `429d459`; evidence in `plans/closure/M033-python-and-typescript-native-control-sdks-closure.md`. Stdlib-only Python sync/async + zero-dep TypeScript clients over the exact M032 contract; drift-checked derivation, live cross-language qualification, package artifacts built. |
| M034 | `034-python-native-embedding-pilot-and-binding-qualification.md` | **closed** | M033 | Closed on exact candidate `991818b`; evidence in `plans/closure/M034-python-native-embedding-pilot-and-binding-qualification-closure.md`. Safe `eggchaos-embed` facade + PyO3/maturin `eggchaos-native` pilot (abi3), remote/native conformance green, C ABI decided no-go. |
| M035 | `035-cross-language-qualification-and-closure-corrective.md` | **closed** | M034 | Closed on exact candidate `a710cd6`; evidence in `plans/closure/M035-cross-language-qualification-and-closure-corrective-closure.md`. Hosted SDK false-red fixed, hosted native-Python gate green, single datagram fault mutation authority, planning reconciled. Activates no successor. |
| M036 | `036-deterministic-stream-loss-core-and-evidence.md` | **closed** | M035 + ADR 007 | Deterministic 32 KiB logical-grain stream-loss primitive, burst correlation, fragmentation-independent decisions, additive evidence; strict v2.12/native control unchanged in this milestone. |
| M037 | `037-stream-loss-native-contract-and-cross-language-propagation.md` | **closed** | M036 | Propagate proven StreamLoss semantics through native v1/config/CLI/Scenario/OpenAPI/Python/TypeScript/embed/Python-native surfaces without redefining core behavior. |
| M038 | `038-pinned-post-v2-12-toxiproxy-packet-loss-profile.md` | **closed** | M037 | Add opt-in pinned post-v2.12 snapshot profile for upstream `packet_loss` at `40f7fd31`; strict v2.12 remains default/frozen. |
| M039 | `039-post-v2-12-stream-loss-qualification-and-closure.md` | **closed** | M036, M037, M038 | Exact-candidate dual-oracle/native/cross-language/fuzz/performance/hosted qualification and tranche closure. |
| M040 | `040-post-v2-12-stream-loss-corrective-requalification.md` | **active** | M039 historical closure + ADR 007 | Corrective implementation and local qualification are in progress; exact-head hosted Linux/macOS/Windows and language/native-Python evidence remain required before closure. |

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

ADR 005 now activates the next post-release integration-boundary/harness chain:

`M029 (closed) -> M030 (closed) -> M031 (closed)`

M029 closed on `add40b0`, making the EggFetch physical-stream chaos
adapter composable over arbitrary Dialers with caller-controlled
physical connection identity and bounded bidirectional evidence.
M030 closed on `0bc45f0`, adding the consumer-neutral Scenario V2
experiment harness/shared monotonic start epoch behind
`eggchaos-experiment`. M031 closed on exact candidate `fa189b9`,
qualifying the tranche with the downstream handoff table.
EggReplay/EggProbe product adapters remain downstream work and are not
part of this chain.

ADR 006's cross-language contract/binding feature chain is complete:

`M032 (closed at ed05f68) -> M033 (closed at 429d459) -> M034 (closed at 991818b)`

A post-closure audit found bounded qualification/hygiene debt rather than a new
binding feature: hosted remote-SDK qualification prints pass but exits 143
during expected child-process cleanup; native Python is not yet protected by a
portable hosted gate; datagram fault mutation semantics are duplicated between
HTTP and embed; and current-state planning text drifted. M035 closed on
`a710cd6` as the corrective successor (hosted 13/13 matrix green, single
datagram authority, planning reconciled). It does not reopen or rewrite
M032–M034 historical closures. A generic C ABI, Node native addon, JNI,
P/Invoke, cgo, UniFFI, and WASM remain unactivated.

## Post-release roadmap state

The completed UDP/datagram tranche is listed below; the remaining items are post-release or separately planned work and must not be pulled into another milestone implicitly.

| Area | State | Gate |
| --- | --- | --- |
| UDP/datagram impairment engine | completed / maintenance complete | ADR 003 semantics and M020–M025 feature/performance/hygiene work closed; follow-on datagram models require separate planning. |
| Optional `eggress-outbound` chained upstreams | future | M008 closed; prove demand without turning eggchaos into a second proxy framework. |
| cross-project integration boundary / experiment harness | completed | ADR 005 tranche M029–M031 closed and qualified at `fa189b9`; follow-on substrate work requires separate planning. |
| eggreplay timing/fault integration | ready downstream | M031 closure + EggReplay-owned adoption plan; `.eggr`/semantic timing remain EggReplay authority. |
| eggprobe controlled impairment experiments | ready downstream | M031 closure + EggProbe-owned adoption plan; route/probe/report semantics remain EggProbe authority. |
| cross-language control SDKs / native Python embedding | implemented and correctively qualified | ADR 006 chain M032–M034 plus corrective M035 (closed at `a710cd6`) reconciled hosted SDK/native-Python qualification, datagram mutation authority, and planning closure. Generic C ABI remains no-go pending separate ADR + demand. |
| richer deterministic scenarios / time-varying schedule files | completed | ADR 004 tranche M026–M028 closed and qualified at `ceb3bae`; ScenarioV1 remains a compatibility surface. Follow-on schedule work requires separate planning. |
| current-Toxiproxy post-2.12 stream-chunk `packet_loss` extension | implemented / corrective requalification ready | ADR 007 implementation chain M036–M039 is historical; post-M039 audit found snapshot-profile propagation, false-positive/underpowered data-plane qualification, metrics, oracle-toolchain, hosted-exact-head, and closure-evidence defects. M040 is the corrective authority. Strict v2.12 remains frozen/default. |

## Dependency-ready view

Completed work: M000–M039 and M008 are historical closed work.

Active: M040.

Ready: none.

Blocked: none.

Historical pre-tag execution order: `M016 (closed) -> M017 (closed) -> M018 (closed) -> M019 (closed)`. The owner may proceed with the v0.1.0 tag, crates.io publication, and GitHub release as separate release actions.

Completed post-release feature execution order: `M020 (closed) -> M021 (closed) -> M022 (closed) -> M023 (closed)`.

Completed post-release performance/maintenance handoff: `M024 (closed)`.

Completed post-release concurrency/planning hygiene handoff: `M025 (closed)`. Richer datagram semantics still require a separate plan/ADR.

Closed scenario-v2 compiler foundation: `M026 (closed at e0507d1)`.

Closed scenario-v2 runtime/control handoff: `M027 (closed at 5d85d15)`.

Closed scenario-v2 qualification gate: `M028 (closed at ceb3bae)`.

Completed richer-scenario execution order: `ADR 004 -> M026 (closed) -> M027 (closed) -> M028 (closed)`.

Completed integration-boundary execution order: `ADR 005 -> M029 (closed at add40b0) -> M030 (closed at 0bc45f0) -> M031 (closed at fa189b9)`. Downstream EggReplay/EggProbe product work may now register implementation milestones against the closure-backed seams in the M031 handoff table.

Completed cross-language execution order: `ADR 006 -> M032 (closed at ed05f68) -> M033 (closed at 429d459) -> M034 (closed at 991818b)`. The chain activates no automatic successor; a generic C ABI needs a separate ADR plus a second concrete consumer (M034 decided no-go).

Corrective qualification successor: `M035 (closed at a710cd6)`. M035 obtained a green exact-head hosted matrix (13/13 jobs) and reconciled planning without rewriting M032–M034 closure evidence. It activates no successor.

Historical post-release stream-loss implementation order: `ADR 007 -> M036 (closed) -> M037 (closed) -> M038 (closed) -> M039 (closed)`.

A post-M039 audit found bounded corrective debt in the compatibility/qualification/closure layer rather than a redesign need. M040 is therefore the sole active successor:

`M036 (historical) -> M037 (historical) -> M038 (historical) -> M039 (historical closure record) -> M040 (active corrective)`

M036–M037's core/native implementation remains the baseline. M040 owns the discovered snapshot-profile `populate`/conversion inconsistencies, the non-isolated and unconditional post-v2.12 data-plane “passes,” missing intermediate/correlation statistical comparators, missing named stream-loss Prometheus metrics, ambient/unrecorded Go oracle toolchain, missing exact-head hosted rerun, and current closure/planning drift. M040 closure supersedes M039 as the final repository-level authority for ADR 007 without rewriting historical evidence.

## Closure requirements

A milestone becomes `closed` only when:

1. implementation is present on the target branch;
2. the plan's explicit tests/commands have been run on the exact candidate commit where practical;
3. required external/differential evidence is present rather than inferred;
4. documentation and registry state match the implementation;
5. unresolved medium-or-higher findings are either fixed or explicitly move the milestone back to `active`/`blocked`;
6. a closure note under `plans/closure/` identifies the candidate commit, evidence, limitations, and successor activation.

Do not use `closed` for “code written,” “tests likely pass,” or “source inspection looks complete.”
