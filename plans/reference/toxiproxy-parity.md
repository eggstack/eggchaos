# Toxiproxy Compatibility Baseline

Research date: 2026-09-22  
Primary compatibility target: Shopify Toxiproxy v2.12.0

## Version policy

Eggchaos does not claim compatibility with an unversioned moving Toxiproxy `main`.

The first compatibility milestone targets the public API and toxic set present in the v2.12.0 tag. That is the latest tagged Toxiproxy release found during the initial investigation.

Current `main` has additional work, including a `packet_loss` toxic that is not present in the v2.12.0 tag. Such additions are future compatibility extensions and require their own tests/status.

## v2.12 proxy fields

| Field | Required behavior |
| --- | --- |
| `name` | Required, stable identifier. Renaming is not supported through update; recreate instead. |
| `listen` | Listener address; port 0 must resolve to an actual bound ephemeral port in the returned representation. |
| `upstream` | Fixed upstream target address. |
| `enabled` | Defaults true. False takes the proxy down. |

Changing listen/upstream in compatibility mode must be treated as a listener/runtime restart and active connection handling must be documented and differentially checked.

## v2.12 toxic fields

| Field | Behavior |
| --- | --- |
| `name` | Defaults to `<type>_<stream>` when omitted, matching compatibility rules. |
| `type` | One of the supported v2.12 toxic names below. |
| `stream` | `upstream` or `downstream`; default downstream. |
| `toxicity` | Probability 0.0..1.0; default 1.0; selected per connection. |
| `attributes` | Toxic-specific JSON object. |

Directions:

- upstream = client -> target;
- downstream = target -> client.

## v2.12 toxic matrix

| Toxiproxy toxic | Attributes | Native eggchaos mapping | Initial parity intent |
| --- | --- | --- | --- |
| `latency` | `latency` ms, `jitter` ms | `Latency { delay, jitter }` | behaviorally compatible within timing tolerance |
| `bandwidth` | `rate` KB/s | `Bandwidth { rate, burst }` with compatibility burst policy | behaviorally compatible; differential throughput windows |
| `slow_close` | `delay` ms | `SlowClose` | compatible close-delay behavior |
| `timeout` | `timeout` ms | `Blackhole { close_after }` | timeout=0 means indefinite drop until change/removal |
| `reset_peer` | `timeout` ms | `Disconnect { mode=ResetBestEffort }` | platform-qualified; must not fake RST support |
| `slicer` | `average_size`, `size_variation`, `delay` us | `Slice` | stream slicing parity with deterministic RNG |
| `limit_data` | `bytes` | `LimitData` | exact byte-boundary behavior |

Native faults may expose additional fields, but compatibility JSON must retain the v2.12 shape.

## v2.12 endpoint matrix

The adapter must cover:

| Method/path | Required behavior |
| --- | --- |
| `GET /proxies` | list proxies with active toxics |
| `POST /proxies` | create proxy |
| `POST /populate` | create/replace collection idempotently when unchanged |
| `GET /proxies/{proxy}` | proxy plus toxics |
| `POST /proxies/{proxy}` | update supported proxy fields |
| `DELETE /proxies/{proxy}` | remove proxy |
| `GET /proxies/{proxy}/toxics` | list toxics |
| `POST /proxies/{proxy}/toxics` | create toxic |
| `GET /proxies/{proxy}/toxics/{toxic}` | get toxic |
| `POST /proxies/{proxy}/toxics/{toxic}` | update toxic |
| `DELETE /proxies/{proxy}/toxics/{toxic}` | remove toxic |
| `POST /reset` | re-enable all proxies and remove all toxics |
| `GET /version` | compatibility version response |
| `GET /metrics` | Prometheus-compatible metrics endpoint |

HTTP status codes, malformed body handling, missing-field behavior, duplicate names, not-found behavior, and defaulting must be captured from an actual v2.12.0 oracle during M006 rather than guessed from source alone.

## Metrics

Toxiproxy documents per-proxy received/sent byte counters labelled by direction, listener, proxy, and upstream. Eggchaos native metrics may be richer, but compatibility names/labels should only be emitted if they can be supported truthfully.

Do not alias native “bytes intentionally dropped” into “sent bytes.”

## Known semantic cautions

### Latency buffering

Toxiproxy buffers its latency toxic specifically to avoid turning latency into an accidental bandwidth limit. Eggchaos must preserve that intent with bounded byte-based buffering/backpressure.

### Toxic live updates

Toxiproxy interrupts toxic pipelines during update/removal and instructs toxic implementations to flush already accepted bytes to avoid corruption. Eggchaos uses generation/barrier semantics instead, but must differential-test externally visible byte preservation.

### Reset behavior

Toxiproxy's `reset_peer` intent is a connection reset. Portable Rust abstractions cannot guarantee a TCP RST for every erased stream. Compatibility is qualified by platform/transport capability and must not claim reset when only FIN/EOF occurred.

### “packet_loss” after v2.12

Current-main stream-chunk loss is not part of the initial v2.12 target. If added later, native documentation must call out that user-space chunk dropping is not equivalent to lower-layer packet loss/retransmission behavior.

## Differential oracle plan

M006 should launch official Toxiproxy v2.12.0 and eggchaos side by side against the same deterministic fixture and run a machine-readable corpus.

Case groups:

- API create/read/update/delete/defaults/errors;
- populate idempotence and replacement;
- enable/disable/reset;
- upstream/downstream direction isolation;
- each toxic with edge values;
- toxic probability 0/1 and seeded intermediate distribution behavior where exact random parity is not expected;
- active toxic update/removal with bytes in flight;
- half-close interaction;
- upstream refusal and mid-stream close;
- ephemeral listener port;
- metrics exposition presence/labels;
- platform-specific reset observation.

Comparators must distinguish exact fields from tolerance-based timing/throughput observations.

## Compatibility claim levels

Use explicit wording:

- `API shape compatible`: JSON/routes/defaults accepted by tested clients.
- `behaviorally compatible`: tested external behavior matches within documented tolerances.
- `intent compatible`: exact low-level behavior cannot be guaranteed cross-platform, such as TCP RST.
- `not supported`: fail clearly; do not silently no-op.

The first release must publish a matrix using these levels instead of one blanket “Toxiproxy compatible” statement.
