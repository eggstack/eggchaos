# Security policy

## Supported versions

Security fixes are provided on the latest published release line.

| Line    | Status                                              |
| ------- | --------------------------------------------------- |
| v0.2.x  | Supported (current published release line; `v0.2.0` published 2026-10-02) |
| v0.1.x  | Unsupported since `v0.2.0` publication               |
| < v0.1  | Unsupported                                         |

The `main` branch is post-release development after `v0.2.0`, not a
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

## Scope notes

Out-of-scope reports include: the documented developer-mode behavior
of the Toxiproxy differential harness without a verified oracle
(`differential:incomplete` is reported, never a pass), platform
RST-vs-FIN observation differences already qualified as
platform-dependent, and dependency advisories already covered by the
repository's `cargo-deny` policy and scheduled audit workflow.
