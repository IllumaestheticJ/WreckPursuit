//! Police read-outs for the testbed: one racer's wanted level / heat (shaped for the UI kit's
//! `HeatHud`) and the police units that are not sim vehicles (internally driven cars,
//! helicopters) for drawing. Everything comes from `wp_sim::Sim::police()`.

use serde::Serialize;
use wp_contracts::{EntityId, WantedLevel};
use wp_police::unit::Assignment;
use wp_police::{PoliceSystem, PursuitOutcome, PursuitStatus, UnitKind};

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
    /// Seconds since a unit last had line of sight (0 unless `Searching`).
    pub staleness_s: f32,
    /// Escape time for the current level and day phase, s.
    pub escape_s: f32,
    /// Outcome of this racer's most recent finished pursuit (`wp_police::PursuitOutcome`
    /// name: `Escaped`, `Safehouse`, `Captured`, `CapturedTotalLoss`, `CalledOff`), if any.
    pub last_outcome: Option<&'static str>,
    /// Seconds since that pursuit ended (police clock), if any.
    pub outcome_age_s: Option<f32>,
}

/// Name of a pursuit outcome (matches the serde name).
fn outcome_name(o: PursuitOutcome) -> &'static str {
    match o {
        PursuitOutcome::Escaped => "Escaped",
        PursuitOutcome::Safehouse => "Safehouse",
        PursuitOutcome::Captured => "Captured",
        PursuitOutcome::CapturedTotalLoss => "CapturedTotalLoss",
        PursuitOutcome::CalledOff => "CalledOff",
    }
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
    // Latest finished pursuit of this racer (records are in start order).
    let last = p
        .stats()
        .pursuits
        .iter()
        .rev()
        .find(|r| r.racer == racer && r.end.is_some() && r.outcome.is_some());
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
        staleness_s: if st == PursuitStatus::Searching {
            los_lost
        } else {
            0.0
        },
        escape_s,
        last_outcome: last.and_then(|r| r.outcome).map(outcome_name),
        outcome_age_s: last.and_then(|r| r.end).map(|end| p.now().since(end)),
    }
}

/// Whether a sim vehicle's siren and light bar are on. A police car bound to a unit follows
/// the unit's assignment: on while responding, pursuing, intercepting or parked at a roadblock
/// / spike strip; off on patrol, idle or out of service. Other vehicles: their input's siren.
/// Presentation only (sim input and state hashes are untouched).
pub fn siren_on(sim: &wp_sim::Sim, v: &wp_sim::SimVehicle) -> bool {
    match v.police_unit {
        Some(unit) => sim
            .police()
            .units()
            .iter()
            .find(|u| u.id == unit)
            .is_some_and(|u| assignment_has_siren(&u.assignment)),
        None => v.input.siren,
    }
}

/// Assignments that run with lights and siren.
pub fn assignment_has_siren(a: &Assignment) -> bool {
    matches!(
        a,
        Assignment::Respond { .. }
            | Assignment::Pursue { .. }
            | Assignment::Intercept { .. }
            | Assignment::RoadblockAt { .. }
            | Assignment::SpikeAt { .. }
    )
}

/// Floats per unit in [`units`].
pub const UNIT_STRIDE: usize = 6;

/// Police units that are **not** sim vehicles (bound units are drawn from `vehicle_states()`),
/// 6 floats each: `[x, y, z, yaw, speed_mps, flags]`, flags 1 helicopter, 2 pursuing /
/// intercepting (lights on). Yaw is about +Z with 0 = +Y.
pub fn units(p: &PoliceSystem, ground: &dyn Fn([f32; 2]) -> f32) -> Vec<f32> {
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
        // Altitude is above the ground under the unit (hilly cities).
        let z = ground(b.pos) + b.alt_m;
        out.extend_from_slice(&[b.pos[0], b.pos[1], z, yaw, b.speed, flags as f32]);
    }
    out
}

/// Helicopters on station, 7 floats each: `[x, y, z, aim_x, aim_y, aim_z, flags]` (flags 1 =
/// tracking `player`). `aim` is the spotlight's ground point (the target's last known
/// position); `z` values are world heights (ground under the point plus altitude).
pub fn helicopters(
    p: &PoliceSystem,
    player: EntityId,
    ground: &dyn Fn([f32; 2]) -> f32,
) -> Vec<f32> {
    let mut out = Vec::new();
    for h in p.helicopters() {
        let at = [h.pos[0], h.pos[1]];
        let aim = [h.spotlight_target[0], h.spotlight_target[1]];
        let flags = (h.target == player) as u32;
        out.extend_from_slice(&[
            at[0],
            at[1],
            ground(at) + h.pos[2],
            aim[0],
            aim[1],
            ground(aim),
            flags as f32,
        ]);
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
        assert!(units(&p, &|_| 0.0).is_empty());
        assert!(helicopters(&p, EntityId(1), &|_| 0.0).is_empty());
        assert_eq!(s.last_outcome, None);
        assert_eq!(s.outcome_age_s, None);
    }

    #[test]
    fn total_loss_while_wanted_reports_capture_outcome() {
        use wp_contracts::{SimEvent, TimeOfDay, PHYSICS_DT};
        use wp_police::scenario::ScenarioWorld;
        use wp_police::RacerView;
        let racer = EntityId(1);
        let view = RacerView {
            entity: racer,
            pos: [0.0; 3],
            vel: [0.0; 3],
        };
        let mut world = ScenarioWorld::clock(TimeOfDay::from_hours(23.0), 0.0);
        let mut p = PoliceSystem::new();
        let mut out = Vec::new();
        let mut step = |p: &mut PoliceSystem, ev: &[SimEvent]| {
            world.time = world.time.advanced(PHYSICS_DT * world.ratio);
            p.step(&world, &[view], ev, PHYSICS_DT, &mut out);
        };
        step(&mut p, &[]);
        p.set_heat(racer, 30.0);
        step(&mut p, &[SimEvent::TotalLoss { vehicle: racer }]);
        for _ in 0..240 {
            step(&mut p, &[]);
        }
        let s = status(&p, racer);
        assert_eq!(s.last_outcome, Some("CapturedTotalLoss"));
        let age = s.outcome_age_s.unwrap();
        assert!((0.9..1.1).contains(&age), "{age}");
        assert_eq!(s.status, "Clear");
    }
}
