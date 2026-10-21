//! Procedural low-poly car mesh, format 2: a lofted body shaped by the car's style sheet
//! (cross-section rings along +Y with creases, flares, greenhouse, beds and boxes, wheel
//! arches), cut into one part per body region along the lattice's zone boundaries, plus
//! surface-projected lamps, bolt-on parts ([`crate::parts`]), a small interior and styled
//! wheels ([`crate::wheels`]).
//! Units m, +Y forward, +Z up, origin = chassis origin (mid-wheelbase on the ground).

use serde::{Deserialize, Serialize};
use wp_contracts::{BodyRegion, LatticeDef, VehicleDef};

use crate::profile::BodyProfile;
use crate::shell::{Shell, RING_N};
use crate::style::{parse_rgb, BedKind, CarStyle, PaintSlot};

/// Mesh file format version (`CarMesh::format`). 2: style sheets, palette, detail parts,
/// lamp/beacon materials, visual extra axles (`hub_offset_mm`).
pub const MESH_FORMAT: u32 = 2;
/// Triangle budget for cars (min, max).
pub const TRI_BUDGET: (u32, u32) = (3000, 9000);
/// Triangle budget ceiling for heavy vehicles (body longer than [`HEAVY_LENGTH_M`]).
pub const TRI_BUDGET_HEAVY: u32 = 12000;
/// Bodies longer than this are heavy vehicles (bigger budget, coarser loft).
pub const HEAVY_LENGTH_M: f32 = 5.6;

/// What a part is (renderer picks material and behaviour from it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    /// Painted body panel of `region`.
    Panel,
    /// Glass (windshield, rear glass, door and quarter windows).
    Glass,
    /// Lamp lens (head, tail, indicator, marker, reverse, beacon).
    Light,
    /// Door mirror.
    Mirror,
    /// Non-panel body (pillars, sills, underbody, bed floor, frame rails).
    Body,
    /// Bolt-on detail riding with its region (grille, bumper bar, spoiler, exhaust, light
    /// bar, push bar, truck hardware, plates, handles, bezels).
    Detail,
    /// Minimal interior shell.
    Interior,
    /// Tire (wheel-local, not skinned).
    Tire,
    /// Rim (wheel-local, not skinned).
    Rim,
}

/// Material slot names the renderer maps to its palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialSlot {
    /// Car paint (livery colour).
    Paint,
    /// Livery accent colour (stripes, police doors, two-tone).
    Accent,
    /// Primer / mismatched replacement panel.
    Primer,
    /// Window glass.
    Glass,
    /// Dark-tinted glass.
    GlassDark,
    /// Black plastic / trim / underbody.
    Trim,
    /// Chrome.
    Chrome,
    /// Bare / brushed metal.
    Metal,
    /// Headlight lens (lit with the headlights).
    Headlight,
    /// Tail / brake lens (lit with the tail lights, brighter braking).
    Taillight,
    /// Amber indicator / marker lens.
    Indicator,
    /// Clear unlit lens (reversing lamps, spotlights).
    Lens,
    /// Police beacon, red.
    BeaconRed,
    /// Police beacon, blue.
    BeaconBlue,
    /// Licence plate.
    Plate,
    /// Tire rubber.
    Rubber,
    /// Interior.
    Interior,
}

impl From<PaintSlot> for MaterialSlot {
    fn from(p: PaintSlot) -> MaterialSlot {
        match p {
            PaintSlot::Paint => MaterialSlot::Paint,
            PaintSlot::Accent => MaterialSlot::Accent,
            PaintSlot::Primer => MaterialSlot::Primer,
            PaintSlot::Trim => MaterialSlot::Trim,
            PaintSlot::Chrome => MaterialSlot::Chrome,
            PaintSlot::Metal => MaterialSlot::Metal,
        }
    }
}

/// One mesh part. Vertex attributes are quantised: positions in mm, UVs in 1/65535, skin
/// weights in 1/255 (summing to 255 per vertex).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshPart {
    /// Unique part name (`door_fl`, `door_fl_glass`, `wheel_fl_tire`, …).
    pub name: String,
    /// Body region the part belongs to and is skinned to; `None` for wheel parts.
    pub region: Option<BodyRegion>,
    /// Part kind.
    pub kind: PartKind,
    /// Material slot.
    pub material: MaterialSlot,
    /// Wheel index (FL, FR, RL, RR) for tire/rim parts; their positions are relative to the
    /// hub with the axle along +X.
    #[serde(default)]
    pub wheel: Option<u8>,
    /// Visual extra axle: the hub sits this far (mm, chassis axes) from the physics wheel's
    /// hub and spins with it. `None` = on the physics hub.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hub_offset_mm: Option<[i32; 3]>,
    /// xyz per vertex, mm (chassis space; wheel parts: hub space).
    pub positions_mm: Vec<i32>,
    /// uv per vertex in 1/65535 (U 0–0.5 is reserved for liveries; base paint maps to 0.5–1).
    pub uvs: Vec<u16>,
    /// Triangles (counter-clockwise from outside), 3 per triangle.
    pub indices: Vec<u32>,
    /// 4 lattice node ids per vertex (`bind-skin`); empty for wheel parts.
    #[serde(default)]
    pub skin_nodes: Vec<u16>,
    /// 4 weights per vertex in 1/255 (`bind-skin`); empty for wheel parts.
    #[serde(default)]
    pub skin_weights: Vec<u8>,
}

impl MeshPart {
    /// Vertex count.
    pub fn vertex_count(&self) -> usize {
        self.positions_mm.len() / 3
    }

    /// Triangle count.
    pub fn tri_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Vertex `v` in metres.
    pub fn vertex(&self, v: usize) -> [f32; 3] {
        [
            self.positions_mm[v * 3] as f32 * 1e-3,
            self.positions_mm[v * 3 + 1] as f32 * 1e-3,
            self.positions_mm[v * 3 + 2] as f32 * 1e-3,
        ]
    }
}

/// Livery colours carried by the mesh (0xRRGGBB).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshPalette {
    /// Signature body colour.
    pub base: u32,
    /// Accent colour.
    pub accent: u32,
    /// Primer colour.
    pub primer: u32,
    /// Alternative body colours.
    pub alts: Vec<u32>,
}

/// A car's mesh file: `data/panels/meshes/<car>.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarMesh {
    /// Car id (= file stem).
    pub id: String,
    /// Lattice the skin binding refers to.
    pub lattice_id: String,
    /// [`MESH_FORMAT`].
    pub format: u32,
    /// Total triangles.
    pub tri_count: u32,
    /// Livery colours from the style sheet.
    pub palette: MeshPalette,
    /// Parts in a stable order.
    pub parts: Vec<MeshPart>,
}

impl CarMesh {
    /// Serialise one part per line (compact but diffable per part).
    pub fn to_json(&self) -> String {
        let mut s = String::new();
        s.push_str("{\n");
        s.push_str(&format!(
            "  \"id\": {},\n",
            serde_json::to_string(&self.id).unwrap()
        ));
        s.push_str(&format!(
            "  \"lattice_id\": {},\n",
            serde_json::to_string(&self.lattice_id).unwrap()
        ));
        s.push_str(&format!("  \"format\": {},\n", self.format));
        s.push_str(&format!("  \"tri_count\": {},\n", self.tri_count));
        s.push_str(&format!(
            "  \"palette\": {},\n",
            serde_json::to_string(&self.palette).unwrap()
        ));
        s.push_str("  \"parts\": [\n");
        for (i, p) in self.parts.iter().enumerate() {
            s.push_str("    ");
            s.push_str(&serde_json::to_string(p).unwrap());
            s.push_str(if i + 1 < self.parts.len() {
                ",\n"
            } else {
                "\n"
            });
        }
        s.push_str("  ]\n}\n");
        s
    }

    /// Triangle ceiling for a body of `length_m`.
    pub fn budget_max(length_m: f32) -> u32 {
        if length_m > HEAVY_LENGTH_M {
            TRI_BUDGET_HEAVY
        } else {
            TRI_BUDGET.1
        }
    }
}

/// Part key while building: (name, region, kind, material).
pub type Key = (String, Option<BodyRegion>, PartKind, MaterialSlot);

/// One part under construction: key, positions, uvs, indices, vertex dedupe map.
type PartBuild = (
    Key,
    Vec<[f32; 3]>,
    Vec<[f32; 2]>,
    Vec<u32>,
    std::collections::BTreeMap<(i64, i64), u32>,
);

/// Collects triangles per part with per-part vertex dedupe, plus primitive helpers.
#[derive(Default)]
pub struct Builder {
    parts: Vec<PartBuild>,
}

/// Vector helpers.
pub fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
/// a − b.
pub fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
/// a · k.
pub fn scale(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}
/// Cross product.
pub fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
/// Unit vector (zero stays zero).
pub fn norm(a: [f32; 3]) -> [f32; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if l < 1e-9 {
        a
    } else {
        scale(a, 1.0 / l)
    }
}

impl Builder {
    /// Part index for `key` (created on first use).
    pub fn part(&mut self, key: &Key) -> usize {
        if let Some(i) = self.parts.iter().position(|p| p.0 == *key) {
            return i;
        }
        self.parts.push((
            key.clone(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Default::default(),
        ));
        self.parts.len() - 1
    }

    /// Vertex with a dedupe id (`None` = always new).
    pub fn vert(&mut self, part: usize, id: Option<(i64, i64)>, p: [f32; 3], uv: [f32; 2]) -> u32 {
        let entry = &mut self.parts[part];
        if let Some(id) = id {
            if let Some(&v) = entry.4.get(&id) {
                return v;
            }
        }
        let v = entry.1.len() as u32;
        entry.1.push(p);
        entry.2.push(uv);
        if let Some(id) = id {
            entry.4.insert(id, v);
        }
        v
    }

    /// One triangle.
    pub fn tri(&mut self, part: usize, a: u32, b: u32, c: u32) {
        self.parts[part].3.extend_from_slice(&[a, b, c]);
    }

    /// Convex polygon (fan) with its own vertices; degenerate polygons are skipped.
    pub fn poly(&mut self, key: &Key, pts: &[[f32; 3]]) {
        if pts.len() < 3 {
            return;
        }
        let mut area = [0.0f32; 3];
        for i in 1..pts.len() - 1 {
            area = add(area, cross(sub(pts[i], pts[0]), sub(pts[i + 1], pts[0])));
        }
        if area[0].abs() + area[1].abs() + area[2].abs() < 1e-7 {
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

    /// Axis-aligned box `lo`–`hi`.
    pub fn cube(&mut self, key: &Key, lo: [f32; 3], hi: [f32; 3]) {
        let c = scale(add(lo, hi), 0.5);
        let h = scale(sub(hi, lo), 0.5);
        self.obox(
            key,
            c,
            [[h[0], 0.0, 0.0], [0.0, h[1], 0.0], [0.0, 0.0, h[2]]],
        );
    }

    /// Oriented box: centre and three half-extent vectors.
    pub fn obox(&mut self, key: &Key, c: [f32; 3], ax: [[f32; 3]; 3]) {
        let corner = |i: usize| -> [f32; 3] {
            let mut p = c;
            for (k, a) in ax.iter().enumerate() {
                let s = if i & (1 << k) == 0 { -1.0 } else { 1.0 };
                p = add(p, scale(*a, s));
            }
            p
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
            let q: Vec<[f32; 3]> = f.iter().map(|&i| corner(i)).collect();
            self.poly(key, &q);
        }
    }

    /// Box spanning segment `a`→`b` with half width `hw` (across, perpendicular to `up`)
    /// and half height `hh` (along `up`).
    pub fn beam(&mut self, key: &Key, a: [f32; 3], b: [f32; 3], hw: f32, hh: f32, up: [f32; 3]) {
        let d = sub(b, a);
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if len < 1e-5 {
            return;
        }
        let t = scale(d, 1.0 / len);
        let mut side = cross(t, up);
        if side[0].abs() + side[1].abs() + side[2].abs() < 1e-5 {
            side = cross(t, [1.0, 0.0, 0.0]);
        }
        let side = norm(side);
        let u = norm(cross(side, t));
        self.obox(
            key,
            scale(add(a, b), 0.5),
            [scale(t, 0.5 * len), scale(side, hw), scale(u, hh)],
        );
    }

    /// Cylinder from `a` to `b`, radius `r`, `n` sides, capped ends.
    pub fn cyl(&mut self, key: &Key, a: [f32; 3], b: [f32; 3], r: f32, n: usize) {
        self.cone(key, a, b, r, r, n);
    }

    /// Truncated cone from `a` (radius `ra`) to `b` (radius `rb`), capped.
    pub fn cone(&mut self, key: &Key, a: [f32; 3], b: [f32; 3], ra: f32, rb: f32, n: usize) {
        let t = norm(sub(b, a));
        let mut s = cross(t, [0.0, 0.0, 1.0]);
        if s[0].abs() + s[1].abs() + s[2].abs() < 1e-4 {
            s = cross(t, [1.0, 0.0, 0.0]);
        }
        let s = norm(s);
        let u = cross(s, t);
        let ring = |c: [f32; 3], r: f32| -> Vec<[f32; 3]> {
            (0..n)
                .map(|i| {
                    let (sn, cs) = wp_math::sin_cos(std::f32::consts::TAU * i as f32 / n as f32);
                    add(c, add(scale(s, r * cs), scale(u, r * sn)))
                })
                .collect()
        };
        let ra_pts = ring(a, ra);
        let rb_pts = ring(b, rb);
        for i in 0..n {
            let j = (i + 1) % n;
            self.poly(key, &[ra_pts[i], ra_pts[j], rb_pts[j], rb_pts[i]]);
        }
        if ra > 0.0 {
            self.poly(key, &ra_pts);
        }
        if rb > 0.0 {
            self.poly(key, &rb_pts);
        }
    }

    /// Sweep a 2D section (offsets along each point's `side` and `up` vectors) along a path;
    /// consecutive section points form faces (an open section; close it by repeating the
    /// first point).
    pub fn sweep(
        &mut self,
        key: &Key,
        path: &[[f32; 3]],
        sides: &[[f32; 3]],
        up: [f32; 3],
        section: &[(f32, f32)],
    ) {
        for i in 0..path.len().saturating_sub(1) {
            for k in 0..section.len().saturating_sub(1) {
                let at = |pi: usize, sk: usize| {
                    add(
                        path[pi],
                        add(scale(sides[pi], section[sk].0), scale(up, section[sk].1)),
                    )
                };
                self.poly(
                    key,
                    &[at(i, k), at(i + 1, k), at(i + 1, k + 1), at(i, k + 1)],
                );
            }
        }
        // End caps (fan of the section at each end).
        for &pi in &[0, path.len() - 1] {
            let pts: Vec<[f32; 3]> = section
                .iter()
                .map(|&(a, b)| add(path[pi], add(scale(sides[pi], a), scale(up, b))))
                .collect();
            self.poly(key, &pts);
        }
    }

    /// Finish into mesh parts (positions in mm), sorted by name.
    fn finish(self) -> Vec<MeshPart> {
        let mut parts: Vec<MeshPart> = Vec::new();
        for ((name, region, kind, material), pos, uvs, idx, _) in self.parts {
            if idx.is_empty() {
                continue;
            }
            parts.push(MeshPart {
                name,
                region,
                kind,
                material,
                wheel: None,
                hub_offset_mm: None,
                positions_mm: to_mm(&pos),
                uvs: uvs
                    .iter()
                    .flat_map(|t| {
                        t.iter()
                            .map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16)
                    })
                    .collect(),
                indices: idx,
                skin_nodes: Vec::new(),
                skin_weights: Vec::new(),
            });
        }
        parts.sort_by(|a, b| a.name.cmp(&b.name));
        parts
    }
}

/// Metres → integer millimetres.
pub fn to_mm(pos: &[[f32; 3]]) -> Vec<i32> {
    pos.iter()
        .flat_map(|q| q.iter().map(|v| (v * 1000.0).round() as i32))
        .collect()
}

/// Mirror a left-side region to the right side.
pub fn mirror(r: BodyRegion) -> BodyRegion {
    use BodyRegion::*;
    match r {
        FrontLeftFender => FrontRightFender,
        LeftDoorF => RightDoorF,
        LeftDoorR => RightDoorR,
        RearLeftQuarter => RearRightQuarter,
        FrameLeft => FrameRight,
        other => other,
    }
}

/// Name of a painted panel part for a region.
pub fn panel_name(r: BodyRegion) -> String {
    use BodyRegion::*;
    match r {
        FrontBumper => "bumper_front".into(),
        RearBumper => "bumper_rear".into(),
        Hood => "hood".into(),
        Trunk => "trunk".into(),
        Roof => "roof".into(),
        FrontLeftFender => "fender_fl".into(),
        FrontRightFender => "fender_fr".into(),
        LeftDoorF => "door_fl".into(),
        RightDoorF => "door_fr".into(),
        LeftDoorR => "door_rl".into(),
        RightDoorR => "door_rr".into(),
        RearLeftQuarter => "quarter_rl".into(),
        RearRightQuarter => "quarter_rr".into(),
        FrameLeft => "sill_l".into(),
        FrameRight => "sill_r".into(),
        Windshield => "windshield".into(),
        RearGlass => "rear_glass".into(),
        Cabin => "body".into(),
        Accessory => "accessory".into(),
        FrameFront => "frame_front".into(),
        FrameRear => "frame_rear".into(),
    }
}

/// What a lofted face is.
struct Class {
    region: BodyRegion,
    kind: PartKind,
    material: MaterialSlot,
    /// Custom part name stem (`None` = the region's panel name). Side-specific stems end in
    /// `_l` and are renamed `_r` on the right.
    name: Option<&'static str>,
}

impl Class {
    fn panel(region: BodyRegion) -> Class {
        Class {
            region,
            kind: PartKind::Panel,
            material: MaterialSlot::Paint,
            name: None,
        }
    }
    fn of(region: BodyRegion, kind: PartKind, material: MaterialSlot, name: &'static str) -> Class {
        Class {
            region,
            kind,
            material,
            name: Some(name),
        }
    }
}

/// Lower-body region of the LEFT side at `y`, height `z`.
pub fn lower_region(p: &BodyProfile, y: f32, z: f32) -> BodyRegion {
    use BodyRegion::*;
    let r = if y > p.y_bf {
        FrontBumper
    } else if y > p.y_fd_hinge {
        FrontLeftFender
    } else if y < p.y_quarter.1 {
        RearBumper
    } else if y < p.y_quarter.0 {
        RearLeftQuarter
    } else if z < p.z_door_bottom - 0.02 {
        FrameLeft
    } else if let Some((rh, re)) = p.rear_door {
        if y > rh {
            LeftDoorF
        } else if y > re {
            LeftDoorR
        } else {
            RearLeftQuarter
        }
    } else if y > p.y_fd_rear {
        LeftDoorF
    } else {
        LeftDoorR
    };
    // Bumper covers stop at their top; above it the fender / quarter continues.
    match r {
        FrontBumper if z > p.z_bumper_top.0 + 0.04 => FrontLeftFender,
        RearBumper if z > p.z_bumper_top.1 + 0.04 => RearLeftQuarter,
        other => other,
    }
}

/// Classify the LEFT-side loft face `j` (between ring points j and j+1) of the station
/// interval [y0, y1], mid height `z`.
fn classify_left(sh: &Shell, y0: f32, y1: f32, z: f32, j: usize) -> Class {
    use BodyRegion::*;
    let p = &sh.p;
    let y = 0.5 * (y0 + y1);
    let paint_or_trim = if p.s.black_pillars {
        MaterialSlot::Trim
    } else {
        MaterialSlot::Paint
    };
    let pillar = || Class::of(Cabin, PartKind::Body, paint_or_trim, "pillars");
    // Bed zone (decided by the rear station so the box front wall belongs to the box).
    if p.in_bed(y0)
        && matches!(p.s.bed, BedKind::Box | BedKind::Tipper)
        && p.s.bed_floor > 0.0
        && j <= 4
    {
        let q = RearLeftQuarter;
        return match j {
            0..=1 => Class::of(q, PartKind::Body, MaterialSlot::Trim, "rails_l"),
            2 => Class::of(q, PartKind::Body, MaterialSlot::Trim, "box_floor_l"),
            _ if p.s.bed == BedKind::Box => {
                Class::of(q, PartKind::Panel, MaterialSlot::Paint, "box_l")
            }
            _ => Class::of(q, PartKind::Panel, MaterialSlot::Paint, "tipper_l"),
        };
    }
    if p.in_bed(y0) && j >= 5 {
        let q = RearLeftQuarter;
        return match p.s.bed {
            BedKind::Open => match j {
                5 | 6 => Class::panel(q),
                _ => Class::of(q, PartKind::Body, MaterialSlot::Trim, "bed_floor_l"),
            },
            BedKind::Box => match j {
                5..=7 => Class::of(q, PartKind::Panel, MaterialSlot::Paint, "box_l"),
                _ => Class::of(q, PartKind::Panel, MaterialSlot::Paint, "box_roof_l"),
            },
            BedKind::Tipper => match j {
                5..=7 => Class::of(q, PartKind::Panel, MaterialSlot::Paint, "tipper_l"),
                _ => Class::of(q, PartKind::Body, MaterialSlot::Metal, "tipper_inner_l"),
            },
            BedKind::Rails => Class::of(q, PartKind::Body, MaterialSlot::Trim, "rails_l"),
            BedKind::Closed => Class::panel(q),
        };
    }
    if p.in_bed(y0) && p.s.bed == BedKind::Rails {
        return Class::of(
            RearLeftQuarter,
            PartKind::Body,
            MaterialSlot::Trim,
            "rails_l",
        );
    }
    if p.in_bed_gap(y) {
        return Class::of(
            RearLeftQuarter,
            PartKind::Body,
            MaterialSlot::Trim,
            "frame_gap_l",
        );
    }
    let gh = p.has_greenhouse(y);
    let lower = j <= 4 || (j == 5 && gh);
    if lower {
        let r = lower_region(p, y, z);
        return if r == FrameLeft {
            Class::of(r, PartKind::Body, MaterialSlot::Paint, "sill_l")
        } else {
            Class::panel(r)
        };
    }
    let (ry0, ry1, _, _) = p.roof;
    if gh && j == 6 {
        // Side glass: door windows ride with their door, fixed quarter glass is cabin.
        let margin = 0.06;
        let front_edge = (p.y_fd_hinge - margin).min(p.y_fw - 0.45 * (p.y_fw - ry1));
        let win = |a: f32, b: f32| y > a + margin && y < b;
        let glass = |r: BodyRegion, name: &'static str| {
            Class::of(r, PartKind::Glass, sh.side_glass(r), name)
        };
        let cab_rear = p.y_deck + 0.08;
        if p.s.blind_rear {
            let front_rear = p.rear_door.map_or(p.y_fd_rear, |(rh, _)| rh);
            if win(front_rear, front_edge) {
                return glass(LeftDoorF, "door_fl_glass");
            }
            let r = lower_region(p, y, p.z_belt - 0.05);
            return if r == Cabin {
                Class::of(Cabin, PartKind::Body, MaterialSlot::Paint, "cab_side_l")
            } else {
                Class::panel(r)
            };
        }
        if let Some((rh, re)) = p.rear_door {
            if win(rh, front_edge) {
                return glass(LeftDoorF, "door_fl_glass");
            }
            if win(re, rh - margin) {
                return glass(LeftDoorR, "door_rl_glass");
            }
            if win(cab_rear, re - margin) && re - cab_rear > 0.25 {
                return glass(Cabin, "quarter_glass_l");
            }
        } else {
            if win(p.y_fd_rear, front_edge) {
                return glass(LeftDoorF, "door_fl_glass");
            }
            if win(cab_rear.max(ry0 - 0.35), p.y_fd_rear - margin) {
                return glass(Cabin, "quarter_glass_l");
            }
        }
        return pillar();
    }
    // Top surfaces.
    if y >= p.y_fw {
        Class::panel(Hood)
    } else if y > ry1 {
        if j == 7 && gh {
            pillar()
        } else {
            Class::of(
                Windshield,
                PartKind::Glass,
                MaterialSlot::Glass,
                "windshield",
            )
        }
    } else if y >= ry0 {
        Class::panel(Roof)
    } else if y > p.y_deck {
        if p.rear_glass && !(j == 7 && gh) {
            let m = if p.s.bed == BedKind::Closed && sh.dark_rear {
                MaterialSlot::GlassDark
            } else {
                MaterialSlot::Glass
            };
            Class::of(RearGlass, PartKind::Glass, m, "rear_glass")
        } else if p.rear_glass {
            pillar()
        } else {
            Class::of(Cabin, PartKind::Body, MaterialSlot::Paint, "cab_back")
        }
    } else if p.trunk_lid {
        Class::panel(Trunk)
    } else {
        // Pickup bed cover / hatch deck: part of the quarters.
        Class::panel(RearLeftQuarter)
    }
}

/// Key of a class on one side.
fn key_of(c: &Class, right: bool) -> Key {
    let region = if right { mirror(c.region) } else { c.region };
    let name = match c.name {
        Some(n) if right && n.ends_with("_l") => format!("{}_r", &n[..n.len() - 2]),
        Some(n) if right && n.contains("_fl") => n.replace("_fl", "_fr"),
        Some(n) if right && n.contains("_rl") => n.replace("_rl", "_rr"),
        Some(n) => n.to_string(),
        None => panel_name(region),
    };
    (name, Some(region), c.kind, c.material)
}

/// Loft step for a body of `len` m (coarser on long vehicles to hold the budget).
pub fn station_step(len: f32) -> f32 {
    (len / 64.0).clamp(0.07, 0.13)
}

/// Generate the mesh for a car (skin binding left empty; see [`crate::skin::bind_skin`]).
pub fn generate(def: &VehicleDef, lattice: &LatticeDef, style: &CarStyle) -> CarMesh {
    let p = BodyProfile::measure(def, lattice, style);
    let sh = Shell::new(p.clone(), style);
    let ys = p.stations(station_step(p.length()));
    let rings: Vec<[[f32; 3]; RING_N]> = ys.iter().map(|&y| sh.ring(y)).collect();
    let k = RING_N;
    let len = p.length();
    let height = p.roof.2.max(p.z_belt) - p.z_floor;
    let uv = |q: [f32; 3]| {
        [
            0.5 + 0.5 * (q[1] - p.y_tail) / len,
            (q[2] - p.z_floor) / height,
        ]
    };
    let mut b = Builder::default();

    // Loft quads, both sides; the ring's last point is on the centre line.
    for s in 0..ys.len() - 1 {
        for right in [false, true] {
            let mx = if right { -1.0 } else { 1.0 };
            for j in 0..k - 1 {
                let a0 = rings[s][j];
                let b0 = rings[s + 1][j];
                let a1 = rings[s][j + 1];
                let b1 = rings[s + 1][j + 1];
                let zm = 0.25 * (a0[2] + b0[2] + a1[2] + b1[2]);
                let class = classify_left(&sh, ys[s], ys[s + 1], zm, j);
                let key = key_of(&class, right);
                let part = b.part(&key);
                let m = |q: [f32; 3]| [q[0] * mx, q[1], q[2]];
                let side = if right { 1 } else { 0 };
                let id = |ss: usize, jj: usize| Some((ss as i64 * 2 + side, jj as i64));
                let va = b.vert(part, id(s, j), m(a0), uv(a0));
                let vb = b.vert(part, id(s + 1, j), m(b0), uv(b0));
                let va1 = b.vert(part, id(s, j + 1), m(a1), uv(a1));
                let vb1 = b.vert(part, id(s + 1, j + 1), m(b1), uv(b1));
                if right {
                    b.tri(part, va, vb, va1);
                    b.tri(part, vb, vb1, va1);
                } else {
                    b.tri(part, va, va1, vb);
                    b.tri(part, vb, va1, vb1);
                }
            }
        }
    }

    // End caps: horizontal strips between mirrored ring points.
    for (s, front) in [(ys.len() - 1, true), (0usize, false)] {
        let ring = &rings[s];
        let y = ys[s];
        for j in 0..k - 1 {
            let l0 = ring[j];
            let l1 = ring[j + 1];
            let r0 = [-l0[0], l0[1], l0[2]];
            let r1 = [-l1[0], l1[1], l1[2]];
            let zm = 0.5 * (l0[2] + l1[2]);
            let key = if front {
                let region = if zm < p.z_bumper_top.0 + 0.04 {
                    BodyRegion::FrontBumper
                } else {
                    BodyRegion::Hood
                };
                (
                    panel_name(region),
                    Some(region),
                    PartKind::Panel,
                    MaterialSlot::Paint,
                )
            } else if p.in_bed(y) && p.s.bed == BedKind::Rails {
                // Bare rail ends: the cross-member is the rear bumper, the deck plate the
                // trunk (both regions exist in the lattice and own a panel).
                let region = if j >= 4 {
                    BodyRegion::Trunk
                } else {
                    BodyRegion::RearBumper
                };
                (
                    panel_name(region),
                    Some(region),
                    PartKind::Panel,
                    MaterialSlot::Trim,
                )
            } else if p.in_bed(y) && p.s.bed == BedKind::Open && j >= 7 {
                continue;
            } else {
                let region = if zm < p.z_bumper_top.1 + 0.04 {
                    BodyRegion::RearBumper
                } else {
                    BodyRegion::Trunk
                };
                (
                    panel_name(region),
                    Some(region),
                    PartKind::Panel,
                    MaterialSlot::Paint,
                )
            };
            if l1[0].abs() < 1e-6 {
                let q = [l0, l1, r0];
                b.poly(&key, &if front { q } else { [l0, r0, l1] });
            } else if front {
                b.poly(&key, &[l0, l1, r1, r0]);
            } else {
                b.poly(&key, &[l0, r0, r1, l1]);
            }
        }
    }

    // Underbody: one strip across every station.
    {
        let key: Key = (
            "underbody".into(),
            Some(BodyRegion::Cabin),
            PartKind::Body,
            MaterialSlot::Trim,
        );
        for s in 0..ys.len() - 1 {
            let a = rings[s][0];
            let c = rings[s + 1][0];
            b.poly(&key, &[a, [-a[0], a[1], a[2]], [-c[0], c[1], c[2]], c]);
        }
    }

    crate::parts::build(&mut b, &sh, style, def);

    let mut parts = b.finish();
    apply_paint_overrides(&mut parts, style);
    parts.extend(crate::wheels::wheel_parts(def, style));
    let tri_count = parts.iter().map(|p| p.tri_count() as u32).sum();
    let rgb = |s: &str| parse_rgb(s).unwrap_or(0x808080);
    CarMesh {
        id: def.id.clone(),
        lattice_id: lattice.id.clone(),
        format: MESH_FORMAT,
        tri_count,
        palette: MeshPalette {
            base: rgb(&style.palette.base),
            accent: rgb(&style.palette.accent),
            primer: rgb(&style.palette.primer),
            alts: style.palette.alts.iter().map(|s| rgb(s)).collect(),
        },
        parts,
    }
}

/// Re-material painted parts named in `paint_overrides` (exact names, or a `*` suffix
/// wildcard such as `door_*`).
fn apply_paint_overrides(parts: &mut [MeshPart], style: &CarStyle) {
    for part in parts.iter_mut() {
        if !matches!(
            part.material,
            MaterialSlot::Paint | MaterialSlot::Accent | MaterialSlot::Primer
        ) {
            continue;
        }
        for (pat, slot) in &style.paint_overrides {
            let hit = match pat.strip_suffix('*') {
                Some(prefix) => part.name.starts_with(prefix),
                None => part.name == *pat,
            };
            if hit {
                part.material = (*slot).into();
            }
        }
    }
}
