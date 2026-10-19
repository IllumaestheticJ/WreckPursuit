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
//!   the sim's police system;
//! - **captain**: typed `CaptainCommand`s go to `wp_police` through `Sim::captain_command`;
//! - **v0.3.0 replication**: kinematic traffic agents (`TrafficAgentSnapshot`) and an optional
//!   server-side `wp_weather::WeatherSim` (snapshots + `SimEvent::WeatherChanged`).

use std::collections::BTreeMap;

use wp_contracts::{
    AssistProfile, CaptainCommand, CaptainError, ContactImpulse, DamageMode, DamageSim, EntityId,
    LoadedCar, PlayerId, SimEvent, TimeOfDay, TrafficAgentSnapshot, UnitId, VehicleInput,
    VehicleState, WeatherKind, WeatherSnapshot, WorldQuery, PHYSICS_DT,
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
    /// A racer's bounty from `wp_police` (cash value of its heat history). Default: 0.
    fn bounty(&self, _id: EntityId) -> u32 {
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
    /// True when a lattice snapshot taken now reproduces the vehicle's damage model exactly:
    /// it was never hit, or it has settled since (no impact in progress). Otherwise the
    /// server marks the snapshot provisional and sends a final one once it settles.
    /// Default: true.
    fn damage_settled(&self, _id: EntityId) -> bool {
        true
    }
    /// Full internal damage state of a damaged vehicle for lattice resyncs
    /// ([`crate::lattice::ReplicaDamage::save_state`] bytes, NET-N5). Default: none.
    fn damage_state(&self, _id: EntityId) -> Option<Vec<u8>> {
        None
    }
    /// Tell the sim who drives a vehicle the net layer spawned (racer seat or police seat)
    /// and which player owns it.
    /// Default: ignored.
    fn set_owned_kind(&mut self, _id: EntityId, _kind: EntityKind, _player: PlayerId) {}
    /// Kinematic (not physical) traffic agents, ascending id. Physical agents are vehicles.
    /// Default: none.
    fn traffic_agents(&self) -> Vec<TrafficAgentSnapshot> {
        Vec::new()
    }
    /// Current weather for clients (W-5). Default: none (clients assume clear).
    fn weather(&self) -> Option<WeatherSnapshot> {
        None
    }
    /// Execute a human captain's command (validated by `wp_police`). Default: unavailable.
    fn captain_command(&mut self, _cmd: CaptainCommand) -> Result<(), CaptainError> {
        Err(CaptainError::Unavailable)
    }
    /// Seat or unseat a human captain. Default: ignored.
    fn set_human_captain(&mut self, _seated: bool) {}
    /// `wp_police` unit embodied by an AI police vehicle. Default: none.
    fn police_unit(&self, _id: EntityId) -> Option<UnitId> {
        None
    }
}

/// [`NetSim`] over the real `wp_sim::Sim`.
pub struct WpSim {
    sim: Sim,
    cars: BTreeMap<String, LoadedCar>,
    mode: DamageMode,
    /// Damage steps run per live vehicle (see [`NetSim::damage_steps`]).
    damage_steps: BTreeMap<EntityId, u64>,
    /// Server-side weather and the clock ratio it runs at (see [`WpSim::set_weather`]).
    weather: Option<(wp_weather::WeatherSim, f32)>,
    /// `WeatherChanged` events not yet drained.
    weather_events: Vec<SimEvent>,
    /// Vehicles that took a damage-relevant contact since they spawned.
    hit: std::collections::BTreeSet<EntityId>,
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
            weather: None,
            weather_events: Vec::new(),
            hit: Default::default(),
        }
    }

    /// Run weather on the server: stepped with the sim (every net tick), replicated to clients
    /// as `ServerMsg::Weather` and `SimEvent::WeatherChanged`. `time_ratio` is the world
    /// clock's in-game seconds per sim second (`SimConfig::time_ratio`). Grip on the server is
    /// not affected yet (`wp_vehicle` does not read `weather_at`; machine A's step).
    pub fn set_weather(&mut self, weather: wp_weather::WeatherSim, time_ratio: f32) {
        self.weather = Some((weather, time_ratio));
    }

    /// [`WpSim::set_weather`] with [`city_weather`] over this sim's world, starting in `start`
    /// at `intensity` with the season schedule running (seeded from `seed`).
    pub fn enable_weather(
        &mut self,
        seed: u64,
        start: WeatherKind,
        intensity: f32,
        time_ratio: f32,
    ) {
        let cfg = wp_weather::WeatherConfig {
            seed,
            start: wp_weather::Regime::new(start, intensity),
            ..Default::default()
        };
        let weather = city_weather(self.sim.world(), cfg);
        self.set_weather(weather, time_ratio);
    }

    /// The server-side weather, if any.
    pub fn weather_sim(&self) -> Option<&wp_weather::WeatherSim> {
        self.weather.as_ref().map(|(w, _)| w)
    }

    /// The server-side weather, mutably (admin tools, tests forcing a regime).
    pub fn weather_sim_mut(&mut self) -> Option<&mut wp_weather::WeatherSim> {
        self.weather.as_mut().map(|(w, _)| w)
    }

    fn step_weather(&mut self) {
        let Some((weather, ratio)) = self.weather.as_mut() else {
            return;
        };
        // Once per net tick is plenty (internal updates run every `update_interval_s`).
        let every = crate::TICKS_PER_NET_TICK;
        if !self.sim.tick().is_multiple_of(every) {
            return;
        }
        let time = self.sim.world().time_of_day();
        let moving: Vec<[f32; 3]> = self
            .sim
            .vehicle_states()
            .iter()
            .map(|(_, s)| s.pos)
            .collect();
        weather.step(PHYSICS_DT * every as f32, time, *ratio, &moving);
        self.weather_events
            .extend(weather.drain_changes().iter().map(|c| c.to_sim_event()));
    }

    /// A city instance: `Sim` with the world, AI police cars and physical traffic installed
    /// (`wp_sim` builds `wp_police` on the city's roads in `set_world`).
    pub fn with_city(
        config: SimConfig,
        cars: BTreeMap<String, LoadedCar>,
        city: CitySetup,
    ) -> Self {
        let mut sim = Sim::new(config);
        sim.set_world(city.world);
        if let Some(car) = city.police_car {
            sim.set_police_car(car);
        }
        if let Some(car) = city.traffic_car {
            sim.set_traffic_car(car);
        }
        Self::from_sim(sim, cars)
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

/// What a city instance contains besides the road network (see [`WpSim::with_city`]).
pub struct CitySetup {
    /// The city (road network, traffic, clock), e.g. `wp_world::World::load`.
    pub world: wp_world::World,
    /// Car for AI police units (`None` = police stay kinematic, no police cars).
    pub police_car: Option<LoadedCar>,
    /// Car for physical near-player traffic (`None` = traffic stays kinematic).
    pub traffic_car: Option<LoadedCar>,
}

/// `wp_world` settings for a server city: traffic on, clock from the instance, no parked
/// cars (thousands of extra physics bodies; they add little to multiplayer and cost a lot).
pub fn server_world_config(config: &SimConfig) -> wp_world::WorldConfig {
    wp_world::WorldConfig {
        seed: config.seed,
        start_time: config.time_of_day,
        time_ratio: config.time_ratio,
        traffic: wp_world::TrafficConfig {
            enabled: true,
            detect_collisions: true,
            ..Default::default()
        },
        parked_cars: false,
    }
}

/// Server weather over a city: an 8 m wetness grid covering the road network plus 50 m, with
/// `wp_weather`'s embedded tables (the layout `wp_weather`'s `weather_in_sim` example uses).
/// Pass it to [`WpSim::set_weather`].
pub fn city_weather(
    world: &wp_world::World,
    cfg: wp_weather::WeatherConfig,
) -> wp_weather::WeatherSim {
    let grid =
        wp_weather::GridSpec::around(world.network().nodes.iter().map(|n| &n.pos), 50.0, 8.0);
    wp_weather::WeatherSim::from_world(wp_weather::WeatherData::embedded(), cfg, grid, world)
}

/// Up to `n` spawn poses on the city's through lanes: one per lane, from lanes at least 60 m
/// long, 20 m after the lane start, facing along the lane. Deterministic (lane order).
pub fn lane_spawn_points(world: &wp_world::World, n: usize) -> Vec<([f32; 3], f32)> {
    let mut out = Vec::new();
    for lane in &world.network().lanes {
        if out.len() >= n {
            break;
        }
        if lane.kind != wp_world::network::LaneKind::Through || lane.polyline.len() < 2 {
            continue;
        }
        let (a, b) = (lane.polyline[0], lane.polyline[1]);
        let d = [b[0] - a[0], b[1] - a[1]];
        let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
        if len < 60.0 {
            continue;
        }
        let t = 20.0 / len;
        let pos = [a[0] + d[0] * t, a[1] + d[1] * t, 0.0];
        out.push((pos, wp_math::atan2(-d[0], d[1])));
    }
    out
}

/// The static ground of a city with terrain for client prediction rigs
/// ([`crate::client::ClientConfig::city_ground`]): `wp_sim`'s parked safety slab plus
/// `wp_sim::city_ground_colliders`, identical to what the server's sim drives on. `None` for
/// flat cities (the rig's slab already matches).
pub fn city_ground(world: &wp_world::World) -> Option<crate::predict::CityGround> {
    let (colliders, _) = wp_sim::city_ground_colliders(world.map())?;
    let mut all = Vec::with_capacity(colliders.len() + 1);
    all.push(wp_sim::parked_slab_collider());
    all.extend(colliders);
    Some(std::sync::Arc::new(all))
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
        if self.sim.has_terrain() {
            // Cities with terrain: settle on the road / terrain under the spawn point (the
            // spawn height is a hint for grade separation, 0 = lowest surface).
            let near = (pos[2] != 0.0).then_some(pos[2]);
            self.sim
                .place_on_ground(id, [pos[0], pos[1]], yaw, 0.0, near)
                .map_err(|e| e.to_string())?;
        }
        self.damage_steps.insert(id, 0);
        Ok(id)
    }

    fn despawn(&mut self, id: EntityId) {
        let _ = self.sim.despawn_vehicle(id);
        self.damage_steps.remove(&id);
        self.hit.remove(&id);
    }

    fn set_input(&mut self, id: EntityId, input: VehicleInput) {
        let _ = self.sim.set_input(id, input);
    }

    fn set_assists(&mut self, id: EntityId, assists: AssistProfile) {
        let _ = self.sim.set_assists(id, assists);
    }

    fn step(&mut self) {
        self.sim.step_fixed();
        self.step_weather();
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
        let mut events = self.sim.drain_events();
        events.append(&mut self.weather_events);
        events
    }

    fn damage_settled(&self, id: EntityId) -> bool {
        !self.hit.contains(&id) || self.slot(id).is_none_or(|v| !v.damage.is_awake())
    }

    fn damage_state(&self, id: EntityId) -> Option<Vec<u8>> {
        crate::lattice::ReplicaDamage::save_state(&self.slot(id)?.damage)
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
            self.hit.insert(id);
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

    fn bounty(&self, id: EntityId) -> u32 {
        let police = self.sim.police();
        police.racer(id).map_or(0, |r| {
            r.bounty.bounty(&police.config().heat).max(0.0) as u32
        })
    }

    fn wanted_level(&self, id: EntityId) -> u8 {
        self.sim.police().wanted_level(id).number()
    }

    fn set_owned_kind(&mut self, id: EntityId, kind: EntityKind, player: PlayerId) {
        // Player police seats are `wp_police` player units (they spot racers, start
        // pursuits, count for captures, take captain orders) and `VehicleKind::Police`, so
        // AI police never chase them. That kind also drops them from the traffic spawn ring
        // (NET-N9 in docs/integration/net.md).
        if kind.is_police() {
            let _ = self.sim.bind_player_police(id, player);
        } else {
            let _ = self.sim.unbind_player_police(id);
        }
    }

    fn traffic_agents(&self) -> Vec<TrafficAgentSnapshot> {
        self.sim
            .world()
            .agents()
            .filter(|a| a.lod == wp_world::traffic::Lod::Kinematic)
            .map(|a| TrafficAgentSnapshot::from(&a))
            .collect()
    }

    fn weather(&self) -> Option<WeatherSnapshot> {
        let snap = self.weather_sim()?.snapshot();
        Some(WeatherSnapshot {
            kind: WeatherKind::from_code(snap.kind)?,
            intensity: snap.intensity,
            prev_kind: WeatherKind::from_code(snap.prev_kind)?,
            prev_intensity: snap.prev_intensity,
            blend: snap.blend,
            wind_heading_rad: snap.wind_heading_rad,
            water_q8: snap.water_q8,
        })
    }

    fn captain_command(&mut self, cmd: CaptainCommand) -> Result<(), CaptainError> {
        self.sim.captain_command(cmd)
    }

    fn set_human_captain(&mut self, seated: bool) {
        self.sim.set_human_captain(seated);
    }

    fn police_unit(&self, id: EntityId) -> Option<UnitId> {
        self.slot(id).and_then(|v| v.police_unit)
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

    /// A player police seat is a `wp_police` player unit: it spots a wanted racer right in
    /// front of it and the pursuit starts with *its* unit; swapping in a new police system
    /// keeps the seat (re-registered, not despawned); despawning removes the unit.
    #[test]
    fn player_cop_seat_starts_a_pursuit_and_leaves_cleanly() {
        let (cars, modes) = load_data(&repo_data_dir()).unwrap();
        let config = SimConfig {
            seed: 5,
            mode: modes[&DamageMode::Arcade].clone(),
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 1.0,
        };
        let pg = wp_world::procgen::ProcGenConfig {
            nx: 4,
            ny: 4,
            spacing_m: 110.0,
            seed: 1,
            ..Default::default()
        };
        let mut wcfg = server_world_config(&config);
        wcfg.traffic.enabled = false;
        let world = wp_world::World::procedural(&pg, wcfg);
        let city = CitySetup {
            world,
            police_car: None,
            traffic_car: None,
        };
        let mut net = WpSim::with_city(config, cars, city);
        let racer = net
            .spawn("debug_coupe", [53.25, 200.0, 0.0], std::f32::consts::PI)
            .unwrap();
        let cop = net
            .spawn("debug_sedan", [53.25, 225.0, 0.0], std::f32::consts::PI)
            .unwrap();
        net.set_owned_kind(racer, EntityKind::Racer, PlayerId(1));
        net.set_owned_kind(cop, EntityKind::PoliceChase, PlayerId(2));
        let unit = net.police_unit(cop).expect("cop seat is a police unit");
        assert!(net.sim().police().units().iter().any(|u| u.id == unit));
        // A new police system (tests, admin tools): the seat is registered again.
        let fresh = wp_police::PoliceSystem::for_world(
            wp_police::PoliceConfig::builtin(),
            net.sim().world(),
            wp_police::PoliceSetup::default(),
        );
        net.sim_mut().set_police(fresh);
        net.sim_mut().police_mut().set_heat(racer, 55.0);
        let mut started = None;
        for _ in 0..240 * 6 {
            net.step();
            for e in net.drain_events() {
                if let SimEvent::Police(wp_contracts::PoliceEvent::PursuitStarted {
                    racer: r,
                    unit,
                }) = e
                {
                    started.get_or_insert((r, unit));
                }
            }
        }
        assert!(net.vehicles().iter().any(|(e, _)| *e == cop), "seat kept");
        let unit = net.police_unit(cop).expect("re-registered");
        assert_eq!(started, Some((racer, Some(unit))), "the player cop spotted");
        net.despawn(cop);
        assert!(!net.sim().police().units().iter().any(|u| u.id == unit));
    }
}
