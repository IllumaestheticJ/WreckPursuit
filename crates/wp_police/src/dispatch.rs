//! Presence & dispatch: the patrol pool per district and phase, incidents (civilian reports
//! and crash scenes), reinforcements with phase-dependent response times, the helicopter, and
//! the AI captain that places tools when no human captain is seated.
//!
//! Every assignment change goes through [`PoliceSystem::assign`], which emits a radio line.

use crate::radio::{RadioCue, RadioTier};
use serde::{Deserialize, Serialize};
use wp_contracts::{EntityId, ViolationKind, WorldQuery};

use crate::captain::CaptainError;
use crate::chatter::round_m;
use crate::events::{
    DisableCause, IncidentId, PoliceEvent, PursuitOutcome, ToolId, UnitId, UnitKind,
};
use crate::graph::{
    add, dist, dot, len, lerp2, normalize, perp_left, scale, sub, xy, District, NodeId, PoiKind,
    RoadClass,
};
use crate::heat::{PursuitStatus, WantedLevel};
use crate::time::SimTime;
use crate::tools::{
    plan_site, plan_site_where, roadblock_barriers, roadblock_unit_spots, Roadblock, SiteRules,
    SpikeStrip, ToolSite,
};
use crate::unit::{Assignment, DriveCommand, Tactic, Track, Unit, UnitRole, LANE_OFFSET_M};
use crate::PoliceSystem;

/// At most one new ambush (a unit sent to wait ahead) per racer this often, s.
pub const AMBUSH_EVERY_S: f32 = 45.0;

/// A racer faster than this also "sees" from points on its predicted road ahead (no spawn
/// that it would see within [`ROUTE_VIEW_S`]), m/s.
pub const ROUTE_VIEW_MIN_MPS: f32 = 5.0;
/// ... this many seconds of its travel ahead, s.
pub const ROUTE_VIEW_S: f32 = 3.0;
/// ... sampled this far apart, m.
pub const ROUTE_VIEW_STEP_M: f32 = 15.0;

/// Road half width used for tool layout, m.
pub const ROAD_HALF_WIDTH_M: f32 = 5.0;

/// Within this distance of a racer's road (ahead or behind it), a one-way road running the
/// other way is the far carriageway of a divided road (row 140), m.
pub const FAR_CARRIAGEWAY_M: f32 = 30.0;
/// How far ahead along its predicted road and back along the road behind it a racer's
/// corridor reaches (row 140), m.
pub const CORRIDOR_AHEAD_M: f32 = 900.0;
/// See [`CORRIDOR_AHEAD_M`].
pub const CORRIDOR_BEHIND_M: f32 = 500.0;
/// Corridor sample spacing, m.
const CORRIDOR_STEP_M: f32 = 10.0;

/// A racer's road as travelled, ahead and behind: sample points with the travel direction
/// there (row 140, [`PoliceSystem::on_far_carriageway`]).
pub(crate) type Corridor = Vec<([f32; 2], [f32; 2])>;

/// Sample the leg `a → b` into `out` every [`CORRIDOR_STEP_M`] (with `b`).
fn sample_leg(a: [f32; 2], b: [f32; 2], out: &mut Corridor) {
    let d = sub(b, a);
    let l = crate::graph::len(d);
    if l < 0.1 {
        return;
    }
    let dir = scale(d, 1.0 / l);
    let n = (l / CORRIDOR_STEP_M).ceil() as usize;
    for k in 1..=n {
        out.push((lerp2(a, b, k as f32 / n as f32), dir));
    }
}

/// A car never appears closer than this to a parked car or cone (row 75), m.
pub const SPAWN_PROP_CLEAR_M: f32 = 3.0;

/// Row 86 audit: an active unit standing still longer than this is logged, s.
pub const ACTIVE_STOP_S: f32 = 5.0;

/// How far beyond the road edge a spike-strip car parks, m.
pub const SPIKE_CAR_SHOULDER_M: f32 = 2.5;

/// A spike strip is lifted once its unit is further than this from where it parked (shoved
/// away, driven off, reset elsewhere), so a strip never lies on the road without a police car
/// beside it (PR #54), m.
pub const SPIKE_UNIT_LEASH_M: f32 = 15.0;

/// An unmanned spike strip (its car's spot was in a racer's view, row 131 / 158) is lifted
/// once its racer is this far past it along the road, m.
pub const SPIKE_PASSED_M: f32 = 25.0;

/// A roadblock or spike-strip car not spawned yet. A roadblock car waits while its spot is in a
/// racer's view (row 131) and appears once the spot is hidden, while its tool lasts. A spike
/// car is decided on the next think: spot hidden, it appears beside its strip; in view, it is
/// dropped and the strip stays unmanned (PR #54 port, lead 2026-10-23).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PendingToolCar {
    /// Tool it belongs to.
    pub tool: ToolId,
    /// Parking spot, m.
    pub pos: [f32; 2],
    /// Heading (unit vector).
    pub heading: [f32; 2],
    /// Its assignment once spawned.
    pub assignment: Assignment,
}

/// Where a tool goes across the road at a site (row 133): the traffic flow there and the
/// lateral span the tool may cover, as offsets to the right of `flow` from the site, m. On a
/// one-way carriageway (a divided highway's half) that is the carriageway only, never across
/// the median.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ToolSpan {
    /// Traffic flow direction at the site (unit vector).
    pub flow: [f32; 2],
    /// Left edge (negative: left of the site), m.
    pub inner: f32,
    /// Right edge, m.
    pub outer: f32,
    /// The site is on a one-way carriageway.
    pub oneway: bool,
}

impl ToolSpan {
    /// Unit vector to the right of the flow.
    pub fn right(&self) -> [f32; 2] {
        [self.flow[1], -self.flow[0]]
    }

    /// The site point moved `off` m to the right of the flow.
    pub fn at(&self, site: [f32; 2], off: f32) -> [f32; 2] {
        add(site, scale(self.right(), off))
    }
}

/// A reckless-driving report this close to an open call about another racer joins that call
/// (a pack of street racers is one call), m.
const CALL_MERGE_M: f32 = 200.0;

/// World contacts closer together than this are one static-geometry episode, s.
const STATIC_EPISODE_S: f32 = 1.0;

/// Spots tried per pool spawn before giving up until the next think step.
const SPAWN_TRIES: usize = 8;

/// Ambushing reinforcements wait at least this far (in the racer's travel time) ahead of it, s.
const AMBUSH_MIN_LEAD_S: f32 = 6.0;
/// They must reach the wait point at least this long before the racer does, s.
const AMBUSH_SETTLE_S: f32 = 3.0;
/// Farthest wait point along the racer's predicted path, m.
const AMBUSH_MAX_AHEAD_M: f32 = 900.0;
/// The wait point is this far into the racer's road past the junction the unit turns in at, m.
pub(crate) const AMBUSH_INTO_LEG_M: f32 = 25.0;
/// Spawn nodes are at most this far from the wait point, m.
const AMBUSH_SPAWN_RADIUS_M: f32 = 400.0;
/// Spawn nodes tried per wait point, closest first.
const AMBUSH_SPAWN_CANDIDATES: usize = 6;
/// Cruise time along an ambush route is scaled by this (acceleration, traffic), -.
const AMBUSH_PACE_MARGIN: f32 = 1.3;
/// Time added per junction on an ambush route (slowing for the turn), s.
const AMBUSH_JUNCTION_S: f32 = 3.0;

/// A spot to wait for a racer at (see `PoliceSystem::ambush_waits`).
struct AmbushWait {
    /// Junction the unit turns in at.
    junction: NodeId,
    /// Wait point, [`AMBUSH_INTO_LEG_M`] past `junction` on the racer's road, m.
    wait: [f32; 2],
    /// When the racer gets there at its current speed, s from now.
    racer_eta: f32,
}

/// A reinforcement spawn that waits for the racer ahead of it (see `PoliceSystem::ambush_plan`).
struct AmbushPlan {
    /// Spawn position (right-hand lane), m.
    pos: [f32; 2],
    /// Spawn heading, toward the route's first node.
    heading: [f32; 2],
    /// Where to wait, on the racer's predicted path, m.
    wait: [f32; 2],
    /// Centreline route from the spawn node to `wait`.
    track: Track,
}

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

/// Radio bookkeeping of one tracking helicopter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct HeloSight {
    /// Line of sight to the target at the latest think.
    pub(crate) los: bool,
    /// Visual as radioed: set on the first sighting, cleared after
    /// `lost_visual_after_s` without line of sight.
    pub(crate) visual: bool,
    /// Ever had visual on this track ("eyes on" vs "reacquired").
    pub(crate) acquired: bool,
    /// Last think with line of sight.
    pub(crate) last_los: SimTime,
    /// Next position report.
    pub(crate) next_report: SimTime,
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

    /// Radar ([`crate::config::RadarConfig`]): every racer a ground unit can see (this think
    /// step's witness) that is over the limit where it drives by more than the margin commits
    /// a witnessed speeding violation, so the blatant rule and the witness's pursuit apply
    /// at once, whichever way the racer is facing and however often `wp_world` reports.
    /// Only racers not wanted yet are clocked (the owner's gap: speeding past a unit never
    /// made them wanted); a wanted racer a unit sees is recognised within about a second and
    /// pursued anyway, and pursuit heat covers a pursued one. Held racers are never clocked.
    pub(crate) fn clock_speeders(&mut self, world: &dyn WorldQuery, tdt: f32) {
        let radar = self.cfg.heat.heat.radar.clone();
        let speeding = crate::heat::violation_index(&ViolationKind::Speeding { over_mps: 0.0 });
        let mut clocked: Vec<(EntityId, f32, [f32; 2])> = Vec::new();
        let ids: Vec<EntityId> = self.racers.keys().copied().collect();
        for id in ids {
            let r = &self.racers[&id];
            let watched = r.level == WantedLevel::None
                && !self.held.contains(&id)
                && self.obs.get(&id).is_some_and(|o| o.witness.is_some());
            // The higher of the world's limit (lane under the car; at a junction that can be
            // the slower cross street) and the police road graph's nearest road. Off the lanes
            // (over the kerb, on parking) the world has none: the road's, within half a street.
            let road = self
                .graph
                .nearest_on_road(xy(r.pos))
                .filter(|rp| rp.dist_m <= RADAR_ROAD_M)
                .map(|rp| self.graph.segment(rp.segment).speed_limit_mps);
            let limit = match (world.speed_limit_at(r.pos), road) {
                (Some(a), Some(b)) => Some(a.max(b)),
                (a, b) => a.or(b),
            };
            let speed = (r.vel[0] * r.vel[0] + r.vel[1] * r.vel[1]).sqrt();
            let over = limit.map(|l| (speed - l, l));
            let fast = over.is_some_and(|(o, l)| o > radar.margin_mps.max(radar.ratio * l));
            let held = self.radar_over.entry(id).or_insert(0.0);
            *held = if watched && fast { *held + tdt } else { 0.0 };
            // At wp_world's cadence: only when no speeding counted for `repeat_s` (a clock a
            // second made a race field passing one patrol wanted at once, Hollins), and only
            // after `hold_s` of speeding in sight (wp_world's hold).
            let due =
                r.last_violation[speeding].is_none_or(|t| self.now.since(t) >= radar.repeat_s);
            if *held >= radar.hold_s && due {
                if let Some((o, _)) = over {
                    clocked.push((id, o, xy(r.pos)));
                }
            }
        }
        for (racer, over_mps, at) in clocked {
            self.stats.radar_clocks += 1;
            self.on_violation(world, racer, &ViolationKind::Speeding { over_mps }, at);
        }
    }

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
        // Row 80: the unit that saw it acts on it. A violation it witnessed that makes the
        // racer wanted starts the pursuit at once, day or night (recognition_time_s is only
        // for a wanted car a unit runs into later).
        if let Some(unit) = obs.witness {
            if after > WantedLevel::None && !self.racers[&racer].pursued() {
                self.stats.witnessed_pursuits += 1;
                let desc = self.racer_desc(racer);
                self.radio(RadioCue::new(
                    RadioTier::Escalation,
                    Some(unit),
                    format!(
                        "Unit {}, I just watched {desc} {}, I'm on it.",
                        unit.0,
                        offence_text(kind)
                    ),
                ));
                self.start_pursuit(racer, unit);
                return;
            }
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
        let sight = self.cfg.heat.heat.sight.clone();
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
                && crate::pursuit::in_view(u.body.heading, u.body.pos, rp, d, &sight)
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
            RadioTier::Escalation,
            Some(unit),
            format!(
                "Unit {}, {desc} just blew past me at {:.0} over, I'm on it.",
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

    /// Row 86: a unit on a pursuit or intercept held at a standstill behind civilian traffic
    /// (a kinematic agent or a non-racer car, not another unit, not a red light; under 3 m/s)
    /// for
    /// [`crate::stuck::JAM_REROUTE_S`] blocks the road segment it is on for itself and re-plans
    /// (turning round if the way on is the jam). Measured on the police bench: chasers stood
    /// 10-20 s in rain-hour queues while the racer drove off.
    fn reroute_jammed(&mut self) {
        use crate::stuck;
        let tdt = 1.0 / self.cfg.pursuit.think_hz;
        let now = self.now;
        for i in 0..self.units.len() {
            let u = &self.units[i];
            let active = matches!(
                u.assignment,
                Assignment::Pursue { .. } | Assignment::Intercept { .. }
            );
            let red = u.avoid.junction.is_some_and(|j| {
                j.light == Some(crate::avoid::Light::Stop) && j.s - u.track_s < 30.0
            });
            let by_traffic = match u.avoid.blocked_by {
                Some(crate::avoid::BlockedBy::Traffic) => true,
                Some(crate::avoid::BlockedBy::Vehicle(e)) => !self.racers.contains_key(&e),
                _ => false,
            };
            let held = active
                && u.player.is_none()
                && !u.wrecked
                && u.kind == UnitKind::Car
                && by_traffic
                && !red
                && u.avoid.limit_mps < 3.0
                && u.body.speed.abs() < 3.0;
            let segment = held
                .then(|| self.graph.nearest_on_road(u.body.pos).map(|r| r.segment))
                .flatten();
            let st = &mut self.units[i].unstick;
            if !held {
                // Creeping a car length or a blink of a free lane is still the jam.
                st.jam_s = (st.jam_s - 2.0 * tdt).max(0.0);
                continue;
            }
            st.jam_s += tdt;
            if st.jam_s < stuck::JAM_REROUTE_S {
                continue;
            }
            st.jam_s = 0.0;
            if let Some(s) = segment {
                st.blocked.retain(|(b, until)| *b != s && now < *until);
                st.blocked.push((s, now.plus(stuck::BLOCK_SEGMENT_S)));
                st.reroute = true;
                self.stats.jam_reroutes += 1;
            }
        }
    }

    /// Think-rate part of un-sticking: re-plan routes around blocked segments, and despawn
    /// units that gave up once nobody can see them (their call is dispatched again; a patrol
    /// is refilled out of sight by `maintain_pool`).
    pub(crate) fn maintain_stuck(&mut self, world: &dyn WorldQuery) {
        self.reroute_jammed();
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
            // Turned round MAX_TURNS times on one spot: stop trying. A unit on a call hands it
            // back (Idle: siren off, so traffic stops yielding to it) and, like any unit that
            // gave up, despawns once nobody can see it.
            if self.units[i].unstick.turn_loop {
                self.units[i].unstick.turn_loop = false;
                self.units[i].unstick.give_up = true;
                self.stats.turn_loops += 1;
                if let Assignment::Respond { incident } = self.units[i].assignment {
                    let id = self.units[i].id;
                    self.end_response(
                        id,
                        crate::ResponseOutcome::Stuck(crate::stuck::StuckCause::NoContact),
                    );
                    for inc in self.incidents.iter_mut() {
                        if inc.id == incident {
                            inc.unit = None;
                        }
                    }
                    self.assign(i, Assignment::Idle);
                }
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
                let u = &self.units[i];
                let near = crate::stuck::DEADLOCK_NEAR_M * crate::stuck::DEADLOCK_NEAR_M;
                let deadlocked = u.unstick.stand_s >= crate::stuck::DEADLOCK_VANISH_S
                    && self.racers.values().any(|r| {
                        let d = sub(xy(r.pos), u.body.pos);
                        d[0] * d[0] + d[1] * d[1] < near
                    });
                if deadlocked {
                    self.stats.deadlock_despawns += 1;
                }
                if deadlocked || self.may_vanish(world, i) {
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

    /// Reset in place (owner row 78): an un-wrecked AI unit that has stood stuck long enough
    /// (see [`crate::stuck::RESET_ACTIVE_S`]) snaps to the nearest clear spot in its lane
    /// within [`crate::stuck::RESET_RADIUS_M`] along its route, keeps its damage and drives on
    /// as a ghost for a second. A racer that can see it gets
    /// [`crate::stuck::RESET_VIEW_DELAY_S`] more first. No clear spot: it despawns if nobody
    /// can see it (patrols are refilled out of sight), else tries again next think step.
    /// Wrecked units stay down (backup replaces them).
    pub(crate) fn maintain_resets(&mut self, world: &dyn WorldQuery) {
        use crate::stuck;
        let now = self.now;
        let mut i = 0;
        while i < self.units.len() {
            let Some(active) = self.reset_wanted(i) else {
                self.units[i].unstick.reset_due = None;
                i += 1;
                continue;
            };
            let due = match self.units[i].unstick.reset_due {
                Some(t) => t,
                None => {
                    let in_view = self.in_racer_view(world, self.units[i].body.pos);
                    let t = if in_view {
                        now.plus(stuck::RESET_VIEW_DELAY_S)
                    } else {
                        now
                    };
                    self.units[i].unstick.reset_due = Some(t);
                    t
                }
            };
            if now < due {
                i += 1;
                continue;
            }
            self.rebuild_obstacles();
            let turn = self.turn_round_spot(i);
            let spot = match turn {
                Some((pos, heading)) => Some((0.0, pos, heading)),
                None => self.reset_spot(i),
            };
            // Its own spot is the clear one: nothing hems it in, so it is waiting for something
            // (traffic rules, a call) rather than stuck; only a unit the stuck detector flagged
            // (wedged on a wall, beached) is re-placed where it stands.
            // (A unit on its roof is righted where it lies: row 75.)
            let pointless = self.units[i].flipped_since.is_none()
                && spot.is_some_and(|(_, p, _)| {
                    dist(p, self.units[i].body.pos) < 1.0 && self.units[i].unstick.first.is_none()
                });
            if pointless {
                self.units[i].unstick.reset_due = Some(now.plus(stuck::RESET_RETRY_S));
                i += 1;
                continue;
            }
            match spot {
                Some((s, pos, heading)) => {
                    let in_view = due > now || self.in_racer_view(world, self.units[i].body.pos);
                    let u = &mut self.units[i];
                    let from = u.body.pos;
                    u.body.pos = pos;
                    u.body.heading = heading;
                    u.body.speed = 0.0;
                    u.track_s = s;
                    let blocked = std::mem::take(&mut u.unstick.blocked);
                    let turn_hold = u.unstick.turn_hold;
                    u.unstick = crate::stuck::UnstickState::default();
                    u.unstick.blocked = blocked;
                    u.unstick.turn_hold = turn_hold;
                    u.avoid = crate::avoid::AvoidState::default();
                    let (unit, entity) = (u.id, u.entity);
                    if turn.is_some() {
                        // Turned round: patrol on somewhere the new way.
                        let d = match self.units[i].role {
                            UnitRole::Pool(d) => Some(d),
                            _ => None,
                        };
                        if let Some(next) = self.random_node_in(d) {
                            self.units[i].assignment = Assignment::Patrol { dest: next };
                            let track = self.route_track(i, next, None);
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
                    }
                    self.resets.push(crate::UnitReset {
                        unit,
                        entity,
                        pos,
                        heading,
                        ghost_s: stuck::RESET_GHOST_S,
                    });
                    self.stats.resets.push(crate::ResetRecord {
                        unit,
                        at: now,
                        from,
                        to: pos,
                        active,
                        in_view,
                    });
                }
                None => {
                    let u = &self.units[i];
                    let near = stuck::DEADLOCK_NEAR_M * stuck::DEADLOCK_NEAR_M;
                    let wedged = u.unstick.stand_s >= stuck::RESET_VANISH_S
                        && self.racers.values().any(|r| {
                            let d = sub(xy(r.pos), u.body.pos);
                            d[0] * d[0] + d[1] * d[1] < near
                        });
                    if wedged {
                        self.stats.deadlock_despawns += 1;
                    }
                    if wedged || self.may_vanish(world, i) {
                        let id = self.units[i].id;
                        self.end_response(
                            id,
                            crate::ResponseOutcome::Stuck(crate::stuck::StuckCause::NoContact),
                        );
                        for inc in self.incidents.iter_mut() {
                            if inc.unit == Some(id) {
                                inc.unit = None;
                            }
                        }
                        self.despawn(i);
                        continue;
                    }
                }
            }
            i += 1;
        }
    }

    /// Whether unit `i` should be reset in place now (`Some(on a pursuit or search)`): an AI
    /// ground unit, not wrecked or out of service, that has stood within
    /// [`crate::stuck::STAND_M`] long enough while it wants to drive, and is not waiting at a
    /// red light, holding a block or standing over a stopped racer it is after.
    fn reset_wanted(&self, i: usize) -> Option<bool> {
        use crate::stuck;
        let u = &self.units[i];
        // Row 75: on its side or roof (not a wreck): back on its wheels, whatever it was doing.
        if u.kind == UnitKind::Car
            && u.player.is_none()
            && !u.wrecked
            && u.flipped_since
                .is_some_and(|t| self.now.since(t) >= stuck::FLIPPED_RESET_S)
        {
            return Some(matches!(
                u.assignment,
                Assignment::Pursue { .. }
                    | Assignment::Intercept { .. }
                    | Assignment::Respond { .. }
            ));
        }
        if u.kind != UnitKind::Car
            || u.player.is_some()
            || u.wrecked
            || u.assignment == Assignment::OutOfService
            || u.role == UnitRole::Tool
            || matches!(u.tactic, Tactic::RollingBlock)
            || !u.cmd.track.is_drivable()
            || u.cmd.speed_mps.min(u.planned_speed) <= stuck::WANT_MPS
            || u.unstick.reversing()
        {
            return None;
        }
        let red = u
            .avoid
            .junction
            .is_some_and(|j| j.light == Some(crate::avoid::Light::Stop) && j.s - u.track_s < 80.0);
        // Queued at a junction (giving way, waiting to turn, cross traffic streaming past):
        // waiting, not stuck, unless the stuck detector says otherwise.
        let at_junction = u
            .avoid
            .junction
            .is_some_and(|j| j.s - u.track_s < stuck::RESET_JUNCTION_M);
        let deadlock = self.deadlocked(i);
        if deadlock && u.unstick.stand_s >= stuck::RESET_DEADLOCK_S {
            return Some(false);
        }
        if red || (at_junction && u.unstick.first.is_none()) {
            return None;
        }
        let target = u.assignment.target();
        let active = matches!(
            u.assignment,
            Assignment::Pursue { .. } | Assignment::Intercept { .. }
        );
        // Standing over a stopped racer it is after (inside the capture radius): an arrest,
        // not a jam. Farther out and held up (Hollins hairpin: 20 m off behind other cars, for
        // minutes) it is a jam like any other.
        let arrest_m = self.cfg.heat.capture.unit_radius_m;
        if let Some(r) = target.and_then(|t| self.racers.get(&t)) {
            let slow = r.vel[0] * r.vel[0] + r.vel[1] * r.vel[1] < 4.0;
            if slow && dist(xy(r.pos), u.body.pos) < arrest_m {
                return None;
            }
        }
        let need = if active {
            stuck::RESET_ACTIVE_S
        } else if u.avoid.blocked_by == Some(crate::avoid::BlockedBy::Traffic) {
            stuck::RESET_TRAFFIC_S
        } else {
            stuck::RESET_IDLE_S
        };
        (u.unstick.stand_s >= need).then_some(active)
    }

    /// Whether unit `i` stands in a two-unit deadlock it should give way in: nose to nose
    /// with another standing unit (main's ramp terminals) or each waiting for the other. The
    /// one less busy (not chasing; else the higher id) gives way.
    fn deadlocked(&self, i: usize) -> bool {
        use crate::stuck;
        let u = &self.units[i];
        match u.avoid.blocked_by {
            Some(crate::avoid::BlockedBy::Unit(other)) => {
                self.unit_index(other).is_some_and(|j| {
                    let o = &self.units[j];
                    let o_active = matches!(
                        o.assignment,
                        Assignment::Pursue { .. } | Assignment::Intercept { .. }
                    );
                    let me_active = matches!(
                        u.assignment,
                        Assignment::Pursue { .. } | Assignment::Intercept { .. }
                    );
                    // Nose to nose (or it waits on this one): the other's passing attempt
                    // flips its own `blocked_by` on and off, so facing decides.
                    let facing = o.body.heading[0] * u.body.heading[0]
                        + o.body.heading[1] * u.body.heading[1]
                        < -0.5;
                    let waits_on_me =
                        o.avoid.blocked_by == Some(crate::avoid::BlockedBy::Unit(u.id));
                    (facing || waits_on_me)
                        && o.body.speed.abs() < stuck::SLOW_MPS
                        && (o_active, o.id) > (me_active, u.id)
                })
            }
            Some(crate::avoid::BlockedBy::Vehicle(e)) => self.facing_racer(i, e),
            _ => false,
        }
    }

    /// Whether patrolling unit `i` stands nose to nose with racer `e`, stopped (Hollins
    /// hairpin on main's physics: a patrol met the racers' queue head-on in their lane and
    /// neither side could move for 20 s).
    fn facing_racer(&self, i: usize, e: EntityId) -> bool {
        let u = &self.units[i];
        if !matches!(u.assignment, Assignment::Patrol { .. }) || !self.racers.contains_key(&e) {
            return false;
        }
        self.obstacles.iter().any(|o| {
            o.entity == Some(e)
                && o.speed() < crate::stuck::SLOW_MPS
                && o.fwd[0] * u.body.heading[0] + o.fwd[1] * u.body.heading[1] < -0.5
        })
    }

    /// A turn-round spot for patrolling unit `i` facing a stopped racer (see
    /// [`Self::facing_racer`]) or giving way in a unit deadlock: the other lane, facing the other way, up to
    /// [`crate::stuck::RESET_RADIUS_M`] back from where it stands, on the road and clear.
    fn turn_round_spot(&self, i: usize) -> Option<([f32; 2], [f32; 2])> {
        use crate::stuck;
        let u = &self.units[i];
        // Facing a stopped racer, or giving way in a nose-to-nose unit deadlock (where the
        // spot past the other unit can be taken by a third one queued behind it).
        let stuck_facing = match u.avoid.blocked_by {
            Some(crate::avoid::BlockedBy::Vehicle(e)) => self.facing_racer(i, e),
            Some(crate::avoid::BlockedBy::Unit(_)) => {
                matches!(u.assignment, Assignment::Patrol { .. }) && self.deadlocked(i)
            }
            _ => false,
        };
        if !stuck_facing {
            return None;
        }
        let h = u.body.heading;
        let back = scale(h, -1.0);
        let left = [-h[1], h[0]];
        let others: Vec<crate::avoid::Obstacle> = self
            .obstacles
            .iter()
            .filter(|o| o.unit != Some(u.id) && (o.entity.is_none() || o.entity != u.entity))
            .copied()
            .collect();
        let steps = (stuck::RESET_RADIUS_M / stuck::RESET_STEP_M) as i32;
        (0..=steps).find_map(|k| {
            let pos = add(
                add(u.body.pos, scale(left, 2.0 * LANE_OFFSET_M)),
                scale(back, k as f32 * stuck::RESET_STEP_M),
            );
            let on_road = self
                .graph
                .nearest_on_road(pos)
                .is_some_and(|r| r.dist_m < 2.0 * LANE_OFFSET_M);
            (on_road && reset_clear(pos, &others)).then_some((pos, back))
        })
    }

    /// The nearest clear spot in unit `i`'s lane along its route within
    /// [`crate::stuck::RESET_RADIUS_M`]: (route position, point, heading). Uses the
    /// obstacles as last rebuilt.
    fn reset_spot(&self, i: usize) -> Option<(f32, [f32; 2], [f32; 2])> {
        use crate::stuck;
        let u = &self.units[i];
        // Giving way in a deadlock: only past the other unit (a spot behind still faces it).
        let ahead_only = self.deadlocked(i);
        let tr = &u.cmd.track;
        let len = tr.length();
        let (id, entity) = (u.id, u.entity);
        let others: Vec<crate::avoid::Obstacle> = self
            .obstacles
            .iter()
            .filter(|o| o.unit != Some(id) && (o.entity.is_none() || o.entity != entity))
            .copied()
            .collect();
        let steps = (stuck::RESET_RADIUS_M / stuck::RESET_STEP_M) as i32;
        for k in 0..=2 * steps {
            // 0, +1, -1, +2, -2, ...: ahead first.
            let n = (k + 1) / 2;
            let d = if k % 2 == 1 { n } else { -n } as f32 * stuck::RESET_STEP_M;
            let s = u.track_s + d;
            if s < 0.0 || s > len || (ahead_only && d <= 0.0) {
                continue;
            }
            let pos = crate::unit::lane_point(tr, s, u.cmd.lane_offset_m);
            let a = tr.point_at((s - 2.0).max(0.0));
            let b = tr.point_at((s + 2.0).min(len));
            let dir = sub(b, a);
            // Within reach of where it stands, whatever its route progress says.
            if crate::graph::len(dir) < 0.1 || dist(pos, u.body.pos) > stuck::RESET_RADIUS_M {
                continue;
            }
            if reset_clear(pos, &others) {
                return Some((s, pos, normalize(dir)));
            }
        }
        None
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
        let building = self.walls.nearest(p).is_some_and(|d| d <= 1.5);
        self.stats.static_hits.push(crate::StaticHit {
            unit: u.id,
            at: now,
            pos: p,
            kind,
            speed_mps: u.body.speed,
            pursuing,
            building,
            turning: u.unstick.swinging() || u.unstick.reversing(),
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
            let rv = self
                .prev_vel
                .get(&racer)
                .copied()
                .unwrap_or(self.racers[&racer].vel);
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
        // Row 144: shouted, an impact line (tier 3) that still breaks in on lower traffic.
        self.radio(
            RadioCue::new(
                RadioTier::Impact,
                Some(unit),
                format!(
                    "Unit {}, I've been hit! I'm hit! {desc} rammed my unit, assault on an officer!",
                    unit.0
                ),
            )
            .prio(5)
            .breaks_in(),
        );
        self.chatter_note_impact_line(unit);
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
                "Dispatch: collision with injuries {}, need a unit.",
                self.place_phrase(pos, [0.0, 0.0])
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
        self.watch_spawns(world);
        self.helo_reports();
        self.clock_speeders(world, tdt);
        self.update_racer_rules(tdt);
        self.check_unit_health();
        self.maintain_stuck(world);
        self.maintain_resets(world);
        self.maintain_pool(world);
        self.dispatch_incidents(world);
        self.update_responses();
        self.maintain_reinforcements(world);
        self.maintain_helicopters();
        self.race_think(world);
        if !self.captain.human {
            self.ai_captain_tools();
        }
        self.expire_tools();
        self.maintain_tool_cars(world);
        self.plan_units(world, tdt);
        self.audit_active_stops(tdt);
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
                // +Z up: a positive cross product turns left.
                let cross = into[0] * out[1] - into[1] * out[0];
                let turn_dir = match (turn, cross > 0.0) {
                    (false, _) => 0,
                    (true, true) => -1,
                    (true, false) => 1,
                };
                crate::avoid::JunctionAhead {
                    s,
                    light,
                    turn,
                    turn_dir,
                }
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
        // Booking a racer it captured: no new chase yet (`pursuit::ARREST_BUSY_S`).
        if matches!(a, Assignment::Pursue { .. } | Assignment::Intercept { .. })
            && self.booking(self.units[i].id)
        {
            return;
        }
        // A unit leaving its spike post (wrecked, pulled over, released) takes the strip with
        // it (PR #54).
        if let Assignment::SpikeAt { tool, .. } = self.units[i].assignment {
            self.lift_spikes(tool);
        }
        // A unit's first patrol is not "resuming" anything: no line (13 units "resuming
        // patrol" at the start held the channel for ≈ 25 s).
        let first_patrol =
            matches!(a, Assignment::Patrol { .. } | Assignment::Idle) && !self.units[i].had_job;
        let text = self.radio_text(i, &a);
        let deploy = matches!(
            a,
            Assignment::Pursue { .. }
                | Assignment::Intercept { .. }
                | Assignment::Respond { .. }
                | Assignment::RoadblockAt { .. }
                | Assignment::SpikeAt { .. }
                | Assignment::HeloTrack { .. }
        );
        let now = self.now;
        {
            let u = &mut self.units[i];
            if !matches!(a, Assignment::Patrol { .. } | Assignment::Idle) {
                u.had_job = true;
            }
            u.assignment = a;
            u.assigned = now;
            u.quiet = false;
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
        match text {
            Some(cue) if !first_patrol => self.radio(cue),
            None if deploy => self.radio_deploy_suppressed(),
            _ => {}
        }
    }

    /// Spoken description of a racer ("racer #3 northbound on Harbor Boulevard", "racer #3
    /// downtown" on unnamed roads; see [`PoliceSystem::place_phrase`]).
    pub(crate) fn racer_desc(&self, racer: EntityId) -> String {
        match self.racers.get(&racer) {
            Some(r) => format!(
                "racer #{} {}",
                racer.0,
                self.place_phrase(xy(r.pos), [r.vel[0], r.vel[1]])
            ),
            None => format!("racer #{}", racer.0),
        }
    }

    /// The radio line for unit `i` taking assignment `a`: tier and collapse rule per
    /// assignment (`data/police/radio.toml` paces them, see [`crate::radio`]). `None`: a
    /// deployment line that is not spoken (row 144): units joining a pursuit, responding to
    /// a known call, tool cars arriving (the tool was announced when it was planned) and
    /// intercepts that are not ahead of the racer. The low-heat "units responding" summary
    /// and the tier 1 tool / intercept / helicopter lines stand in for them.
    fn radio_text(&self, i: usize, a: &Assignment) -> Option<RadioCue> {
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
        Some(match a {
            Assignment::Idle => line(
                RadioTier::Routine,
                format!("{callsign}, clear and available."),
                "clear".into(),
                "{units}, clear and available.",
            ),
            Assignment::Patrol { .. } => line(
                RadioTier::Routine,
                format!(
                    "{callsign} resuming patrol {}.",
                    self.place_phrase(u.body.pos, [0.0, 0.0])
                ),
                "patrol".into(),
                "{units} resuming patrol.",
            ),
            Assignment::Respond { incident } => {
                // A new call goes out with the first unit sent: one line, not two. Later units
                // on the same call are deployment chatter (row 144): not spoken.
                let inc = self
                    .incidents
                    .iter()
                    .find(|x| x.id == *incident && !x.announced)?;
                let tier = match inc.kind {
                    IncidentKind::Report { .. } => RadioTier::Investigation,
                    IncidentKind::Crash => RadioTier::Routine,
                };
                let text = format!(
                    "Dispatch: {}, {}. {callsign} responding.",
                    call_text(inc),
                    self.place_phrase(inc.pos, [0.0, 0.0])
                );
                line(
                    tier,
                    text,
                    format!("respond:{}", incident.0),
                    "Dispatch: {units} responding.",
                )
            }
            Assignment::Pursue { .. } => return None,
            Assignment::Intercept { target, point, .. } => {
                let r = self.racers.get(target)?;
                let searching = r.status == crate::heat::PursuitStatus::Searching;
                if searching {
                    // Sent to the last known position (search points): say where (tier 4,
                    // "checking the area").
                    let eta = self
                        .eta_phrase(i, *point)
                        .map(|e| format!(", {e}"))
                        .unwrap_or_default();
                    line(
                        RadioTier::Investigation,
                        format!(
                            "{callsign}, copy, searching {}{eta}.",
                            self.place_phrase(*point, [0.0, 0.0])
                        ),
                        format!("search:{}", target.0),
                        &format!("{{units}} searching for racer #{}.", target.0),
                    )
                } else {
                    // Tier 1 only when it matters to the driver: L2+, and the intercept point
                    // is ahead of the racer ("a unit ahead of you").
                    let (rp, rv) = (xy(r.pos), [r.vel[0], r.vel[1]]);
                    let to = sub(*point, rp);
                    let ahead = len(rv) >= 2.0 && dot(normalize(rv), to) > 0.0;
                    if r.level.number() < 2 || !ahead {
                        return None;
                    }
                    let where_ = self.place_phrase(*point, [0.0, 0.0]);
                    let m = round_m(len(to));
                    line(
                        RadioTier::Actionable,
                        format!(
                            "{callsign} moving to intercept racer #{} ahead {where_}, {m} metres.",
                            target.0
                        ),
                        format!("intercept:{}", target.0),
                        &format!(
                            "{{units}} moving to intercept racer #{} ahead {where_}, {m} metres.",
                            target.0
                        ),
                    )
                    .supersedes(format!("intercept:{}", target.0))
                }
            }
            // Announced when planned (`chatter`, from `RoadblockPlaced` / `SpikesPlaced`).
            Assignment::RoadblockAt { .. } | Assignment::SpikeAt { .. } => return None,
            // Announced at the request ("air unit launching") and when it has eyes on.
            Assignment::HeloTrack { .. } => return None,
            Assignment::OutOfService => line(
                RadioTier::Routine,
                format!("{callsign} out of service."),
                "out_of_service".into(),
                "{units} out of service.",
            ),
        })
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
            let hidden = !self.warmed_up || !self.spawn_in_view(world, pos);
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
        self.spot_clear(pos)
    }

    /// Whether a car may be placed at `pos` with the current obstacle list: clear of moving
    /// and standing vehicles ([`crate::avoid::spawn_clear`]) and of parked cars and cones
    /// (row 75: a unit spawned on a parked car was thrown upside down).
    pub(crate) fn spot_clear(&self, pos: [f32; 2]) -> bool {
        crate::avoid::spawn_clear(pos, &self.obstacles, &self.cfg.pursuit.avoid)
            && self.props.clear_of(pos, SPAWN_PROP_CLEAR_M)
    }

    pub(crate) fn despawn(&mut self, i: usize) {
        if let Assignment::SpikeAt { tool, .. } = self.units[i].assignment {
            self.lift_spikes(tool);
        }
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
                || self.booking(u.id)
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

    /// [`Self::nearest_available`] for backup on `racer` at `p` (row 140): a unit on the far
    /// carriageway of the racer's divided road is scored by its legal drive (round by the next
    /// interchange or turning point), not by the straight line across the median.
    fn nearest_responder(&self, racer: EntityId, p: [f32; 2], max_m: f32) -> Option<usize> {
        let corridor = self.racer_corridor(racer);
        let mut to_p: Option<Vec<f32>> = None;
        let mut best: Option<(f32, usize)> = None;
        for (i, u) in self.units.iter().enumerate() {
            if !u.available()
                || self.booking(u.id)
                || u.player.is_some()
                || !matches!(
                    u.role,
                    UnitRole::Pool(_) | UnitRole::Reinforcement | UnitRole::Responder
                )
            {
                continue;
            }
            let pos = u.body.pos;
            let mut d = dist(pos, p);
            if d <= max_m && self.far_side(&corridor, pos) {
                let to_p = to_p.get_or_insert_with(|| self.graph.distances_to(p));
                d = self
                    .start_node(pos, u.body.heading)
                    .map_or(f32::INFINITY, |n| {
                        dist(pos, self.graph.node(n).pos) + to_p[n.0 as usize]
                    });
            }
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
                let text = format!(
                    "Dispatch: {}, {}.",
                    call_text(&inc),
                    self.place_phrase(inc.pos, [0.0, 0.0])
                );
                self.radio(RadioCue::new(RadioTier::Investigation, None, text));
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
    /// its current velocity, or from points on its predicted road up to [`ROUTE_VIEW_S`] of its
    /// travel ahead (row 131), to a car body at `p` (taken at the racer's height). Units never appear or vanish where this is true.
    pub(crate) fn in_racer_view(&self, world: &dyn WorldQuery, p: [f32; 2]) -> bool {
        self.in_racer_view_opt(Some(world), p, false)
    }

    /// Whether a unit may not appear at `p`: [`Self::in_racer_view`] plus the racers' road
    /// ahead for [`ROUTE_VIEW_S`] (row 131: no pop-in just round the corner ahead). Spawns
    /// only; despawns and resets keep the plain check.
    pub(crate) fn spawn_in_view(&self, world: &dyn WorldQuery, p: [f32; 2]) -> bool {
        self.in_racer_view_opt(Some(world), p, true)
    }

    /// [`Self::in_racer_view`] (`route`: also from the road ahead, see
    /// [`Self::spawn_in_view`]); without a world (captain tool placement) only the road
    /// graph's own sight lines (city blocks) are checked.
    pub(crate) fn in_racer_view_opt(
        &self,
        world: Option<&dyn WorldQuery>,
        p: [f32; 2],
        route: bool,
    ) -> bool {
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
            // Where it will be over the next few seconds along its road (row 131: a car must
            // not pop in just round the corner ahead of a fast racer): a few points on its
            // predicted path, the look-ahead growing with speed.
            let speed = crate::graph::len(v);
            if route && speed > ROUTE_VIEW_MIN_MPS {
                let horizon = speed * ROUTE_VIEW_S;
                let path = self.graph.predict_path(rp, v, horizon);
                let pts = Track::new(path.points());
                let len = pts.length().min(horizon);
                let mut d = ROUTE_VIEW_STEP_M;
                while d <= len + 0.5 * ROUTE_VIEW_STEP_M {
                    let q = pts.point_at(d.min(len));
                    views.push([q[0], q[1], z + EYE_UP_M]);
                    d += ROUTE_VIEW_STEP_M;
                }
            }
            // Police only know the racer's velocity, not which way it faces (sliding, turning,
            // stopped): the camera may be on any side.
            for d in [[1.0, 0.0], [-1.0, 0.0], [0.0, 1.0], [0.0, -1.0]] {
                let c = add(rp, scale(d, CAM_BACK_M));
                views.push([c[0], c[1], z + CAM_UP_M]);
            }
            views.iter().any(|eye| {
                self.graph.line_of_sight([eye[0], eye[1]], p)
                    && world.is_none_or(|w| w.line_of_sight(*eye, body))
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
        let far_carriageway = self
            .racers
            .values()
            .any(|r| r.pursued() && self.far_side(&self.racer_corridor(r.entity), p));
        let u = &self.units[i];
        self.stats.spawns.push(crate::SpawnRecord {
            unit: u.id,
            role: u.role,
            at: self.now,
            pos: p,
            nearest_racer_m: nearest,
            in_sight,
            seen_after_s: None,
            far_carriageway,
        });
        if !in_sight {
            self.spawn_watch.push(self.stats.spawns.len() - 1);
        }
    }

    /// Pop-in audit (row 131): note when a racer first sees the spot a unit appeared at,
    /// within [`ROUTE_VIEW_S`] of it appearing. Statistics only.
    fn watch_spawns(&mut self, world: &dyn WorldQuery) {
        let now = self.now;
        let watch = std::mem::take(&mut self.spawn_watch);
        for k in watch {
            let sp = self.stats.spawns[k];
            let age = now.since(sp.at);
            if age > ROUTE_VIEW_S {
                continue;
            }
            let seen = self
                .racers
                .values()
                .any(|r| self.sees_at(world, xy(r.pos), r.pos[2], sp.pos, r.pos[2]));
            if seen {
                self.stats.spawns[k].seen_after_s = Some(age);
            } else {
                self.spawn_watch.push(k);
            }
        }
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
        // Lost (row 84): around the last known position and the way it was heading, not
        // where the hidden racer really is.
        let (target, rv) = match (rec.status, rec.last_known) {
            (PursuitStatus::Searching, Some(lk)) => (lk, rec.last_known_vel),
            _ => (xy(rec.pos), [rec.vel[0], rec.vel[1]]),
        };
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
        let mut cands: Vec<(f32, [f32; 2], [f32; 2], bool)> = Vec::new();
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
                cands.push((score, pos, dir, seg.oneway));
            }
        }
        // Best first; ties by position (deterministic).
        cands.sort_by(|x, y| {
            x.0.total_cmp(&y.0)
                .then(x.1[0].total_cmp(&y.1[0]))
                .then(x.1[1].total_cmp(&y.1[1]))
        });
        // Row 140: never on the far carriageway of the racer's divided road.
        let corridor = self.racer_corridor(racer);
        cands
            .into_iter()
            .find(|(_, pos, dir, oneway)| {
                !(*oneway && Self::on_far_carriageway(&corridor, *pos, *dir))
                    && !self.spawn_in_view(world, *pos)
                    && self.spot_clear(*pos)
            })
            .map(|(_, pos, dir, _)| (pos, dir))
    }

    /// `racer`'s corridor (row 140): its predicted road [`CORRIDOR_AHEAD_M`] ahead and the
    /// road it came along [`CORRIDOR_BEHIND_M`] back (legal, straightest way in at each
    /// junction), as `(point, travel direction)` samples. Empty for an unknown racer or one
    /// off the road network.
    pub(crate) fn racer_corridor(&self, racer: EntityId) -> Corridor {
        let mut out = Corridor::new();
        let Some(r) = self.racers.get(&racer) else {
            return out;
        };
        let rp = r.last_known.unwrap_or(xy(r.pos));
        let v = [r.vel[0], r.vel[1]];
        let g = &self.graph;
        let Some(on) = g.nearest_on_road(rp) else {
            return out;
        };
        let seg = g.segment(on.segment);
        // Which way it travels along its segment: the one way on a one-way road, else as
        // it moves (towards b when stopped).
        let to_b =
            seg.oneway || crate::graph::dot(sub(g.node(seg.b).pos, g.node(seg.a).pos), v) >= 0.0;
        let (from, to) = if to_b { (seg.a, seg.b) } else { (seg.b, seg.a) };
        let travel = normalize(sub(g.node(to).pos, g.node(from).pos));
        // Ahead.
        let ahead_v = if crate::graph::len(v) > 1.0 {
            v
        } else {
            travel
        };
        let path = g.predict_path(on.point, ahead_v, CORRIDOR_AHEAD_M);
        let pts = path.points();
        out.push((on.point, travel));
        for w in pts.windows(2) {
            sample_leg(w[0], w[1], &mut out);
        }
        // Behind: back to the node it came from, then on back the straightest legal way in.
        let mut back = Corridor::new();
        sample_leg(g.node(from).pos, on.point, &mut back);
        let mut node = from;
        let mut dir = travel;
        let mut walked = dist(g.node(from).pos, on.point);
        while walked < CORRIDOR_BEHIND_M {
            let here = g.node(node).pos;
            let best = g
                .incoming(node)
                .iter()
                .map(|&(_, prev)| {
                    let d = normalize(sub(here, g.node(prev).pos));
                    (crate::graph::dot(d, dir), prev, d)
                })
                .filter(|x| x.0 > 0.0)
                .max_by(|x, y| x.0.total_cmp(&y.0).then(y.1 .0.cmp(&x.1 .0)));
            let Some((_, prev, d)) = best else {
                break;
            };
            let p = g.node(prev).pos;
            sample_leg(p, here, &mut back);
            walked += dist(p, here);
            node = prev;
            dir = d;
        }
        out.extend(back);
        out
    }

    /// Whether `p`, on a one-way road running along `flow`, is on the far carriageway of a
    /// divided road from the racer whose [`Self::racer_corridor`] is `corridor` (row 140):
    /// within [`FAR_CARRIAGEWAY_M`] of the racer's road and running the other way. A pursuit
    /// unit never spawns there: it can neither drive back the wrong way nor cross the median.
    pub(crate) fn on_far_carriageway(corridor: &Corridor, p: [f32; 2], flow: [f32; 2]) -> bool {
        corridor
            .iter()
            .any(|(q, d)| dist(*q, p) < FAR_CARRIAGEWAY_M && crate::graph::dot(flow, *d) < -0.5)
    }

    /// The one-way flow at `p` when it lies on a one-way road (within 8 m of its line).
    pub(crate) fn oneway_flow_at(&self, p: [f32; 2]) -> Option<[f32; 2]> {
        let rp = self.graph.nearest_on_road(p)?;
        let seg = self.graph.segment(rp.segment);
        (seg.oneway && rp.dist_m < 8.0).then(|| self.graph.dir_from(rp.segment, seg.a))
    }

    /// Whether `a` and `b` are on one-way roads running opposite ways (the two carriageways
    /// of a divided road, when close): neither can reach the other across the median.
    pub(crate) fn opposed_carriageways(&self, a: [f32; 2], b: [f32; 2]) -> bool {
        match (self.oneway_flow_at(a), self.oneway_flow_at(b)) {
            (Some(fa), Some(fb)) => crate::graph::dot(fa, fb) < -0.5,
            _ => false,
        }
    }

    /// Whether the racer's corridor puts a spot at `p` on its far carriageway
    /// ([`Self::on_far_carriageway`]); `p` off one-way roads never is.
    pub(crate) fn far_side(&self, corridor: &Corridor, p: [f32; 2]) -> bool {
        self.oneway_flow_at(p)
            .is_some_and(|flow| Self::on_far_carriageway(corridor, p, flow))
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

    /// Ground units wanted on `racer`: `units_per_level` for its level, plus
    /// `search_extra_units` while it is lost (row 84).
    pub(crate) fn wanted_units(&self, racer: EntityId) -> u32 {
        let Some(rec) = self.racers.get(&racer) else {
            return 0;
        };
        let rc = &self.cfg.presence.reinforcement;
        let li = rec.level.table_index();
        rc.units_per_level[li]
            + if rec.status == PursuitStatus::Searching {
                rc.search_extra_units[li]
            } else {
                0
            }
    }

    /// Where the police work a pursuit on `racer` from: the racer while they see it, its last
    /// known position while it is lost (row 84: units sent to search there were stood down as
    /// "far" from where the hidden racer had really got to).
    pub(crate) fn search_centre(&self, racer: EntityId) -> Option<[f32; 2]> {
        let r = self.racers.get(&racer)?;
        Some(match (r.status, r.last_known) {
            (PursuitStatus::Searching, Some(lk)) => lk,
            _ => xy(r.pos),
        })
    }

    /// Units on `racer` within [`PoliceSystem::far_m`] of it (what counts toward the level's
    /// units: a unit far across town doesn't stop a closer one being sent).
    pub(crate) fn units_near(&self, racer: EntityId) -> u32 {
        let Some(p) = self.search_centre(racer) else {
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
        let Some(p) = self.search_centre(racer) else {
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
            let want = self.wanted_units(racer);
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
                // Row 144: "copy, sending unit n of m" is a deployment line, not actionable
                // for the driver: not spoken (the units show up as interceptors ahead or on
                // his bumper instead).
                self.radio_deploy_suppressed();
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
            let want = self.wanted_units(req.racer);
            if self.units_near(req.racer) >= want {
                continue;
            }
            let vel = self.ambush_vel(req.racer);
            let i = match self.nearest_responder(req.racer, target, 700.0) {
                Some(i) => {
                    // A unit already on the road also sets up ahead if it can get there in time.
                    if let Some((wait, track)) = self.ambush_route(i, target, vel) {
                        self.stats
                            .reinforcement_delays_s
                            .push(now.since(req.requested));
                        self.note_first_backup(req.racer);
                        self.start_ambush(i, req.racer, wait, track);
                        continue;
                    }
                    i
                }
                None => {
                    if self.ground_units_alive() >= self.unit_cap() {
                        continue;
                    }
                    if let Some(plan) = self.ambush_plan(world, req.racer, target, vel) {
                        let i = self.spawn_unit(
                            UnitKind::Car,
                            UnitRole::Reinforcement,
                            plan.pos,
                            plan.heading,
                        );
                        self.stats
                            .reinforcement_delays_s
                            .push(now.since(req.requested));
                        self.note_first_backup(req.racer);
                        // Starts driving right away (before `wp_sim` gives it a body), the way
                        // it faces.
                        self.record_spawn(world, i);
                        self.start_ambush(i, req.racer, plan.wait, plan.track);
                        continue;
                    }
                    self.rebuild_obstacles();
                    let Some((pos, heading)) = self
                        .response_spawn_point(world, req.racer)
                        .or_else(|| self.reinforcement_spawn_point(world, req.racer, target))
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
            self.note_first_backup(req.racer);
            self.assign(
                i,
                Assignment::Intercept {
                    target: req.racer,
                    point: target,
                    ambush: false,
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
                    && !self.booking(u.id)
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
            let vel = self.ambush_vel(racer);
            // Set up ahead, facing the racer's way, when it can get there first (PR #33);
            // else to its predicted position.
            if let Some((wait, track)) = self.ambush_route(i, target, vel) {
                self.start_ambush(i, racer, wait, track);
            } else {
                self.assign(
                    i,
                    Assignment::Intercept {
                        target: racer,
                        point: target,
                        ambush: false,
                    },
                );
            }
            self.stats.backup_joins += 1;
            // Reinforcement delay for backup: pursuit start → unit committed.
            let since = self.racers[&racer]
                .pursuit_started
                .map_or(0.0, |t| self.now.since(t));
            self.stats.reinforcement_delays_s.push(since);
            self.note_first_backup(racer);
            joined += 1;
        }
        joined
    }

    /// The racer's velocity for planning an ambush, or zero (no ambush): only on a racer police
    /// can see, and at most one new ambush per [`AMBUSH_EVERY_S`] (back-to-back ambushes kept
    /// a fast racer in sight forever: escape_break_contact_night ran 900 s, never escaped,
    /// never caught).
    fn ambush_vel(&self, racer: EntityId) -> [f32; 2] {
        let Some(rec) = self.racers.get(&racer) else {
            return [0.0, 0.0];
        };
        let recent = self
            .last_ambush
            .get(&racer)
            .is_some_and(|t| self.now.since(*t) < AMBUSH_EVERY_S);
        if rec.status == PursuitStatus::Pursuit && !recent {
            [rec.vel[0], rec.vel[1]]
        } else {
            [0.0, 0.0]
        }
    }

    /// Log the first unit committed to `racer`'s current pursuit, `since` its start (once per
    /// pursuit): what "backup arrives later at night" means.
    fn note_first_backup(&mut self, racer: EntityId) {
        let Some(started) = self.racers.get(&racer).and_then(|r| r.pursuit_started) else {
            return;
        };
        if self.first_backup.get(&racer) != Some(&started) {
            self.first_backup.insert(racer, started);
            let since = self.now.since(started);
            self.stats.first_backup_delays_s.push(since);
        }
    }

    /// Send unit `i` to wait for `racer` at `wait` along `track`, lights on.
    fn start_ambush(&mut self, i: usize, racer: EntityId, wait: [f32; 2], track: Track) {
        self.last_ambush.insert(racer, self.now);
        self.assign(
            i,
            Assignment::Intercept {
                target: racer,
                point: wait,
                ambush: true,
            },
        );
        let cornering_g = self.cfg.pursuit.pursuit_cornering_g;
        self.units[i].set_command(DriveCommand {
            track,
            speed_mps: 0.0,
            cornering_g,
            siren: true,
            lane_offset_m: LANE_OFFSET_M,
        });
    }

    /// Candidate ambush wait points for a racer at `racer` moving at `vel`, nearest first: just
    /// past each junction on its predicted path, ≥ [`AMBUSH_MIN_LEAD_S`] of its travel ahead,
    /// not already taken by another ambusher. Also returns every node on the racer's road, which
    /// a unit must not drive along to get in place (it would be seen, or come at the racer
    /// head-on). Empty for a stopped racer.
    fn ambush_waits(&self, racer: [f32; 2], vel: [f32; 2]) -> (Vec<AmbushWait>, Vec<NodeId>) {
        let speed = (vel[0] * vel[0] + vel[1] * vel[1]).sqrt();
        if speed < crate::pursuit::STOPPED_RACER_MPS {
            return (Vec::new(), Vec::new());
        }
        let path = self.graph.predict_path(racer, vel, AMBUSH_MAX_AHEAD_M);
        let mut on_path: Vec<NodeId> = Vec::new();
        for leg in &path.legs {
            on_path.push(leg.from_node);
            on_path.push(self.graph.other_end(leg.segment, leg.from_node));
        }
        // One unit per wait point: later reinforcements take the next junction along.
        let taken: Vec<[f32; 2]> = self
            .units
            .iter()
            .filter_map(|u| match u.assignment {
                Assignment::Intercept {
                    point,
                    ambush: true,
                    ..
                } => Some(point),
                _ => None,
            })
            .collect();
        let mut waits = Vec::new();
        let mut leg_start = 0.0;
        for (k, leg) in path.legs.iter().enumerate() {
            let s0 = leg_start;
            leg_start += leg.len_m;
            // The first leg starts behind the racer; short legs leave no room to pull in.
            if k == 0 || leg.len_m < 2.0 * AMBUSH_INTO_LEG_M {
                continue;
            }
            let racer_eta = (s0 + AMBUSH_INTO_LEG_M) / speed;
            if racer_eta < AMBUSH_MIN_LEAD_S {
                continue;
            }
            let wait = add(
                leg.from,
                scale(normalize(sub(leg.to, leg.from)), AMBUSH_INTO_LEG_M),
            );
            if taken
                .iter()
                .any(|t| dist(*t, wait) < 2.0 * AMBUSH_INTO_LEG_M)
            {
                continue;
            }
            waits.push(AmbushWait {
                junction: leg.from_node,
                wait,
                racer_eta,
            });
        }
        (waits, on_path)
    }

    /// Where a fresh reinforcement should wait for a moving racer, if anywhere (see
    /// [`Self::ambush_waits`]): it spawns on a side road the racer cannot see and drives in, so
    /// it arrives facing the racer's direction of travel and joins the chase as the racer comes
    /// by instead of turning around behind it. Spawns at least `spawn_min_distance_m` from the
    /// racer, and only where the unit can reach the wait point [`AMBUSH_SETTLE_S`] before the
    /// racer does.
    fn ambush_plan(
        &self,
        world: &dyn WorldQuery,
        racer_id: EntityId,
        racer: [f32; 2],
        vel: [f32; 2],
    ) -> Option<AmbushPlan> {
        let min_d = self.cfg.presence.reinforcement.spawn_min_distance_m;
        let corridor = self.racer_corridor(racer_id);
        let (waits, on_path) = self.ambush_waits(racer, vel);
        for w in waits {
            // Closest side-road nodes first (index breaks ties: deterministic).
            let mut cands: Vec<(f32, usize)> = self
                .graph
                .nodes
                .iter()
                .enumerate()
                .filter(|(n, node)| {
                    !on_path.contains(&NodeId(*n as u32))
                        && dist(node.pos, w.wait) <= AMBUSH_SPAWN_RADIUS_M
                        && dist(node.pos, racer) >= min_d
                })
                .map(|(n, node)| (dist(node.pos, w.wait), n))
                .collect();
            cands.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
            // Of the spots that make it in time, take the quickest to get into place.
            let mut best: Option<(f32, AmbushPlan)> = None;
            for &(_, n) in cands.iter().take(AMBUSH_SPAWN_CANDIDATES) {
                // Row 140: never on the far carriageway of the racer's divided road.
                if self.node_far_side(&corridor, NodeId(n as u32)) {
                    continue;
                }
                let Some(route) = self.graph.shortest_path(NodeId(n as u32), w.junction) else {
                    continue;
                };
                if route.len() < 2 || route[..route.len() - 1].iter().any(|r| on_path.contains(r)) {
                    continue;
                }
                let unit_eta = self.route_time_s(&route) + AMBUSH_SETTLE_S;
                if unit_eta > w.racer_eta || best.as_ref().is_some_and(|b| unit_eta >= b.0) {
                    continue;
                }
                let start = self.graph.node(route[0]).pos;
                let heading = normalize(sub(self.graph.node(route[1]).pos, start));
                // In the right-hand lane, facing the first turn of the route.
                let pos = add(start, scale([heading[1], -heading[0]], LANE_OFFSET_M));
                if !self.spot_clear(pos) || self.spawn_in_view(world, pos) {
                    continue;
                }
                // The track runs on the centreline; the drive command adds the lane offset.
                let mut pts: Vec<[f32; 2]> =
                    route.iter().map(|r| self.graph.node(*r).pos).collect();
                pts.push(w.wait);
                let plan = AmbushPlan {
                    pos,
                    heading,
                    wait: w.wait,
                    track: Track::new(pts),
                };
                best = Some((unit_eta, plan));
            }
            if let Some((_, plan)) = best {
                return Some(plan);
            }
        }
        None
    }

    /// Wait point and track for unit `i` (already on the road) to set up ahead of a racer at
    /// `racer` moving at `vel` (see [`Self::ambush_waits`]), if it can get there
    /// [`AMBUSH_SETTLE_S`] before the racer without driving along the racer's road.
    fn ambush_route(&self, i: usize, racer: [f32; 2], vel: [f32; 2]) -> Option<([f32; 2], Track)> {
        let (waits, on_path) = self.ambush_waits(racer, vel);
        let u = &self.units[i];
        let (pos, heading) = (u.body.pos, u.body.heading);
        // A unit on the racer's road itself would meet it head-on or be seen getting there.
        let rp = self.graph.nearest_on_road(pos)?;
        let seg = self.graph.segment(rp.segment);
        if on_path.contains(&seg.a) && on_path.contains(&seg.b) {
            return None;
        }
        let start = self.start_node(pos, heading)?;
        let to_start = sub(self.graph.node(start).pos, pos);
        let cap = self.cfg.pursuit.pursuit_cap_mps.get(self.phase);
        let v = (seg.speed_limit_mps * self.cfg.pursuit.respond_speed_factor * 1.15)
            .min(cap)
            .max(1.0);
        let mut lead_in = dist(pos, self.graph.node(start).pos) / v * AMBUSH_PACE_MARGIN;
        if crate::graph::dot(to_start, heading) < 0.0 {
            lead_in += AMBUSH_JUNCTION_S; // turning around first
        }
        for w in waits {
            let Some(route) = self.graph.shortest_path(start, w.junction) else {
                continue;
            };
            if route[..route.len() - 1].iter().any(|r| on_path.contains(r)) {
                continue;
            }
            if lead_in + self.route_time_s(&route) + AMBUSH_SETTLE_S > w.racer_eta {
                continue;
            }
            let (track, _) = self.route_from(pos, heading, w.junction, Some(w.wait));
            if track.is_drivable() {
                return Some((w.wait, track));
            }
        }
        None
    }

    /// Driving time along a node route from standstill at intercept pace, s: cruise time
    /// padded by [`AMBUSH_PACE_MARGIN`] plus [`AMBUSH_JUNCTION_S`] for every junction passed.
    fn route_time_s(&self, route: &[NodeId]) -> f32 {
        let p = &self.cfg.pursuit;
        let cap = p.pursuit_cap_mps.get(self.phase);
        let mut t = AMBUSH_JUNCTION_S * route.len().saturating_sub(1) as f32;
        for w in route.windows(2) {
            if let Some(&(s, _)) = self.graph.outgoing(w[0]).iter().find(|(_, n)| *n == w[1]) {
                let seg = self.graph.segment(s);
                let v = (seg.speed_limit_mps * p.respond_speed_factor * 1.15)
                    .min(cap)
                    .max(1.0);
                t += seg.length_m / v * AMBUSH_PACE_MARGIN;
            }
        }
        t
    }

    /// The closest node at least `spawn_min_distance_m` from `target` without line of sight.
    fn reinforcement_spawn_point(
        &self,
        world: &dyn WorldQuery,
        racer: EntityId,
        target: [f32; 2],
    ) -> Option<([f32; 2], [f32; 2])> {
        let min_d = self.cfg.presence.reinforcement.spawn_min_distance_m;
        let corridor = self.racer_corridor(racer);
        let mut best: Option<(f32, usize)> = None;
        for (i, n) in self.graph.nodes.iter().enumerate() {
            let d = dist(n.pos, target);
            if d < min_d
                || best.is_some_and(|b| d >= b.0)
                || self.sees(world, n.pos, target)
                || self.node_far_side(&corridor, NodeId(i as u32))
                || self.spawn_in_view(world, n.pos)
                || !self.spot_clear(n.pos)
            {
                continue;
            }
            best = Some((d, i));
        }
        best.map(|(_, i)| {
            let p = self.graph.nodes[i].pos;
            // On a one-way road: facing its flow (row 140), not straight at the racer.
            let heading = self
                .node_oneway_flow(NodeId(i as u32))
                .unwrap_or_else(|| normalize(sub(target, p)));
            (p, heading)
        })
    }

    /// The flow of the one-way road leaving node `n`, if every road leaving it is one-way
    /// (a node along a carriageway).
    fn node_oneway_flow(&self, n: NodeId) -> Option<[f32; 2]> {
        let out = self.graph.outgoing(n);
        let all_oneway = !out.is_empty()
            && out.iter().all(|(s, _)| self.graph.segment(*s).oneway)
            && self
                .graph
                .incoming(n)
                .iter()
                .all(|(s, _)| self.graph.segment(*s).oneway);
        all_oneway.then(|| self.graph.dir_from(out[0].0, n))
    }

    /// Whether node `n` is on the far carriageway of the racer with `corridor` (row 140): any
    /// one-way road through it runs the other way alongside the racer's road.
    fn node_far_side(&self, corridor: &Corridor, n: NodeId) -> bool {
        let p = self.graph.node(n).pos;
        self.graph
            .outgoing(n)
            .iter()
            .chain(self.graph.incoming(n))
            .filter(|(s, _)| self.graph.segment(*s).oneway)
            .any(|(s, _)| {
                let seg = self.graph.segment(*s);
                Self::on_far_carriageway(corridor, p, self.graph.dir_from(*s, seg.a))
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
                RadioTier::Investigation,
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
        // Row 144, tier 1: the helicopter deploying is news the driver can use.
        self.radio(
            RadioCue::new(
                RadioTier::Actionable,
                None,
                format!(
                    "Dispatch: air unit launching on racer #{}, ETA {launch:.0} seconds.",
                    racer.0
                ),
            )
            .supersedes(format!("air:{}", racer.0)),
        );
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

    /// Debug / e2e: put a helicopter on `racer` now, `offset_m` away from its last known position
    /// at cruise altitude, skipping availability, cooldown, budget and launch time. `None` unless
    /// the racer is pursued.
    pub fn debug_launch_helicopter(&mut self, racer: EntityId, offset_m: f32) -> Option<UnitId> {
        let at = self
            .racers
            .get(&racer)
            .filter(|r| r.pursued())
            .and_then(|r| r.last_known)?;
        let pos = [at[0] + offset_m, at[1]];
        let i = self.spawn_unit(UnitKind::Helicopter, UnitRole::Helicopter, pos, [-1.0, 0.0]);
        self.units[i].body.alt_m = self.cfg.tools.helicopter.altitude_m;
        self.stats.helo_launches += 1;
        let id = self.units[i].id;
        self.pending.push(PoliceEvent::HeloLaunched {
            unit: id,
            target: racer,
        });
        self.assign(i, Assignment::HeloTrack { target: racer });
        Some(id)
    }

    /// Helicopter radio from this think's line of sight: "eyes on" when it first sees its
    /// target, a position report every `position_report_s` while it keeps visual, "lost
    /// visual" after `lost_visual_after_s` without line of sight (under a deck, in a tunnel or
    /// a parking structure), "reacquired" when it sees the car again. Ground units already
    /// steer by what it sees (it keeps the racer's last known position fresh); these lines are
    /// the scanner's and the player's view of it.
    pub(crate) fn helo_reports(&mut self) {
        let now = self.now;
        let every = self.cfg.tools.helicopter.position_report_s;
        let lost_after = self.cfg.tools.helicopter.lost_visual_after_s;
        let tracking: Vec<(UnitId, EntityId)> = self
            .units
            .iter()
            .filter_map(|u| match u.assignment {
                Assignment::HeloTrack { target } if !u.wrecked => Some((u.id, target)),
                _ => None,
            })
            .collect();
        self.helo_sight
            .retain(|id, _| tracking.iter().any(|(u, _)| u == id));
        for (id, target) in tracking {
            let Some(s) = self.helo_sight.get(&id).copied() else {
                continue; // not in range yet (update_observations adds it on first sight)
            };
            let callsign = format!("Air {}", id.0);
            let mut s = s;
            let mut cue = None;
            if s.los {
                s.last_los = now;
                if !s.visual {
                    s.visual = true;
                    s.next_report = now.plus(every);
                    let desc = self.racer_desc(target);
                    // Both are position calls the ground units act on: always spoken. A
                    // lost/reacquired pair needs `lost_visual_after_s` of cover in between.
                    let text = if s.acquired {
                        format!("{callsign}: reacquired, {desc}.")
                    } else {
                        format!("{callsign}: eyes on {desc}. Spotlight on.")
                    };
                    cue = Some(
                        RadioCue::new(RadioTier::Actionable, Some(id), text)
                            .supersedes(format!("air:{}", target.0)),
                    );
                    s.acquired = true;
                } else if now >= s.next_report {
                    s.next_report = now.plus(every);
                    let desc = self.racer_desc(target);
                    let kmh = self.racers.get(&target).map_or(0.0, |r| {
                        (r.vel[0] * r.vel[0] + r.vel[1] * r.vel[1]).sqrt() * 3.6
                    });
                    cue = Some(RadioCue::new(
                        RadioTier::Investigation,
                        Some(id),
                        format!("{callsign}: still on {desc}, {kmh:.0} km/h."),
                    ));
                }
            } else if s.visual && now.since(s.last_los) >= lost_after {
                s.visual = false;
                let desc = self.racer_desc(target);
                cue = Some(
                    RadioCue::new(
                        RadioTier::Actionable,
                        Some(id),
                        format!("{callsign}: lost visual on {desc}. Units, check the cover."),
                    )
                    .supersedes(format!("air:{}", target.0)),
                );
            }
            self.helo_sight.insert(id, s);
            if let Some(c) = cue {
                self.radio(c);
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
        if self.roadblocks.iter().filter(|r| !r.scripted).count() as u32 >= rb.max_active {
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
        // Row 133: across the racer's carriageway only (centred on it, never over the
        // median of a divided road).
        let span = self.tool_span(&site);
        let mut across = site;
        across.pos = span.at(site.pos, 0.5 * (span.inner + span.outer));
        let half = 0.5 * (span.outer - span.inner);
        let barriers =
            roadblock_barriers(&across, rb.barrier_half_extents_m, rb.barrier_mass_kg, half);
        let lifetime = rb.lifetime_s;
        let cooldown = rb.cooldown_s.get(self.phase);
        let id = ToolId(self.next_tool);
        self.next_tool += 1;
        for (pos, heading) in roadblock_unit_spots(&across, n, half) {
            self.tool_cars.push(PendingToolCar {
                tool: id,
                pos,
                heading,
                assignment: Assignment::RoadblockAt {
                    segment: site.segment,
                    tool: id,
                },
            });
        }
        self.roadblocks.push(Roadblock {
            id,
            racer,
            site,
            units: Vec::new(),
            barriers,
            expires: now.plus(lifetime),
            scripted: false,
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

    fn scenario_site(pos: [f32; 2], dir: [f32; 2]) -> ToolSite {
        ToolSite {
            segment: crate::graph::SegmentId(0),
            from_node: NodeId(0),
            d_m: 0.0,
            pos,
            dir: normalize(dir),
            sight_m: 0.0,
            ahead_m: 0.0,
        }
    }

    /// Scenario / test placement: a roadblock's barriers across the road at `pos`, facing
    /// traffic that travels along `dir`, attributed to `racer`. Unlike
    /// [`PoliceSystem::captain_command`] this bypasses every dispatch rule (pursuit, unlock
    /// level, cooldown, budget, site planning) and spawns no units, so a scenario can put a
    /// barricade exactly where a car will hit it, even on the flat world. Expires after the
    /// configured lifetime like any roadblock; emits `RoadblockPlaced`.
    pub fn place_roadblock_at(&mut self, racer: EntityId, pos: [f32; 2], dir: [f32; 2]) -> ToolId {
        let rb = &self.cfg.tools.roadblock;
        let site = Self::scenario_site(pos, dir);
        let barriers = roadblock_barriers(
            &site,
            rb.barrier_half_extents_m,
            rb.barrier_mass_kg,
            ROAD_HALF_WIDTH_M,
        );
        let expires = self.now.plus(rb.lifetime_s);
        let id = ToolId(self.next_tool);
        self.next_tool += 1;
        self.roadblocks.push(Roadblock {
            id,
            racer,
            site,
            units: Vec::new(),
            barriers,
            expires,
            scripted: false,
        });
        self.stats.roadblocks += 1;
        self.pending.push(PoliceEvent::RoadblockPlaced {
            tool: id,
            racer,
            pos: [pos[0], pos[1], 0.0],
            sight_m: 0.0,
        });
        id
    }

    /// Scenario / test placement: a spike strip across the road at `pos`, perpendicular to
    /// `dir`, attributed to `racer`, bypassing every dispatch rule (see
    /// [`PoliceSystem::place_roadblock_at`]), the row-131 spawn view check included. Like
    /// every strip it comes with a spike unit parked on the shoulder beside it (left of `dir`)
    /// and is lifted when that unit leaves (PR #54). Any racer crossing it is spiked. Emits
    /// `SpikesPlaced`.
    pub fn place_spikes_at(&mut self, racer: EntityId, pos: [f32; 2], dir: [f32; 2]) -> ToolId {
        let sc = &self.cfg.tools.spikes;
        let (length, lifetime) = (sc.length_m, sc.lifetime_s);
        let site = Self::scenario_site(pos, dir);
        let id = ToolId(self.next_tool);
        self.next_tool += 1;
        let post = add(
            site.pos,
            scale(
                perp_left(site.dir),
                ROAD_HALF_WIDTH_M + SPIKE_CAR_SHOULDER_M,
            ),
        );
        let i = self.spawn_unit(UnitKind::Car, UnitRole::Tool, post, site.dir);
        self.assign(
            i,
            Assignment::SpikeAt {
                segment: site.segment,
                tool: id,
            },
        );
        let mut strip = SpikeStrip::across(id, racer, site, length, None, self.now.plus(lifetime));
        strip.unit = Some(self.units[i].id);
        strip.post = Some(post);
        self.lay_spikes(strip);
        id
    }

    /// Put `strip` on the road (its unit is beside it) and announce it (`SpikesPlaced`).
    fn lay_spikes(&mut self, strip: SpikeStrip) {
        self.pending.push(PoliceEvent::SpikesPlaced {
            tool: strip.id,
            racer: strip.racer,
            a: [strip.a[0], strip.a[1], 0.0],
            b: [strip.b[0], strip.b[1], 0.0],
            sight_m: strip.site.sight_m,
        });
        self.spikes.push(strip);
    }

    /// Whether strip `s` may stay on the road as far as its unit goes. A manned strip needs
    /// its unit beside it: present, not wrecked, still on this strip's post and within
    /// [`SPIKE_UNIT_LEASH_M`] of where it parked. Strips without a unit (scripted race-profile
    /// strips; strips whose car's spot was in view) are exempt here; an unmanned dynamic strip
    /// goes once its racer has passed it ([`Self::spike_passed`]).
    fn spike_unit_holds(&self, s: &SpikeStrip) -> bool {
        let Some(uid) = s.unit else {
            return true;
        };
        let Some(u) = self.units.iter().find(|u| u.id == uid) else {
            return false;
        };
        !u.wrecked
            && matches!(u.assignment, Assignment::SpikeAt { tool, .. } if tool == s.id)
            && dist(u.body.pos, s.post.unwrap_or(s.site.pos)) <= SPIKE_UNIT_LEASH_M
    }

    /// Whether an unmanned dynamic strip's racer is more than [`SPIKE_PASSED_M`] past it along
    /// the road (the strip has done its job, or missed).
    fn spike_passed(&self, s: &SpikeStrip) -> bool {
        if s.unit.is_some() || s.scripted {
            return false;
        }
        self.racers
            .get(&s.racer)
            .is_some_and(|r| dot(sub(xy(r.pos), s.site.pos), s.site.dir) > SPIKE_PASSED_M)
    }

    /// Remove strip `tool` from the road (no-op if already gone). Its unit is not touched.
    fn lift_spikes(&mut self, tool: ToolId) {
        let before = self.spikes.len();
        self.spikes.retain(|s| s.id != tool);
        if self.spikes.len() != before {
            self.pending.push(PoliceEvent::ToolRemoved { tool });
        }
    }

    /// The host could not give AI unit `unit` a physical car (vehicle cap, failed spawn). A
    /// spike unit is then withdrawn and its strip lifted, so no strip lies on the road without
    /// a visible police car; returns whether that happened. Other units are left as they are.
    pub fn withdraw_unbodied_spike_unit(&mut self, unit: UnitId) -> bool {
        let Some(i) = self.unit_index(unit) else {
            return false;
        };
        if !matches!(self.units[i].assignment, Assignment::SpikeAt { .. })
            || self.units[i].entity.is_some()
            || self.units[i].player.is_some()
        {
            return false;
        }
        self.despawn(i);
        true
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
        if self.spikes.iter().filter(|s| !s.scripted).count() as u32 >= sc.max_active {
            return Err(CaptainError::Budget);
        }
        let (p, v) = self.racer_motion(racer).ok_or(CaptainError::NotPursued)?;
        let rules = SiteRules {
            min_ahead_m: sc.min_ahead_m,
            max_ahead_m: sc.max_ahead_m,
            min_sight_m: sc.min_sight_m,
            highway_ok: true,
        };
        // PR #54: the strip goes down with its car, so the site needs a shoulder spot the car
        // can appear on (row 75: clear of parked cars and cones, and of other vehicles).
        let parkable = |site: &ToolSite| {
            let span = self.tool_span(site);
            self.spot_clear(span.at(site.pos, span.outer + SPIKE_CAR_SHOULDER_M))
        };
        let site = plan_site_where(&self.graph, p, v, &rules, road.map(|r| r.0), &parkable)
            .map_err(CaptainError::Site)?;
        let (length, lifetime, cooldown) =
            (sc.length_m, sc.lifetime_s, sc.cooldown_s.get(self.phase));
        let id = ToolId(self.next_tool);
        self.next_tool += 1;
        // Row 133: the car parks on the right shoulder of the racer's own carriageway,
        // facing its traffic; on a one-way carriageway (a divided road's half) the strip
        // spans that carriageway, lane edge to median.
        // PR #54 port (lead, 2026-10-23; rows 131 / 158: no police car appears in a racer's
        // view): the strip goes down now. Its car is decided on the next think
        // (`maintain_tool_cars`, which has the world for the view check): spot hidden, the car
        // appears beside the strip; in view, no car and the strip stays unmanned, like a
        // scripted race strip, until its racer passes it, the pursuit ends or it expires.
        let span = self.tool_span(&site);
        let post = span.at(site.pos, span.outer + SPIKE_CAR_SHOULDER_M);
        let expires = now.plus(lifetime);
        let strip = if span.oneway {
            let (a, b) = (span.at(site.pos, span.inner), span.at(site.pos, span.outer));
            SpikeStrip::between(id, racer, site, a, b, None, expires)
        } else {
            SpikeStrip::across(id, racer, site, length, None, expires)
        };
        self.tool_cars.push(PendingToolCar {
            tool: id,
            pos: post,
            heading: span.flow,
            assignment: Assignment::SpikeAt {
                segment: site.segment,
                tool: id,
            },
        });
        self.lay_spikes(strip);
        self.captain.spikes_ready = now.plus(cooldown);
        self.stats.spikes += 1;
        self.stats.tool_sight_m.push(site.sight_m);
        Ok(id)
    }

    /// The traffic flow and lateral span a tool at `site` may use (row 133, see
    /// [`ToolSpan`]): a one-way segment's carriageway, or a two-way road's full width
    /// ([`ROAD_HALF_WIDTH_M`] each side; the right half's real width when the lanes are known
    /// sets `outer`).
    pub(crate) fn tool_span(&self, site: &ToolSite) -> ToolSpan {
        let g = &self.graph;
        if let Some((inner, outer)) = g.carriageway_m(site.segment) {
            let a = g.segment(site.segment).a;
            return ToolSpan {
                flow: g.dir_from(site.segment, a),
                inner,
                outer,
                oneway: true,
            };
        }
        ToolSpan {
            flow: site.dir,
            inner: -ROAD_HALF_WIDTH_M,
            outer: g
                .half_road_m(site.segment, site.from_node)
                .unwrap_or(ROAD_HALF_WIDTH_M),
            oneway: false,
        }
    }

    /// Spawn the roadblock cars whose spots are now out of every racer's view, now and along
    /// its road ahead (row 131); drop those whose roadblock has gone. A spike car gets one
    /// chance: spot hidden, it appears beside its strip (which becomes manned); in view, it is
    /// dropped and the strip stays unmanned (PR #54 port).
    fn maintain_tool_cars(&mut self, world: &dyn WorldQuery) {
        let pending = std::mem::take(&mut self.tool_cars);
        for car in pending {
            let rb = self.roadblocks.iter().position(|r| r.id == car.tool);
            let sp = self.spikes.iter().position(|s| s.id == car.tool);
            // Row 140: by the time its spot is hidden the racer may have moved on, leaving the
            // spot on the far carriageway of its road now; such a car is not spawned at all.
            let racer = match (rb, sp) {
                (Some(k), _) => self.roadblocks[k].racer,
                (None, Some(k)) => self.spikes[k].racer,
                (None, None) => continue,
            };
            if self.far_side(&self.racer_corridor(racer), car.pos) {
                continue;
            }
            if self.warmed_up && self.spawn_in_view(world, car.pos) {
                if rb.is_some() {
                    self.tool_cars.push(car);
                }
                continue;
            }
            let i = self.spawn_unit(UnitKind::Car, UnitRole::Tool, car.pos, car.heading);
            let uid = self.units[i].id;
            self.assign(i, car.assignment);
            self.record_spawn(world, i);
            if let Some(k) = rb {
                self.roadblocks[k].units.push(uid);
            }
            if let Some(k) = sp {
                self.spikes[k].unit = Some(uid);
                self.spikes[k].post = Some(car.pos);
            }
        }
    }

    /// Row 86 audit: time active ground units stand still with no reason to, and log each
    /// stop longer than [`ACTIVE_STOP_S`] when it ends. Statistics only.
    fn audit_active_stops(&mut self, tdt: f32) {
        for i in 0..self.units.len() {
            let u = &self.units[i];
            let active = u.kind == UnitKind::Car
                && !u.wrecked
                && u.player.is_none()
                && matches!(
                    u.assignment,
                    Assignment::Pursue { .. }
                        | Assignment::Intercept { ambush: false, .. }
                        | Assignment::Respond { .. }
                );
            let excused = self.booking(u.id)
                || matches!(u.tactic, Tactic::RollingBlock)
                || u.assignment
                    .target()
                    .and_then(|t| self.racers.get(&t))
                    .is_some_and(|r| {
                        // The racer stopped close by: an arrest scene, not a jam.
                        len([r.vel[0], r.vel[1]]) < 3.0 && dist(xy(r.pos), u.body.pos) < 40.0
                    });
            let still = active && !excused && u.body.speed.abs() < 1.0;
            if still {
                if self.units[i].active_stop_s == 0.0 {
                    let u = &self.units[i];
                    let by = match u.avoid.blocked_by {
                        None => 0,
                        Some(crate::avoid::BlockedBy::Vehicle(e))
                            if Some(e) == u.assignment.target() =>
                        {
                            4
                        }
                        Some(crate::avoid::BlockedBy::Vehicle(_)) => 1,
                        Some(crate::avoid::BlockedBy::Unit(_)) => 2,
                        Some(crate::avoid::BlockedBy::Traffic) => 3,
                    };
                    self.units[i].active_stop_why = (u.branch, by);
                }
                self.units[i].active_stop_s += tdt;
                continue;
            }
            let stood = self.units[i].active_stop_s;
            if stood > ACTIVE_STOP_S {
                let u = &self.units[i];
                self.stats.active_stops.push(crate::ActiveStop {
                    unit: u.id,
                    at: self.now.plus(-stood),
                    stood_s: stood,
                    pos: u.body.pos,
                    branch: u.active_stop_why.0,
                    blocked_by: u.active_stop_why.1,
                });
            }
            self.units[i].active_stop_s = 0.0;
        }
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
            let keep = now < rb.expires && (rb.scripted || racer != Some(rb.racer));
            if !keep {
                removed.push((rb.id, rb.units.clone()));
            }
            keep
        });
        // PR #54: a strip whose unit has left its post (wrecked, shoved or driven away,
        // reassigned, gone) is lifted, and so is an unmanned strip its racer has passed.
        let unmanned: Vec<ToolId> = self
            .spikes
            .iter()
            .filter(|s| !self.spike_unit_holds(s) || self.spike_passed(s))
            .map(|s| s.id)
            .collect();
        self.spikes.retain(|s| {
            let keep = now < s.expires
                && (s.scripted || racer != Some(s.racer))
                && !unmanned.contains(&s.id);
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

/// A violation in a radio line ("running a red").
/// The radar uses the nearest road's limit for a racer this far off it at most, m.
const RADAR_ROAD_M: f32 = 12.0;

/// Whether a reset spot at `pos` is free: no other car's centre within
/// [`crate::stuck::RESET_CLEAR_M`] and no moving car about to drive through it.
fn reset_clear(pos: [f32; 2], others: &[crate::avoid::Obstacle]) -> bool {
    use crate::stuck::{RESET_CLEAR_M, RESET_PATH_S};
    others.iter().all(|o| {
        let rel = sub(pos, o.pos);
        if crate::graph::dot(rel, rel) < RESET_CLEAR_M * RESET_CLEAR_M {
            return false;
        }
        let v = o.speed();
        if v < 1.0 {
            return true;
        }
        let dir = [o.vel[0] / v, o.vel[1] / v];
        let along = crate::graph::dot(rel, dir);
        let lat = crate::graph::cross(dir, rel).abs();
        !(along > 0.0 && along < v * RESET_PATH_S + RESET_CLEAR_M && lat < 2.5)
    })
}

fn offence_text(kind: &ViolationKind) -> &'static str {
    match kind {
        ViolationKind::Speeding { .. } => "speeding past me",
        ViolationKind::RedLight => "run a red",
        ViolationKind::WrongWay => "going the wrong way",
        ViolationKind::Collision => "hit a car",
        ViolationKind::Reckless => "driving like a maniac",
    }
}
