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
mod parked;
mod physics;
mod police_link;
mod police_tools;
mod props;
mod systems;
mod traffic_link;
mod weather_link;

pub use contacts::CONTACT_EVENT_MIN_IMPULSE_NS;
pub use parked::{
    prop_of_entity, ChassisMap, ParkedStats, ParkedWreck, MAX_PARKED_WRECKS,
    PARKED_CAR_HALF_EXTENTS, PARKED_CAR_MASS_KG, PARKED_ENTITY_BASE, PROMOTE_CLOSING_MPS,
    PROMOTE_IMPULSE_NS,
};
pub use physics::{
    city_ground_colliders, city_sidewalk_colliders, parked_slab_collider, CityColliders,
    PhysicsWorld, SAFETY_FLOOR_Z,
};
pub use police_link::POLICE_SLOT_RESERVE;
pub use police_tools::{BarrierView, SpikeStripView};
pub use systems::{SystemId, Systems};
pub use traffic_link::TRAFFIC_SLOT_RESERVE;
pub use weather_link::{WEATHER_CELL_M, WEATHER_GRID_MARGIN_M};
/// The free per-car tune, re-exported for spawners ([`Sim::spawn_vehicle_tuned`]).
pub use wp_vehicle::CarTune;

use rapier3d::prelude::QueryFilter;
use thiserror::Error;
use wp_contracts::{
    AssistProfile, DamageSim, EntityId, LoadedCar, ModeTable, SimEvent, TimeOfDay, VehicleInput,
    VehicleSim, VehicleState, WorldQuery, MAX_VEHICLES, PHYSICS_DT,
};
use wp_damage::DamageModel;
use wp_math::{FixedStep, Rng, StateHasher};
use wp_police::PoliceSystem;
use wp_vehicle::{RapierGround, Vehicle, VehicleBody, VehicleExtras};
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
#[derive(Clone)]
pub struct SimVehicle {
    /// Entity id.
    pub id: EntityId,
    /// Car id (`VehicleDef::id`).
    pub car_id: String,
    /// The tune the car was built with ([`Sim::spawn_vehicle_tuned`]; stock otherwise).
    pub tune: CarTune,
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
    /// The `wp_police` unit this car embodies (AI police cars and player cop seats).
    pub police_unit: Option<wp_police::UnitId>,
    /// The `wp_world` traffic agent this car embodies (physical traffic only).
    pub traffic_agent: Option<wp_world::AgentId>,
    spawn: ([f32; 3], f32),
    /// Player driving this police seat (`Sim::bind_player_police`).
    player_police: Option<wp_contracts::PlayerId>,
    /// Wheel radius / width (m) for debris cylinders when a wheel tears off.
    wheel_size: (f32, f32),
    /// `DamageModel::hull_version` the chassis collider was last built from.
    hull_version: u32,
    /// Lowest chassis-space z of the spawn collider; the damaged hull is clamped to it so a
    /// crushed car never drags its collider on the road (the wheels carry it).
    collider_floor_z: f32,
    /// Whether the chassis collider currently ignores sidewalks (upright on its wheels; see
    /// [`physics::SIDEWALK_GROUP`]).
    chassis_skips_sidewalks: bool,
    /// Ticks left as a ghost ([`Sim::set_ghost`]): 0 = solid.
    ghost_ticks: u32,
    /// Ticks the ghost was extended because the car still overlapped another car.
    ghost_overstay: u32,
}

/// Longest a ghost is extended past its window while it still overlaps another car (3 s).
pub const GHOST_MAX_OVERSTAY_TICKS: u32 = 3 * 240;

/// Gap left under a new prop in hilly cities, m.
const PROP_HOVER_M: f32 = 0.04;

/// Pose on the city's drivable ground at `xy` facing `yaw`: height from the road nearest
/// `near_z` (or the terrain), rotation pitched and rolled to the surface normal (sampled a
/// metre around).
fn ground_pose(
    map: &wp_world::CityMap,
    xy: [f32; 2],
    yaw: f32,
    near_z: Option<f32>,
) -> ([f32; 3], rapier3d::math::Rotation) {
    use rapier3d::math::{Mat3, Rotation, Vector};
    let z = map.ground_height(xy, near_z);
    let e = 1.0;
    let dx = map.ground_height([xy[0] + e, xy[1]], Some(z))
        - map.ground_height([xy[0] - e, xy[1]], Some(z));
    let dy = map.ground_height([xy[0], xy[1] + e], Some(z))
        - map.ground_height([xy[0], xy[1] - e], Some(z));
    let n = Vector::new(-dx / (2.0 * e), -dy / (2.0 * e), 1.0).normalize();
    let (sy, cy) = wp_math::sin_cos(yaw);
    let f0 = Vector::new(-sy, cy, 0.0);
    let f = (f0 - n * f0.dot(n)).normalize();
    let r = f.cross(n);
    (
        [xy[0], xy[1], z],
        Rotation::from_mat3(&Mat3::from_cols(r, f, n)),
    )
}

/// A complete simulation instance. Deterministic given the same config, spawns and inputs.
///
/// `Clone` is a full snapshot (Rapier sets, lattices, world, police): stepping a clone with
/// the same inputs gives the same tick hashes as the original (in-memory crash replays,
/// `wp_wasm::crash_replay`).
#[derive(Clone)]
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
    /// Promoted (damageable) parked cars (`parked.rs`).
    parked: parked::ParkedDamage,
    contact_filter: Option<ContactFilter>,
    police_car: Option<LoadedCar>,
    police_car_limits: Option<wp_police::pro::CarLimits>,
    traffic_car: Option<LoadedCar>,
    record_contacts: bool,
    applied_contacts: Vec<(EntityId, Vec<wp_contracts::ContactImpulse>)>,
    debris: Vec<rapier3d::prelude::RigidBodyHandle>,
    /// Roadblock barrier bodies (see `police_tools`).
    tool_bodies: Vec<police_tools::ToolBody>,
    city_colliders: CityColliders,
    /// Hilly cities: props still resting (not colliding with terrain), in creation order.
    resting_props: Vec<(
        rapier3d::prelude::RigidBodyHandle,
        rapier3d::prelude::ColliderHandle,
    )>,
    /// The bodies of `resting_props` (raw handle parts), for the per-tick "did one start
    /// moving" test.
    resting_bodies: std::collections::BTreeSet<(u32, u32)>,
    /// Weather (`None` = none: dry road, clear air), see [`Sim::set_weather`].
    weather: Option<weather_link::WeatherLink>,
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
            parked: parked::ParkedDamage::default(),
            contact_filter: None,
            police_car: None,
            police_car_limits: None,
            traffic_car: None,
            record_contacts: false,
            applied_contacts: Vec::new(),
            debris: Vec::new(),
            tool_bodies: Vec::new(),
            city_colliders: CityColliders::default(),
            resting_props: Vec::new(),
            resting_bodies: Default::default(),
            weather: None,
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
        self.spawn_vehicle_tuned(car, pos, yaw, CarTune::STOCK)
    }

    /// [`Sim::spawn_vehicle`] with the player's free tune applied to the car first
    /// (`wp_vehicle::tune`; a stock tune builds exactly the car file).
    pub fn spawn_vehicle_tuned(
        &mut self,
        car: &LoadedCar,
        pos: [f32; 3],
        yaw: f32,
        tune: CarTune,
    ) -> Result<EntityId, SimError> {
        if self.vehicles.len() >= MAX_VEHICLES {
            return Err(SimError::TooManyVehicles);
        }
        let id = EntityId(self.next_entity);
        self.next_entity += 1;
        let tune = tune.clamped();
        let mut vehicle = if tune.is_stock() {
            Vehicle::new(&car.def, &car.tires, pos, yaw)
        } else {
            let (def, diffs) = wp_vehicle::tune::tuned(&car.def, tune);
            let extras = VehicleExtras {
                diffs: Some(diffs),
                ..VehicleExtras::default()
            };
            Vehicle::with_extras(&def, &car.tires, pos, yaw, extras)
        };
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
            tune,
            body,
            vehicle,
            damage,
            input: VehicleInput::default(),
            assists: AssistProfile::default(),
            kind: VehicleKind::Player,
            police_unit: None,
            traffic_agent: None,
            spawn: (pos, yaw),
            player_police: None,
            wheel_size: (car.def.wheels[0].radius_m, car.def.wheels[0].width_m),
            hull_version,
            collider_floor_z,
            chassis_skips_sidewalks: false,
            ghost_ticks: 0,
            ghost_overstay: 0,
        });
        Ok(id)
    }

    /// Replace the world (city, traffic, clock) — e.g. `World::load`, `World::procedural` or
    /// `World::vesper` from `wp_world`. Call before spawning or the first tick. Every
    /// `static_props()` entry (parked cars, cones) becomes a dynamic Rapier box with its mass.
    /// Every city gets sidewalk and kerb colliders; flat cities otherwise drive on the ground
    /// slab at z = 0, and a city with terrain (Port Vesper v2) replaces the slab with terrain,
    /// road, building and deck-rail colliders ([`PhysicsWorld::set_city_ground`]).
    pub fn set_world(&mut self, world: World) {
        self.city_colliders = self.physics.set_city_ground(world.map());
        self.world = world;
        let props: Vec<wp_world::props::StaticProp> = self.world.static_props().copied().collect();
        if self.has_terrain() {
            // Seat every parked car on the ground colliders under its wheels and drop the spots
            // where a car cannot stand (owner bug 0.12.26: parked cars in the ground on Hollins
            // Drive); the world keeps exactly the cars the sim has.
            let mut kept = Vec::new();
            for prop in &props {
                if prop.kind != wp_world::props::PropKind::ParkedCar {
                    self.insert_prop(prop, None);
                    continue;
                }
                if let Some(pose) = self.seat_parked_car(prop) {
                    self.insert_prop(prop, Some(pose));
                    let t = pose.translation;
                    kept.push(wp_world::props::StaticProp {
                        pos: [t.x, t.y, t.z],
                        ..*prop
                    });
                }
            }
            // `static_props()` is parked cars then cones: the kept cars replace the first part.
            self.world.set_parked_cars(kept);
        } else {
            for prop in &props {
                self.insert_prop(prop, None);
            }
        }
        self.rebuild_police();
        self.rebuild_weather();
    }

    /// Add one parked car (a city-style prop: fixed box until something comes near, dented
    /// through `parked.rs` once hit) standing on the ground at `pos` (world m; on a city with
    /// terrain only x, y count and the car sits on the ground there), heading `yaw` rad about
    /// +Z. Scenario setup (simcli `parked`): call after [`Sim::set_world`] and before the first
    /// tick so replays match. Returns its prop index ([`Sim::prop_pose`]).
    pub fn add_parked_car(&mut self, pos: [f32; 3], yaw: f32) -> usize {
        let [_, _, hz] = parked::PARKED_CAR_HALF_EXTENTS;
        let prop = wp_world::props::StaticProp {
            kind: wp_world::props::PropKind::ParkedCar,
            pos: [pos[0], pos[1], pos[2] + hz],
            yaw,
            half_extents: parked::PARKED_CAR_HALF_EXTENTS,
            mass_kg: parked::PARKED_CAR_MASS_KG,
        };
        let seated = if self.has_terrain() {
            self.seat_parked_car(&prop)
        } else {
            None
        };
        self.insert_prop(&prop, seated);
        self.props.len() - 1
    }

    /// One prop as a fixed Rapier box (see [`Sim::set_world`]), at `seated` if given (a parked
    /// car seated on the ground colliders), else at its laid-out position (on a city with
    /// terrain: on the ground under it, pitched to the road).
    fn insert_prop(
        &mut self,
        prop: &wp_world::props::StaticProp,
        seated: Option<rapier3d::prelude::Pose>,
    ) {
        use rapier3d::math::Rotation;
        use rapier3d::prelude::{ColliderBuilder, Pose, RigidBodyBuilder, Vector};
        let hilly = self.world.network().terrain.is_some();
        let (s, c) = wp_math::sin_cos(prop.yaw * 0.5);
        let mut pose = Pose::from_parts(
            Vector::from_array(prop.pos),
            Rotation::from_xyzw(0.0, 0.0, s, c),
        );
        if let Some(p) = seated {
            pose = p;
        } else if hilly {
            // Flush with the (sloped, banked) surface under it and asleep until something
            // hits it: thousands of parked cars otherwise keep settling on the road mesh.
            let (pos, rot) = ground_pose(
                self.world.map(),
                [prop.pos[0], prop.pos[1]],
                prop.yaw,
                Some(prop.pos[2] - prop.half_extents[2]),
            );
            // A hair above the surface: no touching contact at creation, so it stays asleep
            // until something reaches it (then it drops the last few centimetres).
            let up = rot * Vector::Z;
            pose = Pose::from_parts(
                Vector::from_array(pos) + up * (prop.half_extents[2] + PROP_HOVER_M),
                rot,
            );
        }
        // Fixed until a vehicle or debris comes near (`props.rs`), then dynamic.
        let body = self
            .physics
            .bodies
            .insert(RigidBodyBuilder::fixed().pose(pose).build());
        let [hx, hy, hz] = prop.half_extents;
        let mut col = ColliderBuilder::cuboid(hx, hy, hz)
            .mass(prop.mass_kg.max(1.0))
            .friction(0.6)
            .user_data(physics::PROP_USER_BIT)
            .build();
        if hilly {
            // Resting on the road mesh: no pairs with the terrain tiles under it (thousands
            // of them otherwise cost ~0.6 ms per frame); restored when it wakes.
            col.set_collision_groups(physics::resting_prop_groups());
        }
        let ch = self
            .physics
            .colliders
            .insert_with_parent(col, body, &mut self.physics.bodies);
        self.props
            .add(body, ch, prop.pos, prop.kind, prop.half_extents);
        if hilly {
            self.resting_props.push((body, ch));
            self.resting_bodies.insert(body.into_raw_parts());
        }
    }

    /// A car upright on its wheels lets the wheels alone deal with kerbs and sidewalks: its
    /// chassis box (whose floor sits at kerb height) would otherwise scrape the kerb edge
    /// while straddling it and snag on the edges between sidewalk triangles. Rolled, airborne
    /// or wheel-less cars collide with sidewalks again, so a wreck never sinks into one.
    /// Vehicle order, only on change: deterministic.
    fn update_sidewalk_filters(&mut self) {
        if self.city_colliders.sidewalk_triangles == 0 {
            return;
        }
        for slot in &mut self.vehicles {
            let st = slot.vehicle.state();
            let on_wheels = st.wheel_states.iter().filter(|w| w.contact).count() >= 2;
            let up = slot.vehicle.chassis().rot * rapier3d::math::Vector::Z;
            let skip = on_wheels && up.z > 0.7;
            if skip != slot.chassis_skips_sidewalks {
                slot.chassis_skips_sidewalks = skip;
                if let Some(col) = self.physics.colliders.get_mut(slot.body.collider) {
                    col.set_collision_groups(if skip {
                        physics::chassis_on_wheels_groups()
                    } else {
                        rapier3d::prelude::InteractionGroups::all()
                    });
                }
            }
        }
    }

    /// Props still resting on the road mesh without terrain pairs (hilly cities; diagnostics).
    pub fn resting_prop_count(&self) -> usize {
        self.resting_props.len()
    }

    /// Rigid bodies currently awake (perf diagnostics: resting props should be asleep).
    pub fn awake_body_count(&self) -> usize {
        self.physics
            .bodies
            .iter()
            .filter(|(_, b)| b.is_dynamic() && !b.is_sleeping())
            .count()
    }

    /// Props that turned dynamic (a vehicle or debris is near, `props.rs`) collide with the
    /// terrain again, so a parked car knocked off the road lands on the ground instead of
    /// sinking into it. Earlier this waited for the prop to move faster than 0.5 m/s after a
    /// 1 s settle window, and a car knocked inside that window (or pushed slowly) kept no
    /// terrain contacts and was shoved into the hillside (owner bug 0.12.26). Only the few
    /// dynamic props are checked; in creation order: deterministic.
    fn wake_resting_props(&mut self) {
        if self.resting_props.is_empty() {
            return;
        }
        let woken: Vec<(u32, u32)> = self
            .props
            .active()
            .map(|i| self.props.bodies[i].into_raw_parts())
            .filter(|b| self.resting_bodies.contains(b))
            .collect();
        if woken.is_empty() {
            return;
        }
        for b in &woken {
            self.resting_bodies.remove(b);
        }
        let colliders = &mut self.physics.colliders;
        let resting_bodies = &self.resting_bodies;
        self.resting_props.retain(|(b, c)| {
            if resting_bodies.contains(&b.into_raw_parts()) {
                return true;
            }
            if let Some(col) = colliders.get_mut(*c) {
                col.set_collision_groups(rapier3d::prelude::InteractionGroups::all());
            }
            false
        });
    }

    /// Colliders built for the current city's ground (all zero for flat cities).
    pub fn city_colliders(&self) -> CityColliders {
        self.city_colliders
    }

    /// Height of the first static physics surface below `(xy, from_z)` (at most `max_m`
    /// down), ignoring vehicles and props. See [`PhysicsWorld::static_ground_z`].
    pub fn static_ground_z(&self, xy: [f32; 2], from_z: f32, max_m: f32) -> Option<f32> {
        self.physics.static_ground_z(xy, from_z, max_m)
    }

    /// Height of the drivable ground (road, sidewalk and terrain colliders; never buildings,
    /// rails, decor or bodies) below `(xy, from_z)`, at most `max_m` down. See
    /// [`PhysicsWorld::drivable_ground_z`].
    pub fn drivable_ground_z(&self, xy: [f32; 2], from_z: f32, max_m: f32) -> Option<f32> {
        self.physics.drivable_ground_z(xy, from_z, max_m)
    }

    /// Whether the current city has terrain (hills, decks): spawns are then placed on the
    /// ground under them and pitched to its slope.
    pub fn has_terrain(&self) -> bool {
        self.world.network().terrain.is_some()
    }

    /// Teleport a vehicle onto the city's drivable ground at `xy`: height from the road (the
    /// one nearest `near_z`, if given) or terrain — or the sidewalk top over it — chassis
    /// pitched and rolled to the surface, rolling forward along it at `speed_mps`. On a flat
    /// city this is [`Sim::place_vehicle`] at z = 0 (or on the sidewalk top).
    pub fn place_on_ground(
        &mut self,
        id: EntityId,
        xy: [f32; 2],
        yaw: f32,
        speed_mps: f32,
        near_z: Option<f32>,
    ) -> Result<(), SimError> {
        if !self.has_terrain() {
            let z = self.sidewalk_lift(xy, 0.0);
            return self.place_vehicle(id, [xy[0], xy[1], z], yaw, speed_mps);
        }
        use rapier3d::math::Vector;
        let (mut pos, rot) = ground_pose(self.world.map(), xy, yaw, near_z);
        pos[2] = self.sidewalk_lift(xy, pos[2]);
        let f = rot * Vector::Y;
        self.place_vehicle(id, pos, yaw, speed_mps)?;
        let slot = self
            .vehicles
            .iter_mut()
            .find(|v| v.id == id)
            .ok_or(SimError::UnknownEntity(id))?;
        let rb = &mut self.physics.bodies[slot.body.body];
        rb.set_position(
            rapier3d::prelude::Pose::from_parts(Vector::from_array(pos), rot),
            true,
        );
        rb.set_linvel(f * speed_mps, true);
        slot.vehicle.read_body(rb);
        slot.spawn = (pos, yaw);
        Ok(())
    }

    /// `z` raised onto a sidewalk (or kerb) top just above it at `xy`, if there is one: the
    /// road/terrain height `CityMap::ground_height` knows nothing about the raised sidewalks.
    fn sidewalk_lift(&self, xy: [f32; 2], z: f32) -> f32 {
        if self.city_colliders.sidewalk_triangles == 0 {
            return z;
        }
        // A start inside a building hits at once (from_z itself): ignored by the band.
        match self.physics.static_ground_z(xy, z + 0.4, 0.8) {
            Some(h) if h > z && h < z + 0.35 => h,
            _ => z,
        }
    }

    /// Replace the police system built by [`Sim::set_world`], e.g. with scaled patrol tables or
    /// starting heat (`PoliceSystem::for_world(cfg, self.world(), setup)`). Call after
    /// `set_world` (which rebuilds police) and before the first tick so replays match.
    pub fn set_police(&mut self, police: PoliceSystem) {
        self.clear_police_tools();
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

    /// World pose of city prop `i` (index into `world().static_props()`, the order
    /// [`Sim::set_world`] created them in): box centre, m, and rotation `[x, y, z, w]`. On
    /// hilly cities this is the body as seated on the ground, not the raw prop position.
    /// Read-only (renderers).
    pub fn prop_pose(&self, i: usize) -> Option<([f32; 3], [f32; 4])> {
        let rb = self.physics.bodies.get(*self.props.bodies.get(i)?)?;
        let r = *rb.rotation();
        Some((rb.translation().to_array(), [r.x, r.y, r.z, r.w]))
    }

    /// Indices of the props currently dynamic (near a mover, or knocked and not yet asleep),
    /// ascending. Every other prop is fixed and has not moved since it last was in this list.
    /// Read-only (renderers).
    pub fn active_props(&self) -> impl Iterator<Item = usize> + '_ {
        self.props.active()
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
        if let Some(unit) = slot.police_unit {
            // Player cop seats (`bind_player_police`); AI units are retired by `wp_police`.
            self.police.remove_player_unit(unit);
        }
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

    /// Mutable access to a vehicle's damage model, e.g. to attach hinged panels
    /// (`sim.damage_mut(id)?.attach_panels(&spec)`, workstream R). Changing damage state
    /// mid-run is an input as far as replays are concerned.
    pub fn damage_mut(&mut self, id: EntityId) -> Result<&mut DamageModel, SimError> {
        Ok(&mut self.slot_mut(id)?.damage)
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

    /// Set the road conditions under one vehicle's wheels by hand (wet road tests and tools
    /// until world weather drives every car; `wp_vehicle::Vehicle::set_surface_conditions`).
    /// Not recorded in replays.
    pub fn set_surface_conditions(
        &mut self,
        id: EntityId,
        conditions: [wp_vehicle::surface::SurfaceConditions; 4],
    ) -> Result<(), SimError> {
        self.slot_mut(id)?
            .vehicle
            .set_surface_conditions(conditions);
        Ok(())
    }

    /// Set a vehicle's assist profile.
    pub fn set_assists(&mut self, id: EntityId, assists: AssistProfile) -> Result<(), SimError> {
        self.slot_mut(id)?.assists = assists;
        Ok(())
    }

    /// Set a vehicle's drift-assist tuning (dev override for the testbed's drift lab, owner
    /// rows 90 / 91 / 98; see `wp_vehicle::DriftTuning`). Not recorded in replays or
    /// snapshots: a tuned session does not replay bit-exactly.
    pub fn set_drift_tuning(
        &mut self,
        id: EntityId,
        tuning: wp_vehicle::DriftTuning,
    ) -> Result<(), SimError> {
        self.slot_mut(id)?.vehicle.set_drift_tuning(tuning);
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

    /// Make a vehicle a ghost for `ticks` physics ticks (0 = solid again now): its chassis
    /// passes through other cars, props and debris (no contacts, so no damage either way) but
    /// still rests on and collides with static world geometry; its wheels drive as usual. Used
    /// after a reset in place so the car cannot land stuck in traffic (playtest row 65). When
    /// the window ends while the chassis still overlaps another car, it stays a ghost until it
    /// is clear, at most [`GHOST_MAX_OVERSTAY_TICKS`] longer. An input as far as replays are
    /// concerned (simcli op `Ghost`).
    pub fn set_ghost(&mut self, id: EntityId, ticks: u32) -> Result<(), SimError> {
        let slot = self
            .vehicles
            .iter_mut()
            .find(|v| v.id == id)
            .ok_or(SimError::UnknownEntity(id))?;
        slot.ghost_ticks = ticks;
        slot.ghost_overstay = 0;
        if let Some(col) = self.physics.colliders.get_mut(slot.body.collider) {
            physics::set_ghost_flags(col, ticks > 0);
        }
        Ok(())
    }

    /// Ticks a vehicle stays a ghost ([`Sim::set_ghost`]); 0 = solid.
    pub fn ghost_ticks(&self, id: EntityId) -> Option<u32> {
        self.vehicles
            .iter()
            .find(|v| v.id == id)
            .map(|v| v.ghost_ticks)
    }

    /// Count ghost windows down (before the physics step). A ghost whose window ends while its
    /// chassis box still overlaps another car's stays a ghost one more tick, up to the cap.
    /// Vehicle order: deterministic.
    fn update_ghosts(&mut self) {
        use rapier3d::parry::bounding_volume::BoundingVolume;
        if self.vehicles.iter().all(|v| v.ghost_ticks == 0) {
            return;
        }
        let aabbs: Vec<_> = self
            .vehicles
            .iter()
            .map(|v| {
                self.physics
                    .colliders
                    .get(v.body.collider)
                    .map(|c| c.compute_aabb())
            })
            .collect();
        for (i, slot) in self.vehicles.iter_mut().enumerate() {
            if slot.ghost_ticks == 0 {
                continue;
            }
            slot.ghost_ticks -= 1;
            if slot.ghost_ticks > 0 {
                continue;
            }
            let overlaps = aabbs[i].is_some_and(|me| {
                aabbs
                    .iter()
                    .enumerate()
                    .any(|(j, o)| j != i && o.is_some_and(|o| me.intersects(&o)))
            });
            if overlaps && slot.ghost_overstay < GHOST_MAX_OVERSTAY_TICKS {
                slot.ghost_overstay += 1;
                slot.ghost_ticks = 1;
                continue;
            }
            slot.ghost_overstay = 0;
            if let Some(col) = self.physics.colliders.get_mut(slot.body.collider) {
                physics::set_ghost_flags(col, false);
            }
        }
    }

    /// Repair a vehicle in place: its damage model goes back to pristine (dents, broken parts,
    /// mechanical damage and handling effects), its pose and velocity are kept. The "Repair &
    /// continue" step after a paid repair bill (replay op `Repair`).
    pub fn repair_vehicle(&mut self, id: EntityId) -> Result<(), SimError> {
        let slot = self
            .vehicles
            .iter_mut()
            .find(|v| v.id == id)
            .ok_or(SimError::UnknownEntity(id))?;
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
        // Weather after the world (current clock), before vehicles read it.
        self.step_weather(&mut out);
        if sys.police {
            self.drive_police_units();
        }
        if sys.vehicle {
            // Weather reaches every wheel through `weather_at` (the plain world without weather).
            let wx = self.weather.as_ref().map(|w| w.query(&self.world));
            let world_q: &dyn WorldQuery = match &wx {
                Some(q) => q,
                None => &self.world,
            };
            // Wheel rays never land on a ghost's chassis; a ghost's own wheels only see static
            // world geometry (not other cars, debris or props), like its chassis.
            let not_ghost = |_: rapier3d::prelude::ColliderHandle,
                             c: &rapier3d::prelude::Collider| {
                c.user_data & physics::GHOST_USER_BIT == 0
            };
            let ghost_sees = |_: rapier3d::prelude::ColliderHandle,
                              c: &rapier3d::prelude::Collider| {
                c.user_data & (physics::GHOST_USER_BIT | physics::PROP_USER_BIT) == 0
            };
            for slot in &mut self.vehicles {
                let filter = if slot.ghost_ticks > 0 {
                    QueryFilter::exclude_dynamic()
                        .exclude_rigid_body(slot.body.body)
                        .predicate(&ghost_sees)
                } else {
                    QueryFilter::default()
                        .exclude_rigid_body(slot.body.body)
                        .predicate(&not_ghost)
                };
                let ground = RapierGround {
                    query: self.physics.query_pipeline(filter),
                };
                slot.vehicle.step_with_ground(
                    &slot.input,
                    &slot.assists,
                    world_q,
                    &ground,
                    dt,
                    &mut out,
                );
            }
            for slot in &self.vehicles {
                slot.vehicle
                    .write_body(&mut self.physics.bodies[slot.body.body]);
            }
            self.update_sidewalk_filters();
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
        self.wake_resting_props();
        self.update_ghosts();
        let parked = if sys.damage {
            self.parked_candidates()
        } else {
            Vec::new()
        };
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
                // Hinged panels (workstream R) swing from the chassis motion; no-op without
                // panels attached.
                let rb = &self.physics.bodies[slot.body.body];
                let r = *rb.rotation();
                slot.damage.set_chassis_motion(
                    rb.linvel().to_array(),
                    rb.angvel().to_array(),
                    [r.x, r.y, r.z, r.w],
                    dt,
                );
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
            if !parked.is_empty() {
                self.step_parked_damage(&parked, &infos, dt);
            }
        }
        if sys.vehicle {
            self.sync_traffic();
        }
        if sys.police {
            let racers = self.racer_views();
            let tick_events = out.clone();
            // Fog and rain cap police line of sight at `visibility_m` (the adapter's
            // `line_of_sight`); clear air (800 m) is longer than every police range.
            let wx = self.weather.as_ref().map(|w| w.query(&self.world));
            let world_q: &dyn WorldQuery = match &wx {
                Some(q) => q,
                None => &self.world,
            };
            self.police
                .step(world_q, &racers, &tick_events, dt, &mut out);
            self.reconcile_police_units();
            self.sync_police_tools();
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

    /// Damage-model node positions (chassis space) of a vehicle, indexed by node id. Also
    /// answers for a dented parked car by its [`PARKED_ENTITY_BASE`] id ([`Sim::parked_wrecks`]).
    pub fn node_positions(&self, id: EntityId) -> Result<&[[f32; 3]], SimError> {
        if let Some(w) = parked::prop_of_entity(id).and_then(|i| self.parked.wreck(i)) {
            return Ok(w.damage.node_positions());
        }
        self.vehicles
            .iter()
            .find(|v| v.id == id)
            .map(|v| v.damage.node_positions())
            .ok_or(SimError::UnknownEntity(id))
    }

    /// Stable hash of the whole simulation state (hash spec v4, see `docs/integration/harness.md`):
    /// FNV-1a 64 over `HASH_SPEC_VERSION`, tick, in-game clock (1 ms), [`World::state_hash`]
    /// (signals + traffic), `PoliceSystem::state_hash` (units, heat, pursuits), roadblock
    /// barrier bodies (count, then tool id + position 1e-3 m each), the weather
    /// (`WeatherSim::hash_into`, only when [`Sim::set_weather`] was called), dented parked cars
    /// (count, then prop index, lattice hash and body position (1e-3 m) each; only once a parked
    /// car has been promoted, see `parked.rs`), debris count and every debris body position (1e-3 m) in spawn order,
    /// vehicle count, then every [`Sim::entity_hashes`] value in ascending entity order.
    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        h.write_u32(HASH_SPEC_VERSION);
        h.write_u64(self.tick);
        h.write_f32_q(self.world.clock.time.seconds_since_midnight, 1e3);
        h.write_u64(self.world.state_hash());
        h.write_u64(self.police.state_hash());
        h.write_u32(self.tool_bodies.len() as u32);
        for t in &self.tool_bodies {
            h.write_u32(t.tool.0);
            for x in self.physics.bodies[t.body].translation().to_array() {
                h.write_f32_q(x, 1e3);
            }
        }
        self.hash_weather(&mut h);
        // Dented parked cars (only once one exists, so runs without one keep their hashes).
        if !self.parked.wrecks.is_empty() {
            h.write_u32(self.parked.wrecks.len() as u32);
            for w in &self.parked.wrecks {
                h.write_u32(w.prop as u32);
                h.write_u64(w.damage.state_hash());
                for x in self.physics.bodies[self.props.bodies[w.prop]]
                    .translation()
                    .to_array()
                {
                    h.write_f32_q(x, 1e3);
                }
            }
        }
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
pub const HASH_SPEC_VERSION: u32 = 4;

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

    /// A stock tune spawns exactly the car file; a tuned spawn carries the tuned definition and
    /// differential, and drives differently.
    #[test]
    fn tuned_spawn_applies_the_tune() {
        let car = wp_contracts::load_car(data("cars/rampart_50.json")).unwrap();
        let mode = wp_contracts::load_mode_table(data("modes/arcade.toml")).unwrap();
        let drive = |tune: CarTune| {
            let mut sim = Sim::new(SimConfig {
                seed: 7,
                mode: mode.clone(),
                time_of_day: TimeOfDay::from_hours(12.0),
                time_ratio: 10.0,
            });
            let id = sim.spawn_vehicle_tuned(&car, [0.0; 3], 0.0, tune).unwrap();
            sim.set_input(
                id,
                VehicleInput {
                    throttle: 1.0,
                    ..Default::default()
                },
            )
            .unwrap();
            for _ in 0..480 {
                sim.step_fixed();
            }
            let v = &sim.vehicles()[0];
            (
                v.tune,
                v.vehicle.def().clone(),
                *v.vehicle.diffs(),
                sim.state_hash(),
            )
        };
        let mut plain = Sim::new(SimConfig {
            seed: 7,
            mode: mode.clone(),
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 10.0,
        });
        let id = plain.spawn_vehicle(&car, [0.0; 3], 0.0).unwrap();
        plain
            .set_input(
                id,
                VehicleInput {
                    throttle: 1.0,
                    ..Default::default()
                },
            )
            .unwrap();
        for _ in 0..480 {
            plain.step_fixed();
        }
        let (t, def, _, hash) = drive(CarTune::STOCK);
        assert_eq!(t, CarTune::STOCK);
        assert_eq!(def, car.def);
        assert_eq!(hash, plain.state_hash());

        let owner = CarTune {
            suspension: 5,
            gearing: 5,
            diff: 5,
            brake_balance: 2,
        };
        let (t, def, diffs, tuned_hash) = drive(owner);
        assert_eq!(t, owner);
        assert!(def.drivetrain.final_drive < car.def.drivetrain.final_drive);
        assert_eq!(diffs.rear, wp_vehicle::AxleDiff::Locked);
        assert_ne!(tuned_hash, hash);
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

    fn two_sedan_sim(seed: u64) -> (Sim, EntityId, EntityId) {
        let car = wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap();
        let mode = wp_contracts::load_mode_table(data("modes/sim.toml")).unwrap();
        let mut sim = Sim::new(SimConfig {
            seed,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 10.0,
        });
        let a = sim.spawn_vehicle(&car, [0.0; 3], 0.0).unwrap();
        let b = sim.spawn_vehicle(&car, [0.0, 12.0, 0.0], 0.0).unwrap();
        (sim, a, b)
    }

    /// A ghost drives through a parked car (no contact, no damage, the parked car does not
    /// move) while still resting on the ground; static geometry stays solid for it; once the
    /// window is over it is solid again and the same run crashes.
    #[test]
    fn ghost_passes_through_cars_but_not_the_world() {
        let run = |ghost: bool| {
            let (mut sim, a, _b) = two_sedan_sim(7);
            sim.place_vehicle(a, [0.0; 3], 0.0, 10.0).unwrap();
            let hold = VehicleInput {
                throttle: 0.3,
                ..Default::default()
            };
            sim.set_input(a, hold).unwrap();
            if ghost {
                sim.set_ghost(a, 480).unwrap();
            }
            for _ in 0..480 {
                sim.step_fixed();
            }
            // Car-to-car contacts only (ground contacts are reported against WORLD_ENTITY).
            let contacts = sim
                .drain_events()
                .iter()
                .filter(|e| matches!(e, SimEvent::Contact { b, .. } if *b != wp_contracts::WORLD_ENTITY))
                .count();
            let st = sim.vehicle_states();
            (
                contacts,
                st[0].1.clone(),
                st[1].1.clone(),
                sim.ghost_ticks(a),
                sim.state_hash(),
            )
        };
        let (contacts, a, b, left, h1) = run(true);
        assert_eq!(contacts, 0, "a ghost makes no contacts with the other car");
        assert!(a.pos[1] > 20.0, "the ghost drove through: y = {}", a.pos[1]);
        assert!(
            a.pos[2].abs() < 1.0,
            "the ghost still rests on the ground: z = {}",
            a.pos[2]
        );
        assert!(
            a.health > 0.999 && b.health > 0.999,
            "no damage ({}, {})",
            a.health,
            b.health
        );
        assert!(
            b.pos[1] > 11.9 && b.pos[1] < 12.1 && b.speed_mps.abs() < 0.1,
            "parked car untouched"
        );
        assert_eq!(left, Some(0), "window over, solid again");
        assert_eq!(h1, run(true).4, "ghost runs are deterministic");
        let (contacts, _, b, _, _) = run(false);
        assert!(
            contacts > 0 && (b.pos[1] - 12.0).abs() > 0.2,
            "without the ghost the same run crashes ({contacts} contacts, parked car at y = {})",
            b.pos[1]
        );

        // Static world geometry stays solid for a ghost.
        let (mut sim, a, _) = two_sedan_sim(8);
        sim.add_static_box([5.0, 20.0, 0.75], [4.0, 1.0, 1.5], 0.0);
        sim.place_vehicle(a, [5.0, 0.0, 0.0], 0.0, 15.0).unwrap();
        sim.set_ghost(a, 720).unwrap();
        for _ in 0..720 {
            sim.step_fixed();
        }
        let y = sim.vehicle_states()[0].1.pos[1];
        assert!(y < 20.0, "a ghost still hits a barrier: y = {y}");
    }

    /// A ghost whose window ends inside another car stays a ghost until it is clear (capped).
    #[test]
    fn ghost_overstays_while_overlapping() {
        let (mut sim, a, b) = two_sedan_sim(9);
        sim.place_vehicle(a, [0.0, 11.0, 0.0], 0.0, 0.0).unwrap();
        sim.set_ghost(a, 10).unwrap();
        for _ in 0..60 {
            sim.step_fixed();
        }
        assert!(
            sim.ghost_ticks(a).unwrap() > 0,
            "still overlapping: still a ghost"
        );
        assert!(sim.ghost_ticks(b) == Some(0));
        // Drive clear: it turns solid once out.
        sim.set_input(
            a,
            VehicleInput {
                throttle: 0.5,
                ..Default::default()
            },
        )
        .unwrap();
        for _ in 0..(GHOST_MAX_OVERSTAY_TICKS + 240) {
            sim.step_fixed();
        }
        assert_eq!(
            sim.ghost_ticks(a),
            Some(0),
            "solid again after leaving (or at the cap)"
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
