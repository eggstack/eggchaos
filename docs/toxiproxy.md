# Toxiproxy v2.12 compatibility

The `eggchaos-toxiproxy` crate exposes the Toxiproxy v2.12 route family backed
entirely by native state: every view derives from `ControlState` snapshots
and every mutation goes through the native control authority, so
compatibility presentation cannot drift from what the runtime executes.

Supported API surface (route and shape corpus differential-verified against pinned v2.12.0):

- proxy CRUD plus `POST /proxies/{proxy}` and `PATCH` updates;
- `POST /populate` with oracle keep/replace/create/skip semantics;
- toxic CRUD for all seven v2.12 toxics (`latency`, `bandwidth`,
  `slow_close`, `timeout`, `slicer`, `limit_data`, `reset_peer`),
  with `PATCH` toxic updates;
- `POST /reset` (re-enable all proxies, remove all toxics);
- `GET /version` returning exactly `{"version":"2.12.0"}`.

Deliberate, documented divergences (see
`plans/reference/toxiproxy-parity.md` for the full matrix):

- toxicity outside [0, 1] is clamped to the range;
- degenerate zero `rate`/`average_size`/`bytes` coalesce to 1;
- stream echo is lowercase; toxic order is upstream faults then downstream;
- missing `listen` binds an ephemeral loopback port;
- non-socket `upstream` values and out-of-charset proxy names are rejected
  with a clear 400 (native fixed-target and path-segment invariants);
- `reset_peer` termination is platform-qualified (RST vs FIN not asserted);
- bandwidth, slow_close, and slicer data-plane byte/timing cases now run in
  the oracle corpus with explicit tolerances. The Darwin ARM64 measurement and
  limits are recorded in `plans/reference/toxiproxy-parity.md`; native
  bandwidth was faster than the oracle in the recorded 1 MiB sample, so exact
  sustained timing is not claimed. `GET /metrics` matches the oracle
  (plain-text 404 without metrics flags).

Toxicity is a deterministic per-connection activation probability. Slicer
behavior is stream segmentation, not IP packet loss. The adapter does not
claim current-Toxiproxy `main` extensions such as `packet_loss`.

Run a standalone compat server with
`cargo run -p eggchaos-toxiproxy --example compat_server -- 127.0.0.1:8474`
(loopback by default). Qualification:
`scripts/fetch_toxiproxy_v2_12.sh` acquires the official architecture-matched
binary and verifies its pinned SHA-256. For a mandatory run, set
`TOXIPROXY_SERVER` to that path and run
`EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 ./scripts/qualify_toxiproxy_v2_12.sh`.
