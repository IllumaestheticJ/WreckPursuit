//! Police read-outs for the testbed: one racer's wanted level / heat (shaped for the UI kit's
//! `HeatHud`) and the police units that are not sim vehicles (internally driven cars,
//! helicopters) for drawing. Everything comes from `wp_sim::Sim::police()`.

use serde::Serialize;
use wp_contracts::{EntityId, WantedLevel};
use wp_police::unit::Assignment;
use wp_police::{PoliceSystem, PursuitStatus, UnitKind};

/// One racer's police status (JSON for the testbed's heat panel).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PoliceStatus {
    /// Wanted level (`"None"`, `"L1"`…`"L5"`).
    pub level: WantedLevel,
    /// Raw heat.
    pub heat: f32,
    /// Heat 0..=1 within the current level (to the next threshold, or the ceiling at L5).
    pub heat_in_level: f32,
    /// `wp_police::PursuitStatus` name.
    pub status: &'static str,
    /// Escape progress 0..=1 while `Searching` (time without line of sight ÷ escape time).
    pub cooldown: f32,
    /// Units assigned to pursue or intercept this racer.
    pub units_in_pursuit: u32,
    /// Bounty.
    pub bounty: f32,
    /// Police units in the session (cars and helicopters).
    pub units_total: u32,
}

/// Status of `racer` (a racer the police have never seen is `Clear` at heat 0).
pub fn status(p: &PoliceSystem, racer: EntityId) -> PoliceStatus {
    let cfg = p.config();
    let thresholds = cfg.heat.heat.level_thresholds;
    let max_heat = cfg.heat.heat.max_heat;
    let (heat, level, st, los_lost) = p
        .racer(racer)
        .map_or((0.0, WantedLevel::None, PursuitStatus::Clear, 0.0), |r| {
            (r.heat, r.level, r.status, r.los_lost_s)
        });
    let n = level.number() as usize;
    let lo = if n == 0 { 0.0 } else { thresholds[n - 1] };
    let hi = if n < 5 { thresholds[n] } else { max_heat };
    let heat_in_level = ((heat - lo) / (hi - lo).max(1e-6)).clamp(0.0, 1.0);
    let escape_s =
        cfg.heat.escape.base_s[level.table_index()] * cfg.heat.escape.phase_scale.get(p.phase());
    let cooldown = if st == PursuitStatus::Searching {
        (los_lost / escape_s.max(1e-3)).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let units_in_pursuit = p
        .units()
        .iter()
        .filter(|u| {
            matches!(u.assignment, Assignment::Pursue { target } | Assignment::Intercept { target, .. } if target == racer)
        })
        .count() as u32;
    PoliceStatus {
        level,
        heat,
        heat_in_level,
        status: match st {
            PursuitStatus::Clear => "Clear",
            PursuitStatus::Wanted => "Wanted",
            PursuitStatus::Pursuit => "Pursuit",
            PursuitStatus::Searching => "Searching",
        },
        cooldown,
        units_in_pursuit,
        bounty: p.bounty(racer),
        units_total: p.units().len() as u32,
    }
}

/// Floats per unit in [`units`].
pub const UNIT_STRIDE: usize = 6;

/// Police units that are **not** sim vehicles (bound units are drawn from `vehicle_states()`),
/// 6 floats each: `[x, y, z, yaw, speed_mps, flags]`, flags 1 helicopter, 2 pursuing /
/// intercepting (lights on). Yaw is about +Z with 0 = +Y.
pub fn units(p: &PoliceSystem) -> Vec<f32> {
    let mut out = Vec::new();
    for u in p.units().iter().filter(|u| u.entity.is_none()) {
        let b = &u.body;
        let heli = u.kind == UnitKind::Helicopter;
        let chasing = matches!(
            u.assignment,
            Assignment::Pursue { .. } | Assignment::Intercept { .. }
        );
        // Heading (hx, hy) → yaw with 0 = +Y, positive counter-clockwise.
        let yaw = wp_math::atan2(-b.heading[0], b.heading[1]);
        let flags = heli as u32 | (chasing as u32) << 1;
        out.extend_from_slice(&[b.pos[0], b.pos[1], b.alt_m, yaw, b.speed, flags as f32]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_racer_is_clear() {
        let p = PoliceSystem::new();
        let s = status(&p, EntityId(1));
        assert_eq!(s.level, WantedLevel::None);
        assert_eq!(s.status, "Clear");
        assert_eq!(s.heat_in_level, 0.0);
        assert!(units(&p).is_empty());
    }
}
