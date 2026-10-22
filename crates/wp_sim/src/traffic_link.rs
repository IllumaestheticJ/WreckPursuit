//! Physical traffic (lead, run 5; workstream D's request #3).
//!
//! `wp_world` promotes civilian cars near players to full `wp_vehicle` physics. With a traffic
//! car model set ([`Sim::set_traffic_car`], or several with [`Sim::set_traffic_cars`]), every promoted agent also becomes a `wp_sim`
//! vehicle: a Rapier body that collides and a damage model that dents. `wp_world`'s driving
//! controller keeps choosing the inputs; this module copies them into the vehicle before the
//! step, writes the real state back into the agent afterwards, and reports damage effects.
//! Demoted or despawned agents lose their vehicle. Deterministic: LOD changes in `wp_world`
//! order, agents in id order.

use wp_contracts::{AssistProfile, DamageSim, EntityId, LoadedCar, VehicleSim, MAX_VEHICLES};
use wp_world::{AgentId, LodChange};

use crate::{Sim, VehicleKind};

/// Vehicle slots kept free for players, racers and police when embodying traffic.
pub const TRAFFIC_SLOT_RESERVE: usize = 24;

impl Sim {
    /// Car model for physical traffic (also handed to `wp_world`, which only promotes agents
    /// to full physics when it has one). Set after [`Sim::set_world`].
    pub fn set_traffic_car(&mut self, car: LoadedCar) {
        self.world.set_traffic_car(&car);
        self.traffic_cars = vec![car];
    }

    /// Several car models for physical traffic (also handed to `wp_world`). Agent `id` drives
    /// `cars[wp_world::traffic::traffic_model_index(id, cars.len())]` — a pure function of the
    /// id, so a car keeps its model when it is promoted, demoted and promoted again, and
    /// kinematic agents can be drawn with the same model ([`wp_world::World::traffic_model_of`]).
    /// One car is exactly [`Sim::set_traffic_car`]. Set after [`Sim::set_world`].
    pub fn set_traffic_cars(&mut self, cars: Vec<LoadedCar>) {
        self.world.set_traffic_cars(&cars);
        self.traffic_cars = cars;
    }

    /// The physical traffic models in index order (see [`Sim::set_traffic_cars`]).
    pub fn traffic_cars(&self) -> &[LoadedCar] {
        &self.traffic_cars
    }

    /// After the world step: give promoted agents a vehicle, remove demoted / despawned ones.
    pub(crate) fn apply_traffic_lod(&mut self) {
        for change in self.world.drain_lod_changes() {
            match change {
                LodChange::Promoted(agent) => self.embody_agent(agent),
                LodChange::Demoted(agent) | LodChange::Despawned(agent) => {
                    let id = self
                        .vehicles
                        .iter()
                        .find(|v| v.traffic_agent == Some(agent))
                        .map(|v| v.id);
                    if let Some(id) = id {
                        let _ = self.despawn_vehicle(id);
                    }
                    if matches!(change, LodChange::Demoted(_)) {
                        self.world.set_agent_entity(agent, None);
                    }
                }
            }
        }
    }

    fn embody_agent(&mut self, agent: AgentId) {
        let model = self.world.traffic_model_of(agent);
        let Some(car) = self.traffic_cars.get(model).cloned() else {
            return;
        };
        if self.vehicles.len() + TRAFFIC_SLOT_RESERVE >= MAX_VEHICLES {
            return;
        }
        let Some(state) = self
            .world
            .physical_agents_mut()
            .find(|(id, _)| *id == agent)
            .map(|(_, pa)| pa.vehicle.state())
        else {
            return;
        };
        let q = state.rot;
        let fx = 2.0 * (q[0] * q[1] - q[3] * q[2]);
        let fy = 1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]);
        let yaw = wp_math::atan2(-fx, fy);
        let Ok(id) = self.spawn_vehicle(&car, state.pos, yaw) else {
            return;
        };
        // Pitched onto the lane under it on hilly cities (plain place on flat ones).
        let _ = self.place_on_ground(
            id,
            [state.pos[0], state.pos[1]],
            yaw,
            state.speed_mps,
            Some(state.pos[2]),
        );
        let _ = self.set_assists(id, AssistProfile::ai());
        if let Ok(slot) = self.slot_mut(id) {
            slot.kind = VehicleKind::Traffic;
            slot.traffic_agent = Some(agent);
        }
        self.world.set_agent_entity(agent, Some(id));
    }

    /// Before the vehicle step: traffic drivers' inputs into their vehicles.
    pub(crate) fn drive_traffic(&mut self) {
        if !self.vehicles.iter().any(|v| v.traffic_agent.is_some()) {
            return;
        }
        let inputs: Vec<(AgentId, wp_contracts::VehicleInput)> = self
            .world
            .physical_agents_mut()
            .map(|(id, pa)| (id, pa.input))
            .collect();
        for slot in &mut self.vehicles {
            if let Some(agent) = slot.traffic_agent {
                if let Some((_, input)) = inputs.iter().find(|(id, _)| *id == agent) {
                    slot.input = *input;
                }
            }
        }
    }

    /// After physics and damage: the real state and damage back into each agent.
    pub(crate) fn sync_traffic(&mut self) {
        let embodied: Vec<(AgentId, EntityId)> = self
            .vehicles
            .iter()
            .filter_map(|v| v.traffic_agent.map(|a| (a, v.id)))
            .collect();
        if embodied.is_empty() {
            return;
        }
        for (agent, id) in embodied {
            let Some(slot) = self.vehicles.iter().find(|v| v.id == id) else {
                continue;
            };
            let state = slot.vehicle.state();
            let effects = slot.damage.handling_effects();
            if let Some((_, pa)) = self.world.physical_agents_mut().find(|(a, _)| *a == agent) {
                pa.vehicle.set_state(&state);
            }
            self.world.apply_agent_damage(agent, &effects);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Sim, SimConfig, VehicleKind};
    use wp_contracts::{LoadedCar, TimeOfDay, VehicleInput};
    use wp_world::World;

    fn car(id: &str) -> LoadedCar {
        let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
        wp_contracts::load_car(p).unwrap()
    }

    /// Player drives through a small city with `models` as physical traffic (`list` = through
    /// [`Sim::set_traffic_cars`], else [`Sim::set_traffic_car`] with the first). Returns the
    /// state hash and every (agent, car id) seen as a physical traffic vehicle.
    fn run(models: &[&str], list: bool) -> (u64, Vec<(u32, String)>) {
        let player = car("debug_sedan");
        let mode = wp_contracts::load_mode_table(format!(
            "{}/../../data/modes/sim.toml",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let mut sim = Sim::new(SimConfig {
            seed: 12,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 10.0,
        });
        let mut world = World::procedural(
            &wp_world::procgen::ProcGenConfig::small(),
            wp_world::WorldConfig::default(),
        );
        let id = sim.spawn_vehicle(&player, [0.0, 0.0, 0.0], 0.0).unwrap();
        let tracked: Vec<_> = sim
            .vehicle_states()
            .iter()
            .map(|(e, s)| wp_world::TrackedVehicle::from_state(*e, s, false, true))
            .collect();
        world.set_tracked_vehicles(&tracked);
        world.populate();
        sim.set_world(world);
        if list {
            sim.set_traffic_cars(models.iter().map(|m| car(m)).collect());
        } else {
            sim.set_traffic_car(car(models[0]));
        }
        sim.set_input(
            id,
            VehicleInput {
                throttle: 0.4,
                ..Default::default()
            },
        )
        .unwrap();
        let mut seen: Vec<(u32, String)> = Vec::new();
        for _ in 0..1440 {
            sim.step_fixed();
            for v in sim.vehicles() {
                if v.kind == VehicleKind::Traffic {
                    let a = v.traffic_agent.unwrap();
                    // The physical car is the model the agent had while kinematic.
                    let m = sim.world().traffic_model_of(a);
                    assert_eq!(sim.traffic_cars()[m].def.id, v.car_id);
                    if !seen.iter().any(|(s, _)| *s == a) {
                        seen.push((a, v.car_id.clone()));
                    }
                }
            }
        }
        (sim.state_hash(), seen)
    }

    #[test]
    fn several_traffic_models_by_agent_id() {
        let models = ["debug_sedan", "tansy_13", "furrow_24", "junkyard_beater"];
        let (h, seen) = run(&models, true);
        assert!(!seen.is_empty(), "no traffic became physical");
        let distinct: std::collections::BTreeSet<&String> = seen.iter().map(|(_, c)| c).collect();
        assert!(distinct.len() >= 2, "only {distinct:?} became physical");
        assert_eq!((h, seen), run(&models, true));
    }

    #[test]
    fn a_one_model_list_matches_set_traffic_car() {
        assert_eq!(run(&["debug_sedan"], false), run(&["debug_sedan"], true));
    }
}
