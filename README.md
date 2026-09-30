# eggchaos

Fixed-target chaos proxy with deterministic stream and whole-datagram fault
engines. Put it between your service and a dependency, then inject latency,
bandwidth limits, blackholes, byte limits, slow closes, slicing, disconnects,
or stream loss — reproducibly, from versioned seeds. Faults apply per
direction (`upstream` is client→target, `downstream` is target→client).

Not a forward proxy, TLS intercept, or IP-layer impairment tool: TCP faults
shape userspace byte streams, the UDP runtime shapes whole application
datagrams.

## Install

Published release `0.1.0` (requires Rust 1.89+):

```sh
cargo install eggchaos-cli --version 0.1.0
```

From source (unreleased `0.2.0` tree — do not `cargo install --version 0.2.0`
until the owner publishes it):

```sh
git clone https://github.com/eggstack/eggchaos
cd eggchaos
cargo run -p eggchaos-cli -- serve --config eggchaos.toml
```

## Quickstart

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

Start it, check health, and point your client at the listen address:

```sh
eggchaos serve --config eggchaos.toml
curl -s http://127.0.0.1:8475/v1/health
curl -s http://127.0.0.1:8475/v1/proxies
curl -s http://127.0.0.1:8475/metrics
```

Replies from the upstream now arrive ~200ms later via the `lag` fault.
The admin API defaults to `http://127.0.0.1:8475` (override with
`--admin <url>`); add `--json` for one machine-readable JSON document per
command. Config schema, bounds, and UDP `[[datagram_proxies]]` are covered
in `docs/configuration.md`.

## Change faults live

No restart needed; existing connections drain already-accepted bytes before
the new plan takes effect:

```sh
eggchaos proxy add redis --listen 127.0.0.1:16379 --upstream 127.0.0.1:6379
eggchaos fault add redis lag --direction downstream --kind latency --delay-ms 200 --jitter-ms 50
eggchaos fault list redis
eggchaos fault remove redis lag
eggchaos connection list
eggchaos reset
```

Every fault takes a `probability` (0–1): the deterministic per-connection
activation chance, decided from the seed — never from global RNG state.
Full fault semantics live in `docs/architecture.md`; routes, CLI, scenarios,
and UDP resources in `docs/control-plane.md`.

## More surfaces

- **Toxiproxy v2.12 clients:** point existing tooling at the compat server
  (loopback by default). Details in `docs/toxiproxy.md`:
  ```sh
  cargo run -p eggchaos-toxiproxy --example compat_server -- 127.0.0.1:8474
  ```
- **In-process HTTP chaos:** `eggchaos-eggfetch::ChaosDialer` wraps physical
  connections in live fault policy while Eggfetch keeps HTTP/TLS/pooling.
  Details in `docs/eggfetch.md`:
  ```rust
  let dialer = ChaosDialer::with_policies(seed, "api", upstream, downstream);
  let client = eggfetch_core::Client::builder().dialer(dialer.clone()).build();
  dialer.publish_downstream(FaultPlan::empty())?; // live update, no reconnect
  ```
- **Embedded experiments:** `eggchaos-experiment` runs Scenario V2 schedules
  against a consumer-neutral policy target with a shared monotonic start
  epoch — see `docs/control-plane.md`.

## Docs

`docs/architecture.md`, `docs/configuration.md`, `docs/control-plane.md`,
`docs/toxiproxy.md`, `docs/eggfetch.md`. Crate-by-crate deep dives in
`architecture/overview.md`. Admin listeners bind loopback by default;
non-loopback binds need an explicit opt-in plus bearer token.

## License

MIT. See `LICENSE`.
