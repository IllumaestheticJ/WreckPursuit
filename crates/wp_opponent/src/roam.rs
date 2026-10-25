//! Free-roam AI street racers: cars that live on the city's roads outside any organised race.
//!
//! Each roamer has a home **haunt** ([`Haunt`]: highway, touge, dirt, downtown, rural). It
//! chains lane-legal *legs* (`wp_world::routes::plan_route`) from wherever it is to a lane it
//! picked, mostly inside its haunt and sometimes across town to another one. A leg whose
//! route lies mostly in the roamer's home haunt is a **run** (full pace: the driver's whole
//! [`DriverProfile`]); any other leg is **transit** (posted limit × `transit_speed_scale`,
//! calmer driver). Every leg is driven by a one-car [`Field`], so roamers get the same
//! perception, civilian avoidance, signals, police flight/yield, damage awareness and
//! recovery as race opponents. Other roamers, players, police and physical traffic are passed
//! like any other vehicle.
//!
//! Haunts are classified from data any `wp_world` city already has, so new maps need no
//! contract change: highway = `LaneKind::Highway`; dirt = `WorldQuery::surface_at` reads
//! dirt or gravel on the lane; touge = a winding stretch (heading change per 100 m over the
//! next [`HauntParams::touge_window_m`], following the road through bends and straight on
//! at junctions) that also climbs, or is very winding; downtown = a `DistrictId::Core` zone;
//! rural = a `Suburb` zone or no zone at all. A haunt the city lacks falls back to any
//! drivable lane.
//!
//! Deterministic: f32 only, seeded `wp_math::Rng` per roamer, fixed iteration order,
//! polynomial `atan2` from `wp_math`.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::{DistrictId, EntityId, SimEvent, SurfaceType, VehicleInput, WorldQuery};
use wp_math::{Rng, StateHasher};
use wp_world::drive::forward_of;
use wp_world::geom::{self, V2, V3};
use wp_world::network::{LaneId, LaneKind, TurnKind};
use wp_world::{Route, World};

use crate::capability::CarCapability;
use crate::corridor::{CorridorOptions, OpposingLanes};
use crate::field::{Field, OpponentEvent, RaceConfig, RacerSpec, RacerStatus, VehicleObs};
use crate::profile::DriverProfile;
use crate::pursuit::PoliceQuery;

/// Where a roamer likes to drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Haunt {
    /// Limited-access highways and their connectors: long top-speed pulls.
    Highway,
    /// Winding mountain-pass roads.
    Touge,
    /// Unpaved roads (dirt or gravel surface).
    Dirt,
    /// The dense downtown core, threading city traffic.
    Downtown,
    /// Farmland and outskirts.
    Rural,
}

impl Haunt {
    /// Every haunt, in declaration order.
    pub const ALL: [Haunt; 5] = [
        Haunt::Highway,
        Haunt::Touge,
        Haunt::Dirt,
        Haunt::Downtown,
        Haunt::Rural,
    ];

    /// Lower-case name (logs, UI).
    pub fn name(self) -> &'static str {
        match self {
            Haunt::Highway => "highway",
            Haunt::Touge => "touge",
            Haunt::Dirt => "dirt",
            Haunt::Downtown => "downtown",
            Haunt::Rural => "rural",
        }
    }
}

/// Thresholds of the haunt classifier.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HauntParams {
    /// Stretch of road (from the lane's start, along the road) the touge test looks at, m.
    pub touge_window_m: f32,
    /// Shortest stretch that can count as touge (the road may end or branch before the
    /// window does), m.
    pub touge_min_stretch_m: f32,
    /// Winding enough for touge when it also climbs: total heading change, rad per 100 m.
    pub touge_turn_rad_per_100m: f32,
    /// Climb that makes a winding road a mountain pass: (max z − min z) / stretch length.
    pub touge_min_grade: f32,
    /// Winding enough for touge on the flat, rad per 100 m.
    pub touge_flat_turn_rad_per_100m: f32,
    /// Lanes posted faster than this are never touge (fast sweepers, highways), m/s.
    pub touge_max_limit_mps: f32,
}

impl Default for HauntParams {
    fn default() -> Self {
        Self {
            touge_window_m: 300.0,
            touge_min_stretch_m: 150.0,
            // ≈ 26° per 100 m with a climb, ≈ 52° per 100 m on the flat. A city grid driven
            // straight on scores ~0; a hairpin section scores several rad per 100 m.
            touge_turn_rad_per_100m: 0.45,
            touge_min_grade: 0.03,
            touge_flat_turn_rad_per_100m: 0.9,
            touge_max_limit_mps: 25.0,
        }
    }
}

/// Every drivable lane of a city sorted into haunts.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HauntMap {
    /// Lanes per haunt, ascending lane id.
    by_haunt: BTreeMap<Haunt, Vec<LaneId>>,
    /// Haunt of each lane (`None`: not in any haunt, or not drivable by roamers).
    of_lane: Vec<Option<Haunt>>,
    /// Every lane a roamer may head for (drivable, not an alley, at least 20 m long).
    any: Vec<LaneId>,
}

/// Shortest lane a roamer heads for or spawns on, m.
const MIN_TARGET_LANE_M: f32 = 20.0;

impl HauntMap {
    /// Classify every lane of `world`'s city.
    pub fn classify(world: &World, params: &HauntParams) -> Self {
        let map = world.map();
        let net = world.network();
        let mut of_lane = vec![None; net.lanes.len()];
        let mut by_haunt: BTreeMap<Haunt, Vec<LaneId>> = BTreeMap::new();
        let mut any = Vec::new();
        for (i, lane) in net.lanes.iter().enumerate() {
            let id = i as LaneId;
            if !lane.kind.drivable() || lane.kind == LaneKind::Alley {
                continue;
            }
            let length = map.seg(id).length;
            if length >= MIN_TARGET_LANE_M {
                any.push(id);
            }
            let h = classify_lane(world, id, params);
            of_lane[i] = h;
            if let Some(h) = h {
                by_haunt.entry(h).or_default().push(id);
            }
        }
        Self {
            by_haunt,
            of_lane,
            any,
        }
    }

    /// Lanes of haunt `h` (ascending id; empty when the city has none).
    pub fn lanes(&self, h: Haunt) -> &[LaneId] {
        self.by_haunt.get(&h).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Haunt of `lane`, if any.
    pub fn haunt_of(&self, lane: LaneId) -> Option<Haunt> {
        self.of_lane.get(lane as usize).copied().flatten()
    }

    /// Every lane a roamer may head for, whatever its haunt.
    pub fn any_lanes(&self) -> &[LaneId] {
        &self.any
    }

    /// Haunts this city has lanes for.
    pub fn present(&self) -> Vec<Haunt> {
        Haunt::ALL
            .into_iter()
            .filter(|h| !self.lanes(*h).is_empty())
            .collect()
    }

    /// Share of `route`'s lane length that lies in haunt `h`, 0..=1.
    pub fn share_of(&self, world: &World, route: &Route, h: Haunt) -> f32 {
        let map = world.map();
        let (mut total, mut inside) = (0.0f32, 0.0f32);
        for l in &route.lanes {
            let len = map.seg(*l).length;
            total += len;
            if self.haunt_of(*l) == Some(h) {
                inside += len;
            }
        }
        if total > 0.0 {
            inside / total
        } else {
            0.0
        }
    }
}

/// Haunt of one drivable lane. Order of precedence: dirt, highway, touge, downtown, rural.
fn classify_lane(world: &World, id: LaneId, params: &HauntParams) -> Option<Haunt> {
    let net = world.network();
    let map = world.map();
    let lane = &net.lanes[id as usize];
    let seg = map.seg(id);
    let at = |f: f32| geom::point_at(&seg.pts, &seg.cum, seg.length * f).0;
    let dirt = [0.25f32, 0.5, 0.75]
        .iter()
        .filter(|f| {
            matches!(
                world.surface_at(at(**f)),
                SurfaceType::Dirt | SurfaceType::Gravel
            )
        })
        .count()
        >= 2;
    if dirt {
        return Some(Haunt::Dirt);
    }
    if lane.kind == LaneKind::Highway {
        return Some(Haunt::Highway);
    }
    let mid = geom::xy(at(0.5));
    let district = map.zone_at(mid).map(|z| net.zones[z as usize].kind);
    if district != Some(DistrictId::Core)
        && lane.speed_limit_mps <= params.touge_max_limit_mps
        && is_touge(world, id, params)
    {
        return Some(Haunt::Touge);
    }
    match district {
        Some(DistrictId::Core) => Some(Haunt::Downtown),
        Some(DistrictId::Suburb) | None => Some(Haunt::Rural),
        _ => None,
    }
}

/// Whether the road from the start of `id` onward is a mountain pass (see [`HauntParams`]).
fn is_touge(world: &World, id: LaneId, params: &HauntParams) -> bool {
    let net = world.network();
    let mut turn = 0.0f32;
    let mut dist = 0.0f32;
    let (mut z_min, mut z_max) = (f32::INFINITY, f32::NEG_INFINITY);
    let mut prev_dir: Option<V2> = None;
    let mut lane_id = id;
    let mut visited = 0;
    'walk: loop {
        let lane = &net.lanes[lane_id as usize];
        for w in lane.polyline.windows(2) {
            let d = geom::sub(geom::xy(w[1]), geom::xy(w[0]));
            let l = geom::len(d);
            z_min = z_min.min(w[0][2]).min(w[1][2]);
            z_max = z_max.max(w[0][2]).max(w[1][2]);
            if l < 1e-3 {
                continue;
            }
            let dir = geom::scale(d, 1.0 / l);
            if let Some(p) = prev_dir {
                turn += geom::atan2(geom::cross(p, dir), geom::dot(p, dir)).abs();
            }
            prev_dir = Some(dir);
            dist += l;
            if dist >= params.touge_window_m {
                break 'walk;
            }
        }
        // Follow the road: its only continuation, or straight on at a junction.
        let next = match lane.connections.as_slice() {
            [only] if only.turn != TurnKind::UTurn => Some(only.to),
            many => many
                .iter()
                .find(|c| c.turn == TurnKind::Straight)
                .map(|c| c.to),
        };
        visited += 1;
        match next {
            Some(n) if n != id && visited < 64 => lane_id = n,
            _ => break,
        }
    }
    if dist < params.touge_min_stretch_m {
        return false;
    }
    let per_100 = turn / dist * 100.0;
    let grade = (z_max - z_min) / dist;
    (per_100 >= params.touge_turn_rad_per_100m && grade >= params.touge_min_grade)
        || per_100 >= params.touge_flat_turn_rad_per_100m
}

/// One roamer to put on the roads.
#[derive(Debug, Clone, PartialEq)]
pub struct RoamerSpec {
    /// Sim entity of its car (spawn it as `VehicleKind::AiRacer` with `AssistProfile::ai()`).
    pub entity: EntityId,
    /// Personality at full pace (runs); transit legs calm it down.
    pub profile: DriverProfile,
    /// Measured car limits ([`CarCapability::for_car`]).
    pub capability: CarCapability,
    /// Home haunt.
    pub haunt: Haunt,
}

/// Roaming rules.
#[derive(Debug, Clone, PartialEq)]
pub struct RoamConfig {
    /// Seed of every roamer's choices and driving noise.
    pub seed: u64,
    /// Shortest leg, m.
    pub leg_min_m: f32,
    /// Longest leg, m.
    pub leg_max_m: f32,
    /// Chance that the next leg heads into the roamer's home haunt (otherwise another haunt
    /// the city has, or anywhere).
    pub home_bias: f32,
    /// Share of a leg inside the home haunt that makes it a full-pace run.
    pub run_share: f32,
    /// Transit legs: speed cap = this × the fastest posted limit on the leg.
    pub transit_speed_scale: f32,
    /// Transit legs: the driver's skill and aggression are scaled by this.
    pub transit_calm: f32,
    /// A new leg is planned this far before the current one ends, m (a leg's last metres
    /// are where a race field parks its finishers).
    pub handoff_m: f32,
    /// A busted roamer stays put this long before it drives off again, s.
    pub busted_hold_s: f32,
    /// Destinations tried per leg before falling back to any lane.
    pub plan_attempts: u32,
    /// Seconds between attempts when no leg could be planned.
    pub replan_wait_s: f32,
    /// Physics ticks between AI decisions (4 = 60 Hz).
    pub decision_ticks: u32,
}

impl Default for RoamConfig {
    fn default() -> Self {
        Self {
            seed: 1,
            leg_min_m: 500.0,
            leg_max_m: 3000.0,
            home_bias: 0.7,
            run_share: 0.5,
            transit_speed_scale: 1.25,
            transit_calm: 0.7,
            handoff_m: 60.0,
            busted_hold_s: 45.0,
            plan_attempts: 8,
            replan_wait_s: 2.0,
            decision_ticks: 4,
        }
    }
}

/// What a roamer is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoamPhase {
    /// No leg yet (first tick, or planning failed and it waits to retry).
    Idle,
    /// Driving a transit leg at street pace.
    Cruising,
    /// Driving a full-pace leg in its haunt.
    Running,
    /// Running from the police.
    Fleeing,
    /// Caught: parked until the hold ends.
    Busted,
    /// Car too damaged to drive: parked for good.
    Wrecked,
}

/// Roamer events (UI, audio, economy, tests).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RoamEvent {
    /// A new leg was planned.
    LegStarted {
        /// Roamer.
        entity: EntityId,
        /// Haunt it heads for (`None`: anywhere).
        haunt: Option<Haunt>,
        /// Full-pace run (else transit).
        run: bool,
        /// Route length, m.
        length_m: f32,
    },
    /// Ran from the police.
    Fled {
        /// Roamer.
        entity: EntityId,
        /// Wanted level that triggered it.
        wanted_level: u8,
    },
    /// Lost the police.
    Escaped {
        /// Roamer.
        entity: EntityId,
    },
    /// Caught by the police.
    Busted {
        /// Roamer.
        entity: EntityId,
    },
    /// Wrecked (parked for good).
    Wrecked {
        /// Roamer.
        entity: EntityId,
    },
    /// Touched a civilian car while driving into it.
    HitCivilian {
        /// Roamer.
        entity: EntityId,
        /// Civilian agent id.
        civilian: u32,
    },
}

/// The current leg of a roamer.
#[derive(Debug, Clone)]
struct Leg {
    field: Field,
    haunt: Option<Haunt>,
    run: bool,
}

#[derive(Debug, Clone)]
struct Roamer {
    spec: RoamerSpec,
    rng: Rng,
    leg: Option<Leg>,
    phase: RoamPhase,
    /// Seconds left before a busted roamer may plan again, or before the next planning
    /// attempt after a failure.
    wait_s: f32,
    legs: u32,
    distance_m: f32,
    last_pos: Option<V2>,
    /// Where the car last made [`STRANDED_MOVE_M`] of headway from, and for how long it has
    /// not (s); also how long it has been wrecked.
    anchor: Option<V2>,
    stuck_s: f32,
    /// Last civilian hit (agent id, tick), so one touch is reported once.
    last_hit: Option<(u32, u64)>,
}

/// The same civilian touched again within this many ticks is the same incident (5 s).
const HIT_REPEAT_TICKS: u64 = 1200;

/// Moving this far from where it stood resets a roamer's stranded clock, m.
const STRANDED_MOVE_M: f32 = 6.0;

/// Every roamer of a session. Call [`Roamers::tick`] once per physics tick.
#[derive(Debug, Clone)]
pub struct Roamers {
    haunts: HauntMap,
    roamers: Vec<Roamer>,
    config: RoamConfig,
    tick: u64,
    events: Vec<RoamEvent>,
    outputs: Vec<(EntityId, VehicleInput)>,
}

/// Stop and hold on the handbrake (the brake pedal alone selects reverse at a standstill).
fn hold() -> VehicleInput {
    VehicleInput {
        handbrake: 1.0,
        ..VehicleInput::default()
    }
}

impl Roamers {
    /// Roamers on `world`'s roads. `haunts` from [`HauntMap::classify`] (classify once per
    /// city and share it).
    pub fn new(haunts: HauntMap, specs: &[RoamerSpec], config: RoamConfig) -> Self {
        let mut master = Rng::new(config.seed ^ 0x0A0A_4D52_u64);
        let roamers = specs
            .iter()
            .map(|s| Roamer {
                spec: s.clone(),
                rng: master.fork(),
                leg: None,
                phase: RoamPhase::Idle,
                wait_s: 0.0,
                legs: 0,
                distance_m: 0.0,
                last_pos: None,
                anchor: None,
                stuck_s: 0.0,
                last_hit: None,
            })
            .collect();
        Self {
            haunts,
            roamers,
            config,
            tick: 0,
            events: Vec::new(),
            outputs: Vec::new(),
        }
    }

    /// The haunt classification in use.
    pub fn haunts(&self) -> &HauntMap {
        &self.haunts
    }

    /// Number of roamers.
    pub fn len(&self) -> usize {
        self.roamers.len()
    }

    /// No roamers.
    pub fn is_empty(&self) -> bool {
        self.roamers.is_empty()
    }

    /// Roamer entities, in spec order.
    pub fn entities(&self) -> Vec<EntityId> {
        self.roamers.iter().map(|r| r.spec.entity).collect()
    }

    /// Phase of roamer `k` (spec order).
    pub fn phase_of(&self, k: usize) -> RoamPhase {
        self.roamers[k].phase
    }

    /// Legs roamer `k` has started.
    pub fn legs_of(&self, k: usize) -> u32 {
        self.roamers[k].legs
    }

    /// Distance roamer `k` has driven, m.
    pub fn distance_of(&self, k: usize) -> f32 {
        self.roamers[k].distance_m
    }

    /// Haunt the current leg of roamer `k` heads for.
    pub fn leg_haunt_of(&self, k: usize) -> Option<Haunt> {
        self.roamers[k].leg.as_ref().and_then(|l| l.haunt)
    }

    /// Whether roamer `k`'s current leg is a full-pace run (else transit or no leg).
    pub fn on_run(&self, k: usize) -> bool {
        self.roamers[k].leg.as_ref().is_some_and(|l| l.run)
    }

    /// The field driving roamer `k`'s current leg (debugging: `debug_of(0)`, `tactic_of(0)`).
    pub fn field_of(&self, k: usize) -> Option<&Field> {
        self.roamers[k].leg.as_ref().map(|l| &l.field)
    }

    /// Inputs of the last tick, in spec order.
    pub fn inputs(&self) -> &[(EntityId, VehicleInput)] {
        &self.outputs
    }

    /// Roamers that need the session to put them somewhere else: stuck within a few metres
    /// for at least `stuck_s` (a world snag, a deadlock it cannot back out of), or wrecked
    /// for at least as long. Place the car on a fresh [`spawn_slots`] slot (out of the
    /// player's sight; repaired when wrecked) and call [`Roamers::relocated`].
    pub fn stranded(&self, stuck_s: f32) -> Vec<EntityId> {
        self.roamers
            .iter()
            .filter(|r| r.stuck_s >= stuck_s && r.phase != RoamPhase::Busted)
            .map(|r| r.spec.entity)
            .collect()
    }

    /// The session moved (and possibly repaired) `entity`'s car: forget its leg and plan a
    /// new one from where it is now.
    pub fn relocated(&mut self, entity: EntityId) {
        if let Some(r) = self.roamers.iter_mut().find(|r| r.spec.entity == entity) {
            r.leg = None;
            r.phase = RoamPhase::Idle;
            r.wait_s = 0.0;
            r.anchor = None;
            r.stuck_s = 0.0;
            r.last_pos = None;
        }
    }

    /// Take the events since the last call.
    pub fn drain_events(&mut self) -> Vec<RoamEvent> {
        std::mem::take(&mut self.events)
    }

    /// Advance one physics tick. `vehicles`: every sim vehicle (roamers included);
    /// `events`: this tick's sim events. Returns the inputs for every roamer whose car
    /// exists, in spec order.
    pub fn tick(
        &mut self,
        world: &World,
        vehicles: &[VehicleObs],
        events: &[SimEvent],
        police: &dyn PoliceQuery,
    ) -> &[(EntityId, VehicleInput)] {
        let decide = self
            .tick
            .is_multiple_of(self.config.decision_ticks.max(1) as u64);
        let dt = wp_contracts::PHYSICS_DT;
        self.outputs.clear();
        for k in 0..self.roamers.len() {
            let entity = self.roamers[k].spec.entity;
            let Some(obs) = vehicles.iter().find(|v| v.entity == entity) else {
                continue;
            };
            let pos = [obs.state.pos[0], obs.state.pos[1]];
            let r = &mut self.roamers[k];
            if let Some(p) = r.last_pos {
                r.distance_m += geom::dist(p, pos);
            }
            r.last_pos = Some(pos);
            r.wait_s = (r.wait_s - dt).max(0.0);
            match r.anchor {
                Some(a) if geom::dist(a, pos) < STRANDED_MOVE_M && r.phase != RoamPhase::Busted => {
                    r.stuck_s += dt
                }
                _ => {
                    r.anchor = Some(pos);
                    r.stuck_s = 0.0;
                }
            }
            let needs_leg = match r.phase {
                RoamPhase::Wrecked | RoamPhase::Fleeing => false,
                RoamPhase::Busted => r.wait_s <= 0.0,
                RoamPhase::Idle => r.wait_s <= 0.0,
                RoamPhase::Cruising | RoamPhase::Running => r.leg.as_ref().is_none_or(|l| {
                    let st = &l.field.standings()[0];
                    match st.status {
                        RacerStatus::Racing => {
                            st.progress_m >= l.field.corridor().length_m - self.config.handoff_m
                        }
                        RacerStatus::Finished { .. } | RacerStatus::Disqualified => true,
                        _ => false,
                    }
                }),
            };
            if needs_leg && decide {
                self.plan_leg(k, world, obs);
            }
            let r = &mut self.roamers[k];
            let input = match &mut r.leg {
                Some(leg) if r.phase != RoamPhase::Busted => {
                    let inp = leg.field.tick(world, vehicles, events, police)[0].1;
                    for ev in leg.field.drain_events() {
                        match ev {
                            OpponentEvent::Abandoned { wanted_level, .. } => {
                                r.phase = RoamPhase::Fleeing;
                                self.events.push(RoamEvent::Fled {
                                    entity,
                                    wanted_level,
                                });
                            }
                            OpponentEvent::Escaped { .. } => {
                                // Next leg from here (the field would cool down and park).
                                r.phase = RoamPhase::Idle;
                                r.wait_s = 0.0;
                                self.events.push(RoamEvent::Escaped { entity });
                            }
                            OpponentEvent::Busted { .. } => {
                                r.phase = RoamPhase::Busted;
                                r.wait_s = self.config.busted_hold_s;
                                self.events.push(RoamEvent::Busted { entity });
                            }
                            OpponentEvent::Disqualified { civilian, .. } => {
                                let again = r.last_hit.is_some_and(|(c, t)| {
                                    c == civilian && self.tick < t + HIT_REPEAT_TICKS
                                });
                                r.last_hit = Some((civilian, self.tick));
                                if !again {
                                    self.events
                                        .push(RoamEvent::HitCivilian { entity, civilian });
                                }
                            }
                            OpponentEvent::Retired { .. } => {
                                r.phase = RoamPhase::Wrecked;
                                self.events.push(RoamEvent::Wrecked { entity });
                            }
                            _ => {}
                        }
                    }
                    inp
                }
                // No leg yet, or busted: stand still.
                _ => hold(),
            };
            self.outputs.push((entity, input));
        }
        self.tick += 1;
        &self.outputs
    }

    /// Plan the next leg of roamer `k` from where its car is.
    fn plan_leg(&mut self, k: usize, world: &World, obs: &VehicleObs) {
        let cfg = self.config.clone();
        let r = &mut self.roamers[k];
        let st = &obs.state;
        let pos: V3 = st.pos;
        let fwd = forward_of(st.rot);
        let present = self.haunts.present();
        // Which haunt to head for.
        let target = if r.rng.chance(cfg.home_bias) && present.contains(&r.spec.haunt) {
            Some(r.spec.haunt)
        } else {
            let others: Vec<Haunt> = present
                .iter()
                .copied()
                .filter(|h| *h != r.spec.haunt)
                .collect();
            if others.is_empty() || r.rng.chance(0.25) {
                None
            } else {
                Some(others[r.rng.below(others.len() as u32) as usize])
            }
        };
        let mut planned: Option<(Route, Option<Haunt>)> = None;
        for set in [target, None] {
            let lanes = match set {
                Some(h) => self.haunts.lanes(h),
                None => self.haunts.any_lanes(),
            };
            if lanes.is_empty() {
                continue;
            }
            let mut best: Option<Route> = None;
            for _ in 0..cfg.plan_attempts {
                let lane = lanes[r.rng.below(lanes.len() as u32) as usize];
                let seg = world.map().seg(lane);
                if seg.length < MIN_TARGET_LANE_M {
                    continue;
                }
                let goal = geom::point_at(&seg.pts, &seg.cum, seg.length * 0.5).0;
                let crow = geom::dist(geom::xy(goal), geom::xy(pos));
                if crow > cfg.leg_max_m || crow < 0.3 * cfg.leg_min_m {
                    continue;
                }
                let Some(route) = world.plan_route(pos, Some(fwd), goal) else {
                    continue;
                };
                if route.length_m < cfg.handoff_m + 40.0 || route.length_m > 1.5 * cfg.leg_max_m {
                    continue;
                }
                let good = route.length_m >= cfg.leg_min_m;
                if good {
                    best = Some(route);
                    break;
                }
                if best.as_ref().is_none_or(|b| b.length_m < route.length_m) {
                    best = Some(route);
                }
            }
            if let Some(route) = best {
                planned = Some((route, set));
                break;
            }
        }
        let Some((route, haunt)) = planned else {
            r.phase = RoamPhase::Idle;
            r.wait_s = cfg.replan_wait_s;
            return;
        };
        let run = haunt == Some(r.spec.haunt)
            && self.haunts.share_of(world, &route, r.spec.haunt) >= cfg.run_share;
        let mut profile = r.spec.profile;
        let mut capability = r.spec.capability;
        if !run {
            profile.skill *= cfg.transit_calm;
            profile.aggression *= cfg.transit_calm;
            let limit = route
                .lanes
                .iter()
                .map(|l| world.map().seg(*l).speed_limit)
                .fold(0.0f32, f32::max)
                .max(10.0);
            capability.top_speed_mps = capability
                .top_speed_mps
                .min(limit * cfg.transit_speed_scale);
        }
        let seed = cfg.seed
            ^ (r.spec.entity.0 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (r.legs as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
        let config = RaceConfig {
            laps: 1,
            decision_ticks: cfg.decision_ticks,
            allow_abandon: true,
            // Detects civilian hits (reported as `RoamEvent::HitCivilian`; the roamer then
            // plans a new leg instead of cooling down).
            civilian_contact_dq: true,
            // Out of a race, street racers still drive round traffic.
            ram_traffic: false,
            // Cruising pace through junctions: the full look-ahead holds the lane.
            tight_turn_lookahead: false,
            seed,
            corridor: CorridorOptions {
                opposing: OpposingLanes::ForOvertaking,
                ..CorridorOptions::default()
            },
            signals: true,
            ..RaceConfig::default()
        };
        let spec = RacerSpec {
            entity: r.spec.entity,
            profile,
            capability,
        };
        let length_m = route.length_m;
        let field = Field::from_route(world, &route, &[spec], config);
        r.leg = Some(Leg { field, haunt, run });
        r.phase = if run {
            RoamPhase::Running
        } else {
            RoamPhase::Cruising
        };
        r.wait_s = 0.0;
        r.legs += 1;
        self.events.push(RoamEvent::LegStarted {
            entity: r.spec.entity,
            haunt,
            run,
            length_m,
        });
    }

    /// Deterministic hash of every roamer's state.
    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        h.write_u64(self.tick);
        for r in &self.roamers {
            h.write_u64(r.spec.entity.0 as u64);
            h.write_u64(r.phase as u64);
            h.write_u64(r.legs as u64);
            h.write_u32(r.stuck_s.to_bits());
            h.write_u64(r.rng.clone().next_u64());
            if let Some(l) = &r.leg {
                h.write_u64(l.field.state_hash());
            }
        }
        h.finish()
    }
}

/// Where to put a roamer's car when the session starts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpawnSlot {
    /// Position on the lane centreline, m.
    pub pos: V3,
    /// Facing along the lane (yaw about +Z, 0 = +Y), rad.
    pub yaw: f32,
    /// Lane.
    pub lane: LaneId,
    /// Haunt the slot was picked for (the lane's own haunt may differ when the city lacks
    /// that haunt).
    pub haunt: Haunt,
}

/// Spawn slots for `wanted` haunts (one slot per entry, same order): mid-lane on a lane of
/// that haunt (any lane when the city has none), at least `spacing_m` from every other slot
/// and from every point in `avoid` (the player). Fewer slots come back when the city has no
/// room left.
pub fn spawn_slots(
    world: &World,
    haunts: &HauntMap,
    wanted: &[Haunt],
    avoid: &[V2],
    spacing_m: f32,
    seed: u64,
) -> Vec<SpawnSlot> {
    let mut rng = Rng::new(seed ^ 0x5B4A_5107_u64);
    let map = world.map();
    let mut out: Vec<SpawnSlot> = Vec::new();
    for h in wanted {
        let lanes = match haunts.lanes(*h) {
            [] => haunts.any_lanes(),
            l => l,
        };
        if lanes.is_empty() {
            continue;
        }
        for _ in 0..64 {
            let lane = lanes[rng.below(lanes.len() as u32) as usize];
            let seg = map.seg(lane);
            if seg.length < 40.0 {
                continue;
            }
            let (p, dir) = geom::point_at(&seg.pts, &seg.cum, seg.length * rng.range_f32(0.3, 0.6));
            let q = geom::xy(p);
            let free = out
                .iter()
                .all(|s| geom::dist(geom::xy(s.pos), q) >= spacing_m)
                && avoid.iter().all(|a| geom::dist(*a, q) >= spacing_m);
            if free {
                out.push(SpawnSlot {
                    pos: p,
                    yaw: geom::yaw_of(dir),
                    lane,
                    haunt: *h,
                });
                break;
            }
        }
    }
    out
}

/// One entry of the stock roaming roster.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RosterEntry {
    /// Home haunt.
    pub haunt: Haunt,
    /// Car id (`data/cars/<id>.json`).
    pub car: &'static str,
    /// Personality.
    pub profile: DriverProfile,
}

/// Cars that suit each haunt (analog names, `data/cars`).
pub fn haunt_cars(h: Haunt) -> &'static [&'static str] {
    match h {
        Haunt::Highway => &["ironside_440", "rampart_50", "fulmine_v10", "lindwurm_4t"],
        Haunt::Touge => &["hakone_gt", "shiro_turbo", "kestrel_ts", "sperber_32"],
        Haunt::Dirt => &["junkyard_beater", "furrow_24", "vireo_xs"],
        Haunt::Downtown => &["kestrel_ts", "lucciola_350", "tansy_13", "shiro_turbo"],
        Haunt::Rural => &["hauler_1500", "junkyard_beater", "tansy_13", "rampart_50"],
    }
}

/// A personality drawn around haunt `h`'s style (deterministic in `seed`): touge drivers are
/// skilled and consistent, highway drivers aggressive, downtown drivers take risks, dirt and
/// rural drivers are looser.
pub fn stock_profile(h: Haunt, seed: u64) -> DriverProfile {
    let mut rng = Rng::new(seed ^ 0x2057_E200_u64);
    let mut j = |lo: f32, hi: f32| rng.range_f32(lo, hi);
    match h {
        Haunt::Touge => DriverProfile {
            skill: j(0.65, 0.95),
            aggression: j(0.4, 0.7),
            consistency: j(0.7, 0.95),
            risk: j(0.35, 0.6),
            awareness: j(0.6, 0.9),
        },
        Haunt::Highway => DriverProfile {
            skill: j(0.5, 0.85),
            aggression: j(0.6, 0.9),
            consistency: j(0.55, 0.85),
            risk: j(0.45, 0.75),
            awareness: j(0.6, 0.9),
        },
        Haunt::Downtown => DriverProfile {
            skill: j(0.45, 0.8),
            aggression: j(0.5, 0.85),
            consistency: j(0.5, 0.8),
            risk: j(0.5, 0.85),
            awareness: j(0.65, 0.95),
        },
        Haunt::Dirt | Haunt::Rural => DriverProfile {
            skill: j(0.4, 0.75),
            aggression: j(0.3, 0.6),
            consistency: j(0.4, 0.7),
            risk: j(0.3, 0.7),
            awareness: j(0.5, 0.8),
        },
    }
}

/// `n` roamers spread over `haunts` (cycled in order; all five when empty), each with a car
/// that suits its haunt ([`haunt_cars`]) and a [`stock_profile`] seeded by `seed` and its
/// index.
pub fn stock_roster(n: usize, haunts: &[Haunt], seed: u64) -> Vec<RosterEntry> {
    let order: &[Haunt] = if haunts.is_empty() {
        &Haunt::ALL
    } else {
        haunts
    };
    let mut rng = Rng::new(seed ^ 0x0C42_5E1E_u64);
    (0..n)
        .map(|i| {
            let haunt = order[i % order.len()];
            let cars = haunt_cars(haunt);
            let car = cars[rng.below(cars.len() as u32) as usize];
            RosterEntry {
                haunt,
                car,
                profile: stock_profile(haunt, roster_seed(seed, i)),
            }
        })
        .collect()
}

/// Profile seed of roster entry `i` (what [`stock_roster`] passes to [`stock_profile`]).
pub fn roster_seed(seed: u64, i: usize) -> u64 {
    seed.wrapping_add((i as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15))
}

/// Haunt by its [`Haunt::name`].
pub fn haunt_by_name(name: &str) -> Option<Haunt> {
    Haunt::ALL.into_iter().find(|h| h.name() == name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_world::procgen::ProcGenConfig;
    use wp_world::WorldConfig;

    fn city() -> World {
        World::procedural(
            &ProcGenConfig {
                nx: 8,
                ny: 8,
                ..ProcGenConfig::default()
            },
            WorldConfig::default(),
        )
    }

    #[test]
    fn grid_city_has_downtown_and_no_touge() {
        let w = city();
        let h = HauntMap::classify(&w, &HauntParams::default());
        assert!(!h.lanes(Haunt::Downtown).is_empty(), "{:?}", h.present());
        assert!(
            h.lanes(Haunt::Touge).is_empty(),
            "a straight grid is not a pass"
        );
        assert!(!h.any_lanes().is_empty());
    }

    #[test]
    fn roster_is_deterministic_and_fits_haunts() {
        let a = stock_roster(10, &[], 3);
        let b = stock_roster(10, &[], 3);
        assert_eq!(a, b);
        for e in &a {
            assert!(haunt_cars(e.haunt).contains(&e.car));
        }
        assert_eq!(a[0].haunt, Haunt::Highway);
        assert_eq!(a[3].haunt, Haunt::Downtown);
    }

    #[test]
    fn spawn_slots_are_spaced() {
        let w = city();
        let h = HauntMap::classify(&w, &HauntParams::default());
        let slots = spawn_slots(&w, &h, &Haunt::ALL, &[[0.0, 0.0]], 60.0, 9);
        assert_eq!(slots.len(), 5);
        for (i, a) in slots.iter().enumerate() {
            assert!(geom::dist(geom::xy(a.pos), [0.0, 0.0]) >= 60.0);
            for b in &slots[i + 1..] {
                assert!(geom::dist(geom::xy(a.pos), geom::xy(b.pos)) >= 60.0);
            }
        }
    }
}
