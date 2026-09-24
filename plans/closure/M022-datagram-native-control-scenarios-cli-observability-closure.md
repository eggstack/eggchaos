# M022 closure — Datagram native control, scenarios, CLI, and observability

## Verdict

Closed on exact implementation candidate `8c4e3fb04cfbff1e639d9c5e07b33281c0fc1ea7`.

The candidate adds explicit v1 datagram DTOs and native resources backed by the shared M021 runtime, schema-v1 TOML datagram definitions and limits, JSON-first CLI commands, directional datagram scenario actions with seeded generation-guarded publication, association inspection/kill, bounded datagram metrics, and documented global reset behavior. Toxiproxy v2.12 remains stream/TCP-only. Association evidence DTO fields are explicit and contain no payload data.

## Candidate evidence

All implementation evidence below was run against the candidate above before this closure-only commit:

- `RUST_TEST_THREADS=4 ./scripts/check.sh` — passed formatting, workspace Clippy, all workspace tests, and workspace docs. Four test threads were required on this macOS host, whose process descriptor limit is 256; higher workspace test concurrency produced transient `EMFILE` failures in existing TCP socket tests. At four threads all 63 server tests passed.
- `RUST_TEST_THREADS=4 ./scripts/qualify_eggfetch.sh` — passed Eggfetch regressions and the server suite (63/63).
- `TOXIPROXY_SERVER=<verified-v2.12.0-binary> EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 ./scripts/qualify_toxiproxy_v2_12.sh` — passed against the checksum-verified official v2.12.0 oracle, including differential qualification.
- `cargo audit --deny warnings` — passed.
- `cargo deny check advisories licenses bans sources` — passed. Existing duplicate-version advisory warnings were informational; all configured checks reported `ok`.
- `cargo tree --locked -p eggchaos-server` — no dependency change beyond the already-qualified runtime dependencies; no Eggress graph expansion.

Focused evidence includes native DTO/config fixtures for all six fault kinds and invalid bounds, end-to-end CLI JSON operations, real HTTP association list/get/kill, listener-backed proxy/fault CRUD and reset, empty fault-patch rejection, scenario seed/generation publication, metrics label bounds, bind-failure rollback, multi-client isolation, multi/unsolicited replies, lifecycle/expiry, queue/capacity limits, oversize classification, and IPv6 loopback where available.

## Contract decisions

- Adding the optional `datagram_proxies` collection and `runtime.datagram` bounds is backward-compatible within schema version 1; existing stream fields retain their prior meaning.
- `/v1/reset` clears stream and datagram plans, terminates active associations/work, and re-enables stored listeners. The response reports datagram enable failures separately.
- Datagram association and metrics evidence remain bounded and omit payloads. Prometheus labels exclude association, peer, run, and fault IDs.
- Datagrams are user-space message impairment. The implementation and documentation make no lower-layer packet-loss or qdisc-equivalence claim.

## Follow-on activation

M023 depends only on M022. M022 is now formally closed, so M023 is ready. Its exact-candidate cross-platform UDP qualification and measured performance budget remain required before this tranche can be declared complete.
