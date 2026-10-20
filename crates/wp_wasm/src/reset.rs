//! Reset in place (testbed R, playtest row 65): where to put a car back on its wheels.
//!
//! The pose is the nearest drivable lane within [`SNAP_RADIUS_M`] of the car, heading along the
//! lane in the car's direction of travel, at the first spot along that lane whose footprint is
//! clear of other vehicles, traffic agents and static props (parked cars, cones). With no clear
//! lane spot in range (open ground, free drive without a city) the car is set upright where it
//! is (or a few metres along its heading if that spot is blocked).
//!
//! Pure and deterministic (fixed iteration order, f32, no hashing), so the session can compute
//! the pose once and log it in the replay op (`simcli` `OpKind::ResetAt`).

use wp_world::geom::{V2, V3};
use wp_world::map::CityMap;

/// Farthest a reset may move the car to reach a lane, m.
pub const SNAP_RADIUS_M: f32 = 15.0;

/// Half footprint of the reset car (x right, y forward), m, including a margin.
const CAR_HALF: [f32; 2] = [1.2, 2.8];

/// Half footprint assumed for other cars and traffic agents, m.
pub const OTHER_HALF: [f32; 2] = [1.0, 2.5];

/// Offsets along the lane tried in order (positive = the car's direction of travel), m.
const LANE_STEPS: [f32; 7] = [0.0, 6.0, -6.0, 12.0, -12.0, 18.0, -18.0];

/// Offsets along the heading tried when there is no lane in range, m.
const OPEN_STEPS: [f32; 5] = [0.0, 4.0, -4.0, 8.0, -8.0];

/// Extra distance charged to a lane that runs against the car's direction of travel, m (the
/// car is still placed heading its own way: one-way streets, or no other lane nearby).
const OPPOSING_PENALTY_M: f32 = 4.0;

/// Something the reset car must not be placed on: a box footprint in the XY plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Obstacle {
    /// Centre, world m.
    pub pos: V3,
    /// Heading, rad about +Z (0 = facing +Y).
    pub yaw: f32,
    /// Half extents (x right, y forward), m.
    pub half: [f32; 2],
}

/// Where to put the car.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResetPose {
    /// Ground position, world m.
    pub xy: [f32; 2],
    /// Heading, rad about +Z (0 = facing +Y).
    pub yaw: f32,
    /// Height hint for `Sim::place_on_ground` (the road nearest this z).
    pub near_z: f32,
    /// Whether the pose is on a lane (false: upright where the car was).
    pub on_lane: bool,
}

/// Heading (yaw, 0 = +Y) of a direction in the XY plane.
fn yaw_of(d: V2) -> f32 {
    wp_math::atan2(-d[0], d[1])
}

/// Unit direction of a yaw.
fn dir_of(yaw: f32) -> V2 {
    let (s, c) = wp_math::sin_cos(yaw);
    [-s, c]
}

fn norm(v: V2) -> Option<V2> {
    let l = (v[0] * v[0] + v[1] * v[1]).sqrt();
    (l > 1e-4).then(|| [v[0] / l, v[1] / l])
}

/// The car's direction of travel in XY: its velocity when moving, else the chassis forward
/// axis (if it is not pointing straight up or down), else +Y.
pub fn travel_dir(rot: [f32; 4], lin_vel: [f32; 3]) -> V2 {
    let v = [lin_vel[0], lin_vel[1]];
    if v[0] * v[0] + v[1] * v[1] > 1.5 * 1.5 {
        if let Some(d) = norm(v) {
            return d;
        }
    }
    // Chassis +Y in world, from the quaternion [x, y, z, w].
    let q = rot;
    let fx = 2.0 * (q[0] * q[1] - q[3] * q[2]);
    let fy = 1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]);
    if fx * fx + fy * fy > 0.3 * 0.3 {
        if let Some(d) = norm([fx, fy]) {
            return d;
        }
    }
    norm(v).unwrap_or([0.0, 1.0])
}

/// Do two oriented boxes in the XY plane overlap? (Separating axis test.)
fn boxes_overlap(
    a_pos: V2,
    a_yaw: f32,
    a_half: [f32; 2],
    b_pos: V2,
    b_yaw: f32,
    b_half: [f32; 2],
) -> bool {
    let a_y = dir_of(a_yaw);
    let a_x = [a_y[1], -a_y[0]];
    let b_y = dir_of(b_yaw);
    let b_x = [b_y[1], -b_y[0]];
    let d = [b_pos[0] - a_pos[0], b_pos[1] - a_pos[1]];
    let dot = |u: V2, v: V2| u[0] * v[0] + u[1] * v[1];
    for axis in [a_x, a_y, b_x, b_y] {
        let ra = a_half[0] * dot(a_x, axis).abs() + a_half[1] * dot(a_y, axis).abs();
        let rb = b_half[0] * dot(b_x, axis).abs() + b_half[1] * dot(b_y, axis).abs();
        if dot(d, axis).abs() > ra + rb {
            return false;
        }
    }
    true
}

/// Is the car's footprint at `xy`/`yaw` clear of every obstacle?
pub fn is_clear(xy: V2, yaw: f32, obstacles: &[Obstacle]) -> bool {
    obstacles
        .iter()
        .all(|o| !boxes_overlap(xy, yaw, CAR_HALF, [o.pos[0], o.pos[1]], o.yaw, o.half))
}

/// Pick the reset pose for a car at `pos` travelling along `dir` (see the module docs).
pub fn reset_pose(map: Option<&CityMap>, pos: V3, dir: V2, obstacles: &[Obstacle]) -> ResetPose {
    let p = [pos[0], pos[1]];
    if let Some(map) = map {
        // Lanes in range, nearest first (opposing lanes charged a penalty); ties by id.
        let mut lanes: Vec<(f32, usize, f32, bool)> = Vec::new();
        for i in 0..map.n_lanes().min(map.segs.len()) {
            let seg = &map.segs[i];
            if !seg.drivable || seg.length <= 0.0 {
                continue;
            }
            let pr = seg.project(p);
            if pr.dist > SNAP_RADIUS_M {
                continue;
            }
            let s = pr.s.clamp(0.0, seg.length);
            let (_, ldir) = seg.point_at(s);
            let agrees = ldir[0] * dir[0] + ldir[1] * dir[1] >= 0.0;
            let key = pr.dist + if agrees { 0.0 } else { OPPOSING_PENALTY_M };
            lanes.push((key, i, s, agrees));
        }
        lanes.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for &(_, i, s, agrees) in &lanes {
            let seg = &map.segs[i];
            // Steps are along the car's direction of travel.
            let sign = if agrees { 1.0 } else { -1.0 };
            for step in LANE_STEPS {
                let sc = s + sign * step;
                if !(0.0..=seg.length).contains(&sc) {
                    continue;
                }
                let (pt, ldir) = seg.point_at(sc);
                let d = [pt[0] - p[0], pt[1] - p[1]];
                if d[0] * d[0] + d[1] * d[1] > SNAP_RADIUS_M * SNAP_RADIUS_M {
                    continue;
                }
                let heading = if agrees { ldir } else { [-ldir[0], -ldir[1]] };
                let yaw = yaw_of(heading);
                if is_clear([pt[0], pt[1]], yaw, obstacles) {
                    return ResetPose {
                        xy: [pt[0], pt[1]],
                        yaw,
                        near_z: pt[2],
                        on_lane: true,
                    };
                }
            }
        }
    }
    // No clear lane spot in range: upright here, or a little along the heading.
    let yaw = yaw_of(dir);
    for step in OPEN_STEPS {
        let xy = [p[0] + dir[0] * step, p[1] + dir[1] * step];
        if is_clear(xy, yaw, obstacles) {
            return ResetPose {
                xy,
                yaw,
                near_z: pos[2],
                on_lane: false,
            };
        }
    }
    ResetPose {
        xy: p,
        yaw,
        near_z: pos[2],
        on_lane: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_basics() {
        assert!(boxes_overlap(
            [0.0, 0.0],
            0.0,
            [1.0, 2.0],
            [0.0, 3.5],
            0.0,
            [1.0, 2.0]
        ));
        assert!(!boxes_overlap(
            [0.0, 0.0],
            0.0,
            [1.0, 2.0],
            [0.0, 4.5],
            0.0,
            [1.0, 2.0]
        ));
        // Rotated 90°: the long side now lies along X.
        assert!(boxes_overlap(
            [0.0, 0.0],
            0.0,
            [1.0, 2.0],
            [2.8, 0.0],
            std::f32::consts::FRAC_PI_2,
            [1.0, 2.0]
        ));
        assert!(!boxes_overlap(
            [0.0, 0.0],
            0.0,
            [1.0, 2.0],
            [3.2, 0.0],
            std::f32::consts::FRAC_PI_2,
            [1.0, 2.0]
        ));
    }

    #[test]
    fn travel_dir_prefers_velocity_then_chassis() {
        // Identity rotation faces +Y; moving along +X wins.
        let d = travel_dir([0.0, 0.0, 0.0, 1.0], [5.0, 0.0, 0.0]);
        assert!((d[0] - 1.0).abs() < 1e-5 && d[1].abs() < 1e-5);
        let d = travel_dir([0.0, 0.0, 0.0, 1.0], [0.1, 0.0, 0.0]);
        assert!(d[0].abs() < 1e-5 && (d[1] - 1.0).abs() < 1e-5);
        // Upside down (180° about Y): forward is still +Y.
        let d = travel_dir([0.0, 1.0, 0.0, 0.0], [0.0; 3]);
        assert!((d[1] - 1.0).abs() < 1e-4, "{d:?}");
    }

    #[test]
    fn open_ground_stays_put_or_steps_clear() {
        let pose = reset_pose(None, [3.0, 4.0, 0.5], [0.0, 1.0], &[]);
        assert_eq!(pose.xy, [3.0, 4.0]);
        assert!(pose.yaw.abs() < 1e-6 && !pose.on_lane);
        // Another car sitting on the spot: step along the heading.
        let blocker = Obstacle {
            pos: [3.0, 4.0, 0.5],
            yaw: 0.0,
            half: OTHER_HALF,
        };
        let pose = reset_pose(None, [3.0, 4.0, 0.5], [0.0, 1.0], &[blocker]);
        assert!(is_clear(pose.xy, pose.yaw, &[blocker]));
        assert!((pose.xy[1] - 4.0).abs() <= 8.0 + 1e-4);
    }
}
