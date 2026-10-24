//! Patrol & pursuit driving and tactics.
//!
//! Every pursuing unit follows the racer's **breadcrumb trail** (recorded only while some unit,
//! camera or helicopter sees the racer) extended by the racer's predicted path. Units therefore
//! lose the scent when line of sight is lost, which is what makes escapes possible.
//!
//! Speed and tactics escalate by wanted level (`[chase]` in `data/police/pursuit.toml`, owner
//! playtest row 63): corners at the car's limit, late braking, junctions and reds taken fast.
//! Tactics per pursued racer (re-evaluated at think rate, closest pursuer first):
//! - `BoxIn` — racer below `boxin_max_kmh`, ≥ 2 pursuers with budget within 60 m.
//! - `Pit` (L2+) — lead unit, racer below `chase.pit_max_kmh[level]`, straight-ish road ahead.
//! - `Ram` — rear quarter (L2+), hard and side slam (L3+); the second unit flanks alongside.
//! - `RollingBlock` (L3+) — a unit that got ahead of the racer slows in front of it.
//! - `Shadow` — everyone else: tail `chase.shadow_*_m[level]` back.
//! - `Intercept` (assignment) — units off the trail or far behind route over the road graph to
//!   the racer's predicted position 10–20 s out; at L3+ they park across the junction ahead.
//! - Search — racer out of sight: units spread over the junctions around its last known spot.
//! - Disengage — health below `disengage_health[mode]` or a bent wheel / flat: pull over
//!   (handled in `dispatch`).

use crate::radio::{RadioCue, RadioTier};
use wp_contracts::{EntityId, WorldQuery, GRAVITY_MPS2};

use crate::dispatch::ROAD_HALF_WIDTH_M;
use crate::events::{PoliceEvent, PursuitOutcome, UnitId, UnitKind};
use crate::graph::{dist, dot, normalize, perp_left, scale, sub, xy, NodeId, PoiKind, SegmentId};
use crate::heat::{PursuitStatus, RuleInputs, RuleOutcome, WantedLevel};
use crate::unit::{
    plan_unit_speed, step_ground, step_helo, Assignment, BoxSlot, DriveCommand, DriveEnv, RamKind,
    Tactic, Track, UnitRole, LANE_OFFSET_M,
};
use crate::{PoliceSystem, PursuitRecord, RacerObs};

/// A unit that captured a racer is busy booking it this long: it counts for no other capture
/// and takes no new pursuit or intercept (Hollins hairpin: one unit stopped in the racing line
/// arrested the whole field as it queued up behind, one after another), s.
pub const ARREST_BUSY_S: f32 = 30.0;

/// Off the road by `d`, a route joins it at its first node at least this many `d` away.
const REJOIN_NODE_PER_M: f32 = 2.0;
/// ... when that node lies off to the side of its heading (cosine below this) and the car
/// is this far past the road's edge, m.
const REJOIN_SIDE_COS: f32 = 0.5;
/// See [`REJOIN_SIDE_COS`], m.
const REJOIN_KERB_M: f32 = 1.5;

/// Below this speed a racer counts as stopped for interception, m/s.
pub(crate) const STOPPED_RACER_MPS: f32 = 2.0;

/// An ambush wait point this close to the racer's predicted path still counts as on it, m.
const AMBUSH_PATH_TOLERANCE_M: f32 = 8.0;

/// An ambusher joins the chase only facing within ~45° of the racer's line, cos.
const AMBUSH_JOIN_MIN_COS: f32 = 0.7;

/// A pursuer this close to the racer tracks the racer's end of the breadcrumb trail, m.
const NEAR_RACER_M: f32 = 40.0;

/// A responding unit re-routes when its call's position has moved this far from the end of
/// its route (later calls about the same racer), m.
const RESPOND_REROUTE_M: f32 = 40.0;

/// A pursuer this close to the racer with line of sight drives the racer's own line; farther
/// or out of sight it follows the roads ([`PoliceSystem::chase_route`]), m.
const CLOSE_LOS_M: f32 = 70.0;

/// Turning round counts this much road distance on a chase route (plus
/// [`TURN_ROUND_COST_PER_MPS`] × speed), m.
const TURN_ROUND_COST_M: f32 = 20.0;
/// See [`TURN_ROUND_COST_M`], m per m/s.
const TURN_ROUND_COST_PER_MPS: f32 = 3.0;

/// A routed pursuer re-plans its road route at least this often, s.
const CHASE_ROUTE_REFRESH_S: f32 = 1.5;

/// A pursuer that reached the last known position with the racer out of sight this long
/// starts searching (`Intercept` to search points), s.
const LOST_SEARCH_AFTER_S: f32 = 2.0;

/// Interceptors drive at up to this × the local limit (capped by the pursuit cap).
const INTERCEPT_LIMIT_FACTOR: f32 = 2.2;

/// An interceptor faster than this keeps its route rather than switch to one that starts with
/// a turn-around, m/s.
const INTERCEPT_KEEP_MPS: f32 = 6.0;

/// PIT needs the racer's path ahead to stay within ≈ 15° of its line now (cosine).
const PIT_STRAIGHT_COS: f32 = 0.966;

/// Units searching for a racer lost in a tunnel hold this far outside its portals, m.
const PORTAL_HOLD_M: f32 = 30.0;

/// A searching unit moves on to the next junction around the last known position this often, s.
const SEARCH_SWEEP_S: f32 = 10.0;
/// Search area (row 84): junctions this far from the last known position from the start...
const SEARCH_REACH_MIN_M: f32 = 150.0;
/// ...growing with the time unseen at this share of its last known speed (at least
/// [`SEARCH_REACH_MIN_MPS`]: a racer that stopped can drive off again)...
const SEARCH_REACH_FRAC: f32 = 0.5;
/// See [`SEARCH_REACH_FRAC`], m/s.
const SEARCH_REACH_MIN_MPS: f32 = 12.0;
/// ...up to this, m.
const SEARCH_REACH_MAX_M: f32 = 700.0;
/// Search points handed out (predicted exits first, then nearest junctions).
const SEARCH_MAX_POINTS: usize = 16;

/// Braking a shadowing unit plans with when closing on its gap behind the racer, m/s².
const SHADOW_CLOSE_DECEL_MPS2: f32 = 4.0;

/// A responder this close to the reported racer, with it in sight, follows it, m.
const RESPOND_FOLLOW_M: f32 = 150.0;

/// A responder this close to its meeting junction holds short of it while the reported racer
/// is coming to that junction (within [`RESPOND_HOLD_RACER_M`]), m.
const RESPOND_HOLD_SHORT_M: f32 = 30.0;

/// See [`RESPOND_HOLD_SHORT_M`], m.
const RESPOND_HOLD_RACER_M: f32 = 250.0;

/// A responder with the reported racer coming head-on this close pulls over and stops, m.
const RESPOND_YIELD_M: f32 = 70.0;

/// A yielding responder stops only this well lined up with its lane (cos 15°); before that it
/// creeps on at [`RESPOND_YIELD_CREEP_MPS`].
const RESPOND_YIELD_ALIGNED_COS: f32 = 0.966;

/// See [`RESPOND_YIELD_ALIGNED_COS`], m/s.
const RESPOND_YIELD_CREEP_MPS: f32 = 2.0;

/// How far further right a yielding responder pulls over, m.
const RESPOND_YIELD_SHIFT_M: f32 = 1.2;

/// How far ahead a responder looks for a junction to meet a reported racer at, s.
const RESPOND_HORIZON_S: f32 = 12.0;

/// Whether a unit at `pos` facing `heading` has the point `p` (`d` m away) in its field of view
/// (row 89): inside the forward cone, or close enough to see all round.
pub(crate) fn in_view(
    heading: [f32; 2],
    pos: [f32; 2],
    p: [f32; 2],
    d: f32,
    sight: &crate::config::SightConfig,
) -> bool {
    if d <= sight.all_round_m || sight.fov_deg >= 360.0 {
        return true;
    }
    let half = sight.fov_deg * 0.5 * (std::f32::consts::PI / 180.0);
    let to = sub(p, pos);
    dot(heading, to) >= wp_math::sin_cos(half).1 * d
}

/// Pursuit speed: `min(max(target × (1 + margin), floor), cap)`. The floor (10 m/s) lets units
/// close on a stopped racer for the arrest.
pub fn pursuit_speed_cap(target_speed: f32, margin: f32, cap: f32) -> f32 {
    (target_speed * (1.0 + margin)).max(10.0).min(cap)
}

/// Shift every point of a track `lateral_m` to the left of its local direction.
pub fn offset_track(t: &Track, lateral_m: f32) -> Track {
    let n = t.pts.len();
    if n < 2 {
        return t.clone();
    }
    let pts = (0..n)
        .map(|i| {
            let a = t.pts[i.saturating_sub(1).min(n - 2)];
            let b = t.pts[(i.max(1)).min(n - 1)];
            let d = normalize(sub(b, a));
            crate::graph::add(t.pts[i], scale(perp_left(d), lateral_m))
        })
        .collect();
    Track::new(pts)
}

impl PoliceSystem {
    // -----------------------------------------------------------------------------------------
    // Observation & rules
    // -----------------------------------------------------------------------------------------

    /// Seconds of line of sight a ground unit needs to become fully aware of a racer `d` m
    /// away ([`crate::config::AwarenessConfig`]), before the phase's time scale.
    fn awareness_time_s(&self, d: f32) -> f32 {
        let a = &self.cfg.heat.heat.awareness;
        let identify = self.cfg.heat.heat.identify_range_m.get(self.phase);
        let notice = self.cfg.pursuit.los_range_m.get(self.phase).max(identify);
        let t = if d <= a.near_m {
            a.near_s
        } else if d <= identify {
            let k = (d - a.near_m) / (identify - a.near_m).max(1.0);
            a.near_s + (a.identify_s - a.near_s) * k
        } else {
            let k = ((d - identify) / (notice - identify).max(1.0)).min(1.0);
            a.identify_s + (a.notice_s - a.identify_s) * k
        };
        t * a.time_scale.get(self.phase)
    }

    /// Raise or drain unit `unit`'s awareness of `racer` by one think step of `tdt` s
    /// (`rate`: awareness per second while in sight; `None`: out of sight). Returns the new
    /// awareness.
    fn step_awareness(
        &mut self,
        unit: UnitId,
        racer: EntityId,
        rate: Option<f32>,
        tdt: f32,
    ) -> f32 {
        let decay = self.cfg.heat.heat.awareness.decay_per_s;
        let key = (unit, racer);
        let old = self.awareness.get(&key).copied().unwrap_or(0.0);
        let new = match rate {
            Some(r) => (old + r * tdt).min(1.0),
            None => (old - decay * tdt).max(0.0),
        };
        if new > 0.0 {
            self.awareness.insert(key, new);
        } else {
            self.awareness.remove(&key);
        }
        new
    }

    /// Whether unit `unit` is fully aware of `racer` (sees it itself, not over the radio).
    pub(crate) fn unit_aware(&self, unit: UnitId, racer: EntityId) -> bool {
        self.awareness
            .get(&(unit, racer))
            .is_some_and(|a| *a >= 1.0)
    }

    pub(crate) fn update_observations(&mut self, world: &dyn WorldQuery, tdt: f32) {
        let los_range = self.cfg.pursuit.los_range_m.get(self.phase);
        let witness = self.cfg.heat.heat.witness_range_m.get(self.phase);
        let identify = self.cfg.heat.heat.identify_range_m.get(self.phase);
        let helo_range = self.cfg.tools.helicopter.los_range_m.get(self.phase);
        let helo_rate = 1.0
            / (self.cfg.heat.heat.awareness.helo_s
                * self.cfg.heat.heat.awareness.time_scale.get(self.phase))
            .max(1e-3);
        let sight = self.cfg.heat.heat.sight.clone();
        let ids: Vec<EntityId> = self.racers.keys().copied().collect();
        for s in self.helo_sight.values_mut() {
            s.los = false;
        }
        // Units gone since the last think lose what they knew.
        let live: std::collections::BTreeSet<UnitId> = self.units.iter().map(|u| u.id).collect();
        self.awareness.retain(|k, _| live.contains(&k.0));
        let mut helo_los: Vec<UnitId> = Vec::new();
        for id in ids {
            let p = xy(self.racers[&id].pos);
            let pz = self.racers[&id].pos[2];
            let in_pursuit = self.racers[&id].status == PursuitStatus::Pursuit;
            let mut o = RacerObs {
                nearest_unit_m: f32::INFINITY,
                nearest_chaser_m: f32::INFINITY,
                ..Default::default()
            };
            let mut best_spot = f32::INFINITY;
            let mut best_witness = f32::INFINITY;
            for k in 0..self.units.len() {
                let u = &self.units[k];
                let uid = u.id;
                if u.wrecked {
                    self.awareness.remove(&(uid, id));
                    continue;
                }
                let d = dist(u.body.pos, p);
                match u.kind {
                    UnitKind::Car => {
                        // Booking another racer: not there to arrest this one.
                        let booking = self.arresting.get(&u.id).is_some_and(|t| self.now < *t);
                        if !booking {
                            o.nearest_unit_m = o.nearest_unit_m.min(d);
                        }
                        if !booking && u.assignment.target() == Some(id) {
                            o.nearest_chaser_m = o.nearest_chaser_m.min(d);
                        }
                        if u.assignment == Assignment::OutOfService {
                            self.awareness.remove(&(uid, id));
                            continue;
                        }
                        // Busy elsewhere (booking, answering another call): not watching.
                        let busy = booking || self.responding_elsewhere(k, id);
                        let in_sight = d <= los_range
                            && in_view(u.body.heading, u.body.pos, p, d, &sight)
                            && self.sees_at(world, u.body.pos, u.body.alt_m, p, pz);
                        let rate = in_sight.then(|| {
                            let focus = in_view(
                                u.body.heading,
                                u.body.pos,
                                p,
                                d,
                                &crate::config::SightConfig {
                                    fov_deg: sight.focus_fov_deg,
                                    ..sight.clone()
                                },
                            );
                            let f = if focus { 1.0 } else { sight.peripheral_factor };
                            f / self.awareness_time_s(d).max(1e-3)
                        });
                        let aware = self.step_awareness(uid, id, rate, tdt) >= 1.0;
                        // Witnessing a violation is an event: line of sight within the witness
                        // range is enough (no build-up), but a busy unit isn't looking.
                        if in_sight && !busy && d <= witness && d < best_witness {
                            o.witnessed = true;
                            best_witness = d;
                            o.witness = Some(uid);
                        }
                        // A unit already chasing keeps the car in sight as long as it can
                        // see it within the notice range (it knows what it is looking at);
                        // only finding a LOST car again needs an identification.
                        if in_sight {
                            o.tracked = true;
                        }
                        if in_sight && aware && d <= identify {
                            o.identified = true;
                            if d < best_spot {
                                best_spot = d;
                                o.spotter = Some(uid);
                                o.spotter_m = d;
                            }
                        }
                    }
                    UnitKind::Helicopter => {
                        let tracking = matches!(u.assignment, Assignment::HeloTrack { target } if target == id);
                        let los = tracking
                            && d <= helo_range
                            // Looking down from altitude: only tall buildings, high ground,
                            // covered roads (tunnels) and bridge decks (underpasses) block.
                            // The target point follows the car up on terrain like the ground
                            // units' eyes do (a fixed 1.3 m point sits underground on Port
                            // Vesper v2's hills).
                            && world.line_of_sight(
                                [u.body.pos[0], u.body.pos[1], u.body.alt_m],
                                [p[0], p[1], crate::eye_height_at(pz)],
                            );
                        if los {
                            helo_los.push(uid);
                        }
                        if los {
                            o.tracked = true;
                        }
                        if self.step_awareness(uid, id, los.then_some(helo_rate), tdt) >= 1.0 && los
                        {
                            o.identified = true;
                        }
                    }
                }
            }
            // Traffic cameras only report violations (`on_violation`); they never keep a
            // pursuit going or tell units where the racer is (2026-10-23).
            o.seen = if in_pursuit { o.tracked } else { o.identified };
            for &h in &helo_los {
                self.helo_sight
                    .entry(h)
                    .or_insert(crate::dispatch::HeloSight {
                        los: false,
                        visual: false,
                        acquired: false,
                        last_los: self.now,
                        next_report: self.now,
                    })
                    .los = true;
            }
            helo_los.clear();
            self.obs.insert(id, o);
            // Breadcrumbs only while seen.
            if o.seen && self.racers[&id].pursued() {
                let trail = self.trails.entry(id).or_default();
                if trail.pts.last().is_none_or(|l| dist(*l, p) >= 4.0) {
                    let mut pts = std::mem::take(&mut trail.pts);
                    pts.push(p);
                    *trail = Track::new(pts);
                    let len = trail.length();
                    trail.trim_before(len, 1500.0);
                }
            }
        }
    }

    /// Whether unit `i` is answering a call that is not about `racer` (it isn't watching for
    /// other offences on the way).
    fn responding_elsewhere(&self, i: usize, racer: EntityId) -> bool {
        let Assignment::Respond { incident } = self.units[i].assignment else {
            return false;
        };
        self.incidents.iter().any(|inc| {
            inc.id == incident && inc.kind != crate::dispatch::IncidentKind::Report { racer }
        })
    }

    pub(crate) fn update_racer_rules(&mut self, tdt: f32) {
        let ids: Vec<EntityId> = self.racers.keys().copied().collect();
        let cfg = self.cfg.heat.clone();
        for id in ids {
            if self.held.contains(&id) {
                // Driver dropped out: everything stays as it was (see `set_racer_held`).
                self.awareness.retain(|k, _| k.1 != id);
                continue;
            }
            let o = self.obs.get(&id).copied().unwrap_or(RacerObs {
                nearest_unit_m: f32::INFINITY,
                nearest_chaser_m: f32::INFINITY,
                ..Default::default()
            });
            let p = xy(self.racers[&id].pos);
            let in_safehouse = self
                .graph
                .pois_of(PoiKind::Safehouse)
                .any(|s| dist(s.pos, p) <= s.radius_m);
            let speed = {
                let v = self.racers[&id].vel;
                (v[0] * v[0] + v[1] * v[1]).sqrt()
            };
            let district = self.graph.district_at(p);
            let inp = RuleInputs {
                seen: o.seen,
                nearest_unit_m: o.nearest_unit_m,
                nearest_chaser_m: o.nearest_chaser_m,
                in_safehouse,
                speed_mps: speed,
                district,
                phase: self.phase,
                now: self.now,
            };
            let rec = self.racers.get_mut(&id).expect("racer");
            let before = rec.level;
            let outcome = rec.update(&inp, tdt, &cfg);
            let after = rec.level;
            if after != before {
                self.pending.push(PoliceEvent::WantedLevelChanged {
                    racer: id,
                    level: after,
                });
                // Dispatch's level-change summary: `chatter` (every path that raises a level).
            }
            match outcome {
                Some(RuleOutcome::Escaped) => self.end_pursuit(id, PursuitOutcome::Escaped),
                Some(RuleOutcome::Safehouse) => self.end_pursuit(id, PursuitOutcome::Safehouse),
                Some(RuleOutcome::Captured) => self.end_pursuit(id, PursuitOutcome::Captured),
                None => {}
            }
            // A unit that identifies a wanted racer (fully aware of it within the identify
            // range, see `update_observations`) starts a pursuit.
            let rec = &self.racers[&id];
            if !rec.pursued() && rec.level > WantedLevel::None {
                if let Some(spotter) = o.spotter {
                    self.start_pursuit(id, spotter);
                }
            }
        }
    }

    pub(crate) fn start_pursuit(&mut self, racer: EntityId, unit: UnitId) {
        let now = self.now;
        let p = xy(self.racers[&racer].pos);
        {
            let rec = self.racers.get_mut(&racer).expect("racer");
            rec.start_pursuit(now);
            rec.last_seen = Some(now);
            rec.last_known = Some(p);
        }
        self.trails.insert(racer, Track::new(vec![p]));
        // The unit that starts it has identified the car (witnessed, rammed, blown past).
        self.awareness.insert((unit, racer), 1.0);
        self.stats.pursuits.push(PursuitRecord {
            racer,
            start: now,
            end: None,
            outcome: None,
        });
        self.incidents
            .retain(|i| i.kind != crate::dispatch::IncidentKind::Report { racer });
        self.pending.push(PoliceEvent::PursuitStarted {
            racer,
            unit: Some(unit),
        });
        let desc = self.racer_desc(racer);
        // The pursuit call covers the low-heat "units responding" summary (row 144).
        self.radio_director
            .withdraw(&format!("summary:{}", racer.0));
        self.chatter_pursuit_started(racer);
        self.radio(RadioCue::new(
            RadioTier::Escalation,
            None,
            format!("Dispatch: all units, pursuit of {desc}."),
        ));
        if let Some(i) = self.unit_index(unit) {
            // A unit already on another racer's pursuit stays on it (a pack of racers past
            // one unit flipped it to each new one in turn, and it stood in the road, Loop 9
            // round 12c); backup for this one comes from the reinforcement step.
            let busy = self.units[i]
                .assignment
                .target()
                .is_some_and(|t| t != racer && self.racers.get(&t).is_some_and(|r| r.pursued()));
            if self.units[i].player.is_none() && !busy {
                self.units[i].racer_contact = false;
                self.assign(i, Assignment::Pursue { target: racer });
            }
        }
    }

    /// Whether unit `id` is still booking a racer it captured (see [`ARREST_BUSY_S`]).
    pub(crate) fn booking(&self, id: UnitId) -> bool {
        self.arresting.get(&id).is_some_and(|t| self.now < *t)
    }

    /// Units credited with capturing `racer`: the closest in-service ground unit assigned to
    /// it (or, if none is assigned, the closest in-service ground unit) and the other assigned
    /// units as assists, ascending ids. Read-only; does not touch simulation state.
    fn capture_credit(&self, racer: EntityId) -> (Option<UnitId>, Vec<UnitId>) {
        let p = xy(self.racers[&racer].pos);
        let candidates: Vec<&crate::unit::Unit> = self
            .units
            .iter()
            .filter(|u| u.kind == UnitKind::Car && !u.wrecked)
            .collect();
        let on_racer: Vec<&crate::unit::Unit> = candidates
            .iter()
            .copied()
            .filter(|u| u.assignment.target() == Some(racer))
            .collect();
        let pool = if on_racer.is_empty() {
            &candidates
        } else {
            &on_racer
        };
        let capturer = pool
            .iter()
            .min_by(|a, b| {
                crate::graph::dist2(a.body.pos, p)
                    .total_cmp(&crate::graph::dist2(b.body.pos, p))
                    .then(a.id.cmp(&b.id))
            })
            .map(|u| u.id);
        let mut assists: Vec<UnitId> = on_racer
            .iter()
            .map(|u| u.id)
            .filter(|id| Some(*id) != capturer)
            .collect();
        assists.sort();
        (capturer, assists)
    }

    /// Credit for capturing an abandoned racer (`capture_abandoned`): only in-service ground
    /// units assigned to it count; the nearest is the capturer (`None` if no unit was on it),
    /// the others assists, ascending ids.
    pub(crate) fn pursuing_credit(&self, racer: EntityId) -> (Option<UnitId>, Vec<UnitId>) {
        let p = xy(self.racers[&racer].pos);
        let on_racer: Vec<&crate::unit::Unit> = self
            .units
            .iter()
            .filter(|u| {
                u.kind == UnitKind::Car && !u.wrecked && u.assignment.target() == Some(racer)
            })
            .collect();
        let capturer = on_racer
            .iter()
            .min_by(|a, b| {
                crate::graph::dist2(a.body.pos, p)
                    .total_cmp(&crate::graph::dist2(b.body.pos, p))
                    .then(a.id.cmp(&b.id))
            })
            .map(|u| u.id);
        let mut assists: Vec<UnitId> = on_racer
            .iter()
            .map(|u| u.id)
            .filter(|id| Some(*id) != capturer)
            .collect();
        assists.sort();
        (capturer, assists)
    }

    /// End the pursuit of `racer` with `outcome` (rules, total loss, or captain call-off).
    pub(crate) fn end_pursuit(&mut self, racer: EntityId, outcome: PursuitOutcome) {
        let credit = if outcome.is_capture() {
            self.capture_credit(racer)
        } else {
            (None, Vec::new())
        };
        self.end_pursuit_credited(racer, outcome, credit);
    }

    /// [`Self::end_pursuit`] with the capture credit already decided.
    pub(crate) fn end_pursuit_credited(
        &mut self,
        racer: EntityId,
        outcome: PursuitOutcome,
        (capturer, assists): (Option<UnitId>, Vec<UnitId>),
    ) {
        let now = self.now;
        let cfg = self.cfg.heat.clone();
        let was_pursued = self.racers[&racer].pursued();
        let level_at_end = self.racers[&racer].level;
        {
            let rec = self.racers.get_mut(&racer).expect("racer");
            match outcome {
                PursuitOutcome::Captured | PursuitOutcome::CapturedTotalLoss => {
                    rec.heat = 0.0;
                    rec.add_heat(0.0, &cfg);
                }
                PursuitOutcome::Safehouse => {
                    let h = rec.heat * cfg.safehouse.heat_scale_on_enter;
                    rec.heat = 0.0;
                    rec.add_heat(h, &cfg);
                }
                PursuitOutcome::Escaped | PursuitOutcome::CalledOff => {}
            }
            rec.end_pursuit();
        }
        if let Some(p) = self
            .stats
            .pursuits
            .iter_mut()
            .rev()
            .find(|p| p.racer == racer && p.end.is_none())
        {
            p.end = Some(now);
            p.outcome = Some(outcome);
        } else if !was_pursued {
            // Total loss / safehouse while merely wanted: record a zero-length entry.
            self.stats.pursuits.push(PursuitRecord {
                racer,
                start: now,
                end: Some(now),
                outcome: Some(outcome),
            });
        }
        self.trails.remove(&racer);
        if let (Some(u), true) = (capturer, outcome.is_capture()) {
            let ai = self
                .unit_index(u)
                .is_some_and(|i| self.units[i].player.is_none());
            if ai {
                self.arresting.insert(u, now.plus(ARREST_BUSY_S));
            }
        }
        self.pending.push(PoliceEvent::PursuitEnded {
            racer,
            outcome,
            level: level_at_end,
            capturer,
            assists,
        });
        self.pending.push(PoliceEvent::WantedLevelChanged {
            racer,
            level: self.racers[&racer].level,
        });
        let r = &self.racers[&racer];
        let here = self.place_phrase(xy(r.pos), [0.0, 0.0]);
        let by = capturer.map(|u| self.callsign(u));
        let text = match outcome {
            PursuitOutcome::Escaped => format!(
                "Dispatch: we've lost racer #{}, last seen {}.",
                racer.0,
                self.last_seen_phrase(racer)
            ),
            PursuitOutcome::Safehouse => format!("Dispatch: racer #{} went to ground.", racer.0),
            PursuitOutcome::Captured => match by {
                Some(cs) => format!("Dispatch: {cs} has racer #{} in custody {here}.", racer.0),
                None => format!("Dispatch: racer #{} in custody {here}.", racer.0),
            },
            PursuitOutcome::CapturedTotalLoss => {
                format!("Dispatch: racer #{} wrecked {here}, in custody.", racer.0)
            }
            PursuitOutcome::CalledOff => {
                format!("Dispatch: pursuit of racer #{} called off.", racer.0)
            }
        };
        self.radio(
            RadioCue::new(RadioTier::Escalation, None, text)
                .supersedes(format!("level:{}", racer.0)),
        );
        self.release_units(racer);
    }

    // -----------------------------------------------------------------------------------------
    // Planning: assignments → drive commands
    // -----------------------------------------------------------------------------------------

    /// Node to start routing from: the end of the unit's current segment it is heading toward
    /// (respecting one-way where possible).
    pub(crate) fn start_node(&self, pos: [f32; 2], heading: [f32; 2]) -> Option<NodeId> {
        let rp = self.graph.nearest_on_road(pos)?;
        let seg = *self.graph.segment(rp.segment);
        let ab = sub(self.graph.node(seg.b).pos, self.graph.node(seg.a).pos);
        let toward_b = dot(ab, heading) >= 0.0;
        Some(if toward_b && self.graph.can_travel(rp.segment, seg.a) {
            seg.b
        } else if self.graph.can_travel(rp.segment, seg.b) {
            seg.a
        } else {
            seg.b
        })
    }

    /// A road route from a unit's position to `dest` as a drivable track.
    pub(crate) fn route_track(
        &self,
        i: usize,
        dest: NodeId,
        final_point: Option<[f32; 2]>,
    ) -> Track {
        let u = &self.units[i];
        let blocked = u.unstick.blocked_at(self.now);
        if blocked.is_empty() {
            return self
                .route_from_body(u.body.pos, u.body.heading, dest, final_point, !u.internal)
                .0;
        }
        self.route_around(u.body.pos, u.body.heading, dest, final_point, &blocked)
    }

    /// A chase route from unit `i` to `dest`: the shorter of driving on (the junction ahead of
    /// its heading) and turning round (the junction behind it), turning round counted
    /// [`TURN_ROUND_COST_M`] + [`TURN_ROUND_COST_PER_MPS`] × speed longer. The plain route
    /// started at the junction ahead of the unit's heading at that moment: a cop half-way
    /// through a U-turn toward a racer that had passed it was routed on round the block, the
    /// wrong way (owner: "they don't turn around fast enough"). Blocked segments: as
    /// [`Self::route_track`].
    pub(crate) fn chase_route_track(
        &self,
        i: usize,
        dest: NodeId,
        final_point: Option<[f32; 2]>,
    ) -> Track {
        let u = &self.units[i];
        if !u.unstick.blocked_at(self.now).is_empty() {
            return self.route_track(i, dest, final_point);
        }
        let (pos, h) = (u.body.pos, u.body.heading);
        let rejoin = !u.internal;
        let on = self.route_from_body(pos, h, dest, final_point, rejoin).0;
        let back = self
            .route_from_body(pos, scale(h, -1.0), dest, final_point, rejoin)
            .0;
        let cost = TURN_ROUND_COST_M + TURN_ROUND_COST_PER_MPS * u.body.speed;
        if back.is_drivable() && (!on.is_drivable() || back.length() + cost < on.length()) {
            back
        } else {
            on
        }
    }

    /// A route that avoids `blocked` segments (un-sticking, [`crate::stuck`]). Stuck on one of
    /// them, the unit turns around and leaves by the end behind it.
    fn route_around(
        &self,
        pos: [f32; 2],
        heading: [f32; 2],
        dest: NodeId,
        final_point: Option<[f32; 2]>,
        blocked: &[SegmentId],
    ) -> Track {
        let Some(rp) = self.graph.nearest_on_road(pos) else {
            return Track::default();
        };
        let Some(ahead) = self.start_node(pos, heading) else {
            return Track::default();
        };
        let start = if blocked.contains(&rp.segment) {
            self.graph.other_end(rp.segment, ahead)
        } else {
            ahead
        };
        let Some(route) = self.graph.shortest_path_avoiding(start, dest, blocked) else {
            return self.route_from(pos, heading, dest, final_point).0;
        };
        let mut pts = vec![pos];
        pts.extend(route.iter().map(|n| self.graph.node(*n).pos));
        if let Some(p) = final_point {
            pts.push(p);
        }
        Track::new(pts)
    }

    /// A road route from `pos` (facing `heading`) to `dest`, and whether it starts with a
    /// turn-around.
    pub(crate) fn route_from(
        &self,
        pos: [f32; 2],
        heading: [f32; 2],
        dest: NodeId,
        final_point: Option<[f32; 2]>,
    ) -> (Track, bool) {
        self.route_from_body(pos, heading, dest, final_point, false)
    }

    /// [`Self::route_from`] for a car at `pos`; with `rejoin` (a physically driven unit) a car
    /// past the road's edge joins it at a node well ahead, not at one beside it.
    pub(crate) fn route_from_body(
        &self,
        pos: [f32; 2],
        heading: [f32; 2],
        dest: NodeId,
        final_point: Option<[f32; 2]>,
        rejoin: bool,
    ) -> (Track, bool) {
        let Some(start) = self.start_node(pos, heading) else {
            return (Track::default(), false);
        };
        let on_road = self.graph.nearest_on_road(pos);
        let mut turn_around = false;
        let mut pts = vec![pos];
        if let Some(route) = self.graph.shortest_path(start, dest) {
            // Don't drive to the node ahead only to come straight back along the same road:
            // turn around now instead.
            let behind = on_road.map(|rp| self.graph.other_end(rp.segment, start));
            let skip = route.len() >= 2 && Some(route[1]) == behind;
            turn_around = skip;
            // Start on the centreline, not at the body: the lane offset is applied on top, so
            // a unit already in its lane would otherwise be pushed a second lane width right
            // (into parked cars).
            let mut first = skip as usize;
            match on_road.filter(|_| !skip) {
                Some(rp) if rp.dist_m < ROAD_HALF_WIDTH_M => pts[0] = rp.point,
                Some(rp) if !rejoin || rp.dist_m < self.road_edge_m(rp.segment) + REJOIN_KERB_M => {
                }
                // Off the road (out of a U-turn that swung wide onto the far kerb): join it at
                // a node well ahead, not at one just beside the car (wp_sim police_passby,
                // Vesper: a leg 9 m sideways to the junction 3 m ahead looped the car across
                // the road and back, 4 s lost).
                Some(rp) => {
                    while first + 1 < route.len() && {
                        let to = sub(self.graph.node(route[first]).pos, pos);
                        let d = crate::graph::len(to);
                        d < REJOIN_NODE_PER_M * rp.dist_m
                            && d > 0.1
                            && crate::graph::dot(scale(to, 1.0 / d), heading) < REJOIN_SIDE_COS
                    } {
                        first += 1;
                    }
                }
                None => {}
            }
            pts.extend(route.iter().skip(first).map(|n| self.graph.node(*n).pos));
        }
        if let Some(p) = final_point {
            pts.push(p);
        }
        (Track::new(pts), turn_around)
    }

    /// Distance from segment `seg`'s centreline to its outer road edge, m
    /// ([`ROAD_HALF_WIDTH_M`] when the lanes are unknown).
    fn road_edge_m(&self, seg: SegmentId) -> f32 {
        let g = self.graph.segment(seg);
        if g.lane_width_m <= 0.0 || g.lanes_fwd == 0 {
            return ROAD_HALF_WIDTH_M;
        }
        let lanes = if g.oneway {
            0.5 * g.lanes_fwd as f32
        } else {
            g.lanes_fwd.max(g.lanes_back) as f32
        };
        lanes * g.lane_width_m
    }

    pub(crate) fn random_node_in(&mut self, d: Option<crate::graph::District>) -> Option<NodeId> {
        // By road length (2026-10-23), like patrol spawn spots: an end of a segment drawn by
        // length. Patrol routes spread over the district's roads, not its curve vertices.
        let u = self.rng.next_f32();
        let pick_b = self.rng.below(2) == 1;
        let (sid, _) = self
            .graph
            .spot_by_length(u, |s| d.is_none_or(|d| s.district == d))?;
        let seg = self.graph.segment(sid);
        // A one-way segment's far end is always reachable from it.
        Some(if pick_b || seg.oneway { seg.b } else { seg.a })
    }

    fn limit_at(&self, p: [f32; 2]) -> f32 {
        self.graph
            .nearest_on_road(p)
            .map(|r| self.graph.segment(r.segment).speed_limit_mps)
            .unwrap_or(13.9)
    }

    /// Whether a racer is still on course for an ambush wait point: moving, and the point lies
    /// on its predicted path ahead.
    pub(crate) fn ambush_holds(&self, racer: EntityId, wait: [f32; 2]) -> bool {
        let Some(rec) = self.racers.get(&racer) else {
            return false;
        };
        let Some(p) = rec.last_known else {
            return false;
        };
        // Lost from sight: no more guessing where it goes; search as usual (breaking line of
        // sight still escapes: escape_break_contact_night).
        if rec.status != PursuitStatus::Pursuit {
            return false;
        }
        let v = [rec.vel[0], rec.vel[1]];
        if (v[0] * v[0] + v[1] * v[1]).sqrt() < STOPPED_RACER_MPS {
            return false;
        }
        let path = self.graph.predict_path(p, v, dist(p, wait) * 1.5 + 100.0);
        let (_, lat) = Track::new(path.points()).project(wait, 0.0, f32::INFINITY);
        lat < AMBUSH_PATH_TOLERANCE_M
    }

    /// Predicted intercept: the node on the racer's predicted path, 10–20 s out, that the unit
    /// can reach in time (or the far end of the horizon).
    /// A unit that sees the racer itself plans on what it sees (`last_known`); the others on
    /// the radioed position, never the racer's real velocity (2026-10-23).
    fn intercept_point(&self, i: usize, racer: EntityId) -> Option<(NodeId, [f32; 2])> {
        let rec = self.racers.get(&racer)?;
        let (p, v) = if self.unit_aware(self.units[i].id, racer) {
            (rec.last_known?, rec.last_known_vel)
        } else {
            rec.radioed()?
        };
        self.meet_point(i, p, v)
    }

    /// Where a unit answering a reckless-driving call meets the racer reported at `p` moving
    /// at `v`: the first junction on the racer's predicted path (up to
    /// [`RESPOND_HORIZON_S`] out) the unit reaches before the racer, else the racer's reported
    /// position itself. Shorter-sighted than [`PoliceSystem::intercept_point`]: responders
    /// start close, and a racer roaming a grid turns long before 10–20 s are up.
    pub(crate) fn respond_meet_point(
        &self,
        i: usize,
        p: [f32; 2],
        v: [f32; 2],
    ) -> Option<(NodeId, [f32; 2])> {
        let speed = (v[0] * v[0] + v[1] * v[1]).sqrt();
        let to_report = || self.graph.nearest_node(p).map(|n| (n, p));
        if speed < STOPPED_RACER_MPS {
            return to_report();
        }
        let path = self.graph.predict_path(p, v, speed * RESPOND_HORIZON_S);
        let u = &self.units[i];
        let Some(start) = self.start_node(u.body.pos, u.body.heading) else {
            return to_report();
        };
        let factor = self.cfg.pursuit.respond_speed_factor;
        let lead = dist(u.body.pos, self.graph.node(start).pos) / 15.0;
        let mut s = 0.0;
        for leg in &path.legs {
            s += leg.len_m;
            let node = self.graph.other_end(leg.segment, leg.from_node);
            let Some(route) = self.graph.shortest_path(start, node) else {
                continue;
            };
            let eta = self.graph.route_time_s(&route) / factor + lead;
            if eta <= s / speed {
                return Some((node, self.graph.node(node).pos));
            }
        }
        to_report()
    }

    /// Where unit `i` searches for a racer it lost sight of (row 64): the junction nearest the
    /// last known position, the next junctions on the path it was taking, and the roads out of
    /// that first junction. Units on the racer take different spots (by id), and every
    /// [`SEARCH_SWEEP_S`] each moves on to the next one.
    pub(crate) fn search_point(&self, i: usize, racer: EntityId) -> Option<(NodeId, [f32; 2])> {
        let rec = self.racers.get(&racer)?;
        let p = rec.last_known?;
        let me = self.units[i].id;
        let ordinal = self
            .units
            .iter()
            .filter(|u| u.assignment.target() == Some(racer) && u.kind == UnitKind::Car)
            .position(|u| u.id == me)
            .unwrap_or(0);
        // Lost inside a tunnel / covered road: hold its portals (PVPD doctrine §5: units go to
        // both portals, nobody chases inside). Each unit takes the portal on its side (a unit
        // sent to the far one drove through the tunnel to get there and found him inside:
        // Founders Cut after the residential courts changed the road graph, lead/city-
        // residential); equally near: alternate by unit.
        if let Some(k) = self.cover_at(p) {
            let portals = self.covers[k].outside_portals(PORTAL_HOLD_M);
            let here = self.units[i].body.pos;
            let (d0, d1) = (dist(here, portals[0]), dist(here, portals[1]));
            let q = if (d0 - d1).abs() < 1.0 {
                portals[ordinal % 2]
            } else if d0 < d1 {
                portals[0]
            } else {
                portals[1]
            };
            let n = self.graph.nearest_node(q)?;
            return Some((n, self.graph.node(n).pos));
        }
        let first = self.graph.nearest_node(p)?;
        let mut nodes = vec![first];
        let v = rec.last_known_vel;
        let speed = crate::graph::len(v);
        let sweep = (rec.los_lost_s / SEARCH_SWEEP_S) as usize;
        // Row 84, a growing sweep: the area it can have reached grows with the time unseen
        // (stepped every SEARCH_SWEEP_S, so units are not re-routed every tick).
        let reach = (SEARCH_REACH_MIN_M
            + speed.max(SEARCH_REACH_MIN_MPS)
                * SEARCH_REACH_FRAC
                * (sweep as f32 * SEARCH_SWEEP_S))
            .min(SEARCH_REACH_MAX_M);
        // Its likely exits first: the junctions along its predicted road...
        if speed > STOPPED_RACER_MPS {
            let path = self
                .graph
                .predict_path(p, v, reach.max(speed * 12.0).max(120.0));
            for leg in path.legs.iter().take(8) {
                nodes.push(self.graph.other_end(leg.segment, leg.from_node));
            }
        }
        for &(_, n) in self.graph.outgoing(first) {
            nodes.push(n);
        }
        // ...then every junction within reach, nearest first.
        let mut around: Vec<(f32, u32)> = (0..self.graph.nodes.len() as u32)
            .filter(|&n| self.graph.nodes[n as usize].junction)
            .map(|n| (dist(self.graph.nodes[n as usize].pos, p), n))
            .filter(|(d, _)| *d <= reach)
            .collect();
        around.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        nodes.extend(around.into_iter().map(|(_, n)| NodeId(n)));
        let mut seen = std::collections::BTreeSet::new();
        nodes.retain(|n| seen.insert(*n));
        nodes.truncate(SEARCH_MAX_POINTS);
        let n = nodes[(ordinal + sweep) % nodes.len()];
        Some((n, self.graph.node(n).pos))
    }

    /// Where unit `i` can meet a racer last placed at `p` moving at `v`: see
    /// [`PoliceSystem::intercept_point`]. Responders to a reckless-driving call use the call's
    /// position.
    fn meet_point(&self, i: usize, p: [f32; 2], v: [f32; 2]) -> Option<(NodeId, [f32; 2])> {
        let real_speed = (v[0] * v[0] + v[1] * v[1]).sqrt();
        if real_speed < STOPPED_RACER_MPS {
            // A stopped racer stays put: drive to it (via the end of its road nearer the unit).
            let rp = self.graph.nearest_on_road(p)?;
            let seg = *self.graph.segment(rp.segment);
            let me = self.units[i].body.pos;
            let near =
                if dist(self.graph.node(seg.a).pos, me) <= dist(self.graph.node(seg.b).pos, me) {
                    seg.a
                } else {
                    seg.b
                };
            return Some((near, p));
        }
        let speed = real_speed.max(5.0);
        let h = &self.cfg.pursuit.tactics.intercept_horizon_s;
        let path = self.graph.predict_path(p, v, speed * h.max + 50.0);
        let u = &self.units[i];
        let start = self.start_node(u.body.pos, u.body.heading)?;
        let factor = self.cfg.pursuit.respond_speed_factor;
        let mut fallback = None;
        // Interceptors on the same racer aim at different points of its path (by id), so
        // they don't queue for one junction: the n-th tries the later horizons first.
        let me = u.id;
        let nth = self.units.iter().filter(|o| {
            o.id < me
                && matches!(o.assignment, Assignment::Intercept { target, .. } if Some(target) == u.assignment.target())
        }).count();
        for j in 0..3 {
            let k = (j + nth) % 3;
            let t = h.min + (h.max - h.min) * k as f32 * 0.5;
            let s = speed * t;
            let Some((leg, _)) = path.leg_at(s) else {
                break;
            };
            let node = self
                .graph
                .other_end(path.legs[leg].segment, path.legs[leg].from_node);
            let route = self.graph.shortest_path(start, node)?;
            let eta = self.graph.route_time_s(&route) / factor
                + dist(u.body.pos, self.graph.node(start).pos) / 15.0;
            fallback = Some((node, self.graph.node(node).pos));
            if eta <= t {
                return fallback;
            }
        }
        fallback
    }

    pub(crate) fn plan_units(&mut self, world: &dyn WorldQuery, tdt: f32) {
        let now = self.now;
        let p = self.cfg.pursuit.clone();
        for i in 0..self.units.len() {
            if self.units[i].player.is_some() {
                continue;
            }
            if self.units[i].wrecked {
                self.units[i].cmd = DriveCommand::stop();
                continue;
            }
            let pursuing = matches!(
                self.units[i].assignment,
                Assignment::Pursue { .. } | Assignment::Intercept { .. }
            );
            if !pursuing {
                let u = &mut self.units[i];
                u.risk = (u.risk + p.risk.regen_per_s * tdt).min(p.risk.budget);
            }
            let assignment = self.units[i].assignment.clone();
            match assignment {
                Assignment::Idle => match self.units[i].role {
                    UnitRole::Pool(d) => {
                        if let Some(dest) = self.random_node_in(Some(d)) {
                            self.assign(i, Assignment::Patrol { dest });
                        }
                    }
                    // Done: cruise on until out of every racer's sight, then `cleanup_units`
                    // despawns it (parked in view it would have to vanish in view).
                    UnitRole::Responder | UnitRole::Reinforcement => {
                        let d = Some(self.graph.district_at(self.units[i].body.pos));
                        if let Some(dest) = self.random_node_in(d) {
                            self.assign(i, Assignment::Patrol { dest });
                        }
                    }
                    _ => {}
                },
                Assignment::Patrol { dest } => {
                    let u = &self.units[i];
                    let near_end =
                        !u.cmd.track.is_drivable() || u.track_s > u.cmd.track.length() - 8.0;
                    if near_end {
                        if u.cmd.track.is_drivable() {
                            // Arrived: pick the next destination (radio only on assignment change).
                            let d = match u.role {
                                UnitRole::Pool(d) => Some(d),
                                _ => None,
                            };
                            if let Some(next) = self.random_node_in(d) {
                                self.units[i].assignment = Assignment::Patrol { dest: next };
                                let t = self.route_track(i, next, None);
                                self.units[i].set_command(DriveCommand {
                                    track: t,
                                    speed_mps: 0.0,
                                    cornering_g: p.patrol_cornering_g,
                                    siren: false,
                                    lane_offset_m: LANE_OFFSET_M,
                                });
                            }
                        } else {
                            let t = self.route_track(i, dest, None);
                            self.units[i].set_command(DriveCommand {
                                track: t,
                                speed_mps: 0.0,
                                cornering_g: p.patrol_cornering_g,
                                siren: false,
                                lane_offset_m: LANE_OFFSET_M,
                            });
                        }
                    }
                    let lim = self.limit_at(self.units[i].body.pos);
                    self.units[i].cmd.speed_mps = lim * p.patrol_speed_factor;
                }
                Assignment::Respond { incident } => {
                    let Some(k) = self.incidents.iter().position(|x| x.id == incident) else {
                        self.assign(i, Assignment::Idle);
                        continue;
                    };
                    let inc = self.incidents[k];
                    // On scene with the reported racer in sight: follow it (it may be
                    // recognised, which starts a pursuit and closes the call) instead of
                    // standing where the call came from.
                    let follow = match inc.kind {
                        crate::dispatch::IncidentKind::Report { racer } => {
                            self.racers.get(&racer).and_then(|r| {
                                let u = &self.units[i];
                                let rp = xy(r.pos);
                                (dist(u.body.pos, rp) < RESPOND_FOLLOW_M
                                    && self.sees_at(world, u.body.pos, u.body.alt_m, rp, r.pos[2]))
                                .then_some((rp, [r.vel[0], r.vel[1]]))
                            })
                        }
                        crate::dispatch::IncidentKind::Crash => None,
                    };
                    if let Some((rp, rv)) = follow {
                        // Following a reported car to see who it is, not chasing it: siren
                        // quiet (it comes with a pursuit), same driving (`Unit::quiet`).
                        self.units[i].quiet = true;
                        let u = &self.units[i];
                        // The racer coming straight at it: pull over to the right and stop,
                        // let it pass, then follow (no head-on meeting on a narrow road).
                        let to = sub(rp, u.body.pos);
                        let d = dist(rp, u.body.pos);
                        let ahead = dot(to, u.body.heading) > 0.0;
                        let v_r = (rv[0] * rv[0] + rv[1] * rv[1]).sqrt();
                        // Nose to nose with the racer stopped in front (it waits for us on a
                        // narrow road): back out of its way.
                        if ahead && d < 20.0 && v_r < 1.0 && !u.internal {
                            let room = self.room_behind(i, crate::stuck::YIELD_REVERSE_M);
                            let u = &mut self.units[i];
                            u.unstick.yield_s += tdt;
                            if u.unstick.yield_s >= crate::stuck::YIELD_DEADLOCK_S
                                && room
                                && u.unstick.may_turn(now)
                            {
                                u.unstick.yield_s = 0.0;
                                let pos = u.body.pos;
                                u.unstick.note_turn(pos, now);
                                let steer = -u.unstick.last_steer.signum();
                                u.unstick.phase = crate::stuck::Phase::Reverse {
                                    from: u.body.pos,
                                    dist_m: crate::stuck::YIELD_REVERSE_M,
                                    until: now.plus(2.0 * crate::stuck::REVERSE_MAX_S),
                                    steer,
                                };
                            }
                            self.units[i].cmd.speed_mps = 0.0;
                            continue;
                        }
                        let head_on = dot(rv, u.body.heading) < -3.0;
                        self.units[i].unstick.yield_s = 0.0;
                        if d < RESPOND_YIELD_M && ahead && head_on {
                            // Stop only lined up with the lane: stopped half way round a bend
                            // it stood across the road (opponent-pace, Hollins hairpin).
                            // Until then creep on along it.
                            let u = &self.units[i];
                            let t = &u.cmd.track;
                            let a = crate::unit::lane_point(t, u.track_s, 0.0);
                            let b = crate::unit::lane_point(t, u.track_s + 2.0, 0.0);
                            let along = crate::graph::normalize(sub(b, a));
                            let aligned = !t.is_drivable()
                                || dot(along, u.body.heading) >= RESPOND_YIELD_ALIGNED_COS;
                            self.units[i].cmd.speed_mps = if aligned {
                                0.0
                            } else {
                                RESPOND_YIELD_CREEP_MPS
                            };
                            self.units[i].cmd.lane_offset_m = LANE_OFFSET_M + RESPOND_YIELD_SHIFT_M;
                            continue;
                        }
                        let u = &self.units[i];
                        let t = &u.cmd.track;
                        let stale = !t.is_drivable()
                            || u.track_s > t.length() - 10.0
                            || dist(t.point_at(t.length()), rp) > 15.0;
                        if stale {
                            // Through the junction the racer is heading for, ending on it.
                            let node = self
                                .start_node(rp, rv)
                                .or_else(|| self.graph.nearest_node(rp))
                                .unwrap_or(NodeId(0));
                            let t = self.route_track(i, node, Some(rp));
                            self.units[i].set_command(DriveCommand {
                                track: t,
                                speed_mps: 0.0,
                                cornering_g: p.pursuit_cornering_g,
                                siren: true,
                                lane_offset_m: LANE_OFFSET_M,
                            });
                        }
                        let lim = self.limit_at(self.units[i].body.pos);
                        let v_r = (rv[0] * rv[0] + rv[1] * rv[1]).sqrt();
                        self.units[i].cmd.speed_mps =
                            (lim * p.respond_speed_factor).max(v_r * (1.0 + p.target_speed_margin));
                        continue;
                    }
                    let arrived = dist(self.units[i].body.pos, inc.pos) < 25.0;
                    if arrived {
                        match inc.kind {
                            crate::dispatch::IncidentKind::Report { .. } => {
                                self.incidents.remove(k);
                                self.assign(i, Assignment::Idle);
                            }
                            crate::dispatch::IncidentKind::Crash => {
                                if inc.scene_until.is_none() {
                                    let dur = self.cfg.presence.crash_scene_s.get(self.phase);
                                    self.incidents[k].scene_until = Some(now.plus(dur));
                                }
                                self.units[i].cmd = DriveCommand::stop();
                            }
                        }
                        continue;
                    }
                    let u = &self.units[i];
                    let t = &u.cmd.track;
                    let reroute = match inc.kind {
                        // Reckless driver: head for where the racer will be (later calls keep
                        // `inc.pos` on its latest reported position), refreshed like an
                        // intercept. Driving to where it was only ends in a stern chase.
                        crate::dispatch::IncidentKind::Report { .. } => {
                            let held_for = now.since(u.assigned);
                            let stale = held_for > 0.0 && held_for % 3.0 < tdt;
                            let goal = if !t.is_drivable() || u.track_s > t.length() - 10.0 || stale
                            {
                                // The velocity it was reported at, not the racer's real one.
                                let v = self.call_vel.get(&inc.id).copied().unwrap_or([0.0, 0.0]);
                                self.respond_meet_point(i, inc.pos, v)
                            } else {
                                None
                            };
                            goal.map(|(node, point)| {
                                let near = dist(point, inc.pos) < RESPOND_REROUTE_M;
                                (node, near.then_some(inc.pos))
                            })
                        }
                        crate::dispatch::IncidentKind::Crash => (!t.is_drivable()).then(|| {
                            let n = self.graph.nearest_node(inc.pos).unwrap_or(NodeId(0));
                            (n, Some(inc.pos))
                        }),
                    };
                    if let Some((dest, fin)) = reroute {
                        let t = self.route_track(i, dest, fin);
                        self.units[i].set_command(DriveCommand {
                            track: t,
                            speed_mps: 0.0,
                            cornering_g: p.patrol_cornering_g,
                            siren: true,
                            lane_offset_m: LANE_OFFSET_M,
                        });
                    }
                    // On the way to the call: siren sounding again.
                    self.units[i].quiet = false;
                    let lim = self.limit_at(self.units[i].body.pos);
                    self.units[i].cmd.speed_mps = lim * p.respond_speed_factor;
                    // Meeting the racer at a junction means waiting short of it, not driving
                    // into its path as it arrives (a T-bone at 35 m/s).
                    if let crate::dispatch::IncidentKind::Report { racer } = inc.kind {
                        if let Some(r) = self.racers.get(&racer) {
                            let u = &self.units[i];
                            let t = &u.cmd.track;
                            let end = t.point_at(t.length());
                            let rp = xy(r.pos);
                            let rv = [r.vel[0], r.vel[1]];
                            let coming =
                                dot(rv, sub(end, rp)) > 0.0 && dist(rp, end) < RESPOND_HOLD_RACER_M;
                            if t.is_drivable()
                                && t.length() - u.track_s < RESPOND_HOLD_SHORT_M
                                && coming
                            {
                                self.units[i].cmd.speed_mps = 0.0;
                            }
                        }
                    }
                }
                Assignment::Intercept {
                    target,
                    point: wait,
                    ambush,
                } => {
                    let Some(rec) = self.racers.get(&target) else {
                        self.assign(i, Assignment::Idle);
                        continue;
                    };
                    if !rec.pursued() {
                        self.assign(i, Assignment::Idle);
                        continue;
                    }
                    let rp = xy(rec.pos);
                    let seen = self.obs.get(&target).is_some_and(|o| o.seen);
                    let u = &self.units[i];
                    // Join the chase only once on the racer's line and behind it (hysteresis with
                    // the off-trail check in coordinate_pursuits). An ambusher joins once it is
                    // on the racer's road facing its way (not while still turning in from its
                    // side road) and the racer has come level with it.
                    if seen
                        && dist(u.body.pos, rp) < 120.0
                        && self.sees_at(world, u.body.pos, u.body.alt_m, rp, rec.pos[2])
                    {
                        // Wanted level 3+: also ahead of the racer on its line, going its
                        // way: it joins as a rolling block.
                        let ahead_ok = rec.level.number()
                            >= self.cfg.presence.unlocks.rolling_block_level
                            && dot(u.body.heading, normalize([rec.vel[0], rec.vel[1]])) > 0.7;
                        let joins = self.pursuit_track(target).is_some_and(|(tr, racer_s)| {
                            let (s_u, lat) = tr.project(u.body.pos, racer_s, 200.0);
                            let ok = if ambush {
                                // Turned onto the racer's road: >= 5 m past the junction.
                                let along =
                                    normalize(sub(tr.point_at(s_u + 5.0), tr.point_at(s_u)));
                                let in_place = u.track_s
                                    > u.cmd.track.length()
                                        - (crate::dispatch::AMBUSH_INTO_LEG_M - 5.0);
                                // Once the racer is level or past: joining ahead of it at
                                // the wait point meant a racer at speed closing on a slow
                                // car (headless self-wrecks 8.7 % -> 11.7 %).
                                in_place
                                    && dot(u.body.heading, along) > AMBUSH_JOIN_MIN_COS
                                    && s_u <= racer_s + 5.0
                            } else {
                                s_u <= racer_s + 5.0 || (ahead_ok && s_u <= racer_s + 60.0)
                            };
                            lat < 12.0 && ok
                        });
                        if joins {
                            self.assign(i, Assignment::Pursue { target });
                            continue;
                        }
                    }
                    if ambush {
                        // Keep driving to (and then sit at) the wait point while the racer is
                        // still headed for it; otherwise intercept as usual.
                        if self.ambush_holds(target, wait) {
                            let lim = self.limit_at(self.units[i].body.pos);
                            let cap = p.pursuit_cap_mps.get(self.phase);
                            self.units[i].cmd.speed_mps =
                                (lim * p.respond_speed_factor * 1.15).min(cap);
                        } else {
                            // Still intercepting (no new radio line); replan next think.
                            let u = &mut self.units[i];
                            u.assignment = Assignment::Intercept {
                                target,
                                point: rp,
                                ambush: false,
                            };
                            u.cmd.track = Track::default();
                        }
                        continue;
                    }
                    // Lost: search around the last known position (row 64), spread over the
                    // junctions it could have taken.
                    if rec.status == PursuitStatus::Searching {
                        if let Some((node, point)) = self.search_point(i, target) {
                            let cur = match self.units[i].assignment {
                                Assignment::Intercept { point, .. } => point,
                                _ => [f32::MAX, f32::MAX],
                            };
                            let u = &self.units[i];
                            let done = u.track_s > u.cmd.track.length() - 10.0;
                            if dist(cur, point) > 1.0 || !u.cmd.track.is_drivable() || done {
                                let t = self.route_track(i, node, None);
                                self.units[i].assignment = Assignment::Intercept {
                                    target,
                                    point,
                                    ambush: false,
                                };
                                self.units[i].set_command(DriveCommand {
                                    track: t,
                                    speed_mps: 0.0,
                                    cornering_g: p.chase.cornering_g,
                                    siren: true,
                                    lane_offset_m: LANE_OFFSET_M,
                                });
                            }
                        }
                        let lim = self.limit_at(self.units[i].body.pos);
                        self.units[i].cmd.speed_mps = lim * p.respond_speed_factor;
                        continue;
                    }
                    let li = rec.level.table_index();
                    let lim = self.limit_at(self.units[i].body.pos);
                    let cap = p.pursuit_cap_mps.get(self.phase) * p.chase.cap_scale[li];
                    let intercept_speed = (lim * INTERCEPT_LIMIT_FACTOR).min(cap);
                    // Wanted level 3+: an interceptor that got to a junction on the racer's
                    // path first parks across it (track ends there) until the racer arrives.
                    let u = &self.units[i];
                    if rec.level.number() >= p.chase.junction_block_level
                        && u.cmd.track.is_drivable()
                    {
                        let end = u.cmd.track.point_at(u.cmd.track.length());
                        // What this unit knows: its own sight, else the radio.
                        let (kp, rv) = if self.unit_aware(u.id, target) {
                            (rec.last_known.unwrap_or(rp), rec.last_known_vel)
                        } else {
                            rec.radioed().unwrap_or((rp, [0.0, 0.0]))
                        };
                        let d_racer = dist(kp, end);
                        let coming =
                            dot(rv, sub(end, kp)) > 0.0 && d_racer < 250.0 && d_racer > 12.0;
                        if coming && dist(u.body.pos, end) < 35.0 {
                            if !matches!(u.tactic, Tactic::RollingBlock) {
                                // Reuse RollingBlock as the "blocking" marker for views.
                                self.units[i].tactic = Tactic::RollingBlock;
                                self.stats.junction_blocks += 1;
                            }
                            self.units[i].cmd.speed_mps = intercept_speed;
                            continue;
                        }
                    }
                    self.units[i].tactic = Tactic::Shadow;
                    let u = &self.units[i];
                    let held_for = now.since(u.assigned);
                    let stale = held_for > 0.0 && held_for % 3.0 < tdt;
                    if !u.cmd.track.is_drivable()
                        || u.track_s > u.cmd.track.length() - 10.0
                        || stale
                    {
                        if let Some((node, point)) = self.intercept_point(i, target) {
                            // A stopped racer: drive all the way to it, not just to the junction.
                            let kv = rec.last_known_vel;
                            let v_r = (kv[0] * kv[0] + kv[1] * kv[1]).sqrt();
                            let fin = (v_r < STOPPED_RACER_MPS).then_some(point);
                            let u = &self.units[i];
                            let end = fin.unwrap_or(self.graph.node(node).pos);
                            let done = !u.cmd.track.is_drivable()
                                || u.track_s > u.cmd.track.length() - 10.0;
                            let same = !done
                                && dist(u.cmd.track.point_at(u.cmd.track.length()), end) < 1.0;
                            // Don't flip to a route that starts with a turn-around while still
                            // driving the current one at speed (a moving racer moves the
                            // intercept point back and forth).
                            let flip = !done
                                && u.body.speed > INTERCEPT_KEEP_MPS
                                && self.route_from(u.body.pos, u.body.heading, node, fin).1;
                            if !same && !flip {
                                let t = self.route_track(i, node, fin);
                                self.units[i].assignment = Assignment::Intercept {
                                    target,
                                    point,
                                    ambush: false,
                                };
                                self.units[i].set_command(DriveCommand {
                                    track: t,
                                    speed_mps: 0.0,
                                    cornering_g: p.chase.cornering_g,
                                    siren: true,
                                    lane_offset_m: LANE_OFFSET_M,
                                });
                            }
                        }
                    }
                    self.units[i].cmd.speed_mps = intercept_speed;
                }
                Assignment::Pursue { .. } => {} // coordinate_pursuits
                Assignment::HeloTrack { target } => {
                    let aim = self
                        .racers
                        .get(&target)
                        .and_then(|r| r.last_known)
                        .unwrap_or(self.units[i].body.pos);
                    self.units[i].cmd = DriveCommand {
                        track: Track::new(vec![aim]),
                        speed_mps: self.cfg.tools.helicopter.speed_mps,
                        cornering_g: 1.0,
                        siren: false,
                        lane_offset_m: 0.0,
                    };
                }
                Assignment::RoadblockAt { .. }
                | Assignment::SpikeAt { .. }
                | Assignment::OutOfService => {
                    if self.units[i].kind == UnitKind::Car {
                        self.units[i].cmd = DriveCommand::stop();
                    }
                }
            }
        }
        self.coordinate_pursuits(world);
        // Corner planning and local speed limit (think rate).
        let brake = p.unit.brake_mps2;
        let limits: Vec<f32> = self
            .units
            .iter()
            .map(|u| self.limit_at(u.body.pos))
            .collect();
        for (u, lim) in self.units.iter_mut().zip(limits) {
            u.limit_mps = lim;
            if u.kind == UnitKind::Car {
                // Pursuing and intercepting units brake late (row 63).
                let chasing = matches!(
                    u.assignment,
                    Assignment::Pursue { .. } | Assignment::Intercept { .. }
                );
                let b = if chasing { p.chase.brake_mps2 } else { brake };
                u.planned_speed = plan_unit_speed(u, b);
            }
        }
    }

    /// Pursuit track for a racer: breadcrumbs + predicted path from the last known position.
    fn pursuit_track(&self, racer: EntityId) -> Option<(Track, f32)> {
        let rec = self.racers.get(&racer)?;
        let last = rec.last_known?;
        let trail = self
            .trails
            .get(&racer)
            .cloned()
            .unwrap_or_else(|| Track::new(vec![last]));
        let racer_s = trail.length();
        let v = if rec.status == PursuitStatus::Searching {
            rec.last_known_vel
        } else {
            [rec.vel[0], rec.vel[1]]
        };
        let ahead = self.graph.predict_path(last, v, 250.0);
        let mut pts = trail.pts.clone();
        pts.extend(ahead.points().into_iter().skip(1));
        Some((Track::new(pts), racer_s))
    }

    fn coordinate_pursuits(&mut self, world: &dyn WorldQuery) {
        let now = self.now;
        let p = self.cfg.pursuit.clone();
        let t = &p.tactics;
        let ch = &p.chase;
        let unlocks = self.cfg.presence.unlocks.clone();
        let racers: Vec<EntityId> = self
            .racers
            .values()
            .filter(|r| r.pursued())
            .map(|r| r.entity)
            .collect();
        for racer in racers {
            let Some((track, racer_s)) = self.pursuit_track(racer) else {
                continue;
            };
            let rec = self.racers[&racer].clone();
            let seen = rec.status == PursuitStatus::Pursuit;
            let level = rec.level.number();
            let li = rec.level.table_index();
            let v_r = if seen {
                (rec.vel[0] * rec.vel[0] + rec.vel[1] * rec.vel[1]).sqrt()
            } else {
                0.0
            };
            let cap_abs = p.pursuit_cap_mps.get(self.phase) * ch.cap_scale[li];
            let cap = pursuit_speed_cap(v_r, ch.speed_margin, cap_abs);
            let rp = xy(rec.pos);
            // (gap along track, lateral, unit index)
            let mut pursuers: Vec<(f32, f32, usize)> = Vec::new();
            for i in 0..self.units.len() {
                let u = &self.units[i];
                if u.player.is_some()
                    || u.wrecked
                    || u.assignment != (Assignment::Pursue { target: racer })
                {
                    continue;
                }
                let hint = if u.cmd.track.pts.first() == track.pts.first() {
                    u.track_s
                } else {
                    racer_s
                };
                // Right next to the racer: it is the racer's end of the trail, not an older leg
                // of it that passes close by (a racer looping a block).
                let near = rec
                    .last_known
                    .is_some_and(|p| dist(p, u.body.pos) < NEAR_RACER_M);
                let (s, lat) = if near {
                    track.project(u.body.pos, racer_s, NEAR_RACER_M)
                } else {
                    // The closer of the projection from its own progress and the one the
                    // intercept branch joins on (from the racer's end of the trail): they
                    // disagreed for a unit 50 m behind a stopped racer, which then flipped
                    // between Pursue and Intercept every think step and never moved.
                    let own = track.project(u.body.pos, hint, 200.0);
                    let back = track.project(u.body.pos, racer_s, 200.0);
                    if back.1 < own.1 {
                        back
                    } else {
                        own
                    }
                };
                pursuers.push((racer_s - s, lat, i));
            }
            pursuers.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.2.cmp(&b.2)));
            // Units off the trail or far behind go intercept; units that reached the last
            // known position without finding the racer search around it (row 64).
            let mut active = Vec::new();
            for &(gap, lat, i) in &pursuers {
                let held = now < self.units[i].captain_hold_until;
                // Lost in a tunnel: straight to its portals, not in after him.
                let lost_here = !seen
                    && rec.los_lost_s > LOST_SEARCH_AFTER_S
                    && rec.last_known.is_some_and(|k| {
                        dist(k, self.units[i].body.pos) < 30.0 || self.cover_at(k).is_some()
                    });
                if !held && (lat > 30.0 || gap > t.intercept_min_gap_m + 100.0 || lost_here) {
                    let point = rec.last_known.unwrap_or(self.units[i].body.pos);
                    self.assign(
                        i,
                        Assignment::Intercept {
                            target: racer,
                            point,
                            ambush: false,
                        },
                    );
                } else {
                    active.push((gap, i));
                }
            }
            // Straight road ahead of the racer: its line now and `pit_min_straight_m` on point
            // the same way (the breadcrumb trail is too noisy for a curvature test).
            let straight_ahead = {
                let dir = |s0: f32| normalize(sub(track.point_at(s0 + 5.0), track.point_at(s0)));
                let (d0, d1) = (dir(racer_s), dir(racer_s + ch.pit_min_straight_m));
                let mid = dir(racer_s + 0.5 * ch.pit_min_straight_m);
                dot(d0, d1) > PIT_STRAIGHT_COS && dot(d0, mid) > PIT_STRAIGHT_COS
            };
            // Civilians close to the racer hold back deliberate contact (less so at higher
            // levels). Parked props (no entity, not moving) are not people.
            let clear_m = ch.civilian_clear_m[li];
            let civ_near = clear_m > 0.0
                && self.obstacles.iter().any(|o| {
                    o.unit.is_none()
                        && o.entity != Some(racer)
                        && !o.entity.is_some_and(|e| self.racers.contains_key(&e))
                        && (o.entity.is_some() || crate::graph::len(o.vel) > 0.1)
                        && dist(o.pos, rp) < clear_m
                });
            let contact_ok = seen && !civ_near && v_r > 3.0;
            let budget_floor = -p.risk.budget * (ch.risk_budget_scale[li] - 1.0);
            let with_budget: Vec<(f32, usize)> = active
                .iter()
                .copied()
                .filter(|(gap, i)| self.units[*i].risk > budget_floor && gap.abs() < 60.0)
                .collect();
            let boxin = seen
                && !civ_near
                && level >= unlocks.boxin_level
                && v_r < t.boxin_max_kmh / 3.6
                && v_r > 1.0
                && with_budget.len() >= 2;
            let pit_ok = level >= unlocks.pit_level
                && ch.pit_max_kmh[li] > 0.0
                && v_r < ch.pit_max_kmh[li] / 3.6
                && straight_ahead;
            let mut shadow_rank = 0.0f32;
            let mut behind_rank = 0usize;
            for &(gap, i) in &active {
                let u = &self.units[i];
                let has_budget = u.risk > budget_floor;
                let ready = now >= u.pit_ready;
                // Left (+) or right (-) of the racer's line, fixed per unit.
                let side = if u.id.0.is_multiple_of(2) { 1.0 } else { -1.0 };
                // Close and in sight: chase the racer's own line. Otherwise drive the roads
                // (row 66: following the trail round a corner cuts it through the building).
                // A unit routing round a jam it stood in (row 86) drives the roads, not the
                // trail through the jam.
                // Row 140: across a divided road's median it is not "close": it routes round
                // by the roads instead of shadowing the racer from the other carriageway.
                let close = dist(u.body.pos, rp) < CLOSE_LOS_M
                    && u.unstick.blocked_at(now).is_empty()
                    && !self.opposed_carriageways(u.body.pos, rp)
                    && self.sees_at(world, u.body.pos, u.body.alt_m, rp, rec.pos[2]);
                let tactic = if boxin && with_budget.iter().take(3).any(|x| x.1 == i) {
                    let k = with_budget.iter().position(|x| x.1 == i).unwrap_or(0);
                    Tactic::BoxIn(
                        [BoxSlot::Rear, BoxSlot::FrontLeft, BoxSlot::FrontRight][k.min(2)],
                    )
                } else if gap < -5.0 && level >= unlocks.rolling_block_level && has_budget && seen {
                    Tactic::RollingBlock
                } else {
                    let rank = behind_rank;
                    behind_rank += 1;
                    let attack = contact_ok && has_budget && close && gap > -3.0 && gap < 35.0;
                    if attack && rank == 0 && ready && pit_ok {
                        Tactic::Pit
                    } else if attack && rank == 0 && ready && level >= ch.hard_ram_level {
                        Tactic::Ram(RamKind::Hard)
                    } else if attack && rank == 0 && ready && level >= ch.rear_ram_level {
                        Tactic::Ram(RamKind::RearQuarter)
                    } else if attack && rank == 1 && level >= ch.flank_level {
                        Tactic::Ram(RamKind::Side)
                    } else {
                        Tactic::Shadow
                    }
                };
                let cornering = if has_budget {
                    ch.cornering_g
                } else {
                    p.shadow_cornering_g
                };
                let lim_here = self.limit_at(u.body.pos);
                let mut routed = false;
                let (cmd_track, speed) = match tactic {
                    Tactic::Shadow => {
                        let mid =
                            (ch.shadow_min_m[li] + ch.shadow_max_m[li]) * 0.5 + 8.0 * shadow_rank;
                        shadow_rank += 1.0;
                        // Close in for the arrest when the racer stops (by straight-line
                        // distance: a unit on the cross street can project right next to it
                        // on the trail).
                        let arrest = v_r < 3.0 && seen;
                        let want_gap = if arrest { 6.0 } else { mid };
                        let gap = if arrest {
                            gap.max(dist(u.body.pos, rp))
                        } else {
                            gap
                        };
                        // Far behind: close the gap at up to the pursuit cap, braking in time
                        // to arrive at the shadow gap.
                        let excess = gap - want_gap;
                        let mut sp = if excess > 0.0 {
                            v_r + (2.0 * SHADOW_CLOSE_DECEL_MPS2 * excess).sqrt()
                        } else {
                            v_r + 0.6 * excess
                        };
                        if !seen {
                            // Lost contact: to the last known position at a measured pace.
                            sp = lim_here * p.respond_speed_factor;
                        }
                        let far = gap > ch.shadow_max_m[li] + 15.0;
                        let base_cap = if far { cap_abs } else { cap };
                        let own_cap = if has_budget {
                            base_cap
                        } else {
                            base_cap.min(lim_here * 1.3)
                        };
                        let tr = if close {
                            track.clone()
                        } else {
                            routed = true;
                            self.chase_route(i, racer, &track, racer_s)
                        };
                        (tr, sp.clamp(0.0, own_cap))
                    }
                    Tactic::Pit => {
                        // Line up beside the racer's rear quarter, then turn into it.
                        let lined_up = gap <= ch.pit_turn_in_gap_m + 0.8;
                        let (lat, sp) = if lined_up {
                            (side * 0.3, v_r + 2.0)
                        } else {
                            let closing = (0.9 * (gap - ch.pit_turn_in_gap_m)).clamp(0.0, 10.0);
                            (side * ch.pit_side_offset_m, v_r + closing)
                        };
                        (offset_track(&track, lat), sp.min(cap_abs + 6.0))
                    }
                    Tactic::Ram(kind) => {
                        // Close fast from farther back, at the ram's closing speed at the end.
                        let approach = (0.5 * gap).max(0.0);
                        let (lat, sp) = match kind {
                            RamKind::RearQuarter => {
                                (side * 0.9, v_r + ch.ram_closing_mps[0].max(approach))
                            }
                            RamKind::Hard => (0.0, v_r + ch.ram_closing_mps[2].max(approach)),
                            RamKind::Side => {
                                // Flank: alongside the racer; from the hard-ram level slam
                                // into it once level with it.
                                let slam = level >= ch.hard_ram_level && ready && gap.abs() < 1.5;
                                if slam {
                                    (side * 0.8, v_r + ch.ram_closing_mps[1])
                                } else {
                                    (side * ch.flank_offset_m, v_r + (0.8 * gap).clamp(-3.0, 6.0))
                                }
                            }
                        };
                        let tr = if lat == 0.0 {
                            track.clone()
                        } else {
                            offset_track(&track, lat)
                        };
                        (tr, sp.clamp(0.0, cap_abs + 7.0))
                    }
                    Tactic::RollingBlock => {
                        let ahead = -gap;
                        let sp = v_r * t.rolling_block_speed_factor
                            - 0.4 * (ahead - t.rolling_block_lead_m);
                        (track.clone(), sp.clamp(0.0, cap))
                    }
                    Tactic::BoxIn(slot) => {
                        let (lat, lon) = match slot {
                            BoxSlot::Rear => (0.0, -t.boxin_offset_m[1]),
                            BoxSlot::FrontLeft => (t.boxin_offset_m[0], t.boxin_offset_m[1]),
                            BoxSlot::FrontRight => (-t.boxin_offset_m[0], t.boxin_offset_m[1]),
                        };
                        let tr = if lat == 0.0 {
                            track.clone()
                        } else {
                            offset_track(&track, lat)
                        };
                        let sp = v_r + 0.8 * (gap + lon);
                        (tr, sp.clamp(0.0, v_r * 1.3 + 5.0))
                    }
                };
                let same = self.units[i].cmd.track.pts.first() == cmd_track.pts.first()
                    && self.units[i].tactic == tactic;
                let u = &mut self.units[i];
                if !routed {
                    u.chase_route = None;
                }
                if u.tactic != tactic {
                    u.tactic = tactic;
                }
                if same {
                    u.cmd.track = cmd_track;
                } else {
                    u.set_command(DriveCommand {
                        track: cmd_track,
                        speed_mps: 0.0,
                        cornering_g: cornering,
                        siren: true,
                        lane_offset_m: 0.0,
                    });
                }
                u.cmd.speed_mps = speed;
                u.cmd.cornering_g = cornering;
                u.cmd.siren = true;
            }
        }
    }

    /// A road route from pursuing unit `i` to the racer (row 66): the legal route to the
    /// junction the racer is heading for, through the racer's position, then on along the
    /// pursuit `track` (its predicted path). Kept for [`CHASE_ROUTE_REFRESH_S`] unless the
    /// racer heads for another junction, so the unit's progress along it is not reset.
    fn chase_route(&mut self, i: usize, racer: EntityId, track: &Track, racer_s: f32) -> Track {
        let now = self.now;
        let rec = &self.racers[&racer];
        let rp = rec.last_known.unwrap_or(xy(rec.pos));
        let v = if rec.status == PursuitStatus::Pursuit {
            [rec.vel[0], rec.vel[1]]
        } else {
            rec.last_known_vel
        };
        let stopped = crate::graph::len(v) < STOPPED_RACER_MPS;
        // A stopped racer: to the end of its road nearer the unit, then right up to it.
        let node = if stopped {
            self.meet_point(i, rp, [0.0, 0.0]).map(|(n, _)| n)
        } else {
            self.start_node(rp, v)
        }
        .or_else(|| self.graph.nearest_node(rp));
        let Some(node) = node else {
            return track.clone();
        };
        if let Some(c) = self.units[i].chase_route {
            if c.node == node && now.since(c.at) < CHASE_ROUTE_REFRESH_S {
                return self.units[i].cmd.track.clone();
            }
        }
        if stopped {
            let t = self.chase_route_track(i, node, Some(rp));
            self.units[i].chase_route = Some(crate::unit::ChaseRoute { node, at: now });
            return t;
        }
        let mut t = self.chase_route_track(i, node, None);
        // Then the racer's predicted path beyond that junction (no stop at the route's end).
        let np = self.graph.node(node).pos;
        let (s_node, _) = track.project(np, racer_s, 400.0);
        let mut pts = t.pts.clone();
        let mut s = s_node + 10.0;
        while s < track.length() {
            pts.push(track.point_at(s));
            s += 10.0;
        }
        if pts.len() > t.pts.len() {
            t = Track::new(pts);
        }
        self.units[i].chase_route = Some(crate::unit::ChaseRoute { node, at: now });
        t
    }

    // -----------------------------------------------------------------------------------------
    // Per-tick driving
    // -----------------------------------------------------------------------------------------

    pub(crate) fn drive_units(&mut self, world: &dyn WorldQuery, dt: f32) {
        if self.units.is_empty() {
            return;
        }
        let cfg = self.cfg.pursuit.clone();
        let helo = self.cfg.tools.helicopter.clone();
        let phase = self.phase;
        let mode = self.setup.mode;
        let hazards = self.setup.hazards;
        for i in 0..self.units.len() {
            let u = &self.units[i];
            if !u.internal || u.player.is_some() {
                continue;
            }
            match u.kind {
                UnitKind::Helicopter => {
                    let target = u.cmd.track.pts.first().copied().unwrap_or(u.body.pos);
                    let speed = if u.assignment == Assignment::OutOfService {
                        helo.speed_mps
                    } else {
                        u.cmd.speed_mps
                    };
                    let target = if u.assignment == Assignment::OutOfService {
                        crate::graph::add(u.body.pos, scale(u.body.heading, 100.0))
                    } else {
                        target
                    };
                    step_helo(&mut self.units[i], target, speed, helo.altitude_m, dt);
                }
                UnitKind::Car => {
                    if u.wrecked && u.body.speed == 0.0 {
                        continue;
                    }
                    let env = DriveEnv {
                        limit_mps: u.limit_mps,
                        phase,
                        mode,
                        hazards,
                    };
                    let pursuing = matches!(
                        u.assignment,
                        Assignment::Pursue { .. } | Assignment::Intercept { .. }
                    );
                    self.update_avoid(i, dt);
                    let rep = step_ground(&mut self.units[i], &env, &cfg, &mut self.rng, dt);
                    // Standing time (reset in place, row 78), as physical units count it in
                    // `unit_input`.
                    let u = &mut self.units[i];
                    match u.unstick.stand_at {
                        Some(p) if dist(p, u.body.pos) < crate::stuck::STAND_M => {
                            u.unstick.stand_s += dt
                        }
                        _ => {
                            u.unstick.stand_at = Some(u.body.pos);
                            u.unstick.stand_s = 0.0;
                        }
                    }
                    self.stats.near_misses += rep.near_miss as u32;
                    self.stats.traffic_collisions += rep.traffic_collision as u32;
                    self.stats.run_wide_crashes += rep.crashed as u32;
                    if pursuing {
                        self.stats.pursuit_harm += rep.near_miss as u32
                            + rep.traffic_collision as u32
                            + rep.crashed as u32;
                    }
                    let u = &mut self.units[i];
                    if pursuing {
                        if u.body.lat_g > cfg.risk.lateral_g {
                            u.risk -= cfg.risk.cost_per_s_lateral * dt;
                            self.stats.pursuit_high_g_s += dt;
                        }
                        if rep.near_miss {
                            u.risk -= cfg.risk.cost_near_miss;
                        }
                        if rep.traffic_collision || rep.crashed {
                            u.risk -= cfg.risk.cost_collision;
                        }
                        if u.risk <= 0.0 {
                            self.stats.pursuit_no_risk_s += dt;
                        }
                    }
                }
            }
        }
        self.internal_racer_contacts(world);
    }

    /// Proximity contacts between internally driven units and racers (physical units get
    /// `SimEvent::Contact` from Rapier instead).
    fn internal_racer_contacts(&mut self, world: &dyn WorldQuery) {
        let racers: Vec<(EntityId, [f32; 2], [f32; 2])> = self
            .racers
            .values()
            .map(|r| (r.entity, xy(r.pos), [r.vel[0], r.vel[1]]))
            .collect();
        for i in 0..self.units.len() {
            let u = &self.units[i];
            if !u.internal || u.kind != UnitKind::Car || u.wrecked {
                continue;
            }
            for &(e, p, v) in &racers {
                if dist(self.units[i].body.pos, p) < 2.4 {
                    let uv = self.units[i].body.vel();
                    let rel = sub(uv, v);
                    let rel_speed = (rel[0] * rel[0] + rel[1] * rel[1]).sqrt();
                    let before = self.stats.racer_contacts;
                    let hit = crate::dispatch::RacerHit {
                        rel_speed,
                        impulse_ns: 0.0,
                        physical: false,
                    };
                    self.unit_racer_contact(world, i, e, hit, true);
                    if self.stats.racer_contacts != before {
                        let u = &mut self.units[i];
                        u.body.speed = u.body.speed.min((v[0] * v[0] + v[1] * v[1]).sqrt()) * 0.8;
                    }
                }
            }
        }
    }
}

/// Lateral acceleration needed to take a corner of radius `r_m` at `v_mps`, g.
pub fn lateral_g(v_mps: f32, r_m: f32) -> f32 {
    v_mps * v_mps / (r_m * GRAVITY_MPS2)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Port Vesper, police with no ground units (nothing can capture a parked racer) and a
    /// helicopter that launches at once and flies fast, so the test is about sight only.
    fn vesper_helo_police() -> (
        PoliceSystem,
        crate::scenario::ScenarioWorld,
        wp_world::vesper::VesperReport,
        std::sync::Arc<wp_world::CityMap>,
    ) {
        use wp_world::vesper::{generate_with_report, VesperConfig};
        let (net, rep) = generate_with_report(&VesperConfig::default());
        let map = std::sync::Arc::new(wp_world::CityMap::new(net));
        let mut cfg = crate::PoliceConfig::builtin();
        cfg.presence.max_ground_units = 0;
        // No patrols either (they come from the pool table, not the cap): with round 12c's
        // 380 m day sight a ground unit saw the racer under the deck.
        let none = crate::config::PhaseTable {
            dawn: 0,
            day: 0,
            dusk: 0,
            night: 0,
        };
        cfg.presence.patrols = crate::config::DistrictTable {
            core: none,
            suburb: none,
            industrial: none,
        };
        // No roadblock or spikes either (their tool units also see 380 m by day).
        cfg.presence.unlocks.spikes_level = 99;
        cfg.presence.unlocks.roadblock_level = 99;
        cfg.presence.helicopter.launch_s.day = 0.5;
        cfg.tools.helicopter.speed_mps = 400.0;
        let police = PoliceSystem::with_city(
            cfg,
            crate::graph::RoadGraph::from_city(&map),
            crate::PoliceSetup {
                seed: 4,
                ..Default::default()
            },
        );
        let world = crate::scenario::ScenarioWorld {
            map: Some(map.clone()),
            ..crate::scenario::ScenarioWorld::clock(wp_contracts::TimeOfDay::from_hours(12.0), 0.0)
        };
        (police, world, rep, map)
    }

    #[test]
    fn helicopter_joins_at_level_5_keeps_visual_and_loses_it_under_an_overpass() {
        // Both grade separations: the overpass and the underpass (in a cutting).
        for crossing in 0..2 {
            helicopter_over_crossing(crossing);
        }
    }

    fn helicopter_over_crossing(crossing: usize) {
        let (mut p, world, rep, map) = vesper_helo_police();
        let racer = EntityId(3);
        // A point under the deck and one 80 m along the road below, out in the open.
        let c = rep.crossings[crossing];
        let s = &map.net.surfaces[c.lower];
        let k = (0..s.centre.len())
            .min_by(|&a, &b| dist(xy(s.centre[a]), c.pos).total_cmp(&dist(xy(s.centre[b]), c.pos)))
            .unwrap();
        let under = s.centre[k];
        let open = (0..s.centre.len())
            .map(|j| s.centre[j])
            .find(|q| (dist(xy(*q), c.pos) - 80.0).abs() < 6.0)
            .expect("lower road runs 80 m past the crossing");
        let view = |q: [f32; 3]| crate::RacerView {
            entity: racer,
            pos: [q[0], q[1], q[2] + 0.5],
            vel: [0.0, 12.0, 0.0],
        };
        let mut out = Vec::new();
        let mut run = |p: &mut PoliceSystem, at: [f32; 3], secs: f32| {
            for _ in 0..(secs / wp_contracts::PHYSICS_DT) as usize {
                p.step(&world, &[view(at)], &[], wp_contracts::PHYSICS_DT, &mut out);
            }
        };
        run(&mut p, open, 0.5);
        // Wanted level 4 with a pursuit on: no helicopter (it joins at level 5, owner row 156).
        p.set_heat(racer, 75.0);
        assert_eq!(p.wanted_level(racer), WantedLevel::L4);
        p.start_pursuit(racer, UnitId(999));
        run(&mut p, open, 3.0);
        assert!(p.helicopters().is_empty());
        // Level 5: the AI captain calls it in; it flies over and has visual.
        p.set_heat(racer, 95.0);
        assert_eq!(p.wanted_level(racer), WantedLevel::L5);
        run(&mut p, open, 6.0);
        let h = p.helicopters();
        assert_eq!(h.len(), 1, "{:?}", p.stats());
        assert!(h[0].has_visual);
        assert!(
            dist(xy(h[0].pos), xy(open)) < 30.0,
            "overhead: {:?}",
            h[0].pos
        );
        let said = |p: &PoliceSystem, what: &str| p.radio_log().any(|l| l.text.contains(what));
        let log = |p: &PoliceSystem| p.radio_log().map(|l| l.text.clone()).collect::<Vec<_>>();
        // The radio speaks one line at a time, so the helicopter's lines may wait for the
        // channel (the level-5 call is still on the air); critical lines are never dropped.
        // `go(p, at, secs, what)`: `secs` at `at`, then on until `what` is said (<= 20 s).
        let mut go = |p: &mut PoliceSystem, at: [f32; 3], secs: f32, what: &str| {
            run(p, at, secs);
            let mut t = 0.0;
            while !said(p, what) && t < 20.0 {
                run(p, at, 0.25);
                t += 0.25;
            }
            assert!(said(p, what), "{what:?} never said: {:?}", log(p));
        };
        // Row 144: helicopter lines are tier 1 and go out at once, so give it the time it
        // used to spend waiting for the channel to settle overhead (from its approach angle it
        // can still see under the deck's edge).
        go(&mut p, open, 4.0, "eyes on racer #3");
        // It keeps the racer's last known position fresh for the ground units.
        let last = p.racer(racer).unwrap().last_known.unwrap();
        assert!(dist(last, xy(open)) < 1.0);
        // Under the deck: visual lost after the grace time, the radio says so, the last
        // known position stays out in the open, the spotlight stays there.
        go(&mut p, under, 5.0, "lost visual on racer #3");
        let h = p.helicopters();
        assert!(!h[0].has_visual);
        assert!(dist(p.racer(racer).unwrap().last_known.unwrap(), xy(open)) < 1.0);
        assert!(p.racer(racer).unwrap().los_lost_s > 3.0);
        // Out again: reacquired.
        go(&mut p, open, 2.0, "reacquired");
        assert!(p.helicopters()[0].has_visual);
    }

    #[test]
    fn cap_is_target_plus_margin_bounded() {
        assert_eq!(pursuit_speed_cap(30.0, 0.1, 40.0), 33.0);
        assert_eq!(pursuit_speed_cap(50.0, 0.1, 40.0), 40.0);
        assert_eq!(pursuit_speed_cap(0.0, 0.1, 40.0), 10.0);
    }

    #[test]
    fn offset_track_shifts_left() {
        let t = Track::new(vec![[0.0, 0.0], [0.0, 100.0]]);
        let o = offset_track(&t, 3.0);
        assert!((o.pts[0][0] + 3.0).abs() < 1e-5);
        assert!((o.pts[1][0] + 3.0).abs() < 1e-5);
    }

    #[test]
    fn lateral_g_formula() {
        assert!((lateral_g(9.81, 9.81) - 1.0).abs() < 1e-5);
    }
}

/// 2026-10-23 perception and sensitivity rules ("cops see through walls", "not overly
/// sensitive"), on a hand-built straight road with one stationary observer unit and no AI.
#[cfg(test)]
mod perception_tests {
    use super::*;
    use crate::graph::{District, Occluder, Poi, RoadClass, RoadGraph};
    use crate::scenario::ScenarioWorld;
    use crate::{PoliceConfig, PoliceSetup, RacerView};
    use wp_contracts::{PlayerId, SimEvent, TimeOfDay, ViolationKind, PHYSICS_DT};

    const RACER: EntityId = EntityId(42);

    /// A 2 km straight Core road along y = 0, no patrols, no spawns of any kind.
    fn police(occluders: &[Occluder], camera: Option<[f32; 2]>) -> PoliceSystem {
        let mut g = RoadGraph::new();
        let ids: Vec<NodeId> = (0..=20)
            .map(|k| g.add_node([-1000.0 + 100.0 * k as f32, 0.0], District::Core))
            .collect();
        for k in 0..20 {
            g.add_segment(ids[k], ids[k + 1], 13.9, RoadClass::Arterial, false);
        }
        g.occluders.extend_from_slice(occluders);
        if let Some(c) = camera {
            g.pois.push(Poi {
                kind: PoiKind::Camera,
                pos: c,
                radius_m: 60.0,
            });
        }
        let mut cfg = PoliceConfig::builtin();
        let none = crate::config::PhaseTable {
            dawn: 0,
            day: 0,
            dusk: 0,
            night: 0,
        };
        cfg.presence.patrols = crate::config::DistrictTable {
            core: none,
            suburb: none,
            industrial: none,
        };
        cfg.presence.max_ground_units = 0;
        PoliceSystem::with_city(
            cfg,
            g,
            PoliceSetup {
                seed: 1,
                ..Default::default()
            },
        )
    }

    fn world() -> ScenarioWorld {
        ScenarioWorld::clock(TimeOfDay::from_hours(12.0), 0.0)
    }

    /// Step `secs` with the racer standing at `at` (or moving at `vel`).
    fn run(p: &mut PoliceSystem, at: [f32; 2], vel: [f32; 2], secs: f32, ev: &[SimEvent]) {
        let w = world();
        let view = RacerView {
            entity: RACER,
            pos: [at[0], at[1], 0.0],
            vel: [vel[0], vel[1], 0.0],
        };
        let mut out = Vec::new();
        for k in 0..(secs / PHYSICS_DT).round() as usize {
            let e: &[SimEvent] = if k == 0 { ev } else { &[] };
            p.step(&w, &[view], e, PHYSICS_DT, &mut out);
        }
    }

    fn observer(p: &mut PoliceSystem) -> UnitId {
        p.add_player_unit(PlayerId(1), EntityId(900), [0.0, 0.0, 0.0], [1.0, 0.0])
    }

    fn status(p: &PoliceSystem) -> PursuitStatus {
        p.racer(RACER).map(|r| r.status).unwrap_or_default()
    }

    fn violation(kind: ViolationKind, at: [f32; 2]) -> SimEvent {
        SimEvent::TrafficViolation {
            vehicle: RACER,
            kind,
            pos: [at[0], at[1], 0.0],
        }
    }

    /// A wall across the road between the observer (x = 0) and x = 150.
    const WALL: Occluder = Occluder {
        min: [60.0, -6.0],
        max: [80.0, 6.0],
    };
    const HIDDEN: [f32; 2] = [150.0, 0.0];
    const OPEN: [f32; 2] = [150.0, 40.0];

    #[test]
    fn a_wanted_car_is_identified_only_after_a_moment_in_sight() {
        let mut p = police(&[], None);
        observer(&mut p);
        run(&mut p, HIDDEN, [0.0, 0.0], 0.2, &[]);
        p.set_heat(RACER, 15.0);
        // 150 m by day: ~1.2 s of sight. Not after 0.5 s, by 2 s.
        run(&mut p, HIDDEN, [0.0, 0.0], 0.5, &[]);
        assert_eq!(status(&p), PursuitStatus::Wanted);
        run(&mut p, HIDDEN, [0.0, 0.0], 1.5, &[]);
        assert_eq!(status(&p), PursuitStatus::Pursuit);
    }

    #[test]
    fn a_glimpse_does_not_re_acquire_a_lost_racer_but_identification_does() {
        let mut p = police(&[WALL], None);
        let u = observer(&mut p);
        run(&mut p, OPEN, [0.0, 0.0], 0.2, &[]);
        p.set_heat(RACER, 15.0);
        p.start_pursuit(RACER, u);
        // Behind the wall for 6 s: lost (Searching), awareness drained.
        run(&mut p, HIDDEN, [0.0, 0.0], 6.0, &[]);
        assert_eq!(status(&p), PursuitStatus::Searching);
        let lost = p.racer(RACER).unwrap().los_lost_s;
        // A 0.1 s glimpse: still lost, the escape timer keeps running.
        run(&mut p, OPEN, [0.0, 0.0], 0.1, &[]);
        run(&mut p, HIDDEN, [0.0, 0.0], 0.5, &[]);
        assert_eq!(status(&p), PursuitStatus::Searching);
        assert!(p.racer(RACER).unwrap().los_lost_s > lost);
        // In sight long enough to identify it: found again.
        run(&mut p, OPEN, [0.0, 0.0], 2.0, &[]);
        assert_eq!(status(&p), PursuitStatus::Pursuit);
    }

    #[test]
    fn a_racer_beyond_the_identify_range_is_not_re_acquired() {
        let mut p = police(&[WALL], None);
        let u = observer(&mut p);
        run(&mut p, OPEN, [0.0, 0.0], 0.2, &[]);
        p.set_heat(RACER, 15.0);
        p.start_pursuit(RACER, u);
        run(&mut p, HIDDEN, [0.0, 0.0], 6.0, &[]);
        // 320 m out in the open by day (identify range 250 m, notice range 380 m): noticed
        // (fully aware after ~3 s), never identified; 11 s lost in all, short of the 12 s escape.
        run(&mut p, [320.0, 0.0], [0.0, 0.0], 5.0, &[]);
        assert_eq!(status(&p), PursuitStatus::Searching);
    }

    #[test]
    fn a_far_witness_does_not_start_a_pursuit_at_once() {
        let mut p = police(&[], None);
        observer(&mut p);
        // 200 m: within the witness range (250 m), beyond witness_pursuit_m (150 m).
        let at = [200.0, 0.0];
        run(&mut p, at, [0.0, 0.0], 0.15, &[]);
        p.set_heat(RACER, 9.0);
        run(
            &mut p,
            at,
            [12.0, 0.0],
            0.1,
            &[violation(ViolationKind::RedLight, at)],
        );
        assert_eq!(
            p.wanted_level(RACER),
            WantedLevel::L1,
            "witnessed: full heat"
        );
        assert_eq!(status(&p), PursuitStatus::Wanted);
        // It identifies the car (~1.6 s at 200 m) and then pursues.
        run(&mut p, at, [0.0, 0.0], 2.0, &[]);
        assert_eq!(status(&p), PursuitStatus::Pursuit);
    }

    #[test]
    fn a_close_witness_starts_a_pursuit_at_once() {
        let mut p = police(&[], None);
        observer(&mut p);
        let at = [100.0, 0.0];
        run(&mut p, at, [0.0, 0.0], 0.15, &[]);
        p.set_heat(RACER, 9.0);
        run(
            &mut p,
            at,
            [12.0, 0.0],
            0.05,
            &[violation(ViolationKind::RedLight, at)],
        );
        assert_eq!(status(&p), PursuitStatus::Pursuit);
    }

    #[test]
    fn a_standing_car_hit_by_a_civilian_is_not_the_offender() {
        let mut p = police(&[], None);
        run(
            &mut p,
            OPEN,
            [0.0, 0.0],
            0.2,
            &[violation(ViolationKind::Collision, OPEN)],
        );
        assert_eq!(p.racer(RACER).unwrap().heat, 0.0);
        assert!(p.incidents().is_empty());
    }

    #[test]
    fn an_unseen_offence_is_called_in_only_by_someone_nearby_and_the_call_expires() {
        let mut p = police(&[], None);
        // The world reports its traffic, none of it near the racer: nobody calls.
        p.set_obstacles(&[crate::ObstacleView::car(
            Some(EntityId(500)),
            [-600.0, 0.0, 0.0],
            [0.0; 3],
            [1.0, 0.0],
        )]);
        let at = [400.0, 0.0];
        run(
            &mut p,
            at,
            [12.0, 0.0],
            0.2,
            &[violation(ViolationKind::RedLight, at)],
        );
        assert!(
            p.racer(RACER).unwrap().heat > 0.0,
            "reported heat still counts"
        );
        assert!(p.incidents().is_empty());
        // A civilian 30 m away: a call.
        p.set_obstacles(&[crate::ObstacleView::car(
            Some(EntityId(500)),
            [430.0, 0.0, 0.0],
            [0.0; 3],
            [1.0, 0.0],
        )]);
        // (The same violation kind counts once per `repeat_cooldown_s`.)
        run(&mut p, at, [12.0, 0.0], 1.2, &[]);
        run(
            &mut p,
            at,
            [12.0, 0.0],
            1.5,
            &[violation(ViolationKind::RedLight, at)],
        );
        assert_eq!(p.incidents().len(), 1);
        // No further report: dropped `call_ttl_s` (60 s) later.
        run(&mut p, at, [12.0, 0.0], 55.0, &[]);
        assert_eq!(p.incidents().len(), 1);
        run(&mut p, at, [12.0, 0.0], 8.0, &[]);
        assert!(p.incidents().is_empty());
    }

    #[test]
    fn mild_speeding_is_not_called_in() {
        let mut p = police(&[], None);
        let at = [400.0, 0.0];
        let ev = violation(ViolationKind::Speeding { over_mps: 3.0 }, at);
        run(&mut p, at, [17.0, 0.0], 0.2, &[ev]);
        assert!(p.incidents().is_empty());
    }

    #[test]
    fn a_traffic_camera_never_keeps_a_pursuit_going() {
        let mut p = police(&[WALL], Some(HIDDEN));
        let u = observer(&mut p);
        run(&mut p, OPEN, [0.0, 0.0], 0.2, &[]);
        p.set_heat(RACER, 15.0);
        p.start_pursuit(RACER, u);
        // Under the camera, behind the wall from the unit: lost all the same.
        run(&mut p, HIDDEN, [0.0, 0.0], 1.0, &[]);
        assert_eq!(status(&p), PursuitStatus::Searching);
    }

    /// Owner 2026-10-23: police leave AI racers far from every player alone.
    #[test]
    fn ai_racers_far_from_every_player_are_dropped() {
        let mut p = police(&[], None);
        let u = observer(&mut p);
        let player = EntityId(1);
        let near_npc = EntityId(2);
        let w = world();
        let views = |npc_x: f32| {
            [
                RacerView {
                    entity: player,
                    pos: [-500.0, 0.0, 0.0],
                    vel: [0.0; 3],
                },
                RacerView {
                    entity: near_npc,
                    pos: [-300.0, 0.0, 0.0],
                    vel: [0.0; 3],
                },
                RacerView {
                    entity: RACER,
                    pos: [npc_x, 0.0, 0.0],
                    vel: [0.0; 3],
                },
            ]
        };
        let mut out = Vec::new();
        p.step(&w, &views(100.0), &[], PHYSICS_DT, &mut out);
        p.set_heat(RACER, 35.0);
        p.start_pursuit(RACER, u);
        assert!(p.racer(RACER).is_some_and(|r| r.pursued()));
        // No player set: every racer counts (headless runs).
        p.step(&w, &views(100.0), &[], PHYSICS_DT, &mut out);
        assert!(p.racer(RACER).is_some());
        // The sim names the player: the AI racer 600 m from it is dropped, pursuit called off,
        // its violations ignored; the one 200 m from it stays.
        p.set_player_racers(Some(&[player]));
        p.step(&w, &views(100.0), &[], PHYSICS_DT, &mut out);
        assert!(p.racer(RACER).is_none());
        assert!(p.racer(near_npc).is_some());
        assert!(p
            .units()
            .iter()
            .all(|x| x.assignment.target() != Some(RACER)));
        let ev = [violation(ViolationKind::RedLight, [100.0, 0.0])];
        p.step(&w, &views(100.0), &ev, PHYSICS_DT, &mut out);
        assert!(p.racer(RACER).is_none());
    }

    #[test]
    fn patrols_scale_with_road_length() {
        let p = police(&[], None);
        let mut cfg = PoliceConfig::builtin();
        // 2 km of Core road against the 12.8 km reference: 8 x 2 / 12.8 = 1.25 -> 1.
        let n = (cfg.presence.patrols.core.day as f32 * 2.0 / cfg.presence.patrol_reference_km.core)
            .round() as u32;
        let g = (*p.graph).clone();
        let q = PoliceSystem::with_city(cfg.clone(), g.clone(), PoliceSetup::default());
        assert_eq!(q.patrol_target(District::Core), n.max(1));
        assert_eq!(q.patrol_target(District::Suburb), 0, "no road, no patrol");
        // The city-wide cap scales every district down.
        cfg.presence.patrols.core.day = 200;
        cfg.presence.max_patrols.day = 10;
        let q = PoliceSystem::with_city(cfg, g, PoliceSetup::default());
        assert_eq!(q.patrol_target(District::Core), 10);
    }
}
