# M041 — Stream-Loss Metrics and Closure Hygiene Corrective

Status: ready  
Depends on: M040 historical closure record + ADR 007  
Role: narrow post-M040 operational correctness and closure hygiene corrective  
Baseline: `b69dec32a23ac426d57ba3c0e0aeecb80649dbec`

## Objective

Correct the small but release-relevant defects discovered immediately after
M040 closure without reopening ADR 007 stream-loss semantics or repeating the
broader M040 compatibility work.

M041 must:

1. restore valid Prometheus exposition for the M040 stream-loss counters;
2. add a regression that fails on duplicate stream-loss series and literal
   `\\n` corruption in `GET /metrics`;
3. restore a stable, documented stdout contract for
   `scripts/fetch_toxiproxy_post_v2_12.sh` while retaining structured
   oracle metadata;
4. reconcile the remaining M040 planning/documentation drift; and
5. requalify one exact corrective candidate before creating the final M041
   closure record.

M040 remains historical evidence for the substantial ADR 007 corrective
implementation at
`48fe0dd8dfc1f995c53a3b1661fe704dbdc5bce0`. M041 does not invalidate
that work. Its closure supersedes M040 only as the latest repository-level
authority for the corrected metrics/tooling/planning state.

## Baseline and observed defects

Registration baseline:
`b69dec32a23ac426d57ba3c0e0aeecb80649dbec`.

### 1. Stream-loss Prometheus series are emitted twice

M040 added three bounded stream-loss metrics:

- `eggchaos_stream_loss_chunks_evaluated_total`;
- `eggchaos_stream_loss_chunks_dropped_total`;
- `eggchaos_stream_loss_bytes_discarded_total`.

In `crates/eggchaos-server/src/runtime/control.rs`, the per-proxy/direction
render loop currently emits the same three series twice.

The first loop is correct and ends each sample with a real newline:

```rust
"{metric}{{proxy=\"{name}\",direction=\"{direction}\"}} {value}\n"
```

A second immediately-adjacent loop repeats the same labelsets with:

```rust
"{metric}{{proxy=\"{name}\",direction=\"{direction}\"}} {value}\\n"
```

That second form appends a literal backslash plus `n`, not a line
terminator. The resulting exposition can contain:

- duplicate metric labelsets; and
- malformed concatenated text after the literal `\\n`.

This is an operator-facing correctness defect. A green Rust test suite is not
sufficient evidence while a real Prometheus scrape can reject the output.

### 2. Existing metrics tests assert presence, not valid exposition

The M040 server tests prove that valid stream-loss sample text exists, but they
do not assert:

- each metric/labelset appears exactly once;
- the rendered payload contains no literal `\\n` escape sequences;
- every non-comment sample occupies one valid exposition line;
- stream-loss sample labelsets remain unique.

M041 must add a regression at the rendered `GET /metrics` text boundary so
this class of mistake cannot pass through duplicated formatting code again.

### 3. Post-v2.12 oracle fetch stdout contract drifted

Before M040, `scripts/fetch_toxiproxy_post_v2_12.sh` printed only the
executable path, matching the ordinary usage pattern:

```sh
TOXIPROXY_POST_V2_12_SERVER="$(./scripts/fetch_toxiproxy_post_v2_12.sh)"
```

M040 changed the default output to a JSON metadata record and added
`--path-only` for callers that need the executable path. The internal
qualification script uses `--path-only` correctly, but user/operator
documentation still shows the historical command substitution with no flag.

This leaves the script interface and documented usage inconsistent.

M041 must choose and freeze one compatibility-safe CLI contract. Preferred:

- default stdout remains the executable path, preserving shell command
  substitution behavior;
- `--json` emits the structured toolchain/source/oracle metadata added by
  M040;
- `--path-only` may remain as an explicit alias for compatibility with the
  M040 qualifier;
- diagnostics remain on stderr;
- the qualification script consumes an explicit mode rather than depending on
  an ambiguous default.

If implementation instead retains JSON-by-default, every documented and
internal path consumer must be migrated and a compatibility rationale must be
recorded. Restoring path-by-default is preferred because it preserves the
pre-M040 public shell behavior.

### 4. Remaining planning/documentation drift

At registration:

- `plans/040-post-v2-12-stream-loss-corrective-requalification.md` still
  says `Status: active` even though registry/closure state says M040 is
  closed;
- `plans/README.md` still lists numbered plans only through
  `039-*.md`;
- `AGENTS.md` verification guidance still refers to the strict Toxiproxy
  differential as `47/47`, while the current strict corpus is `50/50`;
- current-state docs identify M040 as the final ADR 007 authority even though
  this post-closure defect now requires M041.

Unlike the historical M036–M039 executable handoff headers, M040 is the
immediately preceding corrective plan and its stale `active` status is an
unambiguous current-state contradiction. M041 may update that status to
`closed` as a planning reconciliation, while preserving the M040 closure
record and exact candidate.

## Scope

### In scope

- Remove duplicate/malformed stream-loss sample rendering from
  `ControlState::metrics_text()`.
- Prefer one helper/loop for rendering the three stream-loss metrics per
  proxy/direction rather than duplicated format blocks.
- Preserve the existing bounded proxy/direction cardinality and overflow
  behavior.
- Preserve the existing three metric names and HELP/TYPE declarations.
- Add rendered-exposition regressions for uniqueness and line validity.
- Add an explicit regression that no literal `\\n` appears in metrics
  output.
- Validate overflow stream-loss metrics under the same exposition checks.
- Restore/freeze the post-v2.12 oracle fetcher stdout modes.
- Update `docs/toxiproxy.md` and any script comments/examples to the actual
  fetcher contract.
- Re-run mandatory strict-v2.12 and post-v2.12 oracle qualification after the
  fetcher change.
- Reconcile registry/README/roadmap/AGENTS and M040 status wording.
- Create exact-candidate M041 closure evidence after local and hosted
  qualification.

### Non-goals

- No change to core `StreamLoss` behavior.
- No change to the fixed 32 KiB logical grain.
- No change to `loss_rate`, `correlation`, or deterministic RNG.
- No change to Toxiproxy profile semantics or the pinned snapshot.
- No new compatibility profile or toxic.
- No eighth activation slot.
- No metric rename.
- No metric cardinality expansion.
- No new OpenAPI/native control operation.
- No SDK/binding feature.
- No UDP/datagram semantic change.
- No new performance budget.
- No release/tag/publication action.
- No rewrite of M036–M040 historical evidence beyond narrowly required
  current-state status/correction references.

## Affected surfaces

Expected implementation surfaces:

- `crates/eggchaos-server/src/runtime/control.rs`;
- `crates/eggchaos-server/src/runtime/tests.rs`;
- optionally a narrow metrics rendering helper if it reduces duplication;
- `scripts/fetch_toxiproxy_post_v2_12.sh`;
- `scripts/qualify_toxiproxy_post_v2_12.sh`;
- script tests if the repository has an appropriate shell-test location;
- `docs/toxiproxy.md`;
- `architecture/verification-qualification.md` only if script usage is
  documented there;
- `plans/040-post-v2-12-stream-loss-corrective-requalification.md` status
  only;
- `plans/registry.md`;
- `plans/README.md`;
- `plans/roadmap.md`;
- `AGENTS.md`;
- M040 closure only for an additive “superseded by M041 for metrics/tooling
  closure hygiene” note if needed;
- new M041 closure record.

## Required Prometheus correction

### Single emission authority

For each retained proxy and each direction, emit exactly one sample for each:

```text
eggchaos_stream_loss_chunks_evaluated_total
eggchaos_stream_loss_chunks_dropped_total
eggchaos_stream_loss_bytes_discarded_total
```

The same rule applies to the `_overflow` proxy.

Do not solve this by filtering duplicate strings after rendering. Remove the
duplicate formatting authority.

### Exposition invariants

The rendered metrics text must satisfy all of:

- no literal `\\n` byte sequence created as a substitute for line
  termination;
- every sample ends with a real newline;
- HELP/TYPE lines remain valid comments;
- each metric name + complete labelset occurs at most once;
- each stream-loss proxy/direction emits all three metric families;
- zero values remain present where the existing metrics contract intentionally
  exposes them;
- overflow metrics remain bounded and syntactically valid;
- existing non-stream-loss metric output remains byte/semantic compatible
  except for ordering changes that are explicitly harmless.

### Required regression strategy

At minimum, add a test over `ControlState::metrics_text()` that:

1. creates at least one ordinary proxy with stream-loss evidence;
2. obtains metrics text after connection finalization;
3. asserts no `\\n` literal occurs;
4. parses non-comment/non-empty lines into metric identifier + labelset +
   scalar form, or uses a real Prometheus text parser if already available
   without disproportionate dependency cost;
5. asserts stream-loss labelsets are unique;
6. asserts exactly one evaluated/dropped/discarded sample for each tested
   proxy/direction;
7. exercises the bounded overflow path and applies the same uniqueness rule;
8. retains the existing label-key allowlist test.

A raw `contains()` assertion alone is insufficient.

## Required oracle-fetch CLI contract

Preferred command behavior:

```sh
# Stable command-substitution behavior:
./scripts/fetch_toxiproxy_post_v2_12.sh
# -> /path/to/toxiproxy-server

# Explicit structured record:
./scripts/fetch_toxiproxy_post_v2_12.sh --json
# -> {"requested_toolchain": ...}

# Explicit path mode retained for M040 callers:
./scripts/fetch_toxiproxy_post_v2_12.sh --path-only
# -> /path/to/toxiproxy-server
```

If an optional destination path is supported, mode parsing must remain
unambiguous, e.g.:

```sh
./scripts/fetch_toxiproxy_post_v2_12.sh [--json|--path-only] [DEST]
```

Required script tests/qualification:

- default mode prints exactly one usable executable path;
- `--path-only` prints exactly one usable executable path;
- `--json` parses as JSON and contains requested/resolved toolchain, source
  commit/checksum, oracle path, and oracle version;
- default/path output contains no JSON prefix/suffix;
- JSON mode contains no extra stdout diagnostics;
- requested Go toolchain mismatch still fails;
- mandatory post-v2.12 qualifier remains green using an explicit fetch mode.

## Planning reconciliation

Registration state:

```text
ADR 007
  -> M036–M039 historical implementation
  -> M040 closed historical corrective candidate 48fe0dd
  -> M041 ready narrow closure corrective
```

M041 is the sole ready handoff.

At M041 closure:

- mark M041 closed in `plans/registry.md`;
- make M041 the latest ADR 007 repository-level closure authority;
- update `plans/README.md` numbered-plan range through M041;
- ensure M040 plan status is reconciled to `closed`;
- update `AGENTS.md` strict differential guidance to the current 50/50
  corpus;
- remove any current-state prose claiming no successor after M040 without
  acknowledging M041;
- preserve M040's exact candidate and hosted run as historical evidence;
- record a real M041 exact candidate SHA and hosted run ID.

## Ordered work packages

### WP1 — Freeze metrics exposition regression

Add a failing test that detects:

- duplicate stream-loss metric labelsets;
- literal `\\n` corruption;
- missing real line termination.

Do this before removing the duplicate render block.

### WP2 — Correct stream-loss metrics rendering

Remove the duplicate/malformed block and centralize the three stream-loss
sample writes. Verify ordinary and overflow paths.

### WP3 — Freeze oracle-fetch mode tests

Add focused shell/script coverage for default, `--path-only`, and
`--json` stdout contracts before changing the default.

### WP4 — Restore/freeze fetcher CLI behavior

Implement the selected path-by-default contract, keep explicit JSON metadata,
and update the post-v2.12 qualifier to request the mode it actually consumes.

### WP5 — Documentation/planning reconciliation

Update operator docs, M040 status, plan range, strict differential count,
registry/roadmap/AGENTS handoff state, and any stale fetch examples.

### WP6 — Local exact-candidate qualification

Run the focused metrics/script tests and full workspace gate on one committed
candidate.

### WP7 — Compatibility and release regression

Run both mandatory Toxiproxy oracle gates plus release/package smoke. The
strict and post-v2.12 semantics must remain unchanged.

### WP8 — Hosted exact-head qualification

Push the candidate and require the ordinary hosted CI matrix to conclude
success on that exact SHA.

### WP9 — M041 closure

Create closure evidence only after the candidate and hosted run are final.

## Required tests

### Metrics

- rendered metrics contain no literal `\\n`;
- stream-loss metric/labelset tuples are unique;
- ordinary proxy/direction outputs exactly one sample per stream-loss family;
- overflow proxy/direction outputs exactly one sample per family;
- zero-loss/full-loss counter values remain correct;
- multiple stream-loss faults still do not double-count discarded bytes;
- legacy seven-slot activation metrics remain unchanged;
- metrics label cardinality remains bounded.

### Fetcher/qualifier

- default fetch output is one executable path;
- `--path-only` output is one executable path;
- `--json` output parses and contains all M040 oracle identity fields;
- explicit destination handling remains valid;
- exact `go1.23.0` default/override validation remains enforced;
- mandatory post-v2.12 qualification remains green;
- missing/mismatched mandatory oracle still fails rather than reporting pass.

### Existing regressions

At minimum:

- `./scripts/check.sh`;
- strict pinned Toxiproxy v2.12 qualification;
- pinned post-v2.12 qualification;
- OpenAPI drift check;
- release smoke;
- release artifact smoke.

Because M041 does not change core loss semantics, SDK implementations,
native-Python binding semantics, datagram runtime, or benchmark code, full
10,000-run fuzz and performance reruns are not required unless the
implementation touches those surfaces or another test reveals a related
change.

Hosted CI must still run the repository's ordinary Rust/security and
language/native-Python matrix on the exact corrective SHA so closure does not
repeat the M039 hosted-evidence mistake.

## Verification

Minimum local exact-candidate commands:

```sh
./scripts/check.sh
./scripts/check_openapi.sh

TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" \
  EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 \
  ./scripts/qualify_toxiproxy_v2_12.sh

EGGCHAOS_REQUIRE_POST_V2_12_ORACLE=1 \
  ./scripts/qualify_toxiproxy_post_v2_12.sh

./scripts/release-smoke.sh
./scripts/release-artifact-smoke.sh
```

Also run any new focused metrics/script contract test directly and record it in
closure.

Hosted gate:

- push the exact candidate;
- require Linux/macOS/Windows Rust checks green;
- require hosted `cargo audit` and `cargo deny` green;
- require existing language-client matrix green;
- require hosted native-Python jobs green;
- record run ID/URL, all required job conclusions, and exact SHA.

## Acceptance criteria

M041 closes only when:

- no duplicate stream-loss metric series are emitted for the same complete
  labelset;
- metrics output contains no literal `\\n` corruption;
- rendered stream-loss metrics pass the new exposition regression for ordinary
  and overflow proxies;
- all three stream-loss metrics retain their current names, values, labels,
  and bounded cardinality;
- legacy activation-array/metric semantics remain unchanged;
- default post-v2.12 fetcher stdout once again matches the documented
  command-substitution contract, or an explicitly documented alternative is
  proven compatible;
- `--json` retains the exact Go/source/oracle identity record introduced by
  M040;
- mandatory strict and post-v2.12 oracle gates both pass;
- M040 plan/current-state status contradiction is resolved;
- README plan range includes M040/M041;
- AGENTS/verification text uses the current strict 50/50 count;
- one exact M041 candidate passes the local gate;
- the same candidate passes the required hosted matrix;
- M041 closure records the exact candidate and hosted run;
- no unresolved medium-or-higher correctness/security finding remains from
  this narrow scope.

Create:

`plans/closure/M041-stream-loss-metrics-and-closure-hygiene-corrective-closure.md`

## Stop/rejection conditions

Do not close if:

- a valid stream-loss sample is followed by a duplicate labelset;
- metrics output contains literal `\\n` separators;
- tests merely check substring presence without validating uniqueness/syntax;
- the fix filters malformed text after rendering instead of removing duplicate
  emission authority;
- default fetcher output remains incompatible with documented shell usage;
- structured oracle identity is lost while restoring path output;
- either mandatory Toxiproxy oracle runs in incomplete/developer mode;
- required hosted jobs are inherited from M040 rather than rerun on the M041
  candidate;
- planning docs still disagree about whether M040/M041 is active/closed;
- the corrective expands into new fault semantics or unrelated refactoring.

## Follow-on activation

M041 activates no automatic successor.

After clean M041 closure, ADR 007's stream-loss/post-v2.12 compatibility line
is considered correctively closed again. Future work requires a separately
registered milestone, such as a tagged Toxiproxy release reconciliation or a
new independently justified fault/operational feature.
