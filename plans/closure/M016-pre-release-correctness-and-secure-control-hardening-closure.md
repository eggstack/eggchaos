# M016 — Pre-release Correctness and Secure-Control Hardening Closure

Verdict: **clean**

## Candidate

- Implementation candidate: `5bb1f81a092172d1a61ae128dd599b84137bf70b`
- Platform: macOS 26.0 (Darwin 25.6.0), `aarch64-apple-darwin`
- Toolchain: Rust 1.89.0 (`29483883e`, pinned workspace toolchain)

The candidate includes all M016 runtime, CLI, regression-test, and user-contract documentation changes. Planning-only closure changes follow it.

## Findings repaired

- Schema-v1 bandwidth `burst_bytes = 0` now returns a field-specific `NativeConfigError` instead of panicking. Other parsed nonzero config values (`bytes_per_second`, `bytes`, and `average_size`) already used typed validation; their error paths were reviewed. The remaining `NonZeroU64::new(...).unwrap()` in config compilation uses the fixed internal latency buffer constant `64 * 1024`, not parsed input.
- The CLI accepts `--admin-token` and `EGGCHAOS_ADMIN_TOKEN`, with the explicit option taking precedence, and sends the bearer header through the existing Eggfetch client. With no credential, it omits the header. Auth failures return nonzero and exactly one JSON document under `--json`; transport errors are generic so they cannot expose a malformed token.
- `AdminFileConfig`, `NativeConfig`, and `AdminConfig` redact configured token values in `Debug` output. Error responses and CLI errors do not include supplied or configured credentials.
- The native router splits the raw path before decoding a fault ID exactly once. Invalid escapes and invalid UTF-8 are rejected. The CLI percent-encodes ID components. Native control no longer narrows `FaultId` to an ASCII route-safe subset; an end-to-end test covers IDs containing slash, percent, question mark, and non-ASCII UTF-8 through add/get/set/remove.

## Evidence

Commands passed on the candidate:

```text
cargo test -p eggchaos-server --all-features -- --test-threads=1  (41 passed)
cargo test -p eggchaos-cli --all-features                        (2 passed)
cargo test -p eggchaos-toxiproxy --all-features                   (10 passed)
RUST_TEST_THREADS=1 ./scripts/check.sh                             (fmt, clippy, workspace tests, docs passed)
cargo audit --deny warnings                                       (passed; no advisories)
cargo deny check advisories licenses bans sources                 (passed)
git diff --check                                                  (passed)
```

Focused regressions include:

- `config::tests::zero_bandwidth_burst_is_a_field_error_without_panicking`
- `config::tests::admin_tokens_are_redacted_from_debug_output`
- `admin::tests::admin_config_debug_redacts_tokens`
- `admin::tests::native_fault_path_components_decode_once_and_reject_malformed_input`
- `cli_authenticates_and_round_trips_opaque_fault_ids`

The first default-parallel server/workspace test attempts on this host hit its open-file limit in existing connection stress tests (`Too many open files`). The same full workspace gate passed with `RUST_TEST_THREADS=1`; focused server tests also passed serially. This is recorded as a local host constraint, not a code failure.

Only the macOS ARM64 host was available in this run; cross-platform CI evidence was not required by M016 and remains part of M019. The pinned Toxiproxy external oracle was not provisioned for this milestone; Toxiproxy translation tests passed, while exact-oracle requalification remains explicitly assigned to M019.

The config/control parsing census found no remaining user-controlled `unwrap`/`expect` path in schema compilation. Remaining production panic assumptions are fixed constants, static response construction, the runtime's internal lock/state invariants, or serialization of `serde_json::Value`; test-only unwraps are excluded.

## Follow-on status

M017 is **ready** because its sole dependency, M016, is closed. M018 remains blocked on M017. M019 remains blocked on M017 and M018 (M016 is now satisfied). No release/tag action is authorized; M019 remains the final pre-tag authority.
