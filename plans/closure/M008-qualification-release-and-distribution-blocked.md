# M008 qualification record — blocked release gate

Candidate audit commit: `4a1f992b9f540453e219b14d7ffbacdb5306913a`.

This is a blocked qualification record, not a closure record. M008 is not
released or closed.

## Passing evidence on the candidate

```text
cargo fmt --all -- --check                         PASS
cargo clippy --workspace --all-targets --all-features -- -D warnings  PASS
cargo test --workspace --all-features              PASS (20 tests)
cargo doc --workspace --all-features --no-deps     PASS
cargo build --workspace --release                  PASS
./scripts/qualify_eggfetch.sh                      PASS
./scripts/qualify_toxiproxy_v2_12.sh               PASS, oracle unavailable and differential incomplete
```

The pinned Toxiproxy oracle identity and earlier loopback API smoke remain in
`M006-toxiproxy-v2-12-compatibility-closure.md`; this environment no longer
has that downloaded binary available for this rerun.

## Blocking evidence gaps

1. `cargo package -p eggchaos-eggfetch --allow-dirty` and
   `cargo package -p eggchaos-server --allow-dirty --no-verify` cannot resolve
   the local `eggchaos-core` dependency from crates.io. The repository now has
   versioned package metadata and the core crate packages successfully, but a
   real publish-order qualification requires publishing `eggchaos-core` first
   (and the pinned EggServe 0.2.0 leaves must also be available in the target
   registry). Publishing is an external release action and was not performed.
2. `cargo-audit` and `cargo-deny` are not installed in this environment, so the
   advisory/license gates are incomplete.
3. The release workflow runs qualification on Ubuntu only; Linux aarch64,
   macOS x86_64/aarch64, and Windows release binaries/checksums and artifact
   smokes were not executed here.
4. No repeated performance baseline, bounded fuzz/property soak, external
   consumer package smoke, or full Toxiproxy byte-differential corpus was
   executed. These are required by M008 and cannot be inferred from source.

## Required reassessment

Keep M008 blocked until a release environment can publish or stage the exact
dependency graph, run the advisory/license tools, execute the supported target
matrix and release artifact smokes, and collect the remaining qualification
evidence. Do not create an M008 closure record or tag from this candidate.
