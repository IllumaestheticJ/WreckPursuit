//! Native tests of the combined world's spawn choices (`crate::scenario::SPAWNS`).

use wp_contracts::{DamageMode, EntityId, VehicleSim};
use wp_world::ashpeak::plan;

use crate::scenario::{city_spec, free_drive_in, night_run_in, spawn_grid, with_spawn, SPAWNS};
use crate::session::Session;

const CAR: &str = "lindwurm_4t";

/// The combined world's road network (generated once per test).
fn ashpeak() -> wp_world::World {
    let sc = free_drive_in(CAR, DamageMode::Arcade, city_spec("ashpeak"));
    simcli::data::load_world(
        crate::data::mem_data(),
        &sc.city,
        simcli::run::world_config(&sc),
    )
    .unwrap()
    .unwrap()
}

/// Every spawn choice lands on its own start grid in its own city: Port Vesper keeps the old
/// default (the city's first grid, where night runs always began), Ashpeak City starts
/// in the downtown core and Harnell Crossing in the village.
#[test]
fn each_spawn_picks_its_city_grid() {
    let world = ashpeak();
    let net = world.network();
    let grid = |id: &str| {
        let s = SPAWNS.iter().find(|s| s.id == id).unwrap();
        spawn_grid(net, s.anchor).unwrap()
    };
    let name = |id: &str| grid(id).name.clone();
    let first = spawn_grid(net, [0.0; 3]).unwrap();
    assert_eq!(name("port_vesper"), first.name);
    assert!(
        first.pos[0].abs() < plan::VESPER_HI[0] && first.pos[1] < plan::VESPER_HI[1],
        "Port Vesper's grid {first:?} is inside Port Vesper"
    );
    let city = grid("ashpeak_city").pos;
    assert!(
        (plan::METRO_CORE_LO[0]..=plan::METRO_CORE_HI[0]).contains(&city[0])
            && (plan::METRO_CORE_LO[1]..=plan::METRO_CORE_HI[1]).contains(&city[1]),
        "Ashpeak City's spawn {city:?} is in the downtown core"
    );
    assert_eq!(name("harnell_crossing"), plan::POI_HARNELL_CROSSING);
    assert_eq!(SPAWNS[0].id, "port_vesper", "Port Vesper is the default");
}

/// A night run with a spawn choice starts the player on that grid in the live session (and the
/// scenario the session records, so replays start there too); without one it starts where it
/// always did.
#[test]
fn night_run_starts_at_the_chosen_city() {
    crate::session::set_street_racers_enabled(false);
    let world = ashpeak();
    let anchor = SPAWNS
        .iter()
        .find(|s| s.id == "ashpeak_city")
        .unwrap()
        .anchor;
    let downtown = spawn_grid(world.network(), anchor).unwrap().pos;
    let run = |spawn: &str| {
        let sc = with_spawn(
            night_run_in(CAR, DamageMode::Arcade, city_spec("ashpeak")),
            spawn,
        );
        let s = Session::new(sc, false).unwrap();
        (
            s.vehicle(EntityId(1)).unwrap().vehicle.state().pos,
            s.scenario().entities[0].pos,
        )
    };
    let (car, recorded) = run("ashpeak_city");
    assert!(
        (car[0] - downtown[0]).hypot(car[1] - downtown[1]) < 5.0,
        "{car:?} vs {downtown:?}"
    );
    assert_eq!(recorded, downtown);
    let (default, _) = run("");
    let first = spawn_grid(world.network(), [0.0; 3]).unwrap().pos;
    assert!((default[0] - first[0]).hypot(default[1] - first[1]) < 5.0);
    // An unknown id is ignored.
    assert_eq!(run("atlantis").1, first);
}
