//! Police helicopter game model: a point-mass flight model with sim-cade limits and the air
//! unit's behaviour, flown entirely inside `wp_police` (the helicopter has no Rapier body).
//!
//! **Flight model** ([`fly`]): the helicopter asks for a horizontal velocity; it gets there at
//! most `accel_mps2` (≈ g·tan(max bank)), never faster than `speed_mps`. Bank and pitch follow
//! the acceleration it actually uses (first-order lag `attitude_tau_s`), the nose turns at most
//! `yaw_rate_rad_s` and altitude changes at most `climb_mps`. A racer above `speed_mps` on an
//! open road out-runs it; a hard direction change makes it overshoot.
//!
//! **Behaviour** ([`step`], one call per physics tick):
//! - [`HeloPhase::Launch`]: climb `launch_climb_m` straight up from the pad.
//! - [`HeloPhase::Transit`]: fly to the target's last known position, led by `lead_s`.
//! - [`HeloPhase::Track`]: orbit a slow target at `orbit_radius_m`, trail a fast one
//!   (`> orbit_max_target_mps`) by `trail_distance_m`; the spotlight follows the car.
//! - [`HeloPhase::Search`]: visual lost for `search_after_s`: orbit the last known position
//!   while the spotlight sweeps a circle that grows at `search_growth_mps`.
//! - [`HeloPhase::Return`]: job over or **bingo fuel** (fuel left only covers the flight home
//!   plus `reserve_s`): fly to the pad, descend inside `landing_radius_m`, then
//!   [`HeloPhase::Landed`] (the dispatcher removes it).
//!
//! **Visual** is decided by the police system each think step ([`HeloState::can_see`]): range,
//! line of sight from altitude and, where `spotlight_required` (night), the spotlight's ground
//! circle on the car. Breaking the beam (a sharp turn, a tunnel, tall buildings) is how racers
//! shake it.

use serde::{Deserialize, Serialize};
use wp_contracts::{DayPhase, EntityId};
use wp_math::{atan, atan2, sin_cos};

use crate::config::HeloConfig;
use crate::graph::{add, cross, dist, dot, len, normalize, perp_left, scale, sub};
use crate::time::SimTime;
use crate::unit::UnitBody;

/// Gravity for bank / pitch, m/s².
const G_MPS2: f32 = 9.81;

/// Below this speed the nose points at the target instead of along the velocity, m/s.
const HOVER_SPEED_MPS: f32 = 3.0;

/// Spotlight beam starts this far from the car before the search pattern widens it, m.
const SEARCH_START_RADIUS_M: f32 = 20.0;

/// What the helicopter is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HeloPhase {
    /// Climbing out of the pad.
    Launch,
    /// Flying to the target.
    Transit,
    /// Orbiting / trailing the target with visual.
    Track,
    /// Visual lost: search pattern around the last known position.
    Search,
    /// Flying back to the pad (job over or bingo fuel).
    Return,
    /// On the pad; the dispatcher removes it.
    Landed,
}

/// Flight and behaviour state of one helicopter (lives on its [`crate::Unit`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HeloState {
    /// Behaviour phase.
    pub phase: HeloPhase,
    /// Racer it was launched on.
    pub target: EntityId,
    /// Pad it flies home to.
    pub base: [f32; 2],
    /// Horizontal velocity, m/s.
    pub vel: [f32; 2],
    /// Vertical speed, m/s (+ up).
    pub climb_mps: f32,
    /// Roll, rad (+ right side down).
    pub bank_rad: f32,
    /// Pitch, rad (+ nose up).
    pub pitch_rad: f32,
    /// Spotlight aim point on the ground (XY), m.
    pub spot: [f32; 2],
    /// Has visual on the target (set every think step).
    pub visual: bool,
    /// Last time it had visual.
    pub last_visual: Option<SimTime>,
    /// Centre of the search pattern.
    pub search_center: [f32; 2],
    /// Radius of the spotlight's search circle, m.
    pub search_radius_m: f32,
    /// Angle of the spotlight around the search circle, rad.
    pub sweep_rad: f32,
    /// Fuel left, s of flight.
    pub fuel_s: f32,
}

/// What the helicopter knows about its target this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeloTarget {
    /// The car when it has visual, otherwise the last known position, m.
    pub pos: [f32; 2],
    /// The car's velocity when it has visual, otherwise zero, m/s.
    pub vel: [f32; 2],
}

impl HeloState {
    /// A helicopter on the pad at `base`, launched on `target`, with full fuel.
    pub fn new(target: EntityId, base: [f32; 2], cfg: &HeloConfig) -> Self {
        Self {
            phase: HeloPhase::Launch,
            target,
            base,
            vel: [0.0, 0.0],
            climb_mps: 0.0,
            bank_rad: 0.0,
            pitch_rad: 0.0,
            spot: base,
            visual: false,
            last_visual: None,
            search_center: base,
            search_radius_m: SEARCH_START_RADIUS_M,
            sweep_rad: 0.0,
            fuel_s: cfg.endurance_s,
        }
    }

    /// In the air (not yet landed).
    pub fn airborne(&self) -> bool {
        self.phase != HeloPhase::Landed
    }

    /// On a job (can give visual): transit, track or search.
    pub fn on_station(&self) -> bool {
        matches!(
            self.phase,
            HeloPhase::Transit | HeloPhase::Track | HeloPhase::Search
        )
    }

    /// Spotlight on (on station).
    pub fn spotlight_on(&self) -> bool {
        self.on_station()
    }

    /// Radius of the spotlight's circle on the ground at `alt_m` and horizontal distance
    /// `ground_m` from the helicopter to the aim point, m.
    pub fn beam_radius_m(alt_m: f32, ground_m: f32, cfg: &HeloConfig) -> f32 {
        let (s, c) = sin_cos(cfg.spotlight_half_angle_rad);
        let slant = (alt_m * alt_m + ground_m * ground_m).sqrt();
        slant * s / c.max(1e-3)
    }

    /// Whether the car at `p` can be seen from `body` this `phase` (range and spotlight; the
    /// caller checks line of sight). `has_los` is the world's line of sight from altitude.
    pub fn can_see(
        &self,
        body: &UnitBody,
        p: [f32; 2],
        has_los: bool,
        phase: DayPhase,
        cfg: &HeloConfig,
    ) -> bool {
        if !self.on_station() || !has_los || dist(body.pos, p) > cfg.los_range_m.get(phase) {
            return false;
        }
        if !cfg.spotlight_required.get(phase) {
            return true;
        }
        let r = Self::beam_radius_m(body.alt_m, dist(body.pos, self.spot), cfg);
        dist(self.spot, p) <= r
    }

    /// Record this think step's visual.
    pub fn set_visual(&mut self, visual: bool, now: SimTime) {
        self.visual = visual;
        if visual {
            self.last_visual = Some(now);
        }
    }

    /// Fuel left only covers the flight home from `pos` plus the reserve.
    pub fn bingo(&self, pos: [f32; 2], cfg: &HeloConfig) -> bool {
        self.fuel_s <= Self::time_home_s(pos, self.base, cfg) + cfg.reserve_s
    }

    /// Estimated flight time from `pos` to landed at `base`: cruise, speeding up and slowing
    /// down (≈ v/a extra), and the descent (the climb law slows near the ground, ≈ 2× the
    /// straight descent time).
    pub fn time_home_s(pos: [f32; 2], base: [f32; 2], cfg: &HeloConfig) -> f32 {
        let v = cfg.speed_mps.max(1.0);
        dist(pos, base) / v
            + v / cfg.accel_mps2.max(0.1)
            + 2.0 * cfg.altitude_m / cfg.climb_mps.max(0.1)
    }

    /// Seconds without visual at `now` (`f32::INFINITY` if it never had it).
    pub fn lost_s(&self, now: SimTime) -> f32 {
        self.last_visual
            .map_or(f32::INFINITY, |t| now.since(t).max(0.0))
    }
}

/// Velocity that arrives at `goal` from `pos` and stops there, m/s.
fn arrive(pos: [f32; 2], goal: [f32; 2], cfg: &HeloConfig) -> [f32; 2] {
    let to = sub(goal, pos);
    let d = len(to);
    if d < 1e-3 {
        return [0.0, 0.0];
    }
    // Brake at 70 % of the acceleration limit so the stop is reachable.
    let v = (2.0 * 0.7 * cfg.accel_mps2 * d).sqrt().min(cfg.speed_mps);
    scale(to, v / d)
}

/// Velocity that circles `center` counter-clockwise at `radius_m` (moving with `center_vel`).
fn orbit(
    pos: [f32; 2],
    center: [f32; 2],
    center_vel: [f32; 2],
    radius_m: f32,
    cfg: &HeloConfig,
) -> [f32; 2] {
    let r = sub(pos, center);
    let d = len(r);
    if d < 1.0 {
        // Directly overhead: move out along any direction.
        return add(center_vel, [cfg.orbit_speed_mps, 0.0]);
    }
    let out = scale(r, 1.0 / d);
    let tangent = perp_left(out);
    // Radial correction toward the circle, gentle enough not to spiral.
    let radial = ((radius_m - d) * 0.3).clamp(-15.0, 15.0);
    add(
        center_vel,
        add(scale(tangent, cfg.orbit_speed_mps), scale(out, radial)),
    )
}

/// Turn `from` toward `to` by at most `max_rad` (unit vectors).
fn slew_dir(from: [f32; 2], to: [f32; 2], max_rad: f32) -> [f32; 2] {
    let ang = atan2(cross(from, to), dot(from, to));
    let step = ang.clamp(-max_rad, max_rad);
    let (s, c) = sin_cos(step);
    normalize([from[0] * c - from[1] * s, from[0] * s + from[1] * c])
}

/// Flight model: chase `v_des` (horizontal, m/s) and `alt_goal_m` within the limits for `dt`.
/// `look_at` is where the nose points while hovering.
pub fn fly(
    body: &mut UnitBody,
    st: &mut HeloState,
    v_des: [f32; 2],
    alt_goal_m: f32,
    look_at: [f32; 2],
    cfg: &HeloConfig,
    dt: f32,
) {
    let dt = dt.max(1e-4);
    // Speed limit on the request.
    let vd = len(v_des);
    let v_des = if vd > cfg.speed_mps {
        scale(v_des, cfg.speed_mps / vd)
    } else {
        v_des
    };
    // Acceleration limit.
    let dv = sub(v_des, st.vel);
    let need = len(dv);
    let a = if need > cfg.accel_mps2 * dt {
        scale(dv, cfg.accel_mps2 / need)
    } else {
        scale(dv, 1.0 / dt)
    };
    st.vel = add(st.vel, scale(a, dt));
    body.pos = add(body.pos, scale(st.vel, dt));
    body.speed = len(st.vel);
    // Nose along the velocity, or at the target while hovering.
    let want = if body.speed > HOVER_SPEED_MPS {
        scale(st.vel, 1.0 / body.speed)
    } else {
        let to = sub(look_at, body.pos);
        if len(to) > 1.0 {
            normalize(to)
        } else {
            body.heading
        }
    };
    body.heading = slew_dir(body.heading, want, cfg.yaw_rate_rad_s * dt);
    // Attitude from the acceleration in the body frame.
    let a_long = dot(a, body.heading);
    let a_left = cross(body.heading, a);
    let k = (dt / cfg.attitude_tau_s.max(1e-3)).min(1.0);
    st.bank_rad += (atan(-a_left / G_MPS2) - st.bank_rad) * k;
    st.pitch_rad += (atan(-a_long / G_MPS2) - st.pitch_rad) * k;
    body.lat_g = a_left.abs() / G_MPS2;
    // Altitude: proportional climb, rate-limited.
    st.climb_mps = ((alt_goal_m - body.alt_m) * 0.5).clamp(-cfg.climb_mps, cfg.climb_mps);
    body.alt_m = (body.alt_m + st.climb_mps * dt).max(0.0);
}

/// Move the spotlight toward `aim` at the slew rate.
fn slew_spot(body: &UnitBody, st: &mut HeloState, aim: [f32; 2], cfg: &HeloConfig, dt: f32) {
    let to = sub(aim, st.spot);
    let d = len(to);
    let ground = dist(body.pos, st.spot);
    let slant = (body.alt_m * body.alt_m + ground * ground).sqrt();
    let max = cfg.spotlight_slew_rad_s * slant.max(1.0) * dt;
    st.spot = if d <= max {
        aim
    } else {
        add(st.spot, scale(to, max / d))
    };
}

/// One tick of behaviour + flight. `target` is `None` once the job is over (pursuit ended,
/// called off, out of service): the helicopter returns. Burns fuel while airborne.
pub fn step(
    body: &mut UnitBody,
    st: &mut HeloState,
    target: Option<HeloTarget>,
    now: SimTime,
    cfg: &HeloConfig,
    dt: f32,
) {
    if st.phase == HeloPhase::Landed {
        st.vel = [0.0, 0.0];
        body.speed = 0.0;
        return;
    }
    st.fuel_s = (st.fuel_s - dt).max(0.0);
    let target = match target {
        Some(t) if st.phase != HeloPhase::Return => t,
        _ => {
            st.phase = HeloPhase::Return;
            HeloTarget {
                pos: st.base,
                vel: [0.0, 0.0],
            }
        }
    };
    // Phase transitions.
    let lost = st.lost_s(now);
    match st.phase {
        HeloPhase::Launch if body.alt_m >= cfg.launch_climb_m - 1.0 => {
            st.phase = HeloPhase::Transit
        }
        HeloPhase::Transit if st.visual => st.phase = HeloPhase::Track,
        HeloPhase::Transit if dist(body.pos, target.pos) < cfg.orbit_radius_m => {
            // Arrived without visual (e.g. the beam missed at night): search from here.
            start_search(st, target.pos);
        }
        HeloPhase::Track if !st.visual && lost > cfg.search_after_s => start_search(st, target.pos),
        HeloPhase::Search if st.visual => st.phase = HeloPhase::Track,
        _ => {}
    }
    // Desired motion and spotlight aim per phase.
    let lead = add(target.pos, scale(target.vel, cfg.lead_s));
    let (v_des, alt, aim) = match st.phase {
        HeloPhase::Launch => ([0.0, 0.0], cfg.launch_climb_m, target.pos),
        HeloPhase::Transit => (arrive(body.pos, lead, cfg), cfg.altitude_m, target.pos),
        HeloPhase::Track => {
            let tv = len(target.vel);
            let v = if tv > cfg.orbit_max_target_mps {
                let behind = sub(lead, scale(target.vel, cfg.trail_distance_m / tv));
                add(target.vel, arrive(body.pos, behind, cfg))
            } else {
                orbit(body.pos, lead, target.vel, cfg.orbit_radius_m, cfg)
            };
            (v, cfg.altitude_m, target.pos)
        }
        HeloPhase::Search => {
            // Fresh sightings by ground units move the search with them.
            if dist(st.search_center, target.pos) > 1.0 {
                st.search_center = target.pos;
            }
            st.search_radius_m =
                (st.search_radius_m + cfg.search_growth_mps * dt).min(cfg.search_max_radius_m);
            st.sweep_rad += cfg.search_sweep_rad_s * dt;
            let (s, c) = sin_cos(st.sweep_rad);
            let aim = add(st.search_center, scale([c, s], st.search_radius_m));
            let radius = cfg.orbit_radius_m.max(st.search_radius_m * 0.5);
            let v = orbit(body.pos, st.search_center, [0.0, 0.0], radius, cfg);
            (v, cfg.altitude_m, aim)
        }
        HeloPhase::Return | HeloPhase::Landed => {
            let near = dist(body.pos, st.base) <= cfg.landing_radius_m;
            let alt = if near { 0.0 } else { cfg.altitude_m };
            (arrive(body.pos, st.base, cfg), alt, body.pos)
        }
    };
    fly(body, st, v_des, alt, target.pos, cfg, dt);
    if st.spotlight_on() {
        slew_spot(body, st, aim, cfg, dt);
    } else {
        st.spot = body.pos;
    }
    if st.phase == HeloPhase::Return
        && dist(body.pos, st.base) <= cfg.landing_radius_m
        && body.alt_m < 0.5
        && body.speed < 1.0
    {
        st.phase = HeloPhase::Landed;
        body.alt_m = 0.0;
        st.vel = [0.0, 0.0];
        body.speed = 0.0;
    }
}

fn start_search(st: &mut HeloState, center: [f32; 2]) {
    st.phase = HeloPhase::Search;
    st.search_center = center;
    st.search_radius_m = SEARCH_START_RADIUS_M;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PoliceConfig;

    fn cfg() -> HeloConfig {
        PoliceConfig::builtin().tools.helicopter
    }

    fn body(pos: [f32; 2]) -> UnitBody {
        UnitBody {
            pos,
            heading: [0.0, 1.0],
            speed: 0.0,
            lat_g: 0.0,
            alt_m: 0.0,
        }
    }

    const DT: f32 = 1.0 / 60.0;

    /// Fly `secs` after a car moving `car_vel` from `car0`, always visible.
    fn chase(
        b: &mut UnitBody,
        st: &mut HeloState,
        car0: [f32; 2],
        car_vel: [f32; 2],
        secs: f32,
        t0: f32,
    ) -> [f32; 2] {
        let c = cfg();
        let n = (secs / DT) as usize;
        let mut car = car0;
        for k in 0..n {
            let now = SimTime::from_secs(t0 + k as f32 * DT);
            car = add(car, scale(car_vel, DT));
            st.set_visual(st.on_station(), now);
            step(
                b,
                st,
                Some(HeloTarget {
                    pos: car,
                    vel: car_vel,
                }),
                now,
                &c,
                DT,
            );
        }
        car
    }

    #[test]
    fn launches_vertically_then_transits_within_limits() {
        let c = cfg();
        let mut b = body([0.0, 0.0]);
        let mut st = HeloState::new(EntityId(1), [0.0, 0.0], &c);
        let target = HeloTarget {
            pos: [2000.0, 0.0],
            vel: [0.0, 0.0],
        };
        let mut prev_v = st.vel;
        for k in 0..(60.0 / DT) as usize {
            let now = SimTime::from_secs(k as f32 * DT);
            step(&mut b, &mut st, Some(target), now, &c, DT);
            let dv = len(sub(st.vel, prev_v)) / DT;
            assert!(dv <= c.accel_mps2 * 1.001, "accel {dv}");
            assert!(b.speed <= c.speed_mps * 1.001);
            if st.phase == HeloPhase::Launch {
                assert!(b.speed < 1e-3, "moved before climbing out");
            }
            prev_v = st.vel;
        }
        assert!(b.alt_m > c.altitude_m * 0.9, "alt {}", b.alt_m);
        assert!(b.pos[0] > 1000.0, "pos {:?}", b.pos);
        assert!(st.fuel_s < c.endurance_s - 59.0);
    }

    #[test]
    fn orbits_a_parked_car_at_the_orbit_radius() {
        let c = cfg();
        let mut b = body([300.0, 0.0]);
        b.alt_m = c.altitude_m;
        let mut st = HeloState::new(EntityId(1), [300.0, 0.0], &c);
        st.phase = HeloPhase::Transit;
        chase(&mut b, &mut st, [0.0, 0.0], [0.0, 0.0], 60.0, 0.0);
        assert_eq!(st.phase, HeloPhase::Track);
        let mut min_r = f32::INFINITY;
        let mut max_r = 0.0f32;
        for k in 0..(30.0 / DT) as usize {
            chase(
                &mut b,
                &mut st,
                [0.0, 0.0],
                [0.0, 0.0],
                DT,
                60.0 + k as f32 * DT,
            );
            let r = len(b.pos);
            min_r = min_r.min(r);
            max_r = max_r.max(r);
        }
        let r0 = c.orbit_radius_m;
        assert!(min_r > r0 * 0.8 && max_r < r0 * 1.2, "{min_r}..{max_r}");
        // Banked into the (left-hand) turn.
        assert!(st.bank_rad < -0.1, "bank {}", st.bank_rad);
    }

    #[test]
    fn trails_a_fast_car_and_a_faster_one_gets_away() {
        let c = cfg();
        let mut b = body([0.0, -150.0]);
        b.alt_m = c.altitude_m;
        let mut st = HeloState::new(EntityId(1), [0.0, 0.0], &c);
        st.phase = HeloPhase::Track;
        // 35 m/s (126 km/h): it keeps up.
        let car = chase(&mut b, &mut st, [0.0, 0.0], [0.0, 35.0], 60.0, 0.0);
        let gap = dist(b.pos, car);
        assert!(gap < c.trail_distance_m + 80.0, "gap {gap}");
        // 70 m/s (252 km/h) is beyond its top speed.
        let mut b2 = b;
        let mut st2 = st;
        let car2 = chase(&mut b2, &mut st2, car, [0.0, 70.0], 30.0, 60.0);
        assert!(dist(b2.pos, car2) > c.los_range_m.day, "kept up at 70 m/s");
    }

    #[test]
    fn night_visual_needs_the_beam_on_the_car() {
        let c = cfg();
        let mut b = body([0.0, 0.0]);
        b.alt_m = c.altitude_m;
        let mut st = HeloState::new(EntityId(1), [0.0, 0.0], &c);
        st.phase = HeloPhase::Track;
        st.spot = [0.0, 0.0];
        let car = [80.0, 0.0];
        assert!(st.can_see(&b, car, true, DayPhase::Day, &c));
        assert!(!st.can_see(&b, car, true, DayPhase::Night, &c));
        assert!(!st.can_see(&b, car, false, DayPhase::Day, &c));
        st.spot = [75.0, 0.0];
        assert!(st.can_see(&b, car, true, DayPhase::Night, &c));
        // The beam slews, it does not jump.
        st.spot = [0.0, 0.0];
        slew_spot(&b, &mut st, car, &c, 0.1);
        assert!(st.spot[0] > 0.0 && st.spot[0] < 80.0, "{:?}", st.spot);
    }

    #[test]
    fn search_after_losing_visual_and_reacquire() {
        let c = cfg();
        let mut b = body([150.0, 0.0]);
        b.alt_m = c.altitude_m;
        let mut st = HeloState::new(EntityId(1), [0.0, 0.0], &c);
        st.phase = HeloPhase::Track;
        st.set_visual(true, SimTime::ZERO);
        let t = HeloTarget {
            pos: [0.0, 0.0],
            vel: [0.0, 0.0],
        };
        st.set_visual(false, SimTime::from_secs(0.1));
        for k in 0..(10.0 / DT) as usize {
            let now = SimTime::from_secs(0.1 + k as f32 * DT);
            step(&mut b, &mut st, Some(t), now, &c, DT);
        }
        assert_eq!(st.phase, HeloPhase::Search);
        assert!(st.search_radius_m > SEARCH_START_RADIUS_M);
        st.set_visual(true, SimTime::from_secs(10.2));
        step(&mut b, &mut st, Some(t), SimTime::from_secs(10.2), &c, DT);
        assert_eq!(st.phase, HeloPhase::Track);
    }

    #[test]
    fn bingo_fuel_returns_and_lands_at_the_pad() {
        let c = cfg();
        let base = [0.0, 0.0];
        let mut b = body([1500.0, 0.0]);
        b.alt_m = c.altitude_m;
        let mut st = HeloState::new(EntityId(1), base, &c);
        st.phase = HeloPhase::Track;
        st.fuel_s = HeloState::time_home_s(b.pos, base, &c) + c.reserve_s;
        assert!(st.bingo(b.pos, &c));
        let mut landed = false;
        for k in 0..(180.0 / DT) as usize {
            let now = SimTime::from_secs(k as f32 * DT);
            step(&mut b, &mut st, None, now, &c, DT);
            if st.phase == HeloPhase::Landed {
                landed = true;
                break;
            }
        }
        assert!(
            landed,
            "phase {:?} at {:?} alt {}",
            st.phase, b.pos, b.alt_m
        );
        assert!(dist(b.pos, base) <= c.landing_radius_m);
        assert!(st.fuel_s > 0.0, "ran dry on the way home");
    }
}
