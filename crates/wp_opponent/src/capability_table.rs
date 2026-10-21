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
        id: "arashi_t4",
        mass_kg: 1312.7,
        drag_cd_a_m2: 0.706,
        peak_torque_nm: 340.0,
        redline_rpm: 7000.0,
        max_lat_g: 0.94894916,
        brake_g: 1.0734198,
        accel_mps2: 4.9382715,
        top_speed_mps: 68.92225,
    },
    CapRow {
        id: "brigand_400",
        mass_kg: 1989.6,
        drag_cd_a_m2: 1.171,
        peak_torque_nm: 475.0,
        redline_rpm: 4800.0,
        max_lat_g: 0.5755287,
        brake_g: 0.71709806,
        accel_mps2: 2.8344672,
        top_speed_mps: 54.68779,
    },
    CapRow {
        id: "bulwark_400",
        mass_kg: 1919.6,
        drag_cd_a_m2: 1.159,
        peak_torque_nm: 508.0,
        redline_rpm: 5000.0,
        max_lat_g: 0.7002093,
        brake_g: 0.9135559,
        accel_mps2: 3.5106196,
        top_speed_mps: 55.91675,
    },
    CapRow {
        id: "debug_coupe",
        mass_kg: 1500.0,
        drag_cd_a_m2: 0.66,
        peak_torque_nm: 510.0,
        redline_rpm: 7000.0,
        max_lat_g: 0.9361502,
        brake_g: 0.94480133,
        accel_mps2: 5.3763437,
        top_speed_mps: 80.480484,
    },
    CapRow {
        id: "debug_sedan",
        mass_kg: 1400.0,
        drag_cd_a_m2: 0.66,
        peak_torque_nm: 272.0,
        redline_rpm: 6500.0,
        max_lat_g: 0.8225206,
        brake_g: 1.0087949,
        accel_mps2: 3.3316674,
        top_speed_mps: 64.88475,
    },
    CapRow {
        id: "drayman_cargo",
        mass_kg: 2481.4,
        drag_cd_a_m2: 1.335,
        peak_torque_nm: 400.0,
        redline_rpm: 5600.0,
        max_lat_g: 0.646128,
        brake_g: 0.91605616,
        accel_mps2: 2.5572178,
        top_speed_mps: 56.07481,
    },
    CapRow {
        id: "furrow_24",
        mass_kg: 1372.7,
        drag_cd_a_m2: 1.022,
        peak_torque_nm: 218.0,
        redline_rpm: 5500.0,
        max_lat_g: 0.7209564,
        brake_g: 0.9401673,
        accel_mps2: 2.4737167,
        top_speed_mps: 49.753487,
    },
    CapRow {
        id: "hakone_gt",
        mass_kg: 1598.9,
        drag_cd_a_m2: 0.601,
        peak_torque_nm: 435.0,
        redline_rpm: 6800.0,
        max_lat_g: 0.90462226,
        brake_g: 1.0196587,
        accel_mps2: 4.6264167,
        top_speed_mps: 78.34913,
    },
    CapRow {
        id: "hauler_1500",
        mass_kg: 2438.5,
        drag_cd_a_m2: 1.377,
        peak_torque_nm: 519.0,
        redline_rpm: 5800.0,
        max_lat_g: 0.6094219,
        brake_g: 0.8858057,
        accel_mps2: 3.427592,
        top_speed_mps: 57.356747,
    },
    CapRow {
        id: "ironside_440",
        mass_kg: 1695.2,
        drag_cd_a_m2: 1.039,
        peak_torque_nm: 560.0,
        redline_rpm: 5200.0,
        max_lat_g: 0.5906859,
        brake_g: 0.7593877,
        accel_mps2: 3.399626,
        top_speed_mps: 58.459106,
    },
    CapRow {
        id: "junkyard_beater",
        mass_kg: 1048.2,
        drag_cd_a_m2: 0.731,
        peak_torque_nm: 124.0,
        redline_rpm: 5800.0,
        max_lat_g: 0.66457266,
        brake_g: 0.8679729,
        accel_mps2: 1.7688158,
        top_speed_mps: 40.49203,
    },
    CapRow {
        id: "kestrel_ts",
        mass_kg: 1452.7,
        drag_cd_a_m2: 0.668,
        peak_torque_nm: 335.0,
        redline_rpm: 6500.0,
        max_lat_g: 0.9044694,
        brake_g: 1.0222387,
        accel_mps2: 3.7686074,
        top_speed_mps: 68.897545,
    },
    CapRow {
        id: "lindwurm_4t",
        mass_kg: 1554.5,
        drag_cd_a_m2: 0.719,
        peak_torque_nm: 380.0,
        redline_rpm: 6500.0,
        max_lat_g: 0.9272439,
        brake_g: 1.0746102,
        accel_mps2: 4.9640107,
        top_speed_mps: 72.10733,
    },
    CapRow {
        id: "lucciola_350",
        mass_kg: 1344.5,
        drag_cd_a_m2: 0.602,
        peak_torque_nm: 363.0,
        redline_rpm: 8500.0,
        max_lat_g: 0.96911716,
        brake_g: 1.0944158,
        accel_mps2: 5.9259257,
        top_speed_mps: 80.69577,
    },
    CapRow {
        id: "marten_20",
        mass_kg: 1114.5,
        drag_cd_a_m2: 0.68,
        peak_torque_nm: 180.0,
        redline_rpm: 6800.0,
        max_lat_g: 0.8612511,
        brake_g: 1.0265903,
        accel_mps2: 3.1640563,
        top_speed_mps: 57.1444,
    },
    CapRow {
        id: "rampart_50",
        mass_kg: 1752.7,
        drag_cd_a_m2: 0.779,
        peak_torque_nm: 529.0,
        redline_rpm: 7000.0,
        max_lat_g: 0.8691106,
        brake_g: 1.0202022,
        accel_mps2: 4.4953923,
        top_speed_mps: 74.672485,
    },
    CapRow {
        id: "satchel_20",
        mass_kg: 1895.2,
        drag_cd_a_m2: 1.392,
        peak_torque_nm: 176.0,
        redline_rpm: 5800.0,
        max_lat_g: 0.69930327,
        brake_g: 0.87965584,
        accel_mps2: 1.5031942,
        top_speed_mps: 40.733494,
    },
    CapRow {
        id: "sentinel_bastion",
        mass_kg: 2638.5,
        drag_cd_a_m2: 1.378,
        peak_torque_nm: 519.0,
        redline_rpm: 5800.0,
        max_lat_g: 0.7101902,
        brake_g: 0.92021924,
        accel_mps2: 3.4904013,
        top_speed_mps: 59.58686,
    },
    CapRow {
        id: "sentinel_cruiser",
        mass_kg: 2048.2,
        drag_cd_a_m2: 0.857,
        peak_torque_nm: 535.0,
        redline_rpm: 5800.0,
        max_lat_g: 0.8321438,
        brake_g: 0.9847232,
        accel_mps2: 4.3402777,
        top_speed_mps: 72.4234,
    },
    CapRow {
        id: "sentinel_wolfhound",
        mass_kg: 2048.9,
        drag_cd_a_m2: 0.884,
        peak_torque_nm: 495.0,
        redline_rpm: 6000.0,
        max_lat_g: 0.8464481,
        brake_g: 1.0168829,
        accel_mps2: 4.6948357,
        top_speed_mps: 67.83273,
    },
    CapRow {
        id: "shiro_turbo",
        mass_kg: 1300.8,
        drag_cd_a_m2: 0.531,
        peak_torque_nm: 276.0,
        redline_rpm: 7200.0,
        max_lat_g: 0.92968905,
        brake_g: 1.05013,
        accel_mps2: 4.235493,
        top_speed_mps: 72.17973,
    },
    CapRow {
        id: "sperber_32",
        mass_kg: 1601.5,
        drag_cd_a_m2: 0.655,
        peak_torque_nm: 365.0,
        redline_rpm: 7900.0,
        max_lat_g: 0.920159,
        brake_g: 1.053651,
        accel_mps2: 4.3917437,
        top_speed_mps: 75.75281,
    },
    CapRow {
        id: "stoat_16v",
        mass_kg: 1144.5,
        drag_cd_a_m2: 0.678,
        peak_torque_nm: 181.0,
        redline_rpm: 6800.0,
        max_lat_g: 0.85034907,
        brake_g: 1.0029991,
        accel_mps2: 3.0778701,
        top_speed_mps: 56.81565,
    },
    CapRow {
        id: "talus_20t",
        mass_kg: 1386.4,
        drag_cd_a_m2: 0.718,
        peak_torque_nm: 373.0,
        redline_rpm: 7000.0,
        max_lat_g: 0.9172716,
        brake_g: 1.0428925,
        accel_mps2: 5.0466814,
        top_speed_mps: 66.73122,
    },
    CapRow {
        id: "tansy_13",
        mass_kg: 1058.2,
        drag_cd_a_m2: 0.694,
        peak_torque_nm: 121.0,
        redline_rpm: 6200.0,
        max_lat_g: 0.7742877,
        brake_g: 0.9858035,
        accel_mps2: 2.0244963,
        top_speed_mps: 45.131096,
    },
    CapRow {
        id: "tumbril_20",
        mass_kg: 1845.2,
        drag_cd_a_m2: 1.554,
        peak_torque_nm: 180.0,
        redline_rpm: 6000.0,
        max_lat_g: 0.706841,
        brake_g: 0.8810836,
        accel_mps2: 1.5779092,
        top_speed_mps: 39.35105,
    },
    CapRow {
        id: "vireo_xs",
        mass_kg: 1108.2,
        drag_cd_a_m2: 0.644,
        peak_torque_nm: 152.0,
        redline_rpm: 7600.0,
        max_lat_g: 0.86341476,
        brake_g: 1.0294513,
        accel_mps2: 2.8212724,
        top_speed_mps: 57.35706,
    },
];
