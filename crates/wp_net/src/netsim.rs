//! The simulation surface the server needs ([`NetSim`]) and its adapter over `wp_sim::Sim`
//! ([`WpSim`]).
//!
//! `wp_sim` is owned by the integration lead; everything the net layer needs that `Sim` does
//! not expose yet is listed in `docs/integration/net.md` ("Needs") and worked around here:
//! - **despawn**: `Sim` has none, so [`WpSim`] parks the vehicle (reset, hidden, braking) and
//!   reuses it for the next spawn of the same car at the same spawn point;
//! - **applied contacts**: `Sim` does not extract Rapier contacts yet and has no hook to
//!   quantise them before damage, so [`WpSim::drain_applied_contacts`] is always empty;
//! - **AI vehicles**: none exist yet, so every vehicle is spawned through the net layer.

use std::collections::{BTreeMap, BTreeSet};

use wp_contracts::{
    AssistProfile, ContactImpulse, DamageMode, DamageSim, EntityId, LoadedCar, SimEvent, TimeOfDay,
    VehicleInput, VehicleState, WorldQuery,
};
use wp_sim::{Sim, SimConfig};

use crate::protocol::EntityKind;

/// What the authoritative server needs from a simulation.
///
/// Contract for damage replication: every contact applied to a vehicle's damage model in the
/// last [`NetSim::step`] must be reported by [`NetSim::drain_applied_contacts`], and must have
/// been passed through [`crate::quant::QContact::round_trip`] *before* it was applied, so
/// clients re-running the quantised contacts reproduce the lattice bit-for-bit.
pub trait NetSim {
    /// Physics ticks run so far.
    fn tick(&self) -> u64;
    /// Spawn a car at `pos` with heading `yaw` (rad about +Z, 0 = +Y).
    fn spawn(&mut self, car_id: &str, pos: [f32; 3], yaw: f32) -> Result<EntityId, String>;
    /// Remove a vehicle from the world.
    fn despawn(&mut self, id: EntityId);
    /// Input applied from the next step on.
    fn set_input(&mut self, id: EntityId, input: VehicleInput);
    /// Assists for a vehicle.
    fn set_assists(&mut self, id: EntityId, assists: AssistProfile);
    /// Run exactly one physics tick.
    fn step(&mut self);
    /// Every live vehicle's state, ascending by entity.
    fn vehicles(&self) -> Vec<(EntityId, VehicleState)>;
    /// Car id of a live vehicle.
    fn car_id(&self, id: EntityId) -> Option<String>;
    /// Current lattice node positions of a vehicle (chassis space).
    fn lattice_nodes(&self, id: EntityId) -> Option<Vec<[f32; 3]>>;
    /// Rest lattice node positions of a vehicle (chassis space).
    fn lattice_rest(&self, id: EntityId) -> Option<Vec<[f32; 3]>>;
    /// Events emitted since the last drain.
    fn drain_events(&mut self) -> Vec<SimEvent>;
    /// Contacts applied to damage models during the last step (see the trait docs).
    fn drain_applied_contacts(&mut self) -> Vec<(EntityId, Vec<ContactImpulse>)>;
    /// In-game time.
    fn time_of_day(&self) -> TimeOfDay;
    /// Damage mode of the instance.
    fn damage_mode(&self) -> DamageMode;
    /// Kind of a vehicle the net layer did not spawn (AI). Default: traffic.
    fn ai_kind(&self, _id: EntityId) -> EntityKind {
        EntityKind::Traffic
    }
    /// Wanted level 0..=5 of a vehicle (from `wp_police`). Default: 0.
    fn wanted_level(&self, _id: EntityId) -> u8 {
        0
    }
}

#[derive(Debug, Clone)]
struct Spawned {
    car_id: String,
    pos: [f32; 3],
    yaw: f32,
}

/// [`NetSim`] over the real `wp_sim::Sim`.
pub struct WpSim {
    sim: Sim,
    cars: BTreeMap<String, LoadedCar>,
    spawned: BTreeMap<EntityId, Spawned>,
    parked: BTreeSet<EntityId>,
    mode: DamageMode,
}

impl WpSim {
    /// Wrap a new `Sim` with the cars players may drive.
    pub fn new(config: SimConfig, cars: BTreeMap<String, LoadedCar>) -> Self {
        Self {
            mode: config.mode.mode,
            sim: Sim::new(config),
            cars,
            spawned: BTreeMap::new(),
            parked: BTreeSet::new(),
        }
    }

    /// The wrapped simulation.
    pub fn sim(&self) -> &Sim {
        &self.sim
    }
}

impl NetSim for WpSim {
    fn tick(&self) -> u64 {
        self.sim.tick()
    }

    fn spawn(&mut self, car_id: &str, pos: [f32; 3], yaw: f32) -> Result<EntityId, String> {
        let reusable = self.parked.iter().copied().find(|id| {
            let s = &self.spawned[id];
            s.car_id == car_id && s.pos == pos && s.yaw == yaw
        });
        if let Some(id) = reusable {
            self.parked.remove(&id);
            self.sim.reset_vehicle(id).map_err(|e| e.to_string())?;
            self.sim
                .set_input(id, VehicleInput::default())
                .map_err(|e| e.to_string())?;
            return Ok(id);
        }
        let car = self
            .cars
            .get(car_id)
            .ok_or_else(|| format!("unknown car {car_id}"))?;
        let id = self
            .sim
            .spawn_vehicle(car, pos, yaw)
            .map_err(|e| e.to_string())?;
        self.spawned.insert(
            id,
            Spawned {
                car_id: car_id.to_string(),
                pos,
                yaw,
            },
        );
        Ok(id)
    }

    fn despawn(&mut self, id: EntityId) {
        if self.sim.reset_vehicle(id).is_ok() {
            let _ = self.sim.set_input(
                id,
                VehicleInput {
                    brake: 1.0,
                    ..Default::default()
                },
            );
            self.parked.insert(id);
        }
    }

    fn set_input(&mut self, id: EntityId, input: VehicleInput) {
        let _ = self.sim.set_input(id, input);
    }

    fn set_assists(&mut self, id: EntityId, assists: AssistProfile) {
        let _ = self.sim.set_assists(id, assists);
    }

    fn step(&mut self) {
        self.sim.step_fixed();
    }

    fn vehicles(&self) -> Vec<(EntityId, VehicleState)> {
        self.sim
            .vehicle_states()
            .into_iter()
            .filter(|(id, _)| !self.parked.contains(id))
            .collect()
    }

    fn car_id(&self, id: EntityId) -> Option<String> {
        if self.parked.contains(&id) {
            return None;
        }
        self.sim
            .vehicles()
            .iter()
            .find(|v| v.id == id)
            .map(|v| v.car_id.clone())
    }

    fn lattice_nodes(&self, id: EntityId) -> Option<Vec<[f32; 3]>> {
        self.sim
            .vehicles()
            .iter()
            .find(|v| v.id == id)
            .map(|v| v.damage.node_positions().to_vec())
    }

    fn lattice_rest(&self, id: EntityId) -> Option<Vec<[f32; 3]>> {
        self.sim
            .vehicles()
            .iter()
            .find(|v| v.id == id)
            .map(|v| v.damage.lattice().nodes.iter().map(|n| n.pos_m).collect())
    }

    fn drain_events(&mut self) -> Vec<SimEvent> {
        self.sim.drain_events()
    }

    fn drain_applied_contacts(&mut self) -> Vec<(EntityId, Vec<ContactImpulse>)> {
        Vec::new()
    }

    fn time_of_day(&self) -> TimeOfDay {
        self.sim.world().time_of_day()
    }

    fn damage_mode(&self) -> DamageMode {
        self.mode
    }
}
