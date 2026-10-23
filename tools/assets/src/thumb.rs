//! Car thumbnails and turntable strips for the menus (cars audit F26).
//!
//! A small software rasteriser draws a car's panel mesh (`data/panels/meshes/<car>.json`) in its
//! stock colours: flat-shaded triangles, a z-buffer, 2 × 2 supersampling and a soft ground
//! shadow, on a transparent background. No GPU, no browser: `gen-thumbs` runs anywhere the
//! other asset tools run and the output is a plain PNG per car.
//!
//! - `data/cars/thumbs/<car>.png` — [`THUMB_W`] × [`THUMB_H`], three-quarter front view.
//! - `data/cars/thumbs/<car>.turn.png` — [`TURN_FRAMES`] frames of [`TURN_W`] × [`TURN_H`] side
//!   by side, the car turning [`360 / TURN_FRAMES`](TURN_FRAMES)° per frame (the UI steps through
//!   them for a turntable preview).
//!
//! The renders depend only on the mesh file and the car's wheel positions, so the
//! `car-assets` test `every_car_has_up_to_date_thumbnails` flags a thumbnail whose mesh moved.

use std::path::{Path, PathBuf};

use wp_contracts::VehicleDef;

use crate::mesh::{CarMesh, MaterialSlot};

/// Thumbnail width, px.
pub const THUMB_W: u32 = 320;
/// Thumbnail height, px.
pub const THUMB_H: u32 = 180;
/// Turntable frame width, px.
pub const TURN_W: u32 = 160;
/// Turntable frame height, px.
pub const TURN_H: u32 = 90;
/// Turntable frames per revolution.
pub const TURN_FRAMES: u32 = 12;
/// Thumbnail view: yaw of the camera around the car, degrees (0 = straight ahead of the nose,
/// negative = towards the left / driver's side).
pub const THUMB_YAW_DEG: f32 = -35.0;
/// Camera elevation above the horizon, degrees.
pub const VIEW_ELEVATION_DEG: f32 = 14.0;

/// A thumbnail is stale when more than this fraction of its pixels moved by more than
/// [`STALE_TOLERANCE`] in some channel (float noise between platforms stays far below).
pub const STALE_FRACTION: f32 = 0.002;
/// Per-channel tolerance for [`STALE_FRACTION`].
pub const STALE_TOLERANCE: u8 = 12;

/// Whether a thumbnail read from disk still matches a fresh render.
pub fn is_current(on_disk: &Image, fresh: &Image) -> bool {
    on_disk.diff_fraction(fresh, STALE_TOLERANCE) <= STALE_FRACTION
}

/// Supersampling factor per axis.
const SS: u32 = 2;
/// Vertical field of view, degrees (long lens: little perspective distortion).
const FOV_DEG: f32 = 24.0;

/// Path of a car's thumbnail.
pub fn thumb_path(root: &Path, id: &str) -> PathBuf {
    root.join(format!("data/cars/thumbs/{id}.png"))
}

/// Path of a car's turntable strip.
pub fn turntable_path(root: &Path, id: &str) -> PathBuf {
    root.join(format!("data/cars/thumbs/{id}.turn.png"))
}

/// An RGBA8 image (straight alpha), row-major from the top.
#[derive(Debug, Clone, PartialEq)]
pub struct Image {
    /// Width, px.
    pub width: u32,
    /// Height, px.
    pub height: u32,
    /// `width × height × 4` bytes.
    pub rgba: Vec<u8>,
}

impl Image {
    /// Encode as PNG.
    pub fn to_png(&self) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut out, self.width, self.height);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_compression(png::Compression::High);
            let mut w = enc.write_header().expect("png header");
            w.write_image_data(&self.rgba).expect("png data");
        }
        out
    }

    /// Decode a PNG written by [`Image::to_png`].
    pub fn from_png(bytes: &[u8]) -> Result<Image, String> {
        let dec = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = dec.read_info().map_err(|e| e.to_string())?;
        let mut buf = vec![0; reader.output_buffer_size().ok_or("png too large")?];
        let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
        if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
            return Err("expected 8-bit RGBA".into());
        }
        buf.truncate(info.buffer_size());
        Ok(Image {
            width: info.width,
            height: info.height,
            rgba: buf,
        })
    }

    /// Fraction of pixels whose largest channel difference to `other` exceeds `tol` (1.0 when
    /// the sizes differ). Thumbnails are compared with a tolerance so float differences
    /// between platforms do not count as stale.
    pub fn diff_fraction(&self, other: &Image, tol: u8) -> f32 {
        if self.width != other.width || self.height != other.height {
            return 1.0;
        }
        let n = (self.width * self.height) as usize;
        let bad = self
            .rgba
            .chunks_exact(4)
            .zip(other.rgba.chunks_exact(4))
            .filter(|(a, b)| a.iter().zip(b.iter()).any(|(x, y)| x.abs_diff(*y) > tol))
            .count();
        bad as f32 / n.max(1) as f32
    }

    /// Paste `src` with its top-left corner at (`x`, `y`).
    fn blit(&mut self, src: &Image, x: u32, y: u32) {
        for row in 0..src.height {
            let d = (((y + row) * self.width + x) * 4) as usize;
            let s = (row * src.width * 4) as usize;
            let len = (src.width * 4) as usize;
            self.rgba[d..d + len].copy_from_slice(&src.rgba[s..s + len]);
        }
    }
}

/// Stock colour of a material slot (sRGB), from the mesh's tints where it has them.
fn slot_colour(mesh: &CarMesh, slot: MaterialSlot) -> [f32; 3] {
    use MaterialSlot as M;
    let tint = |s: M| mesh.tints.get(&s).copied();
    let rgb = match slot {
        M::Paint | M::Livery => tint(M::Paint).unwrap_or([175, 178, 182]),
        M::Glass => [38, 50, 62],
        M::Trim => [30, 30, 33],
        M::Headlight => [232, 232, 222],
        M::Taillight => [165, 22, 22],
        M::Rubber => [22, 22, 22],
        M::Metal => [150, 150, 156],
        M::Interior => [44, 41, 39],
        M::Chrome => [212, 212, 218],
        M::Carbon => [34, 34, 37],
        M::Caliper => tint(M::Caliper).unwrap_or([175, 30, 30]),
        M::Engine => [72, 72, 74],
        M::LightbarRed => [190, 22, 22],
        M::LightbarBlue => [22, 44, 200],
        M::LightbarClear => [222, 222, 222],
        M::EmissiveAmber => [235, 150, 25],
        M::Grille => [20, 20, 22],
        M::Exhaust => [120, 110, 100],
        M::Rim => tint(M::Rim).unwrap_or([150, 150, 156]),
        M::Spring => tint(M::Spring)
            .or_else(|| tint(M::Caliper))
            .unwrap_or([175, 30, 30]),
        M::Accent => tint(M::Accent).unwrap_or([30, 30, 33]),
        M::Underglow => tint(M::Underglow).unwrap_or([0, 170, 255]),
    };
    [rgb[0] as f32, rgb[1] as f32, rgb[2] as f32]
}

type V3 = [f32; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn dot(a: V3, b: V3) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: V3) -> V3 {
    let l = dot(a, a).sqrt().max(1e-9);
    [a[0] / l, a[1] / l, a[2] / l]
}

/// World-space triangles of a mesh with their slot colour.
fn triangles(mesh: &CarMesh, def: &VehicleDef) -> Vec<([V3; 3], [f32; 3])> {
    let mut out = Vec::new();
    for p in &mesh.parts {
        let off = match p.wheel {
            Some(i) => def
                .wheels
                .get(i as usize)
                .map_or([0.0; 3], |w| w.position_m),
            None => [0.0; 3],
        };
        let v = |i: u32| -> V3 {
            let k = i as usize * 3;
            [
                p.positions_mm[k] as f32 * 1e-3 + off[0],
                p.positions_mm[k + 1] as f32 * 1e-3 + off[1],
                p.positions_mm[k + 2] as f32 * 1e-3 + off[2],
            ]
        };
        let col = slot_colour(mesh, p.material);
        for t in p.indices.chunks_exact(3) {
            out.push(([v(t[0]), v(t[1]), v(t[2])], col));
        }
    }
    out
}

/// Render one view of a car: camera `yaw_deg` around it (0 = ahead of the nose, negative =
/// left side), [`VIEW_ELEVATION_DEG`] up, framed by the car's bounding sphere so every view of
/// one car has the same scale.
pub fn render(mesh: &CarMesh, def: &VehicleDef, yaw_deg: f32, width: u32, height: u32) -> Image {
    let tris = triangles(mesh, def);
    // Bounding box → centre and radius.
    let mut lo = [f32::MAX; 3];
    let mut hi = [f32::MIN; 3];
    for (t, _) in &tris {
        for v in t {
            for k in 0..3 {
                lo[k] = lo[k].min(v[k]);
                hi[k] = hi[k].max(v[k]);
            }
        }
    }
    if tris.is_empty() {
        lo = [-1.0; 3];
        hi = [1.0; 3];
    }
    let centre = [
        0.5 * (lo[0] + hi[0]),
        0.5 * (lo[1] + hi[1]),
        0.5 * (lo[2] + hi[2]),
    ];
    let radius = 0.5 * dot(sub(hi, lo), sub(hi, lo)).sqrt();

    let (yaw, elev) = (yaw_deg.to_radians(), VIEW_ELEVATION_DEG.to_radians());
    let to_cam = [elev.cos() * yaw.sin(), elev.cos() * yaw.cos(), elev.sin()];
    let half_fov = (0.5 * FOV_DEG).to_radians();
    let dist = radius / half_fov.sin();
    let eye = [
        centre[0] + to_cam[0] * dist,
        centre[1] + to_cam[1] * dist,
        centre[2] + to_cam[2] * dist,
    ];
    let fwd = norm(sub(centre, eye));
    let right = norm(cross(fwd, [0.0, 0.0, 1.0]));
    let up = cross(right, fwd);

    let (w, h) = (width * SS, height * SS);
    // The bounding sphere spans 82 % of the frame width: the car's length decides in side
    // views and a tall van still fits the 16:9 frame in the three-quarter view.
    let focal = 0.41 * w as f32 / half_fov.tan();
    let raw = |p: V3| -> (f32, f32, f32) {
        let v = sub(p, eye);
        let z = dot(v, fwd).max(1e-3);
        (dot(v, right) / z * focal, -dot(v, up) / z * focal, z)
    };
    // Centre the car's projected outline in the frame (perspective shifts it off the sphere
    // centre in three-quarter views).
    let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for (t, _) in &tris {
        for v in t {
            let (x, y, _) = raw(*v);
            x0 = x0.min(x);
            x1 = x1.max(x);
            y0 = y0.min(y);
            y1 = y1.max(y);
        }
    }
    let (ox, oy) = if tris.is_empty() {
        (0.5 * w as f32, 0.5 * h as f32)
    } else {
        (
            0.5 * w as f32 - 0.5 * (x0 + x1),
            0.5 * h as f32 - 0.5 * (y0 + y1),
        )
    };
    let project = |p: V3| -> (f32, f32, f32) {
        let (x, y, z) = raw(p);
        (x + ox, y + oy, z)
    };

    let mut colour = vec![[0.0f32; 4]; (w * h) as usize];
    let mut depth = vec![f32::INFINITY; (w * h) as usize];

    // Soft ground shadow: the footprint (body box at z = 0), 30 % black, under everything.
    let [bw, bl, _] = def.body_size_m;
    let (sx, sy) = (0.5 * bw + 0.08, 0.5 * bl + 0.08);
    let ground = [
        [centre[0] - sx, centre[1] - sy, 0.0],
        [centre[0] + sx, centre[1] - sy, 0.0],
        [centre[0] + sx, centre[1] + sy, 0.0],
        [centre[0] - sx, centre[1] + sy, 0.0],
    ];
    for tri in [[0, 1, 2], [0, 2, 3]] {
        let pts = tri.map(|i| project(ground[i]));
        raster(&pts, w, h, |i, _| {
            colour[i] = [0.0, 0.0, 0.0, 0.3];
        });
    }

    let light = norm([-0.35, 0.45, 0.82]);
    for (t, col) in &tris {
        let mut n = norm(cross(sub(t[1], t[0]), sub(t[2], t[0])));
        // Light the side that faces the camera (some parts are single-sided boxes).
        if dot(n, sub(eye, t[0])) < 0.0 {
            n = [-n[0], -n[1], -n[2]];
        }
        let shade = 0.42 + 0.58 * dot(n, light).max(0.0);
        let c = [
            (col[0] * shade).min(255.0),
            (col[1] * shade).min(255.0),
            (col[2] * shade).min(255.0),
            1.0,
        ];
        let pts = [project(t[0]), project(t[1]), project(t[2])];
        raster(&pts, w, h, |i, z| {
            if z < depth[i] {
                depth[i] = z;
                colour[i] = c;
            }
        });
    }

    // 2 × 2 box filter down to the output size (premultiplied, straight alpha out).
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for y in 0..height {
        for x in 0..width {
            let mut acc = [0.0f32; 4];
            for dy in 0..SS {
                for dx in 0..SS {
                    let c = colour[((y * SS + dy) * w + x * SS + dx) as usize];
                    let a = c[3];
                    acc[0] += c[0] * a;
                    acc[1] += c[1] * a;
                    acc[2] += c[2] * a;
                    acc[3] += a;
                }
            }
            let a = acc[3] / (SS * SS) as f32;
            let un = |v: f32| {
                if acc[3] > 0.0 {
                    (v / acc[3]).round().clamp(0.0, 255.0) as u8
                } else {
                    0
                }
            };
            rgba.extend_from_slice(&[
                un(acc[0]),
                un(acc[1]),
                un(acc[2]),
                (a * 255.0).round() as u8,
            ]);
        }
    }
    Image {
        width,
        height,
        rgba,
    }
}

/// Fill a screen-space triangle (pixel centres inside, either winding), calling `put(index,
/// depth)` with the interpolated camera depth.
fn raster(p: &[(f32, f32, f32); 3], w: u32, h: u32, mut put: impl FnMut(usize, f32)) {
    let area = (p[1].0 - p[0].0) * (p[2].1 - p[0].1) - (p[2].0 - p[0].0) * (p[1].1 - p[0].1);
    if area.abs() < 1e-6 {
        return;
    }
    let min_x = p
        .iter()
        .map(|q| q.0)
        .fold(f32::MAX, f32::min)
        .floor()
        .max(0.0) as u32;
    let max_x = (p.iter().map(|q| q.0).fold(f32::MIN, f32::max).ceil() as i64)
        .clamp(0, w as i64 - 1) as u32;
    let min_y = p
        .iter()
        .map(|q| q.1)
        .fold(f32::MAX, f32::min)
        .floor()
        .max(0.0) as u32;
    let max_y = (p.iter().map(|q| q.1).fold(f32::MIN, f32::max).ceil() as i64)
        .clamp(0, h as i64 - 1) as u32;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let w0 = ((p[1].0 - px) * (p[2].1 - py) - (p[2].0 - px) * (p[1].1 - py)) / area;
            let w1 = ((p[2].0 - px) * (p[0].1 - py) - (p[0].0 - px) * (p[2].1 - py)) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let z = w0 * p[0].2 + w1 * p[1].2 + w2 * p[2].2;
            put((y * w + x) as usize, z);
        }
    }
}

/// The thumbnail of a car ([`THUMB_W`] × [`THUMB_H`], [`THUMB_YAW_DEG`]).
pub fn thumbnail(mesh: &CarMesh, def: &VehicleDef) -> Image {
    render(mesh, def, THUMB_YAW_DEG, THUMB_W, THUMB_H)
}

/// The turntable strip of a car: [`TURN_FRAMES`] frames side by side, starting at
/// [`THUMB_YAW_DEG`] and turning the car anticlockwise seen from above.
pub fn turntable(mesh: &CarMesh, def: &VehicleDef) -> Image {
    let mut sheet = Image {
        width: TURN_W * TURN_FRAMES,
        height: TURN_H,
        rgba: vec![0; (TURN_W * TURN_FRAMES * TURN_H * 4) as usize],
    };
    for k in 0..TURN_FRAMES {
        let yaw = THUMB_YAW_DEG + 360.0 * k as f32 / TURN_FRAMES as f32;
        let frame = render(mesh, def, yaw, TURN_W, TURN_H);
        sheet.blit(&frame, k * TURN_W, 0);
    }
    sheet
}
