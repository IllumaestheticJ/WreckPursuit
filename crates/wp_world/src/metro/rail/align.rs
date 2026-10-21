//! Rail alignments: filleted centrelines and vertical profiles, computed only from the plan
//! and the natural ground (so [`super::earthworks`] and [`super::generate`] agree exactly).

use super::super::ground::{inside, signed_dist, Earthwork, Natural};
use super::super::plan::{self, Plan, RailKind, RailLine, SiteKind, StructureKind};
use crate::geom::{self, V2, V3};

/// Spacing of alignment samples, m.
pub const SAMPLE_M: f32 = 5.0;
/// Grade limit every alignment honours (rise over run).
pub const MAX_GRADE: f32 = 0.015;
/// Grade the profile is designed to (a margin under [`MAX_GRADE`]).
const DESIGN_GRADE: f32 = 0.0135;
/// Curve radius of main-line corners, m.
pub const MAIN_RADIUS_M: f32 = 300.0;
/// Curve radius of spur / yard / station-throat corners, m.
pub const SPUR_RADIUS_M: f32 = 180.0;
/// Distance between the centres of two parallel tracks, m.
pub const TRACK_SPACING_M: f32 = 4.5;
/// Half width of one ballast bed, m.
pub const BALLAST_HALF_M: f32 = 1.6;
/// Formation (top of the rail bed) of a bridge over water, m.
pub const WATER_DECK_Z: f32 = plan::WATER_Z + 5.75;
/// Formation above the highest ground under an overbridge (deck + 6 m clearance), m.
pub const OVERBRIDGE_RISE_M: f32 = 7.5;
/// Up to this height above the natural ground the bed is an earth embankment (terrain);
/// above it the line is carried on a viaduct, m.
pub const EMBANKMENT_MAX_M: f32 = 6.0;
/// The bed counts as "at grade" up to this height above the natural ground, m.
pub const AT_GRADE_MAX_M: f32 = 3.0;
/// Blend of the corridor earthwork beyond its flat part, m.
pub const CORRIDOR_BLEND_M: f32 = 20.0;
/// Bridges over water extend this far past the water line onto each bank, m.
const ABUTMENT_M: f32 = 40.0;
/// Corridor earthworks are split into pieces of at most this many samples (keeps
/// `Ground::apply` fast: it searches every segment of a corridor per terrain vertex).
const CORRIDOR_CHUNK: usize = 40;

/// How the bed is carried at one alignment sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Support {
    /// On the (graded) ground: cut, or fill up to [`AT_GRADE_MAX_M`].
    Grade,
    /// On an earth embankment [`AT_GRADE_MAX_M`]..[`EMBANKMENT_MAX_M`] high.
    Embankment,
    /// On a concrete viaduct (deck + piers).
    Viaduct,
    /// On a named bridge span ([`RailAlignment::spans`]).
    Bridge,
    /// On the elevated deck of a station (the station builds the deck).
    StationDeck,
}

impl Support {
    /// Graded ground carries the bed (a corridor earthwork is requested here).
    pub fn on_ground(self) -> bool {
        matches!(self, Support::Grade | Support::Embankment)
    }
}

/// Kind of a bridge span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    /// Swing bridge over water: camelback truss on a pivot pier.
    Swing,
    /// Plate-girder overbridge (over streets).
    Girder,
}

/// A bridge span along an alignment.
#[derive(Debug, Clone, PartialEq)]
pub struct BridgeSpan {
    /// Name (the plan structure's, or a generated one).
    pub name: String,
    /// Kind.
    pub kind: SpanKind,
    /// Start arc length, m.
    pub s0: f32,
    /// End arc length, m.
    pub s1: f32,
    /// Water arc-length interval under the span (`None` for overbridges), m.
    pub water: Option<(f32, f32)>,
}

/// One rail line: a filleted centreline sampled every [`SAMPLE_M`] with the formation
/// height (top of the rail bed; ballast and rails sit on it) as `z`.
#[derive(Debug, Clone)]
pub struct RailAlignment {
    /// Plan name.
    pub name: &'static str,
    /// Freight or passenger.
    pub kind: RailKind,
    /// Number of parallel tracks.
    pub tracks: u8,
    /// Smallest design radius of the line's corners, m.
    pub radius_m: f32,
    /// Centreline samples (`z` = formation), m.
    pub pts: Vec<V3>,
    /// Arc length of every sample, m.
    pub cum: Vec<f32>,
    /// Natural ground under every sample, m.
    pub natural: Vec<f32>,
    /// Support of every sample.
    pub support: Vec<Support>,
    /// Bridge spans.
    pub spans: Vec<BridgeSpan>,
    /// Ground level requested under an elevated station at the end of the line (an
    /// [`Earthwork::Pad`] over the station site), m.
    pub station_ground_z: Option<f32>,
}

impl RailAlignment {
    /// Length, m.
    pub fn length(&self) -> f32 {
        *self.cum.last().unwrap_or(&0.0)
    }

    /// Point (with formation height) and unit tangent at arc length `s`.
    pub fn at(&self, s: f32) -> (V3, V2) {
        geom::point_at(&self.pts, &self.cum, s)
    }

    /// Unit normal (right of travel) at sample `i` (averaged over the two adjacent segments).
    pub fn normal(&self, i: usize) -> V2 {
        let n = self.pts.len();
        let a = self.pts[i.saturating_sub(1)];
        let b = self.pts[(i + 1).min(n - 1)];
        geom::right_of(geom::normalize(geom::sub(geom::xy(b), geom::xy(a))))
    }

    /// Lateral offsets of the track centres from the centreline (right positive), m.
    pub fn track_offsets(&self) -> Vec<f32> {
        let n = self.tracks.max(1) as usize;
        (0..n)
            .map(|k| (k as f32 - (n - 1) as f32 * 0.5) * TRACK_SPACING_M)
            .collect()
    }

    /// Half width of the bed (outer ballast edge), m.
    pub fn half_span_m(&self) -> f32 {
        (self.tracks.max(1) as f32 - 1.0) * TRACK_SPACING_M * 0.5 + BALLAST_HALF_M
    }

    /// Flat half width of the corridor earthwork (8 m single, 10 m double track), m.
    pub fn corridor_half_width_m(&self) -> f32 {
        if self.tracks >= 2 {
            5.0
        } else {
            4.0
        }
    }

    /// Index of the sample nearest arc length `s`.
    pub fn index_at(&self, s: f32) -> usize {
        ((s / SAMPLE_M).round().max(0.0) as usize).min(self.pts.len() - 1)
    }

    /// Projection of `p` onto the centreline.
    pub fn project(&self, p: V2) -> geom::Projection {
        geom::project(&self.pts, &self.cum, p)
    }

    /// Largest grade between consecutive samples.
    pub fn max_grade(&self) -> f32 {
        self.pts
            .windows(2)
            .map(|w| {
                let d = geom::dist(geom::xy(w[0]), geom::xy(w[1])).max(1e-3);
                (w[1][2] - w[0][2]).abs() / d
            })
            .fold(0.0, f32::max)
    }

    /// Smallest curve radius, measured over chords of ~30 m (the fillet arcs), m.
    pub fn min_radius_m(&self) -> f32 {
        let k = 3;
        let mut best = f32::MAX;
        for i in k..self.pts.len().saturating_sub(k) {
            let (a, b, c) = (
                geom::xy(self.pts[i - k]),
                geom::xy(self.pts[i]),
                geom::xy(self.pts[i + k]),
            );
            let area2 = geom::cross(geom::sub(b, a), geom::sub(c, a)).abs();
            if area2 < 1e-3 {
                continue;
            }
            let r = geom::dist(a, b) * geom::dist(b, c) * geom::dist(a, c) / (2.0 * area2);
            best = best.min(r);
        }
        best
    }
}

/// The rail lines of the plan, filleted and profiled. Deterministic: depends only on the
/// plan and the natural ground (earthworks run before roads exist). Other modules may call
/// it to query where the tracks are (`pts`, `support`, `spans`).
///
/// Control points that differ from the plan (reported to the lead):
/// - Westline Freight Line: the corner south of the swing bridge moves from (-1700, -700) to
///   (-1700, -850) and the two corners north of it, (-1700, -100) and (-1900, 0), become one
///   corner at (-1700, 0): with 300 m radii the original corners put curves on the bridge.
///   The west end is clipped to the map.
/// - Terminal Spur: starts 200 m west of the plan start on the main line's south track (a
///   turnout instead of a 45° kink).
/// - Vesper Central Line: starts at the yard's east edge (-2050, 42) and enters the station
///   east-west along y = 300 (corner at (-960, 300), 180 m station-throat radius).
pub fn rail_lines(plan: &Plan, nat: &Natural) -> Vec<RailAlignment> {
    plan.rail
        .iter()
        .map(|line| {
            let (ctrl, radii) = control_points(line);
            let path = resample(&fillet(&ctrl, &radii, 2.0), SAMPLE_M);
            let radius = radii[1..radii.len().saturating_sub(1)]
                .iter()
                .copied()
                .fold(MAIN_RADIUS_M, f32::min);
            profile_line(plan, nat, line, path, radius)
        })
        .collect()
}

/// Corridor earthworks of every line, plus the pad under an elevated station (see
/// [`super::earthworks`]).
pub fn corridor_earthworks(plan: &Plan, lines: &[RailAlignment]) -> Vec<Earthwork> {
    let mut out = Vec::new();
    for a in lines {
        let Some(z) = a.station_ground_z else {
            continue;
        };
        for s in plan.sites_of(SiteKind::Station) {
            if a.pts.iter().any(|p| inside(&s.polygon, geom::xy(*p))) {
                out.push(Earthwork::Pad {
                    polygon: s.polygon.clone(),
                    z,
                    blend_m: 40.0,
                });
            }
        }
    }
    for a in lines {
        let mut run: Vec<V3> = Vec::new();
        let flush = |run: &mut Vec<V3>, out: &mut Vec<Earthwork>| {
            if run.len() >= 2 {
                let mut k = 0;
                while k + 1 < run.len() {
                    let end = (k + CORRIDOR_CHUNK).min(run.len() - 1);
                    out.push(Earthwork::Corridor {
                        line: run[k..=end].to_vec(),
                        half_width_m: a.corridor_half_width_m(),
                        blend_m: CORRIDOR_BLEND_M,
                    });
                    k = end;
                }
            }
            run.clear();
        };
        for i in 0..a.pts.len() {
            if a.support[i].on_ground() {
                run.push(a.pts[i]);
            } else {
                flush(&mut run, &mut out);
            }
        }
        flush(&mut run, &mut out);
    }
    out
}

/// Control polyline and per-vertex fillet radius of a plan line (see [`rail_lines`]).
fn control_points(line: &RailLine) -> (Vec<V2>, Vec<f32>) {
    let mut ctrl = line.line.clone();
    let mut radii = vec![MAIN_RADIUS_M; ctrl.len()];
    match line.name {
        "Westline Freight Line" => {
            ctrl = vec![
                line.line[0],
                line.line[1],
                line.line[2],
                [-1700.0, -850.0],
                [-1700.0, 0.0],
                *line.line.last().unwrap_or(&[-4450.0, 0.0]),
            ];
            radii = vec![MAIN_RADIUS_M; ctrl.len()];
        }
        "Terminal Spur" => {
            let first = line.line[0];
            let south = [first[0], first[1] - TRACK_SPACING_M * 0.5];
            ctrl[0] = south;
            ctrl.insert(0, [first[0] - 200.0, south[1]]);
            radii = vec![SPUR_RADIUS_M + 20.0; ctrl.len()];
        }
        "Vesper Central Line" => {
            ctrl = vec![
                [-2050.0, 42.0],
                line.line[1],
                [-960.0, 300.0],
                [-630.0, 300.0],
            ];
            radii = vec![MAIN_RADIUS_M, MAIN_RADIUS_M, SPUR_RADIUS_M, MAIN_RADIUS_M];
        }
        _ => {}
    }
    // Clip to the map (with a 2 m margin) — lines run "to the map edge".
    let lo = [plan::MAP_LO[0] + 2.0, plan::MAP_LO[1] + 2.0];
    let hi = [plan::MAP_HI[0] - 2.0, plan::MAP_HI[1] - 2.0];
    for p in ctrl.iter_mut() {
        p[0] = p[0].clamp(lo[0], hi[0]);
        p[1] = p[1].clamp(lo[1], hi[1]);
    }
    (ctrl, radii)
}

/// Round the corners of an open polyline with circular arcs of radius `radius[i]` at
/// vertex `i` (shortened only where two arcs would overlap), sampled every `step` metres.
pub fn fillet(ctrl: &[V2], radius: &[f32], step: f32) -> Vec<V2> {
    let n = ctrl.len();
    let mut out: Vec<V2> = vec![ctrl[0]];
    for i in 1..n.saturating_sub(1) {
        let (prev, c, next) = (ctrl[i - 1], ctrl[i], ctrl[i + 1]);
        let (l1, l2) = (geom::dist(prev, c), geom::dist(c, next));
        let d1 = geom::normalize(geom::sub(c, prev));
        let d2 = geom::normalize(geom::sub(next, c));
        let theta = wp_math::atan2(geom::cross(d1, d2), geom::dot(d1, d2));
        let half = theta.abs() * 0.5;
        if half < 1e-3 {
            out.push(c);
            continue;
        }
        let (s, co) = wp_math::sin_cos(half);
        let tan_half = s / co;
        let t = (radius[i] * tan_half).min(0.49 * l1.min(l2));
        let r = t / tan_half;
        let a = geom::sub(c, geom::scale(d1, t));
        let left = [-d1[1], d1[0]];
        let sign = if theta > 0.0 { 1.0 } else { -1.0 };
        let centre = geom::add(a, geom::scale(left, r * sign));
        let arm = geom::sub(a, centre);
        let m = ((r * theta.abs() / step).ceil() as usize).max(2);
        for k in 0..=m {
            let (sn, cs) = wp_math::sin_cos(theta * k as f32 / m as f32);
            out.push(geom::add(
                centre,
                [arm[0] * cs - arm[1] * sn, arm[0] * sn + arm[1] * cs],
            ));
        }
    }
    out.push(ctrl[n - 1]);
    out
}

/// Resample a polyline at (nearly) equal spacing `step`.
pub fn resample(p: &[V2], step: f32) -> Vec<V2> {
    let mut cum = vec![0.0];
    for w in p.windows(2) {
        let l = cum[cum.len() - 1] + geom::dist(w[0], w[1]);
        cum.push(l);
    }
    let total = cum[cum.len() - 1];
    let n = ((total / step).round() as usize).max(1);
    let mut out = Vec::with_capacity(n + 1);
    let mut seg = 0;
    for k in 0..=n {
        let s = total * k as f32 / n as f32;
        while seg + 2 < cum.len() && cum[seg + 1] < s {
            seg += 1;
        }
        let l = (cum[seg + 1] - cum[seg]).max(1e-6);
        let t = ((s - cum[seg]) / l).clamp(0.0, 1.0);
        out.push(geom::add(
            p[seg],
            geom::scale(geom::sub(p[seg + 1], p[seg]), t),
        ));
    }
    out
}

/// Profile one line: fixed levels on bridges, station decks and site pads, a smoothed
/// natural ground elsewhere, then a grade limit.
fn profile_line(
    plan: &Plan,
    nat: &Natural,
    line: &RailLine,
    path: Vec<V2>,
    radius_m: f32,
) -> RailAlignment {
    let n = path.len();
    let mut cum = vec![0.0f32; n];
    for i in 1..n {
        cum[i] = cum[i - 1] + geom::dist(path[i - 1], path[i]);
    }
    let natural: Vec<f32> = path.iter().map(|p| nat.z(*p)).collect();

    // Water spans → bridges.
    let wet: Vec<bool> = natural.iter().map(|z| *z < plan::WATER_Z + 0.5).collect();
    let mut spans: Vec<BridgeSpan> = Vec::new();
    let mut i = 0;
    while i < n {
        if !wet[i] {
            i += 1;
            continue;
        }
        let j0 = i;
        while i < n && wet[i] {
            i += 1;
        }
        let (w0, w1) = (cum[j0], cum[i - 1]);
        let mid = geom::add(path[j0], geom::scale(geom::sub(path[i - 1], path[j0]), 0.5));
        let name = plan
            .structures
            .iter()
            .filter(|s| s.kind == StructureKind::RailBridge && s.carries == line.name)
            .min_by(|a, b| {
                let da = super::super::ground::seg_dist(a.from, a.to, mid);
                let db = super::super::ground::seg_dist(b.from, b.to, mid);
                da.total_cmp(&db)
            })
            .map(|s| s.name.to_string())
            .unwrap_or_else(|| format!("{} river bridge", line.name));
        spans.push(BridgeSpan {
            name,
            kind: SpanKind::Swing,
            s0: (w0 - ABUTMENT_M).max(0.0),
            s1: (w1 + ABUTMENT_M).min(cum[n - 1]),
            water: Some((w0, w1)),
        });
    }
    // Plan rail bridges of this line that are not over water → overbridges.
    let path3: Vec<V3> = path.iter().map(|p| [p[0], p[1], 0.0]).collect();
    for st in plan
        .structures
        .iter()
        .filter(|s| s.kind == StructureKind::RailBridge && s.carries == line.name)
    {
        if spans.iter().any(|sp| sp.name == st.name) {
            continue;
        }
        let mid = geom::scale(geom::add(st.from, st.to), 0.5);
        let half = (geom::dist(st.from, st.to) * 0.5).max(60.0);
        let s = geom::project(&path3, &cum, mid).s.clamp(0.0, cum[n - 1]);
        spans.push(BridgeSpan {
            name: st.name.to_string(),
            kind: SpanKind::Girder,
            s0: (s - half).max(0.0),
            s1: (s + half).min(cum[n - 1]),
            water: None,
        });
    }
    spans.sort_by(|a, b| a.s0.total_cmp(&b.s0));

    // Station deck: the part of the line inside a station site, elevated with the
    // overbridge in front of it when the line has one.
    let station: Vec<bool> = path
        .iter()
        .map(|p| {
            plan.sites_of(SiteKind::Station)
                .any(|s| inside(&s.polygon, *p))
        })
        .collect();
    let has_girder = spans.iter().any(|s| s.kind == SpanKind::Girder);
    let in_span = |k: usize| {
        spans
            .iter()
            .position(|sp| cum[k] >= sp.s0 && cum[k] <= sp.s1)
    };

    // The overbridge clears the highest ground under it; an elevated station behind it
    // gets its ground levelled to the same height (a pad, see `station_ground_z`), so the
    // whole deck stands ~7.5 m above the streets.
    let mut girder_ground = f32::MIN;
    for (k, z) in natural.iter().enumerate() {
        if in_span(k).is_some_and(|q| spans[q].kind == SpanKind::Girder) {
            girder_ground = girder_ground.max(*z);
        }
    }
    let station_ground_z = (has_girder && station.iter().any(|s| *s)).then_some(girder_ground);
    let overbridge_z = girder_ground + OVERBRIDGE_RISE_M;

    // Fixed levels.
    let mut fixed: Vec<Option<f32>> = vec![None; n];
    for k in 0..n {
        if let Some(q) = in_span(k) {
            fixed[k] = Some(match spans[q].kind {
                SpanKind::Swing => WATER_DECK_Z,
                SpanKind::Girder => overbridge_z,
            });
        } else if station[k] && has_girder {
            fixed[k] = Some(overbridge_z);
        } else if let Some(z) = plan
            .sites
            .iter()
            .filter(|s| s.pad_z.is_some() && signed_dist(&s.polygon, path[k]) <= 3.0)
            .find_map(|s| s.pad_z)
        {
            fixed[k] = Some(z);
        }
    }

    // Desired: natural ground averaged over ±100 m, kept off the water line.
    let floor = plan::MIN_LAND_Z + 0.6;
    let desired: Vec<f32> = box_smooth(&natural, 20)
        .into_iter()
        .map(|z| z.max(floor))
        .collect();
    let ds = SAMPLE_M;
    let g = DESIGN_GRADE * ds;
    // Envelopes every grade-limited profile through the fixed levels must stay in.
    let mut lo = vec![f32::MIN; n];
    let mut hi = vec![f32::MAX; n];
    for k in 0..n {
        if let Some(z) = fixed[k] {
            lo[k] = z;
            hi[k] = z;
        }
    }
    for k in 1..n {
        lo[k] = lo[k].max(lo[k - 1] - g);
        hi[k] = hi[k].min(hi[k - 1] + g);
    }
    for k in (0..n.saturating_sub(1)).rev() {
        lo[k] = lo[k].max(lo[k + 1] - g);
        hi[k] = hi[k].min(hi[k + 1] + g);
    }
    let clamp = |z: f32, k: usize| {
        if lo[k] <= hi[k] {
            z.clamp(lo[k], hi[k])
        } else {
            (lo[k] + hi[k]) * 0.5
        }
    };
    let mut z: Vec<f32> = (0..n).map(|k| clamp(desired[k], k)).collect();
    for k in 1..n {
        z[k] = z[k].clamp(z[k - 1] - g, z[k - 1] + g);
    }
    for _ in 0..2 {
        z = box_smooth(&z, 8);
    }
    let z: Vec<f32> = (0..n).map(|k| clamp(z[k], k)).collect();

    let support: Vec<Support> = (0..n)
        .map(|k| {
            if in_span(k).is_some() {
                Support::Bridge
            } else if station[k] && has_girder {
                Support::StationDeck
            } else if wet[k] || z[k] - natural[k] > EMBANKMENT_MAX_M {
                Support::Viaduct
            } else if z[k] - natural[k] > AT_GRADE_MAX_M {
                Support::Embankment
            } else {
                Support::Grade
            }
        })
        .collect();
    // An embankment right next to a viaduct, an overbridge or a station deck would push a
    // terrain hump under the street beside them: carry the line on the viaduct instead.
    let elevated = |s: Support, k: usize| {
        matches!(s, Support::Viaduct | Support::StationDeck)
            || (s == Support::Bridge
                && in_span(k).is_some_and(|q| spans[q].kind == SpanKind::Girder))
    };
    let reach = (40.0 / ds) as usize;
    let support: Vec<Support> = (0..n)
        .map(|k| {
            let near =
                (k.saturating_sub(reach)..(k + reach + 1).min(n)).any(|j| elevated(support[j], j));
            if support[k] == Support::Embankment && near {
                Support::Viaduct
            } else {
                support[k]
            }
        })
        .collect();

    RailAlignment {
        name: line.name,
        kind: line.kind,
        tracks: line.tracks,
        radius_m,
        pts: (0..n).map(|k| [path[k][0], path[k][1], z[k]]).collect(),
        cum,
        natural,
        support,
        spans,
        station_ground_z,
    }
}

/// Box filter of half width `r` samples (ends padded with the end values, so a
/// grade-limited input stays grade-limited).
fn box_smooth(v: &[f32], r: usize) -> Vec<f32> {
    let n = v.len() as i64;
    let r = r as i64;
    let at = |k: i64| v[k.clamp(0, n - 1) as usize];
    let w = (2 * r + 1) as f32;
    (0..n)
        .map(|k| (k - r..=k + r).map(at).sum::<f32>() / w)
        .collect()
}
