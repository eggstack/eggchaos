# M021 closure — Fixed-Target UDP Runtime and Association Lifecycle

Status: closed  
Exact implementation candidate: `686838bded48fa41d8cb2e5232cceff343d30811`  
Closure/activation commit: recorded in the M021 registry row

## Result

M021 adds an independent bounded Tokio UDP runtime in `eggchaos-server`. Each
client `SocketAddr` owns one connected upstream UDP socket and independent
M020 upstream/downstream direction engines. Replies, multiple responses, and
unsolicited target pushes are routed to the owning client. Listener disable,
delete, shutdown, explicit association kill, and idle expiry own and join all
association tasks. Idle cleanup waits for both engine queues and pre-engine
ingress to empty; explicit cancellation records administrative discards.

Association admission, total active associations, proxy definitions, ingress
slots and bytes, engine queues, and retained history are bounded. The listener
receives into a 65,536-byte buffer before enforcing the logical datagram size,
so oversize input is classified and dropped without forwarding a truncated
prefix. IPv4 and IPv6 upstream sockets use family-correct local addresses.

The Eggress reuse gate found no suitable generic fixed-target association
primitive in published `eggress-udp` 1.0.8: its API is routing/SOCKS-oriented
and brings routing authority. M021 therefore uses Tokio's connected UDP socket
ownership locally and adds no Eggress dependency. The dependency tree confirms
the new direct server dependency is only `bytes` for whole-datagram payloads.

The candidate also closes an M020 validation gap found during runtime
integration: plans deserialized from JSON now revalidate empty or oversized
fault IDs before policy publication. The original M020 closure remains tied to
its recorded candidate; this regression is covered by the M021 exact-candidate
core suite.

## Exact-candidate evidence

All commands below ran against `686838bded48fa41d8cb2e5232cceff343d30811`:

- `cargo fmt --all -- --check` — passed.
- `cargo test -p eggchaos-core --all-features` — 51 passed.
- `cargo test -p eggchaos-server --all-features` — 56 passed.
- `RUST_TEST_THREADS=1 ./scripts/check.sh` — passed workspace format, clippy,
  all workspace tests, and docs. The 7 UDP runtime tests passed, including
  two-client isolation, multiple/unsolicited responses, stable per-client
  source port, bidirectional duplication, delayed-queue expiry, administrative
  discard, capacity/oversize classification, IPv6 loopback, and listener
  lifecycle; the existing TCP suite passed.
- `cargo tree --locked -p eggchaos-server` — passed; no Eggress UDP/routing
  dependency introduced.
- `cargo audit --deny warnings` — passed, no advisories.
- `cargo deny check advisories licenses bans sources` — passed. Existing
  duplicate `getrandom` and `winnow` lock entries were warnings only.

## Follow-on

M022 is unblocked and ready: it can build its distinct native datagram
resources over the verified runtime. M023 remains blocked on M022. The UDP
runtime is post-release work and does not alter M019's v0.1.0 qualification.
