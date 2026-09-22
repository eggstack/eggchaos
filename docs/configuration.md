# Configuration

Native configuration is schema-versioned TOML. The canonical runtime model
is the typed `eggchaos-core::FaultPlan`; adapters must convert into it rather
than implementing their own validation.

The file/API surface is completed by M004. Until then, the supported native
contract is the Rust constructors in `eggchaos-core`: fault IDs are bounded,
probabilities are finite and within `0..=1`, slice variation is smaller than
average size, and required capacities/rates are non-zero.

The `Direction` names are `upstream` (client to target) and `downstream`
(target to client). A plan is ordered, and a connection-local fault activation
decision is derived from the policy seed namespace, proxy identity, connection key,
direction, and fault identity. No process-global RNG is used. Each published
policy generation carries its own seed namespace: manual updates retain the
current namespace, while scenario runs publish namespaces derived from
`(scenario seed, run id, event index)`; see `docs/control-plane.md` for the
replay limits this implies.
