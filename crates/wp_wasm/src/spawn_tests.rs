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

/// The career garage's fixed shop table matches the generated world: same shops in POI order,
/// same district and kind, and each in the spawn city whose start grid is nearest. Every spawn
/// city has a shop (the Repair panel starts on it when opened from the menu).
#[test]
fn world_shops_match_the_generated_world() {
    let world = ashpeak();
    let net = world.network();
    let grids: Vec<(&str, [f32; 3])> = SPAWNS
        .iter()
        .map(|s| (s.id, spawn_grid(net, s.anchor).unwrap().pos))
        .collect();
    let city = |p: [f32; 3]| {
        let d = |g: &[f32; 3]| (g[0] - p[0]).hypot(g[1] - p[1]);
        grids
            .iter()
            .min_by(|a, b| d(&a.1).total_cmp(&d(&b.1)))
            .unwrap()
            .0
    };
    let generated: Vec<_> = crate::garage_visit::sites(&world)
        .iter()
        .map(|g| (g.name.clone(), g.district, g.kind, city(g.pos)))
        .collect();
    let table: Vec<_> = crate::garage_visit::WORLD_SHOPS
        .iter()
        .map(|w| (w.name.to_string(), w.district, w.kind, w.city))
        .collect();
    assert_eq!(table, generated);
    // The tow service prices distances from the table's positions (no world in the garage).
    for (w, g) in crate::garage_visit::WORLD_SHOPS
        .iter()
        .zip(crate::garage_visit::sites(&world))
    {
        let off = (w.pos[0] - g.pos[0]).hypot(w.pos[1] - g.pos[1]);
        assert!(
            off < 1.0,
            "{}: table {:?}, world {:?}",
            w.name,
            w.pos,
            g.pos
        );
    }
    for s in SPAWNS {
        assert!(
            table.iter().any(|t| t.3 == s.id),
            "{} has a repair shop",
            s.id
        );
    }
}

/// The car lots and the player's garages ([`crate::garage_visit::WORLD_LOTS`],
/// [`crate::garage_visit::HOME_GARAGES`]) land on a road near their anchor, in their spawn
/// city, and far enough from each other and from every shop and safehouse that no two rings
/// overlap. Every lot of `lots.toml` and every garage of `career.toml` has a spot.
#[test]
fn drive_to_places_stand_on_roads_apart() {
    use crate::garage_visit::{snap_to_lane, HOME_GARAGES, PLACE_RADIUS_M, WORLD_LOTS};
    let world = ashpeak();
    let net = world.network();
    let grids: Vec<(&str, [f32; 3])> = SPAWNS
        .iter()
        .map(|s| (s.id, spawn_grid(net, s.anchor).unwrap().pos))
        .collect();
    let city = |p: [f32; 3]| {
        let d = |g: &[f32; 3]| (g[0] - p[0]).hypot(g[1] - p[1]);
        grids
            .iter()
            .min_by(|a, b| d(&a.1).total_cmp(&d(&b.1)))
            .unwrap()
            .0
    };
    let mut spots: Vec<(String, [f32; 3])> = Vec::new();
    let anchors = WORLD_LOTS
        .iter()
        .map(|l| (l.lot, l.anchor, l.city))
        .chain(HOME_GARAGES.iter().map(|h| (h.id, h.anchor, h.city)));
    for (id, anchor, home_city) in anchors {
        let (pos, _) = snap_to_lane(&world, anchor).unwrap();
        let off = (pos[0] - anchor[0]).hypot(pos[1] - anchor[1]);
        assert!(off < 70.0, "{id}: nearest lane {off:.0} m from its anchor");
        assert_eq!(city(pos), home_city, "{id} at {pos:?}");
        spots.push((id.to_string(), pos));
    }
    for g in crate::garage_visit::sites(&world) {
        spots.push((g.name.clone(), g.pos));
    }
    for h in crate::garage_visit::safehouse_sites(&world) {
        spots.push((h.name.clone(), h.pos));
    }
    for (i, (a, pa)) in spots.iter().enumerate() {
        for (b, pb) in &spots[i + 1..] {
            let d = (pa[0] - pb[0]).hypot(pa[1] - pb[1]);
            assert!(
                d > 2.0 * PLACE_RADIUS_M + 10.0,
                "{a} and {b} only {d:.0} m apart"
            );
        }
    }
    let econ = crate::garage::with_economy(|e| e.clone());
    for l in &econ.lots.lots {
        assert!(
            WORLD_LOTS.iter().any(|w| w.lot == l.id),
            "{} has a spot",
            l.id
        );
    }
    assert_eq!(econ.progression.home_garages.len(), 3);
    for h in &econ.progression.home_garages {
        assert!(
            crate::garage_visit::home_spot(&h.id).is_some(),
            "{} has a spot",
            h.id
        );
    }
}

/// A career run starts in the chosen garage: on its lane, with its visit open at once (the
/// player starts parked in their garage). Car lots and the unlocked garages are on the map.
#[test]
fn career_run_starts_in_the_chosen_garage() {
    use wp_economy::Condition;
    crate::session::set_street_racers_enabled(false);
    let econ = crate::garage::with_economy(|e| e.clone());
    let mut c = crate::career::Career::new(econ, DamageMode::Arcade);
    c.profile_mut().account.cash = 60_000;
    let uid = c.buy_car(CAR, Condition::Used).unwrap();
    c.set_active(uid).unwrap();
    assert_eq!(c.home_id(), "vesper_lockup");
    assert!(c.set_home("spire_loft").is_err(), "locked below 2000 rep");
    c.profile_mut().account.rep = 2_500;
    c.set_home("spire_loft").unwrap();
    let run = |c: &mut crate::career::Career| {
        let mut sc = with_spawn(
            night_run_in(CAR, DamageMode::Arcade, city_spec("ashpeak")),
            "harnell_crossing",
        );
        sc.entities[0].speed_mps = 0.0;
        let mut s = c.start_run(sc).unwrap();
        s.run_ticks(5);
        s
    };
    let mut s = run(&mut c);
    let world = ashpeak();
    let spot = crate::garage_visit::home_spot("spire_loft").unwrap();
    let (want, _) = crate::garage_visit::snap_to_lane(&world, spot.anchor).unwrap();
    let at = s.vehicle(EntityId(1)).unwrap().vehicle.state().pos;
    assert!(
        (at[0] - want[0]).hypot(at[1] - want[1]) < 5.0,
        "{at:?} vs {want:?}"
    );
    let v = s
        .place()
        .visit
        .expect("the garage's visit opens at the start");
    assert_eq!(v.kind, crate::garage_visit::PlaceKind::Home);
    assert_eq!(v.id.as_deref(), Some("spire_loft"));
    assert_eq!(s.lots().len(), 7);
    let homes: Vec<&str> = s.homes().iter().map(|h| h.id.as_str()).collect();
    assert_eq!(
        homes,
        ["vesper_lockup", "spire_loft"],
        "only unlocked garages"
    );
    let places = s.map_places();
    assert!(places
        .iter()
        .any(|p| p.kind == crate::garage_visit::MapPlaceKind::Lot));
    c.finish_run(&mut s).unwrap();

    // Rep falling back below the gate keeps the garage (peak tier); an unknown one is refused.
    c.profile_mut().account.rep = 100;
    assert_eq!(c.home_id(), "spire_loft");
    assert!(c.set_home("atlantis").is_err());
}
