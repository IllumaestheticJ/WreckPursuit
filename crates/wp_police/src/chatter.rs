//! Unit-to-unit radio callouts (owner playtest row 79: "cops don't talk with each other").
//!
//! Units and dispatch say what is actually happening in the chase, each line led by a call
//! sign and placed on the map ("Unit 12, I've got eyes on him again, northbound on Harbor
//! Boulevard at Lantern Way."):
//!
//! Tiers are the row-144 priority tiers ([`RadioTier`], 1 = most important):
//!
//! | Callout | When | Tier |
//! |---|---|---|
//! | "racer #7: roadblock 400 metres ahead, northbound on Lantern Row" | `RoadblockPlaced` / `SpikesPlaced` (the decision, not the arrival) | 1 |
//! | dispatch level summary: level, place, units on him, air unit, a tougher department / rams authorised | a pursued racer's level rises | 2 (newer replaces older) |
//! | racer down to level n | a pursued racer's level drops | 2 |
//! | hit / rammed / lost my front end / I'm done (by impact severity) | a physical unit's contact ([`impact_severity`]) | 3 |
//! | "Unit 4 is down, unit 4 is down!" / "officer down, need EMS!" | another unit reacting to a wreck | 3 (stacks after the victim) |
//! | he took out a civilian | a pursued racer's contact with a civilian car | 3 |
//! | going for the PIT, he's spun / he got away | PIT tactic; [`PIT_VERDICT_S`] after the contact | 3 |
//! | spikes hit | a racer runs over a strip | 3 |
//! | unit down / out of the chase | `UnitDisabled` (shouted, by cause) | 3 |
//! | units responding (once) | a racer's level first rises above 0 with no pursuit yet | 4 |
//! | lost visual + last seen place and heading | pursuit → searching for [`LOST_CONFIRM_S`] | 4 |
//! | eyes on him again | searching → pursuit after a lost-visual call | 4 |
//! | on his bumper | a pursuer [`BUMPER_M`] behind a moving racer for [`BUMPER_HOLD_S`] | 4 |
//! | boxing him in | two or more pursuers in box-in slots | 4 |
//! | gone into the tunnel | a pursued racer enters a covered road | 4 |
//!
//! The row-79 "requesting backup" call is a deployment line and is no longer spoken (row
//! 144: deployment lines only when actionable).
//!
//! Dispatch's own lines (pursuit start, roadblocks, custody, pursuit
//! terminated) are raised where they happen and use the same place phrases
//! ([`PoliceSystem::place_phrase`]). Everything goes through the radio director
//! ([`crate::radio`]): one transmission at a time, paced by wanted level.
//!
//! **Real information.** The lost-visual callout reads the racer record's `last_known` /
//! `last_seen`, the same fields search and interception (`pursuit.rs`, machine E's search
//! points) steer to, and the units sent to search say where they are going ("Unit 5, copy,
//! searching near …", from their intercept point). The callouts describe that shared
//! knowledge; they never change it.
//!
//! **Observer only.** Callouts are derived once per police step from the step's police events,
//! the sim's contacts and (on think ticks) state diffs. Nothing here feeds back into the
//! simulation or is hashed. Line variety (which of several shouted lines) comes from the
//! chatter's own seeded RNG ([`Chatter::new`]: the police seed, forked), never the
//! simulation's. Sim time only.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::{EntityId, SimEvent, WorldQuery};

use crate::events::{DisableCause, PoliceEvent, UnitId, UnitKind};
use crate::graph::{closest_on_segment, dist, dot, len, normalize, sub, xy, District, NodeId};
use crate::heat::PursuitStatus;
use crate::radio::{ImpactConfig, RadioCue, RadioTier};
use crate::time::SimTime;
use crate::tools::compass;
use crate::unit::{Assignment, Tactic};
use crate::PoliceSystem;
use wp_math::Rng;

/// Searching this long before a unit calls "lost visual" (line-of-sight flicker stays quiet), s.
pub const LOST_CONFIRM_S: f32 = 1.5;
/// Lost this long, the lost-visual call goes out even within [`SIGHT_CALL_GAP_S`], s.
pub const LOST_SUSTAINED_S: f32 = 4.5;
/// At most one lost-visual / eyes-on call per racer per this long, s.
pub const SIGHT_CALL_GAP_S: f32 = 10.0;
/// A pursuer this close behind the racer is "on his bumper", m.
pub const BUMPER_M: f32 = 14.0;
/// ... for this long, s.
pub const BUMPER_HOLD_S: f32 = 1.5;
/// The racer must be moving at least this fast for a bumper call, m/s.
pub const BUMPER_MIN_SPEED_MPS: f32 = 8.0;
/// One bumper call per unit per this long, s.
pub const BUMPER_UNIT_GAP_S: f32 = 25.0;
/// One bumper call per racer per this long, s.
pub const BUMPER_RACER_GAP_S: f32 = 12.0;
/// The PIT verdict is called this long after the contact, s.
pub const PIT_VERDICT_S: f32 = 2.5;
/// One PIT line-up call per unit per this long, s.
pub const PIT_UNIT_GAP_S: f32 = 15.0;
/// One box-in / backup / spikes-hit call per racer per this long, s.
pub const RACER_CALL_GAP_S: f32 = 20.0;
/// Street names are used when the point is within this distance of the road centreline, m.
const ON_ROAD_M: f32 = 40.0;
/// "at <cross street>" within this distance of a junction, m.
const AT_JUNCTION_M: f32 = 30.0;

/// The kinds of callout ([`ChatterStats`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Callout {
    /// "I've got eyes on him again."
    EyesOn,
    /// "Lost visual, last seen …"
    LostVisual,
    /// "Requesting backup."
    BackupRequest,
    /// "I'm on his bumper."
    Bumper,
    /// "Going for the PIT."
    PitLineUp,
    /// "He's spun!"
    PitSpun,
    /// "Negative on the PIT, he got away."
    PitMissed,
    /// "Boxing him in."
    BoxIn,
    /// "Spikes hit!"
    SpikesHit,
    /// "I'm out of the chase."
    UnitDown,
    /// "He's gone into the tunnel."
    Tunnel,
    /// Dispatch: "racer #3 is now level 4, … 3 units on him."
    LevelSummary,
    /// A tougher department / rams authorised, added to a level summary.
    Takeover,
    /// Dispatch: "racer #3 down to level 2."
    LevelDown,
    /// Dispatch: "units responding" (the one low-heat deployment summary).
    UnitsResponding,
    /// Dispatch: "racer #3: roadblock 400 metres ahead, northbound on X."
    RoadblockAhead,
    /// Dispatch: "racer #3: spikes 250 metres ahead, northbound on X."
    SpikesAhead,
    /// A unit's shouted impact line ("I'm hit!", "I've lost my front end!").
    Impact,
    /// Another unit reacting to a wreck ("Unit 4 is down, unit 4 is down!").
    WreckReaction,
    /// "He just took out a civilian!"
    CivilianCrash,
}

impl Callout {
    /// Every callout, in a fixed order.
    pub const ALL: [Callout; 20] = [
        Callout::EyesOn,
        Callout::LostVisual,
        Callout::BackupRequest,
        Callout::Bumper,
        Callout::PitLineUp,
        Callout::PitSpun,
        Callout::PitMissed,
        Callout::BoxIn,
        Callout::SpikesHit,
        Callout::UnitDown,
        Callout::Tunnel,
        Callout::LevelSummary,
        Callout::Takeover,
        Callout::LevelDown,
        Callout::UnitsResponding,
        Callout::RoadblockAhead,
        Callout::SpikesAhead,
        Callout::Impact,
        Callout::WreckReaction,
        Callout::CivilianCrash,
    ];
}

/// Callouts raised so far (raised, not necessarily spoken: the radio director may drop them).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatterStats {
    /// Raised count per kind.
    pub raised: BTreeMap<Callout, u32>,
    /// Deployment lines not spoken (row 144: units joining, responding to a known call, backup
    /// requests, tool cars arriving, intercepts behind the racer).
    pub deploy_suppressed: u32,
}

impl ChatterStats {
    /// Raised count of one kind.
    pub fn count(&self, c: Callout) -> u32 {
        self.raised.get(&c).copied().unwrap_or(0)
    }
}

/// A covered road (tunnel, covered trench) for "he's gone into the tunnel" calls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tunnel {
    /// Display name, e.g. `Hollins Tunnel`.
    pub name: String,
    /// Centreline portal to portal, m.
    pub centre: Vec<[f32; 2]>,
    /// Centreline to wall, m.
    pub half_width_m: f32,
}

impl Tunnel {
    /// Whether `p` is inside the covered stretch (portals included).
    pub fn contains(&self, p: [f32; 2]) -> bool {
        self.centre.windows(2).any(|w| {
            let (_, q) = closest_on_segment(w[0], w[1], p);
            dist(p, q) <= self.half_width_m
        })
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
struct RacerChat {
    status: PursuitStatus,
    /// Wanted level at the last think step.
    level: u8,
    searching_since: Option<SimTime>,
    lost_called: bool,
    last_sight_call: Option<SimTime>,
    /// Racer velocity the last time it was seen, m/s.
    seen_vel: [f32; 2],
    /// Closest in-service ground unit on the racer while it was seen.
    spotter: Option<UnitId>,
    tunnel: Option<usize>,
    last_bumper: Option<SimTime>,
    last_boxin: Option<SimTime>,
    boxers: usize,
    last_backup: Option<SimTime>,
    last_spikes: Option<SimTime>,
    /// The low-heat "units responding" summary went out for this rise of heat.
    summarized: bool,
    last_civilian: Option<SimTime>,
}

#[derive(Debug, Clone, Default, PartialEq)]
struct UnitChat {
    tactic: Option<Tactic>,
    /// Racer it was pursuing / intercepting at the last think step.
    target: Option<EntityId>,
    bumper_since: Option<SimTime>,
    last_bumper: Option<SimTime>,
    last_pit_call: Option<SimTime>,
    /// Last impact line about this unit: when and how bad.
    last_impact: Option<(SimTime, ImpactSeverity)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PitWatch {
    unit: UnitId,
    racer: EntityId,
    at: SimTime,
    dir0: [f32; 2],
}

/// Callout state ([`PoliceSystem`] holds one). See the module docs.
#[derive(Debug, Clone, PartialEq)]
pub struct Chatter {
    /// Display name per road-graph edge (`RoadEdgeId`); `""` = unnamed.
    names: Vec<String>,
    tunnels: Vec<Tunnel>,
    racers: BTreeMap<EntityId, RacerChat>,
    units: BTreeMap<UnitId, UnitChat>,
    pits: Vec<PitWatch>,
    /// Reinforcement requests already called in: (racer, requested at).
    backups: Vec<(EntityId, SimTime)>,
    stats: ChatterStats,
    /// Line variety only (which shouted line). Never the simulation's RNG.
    rng: Rng,
}

impl Default for Chatter {
    fn default() -> Self {
        Self::new(0)
    }
}

impl Chatter {
    /// Empty callout state; `seed` = the police seed (the variety RNG is derived from it, so
    /// a replay picks the same lines).
    pub fn new(seed: u64) -> Self {
        Self {
            names: Vec::new(),
            tunnels: Vec::new(),
            racers: BTreeMap::new(),
            units: BTreeMap::new(),
            pits: Vec::new(),
            backups: Vec::new(),
            stats: ChatterStats::default(),
            rng: Rng::new(seed ^ 0x5241_4449_4F5F_3134),
        }
    }

    fn pick<'a>(&mut self, pool: &[&'a str]) -> &'a str {
        pool[self.rng.below(pool.len() as u32) as usize]
    }
}

/// How bad a unit's impact was (row 144: the line is picked by severity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ImpactSeverity {
    /// A light tap: no line.
    Tap,
    /// A real hit: "I'm hit! I'm hit!"
    Hit,
    /// A hard hit or a badly damaged car: "He rammed me, I've lost my front end!"
    Heavy,
    /// The car is wrecked: "I'm done!", and another unit: "Unit 4 is down!"
    Wreck,
}

/// Severity of a contact from the sim's energy and the unit's damage: collision energy
/// `½ · impulse · closing speed` (J, or a nominal [`ImpactConfig::nominal_mass_kg`] car when
/// the contact carries no impulse) against the config's thresholds, raised to `Heavy` when
/// the unit's health is at or below `heavy_health`. Only a wrecked unit is a `Wreck`.
pub fn impact_severity(
    cfg: &ImpactConfig,
    rel_speed_mps: f32,
    impulse_ns: f32,
    health: f32,
    wrecked: bool,
) -> ImpactSeverity {
    if wrecked {
        return ImpactSeverity::Wreck;
    }
    let v = rel_speed_mps.abs();
    let energy = if impulse_ns > 0.0 {
        0.5 * impulse_ns * v
    } else {
        0.5 * cfg.nominal_mass_kg * v * v
    };
    // Energy alone tops out at Heavy: only a car that is really out (the damage model wrecked
    // it) may sound like "officer down", or it would say so and then keep driving.
    let by_energy = if energy >= cfg.heavy_j {
        ImpactSeverity::Heavy
    } else if energy >= cfg.hit_j {
        ImpactSeverity::Hit
    } else {
        ImpactSeverity::Tap
    };
    if by_energy > ImpactSeverity::Tap && health <= cfg.heavy_health {
        by_energy.max(ImpactSeverity::Heavy)
    } else {
        by_energy
    }
}

/// Who an impact line is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImpactRole {
    /// The racer hit this unit.
    Victim,
    /// This unit rammed or PITed its target on purpose.
    Rammer,
    /// This unit crashed on its own (a wall, a civilian car).
    SelfCrash,
}

/// Shouted impact lines, `{cs}` = call sign, `{place}` = place phrase. `None` for a tap: a
/// light touch is never on the radio, and only a wreck can be "officer down".
pub fn impact_lines(sev: ImpactSeverity, role: ImpactRole) -> Option<&'static [&'static str]> {
    use ImpactRole::*;
    use ImpactSeverity::*;
    Some(match (sev, role) {
        (Tap, _) => return None,
        (Hit, Victim) => &[
            "{cs}, I'm hit! I'm hit!",
            "{cs}, he hit me! I'm still in it!",
            "{cs}, contact! He slammed into me {place}!",
        ],
        (Heavy, Victim) => &[
            "{cs}, he rammed me, I've lost my front end!",
            "{cs}, I'm hit hard! Steering's gone, steering's gone!",
            "{cs}, he took me out {place}! I'm smoking bad!",
        ],
        (Hit, Rammer) => &[
            "{cs}, contact! I'm on him!",
            "{cs}, I've got him, I tagged him {place}!",
        ],
        (Heavy, Rammer) => &[
            "{cs}, rammed him hard! He's all over the road!",
            "{cs}, big hit on the suspect {place}! He's sliding!",
        ],
        (Hit, SelfCrash) => &[
            "{cs}, I clipped something, still on him!",
            "{cs}, whoa! Clipped a car, I'm OK!",
        ],
        (Heavy, SelfCrash) => &[
            "{cs}, I'm into the wall! I'm into the wall!",
            "{cs}, lost it {place}! I'm hurt bad, unit's hurt bad!",
        ],
        (Wreck, _) => &[
            "{cs}, I'm done! I'm done! Unit's totalled {place}!",
            "{cs}, I'm wrecked! I'm out, I'm out!",
            "{cs}, officer down {place}! Need EMS!",
        ],
    })
}

/// Queue priority of both lines of a big wreck (the victim, then the reaction). Above every
/// other impact line, so a wreck is never crowded out by PIT or bent-wheel calls; both lines
/// share it, so the older one (the victim) goes first and its reaction right after.
pub const WRECK_PRIO: u8 = 5;

/// Another unit reacting to a wreck: `{other}` = its call sign, `{cs}` = the wrecked unit's,
/// `{place}` = where.
pub const WRECK_REACTIONS: [&str; 3] = [
    "{other}, {cs} is down, {cs} is down!",
    "{other}, officer down {place}, need EMS now!",
    "{other}, {cs} just got wrecked {place}! Get EMS rolling!",
];

/// Dispatch reacting to a wreck when no other unit is on the racer.
pub const WRECK_REACTION_DISPATCH: &str = "Dispatch: officer down {place}! EMS rolling, all units!";

/// A pursued racer hitting a civilian car: `{cs}` = the lead unit (or Dispatch).
pub const CIVILIAN_LINES: [&str; 3] = [
    "{cs}, he just took out a civilian {place}!",
    "{cs}, he plowed into a civilian car {place}! Need EMS!",
    "{cs}, civilian vehicle hit {place}, he's still going!",
];

/// Spoken distance: nearest 50 m (100 m from a kilometre on).
pub fn round_m(m: f32) -> u32 {
    let step = if m >= 1000.0 { 100.0 } else { 50.0 };
    (((m / step).round() * step) as u32).max(50)
}

/// Name the roads and tunnels of `world` for callouts (called by [`PoliceSystem::for_world`]).
pub fn attach_world(p: &mut PoliceSystem, world: &wp_world::World) {
    p.set_road_names(world.edge_names());
    p.set_tunnels(
        world
            .network()
            .covered
            .iter()
            .map(|c| Tunnel {
                name: c.name.clone(),
                centre: c.centre.iter().map(|q| [q[0], q[1]]).collect(),
                half_width_m: c.half_width_m,
            })
            .collect(),
    );
}

fn district_words(d: District) -> &'static str {
    match d {
        District::Core => "downtown",
        District::Suburb => "in the suburbs",
        District::Industrial => "in the industrial district",
    }
}

impl PoliceSystem {
    /// Display names per road-graph edge (`RoadEdgeId` order, `""` = unnamed) used in radio
    /// place phrases. [`PoliceSystem::for_world`] sets them from `World::edge_names`.
    pub fn set_road_names(&mut self, names: Vec<String>) {
        self.chatter.names = names;
    }

    /// Covered roads for "he's gone into the tunnel" calls ([`PoliceSystem::for_world`] sets
    /// them from the world's covered spans).
    pub fn set_tunnels(&mut self, tunnels: Vec<Tunnel>) {
        self.chatter.tunnels = tunnels;
    }

    /// Callouts raised so far, per kind.
    pub fn chatter_stats(&self) -> &ChatterStats {
        &self.chatter.stats
    }

    /// Radio call sign of a unit: "Unit 12", "Air 9" ("Dispatch" if it is gone).
    pub fn callsign(&self, id: UnitId) -> String {
        match self.unit_index(id).map(|i| self.units[i].kind) {
            Some(UnitKind::Car) => format!("Unit {}", id.0),
            Some(UnitKind::Helicopter) => format!("Air {}", id.0),
            None => "Dispatch".to_string(),
        }
    }

    /// Display name of the road under `p` (within 40 m of its centreline), if it has one.
    pub fn road_name_at(&self, p: [f32; 2]) -> Option<&str> {
        let rp = self.graph.nearest_on_road(p)?;
        if rp.dist_m > ON_ROAD_M {
            return None;
        }
        let edge = self.graph.segment(rp.segment).edge?;
        self.chatter
            .names
            .get(edge as usize)
            .map(|n| n.as_str())
            .filter(|n| !n.is_empty())
    }

    /// A named road other than `name` meeting at node `n`, in edge order.
    fn cross_street(&self, n: NodeId, name: &str) -> Option<&str> {
        let mut edges: Vec<u32> = self
            .graph
            .outgoing(n)
            .iter()
            .chain(self.graph.incoming(n).iter())
            .filter_map(|(s, _)| self.graph.segment(*s).edge)
            .collect();
        edges.sort_unstable();
        edges
            .into_iter()
            .filter_map(|e| self.chatter.names.get(e as usize))
            .map(|s| s.as_str())
            .find(|s| !s.is_empty() && *s != name)
    }

    /// Where something is, for the radio: "northbound on Harbor Boulevard at Lantern Way",
    /// "on Ridge Road" (slower than 2 m/s: no heading), "westbound in the Hollins Tunnel", or
    /// the district when the road has no name ("northbound downtown").
    pub fn place_phrase(&self, p: [f32; 2], vel: [f32; 2]) -> String {
        let heading = if len(vel) >= 2.0 {
            format!("{} ", compass(vel))
        } else {
            String::new()
        };
        if let Some(t) = self.chatter.tunnels.iter().find(|t| t.contains(p)) {
            return format!("{heading}in the {}", t.name);
        }
        if let Some(name) = self.road_name_at(p) {
            let rp = self.graph.nearest_on_road(p).expect("named road");
            let seg = self.graph.segment(rp.segment);
            let (a, b) = (seg.a, seg.b);
            let near = if dist(p, self.graph.node(a).pos) <= dist(p, self.graph.node(b).pos) {
                a
            } else {
                b
            };
            if dist(p, self.graph.node(near).pos) <= AT_JUNCTION_M {
                if let Some(cross) = self.cross_street(near, name) {
                    return format!("{heading}on {name} at {cross}");
                }
            }
            return format!("{heading}on {name}");
        }
        format!("{heading}{}", district_words(self.graph.district_at(p)))
    }

    /// "ETA 40 seconds" for unit `i` driving to `p` (straight line × 1.3 at its road speed),
    /// `None` when it is close (under 150 m) or a helicopter.
    pub(crate) fn eta_phrase(&self, i: usize, p: [f32; 2]) -> Option<String> {
        let u = &self.units[i];
        let d = dist(u.body.pos, p);
        if u.kind != UnitKind::Car || d < 150.0 {
            return None;
        }
        let eta = d * 1.3 / u.limit_mps.max(15.0);
        let rounded = ((eta / 10.0).round() * 10.0).max(10.0);
        Some(if rounded >= 90.0 {
            format!("ETA {:.0} minutes", (eta / 60.0).round().max(2.0))
        } else {
            format!("ETA {rounded:.0} seconds")
        })
    }

    /// What changes for the units when a racer goes from level `from` to `to` (row 144,
    /// tier 2): the rams sentence from the pursuit config's ram levels, then the highest
    /// "tougher department" sentence crossed (`radio.toml` `takeover`). Empty if nothing.
    pub(crate) fn takeover_phrase(&self, from: u8, to: u8) -> String {
        let ch = &self.cfg.pursuit.chase;
        let crossed = |l: u8| from < l && to >= l;
        let mut parts: Vec<String> = Vec::new();
        if let Some(t) = (from + 1..=to.min(5))
            .rev()
            .map(|l| &self.cfg.radio.takeover[l as usize - 1])
            .find(|t| !t.is_empty())
        {
            parts.push(t.clone());
        }
        if crossed(ch.hard_ram_level) {
            parts.push("Hard rams and rolling blocks authorised.".into());
        } else if crossed(ch.rear_ram_level) {
            parts.push("Rams authorised.".into());
        }
        parts.join(" ")
    }

    /// Dispatch's summary when a pursued racer's wanted level rises: level, where, who is on it.
    pub(crate) fn level_summary(&self, racer: EntityId, level: u8) -> String {
        let r = &self.racers[&racer];
        let place = self.place_phrase(xy(r.pos), [r.vel[0], r.vel[1]]);
        let on_it = |kind: UnitKind| {
            self.units
                .iter()
                .filter(|u| {
                    u.kind == kind
                        && !u.wrecked
                        && matches!(
                            u.assignment,
                            Assignment::Pursue { target }
                                | Assignment::Intercept { target, .. }
                                | Assignment::HeloTrack { target } if target == racer
                        )
                })
                .count()
        };
        let cars = on_it(UnitKind::Car);
        let units = match cars {
            0 => "No units on him yet".to_string(),
            1 => "One unit on him".to_string(),
            n => format!("{n} units on him"),
        };
        let air = if on_it(UnitKind::Helicopter) > 0 {
            ", air unit overhead"
        } else {
            ""
        };
        format!(
            "Dispatch, all units: racer #{} is now level {level}, {place}. {units}{air}.",
            racer.0
        )
    }

    /// Last seen place and heading of a racer, from the shared racer record (`last_known`)
    /// and its velocity when last seen.
    pub(crate) fn last_seen_phrase(&self, racer: EntityId) -> String {
        let r = &self.racers[&racer];
        let p = r.last_known.unwrap_or(xy(r.pos));
        let v = self
            .chatter
            .racers
            .get(&racer)
            .map(|c| c.seen_vel)
            .unwrap_or([r.vel[0], r.vel[1]]);
        self.place_phrase(p, v)
    }

    fn callout(&mut self, kind: Callout, cue: RadioCue) {
        *self.chatter.stats.raised.entry(kind).or_insert(0) += 1;
        self.radio(cue);
    }

    /// A deployment line was not spoken (row 144); counted for the bench.
    pub(crate) fn radio_deploy_suppressed(&mut self) {
        self.chatter.stats.deploy_suppressed += 1;
    }

    /// Another code path raised an impact line for `unit` this step (the assault line): no
    /// second one from its contacts.
    pub(crate) fn chatter_note_impact_line(&mut self, unit: UnitId) {
        let now = self.now;
        let c = self.chatter.units.entry(unit).or_default();
        c.last_impact = Some((now, ImpactSeverity::Heavy));
    }

    /// A pursuit started: the low-heat summary is no longer needed for this racer.
    pub(crate) fn chatter_pursuit_started(&mut self, racer: EntityId) {
        self.chatter.racers.entry(racer).or_default().summarized = true;
    }

    /// Fill an impact template.
    fn fill(&self, t: &str, cs: &str, place: &str) -> String {
        t.replace("{cs}", cs).replace("{place}", place)
    }

    /// Tier 1 (row 144): roadblocks and spikes are announced when they are planned, with the
    /// place and the distance ahead of the racer, so the driver can act on them. Tools planned
    /// in the same step for the same racer (spikes and a roadblock behind them) share one line:
    /// the second would otherwise wait behind the first and arrive too late.
    fn chatter_tools(&mut self, tools: &[(bool, crate::events::ToolId, EntityId)]) {
        let mut racers: Vec<EntityId> = tools.iter().map(|t| t.2).collect();
        racers.dedup();
        for racer in racers {
            let Some(r) = self.racers.get(&racer) else {
                continue;
            };
            let (rp, rv) = (xy(r.pos), [r.vel[0], r.vel[1]]);
            let mut parts: Vec<(f32, String)> = Vec::new();
            let mut kinds: Vec<Callout> = Vec::new();
            for &(roadblock, tool, _) in tools.iter().filter(|t| t.2 == racer) {
                let site = if roadblock {
                    self.roadblocks
                        .iter()
                        .find(|r| r.id == tool)
                        .map(|r| r.site)
                } else {
                    self.spikes
                        .iter()
                        .find(|s| s.id == tool)
                        .map(|s| s.site)
                        .or_else(|| {
                            self.spike_runs
                                .iter()
                                .find(|r| r.strip.id == tool)
                                .map(|r| r.strip.site)
                        })
                };
                let Some(site) = site else {
                    continue;
                };
                let ahead_m = if site.ahead_m > 0.0 {
                    site.ahead_m
                } else {
                    dist(rp, site.pos)
                };
                let ahead = len(rv) < 2.0 || dot(normalize(rv), sub(site.pos, rp)) >= 0.0;
                let place = self.place_phrase(site.pos, rv);
                let m = round_m(ahead_m);
                let rel = if ahead { "ahead" } else { "behind" };
                let what = if roadblock { "roadblock" } else { "spikes" };
                parts.push((ahead_m, format!("{what} {m} metres {rel}, {place}")));
                kinds.push(if roadblock {
                    Callout::RoadblockAhead
                } else {
                    Callout::SpikesAhead
                });
            }
            if parts.is_empty() {
                continue;
            }
            // Nearest first.
            parts.sort_by(|a, b| a.0.total_cmp(&b.0));
            let nearest = parts[0].0;
            let list: Vec<String> = parts.into_iter().map(|p| p.1).collect();
            // Short: the line holds the channel while the racer closes in.
            let text = format!("Dispatch, racer #{}: {}.", racer.0, list.join("; "));
            for k in &kinds[1..] {
                *self.chatter.stats.raised.entry(*k).or_insert(0) += 1;
            }
            // Only news while the racer has not reached it: life ≈ time to get there.
            let eta = nearest / len(rv).max(5.0);
            let ttl = (eta - 2.0).clamp(2.0, 12.0);
            self.callout(
                kinds[0],
                RadioCue::new(RadioTier::Actionable, None, text)
                    .prio(crate::radio::TOOL_WARNING_PRIO)
                    .ttl(ttl),
            );
        }
    }

    /// Tier 3 (row 144): physical units' contacts, worst per unit this step, as shouted lines
    /// picked by [`impact_severity`]; pursued racers hitting civilian cars.
    fn chatter_impacts(&mut self, events: &[SimEvent]) {
        let now = self.now;
        let icfg = self.cfg.radio.impact.clone();
        // (unit index, other entity, rel speed, impulse): worst per unit.
        let mut worst: BTreeMap<UnitId, (usize, EntityId, f32, f32)> = BTreeMap::new();
        let mut civ: Vec<(EntityId, f32, f32)> = Vec::new();
        for ev in events {
            let SimEvent::Contact {
                a,
                b,
                impulse_ns,
                rel_speed,
                ..
            } = ev
            else {
                continue;
            };
            for (x, y) in [(*a, *b), (*b, *a)] {
                if let Some(i) = self.unit_index_by_entity(x) {
                    let u = &self.units[i];
                    if u.kind != UnitKind::Car || u.player.is_some() {
                        continue;
                    }
                    let e = 0.5 * impulse_ns * rel_speed.abs();
                    let w = worst.entry(u.id).or_insert((i, y, *rel_speed, *impulse_ns));
                    if e > 0.5 * w.3 * w.2.abs() {
                        *w = (i, y, *rel_speed, *impulse_ns);
                    }
                } else if self.racers.contains_key(&x)
                    && y != wp_contracts::WORLD_ENTITY
                    && !self.racers.contains_key(&y)
                    && self.unit_index_by_entity(y).is_none()
                {
                    civ.push((x, *rel_speed, *impulse_ns));
                }
            }
        }
        for (id, (i, other, v, imp)) in worst {
            let u = &self.units[i];
            let sev = impact_severity(&icfg, v, imp, u.health, u.wrecked);
            if sev == ImpactSeverity::Tap {
                continue;
            }
            let racer_hit = self.racers.contains_key(&other);
            let meant = racer_hit
                && u.assignment.target() == Some(other)
                && matches!(u.tactic, Tactic::Pit | Tactic::Ram(_));
            // PIT contacts get their verdict line instead.
            if meant && u.tactic == Tactic::Pit && sev < ImpactSeverity::Wreck {
                continue;
            }
            let role = if meant {
                ImpactRole::Rammer
            } else if racer_hit {
                ImpactRole::Victim
            } else {
                ImpactRole::SelfCrash
            };
            // A unit with nobody to chase does not narrate its own scrapes ("still on him!"
            // after the arrest); a wreck is still called in by `unit_down`.
            if role == ImpactRole::SelfCrash && u.assignment.target().is_none() {
                continue;
            }
            let last = self.chatter.units.get(&id).and_then(|c| c.last_impact);
            if last.is_some_and(|(t, s)| now.since(t) < icfg.unit_gap_s && sev <= s) {
                continue;
            }
            self.chatter.units.entry(id).or_default().last_impact = Some((now, sev));
            let pos = u.body.pos;
            let target = u.assignment.target();
            self.impact_callout(id, sev, role, pos, target);
        }
        for (racer, v, imp) in civ {
            let pursued = self.racers.get(&racer).is_some_and(|r| r.pursued());
            let sev = impact_severity(&icfg, v, imp, 1.0, false);
            if !pursued || sev == ImpactSeverity::Tap {
                continue;
            }
            let c = self.chatter.racers.entry(racer).or_default();
            if c.last_civilian
                .is_some_and(|t| now.since(t) < icfg.civilian_gap_s)
            {
                continue;
            }
            c.last_civilian = Some(now);
            let lead = self.lead_unit(racer);
            let cs = lead.map_or("Dispatch".to_string(), |u| self.callsign(u));
            let r = &self.racers[&racer];
            let place = self.place_phrase(xy(r.pos), [r.vel[0], r.vel[1]]);
            let t = self.chatter.pick(&CIVILIAN_LINES);
            let text = self.fill(t, &cs, &place);
            self.callout(
                Callout::CivilianCrash,
                RadioCue::new(RadioTier::Impact, lead, text)
                    .prio(if sev >= ImpactSeverity::Heavy { 3 } else { 1 }),
            );
        }
    }

    /// Raise the impact line for unit `id` (and, for a wreck of a unit on a racer, another
    /// unit's reaction stacked right after it).
    fn impact_callout(
        &mut self,
        id: UnitId,
        sev: ImpactSeverity,
        role: ImpactRole,
        pos: [f32; 2],
        target: Option<EntityId>,
    ) {
        let Some(pool) = impact_lines(sev, role) else {
            return;
        };
        let cs = self.callsign(id);
        let place = self.place_phrase(pos, [0.0, 0.0]);
        let t = self.chatter.pick(pool);
        let text = self.fill(t, &cs, &place);
        // A worse impact stays news longer (it may wait behind tier 1–2 traffic).
        let (prio, ttl) = match sev {
            ImpactSeverity::Wreck => (WRECK_PRIO, 8.0),
            ImpactSeverity::Heavy => (3, 6.0),
            _ => (1, self.cfg.radio.impact_stale_s),
        };
        self.callout(
            Callout::Impact,
            RadioCue::new(RadioTier::Impact, Some(id), text)
                .prio(prio)
                .ttl(ttl),
        );
        if sev == ImpactSeverity::Wreck {
            self.wreck_reaction(id, &cs, &place, target);
        }
    }

    /// Another unit (or dispatch) reacts to unit `id`'s wreck: stacked after the victim's line
    /// when nothing more important waits (short life).
    fn wreck_reaction(&mut self, id: UnitId, cs: &str, place: &str, target: Option<EntityId>) {
        let other = target.and_then(|r| {
            self.units
                .iter()
                .filter(|u| {
                    u.id != id
                        && u.kind == UnitKind::Car
                        && !u.wrecked
                        && u.assignment.target() == Some(r)
                })
                .min_by(|a, b| {
                    let rp = self.racers.get(&r).map_or(a.body.pos, |x| xy(x.pos));
                    dist(a.body.pos, rp).total_cmp(&dist(b.body.pos, rp))
                })
                .map(|u| u.id)
        });
        let text = match other {
            Some(o) => {
                let ocs = self.callsign(o);
                let t = self.chatter.pick(&WRECK_REACTIONS);
                t.replace("{other}", &ocs)
                    .replace("{cs}", cs)
                    .replace("{place}", place)
            }
            None => WRECK_REACTION_DISPATCH.replace("{place}", place),
        };
        self.callout(
            Callout::WreckReaction,
            RadioCue::new(RadioTier::Impact, other, text)
                .prio(WRECK_PRIO)
                // It goes after its victim line (and any tier 1 traffic that cuts in), so it
                // lives longer than the victim line's 8 s.
                .ttl(12.0),
        );
    }

    /// Closest in-service ground unit pursuing `racer` (any in-service ground unit on it if
    /// none pursues).
    fn lead_unit(&self, racer: EntityId) -> Option<UnitId> {
        let p = xy(self.racers.get(&racer)?.pos);
        let mut best: Option<(u8, f32, UnitId)> = None;
        for u in &self.units {
            if u.kind != UnitKind::Car || u.wrecked {
                continue;
            }
            let rank = match u.assignment {
                Assignment::Pursue { target } if target == racer => 0,
                Assignment::Intercept { target, .. } if target == racer => 1,
                _ => continue,
            };
            let d = dist(u.body.pos, p);
            if best.is_none_or(|b| (rank, d) < (b.0, b.1)) {
                best = Some((rank, d, u.id));
            }
        }
        best.map(|b| b.2)
    }

    /// Derive this step's callouts. `thought`: a think step ran (state diffs are taken then).
    pub(crate) fn chatter_step(
        &mut self,
        _world: &dyn WorldQuery,
        events: &[SimEvent],
        thought: bool,
    ) {
        if self.graph.nodes.is_empty() {
            return;
        }
        self.chatter_events(events);
        self.pit_verdicts();
        if thought {
            self.chatter_racers();
            self.chatter_units();
            self.chatter_backup();
        }
    }

    /// Police events of this step and the sim's contacts.
    fn chatter_events(&mut self, events: &[SimEvent]) {
        let now = self.now;
        let mut pits: Vec<(UnitId, EntityId)> = Vec::new();
        let mut downs: Vec<(UnitId, DisableCause)> = Vec::new();
        let mut spikes: Vec<EntityId> = Vec::new();
        let mut tools: Vec<(bool, crate::events::ToolId, EntityId)> = Vec::new();
        for e in &self.pending {
            match e {
                PoliceEvent::PitAttempt { unit, target } => pits.push((*unit, *target)),
                PoliceEvent::UnitDisabled { unit, cause } => downs.push((*unit, *cause)),
                PoliceEvent::SpikeHit { vehicle, .. } if self.racers.contains_key(vehicle) => {
                    spikes.push(*vehicle)
                }
                PoliceEvent::RoadblockPlaced { tool, racer, .. } => {
                    tools.push((true, *tool, *racer))
                }
                // Announced at the decision already (row 165: laid later, with its car).
                PoliceEvent::SpikesPlaced { tool, .. } if self.spikes_announced.contains(tool) => {}
                PoliceEvent::SpikesPlaced { tool, racer, .. } => tools.push((false, *tool, *racer)),
                _ => {}
            }
        }
        for (tool, racer) in std::mem::take(&mut self.spikes_decided) {
            self.spikes_announced.insert(tool);
            tools.push((false, tool, racer));
        }
        let (laid, runs) = (&self.spikes, &self.spike_runs);
        self.spikes_announced
            .retain(|t| laid.iter().any(|s| s.id == *t) || runs.iter().any(|r| r.strip.id == *t));
        self.chatter_tools(&tools);
        self.chatter_impacts(events);
        // Physical units: a contact between a unit lining up a PIT and its target.
        for ev in events {
            if let SimEvent::Contact { a, b, .. } = ev {
                for (x, y) in [(*a, *b), (*b, *a)] {
                    if let Some(i) = self.unit_index_by_entity(x) {
                        let u = &self.units[i];
                        if u.tactic == Tactic::Pit
                            && matches!(u.assignment, Assignment::Pursue { target } if target == y)
                        {
                            pits.push((u.id, y));
                        }
                    }
                }
            }
        }
        for (unit, racer) in pits {
            if self.chatter.pits.iter().any(|w| w.unit == unit) {
                continue;
            }
            let Some(r) = self.racers.get(&racer) else {
                continue;
            };
            self.chatter.pits.push(PitWatch {
                unit,
                racer,
                at: now,
                dir0: normalize([r.vel[0], r.vel[1]]),
            });
        }
        for (unit, cause) in downs {
            self.unit_down(unit, cause);
        }
        for racer in spikes {
            let c = self.chatter.racers.entry(racer).or_default();
            if c.last_spikes
                .is_some_and(|t| now.since(t) < RACER_CALL_GAP_S)
            {
                continue;
            }
            c.last_spikes = Some(now);
            let speaker = self
                .units
                .iter()
                .find(|u| matches!(u.assignment, Assignment::SpikeAt { .. }))
                .map(|u| u.id)
                .or_else(|| self.lead_unit(racer));
            let r = &self.racers[&racer];
            let place = self.place_phrase(xy(r.pos), [r.vel[0], r.vel[1]]);
            let cs = speaker.map_or("Dispatch".to_string(), |u| self.callsign(u));
            self.callout(
                Callout::SpikesHit,
                RadioCue::new(
                    RadioTier::Impact,
                    speaker,
                    format!("{cs}, spikes hit! Spikes hit! He's losing tires {place}!"),
                )
                .prio(3)
                .ttl(5.0),
            );
        }
    }

    /// Call the result of PIT contacts [`PIT_VERDICT_S`] later.
    fn pit_verdicts(&mut self) {
        let now = self.now;
        let due: Vec<PitWatch> = self
            .chatter
            .pits
            .iter()
            .copied()
            .filter(|w| now.since(w.at) >= PIT_VERDICT_S)
            .collect();
        self.chatter
            .pits
            .retain(|w| now.since(w.at) < PIT_VERDICT_S);
        for w in due {
            let Some(r) = self.racers.get(&w.racer) else {
                continue;
            };
            if !r.pursued() || self.unit_index(w.unit).is_none() {
                continue;
            }
            let v = [r.vel[0], r.vel[1]];
            let turned = dot(normalize(v), w.dir0) < 0.5;
            let place = self.place_phrase(xy(r.pos), v);
            let cs = self.callsign(w.unit);
            if len(v) < 5.0 || turned {
                self.callout(
                    Callout::PitSpun,
                    RadioCue::new(
                        RadioTier::Impact,
                        Some(w.unit),
                        format!("{cs}, he's spun! He's spun! Suspect is spun out {place}!"),
                    )
                    .prio(3)
                    .ttl(4.0),
                );
            } else {
                self.callout(
                    Callout::PitMissed,
                    RadioCue::new(
                        RadioTier::Impact,
                        Some(w.unit),
                        format!("{cs}, negative on the PIT, he got away {place}."),
                    )
                    .prio(0)
                    .ttl(3.0),
                );
            }
        }
    }

    /// A unit is out of action (row 144: shouted, tier 3). A wreck of a unit that was on a
    /// racer also gets another unit's reaction.
    fn unit_down(&mut self, unit: UnitId, cause: DisableCause) {
        let Some(i) = self.unit_index(unit) else {
            return;
        };
        if self.units[i].kind != UnitKind::Car {
            return;
        }
        let now = self.now;
        let target = self
            .chatter
            .units
            .get(&unit)
            .and_then(|c| c.target)
            .or(self.units[i].assignment.target());
        let cs = self.callsign(unit);
        let pos = self.units[i].body.pos;
        let place = self.place_phrase(pos, [0.0, 0.0]);
        let wreck = matches!(cause, DisableCause::SelfCrash | DisableCause::RacerContact);
        let last = self.chatter.units.get(&unit).and_then(|c| c.last_impact);
        let already = last.is_some_and(|(t, s)| {
            now.since(t) < self.cfg.radio.impact.unit_gap_s && s == ImpactSeverity::Wreck
        });
        self.chatter.units.entry(unit).or_default().last_impact =
            Some((now, ImpactSeverity::Wreck));
        if wreck {
            if !already {
                let pool =
                    impact_lines(ImpactSeverity::Wreck, ImpactRole::Victim).expect("wreck lines");
                let t = self.chatter.pick(pool);
                let text = self.fill(t, &cs, &place);
                self.callout(
                    Callout::UnitDown,
                    RadioCue::new(RadioTier::Impact, Some(unit), text)
                        .prio(WRECK_PRIO)
                        .ttl(8.0),
                );
                // A big wreck always stacks a second line: the nearest unit on the same
                // racer, or dispatch.
                self.wreck_reaction(unit, &cs, &place, target);
            }
            return;
        }
        let why = match cause {
            DisableCause::BentWheel => "bent wheel! Bent wheel",
            _ => "unit's too beat up, I'm losing it",
        };
        let text = match target {
            Some(_) => format!("{cs}, {why}! I'm out of the chase {place}!"),
            None => format!("{cs}, {why}! Out of service {place}!"),
        };
        self.callout(
            Callout::UnitDown,
            RadioCue::new(RadioTier::Impact, Some(unit), text)
                .prio(3)
                .ttl(6.0),
        );
    }

    /// Racer state diffs: lost visual, eyes on again, tunnel, box-in.
    fn chatter_racers(&mut self) {
        let now = self.now;
        let ids: Vec<EntityId> = self.racers.keys().copied().collect();
        for id in ids {
            let (status, pursued, pos, vel, level) = {
                let r = &self.racers[&id];
                (
                    r.status,
                    r.pursued(),
                    xy(r.pos),
                    [r.vel[0], r.vel[1]],
                    r.level.number(),
                )
            };
            let seen = self.obs.get(&id).is_some_and(|o| o.seen);
            let lead = self.lead_unit(id);
            let spotter = self.obs.get(&id).and_then(|o| o.spotter).or(lead);
            let tunnel = self.chatter.tunnels.iter().position(|t| t.contains(pos));
            let boxers = self
                .units
                .iter()
                .filter(|u| {
                    !u.wrecked
                        && matches!(u.tactic, Tactic::BoxIn(_))
                        && matches!(u.assignment, Assignment::Pursue { target } if target == id)
                })
                .count();
            let mut calls: Vec<(Callout, RadioCue)> = Vec::new();
            let from_level = self.chatter.racers.get(&id).map_or(0, |c| c.level);
            {
                let c = self.chatter.racers.entry(id).or_default();
                if seen {
                    if len(vel) >= 2.0 {
                        c.seen_vel = vel;
                    }
                    if spotter.is_some() {
                        c.spotter = spotter;
                    }
                }
                let was_pursued =
                    matches!(c.status, PursuitStatus::Pursuit | PursuitStatus::Searching);
                let level_up = pursued && was_pursued && level > c.level;
                let level_down = pursued && was_pursued && level < c.level && level > 0;
                c.level = level;
                if level_up {
                    calls.push((
                        Callout::LevelSummary,
                        RadioCue::new(RadioTier::Escalation, None, String::new())
                            .supersedes(format!("level:{}", id.0)),
                    ));
                }
                if level_down {
                    calls.push((
                        Callout::LevelDown,
                        RadioCue::new(RadioTier::Escalation, None, String::new())
                            .ttl(8.0)
                            .supersedes(format!("level:{}", id.0)),
                    ));
                }
                if !pursued {
                    // Row 144: at L0–L1 with no pursuit, one "units responding" summary when
                    // heat first rises, then nothing about deployments.
                    let summarize = level > 0 && !c.summarized;
                    let summarized = (c.summarized || summarize) && level > 0;
                    *c = RacerChat {
                        status,
                        level,
                        seen_vel: c.seen_vel,
                        last_sight_call: c.last_sight_call,
                        summarized,
                        last_civilian: c.last_civilian,
                        ..Default::default()
                    };
                    if summarize && from_level == 0 {
                        calls.push((
                            Callout::UnitsResponding,
                            RadioCue::new(RadioTier::Investigation, None, String::new())
                                .ttl(10.0)
                                .supersedes(format!("summary:{}", id.0)),
                        ));
                    }
                    if calls.is_empty() {
                        continue;
                    }
                }
                if status == PursuitStatus::Searching && c.status != PursuitStatus::Searching {
                    c.searching_since = Some(now);
                }
                c.status = status;
                let quiet = |c: &RacerChat| {
                    c.last_sight_call
                        .is_none_or(|t| now.since(t) >= SIGHT_CALL_GAP_S)
                };
                // Lost visual, confirmed. A loss that lasts [`LOST_SUSTAINED_S`] is called even
                // inside the gap (on main's rebuilt grid, interceptors on a parallel road saw the
                // racer on and off; the flicker used up the gap and the real loss was called
                // 11.5 s late, a moment before the escape).
                let lost_for = c.searching_since.map_or(0.0, |t| now.since(t));
                if status == PursuitStatus::Searching
                    && !c.lost_called
                    && lost_for >= LOST_CONFIRM_S
                    && (quiet(c) || lost_for >= LOST_SUSTAINED_S)
                {
                    c.lost_called = true;
                    c.last_sight_call = Some(now);
                    calls.push((
                        Callout::LostVisual,
                        RadioCue::new(RadioTier::Investigation, c.spotter, String::new())
                            .prio(5)
                            .ttl(12.0)
                            .supersedes(format!("sight:{}", id.0)),
                    ));
                }
                // Eyes on again after a lost-visual call.
                if status == PursuitStatus::Pursuit && c.lost_called {
                    c.lost_called = false;
                    c.searching_since = None;
                    c.last_sight_call = Some(now);
                    // Not aired yet: drop the lost-visual call instead of "eyes on again".
                    if !self.radio_director.withdraw(&format!("sight:{}", id.0)) {
                        calls.push((
                            Callout::EyesOn,
                            RadioCue::new(RadioTier::Investigation, spotter, String::new())
                                .prio(5)
                                .ttl(8.0)
                                .supersedes(format!("sight:{}", id.0)),
                        ));
                    }
                }
                if status == PursuitStatus::Pursuit {
                    c.searching_since = None;
                }
                // Into a tunnel.
                if tunnel.is_some() && tunnel != c.tunnel {
                    calls.push((
                        Callout::Tunnel,
                        RadioCue::new(RadioTier::Investigation, c.spotter, String::new())
                            .prio(4)
                            .ttl(5.0),
                    ));
                }
                c.tunnel = tunnel;
                // Box-in formed.
                if boxers >= 2
                    && c.boxers < 2
                    && c.last_boxin
                        .is_none_or(|t| now.since(t) >= RACER_CALL_GAP_S)
                {
                    c.last_boxin = Some(now);
                    calls.push((
                        Callout::BoxIn,
                        RadioCue::new(RadioTier::Investigation, lead, String::new()).ttl(4.0),
                    ));
                }
                c.boxers = boxers;
            }
            for (kind, mut cue) in calls {
                let cs = cue
                    .unit
                    .map_or("Dispatch".to_string(), |u| self.callsign(u));
                cue.text = match kind {
                    Callout::LostVisual => format!(
                        "{cs}, lost visual. Last seen {}.",
                        self.last_seen_phrase(id)
                    ),
                    Callout::EyesOn => format!(
                        "{cs}, I've got eyes on him again, {}.",
                        self.place_phrase(pos, vel)
                    ),
                    Callout::Tunnel => {
                        let t = &self.chatter.tunnels[tunnel.expect("tunnel")];
                        let heading = if len(vel) >= 2.0 {
                            format!(", {}", compass(vel))
                        } else {
                            String::new()
                        };
                        format!("{cs}, he's gone into the {}{heading}!", t.name)
                    }
                    Callout::BoxIn => {
                        format!("{cs}, boxing him in, {}.", self.place_phrase(pos, vel))
                    }
                    Callout::LevelSummary => {
                        let more = self.takeover_phrase(from_level, level);
                        if more.is_empty() {
                            self.level_summary(id, level)
                        } else {
                            *self
                                .chatter
                                .stats
                                .raised
                                .entry(Callout::Takeover)
                                .or_insert(0) += 1;
                            format!("{} {more}", self.level_summary(id, level))
                        }
                    }
                    Callout::LevelDown => format!(
                        "Dispatch, all units: racer #{} down to level {level}, {}.",
                        id.0,
                        self.place_phrase(pos, vel)
                    ),
                    Callout::UnitsResponding => format!(
                        "Dispatch: units responding, racer #{} {}.",
                        id.0,
                        self.place_phrase(pos, vel)
                    ),
                    _ => unreachable!("racer callouts only"),
                };
                self.callout(kind, cue);
            }
        }
    }

    /// Unit state diffs: bumper, PIT line-up; remembers each unit's target for unit-down calls.
    fn chatter_units(&mut self) {
        let now = self.now;
        let mut calls: Vec<(Callout, RadioCue)> = Vec::new();
        for k in 0..self.units.len() {
            let u = &self.units[k];
            let id = u.id;
            let target = match u.assignment {
                Assignment::Pursue { target } | Assignment::Intercept { target, .. } => {
                    Some(target)
                }
                _ => None,
            };
            let pursuing = match u.assignment {
                Assignment::Pursue { target } => Some(target),
                _ => None,
            };
            let car = u.kind == UnitKind::Car && !u.wrecked && u.player.is_none();
            let tactic = u.tactic;
            let upos = u.body.pos;
            // Bumper: close behind a moving racer.
            let close_behind = pursuing.filter(|_| car).and_then(|racer| {
                let r = self.racers.get(&racer)?;
                let (rp, rv) = (xy(r.pos), [r.vel[0], r.vel[1]]);
                let behind = dot(normalize(rv), sub(upos, rp)) < 0.0;
                (len(rv) >= BUMPER_MIN_SPEED_MPS && behind && dist(upos, rp) <= BUMPER_M)
                    .then_some((racer, rp, rv))
            });
            let racer_bumper_ok = |c: &BTreeMap<EntityId, RacerChat>, r: EntityId| {
                c.get(&r)
                    .and_then(|x| x.last_bumper)
                    .is_none_or(|t| now.since(t) >= BUMPER_RACER_GAP_S)
            };
            let cs = self.callsign(id);
            let mut place_for: Option<([f32; 2], [f32; 2])> = None;
            let mut kind: Option<Callout> = None;
            {
                let c = self.chatter.units.entry(id).or_default();
                c.target = target;
                match close_behind {
                    Some((racer, rp, rv)) => {
                        let since = *c.bumper_since.get_or_insert(now);
                        if now.since(since) >= BUMPER_HOLD_S
                            && c.last_bumper
                                .is_none_or(|t| now.since(t) >= BUMPER_UNIT_GAP_S)
                            && racer_bumper_ok(&self.chatter.racers, racer)
                        {
                            let c = self.chatter.units.get_mut(&id).expect("unit chat");
                            c.last_bumper = Some(now);
                            self.chatter.racers.entry(racer).or_default().last_bumper = Some(now);
                            place_for = Some((rp, rv));
                            kind = Some(Callout::Bumper);
                        }
                    }
                    None => c.bumper_since = None,
                }
                let c = self.chatter.units.get_mut(&id).expect("unit chat");
                // PIT line-up.
                if car
                    && pursuing.is_some()
                    && tactic == Tactic::Pit
                    && c.tactic != Some(Tactic::Pit)
                    && c.last_pit_call
                        .is_none_or(|t| now.since(t) >= PIT_UNIT_GAP_S)
                    && kind.is_none()
                {
                    c.last_pit_call = Some(now);
                    kind = Some(Callout::PitLineUp);
                }
                c.tactic = Some(tactic);
            }
            match kind {
                Some(Callout::Bumper) => {
                    let (rp, rv) = place_for.expect("bumper place");
                    calls.push((
                        Callout::Bumper,
                        RadioCue::new(
                            RadioTier::Investigation,
                            Some(id),
                            format!("{cs}, I'm on his bumper, {}.", self.place_phrase(rp, rv)),
                        )
                        .prio(1)
                        .ttl(2.5),
                    ));
                }
                Some(Callout::PitLineUp) => calls.push((
                    Callout::PitLineUp,
                    RadioCue::new(
                        RadioTier::Impact,
                        Some(id),
                        format!("{cs}, going for the PIT!"),
                    )
                    .prio(1)
                    .ttl(2.5),
                )),
                _ => {}
            }
        }
        let alive: Vec<UnitId> = self.units.iter().map(|u| u.id).collect();
        self.chatter
            .units
            .retain(|id, _| alive.binary_search(id).is_ok());
        for (kind, cue) in calls {
            self.callout(kind, cue);
        }
    }

    /// New reinforcement requests: the lead unit asks for backup (dispatch's "copy" follows).
    fn chatter_backup(&mut self) {
        let now = self.now;
        let reqs: Vec<(EntityId, SimTime)> = self
            .reinforcements
            .iter()
            .map(|r| (r.racer, r.requested))
            .collect();
        self.chatter.backups.retain(|b| reqs.contains(b));
        let mut asked: Vec<EntityId> = Vec::new();
        for (racer, at) in reqs {
            if self.chatter.backups.contains(&(racer, at)) {
                continue;
            }
            self.chatter.backups.push((racer, at));
            if asked.contains(&racer) {
                continue;
            }
            asked.push(racer);
            let c = self.chatter.racers.entry(racer).or_default();
            if c.last_backup
                .is_some_and(|t| now.since(t) < RACER_CALL_GAP_S)
            {
                continue;
            }
            c.last_backup = Some(now);
            // Row 144: a deployment line, not spoken (still counted as raised).
            *self
                .chatter
                .stats
                .raised
                .entry(Callout::BackupRequest)
                .or_insert(0) += 1;
            self.chatter.stats.deploy_suppressed += 1;
        }
    }
}
