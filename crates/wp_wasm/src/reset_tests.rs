//! Reset in place when no lane is near the car (playtester B1): on the grid city's viaduct over
//! a busy junction, and wedged off-road more than 15 m from any lane. Before the fallbacks the
//! car was reset onto the same spot forever.
use crate::reset::{ResetVia, SNAP_RADIUS_M};
use crate::session::Session;
use wp_contracts::{DamageMode, EntityId};

const PLAYER: EntityId = EntityId(1);

fn grid_free_drive() -> Session {
    let sc = crate::scenario::free_drive_in(
        "debug_sedan",
        DamageMode::Arcade,
        crate::scenario::city_spec("procedural_12x12"),
    );
    let mut s = Session::new(sc, false).unwrap();
    s.run_ticks(10);
    s
}

/// Put the player at `xy` on the ground nearest `near_z`, at rest, and let it settle.
fn place(s: &mut Session, xy: [f32; 2], yaw: f32, near_z: Option<f32>) {
    let z = s.sim().world().map().ground_height(xy, near_z);
    s.debug_sim_mut()
        .unwrap()
        .place_on_ground(PLAYER, xy, yaw, 0.0, Some(z))
        .unwrap();
    s.run_ticks(120);
}

fn player_pos(s: &Session) -> [f32; 3] {
    s.sim().vehicle_states()[0].1.pos
}

/// Distance in XY from `p` to the nearest drivable lane.
fn lane_dist(s: &Session, p: [f32; 2]) -> f32 {
    let map = s.sim().world().map();
    (0..map.n_lanes().min(map.segs.len()))
        .filter(|&i| map.segs[i].drivable && map.segs[i].length > 0.0)
        .map(|i| map.segs[i].project(p).dist)
        .fold(f32::INFINITY, f32::min)
}

/// The point of a coarse 25 m scan of the grid city nearest `near` that is more than 30 m from
/// every lane (the middle of a block).
fn off_road_spot(s: &Session, near: [f32; 3]) -> [f32; 2] {
    let mut best: Option<(f32, [f32; 2])> = None;
    for ix in -40..=40 {
        for iy in -40..=40 {
            let p = [near[0] + ix as f32 * 25.0, near[1] + iy as f32 * 25.0];
            let d = (p[0] - near[0]).hypot(p[1] - near[1]);
            if best.is_some_and(|(b, _)| b <= d) || lane_dist(s, p) <= 30.0 {
                continue;
            }
            best = Some((d, p));
        }
    }
    best.expect("no off-road spot in the grid city").1
}

/// On the grid viaduct where it runs through an interchange (junction connectors only, traffic
/// queued below), the reset finds a deck spot near the car and keeps it on the deck.
#[test]
fn reset_on_the_viaduct_lands_on_a_deck_lane() {
    let mut s = grid_free_drive();
    place(&mut s, [10.75, 100.0], 0.0, Some(8.0));
    let before = player_pos(&s);
    assert!(before[2] > 5.0, "on the deck: {before:?}");
    // Busy junction below: let the traffic queue up.
    s.run_ticks(240 * 5);
    let pose = s.reset_in_place(PLAYER, true).unwrap();
    assert!(pose.on_lane && pose.via == ResetVia::NearLane, "{pose:?}");
    assert!(
        (pose.near_z - before[2]).abs() < 2.0,
        "stays on the deck level: {pose:?} from {before:?}"
    );
    let d = ((pose.xy[0] - before[0]).powi(2) + (pose.xy[1] - before[1]).powi(2)).sqrt();
    assert!(d <= SNAP_RADIUS_M, "moved {d} m");
    s.run_ticks(60);
    assert!(
        player_pos(&s)[2] > 5.0,
        "still on the deck after the reset: {:?}",
        player_pos(&s)
    );
}

/// Wedged far from any lane with no road history: the nearest clear lane anywhere.
#[test]
fn reset_far_off_road_goes_to_the_nearest_lane() {
    let mut s = grid_free_drive();
    let spot = off_road_spot(&s, player_pos(&s));
    place(&mut s, spot, 0.0, None);
    // The free drive starts at rest on its spawn lane: no crumb yet.
    assert!(s.road_crumb().is_none());
    let pose = s.reset_in_place(PLAYER, true).unwrap();
    assert!(
        pose.on_lane && pose.via == ResetVia::NearestLane,
        "{pose:?}"
    );
    let lane = lane_dist(&s, pose.xy);
    assert!(lane < 0.5, "pose is on a lane ({lane} m off)");
    let moved = ((pose.xy[0] - spot[0]).powi(2) + (pose.xy[1] - spot[1]).powi(2)).sqrt();
    assert!(
        moved > SNAP_RADIUS_M,
        "moved {moved} m (farther than the old 15 m limit)"
    );
    // A second reset from there stays on that lane.
    s.run_ticks(30);
    let again = s.reset_in_place(PLAYER, true).unwrap();
    assert!(
        again.on_lane && again.via == ResetVia::NearLane,
        "{again:?}"
    );
}

/// Driving on a road records the last on-road spot; wedged off-road later, the reset goes back
/// to it. The pose is the same in a second identical session (deterministic).
#[test]
fn reset_far_off_road_returns_to_the_last_road_spot() {
    let run = || {
        let mut s = grid_free_drive();
        let start = player_pos(&s);
        let st = s.sim().vehicle_states()[0].1.clone();
        let dir = crate::reset::travel_dir(st.rot, st.lin_vel);
        let yaw = wp_math::atan2(-dir[0], dir[1]);
        // Rolling along the spawn lane records crumbs.
        s.debug_sim_mut()
            .unwrap()
            .place_on_ground(PLAYER, [start[0], start[1]], yaw, 8.0, None)
            .unwrap();
        s.run_ticks(120);
        let crumb = s.road_crumb().expect("a crumb while rolling on the lane");
        let spot = off_road_spot(&s, player_pos(&s));
        place(&mut s, spot, 0.0, None);
        let pose = s.reset_in_place(PLAYER, true).unwrap();
        (crumb, pose)
    };
    let (crumb, pose) = run();
    assert!(pose.on_lane && pose.via == ResetVia::Crumb, "{pose:?}");
    let d = ((pose.xy[0] - crumb.pos[0]).powi(2) + (pose.xy[1] - crumb.pos[1]).powi(2)).sqrt();
    assert!(
        d <= 18.5,
        "at (or a few metres along from) the crumb: {d} m"
    );
    let (crumb2, pose2) = run();
    assert_eq!((crumb, pose), (crumb2, pose2), "deterministic");
}

/// A crumb is only taken while upright and moving on a lane of the car's own level.
#[test]
fn crumbs_need_an_upright_moving_car_on_a_lane() {
    let s = grid_free_drive();
    let map = s.sim().world().map();
    let p = player_pos(&s);
    let on = crate::reset::crumb_at(map, p, [0.0, 1.0], 10.0, 1.0);
    assert!(on.is_some(), "spawn lane at {p:?}");
    assert!(
        crate::reset::crumb_at(map, p, [0.0, 1.0], 0.5, 1.0).is_none(),
        "parked"
    );
    assert!(
        crate::reset::crumb_at(map, p, [0.0, 1.0], 10.0, 0.2).is_none(),
        "on its side"
    );
    assert!(
        crate::reset::crumb_at(map, [p[0], p[1], p[2] + 8.0], [0.0, 1.0], 10.0, 1.0).is_none(),
        "8 m above the lane"
    );
    let off = off_road_spot(&s, p);
    assert!(
        crate::reset::crumb_at(map, [off[0], off[1], p[2]], [0.0, 1.0], 10.0, 1.0).is_none(),
        "off-road"
    );
}

/// "Return to road" (F41, R held): back to the last road the car drove on even when another
/// lane is right next to it and the old road is far away (reset in place would take the near
/// lane); deterministic.
#[test]
fn return_to_road_goes_back_to_the_last_road_spot_at_any_distance() {
    let run = || {
        let mut s = grid_free_drive();
        let start = player_pos(&s);
        let st = s.sim().vehicle_states()[0].1.clone();
        let dir = crate::reset::travel_dir(st.rot, st.lin_vel);
        let yaw = wp_math::atan2(-dir[0], dir[1]);
        s.debug_sim_mut()
            .unwrap()
            .place_on_ground(PLAYER, [start[0], start[1]], yaw, 8.0, None)
            .unwrap();
        s.run_ticks(120);
        let crumb = s.road_crumb().expect("a crumb while rolling on the lane");
        // A lane point 600 m+ from the crumb, at ground level.
        let far = {
            let map = s.sim().world().map();
            (0..map.n_lanes().min(map.segs.len()))
                .filter(|&i| map.segs[i].drivable && map.segs[i].length > 20.0)
                .map(|i| map.segs[i].point_at(map.segs[i].length * 0.5).0)
                .find(|p| {
                    (p[0] - crumb.pos[0]).hypot(p[1] - crumb.pos[1]) > 600.0 && p[2].abs() < 0.5
                })
                .expect("a far lane")
        };
        place(&mut s, [far[0], far[1]], 0.0, Some(0.0));
        let in_place = crate::reset::reset_pose(
            Some(s.sim().world().map()),
            player_pos(&s),
            [0.0, 1.0],
            &[],
            s.road_crumb(),
        );
        let pose = s.reset_to_road(PLAYER, true).unwrap();
        (crumb, in_place, pose)
    };
    let (crumb, in_place, pose) = run();
    assert_eq!(
        in_place.via,
        ResetVia::NearLane,
        "reset in place would stay on the near lane"
    );
    assert_eq!(pose.via, ResetVia::Crumb, "{pose:?}");
    let d = (pose.xy[0] - crumb.pos[0]).hypot(pose.xy[1] - crumb.pos[1]);
    assert!(
        d <= 18.5,
        "at (or a few metres along from) the crumb: {d} m"
    );
    assert_eq!(run().2, pose, "deterministic");
}

/// Without a crumb, return to road is the ordinary reset chain.
#[test]
fn return_to_road_without_a_crumb_is_a_reset_in_place() {
    let mut s = grid_free_drive();
    let spot = off_road_spot(&s, player_pos(&s));
    place(&mut s, spot, 0.0, None);
    assert!(s.road_crumb().is_none());
    let pose = s.reset_to_road(PLAYER, true).unwrap();
    assert!(
        pose.on_lane && pose.via == ResetVia::NearestLane,
        "{pose:?}"
    );
}

fn race_sprint() -> Session {
    let def = wp_race::catalog::race("grid_highway_sprint").unwrap();
    Session::new(
        crate::race::race_scenario(&def, "lindwurm_4t", DamageMode::Arcade, true).unwrap(),
        false,
    )
    .unwrap()
}

/// Restart from the last checkpoint (F42): refused during the countdown; mid-race the player
/// goes back to the last gate it passed, at rest, damage kept, the race clock running on.
/// Deterministic.
#[test]
fn race_restart_from_the_last_checkpoint() {
    let run = || {
        let mut s = race_sprint();
        assert!(
            s.restart_race_checkpoint(PLAYER).is_err(),
            "not during the countdown"
        );
        // Autopilot drives the player through the first gate.
        let mut passed = 0;
        for _ in 0..240 * 60 {
            s.run_ticks(24);
            let st = s.race().unwrap().standings();
            passed = st.iter().find(|x| x.racer == PLAYER).unwrap().gates_passed;
            if passed >= 1 {
                break;
            }
        }
        assert!(passed >= 1, "the player passed a gate");
        // Drive on a little past the gate, then restart from it.
        s.run_ticks(240);
        let health = s.sim().vehicle_states()[0].1.health;
        let t0 = s.race().unwrap().info(s.sim().tick())[2];
        let pose = s.restart_race_checkpoint(PLAYER).unwrap();
        let race = s.race().unwrap();
        let st = race
            .standings()
            .into_iter()
            .find(|x| x.racer == PLAYER)
            .unwrap();
        let course = &race.live().race().course;
        let n = course.gates.len();
        let g = &course.gates[(st.next_gate as usize + n - 1) % n];
        let d = (pose.xy[0] - g.pos[0]).hypot(pose.xy[1] - g.pos[1]);
        s.run_ticks(2);
        let after = s.sim().vehicle_states()[0].1.clone();
        let t1 = s.race().unwrap().info(s.sim().tick())[2];
        (pose, d, health, after.health, t0, t1)
    };
    let (pose, d, h0, h1, t0, t1) = run();
    assert!(
        pose.on_lane && d <= 20.0,
        "back at the last gate ({d:.1} m): {pose:?}"
    );
    assert_eq!(h0, h1, "damage kept (no free repair)");
    assert!(t1 > t0, "the race clock keeps running ({t0} → {t1})");
    assert_eq!(run().0, pose, "deterministic");
}
