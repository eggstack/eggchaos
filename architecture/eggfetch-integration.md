# Eggfetch in-process integration

Back to [architecture overview](overview.md) (§ “Eggfetch in-process integration”, workspace map row `eggchaos-eggfetch`).

## 1. Purpose

`eggchaos-eggfetch` provides in-process physical-stream chaos for HTTP clients via Eggfetch’s public `Dialer` seam. `ChaosDialer` performs bounded direct TCP dialing and returns the connected socket wrapped as a `BidirectionalChaosStream` carrying a live upstream/downstream fault policy. Eggfetch continues to own everything above the physical stream.

Canonical contract: `docs/eggfetch.md`. Implementation: `crates/eggchaos-eggfetch/src/lib.rs`. Feature gate: `crates/eggchaos-eggfetch/Cargo.toml`. Regression matrix: `crates/eggchaos-eggfetch/tests/regression.rs`. Qualification entrypoint: `scripts/qualify_eggfetch.sh`.

Non-goals (explicit in `docs/eggfetch.md` and `crates/eggchaos-eggfetch/src/lib.rs:1-5`):

- No per-request chaos. Policy is physical-connection scoped; one pooled H2 connection exposes one policy to many logical streams.
- No HTTP parsing, body inspection, or TLS interception in the adapter.
- No forward-proxy / CONNECT / SOCKS routing. Dial is direct TCP to the resolved `DialTarget` host/port.
- No pooling, retry, SNI, or certificate-verification logic in the adapter.

## 2. `ChaosDialer` shape

Defined in `crates/eggchaos-eggfetch/src/lib.rs:21-29`:

```rust
pub struct ChaosDialer {
    upstream: LivePolicy,                        // physical-stream client→server direction
    downstream: LivePolicy,                      // physical-stream server→client direction
    proxy_name: Arc<str>,                        // RNG identity component
    connection_ordinal: Arc<AtomicU64>,          // starts at 1, fetch_add per dial
    connect_timeout: Duration,                   // per-address TCP connect bound, default 10s
}
```

`Clone` is cheap (`LivePolicy`, `Arc<str>`, `Arc<AtomicU64>` all shared); clones share the same policy handles and ordinal counter, so pooled clients and manual `dialer.clone()` handles observe the same generations.

Constructor / accessor surface (`crates/eggchaos-eggfetch/src/lib.rs:31-82`):

| Method | Location | Semantics |
| --- | --- | --- |
| `new(run_seed, proxy_name)` | `lib.rs:33-35` | Empty upstream + empty downstream plans; both `LivePolicy` namespaces set to `run_seed` (generation 1). |
| `with_policies(run_seed, proxy_name, upstream, downstream)` | `lib.rs:38-51` | Validated initial directional plans; `run_seed` is the namespace for both initial generations. Ordinal counter starts at `1`. `connect_timeout` defaults to `Duration::from_secs(10)`. |
| `with_connect_timeout(timeout)` | `lib.rs:53-56` | Builder-style override of the bounded physical connect timeout. Consumes `self`. |
| `upstream_policy()` / `downstream_policy()` | `lib.rs:58-64` | Return cloned `LivePolicy` handles (shared state, not snapshots). |
| `publish_upstream(plan)` | `lib.rs:66-71` | Validates `plan`, publishes generation `N+1` retaining `self.upstream.seed_namespace()`. Returns new generation or `ValidationError`. |
| `publish_downstream(plan)` | `lib.rs:73-81` | Same for the downstream handle. |

`proxy_name` is carried as `Arc<str>` so every dial can pass `&*proxy_name` into `BidirectionalChaosStream::new_live` without allocation (`lib.rs:107-111`). `connection_ordinal` uses `Ordering::AcqRel` fetch-add (`lib.rs:90`); first dial observes ordinal `1`, second `2`, and so on.

## 3. Dial flow

`impl Dialer for ChaosDialer` at `crates/eggchaos-eggfetch/src/lib.rs:84-133`. The returned `DialFuture` owns clones of timeout, proxy name, both policies, and the assigned ordinal.

Ordered steps:

1. **Resolve.** `lookup_host((target.host(), target.port()))` (`lib.rs:92-100`). Only `target.host()` / `target.port()` are used; no SNI derivation, no proxy routing.
2. **Per-address bounded connect.** For each resolved `SocketAddr`, `timeout(connect_timeout, TcpStream::connect(address))` (`lib.rs:102-103`):
   - `Ok(Ok(stream))` → wrap and return (step 3).
   - `Ok(Err(error))` → stash as `last_error`, try the next address (`lib.rs:115`).
   - `Err(_)` (elapsed) → immediate `DialErrorKind::Timeout` with message `"direct route connect timed out"` (`lib.rs:116-121`). Later addresses are not tried.
3. **Wrap.** `BidirectionalChaosStream::new_live(stream, upstream, downstream, &*proxy_name, ordinal)` (`lib.rs:105-112`). Construction compiles two `DirectionEngine`s from atomic policy snapshots (see §5). An `EngineError` here maps to `DialErrorKind::Other` (`lib.rs:112`).
4. **Return.** `Ok(Box::new(stream) as DialStream)` (`lib.rs:113`). From this point Eggfetch owns framing/TLS/pooling above the stream.

Error mapping (all messages prefixed `"direct route …"` so dial-origin failures are distinguishable in client error surfaces; asserted in `tests/regression.rs:405-426`):

| Failure | `DialErrorKind` | Message | Location |
| --- | --- | --- | --- |
| `lookup_host` fails | `Connection` (with `io` source) | `"direct route resolution failed"` | `lib.rs:94-99` |
| Per-address connect times out | `Timeout` | `"direct route connect timed out"` | `lib.rs:117-120` |
| All addresses refuse/fail | `Connection` (with last `io` source, or `NotFound` when zero addresses) | `"direct route connection failed"` | `lib.rs:124-130` |
| `new_live` engine construction fails | `Other` | `error.to_string()` | `lib.rs:112` |

Deliberately absent: any HTTP status handling, TLS handshake, SNI selection, certificate validation, pool lookup, or retry. Unit proof that the dialer does not own HTTP/TLS is `lib.rs:141-158` (`dialer_connects_without_owning_http_or_tls`, raw TCP `ok` bytes) versus `lib.rs:160-194` (`eggfetch_owns_http_over_the_physical_chaos_stream`, real `eggfetch_core::Client` GET ×2 over the same dialer type).

## 4. Ownership split

| Concern | Adapter (`ChaosDialer`) owns | Eggfetch owns |
| --- | --- | --- |
| DNS / address iteration | `lookup_host` + per-address loop (`lib.rs:92-102`) | Nothing (receives one `DialStream`) |
| TCP connect bound | `connect_timeout`, default 10 s (`lib.rs:49,53-56,103`) | Client-level request timeouts above the stream |
| Physical-stream faults | `LivePolicy` pair + `BidirectionalChaosStream` wrapper (`lib.rs:105-112`) | Observes faults as slow/failed reads/writes |
| Connection identity | `proxy_name` + `connection_ordinal` → RNG seed (`lib.rs:88-90,109-110`) | Connection pooling / reuse decisions |
| HTTP framing (H1/H2) | Nothing | Request encoding, response parsing, H2 multiplexing (`lib.rs:160-194`, `lib.rs:197-281` under `http2`) |
| TLS, SNI, cert verification | Nothing | Handshake above the returned stream; `TlsConfig`, `danger_accept_invalid_certs`, added-CA trust (`tests/regression.rs:131-174,176-218`) |
| Pooling, keep-alive, redial | Nothing (each `dial` = one fresh TCP connection) | Keep-alive reuse, idle-close redial (`tests/regression.rs:96-128,381-402`) |
| Retry policy | Nothing | Client send/retry behavior |

The header comment (`lib.rs:1-5`) states the split normatively: “Eggfetch remains the authority for HTTP framing, pooling, TLS, SNI, and certificate verification.”

## 5. Live-policy publishing model

Policies are `eggchaos_core::LivePolicy` handles (`crates/eggchaos-core/src/policy.rs:24-28`): a read-mostly `Arc<ArcSwap<PublishedPolicy>>` where each `PublishedPolicy` bundles `(plan, generation, seed_namespace)` immutably (`policy.rs:7-22`). Readers load the whole snapshot atomically, so generation can never disagree with its plan.

### 5.1 Namespace retention

- `LivePolicy::new(plan, seed_namespace)` starts at generation `1` (`policy.rs:63-72`). `ChaosDialer::new` / `with_policies` pass `run_seed` as the namespace for both directions (`lib.rs:33-51`).
- `publish_upstream` / `publish_downstream` reload `seed_namespace()` from the handle they publish to and re-publish under that same namespace (`lib.rs:66-81`). Manual updates therefore retain the namespace; only scenario runs derive fresh namespaces via `derive_policy_seed` (`crates/eggchaos-core/src/rng.rs:46-63`). This matches the overview invariant: “Manual updates retain the namespace; scenario runs derive namespaces.”
- `LivePolicy::publish` validates before storing (`policy.rs:91-104`); an invalid plan returns `ValidationError` and changes nothing.

### 5.2 Per-connection identity

When `BidirectionalChaosStream::new_live` is called (`crates/eggchaos-core/src/stream.rs:578-614`), each direction compiles a `DirectionEngine::new(plan, seed_namespace, proxy, connection_key, direction)` from its snapshot (`stream.rs:592-605`). RNG derivation is then per `(run_seed = seed_namespace, proxy, connection_key = ordinal, direction, fault.id)` via `derive_seed` (`crates/eggchaos-core/src/rng.rs:17-36`; consumed at `crates/eggchaos-core/src/engine.rs:373`).

Concretely for this adapter:

- `proxy` = `ChaosDialer.proxy_name` (e.g. `"keepalive"`, `"h2-concurrent"` in tests).
- `connection_key` (`ordinal`) = per-dial `fetch_add` starting at `1` (`lib.rs:48,90`).
- `direction` = `Upstream` (client→server bytes) vs `Downstream` (server→client bytes).
- `fault.id` = each `FaultSpec.id` in the published plan.
- `run_seed` = the `LivePolicy` seed namespace (initially the dialer `run_seed`, retained across manual publishes).

Two dials therefore never share an RNG stream unless all five components coincide, and two directions / two faults on the same connection diverge by construction. Golden determinism for the derivation itself lives in core (`rng.rs` tests, `engine.rs:373` call site).

### 5.3 Live engagement without reconnect

The stream retains both `LivePolicy` handles (`stream.rs:566-567`) and reconciles generations on the I/O path (`stream.rs:626-676`): when a snapshot generation differs from the observed generation, the engine recompiles after draining queued/buffered bytes (upstream flush, downstream `output` drain), preserving the termination handle. The observable adapter-level proof is `tests/regression.rs:96-128` (`h1_keepalive_survives_live_policy_update`): a pooled keep-alive connection picks up a newly published 150 ms downstream latency with no reconnect.

Downstream termination resolution surfaces on the physical stream as EOF (graceful `Disconnect`, `LimitData` exhaustion reads as truncation error at the HTTP layer) or I/O errors (hard reset); the adapter adds no HTTP-level translation. Regression pins: graceful disconnect → send-or-body error, never a forged-complete body (`tests/regression.rs:428-471`); 50-byte `LimitData` on a 200-byte body → error, never truncation-as-success (`tests/regression.rs:311-342`).

## 6. H1 default vs `http2` feature path

`crates/eggchaos-eggfetch/Cargo.toml:11-13`:

```toml
[features]
default = []
http2 = ["eggfetch-core/http2"]
```

- **Default (H1).** No extra Eggfetch transport. `eggfetch_core::Client::builder().dialer(dialer).build()` speaks HTTP/1.1 over the chaos stream; keep-alive reuses the same physical policy until the server closes, at which point Eggfetch redials through `ChaosDialer::dial` again (new ordinal, current generations). Pinned by `lib.rs:160-194` and `tests/regression.rs:96-128,287-402`.
- **`http2` feature.** Enables `eggfetch-core/http2` without changing the adapter boundary (`docs/eggfetch.md:18-23`). Eggfetch performs TLS/SNI, ALPN negotiation, and H2 session management above the returned `DialStream`; the adapter still returns a raw TCP chaos stream.

Test-only H2/TLS scaffolding (dev-dependencies in `Cargo.toml:21-27`: `h2`, `http`, `rcgen`, `rustls`, `tokio-rustls`, `bytes`):

- Server side builds `rustls::ServerConfig` with a self-signed `rcgen` cert and `alpn_protocols = [b"h2"]`, wraps the accepted TCP stream in `TlsAcceptor`, then runs `h2::server::handshake` (`lib.rs:212-237`; same pattern in `tests/regression.rs:226-256`).
- Client side sets `HttpVersionPolicy::Http2Only` plus either `danger_accept_invalid_certs(true)` (`lib.rs:252-257`) or an added-CA `TlsConfig` (`tests/regression.rs:257-266`).
- A physical connection-level fault consequently affects all multiplexed H2 streams on that connection; per-request chaos is explicitly out of scope (`docs/eggfetch.md:12-16`). Concurrency proof: 4 concurrent H2 streams share one chaos connection and each echoes its path (`tests/regression.rs:220-285`).

### Pooled-connection teardown discipline

H2 (and H1 keep-alive) pooling means the server cannot wait for peer close: a live client may hold the TCP connection open indefinitely. Both H2 tests therefore use bounded phases with named stall messages (`lib.rs:204-210`):

- `CLIENT_PHASE = 120 s` around dial + TLS/H2 handshake + body; stall message names the phase (`lib.rs:258-268`).
- `SERVER_JOIN = 60 s` around the server task join.
- Server calls `connection.graceful_shutdown()` then a 5 s bounded drain (`while accept().await.is_some()` under `timeout(5s)`), then drops `connection` to close the transport regardless of peer behavior (`lib.rs:238-250`).
- The test drops the `Client` before joining the server so pooled connections close deterministically (`lib.rs:272-280`).

All `tests/regression.rs` cases additionally wrap client sends/body reads in `timeout(ROUND /* 20 s */, …)` (`regression.rs:20,118-122,163-172,…`) so a fault that swallows bytes (e.g. indefinite `Blackhole`) becomes an assertion (`regression.rs:287-309`) rather than a hung runner.

## 7. Regression coverage map

All in `crates/eggchaos-eggfetch/tests/regression.rs` (matrix header at `regression.rs:1-6`); unit layer in `lib.rs:135-282`.

| Test | Location | What it pins |
| --- | --- | --- |
| `dialer_connects_without_owning_http_or_tls` | `lib.rs:141-158` | Raw TCP dial + read through chaos stream, no HTTP/TLS in adapter. |
| `eggfetch_owns_http_over_the_physical_chaos_stream` | `lib.rs:160-194` | Real H1 GET ×2 through `eggfetch_core::Client` over chaos dialer. |
| `eggfetch_owns_tls_and_http2_over_the_physical_chaos_stream` (`http2`) | `lib.rs:196-281` | TLS + ALPN-h2 + H2 body over chaos dialer; bounded teardown discipline. |
| `h1_keepalive_survives_live_policy_update` | `regression.rs:96-128` | `publish_downstream(latency 150 ms)` engages on a pooled keep-alive connection (≥100 ms observed). |
| `https_valid_added_ca_is_trusted` | `regression.rs:130-174` | Added-CA trust succeeds; Eggfetch owns cert validation. |
| `https_invalid_certificate_is_rejected` | `regression.rs:176-218` | Default trust rejects self-signed; error text leaks no key material. |
| `h2_concurrent_streams_share_one_chaos_connection` (`http2`) | `regression.rs:220-285` | 4 concurrent H2 streams, per-path echo bodies. |
| `blackhole_response_blocks_without_hanging_the_client` | `regression.rs:287-309` | Indefinite downstream `Blackhole` never completes within 2 s; client remains free. |
| `mid_response_termination_is_an_error_not_truncation` | `regression.rs:311-342` | 50-byte `LimitData` on 200-byte body errors at send or body read. |
| `downstream_bandwidth_shapes_http_body` | `regression.rs:344-379` | 8 KiB body intact (byte-exact `0x11`) but paced ≥1200 ms at 4096 B/s. |
| `server_closed_idle_connection_redials` | `regression.rs:381-402` | `Connection: close` ×2 yields 2 server hits (Eggfetch redials via dialer). |
| `refused_dial_reports_shaped_error` | `regression.rs:404-426` | Closed port surfaces a `"direct route"`-shaped `DialError`. |
| `disconnect_fault_terminates_http_stream` | `regression.rs:428-471` | Graceful downstream `Disconnect` truncates stalled 100-byte body → error, no hang. |

Shared helpers: `fault` / `downstream_plan` (`regression.rs:22-33`), `h1_origin` minimal keep-alive origin with optional hit counter (`regression.rs:36-83`), `ok` response builder (`regression.rs:85-94`).

## 8. Review checklist

For any change touching `crates/eggchaos-eggfetch/`:

1. **Boundary.** Does the diff parse HTTP, inspect bodies, terminate TLS, select SNI, verify certs, pool, or retry? If yes, it belongs in Eggfetch, not here (`lib.rs:1-5`, `docs/eggfetch.md:1-16`).
2. **Dial errors.** Do new failure modes keep the `resolution / connection / timeout` mapping with `"direct route …"` messages (`lib.rs:92-130`)? Is a new closed-port-style case covered like `regression.rs:404-426`?
3. **Ordinal.** Is `connection_ordinal` still `fetch_add` from `1` with `AcqRel` and passed as `connection_key` (`lib.rs:48,90,109-110`)? Cloned dialers must share the counter.
4. **Namespace.** Do `publish_upstream` / `publish_downstream` retain `seed_namespace()` (`lib.rs:66-81`, `policy.rs:87-104`)? Fresh namespaces come only from scenario derivation (`rng.rs:46-63`).
5. **Identity.** Does any RNG-affecting change preserve the `(run_seed, proxy, ordinal, direction, fault.id)` derivation (`rng.rs:17-36`, `engine.rs:373`)?
6. **H2 scope.** Is per-request chaos claimed? It must not be (`docs/eggfetch.md:12-16`). H2 coverage stays physical-connection scoped (`regression.rs:220-285`).
7. **Boundedness.** Every new await on network I/O needs a `timeout` with a named phase; unbounded joins stall all CI test binaries serially (`lib.rs:204-210` comment). H2 tests must keep the client-drop-before-join + `graceful_shutdown` + 5 s drain discipline (`lib.rs:238-280`).
8. **Features.** Does the default build stay H1-only, with H2 strictly behind `http2 = ["eggfetch-core/http2"]` (`Cargo.toml:11-13`)? H2 tests need `#[cfg(feature = "http2")]` (`lib.rs:196`, `regression.rs:220`).
9. **Docs.** Does `docs/eggfetch.md` still describe the implemented behavior (no new adapter-owned capabilities left undocumented)?

## 9. Verification

```sh
cargo test -p eggchaos-eggfetch
cargo test -p eggchaos-eggfetch --all-features   # includes http2 / H2 + TLS matrix
sh scripts/qualify_eggfetch.sh                   # eggfetch suite + eggchaos-server suite (all-features)
```

`scripts/qualify_eggfetch.sh:1-4` runs exactly:

```sh
cargo test -p eggchaos-eggfetch --all-features
cargo test -p eggchaos-server --all-features
```

Wall-clock assertions in this crate use justified windows, not bare sleeps: latency engagement ≥100 ms on a 150 ms fault (`regression.rs:123-127`), shaping ≥1200 ms at 4096 B/s over 8 KiB (`regression.rs:375-378`), blackhole non-completion within 2 s (`regression.rs:300-308`), all client phases under `ROUND = 20 s` (`regression.rs:20`). H2 phases use `CLIENT_PHASE = 120 s` / `SERVER_JOIN = 60 s` (`lib.rs:209-210`).

## 10. References

- `crates/eggchaos-eggfetch/src/lib.rs` — `ChaosDialer`, dial flow, H1 + H2 unit tests.
- `crates/eggchaos-eggfetch/tests/regression.rs` — 10-case HTTP regression matrix.
- `crates/eggchaos-eggfetch/Cargo.toml` — `http2` feature (`eggfetch-core/http2`).
- `docs/eggfetch.md` — user-facing contract (ownership split, H1/H2 profiles, no per-request chaos).
- `scripts/qualify_eggfetch.sh` — qualification entrypoint.
- `crates/eggchaos-core/src/policy.rs` — `LivePolicy` / `PublishedPolicy` snapshot + publish semantics.
- `crates/eggchaos-core/src/stream.rs:578-676` — `BidirectionalChaosStream::new_live` + generation reconciliation.
- `crates/eggchaos-core/src/rng.rs:17-63` — `derive_seed` / `derive_policy_seed`.
- `architecture/overview.md` — workspace index; this file is its §6 deep dive.
