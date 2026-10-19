//! Traits each system crate implements. Defined here so crates can be developed and tested
//! against each other's interfaces in parallel.

use crate::{
    AssistProfile, ContactImpulse, FfbOutput, HandlingEffects, ModeTable, PriceTable,
    RepairEstimate, SignalState, SimEvent, SurfaceType, TimeOfDay, VehicleInput, VehicleState,
};

/// A drivable vehicle simulation (implemented by `wp_vehicle`).
pub trait VehicleSim {
    /// Advance by `dt` seconds (normally [`crate::PHYSICS_DT`]; sub-step internally with
    /// [`crate::VEHICLE_SUBSTEPS`]), probing the ground under each wheel through `ground`
    /// (v0.2.0: `wp_sim` passes its Rapier scene, tests a flat plane). Push events into `out`.
    fn step(
        &mut self,
        input: &VehicleInput,
        assists: &AssistProfile,
        world: &dyn WorldQuery,
        ground: &dyn GroundQuery,
        dt: f32,
        out: &mut Vec<SimEvent>,
    );
    /// Current state snapshot.
    fn state(&self) -> VehicleState;
    /// Apply the latest damage → handling effects (replaces previous effects, not cumulative).
    fn apply_damage_effects(&mut self, eff: &HandlingEffects);
    /// Current force-feedback output.
    fn ffb(&self) -> FfbOutput;
}

/// A deformable damage model for one vehicle (implemented by `wp_damage`).
pub trait DamageSim {
    /// Inject this step's contact impulses and advance the lattice by `dt`.
    fn apply_contacts(
        &mut self,
        contacts: &[ContactImpulse],
        mode: &ModeTable,
        dt: f32,
        out: &mut Vec<SimEvent>,
    );
    /// Current handling effects derived from damage.
    fn handling_effects(&self) -> HandlingEffects;
    /// Current node positions in chassis space, indexed by node id.
    fn node_positions(&self) -> &[[f32; 3]];
    /// Repair estimate for the current damage.
    fn repair_estimate(&self, mode: &ModeTable, price_table: &PriceTable) -> RepairEstimate;
}

/// A ray hit on world geometry (v0.2.0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroundHit {
    /// Distance from the ray origin, m.
    pub dist_m: f32,
    /// Hit point, world space, m.
    pub point: [f32; 3],
    /// Unit surface normal, world space.
    pub normal: [f32; 3],
}

/// Ray queries against solid world geometry (v0.2.0). Implemented by `wp_sim` over its Rapier
/// scene and by `wp_vehicle::FlatGround` for tests and headless tools.
pub trait GroundQuery {
    /// First hit of the ray from `origin` along unit `dir` within `max_m`, if any.
    fn cast_ray(&self, origin: [f32; 3], dir: [f32; 3], max_m: f32) -> Option<GroundHit>;
}

/// Read-only world queries the vehicle, damage and police systems need
/// (implemented by `wp_world`).
pub trait WorldQuery {
    /// Surface type at a world-space point.
    fn surface_at(&self, p: [f32; 3]) -> SurfaceType;
    /// Speed limit at a world-space point, m/s (None off-road).
    fn speed_limit_at(&self, p: [f32; 3]) -> Option<f32>;
    /// State of the next traffic signal ahead of `p` along `heading` (unit vector), if any.
    fn signal_state_ahead(&self, p: [f32; 3], heading: [f32; 3]) -> Option<SignalState>;
    /// Current in-game time of day.
    fn time_of_day(&self) -> TimeOfDay;
    /// Whether the straight segment from `a` to `b` is unobstructed by buildings / terrain
    /// (v0.2.0; police sightlines). Worlds without occluders return `true`.
    fn line_of_sight(&self, a: [f32; 3], b: [f32; 3]) -> bool;
}
