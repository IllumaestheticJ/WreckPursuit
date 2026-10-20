//! Repeated-hit toughness table (workstream 03, round 12b; playtest rows 48–49).
//!
//! For a car, a crash kind and a speed, the same crash is repeated on the same car (damage kept,
//! the car put back at speed before every hit; the other car is repaired between hits so every
//! hit is the same) and after every hit the damage state is read. Reports per row the hit on
//! which the car first shows:
//! - **feel**: noticeable handling loss — toe > 0.01 rad, a wheel's grip < 0.9, power < 0.85,
//!   steering ratio < 0.85, |steer pull| > 0.03, a brake < 0.8, a puncture > 0.3;
//! - **out**: a police unit pulls over (`data/police/pursuit.toml`: health below the mode's
//!   `disengage_health`, toe > `bent_wheel_toe_rad` 0.02, puncture > 0.5, wheel lost, engine dead);
//! - **wreck**: undrivable — total loss, engine dead, a wheel lost or power < 0.35.
//!
//! Crash kinds (the measured car is the first one named):
//! - `frontal`: full-width rigid wall at the speed;
//! - `swipe`: a 1400 kg sedan at the same speed converging at 20° into the measured car's side;
//! - `tbone`: the measured car at rest, struck on the driver door by a 1400 kg sedan;
//! - `ram`: the measured car drives into the rear of a 1400 kg sedan at rest (a cop ramming);
//! - `pit-ee`: the measured car at the speed, PIT-ed on its left rear quarter by a cruiser at
//!   1.1× the speed angled 11° in; `pit-er`: the measured car does that PIT to a sedan.
//!
//! ```sh
//! cargo run --release -p simcli --example damage_toughness -- [arcade|sim] [car …]
//! ```
//! Defaults: Arcade, `hakone_gt sentinel_cruiser`. Every row also prints the health after hits 1
//! and 3 (`h1`, `h3`).

use std::path::Path;

use simcli::data::{load_car, load_mode};
use simcli::FsData;
use wp_contracts::{
    AssistProfile, DamageMode, EntityId, HandlingEffects, LoadedCar, TimeOfDay, VehicleInput,
};
use wp_sim::{Sim, SimConfig};

/// Hits per row before giving up.
const MAX_HITS: usize = 8;
/// Seconds simulated per hit (contact, rebound, lattice sleep).
const HIT_S: f32 = 3.0;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Frontal,
    Swipe,
    Tbone,
    PitEe,
    PitEr,
    Ram,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Frontal => "frontal",
            Kind::Swipe => "swipe",
            Kind::Tbone => "tbone",
            Kind::PitEe => "pit-ee",
            Kind::PitEr => "pit-er",
            Kind::Ram => "ram",
        }
    }
}

/// Damage state after one hit.
struct After {
    health: f32,
    feel: bool,
    out: bool,
    wreck: bool,
}

fn read(fx: &HandlingEffects, health: f32, total_loss: bool, disengage: f32) -> After {
    let toe = fx.toe_delta.iter().fold(0.0f32, |m, t| m.max(t.abs()));
    let punct = fx.tire_puncture.iter().fold(0.0f32, |m, p| m.max(*p));
    let grip = fx.grip_scale.iter().fold(1.0f32, |m, g| m.min(*g));
    let brake = fx.brake_scale.iter().fold(1.0f32, |m, b| m.min(*b));
    let wheel = fx.wheel_lost.iter().any(|w| *w);
    let feel = toe > 0.01
        || grip < 0.9
        || fx.power_scale < 0.85
        || fx.steer_ratio_scale < 0.85
        || fx.steer_pull.abs() > 0.03
        || brake < 0.8
        || punct > 0.3;
    let wreck = total_loss || fx.engine_dead || wheel || fx.power_scale < 0.35;
    let out = health < disengage || toe > 0.02 || punct > 0.5 || wheel || fx.engine_dead;
    After {
        health,
        feel: feel || wreck,
        out: out || wreck,
        wreck,
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

/// Run one row: `hits` repeats of `kind` at `kmh` on `measured`.
fn row(
    data: &FsData,
    mode: DamageMode,
    measured: &LoadedCar,
    other: &LoadedCar,
    kind: Kind,
    kmh: f32,
) -> Vec<After> {
    let table = load_mode(data, mode).expect("mode table");
    let disengage = match mode {
        DamageMode::Arcade => 0.15,
        DamageMode::Simulation => 0.4,
    };
    let mut sim = Sim::new(SimConfig {
        seed: 7,
        mode: table,
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 1.0,
    });
    let v = kmh / 3.6;
    let m_id = sim.spawn_vehicle(measured, [0.0, 0.0, 0.0], 0.0).unwrap();
    let o_id = if kind == Kind::Frontal {
        // Wall 1.5 m ahead of the bumper.
        sim.add_static_box([0.0, half_len(measured) + 2.0, 0.75], [10.0, 1.0, 1.5], 0.0);
        None
    } else {
        Some(sim.spawn_vehicle(other, [30.0, 0.0, 0.0], 0.0).unwrap())
    };
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
    let mut out = Vec::new();
    for _ in 0..MAX_HITS {
        place(&mut sim, kind, m_id, o_id, measured, other, v);
        let ticks = (HIT_S / wp_contracts::PHYSICS_DT) as usize;
        for _ in 0..ticks {
            sim.step_fixed();
        }
        sim.drain_events();
        let d = &sim.vehicles()[sim_idx(&sim, m_id)].damage;
        let fx = wp_contracts::DamageSim::handling_effects(d);
        if std::env::var_os("WP_VERBOSE").is_some() {
            let toe = fx.toe_delta.iter().fold(0.0f32, |m, t| m.max(t.abs()));
            let grip = fx.grip_scale.iter().fold(1.0f32, |m, g| m.min(*g));
            let punct = fx.tire_puncture.iter().fold(0.0f32, |m, p| m.max(*p));
            eprintln!(
                "  {} {kmh} hit {}: h {:.3} tl {} toe {:.4} grip {:.2} punct {:.2} power {:.2} \
                 steer {:.2} pull {:.3} wheel {:?} dead {} crush {:.2}",
                kind.name(),
                out.len() + 1,
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
            );
        }
        out.push(read(&fx, d.health(), d.is_total_loss(), disengage));
        if let Some(o) = o_id {
            sim.repair_vehicle(o).unwrap();
        }
    }
    out
}

fn sim_idx(sim: &Sim, id: EntityId) -> usize {
    sim.vehicles().iter().position(|v| v.id == id).unwrap()
}

/// Yaw that heads along +X tilted by `ang` toward +Y... (yaw 0 = +Y, positive turns to -X).
fn place(
    sim: &mut Sim,
    kind: Kind,
    m: EntityId,
    o: Option<EntityId>,
    mc: &LoadedCar,
    oc: &LoadedCar,
    v: f32,
) {
    let coast = VehicleInput::default();
    match kind {
        Kind::Frontal => {
            sim.place_vehicle(m, [0.0, 0.0, 0.0], 0.0, v).unwrap();
        }
        Kind::Swipe => {
            let o = o.unwrap();
            sim.place_vehicle(m, [0.0, 0.0, 0.0], 0.0, v).unwrap();
            // 20° in from the left, 0.6 m gap at the nearest corner.
            let x = -(half_w(mc) + half_w(oc) + 0.6 + 0.35 * half_len(oc));
            sim.place_vehicle(o, [x, 0.5, 0.0], -0.349, v).unwrap();
        }
        Kind::Tbone => {
            let o = o.unwrap();
            sim.place_vehicle(m, [0.0, 0.0, 0.0], 0.0, 0.0).unwrap();
            // Driver door: slightly ahead of centre; striker from -X heading +X.
            let x = -(half_w(mc) + half_len(oc) + 1.5);
            sim.place_vehicle(o, [x, 0.3, 0.0], -std::f32::consts::FRAC_PI_2, v)
                .unwrap();
        }
        Kind::Ram => {
            let o = o.unwrap();
            sim.place_vehicle(m, [0.0, 0.0, 0.0], 0.0, v).unwrap();
            let y = half_len(mc) + half_len(oc) + 1.5;
            sim.place_vehicle(o, [0.0, y, 0.0], 0.0, 0.0).unwrap();
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

fn first(hits: &[After], f: impl Fn(&After) -> bool) -> String {
    match hits.iter().position(f) {
        Some(i) => format!("{}", i + 1),
        None => format!(">{MAX_HITS}"),
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mode = match args.first().map(String::as_str) {
        Some("sim") => {
            args.remove(0);
            DamageMode::Simulation
        }
        Some("arcade") => {
            args.remove(0);
            DamageMode::Arcade
        }
        _ => DamageMode::Arcade,
    };
    if args.is_empty() {
        args = vec!["hakone_gt".into(), "sentinel_cruiser".into()];
    }
    let data = FsData::new("data");
    let sedan = load_car(&data, "debug_sedan").unwrap();
    let cruiser = load_car(&data, "sentinel_cruiser").unwrap();
    println!("| car | crash | km/h | feel | out | wreck | h1 | h3 |");
    println!("|---|---|---|---|---|---|---|---|");
    for car_id in &args {
        let car = load_car(&data, car_id).unwrap();
        for kind in [
            Kind::Frontal,
            Kind::Ram,
            Kind::Swipe,
            Kind::Tbone,
            Kind::PitEe,
            Kind::PitEr,
        ] {
            // `WP_KINDS=frontal,ram` limits the run to those crash kinds.
            if let Ok(only) = std::env::var("WP_KINDS") {
                if !only.split(',').any(|k| k == kind.name()) {
                    continue;
                }
            }
            let other = if kind == Kind::PitEe {
                &cruiser
            } else {
                &sedan
            };
            for kmh in [30.0, 50.0, 70.0, 90.0] {
                let hits = row(&data, mode, &car, other, kind, kmh);
                println!(
                    "| {car_id} | {} | {kmh:.0} | {} | {} | {} | {:.2} | {:.2} |",
                    kind.name(),
                    first(&hits, |a| a.feel),
                    first(&hits, |a| a.out),
                    first(&hits, |a| a.wreck),
                    hits[0].health,
                    hits[2].health,
                );
            }
        }
    }
}
