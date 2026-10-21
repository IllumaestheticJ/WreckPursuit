//! Mixed fleets (`fleets: true`): police units drive more than one model and escalate with
//! the wanted level, traffic spawns cars, vans and heavy trucks with their own lengths, and
//! the run stays deterministic. Built on the golden city chase (night, a known speeder).

use std::collections::BTreeSet;
use std::path::PathBuf;

use simcli::run::run_with_hook;
use simcli::{load_scenario, FsData, RunOptions, Scenario};
use wp_sim::VehicleKind;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The golden city chase with fleets on and the racer already at `heat`.
fn chase(heat: f32, duration_s: f32) -> Scenario {
    let mut sc = load_scenario(&root().join("scenarios/golden/city_chase.ron")).unwrap();
    sc.name = "fleets_city_chase".into();
    sc.fleets = true;
    sc.duration_s = duration_s;
    sc.entities[0].police_heat = heat;
    sc.expect.clear();
    sc
}

#[derive(Default, Debug)]
struct Seen {
    police_cars: BTreeSet<String>,
    traffic_models: BTreeSet<u8>,
    physical_traffic: BTreeSet<String>,
    final_hash: u64,
}

fn observe(sc: &Scenario) -> Seen {
    let data = FsData::new(root().join("../../data"));
    let mut seen = Seen::default();
    let mut hook = |v: &simcli::TickView| {
        for veh in v.sim.vehicles() {
            match veh.kind {
                VehicleKind::Police => {
                    seen.police_cars.insert(veh.car_id.clone());
                }
                VehicleKind::Traffic => {
                    seen.physical_traffic.insert(veh.car_id.clone());
                }
                _ => {}
            }
        }
        for a in v.sim.world().agents() {
            seen.traffic_models.insert(a.model);
        }
        seen.final_hash = v.sim.state_hash();
        Ok(())
    };
    run_with_hook(sc, &data, &RunOptions::default(), &mut hook).unwrap();
    seen
}

#[test]
fn fleets_mix_police_and_traffic_models_deterministically() {
    // Release: 20 s at L4 (reinforcements and roadblocks draw from the heavy end).
    // Debug builds run a shorter slice of the same setup.
    let dur = if cfg!(debug_assertions) { 4.0 } else { 20.0 };
    let sc = chase(75.0, dur);
    let a = observe(&sc);
    println!("{a:#?}");
    assert!(
        a.traffic_models.len() >= 8,
        "traffic should draw many models, saw {:?}",
        a.traffic_models
    );
    assert!(
        a.police_cars.len() >= 2,
        "police should drive more than one model, saw {:?}",
        a.police_cars
    );
    assert!(
        a.police_cars.iter().all(|c| c.starts_with("sentinel_")),
        "{:?}",
        a.police_cars
    );
    let b = observe(&sc);
    assert_eq!(
        a.final_hash, b.final_hash,
        "fleet runs must be deterministic"
    );
}

#[test]
fn fleets_off_keeps_the_single_models() {
    let mut sc = chase(15.0, 3.0);
    sc.fleets = false;
    let a = observe(&sc);
    assert!(
        a.traffic_models.iter().all(|m| *m == 0),
        "{:?}",
        a.traffic_models
    );
    assert!(
        a.police_cars.iter().all(|c| c == simcli::run::POLICE_CAR),
        "{:?}",
        a.police_cars
    );
}
