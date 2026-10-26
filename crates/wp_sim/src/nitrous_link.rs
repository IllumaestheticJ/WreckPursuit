//! AI nitrous (owner 2026-10-26): AI racers and police burn the same bottle as the player
//! (`wp_vehicle::nitrous`), stronger and refilling faster the further they trail their
//! target. That is the rubberbanding; the tables are `data/tune/nitrous.toml`.
//!
//! The hook sits here, not in the racer or police drivers: each tick, before the vehicles
//! step, every AI racer looks at the nearest player and every pursuing police car at the car
//! it chases. The gap is the target's distance ahead along the AI's own heading (negative =
//! the target is behind it), so a car alongside or behind never counts as trailing.

use std::collections::BTreeMap;

use wp_contracts::{EntityId, VehicleInput};
use wp_vehicle::nitrous::{self, AiNitrous, NITROUS_TOOL};

use crate::{Sim, VehicleKind};

/// Distance of `target` ahead of a car at `pos` facing `fwd` (unit, horizontal), m.
fn gap_ahead(pos: [f32; 3], fwd: [f32; 2], target: [f32; 3]) -> f32 {
    (target[0] - pos[0]) * fwd[0] + (target[1] - pos[1]) * fwd[1]
}

/// The input an AI actually drives with: its own, plus the nitrous tool while it fires.
pub(crate) fn with_nitrous(input: &VehicleInput, fire: bool) -> VehicleInput {
    if fire {
        VehicleInput {
            tool_use: NITROUS_TOOL,
            ..*input
        }
    } else {
        *input
    }
}

impl Sim {
    /// Decide this tick's AI nitrous: set every AI car's bottle scale and `nitrous_fire`.
    pub(crate) fn update_ai_nitrous(&mut self) {
        let tune = nitrous::tune();
        let players: Vec<[f32; 3]> = self
            .vehicles
            .iter()
            .filter(|v| v.kind == VehicleKind::Player && !v.dormant)
            .map(|v| v.vehicle.chassis().pos.to_array())
            .collect();
        let positions: BTreeMap<EntityId, [f32; 3]> = self
            .vehicles
            .iter()
            .map(|v| (v.id, v.vehicle.chassis().pos.to_array()))
            .collect();
        let chase: BTreeMap<wp_police::UnitId, EntityId> = self
            .police
            .units()
            .iter()
            .filter_map(|u| Some((u.id, u.assignment.target()?)))
            .collect();
        for slot in &mut self.vehicles {
            let (ai, target): (&AiNitrous, Option<[f32; 3]>) = match slot.kind {
                VehicleKind::AiRacer => {
                    let p = slot.vehicle.chassis().pos.to_array();
                    let nearest = players.iter().copied().min_by(|a, b| {
                        let d = |q: &[f32; 3]| (q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2);
                        d(a).total_cmp(&d(b))
                    });
                    (&tune.racer, nearest)
                }
                VehicleKind::Police if slot.player_police.is_none() => {
                    let target = slot
                        .police_unit
                        .and_then(|u| chase.get(&u))
                        .filter(|t| **t != slot.id)
                        .and_then(|t| positions.get(t).copied());
                    (&tune.police, target)
                }
                _ => {
                    slot.nitrous_fire = false;
                    continue;
                }
            };
            let ch = slot.vehicle.chassis();
            let f = ch.rot * wp_vehicle::math::V3::Y;
            let len = (f.x * f.x + f.y * f.y).sqrt().max(1e-3);
            let pos = ch.pos.to_array();
            let speed = ch.lin_vel.dot(f);
            match target {
                Some(t) => {
                    let gap = gap_ahead(pos, [f.x / len, f.y / len], t);
                    slot.vehicle.set_nitrous_scale(ai.scale(gap));
                    slot.nitrous_fire = ai.wants_fire(gap, &slot.input, speed);
                }
                None => {
                    slot.vehicle.set_nitrous_scale(ai.scale(0.0));
                    slot.nitrous_fire = false;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SimConfig;
    use wp_contracts::TimeOfDay;

    fn data(path: &str) -> String {
        format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
    }

    /// A player at `player_y` and an AI car of `kind` at the origin rolling at 20 m/s flat
    /// out, both facing +Y; returns the AI's bottle after 2 s.
    fn ai_bottle(kind: VehicleKind, player_y: f32) -> (wp_vehicle::nitrous::Nitrous, u64) {
        let car = wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap();
        let mode = wp_contracts::load_mode_table(data("modes/arcade.toml")).unwrap();
        let mut sim = Sim::new(SimConfig {
            seed: 3,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 10.0,
        });
        let player = sim.spawn_vehicle(&car, [4.0, player_y, 0.0], 0.0).unwrap();
        let ai = sim.spawn_vehicle(&car, [0.0; 3], 0.0).unwrap();
        sim.set_vehicle_kind(player, VehicleKind::Player).unwrap();
        sim.set_vehicle_kind(ai, kind).unwrap();
        sim.place_vehicle(ai, [0.0; 3], 0.0, 20.0).unwrap();
        sim.set_input(
            ai,
            VehicleInput {
                throttle: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
        for _ in 0..480 {
            sim.step_fixed();
        }
        let v = sim.vehicles().iter().find(|v| v.id == ai).unwrap();
        (v.vehicle.nitrous(), sim.state_hash())
    }

    #[test]
    fn a_trailing_ai_racer_burns_a_rubberbanded_bottle() {
        let (behind, h) = ai_bottle(VehicleKind::AiRacer, 120.0);
        assert!(behind.fill < 0.8, "burned: {}", behind.fill);
        assert!(behind.scale.strength > nitrous::tune().racer.strength.near);
        assert!(behind.scale.refill > nitrous::tune().racer.refill.near);
        assert_eq!(h, ai_bottle(VehicleKind::AiRacer, 120.0).1, "deterministic");
        // The player behind the AI (it leads): no nitrous, the near scale.
        let (ahead, _) = ai_bottle(VehicleKind::AiRacer, -120.0);
        assert_eq!(ahead.fill, 1.0);
        assert_eq!(ahead.scale, nitrous::tune().racer.scale(0.0));
    }

    #[test]
    fn traffic_never_burns() {
        let (n, _) = ai_bottle(VehicleKind::Traffic, 120.0);
        assert_eq!(n.fill, 1.0);
        assert_eq!(n.scale, wp_vehicle::nitrous::NitrousScale::PLAYER);
    }

    #[test]
    fn gap_is_along_the_heading() {
        assert_eq!(gap_ahead([0.0; 3], [0.0, 1.0], [3.0, 40.0, 0.0]), 40.0);
        assert_eq!(gap_ahead([0.0; 3], [0.0, 1.0], [0.0, -10.0, 0.0]), -10.0);
        assert_eq!(gap_ahead([0.0; 3], [1.0, 0.0], [0.0, 25.0, 0.0]), 0.0);
    }
}
