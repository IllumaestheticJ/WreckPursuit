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
mod police_link;
mod props;
mod systems;
mod traffic_link;

pub use contacts::CONTACT_EVENT_MIN_IMPULSE_NS;
pub use physics::PhysicsWorld;
pub use police_link::POLICE_SLOT_RESERVE;
pub use systems::{SystemId, Systems};
pub use traffic_link::TRAFFIC_SLOT_RESERVE;

use rapier3d::prelude::QueryFilter;
use thiserror::Error;
use wp_contracts::{
    AssistProfile, DamageSim, EntityId, LoadedCar, ModeTable, SimEvent, TimeOfDay, VehicleInput,
    VehicleSim, VehicleState, MAX_VEHICLES, PHYSICS_DT,
};
use wp_damage::DamageModel;
use wp_math::{FixedStep, Rng, StateHasher};
use wp_police::PoliceSystem;
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

/// Who drives a vehicle. Decides how other systems treat it (traffic sight ring, net
/// interest); does not change physics. Default [`VehicleKind::Player`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum VehicleKind {
    /// Human player (racer or police seat).
    #[default]
    Player,
    /// AI opponent racer.
    AiRacer,
    /// AI police unit.
    Police,
    /// Physical (promoted) traffic agent.
    Traffic,
}

/// Hook run on every contact before damage applies it (e.g. the server quantising contacts
/// so clients can replay them bit-identically, see `wp_net`). Plain function pointer:
/// deterministic and `wp_sim` does not depend on the caller's crate.
pub type ContactFilter = fn(&mut wp_contracts::ContactImpulse);

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
    /// Who drives it (see [`Sim::set_vehicle_kind`]).
    pub kind: VehicleKind,
    /// The `wp_police` unit this car embodies (AI police cars only).
    pub police_unit: Option<wp_police::UnitId>,
    /// The `wp_world` traffic agent this car embodies (physical traffic only).
    pub traffic_agent: Option<wp_world::AgentId>,
    spawn: ([f32; 3], f32),
    /// Wheel radius / width (m) for debris cylinders when a wheel tears off.
    wheel_size: (f32, f32),
    /// `DamageModel::hull_version` the chassis collider was last built from.
    hull_version: u32,
    /// Lowest chassis-space z of the spawn collider; the damaged hull is clamped to it so a
    /// crushed car never drags its collider on the road (the wheels carry it).
    collider_floor_z: f32,
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
    props: props::Props,
    contact_filter: Option<ContactFilter>,
    police_car: Option<LoadedCar>,
    traffic_car: Option<LoadedCar>,
    record_contacts: bool,
    applied_contacts: Vec<(EntityId, Vec<wp_contracts::ContactImpulse>)>,
    debris: Vec<rapier3d::prelude::RigidBodyHandle>,
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
            props: props::Props::default(),
            contact_filter: None,
            police_car: None,
            traffic_car: None,
            record_contacts: false,
            applied_contacts: Vec::new(),
            debris: Vec::new(),
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
        let mut damage = DamageModel::new(&car.lattice, &car.materials);
        damage.set_entity(id);
        let collider_floor_z = {
            let col = &self.physics.colliders[body.collider];
            let rb = &self.physics.bodies[body.body];
            let aabb = col.compute_aabb();
            aabb.mins.z - rb.translation().z
        };
        let hull_version = damage.hull_version();
        self.vehicles.push(SimVehicle {
            id,
            car_id: car.def.id.clone(),
            body,
            vehicle,
            damage,
            input: VehicleInput::default(),
            assists: AssistProfile::default(),
            kind: VehicleKind::Player,
            police_unit: None,
            traffic_agent: None,
            spawn: (pos, yaw),
            wheel_size: (car.def.wheels[0].radius_m, car.def.wheels[0].width_m),
            hull_version,
            collider_floor_z,
        });
        Ok(id)
    }

    /// Replace the world (city, traffic, clock) — e.g. `World::load` or `World::procedural`
    /// from `wp_world`. Call before spawning or the first tick. Every `static_props()` entry
    /// (parked cars, cones) becomes a Rapier box with its mass: fixed while nothing is near,
    /// dynamic once a vehicle or debris body comes within reach (see `props.rs`). The city
    /// road surface is the flat ground slab at z = 0 (cities are flat in phase 1).
    pub fn set_world(&mut self, world: World) {
        use rapier3d::math::Rotation;
        use rapier3d::prelude::{ColliderBuilder, Pose, RigidBodyBuilder, Vector};
        for prop in world.static_props() {
            let (s, c) = wp_math::sin_cos(prop.yaw * 0.5);
            let pose = Pose::from_parts(
                Vector::from_array(prop.pos),
                Rotation::from_xyzw(0.0, 0.0, s, c),
            );
            let body = self
                .physics
                .bodies
                .insert(RigidBodyBuilder::fixed().pose(pose).build());
            let [hx, hy, hz] = prop.half_extents;
            let col = ColliderBuilder::cuboid(hx, hy, hz)
                .mass(prop.mass_kg.max(1.0))
                .friction(0.6)
                .build();
            self.physics
                .colliders
                .insert_with_parent(col, body, &mut self.physics.bodies);
            let radius = (hx * hx + hy * hy + hz * hz).sqrt();
            self.props.add(body, prop.pos, radius);
        }
        self.world = world;
        self.rebuild_police();
    }

    /// Replace the police system, e.g. `PoliceSystem::for_world(cfg, &world, setup)` so units
    /// patrol the city's road graph (the default `PoliceSystem::new()` has no road network and
    /// therefore no units). Call before the first tick so replays match.
    pub fn set_police(&mut self, police: PoliceSystem) {
        self.police = police;
    }

    /// Number of city props turned into physics bodies by [`Sim::set_world`].
    pub fn prop_count(&self) -> usize {
        self.props.len()
    }

    /// Number of city props currently simulated as dynamic bodies (near a vehicle or still
    /// settling after a hit); the rest are fixed until something comes near.
    pub fn active_prop_count(&self) -> usize {
        self.props.active_count()
    }

    /// Number of detached-part debris bodies in the world.
    pub fn debris_count(&self) -> usize {
        self.debris.len()
    }

    /// After damage: spawn debris for torn-off parts and rebuild a chassis collider whose
    /// hull changed. Slot order, deterministic.
    fn sync_damage_to_physics(&mut self) {
        use rapier3d::prelude::{Pose, SharedShape, Vector};
        for slot in &mut self.vehicles {
            let parts = slot.damage.take_detached_parts();
            let hull_changed = slot.damage.hull_version() != slot.hull_version;
            if parts.is_empty() && !hull_changed {
                continue;
            }
            let rb = &self.physics.bodies[slot.body.body];
            let pos = rb.translation().to_array();
            let r = *rb.rotation();
            let rot = [r.x, r.y, r.z, r.w];
            let vel = rb.linvel().to_array();
            for part in parts {
                let (radius, width) = slot.wheel_size;
                if let Some((body, col)) = part.to_rapier(pos, rot, vel, radius, width) {
                    let h = self.physics.bodies.insert(body);
                    self.physics
                        .colliders
                        .insert_with_parent(col, h, &mut self.physics.bodies);
                    self.debris.push(h);
                }
            }
            if hull_changed {
                slot.hull_version = slot.damage.hull_version();
                let floor = slot.collider_floor_z;
                let pts: Vec<Vector> = slot
                    .damage
                    .convex_hull_outer()
                    .iter()
                    .map(|p| Vector::new(p[0], p[1], p[2].max(floor)))
                    .collect();
                if let Some(shape) = SharedShape::convex_hull(&pts) {
                    let col = &mut self.physics.colliders[slot.body.collider];
                    col.set_shape(shape);
                    col.set_position_wrt_parent(Pose::IDENTITY);
                }
            }
        }
    }

    /// Set who drives a vehicle (player / AI racer / police / traffic).
    pub fn set_vehicle_kind(&mut self, id: EntityId, kind: VehicleKind) -> Result<(), SimError> {
        self.slot_mut(id)?.kind = kind;
        Ok(())
    }

    /// Remove a vehicle and its physics body. Its id is never reused. Debris it already shed
    /// stays in the world.
    pub fn despawn_vehicle(&mut self, id: EntityId) -> Result<(), SimError> {
        let i = self
            .vehicles
            .iter()
            .position(|v| v.id == id)
            .ok_or(SimError::UnknownEntity(id))?;
        let slot = self.vehicles.remove(i);
        self.physics.remove_body(slot.body.body);
        Ok(())
    }

    /// Install (or clear) the [`ContactFilter`] applied to every contact before damage.
    pub fn set_contact_filter(&mut self, filter: Option<ContactFilter>) {
        self.contact_filter = filter;
    }

    /// Start or stop recording the contacts damage actually applied (after the filter).
    pub fn set_record_applied_contacts(&mut self, on: bool) {
        self.record_contacts = on;
        if !on {
            self.applied_contacts.clear();
        }
    }

    /// Contacts damage applied since the last drain, per vehicle per tick in step order
    /// (empty unless [`Sim::set_record_applied_contacts`] is on). Only vehicles that had
    /// contacts appear.
    pub fn drain_applied_contacts(&mut self) -> Vec<(EntityId, Vec<wp_contracts::ContactImpulse>)> {
        std::mem::take(&mut self.applied_contacts)
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
            // Every vehicle is "tracked" (violations, siren reactions); only players drive the
            // traffic spawn ring / physical LOD.
            let tracked: Vec<wp_world::TrackedVehicle> = self
                .vehicles
                .iter()
                .map(|v| {
                    wp_world::TrackedVehicle::from_state(
                        v.id,
                        &v.vehicle.state(),
                        v.input.siren,
                        v.kind == VehicleKind::Player,
                    )
                })
                .collect();
            self.world.set_tracked_vehicles(&tracked);
            self.world.step(dt, &mut out);
            self.apply_traffic_lod();
            self.drive_traffic();
        }
        if sys.police {
            self.drive_police_units();
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
        self.wake_props();
        self.physics.step();
        let mut tick_contacts = contacts::collect(
            self.physics.narrow_phase(),
            &self.physics.bodies,
            &self.physics.colliders,
            &infos,
        );
        out.extend(std::mem::take(&mut tick_contacts.events));
        if let Some(filter) = self.contact_filter {
            for c in tick_contacts.per_vehicle.iter_mut().flatten() {
                filter(c);
            }
        }
        if sys.vehicle {
            for slot in &mut self.vehicles {
                slot.vehicle.read_body(&self.physics.bodies[slot.body.body]);
            }
        }
        if sys.damage {
            for (slot, contacts) in self.vehicles.iter_mut().zip(&tick_contacts.per_vehicle) {
                slot.damage
                    .apply_contacts(contacts, &self.config.mode, dt, &mut out);
                if self.record_contacts && !contacts.is_empty() {
                    self.applied_contacts.push((slot.id, contacts.clone()));
                }
                let mut effects = slot.damage.handling_effects();
                // Spike strips (wp_police) deflate tires on top of damage.
                let spikes = self.police.tire_puncture(slot.id);
                for (t, s) in effects.tire_puncture.iter_mut().zip(spikes) {
                    *t = t.max(s);
                }
                slot.vehicle.apply_damage_effects(&effects);
            }
            self.sync_damage_to_physics();
        }
        if sys.vehicle {
            self.sync_traffic();
        }
        if sys.police {
            let racers = self.racer_views();
            let tick_events = out.clone();
            self.police
                .step(&self.world, &racers, &tick_events, dt, &mut out);
            self.reconcile_police_units();
        }
        self.last_tick_events = out.len();
        self.events.extend(out);
        self.tick += 1;
    }

    /// Make props within reach of a vehicle or debris body dynamic (and far, asleep ones
    /// fixed again) before the physics step.
    fn wake_props(&mut self) {
        if self.props.is_empty() {
            return;
        }
        let bodies = &self.physics.bodies;
        let mover = |h: rapier3d::prelude::RigidBodyHandle| {
            let rb = &bodies[h];
            props::Mover {
                pos: rb.translation().to_array(),
                vel: rb.linvel().to_array(),
            }
        };
        let movers: Vec<props::Mover> = self
            .vehicles
            .iter()
            .map(|v| mover(v.body.body))
            .chain(self.debris.iter().map(|h| mover(*h)))
            .collect();
        self.props.update(&mut self.physics.bodies, &movers);
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
                s.component_health = v.damage.component_health_map();
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

    /// Stable hash of the whole simulation state (hash spec v3, see `docs/integration/harness.md`):
    /// FNV-1a 64 over `HASH_SPEC_VERSION`, tick, in-game clock (1 ms), [`World::state_hash`]
    /// (signals + traffic), `PoliceSystem::state_hash` (units, heat, pursuits), debris count and every debris body position (1e-3 m) in spawn order,
    /// vehicle count, then every [`Sim::entity_hashes`] value in ascending entity order.
    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        h.write_u32(HASH_SPEC_VERSION);
        h.write_u64(self.tick);
        h.write_f32_q(self.world.clock.time.seconds_since_midnight, 1e3);
        h.write_u64(self.world.state_hash());
        h.write_u64(self.police.state_hash());
        h.write_u32(self.debris.len() as u32);
        for d in &self.debris {
            for x in self.physics.bodies[*d].translation().to_array() {
                h.write_f32_q(x, 1e3);
            }
        }
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
        let mut hashers: Vec<StateHasher> = self
            .vehicles
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
                h.write_u32(v.damage.node_positions().len() as u32);
                h
            })
            .collect();
        // Lattice nodes are most of the work (the testbed hashes every tick for its replay
        // log). Each hasher is one serial FNV chain, so hash four cars' nodes in lockstep to
        // let the CPU overlap the chains; the result is the same as one car at a time.
        let nodes: Vec<&[[f32; 3]]> = self
            .vehicles
            .iter()
            .map(|v| v.damage.node_positions())
            .collect();
        for (hs, ns) in hashers.chunks_mut(4).zip(nodes.chunks(4)) {
            hash_nodes_lockstep(hs, ns);
        }
        self.vehicles
            .iter()
            .zip(hashers)
            .map(|(v, h)| (v.id, h.finish()))
            .collect()
    }
}

/// Feed `nodes[k]` (quantised to 0.1 mm) into `hashers[k]`, all lists advancing together.
/// Equivalent to `for n in nodes[k] { for x in n { hashers[k].write_f32_q(*x, 1e4) } }` per
/// `k`, but independent chains interleave.
fn hash_nodes_lockstep(hashers: &mut [StateHasher], nodes: &[&[[f32; 3]]]) {
    let longest = nodes.iter().map(|n| n.len()).max().unwrap_or(0);
    for i in 0..longest {
        for (h, list) in hashers.iter_mut().zip(nodes) {
            if let Some(n) = list.get(i) {
                for x in n {
                    h.write_f32_q(*x, 1e4);
                }
            }
        }
    }
}

/// Version of the [`Sim::state_hash`] layout. Bump when the hashed fields change; every golden
/// must then be re-blessed.
pub const HASH_SPEC_VERSION: u32 = 3;

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
        // 54 km/h into a rigid wall in Simulation: the car is damaged, the nose crushed.
        assert!(
            s.health < 1.0,
            "health {} after a 54 km/h barrier hit",
            s.health
        );
        let v = &sim.vehicles[0];
        let crush = (0..v.damage.node_positions().len())
            .map(|i| {
                let p = v.damage.node_positions()[i];
                let r = v.damage.rest_position(i);
                ((p[0] - r[0]).powi(2) + (p[1] - r[1]).powi(2) + (p[2] - r[2]).powi(2)).sqrt()
            })
            .fold(0.0f32, f32::max);
        assert!(crush > 0.1, "max node displacement {crush} m");
    }

    #[test]
    fn procedural_city_loads_props_and_steps() {
        let car = wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap();
        let mode = wp_contracts::load_mode_table(data("modes/arcade.toml")).unwrap();
        let mk = || {
            let mut sim = Sim::new(SimConfig {
                seed: 9,
                mode: mode.clone(),
                time_of_day: TimeOfDay::from_hours(12.0),
                time_ratio: 10.0,
            });
            let world = World::procedural(
                &wp_world::procgen::ProcGenConfig::default(),
                wp_world::WorldConfig::default(),
            );
            sim.set_world(world);
            let id = sim.spawn_vehicle(&car, [0.0, 0.0, 0.0], 0.0).unwrap();
            sim.set_input(
                id,
                VehicleInput {
                    throttle: 0.5,
                    ..Default::default()
                },
            )
            .unwrap();
            for _ in 0..60 {
                sim.step_fixed();
            }
            sim
        };
        let a = mk();
        assert!(a.prop_count() > 0, "procedural city has no props");
        assert_eq!(a.state_hash(), mk().state_hash());
    }

    #[test]
    fn props_are_fixed_until_a_car_comes_and_get_knocked() {
        let car = wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap();
        let mode = wp_contracts::load_mode_table(data("modes/arcade.toml")).unwrap();
        let mut sim = Sim::new(SimConfig {
            seed: 9,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 1.0,
        });
        let world = World::procedural(
            &wp_world::procgen::ProcGenConfig::default(),
            wp_world::WorldConfig::default(),
        );
        let target = *world.static_props().next().unwrap();
        sim.set_world(world);
        assert_eq!(sim.active_prop_count(), 0, "props start fixed");
        // Approach the first prop from 20 m away at 12 m/s, from its side.
        let (s, c) = wp_math::sin_cos(target.yaw);
        let side = [c, s];
        let start = [
            target.pos[0] + side[0] * 20.0,
            target.pos[1] + side[1] * 20.0,
            0.0,
        ];
        let (dx, dy) = (target.pos[0] - start[0], target.pos[1] - start[1]);
        let yaw = wp_math::atan2(-dx, dy);
        let id = sim.spawn_vehicle(&car, start, yaw).unwrap();
        sim.place_vehicle(id, start, yaw, 12.0).unwrap();
        let h = sim.props.bodies[0];
        let p0 = sim.physics.bodies[h].translation();
        for _ in 0..480 {
            sim.step_fixed();
        }
        let p1 = sim.physics.bodies[h].translation();
        assert!(
            (p1 - p0).length() > 0.1,
            "the prop was not knocked: moved {} m",
            (p1 - p0).length()
        );
        assert!(sim.active_prop_count() < 40, "only nearby props wake");
    }

    fn quantise(c: &mut wp_contracts::ContactImpulse) {
        c.impulse_ns = (c.impulse_ns * 16.0).round() / 16.0;
    }

    #[test]
    fn contact_filter_recording_and_despawn() {
        let car = wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap();
        let mode = wp_contracts::load_mode_table(data("modes/sim.toml")).unwrap();
        let mut sim = Sim::new(SimConfig {
            seed: 5,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 10.0,
        });
        let a = sim.spawn_vehicle(&car, [0.0; 3], 0.0).unwrap();
        let b = sim
            .spawn_vehicle(&car, [0.0, 30.0, 0.0], std::f32::consts::PI)
            .unwrap();
        sim.set_vehicle_kind(b, VehicleKind::Traffic).unwrap();
        sim.place_vehicle(a, [0.0; 3], 0.0, 10.0).unwrap();
        sim.place_vehicle(b, [0.0, 30.0, 0.0], std::f32::consts::PI, 10.0)
            .unwrap();
        sim.set_contact_filter(Some(quantise));
        sim.set_record_applied_contacts(true);
        for _ in 0..480 {
            sim.step_fixed();
        }
        let applied = sim.drain_applied_contacts();
        assert!(!applied.is_empty(), "head-on recorded no contacts");
        assert!(applied
            .iter()
            .flat_map(|(_, c)| c)
            .all(|c| (c.impulse_ns * 16.0).fract() == 0.0));
        assert!(sim.drain_applied_contacts().is_empty());
        sim.despawn_vehicle(b).unwrap();
        assert_eq!(sim.vehicles().len(), 1);
        assert_eq!(sim.despawn_vehicle(b), Err(SimError::UnknownEntity(b)));
        for _ in 0..60 {
            sim.step_fixed();
        }
    }

    /// Small procedural city with police cars; the player speeds down the first road.
    fn police_city_run(ticks: u32) -> Sim {
        let car = wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap();
        let cop = wp_contracts::load_car(data("cars/sentinel_cruiser.json")).unwrap();
        let mode = wp_contracts::load_mode_table(data("modes/arcade.toml")).unwrap();
        let mut sim = Sim::new(SimConfig {
            seed: 11,
            mode,
            time_of_day: TimeOfDay::from_hours(22.0),
            time_ratio: 10.0,
        });
        sim.set_police_car(cop);
        sim.set_world(World::procedural(
            &wp_world::procgen::ProcGenConfig::small(),
            wp_world::WorldConfig::default(),
        ));
        let id = sim.spawn_vehicle(&car, [0.0, 0.0, 0.0], 0.0).unwrap();
        sim.set_input(
            id,
            VehicleInput {
                throttle: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
        for _ in 0..ticks {
            sim.step_fixed();
        }
        sim
    }

    #[test]
    fn police_units_become_driven_vehicles() {
        let sim = police_city_run(2400);
        let cops: Vec<_> = sim
            .vehicles()
            .iter()
            .filter(|v| v.kind == VehicleKind::Police)
            .collect();
        assert!(!cops.is_empty(), "no police cars spawned in 10 s");
        assert!(cops.iter().all(|v| v.police_unit.is_some()));
        // Every police car is bound to a live unit, and at least one is moving.
        let bound = sim
            .police()
            .units()
            .iter()
            .filter(|u| u.entity.is_some())
            .count();
        assert_eq!(bound, cops.len());
        assert!(
            cops.iter().any(|v| v.vehicle.state().speed_mps.abs() > 1.0),
            "police cars never move"
        );
        assert_eq!(sim.state_hash(), police_city_run(2400).state_hash());
    }

    #[test]
    fn traffic_near_the_player_becomes_physical() {
        let run = || {
            let car = wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap();
            let mode = wp_contracts::load_mode_table(data("modes/arcade.toml")).unwrap();
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
            let id = sim.spawn_vehicle(&car, [0.0, 0.0, 0.0], 0.0).unwrap();
            let tracked: Vec<_> = sim
                .vehicle_states()
                .iter()
                .map(|(e, s)| wp_world::TrackedVehicle::from_state(*e, s, false, true))
                .collect();
            world.set_tracked_vehicles(&tracked);
            world.populate();
            sim.set_world(world);
            sim.set_traffic_car(car.clone());
            sim.set_input(
                id,
                VehicleInput {
                    throttle: 0.4,
                    ..Default::default()
                },
            )
            .unwrap();
            let mut max_traffic = 0;
            for _ in 0..1440 {
                sim.step_fixed();
                let n = sim
                    .vehicles()
                    .iter()
                    .filter(|v| v.kind == VehicleKind::Traffic)
                    .count();
                max_traffic = max_traffic.max(n);
            }
            (sim.state_hash(), max_traffic)
        };
        let (h, n) = run();
        assert!(n > 0, "no traffic car became physical near the player");
        assert_eq!(h, run().0, "physical traffic must be deterministic");
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
