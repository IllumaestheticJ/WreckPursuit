//! Pre-measured [`CarCapability`](crate::CarCapability) limits for every car in `data/cars`.
//!
//! `CarCapability::measure` runs the `wp_vehicle::metrics` rigs (~0.3 s per car in release,
//! much more in debug and in the browser), so race glue reads this table instead
//! ([`CarCapability::for_car`](crate::CarCapability::for_car)). A row only applies to a car
//! whose fingerprint (mass, drag area, peak torque, redline) still matches; anything else is
//! measured. The rows are generated, never hand-edited:
//!
//! ```sh
//! WP_WRITE_CAPS=1 cargo test --release -p wp_opponent --test capability_table -- --nocapture
//! ```
//!
//! `tests/capability_table.rs` (release builds) fails when a row drifts more than 2 % from a
//! fresh measurement, e.g. after a vehicle-physics change.

use wp_contracts::LoadedCar;

/// One measured car.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CapRow {
    /// Car id.
    pub id: &'static str,
    /// Fingerprint: mass, kg.
    pub mass_kg: f32,
    /// Fingerprint: drag area, m².
    pub drag_cd_a_m2: f32,
    /// Fingerprint: peak engine torque, N·m.
    pub peak_torque_nm: f32,
    /// Fingerprint: redline, rpm.
    pub redline_rpm: f32,
    /// Measured lateral limit, g.
    pub max_lat_g: f32,
    /// Measured braking, g.
    pub brake_g: f32,
    /// Measured mean 0–100 km/h acceleration, m/s².
    pub accel_mps2: f32,
    /// Measured top speed, m/s.
    pub top_speed_mps: f32,
}

/// The fingerprint of `car`: `(mass, drag area, peak torque, redline)`.
pub fn fingerprint(car: &LoadedCar) -> (f32, f32, f32, f32) {
    let d = &car.def;
    let peak = d
        .engine
        .torque_curve_nm
        .iter()
        .fold(0.0f32, |m, (_, t)| m.max(*t));
    (d.mass_kg, d.drag_cd_a_m2, peak, d.engine.redline_rpm)
}

/// The table row for `car`, if its id is listed and its fingerprint still matches.
pub fn lookup(car: &LoadedCar) -> Option<&'static CapRow> {
    let (m, cda, tq, rl) = fingerprint(car);
    let close = |a: f32, b: f32| (a - b).abs() <= 1e-3 * b.abs().max(1e-3);
    TABLE.iter().find(|r| {
        r.id == car.def.id
            && close(m, r.mass_kg)
            && close(cda, r.drag_cd_a_m2)
            && close(tq, r.peak_torque_nm)
            && close(rl, r.redline_rpm)
    })
}

/// Every row (generated).
pub const TABLE: &[CapRow] = &[
    CapRow {
        id: "debug_coupe",
        mass_kg: 1500.0,
        drag_cd_a_m2: 0.66,
        peak_torque_nm: 510.0,
        redline_rpm: 7000.0,
        max_lat_g: 0.95425475,
        brake_g: 0.9284103,
        accel_mps2: 5.3763437,
        top_speed_mps: 80.480484,
    },
    CapRow {
        id: "debug_sedan",
        mass_kg: 1400.0,
        drag_cd_a_m2: 0.66,
        peak_torque_nm: 272.0,
        redline_rpm: 6500.0,
        max_lat_g: 0.84225875,
        brake_g: 0.9890521,
        accel_mps2: 3.320053,
        top_speed_mps: 64.88479,
    },
    CapRow {
        id: "drayman_cargo",
        mass_kg: 2481.4,
        drag_cd_a_m2: 1.335,
        peak_torque_nm: 400.0,
        redline_rpm: 5600.0,
        max_lat_g: 0.67687064,
        brake_g: 0.9039655,
        accel_mps2: 2.5572178,
        top_speed_mps: 56.07481,
    },
    CapRow {
        id: "furrow_24",
        mass_kg: 1372.7,
        drag_cd_a_m2: 1.022,
        peak_torque_nm: 218.0,
        redline_rpm: 5500.0,
        max_lat_g: 0.74519396,
        brake_g: 0.9152694,
        accel_mps2: 2.5680532,
        top_speed_mps: 49.728767,
    },
    CapRow {
        id: "hakone_gt",
        mass_kg: 1598.9,
        drag_cd_a_m2: 0.601,
        peak_torque_nm: 435.0,
        redline_rpm: 6800.0,
        max_lat_g: 0.92716086,
        brake_g: 1.0133064,
        accel_mps2: 4.721435,
        top_speed_mps: 78.3796,
    },
    CapRow {
        id: "hauler_1500",
        mass_kg: 2438.5,
        drag_cd_a_m2: 1.377,
        peak_torque_nm: 519.0,
        redline_rpm: 5800.0,
        max_lat_g: 0.6348734,
        brake_g: 0.87581587,
        accel_mps2: 3.427592,
        top_speed_mps: 57.356747,
    },
    CapRow {
        id: "ironside_440",
        mass_kg: 1695.2,
        drag_cd_a_m2: 1.039,
        peak_torque_nm: 560.0,
        redline_rpm: 5200.0,
        max_lat_g: 0.6181072,
        brake_g: 0.74239516,
        accel_mps2: 3.3978934,
        top_speed_mps: 58.459175,
    },
    CapRow {
        id: "junkyard_beater",
        mass_kg: 1048.2,
        drag_cd_a_m2: 0.731,
        peak_torque_nm: 124.0,
        redline_rpm: 5800.0,
        max_lat_g: 0.68090725,
        brake_g: 0.84656286,
        accel_mps2: 1.7969452,
        top_speed_mps: 40.492233,
    },
    CapRow {
        id: "kestrel_ts",
        mass_kg: 1452.7,
        drag_cd_a_m2: 0.668,
        peak_torque_nm: 335.0,
        redline_rpm: 6500.0,
        max_lat_g: 0.9252982,
        brake_g: 1.0182964,
        accel_mps2: 3.8940809,
        top_speed_mps: 68.90498,
    },
    CapRow {
        id: "lindwurm_4t",
        mass_kg: 1554.5,
        drag_cd_a_m2: 0.719,
        peak_torque_nm: 380.0,
        redline_rpm: 6500.0,
        max_lat_g: 0.94726384,
        brake_g: 1.0676633,
        accel_mps2: 5.104645,
        top_speed_mps: 72.11818,
    },
    CapRow {
        id: "lucciola_350",
        mass_kg: 1344.5,
        drag_cd_a_m2: 0.602,
        peak_torque_nm: 363.0,
        redline_rpm: 8500.0,
        max_lat_g: 0.9874579,
        brake_g: 1.0921491,
        accel_mps2: 6.1387353,
        top_speed_mps: 80.69479,
    },
    CapRow {
        id: "rampart_50",
        mass_kg: 1752.7,
        drag_cd_a_m2: 0.779,
        peak_torque_nm: 529.0,
        redline_rpm: 7000.0,
        max_lat_g: 0.8959025,
        brake_g: 1.015461,
        accel_mps2: 4.7619047,
        top_speed_mps: 74.70939,
    },
    CapRow {
        id: "sentinel_bastion",
        mass_kg: 2638.5,
        drag_cd_a_m2: 1.378,
        peak_torque_nm: 519.0,
        redline_rpm: 5800.0,
        max_lat_g: 0.7409237,
        brake_g: 0.90511006,
        accel_mps2: 3.4849277,
        top_speed_mps: 59.58643,
    },
    CapRow {
        id: "sentinel_cruiser",
        mass_kg: 2048.2,
        drag_cd_a_m2: 0.857,
        peak_torque_nm: 535.0,
        redline_rpm: 5800.0,
        max_lat_g: 0.8545221,
        brake_g: 0.9773005,
        accel_mps2: 4.3402777,
        top_speed_mps: 72.4234,
    },
    CapRow {
        id: "sentinel_wolfhound",
        mass_kg: 2048.9,
        drag_cd_a_m2: 0.884,
        peak_torque_nm: 495.0,
        redline_rpm: 6000.0,
        max_lat_g: 0.8688316,
        brake_g: 0.994762,
        accel_mps2: 4.6948357,
        top_speed_mps: 67.83273,
    },
    CapRow {
        id: "shiro_turbo",
        mass_kg: 1300.8,
        drag_cd_a_m2: 0.531,
        peak_torque_nm: 276.0,
        redline_rpm: 7200.0,
        max_lat_g: 0.947113,
        brake_g: 1.045007,
        accel_mps2: 4.309416,
        top_speed_mps: 73.50169,
    },
    CapRow {
        id: "sperber_32",
        mass_kg: 1601.5,
        drag_cd_a_m2: 0.655,
        peak_torque_nm: 365.0,
        redline_rpm: 7900.0,
        max_lat_g: 0.9435634,
        brake_g: 1.048625,
        accel_mps2: 4.588208,
        top_speed_mps: 75.80255,
    },
    CapRow {
        id: "tansy_13",
        mass_kg: 1058.2,
        drag_cd_a_m2: 0.694,
        peak_torque_nm: 121.0,
        redline_rpm: 6200.0,
        max_lat_g: 0.8113747,
        brake_g: 0.98728687,
        accel_mps2: 2.072324,
        top_speed_mps: 46.66953,
    },
    CapRow {
        id: "vireo_xs",
        mass_kg: 1108.2,
        drag_cd_a_m2: 0.644,
        peak_torque_nm: 152.0,
        redline_rpm: 7600.0,
        max_lat_g: 0.878447,
        brake_g: 1.0301195,
        accel_mps2: 2.9316916,
        top_speed_mps: 57.37502,
    },
];
