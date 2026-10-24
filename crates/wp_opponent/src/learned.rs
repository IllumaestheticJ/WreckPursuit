//! Learned corrections from offline training runs (`wp_race/tests/opponent_train.rs`).
//!
//! The speed profile ([`crate::line::SpeedProfile`]) is built from the car's limits measured
//! on test rigs (skidpad, braking, 0–100). Real roads differ from the rigs: camber, bumps,
//! crests, long sweepers where aero and tyre heat matter, and the steering controller settles
//! a little wide of its target in long bends. Training drives every car flat out round each
//! trained course with no traffic and no police, again and again, and learns two numbers per
//! stretch of road (every [`POINT_STEP_M`]):
//!
//! - `v_scale`: how much faster (> 1) or slower (< 1) than the measured-grip corner speed
//!   this car can really take this stretch. Applied to the corner-speed limit before the
//!   braking / acceleration passes, so braking points move with it.
//! - `aim_m`: a lateral aim correction, m (corridor normal, + = left), that cancels the drift
//!   the car showed off its commanded line here. It was learned at `v_ref` and is applied in
//!   proportion to `(v / v_ref)²` (the drift comes from lateral acceleration).
//!
//! Points are keyed by world position and driving direction, not by race: any course over
//! the same road in the same direction (Loop 9 night and day, a street racer's rest-stop lap)
//! gets the same corrections. A row only applies to a car whose measured capability still
//! matches the one it was trained with (a physics change makes the table stale, never wrong:
//! the car drives untrained). Generated, never hand-edited:
//!
//! ```sh
//! WP_TRAIN_WRITE=1 cargo test --release -p wp_race --test opponent_train stage1 -- --ignored --nocapture
//! ```
//!
//! Deterministic: the table is static data; lookups iterate in a fixed order.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::capability::CarCapability;
use crate::corridor::Corridor;

/// Spacing of stored points along a course, m (the trainer learns per 10 m and stores every
/// 20 m; the corrections vary over hundreds of metres).
pub const POINT_STEP_M: f32 = 20.0;
/// A corridor sample takes a stored point within this distance along its driving direction
/// (half the spacing, so every sample on trained road finds one), m ...
pub const MATCH_ALONG_M: f32 = 0.5 * POINT_STEP_M + 0.5;
/// ... and within this distance across it (the next carriageway is further), m.
pub const MATCH_RADIUS_M: f32 = 6.0;
/// ... and within this height difference (stacked roads: ramps over the loop), m.
pub const MATCH_HEIGHT_M: f32 = 3.0;
/// ... and facing within ~25° of the point's driving direction (cosine).
pub const MATCH_DIR_COS: f32 = 0.9;
/// Bounds of `v_scale`.
pub const V_SCALE_RANGE: (f32, f32) = (0.85, 1.12);
/// Bound of `|aim_m|`, m.
pub const AIM_MAX_M: f32 = 1.5;

/// The embedded table (`data/ai/racer_training.json`).
pub const EMBEDDED_JSON: &str = include_str!("../../../data/ai/racer_training.json");

/// One trained point laid out for matching: `[x, y, z, dir_x, dir_y, v_scale, aim_m, v_ref]`.
pub type LearnedPoint = [f32; 8];

/// A trained course's geometry, shared by every car trained on it: one point every
/// [`POINT_STEP_M`] along the race corridor, `[x, y, z, dir_x, dir_y]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CourseGeom {
    /// Race id it was laid from (informational: lookups are by position).
    pub course: String,
    /// Points.
    pub points: Vec<[f32; 5]>,
}

/// One car's corrections on one course, one entry per [`CourseGeom`] point, each list a hex
/// string of bytes (two hex digits per point; kept compact because the table is embedded in
/// the browser build).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CarCourse {
    /// Course (a [`CourseGeom::course`]).
    pub course: String,
    /// Aim correction per point, decimetres, as `i8` bytes ([`hex_i8`]).
    pub aim_dm: String,
    /// Speed the aim was learned at per point, m/s, as `u8` bytes ([`hex_u8`]).
    pub v_ref: String,
    /// Corner-speed scale − 1 per point, in 0.5 % steps, as `i8` bytes (empty: all 1).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub v_scale: String,
}

/// Bytes as a hex string, two digits each.
pub fn to_hex(bytes: impl IntoIterator<Item = u8>) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::new();
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 15) as usize] as char);
    }
    out
}

/// Bytes back from [`to_hex`] (an odd trailing digit or a non-hex digit reads as 0).
pub fn hex_u8(s: &str) -> Vec<u8> {
    let d = |c: u8| (c as char).to_digit(16).unwrap_or(0) as u8;
    s.as_bytes()
        .chunks(2)
        .map(|p| (d(p[0]) << 4) | p.get(1).map_or(0, |c| d(*c)))
        .collect()
}

/// [`hex_u8`] read as `i8`.
pub fn hex_i8(s: &str) -> Vec<i8> {
    hex_u8(s).into_iter().map(|b| b as i8).collect()
}

/// One car's training.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LearnedCar {
    /// Car id (`data/cars`).
    pub car: String,
    /// Capability it was trained with: `[max_lat_g, brake_g, accel_mps2, top_speed_mps]`.
    pub capability: [f32; 4],
    /// Steering look-ahead multiplier found for this car (a longer look-ahead damps a car
    /// whose yaw answers the wheel late and weaves at highway speed). 1 = untrained.
    #[serde(default = "one")]
    pub lookahead_scale: f32,
    /// Courses.
    pub courses: Vec<CarCourse>,
}

fn one() -> f32 {
    1.0
}

/// Traffic and police behaviour learned in stage 2 (with traffic and police), applied to
/// every racer's [`crate::DriverParams`] (see [`crate::profile`]).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LearnedTraffic {
    /// Multiplies the civilian clearance (`civilian_margin_m`).
    pub civilian_margin_scale: f32,
    /// Multiplies the civilian time gap (`civilian_headway_s`).
    pub civilian_headway_scale: f32,
    /// Lowest `aggression + risk` (0..2) that rams police in the way (see `field`).
    pub ram_min_nerve: f32,
    /// Most closing speed on a police car being rammed, m/s.
    pub ram_closing_mps: f32,
    /// Lateral clearance planned round a spike strip's end, m.
    pub spike_clearance_m: f32,
}

impl LearnedTraffic {
    /// `params` with the learned civilian clearance and time gap.
    pub fn apply(&self, mut params: crate::DriverParams) -> crate::DriverParams {
        params.civilian_margin_m *= self.civilian_margin_scale;
        params.civilian_headway_s *= self.civilian_headway_scale;
        params
    }
}

impl Default for LearnedTraffic {
    /// The untrained behaviour (scales 1, the hand-set ram and spike numbers).
    fn default() -> Self {
        Self {
            civilian_margin_scale: 1.0,
            civilian_headway_scale: 1.0,
            ram_min_nerve: 1.0,
            ram_closing_mps: 6.0,
            spike_clearance_m: 0.6,
        }
    }
}

/// Everything training learned.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct LearnedTable {
    /// Stage 1: the trained courses' geometry.
    #[serde(default)]
    pub courses: Vec<CourseGeom>,
    /// Stage 1 (no traffic, no police): per-car pace and line corrections.
    #[serde(default)]
    pub cars: Vec<LearnedCar>,
    /// Stage 2 (traffic and police).
    #[serde(default)]
    pub traffic: LearnedTraffic,
}

/// Learned corrections laid onto one corridor, per corridor sample.
#[derive(Debug, Clone, PartialEq)]
pub struct LearnedPath {
    /// Corner-speed scale per sample (1 = untrained).
    pub v_scale: Vec<f32>,
    /// Lateral aim correction per sample, m.
    pub aim_m: Vec<f32>,
    /// Speed the aim correction was learned at per sample, m/s (0 = none).
    pub v_ref: Vec<f32>,
}

impl LearnedPath {
    /// A path with no corrections over `m` samples.
    pub fn neutral(m: usize) -> Self {
        Self {
            v_scale: vec![1.0; m],
            aim_m: vec![0.0; m],
            v_ref: vec![0.0; m],
        }
    }

    /// Whether any sample carries a correction.
    pub fn is_neutral(&self) -> bool {
        self.v_scale.iter().all(|v| *v == 1.0) && self.aim_m.iter().all(|a| *a == 0.0)
    }

    /// The aim correction at sample `i` for a car at `v`, m: the learned one scaled by
    /// `(v / v_ref)²`, at most 1.2×.
    pub fn aim_at(&self, i: usize, v: f32) -> f32 {
        let (a, vr) = (self.aim_m[i], self.v_ref[i]);
        if a == 0.0 || vr < 5.0 {
            return 0.0;
        }
        let k = (v / vr).max(0.0);
        a * (k * k).min(1.2)
    }
}

impl LearnedTable {
    /// The embedded table, parsed once.
    pub fn embedded() -> &'static LearnedTable {
        static TABLE: OnceLock<LearnedTable> = OnceLock::new();
        TABLE.get_or_init(|| serde_json::from_str(EMBEDDED_JSON).unwrap_or_default())
    }

    /// The trained car whose capability matches `cap` (within 0.5 % on all four limits).
    pub fn car_for(&self, cap: &CarCapability) -> Option<&LearnedCar> {
        let close = |a: f32, b: f32| (a - b).abs() <= 5e-3 * b.abs().max(1e-3);
        self.cars.iter().find(|c| {
            close(cap.max_lat_g, c.capability[0])
                && close(cap.brake_g, c.capability[1])
                && close(cap.accel_mps2, c.capability[2])
                && close(cap.top_speed_mps, c.capability[3])
        })
    }

    /// Corrections for a car with `cap` on `corridor`, or `None` when the car is untrained or
    /// no sample lies on trained road.
    pub fn path_for(&self, corridor: &Corridor, cap: &CarCapability) -> Option<LearnedPath> {
        let car = self.car_for(cap)?;
        let path = lay_onto(corridor, self.points_of(car).iter());
        (!path.is_neutral()).then_some(path)
    }

    /// Every trained point of `car`, laid out for matching.
    pub fn points_of(&self, car: &LearnedCar) -> Vec<LearnedPoint> {
        let mut out = Vec::new();
        for c in &car.courses {
            let Some(g) = self.courses.iter().find(|g| g.course == c.course) else {
                continue;
            };
            let (aims, v_refs, scales) = (hex_i8(&c.aim_dm), hex_u8(&c.v_ref), hex_i8(&c.v_scale));
            for (i, p) in g.points.iter().enumerate() {
                let aim = aims.get(i).copied().unwrap_or(0) as f32 * 0.1;
                let scale = 1.0 + scales.get(i).copied().unwrap_or(0) as f32 * 0.005;
                let v_ref = v_refs.get(i).copied().unwrap_or(0) as f32;
                if aim != 0.0 || scale != 1.0 {
                    out.push([p[0], p[1], p[2], p[3], p[4], scale, aim, v_ref]);
                }
            }
        }
        out
    }

    /// The steering look-ahead multiplier for a car with `cap` (1 when untrained).
    pub fn lookahead_scale(&self, cap: &CarCapability) -> f32 {
        self.car_for(cap).map_or(1.0, |c| c.lookahead_scale)
    }
}

/// Lay trained points onto a corridor: each sample takes the nearest point within
/// [`MATCH_RADIUS_M`] / [`MATCH_HEIGHT_M`] facing its way ([`MATCH_DIR_COS`]).
pub fn lay_onto<'a>(
    corridor: &Corridor,
    points: impl Iterator<Item = &'a LearnedPoint>,
) -> LearnedPath {
    const CELL_M: f32 = 20.0;
    let cell = |x: f32, y: f32| ((x / CELL_M).floor() as i32, (y / CELL_M).floor() as i32);
    let mut grid: BTreeMap<(i32, i32), Vec<&LearnedPoint>> = BTreeMap::new();
    for p in points {
        grid.entry(cell(p[0], p[1])).or_default().push(p);
    }
    let m = corridor.len();
    let mut out = LearnedPath::neutral(m);
    if grid.is_empty() {
        return out;
    }
    for (i, s) in corridor.samples.iter().enumerate() {
        let (cx, cy) = cell(s.pos[0], s.pos[1]);
        let mut best: Option<(f32, &LearnedPoint)> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                let Some(list) = grid.get(&(cx + dx, cy + dy)) else {
                    continue;
                };
                for p in list {
                    let (ex, ey) = (p[0] - s.pos[0], p[1] - s.pos[1]);
                    let d2 = ex * ex + ey * ey;
                    let along = ex * p[3] + ey * p[4];
                    let across = ex * p[4] - ey * p[3];
                    if along.abs() > MATCH_ALONG_M
                        || across.abs() > MATCH_RADIUS_M
                        || (p[2] - s.pos[2]).abs() > MATCH_HEIGHT_M
                        || p[3] * s.tangent[0] + p[4] * s.tangent[1] < MATCH_DIR_COS
                    {
                        continue;
                    }
                    if best.is_none_or(|(b, _)| d2 < b) {
                        best = Some((d2, p));
                    }
                }
            }
        }
        if let Some((_, p)) = best {
            out.v_scale[i] = p[5].clamp(V_SCALE_RANGE.0, V_SCALE_RANGE.1);
            out.aim_m[i] = p[6].clamp(-AIM_MAX_M, AIM_MAX_M);
            out.v_ref[i] = p[7];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::corridor::{CorridorSample, OpposingLanes, CORRIDOR_STEP_M};
    use wp_contracts::SurfaceType;

    fn straight(dir: f32) -> Corridor {
        let samples: Vec<CorridorSample> = (0..50)
            .map(|i| CorridorSample {
                pos: [0.0, dir * CORRIDOR_STEP_M * i as f32, 0.0],
                tangent: [0.0, dir],
                left_m: 4.0,
                left_pass_m: 4.0,
                right_m: 4.0,
                junction: false,
                surface: SurfaceType::Asphalt,
            })
            .collect();
        Corridor {
            length_m: 49.0 * CORRIDOR_STEP_M,
            samples,
            closed: false,
            opposing: OpposingLanes::Never,
            junction_lanes: Vec::new(),
        }
    }

    #[test]
    fn points_apply_only_in_their_driving_direction() {
        let pts: Vec<LearnedPoint> = vec![[0.0, 40.0, 0.0, 0.0, 1.0, 1.05, 0.4, 50.0]];
        let fwd = lay_onto(&straight(1.0), pts.iter());
        assert_eq!(fwd.v_scale[20], 1.05);
        assert_eq!(fwd.aim_m[20], 0.4);
        // Half the point spacing along the road: samples 15..=25 (y 30..50) take it,
        // sample 13 does not.
        assert_eq!(fwd.v_scale[15], 1.05);
        assert_eq!(fwd.v_scale[25], 1.05);
        assert_eq!(fwd.v_scale[13], 1.0);
        let back = lay_onto(&straight(-1.0), pts.iter());
        assert!(back.is_neutral());
    }

    #[test]
    fn hex_round_trips() {
        let v: Vec<i8> = vec![-15, -1, 0, 1, 15, 127, -128];
        assert_eq!(hex_i8(&to_hex(v.iter().map(|x| *x as u8))), v);
        assert_eq!(to_hex([0u8, 255, 16]), "00ff10");
    }

    #[test]
    fn aim_scales_with_speed_squared() {
        let mut p = LearnedPath::neutral(1);
        p.aim_m[0] = 1.0;
        p.v_ref[0] = 40.0;
        assert!((p.aim_at(0, 20.0) - 0.25).abs() < 1e-6);
        assert!((p.aim_at(0, 80.0) - 1.2).abs() < 1e-6);
    }

    #[test]
    fn embedded_table_parses() {
        let t: LearnedTable = serde_json::from_str(EMBEDDED_JSON).expect("racer_training.json");
        for c in &t.cars {
            assert!((0.5..=3.0).contains(&c.lookahead_scale), "{}", c.car);
            for k in &c.courses {
                let g = t.courses.iter().find(|g| g.course == k.course);
                let g = g.unwrap_or_else(|| panic!("{}: no geometry for {}", c.car, k.course));
                let n = 2 * g.points.len();
                assert_eq!(k.aim_dm.len(), n, "{} {}", c.car, k.course);
                assert_eq!(k.v_ref.len(), n, "{} {}", c.car, k.course);
                assert!(k.v_scale.is_empty() || k.v_scale.len() == n);
            }
        }
    }
}
