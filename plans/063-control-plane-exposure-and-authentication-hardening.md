# M063 — Control-Plane Exposure and Authentication Hardening

Status: active
Depends on: M062 closed; 2026-11-17 read-only security audit at `62682d9`
Role: security corrective for the native admin and Toxiproxy compatibility
control listeners; no data-plane, route-removal, or deployment redesign
Registration baseline: `60c28a48c2375d20c0cf793372423751967f2b9a` (`main`)
Audit evidence: `62682d955c802f8caedc43ec66f7100ca1af9dbb`
(`deps/update-dependencies`; no crate source differs from `main`)

## Objective

Close the control-plane exposure findings from the 2026-11-17 security audit
without weakening the local developer workflow or removing any native route,
CLI command, SDK operation, or Toxiproxy semantics:

1. make the native admin listener reject browser-originated cross-site
   requests and DNS-rebinding `Host` values while keeping the documented
   unauthenticated loopback workflow usable by local tools;
2. make bearer credentials for non-loopback administration resistant to
   trivial guessing and make failed authentication observably throttled;
3. make bearer parsing RFC 7235-compatible without changing the accepted
   token bytes;
4. refuse non-loopback Toxiproxy compatibility binds unless the caller
   explicitly opts in, mirroring the native admin guard;
5. document the loopback trust model, the reverse-proxy contract, and the
   remaining cleartext-transport limitation;
6. prove all of the above with integration tests, a manual browser-style
   probe, the full local gate, and exact-head hosted CI.

This milestone corrects an exposure and credential-hardening defect class. It
does not add TLS to the admin listener, does not introduce a second
authentication scheme, and does not treat `Origin` as an authentication
factor.

## Research baseline

At the registration baseline, the audit established:

- the native admin default is `127.0.0.1:8475` and is unauthenticated unless
  `[admin].auth_token` is configured (`docs/configuration.md:7`);
- non-loopback operation already requires `public_admin = true` plus a
  non-empty token, checked at config parse (`crates/eggchaos-server/src/
  config.rs:200-213`) and again before bind (`crates/eggchaos-server/src/
  admin.rs:102-110`), with regression tests in
  `crates/eggchaos-server/tests/admin_auth.rs`;
- token comparison hashes both inputs to SHA-256 before an XOR fold
  (`admin.rs:683-689`), so the comparison itself is fixed-width;
- the request handler authenticates and then routes without inspecting
  `Host`, `Origin`, `Sec-Fetch-Site`, or the request `Content-Type`
  (`admin.rs:168-232`);
- `POST /v1/reset` ignores the request body entirely (`admin.rs:302`);
- proxy creation parses JSON regardless of the declared media type
  (`admin.rs:463`);
- a live probe at the audit revision returned `200` for
  `GET /v1/health` with `Host: evil.example:38475` and
  `Origin: https://evil.example`, `200` for `POST /v1/reset` with
  `Content-Type: text/plain`, and `201` for `POST /v1/proxies` with a
  `text/plain` JSON body;
- a configured token is accepted only as `Bearer <token>` with an exact
  case-sensitive scheme: `bearer <token>` is currently pinned to `403` in
  `admin_auth.rs`, which is stricter than RFC 7235;
- `ToxiproxyHttp::start(bind, adapter)` binds any `SocketAddr` with no
  loopback guard and no authentication
  (`crates/eggchaos-toxiproxy/src/lib.rs:1143-1150`); the docs only say
  "loopback by default" (`docs/toxiproxy.md:62`);
- the CLI and generated SDKs set `Content-Type: application/json` whenever
  they send a body (`eggfetch-core` `Request::json`; Python `client.py`
  `_request`; TypeScript `client.ts`), so a native-admin JSON media-type
  requirement is compatible with first-party clients;
- EggServe preserves `ConnectionInfoExt` in request extensions, so the
  admin handler can obtain the peer address for a bounded failure budget
  without a new dependency;
- EggServe already bounds header read (10 s), body read (30 s), total
  connection (60 s), and handler time (30 s); this milestone does not
  duplicate those limits.

The defect is not missing authentication by default on loopback. Loopback
without a token is a documented same-user trust boundary. The defect is that
a browser origin is not the same trust boundary as a local process: simple
requests, form navigations, and DNS rebinding can reach a loopback listener
that never intended to accept browser-originated control traffic.

## Scope

### In scope

1. Add an admin request-origin policy that validates the `Host` authority for
   every native route and rejects `Host` values outside the configured set
   before routing.
2. Add an `Origin` policy for state-changing methods: a present non-null
   `Origin` must use `http` or `https` and carry an allowed authority, so a
   TLS-terminated deployment can accept its own `https` origin; `null` and
   foreign origins are rejected.
3. Require `application/json` for native-admin requests that carry a
   non-empty body; return `415` otherwise. Bodyless mutations such as
   `POST /v1/reset` remain bodyless and rely on `Host`/`Origin` validation.
4. Add optional `[admin].allowed_hosts` entries for reverse-proxy and
   public-host deployments. With the field absent, allow only the bind
   literal, `localhost`, and the loopback literals that apply to the bind.
   Reject wildcards, schemes, paths, and empty entries.
5. Require a non-loopback `auth_token` to be at least 32 bytes of
   non-whitespace material. Keep loopback tokens optional and unrestricted in
   length, but emit a startup warning when a loopback token is shorter than
   32 bytes.
6. Accept the bearer scheme case-insensitively while preserving the exact
   credential bytes after the scheme separator.
7. Add a bounded per-peer failed-authentication budget (for example a
   five-failure burst with a one-per-second refill and a bounded peer map)
   that returns `429` with `Retry-After: 1`. If peer metadata is absent,
   apply a small fixed failure delay instead of a shared lockout.
8. Keep `403` for missing or wrong credentials, and never echo token
   material, the supplied header, or the configured token.
9. Make `ToxiproxyHttp::start` refuse a non-loopback bind with a clear
   startup error, and add an explicitly named additive constructor or
   options value for intentional public binds. Update the `compat_server`
   example to require an explicit `--allow-public` flag or equivalent before
   it will bind beyond loopback.
10. Update `docs/control-plane.md`, `docs/configuration.md`,
    `docs/toxiproxy.md`, the OpenAPI error descriptions if needed, and the
    README security note so operators can see the trust model and the
    reverse-proxy host contract.
11. Add regression tests for every rejected and accepted shape, including
    the exact audit probes, first-party CLI/SDK traffic, and the Toxiproxy
    compatibility path.
12. Re-run the full local gate, the pinned Toxiproxy v2.12 differential, the
    language-client qualification, and exact-head hosted CI.

### Non-goals

- no TLS listener, certificate management, or automatic TLS termination;
- no replacement of bearer authentication with mTLS, OAuth, signed
  requests, or CSRF tokens stored in server state;
- no change to the native `/v1` route set, request/response DTOs, status
  codes for successful operations, or CLI command inventory;
- no change to Toxiproxy strict-v2.12 or post-v2.12 wire semantics;
- no change to proxy data-plane behavior, fault semantics, RNG, scheduling,
  evidence, or persistence;
- no removal of the documented unauthenticated loopback default;
- no new runtime dependency for host parsing or throttling;
- no change to the admin body limit (1 MiB) or connection limit (128);
- no publication, tag, or release workflow change;
- no rewrite of M059's native-auth corrective record; this plan builds on it.

## Affected surfaces

Expected implementation surfaces:

- `crates/eggchaos-server/src/admin.rs` — origin policy, media-type policy,
  bearer parsing, failure budget, config-derived allowlist;
- `crates/eggchaos-server/src/config.rs` — optional `allowed_hosts`, token
  strength validation, field errors;
- `crates/eggchaos-server/src/lib.rs` if a public type must be re-exported;
- `crates/eggchaos-server/tests/admin_auth.rs` and a new focused integration
  test file for origin/media-type behavior;
- `crates/eggchaos-toxiproxy/src/lib.rs` — loopback guard and explicit public
  constructor/options;
- `crates/eggchaos-toxiproxy/examples/compat_server.rs` — explicit exposure
  flag;
- `crates/eggchaos-toxiproxy/tests/` as needed for guard and public opt-in;
- `docs/control-plane.md`, `docs/configuration.md`, `docs/toxiproxy.md`,
  `README.md`, and `api/openapi/eggchaos-v1.yaml` if error descriptions
  change;
- `plans/registry.md` and the generated planning-state projections;
- a closure record under `plans/closure/` at closure.

Expected non-surfaces: `eggchaos-core`, `eggchaos-experiment`,
`eggchaos-protocol` wire DTOs, `eggchaos-embed`, `eggchaos-eggfetch`, the
Python/TypeScript client source, Cargo manifests/lockfiles, and release
workflows. Any change there needs an explicit recorded reason.

## Ordered work packages

### WP1 — Freeze the trust model and write failing tests

1. Record the registration baseline and the audit probe transcript.
2. Add table-driven tests that currently fail:
   - foreign `Host` on `GET /v1/health` and `/metrics`;
   - foreign `Origin` on `POST /v1/reset`, `POST /v1/proxies`, and
     `PATCH`/`DELETE` mutations;
   - `Origin: null`;
   - `Content-Type: text/plain` with a JSON body;
   - missing `Content-Type` with a non-empty body;
   - `bearer`/`BEARER` scheme acceptance;
   - duplicate `authorization` headers and the documented first-value
     behavior;
   - short non-loopback token rejection;
   - Toxiproxy non-loopback bind rejection.
3. Add positive tests for the CLI, Python client, TypeScript client, and
   loopback no-token workflow so the security policy cannot silently break
   first-party consumers.

### WP2 — Host and Origin policy

1. Derive the default authority set from the bind (`127.0.0.1`, `localhost`,
   `[::1]` as applicable) plus the literal bind address and port.
2. Parse the `Host` header without trusting proxy forwarding headers.
   Reject absent, malformed, userinfo-bearing, scheme-bearing, wildcard, or
   out-of-set values before route dispatch.
3. Add `[admin].allowed_hosts` as an optional bounded list for reverse-proxy
   and public-host deployments. Validate entries at config parse and at
   startup; do not accept `*`.
4. For state-changing methods, validate a present `Origin`: require an
   `http` or `https` scheme and an authority in the allowed set; reject
   `null`; allow an absent `Origin` for non-browser clients.
5. Keep `GET`/`HEAD` usable by local scrapers with the correct `Host`; do not
   require `Origin` for reads.
6. Return a bounded `403` error envelope with a stable code; do not include
   the offending value in the message or in logs.

### WP3 — Media-type policy

1. For native-admin methods that carry a body, require a media type whose
   essence is `application/json` (parameters allowed).
2. Return `415` with a bounded error envelope for a missing or different
   media type; read no body beyond the existing 1 MiB cap.
3. Leave bodyless mutations and bodyless `DELETE` requests unchanged.
4. Do not apply this policy to the Toxiproxy compatibility listener; parity
   with upstream clients takes precedence there, and the loopback/public
   guard is the compatibility control.

### WP4 — Bearer parsing and failure budget

1. Parse the scheme case-insensitively, require at least one separator space,
   and preserve the remainder exactly for the constant-time comparison.
2. Keep SHA-256 fixed-width comparison and redaction behavior unchanged.
3. Enforce `>= 32` bytes and no whitespace-only content for non-loopback
   tokens at both config parse and `NativeAdmin::start`; keep the existing
   `InsecurePublicBind` failure class or add a clearly documented field
   error.
4. Add a bounded per-peer failure budget using `ConnectionInfoExt` when
   present; otherwise apply a fixed small delay. Do not add a process-global
   lockout that a remote attacker could use to deny valid operators.
5. Add tests proving a valid token is never delayed or counted, failed
   attempts are bounded, and the peer map cannot grow without limit.

### WP5 — Toxiproxy exposure guard

1. Make the existing `ToxiproxyHttp::start` reject a non-loopback bind with a
   clear error before binding.
2. Add an additive explicit-public API (`start_with_options`, a policy
   struct, or an equivalent named constructor); do not remove or rename the
   existing entry point.
3. Update `examples/compat_server.rs` so a non-loopback argument fails
   unless `--allow-public` (or an equivalent explicit flag) is present.
4. Add tests for the refusal path using a documentation address and for the
   opt-in path on loopback.
5. Update `docs/toxiproxy.md` to state that the compatibility API has no
   authentication and must not be exposed without an explicit decision.

### WP6 — Documentation and contract reconciliation

1. `docs/control-plane.md`: document the loopback trust model, the
   Host/Origin policy, the media-type requirement, the failure-budget
   behavior, and the reverse-proxy `allowed_hosts` contract.
2. `docs/configuration.md`: document `allowed_hosts`, the 32-byte
   non-loopback token minimum, and the fact that the listener remains
   cleartext HTTP and still requires a trusted TLS terminator.
3. `docs/toxiproxy.md` and the README security paragraph: document the
   compatibility guard and the no-authentication warning.
4. If error codes or status descriptions are added to OpenAPI, update the
   specification and re-run the drift check; do not add routes.
5. Update the registry row and plan header only through the normal planning
   workflow.

### WP7 — Qualification and closure

1. Run the focused suites and the full local gate.
2. Run the audit probe commands against a fresh loopback daemon and record
   the rejected statuses.
3. Run the pinned Toxiproxy v2.12 differential and the language-client
   qualification.
4. Obtain exact-head hosted CI on the final candidate.
5. Write the closure record with the exact SHA, run ID, probe transcript,
   test commands, and any residual limitations.

## Invariants and failure semantics

- No native mutation may execute for a request whose `Host` is outside the
  configured set, regardless of method or body.
- A present, non-null `Origin` must match an allowed authority for
  state-changing methods; `null` is always rejected for those methods.
- A body-bearing native mutation without a JSON media type never reaches
  route dispatch.
- The bearer comparison remains constant-time over fixed-width digests; no
  token, header value, or configured secret appears in a response, error,
  trace, metric label, or test fixture that ships.
- The default loopback no-token workflow remains functional for the CLI,
  Python client, TypeScript client, and local `curl` with the correct
  `Host`.
- The `public_admin` guard still requires both explicit opt-in and a
  non-empty token; this milestone tightens rather than relaxes it.
- Toxiproxy non-loopback binds fail before a socket is bound unless the
  caller passed the explicit public option.
- Existing body, connection, header, and timeout bounds are unchanged.
- No new dependency, no `unsafe`, and no change to deterministic data-plane
  behavior.
- A malformed `Host`, `Origin`, or media type yields a bounded error and
  never a panic.

## Test commands

Focused:

```sh
cargo test -p eggchaos-server --all-features
cargo test -p eggchaos-toxiproxy --all-features
cargo test -p eggchaos-cli --all-features
cargo test -p eggchaos-protocol --all-features
```

Full local gate:

```sh
./scripts/check.sh
```

Language clients (using the repository's pinned Python environment):

```sh
uv run --with-requirements .github/python-ci/requirements.txt -- sh -c '
  ./scripts/check_python_client.sh
  ./scripts/check_typescript_client.sh
  ./scripts/qualify_language_clients.sh
'
```

Pinned compatibility oracle:

```sh
TOXIPROXY_SERVER="$(./scripts/fetch_toxiproxy_v2_12.sh)" \
EGGCHAOS_REQUIRE_TOXIPROXY_ORACLE=1 \
./scripts/qualify_toxiproxy_v2_12.sh
```

Manual audit probe (expected: `403`, `403`, `415`):

```sh
curl -sS -i http://127.0.0.1:8475/v1/health \
  -H 'Host: evil.example:8475' -H 'Origin: https://evil.example'
curl -sS -i -X POST http://127.0.0.1:8475/v1/reset \
  -H 'Host: evil.example:8475' -H 'Origin: https://evil.example' \
  -H 'Content-Type: text/plain' --data 'x'
curl -sS -i -X POST http://127.0.0.1:8475/v1/proxies \
  -H 'Origin: https://evil.example' -H 'Content-Type: text/plain' \
  --data '{"name":"probe","listen":"127.0.0.1:0","upstream":"127.0.0.1:9"}'
```

Planning and contract guards:

```sh
python3 scripts/check_planning_state.py --check
sh scripts/tests/test_planning_state.sh
python3 scripts/check_release_state_docs.py --check
./scripts/check_openapi.sh
python3 scripts/check_action_pins.py --check
```

## Acceptance criteria

1. Every audit probe above returns the documented rejection status; no
   mutation occurs for a foreign `Host` or `Origin`.
2. A correct `Host` and JSON body still produce the existing success status
   and body for CLI and SDK requests.
3. `bearer`, `Bearer`, and `BEARER` schemes all authenticate the same exact
   token; a wrong token still returns `403`.
4. Non-loopback configuration with a token shorter than 32 bytes fails at
   parse and at startup; a 32-byte token succeeds.
5. `ToxiproxyHttp::start` refuses `192.0.2.1:0` before bind; the explicit
   public option is required for any non-loopback bind.
6. The pinned Toxiproxy differential remains 50/50 and the language-client
   qualification remains green.
7. `./scripts/check.sh`, the planning guards, and exact-head hosted CI are
   green on the final candidate.
8. Documentation states the trust model and reverse-proxy contract without
   claiming the listener is encrypted.

## Stop conditions

- Stop if `Host` validation cannot be made compatible with a reverse-proxy
  deployment without a new configuration surface that the project rejects.
- Stop if JSON media-type enforcement breaks a first-party client or the
  Toxiproxy differential; scope the media-type rule to the native admin only
  before proceeding.
- Stop if Toxiproxy public-bind support requires removing or renaming the
  existing `start` entry point; the change must be additive.
- Stop if the failure budget can be repurposed as a global denial of service
  against a valid operator.
- Stop if any test requires weakening constant-time comparison, token
  redaction, or the non-loopback opt-in guard.
- Do not mark the milestone closed on source inspection alone; closure needs
  the probe transcript, the test evidence, and the exact-head hosted run.

## Closure evidence

The closure record under `plans/closure/` must include:

- the exact candidate SHA and the exact hosted run ID with job results;
- the audit probe transcript showing the before/after behavior;
- focused and full-gate test output;
- the pinned oracle differential result and language-client result;
- the final `allowed_hosts`/token-strength documentation diff;
- residual limitations (cleartext HTTP, loopback same-user trust, any
  reverse-proxy configuration requirement).

## Follow-on rules

M063 activates no successor. M064 (repository governance) and M065
(residual runtime/tooling hardening) were registered as independent plans; see
`plans/registry.md` for their current execution states. A future TLS-admin
feature requires a separate ADR and plan; it must not be smuggled into this
corrective.
