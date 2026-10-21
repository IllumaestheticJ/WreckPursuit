//! Small placement kit shared by the parking and construction modules: a site-local frame
//! (origin at a gate, +Y into the site), perimeter fences with exact gate openings, road
//! checks and "emit unless it lands on road paving".

use super::super::ctx::{box_on, footprint, prism, ramp, Ctx, Occ};
use super::super::plan::{Gate, Site};
use crate::geom::{self, V2, V3};
use crate::network::{Feature, FeatureKind, Material};

/// A right-handed local frame on the ground plane: `x` to the right, `y` forward.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    /// Origin (world), m.
    pub o: V2,
    /// Forward unit vector (local +Y).
    pub f: V2,
    /// Right unit vector (local +X).
    pub r: V2,
    /// Yaw of local +Y (0 = north, CCW), rad.
    pub yaw: f32,
}

impl Frame {
    /// Frame at `o` whose +Y points along `yaw`.
    pub fn new(o: V2, yaw: f32) -> Self {
        let (s, c) = wp_math::sin_cos(yaw);
        Self {
            o,
            f: [-s, c],
            r: [c, s],
            yaw,
        }
    }

    /// The frame of a site gate (+Y = into the site).
    pub fn of_gate(g: &Gate) -> Self {
        Self::new(g.pos, g.heading)
    }

    /// The same orientation moved to local point `(x, y)`.
    pub fn shifted(&self, x: f32, y: f32) -> Self {
        Self::new(self.p(x, y), self.yaw)
    }

    /// World point of local `(x, y)`.
    pub fn p(&self, x: f32, y: f32) -> V2 {
        [
            self.o[0] + self.r[0] * x + self.f[0] * y,
            self.o[1] + self.r[1] * x + self.f[1] * y,
        ]
    }

    /// World 3D point of local `(x, y)` at height `z`.
    pub fn p3(&self, x: f32, y: f32, z: f32) -> V3 {
        let q = self.p(x, y);
        [q[0], q[1], z]
    }

    /// Local coordinates of world point `p`.
    pub fn local(&self, p: V2) -> V2 {
        let d = geom::sub(p, self.o);
        [geom::dot(d, self.r), geom::dot(d, self.f)]
    }

    /// World polygon (CCW) of the local rectangle `[x0, x1] × [y0, y1]`.
    pub fn rect(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<V2> {
        vec![
            self.p(x0, y0),
            self.p(x1, y0),
            self.p(x1, y1),
            self.p(x0, y1),
        ]
    }

    /// Local bounds of a world polygon: `(min, max)`.
    pub fn bounds_of(&self, poly: &[V2]) -> (V2, V2) {
        let mut lo = [f32::MAX; 2];
        let mut hi = [f32::MIN; 2];
        for p in poly {
            let q = self.local(*p);
            lo = [lo[0].min(q[0]), lo[1].min(q[1])];
            hi = [hi[0].max(q[0]), hi[1].max(q[1])];
        }
        (lo, hi)
    }

    /// Box standing on `z0` centred at local `(x, y)`, size `[along x, along y, up]`,
    /// turned by `local_yaw` from the frame.
    #[allow(clippy::too_many_arguments)]
    pub fn bx(
        &self,
        kind: FeatureKind,
        material: Material,
        x: f32,
        y: f32,
        z0: f32,
        size: V3,
        local_yaw: f32,
        solid: bool,
    ) -> Feature {
        box_on(
            kind,
            material,
            self.p(x, y),
            z0,
            size,
            self.yaw + local_yaw,
            solid,
        )
    }

    /// Axis-aligned (in this frame) box spanning `[x0, x1] × [y0, y1] × [z0, z1]`.
    #[allow(clippy::too_many_arguments)]
    pub fn span(
        &self,
        kind: FeatureKind,
        material: Material,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        z0: f32,
        z1: f32,
        solid: bool,
    ) -> Feature {
        self.bx(
            kind,
            material,
            (x0 + x1) * 0.5,
            (y0 + y1) * 0.5,
            z0,
            [(x1 - x0).abs(), (y1 - y0).abs(), z1 - z0],
            0.0,
            solid,
        )
    }

    /// Solid prism over the local rectangle `[x0, x1] × [y0, y1]`.
    #[allow(clippy::too_many_arguments)]
    pub fn prism_rect(
        &self,
        kind: FeatureKind,
        material: Material,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
        z0: f32,
        z1: f32,
        solid: bool,
    ) -> Feature {
        prism(kind, material, self.rect(x0, y0, x1, y1), z0, z1, solid)
    }

    /// Sloped slab between local 3D points (top centreline), see [`ramp`].
    #[allow(clippy::too_many_arguments)]
    pub fn ramp(
        &self,
        kind: FeatureKind,
        material: Material,
        a: V3,
        b: V3,
        width: f32,
        thick: f32,
    ) -> Feature {
        ramp(
            kind,
            material,
            self.p3(a[0], a[1], a[2]),
            self.p3(b[0], b[1], b[2]),
            width,
            thick,
        )
    }
}

/// Distance from `p` to segment `a`–`b` and the parameter of the closest point.
fn seg_param(a: V2, b: V2, p: V2) -> (f32, f32) {
    let ab = geom::sub(b, a);
    let l2 = geom::dot(ab, ab);
    let t = if l2 > 0.0 {
        (geom::dot(geom::sub(p, a), ab) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (geom::dist(p, [a[0] + ab[0] * t, a[1] + ab[1] * t]), t)
}

/// Perimeter pieces (fence, wall, hedge, hoarding) along a closed polygon, following the
/// ground, with an exact opening of each gate's width wherever a gate lies within 8 m of
/// an edge. Pieces are at most `max_piece_m` long; pieces that would touch road paving
/// are left out.
#[allow(clippy::too_many_arguments)]
pub fn perimeter(
    ctx: &Ctx,
    kind: FeatureKind,
    material: Material,
    poly: &[V2],
    thick: f32,
    height: f32,
    max_piece_m: f32,
    gates: &[Gate],
) -> Vec<Feature> {
    let mut out = Vec::new();
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let len = geom::dist(a, b);
        if len < 0.1 {
            continue;
        }
        // Openings along this edge, in metres from `a`.
        let mut cuts: Vec<(f32, f32)> = Vec::new();
        for g in gates {
            let (d, t) = seg_param(a, b, g.pos);
            if d < 8.0 {
                let s = t * len;
                cuts.push((s - g.width_m * 0.5, s + g.width_m * 0.5));
            }
        }
        cuts.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut spans = Vec::new();
        let mut s0 = 0.0;
        for (c0, c1) in cuts {
            if c0 > s0 {
                spans.push((s0, c0.min(len)));
            }
            s0 = s0.max(c1);
        }
        if s0 < len {
            spans.push((s0, len));
        }
        for (s0, s1) in spans {
            if s1 - s0 < 0.3 {
                continue;
            }
            let pieces = ((s1 - s0) / max_piece_m).ceil().max(1.0) as usize;
            for k in 0..pieces {
                let t0 = (s0 + (s1 - s0) * k as f32 / pieces as f32) / len;
                let t1 = (s0 + (s1 - s0) * (k + 1) as f32 / pieces as f32) / len;
                let p = super::super::ctx::lerp2(a, b, t0);
                let q = super::super::ctx::lerp2(a, b, t1);
                let m = super::super::ctx::lerp2(p, q, 0.5);
                let zs = [ctx.ground_z(p), ctx.ground_z(q), ctx.ground_z(m)];
                let lo = zs.iter().copied().fold(f32::MAX, f32::min);
                let hi = zs.iter().copied().fold(f32::MIN, f32::max);
                // Pieces meet end to end (no overlap past the gate opening).
                let f = box_on(
                    kind,
                    material,
                    m,
                    lo - 0.3,
                    [thick, geom::dist(p, q), hi - lo + 0.3 + height],
                    super::super::ctx::yaw_of(p, q),
                    true,
                );
                if !on_road(ctx, &f) {
                    out.push(f);
                }
            }
        }
    }
    out
}

/// True when any corner, edge midpoint or the centre of the feature's footprint is on road
/// paving.
pub fn on_road(ctx: &Ctx, f: &Feature) -> bool {
    let fp = footprint(&f.shape);
    let n = fp.len();
    let mut c = [0.0f32; 2];
    for (i, p) in fp.iter().enumerate() {
        if ctx.occ_at(*p) == Occ::Road {
            return true;
        }
        let q = fp[(i + 1) % n];
        if ctx.occ_at(super::super::ctx::lerp2(*p, q, 0.5)) == Occ::Road {
            return true;
        }
        c = [c[0] + p[0] / n as f32, c[1] + p[1] / n as f32];
    }
    ctx.occ_at(c) == Occ::Road
}

/// Collects a module's features, dropping any that would land on road paving.
#[derive(Default)]
pub struct Sink {
    /// Features to `ctx.place` (mark the occupancy grid).
    pub placed: Vec<Feature>,
    /// Features to `ctx.add` (small items: paint, parked cars, clutter).
    pub added: Vec<Feature>,
}

impl Sink {
    /// Queue a feature that marks occupancy.
    pub fn place(&mut self, f: Feature) {
        self.placed.push(f);
    }

    /// Queue a small feature.
    pub fn add(&mut self, f: Feature) {
        self.added.push(f);
    }

    /// Queue many features that mark occupancy.
    pub fn place_all(&mut self, fs: impl IntoIterator<Item = Feature>) {
        self.placed.extend(fs);
    }

    /// Hand everything to the context (placed first, then added), skipping features on
    /// road paving. Returns how many were dropped.
    pub fn flush(self, ctx: &mut Ctx) -> usize {
        let mut dropped = 0;
        for f in self.placed {
            if on_road(ctx, &f) {
                dropped += 1;
            } else {
                ctx.place(f);
            }
        }
        for f in self.added {
            if on_road(ctx, &f) {
                dropped += 1;
            } else {
                ctx.add(f);
            }
        }
        dropped
    }
}

/// Height of an at-grade road surface near `p` (within `r` of a paved ribbon and within
/// 1.5 m of the ground there; bridges overhead are ignored), if any.
pub fn street_z(ctx: &Ctx, p: V2, r: f32) -> Option<f32> {
    let g = ctx.ground_z(p);
    let mut best: Option<(f32, f32)> = None;
    for s in &ctx.roads.built.surfaces {
        let half = s.left_m.max(s.right_m);
        for w in s.centre.windows(2) {
            let (d, t) = seg_param(geom::xy(w[0]), geom::xy(w[1]), p);
            let gap = (d - half).max(0.0);
            let z = w[0][2] + (w[1][2] - w[0][2]) * t;
            if gap <= r && (z - g).abs() < 1.5 && best.is_none_or(|b| gap < b.0) {
                best = Some((gap, z));
            }
        }
    }
    best.map(|b| b.1)
}

/// Height a driveway should start at for a gate: the street surface if a road reaches
/// the gate, else the ground.
pub fn gate_z(ctx: &Ctx, g: &Gate) -> f32 {
    street_z(ctx, g.pos, 3.0).unwrap_or_else(|| ctx.ground_z(g.pos))
}

/// Ground earthwork for a site the plan leaves natural: level the site to the natural
/// ground height at its first gate.
pub fn site_pad(
    site: &Site,
    nat: &super::super::ground::Natural,
) -> Option<super::super::ground::Earthwork> {
    if site.pad_z.is_some() {
        return None;
    }
    let g = site.gates.first()?;
    Some(super::super::ground::Earthwork::Pad {
        polygon: site.polygon.clone(),
        z: nat.z(g.pos).max(super::super::plan::MIN_LAND_Z),
        blend_m: 50.0,
    })
}

/// Regular polygon (CCW) around `c`.
pub fn ngon(c: V2, r: f32, n: usize, phase: f32) -> Vec<V2> {
    (0..n)
        .map(|k| {
            let a = phase + k as f32 * core::f32::consts::TAU / n as f32;
            let (s, co) = wp_math::sin_cos(a);
            [c[0] + r * co, c[1] + r * s]
        })
        .collect()
}

/// Make a feature non-solid (decor that should not catch cars).
pub fn ghost(mut f: Feature) -> Feature {
    f.solid = false;
    f
}
