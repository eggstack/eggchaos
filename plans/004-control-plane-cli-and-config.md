# M004 — Native Control Plane, CLI, and Configuration

Status: closed  
Depends on: M003  
Successor: M005

## Objective

Add a small, secure, versioned native administration surface and operator CLI without creating parallel runtime behavior.

The admin API should be built on EggServe's generic H1 leaf substrate; the CLI should use Eggfetch as its HTTP client. All mutations must delegate to one server/runtime authority.

## User-visible outcome

A user can start eggchaos from TOML and manage it with the `eggchaos` CLI or JSON HTTP calls:

```sh
eggchaos serve --config eggchaos.toml
eggchaos proxy list --json
eggchaos proxy add redis --listen 127.0.0.1:0 --upstream 127.0.0.1:6379
eggchaos fault add redis latency --direction downstream --delay 1s --jitter 100ms
eggchaos fault remove redis latency_downstream
eggchaos reset
```

Active-connection structural mutation may still use the M003 snapshot semantics. M005 upgrades live behavior.

## Preconditions

M003 is closed with a stable runtime handle/registry boundary.

Do not expose HTTP endpoints by reaching around the runtime and mutating internal maps directly. Add/adjust a native service command API first if necessary.

## Dependencies

### Admin server

Use:

- `eggserve-server` 0.2-compatible leaf API (or the current published equivalent rechecked at implementation);
- `eggserve-primitives`.

Avoid `eggress-admin` because it is tied to Eggress routing, UDP, reverse-server, and metrics state.

Avoid `eggserve-core` unless a concrete feature is unavailable from the leaf H1 server.

### CLI HTTP client

Use `eggfetch-core` with the smallest supported H1 high-level profile that supplies:

- URL/URI request construction;
- JSON request/response support as needed;
- timeouts;
- ordinary loopback HTTP.

Do not enable proxy, cookies, compression, HTTP/3, or advanced routing for the CLI control client unless a real requirement appears.

## Native configuration schema v1

Define a versioned TOML schema.

Example shape:

```toml
version = 1
seed = 12345

[admin]
bind = "127.0.0.1:8475"

[[proxy]]
name = "redis"
listen = "127.0.0.1:26379"
upstream = "127.0.0.1:6379"
enabled = true

[[proxy.fault]]
id = "read-latency"
direction = "downstream"
type = "latency"
probability = 1.0
delay = "250ms"
jitter = "25ms"
```

Exact syntax may differ, but requirements are:

- schema version required or defaults only if unambiguous and documented;
- no duplicate proxy names;
- no duplicate fault IDs within the intended identity scope;
- human-friendly duration/rate parsing with canonical internal units;
- all values converted through the same native validation authority used by Rust callers/API;
- unknown critical fields rejected rather than silently ignored unless forward-compatibility rules explicitly define otherwise;
- config errors include field context without dumping secrets.

Separate the serializable config DTO from the canonical compiled runtime model if necessary.

## Native API v1

Initial route family:

```text
GET    /v1/health
GET    /v1/version

GET    /v1/proxies
POST   /v1/proxies
GET    /v1/proxies/{name}
PATCH  /v1/proxies/{name}
DELETE /v1/proxies/{name}

GET    /v1/proxies/{name}/faults
POST   /v1/proxies/{name}/faults
GET    /v1/proxies/{name}/faults/{id}
PATCH  /v1/proxies/{name}/faults/{id}
DELETE /v1/proxies/{name}/faults/{id}

GET    /v1/connections
GET    /v1/connections/{id}

POST   /v1/reset
```

M005 may add/delete connection control and scenario endpoints after the live-mutation model is ready.

Keep paths and JSON bodies versioned. Do not place Toxiproxy-compatible unversioned routes in this native module.

## JSON response/error contract

Define a stable machine-readable envelope for failures, for example:

```json
{
  "error": {
    "code": "invalid_fault",
    "message": "slice variation must be smaller than average size",
    "details": {
      "field": "size_variation"
    }
  }
}
```

Requirements:

- stable short error code;
- human message;
- optional structured details;
- appropriate HTTP status;
- no Rust debug dumps/backtraces in API payloads;
- secret values redacted;
- body parse limits.

Success payloads should include enough generation metadata for M005 to add optimistic/live semantics without a breaking redesign.

## Mutation semantics in M004

M004 may use conservative rules:

- create/delete proxy;
- enable/disable proxy;
- fault add/update/remove affects new connections only unless M003 already safely supports more;
- listen/upstream changes are restart-class and may close active connections;
- response explicitly reports resulting generation and whether active connections were affected.

Do not claim live in-flight updates until M005.

## Atomicity

For a proxy update:

1. parse request;
2. validate full candidate;
3. compile candidate;
4. apply as one generation or reject;
5. never leave half-mutated fault lists.

Bulk reset likewise must have deterministic semantics.

Configuration-file startup should validate all definitions before binding if practical, avoiding a partially launched service due to a late parse error.

## Admin security posture

Default admin bind: loopback.

If a user requests non-loopback admin:

- require an explicit `--public-admin`/config opt-in;
- require authentication in the initial design, preferably a bearer token supplied through environment/file/CLI-safe mechanism rather than embedded plaintext in normal logs;
- use constant-time secret comparison where practical;
- redact auth values from Debug/errors;
- reject startup if public bind is requested without required auth.

TLS for the admin API is not required for M004 if the intended remote exposure is explicitly limited/documented; if remote operation over untrusted networks is a release requirement, add a separate plan rather than improvising TLS here.

Data-plane proxy listeners may bind non-loopback according to explicit proxy configuration; admin rules are stricter.

## Health/readiness

`/v1/health` should distinguish process liveness from usable readiness if meaningful.

At minimum expose:

- running state;
- config generation;
- proxy listener counts;
- whether service is draining.

Do not expose per-request internals or secrets.

## CLI structure

Suggested commands:

```text
eggchaos serve
eggchaos proxy list|get|add|set|remove|enable|disable
eggchaos fault list|get|add|set|remove
eggchaos connection list|get
eggchaos reset
eggchaos version
```

Global control options:

- admin endpoint;
- auth token source;
- request timeout;
- `--json`.

CLI mutating commands should use the native HTTP API even when pointed at a locally started daemon; do not maintain a second direct-mutation path except the `serve` bootstrap itself.

## JSON-first CLI

`--json` is a stable automation interface.

Requirements:

- one complete JSON document per invocation unless a future streaming command explicitly documents JSONL;
- errors are JSON on the designated output stream with stable fields;
- no ANSI/control decoration in JSON mode;
- exit code nonzero on failed operation;
- IDs/generations represented losslessly;
- human table formatting is separate presentation code.

## Body/resource limits

Admin API must bound:

- request head size through EggServe runtime config;
- JSON body bytes;
- proxy count;
- faults per proxy;
- active/retained connection query page size;
- string lengths (names/hosts) where appropriate.

Do not accept arbitrarily large config documents.

## Tests

### API

- every route success path;
- invalid JSON;
- oversized body;
- duplicate proxy/fault;
- invalid config values;
- not found;
- bind/upstream restart semantics;
- reset;
- generation increment behavior;
- concurrent read requests while updates occur;
- admin loopback default;
- public-admin startup rejection without auth;
- auth success/failure/redaction.

### Config

- TOML round-trip/canonical parse;
- schema version;
- unknown/duplicate values;
- port 0;
- duration/rate parsing;
- same candidate produces same compiled plan hash if a hash is exposed.

### CLI

- command argument parsing;
- JSON output schema;
- exit codes;
- human smoke;
- unavailable admin;
- auth token not printed;
- create/fault/remove end-to-end against an ephemeral in-process server.

Use actual Eggfetch for end-to-end CLI-client tests where feasible rather than swapping in a different test HTTP client for all cases.

## Documentation

Create:

- native config reference;
- native API reference;
- CLI reference;
- security/admin exposure note;
- examples for Redis/Postgres/generic TCP;
- explicit statement that active structural fault mutation is not fully live until M005 if still true.

## Ordered work packages

Execute in this order:

1. **WP1 — Native command/mutation boundary:** ensure all runtime mutations are available through one typed server authority before exposing HTTP.
2. **WP2 — TOML schema v1:** implement bounded DTO parsing, full-candidate validation/compilation, startup atomicity, and canonical conversion into native runtime types.
3. **WP3 — EggServe admin service:** mount versioned native routes on the leaf H1 runtime with body/head/resource bounds and stable JSON error envelopes.
4. **WP4 — Security posture:** implement loopback default, explicit public-admin opt-in, required authentication, constant-time comparison where appropriate, and redaction.
5. **WP5 — Eggfetch control client:** add the minimal H1 client profile and typed native API client used by the CLI.
6. **WP6 — CLI surface:** implement serve/proxy/fault/connection/read/reset commands, stable `--json`, exit codes, and separate human formatting.
7. **WP7 — End-to-end/API qualification:** run config, concurrency, restart-class, auth, malformed/oversize, and CLI tests against a real ephemeral admin server.
8. **WP8 — Documentation/closure:** publish config/API/CLI/security references, close M004 with evidence, then activate M005.

## Acceptance criteria

M004 closes only when:

- TOML v1 starts the service through the same validated runtime model;
- native `/v1` API covers registered routes with bounded JSON;
- admin H1 serving uses EggServe leaf substrate;
- CLI control calls use Eggfetch;
- JSON CLI output is machine-stable and tested;
- public admin exposure fails closed without explicit opt-in/auth;
- updates are atomic by generation;
- docs accurately describe any new-connections-only limitation;
- CI/integration tests pass;
- closure evidence is committed.

## Stop/rejection conditions

Stop/revise if:

- implementing admin routes requires modifying EggServe internals rather than using its public service surface;
- the CLI starts duplicating proxy/fault runtime logic;
- config DTOs become a second validation authority;
- public admin can accidentally bind wildcard/non-loopback without explicit opt-in;
- auth secrets appear in Debug/log/API errors;
- mutation can leave an invalid partial runtime generation.

## Closure evidence

Create `plans/closure/M004-control-plane-cli-and-config-closure.md` containing:

- candidate SHA;
- dependency feature tree for EggServe/Eggfetch;
- API/CLI test commands/results;
- security bind/auth cases;
- representative JSON fixtures;
- documented deviations.

Then update registry:

- M004 -> `closed`;
- M005 -> `ready`.
