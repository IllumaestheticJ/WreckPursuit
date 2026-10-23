//! Brake dive sweep (owner playtest row 92, "headlights clip into the ground under braking"):
//! for each car, [`wp_vehicle::brake_dive`] from 100 and 60 km/h as a markdown table.
//!
//! `cargo run --release -p wp_vehicle --example brake_dive [car_id ...]` (default: the whole
//! roster).

use wp_contracts::AssistProfile;
use wp_contracts::LoadedCar;
use wp_vehicle::brake_dive::{brake_dive, clearance_on_front_bump_stops, owner_default_assists};

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
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ids: Vec<String> = if args.is_empty() {
        ROSTER.iter().map(|s| s.to_string()).collect()
    } else {
        args
    };
    println!("| car | km/h | decel g | peak pitch ° | steady pitch ° | pitch rate °/s | front compr / travel | bump travel at rest (cm) | bump stop squash (cm) | front body clearance rest → min (cm) | collider front corner min (cm) |");
    println!("|---|---|---|---|---|---|---|---|---|---|---|");
    for id in &ids {
        let car = load(id);
        println!(
            "| {id} | front struts on the bump stops: body {:.1} cm above the road |",
            clearance_on_front_bump_stops(&car) * 100.0
        );
        let presets = [
            ("wheel", AssistProfile::wheel_default()),
            ("keyboard", AssistProfile::keyboard_default()),
            ("owner default", owner_default_assists()),
        ];
        for (kmh, rev, (name, a)) in [
            (100.0, false, &presets[0]),
            (60.0, false, &presets[0]),
            (100.0, false, &presets[1]),
            (100.0, true, &presets[1]),
            (100.0, false, &presets[2]),
        ] {
            let r = brake_dive(&car, kmh, rev, a);
            let tag = if rev { " + hold into reverse" } else { "" };
            let tag = format!("{name}{tag}");
            println!(
                "| {id} | {kmh:.0} {tag} | {:.2} | {:.2} | {:.2} | {:.0} | {:.2} | {:.1} | {:.1} | {:.1} → {:.1} | {:.1} |",
                r.decel_g,
                r.peak_pitch_deg,
                r.steady_pitch_deg,
                r.peak_pitch_rate_dps,
                r.front_compression_frac,
                r.front_bump_travel_m * 100.0,
                r.bump_stop_m.max(0.0) * 100.0,
                r.rest_clearance_m * 100.0,
                r.min_clearance_m * 100.0,
                r.min_collider_m * 100.0,
            );
        }
    }
}
