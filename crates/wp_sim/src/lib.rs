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
mod crash_mass;
mod nitrous_link;
mod parked;
mod physics;
mod police_link;
mod police_tools;
mod props;
mod ram;
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
pub use police_link::{PoliceModel, POLICE_SLOT_RESERVE};
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
    /// Damage mode table (Arcade, Simulation or Extreme).
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

/// Multiplier on the contact impulses reaching a vehicle's damage model, from the mode table:
/// the player's racer `racer_damage_scale`, AI racers `racer_damage_scale ×
/// ai_racer_damage_mult`, AI police `police_damage_scale`, player cop seats
/// (`player_driven_police`) and traffic 1.0. Damage only: the rigid-body response is never
/// scaled.
pub fn damage_scale(kind: VehicleKind, player_driven_police: bool, mode: &ModeTable) -> f32 {
    match kind {
        VehicleKind::Player => mode.racer_damage_scale,
        VehicleKind::AiRacer => mode.racer_damage_scale * mode.ai_racer_damage_mult,
        VehicleKind::Police if player_driven_police => 1.0,
        VehicleKind::Police => mode.police_damage_scale,
        VehicleKind::Traffic => 1.0,
    }
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
    /// Player steering aids override ([`Sim::set_player_steering`]); `None` = from `kind`.
    player_steering: Option<bool>,
    /// Ticks the ghost was extended because the car still overlapped another car.
    ghost_overstay: u32,
    /// Tyre condition set by the career ([`Sim::set_tire_condition`]): per-wheel grip
    /// multiplier and deflation, applied on top of damage every tick.
    tire_condition: Option<([f32; 4], [f32; 4])>,
    /// Battering-ram faces from its mods ([`Sim::set_ram_profile`]); stock by default.
    ram: wp_economy::RamProfile,
    /// Run-flat tyres fitted ([`Sim::set_run_flat`]): punctures never deflate a tyre past
    /// [`wp_vehicle::vehicle::RUN_FLAT_MAX_DEFLATION`].
    run_flat: bool,
    /// Reinforced cooling tier ([`Sim::set_cooling_tier`]): 0 stock; higher tiers slow the
    /// overheat of a failed radiator ([`COOLING_OVERHEAT_SCALE`]).
    cooling_tier: u8,
    /// Dormant ([`Sim::set_dormant`]): a fixed body, no vehicle or damage-motion step.
    dormant: bool,
    /// May drive in light mode far from every player ([`Sim::set_light_when_far`]).
    light_when_far: bool,
    /// Light mode now: a kinematic road follower instead of full vehicle physics.
    light: Option<LightState>,
    /// AI nitrous fires this tick (`nitrous_link`; AI racers and police only).
    nitrous_fire: bool,
    /// Nitrous Kit tier of a player's car ([`Sim::set_nitrous_kit`]; 0 = no bottle).
    nitrous_kit: u8,
}

/// A vehicle in light mode ([`Sim::set_light_when_far`]): heading, speed and the chassis
/// height above the ground it had when it went light.
#[derive(Clone, Copy, Debug)]
struct LightState {
    yaw: f32,
    speed: f32,
    ride_z: f32,
}

/// A light-mode vehicle ([`Sim::set_light_when_far`]) goes light farther than this from every
/// player car, m.
pub const LIGHT_ON_M: f32 = 350.0;
/// ... and back to full physics closer than this, m (hysteresis; far beyond the distance a
/// car is drawn in detail).
pub const LIGHT_OFF_M: f32 = 250.0;
/// Light mode's acceleration at full throttle, m/s².
const LIGHT_ACCEL_MPS2: f32 = 3.5;
/// Light mode's deceleration at full brake (or handbrake), m/s².
const LIGHT_BRAKE_MPS2: f32 = 7.0;
/// Light mode's rolling resistance and drag, as a deceleration, m/s².
const LIGHT_COAST_MPS2: f32 = 0.4;

/// A dormant vehicle wakes when an awake vehicle's centre comes this close, m
/// ([`Sim::set_dormant`]): well before the two can touch at any speed a car reaches in
/// `DORMANT_WAKE_M / speed` seconds, so a collision always finds it dynamic.
pub const DORMANT_WAKE_M: f32 = 40.0;
/// Only an awake vehicle faster than this wakes dormant ones nearby, m/s.
pub const DORMANT_WAKE_MIN_SPEED_MPS: f32 = 0.5;

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
    /// Multi-model police ([`Sim::set_police_cars`]); empty = the single `police_car`.
    police_fleet: Vec<police_link::PoliceModel>,
    /// Fleet index chosen for each police car unit (assigned once, in unit order).
    police_unit_models: std::collections::BTreeMap<wp_police::UnitId, usize>,
    /// Physical traffic models ([`Sim::set_traffic_cars`]); agent → `wp_world` model index.
    traffic_cars: Vec<LoadedCar>,
    /// `TrafficConfig::max_physical` as configured, before [`Sim::cap_traffic_promotion`] caps
    /// it to the vehicle slots left (`None` until the first capped step).
    traffic_physical_base: Option<usize>,
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
            police_fleet: Vec::new(),
            police_unit_models: std::collections::BTreeMap::new(),
            traffic_cars: Vec::new(),
            traffic_physical_base: None,
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
            nitrous_fire: false,
            nitrous_kit: 0,
            tire_condition: None,
            ram: wp_economy::RamProfile::default(),
            run_flat: false,
            cooling_tier: 0,
            player_steering: None,
            dormant: false,
            light_when_far: false,
            light: None,
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
            pitch: 0.0,
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
    /// terrain: on the ground under it, pitched to the road; guard rails keep their laid-out
    /// pose).
    fn insert_prop(
        &mut self,
        prop: &wp_world::props::StaticProp,
        seated: Option<rapier3d::prelude::Pose>,
    ) {
        use rapier3d::math::Rotation;
        use rapier3d::prelude::{ColliderBuilder, Pose, RigidBodyBuilder, Vector};
        let hilly = self.world.network().terrain.is_some();
        // Yaw about +Z, then pitch about the prop's own +X: q = q_z(yaw) · q_x(pitch).
        let (sz, cz) = wp_math::sin_cos(prop.yaw * 0.5);
        let (sx, cx) = wp_math::sin_cos(prop.pitch * 0.5);
        let mut pose = Pose::from_parts(
            Vector::from_array(prop.pos),
            Rotation::from_xyzw(cz * sx, sz * sx, sz * cx, cz * cx),
        );
        // Guard rails are laid out on their own line (pitched to the grade, ends shared with
        // their neighbours): seating each on the ground under its middle would tilt and roll
        // it off that line.
        let laid_out = prop.kind == wp_world::props::PropKind::GuardRail;
        if let Some(p) = seated {
            pose = p;
        } else if hilly && !laid_out {
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

    /// Override whether a vehicle gets the player steering aids (`wp_vehicle::lab_scope::plain`):
    /// `Some(false)` for a harness or test driver that feeds analog inputs (simcli's scripted
    /// drivers), `Some(true)` to force them, `None` (the default) = only
    /// [`VehicleKind::Player`] cars. The physics is the same either way.
    pub fn set_player_steering(&mut self, id: EntityId, on: Option<bool>) -> Result<(), SimError> {
        self.slot_mut(id)?.player_steering = on;
        Ok(())
    }

    /// Set the Nitrous Kit tier of a player's car (0 = none, the default: a player's car has
    /// no nitrous without the kit; owner decision 2026-10-26). AI racers and police always
    /// carry a bottle, scaled by the rubberbanding (`nitrous_link`), whatever this says.
    pub fn set_nitrous_kit(&mut self, id: EntityId, tier: u8) -> Result<(), SimError> {
        self.slot_mut(id)?.nitrous_kit = tier;
        Ok(())
    }

    /// Re-apply a vehicle's body inertia (`wp_vehicle::Vehicle::body_inertia`) after a lab
    /// ride override changed it (the hidden tuning panel). Lab-only: not a replay op.
    pub fn refresh_body_inertia(&mut self, id: EntityId) -> Result<(), SimError> {
        let slot = self
            .vehicles
            .iter_mut()
            .find(|v| v.id == id)
            .ok_or(SimError::UnknownEntity(id))?;
        slot.vehicle
            .apply_body_inertia(&mut self.physics.bodies[slot.body.body]);
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
        // A placed car is awake and on full physics (a reset, a respawn, a move out of the
        // lockup).
        if slot.dormant || slot.light.is_some() {
            slot.dormant = false;
            slot.light = None;
            rb.set_body_type(rapier3d::prelude::RigidBodyType::Dynamic, true);
        }
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

    /// Swap the damage mode table (Arcade / Simulation / Extreme) without restarting. Applies from the
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

    /// Set a vehicle's tyre condition (career tyres, `wp_economy::lots`): per-wheel grip
    /// multiplier (1 = new tyres, FL, FR, RL, RR) and deflation 0..=1 (1 = flat), applied on
    /// top of damage every tick. Like mods, not recorded in replays: set it before the first
    /// tick of a session.
    pub fn set_tire_condition(
        &mut self,
        id: EntityId,
        grip: [f32; 4],
        deflation: [f32; 4],
    ) -> Result<(), SimError> {
        self.slot_mut(id)?.tire_condition = Some((grip, deflation));
        Ok(())
    }

    /// Set a vehicle's battering-ram faces (its mods' `wp_economy::AppliedMods::ram`): more
    /// effective mass and damage dealt, less damage taken, in car-on-car hits on those faces
    /// (see `ram.rs`). Like mods, not recorded in replays: set it before the first tick.
    pub fn set_ram_profile(
        &mut self,
        id: EntityId,
        profile: wp_economy::RamProfile,
    ) -> Result<(), SimError> {
        self.slot_mut(id)?.ram = profile;
        Ok(())
    }

    /// Fit or remove run-flat tyres on a vehicle (the career's `tires_runflat` mod,
    /// `wp_economy::ToolLoadout::run_flat` > 0). Spike strips, crash damage and a carried
    /// flat still puncture the tyres, but the handling never sees more than
    /// [`wp_vehicle::vehicle::RUN_FLAT_MAX_DEFLATION`]: the car limps on instead of riding on
    /// its rims. Like mods, not recorded in replays: set it before the first tick of a session.
    pub fn set_run_flat(&mut self, id: EntityId, fitted: bool) -> Result<(), SimError> {
        self.slot_mut(id)?.run_flat = fitted;
        Ok(())
    }

    /// Set a vehicle's reinforced cooling tier (the career's radiator / oil cooler mods,
    /// `wp_economy::ToolLoadout::reinforced_cooling`; 0 = stock): a failed radiator heats the
    /// engine [`COOLING_OVERHEAT_SCALE`] as fast, so the car limps further before it cooks.
    /// Like mods, not recorded in replays: set it before the first tick of a session.
    pub fn set_cooling_tier(&mut self, id: EntityId, tier: u8) -> Result<(), SimError> {
        self.slot_mut(id)?.cooling_tier = tier;
        Ok(())
    }

    /// Fit the ABS hardware of a vehicle (the career's `brakes_abs_motorsport` mod,
    /// `wp_economy::ToolLoadout::motorsport_abs` > 0 → [`wp_vehicle::AbsHardware::Motorsport`]).
    /// The driver's ABS level still decides whether it works: motorsport ABS replaces ABS Full.
    /// Like mods, not recorded in replays: set it before the first tick of a session.
    pub fn set_abs_hardware(
        &mut self,
        id: EntityId,
        hardware: wp_vehicle::AbsHardware,
    ) -> Result<(), SimError> {
        self.slot_mut(id)?.vehicle.set_abs_hardware(hardware);
        Ok(())
    }

    /// Tyre deflation a vehicle drives on (FL, FR, RL, RR; 0 inflated … 1 flat): the worst of
    /// crash damage, spike strips and the career tyre condition, capped by run-flat tyres.
    /// Exactly what the handling uses; for the HUD and audio.
    pub fn tire_deflation(&self, id: EntityId) -> Result<[f32; 4], SimError> {
        let slot = self
            .vehicles
            .iter()
            .find(|v| v.id == id)
            .ok_or(SimError::UnknownEntity(id))?;
        let mut effects = slot.damage.handling_effects();
        merge_mod_effects(&mut effects, self.police.tire_puncture(id), slot);
        Ok(effects.tire_puncture)
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

    /// Put a vehicle to sleep (`on`) or wake it. A dormant vehicle's chassis becomes a fixed
    /// body where it stands, at rest: no tyre, suspension or drivetrain step, no forces, no
    /// damage chassis motion, but its collider stays, so other cars and debris still hit it
    /// (as a parked car). It wakes by itself, deterministically, when a moving vehicle comes
    /// within [`DORMANT_WAKE_M`], when anything touches it, and when it is placed or reset.
    /// For cars that wait out of play (off-shift street racers in their lockup, SR-3); callers
    /// that record replays log it (`simcli` `OpKind::Dormant`).
    pub fn set_dormant(&mut self, id: EntityId, on: bool) -> Result<(), SimError> {
        let slot = self
            .vehicles
            .iter_mut()
            .find(|v| v.id == id)
            .ok_or(SimError::UnknownEntity(id))?;
        if slot.dormant == on {
            return Ok(());
        }
        slot.dormant = on;
        let rb = &mut self.physics.bodies[slot.body.body];
        use rapier3d::prelude::{RigidBodyType, Vector};
        if on {
            rb.set_body_type(RigidBodyType::Fixed, true);
            rb.set_linvel(Vector::ZERO, false);
            rb.set_angvel(Vector::ZERO, false);
            rb.reset_forces(false);
            rb.reset_torques(false);
        } else {
            rb.set_body_type(RigidBodyType::Dynamic, true);
        }
        slot.vehicle.read_body(rb);
        Ok(())
    }

    /// Let a vehicle drive in light mode far from every player car (`on`), or never. In light
    /// mode — farther than [`LIGHT_ON_M`] from every `VehicleKind::Player` car, back to full
    /// physics within [`LIGHT_OFF_M`] or on any contact — the chassis is a kinematic body that
    /// follows the ground under it (`CityMap::ground_height`), driven by the same inputs through
    /// a bicycle model (throttle / brake as accelerations, steer through the wheelbase): no
    /// tyre, suspension or drivetrain step, no damage motion. For AI cars far out of play
    /// (roaming street racers, SR-3). Deterministic; replays log the setting (`simcli`
    /// `OpKind::LightWhenFar`), the switches follow from the state.
    pub fn set_light_when_far(&mut self, id: EntityId, on: bool) -> Result<(), SimError> {
        let slot = self
            .vehicles
            .iter_mut()
            .find(|v| v.id == id)
            .ok_or(SimError::UnknownEntity(id))?;
        slot.light_when_far = on;
        if !on && slot.light.is_some() {
            Self::leave_light(slot, &mut self.physics.bodies[slot.body.body]);
        }
        Ok(())
    }

    /// The vehicle is in light mode now ([`Sim::set_light_when_far`]).
    pub fn is_light(&self, id: EntityId) -> Option<bool> {
        self.vehicles
            .iter()
            .find(|v| v.id == id)
            .map(|v| v.light.is_some())
    }

    /// Back to full physics where the light vehicle is, at its speed.
    fn leave_light(slot: &mut SimVehicle, rb: &mut rapier3d::prelude::RigidBody) {
        let Some(l) = slot.light.take() else {
            return;
        };
        rb.set_body_type(rapier3d::prelude::RigidBodyType::Dynamic, true);
        let p = rb.translation().to_array();
        slot.vehicle.place_body(rb, p, l.yaw, l.speed, 0.0);
    }

    /// Light-mode switches before the vehicle step (see [`Sim::set_light_when_far`]).
    fn update_light_modes(&mut self) {
        if !self
            .vehicles
            .iter()
            .any(|v| v.light_when_far || v.light.is_some())
        {
            return;
        }
        let players: Vec<[f32; 2]> = self
            .vehicles
            .iter()
            .filter(|v| v.kind == VehicleKind::Player)
            .map(|v| {
                let t = self.physics.bodies[v.body.body].translation();
                [t.x, t.y]
            })
            .collect();
        let near = |p: [f32; 2], r: f32| {
            players.iter().any(|q| {
                let (dx, dy) = (p[0] - q[0], p[1] - q[1]);
                dx * dx + dy * dy < r * r
            })
        };
        let map = self.world.map();
        for slot in &mut self.vehicles {
            let rb = &mut self.physics.bodies[slot.body.body];
            let t = rb.translation();
            let p = [t.x, t.y];
            match slot.light {
                Some(_) => {
                    if !slot.light_when_far || near(p, LIGHT_OFF_M) {
                        Self::leave_light(slot, rb);
                    }
                }
                None => {
                    if !slot.light_when_far
                        || slot.dormant
                        || slot.ghost_ticks > 0
                        || players.is_empty()
                        || near(p, LIGHT_ON_M)
                    {
                        continue;
                    }
                    // Upright and on the ground only (not mid-crash or airborne).
                    let st = slot.vehicle.state();
                    let up = rb.rotation() * rapier3d::math::Vector::Z;
                    let ground = map.ground_height(p, Some(t.z));
                    let ride_z = t.z - ground;
                    if up.z < 0.95 || !(-0.5..1.5).contains(&ride_z) {
                        continue;
                    }
                    let q = st.rot;
                    let fx = 2.0 * (q[0] * q[1] - q[3] * q[2]);
                    let fy = 1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]);
                    let yaw = wp_math::atan2(-fx, fy);
                    rb.set_body_type(
                        rapier3d::prelude::RigidBodyType::KinematicPositionBased,
                        true,
                    );
                    slot.light = Some(LightState {
                        yaw,
                        speed: st.speed_mps.max(0.0),
                        ride_z,
                    });
                }
            }
        }
    }

    /// Light-mode vehicles drive one tick: the bicycle model on their inputs, then the next
    /// kinematic pose on the ground (pitched and rolled to it).
    fn step_light(&mut self, dt: f32) {
        let map = self.world.map();
        for slot in self.vehicles.iter_mut() {
            let Some(l) = slot.light.as_mut() else {
                continue;
            };
            let def = slot.vehicle.def();
            let i = &slot.input;
            let brake = i.brake.max(i.handbrake).clamp(0.0, 1.0);
            let a = i.throttle.clamp(0.0, 1.0) * LIGHT_ACCEL_MPS2
                - brake * LIGHT_BRAKE_MPS2
                - LIGHT_COAST_MPS2;
            l.speed = (l.speed + a * dt).max(0.0);
            let steer = i.steer.clamp(-1.0, 1.0) * def.wheels[0].max_steer_rad;
            // Small-angle bicycle model (steer is small at speed). Steer +1 is full right, a
            // right turn lowers yaw (0 = +Y, counter-clockwise positive).
            l.yaw -= l.speed * steer / def.wheelbase_m.max(0.5) * dt;
            let (s, c) = wp_math::sin_cos(l.yaw);
            let rb = &mut self.physics.bodies[slot.body.body];
            let t = rb.translation();
            let xy = [t.x - s * l.speed * dt, t.y + c * l.speed * dt];
            let (mut pos, rot) = ground_pose(map, xy, l.yaw, Some(t.z - l.ride_z));
            pos[2] += l.ride_z;
            rb.set_next_kinematic_position(rapier3d::prelude::Pose::from_parts(
                rapier3d::math::Vector::from_array(pos),
                rot,
            ));
        }
    }

    /// The vehicle is dormant ([`Sim::set_dormant`]).
    pub fn is_dormant(&self, id: EntityId) -> Option<bool> {
        self.vehicles.iter().find(|v| v.id == id).map(|v| v.dormant)
    }

    /// Wake dormant vehicles an awake vehicle has come near (before the physics step).
    fn wake_dormant_near(&mut self) {
        if !self.vehicles.iter().any(|v| v.dormant) {
            return;
        }
        // Moving cars only: two cars parked side by side would wake each other (a standing car
        // that starts off nearby wakes it within a few ticks, or on contact).
        let awake: Vec<[f32; 3]> = self
            .vehicles
            .iter()
            .filter(|v| !v.dormant)
            .map(|v| &self.physics.bodies[v.body.body])
            .filter(|rb| {
                rb.linvel().length_squared()
                    > DORMANT_WAKE_MIN_SPEED_MPS * DORMANT_WAKE_MIN_SPEED_MPS
            })
            .map(|rb| rb.translation().to_array())
            .collect();
        let r2 = DORMANT_WAKE_M * DORMANT_WAKE_M;
        let wake: Vec<EntityId> = self
            .vehicles
            .iter()
            .filter(|v| v.dormant)
            .filter(|v| {
                let p = self.physics.bodies[v.body.body].translation().to_array();
                awake.iter().any(|q| {
                    let (dx, dy, dz) = (p[0] - q[0], p[1] - q[1], p[2] - q[2]);
                    dx * dx + dy * dy + dz * dz < r2
                })
            })
            .map(|v| v.id)
            .collect();
        for id in wake {
            let _ = self.set_dormant(id, false);
        }
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
            self.cap_traffic_promotion();
            self.world.step(dt, &mut out);
            self.apply_traffic_lod();
            self.drive_traffic();
        }
        // Weather after the world (current clock), before vehicles read it.
        self.step_weather(&mut out);
        if sys.police {
            self.drive_police_units();
        }
        self.wake_dormant_near();
        self.update_light_modes();
        if sys.vehicle {
            self.update_ai_nitrous();
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
            for slot in self
                .vehicles
                .iter_mut()
                .filter(|v| !v.dormant && v.light.is_none())
            {
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
                // One physics for every car; the plain keyboard steering aids only for a
                // player's car (AI racers, police and traffic steer with their own model).
                slot.vehicle.set_player_input(
                    slot.player_steering
                        .unwrap_or(slot.kind == VehicleKind::Player),
                );
                slot.vehicle.step_with_ground(
                    &nitrous_link::with_nitrous(&slot.input, slot.nitrous_fire),
                    &slot.assists,
                    world_q,
                    &ground,
                    dt,
                    &mut out,
                );
            }
            for slot in self
                .vehicles
                .iter()
                .filter(|v| !v.dormant && v.light.is_none())
            {
                slot.vehicle
                    .write_body(&mut self.physics.bodies[slot.body.body]);
            }
            self.step_light(dt);
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
        // Battering-ram mods: effective mass (body impulses, before the vehicles read their
        // bodies) and damage dealt / taken in car-on-car hits. On the solver's impulses, before
        // any damage scaling below.
        let ram_cars: Vec<ram::RamCar> = self
            .vehicles
            .iter()
            .map(|v| ram::RamCar {
                id: v.id,
                body: v.body.body,
                profile: v.ram,
            })
            .collect();
        ram::apply(
            &ram_cars,
            &mut tick_contacts.per_vehicle,
            &mut self.physics.bodies,
        );
        // Arcade: each vehicle kind takes its own share of the collision impulse (player
        // racer 0.25, AI racers 0.5, AI police 2.0, player cops and traffic all of it). Before the filter, so
        // the scaled contacts are the ones quantised, recorded and replicated.
        for (slot, contacts) in self.vehicles.iter().zip(&mut tick_contacts.per_vehicle) {
            let scale = damage_scale(slot.kind, slot.player_police.is_some(), &self.config.mode);
            wp_damage::scale_contacts(contacts, scale);
        }
        if let Some(filter) = self.contact_filter {
            for c in tick_contacts.per_vehicle.iter_mut().flatten() {
                filter(c);
            }
        }
        if sys.vehicle {
            for slot in self.vehicles.iter_mut().filter(|v| !v.dormant) {
                slot.vehicle.read_body(&self.physics.bodies[slot.body.body]);
            }
        }
        // A hard hit wrecks a traffic car: it stops with hazards on and stays put. Judged on
        // the impulse, not the damage, so both damage modes react the same (owner rule).
        for (slot, contacts) in self.vehicles.iter().zip(&tick_contacts.per_vehicle) {
            let Some(agent) = slot.traffic_agent else {
                continue;
            };
            if contacts.is_empty() {
                continue;
            }
            let mass = self.physics.bodies[slot.body.body].mass();
            let dv = impact_delta_v(contacts, mass);
            self.world.report_agent_impact(agent, dv);
        }
        // Anything that touched a dormant car wakes it for the next tick; a light one goes back
        // to full physics.
        let touched: Vec<EntityId> = self
            .vehicles
            .iter()
            .zip(&tick_contacts.per_vehicle)
            .filter(|(v, c)| (v.dormant || v.light.is_some()) && !c.is_empty())
            .map(|(v, _)| v.id)
            .collect();
        for id in touched {
            let _ = self.set_dormant(id, false);
            if let Some(slot) = self.vehicles.iter_mut().find(|v| v.id == id) {
                Self::leave_light(slot, &mut self.physics.bodies[slot.body.body]);
            }
        }
        if sys.damage {
            for (slot, contacts) in self.vehicles.iter_mut().zip(&tick_contacts.per_vehicle) {
                if (slot.dormant || slot.light.is_some()) && contacts.is_empty() {
                    continue;
                }
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
                merge_mod_effects(&mut effects, self.police.tire_puncture(slot.id), slot);
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
        // Light-mode cars are kinematic far from the player: they never wake (and shove) props.
        let movers: Vec<props::Mover> = self
            .vehicles
            .iter()
            .filter(|v| v.light.is_none())
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

/// Velocity change one tick's contacts give a body of `mass_kg`, m/s: the magnitude of the
/// summed normal and friction impulses (chassis space) over the mass.
/// Overheat rate of a failed radiator per reinforced cooling tier (index = tier, clamped):
/// stock, aluminium radiator / oil cooler, race radiator.
pub const COOLING_OVERHEAT_SCALE: [f32; 3] = [1.0, 0.7, 0.5];

/// Merge spike-strip deflation (`wp_police`) and the career tyre condition (worn tread grips
/// less, a flat stays flat) into a vehicle's damage effects, cap every tyre's deflation for
/// run-flats ([`wp_vehicle::vehicle::run_flat_deflation`]) and slow the overheat for
/// reinforced cooling ([`COOLING_OVERHEAT_SCALE`]).
fn merge_mod_effects(
    effects: &mut wp_contracts::HandlingEffects,
    spikes: [f32; 4],
    slot: &SimVehicle,
) {
    for (t, s) in effects.tire_puncture.iter_mut().zip(spikes) {
        *t = t.max(s);
    }
    if let Some((grip, deflation)) = slot.tire_condition {
        for w in 0..4 {
            effects.grip_scale[w] *= grip[w];
            effects.tire_puncture[w] = effects.tire_puncture[w].max(deflation[w]);
        }
    }
    for t in &mut effects.tire_puncture {
        *t = wp_vehicle::vehicle::run_flat_deflation(*t, slot.run_flat);
    }
    let tier = (slot.cooling_tier as usize).min(COOLING_OVERHEAT_SCALE.len() - 1);
    effects.overheating *= COOLING_OVERHEAT_SCALE[tier];
}

fn impact_delta_v(contacts: &[wp_contracts::ContactImpulse], mass_kg: f32) -> f32 {
    if mass_kg <= 0.0 {
        return 0.0;
    }
    let mut j = [0.0f32; 3];
    for c in contacts {
        for (k, jk) in j.iter_mut().enumerate() {
            *jk += c.normal_local[k] * c.impulse_ns + c.tangent_impulse_local_ns[k];
        }
    }
    (j[0] * j[0] + j[1] * j[1] + j[2] * j[2]).sqrt() / mass_kg
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
        let mode = wp_contracts::load_mode_table(data("modes/sim.toml")).unwrap();
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
        let mode = wp_contracts::load_mode_table(data("modes/sim.toml")).unwrap();
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

    /// Health of four sedans after each hits its own rigid wall at 56 km/h, one per
    /// `VehicleKind` (Player, AiRacer, Police, Traffic), with damage table `file`.
    fn wall_hit_health(file: &str) -> Vec<(VehicleKind, f32)> {
        let car = wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap();
        let mode = wp_contracts::load_mode_table(data(file)).unwrap();
        let mut sim = Sim::new(SimConfig {
            seed: 9,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 1.0,
        });
        let kinds = [
            VehicleKind::Player,
            VehicleKind::AiRacer,
            VehicleKind::Police,
            VehicleKind::Traffic,
        ];
        let mut ids = Vec::new();
        for (i, kind) in kinds.iter().enumerate() {
            let x = 20.0 * i as f32;
            sim.add_static_box([x, 20.0, 0.75], [8.0, 1.0, 1.5], 0.0);
            let id = sim.spawn_vehicle(&car, [x, 0.0, 0.0], 0.0).unwrap();
            sim.place_vehicle(id, [x, 0.0, 0.0], 0.0, 15.56).unwrap();
            sim.set_vehicle_kind(id, *kind).unwrap();
            ids.push(id);
        }
        for _ in 0..720 {
            sim.step_fixed();
        }
        sim.vehicles()
            .iter()
            .map(|v| (v.kind, v.damage.health()))
            .collect()
    }

    /// Contract v0.4.0 (owner, 2026-10-22): in Arcade racers take reduced damage and traffic
    /// takes the same damage as in Simulation. Owner 2026-10-26: AI racers take 2x the
    /// player's share and AI police 2x full damage. In Simulation every kind takes the same.
    #[test]
    fn arcade_scales_damage_per_kind() {
        let arcade = wall_hit_health("modes/arcade.toml");
        let sim = wall_hit_health("modes/sim.toml");
        let lost = |h: &[(VehicleKind, f32)], k: VehicleKind| {
            1.0 - h.iter().find(|(kind, _)| *kind == k).unwrap().1
        };
        let (a, s) = (
            lost(&arcade, VehicleKind::Traffic),
            lost(&sim, VehicleKind::Traffic),
        );
        assert!(
            (a - s).abs() < 1e-3,
            "traffic takes full damage: {a} vs {s}"
        );
        for k in [VehicleKind::Player, VehicleKind::AiRacer] {
            let ratio = lost(&arcade, k) / lost(&sim, k);
            assert!(
                ratio < 0.6,
                "{k:?}: Arcade health lost {ratio:.2} of Simulation's"
            );
            assert!(
                lost(&sim, k) > 0.02,
                "{k:?}: the Simulation hit does damage"
            );
        }
        let (player, ai) = (
            lost(&arcade, VehicleKind::Player),
            lost(&arcade, VehicleKind::AiRacer),
        );
        assert!(
            ai > 1.4 * player,
            "AI racer takes about 2x the player's damage: {ai} vs {player}"
        );
        let (a, s) = (
            lost(&arcade, VehicleKind::Police),
            lost(&sim, VehicleKind::Police),
        );
        assert!(
            a > 1.4 * s,
            "AI police take about 2x full damage: {a} vs {s}"
        );
        let sim_lost: Vec<f32> = sim.iter().map(|(_, h)| 1.0 - h).collect();
        assert!(
            sim_lost.windows(2).all(|w| (w[0] - w[1]).abs() < 0.05),
            "Simulation: every kind takes about the same damage {sim_lost:?}"
        );
    }

    /// Player cop seats keep full damage; only AI police get `police_damage_scale`.
    #[test]
    fn damage_scale_by_kind() {
        let arcade = wp_contracts::load_mode_table(data("modes/arcade.toml")).unwrap();
        assert_eq!(damage_scale(VehicleKind::Player, false, &arcade), 0.25);
        assert_eq!(damage_scale(VehicleKind::AiRacer, false, &arcade), 0.5);
        assert_eq!(damage_scale(VehicleKind::Police, false, &arcade), 2.0);
        assert_eq!(damage_scale(VehicleKind::Police, true, &arcade), 1.0);
        assert_eq!(damage_scale(VehicleKind::Traffic, false, &arcade), 1.0);
        let sim = wp_contracts::load_mode_table(data("modes/sim.toml")).unwrap();
        for k in [
            VehicleKind::Player,
            VehicleKind::AiRacer,
            VehicleKind::Police,
        ] {
            assert_eq!(damage_scale(k, false, &sim), 1.0);
        }
    }

    /// Two sedans 30 m apart, 10 m/s each, head-on. Returns every event of a 3 s run.
    fn head_on_events() -> Vec<SimEvent> {
        let car = wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap();
        let mode = wp_contracts::load_mode_table(data("modes/extreme.toml")).unwrap();
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
        let mode = wp_contracts::load_mode_table(data("modes/extreme.toml")).unwrap();
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

    /// SR-3: a dormant car is a fixed body at rest (throttle does nothing); a moving car that
    /// comes near wakes it before they touch, and the crash then goes as with a car that never
    /// slept; a placed car is awake; the runs are deterministic.
    #[test]
    fn dormant_car_sleeps_until_a_car_comes_near() {
        let run = |sleep: bool| {
            let (mut sim, a, b) = two_sedan_sim(7);
            // Settle on the ground, then b sleeps (with throttle held: it must not drive).
            for _ in 0..240 {
                sim.step_fixed();
            }
            if sleep {
                sim.set_dormant(b, true).unwrap();
                assert_eq!(sim.is_dormant(b), Some(true));
            }
            sim.set_input(
                b,
                VehicleInput {
                    throttle: 1.0,
                    ..Default::default()
                },
            )
            .unwrap();
            for _ in 0..240 {
                sim.step_fixed();
            }
            let b_still = sim.vehicle_states()[1].1.pos;
            let asleep_after_1s = sim.is_dormant(b);
            sim.set_input(b, VehicleInput::default()).unwrap();
            // a drives into b from 70 m back at 15 m/s.
            sim.place_vehicle(a, [0.0, -58.0, 0.0], 0.0, 15.0).unwrap();
            sim.set_input(
                a,
                VehicleInput {
                    throttle: 0.3,
                    ..Default::default()
                },
            )
            .unwrap();
            let mut woke_at_gap = None;
            for _ in 0..(240 * 6) {
                sim.step_fixed();
                if woke_at_gap.is_none() && sim.is_dormant(b) == Some(false) {
                    let st = sim.vehicle_states();
                    woke_at_gap = Some(st[1].1.pos[1] - st[0].1.pos[1]);
                }
            }
            let contacts = sim
                .drain_events()
                .iter()
                .filter(|e| matches!(e, SimEvent::Contact { b, .. } if *b != wp_contracts::WORLD_ENTITY))
                .count();
            (
                b_still,
                asleep_after_1s,
                woke_at_gap,
                contacts,
                sim.vehicle_states()[1].1.pos,
                sim.state_hash(),
            )
        };
        let (still, asleep, gap, contacts, b_end, h) = run(true);
        println!("asleep {asleep:?}, still at {still:?}, woke {gap:?} m ahead, {contacts} contacts, b ends at {b_end:?}");
        assert_eq!(
            asleep,
            Some(true),
            "full throttle does not wake a dormant car"
        );
        assert!(
            (still[1] - 12.0).abs() < 0.05,
            "a dormant car does not drive: y = {}",
            still[1]
        );
        let gap = gap.expect("the approaching car woke it");
        assert!(gap > 20.0, "woken well before contact ({gap} m)");
        assert!(
            contacts > 0 && b_end[1] > 12.5,
            "then it is hit and shoved like any car: y = {}",
            b_end[1]
        );
        assert_eq!(h, run(true).5, "dormant runs are deterministic");
        // A placed (reset) car is awake.
        let (mut sim, _, b) = two_sedan_sim(9);
        sim.set_dormant(b, true).unwrap();
        sim.place_vehicle(b, [0.0, 30.0, 0.0], 0.0, 0.0).unwrap();
        assert_eq!(sim.is_dormant(b), Some(false));
    }

    /// SR-3 light mode: an AI car far from the player drives the light model (moves under
    /// throttle, a right steer turns it right, stays on the ground), goes back to full physics
    /// when the player comes within `LIGHT_OFF_M` (moving on at its speed), and the runs are
    /// deterministic. Without permission it never goes light.
    #[test]
    fn light_mode_far_from_the_player() {
        let run = |allow: bool| {
            let (mut sim, player, ai) = two_sedan_sim(11);
            sim.set_vehicle_kind(ai, VehicleKind::AiRacer).unwrap();
            sim.place_vehicle(ai, [0.0, 600.0, 0.0], 0.0, 15.0).unwrap();
            if allow {
                sim.set_light_when_far(ai, true).unwrap();
            }
            let drive = VehicleInput {
                throttle: 0.4,
                steer: 0.05,
                ..Default::default()
            };
            sim.set_input(ai, drive).unwrap();
            for _ in 0..2 {
                sim.step_fixed();
            }
            let light_at_start = sim.is_light(ai);
            let p0 = sim.vehicle_states()[1].1.pos;
            for _ in 0..(240 * 4) {
                sim.step_fixed();
            }
            let st = sim.vehicle_states()[1].1.clone();
            // The player drives up: back to full physics within LIGHT_OFF_M.
            let near = [st.pos[0], st.pos[1] - 200.0];
            sim.place_vehicle(player, [near[0], near[1], 0.0], 0.0, 0.0)
                .unwrap();
            sim.step_fixed();
            let light_near = sim.is_light(ai);
            let v_after = sim.vehicle_states()[1].1.speed_mps;
            (
                light_at_start,
                p0,
                st,
                light_near,
                v_after,
                sim.state_hash(),
            )
        };
        let (light, p0, st, near, v_after, h) = run(true);
        println!("light {light:?}, moved {:?} -> {:?} at {:.1} m/s, near {near:?}, then {v_after:.1} m/s", p0, st.pos, st.speed_mps);
        assert_eq!(light, Some(true), "far from the player it drives light");
        assert!(
            st.pos[1] - p0[1] > 40.0,
            "it drove forward: {:?} -> {:?}",
            p0,
            st.pos
        );
        assert!(
            st.pos[0] > p0[0] + 1.0,
            "a right steer turned it right (east): {:?}",
            st.pos
        );
        assert!(st.pos[2].abs() < 1.0, "on the ground: z {}", st.pos[2]);
        assert!(
            st.speed_mps > 15.0,
            "throttle accelerates it: {} m/s",
            st.speed_mps
        );
        assert_eq!(near, Some(false), "full physics again near the player");
        assert!(
            (v_after - st.speed_mps).abs() < 2.0,
            "keeps its speed: {v_after} vs {}",
            st.speed_mps
        );
        assert_eq!(h, run(true).5, "light runs are deterministic");
        assert_eq!(run(false).0, Some(false), "never light without permission");
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
        let mode = wp_contracts::load_mode_table(data("modes/extreme.toml")).unwrap();
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
        let mode = wp_contracts::load_mode_table(data("modes/sim.toml")).unwrap();
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
        let mode = wp_contracts::load_mode_table(data("modes/sim.toml")).unwrap();
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
        // Approach the first prop whose side, 20 m out, is on a road (main's grid layout put
        // the first one's approach in a lot), at 12 m/s.
        let side_start = |p: &wp_world::StaticProp| {
            let (s, c) = wp_math::sin_cos(p.yaw);
            [p.pos[0] + c * 20.0, p.pos[1] + s * 20.0, 0.0]
        };
        let (k, target) = world
            .static_props()
            .enumerate()
            .find(|(_, p)| {
                world
                    .map()
                    .locate(side_start(p), None, 0.0, true, false)
                    .is_some()
            })
            .map(|(k, p)| (k, *p))
            .unwrap();
        sim.set_world(world);
        assert_eq!(sim.active_prop_count(), 0, "props start fixed");
        let start = side_start(&target);
        let (dx, dy) = (target.pos[0] - start[0], target.pos[1] - start[1]);
        let yaw = wp_math::atan2(-dx, dy);
        let id = sim.spawn_vehicle(&car, start, yaw).unwrap();
        sim.place_vehicle(id, start, yaw, 12.0).unwrap();
        let h = sim.props.bodies[k];
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
        let mode = wp_contracts::load_mode_table(data("modes/extreme.toml")).unwrap();
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
        let mode = wp_contracts::load_mode_table(data("modes/sim.toml")).unwrap();
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
    fn kinematic_police_do_not_roll_phantom_traffic_hazards() {
        // No police car model: every unit stays on wp_police's kinematic driver (low LOD).
        let car = wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap();
        let mode = wp_contracts::load_mode_table(data("modes/sim.toml")).unwrap();
        let mut sim = Sim::new(SimConfig {
            seed: 11,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 10.0,
        });
        sim.set_world(World::procedural(
            &wp_world::procgen::ProcGenConfig::small(),
            wp_world::WorldConfig::default(),
        ));
        let id = sim.spawn_vehicle(&car, [0.0, 0.0, 0.0], 0.0).unwrap();
        sim.police_mut().set_heat(id, 5.0);
        sim.set_input(
            id,
            VehicleInput {
                throttle: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
        for _ in 0..(60.0 / wp_contracts::PHYSICS_DT) as u32 {
            sim.step_fixed();
        }
        assert!(
            sim.police().units().iter().any(|u| u.entity.is_none()),
            "no kinematic police units spawned"
        );
        let stats = sim.police().stats();
        assert_eq!(
            (stats.near_misses, stats.traffic_collisions),
            (0, 0),
            "kinematic units hit statistical stand-in traffic"
        );
    }

    #[test]
    fn traffic_near_the_player_becomes_physical() {
        let run = || {
            let car = wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap();
            let mode = wp_contracts::load_mode_table(data("modes/sim.toml")).unwrap();
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
        let mode = wp_contracts::load_mode_table(data("modes/sim.toml")).unwrap();
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
        let mode = wp_contracts::load_mode_table(data("modes/sim.toml")).unwrap();
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
#[cfg(test)]
mod cost_probe {
    use super::*;
    /// Probe: ms per 60 Hz frame for 13 cars on the flat slab, idle / driving / dormant, with and
    /// without damage. `cargo test --release -p wp_sim --lib cost_probe -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn cost_probe() {
        let data = |p: &str| format!("{}/../../data/{p}", env!("CARGO_MANIFEST_DIR"));
        let car = wp_contracts::load_car(data("cars/hakone_gt.json")).unwrap();
        let mode = wp_contracts::load_mode_table(data("modes/sim.toml")).unwrap();
        for (label, drive, dormant, damage) in [
            ("idle", false, false, true),
            ("driving", true, false, true),
            ("driving, no damage", true, false, false),
            ("dormant", false, true, true),
        ] {
            let mut sim = Sim::new(SimConfig {
                seed: 1,
                mode: mode.clone(),
                time_of_day: TimeOfDay::from_hours(12.0),
                time_ratio: 10.0,
            });
            let mut ids = Vec::new();
            for k in 0..13 {
                ids.push(
                    sim.spawn_vehicle(&car, [k as f32 * 30.0, 0.0, 0.0], 0.0)
                        .unwrap(),
                );
            }
            let mut sys = sim.systems();
            sys.damage = damage;
            sim.set_systems(sys);
            for _ in 0..240 {
                sim.step_fixed();
            }
            for id in &ids {
                if drive {
                    sim.set_input(
                        *id,
                        VehicleInput {
                            throttle: 0.3,
                            steer: 0.2,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                }
                if dormant {
                    sim.set_dormant(*id, true).unwrap();
                }
            }
            let t0 = std::time::Instant::now();
            let n = 240 * 10;
            for _ in 0..n {
                sim.step_fixed();
            }
            let ms = t0.elapsed().as_secs_f64() * 1000.0 / (n as f64 / 4.0);
            println!(
                "13 cars {label}: {ms:.3} ms per 60 Hz frame ({:.3} per car)",
                ms / 13.0
            );
        }
    }
}
