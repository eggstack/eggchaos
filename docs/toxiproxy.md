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
- slicer `average_size` omitted/0 maps to 1 on the compat path while the
  native default is 1024 (`NATIVE_DEFAULT_SLICE_AVERAGE_SIZE`): compat and
  native defaults do not round-trip by design;
- native nanosecond-precise durations export through `as_millis()` (latency,
  jitter, timeout, slow_close, reset_peer) and `as_micros()` (slicer delay),
  truncating sub-unit precision on the compat view;
- native bytes/s exports as oracle KiB/s via `(bps + 512) / 1024` (min 1),
  losing sub-KiB precision on round-trip;
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
behavior is stream segmentation, not IP packet loss. The adapter exposes
two opt-in compatibility profiles:

- `strict-v2.12` (default): the frozen v2.12 toxic surface above. Rejects
  the post-v2.12 `packet_loss` toxic as `400 invalid toxic type`. `GET
  /version` returns exactly `{"version":"2.12.0"}`.
- `post-v2.12-2026-09-25`: an opt-in profile pinned to the upstream
  source commit `40f7fd31bee529d824116bd2a11a9e3425e904ec` from
  `Shopify/toxiproxy`. Adds `packet_loss` (with `loss_rate` and
  `correlation`) translated into the native `stream-loss` primitive,
  reports `{"version":"git"}` (the source-build oracle identity), and
  never claims equivalence with a moving upstream `main`.

Under both profiles the adapter remains stream/TCP-only; UDP datagram
resources are a separate native API.

Run a standalone compat server with the strict profile with
`cargo run -p eggchaos-toxiproxy --example compat_server -- 127.0.0.1:8474`
(loopback by default). The example accepts an explicit profile as the
second positional argument (`strict-v2.12` or `post-v2.12-2026-09-25`).
Native fixed-target UDP datagram resources are a separate API and do not extend
Toxiproxy v2.12; the compatibility adapter remains stream/TCP-only. Contributor
oracle setup and qualification commands are documented in
[verification and qualification](../architecture/verification-qualification.md).
