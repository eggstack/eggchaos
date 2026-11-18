# Security policy

## Supported versions

Security fixes are provided on the latest published release line.

| Line    | Status                                              |
| ------- | --------------------------------------------------- |
| v0.2.x  | Supported (current published release line; `v0.2.1` published 2026-11-13) |
| v0.1.x  | Unsupported since `v0.2.0` publication               |
| < v0.1  | Unsupported                                         |

The `main` branch is post-release development after `v0.2.1`, not a
supported release line itself. If you deploy from `main`, track it as
unreleased software. Adopting a multi-line support policy for older
release branches is not currently part of this repository's security
posture.

## Reporting a vulnerability

**Do not open a public issue for a suspected vulnerability.**
Do not publish credentials, exploit payloads, private data, or
sensitive deployment details in any public issue, discussion, or PR.

Report privately through GitHub private vulnerability reporting:
the repository **Security** tab → **Report a vulnerability**.
That opens a private advisory visible only to the reporters and the
maintainers, where a fix and disclosure can be coordinated.

### What to include

- Affected component and version (crate name, binary, or binding).
- How the issue was found and the smallest reproduction you can share
  privately (configuration, commands, logs with secrets redacted).
- Impact you were able to confirm (and what you did not test).
- Any workaround you found, if applicable.

### What happens next

Maintainers acknowledge private reports, assess impact against the
supported lines above, and coordinate a fix and release before any
public disclosure. If private vulnerability reporting is ever
unavailable on this repository, open a minimal public issue that
describes only the affected area and asks for a private contact —
never the vulnerability details themselves.

## Repository safeguards and dependency triage

The repository requires pull requests for `main`, current required CI and
dependency-review checks, linear history, and resolved review conversations;
force-push and branch deletion are blocked. A `v*.*.*` release-tag ruleset
blocks creation, updates, and deletion except for the designated release
maintainer. The repository currently has one returned admin collaborator, so
an independent approval is not required or claimed. The maintainer's `main`
bypass is limited to pull requests; the tag bypass is limited to the release
maintainer.

Dependabot vulnerability alerts and security updates are enabled. Secret
scanning and push protection are enabled. The GitHub API readback still reports
non-provider pattern scanning and validity checks as disabled after a
repository-settings update request. The organization API reports a `Free` plan;
GitHub documents non-provider pattern scanning for organization-owned
repositories with GitHub Team and Secret Protection, and validity checks for
GitHub Team or Enterprise accounts. These controls are not represented as
active. See [GitHub's non-provider pattern availability](https://docs.github.com/en/code-security/secret-scanning/using-advanced-secret-scanning-and-push-protection-features/non-provider-patterns)
and [supported secret-scanning patterns](https://docs.github.com/en/code-security/reference/secret-security/supported-secret-scanning-patterns).

A pinned CodeQL workflow has been added to the current candidate, but has no
hosted run or code-scanning result yet; do not treat CodeQL analysis as active
until it runs successfully on the exact candidate SHA. The repository's
current active rulesets and security settings can be checked with
`scripts/check_repository_security_state.py` after that workflow is present on
GitHub.

Triage every open dependency alert against the named manifest and committed
lockfile. Apply a compatible fix when available. If the dependency cannot be
reproduced from the lockfile, keep the alert open while recording the evidence
and investigating the GitHub alert metadata; do not dismiss an unreproduced
alert merely to clear the count. Rust advisories are additionally checked
against all four Cargo lockfiles by the scheduled audit workflow. The PR
dependency-review threshold remains `high`; Python clients are stdlib-only,
Python build/test tooling is exactly pinned, and the TypeScript client has no
runtime dependencies.

## Scope notes

Out-of-scope reports include: the documented developer-mode behavior
of the Toxiproxy differential harness without a verified oracle
(`differential:incomplete` is reported, never a pass), platform
RST-vs-FIN observation differences already qualified as
platform-dependent, and dependency advisories already covered by the
repository's `cargo-deny` policy and scheduled audit workflow.
