//! # wp_police
//!
//! Heat/bounty, dispatch, day/night presence, patrol & pursuit AI, police tools and the captain
//! interface (owned by workstream 06).
//!
//! Design pillars: chases are slow, calculated and lethal to both sides. Police win by
//! containment and attrition (shadowing, box-ins, rolling blocks, spikes and roadblocks ahead of
//! the predicted path, a helicopter keeping visual), racers win by breaking line of sight,
//! reaching a safehouse or disabling pursuers. Day vs night differences are **tables**
//! (`data/police/*.toml`, keyed by `DayPhase`), never code branches.
//!
//! Entry point: [`PoliceSystem::step`], called once per physics tick by `wp_sim`.
//!
//! Module map:
//! - [`config`]: the TOML tables.
//! - [`graph`]: road graph, line of sight, sight distance, routing, the procedural test city.
//! - [`heat`]: heat, wanted levels, bounty, escape/capture/safehouse rules.
//! - [`dispatch`]: patrol pool, incidents, reinforcements, helicopter, AI captain.
//! - [`pursuit`]: tactics (shadow, box-in, PIT, rolling block, intercept, disengage) and driving.
//! - [`unit`]: unit state and the built-in kinematic driver.
//! - [`tools`]: roadblocks, spike strips, radar detector and scanner queries.
//! - [`captain`]: captain commands, validation and the captain view.
//! - [`events`]: the `SimEvent::PoliceEvent` payload.
//! - [`scenario`]: headless scripted scenarios on the procedural city.

#![warn(missing_docs)]

pub mod avoid;
pub mod captain;
pub mod city;
pub mod config;
pub mod dispatch;
pub mod events;
pub mod graph;
pub mod heat;
pub mod pursuit;
pub mod scenario;
pub mod time;
pub mod tools;
pub mod unit;

use std::collections::{BTreeMap, VecDeque};

use serde::{Deserialize, Serialize};
use wp_contracts::{
    DamageMode, DayPhase, EntityId, HandlingEffects, PlayerId, SimEvent, VehicleInput, WorldQuery,
};
use wp_math::{Rng, StateHasher};

pub use avoid::ObstacleView;
pub use captain::{CaptainCommand, CaptainError, CaptainView, RoadEdgeId, UnitOrder};
pub use config::PoliceConfig;
pub use dispatch::{Incident, IncidentKind};
pub use events::{DisableCause, IncidentId, PoliceEvent, PursuitOutcome, ToolId, UnitId, UnitKind};
pub use graph::{District, RoadGraph};
pub use heat::{BountyRecord, PursuitStatus, RacerRecord, WantedLevel};
pub use time::SimTime;
pub use tools::{HeloView, RadarBlip, RadioLine, Roadblock, SpikeStrip, StaticCollider};
pub use unit::{Assignment, DriveCommand, SteerGeometry, Tactic, Unit};

use graph::xy;
use tools::Puncture;
use unit::{UnitBody, UnitRole};

/// Height of a driver's eyes / a car's roof for line-of-sight queries, m.
pub const EYE_HEIGHT_M: f32 = 1.3;

/// What the police system sees of a racer each tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RacerView {
    /// Racer's vehicle.
    pub entity: EntityId,
    /// World position, m.
    pub pos: [f32; 3],
    /// World velocity, m/s.
    pub vel: [f32; 3],
}

/// Instance-level options.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PoliceSetup {
    /// Seed for the police RNG stream.
    pub seed: u64,
    /// Damage mode (PIT speed limits, crash severity).
    pub mode: DamageMode,
    /// Drive AI units with the built-in kinematic driver (true until `wp_sim` gives units
    /// physical bodies and calls [`PoliceSystem::sync_unit`]).
    pub internal_driving: bool,
    /// Simulate background traffic hazards for internally driven units.
    pub hazards: bool,
}

impl Default for PoliceSetup {
    fn default() -> Self {
        Self {
            seed: 0x0090_11CE,
            mode: DamageMode::Arcade,
            internal_driving: true,
            hazards: true,
        }
    }
}

/// External body update for a unit driven by `wp_sim` (player or physical AI car).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnitSync {
    /// World position, m.
    pub pos: [f32; 3],
    /// World velocity, m/s.
    pub vel: [f32; 3],
    /// Heading (XY unit vector).
    pub heading: [f32; 2],
    /// Vehicle health 0..=1.
    pub health: f32,
    /// Current handling effects (a cop with a bent wheel pulls over).
    pub effects: HandlingEffects,
}

/// One pursuit, for statistics.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PursuitRecord {
    /// Racer.
    pub racer: EntityId,
    /// Start.
    pub start: SimTime,
    /// End.
    pub end: Option<SimTime>,
    /// Outcome.
    pub outcome: Option<PursuitOutcome>,
}

/// Counters for tests, tuning and the README perf/behaviour tables.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PoliceStats {
    /// Every pursuit.
    pub pursuits: Vec<PursuitRecord>,
    /// Reinforcement request → unit committed, s.
    pub reinforcement_delays_s: Vec<f32>,
    /// Units that ever pursued.
    pub units_engaged: u32,
    /// Units total-lost without racer contact.
    pub self_wrecks: u32,
    /// Units total-lost after racer contact.
    pub contact_wrecks: u32,
    /// Units that pulled over (damage / bent wheel).
    pub pulled_over: u32,
    /// Approach sight distance of every tool placed, m.
    pub tool_sight_m: Vec<f32>,
    /// Roadblocks placed.
    pub roadblocks: u32,
    /// Spike strips placed.
    pub spikes: u32,
    /// Spike hits.
    pub spike_hits: u32,
    /// PIT attempts.
    pub pit_attempts: u32,
    /// Helicopter launches.
    pub helo_launches: u32,
    /// Helicopter requests that found it unavailable.
    pub helo_unavailable: u32,
    /// Racer–unit contacts.
    pub racer_contacts: u32,
    /// Built-in driver: traffic near misses.
    pub near_misses: u32,
    /// Built-in driver: traffic collisions.
    pub traffic_collisions: u32,
    /// Built-in driver: ran wide and crashed.
    pub run_wide_crashes: u32,
}

/// Captain bookkeeping (human or AI).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CaptainState {
    pub(crate) human: bool,
    pub(crate) priority: Option<EntityId>,
    pub(crate) roadblock_ready: SimTime,
    pub(crate) spikes_ready: SimTime,
    pub(crate) helo_ready: SimTime,
}

/// Per-think observation of a racer.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(crate) struct RacerObs {
    /// Any unit / helicopter / camera has line of sight.
    pub(crate) seen: bool,
    /// A ground unit or camera within witness range has line of sight.
    pub(crate) witnessed: bool,
    /// Closest ground unit with line of sight within recognition range.
    pub(crate) spotter: Option<UnitId>,
    /// Nearest ground unit distance, m.
    pub(crate) nearest_unit_m: f32,
}

/// Police state for one instance.
#[derive(Debug, Clone)]
pub struct PoliceSystem {
    pub(crate) cfg: PoliceConfig,
    pub(crate) graph: RoadGraph,
    pub(crate) setup: PoliceSetup,
    pub(crate) rng: Rng,
    pub(crate) tick: u64,
    pub(crate) now: SimTime,
    pub(crate) think_accum: f32,
    pub(crate) phase: DayPhase,
    pub(crate) warmed_up: bool,
    pub(crate) racers: BTreeMap<EntityId, RacerRecord>,
    pub(crate) obs: BTreeMap<EntityId, RacerObs>,
    pub(crate) recognition_s: BTreeMap<EntityId, f32>,
    pub(crate) trails: BTreeMap<EntityId, unit::Track>,
    pub(crate) prev_pos: BTreeMap<EntityId, [f32; 2]>,
    pub(crate) units: Vec<Unit>,
    pub(crate) next_unit: u32,
    pub(crate) incidents: Vec<Incident>,
    pub(crate) next_incident: u32,
    pub(crate) reinforcements: Vec<dispatch::ReinforcementRequest>,
    pub(crate) helo_requests: Vec<dispatch::HeloRequest>,
    pub(crate) roadblocks: Vec<Roadblock>,
    pub(crate) spikes: Vec<SpikeStrip>,
    pub(crate) next_tool: u32,
    pub(crate) punctures: BTreeMap<EntityId, Puncture>,
    pub(crate) radio: VecDeque<RadioLine>,
    pub(crate) captain: CaptainState,
    pub(crate) stats: PoliceStats,
    pub(crate) contact_cooldown: BTreeMap<(UnitId, EntityId), SimTime>,
    pub(crate) pending: Vec<PoliceEvent>,
    /// Vehicles reported by `wp_sim` ([`PoliceSystem::set_obstacles`]).
    pub(crate) ext_obstacles: Vec<avoid::Obstacle>,
    /// Whether `wp_sim` reports obstacles (then racers and bound units come from that list).
    pub(crate) ext_obstacles_set: bool,
    /// Everything units avoid this tick (rebuilt by `rebuild_obstacles`).
    pub(crate) obstacles: Vec<avoid::Obstacle>,
}

impl Default for PoliceSystem {
    fn default() -> Self {
        Self::new()
    }
}

impl PoliceSystem {
    /// Police with the built-in tables and **no road network**: heat, wanted levels and the
    /// escape/capture rules work, but no units exist. This is what `wp_sim` uses until
    /// `wp_world` provides a road graph (see [`PoliceSystem::with_city`]).
    pub fn new() -> Self {
        Self::with_city(
            PoliceConfig::builtin(),
            RoadGraph::new(),
            PoliceSetup::default(),
        )
    }

    /// Police on `wp_world`'s city: the road graph is converted with
    /// [`RoadGraph::from_city`]. Pass the same `World` as `world` to [`PoliceSystem::step`] so
    /// line of sight uses its buildings.
    pub fn for_world(cfg: PoliceConfig, world: &wp_world::World, setup: PoliceSetup) -> Self {
        Self::with_city(cfg, RoadGraph::from_city(world.map()), setup)
    }

    /// Police on a road network.
    pub fn with_city(cfg: PoliceConfig, graph: RoadGraph, setup: PoliceSetup) -> Self {
        Self {
            cfg,
            graph,
            setup,
            rng: Rng::new(setup.seed),
            tick: 0,
            now: SimTime::ZERO,
            think_accum: 0.0,
            phase: DayPhase::Day,
            warmed_up: false,
            racers: BTreeMap::new(),
            obs: BTreeMap::new(),
            recognition_s: BTreeMap::new(),
            trails: BTreeMap::new(),
            prev_pos: BTreeMap::new(),
            units: Vec::new(),
            next_unit: 1,
            incidents: Vec::new(),
            next_incident: 1,
            reinforcements: Vec::new(),
            helo_requests: Vec::new(),
            roadblocks: Vec::new(),
            spikes: Vec::new(),
            next_tool: 1,
            punctures: BTreeMap::new(),
            radio: VecDeque::new(),
            captain: CaptainState {
                human: false,
                priority: None,
                roadblock_ready: SimTime::ZERO,
                spikes_ready: SimTime::ZERO,
                helo_ready: SimTime::ZERO,
            },
            stats: PoliceStats::default(),
            contact_cooldown: BTreeMap::new(),
            pending: Vec::new(),
            ext_obstacles: Vec::new(),
            ext_obstacles_set: false,
            obstacles: Vec::new(),
        }
    }

    /// Advance by `dt`: consume this tick's `events` (violations, contacts, total losses),
    /// update heat, dispatch and pursuits, push `SimEvent::PoliceEvent`s into `out`.
    ///
    /// Driving integrates every call; decisions (line of sight, rules, dispatch, tactics) run
    /// at `pursuit.think_hz`.
    pub fn step(
        &mut self,
        world: &dyn WorldQuery,
        racers: &[RacerView],
        events: &[SimEvent],
        dt: f32,
        out: &mut Vec<SimEvent>,
    ) {
        self.tick += 1;
        self.now = self.now.plus(dt);
        self.phase = world.time_of_day().phase();
        self.sync_racers(racers);
        self.process_events(world, events);
        self.rebuild_obstacles();
        self.drive_units(dt);
        self.think_accum += dt;
        if self.think_accum >= 1.0 / self.cfg.pursuit.think_hz {
            let tdt = self.think_accum;
            self.think_accum = 0.0;
            self.think(world, tdt);
        }
        for e in self.pending.drain(..) {
            out.push(e.to_sim_event());
        }
    }

    fn sync_racers(&mut self, racers: &[RacerView]) {
        let now = self.now;
        for r in racers {
            let rec = self
                .racers
                .entry(r.entity)
                .or_insert_with(|| RacerRecord::new(r.entity));
            rec.pos = r.pos;
            rec.vel = r.vel;
            let p = xy(r.pos);
            // Spike strips.
            if let Some(prev) = self.prev_pos.get(&r.entity).copied() {
                for s in &self.spikes {
                    let wheels = s.wheels_crossing(prev, p, 0.8);
                    if wheels.iter().any(|w| *w) {
                        self.punctures.entry(r.entity).or_default().hit(wheels, now);
                        self.stats.spike_hits += 1;
                        self.pending.push(PoliceEvent::SpikeHit {
                            vehicle: r.entity,
                            wheels,
                        });
                    }
                }
            }
            self.prev_pos.insert(r.entity, p);
        }
    }

    fn process_events(&mut self, world: &dyn WorldQuery, events: &[SimEvent]) {
        for ev in events {
            match ev {
                SimEvent::TrafficViolation { vehicle, kind, pos } => {
                    if self.racers.contains_key(vehicle) {
                        self.on_violation(world, *vehicle, kind, xy(*pos));
                    }
                }
                SimEvent::TotalLoss { vehicle } => {
                    if self.racers.contains_key(vehicle) {
                        self.on_racer_total_loss(*vehicle);
                    } else if let Some(i) = self.unit_index_by_entity(*vehicle) {
                        self.units[i].health = 0.0;
                    }
                }
                SimEvent::Contact {
                    a,
                    b,
                    point,
                    rel_speed,
                    ..
                } => self.on_contact(*a, *b, *point, *rel_speed),
                _ => {}
            }
        }
    }

    /// Line of sight between two ground points at eye height: the police graph's own occluders
    /// (procedural test city) and the world's buildings (`WorldQuery::line_of_sight`).
    pub(crate) fn sees(&self, world: &dyn WorldQuery, a: [f32; 2], b: [f32; 2]) -> bool {
        self.graph.line_of_sight(a, b)
            && world.line_of_sight([a[0], a[1], EYE_HEIGHT_M], [b[0], b[1], EYE_HEIGHT_M])
    }

    pub(crate) fn unit_index(&self, id: UnitId) -> Option<usize> {
        self.units.binary_search_by_key(&id, |u| u.id).ok()
    }

    pub(crate) fn unit_index_by_entity(&self, e: EntityId) -> Option<usize> {
        self.units.iter().position(|u| u.entity == Some(e))
    }

    /// Emit a radio line (event + scanner log).
    pub(crate) fn radio(&mut self, unit: Option<UnitId>, text: String) {
        self.radio.push_back(RadioLine {
            time: self.now,
            unit,
            text: text.clone(),
        });
        while self.radio.len() > self.cfg.tools.gadgets.radio_log_len {
            self.radio.pop_front();
        }
        self.pending.push(PoliceEvent::Radio { unit, text });
    }

    // -----------------------------------------------------------------------------------------
    // Public queries
    // -----------------------------------------------------------------------------------------

    /// Current wanted level of a racer.
    pub fn wanted_level(&self, racer: EntityId) -> WantedLevel {
        self.racers.get(&racer).map(|r| r.level).unwrap_or_default()
    }

    /// Full police record of a racer.
    pub fn racer(&self, racer: EntityId) -> Option<&RacerRecord> {
        self.racers.get(&racer)
    }

    /// All units, ascending by id.
    pub fn units(&self) -> &[Unit] {
        &self.units
    }

    /// Open incidents.
    pub fn incidents(&self) -> &[Incident] {
        &self.incidents
    }

    /// The road graph in use.
    pub fn graph(&self) -> &RoadGraph {
        &self.graph
    }

    /// Tuning in use.
    pub fn config(&self) -> &PoliceConfig {
        &self.cfg
    }

    /// Statistics so far.
    pub fn stats(&self) -> &PoliceStats {
        &self.stats
    }

    /// Simulation time seen by the police.
    pub fn now(&self) -> SimTime {
        self.now
    }

    /// Current day phase (as of the last step).
    pub fn phase(&self) -> DayPhase {
        self.phase
    }

    /// Pool units alive per district (the patrol presence the table asks for).
    pub fn pool_count(&self, district: District) -> u32 {
        self.units
            .iter()
            .filter(|u| !u.wrecked && u.role == UnitRole::Pool(district))
            .count() as u32
    }

    /// Tire deflation for a vehicle (merge into `HandlingEffects::tire_puncture`).
    pub fn tire_puncture(&self, vehicle: EntityId) -> [f32; 4] {
        self.punctures
            .get(&vehicle)
            .map(|p| p.level(self.now, self.cfg.tools.spikes.deflate_s))
            .unwrap_or([0.0; 4])
    }

    /// Bounty of a racer.
    pub fn bounty(&self, racer: EntityId) -> f32 {
        self.racers
            .get(&racer)
            .map(|r| r.bounty.bounty(&self.cfg.heat))
            .unwrap_or(0.0)
    }

    /// Persistent bounty ledger (prompt 08 stores this).
    pub fn bounty_record(&self, racer: EntityId) -> BountyRecord {
        self.racers
            .get(&racer)
            .map(|r| r.bounty)
            .unwrap_or_default()
    }

    /// Restore a persisted bounty ledger (call before the racer's first step).
    pub fn set_bounty_record(&mut self, racer: EntityId, rec: BountyRecord) {
        self.racers
            .entry(racer)
            .or_insert_with(|| RacerRecord::new(racer))
            .bounty = rec;
    }

    /// Record a race won; counts toward bounty only while wanted.
    pub fn record_race_won(&mut self, racer: EntityId) {
        if let Some(r) = self.racers.get_mut(&racer) {
            if r.level > WantedLevel::None {
                r.bounty.races_won_wanted += 1;
            }
        }
    }

    /// Race payout / stake multiplier for the current phase (night pays more).
    pub fn payout_scale(&self) -> f32 {
        self.cfg.heat.bounty.payout_scale.get(self.phase)
    }

    /// Set a racer's heat directly (scenarios, debug, persistence).
    pub fn set_heat(&mut self, racer: EntityId, heat: f32) {
        let cfg = self.cfg.heat.clone();
        let r = self
            .racers
            .entry(racer)
            .or_insert_with(|| RacerRecord::new(racer));
        let before = r.level;
        r.heat = 0.0;
        r.add_heat(heat, &cfg);
        if r.status == PursuitStatus::Clear && r.level > WantedLevel::None {
            r.status = PursuitStatus::Wanted;
        }
        let after = r.level;
        // Listeners (economy, HUD) track the level from events, so report the change.
        if after != before {
            self.pending.push(PoliceEvent::WantedLevelChanged {
                racer,
                level: after,
            });
        }
    }

    /// Roadblocks in place.
    pub fn roadblocks(&self) -> &[Roadblock] {
        &self.roadblocks
    }

    /// Spike strips in place (line colliders).
    pub fn spike_strips(&self) -> &[SpikeStrip] {
        &self.spikes
    }

    /// Static colliders for `wp_sim`: roadblock barriers plus parked roadblock units (when
    /// units are not physical bodies yet).
    pub fn static_colliders(&self) -> Vec<StaticCollider> {
        let mut v = Vec::new();
        for rb in &self.roadblocks {
            v.extend(rb.barriers.iter().copied());
            for uid in &rb.units {
                if let Some(i) = self.unit_index(*uid) {
                    let u = &self.units[i];
                    if u.entity.is_none() {
                        v.push(StaticCollider {
                            pos: [u.body.pos[0], u.body.pos[1], 0.7],
                            half_extents_m: [0.95, 2.4, 0.7],
                            forward: u.body.heading,
                            mass_kg: self.cfg.tools.roadblock.unit_mass_kg,
                        });
                    }
                }
            }
        }
        v
    }

    // -----------------------------------------------------------------------------------------
    // wp_sim integration for physical units
    // -----------------------------------------------------------------------------------------

    /// Report every vehicle units must not drive into: players, racers, traffic (physical and
    /// kinematic) and police cars, including the units' own bodies (each unit skips itself).
    /// Call once per tick before [`PoliceSystem::unit_input`]; the list stays until replaced.
    /// Without it, units avoid the racers and each other only.
    pub fn set_obstacles(&mut self, vehicles: &[ObstacleView]) {
        self.ext_obstacles.clear();
        self.ext_obstacles
            .extend(vehicles.iter().map(avoid::Obstacle::from_view));
        self.ext_obstacles_set = true;
        self.rebuild_obstacles();
    }

    /// Rebuild `obstacles`: `wp_sim`'s list plus internally driven units, or (headless) the
    /// racers and every ground unit.
    pub(crate) fn rebuild_obstacles(&mut self) {
        self.obstacles.clear();
        let ext = self.ext_obstacles_set;
        if ext {
            self.obstacles.extend_from_slice(&self.ext_obstacles);
            // Mark police cars (junction give-way breaks ties between units by id).
            for o in &mut self.obstacles {
                if let Some(u) = self
                    .units
                    .iter()
                    .find(|u| u.entity.is_some() && u.entity == o.entity)
                {
                    o.unit = Some(u.id);
                }
            }
        } else {
            for r in self.racers.values() {
                self.obstacles.push(avoid::Obstacle::car(
                    Some(r.entity),
                    None,
                    xy(r.pos),
                    [r.vel[0], r.vel[1]],
                    [r.vel[0], r.vel[1]],
                ));
            }
        }
        for u in &self.units {
            if u.kind != UnitKind::Car || (ext && u.entity.is_some()) {
                continue;
            }
            self.obstacles.push(avoid::Obstacle::car(
                u.entity,
                Some(u.id),
                u.body.pos,
                u.body.vel(),
                u.body.heading,
            ));
        }
    }

    /// Run obstacle awareness for unit `i` for one tick; returns the lane offset to steer for.
    pub(crate) fn update_avoid(&mut self, i: usize, dt: f32) -> f32 {
        let u = &mut self.units[i];
        let ignore = match (&u.assignment, u.tactic) {
            (Assignment::Pursue { target }, Tactic::Pit | Tactic::BoxIn(_)) => Some(*target),
            _ => None,
        };
        let me = avoid::Me {
            unit: u.id,
            entity: u.entity,
            pos: u.body.pos,
            heading: u.body.heading,
            track_s: u.track_s,
            ignore,
            target: u.assignment.target(),
            skip_higher_units: false,
        };
        avoid::update(
            &mut u.avoid,
            &me,
            &u.cmd.track,
            u.cmd.lane_offset_m,
            u.body.speed,
            u.cmd.siren,
            &self.obstacles,
            &self.cfg.pursuit.avoid,
            dt,
        )
    }

    /// Bind a unit to the vehicle entity `wp_sim` spawned for it, with that car's steering
    /// geometry (`VehicleDef::wheelbase_m`, front `WheelDef::max_steer_rad`). From then on the
    /// unit is driven externally: `wp_sim` applies [`PoliceSystem::unit_input`] and reports back
    /// with [`PoliceSystem::sync_unit`].
    pub fn bind_unit_entity(&mut self, unit: UnitId, entity: EntityId, geometry: SteerGeometry) {
        if let Some(i) = self.unit_index(unit) {
            let u = &mut self.units[i];
            u.entity = Some(entity);
            u.internal = false;
            u.geometry = geometry;
            u.controller = None;
        }
    }

    /// Add a player-driven chase unit. It receives [`UnitOrder`] suggestions, never control.
    pub fn add_player_unit(
        &mut self,
        player: PlayerId,
        entity: EntityId,
        pos: [f32; 3],
        heading: [f32; 2],
    ) -> UnitId {
        let id = self.alloc_unit_id();
        let mut u = Unit::new(
            id,
            UnitKind::Car,
            UnitRole::Player,
            xy(pos),
            heading,
            self.cfg.pursuit.risk.budget,
        );
        u.entity = Some(entity);
        u.player = Some(player);
        u.internal = false;
        self.units.push(u);
        id
    }

    /// Report the physical state of an externally driven unit.
    pub fn sync_unit(&mut self, unit: UnitId, s: UnitSync) {
        if let Some(i) = self.unit_index(unit) {
            let u = &mut self.units[i];
            let speed = (s.vel[0] * s.vel[0] + s.vel[1] * s.vel[1]).sqrt();
            u.body = UnitBody {
                pos: xy(s.pos),
                heading: graph::normalize(s.heading),
                speed,
                lat_g: u.body.lat_g,
                alt_m: s.pos[2],
            };
            u.health = s.health;
            u.effects = s.effects;
        }
    }

    /// Driver input for an externally driven AI unit, from `wp_world`'s shared
    /// [`DriveController`](wp_world::drive::DriveController): pure-pursuit steering toward the
    /// unit's track and PID pedals toward its planned speed. Call once per physics tick with
    /// that tick's `dt`, after [`PoliceSystem::sync_unit`]. `None` for player units and unknown ids.
    pub fn unit_input(&mut self, unit: UnitId, dt: f32) -> Option<VehicleInput> {
        let i = self.unit_index(unit)?;
        let (accel, brake) = (
            self.cfg.pursuit.unit.accel_mps2,
            self.cfg.pursuit.unit.brake_mps2,
        );
        let u = &mut self.units[i];
        if u.player.is_some() {
            return None;
        }
        let v = u.body.speed;
        if !u.cmd.track.is_drivable() {
            // Parked (tool units, between jobs): hold the brake. The controller alone releases
            // it below its hold speed, and the car would idle-creep away.
            let (_, b) = u.controller().pedals(v, -brake, dt, true);
            let (brake, handbrake) = standstill_hold(v, b);
            return Some(VehicleInput {
                brake,
                handbrake,
                siren: u.cmd.siren,
                headlights: true,
                ..Default::default()
            });
        }
        let (s, _) = u.cmd.track.project_ahead(u.body.pos, u.track_s, 3.0, 40.0);
        u.track_s = s;
        let offset = self.update_avoid(i, dt);
        let u = &mut self.units[i];
        let look = u.controller().lookahead(v);
        let target = unit::lane_point(&u.cmd.track, s + look, offset);
        let (pos, heading) = (u.body.pos, u.body.heading);
        let want = u.cmd.speed_mps.min(u.planned_speed).min(u.avoid.limit_mps);
        let hold = u.avoid.limit_mps <= 0.0;
        let acc = (want - v).clamp(-brake, accel);
        let disabled = u.wrecked || u.assignment == Assignment::OutOfService;
        let siren = u.cmd.siren;
        let ctl = u.controller();
        let steer = ctl.steer(pos, heading, [target[0], target[1], 0.0]);
        let (mut throttle, mut brake) = ctl.pedals(v, acc, dt, disabled);
        let mut handbrake = 0.0;
        if hold {
            // The shared controller releases the brake below its hold speed; right behind
            // another vehicle or at a stop line the unit must not creep forwards either.
            throttle = 0.0;
            (brake, handbrake) = standstill_hold(v, brake.max(1.0));
        }
        Some(VehicleInput {
            steer,
            throttle,
            brake,
            handbrake,
            siren,
            headlights: true,
            ..Default::default()
        })
    }

    pub(crate) fn alloc_unit_id(&mut self) -> UnitId {
        let id = UnitId(self.next_unit);
        self.next_unit += 1;
        id
    }

    // -----------------------------------------------------------------------------------------
    // Racer-side gadgets
    // -----------------------------------------------------------------------------------------

    /// Radar detector: units within the tier's range that are patrolling or pursuing.
    pub fn radar(&self, racer_pos: [f32; 3], tier: usize) -> Vec<RadarBlip> {
        let ranges = &self.cfg.tools.gadgets.radar_range_m;
        let range = ranges[tier.min(ranges.len() - 1)];
        let p = xy(racer_pos);
        let mut v: Vec<RadarBlip> = self
            .units
            .iter()
            .filter(|u| u.kind == UnitKind::Car && !u.wrecked)
            .filter_map(|u| {
                let state = match u.assignment {
                    Assignment::Patrol { .. } => tools::RadarState::Patrol,
                    Assignment::Pursue { .. } | Assignment::Intercept { .. } => {
                        tools::RadarState::Pursuit
                    }
                    _ => return None,
                };
                let d = graph::dist(u.body.pos, p);
                (d <= range).then(|| RadarBlip {
                    unit: u.id,
                    distance_m: d,
                    dir: graph::normalize(graph::sub(u.body.pos, p)),
                    state,
                })
            })
            .collect();
        v.sort_by(|a, b| {
            a.distance_m
                .total_cmp(&b.distance_m)
                .then(a.unit.cmp(&b.unit))
        });
        v
    }

    /// Scanner: radio lines spoken after `since` (all if `None`), delayed by the tier's delay.
    pub fn scanner(&self, tier: usize, since: Option<SimTime>) -> Vec<RadioLine> {
        let delays = &self.cfg.tools.gadgets.scanner_delay_s;
        let delay = delays[tier.min(delays.len() - 1)];
        self.radio
            .iter()
            .filter(|l| since.is_none_or(|s| l.time > s) && self.now.since(l.time) >= delay)
            .cloned()
            .collect()
    }

    /// Full radio log (captain).
    pub fn radio_log(&self) -> impl Iterator<Item = &RadioLine> {
        self.radio.iter()
    }

    /// Helicopters on station.
    pub fn helicopters(&self) -> Vec<HeloView> {
        self.units
            .iter()
            .filter_map(|u| match (u.kind, &u.assignment) {
                (UnitKind::Helicopter, Assignment::HeloTrack { target }) => {
                    let aim = self
                        .racers
                        .get(target)
                        .and_then(|r| r.last_known)
                        .unwrap_or(u.body.pos);
                    Some(HeloView {
                        unit: u.id,
                        pos: [u.body.pos[0], u.body.pos[1], u.body.alt_m],
                        spotlight_target: [aim[0], aim[1], 0.0],
                        cone_half_angle_rad: self.cfg.tools.helicopter.spotlight_half_angle_rad,
                        target: *target,
                    })
                }
                _ => None,
            })
            .collect()
    }

    /// Stable hash of the police state (tick, racers, units, tools) for determinism checks.
    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        h.write_u64(self.tick);
        for (e, r) in &self.racers {
            h.write_u32(e.0);
            h.write_f32_q(r.heat, 1e3);
            h.write_u32(r.status as u32);
            h.write_u32(r.level as u32);
        }
        for u in &self.units {
            h.write_u32(u.id.0);
            h.write_f32_q(u.body.pos[0], 1e3);
            h.write_f32_q(u.body.pos[1], 1e3);
            h.write_f32_q(u.body.speed, 1e3);
            h.write_f32_q(u.health, 1e4);
            h.write_bytes(u.assignment.name().as_bytes());
        }
        h.write_u32(self.roadblocks.len() as u32);
        h.write_u32(self.spikes.len() as u32);
        h.write_u64(self.stats.pursuits.len() as u64);
        h.finish()
    }
}

/// Pedal held at a standstill (below `wp_vehicle`'s auto-reverse threshold of 0.5).
const STANDSTILL_BRAKE: f32 = 0.4;

/// Brake and handbrake that keep a car stopped: above walking pace the requested `brake`; at a
/// standstill a light pedal plus the handbrake, because `wp_vehicle`'s automatic gearbox
/// engages reverse when the pedal is held past half travel at a standstill.
fn standstill_hold(v: f32, brake: f32) -> (f32, f32) {
    if v.abs() > 1.0 {
        (brake, 0.0)
    } else {
        (STANDSTILL_BRAKE, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_city_still_tracks_heat() {
        struct W;
        impl WorldQuery for W {
            fn surface_at(&self, _p: [f32; 3]) -> wp_contracts::SurfaceType {
                wp_contracts::SurfaceType::Asphalt
            }
            fn speed_limit_at(&self, _p: [f32; 3]) -> Option<f32> {
                None
            }
            fn signal_state_ahead(
                &self,
                _p: [f32; 3],
                _h: [f32; 3],
            ) -> Option<wp_contracts::SignalState> {
                None
            }
            fn time_of_day(&self) -> wp_contracts::TimeOfDay {
                wp_contracts::TimeOfDay::from_hours(12.0)
            }
            fn line_of_sight(&self, _a: [f32; 3], _b: [f32; 3]) -> bool {
                true
            }
        }
        let mut p = PoliceSystem::new();
        let r = RacerView {
            entity: EntityId(1),
            pos: [0.0; 3],
            vel: [0.0; 3],
        };
        let ev = vec![SimEvent::TrafficViolation {
            vehicle: EntityId(1),
            kind: wp_contracts::ViolationKind::Collision,
            pos: [0.0; 3],
        }];
        let mut out = Vec::new();
        p.step(&W, &[r], &ev, wp_contracts::PHYSICS_DT, &mut out);
        assert!(p.racer(EntityId(1)).unwrap().heat > 0.0);
        assert!(p.units().is_empty());
    }
}
