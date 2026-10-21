//! Pro-level driving for physical police units (lead `lead/police-driver`, P8).
//!
//! A [`ProDriver`] turns a unit's [`Track`] into driver input the way a trained pursuit driver
//! would, on top of `wp_world`'s shared pure-pursuit + PID controller:
//!
//! - **Racing line.** A window of the track ahead ([`ProParams::window_m`]) is resampled every
//!   [`ProParams::step_m`] and given a lateral offset per sample inside the drivable room
//!   ([`RoomQuery`]; on the city: the road's own-direction lanes, see [`RoadRoom`]) that
//!   minimises the summed squared discrete curvature (projected Gauss–Seidel, coarse to
//!   fine, like `wp_opponent::line`). The window starts at the car's current lateral position,
//!   so a re-plan never asks for a jump. Out-in-out falls out.
//! - **Speed profile at the grip limit.** `v ≤ √(a_lat / |κ|)` per sample, then a backward
//!   pass braking inside the friction ellipse, so braking starts as late as the car allows.
//!   `a_lat` and the braking limit are the car's measured [`CarLimits`] × usage × the grip the
//!   driver feels: road conditions (wet, cold; [`DriveCtx::road_grip`]), damage and a
//!   *confidence* that drops after every slide and recovers slowly.
//! - **Late braking.** The pedal command adds the profile's own deceleration ahead as a
//!   feed-forward, so the car is on the brakes the moment the profile asks for it instead of
//!   waiting for a speed error to build up.
//! - **Slide control.** Body slip past a dead band is countersteered (front wheels toward the
//!   velocity); past [`ProParams::slide_slip_rad`] the driver lifts and its confidence drops.
//!   Running wide of the line in a corner lifts too.
//! - **Recovery.** Facing away from the line: turn round at a crawl. Stuck nose-in (throttle
//!   on, not moving): reverse at opposite lock until the nose points at the line, then on.
//!
//! Deterministic: f32 only, fixed iteration order, polynomial `atan` (`wp_world::geom`), no
//! RNG.

use serde::{Deserialize, Serialize};
use wp_contracts::{VehicleInput, GRAVITY_MPS2};
use wp_world::drive::{DriveController, DriveGains};

use crate::graph::{
    add, closest_on_segment, cross, dot, len, normalize, scale, sub, RoadClass, RoadGraph,
};
use crate::unit::{lane_point, SteerGeometry, Track};

/// What a car can do (measured on the dry, undamaged; `wp_vehicle::metrics`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarLimits {
    /// Maximum steady lateral acceleration, g (skidpad).
    pub lat_g: f32,
    /// Mean deceleration of a 100–0 km/h stop, g.
    pub brake_g: f32,
    /// Mean acceleration 0–100 km/h, m/s².
    pub accel_mps2: f32,
    /// Top speed, m/s.
    pub top_speed_mps: f32,
}

impl CarLimits {
    /// Limits from the four standard measurements: skidpad g, 100–0 km/h stopping distance (m),
    /// 0–100 km/h time (s) and top speed (km/h).
    pub fn from_metrics(skidpad_g: f32, stop_100_m: f32, zero_100_s: f32, vmax_kmh: f32) -> Self {
        let v100 = 100.0 / 3.6;
        Self {
            lat_g: skidpad_g.max(0.3),
            brake_g: (v100 * v100 / (2.0 * stop_100_m.max(5.0)) / GRAVITY_MPS2).max(0.3),
            accel_mps2: (v100 / zero_100_s.max(1.0)).max(0.5),
            top_speed_mps: (vmax_kmh / 3.6).max(10.0),
        }
    }
}

/// Pro driver tuning (`data/police/pursuit.toml` `[pro]`; missing keys take the defaults).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ProParams {
    /// Limits assumed until the sim reports the unit's car (`PoliceSystem::set_unit_limits`).
    pub default_limits: CarLimits,
    /// Share of the car's lateral grip used at full confidence.
    pub lat_usage: f32,
    /// Share of the car's braking used at full confidence.
    pub brake_usage: f32,
    /// Racing-line window ahead of the car, m.
    pub window_m: f32,
    /// Line sample spacing, m.
    pub step_m: f32,
    /// Clearance kept from the edge of the drivable room beyond the half car width, m.
    pub edge_margin_m: f32,
    /// Half car width used for the line, m.
    pub half_width_m: f32,
    /// Lane width assumed where the road graph has none, m.
    pub lane_width_m: f32,
    /// How far past the centreline into the opposing lanes the line may go (two-way roads), m.
    pub opposing_m: f32,
    /// Smoothing passes that round the track's polyline corners before the line is optimised.
    pub corner_round_passes: u32,
    /// Racing line only with no other vehicle this far ahead near the road (otherwise keep
    /// the lane, where obstacle awareness looks), m.
    pub traffic_clear_m: f32,
    /// Re-plan the line at least this often, s.
    pub replan_s: f32,
    /// Shortest-path warm-up sweeps (coarsest level).
    pub smooth_sweeps: u32,
    /// Minimum-curvature sweeps per resolution level.
    pub curvature_sweeps: u32,
    /// Pure-pursuit look-ahead at a standstill, m.
    pub lookahead_base_m: f32,
    /// Look-ahead added per m/s, s.
    pub lookahead_per_mps: f32,
    /// Look-ahead bounds, m.
    pub lookahead_min_m: f32,
    /// Look-ahead bounds, m.
    pub lookahead_max_m: f32,
    /// Speed error → commanded acceleration, 1/s.
    pub speed_gain: f32,
    /// The speed command is read this far ahead in time (pedal and tyre lag), s.
    pub lead_s: f32,
    /// Body slip below this is normal cornering (no countersteer), rad.
    pub slip_deadband_rad: f32,
    /// Countersteer per rad of body slip past the dead band (1 = front wheels along the velocity).
    pub countersteer_gain: f32,
    /// Body slip that counts as a slide (lift, confidence drop), rad.
    pub slide_slip_rad: f32,
    /// Throttle allowed while sliding.
    pub slide_throttle: f32,
    /// Least throttle the friction-ellipse cap leaves at full cornering load.
    pub exit_throttle_min: f32,
    /// Confidence lost per slide or run-wide.
    pub confidence_drop: f32,
    /// Lowest confidence.
    pub confidence_min: f32,
    /// Confidence regained per second without a slide.
    pub confidence_regain_per_s: f32,
    /// Running wide: further than this outside the line in a corner, m.
    pub run_wide_m: f32,
    /// Speed while turning around to face the line, m/s.
    pub turn_around_mps: f32,
    /// Stuck: throttle on and slower than 0.5 m/s for this long, s (then reverse).
    pub stuck_s: f32,
    /// Longest reverse manoeuvre, s.
    pub reverse_s: f32,
    /// Shortest reverse manoeuvre (it ends once the nose points at the line), s.
    pub reverse_min_s: f32,
}

impl Default for ProParams {
    fn default() -> Self {
        Self {
            // Sentinel Cruiser (street spec), measured (`wp_opponent::CarCapability::measure`).
            default_limits: CarLimits {
                lat_g: 0.84,
                brake_g: 0.99,
                accel_mps2: 4.95,
                top_speed_mps: 82.3,
            },
            lat_usage: 0.95,
            brake_usage: 0.92,
            window_m: 220.0,
            step_m: 3.0,
            edge_margin_m: 0.5,
            half_width_m: 0.95,
            lane_width_m: 3.5,
            opposing_m: 0.5,
            corner_round_passes: 12,
            traffic_clear_m: 80.0,
            replan_s: 0.25,
            smooth_sweeps: 30,
            curvature_sweeps: 60,
            lookahead_base_m: 2.0,
            lookahead_per_mps: 0.33,
            lookahead_min_m: 5.0,
            lookahead_max_m: 30.0,
            speed_gain: 2.5,
            lead_s: 0.15,
            slip_deadband_rad: 0.05,
            countersteer_gain: 0.8,
            slide_slip_rad: 0.12,
            slide_throttle: 0.25,
            exit_throttle_min: 0.2,
            confidence_drop: 0.05,
            confidence_min: 0.8,
            confidence_regain_per_s: 0.02,
            run_wide_m: 1.2,
            turn_around_mps: 4.0,
            stuck_s: 1.5,
            reverse_s: 2.5,
            reverse_min_s: 1.5,
        }
    }
}

/// Drivable room around a point of a track: `(left, right)` distances, m, from `p` along the
/// track's left / right normal (tangent `t`) to the edge of where the unit may drive.
pub trait RoomQuery {
    /// Room at `p` for travel along unit tangent `t`.
    fn room(&self, p: [f32; 2], t: [f32; 2]) -> (f32, f32);
}

/// The same room everywhere (tests, open ground).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FixedRoom {
    /// Room to the left, m.
    pub left_m: f32,
    /// Room to the right, m.
    pub right_m: f32,
}

impl RoomQuery for FixedRoom {
    fn room(&self, _p: [f32; 2], _t: [f32; 2]) -> (f32, f32) {
        (self.left_m, self.right_m)
    }
}

/// Below this speed nothing counts as a slide (no countersteer, no lift), m/s.
const SLIDE_MIN_MPS: f32 = 5.0;

/// A junction box reaches this far past the widest road's lanes (parking lane + crosswalk), m.
const JUNCTION_EXTRA_M: f32 = 4.5;

/// Grid cell size of the [`RoadRoom`] index, m.
const ROOM_CELL_M: f32 = 25.0;

/// Drivable room on the police road graph: the own-direction lanes of the road under a point,
/// plus [`ProParams::opposing_m`] across the centreline of a two-way road. Segments are kept in
/// a uniform grid, so a query looks at a handful of roads.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RoadRoom {
    /// Per segment: start, end, lanes forward / back, lane width.
    segs: Vec<RoomSeg>,
    origin: [f32; 2],
    nx: usize,
    ny: usize,
    cells: Vec<Vec<u32>>,
    lane_width_m: f32,
    opposing_m: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct RoomSeg {
    a: [f32; 2],
    b: [f32; 2],
    fwd: f32,
    back: f32,
    oneway: bool,
    /// Junction box radius around `a` / `b` (0 = not a junction), m: inside it the whole
    /// paved width is usable (crossing the box on a racing line is what a pro does).
    box_m: [f32; 2],
}

impl RoadRoom {
    /// Room from the police road graph (hand-built graphs, or when no city map is at hand).
    /// Segments without lane data get one lane each way of `params.lane_width_m` (two per
    /// direction on arterials and highways). On `wp_world` cities prefer
    /// [`RoadRoom::from_city`]: the graph's polylines follow a lane, not the road centre.
    pub fn new(graph: &RoadGraph, params: &ProParams) -> Self {
        let mut segs = Vec::with_capacity(graph.segments.len());
        for s in &graph.segments {
            let a = graph.node(s.a).pos;
            let b = graph.node(s.b).pos;
            let w = if s.lane_width_m > 0.0 {
                s.lane_width_m
            } else {
                params.lane_width_m
            };
            let (nf, nb) = if s.lanes_fwd > 0 || s.lanes_back > 0 {
                (s.lanes_fwd as f32, s.lanes_back as f32)
            } else {
                let n = match s.class {
                    RoadClass::Local => 1.0,
                    RoadClass::Arterial | RoadClass::Highway => 2.0,
                };
                (n, if s.oneway { 0.0 } else { n })
            };
            let oneway = nb == 0.0;
            let (fwd, back) = if oneway {
                (nf * w * 0.5, nf * w * 0.5)
            } else {
                (nf * w, nb * w)
            };
            segs.push(RoomSeg {
                a,
                b,
                fwd,
                back,
                oneway,
                box_m: [0.0, 0.0],
            });
        }
        // Junction boxes reach as far as the stop lines: the widest road's half width plus
        // a parking lane and the crosswalk (`wp_world::builder` setbacks: half width +
        // parking 2.5 m + crosswalk 2 m at a right angle).
        let mut box_r = vec![0.0f32; graph.nodes.len()];
        for (s, r) in graph.segments.iter().zip(&segs) {
            let w = r.fwd.max(r.back) + JUNCTION_EXTRA_M;
            for n in [s.a, s.b] {
                if graph.node(n).junction {
                    box_r[n.0 as usize] = box_r[n.0 as usize].max(w);
                }
            }
        }
        for (s, r) in graph.segments.iter().zip(segs.iter_mut()) {
            r.box_m = [box_r[s.a.0 as usize], box_r[s.b.0 as usize]];
        }
        Self::index(segs, params)
    }

    /// Room from the city's real geometry: every road's paved surface (trimmed at its junction
    /// setbacks) with its own-direction lane widths (parking lanes excluded, bus lanes
    /// included), plus the gaps from each surface end to its node: a junction's whole box, or a
    /// bend's connector. Use this on `wp_world` cities: the contract road network's polylines
    /// follow a lane, not the road centre.
    pub fn from_city(map: &wp_world::CityMap, params: &ProParams) -> Self {
        use wp_world::network::LaneKind;
        let net = &map.net;
        let mut segs = Vec::new();
        for surf in &net.surfaces {
            let c: Vec<[f32; 2]> = surf.centre.iter().map(|p| [p[0], p[1]]).collect();
            if c.len() < 2 {
                continue;
            }
            let dir = normalize(sub(c[c.len() - 1], c[0]));
            let (mut fwd, mut back) = (0.0f32, 0.0f32);
            let (mut node_a, mut node_b) = (None, None);
            for l in net.lanes.iter().filter(|l| l.road == surf.road) {
                if l.kind == LaneKind::Parking || l.polyline.len() < 2 {
                    continue;
                }
                let (p0, p1) = (l.polyline[0], l.polyline[l.polyline.len() - 1]);
                let ld = sub([p1[0], p1[1]], [p0[0], p0[1]]);
                if dot(ld, dir) >= 0.0 {
                    fwd += l.width_m;
                    node_a = Some(l.from_node);
                    node_b = Some(l.to_node);
                } else {
                    back += l.width_m;
                    node_a = node_a.or(Some(l.to_node));
                    node_b = node_b.or(Some(l.from_node));
                }
            }
            if fwd + back <= 0.0 {
                continue;
            }
            let oneway = fwd == 0.0 || back == 0.0;
            let (f, b) = if oneway {
                let h = 0.5 * (fwd + back);
                (h, h)
            } else {
                (fwd, back)
            };
            let piece = |a: [f32; 2], bb: [f32; 2], junction: bool| RoomSeg {
                a,
                b: bb,
                fwd: f,
                back: b,
                oneway,
                box_m: if junction {
                    [f32::INFINITY; 2]
                } else {
                    [0.0, 0.0]
                },
            };
            let node = |n: Option<u32>| {
                n.map(|n| {
                    let nd = &net.nodes[n as usize];
                    ([nd.pos[0], nd.pos[1]], nd.intersection.is_some())
                })
            };
            if let Some((pa, j)) = node(node_a) {
                if len(sub(c[0], pa)) > 0.3 {
                    segs.push(piece(pa, c[0], j));
                }
            }
            for w in c.windows(2) {
                if len(sub(w[1], w[0])) > 1e-3 {
                    segs.push(piece(w[0], w[1], false));
                }
            }
            if let Some((pb, j)) = node(node_b) {
                if len(sub(pb, c[c.len() - 1])) > 0.3 {
                    segs.push(piece(c[c.len() - 1], pb, j));
                }
            }
        }
        Self::index(segs, params)
    }

    /// Build the grid over `segs`.
    fn index(segs: Vec<RoomSeg>, params: &ProParams) -> Self {
        let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
        for s in &segs {
            for p in [s.a, s.b] {
                lo = [lo[0].min(p[0]), lo[1].min(p[1])];
                hi = [hi[0].max(p[0]), hi[1].max(p[1])];
            }
        }
        if segs.is_empty() {
            return Self {
                lane_width_m: params.lane_width_m,
                opposing_m: params.opposing_m,
                ..Self::default()
            };
        }
        let pad = 2.0 * ROOM_CELL_M;
        let origin = [lo[0] - pad, lo[1] - pad];
        let nx = (((hi[0] - lo[0]) + 2.0 * pad) / ROOM_CELL_M) as usize + 1;
        let ny = (((hi[1] - lo[1]) + 2.0 * pad) / ROOM_CELL_M) as usize + 1;
        let mut cells = vec![Vec::new(); nx * ny];
        for (i, s) in segs.iter().enumerate() {
            // Every cell the segment's box (grown by its road width) touches.
            let g = s.fwd.max(s.back) + 2.0;
            let (x0, x1) = (s.a[0].min(s.b[0]) - g, s.a[0].max(s.b[0]) + g);
            let (y0, y1) = (s.a[1].min(s.b[1]) - g, s.a[1].max(s.b[1]) + g);
            let cx0 = ((x0 - origin[0]) / ROOM_CELL_M).max(0.0) as usize;
            let cx1 = (((x1 - origin[0]) / ROOM_CELL_M).max(0.0) as usize).min(nx - 1);
            let cy0 = ((y0 - origin[1]) / ROOM_CELL_M).max(0.0) as usize;
            let cy1 = (((y1 - origin[1]) / ROOM_CELL_M).max(0.0) as usize).min(ny - 1);
            for cy in cy0..=cy1 {
                for cx in cx0..=cx1 {
                    cells[cy * nx + cx].push(i as u32);
                }
            }
        }
        Self {
            segs,
            origin,
            nx,
            ny,
            cells,
            lane_width_m: params.lane_width_m,
            opposing_m: params.opposing_m,
        }
    }

    fn cell(&self, p: [f32; 2]) -> Option<&[u32]> {
        if self.cells.is_empty() {
            return None;
        }
        let cx = (p[0] - self.origin[0]) / ROOM_CELL_M;
        let cy = (p[1] - self.origin[1]) / ROOM_CELL_M;
        if cx < 0.0 || cy < 0.0 || cx as usize >= self.nx || cy as usize >= self.ny {
            return None;
        }
        Some(&self.cells[cy as usize * self.nx + cx as usize])
    }
}

impl RoomQuery for RoadRoom {
    /// Room in the track frame from the roads within 1.5 m of the nearest one whose direction
    /// is within ~70° of the track (the union where several qualify, e.g. in a junction box).
    /// Off the road graph: one lane width either side.
    fn room(&self, p: [f32; 2], t: [f32; 2]) -> (f32, f32) {
        let fallback = (self.lane_width_m, self.lane_width_m);
        let Some(cands) = self.cell(p) else {
            return fallback;
        };
        let mut near: Vec<(f32, u32, bool)> = Vec::new();
        let mut best = f32::INFINITY;
        for &i in cands {
            let s = &self.segs[i as usize];
            let d = normalize(sub(s.b, s.a));
            if dot(d, t).abs() < 0.35 {
                continue;
            }
            let (t, q) = closest_on_segment(s.a, s.b, p);
            let dd = len(sub(p, q));
            best = best.min(dd);
            let seg_len = len(sub(s.b, s.a));
            let in_box = t * seg_len < s.box_m[0] || (1.0 - t) * seg_len < s.box_m[1];
            near.push((dd, i, in_box));
        }
        if near.is_empty() {
            return fallback;
        }
        let (mut left, mut right) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for (dd, i, in_box) in near {
            if dd > best + 1.5 {
                continue;
            }
            let s = &self.segs[i as usize];
            let d = normalize(sub(s.b, s.a));
            let r = [d[1], -d[0]];
            // Lateral of p from the road centre, + = right of a → b.
            let lat = dot(sub(p, s.a), r);
            let same = dot(d, t) >= 0.0;
            // Usable band in the road frame (+ = right of a → b).
            let (lo, hi) = if s.oneway || in_box {
                (-s.back, s.fwd)
            } else if same {
                (-self.opposing_m, s.fwd)
            } else {
                (-s.back, self.opposing_m)
            };
            // Into the track frame (+ = right of t).
            let (l, rr) = if same {
                (lat - lo, hi - lat)
            } else {
                (hi - lat, lat - lo)
            };
            left = left.max(l);
            right = right.max(rr);
        }
        (left, right)
    }
}

/// What the driver feels of a physical car, reported by the sim
/// (`PoliceSystem::sync_unit_feel`) after [`crate::UnitSync`].
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct UnitFeel {
    /// Yaw rate, rad/s (+ = left).
    pub yaw_rate: f32,
    /// Grip of the road under the car relative to the same surface dry (wet, cold, paint,
    /// aquaplaning; `wp_vehicle::Vehicle::conditions_grip`), 0..1. `None` = not reported:
    /// assume dry.
    pub road_grip: Option<f32>,
}

/// What the driver knows about its car this tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriveCtx {
    /// Position (XY), m.
    pub pos: [f32; 2],
    /// Unit forward (XY).
    pub heading: [f32; 2],
    /// Velocity (XY), m/s.
    pub vel: [f32; 2],
    /// Yaw rate, rad/s (+ = left).
    pub yaw_rate: f32,
    /// Grip of the road under the car relative to dry asphalt (wet, cold), 0..1.
    pub road_grip: f32,
    /// Grip left after damage and punctures (worst wheel), 0..1.
    pub damage_grip: f32,
    /// Engine power left, 0..1.
    pub power_scale: f32,
    /// Speed the tactic asks for (pursuit cap, limit factor, obstacle cap), m/s.
    pub target_mps: f32,
    /// Corner-planning cap from the assignment, g (patrol drives gently); `None` = the limit.
    pub cornering_cap_g: Option<f32>,
    /// Braking-planning cap, m/s² (patrol brakes comfortably); `None` = the limit.
    pub braking_cap_mps2: Option<f32>,
    /// Lane offset of the command (+ right), m: the line starts and ends here, and the whole
    /// line stays on it when the racing line is off.
    pub lane_offset_m: f32,
    /// An obstacle pass in progress: drive exactly this offset, no racing line.
    pub pass_offset_m: Option<f32>,
    /// Use the racing line (lights on); otherwise keep the lane.
    pub racing_line: bool,
    /// Brakes held (wrecked, out of service).
    pub disabled: bool,
}

/// One [`ProDriver::drive`] result.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ProOutput {
    /// Driver input.
    pub input: VehicleInput,
    /// Arc length of the car on the track, m.
    pub s: f32,
    /// Speed the profile asks for now, m/s.
    pub planned_mps: f32,
    /// Lateral offset of the line at the car (+ right of the track), m.
    pub line_offset_m: f32,
    /// Body slip beyond the rolling (kinematic) slip, rad (+ = velocity left of the nose).
    pub slip_rad: f32,
    /// Sliding (past [`ProParams::slide_slip_rad`]) this tick.
    pub sliding: bool,
    /// Grip confidence, 0..1.
    pub confidence: f32,
    /// Reversing out of a stuck position.
    pub reversing: bool,
}

/// The line and speed profile over the current window.
#[derive(Debug, Clone, Default, PartialEq)]
struct Window {
    /// Track arc length of the first sample, m.
    s0: f32,
    /// Sample spacing, m.
    step: f32,
    /// Line points.
    pts: Vec<[f32; 2]>,
    /// Unit right normal of the reference centre per sample.
    nrm: Vec<[f32; 2]>,
    /// Lateral offset per sample (+ right).
    off: Vec<f32>,
    /// Speed profile per sample, m/s.
    v: Vec<f32>,
    /// Signed curvature of the line per sample, 1/m (+ = left).
    kappa: Vec<f32>,
    /// Track identity the window was built for.
    key: (usize, u32, u32, u32),
}

impl Window {
    fn index_at(&self, s: f32) -> f32 {
        ((s - self.s0) / self.step).max(0.0)
    }

    fn sample<T: Copy + Into<f32>>(&self, data: &[T], s: f32) -> f32 {
        let n = data.len();
        if n == 0 {
            return 0.0;
        }
        let x = self.index_at(s);
        let i = (x as usize).min(n - 1);
        let j = (i + 1).min(n - 1);
        let k = (x - i as f32).clamp(0.0, 1.0);
        data[i].into() * (1.0 - k) + data[j].into() * k
    }

    fn point_at(&self, s: f32) -> [f32; 2] {
        let n = self.pts.len();
        let x = self.index_at(s);
        let i = (x as usize).min(n - 1);
        let j = (i + 1).min(n - 1);
        let k = (x - i as f32).clamp(0.0, 1.0);
        add(scale(self.pts[i], 1.0 - k), scale(self.pts[j], k))
    }

    fn end_s(&self) -> f32 {
        self.s0 + self.step * (self.pts.len().saturating_sub(1)) as f32
    }
}

fn track_key(t: &Track) -> (usize, u32, u32, u32) {
    let f = t.pts.first().copied().unwrap_or([0.0, 0.0]);
    (
        t.pts.len(),
        f[0].to_bits(),
        f[1].to_bits(),
        t.length().to_bits(),
    )
}

/// Pro police driver for one unit. Keep one per unit; call [`ProDriver::drive`] every
/// physics tick.
#[derive(Debug, Clone, PartialEq)]
pub struct ProDriver {
    /// Tuning.
    pub params: ProParams,
    /// The car's limits.
    pub limits: CarLimits,
    ctrl: DriveController,
    win: Window,
    s_hint: f32,
    since_plan_s: f32,
    confidence: f32,
    sliding: bool,
    running_wide: bool,
    stuck_s: f32,
    reverse_left_s: f32,
    throttle: f32,
}

impl ProDriver {
    /// New driver for a car with this steering geometry and limits, at `speed_mps`.
    pub fn new(
        params: ProParams,
        limits: CarLimits,
        geometry: SteerGeometry,
        speed_mps: f32,
    ) -> Self {
        let gains = DriveGains {
            lookahead_base_m: params.lookahead_base_m,
            lookahead_per_mps: params.lookahead_per_mps,
            lookahead_min_m: params.lookahead_min_m,
            lookahead_max_m: params.lookahead_max_m,
            ..DriveGains::TRAFFIC
        };
        Self {
            ctrl: DriveController::new(
                gains,
                geometry.wheelbase_m,
                geometry.max_steer_rad,
                speed_mps,
            ),
            params,
            limits,
            win: Window::default(),
            s_hint: 0.0,
            since_plan_s: f32::INFINITY,
            confidence: 1.0,
            sliding: false,
            running_wide: false,
            stuck_s: 0.0,
            reverse_left_s: 0.0,
            throttle: 0.0,
        }
    }

    /// The current line window: `(track arc length of the first point, spacing, points,
    /// speed profile)` (debug views and tests).
    pub fn line(&self) -> (f32, f32, &[[f32; 2]], &[f32]) {
        (self.win.s0, self.win.step, &self.win.pts, &self.win.v)
    }

    /// Grip confidence, 0..1.
    pub fn confidence(&self) -> f32 {
        self.confidence
    }

    /// Forget the line (new track, teleport); keeps the confidence.
    pub fn reset(&mut self, speed_mps: f32) {
        self.win = Window::default();
        self.since_plan_s = f32::INFINITY;
        self.ctrl.resync(speed_mps);
        self.stuck_s = 0.0;
        self.reverse_left_s = 0.0;
    }

    /// Usable lateral and braking acceleration now, m/s².
    fn accel_limits(&self, ctx: &DriveCtx) -> (f32, f32) {
        let p = &self.params;
        let grip = (ctx.road_grip * ctx.damage_grip).clamp(0.05, 1.0) * self.confidence;
        let mut lat = self.limits.lat_g * p.lat_usage * grip * GRAVITY_MPS2;
        if let Some(cap) = ctx.cornering_cap_g {
            lat = lat.min(cap * GRAVITY_MPS2);
        }
        let mut brk = self.limits.brake_g * p.brake_usage * grip * GRAVITY_MPS2;
        if let Some(cap) = ctx.braking_cap_mps2 {
            brk = brk.min(cap);
        }
        (lat.max(0.5), brk.max(0.5))
    }

    /// Build the line and speed profile over the window ahead of arc length `s`.
    fn plan(&mut self, ctx: &DriveCtx, track: &Track, room: &dyn RoomQuery, s: f32) {
        let p = self.params.clone();
        let step = p.step_m.max(0.5);
        let end = track.length();
        let n = (((end - s).min(p.window_m)) / step) as usize + 1;
        let n = n.max(2);
        let hw = p.half_width_m + p.edge_margin_m;
        let fixed = ctx
            .pass_offset_m
            .or((!ctx.racing_line).then_some(ctx.lane_offset_m));
        let raw: Vec<[f32; 2]> = (0..n)
            .map(|i| track.point_at((s + i as f32 * step).min(end)))
            .collect();
        // Reference centre: the track with its polyline corners rounded (a few metres), so the
        // samples stay evenly spaced on either side of a corner. Junction corners are sharp
        // polyline vertices; optimising against them pushes the line to the outside, where
        // the samples spread out.
        let ctr = if fixed.is_none() {
            round_corners(&raw, p.corner_round_passes)
        } else {
            raw
        };
        let mut nrm = Vec::with_capacity(n);
        let mut lim = Vec::with_capacity(n);
        for i in 0..n {
            let (a, b) = (ctr[i.saturating_sub(1)], ctr[(i + 1).min(n - 1)]);
            let mut t = normalize(sub(b, a));
            if len(t) < 0.5 {
                t = normalize(sub(track.point_at(s + 1.0), track.point_at(s - 1.0)));
            }
            let r = [t[1], -t[0]];
            let (l, rr) = room.room(ctr[i], t);
            let (lo, hi) = (-l + hw, rr - hw);
            let (lo, hi) = if lo <= hi {
                (lo, hi)
            } else {
                let m = 0.5 * (lo + hi);
                (m, m)
            };
            nrm.push(r);
            lim.push((lo, hi));
        }
        // The car's lateral from the reference centre (+ right).
        let lat_now = dot(sub(ctx.pos, ctr[0]), nrm[0]);
        let mut off: Vec<f32> = match fixed {
            Some(o) => vec![o; n],
            None => lim
                .iter()
                .map(|(lo, hi)| ctx.lane_offset_m.clamp(*lo, *hi))
                .collect(),
        };
        // The line starts where the car is (no jump on a re-plan) and blends from there.
        off[0] = lat_now;
        if n > 1 {
            off[1] = lat_now + (off[1] - lat_now) * 0.5;
        }
        if fixed.is_none() && n >= 8 {
            optimise_line(&ctr, &nrm, &lim, &mut off, &p);
        } else if let Some(o) = fixed {
            // Blend from the car's position into the fixed offset over ~a car length.
            for (i, x) in off.iter_mut().enumerate().skip(1) {
                let k = (i as f32 * step / 8.0).min(1.0);
                *x = lat_now + (o - lat_now) * k;
            }
        }
        let pts: Vec<[f32; 2]> = (0..n).map(|i| add(ctr[i], scale(nrm[i], off[i]))).collect();
        let mut kappa = vec![0.0f32; n];
        for i in 1..n.saturating_sub(1) {
            kappa[i] = signed_curvature(pts[i - 1], pts[i], pts[i + 1]);
        }
        if n >= 3 {
            kappa[0] = kappa[1];
            kappa[n - 1] = kappa[n - 2];
        }
        // Smooth the curvature a little (discrete noise would read as kinks).
        let raw = kappa.clone();
        for i in 1..n.saturating_sub(1) {
            kappa[i] = 0.25 * raw[i - 1] + 0.5 * raw[i] + 0.25 * raw[i + 1];
        }
        let (a_lat, a_brk) = self.accel_limits(ctx);
        let top = ctx.target_mps.min(self.limits.top_speed_mps).max(0.0);
        let mut v: Vec<f32> = kappa
            .iter()
            .map(|k| {
                if k.abs() > 1e-5 {
                    (a_lat / k.abs()).sqrt().min(top)
                } else {
                    top
                }
            })
            .collect();
        // Track end inside the window: stop there.
        let ends_here = s + p.window_m >= end;
        if ends_here {
            v[n - 1] = 0.0;
        }
        // Backward pass: brake inside the friction ellipse.
        for i in (0..n - 1).rev() {
            let vn = v[i + 1];
            let used = (vn * vn * kappa[i].abs() / a_lat).min(1.0);
            let avail = a_brk * (1.0 - used * used).max(0.0).sqrt();
            let vb = (vn * vn + 2.0 * avail.max(0.3) * step).sqrt();
            v[i] = v[i].min(vb);
        }
        self.win = Window {
            s0: s,
            step,
            pts,
            nrm,
            off,
            v,
            kappa,
            key: track_key(track),
        };
        self.since_plan_s = 0.0;
    }

    /// One control step for a car following `track`. `dt` = time since the last call, s.
    pub fn drive(
        &mut self,
        ctx: &DriveCtx,
        track: &Track,
        room: &dyn RoomQuery,
        dt: f32,
    ) -> ProOutput {
        let p = self.params.clone();
        let speed = len(ctx.vel);
        let fwd_speed = dot(ctx.vel, ctx.heading);
        let mut out = ProOutput {
            confidence: self.confidence,
            ..Default::default()
        };
        if !track.is_drivable() {
            let (_, b) = self.ctrl.pedals(fwd_speed, -10.0, dt, true);
            out.input.brake = b;
            return out;
        }
        let (s, _) = track.project_ahead(ctx.pos, self.s_hint, 3.0, 40.0);
        self.s_hint = s;
        out.s = s;

        // Body slip (+ = velocity left of the nose), less the slip a car rolling without any
        // tyre slip has at its centre (the rear axle's path is inside the centre's: half a
        // wheelbase × yaw rate / speed; large in a tight turn at a crawl, not a slide).
        let slip = if speed > SLIDE_MIN_MPS {
            let vd = scale(ctx.vel, 1.0 / speed);
            let body = wp_world::geom::atan2(cross(ctx.heading, vd), dot(ctx.heading, vd));
            body - 0.5 * self.ctrl.wheelbase_m * ctx.yaw_rate / speed
        } else {
            0.0
        };
        out.slip_rad = slip;

        self.since_plan_s += dt;
        let stale = self.win.pts.len() < 2
            || self.win.key != track_key(track)
            || self.since_plan_s >= p.replan_s
            || s > self.win.s0 + 0.3 * (self.win.end_s() - self.win.s0)
            || s < self.win.s0 - 1.0;
        if stale {
            // Mid-slide the car's position is not a good line start: keep the old line.
            if !(self.sliding && self.win.pts.len() >= 2 && self.win.key == track_key(track)) {
                self.plan(ctx, track, room, s);
            }
        }

        // ---- Slide and run-wide awareness ----
        let was_sliding = self.sliding;
        self.sliding = slip.abs() > p.slide_slip_rad;
        if self.sliding && !was_sliding {
            self.confidence = (self.confidence - p.confidence_drop).max(p.confidence_min);
        }
        let k_here = self.win.sample(&self.win.kappa, s);
        let line_here = self.win.sample(&self.win.off, s);
        let i_here = (self.win.index_at(s) as usize).min(self.win.nrm.len() - 1);
        let lat_line = dot(sub(ctx.pos, self.win.point_at(s)), self.win.nrm[i_here]);
        // Outside of the corner = right in a left-hander (κ > 0).
        let wide = lat_line * k_here.signum();
        let was_wide = self.running_wide;
        self.running_wide = k_here.abs() > 1.0 / 120.0 && wide > p.run_wide_m;
        if self.running_wide && !was_wide {
            self.confidence = (self.confidence - 0.5 * p.confidence_drop).max(p.confidence_min);
        }
        if !self.sliding && !self.running_wide {
            self.confidence = (self.confidence + p.confidence_regain_per_s * dt).min(1.0);
        }
        out.sliding = self.sliding;
        out.confidence = self.confidence;
        out.line_offset_m = line_here;

        // ---- Speed ----
        let lead = s + fwd_speed.max(0.0) * p.lead_s;
        let mut v_ref = self.win.sample(&self.win.v, lead);
        // Facing away from where the line goes: turn around at a crawl first.
        // (Measured against the lane on the track itself: a line re-planned from a car
        // pointing the wrong way starts sideways and would hold it at full lock.)
        let lane = ctx.pass_offset_m.unwrap_or(ctx.lane_offset_m);
        let to_line = sub(lane_point(track, s + 8.0, lane), ctx.pos);
        let turning_round = len(to_line) > 1.0 && dot(normalize(to_line), ctx.heading) < 0.3;
        if turning_round {
            v_ref = v_ref.min(p.turn_around_mps);
        }
        out.planned_mps = v_ref;
        let ahead = lead + 4.0;
        let v_ahead = self.win.sample(&self.win.v, ahead);
        let ff = if v_ahead < v_ref {
            (v_ahead * v_ahead - v_ref * v_ref) / (2.0 * 4.0)
        } else {
            0.0
        };
        let (_, a_brk) = self.accel_limits(ctx);
        let mut acc = p.speed_gain * (v_ref - fwd_speed) + ff.min(0.0);
        acc = acc.clamp(-1.3 * a_brk, 2.0 * self.limits.accel_mps2.max(1.0));

        // ---- Steering ----
        let on_line = ctx.racing_line && ctx.pass_offset_m.is_none() && !turning_round;
        let target = if on_line {
            let look = self.ctrl.lookahead(fwd_speed.max(0.0).min(v_ref.max(5.0)));
            self.win.point_at(s + look)
        } else {
            // In the lane (patrol, passing, traffic ahead, turning round): exactly the lane
            // tracking every unit had before, the traffic look-ahead on the lane itself.
            let g = DriveGains::TRAFFIC;
            let look = (g.lookahead_base_m + g.lookahead_per_mps * fwd_speed.abs())
                .clamp(g.lookahead_min_m, g.lookahead_max_m);
            lane_point(track, s + look, lane)
        };
        let to = sub(target, ctx.pos);
        let mut steer = self
            .ctrl
            .steer(ctx.pos, ctx.heading, [target[0], target[1], 0.0]);
        if dot(ctx.heading, to) < 0.0 {
            // Target behind: pure pursuit degenerates; turn at full lock.
            steer = if cross(ctx.heading, to) >= 0.0 {
                -1.0
            } else {
                1.0
            };
        }
        // Countersteer past the dead band: + slip (velocity to the left) wants the wheels left
        // (negative steer input).
        let excess = slip.signum() * (slip.abs() - p.slip_deadband_rad).max(0.0);
        if fwd_speed > SLIDE_MIN_MPS {
            steer -= p.countersteer_gain * excess / self.ctrl.max_steer_rad;
        }
        let steer = steer.clamp(-1.0, 1.0);

        // ---- Stuck: reverse out ----
        if self.reverse_left_s > 0.0 {
            self.reverse_left_s -= dt;
            let reversed_s = p.reverse_s - self.reverse_left_s;
            // Back out until the nose points at the line again (at least `reverse_min_s`).
            let aligned = len(to_line) > 1.0 && dot(normalize(to_line), ctx.heading) > 0.7;
            if self.reverse_left_s <= 0.0 || (aligned && reversed_s >= p.reverse_min_s) {
                self.reverse_left_s = 0.0;
                self.ctrl.resync(0.0);
            } else {
                out.reversing = true;
                out.input = VehicleInput {
                    steer: -steer,
                    throttle: 0.0,
                    // A held brake at a standstill selects reverse; then it drives backwards.
                    brake: 0.7,
                    ..Default::default()
                };
                self.throttle = 0.0;
                return out;
            }
        }

        let (mut throttle, mut brake) = self.ctrl.pedals(fwd_speed, acc, dt, ctx.disabled);
        // Friction ellipse on the way out of a corner: power only with the grip the cornering
        // leaves (lateral load from the measured yaw rate), so the rear stays planted.
        let (a_lat_lim, _) = self.accel_limits(ctx);
        let used = (fwd_speed * ctx.yaw_rate).abs() / (a_lat_lim / self.confidence.max(0.1));
        let ellipse = (1.0 - used * used).max(0.0).sqrt().max(p.exit_throttle_min);
        throttle = throttle.min(ellipse);
        if self.sliding {
            throttle = throttle.min(p.slide_throttle);
            brake = brake.min(0.5);
        }
        if self.running_wide {
            throttle = 0.0;
        }
        // Power comes in progressively (no snap throttle out of a corner).
        throttle = throttle.min(self.throttle + 6.0 * dt);
        self.throttle = throttle;
        let trying = throttle > 0.4 && fwd_speed.abs() < 0.5 && v_ref > 2.0;
        // Only a car that wants to go and cannot is stuck (one held at a line, or turning
        // round at a crawl, is not: reversing blindly hits whatever is behind it).
        if trying && !ctx.disabled {
            self.stuck_s += dt;
        } else {
            self.stuck_s = 0.0;
        }
        if self.stuck_s > p.stuck_s {
            self.stuck_s = 0.0;
            self.reverse_left_s = p.reverse_s;
        }
        let _ = ctx.power_scale;
        out.input = VehicleInput {
            steer,
            throttle,
            brake,
            ..Default::default()
        };
        out
    }
}

/// Signed curvature of the circle through `a`, `b`, `c` (+ = turning left), 1/m.
fn signed_curvature(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    let ab = sub(b, a);
    let bc = sub(c, b);
    let ac = sub(c, a);
    let d = len(ab) * len(bc) * len(ac);
    if d < 1e-6 {
        0.0
    } else {
        2.0 * cross(ab, bc) / d
    }
}

/// `passes` rounds of `p_i ← (p_{i−1} + 2 p_i + p_{i+1}) / 4`, ends fixed.
fn round_corners(pts: &[[f32; 2]], passes: u32) -> Vec<[f32; 2]> {
    let mut a = pts.to_vec();
    let n = a.len();
    if n < 3 {
        return a;
    }
    let mut b = a.clone();
    for _ in 0..passes {
        for i in 1..n - 1 {
            b[i] = scale(add(add(a[i - 1], a[i + 1]), scale(a[i], 2.0)), 0.25);
        }
        std::mem::swap(&mut a, &mut b);
    }
    a
}

/// Minimum-curvature offsets inside `lim` (projected Gauss–Seidel, strides 4 → 1). The first
/// two and last two samples stay fixed.
fn optimise_line(
    ctr: &[[f32; 2]],
    nrm: &[[f32; 2]],
    lim: &[(f32, f32)],
    off: &mut [f32],
    p: &ProParams,
) {
    let n = off.len();
    let at = |off: &[f32], k: usize| add(ctr[k], scale(nrm[k], off[k]));
    for (level, st) in [4usize, 2, 1].into_iter().enumerate() {
        if n / st < 8 {
            continue;
        }
        let lattice: Vec<usize> = {
            let mut l: Vec<usize> = (0..n).step_by(st).collect();
            if *l.last().unwrap_or(&0) != n - 1 {
                l.push(n - 1);
            }
            l
        };
        let q = lattice.len();
        let smooth = if level == 0 { p.smooth_sweeps } else { 0 };
        for _ in 0..smooth {
            for j in 2..q - 2 {
                let k = lattice[j];
                let tgt = scale(add(at(off, lattice[j - 1]), at(off, lattice[j + 1])), 0.5);
                let d = dot(sub(tgt, ctr[k]), nrm[k]);
                off[k] = d.clamp(lim[k].0, lim[k].1);
            }
        }
        for _ in 0..p.curvature_sweeps {
            for j in 2..q - 2 {
                let k = lattice[j];
                let a = add(at(off, lattice[j - 1]), at(off, lattice[j + 1]));
                let b = add(at(off, lattice[j - 2]), at(off, lattice[j + 2]));
                let tgt = scale(sub(scale(a, 4.0), b), 1.0 / 6.0);
                let d = dot(sub(tgt, ctr[k]), nrm[k]);
                let next = off[k] + 1.2 * (d - off[k]);
                off[k] = next.clamp(lim[k].0, lim[k].1);
            }
        }
        // Interpolate the samples between lattice points.
        if st > 1 {
            for w in lattice.windows(2) {
                let (a, b) = (w[0], w[1]);
                for k in a + 1..b {
                    let t = (k - a) as f32 / (b - a) as f32;
                    let v = off[a] + (off[b] - off[a]) * t;
                    off[k] = v.clamp(lim[k].0, lim[k].1);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l_track() -> Track {
        Track::new(vec![[0.0, 0.0], [0.0, 200.0], [200.0, 200.0]])
    }

    #[test]
    fn line_cuts_the_inside_of_a_corner() {
        let mut d = ProDriver::new(
            ProParams::default(),
            ProParams::default().default_limits,
            SteerGeometry::default(),
            20.0,
        );
        let ctx = DriveCtx {
            pos: [0.0, 100.0],
            heading: [0.0, 1.0],
            vel: [0.0, 20.0],
            yaw_rate: 0.0,
            road_grip: 1.0,
            damage_grip: 1.0,
            power_scale: 1.0,
            target_mps: 40.0,
            cornering_cap_g: None,
            braking_cap_mps2: None,
            lane_offset_m: 0.0,
            pass_offset_m: None,
            racing_line: true,
            disabled: false,
        };
        let room = FixedRoom {
            left_m: 5.0,
            right_m: 5.0,
        };
        let out = d.drive(&ctx, &l_track(), &room, 1.0 / 240.0);
        // Right-hand corner at (0, 200): the line clips the inside corner of the allowed band
        // (5 m room - 1.45 m half width and margin) and turns in from the left.
        let apex = [3.55, 196.45];
        let closest = d
            .win
            .pts
            .iter()
            .map(|q| len(sub(*q, apex)))
            .fold(f32::INFINITY, f32::min);
        assert!(closest < 1.0, "apex miss {closest}");
        let turn_in = d.win.point_at(170.0);
        assert!(turn_in[0] < -1.0, "turn-in {turn_in:?}");
        // The profile brakes for the corner and the corner speed sits at the grip limit.
        let v_apex = d.win.sample(&d.win.v, 200.0);
        assert!(v_apex < 20.0 && v_apex > 8.0, "{v_apex}");
        assert!(out.planned_mps > v_apex);
    }

    #[test]
    fn wet_road_slows_the_corner() {
        let mk = |grip: f32| {
            let mut d = ProDriver::new(
                ProParams::default(),
                ProParams::default().default_limits,
                SteerGeometry::default(),
                20.0,
            );
            let ctx = DriveCtx {
                pos: [0.0, 100.0],
                heading: [0.0, 1.0],
                vel: [0.0, 20.0],
                yaw_rate: 0.0,
                road_grip: grip,
                damage_grip: 1.0,
                power_scale: 1.0,
                target_mps: 40.0,
                cornering_cap_g: None,
                braking_cap_mps2: None,
                lane_offset_m: 0.0,
                pass_offset_m: None,
                racing_line: true,
                disabled: false,
            };
            let room = FixedRoom {
                left_m: 5.0,
                right_m: 5.0,
            };
            d.drive(&ctx, &l_track(), &room, 1.0 / 240.0);
            d.win.sample(&d.win.v, 200.0)
        };
        let (dry, wet) = (mk(1.0), mk(0.7));
        assert!(wet < dry * 0.9, "dry {dry} wet {wet}");
    }

    #[test]
    fn countersteers_a_slide() {
        let mut d = ProDriver::new(
            ProParams::default(),
            ProParams::default().default_limits,
            SteerGeometry::default(),
            20.0,
        );
        let room = FixedRoom {
            left_m: 5.0,
            right_m: 5.0,
        };
        // Straight track; the nose points 15° left of the velocity (rear stepped out right).
        let t = Track::new(vec![[0.0, 0.0], [0.0, 400.0]]);
        let (s, c) = wp_math::sin_cos(0.26);
        let ctx = DriveCtx {
            pos: [0.0, 50.0],
            heading: [-s, c],
            vel: [0.0, 20.0],
            yaw_rate: 0.3,
            road_grip: 1.0,
            damage_grip: 1.0,
            power_scale: 1.0,
            target_mps: 40.0,
            cornering_cap_g: None,
            braking_cap_mps2: None,
            lane_offset_m: 0.0,
            pass_offset_m: None,
            racing_line: true,
            disabled: false,
        };
        let out = d.drive(&ctx, &t, &room, 1.0 / 240.0);
        assert!(out.sliding);
        assert!(
            out.input.steer > 0.5,
            "countersteer right: {}",
            out.input.steer
        );
        assert!(out.input.throttle <= ProParams::default().slide_throttle);
        assert!(d.confidence() < 1.0);
    }

    #[test]
    fn road_room_keeps_own_lanes() {
        let mut g = RoadGraph::new();
        let a = g.add_node([0.0, 0.0], crate::graph::District::Core);
        let b = g.add_node([0.0, 200.0], crate::graph::District::Core);
        let s = g.add_segment(a, b, 14.0, RoadClass::Arterial, false);
        g.segments[s.0 as usize].lanes_fwd = 2;
        g.segments[s.0 as usize].lanes_back = 2;
        g.segments[s.0 as usize].lane_width_m = 3.5;
        let room = RoadRoom::new(&g, &ProParams::default());
        // Northbound at the centre of the right lane pair (x = +3.5).
        let (l, r) = room.room([3.5, 100.0], [0.0, 1.0]);
        assert!((l - 4.0).abs() < 1e-4 && (r - 3.5).abs() < 1e-4, "{l} {r}");
        // Southbound: own lanes are at x < 0.
        let (l, r) = room.room([-3.5, 100.0], [0.0, -1.0]);
        assert!((l - 4.0).abs() < 1e-4 && (r - 3.5).abs() < 1e-4, "{l} {r}");
    }
}
