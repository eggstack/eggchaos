# ADR 002 — Determinism and Live Mutation Semantics

Status: accepted  
Date: 2026-09-22

## Context

Randomized fault injection is only valuable in CI when a failure can be reproduced. A single process-global RNG is not sufficient because concurrent Tokio tasks consume values in scheduler-dependent order.

At the same time, a Toxiproxy-like control plane needs to update faults while connections are active. Replacing a pipeline carelessly can lose bytes that a prior generation already accepted.

These concerns must be designed together because live updates can otherwise perturb random draw order and destroy replayability.

## Decision: versioned deterministic substreams

Eggchaos defines a versioned deterministic RNG contract.

Each probabilistic fault instance receives an independent substream derived from stable dimensions:

```text
run_seed
proxy_identity
connection_key
direction
fault_identity
rng_version
```

For the standalone daemon, `connection_key` is at least the proxy-local monotonic accept ordinal and is included in evidence output. Embedded callers may optionally supply a stable application connection key when they need reproduction independent of accept ordering.

No random draw is shared across fault instances or directions.

## RNG algorithm

M002 must freeze an explicit algorithm and commit golden vectors.

The preferred initial algorithm is a tiny, documented non-cryptographic SplitMix64-v1 implementation for seed mixing and random values. This avoids depending on undocumented stability of a library RNG across upgrades.

If implementation selects another algorithm, this ADR must be amended before M002 closes. Requirements remain:

- deterministic from a 64-bit seed;
- no scheduler-order dependence between faults;
- documented conversion to integer ranges/probabilities;
- golden vectors;
- algorithm/version included in evidence metadata;
- never used for security decisions.

## Probability semantics

A fault's `probability` / Toxiproxy `toxicity` is selected per connection by default.

A value of 0 never activates the fault. A value of 1 always activates it. Intermediate probabilities make one deterministic Bernoulli choice from that fault's connection substream.

Faults that intentionally define per-segment randomness, such as slice-size jitter, use subsequent draws from the same fault-local substream.

This distinction must be explicit in public docs so users know whether a probability means “per connection” or “per segment.”

## Configuration generations

Each proxy has a monotonically increasing configuration generation.

Each active connection records:

- generation accepted under;
- current generation observed;
- active fault IDs;
- RNG version;
- run seed fingerprint / safe seed representation according to config policy;
- deterministic connection key;
- any live transitions applied.

The registry publishes immutable plans by generation.

## Mutation classes

Fault updates fall into three classes.

### Parameter-live

Can safely change a parameter without reconstructing buffered state, for example a future bandwidth rate change if token state is preserved.

The runtime updates an atomically shared parameter handle and records the generation.

### Barrier-transition

Requires the current fault state to reach a safe boundary before the new generation applies.

Examples include changing latency queue policy, slice configuration, or ordered fault structure when buffered bytes exist.

The old generation remains responsible for already accepted bytes. New bytes stop entering it at the transition barrier. After its preserving buffers drain—or are deliberately discarded by a documented destructive fault—the new generation takes over.

### Connection-restart

Cannot be represented safely on an existing connection. A proxy listen/upstream change falls in this class and restarts the affected listener/connections according to documented server semantics.

Native API responses should state which class a requested update used.

## Structural fault add/remove

M003 may initially apply structural add/remove only to new connections. M005 is responsible for active structural mutation.

The M005 implementation must not “swap an Arc and forget old state” if the old state owns accepted bytes.

A safe implementation can use a per-direction generation machine with:

```text
Active(old)
  -> Quiescing(old, pending_new)
  -> Active(new)
```

or another design with equivalent evidence.

## Toxiproxy compatibility

Toxiproxy allows active toxic updates and warns custom toxic authors to flush in-flight bytes on interruption.

Eggchaos compatibility aims at the externally relevant property: updates must not accidentally remove bytes from preserving toxics. It does not need to reproduce the internal Go interrupt/channel lifecycle.

Any observable incompatibility discovered by differential tests must be recorded in `plans/reference/toxiproxy-parity.md` and must block an unqualified parity claim.

## Scenario replay

A scenario execution record should include enough information to replay decisions:

- scenario/config hash;
- run seed and RNG version;
- proxy/fault stable IDs;
- connection keys;
- generation transitions with monotonic timestamps relative to scenario start;
- operator-triggered kills/resets;
- termination outcomes;
- intentionally discarded byte counts.

Absolute wall-clock timestamps may be included for diagnostics but are not the deterministic schedule authority.

## Rejected alternatives

### One global seeded RNG

Rejected because task scheduling changes draw order.

### Snapshot forever at connection accept

Acceptable only as the temporary M003 baseline. Rejected as final behavior because the control plane needs useful active mutation and Toxiproxy compatibility.

### Immediate structural swap

Rejected unless the implementation proves it preserves or intentionally accounts for all already accepted bytes.

### Randomness from operating-system entropy for each event

Rejected for normal chaos decisions because failures cannot be reproduced. A CLI may generate a run seed when the user omits one, but that generated seed must be surfaced and recorded before faults execute.
