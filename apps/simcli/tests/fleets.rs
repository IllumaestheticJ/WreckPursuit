//! Mixed fleets (`fleets: true`): police units drive more than one model and escalate with
//! the wanted level, traffic spawns cars, vans and heavy trucks with their own lengths, and
//! the run stays deterministic. Built on the golden city chase (night, a known speeder).

use std::collections::BTreeSet;
use std::path::PathBuf;

use simcli::run::run_with_hook;
use simcli::{load_scenario, FsData, RunOptions, Scenario};
use wp_contracts::VehicleSim;
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

#[test]
fn highway_semis_tow_their_trailers() {
    let dur = if cfg!(debug_assertions) { 4.0 } else { 20.0 };
    let sc = chase(15.0, dur);
    let data = FsData::new(root().join("../../data"));
    let semi = simcli::run::load_traffic_fleet(&data)
        .unwrap()
        .0
        .iter()
        .position(|m| m.trailer.is_some())
        .expect("a fleet model tows a trailer") as u8;
    let (mut kinematic_rigs, mut hitched) = (0usize, 0usize);
    let mut worst_kink: f32 = 0.0;
    let mut hook = |v: &simcli::TickView| {
        let world = v.sim.world();
        for a in world.agents() {
            if a.model != semi || a.lod != wp_world::Lod::Kinematic {
                assert!(a.trailer.is_none() || a.model == semi);
                continue;
            }
            let t = a.trailer.expect("kinematic semi has a trailer pose");
            kinematic_rigs += 1;
            // Kingpin sits on the fifth wheel: trailer origin 7.2 m behind the fifth wheel,
            // the fifth wheel 2 m behind the tractor's origin.
            let d = ((t.pos[0] - a.pos[0]).powi(2) + (t.pos[1] - a.pos[1]).powi(2)).sqrt();
            assert!((2.0..=9.3).contains(&d), "trailer {d} m from its tractor");
            let mut kink = (t.yaw - a.yaw).abs() % std::f32::consts::TAU;
            kink = kink.min(std::f32::consts::TAU - kink);
            worst_kink = worst_kink.max(kink);
        }
        hitched = hitched.max(v.sim.hitches().len());
        Ok(())
    };
    run_with_hook(&sc, &data, &RunOptions::default(), &mut hook).unwrap();
    println!("rig samples {kinematic_rigs}, hitched {hitched}, worst kink {worst_kink}");
    assert!(
        kinematic_rigs > 0,
        "semis with trailers should be in traffic"
    );
    assert!(
        worst_kink < 1.6,
        "trailers stay behind their tractors ({worst_kink} rad)"
    );
}

/// The repo's data with only the trailer-towing models in the traffic fleet.
struct SemisOnly(FsData);

impl simcli::DataSource for SemisOnly {
    fn read(&self, rel: &str) -> Result<String, simcli::RunError> {
        let text = self.0.read(rel)?;
        if rel != simcli::run::TRAFFIC_FLEET {
            return Ok(text);
        }
        let mut file: wp_world::FleetFile = toml::from_str(&text).unwrap();
        file.model.retain(|m| m.trailer.is_some());
        for m in &mut file.model {
            m.weight_city = 1.0;
        }
        Ok(toml::to_string(&file).unwrap())
    }
}

#[test]
fn promoted_semis_get_a_real_hitched_trailer() {
    let dur = if cfg!(debug_assertions) { 3.0 } else { 10.0 };
    let sc = chase(15.0, dur);
    let data = SemisOnly(FsData::new(root().join("../../data")));
    let mut hitched = 0usize;
    let mut worst: f32 = 0.0;
    let mut hook = |v: &simcli::TickView| {
        for h in v.sim.hitches() {
            let pose = |id| {
                v.sim
                    .vehicles()
                    .iter()
                    .find(|s| s.id == id)
                    .map(|s| s.vehicle.state())
                    .unwrap()
            };
            let (a, b) = (pose(h.tractor), pose(h.trailer));
            let d = ((a.pos[0] - b.pos[0]).powi(2) + (a.pos[1] - b.pos[1]).powi(2)).sqrt();
            worst = worst.max(d);
            assert!(
                (a.pos[2] - b.pos[2]).abs() < 1.5,
                "trailer {:?} vs tractor {:?}",
                b.pos,
                a.pos
            );
        }
        hitched = hitched.max(v.sim.hitches().len());
        Ok(())
    };
    run_with_hook(&sc, &data, &RunOptions::default(), &mut hook).unwrap();
    println!("hitched rigs {hitched}, worst tractor-trailer distance {worst}");
    assert!(hitched > 0, "a promoted semi should tow a physical trailer");
    assert!(worst < 9.5, "trailers stay coupled ({worst} m)");
}
