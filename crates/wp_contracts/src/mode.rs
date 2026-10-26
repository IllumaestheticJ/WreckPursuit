//! Damage mode tables (`ModeTable`, `data/modes/{arcade,sim,extreme}.toml`). Every mode uses
//! the same damage code; only these numbers differ.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::DamageMode;

/// Scalars that turn the single damage code path into Arcade, Simulation or Extreme behaviour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModeTable {
    /// Which mode this table describes.
    pub mode: DamageMode,
    /// Multiplier from rigid-body contact impulse to impulse injected into the lattice.
    pub impulse_to_lattice_scale: f32,
    /// Multiplier on plastic flow (how much permanent set a given over-yield strain produces).
    pub plasticity_scale: f32,
    /// Multiplier on beam failure strain and detachable break impulses (higher = tougher).
    pub beam_break_scale: f32,
    /// Multiplier on component health loss per metre of intrusion.
    pub component_damage_scale: f32,
    /// Multiplier on `ComponentMount::intrusion_fail_m` (higher = components survive longer).
    pub component_fail_threshold_scale: f32,
    /// Global health (0..1) below which the vehicle is a total loss.
    pub total_loss_health: f32,
    /// Cabin intrusion that forces a total loss regardless of health, m.
    pub cabin_intrusion_total_loss_m: f32,
    /// Multiplier on repair bills.
    pub repair_cost_scale: f32,
    /// Wheel-detach susceptibility: the wheel mount failure threshold is divided by this
    /// (lower = wheels stay on longer).
    pub wheel_detach_scale: f32,
    /// If true the engine is killed when the vehicle becomes a total loss.
    pub engine_kill_on_total_loss: bool,
    /// If true global health is derived from structural intrusion (in addition to components).
    pub global_health_from_intrusion: bool,
    /// Multiplier on the contact impulses that reach the damage model of *racer* cars (the
    /// player's racer and AI racers; AI racers also get `ai_racer_damage_mult`). Traffic and
    /// player-driven police always take 1.0, AI police `police_damage_scale`. Arcade uses
    /// 0.25, which leaves racers about 20 % of the damage (the lattice takes the first part
    /// of a hit elastically); omitted = 1.0.
    #[serde(default = "one")]
    pub racer_damage_scale: f32,
    /// Extra multiplier for *AI* racers on top of `racer_damage_scale` (the player's racer
    /// does not get it). Arcade uses 2.0: AI racers take twice the player's share of a hit;
    /// omitted = 1.0.
    #[serde(default = "one")]
    pub ai_racer_damage_mult: f32,
    /// Multiplier on the contact impulses that reach the damage model of AI police cars.
    /// Arcade uses 2.0 (police wreck about twice as fast); omitted = 1.0 (full damage).
    #[serde(default = "one")]
    pub police_damage_scale: f32,
}

fn one() -> f32 {
    1.0
}
