//! Lamps projected onto the body surface (car-shapes phase 3).
//!
//! A [`Lamp`] is a 2D outline in the coordinates of a view (the nose seen from ahead, the tail
//! from behind, a side, or the top). Every point of the outline is cast along the view
//! direction onto the body's panel / body triangles already built (the loft runs before style
//! details), so the lens sits proud of exactly the drawn surface and follows the curved nose,
//! fender or tail: a lens drawn in the front view near the corner wraps round the plan-rounded corner
//! onto the fender (more with [`Lamp::yaw_deg`]), one drawn in the top view lies on the hood
//! or fender top.
//!
//! - [`lens`]: the lens face [`Lamp::proud_m`] over the surface with a rim wall down to
//!   [`Lamp::depth_m`] under it (the back, inside the body, is left open). Use the head / tail lamp and turn-signal keys
//!   (`kit::headlight_key`, `mesh::indicator_front_key`, …) so the renderer's lamp slots and
//!   the turn-signal contract keep working.
//! - [`bezel`]: a trim ring of a given width round the outline, a little less proud.
//! - [`element`]: inner elements on the lens face: projector bowls, reflector rings, LED strips.
//! - [`split_outline`] / [`clip_outline`]: cut an outline in two along a view axis with the
//!   [`INDICATOR_SEAM_M`] gap, for the amber turn-signal segment of a lamp cluster.
//!
//! Outlines may be any simple polygon, convex or concave (teardrops, L, C, boomerang, trapezoid,
//! slim strips); they are ear-clipped in the view plane. Points whose ray misses the body (past
//! the silhouette) are pulled toward the centroid until they hit.
//!
//! ```ignore
//! // A swept L-shaped head lamp on the left of the nose that wraps onto the fender, with its
//! // amber outer end, a bezel and a projector bowl (inside a performance kit):
//! use car_assets::lamp::{element, Element, LampView};
//! let l = [[-0.35, 0.58], [-0.80, 0.60], [-0.80, 0.66], [-0.55, 0.66], [-0.52, 0.62], [-0.35, 0.62]];
//! kit.proj_cluster(-1.0, true, LampView::Front, &l, Some(Seg::Outboard(0.08)), M::EmissiveAmber, 0.012, 20.0);
//! ```
//! Chassis metres; deterministic.

use crate::builder::{Builder, PartKey};
use crate::mesh::{PartKind, INDICATOR_SEAM_M};
use crate::profile::BodyProfile;

/// The view an outline is drawn in, and the direction its points are cast along.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LampView {
    /// The nose seen from ahead: outline `[x, z]` (chassis x, signed; z above ground), cast
    /// rearward (−Y).
    Front,
    /// The tail seen from behind: outline `[x, z]` (chassis x, signed), cast forward (+Y).
    Rear,
    /// A side: outline `[y, z]` (chassis y, z), cast inward from the left (`right = false`,
    /// x < 0) or the right side.
    Side {
        /// The right (+X) side.
        right: bool,
    },
    /// The top seen from above: outline `[x, y]` (chassis x signed, y), cast downward.
    Top,
}

/// A projected lamp lens (see the module docs).
#[derive(Debug, Clone, PartialEq)]
pub struct Lamp {
    /// View and cast direction.
    pub view: LampView,
    /// Outline in the view's coordinates (any winding, ≥ 3 points, star-shaped round its
    /// centroid).
    pub outline: Vec<[f32; 2]>,
    /// Front / rear views only: swings the cast direction toward the outline's own side by
    /// this many degrees (0…60), so a lens drawn near the corner wraps further round onto the
    /// fender / quarter (wraparound tail lamps). The outline's x is then measured across the
    /// swung direction (still in metres, still signed). Default 0.
    pub yaw_deg: f32,
    /// Lens face over the surface, m (0.002…0.03). Default 0.008.
    pub proud_m: f32,
    /// How far the lens rim reaches under the surface, m (0…0.01; keep it small: rim faces seen
    /// edge-on on a corner then dip into the body or under a bezel). Default 0.
    pub depth_m: f32,
    /// Grid step of the lens, m (0.01…0.08): perimeter spacing (8…32 points), rings every two
    /// steps (1…3 rings). Default 0.04. A lens costs ≈ 2 × points × rings + 2 × points
    /// triangles (≈ 100–250), a bezel 4 × points.
    pub step_m: f32,
}

impl Lamp {
    /// A lamp in `view` with the default proud / depth / step and no yaw.
    pub fn new(view: LampView, outline: Vec<[f32; 2]>) -> Lamp {
        Lamp {
            view,
            outline,
            yaw_deg: 0.0,
            proud_m: 0.008,
            depth_m: 0.0,
            step_m: 0.04,
        }
    }
}

/// Outline points of an ellipse centred at `c` with half axes `r` (view coordinates), `n`
/// points (≥ 6), counter-clockwise.
pub fn ellipse(c: [f32; 2], r: [f32; 2], n: usize) -> Vec<[f32; 2]> {
    let n = n.max(6);
    (0..n)
        .map(|i| {
            let (s, co) = crate::builder::turn(i, n);
            [c[0] + r[0] * co, c[1] + r[1] * s]
        })
        .collect()
}

/// The part of `outline` with coordinate `axis` (0 = first, 1 = second) below `at`
/// (`keep_below`) or above it (Sutherland–Hodgman clip against one line).
pub fn clip_outline(outline: &[[f32; 2]], axis: usize, at: f32, keep_below: bool) -> Vec<[f32; 2]> {
    let axis = axis.min(1);
    let inside = |q: [f32; 2]| {
        if keep_below {
            q[axis] <= at
        } else {
            q[axis] >= at
        }
    };
    let mut out = Vec::new();
    let n = outline.len();
    for i in 0..n {
        let a = outline[i];
        let b = outline[(i + 1) % n];
        let (ia, ib) = (inside(a), inside(b));
        if ia {
            out.push(a);
        }
        if ia != ib {
            let t = (at - a[axis]) / (b[axis] - a[axis]);
            out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
        }
    }
    out
}

/// Split `outline` along `axis` at `at` into (below, above), each kept
/// [`INDICATOR_SEAM_M`] / 2 clear of the cut (the lens divider of a lamp cluster).
pub fn split_outline(outline: &[[f32; 2]], axis: usize, at: f32) -> (Vec<[f32; 2]>, Vec<[f32; 2]>) {
    let g = 0.5 * INDICATOR_SEAM_M;
    (
        clip_outline(outline, axis, at - g, true),
        clip_outline(outline, axis, at + g, false),
    )
}

/// The lens of `lamp` into part `key` (a face following the surface, with a rim wall).
pub fn lens(b: &mut Builder, p: &BodyProfile, key: &PartKey, lamp: &Lamp) {
    let before = b.vertex_count(&key.name);
    lens_inner(b, p, key, lamp);
    let after = b.vertex_count(&key.name);
    b.mark_placed(&key.name, before, after);
}

fn lens_inner(b: &mut Builder, p: &BodyProfile, key: &PartKey, lamp: &Lamp) {
    let Some(caster) = Caster::new(b, p, lamp) else {
        return;
    };
    let Some(mesh) = triangulate(&lamp.outline, lamp.step_m.clamp(0.01, 0.08)) else {
        return;
    };
    let proud = lamp.proud_m.clamp(0.002, 0.03);
    let depth = lamp.depth_m.clamp(0.0, 0.01);
    shell(b, &caster, key, &mesh, proud, -depth);
}

/// How far a [`bezel`] band stands off the surface, m (flush: it never shadows the lens rim).
pub const BEZEL_LIFT_M: f32 = 0.001;

/// A thin panel (grille, vent, intake surround, badge, applique …) of any part `key` draped on
/// the body like a lens: the outline (any simple polygon, in `view`'s coordinates) cast onto the
/// built body triangles, its face `face_m` over the surface (0.0005…0.10) and its edge wall down
/// to `back_m` (−0.05…face; above 0 the panel floats as a plate and gets a back face too).
/// `Kit::top_poly`, `Kit::side_poly` and `Kit::end_poly` wrap it.
pub fn panel(
    b: &mut Builder,
    p: &BodyProfile,
    key: &PartKey,
    view: LampView,
    outline: &[[f32; 2]],
    face_m: f32,
    back_m: f32,
) {
    let before = b.vertex_count(&key.name);
    panel_inner(b, p, key, view, outline, face_m, back_m);
    let after = b.vertex_count(&key.name);
    b.mark_placed(&key.name, before, after);
}

fn panel_inner(
    b: &mut Builder,
    p: &BodyProfile,
    key: &PartKey,
    view: LampView,
    outline: &[[f32; 2]],
    face_m: f32,
    back_m: f32,
) {
    let lamp = Lamp::new(view, outline.to_vec());
    let Some(caster) = Caster::new(b, p, &lamp) else {
        return;
    };
    let Some(mesh) = triangulate(outline, lamp.step_m) else {
        return;
    };
    let face = face_m.clamp(0.0005, 0.10);
    let back = back_m.clamp(-0.05, face - 0.0005);
    shell(b, &caster, key, &mesh, face, back);
    if back > 0.0 {
        let hits = caster.cast_mesh(&mesh);
        for t in &mesh.tris {
            let q = [0, 1, 2].map(|i| add(hits[t[i]].0, scale(hits[t[i]].1, back)));
            let n = norm(add(add(hits[t[0]].1, hits[t[1]].1), hits[t[2]].1));
            b.poly_facing(key, &q, scale(n, -1.0));
        }
    }
}

/// A trim bezel `width_m` wide (0.003…0.05) round `lamp`'s outline into part `key`: a band
/// [`BEZEL_LIFT_M`] over the surface from the lens edge outward, with a skirt into the body.
/// Draw the lens with [`lens`] (same `lamp`) on top of it.
pub fn bezel(b: &mut Builder, p: &BodyProfile, key: &PartKey, lamp: &Lamp, width_m: f32) {
    let before = b.vertex_count(&key.name);
    bezel_inner(b, p, key, lamp, width_m);
    let after = b.vertex_count(&key.name);
    b.mark_placed(&key.name, before, after);
}

fn bezel_inner(b: &mut Builder, p: &BodyProfile, key: &PartKey, lamp: &Lamp, width_m: f32) {
    let Some(caster) = Caster::new(b, p, lamp) else {
        return;
    };
    let w = width_m.clamp(0.003, 0.05);
    let depth = lamp.depth_m.clamp(0.0, 0.01);
    let ring_in = caster.boundary(lamp, 0.0);
    let ring_out = caster.boundary(lamp, w);
    let centre = caster.centre;
    let n = ring_in.len();
    for i in 0..n {
        let k = (i + 1) % n;
        let (a, c) = (ring_in[i], ring_in[k]);
        let (d, e) = (ring_out[i], ring_out[k]);
        let ai = add(a.0, scale(a.1, BEZEL_LIFT_M));
        let ci = add(c.0, scale(c.1, BEZEL_LIFT_M));
        let dout = add(d.0, scale(d.1, BEZEL_LIFT_M));
        let eout = add(e.0, scale(e.1, BEZEL_LIFT_M));
        let up = norm(add(a.1, d.1));
        b.poly_facing(key, &[ai, dout, eout, ci], up);
        // Skirt from the bezel's outer edge into the body.
        let dd = add(d.0, scale(d.1, -depth.max(0.004)));
        let ed = add(e.0, scale(e.1, -depth.max(0.004)));
        let mid = scale(add(add(dout, eout), add(dd, ed)), 0.25);
        b.poly_facing(key, &[dout, dd, ed, eout], sub(mid, centre));
    }
}

/// A flat window of part `glass` (a glass key) with a seal of part `seal` `seal_w` wide
/// (0.003…0.05) round it, for near-flat body faces such as a van's rear doors: the outline (any
/// simple polygon in `view`'s coordinates) is triangulated on a coarse grid
/// ([`FLAT_WINDOW_STEP_M`]) and cast onto the built body; the glass sits `proud_m` (0.001…0.02)
/// over the surface with a rim wall down to it, the seal is a band [`BEZEL_LIFT_M`] over the
/// surface (no skirt: meant for flat faces). No lens rings: about a third of a [`lens`] + [`bezel`]
/// pair's triangles.
#[allow(clippy::too_many_arguments)]
pub fn flat_window(
    b: &mut Builder,
    p: &BodyProfile,
    glass: &PartKey,
    seal: &PartKey,
    view: LampView,
    outline: &[[f32; 2]],
    proud_m: f32,
    seal_w: f32,
) {
    let (g0, s0) = (b.vertex_count(&glass.name), b.vertex_count(&seal.name));
    flat_window_inner(b, p, glass, seal, view, outline, proud_m, seal_w);
    let (g1, s1) = (b.vertex_count(&glass.name), b.vertex_count(&seal.name));
    b.mark_placed(&glass.name, g0, g1);
    b.mark_placed(&seal.name, s0, s1);
}

#[allow(clippy::too_many_arguments)]
fn flat_window_inner(
    b: &mut Builder,
    p: &BodyProfile,
    glass: &PartKey,
    seal: &PartKey,
    view: LampView,
    outline: &[[f32; 2]],
    proud_m: f32,
    seal_w: f32,
) {
    let lamp = Lamp::new(view, outline.to_vec());
    let Some(mut caster) = Caster::new(b, p, &lamp) else {
        return;
    };
    // A van's rear glass leans forward to the roof: cast further than a lamp on the end face.
    caster.max_d = caster.max_d.max(3.5);
    let Some(mesh) = triangulate(outline, FLAT_WINDOW_STEP_M) else {
        return;
    };
    // The resampled boundary, counter-clockwise in the view (edge normals (dy, −dx) point out).
    let q: Vec<[f32; 2]> = mesh.rim.iter().map(|&i| mesh.pts[i]).collect();
    let n = q.len();
    let w = seal_w.clamp(0.003, 0.05);
    let proud = proud_m.clamp(0.001, 0.02);
    // Outline pushed out by `w` (mitred edge normals).
    let edge_n = |i: usize| {
        let (a, c) = (q[i], q[(i + 1) % n]);
        let (dx, dy) = (c[0] - a[0], c[1] - a[1]);
        let l = (dx * dx + dy * dy).sqrt().max(1e-6);
        [dy / l, -dx / l]
    };
    let outer: Vec<Hit> = (0..n)
        .map(|i| {
            let (e0, e1) = (edge_n((i + n - 1) % n), edge_n(i));
            let m = [e0[0] + e1[0], e0[1] + e1[1]];
            let d = (m[0] * e1[0] + m[1] * e1[1]).max(0.3);
            let o = [q[i][0] + w * m[0] / d, q[i][1] + w * m[1] / d];
            caster.cast_or_pull(o, caster.c2)
        })
        .collect();
    let hits = caster.cast_mesh(&mesh);
    let c = caster.centre;
    let lift = |h: Hit, d: f32| add(h.0, scale(h.1, d));
    // Glass face over the surface.
    for t in &mesh.tris {
        let up = norm(add(add(hits[t[0]].1, hits[t[1]].1), hits[t[2]].1));
        let tri = [0, 1, 2].map(|i| lift(hits[t[i]], proud));
        b.poly_facing(glass, &tri, up);
    }
    for i in 0..n {
        let k = (i + 1) % n;
        let (a, e) = (hits[mesh.rim[i]], hits[mesh.rim[k]]);
        let up = norm(add(a.1, e.1));
        // Glass rim wall down to the surface.
        let mid = scale(add(a.0, e.0), 0.5);
        b.poly_facing(
            glass,
            &[lift(a, proud), a.0, e.0, lift(e, proud)],
            sub(mid, c),
        );
        // Seal band (flat faces: no skirt).
        let (d, f) = (outer[i], outer[k]);
        b.poly_facing(
            seal,
            &[
                lift(a, BEZEL_LIFT_M),
                lift(d, BEZEL_LIFT_M),
                lift(f, BEZEL_LIFT_M),
                lift(e, BEZEL_LIFT_M),
            ],
            up,
        );
    }
}

/// Grid step of a [`flat_window`], m (coarse: a near-flat window needs few points).
const FLAT_WINDOW_STEP_M: f32 = 0.25;

/// An inner element of a lamp, placed on its lens face (see [`element`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Element {
    /// Projector bowl: a dome of radius `r_m` standing `h_m` out of the lens face.
    Bowl {
        /// Radius, m.
        r_m: f32,
        /// Height over the lens face, m.
        h_m: f32,
    },
    /// Reflector / bezel ring of radius `r_m` and tube width `w_m` lying on the lens face.
    Ring {
        /// Ring radius, m.
        r_m: f32,
        /// Tube width, m.
        w_m: f32,
    },
    /// LED strip along a polyline in the lamp's view coordinates, `w_m` wide, 3 mm over the
    /// lens face (it follows the surface like the lens).
    Strip {
        /// Polyline points, view coordinates.
        pts: Vec<[f32; 2]>,
        /// Width, m.
        w_m: f32,
    },
}

/// An inner element of `lamp` at view point `at` (ignored by `Strip`, which carries its own
/// points) into part `key`: projector bowls, reflector rings and LED strips sit on the lens
/// face (they follow the same cast as the lens). Use a light key (the lamp's own part, or a DRL
/// part) for emissive elements, a trim / chrome detail key for rings.
///
/// ```ignore
/// use car_assets::lamp::{element, ellipse, lens, Element, Lamp, LampView};
/// let lamp = Lamp::new(LampView::Front, ellipse([-0.55, 0.62], [0.16, 0.05], 16));
/// lens(b, p, &headlight_key(-1.0), &lamp);
/// element(b, p, &headlight_key(-1.0), &lamp, [-0.62, 0.62], &Element::Bowl { r_m: 0.03, h_m: 0.01 });
/// element(b, p, &chrome_ring_key, &lamp, [-0.62, 0.62], &Element::Ring { r_m: 0.036, w_m: 0.006 });
/// element(b, p, &drl_key, &lamp, [0.0, 0.0], &Element::Strip {
///     pts: vec![[-0.42, 0.59], [-0.66, 0.60], [-0.70, 0.64]], w_m: 0.008 });
/// ```
pub fn element(
    b: &mut Builder,
    p: &BodyProfile,
    key: &PartKey,
    lamp: &Lamp,
    at: [f32; 2],
    el: &Element,
) {
    let before = b.vertex_count(&key.name);
    element_inner(b, p, key, lamp, at, el);
    let after = b.vertex_count(&key.name);
    b.mark_placed(&key.name, before, after);
}

fn element_inner(
    b: &mut Builder,
    p: &BodyProfile,
    key: &PartKey,
    lamp: &Lamp,
    at: [f32; 2],
    el: &Element,
) {
    let face = lamp.proud_m.clamp(0.002, 0.03);
    match el {
        Element::Strip { pts, w_m } => {
            let w = 0.5 * w_m.clamp(0.002, 0.05);
            if pts.len() < 2 {
                return;
            }
            // Thicken the polyline into an outline (left side out, right side back).
            let n = pts.len();
            let side = |i: usize| -> [f32; 2] {
                let (a, c) = (pts[i.saturating_sub(1)], pts[(i + 1).min(n - 1)]);
                let d = [c[0] - a[0], c[1] - a[1]];
                let l = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-6);
                [-d[1] / l * w, d[0] / l * w]
            };
            let mut outline: Vec<[f32; 2]> = (0..n)
                .map(|i| [pts[i][0] + side(i)[0], pts[i][1] + side(i)[1]])
                .collect();
            outline.extend(
                (0..n)
                    .rev()
                    .map(|i| [pts[i][0] - side(i)[0], pts[i][1] - side(i)[1]]),
            );
            let strip = Lamp {
                outline,
                proud_m: face + 0.003,
                depth_m: 0.0,
                step_m: lamp.step_m,
                ..lamp.clone()
            };
            lens(b, p, key, &strip);
        }
        Element::Bowl { r_m, h_m } => {
            let Some(caster) = Caster::new(b, p, lamp) else {
                return;
            };
            let (h, n) = caster.cast_or_pull(at, caster.c2);
            let (r, hh) = (r_m.clamp(0.005, 0.15), h_m.clamp(0.001, 0.06));
            let base = add(h, scale(n, face - 0.001));
            let prof = [[r, 0.0], [r, 0.3 * hh], [0.7 * r, 0.8 * hh], [0.0, hh]];
            b.lathe(key, base, n, &prof, 12);
        }
        Element::Ring { r_m, w_m } => {
            let Some(caster) = Caster::new(b, p, lamp) else {
                return;
            };
            let (h, n) = caster.cast_or_pull(at, caster.c2);
            let w = 0.5 * w_m.clamp(0.002, 0.03);
            b.torus(
                key,
                add(h, scale(n, face)),
                n,
                r_m.clamp(0.005, 0.2),
                w,
                14,
                4,
            );
        }
    }
}

/// Kit parts on a raked / undercut end face (`nose_face_rake_deg`, `nose_chin_m`, tail likewise):
/// every detail / light vertex from 8 cm behind to 30 cm ahead of the old vertical end plane
/// moves along y by the face's set-back at its own x / z (the body triangles cast from ahead /
/// behind), so boxes, grilles, generic lamps, push bars, splitters and badges that kits place on
/// the old face sit on the raked one. Vertices already placed on the surface (projected lamps and panels, surface patches; see
/// `Builder::mark_placed`) are left alone. No-op when the end has no face profile. Run once,
/// after the style details (`mesh::generate_lod` does).
pub fn follow_end_faces(b: &mut Builder, p: &BodyProfile) {
    for front in [true, false] {
        let depth = p.face_depth(front);
        if depth <= 1e-4 {
            continue;
        }
        let plane = if front { p.y_nose } else { p.y_tail };
        // On the face: 8 cm behind the old plane to 30 cm ahead of it (push bars, splitters and
        // bumpers bolted on in front follow too).
        let (behind, ahead) = (0.08, 0.30);
        // Candidate vertices: (part, vertex, position).
        let mut cand: Vec<(String, usize, [f32; 3])> = Vec::new();
        for key in b.keys() {
            if key.wheel.is_some() || !matches!(key.kind, PartKind::Detail | PartKind::Light) {
                continue;
            }
            let Some((pos, _, _)) = b.geometry(&key.name) else {
                continue;
            };
            let pos = pos.to_vec();
            for (v, q) in pos.iter().enumerate() {
                let out = if front { q[1] - plane } else { plane - q[1] };
                if out >= -behind && out <= ahead && !b.is_placed(&key.name, v) {
                    cand.push((key.name.clone(), v, *q));
                }
            }
        }
        if cand.is_empty() {
            continue;
        }
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for (_, _, q) in &cand {
            lo = [lo[0].min(q[0]), lo[1].min(q[2])];
            hi = [hi[0].max(q[0]), hi[1].max(q[2])];
        }
        let view = if front {
            LampView::Front
        } else {
            LampView::Rear
        };
        let bbox = vec![
            [lo[0], lo[1]],
            [hi[0], lo[1]],
            [hi[0], hi[1]],
            [lo[0], hi[1]],
        ];
        let Some(caster) = Caster::new(b, p, &Lamp::new(view, bbox)) else {
            continue;
        };
        // Face span for misses (above the face top, past the silhouette): retry inside it.
        let zb = p.sill(plane) + 0.01;
        let zt = (if front { p.z_nose } else { p.belt(plane) } - 0.01).max(zb);
        let half = p.half_width(plane) - 0.02;
        let mut memo: std::collections::BTreeMap<(i32, i32), f32> =
            std::collections::BTreeMap::new();
        for (name, v, q) in cand {
            let key = ((q[0] * 500.0).round() as i32, (q[2] * 500.0).round() as i32);
            let shift = *memo.entry(key).or_insert_with(|| {
                // View coordinates of chassis (x, z) for an un-yawed front / rear cast.
                let at = |x: f32, z: f32| {
                    let d = sub([x, plane, z], caster.origin);
                    caster
                        .cast_tri([dot(d, caster.u), dot(d, caster.v)])
                        .map(|h| h.0)
                };
                let hit =
                    at(q[0], q[2]).or_else(|| at(q[0].clamp(-half, half), q[2].clamp(zb, zt)));
                hit.map_or(0.0, |h| {
                    let s = h[1] - plane;
                    if front {
                        s.clamp(-(depth + 0.03), 0.0)
                    } else {
                        s.clamp(0.0, depth + 0.03)
                    }
                })
            });
            if shift != 0.0 {
                b.set_vertex(&name, v, [q[0], q[1] + shift, q[2]]);
            }
        }
    }
}

/// One projected grid point: (surface point, outward normal).
type Hit = ([f32; 3], [f32; 3]);

/// A body triangle seen in the view: vertex (u, v) and distance along the cast direction.
type ViewTri = [[f32; 3]; 3];

/// Casts view points onto the body: the loft's panel / body triangles already in the builder
/// (the mesh itself, so a lens is proud of exactly what is drawn).
struct Caster {
    tris: Vec<ViewTri>,
    /// Cast direction (unit).
    dir: [f32; 3],
    /// View axes: first and second outline coordinate.
    u: [f32; 3],
    v: [f32; 3],
    /// Start of a cast for outline point (0, 0): outside the body.
    origin: [f32; 3],
    /// 2D centroid of the outline.
    c2: [f32; 2],
    /// Surface point of the centroid.
    centre: [f32; 3],
    /// Front / rear views: the furthest a cast may land from its start, m (the face zone: a
    /// point over a lowered tailgate is a miss, not a hit on the cab wall behind the bed).
    max_d: f32,
    /// Front / rear views: view height of the face's middle (a second pull target when an
    /// outline point and its centroid both miss, e.g. above a raked tailgate) and the face's
    /// half width in the view.
    face_mid: Option<(f32, f32)>,
}

impl Caster {
    fn new(b: &Builder, p: &BodyProfile, lamp: &Lamp) -> Option<Caster> {
        if lamp.outline.len() < 3 {
            return None;
        }
        let c2 = centroid(&lamp.outline);
        let yaw = lamp.yaw_deg.clamp(0.0, 60.0).to_radians();
        let (sy, cy) = wp_math::sin_cos(yaw);
        let side = if c2[0] < 0.0 { -1.0 } else { 1.0 };
        let (dir, u, v, origin) = match lamp.view {
            LampView::Front => {
                // From ahead (and, with yaw, from the outline's side), travelling in; the cast
                // plane passes through the nose face.
                let dir = [-side * sy, -cy, 0.0];
                let u = horizontal_across(dir, [1.0, 0.0, 0.0]);
                (
                    dir,
                    u,
                    [0.0, 0.0, 1.0],
                    sub([0.0, p.y_nose, 0.0], scale(dir, 1.5)),
                )
            }
            LampView::Rear => {
                let dir = [-side * sy, cy, 0.0];
                let u = horizontal_across(dir, [1.0, 0.0, 0.0]);
                (
                    dir,
                    u,
                    [0.0, 0.0, 1.0],
                    sub([0.0, p.y_tail, 0.0], scale(dir, 1.5)),
                )
            }
            LampView::Side { right } => {
                let sx = if right { 1.0 } else { -1.0 };
                (
                    [-sx, 0.0, 0.0],
                    [0.0, 1.0, 0.0],
                    [0.0, 0.0, 1.0],
                    [sx * (p.half_w + 1.0), 0.0, 0.0],
                )
            }
            LampView::Top => (
                [0.0, 0.0, -1.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, p.z_max() + 1.0],
            ),
        };
        // Body triangles whose view footprint overlaps the outline (+ bezel and normal margin).
        let face_mid = match lamp.view {
            LampView::Front | LampView::Rear => {
                let front = lamp.view == LampView::Front;
                let y = if front { p.y_nose } else { p.y_tail };
                let top = if front { p.z_nose } else { p.belt(y) };
                let zm = 0.5 * (p.sill(y) + top);
                Some((dot(sub([0.0, y, zm], origin), v), p.half_width(y) - 0.05))
            }
            _ => None,
        };
        let m = 0.08;
        let mut lo = [
            lamp.outline.iter().map(|q| q[0]).fold(f32::MAX, f32::min) - m,
            lamp.outline.iter().map(|q| q[1]).fold(f32::MAX, f32::min) - m,
        ];
        let mut hi = [
            lamp.outline.iter().map(|q| q[0]).fold(f32::MIN, f32::max) + m,
            lamp.outline.iter().map(|q| q[1]).fold(f32::MIN, f32::max) + m,
        ];
        // Keep the triangles the face-middle fallback pull may reach.
        if let Some((vm, half)) = face_mid {
            lo[1] = lo[1].min(vm - m);
            hi[1] = hi[1].max(vm + m);
            lo[0] = lo[0].max(-half - m).min(hi[0]);
            hi[0] = hi[0].min(half + m).max(lo[0]);
        }
        let mut tris = Vec::new();
        for key in b.keys() {
            if key.wheel.is_some() || !matches!(key.kind, PartKind::Panel | PartKind::Body) {
                continue;
            }
            let Some((pos, _, idx)) = b.geometry(&key.name) else {
                continue;
            };
            for t in idx.chunks_exact(3) {
                let tri: ViewTri = [0, 1, 2].map(|i| {
                    let d = sub(pos[t[i] as usize], origin);
                    [dot(d, u), dot(d, v), dot(d, dir)]
                });
                let out = (0..2)
                    .any(|k| tri.iter().all(|w| w[k] < lo[k]) || tri.iter().all(|w| w[k] > hi[k]));
                if !out {
                    tris.push(tri);
                }
            }
        }
        if tris.is_empty() {
            return None;
        }
        let max_d = match lamp.view {
            LampView::Front | LampView::Rear => {
                let depth = p.face_depth(lamp.view == LampView::Front);
                1.5 + 1.2 * sy + depth + 0.35
            }
            _ => f32::MAX,
        };
        let mut caster = Caster {
            tris,
            dir,
            u,
            v,
            origin,
            c2,
            centre: [0.0; 3],
            max_d,
            face_mid,
        };
        caster.centre = caster.cast_or_pull(c2, c2).0;
        Some(caster)
    }

    /// Ray start for view point `q`.
    fn start(&self, q: [f32; 2]) -> [f32; 3] {
        add(self.origin, add(scale(self.u, q[0]), scale(self.v, q[1])))
    }

    /// First body surface point along the cast of view point `q` and the outward normal of
    /// the triangle hit (turned to the viewer), or None.
    fn cast_tri(&self, q: [f32; 2]) -> Option<Hit> {
        let mut best: Option<(f32, usize)> = None;
        for (i, t) in self.tris.iter().enumerate() {
            let (a, b, c) = (t[0], t[1], t[2]);
            let det = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
            if det.abs() < 1e-12 {
                continue;
            }
            let l1 = ((q[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (q[1] - a[1])) / det;
            let l2 = ((b[0] - a[0]) * (q[1] - a[1]) - (q[0] - a[0]) * (b[1] - a[1])) / det;
            let e = -1e-5;
            if l1 < e || l2 < e || l1 + l2 > 1.0 - e {
                continue;
            }
            let d = a[2] + (b[2] - a[2]) * l1 + (c[2] - a[2]) * l2;
            if d > 0.0 && d <= self.max_d && best.is_none_or(|x| d < x.0) {
                best = Some((d, i));
            }
        }
        let (d, i) = best?;
        let t = self.tris[i];
        // Triangle normal from view coordinates back to chassis space, turned to the viewer.
        let w = |k: usize| {
            add(
                add(scale(self.u, t[k][0]), scale(self.v, t[k][1])),
                scale(self.dir, t[k][2]),
            )
        };
        let n = norm(cross(sub(w(1), w(0)), sub(w(2), w(0))));
        let n = if dot(n, self.dir) > 0.0 {
            scale(n, -1.0)
        } else {
            n
        };
        Some((add(self.start(q), scale(self.dir, d)), n))
    }

    /// Surface point and normal of view point `q`, pulled toward `toward` (a point known to
    /// hit) until the cast hits.
    fn cast_or_pull(&self, q: [f32; 2], toward: [f32; 2]) -> Hit {
        let pull = |to: [f32; 2]| {
            let mut f = 1.0f32;
            let mut hit = self.cast_tri(q);
            while hit.is_none() && f > 0.02 {
                f -= 0.08;
                hit = self.cast_tri([to[0] + (q[0] - to[0]) * f, to[1] + (q[1] - to[1]) * f]);
            }
            hit
        };
        let mut hit = pull(toward);
        if hit.is_none() {
            if let Some((vm, half)) = self.face_mid {
                hit = pull([q[0].clamp(-half, half), vm]);
            }
        }
        hit.unwrap_or_else(|| (self.start(q), scale(self.dir, -1.0)))
    }

    /// Cast every vertex of a 2D lens mesh.
    fn cast_mesh(&self, m: &LensMesh) -> Vec<Hit> {
        m.pts
            .iter()
            .map(|&q| self.cast_or_pull(q, self.c2))
            .collect()
    }

    /// The outline (resampled at the lens step) offset outward by `off` m in the view, cast.
    fn boundary(&self, lamp: &Lamp, off: f32) -> Vec<Hit> {
        resample(&lamp.outline, lamp.step_m.clamp(0.01, 0.08))
            .iter()
            .map(|q| {
                let d = [q[0] - self.c2[0], q[1] - self.c2[1]];
                let l = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-6);
                let q2 = [q[0] + d[0] / l * off, q[1] + d[1] / l * off];
                self.cast_or_pull(q2, self.c2)
            })
            .collect()
    }
}

/// A lens outline triangulated in the view plane: points, triangles (counter-clockwise in the
/// view) and the boundary loop (point indices in outline order).
struct LensMesh {
    pts: Vec<[f32; 2]>,
    tris: Vec<[usize; 3]>,
    rim: Vec<usize>,
}

/// Triangulate `outline` (any simple polygon, convex or concave): resample the boundary at
/// `step`, ear-clip it, then split every triangle into `n × n` (n = 1…2, from the longest edge
/// over 2 × `step`; the same `n` everywhere, so shared edges split alike and the lens has no
/// cracks).
fn triangulate(outline: &[[f32; 2]], step: f32) -> Option<LensMesh> {
    let mut ring = resample(outline, step);
    if ring.len() < 3 {
        return None;
    }
    if signed_area(&ring) < 0.0 {
        ring.reverse();
    }
    let ears = ear_clip(&ring);
    if ears.is_empty() {
        return None;
    }
    let len = |a: usize, b: usize| {
        let (p, q) = (ring[a], ring[b]);
        ((q[0] - p[0]).powi(2) + (q[1] - p[1]).powi(2)).sqrt()
    };
    let longest = ears
        .iter()
        .map(|t| len(t[0], t[1]).max(len(t[1], t[2])).max(len(t[2], t[0])))
        .fold(0.0f32, f32::max);
    let n = ((longest / (2.0 * step)).ceil() as usize).clamp(1, 2);
    // Points: the ring first, then edge and interior points keyed canonically.
    let mut pts = ring.clone();
    let mut keyed: std::collections::BTreeMap<(usize, usize, usize, usize), usize> =
        std::collections::BTreeMap::new();
    let lerp2 =
        |a: [f32; 2], b: [f32; 2], t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    let mut point = |pts: &mut Vec<[f32; 2]>, t: [usize; 3], w: [usize; 3]| -> usize {
        // Barycentric (w0, w1, w2) / n over triangle t.
        let nz: Vec<usize> = (0..3).filter(|&k| w[k] > 0).collect();
        if nz.len() == 1 {
            return t[nz[0]];
        }
        if nz.len() == 2 {
            // On an edge: key by (low index, high index, steps from the low end).
            let (i, j) = (nz[0], nz[1]);
            let (lo, hi, wl) = if t[i] < t[j] {
                (t[i], t[j], w[i])
            } else {
                (t[j], t[i], w[j])
            };
            let key = (lo, hi, wl, usize::MAX);
            if let Some(&v) = keyed.get(&key) {
                return v;
            }
            let q = lerp2(ring[lo], ring[hi], (n - wl) as f32 / n as f32);
            pts.push(q);
            keyed.insert(key, pts.len() - 1);
            return pts.len() - 1;
        }
        let key = (t[0], t[1], t[2], w[0] * (n + 1) + w[1]);
        if let Some(&v) = keyed.get(&key) {
            return v;
        }
        let f = 1.0 / n as f32;
        let q = [
            (ring[t[0]][0] * w[0] as f32
                + ring[t[1]][0] * w[1] as f32
                + ring[t[2]][0] * w[2] as f32)
                * f,
            (ring[t[0]][1] * w[0] as f32
                + ring[t[1]][1] * w[1] as f32
                + ring[t[2]][1] * w[2] as f32)
                * f,
        ];
        pts.push(q);
        keyed.insert(key, pts.len() - 1);
        pts.len() - 1
    };
    let mut tris = Vec::new();
    for t in &ears {
        // Sub-grid rows: w0 from n down to 0.
        let mut id =
            |pts: &mut Vec<[f32; 2]>, a: usize, b: usize| point(pts, *t, [a, b, n - a - b]);
        for a in 0..n {
            for b in 0..n - a {
                let p0 = id(&mut pts, a + 1, b);
                let p1 = id(&mut pts, a, b + 1);
                let p2 = id(&mut pts, a, b);
                tris.push([p0, p1, p2]);
                if a + b + 1 < n {
                    let p3 = id(&mut pts, a + 1, b + 1);
                    tris.push([p0, p3, p1]);
                }
            }
        }
    }
    // Boundary loop, including the points that split each boundary edge.
    let mut rim = Vec::new();
    let m = ring.len();
    for i in 0..m {
        let j = (i + 1) % m;
        rim.push(i);
        for s in 1..n {
            let (lo, hi) = (i.min(j), i.max(j));
            let wl = if i < j { n - s } else { s };
            if let Some(&v) = keyed.get(&(lo, hi, wl, usize::MAX)) {
                rim.push(v);
            }
        }
    }
    Some(LensMesh { pts, tris, rim })
}

/// Signed area of a polygon (+ = counter-clockwise).
fn signed_area(q: &[[f32; 2]]) -> f32 {
    let n = q.len();
    0.5 * (0..n)
        .map(|i| {
            let (a, b) = (q[i], q[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f32>()
}

/// Ear clipping of a counter-clockwise simple polygon into index triangles (CCW).
fn ear_clip(q: &[[f32; 2]]) -> Vec<[usize; 3]> {
    let cross2 = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| {
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
    };
    let inside = |p: [f32; 2], a: [f32; 2], b: [f32; 2], c: [f32; 2]| {
        cross2(a, b, p) >= 0.0 && cross2(b, c, p) >= 0.0 && cross2(c, a, p) >= 0.0
    };
    let mut idx: Vec<usize> = (0..q.len()).collect();
    let mut out = Vec::new();
    let mut guard = 0;
    while idx.len() > 3 && guard < 4 * q.len() * q.len() {
        guard += 1;
        let n = idx.len();
        let mut clipped = false;
        for i in 0..n {
            let (ia, ib, ic) = (idx[(i + n - 1) % n], idx[i], idx[(i + 1) % n]);
            let (a, b, c) = (q[ia], q[ib], q[ic]);
            if cross2(a, b, c) <= 1e-12 {
                continue;
            }
            let blocked = idx
                .iter()
                .any(|&k| k != ia && k != ib && k != ic && inside(q[k], a, b, c));
            if !blocked {
                out.push([ia, ib, ic]);
                idx.remove(i);
                clipped = true;
                break;
            }
        }
        if !clipped {
            // Degenerate rest (collinear points): fan it.
            for i in 1..idx.len() - 1 {
                out.push([idx[0], idx[i], idx[i + 1]]);
            }
            return out;
        }
    }
    if idx.len() == 3 {
        out.push([idx[0], idx[1], idx[2]]);
    }
    out
}

/// Lens over a triangulated outline: the face at `hi` along the normals and a rim wall down to
/// `lo` (under the surface; the back is inside the body and left open). Faces whose middle would
/// sit less than half of `hi` clear of the body (a chord across a rounded corner) are pushed
/// toward the viewer until they clear it.
fn shell(b: &mut Builder, caster: &Caster, key: &PartKey, m: &LensMesh, hi: f32, lo: f32) {
    let hits = caster.cast_mesh(m);
    let mut top: Vec<[f32; 3]> = hits.iter().map(|h| add(h.0, scale(h.1, hi))).collect();
    let depth = |q: [f32; 3]| dot(sub(q, caster.origin), caster.dir);
    for _ in 0..3 {
        let mut push = vec![0.0f32; top.len()];
        for t in &m.tris {
            let c = scale(add(add(top[t[0]], top[t[1]]), top[t[2]]), 1.0 / 3.0);
            let d = sub(c, caster.origin);
            let Some((s, _)) = caster.cast_tri([dot(d, caster.u), dot(d, caster.v)]) else {
                continue;
            };
            let need = 0.5 * hi - (depth(s) - depth(c));
            if need > 1e-4 {
                for &v in t {
                    push[v] = push[v].max(need);
                }
            }
        }
        if push.iter().all(|&x| x <= 0.0) {
            break;
        }
        for (q, &d) in top.iter_mut().zip(&push) {
            *q = sub(*q, scale(caster.dir, d));
        }
    }
    for t in &m.tris {
        let nrm = norm(add(add(hits[t[0]].1, hits[t[1]].1), hits[t[2]].1));
        b.poly_facing(key, &[top[t[0]], top[t[1]], top[t[2]]], nrm);
    }
    // Rim wall round the boundary, from the (pushed) face down under the surface; outward is
    // the boundary edge's outward normal in the view (the loop is counter-clockwise).
    let n = m.rim.len();
    for i in 0..n {
        let (s0, s1) = (m.rim[i], m.rim[(i + 1) % n]);
        let (h0, h1) = (&hits[s0], &hits[s1]);
        let q = [
            add(h0.0, scale(h0.1, lo)),
            add(h1.0, scale(h1.1, lo)),
            top[s1],
            top[s0],
        ];
        let (a, c) = (m.pts[s0], m.pts[s1]);
        let out2 = [c[1] - a[1], -(c[0] - a[0])];
        let out = add(scale(caster.u, out2[0]), scale(caster.v, out2[1]));
        b.poly_facing(key, &q, out);
    }
}

/// Unit horizontal vector across `dir` (perpendicular to it and to Z), on the side of `hint`.
fn horizontal_across(dir: [f32; 3], hint: [f32; 3]) -> [f32; 3] {
    let a = norm([-dir[1], dir[0], 0.0]);
    if dot(a, hint) < 0.0 {
        scale(a, -1.0)
    } else {
        a
    }
}

/// Area centroid of a polygon (vertex mean if degenerate).
fn centroid(q: &[[f32; 2]]) -> [f32; 2] {
    let n = q.len();
    let (mut a, mut cx, mut cy) = (0.0f32, 0.0f32, 0.0f32);
    for i in 0..n {
        let (p0, p1) = (q[i], q[(i + 1) % n]);
        let c = p0[0] * p1[1] - p1[0] * p0[1];
        a += c;
        cx += (p0[0] + p1[0]) * c;
        cy += (p0[1] + p1[1]) * c;
    }
    if a.abs() < 1e-9 {
        let m = n.max(1) as f32;
        return [
            q.iter().map(|p| p[0]).sum::<f32>() / m,
            q.iter().map(|p| p[1]).sum::<f32>() / m,
        ];
    }
    [cx / (3.0 * a), cy / (3.0 * a)]
}

/// The closed outline resampled to points at most `step` apart (every input corner kept),
/// 8…32 points.
fn resample(q: &[[f32; 2]], step: f32) -> Vec<[f32; 2]> {
    let n = q.len();
    let per: f32 = (0..n)
        .map(|i| {
            let (a, c) = (q[i], q[(i + 1) % n]);
            ((c[0] - a[0]).powi(2) + (c[1] - a[1]).powi(2)).sqrt()
        })
        .sum();
    let target = (per / step).ceil().clamp(8.0, 32.0);
    let step = per / target;
    let mut out = Vec::new();
    for i in 0..n {
        let (a, c) = (q[i], q[(i + 1) % n]);
        let l = ((c[0] - a[0]).powi(2) + (c[1] - a[1]).powi(2)).sqrt();
        let k = ((l / step).round() as usize).max(1);
        for j in 0..k {
            let t = j as f32 / k as f32;
            out.push([a[0] + (c[0] - a[0]) * t, a[1] + (c[1] - a[1]) * t]);
        }
    }
    out
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt();
    if l < 1e-9 {
        [0.0, 0.0, 1.0]
    } else {
        scale(a, 1.0 / l)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_keeps_both_sides_apart_by_the_seam() {
        let sq = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let (lo, hi) = split_outline(&sq, 0, 0.3);
        let max_lo = lo.iter().map(|q| q[0]).fold(f32::MIN, f32::max);
        let min_hi = hi.iter().map(|q| q[0]).fold(f32::MAX, f32::min);
        assert!((max_lo - (0.3 - 0.5 * INDICATOR_SEAM_M)).abs() < 1e-6);
        assert!((min_hi - (0.3 + 0.5 * INDICATOR_SEAM_M)).abs() < 1e-6);
        assert_eq!(lo.len(), 4);
        assert_eq!(hi.len(), 4);
        // A cut outside the outline keeps it whole on one side.
        assert_eq!(clip_outline(&sq, 1, 2.0, true).len(), 4);
        assert!(clip_outline(&sq, 1, 2.0, false).is_empty());
    }

    #[test]
    fn concave_outlines_triangulate_without_losing_area() {
        // An L, a C and a boomerang.
        let shapes: Vec<Vec<[f32; 2]>> = vec![
            vec![
                [0.0, 0.0],
                [0.4, 0.0],
                [0.4, 0.06],
                [0.08, 0.06],
                [0.08, 0.2],
                [0.0, 0.2],
            ],
            vec![
                [0.0, 0.0],
                [0.3, 0.0],
                [0.3, 0.05],
                [0.06, 0.05],
                [0.06, 0.15],
                [0.3, 0.15],
                [0.3, 0.2],
                [0.0, 0.2],
            ],
            vec![[0.0, 0.0], [0.2, 0.08], [0.4, 0.0], [0.2, 0.14]],
        ];
        for (k, q) in shapes.iter().enumerate() {
            let m = triangulate(q, 0.04).unwrap();
            let area: f32 = m
                .tris
                .iter()
                .map(|t| {
                    let (a, b, c) = (m.pts[t[0]], m.pts[t[1]], m.pts[t[2]]);
                    0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs()
                })
                .sum();
            let want = signed_area(q).abs();
            assert!(
                (area - want).abs() < 1e-3 * want.max(1e-3) + 1e-5,
                "shape {k}: {area} vs {want}"
            );
            assert!(m.rim.len() >= q.len(), "shape {k}");
        }
    }

    #[test]
    fn resample_and_centroid() {
        let e = ellipse([0.5, -0.2], [0.2, 0.1], 12);
        let c = centroid(&e);
        assert!((c[0] - 0.5).abs() < 1e-4 && (c[1] + 0.2).abs() < 1e-4);
        let r = resample(&e, 0.01);
        assert!((8..=32).contains(&r.len()), "{}", r.len());
        let r = resample(&[[0.0, 0.0], [0.01, 0.0], [0.0, 0.01]], 0.04);
        assert!(r.len() >= 3 && r.len() <= 8, "{}", r.len());
    }

    #[test]
    fn a_lens_sits_proud_of_the_body_it_is_cast_on() {
        let root = crate::repo_root();
        let car = crate::load_car(&root, "kestrel_ts").unwrap();
        let st = crate::style::style_for(&car.def);
        let p = BodyProfile::styled(&car.def, &car.lattice, &st.body);
        let mut b = Builder::new();
        // A box "body" standing in for the loft: its front face at y_nose.
        let body = PartKey::body(
            "hood",
            wp_contracts::BodyRegion::Hood,
            PartKind::Panel,
            crate::mesh::MaterialSlot::Livery,
        );
        b.cuboid(&body, [-0.8, p.y_nose - 1.0, 0.2], [0.8, p.y_nose, 0.8]);
        let key = PartKey::body(
            "headlight_l",
            wp_contracts::BodyRegion::FrameFront,
            PartKind::Light,
            crate::mesh::MaterialSlot::Headlight,
        );
        let lamp = Lamp::new(LampView::Front, ellipse([-0.5, 0.6], [0.12, 0.05], 12));
        lens(&mut b, &p, &key, &lamp);
        let (pos, _, idx) = b.geometry("headlight_l").unwrap();
        assert!(idx.len() >= 3 * 12);
        let ymax = pos.iter().map(|q| q[1]).fold(f32::MIN, f32::max);
        let ymin = pos.iter().map(|q| q[1]).fold(f32::MAX, f32::min);
        assert!((ymax - (p.y_nose + 0.008)).abs() < 1e-4, "{ymax}");
        assert!(ymin >= p.y_nose - 1e-4, "{ymin}");
        let xmin = pos.iter().map(|q| q[0]).fold(f32::MAX, f32::min);
        assert!((xmin + 0.62).abs() < 0.01, "{xmin}");
    }
}
