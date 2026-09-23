# M016 — Pre-release Correctness and Secure-Control Hardening

Status: closed
Depends on: M015
Role: release-blocking corrective successor

## Objective

Correct the concrete correctness, security, and operability defects found by the 2026-09-23 repository audit before any `v0.1.0` tag or publication.

This plan is intentionally narrow. It does not redesign fault semantics or the runtime architecture. It repairs user-controlled panic paths, makes the already-supported authenticated admin deployment usable through the official CLI, prevents secret disclosure through debug surfaces, and makes opaque fault IDs reliably addressable through the native HTTP API.

## User-visible outcome

After M016:

- malformed native configuration returns a typed error instead of panicking;
- the `eggchaos` CLI can authenticate to a bearer-protected admin endpoint without requiring users to hand-roll HTTP calls;
- configured admin secrets are redacted from debug/error representations;
- every native `FaultId` accepted by the domain model can be addressed through CLI/API fault routes without ambiguous path handling;
- existing loopback/default behavior and Toxiproxy compatibility remain unchanged.

Tagging, crates.io publication, and GitHub release creation remain blocked until the successor qualification gate closes.

## Baseline and findings

Baseline: `main` at `62532be6a6a5cdecabf35c3e61e4e28aa4878e15`.

Audit findings to repair:

1. `FaultFileConfig::compile` constructs bandwidth `burst_bytes` with `NonZeroU64::new(...).unwrap()`. Explicit `burst_bytes = 0` in TOML can therefore panic instead of returning `NativeConfigError::Field`.
2. `NativeAdmin` supports bearer-authenticated non-loopback operation, but the official CLI has no admin-token input and sends no `Authorization` header.
3. `AdminFileConfig` / `NativeConfig` derive `Debug` while storing `auth_token: Option<String>`, so a secret can be exposed if those values are formatted.
4. `FaultId` accepts arbitrary nonempty UTF-8 up to 128 bytes, while native routes address IDs as one path component. The CLI currently interpolates IDs directly into URLs, so characters such as slash, percent, question mark, or non-ASCII are not a reliable round-trip contract.

These are release-blocking defects because they contradict the existing configuration-error, secure-admin, redaction, and addressability claims.

## Scope

### In scope

- Panic-free validation for all user-provided non-zero configuration values touched by the schema-v1 compiler, beginning with bandwidth `burst_bytes` and including a small census for equivalent `unwrap`/`expect` assumptions on user input.
- CLI authentication for native admin requests.
- Secret redaction for native configuration/admin debug surfaces and request failures.
- A single explicit path-component encoding/decoding contract for native proxy/fault identifiers, with fault IDs as the required coverage target.
- Focused tests and user documentation for all repaired behavior.

### Non-goals

- No new fault types.
- No change to the seven release-baseline fault semantics.
- No Toxiproxy v2.12 semantic expansion.
- No UDP/datagram work.
- No general forward-proxy or outbound-chain support.
- No broad native API schema redesign; that belongs to M017.
- No runtime module decomposition; that belongs to M018.
- No final release verdict; that belongs to M019.

## Affected surfaces

Expected primary files:

- `crates/eggchaos-server/src/config.rs`
- `crates/eggchaos-server/src/admin.rs`
- `crates/eggchaos-cli/src/main.rs`
- `crates/eggchaos-cli/tests/cli_e2e.rs`
- core/server tests needed for identifier round trips
- `docs/configuration.md`
- `docs/control-plane.md`
- root `README.md` if CLI auth syntax is user-facing there

If a small shared path-component helper is introduced, keep it on the native control boundary; do not move HTTP concerns into `eggchaos-core`.

## Ordered work packages

### WP1 — Eliminate configuration panic paths

Replace user-input `unwrap`/`expect` assumptions in schema-v1 compilation with typed validation.

At minimum:

- `bytes_per_second = 0` remains a typed field error;
- `burst_bytes = 0` becomes a typed field error rather than a panic;
- other `NonZero*` values reachable from parsed TOML are checked for the same class of defect;
- error messages name the relevant configuration field.

Add a regression test that parses a complete TOML document containing `burst_bytes = 0` and proves `NativeConfig::parse` returns `Err` without unwinding.

### WP2 — Make authenticated admin operable from the CLI

Add a supported credential input to the CLI and apply it to every native admin request.

Required behavior:

- support `EGGCHAOS_ADMIN_TOKEN` for automation and a CLI override such as `--admin-token`;
- if a file-based option is added, trim only the terminal line ending and never echo the token;
- define deterministic precedence when more than one credential source is present;
- send exactly `Authorization: Bearer <token>` when configured;
- omit the header entirely when no token is configured;
- authentication failures still produce one JSON document under `--json` and nonzero exit status;
- no credential appears in request errors, debug output, or human error text.

Do not add a second HTTP client. Continue using `eggfetch-core`.

### WP3 — Redact configured secrets

Ensure formatting native config/admin values cannot expose the bearer token.

Acceptable approaches include a redacted secret wrapper or custom `Debug` implementations. The externally serialized TOML/Serde contract may remain string-based where required for loading configuration, but debug formatting must render a fixed redacted marker.

Tests must assert the literal secret is absent from representative `Debug` output.

### WP4 — Define native path-component round trips

Preserve `FaultId` as an opaque domain identity rather than silently narrowing the embeddable core API solely for HTTP convenience.

Implement strict native route component encoding/decoding:

- the CLI percent-encodes identifier components before composing URLs;
- the native admin router splits the raw path first, then decodes the relevant component exactly once;
- malformed percent escapes and invalid UTF-8 fail as bounded `400 invalid` errors;
- encoded slash and percent characters can round-trip as one logical fault ID;
- decoding must not create route traversal or cause a second routing pass;
- proxy-name restrictions may remain unchanged unless the implementation deliberately generalizes them with equivalent tests.

Keep Toxiproxy compatibility routing independent; do not alter oracle-visible toxic naming unless required by a demonstrated bug.

### WP5 — Documentation and regression census

Update control/config docs with:

- CLI credential sources and precedence;
- redaction behavior;
- path-component behavior for opaque IDs;
- explicit statement that malformed config is rejected, not allowed to panic.

Run a focused source census for production `unwrap`/`expect` sites in config/control parsing and record any remaining invariant-only uses in the closure note.

## Behavioral invariants and failure semantics

- User-controlled malformed TOML/JSON/CLI input must not panic the process.
- Admin remains loopback by default.
- Non-loopback admin still requires explicit `public_admin = true` and a token.
- Secrets are never emitted by JSON errors, human CLI errors, or `Debug` formatting.
- Native URL encoding is transport representation only; it must not mutate the logical fault identity.
- Existing valid unreserved identifiers remain byte-for-byte compatible in URLs.
- `ControlState` remains the only runtime mutation authority.
- No new unbounded allocation or state is introduced.

## Verification

Minimum commands:

```sh
cargo test -p eggchaos-server --all-features
cargo test -p eggchaos-cli --all-features
cargo test -p eggchaos-toxiproxy --all-features
./scripts/check.sh
cargo audit --deny warnings
cargo deny check advisories licenses bans sources
```

Required focused tests:

- TOML `burst_bytes = 0` returns a field error without panic;
- native config debug output does not contain a configured token;
- CLI against authenticated loopback admin succeeds with the configured token;
- same request without/wrong token fails nonzero and does not leak either token;
- special-character fault ID add/get/set/remove round-trips through CLI + HTTP;
- malformed path escaping is rejected safely;
- ordinary unreserved IDs remain unchanged.

## Acceptance criteria

M016 may close only when all four audit defects are repaired, focused regression tests are green, full workspace CI-equivalent checks pass, docs describe the resulting behavior, and no new compatibility regression is introduced.

A closure note under `plans/closure/` must record the exact candidate SHA, commands, platforms, focused test names/results, any remaining invariant-only panic sites, and whether M017 is activated.

## Stop/rejection conditions

Do not close M016 if:

- any demonstrated user-controlled config path can still panic;
- the CLI cannot operate the supported authenticated admin deployment;
- a credential appears in logs/errors/debug output;
- the proposed ID fix breaks declared Toxiproxy compatibility without a separately documented decision;
- path decoding permits ambiguous double-decoding or route traversal;
- full workspace checks fail.

If fixing identifier transport requires changing the public core identity contract, stop and write an ADR rather than silently narrowing it.

## Follow-on activation

On clean closure: M017 becomes `ready`.

Until M019 closes, the previous M015 candidate remains historical qualification evidence only and must not be treated as the final tag candidate.
