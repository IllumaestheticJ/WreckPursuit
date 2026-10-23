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
        brake_g: 0.9285448,
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
        brake_g: 0.98898804,
        accel_mps2: 3.3283408,
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
        id: "erg_hamada",
        mass_kg: 2113.2,
        drag_cd_a_m2: 2.023,
        peak_torque_nm: 560.0,
        redline_rpm: 6300.0,
        max_lat_g: 0.661839,
        brake_g: 0.89247113,
        accel_mps2: 4.471272,
        top_speed_mps: 55.917313,
    },
    CapRow {
        id: "fulmine_v10",
        mass_kg: 1634.1,
        drag_cd_a_m2: 0.673,
        peak_torque_nm: 560.0,
        redline_rpm: 8500.0,
        max_lat_g: 0.99088293,
        brake_g: 1.1109166,
        accel_mps2: 7.5843763,
        top_speed_mps: 93.51525,
    },
    CapRow {
        id: "furrow_24",
        mass_kg: 1372.7,
        drag_cd_a_m2: 1.022,
        peak_torque_nm: 218.0,
        redline_rpm: 5500.0,
        max_lat_g: 0.7442573,
        brake_g: 0.92160493,
        accel_mps2: 2.467308,
        top_speed_mps: 49.753498,
    },
    CapRow {
        id: "hakone_gt",
        mass_kg: 1598.9,
        drag_cd_a_m2: 0.601,
        peak_torque_nm: 435.0,
        redline_rpm: 6800.0,
        max_lat_g: 0.9272185,
        brake_g: 1.0113815,
        accel_mps2: 4.6264167,
        top_speed_mps: 78.34913,
    },
    CapRow {
        id: "halberd_40",
        mass_kg: 1488.2,
        drag_cd_a_m2: 0.722,
        peak_torque_nm: 800.0,
        redline_rpm: 8000.0,
        max_lat_g: 1.1572596,
        brake_g: 1.1804907,
        accel_mps2: 6.680026,
        top_speed_mps: 96.3676,
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
        max_lat_g: 0.6805943,
        brake_g: 0.8438046,
        accel_mps2: 1.7697549,
        top_speed_mps: 40.492233,
    },
    CapRow {
        id: "kagero_hx",
        mass_kg: 1833.0,
        drag_cd_a_m2: 0.662,
        peak_torque_nm: 646.0,
        redline_rpm: 7400.0,
        max_lat_g: 0.9628874,
        brake_g: 1.0870538,
        accel_mps2: 7.077141,
        top_speed_mps: 90.885445,
    },
    CapRow {
        id: "kestrel_ts",
        mass_kg: 1452.7,
        drag_cd_a_m2: 0.668,
        peak_torque_nm: 335.0,
        redline_rpm: 6500.0,
        max_lat_g: 0.9240476,
        brake_g: 1.0078266,
        accel_mps2: 3.7686074,
        top_speed_mps: 68.897484,
    },
    CapRow {
        id: "lindwurm_4t",
        mass_kg: 1554.5,
        drag_cd_a_m2: 0.719,
        peak_torque_nm: 380.0,
        redline_rpm: 6500.0,
        max_lat_g: 0.9463693,
        brake_g: 1.0606524,
        accel_mps2: 4.97884,
        top_speed_mps: 72.10748,
    },
    CapRow {
        id: "lucciola_350",
        mass_kg: 1344.5,
        drag_cd_a_m2: 0.602,
        peak_torque_nm: 363.0,
        redline_rpm: 8500.0,
        max_lat_g: 0.98825836,
        brake_g: 1.0853727,
        accel_mps2: 5.9259257,
        top_speed_mps: 80.69577,
    },
    CapRow {
        id: "merlo_20t",
        mass_kg: 1237.8,
        drag_cd_a_m2: 0.859,
        peak_torque_nm: 420.0,
        redline_rpm: 7000.0,
        max_lat_g: 1.0889323,
        brake_g: 1.1445038,
        accel_mps2: 5.3763437,
        top_speed_mps: 66.93996,
    },
    CapRow {
        id: "mesquite_pr",
        mass_kg: 2758.5,
        drag_cd_a_m2: 1.924,
        peak_torque_nm: 588.0,
        redline_rpm: 6000.0,
        max_lat_g: 0.62915725,
        brake_g: 0.8393927,
        accel_mps2: 3.4118047,
        top_speed_mps: 54.7625,
    },
    CapRow {
        id: "morrigan_55",
        mass_kg: 1287.8,
        drag_cd_a_m2: 0.925,
        peak_torque_nm: 660.0,
        redline_rpm: 7300.0,
        max_lat_g: 1.1800148,
        brake_g: 1.2250822,
        accel_mps2: 6.485084,
        top_speed_mps: 80.94056,
    },
    CapRow {
        id: "nyx_proto",
        mass_kg: 977.1,
        drag_cd_a_m2: 0.83,
        peak_torque_nm: 515.0,
        redline_rpm: 9200.0,
        max_lat_g: 1.2926826,
        brake_g: 1.2320365,
        accel_mps2: 6.9444447,
        top_speed_mps: 86.46057,
    },
    CapRow {
        id: "ombra_v12",
        mass_kg: 1691.5,
        drag_cd_a_m2: 0.702,
        peak_torque_nm: 690.0,
        redline_rpm: 8500.0,
        max_lat_g: 0.97973037,
        brake_g: 1.1038592,
        accel_mps2: 8.022463,
        top_speed_mps: 96.83124,
    },
    CapRow {
        id: "ostwind_6t",
        mass_kg: 1690.0,
        drag_cd_a_m2: 0.685,
        peak_torque_nm: 800.0,
        redline_rpm: 7000.0,
        max_lat_g: 0.9594399,
        brake_g: 1.089508,
        accel_mps2: 7.3827977,
        top_speed_mps: 94.58034,
    },
    CapRow {
        id: "rampart_50",
        mass_kg: 1752.7,
        drag_cd_a_m2: 0.779,
        peak_torque_nm: 529.0,
        redline_rpm: 7000.0,
        max_lat_g: 0.8960953,
        brake_g: 1.0117724,
        accel_mps2: 4.4953923,
        top_speed_mps: 74.672485,
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
        id: "sentinel_harrier",
        mass_kg: 1812.7,
        drag_cd_a_m2: 0.85,
        peak_torque_nm: 529.0,
        redline_rpm: 7000.0,
        max_lat_g: 0.8872415,
        brake_g: 1.0200145,
        accel_mps2: 4.8169556,
        top_speed_mps: 76.45285,
    },
    CapRow {
        id: "sentinel_warden",
        mass_kg: 1872.6,
        drag_cd_a_m2: 1.013,
        peak_torque_nm: 412.0,
        redline_rpm: 5000.0,
        max_lat_g: 0.8226363,
        brake_g: 0.9760545,
        accel_mps2: 3.1505983,
        top_speed_mps: 56.06159,
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
        max_lat_g: 0.9456339,
        brake_g: 1.0441606,
        accel_mps2: 4.235493,
        top_speed_mps: 72.17973,
    },
    CapRow {
        id: "sperber_32",
        mass_kg: 1601.5,
        drag_cd_a_m2: 0.655,
        peak_torque_nm: 365.0,
        redline_rpm: 7900.0,
        max_lat_g: 0.94291836,
        brake_g: 1.0455843,
        accel_mps2: 4.3917437,
        top_speed_mps: 75.75281,
    },
    CapRow {
        id: "tansy_13",
        mass_kg: 1058.2,
        drag_cd_a_m2: 0.694,
        peak_torque_nm: 121.0,
        redline_rpm: 6200.0,
        max_lat_g: 0.8160627,
        brake_g: 0.9668431,
        accel_mps2: 1.9859,
        top_speed_mps: 45.13135,
    },
    CapRow {
        id: "vireo_xs",
        mass_kg: 1108.2,
        drag_cd_a_m2: 0.644,
        peak_torque_nm: 152.0,
        redline_rpm: 7600.0,
        max_lat_g: 0.8765436,
        brake_g: 1.0199542,
        accel_mps2: 2.8236623,
        top_speed_mps: 57.357094,
    },
];
