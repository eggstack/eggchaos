# M039 — Post-v2.12 Stream-Loss Qualification and Closure

Status: blocked  
Depends on: M036, M037, M038  
Role: exact-candidate qualification and tranche closure

## Objective

Qualify the complete ADR 007 tranche on one exact candidate commit and close
the post-Toxiproxy-v2.12 stream-loss compatibility line only after deterministic
core semantics, native/cross-language propagation, strict v2.12 regression, and
the pinned post-v2.12 oracle all pass together.

M039 adds no intended product semantics. Any defect found here is corrected
under M039 and the entire affected gate is rerun on the final candidate.

## Scope

### In scope

- exact-head workspace quality gate;
- deterministic stream-loss golden/property corpus;
- native config/API/CLI/Scenario contract qualification;
- OpenAPI drift and Python/TypeScript client qualification;
- embed/Python-native conformance;
- strict pinned Toxiproxy v2.12 oracle;
- pinned post-v2.12 snapshot oracle;
- fuzz/property coverage for newly added parsing/state transitions;
- cross-platform stream/runtime tests on supported hosted OSes;
- no-fault and representative stream-loss performance measurements;
- dependency/security/package/release smoke;
- documentation/planning reconciliation;
- closure evidence and registry transition.

### Non-goals

- No new stream-loss feature.
- No configurable loss grain.
- No new Toxiproxy extension.
- No moving-main tracking.
- No UDP semantic change.
- No new language binding.
- No generic C ABI.
- No release/tag/publication action; those remain owner decisions unless
  separately requested.

## Exact-candidate rule

All mandatory local and hosted evidence must identify one final commit SHA.

If a correction changes code, tests, scripts, contract artifacts, or behavior
after a gate was run, rerun every gate whose evidence could be affected.
Historical M036-M038 closure candidates remain preserved; M039 records the
combined final authority.

A green local result is not a substitute for a required hosted/platform or
external-oracle result.

## Required qualification matrix

### Core determinism

Require:

- fixed 32 KiB logical-grain golden vectors;
- write-fragmentation equivalence property corpus;
- loss-rate 0/1 edge cases;
- correlation vectors;
- multiple-loss composition;
- existing seven activation-array fixtures unchanged;
- existing RNG/datagram/scenario golden fixtures unchanged;
- live generation transition behavior;
- accounting/bounds invariants.

### Native control and scenarios

Require:

- JSON and TOML round trips;
- native CRUD/patch/conflict;
- CLI human + JSON;
- Scenario V1;
- Scenario V2 compile/fingerprint/apply;
- connection/history evidence with additive loss fields;
- malformed/out-of-range input;
- auth/error redaction unchanged.

### Cross-language

Require:

- OpenAPI drift gate;
- Python sync + async contract/live qualification;
- TypeScript contract/live qualification;
- package artifact builds;
- safe `eggchaos-embed` conformance;
- hosted native-Python qualification on the M035-supported platforms;
- no handwritten unsafe regression.

### Toxiproxy strict v2.12

The existing mandatory pinned v2.12 oracle must pass unchanged.

Record:

- oracle version/hash;
- differential count/result;
- Go + independent client smoke;
- explicit proof that `packet_loss` remains rejected under strict profile.

### Pinned post-v2.12 snapshot

The exact source commit
`40f7fd31bee529d824116bd2a11a9e3425e904ec` must be fetched/verified/built.

Record:

- source/archive checksum;
- Go toolchain;
- built executable hash if reproducible/available;
- `/version` observation;
- packet_loss API corpus;
- edge data-plane corpus;
- statistical comparator parameters/results;
- client smoke;
- known divergences.

### Cross-platform

At minimum run the ordinary hosted Rust matrix used by current CI on:

- Linux;
- macOS;
- Windows.

Stream-loss deterministic unit tests must execute, not merely compile, on each
hosted OS.

Any platform-specific hard-reset behavior remains governed by existing
qualification and is not reinterpreted by stream loss.

### Performance

Add a topology-matched stream benchmark case for:

1. bare `eggress-relay`;
2. empty `ChaosStream`;
3. `stream-loss` with loss_rate=0;
4. representative mid-rate stream loss;
5. loss_rate=1 destructive fast path;
6. representative stream-loss + latency combination.

Measure at least throughput and CPU/allocations where the existing benchmark
harness supports them.

The primary regression requirement is that merely adding the new fault kind
does not materially regress empty/no-fault paths. Freeze a numeric budget only
from measured evidence; do not invent one in planning.

Performance differences while actively discarding bytes are diagnostic rather
than a semantic parity target against Toxiproxy.

### Fuzz/security

Extend existing fuzz targets as necessary so the new discriminated fault and
probability fields are exercised through:

- core plan JSON;
- native control JSON;
- native config TOML;
- evidence serialization;
- policy transitions;
- Toxiproxy toxic attributes.

Require existing security properties:

- no panics on hostile JSON/config;
- bounded request bodies/collections;
- no payload capture in evidence;
- admin auth/redaction unchanged;
- no shell execution from user-controlled fault input;
- no unsafe code added to normal Rust crates.

## Ordered work packages

### WP1 — Qualification inventory freeze

Before running final gates, enumerate all M036-M038 changed surfaces and map
each to a required test/script/host/oracle. Resolve stale docs or missing
coverage before declaring a candidate.

### WP2 — Local deterministic/contract gate

Run workspace, core, protocol, server, CLI, SDK-check, embed, native-Python,
Scenario, and golden tests on the exact candidate.

### WP3 — Dual Toxiproxy oracle gate

Run strict v2.12 and pinned post-v2.12 snapshot qualification back-to-back.
Store normalized artifacts with oracle identities.

### WP4 — Fuzz/security/package gate

Run bounded fuzz qualification, cargo audit/deny, package smoke, release smoke,
and artifact smoke as applicable to the current repository release process.

### WP5 — Performance gate

Capture raw stream benchmark output and compare no-fault/stream-loss paths to
the existing baseline methodology. Document hardware/runtime.

### WP6 — Hosted matrix

Push the exact candidate and require all mandatory Linux/macOS/Windows Rust and
language/native-Python jobs to conclude successfully.

### WP7 — Documentation/planning reconciliation

Update:

- `plans/registry.md`;
- `plans/README.md`;
- `plans/roadmap.md`;
- `AGENTS.md`;
- `docs/toxiproxy.md`;
- core/control/config docs;
- architecture deep dives;
- post-v2.12 reference matrix.

Preserve historical closure records.

### WP8 — Closure

Create M039 closure evidence containing exact SHA, command results, hosted run
IDs/URLs, oracle identities, performance artifacts, limitations, and the final
compatibility claim.

## Minimum verification commands

Use exact script names created by M036-M038; at minimum the final set must be
equivalent to:

```sh
./scripts/check.sh
./scripts/check_openapi.sh
./scripts/check_python_client.sh
./scripts/check_typescript_client.sh
./scripts/qualify_language_clients.sh
./scripts/check_python_native.sh
./scripts/qualify_python_native.sh
./scripts/qualify_eggfetch.sh

TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" \
  EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 \
  ./scripts/qualify_toxiproxy_v2_12.sh

TOXIPROXY_POST_V2_12_SERVER="$(./scripts/fetch_toxiproxy_post_v2_12.sh)" \
  EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1 \
  ./scripts/qualify_toxiproxy_post_v2_12.sh

EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh
./scripts/release-smoke.sh
./scripts/release-artifact-smoke.sh
```

Run the stream benchmark command selected/added by M039 and retain raw output.

Hosted CI must also run the repository's current cargo-audit and cargo-deny
jobs.

## Acceptance criteria

M039 closes only when:

- M036, M037, and M038 are formally closed;
- one exact candidate passes every mandatory gate;
- deterministic stream-loss traces are fragmentation-independent;
- legacy evidence/RNG/scenario/datagram fixtures have no unexplained changes;
- all native and cross-language surfaces agree on StreamLoss;
- strict Toxiproxy v2.12 still passes its pinned oracle unchanged;
- the snapshot profile passes its separately pinned oracle;
- compatibility docs clearly label packet_loss as userspace stream loss;
- live-update divergence is documented;
- no moving-main compatibility claim remains;
- no-fault performance remains within a measured accepted budget;
- hosted required jobs are green;
- planning/current-state documents agree;
- no unresolved medium-or-higher correctness/security finding remains;
- closure evidence identifies all incomplete/unclaimed platforms honestly.

Create
`plans/closure/M039-post-v2-12-stream-loss-qualification-and-closure-closure.md`.

## Stop/rejection conditions

Do not close if:

- any required oracle was unavailable or ran in non-mandatory/incomplete mode;
- local and hosted evidence refer to different candidate commits without an
  explicit rerun;
- strict v2.12 behavior broadened silently;
- packet_loss stochastic comparison relies on exact upstream RNG/chunk
  sequence;
- empty-path performance regresses beyond the measured accepted budget without
  explanation/correction;
- evidence arrays/golden fixtures changed without the corresponding ADR
  decision;
- documentation calls the feature real TCP/IP packet loss;
- a test failure is waived rather than corrected or explicitly moved to a
  successor plan.

## Follow-on activation

M039 activates no automatic successor.

After closure, a future tagged Toxiproxy release may justify a separate
promotion/reconciliation milestone. That work must compare the real release tag
against the pinned M038 snapshot before changing compatibility profile names or
claims.
