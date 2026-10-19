//! Physical traffic (lead, run 5; workstream D's request #3).
//!
//! `wp_world` promotes civilian cars near players to full `wp_vehicle` physics. With a traffic
//! car model set ([`Sim::set_traffic_car`]), every promoted agent also becomes a `wp_sim`
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
        self.traffic_car = Some(car);
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
        let Some(car) = self.traffic_car.clone() else {
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
        let _ = self.place_vehicle(id, state.pos, yaw, state.speed_mps);
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
