//! Anti-roll bar split sweep on the shipped handling (unify, lead 2026-10-21): the owner's
//! springs / bars × 1.5 kept overall, the front / rear split moved until the chassis
//! understeer gradients of `tests/handling_balance.rs` pass. Prints per split: every car's
//! gradient (wheel preset, 80 km/h, deg/g; `wp_vehicle::handling::ramp_steer`) and the change in
//! total bar stiffness (front + rear, summed over the cars) against the owner's.
//!
//! `cargo run --release -p wp_vehicle --example bar_split [-- front,rear,fwd_rear ...]`

use wp_contracts::LoadedCar;
use wp_vehicle::grip_lab::{self, GripBalance};
use wp_vehicle::handling::{presets, ramp_steer};

const CARS: [(&str, f32, f32); 17] = [
    // (id, range lo, hi) from tests/handling_balance.rs
    ("vireo_xs", 0.25, 1.6),
    ("kestrel_ts", 0.25, 1.6),
    ("junkyard_beater", 0.25, 1.6),
    ("tansy_13", 0.25, 1.6),
    ("shiro_turbo", 0.0, 0.6),
    ("hakone_gt", 0.0, 0.6),
    ("rampart_50", 0.0, 0.6),
    ("sperber_32", 0.0, 0.6),
    ("lucciola_350", 0.0, 0.6),
    ("sentinel_cruiser", 0.0, 0.6),
    ("lindwurm_4t", 0.1, 0.6),
    ("sentinel_wolfhound", 0.1, 0.6),
    ("hauler_1500", 0.35, 2.3),
    ("ironside_440", 0.35, 2.3),
    ("drayman_cargo", 0.35, 2.3),
    ("furrow_24", 0.35, 2.3),
    ("sentinel_bastion", 0.35, 2.3),
];

fn car(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).unwrap()
}

/// Total bar rate (N/m, front + rear, both corners) with the bar scales of `g` on top of the
/// owner's ride (× 1.5).
fn bar_total(c: &LoadedCar, g: &GripBalance) -> f32 {
    let s = &c.def.suspension;
    1.5 * (s[0].arb_n_per_m * g.arb_scale(true, &c.def.drivetrain.layout)
        + s[2].arb_n_per_m * g.arb_scale(false, &c.def.drivetrain.layout))
}

fn main() {
    let splits: Vec<(f32, f32, f32)> = {
        let a: Vec<String> = std::env::args().skip(1).collect();
        if a.is_empty() {
            vec![
                (1.0, 1.0, 1.0),
                (1.2, 1.0, 1.0),
                (1.4, 1.0, 1.0),
                (1.2, 0.5, 2.0),
                (1.3, 0.5, 2.0),
                (1.4, 0.5, 2.0),
                (1.3, 0.5, 3.0),
            ]
        } else {
            a.iter()
                .map(|t| {
                    let v: Vec<f32> = t.split(',').map(|x| x.parse().unwrap()).collect();
                    (v[0], v[1], v[2])
                })
                .collect()
        }
    };
    let wheel = presets()[2].1;
    let cars: Vec<(&str, f32, f32, LoadedCar)> = CARS
        .iter()
        .map(|(id, lo, hi)| (*id, *lo, *hi, car(id)))
        .collect();
    let base_total: f32 = cars
        .iter()
        .map(|(_, _, _, c)| bar_total(c, &GripBalance::shipped()))
        .sum();
    for (f, r, fr) in splits {
        // REAR_STIFF=k: the RWD rear cornering stiffness scale instead of the owner's 0.45.
        let rear_stiffness = std::env::var("REAR_STIFF")
            .ok()
            .map_or(GripBalance::shipped().rear_stiffness, |v| {
                v.parse().unwrap()
            });
        let g = GripBalance {
            front_arb: f,
            rear_arb: r,
            fwd_rear_arb: fr,
            rear_stiffness,
            // FWD_REAR_STIFF=k: front-drive cars' rear cornering stiffness scale (default 1).
            fwd_rear_stiffness: std::env::var("FWD_REAR_STIFF")
                .ok()
                .map_or(1.0, |v| v.parse().unwrap()),
            ..GripBalance::shipped()
        };
        grip_lab::set_override(Some(g));
        let total: f32 = cars.iter().map(|(_, _, _, c)| bar_total(c, &g)).sum();
        let mut fails = Vec::new();
        let mut cells = Vec::new();
        for (id, lo, hi, c) in &cars {
            let u = ramp_steer(c, &wheel, 80.0 / 3.6).understeer_deg_per_g;
            if !(*lo..=*hi).contains(&u) {
                fails.push(format!("{id} {u:.2}"));
            }
            cells.push(format!("{id} {u:.2}"));
        }
        println!(
            "rear stiffness x{rear_stiffness} front bar x{f} rear x{r} fwd rear x{fr}: total bars {:+.0} %, {} fails {:?}\n  {}",
            100.0 * (total / base_total - 1.0),
            fails.len(),
            fails,
            cells.join(" · ")
        );
    }
}
