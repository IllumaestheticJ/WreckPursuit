//! Reset in place (testbed R, playtest row 65): where to put a car back on its wheels.
//!
//! The pose is, in order:
//! 1. the nearest drivable lane or junction connector within [`SNAP_RADIUS_M`] of the car and
//!    on its level (within [`LEVEL_DZ_M`] vertically, so a car on a viaduct is not "near" the
//!    street below), heading along it in the car's direction of travel, at the first spot whose
//!    footprint is clear of other vehicles, traffic agents and static props (parked cars,
//!    cones) on that level;
//! 2. else the car's last on-road spot ([`Crumb`], recorded by the session while it drove on a
//!    lane) if it is within [`CRUMB_MAX_M`] and clear;
//! 3. else the nearest clear lane spot anywhere on the map (same level preferred);
//! 4. else (open ground, free drive without a city) upright where the car is, or a few metres
//!    along its heading if that spot is blocked.
//!
//! Before 1's connectors and level check and 2–3 existed, a car wedged more than 15 m off the
//! road, or on the grid city's viaduct where it runs through an interchange (connectors only),
//! was reset onto the same spot forever (playtester B1).
//!
//! Pure and deterministic (fixed iteration order, f32, no hashing), so the session can compute
//! the pose once and log it in the replay op (`simcli` `OpKind::ResetAt`).

use wp_world::geom::{V2, V3};
use wp_world::map::CityMap;

/// Ghost window after a reset in place, physics ticks (1 s): the car passes through other
/// cars, props and debris (`Sim::set_ghost`; extended while it still overlaps a car).
pub const GHOST_TICKS: u16 = 240;

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

/// A lane counts as on the car's level within this height difference, m (decks and viaducts
/// are 6 m+ above the streets they cross; hill roads change height slowly).
pub const LEVEL_DZ_M: f32 = 4.0;

/// An obstacle only blocks a spot within this height difference, m (traffic under a viaduct
/// does not block its deck).
const OBSTACLE_DZ_M: f32 = 3.0;

/// Farthest the last on-road spot may be for a reset to go back to it, m.
pub const CRUMB_MAX_M: f32 = 400.0;

/// Extra distance charged to a junction connector in the near search, m (lanes first; but a
/// viaduct crossing an interchange is all connectors for 50 m+).
const CONNECTOR_PENALTY_M: f32 = 3.0;

/// Nearest lanes tried by the map-wide fallback (bounded work when everything near is busy).
const FALLBACK_LANES: usize = 24;

/// Height difference counts this many metres of horizontal distance when ranking lanes for the
/// map-wide fallback (same level first).
const LEVEL_WEIGHT: f32 = 3.0;

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
    /// Which rule picked the pose.
    pub via: ResetVia,
}

/// Which rule of [`reset_pose`] picked the pose (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetVia {
    /// A lane within [`SNAP_RADIUS_M`].
    NearLane,
    /// The car's last on-road spot.
    Crumb,
    /// The nearest clear lane spot on the map.
    NearestLane,
    /// No lane: upright in place (or a few metres along the heading).
    InPlace,
}

impl ResetVia {
    /// Stable lower-case name (the testbed's reset note).
    pub fn name(self) -> &'static str {
        match self {
            ResetVia::NearLane => "lane",
            ResetVia::Crumb => "last_road",
            ResetVia::NearestLane => "nearest_lane",
            ResetVia::InPlace => "in_place",
        }
    }
}

/// The last spot where the car was driving on a lane (recorded by the session; see
/// [`crumb_at`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crumb {
    /// Lane centre point under the car, world m.
    pub pos: V3,
    /// Direction of travel (unit, XY).
    pub dir: V2,
}

/// The road spot under a car at `pos` travelling along `dir` at `speed_mps`, upright when
/// `up_z` (world z of the chassis up axis) is near 1: the lane centre at the car, if the car is
/// on a drivable lane (not a junction connector) on its own level and moving. None otherwise.
pub fn crumb_at(map: &CityMap, pos: V3, dir: V2, speed_mps: f32, up_z: f32) -> Option<Crumb> {
    if speed_mps.abs() < 2.0 || up_z < 0.9 {
        return None;
    }
    let loc = map.locate(pos, None, 0.5, true, false)?;
    let (pt, _) = map.segs[loc.seg as usize].point_at(loc.s);
    ((pt[2] - pos[2]).abs() <= 1.5).then_some(Crumb { pos: pt, dir })
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

/// Is the car's footprint at `xy`/`yaw` clear of every obstacle (at any height)?
pub fn is_clear(xy: V2, yaw: f32, obstacles: &[Obstacle]) -> bool {
    obstacles
        .iter()
        .all(|o| !boxes_overlap(xy, yaw, CAR_HALF, [o.pos[0], o.pos[1]], o.yaw, o.half))
}

/// Is the car's footprint at `xy`/`yaw`, on a road at height `z`, clear of every obstacle on
/// that level (obstacles more than [`OBSTACLE_DZ_M`] above or below do not count)?
pub fn is_clear_at(xy: V2, z: f32, yaw: f32, obstacles: &[Obstacle]) -> bool {
    obstacles.iter().all(|o| {
        (o.pos[2] - z).abs() > OBSTACLE_DZ_M
            || !boxes_overlap(xy, yaw, CAR_HALF, [o.pos[0], o.pos[1]], o.yaw, o.half)
    })
}

/// First clear spot along lane `i` from arc length `s` (steps along the travel direction:
/// `agrees` = the lane runs the car's way), within `max_move` of `p` (None = any distance).
fn lane_spot(
    map: &CityMap,
    i: usize,
    s: f32,
    agrees: bool,
    p: V2,
    max_move: Option<f32>,
    obstacles: &[Obstacle],
) -> Option<(V3, f32)> {
    let seg = &map.segs[i];
    let sign = if agrees { 1.0 } else { -1.0 };
    for step in LANE_STEPS {
        let sc = s + sign * step;
        if !(0.0..=seg.length).contains(&sc) {
            continue;
        }
        let (pt, ldir) = seg.point_at(sc);
        if let Some(m) = max_move {
            let d = [pt[0] - p[0], pt[1] - p[1]];
            if d[0] * d[0] + d[1] * d[1] > m * m {
                continue;
            }
        }
        let heading = if agrees { ldir } else { [-ldir[0], -ldir[1]] };
        let yaw = yaw_of(heading);
        if is_clear_at([pt[0], pt[1]], pt[2], yaw, obstacles) {
            return Some((pt, yaw));
        }
    }
    None
}

/// Pick the reset pose for a car at `pos` travelling along `dir` (see the module docs).
/// `crumb` = the car's last on-road spot, if the session recorded one.
pub fn reset_pose(
    map: Option<&CityMap>,
    pos: V3,
    dir: V2,
    obstacles: &[Obstacle],
    crumb: Option<Crumb>,
) -> ResetPose {
    let p = [pos[0], pos[1]];
    if let Some(map) = map {
        let n = map.n_lanes().min(map.segs.len());
        // 1. Lanes (and junction connectors) in range on the car's level, nearest first
        //    (opposing lanes and connectors charged a penalty); ties by id.
        let mut lanes: Vec<(f32, usize, f32, bool)> = Vec::new();
        for i in 0..map.segs.len() {
            let seg = &map.segs[i];
            if !seg.drivable || seg.length <= 0.0 {
                continue;
            }
            let pr = seg.project(p);
            if pr.dist > SNAP_RADIUS_M {
                continue;
            }
            let s = pr.s.clamp(0.0, seg.length);
            let (pt, ldir) = seg.point_at(s);
            if (pt[2] - pos[2]).abs() > LEVEL_DZ_M {
                continue;
            }
            let agrees = ldir[0] * dir[0] + ldir[1] * dir[1] >= 0.0;
            let key = pr.dist
                + if agrees { 0.0 } else { OPPOSING_PENALTY_M }
                + if i >= n || seg.is_connector() {
                    CONNECTOR_PENALTY_M
                } else {
                    0.0
                };
            lanes.push((key, i, s, agrees));
        }
        lanes.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for &(_, i, s, agrees) in &lanes {
            if let Some((pt, yaw)) = lane_spot(map, i, s, agrees, p, Some(SNAP_RADIUS_M), obstacles)
            {
                return ResetPose {
                    xy: [pt[0], pt[1]],
                    yaw,
                    near_z: pt[2],
                    on_lane: true,
                    via: ResetVia::NearLane,
                };
            }
        }
        // 2. The last on-road spot, heading the way the car was going there (or a few metres
        //    along it if the spot itself is taken).
        if let Some(c) = crumb {
            let d = [c.pos[0] - p[0], c.pos[1] - p[1]];
            if d[0] * d[0] + d[1] * d[1] <= CRUMB_MAX_M * CRUMB_MAX_M {
                let yaw = yaw_of(c.dir);
                for step in LANE_STEPS {
                    let xy = [c.pos[0] + c.dir[0] * step, c.pos[1] + c.dir[1] * step];
                    if is_clear_at(xy, c.pos[2], yaw, obstacles) {
                        return ResetPose {
                            xy,
                            yaw,
                            near_z: c.pos[2],
                            on_lane: true,
                            via: ResetVia::Crumb,
                        };
                    }
                }
            }
        }
        // 3. The nearest clear lane spot anywhere (same level first), heading the car's way
        //    along it.
        let mut all: Vec<(f32, usize, f32, bool)> = Vec::new();
        for i in 0..n {
            let seg = &map.segs[i];
            if !seg.drivable || seg.length <= 0.0 {
                continue;
            }
            let pr = seg.project(p);
            let s = pr.s.clamp(0.0, seg.length);
            let (pt, ldir) = seg.point_at(s);
            let agrees = ldir[0] * dir[0] + ldir[1] * dir[1] >= 0.0;
            let key = pr.dist
                + LEVEL_WEIGHT * (pt[2] - pos[2]).abs()
                + if agrees { 0.0 } else { OPPOSING_PENALTY_M };
            all.push((key, i, s, agrees));
        }
        all.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for &(_, i, s, agrees) in all.iter().take(FALLBACK_LANES) {
            if let Some((pt, yaw)) = lane_spot(map, i, s, agrees, p, None, obstacles) {
                return ResetPose {
                    xy: [pt[0], pt[1]],
                    yaw,
                    near_z: pt[2],
                    on_lane: true,
                    via: ResetVia::NearestLane,
                };
            }
        }
    }
    // 4. No lane at all: upright here, or a little along the heading.
    let yaw = yaw_of(dir);
    for step in OPEN_STEPS {
        let xy = [p[0] + dir[0] * step, p[1] + dir[1] * step];
        if is_clear(xy, yaw, obstacles) {
            return ResetPose {
                xy,
                yaw,
                near_z: pos[2],
                on_lane: false,
                via: ResetVia::InPlace,
            };
        }
    }
    ResetPose {
        xy: p,
        yaw,
        near_z: pos[2],
        on_lane: false,
        via: ResetVia::InPlace,
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
        let pose = reset_pose(None, [3.0, 4.0, 0.5], [0.0, 1.0], &[], None);
        assert_eq!(pose.xy, [3.0, 4.0]);
        assert!(pose.yaw.abs() < 1e-6 && !pose.on_lane);
        // Another car sitting on the spot: step along the heading.
        let blocker = Obstacle {
            pos: [3.0, 4.0, 0.5],
            yaw: 0.0,
            half: OTHER_HALF,
        };
        let pose = reset_pose(None, [3.0, 4.0, 0.5], [0.0, 1.0], &[blocker], None);
        assert!(is_clear(pose.xy, pose.yaw, &[blocker]));
        assert!((pose.xy[1] - 4.0).abs() <= 8.0 + 1e-4);
    }
}
