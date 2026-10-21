//! Part builder and detail primitives.
//!
//! A [`Builder`] collects triangles per part ([`PartKey`]: name, region, kind, material,
//! wheel, rigid). Primitives (`cuboid`, `beveled_box`, `cylinder`, `torus`, `lathe`,
//! `extrude`, `surface_patch`) emit flat-shaded geometry (own vertices per face) into a part;
//! [`Builder::mirror_part`] copies a part across x = 0. [`Builder::finish`] quantises into
//! [`MeshPart`]s (positions in mm, degenerate triangles dropped, unused vertices removed).
//!
//! Coordinates are chassis space (m, +X right, +Y forward, +Z up) for body parts and hub
//! space (axle along +X, origin at the hub) for wheel parts (`PartKey::wheel`). Triangles
//! are counter-clockwise seen from outside.

use std::collections::BTreeMap;

use wp_contracts::BodyRegion;

use crate::mesh::{MaterialSlot, MeshPart, PartKind};
use crate::profile::BodyProfile;

/// Identity of a part under construction. Two emits with an equal key go into one part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartKey {
    /// Unique lower_snake_case part name.
    pub name: String,
    /// Region (body parts); `None` for wheel-space parts.
    pub region: Option<BodyRegion>,
    /// Part kind.
    pub kind: PartKind,
    /// Material slot.
    pub material: MaterialSlot,
    /// Wheel index (0–3 FL FR RL RR) for wheel-space parts.
    pub wheel: Option<u8>,
    /// Rigid: bound to one lattice node (weight 255).
    pub rigid: bool,
}

impl PartKey {
    /// A body part skinned to `region`.
    pub fn body(name: &str, region: BodyRegion, kind: PartKind, material: MaterialSlot) -> Self {
        PartKey {
            name: name.to_string(),
            region: Some(region),
            kind,
            material,
            wheel: None,
            rigid: false,
        }
    }

    /// A wheel-space part (tire, rim, brake disc, caliper, upright) of wheel `wheel`.
    pub fn wheel(name: &str, wheel: u8, kind: PartKind, material: MaterialSlot) -> Self {
        PartKey {
            name: name.to_string(),
            region: None,
            kind,
            material,
            wheel: Some(wheel),
            rigid: false,
        }
    }

    /// The same key, bound rigidly to one node.
    pub fn rigid(mut self) -> Self {
        self.rigid = true;
        self
    }

    /// The mirror-image key: name `_l` ↔ `_r` (and `fl`↔`fr`, `rl`↔`rr`), region mirrored.
    pub fn mirrored(&self) -> Self {
        PartKey {
            name: mirror_name(&self.name),
            region: self.region.map(mirror_region),
            ..self.clone()
        }
    }
}

/// Mirror a left-side region to the right side (and back); centre regions map to themselves.
pub fn mirror_region(r: BodyRegion) -> BodyRegion {
    use BodyRegion::*;
    match r {
        FrontLeftFender => FrontRightFender,
        FrontRightFender => FrontLeftFender,
        LeftDoorF => RightDoorF,
        RightDoorF => LeftDoorF,
        LeftDoorR => RightDoorR,
        RightDoorR => LeftDoorR,
        RearLeftQuarter => RearRightQuarter,
        RearRightQuarter => RearLeftQuarter,
        FrameLeft => FrameRight,
        FrameRight => FrameLeft,
        other => other,
    }
}

/// Mirror a part name: swaps the side tokens `l`/`r`, `fl`/`fr`, `rl`/`rr` between `_`.
/// Names without a side token get `_m` appended (so they stay unique).
pub fn mirror_name(name: &str) -> String {
    let mut changed = false;
    let parts: Vec<String> = name
        .split('_')
        .map(|t| {
            let m = match t {
                "l" => "r",
                "r" => "l",
                "fl" => "fr",
                "fr" => "fl",
                "rl" => "rr",
                "rr" => "rl",
                other => other,
            };
            if m != t {
                changed = true;
            }
            m.to_string()
        })
        .collect();
    if changed {
        parts.join("_")
    } else {
        format!("{name}_m")
    }
}

/// Which part of the loft surface a [`Patch`] follows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PatchBand {
    /// LEFT side surface between heights `z0..z1` (m above ground).
    Side {
        /// Lower edge height.
        z0: f32,
        /// Upper edge height.
        z1: f32,
    },
    /// Top surface between chassis `x0..x1` (signed; may span the centre line).
    Top {
        /// Left edge x.
        x0: f32,
        /// Right edge x.
        x1: f32,
    },
}

/// A thin closed shell following the loft surface between stations `y0..y1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Patch {
    /// Rear edge y.
    pub y0: f32,
    /// Front edge y.
    pub y1: f32,
    /// Surface band.
    pub band: PatchBand,
    /// Gap between the loft surface and the shell's inner face, m (0.002 for decal carriers).
    pub offset_m: f32,
    /// Shell thickness, m (0 = single outward-facing sheet).
    pub thickness_m: f32,
    /// Grid resolution (along y, across); 0 = automatic (~6 cm).
    pub steps: (usize, usize),
}

/// Borrowed raw geometry of a part: (positions, uvs, indices).
pub type Geometry<'a> = (&'a [[f32; 3]], &'a [[f32; 2]], &'a [u32]);

/// One part under construction.
#[derive(Debug, Clone)]
struct PartBuild {
    key: PartKey,
    pos: Vec<[f32; 3]>,
    uv: Vec<[f32; 2]>,
    idx: Vec<u32>,
    dedupe: BTreeMap<(i64, i64), u32>,
}

/// Collects triangles per part with optional per-part vertex dedupe.
#[derive(Debug, Clone, Default)]
pub struct Builder {
    parts: Vec<PartBuild>,
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
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = dot(a, a).sqrt();
    if l < 1e-9 {
        [0.0, 0.0, 1.0]
    } else {
        scale(a, 1.0 / l)
    }
}

/// Two unit vectors perpendicular to `axis` (and to each other).
pub fn basis(axis: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let a = norm(axis);
    let helper = if a[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    let u = norm(cross(helper, a));
    let v = cross(a, u);
    (u, v)
}

/// `(sin, cos)` of `i / n` turns (deterministic, `wp_math::sin_cos`).
pub fn turn(i: usize, n: usize) -> (f32, f32) {
    wp_math::sin_cos(std::f32::consts::TAU * i as f32 / n.max(1) as f32)
}

impl Builder {
    /// Empty builder.
    pub fn new() -> Builder {
        Builder::default()
    }

    /// Index of the part with `key` (created on first use).
    pub fn part(&mut self, key: &PartKey) -> usize {
        if let Some(i) = self.parts.iter().position(|p| p.key == *key) {
            return i;
        }
        self.parts.push(PartBuild {
            key: key.clone(),
            pos: Vec::new(),
            uv: Vec::new(),
            idx: Vec::new(),
            dedupe: BTreeMap::new(),
        });
        self.parts.len() - 1
    }

    /// True if a part named `name` exists.
    pub fn has(&self, name: &str) -> bool {
        self.parts.iter().any(|p| p.key.name == name)
    }

    /// Vertex with a dedupe id (`None` = always new).
    pub fn vert(&mut self, part: usize, id: Option<(i64, i64)>, p: [f32; 3], uv: [f32; 2]) -> u32 {
        let entry = &mut self.parts[part];
        if let Some(id) = id {
            if let Some(&v) = entry.dedupe.get(&id) {
                return v;
            }
        }
        let v = entry.pos.len() as u32;
        entry.pos.push(p);
        entry.uv.push(uv);
        if let Some(id) = id {
            entry.dedupe.insert(id, v);
        }
        v
    }

    /// Triangle (CCW from outside). Degenerate ones are dropped in [`Builder::finish`].
    pub fn tri(&mut self, part: usize, a: u32, b: u32, c: u32) {
        self.parts[part].idx.extend_from_slice(&[a, b, c]);
    }

    /// Flat polygon (convex, CCW from outside) with its own vertices.
    pub fn poly(&mut self, key: &PartKey, pts: &[[f32; 3]]) {
        if pts.len() < 3 {
            return;
        }
        let part = self.part(key);
        let v: Vec<u32> = pts
            .iter()
            .map(|&q| self.vert(part, None, q, [0.5, 0.5]))
            .collect();
        for i in 1..v.len() - 1 {
            self.tri(part, v[0], v[i], v[i + 1]);
        }
    }

    /// Quad strip between two polylines `a[i]`, `b[i]` (same length), flat-shaded.
    /// Faces are CCW when `b` lies counter-clockwise of `a` seen from outside.
    pub fn strip(&mut self, key: &PartKey, a: &[[f32; 3]], b: &[[f32; 3]]) {
        for i in 0..a.len().min(b.len()).saturating_sub(1) {
            self.poly(key, &[a[i], a[i + 1], b[i + 1], b[i]]);
        }
    }

    /// Axis-aligned box `lo`–`hi` (the PLAN's `box`).
    pub fn cuboid(&mut self, key: &PartKey, lo: [f32; 3], hi: [f32; 3]) {
        let c = |i: usize| -> [f32; 3] {
            [
                if i & 1 == 0 { lo[0] } else { hi[0] },
                if i & 2 == 0 { lo[1] } else { hi[1] },
                if i & 4 == 0 { lo[2] } else { hi[2] },
            ]
        };
        let faces = [
            [0, 4, 6, 2],
            [1, 3, 7, 5],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 2, 3, 1],
            [4, 5, 7, 6],
        ];
        for f in faces {
            self.poly(key, &[c(f[0]), c(f[1]), c(f[2]), c(f[3])]);
        }
    }

    /// Box from a centre, three (orthogonal) unit axes and half extents along them.
    pub fn oriented_box(
        &mut self,
        key: &PartKey,
        center: [f32; 3],
        axes: [[f32; 3]; 3],
        half: [f32; 3],
    ) {
        let c = |i: usize| -> [f32; 3] {
            let s = |bit: usize, k: usize| if i & bit == 0 { -half[k] } else { half[k] };
            add(
                add(
                    add(center, scale(axes[0], s(1, 0))),
                    scale(axes[1], s(2, 1)),
                ),
                scale(axes[2], s(4, 2)),
            )
        };
        let faces = [
            [0, 4, 6, 2],
            [1, 3, 7, 5],
            [0, 1, 5, 4],
            [2, 6, 7, 3],
            [0, 2, 3, 1],
            [4, 5, 7, 6],
        ];
        // Keep CCW-from-outside for left-handed axis sets too.
        let flip = dot(cross(axes[0], axes[1]), axes[2]) < 0.0;
        for f in faces {
            let mut q = [c(f[0]), c(f[1]), c(f[2]), c(f[3])];
            if flip {
                q.reverse();
            }
            self.poly(key, &q);
        }
    }

    /// Axis-aligned box with chamfered edges (`bevel` m, clamped to a third of the size).
    pub fn beveled_box(&mut self, key: &PartKey, lo: [f32; 3], hi: [f32; 3], bevel: f32) {
        let size = sub(hi, lo);
        let b = bevel.clamp(0.0, (size[0].min(size[1]).min(size[2]) / 3.0).max(0.0));
        if b <= 1e-4 {
            return self.cuboid(key, lo, hi);
        }
        // A chamfered box is the convex hull of three inset boxes; emit it as an octagonal
        // prism along Y with chamfered end faces: profile in x–z, swept lo.y → hi.y.
        let (x0, x1, z0, z1) = (lo[0], hi[0], lo[2], hi[2]);
        let oct = [
            [x0 + b, z0],
            [x1 - b, z0],
            [x1, z0 + b],
            [x1, z1 - b],
            [x1 - b, z1],
            [x0 + b, z1],
            [x0, z1 - b],
            [x0, z0 + b],
        ];
        let inset = |y: f32, k: f32| -> Vec<[f32; 3]> {
            oct.iter()
                .map(|q| {
                    let cx = 0.5 * (x0 + x1);
                    let cz = 0.5 * (z0 + z1);
                    let sx = if q[0] > cx {
                        -k
                    } else if q[0] < cx {
                        k
                    } else {
                        0.0
                    };
                    let sz = if q[1] > cz {
                        -k
                    } else if q[1] < cz {
                        k
                    } else {
                        0.0
                    };
                    [q[0] + sx, y, q[1] + sz]
                })
                .collect()
        };
        let rings = [
            inset(lo[1], b),
            inset(lo[1] + b, 0.0),
            inset(hi[1] - b, 0.0),
            inset(hi[1], b),
        ];
        // Sides: ring r → r+1 (y increasing). Octagon is CCW seen from −Y... faces outward
        // when traversed (i, i+1) on ring r and r+1.
        for r in 0..3 {
            for i in 0..8 {
                let j = (i + 1) % 8;
                self.poly(
                    key,
                    &[rings[r][i], rings[r + 1][i], rings[r + 1][j], rings[r][j]],
                );
            }
        }
        let mut back = rings[0].clone();
        self.poly(key, &back);
        back = rings[3].clone();
        back.reverse();
        self.poly(key, &back);
    }

    /// Cylinder / cone frustum from `a` (radius `r_a`) to `b` (radius `r_b`).
    #[allow(clippy::too_many_arguments)]
    pub fn cylinder(
        &mut self,
        key: &PartKey,
        a: [f32; 3],
        b: [f32; 3],
        r_a: f32,
        r_b: f32,
        segments: usize,
        caps: bool,
    ) {
        let n = segments.max(3);
        let axis = sub(b, a);
        let (u, v) = basis(axis);
        let ring = |c: [f32; 3], r: f32| -> Vec<[f32; 3]> {
            (0..n)
                .map(|i| {
                    let (s, co) = turn(i, n);
                    add(c, add(scale(u, co * r), scale(v, s * r)))
                })
                .collect()
        };
        let ra = ring(a, r_a);
        let rb = ring(b, r_b);
        for i in 0..n {
            let j = (i + 1) % n;
            self.poly(key, &[ra[i], ra[j], rb[j], rb[i]]);
        }
        if caps {
            let mut ca = ra.clone();
            ca.reverse();
            self.poly(key, &ca);
            self.poly(key, &rb);
        }
    }

    /// Torus around `axis` through `center`.
    #[allow(clippy::too_many_arguments)]
    pub fn torus(
        &mut self,
        key: &PartKey,
        center: [f32; 3],
        axis: [f32; 3],
        major: f32,
        minor: f32,
        seg_major: usize,
        seg_minor: usize,
    ) {
        let (n, m) = (seg_major.max(3), seg_minor.max(3));
        let ax = norm(axis);
        let (u, v) = basis(ax);
        let pt = |i: usize, k: usize| -> [f32; 3] {
            let (s, c) = turn(i, n);
            let radial = add(scale(u, c), scale(v, s));
            let (s2, c2) = turn(k, m);
            add(
                center,
                add(scale(radial, major + minor * c2), scale(ax, minor * s2)),
            )
        };
        for i in 0..n {
            for k in 0..m {
                let (i1, k1) = ((i + 1) % n, (k + 1) % m);
                self.poly(key, &[pt(i, k), pt(i1, k), pt(i1, k1), pt(i, k1)]);
            }
        }
    }

    /// Surface of revolution about `axis` through `center`; `profile` = (radius, distance
    /// along the axis) points. Faces point away from the axis when the profile runs in +axis.
    pub fn lathe(
        &mut self,
        key: &PartKey,
        center: [f32; 3],
        axis: [f32; 3],
        profile: &[[f32; 2]],
        segments: usize,
    ) {
        let n = segments.max(3);
        let ax = norm(axis);
        let (u, v) = basis(ax);
        let pt = |i: usize, q: [f32; 2]| -> [f32; 3] {
            let (s, c) = turn(i, n);
            add(
                center,
                add(scale(add(scale(u, c), scale(v, s)), q[0]), scale(ax, q[1])),
            )
        };
        for i in 0..n {
            let j = (i + 1) % n;
            for w in profile.windows(2) {
                self.poly(key, &[pt(i, w[0]), pt(j, w[0]), pt(j, w[1]), pt(i, w[1])]);
            }
        }
    }

    /// Sweep a closed CCW 2-D profile along a polyline. Profile x runs along
    /// `tangent × up` ("side"), profile y along the re-orthogonalised `up`.
    pub fn extrude(
        &mut self,
        key: &PartKey,
        profile: &[[f32; 2]],
        path: &[[f32; 3]],
        up: [f32; 3],
        caps: bool,
    ) {
        if path.len() < 2 || profile.len() < 3 {
            return;
        }
        let rings: Vec<Vec<[f32; 3]>> = (0..path.len())
            .map(|i| {
                let t = if i == 0 {
                    sub(path[1], path[0])
                } else if i == path.len() - 1 {
                    sub(path[i], path[i - 1])
                } else {
                    sub(path[i + 1], path[i - 1])
                };
                let t = norm(t);
                let side = norm(cross(t, up));
                let upv = cross(side, t);
                profile
                    .iter()
                    .map(|q| add(path[i], add(scale(side, q[0]), scale(upv, q[1]))))
                    .collect()
            })
            .collect();
        let m = profile.len();
        for r in 0..rings.len() - 1 {
            for k in 0..m {
                let k1 = (k + 1) % m;
                self.poly(
                    key,
                    &[rings[r][k], rings[r + 1][k], rings[r + 1][k1], rings[r][k1]],
                );
            }
        }
        if caps {
            // (side, up, tangent) is left-handed: a CCW profile is CW seen from +tangent.
            self.poly(key, &rings[0]);
            let mut last = rings[rings.len() - 1].clone();
            last.reverse();
            self.poly(key, &last);
        }
    }

    /// Thin shell following the loft surface (see [`Patch`]).
    pub fn surface_patch(&mut self, key: &PartKey, profile: &BodyProfile, patch: &Patch) {
        let (y0, y1) = (patch.y0.min(patch.y1), patch.y0.max(patch.y1));
        let ny = if patch.steps.0 > 0 {
            patch.steps.0
        } else {
            (((y1 - y0) / 0.06).ceil() as usize).max(1)
        };
        let (a0, a1) = match patch.band {
            PatchBand::Side { z0, z1 } => (z0.min(z1), z0.max(z1)),
            PatchBand::Top { x0, x1 } => (x0.min(x1), x0.max(x1)),
        };
        let na = if patch.steps.1 > 0 {
            patch.steps.1
        } else {
            (((a1 - a0) / 0.06).ceil() as usize).max(1)
        };
        // Surface samples: (point, normal) on a (ny+1) × (na+1) grid.
        let grid: Vec<Vec<([f32; 3], [f32; 3])>> = (0..=ny)
            .map(|i| {
                let y = y0 + (y1 - y0) * i as f32 / ny as f32;
                (0..=na)
                    .map(|k| {
                        let a = a0 + (a1 - a0) * k as f32 / na as f32;
                        match patch.band {
                            PatchBand::Side { .. } => profile.side_point(y, a),
                            PatchBand::Top { .. } => profile.top_point(y, a),
                        }
                    })
                    .collect()
            })
            .collect();
        let off = |q: ([f32; 3], [f32; 3]), d: f32| add(q.0, scale(q.1, d));
        let inner: Vec<Vec<[f32; 3]>> = grid
            .iter()
            .map(|row| row.iter().map(|&q| off(q, patch.offset_m)).collect())
            .collect();
        let outer: Vec<Vec<[f32; 3]>> = grid
            .iter()
            .map(|row| {
                row.iter()
                    .map(|&q| off(q, patch.offset_m + patch.thickness_m))
                    .collect()
            })
            .collect();
        // Every face is oriented against an explicit outward direction (robust to the grid's
        // handedness on either side / band).
        let centre = {
            let c = &grid[ny / 2][na / 2];
            off(*c, patch.offset_m + 0.5 * patch.thickness_m)
        };
        for i in 0..ny {
            for k in 0..na {
                let n = grid[i][k].1;
                let q = [
                    outer[i][k],
                    outer[i + 1][k],
                    outer[i + 1][k + 1],
                    outer[i][k + 1],
                ];
                self.poly_facing(key, &q, n);
                if patch.thickness_m > 1e-4 {
                    let q = [
                        inner[i][k],
                        inner[i + 1][k],
                        inner[i + 1][k + 1],
                        inner[i][k + 1],
                    ];
                    self.poly_facing(key, &q, scale(n, -1.0));
                }
            }
        }
        if patch.thickness_m > 1e-4 {
            let mut rim: Vec<(usize, usize)> = Vec::new();
            for k in 0..=na {
                rim.push((0, k));
            }
            for i in 1..=ny {
                rim.push((i, na));
            }
            for k in (0..na).rev() {
                rim.push((ny, k));
            }
            for i in (1..ny).rev() {
                rim.push((i, 0));
            }
            let n = rim.len();
            for e in 0..n {
                let (a, b) = (rim[e], rim[(e + 1) % n]);
                let q = [
                    inner[a.0][a.1],
                    inner[b.0][b.1],
                    outer[b.0][b.1],
                    outer[a.0][a.1],
                ];
                let mid = scale(add(add(q[0], q[1]), add(q[2], q[3])), 0.25);
                self.poly_facing(key, &q, sub(mid, centre));
            }
        }
    }

    /// Flat polygon, reversed if its normal points against `outward`.
    pub fn poly_facing(&mut self, key: &PartKey, pts: &[[f32; 3]], outward: [f32; 3]) {
        if pts.len() < 3 {
            return;
        }
        let mut n = [0.0f32; 3];
        for i in 1..pts.len() - 1 {
            n = add(n, cross(sub(pts[i], pts[0]), sub(pts[i + 1], pts[0])));
        }
        if dot(n, outward) < 0.0 {
            let mut q = pts.to_vec();
            q.reverse();
            self.poly(key, &q);
        } else {
            self.poly(key, pts);
        }
    }

    /// Copy part `from` mirrored across x = 0 into `to` (winding flipped). No-op if missing.
    pub fn mirror_part(&mut self, from: &str, to: &PartKey) {
        let Some(src) = self.parts.iter().find(|p| p.key.name == from).cloned() else {
            return;
        };
        let part = self.part(to);
        let base = self.parts[part].pos.len() as u32;
        for (q, uv) in src.pos.iter().zip(&src.uv) {
            self.parts[part].pos.push([-q[0], q[1], q[2]]);
            self.parts[part].uv.push(*uv);
        }
        for t in src.idx.chunks(3) {
            self.parts[part]
                .idx
                .extend_from_slice(&[base + t[0], base + t[2], base + t[1]]);
        }
    }

    /// Apply `f` to every vertex of every part whose key matches `pred`.
    pub fn transform(&mut self, pred: impl Fn(&PartKey) -> bool, f: impl Fn([f32; 3]) -> [f32; 3]) {
        for p in self.parts.iter_mut().filter(|p| pred(&p.key)) {
            for q in &mut p.pos {
                *q = f(*q);
            }
        }
    }

    /// Set every vertex UV of part `name` with `f(position)`.
    pub fn set_uvs(&mut self, name: &str, f: impl Fn([f32; 3]) -> [f32; 2]) {
        for p in self.parts.iter_mut().filter(|p| p.key.name == name) {
            for (q, uv) in p.pos.iter().zip(p.uv.iter_mut()) {
                *uv = f(*q);
            }
        }
    }

    /// Keys of all parts, in creation order.
    pub fn keys(&self) -> Vec<PartKey> {
        self.parts.iter().map(|p| p.key.clone()).collect()
    }

    /// Raw geometry of part `name`: (positions, uvs, indices).
    pub fn geometry(&self, name: &str) -> Option<Geometry<'_>> {
        self.parts
            .iter()
            .find(|p| p.key.name == name)
            .map(|p| (p.pos.as_slice(), p.uv.as_slice(), p.idx.as_slice()))
    }

    /// Replace the geometry of part `name`.
    pub fn set_geometry(
        &mut self,
        name: &str,
        pos: Vec<[f32; 3]>,
        uv: Vec<[f32; 2]>,
        idx: Vec<u32>,
    ) {
        if let Some(p) = self.parts.iter_mut().find(|p| p.key.name == name) {
            p.pos = pos;
            p.uv = uv;
            p.idx = idx;
            p.dedupe.clear();
        }
    }

    /// Quantise into mesh parts (mm positions, 1/65535 UVs), dropping degenerate triangles
    /// (zero area after quantisation) and unused vertices; empty parts are skipped. Order:
    /// creation order (callers sort).
    pub fn finish(self) -> Vec<MeshPart> {
        let mut out = Vec::new();
        for p in self.parts {
            let q: Vec<[i32; 3]> = p
                .pos
                .iter()
                .map(|v| {
                    [
                        (v[0] * 1000.0).round() as i32,
                        (v[1] * 1000.0).round() as i32,
                        (v[2] * 1000.0).round() as i32,
                    ]
                })
                .collect();
            let mut keep: Vec<u32> = Vec::new();
            for t in p.idx.chunks(3) {
                let (a, b, c) = (q[t[0] as usize], q[t[1] as usize], q[t[2] as usize]);
                let e1 = [
                    (b[0] - a[0]) as i64,
                    (b[1] - a[1]) as i64,
                    (b[2] - a[2]) as i64,
                ];
                let e2 = [
                    (c[0] - a[0]) as i64,
                    (c[1] - a[1]) as i64,
                    (c[2] - a[2]) as i64,
                ];
                let cr = [
                    e1[1] * e2[2] - e1[2] * e2[1],
                    e1[2] * e2[0] - e1[0] * e2[2],
                    e1[0] * e2[1] - e1[1] * e2[0],
                ];
                // |cross| / 2 = area in mm²; drop below 0.5 mm².
                let a2 = cr[0] * cr[0] + cr[1] * cr[1] + cr[2] * cr[2];
                if a2 >= 1 {
                    keep.extend_from_slice(t);
                }
            }
            if keep.is_empty() {
                continue;
            }
            let mut remap = vec![u32::MAX; q.len()];
            let mut positions_mm = Vec::new();
            let mut uvs = Vec::new();
            let mut n = 0u32;
            for i in keep.iter_mut() {
                let v = *i as usize;
                if remap[v] == u32::MAX {
                    remap[v] = n;
                    n += 1;
                    positions_mm.extend_from_slice(&q[v]);
                    for t in p.uv[v] {
                        uvs.push((t.clamp(0.0, 1.0) * 65535.0).round() as u16);
                    }
                }
                *i = remap[v];
            }
            out.push(MeshPart {
                name: p.key.name,
                region: p.key.region,
                kind: p.key.kind,
                material: p.key.material,
                wheel: p.key.wheel,
                rigid: p.key.rigid,
                positions_mm,
                uvs,
                indices: keep,
                skin_nodes: Vec::new(),
                skin_weights: Vec::new(),
                fold: None,
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Signed volume of a closed part (positive when faces are CCW seen from outside).
    fn volume(b: &Builder, name: &str) -> f32 {
        let (pos, _, idx) = b.geometry(name).expect("part");
        idx.chunks(3)
            .map(|t| {
                let (a, bb, c) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
                dot(a, cross(bb, c)) / 6.0
            })
            .sum()
    }

    fn key(name: &str) -> PartKey {
        PartKey::body(name, BodyRegion::Hood, PartKind::Detail, MaterialSlot::Trim)
    }

    #[test]
    fn closed_primitives_face_outward() {
        let mut b = Builder::new();
        b.cuboid(&key("box"), [0.1, 0.2, 0.3], [0.5, 1.2, 0.8]);
        b.beveled_box(&key("bevel"), [0.1, 0.2, 0.3], [0.5, 1.2, 0.8], 0.05);
        b.cylinder(
            &key("cyl"),
            [0.0, 0.0, 0.0],
            [0.3, 0.2, 1.0],
            0.2,
            0.1,
            12,
            true,
        );
        b.torus(
            &key("torus"),
            [1.0, 0.0, 0.5],
            [0.0, 1.0, 0.0],
            0.3,
            0.05,
            16,
            8,
        );
        b.lathe(
            &key("lathe"),
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            &[[0.0, 0.0], [0.2, 0.0], [0.2, 0.5], [0.0, 0.5]],
            12,
        );
        b.extrude(
            &key("extrude"),
            &[[-0.1, -0.05], [0.1, -0.05], [0.1, 0.05], [-0.1, 0.05]],
            &[[0.0, 0.0, 0.0], [0.0, 0.5, 0.1], [0.2, 1.0, 0.1]],
            [0.0, 0.0, 1.0],
            true,
        );
        b.oriented_box(
            &key("obox"),
            [0.0; 3],
            [[0.0, 1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            [0.1, 0.2, 0.3],
        );
        let expect = [
            ("box", 0.4 * 1.0 * 0.5),
            ("bevel", 0.4 * 1.0 * 0.5 * 0.9),
            ("cyl", 0.06),
            ("torus", 0.0145),
            ("lathe", 0.06),
            ("extrude", 0.022),
            ("obox", 0.048),
        ];
        for (name, v) in expect {
            let got = volume(&b, name);
            assert!(got > 0.0, "{name}: faces point inward (volume {got})");
            assert!(
                (got - v).abs() < 0.35 * v,
                "{name}: volume {got}, expected ≈ {v}"
            );
        }
        // Mirroring keeps faces outward.
        b.mirror_part("cyl", &key("cyl_m"));
        assert!(volume(&b, "cyl_m") > 0.0);
    }

    #[test]
    fn surface_patch_is_an_outward_shell() {
        let root = crate::repo_root();
        let car = crate::load_car(&root, "debug_sedan").unwrap();
        let p = BodyProfile::measure(&car.def, &car.lattice);
        let mut b = Builder::new();
        for (name, band) in [
            ("side", PatchBand::Side { z0: 0.3, z1: 0.6 }),
            ("top", PatchBand::Top { x0: -0.3, x1: 0.4 }),
        ] {
            b.surface_patch(
                &key(name),
                &p,
                &Patch {
                    y0: 1.3,
                    y1: 1.8,
                    band,
                    offset_m: 0.002,
                    thickness_m: 0.02,
                    steps: (0, 0),
                },
            );
            assert!(volume(&b, name) > 0.0, "{name}: shell faces inward");
        }
    }

    #[test]
    fn mirror_names() {
        assert_eq!(mirror_name("door_fl_trim"), "door_fr_trim");
        assert_eq!(mirror_name("skirt_l"), "skirt_r");
        assert_eq!(mirror_name("splitter"), "splitter_m");
    }
}
