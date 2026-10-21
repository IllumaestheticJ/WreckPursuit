//! The scenario runner: builds a [`Sim`] from a [`Scenario`], drives it tick by tick, collects
//! metrics, events and hashes, and evaluates expectations.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt::Write as _;

use serde::Serialize;
use wp_contracts::{
    Component, EntityId, SimEvent, TimeOfDay, VehicleInput, VehicleState, GRAVITY_MPS2, PHYSICS_DT,
    PHYSICS_TICKS_PER_NET_TICK,
};
use wp_sim::{Sim, SimConfig, Systems};

use crate::data::{load_car, load_mod_catalog, load_modded_car, load_mode, load_world, DataSource};
use crate::replay::{Checkpoint, EntityCheckpoint, InputEvent, OpEvent, OpKind, Replay};
use crate::scenario::{tick_of, CitySpec, Driver, Expect, OrderKind, Scenario};
use crate::RunError;

/// `wp_police` payload kind treated as "pursuit started" by [`Expect::PursuitStartedBefore`]
/// (`wp_police::events::kind::PURSUIT_STARTED`; the full list is in that module).
pub const PURSUIT_STARTED_KIND: u16 = wp_contracts::police_kind::PURSUIT_STARTED;

/// Systems to run: the [`Systems`] inside [`Sim`] plus the harness-side net round trip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    /// Systems stepped by the sim.
    pub sim: Systems,
    /// Round-trip a snapshot through `wp_net`'s wire codec every net tick ([`crate::net`]) and check
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
    /// Loaded cars by car id (stock).
    pub cars: BTreeMap<String, wp_contracts::LoadedCar>,
    /// The modified car of every scenario entity with `mods` (it drives this, not the stock
    /// car in `cars`).
    pub modded: BTreeMap<EntityId, wp_contracts::LoadedCar>,
    /// Systems that will run.
    pub selection: Selection,
    /// Unsupported features / unapplied overrides.
    pub warnings: Vec<String>,
    /// Whether a city (`wp_world`) was loaded instead of the flat world.
    pub has_city: bool,
    /// Route drivers per scenario entity (`Driver::FollowRoute`), in scenario order.
    pub routes: Vec<Option<RouteDriver>>,
}

/// A `Driver::FollowRoute` entity's planned route and path follower.
#[derive(Debug, Clone)]
pub struct RouteDriver {
    /// The route as a drivable path.
    pub path: wp_world::drive::DrivePath,
    /// Shared path follower (pursuit limits).
    pub follower: wp_world::drive::PathFollower,
    /// Target speed, m/s.
    pub target_mps: f32,
    /// Lateral offset from the route (+ right), m.
    pub lateral_offset_m: f32,
    /// Keep distance from vehicles ahead (`follow_route(keep_distance: true)`).
    pub keep_distance: bool,
    /// Furthest arc length reached, m.
    pub progress_m: f32,
}

impl RouteDriver {
    /// Input for this tick (advances the follower's PID by one physics tick). `obstacles`: the
    /// vehicles to keep distance from (only used with `keep_distance`).
    pub fn input(
        &mut self,
        state: &wp_contracts::VehicleState,
        obstacles: &[wp_world::leader::Obstacle],
    ) -> VehicleInput {
        let traffic = wp_world::drive::FollowTraffic {
            obstacles,
            skip: &[],
            corridor: wp_world::leader::LeaderParams::CAR,
            following: wp_world::leader::CarFollowing::PURSUIT,
        };
        let out = self.follower.follow_behind(
            state,
            &self.path,
            self.target_mps,
            self.lateral_offset_m,
            PHYSICS_DT,
            self.keep_distance.then_some(&traffic),
        );
        self.progress_m = self.progress_m.max(out.s);
        // Stopped at the finish (or crawling to it): hold on the handbrake, as traffic does —
        // the follower releases the foot brake near standstill (auto-reverse).
        if out.done
            || (out.remaining_m < 3.0 && state.speed_mps < wp_world::leader::HOLD_BEHIND_MPS)
        {
            wp_world::leader::held(out.input)
        } else {
            out.input
        }
    }

    /// Fraction of the route covered, 0..=1.
    pub fn progress(&self) -> f32 {
        (self.progress_m / self.path.length().max(1e-3)).clamp(0.0, 1.0)
    }
}

/// States of the scenario's own entities, in scenario order. City runs add police and traffic
/// vehicles (spawned later, so higher ids); expectations and metrics ignore those.
pub fn scenario_states(sim: &Sim, ids: &[EntityId]) -> Vec<(EntityId, wp_contracts::VehicleState)> {
    sim.vehicle_states()
        .into_iter()
        .filter(|(id, _)| ids.contains(id))
        .collect()
}

/// Car model for AI police units in city scenarios.
pub const POLICE_CAR: &str = "sentinel_cruiser";
/// Car model for physical (near-player) traffic in city scenarios.
pub const TRAFFIC_CAR: &str = "debug_sedan";
/// Car model whose lattice dents parked cars (`Sim::set_parked_car`); the same model as
/// traffic, so the testbed draws parked cars with the traffic mesh.
pub const PARKED_CAR: &str = TRAFFIC_CAR;

/// `car_mix` police fleet: `(car, min level, max level, measured limits)`. Limits are
/// `[skidpad g, 100-0 m, 0-100 s, vmax km/h]` from `data/cars/VALIDATION.md` (sim-measured,
/// `tools/specsheet/collect_results.py`); `None` = `pursuit.toml`'s defaults (the cruiser's).
/// Patrols picked while nobody is wanted count as level 1.
pub const CAR_MIX_POLICE: &[(&str, u8, u8, Option<[f32; 4]>)] = &[
    ("sentinel_warden", 1, 2, Some([0.81, 40.1, 8.8, 202.0])),
    ("sentinel_cruiser", 1, 2, None),
    ("sentinel_bastion", 2, 3, Some([0.72, 42.7, 7.97, 214.0])),
    ("sentinel_wolfhound", 3, 5, Some([0.85, 39.6, 5.54, 244.0])),
    ("sentinel_vigil", 4, 5, Some([0.87, 38.2, 5.74, 275.0])),
];

/// `car_mix` physical traffic models, in model-index order (agent `id` drives
/// `CAR_MIX_TRAFFIC[wp_world::traffic::traffic_model_index(id, len)]`).
pub const CAR_MIX_TRAFFIC: &[&str] = &[
    "debug_sedan",
    "junkyard_beater",
    "tansy_13",
    "furrow_24",
    "hauler_1500",
    "drayman_cargo",
    "vireo_xs",
];

/// Load the `car_mix` police fleet ([`CAR_MIX_POLICE`]).
pub fn car_mix_police(data: &dyn DataSource) -> Result<Vec<wp_sim::PoliceModel>, RunError> {
    CAR_MIX_POLICE
        .iter()
        .map(|(id, lo, hi, limits)| {
            let m = wp_sim::PoliceModel::new(load_car(data, id)?, *lo, *hi);
            Ok(match limits {
                Some([g, stop, t, vmax]) => m.with_limits(wp_police::pro::CarLimits::from_metrics(
                    *g, *stop, *t, *vmax,
                )),
                None => m,
            })
        })
        .collect()
}

/// Load the `car_mix` traffic models ([`CAR_MIX_TRAFFIC`]).
pub fn car_mix_traffic(data: &dyn DataSource) -> Result<Vec<wp_contracts::LoadedCar>, RunError> {
    CAR_MIX_TRAFFIC
        .iter()
        .map(|id| load_car(data, id))
        .collect()
}

/// The `wp_world` settings a scenario implies (seed, clock, traffic density override).
pub fn world_config(sc: &Scenario) -> wp_world::WorldConfig {
    let density = sc.traffic_density.unwrap_or(1.0);
    wp_world::WorldConfig {
        seed: sc.seed,
        start_time: TimeOfDay::from_hours(sc.time_of_day_h),
        time_ratio: sc.time_ratio,
        traffic: wp_world::TrafficConfig {
            enabled: sc.traffic_cars && density > 0.0,
            density_scale: density,
            detect_collisions: true,
            ..Default::default()
        },
        parked_cars: sc.parked_cars,
    }
}

/// The `wp_police` tables a scenario implies: the built-in tables with every patrol count
/// scaled by `police_presence` (rounded to whole units).
pub fn police_config(sc: &Scenario) -> wp_police::PoliceConfig {
    let mut cfg = wp_police::PoliceConfig::builtin();
    if let Some(p) = sc.police_presence {
        let scale = |n: &mut u32| *n = (*n as f32 * p).round() as u32;
        let t = &mut cfg.presence.patrols;
        for d in [&mut t.core, &mut t.suburb, &mut t.industrial] {
            for n in [&mut d.dawn, &mut d.day, &mut d.dusk, &mut d.night] {
                scale(n);
            }
        }
    }
    cfg
}

/// Police for a city scenario: `wp_police` on the city's road graph with its built-in kinematic
/// unit driver (units are not physics bodies yet), seeded from the scenario seed.
/// Entities with `police_heat` start with that heat (`ids` in scenario order).
pub fn police_for_city(
    sc: &Scenario,
    world: &wp_world::World,
    ids: &[EntityId],
) -> wp_police::PoliceSystem {
    let mut police = wp_police::PoliceSystem::for_world(
        police_config(sc),
        world,
        wp_police::PoliceSetup {
            seed: sc.seed ^ 0x0090_11CE,
            mode: sc.mode,
            internal_driving: true,
            hazards: sc.traffic_cars,
        },
    );
    for (spec, id) in sc.entities.iter().zip(ids) {
        if spec.police_heat > 0.0 {
            police.set_heat(*id, spec.police_heat);
        }
    }
    police
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
    if sc.city == CitySpec::Flat && sc.police_presence.is_some() {
        warnings.push("police_presence ignored: the flat world has no roads for police".into());
    }
    for e in &sc.entities {
        match &e.driver {
            Driver::AiDriver { .. } => warnings.push(format!(
                "entity {:?}: driver {:?} not available yet; using its input keyframes",
                e.id, e.driver
            )),
            Driver::FollowRoute { route_id, .. } if !route_id.contains("->") => {
                warnings.push(format!(
                    "entity {:?}: driver {:?} not available yet; using its input keyframes",
                    e.id, e.driver
                ))
            }
            Driver::FollowRoute { .. } if sc.city == CitySpec::Flat => warnings.push(format!(
                "entity {:?}: follow_route needs a city; using its input keyframes",
                e.id
            )),
            Driver::Scripted | Driver::Cruise { .. } | Driver::FollowRoute { .. } => {}
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
    let mut modded = BTreeMap::new();
    let catalog = if sc.entities.iter().any(|e| !e.mods.is_empty()) {
        Some(load_mod_catalog(data)?)
    } else {
        None
    };
    for spec in &sc.entities {
        if !cars.contains_key(&spec.car) {
            cars.insert(spec.car.clone(), load_car(data, &spec.car)?);
        }
        let id = match &catalog {
            Some(catalog) if !spec.mods.is_empty() => {
                let car = load_modded_car(data, catalog, &spec.car, &spec.mods, sc.mode)?;
                let id = sim.spawn_vehicle_tuned(&car, spec.pos, spec.yaw, spec.tune)?;
                modded.insert(id, car);
                id
            }
            _ => sim.spawn_vehicle_tuned(&cars[&spec.car], spec.pos, spec.yaw, spec.tune)?,
        };
        if spec.speed_mps != 0.0 {
            sim.place_vehicle(id, spec.pos, spec.yaw, spec.speed_mps)?;
        }
        sim.set_assists(id, spec.assists.unwrap_or(sc.assists).profile())?;
        // Player steering aids: a live-driven entity only (see `EntitySpec::player_steering`).
        let live = matches!(spec.driver, Driver::Scripted) && spec.inputs.is_empty();
        match spec.player_steering {
            Some(on) => sim.set_player_steering(id, Some(on))?,
            None if !live => sim.set_player_steering(id, Some(false))?,
            None => {}
        }
        if sc.panels {
            let rel = format!("panels/{}.json", spec.car);
            let spec_p = wp_damage::PanelSpec::parse(&spec.car, &data.read(&rel)?)
                .map_err(|e| RunError::Scenario(sc.name.clone(), e.to_string()))?;
            sim.damage_mut(id)?
                .attach_panels(&spec_p)
                .map_err(|e| RunError::Scenario(sc.name.clone(), e.to_string()))?;
        }
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
            // `set_world` rebuilds police on the city, so the scenario's police (config,
            // starting heat) are installed after it. Police cars are real vehicles (lead run 5);
            // `police_cars: false` keeps heat and wanted levels with no patrols.
            world.set_covered_los(sc.covered_los);
            sim.set_world(world);
            if sc.police_cars {
                sim.set_police(police_for_city(sc, sim.world(), &ids));
                sim.set_police_car(load_car(data, POLICE_CAR)?);
                if sc.car_mix {
                    sim.set_police_cars(car_mix_police(data)?);
                }
            } else {
                let quiet = Scenario {
                    police_presence: Some(0.0),
                    ..sc.clone()
                };
                sim.set_police(police_for_city(&quiet, sim.world(), &ids));
            }
            if sc.traffic_cars {
                if sc.car_mix {
                    sim.set_traffic_cars(car_mix_traffic(data)?);
                } else {
                    sim.set_traffic_car(load_car(data, TRAFFIC_CAR)?);
                }
            }
            true
        }
        None => false,
    };
    // Parked cars dent with the traffic car's lattice (the testbed draws them with its mesh).
    for p in &sc.parked {
        sim.add_parked_car(p.pos, p.yaw);
    }
    if sim.prop_count() > 0 {
        sim.set_parked_car(load_car(data, PARKED_CAR)?);
    }
    // Weather after the city (the wetness grid covers its road network).
    if let Some(w) = &sc.weather {
        sim.set_weather(w.config(sc.seed));
        if let Some(ws) = sim.weather_mut() {
            for p in &w.puddles {
                ws.flood_rect(p.min, p.max, p.water_mm);
            }
        }
    }
    // Hilly cities: every entity sits on the ground under its (x, y) (z = a hint for which
    // level at grade separations), pitched to the slope. Flat cities keep `pos` exactly.
    if sim.has_terrain() {
        for (spec, id) in sc.entities.iter().zip(&ids) {
            sim.place_on_ground(
                *id,
                [spec.pos[0], spec.pos[1]],
                spec.yaw,
                spec.speed_mps,
                Some(spec.pos[2]),
            )?;
        }
    }
    let mut routes = Vec::with_capacity(ids.len());
    for (spec, id) in sc.entities.iter().zip(&ids) {
        let Driver::FollowRoute {
            route_id,
            target_speed,
            lateral_offset_m,
            keep_distance,
        } = &spec.driver
        else {
            routes.push(None);
            continue;
        };
        if !has_city {
            routes.push(None);
            continue;
        }
        let bad = |m: String| RunError::Scenario(sc.name.clone(), m);
        // Other route-id styles (e.g. opponent circuits, workstream N) are not resolved here:
        // the entity keeps its input keyframes, as before.
        let Some((a, b)) = route_id.split_once("->") else {
            routes.push(None);
            continue;
        };
        let poi = |name: &str| {
            sim.world()
                .network()
                .pois
                .iter()
                .position(|p| p.name == name.trim())
                .ok_or_else(|| {
                    bad(format!(
                        "route_id {route_id:?}: no POI named {:?}",
                        name.trim()
                    ))
                })
        };
        let (pa, pb) = (poi(a)?, poi(b)?);
        let route = sim
            .world()
            .plan_route_between_pois(pa, pb)
            .ok_or_else(|| bad(format!("route_id {route_id:?}: no route")))?;
        let path = wp_world::drive::DrivePath::from_route(&route);
        let (p0, d0) = path.point_at(0.0);
        let yaw = wp_world::geom::yaw_of(d0);
        sim.place_on_ground(*id, [p0[0], p0[1]], yaw, spec.speed_mps, Some(p0[2]))?;
        let follower = wp_world::drive::PathFollower::for_vehicle(
            &modded.get(id).unwrap_or(&cars[&spec.car]).def,
            wp_world::drive::FollowParams::PURSUIT,
            spec.speed_mps,
        );
        routes.push(Some(RouteDriver {
            path,
            follower,
            target_mps: *target_speed,
            lateral_offset_m: *lateral_offset_m,
            keep_distance: *keep_distance,
            progress_m: 0.0,
        }));
    }
    Ok(BuiltSim {
        sim,
        ids,
        cars,
        modded,
        selection,
        warnings,
        has_city,
        routes,
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
        OpKind::Repair => sim.repair_vehicle(id)?,
        OpKind::ResetAt {
            xy,
            yaw,
            near_z,
            repair,
        } => {
            sim.place_on_ground(id, xy, yaw, 0.0, Some(near_z))?;
            if repair {
                sim.repair_vehicle(id)?;
            }
        }
        OpKind::Ghost { ticks } => sim.set_ghost(id, u32::from(ticks))?,
    }
    Ok(())
}

/// Vehicles a `keep_distance` route driver (scenario entity `idx`) keeps its distance from:
/// civilian traffic and the other scenario entities (ids offset by 1 << 24 so they never tie
/// with agent ids).
fn route_obstacles(
    sim: &Sim,
    states: &[(EntityId, wp_contracts::VehicleState)],
    idx: usize,
) -> Vec<wp_world::leader::Obstacle> {
    let mut v: Vec<wp_world::leader::Obstacle> = sim
        .world()
        .agents()
        .map(|a| wp_world::leader::Obstacle::from_agent(&a))
        .collect();
    for (k, (id, st)) in states.iter().enumerate() {
        if k != idx {
            v.push(wp_world::leader::Obstacle::from_state((1 << 24) + id.0, st));
        }
    }
    v
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
    /// Time with any ground-contact wheel's |slip ratio| above 0.2, s (launch wheelspin,
    /// braking lock-ups).
    pub wheelspin_s: f32,
    /// Largest |heading change| from the first tick, degrees (a straight launch stays < 3).
    pub max_heading_change_deg: f32,
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
    /// Panels that unlatched (or went straight to hanging) at some point (scenario `panels`).
    pub panels_unlatched: u32,
    /// Panels that hung by one hinge or tore off at some point.
    pub panels_hanging: u32,
    /// Largest |hinge angle| of a free panel, rad.
    pub max_panel_angle_rad: f32,
    /// Time spent off every road and junction (city runs), s.
    #[serde(default)]
    pub off_road_s: f32,
    /// Longest stretch with all four wheels off the ground, s.
    #[serde(default)]
    pub max_airborne_s: f32,
    /// Fraction of its `follow_route` route the entity covered (0 without one).
    #[serde(default)]
    pub route_progress: f32,
    /// Worst spike-strip deflation of any tyre, 0 (inflated) ..= 1 (flat).
    #[serde(default)]
    pub max_spike_puncture: f32,
    /// Tyres deflated by spikes to at least half (0..=4), at the end of the run.
    #[serde(default)]
    pub spiked_wheels: u32,
    /// Longest stretch of a pursuit with no police line of sight on the entity
    /// (`wp_police` `RacerRecord::los_lost_s`), s. 0 without police or pursuit.
    #[serde(default)]
    pub max_los_lost_s: f32,
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
            "wheelspin_s" => self.wheelspin_s,
            "max_heading_change_deg" => self.max_heading_change_deg,
            "crush_depth_m" => self.crush_depth_m,
            "components_failed" => self.components_failed as f32,
            "parts_detached" => self.parts_detached as f32,
            "wheels_detached" => self.wheels_detached as f32,
            "total_loss" => self.total_loss as u8 as f32,
            "min_z_m" => self.min_z_m,
            "panels_unlatched" => self.panels_unlatched as f32,
            "panels_hanging" => self.panels_hanging as f32,
            "max_panel_angle_rad" => self.max_panel_angle_rad,
            "off_road_s" => self.off_road_s,
            "max_airborne_s" => self.max_airborne_s,
            "route_progress" => self.route_progress,
            "max_spike_puncture" => self.max_spike_puncture,
            "spiked_wheels" => self.spiked_wheels as f32,
            "max_los_lost_s" => self.max_los_lost_s,
            _ => return None,
        })
    }

    /// Every metric name [`EntitySummary::metric`] accepts.
    pub const METRICS: [&'static str; 25] = [
        "final_speed_mps",
        "max_speed_mps",
        "distance_m",
        "path_length_m",
        "final_health",
        "max_lateral_g",
        "max_long_g",
        "max_yaw_rate_rps",
        "wheelspin_s",
        "max_heading_change_deg",
        "crush_depth_m",
        "components_failed",
        "parts_detached",
        "wheels_detached",
        "total_loss",
        "min_z_m",
        "panels_unlatched",
        "panels_hanging",
        "max_panel_angle_rad",
        "off_road_s",
        "max_airborne_s",
        "route_progress",
        "max_spike_puncture",
        "spiked_wheels",
        "max_los_lost_s",
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

/// Harness-side net statistics (see [`crate::net`]).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct NetStats {
    /// Snapshots round-tripped.
    pub snapshots: u64,
    /// Total encoded bytes.
    pub bytes: u64,
    /// Entity updates sent as deltas against a baseline.
    pub deltas: u64,
    /// Worst dequantised position error, m.
    pub max_pos_err_m: f32,
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
    /// Civilians that got around a stationary vehicle: oncoming-lane passes plus lane changes
    /// made for it (`wp_world` blocked-vehicle logic, playtest row 32).
    pub traffic_go_arounds: u64,
    /// Civilians that turned around (U-turn / k-point turn, finished) or reversed back to the
    /// last junction because their lane was blocked.
    pub traffic_turnarounds: u64,
    /// Civilians that pulled over for a siren unit coming up behind them
    /// (`TrafficStats::siren_yields`, playtest row 62).
    pub traffic_siren_yields: u64,
    /// Longest a civilian stood still outside a red / amber light, s
    /// (`TrafficStats::max_stop_s`, owner row 85).
    pub traffic_max_stop_s: f32,
    /// Longest queue of stopped civilians back from a stop line (`TrafficStats::max_queue`).
    pub traffic_max_queue: u32,
    /// Times a civilian stood > 5 s inside a junction box (`TrafficStats::box_stops`).
    pub traffic_box_stops: u64,
    /// Unseen civilians recycled after standing still 30 s outside a red light
    /// (`TrafficStats::jam_recycled`).
    pub traffic_jam_recycled: u64,
    /// City props turned into physics bodies (parked cars, cones).
    pub props: u32,
    /// Static boxes from `barriers`.
    pub static_boxes: u32,
    /// Most police units alive at once.
    pub police_units_max: u32,
    /// Police units alive at the end.
    pub police_units_final: u32,
    /// Most physical roadblock barriers at once.
    pub roadblock_barriers_max: u32,
    /// Most spike strips on the road at once.
    pub spike_strips_max: u32,
    /// Farthest any roadblock barrier was pushed from where it was placed, m (shows a hit).
    pub barrier_displaced_max_m: f32,
    /// Parked cars hit hard enough to dent (promoted to a damage lattice).
    pub parked_dented: u32,
    /// Deepest dent on any parked car at the end (`DamageModel::structural_crush_m`, as the
    /// per-entity `crush_depth_m`), m.
    pub parked_crush_max_m: f32,
    /// Lowest health of any dented parked car at the end (1 when none was dented).
    pub parked_health_min: f32,
    /// Glass regions shattered on parked cars.
    pub parked_glass_shattered: u32,
    /// Parts torn off parked cars.
    pub parked_parts_detached: u32,
    /// Farthest any parked car was shoved from where it was parked, m.
    pub parked_moved_max_m: f32,
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
            "traffic_go_arounds" => self.traffic_go_arounds as f32,
            "traffic_turnarounds" => self.traffic_turnarounds as f32,
            "traffic_siren_yields" => self.traffic_siren_yields as f32,
            "traffic_max_stop_s" => self.traffic_max_stop_s,
            "traffic_max_queue" => self.traffic_max_queue as f32,
            "traffic_box_stops" => self.traffic_box_stops as f32,
            "traffic_jam_recycled" => self.traffic_jam_recycled as f32,
            "props" => self.props as f32,
            "static_boxes" => self.static_boxes as f32,
            "police_units_max" => self.police_units_max as f32,
            "police_units_final" => self.police_units_final as f32,
            "roadblock_barriers_max" => self.roadblock_barriers_max as f32,
            "spike_strips_max" => self.spike_strips_max as f32,
            "barrier_displaced_max_m" => self.barrier_displaced_max_m,
            "parked_dented" => self.parked_dented as f32,
            "parked_crush_max_m" => self.parked_crush_max_m,
            "parked_health_min" => self.parked_health_min,
            "parked_glass_shattered" => self.parked_glass_shattered as f32,
            "parked_parts_detached" => self.parked_parts_detached as f32,
            "parked_moved_max_m" => self.parked_moved_max_m,
            _ => return None,
        })
    }

    /// Every metric name [`WorldSummary::metric`] accepts.
    pub const METRICS: [&'static str; 27] = [
        "traffic_agents_final",
        "traffic_agents_min",
        "traffic_moving_frac_final",
        "traffic_moving_frac_min",
        "civilian_collisions",
        "traffic_spawned",
        "traffic_stuck_recycled",
        "traffic_go_arounds",
        "traffic_turnarounds",
        "traffic_siren_yields",
        "traffic_max_stop_s",
        "traffic_max_queue",
        "traffic_box_stops",
        "traffic_jam_recycled",
        "props",
        "static_boxes",
        "police_units_max",
        "police_units_final",
        "roadblock_barriers_max",
        "spike_strips_max",
        "barrier_displaced_max_m",
        "parked_dented",
        "parked_crush_max_m",
        "parked_health_min",
        "parked_glass_shattered",
        "parked_parts_detached",
        "parked_moved_max_m",
    ];
}

/// Seconds at the start of a run that the `*_min` traffic metrics ignore (spawn-in).
pub const WORLD_WARMUP_S: f32 = 10.0;

#[derive(Default)]
struct WorldTracker {
    agents_min: Option<u32>,
    moving_min: Option<f32>,
    police_max: u32,
    barriers_max: u32,
    spikes_max: u32,
    /// First-seen position of every barrier: `(tool, index within tool) -> pos`.
    barrier_start: BTreeMap<(u32, u32), [f32; 3]>,
    barrier_displaced_max: f32,
    /// Where each parked car stood at the first tick (filled lazily, parked props only).
    parked_start: Vec<[f32; 3]>,
    parked_moved_max: f32,
}

fn moving_frac(world: &wp_world::World) -> f32 {
    let n = world.agent_count();
    if n == 0 {
        return 0.0;
    }
    world.agents().filter(|a| a.speed_mps > 0.5).count() as f32 / n as f32
}

impl WorldTracker {
    fn police_tools(&mut self, sim: &Sim) {
        let barriers = sim.roadblock_barriers();
        self.barriers_max = self.barriers_max.max(barriers.len() as u32);
        self.spikes_max = self.spikes_max.max(sim.spike_strips().len() as u32);
        let mut index: BTreeMap<u32, u32> = BTreeMap::new();
        for b in &barriers {
            let i = index.entry(b.tool).or_insert(0);
            let start = *self.barrier_start.entry((b.tool, *i)).or_insert(b.pos);
            *i += 1;
            self.barrier_displaced_max = self.barrier_displaced_max.max(dist(start, b.pos));
        }
    }

    /// Farthest shove of any parked car: checks only the props that are dynamic now (the
    /// others have not moved since they last were).
    fn parked(&mut self, sim: &Sim) {
        if self.parked_start.len() != sim.prop_count() {
            self.parked_start = (0..sim.prop_count())
                .map(|i| sim.prop_pose(i).map_or([0.0; 3], |p| p.0))
                .collect();
        }
        for i in sim.active_props() {
            if sim.prop_kind(i) != Some(wp_world::props::PropKind::ParkedCar) {
                continue;
            }
            if let Some((p, _)) = sim.prop_pose(i) {
                self.parked_moved_max = self.parked_moved_max.max(dist(self.parked_start[i], p));
            }
        }
    }

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

    fn finish(&self, sim: &Sim, city: &CitySpec) -> WorldSummary {
        let world = sim.world();
        let stats = world.traffic_stats();
        let n = world.agent_count() as u32;
        let frac = moving_frac(world);
        WorldSummary {
            city: match city {
                CitySpec::File(f) => f.clone(),
                other => other.city_id(),
            },
            traffic_agents_final: n,
            traffic_agents_min: self.agents_min.unwrap_or(n),
            traffic_moving_frac_final: frac,
            traffic_moving_frac_min: self.moving_min.unwrap_or(frac),
            civilian_collisions: stats.civilian_collisions,
            traffic_spawned: stats.spawned,
            traffic_stuck_recycled: stats.stuck_recycled,
            traffic_go_arounds: stats.oncoming_passes + stats.go_around_lane_changes,
            traffic_turnarounds: stats.turnarounds_completed + stats.reverses,
            traffic_siren_yields: stats.siren_yields,
            traffic_max_stop_s: stats.max_stop_s,
            traffic_max_queue: stats.max_queue,
            traffic_box_stops: stats.box_stops,
            traffic_jam_recycled: stats.jam_recycled,
            props: sim.prop_count() as u32,
            static_boxes: sim.static_box_count() as u32,
            police_units_max: self.police_max,
            police_units_final: sim.police().units().len() as u32,
            roadblock_barriers_max: self.barriers_max,
            spike_strips_max: self.spikes_max,
            barrier_displaced_max_m: self.barrier_displaced_max,
            parked_dented: sim.parked_wrecks().len() as u32,
            parked_crush_max_m: sim
                .parked_wrecks()
                .iter()
                .map(|w| w.damage.structural_crush_m())
                .fold(0.0, f32::max),
            parked_health_min: sim
                .parked_wrecks()
                .iter()
                .map(|w| w.damage.health())
                .fold(1.0, f32::min),
            parked_glass_shattered: sim.parked_stats().glass_shattered,
            parked_parts_detached: sim.parked_stats().parts_detached,
            parked_moved_max_m: self.parked_moved_max,
        }
    }
}

/// Issue the scenario's `police_orders` due at `tick` (before the tick is stepped; `ids` in
/// scenario order). Shared by [`run`] and the browser testbed so both behave identically,
/// including during replays: orders are scenario setup, not inputs.
pub fn apply_police_orders(
    sc: &Scenario,
    sim: &mut Sim,
    ids: &[EntityId],
    tick: u64,
) -> Result<Vec<OrderResult>, RunError> {
    let mut out = Vec::new();
    for o in sc.police_orders.iter().filter(|o| tick_of(o.t_s) == tick) {
        let idx = sc
            .entity_index(Some(o.order.target()))
            .map_err(|e| RunError::Scenario(sc.name.clone(), e))?;
        let target = ids[idx];
        let result = match &o.order {
            OrderKind::Roadblock { .. } => {
                sim.captain_command(wp_contracts::CaptainCommand::RequestRoadblock {
                    target,
                    road: None,
                })
            }
            OrderKind::Spikes { .. } => {
                sim.captain_command(wp_contracts::CaptainCommand::RequestSpikes {
                    target,
                    road: None,
                })
            }
            OrderKind::RoadblockAt { pos, dir, .. } => {
                sim.police_mut().place_roadblock_at(target, *pos, *dir);
                Ok(())
            }
            OrderKind::SpikesAt { pos, dir, .. } => {
                sim.police_mut().place_spikes_at(target, *pos, *dir);
                Ok(())
            }
        };
        out.push(OrderResult {
            t_s: o.t_s,
            order: crate::scenario::to_ron_line(&o.order),
            result: match result {
                Ok(()) => "ok".into(),
                Err(e) => format!("{e:?}"),
            },
        });
    }
    Ok(out)
}

/// One police order and its outcome.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OrderResult {
    /// When it was given, s.
    pub t_s: f32,
    /// The order, as RON.
    pub order: String,
    /// `"ok"`, or `wp_police`'s refusal (`NotPursued`, `Locked { need_level: 3 }`, …).
    pub result: String,
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
    /// `police_orders` and what `wp_police` answered (`"ok"` or the refusal).
    pub police_orders: Vec<OrderResult>,
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
        // v0.3.0 typed variants keep their v0.2.0 names so scenario event counts stay valid.
        SimEvent::Police(_) => "PoliceEvent",
        SimEvent::Economy(_) => "EconomyEvent",
        SimEvent::Race(_) => "RaceEvent",
        SimEvent::WeatherChanged { .. } => "WeatherChanged",
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
        SimEvent::Police(_)
        | SimEvent::Economy(_)
        | SimEvent::Race(_)
        | SimEvent::WeatherChanged { .. } => Vec::new(),
    }
}

/// Whether `p` is on a road: within a lane or junction connector (plus 1 m) at about its
/// height, or inside a junction box.
fn on_road(map: &wp_world::CityMap, p: [f32; 3]) -> bool {
    map.locate(p, None, 1.0, false, true).is_some()
        || map.intersection_at([p[0], p[1]]).is_some_and(|ix| {
            let n = map.net.intersections[ix as usize].node;
            (map.net.nodes[n as usize].pos[2] - p[2]).abs() < 3.0
        })
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
///
/// This is the raw any-node max: shattered glass and bumper covers count. Scenario
/// `crush_depth` checks use `DamageModel::structural_crush_m()` instead; this helper stays for
/// tools that only have node positions (e.g. the tuning-dash probe).
pub fn crush_depth(rest: &[[f32; 3]], now: &[[f32; 3]]) -> f32 {
    rest.iter()
        .zip(now)
        .map(|(r, n)| dist(*r, *n))
        .fold(0.0, f32::max)
}

struct Tracker {
    /// Current all-wheels-off-the-ground stretch, s.
    airborne_s: f32,
    spawn: [f32; 3],
    vel_window: VecDeque<[f32; 3]>,
    last_pos: [f32; 3],
    lat_g: f32,
    long_g: f32,
    /// Heading at the first tick, rad (`None` before it).
    heading0: Option<f32>,
    summary: EntitySummary,
    failed: BTreeSet<Component>,
}

impl Tracker {
    fn update(&mut self, s: &VehicleState) -> ([f32; 3], f32, f32) {
        let e = &mut self.summary;
        e.max_speed_mps = e.max_speed_mps.max(s.speed_mps);
        e.max_yaw_rate_rps = e.max_yaw_rate_rps.max(s.ang_vel[2].abs());
        if s.wheel_states
            .iter()
            .any(|w| w.contact && w.slip_ratio.abs() > 0.2)
        {
            e.wheelspin_s += PHYSICS_DT;
        }
        // Report-only metric (never fed back into the sim), so plain atan2 is fine here.
        let fwd = rotate(s.rot, [0.0, 1.0, 0.0]);
        let heading = fwd[0].atan2(fwd[1]);
        let h0 = *self.heading0.get_or_insert(heading);
        let mut d = (heading - h0).to_degrees();
        if d > 180.0 {
            d -= 360.0;
        } else if d < -180.0 {
            d += 360.0;
        }
        e.max_heading_change_deg = e.max_heading_change_deg.max(d.abs());
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
        modded: _,
        selection,
        warnings,
        has_city,
        mut routes,
    } = build_sim(sc, data, opts)?;
    let start_states = scenario_states(&sim, &ids);
    let mut trackers = Vec::new();
    for ((spec, id), (_, st0)) in sc.entities.iter().zip(&ids).zip(&start_states) {
        // Route drivers start where their route starts, and cars in hilly cities on the
        // ground under `pos`.
        let start = if matches!(spec.driver, Driver::FollowRoute { .. }) || sim.has_terrain() {
            st0.pos
        } else {
            spec.pos
        };
        trackers.push(Tracker {
            airborne_s: 0.0,
            spawn: start,
            vel_window: VecDeque::new(),
            last_pos: start,
            lat_g: 0.0,
            long_g: 0.0,
            heading0: None,
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
                wheelspin_s: 0.0,
                max_heading_change_deg: 0.0,
                crush_depth_m: 0.0,
                components_failed: 0,
                parts_detached: 0,
                wheels_detached: 0,
                total_loss: false,
                min_z_m: start[2],
                panels_unlatched: 0,
                panels_hanging: 0,
                max_panel_angle_rad: 0.0,
                off_road_s: 0.0,
                max_airborne_s: 0.0,
                route_progress: 0.0,
                max_spike_puncture: 0.0,
                spiked_wheels: 0,
                max_los_lost_s: 0.0,
            },
        });
    }
    let names: Vec<String> = sc.entities.iter().map(|e| e.id.clone()).collect();
    let mut world_tr = WorldTracker::default();
    let mut order_results: Vec<OrderResult> = Vec::new();
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
    // Probe ticks for `travel_between`: path length at each end.
    let mut travel: BTreeMap<(usize, u64), f32> = BTreeMap::new();
    for x in sc.expect.iter().chain(&sc.expect_pending) {
        if let Expect::TravelBetween {
            entity,
            from_s,
            to_s,
            ..
        } = x
        {
            let idx = sc
                .entity_index(entity.as_ref())
                .map_err(|e| RunError::Scenario(sc.name.clone(), e))?;
            travel.insert((idx, tick_of(*from_s)), f32::NAN);
            travel.insert((idx, tick_of(*to_s)), f32::NAN);
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
    // Captured route drivers to restart next tick (`restart_after_capture`).
    let mut restarts: Vec<EntityId> = Vec::new();
    // (tick, racer, outcome) of every PursuitEnded, in order.
    let mut pursuit_ends: Vec<(u64, EntityId, wp_contracts::PursuitOutcome)> = Vec::new();
    let mut event_counts: Vec<(&'static str, Vec<EntityId>)> = Vec::new();
    let mut net_rt = crate::net::NetRoundTrip::default();
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
        // Police captain orders.
        order_results.extend(apply_police_orders(sc, &mut sim, &ids, tick)?);

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
            // `restart_after_capture`: a captured route driver starts its run again.
            for racer in std::mem::take(&mut restarts) {
                let Some(idx) = ids.iter().position(|i| *i == racer) else {
                    continue;
                };
                let spec = &sc.entities[idx];
                if let Some(route) = routes[idx].as_mut() {
                    let (p0, d0) = route.path.point_at(0.0);
                    let yaw = wp_world::geom::yaw_of(d0);
                    sim.place_on_ground(racer, [p0[0], p0[1]], yaw, spec.speed_mps, Some(p0[2]))?;
                    route.follower = wp_world::drive::PathFollower::for_vehicle(
                        &cars[&spec.car].def,
                        wp_world::drive::FollowParams::PURSUIT,
                        spec.speed_mps,
                    );
                    route.progress_m = 0.0;
                    sim.police_mut().set_heat(racer, spec.police_heat);
                }
            }
            let states = scenario_states(&sim, &ids);
            for (idx, id) in ids.iter().enumerate() {
                let input = match routes[idx].as_mut() {
                    Some(route) => {
                        let obstacles = if route.keep_distance {
                            route_obstacles(&sim, &states, idx)
                        } else {
                            Vec::new()
                        };
                        let mut input = route.input(&states[idx].1, &obstacles);
                        // Lights, siren and horn still come from the keyframes.
                        let key = driver_input(sc, idx, tick, &states[idx].1, &last_inputs[idx]);
                        input.siren = key.siren;
                        input.headlights = key.headlights;
                        input.horn = key.horn;
                        input
                    }
                    None => driver_input(sc, idx, tick, &states[idx].1, &last_inputs[idx]),
                };
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
                SimEvent::Police(p) => {
                    police_first.entry(p.kind()).or_insert(now);
                    if let wp_contracts::PoliceEvent::PursuitEnded { racer, outcome, .. } = p {
                        pursuit_ends.push((now, *racer, *outcome));
                        if sc.restart_after_capture
                            && matches!(
                                outcome,
                                wp_contracts::PursuitOutcome::Captured
                                    | wp_contracts::PursuitOutcome::CapturedTotalLoss
                            )
                        {
                            restarts.push(*racer);
                        }
                    }
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
        let states = scenario_states(&sim, &ids);
        let sample = now.is_multiple_of(sample_every);
        for (idx, (id, s)) in states.iter().enumerate() {
            let tr = &mut trackers[idx];
            tr.update(s);
            let spikes = sim.spike_puncture(*id);
            tr.summary.max_spike_puncture = spikes
                .iter()
                .fold(tr.summary.max_spike_puncture, |m, p| m.max(*p));
            if let Some(r) = sim.police().racer(*id) {
                tr.summary.max_los_lost_s = tr.summary.max_los_lost_s.max(r.los_lost_s);
            }
            if s.wheel_states.iter().all(|w| !w.contact) {
                tr.airborne_s += PHYSICS_DT;
                tr.summary.max_airborne_s = tr.summary.max_airborne_s.max(tr.airborne_s);
            } else {
                tr.airborne_s = 0.0;
            }
            if has_city && !on_road(sim.world().map(), s.pos) {
                tr.summary.off_road_s += PHYSICS_DT;
            }
            if let Some(slot) = probes.get_mut(&(idx, now)) {
                *slot = s.speed_mps;
            }
            if let Some(slot) = travel.get_mut(&(idx, now)) {
                *slot = tr.summary.path_length_m;
            }
            if sc.panels {
                let panels = sim.vehicles()[idx].damage.panel_states();
                let free = panels
                    .iter()
                    .filter(|p| p.state != wp_damage::PanelState::Attached)
                    .count() as u32;
                let hanging = panels
                    .iter()
                    .filter(|p| p.state >= wp_damage::PanelState::HangingByOne)
                    .count() as u32;
                let e = &mut tr.summary;
                e.panels_unlatched = e.panels_unlatched.max(free);
                e.panels_hanging = e.panels_hanging.max(hanging);
                for p in &panels {
                    if p.state != wp_damage::PanelState::Detached {
                        e.max_panel_angle_rad = e.max_panel_angle_rad.max(p.angle_rad.abs());
                    }
                }
            }
            if sample || now == ticks {
                // Structural crush (no glass / bumper covers / detached nodes), per B.
                let crush = sim
                    .vehicles()
                    .iter()
                    .find(|v| v.id == *id)
                    .map_or(0.0, |v| v.damage.structural_crush_m());
                tr.summary.crush_depth_m = tr.summary.crush_depth_m.max(crush);
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
            world_tr.police_max = world_tr.police_max.max(sim.police().units().len() as u32);
        }
        // Police tools exist on any world (`roadblock_at` / `spikes_at` work on the flat one).
        world_tr.police_tools(&sim);
        world_tr.parked(&sim);

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

        // Net round trip on wp_net's wire codec.
        if selection.net && now.is_multiple_of(PHYSICS_TICKS_PER_NET_TICK as u64) {
            if let Err(e) = net_rt.snapshot(now, &states) {
                failures.push(format!("net: snapshot at tick {now}: {e}"));
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
    let states = scenario_states(&sim, &ids);
    let mut entities = Vec::new();
    for (i, (tr, (id, s))) in trackers.iter_mut().zip(&states).enumerate() {
        tr.summary.final_speed_mps = s.speed_mps;
        tr.summary.spiked_wheels = sim
            .spike_puncture(*id)
            .iter()
            .filter(|p| **p >= 0.5)
            .count() as u32;
        tr.summary.final_health = s.health;
        tr.summary.distance_m = dist([s.pos[0], s.pos[1], 0.0], [tr.spawn[0], tr.spawn[1], 0.0]);
        tr.summary.components_failed = tr.failed.len() as u32;
        let mut summary = tr.summary.clone();
        if let Some(Some(route)) = routes.get(i) {
            summary.route_progress = route.progress();
        }
        entities.push(summary);
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
        net: NetStats {
            snapshots: net_rt.snapshots,
            bytes: net_rt.bytes,
            deltas: net_rt.deltas,
            max_pos_err_m: net_rt.max_pos_err_m,
        },
        world: world_tr.finish(&sim, &sc.city),
        warnings,
        police_orders: order_results,
        expectations: Vec::new(),
        pending: Vec::new(),
        failures,
    };

    let ctx = EvalCtx {
        sc,
        summary: &summary,
        probes: &probes,
        travel: &travel,
        police_first: &police_first,
        pursuit_ends: &pursuit_ends,
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
    travel: &'a BTreeMap<(usize, u64), f32>,
    police_first: &'a BTreeMap<u16, u64>,
    pursuit_ends: &'a [(u64, EntityId, wp_contracts::PursuitOutcome)],
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
            Expect::TravelBetween {
                entity,
                from_s,
                to_s,
                min,
                max,
            } => {
                let (idx, _) = self.ent(entity.as_ref());
                let at = |t: f32| {
                    self.travel
                        .get(&(idx, tick_of(t)))
                        .copied()
                        .filter(|v| !v.is_nan())
                };
                match (at(*from_s), at(*to_s)) {
                    (Some(a), Some(b)) => range(b - a, *min, *max),
                    _ => (format!("{from_s}..{to_s} s is outside the run"), false),
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
            Expect::FirstPursuitEnd {
                entity,
                outcome,
                min_s,
                max_s,
            } => {
                let id = self.ids[self.ent(entity.as_ref()).0];
                match self.pursuit_ends.iter().find(|(_, r, _)| *r == id) {
                    Some((tick, _, o)) => {
                        let t = *tick as f32 * PHYSICS_DT;
                        let name = format!("{o:?}");
                        let in_time = t >= *min_s && max_s.is_none_or(|m| t <= m);
                        (format!("{name} at {t:.3} s"), name == *outcome && in_time)
                    }
                    None => ("no pursuit ended".into(), outcome == "None"),
                }
            }
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
