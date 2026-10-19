//! Damage state transfer types (v0.3.0; designed by workstream 03 / B, requests #4 and #6).
//! Produced and consumed by [`crate::DamageSim`] implementations.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{BodyRegion, Component};

/// Serialisable damage state of one vehicle (garage persistence, impound, late-join
/// baselines). Arrays are indexed like the lattice the snapshot was taken from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DamageSnapshot {
    /// `LatticeDef::id`; restoring onto another lattice is refused.
    pub lattice_id: String,
    /// Node positions, chassis space m, by node id.
    pub node_pos_m: Vec<[f32; 3]>,
    /// Current beam rest length, m, by beam index.
    pub beam_rest_m: Vec<f32>,
    /// Accumulated plastic strain, by beam index.
    pub beam_plastic_strain: Vec<f32>,
    /// Beam flags: bit 0 broken, bit 1 buckled.
    pub beam_flags: Vec<u8>,
    /// Plastic energy absorbed per region, J.
    pub region_plastic_j: BTreeMap<BodyRegion, f32>,
    /// Component health, only entries below 1.0.
    pub component_health: BTreeMap<Component, f32>,
    /// Failed components.
    pub components_failed: BTreeSet<Component>,
    /// Detached part groups (`Detachable::group`).
    pub detached_groups: BTreeSet<u16>,
    /// Regions whose glass shattered.
    pub shattered: BTreeSet<BodyRegion>,
    /// Lost wheels, FL, FR, RL, RR.
    pub wheels_lost: [bool; 4],
    /// Global health 0..=1.
    pub health: f32,
    /// Total loss.
    pub total_loss: bool,
}

/// Cheap damage read for economy, police and the HUD.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DamageSummary {
    /// Global health 0..=1.
    pub health: f32,
    /// Total loss.
    pub total_loss: bool,
    /// Failed components.
    pub components_failed: u8,
    /// Detached part groups.
    pub parts_detached: u8,
    /// Lost wheels.
    pub wheels_lost: u8,
}
