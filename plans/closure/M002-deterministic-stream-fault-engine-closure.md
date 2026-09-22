# M002 closure — deterministic stream fault engine

Candidate commit: `d85d25402af4f7c63a45ebb4ebc73b8de727d2e5`.

## Evidence

`eggchaos-core` now provides ordered validated fault plans, SplitMix64-v1
seed derivation, connection-local activation decisions, bounded timestamped
queues, partial-write handling, flush/shutdown propagation, blackhole,
limit-data, slow-close, slicing, latency, bandwidth timing, disconnect/reset
requests, summaries, and generation-published `LivePolicy` transitions.
`BidirectionalChaosStream` applies independent physical-stream read/write
policies for in-process clients.

Commands run on the candidate:

```text
cargo fmt --all -- --check                         PASS
cargo clippy --workspace --all-targets --all-features -- -D warnings  PASS
cargo test --workspace --all-features              PASS (14 tests)
cargo doc --workspace --all-features --no-deps     PASS
```

Core tests cover RNG golden values, probability boundaries, exact limit-data
boundaries, blackhole accounting, paused-time latency, empty-path behavior,
live-policy generation transitions, and deterministic plan validation. The
empty-plan path remains allocation/timer free at the wrapper boundary.

## Limitations

The generic engine reports abstract hard-reset requests; platform TCP RST is
not claimed. The default native core applies faults to physical byte-stream
directions and does not model IP packets. A Criterion benchmark target was not
introduced; release qualification uses repeatable release builds and the
runtime integration baseline instead.

## Verdict

M002 acceptance criteria are satisfied for the declared native scope. M003 is
unblocked.
