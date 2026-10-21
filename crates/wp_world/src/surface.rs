//! Drivable surface geometry shared by physics (`wp_sim` colliders), the greybox exporter and
//! `wp_world`'s own physical traffic:
//! - [`road_mesh`]: one triangle mesh of every paved surface — road ribbons (banked) and the
//!   [`node_patches`] between their ends (junction surfaces, bend strips, dead-end
//!   turnarounds), sharing the ribbons' end edges so nothing is left unpaved;
//! - [`sidewalk_mesh`]: raised sidewalks (3 m wide, 0.15 m kerb) along every edge lane that
//!   is not a highway, with the vertical kerb faces, the level lots behind them (flush with
//!   the sidewalk out to the building line or a verge, [`lot_width_m`]) and a short skirt down
//!   to the ground beyond — the greybox draws exactly these triangles and `wp_sim` collides
//!   with exactly these triangles;
//! - [`deck_rails`]: guard-rail lines along bridge decks;
//! - [`MapGround`]: a wheel ground probe over roads and terrain without a physics engine.

use wp_vehicle::{GroundHit, WheelCaster};

use crate::geom::{self, V2, V3};
use crate::map::{CityMap, SegKind};
use std::collections::BTreeMap;

use crate::network::{Lane, LaneKind, RoadNetwork, RoadSurface};
use crate::terrain::Heightfield;

mod nodes;
pub use nodes::{node_patches, NodePatch, NodePatchKind};

/// Sidewalk width beyond the kerb line (the paved edge of an edge lane), m.
pub const SIDEWALK_W_M: f32 = 3.0;

/// Grass verge on residential streets: the inner part of the sidewalk, from the kerb out, m
/// (playtest row 95). Same surface as the sidewalk (physics), drawn as grass with street
/// trees on it; the walkway is the remaining `SIDEWALK_W_M - VERGE_W_M`.
pub const VERGE_W_M: f32 = 1.5;

/// Highest speed limit of a street that gets a grass verge, m/s (30 mph + a margin).
pub const VERGE_MAX_SPEED_MPS: f32 = 13.5;

/// Whether a street lane of this kind and speed limit in district `district` gets a grass
/// verge on its sidewalk: residential and suburban streets up to 30 mph (playtest row 95).
pub fn verge_street(
    district: Option<crate::network::DistrictKind>,
    kind: LaneKind,
    speed_mps: f32,
) -> bool {
    use crate::network::DistrictKind;
    matches!(
        district,
        Some(DistrictKind::Residential) | Some(DistrictKind::Suburb)
    ) && matches!(
        kind,
        LaneKind::Through | LaneKind::TurnLeft | LaneKind::TurnRight | LaneKind::Parking
    ) && speed_mps <= VERGE_MAX_SPEED_MPS
}

/// Whether `lane`'s sidewalks carry a grass verge ([`verge_street`] at the lane's middle).
pub fn lane_has_verge(map: &CityMap, lane: &Lane) -> bool {
    let n = lane.polyline.len();
    if n == 0 {
        return false;
    }
    let mid = geom::xy(lane.polyline[n / 2]);
    verge_street(map.district_at(mid), lane.kind, lane.speed_limit_mps)
}

/// Kerb height: the sidewalk top sits this far above the road edge, m.
pub const KERB_H_M: f32 = 0.15;

/// Level lot behind every sidewalk in a city with terrain, m: flush with the sidewalk's back
/// edge, out past the building line (Port Vesper's facades stand 1.5 m behind the sidewalk)
/// and wide enough for a ≥ 3 m verge where no building stands (playtest #52).
pub const LOT_W_M: f32 = 3.5;

/// [`LOT_W_M`] in a flat city, m: the grid's facades stand 4–8 m behind the sidewalk.
pub const FLAT_LOT_W_M: f32 = 9.0;

/// Skirt beyond a lot's outer edge: this wide (m), sloping down by [`SKIRT_DROP_M`] into the
/// ground, so leaving the lot is a gentle slope, never a step.
pub const SKIRT_W_M: f32 = 1.0;

/// How far a skirt drops over [`SKIRT_W_M`], m (ends below the ground it meets; further where
/// the ground is lower, e.g. next to a deck approach: the skirt then reaches down to it).
pub const SKIRT_DROP_M: f32 = 0.25;

/// A lot is left out (only the sidewalk is built, with a guard rail along its back edge:
/// [`SidewalkMesh::bare_backs`]) where the ground under its outer half is more than this far
/// below the lot top — bridge decks and their approaches, m.
pub const LOT_MAX_DROP_M: f32 = 1.0;

/// Nor where the ground rises more than this above the lot top (an uncarved slope next to a
/// deck), m.
pub const LOT_MAX_RISE_M: f32 = 0.08;

/// Width of the level lot behind every sidewalk of `net`, m ([`LOT_W_M`], or [`FLAT_LOT_W_M`]
/// without terrain).
pub fn lot_width_m(net: &RoadNetwork) -> f32 {
    if net.terrain.is_some() {
        LOT_W_M
    } else {
        FLAT_LOT_W_M
    }
}

/// Indexed triangle mesh, world space (+Z up), counter-clockwise seen from above.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TriMesh {
    /// Vertex positions, m.
    pub positions: Vec<V3>,
    /// Triangles.
    pub indices: Vec<[u32; 3]>,
}

impl TriMesh {
    /// Append quad `a b c d` (counter-clockwise seen from the side it faces).
    pub fn quad(&mut self, a: V3, b: V3, c: V3, d: V3) {
        let i = self.positions.len() as u32;
        self.positions.extend_from_slice(&[a, b, c, d]);
        self.indices.push([i, i + 1, i + 2]);
        self.indices.push([i, i + 2, i + 3]);
    }

    /// Append a ribbon: `left[k]`–`right[k]` cross sections along a strip.
    pub fn strip(&mut self, left: &[V3], right: &[V3]) {
        for k in 0..left.len().saturating_sub(1) {
            // Travel along the strip, right edge first, so triangles face up.
            self.quad(right[k], right[k + 1], left[k + 1], left[k]);
        }
    }

    /// Append a vertical strip hanging from `top[k]` down to `bottom[k]` (same xy), facing the
    /// right of the `top` polyline's direction.
    pub fn strip_vertical(&mut self, top: &[V3], bottom: &[V3]) {
        for k in 0..top.len().saturating_sub(1) {
            self.quad(top[k], bottom[k], bottom[k + 1], top[k + 1]);
        }
    }

    /// Append another mesh.
    pub fn append(&mut self, other: &TriMesh) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.indices.extend(
            other
                .indices
                .iter()
                .map(|t| [t[0] + base, t[1] + base, t[2] + base]),
        );
    }

    /// Append a flat convex polygon (fan) at its own vertex heights.
    pub fn fan(&mut self, poly: &[V3]) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(poly);
        for k in 1..poly.len().saturating_sub(1) as u32 {
            self.indices.push([base, base + k, base + k + 1]);
        }
    }
}

/// Left and right paved edges of a surface (mitred like the lanes; banked heights).
pub fn surface_edges(s: &RoadSurface) -> (Vec<V3>, Vec<V3>) {
    let mut left = geom::offset_polyline(&s.centre, -s.left_m);
    let mut right = geom::offset_polyline(&s.centre, s.right_m);
    for k in 0..s.centre.len() {
        left[k][2] = s.z_at(k, -s.left_m);
        right[k][2] = s.z_at(k, s.right_m);
    }
    (left, right)
}

/// Sides of a junction disc polygon.
pub const JUNCTION_SIDES: usize = 16;

/// The paved disc of a junction of radius `r` at `c`: a [`JUNCTION_SIDES`]-gon that encloses
/// the circle (its edges touch it), so the trimmed road ribbons, whose corners lie on the
/// circle, never leave an unpaved sliver at the junction mouth. Counter-clockwise.
pub fn junction_polygon(c: V2, r: f32) -> Vec<V2> {
    let n = JUNCTION_SIDES as f32;
    let (_, cos_half) = wp_math::sin_cos(std::f32::consts::PI / n);
    let rv = r / cos_half;
    (0..JUNCTION_SIDES)
        .map(|k| {
            let (sn, cs) = wp_math::sin_cos(k as f32 / n * std::f32::consts::TAU);
            [c[0] + cs * rv, c[1] + sn * rv]
        })
        .collect()
}

/// Paved radius of every junction (index = intersection id): at least its `radius_m`, and
/// far enough to reach the corners of every road ribbon trimmed at it (a ribbon of half
/// width `w` ending `d` from the centre has its corners at `sqrt(d² + w²)`), so no unpaved
/// crescent is left at the junction mouth. `radius_m` itself (lane trimming, junction logic)
/// is unchanged.
pub fn junction_paved_radii(net: &RoadNetwork) -> Vec<f32> {
    let mut radii: Vec<f32> = net.intersections.iter().map(|ix| ix.radius_m).collect();
    // Nodes at either end of each road (from its lanes).
    let mut road_nodes: BTreeMap<u32, [u32; 2]> = BTreeMap::new();
    for l in &net.lanes {
        road_nodes.entry(l.road).or_insert([l.from_node, l.to_node]);
    }
    for sfc in &net.surfaces {
        let Some(nodes) = road_nodes.get(&sfc.road) else {
            continue;
        };
        let (left, right) = surface_edges(sfc);
        let n = sfc.centre.len();
        if n < 2 {
            continue;
        }
        for node in nodes {
            let nd = &net.nodes[*node as usize];
            let Some(ix) = nd.intersection else {
                continue;
            };
            let c = geom::xy(nd.pos);
            // The surface end at this node.
            let k = if geom::dist(geom::xy(sfc.centre[0]), c)
                <= geom::dist(geom::xy(sfc.centre[n - 1]), c)
            {
                0
            } else {
                n - 1
            };
            let reach = geom::dist(geom::xy(left[k]), c).max(geom::dist(geom::xy(right[k]), c));
            let r = &mut radii[ix as usize];
            // Only ribbons trimmed at this junction (not a far end that happens to share it).
            if geom::dist(geom::xy(sfc.centre[k]), c) <= *r + 1.0 {
                *r = r.max(reach + 0.05);
            }
        }
    }
    radii
}

/// Every paved surface of a city as one mesh: road ribbons and the [`node_patches`] between
/// their ends (junction surfaces, bend strips, dead-end turnarounds).
pub fn road_mesh(map: &CityMap) -> TriMesh {
    road_mesh_with(map, &node_patches(map))
}

/// The paved ribbon of one road surface (banked), facing up. A ribbon's inner edge folds back
/// over itself on a bend tighter than its half width: the folded (down-facing) triangles are
/// left out; the neighbouring quads cover that spot.
pub fn surface_ribbon(s: &RoadSurface) -> TriMesh {
    let mut m = TriMesh::default();
    let (l, r) = surface_edges(s);
    m.strip(&l, &r);
    m.indices.retain(|t| {
        let [a, b, c] = t.map(|i| geom::xy(m.positions[i as usize]));
        geom::cross(geom::sub(b, a), geom::sub(c, a)) > 0.0
    });
    m
}

/// [`road_mesh`] with the node patches already built.
fn road_mesh_with(map: &CityMap, patches: &[NodePatch]) -> TriMesh {
    let mut m = TriMesh::default();
    for s in &map.net.surfaces {
        m.append(&surface_ribbon(s));
    }
    for p in patches {
        m.append(&p.mesh);
    }
    m
}

/// Cross slope of `lane` at each polyline point (dz per metre to the right of the lane's
/// direction), from its road surface; empty when the lane does not follow the surface point
/// for point (OSM imports, flat cities: no bank).
pub fn lane_bank(net: &RoadNetwork, lane: &Lane) -> Vec<f32> {
    let Some(s) = net.surfaces.get(lane.road as usize) else {
        return Vec::new();
    };
    let n = s.centre.len();
    if s.road != lane.road || n != lane.polyline.len() {
        return Vec::new();
    }
    let p0 = geom::xy(lane.polyline[0]);
    let fwd = geom::dist(p0, geom::xy(s.centre[0])) <= geom::dist(p0, geom::xy(s.centre[n - 1]));
    if fwd {
        s.bank.clone()
    } else {
        s.bank.iter().rev().map(|b| -b).collect()
    }
}

/// Whether `lane` has a raised sidewalk on its left / right edge.
pub fn lane_sidewalks(lane: &Lane) -> [bool; 2] {
    let on = lane.kind != LaneKind::Highway;
    [on && lane.edge_left, on && lane.edge_right]
}

/// Raised sidewalks: the walkable tops, the level lots behind them, the skirts beyond the
/// lots and the kerb faces between the sidewalks and the road.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SidewalkMesh {
    /// Sidewalk tops, facing up: from the kerb line out to [`SIDEWALK_W_M`], [`KERB_H_M`]
    /// above the road edge and continuing the road's bank (on verge streets only the walkway
    /// beyond the verge).
    pub tops: TriMesh,
    /// Grass verges ([`lane_has_verge`]): the first [`VERGE_W_M`] of the sidewalk top from the
    /// kerb line, sharing its vertices with [`SidewalkMesh::tops`] (same surface, drawn as
    /// grass).
    pub verges: TriMesh,
    /// Corner fills between the strip ends at junctions, bends and dead ends, at kerb height
    /// (they reach out to the lots' outer lines).
    pub corners: TriMesh,
    /// Lots, facing up: from each strip's back edge (sharing its vertices) out by
    /// [`lot_width_m`], level with the back edge. None where the ground falls away
    /// ([`LOT_MAX_DROP_M`]: bridge decks).
    pub lots: TriMesh,
    /// Skirts beyond the lots' outer edges, [`SKIRT_W_M`] wide, dropping [`SKIRT_DROP_M`] into
    /// the ground; left out over pavement or another walkable top.
    pub skirts: TriMesh,
    /// Vertical kerb faces from the road edge up to the sidewalk top.
    pub kerbs: TriMesh,
    /// Outer (bottom) edges of the skirt triangles built, `(a, b)` with the skirt on the left
    /// of a → b: where the greybox hangs its retaining walls down to the drawn ground.
    pub skirt_feet: Vec<(V3, V3)>,
    /// Sidewalk back edges without a lot (bridge decks), `(a, b)` with the sidewalk on the
    /// left of a → b: where the greybox hangs its walls.
    pub bare_backs: Vec<(V3, V3)>,
    /// Node centres of the corners filled in [`TRIM_CELL_M`] cells (a sidewalk end degenerate
    /// against the kerb curve, e.g. lying along it): the only places where the sidewalk edge
    /// follows the pavement in steps (≤ 0.35 m slivers) instead of exactly.
    pub cell_corners: Vec<V2>,
}

impl SidewalkMesh {
    /// The level walkable surface: sidewalk tops, corner fills and lots, one mesh (their shared
    /// edges have bit-identical vertices).
    pub fn flush(&self) -> TriMesh {
        let mut m = self.tops.clone();
        m.append(&self.verges);
        m.append(&self.corners);
        m.append(&self.lots);
        m
    }

    /// Everything `wp_sim` collides with: [`SidewalkMesh::flush`], skirts and kerb faces.
    pub fn all(&self) -> TriMesh {
        let mut m = self.flush();
        m.append(&self.skirts);
        m.append(&self.kerbs);
        m
    }
}

/// Every raised sidewalk of a city ([`lane_sidewalks`]): a top strip and a kerb strip along
/// each sidewalk edge of a lane, following the lane polyline and its bank, with the level lot
/// behind it ([`lot_width_m`]) and a skirt beyond the lot. Where a sidewalk or lot would run
/// onto pavement — the crossing road or junction disc at a tight or skewed corner — it is
/// trimmed in [`TRIM_CELL_M`] cells, so no kerb step ever sits inside a roadway. Used by
/// the greybox (drawn) and by `wp_sim` (collided), so the two always agree.
pub fn sidewalk_mesh(map: &CityMap) -> SidewalkMesh {
    let net = &map.net;
    let lot_w = lot_width_m(net);
    // Whether the ground under `p` holds a lot at height `p.z` (no terrain: always): not far
    // below it (a deck) nor poking up through it (an uncarved slope).
    let supported = |p: V3| {
        net.terrain.as_ref().is_none_or(|t| {
            let drop = p[2] - t.height_at(geom::xy(p));
            (-LOT_MAX_RISE_M..=LOT_MAX_DROP_M).contains(&drop)
        })
    };
    let geo = WalkGeometry {
        lot_w,
        max_pull_back: if net.terrain.is_some() {
            MAX_WEDGE_PULL_BACK_M
        } else {
            MAX_PULL_BACK_M + lot_w
        },
        wedges: net.terrain.is_some(),
    };
    let patches = node_patches(map);
    let paved = PavedIndex::new(&road_mesh_with(map, &patches));
    let rims: BTreeMap<u32, &[Vec<V3>]> = patches
        .iter()
        .map(|p| (p.node, p.rims.as_slice()))
        .collect();
    let mut out = SidewalkMesh::default();
    // Every strip with its road and end nodes.
    let mut walks: Vec<Strip> = Vec::new();
    for l in &net.lanes {
        let [left, right] = lane_sidewalks(l);
        if !(left || right) {
            continue;
        }
        let half = l.width_m * 0.5;
        let bank = lane_bank(net, l);
        let b = |k: usize| bank.get(k).copied().unwrap_or(0.0);
        // Polyline offset by `lat` (+ right), on the banked plane, lifted by `dz`.
        let line = |lat: f32, dz: f32| -> Vec<V3> {
            let mut e = geom::offset_polyline(&l.polyline, lat);
            for (k, p) in e.iter_mut().enumerate() {
                p[2] += b(k) * lat + dz;
            }
            e
        };
        let verge = lane_has_verge(map, l);
        for (on, side) in [(right, 1.0f32), (left, -1.0)] {
            if !on {
                continue;
            }
            let back = line(side * (half + SIDEWALK_W_M), KERB_H_M);
            // The lot's outer line, level with the back edge.
            let mut outer =
                geom::offset_polyline(&l.polyline, side * (half + SIDEWALK_W_M + lot_w));
            for (o, b) in outer.iter_mut().zip(&back) {
                o[2] = b[2];
            }
            // Every segment has a lot until the ends are pulled back (below).
            let lot = vec![true; back.len().saturating_sub(1)];
            let walk = SideWalk {
                kerb_top: line(side * half, KERB_H_M),
                back,
                outer,
                kerb_foot: line(side * half, 0.0),
                lot,
                // Tops face up: the strip's left edge must be on the left of its direction.
                outer_is_left: side < 0.0,
                verge,
            };
            let n = walk.kerb_top.len();
            if n < 2 {
                continue;
            }
            walks.push(Strip {
                kerb: walk.kerb_top.clone(),
                walk,
                road: l.road,
                nodes: [l.from_node, l.to_node],
                pulled: [0.0, 0.0],
            });
        }
    }
    // Where two strips of different roads end crossing each other (the outer corner square
    // when the crosswalk setback is shorter than the sidewalk is wide), pull both back until
    // they clear: overlapping strips of differently graded roads would step and flicker; the
    // corner fill paves the rest.
    pull_back_ends(&mut walks, &paved, &geo);
    // Segments with a lot: the ground holds the lot's outer half all along (not a deck or
    // its approach; nearer the road the ground may sit lower, under the sidewalk). Checked
    // after the pull-back, so a strip end no longer reaches over the crossing street's
    // sidewalk, where the ground is carved lower.
    for w in &mut walks {
        let sw = &mut w.walk;
        for k in 0..sw.lot.len() {
            let (b0, b1, o0, o1) = (sw.back[k], sw.back[k + 1], sw.outer[k], sw.outer[k + 1]);
            let len = geom::dist(geom::xy(b0), geom::xy(b1));
            let n = ((len / 4.0).ceil() as usize).max(1);
            sw.lot[k] = (0..=n).all(|i| {
                let t = i as f32 / n as f32;
                let lerp = |a: V3, b: V3| {
                    [
                        a[0] + (b[0] - a[0]) * t,
                        a[1] + (b[1] - a[1]) * t,
                        a[2] + (b[2] - a[2]) * t,
                    ]
                };
                let (b, o) = (lerp(b0, b1), lerp(o0, o1));
                let half_way = [(b[0] + o[0]) * 0.5, (b[1] + o[1]) * 0.5, b[2]];
                supported(o) && supported(half_way)
            });
        }
    }
    let mut ends: BTreeMap<u32, Vec<WalkEnd>> = BTreeMap::new();
    // Skirt candidates: (top edge, outward unit side: +1 right of the edge's direction).
    let mut skirt_edges: Vec<Vec<V3>> = Vec::new();
    for w in &walks {
        w.walk.emit(&paved, &mut out);
        skirt_edges.extend(w.walk.skirt_edges());
        out.bare_backs.extend(w.walk.bare_backs());
        for (i, at_start) in [(0, true), (1, false)] {
            if let Some(mut e) = w.walk.end(w.road, at_start) {
                e.tail = kerb_tail(&w.kerb, w.pulled[i], at_start);
                e.corner = *e.tail.last().unwrap_or(&e.inner);
                ends.entry(w.nodes[i]).or_default().push(e);
            }
        }
    }
    let strips = PavedIndex::new(&out.flush());
    for (node, es) in &ends {
        let node_rims = rims.get(node).copied().unwrap_or(&[]);
        let mut fill = CornerFill {
            paved: &paved,
            strips: &strips,
            geo: &geo,
            out: &mut out,
            skirt_edges: &mut skirt_edges,
        };
        fill_corners(map, *node, es, node_rims, &mut fill);
    }
    // A strip folds back over itself where its lane bends tighter than the strip's offset:
    // drop the folded (down-facing) top triangles; the neighbouring quads cover that spot.
    for m in [
        &mut out.tops,
        &mut out.verges,
        &mut out.corners,
        &mut out.lots,
    ] {
        drop_folded(m);
    }
    // Skirts: down from every lot / corner outer edge on supported ground, except over
    // pavement or another walkable top (a corner fill, a neighbouring strip).
    let flush = PavedIndex::new(&out.flush());
    for edge in &skirt_edges {
        if edge.len() < 2 || !edge.iter().all(|p| supported(*p)) {
            continue;
        }
        let ground = |q: V2| net.terrain.as_ref().map(|t| t.height_at(q));
        let foot: Vec<V3> = geom::offset_polyline(edge, SKIRT_W_M)
            .into_iter()
            .map(|p| {
                let z = p[2] - SKIRT_DROP_M;
                let z = ground(geom::xy(p)).map_or(z, |g| z.min(g - 0.03));
                [p[0], p[1], z]
            })
            .collect();
        // A triangle is kept when it faces up and its centroid is over open ground.
        let open = |t: [V3; 3]| {
            let [a, b, c] = t;
            let up = geom::cross(
                geom::sub(geom::xy(b), geom::xy(a)),
                geom::sub(geom::xy(c), geom::xy(a)),
            ) > 0.0;
            let q = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0];
            let z = (a[2] + b[2] + c[2]) / 3.0;
            up && !paved.covers(q, z) && !flush.covers(q, z)
        };
        for k in 0..edge.len() - 1 {
            // The quad's two triangles as `TriMesh::strip` builds them (edge on the left).
            let (e0, e1, f0, f1) = (edge[k], edge[k + 1], foot[k], foot[k + 1]);
            let outer_tri = [f0, f1, e1];
            if open(outer_tri) {
                out.skirts.fan(&outer_tri);
                out.skirt_feet.push((f0, f1));
            }
            let inner_tri = [f0, e1, e0];
            if open(inner_tri) {
                out.skirts.fan(&inner_tri);
            }
        }
    }
    out
}

/// Drop the folded (down-facing, seen from above) and degenerate triangles of `m`.
fn drop_folded(m: &mut TriMesh) {
    let pos = &m.positions;
    m.indices.retain(|t| {
        let [a, b, c] = t.map(|i| geom::xy(pos[i as usize]));
        geom::cross(geom::sub(b, a), geom::sub(c, a)) > 0.0
    });
}

/// Width-dependent limits of [`sidewalk_mesh`] for one city.
struct WalkGeometry {
    /// Lot width behind the sidewalk, m.
    lot_w: f32,
    /// Largest pull-back of a strip end, m.
    max_pull_back: f32,
    /// Whether strip ends are pulled back out of a neighbour's band (cities with terrain:
    /// overlapping lots there lie at different heights; in a flat city they coincide).
    wedges: bool,
}

/// What the corner fills of [`sidewalk_mesh`] check against and write to.
struct CornerFill<'a> {
    paved: &'a PavedIndex,
    strips: &'a PavedIndex,
    geo: &'a WalkGeometry,
    out: &'a mut SidewalkMesh,
    skirt_edges: &'a mut Vec<Vec<V3>>,
}

/// One sidewalk strip of [`sidewalk_mesh`] with where it belongs.
struct Strip {
    walk: SideWalk,
    road: u32,
    /// Start and finish node.
    nodes: [u32; 2],
    /// The whole kerb line (kerb-top height) before any pulling back.
    kerb: Vec<V3>,
    /// How far each end has been pulled back, m.
    pulled: [f32; 2],
}

/// The stretch of kerb line `kerb` a strip end gave up by being pulled back `pulled` metres at
/// its start (`at_start`) or finish: its points from the strip's new end (exclusive) out to
/// the original end (inclusive), in that order. Empty when not pulled back.
fn kerb_tail(kerb: &[V3], pulled: f32, at_start: bool) -> Vec<V3> {
    if pulled <= 0.0 || kerb.len() < 2 {
        return Vec::new();
    }
    // Walk from the original end towards the strip; keep points closer than `pulled`.
    let ordered: Vec<V3> = if at_start {
        kerb.to_vec()
    } else {
        kerb.iter().rev().copied().collect()
    };
    let mut acc = 0.0f32;
    let mut out: Vec<V3> = vec![ordered[0]];
    for w in ordered.windows(2) {
        acc += geom::dist(geom::xy(w[0]), geom::xy(w[1]));
        if acc >= pulled - 1e-3 {
            break;
        }
        out.push(w[1]);
    }
    out.reverse();
    out
}

/// Largest distance a strip end is pulled back to clear a crossing neighbour, m, plus the
/// city's lot width ([`WalkGeometry::max_pull_back`]).
const MAX_PULL_BACK_M: f32 = 6.0;

/// Largest pull-back in a city with terrain, where strip ends also clear the bands of streets
/// diverging at an acute angle, m.
const MAX_WEDGE_PULL_BACK_M: f32 = 30.0;

/// Clearance left between the end edges of two strips at a node, m: strips of differently
/// graded roads meet at different heights, so the corner fill needs room to slope between
/// them.
const PULL_BACK_CLEARANCE_M: f32 = 0.5;

/// Pull back, in 0.25 m steps, every strip end whose end edge (kerb-line point to outer point)
/// lies over pavement (another road running past a tight corner), and the ends of strips of
/// different roads at the same node whose end edges cross or come within
/// [`PULL_BACK_CLEARANCE_M`], until none do.
fn pull_back_ends(walks: &mut [Strip], paved: &PavedIndex, geo: &WalkGeometry) {
    let max_pull = geo.max_pull_back;
    // Over pavement (kerb-line point excluded: it is on the road's own edge).
    let on_paved = |w: &Strip, end: usize| -> bool {
        let Some(e) = w.walk.end(w.road, end == 0) else {
            return false;
        };
        (1..=6).any(|k| {
            let t = k as f32 / 6.0;
            let q = [
                e.inner[0] + (e.outer[0] - e.inner[0]) * t,
                e.inner[1] + (e.outer[1] - e.inner[1]) * t,
            ];
            let z = e.inner[2] + (e.outer[2] - e.inner[2]) * t - KERB_H_M;
            paved.covers_inside(q, z)
        })
    };
    for w in walks.iter_mut() {
        for end in 0..2 {
            while on_paved(w, end) && w.pulled[end] + 0.25 <= max_pull {
                match w.walk.pulled_back(0.25, end == 0) {
                    Some(nw) => {
                        w.walk = nw;
                        w.pulled[end] += 0.25;
                    }
                    None => break,
                }
            }
        }
    }
    // Strip ends per node: (strip index, end 0 = start / 1 = finish).
    let mut at: BTreeMap<u32, Vec<(usize, usize)>> = BTreeMap::new();
    for (i, w) in walks.iter().enumerate() {
        at.entry(w.nodes[0]).or_default().push((i, 0));
        at.entry(w.nodes[1]).or_default().push((i, 1));
    }
    let edge = |w: &Strip, end: usize| -> Option<(V2, V2)> {
        let e = w.walk.end(w.road, end == 0)?;
        Some((geom::xy(e.inner), geom::xy(e.outer)))
    };
    for list in at.values() {
        for _ in 0..(max_pull / 0.25) as usize {
            let mut moved = false;
            for (x, &(i, ei)) in list.iter().enumerate() {
                for &(j, ej) in &list[x + 1..] {
                    if walks[i].road == walks[j].road {
                        continue;
                    }
                    let (Some((a0, a1)), Some((b0, b1))) =
                        (edge(&walks[i], ei), edge(&walks[j], ej))
                    else {
                        continue;
                    };
                    // End edges crossing or too close: pull both back. An end edge across the
                    // other strip's band (streets diverging at an acute angle, their lots
                    // overlapping at different heights): pull that one back, so the corner
                    // fill blends the two levels across the whole wedge.
                    let close = geom::segment_distance(a0, a1, b0, b1) < PULL_BACK_CLEARANCE_M;
                    let into = |(p0, p1): (V2, V2), w: &SideWalk| {
                        geo.wedges
                            && [&w.kerb_top, &w.outer].iter().any(|line| {
                                line.windows(2).any(|q| {
                                    geom::segments_intersect(p0, p1, geom::xy(q[0]), geom::xy(q[1]))
                                })
                            })
                    };
                    let pull = [
                        close || into((a0, a1), &walks[j].walk),
                        close || into((b0, b1), &walks[i].walk),
                    ];
                    if !pull[0] && !pull[1] {
                        continue;
                    }
                    for (k, ek) in [(i, ei), (j, ej)]
                        .into_iter()
                        .zip(pull)
                        .filter(|(_, p)| *p)
                        .map(|(e, _)| e)
                    {
                        let w = &mut walks[k];
                        if w.pulled[ek] + 0.25 > max_pull {
                            continue;
                        }
                        if let Some(nw) = w.walk.pulled_back(0.25, ek == 0) {
                            w.walk = nw;
                            w.pulled[ek] += 0.25;
                            moved = true;
                        }
                    }
                }
            }
            if !moved {
                break;
            }
        }
    }
}

/// The end of one sidewalk strip at a node: kerb-line point, outer point and the direction
/// of the strip away from the node.
#[derive(Debug, Clone)]
struct WalkEnd {
    road: u32,
    inner: V3,
    outer: V3,
    /// The sidewalk's back edge on the end edge, where the end has a lot (a vertex of the
    /// strip's end edge between `inner` and `outer` that a fill must share).
    back: Option<V3>,
    /// Unit direction along the strip, pointing away from the node.
    dir: V2,
    /// Kerb-line point at the lane end (the pavement's corner); `inner` when the strip was
    /// not pulled back from it.
    corner: V3,
    /// The kerb line given up by pulling the end back, from just past `inner` to `corner`.
    tail: Vec<V3>,
}

/// Fill the gaps between the sidewalk ends at one node — junction corners, the top of a T,
/// the outside of a bend — with sidewalk at kerb height: the area bounded by the two
/// strips' kerb and outer lines (extended to where they meet), in [`TRIM_CELL_M`] cells,
/// leaving out pavement and what the strips already cover. Kerb faces line the cells that
/// border pavement.
fn fill_corners(map: &CityMap, node: u32, ends: &[WalkEnd], rims: &[Vec<V3>], f: &mut CornerFill) {
    let net = &map.net;
    let lot_w = f.geo.lot_w;
    let c = geom::xy(net.nodes[node as usize].pos);
    let angle = |p: V2| {
        let d = geom::sub(p, c);
        wp_math::atan2(d[1], d[0])
    };
    // Road axes at this node (mean of its sidewalk ends), so no patch spans a road.
    let mut axes: BTreeMap<u32, (V2, f32)> = BTreeMap::new();
    for e in ends {
        let a = axes.entry(e.road).or_insert(([0.0, 0.0], 0.0));
        a.0 = geom::add(a.0, geom::xy(e.inner));
        a.1 += 1.0;
    }
    let tau = std::f32::consts::TAU;
    let axis_angles: Vec<f32> = axes
        .values()
        .map(|(sum, n)| angle(geom::scale(*sum, 1.0 / n)))
        .collect();
    // Roads without sidewalks (highways) through this node count as axes too.
    let mut lane_axes: Vec<f32> = Vec::new();
    for l in &net.lanes {
        if l.from_node == node || l.to_node == node {
            let p = if l.from_node == node {
                l.polyline[0]
            } else {
                l.polyline[l.polyline.len() - 1]
            };
            lane_axes.push(angle(geom::xy(p)));
        }
    }
    let mut order: Vec<(f32, usize)> = ends
        .iter()
        .enumerate()
        .map(|(i, e)| {
            (
                angle(geom::scale(
                    geom::add(geom::xy(e.inner), geom::xy(e.outer)),
                    0.5,
                )),
                i,
            )
        })
        .collect();
    order.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let ccw = |from: f32, to: f32| (to - from).rem_euclid(tau);
    let mut done: std::collections::BTreeSet<(usize, usize)> = std::collections::BTreeSet::new();
    for k in 0..order.len() {
        let (a1, i1) = order[k];
        let (a2, i2) = order[(k + 1) % order.len()];
        if order.len() < 2 || i1 == i2 {
            continue;
        }
        let (e1, e2) = (&ends[i1], &ends[i2]);
        let gap = ccw(a1, a2);
        // The kerb curve of the pavement between these two sidewalk ends, if any: fill
        // exactly up to it. (At a sharp corner the two ends can sort the other way round.)
        if !done.insert((i1.min(i2), i1.max(i2))) {
            continue;
        }
        let rim = rim_between(rims, e1, e2)
            .map(|r| (r, e1, e2))
            .or_else(|| rim_between(rims, e2, e1).map(|r| (r, e2, e1)));
        if let Some((rim, e1, e2)) = rim {
            if exact_fill(c, e1, e2, rim, f) {
                continue;
            }
        }
        if e1.road == e2.road && gap < 1.0 {
            // Two strips of the same road side by side (e.g. lanes split at the node).
            continue;
        }
        let between = |x: f32| {
            let t = ccw(a1, x);
            t > 1e-3 && t < gap - 1e-3
        };
        if axis_angles.iter().chain(&lane_axes).any(|x| between(*x)) || gap > 4.4 {
            continue;
        }
        if (e1.inner[2] - e2.inner[2]).abs() > 1.0
            || geom::dist(geom::xy(e1.inner), geom::xy(e2.inner)) > 30.0 + 2.0 * lot_w
        {
            continue;
        }
        f.out.cell_corners.push(c);
        fill_patch(c, e1, e2, f);
    }
}

/// The node patch rim running from `e1`'s kerb-line end to `e2`'s (within 0.15 m).
fn rim_between<'a>(rims: &'a [Vec<V3>], e1: &WalkEnd, e2: &WalkEnd) -> Option<&'a [V3]> {
    let near = |a: V3, b: V3| geom::dist(geom::xy(a), geom::xy(b)) < 0.15;
    rims.iter()
        .find(|r| r.len() >= 2 && near(r[0], e1.corner) && near(r[r.len() - 1], e2.corner))
        .map(|r| r.as_slice())
}

/// Sidewalk between the ends `e1` and `e2` bounded exactly by the pavement's kerb curve `rim`
/// (from `e1.inner` to `e2.inner`, pavement on its left): the polygon from `e1`'s end edge out
/// to where the two outer lines meet (a dead end's turnaround: the rim offset by the sidewalk
/// width) and back along the rim, at kerb height, with a kerb face along the rim. False
/// (nothing emitted) when that polygon is not simple or would cover pavement or another
/// sidewalk; the caller then falls back to [`fill_patch`].
fn exact_fill(c: V2, e1: &WalkEnd, e2: &WalkEnd, rim: &[V3], f: &mut CornerFill) -> bool {
    let n = rim.len();
    let lift = |p: V3| [p[0], p[1], p[2] + KERB_H_M];
    let plane = fit_plane(&[e1.inner, e1.outer, e2.inner, e2.outer], c);
    let z_at = |p: V2| plane[0] + plane[1] * (p[0] - c[0]) + plane[2] * (p[1] - c[1]);
    let (o1, o2) = (geom::xy(e1.outer), geom::xy(e2.outer));
    let (i1, i2) = (geom::xy(e1.inner), geom::xy(e2.inner));
    // The kerb from e2's strip end back to e1's: e2's pulled-back stretch, the rim, e1's.
    let mut rim_back: Vec<V3> = e2.tail.clone();
    rim_back.extend(rim.iter().take(n - 1).skip(1).rev().map(|p| lift(*p)));
    rim_back.extend(e1.tail.iter().rev());
    // The whole kerb line from e1's strip end to e2's, at kerb-top height.
    let mut kerb_line: Vec<V3> = vec![e1.inner];
    kerb_line.extend(rim_back.iter().rev().copied());
    kerb_line.push(e2.inner);
    // Strips overlapping at the outer corner (end edges crossing): fill up to the crossing;
    // the strips cover the rest.
    if e1.road != e2.road && geom::segments_intersect(i1, o1, i2, o2) {
        let (d1, d2) = (
            geom::normalize(geom::sub(o1, i1)),
            geom::normalize(geom::sub(o2, i2)),
        );
        if let Some(x) = line_cross(i1, d1, i2, d2) {
            let mut poly: Vec<V3> = vec![e1.inner, [x[0], x[1], z_at(x)], e2.inner];
            poly.extend_from_slice(&rim_back);
            if emit_exact(&poly, &kerb_line, &[], f) {
                return true;
            }
        }
    }
    // Around the kerb: the rim offset outwards (to its right) by the strips' width (the
    // narrower end's: a deck end has no lot).
    let width = geom::dist(i1, o1).min(geom::dist(i2, o2));
    let ring = |f: &mut CornerFill| {
        let outer = geom::offset_polyline(&kerb_line, width);
        let m = outer.len();
        let mut chain: Vec<V3> = vec![e1.outer];
        chain.extend(outer.iter().take(m - 1).skip(1).copied());
        chain.push(e2.outer);
        let mut poly: Vec<V3> = vec![e1.inner];
        poly.extend(e1.back);
        poly.extend_from_slice(&chain);
        poly.extend(e2.back);
        poly.push(e2.inner);
        poly.extend_from_slice(&rim_back);
        emit_exact(&poly, &kerb_line, &chain, f)
    };
    if e1.road == e2.road {
        // A dead end's turnaround.
        return ring(f);
    }
    // Out to where the two outer lines meet, or straight across between the outer ends, or
    // around the kerb: the first that is simple and covers nothing else.
    // The polygon and its outer chain (e1's outer end, the apex if any, e2's outer end).
    let with_apex = |apex: Option<V2>| {
        let mut chain: Vec<V3> = vec![e1.outer];
        if let Some(a) = apex {
            // Past a strip's end the apex lies on that strip's outer edge: take its height
            // there (the strips end level across); else the corner plane.
            let z = if geom::dot(geom::sub(a, o1), e1.dir) > 0.0 {
                e1.outer[2]
            } else if geom::dot(geom::sub(a, o2), e2.dir) > 0.0 {
                e2.outer[2]
            } else {
                z_at(a)
            };
            chain.push([a[0], a[1], z]);
        }
        chain.push(e2.outer);
        let mut poly: Vec<V3> = vec![e1.inner];
        poly.extend(e1.back);
        poly.extend_from_slice(&chain);
        poly.extend(e2.back);
        poly.push(e2.inner);
        poly.extend_from_slice(&rim_back);
        (poly, chain)
    };
    // (Not when it is one of the outer ends already.)
    let reach = 40.0 + 1.5 * f.geo.lot_w;
    let apex = line_cross(o1, e1.dir, o2, e2.dir).filter(|a| {
        geom::dist(*a, c) < reach && geom::dist(*a, o1) > 0.05 && geom::dist(*a, o2) > 0.05
    });
    if apex.is_some() {
        let (poly, chain) = with_apex(apex);
        if emit_exact(&poly, &kerb_line, &chain, f) {
            return true;
        }
    }
    let (poly, chain) = with_apex(None);
    emit_exact(&poly, &kerb_line, &chain, f) || ring(f)
}

/// Emit the sidewalk polygon `poly` (kerb height) and the kerb face under `kerb_line` (its
/// kerb-top edge, pavement on its left; see [`exact_fill`]), and queue a skirt along `chain`
/// (the polygon's outer edge, outside on its right); false (nothing emitted) when `poly` is
/// not simple or covers pavement or a sidewalk strip.
fn emit_exact(poly: &[V3], kerb_line: &[V3], chain: &[V3], f: &mut CornerFill) -> bool {
    let (paved, strips) = (f.paved, f.strips);
    let mut tops = TriMesh::default();
    if !nodes::polygon_mesh(&mut tops, poly) {
        return false;
    }
    // Nothing under pavement or another sidewalk strip (triangle centroids; the shared edges
    // themselves are on the boundary).
    for t in &tops.indices {
        let [a, b, d] = t.map(|i| tops.positions[i as usize]);
        let q = [(a[0] + b[0] + d[0]) / 3.0, (a[1] + b[1] + d[1]) / 3.0];
        let z = (a[2] + b[2] + d[2]) / 3.0;
        if paved.covers_inside(q, z - KERB_H_M) || strips.covers_inside(q, z) {
            return false;
        }
    }
    f.out.corners.append(&tops);
    // Kerb face down to the pavement (on the line's left; a vertical strip faces the right of
    // its top line, so walk it backwards).
    let top: Vec<V3> = kerb_line.iter().rev().copied().collect();
    let foot: Vec<V3> = top.iter().map(|p| [p[0], p[1], p[2] - KERB_H_M]).collect();
    f.out.kerbs.strip_vertical(&top, &foot);
    if chain.len() >= 2 {
        f.skirt_edges.push(chain.to_vec());
    }
    true
}

/// Where the lines `p1 + s d1` and `p2 + u d2` cross (None when nearly parallel).
fn line_cross(p1: V2, d1: V2, p2: V2, d2: V2) -> Option<V2> {
    let den = geom::cross(d1, d2);
    if den.abs() < 0.34 {
        return None;
    }
    let s = geom::cross(geom::sub(p2, p1), d2) / den;
    Some(geom::add(p1, geom::scale(d1, s)))
}

/// One corner patch between the sidewalk ends `e1` and `e2` around node centre `c`.
fn fill_patch(c: V2, e1: &WalkEnd, e2: &WalkEnd, f: &mut CornerFill) {
    let (paved, strips) = (f.paved, f.strips);
    let reach = 25.0 + 1.5 * f.geo.lot_w;
    let out = &mut *f.out;
    let (i1, o1, i2, o2) = (
        geom::xy(e1.inner),
        geom::xy(e1.outer),
        geom::xy(e2.inner),
        geom::xy(e2.outer),
    );
    let near = |p: Option<V2>| p.filter(|q| geom::dist(*q, c) < reach);
    let poly: Vec<V2> = match (
        near(line_cross(i1, e1.dir, i2, e2.dir)),
        near(line_cross(o1, e1.dir, o2, e2.dir)),
    ) {
        (Some(k), Some(apex)) => vec![k, i1, o1, apex, o2, i2],
        _ => vec![i1, o1, o2, i2],
    };
    // Kerb-top plane through the four end points (least squares; flat when degenerate).
    let pts = [e1.inner, e1.outer, e2.inner, e2.outer];
    let plane = fit_plane(&pts, c);
    let z_at = |p: V2| plane[0] + plane[1] * (p[0] - c[0]) + plane[2] * (p[1] - c[1]);
    let cell = TRIM_CELL_M;
    let lo = poly
        .iter()
        .fold([f32::INFINITY; 2], |a, p| [a[0].min(p[0]), a[1].min(p[1])]);
    let hi = poly.iter().fold([f32::NEG_INFINITY; 2], |a, p| {
        [a[0].max(p[0]), a[1].max(p[1])]
    });
    let (ix0, ix1) = ((lo[0] / cell).floor() as i32, (hi[0] / cell).ceil() as i32);
    let (iy0, iy1) = ((lo[1] / cell).floor() as i32, (hi[1] / cell).ceil() as i32);
    let centre = |ix: i32, iy: i32| [(ix as f32 + 0.5) * cell, (iy as f32 + 0.5) * cell];
    let is_paved = |ix: i32, iy: i32| {
        let q = centre(ix, iy);
        paved.covers(q, z_at(q) - KERB_H_M)
    };
    let keep = |ix: i32, iy: i32| {
        let q = centre(ix, iy);
        geom::point_in_polygon(q, &poly) && !is_paved(ix, iy) && !strips.covers(q, z_at(q))
    };
    let corner = |x: f32, y: f32| [x, y, z_at([x, y])];
    // Kerb edges towards paved neighbours, collected per grid line and merged into runs:
    // horizontal lines keyed by (y line, facing ±y) → cell columns, vertical by (x line,
    // facing ±x) → cell rows.
    let mut h_edges: BTreeMap<(i32, i8), Vec<i32>> = BTreeMap::new();
    let mut v_edges: BTreeMap<(i32, i8), Vec<i32>> = BTreeMap::new();
    for iy in iy0..iy1 {
        let (y0, y1) = (iy as f32 * cell, (iy + 1) as f32 * cell);
        let mut ix = ix0;
        while ix < ix1 {
            if !keep(ix, iy) {
                ix += 1;
                continue;
            }
            let run0 = ix;
            while ix < ix1 && keep(ix, iy) {
                if is_paved(ix, iy - 1) {
                    h_edges.entry((iy, -1)).or_default().push(ix);
                }
                if is_paved(ix, iy + 1) {
                    h_edges.entry((iy + 1, 1)).or_default().push(ix);
                }
                if is_paved(ix - 1, iy) {
                    v_edges.entry((ix, -1)).or_default().push(iy);
                }
                if is_paved(ix + 1, iy) {
                    v_edges.entry((ix + 1, 1)).or_default().push(iy);
                }
                ix += 1;
            }
            let (xa, xb) = (run0 as f32 * cell, ix as f32 * cell);
            // Counter-clockwise from above: faces up.
            out.corners.quad(
                corner(xa, y0),
                corner(xb, y0),
                corner(xb, y1),
                corner(xa, y1),
            );
        }
    }
    // Kerb faces: vertical quads from the top down by the kerb height, facing the pavement.
    let mut kerb = |a: V2, b: V2| {
        let (ta, tb) = (corner(a[0], a[1]), corner(b[0], b[1]));
        let foot = |t: V3| [t[0], t[1], t[2] - KERB_H_M];
        out.kerbs.strip_vertical(&[ta, tb], &[foot(ta), foot(tb)]);
    };
    for ((line, facing), cols) in &mut h_edges {
        cols.sort_unstable();
        let y = *line as f32 * cell;
        for (c0, c1) in runs(cols) {
            let (xa, xb) = (c0 as f32 * cell, c1 as f32 * cell);
            // A face's outward normal is the right of its top edge's direction.
            if *facing < 0 {
                kerb([xa, y], [xb, y]);
            } else {
                kerb([xb, y], [xa, y]);
            }
        }
    }
    for ((line, facing), rows) in &mut v_edges {
        rows.sort_unstable();
        let x = *line as f32 * cell;
        for (r0, r1) in runs(rows) {
            let (ya, yb) = (r0 as f32 * cell, r1 as f32 * cell);
            if *facing < 0 {
                kerb([x, yb], [x, ya]);
            } else {
                kerb([x, ya], [x, yb]);
            }
        }
    }
}

/// Runs of consecutive integers in sorted `v`, as half-open `(start, end)` ranges.
fn runs(v: &[i32]) -> Vec<(i32, i32)> {
    let mut out: Vec<(i32, i32)> = Vec::new();
    for &x in v {
        match out.last_mut() {
            Some(last) if last.1 == x => last.1 = x + 1,
            _ => out.push((x, x + 1)),
        }
    }
    out
}

/// Least-squares plane `z = a + b (x - c.x) + d (y - c.y)` through `pts`, as `[a, b, d]`
/// (a flat plane at the mean height when the points are collinear).
fn fit_plane(pts: &[V3], c: V2) -> [f32; 3] {
    let n = pts.len() as f32;
    let mean_z = pts.iter().map(|p| p[2]).sum::<f32>() / n;
    let (mut sx, mut sy, mut sxx, mut syy, mut sxy, mut sz, mut sxz, mut syz) = (
        0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32,
    );
    for p in pts {
        let (x, y, z) = (p[0] - c[0], p[1] - c[1], p[2]);
        sx += x;
        sy += y;
        sxx += x * x;
        syy += y * y;
        sxy += x * y;
        sz += z;
        sxz += x * z;
        syz += y * z;
    }
    // Solve [[n sx sy][sx sxx sxy][sy sxy syy]] [a b d] = [sz sxz syz] (Cramer).
    let m = [[n, sx, sy], [sx, sxx, sxy], [sy, sxy, syy]];
    let det3 = |m: [[f32; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let det = det3(m);
    if det.abs() < 1e-3 {
        return [mean_z, 0.0, 0.0];
    }
    let r = [sz, sxz, syz];
    let col = |k: usize| {
        let mut mk = m;
        for (row, rv) in mk.iter_mut().zip(r) {
            row[k] = rv;
        }
        det3(mk) / det
    };
    let plane = [col(0), col(1), col(2)];
    // Corner patches are small; a slope steeper than the steepest road means a bad fit.
    if plane[1].abs() > 0.2 || plane[2].abs() > 0.2 {
        return [mean_z, 0.0, 0.0];
    }
    plane
}

/// Trim resolution of [`sidewalk_mesh`] where a sidewalk meets pavement, m (along the lane;
/// across, the sidewalk is split in `SIDEWALK_W_M / TRIM_CELL_M` cells).
pub const TRIM_CELL_M: f32 = 0.5;

/// Largest fan (angle between the end cross-sections) of one sidewalk / lot quad on a bend,
/// rad: keeps the lot level along its cross-sections within a few mm on the steepest grades.
const MAX_FAN_RAD: f32 = 0.025;

/// One sidewalk strip along a lane edge, with its lot.
struct SideWalk {
    /// Kerb line at sidewalk-top height.
    kerb_top: Vec<V3>,
    /// Sidewalk back edge ([`SIDEWALK_W_M`] out): where the lot starts.
    back: Vec<V3>,
    /// The lot's outer line (level with `back`).
    outer: Vec<V3>,
    /// Kerb line at road height.
    kerb_foot: Vec<V3>,
    /// Per segment: whether it has a lot (else the sidewalk ends at its back edge, railed).
    lot: Vec<bool>,
    outer_is_left: bool,
    /// The first [`VERGE_W_M`] from the kerb is a grass verge ([`lane_has_verge`]).
    verge: bool,
}

impl SideWalk {
    /// The strip's end at its start (`at_start`) or finish.
    fn end(&self, road: u32, at_start: bool) -> Option<WalkEnd> {
        let n = self.kerb_top.len();
        if n < 2 {
            return None;
        }
        let (k, k2) = if at_start { (0, 1) } else { (n - 1, n - 2) };
        // The end segment's outer edge: the lot's, or the sidewalk's back edge without one.
        let seg = if at_start { 0 } else { n - 2 };
        Some(WalkEnd {
            road,
            inner: self.kerb_top[k],
            outer: if self.lot[seg] {
                self.outer[k]
            } else {
                self.back[k]
            },
            back: self.lot[seg].then_some(self.back[k]),
            dir: geom::normalize(geom::sub(
                geom::xy(self.kerb_top[k2]),
                geom::xy(self.kerb_top[k]),
            )),
            corner: self.kerb_top[k],
            tail: Vec::new(),
        })
    }

    /// The strip pulled back by `d` metres (along its kerb line) at its start or finish; the
    /// three edge lines are cut at the same point fraction so their cross-sections stay
    /// paired. `None` when that would leave less than 1 m.
    fn pulled_back(&self, d: f32, at_start: bool) -> Option<SideWalk> {
        let rev = |v: &[V3]| v.iter().rev().copied().collect::<Vec<V3>>();
        let (kt, bk, ou, kf) = if at_start {
            (
                rev(&self.kerb_top),
                rev(&self.back),
                rev(&self.outer),
                rev(&self.kerb_foot),
            )
        } else {
            (
                self.kerb_top.clone(),
                self.back.clone(),
                self.outer.clone(),
                self.kerb_foot.clone(),
            )
        };
        // Cut d before the end of the (finish-oriented) lines.
        let cum = geom::cumulative(&kt);
        let total = *cum.last()?;
        if total - d < 1.0 {
            return None;
        }
        let s = total - d;
        let k = cum.partition_point(|c| *c <= s).clamp(1, cum.len() - 1);
        let t = (s - cum[k - 1]) / (cum[k] - cum[k - 1]).max(1e-6);
        let cut = |v: &[V3]| -> Vec<V3> {
            let (a, b) = (v[k - 1], v[k]);
            let mut out = v[..k].to_vec();
            out.push([
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            ]);
            out
        };
        let (mut kt, mut bk, mut ou, mut kf) = (cut(&kt), cut(&bk), cut(&ou), cut(&kf));
        // Segment flags in the same (finish-oriented) order: the first k survive, the last cut.
        let mut lot = if at_start {
            self.lot.iter().rev().copied().collect::<Vec<bool>>()
        } else {
            self.lot.clone()
        };
        lot.truncate(k);
        if at_start {
            kt.reverse();
            bk.reverse();
            ou.reverse();
            kf.reverse();
            lot.reverse();
        }
        Some(SideWalk {
            kerb_top: kt,
            back: bk,
            outer: ou,
            kerb_foot: kf,
            lot,
            outer_is_left: self.outer_is_left,
            verge: self.verge,
        })
    }

    /// Back-edge segments without a lot, the sidewalk on their left.
    fn bare_backs(&self) -> Vec<(V3, V3)> {
        (0..self.lot.len())
            .filter(|k| !self.lot[*k])
            .map(|k| {
                let (a, b) = (self.back[k], self.back[k + 1]);
                if self.outer_is_left {
                    (b, a)
                } else {
                    (a, b)
                }
            })
            .collect()
    }

    /// The outline of every run of lot segments away from the road — across the run's first
    /// cross-section, along the lot's outer line, back across its last — oriented with the
    /// outside on its right (skirt candidates; the parts over a corner fill or the next strip
    /// are dropped later).
    fn skirt_edges(&self) -> Vec<Vec<V3>> {
        let mut out: Vec<Vec<V3>> = Vec::new();
        let mut k = 0;
        while k < self.lot.len() {
            if !self.lot[k] {
                k += 1;
                continue;
            }
            let k0 = k;
            while k < self.lot.len() && self.lot[k] {
                k += 1;
            }
            // Points k0 ..= k.
            let mut run: Vec<V3> = vec![self.back[k0]];
            run.extend_from_slice(&self.outer[k0..=k]);
            run.push(self.back[k]);
            if self.outer_is_left {
                run.reverse();
            }
            out.push(run);
        }
        out
    }

    fn emit(&self, paved: &PavedIndex, out: &mut SidewalkMesh) {
        let verge_frac = VERGE_W_M / SIDEWALK_W_M;
        let lerp = |a: V3, b: V3, t: f32| -> V3 {
            [
                a[0] + (b[0] - a[0]) * t,
                a[1] + (b[1] - a[1]) * t,
                a[2] + (b[2] - a[2]) * t,
            ]
        };
        for k in 0..self.kerb_top.len().saturating_sub(1) {
            let (i0, i1) = (self.kerb_top[k], self.kerb_top[k + 1]);
            let (m0, m1) = (self.back[k], self.back[k + 1]);
            let (o0, o1) = if self.lot[k] {
                (self.outer[k], self.outer[k + 1])
            } else {
                (m0, m1)
            };
            let (f0, f1) = (self.kerb_foot[k], self.kerb_foot[k + 1]);
            // Cells across, and the fraction of the width that is sidewalk (the rest is lot).
            let width =
                geom::dist(geom::xy(i0), geom::xy(o0)).max(geom::dist(geom::xy(i1), geom::xy(o1)));
            let across = (width / TRIM_CELL_M).round().max(1.0) as usize;
            let walk_frac = |t: f32| {
                let (ii, mm, oo) = (lerp(i0, i1, t), lerp(m0, m1, t), lerp(o0, o1, t));
                let w = geom::dist(geom::xy(ii), geom::xy(oo));
                if w < 1e-3 {
                    1.0
                } else {
                    geom::dist(geom::xy(ii), geom::xy(mm)) / w
                }
            };
            let len =
                geom::dist(geom::xy(i0), geom::xy(i1)).max(geom::dist(geom::xy(o0), geom::xy(o1)));
            let rows = ((len / TRIM_CELL_M).ceil() as usize).max(1);
            // On a bend the cross-sections at the two ends fan out, and one quad from the kerb
            // to the lot's outer edge is then not level along the cross-sections in between
            // on a grade: merge clean rows into quads spanning at most MAX_FAN_RAD of fan.
            let (u0, u1) = (
                geom::normalize(geom::sub(geom::xy(o0), geom::xy(i0))),
                geom::normalize(geom::sub(geom::xy(o1), geom::xy(i1))),
            );
            let fan = wp_math::atan2(geom::cross(u0, u1).abs(), geom::dot(u0, u1));
            let pieces = ((fan / MAX_FAN_RAD).ceil() as usize).clamp(1, rows);
            let max_merge = rows.div_ceil(pieces);
            // Pavement that reaches past the kerb line onto this stretch of sidewalk (the
            // lane's own road ends at the kerb line, so it never counts).
            let quad = [geom::xy(i0), geom::xy(i1), geom::xy(o1), geom::xy(o0)];
            let near = paved.reaching_into(&quad, (i0[2] + i1[2]) * 0.5 - KERB_H_M);
            // Cell (row, col) centre on pavement → dropped.
            let on_paved = |r: usize, c: usize| {
                if near.is_empty() {
                    return false;
                }
                let t = (r as f32 + 0.5) / rows as f32;
                let u = (c as f32 + 0.5) / across as f32;
                let p = lerp(lerp(i0, i1, t), lerp(o0, o1, t), u);
                paved.covers_among(&near, geom::xy(p), p[2] - KERB_H_M)
            };
            let kept: Vec<Vec<bool>> = (0..rows)
                .map(|r| (0..across).map(|c| !on_paved(r, c)).collect())
                .collect();
            let at = |r: usize| r as f32 / rows as f32;
            // Tops: runs of clean rows become one quad (exactly the plain strip when the
            // whole segment is clean); a trimmed row gets one quad per run of kept cells.
            let mut r = 0;
            while r < rows {
                if kept[r].iter().all(|k| *k) {
                    let r0 = r;
                    while r < rows && r - r0 < max_merge && kept[r].iter().all(|k| *k) {
                        r += 1;
                    }
                    let (t0, t1) = (at(r0), at(r));
                    let (b0, b1) = (lerp(m0, m1, t0), lerp(m0, m1, t1));
                    let (a0, a1) = (lerp(i0, i1, t0), lerp(i0, i1, t1));
                    if self.verge {
                        // Kerb → verge edge (grass), verge edge → back edge (walkway).
                        let (v0, v1) = (lerp(a0, b0, verge_frac), lerp(a1, b1, verge_frac));
                        self.quad(&mut out.verges, a0, a1, v0, v1);
                        self.quad(&mut out.tops, v0, v1, b0, b1);
                    } else {
                        self.quad(&mut out.tops, a0, a1, b0, b1);
                    }
                    let (q0, q1) = (lerp(o0, o1, t0), lerp(o0, o1, t1));
                    if geom::dist(geom::xy(b0), geom::xy(q0)) > 1e-3
                        || geom::dist(geom::xy(b1), geom::xy(q1)) > 1e-3
                    {
                        self.quad(&mut out.lots, b0, b1, q0, q1);
                    }
                    continue;
                }
                let (t0, t1) = (at(r), at(r + 1));
                let (a0, a1) = (lerp(i0, i1, t0), lerp(i0, i1, t1));
                let (b0, b1) = (lerp(o0, o1, t0), lerp(o0, o1, t1));
                // Cells of this row on the sidewalk / on the lot (by the cell centre).
                let split = walk_frac((t0 + t1) * 0.5);
                // 0 = verge, 1 = walkway, 2 = lot (by the cell centre).
                let band = |c: usize| {
                    let u = (c as f32 + 0.5) / across as f32;
                    if u > split {
                        2
                    } else if self.verge && u < split * verge_frac {
                        0
                    } else {
                        1
                    }
                };
                for which in 0..3 {
                    let cols: Vec<i32> = (0..across)
                        .filter(|c| kept[r][*c])
                        .filter(|c| band(*c) == which)
                        .map(|c| c as i32)
                        .collect();
                    let mesh = match which {
                        0 => &mut out.verges,
                        1 => &mut out.tops,
                        _ => &mut out.lots,
                    };
                    for (c0, c1) in runs(&cols) {
                        let (u0, u1) = (c0 as f32 / across as f32, c1 as f32 / across as f32);
                        self.quad(
                            mesh,
                            lerp(a0, b0, u0),
                            lerp(a1, b1, u0),
                            lerp(a0, b0, u1),
                            lerp(a1, b1, u1),
                        );
                    }
                }
                r += 1;
            }
            // Kerb faces wherever the cell against the kerb line is kept, one per run.
            let inner: Vec<i32> = (0..rows)
                .filter(|r| kept[*r][0])
                .map(|r| r as i32)
                .collect();
            for (r0, r1) in runs(&inner) {
                let (t0, t1) = (at(r0 as usize), at(r1 as usize));
                self.kerb(
                    out,
                    [lerp(i0, i1, t0), lerp(i0, i1, t1)],
                    [lerp(f0, f1, t0), lerp(f0, f1, t1)],
                );
            }
        }
    }

    /// Kerb face along the kerb line `top` (in strip direction) down to `foot`, facing the
    /// road (the outward normal of [`TriMesh::strip_vertical`] is the right of its top edge).
    fn kerb(&self, out: &mut SidewalkMesh, top: [V3; 2], foot: [V3; 2]) {
        if self.outer_is_left {
            // Road on the right of the strip direction.
            out.kerbs.strip_vertical(&top, &foot);
        } else {
            out.kerbs
                .strip_vertical(&[top[1], top[0]], &[foot[1], foot[0]]);
        }
    }

    /// Top quad (into `m`) from the inner edge (`in0` → `in1`) to the outer edge (`out0` →
    /// `out1`).
    fn quad(&self, m: &mut TriMesh, in0: V3, in1: V3, out0: V3, out1: V3) {
        if self.outer_is_left {
            m.strip(&[out0, out1], &[in0, in1]);
        } else {
            m.strip(&[in0, in1], &[out0, out1]);
        }
    }
}

/// Triangles ([`road_mesh`], or sidewalk tops) bucketed by xy on a dense grid, for "is this
/// point on pavement?".
struct PavedIndex {
    tris: Vec<[V3; 3]>,
    /// Bucket of (0, 0), in buckets.
    origin: (i32, i32),
    /// Buckets along x and y.
    size: (i32, i32),
    /// Triangle ids per bucket, row-major.
    cells: Vec<Vec<u32>>,
}

impl PavedIndex {
    const BUCKET_M: f32 = 8.0;

    fn key(p: V2) -> (i32, i32) {
        (
            (p[0] / Self::BUCKET_M).floor() as i32,
            (p[1] / Self::BUCKET_M).floor() as i32,
        )
    }

    fn new(m: &TriMesh) -> Self {
        let tris: Vec<[V3; 3]> = m
            .indices
            .iter()
            .map(|t| t.map(|i| m.positions[i as usize]))
            .collect();
        let bounds = |v: &[V3; 3]| {
            (
                Self::key([
                    v[0][0].min(v[1][0]).min(v[2][0]),
                    v[0][1].min(v[1][1]).min(v[2][1]),
                ]),
                Self::key([
                    v[0][0].max(v[1][0]).max(v[2][0]),
                    v[0][1].max(v[1][1]).max(v[2][1]),
                ]),
            )
        };
        let (mut lo, mut hi) = ((i32::MAX, i32::MAX), (i32::MIN, i32::MIN));
        for v in &tris {
            let (a, b) = bounds(v);
            lo = (lo.0.min(a.0), lo.1.min(a.1));
            hi = (hi.0.max(b.0), hi.1.max(b.1));
        }
        if tris.is_empty() {
            return Self {
                tris,
                origin: (0, 0),
                size: (0, 0),
                cells: Vec::new(),
            };
        }
        let size = (hi.0 - lo.0 + 1, hi.1 - lo.1 + 1);
        let mut cells = vec![Vec::new(); (size.0 * size.1) as usize];
        for (id, v) in tris.iter().enumerate() {
            let (a, b) = bounds(v);
            for by in a.1..=b.1 {
                for bx in a.0..=b.0 {
                    cells[((by - lo.1) * size.0 + (bx - lo.0)) as usize].push(id as u32);
                }
            }
        }
        Self {
            tris,
            origin: lo,
            size,
            cells,
        }
    }

    fn bucket(&self, p: V2) -> &[u32] {
        let (bx, by) = Self::key(p);
        let (x, y) = (bx - self.origin.0, by - self.origin.1);
        if x < 0 || y < 0 || x >= self.size.0 || y >= self.size.1 {
            return &[];
        }
        &self.cells[(y * self.size.0 + x) as usize]
    }

    /// Triangles within 1.5 m of height `z` that reach strictly inside the convex
    /// quadrilateral `quad` (its first edge is the kerb line; a triangle must have a vertex
    /// more than 5 cm past it) — sorted ids, no duplicates.
    fn reaching_into(&self, quad: &[V2; 4], z: f32) -> Vec<u32> {
        let lo = quad
            .iter()
            .fold([f32::INFINITY; 2], |a, p| [a[0].min(p[0]), a[1].min(p[1])]);
        let hi = quad.iter().fold([f32::NEG_INFINITY; 2], |a, p| {
            [a[0].max(p[0]), a[1].max(p[1])]
        });
        let (k0, k1) = (Self::key(lo), Self::key(hi));
        // Unit normal of the kerb line pointing into the quad.
        let d = geom::normalize(geom::sub(quad[1], quad[0]));
        let mut n = geom::right_of(d);
        if geom::dot(geom::sub(quad[3], quad[0]), n) < 0.0 {
            n = geom::scale(n, -1.0);
        }
        let mut out: Vec<u32> = Vec::new();
        for by in k0.1..=k1.1 {
            for bx in k0.0..=k1.0 {
                let p = [
                    (bx as f32 + 0.5) * Self::BUCKET_M,
                    (by as f32 + 0.5) * Self::BUCKET_M,
                ];
                for &id in self.bucket(p) {
                    let v = &self.tris[id as usize];
                    let tlo = [
                        v[0][0].min(v[1][0]).min(v[2][0]),
                        v[0][1].min(v[1][1]).min(v[2][1]),
                    ];
                    let thi = [
                        v[0][0].max(v[1][0]).max(v[2][0]),
                        v[0][1].max(v[1][1]).max(v[2][1]),
                    ];
                    let overlaps =
                        tlo[0] <= hi[0] && thi[0] >= lo[0] && tlo[1] <= hi[1] && thi[1] >= lo[1];
                    let past_kerb = v
                        .iter()
                        .any(|q| geom::dot(geom::sub(geom::xy(*q), quad[0]), n) > 0.05);
                    let level = v.iter().any(|q| (q[2] - z).abs() < 1.5);
                    if overlaps && past_kerb && level {
                        out.push(id);
                    }
                }
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    /// [`PavedIndex::covers`] over the triangles `ids` only.
    fn covers_among(&self, ids: &[u32], p: V2, z: f32) -> bool {
        ids.iter().any(|&id| self.tri_covers(id, p, z))
    }

    /// Whether a triangle within 0.5 m of height `z` contains `p` at least 2 cm inside its
    /// edges (touching a shared edge does not count).
    fn covers_inside(&self, p: V2, z: f32) -> bool {
        self.bucket(p).iter().any(|&id| {
            let v = &self.tris[id as usize];
            let [a, b, c] = v.map(geom::xy);
            let area2 = geom::cross(geom::sub(b, a), geom::sub(c, a));
            if area2.abs() < 1e-9 {
                return false;
            }
            let s = area2.signum();
            let inside = |u: V2, w: V2| {
                let e = geom::sub(w, u);
                s * geom::cross(e, geom::sub(p, u)) / geom::len(e).max(1e-9) > 0.02
            };
            if !(inside(a, b) && inside(b, c) && inside(c, a)) {
                return false;
            }
            let l1 = geom::cross(geom::sub(b, p), geom::sub(c, p)) / area2;
            let l2 = geom::cross(geom::sub(c, p), geom::sub(a, p)) / area2;
            let zt = l1 * v[0][2] + l2 * v[1][2] + (1.0 - l1 - l2) * v[2][2];
            (zt - z).abs() < 0.5
        })
    }

    /// Whether a paved triangle within 1.5 m of height `z` lies under `p`.
    fn covers(&self, p: V2, z: f32) -> bool {
        self.bucket(p).iter().any(|&id| self.tri_covers(id, p, z))
    }

    /// Whether triangle `id` lies under `p` within 1.5 m of height `z`.
    fn tri_covers(&self, id: u32, p: V2, z: f32) -> bool {
        let v = &self.tris[id as usize];
        let (x1, y1, x2, y2, x3, y3) = (v[0][0], v[0][1], v[1][0], v[1][1], v[2][0], v[2][1]);
        let d = (y2 - y3) * (x1 - x3) + (x3 - x2) * (y1 - y3);
        if d.abs() < 1e-9 {
            return false;
        }
        let l1 = ((y2 - y3) * (p[0] - x3) + (x3 - x2) * (p[1] - y3)) / d;
        let l2 = ((y3 - y1) * (p[0] - x3) + (x1 - x3) * (p[1] - y3)) / d;
        let l3 = 1.0 - l1 - l2;
        l1 >= 0.0
            && l2 >= 0.0
            && l3 >= 0.0
            && (l1 * v[0][2] + l2 * v[1][2] + l3 * v[2][2] - z).abs() < 1.5
    }
}

/// Whether the whole cross-section of `s` at point `k` is more than `gap` metres above
/// `ground` (a bridge deck rather than an embankment).
pub fn is_elevated(s: &RoadSurface, k: usize, ground: &dyn Fn(V2) -> f32, gap: f32) -> bool {
    let n = s.centre.len();
    let p = s.centre[k];
    let tan = if k + 1 < n {
        geom::normalize(geom::sub(geom::xy(s.centre[k + 1]), geom::xy(p)))
    } else {
        geom::normalize(geom::sub(geom::xy(p), geom::xy(s.centre[k - 1])))
    };
    let r = geom::right_of(tan);
    [-s.left_m, 0.0, s.right_m].iter().all(|lat| {
        let q = geom::add(geom::xy(p), geom::scale(r, *lat));
        s.z_at(k, *lat) - ground(q) > gap
    })
}

/// Guard-rail lines `(a, b)` (at road height) along both edges of every bridge-deck stretch:
/// surface segments whose whole cross-section is more than 1.5 m above the terrain. Empty
/// without terrain.
pub fn deck_rails(net: &RoadNetwork) -> Vec<(V3, V3)> {
    let Some(t) = &net.terrain else {
        return Vec::new();
    };
    let ground = |q: V2| t.height_at(q);
    let mut out = Vec::new();
    for s in &net.surfaces {
        let (l, r) = surface_edges(s);
        for k in 0..s.centre.len() - 1 {
            if is_elevated(s, k, &ground, 1.5) && is_elevated(s, k + 1, &ground, 1.5) {
                out.push((l[k], l[k + 1]));
                out.push((r[k], r[k + 1]));
            }
        }
    }
    out
}

/// Vertex spacing of the terrain the greybox mesh draws ([`Heightfield::lowered_coarse`]), m.
pub const TERRAIN_RENDER_M: f32 = 10.0;

/// The terrain physics collides with, on the fine grid of `net.terrain` (`None` without
/// terrain): the surface the greybox draws ([`Heightfield::lowered_coarse`] at
/// [`TERRAIN_RENDER_M`]), except in the band around at-grade roads, junctions and bends
/// (paved width + sidewalk + lot + skirt + half a cell), which keeps the fine heights the
/// sidewalks, lots and skirts are laid on. Bridge decks and tunnels are not at grade, so the
/// ground above a tunnel and under a deck is the drawn ground. Drawn and solid ground agree
/// everywhere else, so there is no hillside the player can hit but not see; the fine grid
/// samples the coarse triangles exactly (same diagonal), so the two surfaces are identical
/// off the roads.
pub fn collision_terrain(map: &CityMap) -> Option<Heightfield> {
    let net = &map.net;
    let t = net.terrain.as_ref()?;
    let drawn = t.lowered_coarse(TERRAIN_RENDER_M);
    let mut keep = vec![false; t.z.len()];
    let margin = t.cell_m * 0.5;
    let beside = SIDEWALK_W_M + lot_width_m(net) + SKIRT_W_M;
    let mut mark = |a: V2, b: V2, r: f32| {
        let lo = [a[0].min(b[0]) - r, a[1].min(b[1]) - r];
        let hi = [a[0].max(b[0]) + r, a[1].max(b[1]) + r];
        let cell = |x: f32, o: f32, n: u32| (((x - o) / t.cell_m).max(0.0) as u32).min(n - 1);
        let (i0, i1) = (
            cell(lo[0], t.origin[0], t.nx),
            cell(hi[0], t.origin[0], t.nx) + 1,
        );
        let (j0, j1) = (
            cell(lo[1], t.origin[1], t.ny),
            cell(hi[1], t.origin[1], t.ny) + 1,
        );
        for j in j0..=j1.min(t.ny - 1) {
            for i in i0..=i1.min(t.nx - 1) {
                if geom::point_segment_distance(t.vertex_pos(i, j), a, b) <= r {
                    keep[(j * t.nx + i) as usize] = true;
                }
            }
        }
    };
    // Within 1.5 m of the fine ground: not a deck above it, not a tunnel under it.
    let at_grade = |p: V3| (p[2] - t.height_at(geom::xy(p))).abs() <= 1.5;
    for s in &net.surfaces {
        let side = if s.highway { 0.3 } else { beside };
        let r = s.left_m.max(s.right_m) + side + margin;
        for k in 0..s.centre.len() - 1 {
            if at_grade(s.centre[k]) || at_grade(s.centre[k + 1]) {
                mark(geom::xy(s.centre[k]), geom::xy(s.centre[k + 1]), r);
            }
        }
    }
    for ix in &net.intersections {
        let c = net.nodes[ix.node as usize].pos;
        if at_grade(c) {
            mark(geom::xy(c), geom::xy(c), ix.radius_m + beside + margin);
        }
    }
    for s in map.segs.iter() {
        if let SegKind::Connector { node, .. } = s.kind {
            if net.nodes[node as usize].intersection.is_none() {
                let r = s.width * 0.5 + beside + margin;
                for w in s.pts.windows(2) {
                    if at_grade(w[0]) || at_grade(w[1]) {
                        mark(geom::xy(w[0]), geom::xy(w[1]), r);
                    }
                }
            }
        }
    }
    let mut out = t.clone();
    for j in 0..t.ny {
        for i in 0..t.nx {
            let n = (j * t.nx + i) as usize;
            if !keep[n] {
                out.z[n] = drawn.height_on_triangles(t.vertex_pos(i, j));
            }
        }
    }
    Some(out)
}

/// Wheel probes against the city's roads and terrain (no physics engine): the drivable
/// surface nearest the probe origin's height ([`CityMap::ground_height`]), with a
/// finite-difference normal. On a flat city this is exactly the z = 0 plane.
pub struct MapGround<'a> {
    /// The city.
    pub map: &'a CityMap,
}

impl WheelCaster for MapGround<'_> {
    fn cast(
        &self,
        origin: wp_vehicle::math::V3,
        dir: wp_vehicle::math::V3,
        max_dist: f32,
    ) -> Option<GroundHit> {
        if dir.z >= -1e-6 {
            return None;
        }
        let p = [origin.x, origin.y];
        let h = self.map.ground_height(p, Some(origin.z));
        let t = (origin.z - h) / -dir.z;
        if !(0.0..=max_dist).contains(&t) {
            return None;
        }
        let e = 0.5;
        let hx = self.map.ground_height([p[0] + e, p[1]], Some(h))
            - self.map.ground_height([p[0] - e, p[1]], Some(h));
        let hy = self.map.ground_height([p[0], p[1] + e], Some(h))
            - self.map.ground_height([p[0], p[1] - e], Some(h));
        let normal = if hx == 0.0 && hy == 0.0 {
            wp_vehicle::math::V3::Z
        } else {
            wp_vehicle::math::V3::new(-hx / (2.0 * e), -hy / (2.0 * e), 1.0).normalize()
        };
        Some(GroundHit {
            distance: t,
            point: origin + dir * t,
            normal,
        })
    }
}

impl wp_contracts::GroundQuery for MapGround<'_> {
    fn cast_ray(
        &self,
        origin: [f32; 3],
        dir: [f32; 3],
        max_m: f32,
    ) -> Option<wp_contracts::GroundHit> {
        self.cast(
            wp_vehicle::math::V3::from_array(origin),
            wp_vehicle::math::V3::from_array(dir),
            max_m,
        )
        .map(|h| wp_contracts::GroundHit {
            dist_m: h.distance,
            point: h.point.to_array(),
            normal: h.normal.to_array(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::SegKind;

    /// Points across every drivable lane and junction turning path (`margin` inside its
    /// edges) where a sidewalk top lies over it: a kerb step inside the roadway.
    fn sidewalk_on_roadway(map: &CityMap, margin: f32) -> (usize, usize, Vec<V2>) {
        let tops = PavedIndex::new(&sidewalk_mesh(map).flush());
        let (mut n, mut bad, mut examples) = (0, 0, Vec::new());
        for s in map.segs.iter().filter(|s| s.drivable) {
            let half = s.width * 0.5 - margin;
            if half <= 0.0 {
                continue;
            }
            let mut d = 0.25;
            while d < s.length {
                let (p, dir) = s.point_at(d);
                let r = geom::right_of(dir);
                let mut lat = -half;
                while lat <= half {
                    let q = geom::add(geom::xy(p), geom::scale(r, lat));
                    n += 1;
                    if tops.covers(q, p[2] + KERB_H_M) {
                        bad += 1;
                        if examples.len() < 8 {
                            examples.push(q);
                        }
                    }
                    lat += 0.5;
                }
                d += 1.0;
            }
        }
        (n, bad, examples)
    }

    #[test]
    fn sidewalks_stay_off_the_roadway() {
        for (name, net) in [
            (
                "vesper_v2",
                crate::vesper::generate(&crate::vesper::VesperConfig::default()),
            ),
            (
                "grid",
                crate::procgen::generate(&crate::procgen::ProcGenConfig::default()),
            ),
        ] {
            let map = CityMap::new(net);
            let (n, bad, examples) = sidewalk_on_roadway(&map, 0.3);
            assert!(n > 100_000, "{name}: {n} samples");
            assert_eq!(
                bad, 0,
                "{name}: {bad} of {n} roadway points under a sidewalk, e.g. {examples:?}"
            );
        }
    }

    /// Both shipped cities.
    fn cities() -> Vec<(&'static str, CityMap)> {
        vec![
            (
                "vesper_v2",
                CityMap::new(crate::vesper::generate(
                    &crate::vesper::VesperConfig::default(),
                )),
            ),
            (
                "grid",
                CityMap::new(crate::procgen::generate(
                    &crate::procgen::ProcGenConfig::default(),
                )),
            ),
        ]
    }

    /// Height of the highest triangle of `idx` under `p` within `tol` of `z`, if any.
    fn surface_z(idx: &PavedIndex, p: V2, z: f32, tol: f32) -> Option<f32> {
        let mut best: Option<f32> = None;
        for &id in idx.bucket(p) {
            let v = &idx.tris[id as usize];
            let [a, b, c] = v.map(geom::xy);
            let area2 = geom::cross(geom::sub(b, a), geom::sub(c, a));
            if area2.abs() < 1e-9 {
                continue;
            }
            let l1 = geom::cross(geom::sub(b, p), geom::sub(c, p)) / area2;
            let l2 = geom::cross(geom::sub(c, p), geom::sub(a, p)) / area2;
            let l3 = 1.0 - l1 - l2;
            if l1 < -1e-5 || l2 < -1e-5 || l3 < -1e-5 {
                continue;
            }
            let zt = l1 * v[0][2] + l2 * v[1][2] + l3 * v[2][2];
            if (zt - z).abs() <= tol && best.is_none_or(|bz| (zt - z).abs() < (bz - z).abs()) {
                best = Some(zt);
            }
        }
        best
    }

    /// Every drivable turning path through a junction, bend or dead end (and every lane
    /// within its area) lies on pavement: sampled every 0.25 m along and across, 0.15 m
    /// inside its edges, the road mesh is under it within 0.15 m of its height and no
    /// sidewalk top is over it.
    #[test]
    fn node_patches_pave_every_turning_path() {
        for (name, map) in cities() {
            let patches = node_patches(&map);
            let road = PavedIndex::new(&road_mesh_with(&map, &patches));
            let tops = PavedIndex::new(&sidewalk_mesh(&map).flush());
            let (mut n, mut holes, mut walks, mut worst) = (0usize, Vec::new(), Vec::new(), 0f32);
            for patch in &patches {
                let c = map.net.nodes[patch.node as usize].pos;
                let reach = patch
                    .mesh
                    .positions
                    .iter()
                    .map(|p| geom::dist(geom::xy(*p), geom::xy(c)))
                    .fold(0.0f32, f32::max)
                    + 4.0;
                for s in map.segs.iter().filter(|s| s.drivable) {
                    match s.kind {
                        SegKind::Connector { node, .. } if node != patch.node => continue,
                        _ => {}
                    }
                    let half = s.width * 0.5 - 0.15;
                    let mut d = 0.0;
                    while d <= s.length {
                        let (p, dir) = s.point_at(d);
                        d += 0.25;
                        if geom::dist(geom::xy(p), geom::xy(c)) > reach || (p[2] - c[2]).abs() > 3.0
                        {
                            continue;
                        }
                        let r = geom::right_of(dir);
                        let mut lat = -half;
                        while lat <= half + 1e-3 {
                            let q = geom::add(geom::xy(p), geom::scale(r, lat));
                            lat += 0.25;
                            n += 1;
                            // Overlapping ribbon ends lie on the patch at their own
                            // heights (see `NodePatch::overlapping_ends`).
                            let tol = if patch.overlapping_ends { 0.3 } else { 0.15 };
                            match surface_z(&road, q, p[2], tol) {
                                Some(z) => worst = worst.max((z - p[2]).abs()),
                                None if holes.len() < 10 => holes.push((patch.node, q)),
                                None => holes.push((patch.node, q)),
                            }
                            if surface_z(&tops, q, p[2] + KERB_H_M, 0.3).is_some() {
                                walks.push((patch.node, q));
                            }
                        }
                    }
                }
            }
            println!("{name}: {n} samples, pavement within {worst:.3} m of the paths");
            assert!(n > 500_000, "{name}: {n} samples");
            assert!(
                holes.is_empty(),
                "{name}: {} path samples off the pavement, e.g. {:?}",
                holes.len(),
                &holes[..holes.len().min(10)]
            );
            assert!(
                walks.is_empty(),
                "{name}: {} path samples under a sidewalk, e.g. {:?}",
                walks.len(),
                &walks[..walks.len().min(10)]
            );
        }
    }

    /// Free (unshared) edges of `m` within the node areas, as (a, b) with the mesh on the
    /// left of a → b.
    fn free_edges(m: &TriMesh, areas: &[(V2, f32)]) -> Vec<(V3, V3)> {
        let key = |p: V3| p.map(f32::to_bits);
        let mut count: BTreeMap<([u32; 3], [u32; 3]), u32> = BTreeMap::new();
        for t in &m.indices {
            for k in 0..3 {
                let (a, b) = (
                    m.positions[t[k] as usize],
                    m.positions[t[(k + 1) % 3] as usize],
                );
                let (ka, kb) = (key(a), key(b));
                *count.entry((ka.min(kb), ka.max(kb))).or_default() += 1;
            }
        }
        let mut out = Vec::new();
        for t in &m.indices {
            for k in 0..3 {
                let (a, b) = (
                    m.positions[t[k] as usize],
                    m.positions[t[(k + 1) % 3] as usize],
                );
                let (ka, kb) = (key(a), key(b));
                if count[&(ka.min(kb), ka.max(kb))] != 1 {
                    continue;
                }
                let mid = geom::scale(geom::add(geom::xy(a), geom::xy(b)), 0.5);
                if areas.iter().any(|(c, r)| geom::dist(mid, *c) < *r) {
                    out.push((a, b));
                }
            }
        }
        out
    }

    /// Around every junction, bend and dead end the surfaces join without cracks: just outside
    /// every free edge of the road or sidewalk mesh lies the neighbouring surface at the same
    /// height (≤ 1 cm), a kerb (the other mesh one kerb height away), or nothing at all for at
    /// least 30 cm (the outer edge of the built-up surface). On the pavement a step > 1 cm or a
    /// gap (nothing at 1 cm, pavement within 30 cm) is never allowed. Sidewalk slivers — where
    /// a sidewalk ends next to a road without one (a highway), a tapering wedge of ground up to
    /// 30 cm wide between the sidewalk end and the pavement — are counted and capped.
    #[test]
    fn junction_surfaces_have_no_cracks() {
        for (name, map) in cities() {
            let patches = node_patches(&map);
            let road_m = road_mesh_with(&map, &patches);
            let walks = sidewalk_mesh(&map);
            let walk_m = walks.flush();
            let (road, walk) = (PavedIndex::new(&road_m), PavedIndex::new(&walk_m));
            // Known exceptions, counted and capped: corners filled in cells and junctions whose
            // ribbon ends overlap.
            let overlapping: Vec<u32> = patches
                .iter()
                .filter(|p| p.overlapping_ends)
                .map(|p| p.node)
                .collect();
            println!(
                "{name}: {} corners in cells, overlapping ends at nodes {overlapping:?}",
                walks.cell_corners.len()
            );
            let cap = if name == "grid" { 0 } else { 4 };
            assert!(
                walks.cell_corners.len() <= cap && overlapping.len() <= cap / 4,
                "{name}: {} cell corners {:?}, overlapping {overlapping:?}",
                walks.cell_corners.len(),
                walks.cell_corners
            );
            let areas: Vec<(V2, f32)> = patches
                .iter()
                .filter(|p| !p.overlapping_ends)
                .filter(|p| {
                    let c = geom::xy(map.net.nodes[p.node as usize].pos);
                    !walks.cell_corners.iter().any(|q| geom::dist(*q, c) < 0.01)
                })
                .map(|p| {
                    let c = geom::xy(map.net.nodes[p.node as usize].pos);
                    let r = p
                        .mesh
                        .positions
                        .iter()
                        .map(|q| geom::dist(geom::xy(*q), c))
                        .fold(0.0f32, f32::max);
                    (c, r + SIDEWALK_W_M + lot_width_m(&map.net) + 1.0)
                })
                .collect();
            let mut cracks: Vec<String> = Vec::new();
            let mut checked = 0usize;
            // (mesh, its index, the other index, the other's height offset at a kerb)
            for (what, m, same, other, kerb) in [
                ("road", &road_m, &road, &walk, KERB_H_M),
                ("sidewalk", &walk_m, &walk, &road, -KERB_H_M),
            ] {
                for (a, b) in free_edges(m, &areas) {
                    let d = geom::sub(geom::xy(b), geom::xy(a));
                    if geom::len(d) < 0.02 {
                        continue;
                    }
                    let out = geom::right_of(geom::normalize(d));
                    for t in [0.25f32, 0.5, 0.75] {
                        checked += 1;
                        let p = geom::add(geom::xy(a), geom::scale(d, t));
                        let z = a[2] + (b[2] - a[2]) * t;
                        let at = |dist: f32| geom::add(p, geom::scale(out, dist));
                        let q = at(0.01);
                        let bad = if let Some(zs) = surface_z(same, q, z, 0.6) {
                            ((zs - z).abs() > 0.01).then(|| format!("step {:+.3}", zs - z))
                        } else if surface_z(other, q, z + kerb, 0.02).is_some() {
                            None
                        } else {
                            [0.05f32, 0.1, 0.2, 0.3].iter().find_map(|dd| {
                                let q = at(*dd);
                                if surface_z(same, q, z, 0.3).is_some() {
                                    Some(format!("gap {dd} m to {what}"))
                                } else {
                                    surface_z(other, q, z + kerb, 0.05)
                                        .map(|_| format!("gap {dd} m across the kerb"))
                                }
                            })
                        };
                        if let Some(why) = bad {
                            let node = patches
                                .iter()
                                .filter(|p| !p.overlapping_ends)
                                .filter(|p| {
                                    let c = geom::xy(map.net.nodes[p.node as usize].pos);
                                    !walks.cell_corners.iter().any(|q| geom::dist(*q, c) < 0.01)
                                })
                                .zip(&areas)
                                .filter(|(_, (c, r))| geom::dist(p, *c) < *r)
                                .min_by(|x, y| {
                                    geom::dist(p, x.1 .0).total_cmp(&geom::dist(p, y.1 .0))
                                })
                                .map_or(u32::MAX, |(pt, _)| pt.node);
                            cracks.push(format!(
                                "node {node}: {what} edge at ({:.2}, {:.2}): {why}",
                                p[0], p[1]
                            ));
                        }
                    }
                }
            }
            println!(
                "{name}: {checked} free-edge samples, {} cracks",
                cracks.len()
            );
            for c in &cracks {
                println!("  {c}");
            }
            assert!(checked > 10_000, "{name}: {checked}");
            // Pavement to pavement: no gap, no step.
            let paving: Vec<&String> = cracks
                .iter()
                .filter(|c| {
                    c.contains("to road") || (c.contains("road edge") && c.contains("step"))
                })
                .collect();
            assert!(paving.is_empty(), "{name}: pavement cracks {paving:?}");
            let slivers = cracks.len() - paving.len();
            // Measured at the fix: grid 18 (its four highway crossings), Port Vesper 30; 50
            // after the residential rework (lead/city-residential, rows 94–95): 9 wedges at
            // Loop 9 mouths (the highway's new shoulders), 1–1.4 cm steps at downtown arterial
            // junctions (painted medians) and 1–2.6 cm at the new courts' T-junctions.
            let budget = if name == "grid" { 20 } else { 55 };
            assert!(
                slivers <= budget,
                "{name}: {slivers} sidewalk sliver samples (budget {budget})"
            );
        }
    }

    /// No folded or degenerate pavement: every road-mesh and sidewalk-top triangle faces up.
    #[test]
    fn road_and_sidewalk_triangles_face_up() {
        for (name, map) in cities() {
            for (what, m) in [
                ("road", road_mesh(&map)),
                ("sidewalk", sidewalk_mesh(&map).flush()),
            ] {
                let down: Vec<V3> = m
                    .indices
                    .iter()
                    .filter_map(|t| {
                        let [a, b, c] = t.map(|i| m.positions[i as usize]);
                        let area = geom::cross(
                            geom::sub(geom::xy(b), geom::xy(a)),
                            geom::sub(geom::xy(c), geom::xy(a)),
                        );
                        (area <= 0.0).then_some(a)
                    })
                    .collect();
                assert!(
                    down.is_empty(),
                    "{name} {what}: {} facing down, e.g. {:?}",
                    down.len(),
                    &down[..down.len().min(5)]
                );
            }
        }
    }

    #[test]
    fn kerb_faces_and_tops_meet_and_face_out() {
        let map = CityMap::new(crate::procgen::generate(
            &crate::procgen::ProcGenConfig::default(),
        ));
        let w = sidewalk_mesh(&map);
        assert!(!w.tops.indices.is_empty() && !w.kerbs.indices.is_empty());
        let normal = |m: &TriMesh, t: &[u32; 3]| {
            let [a, b, c] = t.map(|i| m.positions[i as usize]);
            let (u, v) = (
                [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
                [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
            );
            [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ]
        };
        // Tops face up; kerb faces are vertical, KERB_H_M tall.
        let flush = w.flush();
        for t in &flush.indices {
            assert!(normal(&flush, t)[2] > 0.0, "top facing down: {t:?}");
        }
        for t in &w.kerbs.indices {
            let n = normal(&w.kerbs, t);
            assert!(
                n[2].abs() < 1e-3 * (n[0].abs() + n[1].abs()),
                "kerb face not vertical"
            );
            let zs = t.map(|i| w.kerbs.positions[i as usize][2]);
            let span = zs.iter().fold(f32::NEG_INFINITY, |a, z| a.max(*z))
                - zs.iter().fold(f32::INFINITY, |a, z| a.min(*z));
            assert!(
                span > KERB_H_M - 0.02 && span < KERB_H_M + 0.05,
                "kerb span {span}"
            );
        }
        // On the flat grid every kerb face looks at pavement (z = 0) half a trim cell out from
        // its middle (corner-patch kerbs follow the pavement in TRIM_CELL_M steps).
        let paved = PavedIndex::new(&road_mesh(&map));
        let mut looking_at_road = 0;
        for t in w.kerbs.indices.chunks(2) {
            let tri = &t[0];
            let n = normal(&w.kerbs, tri);
            let l = (n[0] * n[0] + n[1] * n[1]).sqrt();
            let [a, b, c] = tri.map(|i| w.kerbs.positions[i as usize]);
            let mid = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0];
            let out = TRIM_CELL_M * 0.5 + 0.01;
            let q = [mid[0] + n[0] / l * out, mid[1] + n[1] / l * out];
            if paved.covers(q, 0.0) {
                looking_at_road += 1;
            }
        }
        let total = w.kerbs.indices.len() / 2;
        assert!(
            looking_at_road * 100 >= total * 98,
            "{looking_at_road} of {total} kerb faces face the road"
        );
    }
}

#[cfg(test)]
mod timing {
    #[test]
    #[ignore = "timing report, run in release"]
    fn sidewalk_mesh_timing() {
        let map = crate::CityMap::new(crate::vesper::generate(
            &crate::vesper::VesperConfig::default(),
        ));
        let t = std::time::Instant::now();
        let r = super::road_mesh(&map);
        let t_road = t.elapsed();
        let t = std::time::Instant::now();
        let w = super::sidewalk_mesh(&map);
        println!(
            "road_mesh {:?} ({} tris), sidewalk_mesh {:?} ({} + {} tris)",
            t_road,
            r.indices.len(),
            t.elapsed(),
            w.tops.indices.len(),
            w.kerbs.indices.len()
        );
    }
}
