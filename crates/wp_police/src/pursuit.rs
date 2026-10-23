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

/// Interceptors drive at up to this × the local limit (capped by the unit car's top speed).
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

/// A racer slower than this is stopped for the chase tactics: shadowing units close in for the
/// arrest and no rolling block is started ahead of it, m/s.
const ARREST_RACER_MPS: f32 = 3.0;

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

/// Pursuit speed: `min(max(target × (1 + margin), floor), cap)`, where `cap` is the unit car's
/// top speed (there is no separate pursuit speed cap). The floor (10 m/s) lets units close on a
/// stopped racer for the arrest.
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

    pub(crate) fn update_observations(&mut self, world: &dyn WorldQuery) {
        let los_range = self.cfg.pursuit.los_range_m.get(self.phase);
        let witness = self.cfg.heat.heat.witness_range_m.get(self.phase);
        let recognise = self.cfg.heat.heat.recognition_range_m.get(self.phase);
        let helo_range = self.cfg.tools.helicopter.los_range_m.get(self.phase);
        let sight = self.cfg.heat.heat.sight.clone();
        let ids: Vec<EntityId> = self.racers.keys().copied().collect();
        for s in self.helo_sight.values_mut() {
            s.los = false;
        }
        let mut helo_los: Vec<UnitId> = Vec::new();
        for id in ids {
            let p = xy(self.racers[&id].pos);
            let pz = self.racers[&id].pos[2];
            let mut o = RacerObs {
                nearest_unit_m: f32::INFINITY,
                nearest_chaser_m: f32::INFINITY,
                ..Default::default()
            };
            let mut best_spot = f32::INFINITY;
            let mut best_witness = f32::INFINITY;
            for u in &self.units {
                if u.wrecked {
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
                            continue;
                        }
                        if d <= los_range
                            && in_view(u.body.heading, u.body.pos, p, d, &sight)
                            && self.sees_at(world, u.body.pos, u.body.alt_m, p, pz)
                        {
                            // Recognition (pursuit start) and witnessing (full violation heat)
                            // have their own ranges inside line-of-sight range.
                            o.seen = true;
                            if d <= witness && d < best_witness {
                                o.witnessed = true;
                                best_witness = d;
                                o.witness = Some(u.id);
                            }
                            if d <= recognise && d < best_spot {
                                best_spot = d;
                                o.spotter = Some(u.id);
                                o.spotter_m = d;
                            }
                        }
                    }
                    UnitKind::Helicopter => {
                        if matches!(u.assignment, Assignment::HeloTrack { target } if target == id)
                            && d <= helo_range
                            // Looking down from altitude: only tall buildings, high ground,
                            // covered roads (tunnels) and bridge decks (underpasses) block.
                            // The target point follows the car up on terrain like the ground
                            // units' eyes do (a fixed 1.3 m point sits underground on Port
                            // Vesper v2's hills).
                            && world.line_of_sight(
                                [u.body.pos[0], u.body.pos[1], u.body.alt_m],
                                [p[0], p[1], crate::eye_height_at(pz)],
                            )
                        {
                            o.seen = true;
                            helo_los.push(u.id);
                        }
                    }
                }
            }
            if self.camera_sees(world, p) {
                o.seen = true;
                o.witnessed = true;
            }
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

    pub(crate) fn update_racer_rules(&mut self, tdt: f32) {
        let ids: Vec<EntityId> = self.racers.keys().copied().collect();
        let cfg = self.cfg.heat.clone();
        for id in ids {
            if self.held.contains(&id) {
                // Driver dropped out: everything stays as it was (see `set_racer_held`).
                self.recognition_s.remove(&id);
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
            // A unit that keeps a wanted racer in recognition range long enough starts a pursuit
            // (longer further out; out of sight the timer drains instead of resetting, row 80).
            let rc = &self.cfg.heat.heat.recognition;
            let acc = self.recognition_s.entry(id).or_insert(0.0);
            *acc = if o.spotter.is_some() {
                *acc + tdt
            } else {
                (*acc - rc.decay_per_s * tdt).max(0.0)
            };
            let range = self.cfg.heat.heat.recognition_range_m.get(self.phase);
            let far = ((o.spotter_m - rc.near_m) / (range - rc.near_m).max(1.0)).clamp(0.0, 1.0);
            let needed =
                self.cfg.heat.heat.recognition_time_s.get(self.phase) + rc.far_extra_s * far;
            let recognised = o.spotter.is_some() && *acc >= needed;
            let rec = &self.racers[&id];
            if !rec.pursued() && rec.level > WantedLevel::None && recognised {
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
            format!("Dispatch: 10-80, all units, pursuit of {desc}."),
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
                "Dispatch: all units, we've lost racer #{}, last seen {}. Resume patrol.",
                racer.0,
                self.last_seen_phrase(racer)
            ),
            PursuitOutcome::Safehouse => format!(
                "Dispatch: racer #{} went to ground. Resume patrol.",
                racer.0
            ),
            PursuitOutcome::Captured => match by {
                Some(cs) => format!(
                    "Dispatch: {cs} has racer #{} in custody {here}. Code 4, all units.",
                    racer.0
                ),
                None => format!(
                    "Dispatch: racer #{} in custody {here}. Code 4, all units.",
                    racer.0
                ),
            },
            PursuitOutcome::CapturedTotalLoss => format!(
                "Dispatch: racer #{} vehicle disabled {here}, suspect in custody. Code 4.",
                racer.0
            ),
            PursuitOutcome::CalledOff => format!(
                "Dispatch: pursuit of racer #{} terminated. All units resume patrol.",
                racer.0
            ),
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
        let cands: Vec<u32> = (0..self.graph.nodes.len() as u32)
            .filter(|&n| d.is_none_or(|d| self.graph.nodes[n as usize].district == d))
            .collect();
        if cands.is_empty() {
            return None;
        }
        Some(NodeId(cands[self.rng.below(cands.len() as u32) as usize]))
    }

    /// Top speed of unit `i`'s car (measured limits, else the `[pro]` defaults), m/s. Pursuing
    /// and intercepting units are capped only by this.
    pub(crate) fn unit_top_speed_mps(&self, i: usize) -> f32 {
        self.units[i]
            .limits
            .unwrap_or(self.cfg.pursuit.pro.default_limits)
            .top_speed_mps
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
    fn intercept_point(&self, i: usize, racer: EntityId) -> Option<(NodeId, [f32; 2])> {
        let rec = self.racers.get(&racer)?;
        let p = rec.last_known?;
        self.meet_point(i, p, [rec.vel[0], rec.vel[1]])
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
                        crate::dispatch::IncidentKind::Report { racer } => {
                            let held_for = now.since(u.assigned);
                            let stale = held_for > 0.0 && held_for % 3.0 < tdt;
                            let goal = if !t.is_drivable() || u.track_s > t.length() - 10.0 || stale
                            {
                                let v = self
                                    .racers
                                    .get(&racer)
                                    .map_or([0.0, 0.0], |r| [r.vel[0], r.vel[1]]);
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
                            let top = self.unit_top_speed_mps(i);
                            self.units[i].cmd.speed_mps =
                                (lim * p.respond_speed_factor * 1.15).min(top);
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
                    let lim = self.limit_at(self.units[i].body.pos);
                    let top = self.unit_top_speed_mps(i);
                    let intercept_speed = (lim * INTERCEPT_LIMIT_FACTOR).min(top);
                    // Wanted level 3+: an interceptor that got to a junction on the racer's
                    // path first parks across it (track ends there) until the racer arrives.
                    let u = &self.units[i];
                    if rec.level.number() >= p.chase.junction_block_level
                        && u.cmd.track.is_drivable()
                    {
                        let end = u.cmd.track.point_at(u.cmd.track.length());
                        let rv = [rec.vel[0], rec.vel[1]];
                        let d_racer = dist(rp, end);
                        let coming =
                            dot(rv, sub(end, rp)) > 0.0 && d_racer < 250.0 && d_racer > 12.0;
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
                            let v_r = (rec.vel[0] * rec.vel[0] + rec.vel[1] * rec.vel[1]).sqrt();
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
                // No pursuit speed cap: the unit's own car is the limit.
                let top = self.unit_top_speed_mps(i);
                let cap = pursuit_speed_cap(v_r, ch.speed_margin, top);
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
                } else if gap < -5.0
                    && level >= unlocks.rolling_block_level
                    && has_budget
                    && seen
                    && v_r >= ARREST_RACER_MPS
                {
                    // A rolling block slows a moving racer; a stopped one is arrested instead
                    // (a block ahead of it would wait there forever).
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
                        let arrest = v_r < ARREST_RACER_MPS && seen;
                        let want_gap = if arrest { 6.0 } else { mid };
                        let gap = if arrest {
                            gap.max(dist(u.body.pos, rp))
                        } else {
                            gap
                        };
                        // Far behind: close the gap at up to the car's top speed, braking in
                        // time to arrive at the shadow gap.
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
                        let base_cap = if far { top } else { cap };
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
                        (offset_track(&track, lat), sp.min(top))
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
                        (tr, sp.clamp(0.0, top))
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
