//! Print what the automatic gearbox does: every shift (time, speed, gears, rpm) on a
//! full-throttle launch and on constant part-throttle runs, plus 0-100 km/h.
//!
//! `cargo run --release -p wp_vehicle --example shift_trace -- [car_id ...]`

use wp_contracts::{AssistProfile, VehicleInput};
use wp_vehicle::metrics::metric_assists;
use wp_vehicle::rig::Rig;

fn car(id: &str) -> wp_contracts::LoadedCar {
    let p = format!("{}/../../data/cars/{id}.json", env!("CARGO_MANIFEST_DIR"));
    wp_contracts::load_car(p).expect("car loads")
}

/// Run `seconds` with a constant throttle; print each gear change.
fn trace(id: &str, throttle: f32, seconds: f32, a: &AssistProfile) {
    let c = car(id);
    let mut rig = Rig::new(&c);
    let input = VehicleInput {
        throttle,
        ..Default::default()
    };
    let mut gear = rig.state().gear;
    let mut shifts = Vec::new();
    let mut t100 = None;
    for _ in 0..(seconds * 240.0) as usize {
        let rpm_before = rig.state().engine_rpm;
        rig.step(&input, a);
        let s = rig.state();
        if s.gear != gear {
            shifts.push(format!(
                "{:.2}s {:.0}km/h {}->{} @{:.0}rpm",
                rig.time(),
                s.speed_mps * 3.6,
                gear,
                s.gear,
                rpm_before
            ));
            gear = s.gear;
        }
        if t100.is_none() && s.speed_mps >= 100.0 / 3.6 {
            t100 = Some(rig.time());
        }
    }
    println!(
        "{id} throttle {throttle:.2}: 0-100 {} | {} shifts: {}",
        t100.map_or("-".into(), |t| format!("{t:.2}s")),
        shifts.len(),
        shifts.join(", ")
    );
}

fn main() {
    let mut ids: Vec<String> = std::env::args().skip(1).collect();
    if ids.is_empty() {
        ids = vec!["debug_sedan".into(), "hakone_gt".into()];
    }
    let a = metric_assists();
    for id in &ids {
        for thr in [1.0, 0.6, 0.35] {
            trace(id, thr, 25.0, &a);
        }
    }
}
