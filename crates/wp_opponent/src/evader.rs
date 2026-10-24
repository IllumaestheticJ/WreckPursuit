//! Scripted escapee ("evader bot") for the police competency benchmark (`simcli police-bench`,
//! `docs/integration/police-bench.md`).
//!
//! A wanted driver who wants to get away. Each tick it:
//! 1. **perceives** police cars it can see (line of sight within its sense range) or hear (a
//!    siren close by), but only acts on what it saw [`EvaderParams::reaction_s`] ago, and
//!    remembers units for a while after losing sight of them;
//! 2. **plans** every [`EvaderParams::replan_s`] (or when its path runs out): a search over the
//!    city's lane graph out to [`EvaderParams::foresight_m`], where roads near known units cost
//!    more, and picks the end point furthest from every unit, with a bonus for routes through
//!    tunnels / covered roads and for turns (both break line of sight), plus seeded noise;
//! 3. **drives** that path near the car's limit with the shared path follower
//!    (`wp_world::drive::PathFollower`, higher lateral-acceleration allowance than police
//!    pursuit driving), overtakes slower traffic when the other lane is clear, adds steering
//!    noise for imprecise drivers, and reverses out when stuck.
//!
//! Three skills ([`EvaderSkill`]) differ only in those parameters. It never reads police
//! internals (assignments, tactics, plans): only positions it could see.
//!
//! Deterministic: f32 only, seeded [`Rng`], fixed iteration order, polynomial `atan2`.
//! Nothing here changes police behaviour; it only drives one car.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap};

use serde::{Deserialize, Serialize};
use wp_contracts::{VehicleDef, VehicleInput, VehicleState};
use wp_math::Rng;
use wp_world::drive::{pose_2d, DrivePath, FollowParams, FollowTraffic, PathFollower};
use wp_world::geom::{self, V2, V3};
use wp_world::leader::{CarFollowing, LeaderParams, Obstacle};
use wp_world::map::SegKind;
use wp_world::network::TurnKind;
use wp_world::{CityMap, SegId};

/// Evader skill level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaderSkill {
    /// Slow to react, plans one block ahead, drives well inside the limit, sloppy.
    Casual,
    /// A decent street racer: reacts in about half a second, plans a few blocks ahead.
    Good,
    /// Near the car's limit, quick, plans far ahead and hunts for cover.
    Expert,
}

impl EvaderSkill {
    /// Every skill, weakest first.
    pub const ALL: [EvaderSkill; 3] = [EvaderSkill::Casual, EvaderSkill::Good, EvaderSkill::Expert];

    /// Short name (`casual`, `good`, `expert`).
    pub fn name(self) -> &'static str {
        match self {
            EvaderSkill::Casual => "casual",
            EvaderSkill::Good => "good",
            EvaderSkill::Expert => "expert",
        }
    }

    /// Parse [`EvaderSkill::name`].
    pub fn from_name(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.name() == s)
    }

    /// The tuning for this skill.
    pub fn params(self) -> EvaderParams {
        match self {
            EvaderSkill::Casual => EvaderParams {
                reaction_s: 1.2,
                replan_s: 3.0,
                foresight_m: 300.0,
                sense_range_m: 150.0,
                hear_range_m: 40.0,
                memory_s: 10.0,
                threat_radius_m: 120.0,
                max_lat_accel: 5.5,
                top_speed_mps: 30.0,
                min_target_mps: 16.0,
                limit_factor: 1.3,
                steer_noise: 0.05,
                overtake: false,
                cover_bonus_m: 0.0,
                turn_bonus_m: 0.0,
                route_noise_m: 150.0,
                unstick_s: 2.0,
            },
            EvaderSkill::Good => EvaderParams {
                reaction_s: 0.6,
                replan_s: 1.5,
                foresight_m: 600.0,
                sense_range_m: 250.0,
                hear_range_m: 70.0,
                memory_s: 20.0,
                threat_radius_m: 180.0,
                max_lat_accel: 6.8,
                top_speed_mps: 40.0,
                min_target_mps: 22.0,
                limit_factor: 1.6,
                steer_noise: 0.025,
                overtake: true,
                cover_bonus_m: 150.0,
                turn_bonus_m: 15.0,
                route_noise_m: 80.0,
                unstick_s: 1.0,
            },
            EvaderSkill::Expert => EvaderParams {
                reaction_s: 0.25,
                replan_s: 0.75,
                foresight_m: 1000.0,
                sense_range_m: 350.0,
                hear_range_m: 100.0,
                memory_s: 30.0,
                threat_radius_m: 250.0,
                max_lat_accel: 7.8,
                top_speed_mps: 48.0,
                min_target_mps: 28.0,
                limit_factor: 2.0,
                steer_noise: 0.01,
                overtake: true,
                cover_bonus_m: 300.0,
                turn_bonus_m: 30.0,
                route_noise_m: 40.0,
                unstick_s: 0.6,
            },
        }
    }
}

/// Evader tuning (see [`EvaderSkill::params`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EvaderParams {
    /// Delay between seeing a unit and acting on it, s.
    pub reaction_s: f32,
    /// Seconds between route re-plans.
    pub replan_s: f32,
    /// How far ahead along the road graph routes are planned, m.
    pub foresight_m: f32,
    /// Police cars in line of sight within this range are seen, m.
    pub sense_range_m: f32,
    /// Police cars this close are noticed without line of sight (siren, engine), m.
    pub hear_range_m: f32,
    /// A unit last seen this long ago is forgotten, s.
    pub memory_s: f32,
    /// Roads within this distance of a known unit are avoided, m.
    pub threat_radius_m: f32,
    /// Lateral acceleration the driver plans corners for, m/s².
    pub max_lat_accel: f32,
    /// Speed cap, m/s.
    pub top_speed_mps: f32,
    /// Speed on a road = its limit × this (capped at `top_speed_mps`)...
    pub limit_factor: f32,
    /// ...but never below this on any road, m/s (fleeing drivers ignore 30 km/h signs; curves
    /// and grip still slow it down).
    pub min_target_mps: f32,
    /// Steering noise amplitude (input units, a slow random walk).
    pub steer_noise: f32,
    /// Passes slower traffic in the next lane when it is clear.
    pub overtake: bool,
    /// Route score bonus for passing through a covered road (tunnel), m-equivalent.
    pub cover_bonus_m: f32,
    /// Route score bonus per turn (each one can break line of sight), m-equivalent.
    pub turn_bonus_m: f32,
    /// Random route score spread, m-equivalent (seeded).
    pub route_noise_m: f32,
    /// Stopped this long while trying to drive → reverse out, s.
    pub unstick_s: f32,
}

/// A police car as the world shows it (the bench hands every unit; the evader decides what it
/// can see).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PoliceCar {
    /// Caller's id (stable per unit).
    pub id: u32,
    /// Position, m.
    pub pos: V3,
    /// Velocity, m/s.
    pub vel: V3,
}

/// What the evader knows about one unit.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Known {
    pos: V2,
    vel: V2,
    seen_at: f32,
}

/// Perception snapshot (taken every [`PERCEIVE_DT`]).
#[derive(Debug, Clone, PartialEq)]
struct Snapshot {
    t: f32,
    cars: Vec<(u32, V2, V2)>,
    /// Some unit had line of sight to us (being watched).
    watched: bool,
}

/// Perception rate, s.
const PERCEIVE_DT: f32 = 0.1;
/// Seconds of reversing when stuck.
const REVERSE_S: f32 = 2.0;
/// Lateral offset used to pass a car (one lane over), m.
const LANE_OFFSET_M: f32 = 3.4;
/// A lane change, once started, is kept at least this long, s.
const OFFSET_HOLD_S: f32 = 2.0;
/// Reversals this close together mean the way ahead is blocked: turn round, m.
const TURN_ROUND_RADIUS_M: f32 = 8.0;

/// What the evader is doing (debug / bench output).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EvaderMode {
    /// Driving its planned route.
    #[default]
    Drive,
    /// Reversing out of a dead stop.
    Reverse,
}

/// Counters for the benchmark report.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct EvaderStats {
    /// Route plans made.
    pub plans: u32,
    /// Times it reversed out of a stop.
    pub reversals: u32,
    /// Lane changes to pass or go round a car.
    pub overtakes: u32,
    /// Plans that routed through a covered road.
    pub cover_routes: u32,
    /// Times it turned round after being blocked.
    pub turn_rounds: u32,
}

/// Last control step, for traces.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct EvaderDebug {
    /// Speed the path planner allowed, m/s.
    pub planned_mps: f32,
    /// Gap to the vehicle ahead on the path, m (`INFINITY` = none).
    pub leader_gap_m: f32,
    /// Id of that vehicle (the caller's [`Obstacle::id`]; `u32::MAX` = none).
    pub leader_id: u32,
    /// Lane-change check of the last blocked / slower moment: bit 0 lane left, 1 left clear,
    /// 2 lane right, 3 right clear.
    pub pass_flags: u8,
    /// Lateral offset driven, m.
    pub offset_m: f32,
    /// Distance off the path, m.
    pub cross_track_m: f32,
    /// Path length left, m.
    pub remaining_m: f32,
}

/// The evader bot. One per car; call [`Evader::step`] every physics tick.
#[derive(Debug, Clone)]
pub struct Evader {
    /// Tuning.
    pub params: EvaderParams,
    rng: Rng,
    follower: PathFollower,
    path: Option<DrivePath>,
    t: f32,
    next_plan_t: f32,
    next_perceive_t: f32,
    snapshots: Vec<Snapshot>,
    known: BTreeMap<u32, Known>,
    watched: bool,
    stopped_s: f32,
    mode: EvaderMode,
    reverse_until: f32,
    offset_m: f32,
    offset_until: f32,
    /// Where it last reversed and how many times in a row near there.
    last_reverse: Option<(V2, u32)>,
    /// Next plan starts on the opposite-direction lane (turn round).
    turn_round: bool,
    /// Segments of the current path and the path arc length where each starts.
    route: Vec<(SegId, f32)>,
    steer_bias: f32,
    /// Time of the newest perception snapshot acted on.
    applied_t: f32,
    stats: EvaderStats,
    debug: EvaderDebug,
}

impl Evader {
    /// New evader for a car (`def`: wheelbase, steering lock) currently at `speed_mps`.
    pub fn new(params: EvaderParams, def: &VehicleDef, seed: u64, speed_mps: f32) -> Self {
        let follow = FollowParams {
            max_accel: 6.0,
            plan_decel: 7.0,
            max_decel: 9.5,
            max_lat_accel: params.max_lat_accel,
            speed_gain: 3.0,
            stop_margin_m: 1.0,
        };
        Self {
            params,
            rng: Rng::new(seed ^ 0x00E7_ADE5),
            follower: PathFollower::for_vehicle(def, follow, speed_mps),
            path: None,
            t: 0.0,
            next_plan_t: 0.0,
            next_perceive_t: 0.0,
            snapshots: Vec::new(),
            known: BTreeMap::new(),
            watched: false,
            stopped_s: 0.0,
            mode: EvaderMode::Drive,
            reverse_until: 0.0,
            offset_m: 0.0,
            offset_until: 0.0,
            last_reverse: None,
            turn_round: false,
            route: Vec::new(),
            steer_bias: 0.0,
            applied_t: f32::NEG_INFINITY,
            stats: EvaderStats::default(),
            debug: EvaderDebug::default(),
        }
    }

    /// Corner for a road with this fraction of dry grip (wet roads: the bench passes the
    /// weather's grip; the driver knows it is raining). Scales the planned lateral and
    /// braking accelerations.
    pub fn set_grip_scale(&mut self, scale: f32) {
        let k = scale.clamp(0.3, 1.0);
        self.follower.params.max_lat_accel = self.params.max_lat_accel * k;
        self.follower.params.plan_decel = 7.0 * k;
        self.follower.params.max_decel = 9.5 * k;
    }

    /// Counters so far.
    pub fn stats(&self) -> EvaderStats {
        self.stats
    }

    /// Last control step (traces).
    pub fn debug(&self) -> EvaderDebug {
        self.debug
    }

    /// Current mode.
    pub fn mode(&self) -> EvaderMode {
        self.mode
    }

    /// Units it currently knows about (acted-on perception).
    pub fn known_units(&self) -> usize {
        self.known.len()
    }

    /// The path it is driving (debug).
    pub fn path(&self) -> Option<&DrivePath> {
        self.path.as_ref()
    }

    /// One control step: input for the car in `me`, given every police car in the world and
    /// every other vehicle (`traffic`, for car-following and overtaking; may include police).
    pub fn step(
        &mut self,
        map: &CityMap,
        me: &VehicleState,
        police: &[PoliceCar],
        traffic: &[Obstacle],
        dt: f32,
    ) -> VehicleInput {
        self.t += dt;
        let (pos, fwd) = pose_2d(me);
        if self.t >= self.next_perceive_t {
            self.next_perceive_t = self.t + PERCEIVE_DT;
            self.perceive(map, me, police);
        }
        // Reverse out of a dead stop (wall, parked car, a unit's nose).
        if self.mode == EvaderMode::Reverse {
            if self.t < self.reverse_until {
                // In reverse the pedals swap (wp_vehicle auto-reverse): the brake drives back.
                let steer = self.reverse_steer(pos, fwd);
                return VehicleInput {
                    steer,
                    brake: 1.0,
                    ..Default::default()
                };
            }
            self.mode = EvaderMode::Drive;
            self.stopped_s = 0.0;
            self.next_plan_t = self.t; // re-plan from where it ended up
        }
        let replan = self.t >= self.next_plan_t
            || self.path.as_ref().is_none_or(|p| {
                let (s, _, _) = p.project(pos, None);
                p.length() - s < 120.0
            });
        if replan {
            self.plan(map, me);
            self.next_plan_t = self.t + self.params.replan_s;
        }
        let Some(path) = self.path.as_ref() else {
            // Off the road graph: crawl forward.
            return VehicleInput {
                throttle: 0.3,
                ..Default::default()
            };
        };
        let (s, _, _) = path.project(pos, None);
        let target = self.target_speed(map, path, s);
        // Lane changes: around a stopped car (anyone) or past slower traffic (skilled drivers),
        // into whichever neighbouring lane is clear (left first), held for a moment.
        let lp = LeaderParams::CAR.lookahead_for(me.speed_mps);
        let mut offset = if self.t < self.offset_until {
            self.offset_m
        } else {
            0.0
        };
        if offset == 0.0 {
            if let Some(l) = path.leader(me.pos, s, 0.0, traffic, &[], &lp) {
                let blocked = l.gap_m < 25.0 && l.speed_mps < 2.0;
                let slower = self.params.overtake && l.gap_m < 35.0 && l.speed_mps < target - 3.0;
                if blocked || slower {
                    let look = LeaderParams {
                        lookahead_m: if blocked {
                            l.gap_m + 15.0
                        } else {
                            lp.lookahead_m + 40.0
                        },
                        ..lp
                    };
                    let mut flags = 0u8;
                    for (k, side) in [-LANE_OFFSET_M, LANE_OFFSET_M].into_iter().enumerate() {
                        let lane = lane_beside(map, path, s, side);
                        let clear = path.leader(me.pos, s, side, traffic, &[], &look).is_none();
                        flags |= (u8::from(lane) | u8::from(clear) << 1) << (2 * k);
                        self.debug.pass_flags = flags;
                        if lane && clear {
                            offset = side;
                            self.offset_m = offset;
                            self.offset_until = self.t + OFFSET_HOLD_S;
                            self.stats.overtakes += 1;
                            break;
                        }
                    }
                }
            }
        }
        let following = CarFollowing {
            headway_s: 0.6,
            min_gap_m: 2.0,
            max_accel: 6.0,
            comfort_decel: 6.0,
        };
        let ft = FollowTraffic {
            obstacles: traffic,
            skip: &[],
            corridor: LeaderParams::CAR,
            following,
        };
        let out = self
            .follower
            .follow_behind(me, path, target, offset, dt, Some(&ft));
        let mut input = out.input;
        let pass_flags = self.debug.pass_flags;
        self.debug = EvaderDebug {
            pass_flags,
            planned_mps: out.planned_speed_mps,
            leader_gap_m: out.leader.map_or(f32::INFINITY, |l| l.gap_m),
            leader_id: out.leader.map_or(u32::MAX, |l| l.id),
            offset_m: offset,
            cross_track_m: out.cross_track_m,
            remaining_m: out.remaining_m,
        };
        // Imprecise hands: a slow random walk on the steering.
        let n = self.params.steer_noise;
        if n > 0.0 {
            let kick = self.rng.range_f32(-1.0, 1.0) * n * 4.0 * dt;
            self.steer_bias = (self.steer_bias * (1.0 - dt) + kick).clamp(-n, n);
            input.steer = (input.steer + self.steer_bias).clamp(-1.0, 1.0);
        }
        input.headlights = true;
        // Stuck: trying to go but not moving.
        let trying = input.throttle > 0.2 || out.planned_speed_mps > 3.0;
        if trying && me.speed_mps.abs() < 1.0 {
            self.stopped_s += dt;
        } else {
            self.stopped_s = 0.0;
        }
        if self.stopped_s >= self.params.unstick_s {
            let n = match self.last_reverse {
                Some((p, n)) if geom::dist(p, pos) < TURN_ROUND_RADIUS_M => n + 1,
                _ => 1,
            };
            self.last_reverse = Some((pos, n));
            self.turn_round = n >= 2;
            self.mode = EvaderMode::Reverse;
            self.reverse_until = self.t + REVERSE_S;
            self.stats.reversals += 1;
            self.follower.reset(0.0);
        }
        input
    }

    /// Steering while reversing: back away turning the nose toward the path (or straight).
    fn reverse_steer(&self, pos: V2, fwd: V2) -> f32 {
        let Some(path) = &self.path else {
            return 0.0;
        };
        let (s, _, _) = path.project(pos, None);
        let (p, _) = path.point_at(s + 15.0);
        let to = geom::sub(geom::xy(p), pos);
        // Target to the right while reversing → steer left (the tail swings right, the nose
        // turns toward the target).
        let side = geom::dot(to, geom::right_of(fwd));
        if side > 0.5 {
            -0.8
        } else if side < -0.5 {
            0.8
        } else {
            0.0
        }
    }

    /// Speed it aims for at arc length `s`: the road's limit × `limit_factor`, capped.
    fn target_speed(&self, map: &CityMap, path: &DrivePath, s: f32) -> f32 {
        let (p, d) = path.point_at(s);
        let limit = map
            .locate(p, Some(d), 3.0, true, false)
            .map(|l| map.seg(l.seg).speed_limit)
            .unwrap_or(13.9);
        (limit * self.params.limit_factor)
            .max(self.params.min_target_mps)
            .min(self.params.top_speed_mps)
    }

    /// Take a perception snapshot now and update what it knows from the one `reaction_s` old.
    fn perceive(&mut self, map: &CityMap, me: &VehicleState, police: &[PoliceCar]) {
        let p = &self.params;
        let eye = [me.pos[0], me.pos[1], me.pos[2] + 1.0];
        let mut cars = Vec::new();
        let mut watched = false;
        for c in police {
            let d = geom::dist(geom::xy(c.pos), geom::xy(me.pos));
            if d > p.sense_range_m {
                continue;
            }
            let ceye = [c.pos[0], c.pos[1], c.pos[2] + 1.0];
            let los = map.line_of_sight(eye, ceye);
            watched |= los;
            if los || d <= p.hear_range_m {
                cars.push((c.id, geom::xy(c.pos), geom::xy(c.vel)));
            }
        }
        self.snapshots.push(Snapshot {
            t: self.t,
            cars,
            watched,
        });
        // Act on the newest snapshot at least `reaction_s` old; drop older ones.
        let cutoff = self.t - p.reaction_s;
        let Some(k) = self.snapshots.iter().rposition(|s| s.t <= cutoff + 1e-4) else {
            return;
        };
        let snap = self.snapshots[k].clone();
        self.snapshots.drain(..k);
        // A snapshot is applied once (it stays first in the list until a newer one is due).
        if snap.t <= self.applied_t {
            return;
        }
        for (id, pos, vel) in &snap.cars {
            self.known.insert(
                *id,
                Known {
                    pos: *pos,
                    vel: *vel,
                    seen_at: snap.t,
                },
            );
        }
        let newly_watched = snap.watched && !self.watched;
        self.watched = snap.watched;
        let mem = p.memory_s;
        let now = self.t;
        self.known.retain(|_, k| now - k.seen_at <= mem);
        // Spotted: think again now rather than at the next scheduled plan.
        if newly_watched {
            self.next_plan_t = self.next_plan_t.min(self.t);
        }
        self.applied_t = snap.t;
    }

    /// Where the next plan starts: `(segment, arc length on it, path points before it)`.
    /// Normally the first max(40 m, 2.5 s) of the current path stay as they are (a decision
    /// is not flipped just before a junction); after a turn-round request, the lane facing the
    /// other way.
    fn plan_start(&mut self, map: &CityMap, me: &VehicleState) -> Option<(SegId, f32, Vec<V3>)> {
        let (pos, fwd) = pose_2d(me);
        let here: V3 = [pos[0], pos[1], me.pos[2]];
        if std::mem::take(&mut self.turn_round) {
            let back = [-fwd[0], -fwd[1]];
            let (seg, s) = map
                .locate(me.pos, Some(back), 4.0, true, false)
                .map(|l| (l.seg, l.s))
                .or_else(|| wp_world::routes::nearest_lane(map, me.pos, Some(back)))?;
            self.stats.turn_rounds += 1;
            return Some((seg, (s + 6.0).min(map.seg(seg).length), vec![here]));
        }
        if let Some(path) = &self.path {
            let (s_now, _, _) = path.project(pos, None);
            let commit = (2.5 * me.speed_mps).max(40.0);
            let s_c = s_now + commit;
            // ...unless a known unit is on or near that stretch.
            let threatened = self.known.values().any(|k| {
                let (_, lat, along) = path.project(k.pos, None);
                lat.abs() < 30.0 && along >= s_now - 10.0 && along <= s_c + 30.0
            });
            if s_c < path.length() - 1.0 && !threatened {
                if let Some(k) = self.route.iter().rposition(|(_, st)| *st <= s_c) {
                    let (seg, st) = self.route[k];
                    let along = (s_c - st).clamp(0.0, map.seg(seg).length);
                    let mut pts = vec![here];
                    let mut d = s_now + 3.0;
                    while d < s_c {
                        pts.push(path.point_at(d).0);
                        d += 3.0;
                    }
                    return Some((seg, along, pts));
                }
            }
        }
        let (seg, s) = map
            .locate(me.pos, Some(fwd), 3.0, true, false)
            .map(|l| (l.seg, l.s))
            .or_else(|| wp_world::routes::nearest_lane(map, me.pos, Some(fwd)))?;
        Some((seg, (s + 4.0).min(map.seg(seg).length), vec![here]))
    }

    /// Plan a route from the current position (see the module docs).
    fn plan(&mut self, map: &CityMap, me: &VehicleState) {
        let Some((seg0, s0, prefix)) = self.plan_start(map, me) else {
            self.path = None;
            self.route.clear();
            return;
        };
        let p = self.params;
        // Threat positions a couple of seconds on (where they will be when we get near).
        let threats: Vec<V2> = self
            .known
            .values()
            .map(|k| {
                let age = (self.t - k.seen_at).min(3.0);
                geom::add(k.pos, geom::scale(k.vel, age + 1.0))
            })
            .collect();
        let penalty = |q: V2| -> f32 {
            let mut x = 0.0;
            for t in &threats {
                let d = geom::dist(q, *t);
                if d < p.threat_radius_m {
                    let f = 1.0 - d / p.threat_radius_m;
                    x += 40.0 * f * f;
                }
            }
            x
        };
        let speed_on = |seg: SegId| -> f32 {
            let sg = map.seg(seg);
            (sg.speed_limit * p.limit_factor)
                .max(p.min_target_mps)
                .min(p.top_speed_mps)
                .min(sg.curve_speed.max(8.0) * 1.6)
                .max(5.0)
        };
        // Dijkstra over segments: cost = seconds + threat penalty; distance tracked separately.
        let n = map.segs.len();
        let mut nodes = vec![PlanNode::UNSEEN; n];
        let first = map.seg(seg0);
        let rest = (first.length - s0).max(0.0);
        nodes[seg0 as usize] = PlanNode {
            cost: rest / speed_on(seg0),
            dist: rest,
            from: SegId::MAX,
            turns: 0,
            covered: map.covered_at(first.point_at(first.length).0).is_some(),
            done: false,
        };
        let mut heap = BinaryHeap::new();
        heap.push(Open {
            cost: nodes[seg0 as usize].cost,
            seg: seg0,
        });
        let mut settled: Vec<SegId> = Vec::new();
        while let Some(Open { seg: cur, .. }) = heap.pop() {
            let node = nodes[cur as usize];
            if node.done {
                continue;
            }
            nodes[cur as usize].done = true;
            settled.push(cur);
            if node.dist >= p.foresight_m {
                continue;
            }
            let sg = map.seg(cur);
            for &nx in &sg.next {
                let ns = map.seg(nx);
                if !ns.drivable && ns.lane().is_some() {
                    continue;
                }
                let (mid3, _) = ns.point_at(ns.length * 0.5);
                let mid = geom::xy(mid3);
                let c = node.cost + ns.length / speed_on(nx) + penalty(mid) * ns.length / 50.0;
                let turn = matches!(
                    ns.kind,
                    SegKind::Connector { turn, .. } if turn != TurnKind::Straight
                );
                let e = &mut nodes[nx as usize];
                if !e.done && c < e.cost {
                    *e = PlanNode {
                        cost: c,
                        dist: node.dist + ns.length,
                        from: cur,
                        turns: node.turns + u16::from(turn),
                        covered: node.covered || map.covered_at(mid3).is_some(),
                        done: false,
                    };
                    heap.push(Open { cost: c, seg: nx });
                }
            }
        }
        // Pick the best end: far from every known unit, through cover / turns, some noise.
        let min_dist = p.foresight_m * 0.5;
        let mut best: Option<(f32, SegId)> = None;
        for &sid in &settled {
            let nd = nodes[sid as usize];
            if nd.dist < min_dist.min(150.0) || map.seg(sid).lane().is_none() {
                continue;
            }
            let end = geom::xy(map.seg(sid).point_at(map.seg(sid).length).0);
            let clearance = threats
                .iter()
                .map(|t| geom::dist(end, *t))
                .fold(800.0f32, f32::min);
            let far_enough = if nd.dist >= min_dist { 0.0 } else { -200.0 };
            let score = clearance
                + far_enough
                + if nd.covered { p.cover_bonus_m } else { 0.0 }
                + p.turn_bonus_m * f32::from(nd.turns.min(6))
                - nd.cost * 2.0
                + self.rng.next_f32() * p.route_noise_m;
            if best.is_none_or(|b| score > b.0) {
                best = Some((score, sid));
            }
        }
        // Nothing far enough (dead end, island): drive whatever was reached furthest.
        let end = best.map(|b| b.1).or_else(|| {
            settled.iter().copied().max_by(|a, b| {
                nodes[*a as usize]
                    .dist
                    .total_cmp(&nodes[*b as usize].dist)
                    .then(b.cmp(a))
            })
        });
        let Some(end) = end else {
            self.path = None;
            self.route.clear();
            return;
        };
        let mut segs = vec![end];
        let mut cur = end;
        while cur != seg0 {
            cur = nodes[cur as usize].from;
            if cur == SegId::MAX || segs.len() > n {
                break;
            }
            segs.push(cur);
        }
        segs.reverse();
        if nodes[end as usize].covered {
            self.stats.cover_routes += 1;
        }
        // Polyline: the kept prefix, then the segments from `s0`; remember where each starts.
        let mut pts: Vec<V3> = prefix;
        let mut len = 0.0f32;
        for w in pts.windows(2) {
            len += geom::dist(geom::xy(w[0]), geom::xy(w[1]));
        }
        let mut route = Vec::with_capacity(segs.len());
        for (k, &sid) in segs.iter().enumerate() {
            let sg = map.seg(sid);
            let from = if k == 0 { s0 } else { 0.0 };
            let mut started = false;
            for (i, q) in sg.pts.iter().enumerate() {
                if sg.cum[i] < from {
                    continue;
                }
                let last = *pts.last().expect("prefix starts with the car");
                let d = geom::dist(geom::xy(last), geom::xy(*q));
                if d > 0.5 {
                    if !started {
                        // Arc length where this segment's (remaining) piece begins.
                        route.push((sid, len + d - (sg.cum[i] - from)));
                        started = true;
                    }
                    len += d;
                    pts.push(*q);
                }
            }
            if !started {
                route.push((sid, len));
            }
        }
        if pts.len() < 2 {
            self.path = None;
            self.route.clear();
            return;
        }
        self.path = Some(DrivePath::new(&pts, false));
        self.route = route;
        self.follower.reset(me.speed_mps);
        self.stats.plans += 1;
    }
}

/// Whether there is a lane (either direction) `side` m beside the path (+ right) 8 and 20 m
/// ahead of arc length `s`: a lane change does not drive onto a sidewalk or into parked cars.
fn lane_beside(map: &CityMap, path: &DrivePath, s: f32, side: f32) -> bool {
    [8.0f32, 20.0].iter().all(|ahead| {
        let (p, d) = path.point_at(s + ahead);
        let r = geom::right_of(d);
        let q = [p[0] + r[0] * side, p[1] + r[1] * side, p[2]];
        map.locate(q, None, 1.2, true, false).is_some()
    })
}

/// Route search state per segment ([`Evader::plan`]).
#[derive(Debug, Clone, Copy)]
struct PlanNode {
    cost: f32,
    dist: f32,
    from: SegId,
    turns: u16,
    covered: bool,
    done: bool,
}

impl PlanNode {
    const UNSEEN: Self = Self {
        cost: f32::INFINITY,
        dist: 0.0,
        from: SegId::MAX,
        turns: 0,
        covered: false,
        done: false,
    };
}

/// Min-heap entry (cheapest first, ties by segment id: deterministic).
#[derive(Debug, PartialEq)]
struct Open {
    cost: f32,
    seg: SegId,
}

impl Eq for Open {}

impl Ord for Open {
    fn cmp(&self, o: &Self) -> Ordering {
        o.cost.total_cmp(&self.cost).then(o.seg.cmp(&self.seg))
    }
}

impl PartialOrd for Open {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wp_world::procgen::{generate, ProcGenConfig};

    fn city() -> CityMap {
        CityMap::new(generate(&ProcGenConfig::small()))
    }

    /// A state on lane `seg` at arc length `s`, facing along it, at `speed`.
    fn on_lane(map: &CityMap, seg: SegId, s: f32, speed: f32) -> VehicleState {
        let (p, d) = map.seg(seg).point_at(s);
        // Yaw about +Z with 0 = +Y: forward (−sin, cos) = d.
        let yaw = geom::yaw_of(d);
        let (sy, cy) = wp_math::sin_cos(yaw * 0.5);
        VehicleState {
            pos: [p[0], p[1], p[2] + 0.5],
            rot: [0.0, 0.0, sy, cy],
            lin_vel: [d[0] * speed, d[1] * speed, 0.0],
            speed_mps: speed,
            health: 1.0,
            ..Default::default()
        }
    }

    /// A long straight drivable lane near the middle of the city.
    fn long_lane(map: &CityMap) -> SegId {
        (0..map.n_lanes() as SegId)
            .filter(|&i| map.seg(i).drivable && map.seg(i).length > 90.0)
            .min_by(|a, b| {
                let pa = map.seg(*a).point_at(0.0).0;
                let pb = map.seg(*b).point_at(0.0).0;
                (pa[0].abs() + pa[1].abs()).total_cmp(&(pb[0].abs() + pb[1].abs()))
            })
            .expect("a long lane")
    }

    fn def() -> VehicleDef {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/cars/debug_coupe.json"
        );
        wp_contracts::load_car(path).expect("debug_coupe").def
    }

    #[test]
    fn skills_get_quicker_and_see_further() {
        let [c, g, e] = EvaderSkill::ALL.map(EvaderSkill::params);
        assert!(c.reaction_s > g.reaction_s && g.reaction_s > e.reaction_s);
        assert!(c.foresight_m < g.foresight_m && g.foresight_m < e.foresight_m);
        assert!(c.max_lat_accel < g.max_lat_accel && g.max_lat_accel < e.max_lat_accel);
        assert!(c.steer_noise > g.steer_noise && g.steer_noise > e.steer_noise);
        for k in EvaderSkill::ALL {
            assert_eq!(EvaderSkill::from_name(k.name()), Some(k));
        }
    }

    #[test]
    fn drives_a_path_and_is_deterministic() {
        let map = city();
        let seg = long_lane(&map);
        let me = on_lane(&map, seg, 10.0, 12.0);
        let run = || {
            let mut ev = Evader::new(EvaderSkill::Good.params(), &def(), 7, 12.0);
            let inputs: Vec<VehicleInput> = (0..480)
                .map(|_| ev.step(&map, &me, &[], &[], 1.0 / 240.0))
                .collect();
            (
                inputs,
                ev.path().map(|p| p.length()),
                ev.stats(),
                ev.debug(),
            )
        };
        let (a, len, stats, _) = run();
        assert_eq!(a, run().0, "same seed, same inputs → same controls");
        let len = len.expect("a path");
        assert!(len >= 150.0, "path {len} m");
        assert!(stats.plans >= 1);
        assert!(
            a.iter().any(|i| i.throttle > 0.0),
            "it drives: {:?}",
            run().3
        );
    }

    #[test]
    fn plans_away_from_a_unit_it_can_see() {
        let map = city();
        let seg = long_lane(&map);
        let me = on_lane(&map, seg, 5.0, 0.0);
        // A police car 60 m up the same lane: in plain sight.
        let ahead = map.seg(seg).point_at(65.0).0;
        let cop = PoliceCar {
            id: 1,
            pos: [ahead[0], ahead[1], ahead[2] + 0.5],
            vel: [0.0; 3],
        };
        let mut ev = Evader::new(EvaderSkill::Expert.params(), &def(), 3, 0.0);
        for _ in 0..240 {
            ev.step(&map, &me, &[cop], &[], 1.0 / 240.0);
        }
        assert_eq!(
            ev.known_units(),
            1,
            "it saw the unit (after its reaction time)"
        );
        let path = ev.path().expect("a path");
        let (end, _) = path.point_at(path.length());
        let d_end = geom::dist(geom::xy(end), geom::xy(cop.pos));
        assert!(d_end > 250.0, "plan ends {d_end:.0} m from the unit");
    }

    #[test]
    fn does_not_see_through_its_reaction_time() {
        let map = city();
        let seg = long_lane(&map);
        let me = on_lane(&map, seg, 5.0, 0.0);
        let ahead = map.seg(seg).point_at(65.0).0;
        let cop = PoliceCar {
            id: 1,
            pos: [ahead[0], ahead[1], ahead[2] + 0.5],
            vel: [0.0; 3],
        };
        let p = EvaderSkill::Casual.params();
        let mut ev = Evader::new(p, &def(), 3, 0.0);
        let early = ((p.reaction_s - 0.2) * 240.0) as usize;
        for _ in 0..early {
            ev.step(&map, &me, &[cop], &[], 1.0 / 240.0);
        }
        assert_eq!(
            ev.known_units(),
            0,
            "not yet: reaction time {} s",
            p.reaction_s
        );
        for _ in 0..120 {
            ev.step(&map, &me, &[cop], &[], 1.0 / 240.0);
        }
        assert_eq!(ev.known_units(), 1);
    }
}
