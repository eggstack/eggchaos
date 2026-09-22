# Toxiproxy v2.12.0 oracle baseline (M012 WP1)

Captured live against the pinned oracle on 2026-09-22. All probes used
`127.0.0.1:18474` with `curl` against a fresh server instance.

## Oracle identity

- Download URL: `https://github.com/Shopify/toxiproxy/releases/download/v2.12.0/toxiproxy-server-darwin-arm64`
- `toxiproxy-server -version` prints `toxiproxy-server version 2.12.0`
- `-port` values above 65535 are rejected by the oracle CLI itself
  (e.g. `-port 84740` fails with `listen tcp: address 84740: invalid port`).
- `GET /version` returns HTTP 200 `{"version": "2.12.0"}`.

## Proxy routes

- `GET /proxies` returns HTTP 200 with a JSON **object keyed by proxy name**,
  not an array: `{"p1": {"name":"p1","listen":"...","upstream":"...","enabled":true,"Logger":{},"toxics":[]}}`.
- `POST /proxies` returns HTTP 201 with the created proxy object.
  The object always contains `"Logger":{}` and `"toxics":[]`.
- Duplicate name returns HTTP 409 `{"error":"proxy already exists","status":409}`.
- Missing proxy on GET/POST/DELETE returns HTTP 404
  `{"error":"proxy not found","status":404}`.
- Malformed JSON returns HTTP 400, e.g.
  `{"error":"bad request body: invalid character 'b' looking for beginning of object key string","status":400}`.
- Missing `name` returns HTTP 400 `{"error":"missing required field: name","status":400}`.
- Listen bind conflict returns HTTP 500, e.g.
  `{"error":"listen tcp 127.0.0.1:19041: bind: address already in use","status":500}`.
- `POST /proxies/{name}` (update) returns HTTP 200 with the full proxy object
  including the embedded toxics array. Setting `{"enabled":false}` persists.
- `DELETE /proxies/{name}` returns HTTP 204 with an empty body.

## Reset and populate

- `POST /reset` returns HTTP 204. It re-enables every proxy (`enabled=false`
  becomes `true`) and removes all toxics.
- `POST /populate` with `[{...}]` returns HTTP 201 `{"proxies":[...]}`.
  Observed semantics: existing proxies (matched by name) are returned
  **unchanged** (enabled flag and toxics preserved); new names are created
  (honoring `enabled:false`); unknown names that fail to bind are skipped.
  Populate does not delete proxies absent from the input.

## Toxic routes (seven types)

Valid types: `latency`, `bandwidth`, `slow_close`, `timeout`, `slicer`,
`limit_data`, `reset_peer`. Anything else returns HTTP 400
`{"error":"invalid toxic type","status":400}`.

- `POST /proxies/{p}/toxics` returns HTTP 200 with the created toxic.
  Omit `attributes` to get zero-valued defaults per type:
  - `latency`: `{"latency":0,"jitter":0}`
  - `bandwidth`: `{"rate":0}`
  - `slow_close`: `{"delay":0}`
  - `timeout`: `{"timeout":0}`
  - `slicer`: `{"average_size":0,"size_variation":0,"delay":0}`
  - `limit_data`: `{"bytes":0}`
  - `reset_peer`: `{"timeout":0}`
- Omitting `name` auto-generates `<type>_<stream>`
  (observed `latency_downstream` for `type=latency, stream=downstream`).
- Invalid `stream` returns HTTP 400
  `{"error":"stream was invalid, can be either upstream or downstream","status":400}`.
- Duplicate toxic name within a proxy returns HTTP 409
  `{"error":"toxic already exists","status":409}`.
- Toxic on a missing proxy returns HTTP 404 `{"error":"proxy not found","status":404}`.
- `toxicity` is **not range-validated**: `2.5` and `-1.0` are stored as-is.
- `GET /proxies/{p}/toxics/{t}` returns HTTP 200; missing toxic returns
  HTTP 404 `{"error":"toxic not found","status":404}`.
- `POST /proxies/{p}/toxics/{t}` returns HTTP 200 with the updated toxic.
  A `type` change in the update body is ignored: the toxic keeps its original
  type and attributes.
- `DELETE /proxies/{p}/toxics/{t}` returns HTTP 204 with an empty body;
  missing toxic returns HTTP 404 `{"error":"toxic not found","status":404}`.
- `PATCH` mirrors `POST` for proxy updates and toxic updates with identical
  semantics (the pinned Go client uses `PATCH` for toxic updates).

## Non-API routes

- `GET /metrics` returns HTTP 404 plain-text `404 page not found`
  (no Prometheus endpoint without the metrics flags).
- Unknown routes return HTTP 404 plain-text `404 page not found`.
