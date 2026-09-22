# M003 — Fixed-Target Proxy Runtime

Status: blocked  
Depends on: M002  
Successor: M004

## Objective

Build the standalone and embeddable TCP runtime around the proven core fault engine, reusing `eggress-relay` as the bidirectional relay authority.

The runtime must manage multiple fixed-target proxies, connection lifecycle, admission limits, graceful shutdown, and truthful concrete TCP reset capabilities without becoming a general forward proxy.

## User-visible outcome

Rust callers can create a service containing one or more named proxy definitions such as:

```text
redis:
  listen   127.0.0.1:26379
  upstream 127.0.0.1:6379
  upstream faults [...]
  downstream faults [...]
```

and receive a stable handle with bound addresses, lifecycle control, and connection snapshots.

No admin HTTP API is required yet; configuration may be supplied through Rust types/test fixtures.

## Preconditions

M002 is closed:

- core fault engine is deterministic;
- buffers are bounded;
- empty/preserving fault behavior is proven;
- termination requests are typed;
- no-fault benchmark baseline exists.

## Scope

Primary work lives in `eggchaos-server`.

Expected domains:

- `ProxySpec` / validated runtime config;
- `ProxyRegistry` or service-owned immutable definition set;
- listener supervisor;
- direct upstream connector;
- per-proxy monotonic connection ordinal;
- global unique connection ID;
- admission limits;
- `ConnectionContext`;
- construction of upstream/downstream `ChaosStream` wrappers;
- `eggress_relay::relay_with_options`;
- connection outcome classification;
- graceful shutdown/drain;
- hard-reset capability edge;
- embeddable `EggchaosService` / handle surface.

## Non-goals

Do not add:

- arbitrary CONNECT/SOCKS destination selection;
- Eggress protocol detection/routing;
- admin HTTP;
- CLI remote mutation;
- Toxiproxy endpoints;
- UDP;
- `eggress-outbound` proxy chains;
- persistence/database;
- TLS interception.

## Fixed-target proxy definition

A native `ProxySpec` should contain at least:

- stable name/id;
- listen socket address;
- upstream target host/address + port;
- enabled state;
- upstream `FaultPlan`;
- downstream `FaultPlan`;
- per-proxy connection limit override if supported;
- relay half-close/drain policy;
- connect timeout;
- fault buffer policy inherited/defaulted from core as appropriate.

Separate parse/config representation from compiled runtime values if that keeps validation single-source.

Port `0` must be supported for tests/ephemeral use, with actual bound address exposed through the runtime handle.

## Listener model

Each enabled proxy owns a listener task supervised by the parent service.

Requirements:

- bind failures are typed and associated with proxy identity;
- one proxy failing to bind during initial atomic service start should produce a clear startup result rather than leaving a silently partial service, unless an explicit partial-start mode is designed;
- accept loops honor service cancellation;
- accepted sockets are registered before child tasks can become invisible;
- connection tasks are structured/owned so `wait`/shutdown can drain them;
- no detached task may survive the service handle's completed shutdown result.

Use Tokio cancellation primitives with durable state, not notify-only races.

## Upstream dialing

Initial path is direct TCP.

Requirements:

- hostname and IP-literal support;
- bounded connect timeout;
- safe error classification;
- no retry loop unless explicitly configured later;
- no automatic general proxy use;
- no credential-bearing upstream URL syntax in the initial target model.

If DNS resolution behavior is delegated to Tokio/std, document it. More advanced resolver behavior is not required for M003.

## Data-plane composition

For every accepted client:

1. assign connection ID/ordinal and capture the current proxy generation;
2. decide deterministic fault activations using the connection key;
3. dial the configured upstream;
4. wrap the upstream destination stream so writes receive the upstream fault engine;
5. wrap the client stream so writes receive the downstream fault engine;
6. call `eggress_relay::relay_with_options`;
7. combine relay report/failure with injected termination/evidence;
8. remove or finalize the active connection record;
9. retain only bounded summary history if history is part of the M003 handle.

Do not copy Eggress relay internals.

## Half-close policy

Expose Eggress's semantics deliberately.

Default should normally preserve useful request-half-close/response behavior. If a bounded drain is chosen as operational default, document the duration and allow explicit configuration.

Tests must include a server that waits for client EOF before responding.

Injected directional faults must not accidentally convert a valid half-close into immediate full connection teardown unless the fault's contract says so.

## Connection admission

Provide explicit bounds:

- global active connections;
- per-proxy active connections;
- optionally connection task/history retention.

Define whether excess accepted connections are immediately closed or prevented by pre-accept/backpressure. Whichever approach is used, expose a classified admission outcome.

Never allow an unbounded task-per-connection registry.

## Connection identity and evidence

An active connection snapshot should include safe operational facts:

- connection ID;
- proxy name/id;
- proxy-local accept ordinal;
- peer/local address;
- upstream target/address where safe;
- accepted generation/current generation;
- started duration/monotonic timing information;
- byte counters;
- selected fault IDs;
- RNG version;
- termination state.

Do not expose credentials or arbitrary application data.

## Reset capability

M002 emits an abstract hard-reset request.

At the concrete TCP boundary M003 must evaluate whether a true reset can be induced portably/safely on the current platform.

Define a capability result such as:

- `SupportedAndApplied`;
- `Unsupported`;
- `Failed(error_kind)`.

If using socket linger or platform APIs:

- keep unsafe code out of eggchaos if `socket2` exposes the needed safe operation;
- verify behavior separately on Linux/macOS/Windows;
- do not claim “RST” from an ordinary Tokio shutdown/EOF;
- if the implementation cannot reliably observe/reset cross-platform, ship graceful disconnect plus truthful best-effort reset status and leave exact reset qualification to M008.

A reset limitation is acceptable. False capability reporting is not.

## Service API

Provide an embeddable async API along the lines of:

```text
EggchaosService::builder(...)
  .proxy(...)
  .limits(...)
  .build()?

handle = service.start().await?
handle.bound_proxies()
handle.connections()
handle.shutdown()
handle.wait()
```

Exact names may differ.

The handle should not expose implementation task handles as the primary contract.

M004 will add mutation/control methods. M003 can keep proxy definitions immutable after start except enable/disable if that is naturally needed for lifecycle tests.

## Test reuse

Use `eggress-testkit` for:

- echo origin;
- half-close server;
- slow I/O fixtures;
- fragmented stream behavior where useful.

Add eggchaos-specific fixture servers only for behaviors not already represented.

## Required integration tests

From the verification matrix, at minimum:

- one proxy/no faults echo;
- upstream-only latency;
- downstream-only latency;
- simultaneous bidirectional traffic;
- request half-close then delayed response;
- target closes first;
- upstream refusal;
- connect timeout if reproducibly injectable;
- client disconnect;
- proxy port 0 reports actual bound port;
- two proxy listeners operate independently;
- bind conflict startup result;
- per-proxy/global admission limit;
- service graceful shutdown while idle;
- service graceful shutdown with active relay;
- forced/deadline shutdown if supported;
- connection record created/finalized exactly once;
- injected limit/disconnect outcome classified separately from ordinary I/O failure;
- no orphan connection tasks after `wait`.

## No-fault baseline

Add a real TCP benchmark or repeatable benchmark harness comparing:

- direct client <-> echo origin;
- fixed-target `eggress-relay` baseline if exposed conveniently;
- eggchaos runtime with empty plans.

Do not freeze a release threshold yet, but record throughput/latency and candidate hardware/runtime for M008.

## Acceptance criteria

M003 closes only when:

- multiple fixed-target proxies can be started/stopped through Rust API;
- every relay uses `eggress-relay`;
- upstream/downstream direction mapping is proven;
- half-close behavior is preserved in no-fault and representative fault cases;
- connection limits and lifecycle are bounded;
- shutdown owns/drains child tasks;
- port 0 and bind errors behave correctly;
- concrete reset capability is truthful and tested where available;
- no general forward-proxy behavior is present;
- integration tests and routine CI pass;
- no-fault runtime benchmark is recorded;
- closure evidence is committed.

## Stop/rejection conditions

Stop and revise if:

- using `eggress-relay` forces duplication of fault state or makes direction impossible to express correctly;
- graceful shutdown requires detached/untracked connection tasks;
- a disabled/failed listener can leave the service in an unknowable partial-start state;
- hard reset requires unsafe code not already covered by an approved ADR;
- implementing fixed-target forwarding starts pulling the entire Eggress protocol/routing graph without a demonstrated need;
- connection history/metrics state grows without a bound.

## Closure evidence

Create `plans/closure/M003-fixed-target-proxy-runtime-closure.md` with:

- candidate SHA;
- exact integration commands/results;
- Eggress crate/version used;
- half-close cases;
- reset capability results by tested platform;
- connection/shutdown leak evidence;
- benchmark result;
- acceptance verdict.

Then update registry:

- M003 -> `closed`;
- M004 -> `ready`.
