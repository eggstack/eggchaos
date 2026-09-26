# M040 — Post-v2.12 Stream-Loss Corrective Requalification

Status: closed  
Depends on: M039 (historical closure record), ADR 007  
Role: post-ADR-007 correctness, qualification, and planning closure corrective  
Baseline: `3b405af7684a681ffcf73709d6b6c90b10267a9a`  
Closed implementation candidate: `48fe0dd8dfc1f995c53a3b1661fe704dbdc5bce0`; see `plans/closure/M040-post-v2-12-stream-loss-corrective-requalification-closure.md`. M041 is the post-closure corrective successor.

## Objective

Correct the bounded defects found after the M036–M039 stream-loss /
post-v2.12 Toxiproxy implementation tranche, then produce a truthful
exact-candidate closure without reopening or redesigning the core ADR 007
semantics.

M040 must:

1. fix compatibility-profile propagation defects in
   `eggchaos-toxiproxy`;
2. make the post-v2.12 data-plane differential capable of failing when
   `packet_loss` behavior is wrong, with isolated exact edge cases and
   predeclared stochastic/correlation comparators;
3. finish the stream-loss metrics propagation promised by M036 without
   changing the frozen seven-slot activation arrays;
4. make the pinned post-v2.12 oracle toolchain identity reproducible and
   recorded rather than relying on an ambient unreported Go compiler;
5. rerun the exact-head hosted Linux/macOS/Windows and language/native-Python
   qualification required by M039 instead of inheriting an older matrix;
6. repair current-state planning/documentation drift and supersede the
   incomplete M039 closure claim with one new authoritative M040 closure.

This is a corrective successor. M036–M039 remain historical records of the
implementation sequence and the evidence that existed at those commits.
M040 must not erase that history. Its closure becomes the repository-level
authority for the corrected ADR 007 tranche.

## Baseline and observed defects

Current implementation head at registration:
`3b405af7684a681ffcf73709d6b6c90b10267a9a`.

The core and native propagation are substantially implemented and should be
preserved. The corrective work is concentrated in compatibility translation,
qualification, metrics, and closure evidence.

### 1. `populate` can silently fall back to the strict profile

`ToxiproxyAdapter::populate_entry()` currently hard-codes
`CompatProfile::StrictV2_12` when:

- an existing proxy has the same listen/upstream pair; and
- an enabled proxy is created/recreated successfully.

The disabled-import path, by contrast, routes back through
`self.proxy_json()` and therefore uses the adapter's active profile.

Under `post-v2.12-2026-09-25`, a keep-style `POST /populate` for a proxy
that already contains a native `StreamLoss` / compatibility
`packet_loss` toxic can therefore render the proxy through the strict
profile and produce an invalid toxic representation.

M040 must thread `self.profile` through every populate response path and add
a regression using an existing snapshot-profile proxy containing
`packet_loss`.

### 2. `Toxic::to_fault_with_profile` rejects `packet_loss` before the
profile-aware translator

The public `to_fault_with_profile()` helper advertises profile-aware
translation, but its preliminary whitelist still contains only the seven
v2.12 toxic names. A `packet_loss` toxic is rejected before
`kind_from_attrs(..., profile)` can accept it.

HTTP toxic CRUD happens to use a separate path, so the snapshot server works
for common requests while this public conversion API is inconsistent with its
documented contract.

M040 must:

- make `to_fault_with_profile(PostV2_12_2026_09_25)` accept
  `packet_loss`;
- keep `to_fault()` strict-v2.12 by default;
- keep strict profile rejection exact;
- audit `translate_proxy()` and either:
  - keep it explicitly strict and add `translate_proxy_with_profile()`; or
  - thread a profile through the existing API only if that can be done
    without a public compatibility break.

No compatibility helper may have a hidden seven-toxic whitelist that bypasses
the active profile.

### 3. The post-v2.12 data-plane differential has false-positive passes

`crates/eggchaos-toxiproxy/tests/post_v212_differential.rs` currently
records the byte counts for the `loss_rate=0` and `loss_rate=1` cases and
then increments `corpus.passed` unconditionally.

That means neither case fails when data-plane behavior is wrong.

The same test also accumulates several active `packet_loss` toxics on one
proxy before the data-plane probes, including a `loss_rate=1` toxic.
Consequently the later `loss_rate=0` observation is not an isolated
zero-loss experiment and cannot prove preservation.

M040 must replace these observations with isolated fixtures and executable
assertions.

### 4. The advertised stochastic/correlation qualification is absent

M038/M039 closure text says the post-v2.12 corpus contains
intent-compatible stochastic comparison, but the committed differential has:

- no intermediate `loss_rate` sample;
- no conditional/burst correlation sample;
- no predeclared sample size/tolerance for either behavior.

The current 14/14 number therefore overstates what was actually qualified.

M040 must add bounded, predeclared statistical tests. They must not compare
exact upstream RNG draws or incidental Go `StreamChunk` boundaries.

### 5. Stream-loss metrics propagation is incomplete

M036 added named stream-loss evidence counters while deliberately freezing
`activations: [u64; 7]`. Connection/history evidence carries the new
counters, but `crates/eggchaos-server/src/runtime/metrics.rs` still records
only the seven legacy activation slots and has no named stream-loss diagnostic
metrics.

M040 must complete the intended metrics propagation without:

- adding an eighth legacy activation slot;
- changing `FAULT_TYPE_NAMES`;
- introducing unbounded/high-cardinality labels.

At minimum Prometheus output must expose bounded counters for:

- logical stream-loss chunks evaluated;
- logical chunks dropped;
- bytes discarded by stream loss.

Per-proxy + direction labels may reuse the existing bounded proxy table, or
equivalent global coarse counters may be used if that better preserves the
current metrics architecture. The choice must be documented and tested.

### 6. The post-v2.12 oracle does not actually pin/record its Go toolchain

The pinned upstream snapshot's `go.mod` declares `go 1.23.0`.
`scripts/fetch_toxiproxy_post_v2_12.sh` pins the source commit and archive
checksum correctly, but invokes ambient `go build` and neither enforces nor
prints the compiler/toolchain identity.

M038/M039 closure records nevertheless claim a “recorded Go toolchain.”

M040 must make this reproducible. Preferred behavior:

- default the source build to an exact Go toolchain compatible with the
  upstream module declaration (prefer `go1.23.0` through
  `GOTOOLCHAIN` when available);
- allow an explicit override only through a named qualification variable;
- print/record the resolved `go version` and `go env GOTOOLCHAIN`;
- include the resolved toolchain in the qualification summary;
- fail mandatory qualification if the requested exact toolchain cannot be
  resolved.

If repository/toolchain constraints require another exact version, record the
reason in M040 closure rather than silently using whatever `go` is first on
`PATH`.

### 7. M039 exact-head hosted evidence was not actually rerun

M039 required the ordinary hosted Rust matrix on Linux, macOS, and Windows,
plus language/native-Python qualification on the same final candidate.

The M039 closure instead states that no per-tranche hosted re-test was
necessary because the Rust changes were not considered platform-specific.
That contradicts the registered M039 acceptance rule.

M040 must push one corrective candidate and require the hosted matrix to finish
green on that exact candidate. Prior runs are historical context, not closure
evidence.

### 8. Closure/current-state documentation is inconsistent

At registration:

- `plans/registry.md` says M036–M039 are closed and nothing is pending;
- `plans/README.md`, `plans/roadmap.md`, and `AGENTS.md` still describe
  M036 as ready and M037–M039 as blocked;
- the numbered M036–M039 plans retain their original handoff statuses, which
  is acceptable historical-plan behavior and does not need rewriting;
- the M036–M039 closure records use placeholder wording such as
  “Candidate: see git rev below” rather than a real exact candidate SHA;
- architecture verification/compatibility deep dives still contain stale
  47/47 / “packet_loss out of scope” statements even though strict v2.12 is
  now 50/50 and the snapshot profile exists.

Registration of M040 must make M040 the sole ready current handoff.
Implementation closure must reconcile the remaining documentation.

## Scope

### In scope

- Correct compatibility profile propagation in every `populate` response.
- Correct `Toxic::to_fault_with_profile` and profile-aware proxy
  translation helpers.
- Preserve strict-v2.12 as the default/frozen behavior.
- Refactor the post-v2.12 differential so every counted data-plane pass is
  backed by an assertion/comparator.
- Isolate data-plane cases from previously created toxics.
- Add exact `loss_rate=0` and `loss_rate=1` data-plane assertions.
- Add predeclared intermediate-loss and correlation/burst statistical
  qualification.
- Keep upstream stochastic behavior classified as intent-compatible, never
  exact-sequence compatible.
- Complete named stream-loss Prometheus metrics without changing the seven
  legacy activation slots.
- Pin/record the post-v2.12 Go build toolchain.
- Re-run strict v2.12 and post-v2.12 oracle gates in mandatory mode.
- Re-run OpenAPI, SDK, embed, Python-native, scenario, fuzz/security,
  package/release, and performance regressions affected by the corrective
  changes.
- Push and require exact-head hosted Linux/macOS/Windows CI plus existing
  language/native-Python jobs.
- Add additive correction notes to historical M036–M039 closure evidence only
  where needed to identify their implementation commits and point to M040 as
  the final corrective authority; do not rewrite historical claims as if M040
  had existed earlier.
- Reconcile registry/README/roadmap/AGENTS and current architecture
  verification/compatibility docs.
- Create M040 closure evidence with exact candidate SHA and hosted run IDs.

### Non-goals

- No change to ADR 007's 32 KiB logical stream-loss grain.
- No configurable grain size.
- No new core stream-loss probability/correlation semantics.
- No change to the seven-slot legacy activation arrays.
- No UDP/datagram semantics change.
- No real IP/TCP packet-loss implementation.
- No new Toxiproxy toxic beyond `packet_loss`.
- No moving-`main` compatibility.
- No promotion to a fictional Toxiproxy release number.
- No live-generation loss-state migration.
- No new language binding or generic C ABI.
- No broad rewrite of the compatibility adapter.
- No release/tag/publication action.

## Affected surfaces

Expected implementation surfaces:

- `crates/eggchaos-toxiproxy/src/lib.rs`;
- `crates/eggchaos-toxiproxy/tests/post_v212_differential.rs`;
- focused compatibility unit/integration tests;
- `scripts/fetch_toxiproxy_post_v2_12.sh`;
- `scripts/qualify_toxiproxy_post_v2_12.sh`;
- `crates/eggchaos-server/src/runtime/metrics.rs`;
- metrics aggregation/rendering call sites and metrics tests;
- `docs/toxiproxy.md`;
- `architecture/toxiproxy-compat.md`;
- `architecture/verification-qualification.md`;
- `architecture/core-fault-engine.md` only if metrics wording needs
  correction;
- `plans/registry.md`;
- `plans/README.md`;
- `plans/roadmap.md`;
- `AGENTS.md`;
- M036–M039 closure files only for clearly labelled additive corrective
  references, not rewritten historical evidence;
- new M040 closure evidence.

## Required compatibility corrections

### Profile propagation

There must be one profile source: `ToxiproxyAdapter.profile`.

Every rendering/translation path that is adapter-profile-sensitive must consume
that profile. In particular:

- `list_json`;
- `proxy_json`;
- create/update;
- populate keep;
- populate recreate;
- toxic add/list/get/update/remove presentation;
- any proxy translation helper used by tests/embedding.

Do not leave literal `CompatProfile::StrictV2_12` in a snapshot-profile
response path merely because strict is the default.

Focused regression:

1. start the adapter with `PostV2_12_2026_09_25`;
2. create a proxy;
3. add `packet_loss`;
4. call `POST /populate` with the same name/listen/upstream;
5. assert the returned proxy still contains a normal `packet_loss` toxic
   with the expected attributes, not an error object;
6. assert strict profile behavior remains unchanged.

### Profile-aware conversion helpers

Required public behavior:

- `Toxic::to_fault()`: strict v2.12 default; rejects `packet_loss`;
- `Toxic::to_fault_with_profile(StrictV2_12)`: rejects `packet_loss`;
- `Toxic::to_fault_with_profile(PostV2_12_2026_09_25)`: accepts and maps
  `packet_loss` to native `StreamLoss`;
- reverse mapping under snapshot produces `packet_loss`;
- reverse mapping under strict fails explicitly.

If a profile-aware proxy translation helper is added, keep the existing
`translate_proxy()` strict by default for compatibility.

## Required post-v2.12 data-plane corpus

Each data-plane experiment must use a clean proxy/fault fixture or explicitly
prove that only the intended toxic is active.

No data-plane case may share residual toxics from API CRUD coverage.

### Exact zero-loss case

Use one clean downstream `packet_loss` toxic:

`loss_rate = 0.0, correlation = 0.0, toxicity = 1.0`.

Send a payload large enough to cross several Eggchaos logical grains. Assert
for both oracle and Eggchaos:

- exact byte count equals input length;
- exact payload bytes equal input bytes;
- no timeout/partial response is accepted as a pass.

### Exact full-loss case

Use one clean downstream toxic:

`loss_rate = 1.0, correlation = 0.0, toxicity = 1.0`.

Within a bounded observation window assert for both implementations:

- zero payload bytes reach the client;
- connection timeout/open-state differences are tolerated and recorded;
- any forwarded payload byte is a failure.

### Intermediate loss-rate comparator

Freeze the comparator before running the final M040 candidate.

Preferred bounded design:

- `loss_rate = 0.25`, `correlation = 0.0`;
- at least 256 independent small tagged probes/connections per
  implementation, arranged so each probe is expected to exercise one
  userspace output chunk in the common case;
- classify each probe as fully received, partially received, or fully
  dropped;
- compute an observed missing/drop fraction;
- require a broad predeclared acceptance interval that establishes
  nonzero/non-total loss and is statistically credible for the chosen sample
  size (recommended starting interval: `0.10..=0.40`);
- record the exact sample size, timeout, payload size, interval, and observed
  result in the qualification artifact.

If implementation research shows the one-probe/one-chunk assumption is not
stable enough against the pinned oracle, replace it with a byte-fraction
comparator, but freeze that comparator and tolerance in source before the
final candidate run.

Do not tune the interval after seeing candidate output.

### Correlation/burst comparator

Demonstrate the directional effect of positive correlation without requiring
identical upstream chunk boundaries.

Preferred design:

- compare `loss_rate = 0.20, correlation = 0.0` with
  `loss_rate = 0.20, correlation = 0.50`;
- use fixed-size uniquely tagged sequential probes over a connection so a
  dropped response cannot be mistaken for a later response;
- collect at least 512 probe outcomes or another predeclared sample size with
  enough dropped and passed predecessor observations;
- compute:
  - `P(drop[n] | drop[n-1])`;
  - `P(drop[n] | pass[n-1])`;
- require positive-correlation runs to show a predeclared meaningful
  conditional gap (recommended floor: at least 0.20) for both the oracle and
  Eggchaos;
- require minimum conditioning-bucket counts before accepting the statistic;
- keep the entire corpus bounded by one explicit wall-clock timeout.

An equivalent predeclared burst metric is acceptable only if it measures the
same property and is documented before the final run.

### Corpus accounting

`Corpus.passed` may increase only after an actual assertion/comparator has
succeeded.

The final JSON summary must separate:

- exact API cases;
- exact data-plane edges;
- stochastic loss comparator;
- correlation comparator;
- recorded normalization/divergence cases.

A count such as “14/14” must never mix unconditional observations with real
passes.

## Stream-loss metrics completion

Do not extend `FAULT_TYPE_NAMES` or `activations: [u64; 7]`.

Add named metrics derived from final connection evidence. Preferred names:

- `eggchaos_stream_loss_chunks_evaluated_total`;
- `eggchaos_stream_loss_chunks_dropped_total`;
- `eggchaos_stream_loss_bytes_discarded_total`.

Use the existing bounded proxy/direction metric architecture where practical.
If labelled by proxy/direction:

- proxy cardinality must remain bounded by the existing metric table cap;
- direction must remain the two-value native enum;
- there must be an overflow strategy consistent with existing per-proxy
  counters.

Tests must prove:

- zero-loss increments evaluated but not dropped/discarded;
- full-loss increments all three consistently;
- multiple loss faults do not double-count discarded bytes;
- Prometheus output includes the new counters;
- existing seven legacy activation metric series remain unchanged.

## Oracle toolchain contract

The source commit and archive SHA remain frozen:

- commit: `40f7fd31bee529d824116bd2a11a9e3425e904ec`;
- archive SHA-256:
  `26351cc70792f1c3391bdd1d376974a79a756b349ec1abfa8b8f1524c042b13d`.

The build script must additionally freeze/record the Go compiler identity.

Preferred default:

`GOTOOLCHAIN=go1.23.0`

because the pinned upstream `go.mod` declares `go 1.23.0`.

The fetch/qualify scripts must print machine-readable fields for at least:

- requested toolchain;
- resolved `go version`;
- source commit;
- source checksum;
- built oracle path;
- oracle `-version` response.

Mandatory qualification must fail if it cannot establish the requested
toolchain identity.

## Planning and closure reconciliation

Registration state:

```text
M036 closed (historical implementation)
  -> M037 closed
  -> M038 closed
  -> M039 closed/provisional closure evidence
  -> M040 ready corrective
```

M040 is the sole ready handoff.

Do not rewrite the original numbered plan headers merely to make them say
`closed`; those files are historical executable handoffs and the repository
already preserves that pattern.

At M040 closure:

- `plans/registry.md` must identify M040 as closed and no successor unless
  new evidence justifies one;
- `plans/README.md`, `plans/roadmap.md`, and `AGENTS.md` must describe
  M036–M039 as completed historical implementation and M040 as the corrective
  final authority;
- stale “M036 ready / M037–M039 blocked” current-state prose must be absent;
- `architecture/toxiproxy-compat.md` must describe both profiles and the
  current strict/post-v2.12 qualification counts;
- `architecture/verification-qualification.md` must describe the current
  strict 50-case corpus plus the corrected post-v2.12 corpus;
- historical M036–M039 closure records may receive only clearly labelled
  additive correction/supersession notes identifying their implementation
  commits and pointing to M040. Do not edit old evidence to pretend it was
  collected after the fact.

Implementation commits to record:

- M036 implementation: `36ddf1a4ba78879938ca65c68193565355bc9be4`;
- M037 implementation: `a521b093cac5319183b6e34aef352afb7be43533`;
- M038 implementation: `8bfe0332ebc19a032443951818c2988319217e7d`;
- M039 implementation/claimed closure: `3b405af7684a681ffcf73709d6b6c90b10267a9a`.

M040 closure must contain its own real exact candidate SHA rather than a
placeholder.

## Ordered work packages

### WP1 — Freeze corrective regression tests

Before production fixes, add failing tests for:

- snapshot-profile `populate` keep rendering;
- `to_fault_with_profile(packet_loss)`;
- zero-loss exact preservation;
- full-loss zero-forwarding;
- isolated fixture enforcement.

Add the intermediate-loss and correlation comparator constants/tolerances in
source before final qualification.

### WP2 — Correct profile propagation/conversion

Remove strict-profile literals from profile-sensitive populate response paths,
fix profile-aware toxic conversion, and add a profile-aware proxy translation
helper if required. Preserve strict defaults.

### WP3 — Repair the post-v2.12 differential

Split API CRUD setup from data-plane fixtures. Implement exact edge assertions
and the predeclared stochastic/correlation comparators. Make the JSON summary
truthfully represent real checks.

### WP4 — Complete stream-loss metrics

Thread named stream-loss evidence into bounded Prometheus metrics and add
metrics regressions without changing legacy activation arrays.

### WP5 — Pin and report the Go oracle toolchain

Update fetch/qualification scripts and qualification output. Rebuild the pinned
oracle under the recorded exact Go toolchain.

### WP6 — Local corrective qualification

Run focused tests first, then the complete local gate on one committed
candidate.

### WP7 — Dual-oracle and cross-language qualification

Run strict v2.12 and corrected post-v2.12 mandatory oracles back-to-back,
followed by OpenAPI/SDK/embed/native-Python/EggFetch/package/fuzz/performance
gates.

### WP8 — Hosted exact-head qualification

Push the committed candidate. Require all mandatory hosted jobs on that exact
SHA, including Linux/macOS/Windows Rust, audit/deny, language clients, and the
M035 native-Python gate.

Do not create final closure before hosted jobs finish.

### WP9 — Documentation/planning reconciliation

Update current-state planning, deep-dive compatibility/verification docs, and
additive historical closure annotations.

### WP10 — Exact-candidate M040 closure

Create the M040 closure note only after all local, oracle, and hosted evidence
refers to the same candidate SHA (or after any documentation-only closure
commit has been requalified where required).

## Required tests

### Compatibility translation/profile

- strict `to_fault()` rejects `packet_loss`;
- strict `to_fault_with_profile` rejects `packet_loss`;
- snapshot `to_fault_with_profile` accepts and round-trips it;
- snapshot add/get/list/update/remove remains green;
- snapshot `populate` keep with existing `packet_loss` preserves the toxic;
- snapshot populate/create/recreate response paths use the active profile;
- strict `populate` behavior is unchanged;
- native `StreamLoss` strict reverse mapping still fails explicitly.

### Post-v2.12 differential

- exact zero-loss payload equality;
- exact full-loss zero forwarded bytes;
- isolated proxy/toxic fixture assertion for every data-plane case;
- intermediate loss-rate comparator;
- positive-correlation conditional/burst comparator;
- wrong type, defaults, update, list, out-of-range normalization remain
  covered;
- repeated qualifier runs do not collide with stale oracle processes;
- mandatory mode fails on missing/mismatched oracle/toolchain.

### Metrics

- named stream-loss counters in final connection accounting;
- Prometheus render contains all three named counters;
- bounded proxy/direction cardinality;
- no eighth activation index;
- existing activation metrics unchanged.

### Existing regressions

- core stream-loss golden/property suite;
- protocol/OpenAPI drift;
- Scenario V1/V2 fingerprints;
- Python sync/async client;
- TypeScript client;
- embed + Python-native;
- strict Toxiproxy v2.12 mandatory oracle;
- EggFetch;
- datagram golden/performance regression;
- fuzz/security;
- release/package/artifact smoke;
- stream performance benchmark.

## Verification

Minimum local exact-candidate commands:

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

Also run the repository's stream and datagram benchmark commands and preserve
raw output.

Hosted gate:

- push the exact candidate;
- require ordinary Linux/macOS/Windows Rust jobs to conclude `success`;
- require `cargo audit` and `cargo deny` hosted checks;
- require language-client qualification;
- require hosted native-Python qualification on the currently claimed
  platforms;
- record workflow run ID/URL, job names, conclusions, and exact SHA in M040
  closure.

A locally green result or a prior hosted run is incomplete evidence.

## Acceptance criteria

M040 closes only when:

- snapshot-profile populate paths never hard-code strict rendering;
- profile-aware toxic translation accepts `packet_loss` only under the
  snapshot profile;
- strict v2.12 behavior remains frozen/default;
- zero-loss differential proves exact payload preservation;
- full-loss differential proves zero payload forwarding;
- every data-plane test uses an isolated intended-toxic fixture;
- intermediate loss-rate behavior passes the predeclared statistical
  comparator on both implementations;
- positive correlation passes the predeclared burst/conditional comparator on
  both implementations;
- no stochastic comparator depends on exact upstream RNG/chunk sequence;
- post-v2.12 pass counts correspond only to actual successful checks;
- named stream-loss metrics are exposed without changing the seven-slot
  activation contract or cardinality bounds;
- the pinned oracle build records/enforces an exact Go toolchain;
- strict and post-v2.12 mandatory oracle gates both pass;
- the exact corrective candidate passes the full local gate;
- the same candidate passes the required hosted Linux/macOS/Windows,
  audit/deny, language-client, and native-Python jobs;
- current-state planning/docs no longer describe M036 as ready;
- architecture compatibility/verification docs reflect the real current
  profile/corpus state;
- historical closure records are preserved with additive correction references
  rather than silently rewritten;
- M040 closure records a real exact candidate SHA and hosted run IDs;
- no unresolved medium-or-higher correctness/security finding remains.

Create:

`plans/closure/M040-post-v2-12-stream-loss-corrective-requalification-closure.md`

## Stop/rejection conditions

Do not close if:

- any data-plane “pass” is merely an observation with no assertion;
- zero-loss/full-loss tests share residual toxics;
- statistical tolerances are chosen or loosened after seeing the final
  candidate output;
- snapshot profile behavior depends on a strict-profile literal in one route;
- public profile-aware translation still rejects its advertised toxic;
- the Go oracle uses an unreported ambient toolchain;
- the post-v2.12 oracle is missing or developer-mode incomplete;
- required hosted jobs were not run on the final corrective SHA;
- an eighth legacy activation slot is introduced;
- metrics cardinality becomes unbounded;
- historical closure evidence is rewritten to conceal the original gaps;
- documentation calls userspace `stream-loss` real TCP/IP packet loss;
- a failing gate is waived instead of corrected or explicitly moved to a new
  successor.

## Follow-on activation

M040 activates no automatic successor.

After clean M040 closure, the ADR 007 stream-loss/post-v2.12 tranche is
considered correctively closed. A future tagged Shopify Toxiproxy release may
justify a separate promotion/reconciliation milestone that diffs that tag
against the pinned `40f7fd31` snapshot before changing profile names or
compatibility claims.
