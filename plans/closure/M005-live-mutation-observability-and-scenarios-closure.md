# M005 closure — live mutation, observability, and scenarios

Candidate commit: `250494fc7d408c3f86933f44437d454864519984`.

## Evidence

`LivePolicy` publishes immutable validated generations with atomic publication;
`ChaosStream` and `BidirectionalChaosStream` drain accepted old-generation
bytes before compiling the new generation. `ControlState` exposes the shared
mutation authority, active connection snapshots, cancellation-backed kill,
bounded scenario v1, final evidence fields, and low-cardinality Prometheus
text for accepted/completed connections and configuration generation.

Commands:

```text
cargo fmt --all -- --check                         PASS
cargo clippy --workspace --all-targets --all-features -- -D warnings  PASS
cargo test --workspace --all-features              PASS (16 tests)
cargo doc --workspace --all-features --no-deps     PASS
```

The core live-policy test proves an already wrapped stream observes a published
generation after its queued bytes drain; the server scenario test proves an
ordered zero-time event produces exactly one next generation. No payload bytes
are stored in snapshots/metrics. Shutdown owns listener and connection task
lifetime.

## Verdict

M005 acceptance criteria are satisfied for the bounded native scope. M006 and
M007 are both unblocked.
