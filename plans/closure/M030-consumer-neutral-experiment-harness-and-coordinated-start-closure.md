# M030 — Consumer-Neutral Experiment Harness and Coordinated Start — Closure

Status: closed
Exact implementation candidate: `0bc45f03e1a944a957cd80edabcf4489c9f4afbc`
Depends on: M029 (closed at `add40b0`), ADR 005 (accepted)

## Objective verdict

M030 creates the narrow, consumer-neutral experiment layer behind a
new `eggchaos-experiment` crate, reusing the qualified Scenario V2
compiler/fingerprint/isolation/cleanup model with no semantic change,
and adds the prepare/arm/start lifecycle with one shared Tokio
monotonic epoch. All acceptance criteria are met on the exact
candidate; no stop condition fired. M031 becomes ready.

## What changed

- New crate `eggchaos-experiment` (deps: `eggchaos-core`, `serde`,
  `sha2`, `thiserror`, `tokio`, `tokio-util` only — no server,
  EggServe, CLI, Toxiproxy, EggReplay, or EggProbe dependency):
  - `action/source/compiler/error/fingerprint/run`: Scenario V2
    semantic authority moved verbatim from `eggchaos-server`
    (git renames; `fingerprint.rs`/`error.rs`/`source.rs`
    byte-identical, `compiler.rs`/`run.rs` doc-comment/import-path
    only). Golden fingerprints, namespace vectors, and corpus
    digests unchanged.
  - `target.rs`: narrow `PolicyTarget` contract (`capabilities`,
    `snapshot`, `current_generation`, expected-generation `publish`,
    `global_generation`) with bounded `TargetError` categories and
    no payload/product model; `action_resource`/`resource_key`
    helpers.
  - `driver.rs`: the single shared schedule driver (absolute
    `epoch + offset` deadlines with `checked_add`, strict/live
    ownership, generation-guarded restore/leave cleanup, async
    `EventSink`, prompt cancellation including pre-start
    cancellation). Failure message vocabulary matches the previous
    server driver.
  - `gate.rs`: `EpochGate`/`EpochWaiter` capturing one monotonic
    `Instant` exactly once (retained state, late waiters resolve
    immediately). Schedule-clock synchronization only; documented
    non-claims for kernel/application timing and cross-process sync.
  - `experiment.rs`: `PreparedExperiment::prepare/prepare_compiled`
    (compile, capability check, touched-resource resolution, initial
    snapshots; no publication, no clock), `run/run_from_epoch/
    run_with_workload`, bounded `ExperimentEvidence` and caller
    integration identity (128 bytes).
  - `stream_target.rs`: `StreamPolicyTarget` mapping one resource
    name to an M029 `LivePolicy` pair (stream-only; datagram is typed
    `UnsupportedCapability`).
  - `tests.rs`: 23 tests (paused-time drift/order, gate sharing,
    prepare/cancel/cleanup/strict/live conformance, bounds).
- Server: `ScenarioAction` authority moved to the experiment crate
  (`scenario.rs` re-exports); `scenario_v2/{source,compiler,error,
  fingerprint,run}` removed in favor of compat re-exports;
  `scenario_v2/runtime.rs` rewritten as `ControlStateTarget` adapter
  + run-record sink delegating to the shared driver (no second state
  store; canonical proxy state and global generations still flow
  through `ControlState` methods); new
  `scenario_v2/conformance_tests.rs` proving server and in-process
  targets produce equivalent event/generation outcomes.
- Planning/docs: `architecture/scenario-observability.md` (§1.8–1.10
  + Owner), `scripts/release-smoke.sh` (experiment package +
  `core->experiment/eggfetch->server/toxiproxy/cli` order proof),
  `AGENTS.md` publish order.
- Workspace: `eggchaos-experiment` member added; server depends on it
  with the `0.1.0` registry req.

## Verification (exact candidate)

Clean tree at gate time. Pinned toolchain 1.89.0, macOS arm64 local.

- `./scripts/check.sh` — exit 0 (fmt, workspace clippy `-D warnings`,
  full workspace tests incl. core 57, eggfetch 19+10, experiment 23,
  server 141 lib + corpus, toxiproxy 10, doc).
- Focused re-run on the candidate: experiment 23 ok; server
  `scenario_v2` lib 69 ok (incl. 3 conformance); `schedule_corpus` 2
  ok (golden byte-for-byte).
- `./scripts/release-smoke.sh` — exit 0, order proof
  `core->experiment/eggfetch->server/toxiproxy/cli`.
- `./scripts/qualify_eggfetch.sh` — green (M029 behavior preserved).
- `cargo tree -p eggchaos-experiment`: only core + serde/sha2/
  thiserror/tokio/tokio-util. No `eggreplay-*`/`eggprobe-*` anywhere.

## Acceptance check

- Embedded harnesses use Scenario V2 without the server/admin stack.
- Server public behavior, re-exports, and fingerprint corpus compatible.
- Expected-generation target abstraction exists; both targets share
  one driver with identical strict/live/cleanup semantics.
- One process-local epoch shared with the caller workload
  (paused-time proofs, no drift, compiled order).
- Unsupported datagram requirements fail explicitly on stream-only
  targets (prepare-time, nothing published).
- Cancellation/shutdown leaves no detached task or stale state
  (caller-owned futures; server keeps its supervised JoinSet path).
- Evidence bounded and consumer-neutral; no EggReplay/EggProbe
  production dependency.

## Residual limitations

- `StreamPolicyTarget` is stream-only by design; datagram harnesses
  need a future datagram-capable target (out of scope).
- The captured epoch is process-local; cross-process coordination is
  explicitly not claimed.
- `RecordingObserver` (M029) correlation with experiment evidence is
  demonstrated in M031, not wired automatically here.

## Follow-on activation

M031 (`031-integration-boundary-qualification-and-downstream-handoff.md`)
is now ready. EggReplay/EggProbe downstream plans remain separate
work and must not modify either sibling repository.
