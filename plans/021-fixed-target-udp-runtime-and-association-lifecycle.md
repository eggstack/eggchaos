# M021 — Fixed-Target UDP Runtime and Association Lifecycle

Status: blocked
Depends on: M020
Role: standalone UDP transport/runtime

## Objective

Build a bounded fixed-target UDP runtime in `eggchaos-server` over the M020 datagram engine, with correct multi-client response ownership and lifecycle semantics.

M021 proves the transport/runtime model before exposing public admin, config, CLI, or scenario surfaces.

## Baseline

The TCP runtime remains authoritative for stream proxies and must not be generalized into transport-conditional branches that weaken its lifecycle model.

Current Eggress provides useful UDP concepts and fixtures in `eggress-udp`: bounded association/target-flow state, connected UDP target sockets, idle reaping, counters, IPv4/IPv6 test support, and routing-aware relay machinery. Its broader runtime is SOCKS/routing/compatibility oriented, and its fixed-target compatibility path is a narrow request/one-response loop rather than the per-client association model required by eggchaos.

M021 must therefore audit the exact published Eggress version/seam before adding a production dependency.

## Scope

### In scope

- A distinct fixed-target datagram proxy runtime model.
- UDP listener bind/supervision/cancellation lifecycle.
- Per-client associations keyed by client `SocketAddr`.
- One connected upstream UDP socket per association.
- Independent upstream/downstream M020 engines and policies.
- Multiple and unsolicited target responses routed to the correct client.
- Bounded association count, queue resources, histories, and idle cleanup.
- IPv4/IPv6 family-correct upstream binding.
- Explicit oversized-datagram classification after receipt.
- Internal snapshots/evidence/metrics sufficient for M022.
- Exact Eggress reuse/dependency decision with evidence.

### Non-goals

- No native HTTP/admin endpoints.
- No TOML/CLI/scenario user surface.
- No SOCKS5 UDP ASSOCIATE, Shadowsocks UDP, routing rules, arbitrary targets, or upstream chains.
- No multicast/broadcast listener product mode.
- No QUIC awareness; QUIC is opaque UDP payload at this layer.
- No Toxiproxy changes.
- No request/response assumption limiting a target to one response.

## Affected surfaces

- `crates/eggchaos-server/src/runtime/` — sibling datagram runtime modules.
- `crates/eggchaos-server/Cargo.toml` if a narrow Eggress seam is justified.
- `architecture/server-runtime.md`, `architecture/overview.md`, and `docs/architecture.md`.
- Potential dev/test reuse from Eggress testkit/UDP fixtures.

## Runtime model

A source client address maps to one association:

```text
client SocketAddr
  -> bounded association
       -> stable association id/key
       -> upstream DatagramDirectionEngine
       -> connected upstream UdpSocket -> fixed target
       -> downstream DatagramDirectionEngine
       -> last activity / cancellation / evidence
```

A dedicated connected upstream socket per client provides unambiguous reply ownership and a stable target-side source port. It must support multiple responses and unsolicited responses without cross-delivery to another client.

A single proxy-wide upstream socket or `send -> recv exactly one response` loop is rejected.

## Admission and receive-size contract

The listener must use a receive buffer large enough to observe the largest supported UDP datagram and then compare the received length to configured `max_datagram_size`.

Do not size `recv_from` to the logical configured limit: the OS/Tokio may truncate and discard the remainder before eggchaos can classify it.

Oversized ingress is an explicit recorded drop, not a truncated payload.

Association and history limits must be finite. New-client admission at capacity must be explicit and observable.

## Association lifetime

Activity is directional and monotonic. Idle expiry may reap an association only when both datagram engine queues are empty; a deliberately delayed datagram must not disappear because its delay exceeds the ordinary idle timeout.

Explicit proxy disable/delete/service shutdown may cancel queued datagrams. Such discards are administrative-discard evidence, distinct from configured loss and queue overflow.

Association removal must cancel/join owned tasks and release sockets exactly once. No detached receive loop may survive registry removal.

## Eggress reuse gate

Before implementation chooses dependencies:

1. inspect the exact Eggress version line compatible with eggchaos;
2. identify whether a public generic connected fixed-target UDP/association primitive exists without pulling routing/SOCKS/runtime authority;
3. verify crate-version alignment so one binary does not contain mismatched Eggress generations;
4. prefer a genuine generic seam or testkit reuse;
5. otherwise implement the small `tokio::net::UdpSocket` ownership layer locally.

Do not copy Eggress source. Do not depend on routing-heavy `eggress-udp` APIs merely to avoid a small transport owner.

Any needed upstream extraction becomes a separate Eggress plan and keeps M021 blocked until published/usable.

## Ordered work packages

### WP1 — Runtime types and dependency census

Define internal datagram proxy/association/runtime parameter types and perform the Eggress reuse gate. Record the decision in architecture docs and the closure note.

### WP2 — Listener supervision

Add bind-before-visible-success, port-zero resolution, cancellation, restart/delete/disable safety, and task ownership parallel to TCP invariants without merging the registries.

### WP3 — Per-client association registry

Create/reuse bounded associations by client address, allocate stable IDs/ordinals, create a connected upstream socket per association, enforce global/per-proxy limits, and track exact active counts.

### WP4 — Bidirectional datagram pump

Feed client datagrams through the upstream M020 engine to the connected target socket. Independently receive target datagrams, feed the downstream engine, and send emitted datagrams to the owning client. Scheduler deadlines must be polled without spawning one task per datagram.

### WP5 — Expiry and administrative termination

Implement idle reaping only when queues are empty, explicit kill/removal/shutdown cancellation, exact queue discard accounting, and clean task/socket teardown.

### WP6 — Internal observability

Add bounded association snapshots/history, packet+byte counters, configured loss/overflow/admin discard distinctions, generation/evidence merge, and low-cardinality metric primitives for M022.

### WP7 — Runtime tests and docs

Add live UDP echo/multi-response/unsolicited-response fixtures, multi-client isolation, capacity/reap tests, oversize handling, IPv4/IPv6 qualification, and architecture documentation.

## Verification

Minimum:

```sh
cargo fmt --all -- --check
cargo clippy -p eggchaos-server --all-targets --all-features -- -D warnings
cargo test -p eggchaos-core --all-features
cargo test -p eggchaos-server --all-features
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
cargo tree --locked
cargo audit --deny warnings
cargo deny check advisories licenses bans sources
```

Required runtime tests:

- two concurrent clients never receive each other's responses;
- one request can produce multiple responses;
- unsolicited target response reaches the owning client;
- per-client upstream source ports remain stable during an association;
- loss/delay/duplicate/reorder operate both directions;
- delayed queued data prevents idle reap;
- explicit removal discards queued data with administrative evidence;
- capacity rejects new associations without corrupting existing ones;
- oversized datagrams are dropped, never truncated into accepted payload;
- IPv4 and IPv6 work where host capability exists;
- listener bind/restart/delete/shutdown leave no ghost tasks or sockets;
- TCP proxy lifecycle tests remain green.

## Acceptance criteria

M021 closes only when:

- M020 is closed;
- a fixed-target UDP listener handles concurrent clients with per-client upstream sockets;
- reply ownership is correct for multiple/unsolicited responses;
- all association/queue/history/task state is bounded;
- idle and explicit termination semantics match this plan;
- receive-size behavior is explicitly tested;
- Eggress reuse/dependency decision is documented and version-coherent;
- no control/CLI/Toxiproxy implementation is smuggled into the runtime milestone;
- full workspace checks are green.

Create `plans/closure/M021-fixed-target-udp-runtime-and-association-lifecycle-closure.md`.

## Stop/rejection conditions

Do not close if:

- a shared upstream socket can cross-route replies;
- runtime assumes exactly one response per client datagram;
- idle reap destroys queued delayed datagrams;
- receive buffers silently truncate oversized input;
- task/socket ownership can detach on removal;
- production dependency pulls Eggress routing/SOCKS authority without a justified generic seam;
- Eggress versions are mismatched in the final dependency graph;
- TCP runtime semantics regress.

## Follow-on activation

On clean closure, M022 becomes `ready`.
