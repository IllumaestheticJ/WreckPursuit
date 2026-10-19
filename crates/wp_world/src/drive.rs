//! Shared AI driving controller: pure-pursuit steering + PID speed control.
//!
//! Built for physical (near-LOD) civilian traffic and shared with every other AI driver
//! (`wp_opponent` racers, police). It turns "where should I be" (a target point ahead) and
//! "how should my speed change" (a commanded acceleration) into a [`VehicleInput`]'s steer,
//! throttle and brake. Higher-level behaviour (paths, speed planning, tactics) lives in the
//! callers; this module only tracks.
//!
//! Defaults ([`DriveGains::TRAFFIC`]) are exactly the gains civilian traffic always used.
//!
//! Two levels:
//! - [`DriveController`]: the raw tracker — you supply the look-ahead target and a commanded
//!   acceleration (what traffic does, with IDM supplying the acceleration).
//! - [`PathFollower`]: `follow_path`-style driving along any [`DrivePath`] (a race route, a
//!   police pursuit path, a lane sequence): projects the car onto the path, picks the
//!   look-ahead point (with an optional lateral offset), plans speed from the target speed,
//!   the curvature ahead (lateral-acceleration limit) and the end of the path, and returns a
//!   complete [`VehicleInput`].

use wp_contracts::{VehicleDef, VehicleInput, VehicleState};

use crate::geom::{self, V2, V3};
pub use crate::leader::HOLD_BEHIND_MPS;
use crate::leader::{find_leader, CarFollowing, Leader, LeaderParams, Obstacle};

/// Tuning for [`DriveController`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriveGains {
    /// Lookahead distance at standstill, m.
    pub lookahead_base_m: f32,
    /// Lookahead added per m/s of speed, s.
    pub lookahead_per_mps: f32,
    /// Minimum lookahead, m.
    pub lookahead_min_m: f32,
    /// Maximum lookahead, m.
    pub lookahead_max_m: f32,
    /// Commanded acceleration that alone maps to full pedal (feed-forward), m/s².
    pub accel_full_pedal: f32,
    /// Proportional pedal per m/s of speed error.
    pub kp: f32,
    /// Integral pedal per m of accumulated speed error.
    pub ki: f32,
    /// Integral clamp, m.
    pub integral_limit: f32,
    /// Reference speed is re-synced to the measured speed when they drift further apart, m/s.
    pub resync_mps: f32,
    /// Below this speed the brake is released (no creeping backwards), m/s.
    pub hold_speed_mps: f32,
}

impl DriveGains {
    /// The gains civilian traffic uses (machine D's original controller).
    pub const TRAFFIC: Self = Self {
        lookahead_base_m: 4.0,
        lookahead_per_mps: 0.6,
        lookahead_min_m: 5.0,
        lookahead_max_m: 25.0,
        accel_full_pedal: 6.0,
        kp: 0.5,
        ki: 0.1,
        integral_limit: 5.0,
        resync_mps: 3.0,
        hold_speed_mps: 0.6,
    };
}

impl Default for DriveGains {
    fn default() -> Self {
        Self::TRAFFIC
    }
}

/// Pure-pursuit + PID tracker for one vehicle. Deterministic (IEEE `+ - * /`, polynomial
/// `atan`); call [`DriveController::steer`] and [`DriveController::pedals`] at a fixed rate.
#[derive(Debug, Clone, PartialEq)]
pub struct DriveController {
    /// Gains.
    pub gains: DriveGains,
    /// Wheelbase, m.
    pub wheelbase_m: f32,
    /// Road-wheel angle at full steering input, rad.
    pub max_steer_rad: f32,
    v_ref: f32,
    integ: f32,
}

impl DriveController {
    /// New controller for a car with this wheelbase and full-lock road-wheel angle, starting at
    /// `speed_mps`.
    pub fn new(gains: DriveGains, wheelbase_m: f32, max_steer_rad: f32, speed_mps: f32) -> Self {
        Self {
            gains,
            wheelbase_m,
            max_steer_rad: max_steer_rad.max(0.1),
            v_ref: speed_mps,
            integ: 0.0,
        }
    }

    /// Lookahead distance at speed `v`, m.
    pub fn lookahead(&self, v: f32) -> f32 {
        let g = &self.gains;
        (g.lookahead_base_m + g.lookahead_per_mps * v.abs())
            .clamp(g.lookahead_min_m, g.lookahead_max_m)
    }

    /// Pure-pursuit steering input (-1..1, + = right) toward `target` for a car at `pos`
    /// facing unit `forward` (both horizontal).
    pub fn steer(&self, pos: V2, forward: V2, target: V3) -> f32 {
        let d = geom::sub(geom::xy(target), pos);
        let long = geom::dot(d, forward);
        let latd = geom::dot(d, geom::right_of(forward));
        let l2 = (long * long + latd * latd).max(1.0);
        let curvature = -2.0 * latd / l2;
        let delta = geom::atan(self.wheelbase_m * curvature);
        (-delta / self.max_steer_rad).clamp(-1.0, 1.0)
    }

    /// Throttle and brake (0..1 each) for commanded acceleration `acc` (m/s²) at measured
    /// forward speed `v`, advancing the PID by `dt`. `disabled` = hold the brakes.
    pub fn pedals(&mut self, v: f32, acc: f32, dt: f32, disabled: bool) -> (f32, f32) {
        let g = self.gains;
        self.v_ref = (self.v_ref + acc * dt).max(0.0);
        if (self.v_ref - v).abs() > g.resync_mps {
            self.v_ref = v.max(0.0);
            self.integ = 0.0;
        }
        let err = self.v_ref - v;
        self.integ = (self.integ + err * dt).clamp(-g.integral_limit, g.integral_limit);
        let u = acc / g.accel_full_pedal + g.kp * err + g.ki * self.integ;
        if disabled {
            (0.0, if v > g.hold_speed_mps { 1.0 } else { 0.0 })
        } else if u >= 0.0 {
            (u.min(1.0), 0.0)
        } else if v > g.hold_speed_mps {
            (0.0, (-u).min(1.0))
        } else {
            (0.0, 0.0)
        }
    }

    /// Reset the speed reference (after a teleport or a reverse manoeuvre).
    pub fn resync(&mut self, speed_mps: f32) {
        self.v_ref = speed_mps.max(0.0);
        self.integ = 0.0;
    }
}

/// A polyline to drive along (open or looped), arc-length parametrised.
#[derive(Debug, Clone, PartialEq)]
pub struct DrivePath {
    pts: Vec<V3>,
    cum: Vec<f32>,
    looped: bool,
}

impl DrivePath {
    /// Path through `points` (consecutive duplicates dropped). A looped path closes back to its
    /// first point and arc lengths wrap.
    ///
    /// # Panics
    /// If fewer than two distinct points remain.
    pub fn new(points: &[V3], looped: bool) -> Self {
        let mut pts: Vec<V3> = Vec::with_capacity(points.len() + 1);
        for p in points {
            if pts
                .last()
                .is_none_or(|q| geom::dist(geom::xy(*q), geom::xy(*p)) > 0.01)
            {
                pts.push(*p);
            }
        }
        if looped
            && pts.len() > 2
            && geom::dist(geom::xy(pts[0]), geom::xy(pts[pts.len() - 1])) > 0.01
        {
            pts.push(pts[0]);
        }
        assert!(pts.len() >= 2, "a drive path needs two distinct points");
        let cum = geom::cumulative(&pts);
        Self { pts, cum, looped }
    }

    /// Path along a race route ([`crate::Route`]); circuits loop.
    pub fn from_route(route: &crate::Route) -> Self {
        Self::new(&route.polyline, route.closed_loop)
    }

    /// Length, m.
    pub fn length(&self) -> f32 {
        self.cum[self.cum.len() - 1]
    }

    /// Whether arc length wraps.
    pub fn looped(&self) -> bool {
        self.looped
    }

    /// Points.
    pub fn points(&self) -> &[V3] {
        &self.pts
    }

    fn wrap(&self, s: f32) -> f32 {
        if self.looped {
            s.rem_euclid(self.length())
        } else {
            s
        }
    }

    /// Point and unit tangent at arc length `s` (wrapped when looped, extrapolated past the
    /// ends otherwise).
    pub fn point_at(&self, s: f32) -> (V3, V2) {
        geom::point_at(&self.pts, &self.cum, self.wrap(s))
    }

    /// Closest point to `p`: `(s, signed lateral offset (+ right), distance)`. With a `hint`
    /// (last known `s`) only the stretch `hint - 20 m ..= hint + 60 m` is searched, so
    /// self-crossing circuits do not jump; it falls back to the whole path if the car is more
    /// than 15 m from that stretch.
    pub fn project(&self, p: V2, hint: Option<f32>) -> (f32, f32, f32) {
        let n = self.pts.len() - 1;
        let pick = |best: Option<(f32, f32, f32)>, c: (f32, f32, f32)| match best {
            Some(b) if b.2 <= c.2 => Some(b),
            _ => Some(c),
        };
        if let Some(h) = hint {
            let (lo, hi) = (h - 20.0, h + 60.0);
            let len = self.length();
            let mut best = None;
            for i in 0..n {
                let (c0, c1) = (self.cum[i], self.cum[i + 1]);
                let hit = if self.looped {
                    [-len, 0.0, len]
                        .iter()
                        .any(|o| c1 + o >= lo && c0 + o <= hi)
                } else {
                    c1 >= lo && c0 <= hi
                };
                if hit {
                    best = pick(best, self.project_segment(i, p));
                }
            }
            if let Some(b) = best.filter(|b| b.2 <= 15.0) {
                return b;
            }
        }
        let mut best = None;
        for i in 0..n {
            best = pick(best, self.project_segment(i, p));
        }
        best.expect("path has segments")
    }

    /// Nearest obstacle ahead of a car at `own_pos` (arc length `s` on this path, driving
    /// `lateral_offset_m` right of it) — [`find_leader`] along this path. `skip` = ids to
    /// ignore (the car itself, a target it means to hit).
    pub fn leader(
        &self,
        own_pos: V3,
        s: f32,
        lateral_offset_m: f32,
        obstacles: &[Obstacle],
        skip: &[u32],
        params: &LeaderParams,
    ) -> Option<Leader> {
        let end = if self.looped {
            f32::INFINITY
        } else {
            self.length()
        };
        find_leader(
            own_pos,
            |d| {
                // Stop the corridor at the end of an open path (no extrapolation past it).
                let (p, t) = self.point_at((s + d).min(end));
                let r = geom::right_of(t);
                [
                    p[0] + r[0] * lateral_offset_m,
                    p[1] + r[1] * lateral_offset_m,
                ]
            },
            obstacles,
            skip,
            params,
        )
    }

    /// `(s, lateral, distance)` of `p` against segment `i`.
    fn project_segment(&self, i: usize, p: V2) -> (f32, f32, f32) {
        let (a, b) = (geom::xy(self.pts[i]), geom::xy(self.pts[i + 1]));
        let ab = geom::sub(b, a);
        let len = geom::len(ab).max(1e-6);
        let dir = geom::scale(ab, 1.0 / len);
        let t = (geom::dot(geom::sub(p, a), dir) / len).clamp(0.0, 1.0);
        let c = geom::add(a, geom::scale(ab, t));
        let lat = geom::dot(geom::sub(p, a), geom::right_of(dir));
        (self.cum[i] + t * len, lat, geom::dist(p, c))
    }
}

/// Speed planning limits for [`PathFollower`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FollowParams {
    /// Maximum commanded acceleration, m/s².
    pub max_accel: f32,
    /// Deceleration used to plan for curves and the end of the path, m/s².
    pub plan_decel: f32,
    /// Hardest commanded deceleration, m/s².
    pub max_decel: f32,
    /// Lateral acceleration allowed in curves, m/s².
    pub max_lat_accel: f32,
    /// Speed error → commanded acceleration gain, 1/s. Too low and the car reaches corners
    /// faster than planned and runs wide (1.5 → 6 m off line at pursuit pace; 3 → 1.4 m).
    pub speed_gain: f32,
    /// Open paths: stop this far before the end, m.
    pub stop_margin_m: f32,
}

impl FollowParams {
    /// Civilian-like driving (gentle).
    pub const CIVILIAN: Self = Self {
        max_accel: 2.0,
        plan_decel: 2.5,
        max_decel: 7.0,
        max_lat_accel: 2.8,
        speed_gain: 2.0,
        stop_margin_m: 1.0,
    };
    /// Pursuit / emergency driving (close to the grip limit of a street car).
    pub const PURSUIT: Self = Self {
        max_accel: 4.0,
        plan_decel: 6.0,
        max_decel: 8.5,
        max_lat_accel: 6.5,
        speed_gain: 3.0,
        stop_margin_m: 1.0,
    };
}

impl Default for FollowParams {
    fn default() -> Self {
        Self::CIVILIAN
    }
}

/// Result of one [`PathFollower::follow`] call.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FollowOutput {
    /// Driver input to apply until the next call.
    pub input: VehicleInput,
    /// Arc length of the car on the path, m.
    pub s: f32,
    /// Lateral offset from the path (+ right), m.
    pub cross_track_m: f32,
    /// Distance left to the end (open paths; `INFINITY` when looped), m.
    pub remaining_m: f32,
    /// Speed the planner is aiming for now, m/s.
    pub planned_speed_mps: f32,
    /// Open path finished: within the stop margin and (nearly) stopped.
    pub done: bool,
    /// Vehicle followed this step ([`PathFollower::follow_behind`] only).
    pub leader: Option<Leader>,
}

/// Other vehicles for [`PathFollower::follow_behind`].
#[derive(Debug, Clone, Copy)]
pub struct FollowTraffic<'a> {
    /// Every vehicle near the car (it may include the car itself if its id is in `skip`).
    pub obstacles: &'a [Obstacle],
    /// Ids to ignore (own id, a target the car means to hit).
    pub skip: &'a [u32],
    /// Corridor shape; the look-ahead is re-scaled to speed each step
    /// ([`LeaderParams::lookahead_for`]).
    pub corridor: LeaderParams,
    /// Car-following law.
    pub following: CarFollowing,
}

/// `follow_path`: drives a vehicle along a [`DrivePath`] with the shared controller. Keep one
/// per vehicle (it holds the PID state and the last path position); call at a fixed rate.
#[derive(Debug, Clone, PartialEq)]
pub struct PathFollower {
    /// Steering / pedal tracker.
    pub controller: DriveController,
    /// Speed planning limits.
    pub params: FollowParams,
    s_hint: Option<f32>,
}

impl PathFollower {
    /// Follower with an existing controller.
    pub fn new(controller: DriveController, params: FollowParams) -> Self {
        Self {
            controller,
            params,
            s_hint: None,
        }
    }

    /// Follower for a car definition (wheelbase and full-lock angle from `def`) currently at
    /// `speed_mps`, with traffic gains.
    pub fn for_vehicle(def: &VehicleDef, params: FollowParams, speed_mps: f32) -> Self {
        let max_steer = def
            .wheels
            .iter()
            .map(|w| w.max_steer_rad)
            .fold(0.0, f32::max);
        Self::new(
            DriveController::new(DriveGains::TRAFFIC, def.wheelbase_m, max_steer, speed_mps),
            params,
        )
    }

    /// Forget the path position and PID state (new path, teleport).
    pub fn reset(&mut self, speed_mps: f32) {
        self.s_hint = None;
        self.controller.resync(speed_mps);
    }

    /// Highest speed allowed now so that every curve within the braking horizon (and the end
    /// of an open path) can be met at `plan_decel`.
    pub fn planned_speed(&self, path: &DrivePath, s: f32, v: f32, target_mps: f32) -> f32 {
        let p = &self.params;
        let mut allowed = target_mps.max(0.0);
        let horizon = (v * v / (2.0 * p.plan_decel) + 25.0).min(200.0);
        const STEP: f32 = 4.0;
        let mut d = 0.0;
        while d <= horizon {
            let (_, t0) = path.point_at(s + d);
            let (_, t1) = path.point_at(s + d + STEP);
            let ang = geom::atan2(geom::cross(t0, t1), geom::dot(t0, t1)).abs();
            let kappa = ang / STEP;
            if kappa > 1e-4 {
                let vc2 = p.max_lat_accel / kappa;
                allowed = allowed.min((vc2 + 2.0 * p.plan_decel * d).sqrt());
            }
            d += STEP;
        }
        if !path.looped {
            let rem = (path.length() - p.stop_margin_m - s).max(0.0);
            allowed = allowed.min((2.0 * p.plan_decel * rem).sqrt());
        }
        allowed
    }

    /// One control step: input for a car in `state` following `path` at up to `target_mps`,
    /// offset `lateral_offset_m` to the right of the path (negative = left), advancing the PID
    /// by `dt` seconds.
    pub fn follow(
        &mut self,
        state: &VehicleState,
        path: &DrivePath,
        target_mps: f32,
        lateral_offset_m: f32,
        dt: f32,
    ) -> FollowOutput {
        self.follow_behind(state, path, target_mps, lateral_offset_m, dt, None)
    }

    /// [`PathFollower::follow`] that also keeps its distance from vehicles ahead: looks for
    /// the nearest of `traffic.obstacles` in the corridor along the path and caps the
    /// commanded acceleration with the IDM law (`traffic.following`), exactly how civilian
    /// traffic follows cars — slows behind a slower car, stops behind a stopped one at the
    /// standstill gap. `None` traffic = plain [`PathFollower::follow`]. The leader found is
    /// returned in [`FollowOutput::leader`].
    pub fn follow_behind(
        &mut self,
        state: &VehicleState,
        path: &DrivePath,
        target_mps: f32,
        lateral_offset_m: f32,
        dt: f32,
        traffic: Option<&FollowTraffic<'_>>,
    ) -> FollowOutput {
        let (pos, fwd) = pose_2d(state);
        let v = state.speed_mps;
        let (s, lat, _) = path.project(pos, self.s_hint);
        self.s_hint = Some(s);
        let planned = self.planned_speed(path, s, v, target_mps);
        // Look ahead for the speed we are about to drive, so the target point does not jump
        // across the inside of a corner we are braking for.
        let look = self.controller.lookahead(v.min(planned.max(0.0)));
        let (tp, tt) = path.point_at(s + look);
        let r = geom::right_of(tt);
        let target = [
            tp[0] + r[0] * lateral_offset_m,
            tp[1] + r[1] * lateral_offset_m,
            tp[2],
        ];
        let steer = self.controller.steer(pos, fwd, target);
        let p = self.params;
        let mut acc = ((planned - v) * p.speed_gain).clamp(-p.max_decel, p.max_accel);
        let leader = traffic.and_then(|t| {
            let lp = t.corridor.lookahead_for(v);
            path.leader(state.pos, s, lateral_offset_m, t.obstacles, t.skip, &lp)
        });
        // Held behind a stopped / stopping leader: the PID releases the brake near standstill
        // (holding it engages auto-reverse), so the car would creep into the leader.
        let mut hold = false;
        if let (Some(t), Some(l)) = (traffic, leader.as_ref()) {
            let cap = t.following.leader_cap(v, Some(l));
            acc = acc.min(cap.max(-p.max_decel));
            hold = crate::leader::hold_behind(v, cap, l);
        }
        let remaining = if path.looped {
            f32::INFINITY
        } else {
            (path.length() - s).max(0.0)
        };
        let done = !path.looped && remaining <= p.stop_margin_m + 0.5 && v < 0.5;
        let (throttle, brake) = self.controller.pedals(v, acc, dt, done);
        let mut input = VehicleInput {
            steer,
            throttle,
            brake,
            ..Default::default()
        };
        if hold {
            input = crate::leader::held(input);
        }
        FollowOutput {
            input,
            s,
            cross_track_m: lat - lateral_offset_m,
            remaining_m: remaining,
            planned_speed_mps: planned,
            done,
            leader,
        }
    }
}

/// Horizontal position and unit forward direction of a vehicle state.
pub fn pose_2d(state: &VehicleState) -> (V2, V2) {
    (geom::xy(state.pos), forward_of(state.rot))
}

/// Unit horizontal forward (+Y body axis) of a quaternion `[x, y, z, w]`.
pub fn forward_of(q: [f32; 4]) -> V2 {
    let [x, y, z, w] = q;
    geom::normalize([2.0 * (x * y - w * z), 1.0 - 2.0 * (x * x + z * z)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steers_toward_target_side() {
        let c = DriveController::new(DriveGains::TRAFFIC, 2.7, 0.6, 10.0);
        // Target ahead and to the right (+X) → positive (right) steer.
        let s = c.steer([0.0, 0.0], [0.0, 1.0], [3.0, 10.0, 0.0]);
        assert!(s > 0.0, "{s}");
        let s = c.steer([0.0, 0.0], [0.0, 1.0], [-3.0, 10.0, 0.0]);
        assert!(s < 0.0, "{s}");
        assert_eq!(c.steer([0.0, 0.0], [0.0, 1.0], [0.0, 10.0, 0.0]), 0.0);
    }

    #[test]
    fn path_projection_and_wrap() {
        let sq = [
            [0.0, 0.0, 0.0],
            [100.0, 0.0, 0.0],
            [100.0, 100.0, 0.0],
            [0.0, 100.0, 0.0],
        ];
        let p = DrivePath::new(&sq, true);
        assert_eq!(p.length(), 400.0);
        let (s, lat, d) = p.project([50.0, 2.0], None);
        assert!((s - 50.0).abs() < 1e-4 && (lat + 2.0).abs() < 1e-4 && (d - 2.0).abs() < 1e-4);
        // Wraps past the end of a loop.
        let (q, _) = p.point_at(410.0);
        assert!((q[0] - 10.0).abs() < 1e-4 && q[1].abs() < 1e-4);
        // Hint keeps the projection on the nearby stretch.
        let (s, _, _) = p.project([2.0, 1.0], Some(395.0));
        assert!(!(5.0..=390.0).contains(&s), "{s}");
        let open = DrivePath::new(&sq, false);
        assert_eq!(open.length(), 300.0);
    }

    #[test]
    fn planner_slows_for_corners_and_the_end() {
        let f = PathFollower::new(
            DriveController::new(DriveGains::TRAFFIC, 2.7, 0.6, 0.0),
            FollowParams::CIVILIAN,
        );
        let l = DrivePath::new(
            &[[0.0, 0.0, 0.0], [0.0, 200.0, 0.0], [200.0, 200.0, 0.0]],
            false,
        );
        // Far from the corner: target speed. Near the 90° corner: much slower.
        assert!((f.planned_speed(&l, 0.0, 15.0, 15.0) - 15.0).abs() < 1e-3);
        assert!(f.planned_speed(&l, 195.0, 15.0, 15.0) < 8.0);
        // Approaching the end of an open path: stop.
        assert!(f.planned_speed(&l, 399.0, 1.0, 15.0) < 0.05);
        assert!(f.planned_speed(&l, 390.0, 5.0, 15.0) < 7.0);
    }

    #[test]
    fn pedals_follow_acceleration_sign() {
        let mut c = DriveController::new(DriveGains::TRAFFIC, 2.7, 0.6, 10.0);
        let (t, b) = c.pedals(10.0, 3.0, 1.0 / 60.0, false);
        assert!(t > 0.0 && b == 0.0);
        let mut c = DriveController::new(DriveGains::TRAFFIC, 2.7, 0.6, 10.0);
        let (t, b) = c.pedals(10.0, -8.0, 1.0 / 60.0, false);
        assert!(t == 0.0 && b > 0.9);
        let (t, b) = c.pedals(10.0, 0.0, 1.0 / 60.0, true);
        assert_eq!((t, b), (0.0, 1.0));
    }
}
