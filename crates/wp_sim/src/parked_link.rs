//! Dentable parked cars (prototype).
//!
//! City parked cars are cheap rigid boxes (`props.rs`): they can be shoved but never dent.
//! With a parked car model set ([`Sim::set_parked_car`]), a parked-car prop that a vehicle is
//! about to hit is swapped for a full `wp_sim` vehicle (Rapier chassis, wheels, damage model)
//! at the prop's pose, with no driver and the handbrake on. It takes the hit like any other
//! car. Once it has been at rest for a second with no other vehicle near, the vehicle is
//! removed and the prop put back where the car stopped. A car that was damaged keeps its
//! damage model, which it gets back if the prop is swapped in again.
//!
//! Bounded: at most [`MAX_PARKED_VEHICLES`] at a time, and none while the sim already has
//! `MAX_VEHICLES - PARKED_SLOT_RESERVE` vehicles; past either limit a prop stays a box.
//! Deterministic: props are swapped in vehicle slot order then prop index order, and released
//! in slot order, all from sim state only.

use std::collections::BTreeSet;

use rapier3d::parry::query;
use rapier3d::prelude::{Pose, Vector};
use wp_contracts::{LoadedCar, VehicleInput, MAX_VEHICLES};

use crate::{Sim, VehicleKind};

/// Vehicle slots kept free for players, racers, police and traffic when swapping in parked
/// cars.
pub const PARKED_SLOT_RESERVE: usize = 32;

/// Most parked cars simulated as vehicles at once.
pub const MAX_PARKED_VEHICLES: usize = 8;

/// A prop is swapped in when a vehicle's chassis is within this gap of it, m...
const SWAP_MARGIN_M: f32 = 0.5;
/// ...plus the closing speed times this look-ahead, s (a fast car covers it in a few ticks).
const SWAP_LOOKAHEAD_S: f32 = 0.15;
/// Rough chassis bounding radius used to find candidate props around a vehicle, m.
const MOVER_RADIUS_M: f32 = 3.0;
/// A parked car counts as at rest below this speed, m/s...
const REST_SPEED_MPS: f32 = 0.1;
/// ...and this spin rate, rad/s.
const REST_SPIN_RADPS: f32 = 0.1;
/// At rest this long (1 s) before it can turn back into a prop, ticks.
const RELEASE_QUIET_TICKS: u32 = 240;
/// No other moving vehicle within this distance when it turns back into a prop, m.
const RELEASE_CLEAR_M: f32 = 12.0;

impl Sim {
    /// Car model parked-car props turn into when something is about to hit them, so they dent
    /// (opt-in; without it parked cars stay rigid boxes). Set after [`Sim::set_world`].
    pub fn set_parked_car(&mut self, car: LoadedCar) {
        self.parked_car = Some(car);
    }

    /// Number of parked cars currently simulated as vehicles.
    pub fn parked_vehicle_count(&self) -> usize {
        self.vehicles
            .iter()
            .filter(|v| v.parked_prop.is_some())
            .count()
    }

    /// Number of parked cars back as props that carry damage from an earlier hit.
    pub fn damaged_parked_count(&self) -> usize {
        self.parked_damage.len()
    }

    /// Drop parked-car vehicles and stored damage (new world).
    pub(crate) fn clear_parked(&mut self) {
        let ids: Vec<_> = self
            .vehicles
            .iter()
            .filter(|v| v.parked_prop.is_some())
            .map(|v| v.id)
            .collect();
        for id in ids {
            let _ = self.despawn_vehicle(id);
        }
        self.parked_damage.clear();
    }

    /// Before the vehicle step: swap in parked-car props a vehicle is about to reach.
    pub(crate) fn embody_parked(&mut self) {
        if self.parked_car.is_none() || self.props.is_empty() {
            return;
        }
        let mut want = BTreeSet::new();
        for v in &self.vehicles {
            let rb = &self.physics.bodies[v.body.body];
            let pos = rb.translation();
            let vel = rb.linvel();
            let span = MOVER_RADIUS_M + SWAP_MARGIN_M + vel.length() * SWAP_LOOKAHEAD_S;
            let col = &self.physics.colliders[v.body.collider];
            for i in self.props.near(&self.physics.bodies, pos.to_array(), span) {
                if !self.props.is_dentable(i) || want.contains(&i) {
                    continue;
                }
                let prb = &self.physics.bodies[self.props.bodies[i as usize]];
                let Some(&pch) = prb.colliders().first() else {
                    continue;
                };
                let pcol = &self.physics.colliders[pch];
                let to = (prb.translation() - pos).normalize_or_zero();
                let closing = (vel - prb.linvel()).dot(to).max(0.0);
                let gap =
                    query::distance(col.position(), col.shape(), pcol.position(), pcol.shape())
                        .map(|d| d.distance)
                        .unwrap_or(f32::MAX);
                if gap <= SWAP_MARGIN_M + closing * SWAP_LOOKAHEAD_S {
                    want.insert(i);
                }
            }
        }
        for i in want {
            if self.parked_vehicle_count() >= MAX_PARKED_VEHICLES
                || self.vehicles.len() + PARKED_SLOT_RESERVE >= MAX_VEHICLES
            {
                break;
            }
            self.embody_prop(i);
        }
    }

    /// Replace prop `i` with a parked vehicle standing where the prop is.
    fn embody_prop(&mut self, i: u32) {
        let Some(car) = self.parked_car.clone() else {
            return;
        };
        let h = self.props.bodies[i as usize];
        let rb = &self.physics.bodies[h];
        let half_z = rb
            .colliders()
            .first()
            .and_then(|c| self.physics.colliders[*c].shape().as_cuboid())
            .map_or(0.75, |c| c.half_extents.z);
        let rot = *rb.rotation();
        let up = rot * Vector::Z;
        let fwd = rot * Vector::Y;
        let ground = rb.translation() - up * half_z;
        let yaw = wp_math::atan2(-fwd.x, fwd.y);
        let Ok(id) = self.spawn_vehicle(&car, ground.to_array(), yaw) else {
            return;
        };
        if self.has_terrain() {
            let _ = self.place_on_ground(id, [ground.x, ground.y], yaw, 0.0, Some(ground.z));
        }
        let stored = self.parked_damage.remove(&i);
        if let Ok(slot) = self.slot_mut(id) {
            slot.kind = VehicleKind::Parked;
            slot.parked_prop = Some(i);
            slot.input = VehicleInput {
                handbrake: 1.0,
                ..Default::default()
            };
            if let Some(mut damage) = stored {
                damage.set_entity(id);
                // Forces a collider rebuild from the dented hull on the next damage sync.
                slot.hull_version = damage.hull_version().wrapping_add(1);
                slot.damage = damage;
            }
        }
        self.props
            .hide(i, &mut self.physics.bodies, &mut self.physics.colliders);
    }

    /// After the step: parked cars at rest with nobody near turn back into props (keeping
    /// their damage, if any).
    pub(crate) fn release_parked(&mut self) {
        if !self.vehicles.iter().any(|v| v.parked_prop.is_some()) {
            return;
        }
        for slot in &mut self.vehicles {
            if slot.parked_prop.is_none() {
                continue;
            }
            let rb = &self.physics.bodies[slot.body.body];
            let still =
                rb.linvel().length() < REST_SPEED_MPS && rb.angvel().length() < REST_SPIN_RADPS;
            slot.parked_quiet_ticks = if still {
                slot.parked_quiet_ticks.saturating_add(1)
            } else {
                0
            };
        }
        let mut release = Vec::new();
        for v in &self.vehicles {
            let Some(prop) = v.parked_prop else {
                continue;
            };
            if v.parked_quiet_ticks < RELEASE_QUIET_TICKS {
                continue;
            }
            let p = self.physics.bodies[v.body.body].translation();
            let crowded = self.vehicles.iter().any(|o| {
                o.id != v.id
                    && o.parked_quiet_ticks < RELEASE_QUIET_TICKS
                    && (self.physics.bodies[o.body.body].translation() - p).length()
                        < RELEASE_CLEAR_M
            });
            if !crowded {
                release.push((v.id, prop));
            }
        }
        for (id, prop) in release {
            let Some(slot) = self.vehicles.iter().find(|v| v.id == id) else {
                continue;
            };
            let rb = &self.physics.bodies[slot.body.body];
            let ph = self.props.bodies[prop as usize];
            let half_z = self.physics.bodies[ph]
                .colliders()
                .first()
                .and_then(|c| self.physics.colliders[*c].shape().as_cuboid())
                .map_or(0.75, |c| c.half_extents.z);
            let rot = *rb.rotation();
            let pose = Pose::from_parts(rb.translation() + rot * Vector::Z * half_z, rot);
            let damaged =
                slot.damage.total_plastic_energy_j() > 0.0 || slot.damage.broken_beam_count() > 0;
            let damage = damaged.then(|| slot.damage.clone());
            let _ = self.despawn_vehicle(id);
            self.props.show(
                prop,
                &mut self.physics.bodies,
                &mut self.physics.colliders,
                pose,
            );
            if let Some(d) = damage {
                self.parked_damage.insert(prop, d);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SimConfig;
    use wp_contracts::{EntityId, TimeOfDay, VehicleSim};
    use wp_world::World;

    fn data(path: &str) -> String {
        format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
    }

    /// Procedural city with dentable parked cars and one sedan 20 m beside the first parked
    /// car, aimed at its side at `speed` m/s.
    fn setup(speed: f32) -> (Sim, EntityId, wp_world::StaticProp) {
        let car = wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap();
        let mode = wp_contracts::load_mode_table(data("modes/arcade.toml")).unwrap();
        let mut sim = Sim::new(SimConfig {
            seed: 9,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 1.0,
        });
        let world = World::procedural(
            &wp_world::procgen::ProcGenConfig::small(),
            wp_world::WorldConfig::default(),
        );
        let target = *world.static_props().next().unwrap();
        assert_eq!(target.kind, wp_world::PropKind::ParkedCar);
        sim.set_world(world);
        sim.set_parked_car(car.clone());
        let (s, c) = wp_math::sin_cos(target.yaw);
        let start = [target.pos[0] + c * 20.0, target.pos[1] + s * 20.0, 0.0];
        let (dx, dy) = (target.pos[0] - start[0], target.pos[1] - start[1]);
        let yaw = wp_math::atan2(-dx, dy);
        let id = sim.spawn_vehicle(&car, start, yaw).unwrap();
        sim.place_vehicle(id, start, yaw, speed).unwrap();
        (sim, id, target)
    }

    fn parked(sim: &Sim) -> Option<&crate::SimVehicle> {
        sim.vehicles().iter().find(|v| v.parked_prop == Some(0))
    }

    #[test]
    fn untouched_parked_cars_stay_props() {
        let (mut sim, id, _) = setup(0.0);
        // Parked 200 m away: nothing is near any parked car.
        sim.place_vehicle(id, [200.0, 200.0, 0.0], 0.0, 0.0)
            .unwrap();
        for _ in 0..240 {
            sim.step_fixed();
        }
        assert_eq!(sim.parked_vehicle_count(), 0);
    }

    #[test]
    fn hit_parked_car_dents_then_turns_back_into_a_prop_with_its_damage() {
        let (mut sim, id, target) = setup(14.0);
        let mut seen = false;
        for _ in 0..480 {
            sim.step_fixed();
            seen |= parked(&sim).is_some();
        }
        assert!(seen, "the parked car was never swapped for a vehicle");
        let v = parked(&sim).expect("still a vehicle while the hitter is against it");
        assert_eq!(v.kind, VehicleKind::Parked);
        let plastic = v.damage.total_plastic_energy_j();
        assert!(plastic > 50.0, "parked car barely dented: {plastic} J");
        assert!(sim.parked_vehicle_count() <= MAX_PARKED_VEHICLES);
        let moved = {
            let p = v.vehicle.state().pos;
            ((p[0] - target.pos[0]).powi(2) + (p[1] - target.pos[1]).powi(2)).sqrt()
        };
        assert!(moved < 10.0, "parked car flew {moved} m");

        // Hitter leaves: the wreck settles, turns back into a prop and keeps its damage.
        sim.place_vehicle(id, [200.0, 200.0, 0.0], 0.0, 0.0)
            .unwrap();
        for _ in 0..720 {
            sim.step_fixed();
        }
        assert!(
            parked(&sim).is_none(),
            "wreck never turned back into a prop"
        );
        assert_eq!(sim.damaged_parked_count(), 1);
        let rb = &sim.physics.bodies[sim.props.bodies[0]];
        assert!(rb.is_enabled() && rb.is_fixed());
        let z = rb.translation().z;
        assert!((z - target.pos[2]).abs() < 0.5, "prop put back at z {z}");

        // Driven at again: swapped in with the stored damage.
        let p = rb.translation();
        let (s, c) = wp_math::sin_cos(target.yaw);
        let start = [p.x + c * 12.0, p.y + s * 12.0, 0.0];
        let yaw = wp_math::atan2(-(p.x - start[0]), p.y - start[1]);
        sim.place_vehicle(id, start, yaw, 5.0).unwrap();
        for _ in 0..480 {
            sim.step_fixed();
            if parked(&sim).is_some() {
                break;
            }
        }
        let v = parked(&sim).expect("not swapped in on the second approach");
        assert!(v.damage.total_plastic_energy_j() >= plastic * 0.999);
        assert_eq!(sim.damaged_parked_count(), 0);
    }

    #[test]
    fn parked_swaps_are_deterministic() {
        let run = || {
            let (mut sim, _, _) = setup(14.0);
            for _ in 0..480 {
                sim.step_fixed();
            }
            sim.state_hash()
        };
        assert_eq!(run(), run());
    }
}
