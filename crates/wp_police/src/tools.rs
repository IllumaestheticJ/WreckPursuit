//! Police tools (roadblocks, spike strips, helicopter view) and racer-side gadget queries
//! (radar detector, scanner).
//!
//! Placement rules (enforced by [`plan_site`], used by both the AI and captain paths):
//! - on the racer's predicted path, between `min_ahead_m` and `max_ahead_m` ahead;
//! - never within 15 m of an intersection;
//! - approach sight distance ([`RoadGraph::approach_sight_m`]) ≥ `min_sight_m` — never around a
//!   blind corner;
//! - roadblocks never on a highway when the wanted level is above `highway_max_level`.
//!
//! Roadblocks and spike strips are arcade-classic-style (owner 2026-10-31): a roadblock is a
//! line of police cars parked at an angle across the road with gaps between them
//! ([`roadblock_layout`], tiers by wanted level in `tools.toml`); a spike strip lies across the
//! racer's lanes from under its unit's car to the kerb ([`spike_layout`]). Both appear already
//! set up, out of every racer's view.

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

/// One police car of a roadblock: where it parks, which way it points and what it is.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BlockCar {
    /// Parking spot (car centre), m.
    pub pos: [f32; 2],
    /// Heading (unit vector): angled across the road, nose toward the racer.
    pub heading: [f32; 2],
    /// A heavy unit (police SUV / armoured type): much harder to shove aside. `wp_sim` gives
    /// it the heaviest police model it has.
    pub heavy: bool,
    /// Line it stands in: 0 = the front line, 1 = the second line behind it.
    pub line: u8,
}

/// A way through a roadblock: the open road between two cars (or between a car and the
/// kerb), measured along the car line.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BlockGap {
    /// Left end of the gap (seen by the approaching racer), m.
    pub a: [f32; 2],
    /// Right end, m.
    pub b: [f32; 2],
    /// Clear width, m (planned with [`CAR_PLAN_L_M`] × [`CAR_PLAN_W_M`] footprints, so a
    /// slightly smaller car leaves a little more).
    pub width_m: f32,
    /// A spike strip lies across it, in front of the car line.
    pub spiked: bool,
    /// Line it belongs to (see [`BlockCar::line`]).
    pub line: u8,
}

/// A roadblock (owner 2026-10-31, arcade-classic-style roadblocks): a line of police cars
/// parked across the road at an angle, nose to nose or staggered, lights flashing. The ways
/// through are the gaps between the cars (car-width or wider; spiked at higher heat) and the
/// cars themselves: ramming the light end of a car shoves it aside at a cost in speed and
/// damage; heavy units barely move. The cars are ordinary police units ([`crate::Unit`]) that
/// `wp_sim` makes real vehicles parked with the brakes held; once the racer is past they
/// rejoin the pursuit or drive off.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Roadblock {
    /// Id.
    pub id: ToolId,
    /// Racer it targets.
    pub racer: EntityId,
    /// Site.
    pub site: ToolSite,
    /// Parked units (filled as the cars appear, out of every racer's view).
    pub units: Vec<UnitId>,
    /// Removed at.
    pub expires: SimTime,
    /// Placed by a race profile ([`crate::race`]): stays when the racer's pursuit ends and
    /// does not count against the AI captain's budget.
    #[serde(default)]
    pub scripted: bool,
    /// Road height at the site, m, when the city's geometry is known.
    #[serde(default)]
    pub z_m: Option<f32>,
    /// Wanted level (1..=5) the block was built for (its tier, `tools.toml` `[[roadblock.tiers]]`).
    #[serde(default)]
    pub level: u8,
    /// The planned cars, front line first, left to right.
    #[serde(default)]
    pub cars: Vec<BlockCar>,
    /// The ways through, front line first, left to right.
    #[serde(default)]
    pub gaps: Vec<BlockGap>,
    /// Spike strips across the spiked gaps, `[a, b]` (laid as [`SpikeStrip`]s with
    /// [`SpikeStrip::block`] set once every car stands).
    #[serde(default)]
    pub gap_strips: Vec<[[f32; 2]; 2]>,
    /// Traffic cones in front of the cars (drawing only), m.
    #[serde(default)]
    pub cones: Vec<[f32; 2]>,
    /// Burning road flares on the approach (drawing only: a red glow seen from far off), m.
    #[serde(default)]
    pub flares: Vec<[f32; 2]>,
    /// Every car stands at its spot (they appear together, out of every racer's view) and the
    /// gap strips are down. Until then nothing of the block is on the road or the map.
    #[serde(default)]
    pub set_up: bool,
}

impl Roadblock {
    /// Heavy units in the block.
    pub fn heavy_cars(&self) -> usize {
        self.cars.iter().filter(|c| c.heavy).count()
    }

    /// How the radio names the block: "two-car roadblock", "three-car roadblock with spikes",
    /// "heavy roadblock with spikes, two lines".
    pub fn radio_name(&self) -> String {
        let n = self.cars.iter().filter(|c| c.line == 0).count();
        let mut s = if self.heavy_cars() > 0 {
            "heavy roadblock".to_string()
        } else {
            let count = match n {
                0 | 1 => "one-car",
                2 => "two-car",
                3 => "three-car",
                4 => "four-car",
                _ => "wide",
            };
            format!("{count} roadblock")
        };
        if self.gaps.iter().any(|g| g.spiked) {
            s.push_str(" with spikes");
        }
        if self.cars.iter().any(|c| c.line > 0) {
            s.push_str(", two lines");
        }
        s
    }

    /// Distance from `p` to the front car line along the block's travel direction, m
    /// (negative: past it).
    pub fn ahead_of(&self, p: [f32; 2]) -> f32 {
        let line = add(self.site.pos, scale(self.site.dir, BLOCK_ALONG_M));
        dot2(sub(line, p), self.site.dir)
    }

    /// Distance past the last car line along the travel direction a racer at `p` is, m
    /// (negative: not past it yet).
    pub fn past_by(&self, p: [f32; 2]) -> f32 {
        let last = self
            .cars
            .iter()
            .map(|c| dot2(sub(c.pos, self.site.pos), self.site.dir))
            .fold(BLOCK_ALONG_M, f32::max);
        dot2(sub(p, self.site.pos), self.site.dir) - last
    }
}

/// Dot product.
fn dot2(a: [f32; 2], b: [f32; 2]) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}

/// What a roadblock is made of at one wanted level (`tools.toml` `[[roadblock.tiers]]`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockRecipe {
    /// Cars in the front line (more on a road too wide for them, fewer on one too narrow).
    pub cars: u32,
    /// How many of them are heavy units (the ones nearest the middle of the road first).
    pub heavy: u32,
    /// Planned width of each gap between cars, m (never below [`GAP_MIN_M`]).
    pub gap_m: f32,
    /// How many gaps get a spike strip (0 = none; more than there are = all).
    pub spiked_gaps: u32,
    /// Chance (0..=1) of a second, staggered line [`SECOND_LINE_M`] behind the first, with
    /// its gaps off the front line's.
    pub second_line_chance: f32,
}

/// Where a single-car block leaves its gap, and which gaps get spikes when only some do
/// (picked by tool id: no draw on the police RNG).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GapPick {
    /// At the left edge of the carriageway / the leftmost gaps.
    Left,
    /// The middle.
    Centre,
    /// At the right edge / the rightmost gaps.
    Right,
}

/// The front car line stands this far past the site point along the racer's travel, m.
pub const BLOCK_ALONG_M: f32 = 4.0;
/// A second line stands this far behind the first, m.
pub const SECOND_LINE_M: f32 = 14.0;
/// Odd cars of a line of three or more stand this much further along (staggered), m.
pub const STAGGER_M: f32 = 1.2;
/// Car size the layout plans with (the largest police models: 5.18 m × 2.04 m), m.
pub const CAR_PLAN_L_M: f32 = 5.2;
/// See [`CAR_PLAN_L_M`].
pub const CAR_PLAN_W_M: f32 = 2.0;
/// Widest a parked car's footprint across the road is planned (near the largest it can be,
/// `hypot(CAR_PLAN_L_M, CAR_PLAN_W_M)` = 5.57 m), m.
pub const FOOT_MAX_M: f32 = 5.4;
/// Narrowest footprint planned: a car still angled about 30° across the line of travel
/// (fewer cars on a road too narrow for more), m.
pub const FOOT_MIN_M: f32 = 4.4;
/// Narrowest gap a block leaves: one car width (about 2.0 m) and a quarter metre each side, m.
pub const GAP_MIN_M: f32 = 2.5;
/// Most cars in one line.
pub const MAX_LINE_CARS: u32 = 6;
/// A gap's spike strip lies this far in front of the car line (toward the racer), m.
pub const GAP_SPIKE_AHEAD_M: f32 = 3.5;
/// ... and reaches this far past each end of the gap (under the cars' noses), m.
pub const GAP_SPIKE_OVERHANG_M: f32 = 1.0;
/// Road flares burn this far before the car line, m.
pub const FLARE_AHEAD_M: f32 = 22.0;
/// Within this of the paved road a side counts as within reach (sections, spike extras), m.
pub const VERGE_REACH_M: f32 = 12.0;
/// A spike strip's end lies this far under its unit's car (its inner side), m.
pub const SPIKE_UNDER_CAR_M: f32 = 0.15;
/// On a two-way road a spike car stands this far left of the centre line (its strip then
/// starts 0.3 m right of the line), m.
pub const TWO_WAY_CAR_LEFT_M: f32 = 0.5;
/// Half width of a parked police car, m.
pub const UNIT_HALF_W_M: f32 = 0.95;
/// Half length of a parked police car, m.
pub const UNIT_HALF_L_M: f32 = 2.45;

/// A roadblock laid out across a [`crate::section::Section`] (see [`roadblock_layout`]).
#[derive(Debug, Clone, PartialEq)]
pub struct BlockLayout {
    /// The cars, front line first, left to right.
    pub cars: Vec<BlockCar>,
    /// The ways through, front line first, left to right.
    pub gaps: Vec<BlockGap>,
    /// Spike strips across the spiked gaps, `[a, b]`.
    pub gap_strips: Vec<[[f32; 2]; 2]>,
    /// Cones (drawing only).
    pub cones: Vec<[f32; 2]>,
    /// Road flares (drawing only).
    pub flares: Vec<[f32; 2]>,
}

/// Unit `(across, along)` direction of a car whose footprint across the road is `foot_m`:
/// `across · L + along · W = foot` with `across² + along² = 1`, the solution with the car more
/// across than along the road (bisection on `along`, no trig: deterministic everywhere).
fn angle_for_footprint(foot_m: f32) -> (f32, f32) {
    let (l, w) = (CAR_PLAN_L_M, CAR_PLAN_W_M);
    let foot = |s: f32| (1.0 - s * s).max(0.0).sqrt() * l + s * w;
    // The footprint peaks at along = w / hypot(l, w); search from there to fully along.
    let peak = w / (l * l + w * w).sqrt();
    let (mut lo, mut hi) = (peak, 1.0f32);
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if foot(mid) > foot_m {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let s = 0.5 * (lo + hi);
    ((1.0 - s * s).max(0.0).sqrt(), s)
}

/// One line of cars across `[lo, hi]` (lateral offsets, + right of travel): `n` cars and the
/// gaps between them (about `gap_m` wide; wider, and open lanes at the kerbs, on a road too
/// wide for `n`), fewer cars (one car and a gap at a kerb at least) on one too narrow. Returns the cars' `(centre, footprint)` and the gaps'
/// `(left, right)`, left to right.
fn line_layout(
    lo: f32,
    hi: f32,
    n: u32,
    gap_m: f32,
    edge: GapPick,
) -> (Vec<(f32, f32)>, Vec<(f32, f32)>) {
    let w = (hi - lo).max(0.0);
    let gap_m = gap_m.max(GAP_MIN_M);
    // Fewer cars where the road cannot hold them angled with car-width gaps between them.
    let mut n = n.clamp(1, MAX_LINE_CARS);
    while n > 1 && n as f32 * FOOT_MIN_M + (n as f32 - 1.0) * GAP_MIN_M > w {
        n -= 1;
    }
    if n == 1 {
        // One car and a gap at a kerb (the side picked).
        let lo_f = FOOT_MIN_M.min((w - GAP_MIN_M).max(CAR_PLAN_W_M));
        let f = (w - gap_m).clamp(lo_f, FOOT_MAX_M);
        let c = match edge {
            GapPick::Left => hi - 0.5 * f,
            GapPick::Right | GapPick::Centre => lo + 0.5 * f,
        };
        let gap = if c - 0.5 * f - lo > hi - (c + 0.5 * f) {
            (lo, c - 0.5 * f)
        } else {
            (c + 0.5 * f, hi)
        };
        return (vec![(c, f)], vec![gap]);
    }
    let k = n as f32 - 1.0;
    // Gaps as planned, closed toward the minimum where the cars would be too narrow.
    let mut g = gap_m;
    if (w - k * g) / (n as f32) < FOOT_MIN_M {
        g = ((w - n as f32 * FOOT_MIN_M) / k).max(GAP_MIN_M);
    }
    let mut f = ((w - k * g) / n as f32).max(FOOT_MIN_M);
    let mut edge_m = 0.0;
    if f > FOOT_MAX_M {
        // A wide road: the cars as wide as they get, the gaps up to half as wide again, the
        // rest open at the kerbs (or shared by the gaps when too narrow to drive).
        f = FOOT_MAX_M;
        let spare = w - n as f32 * f - k * g;
        let widen = spare.min(k * 0.5 * gap_m);
        g += widen / k;
        let rest = 0.5 * (spare - widen);
        if rest >= GAP_MIN_M {
            edge_m = rest;
        } else {
            g += 2.0 * rest / k;
        }
    }
    let mut cars = Vec::new();
    let mut gaps = Vec::new();
    if edge_m > 0.0 {
        gaps.push((lo, lo + edge_m));
    }
    for i in 0..n {
        let x0 = lo + edge_m + i as f32 * (f + g);
        cars.push((x0 + 0.5 * f, f));
        if i + 1 < n {
            gaps.push((x0 + f, x0 + f + g));
        }
    }
    if edge_m > 0.0 {
        gaps.push((hi - edge_m, hi));
    }
    (cars, gaps)
}

/// Lay a roadblock across `sec` at `site` for `recipe` (owner 2026-10-31, arcade-classic
/// style): the front line of cars [`BLOCK_ALONG_M`] past the site, kerb to kerb across the
/// carriageway, each car angled across the road (nose toward the racer, neighbours nose to
/// nose; odd cars of three or more staggered [`STAGGER_M`] further along), the gaps between
/// them about `recipe.gap_m` wide. The heavy cars are the ones nearest the middle. With
/// `second_line` a second line of one car fewer stands [`SECOND_LINE_M`] behind, its gaps off
/// the front line's. `pick` chooses which gaps get spikes when only some do (and the kerb a
/// one-car block leaves open). Cones stand in front of every car and flares burn on the
/// approach.
pub fn roadblock_layout(
    site: &ToolSite,
    sec: &crate::section::Section,
    recipe: &BlockRecipe,
    second_line: bool,
    pick: GapPick,
) -> BlockLayout {
    let right = [site.dir[1], -site.dir[0]];
    let at = |x: f32, y: f32| add(add(site.pos, scale(site.dir, y)), scale(right, x));
    let (lo, hi) = (sec.road_lo, sec.road_hi.max(sec.road_lo + 2.0 * GAP_MIN_M));
    let mut cars = Vec::new();
    let mut gaps = Vec::new();
    let mut strips = Vec::new();
    let mut cones = Vec::new();
    let lines: &[(u8, u32, f32)] = if second_line {
        &[
            (0, recipe.cars, BLOCK_ALONG_M),
            (1, 0, BLOCK_ALONG_M + SECOND_LINE_M),
        ]
    } else {
        &[(0, recipe.cars, BLOCK_ALONG_M)]
    };
    let mut front_cars = 0u32;
    for &(line, n, y) in lines {
        let n = if line == 0 {
            n
        } else {
            front_cars.saturating_sub(1).max(2)
        };
        let (line_cars, line_gaps) = line_layout(lo, hi, n, recipe.gap_m, pick);
        if line == 0 {
            front_cars = line_cars.len() as u32;
        }
        // Heavy cars: nearest the middle of the road first (front line only).
        let mid = 0.5 * (lo + hi);
        let mut order: Vec<usize> = (0..line_cars.len()).collect();
        order.sort_by(|a, b| {
            (line_cars[*a].0 - mid)
                .abs()
                .total_cmp(&(line_cars[*b].0 - mid).abs())
                .then(a.cmp(b))
        });
        let heavy_n = if line == 0 { recipe.heavy as usize } else { 0 };
        let stagger = line_cars.len() >= 3;
        for (k, &(x, f)) in line_cars.iter().enumerate() {
            let (across, along) = angle_for_footprint(f);
            // Neighbours nose to nose: even cars point right, odd ones left; noses toward the
            // racer.
            let side = if k % 2 == 0 { 1.0 } else { -1.0 };
            let heading = add(scale(right, side * across), scale(site.dir, -along));
            let yk = y + if stagger && k % 2 == 1 {
                STAGGER_M
            } else {
                0.0
            };
            let heavy = order.iter().take(heavy_n).any(|i| *i == k);
            cars.push(BlockCar {
                pos: at(x, yk),
                heading,
                heavy,
                line,
            });
            // Two cones before each car's front, inside its footprint.
            for dx in [-0.25f32, 0.25] {
                cones.push(at(x + dx * f, y - GAP_SPIKE_AHEAD_M - 1.5));
            }
        }
        // Spiked gaps: all, or `spiked_gaps` of them from the picked side.
        let ng = line_gaps.len();
        let want = (recipe.spiked_gaps as usize).min(ng);
        let spiked: Vec<bool> = (0..ng)
            .map(|i| {
                if want >= ng {
                    return true;
                }
                match pick {
                    GapPick::Left => i < want,
                    GapPick::Right => i >= ng - want,
                    GapPick::Centre => {
                        let first = (ng - want) / 2;
                        i >= first && i < first + want
                    }
                }
            })
            .collect();
        for (i, &(x0, x1)) in line_gaps.iter().enumerate() {
            gaps.push(BlockGap {
                a: at(x0, y),
                b: at(x1, y),
                width_m: x1 - x0,
                spiked: spiked[i],
                line,
            });
            if spiked[i] {
                let ys = y - GAP_SPIKE_AHEAD_M;
                strips.push([
                    at((x0 - GAP_SPIKE_OVERHANG_M).max(lo - 0.2), ys),
                    at((x1 + GAP_SPIKE_OVERHANG_M).min(hi + 0.2), ys),
                ]);
            }
        }
    }
    // Flares on the approach: at both kerbs and in front of every front-line gap.
    let yf = BLOCK_ALONG_M - FLARE_AHEAD_M;
    let mut flares = vec![at(lo + 0.5, yf), at(hi - 0.5, yf)];
    for g in gaps.iter().filter(|g| g.line == 0) {
        let x = 0.5 * (dot2(sub(g.a, site.pos), right) + dot2(sub(g.b, site.pos), right));
        flares.push(at(x, yf + 6.0));
    }
    BlockLayout {
        cars,
        gaps,
        gap_strips: strips,
        cones,
        flares,
    }
}

/// A spike strip laid out across a [`crate::section::Section`] (see [`spike_layout`]).
#[derive(Debug, Clone, PartialEq)]
pub struct SpikeLayout {
    /// Strip end on the left: under the edge of its car, [`SPIKE_UNDER_CAR_M`] in, m.
    pub a: [f32; 2],
    /// Strip end on the right: the kerb of the racer's carriageway, m.
    pub b: [f32; 2],
    /// Where its car parks, m: at the inner (left) end of the strip.
    pub post: [f32; 2],
    /// Parked heading: the racer's travel direction.
    pub heading: [f32; 2],
    /// Burning road flares on the approach (drawing only), m.
    pub flares: Vec<[f32; 2]>,
}

/// Lay a spike strip across `sec` at `site` (owner 2026-10-31, arcade-classic style): across
/// the racer's lanes, from under its unit's car to the kerb. On a two-way road (`two_way`) the
/// car stands astride the centre line and the strip covers the racer's half; on a one-way road
/// the car stands at the left kerb and the strip covers the rest of the carriageway. The ways
/// past are the other lane (two-way roads), off the road (sidewalk or verge) and ramming the
/// car aside. Flares burn on the approach.
pub fn spike_layout(site: &ToolSite, sec: &crate::section::Section, two_way: bool) -> SpikeLayout {
    let right = [site.dir[1], -site.dir[0]];
    let at = |x: f32, y: f32| add(add(site.pos, scale(site.dir, y)), scale(right, x));
    let (lo, hi) = (sec.road_lo, sec.road_hi);
    let car = if two_way {
        // Astride the centre line, a little into the oncoming lane: the strip starts just
        // right of the line, so a car on the line (or in its own lane) meets it.
        // The site lies on the road's line: on a two-way road, its centre line.
        (0.0 - TWO_WAY_CAR_LEFT_M)
            .min(hi - UNIT_HALF_W_M - 1.0)
            .max(lo + UNIT_HALF_W_M)
    } else {
        lo + UNIT_HALF_W_M + 0.1
    };
    let start = car + UNIT_HALF_W_M - SPIKE_UNDER_CAR_M;
    let end = (hi + 0.2).max(start + 0.5);
    SpikeLayout {
        a: at(start, 0.0),
        b: at(end, 0.0),
        post: at(car, 0.0),
        heading: site.dir,
        flares: vec![at(car, -FLARE_AHEAD_M), at(hi - 0.5, -FLARE_AHEAD_M + 4.0)],
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
    /// More segments of the same strip, `[a, b]` (owner 2026-10-28): other paved runs across
    /// the road between the hard stops (a ramp beside the carriageway past an open gore). All
    /// belong to the same unit and go with it.
    #[serde(default)]
    pub extra: Vec<[[f32; 2]; 2]>,
    /// The roadblock this strip lies in a gap of (owner 2026-10-31: spiked gaps at higher
    /// heat). Such a strip has no unit of its own: it lies while its block stands and goes
    /// with it.
    #[serde(default)]
    pub block: Option<ToolId>,
    /// Burning road flares on the approach (drawing only), m.
    #[serde(default)]
    pub flares: Vec<[f32; 2]>,
}

impl SpikeStrip {
    /// Every segment of the strip, `a`–`b` first.
    pub fn segments(&self) -> impl Iterator<Item = ([f32; 2], [f32; 2])> + '_ {
        std::iter::once((self.a, self.b)).chain(self.extra.iter().map(|s| (s[0], s[1])))
    }

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
            extra: Vec::new(),
            block: None,
            flares: Vec::new(),
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
            extra: Vec::new(),
            block: None,
            flares: Vec::new(),
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
            if self
                .segments()
                .any(|(a, b)| segments_intersect(add(p0, off), add(p1, off), a, b))
            {
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

    fn recipe(cars: u32, heavy: u32, gap_m: f32, spiked: u32) -> BlockRecipe {
        BlockRecipe {
            cars,
            heavy,
            gap_m,
            spiked_gaps: spiked,
            second_line_chance: 0.0,
        }
    }

    /// Lateral extent `(lo, hi)` (+x = right of north) of a planned car's footprint.
    fn foot(c: &BlockCar) -> (f32, f32) {
        let half = 0.5 * (c.heading[0].abs() * CAR_PLAN_L_M + c.heading[1].abs() * CAR_PLAN_W_M);
        (c.pos[0] - half, c.pos[0] + half)
    }

    #[test]
    fn footprint_angle_solves_the_footprint() {
        for f in [2.4f32, 3.0, 4.0, 4.4, 5.0, 5.4] {
            let (c, s) = angle_for_footprint(f);
            assert!((c * c + s * s - 1.0).abs() < 1e-4);
            let got = c * CAR_PLAN_L_M + s * CAR_PLAN_W_M;
            assert!((got - f).abs() < 1e-3, "{f}: {got}");
            assert!(c > 0.0 && s > 0.0);
        }
    }

    #[test]
    fn roadblock_tiers_lay_angled_cars_with_gaps_between_them() {
        use crate::section::Section;
        // A 19 m boulevard.
        let sec = Section::plain(-9.5, 9.5);
        for (r, cars, heavy, spiked) in [
            (recipe(2, 0, 4.5, 0), 2, 0, 0),
            (recipe(3, 0, 3.0, 1), 3, 0, 1),
            (recipe(3, 2, 2.7, 9), 3, 2, 2),
        ] {
            let lay = roadblock_layout(&site_north(), &sec, &r, false, GapPick::Centre);
            assert_eq!(lay.cars.len(), cars, "{r:?}");
            assert_eq!(lay.cars.iter().filter(|c| c.heavy).count(), heavy);
            assert_eq!(lay.gaps.len(), cars - 1);
            assert_eq!(lay.gaps.iter().filter(|g| g.spiked).count(), spiked);
            assert_eq!(lay.gap_strips.len(), spiked);
            // Cars kerb to kerb, angled across the road, noses toward the racer; the gaps
            // between neighbours as planned, at least a car width.
            let mut feet: Vec<(f32, f32)> = lay.cars.iter().map(foot).collect();
            feet.sort_by(|a, b| a.0.total_cmp(&b.0));
            assert!((feet[0].0 - -9.5).abs() < 0.05, "{feet:?}");
            assert!((feet.last().unwrap().1 - 9.5).abs() < 0.05, "{feet:?}");
            for (w, g) in feet.windows(2).zip(&lay.gaps) {
                let open = w[1].0 - w[0].1;
                assert!((open - g.width_m).abs() < 0.05, "{open} vs {g:?}");
                assert!(g.width_m >= GAP_MIN_M - 1e-3);
            }
            for c in &lay.cars {
                assert!(c.heading[0].abs() > 0.5, "angled across the road: {c:?}");
                assert!(c.heading[1] < 0.0, "nose toward the racer: {c:?}");
                assert!(c.pos[1] >= 54.0 - 1e-3 && c.pos[1] <= 54.0 + STAGGER_M + 1e-3);
            }
            // Neighbours point at each other (nose to nose).
            assert!(lay.cars[0].heading[0] > 0.0 && lay.cars[1].heading[0] < 0.0);
            // Heavy units in the middle.
            if heavy > 0 {
                let mid = lay
                    .cars
                    .iter()
                    .min_by(|a, b| a.pos[0].abs().total_cmp(&b.pos[0].abs()));
                assert!(mid.unwrap().heavy);
            }
            // Gap strips lie across their gap, in front of the line.
            for (s, g) in lay
                .gap_strips
                .iter()
                .zip(lay.gaps.iter().filter(|g| g.spiked))
            {
                assert!(s[0][0] < g.a[0] && s[1][0] > g.b[0]);
                assert!((s[0][1] - (54.0 - GAP_SPIKE_AHEAD_M)).abs() < 1e-3);
            }
            assert!(!lay.flares.is_empty() && !lay.cones.is_empty());
        }
        // The wide-gap tier leaves a gap well over a car width.
        let lay = roadblock_layout(
            &site_north(),
            &sec,
            &recipe(2, 0, 4.5, 0),
            false,
            GapPick::Centre,
        );
        assert!(lay.gaps[0].width_m >= 4.5);
    }

    #[test]
    fn roads_too_wide_or_narrow_get_more_or_fewer_cars() {
        use crate::section::Section;
        let r = recipe(2, 0, 4.5, 0);
        // Too wide for two cars: a wider gap and an open lane at each kerb.
        let wide = roadblock_layout(
            &site_north(),
            &Section::plain(-12.0, 12.0),
            &r,
            false,
            GapPick::Centre,
        );
        assert_eq!(wide.cars.len(), 2);
        assert_eq!(wide.gaps.len(), 3);
        assert!(wide.gaps.iter().all(|g| g.width_m >= GAP_MIN_M - 1e-3));
        // Too narrow for three: two.
        let two = roadblock_layout(
            &site_north(),
            &Section::plain(-7.0, 7.0),
            &recipe(3, 0, 3.0, 1),
            false,
            GapPick::Centre,
        );
        assert_eq!(two.cars.len(), 2);
        let narrow = roadblock_layout(
            &site_north(),
            &Section::plain(-3.0, 3.0),
            &r,
            false,
            GapPick::Left,
        );
        assert_eq!(narrow.cars.len(), 1);
        assert_eq!(narrow.gaps.len(), 1);
        assert!(narrow.gaps[0].width_m >= GAP_MIN_M - 1e-3);
        for lay in [&wide, &narrow] {
            for c in &lay.cars {
                let (a, b) = foot(c);
                assert!(a >= -12.05 && b <= 12.05);
            }
        }
    }

    #[test]
    fn a_second_line_staggers_its_gaps() {
        use crate::section::Section;
        let lay = roadblock_layout(
            &site_north(),
            &Section::plain(-9.5, 9.5),
            &recipe(3, 2, 2.7, 9),
            true,
            GapPick::Centre,
        );
        let second: Vec<&BlockGap> = lay.gaps.iter().filter(|g| g.line == 1).collect();
        assert!(!second.is_empty());
        assert!(lay.cars.iter().any(|c| c.line == 1 && c.pos[1] > 60.0));
        // No second-line gap lines up with a front-line gap.
        for g2 in &second {
            let c2 = 0.5 * (g2.a[0] + g2.b[0]);
            for g1 in lay.gaps.iter().filter(|g| g.line == 0) {
                let c1 = 0.5 * (g1.a[0] + g1.b[0]);
                assert!((c1 - c2).abs() > 1.5, "{c1} vs {c2}");
            }
        }
    }

    #[test]
    fn spike_strip_covers_the_racers_lanes_from_its_car_to_the_kerb() {
        use crate::section::Section;
        let sec = Section::plain(-5.0, 5.0);
        // Two-way: the car astride the centre line, the strip over the racer's half, the
        // oncoming lane open.
        let lay = spike_layout(&site_north(), &sec, true);
        assert!((lay.post[0] + TWO_WAY_CAR_LEFT_M).abs() < 1e-4);
        assert!(
            lay.a[0] > 0.0 && lay.a[0] < 0.8,
            "the strip reaches the centre line"
        );
        assert!(lay.a[0] > lay.post[0] && lay.a[0] < lay.post[0] + UNIT_HALF_W_M);
        assert!(lay.b[0] >= 5.0);
        assert!(
            lay.post[0] - UNIT_HALF_W_M - -5.0 >= 2.0,
            "oncoming lane open"
        );
        assert_eq!(lay.heading, [0.0, 1.0]);
        // One-way: the car at the left kerb, the strip over the rest.
        let lay = spike_layout(&site_north(), &sec, false);
        assert!(lay.post[0] - UNIT_HALF_W_M >= -5.0 && lay.post[0] < -3.5);
        assert!(lay.b[0] >= 5.0);
        assert!(!lay.flares.is_empty());
    }

    #[test]
    fn compass_words() {
        assert_eq!(compass([0.0, 1.0]), "northbound");
        assert_eq!(compass([-1.0, 0.2]), "westbound");
    }
}
