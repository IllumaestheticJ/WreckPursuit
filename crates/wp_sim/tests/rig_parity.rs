//! `wp_vehicle::rig::Rig` (client prediction stepper) must reproduce `wp_sim` bit-for-bit for
//! a lone car on flat ground. Added by workstream 02 for NET-N2.

use wp_contracts::{AssistProfile, TimeOfDay, VehicleInput};
use wp_sim::{Sim, SimConfig};
use wp_vehicle::rig::{FlatWorld, Rig};
use wp_vehicle::VehicleExtras;

#[test]
fn rig_matches_sim_bit_for_bit() {
    let data = format!("{}/../../data", env!("CARGO_MANIFEST_DIR"));
    let car = wp_contracts::load_car(format!("{data}/cars/debug_coupe.json")).unwrap();
    let mode = wp_contracts::load_mode_table(format!("{data}/modes/arcade.toml")).unwrap();
    let mut sim = Sim::new(SimConfig {
        seed: 1,
        mode,
        time_of_day: TimeOfDay::from_hours(12.0),
        time_ratio: 10.0,
    });
    let pos = [3.0, -2.0, 0.0];
    let yaw = 0.4;
    let id = sim.spawn_vehicle(&car, pos, yaw).unwrap();
    let assists = AssistProfile::keyboard_default();
    sim.set_assists(id, assists).unwrap();
    let mut rig = Rig::with_world(
        &car,
        VehicleExtras::default(),
        FlatWorld::default(),
        pos,
        yaw,
    );

    for tick in 0..240 * 8 {
        let t = tick as f32 / 240.0;
        let input = VehicleInput {
            throttle: if t < 5.0 { 1.0 } else { 0.0 },
            brake: if t >= 5.0 { 1.0 } else { 0.0 },
            steer: if (2.0..4.0).contains(&t) { -0.6 } else { 0.1 },
            ..Default::default()
        };
        sim.set_input(id, input).unwrap();
        sim.step_fixed();
        rig.step(&input, &assists);
        let a = &sim.vehicle_states()[0].1;
        let b = rig.state();
        assert_eq!(a.pos, b.pos, "pos diverged at tick {tick}");
        assert_eq!(a.rot, b.rot, "rot diverged at tick {tick}");
        assert_eq!(a.lin_vel, b.lin_vel, "vel diverged at tick {tick}");
        assert_eq!(a.engine_rpm, b.engine_rpm, "rpm diverged at tick {tick}");
    }
}
