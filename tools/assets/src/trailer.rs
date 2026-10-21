//! Semi-trailer mesh (`bed = "trailer"` in the style sheet): a cab-less dry-van box on a
//! sub-frame, built directly instead of lofted like a car body.
//!
//! The trailer lattice is a car-topology lattice stretched over the box (`gen-lattice --profile
//! suv`, its Cabin relabelled `FrameRear`), so its regions do not follow the box's own seams.
//! Every surface is therefore cut into a grid and each cell takes the region of the **nearest
//! lattice node** (left side mirrored to the right): the box skin dents, and falls off with a
//! detached group, exactly where the lattice deforms. Painted box skin parts carry their
//! region's panel name (`door_fl`, `frame_rear`, `hood`, …, see [`panel_name`]); bolt-ons that
//! span several regions are cut the same way and named `<stem>_<panel name>`
//! (`bottom_rail_door_fl`). Fixed assignments: the rear door leaves are the `Trunk` (as on
//! box trucks), the rear sill and underride bar the `RearBumper`, the rear door posts the
//! rear quarters, the rear header the `RearGlass` region, the landing gear and the upper
//! coupler `FrameFront`, the tandem sub-frame `FrameRear`. The lattice's `Windshield` nodes sit
//! inside the box, so that region gets the interior cargo load bars.
//!
//! Layout (chassis metres, +Y forward): the box spans the lattice length and width, from the
//! style's `bed_floor` to `bed_height`; front vertical corners are rounded in plan; the kingpin
//! hangs under an upper coupler plate at `[parts.trailer] kingpin`; the landing gear stands
//! behind it; the tandem rides on a slider sub-frame with air springs; mud flaps, a side
//! underride rail and a rear (ICC) underride bar complete the chassis. Lamps come from the
//! style's `[[lamp]]` list projected onto the flat box faces; rear-facing clearance markers are
//! red (taillight lens), all other markers amber.

use wp_contracts::{BodyRegion, LatticeDef, VehicleDef};

use crate::mesh::{
    add, apply_paint_overrides, mesh_palette, mirror, panel_name, scale, Builder, CarMesh, Key,
    MaterialSlot, PartKind, MESH_FORMAT,
};
use crate::parts::{lamp_kind_name, lamp_material, outline};
use crate::style::{CarStyle, Face, Lamp, LampKind, TrailerParts};

/// Fore-aft spacing of the box skin grid (about the lattice node pitch).
const STEP_Y: f32 = 0.3;
/// Target height of a box side grid band.
const STEP_Z: f32 = 0.36;
/// Segments of a rounded front corner.
const ARC_SEG: usize = 3;
/// Rear door frame (posts, header, sill) stands this far behind the box end.
const FRAME_PROUD: f32 = 0.03;
/// Width of a rear door post.
const POST_W: f32 = 0.1;
/// Height of the rear door header.
const HEADER_H: f32 = 0.2;
/// The bottom rail stands this far off the box side.
const RAIL_PROUD: f32 = 0.025;

/// Regions the box skin and its bolt-ons may take (nearest lattice node among these). The
/// Trunk (rear doors), RearGlass (header) and Windshield (load bars) are assigned explicitly.
const SHELL_REGIONS: [BodyRegion; 16] = [
    BodyRegion::FrontBumper,
    BodyRegion::RearBumper,
    BodyRegion::Hood,
    BodyRegion::Roof,
    BodyRegion::FrontLeftFender,
    BodyRegion::FrontRightFender,
    BodyRegion::LeftDoorF,
    BodyRegion::RightDoorF,
    BodyRegion::LeftDoorR,
    BodyRegion::RightDoorR,
    BodyRegion::RearLeftQuarter,
    BodyRegion::RearRightQuarter,
    BodyRegion::FrameLeft,
    BodyRegion::FrameRight,
    BodyRegion::FrameFront,
    BodyRegion::FrameRear,
];

/// Chassis (under-floor) regions.
const FRAME_REGIONS: [BodyRegion; 4] = [
    BodyRegion::FrameFront,
    BodyRegion::FrameRear,
    BodyRegion::FrameLeft,
    BodyRegion::FrameRight,
];

/// Measured trailer box (chassis metres).
#[derive(Debug, Clone)]
pub struct TrailerBox {
    /// Box half width.
    pub half_w: f32,
    /// Front bulkhead y.
    pub y_nose: f32,
    /// Rear end y (rear doors).
    pub y_tail: f32,
    /// Box floor (underside of the box; the kingpin plate is just below it).
    pub z_floor: f32,
    /// Box roof.
    pub z_roof: f32,
    /// Plan radius of the front vertical corners.
    pub corner_r: f32,
    /// Leading / trailing axle y.
    pub axles: (f32, f32),
    /// Wheel radius and lateral wheel centre |x|.
    pub wheel: (f32, f32),
    /// Dual tire width.
    pub wheel_w: f32,
}

impl TrailerBox {
    /// Measure the box from the lattice extent, the def's wheels and the style.
    pub fn measure(def: &VehicleDef, lattice: &LatticeDef, style: &CarStyle) -> TrailerBox {
        let fold = |k: usize, f: fn(f32, f32) -> f32, init: f32| {
            lattice.nodes.iter().map(|n| n.pos_m[k]).fold(init, f)
        };
        let lat_half_w = fold(0, f32::max, f32::MIN).max(-fold(0, f32::min, f32::MAX));
        let s = &style.body;
        let half_w = if s.bed_half_w > 0.0 {
            s.bed_half_w
        } else {
            lat_half_w
        };
        let z_roof = if s.bed_height > 0.0 {
            s.bed_height
        } else {
            fold(2, f32::max, f32::MIN)
        };
        let z_floor = if s.bed_floor > 0.0 {
            s.bed_floor
        } else {
            fold(2, f32::min, f32::MAX)
        };
        let ys: Vec<f32> = def.wheels.iter().map(|w| w.position_m[1]).collect();
        let lead = ys.iter().copied().fold(f32::MIN, f32::max);
        let trail = ys.iter().copied().fold(f32::MAX, f32::min);
        let w0 = &def.wheels[0];
        TrailerBox {
            half_w,
            y_nose: fold(1, f32::max, f32::MIN),
            y_tail: fold(1, f32::min, f32::MAX),
            z_floor,
            z_roof,
            corner_r: style.parts.trailer.corner_r.clamp(0.0, 0.5 * half_w),
            axles: (lead, trail),
            wheel: (w0.radius_m, w0.position_m[0].abs()),
            wheel_w: w0.width_m,
        }
    }

    /// Left plan outline from the rear corner forward, round the front corner, to the centre
    /// of the front face: points (x ≤ 0, y) with their outward plan normals.
    pub fn outline(&self) -> Vec<([f32; 2], [f32; 2])> {
        let r = self.corner_r;
        let y_side_end = self.y_nose - r;
        let mut out = Vec::new();
        let n = ((y_side_end - self.y_tail) / STEP_Y).ceil().max(1.0) as usize;
        for i in 0..=n {
            let y = self.y_tail + (y_side_end - self.y_tail) * i as f32 / n as f32;
            out.push(([-self.half_w, y], [-1.0, 0.0]));
        }
        if r > 0.0 {
            for i in 1..=ARC_SEG {
                let a = std::f32::consts::FRAC_PI_2 * i as f32 / ARC_SEG as f32;
                let (sn, cs) = wp_math::sin_cos(a);
                out.push(([-self.half_w + r - r * cs, y_side_end + r * sn], [-cs, sn]));
            }
        }
        let x0 = -self.half_w + r;
        let m = ((-x0) / STEP_Y).ceil().max(1.0) as usize;
        for i in 1..=m {
            out.push(([x0 * (1.0 - i as f32 / m as f32), self.y_nose], [0.0, 1.0]));
        }
        out
    }

    /// Box half width at `y` (narrower in the rounded front corners).
    pub fn half_width(&self, y: f32) -> f32 {
        let r = self.corner_r;
        let d = y - (self.y_nose - r);
        if r <= 0.0 || d <= 0.0 {
            return self.half_w;
        }
        let d = d.min(r);
        self.half_w - r + (r * r - d * d).max(0.0).sqrt()
    }

    /// Height bands of the box side (floor → roof).
    fn bands(&self) -> Vec<f32> {
        let h = self.z_roof - self.z_floor;
        let n = (h / STEP_Z).round().max(1.0) as usize;
        (0..=n)
            .map(|i| self.z_floor + h * i as f32 / n as f32)
            .collect()
    }

    /// Rear door frame: sill bottom and top, header bottom.
    fn rear_frame(&self, tp: &TrailerParts) -> (f32, f32, f32) {
        let sill_top = self.z_floor + 0.55 * tp.rear_sill;
        (sill_top - tp.rear_sill, sill_top, self.z_roof - HEADER_H)
    }
}

/// Nearest-node region lookup.
struct Zones<'a> {
    lattice: &'a LatticeDef,
}

impl Zones<'_> {
    /// Region of the node nearest to `p` among `allowed`, evaluated on the left side and
    /// mirrored to the right for points with x > 0 (keeps the two sides symmetric).
    fn at(&self, p: [f32; 3], allowed: &[BodyRegion]) -> BodyRegion {
        let q = [-p[0].abs(), p[1], p[2]];
        let mut best = (f32::MAX, BodyRegion::FrameRear);
        for n in &self.lattice.nodes {
            if !allowed.contains(&n.region) {
                continue;
            }
            let d = [n.pos_m[0] - q[0], n.pos_m[1] - q[1], n.pos_m[2] - q[2]];
            let d2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
            if d2 < best.0 {
                best = (d2, n.region);
            }
        }
        if p[0] > 1e-4 {
            mirror(best.1)
        } else {
            best.1
        }
    }
}

/// Livery UV like the car loft (U 0.5–1 along the length, V up the side).
fn box_uv(bx: &TrailerBox, q: [f32; 3]) -> [f32; 2] {
    [
        0.5 + 0.5 * (q[1] - bx.y_tail) / (bx.y_nose - bx.y_tail),
        (q[2] - bx.z_floor) / (bx.z_roof - bx.z_floor),
    ]
}

/// `<stem>_<panel name of region>`.
fn named(stem: &str, region: BodyRegion) -> String {
    format!("{stem}_{}", panel_name(region))
}

/// Side suffix.
fn sfx(right: bool) -> &'static str {
    if right {
        "r"
    } else {
        "l"
    }
}

/// Builds one trailer.
struct Ctx<'a> {
    b: Builder,
    bx: TrailerBox,
    z: Zones<'a>,
    style: &'a CarStyle,
    def: &'a VehicleDef,
}

impl Ctx<'_> {
    fn tp(&self) -> &TrailerParts {
        &self.style.parts.trailer
    }

    /// Painted box skin: sides, rounded front corners, front bulkhead and roof.
    fn shell(&mut self) {
        let bx = self.bx.clone();
        let ol = bx.outline();
        let bands = bx.bands();
        for right in [false, true] {
            let mx = if right { -1.0 } else { 1.0 };
            let side = right as i64;
            // Walls.
            for i in 0..ol.len() - 1 {
                for k in 0..bands.len() - 1 {
                    let (a, b) = (ol[i].0, ol[i + 1].0);
                    let zc = 0.5 * (bands[k] + bands[k + 1]);
                    let c = [0.5 * (a[0] + b[0]), 0.5 * (a[1] + b[1]), zc];
                    let region = self.z.at([mx * c[0], c[1], c[2]], &SHELL_REGIONS);
                    let key: Key = (
                        panel_name(region),
                        Some(region),
                        PartKind::Panel,
                        MaterialSlot::Paint,
                    );
                    let part = self.b.part(&key);
                    let mut v = |ii: usize, kk: usize| {
                        let p2 = ol[ii].0;
                        let q = [mx * p2[0], p2[1], bands[kk]];
                        let uv = box_uv(&bx, q);
                        self.b.vert(
                            part,
                            Some((1_000_000 + side * 100_000 + ii as i64, kk as i64)),
                            q,
                            uv,
                        )
                    };
                    let (a0, a1, b0, b1) = (v(i, k), v(i, k + 1), v(i + 1, k), v(i + 1, k + 1));
                    if right {
                        self.b.tri(part, a0, b0, a1);
                        self.b.tri(part, b0, b1, a1);
                    } else {
                        self.b.tri(part, a0, a1, b0);
                        self.b.tri(part, b0, a1, b1);
                    }
                }
            }
            // Roof: strips across at every side / corner station, 4 columns per side.
            let stations: Vec<f32> = ol
                .iter()
                .filter(|(_, n)| n[1] < 0.999)
                .map(|(p, _)| p[1])
                .chain(std::iter::once(bx.y_nose))
                .collect();
            let cols = 4;
            let zr = bx.z_roof;
            for s in 0..stations.len() - 1 {
                for c in 0..cols {
                    let xat = |ss: usize, cc: usize| {
                        -bx.half_width(stations[ss]) * (1.0 - cc as f32 / cols as f32)
                    };
                    let cx = 0.25 * (xat(s, c) + xat(s, c + 1) + xat(s + 1, c) + xat(s + 1, c + 1));
                    let cy = 0.5 * (stations[s] + stations[s + 1]);
                    let region = self.z.at([mx * cx, cy, zr], &SHELL_REGIONS);
                    let key: Key = (
                        panel_name(region),
                        Some(region),
                        PartKind::Panel,
                        MaterialSlot::Paint,
                    );
                    let part = self.b.part(&key);
                    let mut v = |ss: usize, cc: usize| {
                        let q = [mx * xat(ss, cc), stations[ss], zr];
                        let uv = box_uv(&bx, q);
                        self.b.vert(
                            part,
                            Some((2_000_000 + side * 100_000 + ss as i64, cc as i64)),
                            q,
                            uv,
                        )
                    };
                    let (a, a1, b, b1) = (v(s, c), v(s, c + 1), v(s + 1, c), v(s + 1, c + 1));
                    if right {
                        self.b.tri(part, a, b, a1);
                        self.b.tri(part, b, b1, a1);
                    } else {
                        self.b.tri(part, a, a1, b);
                        self.b.tri(part, b, a1, b1);
                    }
                }
            }
            // Underbody (the box floor seen from below).
            for s in 0..stations.len() - 1 {
                let (y0, y1) = (stations[s], stations[s + 1]);
                let (w0, w1) = (bx.half_width(y0), bx.half_width(y1));
                let zf = bx.z_floor;
                let region = self
                    .z
                    .at([mx * 0.5 * w0, 0.5 * (y0 + y1), zf], &FRAME_REGIONS);
                let key: Key = (
                    named("underbody", region),
                    Some(region),
                    PartKind::Body,
                    MaterialSlot::Trim,
                );
                let pts = [
                    [-mx * w0, y0, zf],
                    [-mx * w1, y1, zf],
                    [0.0, y1, zf],
                    [0.0, y0, zf],
                ];
                if right {
                    self.b.poly(&key, &[pts[0], pts[3], pts[2], pts[1]]);
                } else {
                    self.b.poly(&key, &pts);
                }
            }
        }
    }

    /// A strip swept along the left outline (and mirrored), split into parts by region:
    /// `section` is (outward offset, z) pairs.
    fn outline_strip(&mut self, stem: &str, material: MaterialSlot, section: &[(f32, f32)]) {
        let ol = self.bx.outline();
        let zc = 0.5 * (section[0].1 + section[section.len() - 1].1);
        for right in [false, true] {
            let mx = if right { -1.0 } else { 1.0 };
            let pts: Vec<[f32; 3]> = ol.iter().map(|(p, _)| [mx * p[0], p[1], 0.0]).collect();
            let sides: Vec<[f32; 3]> = ol.iter().map(|(_, n)| [mx * n[0], n[1], 0.0]).collect();
            let mut i = 0;
            while i + 1 < pts.len() {
                let reg = |j: usize| {
                    let c = scale(add(pts[j], pts[j + 1]), 0.5);
                    self.z.at([c[0], c[1], zc], &SHELL_REGIONS)
                };
                let region = reg(i);
                let mut j = i + 1;
                while j + 1 < pts.len() && reg(j) == region {
                    j += 1;
                }
                let key: Key = (
                    named(stem, region),
                    Some(region),
                    PartKind::Detail,
                    material,
                );
                let sec: Vec<(f32, f32)> = if right {
                    section.iter().rev().copied().collect()
                } else {
                    section.to_vec()
                };
                self.b
                    .sweep(&key, &pts[i..=j], &sides[i..=j], [0.0, 0.0, 1.0], &sec);
                i = j;
            }
        }
    }

    /// Top and bottom rails, side posts.
    fn rails_and_posts(&mut self) {
        let bx = self.bx.clone();
        let zf = bx.z_floor;
        let zr = bx.z_roof;
        let rail = self.tp().bottom_rail;
        self.outline_strip(
            "bottom_rail",
            MaterialSlot::Metal,
            &[
                (0.0, zf - 0.5 * rail),
                (RAIL_PROUD, zf - 0.5 * rail),
                (RAIL_PROUD, zf + 0.5 * rail),
                (0.0, zf + 0.5 * rail),
            ],
        );
        self.outline_strip(
            "top_rail",
            MaterialSlot::Metal,
            &[
                (0.0, zr - 0.08),
                (0.014, zr - 0.08),
                (0.014, zr + 0.012),
                (-0.03, zr + 0.012),
            ],
        );
        let pitch = self.tp().post_pitch;
        if pitch <= 0.0 {
            return;
        }
        // Sheet-and-post sides: vertical aluminium posts, each cut into four segments so it
        // dents with the skin.
        let (y0, y1) = (bx.y_tail + pitch, bx.y_nose - bx.corner_r - 0.3);
        let n = ((y1 - y0) / pitch).floor().max(0.0) as usize;
        let (z0, z1) = (zf + 0.5 * rail, zr - 0.08);
        for right in [false, true] {
            let sg = if right { 1.0 } else { -1.0 };
            for i in 0..=n {
                let y = y0 + pitch * i as f32;
                for seg in 0..4 {
                    let za = z0 + (z1 - z0) * seg as f32 / 4.0;
                    let zb = z0 + (z1 - z0) * (seg + 1) as f32 / 4.0;
                    let x = sg * bx.half_w;
                    let region = self.z.at([x, y, 0.5 * (za + zb)], &SHELL_REGIONS);
                    let key: Key = (
                        named("posts", region),
                        Some(region),
                        PartKind::Detail,
                        MaterialSlot::Metal,
                    );
                    self.b.cube(
                        &key,
                        [x.min(x + sg * 0.012), y - 0.03, za],
                        [x.max(x + sg * 0.012), y + 0.03, zb],
                    );
                }
            }
        }
    }

    /// Rear door frame, door leaves (Trunk), hinges, lock bars, plate.
    fn rear(&mut self) {
        let bx = self.bx.clone();
        let tp = self.tp().clone();
        let (sill_bot, sill_top, header_bot) = bx.rear_frame(&tp);
        let yt = bx.y_tail;
        let hw = bx.half_w;
        let fy = yt - FRAME_PROUD;
        // Frame.
        let sill: Key = (
            "rear_sill".into(),
            Some(BodyRegion::RearBumper),
            PartKind::Body,
            MaterialSlot::Metal,
        );
        self.b.cube(
            &sill,
            [-hw - 0.02, fy, sill_bot],
            [hw + 0.02, yt + 0.08, sill_top],
        );
        let header: Key = (
            "rear_header".into(),
            Some(BodyRegion::RearGlass),
            PartKind::Body,
            MaterialSlot::Metal,
        );
        self.b.cube(
            &header,
            [-hw - 0.02, fy, header_bot],
            [hw + 0.02, yt + 0.08, bx.z_roof + 0.015],
        );
        for right in [false, true] {
            let sg = if right { 1.0 } else { -1.0 };
            let q = if right {
                BodyRegion::RearRightQuarter
            } else {
                BodyRegion::RearLeftQuarter
            };
            let post: Key = (
                format!("rear_post_{}", sfx(right)),
                Some(q),
                PartKind::Body,
                MaterialSlot::Metal,
            );
            let (xa, xb) = (sg * (hw - POST_W), sg * (hw + 0.02));
            self.b.cube(
                &post,
                [xa.min(xb), fy, sill_top - 0.01],
                [xa.max(xb), yt + 0.08, header_bot + 0.01],
            );
            // Four hinges per leaf on the post.
            let hinge: Key = (
                format!("door_hinges_{}", sfx(right)),
                Some(q),
                PartKind::Detail,
                MaterialSlot::Metal,
            );
            for k in 0..4 {
                let z = sill_top + 0.22 + (header_bot - sill_top - 0.44) * k as f32 / 3.0;
                let (xa, xb) = (sg * (hw - POST_W - 0.12), sg * (hw - 0.02));
                self.b.cube(
                    &hinge,
                    [xa.min(xb), fy - 0.025, z - 0.04],
                    [xa.max(xb), fy, z + 0.04],
                );
            }
        }
        // Door leaves: a grid on the rear face (Trunk), flush with the box end.
        let door: Key = (
            panel_name(BodyRegion::Trunk),
            Some(BodyRegion::Trunk),
            PartKind::Panel,
            MaterialSlot::Paint,
        );
        let xin = hw - POST_W;
        let rows = 7;
        let cols = 6;
        let part = self.b.part(&door);
        for c in 0..cols {
            for r in 0..rows {
                let x = |cc: usize| -xin + 2.0 * xin * cc as f32 / cols as f32;
                let z = |rr: usize| sill_top + (header_bot - sill_top) * rr as f32 / rows as f32;
                let mut v = |cc: usize, rr: usize| {
                    let q = [x(cc), yt, z(rr)];
                    let uv = box_uv(&bx, q);
                    self.b.vert(part, Some((cc as i64, rr as i64)), q, uv)
                };
                let (a, a1, b, b1) = (v(c, r), v(c, r + 1), v(c + 1, r), v(c + 1, r + 1));
                // Facing −Y (outward at the rear).
                self.b.tri(part, a, b, a1);
                self.b.tri(part, b, b1, a1);
            }
        }
        let seam: Key = (
            "trunk_seam".into(),
            Some(BodyRegion::Trunk),
            PartKind::Detail,
            MaterialSlot::Trim,
        );
        self.b.cube(
            &seam,
            [-0.008, yt - 0.004, sill_top],
            [0.008, yt + 0.004, header_bot],
        );
        // Lock bars: vertical rods with cam keepers, guides and a handle each.
        let n = tp.lock_bars as usize;
        let bars: Key = (
            "lock_bars".into(),
            Some(BodyRegion::Trunk),
            PartKind::Detail,
            MaterialSlot::Metal,
        );
        let keepers: Key = (
            "lock_keepers".into(),
            Some(BodyRegion::Trunk),
            PartKind::Detail,
            MaterialSlot::Trim,
        );
        let yr = yt - 0.045;
        for right in [false, true] {
            let sg = if right { 1.0 } else { -1.0 };
            for k in 0..n {
                let frac = 0.14 + 0.46 * k as f32 / (n.max(2) - 1) as f32;
                let x = sg * (0.01 + frac * (xin - 0.01));
                self.b.cyl(
                    &bars,
                    [x, yr, sill_top - 0.05],
                    [x, yr, header_bot + 0.05],
                    0.017,
                    8,
                );
                for z in [sill_top - 0.05, header_bot + 0.05] {
                    self.b.cube(
                        &keepers,
                        [x - 0.035, fy - 0.07, z - 0.04],
                        [x + 0.035, fy, z + 0.04],
                    );
                }
                for t in [0.3f32, 0.7] {
                    let z = sill_top + t * (header_bot - sill_top);
                    self.b
                        .cube(&bars, [x - 0.03, yr, z - 0.025], [x + 0.03, yt, z + 0.025]);
                }
                // Handle: lever across the door, toward the hinge side.
                let zh = sill_top + 0.95;
                self.b.cube(
                    &bars,
                    [x.min(x + sg * 0.28), yr - 0.04, zh - 0.02],
                    [x.max(x + sg * 0.28), yr - 0.01, zh + 0.02],
                );
            }
        }
        if self.style.parts.plates {
            let plate: Key = (
                "plate_rear".into(),
                Some(BodyRegion::RearBumper),
                PartKind::Detail,
                MaterialSlot::Plate,
            );
            let zc = 0.5 * (sill_bot + sill_top);
            self.b.cube(
                &plate,
                [-0.26, fy - 0.006, zc - 0.055],
                [0.26, fy, zc + 0.055],
            );
        }
    }

    /// Kingpin, upper coupler, landing gear, cross-members, front air/electric couplings.
    fn front_chassis(&mut self) {
        let bx = self.bx.clone();
        let tp = self.tp().clone();
        let ff = Some(BodyRegion::FrameFront);
        let zf = bx.z_floor;
        let [kp_y, kp_z] = tp.kingpin;
        let accent = MaterialSlot::Accent;
        let plate: Key = (
            "upper_coupler".into(),
            ff,
            PartKind::Body,
            MaterialSlot::Metal,
        );
        self.b.cube(
            &plate,
            [-(bx.half_w - 0.2), kp_y - 1.1, kp_z],
            [bx.half_w - 0.2, bx.y_nose - 0.04, zf],
        );
        let pin: Key = ("kingpin".into(), ff, PartKind::Detail, MaterialSlot::Metal);
        self.b
            .cyl(&pin, [0.0, kp_y, kp_z - 0.1], [0.0, kp_y, kp_z], 0.045, 10);
        self.b.cyl(
            &pin,
            [0.0, kp_y, kp_z - 0.1],
            [0.0, kp_y, kp_z - 0.07],
            0.06,
            10,
        );
        // Gladhands and the lighting socket low on the front bulkhead.
        let couplings: Key = (
            "gladhands".into(),
            Some(BodyRegion::FrontBumper),
            PartKind::Detail,
            MaterialSlot::Metal,
        );
        for (x, w) in [(-0.45f32, 0.07f32), (-0.3, 0.07), (0.4, 0.1)] {
            self.b.cube(
                &couplings,
                [x - 0.5 * w, bx.y_nose, zf + 0.12],
                [x + 0.5 * w, bx.y_nose + 0.08, zf + 0.12 + w],
            );
        }
        // Landing gear.
        let ly = if tp.landing_gear_y != 0.0 {
            tp.landing_gear_y
        } else {
            kp_y - 1.6
        };
        let lx = tp.landing_gear_x;
        let lift = tp.landing_gear_lift;
        let z_knee = 0.5 * (zf + lift) + 0.1;
        for right in [false, true] {
            let sg = if right { 1.0 } else { -1.0 };
            let x = sg * lx;
            let leg: Key = (
                format!("landing_gear_{}", sfx(right)),
                ff,
                PartKind::Detail,
                accent,
            );
            // Mounting bracket, outer leg, inner leg.
            self.b.cube(
                &leg,
                [x - 0.1, ly - 0.14, zf - 0.32],
                [x + 0.1, ly + 0.14, zf],
            );
            self.b.cube(
                &leg,
                [x - 0.07, ly - 0.07, z_knee],
                [x + 0.07, ly + 0.07, zf - 0.3],
            );
            self.b.cube(
                &leg,
                [x - 0.05, ly - 0.05, lift + 0.05],
                [x + 0.05, ly + 0.05, z_knee + 0.02],
            );
            // Braces: forward to the floor and inboard to the centre line.
            self.b.beam(
                &leg,
                [x, ly + 0.05, z_knee + 0.1],
                [x, ly + 0.95, zf - 0.02],
                0.025,
                0.025,
                [1.0, 0.0, 0.0],
            );
            self.b.beam(
                &leg,
                [x - sg * 0.05, ly - 0.03, z_knee + 0.1],
                [sg * 0.12, ly - 0.03, zf - 0.02],
                0.02,
                0.02,
                [0.0, 1.0, 0.0],
            );
            let pad: Key = (
                format!("landing_gear_pad_{}", sfx(right)),
                ff,
                PartKind::Detail,
                MaterialSlot::Trim,
            );
            self.b.cube(
                &pad,
                [x - 0.13, ly - 0.16, lift],
                [x + 0.13, ly + 0.16, lift + 0.05],
            );
        }
        let shaft: Key = (
            "landing_gear_shaft".into(),
            ff,
            PartKind::Detail,
            MaterialSlot::Metal,
        );
        let zs = z_knee + 0.2;
        self.b.cyl(&shaft, [-lx, ly, zs], [lx, ly, zs], 0.022, 8);
        // Gearbox and crank handle on the left (road side) leg.
        let crank: Key = (
            "landing_gear_crank".into(),
            ff,
            PartKind::Detail,
            MaterialSlot::Metal,
        );
        self.b.cube(
            &crank,
            [-lx - 0.16, ly - 0.08, zs - 0.08],
            [-lx - 0.07, ly + 0.08, zs + 0.08],
        );
        self.b
            .cyl(&crank, [-lx - 0.16, ly, zs], [-lx - 0.32, ly, zs], 0.014, 6);
        self.b.cyl(
            &crank,
            [-lx - 0.32, ly, zs],
            [-lx - 0.32, ly, zs - 0.3],
            0.014,
            6,
        );
        // Cross-members under the floor, behind the coupler plate.
        let mut y = bx.y_tail + 0.35;
        while y < kp_y - 1.15 {
            let region = self.z.at([0.0, y, zf], &FRAME_REGIONS);
            let k: Key = (
                named("crossmembers", region),
                Some(region),
                PartKind::Body,
                accent,
            );
            self.b.cube(
                &k,
                [-(bx.half_w - 0.03), y - 0.035, zf - 0.11],
                [bx.half_w - 0.03, y + 0.035, zf],
            );
            y += 0.6;
        }
    }

    /// Slider sub-frame, axles, air springs, mud flaps, side and rear underride guards.
    fn running_gear(&mut self) {
        let bx = self.bx.clone();
        let tp = self.tp().clone();
        let zf = bx.z_floor;
        let (ya, yb) = bx.axles;
        let (r, wx) = bx.wheel;
        let wz = self.def.wheels[0].position_m[2];
        let fr = Some(BodyRegion::FrameRear);
        let accent = MaterialSlot::Accent;
        let rail_bot = zf - 0.34;
        for right in [false, true] {
            let sg = if right { 1.0 } else { -1.0 };
            let slider: Key = (format!("slider_{}", sfx(right)), fr, PartKind::Body, accent);
            let x = sg * 0.45;
            self.b.cube(
                &slider,
                [x - 0.06, (yb - 0.6).max(bx.y_tail + 0.15), rail_bot],
                [x + 0.06, ya + 0.95, zf - 0.11],
            );
            let hang: Key = (
                format!("suspension_{}", sfx(right)),
                fr,
                PartKind::Detail,
                accent,
            );
            let bags: Key = (
                format!("air_springs_{}", sfx(right)),
                fr,
                PartKind::Detail,
                MaterialSlot::Rubber,
            );
            for &ay in &[ya, yb] {
                // Hanger ahead of the axle, trailing arm back to it, air spring behind.
                self.b.cube(
                    &hang,
                    [x - 0.05, ay + 0.42, wz + 0.12],
                    [x + 0.05, ay + 0.58, rail_bot],
                );
                self.b.beam(
                    &hang,
                    [x, ay + 0.5, wz + 0.14],
                    [x, ay - 0.38, wz + 0.02],
                    0.05,
                    0.04,
                    [0.0, 0.0, 1.0],
                );
                self.b.cyl(
                    &bags,
                    [x, ay - 0.3, wz + 0.08],
                    [x, ay - 0.3, rail_bot],
                    0.12,
                    10,
                );
            }
        }
        let axles: Key = ("axles".into(), fr, PartKind::Detail, accent);
        for &ay in &[ya, yb] {
            self.b.cyl(
                &axles,
                [-(wx - 0.25), ay, wz],
                [wx - 0.25, ay, wz],
                0.065,
                10,
            );
        }
        // Mud flaps behind the trailing axle, on hangers from the floor.
        if self.style.parts.utility.mudflaps {
            for right in [false, true] {
                let sg = if right { 1.0 } else { -1.0 };
                let q = if right {
                    BodyRegion::RearRightQuarter
                } else {
                    BodyRegion::RearLeftQuarter
                };
                let k: Key = (
                    format!("mudflap_{}", sfx(right)),
                    Some(q),
                    PartKind::Detail,
                    MaterialSlot::Rubber,
                );
                let y = yb - r - 0.08;
                let x = sg * wx;
                let hw = 0.5 * bx.wheel_w + 0.05;
                self.b
                    .cube(&k, [x - hw, y - 0.008, 0.2], [x + hw, y + 0.008, zf - 0.2]);
                let hk: Key = (
                    format!("mudflap_hanger_{}", sfx(right)),
                    Some(q),
                    PartKind::Detail,
                    MaterialSlot::Metal,
                );
                self.b.cube(
                    &hk,
                    [x - hw - 0.02, y - 0.02, zf - 0.24],
                    [x + hw + 0.02, y + 0.02, zf - 0.19],
                );
                self.b.beam(
                    &hk,
                    [x, y, zf - 0.2],
                    [x, y, zf],
                    0.02,
                    0.02,
                    [0.0, 1.0, 0.0],
                );
            }
        }
        // Side underride guard: two horizontal rails on brackets, landing gear → tandem.
        if tp.side_guard {
            let ly = if tp.landing_gear_y != 0.0 {
                tp.landing_gear_y
            } else {
                tp.kingpin[0] - 1.6
            };
            let (y0, y1) = (ya + r + 0.4, ly - 0.45);
            for right in [false, true] {
                let sg = if right { 1.0 } else { -1.0 };
                let x = sg * (bx.half_w - 0.04);
                let n = ((y1 - y0) / 0.6).ceil().max(1.0) as usize;
                for i in 0..n {
                    let ys = y0 + (y1 - y0) * i as f32 / n as f32;
                    let ye = y0 + (y1 - y0) * (i + 1) as f32 / n as f32;
                    let region = self.z.at([x, 0.5 * (ys + ye), zf], &SHELL_REGIONS);
                    let k: Key = (
                        named("side_guard", region),
                        Some(region),
                        PartKind::Detail,
                        MaterialSlot::Metal,
                    );
                    for z in [0.5f32, 0.85] {
                        self.b
                            .cube(&k, [x - 0.025, ys, z - 0.045], [x + 0.025, ye, z + 0.045]);
                    }
                    if i % 2 == 0 {
                        self.b
                            .cube(&k, [x - 0.02, ys, 0.45], [x + 0.02, ys + 0.06, zf - 0.11]);
                    }
                }
                let region = self.z.at([x, y1, zf], &SHELL_REGIONS);
                let k: Key = (
                    named("side_guard", region),
                    Some(region),
                    PartKind::Detail,
                    MaterialSlot::Metal,
                );
                self.b
                    .cube(&k, [x - 0.02, y1 - 0.06, 0.45], [x + 0.02, y1, zf - 0.11]);
            }
        }
        // Rear underride (ICC) bar on two struts up to the rear sill.
        if tp.underride_z > 0.0 {
            let rb = Some(BodyRegion::RearBumper);
            let k: Key = ("underride_bar".into(), rb, PartKind::Detail, accent);
            let y = bx.y_tail + 0.08;
            let z = tp.underride_z;
            let hw = bx.half_w - 0.12;
            let (sill_bot, _, _) = bx.rear_frame(&tp);
            self.b
                .cube(&k, [-hw, y - 0.05, z - 0.06], [hw, y + 0.05, z + 0.06]);
            for sg in [-1.0f32, 1.0] {
                let x = sg * 0.3;
                self.b.cube(
                    &k,
                    [x - 0.04, y - 0.04, z + 0.06],
                    [x + 0.04, y + 0.04, sill_bot],
                );
            }
        }
    }

    /// Interior cargo load bars: the part of the lattice's Windshield region (its nodes sit
    /// inside the box, on the centre line).
    fn load_bars(&mut self) {
        let ws: Vec<[f32; 3]> = self
            .z
            .lattice
            .nodes
            .iter()
            .filter(|n| n.region == BodyRegion::Windshield)
            .map(|n| n.pos_m)
            .collect();
        if ws.is_empty() {
            return;
        }
        let y = ws.iter().map(|p| p[1]).sum::<f32>() / ws.len() as f32;
        let k: Key = (
            "cargo_load_bars".into(),
            Some(BodyRegion::Windshield),
            PartKind::Interior,
            MaterialSlot::Interior,
        );
        let w = self.bx.half_w - 0.06;
        for p in &ws {
            self.b.cyl(&k, [-w, y, p[2]], [w, y, p[2]], 0.025, 6);
        }
    }

    /// Where a lamp's face plane is at face coordinates (u, v): the point on the surface.
    fn face_point(&self, face: Face, u: f32, v: f32, right: bool) -> [f32; 3] {
        let bx = &self.bx;
        let tp = self.tp();
        match face {
            Face::Front => [u, bx.y_nose, v],
            Face::Rear => {
                let (_, sill_top, header_bot) = bx.rear_frame(tp);
                let on_frame = v < sill_top || v > header_bot || u.abs() > bx.half_w - POST_W;
                let y = if on_frame {
                    bx.y_tail - FRAME_PROUD
                } else {
                    bx.y_tail
                };
                [u, y, v]
            }
            Face::Side => {
                let mut x = bx.half_width(u);
                if (v - bx.z_floor).abs() < 0.5 * tp.bottom_rail {
                    x += RAIL_PROUD;
                }
                [if right { x } else { -x }, u, v]
            }
            Face::Top => [u, v, bx.z_roof + 0.012],
        }
    }

    /// Region a lamp at `c` is bolted to.
    fn lamp_region(&self, face: Face, c: [f32; 3]) -> BodyRegion {
        if face == Face::Rear {
            let (_, sill_top, header_bot) = self.bx.rear_frame(self.tp());
            let right = c[0] > 0.0;
            return if c[2] < sill_top {
                BodyRegion::RearBumper
            } else if c[2] > header_bot {
                BodyRegion::RearGlass
            } else if c[0].abs() > self.bx.half_w - POST_W {
                if right {
                    BodyRegion::RearRightQuarter
                } else {
                    BodyRegion::RearLeftQuarter
                }
            } else {
                BodyRegion::Trunk
            };
        }
        self.z.at(c, &SHELL_REGIONS)
    }

    /// Every `[[lamp]]` on the flat box faces: a lens standing `proud` off the face with a
    /// skirt back to it, on an optional dark bezel.
    fn lamps(&mut self) {
        let mut count: std::collections::BTreeMap<LampKind, usize> = Default::default();
        for l in self.style.lamps.clone() {
            let n = count.entry(l.kind).or_insert(0);
            let idx = *n;
            *n += 1;
            let kname = lamp_kind_name(l.kind);
            let stem = if idx == 0 {
                kname.to_string()
            } else {
                format!("{kname}_{}", idx + 1)
            };
            // Rear-facing clearance markers are red.
            let mat = if l.kind == LampKind::Marker && l.face == Face::Rear {
                MaterialSlot::Taillight
            } else {
                lamp_material(l.kind)
            };
            let sides: Vec<Option<bool>> = if l.mirror {
                vec![Some(false), Some(true)]
            } else {
                vec![None]
            };
            for side in sides {
                let right = side.unwrap_or(l.x >= 0.0);
                let name = match side {
                    Some(r) => format!("{stem}_{}", sfx(r)),
                    None => stem.clone(),
                };
                self.lamp(&l, &name, mat, side, right);
            }
        }
    }

    fn lamp(&mut self, l: &Lamp, name: &str, mat: MaterialSlot, side: Option<bool>, right: bool) {
        let sign = if right { 1.0 } else { -1.0 };
        let lx = if side.is_some() {
            sign * l.x.abs()
        } else {
            l.x
        };
        let (cu, cv) = match l.face {
            Face::Front | Face::Rear => (lx, l.z),
            Face::Side => (l.y, l.z),
            Face::Top => (lx, l.y),
        };
        let normal: [f32; 3] = match l.face {
            Face::Front => [0.0, 1.0, 0.0],
            Face::Rear => [0.0, -1.0, 0.0],
            Face::Side => [sign, 0.0, 0.0],
            Face::Top => [0.0, 0.0, 1.0],
        };
        let c = self.face_point(l.face, cu, cv, right);
        let region = self.lamp_region(l.face, c);
        // Face (u, v) → 3D on the face plane through `c`, offset `off` along the normal.
        let to3 = |u: f32, v: f32, off: f32| -> [f32; 3] {
            let q = match l.face {
                Face::Front | Face::Rear => [u, c[1], v],
                Face::Side => [c[0], u, v],
                Face::Top => [u, v, c[2]],
            };
            add(q, scale(normal, off))
        };
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
        let mut base = 0.0;
        if l.bezel > 0.0 {
            base = 0.004;
            let bz: Key = (
                format!("{name}_bezel"),
                Some(region),
                PartKind::Detail,
                MaterialSlot::Trim,
            );
            let grown: Vec<[f32; 3]> = ol
                .iter()
                .map(|&(u, v)| {
                    let (du, dv) = (u - cu, v - cv);
                    let d = (du * du + dv * dv).sqrt().max(1e-4);
                    to3(u + du / d * l.bezel, v + dv / d * l.bezel, base)
                })
                .collect();
            self.fan(&bz, to3(cu, cv, base), &grown, normal);
        }
        let key: Key = (name.to_string(), Some(region), PartKind::Light, mat);
        let top = base + l.proud.max(0.004);
        let lens: Vec<[f32; 3]> = ol.iter().map(|&(u, v)| to3(u, v, top)).collect();
        self.fan(&key, to3(cu, cv, top), &lens, normal);
        // Skirt back to the face.
        for i in 0..lens.len() {
            let j = (i + 1) % lens.len();
            let (a, b) = (lens[i], lens[j]);
            let back = scale(normal, -(top - base));
            self.b.poly(&key, &[a, add(a, back), add(b, back), b]);
        }
    }

    /// Triangle fan around `c`, oriented to face `normal`.
    fn fan(&mut self, key: &Key, c: [f32; 3], ring: &[[f32; 3]], normal: [f32; 3]) {
        for i in 0..ring.len() {
            let j = (i + 1) % ring.len();
            let (a, b) = (ring[i], ring[j]);
            let n = crate::mesh::cross(crate::mesh::sub(a, c), crate::mesh::sub(b, c));
            let dot = n[0] * normal[0] + n[1] * normal[1] + n[2] * normal[2];
            if dot >= 0.0 {
                self.b.poly(key, &[c, a, b]);
            } else {
                self.b.poly(key, &[c, b, a]);
            }
        }
    }
}

/// Generate the mesh of a trailer (skin binding left empty; see [`crate::skin::bind_skin`]).
pub fn generate(def: &VehicleDef, lattice: &LatticeDef, style: &CarStyle) -> CarMesh {
    let mut cx = Ctx {
        b: Builder::default(),
        bx: TrailerBox::measure(def, lattice, style),
        z: Zones { lattice },
        style,
        def,
    };
    cx.shell();
    cx.rails_and_posts();
    cx.rear();
    cx.front_chassis();
    cx.running_gear();
    cx.load_bars();
    cx.lamps();
    let mut parts = cx.b.finish();
    apply_paint_overrides(&mut parts, style);
    parts.extend(crate::wheels::wheel_parts(def, style));
    let tri_count = parts.iter().map(|p| p.tri_count() as u32).sum();
    CarMesh {
        id: def.id.clone(),
        lattice_id: lattice.id.clone(),
        format: MESH_FORMAT,
        tri_count,
        palette: mesh_palette(style),
        parts,
    }
}
