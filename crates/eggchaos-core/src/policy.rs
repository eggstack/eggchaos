use std::sync::atomic::{AtomicU64, Ordering};

use arc_swap::ArcSwap;

use crate::FaultPlan;

/// A read-mostly generation-published fault policy.
#[derive(Clone)]
pub struct LivePolicy {
    plan: std::sync::Arc<ArcSwap<FaultPlan>>,
    generation: std::sync::Arc<AtomicU64>,
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
        Self::new(FaultPlan::empty())
    }
}

impl LivePolicy {
    /// Create a policy at generation one.
    pub fn new(plan: FaultPlan) -> Self {
        Self {
            plan: std::sync::Arc::new(ArcSwap::from_pointee(plan)),
            generation: std::sync::Arc::new(AtomicU64::new(1)),
        }
    }
    /// Return the current immutable plan snapshot.
    pub fn snapshot(&self) -> FaultPlan {
        (*self.plan.load_full()).clone()
    }
    /// Return the current generation.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }
    /// Publish a complete validated plan as exactly one next generation.
    pub fn publish(&self, plan: FaultPlan) -> Result<u64, crate::ValidationError> {
        plan.validate()?;
        self.plan.store(std::sync::Arc::new(plan));
        Ok(self.generation.fetch_add(1, Ordering::AcqRel) + 1)
    }
}
