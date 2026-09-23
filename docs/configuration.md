# Configuration

Native configuration is schema-versioned TOML. The canonical runtime model
is the typed `eggchaos-core::FaultPlan`; adapters must convert into it rather
than implementing their own validation.

The file/API surface is the schema-v1 TOML model plus the versioned native
control API completed under M004 (closed) and requalified through the
M009–M013 corrective chain. The supported native
contract is the Rust constructors in `eggchaos-core`: fault IDs are bounded,
probabilities are finite and within `0..=1`, slice variation is smaller than
average size, and required capacities/rates are non-zero.

The schema-v1 compiler validates required non-zero values and reports the
field name in a configuration error. Invalid values such as a zero bandwidth
burst are rejected without panicking. Admin bearer tokens remain accepted as
strings in TOML, but native configuration and admin debug formatting always
redact their values.

The `Direction` names are `upstream` (client to target) and `downstream`
(target to client). A plan is ordered, and a connection-local fault activation
decision is derived from the policy seed namespace, proxy identity, connection key,
direction, and fault identity. No process-global RNG is used. Each published
policy generation carries its own seed namespace: manual updates retain the
current namespace, while scenario runs publish namespaces derived from
`(scenario seed, run id, event index)`; see `docs/control-plane.md` for the
replay limits this implies.
