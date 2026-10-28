//! Owner request 2026-10-28: "make Loop 9 daytime police activity more dynamic". Headless
//! measurement of how varied the police are in a daytime Loop 9 chase (the `loop9_day` race's
//! police at 14:00 on Port Vesper).
//!
//! The racer is kinematic, like `loop9_night_detection.rs`: it drives Loop 9 (as raced,
//! `World::plan_circuit`) at a constant speed, overtaking units one lane over; it is never
//! caught, so the run shows what the police try over `RUN_S` seconds. Units are the police's
//! own headless cars. Every half second the run samples every ground unit:
//!
//! - **Tactics**: the distinct things units do about the racer: the assignment
//!   (`intercept`, `ambush`, `roadblock`, `spikes`) or, while pursuing, the tactic (`Shadow`,
//!   `BoxIn`, `Pit`, `RollingBlock`, `Ram`), plus the commander's Loop 9 operations
//!   ([`wp_police::PoliceSystem::loop_op`]: `flank`, `speed-trap`, `rotate`, `peel-off`, ...).
//! - **Approach directions**: where each unit that comes for the racer is, relative to the
//!   racer's heading, when it first comes within [`APPROACH_M`]: ahead, behind, left or right
//!   (45° sectors around the heading), and whether it came off the course (an on-ramp or side
//!   road, more than [`OFF_COURSE_M`] from the course line).
//! - **Trailing share**: of the samples of units after the racer within [`TRAIL_M`], the share
//!   behind it (more than 5 m back along its heading). 100 % = a conga line.
//! - **Patrols moving**: of the samples of units not after the racer (patrols on the
//!   corridor), the share moving faster than 3 m/s.
//! - **Peel-offs / rejoins**: units that stopped coming for the racer and later came again.
//!
//! `cargo test -p wp_police --release --test loop9_day_variety -- --ignored --nocapture`
//! prints the table (`docs/integration/police-blocks.md`).

use std::collections::{BTreeMap, BTreeSet};

use wp_contracts::{EntityId, SimEvent, TimeOfDay, ViolationKind, PHYSICS_DT};
use wp_police::race::{RaceCourse, RaceProfiles};
use wp_police::unit::{Assignment, Tactic};
use wp_police::{PoliceConfig, PoliceEvent, PoliceSetup, PoliceSystem, RacerView};
use wp_world::World;

const RACER: EntityId = EntityId(1);
const MPH: f32 = 0.44704;
/// Lateral gap the racer keeps from a unit it overtakes, m (one lane).
const PASS_OFFSET_M: f32 = 3.7;
/// A unit's approach is classified when it first comes this close, m.
const APPROACH_M: f32 = 150.0;
/// Further than this from the course line is off the course (ramp, side road), m.
const OFF_COURSE_M: f32 = 12.0;
/// Units after the racer within this count for the trailing share, m.
const TRAIL_M: f32 = 400.0;
/// Sample period, s.
const SAMPLE_S: f32 = 0.5;

/// The `loop9_day` course (wp_race::catalog `LOOP9`, driving order).
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

fn world_at(hour: f32) -> World {
    World::vesper(
        &wp_world::vesper::VesperConfig::default(),
        wp_world::WorldConfig {
            start_time: TimeOfDay::from_hours(hour),
            time_ratio: 0.0,
            ..wp_world::WorldConfig::default()
        },
    )
}

/// Put every ground unit on the road surface under it (as `wp_sim` does).
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

/// One run's numbers.
#[derive(Clone, Debug, Default)]
pub struct Variety {
    /// Distinct tactics (labels, see the module doc).
    pub tactics: BTreeSet<String>,
    /// Approaches per sector (`ahead`, `behind`, `left`, `right`).
    pub sectors: BTreeMap<&'static str, u32>,
    /// Approaches from off the course (ramps, side roads).
    pub off_course: u32,
    /// Units sent at the racer, by where they were relative to it then (`ahead`, `behind`,
    /// `left`, `right`): the directions they converge from.
    pub commit_sectors: BTreeMap<&'static str, u32>,
    /// Units sent at the racer from off the course (a ramp or side road).
    pub commit_off_course: u32,
    /// Approaches in all.
    pub approaches: u32,
    /// Samples of units after the racer within `TRAIL_M`, and of those behind it.
    pub chase_samples: u32,
    /// See `chase_samples`.
    pub behind_samples: u32,
    /// Samples of units not after the racer, and of those moving.
    pub patrol_samples: u32,
    /// See `patrol_samples`.
    pub patrol_moving: u32,
    /// Units that stopped coming for the racer and came again later.
    pub rejoins: u32,
    /// Roadblocks and spike strips placed during the run (scripted ones at the start too).
    pub roadblocks: u32,
    /// See `roadblocks`.
    pub spikes: u32,
    /// Highest wanted level reached.
    pub max_level: u8,
    /// Different units that came for the racer.
    pub units_engaged: u32,
}

impl Variety {
    fn merge(&mut self, o: &Variety) {
        self.tactics.extend(o.tactics.iter().cloned());
        for (k, v) in &o.sectors {
            *self.sectors.entry(k).or_default() += v;
        }
        self.off_course += o.off_course;
        for (k, v) in &o.commit_sectors {
            *self.commit_sectors.entry(k).or_default() += v;
        }
        self.commit_off_course += o.commit_off_course;
        self.approaches += o.approaches;
        self.chase_samples += o.chase_samples;
        self.behind_samples += o.behind_samples;
        self.patrol_samples += o.patrol_samples;
        self.patrol_moving += o.patrol_moving;
        self.rejoins += o.rejoins;
        self.roadblocks += o.roadblocks;
        self.spikes += o.spikes;
        self.max_level = self.max_level.max(o.max_level);
        self.units_engaged += o.units_engaged;
    }

    /// Share of chase samples behind the racer, %.
    pub fn trailing_pct(&self) -> f32 {
        100.0 * self.behind_samples as f32 / self.chase_samples.max(1) as f32
    }

    /// Share of patrol samples moving, %.
    pub fn patrols_moving_pct(&self) -> f32 {
        100.0 * self.patrol_moving as f32 / self.patrol_samples.max(1) as f32
    }

    /// Sectors with at least one approach.
    pub fn sectors_used(&self) -> usize {
        self.sectors.values().filter(|v| **v > 0).count()
    }
}

/// The label of what unit `u` does about the racer, if anything.
fn label(police: &PoliceSystem, u: &wp_police::unit::Unit) -> Option<String> {
    let op = police.loop_op(u.id).map(|s| s.to_string());
    let base = match &u.assignment {
        Assignment::Pursue { target } if *target == RACER => Some(match u.tactic {
            Tactic::Shadow => "Shadow",
            Tactic::BoxIn(_) => "BoxIn",
            Tactic::Pit => "Pit",
            Tactic::RollingBlock => "RollingBlock",
            Tactic::Ram(_) => "Ram",
        }),
        Assignment::Intercept { target, ambush, .. } if *target == RACER => {
            Some(if *ambush { "ambush" } else { "intercept" })
        }
        Assignment::RoadblockAt { .. } => Some("roadblock"),
        Assignment::SpikeAt { .. } => Some("spikes"),
        _ => None,
    };
    match (base, op) {
        (_, Some(op)) => Some(op),
        (Some(b), None) => Some(b.to_string()),
        (None, None) => None,
    }
}

/// Is `u` coming for the racer?
fn after_racer(u: &wp_police::unit::Unit) -> bool {
    u.assignment.target() == Some(RACER)
}

/// One daytime Loop 9 chase: the racer at `mph`, from a start `seed / 8` of the lap round,
/// for `run_s` seconds.
pub fn run(mph: f32, seed: u64, run_s: f32, cfg: PoliceConfig) -> Variety {
    run_ops(mph, seed, run_s, cfg, true)
}

/// [`run`] with the commander's Loop 9 operations on or off (`ops`).
pub fn run_ops(mph: f32, seed: u64, run_s: f32, cfg: PoliceConfig, ops: bool) -> Variety {
    let world = world_at(14.0);
    let setup = PoliceSetup {
        seed: 0x00DA_7100 ^ seed,
        ..PoliceSetup::default()
    };
    let mut police = PoliceSystem::for_world(cfg, &world, setup);
    let route = world.plan_circuit(LOOP9).expect("Loop 9 circuit");
    let course = Course::new(route.polyline.iter().map(|p| [p[0], p[1], p[2]]).collect());
    let line: Vec<[f32; 2]> = course.pts.iter().map(|p| [p[0], p[1]]).collect();
    let rc = RaceCourse::new(&line);
    let mut profile = RaceProfiles::builtin().get("loop9_day").unwrap().clone();
    profile.dynamic_ops = ops;
    police.start_race("loop9_day", &profile, &rc, &[RACER]);
    let speed = mph * MPH;
    let start = course.length() * ((seed % 8) as f32 / 8.0);
    let mut ev = Vec::new();
    let (p0, _) = course.at(start);
    for _ in 0..(5.0 / PHYSICS_DT) as usize {
        let v = RacerView {
            entity: RACER,
            pos: p0,
            vel: [0.0; 3],
        };
        police.step(&world, &[v], &[], PHYSICS_DT, &mut ev);
        ground_units(&world, &mut police);
    }
    let mut out = Variety::default();
    for e in ev.drain(..) {
        count_event(&e, &mut out);
    }
    let mut last_report: Option<f32> = None;
    let mut over_s = 0.0;
    // Per unit: currently after the racer, has been after it before, already classified.
    let mut was_after: BTreeMap<u32, bool> = BTreeMap::new();
    let mut ever_after: BTreeSet<u32> = BTreeSet::new();
    let mut classified: BTreeSet<u32> = BTreeSet::new();
    let steps = (run_s / PHYSICS_DT) as usize;
    let sample_every = (SAMPLE_S / PHYSICS_DT).round() as usize;
    for k in 0..steps {
        let t = k as f32 * PHYSICS_DT;
        let s = start + speed * t;
        let (c, d) = course.at(s);
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
        // wp_world's speeding reports (1 s over, then every 5 s).
        let mut world_ev = Vec::new();
        if let Some(lim) = wp_contracts::WorldQuery::speed_limit_at(&world, p) {
            if speed > lim + 2.24f32.max(0.1 * lim) {
                over_s += PHYSICS_DT;
                if over_s >= 1.0 && last_report.is_none_or(|t0| t - t0 >= 5.0) {
                    last_report = Some(t);
                    world_ev.push(SimEvent::TrafficViolation {
                        vehicle: RACER,
                        kind: ViolationKind::Speeding {
                            over_mps: speed - lim,
                        },
                        pos: p,
                    });
                }
            } else {
                over_s = 0.0;
            }
        }
        police.step(&world, &[v], &world_ev, PHYSICS_DT, &mut ev);
        ground_units(&world, &mut police);
        for e in ev.drain(..) {
            count_event(&e, &mut out);
        }
        if let Some(r) = police.racer(RACER) {
            out.max_level = out.max_level.max(r.level.number());
        }
        if k % sample_every != 0 {
            continue;
        }
        for u in police.units() {
            if u.kind != wp_police::UnitKind::Car || u.wrecked {
                continue;
            }
            let rel = [u.body.pos[0] - p[0], u.body.pos[1] - p[1]];
            let dist = (rel[0] * rel[0] + rel[1] * rel[1]).sqrt();
            let along = rel[0] * d[0] + rel[1] * d[1];
            let lat = rel[0] * left[0] + rel[1] * left[1];
            let after = after_racer(u);
            if let Some(l) = label(&police, u) {
                out.tactics.insert(l);
            }
            let prev = was_after.insert(u.id.0, after).unwrap_or(false);
            if after && !prev && ever_after.contains(&u.id.0) {
                out.rejoins += 1;
            }
            let sector = if along.abs() >= lat.abs() {
                if along >= 0.0 {
                    "ahead"
                } else {
                    "behind"
                }
            } else if lat > 0.0 {
                "left"
            } else {
                "right"
            };
            if after && !prev {
                *out.commit_sectors.entry(sector).or_default() += 1;
                if rc.project(u.body.pos).1 > OFF_COURSE_M {
                    out.commit_off_course += 1;
                }
            }
            if after {
                ever_after.insert(u.id.0);
                if dist <= TRAIL_M {
                    out.chase_samples += 1;
                    if along < -5.0 {
                        out.behind_samples += 1;
                    }
                }
                if dist <= APPROACH_M && classified.insert(u.id.0) {
                    out.approaches += 1;
                    *out.sectors.entry(sector).or_default() += 1;
                    if rc.project(u.body.pos).1 > OFF_COURSE_M {
                        out.off_course += 1;
                    }
                }
            } else if u.role != wp_police::unit::UnitRole::Tool {
                out.patrol_samples += 1;
                if u.body.speed.abs() > 3.0 {
                    out.patrol_moving += 1;
                }
            }
        }
    }
    out.units_engaged = ever_after.len() as u32;
    if std::env::var("WP_VARIETY_TRACE").is_ok() {
        eprintln!(
            "seed {seed}: ramps {} ops {:?}",
            police.race_police().map_or(0, |r| r.ramps.len()),
            police.loop_ops_started()
        );
    }
    out
}

fn count_event(e: &SimEvent, out: &mut Variety) {
    match e {
        SimEvent::Police(PoliceEvent::RoadblockPlaced { .. }) => out.roadblocks += 1,
        SimEvent::Police(PoliceEvent::SpikesPlaced { .. }) => out.spikes += 1,
        _ => {}
    }
}

fn sectors(m: &BTreeMap<&'static str, u32>) -> String {
    m.iter()
        .map(|(k, n)| format!("{k} {n}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn line(name: &str, v: &Variety, runs: usize) -> String {
    format!(
        "| {name} | {} | {} | {} ({}) | {}/{} | {} ({}) | {}/{} | {:.0} % | {:.0} % | {} | {} / {} | L{} | {} |",
        runs,
        v.tactics.len(),
        v.commit_sectors.values().filter(|n| **n > 0).count(),
        sectors(&v.commit_sectors),
        v.commit_off_course,
        v.commit_sectors.values().sum::<u32>(),
        v.sectors_used(),
        sectors(&v.sectors),
        v.off_course,
        v.approaches,
        v.trailing_pct(),
        v.patrols_moving_pct(),
        v.rejoins,
        v.roadblocks,
        v.spikes,
        v.max_level,
        v.tactics.iter().cloned().collect::<Vec<_>>().join(" "),
    )
}

fn seeds() -> u64 {
    std::env::var("WP_VARIETY_SEEDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(4)
}

/// The table (slow: release build).
#[test]
#[ignore]
fn loop9_day_variety_table() {
    let run_s: f32 = std::env::var("WP_VARIETY_S")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(150.0);
    println!(
        "| mph | runs | distinct tactics | sent from (sectors) | sent from off the course | within 150 m from (sectors) | off the course within 150 m | trailing | patrols moving | rejoins | roadblocks / spikes | max level | tactics |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for mph in [100.0, 140.0] {
        let mut all = Variety::default();
        for seed in 0..seeds() {
            all.merge(&run(mph, seed, run_s, PoliceConfig::builtin()));
        }
        println!("{}", line(&format!("{mph:.0}"), &all, seeds() as usize));
    }
}

/// Acceptance: a daytime Loop 9 chase is varied. The same two 120 s chases at 120 mph with
/// the commander's Loop 9 operations off (as before them) and on: on, more distinct tactics
/// (all five operations among them), a larger share of units sent in from off the course
/// (ramps), a smaller share sent from behind the racer, and units peeling off and rejoining.
#[test]
fn loop9_day_chase_is_varied() {
    let mut off = Variety::default();
    let mut on = Variety::default();
    for seed in 0..2 {
        off.merge(&run_ops(120.0, seed, 120.0, PoliceConfig::builtin(), false));
        on.merge(&run_ops(120.0, seed, 120.0, PoliceConfig::builtin(), true));
    }
    println!("{}", line("120, ops off", &off, 2));
    println!("{}", line("120, ops on", &on, 2));
    let share =
        |v: &Variety, n: u32| n as f32 / v.commit_sectors.values().sum::<u32>().max(1) as f32;
    let behind = |v: &Variety| share(v, v.commit_sectors.get("behind").copied().unwrap_or(0));
    let ramps = |v: &Variety| share(v, v.commit_off_course);
    assert!(
        on.tactics.len() >= off.tactics.len() + 4,
        "tactics {:?} vs {:?}",
        on.tactics,
        off.tactics
    );
    for op in ["flank", "converge", "speed-trap", "rotate", "peel-off"] {
        assert!(on.tactics.contains(op), "no {op}: {:?}", on.tactics);
    }
    assert!(
        on.commit_sectors.values().filter(|n| **n > 0).count() >= 3,
        "sent from {:?}",
        on.commit_sectors
    );
    assert!(
        ramps(&on) >= 1.5 * ramps(&off) && ramps(&on) >= 0.2,
        "sent from off the course: {:.2} on vs {:.2} off",
        ramps(&on),
        ramps(&off)
    );
    assert!(
        behind(&on) < behind(&off),
        "sent from behind: {:.2} on vs {:.2} off",
        behind(&on),
        behind(&off)
    );
    assert!(on.rejoins > off.rejoins, "no unit peeled off and rejoined");
    assert!(
        on.patrols_moving_pct() >= 60.0,
        "patrols moving {:.0} %",
        on.patrols_moving_pct()
    );
}
