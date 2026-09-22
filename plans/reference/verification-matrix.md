# Verification Matrix

This file defines the evidence classes expected before M008 can close. Individual milestones may require subsets earlier.

## 1. Core invariant matrix

| Area | Required evidence |
| --- | --- |
| Empty plan | exact byte preservation, half-close preservation, no intentional sleeps/allocating queues after construction |
| Ordered faults | deterministic order tests; configuration round trip preserves order |
| Backpressure | bounded queue reaches cap, returns pending, wakes, and resumes without duplication/loss |
| Flush | all preserving accepted bytes reach inner writer before successful flush |
| Shutdown | preserving buffers resolve before close; slow-close delay honored; runtime cancellation can still terminate |
| Mutation | old generation owns already accepted bytes; new generation handles new bytes after barrier |
| Error | inner I/O errors retain direction/stage context and byte counts |
| Cancellation | no orphan tasks/timers/connection entries after shutdown |

## 2. Fault matrix

### Latency

Required:

- zero delay fast path;
- fixed delay with paused Tokio time;
- jitter boundaries and deterministic vector;
- multiple segments in flight without serial delay multiplication;
- bounded queue/backpressure;
- flush and shutdown while queue non-empty;
- configuration update while queue non-empty;
- throughput sanity under nonzero delay.

### Bandwidth

Required:

- configured sustained byte rate within tolerance;
- explicit burst behavior;
- zero/invalid rate validation;
- partial write behavior;
- rate change preserves defined token state;
- no wall-clock overflow on long duration;
- paused-time deterministic tests.

### Timeout / blackhole

Required:

- timeout=0 indefinite blackhole compatibility behavior;
- finite close-after timeout;
- removal/update unblocks according to generation rules;
- discarded byte accounting;
- service shutdown interrupts an indefinite blackhole.

### Limit data

Required:

- exact N-byte pass;
- boundary inside a caller buffer;
- N=0;
- both directions independently;
- interaction with upstream/downstream close;
- update/state behavior explicitly tested.

### Slow close

Required:

- close delayed, ordinary data not delayed solely by this fault;
- multiple shutdown polls;
- cancellation;
- zero delay;
- half-close interaction.

### Slice

Required:

- exact byte preservation;
- average/variation bounds;
- invalid variation rejected;
- deterministic slice sequence from seed;
- per-slice delay under paused time;
- large caller buffer and tiny slice;
- flush/shutdown after partial slice progress.

### Disconnect/reset

Required:

- graceful disconnect semantics exact;
- reset capability truthful;
- Linux/macOS/Windows platform cases where CI supports them;
- unsupported erased/non-TCP stream returns capability result instead of pretending success;
- timeout-triggered and operator-triggered termination.

### Probability

Required:

- 0 never activates;
- 1 always activates;
- stable per-connection decision golden vectors;
- fault-local random streams unaffected by unrelated concurrent faults;
- recorded evidence sufficient to reproduce.

## 3. Relay/runtime matrix

Use `eggress-testkit` fixtures where suitable.

Required scenarios:

- echo;
- request half-close then response;
- target half-close;
- fragmented writes;
- slow reader/writer;
- upstream connection refusal;
- upstream connect timeout where controllable;
- client disconnect before upstream completes;
- simultaneous traffic both directions;
- max connection admission/rejection;
- graceful service drain;
- forced shutdown deadline;
- multiple independent proxy listeners;
- ephemeral listen port;
- bind conflict;
- DNS/hostname upstream and IP literal upstream;
- IPv4 and IPv6 where CI supports both.

Every session result should expose enough data to classify normal close, first-side close, drain timeout, injected termination, connect failure, and I/O failure.

## 4. Control/API matrix

Native API:

- exact JSON success schemas;
- stable machine error envelope;
- invalid JSON/body-too-large;
- invalid duration/rate/buffer/probability;
- unknown proxy/fault/connection;
- duplicate create;
- optimistic generation conflict if supported;
- concurrent reads during mutation;
- reset;
- health/readiness;
- auth behavior;
- loopback default;
- explicit non-loopback opt-in.

CLI:

- exit codes;
- `--json` valid JSON on success and failure;
- human output smoke tests;
- no ANSI in JSON;
- config parse errors;
- admin unavailable;
- auth redaction;
- shell completion generation only if added deliberately.

## 5. Toxiproxy differential matrix

Use official v2.12.0 binary/container as oracle.

Minimum route coverage is listed in `toxiproxy-parity.md`.

For each toxic:

- API default fields;
- downstream behavior;
- upstream behavior;
- add/update/remove with an existing connection;
- add/update/remove before connection;
- toxicity 0 and 1;
- representative nontrivial attribute values.

Comparators:

- JSON fields/status codes exact where compatibility requires;
- bytes exact for preserving/limit faults;
- time windows tolerance-based;
- throughput tolerance-based;
- reset transport outcome platform-qualified.

Store corpus input and normalized observations as machine-readable fixtures.

## 6. Eggfetch integration matrix

Through `eggchaos-eggfetch`:

- HTTP/1.1 direct request no fault;
- HTTPS with Eggfetch-owned TLS/SNI;
- latency causing read/total timeout behavior;
- bandwidth throttling a streaming response;
- mid-response disconnect;
- blackhole and retry policy;
- H1 keep-alive reuse;
- H2 multiplexing over one physical fault-wrapped connection;
- live fault update on an already pooled physical connection;
- new connection after generation change;
- safe `DialErrorKind` mapping for physical dial failures;
- no URL/header/credential leakage into error/debug output.

The adapter must not terminate TLS itself for normal HTTPS; Eggfetch remains the TLS authority above the raw dialed stream.

## 7. Determinism/replay matrix

Commit golden fixtures for:

- RNG seed derivation;
- Bernoulli decisions;
- latency jitter values;
- slice sizes;
- scenario event ordering;
- evidence serialization.

Run the same fixture multiple times with unrelated task scheduling noise and verify the relevant fault decisions remain identical.

## 8. Performance matrix

Measure on at least one stable developer/CI class before freezing budgets.

Benchmarks:

- bare `eggress-relay`;
- empty `ChaosStream`;
- latency fault;
- bandwidth fault;
- slice fault;
- combined representative plan;
- connection setup/teardown;
- admin read-heavy operations separated from data plane.

Record:

- throughput;
- CPU where practical;
- allocations/peak queued bytes where tooling permits;
- p50/p95/p99 added latency;
- binary size;
- idle memory;
- connection high-water.

The initial roadmap intentionally does not invent a no-fault overhead percentage. M008 sets a budget from measured evidence and records the baseline hardware/runtime.

## 9. Robustness and fuzzing

Fuzz/property targets should cover:

- config/JSON/TOML parser values;
- ordered fault-plan validation;
- duration/rate arithmetic;
- partial writes and arbitrary poll fragmentation;
- fault transition sequences;
- evidence serialization;
- compatibility JSON attribute maps.

Property tests should emphasize byte conservation for preserving fault combinations.

## 10. Cross-platform matrix

Initial binary support target:

- Linux x86_64;
- Linux aarch64;
- macOS x86_64 where runner availability permits;
- macOS aarch64;
- Windows x86_64.

Rust library correctness should not assume Unix-only socket behavior. Any hard-reset feature with platform differences gets a capability result and explicit per-platform evidence.

SBC target-class benchmarking is a post-first-release follow-up unless readily available during M008.

## 11. Security/operational matrix

Verify:

- admin loopback default;
- non-loopback admin rejected without explicit enable/auth policy;
- bearer/admin secret redaction;
- bounded request bodies;
- bounded connection/fault/proxy counts;
- bounded buffered bytes;
- no path traversal/filesystem serving in admin plane;
- no shell/process execution;
- dependency audit;
- malformed/hostile JSON cannot panic;
- shutdown cannot leave public listener tasks detached.

## 12. Closure evidence format

Each milestone closure note should record:

```text
Milestone:
Candidate commit:
Implementation commits:
Commands executed:
Platforms:
External oracle/version:
Evidence artifacts:
Known limitations:
Acceptance criteria verdict:
Registry transition:
Next milestone activated:
```

If a required command/oracle was not run, say so explicitly and keep the milestone out of `closed` when that evidence is a closure gate.
