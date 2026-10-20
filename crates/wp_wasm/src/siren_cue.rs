//! Siren v2 bridge (playtest row 82): reads each police car's AI state **read-only** and turns
//! it into a [`wp_audio::SirenCue`] for the audio director, and answers the director's
//! acoustic questions (line of sight, covered roads) from the city. Presentation only: nothing
//! here writes to the simulation, the police system or any state hash.

use std::collections::BTreeMap;

use wp_audio::{AcousticQuery, SirenCue};
use wp_contracts::{EntityId, WorldQuery};
use wp_police::unit::{Assignment, Unit};

/// Pursuing within this distance of the target = close pursuit (yelp), m.
pub const CLOSE_M: f32 = 45.0;
/// A junction this close ahead on the unit's track = junction cue (phaser), m.
pub const JUNCTION_AHEAD_M: f32 = 35.0;
/// Held behind a vehicle / waiting at a junction this long = blocked (yelp + air horn), s.
pub const BLOCKED_AFTER_S: f32 = 0.8;
/// …while moving slower than this, m/s.
pub const BLOCKED_BELOW_MPS: f32 = 3.0;

/// The siren cue of one police unit. `target_pos` looks up a racer's position (for the close
/// pursuit test).
pub fn unit_cue(u: &Unit, target_pos: &dyn Fn(EntityId) -> Option<[f32; 3]>) -> SirenCue {
    let hot = crate::police::assignment_has_siren(&u.assignment);
    if !hot || u.wrecked {
        return SirenCue::Off;
    }
    let a = &u.avoid;
    let slow = u.body.speed.abs() < BLOCKED_BELOW_MPS;
    let waited = a.blocked_s.max(a.unit_wait_s).max(a.yield_s);
    let parked = matches!(
        u.assignment,
        Assignment::RoadblockAt { .. } | Assignment::SpikeAt { .. }
    );
    if !parked && slow && waited > BLOCKED_AFTER_S {
        return SirenCue::Blocked;
    }
    if !parked {
        if let Some(j) = a.junction {
            let ahead = j.s - u.track_s;
            if (-5.0..JUNCTION_AHEAD_M).contains(&ahead) {
                return SirenCue::Junction;
            }
        }
    }
    match u.assignment {
        Assignment::Pursue { target } | Assignment::Intercept { target, .. } => {
            let close = target_pos(target).is_some_and(|p| {
                let dx = p[0] - u.body.pos[0];
                let dy = p[1] - u.body.pos[1];
                dx * dx + dy * dy < CLOSE_M * CLOSE_M
            });
            if close {
                SirenCue::Close
            } else {
                SirenCue::Pursuit
            }
        }
        _ => SirenCue::Respond,
    }
}

/// Siren cue of a sim vehicle: its police unit's cue if it is bound to one, else `None` (the
/// vehicle's own siren input and the car's mode decide, e.g. a player driving a police car).
pub fn vehicle_cue(
    sim: &wp_sim::Sim,
    v: &wp_sim::SimVehicle,
    positions: &BTreeMap<EntityId, [f32; 3]>,
) -> Option<SirenCue> {
    let unit = v.police_unit?;
    let u = sim.police().units().iter().find(|u| u.id == unit)?;
    Some(unit_cue(u, &|e| positions.get(&e).copied()))
}

/// The city answering the siren's acoustic questions.
pub struct WorldAcoustics<'a>(pub &'a wp_world::World);

impl AcousticQuery for WorldAcoustics<'_> {
    fn clear(&self, a: [f32; 3], b: [f32; 3]) -> bool {
        self.0.line_of_sight(a, b)
    }
    fn covered(&self, p: [f32; 3]) -> bool {
        self.0.covered_at(p).is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_contracts::{IncidentId, UnitId, UnitKind};
    use wp_police::UnitRole;

    fn unit(a: Assignment) -> Unit {
        let mut u = Unit::new(
            UnitId(1),
            UnitKind::Car,
            UnitRole::Reinforcement,
            [0.0, 0.0],
            [0.0, 1.0],
            1.0,
        );
        u.assignment = a;
        u.body.speed = 15.0;
        u
    }

    #[test]
    fn assignments_map_to_cues() {
        let none = |_: EntityId| None;
        let near = |_: EntityId| Some([0.0, 20.0, 0.0]);
        let far = |_: EntityId| Some([0.0, 200.0, 0.0]);
        let patrol = unit(Assignment::Idle);
        assert_eq!(unit_cue(&patrol, &none), SirenCue::Off);
        let respond = unit(Assignment::Respond {
            incident: IncidentId(3),
        });
        assert_eq!(unit_cue(&respond, &none), SirenCue::Respond);
        let chase = unit(Assignment::Pursue {
            target: EntityId(9),
        });
        assert_eq!(unit_cue(&chase, &far), SirenCue::Pursuit);
        assert_eq!(unit_cue(&chase, &near), SirenCue::Close);
        let mut stuck = chase.clone();
        stuck.body.speed = 0.5;
        stuck.avoid.blocked_s = 2.0;
        assert_eq!(unit_cue(&stuck, &far), SirenCue::Blocked);
        let mut wrecked = chase;
        wrecked.wrecked = true;
        assert_eq!(unit_cue(&wrecked, &near), SirenCue::Off);
    }
}
