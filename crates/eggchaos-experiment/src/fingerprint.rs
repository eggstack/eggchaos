//! Canonical, versioned SHA-256 schedule fingerprint.
//!
//! The fingerprint is the portable replay identity for a scenario v2
//! schedule. It is computed over a frozen semantic encoding of the
//! compiled tape — not over the raw source text — so equivalent JSON
//! or TOML sources with different whitespace, ordering, or comments
//! produce the same digest.
//!
//! Domain/version prefix bytes are written verbatim into the SHA-256
//! input before the schedule payload, so a collision across
//! `compiler_semantics_version` values would be deliberate rather than
//! accidental.

use sha2::{Digest, Sha256};

use crate::ScenarioAction;

use super::compiler::{
    CompiledEventV2, CompiledPhaseIdentity, CompiledScenarioV2, COMPILER_SEMANTICS_VERSION,
};

/// Domain/version prefix written into every fingerprint input.
const FINGERPRINT_DOMAIN: &[u8] = b"eggchaos/scenario-v2/fingerprint/v1/compiler-semantics=";

/// Compute the canonical SHA-256 fingerprint of a compiled scenario.
///
/// Properties pinned by this function:
///
/// 1. SHA-256 over the exact bytes produced by [`encode_compiled_for_fingerprint`].
/// 2. Domain/version prefix makes the digest invalid for any other
///    compiler semantics version or algorithm version.
/// 3. Pure: same compiled tape -> byte-identical digest. No allocator,
///    hashmap, or `Instant` participation.
///
/// The encoded payload includes every field that changes execution:
/// isolation/cleanup, ordered `(compiled_index, phase, offset_ns, action)`
/// tuples for every event. Optional phase names are deliberately
/// excluded because they are presentation-only labels.
pub fn compiled_fingerprint(compiled: &CompiledScenarioV2) -> [u8; 32] {
    let bytes = encode_compiled_for_fingerprint(compiled);
    let mut hasher = Sha256::new();
    hasher.update(FINGERPRINT_DOMAIN);
    hasher.update(COMPILER_SEMANTICS_VERSION.to_be_bytes());
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// Hex-encode a fingerprint digest (lowercase, 64 chars).
pub fn fingerprint_hex(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

/// Encode the canonical schedule payload that participates in the
/// fingerprint. The encoding is intentionally explicit instead of a
/// Serde map dump: it pins byte ordering and excludes non-execution
/// presentation fields (`phase.name`, default values, etc.).
pub fn encode_compiled_for_fingerprint(compiled: &CompiledScenarioV2) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"isolation=");
    write_isolation(&mut out, compiled.isolation);
    out.extend_from_slice(b"\ncleanup=");
    write_cleanup(&mut out, compiled.cleanup);
    out.extend_from_slice(b"\nseed=");
    out.extend_from_slice(compiled.seed.to_string().as_bytes());
    out.extend_from_slice(b"\nexecution_key=");
    out.extend_from_slice(compiled.execution_key.to_string().as_bytes());
    out.extend_from_slice(b"\nevent_count=");
    out.extend_from_slice(compiled.events.len().to_string().as_bytes());
    for event in &compiled.events {
        write_event(&mut out, event);
    }
    out
}

fn write_isolation(out: &mut Vec<u8>, isolation: super::source::IsolationPolicyV2) {
    let s = match isolation {
        super::source::IsolationPolicyV2::Strict => "strict",
        super::source::IsolationPolicyV2::Live => "live",
    };
    out.extend_from_slice(s.as_bytes());
}

fn write_cleanup(out: &mut Vec<u8>, cleanup: super::source::CleanupPolicyV2) {
    let s = match cleanup {
        super::source::CleanupPolicyV2::RestoreInitial => "restore-initial",
        super::source::CleanupPolicyV2::Leave => "leave",
    };
    out.extend_from_slice(s.as_bytes());
}

fn write_phase(out: &mut Vec<u8>, phase: CompiledPhaseIdentity) {
    match phase {
        CompiledPhaseIdentity::Top { index } => {
            out.extend_from_slice(b"top/");
            out.extend_from_slice(index.to_string().as_bytes());
        }
        CompiledPhaseIdentity::Repeat { iteration, index } => {
            out.extend_from_slice(b"repeat/");
            out.extend_from_slice(iteration.to_string().as_bytes());
            out.extend_from_slice(b"/");
            out.extend_from_slice(index.to_string().as_bytes());
        }
    }
}

fn write_action(out: &mut Vec<u8>, action: &ScenarioAction) {
    match action {
        ScenarioAction::SetPlan {
            proxy,
            direction,
            faults,
        } => {
            out.extend_from_slice(b"set-plan;proxy=");
            out.extend_from_slice(proxy.as_bytes());
            out.extend_from_slice(b";direction=");
            out.extend_from_slice(direction.as_str().as_bytes());
            out.extend_from_slice(b";faults=");
            for fault in faults {
                out.extend_from_slice(fault.id.as_str().as_bytes());
                out.extend_from_slice(b"~");
                out.extend_from_slice(format!("{:.17}", fault.probability.get()).as_bytes());
                out.extend_from_slice(b"~");
                write_stream_fault_kind(out, &fault.kind);
                out.extend_from_slice(b"|");
            }
        }
        ScenarioAction::RemoveFault {
            proxy,
            direction,
            id,
        } => {
            out.extend_from_slice(b"remove-fault;proxy=");
            out.extend_from_slice(proxy.as_bytes());
            out.extend_from_slice(b";direction=");
            out.extend_from_slice(direction.as_str().as_bytes());
            out.extend_from_slice(b";id=");
            out.extend_from_slice(id.as_bytes());
        }
        ScenarioAction::SetDatagramPlan {
            proxy,
            direction,
            faults,
        } => {
            out.extend_from_slice(b"set-datagram-plan;proxy=");
            out.extend_from_slice(proxy.as_bytes());
            out.extend_from_slice(b";direction=");
            out.extend_from_slice(direction.as_str().as_bytes());
            out.extend_from_slice(b";faults=");
            for fault in faults {
                out.extend_from_slice(fault.id.as_str().as_bytes());
                out.extend_from_slice(b"~");
                out.extend_from_slice(format!("{:.17}", fault.probability.get()).as_bytes());
                out.extend_from_slice(b"~");
                write_datagram_fault_kind(out, &fault.kind);
                out.extend_from_slice(b"|");
            }
        }
        ScenarioAction::RemoveDatagramFault {
            proxy,
            direction,
            id,
        } => {
            out.extend_from_slice(b"remove-datagram-fault;proxy=");
            out.extend_from_slice(proxy.as_bytes());
            out.extend_from_slice(b";direction=");
            out.extend_from_slice(direction.as_str().as_bytes());
            out.extend_from_slice(b";id=");
            out.extend_from_slice(id.as_bytes());
        }
    }
}

fn ns(duration: std::time::Duration) -> u128 {
    duration.as_nanos()
}

fn write_stream_fault_kind(out: &mut Vec<u8>, kind: &eggchaos_core::FaultKind) {
    use eggchaos_core::FaultKind;
    match kind {
        FaultKind::Latency(c) => {
            out.extend_from_slice(b"latency;delay_ns=");
            out.extend_from_slice(ns(c.delay).to_string().as_bytes());
            out.extend_from_slice(b";jitter_ns=");
            out.extend_from_slice(ns(c.jitter).to_string().as_bytes());
            out.extend_from_slice(b";max_buffer_bytes=");
            out.extend_from_slice(c.max_buffer_bytes.get().to_string().as_bytes());
        }
        FaultKind::Bandwidth(c) => {
            out.extend_from_slice(b"bandwidth;bytes_per_second=");
            out.extend_from_slice(c.bytes_per_second.get().to_string().as_bytes());
            out.extend_from_slice(b";burst_bytes=");
            out.extend_from_slice(c.burst_bytes.get().to_string().as_bytes());
        }
        FaultKind::Blackhole(c) => {
            out.extend_from_slice(b"blackhole;close_after_ns=");
            match c.close_after {
                Some(d) => out.extend_from_slice(ns(d).to_string().as_bytes()),
                None => out.extend_from_slice(b"<none>"),
            };
        }
        FaultKind::LimitData(c) => {
            out.extend_from_slice(b"limit-data;bytes=");
            out.extend_from_slice(c.bytes.get().to_string().as_bytes());
        }
        FaultKind::SlowClose(c) => {
            out.extend_from_slice(b"slow-close;delay_ns=");
            out.extend_from_slice(ns(c.delay).to_string().as_bytes());
        }
        FaultKind::Slice(c) => {
            out.extend_from_slice(b"slice;average_size=");
            out.extend_from_slice(c.average_size.get().to_string().as_bytes());
            out.extend_from_slice(b";variation=");
            out.extend_from_slice(c.variation.to_string().as_bytes());
            out.extend_from_slice(b";delay_ns=");
            out.extend_from_slice(ns(c.delay).to_string().as_bytes());
        }
        FaultKind::Disconnect(c) => {
            out.extend_from_slice(b"disconnect;after_ns=");
            out.extend_from_slice(ns(c.after).to_string().as_bytes());
            out.extend_from_slice(b";hard_reset=");
            out.extend_from_slice(if c.hard_reset { b"true" } else { b"false" });
        }
        FaultKind::StreamLoss(c) => {
            // M036 compile propagation of the ADR 007 primitive; M037 owns
            // the Scenario-facing fixtures. Probabilities render with the
            // same fixed-precision float format as connection probability.
            out.extend_from_slice(b"stream-loss;loss_rate=");
            out.extend_from_slice(format!("{:.17}", c.loss_rate.get()).as_bytes());
            out.extend_from_slice(b";correlation=");
            out.extend_from_slice(format!("{:.17}", c.correlation.get()).as_bytes());
        }
    }
}

fn write_datagram_fault_kind(out: &mut Vec<u8>, kind: &eggchaos_core::DatagramFaultKind) {
    use eggchaos_core::DatagramFaultKind;
    match kind {
        DatagramFaultKind::Delay { delay, jitter } => {
            out.extend_from_slice(b"delay;delay_ns=");
            out.extend_from_slice(ns(*delay).to_string().as_bytes());
            out.extend_from_slice(b";jitter_ns=");
            out.extend_from_slice(ns(*jitter).to_string().as_bytes());
        }
        DatagramFaultKind::Loss => out.extend_from_slice(b"loss"),
        DatagramFaultKind::Duplicate { additional_copies } => {
            out.extend_from_slice(b"duplicate;additional_copies=");
            out.extend_from_slice(additional_copies.to_string().as_bytes());
        }
        DatagramFaultKind::Reorder { hold } => {
            out.extend_from_slice(b"reorder;hold_ns=");
            out.extend_from_slice(ns(*hold).to_string().as_bytes());
        }
        DatagramFaultKind::PayloadCorrupt { bytes } => {
            out.extend_from_slice(b"payload-corrupt;bytes=");
            out.extend_from_slice(bytes.get().to_string().as_bytes());
        }
        DatagramFaultKind::Bandwidth {
            bytes_per_second,
            burst_bytes,
        } => {
            out.extend_from_slice(b"bandwidth;bytes_per_second=");
            out.extend_from_slice(bytes_per_second.get().to_string().as_bytes());
            out.extend_from_slice(b";burst_bytes=");
            out.extend_from_slice(burst_bytes.get().to_string().as_bytes());
        }
    }
}

fn write_event(out: &mut Vec<u8>, event: &CompiledEventV2) {
    out.extend_from_slice(b"\nevent[");
    out.extend_from_slice(event.compiled_index.to_string().as_bytes());
    out.extend_from_slice(b"] phase=");
    write_phase(out, event.phase);
    out.extend_from_slice(b" offset_ns=");
    out.extend_from_slice(event.offset_ns.to_string().as_bytes());
    out.extend_from_slice(b" action=");
    write_action(out, &event.action);
}
