# M065 — Residual Runtime and Qualification-Tooling Hardening

Status: active
Depends on: M062 closed; 2026-11-17 read-only security audit at `62682d9`
Role: low-risk hardening of qualification tooling, metrics exposition, and
documented runtime resource limits; no protocol or default-behavior change
Registration baseline: `60c28a48c2375d20c0cf793372423751967f2b9a` (`main`)
Audit evidence: `62682d955c802f8caedc43ec66f7100ca1af9dbb`

## Objective

Close the residual low-severity findings from the 2026-11-17 security audit
without expanding the runtime's ownership boundaries or changing any default:

1. remove predictable `/tmp` paths and unrestricted redirects from the
   external-oracle fetch and qualification scripts, while preserving their
   documented stdout and checksum contracts;
2. make Prometheus label serialization escape label values even though
   proxy-name validation currently prevents hostile names from reaching it;
3. document the per-connection memory amplification envelope and the
   `max_connections` mitigation for proxy listeners that are intentionally
   exposed beyond loopback;
4. evaluate an optional stream idle timeout through the existing
   `eggress-relay` boundary. If the pinned relay cannot express it without
   forking relay semantics, record the residual risk and an upstream request
   instead of copying the relay loop;
5. document the `--admin-token` process-argument exposure and the environment
   alternative;
6. prove the script and metrics changes with focused tests and the full gate.

M065 is deliberately additive. It changes no default timeout, no default
connection limit, no wire format, and no release workflow.

## Research baseline

At the registration baseline, the audit established:

- `scripts/fetch_toxiproxy_v2_12.sh` downloads to a caller/default destination
  under `${TMPDIR:-/tmp}`, runs `mkdir -p`, downloads with
  `curl --location`, then verifies SHA-256 (lines 15-28);
- `scripts/fetch_toxiproxy_post_v2_12.sh` uses a fixed
  `${TMPDIR:-/tmp}/eggchaos-toxiproxy-post-v2-12-40f7fd31` destination,
  `mkdir -p`, `curl --location`, a committed source-archive SHA-256, and a
  `mkdir`/`tar` extraction path (lines 65-108);
- `scripts/qualify_toxiproxy_post_v2_12.sh` writes a fixed
  `/tmp/post-v212.log` (lines 56-58);
- both fetchers verify content checksums, so the residual risk is local
  file clobbering/symlink behavior on shared hosts and a cleartext redirect
  downgrade, not substituted upstream content;
- `metrics_text` interpolates stream and datagram proxy names, directions,
  fault types, and kinds directly into label strings
  (`crates/eggchaos-server/src/runtime/control.rs:132,149,159,182,225,246,
  260,270,316,322`);
- proxy names are validated to `[A-Za-z0-9._-]+` and 1..=128 bytes
  (`crates/eggchaos-server/src/runtime/mod.rs:135-148`,
  `runtime/datagram/model.rs:133-140`), so the current validation prevents a
  hostile name from reaching the unescaped writer; escaping is
  defense-in-depth, not a live injection fix;
- a latency fault may configure `max_buffer_bytes` up to 67,108,864 bytes
  (`docs/configuration.md:56`), and the default global connection limit is
  1024; the worst-case buffered envelope is therefore an operator-visible
  multiplication that is not currently documented;
- stream connections have no idle timeout; datagram associations do have an
  `association_idle_timeout_ms` bound;
- the proxy data-plane copy and half-close behavior are owned by the pinned
  `eggress-relay` crate; the workspace forbids forking its semantics, and the
  pinned `1.0.12` source exposes no idle-timeout knob (only shutdown drain
  timeouts);
- the CLI exposes `--admin-token` with an `EGGCHAOS_ADMIN_TOKEN` fallback
  (`crates/eggchaos-cli/src/main.rs:26-28`), and the docs already prefer the
  environment variable;
- EggServe bounds admin header/body/total/handler time, so this plan does
  not add duplicate HTTP timeouts.

## Scope

### In scope

1. Harden `scripts/fetch_toxiproxy_v2_12.sh`:
   - create a private temporary directory with a portable `mktemp -d`
     template and mode `0700`;
   - download into the temporary directory, verify the committed SHA-256,
     and only then move/install the executable at the requested destination;
   - use `--proto '=https' --proto-redir '=https'` with `curl --fail
     --location`;
   - install a cleanup trap that removes only the temporary directory and
     preserves the existing stdout contract (one executable path);
   - keep the version assertion and the checksum-mismatch failure behavior.
2. Harden `scripts/fetch_toxiproxy_post_v2_12.sh` equivalently:
   - private temporary archive/extraction/build directory; no fixed
     destination until verification succeeds;
   - preserve the committed source-archive checksum and the Go toolchain
     identity assertions;
   - preserve `--path-only`/`--json`/default stdout contracts and the
     one-record JSON shape;
   - clean temporary state on exit/signal and never leave a partial
     executable at the final destination.
3. Harden `scripts/qualify_toxiproxy_post_v2_12.sh`:
   - use `mktemp` for the differential log and a cleanup trap;
   - keep the existing failure-tail behavior and summary grep.
4. Add a Prometheus label-escaping helper in the server metrics writer:
   - escape backslash, double quote, and newline per the Prometheus text
     exposition format;
   - apply it to every label value inserted by `metrics_text`, including
     proxy names, directions, fault types, outcome/result/flow constants, and
     datagram kinds;
   - leave the existing name validation and cardinality bounds unchanged.
5. Add unit tests for the escaping helper and a metrics-output assertion
   using a synthetic label value that would otherwise break the exposition
   (the test may call the helper directly because the public API correctly
   rejects such names).
6. Document the resource envelope in `docs/configuration.md`:
   - `max_buffer_bytes` per latency fault multiplied by the global and
     per-proxy connection bounds;
   - the recommendation to set per-proxy `max_connections` and to keep
     listeners on loopback unless exposure is intentional;
   - the fact that stream connections have no idle timeout and that
     `max_connections` is the current bound.
7. Evaluate stream idle-timeout support at the `eggress-relay` boundary:
   - if the pinned relay exposes a supported no-progress timeout, add an
     optional `[runtime] stream_idle_timeout_ms` with `0`/absent meaning
     disabled, bounded to a documented range, and implement it through the
     relay API;
   - if it does not, do not fork or reimplement the relay copy loop; record
     the residual risk, document the mitigation, and open an upstream
     request against Eggress. This outcome is an accepted documented gap,
     not a code change.
8. Document the CLI token-argument exposure in `README.md` or
   `docs/control-plane.md`: `--admin-token` is visible in a local process
   listing; prefer `EGGCHAOS_ADMIN_TOKEN` or a protected config file, and do
   not paste tokens into shell history.
9. Run the focused script tests, the server tests, the full local gate, and
   exact-head hosted CI.

### Non-goals

- no change to any default timeout, connection limit, buffer bound, route,
  fault semantic, RNG, scenario, evidence, or payload handling;
- no fork or copy of `eggress-relay`'s bidirectional copy/half-close logic;
- no new runtime dependency;
- no change to the fetch scripts' checksum values, toolchain identity
  assertions, or stdout contracts;
- no change to release workflows or artifact names;
- no TLS or admin-listener work (M063 owns that);
- no repository-settings work (M064 owns that);
- no metrics label cardinality or naming change;
- no performance-threshold retuning.

## Affected surfaces

Expected implementation surfaces:

- `scripts/fetch_toxiproxy_v2_12.sh`;
- `scripts/fetch_toxiproxy_post_v2_12.sh`;
- `scripts/qualify_toxiproxy_post_v2_12.sh`;
- `scripts/tests/test_fetch_toxiproxy_post_v2_12_contract.sh` and any
  existing cleanup-trap test that covers these scripts;
- `crates/eggchaos-server/src/runtime/control.rs` (escaping helper and
  call sites);
- `crates/eggchaos-server/src/config.rs` only if the optional idle timeout is
  supported by the pinned relay;
- `docs/configuration.md`, `docs/control-plane.md`, `README.md`;
- `plans/registry.md`, this plan, and the generated planning-state
  projections;
- a closure record under `plans/closure/` at closure.

## Ordered work packages

### WP1 — Freeze the script contracts

1. Record the current stdout shapes:
   - v2.12 fetcher: one executable path;
   - post-v2.12 fetcher: one path by default/`--path-only`, one JSON record
     with `--json`;
   - failure paths: diagnostics on stderr, nonzero exit, no path on stdout.
2. Run the existing contract tests once to confirm the baseline.
3. Add failing assertions for the new temporary-path and redirect behavior
   without changing the expected stdout.

### WP2 — Fetch-script hardening

1. Replace fixed paths with `mktemp -d` directories created under
   `${TMPDIR:-/tmp}` with a template that works on macOS and Linux.
2. Download to the temporary directory; verify the checksum there; only then
   install the executable at the requested destination with the expected
   mode.
3. Add `--proto '=https' --proto-redir '=https'` to every `curl` invocation.
4. Add `trap` cleanup for `EXIT HUP INT TERM`; ensure a checksum mismatch
   removes temporary artifacts and leaves no executable at the destination.
5. Keep the existing version/toolchain assertions and all stdout contracts.
6. Update the qualification wrapper to use a temporary log and cleanup trap.

### WP3 — Metrics label escaping

1. Add a small pure helper that escapes `\`, `"`, and newline and returns a
   borrowed value when no escaping is needed.
2. Apply it at every label insertion in `metrics_text`; do not escape metric
   names, HELP text, or numeric samples.
3. Add unit tests for the helper and an assertion that a synthetic
   quote/newline value produces one valid exposition line per sample.
4. Confirm existing metric names, labels, and values are byte-identical for
   normal names.

### WP4 — Idle-timeout evaluation

1. Inspect the pinned `eggress-relay` public API for a no-progress or idle
   timeout configuration.
2. If present, add the optional `[runtime] stream_idle_timeout_ms` field,
   validate the range, thread it through the existing relay boundary, and
   test expiry with paused time, half-close, and byte conservation.
3. If absent, do not modify the relay loop. Record the exact API gap, add the
   documentation mitigation, and create a bounded upstream request against
   Eggress. Treat this as the accepted outcome for this milestone.
4. Either outcome must leave the default behavior unchanged.

### WP5 — Resource and token documentation

1. Add the memory-envelope section to `docs/configuration.md` with the
   worst-case multiplication and the per-proxy `max_connections` guidance.
2. Document the no-idle-timeout residual and the loopback/exposure guidance.
3. Document the `--admin-token` process-listing exposure and the environment
   alternative.
4. Keep the existing TLS-terminator warning and the public-admin token rules
   unchanged except for the M063 cross-reference.

### WP6 — Qualification and closure

1. Run the focused script tests and `sh -n` on every changed script.
2. Run `cargo test -p eggchaos-server --all-features` and the full local
   gate.
3. Verify the fetchers still produce their documented stdout on the pinned
   hosts and that a checksum mismatch fails closed.
4. Obtain exact-head hosted CI.
5. Write the closure record with the exact SHA, run ID, script contract
   transcript, metrics test evidence, and the idle-timeout disposition
   (implemented or documented upstream gap).

## Invariants and failure semantics

- The fetchers print no executable path to stdout on failure, and a checksum
  mismatch leaves no executable at the requested destination.
- Temporary directories are private, unique, and removed on normal exit and
  on `HUP`/`INT`/`TERM`.
- HTTPS is required for the initial request and every redirect.
- Existing checksum values and toolchain identity assertions are unchanged.
- The metrics writer emits syntactically valid Prometheus text for any label
  value; normal labels remain byte-identical.
- No default runtime timeout or connection limit changes. If the optional
  idle timeout is added, `0`/absent preserves today's behavior exactly.
- If the idle timeout is implemented, it must close gracefully without
  discarding already-forwarded bytes, must respect half-close, and must not
  run a second copy loop.
- No new dependency and no `unsafe`.
- No script writes a secret or token to a log or stdout.

## Test commands

Script contracts and shell syntax:

```sh
sh -n scripts/fetch_toxiproxy_v2_12.sh
sh -n scripts/fetch_toxiproxy_post_v2_12.sh
sh -n scripts/qualify_toxiproxy_post_v2_12.sh
sh scripts/tests/test_fetch_toxiproxy_post_v2_12_contract.sh
sh scripts/tests/test_cleanup_traps.sh
```

Server and metrics:

```sh
cargo test -p eggchaos-server --all-features
cargo test -p eggchaos-core --all-features
```

Full local gate:

```sh
./scripts/check.sh
```

Oracle smoke (only if the host supports it):

```sh
TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" \
EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 \
./scripts/qualify_toxiproxy_v2_12.sh
```

Planning and sign-off guards:

```sh
python3 scripts/check_planning_state.py --check
sh scripts/tests/test_planning_state.sh
python3 scripts/check_release_state_docs.py --check
python3 scripts/check_action_pins.py --check
git diff --check
```

## Acceptance criteria

1. Every fetcher downloads through a private temporary directory, verifies
   its checksum before installing, uses HTTPS-only redirects, and cleans up
   on exit and signals.
2. The stdout contracts and failure behavior are unchanged; the existing
   contract tests pass with added temporary-path assertions.
3. The qualification wrapper no longer uses a fixed `/tmp` log.
4. The metrics writer escapes label values; unit tests cover backslash,
   quote, newline, and a synthetic mixed value; normal output is unchanged.
5. `docs/configuration.md` documents the memory envelope, the no-idle-timeout
   residual, and the `max_connections` mitigation.
6. The idle-timeout disposition is explicit: implemented through the relay
   API with paused-time tests, or recorded as an upstream gap with no relay
   fork and a documented mitigation.
7. `./scripts/check.sh` and exact-head hosted CI are green; no default
   behavior or release-workflow change is present in the diff.

## Stop conditions

- Stop if a portable `mktemp -d` template cannot be found for the supported
  hosts; use the narrowest documented fallback rather than a fixed path.
- Stop if installing the verified executable at the caller's destination
  requires deleting an existing user file without confirmation; preserve the
  existing overwrite contract only where it is already documented.
- Stop if metrics escaping changes normal output or a snapshot test; fix the
  helper rather than updating fixtures.
- Stop if idle-timeout work requires modifying `eggress-relay` semantics or
  copying its copy loop; take the documentation/upstream-request path.
- Stop if the optional timeout would change default half-close or
  byte-conservation behavior.
- Do not close on script inspection alone; closure needs the contract
  transcript, the mismatch failure, and the hosted run.

## Closure evidence

The closure record under `plans/closure/` must include:

- the exact candidate SHA and exact hosted run ID;
- before/after stdout and failure-path transcripts for both fetchers;
- the checksum-mismatch and signal-cleanup evidence;
- the metrics escaping unit output and a normal-output byte comparison;
- the idle-timeout disposition and, if deferred, the pinned relay API gap and
  the upstream request;
- the documentation diff and guard outputs;
- confirmation that no default timeout, limit, or release workflow changed.

## Follow-on rules

M065 activates no successor. M063 and M064 are independently ready. Any
future idle-timeout implementation that requires relay changes must be
registered against Eggress or a separate plan; it must not fork
`eggress-relay` semantics inside Eggchaos.
