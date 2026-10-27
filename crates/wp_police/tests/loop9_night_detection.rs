//! Owner report 2026-10-27: "Cops on Loop 9 night take too long to detect me if I'm going
//! 185 mph." Headless measurement of how fast the police notice a racer at 50 / 100 / 150 /
//! 185 mph on Loop 9 at night (the `loop9_circuit` race's police), Loop 9 by day, and the
//! Port Vesper streets at night.
//!
//! The racer is kinematic: it drives the course at a constant speed, on the road surface, with
//! nothing in its way. Units are the police's own headless (internally driven) cars. For each
//! sighting episode (a unit has the racer in sight, until no unit does) the run records when
//! it began, when the racer became wanted and when a pursuit started. An episode that ends
//! with no pursuit is a unit the racer drove past unnoticed ("missed").
//!
//! `cargo test -p wp_police --release --test loop9_night_detection -- --ignored --nocapture`
//! prints the full table (`docs/integration/police-loop9-night.md`). The non-ignored tests
//! are the acceptance checks.

use wp_contracts::{EntityId, SimEvent, TimeOfDay, ViolationKind, PHYSICS_DT};
use wp_police::race::{RaceCourse, RaceProfile, RaceProfiles};
use wp_police::{PoliceConfig, PoliceEvent, PoliceSetup, PoliceSystem, RacerView};
use wp_world::World;

const RACER: EntityId = EntityId(1);
const MPH: f32 = 0.44704;
/// Lateral gap the racer keeps from a unit it overtakes, m (one lane).
const PASS_OFFSET_M: f32 = 3.7;

/// The `loop9_circuit` course (wp_race::catalog `LOOP9`, driving order).
const LOOP9: &[[f32; 2]] = &[
    [306.0, 1413.0],
    [-1090.0, 1409.0],
    [-1521.0, 633.0],
    [-1502.0, -365.0],
    [-578.0, -1072.0],
    [621.0, -1053.0],
    [1525.0, -343.0],
    [1530.0, 700.0],
    [1091.0, 1358.0],
];

/// A drivable 3D polyline with arc length.
struct Course {
    pts: Vec<[f32; 3]>,
    cum: Vec<f32>,
}

impl Course {
    fn new(pts: Vec<[f32; 3]>) -> Self {
        let mut cum = vec![0.0];
        for w in pts.windows(2) {
            let d = ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt();
            cum.push(cum.last().unwrap() + d);
        }
        Self { pts, cum }
    }

    fn length(&self) -> f32 {
        *self.cum.last().unwrap()
    }

    /// Position and unit XY direction `s` m along (wrapping on a closed loop).
    fn at(&self, s: f32) -> ([f32; 3], [f32; 2]) {
        let s = s.rem_euclid(self.length());
        let i = match self.cum.binary_search_by(|c| c.total_cmp(&s)) {
            Ok(i) => i.min(self.pts.len() - 2),
            Err(i) => (i - 1).min(self.pts.len() - 2),
        };
        let (a, b) = (self.pts[i], self.pts[i + 1]);
        let seg = (self.cum[i + 1] - self.cum[i]).max(1e-6);
        let t = ((s - self.cum[i]) / seg).clamp(0.0, 1.0);
        let p = [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ];
        let d = [(b[0] - a[0]) / seg, (b[1] - a[1]) / seg];
        (p, d)
    }
}

/// Where and when.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Place {
    /// Loop 9 at 23:00 with the `loop9_circuit` police (the race's own time of day).
    Loop9Night,
    /// Loop 9 at 14:00 with the same police profile (only the time of day differs).
    Loop9Day,
    /// Port Vesper's streets at 23:00, the normal city patrols, no race.
    CityNight,
}

impl Place {
    fn name(self) -> &'static str {
        match self {
            Place::Loop9Night => "Loop 9 night",
            Place::Loop9Day => "Loop 9 day",
            Place::CityNight => "City night",
        }
    }
}

/// Which units are out.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Units {
    /// The race's patrols only (no stakeouts): the general sight / radar rules.
    Patrols,
    /// The race as raced: patrols and the profile's shoulder stakeouts.
    Race,
}

/// One run's result.
#[derive(Clone, Debug, Default)]
struct Outcome {
    /// Racer time the first pursuit started, s.
    pursuit: Option<f32>,
    /// Racer time it became wanted, s.
    wanted: Option<f32>,
    /// Seconds the racer had been within [`NEAR_M`] of the unit that started the pursuit.
    notice_s: Option<f32>,
    /// Metres driven in that time.
    notice_m: Option<f32>,
    /// Distance from that unit when it started the pursuit, m.
    react_m: Option<f32>,
    /// Other units the racer passed within [`PASSED_M`] before any pursuit started.
    passed: u32,
    /// Sighting episodes (some unit had the racer in sight) that ended with no pursuit.
    glimpses: u32,
}

/// "Right there": the racer within this of a unit, m (inside every phase's sight range).
const NEAR_M: f32 = 250.0;
/// A unit the racer drove past this close was passed, m.
const PASSED_M: f32 = 40.0;

/// `wp_world`'s speeding detector (`violations.rs`, defaults): over the lane's limit by more
/// than `max(2.24, 10 %)` for 1 s, then a report every 5 s while it lasts. The harness has no
/// world step, so it raises the same reports itself.
struct WorldSpeeding {
    over_s: f32,
    last: Option<f32>,
}

impl WorldSpeeding {
    fn new() -> Self {
        Self {
            over_s: 0.0,
            last: None,
        }
    }

    /// A report now, if one is due: how far over `limit` (the lane's), m/s.
    fn report(&mut self, limit: Option<f32>, speed: f32, t: f32) -> Option<f32> {
        let lim = limit?;
        if speed <= lim + 2.24f32.max(0.1 * lim) {
            self.over_s = 0.0;
            return None;
        }
        self.over_s += PHYSICS_DT;
        let due = self.last.is_none_or(|t0| t - t0 >= 5.0);
        (self.over_s >= 1.0 && due).then(|| {
            self.last = Some(t);
            speed - lim
        })
    }
}

fn world_at(hour: f32) -> World {
    World::vesper(
        &wp_world::vesper::VesperConfig::default(),
        wp_world::WorldConfig {
            start_time: TimeOfDay::from_hours(hour),
            // Frozen clock: the phase stays put for the whole run.
            time_ratio: 0.0,
            ..wp_world::WorldConfig::default()
        },
    )
}

/// Loop 9 as raced (`World::plan_circuit`), on the road surface.
fn loop9_course(world: &World) -> Course {
    let route = world.plan_circuit(LOOP9).expect("Loop 9 circuit");
    Course::new(route.polyline.iter().map(|p| [p[0], p[1], p[2]]).collect())
}

/// A city drive at night: the straightest legal path from a downtown arterial, ~6 km, on the
/// ground.
fn city_course(world: &World, police: &PoliceSystem) -> Course {
    let g = police.graph();
    let seg = (0..g.segments.len())
        .find(|&i| {
            let s = &g.segments[i];
            s.class == wp_police::graph::RoadClass::Arterial
                && g.node(s.a).district == wp_police::District::Core
        })
        .expect("a downtown arterial");
    let s = g.segment(wp_police::graph::SegmentId(seg as u32));
    let (a, b) = (g.node(s.a).pos, g.node(s.b).pos);
    let path = g.predict_path(a, [b[0] - a[0], b[1] - a[1]], 6000.0);
    let n = (path.length() / 5.0) as usize;
    let pts = (0..=n)
        .filter_map(|i| path.point_at(i as f32 * 5.0))
        .map(|p| {
            let z = world.map().ground_height(p, None);
            [p[0], p[1], z]
        })
        .collect();
    Course::new(pts)
}

fn profile(units: Units) -> RaceProfile {
    let mut p = RaceProfiles::builtin()
        .get("loop9_circuit")
        .unwrap()
        .clone();
    if units == Units::Patrols {
        p.stakeouts.clear();
    }
    p
}

/// Put every ground unit on the road surface under it, as `wp_sim` does for its physical
/// police cars (`Sim::sync_police`: `sync_unit` with the chassis position). Headless units
/// otherwise keep z = 0, and on Port Vesper's hills (Loop 9 runs 10-40 m up) their sightlines
/// would run through the ground.
fn ground_units(world: &World, police: &mut PoliceSystem) {
    let syncs: Vec<_> = police
        .units()
        .iter()
        .filter(|u| u.kind == wp_police::UnitKind::Car && u.player.is_none())
        .map(|u| {
            let near = (u.body.alt_m != 0.0).then_some(u.body.alt_m);
            let z = world.map().ground_height(u.body.pos, near);
            (
                u.id,
                wp_police::UnitSync {
                    pos: [u.body.pos[0], u.body.pos[1], z],
                    vel: [
                        u.body.heading[0] * u.body.speed,
                        u.body.heading[1] * u.body.speed,
                        0.0,
                    ],
                    heading: u.body.heading,
                    health: u.health,
                    effects: u.effects,
                },
            )
        })
        .collect();
    for (id, s) in syncs {
        police.sync_unit(id, s);
    }
}

/// Drive `course` at `speed` for up to `max_s` (or until a pursuit starts), from `start_s`.
fn drive(
    world: &World,
    mut police: PoliceSystem,
    course: &Course,
    start_s: f32,
    speed: f32,
    max_s: f32,
) -> Outcome {
    use std::collections::{BTreeMap, BTreeSet};
    let trace = std::env::var("WP_DETECT_TRACE").is_ok();
    let mut out = Outcome::default();
    let mut ev = Vec::new();
    // Warm up with the racer parked at the start (patrols take their posts out of its sight).
    let (p0, _) = course.at(start_s);
    for _ in 0..(5.0 / PHYSICS_DT) as usize {
        let v = RacerView {
            entity: RACER,
            pos: p0,
            vel: [0.0; 3],
        };
        police.step(world, &[v], &[], PHYSICS_DT, &mut ev);
        ground_units(world, &mut police);
    }
    ev.clear();
    let mut reports = WorldSpeeding::new();
    // Per unit: (racer time, distance along) the racer came within NEAR_M on this approach.
    let mut near: BTreeMap<u32, (f32, f32)> = BTreeMap::new();
    let mut passed: BTreeSet<u32> = BTreeSet::new();
    let mut seen_before = false;
    let steps = (max_s / PHYSICS_DT) as usize;
    for k in 0..steps {
        let t = k as f32 * PHYSICS_DT;
        let s = start_s + speed * t;
        let (c, d) = course.at(s);
        // Overtake, don't drive through: beside a unit within 100 m, one lane over from it,
        // toward the course line (a racer within 2.4 m of a headless unit wrecks it).
        let left = [-d[1], d[0]];
        let off = police
            .units()
            .iter()
            .filter(|u| u.kind == wp_police::UnitKind::Car && !u.wrecked)
            .map(|u| {
                let r = [u.body.pos[0] - c[0], u.body.pos[1] - c[1]];
                (r[0] * r[0] + r[1] * r[1], r[0] * left[0] + r[1] * left[1])
            })
            .filter(|(d2, _)| *d2 < 100.0 * 100.0)
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map_or(0.0, |(_, lat)| {
                if lat.abs() >= PASS_OFFSET_M {
                    0.0
                } else if lat <= 0.0 {
                    lat + PASS_OFFSET_M
                } else {
                    lat - PASS_OFFSET_M
                }
            });
        let p = [c[0] + left[0] * off, c[1] + left[1] * off, c[2]];
        let v = RacerView {
            entity: RACER,
            pos: p,
            vel: [d[0] * speed, d[1] * speed, 0.0],
        };
        let world_ev: Vec<SimEvent> = reports
            .report(wp_contracts::WorldQuery::speed_limit_at(world, p), speed, t)
            .map(|over_mps| SimEvent::TrafficViolation {
                vehicle: RACER,
                kind: ViolationKind::Speeding { over_mps },
                pos: p,
            })
            .into_iter()
            .collect();
        police.step(world, &[v], &world_ev, PHYSICS_DT, &mut ev);
        ground_units(world, &mut police);
        let dist_to = |u: &wp_police::unit::Unit| {
            ((u.body.pos[0] - p[0]).powi(2) + (u.body.pos[1] - p[1]).powi(2)).sqrt()
        };
        for u in police.units() {
            if u.kind != wp_police::UnitKind::Car || u.wrecked {
                continue;
            }
            let dd = dist_to(u);
            if dd <= NEAR_M {
                near.entry(u.id.0).or_insert((t, s));
            } else {
                near.remove(&u.id.0);
            }
            if dd <= PASSED_M {
                passed.insert(u.id.0);
            }
        }
        let seen = police.sighting(RACER).is_some_and(|o| o.seen);
        if trace && k % 24 == 0 && (seen || near.values().any(|n| t - n.0 < 6.0)) {
            let nearest = police
                .units()
                .iter()
                .map(|u| (dist_to(u), u.id.0, u.body.heading, u.assignment.clone()))
                .min_by(|a, b| a.0.total_cmp(&b.0));
            println!(
                "t={t:.2} p={p:?} {:?} nearest={nearest:?} radar={} blatant={} heat={:.1}",
                police.sighting(RACER),
                police.stats().radar_clocks,
                police.stats().blatant,
                police.racer(RACER).map_or(0.0, |r| r.heat),
            );
        }
        if seen_before && !seen {
            out.glimpses += 1;
        }
        seen_before = seen;
        let mut started: Option<wp_police::UnitId> = None;
        for e in ev.drain(..) {
            match e {
                SimEvent::Police(PoliceEvent::WantedLevelChanged { racer, .. })
                    if racer == RACER =>
                {
                    out.wanted.get_or_insert(t);
                }
                SimEvent::Police(PoliceEvent::PursuitStarted { racer, unit }) if racer == RACER => {
                    out.pursuit.get_or_insert(t);
                    started = started.or(unit).or(Some(wp_police::UnitId(u32::MAX)));
                }
                _ => {}
            }
        }
        if let Some(unit) = started {
            if let Some(u) = police.units().iter().find(|u| u.id == unit) {
                out.react_m = Some(dist_to(u));
                let (t0, s0) = near.get(&unit.0).copied().unwrap_or((t, s));
                out.notice_s = Some(t - t0);
                out.notice_m = Some(s - s0);
            }
            passed.remove(&unit.0);
            out.passed = passed.len() as u32;
            return out;
        }
    }
    out.passed = passed.len() as u32;
    out
}

/// One run: `place`, `units`, `mph`, `seed` (patrol placement and the start point).
fn run(place: Place, units: Units, mph: f32, seed: u64) -> Outcome {
    run_with(place, units, mph, seed, PoliceConfig::builtin())
}

fn run_with(place: Place, units: Units, mph: f32, seed: u64, cfg: PoliceConfig) -> Outcome {
    let hour = if place == Place::Loop9Day { 14.0 } else { 23.0 };
    let world = world_at(hour);
    let setup = PoliceSetup {
        seed: 0x0090_11CE ^ seed,
        ..PoliceSetup::default()
    };
    let mut police = PoliceSystem::for_world(cfg, &world, setup);
    let speed = mph * MPH;
    match place {
        Place::Loop9Night | Place::Loop9Day => {
            let course = loop9_course(&world);
            let line: Vec<[f32; 2]> = course.pts.iter().map(|p| [p[0], p[1]]).collect();
            police.start_race(
                "loop9_circuit",
                &profile(units),
                &RaceCourse::new(&line),
                &[RACER],
            );
            // Start points spread round the lap (one lap at most).
            let start = course.length() * ((seed % 8) as f32 / 8.0);
            let max_s = (course.length() / speed).min(240.0);
            drive(&world, police, &course, start, speed, max_s)
        }
        Place::CityNight => {
            let course = city_course(&world, &police);
            let start = course.length() * ((seed % 4) as f32 / 8.0);
            let max_s = ((course.length() - start) / speed).min(240.0);
            drive(&world, police, &course, start, speed, max_s)
        }
    }
}

fn fmt(v: Option<f32>) -> String {
    v.map_or("—".to_string(), |x| format!("{x:.1}"))
}

fn median(mut v: Vec<f32>) -> Option<f32> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.total_cmp(b));
    Some(v[v.len() / 2])
}

fn max(v: &[f32]) -> Option<f32> {
    v.iter().copied().reduce(f32::max)
}

/// One table row over `seeds` runs.
fn summary(place: Place, units: Units, mph: f32, seeds: u64) -> (Vec<Outcome>, String) {
    let runs: Vec<Outcome> = (0..seeds).map(|s| run(place, units, mph, s)).collect();
    let notice: Vec<f32> = runs.iter().filter_map(|o| o.notice_s).collect();
    let metres: Vec<f32> = runs.iter().filter_map(|o| o.notice_m).collect();
    let react: Vec<f32> = runs.iter().filter_map(|o| o.react_m).collect();
    let passed: u32 = runs.iter().map(|o| o.passed).sum();
    let none = runs.iter().filter(|o| o.pursuit.is_none()).count();
    let line = format!(
        "| {} | {:?} | {mph:.0} | {} | {} | {} | {} | {passed} | {none}/{} |",
        place.name(),
        units,
        fmt(median(notice.clone())),
        fmt(max(&notice)),
        fmt(median(metres)),
        fmt(median(react)),
        runs.len()
    );
    (runs, line)
}

/// The full table (slow: release build).
#[test]
#[ignore]
fn loop9_night_detection_table() {
    let seeds: u64 = std::env::var("WP_DETECT_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);
    println!(
        "| Place | Units | mph | notice s (median) | notice s (max) | driven in it m (median) | unit distance at pursuit m (median) | units passed unnoticed | laps with no pursuit |"
    );
    println!("|---|---|---|---|---|---|---|---|---|");
    for place in [Place::Loop9Night, Place::Loop9Day, Place::CityNight] {
        let unit_sets: &[Units] = if place == Place::CityNight {
            &[Units::Patrols]
        } else {
            &[Units::Patrols, Units::Race]
        };
        for &units in unit_sets {
            for mph in [50.0, 100.0, 150.0, 185.0] {
                let (_, line) = summary(place, units, mph, seeds);
                println!("{line}");
            }
        }
    }
}

/// The parked-unit pass for every speed, before / after (`--ignored --nocapture`).
#[test]
#[ignore]
fn parked_unit_pass_table() {
    println!("| time | mph | before: pursuit at m from unit | after |");
    println!("|---|---|---|---|");
    for hour in [23.0, 14.0] {
        for mph in [50.0, 100.0, 150.0, 185.0] {
            let b = pass_parked_unit(hour, mph, without_conspicuity());
            let a = pass_parked_unit(hour, mph, PoliceConfig::builtin());
            println!(
                "| {hour:.0}:00 | {mph:.0} | {} | {} |",
                fmt(b.at_m),
                fmt(a.at_m)
            );
        }
    }
}

/// Debug one run: `WP_DETECT_ONE=place,units,mph,seed` (place 0 night / 1 day / 2 city).
#[test]
#[ignore]
fn loop9_night_detection_one() {
    let Ok(spec) = std::env::var("WP_DETECT_ONE") else {
        return;
    };
    let f: Vec<f32> = spec.split(',').map(|x| x.parse().unwrap()).collect();
    let place = [Place::Loop9Night, Place::Loop9Day, Place::CityNight][f[0] as usize];
    let units = [Units::Patrols, Units::Race][f[1] as usize];
    println!("{:?}", run(place, units, f[2], f[3] as u64));
}

// -----------------------------------------------------------------------------------------
// Acceptance checks on a synthetic straight highway (fast; debug builds too).
// -----------------------------------------------------------------------------------------

/// Loop 9's limit, m/s.
const LIMIT: f32 = 24.6;
/// The parked unit's spot along the highway, m.
const UNIT_X: f32 = 3000.0;

/// A 6 km divided highway along +X (eastbound carriageway at y = -6, westbound at +6).
fn straight_highway() -> wp_police::RoadGraph {
    use wp_police::graph::RoadClass;
    let mut g = wp_police::RoadGraph::new();
    let east: Vec<_> = (0..=30)
        .map(|k| g.add_node([k as f32 * 200.0, -6.0], wp_police::District::Core))
        .collect();
    let west: Vec<_> = (0..=30)
        .map(|k| g.add_node([(30 - k) as f32 * 200.0, 6.0], wp_police::District::Core))
        .collect();
    for w in east.windows(2) {
        g.add_segment(w[0], w[1], LIMIT, RoadClass::Highway, true);
    }
    for w in west.windows(2) {
        g.add_segment(w[0], w[1], LIMIT, RoadClass::Highway, true);
    }
    g
}

/// What a pass of one parked unit did.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Pass {
    /// Racer time the pursuit started, s.
    pursuit: Option<f32>,
    /// Racer x minus the unit's x then, m (negative: still behind the unit).
    at_m: Option<f32>,
    /// Racer heat at the end.
    heat: f32,
}

/// A racer at `mph` eastbound from x = 0 past one unit parked on the shoulder at `UNIT_X`
/// facing the racing direction (away from the racer: the hardest case, a Loop 9 patrol or
/// stakeout being caught up). No other police. `wp_world`'s speeding reports are raised every
/// 5 s as the world would (nobody sees the early ones).
fn pass_parked_unit(hour: f32, mph: f32, cfg: PoliceConfig) -> Pass {
    let mut cfg = cfg;
    let none = wp_police::config::PhaseTable {
        dawn: 0,
        day: 0,
        dusk: 0,
        night: 0,
    };
    cfg.presence.patrols = wp_police::config::DistrictTable {
        core: none,
        suburb: none,
        industrial: none,
    };
    let mut police = PoliceSystem::with_city(cfg, straight_highway(), PoliceSetup::default());
    let mut prof = RaceProfiles::builtin()
        .get("loop9_circuit")
        .unwrap()
        .clone();
    // One unit staked out half way, recognising by the normal rules only.
    prof.recognise_on_sight = false;
    prof.stakeouts.truncate(1);
    prof.stakeouts[0].at = 0.5;
    let course = RaceCourse::new(&[[0.0, -7.75], [6000.0, -7.75]]);
    police.start_race("loop9_circuit", &prof, &course, &[RACER]);
    let unit_x = police.units()[0].body.pos[0];
    assert!((unit_x - UNIT_X).abs() < 50.0, "unit at {unit_x}");
    let world = World::flat(TimeOfDay::from_hours(hour), 0.0);
    let speed = mph * MPH;
    let mut reports = WorldSpeeding::new();
    let mut ev = Vec::new();
    let dt = 1.0 / 120.0;
    let mut out = Pass {
        pursuit: None,
        at_m: None,
        heat: 0.0,
    };
    let steps = ((UNIT_X + 1500.0) / speed / dt) as usize;
    for k in 0..steps {
        let t = k as f32 * dt;
        let x = speed * t;
        let p = [x, -9.5, 0.0];
        let world_ev: Vec<SimEvent> = reports
            .report(Some(LIMIT), speed, t)
            .map(|over_mps| SimEvent::TrafficViolation {
                vehicle: RACER,
                kind: ViolationKind::Speeding { over_mps },
                pos: p,
            })
            .into_iter()
            .collect();
        let v = RacerView {
            entity: RACER,
            pos: p,
            vel: [speed, 0.0, 0.0],
        };
        police.step(&world, &[v], &world_ev, dt, &mut ev);
        for e in ev.drain(..) {
            if let SimEvent::Police(PoliceEvent::PursuitStarted { racer, .. }) = e {
                if racer == RACER && out.pursuit.is_none() {
                    out.pursuit = Some(t);
                    out.at_m = Some(x - unit_x);
                }
            }
        }
    }
    out.heat = police.racer(RACER).map_or(0.0, |r| r.heat);
    out
}

/// The shipped police with the conspicuity rule switched off (the rules before 2026-10-27).
fn without_conspicuity() -> PoliceConfig {
    let mut cfg = PoliceConfig::builtin();
    cfg.heat.heat.conspicuous.from_over_mps = 1.0e6;
    cfg.heat.heat.conspicuous.full_over_mps = 2.0e6;
    cfg
}

#[test]
fn blatant_speed_is_spotted_before_it_reaches_a_unit_facing_away_at_night() {
    // 185 mph caught up with a unit facing away on Loop 9 at 23:00: headlights in the mirror
    // and the engine give it away well before it is alongside. Before the fix: seen only at
    // the 60 m mirror radius, then a 2 s radar hold (unseen wp_world reports held the radar
    // back): the pursuit started ~100 m after the racer had passed the unit.
    let after = pass_parked_unit(23.0, 185.0, PoliceConfig::builtin());
    let at = after.at_m.expect("no pursuit at 185 mph");
    assert!(
        at <= -150.0,
        "pursuit only {at:.0} m from the unit: {after:?}"
    );
    let before = pass_parked_unit(23.0, 185.0, without_conspicuity());
    assert!(
        before.at_m.is_none_or(|b| b > at + 100.0),
        "before {before:?} after {after:?}"
    );
}

#[test]
fn night_does_not_hide_blatant_speed() {
    for mph in [150.0, 185.0] {
        let night = pass_parked_unit(23.0, mph, PoliceConfig::builtin());
        let day = pass_parked_unit(14.0, mph, PoliceConfig::builtin());
        let (n, d) = (night.at_m.unwrap(), day.at_m.unwrap());
        assert!(
            (n - d).abs() <= 20.0,
            "{mph} mph: night {n:.0} m, day {d:.0} m"
        );
        assert!(n < 0.0, "{mph} mph: only noticed after passing ({n:.0} m)");
    }
}

#[test]
fn mild_speeding_and_ordinary_driving_are_unchanged() {
    // Owner PR #64 (police far less sensitive) stays as tuned below 15 m/s over the limit:
    // identical outcome with and without the rule, day and night.
    for hour in [14.0, 23.0] {
        for mph in [50.0, 60.0, 70.0, 85.0] {
            let with = pass_parked_unit(hour, mph, PoliceConfig::builtin());
            let without = pass_parked_unit(hour, mph, without_conspicuity());
            assert_eq!(with, without, "{mph} mph at {hour} h");
        }
    }
}

#[test]
fn conspicuity_scales_with_how_far_over_the_limit() {
    let c = PoliceConfig::builtin().heat.heat.conspicuous;
    assert_eq!(c.level(0.0), 0.0);
    assert_eq!(c.level(c.from_over_mps), 0.0);
    assert_eq!(c.level(c.full_over_mps + 10.0), 1.0);
    let mid = c.level(0.5 * (c.from_over_mps + c.full_over_mps));
    assert!((mid - 0.5).abs() < 1e-5);
    // Loop 9 (24.6 m/s): 100 mph a little, 185 mph fully.
    assert!(c.level(100.0 * MPH - LIMIT) < 0.25);
    assert_eq!(c.level(185.0 * MPH - LIMIT), 1.0);
    // Never slower or shorter than the normal rule.
    use wp_police::config::ConspicuousConfig as C;
    assert!((C::scale(2.0, 0.3, 1.0, false) - 0.3).abs() < 1e-5);
    assert_eq!(C::scale(2.0, 5.0, 1.0, false), 2.0);
    assert_eq!(C::scale(260.0, 380.0, 0.5, true), 320.0);
    assert_eq!(C::scale(400.0, 380.0, 1.0, true), 400.0);
    // A faster car is spotted further out.
    let p150 = pass_parked_unit(23.0, 150.0, PoliceConfig::builtin());
    let p185 = pass_parked_unit(23.0, 185.0, PoliceConfig::builtin());
    assert!(
        p185.at_m.unwrap() <= p150.at_m.unwrap() + 10.0,
        "{p150:?} {p185:?}"
    );
}
