//! Ashpeak County's natural (uncarved) ground. Inside Port Vesper's extent it is Port
//! Vesper's own ground; it blends into Ashpeak's over [`plan::BLEND_M`] outside it.
//!
//! - **Ashpeak Range**: a ridge band between [`plan::RANGE_SOUTH_Y`] and
//!   [`plan::RANGE_NORTH_Y`] whose crest height varies along x (control points in
//!   [`CREST`]): low saddles at Grizzly (Route 12's pass) and Crowfoot (the touge), the
//!   Ashpeak summit, Talon Ridge (Route 7's tunnel).
//! - **Silver River gorge** along the range's northern foot, east to the sea.
//! - **Harlan Flats**: low rolling farmland west of Port Vesper, cut by the Sable River
//!   valley running south to the sea.
//! - **Ashpeak City basin**: gently rising ground north of the range, sea to the east.

use wp_math::Rng;

use super::plan::{self, LO};
use crate::geom::V2;
use crate::terrain::{bump, smoothstep, ValueNoise};

/// Crest height of the range along x: `(x, height above the plain)`, linearly interpolated.
const CREST: &[(f32, f32)] = &[
    (-4600.0, 330.0),
    (-3550.0, 360.0),
    (-3000.0, 255.0), // Grizzly saddle
    (-2500.0, 400.0),
    (-1900.0, 520.0), // Ashpeak
    (-1100.0, 390.0),
    (-250.0, 215.0), // Crowfoot saddle
    (350.0, 300.0),
    (850.0, 330.0), // Talon Ridge
    (1350.0, 240.0),
    (1700.0, 120.0),
];

/// Crest line (y) of the range along x, m.
fn crest_y(x: f32) -> f32 {
    let (s, _) = wp_math::sin_cos(x / 900.0);
    2650.0 + 90.0 * s
}

fn crest_h(x: f32) -> f32 {
    if x <= CREST[0].0 {
        return CREST[0].1;
    }
    for w in CREST.windows(2) {
        let ((x0, h0), (x1, h1)) = (w[0], w[1]);
        if x <= x1 {
            let t = (x - x0) / (x1 - x0);
            // Smooth between control points (no kinks in the crest line).
            let t = t * t * (3.0 - 2.0 * t);
            return h0 + (h1 - h0) * t;
        }
    }
    CREST[CREST.len() - 1].1
}

/// Silver River centreline y along x (east of the gorge's head), m.
fn silver_y(x: f32) -> f32 {
    let (s, _) = wp_math::sin_cos(x / 420.0);
    3330.0 - 0.05 * (x - plan::SILVER_GORGE[0]) + 35.0 * s
}

/// Sable River centreline x along y, m.
fn sable_x(y: f32) -> f32 {
    let (s, _) = wp_math::sin_cos(y / 650.0);
    plan::SABLE_RIVER_X + 150.0 * s
}

/// Lower `z` into a river valley: bed `bed` at the centreline, banks rising `slope` per m
/// beyond a `half` m wide bed.
fn valley(z: f32, dist: f32, bed: f32, half: f32, slope: f32) -> f32 {
    z.min(bed + (dist - half).max(0.0) * slope)
}

/// Distance of `p` outside Port Vesper's extent (0 inside), m.
fn outside_vesper(p: V2) -> f32 {
    let dx = (plan::VESPER_LO[0] - p[0])
        .max(p[0] - plan::VESPER_HI[0])
        .max(0.0);
    let dy = (plan::VESPER_LO[1] - p[1])
        .max(p[1] - plan::VESPER_HI[1])
        .max(0.0);
    (dx * dx + dy * dy).sqrt()
}

/// Ashpeak County's own ground (no blending).
struct Ground {
    n1: ValueNoise,
    n2: ValueNoise,
    n3: ValueNoise,
}

impl Ground {
    fn z(&self, p: V2) -> f32 {
        let [x, y] = p;
        // Plain: Harlan Flats rolls gently, the metro basin rises slowly northwards.
        let plain = 6.0
            + 20.0 * smoothstep(-900.0, 1600.0, y)
            + 10.0 * smoothstep(3500.0, 5600.0, y)
            + 7.0 * self.n1.at(p);
        // Range: rises from its southern foot to the crest line, falls to its northern foot.
        let cy = crest_y(x);
        let up = smoothstep(plan::RANGE_SOUTH_Y - 150.0, cy, y);
        let down = smoothstep(plan::RANGE_NORTH_Y + 150.0, cy, y);
        let ridge = up.min(down);
        let mountain = crest_h(x) * ridge;
        let rough = ridge * (28.0 * self.n2.at(p) + 9.0 * self.n3.at(p));
        let summit = 60.0 * bump(p, plan::ASHPEAK_SUMMIT, 450.0);
        let mut z = plain + mountain + rough + summit;
        // Silver River gorge along the range's northern foot (east of its head).
        if x > -500.0 {
            let head = smoothstep(-500.0, -200.0, x);
            let bed = -2.0 + 30.0 * smoothstep(1500.0, -400.0, x);
            let d = (y - silver_y(x)).abs();
            let carved = valley(z, d, bed, 18.0, 1.4);
            z += (carved - z) * head;
        }
        // Sable River valley through Harlan Flats down to the sea.
        if y < 2100.0 && x < plan::FARMS_EAST_X {
            let bed = -1.5 + 0.0045 * (y + 1150.0).max(0.0);
            let d = (x - sable_x(y)).abs();
            z = valley(z, d, bed, 25.0, 0.18);
        }
        // Sea: south of the coast (as Port Vesper's) and east of x = 1620.
        let land = z.max(1.5);
        let sea = smoothstep(-1110.0, -1180.0, y).max(smoothstep(1600.0, 1660.0, x));
        land + (-3.0 - land) * sea
    }
}

/// The combined map's natural ground: `f(p, vesper_z)`.
pub(super) fn natural(seed: u64) -> Box<dyn Fn(V2, f32) -> f32> {
    let mut rng = Rng::new(seed ^ 0xa5be_a4c0_u64);
    let ground = Ground {
        n1: ValueNoise::new(&mut rng, LO, plan::HI, 520.0),
        n2: ValueNoise::new(&mut rng, LO, plan::HI, 260.0),
        n3: ValueNoise::new(&mut rng, LO, plan::HI, 95.0),
    };
    Box::new(move |p, vesper_z| {
        let w = smoothstep(0.0, plan::BLEND_M, outside_vesper(p));
        if w <= 0.0 {
            return vesper_z;
        }
        let own = ground.z(p);
        vesper_z + (own - vesper_z) * w
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_saddles_and_summit() {
        let f = natural(1);
        let z = |p: V2| f(p, 0.0);
        let summit = z(plan::ASHPEAK_SUMMIT);
        let crowfoot = z(plan::CROWFOOT_SADDLE);
        assert!(summit > 450.0, "summit {summit}");
        assert!(
            crowfoot > 150.0 && crowfoot < summit - 150.0,
            "crowfoot {crowfoot}"
        );
        // Port Vesper's ground is untouched inside its extent.
        assert_eq!(f([0.0, 0.0], 12.5), 12.5);
        // Sea east of the metro.
        assert!(z([1690.0, 4500.0]) < 0.0);
        // The gorge is deep where Route 7 crosses it.
        let rim = z(crate::geom::add(plan::SILVER_GORGE, [0.0, -250.0]));
        let bed = z([plan::SILVER_GORGE[0], silver_y(plan::SILVER_GORGE[0])]);
        assert!(rim - bed > 25.0, "rim {rim} bed {bed}");
    }
}
