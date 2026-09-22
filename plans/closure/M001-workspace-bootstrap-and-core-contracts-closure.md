# M001 closure — workspace bootstrap and core contracts

Candidate verification commit: `309aeff8da9b1d91b36aecd55c190e12d56e537d`.

## Evidence

The implementation created the five planned crates, one-way dependency
direction, MIT package metadata, a Rust 1.89 toolchain declaration, CI for
Linux/macOS/Windows, architecture/configuration documentation, and the typed
validated `FaultPlan`/`ChaosStream` core contract.

Commands run on the candidate:

```text
cargo fmt --all -- --check                         PASS
cargo clippy --workspace --all-targets --all-features -- -D warnings  PASS
cargo test --workspace --all-features              PASS (3 tests)
cargo doc --workspace --all-features --no-deps     PASS
```

`cargo tree -p eggchaos-core` contains Tokio, Bytes, Serde, and thiserror only;
the core has no HTTP, CLI, EggServe, Eggfetch, or relay dependency. The full
workspace compile also resolved the pinned EggServe git leaf crates and
`eggress-relay` 1.0.7. The empty-plan duplex, EOF, validation, and RNG golden
vector tests pass.

## Acceptance verdict

All M001 acceptance criteria are satisfied. The initial EggServe crates are
temporarily pinned to the inspected EggServe revision because they are not
published in the crates.io index; this is documented and does not block local
workspace implementation. M002 is ready for activation.
