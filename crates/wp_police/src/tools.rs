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

/// Build the barrier colliders for a roadblock: barriers across the road with gaps where the
/// units are parked.
pub fn roadblock_barriers(
    site: &ToolSite,
    half_extents: [f32; 3],
    mass_kg: f32,
    road_half_width_m: f32,
) -> Vec<StaticCollider> {
    let across = perp_left(site.dir);
    let mut out = Vec::new();
    let step = half_extents[0] * 2.0 + 0.4;
    let mut x = -road_half_width_m + half_extents[0];
    while x <= road_half_width_m - half_extents[0] + 1e-3 {
        let c = add(site.pos, add(scale(across, x), scale(site.dir, 4.0)));
        out.push(StaticCollider {
            pos: [c[0], c[1], half_extents[2]],
            half_extents_m: half_extents,
            forward: site.dir,
            mass_kg,
        });
        x += step;
    }
    out
}

/// Parking spots for `n` roadblock units (angled across the road).
pub fn roadblock_unit_spots(
    site: &ToolSite,
    n: u32,
    road_half_width_m: f32,
) -> Vec<([f32; 2], [f32; 2])> {
    let across = perp_left(site.dir);
    (0..n)
        .map(|i| {
            let k = if n == 1 {
                0.5
            } else {
                i as f32 / (n - 1) as f32
            };
            let x = -road_half_width_m * 0.7 + k * road_half_width_m * 1.4;
            (add(site.pos, scale(across, x)), across)
        })
        .collect()
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

    #[test]
    fn compass_words() {
        assert_eq!(compass([0.0, 1.0]), "northbound");
        assert_eq!(compass([-1.0, 0.2]), "westbound");
    }
}
