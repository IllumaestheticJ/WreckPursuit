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
use crate::unit::{Assignment, DriveCommand, Unit, UnitRole, LANE_OFFSET_M};
use crate::PoliceSystem;

/// Road half width used for tool layout, m.
pub const ROAD_HALF_WIDTH_M: f32 = 5.0;

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
            } else {
                let id = IncidentId(self.next_incident);
                self.next_incident += 1;
                let delay = self
                    .cfg
                    .presence
                    .reinforcement
                    .report_dispatch_delay_s
                    .get(self.phase);
                self.incidents.push(Incident {
                    id,
                    kind: IncidentKind::Report { racer },
                    pos: at,
                    district,
                    created: now,
                    dispatch_at: now.plus(delay),
                    unit: None,
                    scene_until: None,
                });
                let text = if camera {
                    format!(
                        "Dispatch: traffic camera flagged a vehicle, {:?}.",
                        district
                    )
                } else {
                    format!("Dispatch: caller reports reckless driver, {:?}.", district)
                };
                self.radio(RadioCue::new(RadioTier::Tactical, None, text));
            }
        }
    }

    pub(crate) fn on_racer_total_loss(&mut self, racer: EntityId) {
        let r = &self.racers[&racer];
        if r.level > WantedLevel::None {
            self.end_pursuit(racer, PursuitOutcome::CapturedTotalLoss);
        }
    }

    pub(crate) fn on_contact(&mut self, a: EntityId, b: EntityId, point: [f32; 3], rel_speed: f32) {
        let ua = self.unit_index_by_entity(a);
        let ub = self.unit_index_by_entity(b);
        let ra = self.racers.contains_key(&a);
        let rb = self.racers.contains_key(&b);
        match (ua, ub, ra, rb) {
            (Some(i), _, _, true) => self.unit_racer_contact(i, b, rel_speed, false),
            (_, Some(i), true, _) => self.unit_racer_contact(i, a, rel_speed, false),
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

    /// Record a racer contact on unit `i`. `apply_damage` is used by the built-in driver
    /// (physical units get damage from `wp_damage` via `sync_unit`).
    pub(crate) fn unit_racer_contact(
        &mut self,
        i: usize,
        racer: EntityId,
        rel_speed: f32,
        apply_damage: bool,
    ) {
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
        let cost = self.cfg.pursuit.risk.cost_collision;
        let u = &mut self.units[i];
        u.racer_contact = true;
        u.risk -= cost;
        if apply_damage {
            u.health = (u.health - coef * rel_speed * rel_speed / 1000.0).max(0.0);
        }
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
            self.warm_up();
        }
        self.update_observations(world);
        self.helo_reports();
        self.update_racer_rules(tdt);
        self.check_unit_health();
        self.maintain_pool();
        self.dispatch_incidents();
        self.maintain_reinforcements(world);
        self.maintain_helicopters();
        if !self.captain.human {
            self.ai_captain_tools();
        }
        self.expire_tools();
        self.plan_units(world, tdt);
        self.refresh_junctions(world);
        self.cleanup_units();
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
                line(
                    tier,
                    format!("{callsign} responding to {what}."),
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

    fn spawn_pool_unit(&mut self, d: District) -> Option<usize> {
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
            if self.spawn_clear(pos) {
                let i = self.spawn_unit(UnitKind::Car, UnitRole::Pool(d), pos, dir);
                self.start_patrol(i, dest);
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

    fn warm_up(&mut self) {
        self.warmed_up = true;
        for d in District::ALL {
            let target = self.cfg.presence.patrols.get(d).get(self.phase);
            for _ in 0..target {
                self.spawn_pool_unit(d);
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Pool, incidents, reinforcements, helicopter
    // -----------------------------------------------------------------------------------------

    fn maintain_pool(&mut self) {
        for d in District::ALL {
            let target = self.cfg.presence.patrols.get(d).get(self.phase);
            let alive = self.pool_count(d);
            if alive < target && self.ground_units_alive() < self.cfg.presence.max_ground_units {
                self.spawn_pool_unit(d);
            } else if alive > target {
                // Retire one idle pool unit nobody is looking at.
                let racers: Vec<[f32; 2]> = self.racers.values().map(|r| xy(r.pos)).collect();
                if let Some(i) = self.units.iter().position(|u| {
                    u.role == UnitRole::Pool(d)
                        && u.available()
                        && racers.iter().all(|p| dist(*p, u.body.pos) > 300.0)
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
                || !matches!(u.role, UnitRole::Pool(_) | UnitRole::Reinforcement)
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

    fn dispatch_incidents(&mut self) {
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
        for k in 0..self.incidents.len() {
            let inc = self.incidents[k];
            if inc.unit.is_some() || now < inc.dispatch_at {
                continue;
            }
            if let Some(i) = self.nearest_available(inc.pos, f32::INFINITY) {
                self.incidents[k].unit = Some(self.units[i].id);
                self.assign(i, Assignment::Respond { incident: inc.id });
            }
        }
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
            let have = self.units_on(racer);
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
            let i = match self.nearest_available(target, 700.0) {
                Some(i) => i,
                None => {
                    if self.ground_units_alive() >= self.cfg.presence.max_ground_units {
                        continue;
                    }
                    let Some((pos, heading)) = self.reinforcement_spawn_point(world, target) else {
                        continue;
                    };
                    self.spawn_unit(UnitKind::Car, UnitRole::Reinforcement, pos, heading)
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
                    cue = Some(RadioCue::new(RadioTier::Critical, Some(id), text));
                    s.acquired = true;
                } else if now >= s.next_report {
                    s.next_report = now.plus(every);
                    let desc = self.racer_desc(target);
                    let kmh = self.racers.get(&target).map_or(0.0, |r| {
                        (r.vel[0] * r.vel[0] + r.vel[1] * r.vel[1]).sqrt() * 3.6
                    });
                    cue = Some(RadioCue::new(
                        RadioTier::Tactical,
                        Some(id),
                        format!("{callsign}: still on {desc}, {kmh:.0} km/h."),
                    ));
                }
            } else if s.visual && now.since(s.last_los) >= lost_after {
                s.visual = false;
                let desc = self.racer_desc(target);
                cue = Some(RadioCue::new(
                    RadioTier::Critical,
                    Some(id),
                    format!("{callsign}: lost visual on {desc}. Units, check the cover."),
                ));
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

    fn cleanup_units(&mut self) {
        let now = self.now;
        let racers: Vec<[f32; 2]> = self.racers.values().map(|r| xy(r.pos)).collect();
        let mut i = 0;
        while i < self.units.len() {
            let u = &self.units[i];
            let age = now.since(u.assigned);
            let far = racers.iter().all(|p| dist(*p, u.body.pos) > 250.0);
            let remove = match (&u.assignment, u.role) {
                (_, UnitRole::Player) => false,
                (Assignment::OutOfService, UnitRole::Helicopter) => age > 15.0,
                (Assignment::OutOfService, _) => age > if u.wrecked { 60.0 } else { 20.0 } && far,
                (Assignment::Idle, UnitRole::Reinforcement) => far || age > 30.0,
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
