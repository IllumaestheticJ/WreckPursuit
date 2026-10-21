//! Port Vesper v3 — airport and airfield: Vesper International (runways, taxiways, apron,
//! terminal, control tower, hangars, parked aircraft, perimeter fence) and Northfield
//! Airfield.
//!
//! **Owned by the v3 airport workstream.** Contract with the pipeline (`metro::generate`):
//! - [`earthworks`] runs before roads exist (ground levelling beyond the plan's site pads);
//! - [`generate`] runs after roads, in pipeline order, and adds features / buildings /
//!   POIs to the [`Ctx`]. Randomness comes from `ctx.rng(SALT)`; everything stays inside the
//!   plan sites `airport`, `airport_landside` and `airfield`.
//!
//! Layout ([`international`], [`northfield`]):
//! - Runways, taxiways and aprons are solid, flat slabs 5 cm above the (padded) ground, so
//!   they are drivable; paint (`AirfieldMarking`) and lights are thin non-solid boxes.
//! - The airside is closed by a security fence along the site boundary (moved inward where
//!   a road runs along the edge), open only at the plan's gates, each with a barrier arm and
//!   a guard booth. Buildings on the boundary (the terminal, Northfield Hangar 3) close the
//!   fence line themselves.
//! - Nothing solid is placed on road paving or verges: every solid group is checked against
//!   the occupancy grid and dropped whole if it touches one.

mod international;
mod northfield;
#[cfg(test)]
mod tests;

use super::ctx::{box_on, cylinder, lerp2, wall_along, yaw_of, Ctx, Occ};
use super::ground::{inside, signed_dist, Earthwork, Natural};
use super::plan::{Gate, Plan, Site};
use crate::geom::{self, V2, V3};
use crate::network::{Feature, FeatureKind, FeatureShape, Material};

/// RNG salt of this module.
pub const SALT: u64 = 0xA1;

/// Height of a runway / taxiway / apron deck above the padded ground, m.
const DECK_LIFT_M: f32 = 0.05;
/// Thickness of the deck slabs, m (they reach below the ground).
const DECK_THICK_M: f32 = 0.6;
/// Thickness of painted markings, m.
const PAINT_THICK_M: f32 = 0.02;
/// Longest single deck / paint box, m.
const MAX_SLAB_M: f32 = 200.0;
/// Security fence height, m.
const FENCE_H_M: f32 = 2.8;
/// Longest fence piece, m.
const FENCE_PIECE_M: f32 = 12.0;
/// How far a fence edge may move inward to clear a road running along it, m.
const FENCE_MAX_SHIFT_M: f32 = 60.0;

/// Ground levelling this module needs before roads are profiled.
///
/// None: the plan pads level both sites flat (Vesper International to 6 m, Northfield to
/// 40 m) and every runway, approach-light row and building lies inside its site, so the
/// pad alone gives perfectly flat runways (the river at the airport's north end is ~450 m
/// beyond the site and the pad's 60 m blend).
pub fn earthworks(_plan: &Plan, _nat: &Natural) -> Vec<Earthwork> {
    Vec::new()
}

/// Place this module's content (Vesper International, then Northfield Airfield).
pub fn generate(ctx: &mut Ctx) {
    let mut rng = ctx.rng(SALT);
    let mut intl = rng.fork();
    let mut north = rng.fork();
    international::build(ctx, &mut intl);
    northfield::build(ctx, &mut north);
}

// ---------------------------------------------------------------------------------------
// Local frames and parts
// ---------------------------------------------------------------------------------------

/// A local frame: origin `c` at height `z`, local +Y along `yaw`.
#[derive(Debug, Clone, Copy)]
struct Frame {
    c: V2,
    yaw: f32,
    z: f32,
}

impl Frame {
    fn new(c: V2, yaw: f32, z: f32) -> Self {
        Self { c, yaw, z }
    }

    /// World point of local `(x, y)` (x right, y forward).
    fn pt(&self, x: f32, y: f32) -> V2 {
        let (s, co) = wp_math::sin_cos(self.yaw);
        [self.c[0] + co * x - s * y, self.c[1] + s * x + co * y]
    }

    /// A box part: centre `(x, y)`, bottom `z0` above the frame, full `size` (local axes).
    #[allow(clippy::too_many_arguments)]
    fn part(
        &self,
        kind: FeatureKind,
        mat: Material,
        x: f32,
        y: f32,
        z0: f32,
        size: V3,
        solid: bool,
    ) -> Feature {
        box_on(kind, mat, self.pt(x, y), self.z + z0, size, self.yaw, solid)
    }

    /// A box part with an extra yaw relative to the frame.
    #[allow(clippy::too_many_arguments)]
    fn part_yawed(
        &self,
        kind: FeatureKind,
        mat: Material,
        x: f32,
        y: f32,
        z0: f32,
        size: V3,
        yaw: f32,
        solid: bool,
    ) -> Feature {
        box_on(
            kind,
            mat,
            self.pt(x, y),
            self.z + z0,
            size,
            self.yaw + yaw,
            solid,
        )
    }

    /// An upright cylinder part standing at local `(x, y)`, bottom `z0` above the frame.
    #[allow(clippy::too_many_arguments)]
    fn cyl(
        &self,
        kind: FeatureKind,
        mat: Material,
        x: f32,
        y: f32,
        z0: f32,
        r: f32,
        h: f32,
        solid: bool,
    ) -> Feature {
        let p = self.pt(x, y);
        cylinder(kind, mat, [p[0], p[1], self.z + z0], r, h, solid)
    }
}

/// A flat strip from `a` to `b`, `width` wide, top at `z_top`, `thick` thick.
#[allow(clippy::too_many_arguments)]
fn strip(
    kind: FeatureKind,
    mat: Material,
    a: V2,
    b: V2,
    width: f32,
    z_top: f32,
    thick: f32,
    solid: bool,
) -> Feature {
    box_on(
        kind,
        mat,
        lerp2(a, b, 0.5),
        z_top - thick,
        [width, geom::dist(a, b), thick],
        yaw_of(a, b),
        solid,
    )
}

/// Split `a → b` into pieces of at most `max_m`.
fn pieces(a: V2, b: V2, max_m: f32) -> Vec<(V2, V2)> {
    let n = (geom::dist(a, b) / max_m).ceil().max(1.0) as usize;
    (0..n)
        .map(|k| {
            (
                lerp2(a, b, k as f32 / n as f32),
                lerp2(a, b, (k + 1) as f32 / n as f32),
            )
        })
        .collect()
}

/// Solid deck slabs (runway / taxiway / apron) from `a` to `b`.
fn deck(kind: FeatureKind, mat: Material, a: V2, b: V2, width: f32, z_top: f32) -> Vec<Feature> {
    pieces(a, b, MAX_SLAB_M)
        .into_iter()
        .map(|(p, q)| strip(kind, mat, p, q, width, z_top, DECK_THICK_M, true))
        .collect()
}

/// A painted (non-solid) line from `a` to `b` on a deck whose top is `z_top`.
fn paint(mat: Material, a: V2, b: V2, width: f32, z_top: f32) -> Vec<Feature> {
    pieces(a, b, MAX_SLAB_M)
        .into_iter()
        .map(|(p, q)| {
            strip(
                FeatureKind::AirfieldMarking,
                mat,
                p,
                q,
                width,
                z_top + PAINT_THICK_M,
                PAINT_THICK_M,
                false,
            )
        })
        .collect()
}

/// An axis-aligned rectangular deck (apron) `[x0, y0, x1, y1]` split into ≤ 200 m pieces
/// along its long side.
fn deck_rect(kind: FeatureKind, mat: Material, r: [f32; 4], z_top: f32) -> Vec<Feature> {
    let (w, h) = (r[2] - r[0], r[3] - r[1]);
    if h >= w {
        let x = (r[0] + r[2]) * 0.5;
        deck(kind, mat, [x, r[1]], [x, r[3]], w, z_top)
    } else {
        let y = (r[1] + r[3]) * 0.5;
        deck(kind, mat, [r[0], y], [r[2], y], h, z_top)
    }
}

// ---------------------------------------------------------------------------------------
// Runways and taxiways
// ---------------------------------------------------------------------------------------

/// A runway between its two thresholds.
#[derive(Debug, Clone, Copy)]
struct Runway {
    /// Threshold at one end, m.
    a: V2,
    /// Threshold at the other end, m.
    b: V2,
    /// Paved width, m.
    width: f32,
}

/// Runway slabs, paint, edge lights and approach lights (rows beyond each threshold, as
/// many as fit inside `site_poly`).
fn runway(ctx: &Ctx, rw: Runway, z_top: f32, site_poly: &[V2], out: &mut Vec<Feature>) {
    use Material::*;
    let len = geom::dist(rw.a, rw.b);
    let d = geom::normalize(geom::sub(rw.b, rw.a));
    let r = geom::right_of(d);
    let at =
        |s: f32, l: f32| -> V2 { geom::add(geom::add(rw.a, geom::scale(d, s)), geom::scale(r, l)) };
    out.extend(deck(
        FeatureKind::Runway,
        Asphalt,
        rw.a,
        rw.b,
        rw.width,
        z_top,
    ));
    let hw = rw.width * 0.5;
    // Edge lines.
    for side in [-1.0, 1.0] {
        let l = side * (hw - 1.2);
        out.extend(paint(PaintWhite, at(0.0, l), at(len, l), 0.9, z_top));
    }
    // Centreline dashes (30 m dash, 20 m gap) between the touchdown zones.
    let mut s = 70.0;
    while s + 30.0 <= len - 70.0 {
        out.extend(paint(PaintWhite, at(s, 0.0), at(s + 30.0, 0.0), 0.9, z_top));
        s += 50.0;
    }
    // Threshold piano keys, threshold bar and aiming points at both ends.
    let keys = ((hw - 2.5) / 2.6).floor().max(1.0) as usize;
    for (s0, sign) in [(0.0, 1.0), (len, -1.0)] {
        let p = |ds: f32, l: f32| at(s0 + sign * ds, l);
        out.extend(paint(
            PaintWhite,
            p(1.0, -hw + 0.5),
            p(1.0, hw - 0.5),
            1.8,
            z_top,
        ));
        for k in 0..keys {
            let off = 2.4 + k as f32 * 2.6;
            for side in [-1.0, 1.0] {
                out.extend(paint(
                    PaintWhite,
                    p(6.0, side * off),
                    p(36.0, side * off),
                    1.8,
                    z_top,
                ));
            }
        }
        let aim = (hw - 9.0).max(3.0);
        for side in [-1.0, 1.0] {
            out.extend(paint(
                PaintWhite,
                p(300.0, side * aim),
                p(345.0, side * aim),
                7.0,
                z_top,
            ));
        }
    }
    // Edge lights every 60 m (on the shoulder, flush with the deck).
    let n = (len / 60.0).floor() as usize;
    for k in 0..=n {
        let s = len * k as f32 / n.max(1) as f32;
        for side in [-1.0, 1.0] {
            out.push(box_on(
                FeatureKind::AirfieldMarking,
                Light,
                at(s, side * (hw + 1.5)),
                z_top,
                [0.4, 0.4, 0.35],
                0.0,
                false,
            ));
        }
    }
    // Approach lights: rows every 15 m beyond each threshold, a crossbar at the 4th row.
    for (s0, sign) in [(0.0, -1.0), (len, 1.0)] {
        for k in 1..=6 {
            let ds = 15.0 * k as f32;
            let c = at(s0 + sign * ds, 0.0);
            let w = if k == 4 { hw + 2.0 } else { 4.0 };
            // The whole row must be well inside the site (it sits inside the fence).
            let ends = [at(s0 + sign * ds, -w), at(s0 + sign * ds, w)];
            if ends.iter().any(|e| signed_dist(site_poly, *e) > -8.0) {
                break;
            }
            let g = ctx.ground_z(c);
            let yaw = yaw_of(rw.a, rw.b);
            out.push(box_on(
                FeatureKind::AirfieldMarking,
                Light,
                c,
                g + 1.0,
                [w * 2.0, 0.4, 0.3],
                yaw,
                false,
            ));
            out.push(box_on(
                FeatureKind::AirfieldMarking,
                Steel,
                c,
                g,
                [0.2, 0.2, 1.0],
                yaw,
                false,
            ));
        }
    }
}

/// A taxiway (or connector) from `a` to `b`: slabs plus a yellow centreline painted from
/// `trim_a` metres after `a` to `trim_b` metres before `b` (so it stops at runway edges),
/// and yellow edge lines when `edges`.
#[allow(clippy::too_many_arguments)]
fn taxiway(
    a: V2,
    b: V2,
    width: f32,
    z_top: f32,
    trim_a: f32,
    trim_b: f32,
    edges: bool,
    out: &mut Vec<Feature>,
) {
    use Material::*;
    out.extend(deck(FeatureKind::Taxiway, Asphalt, a, b, width, z_top));
    let len = geom::dist(a, b);
    if len <= trim_a + trim_b + 1.0 {
        return;
    }
    let d = geom::normalize(geom::sub(b, a));
    let r = geom::right_of(d);
    let at =
        |s: f32, l: f32| -> V2 { geom::add(geom::add(a, geom::scale(d, s)), geom::scale(r, l)) };
    out.extend(paint(
        PaintYellow,
        at(trim_a, 0.0),
        at(len - trim_b, 0.0),
        0.3,
        z_top,
    ));
    if edges {
        for side in [-1.0, 1.0] {
            let l = side * (width * 0.5 - 0.6);
            out.extend(paint(
                PaintYellow,
                at(trim_a, l),
                at(len - trim_b, l),
                0.25,
                z_top,
            ));
        }
    }
}

// ---------------------------------------------------------------------------------------
// Aircraft, hangars, furniture
// ---------------------------------------------------------------------------------------

/// A twin-engine airliner, nose along the frame's +Y; `sc` scales the ~40 m base model.
/// Wingspan 34·sc, fin top 12.6·sc.
fn airliner(f: Frame, sc: f32, livery: Material) -> Vec<Feature> {
    use FeatureKind::Aircraft as A;
    use Material::*;
    let mut v = vec![
        f.part(
            A,
            White,
            0.0,
            0.0,
            1.6 * sc,
            [4.0 * sc, 40.0 * sc, 4.0 * sc],
            true,
        ),
        f.part(
            A,
            White,
            0.0,
            -2.0 * sc,
            2.0 * sc,
            [34.0 * sc, 7.0 * sc, 0.6],
            true,
        ),
        f.part(
            A,
            White,
            0.0,
            -17.5 * sc,
            4.6 * sc,
            [13.0 * sc, 3.5 * sc, 0.4],
            true,
        ),
        f.part(
            A,
            livery,
            0.0,
            -17.0 * sc,
            5.6 * sc,
            [0.5, 6.0 * sc, 7.0 * sc],
            true,
        ),
        f.part(A, Black, 0.0, 15.0 * sc, 0.0, [0.5, 0.5, 1.7 * sc], true),
    ];
    for side in [-1.0, 1.0] {
        v.push(f.part(
            A,
            Steel,
            side * 6.5 * sc,
            1.0 * sc,
            0.8 * sc,
            [2.4 * sc, 4.5 * sc, 2.4 * sc],
            true,
        ));
        v.push(f.part(
            A,
            Black,
            side * 2.5 * sc,
            -3.0 * sc,
            0.0,
            [1.2, 1.2, 1.7 * sc],
            true,
        ));
    }
    v
}

/// A single-engine high-wing light aircraft (~7.5 m long, 11 m span), nose along +Y.
fn light_aircraft(f: Frame, livery: Material) -> Vec<Feature> {
    use FeatureKind::Aircraft as A;
    use Material::*;
    vec![
        f.part(A, White, 0.0, 0.0, 0.8, [1.3, 7.5, 1.4], true),
        f.part(A, White, 0.0, 0.8, 2.2, [11.0, 1.6, 0.2], true),
        f.part(A, White, 0.0, -3.3, 1.4, [3.4, 0.9, 0.12], true),
        f.part(A, livery, 0.0, -3.4, 2.0, [0.15, 1.1, 1.3], true),
        f.part(A, Black, 0.0, 0.5, 0.0, [2.2, 0.3, 0.8], true),
        f.part(A, Black, 0.0, 3.2, 0.0, [0.2, 0.2, 0.8], true),
    ]
}

/// A light helicopter (~11 m rotor), nose along +Y. The rotor blades are non-solid.
fn helicopter(f: Frame, livery: Material) -> Vec<Feature> {
    use FeatureKind::Aircraft as A;
    use Material::*;
    let q = core::f32::consts::FRAC_PI_4;
    let mut v = vec![
        f.part(A, livery, 0.0, 0.5, 0.45, [2.2, 4.5, 2.0], true),
        f.part(A, livery, 0.0, -4.4, 1.6, [0.4, 5.5, 0.5], true),
        f.part(A, livery, 0.0, -7.0, 1.6, [0.15, 0.8, 1.4], true),
        f.cyl(A, Steel, 0.0, 0.5, 2.45, 0.15, 0.45, true),
        f.part_yawed(A, Black, 0.0, 0.5, 2.9, [0.3, 11.0, 0.08], q, false),
        f.part_yawed(A, Black, 0.0, 0.5, 2.9, [0.3, 11.0, 0.08], -q, false),
    ];
    for side in [-1.0, 1.0] {
        v.push(f.part(A, Steel, side * 1.1, 0.5, 0.0, [0.15, 4.2, 0.5], true));
    }
    v
}

/// A hangar open on its local +Y side (back wall, side walls, roof, door header): `w`
/// wide (local X), `d` deep (local Y), `h` clear height, plus a solid floor slab flush with
/// the frame height when `floor`.
fn hangar(f: Frame, w: f32, d: f32, h: f32, floor: bool) -> Vec<Feature> {
    use FeatureKind::Hangar as H;
    use Material::*;
    let t = 0.5;
    let mut v = vec![
        f.part(
            H,
            Steel,
            0.0,
            -d * 0.5 + t * 0.5,
            -0.3,
            [w, t, h + 0.3],
            true,
        ),
        f.part(H, DarkConcrete, 0.0, 0.0, h, [w, d, 0.6], true),
        f.part(H, Steel, 0.0, d * 0.5 - 0.6, h - 3.0, [w, 1.2, 3.0], true),
    ];
    for side in [-1.0, 1.0] {
        v.push(f.part(
            H,
            Steel,
            side * (w * 0.5 - t * 0.5),
            0.0,
            -0.3,
            [t, d, h + 0.3],
            true,
        ));
    }
    if floor {
        v.push(f.part(
            FeatureKind::Apron,
            Concrete,
            0.0,
            0.0,
            -DECK_THICK_M,
            [w, d, DECK_THICK_M],
            true,
        ));
    }
    v
}

/// A windsock: a pole and an orange sock (non-solid) pointing along the frame's +Y.
fn windsock(f: Frame) -> Vec<Feature> {
    use FeatureKind::AirportFurniture as F;
    vec![
        f.cyl(F, Material::Steel, 0.0, 0.0, 0.0, 0.1, 6.5, true),
        f.part(
            F,
            Material::SafetyOrange,
            0.0,
            1.6,
            5.6,
            [0.8, 3.2, 0.8],
            false,
        ),
    ]
}

/// Tug, baggage carts and (sometimes) a fuel truck on the right side of an airliner whose
/// frame is `f` (scale `sc`).
fn ground_service(f: Frame, sc: f32, fuel: bool) -> Vec<Feature> {
    use FeatureKind::AirportFurniture as F;
    use Material::*;
    let mut v = vec![
        f.part(
            F,
            SafetyYellow,
            6.0 * sc,
            12.0 * sc,
            0.0,
            [1.6, 2.8, 1.5],
            true,
        ),
        f.part(
            F,
            Steel,
            6.0 * sc,
            12.0 * sc + 3.4,
            0.0,
            [1.5, 3.0, 1.2],
            true,
        ),
    ];
    if fuel {
        v.push(f.part(F, White, 11.0 * sc, 8.0 * sc, 0.0, [2.5, 9.0, 3.2], true));
    }
    v
}

// ---------------------------------------------------------------------------------------
// Placement guards
// ---------------------------------------------------------------------------------------

/// Points spread over a feature's footprint (corners, edge midpoints, centre, and a 4 m
/// grid inside large ones).
fn footprint_samples(f: &Feature) -> Vec<V2> {
    let fp = super::ctx::footprint(&f.shape);
    let n = fp.len();
    let mut pts = fp.clone();
    let mut c = [0.0, 0.0];
    for i in 0..n {
        pts.push(lerp2(fp[i], fp[(i + 1) % n], 0.5));
        c = geom::add(c, fp[i]);
    }
    let c = geom::scale(c, 1.0 / n as f32);
    pts.push(c);
    if let FeatureShape::Box { .. } | FeatureShape::Prism { .. } = f.shape {
        let (lo, hi) = super::ground::bounds(&fp);
        if (hi[0] - lo[0]) * (hi[1] - lo[1]) > 32.0 {
            let mut y = lo[1];
            while y <= hi[1] {
                let mut x = lo[0];
                while x <= hi[0] {
                    if inside(&fp, [x, y]) {
                        pts.push([x, y]);
                    }
                    x += 4.0;
                }
                y += 4.0;
            }
        }
    }
    pts
}

/// True when no solid feature of `group` touches road paving or a verge.
fn clear_of_roads(ctx: &Ctx, group: &[Feature]) -> bool {
    group.iter().filter(|f| f.solid).all(|f| {
        footprint_samples(f)
            .iter()
            .all(|p| !matches!(ctx.occ_at(*p), Occ::Road | Occ::Verge))
    })
}

/// Add a group of features all-or-nothing: dropped if any solid part touches a road or
/// verge. Solid parts are `place`d (occupancy), non-solid ones only added. Returns whether
/// the group was placed.
fn put(ctx: &mut Ctx, group: Vec<Feature>) -> bool {
    if !clear_of_roads(ctx, &group) {
        return false;
    }
    for f in group {
        if f.solid {
            ctx.place(f);
        } else {
            ctx.add(f);
        }
    }
    true
}

/// True when `poly` overlaps no plan site other than those in `own`.
fn outside_other_sites(ctx: &Ctx, poly: &[V2], own: &[&str]) -> bool {
    ctx.plan
        .sites
        .iter()
        .filter(|s| !own.contains(&s.id))
        .all(|s| {
            !poly.iter().any(|p| inside(&s.polygon, *p))
                && !s.polygon.iter().any(|p| inside(poly, *p))
        })
}

// ---------------------------------------------------------------------------------------
// Perimeter
// ---------------------------------------------------------------------------------------

/// An axis-aligned rectangle `[x0, y0, x1, y1]`.
type Rect = [f32; 4];

/// Polygon (CCW) of a rectangle.
fn rect_poly(r: Rect) -> Vec<V2> {
    super::plan::rect(r[0], r[1], r[2], r[3])
}

/// The fence rectangle of a (rectangular) site: its bounds with each edge moved inward (in
/// 2 m steps, at most [`FENCE_MAX_SHIFT_M`]) until no road paving or verge lies on it,
/// ignoring the driveways at the site's own gates.
fn fence_rect(ctx: &Ctx, site: &Site) -> Rect {
    let (lo, hi) = super::ground::bounds(&site.polygon);
    let mut r = [lo[0], lo[1], hi[0], hi[1]];
    // (edge index into r, is the edge horizontal (constant y), inward sign)
    for (k, horiz, sign) in [
        (1usize, true, 1.0f32),
        (3, true, -1.0),
        (0, false, 1.0),
        (2, false, -1.0),
    ] {
        let base = r[k];
        let (s0, s1) = if horiz {
            (lo[0], hi[0])
        } else {
            (lo[1], hi[1])
        };
        let gates: Vec<&Gate> = site
            .gates
            .iter()
            .filter(|g| (if horiz { g.pos[1] } else { g.pos[0] } - base).abs() < 1.0)
            .collect();
        let mut shift = 0.0;
        while shift <= FENCE_MAX_SHIFT_M {
            let c = base + sign * shift;
            let mut blocked = false;
            let mut s = s0;
            while s <= s1 && !blocked {
                let p = if horiz { [s, c] } else { [c, s] };
                let at_gate = gates.iter().any(|g| {
                    (if horiz { g.pos[0] } else { g.pos[1] } - s).abs() < g.width_m * 0.5 + 4.0
                });
                if !at_gate && matches!(ctx.occ_at(p), Occ::Road | Occ::Verge) {
                    blocked = true;
                }
                s += 2.0;
            }
            if !blocked {
                break;
            }
            shift += 2.0;
        }
        if shift > 0.0 && shift <= FENCE_MAX_SHIFT_M {
            // +1 m: sit mid-cell, clear of the last blocked occupancy cell.
            r[k] = base + sign * (shift + 1.0);
        }
    }
    r
}

/// The plan gates of `site` projected onto the nearest edge of the fence rectangle `r`:
/// `(position, heading, width)`.
fn fence_gates(site: &Site, r: Rect) -> Vec<(V2, f32, f32)> {
    site.gates
        .iter()
        .map(|g| {
            let p = g.pos;
            let cands = [
                ((p[1] - r[1]).abs(), [p[0], r[1]]),
                ((p[1] - r[3]).abs(), [p[0], r[3]]),
                ((p[0] - r[0]).abs(), [r[0], p[1]]),
                ((p[0] - r[2]).abs(), [r[2], p[1]]),
            ];
            let mut best = cands[0];
            for c in cands {
                if c.0 < best.0 {
                    best = c;
                }
            }
            (best.1, g.heading, g.width_m)
        })
        .collect()
}

/// Security fence around the rectangle `r`, sampled every metre: open at each gate (gap =
/// gate width + 2 m), wherever a road / verge / water crosses the line, and inside the
/// `closers` polygons (buildings on the line that close it themselves).
fn fence_loop(ctx: &Ctx, r: Rect, gates: &[(V2, f32, f32)], closers: &[Vec<V2>]) -> Vec<Feature> {
    let corners = [[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]];
    let mut out = Vec::new();
    for e in 0..4 {
        let (a, b) = (corners[e], corners[(e + 1) % 4]);
        let len = geom::dist(a, b);
        let n = len.ceil().max(1.0) as usize;
        let open: Vec<bool> = (0..n)
            .map(|k| {
                let p = lerp2(a, b, (k as f32 + 0.5) / n as f32);
                gates
                    .iter()
                    .any(|(g, _, w)| geom::dist(*g, p) < w * 0.5 + 1.0)
                    || matches!(ctx.occ_at(p), Occ::Road | Occ::Verge | Occ::Water)
                    || closers.iter().any(|c| signed_dist(c, p) < 0.3)
            })
            .collect();
        let mut k = 0;
        while k < n {
            if open[k] {
                k += 1;
                continue;
            }
            let k0 = k;
            while k < n && !open[k] {
                k += 1;
            }
            let pa = lerp2(a, b, k0 as f32 / n as f32);
            let pb = lerp2(a, b, k as f32 / n as f32);
            if geom::dist(pa, pb) < 0.5 {
                continue;
            }
            out.extend(wall_along(
                ctx,
                FeatureKind::Fence,
                Material::Steel,
                &[pa, pb],
                0.12,
                FENCE_H_M,
                FENCE_PIECE_M,
                &[],
                0.0,
            ));
        }
    }
    out
}

/// Barrier arm (non-solid, so a racer can crash through) across a gate opening and a guard
/// booth just inside it (right side, else left side, skipped if neither is clear of roads).
fn gate_kit(ctx: &mut Ctx, pos: V2, heading: f32, width: f32) {
    let g = ctx.ground_z(pos);
    let f = Frame::new(pos, heading, g);
    ctx.add(f.part(
        FeatureKind::Gate,
        Material::Red,
        0.0,
        1.0,
        0.9,
        [width * 0.9, 0.12, 0.12],
        false,
    ));
    for side in [1.0, -1.0] {
        let x = side * (width * 0.5 + 3.0);
        let booth = vec![
            f.part(
                FeatureKind::AirportFurniture,
                Material::White,
                x,
                6.0,
                0.0,
                [2.8, 3.2, 2.8],
                true,
            ),
            f.part(
                FeatureKind::AirportFurniture,
                Material::DarkConcrete,
                x,
                6.0,
                2.8,
                [3.4, 3.8, 0.3],
                true,
            ),
        ];
        if put(ctx, booth) {
            break;
        }
    }
}

/// Lowest-common deck height of a site: the highest ground under `poly` plus
/// [`DECK_LIFT_M`].
fn deck_z(ctx: &Ctx, poly: &[V2]) -> f32 {
    ctx.ground_range(poly).1 + DECK_LIFT_M
}

/// Pick `n` distinct indices out of `0..total` (deterministic shuffle).
fn pick(rng: &mut wp_math::Rng, total: usize, n: usize) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..total).collect();
    for i in (1..total).rev() {
        let j = rng.below(i as u32 + 1) as usize;
        idx.swap(i, j);
    }
    idx.truncate(n.min(total));
    idx.sort_unstable();
    idx
}

/// Fictional airline liveries (tail colours).
const LIVERIES: [Material; 5] = [
    Material::Blue,
    Material::Red,
    Material::Green,
    Material::SafetyOrange,
    Material::Black,
];

/// A random livery.
fn livery(rng: &mut wp_math::Rng) -> Material {
    LIVERIES[rng.below(LIVERIES.len() as u32) as usize]
}

/// Regular polygon (CCW) with `n` vertices, circumradius `r`.
fn ngon(c: V2, r: f32, n: usize) -> Vec<V2> {
    (0..n)
        .map(|k| {
            let (s, co) = wp_math::sin_cos(k as f32 * core::f32::consts::TAU / n as f32);
            [c[0] + r * co, c[1] + r * s]
        })
        .collect()
}
