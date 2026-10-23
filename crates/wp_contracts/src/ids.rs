//! Identifier and role types shared across the simulation and the network layer.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Simulation entity identifier (vehicles, debris, police units, traffic agents).
/// Allocated by `wp_sim`, never reused within one instance.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct EntityId(pub u32);

/// Human player identifier, allocated by `wp_net` on connect.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct PlayerId(pub u32);

/// Durable account id (v0.3.0, F NET-5 / meta MC-1): allocated by meta at signup and stable
/// across instances, unlike the per-instance [`PlayerId`] slot. The server maps
/// `PlayerId → AccountId` at join.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct AccountId(pub u32);

/// Role a human player holds in an instance.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum Role {
    /// Street racer driving their own car.
    Racer,
    /// The single police captain: map view, dispatch, tools. Does not drive.
    PoliceCaptain,
    /// Player-driven police chase unit.
    PoliceChase,
    /// Observer with no vehicle.
    Spectator,
}

/// Damage mode. Every mode runs the same code path with a different [`crate::ModeTable`].
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    Serialize,
    Deserialize,
    JsonSchema,
)]
pub enum DamageMode {
    /// Forgiving: the Simulation table with racer cars (player and AI racers) taking about 20 %
    /// of the damage (`ModeTable::racer_damage_scale`); traffic and police take full damage.
    #[default]
    Arcade,
    /// Punishing but survivable; one hard hit degrades, several end the car.
    Simulation,
    /// Real-world-ish; one hard hit at speed is a total loss (the pre-0.4 Simulation table).
    Extreme,
}
