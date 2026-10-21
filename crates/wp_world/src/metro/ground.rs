//! Natural ground of Port Vesper v3 and the earthworks applied before roads are profiled.
//!
//! Pipeline: [`natural_z`] (hills, river valley, bay, noise) is sampled into a
//! [`Heightfield`], every module's [`Earthwork`]s are applied in order (corridors first,
//! then pads), and the result is the [`Ground`] the road generator profiles against and
//! later carves.
//!
//! Water is wherever the ground is below [`plan::WATER_Z`]: the greybox draws one sea plane,
//! so the river, the canal and the bay need no separate geometry.

use wp_math::Rng;

use super::plan::{self, Plan, MAP_HI, MAP_LO};
use crate::geom::{self, V2, V3};
use crate::terrain::{bump, smoothstep, Heightfield, ValueNoise};

/// A ground modification requested by a module before roads exist.
#[derive(Debug, Clone)]
pub enum Earthwork {
    /// Level a polygon to `z`, blending back to the ground over `blend_m` outside it.
    Pad {
        /// Polygon (counter-clockwise), m.
        polygon: Vec<V2>,
        /// Level, m.
        z: f32,
        /// Blend distance outside the polygon, m.
        blend_m: f32,
    },
    /// Grade a corridor to a 3D centreline (rail beds, runways): flat across `half_width_m`,
    /// blending over `blend_m` beyond.
    Corridor {
        /// Centreline with heights, m.
        line: Vec<V3>,
        /// Flat half width, m.
        half_width_m: f32,
        /// Blend distance beyond the flat part, m.
        blend_m: f32,
    },
}

/// Seeded noise for the natural ground.
pub struct Natural {
    n1: ValueNoise,
    n2: ValueNoise,
    channels: Vec<plan::Channel>,
    sea: Vec<Vec<V2>>,
}

impl Natural {
    /// Noise from `rng` (consumed in a fixed order).
    pub fn new(plan: &Plan, rng: &mut Rng) -> Self {
        Self {
            n1: ValueNoise::new(rng, MAP_LO, MAP_HI, 480.0),
            n2: ValueNoise::new(rng, MAP_LO, MAP_HI, 160.0),
            channels: plan.channels.clone(),
            sea: plan.sea.clone(),
        }
    }

    /// Natural ground height at `p`, m (before earthworks and road carving).
    pub fn z(&self, p: V2) -> f32 {
        // Gentle rise to the north: river flats ~2 m, Uptown ~26 m, Northfield ~40 m.
        let rise =
            2.5 + 22.0 * smoothstep(-500.0, 1800.0, p[1]) + 16.0 * smoothstep(1800.0, 3300.0, p[1]);
        // Hollins Heights: the Knob (140 m) and a western shoulder.
        let hollins =
            112.0 * bump(p, [-2300.0, 2580.0], 820.0) + 38.0 * bump(p, [-2900.0, 2050.0], 650.0);
        // Westridge: a long wooded ridge the Westridge Tunnel goes through.
        let westridge =
            70.0 * bump(p, [-3550.0, 1350.0], 520.0) + 45.0 * bump(p, [-3800.0, 2300.0], 700.0);
        // Bayview Bluffs plateau, ending in a cliff at the shore.
        let bluff = 26.0 * smoothstep(780.0, 520.0, geom::dist(p, [1700.0, 2450.0]));
        // Noise: none on the port and river flats, damped downtown and on the airport plain.
        let south_flat = smoothstep(-700.0, -420.0, p[1]);
        let core = 0.35 + 0.65 * smoothstep(300.0, 800.0, geom::dist(p, plan::CORE));
        let noise = (7.0 * self.n1.at(p) + 2.5 * self.n2.at(p)) * south_flat * core;
        // South bank: flat 2-8 m with a little relief.
        let south = (1.0 - south_flat) * (3.0 + 2.0 * self.n2.at(p).abs());
        let land = (rise * south_flat + south + hollins + westridge + bluff + noise)
            .max(plan::MIN_LAND_Z + 0.5);
        let mut z = land;
        // Rivers and canals.
        for c in &self.channels {
            let d = polyline_dist(&c.line, p);
            let bank = if c.walled { 2.0 } else { 28.0 };
            let w = smoothstep(c.half_width_m + bank, c.half_width_m - 4.0, d);
            if w > 0.0 {
                // Banks sit at most 2.5 m above the water near the channel.
                let near = smoothstep(c.half_width_m + 120.0, c.half_width_m + bank, d);
                let land_here = z + (z.min(plan::MIN_LAND_Z + 1.0) - z) * near;
                let bed =
                    c.bed_z + (plan::WATER_Z - 1.5 - c.bed_z) * smoothstep(0.0, c.half_width_m, d);
                z = land_here + (bed - land_here) * w;
            } else {
                let near = smoothstep(c.half_width_m + 120.0, c.half_width_m + bank, d);
                z += (z.min(plan::MIN_LAND_Z + 1.0) - z) * near;
            }
        }
        // Sea: a beach / cliff band 60 m wide, then the sea bed at -8 m.
        let d_sea = self
            .sea
            .iter()
            .map(|poly| signed_dist(poly, p))
            .fold(f32::MAX, f32::min);
        if d_sea < 60.0 {
            let w = smoothstep(60.0, -40.0, d_sea);
            z += (-8.0 - z) * w;
        }
        z
    }
}

/// Distance from `p` to a polyline.
pub fn polyline_dist(line: &[V2], p: V2) -> f32 {
    line.windows(2)
        .map(|w| seg_dist(w[0], w[1], p))
        .fold(f32::MAX, f32::min)
}

/// Distance from `p` to segment `a`–`b`.
pub fn seg_dist(a: V2, b: V2, p: V2) -> f32 {
    let ab = geom::sub(b, a);
    let l2 = geom::dot(ab, ab);
    let t = if l2 > 0.0 {
        (geom::dot(geom::sub(p, a), ab) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    geom::dist(p, [a[0] + ab[0] * t, a[1] + ab[1] * t])
}

/// Point-in-polygon (even-odd).
pub fn inside(poly: &[V2], p: V2) -> bool {
    let mut c = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            c = !c;
        }
        j = i;
    }
    c
}

/// Signed distance to a polygon boundary (negative inside), m.
pub fn signed_dist(poly: &[V2], p: V2) -> f32 {
    let n = poly.len();
    let d = (0..n)
        .map(|i| seg_dist(poly[i], poly[(i + 1) % n], p))
        .fold(f32::MAX, f32::min);
    if inside(poly, p) {
        -d
    } else {
        d
    }
}

/// The ground the road generator profiles against: natural ground plus earthworks, sampled.
pub struct Ground {
    /// Sampled heights (natural + earthworks; the road generator carves this in place).
    pub hf: Heightfield,
}

impl Ground {
    /// Sample the natural ground at `cell_m` and apply `earthworks` in order (corridors
    /// first, then pads; each kind in the given order).
    pub fn new(nat: &Natural, cell_m: f32, earthworks: &[Earthwork]) -> Self {
        let mut hf = Heightfield::flat(MAP_LO, MAP_HI, cell_m, 0.0);
        for j in 0..hf.ny {
            for i in 0..hf.nx {
                let p = hf.vertex_pos(i, j);
                hf.z[(j * hf.nx + i) as usize] = nat.z(p);
            }
        }
        let mut g = Self { hf };
        for e in earthworks
            .iter()
            .filter(|e| matches!(e, Earthwork::Corridor { .. }))
        {
            g.apply(e);
        }
        for e in earthworks
            .iter()
            .filter(|e| matches!(e, Earthwork::Pad { .. }))
        {
            g.apply(e);
        }
        g
    }

    /// Ground height at `p` (bilinear), m.
    pub fn z(&self, p: V2) -> f32 {
        self.hf.height_at(p)
    }

    /// Water at `p` (ground below the water line).
    pub fn is_water(&self, p: V2) -> bool {
        self.z(p) < plan::WATER_Z
    }

    fn apply(&mut self, e: &Earthwork) {
        let (lo, hi, reach) = match e {
            Earthwork::Pad {
                polygon, blend_m, ..
            } => {
                let (lo, hi) = bounds(polygon);
                (lo, hi, *blend_m)
            }
            Earthwork::Corridor {
                line,
                half_width_m,
                blend_m,
            } => {
                let xy: Vec<V2> = line.iter().map(|p| geom::xy(*p)).collect();
                let (lo, hi) = bounds(&xy);
                (lo, hi, half_width_m + blend_m)
            }
        };
        let hf = &mut self.hf;
        let cell = hf.cell_m;
        let i0 = (((lo[0] - reach - hf.origin[0]) / cell).floor().max(0.0)) as u32;
        let j0 = (((lo[1] - reach - hf.origin[1]) / cell).floor().max(0.0)) as u32;
        let i1 = ((((hi[0] + reach - hf.origin[0]) / cell).ceil()) as u32).min(hf.nx - 1);
        let j1 = ((((hi[1] + reach - hf.origin[1]) / cell).ceil()) as u32).min(hf.ny - 1);
        for j in j0..=j1 {
            for i in i0..=i1 {
                let p = hf.vertex_pos(i, j);
                let k = (j * hf.nx + i) as usize;
                let z0 = hf.z[k];
                let (target, w) = match e {
                    Earthwork::Pad {
                        polygon,
                        z,
                        blend_m,
                    } => {
                        let d = signed_dist(polygon, p);
                        (*z, smoothstep(*blend_m, 0.0, d))
                    }
                    Earthwork::Corridor {
                        line,
                        half_width_m,
                        blend_m,
                    } => {
                        let (d, z) = corridor_at(line, p);
                        (z, smoothstep(half_width_m + blend_m, *half_width_m, d))
                    }
                };
                if w > 0.0 {
                    hf.z[k] = z0 + (target - z0) * w;
                }
            }
        }
    }
}

/// Distance to a 3D centreline and its height at the closest point.
fn corridor_at(line: &[V3], p: V2) -> (f32, f32) {
    let mut best = (f32::MAX, 0.0);
    for w in line.windows(2) {
        let (a, b) = (geom::xy(w[0]), geom::xy(w[1]));
        let ab = geom::sub(b, a);
        let l2 = geom::dot(ab, ab);
        let t = if l2 > 0.0 {
            (geom::dot(geom::sub(p, a), ab) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let q = [a[0] + ab[0] * t, a[1] + ab[1] * t];
        let d = geom::dist(p, q);
        if d < best.0 {
            best = (d, w[0][2] + (w[1][2] - w[0][2]) * t);
        }
    }
    best
}

/// Axis-aligned bounds of a point list.
pub fn bounds(pts: &[V2]) -> (V2, V2) {
    let mut lo = [f32::MAX; 2];
    let mut hi = [f32::MIN; 2];
    for p in pts {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    (lo, hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn river_bay_and_hills_are_where_the_plan_says() {
        let plan = Plan::new();
        let nat = Natural::new(&plan, &mut Rng::new(1));
        assert!(nat.z([0.0, -460.0]) < plan::WATER_Z, "river");
        assert!(nat.z([-300.0, -700.0]) < plan::WATER_Z, "canal");
        assert!(nat.z([3000.0, 1000.0]) < -4.0, "bay");
        assert!(nat.z([0.0, -3500.0]) < -4.0, "south sound");
        assert!(nat.z([-2300.0, 2580.0]) > 120.0, "Hollins Knob");
        assert!(nat.z([0.0, 200.0]) > plan::MIN_LAND_Z, "downtown is land");
        assert!(
            nat.z([-3900.0, -2000.0]) > plan::MIN_LAND_Z,
            "airport is land"
        );
    }
}
