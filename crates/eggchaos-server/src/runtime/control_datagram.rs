//! Datagram-related `ControlState` methods.
//!
//! M052 splits the historically large `runtime/control.rs` impl block
//! by responsibility. This file owns every datagram proxy, datagram
//! fault, datagram plan, and datagram association method on
//! [`ControlState`]. The runtime authority is unchanged: each method
//! is a thin shim over `RuntimeInner::datagrams` plus the existing
//! `map_datagram_error` translation. Public surface, error
//! categories, generation semantics, and expected-generation
//! publication rules are byte-identical to the pre-M052 inline
//! implementation; this split is purely textual.
//!
//! See `runtime/control.rs` for the rest of the `ControlState`
//! surface (metrics, stream proxies, plan publication, scenarios,
//! shutdown, reset).

use std::time::Duration;

use eggchaos_core::Direction;

use super::control::map_datagram_error;
use super::{
    ControlError, ControlState, DatagramAssociationSnapshot, DatagramFaultPatch, DatagramProxySpec,
    DatagramProxyView, DatagramRuntimeError,
};

impl ControlState {
    /// Create and bind a fixed-target UDP proxy in the shared runtime authority.
    pub async fn create_datagram_proxy(
        &self,
        proxy: DatagramProxySpec,
    ) -> Result<(DatagramProxyView, u64), ControlError> {
        self.runtime
            .datagrams
            .create_proxy(proxy)
            .await
            .map(|view| (view, self.next_generation()))
            .map_err(map_datagram_error)
    }

    /// List fixed-target UDP proxies.
    pub async fn datagram_proxies(&self) -> Vec<DatagramProxyView> {
        self.runtime.datagrams.proxies().await
    }

    /// Get one fixed-target UDP proxy.
    pub async fn get_datagram_proxy(&self, name: &str) -> Option<DatagramProxyView> {
        self.runtime.datagrams.proxy(name).await
    }

    /// Enable or disable a datagram listener, joining all work when disabled.
    pub async fn set_datagram_proxy_enabled(
        &self,
        name: &str,
        enabled: bool,
    ) -> Result<(DatagramProxyView, u64), ControlError> {
        let view = if enabled {
            self.runtime
                .datagrams
                .enable_proxy(name)
                .await
                .map_err(|error| match error {
                    DatagramRuntimeError::Bind(reason) => ControlError::BindFailed {
                        proxy: name.into(),
                        reason: reason.to_string(),
                    },
                    other => map_datagram_error(other),
                })?
        } else {
            self.runtime
                .datagrams
                .disable_proxy(name)
                .await
                .map_err(map_datagram_error)?;
            self.runtime
                .datagrams
                .proxy(name)
                .await
                .ok_or_else(|| ControlError::NotFound(name.into()))?
        };
        Ok((view, self.next_generation()))
    }

    /// Apply supported datagram listener/target changes with bind-before-swap
    /// behavior for a running listener.
    pub async fn update_datagram_proxy(
        &self,
        name: &str,
        listen: Option<std::net::SocketAddr>,
        upstream: Option<std::net::SocketAddr>,
        max_associations: Option<usize>,
        association_idle_timeout_ms: Option<u64>,
        enabled: Option<bool>,
    ) -> Result<(DatagramProxyView, u64), ControlError> {
        if association_idle_timeout_ms.is_some_and(|timeout| timeout == 0 || timeout > 86_400_000) {
            return Err(ControlError::Invalid(
                "association_idle_timeout_ms must be in 1..=86400000".into(),
            ));
        }
        let view = self
            .runtime
            .datagrams
            .update_proxy(
                name,
                listen,
                upstream,
                max_associations,
                association_idle_timeout_ms.map(Duration::from_millis),
                enabled,
            )
            .await
            .map_err(|error| match error {
                DatagramRuntimeError::Bind(reason) => ControlError::RestartFailed { proxy: name.into(), reason: format!("replacement listener was not bound; existing definition remains active: {reason}") },
                other => map_datagram_error(other),
            })?;
        Ok((view, self.next_generation()))
    }

    /// Delete a fixed-target UDP proxy and join all owned work.
    pub async fn delete_datagram_proxy(&self, name: &str) -> Result<u64, ControlError> {
        if !self
            .runtime
            .datagrams
            .delete_proxy(name)
            .await
            .map_err(map_datagram_error)?
        {
            return Err(ControlError::NotFound(name.to_owned()));
        }
        Ok(self.next_generation())
    }

    /// Read one datagram fault plan snapshot.
    pub async fn get_datagram_plan(
        &self,
        name: &str,
        direction: Direction,
    ) -> Result<(eggchaos_core::DatagramPlan, u64, u64), ControlError> {
        self.runtime
            .datagrams
            .fault_plan(name, direction)
            .await
            .ok_or_else(|| ControlError::NotFound(name.to_owned()))
    }

    /// Publish one datagram plan. Expected generation prevents stale scenario
    /// actions from overwriting a concurrent manual change.
    pub async fn publish_datagram_plan(
        &self,
        name: &str,
        direction: Direction,
        plan: eggchaos_core::DatagramPlan,
        seed_namespace: u64,
        expected_generation: Option<u64>,
    ) -> Result<u64, ControlError> {
        let generation = self
            .runtime
            .datagrams
            .publish_fault_plan(name, direction, plan, seed_namespace, expected_generation)
            .await
            .map_err(map_datagram_error)?;
        self.next_generation();
        Ok(generation)
    }

    /// Add a datagram fault to one direction.
    ///
    /// This is the single HTTP-independent datagram fault mutation
    /// authority shared by the native admin route and `eggchaos-embed`:
    /// same-direction duplicate detection, cross-direction ID uniqueness
    /// (required for path lookup), plan reconstruction, and
    /// generation-guarded publication all live here. Wire DTO conversion
    /// stays with the callers.
    pub async fn add_datagram_fault(
        &self,
        name: &str,
        direction: Direction,
        fault: eggchaos_core::DatagramFaultSpec,
    ) -> Result<(Direction, eggchaos_core::DatagramFaultSpec, u64), ControlError> {
        let (plan, generation, seed) = self.get_datagram_plan(name, direction).await?;
        if plan.faults().iter().any(|existing| existing.id == fault.id) {
            return Err(ControlError::Conflict(
                "datagram fault already exists".into(),
            ));
        }
        let other_direction = match direction {
            Direction::Upstream => Direction::Downstream,
            Direction::Downstream => Direction::Upstream,
        };
        if self
            .get_datagram_plan(name, other_direction)
            .await
            .is_ok_and(|(other, _, _)| {
                other
                    .faults()
                    .iter()
                    .any(|existing| existing.id == fault.id)
            })
        {
            return Err(ControlError::Conflict(
                "datagram fault id must be unique across directions for path lookup".into(),
            ));
        }
        let mut faults = plan.faults().to_vec();
        faults.push(fault.clone());
        let plan = eggchaos_core::DatagramPlan::new(faults)
            .map_err(|error| ControlError::Invalid(error.to_string()))?;
        let next = self
            .publish_datagram_plan(name, direction, plan, seed, Some(generation))
            .await?;
        // A concurrent publish to the opposite direction between the
        // cross-direction check and the guarded publish can still create a
        // duplicate ID across directions. Re-validate after publication and
        // compensate with a removal so the path-lookup invariant holds.
        if self
            .get_datagram_plan(name, other_direction)
            .await
            .is_ok_and(|(other, _, _)| {
                other
                    .faults()
                    .iter()
                    .any(|existing| existing.id == fault.id)
            })
        {
            let (current, current_generation, current_seed) =
                self.get_datagram_plan(name, direction).await?;
            let pruned: Vec<_> = current
                .faults()
                .iter()
                .filter(|existing| existing.id != fault.id)
                .cloned()
                .collect();
            if let Ok(rolled_back) = eggchaos_core::DatagramPlan::new(pruned) {
                let _ = self
                    .publish_datagram_plan(
                        name,
                        direction,
                        rolled_back,
                        current_seed,
                        Some(current_generation),
                    )
                    .await;
            }
            return Err(ControlError::Conflict(
                "datagram fault id must be unique across directions for path lookup".into(),
            ));
        }
        Ok((direction, fault, next))
    }

    /// Fetch one datagram fault by path ID, searching upstream then
    /// downstream. Reads come from live plan snapshots.
    pub async fn get_datagram_fault(
        &self,
        name: &str,
        id: &str,
    ) -> Option<(Direction, eggchaos_core::DatagramFaultSpec)> {
        for direction in [Direction::Upstream, Direction::Downstream] {
            if let Ok((plan, _, _)) = self.get_datagram_plan(name, direction).await {
                if let Some(fault) = plan.faults().iter().find(|fault| fault.id.as_str() == id) {
                    return Some((direction, fault.clone()));
                }
            }
        }
        None
    }

    /// List both directional datagram fault plans from live snapshots.
    pub async fn list_datagram_faults(
        &self,
        name: &str,
    ) -> Option<(
        Vec<eggchaos_core::DatagramFaultSpec>,
        Vec<eggchaos_core::DatagramFaultSpec>,
    )> {
        let (upstream, _, _) = self
            .get_datagram_plan(name, Direction::Upstream)
            .await
            .ok()?;
        let (downstream, _, _) = self
            .get_datagram_plan(name, Direction::Downstream)
            .await
            .ok()?;
        Some((upstream.faults().to_vec(), downstream.faults().to_vec()))
    }

    /// Update a datagram fault's probability and/or behavior in place,
    /// preserving order. Empty patches are rejected at this semantic
    /// layer, after DTO conversion, so both surfaces agree.
    pub async fn update_datagram_fault(
        &self,
        name: &str,
        id: &str,
        patch: DatagramFaultPatch,
    ) -> Result<(Direction, eggchaos_core::DatagramFaultSpec, u64), ControlError> {
        if patch.probability.is_none() && patch.kind.is_none() {
            return Err(ControlError::Invalid(
                "fault patch must include probability or kind".into(),
            ));
        }
        if let Some(probability) = patch.probability {
            if !(0.0..=1.0).contains(&probability) || !probability.is_finite() {
                return Err(ControlError::Invalid(
                    "probability must be finite and between 0 and 1".into(),
                ));
            }
        }
        for direction in [Direction::Upstream, Direction::Downstream] {
            let Ok((plan, generation, seed)) = self.get_datagram_plan(name, direction).await else {
                continue;
            };
            let Some(mut fault) = plan
                .faults()
                .iter()
                .find(|fault| fault.id.as_str() == id)
                .cloned()
            else {
                continue;
            };
            if let Some(probability) = patch.probability {
                fault.probability = eggchaos_core::Probability::new(probability)
                    .map_err(|error| ControlError::Invalid(error.to_string()))?;
            }
            if let Some(kind) = patch.kind.clone() {
                fault.kind = kind;
            }
            let mut faults = plan.faults().to_vec();
            if let Some(existing) = faults
                .iter_mut()
                .find(|candidate| candidate.id.as_str() == id)
            {
                *existing = fault.clone();
            }
            let updated = eggchaos_core::DatagramPlan::new(faults)
                .map_err(|error| ControlError::Invalid(error.to_string()))?;
            let next = self
                .publish_datagram_plan(name, direction, updated, seed, Some(generation))
                .await?;
            return Ok((direction, fault, next));
        }
        Err(ControlError::NotFound(format!("fault {id} on {name}")))
    }

    /// Remove a datagram fault from either direction. Returns the
    /// direction that owned the fault plus the new generation.
    pub async fn remove_datagram_fault(
        &self,
        name: &str,
        id: &str,
    ) -> Result<(Direction, u64), ControlError> {
        for direction in [Direction::Upstream, Direction::Downstream] {
            let Ok((plan, generation, seed)) = self.get_datagram_plan(name, direction).await else {
                continue;
            };
            if plan.faults().iter().any(|fault| fault.id.as_str() == id) {
                let faults = plan
                    .faults()
                    .iter()
                    .filter(|fault| fault.id.as_str() != id)
                    .cloned()
                    .collect();
                let plan = eggchaos_core::DatagramPlan::new(faults)
                    .map_err(|error| ControlError::Invalid(error.to_string()))?;
                let next = self
                    .publish_datagram_plan(name, direction, plan, seed, Some(generation))
                    .await?;
                return Ok((direction, next));
            }
        }
        Err(ControlError::NotFound(format!("fault {id} on {name}")))
    }

    /// List live datagram associations.
    pub async fn datagram_associations(&self) -> Vec<DatagramAssociationSnapshot> {
        self.runtime.datagrams.all_associations().await
    }

    /// Get active or retained datagram association evidence.
    pub async fn get_datagram_association(&self, id: u64) -> Option<DatagramAssociationSnapshot> {
        self.runtime.datagrams.association(id).await
    }

    /// Administratively terminate an active datagram association.
    pub async fn kill_datagram_association(&self, id: u64) -> bool {
        self.runtime.datagrams.kill_association(id).await
    }
}
