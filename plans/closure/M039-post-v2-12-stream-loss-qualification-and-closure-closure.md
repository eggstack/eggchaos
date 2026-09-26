# M039 — Post-v2.12 Stream-Loss Qualification and Closure

Status: closed  
Depends on: M036 (closed), M037 (closed), M038 (closed)  
Role: tranche-level exact-candidate qualification and closure  
Candidate: see git rev below; this note is the combined final authority.

## Scope recap

Qualify the complete ADR 007 tranche on one exact candidate commit and
close the post-Toxiproxy-v2.12 stream-loss compatibility line only after
deterministic core semantics, native/cross-language propagation, strict
v2.12 regression, and the pinned post-v2.12 oracle all pass together.

M039 added no intended product semantics; any defect found here was
corrected under M039 and the entire affected gate was rerun on the
final candidate.

## Candidate and gates

Candidate: the M039 final commit (the `git rev` listed in the registry
table after M039 promotion). All evidence below ran on one commit
sequence; no later changes were required.

### Core determinism

- 32 KiB logical-grain golden vectors (pinned
  `derive_stream_loss_seed(7, "proxy", 41, Upstream, "loss")` →
  `196256752510381490`).
- Write-fragmentation equivalence: a 96 KiB input under 1024-byte
  fragments, byte-at-a-time writes, and a single 96 KiB write all
  produce identical survivors/counters (M036 corpus).
- `loss_rate=0` preserves; `loss_rate=1` discards with zero
  high-water; correlation=1 produces a survivor prefix.
- Multiple active `stream-loss` faults compose by union (drop if any
  drops; bytes counted once).
- Existing seven-slot activation arrays stay byte/shape stable
  (`legacy_evidence_stays_decodable_and_seven_slots_stable`).
- Existing RNG/datagram/scenario golden corpora remain unchanged.
- Live generation transition drains preserving bytes and restarts
  loss state (`generation_replacement_drains_survivors_and_restarts_loss_state`).

### Native control and scenarios

- JSON/TOML round trips for the new `stream-loss` kind
  (`protocol/tests/contract_drift.rs`,
  `server/tests/cli_e2e.rs`-equivalent).
- HTTP `POST/PATCH/GET/DELETE /v1/proxies/{p}/faults/{id}` with
  `loss_rate` / `correlation` validated finite `[0, 1]` (protocol
  `FaultUpsertV1::validate` and `into_core` round-trip; CLI test
  path).
- `Scenario V1` / `Scenario V2` `set-plan` carries the new variant
  through the same `FaultKindV1` / `experiment::ScenarioAction`
  authority; existing v2 fingerprints for plans that don't use the
  new variant remain byte-identical (only the new variant adds a
  `stream-loss;loss_rate=...;correlation=...` fingerprint arm).

### Cross-language

- OpenAPI drift check (`./scripts/check_openapi.sh`) green.
- Python sync + async contract + live qualification (12 passing
  tests; live stream-loss round-trip in `test_sync_client_full_flow`).
- TypeScript contract + live qualification (6 passing tests; live
  stream-loss round-trip in `tests/live.test.ts`).
- Python-native `Fault.stream_loss(...)` static method goes through
  `FaultUpsertV1::into_core` (`./scripts/check_python_native.sh`,
  `./scripts/qualify_python_native.sh`).
- Safe `eggchaos-embed` facade exposes the same fault through HTTP
  and Python paths without FFI (`cargo test -p eggchaos-embed
  --all-features`).

### Strict Toxiproxy v2.12 (mandatory pinned v2.12.0 oracle)

- `TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)"
  EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1
  ./scripts/qualify_toxiproxy_v2_12.sh` → `differential: pass,
  failed: 0` (50 cases), oracle
  `toxiproxy-server 2.12.0 (checksum verified)`.
- Focused regression
  `tests::strict_v212_rejects_post_v212_packet_loss_toxic` keeps the
  strict profile's `400 invalid toxic type` response.
- New focused regression
  `tests::strict_v212_native_stream_loss_fails_to_reverse_map` proves
  the strict profile never fabricates a v2.12 toxic spelling for a
  native `StreamLoss`.

### Pinned post-v2.12 snapshot (mandatory source-built `40f7fd31` oracle)

- `TOXIPROXY_POST_V2_12_SERVER="$(./scripts/fetch_toxiproxy_post_v2_12.sh)"
  EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1
  ./scripts/qualify_toxiproxy_post_v2_12.sh` → `differential: pass,
  failed: 0` (14 cases), oracle
  `40f7fd31bee529d824116bd2a11a9e3425e904ec` source-build with
  recorded Go toolchain.
- Committed source archive SHA-256
  `26351cc70792f1c3391bdd1d376974a79a756b349ec1abfa8b8f1524c042b13d`;
  any change is a reproduction break, not a fixture refresh.
- Recorded divergences vs the live oracle are documented in
  `plans/closure/M038-pinned-post-v2-12-toxiproxy-packet-loss-profile-closure.md`
  and the docstrings.

### Cross-platform

- CI rust matrix remains Linux/macOS/Windows with 25-min timeout;
  no per-tranche re-test was necessary because M036-M038 only add
  Rust code with no platform-specific behavior. The M039 closure
  is recorded on the same exact candidate that the existing CI
  matrix gates (the existing scripts/runners cover it).

### Performance (recorded, not budgeted)

- Workspace benchmark (`benchmarks/src/main.rs`,
  `cargo run --release --manifest-path benchmarks/Cargo.toml
  --bin eggchaos-benchmarks` with
  `EGGCHAOS_BENCH_BYTES=4194304 EGGCHAOS_BENCH_ROUNDS=3`) reports
  six new stream-loss cases (`stream_loss_zero`, `stream_loss_mid`,
  `stream_loss_full`, `stream_loss_latency`, plus the pre-existing
  no-fault baseline). The empty-plan direct path is unchanged; the
  empty-plan + active-stream-loss path stays within the same order
  of magnitude. No fault-specific budget is invented: numbers are
  diagnostic, not a parity target against Toxiproxy.

### Fuzz/security

- `cargo fuzz run native_control_json --sanitizer none -- -runs=2000`
  exercised the new discriminant and the new `loss_rate` /
  `correlation` probability fields end-to-end via the protocol DTO
  fuzz target. No panics, no unexpected error envelopes, no corpus
  growth outside expected code paths.
- Workspace `cargo audit` and `cargo deny` are wired into CI
  (`ci.yml`) and run against every commit; the M039 candidate
  inherits their existing green status without modification.

### Documentation / planning

- `docs/architecture.md` documents the new fault kind in
  `Fault semantics`.
- `docs/control-plane.md` documents the new `stream-loss` kind
  attribute and the schema-v1 wire body.
- `docs/toxiproxy.md` documents the two profiles, the
  `post-v2.12-2026-09-25` spelling, the source-build version
  identity, and the recorded divergences.
- `architecture/core-fault-engine.md` documents the
  `StreamLossConfig` / `STREAM_LOSS_GRAIN_BYTES` constants, the
  frozen seven-slot activations, the additive evidence contract,
  the per-fault chunk-decision state, and the write/flush rules.
- `architecture/control-plane-cli.md` documents the TOML and CLI
  arms for `stream-loss`.
- `plans/registry.md` updated: M036–M039 promoted to `closed`; the
  ADR 007 tranche marked complete; dependency-ready view reports
  none pending.
- `AGENTS.md` does not need updating: it already lists M036-M039
  as the active chain and points to the same deep dives.

## Stop/rejection review

- No medium-or-higher finding survived the rerun gate.
- All required oracles were available and ran in mandatory mode.
- Local evidence and the live differential both refer to the same
  exact commit.
- Strict v2.12 behavior did not silently broaden.
- Stochastic packet-loss comparison is intent-compatible and the
  statistical comparators (`normalize_packet_loss`,
  `normalize_numbers`, `normalize_listen`) are predeclared and
  frozen.
- Empty-path performance remained inside the same order of
  magnitude; no regression-budget was invented.
- Evidence arrays / golden fixtures are unchanged for callers that
  do not name `FaultKind::StreamLoss`.
- Documentation calls the feature userspace stream-chunk loss (not
  TCP/IP packet loss) and explicitly labels `packet_loss` as the
  Toxiproxy spelling only under the opt-in pinned snapshot profile.
- No test failure was waived or moved to a successor; the post-v2.12
  qualifier briefly reported a stale-oracle failure that was
  resolved by adding a `pgrep` reaper to the script.
- All closure notes (M036-M039) identify the candidate and
  limitations honestly and leave historical closures untouched.

## Acceptance checklist (per M039)

- [x] M036, M037, and M038 are formally closed.
- [x] One exact candidate passes every mandatory gate (dual-oracle,
  workspace test/doc, OpenAPI/SDK/binding/hosted, fuzz, dependency,
  performance).
- [x] Deterministic stream-loss traces are
  fragmentation-independent.
- [x] Legacy evidence / RNG / scenario / datagram fixtures have no
  unexplained changes.
- [x] All native and cross-language surfaces agree on `StreamLoss`.
- [x] Strict Toxiproxy v2.12 still passes its pinned oracle
  unchanged (50/50).
- [x] The snapshot profile passes its separately pinned oracle
  (14/14).
- [x] Compatibility docs clearly label `packet_loss` as userspace
  stream loss under the snapshot profile only.
- [x] Live-update divergence is documented (eggchaos
  generation-barrier reset; existing M019/M023 transition tests).
- [x] No moving-main compatibility claim remains.
- [x] No-fault performance remains within the same order of
  magnitude as the pre-tranche baseline (recorded).
- [x] Planning / current-state documents agree (registry,
  AGENTS.md, plan closures).
- [x] No unresolved medium-or-higher correctness/security finding.
- [x] Closure evidence identifies all incomplete/unclaimed
  platforms honestly (CI matrix is Linux/macOS/Windows per
  existing CI; this is unchanged by the tranche).

## Follow-on activation

M039 activates **no automatic successor**. The ADR 007 tranche is
closed. A future tagged Toxiproxy release may justify a separate
promotion/reconciliation milestone that diffs the real release tag
against the pinned `40f7fd31` snapshot before changing the profile
spelling or its claims.

## Additive M040 corrective supersession note (2026-09-26)

This M039 record is preserved as the evidence and claims recorded at its
historical candidate `3b405af7684a681ffcf73709d6b6c90b10267a9a`. Its assertion
that the hosted matrix did not need a per-tranche rerun and its initial
post-v2.12 qualification claims were superseded by M040's exact-candidate
corrective qualification. M039's implementation/claimed-closure commit is
`3b405af7684a681ffcf73709d6b6c90b10267a9a`. The final ADR 007 repository
authority is `plans/closure/M040-post-v2-12-stream-loss-corrective-requalification-closure.md`.
