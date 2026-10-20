//! Ride and launch sweep (owner playtest row 54, "too bouncy / light, launched too easily"):
//! for each car, the [`wp_vehicle::ride`] tests as markdown tables.
//!
//! `cargo run --release -p wp_vehicle --example ride [car_id ...]` (default: the four
//! reference cars sports / sedan / muscle / police cruiser; `all` = whole roster).

use std::thread;

use wp_contracts::{AssistProfile, LoadedCar};
use wp_vehicle::ride::{
    axle_ride, crest, heave_kick, kerb, pitch_kick, ride_assists, side_impact, slide_into_kerb,
    Oscillation,
};

const REFERENCE: [&str; 4] = ["hakone_gt", "kestrel_ts", "rampart_50", "sentinel_cruiser"];
const ROSTER: [&str; 17] = [
    "vireo_xs",
    "kestrel_ts",
    "shiro_turbo",
    "hakone_gt",
    "rampart_50",
    "ironside_440",
    "sperber_32",
    "lindwurm_4t",
    "lucciola_350",
    "hauler_1500",
    "sentinel_cruiser",
    "sentinel_wolfhound",
    "sentinel_bastion",
    "junkyard_beater",
    "tansy_13",
    "drayman_cargo",
    "furrow_24",
];

fn load(id: &str) -> LoadedCar {
    let p = if id.ends_with(".json") {
        id.to_string()
    } else {
        format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"))
    };
    wp_contracts::load_car(p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn osc(o: &Oscillation) -> String {
    if o.damped_hz > 0.0 {
        format!("{:.2} Hz ζ {:.2}", o.natural_hz, o.damping_ratio)
    } else {
        "no overshoot".into()
    }
}

fn row(id: &str) -> Vec<String> {
    let car = load(id);
    let wheel = ride_assists();
    let kb = AssistProfile::keyboard_default();
    let ax = axle_ride(&car);
    let hv = heave_kick(&car, 0.3);
    let pt = pitch_kick(&car, 0.3);
    let mut out = vec![format!(
        "| {id} | {:.2} / {:.2} | {:.2}–{:.2} / {:.2}–{:.2} | {:.0} / {:.0} | {} | {} |",
        ax[0].ride_hz,
        ax[1].ride_hz,
        ax[0].zeta_bump,
        ax[0].zeta_rebound,
        ax[1].zeta_bump,
        ax[1].zeta_rebound,
        ax[0].bump_travel_m * 1000.0,
        ax[1].bump_travel_m * 1000.0,
        osc(&hv),
        osc(&pt),
    )];
    for (kmh, h, ang) in [
        (30.0, 0.10, 0.0),
        (60.0, 0.10, 0.0),
        (60.0, 0.15, 0.0),
        (60.0, 0.15, 20.0),
    ] {
        let k = kerb(&car, kmh, h, ang, &wheel);
        out.push(format!(
            "| {id} | kerb {:.0} cm {kmh:.0} km/h {ang:.0}° | vz {:.2} | rise {:.3} | pitch {:.2} roll {:.2} | air {:.3} s, wheel {:.3} s | {} | {} |",
            h * 100.0,
            k.peak_vz_mps,
            k.peak_rise_m,
            k.peak_pitch_rate,
            k.peak_roll_rate,
            k.airborne_s,
            k.wheel_lift_s,
            k.overshoots,
            k.settle_s.map_or("—".into(), |s| format!("{s:.2} s")),
        ));
    }
    for (kmh, curve) in [(120.0, 20.0), (120.0, 0.0)] {
        let c = crest(&car, kmh, 0.03, 30.0, curve, &wheel);
        out.push(format!(
            "| {id} | crest 3 % {kmh:.0} km/h curve {curve:.0} m | air {:.3} s, wheel {:.3} s | rise {:.3} | land {:.2} m/s | {} |",
            c.airborne_s, c.wheel_lift_s, c.peak_rise_m, c.landing_vz_mps, c.overshoots
        ));
    }
    for kmh in [20.0, 35.0] {
        let s = slide_into_kerb(&car, kmh, 0.15, &wheel);
        out.push(format!(
            "| {id} | slide into 15 cm kerb {kmh:.0} km/h | roll rate {:.2} | roll {:.1}° | vz {:.2} | wheel {:.3} s air {:.3} s | rolled {} |",
            s.peak_roll_rate,
            s.peak_roll.to_degrees(),
            s.peak_vz_mps,
            s.wheel_lift_s,
            s.airborne_s,
            s.rolled
        ));
    }
    let j = car.def.mass_kg * 7.0; // T-bone at ~50 km/h by a similar car: Δv ≈ 7 m/s
    let s = side_impact(&car, j, 0.45, &wheel);
    out.push(format!(
        "| {id} | side impact Δv 7 m/s @0.45 m | roll rate {:.2} | roll {:.1}° | vz {:.2} | wheel {:.3} s air {:.3} s | rolled {} |",
        s.peak_roll_rate,
        s.peak_roll.to_degrees(),
        s.peak_vz_mps,
        s.wheel_lift_s,
        s.airborne_s,
        s.rolled
    ));
    // Keyboard preset (arcade anti-launch on) on the hardest cases.
    let k = kerb(&car, 60.0, 0.15, 0.0, &kb);
    let c = crest(&car, 120.0, 0.03, 30.0, 0.0, &kb);
    let s = slide_into_kerb(&car, 35.0, 0.15, &kb);
    out.push(format!(
        "| {id} | keyboard: kerb 15 cm 60 km/h / sharp crest 120 / slide 35 | kerb vz {:.2} rise {:.3} air {:.3} s wheel {:.3} s | crest air {:.3} s rise {:.3} | slide roll {:.1}° wheel {:.3} s |",
        k.peak_vz_mps, k.peak_rise_m, k.airborne_s, k.wheel_lift_s, c.airborne_s, c.peak_rise_m,
        s.peak_roll.to_degrees(), s.wheel_lift_s
    ));
    out
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ids: Vec<String> = if args.is_empty() {
        REFERENCE.iter().map(|s| s.to_string()).collect()
    } else if args[0] == "all" {
        ROSTER.iter().map(|s| s.to_string()).collect()
    } else {
        args
    };
    let handles: Vec<_> = ids
        .into_iter()
        .map(|id| thread::spawn(move || row(&id)))
        .collect();
    println!("| car | ride Hz F/R | ζ bump–rebound F / R | bump travel mm F/R | heave kick | pitch kick |");
    println!("|---|---|---|---|---|---|");
    let rows: Vec<Vec<String>> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    for r in &rows {
        println!("{}", r[0]);
    }
    println!();
    println!("| car | test | ... |");
    println!("|---|---|---|");
    for r in &rows {
        for l in &r[1..] {
            println!("{l}");
        }
    }
}
