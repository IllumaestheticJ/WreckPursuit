//! The simulation surface the server needs ([`NetSim`]) and its adapter over `wp_sim::Sim`
//! ([`WpSim`]).
//!
//! [`WpSim`] uses the `wp_sim` hooks the lead added for the net layer (NET-N1/N3/N4):
//! - **damage replication**: [`QContact::round_trip`] is installed as the sim's contact filter,
//!   so damage applies the exact values clients receive, and applied contacts are recorded and
//!   drained every step;
//! - **despawn**: `Sim::despawn_vehicle` (ids are never reused);
//! - **AI vehicles**: police units and physical traffic are ordinary sim vehicles; their
//!   `wp_sim::VehicleKind` becomes the replicated [`EntityKind`], and wanted levels come from
//!   the sim's police system.

use std::collections::BTreeMap;

use wp_contracts::{
    AssistProfile, ContactImpulse, DamageMode, DamageSim, EntityId, LoadedCar, SimEvent, TimeOfDay,
    VehicleInput, VehicleState, WorldQuery,
};
use wp_sim::{Sim, SimConfig, VehicleKind};

use crate::lattice::damage_relevant;
use crate::protocol::EntityKind;
use crate::quant::QContact;

/// What the authoritative server needs from a simulation.
///
/// Contract for damage replication: every contact applied to a vehicle's damage model in the
/// last [`NetSim::step`] must be reported by [`NetSim::drain_applied_contacts`] (contacts the
/// damage model provably ignores may be left out, see [`crate::lattice::damage_relevant`]),
/// and must have been passed through [`crate::quant::QContact::round_trip`] *before* it was
/// applied, so clients re-running the quantised contacts reproduce the lattice bit-for-bit.
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
    /// Bit-exact full state of a vehicle (`wp_vehicle::VehicleSnapshot` bytes) for the owning
    /// client's prediction rewind. Default: none (clients fall back to `set_state`).
    fn vehicle_snapshot(&self, _id: EntityId) -> Option<Vec<u8>> {
        None
    }
    /// Damage steps (`DamageSim::apply_contacts` calls) a vehicle's model has run since it was
    /// created. Sent with lattice snapshots so replicas can match the solver phase. Default:
    /// unknown.
    fn damage_steps(&self, _id: EntityId) -> Option<u64> {
        None
    }
    /// Tell the sim who drives a vehicle the net layer spawned (racer seat or police seat).
    /// Default: ignored.
    fn set_owned_kind(&mut self, _id: EntityId, _kind: EntityKind) {}
}

/// [`NetSim`] over the real `wp_sim::Sim`.
pub struct WpSim {
    sim: Sim,
    cars: BTreeMap<String, LoadedCar>,
    mode: DamageMode,
    /// Damage steps run per live vehicle (see [`NetSim::damage_steps`]).
    damage_steps: BTreeMap<EntityId, u64>,
}

impl WpSim {
    /// Wrap a new, empty `Sim` (flat world) with the cars players may drive.
    pub fn new(config: SimConfig, cars: BTreeMap<String, LoadedCar>) -> Self {
        Self::from_sim(Sim::new(config), cars)
    }

    /// Wrap an already built `Sim` (e.g. with a city, police and traffic installed, as
    /// `simcli::run::build_sim` does) with the cars players may drive. Installs the contact
    /// quantiser and turns on applied-contact recording.
    pub fn from_sim(mut sim: Sim, cars: BTreeMap<String, LoadedCar>) -> Self {
        sim.set_contact_filter(Some(quantise_contact));
        sim.set_record_applied_contacts(true);
        let damage_steps = sim.vehicles().iter().map(|v| (v.id, 0)).collect();
        Self {
            mode: sim.mode_table().mode,
            sim,
            cars,
            damage_steps,
        }
    }

    /// The wrapped simulation.
    pub fn sim(&self) -> &Sim {
        &self.sim
    }

    /// The wrapped simulation, mutable (scenario setup, heat injection in tests).
    pub fn sim_mut(&mut self) -> &mut Sim {
        &mut self.sim
    }

    fn slot(&self, id: EntityId) -> Option<&wp_sim::SimVehicle> {
        self.sim.vehicles().iter().find(|v| v.id == id)
    }
}

/// `wp_sim` contact filter: replace a contact with its wire round trip, so the authority's
/// damage applies exactly what clients will replay.
fn quantise_contact(c: &mut ContactImpulse) {
    *c = QContact::round_trip(c);
}

/// Replicated kind of a sim vehicle the net layer did not spawn.
pub fn entity_kind(kind: VehicleKind) -> EntityKind {
    match kind {
        // A `Player` vehicle no client owns (e.g. scenario setup): show it as a racer.
        VehicleKind::Player => EntityKind::Racer,
        VehicleKind::AiRacer => EntityKind::AiRacer,
        VehicleKind::Police => EntityKind::AiPolice,
        VehicleKind::Traffic => EntityKind::Traffic,
    }
}

impl NetSim for WpSim {
    fn tick(&self) -> u64 {
        self.sim.tick()
    }

    fn spawn(&mut self, car_id: &str, pos: [f32; 3], yaw: f32) -> Result<EntityId, String> {
        let car = self
            .cars
            .get(car_id)
            .ok_or_else(|| format!("unknown car {car_id}"))?;
        let id = self
            .sim
            .spawn_vehicle(car, pos, yaw)
            .map_err(|e| e.to_string())?;
        self.damage_steps.insert(id, 0);
        Ok(id)
    }

    fn despawn(&mut self, id: EntityId) {
        let _ = self.sim.despawn_vehicle(id);
        self.damage_steps.remove(&id);
    }

    fn set_input(&mut self, id: EntityId, input: VehicleInput) {
        let _ = self.sim.set_input(id, input);
    }

    fn set_assists(&mut self, id: EntityId, assists: AssistProfile) {
        let _ = self.sim.set_assists(id, assists);
    }

    fn step(&mut self) {
        self.sim.step_fixed();
        // Count damage steps. Vehicles that existed before the tick all ran one. Vehicles the
        // sim spawned during the tick: physical traffic is embodied before the damage step
        // (`apply_traffic_lod` runs with the world step), police cars after it
        // (`reconcile_police_units` runs after the police step). Checked against
        // `DamageModel::state_hash` (which includes the step count) in the tests below.
        let mut next = BTreeMap::new();
        for v in self.sim.vehicles() {
            let steps = match self.damage_steps.get(&v.id) {
                Some(n) => n + 1,
                None if v.traffic_agent.is_some() => 1,
                None => 0,
            };
            next.insert(v.id, steps);
        }
        self.damage_steps = next;
    }

    fn vehicles(&self) -> Vec<(EntityId, VehicleState)> {
        self.sim.vehicle_states()
    }

    fn car_id(&self, id: EntityId) -> Option<String> {
        self.slot(id).map(|v| v.car_id.clone())
    }

    fn lattice_nodes(&self, id: EntityId) -> Option<Vec<[f32; 3]>> {
        self.slot(id).map(|v| v.damage.node_positions().to_vec())
    }

    fn lattice_rest(&self, id: EntityId) -> Option<Vec<[f32; 3]>> {
        self.slot(id)
            .map(|v| v.damage.lattice().nodes.iter().map(|n| n.pos_m).collect())
    }

    fn drain_events(&mut self) -> Vec<SimEvent> {
        self.sim.drain_events()
    }

    fn vehicle_snapshot(&self, id: EntityId) -> Option<Vec<u8>> {
        self.slot(id)
            .map(|v| v.vehicle.snapshot().as_bytes().to_vec())
    }

    fn drain_applied_contacts(&mut self) -> Vec<(EntityId, Vec<ContactImpulse>)> {
        let mode = self.sim.mode_table().clone();
        let mut out: Vec<(EntityId, Vec<ContactImpulse>)> = Vec::new();
        for (id, batch) in self.sim.drain_applied_contacts() {
            // Resting / scraping contacts that cannot dent anything are not replicated.
            let relevant = match self.slot(id) {
                Some(v) => damage_relevant(&batch, v.damage.tuning(), &mode),
                None => batch,
            };
            if relevant.is_empty() {
                continue;
            }
            // Several batches of one vehicle in one step stay separate (they were separate
            // damage steps); `wp_sim` records one per vehicle per tick.
            out.push((id, relevant));
        }
        out
    }

    fn time_of_day(&self) -> TimeOfDay {
        self.sim.world().time_of_day()
    }

    fn damage_mode(&self) -> DamageMode {
        self.mode
    }

    fn ai_kind(&self, id: EntityId) -> EntityKind {
        self.slot(id)
            .map(|v| entity_kind(v.kind))
            .unwrap_or(EntityKind::Traffic)
    }

    fn damage_steps(&self, id: EntityId) -> Option<u64> {
        self.damage_steps.get(&id).copied()
    }

    fn wanted_level(&self, id: EntityId) -> u8 {
        self.sim.police().wanted_level(id).number()
    }

    fn set_owned_kind(&mut self, id: EntityId, kind: EntityKind) {
        // Player police seats must not be treated as racers by `wp_police` (they would earn
        // heat and be chased). `VehicleKind::Police` also drops them from the traffic spawn
        // ring; see NET-N9 in docs/integration/net.md.
        let k = if kind.is_police() {
            VehicleKind::Police
        } else {
            VehicleKind::Player
        };
        let _ = self.sim.set_vehicle_kind(id, k);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{load_data, repo_data_dir};

    /// [`WpSim`]'s damage-step count matches the real models' (`state_hash` includes it) for
    /// player cars, physical traffic and AI police spawned mid-tick.
    #[test]
    fn damage_steps_match_the_models() {
        let (mut cars, modes) = load_data(&repo_data_dir()).unwrap();
        let police =
            wp_contracts::load_car(repo_data_dir().join("cars/sentinel_cruiser.json")).unwrap();
        cars.insert("sentinel_cruiser".into(), police.clone());
        let mode = modes[&DamageMode::Arcade].clone();
        let mut sim = Sim::new(SimConfig {
            seed: 3,
            mode: mode.clone(),
            time_of_day: TimeOfDay::from_hours(23.0),
            time_ratio: 1.0,
        });
        let pg = wp_world::procgen::ProcGenConfig {
            nx: 6,
            ny: 6,
            spacing_m: 110.0,
            seed: 1,
            ..Default::default()
        };
        sim.set_world(wp_world::World::procedural(
            &pg,
            wp_world::WorldConfig {
                seed: 3,
                start_time: TimeOfDay::from_hours(23.0),
                time_ratio: 1.0,
                traffic: wp_world::TrafficConfig {
                    enabled: true,
                    ..Default::default()
                },
                parked_cars: false,
            },
        ));
        sim.set_police_car(police);
        sim.set_traffic_car(cars["debug_sedan"].clone());
        let mut net = WpSim::from_sim(sim, cars.clone());
        let me = net
            .spawn("debug_coupe", [53.25, 300.0, 0.0], std::f32::consts::PI)
            .unwrap();
        for _ in 0..3 {
            net.step();
        }
        net.set_input(
            me,
            VehicleInput {
                throttle: 0.3,
                ..Default::default()
            },
        );
        let (mut traffic, mut police, mut checked) = (0, 0, 0);
        for _ in 0..1200 {
            net.step();
        }
        for v in net.sim().vehicles() {
            let pristine = v
                .damage
                .node_positions()
                .iter()
                .zip(&v.damage.lattice().nodes)
                .all(|(a, b)| *a == b.pos_m);
            if !pristine {
                continue;
            }
            let car = &cars[&v.car_id];
            let mut fresh = wp_damage::DamageModel::new(&car.lattice, &car.materials);
            let mut sink = Vec::new();
            for _ in 0..net.damage_steps(v.id).unwrap() {
                fresh.apply_contacts(&[], &mode, wp_contracts::PHYSICS_DT, &mut sink);
            }
            assert_eq!(
                fresh.state_hash(),
                v.damage.state_hash(),
                "vehicle {:?} ({:?}) step count",
                v.id,
                v.kind
            );
            checked += 1;
            traffic += usize::from(v.kind == VehicleKind::Traffic);
            police += usize::from(v.kind == VehicleKind::Police);
        }
        assert!(
            checked >= 3 && traffic >= 1 && police >= 1,
            "{checked} {traffic} {police}"
        );
    }
}
