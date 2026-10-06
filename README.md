# eggchaos

Deterministic chaos proxy for TCP streams and UDP datagrams. Put it between
your service and a dependency, then inject latency, bandwidth limits,
blackholes, byte limits, slow closes, slicing, disconnects, and stream loss —
reproducibly, from versioned seeds. Faults apply per direction (`upstream` is
client→target, `downstream` is target→client).

Not a forward proxy, TLS intercept, or IP-layer impairment tool: TCP faults
shape userspace byte streams, the UDP runtime shapes whole application
datagrams.

## Install

Requires Rust 1.89+.

```sh
cargo install eggchaos-cli --version 0.2.0 --locked
```

Or run from source with `cargo run -p eggchaos-cli -- serve --config eggchaos.toml`.

## Quickstart

This walks through injecting 200ms ± 50ms latency on replies. It uses a
throwaway echo server as the dependency so you can verify every step; point
`upstream` at your real dependency (a local Redis on `127.0.0.1:6379`, say) and
skip step 1.

**1. Start a target** to sit in front of:

```sh
python3 -c '
import socket
s = socket.socket()
s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("127.0.0.1", 19999))
s.listen(8)
while True:
    c, _ = s.accept()
    c.sendall(c.recv(4096))
    c.close()
' &
```

**2. Write `eggchaos.toml`:**

```toml
version = 1
seed = 7

[admin]
bind = "127.0.0.1:8475"

[[proxy]]
name = "backend"
listen = "127.0.0.1:16379"
upstream = "127.0.0.1:19999"

[[proxy.fault]]
id = "lag"
direction = "downstream"
type = "latency"
delay = "200ms"
jitter = "50ms"
```

**3. Start the proxy and confirm it loaded:**

```sh
eggchaos serve --config eggchaos.toml
```

```sh
eggchaos health
curl -s http://127.0.0.1:8475/v1/proxies
```

**4. Send traffic through it** — point your client at the listen address
`127.0.0.1:16379`:

```sh
python3 -c 'import socket,time;s=socket.create_connection(("127.0.0.1",16379),5);t=time.time();s.sendall(b"ping");print(s.recv(100).decode(), round((time.time()-t)*1000), "ms")'
```

Expect `ping 200 ms` — 174–226ms across runs, the spread being the jitter. The
proxy really is now the only thing between you and the target.

## Change faults live

No restart, and existing connections keep working: already-accepted bytes
drain before the new plan takes effect.

```sh
eggchaos fault list backend
eggchaos fault set backend lag --kind latency --delay-ms 400 --jitter-ms 50
eggchaos fault remove backend lag
eggchaos fault add backend lag --direction downstream --kind latency --delay-ms 200
eggchaos connection list
eggchaos history
eggchaos reset
```

Re-run step 4 after each change to watch the delay appear and disappear. Two
CLI details worth knowing:

- `fault set` needs `--probability` and/or `--kind`, and supplying `--kind`
  rebuilds the whole kind from the attributes you pass rather than merging into
  the stored one — so repeat every attribute you want to keep, or omitted ones
  fall back to their defaults.
- Every fault takes a `probability` (0–1): the deterministic per-connection
  activation chance, decided from the seed — never from global RNG state.

## Faults

TCP: `latency`, `bandwidth`, `blackhole`, `limit-data`, `slow-close`, `slice`,
`disconnect`, `stream-loss`. UDP: `delay`, `loss`, `duplicate`, `reorder`,
`payload-corrupt`, `bandwidth`. Per-fault attributes are in
[docs/control-plane.md](docs/control-plane.md#native-v1-request-and-response-schemas);
what each does to the byte stream is in
[docs/architecture.md](docs/architecture.md#fault-semantics).

## Other surfaces

- **UDP** — `[[datagram_proxies]]` in the config, or the `eggchaos datagram` CLI
  family ([docs/configuration.md](docs/configuration.md#datagram-proxies)).
- **Toxiproxy v2.12 clients** — point existing tooling at the compat server
  ([docs/toxiproxy.md](docs/toxiproxy.md)):
  `cargo run -p eggchaos-toxiproxy --example compat_server -- 127.0.0.1:8474`
- **In-process HTTP chaos** — `ChaosDialer` wraps physical connections in live
  fault policy while Eggfetch keeps HTTP/TLS/pooling
  ([docs/eggfetch.md](docs/eggfetch.md)):
  ```rust
  let dialer = ChaosDialer::with_policies(seed, "api", upstream, downstream);
  let client = eggfetch_core::Client::builder().dialer(dialer.clone()).build();
  dialer.publish_downstream(FaultPlan::empty())?; // live update, no reconnect
  ```
- **Embedded** — `eggchaos-embed` runs the service in your own process;
  `eggchaos-experiment` runs Scenario V2 schedules
  ([docs/control-plane.md](docs/control-plane.md#scenarios)).
- **Remote SDKs** — zero-dependency Python and TypeScript clients
  ([docs/control-plane.md](docs/control-plane.md#remote-control-sdks)).

## Docs

| Document | Covers |
| --- | --- |
| [docs/architecture.md](docs/architecture.md) | Crate layering, fault semantics, datagram runtime |
| [docs/configuration.md](docs/configuration.md) | TOML schema, runtime bounds, UDP proxies |
| [docs/control-plane.md](docs/control-plane.md) | Admin routes, CLI, scenarios, metrics, SDKs |
| [docs/toxiproxy.md](docs/toxiproxy.md) | Toxiproxy v2.12 compatibility and parity |
| [docs/eggfetch.md](docs/eggfetch.md) | Eggfetch `Dialer` integration |

[architecture/overview.md](architecture/overview.md) has the per-crate deep
dives. Admin listeners bind loopback by default; a non-loopback bind needs an
explicit opt-in plus a bearer token (`--admin-token` or
`EGGCHAOS_ADMIN_TOKEN`).

## License

MIT. See [LICENSE](LICENSE).
