# Toxiproxy v2.12 Parity (M012 qualified)

Research date: 2026-09-22; qualification date: 2026-09-22.  
Primary compatibility target: Shopify Toxiproxy v2.12.0 (pinned oracle binary,
SHA-256 `aa299966b52f16a8594f1cd0d1e9049dc2e8fe2c04a90c19860e2719b2b95d15`).
Oracle behavior baseline: `qualification/toxiproxy-v2-12/oracle-baseline-v2.12.0.md`.
Differential corpus: `crates/eggchaos-toxiproxy/tests/differential.rs`
(45 API + data-plane comparisons, all passing against the live oracle).
Client smokes: pinned Go client `github.com/Shopify/toxiproxy/v2@v2.12.0`
(toolchain go1.27.1, 13/13 steps pass) and an independent Python-stdlib client
(13/13 steps pass); transcripts in `qualification/toxiproxy-v2-12/client-smoke/`.

Claim levels: `exact`, `behaviorally compatible` (tolerances noted),
`intent compatible` (documented divergence), `not supported` (fails clearly),
`incomplete` (not yet evidenced; never claimed).

## Version policy

Eggchaos does not claim compatibility with an unversioned moving Toxiproxy
`main`. This file remains the frozen strict-v2.12 parity contract.

ADR 007 + M036–M039 separately plan an opt-in post-v2.12 snapshot profile for
`packet_loss`, pinned to upstream commit
`40f7fd31bee529d824116bd2a11a9e3425e904ec`. That work must not broaden or
rewrite the strict v2.12 claims recorded here.

## Route parity

| Method/path | Level | Notes |
| --- | --- | --- |
| `GET /version` | exact | `{"version":"2.12.0"}` verbatim, differential-verified |
| `GET /proxies` | exact | name-keyed map with `Logger:{}` + embedded toxics |
| `POST /proxies` | exact | 201; duplicate 409; missing name/upstream 400 oracle-verbatim; malformed body 400 (message text differs, status exact); bind conflict 500 (shape matches, OS text differs) |
| `POST /populate` | exact | upsert: same listen+upstream keeps proxy untouched; changed addresses replace (toxics drop); unknown names created; bind failures skipped; empty list echoes `{"proxies":null}`; missing entry name 400 oracle-verbatim with 1-based index |
| `GET /proxies/{proxy}` | exact | 404 envelope oracle-verbatim |
| `POST /proxies/{proxy}` | exact | listen/upstream restart-class, enabled lifecycle; invalid addresses 500 per oracle |
| `PATCH /proxies/{proxy}` | exact | same as POST (the pinned Go client only needs POST here, but the oracle accepts both) |
| `DELETE /proxies/{proxy}` | exact | 204 empty; 404 envelope |
| `GET /proxies/{proxy}/toxics` | exact | array from live native snapshots |
| `POST /proxies/{proxy}/toxics` | exact | 200; duplicate 409; invalid type/stream 400 oracle-verbatim; missing proxy 404 |
| `GET /proxies/{proxy}/toxics/{toxic}` | exact | 404 envelope oracle-verbatim |
| `POST /proxies/{proxy}/toxics/{toxic}` | exact | toxicity + same-type attributes applied; type/stream ignored |
| `PATCH /proxies/{proxy}/toxics/{toxic}` | exact | same as POST (used by the pinned Go client) |
| `DELETE /proxies/{proxy}/toxics/{toxic}` | exact | 204 empty; 404 envelope |
| `POST /reset` | exact | re-enables every proxy and removes every toxic via the native authority |
| `GET /metrics` | exact | plain-text 404, matching the oracle without metrics flags; no fabricated counters |
| unknown routes | exact | plain-text `404 page not found` |

Error envelopes are `{"error","status"}` served as `text/plain`, matching the
oracle; success bodies are `application/json` (version carries the oracle's
`charset` suffix verbatim).

## Toxic parity

| Toxic | Level | Notes |
| --- | --- | --- |
| `latency` | behaviorally compatible | ms/jitter mapping; byte preservation + delay differential-verified (200 ms configured, >=150 ms observed, exact bytes) |
| `bandwidth` | intent compatible | rate unit KiB/s; oracle/native both preserve 1 MiB and pace within 1–10 s and a 3x relative timing window. Darwin ARM64 sample at `rate=256`: oracle 4.156 s, native 1.898 s. Distinct Go/native write scheduling means exact sustained timing is not claimed; native was faster in this sample. |
| `slow_close` | intent compatible | 300 ms close delay observed after exact echo bytes; Darwin ARM64 sample: oracle 302.432 ms, native 302.184 ms; accepts ≥200 ms (100 ms tolerance). |
| `timeout` | behaviorally compatible | timeout=0 maps to indefinite `Blackhole { close_after: None }`; blocking + post-removal flow differential-verified |
| `reset_peer` | intent compatible | maps to delayed `Disconnect { hard_reset: true }`; termination observed on darwin/arm64 in the differential run, but RST vs FIN is platform-dependent and not asserted |
| `slicer` | intent compatible | 64 KiB byte preservation and pacing observed with average 256 / delay 1000 µs; Darwin ARM64 sample: oracle 320.382 ms, native 256.933 ms. Timing comparator uses ≥20 ms and ≤3x between implementations; exact Go random sequence/chunk boundaries are not claimed. |
| `limit_data` | behaviorally compatible | exact 100/1000-byte boundary + termination differential-verified |

Defaults: omitted attributes zero-fill per type (differential-verified);
absent or empty names default to `<type>_<stream>` (lowercase stream);
stream defaults to downstream; toxicity defaults to 1.0.

## Recorded divergences

- Toxicity outside [0, 1] is clamped (oracle echoes verbatim). Runtime
  effect is preserved (always/never applies); exact echo values asserted in
  the corpus.
- Degenerate zero numerics that native `NonZero` bounds cannot represent
  coalesce to 1: bandwidth `rate`, slicer `average_size`, limit_data `bytes`.
- Stream echo is lowercase; the oracle preserves exotic input case.
- Missing `listen` binds an ephemeral loopback port; the oracle binds an
  ephemeral wildcard port (native loopback-by-default invariant wins).
- Non-socket `upstream` values: `not supported` (clear 400) — the oracle
  stores arbitrary strings, but the native fixed-target invariant requires a
  socket address.
- Proxy names outside `[A-Za-z0-9._-]`: `not supported` (clear 400) — native
  path-segment invariant; the oracle is laxer.
- Malformed-body and bind-conflict message text is OS/implementation
  specific; statuses and envelope shapes are exact.
- Toxic presentation order is upstream faults then downstream faults;
  differential comparison sorts by name (creation-interleaved order does not
  round-trip through directional native plans).

## State authority

The adapter holds no proxy or toxic definitions. All views derive from
`ControlState` snapshots (native plans plus actual bound addresses); all
mutations go through the M010 control authority (`create_proxy`,
`import_definition`, `update_proxy`, `set_enabled`, `delete_proxy`,
`add/update/remove_fault`, `reset`). Native `/v1` mutations are immediately
visible through reverse `FaultSpec -> Toxic` translation.
