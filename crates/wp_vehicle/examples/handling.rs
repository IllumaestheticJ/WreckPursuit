//! Driver-facing handling sweep (owner playtest row 28, "all cars understeer too much"): for
//! every roster car and input preset, run the [`wp_vehicle::handling`] tests and print one
//! markdown table per test.
//!
//! `cargo run --release -p wp_vehicle --example handling [--causes] [car_id ...]`
//!
//! `--causes` adds keyboard variants that switch one assist off at a time (lock reduction,
//! ESC, rack filtering), to show where the understeer comes from.

use std::thread;

use wp_contracts::{AssistProfile, LoadedCar};
use wp_vehicle::handling::{city_corner, constant_radius, presets, ramp_steer, slalom, step_steer};
use wp_vehicle::metrics::DriverStyle;

/// One sweep job: car id, preset name, assists, driver style.
type Job = (String, String, AssistProfile, DriverStyle);

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

/// A roster id, or a path to a car JSON (for setup experiments on a copy).
fn load(id: &str) -> LoadedCar {
    let p = if id.ends_with(".json") {
        id.to_string()
    } else {
        format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"))
    };
    wp_contracts::load_car(p).expect("car loads")
}

fn opt(v: Option<f32>, prec: usize) -> String {
    v.map(|x| format!("{x:.prec$}"))
        .unwrap_or_else(|| "—".into())
}

fn row(id: &str, name: &str, a: &AssistProfile, style: DriverStyle) -> String {
    let car = load(id);
    let r80 = ramp_steer(&car, a, 80.0 / 3.6);
    let r120 = ramp_steer(&car, a, 120.0 / 3.6);
    let c40 = constant_radius(&car, a, style, 40.0);
    let c80 = constant_radius(&car, a, style, 80.0);
    let step_in = match style {
        DriverStyle::Keyboard => 1.0,
        DriverStyle::Analog => r80.input_at_04g.unwrap_or(1.0),
    };
    let st = step_steer(&car, a, 80.0 / 3.6, step_in);
    let sl = slalom(&car, a, style);
    let cc = city_corner(&car, a, style);
    format!(
        "| {id} | {name} | {:.2} | {:.2} | {} | {} | {:.2} | {:.2} | {:.2} | {:.2} | {} | {:.2} | {} | {:.2} | {:.0} | {:.0} | {:.2} | {:.2} | {:.2} | {:.2} | {:.2} |",
        r80.understeer_deg_per_g,
        r80.max_lat_g,
        opt(r80.steer_deg_at_06g, 2),
        opt(r80.input_at_06g, 2),
        r80.full_input_g,
        r120.full_input_g,
        c40.max_lat_g,
        c80.max_lat_g,
        opt(c40.steer_deg_at_06g, 2),
        c80.understeer_deg_per_g,
        opt(c80.steer_deg_at_06g, 2),
        st.rise_time_s,
        st.overshoot_pct,
        sl,
        st.steady_lat_g,
        cc.follow_wide_m,
        cc.held_wide_m,
        cc.esc_frac,
        r80.limit_balance,
    )
}

fn main() {
    let mut causes = false;
    let mut ids: Vec<String> = Vec::new();
    for a in std::env::args().skip(1) {
        if a == "--causes" {
            causes = true;
        } else {
            ids.push(a);
        }
    }
    if ids.is_empty() {
        ids = ROSTER.map(String::from).to_vec();
    }
    let mut profiles: Vec<(String, AssistProfile, DriverStyle)> = presets()
        .into_iter()
        .map(|(n, a, s)| (n.to_string(), a, s))
        .collect();
    if causes {
        let kb = AssistProfile::keyboard_default();
        profiles.push((
            "kb no-ESC".into(),
            AssistProfile { esc: false, ..kb },
            DriverStyle::Keyboard,
        ));
        profiles.push((
            "kb no-lock-cut".into(),
            AssistProfile {
                steer_speed_sensitivity: 0.0,
                ..kb
            },
            DriverStyle::Keyboard,
        ));
        profiles.push((
            "kb raw (wheel steer, kb ESC)".into(),
            AssistProfile {
                steer_speed_sensitivity: 0.0,
                countersteer_help: 0.0,
                ..kb
            },
            DriverStyle::Keyboard,
        ));
    }
    println!("| car | preset | US grad 80 km/h deg/g | ramp max g | δ@0.6g deg | input@0.6g | full-input g 80 | full-input g 120 | R40 max g | R80 max g | R40 δ@0.6g | R80 US deg/g | R80 δ@0.6g | step rise s | step overshoot % | slalom km/h | step steady g | city wide m (follow) | city wide m (held) | city ESC frac | limit balance f/r |");
    println!(
        "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|"
    );
    // Fixed-order work split, results printed in roster order.
    let jobs: Vec<Job> = ids
        .iter()
        .flat_map(|id| {
            profiles
                .iter()
                .map(move |(n, a, s)| (id.clone(), n.clone(), *a, *s))
        })
        .collect();
    let threads = thread::available_parallelism().map_or(4, |n| n.get());
    let chunks: Vec<Vec<(usize, Job)>> = (0..threads)
        .map(|t| {
            jobs.iter()
                .cloned()
                .enumerate()
                .filter(|(i, _)| i % threads == t)
                .collect()
        })
        .collect();
    let handles: Vec<_> = chunks
        .into_iter()
        .map(|c| {
            thread::spawn(move || {
                c.into_iter()
                    .map(|(i, (id, n, a, s))| (i, row(&id, &n, &a, s)))
                    .collect::<Vec<_>>()
            })
        })
        .collect();
    let mut rows: Vec<(usize, String)> = handles
        .into_iter()
        .flat_map(|h| h.join().expect("worker"))
        .collect();
    rows.sort_by_key(|r| r.0);
    for (_, r) in rows {
        println!("{r}");
    }
}
