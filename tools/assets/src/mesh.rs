//! Procedural low-poly car mesh: a lofted chamfered body (cross-section rings along +Y with a
//! greenhouse and wheel arches), cut into one part per body region along the lattice's zone
//! boundaries, plus glass, lights, mirrors, a minimal interior and separate wheels.
//! Units m, +Y forward, +Z up, origin = chassis origin (mid-wheelbase on the ground).

use serde::{Deserialize, Serialize};
use wp_contracts::{BodyRegion, LatticeDef, VehicleDef};

use crate::profile::BodyProfile;

/// Mesh file format version (`CarMesh::format`).
pub const MESH_FORMAT: u32 = 1;
/// Station spacing of the loft, m.
pub const STATION_STEP_M: f32 = 0.06;
/// Triangle budget (prompt 11).
pub const TRI_BUDGET: (u32, u32) = (3000, 6000);

/// What a part is (renderer picks material and behaviour from it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    /// Painted body panel of `region`.
    Panel,
    /// Glass (windshield, rear glass, door windows).
    Glass,
    /// Head- or tail-light lens.
    Light,
    /// Door mirror.
    Mirror,
    /// Non-panel body (pillars, sills, underbody).
    Body,
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
    /// Window glass.
    Glass,
    /// Black plastic / trim / underbody.
    Trim,
    /// Headlight lens.
    Headlight,
    /// Tail-light lens.
    Taillight,
    /// Tire rubber.
    Rubber,
    /// Rim metal.
    Metal,
    /// Interior.
    Interior,
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
}

/// Where a lofted face sits on the cross-section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Band {
    /// Below the belt (side panels).
    Lower,
    /// Greenhouse side (windows, pillars).
    Greenhouse,
    /// Top surfaces (hood, roof, glass, deck).
    Top,
}

/// Part key while building: (name, region, kind, material).
type Key = (String, Option<BodyRegion>, PartKind, MaterialSlot);

/// One part under construction: key, positions, uvs, indices, vertex dedupe map.
type PartBuild = (
    Key,
    Vec<[f32; 3]>,
    Vec<[f32; 2]>,
    Vec<u32>,
    std::collections::BTreeMap<(i64, i64), u32>,
);

/// Collects triangles per part with per-part vertex dedupe.
#[derive(Default)]
struct Builder {
    parts: Vec<PartBuild>,
}

impl Builder {
    fn part(&mut self, key: &Key) -> usize {
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
    fn vert(&mut self, part: usize, id: Option<(i64, i64)>, p: [f32; 3], uv: [f32; 2]) -> u32 {
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

    fn tri(&mut self, part: usize, a: u32, b: u32, c: u32) {
        self.parts[part].3.extend_from_slice(&[a, b, c]);
    }

    /// Box from 8 corners `lo`–`hi` (axis aligned), flat-shaded (own vertices).
    fn cube(&mut self, key: &Key, lo: [f32; 3], hi: [f32; 3]) {
        let part = self.part(key);
        let c = |i: usize| -> [f32; 3] {
            [
                if i & 1 == 0 { lo[0] } else { hi[0] },
                if i & 2 == 0 { lo[1] } else { hi[1] },
                if i & 4 == 0 { lo[2] } else { hi[2] },
            ]
        };
        // Faces as corner quads, CCW seen from outside.
        let faces = [
            [0, 4, 6, 2], // -X
            [1, 3, 7, 5], // +X
            [0, 1, 5, 4], // -Y
            [2, 6, 7, 3], // +Y
            [0, 2, 3, 1], // -Z
            [4, 5, 7, 6], // +Z
        ];
        for f in faces {
            let v: Vec<u32> = f
                .iter()
                .map(|&i| self.vert(part, None, c(i), [0.5, 0.5]))
                .collect();
            self.tri(part, v[0], v[1], v[2]);
            self.tri(part, v[0], v[2], v[3]);
        }
    }
}

/// Mirror a left-side region to the right side.
fn mirror(r: BodyRegion) -> BodyRegion {
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

/// Side suffix for part names.
fn side_name(base: &str, right: bool) -> String {
    let s = if right { "r" } else { "l" };
    match base {
        "door_f" => format!("door_f{s}"),
        "door_r" => format!("door_r{s}"),
        other => format!("{other}_{s}"),
    }
}

/// Name of a painted panel part for a region.
fn panel_name(r: BodyRegion) -> String {
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

/// Classify a lofted face of the LEFT side at mid `y`, mid height `z` in `band`.
fn classify_left(p: &BodyProfile, y: f32, z: f32, band: Band) -> (BodyRegion, PartKind) {
    use BodyRegion::*;
    let lower = |y: f32| -> BodyRegion {
        if y > p.y_bf {
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
        }
    };
    match band {
        Band::Lower => {
            let r = lower(y);
            // Bumper covers stop at their top; above it the fender / quarter continues.
            let r = match r {
                FrontBumper if z > p.z_bumper_top.0 + 0.04 => FrontLeftFender,
                RearBumper if z > p.z_bumper_top.1 + 0.04 => RearLeftQuarter,
                other => other,
            };
            let kind = if r == FrameLeft {
                PartKind::Body
            } else {
                PartKind::Panel
            };
            (r, kind)
        }
        Band::Greenhouse => {
            // Door windows ride with their door; pillars and quarter windows are body.
            let margin = 0.06;
            let window = |a: f32, b: f32| y > a + margin && y < b - margin;
            if let Some((rh, re)) = p.rear_door {
                if window(rh, p.y_fd_hinge) {
                    return (LeftDoorF, PartKind::Glass);
                }
                if window(re, rh) {
                    return (LeftDoorR, PartKind::Glass);
                }
            } else if window(p.y_fd_rear, p.y_fd_hinge) {
                return (LeftDoorF, PartKind::Glass);
            }
            (Cabin, PartKind::Body)
        }
        Band::Top => {
            let (ry0, ry1, _, _) = p.roof;
            if y >= p.y_fw {
                (Hood, PartKind::Panel)
            } else if y > ry1 {
                (Windshield, PartKind::Glass)
            } else if y >= ry0 {
                (Roof, PartKind::Panel)
            } else if y > p.y_deck {
                if p.rear_glass {
                    (RearGlass, PartKind::Glass)
                } else {
                    (Cabin, PartKind::Body)
                }
            } else if p.trunk_lid {
                (Trunk, PartKind::Panel)
            } else {
                // Pickup bed cover / hatch deck: part of the quarters.
                (RearLeftQuarter, PartKind::Panel)
            }
        }
    }
}

fn material_of(kind: PartKind) -> MaterialSlot {
    match kind {
        PartKind::Panel => MaterialSlot::Paint,
        PartKind::Glass => MaterialSlot::Glass,
        PartKind::Light => MaterialSlot::Headlight,
        PartKind::Mirror => MaterialSlot::Paint,
        PartKind::Body => MaterialSlot::Paint,
        PartKind::Interior => MaterialSlot::Interior,
        PartKind::Tire => MaterialSlot::Rubber,
        PartKind::Rim => MaterialSlot::Metal,
    }
}

fn key_for(region: BodyRegion, kind: PartKind) -> Key {
    let mut name = panel_name(region);
    let material = match (region, kind) {
        (BodyRegion::Windshield | BodyRegion::RearGlass, _) => MaterialSlot::Glass,
        (_, PartKind::Glass) => {
            name.push_str("_glass");
            MaterialSlot::Glass
        }
        _ => material_of(kind),
    };
    (name, Some(region), kind, material)
}

/// Cross-section ring of the left half, bottom → top (x ≤ 0), at station `y`.
fn ring_left(p: &BodyProfile, y: f32) -> Vec<([f32; 3], Band)> {
    let w = p.half_width(y);
    let sill = p.sill(y);
    let belt = p.belt(y);
    let top = p.top(y);
    let ch = 0.06;
    let gh = p.has_greenhouse(y);
    let gb = w - 0.10;
    let rw = if gh { p.roof_half_w() } else { gb - 0.12 };
    let crown = 0.02;
    let mut pts = vec![
        ([-(w - ch), y, sill], Band::Lower),
        ([-w, y, sill + ch], Band::Lower),
        ([-w, y, sill + 0.45 * (belt - sill)], Band::Lower),
        ([-w, y, belt - ch], Band::Lower),
        ([-(w - 0.5 * ch), y, belt], Band::Lower),
        ([-gb, y, belt + 0.01], Band::Greenhouse),
        ([-rw, y, top - if gh { ch } else { 0.0 }], Band::Top),
        ([-(rw - if gh { ch } else { 0.05 }), y, top], Band::Top),
        ([-0.4 * rw, y, top + crown], Band::Top),
    ];
    // Wheel arches: side points below the arch move up onto it.
    if let Some(arch) = p.arch_floor(y) {
        let arch = arch.min(belt - ch - 0.02);
        for (pt, _) in pts.iter_mut().take(3) {
            if pt[2] < arch {
                pt[2] = arch;
                pt[0] = -w;
            }
        }
    }
    pts
}

/// Generate the mesh for a car (skin binding left empty; see [`crate::skin::bind_skin`]).
pub fn generate(def: &VehicleDef, lattice: &LatticeDef) -> CarMesh {
    let p = BodyProfile::measure(def, lattice);
    let ys = p.stations(STATION_STEP_M);
    let rings: Vec<Vec<([f32; 3], Band)>> = ys.iter().map(|&y| ring_left(&p, y)).collect();
    let k = rings[0].len();
    let len = p.y_nose - p.y_tail;
    let height = p.roof.2.max(p.z_belt) - p.z_floor;
    let uv = |q: [f32; 3]| {
        [
            0.5 + 0.5 * (q[1] - p.y_tail) / len,
            (q[2] - p.z_floor) / height,
        ]
    };
    let mut b = Builder::default();

    // Loft quads, both sides. Centre column (x = 0) closes the top.
    for s in 0..ys.len() - 1 {
        for right in [false, true] {
            let mx = if right { -1.0 } else { 1.0 };
            for j in 0..k {
                let a0 = rings[s][j].0;
                let b0 = rings[s + 1][j].0;
                let (a1, b1, band) = if j + 1 < k {
                    (rings[s][j + 1].0, rings[s + 1][j + 1].0, rings[s][j + 1].1)
                } else {
                    (
                        [0.0, ys[s], rings[s][j].0[2] + 0.004],
                        [0.0, ys[s + 1], rings[s + 1][j].0[2] + 0.004],
                        Band::Top,
                    )
                };
                let band =
                    if band == Band::Greenhouse && !p.has_greenhouse(0.5 * (ys[s] + ys[s + 1])) {
                        Band::Top
                    } else {
                        band
                    };
                let ym = 0.5 * (ys[s] + ys[s + 1]);
                let zm = 0.25 * (a0[2] + b0[2] + a1[2] + b1[2]);
                let (region, kind) = classify_left(&p, ym, zm, band);
                let region = if right { mirror(region) } else { region };
                let key = key_for(region, kind);
                let part = b.part(&key);
                let m = |q: [f32; 3]| [q[0] * mx, q[1], q[2]];
                let jid = |jj: usize| if jj < k { jj as i64 } else { 1000 };
                let side = if right { 1 } else { 0 };
                let va = b.vert(part, Some((s as i64 * 2 + side, jid(j))), m(a0), uv(a0));
                let vb = b.vert(
                    part,
                    Some(((s as i64 + 1) * 2 + side, jid(j))),
                    m(b0),
                    uv(b0),
                );
                let va1 = b.vert(part, Some((s as i64 * 2 + side, jid(j + 1))), m(a1), uv(a1));
                let vb1 = b.vert(
                    part,
                    Some(((s as i64 + 1) * 2 + side, jid(j + 1))),
                    m(b1),
                    uv(b1),
                );
                // Left side: outward normal is −X/up; CCW seen from outside.
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
        for j in 0..k - 1 {
            let l0 = ring[j].0;
            let l1 = ring[j + 1].0;
            let r0 = [-l0[0], l0[1], l0[2]];
            let r1 = [-l1[0], l1[1], l1[2]];
            let zm = 0.5 * (l0[2] + l1[2]);
            let region = if front {
                if zm < p.z_bumper_top.0 + 0.04 {
                    BodyRegion::FrontBumper
                } else {
                    BodyRegion::Hood
                }
            } else if zm < p.z_bumper_top.1 + 0.04 {
                BodyRegion::RearBumper
            } else {
                BodyRegion::Trunk
            };
            let part = b.part(&key_for(region, PartKind::Panel));
            let v: Vec<u32> = [l0, l1, r1, r0]
                .iter()
                .map(|&q| b.vert(part, None, q, uv(q)))
                .collect();
            if front {
                b.tri(part, v[0], v[1], v[3]);
                b.tri(part, v[1], v[2], v[3]);
            } else {
                b.tri(part, v[0], v[3], v[1]);
                b.tri(part, v[1], v[3], v[2]);
            }
        }
        // Close the crown: last ring point to the centre on both sides.
        let l = ring[k - 1].0;
        let c = [0.0, l[1], l[2] + 0.004];
        let region = if front {
            BodyRegion::Hood
        } else {
            BodyRegion::Trunk
        };
        let part = b.part(&key_for(region, PartKind::Panel));
        let v: Vec<u32> = [l, c, [-l[0], l[1], l[2]]]
            .iter()
            .map(|&q| b.vert(part, None, q, uv(q)))
            .collect();
        if front {
            b.tri(part, v[0], v[1], v[2]);
        } else {
            b.tri(part, v[0], v[2], v[1]);
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
        let part = b.part(&key);
        for s in 0..ys.len() - 1 {
            let a = rings[s][0].0;
            let c = rings[s + 1][0].0;
            let q = [a, c, [-c[0], c[1], c[2]], [-a[0], a[1], a[2]]];
            let v: Vec<u32> = q
                .iter()
                .map(|&x| b.vert(part, None, x, [0.0, 0.0]))
                .collect();
            b.tri(part, v[0], v[1], v[3]);
            b.tri(part, v[1], v[2], v[3]);
        }
    }

    // Lights (bound to the fenders / quarters so a hanging bumper leaves them in place).
    let hl_z = p.belt(p.y_nose - 0.05) - 0.07;
    let tl_z = p.belt(p.y_tail + 0.05) - 0.08;
    for right in [false, true] {
        let sx = if right { 1.0 } else { -1.0 };
        let wf = p.half_width(p.y_nose - 0.04);
        let x0 = sx * (wf - 0.08);
        let x1 = sx * (wf - 0.40);
        let (fl, rl) = if right {
            (BodyRegion::FrontRightFender, BodyRegion::RearRightQuarter)
        } else {
            (BodyRegion::FrontLeftFender, BodyRegion::RearLeftQuarter)
        };
        b.cube(
            &(
                side_name("headlight", right),
                Some(fl),
                PartKind::Light,
                MaterialSlot::Headlight,
            ),
            [x0.min(x1), p.y_nose - 0.07, hl_z - 0.05],
            [x0.max(x1), p.y_nose + 0.005, hl_z + 0.05],
        );
        let wt = p.half_width(p.y_tail + 0.04);
        let t0 = sx * (wt - 0.06);
        let t1 = sx * (wt - 0.36);
        b.cube(
            &(
                side_name("taillight", right),
                Some(rl),
                PartKind::Light,
                MaterialSlot::Taillight,
            ),
            [t0.min(t1), p.y_tail - 0.005, tl_z - 0.05],
            [t0.max(t1), p.y_tail + 0.07, tl_z + 0.05],
        );
        // Mirror on the front door, just behind its hinge at the belt.
        let door = if right {
            BodyRegion::RightDoorF
        } else {
            BodyRegion::LeftDoorF
        };
        let w = p.half_width(p.y_fd_hinge);
        let mx0 = sx * (w + 0.02);
        let mx1 = sx * (w + 0.17);
        b.cube(
            &(
                side_name("mirror", right),
                Some(door),
                PartKind::Mirror,
                MaterialSlot::Paint,
            ),
            [mx0.min(mx1), p.y_fd_hinge - 0.20, p.z_belt + 0.03],
            [mx0.max(mx1), p.y_fd_hinge - 0.10, p.z_belt + 0.13],
        );
    }

    // Minimal interior: dash + seat block, dark, visible through the glass.
    {
        let key: Key = (
            "interior".into(),
            Some(BodyRegion::Cabin),
            PartKind::Interior,
            MaterialSlot::Interior,
        );
        let w = p.half_w - 0.18;
        let cab_rear = p.y_deck.max(p.y_quarter.0);
        b.cube(
            &key,
            [-w, p.y_fw - 0.35, p.z_floor + 0.25],
            [w, p.y_fw - 0.05, p.z_belt + 0.02],
        );
        b.cube(
            &key,
            [-w, cab_rear + 0.15, p.z_floor + 0.1],
            [w, p.y_fw - 0.9, p.z_belt - 0.05],
        );
    }

    let mut parts: Vec<MeshPart> = Vec::new();
    for ((name, region, kind, material), pos, uvs, idx, _) in b.parts {
        if idx.is_empty() {
            continue;
        }
        parts.push(MeshPart {
            name,
            region,
            kind,
            material,
            wheel: None,
            positions_mm: pos
                .iter()
                .flat_map(|q| q.iter().map(|v| (v * 1000.0).round() as i32))
                .collect(),
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
    for (i, w) in def.wheels.iter().enumerate() {
        parts.extend(wheel_parts(i as u8, w.radius_m, w.width_m));
    }
    let tri_count = parts.iter().map(|p| p.tri_count() as u32).sum();
    CarMesh {
        id: def.id.clone(),
        lattice_id: lattice.id.clone(),
        format: MESH_FORMAT,
        tri_count,
        parts,
    }
}

const WHEEL_NAMES: [&str; 4] = ["fl", "fr", "rl", "rr"];
/// Tire segments around the axle.
const WHEEL_SEGMENTS: usize = 20;

/// Tire (tread + sidewalls) and a star-shaped rim (so spin is visible), hub space, axle +X.
fn wheel_parts(index: u8, radius: f32, width: f32) -> Vec<MeshPart> {
    let n = WHEEL_SEGMENTS;
    let hw = 0.5 * width;
    let r_in = radius * 0.64;
    let ang = |i: usize| {
        let (s, c) = wp_math::sin_cos(std::f32::consts::TAU * i as f32 / n as f32);
        (s, c)
    };
    let mut pos: Vec<[f32; 3]> = Vec::new();
    let mut idx: Vec<u32> = Vec::new();
    for i in 0..n {
        let (s, c) = ang(i);
        for (x, r) in [(-hw, r_in), (-hw, radius), (hw, radius), (hw, r_in)] {
            pos.push([x, s * r, c * r]);
        }
    }
    for i in 0..n {
        let a = (i * 4) as u32;
        let b = (((i + 1) % n) * 4) as u32;
        // −X sidewall, tread, +X sidewall: quads (a+k, a+k+1, b+k+1, b+k).
        for kk in 0..3u32 {
            let (p0, p1, p2, p3) = (a + kk, a + kk + 1, b + kk + 1, b + kk);
            idx.extend_from_slice(&[p0, p2, p1, p0, p3, p2]);
        }
    }
    let tire = MeshPart {
        name: format!("wheel_{}_tire", WHEEL_NAMES[index as usize]),
        region: None,
        kind: PartKind::Tire,
        material: MaterialSlot::Rubber,
        wheel: Some(index),
        positions_mm: pos
            .iter()
            .flat_map(|q| q.iter().map(|v| (v * 1000.0).round() as i32))
            .collect(),
        uvs: vec![32768; pos.len() * 2],
        indices: idx,
        skin_nodes: Vec::new(),
        skin_weights: Vec::new(),
    };
    // Rim: 10-point star on both faces at the sidewall inner radius.
    let mut rpos: Vec<[f32; 3]> = Vec::new();
    let mut ridx: Vec<u32> = Vec::new();
    for x in [-hw * 0.6, hw * 0.6] {
        let c0 = rpos.len() as u32;
        rpos.push([x, 0.0, 0.0]);
        for i in 0..10 {
            let (s, c) = wp_math::sin_cos(std::f32::consts::TAU * i as f32 / 10.0);
            let r = if i % 2 == 0 { r_in * 1.02 } else { r_in * 0.55 };
            rpos.push([x, s * r, c * r]);
        }
        for i in 0..10u32 {
            let a = c0 + 1 + i;
            let b = c0 + 1 + (i + 1) % 10;
            if x > 0.0 {
                ridx.extend_from_slice(&[c0, a, b]);
            } else {
                ridx.extend_from_slice(&[c0, b, a]);
            }
        }
    }
    let rim = MeshPart {
        name: format!("wheel_{}_rim", WHEEL_NAMES[index as usize]),
        region: None,
        kind: PartKind::Rim,
        material: MaterialSlot::Metal,
        wheel: Some(index),
        positions_mm: rpos
            .iter()
            .flat_map(|q| q.iter().map(|v| (v * 1000.0).round() as i32))
            .collect(),
        uvs: vec![32768; rpos.len() * 2],
        indices: ridx,
        skin_nodes: Vec::new(),
        skin_weights: Vec::new(),
    };
    vec![tire, rim]
}
