# M007 closure — Eggfetch in-process integration

Candidate implementation commit: `eb52ecd2a2a28e06171bbdf96c3ef4947b8d3eb8`.

## Contract and dependency evidence

- `eggchaos-eggfetch::ChaosDialer` implements the public `eggfetch_core::Dialer`
  seam from `eggfetch-core` 0.2.0.
- The adapter performs bounded direct DNS/TCP dialing only. Eggfetch retains
  HTTP framing, pooling, TLS/SNI, certificate policy, retries, and bodies.
- Default features use Eggfetch native HTTP/1.1 plus Rustls/native roots.
  `eggchaos-eggfetch/http2` is an explicit qualification feature and adds only
  Eggfetch's HTTP/2 transport. No server/admin crate, FFI, Python, Node, H3,
  or proxy-routing dependency is used by the adapter.
- `docs/eggfetch.md` records physical-connection pooling, HTTP/2 shared-stream,
  live-policy, timeout, and security-boundary semantics.

## Verification

The following passed on the candidate implementation tree:

```text
cargo fmt --all -- --check                         PASS
cargo test --workspace --all-features              PASS (20 tests)
cargo clippy --workspace --all-targets --all-features -- -D warnings  PASS
cargo doc --workspace --all-features --no-deps     PASS
./scripts/qualify_eggfetch.sh                      PASS
cargo test -p eggchaos-eggfetch --features http2 --all-targets  PASS (3 tests)
```

The adapter tests cover raw direct dialing, two sequential HTTP/1.1 requests
through one accepted physical connection, and HTTPS/HTTP/2 over a local
Rustls server with Eggfetch's explicit TLS policy. Core tests cover live
upstream and downstream policy generation changes, including a downstream
policy published after an initially empty policy.

## Limitations carried into M008

This closes the adapter contract and its deterministic stream boundary. The
release gate must still expand evidence for streamed request bandwidth,
mid-response termination, blackhole/timeout and retry interactions, invalid
certificate failure, concurrent multi-stream HTTP/2 behavior, and full
redaction assertions. The local TLS qualification intentionally accepts the
self-signed test certificate to prove TLS remains above the chaos stream; it
does not claim the certificate-failure matrix is complete.

## Verdict and transition

M007 is closed for the implemented public Dialer contract and documented
physical-stream semantics. M008 is unblocked and activated as `ready`; its
release qualification must resolve the listed evidence gaps before any release
claim or tag.
