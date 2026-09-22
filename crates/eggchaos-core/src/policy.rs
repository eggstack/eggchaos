use std::sync::Arc;

use arc_swap::ArcSwap;

use crate::FaultPlan;

/// One immutable published policy snapshot: plan, generation, and seed
/// namespace move together. Readers load the whole snapshot with a single
/// atomic operation, so a generation value can never disagree with the
/// plan it was published with.
#[derive(Debug, Clone, PartialEq)]
pub struct PublishedPolicy {
    /// Policy generation, starting at one and incrementing once per
    /// successful publication.
    pub generation: u64,
    /// Complete validated fault plan for this generation.
    pub plan: Arc<FaultPlan>,
    /// Seed namespace feeding fault-local RNG compilation for connections
    /// observing this generation. Manual updates retain the current
    /// namespace; scenario runs publish derived namespaces.
    pub seed_namespace: u64,
}

/// A read-mostly generation-published fault policy.
#[derive(Clone)]
pub struct LivePolicy {
    current: Arc<ArcSwap<PublishedPolicy>>,
}

impl std::fmt::Debug for LivePolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LivePolicy")
            .field("generation", &self.generation())
            .finish()
    }
}

impl Default for LivePolicy {
    fn default() -> Self {
        Self::new(FaultPlan::empty(), 0)
    }
}

/// Stale-base publication conflict for compare-and-swap publishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyConflict {
    /// Generation the publisher based its plan on.
    pub expected: u64,
    /// Generation actually current when publishing.
    pub found: u64,
}

/// Failure to publish an expected generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishError {
    /// The replacement plan is invalid; nothing was published.
    Invalid(crate::ValidationError),
    /// The base generation moved; nothing was published.
    Conflict(PolicyConflict),
}

impl LivePolicy {
    /// Create a policy at generation one.
    pub fn new(plan: FaultPlan, seed_namespace: u64) -> Self {
        Self {
            current: Arc::new(ArcSwap::new(Arc::new(PublishedPolicy {
                generation: 1,
                plan: Arc::new(plan),
                seed_namespace,
            }))),
        }
    }
    /// Return the current immutable snapshot: plan, generation, and seed
    /// namespace from one atomic load.
    pub fn snapshot(&self) -> Arc<PublishedPolicy> {
        self.current.load_full()
    }
    /// Return the current plan alone (convenience over `snapshot`).
    pub fn plan(&self) -> FaultPlan {
        (*self.snapshot().plan).clone()
    }
    /// Return the current generation.
    pub fn generation(&self) -> u64 {
        self.snapshot().generation
    }
    /// Return the current seed namespace.
    pub fn seed_namespace(&self) -> u64 {
        self.snapshot().seed_namespace
    }
    /// Publish a complete validated plan as exactly one next generation.
    pub fn publish(
        &self,
        plan: FaultPlan,
        seed_namespace: u64,
    ) -> Result<Arc<PublishedPolicy>, crate::ValidationError> {
        plan.validate()?;
        let next = Arc::new(PublishedPolicy {
            generation: self.generation() + 1,
            plan: Arc::new(plan),
            seed_namespace,
        });
        self.current.store(next.clone());
        Ok(next)
    }
    /// Publish only when the current generation still equals `expected`;
    /// otherwise report the conflict without changing state. Scenario
    /// events use this so a concurrent manual publication fails fast
    /// instead of being silently overwritten by a stale base.
    pub fn publish_expected(
        &self,
        plan: FaultPlan,
        seed_namespace: u64,
        expected: u64,
    ) -> Result<Arc<PublishedPolicy>, PublishError> {
        plan.validate().map_err(PublishError::Invalid)?;
        loop {
            let current = self.snapshot();
            if current.generation != expected {
                return Err(PublishError::Conflict(PolicyConflict {
                    expected,
                    found: current.generation,
                }));
            }
            let next = Arc::new(PublishedPolicy {
                generation: current.generation + 1,
                plan: Arc::new(plan.clone()),
                seed_namespace,
            });
            // The returned guard holds the previous value; pointer
            // equality with our base proves the swap happened.
            let previous = self.current.compare_and_swap(&current, next.clone());
            if Arc::ptr_eq(&previous, &current) {
                return Ok(next);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_loads_plan_and_generation_together() {
        let policy = LivePolicy::new(FaultPlan::empty(), 9);
        let first = policy.snapshot();
        assert_eq!(first.generation, 1);
        assert_eq!(first.seed_namespace, 9);
        let second = policy.publish(FaultPlan::empty(), 9).unwrap();
        assert_eq!(second.generation, 2);
        // The first snapshot is immutable: it still reads generation one.
        assert_eq!(first.generation, 1);
        assert_eq!(policy.generation(), 2);
    }

    #[test]
    fn expected_publish_conflicts_on_stale_base() {
        let policy = LivePolicy::new(FaultPlan::empty(), 0);
        let base = policy.snapshot();
        policy.publish(FaultPlan::empty(), 0).unwrap();
        let conflict = policy
            .publish_expected(FaultPlan::empty(), 0, base.generation)
            .unwrap_err();
        assert_eq!(
            conflict,
            PublishError::Conflict(PolicyConflict {
                expected: 1,
                found: 2
            })
        );
        // A fresh base succeeds.
        let next = policy.publish_expected(FaultPlan::empty(), 0, 2).unwrap();
        assert_eq!(next.generation, 3);
    }
}
