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

/// The player's car starts a run by the place whose ring is centred on `centre`: on a drivable
/// lane within ~60 m of it, outside its ring, facing along the lane, and stopped there it opens
/// no place panel.
fn assert_starts_by(world: &wp_world::World, s: &Session, centre: [f32; 2]) {
    let st = s.vehicle(EntityId(1)).unwrap().vehicle.state();
    let at = [st.pos[0], st.pos[1]];
    let d = (at[0] - centre[0]).hypot(at[1] - centre[1]);
    assert!(d < 60.0, "{at:?} is {d:.0} m from {centre:?}");
    assert!(
        d > crate::garage_visit::PLACE_RADIUS_M,
        "{at:?} inside the ring at {centre:?} ({d:.1} m)"
    );
    let (seg, sl) = wp_world::routes::nearest_lane(world.map(), [at[0], at[1], 0.0], None).unwrap();
    let (lane, tan) = world.map().seg(seg).point_at(sl);
    let off = (lane[0] - at[0]).hypot(lane[1] - at[1]);
    assert!(off < 3.0, "{at:?} is {off:.1} m off the road");
    // Facing along the road: the chassis' +Y (as `Session::gps_route` reads it) is along the lane.
    let q = st.rot;
    let fwd = [
        2.0 * (q[0] * q[1] - q[3] * q[2]),
        1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]),
    ];
    let along = (fwd[0] * tan[0] + fwd[1] * tan[1]).abs() / fwd[0].hypot(fwd[1]).max(1e-6);
    assert!(along > 0.9, "facing {fwd:?} vs lane {tan:?}");
    for r in crate::garage_visit::all_rings(world) {
        let d = (at[0] - r.0[0]).hypot(at[1] - r.0[1]);
        assert!(d > r.1, "{at:?} inside a ring at {:?}", r.0);
    }
}

/// A free roam of the career in the combined world (Port Vesper's spawn, standing still).
fn career_free_roam(c: &mut crate::career::Career) -> Session {
    let mut sc = with_spawn(
        free_drive_in(CAR, DamageMode::Arcade, city_spec("ashpeak")),
        "port_vesper",
    );
    sc.entities[0].speed_mps = 0.0;
    let mut s = c.start_run(sc).unwrap();
    s.run_ticks(30);
    s
}

/// Owner report 2026-10-29: "when im towed to meridian motorworks and continue free roam
/// there - i dont start there". A tow to a shop puts the car there; the next Free Roam starts
/// by that shop's ring (on the road, outside the ring), and the career screen says so.
#[test]
fn free_roam_starts_at_the_shop_the_car_was_towed_to() {
    use wp_economy::Condition;
    crate::session::set_street_racers_enabled(false);
    let econ = crate::garage::with_economy(|e| e.clone());
    let mut c = crate::career::Career::new(econ, DamageMode::Arcade);
    c.profile_mut().account.cash = 60_000;
    let uid = c.buy_car(CAR, Condition::Used).unwrap();
    c.set_active(uid).unwrap();
    assert_eq!(c.view().start_at.id, "vesper_lockup", "no tow yet: home");
    let shop = "Meridian Motorworks";
    c.tow(uid, "Garage", shop, None).unwrap();
    let v = c.view();
    assert_eq!(
        (v.start_at.kind, v.start_at.name.as_str()),
        ("Garage", shop)
    );
    let mut s = career_free_roam(&mut c);
    let world = ashpeak();
    let site = crate::garage_visit::sites(&world)
        .into_iter()
        .find(|g| g.name == shop)
        .unwrap();
    assert_starts_by(&world, &s, [site.pos[0], site.pos[1]]);
    assert!(s.place().visit.is_none(), "the shop's panel waits");
    // The car drove off: the next run (no tow) starts from home again.
    c.finish_run(&mut s).unwrap();
    assert_eq!(c.view().start_at.id, "vesper_lockup");
}

/// The same for a tow to one of the player's garages that is not the chosen one.
#[test]
fn free_roam_starts_at_the_home_garage_the_car_was_towed_to() {
    use wp_economy::Condition;
    crate::session::set_street_racers_enabled(false);
    let econ = crate::garage::with_economy(|e| e.clone());
    let mut c = crate::career::Career::new(econ, DamageMode::Arcade);
    c.profile_mut().account.cash = 200_000;
    c.profile_mut().account.rep = 5_500;
    let uid = c.buy_car(CAR, Condition::Used).unwrap();
    c.set_active(uid).unwrap();
    c.set_home("vesper_lockup").unwrap();
    c.tow(uid, "Home", "harnell_barn", None).unwrap();
    let v = c.view();
    assert_eq!(
        (v.start_at.kind, v.start_at.id.as_str()),
        ("Home", "harnell_barn")
    );
    let s = career_free_roam(&mut c);
    let world = ashpeak();
    let spot = crate::garage_visit::home_spot("harnell_barn").unwrap();
    let (ring, _) = crate::garage_visit::snap_to_lane(&world, spot.anchor).unwrap();
    assert_starts_by(&world, &s, [ring[0], ring[1]]);
    assert!(s.place().visit.is_none(), "the garage's panel waits");
}

/// Owner report 2026-10-29: "i also can't taxi to dealers once i own a car". A taxi to a lot is
/// open with a drivable car; the car stays where it is (not at the lot's gate, its spot
/// untouched), so the next run still starts from the car.
#[test]
fn a_taxi_to_a_lot_leaves_the_owned_car_where_it_is() {
    use wp_economy::Condition;
    crate::session::set_street_racers_enabled(false);
    let econ = crate::garage::with_economy(|e| e.clone());
    let mut c = crate::career::Career::new(econ, DamageMode::Arcade);
    c.profile_mut().account.cash = 60_000;
    let uid = c.buy_car(CAR, Condition::Used).unwrap();
    c.set_active(uid).unwrap();
    let shop = "Meridian Motorworks";
    c.tow(uid, "Garage", shop, None).unwrap();
    let before = c.car_spot(uid);
    for lot in ["budget_motors", "meridian_prestige", "dockside_salvage"] {
        let (s, visit) = c.taxi_to_lot(lot).unwrap();
        assert_eq!(visit.own_car, None, "{lot}: the car did not come along");
        assert!(!visit.cars.is_empty() || s.entities().len() == 1);
        assert_eq!(c.car_spot(uid), before, "{lot}: the car stays at the shop");
    }
    // Driven in, the car waits at the gate (the old visit).
    let (_, drove) = c.visit_lot("budget_motors").unwrap();
    assert!(drove.own_car.is_some());
    assert_eq!(c.view().start_at.name, shop);
    let s = career_free_roam(&mut c);
    let world = ashpeak();
    let site = crate::garage_visit::sites(&world)
        .into_iter()
        .find(|g| g.name == shop)
        .unwrap();
    assert_starts_by(&world, &s, [site.pos[0], site.pos[1]]);
}

/// Every place a run can start from (each repair shop, each of the player's garages) has a
/// spawn on the road outside every ring, within reach of the place (no fallback into a ring).
#[test]
fn every_start_place_has_a_spawn_outside_the_rings() {
    use crate::garage_visit::{all_rings, run_spawn, StartPlace, HOME_GARAGES, WORLD_SHOPS};
    let world = ashpeak();
    let rings = all_rings(&world);
    let places = WORLD_SHOPS
        .iter()
        .map(StartPlace::Shop)
        .chain(HOME_GARAGES.iter().map(StartPlace::Home));
    for p in places {
        let centre = p.ring_centre(&world).unwrap();
        let (pos, _) = run_spawn(&world, centre, &rings).unwrap_or_else(|| panic!("{p:?}"));
        let d = (pos[0] - centre[0]).hypot(pos[1] - centre[1]);
        assert!(d > 15.0 && d < 60.0, "{p:?}: {d:.0} m");
        assert_eq!(
            crate::garage_visit::start_place_at(p.anchor()),
            Some(p),
            "the anchor finds the place again"
        );
    }
}

/// A career run starts by the chosen garage: on the road just outside its ring. Car lots and
/// the unlocked garages are on the map.
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
    assert_starts_by(&world, &s, [want[0], want[1]]);
    // Owner 2026-10-29: just outside the ring, so the garage's panel does not pop up at once.
    assert!(s.place().visit.is_none(), "{:?}", s.place().visit);
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

/// Landmark buildings (lead, 2026-10-29): `wp_world::places` uses the same anchors and names
/// as the career's lots and garages, and every car lot, home garage, repair shop and safehouse
/// ring of a career session has its building close by (the layout the renderer gets).
#[test]
fn every_drive_to_place_has_its_building_by_its_ring() {
    use crate::garage_visit::{lot_icon, snap_to_lane, HOME_GARAGES, WORLD_LOTS};
    use wp_world::places::{PlaceStyle, DRIVE_TO_SPOTS, MAX_RING_DISTANCE_M};
    let econ = crate::garage::with_economy(|e| e.clone());
    for l in &WORLD_LOTS {
        let s = DRIVE_TO_SPOTS.iter().find(|s| s.id == l.lot).unwrap();
        assert_eq!(s.anchor, l.anchor, "{}", l.lot);
        let lot = econ.lots.lots.iter().find(|x| x.id == l.lot).unwrap();
        assert_eq!(s.name, lot.name, "{}", l.lot);
        let want = match lot.kind {
            wp_economy::LotKind::Dealership => PlaceStyle::Dealership,
            wp_economy::LotKind::UsedLot => PlaceStyle::UsedLot,
            wp_economy::LotKind::Junkyard => PlaceStyle::Junkyard,
        };
        assert_eq!(s.style, want, "{}", l.lot);
        assert_eq!(lot_icon(l.lot), format!("{want:?}"));
    }
    for h in &HOME_GARAGES {
        let s = DRIVE_TO_SPOTS.iter().find(|s| s.id == h.id).unwrap();
        assert_eq!(s.anchor, h.anchor, "{}", h.id);
    }
    assert_eq!(DRIVE_TO_SPOTS.len(), WORLD_LOTS.len() + HOME_GARAGES.len());

    let world = ashpeak();
    let layouts = wp_world::places::plan(world.map());
    let near = |id: &str, ring: [f32; 3]| {
        let l = layouts
            .iter()
            .find(|l| l.id == id)
            .unwrap_or_else(|| panic!("{id} has no building"));
        let d = (l.ring[0] - ring[0]).hypot(l.ring[1] - ring[1]);
        assert!(d < 0.01, "{id}: layout ring {d} m off the session's ring");
        let c = l
            .main
            .iter()
            .fold([0.0, 0.0], |s, q| [s[0] + q[0] / 4.0, s[1] + q[1] / 4.0]);
        let to = (c[0] - ring[0]).hypot(c[1] - ring[1]);
        assert!(
            to < MAX_RING_DISTANCE_M + 20.0,
            "{id}: building {to:.0} m from the ring"
        );
    };
    for s in &DRIVE_TO_SPOTS {
        near(s.id, snap_to_lane(&world, s.anchor).unwrap().0);
    }
    for g in crate::garage_visit::sites(&world) {
        near(&g.name, g.pos);
    }
    for h in crate::garage_visit::safehouse_sites(&world) {
        near(&h.name, h.pos);
    }
}
