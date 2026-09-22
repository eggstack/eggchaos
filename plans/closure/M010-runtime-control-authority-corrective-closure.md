# M010 closure — Runtime and Control Authority Corrective

Milestone: M010 (`010-runtime-control-authority-corrective.md`)
Candidate commit: `3961e968e98948cfab1d0c99d3503ba1624e2e6`
Implementation commits: `3961e968e98948cfab1d0c99d3503ba1624e2e6` (code), this closure record
Commands executed: see Verification
Platforms: macOS arm64 (developer host)
External oracle/version: none required for M010 (no Toxiproxy oracle surface)
Evidence artifacts: `cargo test --workspace --all-features` (65 tests total),
`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --all-features -- -D warnings`
Known limitations: listed below
Acceptance criteria verdict: pass — single authority, real lifecycle/CRUD/reset/CLI,
durable cancellation, guaranteed cleanup, truthful M009 termination consumption
Registry transition: M010 `active` -> `closed`
Next milestone activated: M011 (`ready`; M009 and M010 both closed)

## Work package disposition (WP1–WP8)

1. WP1 — Supervisor/control architecture: `ControlState` is the single
   runtime mutation authority. All proxy/connection/fault/reset mutations
   serialize through it, own the generation counter, and publish read
   snapshots; no registry mutates independently of listener ownership.
2. WP2 — Dynamic proxy lifecycle: create binds before registering (bind
   failure leaves no ghost); delete stops the listener and removes the
   definition; disable stops the listener while retaining the definition
   and fault plans; enable rebinds. Restart-class listen/upstream updates
   pre-bind the replacement before stopping the old listener, so a bind
   failure keeps the old listener serving with its bound address and spec
   untouched (no rebind/rollback skew).
3. WP3 — Durable cancellation/cleanup: level-triggered `CancellationToken`
   per connection/proxy/service (kill before relay select is still
   observed; shutdown cascades; proxy-scoped cancellation never touches
   unrelated proxies; repeated cancellation idempotent). No detached
   tasks: supervisors own connection `JoinSet`s, the root service owns
   supervisors, completion is signalled with a flag plus `DoneGuard` and
   awaited (`untrack`). `record_close` finalizes every connection exactly
   once; active and history registries are distinct, history bounded by
   `AdmissionLimits.history`.
4. WP4 — Fault/proxy native CRUD: typed `create/delete/set_enabled/
   update/add_fault/update_fault/remove_fault/publish_plans/reset/kill/
   history` plus versioned `/v1` routes (proxies, per-proxy faults,
   connections, history, reset, scenario apply). Canonical plans and live
   `LivePolicy` objects stay synchronized; fault CRUD is reflected in both
   GET snapshots and the active policy.
5. WP5 — Real reset: the native reset transaction empties fault plans,
   enables proxies, and changes real state (not just a response payload).
6. WP6 — CLI completion: thin Eggfetch-backed commands covering the full
   registered surface — `proxy add/set/remove/enable/disable/list/get`,
   `fault add/set/remove/list/get` (all seven fault kinds with typed
   parameters), `connection list/get/kill`, `reset`, `serve`, `version` —
   with `--json` single-document output and nonzero exit on failure. No
   in-process mutation path separate from the admin API.
7. WP7 — M009 termination integration: graceful requests end the relay
   deterministically (read-side EOF after the accepted prefix drains) and
   record `drained` evidence; hard-reset requests attempt a true abortive
   close at the concrete `TcpStream` edge (`ResettableTcpStream` +
   `TcpResetHandle` applying `SO_LINGER=0` via `socket2`) and report
   truthful per-socket `applied`/`unsupported`/`failed` evidence — never a
   FIN masquerading as a reset.
8. WP8 — Closure: end-to-end dynamic lifecycle, CLI/API, shutdown, and
   exact-commit evidence below.

## Core integration fixes (found during M010 testing)

Three relay-integration defects in `eggchaos-core/src/stream.rs` were
repaired; all 35 core tests (including every M009 test) still pass:

- `ChaosStream::poll_write`/`poll_write_vectored` checked the
  empty-engine fast path before observing the live generation, so a
  connection established without faults never engaged a later-published
  fault. The live generation is now observed first (matching
  `BidirectionalChaosStream`), with a paused-time regression test proving
  a live-published latency fault holds bytes on a previously direct
  connection.
- Already-due queued bytes sat until an unrelated flush because the
  embedding relay never flushes mid-stream (multi-chunk arrivals hung the
  echo intermittently). `poll_write` now drives due bytes immediately
  after acceptance; not-due bytes stay behind their release timers.
  `poll_flush` remains the delivery guarantee. Regression test: due bytes
  arrive with no explicit flush.
- An idle relay never ended after graceful termination (grace expiry
  recorded `drained: false`). A resolved graceful termination now
  surfaces as read-side EOF once the accepted prefix drains — live inner
  bytes are still delivered first; only a would-be-idle read becomes EOF.
  Hard-reset reads keep inner behavior (the runtime owns the abortive
  close). Regression test: reads see EOF after drain.

Two runtime test defects were also fixed: `upstream_update` sent a 5-byte
message but expected a 6-byte response (now symmetric 6-byte), and a
stray empty duplicate kill test was removed.

## Verification (exact candidate tree)

```text
cargo fmt --all -- --check                                             PASS
cargo clippy --workspace --all-targets --all-features -- -D warnings   PASS
cargo test -p eggchaos-core --all-features                             PASS (35 tests)
cargo test -p eggchaos-server --all-features                           PASS (23 tests, 3 consecutive runs)
cargo test -p eggchaos-cli --all-features                              PASS (1 end-to-end test)
cargo test -p eggchaos-toxiproxy --all-features                        PASS (3 tests)
cargo test -p eggchaos-eggfetch --all-features                         PASS (3 tests)
cargo test --workspace --all-features                                  PASS (65 tests total)
```

New coverage maps to every required test in the plan: port-0 bind with
actual address and relay, bind-conflict ghost check, delete stops
accepting, disable/enable round trip with plan retention, upstream update
redirect, failed restart keeps the old listener serving, serialized
unique generations under concurrency, fault CRUD canonical/live sync,
reset semantics, kill during upstream connect, kill during relay, many-
connection shutdown with zero active, finish under registry contention
with exact-once history, bounded history, CLI JSON
create/fault/kill/reset end-to-end against a live admin, graceful
disconnect with drain evidence, live latency engagement without
reconnect, and truthful hard-reset platform outcomes.

## Limitations (explicitly not M010 work)

- Linux/macOS/Windows CI has not been run from this host; only macOS
  arm64 execution evidence exists above. Platform-specific reset
  semantics and the plan's cross-platform lifecycle runs must be covered
  by CI/M013 before release.
- No wall-clock latency regression measurement against bare
  `eggress-relay` was taken; M013 owns the throughput/latency gate.
- Live-policy publication still stores the plan before bumping the
  generation (transient plan/generation skew for racing readers); atomic
  publication is M011 WP1.
- `proxy set --max-connections` cannot be combined with
  `--clear-max-connections` in one call (clap conflict); clearing uses a
  separate invocation.
