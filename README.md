# eggchaos

Rust-native, fixed-target chaos proxy with an embeddable deterministic
byte-stream fault engine.

Put eggchaos between your service and a dependency, then inject latency,
bandwidth limits, blackholes, byte limits, slow closes, slicing, or
disconnects — reproducibly, from versioned seeds. Faults apply per direction
(`upstream` is client→target, `downstream` is target→client).

Eggchaos is not a forward proxy, packet-loss simulator, TLS intercept, or UDP
impairment engine. Faults are user-space byte-stream operations, not IP/TCP
packet behavior.

## Install

```sh
cargo install eggchaos-cli --version 0.1.0
```

Libraries for embedding:

```sh
cargo add eggchaos-core --version 0.1.0
cargo add eggchaos-server --version 0.1.0
cargo add eggchaos-toxiproxy --version 0.1.0
cargo add eggchaos-eggfetch --version 0.1.0
```

Requires Rust 1.89+.

## Quick start

Write `eggchaos.toml`:

```toml
version = 1
seed = 7

[admin]
bind = "127.0.0.1:8475"

[[proxy]]
name = "redis"
listen = "127.0.0.1:16379"
upstream = "127.0.0.1:6379"

[[proxy.fault]]
id = "lag"
direction = "downstream"
type = "latency"
delay = "200ms"
jitter = "50ms"
```

Start it and point your client at the listen address:

```sh
eggchaos serve --config eggchaos.toml
# or without installing: cargo run -p eggchaos-cli -- serve --config eggchaos.toml
```

## Inject faults live

Proxies and faults can be changed at runtime without restarting; existing
connections drain already-accepted bytes before the new plan takes effect.

```sh
eggchaos proxy add redis --listen 127.0.0.1:16379 --upstream 127.0.0.1:6379
eggchaos fault add redis lag --direction downstream --kind latency --delay-ms 200 --jitter-ms 50
eggchaos fault list redis
eggchaos fault remove redis lag
eggchaos connection list
eggchaos connection kill <id>
eggchaos reset
```

The admin API defaults to `http://127.0.0.1:8475` (override with
`--admin <url>`); add `--json` for one machine-readable JSON document per
command. The same surface is available over HTTP:

```sh
curl -s http://127.0.0.1:8475/v1/health
curl -s http://127.0.0.1:8475/v1/proxies
curl -s http://127.0.0.1:8475/metrics
```

## Fault catalog

| Fault | What it does |
| --- | --- |
| `latency` | Delays segments by `delay` ± `jitter`, preserving order |
| `bandwidth` | Throttles to `bytes_per_second` with a `burst_bytes` bucket |
| `blackhole` / `timeout` | Discards bytes, optionally closing after `close_after` |
| `limit_data` | Forwards at most `bytes`, then terminates gracefully |
| `slow_close` | Delays shutdown only, never ordinary writes |
| `slice` | Segments output into `average_size` ± `variation` chunks |
| `disconnect` | Terminates after `after` (`--hard-reset` for abortive close) |

Every fault takes a `probability` (0–1): the deterministic per-connection
activation chance, decided from the seed — never from global RNG state.

## Other surfaces

- **Toxiproxy v2.12 clients:** run the compat server and point existing
  tooling at it (loopback by default):
  ```sh
  cargo run -p eggchaos-toxiproxy --example compat_server -- 127.0.0.1:8474
  ```
- **In-process HTTP chaos:** `eggchaos-eggfetch::ChaosDialer` implements
  Eggfetch's `Dialer` seam, wrapping physical connections in live fault
  policy while Eggfetch keeps HTTP/TLS/pooling:
  ```rust
  let dialer = ChaosDialer::with_policies(seed, "api", upstream, downstream);
  let client = eggfetch_core::Client::builder().dialer(dialer.clone()).build();
  dialer.publish_downstream(FaultPlan::empty())?; // live update, no reconnect
  ```

## Docs

- `docs/architecture.md`, `docs/configuration.md`, `docs/control-plane.md` —
  native boundaries, config model, and admin/CLI contract.
- `docs/toxiproxy.md`, `docs/eggfetch.md` — integration surfaces.
- `architecture/overview.md` — crate-by-crate deep dives.

Requires Rust 1.89+. Admin listeners bind loopback by default; non-loopback
binds need an explicit opt-in plus bearer token. Check behavior with
`cargo test --workspace --all-features` (see `AGENTS.md` for the full gate).

## License

MIT. See `LICENSE`.
