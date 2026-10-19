//! Repeated rams on one car's damage model (workstream 09 tool).
//!
//! ```text
//! cargo run --release -p ram-probe -- [car ...] [--speeds 30,40,50] [--other-kg 1400] [--rams 6]
//! ```
//! A ram is `wp_damage`'s barrier-harness impact (`barrier_contacts`) against a moving car of
//! `--other-kg` (car-to-car energy share applies), repeated on the **same** model with 1 s of
//! settling between hits. Full-width (`Frontal`, push bar centred on the target's tail) and
//! 40 % `Offset` (a nudge on one rear corner) rams by the car, and `Side` / `Rear` hits taken
//! from a car of `--other-kg` (the player hitting back), in Arcade and Simulation.
//! Prints health after each ram and the ram that first makes the car a total loss.

use std::path::PathBuf;

use wp_contracts::{DamageSim, ModeTable, PHYSICS_DT};
use wp_damage::barrier::{barrier_contacts, ImpactKind};
use wp_damage::{DamageModel, DamageTuning};

/// `data/police/pursuit.toml` `tactics.disengage_health`: a police unit at or below this
/// health leaves the pursuit ("off" column).
const DISENGAGE_HEALTH: f32 = 0.4;

/// Repo `data/` directory.
fn data_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../data")
}

/// Health after each of `rams` impacts (with structural crush) and the first total-loss ram
/// (1-based).
fn ram_series(
    car: &wp_contracts::LoadedCar,
    mode: &ModeTable,
    kind: ImpactKind,
    speed_kmh: f32,
    other_kg: f32,
    rams: usize,
) -> (Vec<(f32, f32)>, Option<usize>) {
    let mut m = DamageModel::with_tuning(&car.lattice, &car.materials, DamageTuning::default(), 1);
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for n in &car.lattice.nodes {
        for k in 0..3 {
            lo[k] = lo[k].min(n.pos_m[k]);
            hi[k] = hi[k].max(n.pos_m[k]);
        }
    }
    let (c0, c1) = barrier_contacts(lo, hi, kind, speed_kmh / 3.6, car.def.mass_kg, other_kg);
    let settle = (1.0 / PHYSICS_DT).round() as usize;
    let mut events = Vec::new();
    let mut out = Vec::new();
    let mut first_loss = None;
    for i in 0..rams {
        for step in 0..settle {
            let contacts = match step {
                0 => &c0[..],
                1 => &c1[..],
                _ => &[][..],
            };
            m.apply_contacts(contacts, mode, PHYSICS_DT, &mut events);
        }
        out.push((m.health(), m.structural_crush_m()));
        if first_loss.is_none() && m.is_total_loss() {
            first_loss = Some(i + 1);
        }
    }
    (out, first_loss)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let opt = |name: &str| {
        args.iter()
            .position(|a| a == name)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let speeds: Vec<f32> = opt("--speeds")
        .unwrap_or_else(|| "40,60,80".into())
        .split(',')
        .map(|s| s.parse().expect("speed"))
        .collect();
    let other_kg: f32 = opt("--other-kg").map_or(1400.0, |s| s.parse().expect("kg"));
    let rams: usize = opt("--rams").map_or(6, |s| s.parse().expect("count"));
    let mut cars: Vec<String> = Vec::new();
    let mut skip = false;
    for a in &args {
        if skip {
            skip = false;
        } else if a.starts_with("--") {
            skip = true;
        } else {
            cars.push(a.clone());
        }
    }
    if cars.is_empty() {
        cars.push("sentinel_cruiser".into());
    }
    let root = data_root();
    println!(
        "rams against a {other_kg:.0} kg car; health (structural crush m) after each ram; \
         off = first ram leaving health <= {DISENGAGE_HEALTH} (police disengage); TL = first total-loss ram"
    );
    for id in &cars {
        let car = wp_contracts::load_car(root.join("cars").join(format!("{id}.json")))
            .expect("car loads");
        for (mode_name, file) in [("Arcade", "arcade.toml"), ("Sim", "sim.toml")] {
            let mode = wp_contracts::load_mode_table(root.join("modes").join(file))
                .expect("mode loads");
            for kind in [
                ImpactKind::Frontal,
                ImpactKind::Offset,
                ImpactKind::Side,
                ImpactKind::Rear,
            ] {
                for &v in &speeds {
                    let (series, tl) = ram_series(&car, &mode, kind, v, other_kg, rams);
                    let off = series
                        .iter()
                        .position(|(h, _)| *h <= DISENGAGE_HEALTH)
                        .map_or("-".to_string(), |i| (i + 1).to_string());
                    let cells: Vec<String> = series
                        .iter()
                        .map(|(h, c)| format!("{h:.2} ({c:.2})"))
                        .collect();
                    println!(
                        "{id:20} {mode_name:6} {:7} {v:3.0} km/h  off {off:>2}  TL {:>2}  {}",
                        format!("{kind:?}"),
                        tl.map_or("-".to_string(), |n| n.to_string()),
                        cells.join("  ")
                    );
                }
            }
        }
    }
}
