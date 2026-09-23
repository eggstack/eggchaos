# M019 Closure — Qualification Expansion and Final Corrective Requalification

Verdict: **clean**

## Candidate and scope

- Exact implementation candidate: `ca527dbda953dac705e8349ff9686842cdc8a6f3`
- Candidate branch: `main`
- M016, M017, and M018 were closed before this gate; M015 remains historical evidence for `cd88b22`.
- M019 added mandatory checksum-pinned Toxiproxy release qualification, five fuzz targets and seed corpora, native DTO validators, bounded bandwidth/slow-close/slicer differential cases, and reconciled verification documentation.

## Verification evidence

- Local workspace format, Clippy, tests, and docs passed in `RUST_TEST_THREADS=1 ./scripts/release-smoke.sh`; audit and deny checks, package verification, artifact smoke, and publish-order proof also passed.
- Local strict Toxiproxy v2.12.0 differential passed using the checksum-pinned oracle. The differential corpus reported 50 cases and `failed:0`; bandwidth, slow-close, and slicer data-plane cases preserved bytes and met their documented tolerance windows. Exact Go scheduler/chunk identity is not claimed.
- Expanded fuzz qualification passed all six targets (`plan_json` plus five new targets), 10,000 runs each, on pinned Rust 1.89.0. No crashes were found.
- `./scripts/qualify_eggfetch.sh` passed locally and in the dedicated release workflow.
- Canonical same-session benchmark: bare relay 4928.047 MiB/s; empty-plan 5110.960 MiB/s (103.7% of bare relay), above the frozen 70% floor.
- Fresh Go and Python Toxiproxy v2.12 client smoke transcripts are retained under `qualification/toxiproxy-v2-12/m019-client-smoke/`; each reports zero failures.
- Ordinary GitHub CI run [35875975229](https://github.com/eggstack/eggchaos/actions/runs/35875975229) passed on Ubuntu, macOS, and Windows for the exact candidate SHA.
- Dedicated release qualification run [35875982066](https://github.com/eggstack/eggchaos/actions/runs/35875982066) passed on the exact candidate. The strict oracle differential, expanded fuzz, release smoke, Eggfetch qualification, and artifact smoke steps all succeeded.
- All five release artifacts were built and uploaded by run `35875982066`. Downloaded artifacts were checked against their included `.sha256` files:

| Target | SHA-256 |
| --- | --- |
| `x86_64-apple-darwin` | `05130a6b2dc47a8dd27b5ef2b5e39a8e7ca7e0d48ba7c9b821ec1c186c2734c9` |
| `x86_64-unknown-linux-gnu` | `b2ed1beb7f221f387d8d9154f19f75110324d4907c3904fc068a0675273aafda` |
| `aarch64-apple-darwin` | `a332bd0ae4f92440121cb750f047ab317b55a595f2b74b9b7d6330592696c2bc` |
| `aarch64-unknown-linux-gnu` | `f895925530625f939edc0f05784eff8e9832166a395a1d0f768819124411f2c3` |
| `x86_64-pc-windows-msvc` | `66d134a0f4f4fa21d4f9dc552d55cb26481daae8bf62fdf124bf56e23cbe286e` |

The foreign targets are build/checksum qualified; only the host binary received local runtime smoke. No cross-target runtime claim is made.

## Follow-on state

All declared M019 acceptance criteria are met, with no unresolved corrective work. There is no successor milestone to unblock. M019 is the final current pre-tag authority; the owner may proceed with tagging and publication as separate release actions. This closure record is committed after the candidate and does not change the candidate SHA used for qualification.
