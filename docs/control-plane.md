# Native control plane

The native API is versioned under `/v1` and defaults to loopback. A
non-loopback bind requires both an explicit public-admin opt-in and a bearer
token; failed authentication returns a bounded JSON error without echoing the
token. Request bodies are capped at 1 MiB and route state is changed through
`ControlState` generation publication.

The EggServe leaf H1 runtime owns parsing, request-body bounds, and connection
lifecycle. Eggchaos owns only route dispatch and typed JSON conversion. The
CLI uses Eggfetch for control requests, including JSON mode and nonzero error
exit status.

Fault updates are generation transitions. Existing streams drain bytes already
owned by the old generation before observing a newly published `LivePolicy`.
The transition does not capture payloads.
