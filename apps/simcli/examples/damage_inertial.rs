//! Landing and kerb-strike loads (feature F8, workstream 03): what a drop or a kerb does to a car.
//!
//! Drops a car flat from a height onto the flat test ground, and drives it square over a kerb
//! (a 1 m-deep static box of the given height across the road), then prints per run the peak
//! tyre load (N, and as a multiple of the car's static corner load), the peak vertical chassis
//! acceleration (g), and what the damage model made of it: health, plastic energy (kJ) and the
//! worst suspension health. `WP_VERBOSE=1` also prints the synthetic landing / kerb contacts
//! (`wp_damage::inertial`) each run produced.
//!
//! ```sh
//! cargo run --release -p simcli --example damage_inertial -- [arcade|sim|extreme] [car …]
//! ```
//! Defaults: Simulation, `debug_sedan sentinel_cruiser`.

use simcli::data::{load_car, load_mode};
use simcli::FsData;
use wp_contracts::{AssistProfile, Component, DamageMode, DamageSim, TimeOfDay, VehicleInput};
use wp_sim::{Sim, SimConfig};

/// One measured run.
struct Run {
    peak_fz: f32,
    peak_az_g: f32,
    health: f32,
    plastic_kj: f32,
    worst_susp: f32,
}

fn measure(sim: &mut Sim, idx: usize, seconds: f32, input: VehicleInput) -> Run {
    let id = sim.vehicles()[idx].id;
    sim.set_input(id, input).unwrap();
    let dt = wp_contracts::PHYSICS_DT;
    let mut peak_fz = 0.0f32;
    let mut peak_az = 0.0f32;
    let mut last_vz = sim.vehicles()[idx].vehicle.chassis().lin_vel.z;
    sim.set_record_applied_contacts(true);
    let mut synth = 0usize;
    let mut synth_j = 0.0f32;
    for _ in 0..(seconds / dt) as usize {
        sim.step_fixed();
        for (_, cs) in sim.drain_applied_contacts() {
            for c in cs
                .iter()
                .filter(|c| c.other == Some(wp_damage::inertial::INERTIAL_SOURCE))
            {
                synth += 1;
                synth_j += c.impulse_ns;
                if std::env::var_os("WP_VERBOSE").is_some() {
                    eprintln!(
                        "    j {:.0} v {:.2} p {:?}",
                        c.impulse_ns, c.rel_speed_mps, c.point_local_m
                    );
                }
            }
        }
        let v = &sim.vehicles()[idx].vehicle;
        for f in v.wheel_loads() {
            peak_fz = peak_fz.max(f);
        }
        let vz = v.chassis().lin_vel.z;
        peak_az = peak_az.max((vz - last_vz) / dt);
        last_vz = vz;
    }
    sim.drain_events();
    let d = &sim.vehicles()[idx].damage;
    let worst_susp = [
        Component::SuspensionFL,
        Component::SuspensionFR,
        Component::SuspensionRL,
        Component::SuspensionRR,
    ]
    .iter()
    .map(|c| d.component_health(*c))
    .fold(1.0f32, f32::min);
    if std::env::var_os("WP_VERBOSE").is_some() {
        eprintln!("  synthetic contacts {synth}, {synth_j:.0} N·s");
    }
    Run {
        peak_fz,
        peak_az_g: peak_az / 9.81,
        health: d.health(),
        plastic_kj: d.total_plastic_energy_j() * 1e-3,
        worst_susp,
    }
}

fn fresh(data: &FsData, mode: DamageMode, car: &str, kerb_h: Option<f32>) -> (Sim, usize) {
    let table = load_mode(data, mode).expect("mode table");
    let mut sim = Sim::new(SimConfig {
        seed: 7,
        mode: table,
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 1.0,
    });
    if let Some(h) = kerb_h {
        // Across the road 20 m ahead, 1 m deep, its top `h` above the ground.
        sim.add_static_box([0.0, 20.0, h * 0.5], [30.0, 1.0, h], 0.0);
    }
    let c = load_car(data, car).unwrap();
    let id = sim.spawn_vehicle(&c, [0.0, 0.0, 0.0], 0.0).unwrap();
    sim.set_assists(id, AssistProfile::ai()).unwrap();
    (sim, 0)
}

fn row(label: &str, static_fz: f32, r: &Run) {
    println!(
        "| {label} | {:.0} | {:.1} | {:.1} | {:.3} | {:.2} | {:.2} |",
        r.peak_fz,
        r.peak_fz / static_fz,
        r.peak_az_g,
        r.health,
        r.plastic_kj,
        r.worst_susp
    );
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let mode = match args.first().map(String::as_str) {
        Some("arcade") => DamageMode::Arcade,
        Some("extreme") => DamageMode::Extreme,
        Some("sim") => DamageMode::Simulation,
        _ => DamageMode::Simulation,
    };
    if matches!(
        args.first().map(String::as_str),
        Some("arcade" | "sim" | "extreme")
    ) {
        args.remove(0);
    }
    if args.is_empty() {
        args = vec!["debug_sedan".into(), "sentinel_cruiser".into()];
    }
    let data = FsData::new("data");
    println!("mode {mode:?}");
    println!(
        "| run | peak tyre load N | × static | peak az g | health | plastic kJ | worst susp |"
    );
    println!("|---|---|---|---|---|---|---|");
    for car in &args {
        let mass = load_car(&data, car).unwrap().def.mass_kg;
        let static_fz = mass * 9.81 / 4.0;
        for h in [0.5f32, 1.0, 2.0, 3.0, 5.0] {
            let (mut sim, i) = fresh(&data, mode, car, None);
            let id = sim.vehicles()[i].id;
            sim.place_vehicle(id, [0.0, 0.0, h], 0.0, 0.0).unwrap();
            let r = measure(&mut sim, i, 3.0, VehicleInput::default());
            row(&format!("{car} drop {h} m"), static_fz, &r);
        }
        for (kerb, kmh) in [(0.15f32, 50.0f32), (0.15, 80.0), (0.25, 50.0), (0.25, 80.0)] {
            let (mut sim, i) = fresh(&data, mode, car, Some(kerb));
            let id = sim.vehicles()[i].id;
            sim.place_vehicle(id, [0.0, 0.0, 0.0], 0.0, kmh / 3.6)
                .unwrap();
            let r = measure(&mut sim, i, 3.0, VehicleInput::default());
            row(&format!("{car} kerb {kerb} m at {kmh} km/h"), static_fz, &r);
        }
    }
}
