# M025 — Datagram Association Setup Waiter and Closure Hygiene

Status: ready
Depends on: M024
Role: post-M024 concurrency/planning hygiene

## Objective

Close the two small follow-up findings left after M024 without reopening its
qualified semantics or broadening the datagram roadmap:

1. make the association `Starting` state genuinely event-driven instead of
   polling with a bounded `yield_now()` loop; and
2. reconcile planning/status language so completed M024 work is not described
   as still active.

This is a hygiene pass, not a feature milestone. It must preserve ADR 003,
M024 performance behavior, native contracts, and the single
`DatagramRuntime` authority.

## Baseline

M024 closed on implementation candidate
`ca46801bb5f12a9ecf9232f33d8840bd0c09afad`. Closure/runtime formatting and
qualification bookkeeping subsequently landed on `b2fb031` and
`4f08e0a`.

Hosted evidence is green:

- CI run `36037905729` passed Ubuntu/macOS/Windows on the M024 closure tree.
- Release qualification run `36037910167` passed the complete release
  qualification surface on the M024 closure tree.
- Final documentation-only head `4f08e0a181aa32c0b9421101d072860390175f7f`
  also passed CI in run `36040145038`.

M024 correctly removed the association-registry lock from UDP bind/connect by
introducing `AssociationSlot::Starting`. Concurrent callers that encounter a
starting slot currently retry with `tokio::task::yield_now()` for up to
10,000 iterations. The slot carries an `Arc<Notify>` used as reservation
identity and setup/drain signaling, but waiters do not actually await that
notification.

That implementation is qualified and bounded, but the state model and comments
now imply stronger event-driven wake behavior than the code provides. It can
also cause needless task churn during a burst of first datagrams for one
client.

The plan registry also still labels the UDP/datagram roadmap area
`maintenance active` despite M024 being closed. Registering M025 should make
the state explicit rather than leaving contradictory bookkeeping.

## Scope

### In scope

- Replace the bounded `yield_now()` retry loop for an existing
  `AssociationSlot::Starting` with a retained/event-driven state transition.
- Preserve exactly-one association creation per client and exact
  global/per-proxy capacity accounting.
- Make setup reservation identity explicit rather than relying on an
  incidental synchronization object if that improves clarity.
- Prove there is no lost-wakeup race between observing `Starting`, setup
  publication/failure/drain, and beginning the wait.
- Preserve setup takeover after bind/connect failure or administrative drain.
- Keep delete/update/listener-stop behavior leak-free while setup is in flight.
- Add deterministic or controlled concurrency tests that hold setup in
  `Starting` long enough to exercise waiter races.
- Reconcile registry/roadmap/AGENTS wording after implementation and closure.
- Re-run M024 performance regression gates to ensure the waiter cleanup does
  not regress first-association or steady-state datagram behavior.

### Non-goals

- No new datagram fault kinds or semantic changes.
- No changes to `DatagramDirectionEngine`, the heap scheduler, immediate
  emission, RNG, golden traces, or queue limits unless a correctness bug is
  discovered while testing.
- No change to per-client connected upstream socket ownership.
- No native API, JSON/TOML, CLI, metrics schema, or Toxiproxy changes.
- No change to normal active-association ingress MPSC semantics.
- No broad runtime refactor after M024's module split.
- No new production dependency; Tokio synchronization primitives are already
  available.
- No busy-spin, sleep-based polling, arbitrary retry count, or wall-clock
  timeout as the normal waiter mechanism.
- No claim that M024 was invalid; this is a maintainability/concurrency hygiene
  successor.

## Affected surfaces

Expected implementation areas:

- `crates/eggchaos-server/src/runtime/datagram/association.rs`;
- `crates/eggchaos-server/src/runtime/datagram/model.rs` if the reservation
  token/state type belongs there;
- `crates/eggchaos-server/src/runtime/datagram/supervisor.rs` only if setup
  notification/drain plumbing requires it;
- `crates/eggchaos-server/src/runtime/datagram/tests.rs`;
- `architecture/server-runtime.md`;
- `plans/registry.md`, `plans/roadmap.md`, `plans/README.md`, and
  `AGENTS.md` at closure.

Keep changes outside these surfaces exceptional and document why they were
required.

## Ordered work packages

### WP1 — Make the setup state machine explicit

Document the exact state transitions for one client address:

```text
Absent
  -> Starting(reservation)
  -> Active(association)

Starting(reservation)
  -> Absent       # bind/connect failure or administrative drain

Active(association)
  -> Absent       # kill/idle/delete/update/listener shutdown
```

The starting reservation must carry enough identity to distinguish an old
setup owner from a later takeover for the same client address.

Do not use pointer identity of a wake primitive as the only conceptual
reservation identity unless the implementation and documentation deliberately
choose that design and tests prove it safe.

### WP2 — Replace yield polling with retained/event-driven waiting

A caller that finds `Starting` must suspend until that specific reservation
changes rather than repeatedly calling `yield_now()`.

Use a synchronization pattern with retained/versioned transition semantics,
such as Tokio `watch`, or an equivalently safe checked-state +
notification protocol.

A naïve:

```rust
notify.notified().await
```

after releasing the registry lock is **not sufficient** because
`notify_waiters()` can occur before the waiter is registered. If `Notify`
is retained, the implementation must explicitly prove the arm/recheck order
that prevents lost wakeups. Prefer a primitive whose state/version can be
observed after a wake was emitted.

Required behavior:

- publication to `Active` wakes every same-client waiter;
- abandonment/drain wakes every same-client waiter;
- after publication, waiters return the one active association;
- after abandonment, one waiter may reserve the now-absent slot and retry
  setup while the others wait/observe normally;
- no waiter requires an arbitrary iteration bound;
- cancellation of a caller does not cancel the setup owner or leak capacity.

### WP3 — Preserve exact reservation/capacity ownership

Audit all exits from setup:

- bind failure;
- connect failure;
- successful publication;
- proxy update;
- proxy delete;
- listener stop/disable;
- service shutdown;
- race where administrative drain removes the reservation before publication.

For every path, global association capacity and the per-proxy slot are released
exactly once. A stale setup owner must never remove or overwrite a newer
reservation/association for the same client.

The registry lock must remain absent across UDP bind/connect and worker joins.

### WP4 — Add controlled concurrency regression tests

Add tests that intentionally keep a setup reservation in `Starting` while
other operations race it. Prefer a test-only barrier/hook or equivalent
deterministic seam over scheduler luck or sleeps.

Minimum cases:

1. many simultaneous first datagrams from one client converge on one active
   association without a yield loop;
2. many clients starting concurrently respect global and per-proxy caps;
3. setup publication wakes all waiters and all observe the same association;
4. setup failure wakes waiters and permits exactly one takeover;
5. delete/disable/update while setup is paused drains the reservation, wakes
   waiters, and leaks no socket/task/capacity;
6. stale setup owner cannot publish over a newer reservation;
7. waiter task cancellation leaves setup and capacity correct;
8. no test relies on a fixed 10,000-iteration progress assumption.

If a test-only setup hook is introduced, keep it private/`cfg(test)` and out
of production state/API.

### WP5 — Recheck performance and runtime behavior

Run the M024 topology-matched datagram benchmark on the exact candidate.

The existing matched budgets remain the regression thresholds:

- empty-plan / bare sequential throughput >= 0.7;
- empty-plan / bare sequential p95 <= 1.6;
- empty-plan / bare windowed throughput >= 0.7;
- retained M023 direct comparison >= 0.45 throughput and <= 2.5 p95.

Record the raw report if the repository convention records every maintenance
candidate; otherwise include the benchmark summary in closure evidence.

Steady-state active-association throughput should not materially regress merely
because first-association waiting changed. If it does, investigate rather than
loosening the budget.

### WP6 — Planning/documentation closure reconciliation

At implementation start, the registry should show M025 as the sole ready
handoff. At closure:

- M025 -> `closed`;
- Active -> none;
- Blocked -> none;
- UDP/datagram roadmap state -> completed/maintenance complete, not
  `maintenance active`;
- M024 closure/history stays untouched;
- server-runtime documentation describes the actual waiter primitive/state
  transitions;
- AGENTS reflects the final association setup rule.

Create
`plans/closure/M025-datagram-association-setup-waiter-and-closure-hygiene-closure.md`.

## Invariants

- ADR 003 semantics remain unchanged.
- M024's 14-case datagram golden corpus remains byte-for-byte unchanged.
- One client address maps to at most one active association.
- At most one setup reservation for a client is live at a time.
- A setup owner can publish only if it still owns the current reservation.
- Global association capacity is incremented/released exactly once per
  reservation lifecycle.
- No registry mutex is held across UDP bind/connect or task join.
- Waiters do not busy-spin or depend on an arbitrary retry count.
- Wakeup/state transition cannot be lost if it occurs immediately before a
  waiter suspends.
- Administrative drains wake setup waiters and cannot leak tasks/sockets.
- Public control/config/CLI/Toxiproxy contracts remain unchanged.
- `unsafe_code = "forbid"` remains true.
- No new production dependency is introduced.

## Verification

Minimum exact-candidate gates:

```sh
cargo fmt --all -- --check
cargo clippy -p eggchaos-server --all-targets --all-features -- -D warnings
cargo test -p eggchaos-server --all-features
./scripts/check.sh
./scripts/benchmark_datagram.sh
EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh
./scripts/release-smoke.sh
cargo audit --deny warnings
cargo deny check advisories licenses bans sources
```

Because the release qualification workflow already exercises the complete
cross-project regression surface, dispatch it on the exact candidate as the
final hosted gate. It must include the strict pinned Toxiproxy differential,
Eggfetch qualification, datagram benchmark, fuzz qualification, and artifact
smokes. Ordinary CI must also pass Ubuntu/macOS/Windows.

## Acceptance criteria

M025 closes only when:

- the `Starting` waiter path contains no bounded `yield_now()` polling loop;
- waiters block on a retained/event-driven transition with a documented
  no-lost-wakeup argument;
- reservation ownership is explicit enough that stale setup cannot publish or
  release a successor's capacity;
- controlled tests cover publication, failure/takeover, administrative drain,
  cancellation, caps, and stale-owner races;
- registry lock ownership still excludes UDP bind/connect and worker joins;
- all M024/M023 datagram performance budgets remain green;
- full workspace, fuzz, release smoke, security, three-OS CI, and release
  qualification are green on the exact candidate;
- planning/docs no longer describe closed M024/M025 maintenance as active;
- no unresolved medium-or-higher correctness or maintainability finding
  remains for this narrow pass.

## Stop/rejection conditions

Do not close if:

- the new waiter can miss a transition and sleep indefinitely;
- progress depends on sleeps, retry counts, or scheduler fairness;
- one client can create two active associations;
- setup failure/drain can leak or double-release capacity;
- a stale owner can overwrite a newer slot;
- cancellation of a waiter tears down another task's setup;
- the fix reintroduces lock-across-network-await;
- any ADR 003 trace or public contract changes;
- performance budgets are weakened instead of investigated;
- hosted qualification is skipped because the change is "only hygiene."

## Follow-on activation

A clean M025 closes the known M024 follow-up findings and activates no
successor. Further datagram work returns to the normal roadmap and requires a
new numbered plan; semantic expansion still requires an ADR.
