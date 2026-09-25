# M035 — Cross-Language Qualification and Closure Corrective

Status: ready  
Depends on: M034 (closed)  
Role: post-ADR-006 corrective qualification and planning closure

## Objective

Close the remaining qualification and maintenance defects discovered after the
M032–M034 cross-language implementation tranche without expanding the binding
architecture or adding another language surface.

M035 must:

1. fix the hosted `language-clients` false failure caused by cleanup/trap exit
   semantics and obtain a genuinely green hosted SDK matrix;
2. make native Python qualification portable enough to run in hosted CI on
   supported host platforms and record only platforms actually exercised;
3. remove the duplicated datagram fault-mutation authority currently present in
   both the native HTTP route and `eggchaos-embed`, then prove HTTP/embed
   conformance for datagram CRUD/conflict behavior;
4. reconcile all planning/handoff documents to the actual closed M032–M034
   state; and
5. requalify the exact post-corrective head across Rust, SDK, native-binding,
   compatibility, packaging, security, and performance gates.

This is a corrective successor. It does not rewrite the historical M032,
M033, or M034 closure records. Their exact candidates remain valid evidence for
what those milestones proved. M035 records the later repository-level closure
of the defects found after those candidates.

## Baseline and observed defects

Current registration baseline: `ca53ac892206f17ec12a81494f229e74a6b23d56`.

### Hosted SDK qualification is falsely red

The latest hosted CI Rust jobs pass on Linux, macOS, and Windows, including:

- fmt;
- clippy with `-D warnings`;
- full workspace tests;
- explicit IPv6 datagram qualification;
- docs;
- `cargo audit`;
- `cargo deny`.

The `language-clients` job fails after successful qualification. The failing
job log shows:

- Python live suite passes;
- TypeScript contract/live suite passes;
- Python sdist and wheel build;
- TypeScript tarball builds;
- the script prints `{"language_clients":"pass"}`;
- the step then exits with status 143.

The root cause is the EXIT trap in
`scripts/qualify_language_clients.sh`. Under `set -e`, cleanup intentionally
terminates the two child servers and then `wait` observes SIGTERM status 143.
That cleanup status escapes as the successful script's process status.

The fix must preserve the original test/build exit code while making expected
cleanup failures non-fatal. It must not blindly append `|| true` to the whole
qualification command or otherwise mask real test/package failures.

### Native Python is not protected by hosted CI

M034 added:

- `crates/eggchaos-embed`;
- standalone `bindings/python-native`;
- `scripts/check_python_native.sh`;
- `scripts/qualify_python_native.sh`;
- `scripts/build_python_native_artifacts.sh`.

The M034 closure correctly records macOS x86_64 import/runtime smoke, macOS
arm64 built-only evidence, and no Linux/Windows native-wheel claim.

Current native scripts contain host assumptions from the original local
qualification. In particular, an x86_64 Python interpreter selects
`--target x86_64-apple-darwin` without first checking the operating system.
That is not valid as a generic Linux hosted-CI path.

M035 must make host/target selection explicit and add a hosted native-Python
gate for platforms it can truthfully qualify. Linux x86_64 and the current
hosted macOS architecture are preferred minimum targets. Windows remains
out-of-scope unless implementation can add it without expanding the milestone.

### Datagram fault mutation has two authorities

M034's closure explicitly recorded one logic duplication. Today both
`crates/eggchaos-server/src/admin.rs` and
`crates/eggchaos-embed/src/lib.rs` independently implement datagram fault:

- add;
- path lookup across both directions;
- patch;
- remove;
- duplicate-ID rejection;
- cross-direction ID uniqueness;
- generation-guarded policy publication.

This is exactly the kind of semantic split the embedding facade was intended to
avoid.

The existing facade/HTTP conformance test compares stream proxy/fault views but
does not exercise the duplicated datagram mutation path end-to-end.

M035 must consolidate this mutation authority below both HTTP and embed, then
prove the two surfaces stay equivalent.

### Planning state is inconsistent

The registry rows and roadmap correctly report M032–M034 closed, but narrative
handoff text is stale:

- `plans/README.md` still says M032 ready / M033 blocked / M034 blocked;
- `AGENTS.md` says the same and calls M032 the sole ready item;
- `plans/registry.md` has a closed chain followed by text saying M033 is the
  sole ready handoff.

M035 registration corrects the immediate handoff state to make M035 the sole
ready item. M035 closure must re-check all current-state documents and remove
any remaining contradictory status language.

## Scope

### In scope

- Correct `scripts/qualify_language_clients.sh` cleanup/exit semantics.
- Add a regression that proves successful qualification exits zero after child
  cleanup and genuine failures remain nonzero.
- Preserve deterministic cleanup of both spawned server processes and temporary
  directories.
- Make native-Python build/check scripts host-aware rather than macOS-x86_64
  implicit.
- Add a focused hosted native-Python CI job with pinned Python/PyO3/maturin
  tooling.
- Run `check_python_native.sh` and an appropriate hosted native qualification
  path on every platform claimed by that job.
- Import/runtime smoke every wheel claimed as supported by M035.
- Keep unexercised platforms explicitly unclaimed/incomplete.
- Consolidate datagram fault CRUD/mutation semantics into one server-side,
  HTTP-independent authority reusable by both `NativeAdmin` and
  `eggchaos-embed`.
- Add datagram HTTP ↔ embed conformance covering create/get/list/patch/delete
  and conflict behavior.
- Preserve the native `/v1` JSON/OpenAPI contract byte/semantic behavior.
- Preserve datagram deterministic traces and publication/generation semantics.
- Reconcile `plans/registry.md`, `plans/README.md`,
  `plans/roadmap.md`, `AGENTS.md`, architecture/tooling docs, and binding docs.
- Re-run exact-head local and hosted qualification.
- Create M035 closure evidence only after the hosted CI result is green.

### Non-goals

- No new language SDK.
- No generic C ABI.
- No Node native addon.
- No JNI, P/Invoke, cgo, UniFFI, or WASM.
- No new native API route or OpenAPI version.
- No new stream/datagram fault kind.
- No Scenario V2 language change.
- No retry/daemon-management feature for SDKs.
- No widening of Python-native platform claims without executable evidence.
- No rewrite of M032/M033/M034 historical closure candidates.
- No change to the workspace Rust MSRV.
- No handwritten unsafe code.
- No Python callback in the data plane.
- No refactor of unrelated server/runtime code merely because M035 touches
  `ControlState` or admin routing.

## Affected surfaces

Expected implementation surfaces:

- `scripts/qualify_language_clients.sh`;
- native binding scripts:
  - `scripts/check_python_native.sh`;
  - `scripts/qualify_python_native.sh`;
  - `scripts/build_python_native_artifacts.sh`;
- `.github/workflows/ci.yml`;
- optionally `.github/workflows/release.yml` if native-binding qualification
  belongs in the release gate after runtime cost is measured;
- `crates/eggchaos-server/src/admin.rs`;
- `crates/eggchaos-server/src/runtime/control.rs` and/or one narrow
  HTTP-independent server operation module;
- `crates/eggchaos-server/src/lib.rs` only for the smallest reusable operation
  seam required by embed;
- `crates/eggchaos-embed/src/lib.rs`;
- `crates/eggchaos-embed/tests/facade.rs`;
- native/SDK test directories;
- `architecture/verification-qualification.md`;
- `architecture/tooling-distribution.md`;
- `docs/control-plane.md` and native-binding README only if qualification or
  support claims change;
- planning/current-state documents and M035 closure record.

## Required datagram mutation authority

M035 should not close with parallel HTTP and embed implementations of the same
datagram fault state transition.

Preferred shape:

```text
wire/Python values
      |
      v
protocol/server conversion
      |
      v
HTTP-independent server datagram fault operation authority
      |
      +------> NativeAdmin response adapter
      |
      +------> eggchaos-embed value/error adapter
      |
      v
ControlState + DatagramPlan publication
```

The exact code shape may be:

- methods on `ControlState` using core/runtime types; or
- a narrow server-side operation helper layered over `ControlState`.

Prefer core/runtime types at the mutation authority. Do not push HTTP request
DTOs, JSON response assembly, or PyO3 concepts into `ControlState`.

The shared authority must own, once:

- lookup across directions where path identity requires it;
- duplicate ID detection;
- cross-direction ID uniqueness;
- patch non-emptiness at the semantic layer after DTO conversion;
- plan reconstruction;
- expected-generation publication;
- not-found/conflict propagation.

Wire parsing/serialization remains protocol/admin responsibility. Embed error
mapping remains embed responsibility.

## Hosted SDK cleanup semantics

Refactor shell cleanup into an explicit function that:

1. captures the original command/script status before cleanup;
2. disables fail-fast behavior inside cleanup;
3. terminates only known child PIDs;
4. waits/reaps those children without treating expected SIGTERM as a test
   failure;
5. removes temporary state;
6. exits with the original qualification status.

Do not convert all failures into zero.

Add a focused regression or shell fixture that proves:

- successful main body + SIGTERM cleanup => exit 0;
- deliberately failing main body + cleanup => original nonzero exit;
- both child processes are gone after either path;
- temporary directory cleanup still occurs.

Apply the same cleanup pattern to other newly touched binding qualification
scripts when they have the same defect. Do not broaden into a repository-wide
shell rewrite.

## Native Python hosted qualification

### Host/target detection

Remove the assumption that x86_64 means Apple x86_64.

Derive build behavior from both host OS and architecture, preferably from the
Rust host triple and/or Python platform metadata. An explicit target may be used
where maturin requires it; otherwise prefer native-host builds.

Cross-built wheels are not runtime-qualified unless a matching interpreter
actually imports and exercises them.

### CI floor

Add a dedicated native-Python job separate from the ordinary Rust and remote
language-client jobs so failures are attributable and the Rust gate stays
independent of Python packaging.

Preferred minimum hosted matrix:

- Ubuntu hosted runner, Python 3.11 or 3.12, native host architecture;
- macOS hosted runner, Python 3.11 or 3.12, native host architecture.

A larger Python-version matrix is optional if runtime remains bounded.
Do not multiply identical expensive Rust builds without evidence that the extra
matrix adds useful coverage.

The job must install pinned:

- maturin 1.9.5 (or a later version only with an explicit M035 compatibility
  note);
- required Python test tooling;
- cargo-audit if `check_python_native.sh` continues to require it.

At minimum run:

- the safe Rust embed tests;
- unsafe-boundary audit;
- binding crate audit/build;
- abi3 wheel inspection;
- import/runtime smoke;
- Python-native test suite;
- remote/native conformance where practical on hosted runners.

If the full benchmark makes ordinary CI too expensive/noisy, keep performance
measurement in a dedicated qualification/release command and run a bounded
functional conformance subset in CI.

### Support claims

After M035, documentation must list separately:

- hosted runtime-qualified platforms;
- built-only/cross-built platforms;
- unqualified platforms.

No claim may be inferred from the Rust CLI artifact matrix.

## Datagram HTTP/embed conformance

Extend conformance beyond the existing stream check.

At minimum, on one shared `ControlState`/embedded service, compare HTTP and
embed outcomes for:

1. datagram proxy creation and view;
2. add upstream fault;
3. get by path ID;
4. list directional fault state;
5. patch probability;
6. duplicate same-direction add conflict;
7. duplicate cross-direction ID conflict;
8. delete fault;
9. post-delete not-found;
10. proxy delete/reset behavior as needed to prove cleanup.

Compare semantic DTO/value equality and equivalent error categories/status
mappings. Do not assert incidental formatting or scheduler timing.

The test should fail if admin or embed reintroduces independent mutation
semantics later.

## Planning reconciliation

Registration of M035 should make current handoff state unambiguous:

```text
M032 closed -> M033 closed -> M034 closed -> M035 ready
```

M035 is a corrective successor, not an ADR 006 feature expansion.

At implementation closure, verify all of:

- `plans/registry.md`;
- `plans/README.md`;
- `plans/roadmap.md`;
- `AGENTS.md`;
- architecture/tooling qualification docs;
- binding READMEs;
- closure README if it carries current-state text.

No current-state document may call M032/M033/M034 ready or blocked after M035
closure.

## Ordered work packages

### WP1 — Reproduce and freeze the false-red CI failure

Record the exact hosted run/job evidence showing
`{"language_clients":"pass"}` followed by exit 143. Add a minimal regression
for cleanup status preservation before changing the script.

### WP2 — Correct language-client process cleanup

Implement status-preserving cleanup/reaping. Run the language qualification
locally and in hosted CI. Verify genuine injected test failure still produces a
nonzero step.

### WP3 — Make native-Python scripts host-aware

Refactor target selection and wheel import-smoke logic around actual host OS +
architecture. Preserve abi3-py311 and the existing no-handwritten-unsafe
boundary.

### WP4 — Add hosted native-Python CI

Add the bounded dedicated job, pinned tool installation, cache strategy, and
clear host/platform evidence. Avoid coupling normal Rust checks to PyPI/npm
availability beyond this focused job.

### WP5 — Consolidate datagram fault mutation authority

Move add/get/patch/remove/path-conflict semantics out of HTTP/embed duplicate
branches into one HTTP-independent server authority. Keep DTO conversion and
response/error presentation at their current outer layers.

### WP6 — Expand datagram conformance

Add HTTP/embed datagram mutation equivalence and negative/conflict tests.
Retain existing stream conformance.

### WP7 — Planning/documentation reconciliation

Update all current-state/handoff text and qualification/support claims.
Historical closure files remain immutable except for a new M035 record.

### WP8 — Exact-head local qualification

Run all M035-required local gates on one exact candidate SHA.

### WP9 — Hosted exact-head qualification and closure

Push the candidate, require the complete hosted CI matrix to finish green,
record run URLs/IDs and exact SHA, then create M035 closure evidence and mark
the registry/roadmap closed.

## Invariants and failure semantics

- M032–M034 historical closure evidence is preserved.
- Native `/v1` OpenAPI/JSON behavior does not change.
- The SDK operation inventory remains 36 unless a separately planned native API
  change occurs.
- No cleanup trap may mask the original test/build failure.
- Child server processes are always reaped.
- The remote language-client job is green only when its real qualification
  succeeds.
- Native Python support is claimed only where import/runtime smoke exists.
- `eggchaos-embed` remains safe Rust.
- PyO3 remains the only FFI framework boundary; no handwritten unsafe.
- One datagram fault mutation authority serves HTTP and embed.
- `ControlState` remains the single runtime/state authority.
- Deterministic datagram traces, generation CAS semantics, and association
  lifecycle remain unchanged.
- No Python/TypeScript code enters the stream/datagram data plane.
- No generic C ABI is introduced.
- No existing Toxiproxy v2.12 compatibility behavior changes.

## Required tests

### Shell/process cleanup

- success body + child SIGTERM cleanup exits zero;
- failing body preserves nonzero exit;
- no orphan child processes;
- temp directories removed;
- repeated qualification runs do not hit stale listeners/files.

### Remote SDKs

- `check_openapi.sh`;
- `check_python_client.sh`;
- `check_typescript_client.sh`;
- `qualify_language_clients.sh`;
- hosted Linux/macOS language-client jobs green.

### Server/embed datagram authority

- add/list/get/patch/remove through shared authority;
- same-direction duplicate conflict;
- cross-direction duplicate conflict;
- not-found after delete;
- generation conflict/CAS behavior retained;
- HTTP/embed semantic equivalence for datagram views and errors;
- existing stream HTTP/embed conformance remains green;
- existing datagram golden traces unchanged.

### Native Python

- `cargo test -p eggchaos-embed --all-features`;
- binding crate test/audit;
- no handwritten unsafe audit;
- abi3 wheel inspection;
- import/runtime smoke on each claimed hosted target;
- native Python unit/lifecycle suite;
- remote/native conformance;
- interpreter shutdown and double-close regressions;
- package artifact build for every newly claimed platform.

### Existing regressions

- full workspace gate;
- OpenAPI drift/inventory;
- pinned Toxiproxy 47/47 differential;
- EggFetch qualification;
- datagram benchmark budgets;
- release/package smoke;
- release artifact smoke;
- fuzz qualification if the exact-candidate release gate is run as declared.

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
./scripts/benchmark_datagram.sh
./scripts/qualify_eggfetch.sh
TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" \
  EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 \
  ./scripts/qualify_toxiproxy_v2_12.sh
./scripts/release-smoke.sh
./scripts/release-artifact-smoke.sh
```

If M035 changes a fuzz-covered control/runtime path or the owner is using M035
as a release-quality exact-head gate, also run:

```sh
EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh
```

Hosted gate:

- push the exact candidate;
- wait for all ordinary Rust jobs;
- wait for the complete remote language-client matrix;
- wait for the new native-Python hosted job(s);
- require all required jobs to conclude `success`;
- record the workflow run IDs/URLs in closure evidence.

A locally green script followed by hosted failure is incomplete evidence and
must not be closed.

## Acceptance criteria

M035 closes only when:

- the language-client qualification script returns zero after successful child
  cleanup on hosted Linux and macOS;
- injected/real qualification failures still propagate nonzero;
- latest exact-candidate hosted CI is green, not merely locally green;
- stale M032/M033/M034 ready/blocked narrative is removed from all
  current-state planning/handoff documents;
- native Python scripts are host-aware;
- at least Linux x86_64 and the actual hosted macOS architecture are
  import/runtime-smoked if both are available in the configured hosted matrix;
- any platform not actually runtime-smoked remains explicitly unclaimed;
- a dedicated native-Python hosted gate protects the binding/facade;
- datagram fault mutation logic has one server-side authority shared by HTTP
  and embed;
- datagram HTTP/embed conformance covers CRUD plus duplicate/conflict behavior;
- native JSON/OpenAPI contract and operation inventory are unchanged;
- Rust workspace, SDK, native binding, Toxiproxy, EggFetch, deterministic
  datagram, package, security, and performance gates are green on the exact
  candidate;
- no handwritten unsafe or generic C ABI is introduced;
- M035 closure records exact candidate SHA, hosted workflow evidence,
  platform-support evidence, and any remaining non-blocking limitations.

Create:

`plans/closure/M035-cross-language-qualification-and-closure-corrective-closure.md`.

## Stop/rejection conditions

Do not close if:

- hosted CI remains red even when local scripts pass;
- the cleanup fix masks a genuine qualification failure;
- child daemons are left orphaned;
- native wheel targets are selected by architecture without OS validation;
- documentation claims Linux/macOS/Windows native support without runtime
  evidence for the claimed target;
- HTTP and embed retain separate datagram mutation implementations;
- consolidation moves wire/HTTP concepts into `eggchaos-core`;
- datagram generation/conflict semantics change;
- OpenAPI/native wire fixtures drift;
- deterministic datagram traces change unexpectedly;
- handwritten unsafe appears;
- a generic ABI or new binding surface is added;
- closure is recorded without a green exact-candidate hosted workflow.

## Follow-on activation

M035 activates no automatic successor.

After a clean closure, ADR 006 and its corrective successor are considered
fully reconciled. Future language SDKs should reuse the M032 OpenAPI contract.
Future native bindings require concrete consumer demand. A generic C ABI still
requires a separate ADR and the multi-consumer evidence required by ADR 006.
