//! Bolt-on parts and lamps, placed *on* the lofted shell with the [`Shell`] surface queries:
//! lamps and plates are projected onto the face they sit on, bumpers follow the plan outline,
//! roof gear sits on the crown. Every part is skinned to the region it is bolted to, so it
//! dents, swings and falls off with that panel.

use wp_contracts::{BodyRegion, VehicleDef};

use crate::mesh::{add, lower_region, mirror, norm, scale, Builder, Key, MaterialSlot, PartKind};
use crate::profile::{lerp, BodyProfile};
use crate::shell::Shell;
use crate::style::{
    BedKind, BumperStyle, CarStyle, ExhaustStyle, Face, GrilleStyle, GuardStyle, Lamp, LampKind,
    LampShape, LightbarStyle, MirrorStyle, ScoopStyle, SpoilerStyle,
};

/// How far (m) a projected patch point may sit behind its centre's depth.
const PATCH_REACH: f32 = 0.08;

/// Detail key.
fn dkey(name: impl Into<String>, region: BodyRegion, m: MaterialSlot) -> Key {
    (name.into(), Some(region), PartKind::Detail, m)
}

/// Lamp key.
fn lkey(name: impl Into<String>, region: BodyRegion, m: MaterialSlot) -> Key {
    (name.into(), Some(region), PartKind::Light, m)
}

/// Side suffix.
fn sfx(right: bool) -> &'static str {
    if right {
        "r"
    } else {
        "l"
    }
}

/// Left-side region mirrored to `right`.
fn side_region(r: BodyRegion, right: bool) -> BodyRegion {
    if right {
        mirror(r)
    } else {
        r
    }
}

/// Builds all parts of one car.
struct Ctx<'a> {
    b: &'a mut Builder,
    sh: &'a Shell,
    p: &'a BodyProfile,
    style: &'a CarStyle,
    def: &'a VehicleDef,
}

/// Point on `face` at face coordinates (u, v), standing `proud` off the surface. Right-side
/// sign for side faces comes from `right`.
fn face_point(sh: &Shell, face: Face, u: f32, v: f32, proud: f32, right: bool) -> Option<[f32; 3]> {
    match face {
        Face::Front => sh.front_y(u, v).map(|y| [u, y + proud, v]),
        Face::Rear => sh.rear_y(u, v).map(|y| [u, y - proud, v]),
        Face::Side => sh.side_x(u, v).map(|x| {
            let x = x + proud;
            [if right { x } else { -x }, u, v]
        }),
        Face::Top => Some([u, v, sh.top_z(u, v) + proud]),
    }
}

/// Inward direction of a face (depth of patch skirts).
fn inward(face: Face, right: bool) -> [f32; 3] {
    match face {
        Face::Front => [0.0, -1.0, 0.0],
        Face::Rear => [0.0, 1.0, 0.0],
        Face::Side => [if right { -1.0 } else { 1.0 }, 0.0, 0.0],
        Face::Top => [0.0, 0.0, -1.0],
    }
}

/// Lens outline in local face coordinates (+u = outboard), counter-clockwise.
fn outline(shape: LampShape, w: f32, h: f32, slant: f32) -> Vec<(f32, f32)> {
    let (a, c) = (0.5 * w, 0.5 * h);
    let mut pts: Vec<(f32, f32)> = match shape {
        LampShape::Rect | LampShape::Slit => {
            let corners = [(-a, -c), (a, -c), (a, c), (-a, c)];
            let mut v = Vec::new();
            for i in 0..4 {
                let (p, q) = (corners[i], corners[(i + 1) % 4]);
                for k in 0..3 {
                    let t = k as f32 / 3.0;
                    v.push((p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t));
                }
            }
            v
        }
        LampShape::Round => (0..14)
            .map(|i| {
                let (s, co) = wp_math::sin_cos(std::f32::consts::TAU * i as f32 / 14.0);
                (a * co, c * s)
            })
            .collect(),
        LampShape::Pill => {
            let r = c.min(a);
            let mut v = Vec::new();
            for i in 0..=6 {
                let (s, co) = wp_math::sin_cos(
                    -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 6.0,
                );
                v.push((a - r + r * co, r * s));
            }
            for i in 0..=6 {
                let (s, co) = wp_math::sin_cos(
                    std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 6.0,
                );
                v.push((-a + r + r * co, r * s));
            }
            v
        }
        LampShape::Swept => vec![
            (-a, -c),
            (0.0, -c),
            (a, -c),
            (a - slant, c),
            (0.0, c),
            (-a, c),
        ],
        LampShape::Wedge => vec![(-a, -c), (0.0, -c), (a, -c), (a, c), (0.0, 0.2 * c)],
    };
    if !matches!(shape, LampShape::Swept) && slant != 0.0 {
        for q in pts.iter_mut() {
            q.0 += slant * q.1 / h.max(1e-3);
        }
    }
    pts
}

impl Ctx<'_> {
    /// A flat patch projected onto a face: a lens / plate / grille background. `pts` are face
    /// coordinates (u, v) of the outline, `c` the centre. Returns false (and draws nothing)
    /// if any point misses the body.
    #[allow(clippy::too_many_arguments)]
    fn patch(
        &mut self,
        key: &Key,
        face: Face,
        right: bool,
        c: (f32, f32),
        pts: &[(f32, f32)],
        proud: f32,
        depth: f32,
    ) -> bool {
        let sh = self.sh;
        let Some(cen) = face_point(sh, face, c.0, c.1, proud * 1.2, right) else {
            return false;
        };
        // Points that miss the surface or would slide far along it (a lens edge above the
        // nose sliding back over the bonnet) stay within PATCH_REACH of the centre's depth.
        let inw = inward(face, right);
        let depth_of = |q: [f32; 3]| q[0] * inw[0] + q[1] * inw[1] + q[2] * inw[2];
        let c_depth = depth_of(cen);
        let mut clamped = 0;
        let proj: Vec<[f32; 3]> = pts
            .iter()
            .map(|&(u, v)| {
                let base = match face {
                    Face::Front | Face::Rear => [u, cen[1], v],
                    Face::Side => [cen[0], u, v],
                    Face::Top => [u, v, cen[2]],
                };
                match face_point(sh, face, u, v, proud, right) {
                    Some(q) if depth_of(q) - c_depth <= PATCH_REACH => q,
                    _ => {
                        clamped += 1;
                        add(base, scale(inw, PATCH_REACH))
                    }
                }
            })
            .collect();
        if clamped > 0 {
            eprintln!(
                "{}: {} — {clamped}/{} outline points off the surface (clamped)",
                self.def.id,
                key.0,
                pts.len()
            );
        }
        for i in 0..proj.len() {
            let j = (i + 1) % proj.len();
            self.b.poly(key, &[cen, proj[i], proj[j]]);
        }
        if depth > 0.0 {
            let d = scale(inward(face, right), depth);
            for i in 0..proj.len() {
                let j = (i + 1) % proj.len();
                self.b
                    .poly(key, &[proj[i], add(proj[i], d), add(proj[j], d), proj[j]]);
            }
        }
        true
    }

    /// Ring between an outer and inner outline (bezels, surrounds).
    #[allow(clippy::too_many_arguments)]
    fn ring_patch(
        &mut self,
        key: &Key,
        face: Face,
        right: bool,
        outer: &[(f32, f32)],
        inner: &[(f32, f32)],
        proud: f32,
    ) {
        let sh = self.sh;
        let po: Option<Vec<[f32; 3]>> = outer
            .iter()
            .map(|&(u, v)| face_point(sh, face, u, v, proud, right))
            .collect();
        let pi: Option<Vec<[f32; 3]>> = inner
            .iter()
            .map(|&(u, v)| face_point(sh, face, u, v, proud, right))
            .collect();
        if let (Some(po), Some(pi)) = (po, pi) {
            for i in 0..po.len() {
                let j = (i + 1) % po.len();
                self.b.poly(key, &[po[i], po[j], pi[j], pi[i]]);
            }
        }
    }

    /// A bar on a face from (u0, v0) to (u1, v1), sampled so it follows the surface.
    #[allow(clippy::too_many_arguments)]
    fn face_bar(
        &mut self,
        key: &Key,
        face: Face,
        right: bool,
        a: (f32, f32),
        bb: (f32, f32),
        thick: f32,
        proud: f32,
    ) {
        let n = 6;
        let mut prev: Option<[f32; 3]> = None;
        for i in 0..=n {
            let t = i as f32 / n as f32;
            let u = a.0 + (bb.0 - a.0) * t;
            let v = a.1 + (bb.1 - a.1) * t;
            let Some(q) = face_point(self.sh, face, u, v, proud, right) else {
                prev = None;
                continue;
            };
            if let Some(pq) = prev {
                let up = match face {
                    Face::Top => [0.0, 0.0, 1.0],
                    _ => scale(inward(face, right), -1.0),
                };
                self.b.beam(key, pq, q, 0.5 * thick, 0.5 * thick, up);
            }
            prev = Some(q);
        }
    }

    fn lamp_region(&self, l: &Lamp, u: f32, right: bool) -> BodyRegion {
        let p = self.p;
        use BodyRegion::*;
        match l.face {
            Face::Front => {
                if l.z < p.z_bumper_top.0 + 0.02 {
                    FrontBumper
                } else if u.abs() < 0.22 {
                    Hood
                } else {
                    side_region(FrontLeftFender, right)
                }
            }
            Face::Rear => {
                if l.z < p.z_bumper_top.1 + 0.02 {
                    RearBumper
                } else if u.abs() < 0.3 || (p.trunk_lid && l.z > p.z_deck - 0.03) {
                    Trunk
                } else {
                    side_region(RearLeftQuarter, right)
                }
            }
            Face::Side => side_region(lower_region(p, l.y, l.z), right),
            Face::Top => {
                if l.y >= p.y_fw {
                    Hood
                } else if l.y >= p.roof.0 - 0.15 {
                    Roof
                } else if p.trunk_lid {
                    Trunk
                } else {
                    side_region(RearLeftQuarter, right)
                }
            }
        }
    }

    fn lamps(&mut self) {
        let mut count: std::collections::BTreeMap<LampKind, usize> = Default::default();
        for l in self.style.lamps.clone() {
            let n = count.entry(l.kind).or_insert(0);
            let idx = *n;
            *n += 1;
            let kname = match l.kind {
                LampKind::Head => "head",
                LampKind::Fog => "fog",
                LampKind::Drl => "drl",
                LampKind::Tail => "tail",
                LampKind::Brake3 => "brake3",
                LampKind::Indicator => "indicator",
                LampKind::Marker => "marker",
                LampKind::Reverse => "reverse",
                LampKind::BeaconRed => "beacon_red",
                LampKind::BeaconBlue => "beacon_blue",
            };
            let mat = match l.kind {
                LampKind::Head | LampKind::Fog | LampKind::Drl => MaterialSlot::Headlight,
                LampKind::Tail | LampKind::Brake3 => MaterialSlot::Taillight,
                LampKind::Indicator | LampKind::Marker => MaterialSlot::Indicator,
                LampKind::Reverse => MaterialSlot::Lens,
                LampKind::BeaconRed => MaterialSlot::BeaconRed,
                LampKind::BeaconBlue => MaterialSlot::BeaconBlue,
            };
            let stem = if idx == 0 {
                kname.to_string()
            } else {
                format!("{kname}_{}", idx + 1)
            };
            let sides: Vec<Option<bool>> = if l.mirror {
                vec![Some(false), Some(true)]
            } else {
                vec![None]
            };
            for side in sides {
                let right = side.unwrap_or(l.x >= 0.0);
                let sign = if right { 1.0 } else { -1.0 };
                // Face coordinates of the centre.
                let (cu, cv) = match l.face {
                    Face::Front | Face::Rear => (
                        if side.is_some() {
                            sign * l.x.abs()
                        } else {
                            l.x
                        },
                        l.z,
                    ),
                    Face::Side => (l.y, l.z),
                    Face::Top => (
                        if side.is_some() {
                            sign * l.x.abs()
                        } else {
                            l.x
                        },
                        l.y,
                    ),
                };
                // Outboard direction in u: ±x on front/rear/top, forward on the side.
                let out_u = match l.face {
                    Face::Side => 1.0,
                    _ => {
                        if side.is_some() {
                            sign
                        } else {
                            1.0
                        }
                    }
                };
                let ol: Vec<(f32, f32)> = outline(l.shape, l.w, l.h, l.slant)
                    .into_iter()
                    .map(|(du, dv)| (cu + out_u * du, cv + dv))
                    .collect();
                let ol = if out_u < 0.0 {
                    ol.into_iter().rev().collect()
                } else {
                    ol
                };
                let region = self.lamp_region(&l, cu, right);
                let name = match side {
                    Some(r) => format!("{stem}_{}", sfx(r)),
                    None => stem.clone(),
                };
                let mut proud = l.proud;
                if l.pod > 0.0 && l.face == Face::Top {
                    // Pop-up pod: a body-colour box carrying the lens on its front face.
                    let z0 = self.sh.top_z(cu, cv);
                    let pod = dkey(format!("{name}_pod"), region, MaterialSlot::Paint);
                    self.b.cube(
                        &pod,
                        [cu - 0.5 * l.w - 0.02, cv - 0.5 * l.h, z0 - 0.02],
                        [cu + 0.5 * l.w + 0.02, cv + 0.5 * l.h, z0 + l.pod],
                    );
                    let k = lkey(name.clone(), region, mat);
                    let y = cv + 0.5 * l.h + 0.004;
                    let zc = z0 + 0.5 * l.pod;
                    self.b.poly(
                        &k,
                        &[
                            [cu - 0.5 * l.w, y, zc - 0.4 * l.pod],
                            [cu + 0.5 * l.w, y, zc - 0.4 * l.pod],
                            [cu + 0.5 * l.w, y, zc + 0.4 * l.pod],
                            [cu - 0.5 * l.w, y, zc + 0.4 * l.pod],
                        ],
                    );
                    continue;
                }
                if l.bezel > 0.0 {
                    proud += 0.004;
                    let grow = |pts: &[(f32, f32)], g: f32| -> Vec<(f32, f32)> {
                        pts.iter()
                            .map(|&(u, v)| {
                                let (du, dv) = (u - cu, v - cv);
                                let d = (du * du + dv * dv).sqrt().max(1e-4);
                                (u + du / d * g, v + dv / d * g)
                            })
                            .collect()
                    };
                    let outer = grow(&ol, l.bezel);
                    let bz = dkey(format!("{name}_bezel"), region, MaterialSlot::Trim);
                    self.ring_patch(&bz, l.face, right, &outer, &ol, proud - 0.003);
                }
                let k = lkey(name.clone(), region, mat);
                if !self.patch(&k, l.face, right, (cu, cv), &ol, proud, 0.04) {
                    eprintln!("{}: lamp {name} misses the body; skipped", self.def.id);
                }
            }
        }
    }

    fn grille(&mut self) {
        let g = self.style.parts.grille.clone();
        if g.style == GrilleStyle::None {
            return;
        }
        let p = self.p;
        let region = if g.z < p.z_bumper_top.0 + 0.03 {
            BodyRegion::FrontBumper
        } else {
            BodyRegion::Hood
        };
        let bar_m = if g.chrome {
            MaterialSlot::Chrome
        } else {
            MaterialSlot::Metal
        };
        let bg = dkey("grille", region, MaterialSlot::Trim);
        let rect = |hw: f32, z: f32, h: f32| -> Vec<(f32, f32)> {
            let mut v = Vec::new();
            for k in 0..=6 {
                v.push((-hw + 2.0 * hw * k as f32 / 6.0, z - 0.5 * h));
            }
            v.push((hw, z + 0.5 * h));
            for k in (0..6).rev() {
                v.push((-hw + 2.0 * hw * k as f32 / 6.0, z + 0.5 * h));
            }
            v
        };
        if !self.patch(
            &bg,
            Face::Front,
            true,
            (0.0, g.z),
            &rect(g.hw, g.z, g.h),
            0.006,
            0.05,
        ) {
            eprintln!("{}: grille misses the body", self.def.id);
            return;
        }
        let bars = dkey("grille_bars", region, bar_m);
        let n = g.bars.max(1);
        let (hz, vt) = match g.style {
            GrilleStyle::Slats => (n, 0),
            GrilleStyle::Bars => (0, n),
            GrilleStyle::Mesh => (2 * n, 0),
            GrilleStyle::Egg => (n, 2 * n),
            GrilleStyle::None => (0, 0),
        };
        let thick_h = (g.h / (hz.max(1) as f32) * 0.32).clamp(0.008, 0.03);
        for k in 0..hz {
            let v = g.z - 0.5 * g.h + g.h * (k as f32 + 0.5) / hz as f32;
            self.face_bar(
                &bars,
                Face::Front,
                true,
                (-g.hw + 0.01, v),
                (g.hw - 0.01, v),
                thick_h,
                0.014,
            );
        }
        let thick_v = (2.0 * g.hw / (vt.max(1) as f32) * 0.25).clamp(0.008, 0.03);
        for k in 0..vt {
            let u = -g.hw + 2.0 * g.hw * (k as f32 + 0.5) / vt as f32;
            self.face_bar(
                &bars,
                Face::Front,
                true,
                (u, g.z - 0.5 * g.h + 0.01),
                (u, g.z + 0.5 * g.h - 0.01),
                thick_v,
                0.016,
            );
        }
        if g.surround > 0.0 {
            let sur = dkey(
                "grille_surround",
                region,
                if g.chrome {
                    MaterialSlot::Chrome
                } else {
                    MaterialSlot::Trim
                },
            );
            let outer = rect(g.hw + g.surround, g.z, g.h + 2.0 * g.surround);
            let inner = rect(g.hw, g.z, g.h);
            self.ring_patch(&sur, Face::Front, true, &outer, &inner, 0.012);
        }
        let [ihw, iz, ih] = g.intake;
        if ihw > 0.0 {
            let ireg = if iz < p.z_bumper_top.0 + 0.03 {
                BodyRegion::FrontBumper
            } else {
                region
            };
            let ik = dkey("intake", ireg, MaterialSlot::Trim);
            if self.patch(
                &ik,
                Face::Front,
                true,
                (0.0, iz),
                &rect(ihw, iz, ih),
                0.006,
                0.05,
            ) {
                let slat = dkey("intake_slats", ireg, MaterialSlot::Trim);
                self.face_bar(
                    &slat,
                    Face::Front,
                    true,
                    (-ihw + 0.02, iz),
                    (ihw - 0.02, iz),
                    0.014,
                    0.014,
                );
            }
        }
    }

    fn bumpers(&mut self) {
        for front in [true, false] {
            let bs = if front {
                self.style.parts.bumper_f.clone()
            } else {
                self.style.parts.bumper_r.clone()
            };
            if bs.style == BumperStyle::Integrated {
                continue;
            }
            let (m, thick) = match bs.style {
                BumperStyle::Chrome => (MaterialSlot::Chrome, 0.05),
                BumperStyle::Rubber => (MaterialSlot::Trim, 0.04),
                BumperStyle::Steel => (
                    if bs.chrome {
                        MaterialSlot::Chrome
                    } else {
                        MaterialSlot::Trim
                    },
                    0.07,
                ),
                BumperStyle::Integrated => continue,
            };
            let region = if front {
                BodyRegion::FrontBumper
            } else {
                BodyRegion::RearBumper
            };
            let name = if front {
                "bumper_bar_front"
            } else {
                "bumper_bar_rear"
            };
            let (path, normals) = self.sh.end_path(bs.z, bs.wrap, front);
            if path.len() < 2 {
                eprintln!("{}: {name} misses the body", self.def.id);
                continue;
            }
            let h = 0.5 * bs.h;
            let r = 0.3 * h.min(thick);
            let sec = [
                (-0.02, -h),
                (bs.proud - r, -h),
                (bs.proud, -h + r),
                (bs.proud, h - r),
                (bs.proud - r, h),
                (-0.02, h),
            ];
            let _ = thick;
            self.b.sweep(
                &dkey(name, region, m),
                &path,
                &normals,
                [0.0, 0.0, 1.0],
                &sec,
            );
        }
        let ps = &self.style.parts;
        if ps.splitter > 0.0 {
            let z = self.p.sill(self.p.y_nose - 0.05) + 0.03;
            let (path, normals) = self.sh.end_path(z, 0.15, true);
            if path.len() >= 2 {
                let sec = [
                    (-0.03, 0.0),
                    (ps.splitter, 0.0),
                    (ps.splitter, 0.018),
                    (-0.03, 0.018),
                ];
                self.b.sweep(
                    &dkey("splitter", BodyRegion::FrontBumper, MaterialSlot::Trim),
                    &path,
                    &normals,
                    [0.0, 0.0, 1.0],
                    &sec,
                );
            }
        }
        if ps.diffuser {
            let p = self.p;
            let k = dkey("diffuser", BodyRegion::RearBumper, MaterialSlot::Trim);
            let z0 = p.sill(p.y_tail) + 0.01;
            for i in 0..5 {
                let x = -0.48 + 0.24 * i as f32;
                self.b.cube(
                    &k,
                    [x - 0.008, p.y_tail - 0.01, z0],
                    [x + 0.008, p.y_tail + 0.35, z0 + 0.11],
                );
            }
            self.b.cube(
                &k,
                [-0.6, p.y_tail - 0.01, z0 + 0.1],
                [0.6, p.y_tail + 0.4, z0 + 0.12],
            );
        }
        if ps.skirts {
            let p = self.p;
            let fa = p.arches.iter().find(|a| a.3).copied();
            let ra = p
                .arches
                .iter()
                .filter(|a| !a.3)
                .map(|a| a.0 + a.2)
                .fold(f32::MIN, f32::max);
            if let Some((fy, _, fr, _)) = fa {
                let (y0, y1) = (ra + 0.03, fy - fr - 0.03);
                for right in [false, true] {
                    let reg = side_region(BodyRegion::FrameLeft, right);
                    let k = dkey(format!("skirt_{}", sfx(right)), reg, MaterialSlot::Trim);
                    let n = 8;
                    let mut path = Vec::new();
                    let mut sides = Vec::new();
                    for i in 0..=n {
                        let y = lerp(y0, y1, i as f32 / n as f32);
                        let z = p.sill(y) + 0.06;
                        let x = self.sh.side_x(y, z).unwrap_or(p.half_w);
                        let s = if right { 1.0 } else { -1.0 };
                        path.push([s * x, y, z]);
                        sides.push([s, 0.0, 0.0]);
                    }
                    self.b.sweep(
                        &k,
                        &path,
                        &sides,
                        [0.0, 0.0, 1.0],
                        &[(-0.01, -0.07), (0.028, -0.06), (0.03, 0.04), (-0.01, 0.06)],
                    );
                }
            }
        }
    }

    fn scoop(&mut self) {
        let p = self.p;
        let style = self.style.parts.scoop;
        if style == ScoopStyle::None {
            return;
        }
        let yc = lerp(p.y_fw, p.y_bf, 0.42);
        let top = |x: f32, y: f32| self.sh.top_z(x, y);
        let body = |b: &mut Builder,
                    k: &Key,
                    hw: f32,
                    len: f32,
                    hf: f32,
                    hr: f32,
                    open: Option<&Key>,
                    yc: f32,
                    sh: &Shell| {
            let (yf, yr) = (yc + 0.5 * len, yc - 0.5 * len);
            let zf = sh.top_z(0.0, yf);
            let zr = sh.top_z(0.0, yr);
            let zfo = sh.top_z(hw, yf);
            let zro = sh.top_z(hw, yr);
            let fl = [-hw, yf, zfo];
            let fr = [hw, yf, zfo];
            let rl = [-hw, yr, zro];
            let rr = [hw, yr, zro];
            let ftl = [-hw * 0.9, yf, zf + hf];
            let ftr = [hw * 0.9, yf, zf + hf];
            let rtl = [-hw * 0.9, yr, zr + hr];
            let rtr = [hw * 0.9, yr, zr + hr];
            b.poly(k, &[ftl, ftr, rtr, rtl]);
            b.poly(k, &[fl, ftl, rtl, rl]);
            b.poly(k, &[fr, rr, rtr, ftr]);
            b.poly(k, &[rl, rtl, rtr, rr]);
            b.poly(open.unwrap_or(k), &[fl, fr, ftr, ftl]);
        };
        let open = dkey("scoop_mouth", BodyRegion::Hood, MaterialSlot::Trim);
        match style {
            ScoopStyle::Ram => {
                let k = dkey("scoop", BodyRegion::Hood, MaterialSlot::Paint);
                body(self.b, &k, 0.22, 0.55, 0.075, 0.0, Some(&open), yc, self.sh);
            }
            ScoopStyle::Flat => {
                let k = dkey("scoop", BodyRegion::Hood, MaterialSlot::Paint);
                body(
                    self.b,
                    &k,
                    0.3,
                    0.4,
                    0.045,
                    0.0,
                    Some(&open),
                    yc + 0.1,
                    self.sh,
                );
            }
            ScoopStyle::Shaker => {
                let k = dkey("scoop", BodyRegion::Hood, MaterialSlot::Trim);
                body(
                    self.b,
                    &k,
                    0.2,
                    0.32,
                    0.12,
                    0.1,
                    Some(&open),
                    yc - 0.05,
                    self.sh,
                );
            }
            ScoopStyle::Vents => {
                let k = dkey("hood_vents", BodyRegion::Hood, MaterialSlot::Trim);
                for x in [-0.3f32, 0.3] {
                    for i in 0..4 {
                        let y = yc - 0.12 + 0.08 * i as f32;
                        let z = top(x, y);
                        let _ = z;
                        self.face_bar(
                            &k,
                            Face::Top,
                            x > 0.0,
                            (x - 0.1, y),
                            (x + 0.1, y),
                            0.02,
                            0.006,
                        );
                    }
                }
            }
            ScoopStyle::None => {}
        }
    }

    fn spoiler(&mut self) {
        let p = self.p;
        let sp = self.style.parts.spoiler.clone();
        if sp.style == SpoilerStyle::None {
            return;
        }
        let m = if sp.painted {
            MaterialSlot::Paint
        } else {
            MaterialSlot::Trim
        };
        let k = dkey("spoiler", BodyRegion::Trunk, m);
        let n = 6;
        let xs: Vec<f32> = (0..=n)
            .map(|i| -sp.hw + 2.0 * sp.hw * i as f32 / n as f32)
            .collect();
        match sp.style {
            SpoilerStyle::Lip => {
                let y = if sp.y != 0.0 { sp.y } else { p.y_tail + 0.06 };
                for w in xs.windows(2) {
                    let z0 = self.sh.top_z(w[0], y);
                    let z1 = self.sh.top_z(w[1], y);
                    let a = [w[0], y, z0];
                    let bq = [w[1], y, z1];
                    let a2 = [w[0], y + sp.chord, self.sh.top_z(w[0], y + sp.chord)];
                    let b2 = [w[1], y + sp.chord, self.sh.top_z(w[1], y + sp.chord)];
                    let at = [w[0], y, z0 + sp.rise];
                    let bt = [w[1], y, z1 + sp.rise];
                    self.b.poly(&k, &[a2, b2, bt, at]);
                    self.b.poly(&k, &[a, at, bt, bq]);
                }
            }
            SpoilerStyle::Wing | SpoilerStyle::Roof => {
                let (y0, zb) = if sp.style == SpoilerStyle::Roof {
                    let y = if sp.y != 0.0 { sp.y } else { p.roof.0 + 0.02 };
                    (y, self.sh.top_z(0.0, y))
                } else {
                    let y = if sp.y != 0.0 {
                        sp.y
                    } else {
                        p.y_tail + sp.chord + 0.06
                    };
                    (y, self.sh.top_z(0.0, y - 0.5 * sp.chord) + sp.rise)
                };
                let c = sp.chord;
                let drop = if sp.style == SpoilerStyle::Roof {
                    0.05
                } else {
                    0.0
                };
                let foil = [
                    (y0, zb),
                    (y0 - 0.3 * c, zb + 0.028),
                    (y0 - c, zb + 0.012 - drop),
                    (y0 - c, zb - 0.004 - drop),
                    (y0 - 0.3 * c, zb - 0.008),
                ];
                for w in xs.windows(2) {
                    for i in 0..foil.len() {
                        let (f0, f1) = (foil[i], foil[(i + 1) % foil.len()]);
                        self.b.poly(
                            &k,
                            &[
                                [w[0], f0.0, f0.1],
                                [w[1], f0.0, f0.1],
                                [w[1], f1.0, f1.1],
                                [w[0], f1.0, f1.1],
                            ],
                        );
                    }
                }
                for x in [-sp.hw, sp.hw] {
                    let pts: Vec<[f32; 3]> = foil.iter().map(|f| [x, f.0, f.1]).collect();
                    self.b.poly(&k, &pts);
                }
                if sp.style == SpoilerStyle::Wing {
                    let up = dkey("spoiler_mounts", BodyRegion::Trunk, MaterialSlot::Trim);
                    for x in [-0.62 * sp.hw, 0.62 * sp.hw] {
                        let yb = y0 - 0.55 * c;
                        let z0 = self.sh.top_z(x, yb);
                        self.b.cube(
                            &up,
                            [x - 0.012, yb - 0.06, z0 - 0.01],
                            [x + 0.012, yb + 0.06, zb],
                        );
                    }
                    let ep = dkey("spoiler_endplates", BodyRegion::Trunk, MaterialSlot::Trim);
                    for x in [-sp.hw, sp.hw] {
                        self.b.cube(
                            &ep,
                            [x - 0.006, y0 - c - 0.02, zb - 0.06],
                            [x + 0.006, y0 + 0.01, zb + 0.07],
                        );
                    }
                }
            }
            SpoilerStyle::None => {}
        }
    }

    fn exhaust(&mut self) {
        let p = self.p;
        let e = self.style.parts.exhaust.clone();
        let r = 0.5 * e.d;
        let tips: Vec<f32> = match e.style {
            ExhaustStyle::None | ExhaustStyle::Stacks | ExhaustStyle::Side => Vec::new(),
            ExhaustStyle::Single => vec![e.x],
            ExhaustStyle::Dual => vec![-e.x, e.x],
            ExhaustStyle::Quad => vec![-e.x - 1.3 * e.d, -e.x, e.x, e.x + 1.3 * e.d],
            ExhaustStyle::Centre => vec![-0.65 * e.d, 0.65 * e.d],
        };
        let k = dkey("exhaust", BodyRegion::RearBumper, MaterialSlot::Chrome);
        let dark = dkey("exhaust_bore", BodyRegion::RearBumper, MaterialSlot::Trim);
        for x in tips {
            let y_end = self
                .sh
                .rear_y(x, e.z)
                .map_or(p.y_tail + 0.02, |y| y.min(p.y_tail + 0.12))
                - 0.035;
            self.b
                .cyl(&k, [x, y_end + 0.2, e.z], [x, y_end, e.z], r, 10);
            self.b.cyl(
                &dark,
                [x, y_end + 0.01, e.z],
                [x, y_end - 0.002, e.z],
                0.72 * r,
                10,
            );
        }
        match e.style {
            ExhaustStyle::Stacks => {
                let y = if e.y != 0.0 { e.y } else { p.y_deck - 0.15 };
                for s in [-1.0, 1.0] {
                    let x = s * e.x;
                    let reg = side_region(BodyRegion::RearLeftQuarter, s > 0.0);
                    let sk = dkey(format!("stack_{}", sfx(s > 0.0)), reg, MaterialSlot::Chrome);
                    self.b.cyl(&sk, [x, y, p.z_door_bottom], [x, y, e.z], r, 10);
                    let hs = dkey(
                        format!("stack_shield_{}", sfx(s > 0.0)),
                        reg,
                        MaterialSlot::Metal,
                    );
                    self.b
                        .cyl(&hs, [x, y, p.z_belt], [x, y, p.z_belt + 0.9], r + 0.02, 10);
                }
            }
            ExhaustStyle::Side => {
                let fa = p.arches.iter().find(|a| a.3).copied();
                let ra = p
                    .arches
                    .iter()
                    .filter(|a| !a.3)
                    .map(|a| a.0 + a.2)
                    .fold(f32::MIN, f32::max);
                if let Some((fy, _, fr, _)) = fa {
                    for s in [-1.0f32, 1.0] {
                        let reg = side_region(BodyRegion::FrameLeft, s > 0.0);
                        let sk = dkey(
                            format!("side_pipe_{}", sfx(s > 0.0)),
                            reg,
                            MaterialSlot::Chrome,
                        );
                        let y0 = fy - fr - 0.05;
                        let y1 = ra + 0.05;
                        let z = p.sill(0.0) + 0.07;
                        let x = s
                            * (self
                                .sh
                                .side_x(0.5 * (y0 + y1), z + 0.05)
                                .unwrap_or(p.half_w)
                                + 0.05);
                        self.b.cyl(&sk, [x, y0, z], [x, y1, z], r, 10);
                    }
                }
            }
            _ => {}
        }
    }

    fn mirrors(&mut self) {
        let p = self.p;
        let style = self.style.parts.mirrors;
        for right in [false, true] {
            let s = if right { 1.0 } else { -1.0 };
            let door = side_region(BodyRegion::LeftDoorF, right);
            let y = (p.y_fd_hinge - 0.14).min(p.y_fw - 0.08);
            let zb = p.belt(y);
            let w = self.sh.side_x(y, zb - 0.03).unwrap_or(p.half_w) - 0.03;
            let house = (
                format!("mirror_{}", sfx(right)),
                Some(door),
                PartKind::Mirror,
                MaterialSlot::Paint,
            );
            let glass = (
                format!("mirror_glass_{}", sfx(right)),
                Some(door),
                PartKind::Mirror,
                MaterialSlot::Chrome,
            );
            let arm = (
                format!("mirror_arm_{}", sfx(right)),
                Some(door),
                PartKind::Mirror,
                MaterialSlot::Trim,
            );
            match style {
                MirrorStyle::Shell | MirrorStyle::Slim => {
                    let (sx, sy, sz) = if style == MirrorStyle::Shell {
                        (0.09, 0.05, 0.065)
                    } else {
                        (0.06, 0.035, 0.04)
                    };
                    let cx = s * (w + 0.06 + sx);
                    let cz = zb + 0.09;
                    self.b.beam(
                        &arm,
                        [s * w, y - 0.02, zb + 0.03],
                        [cx - s * sx * 0.8, y - 0.02, cz - 0.02],
                        0.02,
                        0.015,
                        [0.0, 0.0, 1.0],
                    );
                    self.b.obox(
                        &house,
                        [cx, y, cz],
                        [[sx, 0.0, 0.0], [0.0, sy, 0.0], [0.0, 0.0, sz]],
                    );
                    self.b.cube(
                        &glass,
                        [cx - sx * 0.85, y - sy - 0.004, cz - sz * 0.8],
                        [cx + sx * 0.85, y - sy + 0.002, cz + sz * 0.8],
                    );
                }
                MirrorStyle::Bullet => {
                    let cx = s * (w + 0.1);
                    let cz = zb + 0.07;
                    let ch = (
                        format!("mirror_{}", sfx(right)),
                        Some(door),
                        PartKind::Mirror,
                        MaterialSlot::Chrome,
                    );
                    self.b.beam(
                        &ch,
                        [s * w, y, zb],
                        [cx, y, cz],
                        0.01,
                        0.01,
                        [0.0, 0.0, 1.0],
                    );
                    self.b
                        .cone(&ch, [cx, y + 0.07, cz], [cx, y - 0.03, cz], 0.0, 0.045, 8);
                }
                MirrorStyle::Truck => {
                    let cx = s * (w + 0.22);
                    let z0 = zb + 0.02;
                    for dz in [0.04, 0.36] {
                        self.b.beam(
                            &arm,
                            [s * w, y + 0.02, z0 + dz],
                            [cx, y - 0.02, z0 + dz],
                            0.012,
                            0.012,
                            [0.0, 0.0, 1.0],
                        );
                    }
                    self.b.cube(
                        &arm,
                        [cx - 0.08, y - 0.06, z0 - 0.02],
                        [cx + 0.08, y + 0.02, z0 + 0.42],
                    );
                    self.b.cube(
                        &glass,
                        [cx - 0.07, y - 0.066, z0],
                        [cx + 0.07, y - 0.058, z0 + 0.4],
                    );
                }
            }
        }
    }

    fn police(&mut self) {
        let p = self.p;
        let ps = self.style.parts.clone();
        let (ry0, ry1, _, _) = p.roof;
        // Light bar.
        let lb = ps.lightbar.clone();
        if lb.style != LightbarStyle::None {
            let yc = if lb.y != 0.0 {
                lb.y
            } else {
                0.5 * (ry0 + ry1) + 0.1 * (ry1 - ry0)
            };
            let z = self.sh.top_z(0.0, yc);
            let base = dkey("lightbar", BodyRegion::Roof, MaterialSlot::Trim);
            let red = lkey("lightbar_red", BodyRegion::Roof, MaterialSlot::BeaconRed);
            let blue = lkey("lightbar_blue", BodyRegion::Roof, MaterialSlot::BeaconBlue);
            let white = lkey("lightbar_white", BodyRegion::Roof, MaterialSlot::Lens);
            let hw = lb.hw;
            match lb.style {
                LightbarStyle::Full | LightbarStyle::Low => {
                    let (lh, foot) = if lb.style == LightbarStyle::Full {
                        (0.085, 0.05)
                    } else {
                        (0.05, 0.03)
                    };
                    for x in [-0.75 * hw, 0.75 * hw] {
                        self.b.cube(
                            &base,
                            [x - 0.05, yc - 0.1, z - 0.02],
                            [x + 0.05, yc + 0.1, z + foot],
                        );
                    }
                    self.b.cube(
                        &base,
                        [-hw, yc - 0.15, z + foot],
                        [hw, yc + 0.15, z + foot + 0.025],
                    );
                    let n = 6;
                    for i in 0..n {
                        let x0 = -hw + 2.0 * hw * i as f32 / n as f32 + 0.008;
                        let x1 = -hw + 2.0 * hw * (i + 1) as f32 / n as f32 - 0.008;
                        let k = if i == 2 || i == 3 {
                            if i == 2 {
                                &red
                            } else {
                                &blue
                            }
                        } else if i < 2 {
                            &red
                        } else {
                            &blue
                        };
                        let k = if (i == 2 || i == 3) && lb.style == LightbarStyle::Full {
                            &white
                        } else {
                            k
                        };
                        self.b.cube(
                            k,
                            [x0, yc - 0.135, z + foot + 0.025],
                            [x1, yc + 0.135, z + foot + 0.025 + lh],
                        );
                    }
                }
                LightbarStyle::Domes => {
                    self.b
                        .cube(&base, [-hw, yc - 0.06, z - 0.01], [hw, yc + 0.06, z + 0.04]);
                    self.b.cone(
                        &red,
                        [-0.7 * hw, yc, z + 0.04],
                        [-0.7 * hw, yc, z + 0.17],
                        0.09,
                        0.07,
                        10,
                    );
                    self.b.cone(
                        &blue,
                        [0.7 * hw, yc, z + 0.04],
                        [0.7 * hw, yc, z + 0.17],
                        0.09,
                        0.07,
                        10,
                    );
                }
                LightbarStyle::Edge => {
                    let rw = p.roof_half_w() - 0.08;
                    for (y, f) in [(ry1 - 0.08, true), (ry0 + 0.08, false)] {
                        let zz = self.sh.top_z(rw, y);
                        let dy = if f { 0.05 } else { -0.05 };
                        self.b.cube(
                            &red,
                            [-rw - 0.06, y - 0.05 + dy, zz - 0.01],
                            [-rw + 0.14, y + 0.05 + dy, zz + 0.07],
                        );
                        self.b.cube(
                            &blue,
                            [rw - 0.14, y - 0.05 + dy, zz - 0.01],
                            [rw + 0.06, y + 0.05 + dy, zz + 0.07],
                        );
                    }
                    let zz = self.sh.top_z(0.0, ry1 - 0.08);
                    self.b.cube(
                        &base,
                        [-0.35, ry1 - 0.14, zz - 0.01],
                        [0.35, ry1 - 0.02, zz + 0.03],
                    );
                    self.b.cube(
                        &white,
                        [-0.3, ry1 - 0.13, zz + 0.03],
                        [0.3, ry1 - 0.03, zz + 0.07],
                    );
                }
                LightbarStyle::None => {}
            }
        }
        // Front guard.
        if ps.guard != GuardStyle::None {
            let reg = if p.has_accessory {
                BodyRegion::Accessory
            } else {
                BodyRegion::FrontBumper
            };
            let zt = p.z_bumper_top.0;
            let yface = self.sh.front_y(0.0, zt - 0.05).unwrap_or(p.y_nose);
            match ps.guard {
                GuardStyle::Push => {
                    let k = dkey("push_bar", reg, MaterialSlot::Trim);
                    let pad = dkey("push_bar_pads", reg, MaterialSlot::Rubber);
                    let yf = yface + 0.12;
                    for x in [-0.3f32, 0.3] {
                        self.b.cube(
                            &k,
                            [x - 0.035, yf - 0.05, zt - 0.28],
                            [x + 0.035, yf + 0.02, zt + 0.3],
                        );
                        self.b.cube(
                            &pad,
                            [x - 0.04, yf + 0.02, zt - 0.2],
                            [x + 0.04, yf + 0.05, zt + 0.25],
                        );
                        let yb = self.sh.front_y(x, zt - 0.2).unwrap_or(yface) - 0.02;
                        self.b.cube(
                            &k,
                            [x - 0.025, yb, zt - 0.24],
                            [x + 0.025, yf - 0.05, zt - 0.17],
                        );
                    }
                    for zz in [zt + 0.22, zt - 0.08] {
                        self.b.cube(
                            &k,
                            [-0.42, yf - 0.04, zz - 0.025],
                            [0.42, yf + 0.0, zz + 0.025],
                        );
                    }
                }
                GuardStyle::Bull => {
                    let k = dkey("bull_bar", reg, MaterialSlot::Chrome);
                    let yf = yface + 0.1;
                    for x in [-0.32f32, 0.32] {
                        self.b
                            .cyl(&k, [x, yf, zt - 0.25], [x, yf, zt + 0.3], 0.03, 8);
                    }
                    self.b
                        .cyl(&k, [-0.32, yf, zt + 0.3], [0.32, yf, zt + 0.3], 0.03, 8);
                    self.b.cyl(
                        &k,
                        [-0.5, yf - 0.06, zt - 0.05],
                        [0.5, yf - 0.06, zt - 0.05],
                        0.035,
                        8,
                    );
                }
                GuardStyle::Ram => {
                    let k = dkey("ram_plate", reg, MaterialSlot::Trim);
                    let w = p.half_w - 0.08;
                    let yf = yface + 0.22;
                    let z0 = p.sill(p.y_nose) + 0.05;
                    let z1 = zt + 0.25;
                    let a = [-w, yf, z0 + 0.1];
                    let bq = [w, yf, z0 + 0.1];
                    let c = [w, yf - 0.16, z1];
                    let d = [-w, yf - 0.16, z1];
                    self.b.poly(&k, &[a, bq, c, d]);
                    self.b
                        .poly(&k, &[[-w, yf - 0.25, z0], [w, yf - 0.25, z0], bq, a]);
                    for x in [-w, w] {
                        self.b.poly(
                            &k,
                            &[
                                [x, yf - 0.25, z0],
                                [x, yf, z0 + 0.1],
                                [x, yf - 0.16, z1],
                                [x, yf - 0.3, z1],
                            ],
                        );
                    }
                    let rib = dkey("ram_ribs", reg, MaterialSlot::Metal);
                    for i in 0..5 {
                        let x = -0.8 * w + 0.4 * w * i as f32;
                        self.b.beam(
                            &rib,
                            [x, yf + 0.01, z0 + 0.12],
                            [x, yf - 0.15, z1 - 0.02],
                            0.02,
                            0.012,
                            [0.0, 1.0, 0.0],
                        );
                    }
                }
                GuardStyle::None => {}
            }
        }
        if ps.spotlights {
            let y = p.y_fw - 0.12;
            let z = p.belt(y) + 0.14;
            let x = -(self.sh.side_x(y, p.belt(y) + 0.02).unwrap_or(p.half_w) + 0.04);
            let k = dkey("spotlight", BodyRegion::Cabin, MaterialSlot::Chrome);
            self.b.cyl(&k, [x, y - 0.1, z], [x, y + 0.06, z], 0.055, 10);
            self.b.beam(
                &k,
                [x + 0.05, y - 0.05, z - 0.08],
                [x, y - 0.05, z],
                0.01,
                0.01,
                [0.0, 1.0, 0.0],
            );
            let lens = lkey("spotlight_lens", BodyRegion::Cabin, MaterialSlot::Lens);
            self.b
                .cyl(&lens, [x, y + 0.06, z], [x, y + 0.068, z], 0.048, 10);
        }
        if ps.antennas > 0 {
            let k = dkey("antennas", BodyRegion::Roof, MaterialSlot::Trim);
            let n = ps.antennas;
            for i in 0..n {
                let x = (i as f32 - 0.5 * (n as f32 - 1.0)) * 0.32;
                let y = ry0 + 0.18 + 0.05 * (i % 2) as f32;
                let z = self.sh.top_z(x, y);
                self.b
                    .cyl(&k, [x, y, z - 0.005], [x, y, z + 0.02], 0.025, 6);
                self.b.cyl(&k, [x, y, z + 0.02], [x, y, z + 0.6], 0.004, 4);
            }
        }
    }

    fn plates_handles_stripes(&mut self) {
        let p = self.p;
        let ps = self.style.parts.clone();
        if ps.plates {
            let (pw, ph) = (0.36, 0.12);
            let rect = |c: (f32, f32)| -> Vec<(f32, f32)> {
                vec![
                    (c.0 - 0.5 * pw, c.1 - 0.5 * ph),
                    (c.0 + 0.5 * pw, c.1 - 0.5 * ph),
                    (c.0 + 0.5 * pw, c.1 + 0.5 * ph),
                    (c.0 - 0.5 * pw, c.1 + 0.5 * ph),
                ]
            };
            let zf = if ps.plate_z[0] > 0.0 {
                ps.plate_z[0]
            } else {
                (p.z_bumper_top.0 - 0.13).max(p.sill(p.y_nose) + 0.12)
            };
            let freg = if zf < p.z_bumper_top.0 + 0.02 {
                BodyRegion::FrontBumper
            } else {
                BodyRegion::Hood
            };
            let k = dkey("plate_front", freg, MaterialSlot::Plate);
            if !self.patch(
                &k,
                Face::Front,
                true,
                (0.0, zf),
                &rect((0.0, zf)),
                0.008,
                0.02,
            ) {
                eprintln!("{}: front plate misses the body", self.def.id);
            }
            let zr = if ps.plate_z[1] > 0.0 {
                ps.plate_z[1]
            } else if p.s.bed == BedKind::Rails {
                p.s.bed_height - 0.12
            } else {
                (p.z_bumper_top.1 - 0.09).max(p.sill(p.y_tail) + 0.12)
            };
            let reg = if zr < p.z_bumper_top.1 + 0.02 {
                BodyRegion::RearBumper
            } else {
                BodyRegion::Trunk
            };
            let k = dkey("plate_rear", reg, MaterialSlot::Plate);
            if !self.patch(
                &k,
                Face::Rear,
                true,
                (0.0, zr),
                &rect((0.0, zr)),
                0.008,
                0.02,
            ) {
                eprintln!("{}: rear plate misses the body", self.def.id);
            }
        }
        if ps.handles {
            let mut doors = vec![(
                BodyRegion::LeftDoorF,
                match p.rear_door {
                    Some((rh, _)) => rh + 0.14,
                    None => p.y_fd_rear + 0.16,
                },
            )];
            if let Some((_, re)) = p.rear_door {
                doors.push((BodyRegion::LeftDoorR, re + 0.14));
            }
            for (door, y) in doors {
                for right in [false, true] {
                    let reg = side_region(door, right);
                    let k = dkey(
                        format!("handle_{}_{}", crate::mesh::panel_name(reg), "trim"),
                        reg,
                        MaterialSlot::Trim,
                    );
                    let z = p.z_belt - 0.08;
                    let ol = vec![
                        (y - 0.08, z - 0.016),
                        (y + 0.08, z - 0.016),
                        (y + 0.08, z + 0.016),
                        (y - 0.08, z + 0.016),
                    ];
                    self.patch(&k, Face::Side, right, (y, z), &ol, 0.008, 0.01);
                }
            }
        }
        let [shw, gap] = ps.stripes;
        if shw > 0.0 {
            let (ry0, ry1, _, _) = p.roof;
            let mut spans = vec![
                (
                    "hood",
                    BodyRegion::Hood,
                    p.y_fw + 0.03,
                    p.y_nose - 0.5 * p.s.nose_len,
                ),
                ("roof", BodyRegion::Roof, ry0 + 0.04, ry1 - 0.04),
            ];
            if p.trunk_lid {
                spans.push((
                    "trunk",
                    BodyRegion::Trunk,
                    p.y_tail + 0.5 * p.s.tail_len,
                    p.y_deck - 0.03,
                ));
            }
            for (name, reg, y0, y1) in spans {
                if y1 <= y0 {
                    continue;
                }
                let k = dkey(format!("stripes_{name}"), reg, MaterialSlot::Accent);
                let ny = (((y1 - y0) / 0.15).ceil() as usize).max(1);
                for sgn in [-1.0f32, 1.0] {
                    let xa = sgn * 0.5 * gap;
                    let xb = sgn * (0.5 * gap + 2.0 * shw);
                    for i in 0..ny {
                        let ya = lerp(y0, y1, i as f32 / ny as f32);
                        let yb = lerp(y0, y1, (i + 1) as f32 / ny as f32);
                        let q = |x: f32, y: f32| [x, y, self.sh.top_z(x, y) + 0.004];
                        let pts = if sgn > 0.0 {
                            [q(xa, ya), q(xb, ya), q(xb, yb), q(xa, yb)]
                        } else {
                            [q(xb, ya), q(xa, ya), q(xa, yb), q(xb, yb)]
                        };
                        self.b.poly(&k, &pts);
                    }
                }
            }
        }
    }

    fn utility(&mut self) {
        let p = self.p;
        let u = self.style.parts.utility.clone();
        let (ry0, ry1, rz, _) = p.roof;
        let rw = p.roof_half_w();
        if u.sleeper > 0.0 || u.fairing > 0.0 {
            let top = rz.max(u.fairing);
            if u.sleeper > 0.0 {
                let k = dkey("sleeper", BodyRegion::Cabin, MaterialSlot::Paint);
                let w = p.half_w - 0.06;
                let y0 = p.y_deck - u.sleeper;
                let y1 = p.y_deck + 0.02;
                let z0 = p.z_door_bottom - 0.05;
                self.b.cube(&k, [-w, y0, z0], [w, y1, top]);
                let win = dkey(
                    "sleeper_windows",
                    BodyRegion::Cabin,
                    MaterialSlot::GlassDark,
                );
                for s in [-1.0f32, 1.0] {
                    self.b.cube(
                        &win,
                        [s * w - 0.006, y0 + 0.25, p.z_belt + 0.15],
                        [s * w + 0.006, y0 + 0.55, p.z_belt + 0.35],
                    );
                }
            }
            if u.fairing > rz {
                let k = dkey("roof_fairing", BodyRegion::Roof, MaterialSlot::Paint);
                let y_back = p.y_deck - u.sleeper + 0.02;
                let yf = ry1 - 0.05;
                let w = rw + 0.04;
                let a = [-w, yf, rz];
                let bq = [w, yf, rz];
                let c = [w, y_back, u.fairing];
                let d = [-w, y_back, u.fairing];
                let c0 = [w, y_back, rz];
                let d0 = [-w, y_back, rz];
                self.b.poly(&k, &[a, bq, c, d]);
                self.b.poly(&k, &[bq, c0, c]);
                self.b.poly(&k, &[a, d, d0]);
                self.b.poly(&k, &[d0, d, c, c0]);
            }
        }
        if u.fifth_wheel {
            let ra = p
                .arches
                .iter()
                .filter(|a| !a.3)
                .map(|a| a.0)
                .fold(0.0f32, |s, y| s + y)
                / p.arches.iter().filter(|a| !a.3).count().max(1) as f32;
            let z = p.s.bed_height;
            let k = dkey(
                "fifth_wheel",
                BodyRegion::RearLeftQuarter,
                MaterialSlot::Metal,
            );
            self.b.cyl(
                &k,
                [0.0, ra + 0.25, z],
                [0.0, ra + 0.25, z + 0.07],
                0.45,
                14,
            );
            let jaw = dkey(
                "fifth_wheel_jaw",
                BodyRegion::RearLeftQuarter,
                MaterialSlot::Trim,
            );
            self.b.cube(
                &jaw,
                [-0.08, ra - 0.25, z + 0.06],
                [0.08, ra + 0.05, z + 0.075],
            );
        }
        let [tr, tl] = u.fuel_tanks;
        if tr > 0.0 && tl > 0.0 {
            let yc = p.y_deck + 0.15 - 0.5 * tl;
            for right in [false, true] {
                let s = if right { 1.0 } else { -1.0 };
                let reg = side_region(BodyRegion::FrameLeft, right);
                let k = dkey(
                    format!("fuel_tank_{}", sfx(right)),
                    reg,
                    MaterialSlot::Chrome,
                );
                let x = s * (p.half_w - tr - 0.03);
                let z = p.z_floor + tr + 0.12;
                self.b
                    .cyl(&k, [x, yc - 0.5 * tl, z], [x, yc + 0.5 * tl, z], tr, 12);
                let st = dkey(
                    format!("tank_straps_{}", sfx(right)),
                    reg,
                    MaterialSlot::Trim,
                );
                for f in [-0.3f32, 0.3] {
                    let y = yc + f * tl;
                    self.b
                        .cyl(&st, [x, y - 0.02, z], [x, y + 0.02, z], tr + 0.012, 12);
                }
            }
        }
        if u.mudflaps {
            if let Some(&(wy, wz, r, _)) = p
                .arches
                .iter()
                .filter(|a| !a.3)
                .min_by(|a, b| a.0.total_cmp(&b.0))
            {
                for right in [false, true] {
                    let s = if right { 1.0 } else { -1.0 };
                    let w = self.def.wheels[2 + right as usize].width_m;
                    let x = s * self.def.wheels[2 + right as usize].position_m[0].abs();
                    let reg = side_region(BodyRegion::RearLeftQuarter, right);
                    let k = dkey(format!("mudflap_{}", sfx(right)), reg, MaterialSlot::Rubber);
                    let y = wy - r - 0.06;
                    self.b.cube(
                        &k,
                        [x - 0.5 * w - 0.06, y - 0.008, 0.14],
                        [x + 0.5 * w + 0.06, y + 0.008, wz + 0.55 * r],
                    );
                }
            }
        }
        if u.steps {
            for right in [false, true] {
                let s = if right { 1.0 } else { -1.0 };
                let reg = side_region(BodyRegion::FrameLeft, right);
                let k = dkey(format!("step_{}", sfx(right)), reg, MaterialSlot::Metal);
                let w = p.half_w;
                let y1 = p.y_fd_hinge - 0.08;
                let y0 = y1 - 0.5;
                let z = p.z_door_bottom - 0.18;
                let (x0, x1) = if right {
                    (w - 0.12, w + 0.04)
                } else {
                    (-w - 0.04, -w + 0.12)
                };
                self.b
                    .cube(&k, [x0.min(x1), y0, z], [x0.max(x1), y1, z + 0.035]);
                let _ = s;
            }
        }
        if u.roof_rack {
            let k = dkey("roof_rack", BodyRegion::Roof, MaterialSlot::Metal);
            let x = rw - 0.1;
            let (y0, y1) = (ry0 + 0.1, ry1 - 0.12);
            let z = self.sh.top_z(x, 0.5 * (y0 + y1));
            for s in [-1.0f32, 1.0] {
                self.b.cube(
                    &k,
                    [s * x - 0.02, y0, z + 0.06],
                    [s * x + 0.02, y1, z + 0.09],
                );
                for y in [y0 + 0.05, y1 - 0.05] {
                    self.b.cube(
                        &k,
                        [s * x - 0.02, y - 0.02, z - 0.01],
                        [s * x + 0.02, y + 0.02, z + 0.06],
                    );
                }
            }
            let n = (((y1 - y0) / 0.45).floor() as usize).max(2);
            for i in 0..=n {
                let y = lerp(y0, y1, i as f32 / n as f32);
                self.b
                    .cube(&k, [-x, y - 0.015, z + 0.065], [x, y + 0.015, z + 0.085]);
            }
        }
        if u.rollup || u.ladder {
            let top = p.top(p.y_tail + 0.05);
            if u.rollup {
                let k = dkey("rollup_ribs", BodyRegion::Trunk, MaterialSlot::Metal);
                let w = self
                    .sh
                    .side_x(p.y_tail + 0.02, 0.5 * (top + p.z_bumper_top.1))
                    .unwrap_or(p.half_w)
                    - 0.08;
                let mut z = p.z_bumper_top.1 + 0.12;
                while z < top - 0.1 {
                    self.b.cube(
                        &k,
                        [-w, p.y_tail - 0.012, z - 0.008],
                        [w, p.y_tail + 0.01, z + 0.008],
                    );
                    z += 0.22;
                }
            }
            if u.ladder {
                let k = dkey("ladder", BodyRegion::Trunk, MaterialSlot::Metal);
                let x = 0.6 * p.half_w;
                let y = p.y_tail - 0.06;
                let z0 = p.z_bumper_top.1 - 0.15;
                for dx in [-0.2f32, 0.2] {
                    self.b.cube(
                        &k,
                        [x + dx - 0.015, y - 0.015, z0],
                        [x + dx + 0.015, y + 0.015, top + 0.15],
                    );
                }
                let mut z = z0 + 0.1;
                while z < top + 0.1 {
                    self.b.cube(
                        &k,
                        [x - 0.2, y - 0.01, z - 0.01],
                        [x + 0.2, y + 0.01, z + 0.01],
                    );
                    z += 0.3;
                }
            }
        }
        if u.window_guards {
            let (y0, y1) = (p.y_fw - 0.35, ry0 + 0.15);
            for right in [false, true] {
                let s = if right { 1.0 } else { -1.0 };
                let k = dkey(
                    format!("window_guards_{}", sfx(right)),
                    BodyRegion::Cabin,
                    MaterialSlot::Trim,
                );
                let n = (((y1 - y0) / 0.13).floor() as usize).max(2);
                for i in 0..=n {
                    let y = lerp(y0, y1, i as f32 / n as f32);
                    let zb = p.belt(y) + 0.03;
                    let zt = p.top(y) - 0.09;
                    if zt <= zb + 0.1 {
                        continue;
                    }
                    let xb = self.sh.side_x(y, zb).unwrap_or(p.half_w) + 0.015;
                    let xt = self.sh.side_x(y, zt).unwrap_or(p.half_w) + 0.015;
                    self.b.beam(
                        &k,
                        [s * xb, y, zb],
                        [s * xt, y, zt],
                        0.008,
                        0.008,
                        [s, 0.0, 0.0],
                    );
                }
            }
            // Windshield mesh: horizontal slats.
            let k = dkey("windshield_guard", BodyRegion::Cabin, MaterialSlot::Trim);
            for f in [0.25f32, 0.5, 0.75] {
                let y = lerp(p.y_fw, ry1, f);
                let z = p.top(y) + 0.03;
                let w = p.roof_half_w() * 0.95;
                self.b
                    .cube(&k, [-w, y - 0.01, z - 0.01], [w, y + 0.01, z + 0.01]);
            }
        }
        if u.air_horns {
            let k = dkey("air_horns", BodyRegion::Roof, MaterialSlot::Chrome);
            let y = ry1 - 0.25;
            for x in [-0.25f32, 0.25] {
                let z = self.sh.top_z(x, y) + 0.06;
                self.b
                    .cone(&k, [x, y - 0.35, z], [x, y + 0.15, z], 0.025, 0.07, 8);
            }
        }
        if u.cab_guard {
            let z = rz + 0.12;
            for right in [false, true] {
                let s = if right { 1.0 } else { -1.0 };
                let reg = side_region(BodyRegion::RearLeftQuarter, right);
                let k = dkey(
                    format!("cab_guard_{}", sfx(right)),
                    reg,
                    MaterialSlot::Paint,
                );
                let w = p.s.bed_half_w.max(p.half_w);
                let (x0, x1) = if right { (0.0, w) } else { (-w, 0.0) };
                self.b.cube(
                    &k,
                    [x0, ry1 - 0.3, z],
                    [x1, p.y_deck - p.s.bed_gap + 0.02, z + 0.04],
                );
                let (wx0, wx1) = if right {
                    (w - 0.03, w)
                } else {
                    (-w, -w + 0.03)
                };
                self.b.cube(
                    &k,
                    [wx0, ry1 - 0.3, z - 0.12],
                    [wx1, p.y_deck - p.s.bed_gap + 0.02, z],
                );
                let _ = s;
            }
        }
        if u.hatch {
            let k = dkey("roof_hatch", BodyRegion::Roof, MaterialSlot::Trim);
            let y = 0.5 * (ry0 + ry1) - 0.2;
            let z = self.sh.top_z(0.0, y);
            self.b
                .cyl(&k, [0.0, y, z - 0.01], [0.0, y, z + 0.09], 0.36, 14);
            self.b
                .cyl(&k, [0.0, y, z + 0.09], [0.0, y, z + 0.12], 0.3, 14);
        }
        if u.side_intakes {
            if let Some(&(wy, _, r, _)) = p
                .arches
                .iter()
                .filter(|a| !a.3)
                .max_by(|a, b| a.0.total_cmp(&b.0))
            {
                for right in [false, true] {
                    let reg = side_region(BodyRegion::RearLeftQuarter, right);
                    let k = dkey(
                        format!("side_intake_{}", sfx(right)),
                        reg,
                        MaterialSlot::Trim,
                    );
                    let y = wy + r + 0.22;
                    let z = p.z_belt - 0.16;
                    let ol = vec![
                        (y - 0.2, z - 0.1),
                        (y + 0.2, z - 0.06),
                        (y + 0.2, z + 0.08),
                        (y - 0.2, z + 0.08),
                    ];
                    self.patch(&k, Face::Side, right, (y, z), &ol, 0.004, 0.04);
                }
            }
        }
        if u.louvres {
            let k = dkey("louvres", BodyRegion::Trunk, MaterialSlot::Trim);
            let y0 = p.y_deck - 0.08;
            for i in 0..6 {
                let y = y0 - 0.09 * i as f32;
                if y < p.y_tail + 0.2 {
                    break;
                }
                self.face_bar(&k, Face::Top, true, (-0.5, y), (0.5, y), 0.025, 0.008);
            }
        }
        // Rails deck or a raised box/tipper: fenders over the drive wheels.
        if p.s.bed == BedKind::Rails
            || (matches!(p.s.bed, BedKind::Box | BedKind::Tipper) && p.s.bed_floor > 0.0)
        {
            for &(wy, wz, r, front) in &p.arches {
                if front {
                    continue;
                }
                for right in [false, true] {
                    let s = if right { 1.0 } else { -1.0 };
                    let wi = 2 + right as usize;
                    let xc = s * self.def.wheels[wi].position_m[0].abs();
                    let w = 0.5 * self.def.wheels[wi].width_m + 0.05;
                    let reg = side_region(BodyRegion::RearLeftQuarter, right);
                    let k = dkey(
                        format!("wheel_fender_{}", sfx(right)),
                        reg,
                        MaterialSlot::Trim,
                    );
                    let n = 7;
                    let rr = r + 0.02;
                    for i in 0..n {
                        let a0 = 0.35 + 2.44 * i as f32 / n as f32;
                        let a1 = 0.35 + 2.44 * (i + 1) as f32 / n as f32;
                        let (s0, c0) = wp_math::sin_cos(a0);
                        let (s1, c1) = wp_math::sin_cos(a1);
                        let q0 = [wy + rr * c0, wz + rr * s0];
                        let q1 = [wy + rr * c1, wz + rr * s1];
                        self.b.poly(
                            &k,
                            &[
                                [xc - w, q0[0], q0[1]],
                                [xc + w, q0[0], q0[1]],
                                [xc + w, q1[0], q1[1]],
                                [xc - w, q1[0], q1[1]],
                            ],
                        );
                    }
                }
            }
        }
    }

    fn interior(&mut self) {
        let p = self.p;
        let key = (
            "interior".to_string(),
            Some(BodyRegion::Cabin),
            PartKind::Interior,
            MaterialSlot::Interior,
        );
        let w = p.half_w - 0.18;
        let cab_rear = p.y_deck.max(p.y_quarter.0).max(p.roof.0 - 0.3);
        // Dash.
        self.b.cube(
            &key,
            [-w, p.y_fw - 0.38, p.z_belt - 0.25],
            [w, p.y_fw - 0.05, p.z_belt + 0.02],
        );
        // Seats: cushion + back for each front seat, bench behind if there is room.
        let seat_y = p.y_fw - 1.0;
        let floor = p.z_door_bottom.min(p.z_belt - 0.4);
        for x in [-0.37f32, 0.37] {
            self.b.cube(
                &key,
                [x - 0.24, seat_y - 0.05, floor],
                [x + 0.24, seat_y + 0.45, floor + 0.2],
            );
            self.b.beam(
                &key,
                [x, seat_y - 0.02, floor + 0.15],
                [x, seat_y - 0.14, p.z_belt + 0.28],
                0.24,
                0.06,
                [0.0, 1.0, 0.0],
            );
        }
        if seat_y - 0.9 > cab_rear {
            self.b.cube(
                &key,
                [-w, seat_y - 0.95, floor],
                [w, seat_y - 0.5, floor + 0.2],
            );
        }
        // Steering wheel (left-hand drive).
        let sw = (
            "steering_wheel".to_string(),
            Some(BodyRegion::Cabin),
            PartKind::Interior,
            MaterialSlot::Trim,
        );
        let c = [-0.37, p.y_fw - 0.5, p.z_belt + 0.04];
        self.b.cone(
            &sw,
            add(c, [0.0, 0.03, -0.015]),
            add(c, [0.0, -0.01, 0.005]),
            0.19,
            0.19,
            12,
        );
        let col = norm([0.0, 1.0, -0.45]);
        self.b.cyl(&sw, c, add(c, scale(col, 0.3)), 0.025, 6);
    }
}

/// Add every lamp and bolt-on part of `style` to the builder.
pub fn build(b: &mut Builder, sh: &Shell, style: &CarStyle, def: &VehicleDef) {
    let mut ctx = Ctx {
        b,
        sh,
        p: &sh.p,
        style,
        def,
    };
    ctx.lamps();
    ctx.grille();
    ctx.bumpers();
    ctx.scoop();
    ctx.spoiler();
    ctx.exhaust();
    ctx.mirrors();
    ctx.police();
    ctx.plates_handles_stripes();
    ctx.utility();
    ctx.interior();
}
