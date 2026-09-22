# M013 Closure — Corrective Requalification Gate: CLEAN VERDICT

Candidate: `9904490` (code, tests, docs). Closure/registry follow in the
same change family; they touch planning records only, no code.
Registry: M013 `closed`, M008 `ready` — implementation correctness is
requalified; remaining M008 work is release/package/target/performance
evidence.

## Reconciliation

| Historical area | Corrective plan | Current exact-commit verdict |
| --- | --- | --- |
| M002 core faults | M009 | pass — 43 core tests (fault state machines, byte conservation, half-close, backpressure, RNG goldens, 0/1/intermediate probability, combined latency+bandwidth+slice, bidi read-termination regression) + proptest 64 cases + plan_json fuzz 30.4M execs, 0 crashes |
| M003/M004 runtime/control | M010 | pass — 37 server tests (lifecycle, bind conflicts, restart rollback, fault CRUD/live sync, kill paths, shutdown, contention, metrics reconcile) + CLI `--json` e2e |
| M005 live/scenario | M011 | pass — generation/seed/evidence coherence, scenario replay/cancel/shutdown/failure tests, metrics reconciliation test |
| M006 Toxiproxy | M012 | pass — 47/47 differential vs pinned v2.12.0 oracle + Go/pinned-client and Python smokes 13/13 each |
| M007 Eggfetch | impacted by M009/M011 | pass — 3 adapter tests + 10 new regression tests (H1 keep-alive live update, HTTPS trust/rejection, H2 concurrency, blackhole, mid-response termination, shaping, redial, dial errors, disconnect termination) |

## Verification evidence (candidate `9904490`, 2026-09-22, darwin)

- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`: PASS.
- `cargo test --workspace --all-features`: PASS — core 43, server 37,
  toxiproxy lib 10, cli e2e 1, eggfetch lib 3 + regression 10,
  toxiproxy translation 3 (doc suites ok).
- `cargo doc --workspace --all-features --no-deps`: PASS.
- `cargo build --workspace --release`: PASS.
- `cargo audit --deny warnings`: exit 0, no vulnerabilities (205 deps).
- `cargo deny check advisories licenses bans sources`: all ok.
- Rust 1.89.0 toolchain (MSRV target): gate executed at 1.89.0.
- `TOXIPROXY_SERVER=/tmp/oracle/toxiproxy-server
  ./scripts/qualify_toxiproxy_v2_12.sh`: differential pass, 47 passed,
  0 failed, 4 declared normalizations; oracle `toxiproxy-server
  version 2.12.0`, SHA-256
  `aa299966b52f16a8594f1cd0d1e9049dc2e8fe2c04a90c19860e2719b2b95d15`.
- Go smoke `github.com/Shopify/toxiproxy/v2@v2.12.0` (go1.27.1): 13/13.
  Python-stdlib smoke (3.14.2): 13/13.
- Fuzz `plan_json` (nightly libfuzzer): 30,441,080 execs in 241 s,
  126,311 exec/s, 13,641 new units, 0 crashes, artifacts empty.
- Benchmarks (8 MiB, 3 rounds): bare relay 1934 MiB/s, empty plan 3939,
  latency 2269, bandwidth 2975, slice 3923, combined 1753, eggfetch empty
  3206 MiB/s. No-fault paths meet/exceed bare relay: no gross regression.
  No prior baseline file exists; these numbers are the new record.
- Doc census: route inventory + CLI inventory added to
  `docs/control-plane.md`; eggfetch dialer doc corrected
  (`BidirectionalChaosStream`, live-update/termination semantics);
  README verified accurate (pre-release, gate-open wording intact);
  parity matrix current.

## Narrow fixes (WP7)

Two qualification defects, both small and architecture-neutral:

1. `BidirectionalChaosStream::poll_read` served flushed output only via
   re-looping into another inner read: a peer that closed after writing
   stranded flushed bytes behind a premature EOF. Fixed by serving
   flushed output before the next inner read. Regression test
   `downstream_reads_survive_peer_close_after_write`.
2. Dialer-embedded bidi streams never surfaced engine termination (no
   runtime polls their termination handle): graceful disconnects,
   limit-data exhaustion, and finite-blackhole deadlines passed full
   bodies through. Resolved terminations now surface as EOF (graceful)
   or `ConnectionReset` errors (hard reset) once the queue drains.
   Covered by the eggfetch disconnect/mid-response/blackhole tests.

No substantial new design issue was found; no M014 was needed.

## Follow-on

M008 is `ready`: implementation correctness requalified, remaining work
is release/package/target/performance evidence. Platform matrix beyond
darwin (Linux/Windows CI) remains M008 evidence per the roadmap.
