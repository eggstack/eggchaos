# M031 — Integration Boundary Qualification and Downstream Handoff

Status: blocked  
Depends on: M030  
Role: exact-candidate qualification gate for the integration-boundary/harness tranche

## Objective

Qualify M029/M030 as one stable cross-project integration substrate on an exact
candidate commit. Prove that arbitrary EggFetch Dialers can be impaired without
route duplication, deterministic physical connection identity and evidence are
stable, the reusable Scenario V2 experiment harness preserves existing
semantics, and downstream consumers can adopt the public seams without
eggchaos depending on their product models.

M031 adds no EggReplay/EggProbe product feature. Its output is a closure-backed
eggchaos contract and an explicit downstream handoff describing what those
repositories may safely consume.

## Baseline and dependencies

M029 provides:

- arbitrary-inner-Dialer physical stream impairment;
- direct convenience compatibility;
- caller-controlled physical connection identity;
- bounded bidirectional stream evidence;
- an out-of-band evidence observer/sink;
- H1/H2/TLS/pooling ownership semantics.

M030 provides:

- a narrow consumer-neutral Scenario V2 semantic/execution boundary;
- compatibility-preserving server reuse/re-exports;
- an expected-generation policy target abstraction;
- ControlState and in-process stream target adapters;
- prepare/arm/start lifecycle;
- one shared process-local monotonic experiment epoch;
- bounded experiment evidence and owned cancellation/cleanup.

ADR 005 defines the architectural boundary. M031 must reject any implementation
that reaches closure only by importing EggReplay/EggProbe types, duplicating
route logic, weakening schedule semantics, or overstating timing determinism.

## Scope

### In scope

- One exact qualification candidate SHA.
- Public API/source-compatibility review for M029/M030.
- Dependency graph verification.
- Arbitrary-inner-Dialer conformance corpus.
- H1 keep-alive, H2 multiplexing, TLS handshake, cancellation, and live
  transition integration tests.
- Physical connection identity golden/conformance tests.
- Bidirectional evidence correctness and boundedness.
- Shared-epoch paused-time schedule corpus.
- Strict/live/restore/leave target conformance across server and embedded
  adapters.
- Unsupported-capability behavior.
- Existing Scenario V2 golden fingerprint/namespace corpus.
- Existing stream/datagram deterministic regressions.
- EggFetch integration regression.
- Pinned Toxiproxy v2.12 regression.
- Package/publish-order/release smoke for any new crate.
- Security/dependency checks.
- No-fault adapter overhead and evidence-observer overhead measurements.
- Documentation/planning reconciliation.
- A downstream handoff section for EggReplay and EggProbe describing stable
  seams, explicit non-guarantees, and remaining downstream work.

### Non-goals

- No modification of EggReplay or EggProbe repositories.
- No .eggr schema change.
- No ProbePlan/ProbeReport schema change.
- No Eggress route grammar change.
- No H3/QUIC integration.
- No datagram client-Dialer abstraction unless already independently available
  and required by M030 server conformance.
- No new fault or schedule language feature.
- No new operator daemon/API merely for qualification.
- No performance budget weakening to accommodate the new adapter.

## Dependency graph gate

The exact candidate must demonstrate a production dependency graph equivalent
to the ADR 005 direction.

Required checks:

- no `eggreplay-*` dependency anywhere in the eggchaos workspace;
- no `eggprobe-*` dependency anywhere in the eggchaos workspace;
- reusable experiment/harness crate does not depend on
  `eggchaos-server`, EggServe, CLI, or Toxiproxy;
- server may depend on the reusable experiment crate, not the reverse;
- EggFetch adapter remains a leaf over `eggchaos-core` plus
  `eggfetch-core` and narrowly required shared experiment types only if
  justified;
- any new dependency is pinned/feature-limited consistently with repository
  policy.

A test-only fixture crate may mimic downstream behavior, but it must not use
consumer product types.

## Dialer composition qualification

Build a deterministic fixture matrix around an instrumented inner Dialer.

Required cases:

- direct inner stream;
- delayed successful inner dial;
- authentication/rejected/timeout/connection errors;
- a synthetic multi-hop/routed Dialer that proves eggchaos does not resolve or
  redial the target;
- connection cancellation before inner dial completes;
- successful inner dial followed by chaos latency/bandwidth/disconnect;
- live policy update on an existing physical connection;
- inner stream half-close/shutdown behavior;
- evidence observer enabled/disabled.

Assert exactly one inner dial attempt unless the inner Dialer itself owns retry
behavior. Eggchaos must never add route retries.

## HTTP/TLS/pooling qualification

Through EggFetch, cover at minimum:

- HTTP/1.1 request/response with empty plan;
- HTTP/1.1 response latency;
- HTTP/1.1 close/limit behavior;
- TLS handshake under upstream/downstream impairment;
- HTTP/1.1 keep-alive reuse;
- forced separate H1 physical connections;
- HTTP/2 one-connection multiplexing;
- H2 live policy transition;
- body-stream failure/termination propagation;
- cancellation while a chaos delay is pending.

Freeze assertions around the physical connection key count rather than logical
request count.

## Deterministic connection identity corpus

Commit small golden/conformance fixtures for the selected public identity
contract.

Cover:

- default ordinal sequence;
- caller integration identity sensitivity;
- target sensitivity only if target is part of the documented derivation;
- stable derivation across repeated runs;
- explicit caller-selected collision behavior;
- no dependence on wall clock, UUID, task scheduling, or logical request ID;
- H1 reuse and H2 multiplexing retaining one physical key;
- separate physical dials selecting distinct default keys.

If the final API lets callers provide the key directly rather than derive it,
freeze the provider-input contract and adapter behavior instead of inventing an
extra hash.

## Evidence qualification

Verify live and finalized evidence against controlled byte/fault fixtures.

Required assertions:

- empty-plan byte counts are not lost on fast paths;
- upstream/downstream generation changes match publications;
- active fault lists are bounded and truncation is explicit;
- accepted/forwarded/discarded counts match preserving/loss semantics;
- injected/throttled delay totals are directionally correct;
- hard/graceful termination evidence is retained;
- observer sees exactly the successful wrapped dials;
- failed inner dials create no false evidence;
- evidence remains safely readable after physical stream drop;
- no payload, header, URL credential, or arbitrary consumer blob is retained;
- any collector supplied by eggchaos has tested hard capacity/eviction.

## Experiment harness qualification

Use Tokio paused time as primary authority.

Required cases:

- compile/prepare creates no publication;
- one shared start epoch is observed by caller and driver;
- 0-offset event applies after release, not before;
- sparse 1s/2s/3s deadlines do not accumulate drift;
- equal-deadline events preserve compiled order;
- already-due event behavior matches M027;
- strict conflict;
- live-mode current-state behavior;
- restore-initial success;
- restore conflict non-clobber;
- leave behavior;
- cancellation before release;
- cancellation while sleeping;
- cancellation immediately after publication;
- target failure;
- unsupported resource capability;
- service ControlState adapter and embedded stream target produce equivalent
  generation/event outcomes for the supported stream subset.

Existing M028 schedule corpus must pass unchanged.

## Cross-layer correlation fixture

Add at least one fully in-process fixture that demonstrates the intended
downstream contract without importing a downstream product.

Conceptually:

    custom workload
       -> EggFetch
       -> custom route-authoritative inner Dialer
       -> M029 chaos adapter
       -> local test service

    Scenario V2
       -> M030 prepared experiment
       -> shared epoch
       -> live policy publications

The final assertion must correlate:

- schedule fingerprint/seed/execution_key;
- event application/generation evidence;
- physical connection key;
- active fault evidence;
- workload-observed outcome.

The fixture proves correlation, not exact kernel/network timing replay.

## Performance qualification

M029 adds an adapter and evidence surface to a physical connection hot path.
Measure before/after or bare/decorated behavior with stable local harnesses.

At minimum measure:

1. bare/custom Dialer + EggFetch empty-plan throughput/latency;
2. M029 adapter with empty plan and observer disabled;
3. M029 adapter with bounded evidence observer enabled;
4. representative latency/bandwidth fault to ensure evidence accounting does
   not dominate configured behavior;
5. compile/prepare/start overhead for a small and 1024-event Scenario V2
   schedule.

Preserve the existing principle that empty-plan data movement should not incur
per-byte heap allocation or materially regress the qualified no-fault path.

Do not freeze a new numerical budget unless measurements are stable enough.
Any material regression needs profiling and a documented corrective before
closure.

## Security and robustness gate

Verify:

- no secret-bearing Dialer route expression is copied into eggchaos evidence;
- observer/debug output is bounded;
- target/integration labels have explicit length bounds if persisted;
- malicious/buggy connection-key providers cannot panic the process through
  ordinary error return paths;
- unsupported target capability is a bounded typed failure;
- no new shell/file/network control surface is added by the experiment crate;
- cancellation cannot leak tasks or retained policy ownership;
- dependency audit/deny gates remain clean.

If a provider is an in-process closure/trait object, panic behavior should be
documented; do not attempt unsafe panic recovery inside networking internals.

## Regression gates

On the exact candidate run at minimum:

    ./scripts/check.sh
    cargo audit --deny warnings
    cargo deny check advisories licenses bans sources
    ./scripts/qualify_eggfetch.sh
    TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 ./scripts/qualify_toxiproxy_v2_12.sh
    ./scripts/release-smoke.sh
    ./scripts/benchmark_datagram.sh

Also run:

- focused M029 H1/H2/TLS/pooling tests;
- focused M030 paused-time/target conformance tests;
- existing Scenario V2 golden corpus;
- existing stream/datagram exact trace tests;
- release-artifact smoke if package topology changed;
- fuzz/property targets affected by moved Scenario V2 types or new control
  input.

Hosted CI should cover the repository's normal supported OS matrix.

## Downstream handoff contract

M031 closure must include a concise handoff table.

For EggReplay, the stable eggchaos side should identify:

- the arbitrary-inner-Dialer wrapper;
- caller-controlled physical connection identity;
- evidence observer/snapshots;
- Scenario V2 compile/fingerprint identity;
- prepared shared-epoch experiment start;
- stream LivePolicy target adapter;
- pooling/multiplexing caveat;
- explicit statement that .eggr/semantic timing remain EggReplay-owned.

For EggProbe, identify:

- route-versus-impairment orthogonality;
- the arbitrary-inner-Dialer wrapper for HTTP/TLS paths;
- transport evidence correlation;
- Scenario V2 experiment identity/start;
- stream-only initial applicability;
- explicit unsupported status for DNS/route/ICMP/traceroute/PMTU and any
  not-yet-adopted UDP path;
- explicit statement that probe/report schemas remain EggProbe-owned.

The handoff must name only closure-backed public seams, not aspirational APIs.

## Ordered work packages

### WP1 — Freeze exact candidate and public API inventory

Record crate versions, public symbols, feature flags, dependency graph, and
source-compatibility/re-export decisions.

### WP2 — Dialer and HTTP/TLS matrix

Run the arbitrary-inner-Dialer and EggFetch H1/H2/TLS/pooling corpus.

### WP3 — Identity and evidence corpus

Freeze deterministic physical identity behavior and validate live/final
evidence bounds/correlation.

### WP4 — Harness paused-time/conformance matrix

Run shared-epoch, schedule timing, isolation, cleanup, cancellation, and
unsupported-capability tests across target adapters.

### WP5 — Cross-layer correlation fixture

Demonstrate one complete consumer-neutral experiment with schedule and
connection evidence tied to an observed workload outcome.

### WP6 — Performance/security/dependency gates

Run measurements, audits, hostile/bounds tests, and dependency direction
checks.

### WP7 — Existing product regressions

Run workspace, EggFetch, pinned Toxiproxy, stream/datagram, package, and release
smoke gates.

### WP8 — Documentation/planning reconciliation

Update README, docs/architecture.md, docs/eggfetch.md,
architecture/eggfetch-integration.md, architecture/scenario-observability.md,
architecture/verification-qualification.md, verification matrix, AGENTS.md,
roadmap, and registry to the exact qualified behavior.

### WP9 — Closure and downstream handoff

Create the closure record with exact SHA, clean/not-clean verdict, commands,
hosted evidence, performance data, API inventory, limitations, and the
EggReplay/EggProbe handoff table.

## Acceptance criteria

M031 closes only when:

- M029 and M030 are closed;
- one exact candidate is used for the qualification verdict;
- the dependency graph obeys ADR 005;
- arbitrary-inner-Dialer composition is proven without route duplication;
- physical connection identity is deterministic and pooling-aware;
- bidirectional evidence is bounded, correct, and payload-free;
- Scenario V2 golden fingerprints/namespaces remain unchanged;
- shared-epoch paused-time tests prove the embedded harness contract;
- server and embedded target adapters conform on supported stream semantics;
- unsupported capabilities fail explicitly;
- cancellation/cleanup leaves no untracked task or clobbered external state;
- cross-layer correlation is demonstrated in a consumer-neutral fixture;
- no-fault adapter overhead is measured and no unexplained material regression
  remains;
- security/dependency/package gates are clean;
- EggFetch and pinned Toxiproxy regressions pass;
- existing stream/datagram deterministic regressions remain green;
- documentation states that schedule/policy identity is deterministic while
  live traffic timing is observational;
- the closure record lists stable EggReplay/EggProbe handoff seams without
  modifying those repositories;
- no unresolved medium-or-higher correctness/security finding remains.

Create
`plans/closure/M031-integration-boundary-qualification-and-downstream-handoff-closure.md`.

## Stop/rejection conditions

Do not close if:

- EggReplay/EggProbe production dependencies appear in eggchaos;
- the experiment crate depends on the full server/admin stack;
- a routed inner Dialer is bypassed or redialed;
- H2 logical requests are represented as independent physical fault
  realizations;
- connection identity or schedule randomness depends on live scheduler order;
- evidence contains payloads/secrets or grows without bound;
- existing Scenario V2 golden identity changes without a separately justified
  semantics version change;
- embedded and server schedule drivers diverge;
- cross-process exact timing is claimed;
- a required pinned oracle/regression is skipped but still reported as clean;
- performance gates are weakened to make the tranche pass.

## Follow-on activation

A clean M031 closes the eggchaos integration-boundary/harness tranche.

Only then should downstream repositories register implementation milestones
against these closure-backed seams. EggReplay transport-chaos regression
integration is the preferred first downstream adopter. EggProbe controlled
impairment should follow against its stable diagnostic/native work, initially
for transport-bearing TLS/HTTP paths and only later for UDP when its own
datagram contract is closed.
