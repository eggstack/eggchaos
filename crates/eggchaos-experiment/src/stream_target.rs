//! In-process stream policy target over M029 `LivePolicy` pairs.
//!
//! One logical experiment resource name maps to an upstream/downstream
//! [`LivePolicy`] pair — typically
//! `dialer.upstream_policy()`/`dialer.downstream_policy()` from an
//! M029 chaos adapter, so stream Scenario V2 publications reach the
//! same authorities already-open pooled physical connections observe.
//! Stream Scenario V2 set/remove actions work through expected
//! generation; stable schedule namespaces reach the physical
//! connection engines on live transition. Datagram actions return
//! typed unsupported failures; they are never translated into stream
//! behavior. The target creates no dial path and owns no
//! HTTP/TLS/pooling.

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    RwLock,
};

use eggchaos_core::{FaultPlan, LivePolicy};

use crate::run::{ScheduleResource, ScheduleTransport};
use crate::target::{
    PolicyTarget, PublishReceipt, TargetCapabilities, TargetError, TargetPlan, TargetSnapshot,
    MAX_RESOURCE_NAME_BYTES,
};

/// Named stream resource: the two live policy authorities for one
/// logical experiment identity.
#[derive(Debug, Clone)]
struct StreamResource {
    upstream: LivePolicy,
    downstream: LivePolicy,
}

/// Consumer-neutral expected-generation target over in-process stream
/// policies. Stream-only: datagram actions fail explicitly.
#[derive(Debug, Default)]
pub struct StreamPolicyTarget {
    resources: RwLock<HashMap<String, StreamResource>>,
    global: AtomicU64,
}

impl Clone for StreamPolicyTarget {
    fn clone(&self) -> Self {
        Self {
            resources: RwLock::new(self.resources.read().expect("stream target lock").clone()),
            global: AtomicU64::new(self.global.load(Ordering::Acquire)),
        }
    }
}

impl StreamPolicyTarget {
    /// Create an empty target.
    pub fn new() -> Self {
        Self::default()
    }

    /// Register one logical resource name with its upstream/downstream
    /// policy pair. Re-registering a name replaces its pair.
    ///
    /// # Errors
    ///
    /// Returns [`TargetError::Validation`] when the name is empty or
    /// exceeds [`MAX_RESOURCE_NAME_BYTES`] bytes.
    pub fn register(
        &self,
        name: impl Into<String>,
        upstream: LivePolicy,
        downstream: LivePolicy,
    ) -> Result<(), TargetError> {
        let name = name.into();
        if name.is_empty() || name.len() > MAX_RESOURCE_NAME_BYTES {
            return Err(TargetError::invalid(format!(
                "resource name must be 1..={MAX_RESOURCE_NAME_BYTES} bytes"
            )));
        }
        self.resources.write().expect("stream target lock").insert(
            name,
            StreamResource {
                upstream,
                downstream,
            },
        );
        Ok(())
    }

    /// Registered resource names in sorted order.
    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self
            .resources
            .read()
            .expect("stream target lock")
            .keys()
            .cloned()
            .collect();
        names.sort();
        names
    }

    fn lookup(&self, resource: &ScheduleResource) -> Result<StreamResource, TargetError> {
        if resource.transport != ScheduleTransport::Stream {
            return Err(TargetError::unsupported(
                "stream-only target cannot apply datagram actions",
            ));
        }
        self.resources
            .read()
            .expect("stream target lock")
            .get(&resource.proxy)
            .cloned()
            .ok_or_else(|| {
                TargetError::missing(format!("stream resource {} not found", resource.proxy))
            })
    }

    fn directional(resource: &StreamResource, direction: eggchaos_core::Direction) -> LivePolicy {
        match direction {
            eggchaos_core::Direction::Upstream => resource.upstream.clone(),
            eggchaos_core::Direction::Downstream => resource.downstream.clone(),
        }
    }
}

impl PolicyTarget for StreamPolicyTarget {
    fn capabilities(&self) -> TargetCapabilities {
        TargetCapabilities {
            stream: true,
            datagram: false,
        }
    }

    async fn snapshot(&self, resource: &ScheduleResource) -> Result<TargetSnapshot, TargetError> {
        let entry = self.lookup(resource)?;
        let policy = Self::directional(&entry, resource.direction);
        let snapshot = policy.snapshot();
        Ok(TargetSnapshot {
            generation: snapshot.generation,
            plan: TargetPlan::Stream((*snapshot.plan).clone()),
        })
    }

    async fn current_generation(&self, resource: &ScheduleResource) -> Result<u64, TargetError> {
        let entry = self.lookup(resource)?;
        Ok(Self::directional(&entry, resource.direction).generation())
    }

    async fn publish(
        &self,
        resource: &ScheduleResource,
        expected: u64,
        plan: TargetPlan,
        seed_namespace: u64,
    ) -> Result<PublishReceipt, TargetError> {
        let entry = self.lookup(resource)?;
        let TargetPlan::Stream(plan) = plan else {
            return Err(TargetError::unsupported(
                "stream-only target cannot publish datagram plans",
            ));
        };
        let policy = Self::directional(&entry, resource.direction);
        match policy.publish_expected(plan, seed_namespace, expected) {
            Ok(snapshot) => {
                self.global.fetch_add(1, Ordering::AcqRel);
                Ok(PublishReceipt {
                    generation: snapshot.generation,
                })
            }
            Err(eggchaos_core::PublishError::Invalid(error)) => {
                Err(TargetError::invalid(error.to_string()))
            }
            Err(eggchaos_core::PublishError::Conflict(conflict)) => {
                Err(TargetError::GenerationConflict {
                    expected: conflict.expected,
                    found: conflict.found,
                })
            }
        }
    }

    fn global_generation(&self) -> u64 {
        self.global.load(Ordering::Acquire)
    }
}

/// Build a one-resource stream target directly from an M029-style
/// policy pair without importing the EggFetch adapter crate.
pub fn stream_target_from_policies(
    name: impl Into<String>,
    upstream: LivePolicy,
    downstream: LivePolicy,
) -> Result<StreamPolicyTarget, TargetError> {
    let target = StreamPolicyTarget::new();
    target.register(name, upstream, downstream)?;
    Ok(target)
}

/// Empty stream plan helper for driver-equivalence fixtures.
pub fn empty_stream_plan() -> FaultPlan {
    FaultPlan::empty()
}
