# M020 Closure — Deterministic Datagram Fault Engine

Verdict: **clean**

## Candidate and scope

- Exact implementation candidate: `56c892570c2571beb3f80b39edf55bb5ea26ad3a`
- Candidate branch: `main`
- The candidate adds the protocol-neutral whole-datagram plan, deterministic
  RNG domain, bounded scheduler, generation policy, evidence, tests, and two
  fuzz targets. It does not add socket or control-plane behavior.
- Existing stream RNG vectors and engine behavior are covered by the same
  workspace test run and remain unchanged.

## Verification evidence

All commands below ran against the exact candidate on the local macOS
development host. The repository check ran with `RUST_TEST_THREADS=1` after a
parallel run exhausted the host's open-file limit in two pre-existing TCP
runtime tests; the serialized exact-candidate rerun passed.

- `RUST_TEST_THREADS=1 ./scripts/check.sh` — passed: format, workspace Clippy,
  all workspace tests (50 core tests, including datagram coverage), and docs.
- `EGGCHAOS_FUZZ_RUNS=10000 ./scripts/qualify_fuzz.sh` — passed all eight
  targets, 10,000 runs each, including `datagram_plan_json` and
  `datagram_transitions`; no crashes. Run used cargo-fuzz 0.13.2 and the
  repository's sanitizer-none workflow. Generated corpus entries were
  removed; the tracked datagram seed remains.
- `cargo audit --deny warnings` — passed with no advisories.
- `cargo deny check advisories licenses bans sources` — passed. It emitted
  the existing duplicate-lock-entry warnings for `getrandom` and `winnow`;
  advisories, bans, licenses, and sources all report `ok`.
- `cargo tree --locked` — passed. No production `eggress-udp` dependency or
  other dependency was added for M020.

Coverage includes the independent datagram RNG golden vector; ordered
duplicate/loss composition; unique cascading copy identities; delay ordering;
payload corruption with length preservation; whole-datagram rate scheduling;
queue count/byte overflow; duplicate amplification bounds; and queued
old-generation candidates coexisting with newly admitted generations.

## Limitations and findings

- M020 is protocol-neutral and has no socket or external oracle requirement.
  Cross-platform live UDP behavior and performance remain M021/M023 evidence.
- The security dependency checks passed; no unresolved correctness or
  security finding remains for this milestone.

## Acceptance verdict and follow-on

M020 acceptance criteria are met on the candidate above. Its six datagram fault
types, per-candidate decisions, separate RNG domain, count/byte bounds,
drop-newest overflow distinction, admission-time policy snapshot, and
stream-independent API are implemented and verified.

M021 is activated as `ready`. Its Eggress reuse census found that the
published `eggress-udp` 1.0.8 API is organized around routing/SOCKS-oriented
flows and depends on `eggress-routing`; it does not provide the narrow
fixed-target connected-association seam M021 needs. Per ADR 003 and the M021
reuse gate, M021 can implement the small socket-ownership layer locally with
Tokio rather than adding that production dependency. M022 and M023 remain
blocked behind M021 and M022 respectively.

The closure record and registry transition are committed after the candidate;
they do not change the candidate SHA used for qualification.
