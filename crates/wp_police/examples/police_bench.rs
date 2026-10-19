//! `cargo run --release -p wp_police --example police_bench`
//! Measures `PoliceSystem::step` cost per 60 Hz frame (4 physics ticks) with a day-time city
//! (13 patrol units + reinforcements) and 4 wanted racers driving scripted laps, in an active
//! high-heat pursuit. Wall-clock timing lives only in this example, never in sim code.

use std::time::Instant;

use wp_police::scenario::{run_scenario, PoliceScenario, RacerScript};

fn main() {
    let racer = |heat: f32| RacerScript {
        district: wp_police::District::Core,
        speed_factor: 2.0,
        cornering_g: 0.85,
        initial_heat: heat,
        violations: true,
        stay_in_district: false,
        stop_at_s: None,
        safehouse_at_s: None,
        total_loss_at_s: None,
    };
    let sc = PoliceScenario {
        name: "bench".into(),
        seed: 9,
        city: Default::default(),
        mode: wp_contracts::DamageMode::Simulation,
        time_of_day_h: 12.0,
        time_ratio: 0.0,
        duration_s: 120.0,
        runs: 1,
        stop_after_first_pursuit: false,
        racers: vec![racer(80.0), racer(60.0), racer(40.0), racer(0.0)],
        expect: vec![],
    };
    // Baseline: same scenario cost minus police is dominated by police (scripted racers are
    // cheap); report total time per frame as an upper bound for police.
    let t0 = Instant::now();
    let s = run_scenario(&sc);
    let el = t0.elapsed().as_secs_f64();
    let frames = sc.duration_s as f64 * 60.0;
    println!(
        "{} frames, {:.3} ms/frame (police + 4 scripted racers), units engaged {}, tools {}",
        frames,
        el * 1000.0 / frames,
        s.metric("units_engaged").unwrap(),
        s.metric("tools_placed").unwrap()
    );
}
