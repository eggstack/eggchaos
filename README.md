# eggchaos

Deterministic fixed-target chaos proxy for TCP streams and UDP datagrams. Place it between a client and its dependency to inject latency, bandwidth limits, blackholes, byte limits, slow closes, slicing, disconnects, or stream loss. Fault decisions are reproducible from versioned seeds. `upstream` means client to target; `downstream` means target to client.

TCP faults shape userspace byte streams. UDP faults apply to whole datagrams. Eggchaos is not a forward proxy, TLS interceptor, or IP-layer impairment tool.

## Install

The current release is v0.2.1. Install with Rust 1.89 or newer:

```sh
cargo install eggchaos-cli --version 0.2.1 --locked
```

Prebuilt CLI binaries are available from the [v0.2.1 GitHub release](https://github.com/eggstack/eggchaos/releases/tag/v0.2.1). To run from a source checkout:

```sh
cargo run -p eggchaos-cli -- serve --config eggchaos.toml
```

## Quickstart

This example adds 200 ms latency with 50 ms of jitter to replies. It uses a small local echo server as the target. To test a real dependency instead, set `upstream` to its address and skip step 1.

**1. Start the target** in a terminal:

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

**2. Save this configuration as `eggchaos.toml`:**

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

**3. Start Eggchaos** and leave it running:

```sh
eggchaos serve --config eggchaos.toml
```

Use another terminal for the remaining commands. Check that the service is ready:

```sh
eggchaos health
curl -s http://127.0.0.1:8475/v1/proxies
```

**4. Send traffic** to the proxy listener at `127.0.0.1:16379`:

```sh
python3 - <<'PY'
import socket
import time

with socket.create_connection(("127.0.0.1", 16379), timeout=5) as s:
    start = time.perf_counter()
    s.sendall(b"ping")
    reply = s.recv(100).decode()
    elapsed_ms = round((time.perf_counter() - start) * 1000)
    print(reply, elapsed_ms, "ms")
PY
```

Expect roughly 150 to 250 ms, plus local scheduling overhead. The configured jitter varies the 200 ms base delay by up to 50 ms in either direction.

## Change faults live

No restart is needed. When a stream policy changes, preserving bytes accepted under the old generation drain before the new generation takes effect.

```sh
eggchaos fault list backend
eggchaos fault set backend lag --kind latency --delay-ms 400 --jitter-ms 50
eggchaos fault remove backend lag
eggchaos fault add backend lag --direction downstream --kind latency --delay-ms 200
eggchaos connection list
eggchaos history
eggchaos reset
```

Re-run the traffic command after each change. `fault set` needs `--probability` and/or `--kind`. Supplying `--kind` rebuilds that fault from the attributes provided, so repeat any values you want to keep. Omitted attributes use their defaults.

Fault probability defaults to `1.0`. Stream fault activation is deterministic per connection; datagram fault probability is evaluated per datagram. Both use seeded, fault-local decisions rather than process-global randomness.

## Faults

TCP stream faults are `latency`, `bandwidth`, `blackhole`, `limit-data`, `slow-close`, `slice`, `disconnect`, and `stream-loss`. UDP datagram faults are `delay`, `loss`, `duplicate`, `reorder`, `payload-corrupt`, and `bandwidth`.

See [configuration](docs/configuration.md) for TOML fields, [control plane](docs/control-plane.md) for API and CLI schemas, and [architecture](docs/architecture.md#fault-semantics) for fault behavior.

## Other ways to use Eggchaos

- **UDP proxy:** configure `[[datagram_proxies]]` or use the `eggchaos datagram` CLI commands. See [datagram configuration](docs/configuration.md#datagram-proxies).
- **Toxiproxy compatibility:** run the loopback compatibility server with `cargo run -p eggchaos-toxiproxy --example compat_server -- 127.0.0.1:8474`. See [Toxiproxy compatibility](docs/toxiproxy.md).
- **Eggfetch integration:** `eggchaos-eggfetch` applies policies to physical connections while Eggfetch retains HTTP, TLS, and pooling responsibilities. See [Eggfetch integration](docs/eggfetch.md).
- **Embedding and scenarios:** `eggchaos-embed` runs the service in-process, and `eggchaos-experiment` provides Scenario V2 schedules. See [control plane](docs/control-plane.md#scenarios).
- **Remote SDKs:** source-only Python and TypeScript clients control the native API. They do not manage the daemon lifecycle. See [control plane](docs/control-plane.md#remote-control-sdks).

## Documentation

| Document | Covers |
| --- | --- |
| [Architecture](docs/architecture.md) | Crate boundaries and fault semantics |
| [Configuration](docs/configuration.md) | TOML schema, defaults, and runtime bounds |
| [Control plane](docs/control-plane.md) | Admin API, CLI, scenarios, metrics, and SDKs |
| [Toxiproxy](docs/toxiproxy.md) | Compatibility profiles and differences |
| [Eggfetch](docs/eggfetch.md) | In-process transport integration |

The admin listener binds to loopback by default. For non-loopback access, configure `public_admin = true` and a non-empty `auth_token` in `[admin]`. The listener uses HTTP, so protect remote access with a trusted TLS terminator and network controls. The CLI's `--admin-token` option and `EGGCHAOS_ADMIN_TOKEN` environment variable send credentials to an existing admin service; they do not configure the service listener.

## License

MIT. See [LICENSE](LICENSE).
