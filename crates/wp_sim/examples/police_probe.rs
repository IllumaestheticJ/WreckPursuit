//! Scratch probe (lead): speeding player in a small city with police cars; prints a timeline.
use wp_contracts::{SimEvent, TimeOfDay, VehicleInput};
use wp_sim::{Sim, SimConfig, VehicleKind};
use wp_world::World;
fn main() {
    let d = |p: &str| format!("{}/../../data/{p}", env!("CARGO_MANIFEST_DIR"));
    let car = wp_contracts::load_car(d("cars/debug_coupe.json")).unwrap();
    let cop = wp_contracts::load_car(d("cars/sentinel_cruiser.json")).unwrap();
    let mode = wp_contracts::load_mode_table(d("modes/sim.toml")).unwrap();
    let mut sim = Sim::new(SimConfig {
        seed: 11,
        mode,
        time_of_day: TimeOfDay::from_hours(22.0),
        time_ratio: 10.0,
    });
    sim.set_police_car(cop);
    sim.set_world(World::procedural(
        &wp_world::procgen::ProcGenConfig::small(),
        wp_world::WorldConfig::default(),
    ));
    let id = sim.spawn_vehicle(&car, [0.0, 0.0, 0.0], 0.0).unwrap();
    sim.set_input(
        id,
        VehicleInput {
            throttle: 1.0,
            ..Default::default()
        },
    )
    .unwrap();
    let (mut viol, mut police_ev, mut contacts) = (0, 0, 0);
    for t in 0..240 * 40 {
        sim.step_fixed();
        for e in sim.drain_events() {
            match e {
                SimEvent::TrafficViolation { .. } => viol += 1,
                SimEvent::Police(_) => police_ev += 1,
                SimEvent::Contact { .. } => contacts += 1,
                _ => {}
            }
        }
        if t % (240 * 5) == 0 {
            let cops = sim
                .vehicles()
                .iter()
                .filter(|v| v.kind == VehicleKind::Police)
                .count();
            let r = sim.police().racer(id);
            println!("t={:>3}s cops={cops} units={} heat={:.2} status={:?} viol={viol} police_ev={police_ev} contacts={contacts}",
                t / 240, sim.police().units().len(), r.map_or(0.0, |r| r.heat), r.map(|r| r.status));
        }
    }
}
