# eggchaos

Eggchaos is a Rust-native, fixed-target chaos proxy and embeddable bounded
byte-stream fault engine. The project is pre-release (`0.1.0`) and its public
semantics are documented in the [roadmap](plans/roadmap.md).

The workspace provides the validated native fault model, deterministic
SplitMix64-v1 identity derivation, bounded live mutation, a fixed-target relay,
native JSON/TOML control, Toxiproxy v2.12 translation, and an Eggfetch
physical-stream `Dialer` adapter. The first-release qualification gate remains
open until cross-platform, packaging, and release evidence is complete.

Eggchaos is not a general forward proxy, packet-loss simulator, TLS MITM, UDP
impairment engine, or arbitrary CONNECT/SOCKS router. Stream slicing and loss
semantics are user-space byte-stream operations, not IP/TCP packet behavior.

```sh
cargo test --workspace --all-features
```

See [architecture](docs/architecture.md) and [configuration](docs/configuration.md)
for the implementation boundaries and versioned native configuration model.
See [control-plane](docs/control-plane.md), [Toxiproxy](docs/toxiproxy.md), and
[Eggfetch](docs/eggfetch.md) for the supported integration surfaces.
