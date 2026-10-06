# M052 — Private Module Boundary Maintainability Corrective

Status: closed (header reconciled to the registry status by M061; as written at registration: `blocked`; evidence in `plans/closure/M052-private-module-boundary-maintainability-corrective-closure.md`)
Depends on: M051 closed
Role: low-semantic-risk decomposition of oversized control/translation modules
Activation baseline: exact M051 closure candidate

## Objective

Reduce maintenance risk in the largest control/translation source files by moving coherent private responsibilities into modules while preserving every public path, behavior, compatibility surface, and hot-path semantic contract.

The audit baseline identified several very large files. Size alone is not a defect, so M052 is deliberately limited to files where multiple independent responsibilities are already visible and can be separated without algorithmic change.

Primary candidates at registration time:

- eggchaos-server runtime/control.rs;
- eggchaos-server admin.rs;
- eggchaos-protocol stream.rs;
- eggchaos-toxiproxy lib.rs;
- eggchaos-cli main.rs where command parsing/presentation groups are cleanly separable.

eggchaos-core stream.rs and engine.rs are explicitly not automatic targets in this milestone. They are high-risk fault-engine hot paths; decompose them only under separate evidence-backed planning if a concrete maintenance defect warrants the churn.

## Scope

### In scope

- private module extraction;
- moving tests alongside the responsibility they prove when doing so improves locality;
- crate-root/module re-exports needed to keep existing public paths compiling;
- removal of duplicated imports/helpers exposed by M050/M051 consolidation;
- documentation/comments describing module ownership.

### Non-goals

- no algorithm rewrite;
- no performance optimization;
- no new route, command, fault, DTO, or compatibility profile;
- no public item rename/removal;
- no dependency change unless a now-unused direct dependency is proven removable;
- no core stream/engine decomposition by default;
- no generated-code framework.

## Decomposition rules

Every extraction must satisfy all of the following:

1. one responsibility can be named in a sentence;
2. moving it does not require changing its external semantics;
3. the original public path can be preserved with re-export when necessary;
4. tests can demonstrate equivalence;
5. the diff is mostly movement, not simultaneous redesign.

Prefer a few stable modules over many one-function files.

## Ordered work packages

### WP1 — Re-measure after M050/M051

Record post-consolidation file sizes and responsibility maps. Some audit hotspots may already shrink materially.

Only decompose files that remain multi-responsibility hotspots.

### WP2 — Decompose server control/admin presentation

Likely boundaries include:

- proxy/fault mutation operations;
- connection/history/reset operations;
- scenario lifecycle adapters;
- metrics rendering;
- HTTP route dispatch;
- request parsing/path decoding;
- response/error serialization.

Do not create a second ControlState authority.

### WP3 — Decompose protocol stream DTO families

Keep eggchaos_protocol::stream::* source compatibility while moving internal definitions into cohesive files such as proxy, stream_fault, scenario_v1, and datagram DTO groups.

The public stream module should re-export the same names.

### WP4 — Decompose Toxiproxy compatibility adapter

Candidate internal boundaries:

- compatibility DTOs;
- toxic translation;
- strict-v2.12 versus pinned-snapshot profile policy;
- route dispatch;
- response/error compatibility helpers.

Strict v2.12 default behavior and pinned packet_loss profile must remain byte/behavior compatible.

### WP5 — CLI decomposition if still warranted

If main.rs remains a clear hotspot, split command definitions, HTTP client invocation, and human/JSON presentation privately while retaining the exact binary command syntax and output contract.

Skip this package if M051 or incidental cleanup already makes the benefit marginal.

## Required verification

At minimum:

    cargo test -p eggchaos-server --all-features
    cargo test -p eggchaos-protocol --all-features
    cargo test -p eggchaos-toxiproxy --all-features
    cargo test -p eggchaos-cli --all-features
    ./scripts/check_openapi.sh
    ./scripts/qualify_toxiproxy_v2_12.sh
    ./scripts/qualify_toxiproxy_post_v2_12.sh
    cargo test --workspace --all-features
    ./scripts/check.sh

The mandatory oracle mode belongs to M054 exact-head qualification; M052 local implementation evidence may use the repo's normal developer-mode oracle behavior but must never call an incomplete oracle a pass.

## Acceptance criteria

M052 may close only when:

1. each moved responsibility has one obvious private module authority;
2. existing public Rust import paths compile unchanged;
3. all native routes/CLI commands/Toxiproxy routes are unchanged;
4. no core hot-path semantics were opportunistically rewritten;
5. tests show no behavior drift;
6. any dependency removal is independently proven unused;
7. architecture docs reflect the new private module map;
8. exact-candidate closure evidence is recorded.

## Rejection / stop conditions

Do not continue a decomposition that requires simultaneous semantic redesign. Leave a large file intact and record the no-op disposition instead.

Any proposed eggchaos-core stream/engine split must be removed from M052 unless it can be demonstrated as pure movement with no hot-path behavior or performance risk.

## Closure evidence

Create plans/closure/M052-private-module-boundary-maintainability-corrective-closure.md with before/after responsibility and file-size table, public-path regression evidence, tests, and any intentionally skipped hotspot.

## Successor activation

Closing M052 activates M053.
