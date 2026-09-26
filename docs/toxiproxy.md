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
Qualification:

- Strict: `scripts/fetch_toxiproxy_v2_12.sh` acquires the official
  architecture-matched binary and verifies its pinned SHA-256; for a
  mandatory run, set `TOXIPROXY_SERVER` to that path and run
  `EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1
  ./scripts/qualify_toxiproxy_v2_12.sh`.
- Post-v2.12: `scripts/fetch_toxiproxy_post_v2_12.sh` fetches the
  pinned `40f7fd31` source archive, verifies its committed SHA-256, and
  builds the oracle from source with the recorded Go toolchain. The
  fetcher stdout contract (M041):

  ```sh
  # default and --path-only: prints the executable path on stdout so
  # documented command substitution keeps working.
  TOXIPROXY_POST_V2_12_SERVER="$(./scripts/fetch_toxiproxy_post_v2_12.sh)"
  TOXIPROXY_POST_V2_12_SERVER="$(./scripts/fetch_toxiproxy_post_v2_12.sh --path-only)"

  # --json prints one metadata record on stdout (requested/resolved
  # toolchain, source commit, source SHA-256, oracle path, -version).
  ./scripts/fetch_toxiproxy_post_v2_12.sh --json
  ```

  For a mandatory run, set `TOXIPROXY_POST_V2_12_SERVER` to that path
  and run `EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1
  ./scripts/qualify_toxiproxy_post_v2_12.sh` (which consumes the path
  via an explicit `--path-only` fetch mode). Recorded divergences from
  the live oracle (verbatim out-of-range `loss_rate`/`correlation`
  storage, mixed int/float JSON acceptance, `{"version":"git"}` instead
  of a numbered release tag) are classified in
  `plans/closure/M038-pinned-post-v2-12-toxiproxy-packet-loss-profile-closure.md`.

Native fixed-target UDP datagram resources are a separate API and do not extend
Toxiproxy v2.12; the compatibility adapter remains stream/TCP-only.
