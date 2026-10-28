//! Police tools (roadblocks, spike strips, helicopter view) and racer-side gadget queries
//! (radar detector, scanner).
//!
//! Placement rules (enforced by [`plan_site`], used by both the AI and captain paths):
//! - on the racer's predicted path, between `min_ahead_m` and `max_ahead_m` ahead;
//! - never within 15 m of an intersection;
//! - approach sight distance ([`RoadGraph::approach_sight_m`]) ≥ `min_sight_m` — never around a
//!   blind corner;
//! - roadblocks never on a highway when the wanted level is above `highway_max_level`.

use serde::{Deserialize, Serialize};
use wp_contracts::EntityId;

use crate::events::{ToolId, UnitId};
use crate::graph::{
    add, cross, dist, len, perp_left, scale, sub, NodeId, RoadClass, RoadGraph, SegmentId,
};
use crate::time::SimTime;

/// Distance kept clear of intersections when placing tools, m.
pub const NODE_CLEARANCE_M: f32 = 15.0;

/// A validated tool site.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ToolSite {
    /// Segment.
    pub segment: SegmentId,
    /// Node the racer approaches from.
    pub from_node: NodeId,
    /// Distance from `from_node` along the segment, m.
    pub d_m: f32,
    /// Centre, m.
    pub pos: [f32; 2],
    /// Travel direction of the approaching racer.
    pub dir: [f32; 2],
    /// Approach sight distance, m.
    pub sight_m: f32,
    /// Distance ahead of the racer along its predicted path, m.
    pub ahead_m: f32,
}

/// Placement constraints.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SiteRules {
    /// Minimum distance ahead, m.
    pub min_ahead_m: f32,
    /// Maximum distance ahead, m.
    pub max_ahead_m: f32,
    /// Minimum approach sight distance, m.
    pub min_sight_m: f32,
    /// Highways allowed.
    pub highway_ok: bool,
}

pub use wp_contracts::SiteError;

/// Check one candidate point `d_m` along `seg` from `from`.
pub fn check_point(
    g: &RoadGraph,
    seg: SegmentId,
    from: NodeId,
    d_m: f32,
    rules: &SiteRules,
) -> Result<f32, SiteError> {
    let s = g.segment(seg);
    if s.class == RoadClass::Highway && !rules.highway_ok {
        return Err(SiteError::Highway);
    }
    if d_m < NODE_CLEARANCE_M || d_m > s.length_m - NODE_CLEARANCE_M {
        return Err(SiteError::BlindCorner);
    }
    let sight = g.approach_sight_m(seg, from, d_m, 1000.0);
    if sight < rules.min_sight_m {
        return Err(SiteError::BlindCorner);
    }
    Ok(sight)
}

/// Find the first valid site on the racer's predicted path. When `only_road` (a
/// `RoadSegment::edge` id) is given, the site must be on that road (captain request).
pub fn plan_site(
    g: &RoadGraph,
    racer_pos: [f32; 2],
    racer_vel: [f32; 2],
    rules: &SiteRules,
    only_road: Option<u32>,
) -> Result<ToolSite, SiteError> {
    plan_site_where(g, racer_pos, racer_vel, rules, only_road, &|_| true)
}

/// [`plan_site`] with an extra test: the first site that meets the rules and that `accept`
/// takes (spike strips: one whose car can park out of the racer's view, PR #54 / row 131).
/// When the rules are met but `accept` turns every such site down: [`SiteError::NoSite`].
pub fn plan_site_where(
    g: &RoadGraph,
    racer_pos: [f32; 2],
    racer_vel: [f32; 2],
    rules: &SiteRules,
    only_road: Option<u32>,
    accept: &dyn Fn(&ToolSite) -> bool,
) -> Result<ToolSite, SiteError> {
    let path = g.predict_path(racer_pos, racer_vel, rules.max_ahead_m + 50.0);
    let mut s = rules.min_ahead_m;
    let mut seen_segment = false;
    let mut last_err = SiteError::NoSite;
    while s <= rules.max_ahead_m.min(path.length()) {
        let Some((i, into)) = path.leg_at(s) else {
            break;
        };
        let leg = path.legs[i];
        if only_road.is_some_and(|o| g.segment(leg.segment).edge != Some(o)) {
            s += 10.0;
            continue;
        }
        seen_segment = true;
        // Distance from the leg's node (the first leg starts mid-segment at the racer).
        let node_pos = g.node(leg.from_node).pos;
        let d_m = dist(node_pos, leg.from) + into;
        match check_point(g, leg.segment, leg.from_node, d_m, rules) {
            Ok(sight) => {
                let dir = g.dir_from(leg.segment, leg.from_node);
                let site = ToolSite {
                    segment: leg.segment,
                    from_node: leg.from_node,
                    d_m,
                    pos: add(node_pos, scale(dir, d_m)),
                    dir,
                    sight_m: sight,
                    ahead_m: s,
                };
                if accept(&site) {
                    return Ok(site);
                }
                last_err = SiteError::NoSite;
            }
            Err(e) => last_err = e,
        }
        s += 10.0;
    }
    if only_road.is_some() && !seen_segment {
        return Err(SiteError::NotOnPath);
    }
    Err(last_err)
}

/// A static collider for `wp_sim` to insert into Rapier (box, rotated about Z).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StaticCollider {
    /// Centre, m.
    pub pos: [f32; 3],
    /// Half extents in the collider frame (x across, y along, z up), m.
    pub half_extents_m: [f32; 3],
    /// Collider +Y direction in world XY.
    pub forward: [f32; 2],
    /// Mass, kg (movable when hit; `wp_sim` may make it dynamic).
    pub mass_kg: f32,
}

/// A roadblock: parked units + barriers across the road.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Roadblock {
    /// Id.
    pub id: ToolId,
    /// Racer it targets.
    pub racer: EntityId,
    /// Site.
    pub site: ToolSite,
    /// Parked units.
    pub units: Vec<UnitId>,
    /// Barrier colliders.
    pub barriers: Vec<StaticCollider>,
    /// Removed at.
    pub expires: SimTime,
    /// Placed by a race profile ([`crate::race`]): stays when the racer's pursuit ends and
    /// does not count against the AI captain's budget.
    #[serde(default)]
    pub scripted: bool,
    /// The wooden barricade over the block's one gap (owner, 2026-10-28: the only way
    /// through, smashed at a speed and damage cost). `None` only for blocks from saves made
    /// before the rule.
    #[serde(default)]
    pub barricade: Option<Barricade>,
}

/// The one gap a roadblock leaves open (about one car width, [`GAP_W_M`]) and the breakable
/// wooden sawhorse barricade standing in it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Barricade {
    /// The barricade's box (spans the gap and overlaps the barriers either side a little).
    pub collider: StaticCollider,
    /// Gap edges on the barrier line, m: left end, right end (seen by the approaching racer).
    pub gap: [[f32; 2]; 2],
    /// Clear width of the gap, m.
    pub width_m: f32,
}

/// Clear width of a roadblock's one gap, m: one car width (the widest cars are about 2.0 m
/// across) with ~0.3 m to spare each side.
pub const GAP_W_M: f32 = 2.6;
/// The barrier line stands this far past the site point along the racer's travel, m.
pub const BLOCK_ALONG_M: f32 = 4.0;
/// Boxes on the barrier line overlap their neighbours by this much (no seam a wheel could
/// find), m.
pub const BARRIER_OVERLAP_M: f32 = 0.1;
/// End barriers reach this far into the hard stop they meet (wall, barrier, rail), m.
pub const STOP_OVERLAP_M: f32 = 0.25;
/// A roadblock seals only where the hard stops on both sides are within this of the paved
/// road's edge (a lot or verge beside the sidewalk), m.
pub const VERGE_REACH_M: f32 = 12.0;
/// Barricade size: half height and half depth, m (a sawhorse with a board ~1 m up).
pub const BARRICADE_HALF_H_M: f32 = 0.55;
/// See [`BARRICADE_HALF_H_M`].
pub const BARRICADE_HALF_D_M: f32 = 0.3;
/// Barricade mass (timber frame weighted with sandbags), kg.
pub const BARRICADE_MASS_KG: f32 = 90.0;
/// A spike strip's end lies this far under its unit's car (its inner side), m.
pub const SPIKE_UNDER_CAR_M: f32 = 0.4;
/// Half width of a parked police car, m.
pub const UNIT_HALF_W_M: f32 = 0.95;
/// Half length of a parked police car, m.
pub const UNIT_HALF_L_M: f32 = 2.45;

/// Where a roadblock leaves its gap, across the carriageway.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GapPick {
    /// At the left edge of the carriageway.
    Left,
    /// In the middle of it.
    Centre,
    /// At the right edge.
    Right,
}

/// A roadblock laid out across a [`crate::section::Section`] (see [`roadblock_layout`]).
#[derive(Debug, Clone, PartialEq)]
pub struct BlockLayout {
    /// Solid barriers, left to right.
    pub barriers: Vec<StaticCollider>,
    /// The gap and its barricade.
    pub barricade: Barricade,
    /// Parking spots for the block's cars: nose to tail just behind the barrier line, on the
    /// solid parts (never behind the gap), nearest the gap first.
    pub unit_spots: Vec<([f32; 2], [f32; 2])>,
    /// Lateral span covered, m (+ right of the site's travel direction).
    pub lo: f32,
    /// See `lo`.
    pub hi: f32,
}

/// Lay a roadblock across `sec` at `site`: one barrier line from the left hard stop to the
/// right one (into them by [`STOP_OVERLAP_M`]; the paved edges where a side has no stop),
/// boxes overlapping by [`BARRIER_OVERLAP_M`], broken only by one [`GAP_W_M`] gap on the
/// carriageway at `gap`, which a wooden barricade fills. `he` is the barrier box's half
/// extents (its length is adjusted to fill each run exactly).
pub fn roadblock_layout(
    site: &ToolSite,
    sec: &crate::section::Section,
    he: [f32; 3],
    mass_kg: f32,
    gap: GapPick,
) -> BlockLayout {
    let right = [site.dir[1], -site.dir[0]];
    let line = add(site.pos, scale(site.dir, BLOCK_ALONG_M));
    let at = |x: f32| add(line, scale(right, x));
    let lo = sec.stop_lo.map_or(sec.paved_lo, |s| s - STOP_OVERLAP_M);
    let hi = sec.stop_hi.map_or(sec.paved_hi, |s| s + STOP_OVERLAP_M);
    // The gap on the carriageway, 0.3 m in from its edge at the sides.
    let half = GAP_W_M * 0.5;
    let (rlo, rhi) = (sec.road_lo, sec.road_hi);
    let mut c = match gap {
        GapPick::Left => rlo + 0.3 + half,
        GapPick::Centre => 0.5 * (rlo + rhi),
        GapPick::Right => rhi - 0.3 - half,
    };
    c = c.clamp(lo + half + 0.05, hi - half - 0.05);
    let (g0, g1) = (c - half, c + half);
    let mut barriers = Vec::new();
    for (a, b) in [(lo, g0), (g1, hi)] {
        let w = b - a;
        if w <= 0.02 {
            continue;
        }
        let full = 2.0 * he[0];
        let n = ((w - BARRIER_OVERLAP_M) / (full - BARRIER_OVERLAP_M))
            .ceil()
            .max(1.0) as usize;
        let piece = (w + (n as f32 - 1.0) * BARRIER_OVERLAP_M) / n as f32;
        for i in 0..n {
            let x = a + 0.5 * piece + i as f32 * (piece - BARRIER_OVERLAP_M);
            let p = at(x);
            barriers.push(StaticCollider {
                pos: [p[0], p[1], he[2]],
                half_extents_m: [0.5 * piece, he[1], he[2]],
                forward: site.dir,
                mass_kg,
            });
        }
    }
    let bp = at(c);
    let barricade = Barricade {
        collider: StaticCollider {
            pos: [bp[0], bp[1], BARRICADE_HALF_H_M],
            half_extents_m: [
                half + BARRIER_OVERLAP_M,
                BARRICADE_HALF_D_M,
                BARRICADE_HALF_H_M,
            ],
            forward: site.dir,
            mass_kg: BARRICADE_MASS_KG,
        },
        gap: [at(g0), at(g1)],
        width_m: GAP_W_M,
    };
    // Cars nose to tail behind the line (racer side clear), never behind the gap; a second
    // row behind the first when one row has too few spots.
    let step = 2.0 * UNIT_HALF_L_M + 0.4;
    let mut spots: Vec<(f32, [f32; 2], [f32; 2])> = Vec::new();
    let (plo, phi) = (sec.paved_lo.max(lo), sec.paved_hi.min(hi));
    for row in 0..2 {
        let behind =
            BLOCK_ALONG_M + he[1] + 0.3 + UNIT_HALF_W_M + row as f32 * (2.0 * UNIT_HALF_W_M + 0.6);
        let car_at = |x: f32| add(add(site.pos, scale(site.dir, behind)), scale(right, x));
        let rank = |x: f32| row as f32 * 1000.0 + (x - c).abs();
        // Left of the gap, from the gap outward; then right of it.
        let mut x = g0 - 0.6 - UNIT_HALF_L_M;
        while x - UNIT_HALF_L_M >= plo {
            spots.push((rank(x), car_at(x), perp_left(site.dir)));
            x -= step;
        }
        let mut x = g1 + 0.6 + UNIT_HALF_L_M;
        while x + UNIT_HALF_L_M <= phi {
            spots.push((rank(x), car_at(x), scale(perp_left(site.dir), -1.0)));
            x += step;
        }
    }
    spots.sort_by(|a, b| a.0.total_cmp(&b.0));
    BlockLayout {
        barriers,
        barricade,
        unit_spots: spots.into_iter().map(|(_, p, h)| (p, h)).collect(),
        lo,
        hi,
    }
}

/// A spike strip laid out across a [`crate::section::Section`] (see [`spike_layout`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpikeLayout {
    /// Strip end on the left (the far end from its car), m.
    pub a: [f32; 2],
    /// Strip end on the right, [`SPIKE_UNDER_CAR_M`] under its car.
    pub b: [f32; 2],
    /// Where its car parks, m: on the right edge of the paving, beside the strip's end.
    pub post: [f32; 2],
    /// Parked heading: the racer's travel direction.
    pub heading: [f32; 2],
}

/// Lay a spike strip across `sec` at `site`: from the left end of the paved run (into the
/// hard stop there, if it meets one) to [`SPIKE_UNDER_CAR_M`] under its car, which stands on
/// the right edge of the paving. Every lane, shoulder and sidewalk of the road is covered;
/// the ways past are off the paving (where there is no wall or barrier) and through the car.
pub fn spike_layout(site: &ToolSite, sec: &crate::section::Section) -> SpikeLayout {
    let right = [site.dir[1], -site.dir[0]];
    let at = |x: f32| add(site.pos, scale(right, x));
    let lo = match sec.stop_lo {
        Some(s) if s >= sec.paved_lo - 0.3 => s,
        _ => sec.paved_lo,
    };
    let edge = match sec.stop_hi {
        Some(s) if s <= sec.paved_hi + 0.3 => s.min(sec.paved_hi),
        _ => sec.paved_hi,
    };
    let car = edge - UNIT_HALF_W_M - 0.05;
    let end = car - UNIT_HALF_W_M + SPIKE_UNDER_CAR_M;
    SpikeLayout {
        a: at(lo),
        b: at(end.max(lo + 0.5)),
        post: at(car),
        heading: site.dir,
    }
}

/// A spike strip: a line across the road.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpikeStrip {
    /// Id.
    pub id: ToolId,
    /// Racer it targets.
    pub racer: EntityId,
    /// Site.
    pub site: ToolSite,
    /// End A, m.
    pub a: [f32; 2],
    /// End B, m.
    pub b: [f32; 2],
    /// Unit that deployed it and stands beside it. A strip only lies on the road while that
    /// unit is at its post (`None` only for scripted race-profile strips, which have none).
    pub unit: Option<UnitId>,
    /// Where its unit parked, m: the strip is lifted once the unit is further than
    /// [`crate::dispatch::SPIKE_UNIT_LEASH_M`] from here.
    #[serde(default)]
    pub post: Option<[f32; 2]>,
    /// Removed at.
    pub expires: SimTime,
    /// Placed by a race profile ([`crate::race`]): stays when the racer's pursuit ends and
    /// does not count against the AI captain's budget.
    #[serde(default)]
    pub scripted: bool,
}

impl SpikeStrip {
    /// Strip across the road at `site`, `length_m` long.
    pub fn across(
        id: ToolId,
        racer: EntityId,
        site: ToolSite,
        length_m: f32,
        unit: Option<UnitId>,
        expires: SimTime,
    ) -> Self {
        let half = scale(perp_left(site.dir), length_m * 0.5);
        Self {
            id,
            racer,
            site,
            a: add(site.pos, half),
            b: sub(site.pos, half),
            unit,
            post: None,
            expires,
            scripted: false,
        }
    }

    /// Strip from `a` to `b` at `site` (a carriageway's width on a divided road, row 133).
    pub fn between(
        id: ToolId,
        racer: EntityId,
        site: ToolSite,
        a: [f32; 2],
        b: [f32; 2],
        unit: Option<UnitId>,
        expires: SimTime,
    ) -> Self {
        Self {
            id,
            racer,
            site,
            a,
            b,
            unit,
            post: None,
            expires,
            scripted: false,
        }
    }

    /// Which wheels (FL, FR, RL, RR) cross the strip when a car moves from `p0` to `p1`.
    /// Wheels sit ±`half_track_m` across the travel direction; front and rear take the same
    /// line, so a crossing wheel pair flags both axles.
    pub fn wheels_crossing(&self, p0: [f32; 2], p1: [f32; 2], half_track_m: f32) -> [bool; 4] {
        let mv = sub(p1, p0);
        let l = len(mv);
        if l < 1e-4 {
            return [false; 4];
        }
        let right = [mv[1] / l, -mv[0] / l];
        let mut hit = [false; 4];
        for (side, sign) in [(0usize, -1.0f32), (1usize, 1.0f32)] {
            let off = scale(right, sign * half_track_m);
            if segments_intersect(add(p0, off), add(p1, off), self.a, self.b) {
                hit[side] = true; // front
                hit[side + 2] = true; // rear
            }
        }
        hit
    }
}

/// Whether segments `p–q` and `a–b` intersect (inclusive).
pub fn segments_intersect(p: [f32; 2], q: [f32; 2], a: [f32; 2], b: [f32; 2]) -> bool {
    let d1 = cross(sub(q, p), sub(a, p));
    let d2 = cross(sub(q, p), sub(b, p));
    let d3 = cross(sub(b, a), sub(p, a));
    let d4 = cross(sub(b, a), sub(q, a));
    (d1 * d2 <= 0.0) && (d3 * d4 <= 0.0) && !(d1 == 0.0 && d2 == 0.0)
}

/// Tire deflation state of one vehicle.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Puncture {
    /// Time each wheel was punctured.
    pub since: [Option<SimTime>; 4],
}

impl Puncture {
    #[allow(clippy::needless_range_loop)]
    /// Flag wheels as punctured at `now_s` (already punctured wheels keep their time).
    pub fn hit(&mut self, wheels: [bool; 4], now: SimTime) {
        for i in 0..4 {
            if wheels[i] && self.since[i].is_none() {
                self.since[i] = Some(now);
            }
        }
    }

    #[allow(clippy::needless_range_loop)]
    /// Deflation per wheel 0..=1 (`HandlingEffects::tire_puncture`), linear over `deflate_s`.
    pub fn level(&self, now: SimTime, deflate_s: f32) -> [f32; 4] {
        let mut out = [0.0; 4];
        for i in 0..4 {
            if let Some(t) = self.since[i] {
                out[i] = (now.since(t) / deflate_s.max(1e-3)).clamp(0.0, 1.0);
            }
        }
        out
    }
}

/// State of a unit as seen by a radar detector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RadarState {
    /// Patrolling.
    Patrol,
    /// Pursuing someone (not necessarily you).
    Pursuit,
}

/// One radar detector alert.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RadarBlip {
    /// Unit.
    pub unit: UnitId,
    /// Distance, m.
    pub distance_m: f32,
    /// Unit direction from the racer (XY unit vector).
    pub dir: [f32; 2],
    /// Unit state.
    pub state: RadarState,
}

/// One radio-log line as seen through a scanner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RadioLine {
    /// Sim time spoken.
    pub time: SimTime,
    /// Speaking unit (`None` = dispatch).
    pub unit: Option<UnitId>,
    /// Text.
    pub text: String,
    /// Pacing tier the line was raised with (row 79).
    #[serde(default = "crate::radio::default_tier")]
    pub tier: crate::radio::RadioTier,
    /// Estimated transmission length, s (the channel was busy this long plus the radio
    /// config's `min_gap_s`).
    #[serde(default)]
    pub duration_s: f32,
    /// Emergency traffic that cut the line before it.
    #[serde(default)]
    pub cut_in: bool,
}

/// Helicopter view for renderers / the captain.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HeloView {
    /// Helicopter unit.
    pub unit: UnitId,
    /// World position, m.
    pub pos: [f32; 3],
    /// Spotlight aim point: the racer's last known position (where it is while the
    /// helicopter has visual); z is the racer's current height, m.
    pub spotlight_target: [f32; 3],
    /// Spotlight cone half angle, rad.
    pub cone_half_angle_rad: f32,
    /// Racer being tracked.
    pub target: EntityId,
    /// The helicopter currently has visual on `target` (spotlight on the car rather than on
    /// where it was last seen).
    pub has_visual: bool,
}

/// Compass word for a direction (radio flavour). Pure comparisons, no trig.
pub fn compass(dir: [f32; 2]) -> &'static str {
    if dir[0].abs() < 1e-3 && dir[1].abs() < 1e-3 {
        return "stationary";
    }
    if dir[1].abs() >= dir[0].abs() {
        if dir[1] > 0.0 {
            "northbound"
        } else {
            "southbound"
        }
    } else if dir[0] > 0.0 {
        "eastbound"
    } else {
        "westbound"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::lerp2;

    fn rules() -> SiteRules {
        SiteRules {
            min_ahead_m: 300.0,
            max_ahead_m: 700.0,
            min_sight_m: 120.0,
            highway_ok: true,
        }
    }

    #[test]
    fn sites_respect_sight_and_distance_everywhere() {
        let g = crate::city::procedural(&crate::city::ProcGenConfig::small()).0;
        let mut placed = 0;
        let mut rejected_blind = 0;
        for (i, seg) in g.segments.iter().enumerate() {
            for dir in [1.0f32, -1.0] {
                let a = g.node(seg.a).pos;
                let b = g.node(seg.b).pos;
                let p = lerp2(a, b, 0.5);
                let v = scale(sub(b, a), dir);
                match plan_site(&g, p, v, &rules(), None) {
                    Ok(site) => {
                        placed += 1;
                        assert!(site.sight_m >= 120.0, "seg {i}: sight {}", site.sight_m);
                        assert!(site.ahead_m >= 300.0 && site.ahead_m <= 700.0);
                        // Independent check: either the site is ≥120 m from its approach node,
                        // or every road into that node continues straight.
                        let recheck =
                            g.approach_sight_m(site.segment, site.from_node, site.d_m, 1000.0);
                        assert!(recheck >= 120.0);
                        assert!(site.d_m >= NODE_CLEARANCE_M);
                    }
                    Err(SiteError::BlindCorner) => rejected_blind += 1,
                    Err(_) => {}
                }
            }
        }
        assert!(placed > 20, "placed {placed}");
        let _ = rejected_blind;
    }

    #[test]
    fn spike_wheels_and_puncture_ramp() {
        let site = ToolSite {
            segment: SegmentId(0),
            from_node: NodeId(0),
            d_m: 50.0,
            pos: [0.0, 50.0],
            dir: [0.0, 1.0],
            sight_m: 200.0,
            ahead_m: 300.0,
        };
        let s = SpikeStrip::across(
            ToolId(1),
            EntityId(1),
            site,
            7.0,
            None,
            SimTime::from_secs(100.0),
        );
        assert_eq!(s.wheels_crossing([0.0, 40.0], [0.0, 60.0], 0.8), [true; 4]);
        // Only the right-hand wheels clip the strip's end.
        let hit = s.wheels_crossing([-4.0, 40.0], [-4.0, 60.0], 0.8);
        assert_eq!(hit, [false, true, false, true]);
        assert_eq!(
            s.wheels_crossing([10.0, 40.0], [10.0, 60.0], 0.8),
            [false; 4]
        );
        let mut p = Puncture::default();
        p.hit([true, false, false, false], SimTime::from_secs(10.0));
        let l = p.level(SimTime::from_secs(12.5), 5.0);
        assert!((l[0] - 0.5).abs() < 1e-5 && l[1] == 0.0);
        assert_eq!(p.level(SimTime::from_secs(20.0), 5.0)[0], 1.0);
    }

    fn site_north() -> ToolSite {
        ToolSite {
            segment: SegmentId(0),
            from_node: NodeId(0),
            d_m: 50.0,
            pos: [0.0, 50.0],
            dir: [0.0, 1.0],
            sight_m: 200.0,
            ahead_m: 300.0,
        }
    }

    /// Lateral spans `(lo, hi)` (+x = right of north) of the boxes, sorted.
    fn spans(cs: &[StaticCollider]) -> Vec<(f32, f32)> {
        let mut v: Vec<(f32, f32)> = cs
            .iter()
            .map(|c| {
                (
                    c.pos[0] - c.half_extents_m[0],
                    c.pos[0] + c.half_extents_m[0],
                )
            })
            .collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0));
        v
    }

    #[test]
    fn roadblock_spans_stop_to_stop_with_one_barricaded_gap() {
        use crate::section::Section;
        // A 14 m road with sidewalks, buildings 4 m past them on the left and 6 m on the right.
        let sec = Section {
            paved_lo: -10.0,
            paved_hi: 10.0,
            road_lo: -7.0,
            road_hi: 7.0,
            stop_lo: Some(-14.0),
            stop_hi: Some(16.0),
            opposing: false,
            on_road: true,
        };
        assert!(sec.sealable(VERGE_REACH_M));
        for gap in [GapPick::Left, GapPick::Centre, GapPick::Right] {
            let lay = roadblock_layout(&site_north(), &sec, [1.2, 0.3, 0.5], 400.0, gap);
            let sp = spans(&lay.barriers);
            // Into both stops.
            assert!(sp[0].0 <= -14.0 - STOP_OVERLAP_M + 1e-3);
            assert!(sp.last().unwrap().1 >= 16.0 + STOP_OVERLAP_M - 1e-3);
            // Neighbours overlap, except across exactly one gap of GAP_W_M.
            let mut gaps = Vec::new();
            for w in sp.windows(2) {
                if w[1].0 > w[0].1 + 1e-3 {
                    gaps.push((w[0].1, w[1].0));
                } else {
                    assert!(w[0].1 - w[1].0 >= BARRIER_OVERLAP_M - 1e-3, "{w:?}");
                }
            }
            assert_eq!(gaps.len(), 1, "{gap:?}: {sp:?}");
            let (g0, g1) = gaps[0];
            assert!((g1 - g0 - GAP_W_M).abs() < 1e-3);
            assert!(
                g0 >= sec.road_lo - 1e-3 && g1 <= sec.road_hi + 1e-3,
                "gap on the road"
            );
            // The barricade fills it.
            let b = lay.barricade.collider;
            assert!(b.pos[0] - b.half_extents_m[0] < g0 && b.pos[0] + b.half_extents_m[0] > g1);
            // All on one line, 4 m past the site.
            assert!(lay.barriers.iter().all(|c| (c.pos[1] - 54.0).abs() < 1e-3));
            // Cars behind the line, never behind the gap.
            assert!(lay.unit_spots.len() >= 3);
            for (p, _) in &lay.unit_spots {
                assert!(p[1] > 54.0 + 1.0);
                assert!(p[0] + UNIT_HALF_L_M <= g0 - 0.5 || p[0] - UNIT_HALF_L_M >= g1 + 0.5);
            }
        }
        // No stop within reach on one side, or a far carriageway: not sealable.
        assert!(!Section {
            stop_hi: None,
            ..sec
        }
        .sealable(VERGE_REACH_M));
        assert!(!Section {
            stop_hi: Some(30.0),
            ..sec
        }
        .sealable(VERGE_REACH_M));
        assert!(!Section {
            opposing: true,
            ..sec
        }
        .sealable(VERGE_REACH_M));
    }

    #[test]
    fn spike_strip_covers_the_paving_and_ends_under_its_car() {
        use crate::section::Section;
        // Barrier on the left at the paved edge, open verge on the right.
        let sec = Section {
            paved_lo: -2.0,
            paved_hi: 9.0,
            road_lo: -2.0,
            road_hi: 9.0,
            stop_lo: Some(-2.0),
            stop_hi: None,
            opposing: false,
            on_road: true,
        };
        let lay = spike_layout(&site_north(), &sec);
        assert!((lay.a[0] - -2.0).abs() < 1e-4, "{:?}", lay.a);
        let car = lay.post[0];
        assert!(car + UNIT_HALF_W_M <= 9.0 + 1e-4);
        assert!(lay.b[0] > car - UNIT_HALF_W_M && lay.b[0] < car);
        assert_eq!(lay.heading, [0.0, 1.0]);
    }

    #[test]
    fn compass_words() {
        assert_eq!(compass([0.0, 1.0]), "northbound");
        assert_eq!(compass([-1.0, 0.2]), "westbound");
    }
}
