//! Slipstream (drafting) for every vehicle (owner approval 2026-10-31: the one exception to
//! the physics freeze).
//!
//! Each fixed tick, before the vehicles step, the sim builds one [`DraftBody`] per vehicle
//! slot (Rapier body position and velocity, `VehicleDef::body_size_m` width and length) and
//! runs [`wp_vehicle::draft::drag_scales_into`] over all of them. Every car gets its
//! multiplier through [`wp_vehicle::vehicle::Vehicle::set_draft_drag_scale`], which scales its
//! aerodynamic drag on top of the damage model's `drag_scale`.
//!
//! One physics for everyone: players, AI racers, police (AI or player seats), physical
//! traffic and network players all draft and all make a wake; nothing here looks at the
//! vehicle kind or the damage mode. Dormant cars stand still (no wake: slower than
//! `min_speed_mps`); light-mode cars make a wake from their kinematic body but have no aero
//! step of their own. The multiplier is derived from the vehicle states every tick, so it is
//! not stored, hashed or replicated: replays and client prediction recompute it.

use wp_vehicle::draft::{drag_scales_into, DraftBody, DraftParams, DraftScratch as ModelScratch};

use crate::Sim;

/// Slipstream tuning the sim applies (`wp_vehicle::draft` defaults: 35 % less drag at a
/// zero gap, fading to nothing 25 m behind; leaders under 10 m/s make no wake).
pub const DRAFT_PARAMS: DraftParams = DraftParams {
    range_m: 25.0,
    max_reduction: 0.35,
    min_speed_mps: 10.0,
    min_alignment: 0.9,
};

/// Per-tick scratch for the slipstream pass (not simulation state).
#[derive(Clone, Default)]
pub(crate) struct DraftScratch {
    bodies: Vec<DraftBody>,
    model: ModelScratch,
    scales: Vec<f32>,
}

impl Sim {
    /// Set every vehicle's slipstream drag multiplier from this tick's positions and
    /// velocities (before the vehicle step). Fixed order: vehicle slot order.
    pub(crate) fn apply_slipstream(&mut self) {
        let mut s = std::mem::take(&mut self.draft);
        s.bodies.clear();
        s.bodies.extend(self.vehicles.iter().map(|v| {
            let rb = &self.physics.bodies[v.body.body];
            let (t, lv) = (rb.translation(), rb.linvel());
            let size = v.vehicle.def().body_size_m;
            DraftBody {
                pos: [t.x, t.y],
                vel: [lv.x, lv.y],
                width_m: size[0],
                length_m: size[1],
            }
        }));
        drag_scales_into(&s.bodies, &DRAFT_PARAMS, &mut s.model, &mut s.scales);
        for (v, k) in self.vehicles.iter_mut().zip(&s.scales) {
            v.vehicle.set_draft_drag_scale(*k);
        }
        self.draft = s;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_match_the_model_defaults() {
        assert_eq!(DRAFT_PARAMS, DraftParams::default());
    }
}
