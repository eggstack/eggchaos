# M029 — Composable Transport Chaos Adapter and Evidence — Closure

Status: closed
Exact implementation candidate: `add40b0c6659c68270c4b0b971b8a1187e22a340`
Depends on: M028 (closed at `ceb3bae`), ADR 005 (accepted)

## Objective verdict

M029 refactors the EggFetch integration from a direct-only chaos dialer
into a composable physical-stream impairment layer over an arbitrary
`eggfetch_core::Dialer`, freezes caller-controlled deterministic
physical connection identity, and exposes bounded bidirectional
transport evidence out of band. All acceptance criteria are met on the
exact candidate above; no stop condition fired. M030 becomes ready.

## What changed

- `crates/eggchaos-core/src/stream.rs`: `StreamEvidence` now mirrors
  the full engine counters (delays, segments/slices, buffered bytes,
  termination) instead of bytes/activations only; new bounded
  `DirectionEvidenceSnapshot`, `BidirectionalEvidenceSnapshot`, and
  shareable `LiveBidirectionalEvidence` handle. `BidirectionalChaosStream`
  carries per-direction evidence arcs, mirrors them on every drive
  (including the empty-plan direct path in both directions), tracks
  pending generations and transition counts, and exposes
  `connection_key()`, observed/pending generations, `live_evidence()`,
  `evidence_snapshot()`, and `into_inner`/`get_ref`/`get_mut`.
  No RNG, scheduling, or fault semantic changed; existing golden
  vectors untouched.
- `crates/eggchaos-eggfetch/src/lib.rs`: generic `ChaosDialer<D =
  DirectDialer>` with one `wrap_stream` chaos path. `DirectDialer` owns
  the historical DNS/TCP behavior (10 s default, 300 s max, bounded
  config errors). Inner `DialError` kind/source passes through
  untouched; exactly one inner attempt per dial; ordinals assigned only
  after inner success starting at `INITIAL_CONNECTION_ORDINAL = 1`.
  `ConnectionKeyProvider` over `(ordinal, DialTarget, integration_id)`
  with ordinal-preserving default, bounded `KeyError` (256 bytes), and
  dial-time failure before bytes are exposed. Synchronous
  `ConnectionObserver` fired before Eggfetch handoff; bounded
  `RecordingObserver` with explicit capacity and oldest-first eviction.
  Caller integration identity bounded to 128 bytes at configuration
  time. `ChaosDialer::new` / `with_policies` remain source-compatible.
- `crates/eggchaos-eggfetch/src/tests.rs` (new): 19-test composition,
  identity, evidence, pooling, and H1/H2 client corpus.
- Docs: `docs/eggfetch.md` (composable model, identity, evidence),
  `architecture/eggfetch-integration.md` (§2–§4, §7–§8 rewritten),
  `README.md` (composed-dialer example).

## Verification (exact candidate)

Clean tree at gate time. Pinned toolchain 1.89.0, macOS arm64 local.

- `cargo fmt --all -- --check` — clean.
- `cargo clippy -p eggchaos-core -p eggchaos-eggfetch --all-targets --all-features -- -D warnings` — clean.
- `cargo test -p eggchaos-core --all-features` — 57 + 1 golden ok.
- `cargo test -p eggchaos-eggfetch --all-features` — 19 lib + 10 integration ok.
- `cargo test -p eggchaos-eggfetch --all-features --features http2` — same matrix ok (H2 multiplexing shares one key).
- `cargo test --workspace --all-features` — all suites green (core 57, eggfetch 19+10, server 138+2, toxiproxy 10, others ok).
- `cargo doc --workspace --all-features --no-deps` — clean.
- `./scripts/qualify_eggfetch.sh` — eggfetch + server suites green.

## Acceptance check

- Arbitrary inner dialers decorate without route duplication (fake,
  routed, duplex fixtures; target forwarded exactly; one attempt).
- Direct convenience preserved and source-compatible
  (`tests/regression.rs` unmodified, still green).
- Caller-controlled deterministic identity implemented, golden-tested,
  and documented; default preserves ordinal behavior.
- Bidirectional live/final evidence externally consumable, bounded,
  payload-free, readable after stream drop.
- No `eggreplay-*` / `eggprobe-*` dependency introduced.
- H1 keep-alive reuse = one key; forced separate H1 = distinct keys;
  H2 multiplexed = one key; live publication engages pooled
  connections at the physical transition boundary.
- Existing RNG/stream semantics unchanged (core suite + golden green).

## Residual limitations

- Hard-reset mapping through an arbitrary inner transport is not
  promised (documented non-goal); graceful/hard termination evidence
  is recorded, transport application stays the inner stream's.
- `RecordingObserver` is a test/small-harness collector; production
  consumers implement `ConnectionObserver` over their own sink.
- Observer panics propagate (documented); the stream is dropped before
  Eggfetch handoff, so no handed-over stream can be corrupted.

## Follow-on activation

M030 (`030-consumer-neutral-experiment-harness-and-coordinated-start.md`)
is now ready. M031 remains blocked on M030.
