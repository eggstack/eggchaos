# ADR 005 — Cross-Project Integration Boundary and Experiment Identity

Status: accepted  
Date: 2026-09-25

## Context

Eggchaos has completed the first deterministic stream, datagram, live-mutation,
and Scenario V2 tranches. The roadmap now calls for later integration with
EggReplay and EggProbe. Both sibling projects are active and have their own
authoritative semantic models:

- EggReplay owns semantic HTTP recording/replay, fixture identity, matching,
  stream-event timing, regression, and replay-server behavior.
- EggProbe owns diagnostic plans/reports, route truth, probe semantics,
  assertions, native diagnostics, and unsupported-capability reporting.
- EggFetch owns HTTP/TLS/pooling and its physical connection Dialer seam.
- Eggress owns outbound route/proxy-chain semantics.
- Eggchaos owns deterministic transport impairment, fault-policy publication,
  schedule identity, and fault evidence.

A direct dependency from eggchaos onto either consumer would invert this
ownership model and make eggchaos follow two rapidly changing product schemas.
Conversely, using only a standalone loopback proxy leaves important integration
information implicit: the consumer cannot reliably correlate its observation
with the exact physical connection identity, active policy generation, schedule
fingerprint, or injected fault evidence.

The existing `eggchaos-eggfetch::ChaosDialer` also owns direct DNS/TCP dialing.
That is too narrow for downstream consumers that already have a direct or
Eggress-backed Dialer. It prevents clean composition of:

    consumer
      -> EggFetch HTTP/TLS/pooling
      -> consumer-selected physical route Dialer
      -> Eggchaos impairment of the returned physical stream

Scenario V2 supplies stable compile/fingerprint/seed semantics, but its runtime
driver currently lives behind server ControlState. Cross-project tests need a
consumer-neutral way to coordinate an in-process workload with the same
monotonic schedule epoch without creating a second fault engine or teaching
eggchaos about HTTP flows or diagnostic probes.

## Decision 1: dependency direction stays consumer -> eggchaos

Eggchaos MUST NOT add production dependencies on `eggreplay-*` or
`eggprobe-*`.

Cross-project adoption is downstream work. Eggchaos provides stable,
consumer-neutral primitives and evidence. EggReplay and EggProbe decide how
those primitives appear in their own plans, reports, fixtures, CLIs, and
compatibility guarantees.

The intended dependency direction is:

    eggchaos-core
          ^
          |
    eggchaos integration / experiment primitives
          ^
          |
      +---+------------------+
      |                      |
    EggReplay              EggProbe
      |                      |
      +---- EggFetch/Eggress-+

No EggReplay flow type, .eggr artifact, ProbePlan, ProbeReport, assertion,
gRPC view, HTTP matcher, or native-probe type becomes an eggchaos public type.

## Decision 2: impairment decorates a physical route; it is not a route

An EggFetch integration MUST be able to wrap an arbitrary existing
`eggfetch_core::Dialer`. The inner Dialer remains authoritative for:

- destination resolution policy;
- direct versus Eggress/proxy routing;
- authentication;
- physical connect timeout;
- route-stage errors and metadata available through that Dialer.

Eggchaos begins only after a physical stream has been successfully returned by
the inner Dialer. It then applies directional transport impairment to that
stream.

This composition must not be represented as a new EggProbe-style route kind.
"Direct", "Eggress", and future datagram route choices describe where traffic
travels. Eggchaos describes conditions imposed on the selected transport.
Downstream consumers therefore keep route provenance and impairment provenance
as orthogonal dimensions.

The existing direct-dial convenience behavior MAY remain, but it must be
implemented as a convenience composition over the same decorator authority,
not as a second chaos implementation.

## Decision 3: physical connection is the deterministic stream unit

Stream fault realization is attached to a physical connection key.

The default integration may retain a monotonically assigned physical connection
ordinal for convenience, but callers MUST be able to provide a deterministic
connection-key source scoped to their experiment.

The connection-key source may depend on stable caller-supplied experiment
identity, target identity, and the physical connection ordinal. It MUST NOT
depend on Tokio task scheduling, wall-clock timestamps, random UUIDs, or
logical HTTP request order that cannot be observed at physical dial time.

Pooling and multiplexing remain transport facts. If multiple HTTP requests use
one HTTP/1 keep-alive connection or one multiplexed HTTP/2 connection, they
share one physical chaos realization. Eggchaos MUST NOT claim per-request
transport-fault isolation on a pooled connection.

A downstream product that requires one transport realization per logical test
case must configure its own connection/pooling policy accordingly.

## Decision 4: integration evidence is bounded and transport-level

The composable transport adapter must expose enough bounded evidence for a
consumer to correlate an observation with what eggchaos actually injected.

An integration evidence record or live handle must be able to identify, at
minimum:

- stable consumer-supplied experiment/integration identity if configured;
- physical connection ordinal and derived connection key;
- deterministic RNG/seed namespace version information needed to reproduce the
  realization;
- upstream and downstream observed policy generations;
- active fault identities/types, subject to the existing bounded fault count;
- accepted/forwarded/discarded byte counts and high-water evidence;
- injected/throttled delay evidence where already available;
- graceful/hard termination evidence;
- whether evidence is live, finalized, or incomplete due to abrupt owner drop.

No payload bytes, headers, URLs, credentials, request bodies, response bodies,
or arbitrary consumer annotations become required eggchaos evidence.

The adapter must not retain an unbounded global connection history. Prefer a
caller-supplied observer/sink receiving bounded shared evidence handles or
final snapshots. Any built-in registry must have an explicit hard capacity and
eviction policy.

## Decision 5: consumer-neutral experiment orchestration uses Scenario V2

Cross-project deterministic schedules must consume the existing Scenario V2
compiled/evidence model. They must not create an EggReplay scheduler, EggProbe
scheduler, or integration-only fault language inside eggchaos.

To avoid forcing consumers to depend on the full server/admin runtime, the
pure Scenario V2 semantics needed by both the server and an embedded experiment
harness should live behind a reusable consumer-neutral crate boundary. The
exact crate name may be selected by M030; a dedicated
`eggchaos-experiment`/equivalent leaf is preferred if it keeps dependencies
narrow.

The server may re-export existing public Scenario V2 symbols for source
compatibility after internal extraction.

The reusable experiment layer may define an abstract policy-publication target
that supports the already-proven operations required by Scenario V2:

- snapshot a named directional stream or datagram policy;
- expected-generation publication;
- capability reporting for unsupported transport/resource families;
- bounded cleanup through the same ownership semantics.

The standalone server adapts ControlState to this contract. An in-process
EggFetch integration may expose only stream resources. Unsupported datagram
actions must fail explicitly; they must never be ignored or translated into
stream behavior.

## Decision 6: one monotonic experiment epoch, not cross-process clock sync

A reusable harness must support preparing work before the schedule starts and
then releasing schedule execution and the consumer workload from one
in-process monotonic epoch.

The conceptual model is:

    parse/compile schedule
        -> prepare target snapshots/resources
        -> arm schedule driver + consumer workload
        -> capture one Tokio Instant epoch
        -> publish/release that epoch to both sides
        -> deadlines = epoch + compiled offset
        -> collect workload result + schedule/transport evidence
        -> cleanup

The exact API may be a start gate/barrier or equivalent. Both the schedule
driver and caller must observe the same captured monotonic epoch value.

This does NOT make OS/application traffic timing deterministic. The stable
contract remains deterministic policy/event identity and schedule deadlines.
Task wake-up latency, socket scheduling, server processing, and packet arrival
remain observations.

A remote HTTP control API cannot provide exact cross-process monotonic clock
synchronization and must not claim to. The existing native API remains useful
for operator-driven experiments, but M030's coordinated-start primitive is an
embedded harness capability.

## Decision 7: downstream experiment identity composes existing identities

Eggchaos does not define EggReplay fixture identity or EggProbe plan identity.
It defines a bounded integration identity that downstream products may include
in their own reports.

A reproducible experiment can be identified by the tuple or equivalent of:

    eggchaos semantics/tool version
    scenario compiler semantics version
    schedule fingerprint
    scenario seed
    execution_key
    caller integration identity
    connection-key derivation version/input identity
    physical connection evidence

A downstream report may add its own fixture/flow/plan/probe identity. Those
fields remain outside eggchaos's canonical schema.

## Consequences

Positive:

- EggReplay and EggProbe can evolve independently without dragging their
  product contracts into eggchaos.
- Direct, Eggress, and future routes can be impaired without duplicating their
  connect logic.
- deterministic connection identity becomes caller-controllable;
- transport evidence can explain observed regression/diagnostic failures;
- Scenario V2 remains the one deterministic schedule authority;
- embedded tests can share one monotonic experiment epoch;
- HTTP pooling/multiplexing semantics remain truthful.

Costs:

- the existing direct ChaosDialer needs a compatibility-preserving
  refactor/layering pass;
- BidirectionalChaosStream needs externally consumable evidence;
- Scenario V2 pure semantics may move behind a narrower reusable crate and
  require compatibility re-exports;
- the harness needs a generic policy publication adapter and explicit
  unsupported-capability handling;
- downstream integrations still require separate plans in their repositories.

## Rejected alternatives

### Add eggreplay and eggprobe adapter crates inside eggchaos

Rejected. That inverts dependency ownership, couples eggchaos releases to
consumer schema churn, and encourages eggchaos to understand semantic HTTP or
diagnostic concepts.

### Represent eggchaos as another outbound route

Rejected. Route and impairment are orthogonal. Collapsing them loses truthful
provenance for direct-versus-Eggress comparisons and future datagram routing.

### Keep ChaosDialer direct-only

Rejected. Downstream callers already have route-authoritative Dialers; forcing a
second direct dial path would duplicate DNS/connect behavior and make routed
experiments awkward or impossible.

### Assign per-request chaos keys inside EggFetch

Rejected. Eggchaos operates on physical streams. HTTP/2 multiplexing and
keep-alive make logical request identity different from physical connection
identity.

### Use the native HTTP API as the only experiment harness

Rejected. It is suitable for operator control but cannot share one process-local
Tokio monotonic epoch with a consumer workload. Cross-process wall-clock
coordination would create false determinism claims.

### Add a second schedule language for integrations

Rejected. Scenario V2 already provides bounded deterministic compilation,
fingerprinting, isolation, cleanup, and run evidence. Integration must compose
with that authority.
