//! Static feelers for ground units (owner playtest row 66: "cops should route around
//! buildings, not drive into them").
//!
//! [`WallIndex`] holds the city's solid outlines as 2D segments on a coarse grid: every building
//! footprint edge and both walls of every covered road (tunnel). A physical unit casts three
//! rays from its front bumper (straight ahead and [`crate::config::AvoidConfig::wall_feeler_rad`]
//! either side) as far as it needs to stop plus a margin. A wall straight ahead caps its speed
//! so it stops short of it; a wall closer on one side than the other shifts its line away from
//! it. Parked cars and cones are vehicles-like obstacles already handled by [`crate::avoid`].

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::graph::{add, cross, normalize, scale, sub};

/// Grid cell size, m.
const CELL_M: f32 = 20.0;

/// Solid outlines of the city as segments, on a grid.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WallIndex {
    segs: Vec<[[f32; 2]; 2]>,
    grid: BTreeMap<(i32, i32), Vec<u32>>,
}

fn cell(p: [f32; 2]) -> (i32, i32) {
    (
        (p[0] / CELL_M).floor() as i32,
        (p[1] / CELL_M).floor() as i32,
    )
}

impl WallIndex {
    /// Index `segs` (each a wall from one point to another).
    pub fn new(segs: Vec<[[f32; 2]; 2]>) -> Self {
        let mut grid: BTreeMap<(i32, i32), Vec<u32>> = BTreeMap::new();
        for (k, s) in segs.iter().enumerate() {
            let (a, b) = (cell(s[0]), cell(s[1]));
            for x in a.0.min(b.0)..=a.0.max(b.0) {
                for y in a.1.min(b.1)..=a.1.max(b.1) {
                    grid.entry((x, y)).or_default().push(k as u32);
                }
            }
        }
        Self { segs, grid }
    }

    /// Walls from closed outlines (building footprints): every edge, closing edge included.
    pub fn from_outlines<'a>(outlines: impl Iterator<Item = &'a [[f32; 2]]>) -> Vec<[[f32; 2]; 2]> {
        let mut v = Vec::new();
        for o in outlines {
            if o.len() < 2 {
                continue;
            }
            for k in 0..o.len() {
                v.push([o[k], o[(k + 1) % o.len()]]);
            }
        }
        v
    }

    /// Walls along an open polyline offset `half_width_m` to both sides (a tunnel's walls).
    pub fn from_corridor(centre: &[[f32; 2]], half_width_m: f32) -> Vec<[[f32; 2]; 2]> {
        let mut v = Vec::new();
        for w in centre.windows(2) {
            let d = normalize(sub(w[1], w[0]));
            let n = [-d[1], d[0]];
            for side in [1.0, -1.0] {
                let o = scale(n, side * half_width_m);
                v.push([add(w[0], o), add(w[1], o)]);
            }
        }
        v
    }

    /// Number of wall segments.
    pub fn len(&self) -> usize {
        self.segs.len()
    }

    /// No walls.
    pub fn is_empty(&self) -> bool {
        self.segs.is_empty()
    }

    /// Distance along the ray from `from` in unit direction `dir` to the first wall within
    /// `max_m`, m.
    pub fn ray(&self, from: [f32; 2], dir: [f32; 2], max_m: f32) -> Option<f32> {
        if self.segs.is_empty() || max_m <= 0.0 {
            return None;
        }
        let to = add(from, scale(dir, max_m));
        let (a, b) = (cell(from), cell(to));
        let mut cands: Vec<u32> = Vec::new();
        for x in a.0.min(b.0)..=a.0.max(b.0) {
            for y in a.1.min(b.1)..=a.1.max(b.1) {
                if let Some(v) = self.grid.get(&(x, y)) {
                    cands.extend_from_slice(v);
                }
            }
        }
        cands.sort_unstable();
        cands.dedup();
        let r = sub(to, from);
        let mut best: Option<f32> = None;
        for k in cands {
            let [p, q] = self.segs[k as usize];
            let s = sub(q, p);
            let den = cross(r, s);
            if den.abs() < 1e-9 {
                continue;
            }
            let w = sub(p, from);
            let t = cross(w, s) / den;
            let u = cross(w, r) / den;
            if (0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u) {
                let d = t * max_m;
                if best.is_none_or(|b| d < b) {
                    best = Some(d);
                }
            }
        }
        best
    }
}

/// What the feelers say for one unit: a speed cap and a lateral shift (+ right), m/s and m.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WallFeel {
    /// Speed that still stops short of the wall ahead, m/s (`f32::INFINITY` = clear).
    pub limit_mps: f32,
    /// Steer this far right (+) or left (-) of the planned line, m.
    pub shift_m: f32,
}

impl Default for WallFeel {
    fn default() -> Self {
        Self {
            limit_mps: f32::INFINITY,
            shift_m: 0.0,
        }
    }
}

/// Distance from the car's centre to its front bumper, m.
const FRONT_M: f32 = 2.3;
/// A side feeler hit this close sideways to the bumper's line is a wall about to be scraped, m.
const SCRAPE_LATERAL_M: f32 = 2.5;
/// Stop this far short of a wall, m.
const STOP_SHORT_M: f32 = 2.5;

/// Cast the feelers for a car at `pos` facing `heading` at `v` m/s.
pub fn feel(
    walls: &WallIndex,
    pos: [f32; 2],
    heading: [f32; 2],
    v: f32,
    cfg: &crate::config::AvoidConfig,
) -> WallFeel {
    if walls.is_empty() || v < 2.0 {
        return WallFeel::default();
    }
    let reach = v * v / (2.0 * cfg.wall_decel_mps2) + 0.3 * v + cfg.wall_margin_m;
    let front = add(pos, scale(heading, FRONT_M));
    let (sn, cs) = wp_math::sin_cos(cfg.wall_feeler_rad);
    let rot = |a: f32| {
        // Rotate the heading by ±feeler angle (a = +1 left, -1 right).
        [
            heading[0] * cs - a * heading[1] * sn,
            a * heading[0] * sn + heading[1] * cs,
        ]
    };
    let mut out = WallFeel::default();
    if let Some(d) = walls.ray(front, heading, reach) {
        let room = (d - STOP_SHORT_M).max(0.0);
        out.limit_mps = (2.0 * cfg.wall_decel_mps2 * room).sqrt();
    }
    let side_reach = reach * 0.7;
    let dl = walls.ray(front, rot(1.0), side_reach).unwrap_or(f32::INFINITY);
    let dr = walls.ray(front, rot(-1.0), side_reach).unwrap_or(f32::INFINITY);
    // A wall closing in on one side (within SCRAPE_LATERAL_M of the bumper's line, ahead):
    // away from it, harder the nearer it is. Symmetric streets and wide roads: nothing.
    let near = dl.min(dr);
    let lateral = near * sn;
    if near.is_finite() && lateral < SCRAPE_LATERAL_M && (dl - dr).abs() > 1.0 {
        let k = (1.0 - lateral / SCRAPE_LATERAL_M).clamp(0.0, 1.0);
        out.shift_m = if dl < dr { 1.0 } else { -1.0 } * cfg.wall_shift_m * (0.5 + 0.5 * k);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::PoliceConfig;

    fn square_building() -> WallIndex {
        // A 10 m square building centred at (0, 30).
        let o = vec![[-5.0, 25.0], [5.0, 25.0], [5.0, 35.0], [-5.0, 35.0]];
        WallIndex::new(WallIndex::from_outlines(std::iter::once(o.as_slice())))
    }

    #[test]
    fn ray_hits_the_nearest_wall() {
        let w = square_building();
        let d = w.ray([0.0, 0.0], [0.0, 1.0], 100.0).unwrap();
        assert!((d - 25.0).abs() < 1e-3);
        assert!(w.ray([0.0, 0.0], [0.0, 1.0], 20.0).is_none());
        assert!(w.ray([20.0, 0.0], [0.0, 1.0], 100.0).is_none());
    }

    #[test]
    fn feelers_brake_for_a_wall_ahead_and_steer_off_one_at_the_side() {
        let cfg = PoliceConfig::builtin().pursuit.avoid;
        let w = square_building();
        // Heading straight at it at 20 m/s from 17.7 m (bumper at 20 m): must slow.
        let f = feel(&w, [0.0, 2.7], [0.0, 1.0], 20.0, &cfg);
        assert!(f.limit_mps < 20.0, "{f:?}");
        // Passing it close on the right: shift left (negative).
        let f = feel(&w, [-7.0, 12.0], [0.0, 1.0], 15.0, &cfg);
        assert!(f.shift_m < 0.0, "{f:?}");
        // Open road: nothing.
        let f = feel(&w, [40.0, 0.0], [0.0, 1.0], 15.0, &cfg);
        assert_eq!(f, WallFeel::default());
        // A tunnel corridor along +y: driving down the middle sees nothing ahead.
        let t = WallIndex::new(WallIndex::from_corridor(&[[100.0, 0.0], [100.0, 200.0]], 5.0));
        let f = feel(&t, [100.0, 20.0], [0.0, 1.0], 20.0, &cfg);
        assert!(f.limit_mps.is_infinite(), "{f:?}");
    }
}
