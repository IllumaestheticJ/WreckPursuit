//! # wp_world
//!
//! City, road network, signals, civilian traffic and the day/night clock (workstream 05).
//!
//! - [`network`]: the road file format (`data/city/<city>.road.bin` + `.road.json`).
//! - [`builder`]: lane-graph builder from an abstract road graph (used by [`procgen`] and the
//!   OSM importer in `tools/city-import`).
//! - [`procgen`]: procedural fallback city (`gen-city --grid 12x12 --seed N`).
//! - [`map`]: runtime map (connectors, conflicts, spatial grid) implementing
//!   [`WorldQuery`].
//! - [`traffic`]: civilian agents (IDM, MOBIL, junction right of way, hazards, density
//!   spawning, kinematic/physical LOD).
//! - [`drive`]: the shared AI driving controller (pure pursuit + PID) used by physical
//!   traffic, racers (`wp_opponent`) and police.
//! - [`covered`]: tunnels and covered roads (line of sight in and out is blocked).
//! - [`density`], [`signals`], [`clock`], [`violations`], [`routes`], [`props`].
//!
//! [`World`] composes them. The sim loop calls [`World::set_tracked_vehicles`] (players,
//! police, sirens) and then [`World::step`] once per physics tick; traffic AI runs at 60 Hz
//! inside, physical traffic vehicles step every tick.

#![warn(missing_docs)]

pub mod builder;
pub mod clock;
pub mod covered;
pub mod density;
pub mod drive;
pub mod geom;
pub mod leader;
pub mod manoeuvre;
pub mod map;
pub mod markings;
pub mod network;
pub mod procgen;
pub mod props;
pub mod residential;
pub mod roads;
pub mod routes;
pub mod signals;
pub mod surface;
pub mod terrain;
pub mod traffic;
pub mod vesper;
pub mod violations;

use std::path::Path;

use wp_contracts::PHYSICS_DT;
use wp_contracts::{
    HandlingEffects, LoadedCar, SignalState, SimEvent, SurfaceType, TimeOfDay, WorldQuery,
};
use wp_math::StateHasher;

pub use clock::{sun_direction, WorldClock, DEFAULT_TIME_RATIO};
pub use map::{CityMap, SegId};
pub use network::{RoadError, RoadNetwork};
pub use props::{PropKind, StaticProp};
pub use roads::{RoadEdge, RoadGraphView, RoadNetworkIndex};
pub use routes::{ActiveRoute, Checkpoint, Route, RouteClosure};
pub use traffic::{
    AgentId, AgentView, Hazard, Lod, LodChange, PhysicalAgent, TrackedVehicle, TrafficConfig,
    TrafficStats, TICKS_PER_TRAFFIC_STEP, TRAFFIC_DT,
};
pub use violations::{ViolationConfig, ViolationTracker};

use traffic::{Traffic, TrafficInputs};

/// World construction parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct WorldConfig {
    /// Seed for traffic and parked cars.
    pub seed: u64,
    /// Starting time of day.
    pub start_time: TimeOfDay,
    /// Clock ratio (in-game seconds per simulated second).
    pub time_ratio: f32,
    /// Traffic tuning.
    pub traffic: TrafficConfig,
    /// Generate parked cars along parking lanes.
    pub parked_cars: bool,
}

impl Default for WorldConfig {
    fn default() -> Self {
        Self {
            seed: 1,
            start_time: TimeOfDay::from_hours(12.0),
            time_ratio: DEFAULT_TIME_RATIO,
            traffic: TrafficConfig::default(),
            parked_cars: true,
        }
    }
}

/// The world: road network, signals, traffic, violations and clock.
#[derive(Debug, Clone)]
pub struct World {
    /// Day/night clock.
    pub clock: WorldClock,
    map: CityMap,
    traffic: Traffic,
    violations: ViolationTracker,
    tracked: Vec<TrackedVehicle>,
    hazards: Vec<traffic::Hazard>,
    routes: Vec<ActiveRoute>,
    route_focus: Vec<geom::V2>,
    closed: Vec<bool>,
    cones: Vec<StaticProp>,
    parked: Vec<StaticProp>,
    /// Display name per source road (`Lane::road` index; empty when the city has none).
    /// Presentation only (police radio callouts): never read by the simulation or hashed.
    road_names: Vec<String>,
    /// Physics ticks run (the world's simulation clock).
    tick: u64,
}

impl Default for World {
    fn default() -> Self {
        Self::flat(TimeOfDay::from_hours(12.0), DEFAULT_TIME_RATIO)
    }
}

fn empty_network() -> RoadNetwork {
    RoadNetwork {
        format_version: network::ROAD_FORMAT_VERSION,
        id: "flat".into(),
        source: "empty".into(),
        attribution: None,
        bounds_min: [0.0; 2],
        bounds_max: [0.0; 2],
        core: [0.0; 2],
        nodes: Vec::new(),
        lanes: Vec::new(),
        intersections: Vec::new(),
        signals: Vec::new(),
        zones: Vec::new(),
        pois: Vec::new(),
        buildings: Vec::new(),
        surfaces: Vec::new(),
        terrain: None,
        covered: Vec::new(),
        decor: Vec::new(),
    }
}

impl World {
    /// An empty flat world (infinite asphalt, no limits/signals/traffic) starting at `time`.
    pub fn flat(time: TimeOfDay, ratio: f32) -> Self {
        Self::new(
            empty_network(),
            WorldConfig {
                start_time: time,
                time_ratio: ratio,
                ..Default::default()
            },
        )
    }

    /// World over a road network.
    pub fn new(net: RoadNetwork, cfg: WorldConfig) -> Self {
        let mut map = CityMap::new(net);
        map.time = cfg.start_time;
        let traffic = Traffic::new(&map, cfg.traffic.clone(), cfg.seed);
        let parked = if cfg.parked_cars {
            // Kerbside cars (never across a driveway mouth), then the driveway cars
            // (residential lots, playtest row 95).
            let mut cars = props::parked_cars(&map, cfg.seed);
            cars.retain(|c| !residential::blocks_driveway(&map.net, c.pos));
            cars.extend(residential::driveway_cars(&map, cfg.seed));
            cars
        } else {
            Vec::new()
        };
        let n_lanes = map.n_lanes();
        Self {
            clock: WorldClock::new(cfg.start_time, cfg.time_ratio),
            map,
            traffic,
            violations: ViolationTracker::default(),
            tracked: Vec::new(),
            hazards: Vec::new(),
            routes: Vec::new(),
            route_focus: Vec::new(),
            closed: vec![false; n_lanes],
            cones: Vec::new(),
            parked,
            road_names: Vec::new(),
            tick: 0,
        }
    }

    /// Load a `.road.bin` / `.road.json` city.
    pub fn load(path: impl AsRef<Path>, cfg: WorldConfig) -> Result<Self, RoadError> {
        Ok(Self::new(RoadNetwork::load(path)?, cfg))
    }

    /// Load from `.road.bin` bytes (browser / embedded data).
    pub fn from_bin(bytes: &[u8], cfg: WorldConfig) -> Result<Self, RoadError> {
        Ok(Self::new(RoadNetwork::from_bin(bytes)?, cfg))
    }

    /// Generate the procedural city in memory.
    pub fn procedural(pg: &procgen::ProcGenConfig, cfg: WorldConfig) -> Self {
        Self::new(procgen::generate(pg), cfg)
    }

    /// Generate Port Vesper v2 (curves, elevation, terrain; `vesper::CITY_ID`) in memory.
    /// Road names come from the generator ([`World::road_names`]).
    pub fn vesper(vc: &vesper::VesperConfig, cfg: WorldConfig) -> Self {
        let (net, report) = vesper::generate_with_report(vc);
        let mut w = Self::new(net, cfg);
        w.road_names = report.road_names.iter().map(|n| n.to_string()).collect();
        w
    }

    /// Display name per source road (`Lane::road` index), e.g. `Harbor Boulevard`. Empty for
    /// cities without names (the procedural grid, loaded `.road.bin` files). Presentation only.
    pub fn road_names(&self) -> &[String] {
        &self.road_names
    }

    /// Set the display names per source road (tests, cities loaded from files).
    pub fn set_road_names(&mut self, names: Vec<String>) {
        self.road_names = names;
    }

    /// Display name of every road-graph edge (`RoadEdgeId` order, [`World::road_graph`]);
    /// `""` where the road has no name.
    pub fn edge_names(&self) -> Vec<String> {
        roads::edge_roads(&self.map)
            .into_iter()
            .map(|r| self.road_names.get(r as usize).cloned().unwrap_or_default())
            .collect()
    }

    /// Index of the tunnel / covered road containing `p` (into `network().covered`), if any.
    pub fn covered_at(&self, p: [f32; 3]) -> Option<usize> {
        self.map.covered_at(p)
    }

    /// Turn the line-of-sight effect of covered spans on or off (default on). Off is the
    /// control run of tunnel scenarios: same geometry, sightlines ignore the cover.
    pub fn set_covered_los(&mut self, on: bool) {
        self.map.covered_los = on;
    }

    /// Car used for physical (near-LOD) traffic. Without one, traffic stays kinematic.
    pub fn set_traffic_car(&mut self, car: &LoadedCar) {
        self.traffic
            .set_traffic_car(car.def.clone(), car.tires.clone());
    }

    /// Several car models for physical traffic; agent `id` drives model
    /// [`traffic::traffic_model_index`]`(id, cars.len())` whether kinematic or physical. One
    /// model behaves exactly like [`World::set_traffic_car`]; none keeps traffic kinematic.
    pub fn set_traffic_cars(&mut self, cars: &[LoadedCar]) {
        self.traffic.set_traffic_cars(
            cars.iter()
                .map(|c| (c.def.clone(), c.tires.clone()))
                .collect(),
        );
    }

    /// The traffic model index agent `id` drives (see [`World::set_traffic_cars`]).
    pub fn traffic_model_of(&self, id: AgentId) -> usize {
        self.traffic.model_of(id)
    }

    /// Vehicles the world must know about this tick: players, police, AI racers.
    pub fn set_tracked_vehicles(&mut self, tracked: &[TrackedVehicle]) {
        self.tracked.clear();
        self.tracked.extend_from_slice(tracked);
    }

    /// Debris / obstacles civilians should avoid: every loose body near the road each tick
    /// (detached parts, wheels, knocked props). Lane-blocking debris makes drivers change
    /// lanes or stop; small debris is swerved around within the lane; debris in a junction
    /// box blocks the connectors through it.
    pub fn set_hazards(&mut self, hazards: &[traffic::Hazard]) {
        self.hazards.clear();
        self.hazards.extend_from_slice(hazards);
    }

    /// Active race routes: focus areas for spawning and physical LOD; `closed` ones suppress
    /// traffic on their roads and add cones.
    pub fn set_active_routes(&mut self, routes: Vec<ActiveRoute>) {
        self.closed.iter_mut().for_each(|c| *c = false);
        self.cones.clear();
        self.route_focus.clear();
        for r in &routes {
            let cum = geom::cumulative(&r.route.polyline);
            let len = *cum.last().unwrap_or(&0.0);
            let mut d = 0.0;
            while d <= len && !r.route.polyline.is_empty() {
                let (p, _) = geom::point_at(&r.route.polyline, &cum, d);
                self.route_focus.push(geom::xy(p));
                d += 40.0;
            }
            if r.closed {
                let c = routes::closure_for_route(&self.map, &r.route);
                for l in c.closed_lanes {
                    self.closed[l as usize] = true;
                }
                self.cones.extend(c.cones);
            }
        }
        self.routes = routes;
    }

    /// Fill the active area to its density targets immediately (call after setting tracked
    /// vehicles; the sightline rule still applies).
    pub fn populate(&mut self) {
        let inp = TrafficInputs {
            tracked: &self.tracked,
            hazards: &self.hazards,
            route_focus: &self.route_focus,
            closed: &self.closed,
        };
        self.traffic.populate(&self.map, &inp);
    }

    /// Advance clock, signals and traffic by `dt` seconds (normally `PHYSICS_DT`; other values
    /// advance `round(dt / PHYSICS_DT)` ticks of signal/traffic time). Pushes
    /// `TrafficViolation` events (and any vehicle events from physical traffic) into `out`.
    pub fn step(&mut self, dt: f32, out: &mut Vec<SimEvent>) {
        self.clock.step(dt);
        let ticks = (dt / PHYSICS_DT).round() as u64;
        let traffic_steps =
            (self.tick + ticks) / TICKS_PER_TRAFFIC_STEP - self.tick / TICKS_PER_TRAFFIC_STEP;
        self.tick += ticks;
        self.map.signal_tick = self.tick;
        self.map.time = self.clock.time;
        if self.map.segs.is_empty() {
            return;
        }
        self.traffic.step_physics(&self.map, dt, out);
        // The sim loop tracks every vehicle, including this world's own physical traffic cars.
        // Traffic already knows those (they are its agents): as "tracked" they would block
        // their own kinematic agent, count as players for spawning / promotion, and collect
        // traffic violations (a civilian earning police heat).
        if traffic_steps > 0 {
            let own: std::collections::BTreeSet<wp_contracts::EntityId> = self
                .traffic
                .agents()
                .iter()
                .filter_map(|a| a.entity)
                .collect();
            if !own.is_empty() {
                self.tracked.retain(|t| !own.contains(&t.entity));
            }
        }
        for _ in 0..traffic_steps {
            let inp = TrafficInputs {
                tracked: &self.tracked,
                hazards: &self.hazards,
                route_focus: &self.route_focus,
                closed: &self.closed,
            };
            self.traffic.step(&self.map, &inp, out);
            self.violations.update(
                &self.map,
                &self.tracked,
                &self.traffic,
                TRAFFIC_DT,
                self.traffic.step_count(),
                out,
            );
        }
    }

    /// Runtime map (segments, spatial queries).
    pub fn map(&self) -> &CityMap {
        &self.map
    }

    /// Road network data.
    pub fn network(&self) -> &RoadNetwork {
        &self.map.net
    }

    /// Road-level graph (nodes, roads with class/limit/one-way/district, POIs, building
    /// bounds) for police routing and minimaps. Built on each call; cache it.
    pub fn road_graph(&self) -> RoadGraphView {
        RoadGraphView::from_map(&self.map)
    }

    /// Physics ticks since the world started (signal clock).
    pub fn tick(&self) -> u64 {
        self.tick
    }

    /// Simulation seconds since the world started (`tick · PHYSICS_DT`; informational).
    pub fn sim_time_s(&self) -> f32 {
        self.tick as f32 * PHYSICS_DT
    }

    /// Renderer / netcode view of every civilian agent, ordered by id.
    pub fn agents(&self) -> impl Iterator<Item = AgentView> + '_ {
        self.traffic.views(&self.map)
    }

    /// Number of civilian agents.
    pub fn agent_count(&self) -> usize {
        self.traffic.agents().len()
    }

    /// Physical agents' vehicles (for the sim loop to attach rigid bodies and damage).
    pub fn physical_agents_mut(&mut self) -> impl Iterator<Item = (AgentId, &mut PhysicalAgent)> {
        self.traffic.physical_mut()
    }

    /// LOD transitions since the last call.
    pub fn drain_lod_changes(&mut self) -> Vec<LodChange> {
        self.traffic.drain_lod_changes()
    }

    /// Record the sim-loop entity id of a (physical) agent.
    pub fn set_agent_entity(&mut self, agent: AgentId, entity: Option<wp_contracts::EntityId>) {
        self.traffic.set_entity(agent, entity);
    }

    /// Push damage effects to a physical agent (a dead engine / lost wheel disables it).
    pub fn apply_agent_damage(&mut self, agent: AgentId, effects: &HandlingEffects) {
        self.traffic.apply_damage(agent, effects);
    }

    /// Static colliders: parked cars and route-closure cones.
    pub fn static_props(&self) -> impl Iterator<Item = &StaticProp> + '_ {
        self.parked.iter().chain(self.cones.iter())
    }

    /// Replace the parked cars, e.g. after the sim seated them on its ground colliders and
    /// dropped the spots where a car cannot stand (`wp_sim::Sim::set_world` on a city with
    /// terrain). Every reader of [`World::static_props`] (police obstacles, resets) then sees
    /// exactly the cars the sim has. Scenario setup, before the first tick.
    pub fn set_parked_cars(&mut self, cars: Vec<StaticProp>) {
        self.parked = cars;
    }

    /// Active routes.
    pub fn active_routes(&self) -> &[ActiveRoute] {
        &self.routes
    }

    /// Traffic counters.
    pub fn traffic_stats(&self) -> &TrafficStats {
        self.traffic.stats()
    }

    /// Traffic configuration (mutable: density scale, caps, diagnostics).
    pub fn traffic_config_mut(&mut self) -> &mut TrafficConfig {
        &mut self.traffic.config
    }

    /// Violation thresholds (mutable).
    pub fn violation_config_mut(&mut self) -> &mut ViolationConfig {
        &mut self.violations.config
    }

    /// Per-zone density targets and current counts, for debugging overlays and tests.
    pub fn zone_density(&self) -> (Vec<f32>, Vec<u32>) {
        let inp = TrafficInputs {
            tracked: &self.tracked,
            hazards: &self.hazards,
            route_focus: &self.route_focus,
            closed: &self.closed,
        };
        (
            self.traffic.zone_targets(&self.map, &inp),
            self.traffic.zone_counts(&self.map),
        )
    }

    /// Plan a lane-legal route between two POIs.
    pub fn plan_route_between_pois(&self, start_poi: usize, finish_poi: usize) -> Option<Route> {
        routes::plan_route_between_pois(&self.map, start_poi, finish_poi)
    }

    /// Plan a lane-legal route between two points.
    pub fn plan_route(
        &self,
        start: [f32; 3],
        heading: Option<[f32; 2]>,
        finish: [f32; 3],
    ) -> Option<Route> {
        routes::plan_route(&self.map, start, heading, finish)
    }

    /// Plan a closed circuit through polygon vertices.
    pub fn plan_circuit(&self, polygon: &[[f32; 2]]) -> Option<Route> {
        routes::plan_circuit(&self.map, polygon)
    }

    /// Deterministic hash of clock, signals and traffic.
    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        h.write_f32_q(self.clock.time.seconds_since_midnight, 1e2);
        h.write_u64(self.tick);
        self.traffic.hash_into(&mut h);
        h.finish()
    }
}

impl WorldQuery for World {
    fn surface_at(&self, p: [f32; 3]) -> SurfaceType {
        self.map.surface_at(p)
    }

    fn speed_limit_at(&self, p: [f32; 3]) -> Option<f32> {
        self.map.speed_limit_at(p)
    }

    fn signal_state_ahead(&self, p: [f32; 3], heading: [f32; 3]) -> Option<SignalState> {
        self.map.signal_state_ahead(p, heading)
    }

    fn time_of_day(&self) -> TimeOfDay {
        self.clock.time
    }

    fn line_of_sight(&self, a: [f32; 3], b: [f32; 3]) -> bool {
        self.map.line_of_sight(a, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_world_matches_old_stub() {
        let mut w = World::flat(TimeOfDay::from_hours(23.5), 10.0);
        let mut ev = Vec::new();
        for _ in 0..360 {
            w.step(1.0, &mut ev);
        }
        assert!((w.time_of_day().hours() - 0.5).abs() < 1e-3);
        assert_eq!(w.surface_at([0.0; 3]), SurfaceType::Asphalt);
        assert_eq!(w.speed_limit_at([0.0; 3]), None);
        assert_eq!(w.signal_state_ahead([0.0; 3], [0.0, 1.0, 0.0]), None);
        assert_eq!(w.agent_count(), 0);
        assert!(ev.is_empty());
    }

    #[test]
    fn vesper_edges_are_named_after_their_roads() {
        let w = World::vesper(&vesper::VesperConfig::default(), WorldConfig::default());
        let names = w.edge_names();
        assert_eq!(names.len(), w.road_graph().edges.len());
        assert!(
            names.iter().all(|n| !n.is_empty()),
            "every Vesper road has a name"
        );
        assert!(names.iter().any(|n| n == "Harbor Boulevard"));
        // The procedural grid has no names: every edge is unnamed.
        let g = World::procedural(&procgen::ProcGenConfig::small(), WorldConfig::default());
        assert!(g.edge_names().iter().all(|n| n.is_empty()));
        assert_eq!(g.edge_names().len(), g.road_graph().edges.len());
    }
}
