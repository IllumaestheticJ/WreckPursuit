//! Presence & dispatch: the patrol pool per district and phase, incidents (civilian reports
//! and crash scenes), reinforcements with phase-dependent response times, the helicopter, and
//! the AI captain that places tools when no human captain is seated.
//!
//! Every assignment change goes through [`PoliceSystem::assign`], which emits a radio line.

use crate::radio::{RadioCue, RadioTier};
use serde::{Deserialize, Serialize};
use wp_contracts::{EntityId, ViolationKind, WorldQuery};

use crate::captain::CaptainError;
use crate::events::{
    DisableCause, IncidentId, PoliceEvent, PursuitOutcome, ToolId, UnitId, UnitKind,
};
use crate::graph::{
    add, dist, lerp2, normalize, perp_left, scale, sub, xy, District, NodeId, PoiKind, RoadClass,
};
use crate::heat::{PursuitStatus, WantedLevel};
use crate::time::SimTime;
use crate::tools::{
    compass, plan_site, roadblock_barriers, roadblock_unit_spots, Roadblock, SiteRules, SpikeStrip,
};
use crate::unit::{Assignment, DriveCommand, Tactic, Unit, UnitRole, LANE_OFFSET_M};
use crate::PoliceSystem;

/// Road half width used for tool layout, m.
pub const ROAD_HALF_WIDTH_M: f32 = 5.0;

/// A reckless-driving report this close to an open call about another racer joins that call
/// (a pack of street racers is one call), m.
const CALL_MERGE_M: f32 = 200.0;

/// World contacts closer together than this are one static-geometry episode, s.
const STATIC_EPISODE_S: f32 = 1.0;

/// Spots tried per pool spawn before giving up until the next think step.
const SPAWN_TRIES: usize = 8;

/// Kind of incident.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IncidentKind {
    /// Civilian report of an unseen violation by a racer (position is approximate).
    Report {
        /// Reported racer.
        racer: EntityId,
    },
    /// Crash scene (civilian pile-up); occupies a unit for `crash_scene_s`.
    Crash,
}

/// An incident needing a unit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Incident {
    /// Id.
    pub id: IncidentId,
    /// Kind.
    pub kind: IncidentKind,
    /// Location, m.
    pub pos: [f32; 2],
    /// District.
    pub district: District,
    /// Created at.
    pub created: SimTime,
    /// A unit is dispatched no earlier than this.
    pub dispatch_at: SimTime,
    /// Responding unit.
    pub unit: Option<UnitId>,
    /// Crash scene: the unit is released at this time.
    pub scene_until: Option<SimTime>,
    /// Report: a traffic camera flagged it (otherwise a civilian called it in).
    #[serde(default)]
    pub camera: bool,
    /// The call has been put out on the radio (with the first unit sent, or alone if none
    /// could be).
    #[serde(default)]
    pub announced: bool,
}

/// One racer–unit contact as police sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RacerHit {
    /// Normal closing speed at the contact, m/s.
    pub(crate) rel_speed: f32,
    /// Normal impulse of this contact, N·s (physical contacts).
    pub(crate) impulse_ns: f32,
    /// A physics contact (`SimEvent::Contact`). The headless proximity stand-in (no collider:
    /// a scripted racer drives through a unit, and in `wp_sim` a player can't touch an
    /// overflow kinematic unit) costs risk and health but is never an assault.
    pub(crate) physical: bool,
}

/// Contacts between one racer and one unit that belong together (closer than
/// `heat.assault.episode_s`), summed to tell a deliberate hit from a nudge.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub(crate) struct ContactEpisode {
    /// Latest contact.
    pub(crate) last: Option<SimTime>,
    /// Summed normal impulse, N·s.
    pub(crate) impulse_ns: f32,
    /// Highest racer-to-unit relative speed, m/s.
    pub(crate) rel_speed_mps: f32,
    /// Highest racer speed, m/s.
    pub(crate) racer_speed_mps: f32,
    /// Latest contact that counted as an assault.
    pub(crate) assault_at: Option<SimTime>,
}

/// Pending reinforcement.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ReinforcementRequest {
    /// Racer.
    pub racer: EntityId,
    /// Requested at.
    pub requested: SimTime,
    /// Arrives at.
    pub due: SimTime,
}

/// Pending helicopter launch.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HeloRequest {
    /// Racer.
    pub racer: EntityId,
    /// Launch at.
    pub launch: SimTime,
}

/// What the dispatcher says about a report ("caller reports reckless driver").
fn call_text(inc: &Incident) -> &'static str {
    if inc.camera {
        "traffic camera flagged a vehicle"
    } else {
        "caller reports reckless driver"
    }
}

impl PoliceSystem {
    // -----------------------------------------------------------------------------------------
    // Event handlers
    // -----------------------------------------------------------------------------------------

    /// A traffic violation by `racer` at world position `at` (from `SimEvent::TrafficViolation`).
    pub(crate) fn on_violation(
        &mut self,
        world: &dyn WorldQuery,
        racer: EntityId,
        kind: &ViolationKind,
        at: [f32; 2],
    ) {
        let now = self.now;
        let obs = self.obs.get(&racer).copied().unwrap_or_default();
        let p = {
            let r = &self.racers[&racer];
            xy(r.pos)
        };
        let camera = self.camera_sees(world, p);
        let witnessed = obs.witnessed || camera;
        let cfg = self.cfg.heat.clone();
        let rec = self.racers.get_mut(&racer).expect("racer exists");
        let before = rec.level;
        let added = rec.add_violation(kind, witnessed, now, &cfg);
        if witnessed {
            rec.last_seen = Some(now);
            rec.last_known = Some(p);
        }
        let after = rec.level;
        if added <= 0.0 {
            return;
        }
        if let ViolationKind::Speeding { over_mps } = kind {
            if *over_mps >= cfg.heat.blatant.over_mps
                && !self.racers[&racer].pursued()
                && self.on_blatant_speeding(world, racer, *over_mps, before)
            {
                return;
            }
        }
        if after != before {
            self.pending.push(PoliceEvent::WantedLevelChanged {
                racer,
                level: after,
            });
        }
        if !obs.witnessed && !self.racers[&racer].pursued() && !self.graph.nodes.is_empty() {
            // No unit saw it: a civilian calls it in, or a traffic camera flags it, at the
            // violation's position, so a unit is sent to look (a camera alone never sends
            // anyone otherwise, and a racer that stays put would never be found). One open
            // report per racer; later calls update the position.
            let district = self.graph.district_at(at);
            if let Some(inc) = self
                .incidents
                .iter_mut()
                .find(|i| i.kind == IncidentKind::Report { racer })
            {
                inc.pos = at;
                inc.district = district;
            } else if self.incidents.iter().any(|i| {
                matches!(i.kind, IncidentKind::Report { .. }) && dist(i.pos, at) < CALL_MERGE_M
            }) {
                // Part of a call already out (a pack of racers): one call, one responder.
            } else {
                let id = IncidentId(self.next_incident);
                self.next_incident += 1;
                let rs = &self.cfg.presence.response;
                let delay = rs.call_delay_s.get(self.phase) + rs.district_delay_s.get(district);
                self.stats.calls.push(crate::CallRecord {
                    racer,
                    at: now,
                    pos: at,
                    spawned: false,
                });
                self.incidents.push(Incident {
                    id,
                    kind: IncidentKind::Report { racer },
                    pos: at,
                    district,
                    created: now,
                    dispatch_at: now.plus(delay),
                    unit: None,
                    scene_until: None,
                    camera,
                    announced: false,
                });
                // On the radio together with the unit sent (`radio_text`), a few seconds on.
            }
        }
    }

    /// Speeding `over_mps` over the limit right past a unit that sees it (row 60): that unit
    /// recognises the racer at once and starts a pursuit; wanted at least `blatant.min_level`.
    /// `false` if no in-service unit is close enough with line of sight.
    fn on_blatant_speeding(
        &mut self,
        world: &dyn WorldQuery,
        racer: EntityId,
        over_mps: f32,
        before: WantedLevel,
    ) -> bool {
        let b = self.cfg.heat.heat.blatant.clone();
        let r = &self.racers[&racer];
        let (rp, rz) = (xy(r.pos), r.pos[2]);
        let mut best: Option<(f32, usize)> = None;
        for (i, u) in self.units.iter().enumerate() {
            if u.kind != UnitKind::Car
                || u.wrecked
                || u.player.is_some()
                || u.assignment == Assignment::OutOfService
            {
                continue;
            }
            let d = dist(u.body.pos, rp);
            if d <= b.range_m
                && best.is_none_or(|x| d < x.0)
                && self.sees_at(world, u.body.pos, u.body.alt_m, rp, rz)
            {
                best = Some((d, i));
            }
        }
        let Some((_, i)) = best else {
            return false;
        };
        let now = self.now;
        let cfg = self.cfg.heat.clone();
        let after = {
            let rec = self.racers.get_mut(&racer).expect("racer exists");
            let floor = cfg.heat.level_thresholds[(b.min_level.clamp(1, 5) - 1) as usize];
            if rec.heat < floor {
                rec.add_heat(floor - rec.heat, &cfg);
            }
            rec.last_seen = Some(now);
            rec.last_known = Some(rp);
            rec.level
        };
        if after != before {
            self.pending.push(PoliceEvent::WantedLevelChanged {
                racer,
                level: after,
            });
        }
        self.stats.blatant += 1;
        let unit = self.units[i].id;
        let desc = self.racer_desc(racer);
        self.radio(RadioCue::new(
            RadioTier::Critical,
            Some(unit),
            format!(
                "Unit {}: {desc} just blew past me at {:.0} over, I'm on it.",
                unit.0,
                over_mps * 3.6
            ),
        ));
        self.start_pursuit(racer, unit);
        true
    }

    pub(crate) fn on_racer_total_loss(&mut self, racer: EntityId) {
        let r = &self.racers[&racer];
        if r.level > WantedLevel::None {
            self.end_pursuit(racer, PursuitOutcome::CapturedTotalLoss);
        }
    }

    /// A `SimEvent::Contact` between `a` and `b`: a racer hitting a unit (see
    /// [`PoliceSystem::unit_racer_contact`]) or a civilian–civilian crash scene.
    pub(crate) fn on_contact(
        &mut self,
        world: &dyn WorldQuery,
        a: EntityId,
        b: EntityId,
        point: [f32; 3],
        rel_speed: f32,
        impulse_ns: f32,
    ) {
        let ua = self.unit_index_by_entity(a);
        let ub = self.unit_index_by_entity(b);
        let ra = self.racers.contains_key(&a);
        let rb = self.racers.contains_key(&b);
        let hit = RacerHit {
            rel_speed,
            impulse_ns,
            physical: true,
        };
        match (ua, ub, ra, rb) {
            (Some(i), _, _, true) => self.unit_racer_contact(world, i, b, hit, false),
            (_, Some(i), true, _) => self.unit_racer_contact(world, i, a, hit, false),
            // Civilian–civilian crash (not a car hitting a wall).
            (None, None, false, false)
                if rel_speed >= self.cfg.presence.crash_incident_min_speed_mps
                    && b != wp_contracts::WORLD_ENTITY =>
            {
                self.report_crash_scene(xy(point));
            }
            _ => {}
        }
    }

    /// Physical unit `i` has made no progress for `stuck::STUCK_AFTER_S` while wanting to move:
    /// log why, back out, re-route on a repeat, give up after `stuck::GIVE_UP_S`.
    pub(crate) fn on_stuck(&mut self, i: usize, held: bool) {
        use crate::stuck::{self, ClassifyInput, Phase, UnstickStep};
        let now = self.now;
        let u = &self.units[i];
        let pos = u.body.pos;
        let junction_m = self
            .graph
            .nodes
            .iter()
            .filter(|n| n.junction)
            .map(|n| dist(n.pos, pos))
            .fold(f32::MAX, f32::min);
        let damaged =
            u.effects.engine_dead || u.must_pull_over(&self.cfg.pursuit) || u.health < 0.25;
        let (cause, contact_h_m) = stuck::classify(&ClassifyInput {
            now,
            pos,
            alt_m: u.body.alt_m,
            damaged,
            vehicle_ahead: held || u.avoid.blocked_by.is_some(),
            state: &u.unstick,
            props: &self.props,
            junction_m,
        });
        let segment = self.graph.nearest_on_road(pos).map(|r| r.segment);
        let (id, heading) = (u.id, u.body.heading);
        let behind_clear = self.room_behind(i, stuck::REVERSE_M);
        let recovery = self.unstick_recovery;
        let st = &mut self.units[i].unstick;
        st.slow_s = 0.0;
        st.held_s = 0.0;
        if !recovery {
            self.stats.stuck.push(stuck::StuckRecord {
                unit: id,
                at: now,
                pos,
                cause,
                step: UnstickStep::Logged,
                contact_h_m,
            });
            return;
        }
        st.attempts += 1;
        let (first_at, first_pos) = *st.first.get_or_insert((now, pos));
        let mut step = UnstickStep::Reverse;
        // Held behind a stopped vehicle with cars queued behind too: a jam, not a dead car.
        // Wait; turning around would cut across the traffic.
        let jam = held && !behind_clear;
        if !jam && (held || st.attempts >= 2 && dist(first_pos, pos) <= stuck::SAME_SPOT_M) {
            if let Some(s) = segment {
                st.blocked.retain(|(b, until)| *b != s && now < *until);
                st.blocked.push((s, now.plus(stuck::BLOCK_SEGMENT_S)));
                st.reroute = true;
                step = UnstickStep::Reroute;
            }
        }
        if now.since(first_at) >= stuck::GIVE_UP_S {
            st.give_up = true;
            step = UnstickStep::Respawn;
        }
        // Opposite lock to what it was steering; going straight, swing the nose away from
        // the obstacle it touched (backing with lock toward it).
        let steer = if st.last_steer.abs() > 0.1 {
            -st.last_steer.signum()
        } else {
            st.world_contact
                .map(|w| {
                    let side = heading[0] * w.normal[1] - heading[1] * w.normal[0];
                    if side > 0.0 {
                        -1.0
                    } else {
                        1.0
                    }
                })
                .unwrap_or(0.0)
        };
        if behind_clear {
            st.phase = Phase::Reverse {
                from: pos,
                dist_m: stuck::REVERSE_M,
                until: now.plus(stuck::REVERSE_MAX_S),
                steer,
            };
        }
        self.stats.stuck.push(stuck::StuckRecord {
            unit: id,
            at: now,
            pos,
            cause,
            step,
            contact_h_m,
        });
        if let Some(r) = self
            .stats
            .responses
            .iter_mut()
            .rev()
            .find(|r| r.unit == id && r.end.is_none())
        {
            r.stuck_events += 1;
        }
    }

    /// Whether unit `i` can back up `dist_m`: no vehicle or prop in the corridor behind it.
    pub(crate) fn room_behind(&self, i: usize, dist_m: f32) -> bool {
        let u = &self.units[i];
        let (id, entity, pos, heading) = (u.id, u.entity, u.body.pos, u.body.heading);
        !self.obstacles.iter().any(|o| {
            if o.unit == Some(id) || (o.entity.is_some() && o.entity == entity) {
                return false;
            }
            let rel = sub(o.pos, pos);
            let along = crate::graph::dot(rel, heading);
            let lat = crate::graph::cross(heading, rel).abs();
            along < 0.0
                && along > -(dist_m + 2.0 * crate::avoid::UNIT_HALF_LENGTH_M + o.half_len)
                && lat < 1.0 + o.half_wid
        })
    }

    /// Think-rate part of un-sticking: re-plan routes around blocked segments, and despawn
    /// units that gave up once nobody can see them (their call is dispatched again; a patrol
    /// is refilled out of sight by `maintain_pool`).
    pub(crate) fn maintain_stuck(&mut self, world: &dyn WorldQuery) {
        let mut i = 0;
        while i < self.units.len() {
            let u = &self.units[i];
            if u.unstick.reversing() {
                i += 1;
                continue;
            }
            if u.unstick.reroute {
                let u = &mut self.units[i];
                u.unstick.reroute = false;
                u.cmd.track = crate::unit::Track::default();
                u.track_s = 0.0;
            }
            // No progress GIVE_UP_S after the first event (even between events): give up.
            let now = self.now;
            let st = &mut self.units[i].unstick;
            if !st.give_up
                && st
                    .first
                    .is_some_and(|(t, _)| now.since(t) >= crate::stuck::GIVE_UP_S)
            {
                st.give_up = true;
                let u = &self.units[i];
                let (id, pos) = (u.id, u.body.pos);
                if let Some(last) = self.stats.stuck.iter().rev().find(|s| s.unit == id) {
                    let rec = crate::stuck::StuckRecord {
                        at: now,
                        pos,
                        step: crate::stuck::UnstickStep::Respawn,
                        ..*last
                    };
                    self.stats.stuck.push(rec);
                }
            }
            if self.units[i].unstick.give_up {
                if self.may_vanish(world, i) {
                    let id = self.units[i].id;
                    let cause = self
                        .stats
                        .stuck
                        .iter()
                        .rev()
                        .find(|s| s.unit == id)
                        .map(|s| s.cause)
                        .unwrap_or(crate::stuck::StuckCause::NoContact);
                    self.end_response(id, crate::ResponseOutcome::Stuck(cause));
                    for inc in self.incidents.iter_mut() {
                        if inc.unit == Some(id) {
                            inc.unit = None;
                        }
                    }
                    self.despawn(i);
                    continue;
                }
                if let Some(s) = self
                    .stats
                    .stuck
                    .iter_mut()
                    .rev()
                    .find(|s| s.unit == self.units[i].id)
                {
                    if s.step == crate::stuck::UnstickStep::Respawn {
                        s.step = crate::stuck::UnstickStep::InSight;
                    }
                }
            }
            i += 1;
        }
    }

    /// Start logging unit `i`'s trip to call `incident` about `racer`.
    fn open_response(&mut self, i: usize, incident: IncidentId, racer: EntityId) {
        let Some(rp) = self.racers.get(&racer).map(|r| xy(r.pos)) else {
            return;
        };
        let u = &self.units[i];
        let route_m = self
            .graph
            .nearest_node(u.body.pos)
            .map(|n| {
                self.graph.distances_to(rp)[n.0 as usize] + dist(u.body.pos, self.graph.node(n).pos)
            })
            .unwrap_or(f32::INFINITY);
        let spawned = self
            .stats
            .spawns
            .last()
            .is_some_and(|s| s.unit == u.id && s.at == self.now);
        let rec = crate::ResponseRecord {
            unit: u.id,
            incident,
            racer,
            spawned,
            at: self.now,
            start_m: dist(u.body.pos, rp),
            route_m,
            stuck_events: 0,
            end: None,
        };
        // A unit takes one call at a time.
        let id = u.id;
        self.end_response(id, crate::ResponseOutcome::CallEnded);
        self.stats.responses.push(rec);
    }

    /// Close unit `id`'s open trip to a call with `outcome`.
    pub(crate) fn end_response(&mut self, id: crate::UnitId, outcome: crate::ResponseOutcome) {
        let now = self.now;
        if let Some(r) = self
            .stats
            .responses
            .iter_mut()
            .rev()
            .find(|r| r.unit == id && r.end.is_none())
        {
            r.end = Some((now, outcome));
        }
    }

    /// Follow every open trip to a call: arrived within 100 m of the racer, wrecked, or the
    /// call left it (ended, or given to another unit).
    fn update_responses(&mut self) {
        const ON_SCENE_M: f32 = 100.0;
        let now = self.now;
        for k in 0..self.stats.responses.len() {
            let r = self.stats.responses[k];
            if r.end.is_some() {
                continue;
            }
            let outcome = match self.unit_index(r.unit) {
                None => Some(crate::ResponseOutcome::CallEnded),
                Some(i) => {
                    let u = &self.units[i];
                    let racer = self.racers.get(&r.racer);
                    if u.wrecked || u.assignment == Assignment::OutOfService {
                        Some(crate::ResponseOutcome::Wrecked)
                    } else if racer.is_some_and(|rr| dist(xy(rr.pos), u.body.pos) <= ON_SCENE_M) {
                        Some(crate::ResponseOutcome::Arrived)
                    } else if u.assignment
                        != (Assignment::Respond {
                            incident: r.incident,
                        })
                    {
                        Some(crate::ResponseOutcome::CallEnded)
                    } else {
                        None
                    }
                }
            };
            if let Some(o) = outcome {
                self.stats.responses[k].end = Some((now, o));
            }
        }
    }

    /// Remember a bound unit's latest static-world / vehicle contact (stuck classification,
    /// [`crate::stuck`]). `normal` points from `a` toward `b`.
    pub(crate) fn note_unit_contact(
        &mut self,
        a: EntityId,
        b: EntityId,
        point: [f32; 3],
        normal: [f32; 3],
    ) {
        let now = self.now;
        for (me, other, sign) in [(a, b, 1.0f32), (b, a, -1.0)] {
            let Some(i) = self.unit_index_by_entity(me) else {
                continue;
            };
            if other == wp_contracts::WORLD_ENTITY {
                let n = [normal[0] * sign, normal[1] * sign, normal[2] * sign];
                self.note_static_hit(i, point, n);
            }
            let st = &mut self.units[i].unstick;
            if other == wp_contracts::WORLD_ENTITY {
                st.world_contact = Some(crate::stuck::WorldContact {
                    at: now,
                    point,
                    normal: [normal[0] * sign, normal[1] * sign, normal[2] * sign],
                });
            } else {
                st.car_contact = Some(now);
            }
        }
    }

    /// Log a new static-geometry contact episode of unit `i` (`PoliceStats::static_hits`):
    /// the first world contact after [`STATIC_EPISODE_S`] without one. Ground contacts
    /// (normal mostly vertical) are not hits.
    fn note_static_hit(&mut self, i: usize, point: [f32; 3], normal: [f32; 3]) {
        let now = self.now;
        let u = &self.units[i];
        if normal[2].abs() >= 0.6 {
            return;
        }
        let fresh = u
            .unstick
            .world_contact
            .is_none_or(|w| now.since(w.at) > STATIC_EPISODE_S || w.normal[2].abs() >= 0.6);
        if !fresh {
            return;
        }
        let p = [point[0], point[1]];
        let h = point[2] - u.body.alt_m;
        let kind = if self.props.nearest(p).is_some_and(|d| d <= 0.6) {
            crate::stuck::StuckCause::ParkedProp
        } else if h < 0.35 {
            let near_junction = self
                .graph
                .nodes
                .iter()
                .any(|n| n.junction && dist(n.pos, p) <= 22.0);
            if near_junction {
                crate::stuck::StuckCause::JunctionStep
            } else {
                crate::stuck::StuckCause::Kerb
            }
        } else {
            crate::stuck::StuckCause::Wall
        };
        let pursuing = matches!(
            u.assignment,
            Assignment::Pursue { .. } | Assignment::Intercept { .. }
        );
        self.stats.static_hits.push(crate::StaticHit {
            unit: u.id,
            at: now,
            pos: p,
            kind,
            speed_mps: u.body.speed,
            pursuing,
        });
    }

    /// Record a racer contact on unit `i`. `apply_damage` is used by the built-in driver
    /// (physical units get damage from `wp_damage` via `sync_unit`).
    ///
    /// Every contact also feeds the pair's contact episode; a hard enough one is an assault on
    /// an officer ([`PoliceSystem::on_assault`]).
    pub(crate) fn unit_racer_contact(
        &mut self,
        world: &dyn WorldQuery,
        i: usize,
        racer: EntityId,
        hit: RacerHit,
        apply_damage: bool,
    ) {
        self.track_assault(world, i, racer, hit);
        let key = (self.units[i].id, racer);
        let now = self.now;
        if self
            .contact_cooldown
            .get(&key)
            .is_some_and(|t| now.since(*t) < 1.0)
        {
            return;
        }
        self.contact_cooldown.insert(key, now);
        self.stats.racer_contacts += 1;
        let mode = self.setup.mode;
        let coef = self.cfg.pursuit.unit.contact_health_per_kmps2.get(mode);
        // A contact the unit meant (PIT, ram, box-in on its target) costs less risk, and the
        // unit re-engages after `chase.reengage_s`.
        let u = &self.units[i];
        let meant = u.assignment.target() == Some(racer)
            && matches!(u.tactic, Tactic::Pit | Tactic::Ram(_) | Tactic::BoxIn(_));
        let cost = if meant {
            self.cfg.pursuit.chase.tactic_contact_cost
        } else {
            self.cfg.pursuit.risk.cost_collision
        };
        match u.tactic {
            Tactic::Pit if meant => {
                self.stats.pit_attempts += 1;
                self.pending.push(PoliceEvent::PitAttempt {
                    unit: u.id,
                    target: racer,
                });
            }
            Tactic::Ram(_) if meant => self.stats.rams += 1,
            _ => {}
        }
        let reengage = self.cfg.pursuit.chase.reengage_s;
        let u = &mut self.units[i];
        if meant {
            u.pit_ready = now.plus(reengage);
        }
        u.racer_contact = true;
        u.risk -= cost;
        if apply_damage {
            let v = hit.rel_speed;
            u.health = (u.health - coef * v * v / 1000.0).max(0.0);
        }
    }

    /// Accumulate one racer–unit contact into the pair's episode and fire
    /// [`PoliceSystem::on_assault`] when the episode crosses the `heat.assault` thresholds.
    fn track_assault(&mut self, world: &dyn WorldQuery, i: usize, racer: EntityId, hit: RacerHit) {
        if !hit.physical {
            return;
        }
        let a = self.cfg.heat.heat.assault.clone();
        let now = self.now;
        let u = &self.units[i];
        // A unit making contact on purpose (PIT, box-in, rolling block on this racer) is
        // doing its job, not being assaulted.
        if u.assignment.target() == Some(racer)
            && matches!(
                u.tactic,
                Tactic::Pit | Tactic::BoxIn(_) | Tactic::RollingBlock | Tactic::Ram(_)
            )
        {
            return;
        }
        let key = (u.id, racer);
        let uv = u.body.vel();
        // A unit chasing this racer that drives into it (closing from behind or the side) hit
        // the racer, not the other way round.
        if u.assignment.target() == Some(racer) {
            let rv = self.prev_vel.get(&racer).copied().unwrap_or(self.racers[&racer].vel);
            let to_racer = crate::graph::sub(xy(self.racers[&racer].pos), u.body.pos);
            let closing = crate::graph::dot(crate::graph::sub(uv, [rv[0], rv[1]]), to_racer);
            if closing > 0.0 {
                return;
            }
        }
        // The racer's velocity on the previous step, i.e. just before this tick's contact (by the
        // time a contact is reported, a head-on hit has stopped the racer and a unit driving
        // into a parked racer has shoved it).
        let before = self
            .prev_vel
            .get(&racer)
            .copied()
            .unwrap_or(self.racers[&racer].vel);
        let ep = self.contact_episodes.entry(key).or_default();
        if ep.last.is_none_or(|t| now.since(t) > a.episode_s) {
            // A new episode: judge the racer by how it came into the first contact; later
            // contacts of the episode only add impulse and normal closing speed.
            let rel = [before[0] - uv[0], before[1] - uv[1]];
            ep.impulse_ns = 0.0;
            ep.rel_speed_mps = (rel[0] * rel[0] + rel[1] * rel[1]).sqrt();
            ep.racer_speed_mps = (before[0] * before[0] + before[1] * before[1]).sqrt();
        }
        ep.last = Some(now);
        // Contacts arrive once per tick with that tick's impulse: a scrape adds up.
        ep.impulse_ns += hit.impulse_ns;
        ep.rel_speed_mps = ep.rel_speed_mps.max(hit.rel_speed);
        let hard = ep.impulse_ns >= a.min_impulse_ns
            && ep.rel_speed_mps >= a.min_rel_speed_mps
            && ep.racer_speed_mps >= a.min_racer_speed_mps;
        let fresh = ep
            .assault_at
            .is_none_or(|t| now.since(t) >= a.repeat_cooldown_s);
        if hard && fresh {
            ep.assault_at = Some(now);
            self.on_assault(world, i, racer);
        }
    }

    /// A physical racer–unit contact given by unit id (police scenarios script a sideswipe of
    /// a headless unit with it; `wp_sim` contacts arrive as `SimEvent::Contact`).
    pub(crate) fn scripted_contact(
        &mut self,
        world: &dyn WorldQuery,
        unit: UnitId,
        racer: EntityId,
        impulse_ns: f32,
        rel_speed: f32,
    ) {
        if let Some(i) = self.unit_index(unit) {
            if self.racers.contains_key(&racer) {
                let hit = RacerHit {
                    rel_speed,
                    impulse_ns,
                    physical: true,
                };
                self.unit_racer_contact(world, i, racer, hit, self.units[i].internal);
            }
        }
    }

    /// Racer `racer` hit unit `i` hard enough to count as assaulting an officer: heat to at
    /// least `heat.assault.min_level`, the unit (and nearby units that see it) recognise the
    /// racer at once, and a pursuit starts now, day or night.
    pub(crate) fn on_assault(&mut self, world: &dyn WorldQuery, i: usize, racer: EntityId) {
        let now = self.now;
        let cfg = self.cfg.heat.clone();
        let a = &cfg.heat.assault;
        let unit = self.units[i].id;
        let p = xy(self.racers[&racer].pos);
        self.stats.assaults += 1;
        let (before, after) = {
            let rec = self.racers.get_mut(&racer).expect("racer exists");
            let before = rec.level;
            let floor = cfg.heat.level_thresholds[(a.min_level.clamp(1, 5) - 1) as usize];
            rec.add_heat(a.heat.max(floor - rec.heat), &cfg);
            // The officer saw exactly who hit them.
            rec.last_seen = Some(now);
            rec.last_known = Some(p);
            (before, rec.level)
        };
        if after != before {
            self.pending.push(PoliceEvent::WantedLevelChanged {
                racer,
                level: after,
            });
        }
        let desc = self.racer_desc(racer);
        self.radio(RadioCue::new(
            RadioTier::Critical,
            Some(unit),
            format!(
                "Unit {}: I've been hit! {desc} rammed my unit, assault on an officer.",
                unit.0
            ),
        ));
        // Who can act on it: the hit unit unless it is out of action, else the closest
        // in-service unit nearby that sees the racer.
        let able = |u: &Unit| {
            u.kind == UnitKind::Car && !u.wrecked && u.assignment != Assignment::OutOfService
        };
        let mut spotter = able(&self.units[i]).then_some(unit);
        let mut best = f32::INFINITY;
        for u in &self.units {
            if u.id == unit || !able(u) {
                continue;
            }
            let d = dist(u.body.pos, p);
            if d <= a.nearby_units_m && self.sees(world, u.body.pos, p) {
                // Nearby units recognise the racer straight away too.
                if spotter.is_none() && d < best {
                    best = d;
                    spotter = Some(u.id);
                }
            }
        }
        if !self.racers[&racer].pursued() {
            // No recognition_time_s wait: start the pursuit now. A unit put out of action by
            // the hit still radios it in (the pursuit starts with nobody assigned yet).
            self.start_pursuit(racer, spotter.unwrap_or(unit));
        } else if let Some(s) = spotter {
            // Already pursued: the unit that was hit joins in (dispatch keeps the unit cap).
            if let Some(k) = self.unit_index(s) {
                let u = &self.units[k];
                if u.player.is_none() && u.assignment.target() != Some(racer) {
                    self.assign(k, Assignment::Pursue { target: racer });
                }
            }
        }
        // `start_pursuit` clears the contact flag; this unit was just hit by this racer.
        self.units[i].racer_contact = true;
    }

    /// A crash scene (e.g. prompt 05's civilian pile-up) needs a unit. Scenes within 60 m of an
    /// open crash incident merge into it.
    pub fn report_crash_scene(&mut self, pos: [f32; 2]) {
        if self.graph.nodes.is_empty()
            || self
                .incidents
                .iter()
                .any(|i| i.kind == IncidentKind::Crash && dist(i.pos, pos) < 60.0)
        {
            return;
        }
        let id = IncidentId(self.next_incident);
        self.next_incident += 1;
        let district = self.graph.district_at(pos);
        self.incidents.push(Incident {
            id,
            kind: IncidentKind::Crash,
            pos,
            district,
            created: self.now,
            dispatch_at: self.now,
            unit: None,
            scene_until: None,
            camera: false,
            announced: true,
        });
        self.radio(RadioCue::new(
            RadioTier::Routine,
            None,
            format!(
                "Dispatch: collision with injuries, {:?}, need a unit.",
                district
            ),
        ));
    }

    pub(crate) fn camera_sees(&self, world: &dyn WorldQuery, p: [f32; 2]) -> bool {
        self.graph
            .pois_of(PoiKind::Camera)
            .any(|c| dist(c.pos, p) <= c.radius_m && self.sees(world, c.pos, p))
    }

    // -----------------------------------------------------------------------------------------
    // Think: orchestration
    // -----------------------------------------------------------------------------------------

    pub(crate) fn think(&mut self, world: &dyn WorldQuery, tdt: f32) {
        if self.graph.nodes.is_empty() {
            // No road network: rules only.
            self.update_observations(world);
            self.update_racer_rules(tdt);
            return;
        }
        if !self.warmed_up {
            self.warm_up(world);
        }
        self.update_observations(world);
        self.update_racer_rules(tdt);
        self.check_unit_health();
        self.maintain_stuck(world);
        self.maintain_pool(world);
        self.dispatch_incidents(world);
        self.update_responses();
        self.maintain_reinforcements(world);
        self.maintain_helicopters();
        if !self.captain.human {
            self.ai_captain_tools();
        }
        self.expire_tools();
        self.plan_units(world, tdt);
        self.refresh_junctions(world);
        self.refresh_walls();
        self.cleanup_units(world);
    }

    /// For every ground unit: the next junction on its track and its light (traffic rules in
    /// [`crate::avoid::junction_speed`]).
    pub(crate) fn refresh_junctions(&mut self, world: &dyn WorldQuery) {
        let look = self.cfg.pursuit.avoid.junction_lookahead_m;
        for i in 0..self.units.len() {
            let u = &self.units[i];
            if u.kind != UnitKind::Car || u.player.is_some() || !u.cmd.track.is_drivable() {
                self.units[i].avoid.junction = None;
                continue;
            }
            let mut best: Option<(f32, usize)> = None;
            for (k, n) in self.graph.nodes.iter().enumerate() {
                if !n.junction || dist(n.pos, u.body.pos) > look + 10.0 {
                    continue;
                }
                let (s, lat) = u.cmd.track.project_ahead(n.pos, u.track_s, 2.0, look);
                let ahead = s - u.track_s;
                // On the track (through its centre), not behind the unit's front.
                if lat > ROAD_HALF_WIDTH_M + 2.0 || ahead < -2.0 || ahead > look {
                    continue;
                }
                if best.is_none_or(|b| s < b.0) {
                    best = Some((s, k));
                }
            }
            let junction = best.map(|(s, k)| {
                let n = &self.graph.nodes[k];
                let light = if n.signalised {
                    let h = u.body.heading;
                    world
                        .signal_state_ahead([u.body.pos[0], u.body.pos[1], 0.5], [h[0], h[1], 0.0])
                        .and_then(crate::avoid::Light::from_signal)
                } else {
                    None
                };
                let t = &u.cmd.track;
                let into = normalize(sub(t.point_at(s), t.point_at(s - 8.0)));
                let out = normalize(sub(t.point_at(s + 8.0), t.point_at(s)));
                let turn = crate::graph::dot(into, out) < 0.87;
                crate::avoid::JunctionAhead { s, light, turn }
            });
            self.units[i].avoid.junction = junction;
        }
    }

    /// Static feelers for every physical ground unit (row 66, [`crate::walls`]).
    pub(crate) fn refresh_walls(&mut self) {
        let cfg = &self.cfg.pursuit.avoid;
        for u in &mut self.units {
            u.avoid.wall = if self.feelers
                && u.kind == UnitKind::Car
                && !u.internal
                && u.player.is_none()
                && !u.wrecked
            {
                crate::walls::feel(&self.walls, u.body.pos, u.body.heading, u.body.speed, cfg)
            } else {
                crate::walls::WallFeel::default()
            };
        }
    }

    /// Change a unit's assignment and say so on the radio.
    pub(crate) fn assign(&mut self, i: usize, a: Assignment) {
        if self.units[i].assignment == a {
            return;
        }
        let text = self.radio_text(i, &a);
        let now = self.now;
        {
            let u = &mut self.units[i];
            u.assignment = a;
            u.assigned = now;
            u.cmd = DriveCommand::stop();
            u.track_s = 0.0;
            if matches!(
                u.assignment,
                Assignment::Pursue { .. } | Assignment::Intercept { .. }
            ) && !u.engaged
            {
                u.engaged = true;
                self.stats.units_engaged += 1;
            }
        }
        self.radio(text);
    }

    /// Spoken description of a racer ("racer #3 northbound in Core").
    pub(crate) fn racer_desc(&self, racer: EntityId) -> String {
        match self.racers.get(&racer) {
            Some(r) => {
                let p = xy(r.pos);
                format!(
                    "racer #{} {} in {:?}",
                    racer.0,
                    compass([r.vel[0], r.vel[1]]),
                    self.graph.district_at(p)
                )
            }
            None => format!("racer #{}", racer.0),
        }
    }

    /// The radio line for unit `i` taking assignment `a`: tier and collapse rule per
    /// assignment (`data/police/radio.toml` paces them, see [`crate::radio`]).
    fn radio_text(&self, i: usize, a: &Assignment) -> RadioCue {
        let u = &self.units[i];
        let callsign = match u.kind {
            UnitKind::Car => format!("Unit {}", u.id.0),
            UnitKind::Helicopter => format!("Air {}", u.id.0),
        };
        let line = |tier: RadioTier, text: String, key: String, group: &str| {
            RadioCue::new(tier, Some(u.id), text).grouped(
                Some(callsign.clone()),
                key,
                group.to_string(),
            )
        };
        match a {
            Assignment::Idle => line(
                RadioTier::Routine,
                format!("{callsign}, clear and available."),
                "clear".into(),
                "{units}, clear and available.",
            ),
            Assignment::Patrol { .. } => line(
                RadioTier::Routine,
                format!(
                    "{callsign} resuming patrol, {:?}.",
                    self.graph.district_at(u.body.pos)
                ),
                "patrol".into(),
                "{units} resuming patrol.",
            ),
            Assignment::Respond { incident } => {
                let kind = self
                    .incidents
                    .iter()
                    .find(|x| x.id == *incident)
                    .map(|x| x.kind);
                let (what, tier) = match kind {
                    Some(IncidentKind::Report { .. }) => {
                        ("reckless driver report", RadioTier::Tactical)
                    }
                    Some(IncidentKind::Crash) => ("crash scene", RadioTier::Routine),
                    None => ("incident", RadioTier::Routine),
                };
                // A new call goes out with the first unit sent: one line, not two.
                let fresh = self
                    .incidents
                    .iter()
                    .find(|x| x.id == *incident && !x.announced);
                let text = match fresh {
                    Some(inc) => format!(
                        "Dispatch: {}, {:?}. {callsign} responding.",
                        call_text(inc),
                        inc.district
                    ),
                    None => format!("{callsign} responding to {what}."),
                };
                line(
                    tier,
                    text,
                    format!("respond:{}", incident.0),
                    &format!("{{units}} responding to {what}."),
                )
            }
            Assignment::Pursue { target } => line(
                RadioTier::Tactical,
                format!("{callsign} in pursuit of {}.", self.racer_desc(*target)),
                format!("pursue:{}", target.0),
                &format!("{{units}} in pursuit of racer #{}.", target.0),
            ),
            Assignment::Intercept { target, .. } => line(
                RadioTier::Tactical,
                format!(
                    "{callsign} moving to intercept {}.",
                    self.racer_desc(*target)
                ),
                format!("intercept:{}", target.0),
                &format!("{{units}} moving to intercept racer #{}.", target.0),
            ),
            Assignment::RoadblockAt { segment, .. } => line(
                RadioTier::Critical,
                format!("{callsign} setting up roadblock on segment {}.", segment.0),
                format!("roadblock:{}", segment.0),
                &format!("{{units}} setting up roadblock on segment {}.", segment.0),
            ),
            Assignment::SpikeAt { segment, .. } => line(
                RadioTier::Critical,
                format!("{callsign} deploying spikes on segment {}.", segment.0),
                format!("spikes:{}", segment.0),
                &format!("{{units}} deploying spikes on segment {}.", segment.0),
            ),
            Assignment::HeloTrack { target } => line(
                RadioTier::Tactical,
                format!(
                    "{callsign} overhead, tracking {}.",
                    self.racer_desc(*target)
                ),
                format!("helo:{}", target.0),
                &format!("{{units}} overhead, tracking racer #{}.", target.0),
            ),
            Assignment::OutOfService => line(
                RadioTier::Routine,
                format!("{callsign} out of service."),
                "out_of_service".into(),
                "{units} out of service.",
            ),
        }
    }

    // -----------------------------------------------------------------------------------------
    // Spawning
    // -----------------------------------------------------------------------------------------

    pub(crate) fn spawn_unit(
        &mut self,
        kind: UnitKind,
        role: UnitRole,
        pos: [f32; 2],
        heading: [f32; 2],
    ) -> usize {
        let id = self.alloc_unit_id();
        let mut u = Unit::new(id, kind, role, pos, heading, self.cfg.pursuit.risk.budget);
        u.internal = self.setup.internal_driving;
        u.assigned = self.now;
        if kind == UnitKind::Car {
            // Later spawns this think step must keep clear of this one too.
            self.obstacles.push(crate::avoid::Obstacle::car(
                None,
                Some(id),
                u.body.pos,
                [0.0, 0.0],
                u.body.heading,
            ));
        }
        self.pending.push(PoliceEvent::UnitSpawned {
            unit: id,
            kind,
            pos: [pos[0], pos[1], 0.0],
            heading: u.body.heading,
        });
        self.units.push(u);
        self.units.len() - 1
    }

    fn spawn_pool_unit(&mut self, world: &dyn WorldQuery, d: District) -> Option<usize> {
        let cands: Vec<usize> = (0..self.graph.segments.len())
            .filter(|&s| {
                self.graph.segments[s].district == d
                    && self.graph.segments[s].class != RoadClass::Highway
            })
            .collect();
        if cands.is_empty() {
            return None;
        }
        // A few tries for a spot clear of other vehicles; none clear = try again next think.
        for _ in 0..SPAWN_TRIES {
            let s = self.graph.segments[cands[self.rng.below(cands.len() as u32) as usize]];
            let t = self.rng.range_f32(0.2, 0.8);
            let a = self.graph.node(s.a).pos;
            let b = self.graph.node(s.b).pos;
            let centre = lerp2(a, b, t);
            let mut dir = normalize(sub(b, a));
            let dest = self.random_node_in(Some(d))?;
            // Face the way the first patrol route goes: a physical car turning around from
            // standstill runs over the kerb and into parked cars.
            if self.route_from(centre, dir, dest, None).1 {
                dir = scale(dir, -1.0);
            }
            // In the right-hand lane, not on the centreline.
            let pos = add(centre, scale([dir[1], -dir[0]], LANE_OFFSET_M));
            // The session-start patrols are part of the opening scene; every later refill
            // appears out of every racer's sight (no pop-in).
            let hidden = !self.warmed_up || !self.in_racer_view(world, pos);
            if hidden && self.spawn_clear(pos) {
                let i = self.spawn_unit(UnitKind::Car, UnitRole::Pool(d), pos, dir);
                self.start_patrol(i, dest);
                self.record_spawn(world, i);
                return Some(i);
            }
        }
        None
    }

    /// Put a fresh pool unit on patrol toward `dest` right away (before `wp_sim` gives it a
    /// body), so it starts driving the way it faces.
    fn start_patrol(&mut self, i: usize, dest: NodeId) {
        self.assign(i, Assignment::Patrol { dest });
        let track = self.route_track(i, dest, None);
        let p = &self.cfg.pursuit;
        let cmd = DriveCommand {
            track,
            speed_mps: 0.0,
            cornering_g: p.patrol_cornering_g,
            siren: false,
            lane_offset_m: LANE_OFFSET_M,
        };
        self.units[i].set_command(cmd);
    }

    /// Whether a ground unit may appear at `pos` (see [`crate::avoid::spawn_clear`]).
    pub(crate) fn spawn_clear(&mut self, pos: [f32; 2]) -> bool {
        self.rebuild_obstacles();
        crate::avoid::spawn_clear(pos, &self.obstacles, &self.cfg.pursuit.avoid)
    }

    pub(crate) fn despawn(&mut self, i: usize) {
        let id = self.units[i].id;
        self.units.remove(i);
        self.pending.push(PoliceEvent::UnitDespawned { unit: id });
    }

    fn ground_units_alive(&self) -> u32 {
        self.units
            .iter()
            .filter(|u| u.kind == UnitKind::Car && u.role != UnitRole::Player)
            .count() as u32
    }

    fn warm_up(&mut self, world: &dyn WorldQuery) {
        for d in District::ALL {
            let target = self.cfg.presence.patrols.get(d).get(self.phase);
            for _ in 0..target {
                self.spawn_pool_unit(world, d);
            }
        }
        self.warmed_up = true;
    }

    // -----------------------------------------------------------------------------------------
    // Pool, incidents, reinforcements, helicopter
    // -----------------------------------------------------------------------------------------

    fn maintain_pool(&mut self, world: &dyn WorldQuery) {
        for d in District::ALL {
            let target = self.cfg.presence.patrols.get(d).get(self.phase);
            let alive = self.pool_count(d);
            if alive < target && self.ground_units_alive() < self.cfg.presence.max_ground_units {
                self.spawn_pool_unit(world, d);
            } else if alive > target {
                // Retire one idle pool unit nobody can see.
                if let Some(i) = (0..self.units.len()).find(|&i| {
                    let u = &self.units[i];
                    u.role == UnitRole::Pool(d) && u.available() && self.may_vanish(world, i)
                }) {
                    self.despawn(i);
                }
            }
        }
    }

    fn nearest_available(&self, p: [f32; 2], max_m: f32) -> Option<usize> {
        let mut best: Option<(f32, usize)> = None;
        for (i, u) in self.units.iter().enumerate() {
            if !u.available()
                || u.player.is_some()
                || !matches!(
                    u.role,
                    UnitRole::Pool(_) | UnitRole::Reinforcement | UnitRole::Responder
                )
            {
                continue;
            }
            let d = dist(u.body.pos, p);
            if d <= max_m && best.is_none_or(|b| d < b.0) {
                best = Some((d, i));
            }
        }
        best.map(|b| b.1)
    }

    fn dispatch_incidents(&mut self, world: &dyn WorldQuery) {
        let now = self.now;
        // Reports about racers already in pursuit are moot; stale reports expire.
        let racers = &self.racers;
        self.incidents.retain(|inc| match inc.kind {
            IncidentKind::Report { racer } => {
                !racers.get(&racer).is_some_and(|r| r.pursued()) && now.since(inc.created) < 180.0
            }
            IncidentKind::Crash => inc.scene_until.is_none_or(|t| now < t),
        });
        // Release units whose incident disappeared.
        for i in 0..self.units.len() {
            if let Assignment::Respond { incident } = self.units[i].assignment {
                if !self.incidents.iter().any(|x| x.id == incident) {
                    self.assign(i, Assignment::Idle);
                }
            }
        }
        // A call whose unit fell behind the racer (it turned away) is dispatched again; the
        // old unit drives on (and, if spawned for it, despawns out of sight).
        let (wait, far) = (
            self.cfg.presence.response.redispatch_s,
            self.cfg.presence.response.redispatch_far_m,
        );
        for k in 0..self.incidents.len() {
            let inc = self.incidents[k];
            let (IncidentKind::Report { racer }, Some(id)) = (inc.kind, inc.unit) else {
                continue;
            };
            let (Some(i), Some(r)) = (self.unit_index(id), self.racers.get(&racer)) else {
                continue;
            };
            let u = &self.units[i];
            if now.since(u.assigned) > wait && dist(u.body.pos, xy(r.pos)) > far {
                self.end_response(id, crate::ResponseOutcome::FellBehind);
                self.incidents[k].unit = None;
                self.assign(i, Assignment::Idle);
                self.stats.redispatches += 1;
            }
        }
        for k in 0..self.incidents.len() {
            let inc = self.incidents[k];
            if inc.unit.is_some() || now < inc.dispatch_at {
                continue;
            }
            let unit = match inc.kind {
                IncidentKind::Report { racer } => self.call_responder(world, racer, inc.pos),
                IncidentKind::Crash => self.nearest_available(inc.pos, f32::INFINITY),
            };
            if let Some(i) = unit {
                self.incidents[k].unit = Some(self.units[i].id);
                self.assign(i, Assignment::Respond { incident: inc.id });
                if let IncidentKind::Report { racer } = inc.kind {
                    self.open_response(i, inc.id, racer);
                }
            } else if !inc.announced {
                // Nobody to send: the call still goes out.
                let text = format!("Dispatch: {}, {:?}.", call_text(&inc), inc.district);
                self.radio(RadioCue::new(RadioTier::Tactical, None, text));
            }
            self.incidents[k].announced = true;
        }
    }

    /// The unit that answers a reckless-driving call about `racer` (reported at `at`): the
    /// available unit with the shortest drive to the racer if that is within the district's
    /// `response.radius_m` and the unit isn't behind a moving racer (it would only chase it),
    /// else a responder spawned out of sight near the racer, else the nearest available unit
    /// anywhere.
    fn call_responder(
        &mut self,
        world: &dyn WorldQuery,
        racer: EntityId,
        at: [f32; 2],
    ) -> Option<usize> {
        let radius = self
            .cfg
            .presence
            .response
            .radius_m
            .get(self.graph.district_at(at));
        if let Some(i) = self.unit_placed_for_call(racer, at, radius) {
            return Some(i);
        }
        if self.ground_units_alive() < self.unit_cap() {
            self.rebuild_obstacles();
            if let Some((pos, heading)) = self.response_spawn_point(world, racer) {
                let i = self.spawn_unit(UnitKind::Car, UnitRole::Responder, pos, heading);
                self.record_spawn(world, i);
                if let Some(c) = self.stats.calls.iter_mut().rev().find(|c| c.racer == racer) {
                    c.spawned = true;
                }
                return Some(i);
            }
        }
        self.nearest_available(at, f32::INFINITY)
    }

    /// Junctions on the predicted path of a racer at `p` moving at `v` (none when it is
    /// about stopped): `(racer arrival time s, legal drive distance from every node to the
    /// junction m)`, nearest first.
    fn racer_meets(&self, p: [f32; 2], v: [f32; 2]) -> Vec<(f32, Vec<f32>)> {
        const MEET_HORIZON_S: f32 = 15.0;
        const MEET_NODES: usize = 10;
        let speed = crate::graph::len(v);
        let mut meets = Vec::new();
        if speed > 3.0 {
            let path = self.graph.predict_path(p, v, speed * MEET_HORIZON_S);
            let mut along = 0.0;
            for leg in path.legs.iter().take(MEET_NODES) {
                along += leg.len_m;
                let node = self.graph.other_end(leg.segment, leg.from_node);
                meets.push((
                    along / speed,
                    self.graph.distances_to(self.graph.node(node).pos),
                ));
            }
        }
        meets
    }

    /// The available unit with the shortest legal drive to the racer reported at `at`, if
    /// within `radius_m` by road and, for a moving racer, able to reach a junction on its
    /// predicted path before it (a unit behind the racer would only chase it).
    fn unit_placed_for_call(&self, racer: EntityId, at: [f32; 2], radius_m: f32) -> Option<usize> {
        let v = self
            .racers
            .get(&racer)
            .map_or([0.0, 0.0], |r| [r.vel[0], r.vel[1]]);
        let moving = crate::graph::len(v) > 3.0;
        let meets = self.racer_meets(at, v);
        let factor = self.cfg.pursuit.respond_speed_factor;
        let to_racer = self.graph.distances_to(at);
        let mut best: Option<(f32, usize)> = None;
        for (i, u) in self.units.iter().enumerate() {
            if !u.available()
                || u.player.is_some()
                || !matches!(
                    u.role,
                    UnitRole::Pool(_) | UnitRole::Reinforcement | UnitRole::Responder
                )
                || dist(u.body.pos, at) > radius_m
            {
                continue;
            }
            let Some(n) = self.graph.nearest_node(u.body.pos) else {
                continue;
            };
            let lead = dist(u.body.pos, self.graph.node(n).pos);
            if moving {
                let v_unit = (self.limit_near(u.body.pos) * factor).max(5.0);
                let meets_first = meets
                    .iter()
                    .any(|(t_racer, d)| (lead + d[n.0 as usize]) / v_unit <= *t_racer);
                if !meets_first {
                    continue;
                }
            }
            let d = to_racer[n.0 as usize] + lead;
            if d <= radius_m && best.is_none_or(|b| d < b.0) {
                best = Some((d, i));
            }
        }
        best.map(|b| b.1)
    }

    /// Speed limit of the road nearest `p`, m/s.
    fn limit_near(&self, p: [f32; 2]) -> f32 {
        self.graph
            .nearest_on_road(p)
            .map_or(13.9, |r| self.graph.segment(r.segment).speed_limit_mps)
    }

    /// Ground units allowed at once: `response.max_units_by_level` at the highest wanted
    /// level, never above `max_ground_units`; none at all where the patrol table has no unit
    /// for this phase (a city without police, e.g. simcli `police_cars: false`).
    pub(crate) fn unit_cap(&self) -> u32 {
        let patrols: u32 = District::ALL
            .iter()
            .map(|d| self.cfg.presence.patrols.get(*d).get(self.phase))
            .sum();
        if patrols == 0 {
            return 0;
        }
        let level = self
            .racers
            .values()
            .map(|r| r.level.number() as usize)
            .max()
            .unwrap_or(0);
        let by_level = self.cfg.presence.response.max_units_by_level;
        by_level[level.min(by_level.len() - 1)].min(self.cfg.presence.max_ground_units)
    }

    /// Whether any racer can see `p`, or is closer than `response.never_closer_m`. Seeing
    /// means line of sight, in any direction and as far as the world allows, from the
    /// driver's eye or from a chase camera behind and above the car, now or one second on at
    /// its current velocity, to a car body at `p` (taken at the racer's height). Units never appear or vanish where this is true.
    pub(crate) fn in_racer_view(&self, world: &dyn WorldQuery, p: [f32; 2]) -> bool {
        /// Driver's eye and chase camera above the racer's chassis, camera distance behind
        /// it, and the height of the car body looked at, m.
        const EYE_UP_M: f32 = 1.3;
        const CAM_UP_M: f32 = 2.5;
        const CAM_BACK_M: f32 = 6.0;
        const BODY_UP_M: f32 = 1.0;
        /// Also check from the racer's position this far ahead, s.
        const LOOK_AHEAD_S: f32 = 1.0;
        let near = self.cfg.presence.response.never_closer_m;
        self.racers.values().any(|r| {
            let rp = xy(r.pos);
            if dist(rp, p) < near {
                return true;
            }
            let z = r.pos[2];
            let body = [p[0], p[1], z + BODY_UP_M];
            let mut views = vec![[rp[0], rp[1], z + EYE_UP_M]];
            let v = [r.vel[0], r.vel[1]];
            if crate::graph::len(v) > 1.0 {
                let back = scale(normalize(v), CAM_BACK_M);
                let c = sub(rp, back);
                views.push([c[0], c[1], z + CAM_UP_M]);
                // And from where it will be a moment later: hidden now but seen a tick later
                // is pop-in too (a sightline grazing a corner).
                let ahead = add(rp, scale(v, LOOK_AHEAD_S));
                views.push([ahead[0], ahead[1], z + EYE_UP_M]);
                let c = sub(ahead, back);
                views.push([c[0], c[1], z + CAM_UP_M]);
            }
            // Police only know the racer's velocity, not which way it faces (sliding, turning,
            // stopped): the camera may be on any side.
            for d in [[1.0, 0.0], [-1.0, 0.0], [0.0, 1.0], [0.0, -1.0]] {
                let c = add(rp, scale(d, CAM_BACK_M));
                views.push([c[0], c[1], z + CAM_UP_M]);
            }
            views.iter().any(|eye| {
                self.graph.line_of_sight([eye[0], eye[1]], p) && world.line_of_sight(*eye, body)
            })
        })
    }

    /// Whether unit `i` may despawn now: out of every racer's sight and at least
    /// `response.despawn_min_m` from all of them.
    pub(crate) fn may_vanish(&self, world: &dyn WorldQuery, i: usize) -> bool {
        let p = self.units[i].body.pos;
        let far = self.cfg.presence.response.despawn_min_m;
        self.racers.values().all(|r| dist(xy(r.pos), p) >= far) && !self.in_racer_view(world, p)
    }

    /// Note a ground unit that appeared after the session start (stats: pop-in audit).
    fn record_spawn(&mut self, world: &dyn WorldQuery, i: usize) {
        if !self.warmed_up {
            return;
        }
        let u = &self.units[i];
        let p = u.body.pos;
        let nearest = self
            .racers
            .values()
            .map(|r| dist(xy(r.pos), p))
            .fold(f32::MAX, f32::min);
        let in_sight = self
            .racers
            .values()
            .any(|r| self.sees_at(world, xy(r.pos), r.pos[2], p, r.pos[2]));
        self.stats.spawns.push(crate::SpawnRecord {
            unit: u.id,
            role: u.role,
            at: self.now,
            pos: p,
            nearest_racer_m: nearest,
            in_sight,
        });
    }

    /// Where a responder (or reinforcement) for `racer` appears: a spot in the right-hand lane
    /// `response.spawn_min_m..spawn_max_m` (+ `spawn_lead_s` of its travel) of legal driving from the racer, out of every
    /// racer's sight and never closer than `never_closer_m`, clear of other vehicles, facing
    /// the racer along the road. A moving racer is met, not chased: spots are ranked by the
    /// earliest junction on the racer's predicted path the unit can reach first (from rest,
    /// lights on); spots that can't get ahead of it come after, by drive length (shorter, and
    /// ahead of the racer, first). `None`: no such spot.
    pub(crate) fn response_spawn_point(
        &self,
        world: &dyn WorldQuery,
        racer: EntityId,
    ) -> Option<([f32; 2], [f32; 2])> {
        let rec = self.racers.get(&racer)?;
        let target = xy(rec.pos);
        let rv = [rec.vel[0], rec.vel[1]];
        let moving = crate::graph::len(rv) > 3.0;
        let rs = &self.cfg.presence.response;
        let to_racer = self.graph.distances_to(target);
        // Sample every road at this spacing, away from junction boxes.
        const STEP_M: f32 = 20.0;
        const JUNCTION_CLEAR_M: f32 = 12.0;
        // A spawned car starts from rest: seconds lost against cruising the whole way.
        const STARTUP_S: f32 = 3.0;
        let meets = self.racer_meets(target, rv);
        let factor = self.cfg.pursuit.respond_speed_factor;
        let max_m = rs.spawn_max_m + crate::graph::len(rv) * rs.spawn_lead_s;
        let mut cands: Vec<(f32, [f32; 2], [f32; 2])> = Vec::new();
        for (k, seg) in self.graph.segments.iter().enumerate() {
            let sid = crate::graph::SegmentId(k as u32);
            let a = self.graph.node(seg.a).pos;
            let b = self.graph.node(seg.b).pos;
            let len = seg.length_m;
            // Keep out of junction boxes; bend nodes (curved roads come as many short
            // segments) need no clearance.
            let clear = |n: crate::graph::NodeId| {
                if self.graph.node(n).junction {
                    JUNCTION_CLEAR_M
                } else {
                    0.0
                }
            };
            let (lo, hi) = (clear(seg.a), len - clear(seg.b));
            if hi - lo < 2.0 {
                continue;
            }
            // Cheap reject: the whole segment is outside the window.
            let (da, db) = (to_racer[seg.a.0 as usize], to_racer[seg.b.0 as usize]);
            if da.min(db) > max_m || da.min(db) + len < rs.spawn_min_m {
                continue;
            }
            // Every STEP_M from `lo`, or the middle of a short span.
            let n = ((hi - lo) / STEP_M).floor() as usize + 1;
            for j in 0..n {
                let off = if hi - lo < STEP_M {
                    (lo + hi) * 0.5
                } else {
                    lo + j as f32 * STEP_M
                };
                let t = off / len;
                let centre = lerp2(a, b, t);
                // Drive toward a (leaving b) or toward b (leaving a), whichever is shorter
                // and legal.
                let via_a = if self.graph.can_travel(sid, seg.b) {
                    off + da
                } else {
                    f32::INFINITY
                };
                let via_b = if self.graph.can_travel(sid, seg.a) {
                    (len - off) + db
                } else {
                    f32::INFINITY
                };
                let (d_road, dir) = if via_a <= via_b {
                    (via_a, normalize(sub(a, b)))
                } else {
                    (via_b, normalize(sub(b, a)))
                };
                if !(rs.spawn_min_m..=max_m).contains(&d_road) {
                    continue;
                }
                let pos = add(centre, scale([dir[1], -dir[0]], LANE_OFFSET_M));
                // Earliest meeting on the racer's path that this spot reaches first, s.
                let v_unit = (seg.speed_limit_mps * factor).max(5.0);
                let meet = meets.iter().find_map(|(t_racer, d)| {
                    let drive = |from: crate::graph::NodeId, part: f32| {
                        if self.graph.can_travel(sid, self.graph.other_end(sid, from)) {
                            part + d[from.0 as usize]
                        } else {
                            f32::INFINITY
                        }
                    };
                    let m = drive(seg.a, off).min(drive(seg.b, len - off));
                    (STARTUP_S + m / v_unit <= *t_racer).then_some(*t_racer)
                });
                let score = match meet {
                    Some(t) => t,
                    None => {
                        let ahead = if moving {
                            crate::graph::dot(normalize(sub(pos, target)), normalize(rv))
                        } else {
                            0.0
                        };
                        // Ahead of the racer counts as up to 40 % closer.
                        1000.0 + d_road * (1.0 - 0.4 * ahead.max(0.0))
                    }
                };
                cands.push((score, pos, dir));
            }
        }
        // Best first; ties by position (deterministic).
        cands.sort_by(|x, y| {
            x.0.total_cmp(&y.0)
                .then(x.1[0].total_cmp(&y.1[0]))
                .then(x.1[1].total_cmp(&y.1[1]))
        });
        cands
            .into_iter()
            .find(|(_, pos, _)| {
                !self.in_racer_view(world, *pos)
                    && crate::avoid::spawn_clear(*pos, &self.obstacles, &self.cfg.pursuit.avoid)
            })
            .map(|(_, pos, dir)| (pos, dir))
    }

    /// Reinforcement priority order: captain's priority target first, then bounty, then id.
    fn pursued_by_priority(&self) -> Vec<EntityId> {
        let mut v: Vec<(bool, f32, EntityId)> = self
            .racers
            .values()
            .filter(|r| r.pursued())
            .map(|r| {
                (
                    self.captain.priority == Some(r.entity),
                    r.bounty.bounty(&self.cfg.heat),
                    r.entity,
                )
            })
            .collect();
        v.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.total_cmp(&a.1)).then(a.2.cmp(&b.2)));
        v.into_iter().map(|x| x.2).collect()
    }

    /// Beyond this straight-line distance from `racer` a unit on it is "far" (row 64), m.
    fn far_m(&self, racer: EntityId) -> f32 {
        let rc = &self.cfg.presence.reinforcement;
        let level = self.racers.get(&racer).map_or(1, |r| r.level.table_index());
        rc.backup_radius_m[level] + rc.far_margin_m
    }

    /// Units on `racer` within [`PoliceSystem::far_m`] of it (what counts toward the level's
    /// units: a unit far across town doesn't stop a closer one being sent).
    pub(crate) fn units_near(&self, racer: EntityId) -> u32 {
        let Some(p) = self.racers.get(&racer).map(|r| xy(r.pos)) else {
            return 0;
        };
        let far = self.far_m(racer);
        self.units
            .iter()
            .filter(|u| {
                u.kind == UnitKind::Car
                    && !u.wrecked
                    && dist(u.body.pos, p) <= far
                    && matches!(u.assignment, Assignment::Pursue { target } | Assignment::Intercept { target, .. } if target == racer)
            })
            .count() as u32
    }

    /// Stand down AI units that have been on `racer` for `far_release_s` and are still farther
    /// than `far_m` from it (they despawn once out of sight).
    fn release_far_units(&mut self, racer: EntityId) {
        let Some(p) = self.racers.get(&racer).map(|r| xy(r.pos)) else {
            return;
        };
        let (far, wait) = (
            self.far_m(racer),
            self.cfg.presence.reinforcement.far_release_s,
        );
        let now = self.now;
        for i in 0..self.units.len() {
            let u = &self.units[i];
            if u.kind == UnitKind::Car
                && u.player.is_none()
                && matches!(u.assignment, Assignment::Intercept { target, .. } if target == racer)
                && now.since(u.assigned) > wait
                && dist(u.body.pos, p) > far
            {
                self.assign(i, Assignment::Idle);
            }
        }
    }

    pub(crate) fn units_on(&self, racer: EntityId) -> u32 {
        self.units
            .iter()
            .filter(|u| {
                u.kind == UnitKind::Car
                    && !u.wrecked
                    && matches!(u.assignment, Assignment::Pursue { target } | Assignment::Intercept { target, .. } if target == racer)
            })
            .count() as u32
    }

    fn maintain_reinforcements(&mut self, world: &dyn WorldQuery) {
        let now = self.now;
        let racers = &self.racers;
        self.reinforcements
            .retain(|r| racers.get(&r.racer).is_some_and(|x| x.pursued()));
        for racer in self.pursued_by_priority() {
            let rec = &self.racers[&racer];
            let want = self.cfg.presence.reinforcement.units_per_level[rec.level.table_index()];
            self.release_far_units(racer);
            let mut have = self.units_near(racer);
            if have < want {
                have += self.call_backup(racer, want - have);
            }
            let rec = &self.racers[&racer];
            let pending = self
                .reinforcements
                .iter()
                .filter(|r| r.racer == racer)
                .count() as u32;
            if have + pending < want {
                let range = self.cfg.presence.reinforcement.response_s.get(self.phase);
                let base = self.rng.range_f32(range.min, range.max);
                let prio = if self.captain.priority == Some(racer) {
                    1.0
                } else {
                    rec.bounty.priority(&self.cfg.heat)
                };
                let t =
                    base * (1.0 - prio * self.cfg.presence.reinforcement.priority_max_reduction);
                self.reinforcements.push(ReinforcementRequest {
                    racer,
                    requested: now,
                    due: now.plus(t),
                });
                let n = have + pending + 1;
                self.radio(
                    RadioCue::new(
                        RadioTier::Tactical,
                        None,
                        format!(
                            "Dispatch: copy, sending unit {n} of {want} on racer #{}.",
                            racer.0
                        ),
                    )
                    .grouped(
                        None,
                        format!("backup:{}", racer.0),
                        format!(
                            "Dispatch: copy, {{count}} more units en route to racer #{}.",
                            racer.0
                        ),
                    ),
                );
            }
        }
        // Commit due reinforcements.
        let mut k = 0;
        while k < self.reinforcements.len() {
            let req = self.reinforcements[k];
            if now < req.due {
                k += 1;
                continue;
            }
            self.reinforcements.remove(k);
            let Some(target) = self.racers.get(&req.racer).and_then(|r| r.last_known) else {
                continue;
            };
            // Backup already got there first (units joined from nearby): not needed.
            let want = self.cfg.presence.reinforcement.units_per_level
                [self.racers[&req.racer].level.table_index()];
            if self.units_near(req.racer) >= want {
                continue;
            }
            let i = match self.nearest_available(target, 700.0) {
                Some(i) => i,
                None => {
                    if self.ground_units_alive() >= self.unit_cap() {
                        continue;
                    }
                    self.rebuild_obstacles();
                    let Some((pos, heading)) = self
                        .response_spawn_point(world, req.racer)
                        .or_else(|| self.reinforcement_spawn_point(world, target))
                    else {
                        continue;
                    };
                    let i = self.spawn_unit(UnitKind::Car, UnitRole::Reinforcement, pos, heading);
                    self.record_spawn(world, i);
                    i
                }
            };
            self.stats
                .reinforcement_delays_s
                .push(now.since(req.requested));
            self.assign(
                i,
                Assignment::Intercept {
                    target: req.racer,
                    point: target,
                },
            );
        }
    }

    /// Backup for a pursuit (owner playtest row 64): up to `n` idle or patrolling units within
    /// the level's `backup_radius_m` of legal driving from the racer (only from the racer's
    /// own district below `backup_all_districts_level`) join at once, nearest first, and drive
    /// to its predicted position (`Intercept`). Returns how many joined.
    fn call_backup(&mut self, racer: EntityId, n: u32) -> u32 {
        let rc = &self.cfg.presence.reinforcement;
        let rec = &self.racers[&racer];
        let Some(target) = rec.last_known else {
            return 0;
        };
        let level = rec.level;
        let radius = rc.backup_radius_m[level.table_index()];
        let all = level.number() >= rc.backup_all_districts_level;
        let district = self.graph.district_at(target);
        // Cheap pre-filter by straight-line distance before the road distances.
        let cands: Vec<usize> = (0..self.units.len())
            .filter(|&i| {
                let u = &self.units[i];
                u.available()
                    && u.player.is_none()
                    && matches!(
                        u.role,
                        UnitRole::Pool(_) | UnitRole::Reinforcement | UnitRole::Responder
                    )
                    && dist(u.body.pos, target) <= radius
                    && (all || self.graph.district_at(u.body.pos) == district)
            })
            .collect();
        if cands.is_empty() {
            return 0;
        }
        let to_racer = self.graph.distances_to(target);
        let mut ranked: Vec<(f32, usize)> = cands
            .into_iter()
            .filter_map(|i| {
                let u = &self.units[i];
                let node = self.graph.nearest_node(u.body.pos)?;
                let d = to_racer[node.0 as usize] + dist(u.body.pos, self.graph.node(node).pos);
                (d <= radius).then_some((d, i))
            })
            .collect();
        ranked.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut joined = 0;
        for &(_, i) in ranked.iter().take(n as usize) {
            // A unit answering a call about this racer is already on its way.
            self.assign(
                i,
                Assignment::Intercept {
                    target: racer,
                    point: target,
                },
            );
            self.stats.backup_joins += 1;
            joined += 1;
        }
        joined
    }

    /// The closest node at least `spawn_min_distance_m` from `target` without line of sight.
    fn reinforcement_spawn_point(
        &self,
        world: &dyn WorldQuery,
        target: [f32; 2],
    ) -> Option<([f32; 2], [f32; 2])> {
        let min_d = self.cfg.presence.reinforcement.spawn_min_distance_m;
        let mut best: Option<(f32, usize)> = None;
        for (i, n) in self.graph.nodes.iter().enumerate() {
            let d = dist(n.pos, target);
            if d < min_d
                || self.sees(world, n.pos, target)
                || self.in_racer_view(world, n.pos)
                || !crate::avoid::spawn_clear(n.pos, &self.obstacles, &self.cfg.pursuit.avoid)
            {
                continue;
            }
            if best.is_none_or(|b| d < b.0) {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| {
            let p = self.graph.nodes[i].pos;
            (p, normalize(sub(target, p)))
        })
    }

    /// Request a helicopter on `racer` (AI captain and captain command share this).
    pub(crate) fn request_helo(&mut self, racer: EntityId) -> Result<(), CaptainError> {
        let now = self.now;
        if now < self.captain.helo_ready {
            return Err(CaptainError::Cooldown {
                remaining_s: self.captain.helo_ready.since(now),
            });
        }
        let active = self
            .units
            .iter()
            .filter(|u| u.kind == UnitKind::Helicopter && !u.wrecked)
            .count()
            + self.helo_requests.len();
        if active as u32 >= self.cfg.presence.helicopter.count.get(self.phase) {
            return Err(CaptainError::Budget);
        }
        self.captain.helo_ready = now.plus(self.cfg.tools.helo_request_cooldown_s);
        if self.rng.chance(
            self.cfg
                .presence
                .helicopter
                .unavailable_chance
                .get(self.phase),
        ) {
            self.stats.helo_unavailable += 1;
            self.radio(RadioCue::new(
                RadioTier::Tactical,
                None,
                "Dispatch: air unit unavailable.".to_string(),
            ));
            return Err(CaptainError::Unavailable);
        }
        let launch = self.cfg.presence.helicopter.launch_s.get(self.phase);
        self.helo_requests.push(HeloRequest {
            racer,
            launch: now.plus(launch),
        });
        self.radio(RadioCue::new(
            RadioTier::Tactical,
            None,
            format!("Dispatch: air unit requested, ETA {launch:.0} s."),
        ));
        Ok(())
    }

    fn maintain_helicopters(&mut self) {
        let now = self.now;
        let min_level = self.cfg.presence.helicopter.min_level;
        if !self.captain.human {
            for racer in self.pursued_by_priority() {
                let lvl = self.racers[&racer].level.number();
                let tracked = self.units.iter().any(|u| {
                    u.kind == UnitKind::Helicopter && u.assignment.target() == Some(racer)
                });
                let requested = self.helo_requests.iter().any(|h| h.racer == racer);
                if lvl >= min_level && !tracked && !requested && now >= self.captain.helo_ready {
                    let _ = self.request_helo(racer);
                }
            }
        }
        let racers = &self.racers;
        self.helo_requests
            .retain(|h| racers.get(&h.racer).is_some_and(|r| r.pursued()));
        let mut k = 0;
        while k < self.helo_requests.len() {
            let req = self.helo_requests[k];
            if now < req.launch {
                k += 1;
                continue;
            }
            self.helo_requests.remove(k);
            let pad = self.helipad();
            let i = self.spawn_unit(UnitKind::Helicopter, UnitRole::Helicopter, pad, [0.0, 1.0]);
            self.stats.helo_launches += 1;
            let id = self.units[i].id;
            self.pending.push(PoliceEvent::HeloLaunched {
                unit: id,
                target: req.racer,
            });
            self.assign(i, Assignment::HeloTrack { target: req.racer });
        }
        // Helicopters return when the pursuit ends or they run out of time on station.
        let max_track = self.cfg.tools.helicopter.max_track_s;
        for i in 0..self.units.len() {
            let u = &self.units[i];
            if let Assignment::HeloTrack { target } = u.assignment {
                let done = !self.racers.get(&target).is_some_and(|r| r.pursued());
                if done || now.since(u.assigned) > max_track {
                    self.assign(i, Assignment::OutOfService);
                }
            }
        }
    }

    /// Helicopter base: the city's helipad POI, else the middle of the map.
    fn helipad(&self) -> [f32; 2] {
        if let Some(h) = self.graph.pois_of(PoiKind::Helipad).next() {
            return h.pos;
        }
        let mut min = [f32::INFINITY; 2];
        let mut max = [f32::NEG_INFINITY; 2];
        for n in &self.graph.nodes {
            for k in 0..2 {
                min[k] = min[k].min(n.pos[k]);
                max[k] = max[k].max(n.pos[k]);
            }
        }
        lerp2(min, max, 0.5)
    }

    // -----------------------------------------------------------------------------------------
    // Tools
    // -----------------------------------------------------------------------------------------

    fn racer_motion(&self, racer: EntityId) -> Option<([f32; 2], [f32; 2])> {
        let r = self.racers.get(&racer)?;
        let p = r.last_known?;
        Some((p, [r.vel[0], r.vel[1]]))
    }

    /// Place a roadblock ahead of `racer` (optionally on a specific road).
    pub(crate) fn place_roadblock(
        &mut self,
        racer: EntityId,
        road: Option<crate::captain::RoadEdgeId>,
    ) -> Result<ToolId, CaptainError> {
        let now = self.now;
        let rec = self.racers.get(&racer).ok_or(CaptainError::UnknownTarget)?;
        if !rec.pursued() {
            return Err(CaptainError::NotPursued);
        }
        let level = rec.level.number();
        let rb = &self.cfg.tools.roadblock;
        let need = self.cfg.presence.unlocks.roadblock_level;
        if level < need {
            return Err(CaptainError::Locked { need_level: need });
        }
        if now < self.captain.roadblock_ready {
            return Err(CaptainError::Cooldown {
                remaining_s: self.captain.roadblock_ready.since(now),
            });
        }
        if self.roadblocks.len() as u32 >= rb.max_active {
            return Err(CaptainError::Budget);
        }
        let (p, v) = self.racer_motion(racer).ok_or(CaptainError::NotPursued)?;
        let rules = SiteRules {
            min_ahead_m: rb.min_ahead_m,
            max_ahead_m: rb.max_ahead_m,
            min_sight_m: rb.min_sight_m,
            highway_ok: level <= rb.highway_max_level,
        };
        let site =
            plan_site(&self.graph, p, v, &rules, road.map(|r| r.0)).map_err(CaptainError::Site)?;
        let n = rb.units[0] + self.rng.below(rb.units[1] - rb.units[0] + 1);
        let barriers = roadblock_barriers(
            &site,
            rb.barrier_half_extents_m,
            rb.barrier_mass_kg,
            ROAD_HALF_WIDTH_M,
        );
        let lifetime = rb.lifetime_s;
        let cooldown = rb.cooldown_s.get(self.phase);
        let id = ToolId(self.next_tool);
        self.next_tool += 1;
        let mut units = Vec::new();
        for (pos, heading) in roadblock_unit_spots(&site, n, ROAD_HALF_WIDTH_M) {
            let i = self.spawn_unit(UnitKind::Car, UnitRole::Tool, pos, heading);
            units.push(self.units[i].id);
            self.assign(
                i,
                Assignment::RoadblockAt {
                    segment: site.segment,
                    tool: id,
                },
            );
        }
        self.roadblocks.push(Roadblock {
            id,
            racer,
            site,
            units,
            barriers,
            expires: now.plus(lifetime),
        });
        self.captain.roadblock_ready = now.plus(cooldown);
        self.stats.roadblocks += 1;
        self.stats.tool_sight_m.push(site.sight_m);
        self.pending.push(PoliceEvent::RoadblockPlaced {
            tool: id,
            racer,
            pos: [site.pos[0], site.pos[1], 0.0],
            sight_m: site.sight_m,
        });
        Ok(id)
    }

    /// Place a spike strip ahead of `racer` (optionally on a specific road).
    pub(crate) fn place_spikes(
        &mut self,
        racer: EntityId,
        road: Option<crate::captain::RoadEdgeId>,
    ) -> Result<ToolId, CaptainError> {
        let now = self.now;
        let rec = self.racers.get(&racer).ok_or(CaptainError::UnknownTarget)?;
        if !rec.pursued() {
            return Err(CaptainError::NotPursued);
        }
        let level = rec.level.number();
        let sc = &self.cfg.tools.spikes;
        let need = self.cfg.presence.unlocks.spikes_level;
        if level < need {
            return Err(CaptainError::Locked { need_level: need });
        }
        if now < self.captain.spikes_ready {
            return Err(CaptainError::Cooldown {
                remaining_s: self.captain.spikes_ready.since(now),
            });
        }
        if self.spikes.len() as u32 >= sc.max_active {
            return Err(CaptainError::Budget);
        }
        let (p, v) = self.racer_motion(racer).ok_or(CaptainError::NotPursued)?;
        let rules = SiteRules {
            min_ahead_m: sc.min_ahead_m,
            max_ahead_m: sc.max_ahead_m,
            min_sight_m: sc.min_sight_m,
            highway_ok: true,
        };
        let site =
            plan_site(&self.graph, p, v, &rules, road.map(|r| r.0)).map_err(CaptainError::Site)?;
        let (length, lifetime, cooldown) =
            (sc.length_m, sc.lifetime_s, sc.cooldown_s.get(self.phase));
        let id = ToolId(self.next_tool);
        self.next_tool += 1;
        let side = add(
            site.pos,
            scale(perp_left(site.dir), ROAD_HALF_WIDTH_M + 2.5),
        );
        let i = self.spawn_unit(UnitKind::Car, UnitRole::Tool, side, site.dir);
        let uid = self.units[i].id;
        self.assign(
            i,
            Assignment::SpikeAt {
                segment: site.segment,
                tool: id,
            },
        );
        let strip = SpikeStrip::across(id, racer, site, length, Some(uid), now.plus(lifetime));
        self.pending.push(PoliceEvent::SpikesPlaced {
            tool: id,
            racer,
            a: [strip.a[0], strip.a[1], 0.0],
            b: [strip.b[0], strip.b[1], 0.0],
            sight_m: site.sight_m,
        });
        self.spikes.push(strip);
        self.captain.spikes_ready = now.plus(cooldown);
        self.stats.spikes += 1;
        self.stats.tool_sight_m.push(site.sight_m);
        Ok(id)
    }

    fn ai_captain_tools(&mut self) {
        for racer in self.pursued_by_priority() {
            let r = &self.racers[&racer];
            if r.status != PursuitStatus::Pursuit {
                continue;
            }
            let lvl = r.level.number();
            if lvl >= self.cfg.presence.unlocks.spikes_level
                && self.now >= self.captain.spikes_ready
            {
                // No valid site yet (blind corners ahead) is normal; try again next think.
                let _ = self.place_spikes(racer, None);
            }
            if lvl >= self.cfg.presence.unlocks.roadblock_level
                && self.now >= self.captain.roadblock_ready
            {
                let _ = self.place_roadblock(racer, None);
            }
        }
    }

    pub(crate) fn remove_tools_for(&mut self, racer: Option<EntityId>) {
        let now = self.now;
        let mut removed = Vec::new();
        self.roadblocks.retain(|rb| {
            let keep = now < rb.expires && racer != Some(rb.racer);
            if !keep {
                removed.push((rb.id, rb.units.clone()));
            }
            keep
        });
        self.spikes.retain(|s| {
            let keep = now < s.expires && racer != Some(s.racer);
            if !keep {
                removed.push((s.id, s.unit.into_iter().collect()));
            }
            keep
        });
        for (tool, units) in removed {
            self.pending.push(PoliceEvent::ToolRemoved { tool });
            for uid in units {
                if let Some(i) = self.unit_index(uid) {
                    self.assign(i, Assignment::OutOfService);
                }
            }
        }
    }

    fn expire_tools(&mut self) {
        self.remove_tools_for(None);
    }

    // -----------------------------------------------------------------------------------------
    // Unit health and clean-up
    // -----------------------------------------------------------------------------------------

    fn check_unit_health(&mut self) {
        for i in 0..self.units.len() {
            let u = &self.units[i];
            if u.kind != UnitKind::Car || u.wrecked {
                continue;
            }
            if u.health <= self.cfg.pursuit.unit.total_loss_health.get(self.setup.mode) {
                let cause = if u.racer_contact {
                    DisableCause::RacerContact
                } else {
                    DisableCause::SelfCrash
                };
                let contact_racer = if u.racer_contact {
                    u.assignment.target()
                } else {
                    None
                };
                let id = u.id;
                if u.engaged {
                    match cause {
                        DisableCause::RacerContact => self.stats.contact_wrecks += 1,
                        _ => self.stats.self_wrecks += 1,
                    }
                }
                if let Some(racer) = contact_racer {
                    let heat = self.cfg.heat.clone();
                    if let Some(r) = self.racers.get_mut(&racer) {
                        r.bounty.cops_disabled += 1;
                        r.add_heat(heat.heat.cop_disabled_heat, &heat);
                    }
                }
                self.units[i].wrecked = true;
                self.pending
                    .push(PoliceEvent::UnitDisabled { unit: id, cause });
                self.assign(i, Assignment::OutOfService);
                continue;
            }
            if u.assignment == Assignment::OutOfService || u.player.is_some() {
                continue;
            }
            let damaged = u.health
                < self
                    .cfg
                    .pursuit
                    .tactics
                    .disengage_health
                    .get(self.setup.mode);
            let bent = u.must_pull_over(&self.cfg.pursuit);
            if damaged || bent {
                let id = u.id;
                let cause = if bent {
                    DisableCause::BentWheel
                } else {
                    DisableCause::Damaged
                };
                self.stats.pulled_over += 1;
                self.pending
                    .push(PoliceEvent::UnitDisabled { unit: id, cause });
                self.assign(i, Assignment::OutOfService);
            }
        }
    }

    fn cleanup_units(&mut self, world: &dyn WorldQuery) {
        let now = self.now;
        let mut i = 0;
        while i < self.units.len() {
            let u = &self.units[i];
            let age = now.since(u.assigned);
            // Ground units only vanish out of every racer's sight (no pop-out).
            let remove = match (&u.assignment, u.role) {
                (_, UnitRole::Player) => false,
                (Assignment::OutOfService, UnitRole::Helicopter) => age > 15.0,
                (Assignment::OutOfService, _) => {
                    age > if u.wrecked { 60.0 } else { 20.0 } && self.may_vanish(world, i)
                }
                // Done with their call / chase: drive off (plan_units) and vanish once hidden.
                (
                    Assignment::Idle | Assignment::Patrol { .. },
                    UnitRole::Reinforcement | UnitRole::Responder,
                ) => self.may_vanish(world, i),
                _ => false,
            };
            if remove {
                // A wrecked pool unit is replaced by maintain_pool.
                self.despawn(i);
            } else {
                i += 1;
            }
        }
    }

    /// End every pursuit for `racer` and release its units (shared by rules and captain).
    pub(crate) fn release_units(&mut self, racer: EntityId) {
        for i in 0..self.units.len() {
            let u = &self.units[i];
            if u.assignment.target() != Some(racer) || u.player.is_some() {
                continue;
            }
            let a = match u.kind {
                UnitKind::Helicopter => Assignment::OutOfService,
                UnitKind::Car => Assignment::Idle,
            };
            self.assign(i, a);
        }
        self.reinforcements.retain(|r| r.racer != racer);
        self.helo_requests.retain(|h| h.racer != racer);
        self.remove_tools_for(Some(racer));
    }
}
