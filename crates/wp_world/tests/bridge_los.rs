//! Bridge decks in line of sight (owner bug "cops are seeing me from under the bridge"), on
//! Port Vesper v2 and Ashpeak County: at every grade separation a car parked under the deck
//! and a car anywhere on the bridge (its ends too) do not see each other, a helicopter right
//! above the hidden car does not see it, and the deck's own sidewalks cover too.

use wp_world::geom::{self, V2, V3};
use wp_world::network::RoadSurface;
use wp_world::surface::{deck_occluders, deck_slabs, DECK_COVER_CLEARANCE_M};
use wp_world::vesper::{self, VesperConfig, VesperReport};
use wp_world::{CityMap, RoadNetwork};

/// Eye height above the road for these sightlines (a driver), m.
const EYE_M: f32 = 1.3;

/// Unit direction of surface `s` at the centreline point nearest `p`, and its right.
fn dir_at(s: &RoadSurface, p: V2) -> (V2, V2) {
    let k = (0..s.centre.len() - 1)
        .min_by(|&a, &b| {
            geom::dist(geom::xy(s.centre[a]), p).total_cmp(&geom::dist(geom::xy(s.centre[b]), p))
        })
        .unwrap();
    let d = geom::normalize(geom::sub(geom::xy(s.centre[k + 1]), geom::xy(s.centre[k])));
    (d, geom::right_of(d))
}

fn at(p: V2, d: V2, m: f32) -> V2 {
    geom::add(p, geom::scale(d, m))
}

fn eye(map: &CityMap, p: V2, near_z: f32) -> V3 {
    [p[0], p[1], map.ground_height(p, Some(near_z)) + EYE_M]
}

/// Every crossing of `rep`: cars across the lower road under the deck against cops on the
/// upper road up to 60 m either side (wherever its road is 1.5 m or more over the terrain),
/// both ways, plus the helicopter straight above. Returns the sightlines checked.
fn check(name: &str, net: RoadNetwork, rep: &VesperReport) -> usize {
    let map = CityMap::new(net);
    let terrain = map.net.terrain.clone().expect("terrain");
    let mut checked = 0;
    for (n, c) in rep.crossings.iter().enumerate() {
        let lo = map.ground_height(c.pos, Some(-1.0e3));
        let hi = map.ground_height(c.pos, Some(1.0e3));
        assert!(
            hi - lo > DECK_COVER_CLEARANCE_M,
            "{name} #{n}: grade separated"
        );
        let (up, up_r) = dir_at(&map.net.surfaces[c.upper], c.pos);
        let (low, low_r) = dir_at(&map.net.surfaces[c.lower], c.pos);
        for car_along in [-6.0, 0.0, 6.0] {
            for car_lane in [-2.0, 2.0] {
                let car = eye(&map, at(at(c.pos, low, car_along), low_r, car_lane), lo);
                // The helicopter right above it, and right above the deck beside it.
                let heli = [car[0], car[1], hi + 120.0];
                assert!(
                    !map.line_of_sight(heli, car),
                    "{name} #{n}: helicopter above"
                );
                assert!(
                    !map.line_of_sight(car, heli),
                    "{name} #{n}: helicopter above"
                );
                for along in [
                    -60.0, -40.0, -25.0, -15.0, -8.0, 0.0, 8.0, 15.0, 25.0, 40.0, 60.0,
                ] {
                    for lane in [-5.0, -2.0, 2.0, 5.0] {
                        let p = at(at(c.pos, up, along), up_r, lane);
                        let cop = eye(&map, p, hi);
                        if cop[2] - EYE_M - terrain.height_at(p) < 1.5 {
                            continue; // off the bridge
                        }
                        let what =
                            format!("{name} #{n}: cop {along}/{lane}, car {car_along}/{car_lane}");
                        assert!(!map.line_of_sight(cop, car), "{what}");
                        assert!(!map.line_of_sight(car, cop), "{what}");
                        checked += 1;
                    }
                }
            }
        }
        // On the deck itself the car is in plain view from the air.
        let on_deck = eye(&map, c.pos, hi);
        assert!(map.line_of_sight([on_deck[0] + 20.0, on_deck[1] + 15.0, hi + 120.0], on_deck));
    }
    checked
}

#[test]
fn port_vesper_bridges_hide_the_road_under_them_from_the_deck_and_the_air() {
    let (net, rep) = vesper::generate_with_report(&VesperConfig::default());
    let n = check("Port Vesper", net, &rep);
    assert!(n > 2000, "{n}");
}

#[test]
fn ashpeak_bridges_hide_the_road_under_them_from_the_deck_and_the_air() {
    let (net, rep) = vesper::ashpeak::generate_with_report(&vesper::ashpeak::default_config());
    assert!(rep.crossings.len() > 14, "Ashpeak adds its own overpasses");
    let n = check("Ashpeak", net, &rep);
    assert!(n > 4000, "{n}");
}

#[test]
fn deck_occluders_cover_every_room_for_a_car_piece_and_the_deck_sidewalks() {
    let (net, _) = vesper::generate_with_report(&VesperConfig::default());
    let slabs = deck_slabs(&net);
    let occ = deck_occluders(&net);
    assert!(occ.len() > slabs.len(), "{} vs {}", occ.len(), slabs.len());
    // Every piece with room for a car under it is an occluder, at least as wide.
    for s in &slabs {
        assert!(
            occ.iter().any(|o| o.a == s.a
                && o.b == s.b
                && o.left_m >= s.left_m
                && o.right_m >= s.right_m),
            "{s:?}"
        );
    }
    // Decks with sidewalks are wider than their paving.
    assert!(occ.iter().any(|o| {
        slabs
            .iter()
            .any(|s| o.a == s.a && o.b == s.b && o.left_m + o.right_m > s.left_m + s.right_m + 1.0)
    }));
    // A road on the ground is never one: every occluder piece has an end off the terrain.
    let t = net.terrain.as_ref().unwrap();
    for o in &occ {
        let off = |p: V3| p[2] - t.height_at(geom::xy(p)) > 0.3;
        assert!(off(o.a) || off(o.b), "{o:?}");
    }
}
