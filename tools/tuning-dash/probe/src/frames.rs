//! Top-down scene frames: SVG snapshots of a simcli run at a few moments (the first hard
//! contact, then evenly spaced), for the dashboard's point thumbnails.
//!
//! A frame shows what a camera straight above the scenario cars would see: roads and building
//! footprints (city runs), barriers, every vehicle in view as an oriented box coloured by who
//! drives it (scenario cars, police, traffic), and each scenario car's trail so far. It is
//! drawn from the simulation state, not from the renderer, so it costs a few microseconds per
//! frame and needs no browser.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use wp_contracts::{EntityId, SimEvent, VehicleSim};
use wp_sim::{Sim, VehicleKind};

/// Who drives a vehicle in a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameKind {
    /// One of the scenario's own entities (index in scenario order).
    Scenario(usize),
    /// An AI police car.
    Police,
    /// A physical traffic car.
    Traffic,
    /// Anything else (AI racers spawned by other systems).
    Other,
}

/// One vehicle in a frame.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FrameCar {
    /// Who drives it.
    pub kind: FrameKind,
    /// World position (x, y), m.
    pub pos: [f32; 2],
    /// Unit forward direction (x, y).
    pub fwd: [f32; 2],
    /// Body `[width, length]`, m.
    pub size: [f32; 2],
    /// Speed, m/s.
    pub speed_mps: f32,
}

/// One snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    /// Simulated time, s.
    pub t_s: f32,
    /// Why this frame was taken (`first hit`, `25 %`, `end` ...).
    pub note: String,
    /// Vehicles (all of them; the renderer crops to the view).
    pub cars: Vec<FrameCar>,
    /// Scenario cars' trails up to this frame, by scenario index.
    pub trails: Vec<Vec<[f32; 2]>>,
}

/// Static scenery drawn under every frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scenery {
    /// Road centre lines with their total width, m.
    pub roads: Vec<(Vec<[f32; 2]>, f32)>,
    /// Building footprints `(min, max)`.
    pub buildings: Vec<([f32; 2], [f32; 2])>,
    /// Barrier boxes `(centre, [size x, size y], yaw)`.
    pub barriers: Vec<([f32; 2], [f32; 2], f32)>,
}

impl Scenery {
    /// Roads and buildings from a city world (nothing for the flat world).
    pub fn from_sim(sim: &Sim, has_city: bool, barriers: &[simcli::scenario::Barrier]) -> Self {
        let mut s = Scenery {
            barriers: barriers
                .iter()
                .map(|b| ([b.pos[0], b.pos[1]], [b.size[0], b.size[1]], b.yaw))
                .collect(),
            ..Default::default()
        };
        if has_city {
            let g = sim.world().road_graph();
            for e in &g.edges {
                let lanes = (e.lanes_ab + e.lanes_ba).max(1) as f32;
                s.roads.push((e.polyline.clone(), lanes * 3.5));
            }
            s.buildings = g.occluders.iter().map(|o| (o.min, o.max)).collect();
        }
        s
    }
}

/// When to take frames and what has been taken so far.
#[derive(Debug, Clone)]
pub struct FrameCapture {
    /// Ticks for evenly spaced frames, with their notes.
    pub planned: BTreeMap<u64, String>,
    /// Closing speed above which a contact involving a scenario car is a "hit", m/s.
    pub hit_mps: f32,
    /// Frames taken.
    pub frames: Vec<Frame>,
    hit_taken: bool,
    trails: Vec<Vec<[f32; 2]>>,
    sizes: BTreeMap<String, [f32; 2]>,
}

/// Default number of evenly spaced frames (the last one is the end of the run).
pub const DEFAULT_FRAMES: u64 = 4;

/// Trail sample spacing, ticks (0.1 s).
const TRAIL_EVERY: u64 = 24;

impl FrameCapture {
    /// `count` evenly spaced frames over `ticks` (the last at the end), plus one at the first
    /// hit. `sizes` maps car id → `[width, length]`.
    pub fn new(ticks: u64, count: u64, sizes: BTreeMap<String, [f32; 2]>) -> Self {
        let mut planned = BTreeMap::new();
        let n = count.max(1);
        for k in 1..=n {
            let t = (ticks * k / n).max(1);
            let note = if k == n {
                "end".to_string()
            } else {
                format!("{} %", 100 * k / n)
            };
            planned.insert(t, note);
        }
        Self {
            planned,
            hit_mps: 3.0,
            frames: Vec::new(),
            hit_taken: false,
            trails: Vec::new(),
            sizes,
        }
    }

    /// Call after every tick with the tick's events and the number of scenario entities
    /// (they are the first `n_scenario` vehicles in slot order).
    pub fn on_tick(&mut self, sim: &Sim, events: &[SimEvent], n_scenario: usize) {
        let now = sim.tick();
        let vehicles = sim.vehicles();
        if self.trails.len() < n_scenario {
            self.trails.resize(n_scenario, Vec::new());
        }
        if now.is_multiple_of(TRAIL_EVERY) {
            for (i, v) in vehicles.iter().take(n_scenario).enumerate() {
                let p = v.vehicle.state().pos;
                self.trails[i].push([p[0], p[1]]);
            }
        }
        let scenario_ids: Vec<EntityId> = vehicles.iter().take(n_scenario).map(|v| v.id).collect();
        let mut note = self.planned.get(&now).cloned();
        if !self.hit_taken {
            let hit = events.iter().any(|e| match e {
                SimEvent::Contact {
                    a, b, rel_speed, ..
                } => {
                    rel_speed.abs() >= self.hit_mps
                        && (scenario_ids.contains(a) || scenario_ids.contains(b))
                }
                _ => false,
            });
            if hit {
                self.hit_taken = true;
                note = Some(match note {
                    Some(n) => format!("first hit, {n}"),
                    None => "first hit".to_string(),
                });
            }
        }
        if let Some(note) = note {
            self.take(sim, n_scenario, note);
        }
    }

    fn take(&mut self, sim: &Sim, n_scenario: usize, note: String) {
        let mut cars = Vec::new();
        for (i, v) in sim.vehicles().iter().enumerate() {
            let s = v.vehicle.state();
            let kind = if i < n_scenario {
                FrameKind::Scenario(i)
            } else {
                match v.kind {
                    VehicleKind::Police => FrameKind::Police,
                    VehicleKind::Traffic => FrameKind::Traffic,
                    _ => FrameKind::Other,
                }
            };
            cars.push(FrameCar {
                kind,
                pos: [s.pos[0], s.pos[1]],
                fwd: forward_xy(s.rot),
                size: self.sizes.get(&v.car_id).copied().unwrap_or([1.8, 4.5]),
                speed_mps: s.speed_mps,
            });
        }
        let mut trails = self.trails.clone();
        for (i, c) in cars.iter().take(n_scenario).enumerate() {
            trails[i].push(c.pos);
        }
        self.frames.push(Frame {
            t_s: sim.tick() as f32 * wp_contracts::PHYSICS_DT,
            note,
            cars,
            trails,
        });
    }
}

/// Chassis +Y in world XY for a quaternion `[x, y, z, w]` (normalised).
pub fn forward_xy(q: [f32; 4]) -> [f32; 2] {
    let fx = 2.0 * (q[0] * q[1] - q[3] * q[2]);
    let fy = 1.0 - 2.0 * (q[0] * q[0] + q[2] * q[2]);
    let n = (fx * fx + fy * fy).sqrt().max(1e-6);
    [fx / n, fy / n]
}

/// Frame colours (fixed: the SVG is shown as an image, the same in light and dark themes).
mod color {
    pub const GROUND: &str = "#e9e7e1";
    pub const BUILDING: &str = "#cfcbc2";
    pub const ROAD: &str = "#9b9a96";
    pub const BARRIER: &str = "#5b5a57";
    pub const POLICE: &str = "#2456c7";
    pub const TRAFFIC: &str = "#77746e";
    pub const OTHER: &str = "#8a5cc2";
    pub const TEXT: &str = "#2b2a28";
    /// Scenario cars, in scenario order (cycled).
    pub const SCENARIO: [&str; 4] = ["#d9480f", "#1a8a5a", "#b8860b", "#c2255c"];
}

/// Render a frame as an SVG of `w × h` px. The view is centred on the scenario cars, with at
/// least `min_span_m` across.
pub fn render_svg(frame: &Frame, scenery: &Scenery, w: u32, h: u32, min_span_m: f32) -> String {
    // View: bounding box of the scenario cars, padded.
    let mut lo = [f32::MAX; 2];
    let mut hi = [f32::MIN; 2];
    for c in &frame.cars {
        if let FrameKind::Scenario(_) = c.kind {
            for k in 0..2 {
                lo[k] = lo[k].min(c.pos[k]);
                hi[k] = hi[k].max(c.pos[k]);
            }
        }
    }
    if lo[0] > hi[0] {
        lo = [0.0, 0.0];
        hi = [0.0, 0.0];
    }
    let aspect = w as f32 / h as f32;
    let cx = 0.5 * (lo[0] + hi[0]);
    let cy = 0.5 * (lo[1] + hi[1]);
    let mut span_x = (hi[0] - lo[0] + 20.0).max(min_span_m);
    let mut span_y = (hi[1] - lo[1] + 20.0).max(min_span_m / aspect);
    if span_x / span_y > aspect {
        span_y = span_x / aspect;
    } else {
        span_x = span_y * aspect;
    }
    let (x0, y0) = (cx - 0.5 * span_x, cy - 0.5 * span_y);
    let k = w as f32 / span_x;
    let px = |p: [f32; 2]| -> (f32, f32) { ((p[0] - x0) * k, (y0 + span_y - p[1]) * k) };
    let in_view = |p: [f32; 2], margin: f32| {
        p[0] >= x0 - margin
            && p[0] <= x0 + span_x + margin
            && p[1] >= y0 - margin
            && p[1] <= y0 + span_y + margin
    };

    let mut s = String::new();
    let _ = write!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}">"#
    );
    let _ = write!(
        s,
        r#"<rect width="{w}" height="{h}" fill="{}"/>"#,
        color::GROUND
    );
    for (bmin, bmax) in &scenery.buildings {
        let overlaps =
            bmax[0] >= x0 && bmin[0] <= x0 + span_x && bmax[1] >= y0 && bmin[1] <= y0 + span_y;
        if !overlaps {
            continue;
        }
        let (ax, ay) = px([bmin[0], bmax[1]]);
        let _ = write!(
            s,
            r#"<rect x="{ax:.1}" y="{ay:.1}" width="{:.1}" height="{:.1}" fill="{}"/>"#,
            (bmax[0] - bmin[0]) * k,
            (bmax[1] - bmin[1]) * k,
            color::BUILDING
        );
    }
    for (line, width) in &scenery.roads {
        if !line.iter().any(|p| in_view(*p, 60.0)) {
            continue;
        }
        let pts: Vec<String> = line
            .iter()
            .map(|p| {
                let (a, b) = px(*p);
                format!("{a:.1},{b:.1}")
            })
            .collect();
        let _ = write!(
            s,
            r#"<polyline points="{}" fill="none" stroke="{}" stroke-width="{:.1}" stroke-linecap="round" stroke-linejoin="round"/>"#,
            pts.join(" "),
            color::ROAD,
            (width * k).max(1.0)
        );
    }
    for (c, size, yaw) in &scenery.barriers {
        let (ax, ay) = px(*c);
        let _ = write!(
            s,
            r#"<rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="{}" transform="rotate({:.1} {ax:.1} {ay:.1})"/>"#,
            ax - 0.5 * size[0] * k,
            ay - 0.5 * size[1] * k,
            (size[0] * k).max(1.0),
            (size[1] * k).max(1.0),
            color::BARRIER,
            -yaw.to_degrees()
        );
    }
    for (i, trail) in frame.trails.iter().enumerate() {
        if trail.len() < 2 {
            continue;
        }
        let pts: Vec<String> = trail
            .iter()
            .map(|p| {
                let (a, b) = px(*p);
                format!("{a:.1},{b:.1}")
            })
            .collect();
        let _ = write!(
            s,
            r#"<polyline points="{}" fill="none" stroke="{}" stroke-width="1.5" stroke-opacity="0.7" stroke-dasharray="3 2"/>"#,
            pts.join(" "),
            color::SCENARIO[i % color::SCENARIO.len()]
        );
    }
    // Scenario cars last so they sit on top.
    let mut order: Vec<&FrameCar> = frame.cars.iter().filter(|c| in_view(c.pos, 6.0)).collect();
    order.sort_by_key(|c| matches!(c.kind, FrameKind::Scenario(_)));
    for c in order {
        let fill = match c.kind {
            FrameKind::Scenario(i) => color::SCENARIO[i % color::SCENARIO.len()],
            FrameKind::Police => color::POLICE,
            FrameKind::Traffic => color::TRAFFIC,
            FrameKind::Other => color::OTHER,
        };
        let (ax, ay) = px(c.pos);
        // SVG rotation is clockwise with y down; the car's +Y (forward) points up at angle 0.
        let deg = c.fwd[0].atan2(c.fwd[1]).to_degrees();
        let (bw, bl) = ((c.size[0] * k).max(2.0), (c.size[1] * k).max(3.0));
        let _ = write!(
            s,
            r##"<g transform="translate({ax:.1} {ay:.1}) rotate({deg:.1})"><rect x="{:.1}" y="{:.1}" width="{bw:.1}" height="{bl:.1}" rx="{:.1}" fill="{fill}" stroke="#ffffff" stroke-width="0.6"/><rect x="{:.1}" y="{:.1}" width="{:.1}" height="{:.1}" fill="#ffffff" fill-opacity="0.75"/></g>"##,
            -0.5 * bw,
            -0.5 * bl,
            0.15 * bw,
            -0.35 * bw,
            -0.5 * bl + 0.12 * bl,
            0.7 * bw,
            0.12 * bl,
        );
    }
    let _ = write!(
        s,
        r#"<text x="4" y="{}" font-family="system-ui,sans-serif" font-size="11" fill="{}">t {:.1} s · {}</text>"#,
        h - 5,
        color::TEXT,
        frame.t_s,
        esc(&frame.note)
    );
    // 10 m scale bar.
    let bar = 10.0 * k;
    let _ = write!(
        s,
        r#"<line x1="{:.1}" y1="{}" x2="{:.1}" y2="{}" stroke="{}" stroke-width="2"/></svg>"#,
        w as f32 - 6.0 - bar,
        h - 6,
        w as f32 - 6.0,
        h - 6,
        color::TEXT
    );
    s
}

fn esc(t: &str) -> String {
    t.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Captions of frames in time order (`t 35.6 s · first hit`), for `info.frames`.
pub fn frame_notes(frames: &[Frame]) -> Vec<String> {
    let mut v: Vec<&Frame> = frames.iter().collect();
    v.sort_by(|a, b| a.t_s.total_cmp(&b.t_s));
    v.iter()
        .map(|f| format!("t {:.1} s · {}", f.t_s, f.note))
        .collect()
}

/// File name for frame `i` (`frames/f00.svg`).
pub fn frame_file(i: usize) -> String {
    format!("frames/f{i:02}.svg")
}

/// Write every frame as SVG under `dir/frames/`; returns the relative file names in frame
/// order. Frames are sorted by time first. [`frame_notes`] gives the matching captions.
pub fn write_frames(
    dir: &std::path::Path,
    frames: &mut [Frame],
    scenery: &Scenery,
) -> Result<Vec<String>, String> {
    frames.sort_by(|a, b| a.t_s.total_cmp(&b.t_s));
    let fdir = dir.join("frames");
    let _ = std::fs::remove_dir_all(&fdir);
    std::fs::create_dir_all(&fdir).map_err(|e| format!("{}: {e}", fdir.display()))?;
    let mut out = Vec::new();
    for (i, f) in frames.iter().enumerate() {
        let rel = frame_file(i);
        let svg = render_svg(f, scenery, 320, 200, 50.0);
        std::fs::write(dir.join(&rel), svg).map_err(|e| format!("{rel}: {e}"))?;
        out.push(rel);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> Frame {
        Frame {
            t_s: 1.5,
            note: "first hit".into(),
            cars: vec![
                FrameCar {
                    kind: FrameKind::Scenario(0),
                    pos: [0.0, 0.0],
                    fwd: [0.0, 1.0],
                    size: [1.8, 4.5],
                    speed_mps: 10.0,
                },
                FrameCar {
                    kind: FrameKind::Police,
                    pos: [3.0, 5.0],
                    fwd: [1.0, 0.0],
                    size: [1.9, 5.0],
                    speed_mps: 8.0,
                },
                FrameCar {
                    kind: FrameKind::Traffic,
                    pos: [900.0, 900.0],
                    fwd: [1.0, 0.0],
                    size: [1.9, 5.0],
                    speed_mps: 8.0,
                },
            ],
            trails: vec![vec![[0.0, -20.0], [0.0, 0.0]]],
        }
    }

    #[test]
    fn forward_of_identity_is_plus_y() {
        let f = forward_xy([0.0, 0.0, 0.0, 1.0]);
        assert!((f[0]).abs() < 1e-6 && (f[1] - 1.0).abs() < 1e-6);
        // 90° about +Z (yaw left): +Y → −X.
        let h = std::f32::consts::FRAC_1_SQRT_2;
        let f = forward_xy([0.0, 0.0, h, h]);
        assert!((f[0] + 1.0).abs() < 1e-5 && f[1].abs() < 1e-5, "{f:?}");
    }

    #[test]
    fn svg_draws_cars_in_view_only() {
        let sc = Scenery {
            roads: vec![(vec![[-100.0, 0.0], [100.0, 0.0]], 7.0)],
            buildings: vec![
                ([10.0, 10.0], [30.0, 30.0]),
                ([500.0, 500.0], [520.0, 520.0]),
            ],
            barriers: vec![([0.0, 15.0], [10.0, 1.0], 0.0)],
        };
        let svg = render_svg(&frame(), &sc, 320, 200, 50.0);
        assert!(svg.starts_with("<svg") && svg.ends_with("</svg>"));
        // Scenario car + police car; the far traffic car is cropped.
        assert_eq!(svg.matches("<g transform").count(), 2);
        assert!(svg.contains(color::POLICE) && !svg.contains(color::TRAFFIC));
        // One building in view, one road, the trail.
        assert_eq!(svg.matches(color::BUILDING).count(), 1);
        assert_eq!(svg.matches("<polyline").count(), 2);
        assert!(svg.contains("first hit"));
    }

    #[test]
    fn capture_plans_even_frames_ending_at_the_last_tick() {
        let c = FrameCapture::new(240, 4, BTreeMap::new());
        let ticks: Vec<u64> = c.planned.keys().copied().collect();
        assert_eq!(ticks, vec![60, 120, 180, 240]);
        assert_eq!(c.planned[&240], "end");
    }
}
