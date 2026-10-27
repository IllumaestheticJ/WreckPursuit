//! Procedural low-poly car mesh: a lofted body (cross-section rings along +Y from
//! [`BodyProfile::ring`], shaped by the car's [`BodyStyle`]), cut into one part per body
//! region along the lattice's zone boundaries, plus glass, lights, mirrors, a minimal
//! interior, style details, mechanical parts and wheels.
//! Units m, +Y forward, +Z up, origin = chassis origin (mid-wheelbase on the ground).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_contracts::{BodyRegion, LatticeDef, VehicleDef};

use crate::builder::{mirror_region, Builder, PartKey, Patch, PatchBand};
use crate::profile::{
    BodyProfile, RingPt, Seg, BED_WALL_EPS, RING_PILLAR_IN, RING_POINTS, RING_SHOULDER,
};
use crate::style::{style_for, CarStyle, Detail, DetailCtx, HoodScoop, LiveryHint, Shape, Tier};
use crate::wheels::WheelGeom;

/// Mesh file format version (`CarMesh::format`).
///
/// 4 (current) = the union of two independent extensions of format 1:
/// - from `claude/detailed-car-models` ("format 2" there): new part kinds / materials,
///   `MeshPart::rigid`, `CarMesh::tints`, `livery`, `atlas`, `wheel_camber_deg`, livery atlas
///   UVs on paint parts;
/// - from `lead/round12` ("format 3" there): `MeshPart::fold` ([`FoldBinding`], sheet
///   buckling of hoods / trunk lids / front fenders / rear quarters, written by `bind-skin`)
///   and head / tail lamps in the `FrameFront` / `FrameRear` regions, bound by `bind-skin` to
///   their end of the car (see [`crate::skin::light_regions`]).
///
/// Field names and serde shapes are exactly those of the two sides; only the number changed.
pub const MESH_FORMAT: u32 = 4;
/// How far a head / tail lamp lens stands proud of the nose / tail face, m.
pub const LIGHT_PROUD_M: f32 = 0.014;
/// How deep a head / tail lamp lens is sunk into the body behind the nose / tail face, m.
pub const LIGHT_DEPTH_M: f32 = 0.012;
/// Name prefix reserved for turn-signal lenses. Every car mesh (full and mid LOD) has exactly
/// `indicator_front_l/_r` and `indicator_rear_l/_r` (kind `light`, in `FrameFront` /
/// `FrameRear` like the head / tail lamp they are cut out of) and optionally
/// `indicator_side_l/_r` repeaters (front fender or door mirror); no other part name starts
/// with it. `_l` is the x < 0 side.
pub const INDICATOR_PREFIX: &str = "indicator_";
/// Gap between a turn-signal segment and the rest of its lamp lens (the lens divider), m.
pub const INDICATOR_SEAM_M: f32 = 0.005;

/// Key of the front turn-signal lens of the left (`right = false`) or right side: an amber
/// segment of the head-lamp cluster, in `FrameFront` so it binds and crushes like the lamp.
pub fn indicator_front_key(right: bool) -> PartKey {
    let name = if right {
        "indicator_front_r"
    } else {
        "indicator_front_l"
    };
    PartKey::body(
        name,
        BodyRegion::FrameFront,
        PartKind::Light,
        MaterialSlot::EmissiveAmber,
    )
}

/// Key of the rear turn-signal lens of a side: a section of the tail-lamp cluster in
/// `FrameRear`, `EmissiveAmber` (amber section) or `Taillight` (red-on-red, blinked red).
pub fn indicator_rear_key(right: bool, mat: MaterialSlot) -> PartKey {
    let name = if right {
        "indicator_rear_r"
    } else {
        "indicator_rear_l"
    };
    PartKey::body(name, BodyRegion::FrameRear, PartKind::Light, mat)
}

/// Key of a side repeater (`indicator_side_l/_r`), amber, on `left_region` (a front fender or
/// a front door for a mirror repeater; given for the left side, mirrored for the right).
pub fn indicator_side_key(right: bool, left_region: BodyRegion) -> PartKey {
    let k = PartKey::body(
        "indicator_side_l",
        left_region,
        PartKind::Light,
        MaterialSlot::EmissiveAmber,
    );
    if right {
        k.mirrored()
    } else {
        k
    }
}

/// Hard overall triangle range of any car (per-tier budgets: [`Tier::tri_budget`]).
pub const TRI_BUDGET: (u32, u32) = (4000, 22000);
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
    /// Sheet buckling of a hood / trunk lid, front fender or rear quarter (`bind-skin`); `None`
    /// for every other part. See [`FoldBinding`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fold: Option<FoldBinding>,
}

/// How a sheet panel crumples in the renderer (visual only; the lattice, hull and repair bill
/// never see it). The panel runs from a fixed end (a lid hinge row; a side panel door-side row) to a
/// leading end (the latch row; the fender nose / quarter tail). Per chord (fixed node → leading node):
/// - the leading end follows the crushed front structure: when the `leads` nodes (bumper,
///   frame, fender noses next to the leading node) move toward the fixed end further than the
///   panel's own leading node, the panel's vertices are pulled back by the difference
///   (weight 3), so the panel never floats over a crushed front;
/// - the effective shortening, `gain × max(own chord loss, lead push) −` [`FOLD_DEAD_ZONE_M`],
///   becomes a fold height (`FOLD_SPLIT ×` [`fold_height`], capped at `max_m`) along `dir`,
///   shaped by the crease weight (weight 1: two ridges with a valley, tented at the centre);
/// - once the panel is unlatched or hangs, the fixed end bows up by [`FOLD_REAR_LIFT`] of the
///   fold (weight 2) and the leading edge pops up by `pop_m` (weight 3 squared).
///
/// Values between chords are interpolated linearly along the `across` axis (the chords' rest
/// coordinate on it, from their leading node).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoldBinding {
    /// Lattice node pairs fixed end → leading end, sorted by their rest `across` coordinate.
    pub chords: Vec<[u16; 2]>,
    /// Per chord: three front-structure nodes (other regions) nearest its leading node.
    pub leads: Vec<[u16; 3]>,
    /// Axis the chords are spread along: 0 = x (lids), 2 = z (fenders).
    pub across: u8,
    /// Direction the sheet buckles toward, chassis space at rest (unit).
    pub dir: [f32; 3],
    /// Visual gain on the shortening (own chord loss or lead push, whichever is larger).
    pub gain: f32,
    /// Largest fold height, m.
    pub max_m: f32,
    /// Leading-edge pop of an unlatched / hanging lid, m (0 for bolted panels).
    pub pop_m: f32,
    /// 3 weights per vertex in 1/255: crease shape, fixed-end lift, leading-end follow.
    pub weights: Vec<u8>,
}

/// Lost chord length a panel absorbs in-plane before it buckles, m.
pub const FOLD_DEAD_ZONE_M: f32 = 0.015;
/// Largest fold height of a hood / trunk lid, m.
pub const FOLD_MAX_M: f32 = 0.28;
/// Largest outward buckle of a front fender or rear quarter, m.
pub const FENDER_FOLD_MAX_M: f32 = 0.12;
/// Share of the single-tent height each of the lid's ridges gets (the shortening is split
/// over two ridges).
pub const FOLD_SPLIT: f32 = 0.7;
/// Share of the fold height the fixed (hinge) edge bows up by once the panel is unlatched.
pub const FOLD_REAR_LIFT: f32 = 0.45;
/// Leading-edge pop of a lid whose latch let go, m.
pub const LID_POP_M: f32 = 0.09;
/// Visual crumple gain on a hood's own chord loss.
pub const HOOD_CRUMPLE_GAIN: f32 = 1.8;
/// Visual crumple gain on a trunk lid's own chord loss.
pub const TRUNK_CRUMPLE_GAIN: f32 = 1.4;
/// Visual crumple gain on a front fender or rear quarter.
pub const FENDER_CRUMPLE_GAIN: f32 = 1.5;

/// Fold height (m) of a sheet whose fixed→leading chord measured `rest_m` and is now
/// effectively `lost_m` shorter: an inextensible strip pushed together rises into a tent of
/// that height (uncapped; see [`FoldBinding`]). The browser (`render/panels.ts`) uses the same
/// formula.
pub fn fold_height(rest_m: f32, lost_m: f32) -> f32 {
    if rest_m <= 1e-3 {
        return 0.0;
    }
    let lost = (lost_m - FOLD_DEAD_ZONE_M).max(0.0);
    let c = (lost / rest_m).min(0.5);
    let k = 1.0 - c;
    0.5 * rest_m * (1.0 - k * k).sqrt()
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
    /// B-pillar y span (black when [`crate::style::BodyStyle::b_pillar`] is `Black`).
    b_pillar: Option<(f32, f32)>,
}

fn windows(p: &BodyProfile) -> Windows {
    let m = WINDOW_MARGIN;
    // B-pillar: half its width either side of the front door's rear edge; the front door
    // glass runs forward under the A-pillar to the sail (`BodyProfile::front_window`).
    let mb = 0.5 * p.style.b_pillar_w_m.clamp(0.05, 0.25);
    let front = p.front_window();
    let (rear, last) = match p.rear_door {
        Some((rh, re)) => (Some((re + m, rh - mb)), re),
        None => (None, p.y_fd_rear),
    };
    let q0 = p.roof.0 + 0.08;
    let q1 = last - 0.04;
    let van = p.style.silhouette == crate::style::Silhouette::Van;
    let quarter = (!van && p.style.quarter_glass && q1 - q0 > 0.2).then_some((q0, q1));
    let rear = if van { None } else { rear };
    // B-pillar: between the front window and the rear door window or quarter glass.
    let b_pillar = match (rear, quarter) {
        (Some(r), _) => Some((r.1, front.0)),
        (None, Some(q)) if p.rear_door.is_none() => Some((q.1, front.0)),
        _ => None,
    };
    Windows {
        front,
        rear,
        quarter,
        b_pillar,
    }
}

/// Start of the windshield glass above the cowl panel (y), when there is a cowl.
fn cowl_edge(p: &BodyProfile) -> Option<f32> {
    let len = p.style.cowl_len_m.clamp(0.0, 0.20);
    let y = p.y_fw - len;
    (len > 0.01 && y > p.roof.1 + 0.08).then_some(y)
}

/// Classify a lofted face of the LEFT side at mid `y`, mid height `z`, segment kind `seg`,
/// from ring point `j` to `j + 1`.
fn classify_left(p: &BodyProfile, win: &Windows, y: f32, z: f32, seg: Seg, j: usize) -> PartKey {
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
            let mut clad = p
                .style
                .cladding_z_m
                .clamp(0.0, 0.7)
                .min(p.z_bumper_top.0.min(p.z_bumper_top.1) - 0.06);
            // Bumpers keep their upper half painted.
            if let FrontBumper | RearBumper = r {
                let top = if r == FrontBumper {
                    p.z_bumper_top.0
                } else {
                    p.z_bumper_top.1
                };
                clad = clad.min(0.5 * (p.z_floor + top));
                // Inside a raked / undercut face the bumper's upper points collapse onto the
                // face line: keep its faces painted there.
                if p.face_band(y).is_some() {
                    clad = 0.0;
                }
            }
            if clad > 0.0 && z < clad {
                return PartKey::body(
                    &format!("{}_cladding", panel_name(r)),
                    r,
                    PartKind::Body,
                    MaterialSlot::Trim,
                );
            }
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
            if y > win.front.1 && y < p.y_fw {
                // Sail at the A-pillar foot, ahead of the front door glass (mirror patch).
                return match p.style.a_sail {
                    crate::style::TrimFinish::Black => {
                        PartKey::body("a_sail", Cabin, PartKind::Body, MaterialSlot::Trim)
                    }
                    _ => key_for(Cabin, PartKind::Body),
                };
            }
            if let Some(b) = win.b_pillar {
                if inside(b) && p.style.b_pillar == crate::style::TrimFinish::Black {
                    return PartKey::body("b_pillar", Cabin, PartKind::Body, MaterialSlot::Trim);
                }
            }
            key_for(Cabin, PartKind::Body)
        }
        Seg::Top => {
            let (ry0, ry1, _, _) = p.roof;
            // Ring points 10…13 of a greenhouse ring: A-pillar / roof rail / C-pillar band.
            let pillar = p.has_greenhouse(y) && j < RING_PILLAR_IN;
            if y >= p.y_fw {
                key_for(Hood, PartKind::Panel)
            } else if y > ry1 {
                if pillar {
                    key_for(Cabin, PartKind::Body)
                } else if cowl_edge(p).is_some_and(|c| y > c) {
                    PartKey::body("cowl", Cabin, PartKind::Body, MaterialSlot::Trim)
                } else {
                    key_for(Windshield, PartKind::Glass)
                }
            } else if y >= ry0 {
                key_for(Roof, PartKind::Panel)
            } else if y > p.y_deck {
                if p.rear_glass && !pillar {
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

/// True for a rear-facing loft face of a raked tail face on a tailgate car (no trunk lid, no
/// open bed), above the rear bumper: it is the tailgate, as the end cap's upper part is
/// (`tail_face_rake_deg` / `tail_chin_m` turn the end cap into loft faces). `q` = the face's
/// corners (ring point j, j + 1 at station s, ring point j and j + 1 at station s + 1), LEFT
/// side. The face's top corner is tested against the bumper top (a coarse loft step can be
/// longer than the face is deep, so one face spans the collapsed tail edge up to the tailgate
/// top: its middle sits low; the mid LOD kept no trunk part with the middle tested).
fn tailgate_face(p: &BodyProfile, y: f32, q: [[f32; 3]; 4]) -> bool {
    let depth = p.face_depth(false);
    // The face's tail-side edge inside the face zone (a coarse step can be longer than the face).
    let near = q.iter().map(|c| c[1]).fold(y, f32::min);
    if p.trunk_lid || p.bed.is_some() || depth <= 1e-4 || near > p.y_tail + depth + 0.01 {
        return false;
    }
    let top = q.iter().map(|c| c[2]).fold(f32::MIN, f32::max);
    if top <= p.z_bumper_top.1 + 0.04 {
        return false;
    }
    // Outward normal of the left-side face: (a1 − a0) × (b0 − a0).
    let (u, v) = (
        [q[1][0] - q[0][0], q[1][1] - q[0][1], q[1][2] - q[0][2]],
        [q[2][0] - q[0][0], q[2][1] - q[0][1], q[2][2] - q[0][2]],
    );
    let n = [
        u[1] * v[2] - u[2] * v[1],
        u[2] * v[0] - u[0] * v[2],
        u[0] * v[1] - u[1] * v[0],
    ];
    let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
    l > 1e-9 && n[1] / l < -0.6
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
    // Raked / undercut end faces (phase 3) are lofted too: fine steps over their depth.
    for front in [true, false] {
        let depth = p.face_depth(front);
        if depth > 1e-4 {
            zones.push(if front {
                (p.y_nose - depth - 0.05, p.y_nose + 1.0)
            } else {
                (p.y_tail - 1.0, p.y_tail + depth + 0.05)
            });
        }
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
    if let Some(c) = cowl_edge(p) {
        keys.push(c);
    }
    for r in [win.rear, win.quarter, win.b_pillar].into_iter().flatten() {
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
    generate_lod(def, lattice, style, Lod::Full)
}

/// Level of detail of a generated mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lod {
    /// The full mesh (`data/panels/meshes/<car>.json`): the style's [`Tier`] loft, every part;
    /// skinned and fold-bound by `bind-skin`. Drawn by the browser's per-panel `VehicleView`.
    Full,
    /// Mid LOD (`data/panels/meshes/mid/<car>.json`, [`generate_mid`]): the same style at the
    /// coarser [`MID_LOFT_STEPS`] loft and [`MID_WHEEL_SEGMENTS`] wheels, traffic-tier
    /// mechanical detail, and only the parts that show from outside at 30–150 m (see
    /// [`keep_in_mid`]). Same livery atlas, atlas UVs, tints and livery hint as the full mesh,
    /// so it paints identically. Not skinned: the browser's instanced fleet and parked cars
    /// draw it undeformed.
    Mid,
}

/// Loft resolution of the mid LOD: (base station step, fine step near ends / arches), m.
pub const MID_LOFT_STEPS: (f32, f32) = (0.2, 0.12);
/// Grid step of automatic-resolution surface patches (stripes, vinyl roofs, decal carriers) in
/// the mid LOD, m (full meshes: 6 cm).
pub const MID_PATCH_STEP_M: f32 = 0.16;
/// Segments around a mid-LOD wheel (tire and rim lathes).
pub const MID_WHEEL_SEGMENTS: u8 = 12;
/// Triangle range of every mid-LOD mesh.
pub const MID_TRI_BUDGET: (u32, u32) = (2000, 7000);
/// Detail parts smaller than this (largest bounding-box edge, m) are dropped from the mid LOD
/// (door handles, fuel fillers, plate screws, mirror glass, stripe kicks).
pub const MID_MIN_DETAIL_M: f32 = 0.2;

/// Whether a part of the full-detail generation is kept in the mid LOD: body panels, glass,
/// lights, mirrors, light bars, tires and rims, exhaust tips, and style details at least
/// [`MID_MIN_DETAIL_M`] across. Dropped: interior, engine bay, suspension and exhaust runs,
/// brake discs / calipers / uprights, decal carriers (unit numbers only show on a promoted
/// car), underglow strips and small details.
pub fn keep_in_mid(p: &MeshPart) -> bool {
    if p.material == MaterialSlot::Underglow {
        return false;
    }
    match p.kind {
        PartKind::Interior
        | PartKind::Engine
        | PartKind::BrakeDisc
        | PartKind::Caliper
        | PartKind::Upright
        | PartKind::DecalCarrier => false,
        PartKind::Suspension => p.name.starts_with("exhaust_tip"),
        PartKind::Detail => part_extent_m(p) >= MID_MIN_DETAIL_M,
        _ => true,
    }
}

/// Grid cell of the mid LOD's vertex clustering ([`cluster_part`]), m.
pub const MID_CLUSTER_M: f32 = 0.035;

/// Whether a mid-LOD part is decimated by vertex clustering ([`cluster_part`]): the style
/// details, rims, lamps, light bars and mirrors, whose fixed-resolution shapes do not get
/// coarser with the loft. Body panels, glass and tires are already coarse (loft / wheel
/// segments) and keep their exact shape and seams.
pub fn clustered_in_mid(p: &MeshPart) -> bool {
    matches!(
        p.kind,
        PartKind::Detail
            | PartKind::Rim
            | PartKind::Light
            | PartKind::Lightbar
            | PartKind::Mirror
            | PartKind::Suspension
    )
}

/// Decimate a part by vertex clustering: every vertex snaps to the centre of its `cell_m` grid
/// cell (split further by a coarse UV bucket, so livery-atlas seams stay apart), triangles
/// that collapse are dropped and duplicates removed. Snapping to cell centres (not cluster
/// averages) keeps parts that shared a vertex meeting at the same point. Deterministic.
pub fn cluster_part(p: &mut MeshPart, cell_m: f32) {
    let cell = (cell_m * 1000.0).max(1.0);
    let n = p.vertex_count();
    let mut index: BTreeMap<([i32; 3], [u16; 2]), u32> = BTreeMap::new();
    let mut pos: Vec<i32> = Vec::new();
    let mut uvs: Vec<u16> = Vec::new();
    let mut remap = vec![0u32; n];
    for (v, slot) in remap.iter_mut().enumerate() {
        let c = [0, 1, 2].map(|k| (p.positions_mm[v * 3 + k] as f32 / cell).floor() as i32);
        let uv = [p.uvs[v * 2], p.uvs[v * 2 + 1]];
        let key = (c, [uv[0] >> 12, uv[1] >> 12]);
        *slot = *index.entry(key).or_insert_with(|| {
            for ck in c {
                pos.push(((ck as f32 + 0.5) * cell).round() as i32);
            }
            uvs.extend_from_slice(&uv);
            (pos.len() / 3 - 1) as u32
        });
    }
    let mut seen: std::collections::BTreeSet<[u32; 3]> = std::collections::BTreeSet::new();
    let mut indices = Vec::with_capacity(p.indices.len());
    for t in p.indices.chunks_exact(3) {
        let tri = [
            remap[t[0] as usize],
            remap[t[1] as usize],
            remap[t[2] as usize],
        ];
        if tri[0] == tri[1] || tri[1] == tri[2] || tri[0] == tri[2] {
            continue;
        }
        // Same triangle with the same winding (rotations) only once.
        let r = (0..3).min_by_key(|&i| tri[i]).unwrap_or(0);
        let canon = [tri[r], tri[(r + 1) % 3], tri[(r + 2) % 3]];
        if seen.insert(canon) {
            indices.extend_from_slice(&tri);
        }
    }
    // Drop vertices no triangle uses any more.
    let mut used = vec![u32::MAX; pos.len() / 3];
    let mut out_pos = Vec::new();
    let mut out_uv = Vec::new();
    for i in indices.iter_mut() {
        let v = *i as usize;
        if used[v] == u32::MAX {
            used[v] = (out_pos.len() / 3) as u32;
            out_pos.extend_from_slice(&pos[v * 3..v * 3 + 3]);
            out_uv.extend_from_slice(&uvs[v * 2..v * 2 + 2]);
        }
        *i = used[v];
    }
    p.positions_mm = out_pos;
    p.uvs = out_uv;
    p.indices = indices;
    p.skin_nodes.clear();
    p.skin_weights.clear();
    p.fold = None;
}

/// Largest bounding-box edge of a part, m.
fn part_extent_m(p: &MeshPart) -> f32 {
    let mut lo = [i32::MAX; 3];
    let mut hi = [i32::MIN; 3];
    for v in p.positions_mm.chunks_exact(3) {
        for k in 0..3 {
            lo[k] = lo[k].min(v[k]);
            hi[k] = hi[k].max(v[k]);
        }
    }
    (0..3).map(|k| (hi[k] - lo[k]).max(0)).max().unwrap_or(0) as f32 * 1e-3
}

/// The mid-LOD mesh of a car with its [`style_for`] style ([`Lod::Mid`]).
pub fn generate_mid(def: &VehicleDef, lattice: &LatticeDef) -> CarMesh {
    generate_lod(def, lattice, &style_for(def), Lod::Mid)
}

/// Generate the mesh for a car with an explicit style at a level of detail.
pub fn generate_lod(def: &VehicleDef, lattice: &LatticeDef, style: &CarStyle, lod: Lod) -> CarMesh {
    let mid = lod == Lod::Mid;
    let mid_style;
    let style = if mid {
        let mut s = style.clone();
        s.wheel.segments = MID_WHEEL_SEGMENTS;
        mid_style = s;
        &mid_style
    } else {
        style
    };
    let p = BodyProfile::styled(def, lattice, &style.body);
    let (base, fine) = if mid {
        MID_LOFT_STEPS
    } else {
        loft_steps(style.tier)
    };
    let (ys, rings) = loft_rings(&p, base, fine);
    let win = windows(&p);
    let mut b = Builder::new();
    if mid {
        b.patch_step_m = Some(MID_PATCH_STEP_M);
    }

    loft(&p, &win, &ys, &rings, mid, &mut b);
    end_caps(&p, &ys, &rings, &mut b);
    underbody(&p, def, &ys, &rings, &mut b);
    if style.underglow_rgb.is_some() {
        underglow(&p, &mut b);
    }
    lights_and_mirrors(&p, &mut b);
    interior(&p, &mut b);
    hood_scoop(&p, &mut b);
    crate::trim::body_trim(&p, cowl_edge(&p), mid, &mut b);
    let ctx = DetailCtx { def, profile: &p };
    for d in &style.details {
        emit_detail(&ctx, d, &mut b);
    }
    // Kit parts on a raked / undercut end face sit on it (no-op without a face profile).
    crate::lamp::follow_end_faces(&mut b, &p);
    // Mid LOD: traffic-tier (coarse) mechanical and wheel detail.
    let mech_tier = if mid { Tier::Traffic } else { style.tier };
    let car_spec = crate::mechanical::car_spec(def, &style.mechanical, mech_tier);
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
    if mid {
        parts.retain(keep_in_mid);
        for q in parts.iter_mut().filter(|q| clustered_in_mid(q)) {
            cluster_part(q, MID_CLUSTER_M);
        }
        parts.retain(|q| !q.indices.is_empty());
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

/// Key of the black seal round a glass part (same region, so it swings / detaches with it).
fn seal_key(glass: &PartKey) -> PartKey {
    PartKey::body(
        &format!("{}_seal", glass.name),
        glass.region.unwrap_or(BodyRegion::Cabin),
        PartKind::Body,
        MaterialSlot::Trim,
    )
}

/// Outward normal (x–z plane) of the LEFT-side ring at point `j`.
fn ring_normal(ring: &[RingPt], j: usize) -> [f32; 3] {
    let n = ring.len();
    if j + 1 >= n {
        return [0.0, 0.0, 1.0];
    }
    let a = ring[j.saturating_sub(1)].p;
    let b = ring[j + 1].p;
    let (dx, dz) = (b[0] - a[0], b[2] - a[2]);
    let l = (dx * dx + dz * dz).sqrt();
    if l < 1e-6 {
        return [-1.0, 0.0, 0.0];
    }
    [-dz / l, 0.0, dx / l]
}

/// Loft quads, both sides. Faces between a closed and an open-bed ring are bed end walls.
/// Glass faces sit [`crate::style::BodyStyle::glass_recess_m`] behind the surface and
/// [`crate::style::BodyStyle::glass_frame_m`] inside their opening; a black seal part closes
/// the step round every glass panel (`<glass>_seal`, same region as the glass).
fn loft(
    p: &BodyProfile,
    win: &Windows,
    ys: &[f32],
    rings: &[Vec<RingPt>],
    mid: bool,
    b: &mut Builder,
) {
    let k = RING_POINTS;
    let ns = ys.len();
    // Pass 1: the LEFT-side key of every face.
    let mut keys: Vec<Vec<PartKey>> = Vec::with_capacity(ns.saturating_sub(1));
    for s in 0..ns - 1 {
        let ym = 0.5 * (ys[s] + ys[s + 1]);
        let bed_end = p.bed_open(ys[s]) != p.bed_open(ys[s + 1]);
        let mut row = Vec::with_capacity(k - 1);
        for j in 0..k - 1 {
            let (a0, a1) = (rings[s][j].p, rings[s][j + 1].p);
            let (b0, b1) = (rings[s + 1][j].p, rings[s + 1][j + 1].p);
            let zm = 0.25 * (a0[2] + b0[2] + a1[2] + b1[2]);
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
            } else if tailgate_face(p, ym, [a0, a1, b0, b1]) {
                key_for(BodyRegion::Trunk, PartKind::Panel)
            } else {
                classify_left(p, win, ym, zm, seg, j)
            };
            row.push(left);
        }
        keys.push(row);
    }
    // The mid LOD (30–150 m) keeps its glass flush: no inset, frame or seal.
    let (recess, frame) = if mid {
        (0.0, 0.0)
    } else {
        (
            p.style.glass_recess_m.clamp(0.0, 0.03),
            p.style.glass_frame_m.clamp(0.0, 0.04),
        )
    };
    // Same glass part on face (s, j)? The faces past the centre line (j ≥ k − 1) are the
    // mirror image of the last one: always the same part.
    let same = |s: isize, j: isize, key: &PartKey| -> bool {
        if j >= k as isize - 1 {
            return true;
        }
        s >= 0 && j >= 0 && (s as usize) < ns - 1 && keys[s as usize][j as usize] == *key
    };
    let toward = |a: [f32; 3], b: [f32; 3], d: f32| -> [f32; 3] {
        let v = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        if l < 1e-6 {
            return a;
        }
        let t = d.min(0.4 * l) / l;
        [a[0] + v[0] * t, a[1] + v[1] * t, a[2] + v[2] * t]
    };
    // Rows per face (`twist_rows`) and points per station line: line s of strip j
    // takes the larger count of its two faces, so both share its points (no T-junctions).
    let want: Vec<Vec<usize>> = (0..ns - 1)
        .map(|s| {
            (0..k - 1)
                .map(|j| {
                    twist_rows([
                        rings[s][j].p,
                        rings[s + 1][j].p,
                        rings[s][j + 1].p,
                        rings[s + 1][j + 1].p,
                    ])
                })
                .collect()
        })
        .collect();
    let line_rows = |s: usize, j: usize| -> usize {
        let a = if s > 0 { want[s - 1][j] } else { 1 };
        let b = if s < ns - 1 { want[s][j] } else { 1 };
        a.max(b)
    };
    for s in 0..ns - 1 {
        for j in 0..k - 1 {
            let left = keys[s][j].clone();
            let glass = left.kind == PartKind::Glass;
            let (si, ji) = (s as isize, j as isize);
            // Corner (sv, jv) of this face: surface point and (glass) inset point + pull flags.
            let corner = |sv: usize, jv: usize| -> ([f32; 3], [f32; 3], i64) {
                let q = rings[sv][jv].p;
                if !glass {
                    return (q, q, 0);
                }
                let mut c = q;
                let mut flags = 0i64;
                if sv == s && !same(si - 1, ji, &left) {
                    c = toward(c, rings[s + 1][jv].p, frame);
                    flags |= 1;
                }
                if sv == s + 1 && !same(si + 1, ji, &left) {
                    c = toward(c, rings[s][jv].p, frame);
                    flags |= 2;
                }
                if jv == j && !same(si, ji - 1, &left) {
                    c = toward(c, rings[sv][j + 1].p, frame);
                    flags |= 4;
                }
                if jv == j + 1 && jv < k - 1 && !same(si, ji + 1, &left) {
                    c = toward(c, rings[sv][j].p, frame);
                    flags |= 8;
                }
                let n = ring_normal(&rings[sv], jv);
                let c = [c[0] - n[0] * recess, c[1], c[2] - n[2] * recess];
                (q, c, flags)
            };
            let cs = [
                corner(s, j),
                corner(s + 1, j),
                corner(s, j + 1),
                corner(s + 1, j + 1),
            ];
            let vid = [(s, j), (s + 1, j), (s, j + 1), (s + 1, j + 1)];
            // Glass boundary edges (as corner index pairs) and the face across each.
            let edges: Vec<(usize, usize)> = if glass && !mid {
                let mut e = Vec::new();
                if !same(si - 1, ji, &left) {
                    e.push((0, 2));
                }
                if !same(si + 1, ji, &left) {
                    e.push((1, 3));
                }
                if !same(si, ji - 1, &left) {
                    e.push((0, 1));
                }
                if !same(si, ji + 1, &left) {
                    e.push((2, 3));
                }
                e
            } else {
                Vec::new()
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
                let v: Vec<u32> = (0..4)
                    .map(|i| {
                        let (st, jj) = vid[i];
                        let id = Some((st as i64 * 2 + side, jj as i64 + 100 * cs[i].2));
                        b.vert(part, id, m(cs[i].1), [0.0, 0.0])
                    })
                    .collect();
                // A twisted face is cut into rows across its strip (bilinear points on the
                // station lines, shared with the next face by id): one long twisted quad shades
                // as two facets of different brightness, which repeat along the strip as a
                // sawtooth (C-pillar sail, A-pillar sail, plan-rounded tail corners). Where the
                // two station lines have different counts the face is zipped between them.
                let (nl, nr) = (line_rows(s, j), line_rows(s + 1, j));
                let lerp = |a: [f32; 3], b: [f32; 3], t: f32| {
                    [
                        a[0] + (b[0] - a[0]) * t,
                        a[1] + (b[1] - a[1]) * t,
                        a[2] + (b[2] - a[2]) * t,
                    ]
                };
                // Points of a station line (`at` = 0: line s, 1: line s + 1), bottom to top.
                let line = |b: &mut Builder, at: usize, n: usize| -> Vec<([f32; 3], u32)> {
                    let (q0, q1) = (cs[at].1, cs[at + 2].1);
                    (0..=n)
                        .map(|r| {
                            if r == 0 {
                                (q0, v[at])
                            } else if r == n {
                                (q1, v[at + 2])
                            } else {
                                let q = lerp(q0, q1, r as f32 / n as f32);
                                // Keyed by the end corners' glass pull flags too: faces
                                // whose corners are pulled differently do not share points.
                                let pull = cs[at].2 * 16 + cs[at + 2].2;
                                let id = Some((
                                    (s + at) as i64 * 2 + side,
                                    100_000 + pull * 1_000 + j as i64 * 16 + r as i64,
                                ));
                                (q, b.vert(part, id, m(q), [0.0, 0.0]))
                            }
                        })
                        .collect()
                };
                let lp = line(b, 0, nl);
                let rp = line(b, 1, nr);
                // Left side: outward normal is −X/up; CCW seen from outside.
                let tri = |b: &mut Builder, x: u32, y: u32, z: u32| {
                    if right {
                        b.tri(part, x, z, y);
                    } else {
                        b.tri(part, x, y, z);
                    }
                };
                if nl == nr {
                    // Row quads, each split along the diagonal with the flatter fold.
                    for r in 0..nl {
                        let q = [lp[r].0, rp[r].0, lp[r + 1].0, rp[r + 1].0];
                        let v = [lp[r].1, rp[r].1, lp[r + 1].1, rp[r + 1].1];
                        if flatter_split(q) {
                            tri(b, v[0], v[3], v[1]);
                            tri(b, v[0], v[2], v[3]);
                        } else {
                            tri(b, v[0], v[2], v[1]);
                            tri(b, v[1], v[2], v[3]);
                        }
                    }
                } else {
                    // Zip: advance along whichever line's next point is lower.
                    let (mut i, mut r) = (0, 0);
                    while i < nl || r < nr {
                        let ti = (i + 1) as f32 / nl as f32;
                        let tr = (r + 1) as f32 / nr as f32;
                        if i < nl && (r == nr || ti <= tr) {
                            tri(b, lp[i].1, lp[i + 1].1, rp[r].1);
                            i += 1;
                        } else {
                            tri(b, rp[r].1, lp[i].1, rp[r + 1].1);
                            r += 1;
                        }
                    }
                }
                if !edges.is_empty() {
                    let seal = seal_key(&key);
                    for &(e0, e1) in &edges {
                        let (o0, i0) = (cs[e0].0, cs[e0].1);
                        let (o1, i1) = (cs[e1].0, cs[e1].1);
                        let (st, jj) = vid[e0];
                        let n = ring_normal(&rings[st], jj);
                        let out = [n[0] * mx, 0.0, n[2]];
                        b.poly_facing(&seal, &[m(o0), m(o1), m(i1), m(i0)], out);
                    }
                }
            }
        }
    }
}

/// The loft the mesh is built from: [`stations`] at steps (`base`, `fine`) and their rings,
/// with the rounded end rings.
pub fn loft_rings(p: &BodyProfile, base: f32, fine: f32) -> (Vec<f32>, Vec<Vec<RingPt>>) {
    let ys = stations(p, base, fine);
    let mut rings: Vec<Vec<RingPt>> = ys.iter().map(|&y| p.ring(y)).collect();
    round_ends(p, &mut rings);
    (ys, rings)
}

/// Whether to split a loft quad along its 0–3 diagonal instead of the original 1–2 one: corners
/// `q` = (station s, point j), (s + 1, j), (s, j + 1), (s + 1, j + 1). Picked only when its two
/// triangles fold clearly less (near-planar quads keep the original split).
fn flatter_split(q: [[f32; 3]; 4]) -> bool {
    let (fa, fb) = quad_folds(q);
    fb > fa + 0.02
}

/// Twist (degrees of fold between a quad's two triangles) per row of [`twist_rows`].
const TWIST_ROW_DEG: f32 = 3.0;

/// Rows to cut a loft quad into across its strip (1 = none, at most 3): only long thin quads
/// (across the strip > 2 × along it) that fold more than [`TWIST_ROW_DEG`] whichever way they
/// are split. Corners as in [`flatter_split`].
fn twist_rows(q: [[f32; 3]; 4]) -> usize {
    let len = |a: [f32; 3], b: [f32; 3]| {
        ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt()
    };
    let across = 0.5 * (len(q[0], q[2]) + len(q[1], q[3]));
    let along = 0.5 * (len(q[0], q[1]) + len(q[2], q[3]));
    if across <= 2.0 * along {
        return 1;
    }
    let (fa, fb) = quad_folds(q);
    let deg = fa.max(fb).clamp(-1.0, 1.0).acos().to_degrees();
    ((deg / TWIST_ROW_DEG).ceil() as usize).clamp(1, 3)
}

/// Folds (normal dot products) of the original and the alternative split of a quad.
fn quad_folds(q: [[f32; 3]; 4]) -> (f32, f32) {
    let n = |a: [f32; 3], b: [f32; 3], c: [f32; 3]| -> [f32; 3] {
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let x = [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ];
        let l = (x[0] * x[0] + x[1] * x[1] + x[2] * x[2]).sqrt();
        if l < 1e-12 {
            [0.0; 3]
        } else {
            [x[0] / l, x[1] / l, x[2] / l]
        }
    };
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    // Same winding for both splits (left side): (0, 2, 1) + (1, 2, 3) vs (0, 3, 1) + (0, 2, 3).
    // A degenerate triangle (zero normal) counts as flat: nothing to fold.
    let fold = |a: [f32; 3], b: [f32; 3]| {
        if a == [0.0; 3] || b == [0.0; 3] {
            1.0
        } else {
            dot(a, b)
        }
    };
    let fold_a = fold(n(q[0], q[2], q[1]), n(q[1], q[2], q[3]));
    let fold_b = fold(n(q[0], q[3], q[1]), n(q[0], q[2], q[3]));
    (fold_a, fold_b)
}

/// Set the first and last rings in by [`crate::style::BodyStyle::end_round_m`] (along each
/// point's inward normal in the ring plane, at most a third of the distance to the centre line
/// / underside), so the nose and tail faces get a rounded edge over the last loft step.
fn round_ends(p: &BodyProfile, rings: &mut [Vec<RingPt>]) {
    let r = p.style.end_round_m.clamp(0.0, 0.05);
    if r <= 1e-4 || rings.len() < 3 {
        return;
    }
    let last = rings.len() - 1;
    for s in [0, last] {
        let ring = rings[s].clone();
        let z_lo = ring[0].p[2];
        let z_hi = ring[ring.len() - 1].p[2];
        for (j, pt) in rings[s].iter_mut().enumerate() {
            let n = ring_normal(&ring, j);
            let q = pt.p;
            let dx = (r * n[0]).abs().min(-q[0] / 3.0).max(0.0);
            let dz = (r * n[2]).abs().min((z_hi - z_lo) / 6.0);
            pt.p = [q[0] + dx, q[1], q[2] - dz * n[2].signum()];
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

/// Head / tail lights on the nose / tail faces ([`LIGHT_PROUD_M`] proud, facing out, sunk
/// [`LIGHT_DEPTH_M`]); their region is the front / rear frame (never a free panel), and
/// `bind-skin` binds them to the nearest bumper, hood / trunk, fender / quarter and frame nodes
/// so they crush with the end of the car. Each cluster carries its turn signal as a segment of
/// the same lens (same depth, proud face and bevel, split by [`INDICATOR_SEAM_M`]): an amber
/// strip along the bottom of the head lamp, an amber inboard section of the tail lamp. Style
/// kits reshape both (this default is the police cars' layout). Door mirrors: [`crate::trim`].
fn lights_and_mirrors(p: &BodyProfile, b: &mut Builder) {
    let top_n = p.belt(p.y_nose);
    let top_t = p.belt(p.y_tail);
    let hl_z = (top_n - 0.07).max(p.z_bumper_top.0 + 0.03);
    let tl_z = (top_t - 0.08).max(p.z_bumper_top.1 + 0.02);
    for right in [false, true] {
        let sx = if right { 1.0 } else { -1.0 };
        let wf = p.half_width(p.y_nose);
        let (x0, x1) = (sx * (wf - 0.07), sx * (wf - 0.38));
        let side = if right { "r" } else { "l" };
        // Head lamp over an amber strip along the lens bottom (4 cm tall).
        let z_split = hl_z - 0.01;
        b.beveled_box(
            &PartKey::body(
                &format!("headlight_{side}"),
                BodyRegion::FrameFront,
                PartKind::Light,
                MaterialSlot::Headlight,
            ),
            [
                x0.min(x1),
                p.y_nose - LIGHT_DEPTH_M,
                z_split + INDICATOR_SEAM_M,
            ],
            [x0.max(x1), p.y_nose + LIGHT_PROUD_M, hl_z + 0.045],
            0.008,
        );
        b.beveled_box(
            &indicator_front_key(right),
            [x0.min(x1), p.y_nose - LIGHT_DEPTH_M, hl_z - 0.05],
            [x0.max(x1), p.y_nose + LIGHT_PROUD_M, z_split],
            0.008,
        );
        let wt = p.half_width(p.y_tail);
        // Tail lamp with an amber inboard section (9 cm wide).
        let (t0, t1) = (sx * (wt - 0.05), sx * (wt - 0.36));
        let ts = sx * (wt - 0.27);
        let tsr = ts + sx * INDICATOR_SEAM_M;
        b.beveled_box(
            &PartKey::body(
                &format!("taillight_{side}"),
                BodyRegion::FrameRear,
                PartKind::Light,
                MaterialSlot::Taillight,
            ),
            [t0.min(tsr), p.y_tail - LIGHT_PROUD_M, tl_z - 0.05],
            [t0.max(tsr), p.y_tail + LIGHT_DEPTH_M, tl_z + 0.05],
            0.008,
        );
        b.beveled_box(
            &indicator_rear_key(right, MaterialSlot::EmissiveAmber),
            [t1.min(ts), p.y_tail - LIGHT_PROUD_M, tl_z - 0.05],
            [t1.max(ts), p.y_tail + LIGHT_DEPTH_M, tl_z + 0.05],
            0.008,
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat 1 m × 1 m sheet as an `n` × `n` grid of quads (mm, z = 0).
    fn sheet(n: usize) -> MeshPart {
        let mut positions_mm = Vec::new();
        let mut uvs = Vec::new();
        for j in 0..=n {
            for i in 0..=n {
                positions_mm.extend_from_slice(&[(i * 1000 / n) as i32, (j * 1000 / n) as i32, 0]);
                // Within one UV bucket (a livery seam would split clusters).
                uvs.extend_from_slice(&[(i * 30) as u16, (j * 30) as u16]);
            }
        }
        let mut indices = Vec::new();
        let w = (n + 1) as u32;
        for j in 0..n as u32 {
            for i in 0..n as u32 {
                let a = j * w + i;
                indices.extend_from_slice(&[a, a + 1, a + w + 1, a, a + w + 1, a + w]);
            }
        }
        MeshPart {
            name: "sheet".into(),
            region: None,
            kind: PartKind::Detail,
            material: MaterialSlot::Trim,
            wheel: None,
            rigid: false,
            positions_mm,
            uvs,
            indices,
            skin_nodes: Vec::new(),
            skin_weights: Vec::new(),
            fold: None,
        }
    }

    #[test]
    fn clustering_decimates_a_dense_sheet_and_keeps_its_extent() {
        let mut p = sheet(100); // 1 cm quads
        let before = p.tri_count();
        cluster_part(&mut p, 0.1);
        assert!(
            p.tri_count() * 20 < before,
            "{} → {}",
            before,
            p.tri_count()
        );
        assert!(p.tri_count() >= 2);
        assert_eq!(p.uvs.len(), p.vertex_count() * 2);
        assert!(p.indices.iter().all(|&i| (i as usize) < p.vertex_count()));
        let xs: Vec<i32> = p.positions_mm.chunks_exact(3).map(|v| v[0]).collect();
        let (lo, hi) = (xs.iter().min().unwrap(), xs.iter().max().unwrap());
        assert!(*lo <= 100 && *hi >= 900, "extent {lo}..{hi}");
    }

    #[test]
    fn clustering_leaves_a_coarse_sheet_alone() {
        let mut p = sheet(4); // 25 cm quads, coarser than the cell
        let before = p.clone();
        cluster_part(&mut p, 0.035);
        assert_eq!(p.tri_count(), before.tri_count());
    }

    #[test]
    fn mid_lod_drops_hidden_parts_and_is_much_smaller() {
        let root = crate::repo_root();
        let car = crate::load_car(&root, "kestrel_ts").unwrap();
        let full = generate(&car.def, &car.lattice);
        let mid = generate_mid(&car.def, &car.lattice);
        assert!(
            mid.tri_count * 3 < full.tri_count,
            "{} vs {}",
            mid.tri_count,
            full.tri_count
        );
        assert!(mid.parts.iter().all(keep_in_mid));
        assert!(!mid
            .parts
            .iter()
            .any(|p| p.kind == PartKind::Engine || p.kind == PartKind::Interior));
        assert_eq!(mid.atlas, full.atlas);
    }
}
