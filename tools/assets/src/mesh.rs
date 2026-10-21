//! Procedural low-poly car mesh: a lofted body (cross-section rings along +Y from
//! [`BodyProfile::ring`], shaped by the car's [`BodyStyle`]), cut into one part per body
//! region along the lattice's zone boundaries, plus glass, lights, mirrors, a minimal
//! interior, style details, mechanical parts and wheels.
//! Units m, +Y forward, +Z up, origin = chassis origin (mid-wheelbase on the ground).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::{BodyRegion, LatticeDef, VehicleDef};

use crate::builder::{mirror_region, Builder, PartKey, Patch, PatchBand};
use crate::profile::{BodyProfile, RingPt, Seg, BED_WALL_EPS, RING_POINTS, RING_SHOULDER};
use crate::style::{style_for, CarStyle, Detail, DetailCtx, HoodScoop, LiveryHint, Shape, Tier};
use crate::wheels::WheelGeom;

/// Mesh file format version (`CarMesh::format`). 2: new part kinds / materials, `rigid`,
/// `tints`, `livery`, `atlas`, `wheel_camber_deg`, livery atlas UVs on paint parts.
pub const MESH_FORMAT: u32 = 2;
/// Hard overall triangle range of any car (per-tier budgets: [`Tier::tri_budget`]).
pub const TRI_BUDGET: (u32, u32) = (4000, 20000);
/// Window margin (pillar half width) around door edges, m.
const WINDOW_MARGIN: f32 = 0.06;

/// Loft resolution of a tier: (base station step, fine step near ends / arches), m.
pub fn loft_steps(tier: Tier) -> (f32, f32) {
    match tier {
        Tier::Hero => (0.07, 0.036),
        Tier::Traffic => (0.11, 0.065),
    }
}

/// What a part is (renderer picks material and behaviour from it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartKind {
    /// Painted body panel of `region`.
    Panel,
    /// Glass (windshield, rear glass, door / quarter windows).
    Glass,
    /// Head- or tail-light lens.
    Light,
    /// Door mirror.
    Mirror,
    /// Non-panel body (pillars, sills, underbody).
    Body,
    /// Minimal interior shell.
    Interior,
    /// Tire (wheel space, spins).
    Tire,
    /// Rim (wheel space, spins).
    Rim,
    /// Exterior add-on (scoop, spoiler, splitter, grille, trim, push bar …), skinned.
    Detail,
    /// Engine (body-fixed, rigid; seen with the hood open / gone or through scoops).
    Engine,
    /// Suspension / exhaust / frame hardware (body-fixed, rigid).
    Suspension,
    /// Brake disc (wheel space, spins).
    BrakeDisc,
    /// Brake caliper (wheel space; steers and follows compression, does not spin).
    Caliper,
    /// Upright / knuckle / lower damper half (wheel space; like `Caliper`).
    Upright,
    /// Light-bar body or lens (skinned or rigid to the roof).
    Lightbar,
    /// Thin quad carrying a decal texture (unit numbers), 2 mm off the loft.
    DecalCarrier,
}

impl PartKind {
    /// Wheel-space kinds (positions relative to the hub, axle +X, not skinned).
    pub fn is_wheel_space(self) -> bool {
        matches!(
            self,
            PartKind::Tire
                | PartKind::Rim
                | PartKind::BrakeDisc
                | PartKind::Caliper
                | PartKind::Upright
        )
    }

    /// Wheel-space kinds parented to the wheel pivot (steer + compression, no spin).
    pub fn is_pivot(self) -> bool {
        matches!(self, PartKind::Caliper | PartKind::Upright)
    }
}

/// Material slot names the renderer maps to its palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MaterialSlot {
    /// Car paint (body colour, never textured).
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
    /// Paint that samples the livery texture (body colour when no livery is set).
    Livery,
    /// Chrome.
    Chrome,
    /// Carbon fibre.
    Carbon,
    /// Brake caliper (tinted per car: `tints.caliper`).
    Caliper,
    /// Dark engine metal.
    Engine,
    /// Light-bar red lens (renderer flashes it).
    LightbarRed,
    /// Light-bar blue lens.
    LightbarBlue,
    /// Light-bar clear / white lens.
    LightbarClear,
    /// Amber emissive lens (indicators, beacons).
    EmissiveAmber,
    /// Grille / mesh openings (dark).
    Grille,
    /// Exhaust tips (polished, heat tint).
    Exhaust,
    /// Rim / hubcap finish (defaults to the `metal` colour; tinted per car by
    /// `WheelStyle::rim_rgb` → `tints.rim`).
    Rim,
    /// Coil springs (tinted by `MechStyle::spring_rgb`, else the caliper tint → `tints.spring`).
    Spring,
    /// Second paint colour for stripes / accent bars (`CarStyle::accent_rgb` → `tints.accent`).
    Accent,
    /// Emissive underglow strip (`CarStyle::underglow_rgb` → `tints.underglow`); the renderer
    /// lights it only while the headlights are on.
    Underglow,
}

impl MaterialSlot {
    /// Painted slots get livery-atlas UVs.
    pub fn is_paint(self) -> bool {
        matches!(self, MaterialSlot::Paint | MaterialSlot::Livery)
    }
}

/// One mesh part. Vertex attributes are quantised: positions in mm, UVs in 1/65535, skin
/// weights in 1/255 (summing to 255 per vertex).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MeshPart {
    /// Unique part name (`door_fl`, `door_fl_glass`, `wheel_fl_tire`, …).
    pub name: String,
    /// Body region the part belongs to and is skinned to; `None` for wheel-space parts.
    pub region: Option<BodyRegion>,
    /// Part kind.
    pub kind: PartKind,
    /// Material slot.
    pub material: MaterialSlot,
    /// Wheel index (FL, FR, RL, RR) for wheel-space parts; their positions are relative to
    /// the hub with the axle along +X.
    #[serde(default)]
    pub wheel: Option<u8>,
    /// Rigid part: every vertex is bound to the same single node (weight 255).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub rigid: bool,
    /// xyz per vertex, mm (chassis space; wheel parts: hub space).
    pub positions_mm: Vec<i32>,
    /// uv per vertex in 1/65535. Paint/livery parts: livery atlas in U 0–0.5 (see
    /// `data/panels/README.md`); other parts: U 0.5–1 side projection.
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

/// Car-space frame of the livery atlas (see `data/panels/README.md`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LiveryAtlas {
    /// Tail y, nose y (m).
    pub y: [f32; 2],
    /// Ground z, top z (m).
    pub z: [f32; 2],
    /// Half width covered (m).
    pub x_half: f32,
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
    /// Per-car material colours (sRGB), e.g. `{"caliper": [200, 30, 30]}`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub tints: BTreeMap<MaterialSlot, [u8; 3]>,
    /// Livery hint for the renderer's livery painter.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub livery: Option<LiveryHint>,
    /// Livery atlas frame (paint UVs map this box).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atlas: Option<LiveryAtlas>,
    /// Visual camber per wheel (FL FR RL RR), degrees, + = top leans in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wheel_camber_deg: Option<[f32; 4]>,
    /// Parts in a stable order.
    pub parts: Vec<MeshPart>,
}

impl CarMesh {
    /// Serialise one part per line (compact but diffable per part).
    pub fn to_json(&self) -> String {
        let mut s = String::new();
        let j = |v: &dyn erased::Ser| v.json();
        s.push_str("{\n");
        s.push_str(&format!("  \"id\": {},\n", j(&self.id)));
        s.push_str(&format!("  \"lattice_id\": {},\n", j(&self.lattice_id)));
        s.push_str(&format!("  \"format\": {},\n", self.format));
        s.push_str(&format!("  \"tri_count\": {},\n", self.tri_count));
        if !self.tints.is_empty() {
            s.push_str(&format!("  \"tints\": {},\n", j(&self.tints)));
        }
        if let Some(l) = &self.livery {
            s.push_str(&format!("  \"livery\": {},\n", j(l)));
        }
        if let Some(a) = &self.atlas {
            s.push_str(&format!("  \"atlas\": {},\n", j(a)));
        }
        if let Some(c) = &self.wheel_camber_deg {
            s.push_str(&format!("  \"wheel_camber_deg\": {},\n", j(c)));
        }
        s.push_str("  \"parts\": [\n");
        for (i, p) in self.parts.iter().enumerate() {
            s.push_str("    ");
            s.push_str(&j(p));
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

mod erased {
    /// Tiny helper so `to_json` can serialise heterogeneous values compactly.
    pub trait Ser {
        fn json(&self) -> String;
    }
    impl<T: serde::Serialize> Ser for T {
        fn json(&self) -> String {
            serde_json::to_string(self).expect("serialisable")
        }
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

/// Key of a loft part of `region`/`kind` (painted exterior → `Livery`).
fn key_for(region: BodyRegion, kind: PartKind) -> PartKey {
    let mut name = panel_name(region);
    let material = match (region, kind) {
        (BodyRegion::Windshield | BodyRegion::RearGlass, _) => MaterialSlot::Glass,
        (_, PartKind::Glass) => {
            name.push_str("_glass");
            MaterialSlot::Glass
        }
        (_, PartKind::Panel | PartKind::Body) => MaterialSlot::Livery,
        (_, PartKind::Interior) => MaterialSlot::Interior,
        _ => MaterialSlot::Paint,
    };
    PartKey::body(&name, region, kind, material)
}

/// Right-side key from a left-side key: region mirrored; centre regions keep the key.
fn right_key(k: &PartKey) -> PartKey {
    match k.region {
        Some(r) if mirror_region(r) != r => PartKey {
            name: panel_name_or_mirror(k, mirror_region(r)),
            region: Some(mirror_region(r)),
            ..k.clone()
        },
        _ => k.clone(),
    }
}

fn panel_name_or_mirror(k: &PartKey, r: BodyRegion) -> String {
    let left = panel_name(k.region.unwrap_or(r));
    if let Some(rest) = k.name.strip_prefix(&left) {
        format!("{}{rest}", panel_name(r))
    } else {
        crate::builder::mirror_name(&k.name)
    }
}

/// Door / quarter window y ranges (glass between `a + margin` and `b − margin`).
struct Windows {
    front: (f32, f32),
    rear: Option<(f32, f32)>,
    quarter: Option<(f32, f32)>,
}

fn windows(p: &BodyProfile) -> Windows {
    let m = WINDOW_MARGIN;
    let (front, rear, last) = match p.rear_door {
        Some((rh, re)) => ((rh + m, p.y_fd_hinge - m), Some((re + m, rh - m)), re),
        None => ((p.y_fd_rear + m, p.y_fd_hinge - m), None, p.y_fd_rear),
    };
    let q0 = p.roof.0 + 0.08;
    let q1 = last - 0.04;
    let van = p.style.silhouette == crate::style::Silhouette::Van;
    let quarter = (!van && q1 - q0 > 0.2).then_some((q0, q1));
    Windows {
        front,
        rear: if van { None } else { rear },
        quarter,
    }
}

/// Classify a lofted face of the LEFT side at mid `y`, mid height `z`, segment kind `seg`.
fn classify_left(p: &BodyProfile, win: &Windows, y: f32, z: f32, seg: Seg) -> PartKey {
    use BodyRegion::*;
    let lower = |y: f32, z: f32| -> BodyRegion {
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
    let panel = |r: BodyRegion| {
        let kind = if matches!(r, FrameLeft) {
            PartKind::Body
        } else {
            PartKind::Panel
        };
        key_for(r, kind)
    };
    match seg {
        Seg::Lower => {
            let r = match lower(y, z) {
                FrontBumper if z > p.z_bumper_top.0 + 0.04 => FrontLeftFender,
                RearBumper if z > p.z_bumper_top.1 + 0.04 => RearLeftQuarter,
                other => other,
            };
            panel(r)
        }
        Seg::Ledge => panel(lower(y, f32::MAX)),
        Seg::Glass => {
            if y < p.y_deck {
                // Fastback sail over the trunk zone: part of the quarter.
                return panel(lower(y, f32::MAX));
            }
            let inside = |r: (f32, f32)| y > r.0 && y < r.1;
            if inside(win.front) {
                return key_for(LeftDoorF, PartKind::Glass);
            }
            if let Some((rh, re)) = p.rear_door {
                if y < rh && y > re {
                    return match win.rear {
                        Some(r) if inside(r) => key_for(LeftDoorR, PartKind::Glass),
                        // Van: windowless rear (sliding) door, painted.
                        None => key_for(LeftDoorR, PartKind::Panel),
                        Some(_) => key_for(Cabin, PartKind::Body),
                    };
                }
            }
            if let Some(r) = win.quarter {
                if inside(r) {
                    return PartKey::body(
                        "quarter_glass",
                        Cabin,
                        PartKind::Glass,
                        MaterialSlot::Glass,
                    );
                }
            }
            key_for(Cabin, PartKind::Body)
        }
        Seg::Top => {
            let (ry0, ry1, _, _) = p.roof;
            if y >= p.y_fw {
                key_for(Hood, PartKind::Panel)
            } else if y > ry1 {
                key_for(Windshield, PartKind::Glass)
            } else if y >= ry0 {
                key_for(Roof, PartKind::Panel)
            } else if y > p.y_deck {
                if p.rear_glass {
                    key_for(RearGlass, PartKind::Glass)
                } else {
                    key_for(Cabin, PartKind::Body)
                }
            } else if p.trunk_lid {
                key_for(Trunk, PartKind::Panel)
            } else if matches!(p.bed, Some((y0, _, _)) if y < y0 + BED_WALL_EPS) {
                // Tailgate top.
                key_for(Trunk, PartKind::Panel)
            } else {
                // Pickup bed rail / cover, hatch deck: part of the quarters.
                key_for(RearLeftQuarter, PartKind::Panel)
            }
        }
        Seg::BedWall => key_for(RearLeftQuarter, PartKind::Panel),
        Seg::BedFloor => PartKey::body("bed_floor", FrameRear, PartKind::Body, MaterialSlot::Trim),
    }
}

/// Loft stations: base steps, fine steps near the ends and arches, plus every zone
/// boundary (so panel splits fall on station edges), ascending.
pub fn stations(p: &BodyProfile, base: f32, fine: f32) -> Vec<f32> {
    let s = &p.style;
    let mut zones: Vec<(f32, f32)> = vec![
        (p.y_nose - p.nose_run - 0.05, p.y_nose + 1.0),
        (
            p.y_tail - 1.0,
            p.y_tail
                + s.tail_round.clamp(0.05, 0.6)
                + 0.05
                + if s.ducktail_h.abs() > 1e-3 { 0.3 } else { 0.0 },
        ),
    ];
    for &(wy, _, r) in &p.arches {
        zones.push((wy - r - 0.15, wy + r + 0.15));
    }
    let fine_at = |y: f32| zones.iter().any(|&(a, b)| y >= a && y <= b);
    let mut ys = vec![p.y_tail];
    let mut y = p.y_tail;
    while y < p.y_nose - 1e-4 {
        let step = if fine_at(y) { fine } else { base };
        // Shrink the last step so the nose lands exactly.
        y = (y + step).min(p.y_nose);
        if p.y_nose - y < 0.3 * step {
            y = p.y_nose;
        }
        ys.push(y);
    }
    let win = windows(p);
    let mut keys = vec![
        p.y_fw,
        p.y_bf,
        p.roof.0,
        p.roof.1,
        p.y_deck,
        p.y_quarter.0,
        p.y_quarter.1,
        p.y_fd_hinge,
        p.y_fd_rear,
        win.front.0,
        win.front.1,
    ];
    if let Some((h, e)) = p.rear_door {
        keys.push(h);
        keys.push(e);
    }
    for r in [win.rear, win.quarter].into_iter().flatten() {
        keys.push(r.0);
        keys.push(r.1);
    }
    for &(wy, _, r) in &p.arches {
        keys.push(wy - r);
        keys.push(wy + r);
    }
    let add_keys = |ys: &mut Vec<f32>, keys: &[f32], keep_out: f32| {
        for &k in keys {
            if k > p.y_tail + 0.02 && k < p.y_nose - 0.02 {
                ys.retain(|y| (y - k).abs() > keep_out);
                ys.push(k);
            }
        }
    };
    add_keys(&mut ys, &keys, 0.4 * fine);
    if let Some((y0, y1, _)) = p.bed {
        let bed_keys = [y0, y0 + BED_WALL_EPS, y1 - BED_WALL_EPS, y1];
        for &k in &bed_keys {
            ys.retain(|y| (y - k).abs() > 0.4 * fine || bed_keys.contains(y));
        }
        add_keys(&mut ys, &bed_keys, 0.0005);
    }
    ys.sort_by(|a, b| a.total_cmp(b));
    ys.dedup_by(|a, b| (*a - *b).abs() < 0.001);
    ys
}

/// Generate the mesh for a car with its [`style_for`] style (skin binding left empty; see
/// [`crate::skin::bind_skin`]).
pub fn generate(def: &VehicleDef, lattice: &LatticeDef) -> CarMesh {
    generate_styled(def, lattice, &style_for(def))
}

/// Generate the mesh for a car with an explicit style.
pub fn generate_styled(def: &VehicleDef, lattice: &LatticeDef, style: &CarStyle) -> CarMesh {
    let p = BodyProfile::styled(def, lattice, &style.body);
    let (base, fine) = loft_steps(style.tier);
    let ys = stations(&p, base, fine);
    let rings: Vec<Vec<RingPt>> = ys.iter().map(|&y| p.ring(y)).collect();
    let win = windows(&p);
    let mut b = Builder::new();

    loft(&p, &win, &ys, &rings, &mut b);
    end_caps(&p, &ys, &rings, &mut b);
    underbody(&p, def, &ys, &rings, &mut b);
    if style.underglow_rgb.is_some() {
        underglow(&p, &mut b);
    }
    lights_and_mirrors(&p, &mut b);
    interior(&p, &mut b);
    hood_scoop(&p, &mut b);
    let ctx = DetailCtx { def, profile: &p };
    for d in &style.details {
        emit_detail(&ctx, d, &mut b);
    }
    let car_spec = crate::mechanical::car_spec(def, &style.mechanical, style.tier);
    crate::mechanical::parts_with_spec(def, &p, &car_spec, &mut b);

    let atlas = LiveryAtlas {
        y: [p.y_tail, p.y_nose],
        z: [0.0, p.z_max() + 0.25],
        x_half: p.half_w + 0.12,
    };
    assign_uvs(&mut b, &atlas);
    // Visual stance pitch (nose down = +), about the chassis origin; wheel parts unaffected.
    let (ps, pc) = wp_math::sin_cos(style.body.stance_pitch_deg.clamp(-0.6, 0.8).to_radians());
    if ps.abs() > 1e-6 {
        b.transform(
            |k| k.wheel.is_none(),
            |q| [q[0], q[1] * pc + q[2] * ps, q[2] * pc - q[1] * ps],
        );
    }

    let mut parts = b.finish();
    parts.sort_by(|a, b| a.name.cmp(&b.name));
    let mut camber = [0.0f32; 4];
    for (i, w) in def.wheels.iter().enumerate() {
        let g = WheelGeom::new(i as u8, w, &style.wheel, &car_spec);
        camber[i] = if w.position_m[1] >= 0.0 {
            style.wheel.front_camber_deg
        } else {
            style.wheel.rear_camber_deg
        };
        parts.extend(crate::wheels::wheel_parts(&g, &style.wheel));
        parts.extend(crate::mechanical::brake_parts(
            &g,
            &style.mechanical,
            &style.wheel,
        ));
    }
    let mut tints: BTreeMap<MaterialSlot, [u8; 3]> = BTreeMap::new();
    if let Some(c) = style.mechanical.caliper_rgb {
        tints.insert(MaterialSlot::Caliper, c);
    }
    // Springs: their own colour, else the caliper tint (the pre-`spring`-slot look).
    if let Some(c) = style.mechanical.spring_rgb.or(style.mechanical.caliper_rgb) {
        if parts.iter().any(|q| q.material == MaterialSlot::Spring) {
            tints.insert(MaterialSlot::Spring, c);
        }
    }
    if let Some(c) = style.wheel.rim_rgb {
        tints.insert(MaterialSlot::Rim, c);
    }
    if let Some(c) = style.accent_rgb {
        tints.insert(MaterialSlot::Accent, c);
    }
    if let Some(c) = style.underglow_rgb {
        tints.insert(MaterialSlot::Underglow, c);
    }
    for &(slot, c) in &style.tints {
        tints.insert(slot, c);
    }
    let tri_count = parts.iter().map(|p| p.tri_count() as u32).sum();
    CarMesh {
        id: def.id.clone(),
        lattice_id: lattice.id.clone(),
        format: MESH_FORMAT,
        tri_count,
        tints,
        livery: style.livery.clone(),
        atlas: Some(atlas),
        wheel_camber_deg: camber.iter().any(|c| c.abs() > 1e-4).then_some(camber),
        parts,
    }
}

/// Loft quads, both sides. Faces between a closed and an open-bed ring are bed end walls.
fn loft(p: &BodyProfile, win: &Windows, ys: &[f32], rings: &[Vec<RingPt>], b: &mut Builder) {
    let k = RING_POINTS;
    for s in 0..ys.len() - 1 {
        let ym = 0.5 * (ys[s] + ys[s + 1]);
        let bed_end = p.bed_open(ys[s]) != p.bed_open(ys[s + 1]);
        for j in 0..k - 1 {
            let (a0, a1) = (rings[s][j].p, rings[s][j + 1].p);
            let (b0, b1) = (rings[s + 1][j].p, rings[s + 1][j + 1].p);
            let zm = 0.25 * (a0[2] + b0[2] + a1[2] + b1[2]);
            // The segment kind of the (finer) station that has the feature.
            // Adjacent rings with different layouts (greenhouse / flat top / bed): take the
            // segment kind of the ring whose layout matches the face's middle.
            let seg = {
                let (sa, sb) = (rings[s][j].seg, rings[s + 1][j].seg);
                if sa == sb {
                    sa
                } else {
                    let gh_a = rings[s][RING_SHOULDER].seg == Seg::Ledge;
                    if gh_a == p.has_greenhouse(ym) {
                        sa
                    } else {
                        sb
                    }
                }
            };
            let left = if bed_end && j >= 8 {
                // Bed front wall / tailgate inner face.
                if ym < 0.5 * (p.y_tail + p.y_deck) {
                    key_for(BodyRegion::Trunk, PartKind::Panel)
                } else {
                    PartKey::body(
                        "bed_front",
                        BodyRegion::Cabin,
                        PartKind::Body,
                        MaterialSlot::Livery,
                    )
                }
            } else {
                classify_left(p, win, ym, zm, seg)
            };
            for right in [false, true] {
                let key = if right {
                    right_key(&left)
                } else {
                    left.clone()
                };
                let part = b.part(&key);
                let mx = if right { -1.0 } else { 1.0 };
                let m = |q: [f32; 3]| [q[0] * mx, q[1], q[2]];
                let side = if right { 1 } else { 0 };
                let id = |st: usize, jj: usize| Some((st as i64 * 2 + side, jj as i64));
                let va = b.vert(part, id(s, j), m(a0), [0.0, 0.0]);
                let vb = b.vert(part, id(s + 1, j), m(b0), [0.0, 0.0]);
                let va1 = b.vert(part, id(s, j + 1), m(a1), [0.0, 0.0]);
                let vb1 = b.vert(part, id(s + 1, j + 1), m(b1), [0.0, 0.0]);
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
}

/// Nose and tail caps: horizontal strips between mirrored ring points.
fn end_caps(p: &BodyProfile, ys: &[f32], rings: &[Vec<RingPt>], b: &mut Builder) {
    for (s, front) in [(ys.len() - 1, true), (0usize, false)] {
        let ring = &rings[s];
        for j in 0..ring.len() - 1 {
            let l0 = ring[j].p;
            let l1 = ring[j + 1].p;
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
            let key = key_for(region, PartKind::Panel);
            if front {
                b.poly(&key, &[l0, l1, r1, r0]);
            } else {
                b.poly(&key, &[l0, r0, r1, l1]);
            }
        }
    }
}

/// Underbody: one strip across every station (kept below an open pickup bed's floor).
/// Between the wheel arches the ring's underside point is lifted onto the arch; the plate
/// stays at floor height there instead, narrowed to inside the wheels, so it never covers
/// the engine bay (visible with the hood open or gone).
fn underbody(
    p: &BodyProfile,
    def: &VehicleDef,
    ys: &[f32],
    rings: &[Vec<RingPt>],
    b: &mut Builder,
) {
    let key = PartKey::body(
        "underbody",
        BodyRegion::Cabin,
        PartKind::Body,
        MaterialSlot::Trim,
    );
    // Inner half width between the tires of the axle whose arch covers `y`.
    let inner = |y: f32| -> Option<f32> {
        def.wheels
            .iter()
            .zip(&p.arches)
            .filter(|(_, a)| (y - a.0).abs() < a.2)
            .map(|(w, _)| w.position_m[0].abs() - 0.5 * w.width_m - 0.03)
            .reduce(f32::min)
    };
    let under = |s: usize| {
        let mut q = rings[s][0].p;
        if let Some(xi) = inner(q[1]) {
            q[2] = p.sill(q[1]);
            q[0] = -(-q[0]).min(xi.max(0.2));
        }
        if let Some((y0, y1, floor)) = p.bed {
            if q[1] >= y0 && q[1] <= y1 {
                q[2] = q[2].min(floor - 0.03);
            }
        }
        q
    };
    for s in 0..ys.len() - 1 {
        let a = under(s);
        let c = under(s + 1);
        b.poly(&key, &[a, [-a[0], a[1], a[2]], [-c[0], c[1], c[2]], c]);
    }
}

/// Underglow: a thin emissive strip under each sill between the arches, facing down
/// (`underglow` slot; the renderer lights it with the headlights).
fn underglow(p: &BodyProfile, b: &mut Builder) {
    let (front, rear) = p
        .arches
        .iter()
        .fold((f32::MAX, f32::MIN), |(f, r), &(wy, _, ar)| {
            if wy >= 0.0 {
                (f.min(wy - ar), r)
            } else {
                (f, r.max(wy + ar))
            }
        });
    if front == f32::MAX || rear == f32::MIN || front - rear < 0.4 {
        return;
    }
    let (y0, y1) = (rear + 0.08, front - 0.08);
    for right in [false, true] {
        let sx = if right { 1.0 } else { -1.0 };
        let region = if right {
            BodyRegion::FrameRight
        } else {
            BodyRegion::FrameLeft
        };
        let side = if right { "r" } else { "l" };
        let key = PartKey::body(
            &format!("underglow_{side}"),
            region,
            PartKind::Detail,
            MaterialSlot::Underglow,
        );
        let ym = 0.5 * (y0 + y1);
        let x_out = p.half_width(ym) - 0.10;
        let z = p.sill(ym) - 0.004;
        let (xa, xb) = (sx * (x_out - 0.05), sx * x_out);
        b.cuboid(&key, [xa.min(xb), y0, z - 0.012], [xa.max(xb), y1, z]);
    }
}

/// Head / tail lights on the nose / tail faces (slightly proud, facing out), bound to the
/// fenders / quarters so a hanging bumper leaves them; door mirrors on the front doors.
fn lights_and_mirrors(p: &BodyProfile, b: &mut Builder) {
    let top_n = p.belt(p.y_nose);
    let top_t = p.belt(p.y_tail);
    let hl_z = (top_n - 0.07).max(p.z_bumper_top.0 + 0.03);
    let tl_z = (top_t - 0.08).max(p.z_bumper_top.1 + 0.02);
    for right in [false, true] {
        let sx = if right { 1.0 } else { -1.0 };
        let wf = p.half_width(p.y_nose);
        let (x0, x1) = (sx * (wf - 0.07), sx * (wf - 0.38));
        let (fl, rl) = if right {
            (BodyRegion::FrontRightFender, BodyRegion::RearRightQuarter)
        } else {
            (BodyRegion::FrontLeftFender, BodyRegion::RearLeftQuarter)
        };
        let side = if right { "r" } else { "l" };
        b.beveled_box(
            &PartKey::body(
                &format!("headlight_{side}"),
                fl,
                PartKind::Light,
                MaterialSlot::Headlight,
            ),
            [x0.min(x1), p.y_nose - 0.012, hl_z - 0.05],
            [x0.max(x1), p.y_nose + 0.014, hl_z + 0.045],
            0.008,
        );
        let wt = p.half_width(p.y_tail);
        let (t0, t1) = (sx * (wt - 0.05), sx * (wt - 0.36));
        b.beveled_box(
            &PartKey::body(
                &format!("taillight_{side}"),
                rl,
                PartKind::Light,
                MaterialSlot::Taillight,
            ),
            [t0.min(t1), p.y_tail - 0.014, tl_z - 0.05],
            [t0.max(t1), p.y_tail + 0.012, tl_z + 0.05],
            0.008,
        );
        // Mirror on the front door, just behind its hinge at the belt.
        let door = if right {
            BodyRegion::RightDoorF
        } else {
            BodyRegion::LeftDoorF
        };
        let w = p.half_width(p.y_fd_hinge) + p.haunch(p.y_fd_hinge);
        let (m0, m1) = (sx * (w + 0.02), sx * (w + 0.17));
        let zb = p.belt(p.y_fd_hinge - 0.15);
        b.beveled_box(
            &PartKey::body(
                &format!("mirror_{side}"),
                door,
                PartKind::Mirror,
                MaterialSlot::Paint,
            ),
            [m0.min(m1), p.y_fd_hinge - 0.20, zb + 0.03],
            [m0.max(m1), p.y_fd_hinge - 0.10, zb + 0.13],
            0.02,
        );
    }
}

/// Minimal interior: dash + seat block, dark, visible through the glass.
fn interior(p: &BodyProfile, b: &mut Builder) {
    let key = PartKey::body(
        "interior",
        BodyRegion::Cabin,
        PartKind::Interior,
        MaterialSlot::Interior,
    );
    let w = p.half_w - 0.18;
    let cab_rear = p.y_deck.max(p.y_quarter.0);
    let fw = p.y_fw.min(p.lattice_y_fw + 0.2);
    b.cuboid(
        &key,
        [-w, fw - 0.35, p.z_floor + 0.25],
        [w, fw - 0.05, p.z_belt + 0.02],
    );
    let seat_front = (fw - 0.9).max(cab_rear + 0.3);
    b.cuboid(
        &key,
        [-w, cab_rear + 0.15, p.z_floor + 0.1],
        [w, seat_front, p.z_belt - 0.05],
    );
}

/// Hood scoop / vents from [`BodyStyle::hood_scoop`].
fn hood_scoop(p: &BodyProfile, b: &mut Builder) {
    let s = &p.style;
    if s.hood_scoop == HoodScoop::None {
        return;
    }
    let span = (p.y_bf - p.y_fw).max(0.3);
    let h = s.hood_scoop_h.clamp(0.03, 0.14);
    let hw = 0.5 * s.hood_bulge_w.clamp(0.2, 0.7) * p.half_w;
    let (y0, y1) = (p.y_fw + 0.22 * span, p.y_fw + 0.62 * span);
    let paint = PartKey::body(
        "hood_scoop",
        BodyRegion::Hood,
        PartKind::Detail,
        MaterialSlot::Livery,
    );
    let dark = PartKey::body(
        "hood_scoop_inlet",
        BodyRegion::Hood,
        PartKind::Detail,
        MaterialSlot::Grille,
    );
    let z_at = |y: f32| p.top_point(y, 0.0).0[2];
    match s.hood_scoop {
        HoodScoop::None => {}
        HoodScoop::Bulge => b.surface_patch(
            &paint,
            p,
            &Patch {
                y0,
                y1,
                band: PatchBand::Top { x0: -hw, x1: hw },
                offset_m: 0.0,
                thickness_m: h,
                steps: (0, 0),
            },
        ),
        HoodScoop::Scoop | HoodScoop::Cowl => {
            let front = s.hood_scoop == HoodScoop::Scoop;
            let za = z_at(y0).min(z_at(y1)) - 0.01;
            let zt = z_at(y0).max(z_at(y1)) + h;
            b.beveled_box(&paint, [-hw, y0, za], [hw, y1, zt], 0.015);
            let (yo, dir) = if front {
                (y1 + 0.002, 1.0)
            } else {
                (y0 - 0.002, -1.0)
            };
            let q = [
                [-hw + 0.03, yo, zt - 0.015],
                [hw - 0.03, yo, zt - 0.015],
                [hw - 0.03, yo, za + 0.02 + 0.3 * h],
                [-hw + 0.03, yo, za + 0.02 + 0.3 * h],
            ];
            if dir > 0.0 {
                b.poly(&dark, &[q[0], q[3], q[2], q[1]]);
            } else {
                b.poly(&dark, &q);
            }
        }
        HoodScoop::Shaker => {
            let shaker = PartKey::body(
                "hood_shaker",
                BodyRegion::FrameFront,
                PartKind::Engine,
                MaterialSlot::Engine,
            )
            .rigid();
            let yc = 0.5 * (y0 + y1);
            let zc = z_at(yc);
            b.beveled_box(
                &shaker,
                [-0.7 * hw, yc - 0.18, zc - 0.05],
                [0.7 * hw, yc + 0.18, zc + h],
                0.02,
            );
            b.poly(
                &dark,
                &[
                    [-0.6 * hw, yc + 0.182, zc + h - 0.015],
                    [-0.6 * hw, yc + 0.182, zc + 0.01],
                    [0.6 * hw, yc + 0.182, zc + 0.01],
                    [0.6 * hw, yc + 0.182, zc + h - 0.015],
                ],
            );
        }
        HoodScoop::Vents => {
            let len = h.max(0.06) * 3.0;
            for x in [(-hw * 1.6, -hw * 0.7), (hw * 0.7, hw * 1.6)] {
                b.surface_patch(
                    &dark,
                    p,
                    &Patch {
                        y0: y0 + 0.05,
                        y1: (y0 + 0.05 + len).min(y1 + 0.1),
                        band: PatchBand::Top { x0: x.0, x1: x.1 },
                        offset_m: 0.003,
                        thickness_m: 0.004,
                        steps: (0, 0),
                    },
                );
            }
        }
    }
}

/// Emit one [`Detail`] (and its mirror image).
pub fn emit_detail(ctx: &DetailCtx, d: &Detail, b: &mut Builder) {
    let mut key = PartKey::body(&d.name, d.region, d.kind, d.material);
    key.rigid = d.rigid;
    match &d.shape {
        Shape::Box { lo, hi } => b.cuboid(&key, *lo, *hi),
        Shape::BeveledBox { lo, hi, bevel } => b.beveled_box(&key, *lo, *hi, *bevel),
        Shape::Cylinder {
            a,
            b: e,
            r_a,
            r_b,
            segments,
            caps,
        } => b.cylinder(&key, *a, *e, *r_a, *r_b, *segments, *caps),
        Shape::Torus {
            center,
            axis,
            major,
            minor,
            segments,
        } => b.torus(&key, *center, *axis, *major, *minor, segments.0, segments.1),
        Shape::Lathe {
            center,
            axis,
            profile,
            segments,
        } => b.lathe(&key, *center, *axis, profile, *segments),
        Shape::Extrude {
            profile,
            path,
            up,
            caps,
        } => b.extrude(&key, profile, path, *up, *caps),
        Shape::Patch(patch) => b.surface_patch(&key, ctx.profile, patch),
        Shape::Custom(f) => f(ctx, &key, b),
    }
    if d.mirror {
        let to = key.mirrored();
        b.mirror_part(&key.name, &to);
    }
}

/// Which livery atlas band a face falls in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum AtlasBand {
    Left,
    Right,
    Top,
    Front,
    Rear,
}

/// Livery atlas UV of a point in `band` (see `data/panels/README.md`).
fn atlas_uv(a: &LiveryAtlas, band: AtlasBand, q: [f32; 3]) -> [f32; 2] {
    const PAD: f32 = 0.004;
    let fy = ((q[1] - a.y[0]) / (a.y[1] - a.y[0])).clamp(0.0, 1.0); // 0 tail → 1 nose
    let fz = ((q[2] - a.z[0]) / (a.z[1] - a.z[0])).clamp(0.0, 1.0); // 0 ground → 1 top
    let fx = ((q[0] + a.x_half) / (2.0 * a.x_half)).clamp(0.0, 1.0); // 0 left → 1 right
    let rect = |u0: f32, u1: f32, v0: f32, v1: f32, s: f32, t: f32| {
        [
            u0 + PAD + (u1 - u0 - 2.0 * PAD) * s,
            v0 + PAD + (v1 - v0 - 2.0 * PAD) * t,
        ]
    };
    match band {
        // Left side: nose at u = 0 → tail at 0.25 (reads front → rear from outside).
        AtlasBand::Left => rect(0.0, 0.25, 0.0, 0.5, 1.0 - fy, fz),
        // Right side: tail at u = 0.25 → nose at 0.5 (reads rear → front from outside).
        AtlasBand::Right => rect(0.25, 0.5, 0.0, 0.5, fy, fz),
        // Top: plan view, tail at u = 0 → nose at 0.5; car left (−X) at the band's top.
        AtlasBand::Top => rect(0.0, 0.5, 0.5, 0.8, fy, 1.0 - fx),
        // Front, seen from ahead: car right (+X) at u = 0.
        AtlasBand::Front => rect(0.0, 0.25, 0.8, 1.0, 1.0 - fx, fz),
        // Rear, seen from behind: car left (−X) at u = 0.25.
        AtlasBand::Rear => rect(0.25, 0.5, 0.8, 1.0, fx, fz),
    }
}

/// UVs for every body part: paint / livery parts get livery-atlas UVs (vertices split where
/// faces of one part fall into different atlas bands), the others a side projection in
/// U 0.5–1. Wheel-space parts keep a constant UV.
fn assign_uvs(b: &mut Builder, a: &LiveryAtlas) {
    let len = a.y[1] - a.y[0];
    let height = a.z[1] - a.z[0];
    for key in b.keys() {
        if key.wheel.is_some() {
            b.set_uvs(&key.name, |_| [0.5, 0.5]);
            continue;
        }
        if !key.material.is_paint() {
            b.set_uvs(&key.name, |q| {
                [0.5 + 0.5 * (q[1] - a.y[0]) / len, (q[2] - a.z[0]) / height]
            });
            continue;
        }
        let Some((pos, _, idx)) = b.geometry(&key.name) else {
            continue;
        };
        let (pos, idx) = (pos.to_vec(), idx.to_vec());
        let mut out_pos: Vec<[f32; 3]> = Vec::new();
        let mut out_uv: Vec<[f32; 2]> = Vec::new();
        let mut out_idx: Vec<u32> = Vec::new();
        let mut map: BTreeMap<(u32, AtlasBand), u32> = BTreeMap::new();
        for t in idx.chunks(3) {
            let (p0, p1, p2) = (pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]);
            let e1 = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
            let e2 = [p2[0] - p0[0], p2[1] - p0[1], p2[2] - p0[2]];
            let n = [
                e1[1] * e2[2] - e1[2] * e2[1],
                e1[2] * e2[0] - e1[0] * e2[2],
                e1[0] * e2[1] - e1[1] * e2[0],
            ];
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-12);
            let n = [n[0] / l, n[1] / l, n[2] / l];
            let cx = (p0[0] + p1[0] + p2[0]) / 3.0;
            let band = if n[2] > 0.55 {
                AtlasBand::Top
            } else if n[1].abs() > n[0].abs() && n[1].abs() > 0.5 {
                if n[1] > 0.0 {
                    AtlasBand::Front
                } else {
                    AtlasBand::Rear
                }
            } else if n[0] < -0.05 || (n[0].abs() <= 0.05 && cx < 0.0) {
                AtlasBand::Left
            } else {
                AtlasBand::Right
            };
            for &v in t {
                let id = *map.entry((v, band)).or_insert_with(|| {
                    out_pos.push(pos[v as usize]);
                    out_uv.push(atlas_uv(a, band, pos[v as usize]));
                    (out_pos.len() - 1) as u32
                });
                out_idx.push(id);
            }
        }
        b.set_geometry(&key.name, out_pos, out_uv, out_idx);
    }
}
