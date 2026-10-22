//! Patrol & pursuit driving and tactics.
//!
//! Every pursuing unit follows the racer's **breadcrumb trail** (recorded only while some unit,
//! camera or helicopter sees the racer) extended by the racer's predicted path. Units therefore
//! lose the scent when line of sight is lost, which is what makes escapes possible.
//!
//! Speed: pursuers drive at `min(target_speed × (1 + margin), pursuit_cap[phase])` and plan
//! corners with `pursuit_cornering_g`; time spent above `risk.lateral_g`, near misses and
//! collisions drain a **risk budget**; once it is spent the unit only shadows, at a gentler
//! cornering g.
//!
//! Tactics per pursued racer (re-evaluated at think rate, closest pursuer first):
//! - `BoxIn` — racer below `boxin_max_kmh`, ≥ 2 pursuers with budget within 60 m: rear,
//!   front-left, front-right slots.
//! - `Pit` — racer below `pit_max_kmh[mode]`, ≥ `pit_min_straight_m` of straight road ahead,
//!   lead unit has budget and is off cooldown.
//! - `RollingBlock` — a unit that got ahead of the racer slows in front of it.
//! - `Shadow` — everyone else: hold 25–40 m back (further for each extra unit).
//! - `Intercept` (assignment) — units off the trail or far behind route over the road graph to
//!   the racer's predicted position 10–20 s out (lane-legal shortest path).
//! - Disengage — health below `disengage_health[mode]` or a bent wheel / flat: pull over
//!   (handled in `dispatch`).

use crate::radio::{RadioCue, RadioTier};
use wp_contracts::{EntityId, WorldQuery, GRAVITY_MPS2};

use crate::dispatch::ROAD_HALF_WIDTH_M;
use crate::events::{PoliceEvent, PursuitOutcome, UnitId, UnitKind};
use crate::graph::{dist, dot, normalize, perp_left, scale, sub, xy, NodeId, PoiKind};
use crate::heat::{PursuitStatus, RuleInputs, RuleOutcome, WantedLevel};
use crate::unit::{
    plan_unit_speed, step_ground, step_helo, Assignment, BoxSlot, DriveCommand, DriveEnv,
    SpeedLimiter, Tactic, Track, UnitRole, LANE_OFFSET_M,
};
use crate::{PoliceSystem, PursuitRecord, RacerObs};

/// Below this speed a racer counts as stopped for interception, m/s.
pub(crate) const STOPPED_RACER_MPS: f32 = 2.0;

/// An ambush wait point this close to the racer's predicted path still counts as on it, m.
const AMBUSH_PATH_TOLERANCE_M: f32 = 8.0;

/// An ambusher joins the chase only facing within ~45° of the racer's line, cos.
const AMBUSH_JOIN_MIN_COS: f32 = 0.7;

/// A pursuer this close to the racer tracks the racer's end of the breadcrumb trail, m.
const NEAR_RACER_M: f32 = 40.0;

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

    pub(crate) fn update_observations(&mut self, world: &dyn WorldQuery) {
        let los_range = self.cfg.pursuit.los_range_m.get(self.phase);
        let witness = self.cfg.heat.heat.witness_range_m.get(self.phase);
        let recognise = self.cfg.heat.heat.recognition_range_m.get(self.phase);
        let helo_range = self.cfg.tools.helicopter.los_range_m.get(self.phase);
        let ids: Vec<EntityId> = self.racers.keys().copied().collect();
        for id in ids {
            let p = xy(self.racers[&id].pos);
            let mut o = RacerObs {
                nearest_unit_m: f32::INFINITY,
                ..Default::default()
            };
            let mut best_spot = f32::INFINITY;
            for u in &self.units {
                if u.wrecked {
                    continue;
                }
                let d = dist(u.body.pos, p);
                match u.kind {
                    UnitKind::Car => {
                        o.nearest_unit_m = o.nearest_unit_m.min(d);
                        if u.assignment == Assignment::OutOfService {
                            continue;
                        }
                        if d <= los_range && self.sees(world, u.body.pos, p) {
                            // Recognition (pursuit start) and witnessing (full violation heat)
                            // have their own ranges inside line-of-sight range.
                            o.seen = true;
                            o.witnessed |= d <= witness;
                            if d <= recognise && d < best_spot {
                                best_spot = d;
                                o.spotter = Some(u.id);
                            }
                        }
                    }
                    UnitKind::Helicopter => {
                        if matches!(u.assignment, Assignment::HeloTrack { target } if target == id)
                            && d <= helo_range
                            // Looking down from altitude: only tall buildings block.
                            && world.line_of_sight(
                                [u.body.pos[0], u.body.pos[1], u.body.alt_m],
                                [p[0], p[1], crate::EYE_HEIGHT_M],
                            )
                        {
                            o.seen = true;
                        }
                    }
                }
            }
            if self.camera_sees(world, p) {
                o.seen = true;
                o.witnessed = true;
            }
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
            let o = self.obs.get(&id).copied().unwrap_or(RacerObs {
                nearest_unit_m: f32::INFINITY,
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
                if after > before && self.racers[&id].pursued() {
                    self.radio(RadioCue::new(
                        RadioTier::Critical,
                        None,
                        format!(
                            "Dispatch: racer #{} now wanted level {}.",
                            id.0,
                            after.number()
                        ),
                    ));
                }
            }
            match outcome {
                Some(RuleOutcome::Escaped) => self.end_pursuit(id, PursuitOutcome::Escaped),
                Some(RuleOutcome::Safehouse) => self.end_pursuit(id, PursuitOutcome::Safehouse),
                Some(RuleOutcome::Captured) => self.end_pursuit(id, PursuitOutcome::Captured),
                None => {}
            }
            // A unit that keeps a wanted racer in recognition range long enough starts a pursuit.
            let acc = self.recognition_s.entry(id).or_insert(0.0);
            *acc = if o.spotter.is_some() { *acc + tdt } else { 0.0 };
            let recognised = *acc >= self.cfg.heat.heat.recognition_time_s.get(self.phase);
            let rec = &self.racers[&id];
            if !rec.pursued() && rec.level > WantedLevel::None && recognised {
                if let Some(spotter) = o.spotter {
                    self.start_pursuit(id, spotter);
                }
            }
        }
    }

    fn start_pursuit(&mut self, racer: EntityId, unit: UnitId) {
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
        self.radio(RadioCue::new(
            RadioTier::Critical,
            None,
            format!("Dispatch: 10-80, all units, pursuit of {desc}."),
        ));
        if let Some(i) = self.unit_index(unit) {
            if self.units[i].player.is_none() {
                self.units[i].racer_contact = false;
                self.assign(i, Assignment::Pursue { target: racer });
            }
        }
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

    /// End the pursuit of `racer` with `outcome` (rules, total loss, or captain call-off).
    pub(crate) fn end_pursuit(&mut self, racer: EntityId, outcome: PursuitOutcome) {
        let now = self.now;
        let cfg = self.cfg.heat.clone();
        let was_pursued = self.racers[&racer].pursued();
        let level_at_end = self.racers[&racer].level;
        let (capturer, assists) = if outcome.is_capture() {
            self.capture_credit(racer)
        } else {
            (None, Vec::new())
        };
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
        let text = match outcome {
            PursuitOutcome::Escaped => format!(
                "Dispatch: all units, we've lost racer #{}. Resume patrol.",
                racer.0
            ),
            PursuitOutcome::Safehouse => format!(
                "Dispatch: racer #{} went to ground. Resume patrol.",
                racer.0
            ),
            PursuitOutcome::Captured => format!("Dispatch: racer #{} in custody. Code 4.", racer.0),
            PursuitOutcome::CapturedTotalLoss => {
                format!(
                    "Dispatch: racer #{} vehicle disabled, suspect in custody.",
                    racer.0
                )
            }
            PursuitOutcome::CalledOff => {
                format!("Dispatch: pursuit of racer #{} called off.", racer.0)
            }
        };
        self.radio(RadioCue::new(RadioTier::Critical, None, text));
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
        self.route_from(u.body.pos, u.body.heading, dest, final_point)
            .0
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
            if let Some(rp) = on_road.filter(|rp| !skip && rp.dist_m < ROAD_HALF_WIDTH_M) {
                pts[0] = rp.point;
            }
            pts.extend(
                route
                    .iter()
                    .skip(skip as usize)
                    .map(|n| self.graph.node(*n).pos),
            );
        }
        if let Some(p) = final_point {
            pts.push(p);
        }
        (Track::new(pts), turn_around)
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
        let v = [rec.vel[0], rec.vel[1]];
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
        for k in 0..3 {
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
                Assignment::Idle => {
                    if let UnitRole::Pool(d) = self.units[i].role {
                        if let Some(dest) = self.random_node_in(Some(d)) {
                            self.assign(i, Assignment::Patrol { dest });
                        }
                    }
                }
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
                    if !self.units[i].cmd.track.is_drivable() {
                        let dest = self.graph.nearest_node(inc.pos).unwrap_or(NodeId(0));
                        let t = self.route_track(i, dest, Some(inc.pos));
                        self.units[i].set_command(DriveCommand {
                            track: t,
                            speed_mps: 0.0,
                            cornering_g: p.patrol_cornering_g,
                            siren: true,
                            lane_offset_m: LANE_OFFSET_M,
                        });
                    }
                    let lim = self.limit_at(self.units[i].body.pos);
                    self.units[i].cmd.speed_mps = lim * p.respond_speed_factor;
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
                    // the off-trail check in coordinate_pursuits). An ambusher may join while
                    // still ahead (and shadow or block), but only once it is on the racer's road
                    // facing its way: not while still turning in from its side road.
                    if seen && dist(u.body.pos, rp) < 120.0 && self.sees(world, u.body.pos, rp) {
                        let joins = self.pursuit_track(target).is_some_and(|(tr, racer_s)| {
                            let (s_u, lat) = tr.project(u.body.pos, racer_s, 200.0);
                            let along = normalize(sub(tr.point_at(s_u + 5.0), tr.point_at(s_u)));
                            let ok = if ambush {
                                // Turned onto the racer's road: ≥ 5 m past the junction.
                                let in_place = u.track_s
                                    > u.cmd.track.length()
                                        - (crate::dispatch::AMBUSH_INTO_LEG_M - 5.0);
                                in_place && dot(u.body.heading, along) > AMBUSH_JOIN_MIN_COS
                            } else {
                                s_u <= racer_s + 5.0
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
                            let t = self.route_track(i, node, fin);
                            self.units[i].assignment = Assignment::Intercept {
                                target,
                                point,
                                ambush: false,
                            };
                            self.units[i].set_command(DriveCommand {
                                track: t,
                                speed_mps: 0.0,
                                cornering_g: p.pursuit_cornering_g,
                                siren: true,
                                lane_offset_m: LANE_OFFSET_M,
                            });
                        }
                    }
                    let lim = self.limit_at(self.units[i].body.pos);
                    let cap = p.pursuit_cap_mps.get(self.phase);
                    self.units[i].cmd.speed_mps = (lim * p.respond_speed_factor * 1.15).min(cap);
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
        self.coordinate_pursuits();
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
                u.planned_speed = plan_unit_speed(u, brake);
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
        let v = [rec.vel[0], rec.vel[1]];
        let ahead = self.graph.predict_path(last, v, 250.0);
        let mut pts = trail.pts.clone();
        pts.extend(ahead.points().into_iter().skip(1));
        Some((Track::new(pts), racer_s))
    }

    fn coordinate_pursuits(&mut self) {
        let now = self.now;
        let p = self.cfg.pursuit.clone();
        let t = &p.tactics;
        let unlocks = self.cfg.presence.unlocks.clone();
        let mode = self.setup.mode;
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
            let v_r = if seen {
                (rec.vel[0] * rec.vel[0] + rec.vel[1] * rec.vel[1]).sqrt()
            } else {
                0.0
            };
            let cap = pursuit_speed_cap(
                v_r,
                p.target_speed_margin,
                p.pursuit_cap_mps.get(self.phase),
            );
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
                    track.project(u.body.pos, hint, 200.0)
                };
                pursuers.push((racer_s - s, lat, i));
            }
            pursuers.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.2.cmp(&b.2)));
            // Units off the trail or far behind go intercept.
            let mut active = Vec::new();
            for &(gap, lat, i) in &pursuers {
                let held = now < self.units[i].captain_hold_until;
                if !held && (lat > 30.0 || gap > t.intercept_min_gap_m + 100.0) {
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
            let straight_ahead = {
                let mut ok = true;
                let mut d = 0.0;
                while d <= t.pit_min_straight_m {
                    if track.radius_at(racer_s + d, 10.0) < 300.0 {
                        ok = false;
                        break;
                    }
                    d += 10.0;
                }
                ok
            };
            let with_budget: Vec<(f32, usize)> = active
                .iter()
                .copied()
                .filter(|(gap, i)| self.units[*i].risk > 0.0 && gap.abs() < 60.0)
                .collect();
            let boxin = seen
                && level >= unlocks.boxin_level
                && v_r < t.boxin_max_kmh / 3.6
                && v_r > 1.0
                && with_budget.len() >= 2;
            let mut shadow_rank = 0.0f32;
            for (rank, &(gap, i)) in active.iter().enumerate() {
                let u = &self.units[i];
                let has_budget = u.risk > 0.0;
                let tactic = if boxin && with_budget.iter().take(3).any(|x| x.1 == i) {
                    let k = with_budget.iter().position(|x| x.1 == i).unwrap_or(0);
                    Tactic::BoxIn(
                        [BoxSlot::Rear, BoxSlot::FrontLeft, BoxSlot::FrontRight][k.min(2)],
                    )
                } else if gap < -5.0 && level >= unlocks.rolling_block_level && has_budget && seen {
                    Tactic::RollingBlock
                } else if rank == 0
                    && seen
                    && has_budget
                    && level >= unlocks.pit_level
                    && v_r < t.pit_max_kmh.get(mode) / 3.6
                    && v_r > 3.0
                    && straight_ahead
                    && now >= u.pit_ready
                    && gap < 40.0
                {
                    Tactic::Pit
                } else {
                    Tactic::Shadow
                };
                let cornering = if has_budget {
                    p.pursuit_cornering_g
                } else {
                    p.shadow_cornering_g
                };
                let mut why = SpeedLimiter::TacticOther;
                let (cmd_track, speed) = match tactic {
                    Tactic::Shadow => {
                        let mid = (t.shadow_m.min + t.shadow_m.max) * 0.5 + 12.0 * shadow_rank;
                        shadow_rank += 1.0;
                        // Close in for the arrest when the racer stops.
                        let want_gap = if v_r < 3.0 && seen { 8.0 } else { mid };
                        let mut sp = v_r + 0.6 * (gap - want_gap);
                        if !seen {
                            // Lost contact: drive to the last known position at a measured pace.
                            sp = self.limit_at(u.body.pos) * p.respond_speed_factor;
                        }
                        let own_cap = if has_budget {
                            cap
                        } else {
                            cap.min(self.limit_at(u.body.pos) * 1.3)
                        };
                        why = if sp < own_cap {
                            if seen {
                                SpeedLimiter::TacticGap
                            } else {
                                SpeedLimiter::TacticLostContact
                            }
                        } else if own_cap < cap {
                            SpeedLimiter::TacticNoBudget
                        } else {
                            SpeedLimiter::TacticMargin
                        };
                        (track.clone(), sp.clamp(0.0, own_cap))
                    }
                    Tactic::Pit => (track.clone(), (v_r + 3.0).min(cap + 3.0)),
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
                u.tactic_limiter = why;
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Per-tick driving
    // -----------------------------------------------------------------------------------------

    pub(crate) fn drive_units(&mut self, dt: f32) {
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
                    self.stats.near_misses += rep.near_miss as u32;
                    self.stats.traffic_collisions += rep.traffic_collision as u32;
                    self.stats.run_wide_crashes += rep.crashed as u32;
                    let u = &mut self.units[i];
                    if pursuing {
                        if u.body.lat_g > cfg.risk.lateral_g {
                            u.risk -= cfg.risk.cost_per_s_lateral * dt;
                        }
                        if rep.near_miss {
                            u.risk -= cfg.risk.cost_near_miss;
                        }
                        if rep.traffic_collision || rep.crashed {
                            u.risk -= cfg.risk.cost_collision;
                        }
                    }
                }
            }
        }
        self.internal_racer_contacts();
    }

    /// Proximity contacts between internally driven units and racers (physical units get
    /// `SimEvent::Contact` from Rapier instead).
    fn internal_racer_contacts(&mut self) {
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
                    let pit = self.units[i].tactic == Tactic::Pit;
                    let before = self.stats.racer_contacts;
                    self.unit_racer_contact(i, e, rel_speed, true);
                    if self.stats.racer_contacts != before {
                        let u = &mut self.units[i];
                        u.body.speed = u.body.speed.min((v[0] * v[0] + v[1] * v[1]).sqrt()) * 0.8;
                        if pit {
                            u.pit_ready = self.now.plus(self.cfg.pursuit.tactics.pit_cooldown_s);
                            let id = u.id;
                            self.stats.pit_attempts += 1;
                            self.pending.push(PoliceEvent::PitAttempt {
                                unit: id,
                                target: e,
                            });
                        }
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
