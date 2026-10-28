//! Loop 9 by day: the commander's highway operations (owner, 2026-10-28: "make Loop 9 daytime
//! police activity more dynamic").
//!
//! A race profile with `dynamic_ops` (`races.toml`: `loop9_day`) hands the commander the
//! course. Outside the night phase it then runs these operations on top of the normal patrol,
//! dispatch and pursuit rules, with the same units, spawns and view checks:
//!
//! - **Rotation**: every [`ROTATE_EVERY_S`] the longest-idle patrol on the course is sent to a
//!   ramp junction 0.8–2 km on, so patrols keep circulating round the loop and past its ramps.
//! - **Speed traps**: up to [`traps`] units parked on the shoulder 0.7–1.7 km ahead of the
//!   racer (out of its sight), radar on: one springs out when the racer passes within
//!   [`TRAP_TRIGGER_M`] (the race's stakeout rule). A trap the racer has left behind is
//!   stood down out of sight.
//! - **Converge** (radio call): when a pursuit starts or its wanted level rises, nearby free
//!   units are called in at once ([`PoliceSystem::call_backup`]), each taking its own way: an
//!   ambush ahead where it can get there first, else straight at the racer's position.
//! - **Flankers**: every [`flank_every_s`] (shorter as the level rises, at most
//!   [`max_flankers`] at once) a unit is put up an on-ramp 0.3–2.5 km ahead of the racer, out
//!   of sight, and comes down it to cut the racer off.
//! - **Peel-off / rejoin**: with more than [`MAX_TRAILING`] units (pursuing, or chasing its
//!   position) strung out behind the
//!   racer, the one furthest back peels off (drops out of the chase; off-ramp ahead of it
//!   noted); after [`PEEL_MIN_S`], once out of the racer's sight, it comes back down an
//!   on-ramp ahead (moved there like a stuck unit's reset, between two spots no racer sees).
//!   With no free ramp spot it carries on home like any unit out of service.
//!
//! Roadblocks and spike strips ahead on the predicted route are the AI captain's as before
//! (now sealing the road but for the barricaded gap). Fairness: every operation is capped by
//! the race's ground-unit cap, flankers and traps by the wanted level; units still despawn
//! when far out of the chase; nothing ever appears in a racer's view; the escape rules are
//! unchanged.
//!
//! Deterministic: fixed unit order, the police RNG only, sim time only.

use std::collections::BTreeMap;

use wp_contracts::{DayPhase, EntityId, WorldQuery};

use crate::events::{UnitId, UnitKind};
use crate::graph::{add, dist, dot, len, normalize, scale, sub, xy, NodeId, SegmentId};
use crate::race::{RaceCourse, Stakeout};
use crate::radio::{RadioCue, RadioTier};
use crate::time::SimTime;
use crate::unit::{Assignment, UnitRole};
use crate::PoliceSystem;

/// A patrol is rotated this often, s.
pub const ROTATE_EVERY_S: f32 = 12.0;
/// Traps are reviewed this often, s.
pub const TRAP_EVERY_S: f32 = 6.0;
/// A speed trap springs on a racer passing this close with line of sight, m.
pub const TRAP_TRIGGER_M: f32 = 160.0;
/// Peel-offs are considered this often, s.
pub const PEEL_EVERY_S: f32 = 4.0;
/// Pursuers more than this many strung out behind the racer: the one furthest back peels off.
pub const MAX_TRAILING: usize = 1;
/// At most this many trailing pursuers peel off in one round.
pub const PEEL_AT_ONCE: usize = 2;
/// A peeled unit drops back at least this long before it may rejoin, s.
pub const PEEL_MIN_S: f32 = 4.0;
/// An operation label is kept on a unit this long at most, s.
const OP_HOLD_S: f32 = 45.0;
/// A course junction this close to the course counts; a branch leaving it further than
/// [`RAMP_OFF_M`] from the course [`RAMP_WALK_M`] down is a ramp or side road, m.
const RAMP_NODE_M: f32 = 15.0;
/// See [`RAMP_NODE_M`].
const RAMP_OFF_M: f32 = 12.0;
/// See [`RAMP_NODE_M`].
const RAMP_WALK_M: f32 = 150.0;
/// A flanker starts this far up its on-ramp, m.
const FLANK_UP_RAMP_M: f32 = 140.0;
/// A trailing pursuer is at least this far behind the racer, m.
const TRAILING_M: f32 = 25.0;
/// Shoulder clearance of a speed trap past the paved edge, m.
const TRAP_SHOULDER_M: f32 = 2.0;

/// Seconds between flankers at wanted level 1..=5 (index 0 unused: no flanker without a
/// pursuit).
pub fn flank_every_s(level: u8) -> f32 {
    [60.0, 24.0, 18.0, 14.0, 11.0, 8.0][level.min(5) as usize]
}

/// Flankers at once at a wanted level.
pub fn max_flankers(level: u8) -> usize {
    (level as usize).clamp(1, 3)
}

/// Speed traps out at once at a wanted level (one more with every two levels).
pub fn traps(level: u8) -> usize {
    1 + (level as usize).div_ceil(2).min(2)
}

/// Units called to converge on a radio call at a wanted level.
pub fn converge_units(level: u8) -> u32 {
    1 + (level as u32) / 2
}

/// What a Loop 9 operation has a unit doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LoopOpKind {
    /// Patrol sent round to a ramp junction further on.
    Rotate,
    /// Parked with radar on the shoulder ahead.
    SpeedTrap,
    /// Called in on a radio call to converge on the racer.
    Converge,
    /// Coming down an on-ramp ahead of the racer.
    Flank,
    /// Peeled off the back of the chase at an off-ramp.
    PeelOff,
    /// Back in the chase ahead of the racer after a peel-off.
    Rejoin,
}

impl LoopOpKind {
    /// Short name (views, the variety measurement).
    pub fn label(self) -> &'static str {
        match self {
            LoopOpKind::Rotate => "rotate",
            LoopOpKind::SpeedTrap => "speed-trap",
            LoopOpKind::Converge => "converge",
            LoopOpKind::Flank => "flank",
            LoopOpKind::PeelOff => "peel-off",
            LoopOpKind::Rejoin => "rejoin",
        }
    }
}

/// One unit's current operation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoopOp {
    /// What.
    pub kind: LoopOpKind,
    /// Since when.
    pub since: SimTime,
    /// Where it is headed or posted, m.
    pub point: [f32; 2],
    /// Racer it is about, if any.
    pub racer: Option<EntityId>,
}

/// A ramp or side road at a junction on the course.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ramp {
    /// Distance along the course of the junction, m.
    pub s: f32,
    /// Junction node.
    pub node: NodeId,
    /// First segment of the ramp, at `node`.
    pub segment: SegmentId,
    /// Traffic can come down it onto the course (on-ramp).
    pub on: bool,
    /// Traffic can leave the course along it (off-ramp).
    pub off: bool,
}

/// The commander's Loop 9 operations state (see the module doc).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LoopOps {
    /// Current operation per unit.
    pub(crate) ops: BTreeMap<UnitId, LoopOp>,
    next_rotate: SimTime,
    next_trap: SimTime,
    next_flank: SimTime,
    next_peel: SimTime,
    /// Wanted level last seen per pursued racer (a rise is a radio call to converge).
    levels: BTreeMap<EntityId, u8>,
    /// Operations started, by kind.
    pub(crate) started: BTreeMap<LoopOpKind, u32>,
}

impl PoliceSystem {
    /// Whether the commander runs the Loop 9 operations now: a race whose profile sets
    /// `dynamic_ops`, outside the night phase (night keeps its own behaviour).
    pub(crate) fn loop_ops_active(&self) -> bool {
        self.phase != DayPhase::Night
            && self
                .race
                .as_ref()
                .is_some_and(|r| r.profile.dynamic_ops && r.course.length() > 0.0)
    }

    /// Operations started so far, by label (views and measurements).
    pub fn loop_ops_started(&self) -> Vec<(&'static str, u32)> {
        self.commander
            .loop_ops
            .started
            .iter()
            .map(|(k, n)| (k.label(), *n))
            .collect()
    }

    /// Ramps and side roads at junctions on `course`, by distance along it (see [`Ramp`]).
    pub(crate) fn course_ramps(&self, course: &RaceCourse) -> Vec<Ramp> {
        let g = &self.graph;
        let mut out = Vec::new();
        for (k, n) in g.nodes.iter().enumerate() {
            let node = NodeId(k as u32);
            let (s, d) = course.project(n.pos);
            if d > RAMP_NODE_M {
                continue;
            }
            let mut segs: Vec<SegmentId> = g
                .outgoing(node)
                .iter()
                .chain(g.incoming(node))
                .map(|(seg, _)| *seg)
                .collect();
            segs.sort_by_key(|s| s.0);
            segs.dedup();
            if segs.len() < 3 {
                continue;
            }
            for seg in segs {
                // Ramps peel away from the carriageway gradually: as far down it as it goes
                // (up to RAMP_WALK_M, or its other end).
                let probe = self
                    .walk_road(node, seg, RAMP_WALK_M)
                    .map(|(p, _, _)| p)
                    .unwrap_or_else(|| g.node(g.other_end(seg, node)).pos);
                if course.project(probe).1 < RAMP_OFF_M {
                    continue;
                }
                let other = g.other_end(seg, node);
                out.push(Ramp {
                    s,
                    node,
                    segment: seg,
                    on: g
                        .incoming(node)
                        .iter()
                        .any(|(sg, p)| *sg == seg && *p == other),
                    off: g
                        .outgoing(node)
                        .iter()
                        .any(|(sg, p)| *sg == seg && *p == other),
                });
            }
        }
        out.sort_by(|a, b| a.s.total_cmp(&b.s).then(a.segment.0.cmp(&b.segment.0)));
        out
    }

    /// The commander's Loop 9 operations, at think rate (see the module doc).
    pub(crate) fn loop_ops_think(&mut self, world: &dyn WorldQuery) {
        if !self.loop_ops_active() {
            return;
        }
        self.loop_ops_prune();
        let rp = self.race.as_ref().expect("active");
        let course = rp.course.clone();
        let entrants = rp.entrants.clone();
        // The racer the operations work against: the most wanted entrant the police know of.
        let target = entrants
            .iter()
            .filter_map(|e| self.racers.get(e).map(|r| (r.level.number(), *e)))
            .max_by(|a, b| a.0.cmp(&b.0).then(b.1 .0.cmp(&a.1 .0)))
            .map(|(_, e)| e);
        self.loop_rotate(&course);
        if let Some(racer) = target {
            self.loop_traps(world, &course, racer);
            self.loop_converge(world, &course, racer);
            self.loop_flank(world, &course, racer);
            self.loop_peel(world, &course, racer);
        }
    }

    /// Drop operations of units that are gone, done or too old.
    fn loop_ops_prune(&mut self) {
        let now = self.now;
        let ops = std::mem::take(&mut self.commander.loop_ops.ops);
        let mut keep = BTreeMap::new();
        for (id, op) in ops {
            let Some(u) = self.units.iter().find(|u| u.id == id) else {
                continue;
            };
            if u.wrecked || now.since(op.since) > OP_HOLD_S && op.kind != LoopOpKind::SpeedTrap {
                continue;
            }
            let still = match op.kind {
                LoopOpKind::Rotate => {
                    matches!(u.assignment, Assignment::Patrol { .. })
                        && dist(u.body.pos, op.point) > 30.0
                }
                LoopOpKind::SpeedTrap => u.assignment == Assignment::Idle,
                LoopOpKind::PeelOff => u.assignment == Assignment::OutOfService,
                LoopOpKind::Converge | LoopOpKind::Flank | LoopOpKind::Rejoin => {
                    u.assignment.target().is_some()
                }
            };
            if still {
                keep.insert(id, op);
            }
        }
        self.commander.loop_ops.ops = keep;
    }

    fn loop_op_start(
        &mut self,
        unit: UnitId,
        kind: LoopOpKind,
        point: [f32; 2],
        racer: Option<EntityId>,
    ) {
        let lo = &mut self.commander.loop_ops;
        lo.ops.insert(
            unit,
            LoopOp {
                kind,
                since: self.now,
                point,
                racer,
            },
        );
        *lo.started.entry(kind).or_default() += 1;
    }

    fn loop_count(&self, kind: LoopOpKind) -> usize {
        self.commander
            .loop_ops
            .ops
            .values()
            .filter(|o| o.kind == kind)
            .count()
    }

    /// Distance from course position `from` on to `to`, m (wrapping on a closed course).
    fn course_ahead(course: &RaceCourse, from: f32, to: f32) -> f32 {
        let l = course.length();
        let p = course.polyline.as_slice();
        let closed = p.len() > 2 && dist(p[0], p[p.len() - 1]) < 60.0;
        if closed {
            (to - from).rem_euclid(l)
        } else {
            to - from
        }
    }

    /// Patrol rotation (see the module doc).
    fn loop_rotate(&mut self, course: &RaceCourse) {
        if self.now < self.commander.loop_ops.next_rotate {
            return;
        }
        self.commander.loop_ops.next_rotate = self.now.plus(ROTATE_EVERY_S);
        let ramps = self
            .race
            .as_ref()
            .map(|r| r.ramps.clone())
            .unwrap_or_default();
        // The patrol longest on its current job.
        let pick = self
            .units
            .iter()
            .enumerate()
            .filter(|(_, u)| {
                u.kind == UnitKind::Car
                    && u.player.is_none()
                    && matches!(u.role, UnitRole::Pool(_))
                    && matches!(u.assignment, Assignment::Patrol { .. } | Assignment::Idle)
                    && !self.commander.loop_ops.ops.contains_key(&u.id)
                    && !self.booking(u.id)
            })
            .min_by(|a, b| {
                a.1.assigned
                    .cmp(&b.1.assigned)
                    .then(a.1.id.0.cmp(&b.1.id.0))
            })
            .map(|(i, _)| i);
        let Some(i) = pick else {
            return;
        };
        let (s, _) = course.project(self.units[i].body.pos);
        let Some(ramp) = ramps.iter().find(|r| {
            let a = Self::course_ahead(course, s, r.s);
            (800.0..=2000.0).contains(&a)
        }) else {
            return;
        };
        let (dest, at) = (ramp.node, self.graph.node(ramp.node).pos);
        self.assign(i, Assignment::Patrol { dest });
        let id = self.units[i].id;
        self.loop_op_start(id, LoopOpKind::Rotate, at, None);
    }

    /// Speed traps (see the module doc).
    fn loop_traps(&mut self, world: &dyn WorldQuery, course: &RaceCourse, racer: EntityId) {
        if self.now < self.commander.loop_ops.next_trap {
            return;
        }
        self.commander.loop_ops.next_trap = self.now.plus(TRAP_EVERY_S);
        let Some(rec) = self.racers.get(&racer) else {
            return;
        };
        let (rpos, speed) = (xy(rec.pos), len([rec.vel[0], rec.vel[1]]));
        let (level, rz) = (rec.level.number(), rec.pos[2]);
        let (rs, _) = course.project(rpos);
        // Stand down traps left behind (out of sight: they leave like any unit going home).
        let behind: Vec<UnitId> = self
            .commander
            .loop_ops
            .ops
            .iter()
            .filter(|(_, o)| o.kind == LoopOpKind::SpeedTrap)
            .filter(|(_, o)| {
                let a = Self::course_ahead(course, rs, course.project(o.point).0);
                a > course.length() * 0.5 && course.length() - a > 400.0
            })
            .map(|(id, _)| *id)
            .collect();
        for id in behind {
            if let Some(i) = self.unit_index(id) {
                self.assign(i, Assignment::OutOfService);
            }
            self.commander.loop_ops.ops.remove(&id);
            if let Some(rp) = self.race.as_mut() {
                if let Some(st) = rp.stakeouts.iter_mut().find(|s| s.unit == id) {
                    st.sprung = true;
                }
            }
        }
        if self.loop_count(LoopOpKind::SpeedTrap) >= traps(level)
            || self.ground_units_alive() >= self.unit_cap()
        {
            return;
        }
        let lead = (speed * 25.0).clamp(700.0, 1700.0);
        for k in 0..4 {
            let s = rs + lead + k as f32 * 150.0;
            let s = if Self::course_ahead(course, 0.0, s) < s {
                s.rem_euclid(course.length())
            } else {
                s.min(course.length())
            };
            let dir = course.dir_at(s);
            let p = course.point_at(s);
            let sec = self.tool_section(&Self::scenario_site(p, dir), rz);
            // On the right shoulder, inside any barrier there.
            let edge = match sec.stop_hi {
                Some(st) => st - crate::tools::UNIT_HALF_W_M - 0.3,
                None => sec.paved_hi + TRAP_SHOULDER_M,
            };
            let right = [dir[1], -dir[0]];
            let post = add(p, scale(right, edge.max(sec.road_hi - 1.2)));
            if self.spawn_in_view(world, post) || !self.spot_clear(post) {
                continue;
            }
            let i = self.spawn_unit(UnitKind::Car, UnitRole::Tool, post, dir);
            self.record_spawn(world, i);
            let unit = self.units[i].id;
            if let Some(rp) = self.race.as_mut() {
                rp.stakeouts.push(Stakeout {
                    unit,
                    post,
                    trigger_m: TRAP_TRIGGER_M,
                    sprung: false,
                });
            }
            self.loop_op_start(unit, LoopOpKind::SpeedTrap, post, Some(racer));
            let place = self.place_phrase(post, dir);
            self.radio(RadioCue::new(
                RadioTier::Routine,
                Some(unit),
                format!("Unit setting up radar on the shoulder, {place}."),
            ));
            break;
        }
    }

    /// Converge on a radio call: a new pursuit or a higher wanted level (see the module doc).
    fn loop_converge(&mut self, world: &dyn WorldQuery, course: &RaceCourse, racer: EntityId) {
        let Some(rec) = self.racers.get(&racer) else {
            return;
        };
        if !rec.pursued() {
            self.commander.loop_ops.levels.remove(&racer);
            return;
        }
        let level = rec.level.number();
        let last = self.commander.loop_ops.levels.get(&racer).copied();
        if last.is_some_and(|l| l >= level) {
            return;
        }
        self.commander.loop_ops.levels.insert(racer, level);
        let before: Vec<UnitId> = self
            .units
            .iter()
            .filter(|u| u.assignment.target() == Some(racer))
            .map(|u| u.id)
            .collect();
        let want = converge_units(level);
        let mut joined = self.call_backup(racer, want);
        // Not enough free units near: the rest come in off the ramps around it, ahead and
        // behind in turn (out of sight), heading for where it is.
        if joined < want {
            joined += self.loop_converge_from_ramps(world, course, racer, want - joined);
        }
        if joined == 0 {
            return;
        }
        let fresh: Vec<(UnitId, [f32; 2])> = self
            .units
            .iter()
            .filter(|u| u.assignment.target() == Some(racer) && !before.contains(&u.id))
            .map(|u| (u.id, u.body.pos))
            .collect();
        for (id, p) in fresh {
            self.loop_op_start(id, LoopOpKind::Converge, p, Some(racer));
        }
        let rp = self.racers[&racer].last_known.unwrap_or([0.0, 0.0]);
        let place = self.place_phrase(rp, [0.0, 0.0]);
        self.radio(RadioCue::new(
            RadioTier::Actionable,
            None,
            format!(
                "Command: all Loop 9 units, converge {place}. {joined} {} coming in from every side.",
                if joined == 1 { "unit" } else { "units" }
            ),
        ));
    }

    /// Up to `n` units put up on-ramps around `racer` (alternately 0.3-2.5 km ahead and
    /// 0.15-1.5 km behind it), out of sight, sent at its position. Returns how many.
    fn loop_converge_from_ramps(
        &mut self,
        world: &dyn WorldQuery,
        course: &RaceCourse,
        racer: EntityId,
        n: u32,
    ) -> u32 {
        let Some(rec) = self.racers.get(&racer) else {
            return 0;
        };
        let Some(target) = rec.last_known else {
            return 0;
        };
        let (rs, _) = course.project(target);
        let ramps: Vec<Ramp> = self
            .race
            .as_ref()
            .map(|r| r.ramps.clone())
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.on)
            .collect();
        let ahead: Vec<Ramp> = ramps
            .iter()
            .filter(|r| (300.0..=2500.0).contains(&Self::course_ahead(course, rs, r.s)))
            .copied()
            .collect();
        let behind: Vec<Ramp> = ramps
            .iter()
            .filter(|r| (150.0..=1500.0).contains(&Self::course_ahead(course, r.s, rs)))
            .copied()
            .collect();
        let mut sent = 0;
        let (mut a, mut b) = (ahead.into_iter(), behind.into_iter());
        let mut turn = 0;
        while sent < n && self.ground_units_alive() < self.unit_cap() {
            let next = if turn % 2 == 0 {
                a.next().or_else(|| b.next())
            } else {
                b.next().or_else(|| a.next())
            };
            turn += 1;
            let Some(ramp) = next else {
                break;
            };
            let Some((pos, dir, _)) = [FLANK_UP_RAMP_M, 90.0, 50.0]
                .into_iter()
                .find_map(|d| self.walk_road(ramp.node, ramp.segment, d))
            else {
                continue;
            };
            if self.spawn_in_view(world, pos) || !self.spot_clear(pos) {
                continue;
            }
            let i = self.spawn_unit(
                UnitKind::Car,
                UnitRole::Reinforcement,
                pos,
                scale(dir, -1.0),
            );
            self.record_spawn(world, i);
            self.assign(
                i,
                Assignment::Intercept {
                    target: racer,
                    point: target,
                    ambush: false,
                },
            );
            sent += 1;
        }
        sent
    }

    /// Flankers down an on-ramp ahead (see the module doc).
    fn loop_flank(&mut self, world: &dyn WorldQuery, course: &RaceCourse, racer: EntityId) {
        let Some(rec) = self.racers.get(&racer) else {
            return;
        };
        if rec.status != crate::heat::PursuitStatus::Pursuit
            || self.now < self.commander.loop_ops.next_flank
        {
            return;
        }
        let level = rec.level.number();
        let (rpos, vel) = (xy(rec.pos), [rec.vel[0], rec.vel[1]]);
        let speed = len(vel);
        if self.loop_count(LoopOpKind::Flank) >= max_flankers(level)
            || self.ground_units_alive() >= self.unit_cap()
        {
            return;
        }
        self.commander.loop_ops.next_flank = self.now.plus(flank_every_s(level));
        let (rs, _) = course.project(rpos);
        let (lo, hi) = (
            (speed * 6.0).max(300.0),
            (speed * 45.0).clamp(1200.0, 2500.0),
        );
        let ramps: Vec<_> = self
            .race
            .as_ref()
            .map(|r| r.ramps.clone())
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.on && (lo..=hi).contains(&Self::course_ahead(course, rs, r.s)))
            .collect();
        for ramp in ramps {
            let Some((pos, dir, _)) = [FLANK_UP_RAMP_M, 90.0, 50.0]
                .into_iter()
                .find_map(|d| self.walk_road(ramp.node, ramp.segment, d))
            else {
                continue;
            };
            if self.spawn_in_view(world, pos) || !self.spot_clear(pos) {
                continue;
            }
            let heading = scale(dir, -1.0);
            let i = self.spawn_unit(UnitKind::Car, UnitRole::Reinforcement, pos, heading);
            self.record_spawn(world, i);
            // Down the ramp to wait past the merge, else straight for the racer's road ahead.
            let junction = self.graph.node(ramp.node).pos;
            let wait = add(junction, scale(course.dir_at(ramp.s), 30.0));
            match self.ambush_route_for(i, rpos, vel) {
                Some((w, track)) => self.start_ambush_for(i, racer, w, track),
                None => self.assign(
                    i,
                    Assignment::Intercept {
                        target: racer,
                        point: wait,
                        ambush: false,
                    },
                ),
            }
            let id = self.units[i].id;
            self.loop_op_start(id, LoopOpKind::Flank, wait, Some(racer));
            let place = self.place_phrase(junction, course.dir_at(ramp.s));
            self.radio(RadioCue::new(
                RadioTier::Actionable,
                Some(id),
                format!("Coming down the on-ramp {place}, I'll cut him off ahead."),
            ));
            return;
        }
    }

    /// A spot up an on-ramp ahead of a racer at `rpos` moving at `vel` where a unit may appear
    /// (out of every racer's sight, clear): `(pos, heading down the ramp, wait point just past
    /// the merge)`.
    fn loop_ramp_ahead(
        &self,
        world: &dyn WorldQuery,
        course: &RaceCourse,
        rpos: [f32; 2],
        vel: [f32; 2],
    ) -> Option<([f32; 2], [f32; 2], [f32; 2])> {
        let speed = len(vel);
        let (rs, _) = course.project(rpos);
        let (lo, hi) = (
            (speed * 6.0).max(300.0),
            (speed * 45.0).clamp(1200.0, 2500.0),
        );
        let ramps = self
            .race
            .as_ref()
            .map(|r| r.ramps.clone())
            .unwrap_or_default();
        for ramp in ramps
            .iter()
            .filter(|r| r.on && (lo..=hi).contains(&Self::course_ahead(course, rs, r.s)))
        {
            let Some((pos, dir, _)) = [FLANK_UP_RAMP_M, 90.0, 50.0]
                .into_iter()
                .find_map(|d| self.walk_road(ramp.node, ramp.segment, d))
            else {
                continue;
            };
            if self.spawn_in_view(world, pos) || !self.spot_clear(pos) {
                continue;
            }
            let junction = self.graph.node(ramp.node).pos;
            let wait = add(junction, scale(course.dir_at(ramp.s), 30.0));
            return Some((pos, scale(dir, -1.0), wait));
        }
        None
    }

    /// Peel-off and rejoin (see the module doc).
    fn loop_peel(&mut self, world: &dyn WorldQuery, course: &RaceCourse, racer: EntityId) {
        let Some(rec) = self.racers.get(&racer) else {
            return;
        };
        let (rpos, vel) = (xy(rec.pos), [rec.vel[0], rec.vel[1]]);
        let pursued = rec.pursued();
        // Rejoin: peeled units at their ramp, or out long enough.
        let peeled: Vec<(UnitId, LoopOp)> = self
            .commander
            .loop_ops
            .ops
            .iter()
            .filter(|(_, o)| o.kind == LoopOpKind::PeelOff)
            .map(|(id, o)| (*id, *o))
            .collect();
        for (id, op) in peeled {
            let Some(i) = self.unit_index(id) else {
                continue;
            };
            // Dropped back for a while and out of the racer's sight: back in down an on-ramp
            // ahead. No free ramp spot: it carries on home (dispatch's out-of-service clean-up).
            if !pursued
                || self.now.since(op.since) < PEEL_MIN_S
                || self.in_racer_view(world, self.units[i].body.pos)
            {
                continue;
            }
            let Some((pos, heading, wait)) = self.loop_ramp_ahead(world, course, rpos, vel) else {
                continue;
            };
            self.commander.loop_ops.ops.remove(&id);
            self.relocate_unit(i, pos, heading);
            self.assign(
                i,
                Assignment::Intercept {
                    target: racer,
                    point: wait,
                    ambush: false,
                },
            );
            self.loop_op_start(id, LoopOpKind::Rejoin, wait, Some(racer));
            let place = self.place_phrase(wait, course.dir_at(course.project(wait).0));
            self.radio(RadioCue::new(
                RadioTier::Routine,
                Some(id),
                format!("Back on, coming down the ramp {place}."),
            ));
        }
        if !pursued || self.now < self.commander.loop_ops.next_peel {
            return;
        }
        self.commander.loop_ops.next_peel = self.now.plus(PEEL_EVERY_S);
        let fwd = normalize(vel);
        let mut trailing: Vec<(f32, usize)> = self
            .units
            .iter()
            .enumerate()
            .filter(|(_, u)| {
                u.kind == UnitKind::Car
                    && u.player.is_none()
                    && !u.wrecked
                    && matches!(
                        u.assignment,
                        Assignment::Pursue { target }
                            | Assignment::Intercept { target, ambush: false, .. }
                            if target == racer
                    )
                    && !self.commander.loop_ops.ops.contains_key(&u.id)
            })
            .filter_map(|(i, u)| {
                let along = dot(sub(u.body.pos, rpos), fwd);
                (along < -TRAILING_M && dist(u.body.pos, rpos) < 1200.0).then_some((along, i))
            })
            .collect();
        if trailing.len() <= MAX_TRAILING {
            return;
        }
        trailing.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let ramps = self
            .race
            .as_ref()
            .map(|r| r.ramps.clone())
            .unwrap_or_default();
        // The furthest back first, keeping MAX_TRAILING, at most PEEL_AT_ONCE per round.
        let excess = trailing.len() - MAX_TRAILING;
        for &(_, i) in trailing.iter().take(excess.min(PEEL_AT_ONCE)) {
            let (us, _) = course.project(self.units[i].body.pos);
            let Some(ramp) = ramps
                .iter()
                .find(|r| r.off && (30.0..=1500.0).contains(&Self::course_ahead(course, us, r.s)))
            else {
                continue;
            };
            let Some((exit, _, _)) = self.walk_road(ramp.node, ramp.segment, 60.0) else {
                continue;
            };
            // Drops out of the chase (dispatch leaves an out-of-service unit alone).
            self.assign(i, Assignment::OutOfService);
            let id = self.units[i].id;
            self.loop_op_start(id, LoopOpKind::PeelOff, exit, Some(racer));
            self.radio(RadioCue::new(
                RadioTier::Routine,
                Some(id),
                "Peeling off at the next exit, I'll come back on ahead of him.".to_string(),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escalation_tables_grow_with_the_level() {
        for l in 1..5u8 {
            assert!(flank_every_s(l + 1) < flank_every_s(l));
            assert!(max_flankers(l + 1) >= max_flankers(l));
            assert!(traps(l + 1) >= traps(l));
            assert!(converge_units(l + 1) >= converge_units(l));
        }
        assert_eq!(max_flankers(5), 3);
        assert_eq!(traps(0), 1);
        assert_eq!(traps(5), 3);
    }

    #[test]
    fn labels_are_distinct() {
        let all = [
            LoopOpKind::Rotate,
            LoopOpKind::SpeedTrap,
            LoopOpKind::Converge,
            LoopOpKind::Flank,
            LoopOpKind::PeelOff,
            LoopOpKind::Rejoin,
        ];
        let mut names: Vec<_> = all.iter().map(|k| k.label()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), all.len());
    }
}
