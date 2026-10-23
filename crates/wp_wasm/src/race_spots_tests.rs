//! Native tests of honk-to-start race spots in a live session (`crate::race_spots`).

use wp_contracts::{DamageMode, EntityId, VehicleInput, VehicleSim};
use wp_race::catalog::RaceCity;

use crate::race::{race_city, race_for_scenario, race_scenario_in};
use crate::race_spots::spot_cards;
use crate::scenario::{city_spec, free_drive_in, night_run_in};
use crate::session::Session;

const PLAYER: EntityId = EntityId(1);
const CAR: &str = "lindwurm_4t";

/// A free drive without street racers (a racer in honking range would take the honk first;
/// see [`street_racer_in_reach_takes_the_honk_first`]).
fn free_drive(city: &str) -> Session {
    crate::session::set_street_racers_enabled(false);
    Session::new(
        free_drive_in(CAR, DamageMode::Arcade, city_spec(city)),
        false,
    )
    .unwrap()
}

fn speed(s: &Session) -> f32 {
    let v = s.vehicle(PLAYER).unwrap().vehicle.state().lin_vel;
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

fn pos(s: &Session) -> [f32; 3] {
    s.vehicle(PLAYER).unwrap().vehicle.state().pos
}

fn input(s: &mut Session, horn: bool, brake: f32) {
    s.set_input(
        PLAYER,
        VehicleInput {
            horn,
            brake,
            ..Default::default()
        },
    )
    .unwrap();
}

/// Ticks with `horn` / `brake` held; returns every race honked meanwhile.
fn hold(s: &mut Session, ticks: u64, horn: bool, brake: f32) -> Vec<&'static str> {
    input(s, horn, brake);
    let mut out = Vec::new();
    for _ in 0..ticks {
        s.run_ticks(1);
        out.extend(s.take_race_spot_honked());
    }
    out
}

/// Every Port Vesper race has a spot in a free-drive session on Port Vesper and on the
/// combined Ashpeak map (same place), and the JSON the testbed reads carries the race's name.
#[test]
fn port_vesper_races_have_spots_on_both_maps() {
    let mut vesper = free_drive("vesper_v2");
    crate::session::set_street_racers_enabled(false);
    let mut ashpeak = Session::new(
        night_run_in(CAR, DamageMode::Arcade, city_spec("ashpeak")),
        false,
    )
    .unwrap();
    assert!(vesper.race_spots_active() && ashpeak.race_spots_active());
    let a = spot_cards(vesper.race_spots());
    let b = spot_cards(ashpeak.race_spots());
    for def in wp_race::catalog::races()
        .iter()
        .filter(|r| r.city == RaceCity::Vesper)
    {
        let sa = a
            .iter()
            .find(|c| c.id == def.id)
            .expect("spot on vesper_v2");
        let sb = b.iter().find(|c| c.id == def.id).expect("spot on ashpeak");
        assert_eq!(sa.name, def.name);
        assert_eq!(sa.kind, def.kind.key());
        assert!((sa.x - sb.x).abs() < 1.0 && (sa.y - sb.y).abs() < 1.0);
        assert!(sa.radius > 10.0);
    }
    // The grid city's race is not on Port Vesper.
    assert!(a.iter().all(|c| c.id != "grid_highway_sprint"));
    let json = serde_json::to_string(&a).unwrap();
    assert!(json.contains("\"heading\"") && json.contains("Hollins"));
}

/// Drive into the Hollins sprint's spot, honk while still outside and while moving (nothing),
/// brake to a stop inside, honk: the race is requested once (a held horn does not retrigger)
/// and starts on the same map with the player's car and mode.
#[test]
fn honking_stopped_in_a_spot_starts_its_race_once() {
    for city in ["vesper_v2", "ashpeak"] {
        let mut s = free_drive(city);
        let spot = s
            .race_spots()
            .iter()
            .find(|p| p.race_id == "hollins_sprint")
            .cloned()
            .expect("hollins spot");
        // Roll towards the spot along the race direction from 18 m short of it at 9 m/s.
        let (sn, cs) = wp_math::sin_cos(spot.heading_rad);
        let back = 18.0;
        let xy = [spot.pos[0] + sn * back, spot.pos[1] - cs * back];
        s.debug_sim_mut()
            .unwrap()
            .place_on_ground(PLAYER, xy, spot.heading_rad, 9.0, Some(spot.pos[2]))
            .unwrap();
        // Honk outside the spot: nothing.
        assert!(!spot.contains(pos(&s)));
        assert!(hold(&mut s, 10, true, 0.0).is_empty(), "{city}: outside");
        assert!(hold(&mut s, 10, false, 0.0).is_empty());
        // Roll into the spot, honk while still moving fast: nothing.
        let mut n = 0;
        while !spot.contains(pos(&s)) {
            s.run_ticks(1);
            n += 1;
            assert!(n < 240 * 4, "{city}: never reached the spot");
        }
        assert!(speed(&s) > 3.0, "{city}: {} m/s", speed(&s));
        assert!(hold(&mut s, 10, true, 0.0).is_empty(), "{city}: moving");
        // Brake to a stop (horn released, then a held horn while stopping does not fire).
        assert!(hold(&mut s, 2, false, 1.0).is_empty());
        n = 0;
        while speed(&s) > 0.3 {
            s.run_ticks(1);
            n += 1;
            assert!(n < 240 * 5, "{city}: never stopped");
        }
        assert!(spot.contains(pos(&s)), "{city}: stopped outside the spot");
        assert!(s.take_race_spot_honked().is_none());
        // A fresh press while stopped inside: exactly one request, even with the horn held.
        let hit = hold(&mut s, 240, true, 1.0);
        assert_eq!(hit, vec!["hollins_sprint"], "{city}");
        assert!(hold(&mut s, 30, true, 1.0).is_empty());

        // The testbed starts it like the picker: same map, player's car and mode.
        let current = s.sim().world().network().id.clone();
        assert_eq!(current, city);
        let def = wp_race::catalog::race(hit[0]).unwrap();
        let sc = race_scenario_in(
            &def,
            &s.scenario().entities[0].car,
            s.mode(),
            false,
            race_city(&def, &current),
        )
        .unwrap();
        assert_eq!(sc.city, city_spec(city));
        assert_eq!(race_for_scenario(&sc.name).unwrap().0.id, "hollins_sprint");
        let mut race = Session::new(sc, false).unwrap();
        assert!(race.race().is_some());
        // No spots during a race.
        assert!(!race.race_spots_active());
        assert!(race.race_spots().is_empty());
        input(&mut race, true, 0.0);
        race.run_ticks(5);
        assert!(race.take_race_spot_honked().is_none());
    }
}

/// A race whose own city is the grid starts on the grid from a Port Vesper session; a Port
/// Vesper race started from the grid runs on Port Vesper.
#[test]
fn race_city_keeps_the_map_only_when_the_race_runs_there() {
    let hollins = wp_race::catalog::race("hollins_sprint").unwrap();
    let grid = wp_race::catalog::race("grid_highway_sprint").unwrap();
    assert_eq!(race_city(&hollins, "ashpeak"), "ashpeak");
    assert_eq!(race_city(&hollins, "vesper_v2"), "vesper_v2");
    assert_eq!(race_city(&hollins, "procedural_12x12"), "vesper_v2");
    assert_eq!(race_city(&hollins, ""), "vesper_v2");
    assert_eq!(race_city(&grid, "ashpeak"), "procedural_12x12");
    assert!(race_scenario_in(&grid, CAR, DamageMode::Arcade, false, "vesper_v2").is_err());
}

/// One challenge system on the horn: with the player right behind a roaming street racer in
/// a spot, a honk opens the street challenge and does not request the spot's race; the next
/// honk with the challenge still open does nothing either. Declined, a honk with no racer in
/// reach starts the spot's race.
#[test]
fn street_racer_in_reach_takes_the_honk_first() {
    crate::session::set_street_racers_enabled(true);
    let mut s = Session::new(
        free_drive_in(CAR, DamageMode::Arcade, city_spec("vesper_v2")),
        false,
    )
    .unwrap();
    crate::session::set_street_racers_enabled(false);
    let spot = s
        .race_spots()
        .iter()
        .find(|p| p.race_id == "hollins_sprint")
        .cloned()
        .expect("hollins spot");
    // Player stopped in the spot, right behind a roaming racer parked 9 m ahead (round 12:
    // a challenge only from right behind the racer).
    s.debug_sim_mut()
        .unwrap()
        .place_on_ground(
            PLAYER,
            [spot.pos[0], spot.pos[1]],
            spot.heading_rad,
            0.0,
            Some(spot.pos[2]),
        )
        .unwrap();
    let (sn, cs) = wp_math::sin_cos(spot.heading_rad);
    let racer = s
        .street()
        .expect("street racers")
        .roster()
        .iter()
        .find(|r| r.mode == crate::street::RacerMode::Roam)
        .expect("a roaming racer")
        .entity;
    s.debug_sim_mut()
        .unwrap()
        .place_on_ground(
            EntityId(racer),
            [spot.pos[0] - sn * 9.0, spot.pos[1] + cs * 9.0],
            spot.heading_rad,
            0.0,
            Some(spot.pos[2]),
        )
        .unwrap();
    hold(&mut s, 30, false, 1.0);
    assert!(s.street().unwrap().in_reach(s.sim(), PLAYER).is_some());
    assert!(
        hold(&mut s, 5, true, 1.0).is_empty(),
        "the racer takes the honk"
    );
    assert!(s.street().unwrap().busy(), "street challenge open");
    hold(&mut s, 5, false, 1.0);
    assert!(
        hold(&mut s, 5, true, 1.0).is_empty(),
        "challenge open: no spot"
    );
    // Decline; the racer is on its re-challenge cooldown, so it is not in reach: the spot's
    // race starts.
    s.street_decline();
    hold(&mut s, 5, false, 1.0);
    assert!(s.street().unwrap().in_reach(s.sim(), PLAYER).is_none());
    assert_eq!(hold(&mut s, 5, true, 1.0), vec!["hollins_sprint"]);
}

/// Off-city free drive has no spots and a honk does nothing.
#[test]
fn no_spots_off_city() {
    let mut s = Session::new(
        free_drive_in(CAR, DamageMode::Arcade, simcli::scenario::CitySpec::Flat),
        false,
    )
    .unwrap();
    assert!(!s.race_spots_active());
    assert!(s.race_spots().is_empty());
    assert!(hold(&mut s, 10, true, 1.0).is_empty());
}
