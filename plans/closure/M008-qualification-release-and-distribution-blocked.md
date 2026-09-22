# M008 qualification record — blocked release gate

Candidate audit commit: `5f46825`.

This is a blocked qualification record, not a closure record. M008 is not
released or closed.

## Passing evidence on the candidate

```text
cargo fmt --all -- --check                         PASS
cargo clippy --workspace --all-targets --all-features -- -D warnings  PASS
cargo test --workspace --all-features              PASS (21 tests)
cargo test --manifest-path benchmarks/Cargo.toml   PASS
cargo check --manifest-path fuzz/Cargo.toml        PASS
cargo doc --workspace --all-features --no-deps     PASS
cargo build --workspace --release                  PASS
./scripts/benchmark.sh                              PASS; recorded arm64 baseline
EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh PASS
./scripts/qualify_eggfetch.sh                      PASS
./scripts/qualify_toxiproxy_v2_12.sh               PASS, with differential incomplete
./scripts/release-smoke.sh                         PASS through artifact smoke; dependent package gate blocked
cargo audit --deny warnings                         PASS
cargo deny check advisories licenses bans sources   PASS, duplicate-version warnings only
./scripts/release-artifact-smoke.sh                PASS on native arm64 release binary
cargo package -p eggchaos-core --dry-run            PASS
```

The pinned Toxiproxy v2.12.0 oracle was downloaded and run independently in
this qualification: `toxiproxy-server-darwin-amd64`, SHA-256
`9625bba4bd96117eedae49f982aba4c2f462b268dd406c9ff18186f9b1ef8afe`.
The loopback smoke observed GET `/version` 200, POST `/proxies` 201, POST
`/toxics` 200, and DELETE `/proxies/status` 204. The repository also adds a
release artifact smoke and CI target matrix for Linux x86_64/aarch64, macOS
x86_64/aarch64, and Windows x86_64. Native local builds passed for macOS arm64
and x86_64. Zig-assisted local builds passed for Linux aarch64 and Windows GNU
x86_64; the exact Windows MSVC target still requires its Windows CRT/toolchain
and remains CI evidence.

## Blocking evidence gaps

1. `cargo package -p eggchaos-eggfetch --allow-dirty` fails because the
   versioned `eggchaos-core` package is not published or staged in the target
   registry. The same publish-order constraint applies transitively to
   `eggchaos-server`, `eggchaos-toxiproxy`, and `eggchaos-cli`; the pinned
   EggServe 0.2.0 leaves must also be available. Publishing is an external
   release action and was not performed.
2. The exact Windows MSVC release build was not executable locally because the
   Windows CRT/toolchain is unavailable. A Windows GNU portability build did
   pass; the GitHub release workflow contains the exact MSVC target and still
   requires CI execution.
3. The qualification now has one expanded local performance run and a bounded
   10,000-run plan/configuration fuzz pass, but not repeated cross-platform
   performance, an external consumer package smoke, or a full Toxiproxy
   byte-differential corpus. These cannot be inferred from source or from the
   loopback API smoke.

## Required reassessment

Keep M008 blocked until a release environment can publish or stage the exact
dependency graph, execute the supported target matrix and release artifact
smokes, and collect the remaining performance, soak, consumer, and differential
evidence. Do not create an M008 closure record or tag from this candidate.
