# Eggfetch in-process integration

Back to [architecture overview](overview.md) (§ “Eggfetch in-process integration”, workspace map row `eggchaos-eggfetch`).

## 1. Purpose

`eggchaos-eggfetch` provides in-process physical-stream chaos for HTTP clients via Eggfetch’s public `Dialer` seam. `ChaosDialer<D>` decorates an arbitrary caller-selected inner dialer `D` and wraps the returned physical stream as a `BidirectionalChaosStream` carrying a live upstream/downstream fault policy. The direct convenience form `ChaosDialer<DirectDialer>` composes the same single wrapping path over a small built-in direct TCP dialer. Eggfetch continues to own everything above the physical stream.

Canonical contract: `docs/eggfetch.md`. Implementation: `crates/eggchaos-eggfetch/src/lib.rs` with composition/identity/evidence tests in `crates/eggchaos-eggfetch/src/tests.rs`. Feature gate: `crates/eggchaos-eggfetch/Cargo.toml`. Regression matrix: `crates/eggchaos-eggfetch/tests/regression.rs`. Qualification entrypoint: `scripts/qualify_eggfetch.sh`.

Non-goals (explicit in `docs/eggfetch.md` and `crates/eggchaos-eggfetch/src/lib.rs` header):

- No per-request chaos. Policy is physical-connection scoped; one pooled H2 connection exposes one policy to many logical streams.
- No HTTP parsing, body inspection, or TLS interception in the adapter.
- No routing grammar or proxy-chain ownership. The inner dialer owns route behavior; only the `DirectDialer` convenience performs direct TCP dialing to the resolved `DialTarget` host/port.
- No pooling, retry, SNI, or certificate-verification logic in the adapter.
- No EggReplay/EggProbe dependency or product-specific report type.

## 2. `ChaosDialer<D>` shape

`ChaosDialer` is generic over the inner dialer with a direct default (`ChaosDialer<DirectDialer>`), so existing `ChaosDialer::new` / `with_policies` call sites keep working:

```rust
pub struct ChaosDialer<D = DirectDialer> {
    inner: D,                                    // route authority (DirectDialer or caller-selected)
    upstream: LivePolicy,                        // physical-stream client→server direction
    downstream: LivePolicy,                      // physical-stream server→client direction
    proxy_name: Arc<str>,                        // RNG identity component
    integration_id: Arc<str>,                    // caller identity, ≤128 bytes, key-derivation input
    key_provider: Arc<dyn ConnectionKeyProvider>,// deterministic connection-key source
    observer: Option<Arc<dyn ConnectionObserver>>,// optional per-dial evidence sink
    connection_ordinal: Arc<AtomicU64>,          // starts at 1, fetch_add per successful dial
}
```

`Clone` (requires `D: Clone`) is cheap (`LivePolicy`, `Arc`s all shared); clones share the same policy handles, provider, observer, and ordinal counter, so pooled clients and manual `dialer.clone()` handles observe the same generations. A manual `Debug` impl reports proxy/integration identity and policies without requiring `D: Debug`.

Constructor / accessor surface:

| Method | Semantics |
| --- | --- |
| `new(run_seed, proxy_name)` / `with_policies(run_seed, proxy_name, upstream, downstream)` (on `ChaosDialer<DirectDialer>` only) | Direct convenience: empty or validated initial directional plans; both `LivePolicy` namespaces set to `run_seed` (generation 1). Ordinal counter starts at `INITIAL_CONNECTION_ORDINAL = 1`. |
| `wrap(inner, run_seed, proxy_name)` / `wrap_with_policies(inner, run_seed, proxy_name, upstream, downstream)` | Decorate any `D: Dialer` with empty or initial directional plans. |
| `with_connect_timeout(timeout)` (on `ChaosDialer<DirectDialer>` only) | Bounded physical connect timeout override; rejects values above `MAX_CONNECT_TIMEOUT` (300 s) with `ChaosConfigError`. |
| `with_integration_id(id)` | Caller identity mixed into key derivation and reports; rejects values above `MAX_INTEGRATION_ID_BYTES` (128) with `ChaosConfigError`. |
| `with_key_provider(provider)` | Override the deterministic key source (default: ordinal). |
| `with_observer(observer)` | Optional synchronous per-dial evidence sink. |
| `inner()` | Borrow the route-authoritative inner dialer. |
| `upstream_policy()` / `downstream_policy()` | Return cloned `LivePolicy` handles (shared state, not snapshots). |
| `publish_upstream(plan)` / `publish_downstream(plan)` | Validate `plan`, publish generation `N+1` retaining the handle's `seed_namespace()`. Returns new generation or `ValidationError`. |

`connection_ordinal` uses `Ordering::AcqRel` fetch-add and is assigned only after the inner dial succeeds, so failed inner dials consume no ordinal and create no evidence. First successful dial observes ordinal `1`, second `2`, and so on.

### 2.1 Connection-key providers

`ConnectionKeyContext { ordinal, target, integration_id }` is the only derivation input. The default `DefaultConnectionKeyProvider` returns the ordinal (historical behavior). `KeyError` bounds provider failure messages to `MAX_KEY_ERROR_BYTES` (256, truncated on a character boundary) and fails the dial as `DialErrorKind::Other` before bytes are exposed. A blanket impl covers `Arc<T: ConnectionKeyProvider>`.

### 2.2 Evidence observer

`ConnectionReport { ordinal, connection_key, target_host, target_port, integration_id, evidence: LiveBidirectionalEvidence }` is delivered synchronously to `ConnectionObserver::on_connection` before the stream is handed to Eggfetch, so observer failure can never corrupt a stream Eggfetch already owns. `RecordingObserver` is the bounded test collector: explicit capacity, oldest-first eviction, `records()` in dial order. The adapter itself retains no history and spawns no tasks. `DialTarget` never carries credentials/headers/paths, so reporting host/port retains no secrets.

## 3. Dial flow

`impl<D: Dialer> Dialer for ChaosDialer<D>`: exactly one inner `dial` attempt per call; the inner `DialError` (kind and source) passes through untouched — eggchaos never retries or redials.

Ordered steps:

1. **Inner dial.** `self.inner.dial(target.clone()).await?`. Route/connect behavior, including resolution, authentication, timeout, and multi-hop routing, is entirely the inner dialer's. A route-authoritative fixture dialer that ignores the logical host proves eggchaos neither resolves nor redials (`tests.rs: routed_inner_dialer_is_never_resolved_or_redialed`).
2. **Identity.** `fetch_add` the ordinal, then `key_provider.connection_key(&ctx)`. Provider failure maps to `DialErrorKind::Other` with a bounded message; no evidence is recorded.
3. **Wrap (single path).** `BidirectionalChaosStream::new_live(stream, upstream, downstream, &*proxy_name, connection_key)`. Construction compiles two `DirectionEngine`s from atomic policy snapshots (see §5). An `EngineError` maps to `DialErrorKind::Other`.
4. **Observe.** If configured, `observer.on_connection(&report)` with the live evidence handle.
5. **Return.** `Ok(Box::new(stream) as DialStream)`. From this point Eggfetch owns framing/TLS/pooling above the stream.

Direct-mode error mapping (all messages prefixed `"direct route …"` so dial-origin failures are distinguishable in client error surfaces; asserted in `tests/regression.rs:405-426`):

| Failure | `DialErrorKind` | Message |
| --- | --- | --- |
| `lookup_host` fails | `Connection` (with `io` source) | `"direct route resolution failed"` |
| Per-address connect times out | `Timeout` | `"direct route connect timed out"` |
| All addresses refuse/fail | `Connection` (with last `io` source, or `NotFound` when zero addresses) | `"direct route connection failed"` |
| `new_live` engine construction fails | `Other` | `error.to_string()` |

Composed-mode failures (inner dialer errors of any `DialErrorKind`, provider `KeyError`) are asserted in `src/tests.rs` (`inner_error_kinds_pass_through_with_observer_silence`, `provider_failure_fails_dial_with_bounded_error`).

Deliberately absent: any HTTP status handling, TLS handshake, SNI selection, certificate validation, pool lookup, or retry.

## 4. Ownership split

| Concern | Adapter (`ChaosDialer`) owns | Eggfetch owns |
| --- | --- | --- |
| DNS / address iteration | `DirectDialer` only: `lookup_host` + per-address loop | Nothing (receives one `DialStream`) |
| TCP connect bound | `DirectDialer.connect_timeout`, default 10 s, max 300 s | Client-level request timeouts above the stream |
| Route/auth/timeout/errors | Inner dialer `D` (sole authority; errors pass through) | Receives one impaired `DialStream` |
| Physical-stream faults | `LivePolicy` pair + `BidirectionalChaosStream` wrapper (single `wrap_stream` path) | Observes faults as slow/failed reads/writes |
| Connection identity | `proxy_name` + ordinal + caller `integration_id` + `ConnectionKeyProvider` → RNG seed | Connection pooling / reuse decisions |
| Transport evidence | `LiveBidirectionalEvidence` + optional `ConnectionObserver` (no retained history) | Correlates observations out of band |
| HTTP framing (H1/H2) | Nothing | Request encoding, response parsing, H2 multiplexing (`tests.rs: H1/H2 cases`, `http2` feature) |
| TLS, SNI, cert verification | Nothing | Handshake above the returned stream; `TlsConfig`, `danger_accept_invalid_certs`, added-CA trust (`tests/regression.rs:131-174,176-218`) |
| Pooling, keep-alive, redial | Nothing (each `dial` = one inner dial + wrap) | Keep-alive reuse, idle-close redial (`tests/regression.rs:96-128,381-402`) |
| Retry policy | Nothing (never redials after inner success) | Client send/retry behavior |

The header comment states the split normatively: “Eggfetch remains the authority for HTTP framing, pooling, TLS, SNI, and certificate verification.”

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

HTTP regression matrix in `crates/eggchaos-eggfetch/tests/regression.rs`; composition/identity/evidence/pooling layer in `crates/eggchaos-eggfetch/src/tests.rs` (M029).

| Test | Location | What it pins |
| --- | --- | --- |
| `direct_convenience_mode_still_resolves_and_connects` | `tests.rs` | Raw TCP dial + read through chaos stream, no HTTP/TLS in adapter. |
| `eggfetch_owns_http_over_the_physical_chaos_stream` | `tests.rs` | Real H1 GET ×2 through `eggfetch_core::Client` over chaos dialer. |
| `eggfetch_owns_tls_and_http2_over_the_physical_chaos_stream` (`http2`) | `tests.rs` | TLS + ALPN-h2 + 2 sequential H2 bodies over one physical connection (observer: exactly 1 record); bounded teardown discipline. |
| `custom_dialer_target_forwarded_with_single_attempt` | `tests.rs` | Inner dial target forwarded exactly; exactly one inner attempt. |
| `routed_inner_dialer_is_never_resolved_or_redialed` | `tests.rs` | Route-authoritative inner dialer decides the destination; requested target preserved. |
| `inner_error_kinds_pass_through_with_observer_silence` | `tests.rs` | All five `DialErrorKind`s pass through; failed dials create no evidence. |
| `provider_receives_expected_ordinal_target_and_identity` | `tests.rs` | Provider sees `(ordinal 1, target, integration id)`. |
| `identical_provider_inputs_give_identical_keys` | `tests.rs` | Deterministic derivation; default provider returns the ordinal. |
| `default_keys_follow_successful_dial_ordinals` | `tests.rs` | Failed dials consume no ordinal; successful dials key `1, 2, …`. |
| `caller_selected_collision_is_explicit_and_stable` | `tests.rs` | Constant provider collides keys while ordinals stay distinct. |
| `h1_keepalive_reuse_observes_one_physical_key` | `tests.rs` | 2 sequential H1 GETs → 1 dial, 1 evidence record. |
| `forced_separate_h1_connections_receive_distinct_keys` | `tests.rs` | Separate pools → distinct keys. |
| `live_policy_update_reaches_pooled_connection` | `tests.rs` | Published upstream latency engages pooled conn: generation 2, 1 transition, injected delay, active fault id. |
| `evidence_records_bytes_and_survives_stream_drop` | `tests.rs` | Empty-plan byte counts both directions; handle readable after drop. |
| `evidence_records_termination_without_payload` | `tests.rs` | Graceful disconnect evidence with fault id, no payload. |
| `observer_called_once_per_wrapped_dial` | `tests.rs` | Exactly-once observer invocation with ordinals `1, 2`. |
| `provider_failure_fails_dial_with_bounded_error` | `tests.rs` | `Other` error, bounded message, no evidence. |
| `recording_observer_evicts_oldest_first` | `tests.rs` | Capacity 2 over 3 dials retains ordinals `2, 3`. |
| `configuration_bounds_are_rejected` | `tests.rs` | Integration-id, connect-timeout, and key-error bounds. |
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

1. **Boundary.** Does the diff parse HTTP, inspect bodies, terminate TLS, select SNI, verify certs, pool, retry, or redial after inner success? If yes, it belongs in Eggfetch, not here (`docs/eggfetch.md`).
2. **Dial errors.** Do direct-mode failures keep the `resolution / connection / timeout` mapping with `"direct route …"` messages? Do composed-mode failures pass the inner `DialError` through untouched (`tests.rs: inner_error_kinds…`)?
3. **Ordinal.** Is `connection_ordinal` still `fetch_add` from `INITIAL_CONNECTION_ORDINAL` with `AcqRel`, assigned only after inner success, with failed dials creating no evidence? Cloned dialers must share the counter.
4. **Namespace.** Do `publish_upstream` / `publish_downstream` retain `seed_namespace()` (`policy.rs`)? Fresh namespaces come only from scenario derivation (`rng.rs`).
5. **Identity.** Does any RNG-affecting change preserve the `(run_seed, proxy, connection_key, direction, fault.id)` derivation (`rng.rs:17-36`, `engine.rs:373`)? Key providers must not observe request order, wall clock, UUIDs, or scheduling.
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

- `crates/eggchaos-eggfetch/src/lib.rs` — `ChaosDialer<D>`, `DirectDialer`, key providers, observer contract.
- `crates/eggchaos-eggfetch/src/tests.rs` — composition/identity/evidence/pooling corpus plus H1 + H2 client tests.
- `crates/eggchaos-eggfetch/tests/regression.rs` — 10-case HTTP regression matrix.
- `crates/eggchaos-eggfetch/Cargo.toml` — `http2` feature (`eggfetch-core/http2`).
- `docs/eggfetch.md` — user-facing contract (ownership split, H1/H2 profiles, no per-request chaos).
- `scripts/qualify_eggfetch.sh` — qualification entrypoint.
- `crates/eggchaos-core/src/policy.rs` — `LivePolicy` / `PublishedPolicy` snapshot + publish semantics.
- `crates/eggchaos-core/src/stream.rs:578-676` — `BidirectionalChaosStream::new_live` + generation reconciliation.
- `crates/eggchaos-core/src/rng.rs:17-63` — `derive_seed` / `derive_policy_seed`.
- `architecture/overview.md` — workspace index; this file is its §6 deep dive.
