//! Strategic containment: police predict where a racer can get to in the next 20–60 s, park
//! units on the key **escape points** ahead of it while one or two units stay on its tail, and
//! after losing it run a **widening search** of likely hiding spots.
//!
//! Tables: `data/police/strategy.toml` ([`crate::config::StrategyFile`]).
//!
//! **Escape points** ([`ChokeIndex`], built once per road graph on first use). Roads are
//! grouped into *chains*: the run of segments between two junctions or dead ends (a curved
//! road is one chain of many short segments). A chain is an escape point when
//! - blocking it makes the way between its two ends at least `min_detour_extra_m` (and
//!   `min_detour_ratio` × its own length) longer
//!   ([`ChokeKind::Crossing`]: a bridge, a tunnel, a causeway, the only link between two
//!   districts). The detour is a distance-bounded shortest path that ignores one-way rules
//!   (a fleeing racer does too); or
//! - it is a highway chain that meets surface streets at one of its ends ([`ChokeKind::Ramp`]:
//!   an on-ramp), scoring at least `ramp_score_m`.
//!
//! The road graph has no bridge or tunnel flags: what makes them escape points is exactly that
//! there is no short way around, so the index finds them from the topology alone.
//!
//! **Containment** (every `replan_s`, per racer in `Pursuit` with posts at its level, moving
//! faster than `min_racer_speed_mps`):
//! 1. *Forecast*: travel times from the racer's road position over the whole graph (speed
//!    limit × `racer_limit_factor`, at least `racer_min_mps`; turning around costs
//!    `turnaround_s`), up to `horizon_s[level]`.
//! 2. *Candidates*: escape points the racer can reach between `min_racer_eta_s` and the
//!    horizon, scored by detour length (earlier ones score up to half more).
//! 3. *Tail*: the `tail_units[level]` units closest to the racer keep chasing.
//! 4. *Posts*: the best `posts_per_level[level]` candidates get the committed unit that can
//!    be there soonest, if it arrives `arrive_margin_s` before the racer could. A unit already
//!    holding a post that is still a candidate keeps it. Units park `post_offset_m` into the
//!    escape road from the racer's side, lights on ([`Assignment::Contain`]).
//! 5. A unit at its post joins the chase when the racer is within `engage_m` and in sight.
//!    Units freed from a post go back to intercepting.
//!
//! Reinforcements that must be spawned appear near the best unfilled post (still out of the
//! racer's sight and at least `spawn_min_distance_m` from it).
//!
//! **Search** (racer `Searching`): units keep following the trail to the last known position
//! for `start_after_s`, then every unit on the racer except those holding posts searches
//! ([`Assignment::Search`]). The area is what a racer driving `search_racer_mps` could have
//! reached from the last known position in `initial_s + spread × seconds lost`, so it widens
//! with time. Each unit drives (no siren) to the unchecked node with the best score:
//! hiding-spot bonuses (dead end, local street, Industrial), a bonus for being near the edge of
//! the area, minus distance from the unit; spots searched at the same time are
//! `spot_spacing_m` apart. Sighting the racer again sends the searchers back to intercept.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BinaryHeap};

use serde::{Deserialize, Serialize};
use wp_contracts::{EntityId, WorldQuery};

use crate::config::ChokepointConfig;
use crate::events::UnitKind;
use crate::graph::{add, dist, scale, sub, xy, District, NodeId, RoadClass, RoadGraph, SegmentId};
use crate::heat::PursuitStatus;
use crate::time::SimTime;
use crate::unit::Assignment;
use crate::PoliceSystem;

/// Travel times are integer milliseconds inside the searches (deterministic heap keys).
const UNREACHED: u64 = u64::MAX;

/// Why a road is an escape point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ChokeKind {
    /// No short way around: a bridge, tunnel, causeway or the only link between districts.
    Crossing,
    /// A highway road where it meets surface streets (an on-ramp).
    Ramp,
}

/// A road chain between two junctions or dead ends.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chain {
    /// Nodes in order, both ends included.
    pub nodes: Vec<NodeId>,
    /// Segments in order (`segments[k]` joins `nodes[k]` and `nodes[k + 1]`).
    pub segments: Vec<SegmentId>,
    /// Total length, m.
    pub length_m: f32,
    /// Road class (of the first segment).
    pub class: RoadClass,
}

/// An escape point: a chain the police want to hold.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Chokepoint {
    /// Index into [`ChokeIndex::chains`].
    pub chain: usize,
    /// Why.
    pub kind: ChokeKind,
    /// How much longer the way between the chain's ends gets when it is blocked, m (capped at
    /// `detour_search_m`).
    pub detour_extra_m: f32,
    /// Ranking score, m (the detour, at least `ramp_score_m` for ramps).
    pub score: f32,
}

/// Road chains and escape points of one road graph.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ChokeIndex {
    /// Every chain.
    pub chains: Vec<Chain>,
    /// Escape points, ascending by chain.
    pub chokepoints: Vec<Chokepoint>,
    /// Chain of each segment.
    pub chain_of: Vec<usize>,
    /// `(segment, other node)` per node ignoring one-way rules, ascending by segment.
    pub undirected: Vec<Vec<(SegmentId, NodeId)>>,
}

impl ChokeIndex {
    /// Build the chains and find the escape points of `g`.
    pub fn build(g: &RoadGraph, cfg: &ChokepointConfig) -> Self {
        let mut undirected: Vec<Vec<(SegmentId, NodeId)>> = vec![Vec::new(); g.nodes.len()];
        for (k, s) in g.segments.iter().enumerate() {
            let id = SegmentId(k as u32);
            undirected[s.a.0 as usize].push((id, s.b));
            if s.b != s.a {
                undirected[s.b.0 as usize].push((id, s.a));
            }
        }
        let degree = |n: NodeId| undirected[n.0 as usize].len();
        // Walk chains from every node that is not a plain bend (degree 2), then pick up the
        // remaining pure loops.
        let mut chain_of = vec![usize::MAX; g.segments.len()];
        let mut chains: Vec<Chain> = Vec::new();
        let mut walk = |start: NodeId, first: SegmentId, chain_of: &mut Vec<usize>| {
            let idx = chains.len();
            let mut nodes = vec![start];
            let mut segments = Vec::new();
            let mut length_m = 0.0;
            let (mut node, mut seg) = (start, first);
            loop {
                chain_of[seg.0 as usize] = idx;
                segments.push(seg);
                length_m += g.segment(seg).length_m;
                node = g.other_end(seg, node);
                nodes.push(node);
                if degree(node) != 2 || node == start {
                    break;
                }
                let Some(&(next, _)) = undirected[node.0 as usize]
                    .iter()
                    .find(|(s, _)| chain_of[s.0 as usize] == usize::MAX)
                else {
                    break;
                };
                seg = next;
            }
            chains.push(Chain {
                nodes,
                segments,
                length_m,
                class: g.segment(first).class,
            });
        };
        for (n, roads) in undirected.iter().enumerate() {
            let node = NodeId(n as u32);
            if roads.len() == 2 {
                continue;
            }
            for &(s, _) in roads {
                if chain_of[s.0 as usize] == usize::MAX {
                    walk(node, s, &mut chain_of);
                }
            }
        }
        for k in 0..g.segments.len() {
            if chain_of[k] == usize::MAX {
                let s = SegmentId(k as u32);
                walk(g.segment(s).a, s, &mut chain_of);
            }
        }

        let mut chokepoints = Vec::new();
        for (c, chain) in chains.iter().enumerate() {
            let a = chain.nodes[0];
            let b = *chain.nodes.last().expect("chain has nodes");
            // Loops and cul-de-sacs never lead anywhere new.
            if a == b || degree(a) == 1 || degree(b) == 1 {
                continue;
            }
            let bound = chain.length_m + cfg.detour_search_m;
            let detour = detour_m(g, &undirected, &chain_of, c, a, b, bound);
            let extra = detour.map_or(cfg.detour_search_m, |d| (d - chain.length_m).max(0.0));
            let is_ramp = chain.class == RoadClass::Highway
                && [a, b].iter().any(|n| {
                    undirected[n.0 as usize]
                        .iter()
                        .any(|(s, _)| g.segment(*s).class != RoadClass::Highway)
                });
            if extra >= cfg.min_detour_extra_m && extra >= cfg.min_detour_ratio * chain.length_m {
                chokepoints.push(Chokepoint {
                    chain: c,
                    kind: ChokeKind::Crossing,
                    detour_extra_m: extra,
                    score: extra,
                });
            } else if is_ramp {
                chokepoints.push(Chokepoint {
                    chain: c,
                    kind: ChokeKind::Ramp,
                    detour_extra_m: extra,
                    score: extra.max(cfg.ramp_score_m),
                });
            }
        }
        Self {
            chains,
            chokepoints,
            chain_of,
            undirected,
        }
    }

    /// Undirected degree of a node.
    pub fn degree(&self, n: NodeId) -> usize {
        self.undirected[n.0 as usize].len()
    }

    /// The point `offset_m` along chain `c` from its end `from` (clamped to the chain), and the
    /// segment it lies on.
    pub fn point_along(
        &self,
        g: &RoadGraph,
        c: usize,
        from: NodeId,
        offset_m: f32,
    ) -> (SegmentId, [f32; 2]) {
        let chain = &self.chains[c];
        let n = chain.segments.len();
        let reversed = chain.nodes[0] != from;
        let mut left = offset_m.clamp(0.0, chain.length_m);
        for k in 0..n {
            let (seg, a) = if reversed {
                (chain.segments[n - 1 - k], chain.nodes[n - k])
            } else {
                (chain.segments[k], chain.nodes[k])
            };
            let len = g.segment(seg).length_m;
            if left <= len || k == n - 1 {
                let pa = g.node(a).pos;
                let pb = g.node(g.other_end(seg, a)).pos;
                let t = if len > 0.0 {
                    (left / len).min(1.0)
                } else {
                    0.0
                };
                return (seg, add(pa, scale(sub(pb, pa), t)));
            }
            left -= len;
        }
        (chain.segments[0], g.node(from).pos)
    }
}

/// Shortest undirected distance `a → b` avoiding chain `skip`, if within `bound_m`.
fn detour_m(
    g: &RoadGraph,
    undirected: &[Vec<(SegmentId, NodeId)>],
    chain_of: &[usize],
    skip: usize,
    a: NodeId,
    b: NodeId,
    bound_m: f32,
) -> Option<f32> {
    // Centimetres keep the heap keys integral.
    let bound = (bound_m * 100.0) as u64;
    let mut best = vec![UNREACHED; g.nodes.len()];
    let mut heap = BinaryHeap::new();
    best[a.0 as usize] = 0;
    heap.push(Reverse((0u64, a.0)));
    while let Some(Reverse((cost, node))) = heap.pop() {
        if node == b.0 {
            return Some(cost as f32 / 100.0);
        }
        if cost > best[node as usize] {
            continue;
        }
        for &(s, next) in &undirected[node as usize] {
            if chain_of[s.0 as usize] == skip {
                continue;
            }
            let c = cost + (g.segment(s).length_m * 100.0) as u64 + 1;
            if c <= bound && c < best[next.0 as usize] {
                best[next.0 as usize] = c;
                heap.push(Reverse((c, next.0)));
            }
        }
    }
    None
}

/// Travel times (s) to every node from `starts` (`(node, seconds already spent)`), at
/// `speed(segment)` m/s, up to `limit_s` (`f32::INFINITY` beyond). `adj` gives the legal moves.
pub fn travel_times<'g>(
    g: &RoadGraph,
    adj: &dyn Fn(NodeId) -> &'g [(SegmentId, NodeId)],
    starts: &[(NodeId, f32)],
    speed: &dyn Fn(SegmentId) -> f32,
    limit_s: f32,
) -> Vec<f32> {
    let limit = (limit_s.max(0.0) * 1000.0) as u64;
    let mut best = vec![UNREACHED; g.nodes.len()];
    let mut heap = BinaryHeap::new();
    for &(n, t) in starts {
        let ms = (t.max(0.0) * 1000.0) as u64;
        if ms <= limit && ms < best[n.0 as usize] {
            best[n.0 as usize] = ms;
            heap.push(Reverse((ms, n.0)));
        }
    }
    while let Some(Reverse((cost, node))) = heap.pop() {
        if cost > best[node as usize] {
            continue;
        }
        for &(s, next) in adj(NodeId(node)) {
            let ms = (g.segment(s).length_m / speed(s).max(0.1) * 1000.0) as u64 + 1;
            let c = cost + ms;
            if c <= limit && c < best[next.0 as usize] {
                best[next.0 as usize] = c;
                heap.push(Reverse((c, next.0)));
            }
        }
    }
    best.into_iter()
        .map(|ms| {
            if ms == UNREACHED {
                f32::INFINITY
            } else {
                ms as f32 / 1000.0
            }
        })
        .collect()
}

/// A containment post being held (or driven to) by a unit.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PostView {
    /// Racer it contains.
    pub racer: EntityId,
    /// Unit holding it.
    pub unit: crate::events::UnitId,
    /// Escape road (chain index in [`ChokeIndex::chains`]).
    pub chain: usize,
    /// Why the road is an escape point.
    pub kind: ChokeKind,
    /// Where the unit parks, m.
    pub point: [f32; 2],
    /// When the racer could get there (forecast at the last replan), sim time.
    pub racer_eta: SimTime,
}

/// A search for a lost racer.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SearchView {
    /// Racer searched for.
    pub racer: EntityId,
    /// Last known position, m.
    pub origin: [f32; 2],
    /// Current extent of the search area, racer-seconds of driving from `origin`.
    pub extent_s: f32,
    /// Spots checked so far.
    pub checked: u32,
}

/// Containment plan for one racer (last replan).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct ContainPlan {
    /// Held or driven-to posts.
    pub(crate) posts: Vec<PostView>,
    /// Best candidate posts no unit could take, best first: `(chain, point)`.
    pub(crate) unfilled: Vec<(usize, [f32; 2])>,
}

/// Search state for one lost racer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct SearchState {
    pub(crate) origin: [f32; 2],
    /// Racer's heading when it was lost (zero if it was stopped).
    pub(crate) heading: [f32; 2],
    pub(crate) lost_at: SimTime,
    /// Racer-seconds from `origin` to each node.
    pub(crate) eta: Vec<f32>,
    /// Nodes already checked, ascending.
    pub(crate) checked: Vec<NodeId>,
    /// Units have been sent out to search.
    pub(crate) started: bool,
}

/// Containment and search bookkeeping inside [`PoliceSystem`].
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct StrategyState {
    /// Built on first use.
    pub(crate) index: Option<Box<ChokeIndex>>,
    pub(crate) next_replan: SimTime,
    pub(crate) plans: BTreeMap<EntityId, ContainPlan>,
    pub(crate) searches: BTreeMap<EntityId, SearchState>,
}

/// Held posts are judged on a forecast this much longer than the horizon (hysteresis against
/// posts flickering at its edge).
const KEEP_HORIZON: f32 = 1.25;

/// One candidate escape point for a racer.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    choke: Chokepoint,
    entry: NodeId,
    racer_eta_s: f32,
    rank: f32,
    /// A new post may be set up here: the racer is not on it and gets there between
    /// `min_racer_eta_s` and the horizon. Otherwise it is only kept if already held.
    fresh: bool,
}

impl PoliceSystem {
    /// Escape points of the road graph (built on first call).
    pub fn chokepoints(&mut self) -> &ChokeIndex {
        self.ensure_choke_index();
        self.strategy.index.as_deref().expect("index built")
    }

    fn ensure_choke_index(&mut self) {
        if self.strategy.index.is_none() {
            let idx = ChokeIndex::build(&self.graph, &self.cfg.strategy.chokepoints);
            self.strategy.index = Some(Box::new(idx));
        }
    }

    /// Containment posts currently held or being driven to, by racer then unit.
    pub fn containment_posts(&self) -> Vec<PostView> {
        let mut v: Vec<PostView> = self
            .strategy
            .plans
            .values()
            .flat_map(|p| p.posts.iter().copied())
            .collect();
        v.sort_by(|a, b| a.racer.cmp(&b.racer).then(a.unit.cmp(&b.unit)));
        v
    }

    /// Searches for lost racers, ascending by racer.
    pub fn searches(&self) -> Vec<SearchView> {
        let st = &self.cfg.strategy.search;
        self.strategy
            .searches
            .iter()
            .map(|(r, s)| SearchView {
                racer: *r,
                origin: s.origin,
                extent_s: st.initial_s + st.spread * self.now.since(s.lost_at),
                checked: s.checked.len() as u32,
            })
            .collect()
    }

    /// Think-rate entry point: containment posts and searches (before `plan_units`).
    pub(crate) fn plan_strategy(&mut self, world: &dyn WorldQuery) {
        // Forget racers no longer pursued.
        let racers = &self.racers;
        self.strategy
            .plans
            .retain(|r, _| racers.get(r).is_some_and(|x| x.pursued()));
        // (Searches of racers seen again are ended by `end_search`, which recalls the units.)
        self.strategy
            .searches
            .retain(|r, _| racers.get(r).is_some_and(|x| x.pursued()));
        let replan = self.now >= self.strategy.next_replan;
        if replan {
            self.strategy.next_replan = self.now.plus(self.cfg.strategy.replan_s);
        }
        let pursued: Vec<EntityId> = self
            .racers
            .values()
            .filter(|r| r.pursued())
            .map(|r| r.entity)
            .collect();
        for racer in pursued {
            match self.racers[&racer].status {
                PursuitStatus::Pursuit => {
                    self.end_search(racer);
                    if replan {
                        self.replan_containment(racer);
                    }
                    self.engage_from_posts(world, racer);
                }
                PursuitStatus::Searching => self.run_search(racer),
                _ => {}
            }
            self.refresh_post_views(racer);
        }
    }

    /// Committed, AI-driven, non-held ground units on `racer` with assignments in `kinds`.
    fn committed_units(&self, racer: EntityId) -> Vec<usize> {
        let now = self.now;
        (0..self.units.len())
            .filter(|&i| {
                let u = &self.units[i];
                u.kind == UnitKind::Car
                    && !u.wrecked
                    && u.player.is_none()
                    && now >= u.captain_hold_until
                    && u.assignment.target() == Some(racer)
            })
            .collect()
    }

    // -----------------------------------------------------------------------------------------
    // Containment
    // -----------------------------------------------------------------------------------------

    /// Racer forecast speed on a segment, m/s.
    fn racer_speed_on(&self, s: SegmentId) -> f32 {
        let c = &self.cfg.strategy.containment;
        (self.graph.segment(s).speed_limit_mps * c.racer_limit_factor).max(c.racer_min_mps)
    }

    /// Escape points `racer` could reach within the horizon × [`KEEP_HORIZON`], best first.
    fn containment_candidates(&mut self, racer: EntityId) -> Vec<Candidate> {
        self.ensure_choke_index();
        let rec = &self.racers[&racer];
        let Some(p) = rec.last_known else {
            return Vec::new();
        };
        let v = [rec.vel[0], rec.vel[1]];
        let c = self.cfg.strategy.containment.clone();
        let horizon = c.horizon_s[rec.level.table_index()];
        let Some(rp) = self.graph.nearest_on_road(p) else {
            return Vec::new();
        };
        let idx = self.strategy.index.as_deref().expect("index built");
        let seg = *self.graph.segment(rp.segment);
        let (pa, pb) = (self.graph.node(seg.a).pos, self.graph.node(seg.b).pos);
        let toward_b = crate::graph::dot(sub(pb, pa), v) >= 0.0;
        let speed = self.racer_speed_on(rp.segment);
        let (ahead, behind) = if toward_b {
            (seg.b, seg.a)
        } else {
            (seg.a, seg.b)
        };
        let starts = [
            (ahead, dist(rp.point, self.graph.node(ahead).pos) / speed),
            (
                behind,
                dist(rp.point, self.graph.node(behind).pos) / speed + c.turnaround_s,
            ),
        ];
        let adj = |n: NodeId| idx.undirected[n.0 as usize].as_slice();
        let speed_on = |s: SegmentId| self.racer_speed_on(s);
        let eta = travel_times(
            &self.graph,
            &adj,
            &starts,
            &speed_on,
            horizon * KEEP_HORIZON,
        );
        let racer_chain = idx.chain_of[rp.segment.0 as usize];
        let mut out: Vec<Candidate> = Vec::new();
        for choke in &idx.chokepoints {
            let chain = &idx.chains[choke.chain];
            let (a, b) = (chain.nodes[0], *chain.nodes.last().expect("nodes"));
            let (entry, t) = if eta[a.0 as usize] <= eta[b.0 as usize] {
                (a, eta[a.0 as usize])
            } else {
                (b, eta[b.0 as usize])
            };
            let on_it = choke.chain == racer_chain;
            if !on_it && !t.is_finite() {
                continue;
            }
            let t = if on_it { 0.0 } else { t };
            out.push(Candidate {
                choke: *choke,
                entry,
                racer_eta_s: t,
                rank: choke.score * (1.5 - 0.5 * (t / horizon).min(1.0)),
                fresh: !on_it && (c.min_racer_eta_s..=horizon).contains(&t),
            });
        }
        out.sort_by(|x, y| {
            y.rank
                .total_cmp(&x.rank)
                .then(x.choke.chain.cmp(&y.choke.chain))
        });
        out
    }

    /// Travel time (s) for unit `i` to every node (lane-legal, at intercept speed).
    fn unit_times(&self, i: usize, limit_s: f32) -> Vec<f32> {
        let u = &self.units[i];
        let p = &self.cfg.pursuit;
        let cap = p.pursuit_cap_mps.get(self.phase);
        let factor = p.respond_speed_factor * 1.15;
        let Some(rp) = self.graph.nearest_on_road(u.body.pos) else {
            return vec![f32::INFINITY; self.graph.nodes.len()];
        };
        let seg = *self.graph.segment(rp.segment);
        let v0 = (seg.speed_limit_mps * factor).min(cap).max(1.0);
        let mut starts = Vec::new();
        for (n, other) in [(seg.b, seg.a), (seg.a, seg.b)] {
            if self.graph.can_travel(rp.segment, other) {
                let ahead = crate::graph::dot(
                    sub(self.graph.node(n).pos, self.graph.node(other).pos),
                    u.body.heading,
                ) >= 0.0;
                // Turning a car around costs time.
                let turn = if ahead { 0.0 } else { 6.0 };
                starts.push((n, dist(u.body.pos, self.graph.node(n).pos) / v0 + turn));
            }
        }
        let graph = &self.graph;
        let adj = |n: NodeId| graph.outgoing(n);
        let speed_on = |s: SegmentId| (graph.segment(s).speed_limit_mps * factor).min(cap);
        travel_times(&self.graph, &adj, &starts, &speed_on, limit_s)
    }

    /// Unit `i`'s time to a post on segment `s` at `point`, from its node times.
    fn time_to_post(&self, times: &[f32], s: SegmentId, point: [f32; 2]) -> f32 {
        let seg = self.graph.segment(s);
        let v = (seg.speed_limit_mps * self.cfg.pursuit.respond_speed_factor).max(1.0);
        [seg.a, seg.b]
            .iter()
            .map(|n| times[n.0 as usize] + dist(self.graph.node(*n).pos, point) / v)
            .fold(f32::INFINITY, f32::min)
    }

    fn replan_containment(&mut self, racer: EntityId) {
        let rec = &self.racers[&racer];
        let c = self.cfg.strategy.containment.clone();
        let li = rec.level.table_index();
        let posts = if rec.level.number() == 0 {
            0
        } else {
            c.posts_per_level[li] as usize
        };
        let tail = c.tail_units[li] as usize;
        let speed = (rec.vel[0] * rec.vel[0] + rec.vel[1] * rec.vel[1]).sqrt();
        let racer_pos = rec.last_known.unwrap_or(xy(rec.pos));
        let committed = self.committed_units(racer);
        let mut plan = ContainPlan::default();
        let cands = if posts == 0 {
            Vec::new()
        } else {
            self.containment_candidates(racer)
        };
        // A slow racer gets no new posts (everyone converges for the box-in and arrest);
        // posts already held stay.
        let new_posts = speed >= c.min_racer_speed_mps;
        // Tail: the closest chasers stay on the racer.
        let mut chasers: Vec<(f32, usize)> = committed
            .iter()
            .copied()
            .filter(|&i| !matches!(self.units[i].assignment, Assignment::Contain { .. }))
            .map(|i| (dist(self.units[i].body.pos, racer_pos), i))
            .collect();
        chasers.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let tail_set: Vec<usize> = chasers.iter().take(tail).map(|x| x.1).collect();
        let mut free: Vec<usize> = committed
            .iter()
            .copied()
            .filter(|i| !tail_set.contains(i))
            .collect();
        let top: Vec<Candidate> = cands
            .iter()
            .filter(|x| x.fresh && new_posts)
            .take(posts * 2)
            .copied()
            .collect();
        // Chains with a unit. Units already holding a post the racer can still get to keep
        // it (including the moment the racer is about to arrive or is on it).
        let mut taken: Vec<usize> = Vec::new();
        for &i in &committed {
            if let Assignment::Contain { segment, point, .. } = self.units[i].assignment {
                let chain =
                    self.strategy.index.as_deref().expect("index").chain_of[segment.0 as usize];
                // Kept while the racer is about to get there (or is on it), or while the post
                // still ranks among the best 2 × posts escape points.
                let keep = cands.iter().enumerate().find(|(k, x)| {
                    x.choke.chain == chain && (x.racer_eta_s <= c.min_racer_eta_s || *k < posts * 2)
                });
                if let Some((_, cand)) = keep {
                    if taken.len() < posts && !taken.contains(&chain) {
                        taken.push(chain);
                        free.retain(|&f| f != i);
                        plan.posts.push(PostView {
                            racer,
                            unit: self.units[i].id,
                            chain,
                            kind: cand.choke.kind,
                            point,
                            racer_eta: self.now.plus(cand.racer_eta_s),
                        });
                    }
                }
            }
        }
        // Fill the best remaining posts with whoever gets there soonest (and in time).
        let horizon = c.horizon_s[li];
        let times: Vec<(usize, Vec<f32>)> = free
            .iter()
            .map(|&i| (i, self.unit_times(i, horizon)))
            .collect();
        for cand in &top {
            if taken.len() >= posts {
                break;
            }
            if taken.contains(&cand.choke.chain) {
                continue;
            }
            let (segment, point) = {
                let idx = self.strategy.index.as_deref().expect("index");
                let half = idx.chains[cand.choke.chain].length_m * 0.5;
                idx.point_along(
                    &self.graph,
                    cand.choke.chain,
                    cand.entry,
                    c.post_offset_m.min(half),
                )
            };
            let mut best: Option<(f32, usize)> = None;
            for (i, t) in &times {
                if !free.contains(i) {
                    continue;
                }
                let eta = self.time_to_post(t, segment, point);
                if eta + c.arrive_margin_s <= cand.racer_eta_s && best.is_none_or(|b| eta < b.0) {
                    best = Some((eta, *i));
                }
            }
            match best {
                Some((_, i)) => {
                    taken.push(cand.choke.chain);
                    free.retain(|&f| f != i);
                    self.assign(
                        i,
                        Assignment::Contain {
                            target: racer,
                            segment,
                            point,
                        },
                    );
                    self.stats.containment_posts += 1;
                    plan.posts.push(PostView {
                        racer,
                        unit: self.units[i].id,
                        chain: cand.choke.chain,
                        kind: cand.choke.kind,
                        point,
                        racer_eta: self.now.plus(cand.racer_eta_s),
                    });
                }
                None => plan.unfilled.push((cand.choke.chain, point)),
            }
        }
        // Units left over from posts that no longer matter go back to intercepting.
        for &i in &free {
            if matches!(self.units[i].assignment, Assignment::Contain { .. }) {
                self.assign(
                    i,
                    Assignment::Intercept {
                        target: racer,
                        point: racer_pos,
                    },
                );
            }
        }
        self.strategy.plans.insert(racer, plan);
    }

    /// Units at (or driving to) a post join the chase once the racer is close and in sight.
    fn engage_from_posts(&mut self, world: &dyn WorldQuery, racer: EntityId) {
        let engage = self.cfg.strategy.containment.engage_m;
        let seen = self.obs.get(&racer).is_some_and(|o| o.seen);
        if !seen {
            return;
        }
        let rp = xy(self.racers[&racer].pos);
        for i in self.committed_units(racer) {
            let u = &self.units[i];
            if matches!(u.assignment, Assignment::Contain { .. })
                && dist(u.body.pos, rp) <= engage
                && self.sees(world, u.body.pos, rp)
            {
                self.assign(i, Assignment::Pursue { target: racer });
            }
        }
    }

    /// Keep the plan's post list in step with the units actually holding posts.
    fn refresh_post_views(&mut self, racer: EntityId) {
        let units = &self.units;
        if let Some(plan) = self.strategy.plans.get_mut(&racer) {
            plan.posts.retain(|p| {
                units
                    .binary_search_by_key(&p.unit, |u| u.id)
                    .ok()
                    .is_some_and(|k| {
                        matches!(units[k].assignment, Assignment::Contain { target, .. } if target == racer)
                    })
            });
        }
    }

    /// Where to spawn a reinforcement for `racer`: near its best unfilled post, if any.
    pub(crate) fn unfilled_post(&self, racer: EntityId) -> Option<[f32; 2]> {
        self.strategy
            .plans
            .get(&racer)
            .and_then(|p| p.unfilled.first())
            .map(|x| x.1)
    }

    // -----------------------------------------------------------------------------------------
    // Search
    // -----------------------------------------------------------------------------------------

    /// The racer was seen again: searchers go back to intercepting.
    fn end_search(&mut self, racer: EntityId) {
        if let Some(s) = self.strategy.searches.remove(&racer) {
            if s.started {
                self.stats.search_reacquired += 1;
            }
        }
        let point = self.racers[&racer]
            .last_known
            .unwrap_or(xy(self.racers[&racer].pos));
        for i in self.committed_units(racer) {
            if matches!(self.units[i].assignment, Assignment::Search { .. }) {
                self.assign(
                    i,
                    Assignment::Intercept {
                        target: racer,
                        point,
                    },
                );
            }
        }
    }

    fn run_search(&mut self, racer: EntityId) {
        let sc = self.cfg.strategy.search.clone();
        if !self.strategy.searches.contains_key(&racer) {
            let state = self.new_search(racer);
            self.strategy.searches.insert(racer, state);
        }
        let lost_s = self.now.since(self.strategy.searches[&racer].lost_at);
        if lost_s < sc.start_after_s {
            return;
        }
        // Everyone on the racer searches, except the posts and the followers (the units
        // closest to the last known position keep to the trail and the predicted route).
        let started = self.strategy.searches[&racer].started;
        let origin = self.strategy.searches[&racer].origin;
        let mut chasers: Vec<(f32, usize)> = self
            .committed_units(racer)
            .into_iter()
            .filter(|&i| {
                matches!(
                    self.units[i].assignment,
                    Assignment::Pursue { .. } | Assignment::Intercept { .. }
                )
            })
            .map(|i| (dist(self.units[i].body.pos, origin), i))
            .collect();
        chasers.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        for &(_, i) in chasers.iter().skip(sc.followers as usize) {
            let spot = self
                .pick_spot(racer, i)
                .unwrap_or_else(|| self.search_origin_node(racer));
            self.assign(
                i,
                Assignment::Search {
                    target: racer,
                    spot,
                },
            );
        }
        if !started {
            self.strategy
                .searches
                .get_mut(&racer)
                .expect("search")
                .started = true;
            self.radio(crate::radio::RadioCue::new(
                crate::radio::RadioTier::Tactical,
                None,
                format!(
                    "Dispatch: lost visual on racer #{}, all units start a grid search.",
                    racer.0
                ),
            ));
        }
        // Spots reached (or routes finished): mark checked, pick the next.
        for i in self.committed_units(racer) {
            let Assignment::Search { spot, .. } = self.units[i].assignment else {
                continue;
            };
            let u = &self.units[i];
            let reached = dist(u.body.pos, self.graph.node(spot).pos) <= sc.arrive_m
                || (u.cmd.track.is_drivable() && u.track_s >= u.cmd.track.length() - 2.0);
            if !reached {
                continue;
            }
            let s = self.strategy.searches.get_mut(&racer).expect("search");
            if let Err(k) = s.checked.binary_search(&spot) {
                s.checked.insert(k, spot);
                self.stats.search_spots_checked += 1;
            }
            if let Some(next) = self.pick_spot(racer, i) {
                // Same assignment kind: no radio line, new route.
                let u = &mut self.units[i];
                u.assignment = Assignment::Search {
                    target: racer,
                    spot: next,
                };
                u.cmd = crate::unit::DriveCommand::stop();
                u.track_s = 0.0;
            }
        }
    }

    fn new_search(&mut self, racer: EntityId) -> SearchState {
        self.ensure_choke_index();
        let sc = &self.cfg.strategy.search;
        let rec = &self.racers[&racer];
        let origin = rec.last_known.unwrap_or(xy(rec.pos));
        let lost_at = rec.last_seen.unwrap_or(self.now);
        let v = [rec.vel[0], rec.vel[1]];
        // Longest search the escape rules allow, plus slack.
        let horizon = sc.initial_s + sc.spread * 120.0;
        let idx = self.strategy.index.as_deref().expect("index built");
        let mut starts = Vec::new();
        if let Some(rp) = self.graph.nearest_on_road(origin) {
            let seg = *self.graph.segment(rp.segment);
            let (pa, pb) = (self.graph.node(seg.a).pos, self.graph.node(seg.b).pos);
            let toward_b = crate::graph::dot(sub(pb, pa), v) >= 0.0;
            let turn = self.cfg.strategy.containment.turnaround_s;
            for (n, back) in [(seg.a, toward_b), (seg.b, !toward_b)] {
                let t = dist(rp.point, self.graph.node(n).pos) / sc.search_racer_mps;
                starts.push((n, t + if back { turn } else { 0.0 }));
            }
        }
        let adj = |n: NodeId| idx.undirected[n.0 as usize].as_slice();
        let mps = sc.search_racer_mps;
        let eta = travel_times(&self.graph, &adj, &starts, &|_| mps, horizon);
        SearchState {
            origin,
            heading: crate::graph::normalize(v),
            lost_at,
            eta,
            checked: Vec::new(),
            started: false,
        }
    }

    fn search_origin_node(&self, racer: EntityId) -> NodeId {
        let o = self.strategy.searches[&racer].origin;
        self.graph.nearest_node(o).unwrap_or(NodeId(0))
    }

    /// Hiding-spot score of a node (dead end, local street, Industrial).
    fn hide_score(&self, n: NodeId) -> Option<f32> {
        let sc = &self.cfg.strategy.search;
        let idx = self.strategy.index.as_deref()?;
        let roads = &idx.undirected[n.0 as usize];
        if roads.is_empty()
            || roads
                .iter()
                .all(|(s, _)| self.graph.segment(*s).class == RoadClass::Highway)
        {
            // Nobody hides on a highway.
            return None;
        }
        let mut score = 0.0;
        if roads.len() == 1 {
            score += sc.dead_end_bonus;
        }
        if roads
            .iter()
            .any(|(s, _)| self.graph.segment(*s).class == RoadClass::Local)
        {
            score += sc.local_bonus;
        }
        if self.graph.node(n).district == District::Industrial {
            score += sc.industrial_bonus;
        }
        Some(score)
    }

    /// The best unchecked spot for unit `i` searching for `racer`.
    fn pick_spot(&self, racer: EntityId, i: usize) -> Option<NodeId> {
        let sc = &self.cfg.strategy.search;
        let s = self.strategy.searches.get(&racer)?;
        let extent = sc.initial_s + sc.spread * self.now.since(s.lost_at);
        let me = self.units[i].body.pos;
        let my_id = self.units[i].id;
        // Spots other searchers are heading for.
        let others: Vec<[f32; 2]> = self
            .units
            .iter()
            .filter(|u| u.id != my_id)
            .filter_map(|u| match u.assignment {
                Assignment::Search { target, spot } if target == racer => {
                    Some(self.graph.node(spot).pos)
                }
                _ => None,
            })
            .collect();
        let mut best: Option<(f32, NodeId)> = None;
        for (k, &t) in s.eta.iter().enumerate() {
            if t > extent {
                continue;
            }
            let n = NodeId(k as u32);
            if s.checked.binary_search(&n).is_ok() {
                continue;
            }
            let p = self.graph.node(n).pos;
            if others.iter().any(|o| dist(*o, p) < sc.spot_spacing_m) {
                continue;
            }
            let Some(hide) = self.hide_score(n) else {
                continue;
            };
            let ahead = crate::graph::dot(crate::graph::normalize(sub(p, s.origin)), s.heading);
            let score =
                hide + sc.outward_weight * (t / extent.max(1e-3)) + sc.ahead_bonus * ahead.max(0.0)
                    - dist(me, p) / sc.travel_weight_m;
            if best.is_none_or(|b| score > b.0) {
                best = Some((score, n));
            }
        }
        best.map(|b| b.1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PoliceConfig;

    /// Two 3×3 grids (100 m blocks) joined by a single 400 m bridge.
    fn two_islands() -> RoadGraph {
        let mut g = RoadGraph::new();
        let grid = |g: &mut RoadGraph, x0: f32| {
            let mut ids = [[NodeId(0); 3]; 3];
            for (r, row) in ids.iter_mut().enumerate() {
                for (c, id) in row.iter_mut().enumerate() {
                    *id = g.add_node([x0 + c as f32 * 100.0, r as f32 * 100.0], District::Core);
                }
            }
            for r in 0..3 {
                for c in 0..3 {
                    if c + 1 < 3 {
                        g.add_segment(ids[r][c], ids[r][c + 1], 13.9, RoadClass::Local, false);
                    }
                    if r + 1 < 3 {
                        g.add_segment(ids[r][c], ids[r + 1][c], 13.9, RoadClass::Local, false);
                    }
                }
            }
            ids
        };
        let west = grid(&mut g, 0.0);
        let east = grid(&mut g, 600.0);
        // Bridge in two pieces (a curved road is a chain of segments).
        let mid = g.add_node([400.0, 100.0], District::Core);
        g.add_segment(west[1][2], mid, 16.7, RoadClass::Arterial, false);
        g.add_segment(mid, east[1][0], 16.7, RoadClass::Arterial, false);
        g
    }

    #[test]
    fn bridge_is_the_only_crossing_chokepoint() {
        let g = two_islands();
        let cfg = PoliceConfig::builtin().strategy.chokepoints;
        let idx = ChokeIndex::build(&g, &cfg);
        // Every segment belongs to exactly one chain.
        assert!(idx.chain_of.iter().all(|c| *c < idx.chains.len()));
        let crossings: Vec<&Chokepoint> = idx
            .chokepoints
            .iter()
            .filter(|c| c.kind == ChokeKind::Crossing)
            .collect();
        assert_eq!(crossings.len(), 1, "{:?}", idx.chokepoints);
        let chain = &idx.chains[crossings[0].chain];
        assert_eq!(chain.segments.len(), 2);
        assert!((chain.length_m - 400.0).abs() < 1.0);
        assert_eq!(crossings[0].detour_extra_m, cfg.detour_search_m);
    }

    #[test]
    fn grid_streets_are_not_chokepoints() {
        let g = two_islands();
        let idx = ChokeIndex::build(&g, &PoliceConfig::builtin().strategy.chokepoints);
        // A grid block side has a 200 m detour around the block.
        assert_eq!(idx.chokepoints.len(), 1);
    }

    #[test]
    fn point_along_walks_the_chain_from_either_end() {
        let g = two_islands();
        let idx = ChokeIndex::build(&g, &PoliceConfig::builtin().strategy.chokepoints);
        let c = idx.chokepoints[0].chain;
        let chain = &idx.chains[c];
        let (a, b) = (chain.nodes[0], *chain.nodes.last().unwrap());
        let (_, pa) = idx.point_along(&g, c, a, 30.0);
        let (_, pb) = idx.point_along(&g, c, b, 30.0);
        assert!((dist(pa, g.node(a).pos) - 30.0).abs() < 0.1);
        assert!((dist(pb, g.node(b).pos) - 30.0).abs() < 0.1);
        let (_, far) = idx.point_along(&g, c, a, 250.0);
        assert!((dist(far, g.node(a).pos) - 250.0).abs() < 0.5);
    }

    #[test]
    fn travel_times_respect_the_limit() {
        let g = two_islands();
        let adj = |n: NodeId| g.outgoing(n);
        let t = travel_times(&g, &adj, &[(NodeId(0), 0.0)], &|_| 10.0, 25.0);
        assert_eq!(t[0], 0.0);
        assert!((t[1] - 10.0).abs() < 0.01);
        // 300 m away at 10 m/s is beyond 25 s.
        assert!(t.iter().filter(|x| x.is_finite()).all(|x| *x <= 25.0));
        assert!(t[8].is_infinite());
    }
}
