# M019 — Qualification Expansion and Final Corrective Requalification

Status: blocked
Depends on: M016, M017, M018
Role: final pre-tag corrective qualification gate

## Objective

Requalify the corrected/hardened repository on one exact commit and strengthen release evidence so the automated release lane cannot report success while required compatibility evidence is merely unavailable.

M019 supersedes M015 only as the final tag-candidate authority after M016–M018. M015 remains valid historical evidence for `cd88b22` and must not be rewritten.

## User-visible outcome

After M019, one exact post-hardening commit has:

- green ordinary CI on all supported host OSes;
- a green dedicated release workflow whose Toxiproxy differential gate is mandatory rather than optional;
- expanded fuzz coverage over the native parsing/transition surfaces identified by the verification matrix;
- completed or explicitly reclassified Toxiproxy data-plane evidence for bandwidth, slow-close, and slicer;
- Eggfetch regression evidence;
- artifact/checksum/package/security evidence;
- performance evidence within the frozen budget;
- reconciled planning/docs naming M019 as the final pre-tag authority.

Only then may the owner proceed with tag/publication/release.

## Baseline and evidence gaps

M015 demonstrated a clean release candidate at `cd88b22`, but the later audit identified several qualification gaps:

1. `scripts/qualify_toxiproxy_v2_12.sh` exits 0 with `differential:incomplete` when the pinned oracle is absent.
2. The hosted release workflow does not provision the pinned v2.12.0 oracle, so an automated release run can be green without executing the differential corpus.
3. Current fuzzing has one `plan_json` target, while `plans/reference/verification-matrix.md` calls for hostile config/TOML parsing, arithmetic, transition/evidence serialization, and compatibility attribute coverage.
4. Toxiproxy parity documentation still classifies bandwidth timing, slow-close close timing, and slicer data-plane differential evidence as incomplete.
5. M016–M018 change release-relevant code and therefore invalidate M015 as the final exact-HEAD tag candidate even though its historical verdict remains valid.

## Scope

### In scope

- Make pinned Toxiproxy oracle presence mandatory in the release qualification lane.
- Verify oracle identity/checksum before differential execution.
- Expand fuzz/property targets over the highest-value parser/state surfaces.
- Add bounded differential data-plane cases for currently incomplete v2.12 toxics where a stable comparator is possible.
- Run full exact-candidate CI/release/security/package/artifact/performance qualification.
- Reconcile registry/roadmap/docs and create M019 closure evidence.

### Non-goals

- No post-v2.12 Toxiproxy feature expansion.
- No fake packet-loss semantics.
- No new product features.
- No migration of the benchmark framework to Eggbench in this gate.
- No requirement that cross-built foreign artifacts be runtime-executed where the hosted runner cannot do so.
- No rewriting historical closure evidence.

## Ordered work packages

### WP1 — Make the oracle a hard release dependency

Change qualification tooling so release mode cannot succeed with unavailable/wrong-version oracle.

Required behavior:

- local developer mode may retain an explicit non-strict option that reports `incomplete`;
- release workflow must enable strict mode;
- strict mode exits nonzero if the oracle is missing, wrong version, wrong checksum, or differential execution does not produce a clean verdict;
- workflow obtains the pinned v2.12.0 binary from a reproducible source or a repository-maintained acquisition script;
- verify the expected SHA-256 recorded in the existing parity/closure evidence before execution;
- do not trust a random `toxiproxy-server` on `PATH` in strict release mode.

Record provenance in docs/scripts.

### WP2 — Expand fuzz targets

Add focused bounded fuzz/property targets for at least:

- schema-v1 TOML/native config parse + compile;
- native control JSON DTO parse/validate/round-trip from M017;
- fault plan/evidence serialization round-trip;
- fault transition/publication sequences or the narrowest deterministic state-machine input surface feasible without network timing;
- Toxiproxy toxic attribute translation maps.

Prefer small deterministic targets with stable invariants over one giant integration fuzzer.

Malformed input must not panic. Any crash becomes a blocking defect, not a corpus exception.

### WP3 — Complete high-value Toxiproxy data-plane differential evidence

Attempt oracle-backed cases for:

- bandwidth sustained rate/unit behavior with tolerance;
- slow_close close-delay behavior with tolerance;
- slicer byte preservation and timing/chunk behavior to the extent externally observable.

Comparators must distinguish exact byte semantics from wall-clock tolerance. Do not claim Go scheduler/random chunk identity if the native implementation intentionally differs.

If a comparator is inherently unstable or semantics are intentionally divergent, update the parity matrix to a precise `intent compatible` limitation with evidence explaining why, rather than leaving a vague `incomplete` claim.

### WP4 — Full ordinary CI and integration gates

On one frozen candidate SHA run:

- ordinary Ubuntu/macOS/Windows CI;
- `./scripts/check.sh` locally where practical;
- `./scripts/qualify_eggfetch.sh`;
- strict pinned-oracle Toxiproxy qualification;
- expanded fuzz suite;
- audit/deny.

All evidence must correspond to the frozen candidate or be explicitly identified as supplemental.

### WP5 — Package and artifact release gate

Run the existing release workflow on the candidate with:

- release smoke;
- strict oracle differential;
- Eggfetch qualification;
- fuzz suite;
- five-target artifact matrix;
- checksums;
- package/order proof.

Retain target expectations:

- Linux x86_64 GNU;
- Linux aarch64 GNU;
- macOS x86_64;
- macOS aarch64;
- Windows x86_64 MSVC.

Record build-only vs runtime-smoked status truthfully.

### WP6 — Performance and benchmarking boundary

Rerun the canonical same-session bare-relay vs empty-plan performance measurement. The frozen M008 budget remains:

`eggchaos_empty_plan mean throughput >= 70% of same-session bare_eggress_relay mean`.

If M016–M018 contain no hot-path semantic changes, a representative sanity run is sufficient; otherwise rerun the full fault matrix.

Do not expand the bespoke benchmark framework into a general experiment system. Document that future repeated-trial/environment/comparison orchestration should integrate with Eggbench once its production Eggstack driver surface is ready; Eggchaos should retain only project-specific workloads/fixtures.

### WP7 — Documentation and planning reconciliation

Update:

- `plans/registry.md`;
- `plans/roadmap.md`;
- `plans/README.md`;
- `architecture/verification-qualification.md`;
- `architecture/tooling-distribution.md`;
- `docs/toxiproxy.md` if evidence classification changes;
- `AGENTS.md` planning/gate notes.

Create `plans/closure/M019-qualification-expansion-and-final-corrective-requalification-closure.md` with one clean/not-clean verdict.

## Verification commands

At minimum on the frozen candidate:

```sh
./scripts/check.sh
cargo audit --deny warnings
cargo deny check advisories licenses bans sources
EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh
./scripts/qualify_eggfetch.sh
EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 TOXIPROXY_SERVER=/path/to/pinned/v2.12.0 ./scripts/qualify_toxiproxy_v2_12.sh
./scripts/release-smoke.sh
./scripts/benchmark.sh
```

The exact strict-mode variable/name may differ, but release behavior must be equivalent.

Then dispatch the dedicated release workflow against the exact candidate and record run/job IDs plus artifact checksums.

## Acceptance criteria

M019 closes only when:

- M016–M018 are closed;
- one exact candidate SHA is frozen;
- ordinary three-OS CI is green on that candidate;
- dedicated release qualification is green on that candidate;
- the release lane proves the pinned Toxiproxy oracle actually ran and passed;
- expanded fuzz targets execute with no crashes;
- Toxiproxy bandwidth/slow-close/slicer evidence is completed or explicitly classified with precise justified limits;
- Eggfetch qualification passes;
- audit/deny/package/artifact gates pass;
- performance remains within budget or a deliberate budget change is separately justified;
- planning/docs identify M019, not M015, as the final pre-tag authority;
- no unresolved medium-or-higher correctness/release finding remains.

## Stop/rejection conditions

Do not close if:

- the hosted release workflow can still return success when the oracle is unavailable;
- evidence is assembled from moving SHAs without justification;
- a new fuzz crash is suppressed rather than fixed;
- differential timing is declared exact without a defensible comparator;
- an artifact target silently skips;
- M016/M017/M018 changes are not represented in the frozen candidate;
- a release-critical fix lands after candidate freeze without rerunning affected gates.

## Follow-on activation

On a clean M019 closure there is no additional planned pre-tag implementation work. The owner may then create the `v0.1.0` tag, publish crates in the verified order, and create the GitHub release.

If M019 is not clean, register a narrowly scoped successor corrective plan rather than weakening the gate.
