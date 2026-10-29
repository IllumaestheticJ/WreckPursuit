//! Police sight across bridge decks on Port Vesper v2 (owner bug "cops are seeing me from
//! under the bridge"): a cop on an overpass does not see a car parked under it, a cop on the
//! road under the deck does not see a car on the bridge, both see each other once the car
//! drives out from under, and a racer hiding under the deck loses its heat.
//!
//! Units here are bound and synced like `wp_sim`'s physical units (real chassis heights); the
//! kinematic-unit test checks that a unit without a body gets the same heights from the roads.

use std::sync::OnceLock;

use wp_contracts::{EntityId, TimeOfDay, PHYSICS_DT};
use wp_world::vesper::{generate_with_report, Crossing, VesperConfig};
use wp_world::{World, WorldConfig};

use crate::events::UnitKind;
use crate::graph::{dist, dot, normalize, sub, xy, PoiKind};
use crate::unit::{SteerGeometry, UnitRole};
use crate::{PoliceConfig, PoliceSetup, PoliceSystem, RacerView, UnitId, UnitSync};

type V3 = [f32; 3];

/// A point counts as on the bridge when its road is at least this above the terrain, m (the
/// bridge's ends too, not only where a car fits under).
const ON_BRIDGE_M: f32 = 1.5;

/// Port Vesper v2 at noon and its grade separations (built once: generation takes a while).
fn vesper() -> &'static (World, Vec<Crossing>) {
    static CITY: OnceLock<(World, Vec<Crossing>)> = OnceLock::new();
    CITY.get_or_init(|| {
        let (net, rep) = generate_with_report(&VesperConfig::default());
        let cfg = WorldConfig {
            start_time: TimeOfDay::from_hours(12.0),
            parked_cars: false,
            ..WorldConfig::default()
        };
        (World::new(net, cfg), rep.crossings)
    })
}

/// Police on Port Vesper with no AI of their own (no patrols, tools or helicopter) and no
/// traffic cameras, so the only eyes are the units a test places.
fn police(world: &World) -> PoliceSystem {
    let mut cfg = PoliceConfig::builtin();
    let none = crate::config::PhaseTable {
        dawn: 0,
        day: 0,
        dusk: 0,
        night: 0,
    };
    cfg.presence.patrols = crate::config::DistrictTable {
        core: none,
        suburb: none,
        industrial: none,
    };
    cfg.presence.unlocks.spikes_level = 99;
    cfg.presence.unlocks.roadblock_level = 99;
    cfg.presence.helicopter.launch_s.day = 1.0e6;
    let mut p = PoliceSystem::for_world(
        cfg,
        world,
        PoliceSetup {
            seed: 11,
            ..Default::default()
        },
    );
    std::sync::Arc::make_mut(&mut p.graph)
        .pois
        .retain(|q| q.kind != PoiKind::Camera);
    p.camera_z = std::sync::Arc::new(Vec::new());
    p
}

/// Points of one crossing (road surface heights): the lower road under the deck, the deck
/// above it, the upper road's direction and its right, and a point out in the open about
/// 80 m along the lower road.
struct Spots {
    under: V3,
    deck: V3,
    upper_dir: [f32; 2],
    upper_right: [f32; 2],
    lower_open: V3,
}

fn spots(world: &World, c: &Crossing) -> Spots {
    let map = world.map();
    let lo = map.ground_height(c.pos, Some(-1.0e3));
    let hi = map.ground_height(c.pos, Some(1.0e3));
    let s = &map.net.surfaces[c.upper];
    let k = (0..s.centre.len() - 1)
        .min_by(|&a, &b| dist(xy(s.centre[a]), c.pos).total_cmp(&dist(xy(s.centre[b]), c.pos)))
        .unwrap();
    let upper_dir = normalize(sub(xy(s.centre[k + 1]), xy(s.centre[k])));
    let open = map.net.surfaces[c.lower]
        .centre
        .iter()
        .copied()
        .find(|q| (dist(xy(*q), c.pos) - 80.0).abs() < 8.0)
        .expect("lower road runs 80 m past the crossing");
    Spots {
        under: [c.pos[0], c.pos[1], lo],
        deck: [c.pos[0], c.pos[1], hi],
        upper_dir,
        upper_right: [upper_dir[1], -upper_dir[0]],
        lower_open: [open[0], open[1], map.ground_height(xy(open), Some(open[2]))],
    }
}

/// The road surface at `p` nearest the height `near_z`.
fn on_road(world: &World, p: [f32; 2], near_z: f32) -> V3 {
    [p[0], p[1], world.map().ground_height(p, Some(near_z))]
}

/// `p` moved `m` metres along `d` (XY).
fn offset(p: [f32; 2], d: [f32; 2], m: f32) -> [f32; 2] {
    [p[0] + d[0] * m, p[1] + d[1] * m]
}

/// `along` m along the upper road and `lane` m to its right of the crossing, on the bridge.
fn on_bridge(world: &World, s: &Spots, along: f32, lane: f32) -> Option<V3> {
    let terrain = world
        .map()
        .net
        .terrain
        .as_ref()
        .expect("Port Vesper has terrain");
    let p = offset(offset(xy(s.deck), s.upper_dir, along), s.upper_right, lane);
    let q = on_road(world, p, s.deck[2]);
    (q[2] - terrain.height_at(p) >= ON_BRIDGE_M).then_some(q)
}

/// The deck's lane on the side of the lower road's open stretch (5 m off the centreline).
fn edge_lane(world: &World, c: &Crossing, s: &Spots) -> V3 {
    let side = if dot(sub(xy(s.lower_open), c.pos), s.upper_right) > 0.0 {
        5.0
    } else {
        -5.0
    };
    on_road(world, offset(xy(s.deck), s.upper_right, side), s.deck[2])
}

/// A bound, synced ground unit at road point `at`, facing `look_at` (as `wp_sim` keeps it).
fn place_unit(p: &mut PoliceSystem, at: V3, look_at: [f32; 2]) -> UnitId {
    let heading = normalize(sub(look_at, xy(at)));
    let i = p.spawn_unit(UnitKind::Car, UnitRole::Reinforcement, xy(at), heading);
    let id = p.units[i].id;
    p.bind_unit_entity(id, EntityId(900 + id.0), SteerGeometry::default());
    sync(p, id, at, heading);
    id
}

/// Report unit `id` parked at road point `at`, facing along `heading`.
fn sync(p: &mut PoliceSystem, id: UnitId, at: V3, heading: [f32; 2]) {
    p.sync_unit(
        id,
        UnitSync {
            pos: at,
            vel: [0.0; 3],
            heading,
            health: 1.0,
            effects: wp_contracts::HandlingEffects::NEUTRAL,
        },
    );
}

const RACER: EntityId = EntityId(3);

/// Step `secs` with the racer parked at road point `at`; whether any unit saw it at the end.
fn run(p: &mut PoliceSystem, world: &World, at: V3, secs: f32) -> bool {
    let view = RacerView {
        entity: RACER,
        pos: at,
        vel: [0.0; 3],
    };
    let mut out = Vec::new();
    for _ in 0..(secs / PHYSICS_DT).round() as usize {
        p.step(world, &[view], &[], PHYSICS_DT, &mut out);
    }
    p.obs.get(&RACER).is_some_and(|o| o.seen)
}

/// The world's sightline between the eyes of two cars parked at road points `a` and `b`.
fn eyes_see(world: &World, a: V3, b: V3) -> bool {
    let eye = |q: V3| [q[0], q[1], crate::eye_height_at(q[2])];
    wp_contracts::WorldQuery::line_of_sight(world, eye(a), eye(b))
}

#[test]
fn deck_blocks_sight_both_ways_at_every_port_vesper_crossing() {
    let (world, crossings) = vesper();
    assert!(crossings.len() >= 2);
    let mut checked = 0;
    for (n, c) in crossings.iter().enumerate() {
        let s = spots(world, c);
        assert!(
            s.deck[2] - s.under[2] > 4.0,
            "#{n}: a real grade separation"
        );
        let lower_dir = normalize(sub(xy(s.lower_open), c.pos));
        let lower_right = [lower_dir[1], -lower_dir[0]];
        // Cops anywhere on the bridge, cars across the road under the deck.
        for along in [
            -40.0, -30.0, -20.0, -12.0, -5.0, 0.0, 5.0, 12.0, 20.0, 30.0, 40.0,
        ] {
            for lane in [-5.0, -2.0, 2.0, 5.0] {
                let Some(cop) = on_bridge(world, &s, along, lane) else {
                    continue;
                };
                for car_along in [-6.0, 0.0, 6.0] {
                    for car_lane in [-2.0, 2.0] {
                        let p = offset(
                            offset(xy(s.under), lower_dir, car_along),
                            lower_right,
                            car_lane,
                        );
                        let car = on_road(world, p, s.under[2]);
                        // Cop on the bridge, car under it; and the other way round.
                        let at = format!("#{n} cop {along}/{lane} car {car_along}/{car_lane}");
                        assert!(!eyes_see(world, cop, car), "{at}");
                        assert!(!eyes_see(world, car, cop), "{at}");
                        checked += 1;
                    }
                }
            }
        }
        // Out from under, at the city's first overpass (both carriageways): the car 80 m
        // along the lower road, the cop on the deck in the lane on that side, and the cop
        // under the deck all see each other. (Not at every crossing: from some decks the
        // deck's own sidewalk, the twin carriageway's deck or the cutting is in the way.)
        if n < 2 {
            let edge = edge_lane(world, c, &s);
            assert!(
                eyes_see(world, edge, s.lower_open),
                "#{n}: deck to open road"
            );
            assert!(
                eyes_see(world, s.lower_open, edge),
                "#{n}: open road to deck"
            );
            assert!(
                eyes_see(world, s.under, s.lower_open),
                "#{n}: under to open road"
            );
            assert!(
                eyes_see(world, s.lower_open, s.under),
                "#{n}: open road to under"
            );
        }
    }
    assert!(checked > 1000, "{checked} sightlines checked");
}

/// Points on the bridge 30 m before, 6 m and 30 m past the crossing, in both outer lanes.
fn bridge_points(world: &World, s: &Spots) -> Vec<V3> {
    let mut out = Vec::new();
    for along in [-30.0, 6.0, 30.0] {
        for lane in [-5.0, 5.0] {
            out.extend(on_bridge(world, s, along, lane));
        }
    }
    out
}

#[test]
fn cop_on_the_bridge_does_not_see_a_car_parked_under_it() {
    let (world, crossings) = vesper();
    for c in crossings.iter().take(2) {
        let s = spots(world, c);
        let cops = bridge_points(world, &s);
        assert!(cops.len() >= 4, "{} points on the bridge", cops.len());
        for cop in cops {
            let mut p = police(world);
            // On the bridge, looking at the spot under the deck.
            let id = place_unit(&mut p, cop, c.pos);
            assert!(
                !run(&mut p, world, s.under, 0.25),
                "seen under the deck from {cop:?}"
            );
            assert!(p.racer(RACER).unwrap().last_seen.is_none());
            // The car drives out from under, 80 m along the lower road; the cop turns to it
            // from the lane on that side and sees it.
            let edge = edge_lane(world, c, &s);
            sync(&mut p, id, edge, normalize(sub(xy(s.lower_open), xy(edge))));
            assert!(
                run(&mut p, world, s.lower_open, 0.25),
                "out in the open at {:?}",
                c.pos
            );
        }
    }
}

#[test]
fn cop_under_the_deck_does_not_see_a_car_on_the_bridge() {
    let (world, crossings) = vesper();
    for c in crossings.iter().take(2) {
        let s = spots(world, c);
        for car in bridge_points(world, &s) {
            let mut p = police(world);
            // Under the deck, looking at the car on the bridge.
            let id = place_unit(&mut p, s.under, xy(car));
            assert!(
                !run(&mut p, world, car, 0.25),
                "seen on the bridge at {car:?}"
            );
            // The car comes down to the lower road and out from under; the cop looks its way.
            sync(&mut p, id, s.under, normalize(sub(xy(s.lower_open), c.pos)));
            assert!(
                run(&mut p, world, s.lower_open, 0.25),
                "out in the open at {:?}",
                c.pos
            );
        }
    }
}

#[test]
fn kinematic_units_take_their_height_from_the_road_they_drive() {
    let (world, crossings) = vesper();
    let c = &crossings[0];
    let s = spots(world, c);
    let p = police(world);
    // Along the upper road at the crossing: the deck; along the lower road: under it.
    let lower_dir = normalize(sub(xy(s.lower_open), c.pos));
    let on_deck = p.kinematic_alt(c.pos, s.upper_dir, 0.0).unwrap();
    let under = p.kinematic_alt(c.pos, lower_dir, 0.0).unwrap();
    assert!(
        (on_deck - s.deck[2]).abs() < 0.3,
        "{on_deck} vs {:?}",
        s.deck
    );
    assert!((under - s.under[2]).abs() < 0.3, "{under} vs {:?}", s.under);
    // A unit spawned on the deck starts there (no body yet), not at z = 0.
    let mut p = police(world);
    let i = p.spawn_unit(UnitKind::Car, UnitRole::Reinforcement, c.pos, s.upper_dir);
    assert!((p.units[i].body.alt_m - s.deck[2]).abs() < 0.3);
}

#[test]
fn hiding_under_the_deck_lets_heat_decay() {
    let (world, crossings) = vesper();
    let c = &crossings[1];
    let s = spots(world, c);
    let start_heat = 30.0;
    // Wanted (not pursued), parked under the deck with a cop parked on the bridge 30 m back,
    // looking at it (where the ends of the deck used to let the sightline through).
    let mut p = police(world);
    let cop = on_bridge(world, &s, -30.0, -5.0).expect("on the bridge");
    place_unit(&mut p, cop, c.pos);
    run(&mut p, world, s.under, 0.1);
    p.set_heat(RACER, start_heat);
    assert!(!run(&mut p, world, s.under, 20.0));
    let r = p.racer(RACER).unwrap();
    assert!(!r.pursued(), "never recognised");
    assert!(r.heat < start_heat - 1.0, "heat {start_heat} -> {}", r.heat);
    // Control: the same wait out in the open, in the cop's view from the deck's edge lane:
    // seen the whole time, so no decay (or a pursuit).
    let mut p = police(world);
    let edge = edge_lane(world, c, &s);
    place_unit(&mut p, edge, xy(s.lower_open));
    run(&mut p, world, s.lower_open, 0.1);
    p.set_heat(RACER, start_heat);
    assert!(run(&mut p, world, s.lower_open, 20.0));
    let r = p.racer(RACER).unwrap();
    assert!(
        r.pursued() || r.heat >= start_heat,
        "heat {start_heat} -> {}",
        r.heat
    );
}

/// The procedural 7×7 grid the city race scenarios run on (its highway is a divided viaduct,
/// deck at ~7.9 m), with no parked cars.
fn grid() -> &'static World {
    static CITY: OnceLock<World> = OnceLock::new();
    CITY.get_or_init(|| {
        let net = wp_world::procgen::generate(&wp_world::procgen::ProcGenConfig {
            nx: 7,
            ny: 7,
            ..Default::default()
        });
        let cfg = WorldConfig {
            start_time: TimeOfDay::from_hours(13.0),
            parked_cars: false,
            ..WorldConfig::default()
        };
        World::new(net, cfg)
    })
}

#[test]
fn a_car_on_the_grid_viaduct_shows_over_its_deck_edge_and_hides_under_it() {
    let world = grid();
    let deck = |p: [f32; 2]| on_road(world, p, 100.0);
    let ground = |p: [f32; 2]| on_road(world, p, -100.0);
    // The northbound carriageway (centre x = 62) is on the deck here, the row under it at grade.
    let row_y = -110.0;
    assert!(deck([62.0, row_y])[2] > 7.0);
    assert!(
        ground([55.0, row_y])[2].abs() < 0.5,
        "a street under the viaduct"
    );
    // Cars in the northbound carriageway (where race_city_sprint_police's racers drive on after
    // the finish) against ground cops east of the viaduct along the row under it and in the
    // streets to the north-east and far north-west (sightlines from that scenario's run). The
    // car's roof shows over the deck edge. With the eye 0.3 m over the deck (the old
    // `eye_height_at` dead band) its own deck hid it from every one of these.
    let seen = [
        ([120.0, row_y], [66.0, row_y]),
        ([160.0, row_y], [65.7, row_y - 20.0]),
        ([98.9, 111.8], [66.6, -25.7]),
        ([104.2, 111.8], [67.0, -32.6]),
        ([129.0, 111.8], [62.8, -86.3]),
        ([-160.4, 157.9], [69.0, -62.0]),
        ([-113.1, 110.6], [65.0, -3.5]),
    ];
    for (cop, car) in seen {
        let (cop, car) = (ground(cop), deck(car));
        assert!(eyes_see(world, cop, car), "cop {cop:?} car {car:?}");
        assert!(eyes_see(world, car, cop), "car {car:?} cop {cop:?}");
    }
    // A car on the row right under the deck: hidden from cops anywhere on the viaduct around
    // it, both carriageways, and it does not see them.
    for car_x in [50.0, 55.0, 60.0] {
        let car = ground([car_x, row_y]);
        for along in [-40.0, -25.0, -12.0, 0.0, 12.0, 25.0, 40.0] {
            for lane_x in [44.3, 48.0, 51.7, 58.3, 62.0, 65.7] {
                let cop = deck([lane_x, row_y + along]);
                assert!(cop[2] > 7.0, "on the viaduct: {cop:?}");
                assert!(!eyes_see(world, cop, car), "cop {cop:?} car {car:?}");
                assert!(!eyes_see(world, car, cop), "car {car:?} cop {cop:?}");
            }
        }
    }
}
