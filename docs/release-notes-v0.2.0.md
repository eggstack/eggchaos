# Eggchaos v0.2.0 release notes

Status: **published**. `v0.2.0` is the current Eggchaos release
(annotated tag `v0.2.0` peeling to frozen candidate
`b6a277d5ad4267bd602bc15a4333b14322057b90`, GitHub Release
`eggchaos v0.2.0`). M058 closed the publication on `b6a277d` with
eight crates.io publishes, a tag-triggered release workflow (tag CI
`15/15` + tag release `7/7`), a GitHub Release with five binaries +
SHA-256 sidecars, and fresh-install verification. M059 closed the
pre-publication hardening corrective on `1409d0f` (hosted CI
`15/15` + release dispatch `7/7`). Language registries are
separately gated (see below) and remain deferred.

## Required release surface (published)

- Rust crates.io graph at `0.2.0` (publish order executed):
  `eggchaos-core -> eggchaos-experiment -> eggchaos-eggfetch ->
  eggchaos-protocol -> eggchaos-server -> eggchaos-toxiproxy ->
  eggchaos-cli -> eggchaos-embed`.
- Git tag: annotated `v0.2.0` on frozen candidate `b6a277d`.
- GitHub Release: `eggchaos v0.2.0` on `v0.2.0` with five binaries +
  five `.sha256` sidecars:
  `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`,
  `x86_64-apple-darwin`, `aarch64-apple-darwin`,
  `x86_64-pc-windows-msvc`.
- Stable install:
  `cargo install eggchaos-cli --version 0.2.0 --locked`.

## What is new since v0.1.0 (supported outcomes)

- **UDP/datagram chaos (ADR 003, M020–M025):** fixed-target UDP
  listeners with per-client associations (own connected upstream
  socket each), whole-datagram delay/jitter/loss/duplication/
  reorder-by-hold/corruption/bandwidth, bounded deadline scheduling,
  explicit drop-newest overflow, native
  `/v1/datagram-proxies` + `/v1/datagram-associations`, TOML
  `[[datagram_proxies]]`, CLI `datagram` commands, scenario actions,
  metrics/evidence, exact golden traces, measured no-fault budget
  (M023 direct + M024 topology-matched).
- **Deterministic Scenario V2 schedules (ADR 004, M026–M028):**
  bounded source language (named phases, finite repetition,
  strict/live isolation, restore/leave cleanup), deterministic
  compiler to an inspectable event tape, canonical SHA-256 schedule
  fingerprint, run_id-independent v2 seed namespace, epoch-anchored
  absolute deadlines, generation-guarded cleanup. ScenarioV1 remains
  a compatibility surface.
- **Consumer-neutral experiment/integration substrate (ADR 005,
  M029–M031):** `eggchaos-eggfetch::ChaosDialer` composes over an
  arbitrary caller-selected `Dialer` with caller-controlled physical
  connection identity and bounded bidirectional evidence;
  `eggchaos-experiment` owns Scenario V2 semantics plus a
  prepare/arm/start lifecycle with one shared Tokio monotonic epoch.
  No `eggreplay-*`/`eggprobe-*` dependency; downstream EggReplay /
  EggProbe adoption is separate.
- **Python/TypeScript remote control SDK source surfaces (ADR 006,
  M032–M033, qualified M035):** stdlib-only Python sync/async and
  zero-dep TypeScript clients over the exact native `/v1` contract
  (36 operations), drift-checked from
  `api/openapi/eggchaos-v1.yaml`. Daemon lifecycle is out of scope.
- **Python native embedding pilot status (ADR 006, M034–M035):**
  safe `eggchaos-embed` facade plus alpha PyO3/maturin
  `eggchaos-native` (abi3) pilot. Remote/native conformance is
  green; generic C ABI is an explicit no-go pending a separate ADR
  and second-consumer demand.
- **Deterministic stream-loss / opt-in post-v2.12 `packet_loss`
  compatibility (ADR 007, M036–M041):** native `stream-loss` is
  fragmentation-independent 32 KiB logical-grain loss with burst
  correlation and additive evidence. Strict Toxiproxy v2.12 stays
  default/frozen (seven toxics, 50/50 differential vs pinned
  v2.12.0). The post-v2.12 spelling `packet_loss` is opt-in only,
  pinned to upstream `40f7fd31`.
- **Performance/provenance/tooling hardening (M042–M048 plus
  M049–M054 corrective tranche, M055–M057 release baseline):**
  corrected stream/datagram benchmark authority with pre-warmed
  scale and hot-with-idle profiles, measured stream/datagram
  optimizations with no-op dispositions where thresholds were not
  met, shared provenance schema v1 with clean/dirty policy and
  `EGGCHAOS_BENCH_REQUIRE_CLEAN=1` guard, hosted Tier A + Tier B +
  structural CI integration, core deserialization/plan-invariant
  hardening, scenario-lifecycle and native-operation authority
  consolidation, module-boundary hygiene, planning-status and
  version-coherence drift guards, and the `release-contract`
  workflow gate.
- **Pre-release security/dependency hardening (M059):** workspace
  lint policy active on all eight crates, first-party Eggstack
  graphs reconciled within existing compatibility lines
  (relay 1.0.11 / eggfetch-core 0.2.1 / primitives 0.2.2 /
  server 0.2.1), every CI/release action SHA-pinned with
  least-privilege permissions, scheduled audit plus PR review plus
  Dependabot monitoring, deterministic npm/exact-Python CI installs,
  narrowed PyO3 unsafe boundary, `cargo-semver-checks` API gate
  green against M057, `SECURITY.md` private-reporting policy, and
  Sigstore attestations on release binaries (SHA-256 sidecars
  remain the primary checksum contract). No API, route, CLI,
  RNG, fault, scenario, or compatibility behavior change.
- **0.2.0 correctness hardening (owner-authorized, post-M057):**
  validation tightening (connection/relay/datagram bounds,
  `additional_copies 1..=16`, proxy-name checks, unknown-field
  rejection on flattened DTOs), insecure non-loopback admin now
  fails fast at config parse, 1 MiB config-file cap with
  metadata+take double guard, vectored-pending/termination/flush
  barrier fixes, poison-safe locks, epoch-gate lost-wakeup fix,
  atomic dual-publish/admit, JoinSet reaping, plus deterministic
  contract clarifications below.

## Deterministic-contract clarifications included in 0.2.0

Owner-authorized for this release (M058 WP1 decision: publish with
fixes):

- `derive_seed` now domain-separates `(proxy, fault)` with an
  explicit separator plus length prefixes, so pairs like
  `("ab","c")` vs `("a","bc")` cannot collide. Golden seed vector
  for `(42, "proxy", 7, upstream, "latency")` changes from
  `11882912530514077282` to `1461454122856154019`.
- `DeterministicRng::below(0)` now advances state before returning
  `0`; `bernoulli` rejects NaN and uses strict `< threshold`.
- Scenario V2 fingerprint encoding now includes
  `total_duration_ns` alongside seed/execution_key/event tape.
- These change replay values versus pre-0.2.0 development
  snapshots. Within `0.2.0`, replay is exact for policy/per-key
  decisions from versioned seeds; live connection-key timing still
  depends on accept order.

## Compatibility and security posture (unchanged boundaries)

- Native HTTP remains `/v1` (36 operations, 21 paths);
  `GET /metrics` stays unprefixed Prometheus text.
- Config schema remains version 1; deterministic RNG stays
  SplitMix64-v1; provenance schema stays v1.
- **Strict/default Toxiproxy v2.12 compatibility** is frozen and
  default. **Opt-in pinned post-v2.12 profile** (`packet_loss` at
  `40f7fd31`) is not a claim against moving upstream `main`.
- Native userspace `stream-loss` is byte-chunk dropping; it is not
  IP/TCP packet loss and never reuses ADR 003 datagram-loss
  semantics.
- Fixed-target proxy only: never a forward proxy, no TLS
  interception/MITM, no IP-layer impairment.
- Admin binds loopback by default; non-loopback requires explicit
  public-admin opt-in plus bearer token; bodies capped at 1 MiB;
  all queues/histories/connections bounded with backpressure
  (`Pending`) as the default overflow.
- `reset_peer`/hard-reset is best-effort and platform-qualified
  (RST vs FIN not asserted); ordinary `poll_shutdown` is never
  advertised as TCP RST.
- MSRV: Rust 1.89+.
- Release-correctness lockfile: transitive `yoke-derive 0.8.3 ->
  0.8.4` (yanked-version replacement only; no direct dependency
  change, no API/RNG/fault change from this bump).

## Language-package decisions (M058 WP1/WP5/WP9)

| Package | Registry | M058 disposition |
| --- | --- | --- |
| `eggchaos-client` (Python remote) | PyPI | **defer** — owner decision 2026-10-01: the name is unclaimed on PyPI (404) and M058 does not allocate a new public namespace implicitly; source at the `v0.2.0` tag stays versioned `0.2.0` |
| `@eggstack/eggchaos-client` (TypeScript remote) | npm | **defer** — source at the `v0.2.0` tag stays versioned `0.2.0`; no npm upload absent explicit scope/ownership/provenance go |
| `eggchaos-native` (Python native pilot) | PyPI | **defer by default** — alpha pilot without a release-grade cross-platform wheel matrix; host-native build evidence does not imply broad wheel support |

Deferral does not block the required Rust/GitHub release and does
not change aligned `0.2.0` source metadata. Do not install these
packages from a registry at `0.2.0` until a later gated publication
says otherwise.

## Supported targets and installs

- Rust MSRV 1.89+.
- Binaries: five triples above, filenames encode `v0.2.0` plus the
  target triple; each has a SHA-256 sidecar. Each binary also
  carries a Sigstore provenance attestation verifiable with
  `gh attestation verify <binary> --repo eggstack/eggchaos`;
  checksums remain the primary artifact contract.
- After publication: `cargo install eggchaos-cli --version 0.2.0
  --locked`, then `eggchaos --json version`, `--help`, minimal TCP
  + UDP proxy startup/health, and one native control operation.
  Fresh-install verification was exercised against the v0.2.0
  artifacts (see M058 closure evidence).
- External consumer fixtures cover at least `eggchaos-core`,
  `eggchaos-server`, `eggchaos-eggfetch`, `eggchaos-toxiproxy` at
  `0.2.0`.
