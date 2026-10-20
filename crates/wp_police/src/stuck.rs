//! Un-sticking physical police cars (owner playtest rows 44 / 55, lead round 12b: "police cars
//! getting stuck", "getting lost on the way").
//!
//! A physical unit that wants to move (planned speed above [`WANT_MPS`], not holding for traffic
//! or a light, in service) but has been slower than [`SLOW_MPS`] for [`STUCK_AFTER_S`] is
//! **stuck**; so is one that has waited [`HELD_AFTER_S`] behind a stopped vehicle it can't
//! pass (not queued at a red light). Each stuck event is classified by what the car touched last ([`StuckCause`]) and
//! logged in `PoliceStats::stuck`. Recovery, in order:
//! 1. reverse with opposite lock for [`REVERSE_M`] (at most [`REVERSE_MAX_S`]) and drive on;
//! 2. stuck again within [`SAME_SPOT_M`] of the first event (or held behind a vehicle at once):
//!    re-route with the road segment it is on blocked for this unit ([`BLOCK_SEGMENT_S`]);
//! 3. still stuck [`GIVE_UP_S`] after the first event and out of every racer's sight: despawn.
//!    A call it was answering is dispatched again (usually to a fresh responder out of sight
//!    nearer the racer); patrols are refilled the same way.
//!
//! Moving [`PROGRESS_M`] from the first event clears the episode. Same behaviour in both damage
//! modes. Headless (built-in driver) units can't get pinned and skip all of this.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::events::UnitId;
use crate::graph::{dist, SegmentId};
use crate::time::SimTime;

/// A unit planning to go faster than this wants to move, m/s.
pub const WANT_MPS: f32 = 1.5;
/// Slower than this while wanting to move counts toward stuck, m/s.
pub const SLOW_MPS: f32 = 1.0;
/// Seconds slow before a stuck event.
pub const STUCK_AFTER_S: f32 = 2.0;
/// Seconds waiting behind a stopped vehicle (not a red-light queue) before a stuck event: a
/// dead or abandoned car the unit can't pass, m.
pub const HELD_AFTER_S: f32 = 20.0;
/// Same for a pursuing / intercepting unit held behind a stopped racer or police car (two
/// chase units nose to nose), s.
pub const HELD_CHASE_S: f32 = 3.0;
/// Reverse this far before driving on, m.
pub const REVERSE_M: f32 = 4.0;
/// A responder yielding to the racer it was sent for, nose to nose with it stopped this long,
/// backs out [`YIELD_REVERSE_M`] to clear a narrow road, s.
pub const YIELD_DEADLOCK_S: f32 = 2.0;
/// See [`YIELD_DEADLOCK_S`], m.
pub const YIELD_REVERSE_M: f32 = 8.0;
/// A unit whose steering target is behind it (cosine of the angle below this) turns round
/// with a reverse swing instead of a wide forward U-turn (row 60).
pub const SWING_BEHIND_COS: f32 = -0.3;
/// ...when slower than this, m/s (faster, it brakes into the swing).
pub const SWING_MAX_MPS: f32 = 8.0;
/// The swing ends when the car faces the target this well (cosine) or after this long, s.
pub const SWING_DONE_COS: f32 = 0.5;
/// See [`SWING_DONE_COS`].
pub const SWING_MAX_S: f32 = 3.5;
/// No new swing for this long after one, s (no reverse/forward oscillation).
pub const SWING_COOLDOWN_S: f32 = 3.0;
/// Faster than this, a unit turns round with a forward U-turn instead of a reverse swing, m/s.
pub const UTURN_MIN_MPS: f32 = 3.0;
/// Speed held through a U-turn, m/s.
pub const UTURN_MPS: f32 = 6.0;
/// Handbrake pulled for this long at the start of a U-turn from above `UTURN_MPS`, s.
pub const UTURN_HANDBRAKE_S: f32 = 0.6;
/// A U-turn gives up after this long, s.
pub const UTURN_MAX_S: f32 = 4.5;
/// Give up reversing after this long, s.
pub const REVERSE_MAX_S: f32 = 3.0;
/// A second event this close to the first one re-routes around the segment, m.
pub const SAME_SPOT_M: f32 = 15.0;
/// How long a segment stays blocked for the unit that got stuck on it, s.
pub const BLOCK_SEGMENT_S: f32 = 30.0;
/// Despawn (out of sight) a unit still stuck this long after its first event, s.
pub const GIVE_UP_S: f32 = 8.0;
/// Driving this far from the first event ends the stuck episode, m.
pub const PROGRESS_M: f32 = 20.0;
/// Contacts this recent explain a stuck event, s.
const CONTACT_WINDOW_S: f32 = 2.0;
/// A world contact point this close to a parked-car / cone footprint is that prop, m.
const PROP_MARGIN_M: f32 = 0.6;
/// A world contact below this height above the chassis origin is a kerb or step, m.
const LOW_CONTACT_M: f32 = 0.35;
/// A low contact this close to a junction node is a junction step, m.
const JUNCTION_STEP_M: f32 = 22.0;

/// What a stuck unit was up against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum StuckCause {
    /// Damaged: engine dead, a wheel lost or bent, or health very low.
    Damage,
    /// Touching another vehicle (traffic, another unit, a racer).
    Vehicle,
    /// A parked-car or cone prop.
    ParkedProp,
    /// A low step near a junction (lane meeting the junction surface).
    JunctionStep,
    /// A low step away from junctions (kerb, sidewalk edge).
    Kerb,
    /// Something tall and static (building, barrier, rail).
    Wall,
    /// The underside on the ground (high-centred, steep grade).
    Terrain,
    /// No recent contact: wheelspin on a grade, or the controller.
    NoContact,
}

impl StuckCause {
    /// Every cause, for reports.
    pub const ALL: [StuckCause; 8] = [
        StuckCause::Damage,
        StuckCause::Vehicle,
        StuckCause::ParkedProp,
        StuckCause::JunctionStep,
        StuckCause::Kerb,
        StuckCause::Wall,
        StuckCause::Terrain,
        StuckCause::NoContact,
    ];
}

/// How a stuck event was handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnstickStep {
    /// Reversed with opposite lock.
    Reverse,
    /// Reversed and re-routed around the segment.
    Reroute,
    /// Despawned out of sight (respawn via dispatch / pool).
    Respawn,
    /// Gave up time reached but a racer could see it: kept trying.
    InSight,
    /// Recovery switched off ([`crate::PoliceSystem::set_unstick_recovery`]): logged only.
    Logged,
}

/// One stuck event (`PoliceStats::stuck`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StuckRecord {
    /// Unit.
    pub unit: UnitId,
    /// When.
    pub at: SimTime,
    /// Where, m.
    pub pos: [f32; 2],
    /// Why.
    pub cause: StuckCause,
    /// What was done.
    pub step: UnstickStep,
    /// Height of the last world contact above the chassis origin, m (calibration).
    pub contact_h_m: Option<f32>,
}

/// The last static-world contact of a unit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) struct WorldContact {
    pub(crate) at: SimTime,
    pub(crate) point: [f32; 3],
    /// Unit normal from the unit toward the obstacle.
    pub(crate) normal: [f32; 3],
}

/// Recovery manoeuvre in progress.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub(crate) enum Phase {
    /// Normal driving.
    #[default]
    Drive,
    /// Turning round toward a target behind the car (J / three-point turn, owner playtest
    /// row 60): reversing with lock until it faces the target or `until`.
    Swing { until: SimTime, steer: f32 },
    /// Turning round from speed: a forward U-turn at full lock (handbrake pulled at the start),
    /// until it faces the target or `until`; `since` is when it began.
    UTurn {
        since: SimTime,
        until: SimTime,
        steer: f32,
    },
    /// Push-through (owner playtest row 61): shoving a stopped civilian or parked car aside at
    /// low speed, from where, until when.
    Push { from: [f32; 2], until: SimTime },
    /// Backing out: from where, how far, until when, with which steer.
    Reverse {
        from: [f32; 2],
        dist_m: f32,
        until: SimTime,
        steer: f32,
    },
}

/// Per-unit un-sticking state.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct UnstickState {
    pub(crate) slow_s: f32,
    /// Seconds yielding nose to nose with a stopped racer.
    pub(crate) yield_s: f32,
    /// No new turn-round swing before this.
    #[serde(default)]
    pub(crate) swing_ready: SimTime,
    /// No new push-through before this.
    #[serde(default)]
    pub(crate) push_ready: SimTime,
    /// Seconds holding behind a stopped vehicle outside a red-light queue.
    pub(crate) held_s: f32,
    pub(crate) phase: Phase,
    /// Stuck events in this episode.
    pub(crate) attempts: u32,
    pub(crate) first: Option<(SimTime, [f32; 2])>,
    pub(crate) last_steer: f32,
    pub(crate) world_contact: Option<WorldContact>,
    pub(crate) car_contact: Option<SimTime>,
    /// Segments this unit routes around, until when.
    pub(crate) blocked: Vec<(SegmentId, SimTime)>,
    /// Re-plan the route at the next think step.
    pub(crate) reroute: bool,
    /// Despawn when out of sight.
    pub(crate) give_up: bool,
}

impl UnstickState {
    /// Whether the unit is turning round (reverse swing or U-turn) right now.
    pub fn swinging(&self) -> bool {
        matches!(self.phase, Phase::Swing { .. } | Phase::UTurn { .. })
    }

    /// Whether the unit is pushing a blocker aside right now (row 61).
    pub fn pushing(&self) -> bool {
        matches!(self.phase, Phase::Push { .. })
    }

    /// Whether the unit is backing out right now.
    pub fn reversing(&self) -> bool {
        matches!(self.phase, Phase::Reverse { .. })
    }

    /// Segments blocked for routing at `now`.
    pub(crate) fn blocked_at(&self, now: SimTime) -> Vec<SegmentId> {
        self.blocked
            .iter()
            .filter(|(_, until)| now < *until)
            .map(|(s, _)| *s)
            .collect()
    }

    /// Note forward progress: far enough from the first event ends the episode.
    pub(crate) fn progress(&mut self, pos: [f32; 2]) {
        if let Some((_, p)) = self.first {
            if dist(p, pos) > PROGRESS_M {
                self.first = None;
                self.attempts = 0;
                self.give_up = false;
            }
        }
    }
}

/// Footprint of a parked car or cone.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PropBox {
    /// Centre, m.
    pub pos: [f32; 2],
    /// Half length / width along its own axes, m.
    pub half: [f32; 2],
    /// Yaw, as (sin, cos).
    pub rot: [f32; 2],
}

impl PropBox {
    /// Distance from `p` to the box outline (0 inside), m.
    pub fn distance(&self, p: [f32; 2]) -> f32 {
        let d = [p[0] - self.pos[0], p[1] - self.pos[1]];
        let (s, c) = (self.rot[0], self.rot[1]);
        let lx = (c * d[0] + s * d[1]).abs() - self.half[0];
        let ly = (-s * d[0] + c * d[1]).abs() - self.half[1];
        let ox = lx.max(0.0);
        let oy = ly.max(0.0);
        (ox * ox + oy * oy).sqrt()
    }
}

/// Parked cars and cones of the city, on a coarse grid (they are solid in `wp_sim` and police
/// cars hit them; the static-world contact alone can't tell a prop from a wall).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PropIndex {
    boxes: Vec<PropBox>,
    grid: BTreeMap<(i32, i32), Vec<u32>>,
}

/// Grid cell size, m.
const PROP_CELL_M: f32 = 25.0;

fn cell(p: [f32; 2]) -> (i32, i32) {
    (
        (p[0] / PROP_CELL_M).floor() as i32,
        (p[1] / PROP_CELL_M).floor() as i32,
    )
}

impl PropIndex {
    /// Index `boxes`.
    pub fn new(boxes: Vec<PropBox>) -> Self {
        let mut grid: BTreeMap<(i32, i32), Vec<u32>> = BTreeMap::new();
        for (i, b) in boxes.iter().enumerate() {
            grid.entry(cell(b.pos)).or_default().push(i as u32);
        }
        Self { boxes, grid }
    }

    /// Props indexed.
    pub fn len(&self) -> usize {
        self.boxes.len()
    }

    /// No props.
    pub fn is_empty(&self) -> bool {
        self.boxes.is_empty()
    }

    /// Indices of props in the grid cells around `p` (everything within one cell, ≥ 25 m),
    /// ascending.
    pub fn around(&self, p: [f32; 2], out: &mut Vec<u32>) {
        let (cx, cy) = cell(p);
        for x in cx - 1..=cx + 1 {
            for y in cy - 1..=cy + 1 {
                if let Some(v) = self.grid.get(&(x, y)) {
                    out.extend_from_slice(v);
                }
            }
        }
    }

    /// Prop by index.
    pub fn get(&self, i: u32) -> &PropBox {
        &self.boxes[i as usize]
    }

    /// Distance from `p` to the nearest prop outline within one grid cell, m.
    pub fn nearest(&self, p: [f32; 2]) -> Option<f32> {
        let (cx, cy) = cell(p);
        let mut best: Option<f32> = None;
        for x in cx - 1..=cx + 1 {
            for y in cy - 1..=cy + 1 {
                for &i in self.grid.get(&(x, y)).map_or(&[][..], |v| v.as_slice()) {
                    let d = self.boxes[i as usize].distance(p);
                    if best.is_none_or(|b| d < b) {
                        best = Some(d);
                    }
                }
            }
        }
        best
    }
}

/// Inputs for classifying one stuck event.
pub(crate) struct ClassifyInput<'a> {
    pub(crate) now: SimTime,
    pub(crate) pos: [f32; 2],
    pub(crate) alt_m: f32,
    pub(crate) damaged: bool,
    pub(crate) vehicle_ahead: bool,
    pub(crate) state: &'a UnstickState,
    pub(crate) props: &'a PropIndex,
    /// Distance to the nearest junction node, m.
    pub(crate) junction_m: f32,
}

/// Why a unit is stuck, and the height of its last world contact above its chassis origin.
pub(crate) fn classify(c: &ClassifyInput) -> (StuckCause, Option<f32>) {
    let recent = |t: SimTime| c.now.since(t) <= CONTACT_WINDOW_S;
    let wc = c.state.world_contact.filter(|w| recent(w.at));
    let h = wc.map(|w| w.point[2] - c.alt_m);
    if c.damaged {
        return (StuckCause::Damage, h);
    }
    if c.state.car_contact.is_some_and(recent) || c.vehicle_ahead {
        return (StuckCause::Vehicle, h);
    }
    let Some(w) = wc else {
        // Pinned against a prop without a fresh contact (resting on it).
        if c.props.nearest(c.pos).is_some_and(|d| d < 3.0) {
            return (StuckCause::ParkedProp, h);
        }
        return (StuckCause::NoContact, h);
    };
    if c.props
        .nearest([w.point[0], w.point[1]])
        .is_some_and(|d| d <= PROP_MARGIN_M)
    {
        return (StuckCause::ParkedProp, h);
    }
    let h_m = w.point[2] - c.alt_m;
    let cause = if w.normal[2].abs() >= 0.6 {
        StuckCause::Terrain
    } else if h_m < LOW_CONTACT_M {
        if c.junction_m <= JUNCTION_STEP_M {
            StuckCause::JunctionStep
        } else {
            StuckCause::Kerb
        }
    } else {
        StuckCause::Wall
    };
    (cause, Some(h_m))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prop_distance_and_index() {
        let b = PropBox {
            pos: [10.0, 0.0],
            half: [2.3, 0.95],
            rot: [0.0, 1.0],
        };
        assert_eq!(b.distance([10.0, 0.0]), 0.0);
        assert!((b.distance([13.3, 0.0]) - 1.0).abs() < 1e-4);
        let idx = PropIndex::new(vec![b]);
        assert!((idx.nearest([10.0, 2.95]).unwrap() - 2.0).abs() < 1e-4);
        assert!(idx.nearest([200.0, 0.0]).is_none());
    }

    #[test]
    fn classify_by_contact() {
        let props = PropIndex::new(vec![PropBox {
            pos: [0.0, 5.0],
            half: [2.3, 0.95],
            rot: [0.0, 1.0],
        }]);
        let now = SimTime::ZERO.plus(10.0);
        let mut st = UnstickState::default();
        let mut input = |point: [f32; 3], normal: [f32; 3], junction_m: f32| {
            st.world_contact = Some(WorldContact {
                at: now,
                point,
                normal,
            });
            let c = ClassifyInput {
                now,
                pos: [0.0, 0.0],
                alt_m: 0.5,
                damaged: false,
                vehicle_ahead: false,
                state: &st,
                props: &props,
                junction_m,
            };
            classify(&c).0
        };
        // Chassis origin at z = 0.5: a contact at 0.6 is low (kerb / step), at 1.2 tall.
        assert_eq!(
            input([0.0, 4.2, 0.6], [0.0, 1.0, 0.0], 100.0),
            StuckCause::ParkedProp
        );
        assert_eq!(
            input([30.0, 0.0, 1.2], [1.0, 0.0, 0.0], 100.0),
            StuckCause::Wall
        );
        assert_eq!(
            input([30.0, 0.0, 0.6], [1.0, 0.0, 0.0], 100.0),
            StuckCause::Kerb
        );
        assert_eq!(
            input([30.0, 0.0, 0.6], [1.0, 0.0, 0.0], 10.0),
            StuckCause::JunctionStep
        );
        assert_eq!(
            input([30.0, 0.0, 0.2], [0.0, 0.0, -1.0], 100.0),
            StuckCause::Terrain
        );
    }
}
