//! Differential numbers for the owner (playtest log row 26) and for tuning.
//!
//! `cargo run --release -p wp_vehicle --example diff_demo [car ids…]`
//!
//! Table 1, per car with its own setup (`data/cars/differentials.toml`):
//! - **split-µ launch:** left wheels on ice, right wheels on dry asphalt, 60 % throttle for
//!   4 s from a standstill, heading held (TCS off; then full throttle with TCS on): distance
//!   and the ice-side driven wheel's spin (surface speed minus road speed).
//! - **hairpin exit:** steady 0.6 g on a 20 m circle, then full throttle for 2 s while the
//!   driver keeps following the circle (slide help 0.6 with TCS off; then TCS on, no slide
//!   help): mean inside driven-wheel slip ratio, speed after 2 s ("exit speed") and peak body
//!   slip (oversteer).
//! - **lift-off:** 0.7 g on a 40 m circle, gear held, throttle off: path-curvature ratio after
//!   1 s (> 1 = the car tucks in / rotates).
//!
//! Table 2: one RWD car (first id, default `shiro_turbo`) with every rear diff type.

use wp_contracts::{AssistProfile, LoadedCar};
use wp_vehicle::diff::{AxleDiff, DiffSetup};
use wp_vehicle::metrics::{
    exit_on_circle_with, lift_off_with, metric_assists, split_mu_launch_with,
};
use wp_vehicle::VehicleExtras;

fn car(id: &str) -> LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).unwrap()
}

fn row(label: &str, c: &LoadedCar, extras: VehicleExtras) {
    let no_tcs = AssistProfile {
        tcs: false,
        ..metric_assists()
    };
    let help = AssistProfile {
        countersteer_help: 0.6,
        ..no_tcs
    };
    let held = AssistProfile {
        auto_gears: false,
        ..no_tcs
    };
    let a = split_mu_launch_with(c, extras, &no_tcs, 0.6, 4.0);
    let b = split_mu_launch_with(c, extras, &metric_assists(), 1.0, 4.0);
    let e = exit_on_circle_with(c, extras, 20.0, 0.6, 1.0, 2.0, &help);
    let t = exit_on_circle_with(c, extras, 20.0, 0.6, 1.0, 2.0, &metric_assists());
    let l = lift_off_with(c, extras, 40.0, 0.7, &held);
    println!(
        "| {label} | {:.1} / {:.1} | {:.1} / {:.1} | {:.2} / {:.0} / {:.0} | {:.2} / {:.0} / {:.0} | {:.3} |",
        a.distance_m,
        a.ice_wheel_slip_mps,
        b.distance_m,
        b.ice_wheel_slip_mps,
        e.inside_slip,
        e.exit_speed_mps * 3.6,
        e.peak_body_slip_deg,
        t.inside_slip,
        t.exit_speed_mps * 3.6,
        t.peak_body_slip_deg,
        l.curvature_ratio,
    );
}

fn header(first: &str) {
    println!("| {first} | split-µ 60 %, TCS off: m in 4 s / ice-wheel spin m/s | split-µ 100 %, TCS on: m / spin m/s | hairpin exit, slide help, TCS off: inside slip / exit km/h / peak slip ° | hairpin exit, TCS on: inside slip / km/h / slip ° | lift-off curvature ratio |");
    println!("|---|---|---|---|---|---|");
}

fn label(c: &LoadedCar) -> String {
    let s = DiffSetup::for_car(&c.def);
    match c.def.drivetrain.layout {
        wp_contracts::DriveLayout::Fwd => s.front.label().to_string(),
        wp_contracts::DriveLayout::Rwd => s.rear.label().to_string(),
        _ => format!(
            "{} / {} / centre {}",
            s.front.label(),
            s.rear.label(),
            s.centre.label()
        ),
    }
}

fn main() {
    let ids: Vec<String> = std::env::args().skip(1).collect();
    let ids = if ids.is_empty() {
        [
            "shiro_turbo",
            "rampart_50",
            "hakone_gt",
            "sperber_32",
            "lucciola_350",
            "ironside_440",
            "sentinel_cruiser",
            "sentinel_bastion",
            "hauler_1500",
            "kestrel_ts",
            "vireo_xs",
            "junkyard_beater",
            "lindwurm_4t",
            "sentinel_wolfhound",
            "debug_coupe",
        ]
        .map(String::from)
        .to_vec()
    } else {
        ids
    };
    header("car (own setup)");
    for id in &ids {
        let c = car(id);
        row(
            &format!("`{id}` ({})", label(&c)),
            &c,
            VehicleExtras::default(),
        );
    }

    println!();
    let base = car(&ids[0]);
    header(&format!("`{}` rear diff", ids[0]));
    let rears = [
        AxleDiff::Open,
        AxleDiff::Clutch {
            preload_nm: 60.0,
            accel: 0.45,
            decel: 0.0,
        },
        AxleDiff::Clutch {
            preload_nm: 60.0,
            accel: 0.45,
            decel: 0.25,
        },
        AxleDiff::Clutch {
            preload_nm: 60.0,
            accel: 0.45,
            decel: 0.45,
        },
        AxleDiff::Viscous {
            nm_per_rads: 40.0,
            max_nm: 1200.0,
        },
        AxleDiff::Helical {
            tbr: 2.5,
            tbr_coast: 1.8,
            preload_nm: 0.0,
        },
        AxleDiff::Active {
            max_nm: 1500.0,
            preload_nm: 0.0,
            accel: 0.3,
            decel: 0.05,
            slip_gain: 150.0,
        },
        AxleDiff::Locked,
    ];
    for rear in rears {
        let extras = VehicleExtras {
            diffs: Some(DiffSetup {
                rear,
                ..DiffSetup::for_car(&base.def)
            }),
            ..VehicleExtras::default()
        };
        row(rear.label(), &base, extras);
    }
}
