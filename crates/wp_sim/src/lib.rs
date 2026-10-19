//! # wp_sim
//!
//! The fixed-step game loop (owned by the integration lead). Each physics tick
//! ([`wp_contracts::PHYSICS_DT`]) it: applies inputs, steps the world clock/traffic, steps
//! every vehicle, feeds contacts to damage, pushes handling effects back to vehicles, steps
//! police, and collects events. **Current state:** vehicles are `wp_vehicle` rigid bodies
//! (wheel probes via the Rapier query pipeline); chassis contacts are extracted from Rapier
//! manifolds into `ContactImpulse`s for `wp_damage` and `SimEvent::Contact` events
//! (`contacts.rs`); police and economy are stubs.

#![warn(missing_docs)]

mod contacts;
mod physics;
mod systems;

pub use contacts::CONTACT_EVENT_MIN_IMPULSE_NS;
pub use physics::PhysicsWorld;
pub use systems::{SystemId, Systems};

use rapier3d::prelude::QueryFilter;
use thiserror::Error;
use wp_contracts::{
    AssistProfile, DamageSim, EntityId, LoadedCar, ModeTable, SimEvent, TimeOfDay, VehicleInput,
    VehicleSim, VehicleState, MAX_VEHICLES, PHYSICS_DT,
};
use wp_damage::DamageModel;
use wp_math::{FixedStep, Rng, StateHasher};
use wp_police::{PoliceSystem, RacerView};
use wp_vehicle::{RapierGround, Vehicle, VehicleBody};
use wp_world::World;

/// Errors from simulation control calls.
#[derive(Debug, Error, PartialEq)]
pub enum SimError {
    /// [`MAX_VEHICLES`] reached.
    #[error("vehicle limit ({MAX_VEHICLES}) reached")]
    TooManyVehicles,
    /// No vehicle with this id.
    #[error("unknown entity {0:?}")]
    UnknownEntity(EntityId),
}

/// Parameters for a new simulation instance.
#[derive(Debug, Clone)]
pub struct SimConfig {
    /// Master RNG seed.
    pub seed: u64,
    /// Damage mode table (Arcade or Simulation).
    pub mode: ModeTable,
    /// Starting time of day.
    pub time_of_day: TimeOfDay,
    /// In-game seconds per simulated second.
    pub time_ratio: f32,
}

/// One vehicle slot in the simulation.
pub struct SimVehicle {
    /// Entity id.
    pub id: EntityId,
    /// Car id (`VehicleDef::id`).
    pub car_id: String,
    /// Vehicle dynamics.
    pub vehicle: Vehicle,
    /// Damage model.
    pub damage: DamageModel,
    /// Input applied every tick until changed.
    pub input: VehicleInput,
    /// Assists in effect.
    pub assists: AssistProfile,
    /// Rapier handles of the chassis.
    pub body: VehicleBody,
    spawn: ([f32; 3], f32),
}

/// A complete simulation instance. Deterministic given the same config, spawns and inputs.
pub struct Sim {
    config: SimConfig,
    tick: u64,
    next_entity: u32,
    rng: Rng,
    fixed: FixedStep,
    world: World,
    physics: PhysicsWorld,
    police: PoliceSystem,
    vehicles: Vec<SimVehicle>,
    events: Vec<SimEvent>,
    systems: Systems,
    last_tick_events: usize,
    static_boxes: usize,
}

impl Sim {
    /// New, empty simulation.
    pub fn new(config: SimConfig) -> Self {
        Self {
            rng: Rng::new(config.seed),
            fixed: FixedStep::new(PHYSICS_DT, 240),
            world: World::flat(config.time_of_day, config.time_ratio),
            physics: PhysicsWorld::new(),
            police: PoliceSystem::new(),
            config,
            tick: 0,
            next_entity: 1,
            vehicles: Vec::new(),
            events: Vec::new(),
            systems: Systems::ALL,
            last_tick_events: 0,
            static_boxes: 0,
        }
    }

    /// Choose which systems [`Sim::step_fixed`] runs (harness hook; default [`Systems::ALL`]).
    /// A disabled system keeps its state frozen: a disabled vehicle system leaves every car where
    /// it is, a disabled damage system feeds no contacts and pushes no handling effects, a
    /// disabled world freezes the clock, a disabled police system sees nothing.
    pub fn set_systems(&mut self, systems: Systems) {
        self.systems = systems;
        // A frozen vehicle system also takes the chassis bodies out of the physics step, so
        // they neither fall nor produce contacts.
        for v in &self.vehicles {
            self.physics.bodies[v.body.body].set_enabled(systems.vehicle);
        }
    }

    /// Systems currently stepped.
    pub fn systems(&self) -> Systems {
        self.systems
    }

    /// Spawn a car at world position `pos` with heading `yaw` (rad about +Z, 0 = +Y).
    pub fn spawn_vehicle(
        &mut self,
        car: &LoadedCar,
        pos: [f32; 3],
        yaw: f32,
    ) -> Result<EntityId, SimError> {
        if self.vehicles.len() >= MAX_VEHICLES {
            return Err(SimError::TooManyVehicles);
        }
        let id = EntityId(self.next_entity);
        self.next_entity += 1;
        let mut vehicle = Vehicle::new(&car.def, &car.tires, pos, yaw);
        vehicle.set_entity(id);
        let body = vehicle.build_body(&mut self.physics.bodies, &mut self.physics.colliders);
        self.physics.bodies[body.body].set_enabled(self.systems.vehicle);
        self.vehicles.push(SimVehicle {
            id,
            car_id: car.def.id.clone(),
            body,
            vehicle,
            damage: DamageModel::new(&car.lattice, &car.materials),
            input: VehicleInput::default(),
            assists: AssistProfile::default(),
            spawn: (pos, yaw),
        });
        Ok(id)
    }

    /// Teleport a vehicle to `pos`/`yaw`, rolling forward at `speed_mps` (wheels and engine
    /// spun up to match; 0 = at rest). Scenario setup: lets crash tests start at speed instead
    /// of a long run-up. Damage is kept; use [`Sim::reset_vehicle`] for a pristine car.
    pub fn place_vehicle(
        &mut self,
        id: EntityId,
        pos: [f32; 3],
        yaw: f32,
        speed_mps: f32,
    ) -> Result<(), SimError> {
        let slot = self
            .vehicles
            .iter_mut()
            .find(|v| v.id == id)
            .ok_or(SimError::UnknownEntity(id))?;
        let rb = &mut self.physics.bodies[slot.body.body];
        slot.vehicle.place_body(rb, pos, yaw, speed_mps, 0.0);
        Ok(())
    }

    /// Add a static box to the world (barrier, wall), centre `pos` (world m), full `size`
    /// `[x, y, z]` m, heading `yaw` rad about +Z. Contacts with it reach damage as static world
    /// geometry (`other: None`, `other_mass_kg: 0.0`; events name [`wp_contracts::WORLD_ENTITY`]).
    /// Static boxes are scenario setup: add them before the first tick so replays match.
    pub fn add_static_box(&mut self, pos: [f32; 3], size: [f32; 3], yaw: f32) {
        use rapier3d::math::Rotation;
        use rapier3d::prelude::{ColliderBuilder, Pose, Vector};
        let (s, c) = wp_math::sin_cos(yaw * 0.5);
        let pose = Pose::from_parts(Vector::from_array(pos), Rotation::from_xyzw(0.0, 0.0, s, c));
        let col = ColliderBuilder::cuboid(size[0] * 0.5, size[1] * 0.5, size[2] * 0.5)
            .position(pose)
            .friction(0.6)
            .restitution(0.05)
            .build();
        self.physics.colliders.insert(col);
        self.static_boxes += 1;
    }

    /// Number of static boxes added with [`Sim::add_static_box`].
    pub fn static_box_count(&self) -> usize {
        self.static_boxes
    }

    /// Swap the damage mode table (Arcade ↔ Simulation) without restarting. Applies from the
    /// next tick; existing damage is kept. Log it as an input-like op if replays must match.
    pub fn set_mode_table(&mut self, mode: ModeTable) {
        self.config.mode = mode;
    }

    /// The damage mode table in effect.
    pub fn mode_table(&self) -> &ModeTable {
        &self.config.mode
    }

    fn slot_mut(&mut self, id: EntityId) -> Result<&mut SimVehicle, SimError> {
        self.vehicles
            .iter_mut()
            .find(|v| v.id == id)
            .ok_or(SimError::UnknownEntity(id))
    }

    /// Set the input a vehicle uses from the next tick on.
    pub fn set_input(&mut self, id: EntityId, input: VehicleInput) -> Result<(), SimError> {
        self.slot_mut(id)?.input = input;
        Ok(())
    }

    /// Set a vehicle's assist profile.
    pub fn set_assists(&mut self, id: EntityId, assists: AssistProfile) -> Result<(), SimError> {
        self.slot_mut(id)?.assists = assists;
        Ok(())
    }

    /// Return a vehicle to its spawn pose, pristine.
    pub fn reset_vehicle(&mut self, id: EntityId) -> Result<(), SimError> {
        let slot = self
            .vehicles
            .iter_mut()
            .find(|v| v.id == id)
            .ok_or(SimError::UnknownEntity(id))?;
        let (pos, yaw) = slot.spawn;
        let rb = &mut self.physics.bodies[slot.body.body];
        slot.vehicle.place_body(rb, pos, yaw, 0.0, 0.0);
        slot.damage.reset();
        Ok(())
    }

    /// Run exactly one fixed physics tick.
    pub fn step_fixed(&mut self) {
        let dt = PHYSICS_DT;
        let sys = self.systems;
        let mut out = Vec::new();
        if sys.world {
            self.world.step(dt, &mut out);
        }
        if sys.vehicle {
            for slot in &mut self.vehicles {
                let filter = QueryFilter::default().exclude_rigid_body(slot.body.body);
                let ground = RapierGround {
                    query: self.physics.query_pipeline(filter),
                };
                slot.vehicle.step_with_ground(
                    &slot.input,
                    &slot.assists,
                    &self.world,
                    &ground,
                    dt,
                    &mut out,
                );
            }
            for slot in &self.vehicles {
                slot.vehicle
                    .write_body(&mut self.physics.bodies[slot.body.body]);
            }
        }
        let infos: Vec<contacts::BodyInfo> = self
            .vehicles
            .iter()
            .map(|v| {
                let rb = &self.physics.bodies[v.body.body];
                contacts::BodyInfo {
                    id: v.id,
                    body: v.body.body,
                    collider: v.body.collider,
                    pre_linvel: rb.linvel(),
                    pre_angvel: rb.angvel(),
                    pre_com: rb.center_of_mass(),
                }
            })
            .collect();
        self.physics.step();
        let tick_contacts = contacts::collect(
            self.physics.narrow_phase(),
            &self.physics.bodies,
            &self.physics.colliders,
            &infos,
        );
        out.extend(tick_contacts.events);
        if sys.vehicle {
            for slot in &mut self.vehicles {
                slot.vehicle.read_body(&self.physics.bodies[slot.body.body]);
            }
        }
        if sys.damage {
            for (slot, contacts) in self.vehicles.iter_mut().zip(&tick_contacts.per_vehicle) {
                slot.damage
                    .apply_contacts(contacts, &self.config.mode, dt, &mut out);
                let effects = slot.damage.handling_effects();
                slot.vehicle.apply_damage_effects(&effects);
            }
        }
        if sys.police {
            let racers: Vec<RacerView> = self
                .vehicles
                .iter()
                .map(|v| {
                    let s = v.vehicle.state();
                    RacerView {
                        entity: v.id,
                        pos: s.pos,
                        vel: s.lin_vel,
                    }
                })
                .collect();
            let tick_events = out.clone();
            self.police
                .step(&self.world, &racers, &tick_events, dt, &mut out);
        }
        self.last_tick_events = out.len();
        self.events.extend(out);
        self.tick += 1;
    }

    /// Add `frame_dt` seconds of wall time (supplied by the caller), run the whole ticks it
    /// covers, and return the render interpolation alpha `0..1`.
    pub fn advance(&mut self, frame_dt: f32) -> f32 {
        let ticks = self.fixed.advance(frame_dt);
        for _ in 0..ticks {
            self.step_fixed();
        }
        self.fixed.alpha()
    }

    /// Physics ticks run so far.
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// The simulation's master RNG (systems should `fork()` their own stream).
    pub fn rng_mut(&mut self) -> &mut Rng {
        &mut self.rng
    }

    /// The world (clock, roads, traffic).
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Vehicle slots in ascending entity order.
    pub fn vehicles(&self) -> &[SimVehicle] {
        &self.vehicles
    }

    /// State of every vehicle, ascending by entity id. Health and mode come from damage.
    pub fn vehicle_states(&self) -> Vec<(EntityId, VehicleState)> {
        self.vehicles
            .iter()
            .map(|v| {
                let mut s = v.vehicle.state();
                s.health = v.damage.health();
                s.mode = self.config.mode.mode;
                (v.id, s)
            })
            .collect()
    }

    /// Take all events emitted since the last drain.
    pub fn drain_events(&mut self) -> Vec<SimEvent> {
        std::mem::take(&mut self.events)
    }

    /// Number of events emitted by the most recent [`Sim::step_fixed`] (harness hook: event
    /// flood detection without draining).
    pub fn last_tick_event_count(&self) -> usize {
        self.last_tick_events
    }

    /// Damage-model node positions (chassis space) of a vehicle, indexed by node id.
    pub fn node_positions(&self, id: EntityId) -> Result<&[[f32; 3]], SimError> {
        self.vehicles
            .iter()
            .find(|v| v.id == id)
            .map(|v| v.damage.node_positions())
            .ok_or(SimError::UnknownEntity(id))
    }

    /// Stable hash of the whole simulation state (hash spec v1, see `docs/integration/harness.md`):
    /// FNV-1a 64 over `HASH_SPEC_VERSION`, tick, in-game clock (1 ms), vehicle count, then every
    /// [`Sim::entity_hashes`] value in ascending entity order.
    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        h.write_u32(HASH_SPEC_VERSION);
        h.write_u64(self.tick);
        h.write_f32_q(self.world.clock.time.seconds_since_midnight, 1e3);
        h.write_u32(self.vehicles.len() as u32);
        for (_, eh) in self.entity_hashes() {
            h.write_u64(eh);
        }
        h.finish()
    }

    /// Per-vehicle state hashes in ascending entity order. Each is FNV-1a 64 over: entity id;
    /// quantised pos (1e-4 m), rot (1e-5), linear and angular velocity (1e-4); rpm (1 rpm);
    /// gear; health (1e-4); per wheel (FL, FR, RL, RR) omega (1e-3 rad/s), steer (1e-5 rad),
    /// compression (1e-5 m), detached flag; node count and every damage node position (1e-4 m).
    /// Used by the harness to name the entity that diverged first.
    pub fn entity_hashes(&self) -> Vec<(EntityId, u64)> {
        self.vehicles
            .iter()
            .map(|v| {
                let mut s = v.vehicle.state();
                s.health = v.damage.health();
                let mut h = StateHasher::new();
                h.write_u32(v.id.0);
                for x in s.pos {
                    h.write_f32_q(x, 1e4);
                }
                for x in s.rot {
                    h.write_f32_q(x, 1e5);
                }
                for x in s.lin_vel.iter().chain(s.ang_vel.iter()) {
                    h.write_f32_q(*x, 1e4);
                }
                h.write_f32_q(s.engine_rpm, 1.0);
                h.write_i64(s.gear as i64);
                h.write_f32_q(s.health, 1e4);
                for w in &s.wheel_states {
                    h.write_f32_q(w.omega, 1e3);
                    h.write_f32_q(w.steer_rad, 1e5);
                    h.write_f32_q(w.compression_m, 1e5);
                    h.write_bytes(&[w.detached as u8]);
                }
                let nodes = v.damage.node_positions();
                h.write_u32(nodes.len() as u32);
                for n in nodes {
                    for x in n {
                        h.write_f32_q(*x, 1e4);
                    }
                }
                (v.id, h.finish())
            })
            .collect()
    }
}

/// Version of the [`Sim::state_hash`] layout. Bump when the hashed fields change; every golden
/// must then be re-blessed.
pub const HASH_SPEC_VERSION: u32 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    fn data(path: &str) -> String {
        format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
    }

    fn run(seed: u64) -> (u64, VehicleState) {
        let car = wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap();
        let mode = wp_contracts::load_mode_table(data("modes/arcade.toml")).unwrap();
        let mut sim = Sim::new(SimConfig {
            seed,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 10.0,
        });
        let id = sim.spawn_vehicle(&car, [0.0; 3], 0.0).unwrap();
        sim.set_input(
            id,
            VehicleInput {
                throttle: 1.0,
                steer: 0.3,
                ..Default::default()
            },
        )
        .unwrap();
        for _ in 0..480 {
            sim.step_fixed();
        }
        let state = sim.vehicle_states()[0].1.clone();
        (sim.state_hash(), state)
    }

    /// Two sedans 30 m apart, 10 m/s each, head-on. Returns every event of a 3 s run.
    fn head_on_events() -> Vec<SimEvent> {
        let car = wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap();
        let mode = wp_contracts::load_mode_table(data("modes/sim.toml")).unwrap();
        let mut sim = Sim::new(SimConfig {
            seed: 3,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 10.0,
        });
        let a = sim.spawn_vehicle(&car, [0.0; 3], 0.0).unwrap();
        let b = sim
            .spawn_vehicle(&car, [0.0, 30.0, 0.0], std::f32::consts::PI)
            .unwrap();
        sim.place_vehicle(a, [0.0; 3], 0.0, 10.0).unwrap();
        sim.place_vehicle(b, [0.0, 30.0, 0.0], std::f32::consts::PI, 10.0)
            .unwrap();
        let hold = VehicleInput {
            throttle: 0.3,
            ..Default::default()
        };
        sim.set_input(a, hold).unwrap();
        sim.set_input(b, hold).unwrap();
        for _ in 0..720 {
            sim.step_fixed();
        }
        sim.drain_events()
    }

    #[test]
    fn head_on_crash_emits_contacts() {
        let events = head_on_events();
        let contacts: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                SimEvent::Contact {
                    a,
                    b,
                    impulse_ns,
                    rel_speed,
                    normal,
                    ..
                } => Some((*a, *b, *impulse_ns, *rel_speed, *normal)),
                _ => None,
            })
            .collect();
        assert!(
            !contacts.is_empty(),
            "no Contact events from a head-on crash"
        );
        // Emitted once per pair, by the lower entity id, pointing from a (south) toward b.
        assert!(contacts
            .iter()
            .all(|c| c.0 == EntityId(1) && c.1 == EntityId(2)));
        let first = contacts[0];
        assert!(
            (14.0..=25.0).contains(&first.3),
            "closing speed {} m/s, expected ≈ 20",
            first.3
        );
        assert!(first.4[1] > 0.7, "normal {:?} should point +Y", first.4);
        let total: f32 = contacts.iter().map(|c| c.2).sum();
        // 2 × 1400 kg at ±10 m/s: momentum change of each car is O(10⁴) N·s.
        assert!(total > 5_000.0, "total impulse {total} N·s");
        assert_eq!(
            events,
            head_on_events(),
            "contact events must be deterministic"
        );
    }

    #[test]
    fn static_barrier_stops_car_and_reports_world_contact() {
        let car = wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap();
        let mode = wp_contracts::load_mode_table(data("modes/sim.toml")).unwrap();
        let mut sim = Sim::new(SimConfig {
            seed: 4,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 10.0,
        });
        sim.add_static_box([0.0, 20.0, 0.75], [8.0, 1.0, 1.5], 0.0);
        let id = sim.spawn_vehicle(&car, [0.0; 3], 0.0).unwrap();
        sim.place_vehicle(id, [0.0; 3], 0.0, 15.0).unwrap();
        for _ in 0..720 {
            sim.step_fixed();
        }
        let s = &sim.vehicle_states()[0].1;
        assert!(
            s.pos[1] < 20.0,
            "car went through the barrier: y = {}",
            s.pos[1]
        );
        let hit = sim.drain_events().into_iter().find_map(|e| match e {
            SimEvent::Contact { b, rel_speed, .. } if b == wp_contracts::WORLD_ENTITY => {
                Some(rel_speed)
            }
            _ => None,
        });
        let speed = hit.expect("no world Contact event");
        assert!((12.0..=17.0).contains(&speed), "closing speed {speed}");
    }

    #[test]
    fn deterministic_hash() {
        assert_eq!(run(7).0, run(7).0);
    }

    #[test]
    fn disabled_vehicle_system_freezes_cars() {
        let car = wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap();
        let mode = wp_contracts::load_mode_table(data("modes/arcade.toml")).unwrap();
        let mut sim = Sim::new(SimConfig {
            seed: 1,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 10.0,
        });
        let id = sim.spawn_vehicle(&car, [0.0; 3], 0.0).unwrap();
        sim.set_systems(Systems::parse("damage,world,police").unwrap());
        let input = VehicleInput {
            throttle: 1.0,
            ..Default::default()
        };
        sim.set_input(id, input).unwrap();
        for _ in 0..240 {
            sim.step_fixed();
        }
        assert_eq!(sim.vehicle_states()[0].1.pos, [0.0; 3]);
        assert_eq!(sim.last_tick_event_count(), 0);
        assert_eq!(sim.entity_hashes().len(), 1);
    }

    #[test]
    fn hash_changes_with_state() {
        let car = wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap();
        let mode = wp_contracts::load_mode_table(data("modes/arcade.toml")).unwrap();
        let mut sim = Sim::new(SimConfig {
            seed: 7,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 10.0,
        });
        sim.spawn_vehicle(&car, [0.0; 3], 0.0).unwrap();
        let before = (sim.state_hash(), sim.entity_hashes()[0].1);
        sim.reset_vehicle(EntityId(1)).unwrap();
        assert_eq!(before, (sim.state_hash(), sim.entity_hashes()[0].1));
        sim.step_fixed();
        assert_ne!(before.0, sim.state_hash());
    }

    #[test]
    fn car_drives() {
        let (_, s) = run(1);
        assert!(s.speed_mps > 5.0);
        assert_eq!(s.health, 1.0);
    }
}
