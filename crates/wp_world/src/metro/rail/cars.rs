//! Parked rail cars: locomotives, freight wagons, coaches. Every car is a solid underframe
//! plus a solid body (cars collide with the whole silhouette; nothing to wedge under).

use wp_math::Rng;

use super::super::ctx::{box_on, yaw_of};
use crate::geom::{self, V2, V3};
use crate::network::{Feature, FeatureKind, Material};

/// Height of the rail top above the formation, m.
pub const RAIL_TOP_M: f32 = 0.46;
/// Gap between coupled cars, m.
const COUPLING_M: f32 = 1.2;

/// Kind of a rail car.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarKind {
    /// Diesel road locomotive.
    Loco,
    /// Box car.
    Boxcar,
    /// Tank car (a box: lying cylinders are not a shape).
    Tank,
    /// Open hopper with a load.
    Hopper,
    /// Flat car with one or two containers.
    Flat,
    /// Passenger coach.
    Coach,
    /// Passenger power car (cab at one end).
    PowerCar,
    /// Old steam locomotive with tender (roundhouse).
    Steam,
}

impl CarKind {
    /// Length over couplers, m.
    pub fn length(self) -> f32 {
        match self {
            CarKind::Loco => 21.0,
            CarKind::Boxcar => 16.5,
            CarKind::Tank => 15.0,
            CarKind::Hopper => 14.0,
            CarKind::Flat => 18.5,
            CarKind::Coach => 24.0,
            CarKind::PowerCar => 20.0,
            CarKind::Steam => 22.0,
        }
    }
}

/// A random freight consist of at most `max_len` metres (locomotive first when `loco`).
pub fn freight_consist(rng: &mut Rng, max_len: f32, loco: bool) -> Vec<CarKind> {
    let mut out = Vec::new();
    let mut len = 0.0;
    if loco {
        let n = 1 + rng.below(2) as usize;
        for _ in 0..n {
            if len + CarKind::Loco.length() > max_len {
                break;
            }
            out.push(CarKind::Loco);
            len += CarKind::Loco.length() + COUPLING_M;
        }
    }
    // Wagons come in blocks of one type, like real trains.
    while len < max_len {
        let kind = match rng.below(4) {
            0 => CarKind::Boxcar,
            1 => CarKind::Tank,
            2 => CarKind::Hopper,
            _ => CarKind::Flat,
        };
        let block = 2 + rng.below(5);
        for _ in 0..block {
            if len + kind.length() > max_len {
                return out;
            }
            out.push(kind);
            len += kind.length() + COUPLING_M;
        }
    }
    out
}

/// Total length of a consist, m.
pub fn consist_length(cars: &[CarKind]) -> f32 {
    cars.iter().map(|c| c.length() + COUPLING_M).sum::<f32>() - COUPLING_M
}

/// Park `cars` along a track polyline (`z` = formation) starting at arc length `s0` and
/// running towards increasing `s`. Returns the arc length after the last car.
pub fn park(
    line: &[V3],
    cum: &[f32],
    s0: f32,
    cars: &[CarKind],
    rng: &mut Rng,
    out: &mut Vec<Feature>,
) -> f32 {
    let mut s = s0;
    for (k, kind) in cars.iter().enumerate() {
        let l = kind.length();
        let (a, _) = geom::point_at(line, cum, s);
        let (b, _) = geom::point_at(line, cum, s + l);
        // A locomotive at the end of a consist faces the other way.
        let flip = k + 1 == cars.len() && k > 0 && *kind == CarKind::Loco;
        let (a2, b2) = if flip { (b, a) } else { (a, b) };
        car(*kind, a2, b2, rng, out);
        s += l + COUPLING_M;
    }
    s
}

/// One car between its coupler points `a` → `b` (front at `b`; `z` = formation).
pub fn car(kind: CarKind, a: V3, b: V3, rng: &mut Rng, out: &mut Vec<Feature>) {
    let c: V2 = [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5];
    let yaw = yaw_of(geom::xy(a), geom::xy(b));
    let fwd = geom::normalize(geom::sub(geom::xy(b), geom::xy(a)));
    let rail = (a[2] + b[2]) * 0.5 + RAIL_TOP_M;
    let l = kind.length() - 0.8;
    let paint = [
        Material::Rust,
        Material::Red,
        Material::Blue,
        Material::Green,
        Material::Steel,
    ];
    // Underframe and bogies.
    let frame_h = if kind == CarKind::Flat { 1.25 } else { 1.0 };
    out.push(sbox(
        Material::Black,
        c,
        rail + 0.05,
        [2.7, l - 0.6, frame_h - 0.05],
        FeatureKind::RailCar,
        yaw,
    ));
    let body_z = rail + frame_h;
    let off = |d: f32| [c[0] + fwd[0] * d, c[1] + fwd[1] * d];
    match kind {
        CarKind::Loco => {
            let m = [Material::SafetyYellow, Material::Red, Material::Blue][rng.below(3) as usize];
            // Long hood, then the taller cab at the front.
            out.push(sbox(
                m,
                off(-2.0),
                body_z,
                [2.9, l - 4.0, 3.0],
                FeatureKind::RailCar,
                yaw,
            ));
            out.push(sbox(
                m,
                off(l * 0.5 - 2.0),
                body_z,
                [3.05, 4.0, 3.4],
                FeatureKind::RailCar,
                yaw,
            ));
            out.push(box_on(
                FeatureKind::RailCar,
                Material::Light,
                off(l * 0.5 - 0.02),
                body_z + 2.6,
                [1.2, 0.08, 0.3],
                yaw,
                false,
            ));
        }
        CarKind::Boxcar => {
            let m = paint[rng.below(paint.len() as u32) as usize];
            out.push(sbox(m, c, body_z, [3.1, l, 3.2], FeatureKind::RailCar, yaw));
        }
        CarKind::Tank => {
            let m = [Material::Black, Material::White, Material::Steel][rng.below(3) as usize];
            out.push(sbox(
                m,
                c,
                body_z,
                [2.9, l - 1.0, 2.7],
                FeatureKind::RailCar,
                yaw,
            ));
            out.push(box_on(
                FeatureKind::RailCar,
                m,
                c,
                body_z + 2.7,
                [1.2, 1.4, 0.5],
                yaw,
                false,
            ));
        }
        CarKind::Hopper => {
            let m = [Material::Rust, Material::Steel][rng.below(2) as usize];
            out.push(sbox(m, c, body_z, [3.1, l, 2.9], FeatureKind::RailCar, yaw));
            out.push(box_on(
                FeatureKind::RailCar,
                [Material::Black, Material::Gravel, Material::Sand][rng.below(3) as usize],
                c,
                body_z + 2.9,
                [2.8, l - 0.6, 0.15],
                yaw,
                false,
            ));
        }
        CarKind::Flat => {
            let cm = [
                Material::Red,
                Material::Blue,
                Material::Green,
                Material::White,
                Material::Rust,
            ];
            if rng.chance(0.25) {
                // Empty flat car: nothing above the deck.
            } else if rng.chance(0.5) {
                let m = cm[rng.below(cm.len() as u32) as usize];
                out.push(sbox(
                    m,
                    c,
                    body_z,
                    [2.44, 12.2, 2.6],
                    FeatureKind::Container,
                    yaw,
                ));
            } else {
                for d in [-3.3, 3.3] {
                    let m = cm[rng.below(cm.len() as u32) as usize];
                    out.push(sbox(
                        m,
                        off(d),
                        body_z,
                        [2.44, 6.06, 2.6],
                        FeatureKind::Container,
                        yaw,
                    ));
                }
            }
        }
        CarKind::Coach | CarKind::PowerCar => {
            let m = if kind == CarKind::PowerCar {
                Material::Red
            } else {
                Material::White
            };
            out.push(sbox(m, c, body_z, [3.0, l, 3.0], FeatureKind::RailCar, yaw));
            out.push(box_on(
                FeatureKind::RailCar,
                Material::Glass,
                c,
                body_z + 1.2,
                [3.04, l - 2.0, 0.9],
                yaw,
                false,
            ));
        }
        CarKind::Steam => {
            // Boiler + smokebox up front, cab, tender behind.
            out.push(sbox(
                Material::Black,
                off(3.0),
                body_z,
                [2.4, 11.0, 2.4],
                FeatureKind::RailCar,
                yaw,
            ));
            out.push(sbox(
                Material::Black,
                off(9.0),
                body_z + 2.4,
                [0.8, 0.8, 1.0],
                FeatureKind::RailCar,
                yaw,
            ));
            out.push(sbox(
                Material::Black,
                off(-3.6),
                body_z,
                [3.0, 3.6, 3.2],
                FeatureKind::RailCar,
                yaw,
            ));
            out.push(sbox(
                Material::Rust,
                off(-8.3),
                body_z,
                [2.9, 5.0, 2.4],
                FeatureKind::RailCar,
                yaw,
            ));
        }
    }
}

/// A solid box standing on `z0` (see [`box_on`]).
fn sbox(m: Material, at: V2, z0: f32, size: V3, kind: FeatureKind, yaw: f32) -> Feature {
    box_on(kind, m, at, z0, size, yaw, true)
}
