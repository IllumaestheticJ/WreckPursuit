//! The scenario runner: builds a [`Sim`] from a [`Scenario`], drives it tick by tick, collects
//! metrics, events and hashes, and evaluates expectations.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;

use serde::Serialize;
use wp_contracts::{
    Component, EntityId, ServerToClient, SimEvent, TimeOfDay, VehicleInput, VehicleState,
    GRAVITY_MPS2, PHYSICS_DT, PHYSICS_TICKS_PER_NET_TICK,
};
use wp_sim::{Sim, SimConfig, Systems};

use crate::data::{load_car, load_mode, load_world, DataSource};
use crate::replay::{Checkpoint, EntityCheckpoint, InputEvent, OpEvent, OpKind, Replay};
use crate::scenario::{tick_of, CitySpec, Driver, Expect, Scenario};
use crate::RunError;

/// `wp_police` payload kind treated as "pursuit started" by [`Expect::PursuitStartedBefore`]
/// (`wp_police::events::kind::PURSUIT_STARTED`; the full list is in that module).
pub const PURSUIT_STARTED_KIND: u16 = wp_police::events::kind::PURSUIT_STARTED;

/// Systems to run: the [`Systems`] inside [`Sim`] plus the harness-side net round trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// Systems stepped by the sim.
    pub sim: Systems,
    /// Encode → decode a `ServerToClient::Snapshot` through `wp_net` every net tick and check
    /// it round-trips.
    pub net: bool,
}

impl Selection {
    /// Everything on.
    pub const ALL: Self = Self {
        sim: Systems::ALL,
        net: true,
    };

    /// Parse `"vehicle,damage,world,police,net"` (or `"all"`).
    pub fn parse(list: &str) -> Result<Self, String> {
        if list.trim() == "all" {
            return Ok(Self::ALL);
        }
        let mut net = false;
        let rest: Vec<&str> = list
            .split(',')
            .map(str::trim)
            .filter(|n| {
                if *n == "net" {
                    net = true;
                    false
                } else {
                    true
                }
            })
            .collect();
        Ok(Self {
            sim: Systems::parse(&rest.join(","))?,
            net,
        })
    }
}

impl std::fmt::Display for Selection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.sim == Systems::NONE, self.net) {
            (true, true) => f.write_str("net"),
            (_, false) => write!(f, "{}", self.sim),
            (false, true) => write!(f, "{},net", self.sim),
        }
    }
}

/// How to run a scenario.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// Override the scenario's `systems`.
    pub systems: Option<Selection>,
    /// Build `metrics.csv`, `events.jsonl` and the replay (costs time; off for fuzz/bench).
    pub record: bool,
    /// Replay mode: apply exactly these input events instead of the scenario's drivers.
    pub input_track: Option<Vec<InputEvent>>,
    /// Treat warnings (unsupported features, unapplied overrides) as failures.
    pub strict: bool,
    /// Replay mode: apply these non-input ops (reset, assists, mode switch) before the inputs of
    /// the same tick. Recorded into the replay when `record` is on.
    pub op_track: Option<Vec<OpEvent>>,
}

/// A simulation built from a scenario's setup, before the first tick. Shared by [`run`] and the
/// browser testbed (`wp_wasm`) so both construct identical sims.
pub struct BuiltSim {
    /// The simulation (systems selected, barriers added, entities spawned with their assists).
    pub sim: Sim,
    /// Entity ids in scenario order.
    pub ids: Vec<EntityId>,
    /// Loaded cars by car id.
    pub cars: BTreeMap<String, wp_contracts::LoadedCar>,
    /// Systems that will run.
    pub selection: Selection,
    /// Unsupported features / unapplied overrides.
    pub warnings: Vec<String>,
    /// Whether a city (`wp_world`) was loaded instead of the flat world.
    pub has_city: bool,
}

/// The `wp_world` settings a scenario implies (seed, clock, traffic density override).
pub fn world_config(sc: &Scenario) -> wp_world::WorldConfig {
    let density = sc.traffic_density.unwrap_or(1.0);
    wp_world::WorldConfig {
        seed: sc.seed,
        start_time: TimeOfDay::from_hours(sc.time_of_day_h),
        time_ratio: sc.time_ratio,
        traffic: wp_world::TrafficConfig {
            enabled: density > 0.0,
            density_scale: density,
            detect_collisions: true,
            ..Default::default()
        },
        parked_cars: true,
    }
}

/// Build the [`Sim`] for a scenario: systems, barriers, every entity (spawn pose, start speed,
/// assists), then the city (`None` = flat world) populated around the spawned cars. Shared by
/// [`run`] and the browser testbed (`wp_wasm`) so both construct identical sims.
pub fn build_sim(
    sc: &Scenario,
    data: &dyn DataSource,
    opts: &RunOptions,
) -> Result<BuiltSim, RunError> {
    let selection = match (&opts.systems, &sc.systems) {
        (Some(s), _) => *s,
        (None, Some(list)) => {
            Selection::parse(list).map_err(|e| RunError::Scenario(sc.name.clone(), e))?
        }
        (None, None) => Selection::ALL,
    };
    let mut warnings = Vec::new();
    if sc.city == CitySpec::Flat && sc.traffic_density.is_some() {
        warnings.push("traffic_density ignored: the flat world has no roads or traffic".into());
    }
    if let Some(p) = sc.police_presence {
        warnings.push(format!(
            "police_presence override {p} not applied: wp_police has no presence API yet"
        ));
    }
    for e in &sc.entities {
        match &e.driver {
            Driver::FollowRoute { .. } | Driver::AiDriver { .. } => warnings.push(format!(
                "entity {:?}: driver {:?} not available yet; using its input keyframes",
                e.id, e.driver
            )),
            Driver::Scripted | Driver::Cruise { .. } => {}
        }
    }

    let mode = load_mode(data, sc.mode)?;
    let mut sim = Sim::new(SimConfig {
        seed: sc.seed,
        mode,
        time_of_day: TimeOfDay::from_hours(sc.time_of_day_h),
        time_ratio: sc.time_ratio,
    });
    sim.set_systems(selection.sim);
    for b in &sc.barriers {
        sim.add_static_box(b.pos, b.size, b.yaw);
    }
    let mut ids = Vec::new();
    let mut cars: BTreeMap<String, wp_contracts::LoadedCar> = BTreeMap::new();
    for spec in &sc.entities {
        if !cars.contains_key(&spec.car) {
            cars.insert(spec.car.clone(), load_car(data, &spec.car)?);
        }
        let id = sim.spawn_vehicle(&cars[&spec.car], spec.pos, spec.yaw)?;
        if spec.speed_mps != 0.0 {
            sim.place_vehicle(id, spec.pos, spec.yaw, spec.speed_mps)?;
        }
        sim.set_assists(id, spec.assists.unwrap_or(sc.assists).profile())?;
        ids.push(id);
    }
    let has_city = match load_world(data, &sc.city, world_config(sc))? {
        Some(mut world) => {
            // Fill the area around the spawned cars before the first tick (the sim refreshes
            // the tracked set every tick from then on).
            let tracked: Vec<wp_world::TrackedVehicle> = sim
                .vehicle_states()
                .iter()
                .map(|(id, s)| wp_world::TrackedVehicle::from_state(*id, s, false, true))
                .collect();
            world.set_tracked_vehicles(&tracked);
            world.populate();
            sim.set_world(world);
            true
        }
        None => false,
    };
    Ok(BuiltSim {
        sim,
        ids,
        cars,
        selection,
        warnings,
        has_city,
    })
}

/// Apply one replay op to a built sim (`ids` in scenario order).
pub fn apply_op(
    sim: &mut Sim,
    ids: &[EntityId],
    data: &dyn DataSource,
    ev: &OpEvent,
) -> Result<(), RunError> {
    let id = *ids.get(ev.entity as usize).ok_or_else(|| {
        RunError::Replay(format!("op for entity index {} out of range", ev.entity))
    })?;
    match ev.op {
        OpKind::Reset => sim.reset_vehicle(id)?,
        OpKind::Assists(p) => sim.set_assists(id, p)?,
        OpKind::Mode(m) => sim.set_mode_table(load_mode(data, m)?),
    }
    Ok(())
}

/// Read-only view handed to [`run_with_hook`] after every tick.
pub struct TickView<'a> {
    /// The simulation, after the tick.
    pub sim: &'a Sim,
    /// Events emitted this tick.
    pub events: &'a [SimEvent],
    /// Scenario-local entity names, in spawn order (same order as `sim.vehicles()`).
    pub names: &'a [String],
}

/// Per-entity results.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EntitySummary {
    /// Scenario-local name.
    pub id: String,
    /// Sim entity id.
    pub entity: u32,
    /// Car id.
    pub car: String,
    /// Final forward speed, m/s.
    pub final_speed_mps: f32,
    /// Maximum forward speed, m/s.
    pub max_speed_mps: f32,
    /// Horizontal straight-line distance from spawn, m.
    pub distance_m: f32,
    /// Horizontal path length driven, m.
    pub path_length_m: f32,
    /// Final health 0..=1.
    pub final_health: f32,
    /// Maximum |lateral acceleration|, g (over a 1/60 s window).
    pub max_lateral_g: f32,
    /// Maximum |longitudinal acceleration|, g (over a 1/60 s window).
    pub max_long_g: f32,
    /// Maximum |yaw rate|, rad/s.
    pub max_yaw_rate_rps: f32,
    /// Maximum lattice node displacement from rest, m.
    pub crush_depth_m: f32,
    /// Distinct components failed.
    pub components_failed: u32,
    /// `PartDetached` events.
    pub parts_detached: u32,
    /// `WheelDetached` events.
    pub wheels_detached: u32,
    /// A `TotalLoss` event was emitted.
    pub total_loss: bool,
    /// Lowest chassis z reached, m.
    pub min_z_m: f32,
}

impl EntitySummary {
    /// Look up a metric by name (the names `Expect::Metric` accepts).
    pub fn metric(&self, name: &str) -> Option<f32> {
        Some(match name {
            "final_speed_mps" => self.final_speed_mps,
            "max_speed_mps" => self.max_speed_mps,
            "distance_m" => self.distance_m,
            "path_length_m" => self.path_length_m,
            "final_health" => self.final_health,
            "max_lateral_g" => self.max_lateral_g,
            "max_long_g" => self.max_long_g,
            "max_yaw_rate_rps" => self.max_yaw_rate_rps,
            "crush_depth_m" => self.crush_depth_m,
            "components_failed" => self.components_failed as f32,
            "parts_detached" => self.parts_detached as f32,
            "wheels_detached" => self.wheels_detached as f32,
            "total_loss" => self.total_loss as u8 as f32,
            "min_z_m" => self.min_z_m,
            _ => return None,
        })
    }

    /// Every metric name [`EntitySummary::metric`] accepts.
    pub const METRICS: [&'static str; 14] = [
        "final_speed_mps",
        "max_speed_mps",
        "distance_m",
        "path_length_m",
        "final_health",
        "max_lateral_g",
        "max_long_g",
        "max_yaw_rate_rps",
        "crush_depth_m",
        "components_failed",
        "parts_detached",
        "wheels_detached",
        "total_loss",
        "min_z_m",
    ];
}

/// Outcome of one expectation.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExpectResult {
    /// The expectation, as RON.
    pub expect: String,
    /// Measured value, human-readable.
    pub actual: String,
    /// Whether it held.
    pub passed: bool,
}

/// Harness-side net statistics.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct NetStats {
    /// Snapshots round-tripped.
    pub snapshots: u64,
    /// Total encoded bytes.
    pub bytes: u64,
}

/// World and traffic results.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct WorldSummary {
    /// City (`flat`, `procedural_<nx>x<ny>` or the file path).
    pub city: String,
    /// Traffic agents at the end.
    pub traffic_agents_final: u32,
    /// Fewest agents at any tick after the first [`WORLD_WARMUP_S`] s (the final count if the
    /// run is shorter).
    pub traffic_agents_min: u32,
    /// Fraction of agents moving (> 0.5 m/s) at the end.
    pub traffic_moving_frac_final: f32,
    /// Lowest moving fraction over 1 Hz samples after the warm-up.
    pub traffic_moving_frac_min: f32,
    /// New civilian–civilian overlap pairs (`TrafficStats::civilian_collisions`).
    pub civilian_collisions: u64,
    /// Agents spawned.
    pub traffic_spawned: u64,
    /// Agents recycled because they were stuck.
    pub traffic_stuck_recycled: u64,
    /// City props turned into physics bodies (parked cars, cones).
    pub props: u32,
    /// Static boxes from `barriers`.
    pub static_boxes: u32,
}

impl WorldSummary {
    /// Look up a metric by name (the names `Expect::WorldMetric` accepts).
    pub fn metric(&self, name: &str) -> Option<f32> {
        Some(match name {
            "traffic_agents_final" => self.traffic_agents_final as f32,
            "traffic_agents_min" => self.traffic_agents_min as f32,
            "traffic_moving_frac_final" => self.traffic_moving_frac_final,
            "traffic_moving_frac_min" => self.traffic_moving_frac_min,
            "civilian_collisions" => self.civilian_collisions as f32,
            "traffic_spawned" => self.traffic_spawned as f32,
            "traffic_stuck_recycled" => self.traffic_stuck_recycled as f32,
            "props" => self.props as f32,
            "static_boxes" => self.static_boxes as f32,
            _ => return None,
        })
    }

    /// Every metric name [`WorldSummary::metric`] accepts.
    pub const METRICS: [&'static str; 9] = [
        "traffic_agents_final",
        "traffic_agents_min",
        "traffic_moving_frac_final",
        "traffic_moving_frac_min",
        "civilian_collisions",
        "traffic_spawned",
        "traffic_stuck_recycled",
        "props",
        "static_boxes",
    ];
}

/// Seconds at the start of a run that the `*_min` traffic metrics ignore (spawn-in).
pub const WORLD_WARMUP_S: f32 = 10.0;

#[derive(Default)]
struct WorldTracker {
    agents_min: Option<u32>,
    moving_min: Option<f32>,
}

fn moving_frac(world: &wp_world::World) -> f32 {
    let n = world.agent_count();
    if n == 0 {
        return 0.0;
    }
    world.agents().filter(|a| a.speed_mps > 0.5).count() as f32 / n as f32
}

impl WorldTracker {
    fn update(&mut self, world: &wp_world::World, tick: u64) {
        if tick < tick_of(WORLD_WARMUP_S) {
            return;
        }
        let n = world.agent_count() as u32;
        self.agents_min = Some(self.agents_min.map_or(n, |m| m.min(n)));
        if tick.is_multiple_of(240) {
            let f = moving_frac(world);
            self.moving_min = Some(self.moving_min.map_or(f, |m| m.min(f)));
        }
    }

    fn finish(
        &self,
        world: &wp_world::World,
        city: &CitySpec,
        props: usize,
        boxes: usize,
    ) -> WorldSummary {
        let stats = world.traffic_stats();
        let n = world.agent_count() as u32;
        let frac = moving_frac(world);
        WorldSummary {
            city: match city {
                CitySpec::Flat => "flat".into(),
                CitySpec::Procedural {
                    blocks_x, blocks_y, ..
                } => format!("procedural_{blocks_x}x{blocks_y}"),
                CitySpec::File(f) => f.clone(),
            },
            traffic_agents_final: n,
            traffic_agents_min: self.agents_min.unwrap_or(n),
            traffic_moving_frac_final: frac,
            traffic_moving_frac_min: self.moving_min.unwrap_or(frac),
            civilian_collisions: stats.civilian_collisions,
            traffic_spawned: stats.spawned,
            traffic_stuck_recycled: stats.stuck_recycled,
            props: props as u32,
            static_boxes: boxes as u32,
        }
    }
}

/// Result of one run (`summary.json`).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RunSummary {
    /// Scenario name.
    pub name: String,
    /// Contract version the run was built against.
    pub contract_version: String,
    /// Hash spec version ([`wp_sim::HASH_SPEC_VERSION`]).
    pub hash_spec: u32,
    /// Systems run.
    pub systems: String,
    /// Scenario seed.
    pub seed: u64,
    /// Physics ticks run.
    pub ticks: u64,
    /// Final state hash, 16 hex digits.
    pub final_hash: String,
    /// Per-entity results, in spawn order.
    pub entities: Vec<EntitySummary>,
    /// Events emitted.
    pub events_total: u64,
    /// Most events in a single tick.
    pub max_events_per_tick: u64,
    /// Net round-trip stats (zero when `net` is off).
    pub net: NetStats,
    /// World and traffic results (zeros on the flat world).
    pub world: WorldSummary,
    /// Things the run could not do as asked (unsupported city, drivers, overrides).
    pub warnings: Vec<String>,
    /// `expect` results.
    pub expectations: Vec<ExpectResult>,
    /// `expect_pending` results (never fail the run).
    pub pending: Vec<ExpectResult>,
    /// Failure messages. Empty = pass.
    pub failures: Vec<String>,
}

impl RunSummary {
    /// Whether every expectation held.
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }

    /// Look up an entity's summary by scenario-local name.
    pub fn entity(&self, id: &str) -> Option<&EntitySummary> {
        self.entities.iter().find(|e| e.id == id)
    }
}

/// Everything a run produces.
#[derive(Debug, Clone)]
pub struct RunOutput {
    /// Summary and expectation results.
    pub summary: RunSummary,
    /// `(tick, state hash)` every `hash_every` ticks and at the end.
    pub hashes: Vec<(u64, u64)>,
    /// `metrics.csv` (empty unless recording).
    pub metrics_csv: String,
    /// `events.jsonl` (empty unless recording).
    pub events_jsonl: String,
    /// The replay (empty input/hash tracks unless recording).
    pub replay: Replay,
}

impl RunOutput {
    /// `hashes.txt` text: a `#` header line, then `<tick> <hash hex>` lines.
    pub fn hashes_txt(&self) -> String {
        hashes_txt(&self.summary, &self.hashes)
    }
}

/// Format a `hashes.txt` file.
pub fn hashes_txt(summary: &RunSummary, hashes: &[(u64, u64)]) -> String {
    let mut s = format!(
        "# simcli hashes scenario={} systems={} hash_spec={} contract={}\n",
        summary.name, summary.systems, summary.hash_spec, summary.contract_version
    );
    for (tick, h) in hashes {
        let _ = writeln!(s, "{tick} {h:016x}");
    }
    s
}

/// Name of a `SimEvent` variant.
pub fn event_name(e: &SimEvent) -> &'static str {
    match e {
        SimEvent::Contact { .. } => "Contact",
        SimEvent::LatticeDeformed { .. } => "LatticeDeformed",
        SimEvent::PartDetached { .. } => "PartDetached",
        SimEvent::ComponentDamaged { .. } => "ComponentDamaged",
        SimEvent::ComponentFailed { .. } => "ComponentFailed",
        SimEvent::WheelDetached { .. } => "WheelDetached",
        SimEvent::TotalLoss { .. } => "TotalLoss",
        SimEvent::TrafficViolation { .. } => "TrafficViolation",
        SimEvent::GlassShattered { .. } => "GlassShattered",
        SimEvent::RegionDamaged { .. } => "RegionDamaged",
        SimEvent::PoliceEvent { .. } => "PoliceEvent",
        SimEvent::EconomyEvent { .. } => "EconomyEvent",
    }
}

/// Entities an event names (both sides of a contact).
pub fn event_entities(e: &SimEvent) -> Vec<EntityId> {
    match e {
        SimEvent::Contact { a, b, .. } => vec![*a, *b],
        SimEvent::LatticeDeformed { vehicle, .. }
        | SimEvent::PartDetached { vehicle, .. }
        | SimEvent::ComponentDamaged { vehicle, .. }
        | SimEvent::ComponentFailed { vehicle, .. }
        | SimEvent::WheelDetached { vehicle, .. }
        | SimEvent::TotalLoss { vehicle }
        | SimEvent::TrafficViolation { vehicle, .. }
        | SimEvent::GlassShattered { vehicle, .. }
        | SimEvent::RegionDamaged { vehicle, .. } => vec![*vehicle],
        SimEvent::PoliceEvent { .. } | SimEvent::EconomyEvent { .. } => Vec::new(),
    }
}

/// Rotate `v` by unit quaternion `q = [x, y, z, w]`.
pub fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q;
    // t = 2 * cross(q.xyz, v); v' = v + w * t + cross(q.xyz, t)
    let t = [
        2.0 * (y * v[2] - z * v[1]),
        2.0 * (z * v[0] - x * v[2]),
        2.0 * (x * v[1] - y * v[0]),
    ];
    [
        v[0] + w * t[0] + (y * t[2] - z * t[1]),
        v[1] + w * t[1] + (z * t[0] - x * t[2]),
        v[2] + w * t[2] + (x * t[1] - y * t[0]),
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn dist(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    dot(d, d).sqrt()
}

/// Maximum distance of any lattice node from its rest position, m.
pub fn crush_depth(rest: &[[f32; 3]], now: &[[f32; 3]]) -> f32 {
    rest.iter()
        .zip(now)
        .map(|(r, n)| dist(*r, *n))
        .fold(0.0, f32::max)
}

struct Tracker {
    spawn: [f32; 3],
    rest_nodes: Vec<[f32; 3]>,
    vel_window: VecDeque<[f32; 3]>,
    last_pos: [f32; 3],
    lat_g: f32,
    long_g: f32,
    summary: EntitySummary,
    failed: BTreeSet<Component>,
}

impl Tracker {
    fn update(&mut self, s: &VehicleState) -> ([f32; 3], f32, f32) {
        let e = &mut self.summary;
        e.max_speed_mps = e.max_speed_mps.max(s.speed_mps);
        e.max_yaw_rate_rps = e.max_yaw_rate_rps.max(s.ang_vel[2].abs());
        e.min_z_m = e.min_z_m.min(s.pos[2]);
        e.path_length_m += dist(
            [s.pos[0], s.pos[1], 0.0],
            [self.last_pos[0], self.last_pos[1], 0.0],
        );
        self.last_pos = s.pos;
        self.vel_window.push_back(s.lin_vel);
        let window = PHYSICS_TICKS_PER_NET_TICK as usize;
        if self.vel_window.len() > window + 1 {
            self.vel_window.pop_front();
        }
        let mut accel = [0.0; 3];
        if self.vel_window.len() == window + 1 {
            let old = self.vel_window[0];
            let span = window as f32 * PHYSICS_DT;
            accel = [
                (s.lin_vel[0] - old[0]) / span,
                (s.lin_vel[1] - old[1]) / span,
                (s.lin_vel[2] - old[2]) / span,
            ];
        }
        self.lat_g = dot(accel, rotate(s.rot, [1.0, 0.0, 0.0])) / GRAVITY_MPS2;
        self.long_g = dot(accel, rotate(s.rot, [0.0, 1.0, 0.0])) / GRAVITY_MPS2;
        e.max_lateral_g = e.max_lateral_g.max(self.lat_g.abs());
        e.max_long_g = e.max_long_g.max(self.long_g.abs());
        (accel, self.lat_g, self.long_g)
    }
}

/// Per-tick input for one entity, from its driver and keyframes (`prev` = the input applied
/// last tick). Public so the testbed drives scenarios exactly like the runner.
pub fn driver_input(
    sc: &Scenario,
    idx: usize,
    tick: u64,
    state: &VehicleState,
    prev: &VehicleInput,
) -> VehicleInput {
    let spec = &sc.entities[idx];
    let mut input = VehicleInput::default();
    if let Some(key) = spec.inputs.iter().rev().find(|k| tick_of(k.t_s) <= tick) {
        input = key.input;
        if tick_of(key.t_s) != tick {
            input.gear_up = false;
            input.gear_down = false;
        }
    }
    if let Driver::Cruise {
        target_mps,
        from_s,
        until_s,
    } = &spec.driver
    {
        let active = tick >= tick_of(*from_s) && until_s.is_none_or(|u| tick < tick_of(u));
        if active {
            // Pedals are quantised to 1/32 and held inside a ±0.1 m/s deadband so the
            // controller does not log an input change every tick (keeps replays small).
            let pedal = |x: f32| ((x * 0.5).clamp(0.0, 1.0) * 32.0).round() / 32.0;
            let err = target_mps - state.speed_mps;
            if err.abs() < 0.1 {
                input.throttle = prev.throttle;
                input.brake = prev.brake;
            } else if err >= 0.0 {
                input.throttle = pedal(err);
                input.brake = 0.0;
            } else {
                input.throttle = 0.0;
                input.brake = pedal(-err);
            }
        }
    }
    input
}

/// Run a scenario. Equivalent to [`run_with_hook`] with a no-op hook.
pub fn run(sc: &Scenario, data: &dyn DataSource, opts: &RunOptions) -> Result<RunOutput, RunError> {
    run_with_hook(sc, data, opts, &mut |_| Ok(()))
}

/// Run a scenario, calling `hook` after every tick. A hook error aborts the run with
/// [`RunError::Invariant`] (used by fuzzing).
pub fn run_with_hook(
    sc: &Scenario,
    data: &dyn DataSource,
    opts: &RunOptions,
    hook: &mut dyn FnMut(&TickView) -> Result<(), String>,
) -> Result<RunOutput, RunError> {
    let BuiltSim {
        mut sim,
        ids,
        cars,
        selection,
        warnings,
        has_city,
    } = build_sim(sc, data, opts)?;
    let mut trackers = Vec::new();
    for (spec, id) in sc.entities.iter().zip(&ids) {
        let car = &cars[&spec.car];
        trackers.push(Tracker {
            spawn: spec.pos,
            rest_nodes: car.lattice.nodes.iter().map(|n| n.pos_m).collect(),
            vel_window: VecDeque::new(),
            last_pos: spec.pos,
            lat_g: 0.0,
            long_g: 0.0,
            failed: BTreeSet::new(),
            summary: EntitySummary {
                id: spec.id.clone(),
                entity: id.0,
                car: spec.car.clone(),
                final_speed_mps: 0.0,
                max_speed_mps: 0.0,
                distance_m: 0.0,
                path_length_m: 0.0,
                final_health: 1.0,
                max_lateral_g: 0.0,
                max_long_g: 0.0,
                max_yaw_rate_rps: 0.0,
                crush_depth_m: 0.0,
                components_failed: 0,
                parts_detached: 0,
                wheels_detached: 0,
                total_loss: false,
                min_z_m: spec.pos[2],
            },
        });
    }
    let names: Vec<String> = sc.entities.iter().map(|e| e.id.clone()).collect();
    let mut world_tr = WorldTracker::default();
    let index_of = |id: EntityId| ids.iter().position(|x| *x == id);

    // Probe ticks for `speed_at` expectations: tick -> [(entity index, slot)].
    let mut probes: BTreeMap<(usize, u64), f32> = BTreeMap::new();
    for x in sc.expect.iter().chain(&sc.expect_pending) {
        if let Expect::SpeedAt { entity, t_s, .. } = x {
            let idx = sc
                .entity_index(entity.as_ref())
                .map_err(|e| RunError::Scenario(sc.name.clone(), e))?;
            probes.insert((idx, tick_of(*t_s)), f32::NAN);
        }
    }

    let ticks = sc.ticks();
    let sample_every = sc.sample_every();
    let mut last_inputs = vec![VehicleInput::default(); ids.len()];
    let mut replay = Replay::new(sc, &selection.to_string(), ids.len());
    let mut hashes = Vec::new();
    let mut metrics_csv = String::new();
    let mut events_jsonl = String::new();
    if opts.record {
        metrics_csv.push_str(
            "t_s,tick,entity,x,y,z,speed_mps,lat_g,long_g,yaw_rate_rps,engine_rpm,gear,health,crush_depth_m\n",
        );
    }
    let mut events_total = 0u64;
    let mut max_events_per_tick = 0u64;
    let mut police_first: BTreeMap<u16, u64> = BTreeMap::new();
    let mut event_counts: Vec<(&'static str, Vec<EntityId>)> = Vec::new();
    let mut net = NetStats::default();
    let mut net_events: Vec<SimEvent> = Vec::new();
    let mut failures = Vec::new();
    let mut track_cursor = 0usize;

    let mut op_cursor = 0usize;
    for tick in 0..ticks {
        // Ops (reset / assists / mode), then inputs.
        if let Some(track) = &opts.op_track {
            while op_cursor < track.len() && track[op_cursor].tick == tick {
                apply_op(&mut sim, &ids, data, &track[op_cursor])?;
                if opts.record {
                    replay.ops.push(track[op_cursor]);
                }
                op_cursor += 1;
            }
        }
        // Inputs.
        if let Some(track) = &opts.input_track {
            while track_cursor < track.len() && track[track_cursor].tick == tick {
                let ev = &track[track_cursor];
                let idx = ev.entity as usize;
                let id = *ids.get(idx).ok_or_else(|| {
                    RunError::Replay(format!("input event for entity index {idx} out of range"))
                })?;
                sim.set_input(id, ev.input)?;
                last_inputs[idx] = ev.input;
                if opts.record {
                    replay.inputs.push(*ev);
                }
                track_cursor += 1;
            }
        } else {
            let states = sim.vehicle_states();
            for (idx, id) in ids.iter().enumerate() {
                let input = driver_input(sc, idx, tick, &states[idx].1, &last_inputs[idx]);
                if input != last_inputs[idx] {
                    sim.set_input(*id, input)?;
                    last_inputs[idx] = input;
                    if opts.record {
                        replay.inputs.push(InputEvent {
                            tick,
                            entity: idx as u16,
                            input,
                        });
                    }
                }
            }
        }

        sim.step_fixed();
        let events = sim.drain_events();
        let now = sim.tick();
        let t_s = now as f32 * PHYSICS_DT;

        // Events.
        events_total += events.len() as u64;
        max_events_per_tick = max_events_per_tick.max(events.len() as u64);
        for e in &events {
            event_counts.push((event_name(e), event_entities(e)));
            match e {
                SimEvent::PoliceEvent { kind, .. } => {
                    police_first.entry(*kind).or_insert(now);
                }
                SimEvent::ComponentFailed { vehicle, component } => {
                    if let Some(i) = index_of(*vehicle) {
                        trackers[i].failed.insert(*component);
                    }
                }
                SimEvent::PartDetached { vehicle, .. } => {
                    if let Some(i) = index_of(*vehicle) {
                        trackers[i].summary.parts_detached += 1;
                    }
                }
                SimEvent::WheelDetached { vehicle, .. } => {
                    if let Some(i) = index_of(*vehicle) {
                        trackers[i].summary.wheels_detached += 1;
                    }
                }
                SimEvent::TotalLoss { vehicle } => {
                    if let Some(i) = index_of(*vehicle) {
                        trackers[i].summary.total_loss = true;
                    }
                }
                _ => {}
            }
            if opts.record {
                let json = serde_json::to_string(e).expect("event serializes");
                let _ = writeln!(
                    events_jsonl,
                    "{{\"tick\":{now},\"t_s\":{t_s:.6},\"event\":{json}}}"
                );
            }
        }

        // Metrics.
        let states = sim.vehicle_states();
        let sample = now.is_multiple_of(sample_every);
        for (idx, (id, s)) in states.iter().enumerate() {
            let tr = &mut trackers[idx];
            tr.update(s);
            if let Some(slot) = probes.get_mut(&(idx, now)) {
                *slot = s.speed_mps;
            }
            if sample || now == ticks {
                let nodes = sim.node_positions(*id)?;
                tr.summary.crush_depth_m = tr
                    .summary
                    .crush_depth_m
                    .max(crush_depth(&tr.rest_nodes, nodes));
            }
            if opts.record && sample {
                let _ = writeln!(
                    metrics_csv,
                    "{t_s:.4},{now},{},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.1},{},{:.4},{:.4}",
                    names[idx],
                    s.pos[0],
                    s.pos[1],
                    s.pos[2],
                    s.speed_mps,
                    tr.lat_g,
                    tr.long_g,
                    s.ang_vel[2],
                    s.engine_rpm,
                    s.gear,
                    s.health,
                    tr.summary.crush_depth_m
                );
            }
        }

        // World / traffic.
        if has_city {
            world_tr.update(sim.world(), now);
        }

        // Hashes.
        let checkpoint = now.is_multiple_of(sc.hash_every) || now == ticks;
        if checkpoint {
            hashes.push((now, sim.state_hash()));
        }
        if opts.record {
            replay.tick_hashes.push(sim.state_hash());
            if checkpoint {
                let entity_hashes = sim.entity_hashes();
                replay.checkpoints.push(Checkpoint {
                    tick: now,
                    entities: states
                        .iter()
                        .zip(&entity_hashes)
                        .map(|((_, s), (_, hash))| EntityCheckpoint {
                            hash: *hash,
                            pos: s.pos,
                            lin_vel: s.lin_vel,
                            health: s.health,
                        })
                        .collect(),
                });
            }
        }

        // Net round trip.
        if selection.net {
            net_events.extend(events.iter().cloned());
            if now.is_multiple_of(PHYSICS_TICKS_PER_NET_TICK as u64) {
                let msg = ServerToClient::Snapshot {
                    tick: now,
                    vehicles: states.clone(),
                    events: std::mem::take(&mut net_events),
                };
                // Contract-message JSON round trip (the old wp_net stub codec). Workstream F
                // replaced it with its own snapshot protocol; moving this check onto
                // `wp_net::snapshot` is on I's list.
                let bytes = serde_json::to_vec(&msg).expect("ServerToClient serializes");
                net.snapshots += 1;
                net.bytes += bytes.len() as u64;
                match serde_json::from_slice::<ServerToClient>(&bytes) {
                    Ok(back) if back == msg => {}
                    Ok(_) => {
                        failures.push(format!("net: snapshot at tick {now} did not round-trip"))
                    }
                    Err(e) => failures.push(format!("net: snapshot at tick {now}: {e}")),
                }
            }
        }

        hook(&TickView {
            sim: &sim,
            events: &events,
            names: &names,
        })
        .map_err(|message| RunError::Invariant { tick: now, message })?;
    }
    replay.ticks = ticks;

    // Final per-entity values.
    let states = sim.vehicle_states();
    let mut entities = Vec::new();
    for (tr, (_, s)) in trackers.iter_mut().zip(&states) {
        tr.summary.final_speed_mps = s.speed_mps;
        tr.summary.final_health = s.health;
        tr.summary.distance_m = dist([s.pos[0], s.pos[1], 0.0], [tr.spawn[0], tr.spawn[1], 0.0]);
        tr.summary.components_failed = tr.failed.len() as u32;
        entities.push(tr.summary.clone());
    }

    let final_hash = sim.state_hash();
    let mut summary = RunSummary {
        name: sc.name.clone(),
        contract_version: wp_contracts::CONTRACT_VERSION.to_string(),
        hash_spec: wp_sim::HASH_SPEC_VERSION,
        systems: selection.to_string(),
        seed: sc.seed,
        ticks,
        final_hash: format!("{final_hash:016x}"),
        entities,
        events_total,
        max_events_per_tick,
        net,
        world: world_tr.finish(
            sim.world(),
            &sc.city,
            sim.prop_count(),
            sim.static_box_count(),
        ),
        warnings,
        expectations: Vec::new(),
        pending: Vec::new(),
        failures,
    };

    let ctx = EvalCtx {
        sc,
        summary: &summary,
        probes: &probes,
        police_first: &police_first,
        event_counts: &event_counts,
        ids: &ids,
    };
    let expectations: Vec<ExpectResult> = sc.expect.iter().map(|x| ctx.eval(x)).collect();
    let pending: Vec<ExpectResult> = sc.expect_pending.iter().map(|x| ctx.eval(x)).collect();
    for r in &expectations {
        if !r.passed {
            summary
                .failures
                .push(format!("{} (actual {})", r.expect, r.actual));
        }
    }
    if opts.strict {
        for w in &summary.warnings {
            summary.failures.push(format!("strict: {w}"));
        }
    }
    summary.expectations = expectations;
    summary.pending = pending;

    Ok(RunOutput {
        summary,
        hashes,
        metrics_csv,
        events_jsonl,
        replay,
    })
}

struct EvalCtx<'a> {
    sc: &'a Scenario,
    summary: &'a RunSummary,
    probes: &'a BTreeMap<(usize, u64), f32>,
    police_first: &'a BTreeMap<u16, u64>,
    event_counts: &'a [(&'static str, Vec<EntityId>)],
    ids: &'a [EntityId],
}

impl EvalCtx<'_> {
    fn ent(&self, name: Option<&String>) -> (usize, &EntitySummary) {
        let idx = self.sc.entity_index(name).expect("validated");
        (idx, &self.summary.entities[idx])
    }

    fn eval(&self, x: &Expect) -> ExpectResult {
        let range = |v: f32, min: f32, max: f32| (format!("{v}"), v >= min && v <= max);
        let (actual, passed) = match x {
            Expect::Metric {
                name,
                entity,
                min,
                max,
            } => match self.ent(entity.as_ref()).1.metric(name) {
                Some(v) => range(v, *min, *max),
                None => (
                    format!(
                        "unknown metric {name:?}; known: {:?}",
                        EntitySummary::METRICS
                    ),
                    false,
                ),
            },
            Expect::SpeedAt {
                entity,
                t_s,
                min,
                max,
            } => {
                let (idx, _) = self.ent(entity.as_ref());
                match self.probes.get(&(idx, tick_of(*t_s))) {
                    Some(v) if !v.is_nan() => range(*v, *min, *max),
                    _ => (format!("t_s {t_s} is outside the run"), false),
                }
            }
            Expect::MaxLateralG { entity, min, max } => {
                range(self.ent(entity.as_ref()).1.max_lateral_g, *min, *max)
            }
            Expect::CrushDepth { entity, min, max } => {
                range(self.ent(entity.as_ref()).1.crush_depth_m, *min, *max)
            }
            Expect::ComponentsFailed { entity, min, max } => {
                let v = self.ent(entity.as_ref()).1.components_failed;
                (v.to_string(), v >= *min && v <= *max)
            }
            Expect::TotalLoss { entity, expected } => {
                let v = self.ent(entity.as_ref()).1.total_loss;
                (v.to_string(), v == *expected)
            }
            Expect::WorldMetric { name, min, max } => match self.summary.world.metric(name) {
                Some(v) => range(v, *min, *max),
                None => (
                    format!(
                        "unknown world metric {name:?}; known: {:?}",
                        WorldSummary::METRICS
                    ),
                    false,
                ),
            },
            Expect::PursuitStartedBefore { t_s } => self.police_before(PURSUIT_STARTED_KIND, *t_s),
            Expect::PoliceEventBefore { kind, t_s } => self.police_before(*kind, *t_s),
            Expect::EventCount {
                event,
                entity,
                min,
                max,
            } => {
                let filter = entity.as_ref().map(|n| self.ids[self.ent(Some(n)).0]);
                let n = self
                    .event_counts
                    .iter()
                    .filter(|(name, ents)| {
                        *name == event.as_str() && filter.is_none_or(|f| ents.contains(&f))
                    })
                    .count() as u32;
                (n.to_string(), n >= *min && n <= *max)
            }
        };
        ExpectResult {
            expect: crate::scenario::to_ron_line(x),
            actual,
            passed,
        }
    }

    fn police_before(&self, kind: u16, t_s: f32) -> (String, bool) {
        match self.police_first.get(&kind) {
            Some(tick) => (
                format!("first at {:.3} s", *tick as f32 * PHYSICS_DT),
                *tick <= tick_of(t_s),
            ),
            None => ("never".to_string(), false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotate_yaw_quarter_turn() {
        // 90° about +Z: +X -> +Y.
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let v = rotate([0.0, 0.0, h, h], [1.0, 0.0, 0.0]);
        assert!((v[0]).abs() < 1e-6 && (v[1] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn selection_parse() {
        let s = Selection::parse("vehicle,net").unwrap();
        assert!(s.net && s.sim.vehicle && !s.sim.damage);
        assert_eq!(s.to_string(), "vehicle,net");
        assert_eq!(Selection::parse("all").unwrap(), Selection::ALL);
        assert_eq!(
            Selection::ALL.to_string(),
            "world,vehicle,damage,police,net"
        );
    }
}
