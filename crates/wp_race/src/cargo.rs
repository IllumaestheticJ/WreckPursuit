//! Delivery cargo as real mass on the car.
//!
//! `wp_vehicle` builds the chassis body and the static wheel loads from the car's
//! [`VehicleDef`] (mass, front weight fraction, CG height, principal inertias). So the cargo is
//! applied by spawning the Delivery entrant with a [`LoadedCar`] whose definition already
//! carries the cargo: [`load_cargo`] adds the mass, moves the CG towards the cargo and adds its
//! inertia (box + parallel-axis terms). The car then accelerates, brakes, squats and turns in
//! like a loaded car, with no change to `wp_vehicle` or `wp_sim`.
//!
//! Server use (Delivery events):
//! ```ignore
//! let car = match ev.kind {
//!     RaceKind::Delivery { cargo_kg, .. } => {
//!         wp_race::cargo::load_cargo(&car, &CargoLoad::in_boot(&car.def, cargo_kg))
//!     }
//!     _ => car,
//! };
//! let id = sim.spawn_vehicle(&car, slot.pos, yaw)?;
//! ```

use serde::{Deserialize, Serialize};
use wp_contracts::{LoadedCar, VehicleDef};

use crate::kind::RaceKind;

/// Cargo placed in a car (chassis space: +X right, +Y forward, +Z up from the ground plane at
/// static ride height, same frame as `WheelDef::position_m`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CargoLoad {
    /// Cargo mass, kg.
    pub mass_kg: f32,
    /// Centre of the cargo in chassis space, m.
    pub pos_m: [f32; 3],
    /// Cargo box size `[x, y, z]`, m (for its own inertia).
    pub size_m: [f32; 3],
}

impl CargoLoad {
    /// Cargo of `mass_kg` in the boot: centred, just behind the rear axle, on the boot floor.
    pub fn in_boot(def: &VehicleDef, mass_kg: f32) -> CargoLoad {
        let y_r = def.wheels[2].position_m[1];
        let half_len = def.body_size_m[1] * 0.5;
        // Behind the rear axle but inside the body.
        let y = (y_r - 0.35).max(-half_len + 0.45);
        CargoLoad {
            mass_kg: mass_kg.max(0.0),
            pos_m: [0.0, y, 0.55],
            size_m: [1.0, 0.7, 0.45],
        }
    }

    /// The cargo for a race kind (`Delivery { cargo_kg, .. }`), `None` for every other kind.
    pub fn for_kind(kind: RaceKind, def: &VehicleDef) -> Option<CargoLoad> {
        match kind {
            RaceKind::Delivery { cargo_kg, .. } if cargo_kg > 0.0 => {
                Some(CargoLoad::in_boot(def, cargo_kg))
            }
            _ => None,
        }
    }
}

/// A copy of `car` carrying `cargo`: mass added, front weight fraction and CG height moved to
/// the combined centre of mass, principal inertias grown by the cargo box and both bodies'
/// parallel-axis terms about the new CG. Lateral offset of the cargo is ignored for the CG
/// (the contract has no lateral CG) but counted in the inertia.
pub fn load_cargo(car: &LoadedCar, cargo: &CargoLoad) -> LoadedCar {
    let mut out = car.clone();
    if cargo.mass_kg <= 0.0 {
        return out;
    }
    let d = &car.def;
    let m = d.mass_kg;
    let mc = cargo.mass_kg;
    let total = m + mc;
    let y_f = d.wheels[0].position_m[1];
    let y_r = d.wheels[2].position_m[1];
    let wb = y_f - y_r;
    // Car CG in chassis space.
    let car_cg = [0.0, y_r + d.front_weight_frac * wb, d.cg_height_m];
    let c = cargo.pos_m;
    let new_cg = [
        0.0,
        (m * car_cg[1] + mc * c[1]) / total,
        (m * car_cg[2] + mc * c[2]) / total,
    ];
    let front_frac = if wb.abs() > 1e-3 {
        ((new_cg[1] - y_r) / wb).clamp(0.05, 0.95)
    } else {
        d.front_weight_frac
    };
    // Offsets from the new CG.
    let dc = [
        car_cg[0] - new_cg[0],
        car_cg[1] - new_cg[1],
        car_cg[2] - new_cg[2],
    ];
    let dq = [c[0] - new_cg[0], c[1] - new_cg[1], c[2] - new_cg[2]];
    let [sx, sy, sz] = cargo.size_m;
    let box_i = [
        mc / 12.0 * (sy * sy + sz * sz), // roll, about Y
        mc / 12.0 * (sx * sx + sz * sz), // pitch, about X
        mc / 12.0 * (sx * sx + sy * sy), // yaw, about Z
    ];
    // Perpendicular distance² to each axis: roll (Y) → x, z; pitch (X) → y, z; yaw (Z) → x, y.
    let perp = |v: [f32; 3]| {
        [
            v[0] * v[0] + v[2] * v[2],
            v[1] * v[1] + v[2] * v[2],
            v[0] * v[0] + v[1] * v[1],
        ]
    };
    let pc = perp(dc);
    let pq = perp(dq);
    let i0 = d.inertia_kgm2;
    let inertia = [
        i0[0] + m * pc[0] + box_i[0] + mc * pq[0],
        i0[1] + m * pc[1] + box_i[1] + mc * pq[1],
        i0[2] + m * pc[2] + box_i[2] + mc * pq[2],
    ];
    out.def.mass_kg = total;
    out.def.front_weight_frac = front_frac;
    out.def.cg_height_m = new_cg[2];
    out.def.inertia_kgm2 = inertia;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sedan() -> LoadedCar {
        let root = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../data/cars/debug_sedan.json"
        );
        wp_contracts::load_car(root).expect("debug_sedan loads")
    }

    #[test]
    fn cargo_adds_mass_moves_cg_back_and_grows_inertia() {
        let car = sedan();
        let cargo = CargoLoad::in_boot(&car.def, 300.0);
        let loaded = load_cargo(&car, &cargo);
        let (a, b) = (&car.def, &loaded.def);
        assert!((b.mass_kg - a.mass_kg - 300.0).abs() < 1e-3);
        assert!(
            b.front_weight_frac < a.front_weight_frac,
            "CG must move back"
        );
        assert!(b.front_weight_frac > 0.3);
        for k in 0..3 {
            assert!(b.inertia_kgm2[k] > a.inertia_kgm2[k], "axis {k}");
        }
        // Pitch and yaw grow far more than roll (cargo sits on the centreline, far aft).
        let grow = |k: usize| b.inertia_kgm2[k] / a.inertia_kgm2[k];
        assert!(grow(2) > grow(0));
        // Static front axle load stays put or drops; rear axle load rises by most of the cargo.
        let g = 9.81;
        let front = |d: &VehicleDef| d.mass_kg * d.front_weight_frac * g;
        let rear = |d: &VehicleDef| d.mass_kg * (1.0 - d.front_weight_frac) * g;
        assert!(front(b) <= front(a) + 1.0);
        assert!(rear(b) - rear(a) > 300.0 * g);
    }

    #[test]
    fn zero_cargo_is_identity_and_kinds_without_cargo_get_none() {
        let car = sedan();
        let same = load_cargo(&car, &CargoLoad::in_boot(&car.def, 0.0));
        assert_eq!(same.def, car.def);
        assert!(CargoLoad::for_kind(RaceKind::Sprint, &car.def).is_none());
        let k = RaceKind::Delivery {
            cargo_kg: 150.0,
            cargo_value: 1000,
        };
        assert_eq!(CargoLoad::for_kind(k, &car.def).unwrap().mass_kg, 150.0);
    }
}
