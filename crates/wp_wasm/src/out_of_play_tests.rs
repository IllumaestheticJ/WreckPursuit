//! Out of play (owner report 2026-10-26: water and the map edge were free escapes from the
//! police): on the combined Ashpeak map a pursued car driven into deep water, the sea or off
//! the map edge is put back on a lane with the pursuit still on (logged as replay ops).

use wp_contracts::{DamageMode, EntityId, VehicleInput, VehicleSim};
use wp_police::PursuitStatus;
use wp_world::network::SEA_LEVEL_M;

use simcli::replay::OpKind;
use simcli::scenario::CitySpec;

use crate::out_of_play::{out_of_play, Reason, RECOVER_AFTER_TICKS};
use crate::session::Session;

/// A night run on the combined map with the player wanted and pursued.
fn chased() -> (Session, EntityId) {
    let sc = crate::scenario::night_run_in("debug_sedan", DamageMode::Arcade, CitySpec::ashpeak());
    let mut s = Session::new(sc, false).unwrap();
    let p = EntityId(1);
    s.set_input(
        p,
        VehicleInput {
            throttle: 0.4,
            ..Default::default()
        },
    )
    .unwrap();
    s.run_ticks(240 * 2);
    let police = s.debug_sim_mut().unwrap().police_mut();
    police.set_heat(p, 40.0);
    assert!(
        police.begin_pursuit(p).is_some(),
        "a unit takes the pursuit"
    );
    s.run_ticks(10);
    (s, p)
}

fn pos_of(s: &Session, p: EntityId) -> [f32; 3] {
    s.sim()
        .vehicles()
        .iter()
        .find(|v| v.id == p)
        .unwrap()
        .vehicle
        .state()
        .pos
}

/// Drop the car at `xy` (on whatever ground is there), let it sit out of play, and check it
/// comes back on a lane, still pursued, with its heat, and that the recovery is logged.
fn recovers_from(xy: [f32; 2], want: Reason) {
    let (mut s, p) = chased();
    let heat0 = s.sim().police().racer(p).unwrap().heat;
    s.record_start();
    s.set_input(p, VehicleInput::default()).unwrap();
    s.debug_place(p, xy, 0.0, 0.0, None).unwrap();
    s.run_ticks(2);
    let at = pos_of(&s, p);
    assert_eq!(
        out_of_play(s.sim().world().map(), at),
        Some(want),
        "placed out of play at {at:?}"
    );
    s.run_ticks(u64::from(RECOVER_AFTER_TICKS) + 240);
    let back = pos_of(&s, p);
    let map = s.sim().world().map();
    assert_eq!(out_of_play(map, back), None, "back in play at {back:?}");
    assert!(
        map.locate(back, None, 1.5, true, true).is_some(),
        "on a lane at {back:?}"
    );
    let rec = s.sim().police().racer(p).unwrap();
    assert!(
        matches!(
            rec.status,
            PursuitStatus::Pursuit | PursuitStatus::Searching
        ),
        "the pursuit carries on ({:?})",
        rec.status
    );
    assert!(
        rec.heat >= heat0 - 1e-3,
        "heat kept ({heat0} → {})",
        rec.heat
    );
    // (The night run's street racers are live-only AI, so its recordings do not verify in
    // `simcli replay`; `debug_place` is not logged either.)
    let rp = s.record_stop().unwrap();
    assert!(rp.ops.iter().any(|o| matches!(o.op, OpKind::Recovered)));
    assert!(rp
        .ops
        .iter()
        .any(|o| matches!(o.op, OpKind::ResetAt { repair: false, .. })));
}

/// A point in deep water: the middle of a waterway, or open sea well inside the map.
fn deep_water(river: bool) -> [f32; 2] {
    let sc = crate::scenario::night_run_in("debug_sedan", DamageMode::Arcade, CitySpec::ashpeak());
    let s = Session::new(sc, false).unwrap();
    let map = s.sim().world().map();
    let deep = |p: [f32; 2]| {
        map.water_surface_at(p)
            .is_some_and(|w| w - map.net.ground_z(p) > 2.0)
            // No bridge deck or road over it.
            && map.locate([p[0], p[1], 0.0], None, 6.0, false, true).is_none()
    };
    if river {
        for w in &map.net.features.waterways {
            for c in &w.centre {
                if deep([c[0], c[1]]) {
                    return [c[0], c[1]];
                }
            }
        }
        panic!("no deep waterway");
    }
    let g = map.ground.as_ref().unwrap();
    for j in (10..g.ny - 10).step_by(7) {
        for i in (10..g.nx - 10).step_by(7) {
            let p = g.vertex_pos(i, j);
            if g.at(i, j) < SEA_LEVEL_M - 2.5 && deep(p) {
                return p;
            }
        }
    }
    panic!("no open sea");
}

#[test]
fn pursued_car_in_a_river_goes_back_on_the_road() {
    recovers_from(deep_water(true), Reason::Water);
}

#[test]
fn pursued_car_in_the_sea_goes_back_on_the_road() {
    recovers_from(deep_water(false), Reason::Water);
}

#[test]
fn pursued_car_off_the_map_goes_back_on_the_road() {
    let sc = crate::scenario::night_run_in("debug_sedan", DamageMode::Arcade, CitySpec::ashpeak());
    let s = Session::new(sc, false).unwrap();
    let g = s.sim().world().map().ground.clone().unwrap();
    let mid_y = g.origin[1] + 0.5 * (g.max_corner()[1] - g.origin[1]);
    recovers_from([g.origin[0] - 30.0, mid_y], Reason::OffMap);
}
