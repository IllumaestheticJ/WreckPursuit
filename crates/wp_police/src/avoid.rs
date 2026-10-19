//! Obstacle awareness for ground units.
//!
//! Every tick a ground unit looks along its own track for vehicles in its lane corridor
//! (players, racers, traffic, other units). The nearest one caps the unit's speed so it can
//! stop [`AvoidConfig::standstill_gap_m`] behind it. A unit stuck behind a stopped vehicle for
//! a while pulls into the other lane and passes when that lane is clear. Spawning uses
//! [`spawn_clear`] so units never appear on top of, or right in front of, another vehicle.
//!
//! `wp_sim` reports every vehicle with [`crate::PoliceSystem::set_obstacles`]. Without that
//! (headless runs) units see the racers and each other.

use serde::{Deserialize, Serialize};
use wp_contracts::EntityId;

use crate::config::AvoidConfig;
use crate::events::UnitId;
use crate::graph::{dist2, dot, normalize, sub};
use crate::unit::{lane_point, Track};

/// Half length of a police car, m (bumper to centre).
pub const UNIT_HALF_LENGTH_M: f32 = 2.45;
/// Half width of a police car, m.
pub const UNIT_HALF_WIDTH_M: f32 = 0.95;

/// A vehicle police units must not drive into, as reported by `wp_sim`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ObstacleView {
    /// Entity, if the vehicle has one (kinematic traffic has none).
    pub entity: Option<EntityId>,
    /// Centre position, m.
    pub pos: [f32; 3],
    /// Velocity, m/s.
    pub vel: [f32; 3],
    /// Unit forward direction (XY).
    pub forward: [f32; 2],
    /// Half length, m.
    pub half_length_m: f32,
    /// Half width, m.
    pub half_width_m: f32,
}

impl ObstacleView {
    /// A car-sized obstacle (4.6 m × 1.9 m) at `pos` moving with `vel`, facing `forward`.
    pub fn car(entity: Option<EntityId>, pos: [f32; 3], vel: [f32; 3], forward: [f32; 2]) -> Self {
        Self {
            entity,
            pos,
            vel,
            forward,
            half_length_m: 2.3,
            half_width_m: 0.95,
        }
    }
}

/// An obstacle in the police system's own 2D form.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Obstacle {
    /// Entity, if any (a unit skips its own).
    pub(crate) entity: Option<EntityId>,
    /// Internally driven unit, if this is one (a unit skips itself).
    pub(crate) unit: Option<UnitId>,
    pub(crate) pos: [f32; 2],
    pub(crate) vel: [f32; 2],
    pub(crate) fwd: [f32; 2],
    pub(crate) half_len: f32,
    pub(crate) half_wid: f32,
}

impl Obstacle {
    pub(crate) fn from_view(v: &ObstacleView) -> Self {
        let f = if v.forward == [0.0, 0.0] {
            [0.0, 1.0]
        } else {
            normalize(v.forward)
        };
        Self {
            entity: v.entity,
            unit: None,
            pos: [v.pos[0], v.pos[1]],
            vel: [v.vel[0], v.vel[1]],
            fwd: f,
            half_len: v.half_length_m.max(0.1),
            half_wid: v.half_width_m.max(0.1),
        }
    }

    /// A car-sized obstacle from a position and velocity (heading taken from the velocity).
    pub(crate) fn car(
        entity: Option<EntityId>,
        unit: Option<UnitId>,
        pos: [f32; 2],
        vel: [f32; 2],
        heading: [f32; 2],
    ) -> Self {
        Self {
            entity,
            unit,
            pos,
            vel,
            fwd: normalize(heading),
            half_len: 2.3,
            half_wid: 0.95,
        }
    }

    fn speed(&self) -> f32 {
        dot(self.vel, self.vel).sqrt()
    }
}

/// Per-unit avoidance state.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AvoidState {
    /// Speed cap from the vehicle ahead, m/s (`f32::INFINITY` when the lane is clear).
    pub limit_mps: f32,
    /// Time spent blocked by a stopped vehicle, s.
    pub blocked_s: f32,
    /// Lane offset used while passing, m (replaces the command's offset).
    pub pass_offset_m: Option<f32>,
    /// Passing ends once the unit reaches this arc length on its track, m.
    pub pass_until_s: f32,
}

impl Default for AvoidState {
    fn default() -> Self {
        Self {
            limit_mps: f32::INFINITY,
            blocked_s: 0.0,
            pass_offset_m: None,
            pass_until_s: 0.0,
        }
    }
}

/// The nearest vehicle in a lane corridor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Blocker {
    /// Bumper-to-bumper gap along the track, m (negative = overlapping).
    pub(crate) gap_m: f32,
    /// Its speed along the track, m/s (negative = oncoming).
    pub(crate) along_mps: f32,
    /// Its total speed, m/s.
    pub(crate) speed_mps: f32,
    /// Arc length of its far end on the track, m.
    pub(crate) far_s: f32,
}

/// What a unit is, for the scan.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Me {
    pub(crate) unit: UnitId,
    pub(crate) entity: Option<EntityId>,
    pub(crate) pos: [f32; 2],
    pub(crate) heading: [f32; 2],
    pub(crate) track_s: f32,
    /// An entity the unit means to touch (PIT / box-in target).
    pub(crate) ignore: Option<EntityId>,
}

/// Nearest obstacle within `horizon_m` ahead on `track`, in the corridor around the lane
/// `offset_m` right of the centreline. `bumper` adds the short check straight ahead of the
/// body (off when testing whether another lane is free).
pub(crate) fn scan(
    me: &Me,
    track: &Track,
    offset_m: f32,
    horizon_m: f32,
    obstacles: &[Obstacle],
    cfg: &AvoidConfig,
    bumper: bool,
) -> Option<Blocker> {
    if !track.is_drivable() {
        return None;
    }
    let reach2 = (horizon_m + 10.0) * (horizon_m + 10.0);
    let mut best: Option<Blocker> = None;
    for o in obstacles {
        if o.unit == Some(me.unit)
            || (o.entity.is_some() && (o.entity == me.entity || o.entity == me.ignore))
        {
            continue;
        }
        let rel = sub(o.pos, me.pos);
        if dot(rel, rel) > reach2 {
            continue;
        }
        // Clearly behind the unit: not its problem.
        if dot(rel, me.heading) < -(o.half_len + UNIT_HALF_LENGTH_M) {
            continue;
        }
        // Bumper check in the unit's own frame: catches what the lane corridor misses while
        // the body is not on its track yet (turning around, cutting a corner).
        let along = dot(rel, me.heading);
        let side = [me.heading[1], -me.heading[0]];
        let ext_side =
            o.half_len * dot(o.fwd, side).abs() + o.half_wid * dot(o.fwd, me.heading).abs();
        let ext_fwd =
            o.half_len * dot(o.fwd, me.heading).abs() + o.half_wid * dot(o.fwd, side).abs();
        let bumper_gap = along - UNIT_HALF_LENGTH_M - ext_fwd;
        if bumper
            && along > 0.0
            && bumper_gap < cfg.bumper_m
            && dot(rel, side).abs() < UNIT_HALF_WIDTH_M + ext_side + cfg.lateral_margin_m
        {
            let b = Blocker {
                gap_m: bumper_gap,
                along_mps: dot(o.vel, me.heading),
                speed_mps: o.speed(),
                far_s: me.track_s + along + ext_fwd,
            };
            if best.is_none_or(|x| b.gap_m < x.gap_m) {
                best = Some(b);
            }
            continue;
        }
        let (s_o, _) = track.project_ahead(o.pos, me.track_s, 2.0, horizon_m + 5.0);
        if s_o > me.track_s + horizon_m + o.half_len {
            continue;
        }
        let t = normalize(sub(track.point_at(s_o + 1.0), track.point_at(s_o - 1.0)));
        let n = [t[1], -t[0]];
        let c = lane_point(track, s_o, offset_m);
        let lat = dot(sub(o.pos, c), n).abs();
        let ext_n = o.half_len * dot(o.fwd, n).abs() + o.half_wid * dot(o.fwd, t).abs();
        if lat > UNIT_HALF_WIDTH_M + ext_n + cfg.lateral_margin_m {
            continue;
        }
        let ext_t = o.half_len * dot(o.fwd, t).abs() + o.half_wid * dot(o.fwd, n).abs();
        let gap = s_o - me.track_s - UNIT_HALF_LENGTH_M - ext_t;
        // Already past its far end (the projection can lag the body by a little).
        if gap < -(2.0 * ext_t + 2.0 * UNIT_HALF_LENGTH_M) {
            continue;
        }
        let b = Blocker {
            gap_m: gap,
            along_mps: dot(o.vel, t),
            speed_mps: o.speed(),
            far_s: s_o + ext_t,
        };
        if best.is_none_or(|x| b.gap_m < x.gap_m) {
            best = Some(b);
        }
    }
    best
}

/// Highest speed that still stops `standstill_gap_m` behind `b`, m/s.
pub(crate) fn follow_speed(b: &Blocker, v: f32, cfg: &AvoidConfig) -> f32 {
    let room = b.gap_m - cfg.standstill_gap_m - v * cfg.reaction_s;
    if room <= 0.0 {
        return 0.0;
    }
    let lead = b.along_mps.max(0.0);
    (lead * lead + 2.0 * cfg.decel_mps2 * room).sqrt()
}

/// How far ahead to look at speed `v`, m.
pub(crate) fn horizon(v: f32, cfg: &AvoidConfig) -> f32 {
    (cfg.standstill_gap_m + v * cfg.reaction_s + v * v / (2.0 * cfg.decel_mps2) + 15.0)
        .clamp(cfg.min_horizon_m, cfg.max_horizon_m)
}

/// Update `st` for one tick of `dt` and return the lane offset to steer for.
///
/// `base_offset_m` is the command's lane offset, `v` the unit's speed and `siren` whether it
/// runs lights (it then passes sooner).
#[allow(clippy::too_many_arguments)]
pub(crate) fn update(
    st: &mut AvoidState,
    me: &Me,
    track: &Track,
    base_offset_m: f32,
    v: f32,
    siren: bool,
    obstacles: &[Obstacle],
    cfg: &AvoidConfig,
    dt: f32,
) -> f32 {
    if st.pass_offset_m.is_some() && me.track_s > st.pass_until_s {
        st.pass_offset_m = None;
    }
    let offset = st.pass_offset_m.unwrap_or(base_offset_m);
    let h = horizon(v, cfg);
    let blocker = scan(me, track, offset, h, obstacles, cfg, true);
    st.limit_mps = blocker.map_or(f32::INFINITY, |b| follow_speed(&b, v, cfg));
    if st.pass_offset_m.is_some() {
        // Pulling out from right behind the stopped vehicle: it stays in the bumper box until
        // the car has turned, so creep while there is room.
        let creep = if blocker.is_none_or(|b| b.gap_m > cfg.pass_creep_gap_m) {
            cfg.pass_creep_mps
        } else {
            0.0
        };
        st.limit_mps = st.limit_mps.max(creep).min(cfg.pass_speed_mps);
        st.blocked_s = 0.0;
        return offset;
    }
    let stuck = blocker.filter(|b| b.speed_mps < cfg.stopped_mps && b.gap_m < cfg.pass_gap_m);
    let Some(b) = stuck else {
        st.blocked_s = 0.0;
        return offset;
    };
    st.blocked_s += dt;
    let wait = if siren {
        cfg.pass_wait_siren_s
    } else {
        cfg.pass_wait_s
    };
    if st.blocked_s < wait {
        return offset;
    }
    // Other lane (or, for a vehicle straddling the centreline, a little further left): clear
    // past the blocker, and no oncoming vehicle close.
    let need = (b.far_s - me.track_s) + cfg.pass_clear_m;
    for shift in [cfg.pass_shift_m, cfg.pass_shift_m + cfg.pass_extra_shift_m] {
        let alt = base_offset_m - shift;
        let clear = match scan(
            me,
            track,
            alt,
            need + cfg.oncoming_clear_m,
            obstacles,
            cfg,
            false,
        ) {
            None => true,
            Some(o) => {
                o.gap_m > need && (o.along_mps > -1.0 || o.gap_m > need + cfg.oncoming_clear_m)
            }
        };
        if clear {
            st.pass_offset_m = Some(alt);
            st.pass_until_s = b.far_s + UNIT_HALF_LENGTH_M + cfg.pass_clear_m;
            st.blocked_s = 0.0;
            return alt;
        }
    }
    offset
}

/// Whether a unit may spawn at `pos`: at least `spawn_clear_m` from every obstacle and out of
/// every moving vehicle's path for the next `spawn_path_s` seconds.
pub(crate) fn spawn_clear(pos: [f32; 2], obstacles: &[Obstacle], cfg: &AvoidConfig) -> bool {
    let r2 = cfg.spawn_clear_m * cfg.spawn_clear_m;
    obstacles.iter().all(|o| {
        if dist2(o.pos, pos) < r2 {
            return false;
        }
        let v = o.speed();
        if v < 1.0 {
            return true;
        }
        let dir = [o.vel[0] / v, o.vel[1] / v];
        let rel = sub(pos, o.pos);
        let along = dot(rel, dir);
        let lat = (rel[0] * dir[1] - rel[1] * dir[0]).abs();
        !(along > 0.0
            && along < v * cfg.spawn_path_s + cfg.spawn_clear_m
            && lat < cfg.spawn_path_half_width_m)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PoliceConfig;

    fn cfg() -> AvoidConfig {
        PoliceConfig::builtin().pursuit.avoid
    }

    fn me(track_s: f32, pos: [f32; 2]) -> Me {
        Me {
            unit: UnitId(1),
            entity: Some(EntityId(10)),
            pos,
            heading: [0.0, 1.0],
            track_s,
            ignore: None,
        }
    }

    fn straight() -> Track {
        Track::new(vec![[0.0, 0.0], [0.0, 400.0]])
    }

    fn parked(entity: u32, pos: [f32; 2]) -> Obstacle {
        Obstacle::car(Some(EntityId(entity)), None, pos, [0.0, 0.0], [0.0, 1.0])
    }

    #[test]
    fn stops_behind_a_parked_car_in_lane() {
        let c = cfg();
        // Lane is 1.75 m right of the centreline (x = +1.75 heading +Y).
        let obs = [parked(2, [1.75, 60.0])];
        let m = me(10.0, [1.75, 10.0]);
        let b = scan(&m, &straight(), 1.75, 80.0, &obs, &c, true).expect("blocker");
        assert!((b.gap_m - (50.0 - 2.45 - 2.3)).abs() < 0.1, "{b:?}");
        // Close: must stop.
        let near = [parked(2, [1.75, 16.0])];
        let b = scan(&m, &straight(), 1.75, 80.0, &near, &c, true).unwrap();
        assert_eq!(follow_speed(&b, 10.0, &c), 0.0);
        // Far: may still drive, but slower than flat out.
        let b = scan(&m, &straight(), 1.75, 80.0, &obs, &c, true).unwrap();
        let v = follow_speed(&b, 15.0, &c);
        assert!(v > 5.0 && v < 25.0, "{v}");
    }

    #[test]
    fn ignores_other_lane_behind_and_self() {
        let c = cfg();
        let m = me(50.0, [1.75, 50.0]);
        let obs = [
            parked(2, [-1.75, 70.0]), // oncoming lane
            parked(3, [1.75, 30.0]),  // behind
            parked(10, [1.75, 55.0]), // itself
        ];
        assert!(scan(&m, &straight(), 1.75, 80.0, &obs, &c, true).is_none());
    }

    #[test]
    fn follows_a_moving_leader() {
        let c = cfg();
        let m = me(0.0, [1.75, 0.0]);
        let lead = [Obstacle::car(
            Some(EntityId(2)),
            None,
            [1.75, 30.0],
            [0.0, 12.0],
            [0.0, 1.0],
        )];
        let b = scan(&m, &straight(), 1.75, 80.0, &lead, &c, true).unwrap();
        let v = follow_speed(&b, 12.0, &c);
        assert!(
            v >= 12.0,
            "a leader at our speed with room must not force braking: {v}"
        );
    }

    #[test]
    fn crossing_car_blocks() {
        let c = cfg();
        let m = me(0.0, [1.75, 0.0]);
        // Broadside across the lane 20 m ahead.
        let obs = [Obstacle::car(
            Some(EntityId(2)),
            None,
            [0.0, 20.0],
            [0.0, 0.0],
            [1.0, 0.0],
        )];
        assert!(scan(&m, &straight(), 1.75, 80.0, &obs, &c, true).is_some());
    }

    #[test]
    fn passes_a_stopped_car_when_the_other_lane_is_clear() {
        let c = cfg();
        // Stopped right behind it (gap 2.75 m).
        let obs = [parked(2, [1.75, 7.5])];
        let m = me(0.0, [1.75, 0.0]);
        let mut st = AvoidState::default();
        let dt = 1.0 / 240.0;
        let mut off = 1.75;
        for _ in 0..(240.0 * (c.pass_wait_siren_s + 0.5)) as usize {
            off = update(&mut st, &m, &straight(), 1.75, 0.0, true, &obs, &c, dt);
        }
        assert!(off < 0.0, "should pull into the other lane, offset {off}");
        assert!(st.limit_mps > 0.0);

        // Oncoming car in the other lane: wait.
        let obs2 = [
            parked(2, [1.75, 7.5]),
            Obstacle::car(
                Some(EntityId(3)),
                None,
                [-1.75, 60.0],
                [0.0, -10.0],
                [0.0, -1.0],
            ),
        ];
        let mut st = AvoidState::default();
        for _ in 0..(240.0 * (c.pass_wait_siren_s + 0.5)) as usize {
            off = update(&mut st, &m, &straight(), 1.75, 0.0, true, &obs2, &c, dt);
        }
        assert_eq!(off, 1.75);
        assert_eq!(st.limit_mps, 0.0);
    }

    #[test]
    fn spawn_clearance() {
        let c = cfg();
        let racer = Obstacle::car(Some(EntityId(1)), None, [0.0, 0.0], [0.0, 22.0], [0.0, 1.0]);
        assert!(!spawn_clear([0.0, 5.0], &[racer], &c), "on top");
        assert!(!spawn_clear([0.0, 60.0], &[racer], &c), "in its path");
        assert!(spawn_clear([0.0, -60.0], &[racer], &c), "behind is fine");
        assert!(
            spawn_clear([30.0, 60.0], &[racer], &c),
            "parallel street is fine"
        );
    }
}
