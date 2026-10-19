//! Simulation events. Emitted into `&mut Vec<SimEvent>` by every system, drained by `wp_sim`
//! each tick and forwarded to police, economy, the net layer and the renderer.
//!
//! Serialized with serde's default (externally tagged) representation so both JSON and compact
//! binary formats (postcard/bincode) work. Keep variants small: they are replicated.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{BodyRegion, Component, EconomyEvent, EntityId, PoliceEvent, RaceEvent, WeatherKind};

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
        /// Where it happened, world m (v0.2.0; police witnesses / cameras).
        pos: [f32; 3],
    },
    /// All glass of a body region shattered (v0.2.0; renderer, audio, net).
    GlassShattered {
        /// Vehicle.
        vehicle: EntityId,
        /// Region whose glass broke.
        region: BodyRegion,
    },
    /// A body region crossed a damage threshold (v0.2.0; HUD silhouette, police "visibly
    /// wrecked" heuristics). Thresholded like `ComponentDamaged`, not emitted every tick.
    RegionDamaged {
        /// Vehicle.
        vehicle: EntityId,
        /// Region.
        region: BodyRegion,
        /// Damage severity 0 (cosmetic) ..= 1 (destroyed).
        severity: f32,
    },
    /// Police event (v0.3.0: typed; replaces `PoliceEvent { kind, payload }`).
    Police(PoliceEvent),
    /// Economy event (v0.3.0: typed; replaces `EconomyEvent { kind, payload }`).
    Economy(EconomyEvent),
    /// Race event (v0.3.0).
    Race(RaceEvent),
    /// The weather regime changed (v0.3.0, weather W-2; audio, HUD, replays, stakes).
    WeatherChanged {
        /// New regime.
        kind: WeatherKind,
        /// Intensity 0..1.
        intensity: f32,
        /// Forced by a scenario / admin rather than the schedule.
        forced: bool,
    },
}

/// Entity id used for "the static world" in [`SimEvent::Contact`].
pub const WORLD_ENTITY: EntityId = EntityId(u32::MAX);
