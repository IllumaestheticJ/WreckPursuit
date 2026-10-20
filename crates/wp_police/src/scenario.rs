//! Headless police scenarios on `wp_world`'s procedural city (`crates/wp_police/scenarios/*.ron`).
//!
//! A scenario scripts one or more racers (roam a district at a multiple of the speed limit,
//! stop, run for a safehouse, total their car) and checks metric **ranges** after the run. With
//! `runs > 1` the scenario is repeated with seeds `seed, seed+1, …` and metrics are aggregated
//! (times averaged, counts summed, rates pooled). Speeding violations are generated here from
//! the racer's speed vs the segment limit, standing in for `wp_world`'s violation detector.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use wp_contracts::{
    DamageMode, EntityId, SignalState, SimEvent, SurfaceType, TimeOfDay, ViolationKind, WorldQuery,
    PHYSICS_DT,
};
use wp_math::{Rng, StateHasher};
use wp_world::procgen::ProcGenConfig;
use wp_world::CityMap;

use crate::config::PoliceConfig;
use crate::events::{PoliceEvent, PoliceEventExt, UnitKind};
use crate::graph::{dist, lerp2, sub, District, NodeId, PoiKind, RoadClass, RoadGraph};
use crate::unit::{
    plan_unit_speed, step_ground, DriveCommand, DriveEnv, Track, Unit, UnitRole, LANE_OFFSET_M,
};
use crate::{PoliceSetup, PoliceSystem, RacerView};

fn d_core() -> District {
    District::Core
}
fn d_two() -> f32 {
    2.0
}
fn d_corner() -> f32 {
    0.85
}
/// Scripted sideswipe impulse = this × the racer's speed, kg. Matches `wp_sim` with real
/// cars (debug_sedan into a stopped sentinel_cruiser): 30 km/h ≈ 6,700 N·s, 50 km/h ≈
/// 12,000 N·s, 5 km/h ≈ 860 N·s (see `data/police/heat.toml` `[heat.assault]`).
const SCRIPTED_HIT_MASS_KG: f32 = 800.0;

fn d_hit_kmh() -> f32 {
    30.0
}
fn d_true() -> bool {
    true
}
fn d_one() -> u32 {
    1
}

/// Scripted racer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RacerScript {
    /// District to start in and roam.
    #[serde(default = "d_core")]
    pub district: District,
    /// Target speed as a multiple of the local limit.
    #[serde(default = "d_two")]
    pub speed_factor: f32,
    /// Planning lateral acceleration, g.
    #[serde(default = "d_corner")]
    pub cornering_g: f32,
    /// Heat at t = 0.
    #[serde(default)]
    pub initial_heat: f32,
    /// Emit speeding violations when over the limit.
    #[serde(default = "d_true")]
    pub violations: bool,
    /// Stay in `district` when choosing destinations (otherwise roam the whole city).
    #[serde(default = "d_true")]
    pub stay_in_district: bool,
    /// Brake to a stop and stay stopped from this time, s.
    #[serde(default)]
    pub stop_at_s: Option<f32>,
    /// Drive to the nearest safehouse from this time and park inside, s.
    #[serde(default)]
    pub safehouse_at_s: Option<f32>,
    /// Emit `TotalLoss` for this racer at this time and stop, s.
    #[serde(default)]
    pub total_loss_at_s: Option<f32>,
    /// Sideswipe the nearest in-service patrol car at this time, s (playtest row 24): the
    /// patrol car is paused where it is (as at a junction), the racer is put alongside it at
    /// `hit_kmh` and keeps that speed from then on.
    #[serde(default)]
    pub hit_unit_at_s: Option<f32>,
    /// Racer speed for `hit_unit_at_s`, km/h.
    #[serde(default = "d_hit_kmh")]
    pub hit_kmh: f32,
}

/// Expected metric range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    /// Metric name (see [`PoliceRunSummary::metrics`]).
    pub metric: String,
    /// Inclusive minimum.
    pub min: f32,
    /// Inclusive maximum.
    pub max: f32,
}

/// A police scenario.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PoliceScenario {
    /// Name.
    pub name: String,
    /// Base seed (police RNG, racer starts, routes).
    pub seed: u64,
    /// `wp_world` procedural city to run on.
    #[serde(default)]
    pub city: ScenarioCity,
    /// Damage mode.
    #[serde(default)]
    pub mode: DamageMode,
    /// Start time of day, hours.
    pub time_of_day_h: f32,
    /// In-game seconds per sim second (0 = frozen clock).
    #[serde(default)]
    pub time_ratio: f32,
    /// Duration per run, s.
    pub duration_s: f32,
    /// Repetitions (seed, seed+1, …).
    #[serde(default = "d_one")]
    pub runs: u32,
    /// Stop a run as soon as the first pursuit ends.
    #[serde(default)]
    pub stop_after_first_pursuit: bool,
    /// Racers.
    pub racers: Vec<RacerScript>,
    /// Expected ranges.
    #[serde(default)]
    pub expect: Vec<Expect>,
}

/// Result of a scenario (all runs).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PoliceRunSummary {
    /// Scenario name.
    pub name: String,
    /// Combined state hash of every run (determinism).
    pub state_hash: u64,
    /// Aggregated metrics.
    pub metrics: BTreeMap<String, f32>,
    /// Failed expectations. Empty = pass.
    pub failures: Vec<String>,
}

impl PoliceRunSummary {
    /// Metric by name.
    pub fn metric(&self, name: &str) -> Option<f32> {
        self.metrics.get(name).copied()
    }
}

/// Scenario load error.
#[derive(Debug, thiserror::Error)]
pub enum ScenarioError {
    /// Unreadable.
    #[error("{0}: {1}")]
    Io(String, std::io::Error),
    /// Malformed.
    #[error("{0}: {1}")]
    Parse(String, String),
}

/// Parse a scenario file.
pub fn load_scenario(path: &Path) -> Result<PoliceScenario, ScenarioError> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| ScenarioError::Io(path.display().to_string(), e))?;
    ron::from_str(&text)
        .map_err(|e| ScenarioError::Parse(path.display().to_string(), e.to_string()))
}

/// `wp_world` procedural city size and seed (`ProcGenConfig` with the remaining fields at
/// their defaults). The default is `ProcGenConfig::small()`; the safehouse scenario needs the
/// full 12×12 city (the small one has no residential district).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScenarioCity {
    /// Grid columns.
    pub nx: usize,
    /// Grid rows.
    pub ny: usize,
    /// Generator seed.
    pub seed: u64,
}

impl Default for ScenarioCity {
    fn default() -> Self {
        let small = ProcGenConfig::small();
        Self {
            nx: small.nx,
            ny: small.ny,
            seed: small.seed,
        }
    }
}

impl ScenarioCity {
    /// The generator config.
    pub fn procgen(&self) -> ProcGenConfig {
        ProcGenConfig {
            nx: self.nx,
            ny: self.ny,
            seed: self.seed,
            ..ProcGenConfig::default()
        }
    }
}

/// Minimal world for scenarios: the clock, plus line of sight from the `wp_world` city.
#[derive(Debug, Clone)]
pub struct ScenarioWorld {
    /// Time of day.
    pub time: TimeOfDay,
    /// Clock ratio.
    pub ratio: f32,
    /// City whose buildings block line of sight (`None`: nothing blocks).
    pub map: Option<Arc<CityMap>>,
}

impl ScenarioWorld {
    /// Clock-only world.
    pub fn clock(time: TimeOfDay, ratio: f32) -> Self {
        Self {
            time,
            ratio,
            map: None,
        }
    }
}

impl WorldQuery for ScenarioWorld {
    fn surface_at(&self, _p: [f32; 3]) -> SurfaceType {
        SurfaceType::Asphalt
    }
    fn speed_limit_at(&self, _p: [f32; 3]) -> Option<f32> {
        None
    }
    fn signal_state_ahead(&self, _p: [f32; 3], _h: [f32; 3]) -> Option<SignalState> {
        None
    }
    fn time_of_day(&self) -> TimeOfDay {
        self.time
    }
    fn line_of_sight(&self, a: [f32; 3], b: [f32; 3]) -> bool {
        self.map.as_ref().is_none_or(|m| m.line_of_sight(a, b))
    }
}

struct ScriptedRacer {
    entity: EntityId,
    script: RacerScript,
    body: Unit,
    total_lost: bool,
    to_safehouse: bool,
    /// When the scripted sideswipe happened.
    hit_at: Option<f32>,
}

/// Per-run raw numbers.
#[derive(Debug, Clone, Default)]
struct RunNumbers {
    pursuit_start_s: Option<f32>,
    pursuit_end_s: Option<f32>,
    reinforcement_delays: Vec<f32>,
    escapes: u32,
    safehouse: u32,
    captures: u32,
    total_loss_captures: u32,
    pursuits: u32,
    units_engaged: u32,
    self_wrecks: u32,
    contact_wrecks: u32,
    pulled_over: u32,
    tool_sight: Vec<f32>,
    spike_hits: u32,
    helo_launches: u32,
    near_misses: u32,
    pursuit_harm: u32,
    pursuit_high_g_s: f32,
    pursuit_no_risk_s: f32,
    traffic_collisions: u32,
    run_wide: u32,
    pool: [u32; 3],
    max_level: u8,
    /// Scripted sideswipe → pursuit start, s.
    hit_to_pursuit_s: Option<f32>,
    /// Scripted sideswipes done.
    hits: u32,
    /// Contacts counted as assaulting an officer.
    assaults: u32,
    /// Radio lines while no racer is wanted / while some racer is in a pursuit.
    radio_calm: u32,
    radio_hot: u32,
    /// Seconds with no racer wanted / with some racer in a pursuit.
    calm_s: f32,
    hot_s: f32,
    /// First reckless-driving call → first ground unit within [`ON_SCENE_M`] of that racer, s.
    call_to_unit_s: Option<f32>,
    /// Reckless-driving calls, and how many of them spawned a responder.
    calls: u32,
    responder_spawns: u32,
    /// Ground units that appeared after the session start (pool refills, responders,
    /// reinforcements), and those that did so closer than `never_closer_m` to a racer or in
    /// its view cone with line of sight.
    late_spawns: u32,
    spawns_in_view: u32,
    /// Ground units that despawned while a racer could see them or was within
    /// `never_closer_m`.
    despawns_in_sight: u32,
    /// Ground units that despawned.
    despawns: u32,
    /// Closest late spawn to a racer, m.
    spawn_min_racer_m: f32,
    hash: u64,
}

/// "On scene": a unit this close to the reported racer, m (lead feedback round 12b).
const ON_SCENE_M: f32 = 100.0;

/// Half-angle of a racer's view cone for the pop-in audit (chase camera plus a margin), rad.
const VIEW_HALF_ANGLE_RAD: f32 = 1.05;

fn random_node(rng: &mut Rng, g: &RoadGraph, d: Option<District>) -> NodeId {
    let cands: Vec<u32> = (0..g.nodes.len() as u32)
        .filter(|&n| d.is_none_or(|d| g.nodes[n as usize].district == d))
        .collect();
    NodeId(cands[rng.below(cands.len() as u32) as usize])
}

fn racer_route(rng: &mut Rng, g: &RoadGraph, r: &ScriptedRacer) -> Track {
    let pos = r.body.body.pos;
    let rp = g.nearest_on_road(pos).expect("city has roads");
    let seg = *g.segment(rp.segment);
    let ab = sub(g.node(seg.b).pos, g.node(seg.a).pos);
    let start = if crate::graph::dot(ab, r.body.body.heading) >= 0.0 {
        seg.b
    } else {
        seg.a
    };
    // Don't drive to the node ahead only to come straight back: turn around now instead.
    let behind = g.other_end(rp.segment, start);
    let route_pts = |route: Vec<NodeId>| {
        let skip = route.len() >= 2 && route[1] == behind;
        let mut pts = vec![pos];
        pts.extend(route.iter().skip(skip as usize).map(|n| g.node(*n).pos));
        pts
    };
    let dest = if r.to_safehouse {
        let sh = g
            .pois_of(PoiKind::Safehouse)
            .min_by(|a, b| dist(a.pos, pos).total_cmp(&dist(b.pos, pos)))
            .expect("city has safehouses");
        let n = g.nearest_node(sh.pos).expect("nodes");
        let mut pts = route_pts(g.shortest_path(start, n).unwrap_or_default());
        pts.push(sh.pos);
        return Track::new(pts);
    } else {
        let d = r.script.stay_in_district.then_some(r.script.district);
        let mut dest = random_node(rng, g, d);
        if dest == start {
            dest = random_node(rng, g, d);
        }
        dest
    };
    Track::new(route_pts(g.shortest_path(start, dest).unwrap_or_default()))
}

fn run_once(
    sc: &PoliceScenario,
    seed: u64,
    cfg: &PoliceConfig,
    hook: &mut dyn FnMut(&PoliceSystem, &[SimEvent]),
) -> RunNumbers {
    let (graph, map) = crate::city::procedural(&sc.city.procgen());
    let map = Some(Arc::new(map));
    let setup = PoliceSetup {
        seed,
        mode: sc.mode,
        internal_driving: true,
        hazards: true,
    };
    let mut police = PoliceSystem::with_city(cfg.clone(), graph.clone(), setup);
    let mut world = ScenarioWorld {
        time: TimeOfDay::from_hours(sc.time_of_day_h),
        ratio: sc.time_ratio,
        map,
    };
    let mut rng = Rng::new(seed ^ 0x5CE7_A210);
    let mut racers: Vec<ScriptedRacer> = Vec::new();
    for (k, script) in sc.racers.iter().enumerate() {
        let cands: Vec<usize> = (0..graph.segments.len())
            .filter(|&s| {
                graph.segments[s].district == script.district
                    && graph.segments[s].class != RoadClass::Highway
            })
            .collect();
        let s = graph.segments[cands[rng.below(cands.len() as u32) as usize]];
        let a = graph.node(s.a).pos;
        let b = graph.node(s.b).pos;
        let entity = EntityId(1000 + k as u32);
        let mut body = Unit::new(
            crate::events::UnitId(0),
            UnitKind::Car,
            UnitRole::Player,
            lerp2(a, b, 0.5),
            sub(b, a),
            0.0,
        );
        body.body.speed = s.speed_limit_mps * script.speed_factor * 0.5;
        if script.initial_heat > 0.0 {
            police.set_heat(entity, script.initial_heat);
        }
        racers.push(ScriptedRacer {
            entity,
            script: script.clone(),
            body,
            total_lost: false,
            to_safehouse: false,
            hit_at: None,
        });
    }
    let ticks = (sc.duration_s / PHYSICS_DT).round() as u64;
    let think_every = 24u64;
    let mut out = Vec::new();
    // Scripted sideswipes placed this tick, reported after the next step (see below).
    let mut placed: Vec<(crate::UnitId, EntityId, f32, f32)> = Vec::new();
    let mut nums = RunNumbers {
        spawn_min_racer_m: f32::MAX,
        ..RunNumbers::default()
    };
    let mut spawns_seen = 0usize;
    let env_template = DriveEnv {
        limit_mps: 13.9,
        phase: world.time.phase(),
        mode: sc.mode,
        hazards: false,
    };
    for tick in 0..ticks {
        let t = tick as f32 * PHYSICS_DT;
        let mut events = Vec::new();
        let mut hits = Vec::new();
        let due = std::mem::take(&mut placed);
        for r in racers.iter_mut() {
            // Script triggers.
            if let Some(ts) = r.script.total_loss_at_s {
                if t >= ts && !r.total_lost {
                    r.total_lost = true;
                    events.push(SimEvent::TotalLoss { vehicle: r.entity });
                }
            }
            if let Some(ts) = r.script.safehouse_at_s {
                if t >= ts && !r.to_safehouse {
                    r.to_safehouse = true;
                    r.body.cmd.track = Track::default();
                }
            }
            if let Some(ts) = r.script.hit_unit_at_s {
                if t >= ts && r.hit_at.is_none() {
                    if let Some(u) = police
                        .units
                        .iter_mut()
                        .filter(|u| {
                            u.kind == UnitKind::Car
                                && u.internal
                                && !u.wrecked
                                && u.player.is_none()
                                && u.assignment != crate::Assignment::OutOfService
                        })
                        .min_by(|a, b| {
                            dist(a.body.pos, r.body.body.pos)
                                .total_cmp(&dist(b.body.pos, r.body.body.pos))
                                .then(a.id.cmp(&b.id))
                        })
                    {
                        // Paused patrol car; the racer's flank against its left side.
                        u.body.speed = 0.0;
                        let h = u.body.heading;
                        let left = [-h[1], h[0]];
                        r.body.body.pos =
                            [u.body.pos[0] + left[0] * 2.0, u.body.pos[1] + left[1] * 2.0];
                        r.body.body.heading = h;
                        r.body.body.speed = r.script.hit_kmh / 3.6;
                        r.body.cmd.track = Track::default();
                        r.hit_at = Some(t);
                        nums.hits += 1;
                        let v = r.script.hit_kmh / 3.6;
                        hits.push((u.id, r.entity, SCRIPTED_HIT_MASS_KG * v, v));
                    }
                }
            }
            let stopped = r.total_lost || r.script.stop_at_s.is_some_and(|s| t >= s);
            if tick % think_every == 0 {
                let limit = graph
                    .nearest_on_road(r.body.body.pos)
                    .map(|p| graph.segment(p.segment).speed_limit_mps)
                    .unwrap_or(13.9);
                r.body.limit_mps = limit;
                let arrived = r.body.track_s > r.body.cmd.track.length() - 10.0;
                if !r.body.cmd.track.is_drivable() || (arrived && !r.to_safehouse) {
                    let tr = racer_route(&mut rng, &graph, r);
                    r.body.set_command(DriveCommand {
                        track: tr,
                        speed_mps: 0.0,
                        cornering_g: r.script.cornering_g,
                        siren: false,
                        lane_offset_m: LANE_OFFSET_M,
                    });
                }
                r.body.cmd.speed_mps = if stopped {
                    0.0
                } else if r.hit_at.is_some() {
                    r.script.hit_kmh / 3.6
                } else {
                    limit * r.script.speed_factor
                };
                r.body.planned_speed = plan_unit_speed(&r.body, 8.0);
            }
            let env = DriveEnv {
                limit_mps: r.body.limit_mps,
                ..env_template
            };
            step_ground(&mut r.body, &env, &cfg.pursuit, &mut rng, PHYSICS_DT);
            r.body.health = 1.0;
            if r.script.violations && r.body.body.speed > r.body.limit_mps + 1.0 {
                events.push(SimEvent::TrafficViolation {
                    vehicle: r.entity,
                    kind: ViolationKind::Speeding {
                        over_mps: r.body.body.speed - r.body.limit_mps,
                    },
                    pos: [r.body.body.pos[0], r.body.body.pos[1], 0.0],
                });
            }
        }
        let views: Vec<RacerView> = racers
            .iter()
            .map(|r| {
                let v = r.body.body.vel();
                RacerView {
                    entity: r.entity,
                    pos: [r.body.body.pos[0], r.body.body.pos[1], 0.0],
                    vel: [v[0], v[1], 0.0],
                }
            })
            .collect();
        world.time = world.time.advanced(PHYSICS_DT * world.ratio);
        // Ground unit positions before the step, for the despawn audit.
        let before: BTreeMap<crate::UnitId, [f32; 2]> = police
            .units
            .iter()
            .filter(|u| u.kind == UnitKind::Car)
            .map(|u| (u.id, u.body.pos))
            .collect();
        out.clear();
        police.step(&world, &views, &events, PHYSICS_DT, &mut out);
        for e in &out {
            if let Some(PoliceEvent::UnitDespawned { unit }) = PoliceEvent::from_sim_event(e) {
                if let Some(p) = before.get(&unit) {
                    nums.despawns += 1;
                    let seen = racers.iter().any(|r| {
                        let rp = r.body.body.pos;
                        dist(rp, *p) < cfg.presence.response.never_closer_m
                            || police.sees(&world, rp, *p)
                    });
                    nums.despawns_in_sight += seen as u32;
                }
            }
        }
        // Scripted sideswipes: the contact `wp_sim` would report. It comes one step after the
        // racer was placed, so police have seen it at the placed speed twice (police also
        // look at the previous step's speed, which would otherwise be from before the move).
        for (unit, racer, impulse_ns, rel_speed) in due {
            police.scripted_contact(&world, unit, racer, impulse_ns, rel_speed);
        }
        placed = hits;
        hook(&police, &out);
        // Response time: first call → a ground unit within ON_SCENE_M of that racer.
        if nums.call_to_unit_s.is_none() {
            if let Some(call) = police.stats().calls.first() {
                let on_scene = racers
                    .iter()
                    .find(|r| r.entity == call.racer)
                    .is_some_and(|r| {
                        police.units.iter().any(|u| {
                            u.kind == UnitKind::Car
                                && !u.wrecked
                                && u.player.is_none()
                                && u.role != UnitRole::Tool
                                && dist(u.body.pos, r.body.body.pos) <= ON_SCENE_M
                        })
                    });
                if on_scene {
                    nums.call_to_unit_s = Some(police.now().since(call.at));
                }
            }
        }
        // Pop-in audit of every ground unit that appeared this tick.
        let spawns = &police.stats().spawns;
        for sp in &spawns[spawns_seen..] {
            nums.late_spawns += 1;
            nums.responder_spawns += (sp.role == UnitRole::Responder) as u32;
            nums.spawn_min_racer_m = nums.spawn_min_racer_m.min(sp.nearest_racer_m);
            let in_view = racers.iter().any(|r| {
                let rp = r.body.body.pos;
                let to = sub(sp.pos, rp);
                let d = crate::graph::len(to);
                let h = r.body.body.heading;
                let cos = if d > 0.0 {
                    crate::graph::dot(to, h) / d
                } else {
                    1.0
                };
                d < cfg.presence.response.never_closer_m
                    || (cos >= wp_math::sin_cos(VIEW_HALF_ANGLE_RAD).1
                        && police.sees(&world, rp, sp.pos))
            });
            nums.spawns_in_view += in_view as u32;
        }
        spawns_seen = spawns.len();
        let mut wanted = false;
        let mut hot = false;
        for r in &racers {
            let level = police.wanted_level(r.entity).number();
            nums.max_level = nums.max_level.max(level);
            wanted |= level > 0;
            hot |= police.racer(r.entity).is_some_and(|rec| rec.pursued());
        }
        let lines = out
            .iter()
            .filter(|e| {
                matches!(
                    PoliceEvent::from_sim_event(e),
                    Some(PoliceEvent::Radio { .. })
                )
            })
            .count() as u32;
        if hot {
            nums.hot_s += PHYSICS_DT;
            nums.radio_hot += lines;
        } else if !wanted {
            nums.calm_s += PHYSICS_DT;
            nums.radio_calm += lines;
        }
        if sc.stop_after_first_pursuit && police.stats().pursuits.iter().any(|p| p.end.is_some()) {
            break;
        }
    }
    let st = police.stats();
    nums.calls = st.calls.len() as u32;
    nums.pursuit_start_s = st.pursuits.first().map(|p| p.start.secs());
    nums.assaults = st.assaults;
    if let Some(hit) = racers.iter().find_map(|r| r.hit_at) {
        nums.hit_to_pursuit_s = st
            .pursuits
            .iter()
            .map(|p| p.start.secs())
            .find(|s| *s >= hit - PHYSICS_DT)
            .map(|s| s - hit);
    }
    nums.pursuit_end_s = st.pursuits.iter().find_map(|p| p.end).map(|t| t.secs());
    nums.reinforcement_delays = st.reinforcement_delays_s.clone();
    for p in &st.pursuits {
        match p.outcome {
            Some(crate::PursuitOutcome::Escaped) => nums.escapes += 1,
            Some(crate::PursuitOutcome::Safehouse) => nums.safehouse += 1,
            Some(crate::PursuitOutcome::Captured) => nums.captures += 1,
            Some(crate::PursuitOutcome::CapturedTotalLoss) => nums.total_loss_captures += 1,
            _ => {}
        }
    }
    nums.pursuits = st
        .pursuits
        .iter()
        .filter(|p| p.end != Some(p.start))
        .count() as u32;
    nums.units_engaged = st.units_engaged;
    nums.self_wrecks = st.self_wrecks;
    nums.contact_wrecks = st.contact_wrecks;
    nums.pulled_over = st.pulled_over;
    nums.tool_sight = st.tool_sight_m.clone();
    nums.spike_hits = st.spike_hits;
    nums.helo_launches = st.helo_launches;
    nums.near_misses = st.near_misses;
    nums.pursuit_harm = st.pursuit_harm;
    nums.pursuit_high_g_s = st.pursuit_high_g_s;
    nums.pursuit_no_risk_s = st.pursuit_no_risk_s;
    nums.traffic_collisions = st.traffic_collisions;
    nums.run_wide = st.run_wide_crashes;
    for (k, d) in District::ALL.iter().enumerate() {
        nums.pool[k] = police.pool_count(*d);
    }
    let mut h = StateHasher::new();
    h.write_u64(police.state_hash());
    for r in &racers {
        h.write_f32_q(r.body.body.pos[0], 1e3);
        h.write_f32_q(r.body.body.pos[1], 1e3);
    }
    nums.hash = h.finish();
    nums
}

fn mean(v: &[f32]) -> Option<f32> {
    (!v.is_empty()).then(|| v.iter().sum::<f32>() / v.len() as f32)
}

/// Run a scenario with the built-in tables.
pub fn run_scenario(sc: &PoliceScenario) -> PoliceRunSummary {
    run_scenario_with(sc, &PoliceConfig::builtin())
}

/// Run a scenario with explicit tables.
pub fn run_scenario_with(sc: &PoliceScenario, cfg: &PoliceConfig) -> PoliceRunSummary {
    run_scenario_with_hook(sc, cfg, &mut |_, _| {})
}

/// Run a scenario with explicit tables, calling `hook` after every police step with the
/// system and the events it emitted that tick (e.g. to print the radio log).
pub fn run_scenario_with_hook(
    sc: &PoliceScenario,
    cfg: &PoliceConfig,
    hook: &mut dyn FnMut(&PoliceSystem, &[SimEvent]),
) -> PoliceRunSummary {
    let runs: Vec<RunNumbers> = (0..sc.runs.max(1))
        .map(|k| run_once(sc, sc.seed + k as u64, cfg, hook))
        .collect();
    let never = sc.duration_s + 1.0;
    let mut m = BTreeMap::new();
    let starts: Vec<f32> = runs
        .iter()
        .map(|r| r.pursuit_start_s.unwrap_or(never))
        .collect();
    m.insert(
        "pursuit_start_s".to_string(),
        mean(&starts).unwrap_or(never),
    );
    m.insert(
        "pursuit_start_max_s".to_string(),
        starts.iter().copied().fold(0.0, f32::max),
    );
    m.insert(
        "pursuit_start_min_s".to_string(),
        starts.iter().copied().fold(f32::INFINITY, f32::min),
    );
    let ends: Vec<f32> = runs.iter().filter_map(|r| r.pursuit_end_s).collect();
    m.insert("pursuit_end_s".to_string(), mean(&ends).unwrap_or(never));
    let delays: Vec<f32> = runs
        .iter()
        .flat_map(|r| r.reinforcement_delays.iter().copied())
        .collect();
    m.insert(
        "reinforcement_delay_s".to_string(),
        mean(&delays).unwrap_or(-1.0),
    );
    m.insert("reinforcements".to_string(), delays.len() as f32);
    let sum = |f: &dyn Fn(&RunNumbers) -> u32| runs.iter().map(f).sum::<u32>() as f32;
    m.insert("escapes".to_string(), sum(&|r| r.escapes));
    m.insert("safehouse_escapes".to_string(), sum(&|r| r.safehouse));
    m.insert("captures".to_string(), sum(&|r| r.captures));
    m.insert(
        "total_loss_captures".to_string(),
        sum(&|r| r.total_loss_captures),
    );
    m.insert("pursuits".to_string(), sum(&|r| r.pursuits));
    let engaged = sum(&|r| r.units_engaged);
    let selfw = sum(&|r| r.self_wrecks);
    m.insert("units_engaged".to_string(), engaged);
    m.insert("self_wrecks".to_string(), selfw);
    m.insert("contact_wrecks".to_string(), sum(&|r| r.contact_wrecks));
    m.insert("pulled_over".to_string(), sum(&|r| r.pulled_over));
    m.insert(
        "self_wreck_rate".to_string(),
        if engaged > 0.0 { selfw / engaged } else { 0.0 },
    );
    let sights: Vec<f32> = runs
        .iter()
        .flat_map(|r| r.tool_sight.iter().copied())
        .collect();
    m.insert("tools_placed".to_string(), sights.len() as f32);
    m.insert(
        "min_tool_sight_m".to_string(),
        sights.iter().copied().fold(9999.0, f32::min),
    );
    m.insert("spike_hits".to_string(), sum(&|r| r.spike_hits));
    m.insert("helo_launches".to_string(), sum(&|r| r.helo_launches));
    m.insert("near_misses".to_string(), sum(&|r| r.near_misses));
    m.insert("pursuit_harm".to_string(), sum(&|r| r.pursuit_harm));
    m.insert(
        "pursuit_high_g_s".to_string(),
        runs.iter().map(|r| r.pursuit_high_g_s).sum(),
    );
    m.insert(
        "pursuit_no_risk_s".to_string(),
        runs.iter().map(|r| r.pursuit_no_risk_s).sum(),
    );
    m.insert(
        "traffic_collisions".to_string(),
        sum(&|r| r.traffic_collisions),
    );
    m.insert("run_wide_crashes".to_string(), sum(&|r| r.run_wide));
    m.insert("hits".to_string(), sum(&|r| r.hits));
    m.insert("assaults".to_string(), sum(&|r| r.assaults));
    // Slowest scripted sideswipe → pursuit over runs (`never` if any run had none).
    m.insert(
        "hit_to_pursuit_max_s".to_string(),
        runs.iter()
            .filter(|r| r.hits > 0)
            .map(|r| r.hit_to_pursuit_s.unwrap_or(never))
            .fold(-1.0, f32::max),
    );
    m.insert(
        "max_wanted_level".to_string(),
        runs.iter().map(|r| r.max_level).max().unwrap_or(0) as f32,
    );
    // Radio rates, pooled over runs (lines per minute of sim time).
    let per_min = |lines: u32, secs: f32| {
        if secs > 0.0 {
            lines as f32 * 60.0 / secs
        } else {
            0.0
        }
    };
    let calm_s: f32 = runs.iter().map(|r| r.calm_s).sum();
    let hot_s: f32 = runs.iter().map(|r| r.hot_s).sum();
    m.insert(
        "radio_per_min_calm".to_string(),
        per_min(sum(&|r| r.radio_calm) as u32, calm_s),
    );
    m.insert(
        "radio_per_min_pursuit".to_string(),
        per_min(sum(&|r| r.radio_hot) as u32, hot_s),
    );
    m.insert("calm_s".to_string(), calm_s);
    m.insert("pursuit_s".to_string(), hot_s);
    // Dispatch-driven response (round 12b): runs with a call only; `never` if a run's first
    // call never got a unit within ON_SCENE_M, -1 if no run had a call.
    let responses: Vec<f32> = runs
        .iter()
        .filter(|r| r.calls > 0)
        .map(|r| r.call_to_unit_s.unwrap_or(never))
        .collect();
    m.insert(
        "call_to_unit_100m_s".to_string(),
        mean(&responses).unwrap_or(-1.0),
    );
    m.insert(
        "call_to_unit_100m_max_s".to_string(),
        responses.iter().copied().fold(-1.0, f32::max),
    );
    // Share of runs with a call whose first unit got within ON_SCENE_M inside 25 s (lead
    // round 12b target: ≥ 90 %).
    m.insert(
        "call_reached_25s_frac".to_string(),
        if responses.is_empty() {
            -1.0
        } else {
            responses.iter().filter(|v| **v <= 25.0).count() as f32 / responses.len() as f32
        },
    );
    m.insert("calls".to_string(), sum(&|r| r.calls));
    m.insert("responder_spawns".to_string(), sum(&|r| r.responder_spawns));
    m.insert("late_spawns".to_string(), sum(&|r| r.late_spawns));
    m.insert("spawns_in_view".to_string(), sum(&|r| r.spawns_in_view));
    m.insert("despawns".to_string(), sum(&|r| r.despawns));
    m.insert(
        "despawns_in_sight".to_string(),
        sum(&|r| r.despawns_in_sight),
    );
    m.insert(
        "spawn_min_racer_m".to_string(),
        runs.iter()
            .map(|r| r.spawn_min_racer_m)
            .fold(9999.0, f32::min),
    );
    let last = runs.last().expect("at least one run");
    m.insert("patrols_core".to_string(), last.pool[0] as f32);
    m.insert("patrols_suburb".to_string(), last.pool[1] as f32);
    m.insert("patrols_industrial".to_string(), last.pool[2] as f32);
    let mut h = StateHasher::new();
    for r in &runs {
        h.write_u64(r.hash);
    }
    let mut summary = PoliceRunSummary {
        name: sc.name.clone(),
        state_hash: h.finish(),
        metrics: m,
        failures: Vec::new(),
    };
    for e in &sc.expect {
        match summary.metric(&e.metric) {
            Some(v) if v >= e.min && v <= e.max => {}
            Some(v) => summary
                .failures
                .push(format!("{} = {v} not in [{}, {}]", e.metric, e.min, e.max)),
            None => summary
                .failures
                .push(format!("unknown metric {}", e.metric)),
        }
    }
    summary
}
