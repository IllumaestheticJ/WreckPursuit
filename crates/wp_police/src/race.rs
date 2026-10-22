//! Race-specific police: one profile per race (`data/police/races.toml`, keyed by the race
//! id of `wp_race::catalog`), applied when a race is set up.
//!
//! A [`RaceProfile`] has two halves:
//! - **Doctrine** ([`RaceProfile::apply`]): which tactics and tools the police may use during
//!   the race ([`RaceTactics`]), speed overrides for the chase ([`RaceSpeed`]) and how much
//!   backup comes. It rewrites the unlock levels and chase tables of the police's
//!   [`PoliceConfig`]; nothing else in the crate knows about races.
//! - **Pre-scripted placements** ([`PoliceSystem::start_race`]): roadblocks across the course
//!   ([`RouteBlock`], with a gap on one side), roadblocks sealing the side roads off the course
//!   ([`SideBlocks`]), spike strips over part of the course ([`SpikeSpec`]) and units staked
//!   out beside it ([`StakeoutSpec`]) that join or start a chase when a racer passes. Scripted
//!   tools stay for the profile's `tool_lifetime_s`, outlive any one racer's pursuit and do not
//!   count against the AI captain's tool budget.
//!
//! Patrol numbers come from the profile's `presence`, which the race's scenario carries
//! (`Scenario::police_presence`); everything else is applied here.
//!
//! Same code path for both damage modes: a profile changes what the police do, never physics.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::{EntityId, WorldQuery};

use crate::config::{ConfigError, PoliceConfig};
use crate::dispatch::ROAD_HALF_WIDTH_M;
use crate::events::{PoliceEvent, ToolId, UnitId, UnitKind};
use crate::graph::{
    add, closest_on_segment, dist, dot, len, normalize, perp_left, scale, sub, xy, NodeId,
    SegmentId,
};
use crate::heat::WantedLevel;
use crate::radio::{RadioCue, RadioTier};
use crate::tools::{
    roadblock_barriers, roadblock_unit_spots, Roadblock, SpikeStrip, ToolSite, NODE_CLEARANCE_M,
};
use crate::unit::{Assignment, UnitRole, LANE_OFFSET_M};
use crate::PoliceSystem;

/// A tactic switched off unlocks at this wanted level, which no racer reaches (L5 is the top).
pub const NEVER_LEVEL: u8 = 6;

/// Scripted tools keep this far from the start line and the finish, m.
const ENDS_CLEAR_M: f32 = 40.0;

/// A course point is "at" a junction node this close to it, m.
const JUNCTION_ON_COURSE_M: f32 = 12.0;

/// A side road's direction is across the course when |cos| to the course is below this.
const SIDE_ROAD_MAX_COS: f32 = 0.5;

/// A stakeout looks for a side road this far either side of its spot along the course, m.
const STAKEOUT_SIDE_ROAD_M: f32 = 60.0;

/// A side-road stakeout parks this far in from the junction, m.
const STAKEOUT_INTO_M: f32 = 15.0;

/// A shoulder stakeout parks this far outside the road's edge, m.
const SHOULDER_CLEAR_M: f32 = 2.0;

/// A stakeout ignores racers slower than this (a car parked by the course is not a racer
/// flying past), m/s.
const STAKEOUT_MIN_RACER_MPS: f32 = 5.0;

const BUILTIN_RACES: &str = include_str!("../../../data/police/races.toml");

fn default_one() -> f32 {
    1.0
}

fn default_true() -> bool {
    true
}

fn default_units() -> u32 {
    2
}

fn default_side_units() -> u32 {
    1
}

fn default_into_m() -> f32 {
    25.0
}

/// Which tactics and dynamic tools the police may use during the race. Everything is allowed
/// unless switched off. Scripted placements are separate and always happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RaceTactics {
    /// AI captain roadblocks ahead of a pursued racer.
    #[serde(default = "default_true")]
    pub roadblocks: bool,
    /// AI captain spike strips ahead of a pursued racer.
    #[serde(default = "default_true")]
    pub spikes: bool,
    /// Helicopter support.
    #[serde(default = "default_true")]
    pub helicopter: bool,
    /// Box-ins of slow racers.
    #[serde(default = "default_true")]
    pub box_in: bool,
    /// PIT manoeuvres.
    #[serde(default = "default_true")]
    pub pit: bool,
    /// Rear-quarter, side and hard rams (and the flanking unit that sets up the side slam).
    #[serde(default = "default_true")]
    pub rams: bool,
    /// Rolling blocks by units that got ahead.
    #[serde(default = "default_true")]
    pub rolling_block: bool,
    /// Interceptors parking across the junction ahead.
    #[serde(default = "default_true")]
    pub junction_block: bool,
}

impl Default for RaceTactics {
    fn default() -> Self {
        Self {
            roadblocks: true,
            spikes: true,
            helicopter: true,
            box_in: true,
            pit: true,
            rams: true,
            rolling_block: true,
            junction_block: true,
        }
    }
}

/// Optional chase speed overrides (unset = the normal `pursuit.toml` values).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RaceSpeed {
    /// Absolute pursuit speed cap in every day phase, m/s (`pursuit_cap_mps`).
    #[serde(default)]
    pub pursuit_cap_mps: Option<f32>,
    /// PIT allowed below this racer speed per wanted level, km/h (`chase.pit_max_kmh`).
    #[serde(default)]
    pub pit_max_kmh: Option<[f32; 5]>,
    /// Pursuers close on the racer up to its speed × (1 + this) (`chase.speed_margin`).
    #[serde(default)]
    pub speed_margin: Option<f32>,
}

/// Side of the road a route roadblock leaves open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GapSide {
    /// Left of the racing direction.
    #[default]
    Left,
    /// Right of the racing direction.
    Right,
}

/// A roadblock across the course, facing the racing direction.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteBlock {
    /// Where, as a fraction of the course length (moved on to the first spot clear of
    /// junctions).
    pub at: f32,
    /// Open gap at one edge of the road, m (0 = closed; racers ram through the barriers).
    #[serde(default)]
    pub gap_m: f32,
    /// Which edge the gap is on.
    #[serde(default)]
    pub gap_side: GapSide,
    /// Police cars parked in the block (on the closed side).
    #[serde(default = "default_units")]
    pub units: u32,
}

/// Roadblocks sealing side roads that branch off the course.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SideBlocks {
    /// Course fraction from which side roads are candidates.
    pub from: f32,
    /// ... up to this fraction.
    pub to: f32,
    /// Side roads to seal (spread evenly over the candidates).
    pub count: u32,
    /// The block sits this far down the side road from the junction, m.
    #[serde(default = "default_into_m")]
    pub into_m: f32,
    /// Police cars parked in each block.
    #[serde(default = "default_side_units")]
    pub units: u32,
}

/// A unit staked out beside the course.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StakeoutSpec {
    /// Where, as a fraction of the course length (a side road nearby, else the shoulder).
    pub at: f32,
    /// A racer passing this close with line of sight springs the stakeout, m.
    pub trigger_m: f32,
}

/// A spike strip over part of the course.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpikeSpec {
    /// Where, as a fraction of the course length.
    pub at: f32,
    /// Strip length, m (shorter than the road: the rest stays open).
    pub length_m: f32,
    /// Strip centre this far left (+) or right (−) of the course line (the planned lane), m.
    #[serde(default)]
    pub offset_m: f32,
}

/// One race's police (a table of `data/police/races.toml`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RaceProfile {
    /// Display name of the style ("Downtown lockdown").
    pub name: String,
    /// One line for the race picker.
    pub summary: String,
    /// Patrol multiplier for the race's scenario (`Scenario::police_presence`).
    pub presence: f32,
    /// Reinforcements per wanted level × this.
    #[serde(default = "default_one")]
    pub backup_scale: f32,
    /// Scripted roadblocks and spike strips stay this long after the race is set up, s.
    #[serde(default)]
    pub tool_lifetime_s: f32,
    /// A racer that springs a stakeout is recognised at once (made wanted if it was not) and
    /// the stakeout starts the pursuit; otherwise a stakeout only joins existing pursuits.
    #[serde(default)]
    pub recognise_on_sight: bool,
    /// Allowed tactics and dynamic tools.
    #[serde(default)]
    pub tactics: RaceTactics,
    /// Chase speed overrides.
    #[serde(default)]
    pub speed: RaceSpeed,
    /// Roadblocks across the course.
    #[serde(default)]
    pub route_blocks: Vec<RouteBlock>,
    /// Roadblocks on side roads off the course.
    #[serde(default)]
    pub side_blocks: Vec<SideBlocks>,
    /// Units staked out beside the course.
    #[serde(default)]
    pub stakeouts: Vec<StakeoutSpec>,
    /// Spike strips over part of the course.
    #[serde(default)]
    pub spikes: Vec<SpikeSpec>,
}

impl RaceProfile {
    /// Rewrite `cfg` for this race: tactics switched off unlock at [`NEVER_LEVEL`], speed
    /// overrides replace the chase values, reinforcement counts scale by `backup_scale`.
    /// Patrol counts (`presence`) are not touched here: the race's scenario scales them.
    pub fn apply(&self, cfg: &mut PoliceConfig) {
        let t = &self.tactics;
        let unlocks = &mut cfg.presence.unlocks;
        if !t.roadblocks {
            unlocks.roadblock_level = NEVER_LEVEL;
        }
        if !t.spikes {
            unlocks.spikes_level = NEVER_LEVEL;
        }
        if !t.box_in {
            unlocks.boxin_level = NEVER_LEVEL;
        }
        if !t.pit {
            unlocks.pit_level = NEVER_LEVEL;
        }
        if !t.rolling_block {
            unlocks.rolling_block_level = NEVER_LEVEL;
        }
        let chase = &mut cfg.pursuit.chase;
        if !t.rams {
            chase.rear_ram_level = NEVER_LEVEL;
            chase.hard_ram_level = NEVER_LEVEL;
            chase.flank_level = NEVER_LEVEL;
        }
        if !t.junction_block {
            chase.junction_block_level = NEVER_LEVEL;
        }
        if let Some(kmh) = self.speed.pit_max_kmh {
            chase.pit_max_kmh = kmh;
        }
        if let Some(m) = self.speed.speed_margin {
            chase.speed_margin = m;
        }
        if let Some(cap) = self.speed.pursuit_cap_mps {
            let c = &mut cfg.pursuit.pursuit_cap_mps;
            for v in [&mut c.dawn, &mut c.day, &mut c.dusk, &mut c.night] {
                *v = cap;
            }
        }
        if !t.helicopter {
            let h = &mut cfg.presence.helicopter;
            h.min_level = NEVER_LEVEL;
            for v in [
                &mut h.count.dawn,
                &mut h.count.day,
                &mut h.count.dusk,
                &mut h.count.night,
            ] {
                *v = 0;
            }
        }
        for n in &mut cfg.presence.reinforcement.units_per_level {
            *n = (*n as f32 * self.backup_scale).round() as u32;
        }
    }

    /// Sanity checks: fractions in 0..=1, positive sizes, a lifetime for scripted tools.
    pub fn validate(&self, id: &str) -> Result<(), String> {
        let frac = |v: f32| (0.0..=1.0).contains(&v);
        if !(self.presence.is_finite() && self.presence >= 0.0) {
            return Err(format!("{id}: presence must be >= 0"));
        }
        if !(self.backup_scale.is_finite() && self.backup_scale >= 0.0) {
            return Err(format!("{id}: backup_scale must be >= 0"));
        }
        let tools = !self.route_blocks.is_empty()
            || !self.side_blocks.is_empty()
            || !self.spikes.is_empty();
        if tools && self.tool_lifetime_s <= 0.0 {
            return Err(format!("{id}: scripted tools need tool_lifetime_s > 0"));
        }
        if let Some(c) = self.speed.pursuit_cap_mps {
            if c <= 0.0 {
                return Err(format!("{id}: speed.pursuit_cap_mps must be > 0"));
            }
        }
        for b in &self.route_blocks {
            if !frac(b.at) || b.gap_m < 0.0 {
                return Err(format!("{id}: route_blocks need at in 0..=1, gap_m >= 0"));
            }
        }
        for b in &self.side_blocks {
            if !frac(b.from) || !frac(b.to) || b.from > b.to || b.count == 0 || b.into_m <= 0.0 {
                return Err(format!(
                    "{id}: side_blocks need 0 <= from <= to <= 1, count >= 1, into_m > 0"
                ));
            }
        }
        for s in &self.stakeouts {
            if !frac(s.at) || s.trigger_m <= 0.0 {
                return Err(format!("{id}: stakeouts need at in 0..=1, trigger_m > 0"));
            }
        }
        for s in &self.spikes {
            if !frac(s.at) || s.length_m <= 0.0 {
                return Err(format!("{id}: spikes need at in 0..=1, length_m > 0"));
            }
        }
        Ok(())
    }
}

/// Every race's police profile, by race id (`data/police/races.toml`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RaceProfiles {
    /// Profiles by race id.
    pub profiles: BTreeMap<String, RaceProfile>,
}

impl RaceProfiles {
    /// Parse and validate a `races.toml` text.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let p: Self =
            toml::from_str(text).map_err(|e| ConfigError::Parse("races.toml".to_string(), e))?;
        for (id, prof) in &p.profiles {
            prof.validate(id).map_err(ConfigError::Invalid)?;
        }
        Ok(p)
    }

    /// The table compiled into the crate (`data/police/races.toml`).
    pub fn builtin() -> Self {
        Self::parse(BUILTIN_RACES).expect("data/police/races.toml compiled into wp_police")
    }

    /// The profile of race `id`.
    pub fn get(&self, id: &str) -> Option<&RaceProfile> {
        self.profiles.get(id)
    }
}

/// The course a race runs, as the police need it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RaceCourse {
    /// Centreline of the planned lane in driving order, m (XY).
    pub polyline: Vec<[f32; 2]>,
    cum: Vec<f32>,
}

impl RaceCourse {
    /// Course along `polyline` (XY, driving order; consecutive duplicates are dropped).
    pub fn new(polyline: &[[f32; 2]]) -> Self {
        let mut pts: Vec<[f32; 2]> = Vec::with_capacity(polyline.len());
        for p in polyline {
            if pts.last().is_none_or(|q| dist(*q, *p) > 1e-3) {
                pts.push(*p);
            }
        }
        let mut cum = Vec::with_capacity(pts.len());
        let mut s = 0.0;
        for (i, p) in pts.iter().enumerate() {
            if i > 0 {
                s += dist(pts[i - 1], *p);
            }
            cum.push(s);
        }
        Self { polyline: pts, cum }
    }

    /// Course length, m.
    pub fn length(&self) -> f32 {
        self.cum.last().copied().unwrap_or(0.0)
    }

    fn leg(&self, s: f32) -> usize {
        let n = self.polyline.len();
        if n < 2 {
            return 0;
        }
        let s = s.clamp(0.0, self.length());
        match self.cum.binary_search_by(|c| c.total_cmp(&s)) {
            Ok(i) => i.min(n - 2),
            Err(i) => i.saturating_sub(1).min(n - 2),
        }
    }

    /// Point `s` m along the course.
    pub fn point_at(&self, s: f32) -> [f32; 2] {
        match self.polyline.len() {
            0 => [0.0, 0.0],
            1 => self.polyline[0],
            _ => {
                let i = self.leg(s);
                let (a, b) = (self.polyline[i], self.polyline[i + 1]);
                let l = self.cum[i + 1] - self.cum[i];
                let t = ((s - self.cum[i]) / l.max(1e-6)).clamp(0.0, 1.0);
                add(a, scale(sub(b, a), t))
            }
        }
    }

    /// Unit racing direction `s` m along the course.
    pub fn dir_at(&self, s: f32) -> [f32; 2] {
        if self.polyline.len() < 2 {
            return [0.0, 1.0];
        }
        let i = self.leg(s);
        normalize(sub(self.polyline[i + 1], self.polyline[i]))
    }

    /// Closest course point to `p`: `(s along the course, distance)`, m.
    pub fn project(&self, p: [f32; 2]) -> (f32, f32) {
        let mut best = (0.0, f32::INFINITY);
        for i in 0..self.polyline.len().saturating_sub(1) {
            let (t, q) = closest_on_segment(self.polyline[i], self.polyline[i + 1], p);
            let d = dist(p, q);
            if d < best.1 {
                best = (self.cum[i] + t * (self.cum[i + 1] - self.cum[i]), d);
            }
        }
        best
    }
}

/// What [`PoliceSystem::start_race`] put out.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RacePolicePlan {
    /// Roadblocks across the course.
    pub route_blocks: Vec<ToolId>,
    /// Roadblocks on side roads.
    pub side_blocks: Vec<ToolId>,
    /// Staked-out units.
    pub stakeouts: Vec<UnitId>,
    /// Spike strips.
    pub spikes: Vec<ToolId>,
}

/// A staked-out unit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stakeout {
    /// Unit.
    pub unit: UnitId,
    /// Where it waits, m.
    pub post: [f32; 2],
    /// Springs when a racer passes this close, m.
    pub trigger_m: f32,
    /// Has left its post (joined or started a chase, or was taken by dispatch).
    pub sprung: bool,
}

/// The race the police are working (see [`PoliceSystem::race_police`]).
#[derive(Debug, Clone, PartialEq)]
pub struct RacePolice {
    /// Race id (profile key).
    pub race: String,
    /// Profile in force.
    pub profile: RaceProfile,
    /// Racers in the race.
    pub entrants: Vec<EntityId>,
    /// Staked-out units.
    pub stakeouts: Vec<Stakeout>,
    /// What was placed at the start.
    pub plan: RacePolicePlan,
}

/// A side road branching off the course.
#[derive(Debug, Clone, Copy)]
struct SideRoad {
    /// Distance along the course of the junction, m.
    s: f32,
    node: NodeId,
    /// First segment of the side road, leaving `node`.
    segment: SegmentId,
}

impl PoliceSystem {
    /// Set up the police for race `race` (id) with `profile` on `course`, raced by
    /// `entrants`: apply the profile's doctrine to the police tables and place its scripted
    /// roadblocks, spike strips and stakeouts. Call once, before the countdown. Placements
    /// that find no fitting spot (no side road in range, a course too short) are skipped.
    pub fn start_race(
        &mut self,
        race: &str,
        profile: &RaceProfile,
        course: &RaceCourse,
        entrants: &[EntityId],
    ) -> RacePolicePlan {
        profile.apply(&mut self.cfg);
        let owner = entrants.first().copied().unwrap_or(EntityId(0));
        let expires = self.now.plus(profile.tool_lifetime_s);
        let mut plan = RacePolicePlan::default();
        let total = course.length();
        let sides = self.side_roads(course);
        let mut used_nodes: Vec<NodeId> = Vec::new();

        for b in &profile.route_blocks {
            if let Some(id) = self.place_route_block(course, b, owner, expires) {
                plan.route_blocks.push(id);
            }
        }
        for b in &profile.side_blocks {
            let (lo, hi) = (
                (b.from * total).max(ENDS_CLEAR_M),
                (b.to * total).min(total - 10.0),
            );
            let cands: Vec<SideRoad> = sides
                .iter()
                .filter(|r| r.s >= lo && r.s <= hi && !used_nodes.contains(&r.node))
                .copied()
                .collect();
            for r in spread(&cands, b.count as usize) {
                if let Some(id) = self.place_side_block(&r, b, owner, expires) {
                    used_nodes.push(r.node);
                    plan.side_blocks.push(id);
                }
            }
        }
        for s in &profile.spikes {
            plan.spikes
                .push(self.place_course_spikes(course, s, owner, expires));
        }
        let mut stakeouts = Vec::new();
        for st in &profile.stakeouts {
            let s = (st.at * total).clamp(0.0, total);
            let side = sides
                .iter()
                .filter(|r| (r.s - s).abs() < STAKEOUT_SIDE_ROAD_M && !used_nodes.contains(&r.node))
                .min_by(|a, b| {
                    (a.s - s)
                        .abs()
                        .total_cmp(&(b.s - s).abs())
                        .then(a.segment.0.cmp(&b.segment.0))
                })
                .copied();
            // In the side road's lane toward the junction, nose to the course; else on the
            // shoulder facing the racing direction.
            let parked = side.and_then(|r| {
                let (p, dir, _) = self.walk_road(r.node, r.segment, STAKEOUT_INTO_M)?;
                used_nodes.push(r.node);
                let face = scale(dir, -1.0);
                Some((add(p, scale([face[1], -face[0]], LANE_OFFSET_M)), face))
            });
            let (pos, heading) = parked.unwrap_or_else(|| {
                let dir = course.dir_at(s);
                let (centre, hw) = self.road_at(course.point_at(s));
                let right = [dir[1], -dir[0]];
                (add(centre, scale(right, hw + SHOULDER_CLEAR_M)), dir)
            });
            let i = self.spawn_unit(UnitKind::Car, UnitRole::Tool, pos, heading);
            let unit = self.units[i].id;
            plan.stakeouts.push(unit);
            stakeouts.push(Stakeout {
                unit,
                post: pos,
                trigger_m: st.trigger_m,
                sprung: false,
            });
        }
        self.radio(RadioCue::new(
            RadioTier::Escalation,
            None,
            format!(
                "Dispatch: street race reported, all units. Operation {}: {} roadblocks, {} spike strips, {} units staked out.",
                profile.name,
                plan.route_blocks.len() + plan.side_blocks.len(),
                plan.spikes.len(),
                plan.stakeouts.len()
            ),
        ));
        self.race = Some(RacePolice {
            race: race.to_string(),
            profile: profile.clone(),
            entrants: entrants.to_vec(),
            stakeouts,
            plan: plan.clone(),
        });
        plan
    }

    /// The race the police are working, if [`PoliceSystem::start_race`] was called.
    pub fn race_police(&self) -> Option<&RacePolice> {
        self.race.as_ref()
    }

    /// Think step for the race: spring stakeouts that a racer passes.
    pub(crate) fn race_think(&mut self, world: &dyn WorldQuery) {
        let Some(rp) = self.race.as_ref() else {
            return;
        };
        let recognise = rp.profile.recognise_on_sight;
        let entrants = rp.entrants.clone();
        let threshold = self.cfg.heat.heat.level_thresholds[0];
        for k in 0..rp.stakeouts.len() {
            let st = self.race.as_ref().expect("race").stakeouts[k];
            if st.sprung {
                continue;
            }
            let Some(i) = self.unit_index(st.unit) else {
                self.set_sprung(k);
                continue;
            };
            let u = &self.units[i];
            if u.wrecked || u.assignment != Assignment::Idle {
                // Taken by the normal rules (it recognised a racer itself): it is an ordinary
                // chase unit from now on, cleaned up like backup once done.
                self.units[i].role = UnitRole::Reinforcement;
                self.set_sprung(k);
                continue;
            }
            let (upos, ualt) = (u.body.pos, u.body.alt_m);
            let mut best: Option<(f32, EntityId)> = None;
            for e in &entrants {
                if self.held.contains(e) {
                    continue;
                }
                let Some(r) = self.racers.get(e) else {
                    continue;
                };
                let rp = xy(r.pos);
                let d = dist(upos, rp);
                if d > st.trigger_m || len([r.vel[0], r.vel[1]]) < STAKEOUT_MIN_RACER_MPS {
                    continue;
                }
                if !self.sees_at(world, upos, ualt, rp, r.pos[2]) {
                    continue;
                }
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, *e));
                }
            }
            let Some((_, racer)) = best else {
                continue;
            };
            let rec = &self.racers[&racer];
            if rec.pursued() {
                self.units[i].role = UnitRole::Reinforcement;
                self.units[i].racer_contact = false;
                self.assign(i, Assignment::Pursue { target: racer });
            } else if recognise {
                if rec.level == WantedLevel::None {
                    let heat = rec.heat.max(threshold + 1.0);
                    self.set_heat(racer, heat);
                }
                self.units[i].role = UnitRole::Reinforcement;
                self.start_pursuit(racer, st.unit);
            } else {
                // Only joins chases: the normal recognition rules apply to this racer.
                continue;
            }
            self.set_sprung(k);
        }
    }

    fn set_sprung(&mut self, k: usize) {
        if let Some(rp) = self.race.as_mut() {
            rp.stakeouts[k].sprung = true;
        }
    }

    /// Road centre and half width at the road nearest `p` (the course point itself and the
    /// default half width without a road graph).
    fn road_at(&self, p: [f32; 2]) -> ([f32; 2], f32) {
        match self.graph.nearest_on_road(p) {
            Some(r) if r.dist_m < 2.0 * ROAD_HALF_WIDTH_M => (r.point, self.half_width(r.segment)),
            _ => (p, ROAD_HALF_WIDTH_M),
        }
    }

    fn half_width(&self, s: SegmentId) -> f32 {
        let seg = self.graph.segment(s);
        let w = (seg.lanes_fwd as f32 + seg.lanes_back as f32) * seg.lane_width_m * 0.5;
        if w > 1.0 {
            w
        } else {
            ROAD_HALF_WIDTH_M
        }
    }

    /// Whether node `k` joins three or more roads (or is flagged a junction by the city).
    fn is_junction(&self, k: usize) -> bool {
        let n = NodeId(k as u32);
        let mut segs: Vec<u32> = self
            .graph
            .outgoing(n)
            .iter()
            .chain(self.graph.incoming(n))
            .map(|(s, _)| s.0)
            .collect();
        segs.sort_unstable();
        segs.dedup();
        self.graph.nodes[k].junction || segs.len() >= 3
    }

    fn near_junction(&self, p: [f32; 2], r: f32) -> bool {
        (0..self.graph.nodes.len())
            .any(|k| dist(self.graph.nodes[k].pos, p) < r && self.is_junction(k))
    }

    /// Side roads off the course: for every junction on the course, each road leaving it
    /// across the racing direction whose first stretch is not itself part of the course.
    fn side_roads(&self, course: &RaceCourse) -> Vec<SideRoad> {
        let total = course.length();
        let mut out = Vec::new();
        for (k, n) in self.graph.nodes.iter().enumerate() {
            if !self.is_junction(k) {
                continue;
            }
            let (s, d) = course.project(n.pos);
            if d > JUNCTION_ON_COURSE_M || s < ENDS_CLEAR_M || s > total {
                continue;
            }
            let node = NodeId(k as u32);
            let tangent = course.dir_at(s);
            let mut segs: Vec<SegmentId> = self
                .graph
                .outgoing(node)
                .iter()
                .chain(self.graph.incoming(node))
                .map(|(seg, _)| *seg)
                .collect();
            segs.sort_by_key(|s| s.0);
            segs.dedup();
            for seg in segs {
                let dir = self.graph.dir_from(seg, node);
                if dot(dir, tangent).abs() > SIDE_ROAD_MAX_COS {
                    continue;
                }
                // City roads are chains of short segments: walk down the side road.
                let Some((probe, _, _)) = self.walk_road(node, seg, default_into_m()) else {
                    continue;
                };
                if course.project(probe).1 < self.half_width(seg) + 4.0 {
                    continue;
                }
                out.push(SideRoad {
                    s,
                    node,
                    segment: seg,
                });
            }
        }
        out.sort_by(|a, b| a.s.total_cmp(&b.s).then(a.segment.0.cmp(&b.segment.0)));
        out
    }

    /// The point `d` m down the road that leaves `node` along `seg`, following the road
    /// through plain (non-junction) nodes: `(point, travel direction there, segment there)`.
    /// `None` when the road ends or reaches another junction first.
    fn walk_road(
        &self,
        node: NodeId,
        seg: SegmentId,
        d: f32,
    ) -> Option<([f32; 2], [f32; 2], SegmentId)> {
        let (mut from, mut seg, mut left) = (node, seg, d);
        for _ in 0..self.graph.segments.len() {
            let l = self.graph.segment(seg).length_m;
            let dir = self.graph.dir_from(seg, from);
            if left <= l {
                return Some((add(self.graph.node(from).pos, scale(dir, left)), dir, seg));
            }
            left -= l;
            let to = self.graph.other_end(seg, from);
            if self.is_junction(to.0 as usize) {
                return None;
            }
            let next = self
                .graph
                .outgoing(to)
                .iter()
                .chain(self.graph.incoming(to))
                .map(|(s, _)| *s)
                .find(|s| *s != seg)?;
            from = to;
            seg = next;
        }
        None
    }

    fn push_roadblock(
        &mut self,
        site: ToolSite,
        half_width_m: f32,
        units: u32,
        gap: Option<(GapSide, f32)>,
        owner: EntityId,
        expires: crate::time::SimTime,
    ) -> ToolId {
        let rb = &self.cfg.tools.roadblock;
        let he = rb.barrier_half_extents_m;
        let across = perp_left(site.dir);
        // Lateral offset (+ left) of a point from the block's centre.
        let lateral = |p: [f32; 2]| dot(sub(p, site.pos), across);
        let open = |x: f32, half: f32| match gap {
            Some((GapSide::Left, g)) if g > 0.0 => x + half > half_width_m - g,
            Some((GapSide::Right, g)) if g > 0.0 => x - half < -half_width_m + g,
            _ => false,
        };
        let barriers: Vec<_> = roadblock_barriers(&site, he, rb.barrier_mass_kg, half_width_m)
            .into_iter()
            .filter(|c| !open(lateral([c.pos[0], c.pos[1]]), he[0]))
            .collect();
        let id = ToolId(self.next_tool);
        self.next_tool += 1;
        let spots: Vec<_> = roadblock_unit_spots(&site, units.max(1), half_width_m)
            .into_iter()
            .filter(|(p, _)| !open(lateral(*p), 1.2))
            .take(units as usize)
            .collect();
        let mut ids = Vec::new();
        for (pos, heading) in spots {
            let i = self.spawn_unit(UnitKind::Car, UnitRole::Tool, pos, heading);
            ids.push(self.units[i].id);
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
            racer: owner,
            site,
            units: ids,
            barriers,
            expires,
            scripted: true,
        });
        self.stats.roadblocks += 1;
        self.pending.push(PoliceEvent::RoadblockPlaced {
            tool: id,
            racer: owner,
            pos: [site.pos[0], site.pos[1], 0.0],
            sight_m: site.sight_m,
        });
        id
    }

    fn place_route_block(
        &mut self,
        course: &RaceCourse,
        b: &RouteBlock,
        owner: EntityId,
        expires: crate::time::SimTime,
    ) -> Option<ToolId> {
        let total = course.length();
        let end = total - ENDS_CLEAR_M;
        let mut s = (b.at * total).max(ENDS_CLEAR_M);
        // Off the junction boxes: the first spot clear of every junction.
        while s <= end && self.near_junction(course.point_at(s), NODE_CLEARANCE_M + 5.0) {
            s += 5.0;
        }
        if s > end {
            return None;
        }
        let (centre, hw) = self.road_at(course.point_at(s));
        let seg = self
            .graph
            .nearest_on_road(centre)
            .map_or(SegmentId(0), |r| r.segment);
        let from_node = if self.graph.segments.is_empty() {
            NodeId(0)
        } else {
            self.graph.segment(seg).a
        };
        let site = ToolSite {
            segment: seg,
            from_node,
            d_m: 0.0,
            pos: centre,
            dir: course.dir_at(s),
            sight_m: 0.0,
            ahead_m: s,
        };
        Some(self.push_roadblock(
            site,
            hw,
            b.units,
            Some((b.gap_side, b.gap_m)),
            owner,
            expires,
        ))
    }

    fn place_side_block(
        &mut self,
        r: &SideRoad,
        b: &SideBlocks,
        owner: EntityId,
        expires: crate::time::SimTime,
    ) -> Option<ToolId> {
        let (pos, dir, seg) = self
            .walk_road(r.node, r.segment, b.into_m)
            .or_else(|| self.walk_road(r.node, r.segment, NODE_CLEARANCE_M))?;
        let site = ToolSite {
            segment: seg,
            from_node: r.node,
            d_m: b.into_m,
            pos,
            dir,
            sight_m: 0.0,
            ahead_m: 0.0,
        };
        let hw = self.half_width(seg);
        Some(self.push_roadblock(site, hw, b.units, None, owner, expires))
    }

    fn place_course_spikes(
        &mut self,
        course: &RaceCourse,
        sp: &SpikeSpec,
        owner: EntityId,
        expires: crate::time::SimTime,
    ) -> ToolId {
        let total = course.length();
        let s = (sp.at * total).clamp(0.0, total);
        let dir = course.dir_at(s);
        let pos = add(course.point_at(s), scale(perp_left(dir), sp.offset_m));
        let seg = self
            .graph
            .nearest_on_road(pos)
            .map_or(SegmentId(0), |r| r.segment);
        let site = ToolSite {
            segment: seg,
            from_node: NodeId(0),
            d_m: 0.0,
            pos,
            dir,
            sight_m: 0.0,
            ahead_m: s,
        };
        let id = ToolId(self.next_tool);
        self.next_tool += 1;
        let mut strip = SpikeStrip::across(id, owner, site, sp.length_m, None, expires);
        strip.scripted = true;
        self.pending.push(PoliceEvent::SpikesPlaced {
            tool: id,
            racer: owner,
            a: [strip.a[0], strip.a[1], 0.0],
            b: [strip.b[0], strip.b[1], 0.0],
            sight_m: 0.0,
        });
        self.spikes.push(strip);
        self.stats.spikes += 1;
        id
    }
}

/// `count` items spread evenly over `items` (all of them when there are not more).
fn spread<T: Copy>(items: &[T], count: usize) -> Vec<T> {
    if count == 0 || items.is_empty() {
        return Vec::new();
    }
    if items.len() <= count {
        return items.to_vec();
    }
    (0..count)
        .map(|k| items[((2 * k + 1) * items.len()) / (2 * count)])
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{District, RoadClass, RoadGraph};
    use crate::{PoliceSetup, RacerView};

    #[test]
    fn builtin_profiles_cover_the_picker_races() {
        let p = RaceProfiles::builtin();
        for id in [
            "lantern_checkpoint",
            "loop9_circuit",
            "hollins_sprint",
            "grid_highway_sprint",
        ] {
            assert!(p.get(id).is_some(), "{id} has no police profile");
        }
        // The disk copy is the compiled-in copy.
        let disk = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/police/races.toml"
        ))
        .unwrap();
        assert_eq!(RaceProfiles::parse(&disk).unwrap(), p);
    }

    #[test]
    fn daytime_is_heavy_with_roadblocks_on_and_off_the_route() {
        let p = RaceProfiles::builtin();
        let day = p.get("lantern_checkpoint").unwrap();
        assert!(day.presence >= 2.0);
        assert!(day.route_blocks.len() >= 2);
        assert!(day.side_blocks.iter().map(|b| b.count).sum::<u32>() >= 2);
        for other in ["loop9_circuit", "hollins_sprint", "grid_highway_sprint"] {
            assert!(p.get(other).unwrap().presence < day.presence);
        }
    }

    #[test]
    fn loop9_is_pursuit_only() {
        let prof = RaceProfiles::builtin()
            .get("loop9_circuit")
            .unwrap()
            .clone();
        assert!(prof.route_blocks.is_empty() && prof.side_blocks.is_empty());
        assert!(prof.spikes.is_empty());
        let mut cfg = PoliceConfig::builtin();
        prof.apply(&mut cfg);
        let u = &cfg.presence.unlocks;
        for lvl in [
            u.roadblock_level,
            u.spikes_level,
            u.boxin_level,
            u.rolling_block_level,
            cfg.pursuit.chase.junction_block_level,
            cfg.presence.helicopter.min_level,
        ] {
            assert_eq!(lvl, NEVER_LEVEL);
        }
        assert!(u.pit_level <= 5, "PIT stays");
        assert!(
            cfg.pursuit.pursuit_cap_mps.night >= 60.0,
            "highway speed chase"
        );
        assert!(cfg.pursuit.chase.pit_max_kmh[2] >= 180.0);
    }

    #[test]
    fn apply_leaves_unlisted_tables_alone() {
        let prof = RaceProfiles::builtin()
            .get("lantern_checkpoint")
            .unwrap()
            .clone();
        let base = PoliceConfig::builtin();
        let mut cfg = base.clone();
        prof.apply(&mut cfg);
        assert_eq!(cfg.presence.unlocks, base.presence.unlocks);
        assert_eq!(cfg.pursuit.chase, base.pursuit.chase);
        assert_eq!(cfg.presence.patrols, base.presence.patrols);
        // Backup × 1.5.
        assert_eq!(cfg.presence.reinforcement.units_per_level[1], 6);
    }

    #[test]
    fn validate_rejects_bad_tables() {
        let ok = RaceProfiles::builtin()
            .get("lantern_checkpoint")
            .unwrap()
            .clone();
        let mut p = ok.clone();
        p.route_blocks[0].at = 1.5;
        assert!(p.validate("x").is_err());
        let mut p = ok.clone();
        p.tool_lifetime_s = 0.0;
        assert!(p.validate("x").is_err());
        let mut p = ok;
        p.side_blocks[0].from = 0.9;
        p.side_blocks[0].to = 0.1;
        assert!(p.validate("x").is_err());
    }

    /// A straight east–west avenue 0..800 m with a cross street every 200 m.
    fn avenue() -> RoadGraph {
        let mut g = RoadGraph::new();
        let mut main = Vec::new();
        for k in 0..=4 {
            let x = k as f32 * 200.0;
            main.push(g.add_node([x, 0.0], District::Core));
        }
        for w in main.windows(2) {
            g.add_segment(w[0], w[1], 14.0, RoadClass::Arterial, false);
        }
        for &n in &main[1..4] {
            let p = g.node(n).pos;
            let north = g.add_node([p[0], 150.0], District::Core);
            let south = g.add_node([p[0], -150.0], District::Core);
            g.add_segment(n, north, 11.0, RoadClass::Local, false);
            g.add_segment(n, south, 11.0, RoadClass::Local, false);
        }
        g
    }

    fn system() -> PoliceSystem {
        PoliceSystem::with_city(PoliceConfig::builtin(), avenue(), PoliceSetup::default())
    }

    fn course() -> RaceCourse {
        RaceCourse::new(&[[0.0, -1.75], [800.0, -1.75]])
    }

    #[test]
    fn course_geometry() {
        let c = course();
        assert!((c.length() - 800.0).abs() < 1e-3);
        assert_eq!(c.point_at(100.0), [100.0, -1.75]);
        assert_eq!(c.dir_at(400.0), [1.0, 0.0]);
        let (s, d) = c.project([300.0, 10.0]);
        assert!((s - 300.0).abs() < 1e-3 && (d - 11.75).abs() < 1e-3);
    }

    #[test]
    fn scripted_placements_land_on_and_off_the_course() {
        let mut police = system();
        let prof = RaceProfiles::builtin()
            .get("lantern_checkpoint")
            .unwrap()
            .clone();
        let c = course();
        let racers = [EntityId(1), EntityId(2)];
        let plan = police.start_race("lantern_checkpoint", &prof, &c, &racers);
        assert_eq!(plan.route_blocks.len(), 2);
        assert_eq!(plan.side_blocks.len(), 4);
        assert_eq!(plan.stakeouts.len(), 2);
        for rb in police.roadblocks() {
            assert!(rb.scripted);
            let (_, d) = c.project(rb.site.pos);
            if plan.route_blocks.contains(&rb.id) {
                // Across the avenue, clear of the junctions, facing the racing direction.
                assert!(d < 3.0, "route block {d:.1} m off the course");
                assert!(dot(rb.site.dir, [1.0, 0.0]) > 0.99);
                assert!(police
                    .graph()
                    .nodes
                    .iter()
                    .all(|n| dist(n.pos, rb.site.pos) >= 15.0));
                // The gap: no barrier within gap_m of one edge.
                let hw = ROAD_HALF_WIDTH_M;
                let left: Vec<f32> = rb
                    .barriers
                    .iter()
                    .map(|b| b.pos[1] - rb.site.pos[1])
                    .collect();
                let open_left = left.iter().all(|y| *y + 1.2 <= hw - 3.5 + 1e-3);
                let open_right = left.iter().all(|y| *y - 1.2 >= -hw + 3.5 - 1e-3);
                assert!(open_left ^ open_right, "one side open: {left:?}");
            } else {
                // Down a cross street, across it.
                assert!(d > 15.0, "side block only {d:.1} m off the course");
                assert!(dot(rb.site.dir, [1.0, 0.0]).abs() < 0.1);
            }
        }
        // Scripted tools stay when a racer's pursuit ends.
        police.release_units(EntityId(1));
        assert_eq!(police.roadblocks().len(), 6);
        assert!(police.race_police().is_some());
    }

    #[test]
    fn stakeout_starts_a_pursuit_when_a_racer_flies_past() {
        let mut police = system();
        let prof = RaceProfiles::builtin()
            .get("lantern_checkpoint")
            .unwrap()
            .clone();
        let c = course();
        let racer = EntityId(7);
        let plan = police.start_race("lantern_checkpoint", &prof, &c, &[racer]);
        let unit = plan.stakeouts[0];
        let post = police.race_police().unwrap().stakeouts[0].post;
        let world = wp_world::World::flat(wp_contracts::TimeOfDay::from_hours(14.0), 1.0);
        let mut out = Vec::new();
        // Far away: nothing happens.
        let far = [RacerView {
            entity: racer,
            pos: [post[0] - 400.0, -1.75, 0.0],
            vel: [30.0, 0.0, 0.0],
        }];
        for _ in 0..30 {
            police.step(&world, &far, &[], 1.0 / 60.0, &mut out);
        }
        assert!(!police.racer(racer).unwrap().pursued());
        // Flying past the stakeout: recognised at once, the stakeout gives chase.
        let near = [RacerView {
            entity: racer,
            pos: [post[0] - 20.0, -1.75, 0.0],
            vel: [30.0, 0.0, 0.0],
        }];
        for _ in 0..30 {
            police.step(&world, &near, &[], 1.0 / 60.0, &mut out);
        }
        let rec = police.racer(racer).unwrap();
        assert!(rec.pursued(), "{:?}", rec.status);
        assert!(rec.level >= WantedLevel::L1);
        let u = police.units().iter().find(|u| u.id == unit).unwrap();
        assert_eq!(u.assignment, Assignment::Pursue { target: racer });
        assert_eq!(u.role, UnitRole::Reinforcement);
    }

    #[test]
    fn spikes_cover_part_of_the_course_only() {
        let mut police = system();
        let prof = RaceProfiles::builtin()
            .get("grid_highway_sprint")
            .unwrap()
            .clone();
        let c = course();
        let plan = police.start_race("grid_highway_sprint", &prof, &c, &[EntityId(1)]);
        assert_eq!(plan.spikes.len(), 1);
        let s = &police.spike_strips()[0];
        assert!(s.scripted);
        assert!((dist(s.a, s.b) - prof.spikes[0].length_m).abs() < 1e-3);
        // A car in the next lane (3.5 m left of the course line) misses it.
        let sp = prof.spikes[0];
        let x = sp.at * c.length();
        let lane = 3.5 + sp.offset_m;
        let hit = s.wheels_crossing([x - 5.0, -1.75 + lane], [x + 5.0, -1.75 + lane], 0.8);
        assert_eq!(hit, [false; 4]);
        let hit = s.wheels_crossing([x - 5.0, -1.75], [x + 5.0, -1.75], 0.8);
        assert!(hit.iter().any(|w| *w));
    }

    #[test]
    fn spread_is_even_and_deterministic() {
        let v: Vec<u32> = (0..10).collect();
        assert_eq!(spread(&v, 2), vec![2, 7]);
        assert_eq!(spread(&v, 4), vec![1, 3, 6, 8]);
        assert_eq!(spread(&v[..3], 5), vec![0, 1, 2]);
        assert!(spread(&v, 0).is_empty());
    }
}
