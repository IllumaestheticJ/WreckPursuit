//! Simulation events. Emitted into `&mut Vec<SimEvent>` by every system, drained by `wp_sim`
//! each tick and forwarded to police, economy, the net layer and the renderer.
//!
//! Serialized with serde's default (externally tagged) representation so both JSON and compact
//! binary formats (postcard/bincode) work. Keep variants small: they are replicated.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Component, EntityId};

/// Kind of traffic violation detected by `wp_world`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum ViolationKind {
    /// Exceeding the speed limit.
    Speeding {
        /// Amount over the limit, m/s.
        over_mps: f32,
    },
    /// Crossing a stop line on red.
    RedLight,
    /// Driving against lane direction.
    WrongWay,
    /// Collision with another road user or property.
    Collision,
    /// Near-misses / dangerous driving.
    Reckless,
}

/// Every event the simulation can emit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub enum SimEvent {
    /// Rigid-body contact between two entities (or an entity and the world).
    Contact {
        /// First entity.
        a: EntityId,
        /// Second entity (`EntityId(u32::MAX)` = static world).
        b: EntityId,
        /// Contact point in world space, m.
        point: [f32; 3],
        /// Unit contact normal in world space (from `a` toward `b`).
        normal: [f32; 3],
        /// Normal impulse magnitude, N·s.
        impulse_ns: f32,
        /// Relative closing speed, m/s.
        rel_speed: f32,
    },
    /// Lattice node displacement deltas. **Debug / replay only**; never sent over the network.
    LatticeDeformed {
        /// Deformed vehicle.
        vehicle: EntityId,
        /// `(node id, displacement delta in chassis space m)`.
        nodes_delta: Vec<(u16, [f32; 3])>,
    },
    /// A detachable part group tore off.
    PartDetached {
        /// Vehicle the part came from.
        vehicle: EntityId,
        /// `Detachable::group` id.
        group: u16,
    },
    /// A component's health changed noticeably (emitters should threshold, not spam).
    ComponentDamaged {
        /// Vehicle.
        vehicle: EntityId,
        /// Component.
        component: Component,
        /// New health 0..=1.
        health: f32,
    },
    /// A component failed (crossed its failure intrusion).
    ComponentFailed {
        /// Vehicle.
        vehicle: EntityId,
        /// Component.
        component: Component,
    },
    /// A wheel came off.
    WheelDetached {
        /// Vehicle.
        vehicle: EntityId,
        /// Wheel index (0 FL, 1 FR, 2 RL, 3 RR).
        wheel: u8,
    },
    /// The vehicle is a total loss.
    TotalLoss {
        /// Vehicle.
        vehicle: EntityId,
    },
    /// A traffic violation was detected.
    TrafficViolation {
        /// Offending vehicle.
        vehicle: EntityId,
        /// What happened.
        kind: ViolationKind,
    },
    /// Police event with a payload defined by `wp_police` (opaque to everyone else).
    PoliceEvent {
        /// Payload kind discriminator, defined by `wp_police`.
        kind: u16,
        /// Encoded payload, defined by `wp_police`.
        payload: Vec<u8>,
    },
    /// Economy event with a payload defined by `wp_economy` (opaque to everyone else).
    EconomyEvent {
        /// Payload kind discriminator, defined by `wp_economy`.
        kind: u16,
        /// Encoded payload, defined by `wp_economy`.
        payload: Vec<u8>,
    },
}

/// Entity id used for "the static world" in [`SimEvent::Contact`].
pub const WORLD_ENTITY: EntityId = EntityId(u32::MAX);
