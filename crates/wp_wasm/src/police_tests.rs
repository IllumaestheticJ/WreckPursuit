//! Native tests of police in the city-chase preset on Port Vesper v2 (hills): cops see over
//! the terrain, engage a racer they watch break the law, and back out of spots they get
//! pinned in (playtest: "cops spawn into stuck spots, sirens on but no chase").

use wp_contracts::{DamageMode, EntityId, VehicleInput, VehicleSim};

use crate::scenario::city_chase;
use crate::session::Session;

const PLAYER: EntityId = EntityId(1);

/// Night city chase on Port Vesper v2 with the player stopped and no heat.
fn vesper_at_rest() -> Session {
    let mut sc = city_chase("debug_sedan", DamageMode::Arcade);
    sc.entities[0].speed_mps = 0.0;
    Session::new(sc, false).unwrap()
}

/// The player parked just ahead of a patrol car on the hills, then speeding off in front of
/// it: the cop watched it and gives chase (before, its sightline started 1.3 m above z = 0,
/// under the hill, and a first violation stayed below wanted level 1 anyway).
#[test]
fn vesper_cop_chases_a_racer_speeding_in_front_of_it() {
    let mut s = vesper_at_rest();
    let mut parked = false;
    for _ in 0..240 * 5 {
        s.run_ticks(1);
        if s.debug_park_near_police(PLAYER, 15.0).is_ok() {
            parked = true;
            break;
        }
    }
    assert!(parked, "no police car spawned");
    let full = VehicleInput {
        throttle: 1.0,
        ..Default::default()
    };
    let mut pursued = false;
    for _ in 0..12 * 4 {
        s.set_input(PLAYER, full).unwrap();
        s.run_ticks(60);
        if s.sim().police().racer(PLAYER).is_some_and(|r| r.pursued()) {
            pursued = true;
            break;
        }
    }
    let st = crate::police::status(s.sim().police(), PLAYER);
    assert!(pursued, "no pursuit: {st:?}");
}

/// Patrol cars on Port Vesper v2 keep moving: none sits pinned with the throttle down for
/// long (before, several pressed against walls at corners for minutes).
#[test]
fn vesper_patrol_cars_do_not_stay_stuck() {
    let mut s = vesper_at_rest();
    let mut stuck: std::collections::BTreeMap<u32, f32> = Default::default();
    let mut worst = 0.0f32;
    for _ in 0..120 * 4 {
        s.set_input(
            PLAYER,
            VehicleInput {
                brake: 1.0,
                ..Default::default()
            },
        )
        .unwrap();
        s.run_ticks(60);
        for v in s.sim().vehicles() {
            let Some(unit) = v.police_unit else {
                continue;
            };
            let sp = v.vehicle.state().lin_vel;
            let moving = (sp[0] * sp[0] + sp[1] * sp[1]).sqrt() > 0.5;
            // Pushing forward without getting anywhere.
            let t = stuck.entry(unit.0).or_insert(0.0);
            *t = if !moving && v.input.throttle > 0.3 {
                *t + 0.25
            } else {
                0.0
            };
            worst = worst.max(*t);
        }
    }
    assert!(!stuck.is_empty(), "police cars spawned");
    assert!(worst < 10.0, "a police car was stuck for {worst} s");
}
