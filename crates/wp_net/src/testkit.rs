//! Test support: a crash arena that produces real contact traffic for damage replication.
//!
//! `wp_sim` does not yet turn Rapier contacts into damage, and the `wp_damage` stub does not
//! deform, so on the real stack damage replication would carry nothing. [`ArenaSim`] wraps
//! [`WpSim`] (vehicle motion is the real `wp_vehicle` code) and adds:
//! - a circular wall: leaving the disc produces a wall contact;
//! - car-to-car proximity contacts (cars still pass through each other);
//! - a [`ToyLattice`] per vehicle that dents deterministically from those contacts.
//!
//! It follows the [`NetSim`] contract exactly: contacts are quantised with
//! [`QContact::round_trip`] *before* being applied, and every applied batch is reported. It is
//! used by the lockstep/bandwidth tests and by `wp-server --arena` for load tests.

use std::collections::{BTreeMap, BTreeSet};

use wp_contracts::{
    AssistProfile, Component, ContactImpulse, DamageMode, EntityId, HandlingEffects, LoadedCar,
    ModeTable, SimEvent, TimeOfDay, VehicleInput, VehicleState, PHYSICS_DT,
};
use wp_sim::SimConfig;

use crate::lattice::ReplicaDamage;
use crate::netsim::{NetSim, WpSim};
use crate::protocol::EntityKind;
use crate::quant::{rotate_inv, QContact};

/// Deterministic dent model: each contact pushes nodes within 0.8 m of the contact point along
/// the contact normal, up to 0.25 m, scaled by impulse. No dynamics, no handling effects
/// (the arena's vehicles are not affected by damage, so replicas must not be either).
#[derive(Debug, Clone)]
pub struct ToyLattice {
    rest: Vec<[f32; 3]>,
    nodes: Vec<[f32; 3]>,
}

impl ToyLattice {
    /// Pristine lattice for a car.
    pub fn for_car(car: &LoadedCar) -> Self {
        let rest: Vec<[f32; 3]> = car.lattice.nodes.iter().map(|n| n.pos_m).collect();
        Self {
            nodes: rest.clone(),
            rest,
        }
    }

    /// Factory for [`crate::client::ClientConfig::damage_factory`].
    pub fn factory(car: &LoadedCar) -> Box<dyn ReplicaDamage> {
        Box::new(Self::for_car(car))
    }
}

impl ReplicaDamage for ToyLattice {
    fn apply_contacts(
        &mut self,
        contacts: &[ContactImpulse],
        mode: &ModeTable,
        _dt: f32,
        _out: &mut Vec<SimEvent>,
    ) {
        for c in contacts {
            let depth = (c.impulse_ns * mode.impulse_to_lattice_scale / 40_000.0).min(0.25);
            for n in &mut self.nodes {
                let d = ((n[0] - c.point_local_m[0]).powi(2)
                    + (n[1] - c.point_local_m[1]).powi(2)
                    + (n[2] - c.point_local_m[2]).powi(2))
                .sqrt();
                if d < 0.8 {
                    let w = depth * (1.0 - d / 0.8);
                    for (axis, v) in n.iter_mut().enumerate() {
                        *v += c.normal_local[axis] * w;
                    }
                }
            }
        }
    }

    fn node_positions(&self) -> &[[f32; 3]] {
        &self.nodes
    }

    fn rest_positions(&self) -> Vec<[f32; 3]> {
        self.rest.clone()
    }

    fn restore_nodes(&mut self, nodes: &[[f32; 3]]) {
        for (dst, src) in self.nodes.iter_mut().zip(nodes) {
            *dst = *src;
        }
    }

    fn handling_effects(&self) -> HandlingEffects {
        HandlingEffects::NEUTRAL
    }
}

/// Crash arena over the real vehicle sim. See the module docs.
pub struct ArenaSim {
    inner: WpSim,
    cars: BTreeMap<String, LoadedCar>,
    mode: ModeTable,
    damage: BTreeMap<EntityId, ToyLattice>,
    inside: BTreeMap<EntityId, bool>,
    touching: BTreeSet<(EntityId, EntityId)>,
    applied: Vec<(EntityId, Vec<ContactImpulse>)>,
    events: Vec<SimEvent>,
    /// Wall radius, m.
    pub radius_m: f32,
    /// Car-to-car contact distance, m.
    pub contact_dist_m: f32,
}

impl ArenaSim {
    /// Arena of `radius_m` around the origin.
    pub fn new(config: SimConfig, cars: BTreeMap<String, LoadedCar>, radius_m: f32) -> Self {
        Self {
            mode: config.mode.clone(),
            inner: WpSim::new(config, cars.clone()),
            cars,
            damage: BTreeMap::new(),
            inside: BTreeMap::new(),
            touching: BTreeSet::new(),
            applied: Vec::new(),
            events: Vec::new(),
            radius_m,
            contact_dist_m: 4.0,
        }
    }

    fn contact(
        &self,
        s: &VehicleState,
        normal_world: [f32; 3],
        closing: f32,
        other: Option<(EntityId, f32)>,
    ) -> ContactImpulse {
        let n = rotate_inv(s.rot, normal_world);
        // Hit point on the body surface opposite the normal (normal points into the car).
        let point = [-n[0] * 0.9, -n[1] * 2.3, 0.45];
        let impulse = 1400.0 * closing.abs() * if other.is_some() { 0.5 } else { 1.0 } + 300.0;
        QContact::round_trip(&ContactImpulse {
            point_local_m: point,
            normal_local: n,
            impulse_ns: impulse,
            tangent_impulse_local_ns: [0.0, -0.2 * impulse * n[0], 0.0],
            rel_speed_mps: closing.abs(),
            other: other.map(|o| o.0),
            other_mass_kg: other.map_or(0.0, |o| o.1),
        })
    }
}

impl NetSim for ArenaSim {
    fn tick(&self) -> u64 {
        self.inner.tick()
    }

    fn spawn(&mut self, car_id: &str, pos: [f32; 3], yaw: f32) -> Result<EntityId, String> {
        let id = self.inner.spawn(car_id, pos, yaw)?;
        let car = self.cars.get(car_id).ok_or("unknown car")?;
        self.damage.insert(id, ToyLattice::for_car(car));
        Ok(id)
    }

    fn despawn(&mut self, id: EntityId) {
        self.inner.despawn(id);
        self.damage.remove(&id);
        self.inside.remove(&id);
        self.touching.retain(|(a, b)| *a != id && *b != id);
    }

    fn set_input(&mut self, id: EntityId, input: VehicleInput) {
        self.inner.set_input(id, input);
    }

    fn set_assists(&mut self, id: EntityId, assists: AssistProfile) {
        self.inner.set_assists(id, assists);
    }

    fn step(&mut self) {
        self.inner.step();
        let states = self.inner.vehicles();
        let mut contacts: BTreeMap<EntityId, Vec<ContactImpulse>> = BTreeMap::new();
        for (e, s) in &states {
            let r = (s.pos[0] * s.pos[0] + s.pos[1] * s.pos[1]).sqrt();
            let now_inside = r < self.radius_m;
            let was_inside = self.inside.insert(*e, now_inside).unwrap_or(true);
            if was_inside && !now_inside {
                let inward = [-s.pos[0] / r, -s.pos[1] / r, 0.0];
                let closing = s.lin_vel[0] * inward[0] + s.lin_vel[1] * inward[1];
                contacts
                    .entry(*e)
                    .or_default()
                    .push(self.contact(s, inward, closing, None));
            }
        }
        let mut touching = BTreeSet::new();
        for (i, (a, sa)) in states.iter().enumerate() {
            for (b, sb) in &states[i + 1..] {
                let d = [sa.pos[0] - sb.pos[0], sa.pos[1] - sb.pos[1]];
                let dist = (d[0] * d[0] + d[1] * d[1]).sqrt();
                if dist >= self.contact_dist_m || dist < 1e-3 {
                    continue;
                }
                touching.insert((*a, *b));
                if self.touching.contains(&(*a, *b)) {
                    continue;
                }
                let n_a = [d[0] / dist, d[1] / dist, 0.0]; // from b into a
                let rel = [sa.lin_vel[0] - sb.lin_vel[0], sa.lin_vel[1] - sb.lin_vel[1]];
                let closing = rel[0] * n_a[0] + rel[1] * n_a[1];
                contacts.entry(*a).or_default().push(self.contact(
                    sa,
                    n_a,
                    closing,
                    Some((*b, 1400.0)),
                ));
                let n_b = [-n_a[0], -n_a[1], 0.0];
                contacts.entry(*b).or_default().push(self.contact(
                    sb,
                    n_b,
                    closing,
                    Some((*a, 1400.0)),
                ));
            }
        }
        self.touching = touching;
        // Every vehicle's damage steps every tick, with or without contacts.
        let mut sink = Vec::new();
        for (e, lattice) in &mut self.damage {
            let batch = contacts.remove(e).unwrap_or_default();
            lattice.apply_contacts(&batch, &self.mode, PHYSICS_DT, &mut sink);
            if !batch.is_empty() {
                if batch.iter().any(|c| c.impulse_ns > 15_000.0) {
                    self.events.push(SimEvent::ComponentDamaged {
                        vehicle: *e,
                        component: Component::Radiator,
                        health: 0.5,
                    });
                }
                self.applied.push((*e, batch));
            }
        }
    }

    fn vehicles(&self) -> Vec<(EntityId, VehicleState)> {
        self.inner.vehicles()
    }

    fn car_id(&self, id: EntityId) -> Option<String> {
        self.inner.car_id(id)
    }

    fn lattice_nodes(&self, id: EntityId) -> Option<Vec<[f32; 3]>> {
        self.damage.get(&id).map(|d| d.nodes.clone())
    }

    fn lattice_rest(&self, id: EntityId) -> Option<Vec<[f32; 3]>> {
        self.damage.get(&id).map(|d| d.rest.clone())
    }

    fn drain_events(&mut self) -> Vec<SimEvent> {
        let mut ev = self.inner.drain_events();
        ev.append(&mut self.events);
        ev
    }

    fn drain_applied_contacts(&mut self) -> Vec<(EntityId, Vec<ContactImpulse>)> {
        std::mem::take(&mut self.applied)
    }

    fn time_of_day(&self) -> TimeOfDay {
        self.inner.time_of_day()
    }

    fn damage_mode(&self) -> DamageMode {
        self.inner.damage_mode()
    }

    fn ai_kind(&self, _id: EntityId) -> EntityKind {
        EntityKind::Traffic
    }

    fn vehicle_snapshot(&self, id: EntityId) -> Option<Vec<u8>> {
        self.inner.vehicle_snapshot(id)
    }
}

/// Cars by id and mode tables by mode, as the net layer consumes them.
pub type GameData = (BTreeMap<String, LoadedCar>, BTreeMap<DamageMode, ModeTable>);

/// Load the cars and mode tables the net layer uses from a `data/` directory:
/// `debug_sedan` (racers), `debug_coupe` (police), `arcade` and `sim` modes.
pub fn load_data(data_dir: &std::path::Path) -> Result<GameData, wp_contracts::LoadError> {
    let mut cars = BTreeMap::new();
    for id in ["debug_sedan", "debug_coupe"] {
        let car = wp_contracts::load_car(data_dir.join(format!("cars/{id}.json")))?;
        cars.insert(id.to_string(), car);
    }
    let mut modes = BTreeMap::new();
    for name in ["arcade", "sim"] {
        let m = wp_contracts::load_mode_table(data_dir.join(format!("modes/{name}.toml")))?;
        modes.insert(m.mode, m);
    }
    Ok((cars, modes))
}

/// The repository's `data/` directory (for tests and tools run from the workspace).
pub fn repo_data_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}
