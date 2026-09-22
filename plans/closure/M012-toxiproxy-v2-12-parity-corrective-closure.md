# M012 Closure — Toxiproxy v2.12 Parity Corrective

Candidate: `a040ed7` (code, docs, qualification, corpus, smokes).
Registry: M012 `closed`, M013 `ready` (same change family as this record).

## Acceptance check

- Required v2.12 routes implemented through the M010 control authority:
  all 14 compat routes plus `PATCH` proxy/toxic updates (the pinned Go
  client uses `PATCH` for toxic updates; the oracle accepts both).
- Create/update/delete correspond to real native listener state; port 0
  echoes the actual bound address.
- Populate is a real upsert (keep/replace/create/skip) and reset is a real
  native transaction (re-enable all, strip all toxics).
- Toxic default names (`<type>_<stream>`, empty string included),
  default stream downstream, default toxicity 1.0 match the oracle.
- Invalid stream is rejected with the oracle-verbatim 400.
- Timeout=0 maps to indefinite `Blackhole { close_after: None }`.
- `reset_peer` timeout reaches the corrected disconnect/reset path
  (`Disconnect { hard_reset: true }`); termination observed, RST not
  asserted (platform-qualified).
- Compatibility `/version` returns exactly `{"version":"2.12.0"}`.
- Adapter holds no proxy/toxic definitions; all views derive from
  `ControlState` snapshots with reverse `FaultSpec -> Toxic` translation,
  so native mutation cannot leave compat GET/list stale.
- Differential corpus passes against the live pinned oracle (below).
- Client smokes pass: pinned Go client 13/13, independent Python client
  13/13 (below).
- Parity matrix is truthful and updated, including `incomplete` items that
  are not claimed (bandwidth/slicer/slow_close data-plane timing).
- No second execution registry, no weakened native defaults, no
  current-main features.

## Verification evidence (exact)

All executed 2026-09-22 on darwin (Go toolchain darwin/arm64),
candidate `a040ed7`, oracle `/tmp/oracle/toxiproxy-server`
(`toxiproxy-server version 2.12.0`,
SHA-256 `aa299966b52f16a8594f1cd0d1e9049dc2e8fe2c04a90c19860e2719b2b95d15`):

- `cargo fmt --all -- --check`: PASS.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`:
  PASS, no warnings.
- `cargo test --workspace --all-features`: PASS — core 42, server 37,
  toxiproxy lib 10, cli e2e 1, eggfetch 3, toxiproxy translation 3
  (plus doc-test suites, all ok).
- `TOXIPROXY_SERVER=/tmp/oracle/toxiproxy-server
  ./scripts/qualify_toxiproxy_v2_12.sh`: `{"translation":"pass",
  "oracle":"toxiproxy-server 2.12.0","differential":"pass"}`.
  Corpus summary: 47 passed, 0 failed, with 4 declared normalizations
  (listen placeholders with per-server port assertions; JSON number
  encoding canonicalization; toxicity clamping with separately asserted
  exact values; degenerate-zero coalescing).
- Go smoke `github.com/Shopify/toxiproxy/v2@v2.12.0` (go1.27.1):
  13/13 pass — transcript
  `qualification/toxiproxy-v2-12/client-smoke/go/go_results.json`.
- Python-stdlib smoke (Python 3.14.2, urllib only): 13/13 pass —
  transcript `qualification/toxiproxy-v2-12/client-smoke/py_results.json`.

## Follow-on

M013 becomes ready on this closure.
