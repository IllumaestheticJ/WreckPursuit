//! Small geometry helpers shared by the wheel and mechanical generators, built on
//! [`Builder`] primitives. Everything is deterministic (`builder::turn`, `wp_math::sin_cos`).
//!
//! Hub-space helpers take the wheel's outboard sign `s` and work in `(r, u)` coordinates:
//! `r` = distance from the axle, `u` = distance along the axle **toward the outboard face**
//! (hub-space `x = s · u`). The clock angle `θ` follows `research_mechanical.md` §0
//! (0° forward = +Y, 90° up = +Z).

use crate::builder::{Builder, PartKey};

/// `a + b`.
pub fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// `a − b`.
pub fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

/// `a · k`.
pub fn mul(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

/// Linear interpolation between two points.
pub fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    add(a, mul(sub(b, a), t))
}

/// Dot product.
pub fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// Cross product.
pub fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// Unit vector (or +Z for a zero vector).
pub fn unit(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt();
    if l < 1e-9 {
        [0.0, 0.0, 1.0]
    } else {
        mul(a, 1.0 / l)
    }
}

/// Mean of points.
pub fn centroid(pts: &[[f32; 3]]) -> [f32; 3] {
    let mut c = [0.0; 3];
    for &q in pts {
        c = add(c, q);
    }
    mul(c, 1.0 / pts.len().max(1) as f32)
}

/// `(sin, cos)` of an angle in degrees.
pub fn sc_deg(deg: f32) -> (f32, f32) {
    wp_math::sin_cos(deg.to_radians())
}

/// Hub-space point at clock angle `deg`, radius `r`, outboard distance `u`.
pub fn hub_pt(s: f32, deg: f32, r: f32, u: f32) -> [f32; 3] {
    let (sn, cs) = sc_deg(deg);
    [s * u, r * cs, r * sn]
}

/// Convex hexahedron from 8 corners: `c[0..4]` one end loop, `c[4..8]` the other, `c[i]`
/// joined to `c[i + 4]`. Faces are oriented away from the centroid (12 tris).
pub fn hexa(b: &mut Builder, key: &PartKey, c: [[f32; 3]; 8]) {
    let mid = centroid(&c);
    let faces = [
        [0, 1, 2, 3],
        [4, 5, 6, 7],
        [0, 1, 5, 4],
        [1, 2, 6, 5],
        [2, 3, 7, 6],
        [3, 0, 4, 7],
    ];
    for f in faces {
        let q = [c[f[0]], c[f[1]], c[f[2]], c[f[3]]];
        let out = sub(centroid(&q), mid);
        b.poly_facing(key, &q, out);
    }
}

/// Lathe about the hub axle in `(r, u)` coordinates. Faces point "left" of the profile
/// direction in the `(u, r)` plane: a profile running toward +u faces away from the axle,
/// one running outward in r faces inboard (−u).
pub fn hub_lathe(b: &mut Builder, key: &PartKey, s: f32, profile: &[[f32; 2]], n: usize) {
    // Builder::lathe faces outward when the profile runs in +axis; with axis = s·X that is +u.
    b.lathe(key, [0.0; 3], [s, 0.0, 0.0], profile, n);
}

/// Flat polygon in hub space facing outboard (+u) or inboard (`facing_out = false`).
pub fn hub_poly(b: &mut Builder, key: &PartKey, s: f32, pts: &[[f32; 3]], facing_out: bool) {
    let n = if facing_out {
        [s, 0.0, 0.0]
    } else {
        [-s, 0.0, 0.0]
    };
    b.poly_facing(key, pts, n);
}

/// Sweep a closed `(r, u)` section along the hub circle from `deg0` to `deg1` in `segs`
/// steps (an arc-shaped solid: calipers, fins). Faces point away from the section centre.
#[allow(clippy::too_many_arguments)]
pub fn arc_sweep(
    b: &mut Builder,
    key: &PartKey,
    s: f32,
    section: &[[f32; 2]],
    deg0: f32,
    deg1: f32,
    segs: usize,
    caps: bool,
) {
    let segs = segs.max(1);
    let m = section.len();
    let (rc, uc) = {
        let (mut a, mut c) = (0.0, 0.0);
        for q in section {
            a += q[0];
            c += q[1];
        }
        (a / m as f32, c / m as f32)
    };
    let ring = |k: usize| -> Vec<[f32; 3]> {
        let d = deg0 + (deg1 - deg0) * k as f32 / segs as f32;
        section.iter().map(|q| hub_pt(s, d, q[0], q[1])).collect()
    };
    let rings: Vec<Vec<[f32; 3]>> = (0..=segs).map(ring).collect();
    for k in 0..segs {
        let dm = deg0 + (deg1 - deg0) * (k as f32 + 0.5) / segs as f32;
        let axis_pt = hub_pt(s, dm, rc, uc);
        for i in 0..m {
            let j = (i + 1) % m;
            let q = [rings[k][i], rings[k][j], rings[k + 1][j], rings[k + 1][i]];
            let out = sub(centroid(&q), axis_pt);
            b.poly_facing(key, &q, out);
        }
    }
    if caps {
        for (k, d) in [(0, deg0), (segs, deg1)] {
            let other = if k == 0 { deg0 + 1.0 } else { deg1 - 1.0 };
            let dir = sub(hub_pt(s, d, rc, uc), hub_pt(s, other, rc, uc));
            b.poly_facing(key, &rings[k], dir);
        }
    }
}

/// Up vector least aligned with the polyline's segments (for sweeping tubes).
fn tube_up(path: &[[f32; 3]]) -> [f32; 3] {
    let axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let mut best = axes[2];
    let mut best_v = f32::MAX;
    for a in axes {
        let worst = path
            .windows(2)
            .map(|w| dot(unit(sub(w[1], w[0])), a).abs())
            .fold(0.0, f32::max);
        if worst < best_v {
            best_v = worst;
            best = a;
        }
    }
    best
}

/// Round tube of radius `r` with `sides` sides swept along `path` (no caps unless asked).
pub fn tube(b: &mut Builder, key: &PartKey, path: &[[f32; 3]], r: f32, sides: usize, caps: bool) {
    let sides = sides.max(3);
    let prof: Vec<[f32; 2]> = (0..sides)
        .map(|i| {
            let (sn, cs) = crate::builder::turn(i, sides);
            [cs * r, sn * r]
        })
        .collect();
    b.extrude(key, &prof, path, tube_up(path), caps);
}

/// Straight rod from `a` to `b` (`sides`-sided, open ends).
pub fn rod(bld: &mut Builder, key: &PartKey, a: [f32; 3], b: [f32; 3], r: f32, sides: usize) {
    bld.cylinder(key, a, b, r, r, sides.max(3), false);
}

/// Box between two points with a square-ish section `w × h` (`h` along `up`).
pub fn bar(
    bld: &mut Builder,
    key: &PartKey,
    a: [f32; 3],
    b: [f32; 3],
    w: f32,
    h: f32,
    up: [f32; 3],
) {
    let t = unit(sub(b, a));
    let side = unit(cross(t, up));
    let upv = cross(side, t);
    let len = dot(sub(b, a), t);
    bld.oriented_box(
        key,
        mix(a, b, 0.5),
        [side, t, upv],
        [0.5 * w, 0.5 * len, 0.5 * h],
    );
}

/// Coil spring: a helix of `coils` turns between `a` (bottom) and `b` (top), mean radius
/// `mean_r`, wire radius `wire_r`, swept with a `sides`-gon (3 = cheapest that reads).
#[allow(clippy::too_many_arguments)]
pub fn helix(
    bld: &mut Builder,
    key: &PartKey,
    a: [f32; 3],
    b: [f32; 3],
    mean_r: f32,
    wire_r: f32,
    coils: f32,
    segs_per_coil: usize,
    sides: usize,
) {
    let ax = unit(sub(b, a));
    let (u, v) = crate::builder::basis(ax);
    let len = dot(sub(b, a), ax);
    let n = ((coils * segs_per_coil as f32).ceil() as usize).max(2);
    let path: Vec<[f32; 3]> = (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let ang = std::f32::consts::TAU * coils * t;
            let (sn, cs) = wp_math::sin_cos(ang);
            add(
                add(a, mul(ax, len * t)),
                add(mul(u, cs * mean_r), mul(v, sn * mean_r)),
            )
        })
        .collect();
    let sides = sides.max(3);
    let prof: Vec<[f32; 2]> = (0..sides)
        .map(|i| {
            let (sn, cs) = crate::builder::turn(i, sides);
            [cs * wire_r, sn * wire_r]
        })
        .collect();
    bld.extrude(key, &prof, &path, ax, false);
}

/// A placed local frame with per-axis scale (engines):
/// `pt(l) = o + ax[0]·l.x·k[0] + ax[1]·l.y·k[1] + ax[2]·l.z·k[2]`.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    /// Origin, chassis space.
    pub o: [f32; 3],
    /// Local axes in chassis space (right-handed, unit).
    pub ax: [[f32; 3]; 3],
    /// Scale along each local axis.
    pub k: [f32; 3],
}

impl Frame {
    /// Unscaled frame.
    pub fn new(o: [f32; 3], ax: [[f32; 3]; 3]) -> Frame {
        Frame { o, ax, k: [1.0; 3] }
    }

    /// Chassis vector of local vector `d` (scaled, not normalised).
    pub fn lin(&self, d: [f32; 3]) -> [f32; 3] {
        add(
            add(
                mul(self.ax[0], d[0] * self.k[0]),
                mul(self.ax[1], d[1] * self.k[1]),
            ),
            mul(self.ax[2], d[2] * self.k[2]),
        )
    }

    /// Chassis point of local `l`.
    pub fn pt(&self, l: [f32; 3]) -> [f32; 3] {
        add(self.o, self.lin(l))
    }

    /// Mean radial scale (for round things).
    pub fn kr(&self) -> f32 {
        (self.k[0] + self.k[1] + self.k[2]) / 3.0
    }

    /// Box at local centre `c` with local half extents `h`, rolled `roll_deg` about local Y
    /// (positive = top toward +x).
    pub fn bx(&self, b: &mut Builder, key: &PartKey, c: [f32; 3], h: [f32; 3], roll_deg: f32) {
        let (sn, cs) = sc_deg(roll_deg);
        let ax = self.lin([cs, 0.0, -sn]);
        let az = self.lin([sn, 0.0, cs]);
        let ay = self.lin([0.0, 1.0, 0.0]);
        b.oriented_box(key, self.pt(c), [ax, ay, az], h);
    }

    /// Tapered box (frustum) between two local end rectangles along local Y: rear end at
    /// `y0` with half sizes `(hx0, hz0)`, front end at `y1` with `(hx1, hz1)`, centred on
    /// local `(x, z)`.
    #[allow(clippy::too_many_arguments)]
    pub fn taper(
        &self,
        b: &mut Builder,
        key: &PartKey,
        x: f32,
        z: f32,
        y0: f32,
        y1: f32,
        h0: [f32; 2],
        h1: [f32; 2],
    ) {
        let c = |y: f32, h: [f32; 2], i: usize| {
            let sx = if i == 1 || i == 2 { h[0] } else { -h[0] };
            let sz = if i >= 2 { h[1] } else { -h[1] };
            self.pt([x + sx, y, z + sz])
        };
        hexa(
            b,
            key,
            [
                c(y0, h0, 0),
                c(y0, h0, 1),
                c(y0, h0, 2),
                c(y0, h0, 3),
                c(y1, h1, 0),
                c(y1, h1, 1),
                c(y1, h1, 2),
                c(y1, h1, 3),
            ],
        );
    }

    /// Cylinder between local points.
    #[allow(clippy::too_many_arguments)]
    pub fn cyl(
        &self,
        b: &mut Builder,
        key: &PartKey,
        a: [f32; 3],
        e: [f32; 3],
        r0: f32,
        r1: f32,
        sides: usize,
        caps: bool,
    ) {
        let k = self.kr();
        b.cylinder(key, self.pt(a), self.pt(e), r0 * k, r1 * k, sides, caps);
    }

    /// Lathe about local axis `axis` through local `c`; profile `(radius, along)`.
    pub fn lathe(
        &self,
        b: &mut Builder,
        key: &PartKey,
        c: [f32; 3],
        axis: [f32; 3],
        profile: &[[f32; 2]],
        n: usize,
    ) {
        let along = self.lin(axis);
        let ka = dot(along, along).sqrt();
        let k = self.kr();
        let prof: Vec<[f32; 2]> = profile.iter().map(|q| [q[0] * k, q[1] * ka]).collect();
        b.lathe(key, self.pt(c), unit(along), &prof, n);
    }

    /// Tube along local points.
    pub fn tube(&self, b: &mut Builder, key: &PartKey, path: &[[f32; 3]], r: f32, sides: usize) {
        let p: Vec<[f32; 3]> = path.iter().map(|&q| self.pt(q)).collect();
        tube(b, key, &p, r * self.kr(), sides, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::{MaterialSlot, PartKind};
    use wp_contracts::BodyRegion;

    fn volume(b: &Builder, name: &str) -> f32 {
        let (pos, _, idx) = b.geometry(name).expect("part");
        idx.chunks(3)
            .map(|t| {
                let (a, bb, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
                dot(a, cross(bb, c)) / 6.0
            })
            .sum()
    }

    fn key(n: &str) -> PartKey {
        PartKey::body(
            n,
            BodyRegion::FrameFront,
            PartKind::Engine,
            MaterialSlot::Engine,
        )
    }

    #[test]
    fn closed_helpers_face_outward() {
        let mut b = Builder::new();
        let c = [
            [0.0, 0.0, 0.0],
            [0.2, 0.0, 0.0],
            [0.2, 0.0, 0.1],
            [0.0, 0.0, 0.1],
            [0.0, 0.3, 0.0],
            [0.2, 0.3, 0.0],
            [0.2, 0.3, 0.1],
            [0.0, 0.3, 0.1],
        ];
        hexa(&mut b, &key("hexa"), c);
        for s in [-1.0, 1.0] {
            let name = format!("arc{}", if s < 0.0 { "l" } else { "r" });
            arc_sweep(
                &mut b,
                &key(&name),
                s,
                &[[0.15, -0.02], [0.2, -0.02], [0.2, 0.03], [0.15, 0.03]],
                150.0,
                200.0,
                4,
                true,
            );
            assert!(volume(&b, &name) > 0.0, "{name} inward");
            let lname = format!("lathe{}", if s < 0.0 { "l" } else { "r" });
            hub_lathe(
                &mut b,
                &key(&lname),
                s,
                &[
                    [0.1, -0.05],
                    [0.2, -0.05],
                    [0.2, 0.05],
                    [0.1, 0.05],
                    [0.1, -0.05],
                ],
                16,
            );
            assert!(volume(&b, &lname) > 0.0, "{lname} inward");
        }
        let f = Frame {
            o: [0.0, 1.0, 0.4],
            ax: [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            k: [1.0, 1.0, 0.8],
        };
        f.bx(
            &mut b,
            &key("fbox"),
            [0.1, 0.0, 0.1],
            [0.1, 0.2, 0.05],
            30.0,
        );
        f.taper(
            &mut b,
            &key("ftaper"),
            0.0,
            0.0,
            -0.3,
            0.1,
            [0.1, 0.1],
            [0.05, 0.08],
        );
        for n in ["hexa", "fbox", "ftaper"] {
            assert!(volume(&b, n) > 0.0, "{n} inward");
        }
    }
}
