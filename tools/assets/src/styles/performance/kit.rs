//! Body-kit geometry shared by the performance styles: lamps, grilles, splitters, skirts,
//! wings, diffusers, vents, louvres, side intakes, mirrors, handles, fillers (wipers: `trim`).
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
use crate::mesh::{self, MaterialSlot, PartKind, INDICATOR_SEAM_M};
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

/// Front turn-signal key of side `sx` (`indicator_front_l/_r`, amber, `FrameFront`).
pub fn indicator_front_key(sx: f32) -> PartKey {
    mesh::indicator_front_key(sx > 0.0)
}

/// Rear turn-signal key of side `sx` (`indicator_rear_l/_r`, `FrameRear`): `mat` is
/// `EmissiveAmber` for an amber section, `Taillight` for a red-on-red one.
pub fn indicator_rear_key(sx: f32, mat: MaterialSlot) -> PartKey {
    mesh::indicator_rear_key(sx > 0.0, mat)
}

/// Where a turn-signal segment sits in its lamp lens (see [`Kit::front_cluster`]); the value
/// is the segment's width (x) or height (z), m.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    /// Outer end of the lens (toward the car's corner).
    Outboard(f32),
    /// Inner end of the lens (toward the grille / plate).
    Inboard(f32),
    /// Strip along the bottom of the lens.
    Lower(f32),
    /// Strip along the top of the lens.
    Upper(f32),
}

impl Seg {
    /// Split a lens rectangle (|x| `xin..xout`, `z0..z1`) into (lamp, signal) rectangles
    /// `[xin, xout, z0, z1]`, the lamp keeping [`INDICATOR_SEAM_M`] clear of the signal.
    pub fn split(self, xin: f32, xout: f32, z0: f32, z1: f32) -> ([f32; 4], [f32; 4]) {
        let g = INDICATOR_SEAM_M;
        match self {
            Seg::Outboard(w) => ([xin, xout - w - g, z0, z1], [xout - w, xout, z0, z1]),
            Seg::Inboard(w) => ([xin + w + g, xout, z0, z1], [xin, xin + w, z0, z1]),
            Seg::Lower(h) => ([xin, xout, z0 + h + g, z1], [xin, xout, z0, z0 + h]),
            Seg::Upper(h) => ([xin, xout, z0, z1 - h - g], [xin, xout, z1 - h, z1]),
        }
    }
}

/// An outline given for the LEFT side (x < 0, or |x|) moved to side `sx` (x only).
pub fn on_side(sx: f32, pts: &[[f32; 2]]) -> Vec<[f32; 2]> {
    pts.iter().map(|q| [sx * q[0].abs(), q[1]]).collect()
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

/// Where a door mirror sits (see [`Kit::mirrors`]): belt height `zb` and side |x| `w` at the
/// mirror, housing x span `x0 → x1` (signed, inner → outer), housing bottom / top `zl` / `zh`
/// and its rear (glass) face `ylo`.
struct MirrorFrame {
    zb: f32,
    w: f32,
    x0: f32,
    x1: f32,
    zl: f32,
    zh: f32,
    ylo: f32,
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
    /// Lens grid step of [`Kit::proj_cluster`] lamps, m (0.01…0.08, see
    /// [`crate::lamp::Lamp::step_m`]): coarser for cars near their triangle budget. Default 0.04.
    pub lens_step_m: f32,
}

impl<'a> Kit<'a> {
    /// Wrap a builder; removes the foundation's default head / tail lamps, turn signals and
    /// mirrors (the kit re-emits them into the same parts in the car's own shapes).
    pub fn new(p: &'a BodyProfile, b: &'a mut Builder) -> Kit<'a> {
        // The kit's mirrors (`Kit::mirrors`) replace the generic ones (`trim::mirrors`).
        crate::trim::remove_generic(b, &crate::trim::GENERIC_MIRRORS);
        for sx in SIDES {
            for k in [headlight_key(sx), taillight_key(sx), mirror_key(sx)] {
                b.set_geometry(&k.name, Vec::new(), Vec::new(), Vec::new());
            }
            // The kit's turn signals may use another material (red-on-red): drop the parts.
            for k in [
                indicator_front_key(sx),
                indicator_rear_key(sx, M::EmissiveAmber),
            ] {
                b.remove_part(&k.name);
            }
        }
        Kit {
            p,
            b,
            yn: p.y_nose,
            yt: p.y_tail,
            lens_step_m: 0.04,
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
            -ring[crate::profile::RING_PILLAR_IN].p[0]
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

    /// Flying buttress (sail panel) on side `sx`: a vertical fin `w` thick centred at |x| = `x`,
    /// whose top runs straight from the top surface at `y0` (the roof's rear corner) to
    /// `h_end` above the top surface at `y1` (on the deck), its foot buried in the body.
    #[allow(clippy::too_many_arguments)]
    pub fn sail(&mut self, k: &PartKey, sx: f32, y0: f32, y1: f32, x: f32, w: f32, h_end: f32) {
        let zt0 = self.top_z(y0, sx * x);
        let zt1 = self.top_z(y1, sx * x) + h_end;
        let zb0 = zt0 - 0.08;
        let zb1 = self.top_z(y1, sx * x) - 0.03;
        let prof = [[y0, zb0], [y1, zb1], [y1, zt1], [y0, zt0]];
        self.prism_x(k, &prof, sx * (x - 0.5 * w), sx * (x + 0.5 * w));
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
        self.top_lamp_housing(sx, y0, y1, xin, xout, border);
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

    /// Dark housing of a [`Kit::top_lamp`], `border` m larger than the lens all round.
    fn top_lamp_housing(&mut self, sx: f32, y0: f32, y1: f32, xin: f32, xout: f32, border: f32) {
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
    }

    /// [`Kit::top_lamp`] whose lens carries the front turn signal: the same housing, the lens
    /// split into the head lamp and an amber segment `seg` (`Outboard` / `Inboard` cut across
    /// x; `Lower` = the leading strip, `Upper` = the rear strip, cut along y), both at the same
    /// offset and thickness so the signal is flush with the lens.
    #[allow(clippy::too_many_arguments)]
    pub fn top_lamp_split(
        &mut self,
        sx: f32,
        y0: f32,
        y1: f32,
        xin: f32,
        xout: f32,
        border: f32,
        seg: Seg,
    ) {
        self.top_lamp_housing(sx, y0, y1, xin, xout, border);
        // Rectangles in (|x|, y): the split's "z" is y here.
        let (lamp, sig) = seg.split(xin, xout, y0, y1);
        let (lamp, sig) = match seg {
            // On the sloping nose top the leading (front) strip is the lower one.
            Seg::Lower(h) => (
                [xin, xout, y0, y1 - h - INDICATOR_SEAM_M],
                [xin, xout, y1 - h, y1],
            ),
            Seg::Upper(h) => (
                [xin, xout, y0 + h + INDICATOR_SEAM_M, y1],
                [xin, xout, y0, y0 + h],
            ),
            _ => (lamp, sig),
        };
        let hk = headlight_key(sx);
        self.patch(
            &hk,
            lamp[2],
            lamp[3],
            top_band(sx * lamp[0], sx * lamp[1]),
            0.003,
            0.012,
            (2, 2),
        );
        let ik = indicator_front_key(sx);
        self.patch(
            &ik,
            sig[2],
            sig[3],
            top_band(sx * sig[0], sx * sig[1]),
            0.003,
            0.012,
            (2, 1),
        );
    }

    /// Nose-face lamp lens (|x| `xin..xout`, `z0..z1`, `proud`, `bevel`, as [`Kit::front_box`])
    /// with the front turn signal cut out of it: the head lamp and an amber segment `seg` of
    /// the same lens, same proud face and bevel, split by [`INDICATOR_SEAM_M`].
    #[allow(clippy::too_many_arguments)]
    pub fn front_cluster(
        &mut self,
        sx: f32,
        xin: f32,
        xout: f32,
        z0: f32,
        z1: f32,
        proud: f32,
        bevel: f32,
        seg: Seg,
    ) {
        let (lamp, sig) = seg.split(xin, xout, z0, z1);
        let hk = headlight_key(sx);
        self.front_box(
            &hk,
            sx * lamp[0],
            sx * lamp[1],
            lamp[2],
            lamp[3],
            proud,
            bevel,
        );
        let ik = indicator_front_key(sx);
        self.front_box(&ik, sx * sig[0], sx * sig[1], sig[2], sig[3], proud, bevel);
    }

    /// A head (`front`) or tail lamp of side `sx` projected onto the body
    /// ([`crate::lamp`]): `outline` is given for the LEFT side in `view`'s coordinates (front /
    /// rear / top: x < 0; side: the left side's `[y, z]`) and mirrored for the right. The lens
    /// goes into the head / tail lamp part; with `seg` the turn signal is cut out of it as a
    /// segment of the same lens (front: amber; rear: `rear_mat`, `EmissiveAmber` or `Taillight`;
    /// `Outboard` / `Inboard` cut across the outline's first axis — the car's corner side; for
    /// a side view, the end of the car — `Lower` / `Upper` along the second: z, or for a top
    /// view y with `Lower` the leading strip). Without `seg` the kit must add the signal
    /// itself (e.g. [`Kit::signals_beside_lamps`]). `bezel_w` > 0 adds a black bezel that wide
    /// (`headlamp_bezel_l` / `taillamp_bezel_l` details on the frame). `yaw_deg` swings a
    /// front / rear cast round the corner (wraparound lamps). The lens grid step is
    /// [`Kit::lens_step_m`]. A top-view outline moves back to end 1 cm short of a raked end
    /// face's top edge.
    pub fn proj_cluster(
        &mut self,
        sx: f32,
        front: bool,
        view: crate::lamp::LampView,
        outline: &[[f32; 2]],
        seg: Option<Seg>,
        rear_mat: MaterialSlot,
        bezel_w: f32,
        yaw_deg: f32,
    ) {
        use crate::lamp::{bezel, clip_outline, lens, Lamp, LampView};
        let right = sx > 0.0;
        let (view, mut pts): (LampView, Vec<[f32; 2]>) = match view {
            LampView::Side { .. } => (LampView::Side { right }, outline.to_vec()),
            v => (v, outline.iter().map(|q| [sx * q[0].abs(), q[1]]).collect()),
        };
        // A top-view lamp near a raked end face moves back (or forward, at the tail) until it
        // ends 1 cm short of the edge where the top meets the face: past it the outline ran down
        // the face, and the lens there sat under the body top's edge seen from ahead. Shape
        // kept. No-op without a face rake.
        if view == LampView::Top {
            let p = self.p;
            let depth = p.face_depth(front);
            if depth > 1e-4 {
                let y_ref = if front {
                    p.y_nose - depth - 0.05
                } else {
                    p.y_tail + depth + 0.05
                };
                let z_top = pts
                    .iter()
                    .map(|q| p.top_point(y_ref, q[0]).0[2])
                    .fold(f32::MIN, f32::max);
                let set = p.face_setback(front, z_top);
                let over = if front {
                    pts.iter().map(|q| q[1]).fold(f32::MIN, f32::max) - (p.y_nose - set - 0.01)
                } else {
                    (p.y_tail + set + 0.01) - pts.iter().map(|q| q[1]).fold(f32::MAX, f32::min)
                };
                if set > 1e-4 && over > 0.0 {
                    let dy = if front { -over } else { over };
                    for q in &mut pts {
                        q[1] += dy;
                    }
                }
            }
        }
        let mut lamp = Lamp::new(view, pts.clone());
        lamp.yaw_deg = yaw_deg;
        lamp.step_m = self.lens_step_m;
        let (lamp_key, sig_key, region) = if front {
            (
                headlight_key(sx),
                indicator_front_key(sx),
                BodyRegion::FrameFront,
            )
        } else {
            (
                taillight_key(sx),
                indicator_rear_key(sx, rear_mat),
                BodyRegion::FrameRear,
            )
        };
        if bezel_w > 0.0 {
            let name = if front {
                "headlamp_bezel_l"
            } else {
                "taillamp_bezel_l"
            };
            let bk = sided(&dkey(name, region, M::Trim), sx);
            bezel(self.b, self.p, &bk, &lamp, bezel_w);
        }
        let Some(seg) = seg else {
            lens(self.b, self.p, &lamp_key, &lamp);
            return;
        };
        let lo = |k: usize| pts.iter().map(|q| q[k]).fold(f32::MAX, f32::min);
        let hi = |k: usize| pts.iter().map(|q| q[k]).fold(f32::MIN, f32::max);
        let g = INDICATOR_SEAM_M;
        // (axis, cut, signal on the high side of the cut).
        let (axis, at, sig_high) = match (seg, view) {
            (Seg::Outboard(w) | Seg::Inboard(w), LampView::Side { .. }) => {
                let toward_end = matches!(seg, Seg::Outboard(_)) == front;
                if toward_end {
                    (0, hi(0) - w, true)
                } else {
                    (0, lo(0) + w, false)
                }
            }
            (Seg::Outboard(w) | Seg::Inboard(w), _) => {
                let outer_high = matches!(seg, Seg::Outboard(_)) == right;
                if outer_high {
                    (0, hi(0) - w, true)
                } else {
                    (0, lo(0) + w, false)
                }
            }
            (Seg::Lower(h), LampView::Top) => (1, hi(1) - h, true),
            (Seg::Upper(h), LampView::Top) => (1, lo(1) + h, false),
            (Seg::Lower(h), _) => (1, lo(1) + h, false),
            (Seg::Upper(h), _) => (1, hi(1) - h, true),
        };
        let below_cut = |d: f32| clip_outline(&pts, axis, at + d, true);
        let above_cut = |d: f32| clip_outline(&pts, axis, at + d, false);
        let (sig, rest) = if sig_high {
            (above_cut(0.5 * g), below_cut(-0.5 * g))
        } else {
            (below_cut(-0.5 * g), above_cut(0.5 * g))
        };
        for (k, o) in [(&lamp_key, rest), (&sig_key, sig)] {
            if o.len() >= 3 {
                let mut l = lamp.clone();
                l.outline = o;
                lens(self.b, self.p, k, &l);
            }
        }
    }

    // ------------------------------------------------------------------ shared panels
    // Promoted from the phase-2 groups' local helpers (tuner / hatch `face_poly`, supercar
    // `top_poly` / `side_poly` / `end_poly`, tuner `swept_top_lamp`); the draped ones are cast
    // onto the built body like the projected lamps (`crate::lamp::panel`), so they follow
    // curved, sculpted and raked surfaces and take concave outlines.
    //
    // ```ignore
    // // Inside a style kit (`k: Kit`, side `sx`):
    // // a mesh intake on the nose face, a vent on each fender top, a side gill, a badge:
    // k.end_poly(&dkey("intake", BodyRegion::FrontBumper, M::Grille), true,
    //     &[[-0.30, 0.25], [0.30, 0.25], [0.24, 0.36], [-0.24, 0.36]], 0.004, 0.02);
    // k.top_poly(&sided(&dkey("fender_vent_l", BodyRegion::FrontLeftFender, M::Grille), sx),
    //     &on_side(sx, &[[0.55, 1.6], [0.75, 1.6], [0.72, 1.3], [0.58, 1.3]]), 0.003, 0.01);
    // k.side_poly(&dkey("gill_l", BodyRegion::FrontLeftFender, M::Trim), sx,
    //     &[[0.9, 0.45], [1.1, 0.48], [1.1, 0.62], [0.95, 0.60]], 0.004, 0.01);
    // k.face_poly(&dkey("badge", BodyRegion::Hood, M::Chrome), true,
    //     &[[-0.04, 0.55], [0.04, 0.55], [0.04, 0.59], [-0.04, 0.59]], 0.006);
    // ```

    /// Flat prism on the nose (`front`) or tail face plane: convex outline `(x, z)` swept from
    /// 2 cm inside the face to `proud` out of it (lamp lenses, grilles, badges on a flat face;
    /// for a curved or raked face use [`Kit::end_poly`]).
    pub fn face_poly(&mut self, key: &PartKey, front: bool, outline: &[[f32; 2]], proud: f32) {
        let (y0, y1) = if front {
            (self.yn - 0.02, self.yn + proud)
        } else {
            (self.yt + 0.02, self.yt - proud)
        };
        self.prism_y(key, outline, y0, y1);
    }

    /// Panel lying on the top surface (hood, fender tops, deck, roof): outline `pts` in chassis
    /// `(x, y)` (any simple polygon), its face `lift` m above the paint and `thick` m deep.
    pub fn top_poly(&mut self, key: &PartKey, pts: &[[f32; 2]], lift: f32, thick: f32) {
        crate::lamp::panel(
            self.b,
            self.p,
            key,
            crate::lamp::LampView::Top,
            pts,
            lift,
            lift - thick,
        );
    }

    /// Panel on the body side of side `sx` (`key_left` is the left-side key; the right one is
    /// derived): outline `pts` in `(y, z)`, its face `off` m out of the paint, `thick` m deep.
    /// Follows sculpts, bulges and arches (and the greenhouse above the belt).
    pub fn side_poly(
        &mut self,
        key_left: &PartKey,
        sx: f32,
        pts: &[[f32; 2]],
        off: f32,
        thick: f32,
    ) {
        let key = sided(key_left, sx);
        let view = crate::lamp::LampView::Side { right: sx > 0.0 };
        crate::lamp::panel(self.b, self.p, &key, view, pts, off, off - thick);
    }

    /// Panel on the nose (`front`) or tail face: outline `pts` in `(x, z)` (signed x), its face
    /// `proud` m out of the surface and `depth` m deep, following the plan-rounded corners and a
    /// raked / bowed face.
    pub fn end_poly(
        &mut self,
        key: &PartKey,
        front: bool,
        pts: &[[f32; 2]],
        proud: f32,
        depth: f32,
    ) {
        let view = if front {
            crate::lamp::LampView::Front
        } else {
            crate::lamp::LampView::Rear
        };
        crate::lamp::panel(self.b, self.p, key, view, pts, proud, proud - depth);
    }

    /// Head lamp of side `sx` lying on the nose top and sweeping back into the fender, drawn in
    /// the top view and projected ([`Kit::proj_cluster`]): the lens runs across |x| `xin..xout`
    /// at its front edge `y1`; its inboard edge goes back to `y0`, its outboard edge `sweep` m
    /// further (a swept, tapering lamp). The outboard `sig_w` is the amber turn signal; a flush
    /// dark bezel `border` m wide rings it.
    pub fn swept_top_lamp(
        &mut self,
        sx: f32,
        y0: f32,
        y1: f32,
        xin: f32,
        xout: f32,
        sweep: f32,
        border: f32,
        sig_w: f32,
    ) {
        let y1 = y1.min(self.yn - 0.01);
        // Keep a real lens when a narrow nose pulls the outer edge in.
        let xin = xin.min(xout - 0.15);
        let sig_w = sig_w.min(0.4 * (xout - xin));
        let outline = [[xin, y1], [xout, y1], [xout - 0.03, y0 - sweep], [xin, y0]];
        self.proj_cluster(
            sx,
            true,
            crate::lamp::LampView::Top,
            &outline,
            Some(Seg::Outboard(sig_w)),
            M::EmissiveAmber,
            border,
            0.0,
        );
    }

    /// Bounding box (lo, hi) of part `name` as built so far, or `None` if it is empty.
    fn part_box(&self, name: &str) -> Option<([f32; 3], [f32; 3])> {
        let (pos, _, _) = self.b.geometry(name)?;
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for q in pos {
            for k in 0..3 {
                lo[k] = lo[k].min(q[k]);
                hi[k] = hi[k].max(q[k]);
            }
        }
        (!pos.is_empty()).then_some((lo, hi))
    }

    /// Turn signals for kits whose own lamp shapes do not carve one (call after the kit has
    /// built its head and tail lamps): an amber lens `w` wide on the nose / tail face, flush
    /// against the outboard end of each lamp's box (a strip under its outboard end when the lamp
    /// already reaches the corner), over the lamp's height clamped to the face. Signals the kit already built are
    /// left alone. Rear segments use `rear_mat`
    /// (`EmissiveAmber`, or `Taillight` for red-on-red).
    pub fn signals_beside_lamps(&mut self, w: f32, rear_mat: MaterialSlot) {
        for sx in SIDES {
            for front in [true, false] {
                let lamp = if front {
                    headlight_key(sx)
                } else {
                    taillight_key(sx)
                };
                let sig = if front {
                    indicator_front_key(sx)
                } else {
                    indicator_rear_key(sx, rear_mat)
                };
                // The kit built this one itself (a segment of its own lamp shape).
                if self.part_box(&sig.name).is_some() {
                    continue;
                }
                let Some((lo, hi)) = self.part_box(&lamp.name) else {
                    continue;
                };
                let outer = lo[0].abs().max(hi[0].abs());
                let (half, (fz0, fz1)) = if front {
                    (self.wn(), self.nose_z())
                } else {
                    (self.wt(), self.tail_z())
                };
                // Lamps on the nose top (set back, or above the nose face): an amber strip on
                // the top surface right inboard of the lamp, over its length (never in front
                // of it, where it would shadow the lens).
                if front && (hi[1] < self.yn - 0.04 || lo[2] > fz1 - 0.02) {
                    let inner = lo[0].abs().min(hi[0].abs());
                    self.patch(
                        &sig,
                        lo[1],
                        hi[1].min(self.yn - 0.005),
                        top_band(
                            sx * (inner - INDICATOR_SEAM_M - w),
                            sx * (inner - INDICATOR_SEAM_M),
                        ),
                        0.003,
                        0.012,
                        (2, 1),
                    );
                    continue;
                }
                // Lamps on the nose top sit above the face: use the face's top strip instead.
                let z1 = hi[2].min(fz1 - 0.01);
                let z0 = lo[2].max(fz0 + 0.02).min(z1 - 0.03);
                let (x0, x1, z0, z1) = if outer + INDICATOR_SEAM_M + w <= half - 0.01 {
                    (
                        outer + INDICATOR_SEAM_M,
                        outer + INDICATOR_SEAM_M + w,
                        z0,
                        z1,
                    )
                } else {
                    // Lamp runs to the corner (light bars): a strip under its outboard end.
                    let zt = lo[2] - INDICATOR_SEAM_M;
                    (
                        outer.min(half - 0.01) - 2.0 * w,
                        outer.min(half - 0.01),
                        zt - 0.035,
                        zt,
                    )
                };
                if front {
                    self.front_box(&sig, sx * x0, sx * x1, z0, z1, 0.012, 0.004);
                } else {
                    self.rear_box(&sig, sx * x0, sx * x1, z0, z1, 0.012, 0.004);
                }
            }
        }
    }

    /// Tail-face lamp lens (as [`Kit::rear_box`]) with the rear turn signal cut out of it:
    /// the tail lamp and a segment `seg` of the same lens in `mat` (`EmissiveAmber`, or
    /// `Taillight` for red-on-red).
    #[allow(clippy::too_many_arguments)]
    pub fn rear_cluster(
        &mut self,
        sx: f32,
        xin: f32,
        xout: f32,
        z0: f32,
        z1: f32,
        proud: f32,
        bevel: f32,
        seg: Seg,
        mat: MaterialSlot,
    ) {
        let (lamp, sig) = seg.split(xin, xout, z0, z1);
        let tk = taillight_key(sx);
        self.rear_box(
            &tk,
            sx * lamp[0],
            sx * lamp[1],
            lamp[2],
            lamp[3],
            proud,
            bevel,
        );
        let ik = indicator_rear_key(sx, mat);
        self.rear_box(&ik, sx * sig[0], sx * sig[1], sig[2], sig[3], proud, bevel);
    }

    /// Flush side repeaters (`indicator_side_l/_r`) on both front fenders: a small amber lens
    /// 1 mm off the fender surface (following it), just behind the front wheel arch near the
    /// arch-top height. Bound to the fender like any fender part.
    pub fn fender_repeaters(&mut self) {
        let (yf, zf, rf) = self.arch(true);
        let y1 = (yf - rf - 0.05).max(self.p.y_fd_hinge + 0.12);
        let y0 = y1 - 0.075;
        let zc = (zf + 0.85 * rf).min(self.p.belt(y1) - 0.06);
        let kl = mesh::indicator_side_key(false, BodyRegion::FrontLeftFender);
        for sx in SIDES {
            self.side_patch(&kl, sx, y0, y1, zc - 0.02, zc + 0.02, 0.001, 0.004, (1, 1));
        }
    }

    /// Flush side repeaters (`indicator_side_l/_r`) on the door mirrors of
    /// [`Kit::mirrors`]`(y_off, size, cap)`: an amber strip over the outer half of the
    /// housing's front-lower chamfer, 1.5 mm proud of it (of the cap when `cap`). Region and
    /// binding as the mirror.
    pub fn mirror_repeaters(&mut self, y_off: f32, size: f32, cap: bool) {
        for sx in SIDES {
            let m = self.mirror_frame(sx, y_off, size);
            let k = sided(&mesh::indicator_side_key(false, BodyRegion::LeftDoorF), sx);
            // Chamfer edge a → c of the housing profile and its outward (forward-down) normal.
            let a = [m.ylo + 0.45 * size, m.zl - 0.005];
            let c = [m.ylo + 0.8 * size, m.zl + 0.2 * size];
            let (dy, dz) = (c[0] - a[0], c[1] - a[1]);
            let l = (dy * dy + dz * dz).sqrt().max(1e-6);
            let n = [dz / l, -dy / l];
            let off = |q: [f32; 2], d: f32| [q[0] + n[0] * d, q[1] + n[1] * d];
            let out = if cap { 0.0045 } else { 0.0015 };
            let prof = [off(a, out), off(c, out), off(c, -0.004), off(a, -0.004)];
            let xo = m.x1 - sx * 0.008;
            let xi = m.x1 - sx * 0.55 * (m.x1 - m.x0).abs();
            self.prism_x(&k, &prof, xi, xo);
        }
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
    /// stalk, optional contrast cap (`cap`), `y_off` back from 5 cm ahead of the sail front
    /// (`BodyProfile::sail`, the A-pillar foot).
    pub fn mirrors(&mut self, y_off: f32, size: f32, cap: Option<MaterialSlot>) {
        for sx in SIDES {
            let MirrorFrame {
                zb,
                w,
                x0,
                x1,
                zl,
                zh,
                ylo,
            } = self.mirror_frame(sx, y_off, size);
            let k = mirror_key(sx);
            // Housing: a wedge, blunt at the back (mirror glass), rounded front.
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

    /// Placement of the door mirror of side `sx` (see [`Kit::mirrors`]).
    fn mirror_frame(&self, sx: f32, y_off: f32, size: f32) -> MirrorFrame {
        // `y_off` back from the front of the sail at the A-pillar foot (+5 cm).
        let yh = self.p.sail().1 + 0.05 - y_off;
        let zb = self.p.belt(yh) + 0.02;
        let w = self.side_x(yh, zb - 0.06).max(self.p.half_width(yh) - 0.05);
        let zl = zb + 0.03;
        MirrorFrame {
            zb,
            w,
            x0: sx * (w + 0.05),
            x1: sx * (w + 0.05 + 1.4 * size),
            zl,
            zh: zl + 0.75 * size,
            ylo: yh - 0.75 * size,
        }
    }

    /// Door handles (front doors; rear doors too on 4-door cars), `mat`.
    pub fn door_handles(&mut self, mat: MaterialSlot) {
        crate::trim::remove_generic(self.b, &crate::trim::GENERIC_HANDLES);
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
        crate::trim::remove_generic(self.b, &crate::trim::GENERIC_FUEL_DOOR);
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
