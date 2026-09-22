# M006 closure — Toxiproxy v2.12 compatibility

Candidate commit: `e3d1d1faaf390f7d2b8b134f8650dc5a385a0110`.

## Oracle evidence

The Shopify Toxiproxy v2.12.0 Darwin amd64 release binary was downloaded
from the v2.12.0 release and verified with SHA-256:

```text
toxiproxy-server version 2.12.0
9625bba4bd96117eedae49f982aba4c2f462b268dd406c9ff18186f9b1ef8afe
```

The oracle was started on loopback port 18474. Its `/version` response was
`{"version":"2.12.0"}` and its minimal `/proxies` create response was observed
as HTTP 201 with an actual ephemeral listen port. The checked-in qualification
script accepts `TOXIPROXY_SERVER` and reports the pinned oracle identity.

## Implementation and tests

`eggchaos-toxiproxy` now owns v2.12 DTOs/defaults, all seven toxic mappings,
the compatibility mutation facade, and an EggServe-backed compatibility HTTP
listener for `/version`, `/proxies`, proxy toxic listing/creation, delete, and
reset. Translation always delegates fault behavior and live changes to native
`ControlState`/`LivePolicy` state.

```text
cargo test --workspace --all-features              PASS (17 tests)
cargo clippy --workspace --all-targets --all-features -- -D warnings  PASS
TOXIPROXY_SERVER=... ./scripts/qualify_toxiproxy_v2_12.sh              PASS
```

The checked-in route tests cover version, create/list, toxic add/list, all
seven mappings, direction defaults, and native generation updates. The
qualification is API-shape and translation qualified on macOS; a full byte-
behavior differential corpus across all toxic timing/reset cases and a
Go-client smoke were not run in this environment. Accordingly the published
claim is compatibility for the tested surface, with reset behavior
intent-qualified and platform-qualified, not an unqualified drop-in claim.

## Verdict

M006 is closed for the declared, evidence-backed compatibility surface. M007
is unblocked.
