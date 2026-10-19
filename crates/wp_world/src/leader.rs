//! Leader / obstacle query for AI drivers that are not lane-bound traffic (police units, AI
//! racers): "what is the nearest vehicle ahead of me on the path I am about to drive, how far
//! away is it and how fast is it going along my path?", plus the IDM car-following law civilian
//! traffic uses, so these drivers slow down and stop behind other cars the same way.
//!
//! Everything here is pure geometry on data the caller passes in; it knows nothing about
//! `wp_sim` entities. The caller builds one [`Obstacle`] per nearby vehicle (players, physical
//! traffic, kinematic traffic via [`crate::AgentView`], other units) and describes its own path
//! ahead with a closure (`d` metres ahead → centreline point), so any path type works:
//! [`crate::drive::DrivePath`] has a ready-made wrapper ([`DrivePath::leader`]), and
//! `wp_police` can pass its own track sampler.
//!
//! How the corridor works: the path ahead is sampled every [`LeaderParams::step_m`] from the
//! car's own position out to [`LeaderParams::lookahead_m`]. Each obstacle's footprint (an
//! oriented box) is projected onto that polyline; it is "in the way" when its box overlaps a
//! strip [`LeaderParams::corridor_half_width_m`] either side of the centreline. The leader is
//! the in-the-way obstacle with the smallest bumper-to-bumper gap. Its speed is its velocity
//! along the path tangent at its position (negative for oncoming cars), which is what IDM
//! wants.
//!
//! [`DrivePath::leader`]: crate::drive::DrivePath::leader

use wp_contracts::{VehicleInput, VehicleState};

use crate::drive::forward_of;
use crate::geom::{self, V2, V3};

/// Default half width of a car footprint, m (matches traffic's `CAR_WIDTH_M / 2`).
pub const CAR_HALF_WIDTH_M: f32 = 0.9;
/// Default half length of a car footprint, m (matches traffic's `CAR_LENGTH_M / 2`).
pub const CAR_HALF_LENGTH_M: f32 = 2.25;

/// A vehicle (or any box-shaped thing) another driver may have to follow or stop behind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Obstacle {
    /// Caller's id (e.g. `EntityId.0`, `AgentId.0`); returned in [`Leader::id`].
    pub id: u32,
    /// Centre position, m.
    pub pos: V3,
    /// Velocity, m/s.
    pub vel: V3,
    /// Unit horizontal forward direction (the box's long axis).
    pub forward: V2,
    /// Half width of the footprint, m.
    pub half_width_m: f32,
    /// Half length of the footprint, m.
    pub half_length_m: f32,
}

impl Obstacle {
    /// Obstacle for a vehicle state with the default car footprint.
    pub fn from_state(id: u32, state: &VehicleState) -> Self {
        Self {
            id,
            pos: state.pos,
            vel: state.lin_vel,
            forward: forward_of(state.rot),
            half_width_m: CAR_HALF_WIDTH_M,
            half_length_m: CAR_HALF_LENGTH_M,
        }
    }

    /// Obstacle for a kinematic traffic agent ([`crate::AgentView`]).
    pub fn from_agent(view: &crate::AgentView) -> Self {
        let forward = geom::dir_of_yaw(view.yaw);
        Self {
            id: view.id,
            pos: view.pos,
            vel: [
                forward[0] * view.speed_mps,
                forward[1] * view.speed_mps,
                0.0,
            ],
            forward,
            half_width_m: CAR_HALF_WIDTH_M,
            half_length_m: CAR_HALF_LENGTH_M,
        }
    }

    /// Same obstacle with a different footprint (half width, half length), m.
    pub fn with_size(mut self, half_width_m: f32, half_length_m: f32) -> Self {
        self.half_width_m = half_width_m;
        self.half_length_m = half_length_m;
        self
    }
}

/// Corridor shape for [`find_leader`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LeaderParams {
    /// How far ahead along the path to look, m. Use [`LeaderParams::lookahead_for`] to scale it
    /// with speed.
    pub lookahead_m: f32,
    /// Half width of the strip around the centreline that counts as "in the way", m. 1.2 m
    /// (a car plus a little) keeps cars in neighbouring lanes (≥ 3 m apart) out.
    pub corridor_half_width_m: f32,
    /// The querying car's own half length (front bumper = centre + this), m.
    pub own_half_length_m: f32,
    /// Sampling step along the path, m.
    pub step_m: f32,
    /// Obstacles further than this above or below the car are ignored (bridges), m.
    pub max_height_diff_m: f32,
}

impl LeaderParams {
    /// Defaults for a car: 60 m ahead, 1.2 m corridor, 2 m steps.
    pub const CAR: Self = Self {
        lookahead_m: 60.0,
        corridor_half_width_m: 1.2,
        own_half_length_m: CAR_HALF_LENGTH_M,
        step_m: 2.0,
        max_height_diff_m: 4.0,
    };

    /// The same corridor with the look-ahead scaled to speed `v` (m/s): far enough to stop from
    /// `v` at 6 m/s² plus 3 s of travel and a 20 m margin, at least 40 m, at most 150 m.
    pub fn lookahead_for(mut self, v: f32) -> Self {
        let v = v.max(0.0);
        self.lookahead_m = (v * v / 12.0 + 3.0 * v + 20.0).clamp(40.0, 150.0);
        self
    }
}

impl Default for LeaderParams {
    fn default() -> Self {
        Self::CAR
    }
}

/// The nearest obstacle ahead in the corridor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Leader {
    /// The obstacle's [`Obstacle::id`].
    pub id: u32,
    /// Bumper-to-bumper gap along the path, m (0 when already touching / overlapping).
    pub gap_m: f32,
    /// The obstacle's velocity along the path direction at its position, m/s (negative =
    /// coming towards us).
    pub speed_mps: f32,
    /// Distance of the obstacle's centre along the path from our centre, m.
    pub along_m: f32,
    /// Signed lateral offset of the obstacle's centre from the centreline (+ right), m.
    pub lateral_m: f32,
}

/// Nearest obstacle in the corridor ahead of a car at `own_pos`.
///
/// `centreline(d)` returns the point the car will be at after driving `d` metres along its path
/// (`d = 0` is the car's projection onto the path; include any lane offset the car drives
/// with). The corridor starts at the car's own position, so an obstacle right in front of the
/// bumper counts even when the car is off its path. Obstacles whose id is in `skip` (the car
/// itself, a pursuit target the unit means to hit) are ignored. Ties break on the smaller id,
/// so the result does not depend on the order of `obstacles`.
pub fn find_leader(
    own_pos: V3,
    mut centreline: impl FnMut(f32) -> V2,
    obstacles: &[Obstacle],
    skip: &[u32],
    params: &LeaderParams,
) -> Option<Leader> {
    let p = params;
    let step = p.step_m.max(0.5);
    let reach = p.lookahead_m.max(step);
    // Corridor polyline: own position, then the path every `step` metres.
    let mut pts: Vec<V2> = Vec::with_capacity((reach / step) as usize + 2);
    pts.push(geom::xy(own_pos));
    let mut d = step;
    loop {
        let q = centreline(d.min(reach));
        if geom::dist(q, pts[pts.len() - 1]) > 1e-3 {
            pts.push(q);
        }
        if d >= reach {
            break;
        }
        d += step;
    }
    if pts.len() < 2 {
        return None;
    }
    let cum = cumulative(&pts);
    let total = cum[cum.len() - 1];
    // Cheap pre-filter: nothing outside a circle around the corridor can be in it.
    let margin = p.corridor_half_width_m + 4.0;
    let mut best: Option<Leader> = None;
    for o in obstacles {
        if skip.contains(&o.id) || (o.pos[2] - own_pos[2]).abs() > p.max_height_diff_m {
            continue;
        }
        let c = geom::xy(o.pos);
        if geom::dist(c, pts[0]) > total + margin {
            continue;
        }
        let Some((along, lat, tangent)) = project(&pts, &cum, c) else {
            continue;
        };
        // Footprint extents along and across the path at the obstacle.
        let f = geom::normalize(o.forward);
        let r = geom::right_of(f);
        let n = geom::right_of(tangent);
        let ext_along = geom::dot(f, tangent).abs() * o.half_length_m
            + geom::dot(r, tangent).abs() * o.half_width_m;
        let ext_across =
            geom::dot(f, n).abs() * o.half_length_m + geom::dot(r, n).abs() * o.half_width_m;
        if lat.abs() - ext_across > p.corridor_half_width_m {
            continue;
        }
        // Must be ahead of our front bumper's rear-most reach (centre) and within look-ahead.
        if along + ext_along <= 0.0 || along - ext_along > total {
            continue;
        }
        let gap = (along - ext_along - p.own_half_length_m).max(0.0);
        let speed = geom::dot([o.vel[0], o.vel[1]], tangent);
        let cand = Leader {
            id: o.id,
            gap_m: gap,
            speed_mps: speed,
            along_m: along,
            lateral_m: lat,
        };
        best = match best {
            Some(b) if (b.gap_m, b.id) <= (cand.gap_m, cand.id) => Some(b),
            _ => Some(cand),
        };
    }
    best
}

fn cumulative(pts: &[V2]) -> Vec<f32> {
    let mut cum = Vec::with_capacity(pts.len());
    let mut acc = 0.0;
    cum.push(0.0);
    for w in pts.windows(2) {
        acc += geom::dist(w[0], w[1]);
        cum.push(acc);
    }
    cum
}

/// Closest point on the corridor polyline: `(arc length, signed lateral (+ right), tangent)`.
/// Points behind the first vertex extrapolate backwards along the first segment (negative
/// arc length); points past the last vertex extrapolate forwards.
fn project(pts: &[V2], cum: &[f32], c: V2) -> Option<(f32, f32, V2)> {
    let n = pts.len() - 1;
    let mut best: Option<(f32, f32, f32, V2)> = None; // (dist², along, lat, tangent)
    for i in 0..n {
        let (a, b) = (pts[i], pts[i + 1]);
        let ab = geom::sub(b, a);
        let len = geom::len(ab);
        if len < 1e-6 {
            continue;
        }
        let t = geom::scale(ab, 1.0 / len);
        let ac = geom::sub(c, a);
        let mut u = geom::dot(ac, t);
        if i > 0 {
            u = u.max(0.0);
        }
        if i + 1 < n {
            u = u.min(len);
        }
        let foot = geom::add(a, geom::scale(t, u));
        let off = geom::sub(c, foot);
        let d2 = geom::dot(off, off);
        // Signed distance to the clamped foot point (+ right): past a corner vertex the
        // offset is the full distance, not the offset from the segment's extension.
        let dist = d2.sqrt();
        let lat = if geom::dot(off, geom::right_of(t)) < 0.0 {
            -dist
        } else {
            dist
        };
        if best.is_none_or(|b| d2 < b.0) {
            best = Some((d2, cum[i] + u, lat, t));
        }
    }
    best.map(|b| (b.1, b.2, b.3))
}

/// IDM car-following parameters (the same law and meaning as traffic's
/// [`crate::traffic::DriverParams`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CarFollowing {
    /// Time headway, s.
    pub headway_s: f32,
    /// Standstill gap, m.
    pub min_gap_m: f32,
    /// Maximum acceleration, m/s².
    pub max_accel: f32,
    /// Comfortable deceleration, m/s² (the IDM `b`; IDM brakes harder than this when needed).
    pub comfort_decel: f32,
}

impl CarFollowing {
    /// A law-abiding civilian (traffic's middle-of-the-range values).
    pub const CIVILIAN: Self = Self {
        headway_s: 1.5,
        min_gap_m: 2.0,
        max_accel: 2.0,
        comfort_decel: 2.5,
    };
    /// Emergency / pursuit driving: short headway, hard but controlled braking. Still keeps a
    /// standstill gap so units do not nudge stopped cars.
    pub const PURSUIT: Self = Self {
        headway_s: 0.8,
        min_gap_m: 2.5,
        max_accel: 4.0,
        comfort_decel: 5.0,
    };

    /// IDM acceleration at speed `v` with desired speed `v0`, `gap` m to a leader moving at
    /// `v_lead` (`gap = INFINITY` = free road). Above `v0` the free term is the IIDM braking
    /// form. Bit-identical to what civilian traffic computes.
    pub fn idm(&self, v: f32, v0: f32, gap: f32, v_lead: f32) -> f32 {
        let v0 = v0.max(0.1);
        // x⁴ by multiplies: `powi` is not guaranteed bit-identical across targets.
        let pow4 = |x: f32| {
            let x2 = x * x;
            x2 * x2
        };
        let free = if v <= v0 {
            self.max_accel * (1.0 - pow4(v / v0))
        } else {
            -self.comfort_decel * (1.0 - pow4(v0 / v))
        };
        if !gap.is_finite() {
            return free;
        }
        let dv = v - v_lead;
        let s_star = self.min_gap_m
            + (v * self.headway_s + v * dv / (2.0 * (self.max_accel * self.comfort_decel).sqrt()))
                .max(0.0);
        let g = gap.max(0.05);
        free - self.max_accel * (s_star / g) * (s_star / g)
    }

    /// Acceleration cap from the leader alone (IDM interaction term with the free-road term
    /// at full `max_accel`): combine with your own speed control as `acc.min(cap)`. `None` →
    /// `INFINITY` (no cap).
    pub fn leader_cap(&self, v: f32, leader: Option<&Leader>) -> f32 {
        match leader {
            Some(l) => self.idm(v, f32::INFINITY, l.gap_m, l.speed_mps),
            None => f32::INFINITY,
        }
    }
}

impl Default for CarFollowing {
    fn default() -> Self {
        Self::CIVILIAN
    }
}

/// Below this speed, behind a (nearly) stopped leader with the IDM cap asking to slow down, a
/// car should be held still ([`hold_behind`] / [`held`]), m/s.
pub const HOLD_BEHIND_MPS: f32 = 1.0;
/// Foot brake while held: under the drivetrain's 0.5 auto-reverse threshold.
const HOLD_BRAKE: f32 = 0.4;

/// Whether a car at speed `v` with leader cap `cap` ([`CarFollowing::leader_cap`]) behind
/// `leader` should be held still with [`held`].
///
/// Why this exists: [`crate::drive::DriveController::pedals`] releases the brake near
/// standstill, because holding the brake at a standstill engages `wp_vehicle`'s auto-reverse.
/// Without a hold the car idles forward and creeps into the car it stopped behind.
pub fn hold_behind(v: f32, cap: f32, leader: &Leader) -> bool {
    cap < 0.0 && v < HOLD_BEHIND_MPS && leader.speed_mps < HOLD_BEHIND_MPS
}

/// `input` changed to hold the car still: no throttle, handbrake on, foot brake capped under
/// the auto-reverse threshold. Steering and the other controls are kept.
pub fn held(input: VehicleInput) -> VehicleInput {
    VehicleInput {
        throttle: 0.0,
        brake: input.brake.min(HOLD_BRAKE),
        handbrake: 1.0,
        ..input
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn straight(d: f32) -> V2 {
        [0.0, d]
    }

    fn car(id: u32, x: f32, y: f32, vy: f32) -> Obstacle {
        Obstacle {
            id,
            pos: [x, y, 0.0],
            vel: [0.0, vy, 0.0],
            forward: [0.0, 1.0],
            half_width_m: CAR_HALF_WIDTH_M,
            half_length_m: CAR_HALF_LENGTH_M,
        }
    }

    #[test]
    fn finds_nearest_car_in_lane_and_ignores_neighbours() {
        let obs = [
            car(1, 0.0, 30.0, 5.0),
            car(2, 0.2, 20.0, 3.0),
            car(3, 3.5, 10.0, 0.0),  // neighbour lane
            car(4, -3.5, 8.0, -9.0), // oncoming lane
            car(5, 0.0, -10.0, 0.0), // behind
            car(6, 0.0, 90.0, 0.0),  // beyond look-ahead
        ];
        let l = find_leader([0.0, 0.0, 0.0], straight, &obs, &[], &LeaderParams::CAR).unwrap();
        assert_eq!(l.id, 2);
        assert!((l.gap_m - (20.0 - 4.5)).abs() < 1e-3, "{l:?}");
        assert!((l.speed_mps - 3.0).abs() < 1e-5);
        // Skipping it reveals the next one; skipping everything finds nothing.
        let l = find_leader([0.0, 0.0, 0.0], straight, &obs, &[2], &LeaderParams::CAR).unwrap();
        assert_eq!(l.id, 1);
        assert!(
            find_leader([0.0, 0.0, 0.0], straight, &obs, &[1, 2], &LeaderParams::CAR).is_none()
        );
    }

    #[test]
    fn crossing_car_counts_by_its_footprint() {
        // A car stopped sideways across the lane, centre 2.5 m to the right: its 2.25 m half
        // length reaches into the corridor.
        let o = car(7, 2.5, 25.0, 0.0);
        let side = Obstacle {
            forward: [1.0, 0.0],
            ..o
        };
        let l = find_leader([0.0, 0.0, 0.0], straight, &[side], &[], &LeaderParams::CAR).unwrap();
        assert_eq!(l.id, 7);
        // Along-path extent is its half width.
        assert!((l.gap_m - (25.0 - 0.9 - 2.25)).abs() < 1e-3, "{l:?}");
        // Pointing along the lane at the same offset, it is clear.
        assert!(find_leader([0.0, 0.0, 0.0], straight, &[o], &[], &LeaderParams::CAR).is_none());
    }

    #[test]
    fn follows_the_curve_of_the_path() {
        // Path turns right 90° after 10 m (radius-free corner). A car 15 m down the side
        // street is ahead on the path; a car straight on past the corner is not.
        let path = |d: f32| {
            if d <= 10.0 {
                [0.0, d]
            } else {
                [d - 10.0, 10.0]
            }
        };
        let down_side = car(1, 15.0, 10.0, 0.0);
        let straight_on = car(2, 0.0, 25.0, 0.0);
        let l = find_leader(
            [0.0, 0.0, 0.0],
            path,
            &[straight_on, down_side],
            &[],
            &LeaderParams::CAR,
        )
        .unwrap();
        assert_eq!(l.id, 1);
        assert!((l.along_m - 25.0).abs() < 1e-3, "{l:?}");
    }

    #[test]
    fn car_right_in_front_counts_when_off_path() {
        // The car has drifted 2 m left of its path; a stopped car is right in front of it.
        let o = car(3, -2.0, 6.0, 0.0);
        let l = find_leader([-2.0, 0.0, 0.0], straight, &[o], &[], &LeaderParams::CAR).unwrap();
        assert_eq!(l.id, 3);
        // Gap measured along the corridor's first (diagonal) leg back to the path.
        assert!(l.gap_m < 3.0, "{l:?}");
    }

    #[test]
    fn idm_brakes_for_a_stopped_leader_and_is_free_without_one() {
        let cf = CarFollowing::PURSUIT;
        assert_eq!(cf.leader_cap(20.0, None), f32::INFINITY);
        let near = Leader {
            id: 1,
            gap_m: 30.0,
            speed_mps: 0.0,
            along_m: 34.5,
            lateral_m: 0.0,
        };
        assert!(cf.leader_cap(20.0, Some(&near)) < -5.0);
        // Far-away leader at the same speed barely matters.
        let far = Leader {
            gap_m: 140.0,
            speed_mps: 20.0,
            ..near
        };
        assert!(cf.leader_cap(20.0, Some(&far)) > 3.0);
        // Standstill at the minimum gap: no creeping forward.
        let touching = Leader {
            gap_m: cf.min_gap_m,
            ..near
        };
        assert!(cf.leader_cap(0.0, Some(&touching)) <= 0.0);
    }

    #[test]
    fn lookahead_scales_with_speed() {
        assert_eq!(LeaderParams::CAR.lookahead_for(0.0).lookahead_m, 40.0);
        let l = LeaderParams::CAR.lookahead_for(30.0).lookahead_m;
        assert!(l > 30.0 * 30.0 / 12.0 && l <= 150.0, "{l}");
    }
}
