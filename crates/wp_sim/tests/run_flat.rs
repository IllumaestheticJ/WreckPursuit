//! Run-flat tyres (the career's `tires_runflat` mod, `Sim::set_run_flat`) through the full game
//! loop: with all four tyres flat, a car on run-flats never sits on its rims and keeps most of
//! its pace, while the same car on plain tyres rides on its rims and drags. Physics, so the
//! same in both damage modes (CLAUDE.md).

use wp_contracts::{load_car, load_mode_table, AssistProfile, TimeOfDay, VehicleInput};
use wp_sim::{Sim, SimConfig};
use wp_vehicle::vehicle::RUN_FLAT_MAX_DEFLATION;

fn data(path: &str) -> String {
    format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
}

struct Run {
    /// Deflation the handling drives on, per wheel.
    deflation: [f32; 4],
    /// Ticks with any wheel on its rim.
    rim_ticks: u32,
    /// Speed after 6 s of full throttle from rest, m/s.
    speed_mps: f32,
}

/// Four flat tyres (a carried flat, as the career sets it), then 6 s of full throttle.
fn four_flats(mode_file: &str, run_flat: bool) -> Run {
    let car = load_car(data("cars/kestrel_ts.json")).unwrap();
    let mode = load_mode_table(data(mode_file)).unwrap();
    let mut sim = Sim::new(SimConfig {
        seed: 7,
        mode,
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 10.0,
    });
    let id = sim.spawn_vehicle(&car, [0.0; 3], 0.0).unwrap();
    sim.set_assists(id, AssistProfile::wheel_default()).unwrap();
    sim.set_tire_condition(id, [1.0; 4], [1.0; 4]).unwrap();
    sim.set_run_flat(id, run_flat).unwrap();
    for _ in 0..240 {
        sim.step_fixed();
    }
    sim.set_input(
        id,
        VehicleInput {
            throttle: 1.0,
            ..Default::default()
        },
    )
    .unwrap();
    let mut rim_ticks = 0;
    for _ in 0..240 * 6 {
        sim.step_fixed();
        if sim.vehicles()[0].vehicle.rim_contact().iter().any(|r| *r) {
            rim_ticks += 1;
        }
    }
    Run {
        deflation: sim.tire_deflation(id).unwrap(),
        rim_ticks,
        speed_mps: sim.vehicle_states()[0].1.speed_mps,
    }
}

#[test]
fn run_flats_keep_a_flat_car_off_its_rims_in_both_modes() {
    for mode in ["modes/sim.toml", "modes/arcade.toml"] {
        let plain = four_flats(mode, false);
        let rf = four_flats(mode, true);
        println!(
            "{mode}: plain {:.1} m/s, {} rim ticks; run-flat {:.1} m/s, {} rim ticks",
            plain.speed_mps, plain.rim_ticks, rf.speed_mps, rf.rim_ticks
        );
        assert_eq!(plain.deflation, [1.0; 4], "{mode}: plain tyres stay flat");
        assert!(
            plain.rim_ticks > 0,
            "{mode}: plain flats should ride on the rims"
        );
        assert_eq!(
            rf.deflation, [RUN_FLAT_MAX_DEFLATION; 4],
            "{mode}: run-flats cap the deflation"
        );
        assert_eq!(rf.rim_ticks, 0, "{mode}: a run-flat never touches its rim");
        assert!(
            rf.speed_mps > plain.speed_mps * 1.15,
            "{mode}: run-flats {:.1} m/s should clearly outrun flats {:.1} m/s",
            rf.speed_mps,
            plain.speed_mps
        );
    }
}
