//! Body-kit geometry shared by the performance styles: lamps, grilles, splitters, skirts,
//! wings, diffusers, vents, louvres, side intakes, mirrors, handles, fillers, wipers.
//!
//! Everything is built from the foundation's [`Builder`] primitives and positioned from the
//! styled [`BodyProfile`], so it follows each car's real loft. [`Kit`] wraps the builder and
//! the profile; sided helpers take `sx` (−1 = left, +1 = right) and emit the left / right
//! part names (`…_l` / `…_r`) and regions themselves.
//!
//! Conventions (chassis space, m): +X right, +Y forward, +Z up. The nose face is the vertical
//! plane `y = p.y_nose` between `p.sill(y_nose)` and `p.belt(y_nose)`; the tail face is
//! `y = p.y_tail`. "Proud" = how far a part stands out of the face it sits on.

// Geometry helpers take explicit coordinates (x0, x1, z0, z1, …) on purpose: reads like the
// research tables and keeps call sites boring.
#![allow(clippy::too_many_arguments)]

use wp_contracts::BodyRegion;

use crate::builder::{mirror_name, mirror_region, Builder, PartKey, Patch, PatchBand};
use crate::mesh::{MaterialSlot, PartKind};
use crate::profile::BodyProfile;

use MaterialSlot as M;

/// Top-surface band between two signed x values (any order).
pub fn top_band(x0: f32, x1: f32) -> PatchBand {
    PatchBand::Top {
        x0: x0.min(x1),
        x1: x0.max(x1),
    }
}

/// Left (−1) and right (+1).
pub const SIDES: [f32; 2] = [-1.0, 1.0];

/// A detail part key (kind `Detail`).
pub fn dkey(name: &str, region: BodyRegion, mat: MaterialSlot) -> PartKey {
    PartKey::body(name, region, PartKind::Detail, mat)
}

/// A lamp part key (kind `Light`).
pub fn lkey(name: &str, region: BodyRegion, mat: MaterialSlot) -> PartKey {
    PartKey::body(name, region, PartKind::Light, mat)
}

/// Key for side `sx` from the LEFT-side key (`…_l`, left region): the right copy swaps the
/// name token and mirrors the region.
pub fn sided(k: &PartKey, sx: f32) -> PartKey {
    if sx > 0.0 {
        PartKey {
            name: mirror_name(&k.name),
            region: k.region.map(mirror_region),
            ..k.clone()
        }
    } else {
        k.clone()
    }
}

/// The foundation's default head-light key of side `sx` (we append our lamp shapes to it).
pub fn headlight_key(sx: f32) -> PartKey {
    sided(
        &lkey(
            "headlight_l",
            BodyRegion::FrameFront,
            MaterialSlot::Headlight,
        ),
        sx,
    )
}

/// The foundation's default tail-light key of side `sx`.
pub fn taillight_key(sx: f32) -> PartKey {
    sided(
        &lkey(
            "taillight_l",
            BodyRegion::FrameRear,
            MaterialSlot::Taillight,
        ),
        sx,
    )
}

/// The foundation's default door-mirror key of side `sx`.
pub fn mirror_key(sx: f32) -> PartKey {
    sided(
        &PartKey::body(
            "mirror_l",
            BodyRegion::LeftDoorF,
            PartKind::Mirror,
            MaterialSlot::Paint,
        ),
        sx,
    )
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn scale(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-9);
    scale(a, 1.0 / l)
}

/// Wing stand style.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Stand {
    /// No stands (the wing sits on its end plates / legs).
    None,
    /// Upright plates under the wing at ±x.
    Pedestal(f32),
    /// Swan-neck: risers behind the wing that hook over its top, at ±x.
    SwanNeck(f32),
}

/// Rear wing description (see [`Kit::wing`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Wing {
    /// Leading-edge y relative to the tail face (`y_tail + lead`).
    pub lead: f32,
    /// Chord, m.
    pub chord: f32,
    /// Height of the wing underside above the deck at the stands, m.
    pub height: f32,
    /// Half span, m (≤ body half width).
    pub half_span: f32,
    /// Thickness, m.
    pub thick: f32,
    /// Angle of attack, degrees (trailing edge up).
    pub aoa_deg: f32,
    /// Stands.
    pub stand: Stand,
    /// End plates: (height, length) or none.
    pub endplates: Option<(f32, f32)>,
    /// Material of the element.
    pub mat: MaterialSlot,
    /// Material of stands / plates.
    pub stand_mat: MaterialSlot,
    /// Gurney flap height (0 = none).
    pub gurney: f32,
}

/// Body-kit builder: the styled profile plus the part builder.
pub struct Kit<'a> {
    /// Styled profile of the car.
    pub p: &'a BodyProfile,
    /// Part builder.
    pub b: &'a mut Builder,
    /// Nose face y.
    pub yn: f32,
    /// Tail face y.
    pub yt: f32,
}

impl<'a> Kit<'a> {
    /// Wrap a builder; removes the foundation's default head / tail lamps and mirrors (the kit
    /// re-emits them into the same parts in the car's own shapes).
    pub fn new(p: &'a BodyProfile, b: &'a mut Builder) -> Kit<'a> {
        for sx in SIDES {
            for k in [headlight_key(sx), taillight_key(sx), mirror_key(sx)] {
                b.set_geometry(&k.name, Vec::new(), Vec::new(), Vec::new());
            }
        }
        Kit {
            p,
            b,
            yn: p.y_nose,
            yt: p.y_tail,
        }
    }

    // ------------------------------------------------------------------ anchors

    /// Half width of the nose face.
    pub fn wn(&self) -> f32 {
        self.p.half_width(self.yn)
    }
    /// Half width of the tail face.
    pub fn wt(&self) -> f32 {
        self.p.half_width(self.yt)
    }
    /// Nose face (bottom, top) z.
    pub fn nose_z(&self) -> (f32, f32) {
        (self.p.sill(self.yn), self.p.belt(self.yn))
    }
    /// Tail face (bottom, top) z (top at the outer edge; the centre is a little higher).
    pub fn tail_z(&self) -> (f32, f32) {
        (self.p.sill(self.yt), self.p.belt(self.yt))
    }
    /// Top-surface height at (y, x).
    pub fn top_z(&self, y: f32, x: f32) -> f32 {
        self.p.top_point(y, x).0[2]
    }
    /// Half width of the flat roof top at `y` (inside the roof-edge rounding).
    pub fn roof_edge(&self, y: f32) -> f32 {
        let ring = self.p.ring(y);
        if self.p.has_greenhouse(y) {
            -ring[12].p[0]
        } else {
            self.p.half_width(y) - 0.1
        }
    }
    /// Left side surface |x| at (y, z).
    pub fn side_x(&self, y: f32, z: f32) -> f32 {
        -self.p.side_point(y, z).0[0]
    }
    /// Front / rear wheel arches (y, z, r).
    pub fn arch(&self, front: bool) -> (f32, f32, f32) {
        let a = &self.p.arches;
        let i = if front {
            a.iter()
                .enumerate()
                .max_by(|x, y| x.1 .0.total_cmp(&y.1 .0))
                .map(|e| e.0)
        } else {
            a.iter()
                .enumerate()
                .min_by(|x, y| x.1 .0.total_cmp(&y.1 .0))
                .map(|e| e.0)
        };
        i.map(|i| a[i]).unwrap_or((0.0, 0.3, 0.35))
    }

    // ------------------------------------------------------------------ primitives

    /// Box between two corners (any order).
    pub fn bx(&mut self, k: &PartKey, a: [f32; 3], c: [f32; 3]) {
        let lo = [a[0].min(c[0]), a[1].min(c[1]), a[2].min(c[2])];
        let hi = [a[0].max(c[0]), a[1].max(c[1]), a[2].max(c[2])];
        self.b.cuboid(k, lo, hi);
    }

    /// Bevelled box between two corners (any order).
    pub fn bbx(&mut self, k: &PartKey, a: [f32; 3], c: [f32; 3], bevel: f32) {
        let lo = [a[0].min(c[0]), a[1].min(c[1]), a[2].min(c[2])];
        let hi = [a[0].max(c[0]), a[1].max(c[1]), a[2].max(c[2])];
        self.b.beveled_box(k, lo, hi, bevel);
    }

    /// Convex prism: `prof` = (y, z) outline (any winding), swept along x from `x0` to `x1`.
    pub fn prism_x(&mut self, k: &PartKey, prof: &[[f32; 2]], x0: f32, x1: f32) {
        let n = prof.len();
        let (cy, cz) = centroid(prof);
        for i in 0..n {
            let a = prof[i];
            let c = prof[(i + 1) % n];
            let q = [
                [x0, a[0], a[1]],
                [x1, a[0], a[1]],
                [x1, c[0], c[1]],
                [x0, c[0], c[1]],
            ];
            let out = [0.0, 0.5 * (a[0] + c[0]) - cy, 0.5 * (a[1] + c[1]) - cz];
            self.b.poly_facing(k, &q, out);
        }
        let e0: Vec<[f32; 3]> = prof.iter().map(|q| [x0, q[0], q[1]]).collect();
        let e1: Vec<[f32; 3]> = prof.iter().map(|q| [x1, q[0], q[1]]).collect();
        let d = if x1 >= x0 { 1.0 } else { -1.0 };
        self.b.poly_facing(k, &e0, [-d, 0.0, 0.0]);
        self.b.poly_facing(k, &e1, [d, 0.0, 0.0]);
    }

    /// Convex prism: `prof` = (x, z) outline, swept along y from `y0` to `y1`.
    pub fn prism_y(&mut self, k: &PartKey, prof: &[[f32; 2]], y0: f32, y1: f32) {
        let n = prof.len();
        let (cx, cz) = centroid(prof);
        for i in 0..n {
            let a = prof[i];
            let c = prof[(i + 1) % n];
            let q = [
                [a[0], y0, a[1]],
                [a[0], y1, a[1]],
                [c[0], y1, c[1]],
                [c[0], y0, c[1]],
            ];
            let out = [0.5 * (a[0] + c[0]) - cx, 0.0, 0.5 * (a[1] + c[1]) - cz];
            self.b.poly_facing(k, &q, out);
        }
        let e0: Vec<[f32; 3]> = prof.iter().map(|q| [q[0], y0, q[1]]).collect();
        let e1: Vec<[f32; 3]> = prof.iter().map(|q| [q[0], y1, q[1]]).collect();
        let d = if y1 >= y0 { 1.0 } else { -1.0 };
        self.b.poly_facing(k, &e0, [0.0, -d, 0.0]);
        self.b.poly_facing(k, &e1, [0.0, d, 0.0]);
    }

    /// Plate between two points `a` → `c` (its long axis), `w` wide along `across` (unit),
    /// `t` thick along the third axis.
    pub fn plate(
        &mut self,
        k: &PartKey,
        a: [f32; 3],
        c: [f32; 3],
        across: [f32; 3],
        w: f32,
        t: f32,
    ) {
        let ax = sub(c, a);
        let len = (ax[0] * ax[0] + ax[1] * ax[1] + ax[2] * ax[2]).sqrt();
        if len < 1e-4 {
            return;
        }
        let a0 = scale(ax, 1.0 / len);
        let a2 = norm(cross(a0, across));
        let a1 = cross(a2, a0);
        let centre = scale(add(a, c), 0.5);
        self.b
            .oriented_box(k, centre, [a0, a1, a2], [0.5 * len, 0.5 * w, 0.5 * t]);
    }

    /// Disc / cylinder along y centred at (x, y, z): from `y0` to `y1`.
    pub fn cyl_y(&mut self, k: &PartKey, x: f32, z: f32, y0: f32, y1: f32, r: f32, seg: usize) {
        self.b.cylinder(k, [x, y0, z], [x, y1, z], r, r, seg, true);
    }

    /// Surface patch.
    pub fn patch(
        &mut self,
        k: &PartKey,
        y0: f32,
        y1: f32,
        band: PatchBand,
        off: f32,
        thick: f32,
        steps: (usize, usize),
    ) {
        let p = self.p;
        self.b.surface_patch(
            k,
            p,
            &Patch {
                y0,
                y1,
                band,
                offset_m: off,
                thickness_m: thick,
                steps,
            },
        );
    }

    /// Side patch on side `sx` (the builder's side patches are left-side; the right copy is
    /// mirrored into `k`'s right key).
    pub fn side_patch(
        &mut self,
        k_left: &PartKey,
        sx: f32,
        y0: f32,
        y1: f32,
        z0: f32,
        z1: f32,
        off: f32,
        thick: f32,
        steps: (usize, usize),
    ) {
        if sx < 0.0 {
            self.patch(
                k_left,
                y0,
                y1,
                PatchBand::Side { z0, z1 },
                off,
                thick,
                steps,
            );
        } else {
            // Build into a scratch key, mirror into the right key, then drop the scratch.
            let tmp = PartKey {
                name: format!("{}_tmp_mirror", k_left.name),
                ..k_left.clone()
            };
            self.patch(&tmp, y0, y1, PatchBand::Side { z0, z1 }, off, thick, steps);
            self.b.mirror_part(&tmp.name, &sided(k_left, 1.0));
            self.b
                .set_geometry(&tmp.name, Vec::new(), Vec::new(), Vec::new());
        }
    }

    /// Small box standing on the left/right side surface at (y, z): `len` along y, `h` up,
    /// `depth` outward from the surface (+`gap`).
    pub fn side_box(
        &mut self,
        k: &PartKey,
        sx: f32,
        y: f32,
        z: f32,
        len: f32,
        h: f32,
        depth: f32,
        gap: f32,
    ) {
        // side_point is the left side (normal toward −x); flip x for the right side.
        let (pt, n) = self.p.side_point(y, z);
        let (base, n) = if sx > 0.0 {
            ([-pt[0], pt[1], pt[2]], [-n[0], 0.0, n[2]])
        } else {
            (pt, [n[0], 0.0, n[2]])
        };
        let n = norm(n);
        let centre = add(base, scale(n, gap + 0.5 * depth));
        let a1 = [0.0, 1.0, 0.0];
        let a2 = norm(cross(n, a1));
        self.b
            .oriented_box(k, centre, [n, a1, a2], [0.5 * depth, 0.5 * len, 0.5 * h]);
    }

    // ------------------------------------------------------------------ faces

    /// Box on the nose face: x0..x1, z0..z1, standing `proud` out of the face.
    pub fn front_box(
        &mut self,
        k: &PartKey,
        x0: f32,
        x1: f32,
        z0: f32,
        z1: f32,
        proud: f32,
        bevel: f32,
    ) {
        let y = self.yn;
        self.bbx(k, [x0, y - 0.02, z0], [x1, y + proud, z1], bevel);
    }

    /// Box on the tail face.
    pub fn rear_box(
        &mut self,
        k: &PartKey,
        x0: f32,
        x1: f32,
        z0: f32,
        z1: f32,
        proud: f32,
        bevel: f32,
    ) {
        let y = self.yt;
        self.bbx(k, [x0, y + 0.02, z0], [x1, y - proud, z1], bevel);
    }

    /// Round lamp on the nose (`front`) or tail face at (x, z), radius `r`, with an optional
    /// bezel ring (`bezel` material) slightly larger and less proud.
    pub fn round_lamp(
        &mut self,
        k: &PartKey,
        front: bool,
        x: f32,
        z: f32,
        r: f32,
        proud: f32,
        bezel: Option<&PartKey>,
    ) {
        let (yf, d) = if front {
            (self.yn, 1.0)
        } else {
            (self.yt, -1.0)
        };
        if let Some(bk) = bezel {
            self.cyl_y(
                bk,
                x,
                z,
                yf - d * 0.02,
                yf + d * (proud - 0.006),
                r + 0.016,
                14,
            );
        }
        self.cyl_y(k, x, z, yf - d * 0.02, yf + d * proud, r, 14);
    }

    /// Head lamp lying on the nose-top curve (side `sx`): lens `y0..y1` × |x| `xin..xout`
    /// on a dark housing `border` m larger all round.
    pub fn top_lamp(&mut self, sx: f32, y0: f32, y1: f32, xin: f32, xout: f32, border: f32) {
        let kh = sided(
            &dkey("headlamp_housing_l", BodyRegion::FrontLeftFender, M::Trim),
            sx,
        );
        let y1h = (y1 + border).min(self.yn - 0.005);
        self.patch(
            &kh,
            y0 - border,
            y1h,
            top_band(sx * (xin - border), sx * (xout + border)),
            0.002,
            0.007,
            (3, 2),
        );
        let hk = headlight_key(sx);
        self.patch(
            &hk,
            y0,
            y1,
            top_band(sx * xin, sx * xout),
            0.003,
            0.012,
            (2, 2),
        );
    }

    /// Black panel across the tail face (lamp surround), x within ±half, z0..z1.
    pub fn rear_panel(
        &mut self,
        name: &str,
        region: BodyRegion,
        half: f32,
        z0: f32,
        z1: f32,
        mat: MaterialSlot,
    ) {
        let k = dkey(name, region, mat);
        self.rear_box(&k, -half, half, z0, z1, 0.008, 0.004);
    }

    /// Grille on the nose face: dark insert with `slats` horizontal bars (`slat_mat`) and an
    /// optional surround frame. `region` is usually `FrontBumper` (or `Hood` above the bumper).
    pub fn grille(
        &mut self,
        name: &str,
        region: BodyRegion,
        half: f32,
        z0: f32,
        z1: f32,
        slats: usize,
        slat_mat: MaterialSlot,
        frame: Option<MaterialSlot>,
    ) {
        let g = dkey(name, region, M::Grille);
        self.front_box(&g, -half, half, z0, z1, 0.006, 0.0);
        if slats > 0 {
            let s = dkey(&format!("{name}_slats"), region, slat_mat);
            let h = (z1 - z0) / (slats as f32 + 1.0);
            for i in 1..=slats {
                let z = z0 + h * i as f32;
                self.front_box(
                    &s,
                    -half + 0.02,
                    half - 0.02,
                    z - 0.008,
                    z + 0.008,
                    0.014,
                    0.0,
                );
            }
        }
        if let Some(fm) = frame {
            let f = dkey(&format!("{name}_frame"), region, fm);
            let t = 0.02;
            self.front_box(&f, -half - t, half + t, z1, z1 + t, 0.018, 0.004);
            self.front_box(&f, -half - t, half + t, z0 - t, z0, 0.018, 0.004);
            self.front_box(&f, -half - t, -half, z0, z1, 0.018, 0.004);
            self.front_box(&f, half, half + t, z0, z1, 0.018, 0.004);
        }
    }

    /// Mesh grille: dark insert plus a lattice of horizontal and vertical bars (honeycomb look).
    pub fn mesh_grille(
        &mut self,
        name: &str,
        region: BodyRegion,
        half: f32,
        z0: f32,
        z1: f32,
        rows: usize,
        cols: usize,
        bar_mat: MaterialSlot,
    ) {
        let g = dkey(name, region, M::Grille);
        self.front_box(&g, -half, half, z0, z1, 0.006, 0.0);
        let s = dkey(&format!("{name}_mesh"), region, bar_mat);
        let h = (z1 - z0) / rows.max(1) as f32;
        for i in 1..rows {
            let z = z0 + h * i as f32;
            self.front_box(&s, -half, half, z - 0.005, z + 0.005, 0.012, 0.0);
        }
        let w = 2.0 * half / cols.max(1) as f32;
        for i in 1..cols {
            let x = -half + w * i as f32;
            self.front_box(&s, x - 0.005, x + 0.005, z0, z1, 0.012, 0.0);
        }
    }

    // ------------------------------------------------------------------ aero

    /// Splitter / chin lip: a plate under the nose sticking out `fwd` m, `inset` from the
    /// face edges, `t` thick, plus small end fences when `fences`.
    pub fn splitter(&mut self, mat: MaterialSlot, fwd: f32, inset: f32, t: f32, fences: bool) {
        let k = dkey("splitter", BodyRegion::FrontBumper, mat);
        let w = self.wn() - inset;
        let z = self.p.sill(self.yn);
        let y = self.yn;
        let prof = [[-w, z - t], [w, z - t], [w + 0.01, z], [-w - 0.01, z]];
        self.prism_y(&k, &prof, y - 0.30, y + fwd);
        if fences {
            for sx in SIDES {
                let x = sx * (w - 0.01);
                self.bx(
                    &k,
                    [x - 0.006, y - 0.05, z],
                    [x + 0.006, y + fwd - 0.01, z + 0.06],
                );
            }
        }
    }

    /// Front bumper corner canards (dive planes), `mat`.
    pub fn canards(&mut self, mat: MaterialSlot, z: f32) {
        let k = dkey("canards", BodyRegion::FrontBumper, mat);
        let y = self.yn;
        for sx in SIDES {
            let w = self.p.half_width(y - 0.12);
            let a = [sx * (w - 0.02), y - 0.18, z - 0.03];
            let c = [sx * (w + 0.05), y - 0.04, z + 0.03];
            self.plate(&k, a, c, [0.0, 1.0, 0.0], 0.10, 0.008);
        }
    }

    /// Side skirts between the arches, standing `out` m off the side, `h` tall from just below
    /// the sill.
    pub fn skirts(&mut self, mat: MaterialSlot, out: f32, h: f32) {
        let (yf, _, rf) = self.arch(true);
        let (yr, _, rr) = self.arch(false);
        let (y0, y1) = (yr + rr + 0.02, yf - rf - 0.02);
        if y1 - y0 < 0.3 {
            return;
        }
        let ym = 0.5 * (y0 + y1);
        let z = self.p.sill(ym);
        let x = self.side_x(ym, z + 0.06);
        for sx in SIDES {
            let k = sided(&dkey("skirt_l", BodyRegion::FrameLeft, mat), sx);
            let prof = [
                [sx * (x - 0.04), z - 0.025],
                [sx * (x + out), z - 0.025],
                [sx * (x + out), z + 0.5 * h],
                [sx * (x - 0.01), z + h],
                [sx * (x - 0.04), z + h],
            ];
            self.prism_y(&k, &prof, y0, y1);
        }
    }

    /// Rear diffuser: dark valance on the tail face, `half` wide, `h` tall from below the
    /// sill, with `fins` vertical fins that stick out `fin_out` m behind the face.
    pub fn diffuser(&mut self, mat: MaterialSlot, half: f32, h: f32, fins: usize, fin_out: f32) {
        let k = dkey("diffuser", BodyRegion::RearBumper, mat);
        let y = self.yt;
        let z0 = self.p.sill(y) - 0.02;
        self.bbx(&k, [-half, y + 0.30, z0], [half, y - 0.02, z0 + 0.025], 0.0);
        self.rear_box(&k, -half, half, z0, z0 + h, 0.012, 0.0);
        if fins > 0 {
            let f = dkey("diffuser_fins", BodyRegion::RearBumper, mat);
            for i in 0..fins {
                let x = -half + 0.06 + (2.0 * half - 0.12) * i as f32 / (fins - 1).max(1) as f32;
                let prof = [
                    [y + 0.25, z0 + 0.02],
                    [y - fin_out, z0 + 0.02],
                    [y - fin_out, z0 + 0.6 * h],
                    [y - 0.01, z0 + h],
                ];
                self.prism_x(&f, &prof, x - 0.006, x + 0.006);
            }
        }
    }

    /// Rear wing (see [`Wing`]). Region `Trunk` (swings with the lid).
    pub fn wing(&mut self, w: Wing) {
        let k = dkey("wing", BodyRegion::Trunk, w.mat);
        let ks = dkey("wing_stands", BodyRegion::Trunk, w.stand_mat);
        let y_le = self.yt + w.lead;
        let ym = y_le - 0.5 * w.chord;
        let deck = self.top_z(ym, 0.0);
        let zb = deck + w.height;
        // Aerofoil in (y, z): leading edge at +y, flat-ish bottom, trailing edge raised by aoa.
        let (s, _) = wp_math::sin_cos(w.aoa_deg.to_radians());
        let c = w.chord;
        let t = w.thick;
        let rise = c * s;
        let lift = |f: f32| rise * f; // trailing edge higher
        let prof = [
            [y_le, zb + 0.35 * t],
            [y_le - 0.04 * c, zb + 0.85 * t + lift(0.04)],
            [y_le - 0.30 * c, zb + t + lift(0.30)],
            [y_le - 0.75 * c, zb + 0.7 * t + lift(0.75)],
            [y_le - c, zb + 0.15 * t + lift(1.0)],
            [y_le - c, zb + lift(1.0) - 0.002],
            [y_le - 0.5 * c, zb + lift(0.5) - 0.004],
            [y_le - 0.08 * c, zb + 0.05 * t],
        ];
        let hs = w.half_span.min(self.p.half_w - 0.02);
        self.prism_x(&k, &prof, -hs, hs);
        if w.gurney > 0.0 {
            let ye = y_le - c;
            let ze = zb + lift(1.0);
            self.bx(
                &ks,
                [-hs, ye, ze],
                [hs, ye + 0.006, ze + w.gurney + 0.15 * t],
            );
        }
        if let Some((ph, pl)) = w.endplates {
            let zc = zb + 0.5 * t + 0.5 * rise;
            for sx in SIDES {
                let x = sx * (hs + 0.004);
                let prof = [
                    [y_le + 0.03, zc - 0.55 * ph],
                    [y_le - pl + 0.04, zc - 0.55 * ph],
                    [y_le - pl, zc + 0.45 * ph],
                    [y_le - 0.04, zc + 0.45 * ph],
                ];
                self.prism_x(&ks, &prof, x - 0.004, x + 0.004);
            }
        }
        match w.stand {
            Stand::None => {}
            Stand::Pedestal(x) => {
                for sx in SIDES {
                    let xs = sx * x;
                    let yb = ym + 0.02;
                    let zd = self.top_z(yb, xs);
                    let prof = [
                        [yb + 0.07, zd - 0.01],
                        [yb - 0.07, zd - 0.01],
                        [yb - 0.04, zb + 0.01],
                        [yb + 0.03, zb + 0.01],
                    ];
                    self.prism_x(&ks, &prof, xs - 0.009, xs + 0.009);
                }
            }
            Stand::SwanNeck(x) => {
                for sx in SIDES {
                    let xs = sx * x;
                    let yb = y_le - c - 0.02;
                    let zd = self.top_z(yb, xs);
                    let ztop = zb + t + lift(0.5) + 0.02;
                    // Riser behind the wing, then a hook forward over its top.
                    self.plate(
                        &ks,
                        [xs, yb - 0.02, zd - 0.01],
                        [xs, yb + 0.01, ztop],
                        [0.0, 1.0, 0.0],
                        0.06,
                        0.012,
                    );
                    self.plate(
                        &ks,
                        [xs, yb + 0.03, ztop - 0.01],
                        [xs, y_le - 0.35 * c, ztop - 0.02],
                        [0.0, 0.0, 1.0],
                        0.03,
                        0.012,
                    );
                }
            }
        }
    }

    /// Hoop wing: a wing spanning the deck whose ends turn down into legs on the quarters.
    pub fn hoop_wing(&mut self, lead: f32, chord: f32, height: f32, mat: MaterialSlot) {
        let k = dkey("wing", BodyRegion::Trunk, mat);
        let y_le = self.yt + lead;
        let ym = y_le - 0.5 * chord;
        let deck = self.top_z(ym, 0.0);
        let zb = deck + height;
        let hs = self.p.half_width(ym) - 0.10;
        let t = 0.035;
        let prof = [
            [y_le, zb + 0.4 * t],
            [y_le - 0.3 * chord, zb + t],
            [y_le - chord, zb + 0.5 * t + 0.02],
            [y_le - chord, zb + 0.02],
            [y_le - 0.1 * chord, zb],
        ];
        let xs = hs - 0.12;
        self.prism_x(&k, &prof, -xs, xs);
        for sx in SIDES {
            // Leg: from the wing end down and out onto the deck edge.
            let x_end = sx * xs;
            let x_foot = sx * hs;
            let zf = self.top_z(ym, x_foot);
            let a = [x_end, ym, zb + 0.5 * t];
            let c = [x_foot, ym, zf - 0.01];
            self.plate(&k, a, c, [0.0, 1.0, 0.0], 0.8 * chord, t);
        }
    }

    /// Roof spoiler at the roof's rear edge, reaching `chord` m back over the rear glass,
    /// `drop` m lower at its trailing edge; optional end plates (`plates` tall).
    pub fn roof_spoiler(
        &mut self,
        mat: MaterialSlot,
        chord: f32,
        drop: f32,
        inset: f32,
        plates: f32,
    ) {
        let k = dkey("roof_spoiler", BodyRegion::Trunk, mat);
        let y0 = self.p.roof.0 + 0.06;
        let z0 = self.top_z(y0, 0.0);
        let hw = self.p.roof_half_w(y0) - inset;
        let t = 0.03;
        let prof = [
            [y0, z0 - 0.01],
            [y0, z0 + 0.6 * t],
            [y0 - 0.4 * chord, z0 + t - 0.3 * drop],
            [y0 - chord, z0 + 0.5 * t - drop],
            [y0 - chord, z0 - drop],
            [y0 - 0.5 * chord, z0 - 0.04 - 0.5 * drop],
        ];
        self.prism_x(&k, &prof, -hw, hw);
        if plates > 0.0 {
            let kp = dkey("roof_spoiler_plates", BodyRegion::Trunk, mat);
            for sx in SIDES {
                let x = sx * (hw - 0.01);
                let prof = [
                    [y0 - 0.02, z0 + t],
                    [y0 - chord - 0.02, z0 + 0.5 * t - drop],
                    [y0 - chord - 0.02, z0 - drop - plates],
                    [y0 - 0.12, z0 - plates],
                ];
                self.prism_x(&kp, &prof, x - 0.008, x + 0.008);
            }
        }
    }

    /// Lip spoiler on the deck at `y`, `h` tall, `chord` long, `inset` from the sides.
    pub fn lip_spoiler(&mut self, mat: MaterialSlot, y: f32, h: f32, chord: f32, inset: f32) {
        let k = dkey("lip_spoiler", BodyRegion::Trunk, mat);
        let z = self.top_z(y, 0.0);
        let z_back = self.top_z(y - chord, 0.0);
        let hw = self.p.half_width(y) - inset;
        let prof = [
            [y, z - 0.01],
            [y - chord, z_back - 0.01],
            [y - chord - 0.01, z_back + h],
            [y - chord + 0.02, z_back + h + 0.005],
        ];
        // Follow the deck camber at the ends by trimming the span to the crown.
        self.prism_x(&k, &prof, -hw, hw);
    }

    // ------------------------------------------------------------------ hood / deck

    /// Dark vent on the hood (top band x0..x1, y0..y1) with `slats` transverse slats on it.
    pub fn top_vent(
        &mut self,
        name: &str,
        region: BodyRegion,
        y0: f32,
        y1: f32,
        x0: f32,
        x1: f32,
        slats: usize,
        slat_mat: MaterialSlot,
    ) {
        let k = dkey(name, region, M::Grille);
        self.patch(&k, y0, y1, PatchBand::Top { x0, x1 }, 0.004, 0.006, (2, 1));
        if slats > 0 {
            let s = dkey(&format!("{name}_slats"), region, slat_mat);
            for i in 0..slats {
                let y = y0 + (y1 - y0) * (i as f32 + 0.5) / slats as f32;
                let z = self
                    .top_z(y, 0.5 * (x0 + x1))
                    .max(self.top_z(y, x0))
                    .max(self.top_z(y, x1));
                self.bx(
                    &s,
                    [x0 + 0.01, y - 0.008, z + 0.002],
                    [x1 - 0.01, y + 0.008, z + 0.016],
                );
            }
        }
    }

    /// Transverse louvres across the deck (`n` slats from `y0` to `y1`) on a dark base,
    /// spanning |x| in `inner..outer` (inner = 0: one span across the centre line).
    pub fn louvres(
        &mut self,
        region: BodyRegion,
        y0: f32,
        y1: f32,
        n: usize,
        inner: f32,
        outer: f32,
        mat: MaterialSlot,
    ) {
        let base = dkey("louvre_base", region, M::Grille);
        let spans: Vec<(f32, f32)> = if inner > 0.0 {
            vec![(-outer, -inner), (inner, outer)]
        } else {
            vec![(-outer, outer)]
        };
        for &(x0, x1) in &spans {
            self.patch(
                &base,
                y0,
                y1,
                PatchBand::Top { x0, x1 },
                0.003,
                0.005,
                (0, 3),
            );
        }
        let k = dkey("louvres", region, mat);
        for i in 0..n {
            let y = y0 + (y1 - y0) * (i as f32 + 0.5) / n as f32;
            for &(x0, x1) in &spans {
                let z = self
                    .top_z(y, x0)
                    .max(self.top_z(y, x1))
                    .max(self.top_z(y, 0.5 * (x0 + x1)));
                let prof = [
                    [y + 0.02, z + 0.006],
                    [y - 0.025, z + 0.006],
                    [y - 0.03, z + 0.024],
                    [y + 0.01, z + 0.022],
                ];
                self.prism_x(&k, &prof, x0 + 0.02, x1 - 0.02);
            }
        }
    }

    /// Hood pins (pair) at y, ±x.
    pub fn hood_pins(&mut self, y: f32, x: f32) {
        let k = dkey("hood_pins", BodyRegion::Hood, M::Chrome);
        for sx in SIDES {
            let z = self.top_z(y, sx * x);
            self.b.cylinder(
                &k,
                [sx * x, y, z - 0.005],
                [sx * x, y, z + 0.014],
                0.024,
                0.02,
                10,
                true,
            );
        }
    }

    /// Stripe decal (matte `mat`) on the top surface from y0 to y1 between x0..x1.
    pub fn top_stripe(
        &mut self,
        name: &str,
        region: BodyRegion,
        y0: f32,
        y1: f32,
        x0: f32,
        x1: f32,
        mat: MaterialSlot,
    ) {
        let k = dkey(name, region, mat);
        self.patch(&k, y0, y1, PatchBand::Top { x0, x1 }, 0.005, 0.0, (0, 0));
    }

    // ------------------------------------------------------------------ sides

    /// Wing-mirror on the front door (replaces the foundation's box): a tapered housing on a
    /// stalk, optional contrast cap (`cap`), `y_off` from the door hinge.
    pub fn mirrors(&mut self, y_off: f32, size: f32, cap: Option<MaterialSlot>) {
        for sx in SIDES {
            let yh = self.p.y_fd_hinge - y_off;
            let zb = self.p.belt(yh) + 0.02;
            let w = self.side_x(yh, zb - 0.06).max(self.p.half_width(yh) - 0.05);
            let k = mirror_key(sx);
            let x0 = sx * (w + 0.05);
            let x1 = sx * (w + 0.05 + 1.4 * size);
            let (zl, zh) = (zb + 0.03, zb + 0.03 + 0.75 * size);
            // Housing: a wedge, blunt at the back (mirror glass), rounded front.
            let ylo = yh - 0.75 * size;
            let prof = [
                [ylo, zl],
                [ylo + 0.45 * size, zl - 0.005],
                [ylo + 0.8 * size, zl + 0.2 * size],
                [ylo + 0.8 * size, zh - 0.2 * size],
                [ylo + 0.45 * size, zh + 0.005],
                [ylo, zh],
            ];
            self.prism_x(&k, &prof, x0, x1);
            let ks = sided(&dkey("mirror_stalk_l", BodyRegion::LeftDoorF, M::Trim), sx);
            self.bx(
                &ks,
                [sx * (w - 0.03), ylo + 0.1 * size, zb - 0.01],
                [x0 + sx * 0.01, ylo + 0.5 * size, zl + 0.02],
            );
            let kg = sided(
                &dkey("mirror_glass_l", BodyRegion::LeftDoorF, M::Chrome),
                sx,
            );
            self.bx(
                &kg,
                [x0 + sx * 0.012, ylo - 0.004, zl + 0.012],
                [x1 - sx * 0.012, ylo + 0.002, zh - 0.012],
            );
            if let Some(cm) = cap {
                let kc = sided(&dkey("mirror_cap_l", BodyRegion::LeftDoorF, cm), sx);
                let prof = [
                    [ylo + 0.42 * size, zl - 0.008],
                    [ylo + 0.82 * size, zl + 0.19 * size],
                    [ylo + 0.82 * size, zh - 0.19 * size],
                    [ylo + 0.42 * size, zh + 0.008],
                ];
                self.prism_x(&kc, &prof, x0 + sx * 0.005, x1 + sx * 0.004);
            }
        }
    }

    /// Door handles (front doors; rear doors too on 4-door cars), `mat`.
    pub fn door_handles(&mut self, mat: MaterialSlot) {
        let p = self.p;
        let mut doors: Vec<(BodyRegion, f32)> = Vec::new();
        match p.rear_door {
            Some((hinge, rear)) => {
                doors.push((BodyRegion::LeftDoorF, hinge + 0.14));
                doors.push((BodyRegion::LeftDoorR, rear + 0.16));
            }
            None => doors.push((BodyRegion::LeftDoorF, p.y_fd_rear + 0.16)),
        }
        for (region, y) in doors {
            let z = p.belt(y) - 0.09;
            for sx in SIDES {
                let k = sided(
                    &dkey(&format!("handle_{}_l", region_tag(region)), region, mat),
                    sx,
                );
                self.side_box(&k, sx, y, z, 0.14, 0.03, 0.014, 0.002);
            }
        }
    }

    /// Fuel filler flap on the rear quarter (side `sx`), Ø `d`.
    pub fn fuel_filler(&mut self, sx: f32, d: f32, mat: MaterialSlot) {
        let p = self.p;
        let (yr, zr, rr) = self.arch(false);
        let y = yr + rr + 0.20;
        let y = y.min(p.y_quarter.0 - 0.10);
        let z = (p.belt(y) - 0.12).max(zr + rr + 0.06);
        let (pt, n) = p.side_point(y, z);
        let (pt, n) = if sx > 0.0 {
            ([-pt[0], pt[1], pt[2]], [-n[0], n[1], n[2]])
        } else {
            (pt, n)
        };
        let k = sided(&dkey("fuel_filler_l", BodyRegion::RearLeftQuarter, mat), sx);
        self.b.cylinder(
            &k,
            add(pt, scale(n, -0.004)),
            add(pt, scale(n, 0.008)),
            0.5 * d,
            0.5 * d,
            12,
            true,
        );
    }

    /// Wipers lying on the windshield base (region `Windshield`).
    pub fn wipers(&mut self) {
        let p = self.p;
        let k = dkey("wipers", BodyRegion::Windshield, M::Trim);
        let y = p.y_fw - 0.06;
        let hw = p.roof_half_w(y).min(p.half_width(y) - 0.2);
        for (xa, xb) in [(-0.9 * hw, -0.08), (0.05, 0.8 * hw)] {
            let ya = y - 0.03;
            let yb = y - 0.11;
            let za = self.top_z(ya, xa) + 0.012;
            let zb = self.top_z(yb, xb) + 0.012;
            self.plate(
                &k,
                [xa, ya, za],
                [xb, yb, zb],
                [0.0, 0.0, 1.0],
                0.014,
                0.012,
            );
        }
    }

    /// Fog / driving lamps on the nose face at ±x, z, radius r (lens `Headlight`, ring `Trim`).
    pub fn fogs(&mut self, x: f32, z: f32, r: f32) {
        for sx in SIDES {
            let k = sided(&lkey("fog_l", BodyRegion::FrontBumper, M::Headlight), sx);
            let kb = sided(&dkey("fog_ring_l", BodyRegion::FrontBumper, M::Trim), sx);
            self.round_lamp(&k, true, sx * x, z, r, 0.014, Some(&kb));
        }
    }

    /// Mud flaps behind each wheel (rally), `mat`.
    pub fn mud_flaps(&mut self, mat: MaterialSlot) {
        let arches = self.p.arches.clone();
        for (i, &(y, z, r)) in arches.iter().enumerate() {
            let sx = if i % 2 == 0 { -1.0 } else { 1.0 };
            let yb = y - r + 0.02;
            let x = self.side_x(yb, z) - 0.02;
            let name = if y > 0.0 {
                "mud_flap_front_l"
            } else {
                "mud_flap_rear_l"
            };
            let region = if y > 0.0 {
                BodyRegion::FrameLeft
            } else {
                BodyRegion::RearLeftQuarter
            };
            let k = sided(&dkey(name, region, mat), sx);
            self.bx(
                &k,
                [sx * (x - 0.24), yb - 0.006, z - 0.24],
                [sx * x, yb + 0.006, z + 0.06],
            );
        }
    }
}

/// Short tag of a door region for part names.
fn region_tag(r: BodyRegion) -> &'static str {
    match r {
        BodyRegion::LeftDoorF | BodyRegion::RightDoorF => "front",
        _ => "rear",
    }
}

fn centroid(prof: &[[f32; 2]]) -> (f32, f32) {
    let n = prof.len().max(1) as f32;
    let (a, b) = prof
        .iter()
        .fold((0.0, 0.0), |acc, q| (acc.0 + q[0], acc.1 + q[1]));
    (a / n, b / n)
}
