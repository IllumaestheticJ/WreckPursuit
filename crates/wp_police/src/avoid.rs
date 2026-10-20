//! Obstacle awareness for ground units.
//!
//! Every tick a ground unit looks along its own track for vehicles in its lane corridor
//! (players, racers, traffic, other units). The nearest one caps the unit's speed so it can
//! stop [`AvoidConfig::standstill_gap_m`] behind it. A unit stuck behind a stopped vehicle for
//! a while pulls into the other lane and passes when that lane is clear. A parked car poking
//! into the lane (kerbside on a curve, knocked askew) is not a reason to stop: the unit nudges
//! up to [`MAX_PARKED_NUDGE_M`] away from it within its lane. Spawning uses [`spawn_clear`] so
//! units never appear on top of, or right in front of, another vehicle.
//!
//! `wp_sim` reports every vehicle with [`crate::PoliceSystem::set_obstacles`]. Without that
//! (headless runs) units see the racers and each other.

use serde::{Deserialize, Serialize};
use wp_contracts::{EntityId, SignalState};

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
    /// A parked car at the kerb (units nudge past it instead of queueing behind it; it never
    /// blocks a spawn or a junction).
    #[serde(default)]
    pub parked: bool,
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
            parked: false,
        }
    }

    /// A parked car (box with these half extents) at `pos`, facing `forward`.
    pub fn parked(pos: [f32; 3], forward: [f32; 2], half_length_m: f32, half_width_m: f32) -> Self {
        Self {
            entity: None,
            pos,
            vel: [0.0; 3],
            forward,
            half_length_m,
            half_width_m,
            parked: true,
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
    /// A parked car (see [`ObstacleView::parked`]).
    pub(crate) parked: bool,
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
            parked: v.parked,
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
            parked: false,
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
    /// Next junction on the track, if within look-ahead.
    pub junction: Option<JunctionAhead>,
    /// Speed cap from traffic rules at that junction, m/s (`f32::INFINITY` = none).
    pub junction_limit_mps: f32,
    /// Time spent waiting for cross traffic at the junction, s.
    pub yield_s: f32,
    /// Time spent stopped behind another police unit, s (gridlock breaker).
    #[serde(default)]
    pub unit_wait_s: f32,
    /// What caps the speed right now: a vehicle's entity id, a police unit id, or none.
    #[serde(default)]
    pub blocked_by: Option<BlockedBy>,
    /// Time spent pressing on without moving (wedged against a kerb, wall or car), s.
    #[serde(default)]
    pub wedged_s: f32,
    /// Reversing out of a wedge for this much longer, s (0 = driving normally).
    #[serde(default)]
    pub reverse_s: f32,
    /// Steering held while reversing out, -1..=1.
    #[serde(default)]
    pub reverse_steer: f32,
    /// Reverse-outs since the unit last drove freely (the dispatcher retires a patrol car that
    /// cannot free itself).
    #[serde(default)]
    pub unstick_attempts: u32,
    /// Nose within [`PARKED_PINNED_GAP_M`] of a parked car it has to get around (backs up).
    #[serde(default)]
    pub pinned: bool,
}

/// What a unit is waiting behind (for views and debugging).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockedBy {
    /// A vehicle.
    Vehicle(EntityId),
    /// Another police unit.
    Unit(UnitId),
    /// A vehicle without an entity (kinematic traffic).
    Traffic,
}

impl Default for AvoidState {
    fn default() -> Self {
        Self {
            limit_mps: f32::INFINITY,
            blocked_s: 0.0,
            pass_offset_m: None,
            pass_until_s: 0.0,
            junction: None,
            junction_limit_mps: f32::INFINITY,
            yield_s: 0.0,
            unit_wait_s: 0.0,
            blocked_by: None,
            wedged_s: 0.0,
            reverse_s: 0.0,
            reverse_steer: 0.0,
            unstick_attempts: 0,
            pinned: false,
        }
    }
}

/// Traffic light for a unit's next junction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Light {
    /// Green.
    Go,
    /// Yellow: stop if there is room to.
    Caution,
    /// Red.
    Stop,
}

impl Light {
    /// From the world's signal state (`Off` = dark signal, treated as unsignalised).
    pub fn from_signal(s: SignalState) -> Option<Light> {
        match s {
            SignalState::Green => Some(Light::Go),
            SignalState::Yellow => Some(Light::Caution),
            SignalState::Red => Some(Light::Stop),
            SignalState::Off => None,
        }
    }
}

/// The next junction on a unit's track (refreshed at think rate).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct JunctionAhead {
    /// Arc length of the junction centre on the track, m.
    pub s: f32,
    /// Its light for this approach (`None` = unsignalised or dark).
    pub light: Option<Light>,
    /// The track turns there (more than ~30°).
    #[serde(default)]
    pub turn: bool,
    /// Which way it turns there: -1 left, 0 straight on, 1 right (drives the turn signal).
    #[serde(default)]
    pub turn_dir: i8,
}

/// The nearest vehicle in a lane corridor.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Blocker {
    /// Its entity, if any.
    pub(crate) entity: Option<EntityId>,
    /// The police unit it is, if any.
    pub(crate) unit: Option<UnitId>,
    /// Bumper-to-bumper gap along the track, m (negative = overlapping).
    pub(crate) gap_m: f32,
    /// Its speed along the track, m/s (negative = oncoming).
    pub(crate) along_mps: f32,
    /// Its total speed, m/s.
    pub(crate) speed_mps: f32,
    /// Arc length of its far end on the track, m.
    pub(crate) far_s: f32,
    /// It is a parked car.
    pub(crate) parked: bool,
    /// Its heading along the track (1 = same way, -1 = facing the unit).
    pub(crate) facing: f32,
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
    /// The racer the unit is after, if any (never overtaken: a stopped target is an arrest).
    pub(crate) target: Option<EntityId>,
    /// Gridlock breaker: ignore stopped police units with a higher id that cross or face us.
    pub(crate) skip_higher_units: bool,
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
    // On its track (not turning around or cutting back onto it): the planned path decides.
    let on_track = dist2(me.pos, lane_point(track, me.track_s, offset_m)) < 2.0 * 2.0;
    let mut best: Option<Blocker> = None;
    for o in obstacles {
        if o.unit == Some(me.unit)
            || (o.entity.is_some() && (o.entity == me.entity || o.entity == me.ignore))
        {
            continue;
        }
        if me.skip_higher_units
            && o.unit.is_some_and(|u| u > me.unit)
            && o.speed() < 0.5
            && dot(o.fwd, me.heading) < 0.5
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
        // Mid-turn the nose sweeps across cars waiting beside the path (the other lane of the
        // road being turned into): on track, those only count right at the bumper.
        let off_path = on_track
            && bumper_gap > BUMPER_CONTACT_M
            && path_clearance(track, me.track_s, horizon_m, offset_m, o) > cfg.lateral_margin_m;
        if bumper
            && !off_path
            && along > 0.0
            && bumper_gap < cfg.bumper_m
            && dot(rel, side).abs() < UNIT_HALF_WIDTH_M + ext_side + cfg.lateral_margin_m
        {
            let b = Blocker {
                entity: o.entity,
                unit: o.unit,
                gap_m: bumper_gap,
                along_mps: dot(o.vel, me.heading),
                speed_mps: o.speed(),
                far_s: me.track_s + along + ext_fwd,
                parked: o.parked,
                facing: dot(o.fwd, me.heading),
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
            entity: o.entity,
            unit: o.unit,
            gap_m: gap,
            along_mps: dot(o.vel, t),
            speed_mps: o.speed(),
            far_s: s_o + ext_t,
            parked: o.parked,
            facing: dot(o.fwd, t),
        };
        if best.is_none_or(|x| b.gap_m < x.gap_m) {
            best = Some(b);
        }
    }
    best
}

/// A pass met head-on by a stopped car is given up after this long (without lights), s.
const PASS_ABORT_S: f32 = 3.0;
/// A stopped vehicle with another stopped one this close ahead of it is in a queue, m.
const QUEUE_GAP_M: f32 = 10.0;

/// Whether the stopped blocker `b` is the end of a queue (another stopped, moving-type vehicle
/// within [`QUEUE_GAP_M`] ahead of it in the lane): units without lights wait in queues
/// instead of overtaking them into the oncoming lane.
fn in_queue(
    me: &Me,
    track: &Track,
    offset_m: f32,
    b: &Blocker,
    obstacles: &[Obstacle],
    cfg: &AvoidConfig,
) -> bool {
    let s = b.far_s + UNIT_HALF_LENGTH_M;
    let ahead = Me {
        pos: lane_point(track, s, offset_m),
        heading: normalize(sub(track.point_at(s + 1.0), track.point_at(s - 1.0))),
        track_s: s,
        ..*me
    };
    // Only what is beyond the blocker's front (the blocker itself projects there too).
    let beyond: Vec<Obstacle> = obstacles
        .iter()
        .filter(|o| dot(sub(o.pos, ahead.pos), ahead.heading) > 0.0)
        .copied()
        .collect();
    scan(&ahead, track, offset_m, QUEUE_GAP_M, &beyond, cfg, false)
        .is_some_and(|q| !q.parked && q.speed_mps < cfg.stopped_mps && q.gap_m < QUEUE_GAP_M)
}

/// Within this bumper gap an obstacle ahead of the body always counts, on the path or not, m.
const BUMPER_CONTACT_M: f32 = 1.5;

/// Sideways clearance between obstacle `o` and the unit's corridor around the lane `offset_m`
/// right of the track, where `o` projects onto the track ahead of `track_s` (negative =
/// overlapping).
fn path_clearance(track: &Track, track_s: f32, horizon_m: f32, offset_m: f32, o: &Obstacle) -> f32 {
    let (s_o, _) = track.project_ahead(o.pos, track_s, 2.0, horizon_m + 5.0);
    let t = normalize(sub(track.point_at(s_o + 1.0), track.point_at(s_o - 1.0)));
    let n = [t[1], -t[0]];
    let lat = dot(sub(o.pos, lane_point(track, s_o, offset_m)), n).abs();
    let ext_n = o.half_len * dot(o.fwd, n).abs() + o.half_wid * dot(o.fwd, t).abs();
    lat - UNIT_HALF_WIDTH_M - ext_n
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
    let h = horizon(v, cfg);
    let offset = parked_nudge(
        me,
        track,
        st.pass_offset_m.unwrap_or(base_offset_m),
        h,
        obstacles,
        cfg,
    );
    let mut blocker = scan(me, track, offset, h, obstacles, cfg, true);
    // Gridlock: units from several directions waiting on each other at a junction. After
    // `gridlock_s` the lower id creeps on past stopped higher-id units (they keep waiting).
    let on_unit = blocker.is_some_and(|b| b.unit.is_some() && b.speed_mps < 0.5) && v < 0.5;
    st.unit_wait_s = if on_unit { st.unit_wait_s + dt } else { 0.0 };
    let mut creeping = false;
    if st.unit_wait_s > cfg.gridlock_s {
        let me2 = Me {
            skip_higher_units: true,
            ..*me
        };
        let b2 = scan(&me2, track, offset, h, obstacles, cfg, true);
        if b2 != blocker {
            blocker = b2;
            creeping = true;
        }
    }
    st.junction_limit_mps = junction_speed(st.junction, me.track_s, v, siren, cfg);
    // Give way to cross traffic: a patrol at an unsignalised junction, a unit with lights at a
    // red. A unit waits at most `yield_max_s` (`yield_max_siren_s` with lights), then goes.
    let give_way = st.junction.is_some_and(|j| match j.light {
        None => !siren,
        Some(Light::Stop) => siren,
        _ => false,
    });
    let max_wait = if siren {
        cfg.yield_max_siren_s
    } else {
        cfg.yield_max_s
    };
    match st.junction {
        Some(j)
            if give_way && st.yield_s < max_wait && must_yield(me, track, j, v, obstacles, cfg) =>
        {
            st.yield_s += dt;
            st.junction_limit_mps = st.junction_limit_mps.min(stop_at_line(j, me.track_s, cfg));
        }
        _ => {
            if !give_way {
                st.yield_s = 0.0;
            }
        }
    }
    st.limit_mps = blocker
        .map_or(f32::INFINITY, |b| follow_speed(&b, v, cfg))
        .min(st.junction_limit_mps);
    if creeping {
        st.limit_mps = st.limit_mps.min(cfg.pass_creep_mps);
    }
    st.pinned = blocker.is_some_and(|b| b.parked && b.gap_m < PARKED_PINNED_GAP_M);
    st.blocked_by = blocker.map(|b| match (b.unit, b.entity) {
        (Some(u), _) => BlockedBy::Unit(u),
        (None, Some(e)) => BlockedBy::Vehicle(e),
        (None, None) => BlockedBy::Traffic,
    });
    // A pass met head-on by a stopped car (it waits for the unit): give it up and go back.
    let head_on = blocker.is_some_and(|b| {
        b.unit.is_none() && !b.parked && b.speed_mps < cfg.stopped_mps && b.facing < -0.5
    }) && v < 0.5;
    if st.pass_offset_m.is_some() && head_on && !siren && st.pass_offset_m != Some(base_offset_m) {
        st.blocked_s += dt;
        if st.blocked_s > PASS_ABORT_S {
            st.pass_offset_m = None;
            st.blocked_s = 0.0;
            return base_offset_m;
        }
    }
    if st.pass_offset_m.is_some() {
        // Pulling out from right behind the stopped vehicle: it stays in the bumper box until
        // the car has turned, so creep while there is room.
        let creep = if blocker.is_none_or(|b| b.gap_m > cfg.pass_creep_gap_m) {
            cfg.pass_creep_mps
        } else {
            0.0
        };
        st.limit_mps = st.limit_mps.max(creep).min(cfg.pass_speed_mps);
        if !head_on {
            st.blocked_s = 0.0;
        }
        return offset;
    }
    let stuck = blocker.filter(|b| {
        b.speed_mps < cfg.stopped_mps
            && b.gap_m < cfg.pass_gap_m
            && (b.entity.is_none() || b.entity != me.target)
    });
    let Some(b) = stuck else {
        st.blocked_s = 0.0;
        return offset;
    };
    // Without lights a unit queues like traffic: no passing near a junction (a red-light queue).
    let near_junction = st
        .junction
        .is_some_and(|j| j.s - me.track_s < (b.far_s - me.track_s) + cfg.no_pass_near_junction_m);
    if !siren && !b.parked && (near_junction || in_queue(me, track, offset, &b, obstacles, cfg)) {
        st.blocked_s = 0.0;
        return offset;
    }
    st.blocked_s += dt;
    let wait = if siren {
        cfg.pass_wait_siren_s
    } else {
        cfg.pass_wait_s
    };
    let wait = if b.parked {
        wait.min(PARKED_PASS_WAIT_S)
    } else {
        wait
    };
    if st.blocked_s < wait {
        return offset;
    }
    // Other lane (or, for a vehicle straddling the centreline, a little further left): clear
    // past the blocker, and no oncoming vehicle close. Parked cars at that lane's kerb do not
    // count: the nudge takes the unit past them once it is over there.
    let need = (b.far_s - me.track_s) + cfg.pass_clear_m;
    let moving: Vec<Obstacle> = obstacles.iter().filter(|o| !o.parked).copied().collect();
    for shift in [cfg.pass_shift_m, cfg.pass_shift_m + cfg.pass_extra_shift_m] {
        let alt = base_offset_m - shift;
        let clear = match scan(
            me,
            track,
            alt,
            need + cfg.oncoming_clear_m,
            &moving,
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

/// Largest sideways shift a unit makes within its lane to clear a parked car, m. A parked car
/// that needs more is a blocker like any other (wait, then pass).
/// (A full car width: the unit may cross the centreline to get by, like a driver would; an
/// oncoming car in the shifted corridor still stops it.)
pub const MAX_PARKED_NUDGE_M: f32 = 2.6;
/// A parked car blocking the lane is passed after this long, also near a junction (it will
/// never move), s.
pub const PARKED_PASS_WAIT_S: f32 = 1.0;
/// Closer than this to a parked car it cannot steer around, a unit backs up first, m.
pub const PARKED_PINNED_GAP_M: f32 = 1.5;

/// Lane offset that clears every parked car (or stopped oncoming car) ahead poking into the
/// corridor around `offset_m`,
/// shifting by at most [`MAX_PARKED_NUDGE_M`] (a car that needs more is left to the scan).
pub(crate) fn parked_nudge(
    me: &Me,
    track: &Track,
    offset_m: f32,
    horizon_m: f32,
    obstacles: &[Obstacle],
    cfg: &AvoidConfig,
) -> f32 {
    if !track.is_drivable() {
        return offset_m;
    }
    let reach2 = (horizon_m + 10.0) * (horizon_m + 10.0);
    // Corridor half width, plus a little so the scan does not see it on the boundary.
    let w = UNIT_HALF_WIDTH_M + cfg.lateral_margin_m + 0.05;
    let (mut lo, mut hi) = (f32::NEG_INFINITY, f32::INFINITY);
    // Parked cars, and stopped oncoming cars waiting at the edge of the corridor (they may be
    // waiting for the unit: edging past breaks the standoff). Never the racer it is after: a
    // stopped target is an arrest.
    let waiting = |o: &&Obstacle| {
        o.parked
            || (o.unit.is_none()
                && (o.entity.is_none() || (o.entity != me.target && o.entity != me.ignore))
                && o.speed() < 0.3
                && dot(o.fwd, me.heading) < -0.5)
    };
    for o in obstacles.iter().filter(waiting) {
        let rel = sub(o.pos, me.pos);
        if dot(rel, rel) > reach2 || dot(rel, me.heading) < -(o.half_len + UNIT_HALF_LENGTH_M) {
            continue;
        }
        let (s_o, _) = track.project_ahead(o.pos, me.track_s, 2.0, horizon_m + 5.0);
        if s_o > me.track_s + horizon_m + o.half_len {
            continue;
        }
        let t = normalize(sub(track.point_at(s_o + 1.0), track.point_at(s_o - 1.0)));
        let n = [t[1], -t[0]];
        // Signed offset of its centre from the centreline (+ right) and its half extent across.
        let lat = dot(sub(o.pos, track.point_at(s_o)), n);
        let ext_n = o.half_len * dot(o.fwd, n).abs() + o.half_wid * dot(o.fwd, t).abs();
        if (lat - offset_m).abs() >= ext_n + w {
            continue; // clear of the corridor
        }
        if lat >= offset_m {
            hi = hi.min(lat - ext_n - w);
        } else {
            lo = lo.max(lat + ext_n + w);
        }
    }
    let mut out = offset_m;
    if hi < out && offset_m - hi <= MAX_PARKED_NUDGE_M {
        out = hi;
    }
    if lo > out && lo - offset_m <= MAX_PARKED_NUDGE_M && lo <= hi {
        out = lo;
    }
    out
}

/// Speed cap from traffic rules at the next junction, m/s.
///
/// Without lights a unit stops at a red (and at a yellow it can stop for) and slows to
/// `yield_mps` through an unsignalised junction. With lights it may run reds and ignore limits,
/// but crosses every junction at `siren_junction_mps` (`siren_red_mps` against a red).
pub(crate) fn junction_speed(
    j: Option<JunctionAhead>,
    track_s: f32,
    v: f32,
    siren: bool,
    cfg: &AvoidConfig,
) -> f32 {
    let Some(j) = j else {
        return f32::INFINITY;
    };
    let to_centre = j.s - track_s;
    if to_centre < -cfg.junction_slow_m {
        return f32::INFINITY; // through it
    }
    // Slow to `v_at` by `junction_slow_m` before the centre.
    let approach = |v_at: f32| {
        let d = (to_centre - cfg.junction_slow_m).max(0.0);
        (v_at * v_at + 2.0 * cfg.decel_mps2 * d).sqrt()
    };
    // Turning: slow enough to hold the lane through the corner (swinging wide clips cars
    // waiting at the junction).
    let turn_cap = if j.turn {
        approach(if siren {
            cfg.siren_turn_mps
        } else {
            cfg.turn_mps
        })
    } else {
        f32::INFINITY
    };
    if siren {
        return approach(if j.light == Some(Light::Stop) {
            cfg.siren_red_mps
        } else {
            cfg.siren_junction_mps
        })
        .min(turn_cap);
    }
    // Bumper to stop line.
    let to_line = to_centre - cfg.stop_line_back_m - UNIT_HALF_LENGTH_M;
    let stop = match j.light {
        Some(Light::Stop) => true,
        Some(Light::Caution) => to_line > v * v / (2.0 * cfg.decel_mps2),
        Some(Light::Go) => return turn_cap,
        None => return approach(cfg.yield_mps).min(turn_cap),
    };
    if !stop || to_line < -1.0 {
        // Already over the line: clear the junction.
        return turn_cap;
    }
    let room = to_line - 0.5;
    if room <= 0.0 {
        0.0
    } else {
        (2.0 * cfg.decel_mps2 * room).sqrt()
    }
}

/// Speed cap that stops the bumper at the junction's stop line (`INFINITY` once over it).
fn stop_at_line(j: JunctionAhead, track_s: f32, cfg: &AvoidConfig) -> f32 {
    let to_line = j.s - track_s - cfg.stop_line_back_m - UNIT_HALF_LENGTH_M;
    if to_line < -1.0 {
        return f32::INFINITY;
    }
    let room = to_line - 0.5;
    if room <= 0.0 {
        0.0
    } else {
        (2.0 * cfg.decel_mps2 * room).sqrt()
    }
}

/// Whether cross traffic has the junction first: a vehicle inside the junction box, or one
/// approaching it that gets there before the unit is through (`yield_gap_s` margin). Vehicles on
/// the unit's own approach (ahead or behind in its lane) are the follow logic's business, and a
/// police unit only gives way to units with a lower id (no mutual waiting).
pub(crate) fn must_yield(
    me: &Me,
    track: &Track,
    j: JunctionAhead,
    v: f32,
    obstacles: &[Obstacle],
    cfg: &AvoidConfig,
) -> bool {
    let to_centre = j.s - me.track_s;
    if to_centre > cfg.yield_lookahead_m || stop_at_line(j, me.track_s, cfg).is_infinite() {
        return false;
    }
    let centre = track.point_at(j.s);
    let my_t = to_centre.max(0.0) / v.max(cfg.yield_mps);
    let r2 = cfg.yield_radius_m * cfg.yield_radius_m;
    obstacles.iter().any(|o| {
        if o.parked
            || o.unit == Some(me.unit)
            || (o.entity.is_some() && (o.entity == me.entity || o.entity == me.ignore))
            || o.unit.is_some_and(|u| u > me.unit)
        {
            return false;
        }
        let d = sub(centre, o.pos);
        let dist2c = dot(d, d);
        if dist2c > r2 {
            return false;
        }
        // On the unit's own approach.
        let (s_o, lat) = track.project_ahead(o.pos, me.track_s, 40.0, to_centre + 5.0);
        if lat < 3.0 && s_o < j.s - cfg.box_radius_m {
            return false;
        }
        if dist2c < cfg.box_radius_m * cfg.box_radius_m {
            return true; // in the junction
        }
        let speed = o.speed();
        if speed < 0.5 || dot(o.vel, d) <= 0.0 {
            return false; // waiting, parked or leaving
        }
        let t_o = dist2c.sqrt() / speed;
        t_o < my_t + cfg.yield_gap_s
    })
}

/// Whether a unit may spawn at `pos`: at least `spawn_clear_m` from every obstacle and out of
/// every moving vehicle's path for the next `spawn_path_s` seconds.
pub(crate) fn spawn_clear(pos: [f32; 2], obstacles: &[Obstacle], cfg: &AvoidConfig) -> bool {
    let r2 = cfg.spawn_clear_m * cfg.spawn_clear_m;
    obstacles.iter().all(|o| {
        // Spawn points are lane centres: kerbside parked cars are beside them, not on them.
        if o.parked {
            return true;
        }
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
            target: None,
            skip_higher_units: false,
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
    fn junction_rules() {
        let c = cfg();
        let red = Some(JunctionAhead {
            s: 60.0,
            light: Some(Light::Stop),
            turn: false,
            turn_dir: 0,
        });
        // Patrol stops before the line at a red, and holds once there.
        let far = junction_speed(red, 0.0, 10.0, false, &c);
        assert!(far > 5.0 && far < 30.0, "{far}");
        let at_line = 60.0 - c.stop_line_back_m - UNIT_HALF_LENGTH_M - 0.4;
        assert_eq!(junction_speed(red, at_line, 0.0, false, &c), 0.0);
        // With lights it runs the red, slowly.
        let v = junction_speed(red, 60.0 - c.junction_slow_m, 20.0, true, &c);
        assert!((v - c.siren_red_mps).abs() < 1e-3, "{v}");
        // Green: no cap. Unsignalised: yield speed at the junction.
        let green = Some(JunctionAhead {
            s: 60.0,
            light: Some(Light::Go),
            turn: false,
            turn_dir: 0,
        });
        assert!(junction_speed(green, 50.0, 10.0, false, &c).is_infinite());
        let plain = Some(JunctionAhead {
            s: 60.0,
            light: None,
            turn: false,
            turn_dir: 0,
        });
        let v = junction_speed(plain, 60.0 - c.junction_slow_m, 10.0, false, &c);
        assert!((v - c.yield_mps).abs() < 1e-3, "{v}");
        // Past the junction: no cap.
        assert!(junction_speed(red, 75.0, 10.0, false, &c).is_infinite());
    }

    #[test]
    fn no_overtaking_a_red_light_queue_without_lights() {
        let c = cfg();
        let obs = [parked(2, [1.75, 7.5])];
        let m = me(0.0, [1.75, 0.0]);
        let mut st = AvoidState {
            junction: Some(JunctionAhead {
                s: 20.0,
                light: Some(Light::Stop),
                turn: false,
                turn_dir: 0,
            }),
            ..AvoidState::default()
        };
        let mut off = 1.75;
        for _ in 0..(240.0 * (c.pass_wait_s + 1.0)) as usize {
            off = update(
                &mut st,
                &m,
                &straight(),
                1.75,
                0.0,
                false,
                &obs,
                &c,
                1.0 / 240.0,
            );
        }
        assert_eq!(off, 1.75);
    }

    #[test]
    fn patrol_gives_way_to_cross_traffic() {
        let c = cfg();
        let junction = JunctionAhead {
            s: 30.0,
            light: None,
            turn: false,
            turn_dir: 0,
        };
        let plain = Some(junction);
        let m = me(10.0, [1.75, 10.0]);
        // Crossing from the right toward the junction centre (0, 30) at 10 m/s, 20 m out.
        let cross = [Obstacle::car(
            Some(EntityId(5)),
            None,
            [20.0, 31.75],
            [-10.0, 0.0],
            [-1.0, 0.0],
        )];
        assert!(must_yield(&m, &straight(), junction, 5.0, &cross, &c));
        let mut st = AvoidState {
            junction: plain,
            ..AvoidState::default()
        };
        update(
            &mut st,
            &m,
            &straight(),
            1.75,
            5.0,
            false,
            &cross,
            &c,
            1.0 / 240.0,
        );
        // Braking to stop at the line (10.5 m away) and counting the wait.
        let line = stop_at_line(junction, 10.0, &c);
        assert!(line < 10.0 && st.limit_mps <= line, "{}", st.limit_mps);
        assert!(st.yield_s > 0.0);
        // Leaving the junction, or far away: no need to wait.
        let leaving = [Obstacle::car(
            Some(EntityId(5)),
            None,
            [20.0, 31.75],
            [10.0, 0.0],
            [1.0, 0.0],
        )];
        assert!(!must_yield(&m, &straight(), junction, 5.0, &leaving, &c));
        // A police unit with a higher id gives way to us instead.
        let mut cop = cross;
        cop[0].unit = Some(UnitId(9));
        assert!(!must_yield(&m, &straight(), junction, 5.0, &cop, &c));
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

    fn kerbside(pos: [f32; 2], fwd: [f32; 2]) -> Obstacle {
        Obstacle::from_view(&ObstacleView::parked([pos[0], pos[1], 0.0], fwd, 2.25, 0.95))
    }

    #[test]
    fn nudges_past_a_parked_car_poking_into_the_lane() {
        let c = cfg();
        let m = me(0.0, [1.75, 0.0]);
        // Angled kerbside car whose corner reaches into the lane (centre 3.2 m right).
        let obs = [kerbside([3.2, 25.0], normalize([0.3, 1.0]))];
        let mut st = AvoidState::default();
        let dt = 1.0 / 240.0;
        let off = update(&mut st, &m, &straight(), 1.75, 10.0, false, &obs, &c, dt);
        assert!(off < 1.75 && off > 1.75 - MAX_PARKED_NUDGE_M, "{off}");
        assert!(st.limit_mps.is_infinite(), "no stopping for it: {st:?}");
        // Square in the lane: too far to nudge, so it blocks (and is passed after a second,
        // even near a junction).
        let obs = [kerbside([1.75, 12.0], [0.0, 1.0])];
        st.junction = Some(JunctionAhead {
            s: 25.0,
            light: None,
            turn: false,
            turn_dir: 0,
        });
        let mut off = 1.75;
        for _ in 0..(240.0 * (PARKED_PASS_WAIT_S + 0.5)) as usize {
            off = update(&mut st, &m, &straight(), 1.75, 0.0, false, &obs, &c, dt);
        }
        assert!(off < 0.0, "passes the parked car: {off}");
        // Parked cars never block a spawn or a junction.
        assert!(spawn_clear([1.75, 40.0], &[kerbside([4.0, 40.0], [0.0, 1.0])], &c));
    }

    #[test]
    fn a_nose_sweeping_past_a_waiting_car_mid_turn_does_not_stop() {
        let c = cfg();
        // Right turn: north, then east at y = 100 (lane 1.75 right of the centreline).
        let t = Track::new(vec![[0.0, 0.0], [0.0, 100.0], [100.0, 100.0]]);
        let mut m = me(98.0, [1.75, 97.5]);
        m.heading = normalize([0.7, 0.7]);
        // A car waiting in the other lane of the road being turned into, ahead of the nose.
        let waiting = Obstacle::car(
            Some(EntityId(3)),
            None,
            [6.0, 102.2],
            [0.0, 0.0],
            [-1.0, 0.0],
        );
        assert!(scan(&m, &t, 1.75, 40.0, &[waiting], &c, true).is_none());
        // Right at the bumper it still counts.
        let close = Obstacle::car(Some(EntityId(3)), None, [4.2, 100.2], [0.0, 0.0], [-1.0, 0.0]);
        assert!(scan(&m, &t, 1.75, 40.0, &[close], &c, true).is_some());
    }

    #[test]
    fn without_lights_a_unit_queues_instead_of_overtaking_a_queue() {
        let c = cfg();
        let m = me(0.0, [1.75, 0.0]);
        let dt = 1.0 / 240.0;
        let run = |obs: &[Obstacle], siren: bool| {
            let mut st = AvoidState::default();
            let mut off = 1.75;
            for _ in 0..(240.0 * (c.pass_wait_s + 1.0)) as usize {
                off = update(&mut st, &m, &straight(), 1.75, 0.0, siren, obs, &c, dt);
            }
            off
        };
        let lone = [parked(2, [1.75, 9.0])];
        let queue = [parked(2, [1.75, 9.0]), parked(3, [1.75, 15.5])];
        assert!(run(&lone, false) < 0.0, "passes a lone stopped car");
        assert_eq!(run(&queue, false), 1.75, "waits in a queue");
        assert!(run(&queue, true) < 0.0, "with lights it goes round the queue");
    }
}
