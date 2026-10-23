//! Repeated-hit toughness table and scripted chase (workstream 03; playtest rows 48–49, 87).
//!
//! **Repeated hits.** For a car, a crash kind and a speed, the same crash is repeated on the same
//! car (damage kept, the car put back at speed before every hit; the other car is repaired
//! between hits so every hit is the same) and after every hit the damage state is read. Reports
//! per row the hit on which the car first shows:
//! - **minor**: noticeable handling loss — toe > 0.01 rad, a wheel's grip < 0.9, power < 0.85,
//!   steering ratio < 0.85, |steer pull| > 0.03, a brake < 0.8, a puncture > 0.3;
//! - **degr** (degraded): clearly impaired — power < 0.75, steering ratio < 0.75, grip < 0.8,
//!   a brake < 0.6, a puncture > 0.5, health < 0.5, or a mechanical component failed (engine,
//!   radiator, steering rack, suspension, wheel, gearbox, driveshaft, brake);
//! - **out**: a police unit pulls over (`data/police/pursuit.toml`: health below the mode's
//!   `disengage_health`, toe > `bent_wheel_toe_rad` 0.02, puncture > 0.5, wheel lost, engine dead);
//! - **wreck**: undrivable — total loss, engine dead, a wheel lost or power < 0.35.
//!
//! Plus `hit1` (cosmetic / minor / degraded / wreck after the first hit), the health after hits
//! 1 and 3 and the repair bill after hit 1 (default price table, the mode's `repair_cost_scale`).
//! Repeated identical hits are sensitive to where exactly the car strikes, so every row runs
//! three times with the impact point shifted −0.15 / 0 / +0.15 m and prints the medians.
//!
//! Crash kinds (the measured car is the first one named; the other car is a 1400 kg sedan):
//! - `wall-front`: full-width rigid wall;
//! - `wall-offset`: rigid wall covering the left 40 % of the car's width;
//! - `wall-angle`: rigid wall angled 30° off square: the right front corner hits first, then the
//!   car glances along it (`Sim::place_vehicle` cannot start a car sliding sideways, so the
//!   side impact is measured car-to-car, `car-side`);
//! - `car-front`: into the rear of the sedan at rest, full overlap (a cop ramming);
//! - `car-offset`: as `car-front`, 50 % overlap on the left;
//! - `car-side`: the measured car at rest, struck on the driver door by the sedan (T-bone);
//! - `mixed`: hit n is the n-th of the six kinds above, cycling (the "several hard hits" case);
//! - legacy (round 12b, `WP_KINDS=…` only): `swipe`, `pit-ee`, `pit-er`.
//!
//! **Chase** (`chase` as the first argument): a scripted sequence of chase contacts on one car,
//! 12 s apart (contact, rebound, then coasting), damage kept: swipe 70, PIT-ed at 80, ram 60,
//! wall-angle 60, T-bone 50, wall-offset 70, swipe 90, ram 80. Prints component health after the
//! sequence and the time at which the engine (power < 0.85) and the steering (ratio < 0.85) are
//! first impaired.
//!
//! ```sh
//! cargo run --release -p simcli --example damage_toughness -- [arcade|sim|extreme] [car …]
//! cargo run --release -p simcli --example damage_toughness -- chase [arcade|sim|extreme] [car …]
//! ```
//! Defaults: Arcade, `hakone_gt debug_sedan junkyard_beater sentinel_cruiser`. `WP_KINDS=a,b`
//! and `WP_SPEEDS=30,50` limit a table run, `WP_NO_PANELS=1` turns R's panels off,
//! `WP_VERBOSE=1` prints every hit. Rows run in parallel (one `Sim` per row; results are
//! independent of thread count).

use std::path::Path;

use simcli::data::{load_car, load_mode};
use simcli::FsData;
use wp_contracts::{
    AssistProfile, Component, DamageMode, DamageSim, EntityId, HandlingEffects, LoadedCar,
    ModeTable, TimeOfDay, VehicleInput,
};
use wp_sim::{Sim, SimConfig};

/// Hits per row before giving up.
const MAX_HITS: usize = 10;
/// Seconds simulated per hit (contact, rebound, lattice sleep).
const HIT_S: f32 = 3.0;
/// Seconds between chase events.
const CHASE_GAP_S: f32 = 12.0;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    WallFront,
    WallOffset,
    WallAngle,
    CarFront,
    CarOffset,
    CarSide,
    Swipe,
    PitEe,
    PitEr,
    /// Cycles through [`DEFAULT_KINDS`] (one of each, then again).
    Mixed,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::WallFront => "wall-front",
            Kind::WallOffset => "wall-offset",
            Kind::WallAngle => "wall-angle",
            Kind::CarFront => "car-front",
            Kind::CarOffset => "car-offset",
            Kind::CarSide => "car-side",
            Kind::Swipe => "swipe",
            Kind::PitEe => "pit-ee",
            Kind::PitEr => "pit-er",
            Kind::Mixed => "mixed",
        }
    }

    fn is_wall(self) -> bool {
        matches!(self, Kind::WallFront | Kind::WallOffset | Kind::WallAngle)
    }
}

const DEFAULT_KINDS: [Kind; 7] = [
    Kind::WallFront,
    Kind::WallOffset,
    Kind::WallAngle,
    Kind::CarFront,
    Kind::CarOffset,
    Kind::CarSide,
    Kind::Mixed,
];
const LEGACY_KINDS: [Kind; 3] = [Kind::Swipe, Kind::PitEe, Kind::PitEr];

/// Damage state after one hit.
#[derive(Clone)]
struct After {
    health: f32,
    minor: bool,
    degraded: bool,
    out: bool,
    wreck: bool,
    bill: f32,
}

fn mech_failed(d: &wp_damage::DamageModel) -> bool {
    use Component::*;
    [
        Engine,
        Radiator,
        SteeringRack,
        SuspensionFL,
        SuspensionFR,
        SuspensionRL,
        SuspensionRR,
        WheelFL,
        WheelFR,
        WheelRL,
        WheelRR,
        Gearbox,
        Driveshaft,
        BrakeFL,
        BrakeFR,
        BrakeRL,
        BrakeRR,
    ]
    .iter()
    .any(|c| d.component_failed(*c))
}

fn read(d: &wp_damage::DamageModel, mode: &ModeTable, disengage: f32) -> After {
    let fx: HandlingEffects = DamageSim::handling_effects(d);
    let health = d.health();
    let toe = fx.toe_delta.iter().fold(0.0f32, |m, t| m.max(t.abs()));
    let punct = fx.tire_puncture.iter().fold(0.0f32, |m, p| m.max(*p));
    let grip = fx.grip_scale.iter().fold(1.0f32, |m, g| m.min(*g));
    let brake = fx.brake_scale.iter().fold(1.0f32, |m, b| m.min(*b));
    let wheel = fx.wheel_lost.iter().any(|w| *w);
    let minor = toe > 0.01
        || grip < 0.9
        || fx.power_scale < 0.85
        || fx.steer_ratio_scale < 0.85
        || fx.steer_pull.abs() > 0.03
        || brake < 0.8
        || punct > 0.3;
    let degraded = fx.power_scale < 0.75
        || fx.steer_ratio_scale < 0.75
        || grip < 0.8
        || brake < 0.6
        || punct > 0.5
        || health < 0.5
        || mech_failed(d);
    let wreck = d.is_total_loss() || fx.engine_dead || wheel || fx.power_scale < 0.35;
    let out = health < disengage || toe > 0.02 || punct > 0.5 || wheel || fx.engine_dead;
    let bill = d
        .repair_estimate(mode, &wp_damage::default_price_table())
        .total;
    After {
        health,
        minor: minor || degraded || wreck,
        degraded: degraded || wreck,
        out: out || wreck,
        wreck,
        bill,
    }
}

/// Half length of a car, m.
fn half_len(car: &LoadedCar) -> f32 {
    car.def.body_size_m[1] * 0.5
}

/// Half width of a car, m.
fn half_w(car: &LoadedCar) -> f32 {
    car.def.body_size_m[0] * 0.5
}

fn disengage(mode: DamageMode) -> f32 {
    match mode {
        DamageMode::Arcade | DamageMode::Simulation => 0.15,
        DamageMode::Extreme => 0.4,
    }
}

/// A sim with the measured car (and the other car unless `walls_only`), panels attached.
fn setup(
    data: &FsData,
    mode: DamageMode,
    measured: &LoadedCar,
    other: Option<&LoadedCar>,
) -> (Sim, EntityId, Option<EntityId>) {
    let table = load_mode(data, mode).expect("mode table");
    let mut sim = Sim::new(SimConfig {
        seed: 7,
        mode: table,
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 1.0,
    });
    let m_id = sim.spawn_vehicle(measured, [0.0, 0.0, 0.0], 0.0).unwrap();
    let o_id = other.map(|o| sim.spawn_vehicle(o, [30.0, 0.0, 0.0], 0.0).unwrap());
    for id in [Some(m_id), o_id].into_iter().flatten() {
        sim.set_assists(id, AssistProfile::ai()).unwrap();
        let car_id = sim.vehicles()[sim_idx(&sim, id)].car_id.clone();
        let rel = format!("panels/{car_id}.json");
        // Panels on, as in the browser game (`WP_NO_PANELS=1` turns them off).
        let txt = std::fs::read_to_string(Path::new("data").join(&rel));
        if let (Ok(txt), None) = (txt, std::env::var_os("WP_NO_PANELS")) {
            let spec = wp_damage::PanelSpec::parse(&car_id, &txt).expect("panel spec");
            sim.damage_mut(id).unwrap().attach_panels(&spec).unwrap();
        }
    }
    (sim, m_id, o_id)
}

/// Walls are placed far from the origin, one per wall kind, so one sim can hold all of them.
fn wall_origin(kind: Kind) -> [f32; 2] {
    match kind {
        Kind::WallFront => [0.0, 1000.0],
        Kind::WallOffset => [200.0, 1000.0],
        _ => [400.0, 1000.0],
    }
}

/// Add the static walls every wall kind uses for `mc` (call once, before the first tick).
fn add_walls(sim: &mut Sim, mc: &LoadedCar) {
    let gap = half_len(mc) + 2.0;
    let [x, y] = wall_origin(Kind::WallFront);
    sim.add_static_box([x, y + gap, 0.75], [10.0, 1.0, 1.5], 0.0);
    // Offset: wall edge at 40 % of the width from the left side.
    let [x, y] = wall_origin(Kind::WallOffset);
    let edge = x - half_w(mc) + 0.4 * 2.0 * half_w(mc);
    sim.add_static_box([edge - 5.0, y + gap, 0.75], [10.0, 1.0, 1.5], 0.0);
    // Angle: a long wall 30° off square, crossing the travel line 4 m ahead of the bumper.
    let [x, y] = wall_origin(Kind::WallAngle);
    let yaw = -std::f32::consts::FRAC_PI_6;
    // sin / cos of -30°.
    let (s, c) = (-0.5f32, 0.866_025_4f32);
    // Box local +X runs along the wall; push its centre 0.5 m along its normal so the face
    // passes through the crossing point.
    let cross = [x, y + half_len(mc) + 4.0];
    let n = [-s, c]; // local +Y in world
    sim.add_static_box(
        [cross[0] + 0.5 * n[0], cross[1] + 0.5 * n[1], 0.75],
        [40.0, 1.0, 1.5],
        yaw,
    );
}

/// Put the cars in place for one hit of `kind` at `v` m/s; `dx` shifts the measured car
/// sideways (or, for `car-side`, along its length) to move the impact point.
#[allow(clippy::too_many_arguments)]
fn place(
    sim: &mut Sim,
    kind: Kind,
    m: EntityId,
    o: Option<EntityId>,
    mc: &LoadedCar,
    oc: &LoadedCar,
    v: f32,
    dx: f32,
) {
    let coast = VehicleInput::default();
    // Park the other car well away for wall hits.
    if kind.is_wall() {
        let [x, y] = wall_origin(kind);
        sim.place_vehicle(m, [x + dx, y, 0.0], 0.0, v).unwrap();
        if let Some(o) = o {
            sim.place_vehicle(o, [-300.0, 0.0, 0.0], 0.0, 0.0).unwrap();
        }
    }
    match kind {
        Kind::WallFront | Kind::WallOffset | Kind::WallAngle => {}
        Kind::Mixed => unreachable!("resolved per hit"),
        Kind::Swipe => {
            let o = o.unwrap();
            sim.place_vehicle(m, [dx, 0.0, 0.0], 0.0, v).unwrap();
            // 20° in from the left, 0.6 m gap at the nearest corner.
            let x = -(half_w(mc) + half_w(oc) + 0.6 + 0.35 * half_len(oc));
            sim.place_vehicle(o, [x, 0.5, 0.0], -0.349, v).unwrap();
        }
        Kind::CarSide => {
            let o = o.unwrap();
            sim.place_vehicle(m, [0.0, dx, 0.0], 0.0, 0.0).unwrap();
            // Driver door: slightly ahead of centre; striker from -X heading +X.
            let x = -(half_w(mc) + half_len(oc) + 1.5);
            sim.place_vehicle(o, [x, 0.3, 0.0], -std::f32::consts::FRAC_PI_2, v)
                .unwrap();
        }
        Kind::CarFront | Kind::CarOffset => {
            let o = o.unwrap();
            sim.place_vehicle(m, [dx, 0.0, 0.0], 0.0, v).unwrap();
            let y = half_len(mc) + half_len(oc) + 1.5;
            // Offset: the struck car's right edge at the measured car's centre line.
            let x = if kind == Kind::CarOffset {
                -half_w(oc)
            } else {
                0.0
            };
            sim.place_vehicle(o, [x, y, 0.0], 0.0, 0.0).unwrap();
        }
        Kind::PitEe | Kind::PitEr => {
            let (target, pitter, tc, pc) = if kind == Kind::PitEe {
                (m, o.unwrap(), mc, oc)
            } else {
                (o.unwrap(), m, oc, mc)
            };
            sim.place_vehicle(target, [0.0, 0.0, 0.0], 0.0, v).unwrap();
            // Pitter's front-right corner just behind-left of the target's rear-left quarter.
            let x = -(half_w(tc) + half_w(pc) + 0.3);
            let y = -half_len(tc) - half_len(pc) + 1.0;
            sim.place_vehicle(pitter, [x, y, 0.0], -0.19, v * 1.1)
                .unwrap();
        }
    }
    sim.set_input(m, coast).unwrap();
    if let Some(o) = o {
        sim.set_input(o, coast).unwrap();
    }
}

fn verbose(label: &str, d: &wp_damage::DamageModel) {
    if std::env::var_os("WP_VERBOSE").is_none() {
        return;
    }
    let fx = DamageSim::handling_effects(d);
    let toe = fx.toe_delta.iter().fold(0.0f32, |m, t| m.max(t.abs()));
    let grip = fx.grip_scale.iter().fold(1.0f32, |m, g| m.min(*g));
    let punct = fx.tire_puncture.iter().fold(0.0f32, |m, p| m.max(*p));
    eprintln!(
        "  {label}: h {:.3} tl {} toe {:.4} grip {:.2} punct {:.2} power {:.2} steer {:.2} \
         pull {:.3} wheel {:?} dead {} crush {:.2} cabin {:.2} failed {:?}",
        d.health(),
        d.is_total_loss(),
        toe,
        grip,
        punct,
        fx.power_scale,
        fx.steer_ratio_scale,
        fx.steer_pull,
        fx.wheel_lost,
        fx.engine_dead,
        d.structural_crush_m(),
        d.cabin_intrusion_m(),
        failed_list(d),
    );
}

fn failed_list(d: &wp_damage::DamageModel) -> Vec<Component> {
    d.component_health_map()
        .keys()
        .copied()
        .filter(|c| d.component_failed(*c))
        .collect()
}

/// Run one row: `MAX_HITS` repeats of `kind` at `kmh` on `measured`.
fn row(
    data: &FsData,
    mode: DamageMode,
    measured: &LoadedCar,
    other: &LoadedCar,
    kind: Kind,
    kmh: f32,
    dx: f32,
) -> Vec<After> {
    let table = load_mode(data, mode).expect("mode table");
    let (mut sim, m_id, o_id) = setup(data, mode, measured, Some(other));
    add_walls(&mut sim, measured);
    let v = kmh / 3.6;
    let mut out = Vec::new();
    for hit in 0..MAX_HITS {
        let k = if kind == Kind::Mixed {
            DEFAULT_KINDS[hit % 6]
        } else {
            kind
        };
        place(&mut sim, k, m_id, o_id, measured, other, v, dx);
        let ticks = (HIT_S / wp_contracts::PHYSICS_DT) as usize;
        for _ in 0..ticks {
            sim.step_fixed();
        }
        sim.drain_events();
        let d = &sim.vehicles()[sim_idx(&sim, m_id)].damage;
        verbose(
            &format!(
                "{} {} {kmh} hit {}",
                measured.def.id,
                k.name(),
                out.len() + 1
            ),
            d,
        );
        out.push(read(d, &table, disengage(mode)));
        if let Some(o) = o_id {
            sim.repair_vehicle(o).unwrap();
        }
    }
    out
}

fn sim_idx(sim: &Sim, id: EntityId) -> usize {
    sim.vehicles().iter().position(|v| v.id == id).unwrap()
}

/// Median over the variants of the hit on which `f` first holds (`MAX_HITS + 1` = never).
fn first(runs: &[Vec<After>], f: impl Fn(&After) -> bool) -> String {
    let mut n: Vec<usize> = runs
        .iter()
        .map(|hits| hits.iter().position(&f).map_or(MAX_HITS + 1, |i| i + 1))
        .collect();
    n.sort_unstable();
    match n[n.len() / 2] {
        i if i > MAX_HITS => format!(">{MAX_HITS}"),
        i => format!("{i}"),
    }
}

/// Lateral shifts of the impact point; every row runs once per shift.
const VARIANTS_DX: [f32; 3] = [-0.15, 0.0, 0.15];

/// Median over the variants of `f` (applied to one variant's hits).
fn median(runs: &[Vec<After>], f: impl Fn(&[After]) -> f32) -> f32 {
    let mut v: Vec<f32> = runs.iter().map(|r| f(r.as_slice())).collect();
    v.sort_by(|a, b| a.total_cmp(b));
    v[v.len() / 2]
}

/// The median variant's state after the first hit.
fn hit1(runs: &[Vec<After>]) -> &'static str {
    let rank = |a: &After| (a.wreck as u8) * 3 + (a.degraded as u8) + (a.minor as u8);
    let mut v: Vec<&After> = runs.iter().map(|r| &r[0]).collect();
    v.sort_by_key(|a| rank(a));
    let a = v[v.len() / 2];
    if a.wreck {
        "wreck"
    } else if a.degraded {
        "degraded"
    } else if a.minor {
        "minor"
    } else {
        "cosmetic"
    }
}

/// Chase script: (kind, km/h).
const CHASE: [(Kind, f32); 8] = [
    (Kind::Swipe, 70.0),
    (Kind::PitEe, 80.0),
    (Kind::CarFront, 60.0),
    (Kind::WallAngle, 60.0),
    (Kind::CarSide, 50.0),
    (Kind::WallOffset, 70.0),
    (Kind::Swipe, 90.0),
    (Kind::CarFront, 80.0),
];

/// Chase result line for one car.
fn chase(data: &FsData, mode: DamageMode, car: &LoadedCar) -> String {
    let sedan = load_car(data, "debug_sedan").unwrap();
    let cruiser = load_car(data, "sentinel_cruiser").unwrap();
    let table = load_mode(data, mode).expect("mode table");
    // The other car is swapped per event (sedan or cruiser): keep both in the world.
    let (mut sim, m_id, s_id) = setup(data, mode, car, Some(&sedan));
    let c_id = sim.spawn_vehicle(&cruiser, [-30.0, 0.0, 0.0], 0.0).unwrap();
    sim.set_assists(c_id, AssistProfile::ai()).unwrap();
    add_walls(&mut sim, car);
    let mut engine_t: Option<f32> = None;
    let mut steer_t: Option<f32> = None;
    let mut wreck_t: Option<f32> = None;
    let ticks = (CHASE_GAP_S / wp_contracts::PHYSICS_DT) as usize;
    for (i, (kind, kmh)) in CHASE.iter().enumerate() {
        let (oid, oc, park) = if *kind == Kind::PitEe {
            (c_id, &cruiser, s_id.unwrap())
        } else {
            (s_id.unwrap(), &sedan, c_id)
        };
        sim.place_vehicle(park, [-300.0, 50.0 * i as f32, 0.0], 0.0, 0.0)
            .unwrap();
        place(&mut sim, *kind, m_id, Some(oid), car, oc, kmh / 3.6, 0.0);
        for t in 0..ticks {
            sim.step_fixed();
            if t % 24 == 0 {
                let now = i as f32 * CHASE_GAP_S + t as f32 * wp_contracts::PHYSICS_DT;
                let d = &sim.vehicles()[sim_idx(&sim, m_id)].damage;
                let fx = DamageSim::handling_effects(d);
                if engine_t.is_none() && fx.power_scale < 0.85 {
                    engine_t = Some(now);
                }
                if steer_t.is_none() && fx.steer_ratio_scale < 0.85 {
                    steer_t = Some(now);
                }
                if wreck_t.is_none() && read(d, &table, disengage(mode)).wreck {
                    wreck_t = Some(now);
                }
            }
        }
        sim.drain_events();
        verbose(
            &format!("chase {} {} {kmh}", car.def.id, kind.name()),
            &sim.vehicles()[sim_idx(&sim, m_id)].damage,
        );
        sim.repair_vehicle(s_id.unwrap()).unwrap();
        sim.repair_vehicle(c_id).unwrap();
    }
    let d = &sim.vehicles()[sim_idx(&sim, m_id)].damage;
    let fx = DamageSim::handling_effects(d);
    let worst = |cs: &[Component]| cs.iter().fold(1.0f32, |m, c| m.min(d.component_health(*c)));
    use Component::*;
    let t = |o: Option<f32>| o.map_or("never".to_string(), |s| format!("{s:.0} s"));
    format!(
        "| {} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} | {} | {} | {} | {:.0} |",
        car.def.id,
        d.health(),
        d.component_health(Engine),
        d.component_health(Radiator),
        d.component_health(SteeringRack),
        worst(&[SuspensionFL, SuspensionFR, SuspensionRL, SuspensionRR]),
        worst(&[WheelFL, WheelFR, WheelRL, WheelRR]),
        fx.power_scale,
        fx.steer_ratio_scale,
        t(engine_t),
        t(steer_t),
        t(wreck_t),
        d.repair_estimate(&table, &wp_damage::default_price_table())
            .total,
    )
}

/// Run `jobs` on all cores, results in input order.
fn par_map<T: Sync, R: Send>(jobs: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let n = std::thread::available_parallelism().map_or(4, |n| n.get());
    let next = std::sync::atomic::AtomicUsize::new(0);
    let out: Vec<std::sync::Mutex<Option<R>>> =
        jobs.iter().map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|s| {
        for _ in 0..n {
            s.spawn(|| loop {
                let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                if i >= jobs.len() {
                    break;
                }
                let r = f(&jobs[i]);
                *out[i].lock().unwrap() = Some(r);
            });
        }
    });
    out.into_iter()
        .map(|m| m.into_inner().unwrap().unwrap())
        .collect()
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let chase_mode = args.first().map(String::as_str) == Some("chase");
    if chase_mode {
        args.remove(0);
    }
    let mode = match args.first().map(String::as_str) {
        Some("sim") => {
            args.remove(0);
            DamageMode::Simulation
        }
        Some("extreme") => {
            args.remove(0);
            DamageMode::Extreme
        }
        Some("arcade") => {
            args.remove(0);
            DamageMode::Arcade
        }
        _ => DamageMode::Arcade,
    };
    if args.is_empty() {
        args = [
            "hakone_gt",
            "debug_sedan",
            "junkyard_beater",
            "sentinel_cruiser",
        ]
        .map(String::from)
        .to_vec();
    }
    let data = FsData::new("data");
    let cars: Vec<LoadedCar> = args.iter().map(|c| load_car(&data, c).unwrap()).collect();
    if chase_mode {
        println!(
            "| car | health | engine | radiator | steering | worst susp. | worst wheel | power | \
             steer ratio | engine impaired | steering impaired | wrecked | bill |"
        );
        println!("|---|---|---|---|---|---|---|---|---|---|---|---|---|");
        for line in par_map(&cars, |c| chase(&data, mode, c)) {
            println!("{line}");
        }
        return;
    }
    let sedan = load_car(&data, "debug_sedan").unwrap();
    let cruiser = load_car(&data, "sentinel_cruiser").unwrap();
    let kinds: Vec<Kind> = match std::env::var("WP_KINDS") {
        Ok(only) => DEFAULT_KINDS
            .iter()
            .chain(LEGACY_KINDS.iter())
            .copied()
            .filter(|k| only.split(',').any(|n| n == k.name()))
            .collect(),
        Err(_) => DEFAULT_KINDS.to_vec(),
    };
    let speeds: Vec<f32> = match std::env::var("WP_SPEEDS") {
        Ok(s) => s.split(',').map(|v| v.parse().unwrap()).collect(),
        Err(_) => vec![30.0, 50.0, 70.0, 100.0],
    };
    let mut jobs = Vec::new();
    for (ci, _) in cars.iter().enumerate() {
        for kind in &kinds {
            for kmh in &speeds {
                for dx in VARIANTS_DX {
                    jobs.push((ci, *kind, *kmh, dx));
                }
            }
        }
    }
    let flat = par_map(&jobs, |(ci, kind, kmh, dx)| {
        let other = if *kind == Kind::PitEe {
            &cruiser
        } else {
            &sedan
        };
        row(&data, mode, &cars[*ci], other, *kind, *kmh, *dx)
    });
    let nv = VARIANTS_DX.len();
    let rows: Vec<_> = jobs
        .iter()
        .step_by(nv)
        .map(|(c, k, v, _)| (c, k, v))
        .collect();
    let results: Vec<&[Vec<After>]> = flat.chunks(nv).collect();
    println!("| car | crash | km/h | hit1 | minor | degr | out | wreck | h1 | h3 | bill1 |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    for ((ci, kind, kmh), hits) in rows.into_iter().zip(results) {
        println!(
            "| {} | {} | {kmh:.0} | {} | {} | {} | {} | {} | {:.2} | {:.2} | {:.0} |",
            cars[*ci].def.id,
            kind.name(),
            hit1(hits),
            first(hits, |a| a.minor),
            first(hits, |a| a.degraded),
            first(hits, |a| a.out),
            first(hits, |a| a.wreck),
            median(hits, |r| r[0].health),
            median(hits, |r| r[2].health),
            median(hits, |r| r[0].bill),
        );
    }
}
