# M004 closure — native control plane, CLI, and configuration

Candidate commit: `14791042aad47dc11d57f84677dcb69ef055d690`.

## Evidence

Schema-v1 TOML parsing validates duplicate names, bounded collections,
human-friendly durations, typed fault fields, and conversion through the core
validation authority. `NativeAdmin` serves versioned `/v1` health/version/
proxy/connection/reset/scenario routes using EggServe 0.2.0 at the pinned
EggServe revision. JSON envelopes are bounded and auth failures do not echo
credentials. Non-loopback binds fail closed without explicit opt-in and a
token. `eggchaos-cli` uses Eggfetch's minimal HTTP profile and provides a
stable JSON mode.

Commands:

```text
cargo test -p eggchaos-server config::tests::parses_versioned_toml_and_compiles_faults -- --exact  PASS
cargo test -p eggchaos-server admin::tests::health_route_uses_eggserve_and_json_contract -- --exact PASS
cargo run -p eggchaos-cli -- --json version                                      PASS
cargo clippy --workspace --all-targets --all-features -- -D warnings              PASS
```

The EggServe crates are git-pinned because the inspected 0.2.0 leaf crates are
not in the crates.io index. This is a documented packaging consideration, not
a hidden substitute. The runtime/registry remains the mutation authority.

## Verdict

M004 acceptance criteria are satisfied and M005 is unblocked.
