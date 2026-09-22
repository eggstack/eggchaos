# M003 closure — fixed-target proxy runtime

Candidate commit: `5dd41027948e7413b595c77f11d7a9e0b31f3785`.

## Evidence

`eggchaos-server` provides validated `ProxySpec`, `ServiceBuilder`,
`EggchaosService`, actual bound-address reporting, global/per-proxy admission
bounds, connection snapshots, structured listener and `JoinSet` ownership,
bounded direct upstream dialing, deterministic connection ordinals, and
graceful service shutdown. Every data-plane session uses `eggress-relay`
1.0.7 with explicit half-close policy. Upstream and downstream policies are
attached to the correct physical write/read directions through the core
bidirectional wrapper.

Commands:

```text
cargo test -p eggchaos-server runtime::tests::ephemeral_fixed_target_proxy_relays_and_drains -- --exact  PASS
cargo test -p eggchaos-server --all-features                                      PASS (4 tests)
cargo clippy -p eggchaos-server --all-targets --all-features -- -D warnings        PASS
```

The integration fixture binds both origin and proxy on ephemeral ports,
round-trips bytes, and verifies clean listener/task drain. Reset capability is
truthfully left as the core abstract request; no unsafe platform code or false
RST claim is present.

## Verdict

M003 acceptance criteria are satisfied for fixed-target TCP. M004 is
unblocked.
