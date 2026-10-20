//! Unit-to-unit radio callouts (owner playtest row 79: "cops don't talk with each other").
//!
//! Units and dispatch say what is actually happening in the chase, each line led by a call
//! sign and placed on the map ("Unit 12, I've got eyes on him again, northbound on Harbor
//! Boulevard at Lantern Way."):
//!
//! | Callout | When | Tier |
//! |---|---|---|
//! | lost visual + last seen place and heading | pursuit → searching for [`LOST_CONFIRM_S`] | Critical |
//! | eyes on him again | searching → pursuit after a lost-visual call | Critical (8 s life) |
//! | requesting backup | a reinforcement request appears for the racer | Tactical (prio 2) |
//! | on his bumper | a pursuer [`BUMPER_M`] behind a moving racer for [`BUMPER_HOLD_S`] | Tactical (2.5 s life) |
//! | going for the PIT | a pursuer switches to the PIT tactic | Tactical (2.5 s life) |
//! | he's spun / he got away | [`PIT_VERDICT_S`] after a PIT contact: racer stopped or turned ≥ 60° | Tactical |
//! | boxing him in | two or more pursuers in box-in slots | Tactical |
//! | spikes hit | a racer runs over a strip | Tactical (prio 4) |
//! | unit down / out of the chase | `UnitDisabled` of a unit that was on a racer | Critical |
//! | gone into the tunnel | a pursued racer enters a covered road | Tactical (prio 4) |
//! | dispatch level summary: level, place, units on him, air unit | a pursued racer's level rises | Critical (newer replaces older) |
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
//! simulation, uses the RNG or is hashed. Sim time only.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::{EntityId, SimEvent, WorldQuery};

use crate::events::{DisableCause, PoliceEvent, UnitId, UnitKind};
use crate::graph::{closest_on_segment, dist, dot, len, normalize, sub, xy, District, NodeId};
use crate::heat::PursuitStatus;
use crate::radio::{RadioCue, RadioTier};
use crate::time::SimTime;
use crate::tools::compass;
use crate::unit::{Assignment, Tactic};
use crate::PoliceSystem;

/// Searching this long before a unit calls "lost visual" (line-of-sight flicker stays quiet), s.
pub const LOST_CONFIRM_S: f32 = 1.5;
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
}

impl Callout {
    /// Every callout, in a fixed order.
    pub const ALL: [Callout; 12] = [
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
    ];
}

/// Callouts raised so far (raised, not necessarily spoken: the radio director may drop them).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatterStats {
    /// Raised count per kind.
    pub raised: BTreeMap<Callout, u32>,
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
}

#[derive(Debug, Clone, Default, PartialEq)]
struct UnitChat {
    tactic: Option<Tactic>,
    /// Racer it was pursuing / intercepting at the last think step.
    target: Option<EntityId>,
    bumper_since: Option<SimTime>,
    last_bumper: Option<SimTime>,
    last_pit_call: Option<SimTime>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PitWatch {
    unit: UnitId,
    racer: EntityId,
    at: SimTime,
    dir0: [f32; 2],
}

/// Callout state ([`PoliceSystem`] holds one). See the module docs.
#[derive(Debug, Clone, Default, PartialEq)]
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

    /// Where a road-graph segment is ("on Harbor Boulevard", "downtown").
    pub(crate) fn segment_place(&self, s: crate::graph::SegmentId) -> String {
        let seg = self.graph.segment(s);
        let mid = crate::graph::lerp2(self.graph.node(seg.a).pos, self.graph.node(seg.b).pos, 0.5);
        self.place_phrase(mid, [0.0, 0.0])
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
        for e in &self.pending {
            match e {
                PoliceEvent::PitAttempt { unit, target } => pits.push((*unit, *target)),
                PoliceEvent::UnitDisabled { unit, cause } => downs.push((*unit, *cause)),
                PoliceEvent::SpikeHit { vehicle, .. } if self.racers.contains_key(vehicle) => {
                    spikes.push(*vehicle)
                }
                _ => {}
            }
        }
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
                    RadioTier::Tactical,
                    speaker,
                    format!("{cs}, spikes hit! He's losing tires {place}."),
                )
                .prio(4)
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
                        RadioTier::Tactical,
                        Some(w.unit),
                        format!("{cs}, he's spun! Suspect is spun out {place}."),
                    )
                    .prio(4)
                    .ttl(4.0),
                );
            } else {
                self.callout(
                    Callout::PitMissed,
                    RadioCue::new(
                        RadioTier::Tactical,
                        Some(w.unit),
                        format!("{cs}, negative on the PIT, he got away {place}."),
                    )
                    .prio(1)
                    .ttl(4.0),
                );
            }
        }
    }

    fn unit_down(&mut self, unit: UnitId, cause: DisableCause) {
        let Some(i) = self.unit_index(unit) else {
            return;
        };
        if self.units[i].kind != UnitKind::Car {
            return;
        }
        let target = self.chatter.units.get(&unit).and_then(|c| c.target);
        let cs = self.callsign(unit);
        let place = self.place_phrase(self.units[i].body.pos, [0.0, 0.0]);
        let why = match cause {
            DisableCause::SelfCrash | DisableCause::RacerContact => "my unit's wrecked",
            DisableCause::Damaged => "my unit's too damaged to continue",
            DisableCause::BentWheel => "I've got a bent wheel",
        };
        let (tier, text) = match target {
            Some(_) => (
                RadioTier::Critical,
                format!("{cs}, {why} {place}. I'm out of the chase."),
            ),
            None => (
                RadioTier::Routine,
                format!("{cs}, {why} {place}. Out of service."),
            ),
        };
        self.callout(
            Callout::UnitDown,
            RadioCue::new(tier, Some(unit), text).prio(3),
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
                c.level = level;
                if level_up {
                    calls.push((
                        Callout::LevelSummary,
                        RadioCue::new(RadioTier::Critical, None, String::new())
                            .supersedes(format!("level:{}", id.0)),
                    ));
                }
                if !pursued {
                    *c = RacerChat {
                        status,
                        level,
                        seen_vel: c.seen_vel,
                        last_sight_call: c.last_sight_call,
                        ..Default::default()
                    };
                    continue;
                }
                if status == PursuitStatus::Searching && c.status != PursuitStatus::Searching {
                    c.searching_since = Some(now);
                }
                c.status = status;
                let quiet = |c: &RacerChat| {
                    c.last_sight_call
                        .is_none_or(|t| now.since(t) >= SIGHT_CALL_GAP_S)
                };
                // Lost visual, confirmed.
                if status == PursuitStatus::Searching
                    && !c.lost_called
                    && c.searching_since
                        .is_some_and(|t| now.since(t) >= LOST_CONFIRM_S)
                    && quiet(c)
                {
                    c.lost_called = true;
                    c.last_sight_call = Some(now);
                    calls.push((
                        Callout::LostVisual,
                        RadioCue::new(RadioTier::Critical, c.spotter, String::new())
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
                            RadioCue::new(RadioTier::Critical, spotter, String::new())
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
                        RadioCue::new(RadioTier::Tactical, c.spotter, String::new())
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
                        RadioCue::new(RadioTier::Tactical, lead, String::new()).ttl(4.0),
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
                    Callout::LevelSummary => self.level_summary(id, level),
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
                            RadioTier::Tactical,
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
                        RadioTier::Tactical,
                        Some(id),
                        format!("{cs}, going for the PIT."),
                    )
                    .prio(2)
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
            let Some(lead) = self.lead_unit(racer) else {
                continue;
            };
            let Some(r) = self.racers.get(&racer) else {
                continue;
            };
            let place = self.place_phrase(xy(r.pos), [r.vel[0], r.vel[1]]);
            let cs = self.callsign(lead);
            self.callout(
                Callout::BackupRequest,
                RadioCue::new(
                    RadioTier::Tactical,
                    Some(lead),
                    format!("{cs}, requesting backup, {place}."),
                )
                .prio(2),
            );
        }
    }
}
