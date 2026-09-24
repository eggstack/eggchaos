# M025 — Datagram Association Setup Waiter and Closure Hygiene — Closure

Status: closed
Exact implementation candidate: `55911f6b72d99367348ee7c5438a2f270cbaf895`
Implementation commit: `55911f6b72d99367348ee7c5438a2f270cbaf895`
Closure commit: this file's commit; the implementation candidate above is the qualified code head
Depends on: M024 (closed at `ca46801`)

## What M025 delivered

M025 is a semantics-preserving concurrency and planning-hygiene successor to
M024. It does not change ADR 003, datagram fault semantics, deterministic golden
traces, native JSON/TOML/CLI contracts, Toxiproxy contracts, or per-client
connected upstream socket ownership. The single `DatagramRuntime` authority
remains intact.

The association setup state machine is now explicit:

```text
Absent
  -> Starting(reservation)
  -> Active(association)

Starting(reservation)
  -> Absent       bind/connect failure or administrative drain

Active(association)
  -> Absent       kill/idle/delete/update/listener shutdown
```

Each `Starting` reservation has a monotonic identity independent of the
public association ID, a retained/versioned Tokio `watch` transition, and an
idempotent capacity lease covering both global and per-proxy counters. A
waiter subscribes while holding the association-map lock and waits with
`watch::Receiver::wait_for` for a terminal version. Publication, setup
failure, and drain change the map and publish the terminal transition under
that same lock. A transition before subscription leaves the terminal state
retained; a transition after subscription wakes the receiver. There is no
retry count, yield loop, sleep-based waiter, or lost-wakeup window.

A retryable abandonment wakes same-client waiters so one can reserve a new
identity. A terminal drain wakes them to a conflict. A stale setup owner is
rejected by the reservation identity and can only release its own lease; it
cannot publish over or decrement a successor. The unpublished worker handle is
abort-on-drop, and listener cleanup is cancellation-safe. The registry mutex
is not held across UDP bind/connect, worker joins, or other network waits.

A private `cfg(test)` setup gate holds setup before bind and before publication
so the race tests use deterministic barriers rather than scheduler timing.
It is absent from production state and public APIs.

## Verification on the exact candidate

All commands below ran against `55911f6b72d99367348ee7c5438a2f270cbaf895`.

### Local gates

- `./scripts/check.sh` — pass: workspace fmt, Clippy with `-D warnings`, all
  workspace tests, and documentation.
- `cargo test -p eggchaos-server --all-features` — pass: 75 server tests,
  including the controlled waiter, failure/takeover, retained-transition,
  capacity, cancellation, stale-owner, update, disable, and delete races.
- `./scripts/benchmark_datagram.sh` — pass; raw report:
  `qualification/performance/2026-09-24-macos-arm64-m025.json`.
- `EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh` — pass for all 8
  targets: `plan_json`, `datagram_plan_json`, `datagram_transitions`,
  `native_config`, `native_control_json`, `fault_evidence_json`,
  `policy_transitions`, and `toxiproxy_attributes`.
- `./scripts/release-smoke.sh` — pass, including audit, deny, package-list
  publish-order proof, release build, and artifact smoke.
- `./scripts/qualify_eggfetch.sh` — pass for Eggfetch and server regression
  suites.
- `TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" \
  EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 \
  ./scripts/qualify_toxiproxy_v2_12.sh` — pass against checksum-verified
  `toxiproxy-server 2.12.0`; current differential result 50/50.
- `cargo audit --deny warnings` — pass.
- `cargo deny check advisories licenses bans sources` — pass.

The strict Toxiproxy oracle was not treated as a `PATH` binary: the fetch
script verified the pinned asset and checksum before qualification.

### Hosted gates

- CI run `36047792068` — pass on Ubuntu, macOS, and Windows. The run used
  `headSha=55911f6b72d99367348ee7c5438a2f270cbaf895` and included the visible
  IPv6 datagram capability check, docs, audit, and deny checks.
- Release qualification run `36047821492` — pass with the same exact
  `headSha`. The qualify job passed release smoke, datagram benchmark, 10,000
  fuzz runs, strict pinned Toxiproxy qualification, Eggfetch, and artifact
  smoke. All five artifact jobs passed: Linux x86_64, Linux AArch64,
  macOS x86_64, macOS AArch64, and Windows MSVC, each with checksum output.

GitHub emitted only non-blocking Node.js 20 action-runtime deprecation
annotations. No test, qualification, artifact, audit, or deny job failed.

## Performance evidence

The local raw report records Apple M4 Pro, macOS arm64, rustc 1.89.0,
1200-byte payloads, 2000 datagrams per sample, 3 rounds, and the exact
candidate SHA. The topology-matched M024 budgets remain green:

| Metric | Result | Budget |
| --- | ---: | ---: |
| Empty/bare sequential throughput | 0.9746 | >= 0.70 |
| Empty/bare sequential p95 | 1.1250 | <= 1.60 |
| Empty/bare windowed throughput | 0.8717 | >= 0.70 |
| Empty/direct M023 throughput | 0.5226 | >= 0.45 |
| Empty/direct M023 p95 | 1.9459 | <= 2.50 |

The associated medians were 41,694.35 direct, 22,358.34 bare-relay, and
21,791.46 empty-plan datagrams/s for sequential mode; windowed medians were
150,802.07, 95,048.75, and 82,854.33 datagrams/s respectively. The raw JSON,
not only this summary, is retained in the repository.

## Regression coverage

The frozen 14-case datagram golden corpus remains byte-for-byte unchanged.
Existing multi-client, multi-response, unsolicited-response, idle expiry,
kill/disable/re-enable, capacity/oversize, rollback, IPv4/IPv6, CLI, and
stream/core suites remain green. M025 adds deterministic tests for:

- many same-client waiters converging on one published association;
- setup failure waking all waiters and permitting one takeover;
- retained terminal state observed by a late waiter;
- global and per-proxy capacity held exactly once;
- waiter cancellation leaving the owner and capacity intact;
- setup-owner cancellation releasing the reservation and worker;
- update takeover rejecting a stale owner;
- disable/delete draining unpublished setup without double release.

## Invariants and unresolved findings

- One client address has at most one active association and one live setup
  reservation.
- A setup owner publishes only while it owns the current reservation.
- Global and per-proxy capacity is released exactly once per lease.
- No registry lock is held across UDP setup or worker joins.
- Waiters are event-driven and cannot miss a retained terminal transition.
- No task, socket, or capacity leak remains on tested failure, drain,
  cancellation, stale-owner, or service-lifecycle paths.
- Native control/configuration/CLI/Toxiproxy contracts are unchanged.
- No new production dependency was added; `unsafe_code = "forbid"` remains.
- No unresolved medium-or-higher correctness or maintainability finding
  remains for this narrow pass.

## Acceptance verdict and planning transition

M025 closes cleanly. `plans/registry.md` moves M025 from `ready` to `closed`,
with M025 as the last completed datagram maintenance handoff. Active work:
none. Blocked work: none. The UDP/datagram roadmap state is completed /
maintenance complete, not active. M024 and M023 historical closure records
remain unchanged, and M019 remains the historical v0.1.0 release authority.

No successor is activated. Optional `eggress-outbound`, eggreplay, eggprobe,
language-binding, richer-scheduler, and post-2.12 Toxiproxy work remains in
the normal `future` state and requires a separately registered numbered plan
(and an ADR for semantic datagram expansion).
