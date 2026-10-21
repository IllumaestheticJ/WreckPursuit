//! Blocked-vehicle behaviour (owner playtest row 32): what a civilian does when a stationary
//! vehicle — a wreck, a parked player, a stopped police car, debris — blocks its lane.
//!
//! 1. **Detect.** Every decision records why the car is held up ([`Wait`]): a stop line /
//!    signal / siren (`Line`), a normal queue (`Queue`: the car ahead is itself waiting at a
//!    junction or for a light), or an obstruction (`Obstructed`: the car ahead is disabled, is
//!    itself stuck behind an obstruction, or is a stationary tracked vehicle / debris that is not
//!    at a stop line). Obstructions are stopped behind with [`OBSTACLE_GAP_M`] extra room.
//! 2. **Patience.** A driver acts once the obstruction has been stationary for its patience
//!    (3–8 s by personality, derived from the id and the traffic seed: no RNG draws) and it has
//!    stood behind it for [`SETTLE_S`].
//! 3. **Go around.** Multi-lane roads: a forced MOBIL lane change (the incentive threshold is
//!    lowered; the safety criterion still applies) — kinematic and physical agents alike. Two-lane
//!    roads: a slow pass in the oncoming lane ([`Pass`]) when the obstruction is short, the pass
//!    ends ≥ 20 m before the junction, the road ahead is visible (no crest, no bend > 20°) and
//!    the oncoming lane is clear for the time the pass takes at 1.25 × its limit.
//! 4. **Turn around** otherwise: a U-turn or k-point turn ([`crate::manoeuvre::plan_turn`])
//!    into the opposing lane when it is clear, or — one-way streets, no room to turn — reverse
//!    back through the junction to the end of the previous lane. Either way the blocked lane is
//!    remembered for [`BLOCKED_LANE_MEMORY_S`]: nobody plans into it while alternatives exist,
//!    and cars already planning to enter it re-route.
//!
//! Safety: passing cars and turning cars put an entry into the lane they intrude into, so
//! oncoming traffic stops for them like for any car ahead; a pass is abandoned if an oncoming
//! car shows up before the passer is alongside the obstruction.

use wp_math::StateHasher;

use super::*;
use crate::manoeuvre::{
    plan_turn, CarShape, ManoeuvreStatus, ReverseDriver, RoadCorridor, TurnDriver, TurnPlan,
    CRAWL_MPS,
};

/// Extra standstill distance kept behind a stationary obstruction (wreck, parked car, debris),
/// so there is room to pull out around it, m.
pub const OBSTACLE_GAP_M: f32 = 6.0;
/// While pulling out to pass, aim this much further left than the oncoming lane's centre (a
/// physical car then turns out more sharply and clears the obstruction sooner), m.
pub const PASS_PULL_OUT_EXTRA_M: f32 = 0.6;
/// A car outside any manoeuvre whose centre is this far left of its lane, towards the oncoming
/// lane, also occupies that lane (same threshold as a passing car's entry), m.
pub const STRAY_INTRUSION_LAT_M: f32 = 0.8;
/// A car left over the centre line ([`STRAY_INTRUSION_LAT_M`]) also holds oncoming traffic this
/// far up the oncoming lane, so cars arriving later stop clear of the room
/// [`turn_oncoming_room`] asks for and it can still turn around out of the spot, m.
pub const STRAY_HOLD_BACK_M: f32 = 22.0;
/// A pass held behind the obstruction (body not clear) for this long is abandoned, s.
pub const PASS_STALL_S: f32 = 4.0;
/// Time a driver stands behind an obstruction before acting (on top of its patience for how
/// long the obstruction has been there), s.
pub const SETTLE_S: f32 = 1.0;
/// Overtaking speed past a stationary vehicle, m/s (≈ 14 km/h).
pub const PASS_MPS: f32 = 4.0;
/// A lane someone turned around in counts as blocked for this long, s.
pub const BLOCKED_LANE_MEMORY_S: f32 = 90.0;
/// Reversing speed back to the junction, m/s.
pub const REVERSE_MPS: f32 = 2.5;
/// A pass ends at least this far before the junction ahead, m.
pub const PASS_JUNCTION_CLEAR_M: f32 = 20.0;
/// Longest obstruction (cluster of stopped cars) a driver passes in the oncoming lane, m.
pub const MAX_PASS_OBSTACLE_M: f32 = 30.0;
/// Sharpest bend allowed within the sight distance of a pass, rad (20°).
pub const PASS_MAX_BEND_RAD: f32 = 0.35;
/// Deceleration oncoming traffic is assumed to manage when a car turns around across its lane,
/// m/s².
pub const TURN_ONCOMING_DECEL: f32 = 3.0;
/// Added to each oncoming car's current speed when checking it can stop for a turn-around (it
/// may still be speeding up), m/s.
pub const TURN_ONCOMING_SPEED_MARGIN_MPS: f32 = 2.0;
/// Room left between a stopped oncoming car and the turning car, m.
pub const TURN_ONCOMING_GAP_M: f32 = 25.0;
/// Comfortable deceleration assumed for a follower rolling up behind a car about to turn
/// around, m/s².
pub const TURN_FOLLOWER_DECEL: f32 = 2.0;
/// Room behind a turning car's rear that its reversing legs may use (≈ a turning radius plus
/// margin): a follower must be able to stop short of it, m.
pub const TURN_REAR_ROOM_M: f32 = 10.0;

/// Distance an oncoming car at `v` m/s needs to stop for a car turning around across its lane:
/// its stopping distance from `v` + [`TURN_ONCOMING_SPEED_MARGIN_MPS`] at
/// [`TURN_ONCOMING_DECEL`], plus [`TURN_ONCOMING_GAP_M`], m.
pub fn turn_oncoming_room(v: f32) -> f32 {
    let v = v.max(0.0) + TURN_ONCOMING_SPEED_MARGIN_MPS;
    v * v / (2.0 * TURN_ONCOMING_DECEL) + TURN_ONCOMING_GAP_M
}

/// Can a follower at arc length `s` doing `v` m/s (same lane) stop at [`TURN_FOLLOWER_DECEL`]
/// short of the room behind a car about to turn around at `turner_s` (its rear minus
/// [`TURN_REAR_ROOM_M`])?
pub fn turn_follower_stops_short(s: f32, v: f32, turner_s: f32) -> bool {
    let v = v.max(0.0);
    s + v * v / (2.0 * TURN_FOLLOWER_DECEL) <= turner_s - CAR_LENGTH_M - TURN_REAR_ROOM_M
}

/// Why an agent is held up (as of its last decision).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Wait {
    /// Not held up (or moving).
    #[default]
    Free,
    /// Stop line, signal, siren, junction rules, pulling-away delay.
    Line,
    /// A normal queue: the car ahead is waiting for one of the above.
    Queue,
    /// A stationary obstruction (or a queue behind one).
    Obstructed,
}

/// What the obstruction ahead is.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(super) enum ObsRef {
    /// Nothing.
    #[default]
    None,
    /// Agent index (this step's order).
    Agent(u32),
    /// Tracked vehicle (entity id).
    Tracked(u32),
    /// Debris (position, radius).
    Hazard(V2, f32),
}

/// Passing a stationary obstruction in the oncoming lane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Pass {
    /// Oncoming lane.
    pub(super) opp: SegId,
    /// Lateral offset of the oncoming lane's centre from the own lane's, m (negative = left).
    pub(super) offset: f32,
    /// Own-lane cars at or before this arc length are the obstruction (ignored as leaders).
    pub(super) skip_until_s: f32,
    /// Arc length of the obstruction's rear on the own lane, m.
    pub(super) obs_rear_s: f32,
    /// Start merging back once the car centre is here, m.
    pub(super) end_s: f32,
    /// The obstruction's footprint (the passer stays clear of it).
    pub(super) obs_box: Obb,
    /// The obstruction.
    pub(super) obs: ObsRef,
    /// Where the passer is on the oncoming lane (refreshed each step), m.
    pub(super) opp_s: f32,
    /// Merging back.
    pub(super) returning: bool,
    /// Time held at a standstill beside the obstruction without being clear of it, s.
    pub(super) stall_s: f32,
}

/// A kinematic agent's progress through a planned turn.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct KinTurn {
    plan: TurnPlan,
    leg: usize,
    u: f32,
    v: f32,
    pause: f32,
    wait_s: f32,
    z: f32,
}

/// Turning around into the opposing lane.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Turn {
    /// Lane to drive off in.
    opp: SegId,
    /// Where the turn holds the oncoming lane (an entry there), m.
    opp_s: f32,
    /// Where it holds its own lane (an entry there, behind the car), m.
    own_s: f32,
    /// Road corridor and goal direction (for re-planning).
    road: RoadCorridor,
    goal: V2,
    kin: Option<KinTurn>,
    drv: Option<TurnDriver>,
}

/// Reversing back through the junction to the end of the previous lane.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Reverse {
    /// Blocked lane being left.
    lane: SegId,
    /// Connector reversed through.
    via: SegId,
    /// Lane reversed into.
    to: SegId,
    /// Stop with the car centre here on `to`, m.
    stop_s: f32,
    /// Still waiting for the spot to clear (its entry there already holds traffic back), s.
    waiting_s: Option<f32>,
    drv: Option<ReverseDriver>,
}

/// A car waiting this long for room to reverse into gives up, s.
const REVERSE_WAIT_MAX_S: f32 = 40.0;

/// A manoeuvre in progress.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Manoeuvre {
    Pass(Pass),
    Turn(Turn),
    Reverse(Reverse),
}

impl Manoeuvre {
    /// The pass, if this is one.
    pub(super) fn pass(&self) -> Option<Pass> {
        match self {
            Manoeuvre::Pass(p) => Some(*p),
            _ => None,
        }
    }

    /// Pose of a kinematic agent that is turning.
    pub(super) fn kinematic_pose(&self) -> Option<(V3, V2)> {
        match self {
            Manoeuvre::Turn(Turn { kin: Some(k), .. }) => {
                let shape = CarShape::CIVILIAN;
                let (rear, f) = k
                    .plan
                    .pose(k.leg.min(k.plan.legs.len().saturating_sub(1)), k.u);
                let (rear, f) = if k.leg >= k.plan.legs.len() {
                    k.plan.end_pose()
                } else {
                    (rear, f)
                };
                let c = shape.centre(rear, f);
                Some(([c[0], c[1], k.z], f))
            }
            _ => None,
        }
    }

    /// Indicator to show (`None` = hazard lights).
    pub(super) fn signal(&self) -> Option<i8> {
        match self {
            Manoeuvre::Pass(p) => Some(if p.returning { 1 } else { -1 }),
            Manoeuvre::Turn(_) => Some(-1),
            Manoeuvre::Reverse(_) => None,
        }
    }

    /// Connector held while reversing through it (not while still waiting to start: holding
    /// it then would stop cross traffic that the car it waits for is yielding to).
    pub(super) fn holds_connector(&self) -> Option<SegId> {
        match self {
            Manoeuvre::Reverse(r) if r.waiting_s.is_none() => Some(r.via),
            _ => None,
        }
    }

    pub(super) fn hash_into(&self, h: &mut StateHasher) {
        match self {
            Manoeuvre::Pass(p) => {
                h.write_u32(1);
                h.write_u32(p.opp);
                h.write_f32_q(p.end_s, 1e3);
                h.write_u32(p.returning as u32);
            }
            Manoeuvre::Turn(t) => {
                h.write_u32(2);
                h.write_u32(t.opp);
                if let Some(k) = &t.kin {
                    h.write_u32(k.leg as u32);
                    h.write_f32_q(k.u, 1e3);
                }
                if let Some(d) = &t.drv {
                    h.write_u32(d.legs_driven() as u32);
                }
            }
            Manoeuvre::Reverse(r) => {
                h.write_u32(3);
                h.write_u32(r.via);
                h.write_u32(r.to);
            }
        }
    }
}

impl Agent {
    /// Reversing (speed `v` is then backwards).
    pub(super) fn reversing(&self) -> bool {
        matches!(self.manoeuvre.as_deref(), Some(Manoeuvre::Reverse(_)))
    }

    /// Extra lane entries this agent occupies: `(segment, s, v)`.
    pub(super) fn intrusions(&self) -> impl Iterator<Item = (SegId, f32, f32)> {
        let mut out: [Option<(SegId, f32, f32)>; 2] = [None, None];
        match self.manoeuvre.as_deref() {
            Some(Manoeuvre::Pass(p)) if self.lat.abs() > STRAY_INTRUSION_LAT_M => {
                out[0] = Some((p.opp, p.opp_s, -self.v));
            }
            Some(Manoeuvre::Turn(t)) => {
                out[0] = Some((t.opp, t.opp_s, 0.0));
                out[1] = Some((self.seg, t.own_s, 0.0));
            }
            Some(Manoeuvre::Reverse(r)) if self.seg != r.to => {
                out[0] = Some((r.to, r.stop_s, 0.0));
            }
            _ => {}
        }
        out.into_iter().flatten()
    }
}

/// Patience of driver `id` (3–8 s): how long an obstruction must have been stationary before
/// the driver goes around or turns. Derived from the id and the traffic seed (deterministic,
/// no RNG draw).
pub fn patience(seed: u64, id: AgentId, p: Personality) -> f32 {
    let mut x = seed ^ (id as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ 0xB10C_ED00;
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    let u = (x >> 40) as f32 / (1u64 << 24) as f32;
    match p {
        Personality::LawAbiding => 4.5 + 3.5 * u,
        Personality::Casual => 3.5 + 2.5 * u,
        Personality::Aggressive => 3.0 + 1.0 * u,
    }
}

/// Per lane segment: the opposing lane across the centre line (only for the leftmost lane of
/// a direction); per road: its paved driving lanes (both directions, bus lanes included).
pub(super) fn lane_topology(map: &CityMap) -> (Vec<SegId>, Vec<Vec<SegId>>) {
    let lanes = &map.net.lanes;
    let n_roads = lanes.iter().map(|l| l.road as usize + 1).max().unwrap_or(0);
    let mut road_lanes: Vec<Vec<SegId>> = vec![Vec::new(); n_roads];
    for (i, l) in lanes.iter().enumerate() {
        if l.kind != LaneKind::Parking {
            road_lanes[l.road as usize].push(i as SegId);
        }
    }
    let mut opposite = vec![NONE; map.segs.len()];
    for (i, l) in lanes.iter().enumerate() {
        if l.index_from_left != 0 || !l.kind.drivable() {
            continue;
        }
        for &o in &road_lanes[l.road as usize] {
            let ol = &lanes[o as usize];
            if ol.from_node == l.to_node
                && ol.to_node == l.from_node
                && ol.index_from_left == 0
                && ol.kind.drivable()
            {
                opposite[i] = o;
                break;
            }
        }
    }
    (opposite, road_lanes)
}

/// Result of a pass attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PassTry {
    Started,
    /// Possible here, but the oncoming lane is not clear now.
    NoGap,
    /// Not possible here (no oncoming lane, junction, crest, bend, obstruction too long).
    Impossible,
}

/// Body box of a car at `p` facing `f`, grown by `m`.
fn car_box(p: V2, f: V2, m: f32) -> Obb {
    Obb {
        c: p,
        f,
        hl: HALF_L + m,
        hw: CAR_WIDTH_M * 0.5 + m,
    }
}

/// Lane-aligned box (axis `t` through `origin`) covering every corner of `boxes`; returns the
/// box and its along-axis extent `(min, max)` relative to `origin`.
fn union_box(origin: V2, t: V2, boxes: &[Obb]) -> (Obb, f32, f32) {
    let r = geom::right_of(t);
    let (mut a0, mut a1, mut l0, mut l1) = (
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
    );
    for b in boxes {
        let br = geom::right_of(b.f);
        for (sl, sw) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
            let c = geom::add(
                b.c,
                geom::add(geom::scale(b.f, sl * b.hl), geom::scale(br, sw * b.hw)),
            );
            let d = geom::sub(c, origin);
            let (al, la) = (geom::dot(d, t), geom::dot(d, r));
            a0 = a0.min(al);
            a1 = a1.max(al);
            l0 = l0.min(la);
            l1 = l1.max(la);
        }
    }
    let c = geom::add(
        origin,
        geom::add(
            geom::scale(t, 0.5 * (a0 + a1)),
            geom::scale(r, 0.5 * (l0 + l1)),
        ),
    );
    (
        Obb {
            c,
            f: t,
            hl: 0.5 * (a1 - a0),
            hw: 0.5 * (l1 - l0),
        },
        a0,
        a1,
    )
}

fn tracked_box(t: &TrackedVehicle) -> Obb {
    let f = if geom::dot(t.forward, t.forward) > 0.25 {
        geom::normalize(t.forward)
    } else {
        [0.0, 1.0]
    };
    Obb {
        c: geom::xy(t.pos),
        f,
        hl: 2.3,
        hw: 1.0,
    }
}

fn hazard_box(h: &Hazard) -> Obb {
    Obb {
        c: geom::xy(h.pos),
        f: [0.0, 1.0],
        hl: h.radius_m,
        hw: h.radius_m,
    }
}

impl Traffic {
    /// Stationary time of each tracked vehicle (by entity).
    pub(super) fn update_tracked_still(&mut self, tracked: &[TrackedVehicle], dt: f32) {
        let mut next = std::collections::BTreeMap::new();
        for t in tracked {
            let sp = (t.vel[0] * t.vel[0] + t.vel[1] * t.vel[1]).sqrt();
            let still = if sp < 0.5 {
                self.tracked_still.get(&t.entity.0).copied().unwrap_or(0.0) + dt
            } else {
                0.0
            };
            next.insert(t.entity.0, still);
        }
        self.tracked_still = next;
    }

    pub(super) fn tracked_still_s(&self, entity: u32) -> f32 {
        self.tracked_still.get(&entity).copied().unwrap_or(0.0)
    }

    pub(super) fn expire_blocked_lanes(&mut self) {
        if !self.blocked_lanes.is_empty() {
            let now = self.step_count;
            self.blocked_lanes.retain(|_, t| *t > now);
        }
    }

    fn mark_blocked(&mut self, lane: SegId) {
        let until = self.step_count + (BLOCKED_LANE_MEMORY_S / TRAFFIC_DT) as u64;
        self.blocked_lanes.insert(lane, until);
    }

    /// Whether `lane` is currently known to be blocked.
    pub(super) fn lane_blocked(&self, lane: SegId) -> bool {
        !self.blocked_lanes.is_empty() && self.blocked_lanes.contains_key(&lane)
    }

    /// How a stationary tracked vehicle `long` metres ahead of agent `i` (in its path) holds it
    /// up: at the stop line / in the junction (`Line`), queued behind a waiting car (`Queue`) or
    /// standing in the lane (`Obstructed`).
    pub(super) fn classify_tracked(&self, map: &CityMap, i: usize, long: f32) -> Wait {
        let a = &self.agents[i];
        let seg = map.seg(a.seg);
        if seg.is_connector() {
            return Wait::Line;
        }
        let d_end = seg.length - a.s;
        let next_conn = a.plan[0] != NONE && map.seg(a.plan[0]).is_connector();
        if long - HALF_L > d_end - 2.0 || (next_conn && d_end - long < 15.0) {
            return Wait::Line;
        }
        let st = a.s + long;
        if let Some(e) = self.lists[a.seg as usize]
            .iter()
            .find(|e| e.s > st && !e.ghost && e.agent as usize != i)
        {
            if e.s - st < CAR_LENGTH_M + 8.0 {
                let l = &self.agents[e.agent as usize];
                return if l.disabled || l.wait == Wait::Obstructed {
                    Wait::Obstructed
                } else {
                    Wait::Queue
                };
            }
        }
        Wait::Obstructed
    }

    /// Agents not in a manoeuvre that still stand over the centre line, more than
    /// [`STRAY_INTRUSION_LAT_M`] towards the oncoming lane: `(agent, oncoming lane, s there)`.
    ///
    /// A pass that is given up while the car is beside the obstruction leaves it there (a
    /// physical car at a standstill cannot steer back), nose out in the oncoming lane. It still
    /// takes up that lane, so oncoming traffic must stop for it as for a passing car.
    pub(super) fn stray_intrusions(&self, map: &CityMap) -> Vec<(usize, SegId, f32)> {
        let mut out = Vec::new();
        for (i, a) in self.agents.iter().enumerate() {
            if a.manoeuvre.is_some() || a.lat > -STRAY_INTRUSION_LAT_M {
                continue;
            }
            let opp = self.opposite[a.seg as usize];
            if opp == NONE {
                continue;
            }
            let (pos, _) = self.pose_of(map, a);
            out.push((i, opp, map.seg(opp).project(geom::xy(pos)).s));
        }
        out
    }

    /// Refresh where passing / turning agents sit on the lane they intrude into.
    pub(super) fn update_intrusions(&mut self, map: &CityMap) {
        for i in 0..self.agents.len() {
            let Some(Manoeuvre::Pass(p)) = self.agents[i].manoeuvre.as_deref() else {
                continue;
            };
            let opp = p.opp;
            let (pos, _) = self.pose_of(map, &self.agents[i]);
            let s = map.seg(opp).project(geom::xy(pos)).s;
            if let Some(Manoeuvre::Pass(p)) = self.agents[i].manoeuvre.as_deref_mut() {
                p.opp_s = s;
            }
        }
    }

    /// Re-plan cars about to enter a lane known to be blocked (when they have a choice). A car
    /// already holding a permit into it gives the permit up if it can still stop comfortably
    /// (otherwise it would drive into the queue and block the junction).
    fn reroute(&mut self, map: &CityMap, inp: &TrafficInputs) {
        for i in 0..self.agents.len() {
            let a = &self.agents[i];
            if !(a.id as u64 + self.step_count).is_multiple_of(15)
                || a.disabled
                || a.manoeuvre.is_some()
                || a.plan[0] == NONE
                || map.seg(a.seg).is_connector()
                || self.lane_blocked(a.seg)
            {
                continue;
            }
            let c = a.plan[0];
            if !map.seg(c).is_connector() || !self.lane_blocked(map.seg(c).next[0]) {
                continue;
            }
            if a.permit != NONE {
                let d_line = map.seg(a.seg).length - a.s - HALF_L - 0.5;
                if a.permit != c || a.v * a.v > 2.0 * 3.0 * d_line.max(0.0) {
                    continue;
                }
            }
            let seg = a.seg;
            let n = self.choose_next(map, seg, inp.closed);
            if n != NONE && n != c {
                let id = self.agents[i].id;
                if self.reservations[c as usize] == id {
                    self.reservations[c as usize] = NONE;
                }
                let a = &mut self.agents[i];
                a.permit = NONE;
                a.plan = [n, map.seg(n).next[0]];
                self.stats.reroutes += 1;
            }
        }
    }

    /// Start go-arounds / turn-arounds for agents that have waited long enough.
    pub(super) fn resolve_blocks(&mut self, map: &CityMap, inp: &TrafficInputs) {
        if !self.blocked_lanes.is_empty() {
            self.reroute(map, inp);
        }
        for i in 0..self.agents.len() {
            let a = &self.agents[i];
            if a.wait != Wait::Obstructed
                || a.disabled
                || a.manoeuvre.is_some()
                || a.v >= 0.5
                || a.blocked_s < SETTLE_S
                || a.obstacle_still_s < a.patience_s
                || a.retry_s > 0.0
                || a.permit != NONE
                || a.ghost.is_some()
            {
                continue;
            }
            let seg_lane = map.seg(a.seg).lane();
            self.agents[i].retry_s = 1.0;
            let Some(lane) = seg_lane else {
                // Stuck in a junction connector behind the queue: back out of it.
                self.try_reverse(map, i);
                continue;
            };
            let a = &self.agents[i];
            let l = &map.net.lanes[lane as usize];
            let side_lane = [l.left, l.right].into_iter().flatten().any(|c| {
                map.net.lanes[c as usize].kind.drivable()
                    && !inp.closed.get(c as usize).copied().unwrap_or(false)
            });
            // Multi-lane roads: the forced lane change (MOBIL) gets the first chance.
            if side_lane && a.blocked_s < a.patience_s + 8.0 {
                continue;
            }
            let waited = a.blocked_s;
            let patience = a.patience_s;
            match self.try_pass(map, inp, i) {
                PassTry::Started => continue,
                PassTry::NoGap if waited < patience + 15.0 => continue,
                PassTry::NoGap => {}
                // No way past in this lane: tell the others (they re-route), then turn or back out.
                PassTry::Impossible => {
                    if !side_lane {
                        let seg = self.agents[i].seg;
                        self.mark_blocked(seg);
                    }
                }
            }
            if self.try_turn(map, inp, i) {
                continue;
            }
            self.try_reverse(map, i);
        }
    }

    /// Points (and tangents) every 5 m along the agent's path, from its position up to `dist`.
    fn path_profile(&self, map: &CityMap, a: &Agent, dist: f32) -> Vec<(V3, V2)> {
        let mut out = Vec::new();
        let mut d = 0.0;
        while d <= dist {
            let mut seg = a.seg;
            let mut s = a.s + d;
            let mut k = 0;
            loop {
                let sg = map.seg(seg);
                if s <= sg.length || k >= 2 || a.plan[k] == NONE {
                    out.push(sg.point_at(s.min(sg.length)));
                    break;
                }
                s -= sg.length;
                seg = a.plan[k];
                k += 1;
            }
            d += 5.0;
        }
        out
    }

    /// Is the oncoming lane `opp` clear of traffic for `dist` metres ahead of the agent
    /// (`s_m` = the agent's position on `opp`; oncoming cars come from lower `s`)?
    fn oncoming_clear(&self, map: &CityMap, i: usize, opp: SegId, s_m: f32, dist: f32) -> bool {
        let lo = s_m - dist;
        let busy = |e: &Entry| e.agent as usize != i && e.s > lo && e.s < s_m + CAR_LENGTH_M + 2.0;
        if self.lists[opp as usize].iter().any(busy) {
            return false;
        }
        if lo < 0.0 {
            let rem = -lo;
            for &c in &map.seg(opp).prev {
                if self.lists[c as usize].iter().any(|e| e.agent as usize != i) {
                    return false;
                }
                let SegKind::Connector { from, .. } = map.seg(c).kind else {
                    continue;
                };
                let fl = map.seg(from).length;
                for e in &self.lists[from as usize] {
                    let ag = &self.agents[e.agent as usize];
                    if !e.ghost && ag.plan[0] == c && fl - e.s < rem {
                        return false;
                    }
                }
            }
        }
        true
    }

    /// Can every car approaching on the oncoming lane `opp` stop before `s_m` (the turning
    /// car's position on it)? Each car needs its stopping distance from its current speed (plus
    /// [`TURN_ONCOMING_SPEED_MARGIN_MPS`]) at [`TURN_ONCOMING_DECEL`], plus [`TURN_ONCOMING_GAP_M`].
    /// Cars further upstream than a car at 1.25 × the limit would need are not looked at; any
    /// car in a junction connector feeding `opp` within that distance blocks the turn.
    fn oncoming_can_stop(&self, map: &CityMap, i: usize, opp: SegId, s_m: f32) -> bool {
        let need = turn_oncoming_room;
        let too_close = |e: &Entry, d: f32| e.agent as usize != i && d < need(e.v);
        if self.lists[opp as usize]
            .iter()
            .any(|e| e.s < s_m + CAR_LENGTH_M + 2.0 && too_close(e, s_m - e.s))
        {
            return false;
        }
        let os = map.seg(opp);
        if s_m >= need(os.speed_limit * 1.25) {
            return true;
        }
        for &c in &os.prev {
            if self.lists[c as usize].iter().any(|e| e.agent as usize != i) {
                return false;
            }
            let SegKind::Connector { from, .. } = map.seg(c).kind else {
                continue;
            };
            let base = s_m + map.seg(c).length + map.seg(from).length;
            for e in &self.lists[from as usize] {
                let ag = &self.agents[e.agent as usize];
                if !e.ghost && ag.plan[0] == c && too_close(e, base - e.s) {
                    return false;
                }
            }
        }
        true
    }

    /// Obstacle boxes within `r` metres of agent `i` (other agents, tracked vehicles, debris).
    fn obstacles_near(&self, map: &CityMap, inp: &TrafficInputs, i: usize, r: f32) -> Vec<Obb> {
        let (p, _) = self.pose_of(map, &self.agents[i]);
        let q = geom::xy(p);
        let near = |c: V2, z: f32| geom::dist(c, q) < r && (z - p[2]).abs() < 4.0;
        let mut out = Vec::new();
        for (j, a) in self.agents.iter().enumerate() {
            if j == i {
                continue;
            }
            let (pj, fj) = self.pose_of(map, a);
            if near(geom::xy(pj), pj[2]) {
                out.push(car_box(geom::xy(pj), fj, 0.0));
            }
        }
        for t in inp.tracked {
            if near(geom::xy(t.pos), t.pos[2]) {
                out.push(tracked_box(t));
            }
        }
        for h in inp.hazards {
            if near(geom::xy(h.pos), h.pos[2]) {
                out.push(hazard_box(h));
            }
        }
        out
    }

    /// The paved driving width of agent `i`'s road around its position, as a corridor along
    /// its lane (reference line = its lane centre).
    fn corridor_at(&self, map: &CityMap, i: usize) -> Option<RoadCorridor> {
        let a = &self.agents[i];
        let lane = map.seg(a.seg).lane()?;
        let (p, t) = map.seg(a.seg).point_at(a.s);
        let origin = geom::xy(p);
        let right = geom::right_of(t);
        let road = map.net.lanes[lane as usize].road as usize;
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for &l in &self.road_lanes[road] {
            let ls = map.seg(l);
            let pr = ls.project(origin);
            if pr.dist > 16.0 {
                continue;
            }
            let (pl, _) = ls.point_at(pr.s);
            let d = geom::dot(geom::sub(geom::xy(pl), origin), right);
            lo = lo.min(d - ls.width * 0.5);
            hi = hi.max(d + ls.width * 0.5);
        }
        lo.is_finite().then_some(RoadCorridor {
            origin,
            dir: t,
            lat_min: lo,
            lat_max: hi,
        })
    }

    /// Try to start passing the obstruction in the oncoming lane.
    fn try_pass(&mut self, map: &CityMap, inp: &TrafficInputs, i: usize) -> PassTry {
        let a = &self.agents[i];
        let seg = map.seg(a.seg);
        let opp = self.opposite[a.seg as usize];
        if opp == NONE || seg.speed_limit > 20.0 {
            return PassTry::Impossible;
        }
        let (lane_p, t) = seg.point_at(a.s);
        let lane_c = geom::xy(lane_p);
        let right = geom::right_of(t);
        // The obstruction: the actual obstacle only (cars further back wait for the queue).
        let mut boxes: Vec<Obb> = Vec::new();
        let mut skip_until = f32::NEG_INFINITY;
        match a.obs {
            ObsRef::Agent(j) => {
                let l = &self.agents[j as usize];
                if !l.disabled || l.seg != a.seg {
                    return PassTry::Impossible;
                }
                let (pl, fl) = self.pose_of(map, l);
                boxes.push(car_box(geom::xy(pl), fl, 0.0));
                skip_until = l.s;
                // Stopped cars right in front of it are part of the obstruction.
                let list = &self.lists[a.seg as usize];
                let mut last = l.s;
                for e in list.iter().filter(|e| e.s > l.s && !e.ghost) {
                    let ag = &self.agents[e.agent as usize];
                    if e.s - last > CAR_LENGTH_M + 6.0 || ag.v > 0.5 {
                        break;
                    }
                    let (pe, fe) = self.pose_of(map, ag);
                    boxes.push(car_box(geom::xy(pe), fe, 0.0));
                    skip_until = e.s;
                    last = e.s;
                }
            }
            ObsRef::Tracked(id) => {
                let Some(t) = inp.tracked.iter().find(|t| t.entity.0 == id) else {
                    return PassTry::Impossible;
                };
                boxes.push(tracked_box(t));
            }
            ObsRef::Hazard(c, r) => boxes.push(Obb {
                c,
                f: [0.0, 1.0],
                hl: r,
                hw: r,
            }),
            ObsRef::None => return PassTry::Impossible,
        }
        let (obs_box, al0, al1) = union_box(lane_c, t, &boxes);
        if al1 - al0 > MAX_PASS_OBSTACLE_M || al0 < HALF_L {
            return PassTry::Impossible;
        }
        let obs_rear_s = a.s + al0;
        let end_s = a.s + al1 + HALF_L + 4.0;
        let next_conn = a.plan[0] != NONE && map.seg(a.plan[0]).is_connector();
        if end_s + HALF_L + PASS_JUNCTION_CLEAR_M > seg.length && next_conn
            || end_s > seg.length - HALF_L
        {
            return PassTry::Impossible;
        }
        // The oncoming lane's centre relative to ours.
        let os = map.seg(opp);
        let pr = os.project(lane_c);
        let (po, _) = os.point_at(pr.s);
        let offset = geom::dot(geom::sub(geom::xy(po), lane_c), right);
        if !(-6.0..=-2.5).contains(&offset) {
            return PassTry::Impossible;
        }
        // The obstruction must leave the oncoming lane free.
        let mid = 0.5 * (a.s + end_s) - a.s;
        let corridor = Obb {
            c: geom::add(
                lane_c,
                geom::add(geom::scale(t, mid), geom::scale(right, offset)),
            ),
            f: t,
            hl: 0.5 * (end_s - a.s) + HALF_L,
            hw: CAR_WIDTH_M * 0.5 + 0.3,
        };
        if geom::obb_overlap(&corridor, &obs_box)
            || inp
                .hazards
                .iter()
                .any(|h| geom::obb_overlap(&corridor, &hazard_box(h)))
        {
            return PassTry::Impossible;
        }
        let d = end_s - a.s;
        let t_pass = d / PASS_MPS + 3.0;
        let v_onc = os.speed_limit * 1.25;
        let clear = (d + v_onc * t_pass + 20.0).min(160.0);
        // No passing over a crest or round a bend: the road must be visible up to the junction
        // ahead (or `clear` m). Traffic turning in at that junction is checked upstream.
        let sight = clear.min(seg.length - a.s);
        if !crate::manoeuvre::sight_clear(&self.path_profile(map, a, sight), PASS_MAX_BEND_RAD) {
            return PassTry::Impossible;
        }
        let q = geom::xy(self.poses[i].p);
        let f = self.poses[i].f;
        if inp.tracked.iter().any(|tv| {
            let r = geom::sub(geom::xy(tv.pos), q);
            let long = geom::dot(r, f);
            if !(-5.0..=clear).contains(&long) || ObsRef::Tracked(tv.entity.0) == a.obs {
                return false;
            }
            geom::obb_overlap(&corridor, &tracked_box(tv))
                || ((geom::dot(r, right) + a.lat - offset).abs() < 3.0
                    && (geom::dot([tv.vel[0], tv.vel[1]], f) < 1.0 || long < d + 10.0))
        }) {
            return PassTry::NoGap;
        }
        for (j, b) in self.agents.iter().enumerate() {
            if j == i || b.s <= skip_until && b.seg == a.seg {
                continue;
            }
            let (pb, fb) = self.pose_of(map, b);
            if geom::dist(geom::xy(pb), corridor.c) < corridor.hl + 6.0
                && geom::obb_overlap(&corridor, &car_box(geom::xy(pb), fb, 0.2))
            {
                return PassTry::NoGap;
            }
        }
        let s_m = os.project(q).s;
        if !self.oncoming_clear(map, i, opp, s_m, clear) {
            return PassTry::NoGap;
        }
        let obs = a.obs;
        self.agents[i].manoeuvre = Some(Box::new(Manoeuvre::Pass(Pass {
            opp,
            offset,
            skip_until_s: skip_until,
            obs_rear_s,
            end_s,
            obs_box,
            obs,
            opp_s: s_m,
            returning: false,
            stall_s: 0.0,
        })));
        PassTry::Started
    }

    /// Try to start turning around into the opposing lane.
    fn try_turn(&mut self, map: &CityMap, inp: &TrafficInputs, i: usize) -> bool {
        let a = &self.agents[i];
        let seg = map.seg(a.seg);
        let Some(mut lane) = seg.lane() else {
            return false;
        };
        // Turning from an inner lane: the opposing lane is across the leftmost one.
        while let Some(l) = map.net.lanes[lane as usize].left {
            lane = l;
        }
        let opp = self.opposite[lane as usize];
        if opp == NONE || a.s < 6.0 || seg.length - a.s < 6.0 {
            return false;
        }
        let Some(road) = self.corridor_at(map, i) else {
            return false;
        };
        let model = self
            .cars
            .get(super::traffic_model_index(a.id, self.cars.len()));
        let shape = match (&a.physical, model) {
            (Some(_), Some((def, _))) => CarShape::from_def(def),
            _ => CarShape::CIVILIAN,
        };
        let pose = self.poses[i];
        let q = geom::xy(pose.p);
        let goal = [-road.dir[0], -road.dir[1]];
        let obstacles = self.obstacles_near(map, inp, i, 14.0);
        let rear = shape.rear(q, pose.f);
        let Some(plan) = plan_turn(&shape, &road, &obstacles, rear, pose.f, goal) else {
            return false;
        };
        // Oncoming traffic must be able to stop for the turning car; cars in the lanes crossed
        // must not be about to arrive.
        let s_m = map.seg(opp).project(q).s;
        if !self.oncoming_can_stop(map, i, opp, s_m + 6.0) {
            return false;
        }
        let mut l = map.seg(a.seg).lane();
        while let Some(c) = l {
            let cs = map.seg(c);
            let sc = cs.project(q).s;
            // Own lane: a follower still rolling up must be able to stop comfortably short of
            // the room a reversing leg may use. Lanes crossed: nobody arriving within 40 m.
            let arriving = |e: &Entry| {
                if c == a.seg {
                    !turn_follower_stops_short(e.s, e.v, sc)
                } else {
                    e.s > sc - 40.0
                }
            };
            if self.lists[c as usize].iter().any(|e| {
                e.agent as usize != i
                    && ((e.s < sc - CAR_LENGTH_M && e.v > 1.0 && arriving(e))
                        || (e.s - sc).abs() < CAR_LENGTH_M + 2.0 && c != a.seg)
            }) {
                return false;
            }
            l = map.net.lanes[c as usize].left;
        }
        if inp.tracked.iter().any(|t| {
            geom::dist(geom::xy(t.pos), q) < 50.0 && (t.vel[0].abs() + t.vel[1].abs()) > 1.0
        }) {
            return false;
        }
        let opp_s = (s_m - 4.0).max(0.0);
        let own_s = a.s - 3.0;
        let z = pose.p[2];
        let (kin, drv) = if a.physical.is_some() {
            (None, Some(TurnDriver::new(shape, road, goal, obstacles)))
        } else {
            (
                Some(KinTurn {
                    plan,
                    leg: 0,
                    u: 0.0,
                    v: 0.0,
                    pause: 0.6,
                    wait_s: 0.0,
                    z,
                }),
                None,
            )
        };
        let blocked = self.agents[i].seg;
        self.mark_blocked(blocked);
        let a = &mut self.agents[i];
        a.manoeuvre = Some(Box::new(Manoeuvre::Turn(Turn {
            opp,
            opp_s,
            own_s,
            road,
            goal,
            kin,
            drv,
        })));
        a.v = 0.0;
        a.acc = 0.0;
        a.pending_lc = None;
        self.stats.turnarounds += 1;
        true
    }

    /// Try to start reversing back through the junction to the previous lane.
    fn try_reverse(&mut self, map: &CityMap, i: usize) -> bool {
        let a = &self.agents[i];
        let lane = a.seg;
        let ls = map.seg(lane);
        // Last in the lane (or the connector it is stuck in).
        let pos = self.list_pos[i] as usize;
        if self.lists[lane as usize][..pos].iter().any(|e| !e.ghost) {
            return false;
        }
        let via = if ls.is_connector() {
            lane
        } else if a.came_from != NONE && map.seg(a.came_from).next.first() == Some(&lane) {
            a.came_from
        } else {
            match ls.prev.iter().copied().find(|c| map.seg(*c).drivable) {
                Some(c) => c,
                None => return false,
            }
        };
        let SegKind::Connector { from: to, .. } = map.seg(via).kind else {
            return false;
        };
        let ts = map.seg(to);
        let stop_s = ts.length - HALF_L - 1.5;
        if stop_s < CAR_LENGTH_M {
            return false;
        }
        let others = |s: SegId| self.lists[s as usize].iter().any(|e| e.agent as usize != i);
        let entering = !ls.is_connector() && ls.prev.iter().any(|&c| others(c));
        // One car at a time backs into the same spot.
        let spot_claimed = self
            .agents
            .iter()
            .any(|b| matches!(b.manoeuvre.as_deref(), Some(Manoeuvre::Reverse(r)) if r.to == to));
        if entering || spot_claimed {
            return false;
        }
        let drv = if a.physical.is_some() {
            let shape = match self
                .cars
                .get(super::traffic_model_index(a.id, self.cars.len()))
            {
                Some((def, _)) => CarShape::from_def(def),
                None => CarShape::CIVILIAN,
            };
            let back = shape.wheelbase_m * 0.5;
            let mut pts: Vec<V3> = Vec::new();
            let mut push_rev = |sg: &crate::map::Segment, hi: f32, lo: f32| {
                let mut s = hi;
                while s > lo {
                    pts.push(sg.point_at(s).0);
                    s -= 1.0;
                }
                pts.push(sg.point_at(lo).0);
            };
            if lane != via {
                push_rev(ls, (a.s - back).max(0.0), 0.0);
                push_rev(map.seg(via), map.seg(via).length, 0.0);
            } else {
                push_rev(ls, (a.s - back).max(0.0), 0.0);
            }
            push_rev(ts, ts.length, stop_s - back);
            Some(ReverseDriver::new(shape, &pts, REVERSE_MPS))
        } else {
            None
        };
        self.mark_blocked(lane);
        let a = &mut self.agents[i];
        a.manoeuvre = Some(Box::new(Manoeuvre::Reverse(Reverse {
            lane,
            via,
            to,
            stop_s,
            waiting_s: Some(0.0),
            drv,
        })));
        a.v = 0.0;
        a.acc = 0.0;
        a.pending_lc = None;
        self.stats.reverses += 1;
        true
    }

    /// Put agent `i` (done turning) on the opposing lane and plan from there.
    fn finish_turn(&mut self, map: &CityMap, i: usize, closed: &[bool]) {
        let Some(Manoeuvre::Turn(t)) = self.agents[i].manoeuvre.as_deref() else {
            return;
        };
        let opp = t.opp;
        let (p, _) = self.pose_of(map, &self.agents[i]);
        let os = map.seg(opp);
        let pr = os.project(geom::xy(p));
        let plan0 = self.choose_next(map, opp, closed);
        let plan1 = if plan0 != NONE {
            map.seg(plan0).next[0]
        } else {
            NONE
        };
        let a = &mut self.agents[i];
        a.manoeuvre = None;
        a.seg = opp;
        a.s = pr.s.clamp(0.0, os.length);
        a.lat = pr.lateral.clamp(-2.5, 2.5);
        a.lat_target = 0.0;
        a.plan = [plan0, plan1];
        a.permit = NONE;
        a.ghost = None;
        a.stopped_since = None;
        a.came_from = NONE;
        a.blocked_s = 0.0;
        a.wait = Wait::Free;
        a.last_lane_change = self.step_count;
        if a.physical.is_none() {
            a.v = 0.0;
        }
        if let Some(pa) = a.physical.as_deref_mut() {
            pa.controller.resync(pa.vehicle.state().speed_mps.max(0.0));
        }
    }

    /// Reversing agent `i` (waiting): is the way back clear now? The spot on `to` is free, cars
    /// approaching it can stop, nothing is in the junction across or beside its path.
    fn reverse_clear(&self, map: &CityMap, i: usize, r: &Reverse) -> bool {
        let others = |s: SegId| self.lists[s as usize].iter().any(|e| e.agent as usize != i);
        let spot_taken = self.lists[r.to as usize].iter().any(|e| {
            let room = r.stop_s - CAR_LENGTH_M - 1.0 - e.s;
            e.agent as usize != i && (room < 0.0 || e.v * e.v / (2.0 * 4.0) > room)
        });
        let vs = map.seg(r.via);
        let sibling_busy = vs.siblings.iter().any(|&(sib, div)| {
            self.lists[sib as usize]
                .iter()
                .any(|e| e.agent as usize != i && e.s - HALF_L < div + 2.0)
        });
        let cross_busy = vs
            .conflicts
            .iter()
            .any(|&c| others(c) || (self.reservations[c as usize] != NONE));
        let via_busy = r.via != r.lane && others(r.via);
        !(spot_taken || sibling_busy || cross_busy || via_busy)
    }

    /// Reversing agent `i`: count the wait, start once clear, give up after a while. Returns
    /// whether it may move now.
    fn reverse_wait(&mut self, map: &CityMap, i: usize, dt: f32) -> bool {
        let Some(Manoeuvre::Reverse(r)) = self.agents[i].manoeuvre.as_deref() else {
            return false;
        };
        let Some(w) = r.waiting_s else {
            return true;
        };
        let clear = self.reverse_clear(map, i, r);
        let a = &mut self.agents[i];
        if clear {
            if let Some(Manoeuvre::Reverse(r)) = a.manoeuvre.as_deref_mut() {
                r.waiting_s = None;
            }
            return true;
        }
        if w + dt > REVERSE_WAIT_MAX_S {
            a.manoeuvre = None;
            a.blocked_s = 0.0;
            a.retry_s = 10.0;
            self.stats.reverses -= 1;
        } else if let Some(Manoeuvre::Reverse(r)) = a.manoeuvre.as_deref_mut() {
            r.waiting_s = Some(w + dt);
        }
        false
    }

    /// Agent `i` has reversed into the previous lane: plan again from there.
    fn finish_reverse(&mut self, map: &CityMap, i: usize, closed: &[bool]) {
        let Some(Manoeuvre::Reverse(r)) = self.agents[i].manoeuvre.as_deref() else {
            return;
        };
        let (to, stop_s) = (r.to, r.stop_s);
        let plan0 = self.choose_next(map, to, closed);
        let plan1 = if plan0 != NONE {
            map.seg(plan0).next[0]
        } else {
            NONE
        };
        let t = self.step_count;
        let a = &mut self.agents[i];
        a.manoeuvre = None;
        a.seg = to;
        if a.physical.is_none() {
            a.s = stop_s;
            a.v = 0.0;
        }
        a.lat_target = 0.0;
        a.plan = [plan0, plan1];
        a.permit = NONE;
        a.ghost = None;
        a.stopped_since = Some(t);
        a.came_from = NONE;
        a.blocked_s = 0.0;
        a.wait = Wait::Free;
        if let Some(pa) = a.physical.as_deref_mut() {
            pa.controller.resync(0.0);
        }
    }

    /// Kinematic agent `i` in a turn or reverse: advance it. Returns `false` if it has no such
    /// manoeuvre (normal integration applies).
    pub(super) fn step_manoeuvre_kinematic(
        &mut self,
        map: &CityMap,
        inp: &TrafficInputs,
        i: usize,
        dt: f32,
    ) -> bool {
        match self.agents[i].manoeuvre.as_deref() {
            Some(Manoeuvre::Turn(_)) => {
                self.step_kin_turn(map, inp, i, dt);
                true
            }
            Some(Manoeuvre::Reverse(_)) => {
                self.step_kin_reverse(map, inp, i, dt);
                true
            }
            _ => false,
        }
    }

    fn step_kin_turn(&mut self, map: &CityMap, inp: &TrafficInputs, i: usize, dt: f32) {
        // Before each leg, make sure nothing has moved into its sweep (re-plan after a while).
        let starting = matches!(
            self.agents[i].manoeuvre.as_deref(),
            Some(Manoeuvre::Turn(Turn { kin: Some(k), .. })) if k.pause > 0.0 && k.pause <= dt
        );
        let obstacles = if starting {
            self.obstacles_near(map, inp, i, 14.0)
        } else {
            Vec::new()
        };
        let mut done = false;
        let mut v_out = 0.0;
        if let Some(Manoeuvre::Turn(t)) = self.agents[i].manoeuvre.as_deref_mut() {
            let Some(k) = t.kin.as_mut() else { return };
            let shape = CarShape::CIVILIAN;
            if k.pause > 0.0 {
                k.pause -= dt;
                k.v = 0.0;
                if k.pause <= 0.0 && k.leg < k.plan.legs.len() {
                    let leg = k.plan.legs[k.leg];
                    let mut d = 0.0;
                    let mut clear = true;
                    while d <= leg.length_m && clear {
                        let (r, f) = k.plan.pose(k.leg, d);
                        let b = car_box(shape.centre(r, f), f, 0.2);
                        clear = !obstacles.iter().any(|o| geom::obb_overlap(&b, o));
                        d += 0.5;
                    }
                    if !clear {
                        k.pause = 0.5;
                        k.wait_s += 0.5;
                        if k.wait_s > 12.0 {
                            let (r, f) = k.plan.pose(k.leg, 0.0);
                            if let Some(p) = plan_turn(&shape, &t.road, &obstacles, r, f, t.goal) {
                                k.plan = p;
                                k.leg = 0;
                                k.u = 0.0;
                                k.wait_s = 0.0;
                            }
                        }
                    }
                }
            } else if k.leg < k.plan.legs.len() {
                let leg = k.plan.legs[k.leg];
                let left = (leg.length_m - k.u).max(0.0);
                let target = CRAWL_MPS.min((2.0 * 0.8 * left).sqrt() + 0.1);
                k.v += (target - k.v).clamp(-1.5 * dt, 1.0 * dt);
                k.u += k.v * dt;
                if k.u >= leg.length_m {
                    k.leg += 1;
                    k.u = 0.0;
                    k.v = 0.0;
                    k.pause = 0.7;
                }
            }
            if k.leg >= k.plan.legs.len() {
                done = true;
            }
            v_out = k.v;
        }
        self.agents[i].v = v_out;
        self.agents[i].acc = 0.0;
        if done {
            self.finish_turn(map, i, inp.closed);
            self.stats.turnaround_completed();
        }
        self.update_stop_state(map, i, dt);
    }

    fn step_kin_reverse(&mut self, map: &CityMap, inp: &TrafficInputs, i: usize, dt: f32) {
        if !self.reverse_wait(map, i, dt) {
            let a = &mut self.agents[i];
            a.v = 0.0;
            a.acc = 0.0;
            self.update_stop_state(map, i, dt);
            return;
        }
        let Some(Manoeuvre::Reverse(r)) = self.agents[i].manoeuvre.as_deref() else {
            return;
        };
        let (lane, via, to, stop_s) = (r.lane, r.via, r.to, r.stop_s);
        let a = &self.agents[i];
        // Room behind: the nearest car behind on this segment or the ones reversed into next.
        let mut gap = f32::INFINITY;
        let behind = |seg: SegId, s_me: f32, extra: f32| {
            self.lists[seg as usize]
                .iter()
                .filter(|e| e.agent as usize != i && e.s < s_me + extra)
                .map(|e| s_me + extra - e.s - CAR_LENGTH_M)
                .fold(f32::INFINITY, f32::min)
        };
        let in_lane = a.seg == lane && lane != via;
        gap = gap.min(behind(a.seg, a.s, 0.0));
        if in_lane {
            gap = gap.min(behind(via, a.s + map.seg(via).length, 0.0));
        }
        if a.seg != to {
            let extra = if in_lane { map.seg(via).length } else { 0.0 };
            gap = gap.min(behind(to, a.s + extra + map.seg(to).length, 0.0));
        }
        let left = if a.seg == to {
            a.s - stop_s
        } else if a.seg == via {
            a.s + map.seg(to).length - stop_s
        } else {
            a.s + map.seg(via).length + map.seg(to).length - stop_s
        };
        let target = if gap < 1.5 {
            0.0
        } else {
            REVERSE_MPS
                .min((2.0 * 1.0 * left.max(0.0)).sqrt())
                .min((2.0 * 2.0 * (gap - 1.5)).sqrt())
        };
        let a = &mut self.agents[i];
        a.v = (a.v + (target - a.v).clamp(-2.5 * dt, 1.0 * dt)).max(0.0);
        a.acc = 0.0;
        a.lat_target = 0.0;
        let step = 1.3 * dt;
        a.lat += (0.0 - a.lat).clamp(-step, step);
        a.s -= a.v * dt;
        if a.seg == lane && lane != via && a.s < 0.0 {
            a.seg = via;
            a.s += map.seg(via).length;
        }
        if a.seg == via && a.s < 0.0 {
            a.seg = to;
            a.s += map.seg(to).length;
        }
        if a.seg == to && a.s <= stop_s {
            a.s = stop_s;
            self.finish_reverse(map, i, inp.closed);
        }
        self.update_stop_state(map, i, dt);
    }

    /// Physical agent `i` in a turn or reverse: keep its lane position consistent with the body
    /// (`q` = position, `speed` = signed forward speed). Returns `true` if handled.
    pub(super) fn sync_manoeuvre(&mut self, map: &CityMap, i: usize, q: V2, speed: f32) -> bool {
        let a = &mut self.agents[i];
        match a.manoeuvre.as_deref() {
            Some(Manoeuvre::Turn(_)) => {
                a.v = speed.abs();
                true
            }
            Some(Manoeuvre::Reverse(r)) => {
                let (lane, via, to) = (r.lane, r.via, r.to);
                a.v = (-speed).max(0.0);
                for _ in 0..3 {
                    let pr = map.seg(a.seg).project(q);
                    if pr.s <= 0.05 && a.seg == lane && lane != via {
                        a.seg = via;
                    } else if pr.s <= 0.05 && a.seg == via {
                        a.seg = to;
                    } else {
                        a.s = pr.s.clamp(0.0, map.seg(a.seg).length);
                        a.lat = pr.lateral;
                        break;
                    }
                    a.s = map.seg(a.seg).length;
                }
                true
            }
            _ => false,
        }
    }

    /// Physical agent `i` in a turn or reverse: drive it. Returns `true` if handled.
    pub(super) fn drive_manoeuvre(&mut self, map: &CityMap, inp: &TrafficInputs, i: usize) -> bool {
        let kind = match self.agents[i].manoeuvre.as_deref() {
            Some(Manoeuvre::Turn(_)) => 1,
            Some(Manoeuvre::Reverse(_)) => 2,
            _ => return false,
        };
        let obstacles = if kind == 1 {
            self.obstacles_near(map, inp, i, 14.0)
        } else {
            Vec::new()
        };
        if kind == 2 && !self.reverse_wait(map, i, TRAFFIC_DT) {
            if let Some(pa) = self.agents[i].physical.as_deref_mut() {
                pa.input = crate::manoeuvre::Crawl::hold(&pa.vehicle.state(), 0.0);
            }
            return true;
        }
        let headlights = map.time.night_factor() > 0.5;
        let a = &mut self.agents[i];
        let Some(pa) = a.physical.as_deref_mut() else {
            return false;
        };
        let st = pa.vehicle.state();
        let (mut input, status) = match a.manoeuvre.as_deref_mut() {
            Some(Manoeuvre::Turn(Turn { drv: Some(d), .. })) => {
                d.set_obstacles(obstacles);
                d.step(&st, TRAFFIC_DT)
            }
            Some(Manoeuvre::Reverse(Reverse { drv: Some(d), .. })) => d.step(&st, TRAFFIC_DT),
            _ => (
                crate::manoeuvre::Crawl::hold(&st, 0.0),
                ManoeuvreStatus::Failed,
            ),
        };
        input.headlights = headlights;
        pa.input = input;
        match (kind, status) {
            (_, ManoeuvreStatus::Running) => {}
            (1, ManoeuvreStatus::Done) => {
                self.finish_turn(map, i, inp.closed);
                self.stats.turnaround_completed();
            }
            (_, ManoeuvreStatus::Done) => self.finish_reverse(map, i, inp.closed),
            (_, ManoeuvreStatus::Failed) => {
                let a = &mut self.agents[i];
                a.manoeuvre = None;
                a.blocked_s = 0.0;
                a.retry_s = 5.0;
            }
        }
        true
    }

    /// Whether agent `i` may be promoted / demoted now (not in the middle of a turn or reverse:
    /// the kinematic and physical versions of those cannot be swapped mid-way).
    pub(super) fn lod_locked(&self, i: usize) -> bool {
        matches!(
            self.agents[i].manoeuvre.as_deref(),
            Some(Manoeuvre::Turn(_)) | Some(Manoeuvre::Reverse(_))
        )
    }
}

impl TrafficStats {
    fn turnaround_completed(&mut self) {
        self.turnarounds_completed += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oncoming_room_grows_with_speed() {
        assert!((turn_oncoming_room(0.0) - (4.0 / 6.0 + 25.0)).abs() < 1e-4);
        // 11 m/s (a car at about 40 km/h): 13²/6 + 25 ≈ 53 m.
        assert!((turn_oncoming_room(11.0) - 53.17).abs() < 0.05);
        assert!(turn_oncoming_room(17.0) > turn_oncoming_room(11.0));
        // A car at 1.25 × a 50 km/h limit needs about as much as the old fixed rule.
        let v = 13.9 * 1.25;
        assert!(turn_oncoming_room(v) >= v * v / 6.0 + 25.0);
    }

    #[test]
    fn slow_follower_far_back_does_not_block_a_turn() {
        let turner = 100.0;
        // Creeping up 30 m back at 3 m/s: stops in 2.25 m.
        assert!(turn_follower_stops_short(turner - 30.0, 3.0, turner));
        // Same speed, already close behind: inside the reversing room.
        assert!(!turn_follower_stops_short(turner - 12.0, 3.0, turner));
        // 39 m back at 11 m/s needs 30 m: too close.
        assert!(!turn_follower_stops_short(turner - 39.0, 11.0, turner));
        // 60 m back at 14 m/s needs 49 m: too close (the old 40 m window missed it).
        assert!(!turn_follower_stops_short(turner - 60.0, 14.0, turner));
    }
}
