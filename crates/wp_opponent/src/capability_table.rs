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
        max_lat_g: 0.9589924,
        brake_g: 1.0874562,
        accel_mps2: 4.9677095,
        top_speed_mps: 67.52507,
    },
    CapRow {
        id: "brigand_400",
        mass_kg: 1989.6,
        drag_cd_a_m2: 1.171,
        peak_torque_nm: 475.0,
        redline_rpm: 4800.0,
        max_lat_g: 0.60017383,
        brake_g: 0.68291974,
        accel_mps2: 2.8723252,
        top_speed_mps: 53.90472,
    },
    CapRow {
        id: "bulwark_400",
        mass_kg: 1919.6,
        drag_cd_a_m2: 1.159,
        peak_torque_nm: 508.0,
        redline_rpm: 5000.0,
        max_lat_g: 0.71077985,
        brake_g: 0.85232055,
        accel_mps2: 3.5574527,
        top_speed_mps: 55.899273,
    },
    CapRow {
        id: "debug_coupe",
        mass_kg: 1500.0,
        drag_cd_a_m2: 0.66,
        peak_torque_nm: 510.0,
        redline_rpm: 7000.0,
        max_lat_g: 0.9151253,
        brake_g: 0.876473,
        accel_mps2: 5.389383,
        top_speed_mps: 80.62595,
    },
    CapRow {
        id: "debug_sedan",
        mass_kg: 1400.0,
        drag_cd_a_m2: 0.66,
        peak_torque_nm: 272.0,
        redline_rpm: 6500.0,
        max_lat_g: 0.8378017,
        brake_g: 0.9682631,
        accel_mps2: 3.4030967,
        top_speed_mps: 62.676334,
    },
    CapRow {
        id: "drayman_cargo",
        mass_kg: 2481.4,
        drag_cd_a_m2: 1.335,
        peak_torque_nm: 400.0,
        redline_rpm: 5600.0,
        max_lat_g: 0.65581214,
        brake_g: 0.9057868,
        accel_mps2: 2.5849812,
        top_speed_mps: 55.44533,
    },
    CapRow {
        id: "fulmine_v10",
        mass_kg: 1634.1,
        drag_cd_a_m2: 0.673,
        peak_torque_nm: 560.0,
        redline_rpm: 8500.0,
        max_lat_g: 0.9797781,
        brake_g: 1.1039021,
        accel_mps2: 7.662835,
        top_speed_mps: 92.65752,
    },
    CapRow {
        id: "furrow_24",
        mass_kg: 1372.7,
        drag_cd_a_m2: 1.022,
        peak_torque_nm: 218.0,
        redline_rpm: 5500.0,
        max_lat_g: 0.7323058,
        brake_g: 0.9033147,
        accel_mps2: 2.4866343,
        top_speed_mps: 48.90923,
    },
    CapRow {
        id: "hakone_gt",
        mass_kg: 1598.9,
        drag_cd_a_m2: 0.601,
        peak_torque_nm: 435.0,
        redline_rpm: 6800.0,
        max_lat_g: 0.8934442,
        brake_g: 1.0218695,
        accel_mps2: 4.718094,
        top_speed_mps: 76.7296,
    },
    CapRow {
        id: "hauler_1500",
        mass_kg: 2438.5,
        drag_cd_a_m2: 1.377,
        peak_torque_nm: 519.0,
        redline_rpm: 5800.0,
        max_lat_g: 0.61238986,
        brake_g: 0.882868,
        accel_mps2: 3.4704144,
        top_speed_mps: 56.967876,
    },
    CapRow {
        id: "ironside_440",
        mass_kg: 1695.2,
        drag_cd_a_m2: 1.039,
        peak_torque_nm: 560.0,
        redline_rpm: 5200.0,
        max_lat_g: 0.6104643,
        brake_g: 0.73633605,
        accel_mps2: 3.4776561,
        top_speed_mps: 58.516068,
    },
    CapRow {
        id: "junkyard_beater",
        mass_kg: 1048.2,
        drag_cd_a_m2: 0.731,
        peak_torque_nm: 124.0,
        redline_rpm: 5800.0,
        max_lat_g: 0.69669443,
        brake_g: 0.84901166,
        accel_mps2: 1.7758834,
        top_speed_mps: 40.37576,
    },
    CapRow {
        id: "kestrel_ts",
        mass_kg: 1452.7,
        drag_cd_a_m2: 0.668,
        peak_torque_nm: 335.0,
        redline_rpm: 6500.0,
        max_lat_g: 0.9227912,
        brake_g: 1.0153239,
        accel_mps2: 3.8182511,
        top_speed_mps: 66.93304,
    },
    CapRow {
        id: "lindwurm_4t",
        mass_kg: 1554.5,
        drag_cd_a_m2: 0.719,
        peak_torque_nm: 380.0,
        redline_rpm: 6500.0,
        max_lat_g: 0.9412171,
        brake_g: 1.0898507,
        accel_mps2: 4.99002,
        top_speed_mps: 70.56389,
    },
    CapRow {
        id: "lucciola_350",
        mass_kg: 1344.5,
        drag_cd_a_m2: 0.602,
        peak_torque_nm: 363.0,
        redline_rpm: 8500.0,
        max_lat_g: 0.9273614,
        brake_g: 1.1021627,
        accel_mps2: 5.931198,
        top_speed_mps: 80.13915,
    },
    CapRow {
        id: "marten_20",
        mass_kg: 1114.5,
        drag_cd_a_m2: 0.68,
        peak_torque_nm: 180.0,
        redline_rpm: 6800.0,
        max_lat_g: 0.8794263,
        brake_g: 1.0502552,
        accel_mps2: 3.2190566,
        top_speed_mps: 56.063526,
    },
    CapRow {
        id: "rampart_50",
        mass_kg: 1752.7,
        drag_cd_a_m2: 0.779,
        peak_torque_nm: 529.0,
        redline_rpm: 7000.0,
        max_lat_g: 0.8641824,
        brake_g: 1.009776,
        accel_mps2: 4.5289855,
        top_speed_mps: 72.78911,
    },
    CapRow {
        id: "satchel_20",
        mass_kg: 1895.2,
        drag_cd_a_m2: 1.392,
        peak_torque_nm: 176.0,
        redline_rpm: 5800.0,
        max_lat_g: 0.7222832,
        brake_g: 0.853491,
        accel_mps2: 1.5179113,
        top_speed_mps: 40.308605,
    },
    CapRow {
        id: "sentinel_bastion",
        mass_kg: 2638.5,
        drag_cd_a_m2: 1.378,
        peak_torque_nm: 645.0,
        redline_rpm: 5800.0,
        max_lat_g: 0.71710515,
        brake_g: 0.9192344,
        accel_mps2: 4.224757,
        top_speed_mps: 62.13859,
    },
    CapRow {
        id: "sentinel_cruiser",
        mass_kg: 2048.2,
        drag_cd_a_m2: 0.857,
        peak_torque_nm: 705.0,
        redline_rpm: 6000.0,
        max_lat_g: 0.83231735,
        brake_g: 0.9729574,
        accel_mps2: 4.7517223,
        top_speed_mps: 80.02057,
    },
    CapRow {
        id: "sentinel_vigil",
        mass_kg: 1812.7,
        drag_cd_a_m2: 0.85,
        peak_torque_nm: 865.0,
        redline_rpm: 7000.0,
        max_lat_g: 0.85006624,
        brake_g: 1.0309129,
        accel_mps2: 5.1880674,
        top_speed_mps: 88.138145,
    },
    CapRow {
        id: "sentinel_warden",
        mass_kg: 1872.6,
        drag_cd_a_m2: 1.013,
        peak_torque_nm: 550.0,
        redline_rpm: 5000.0,
        max_lat_g: 0.79902655,
        brake_g: 0.97460204,
        accel_mps2: 4.0749793,
        top_speed_mps: 62.26593,
    },
    CapRow {
        id: "sentinel_wolfhound",
        mass_kg: 2048.9,
        drag_cd_a_m2: 0.884,
        peak_torque_nm: 720.0,
        redline_rpm: 6000.0,
        max_lat_g: 0.8628881,
        brake_g: 1.0508442,
        accel_mps2: 5.762028,
        top_speed_mps: 78.78189,
    },
    CapRow {
        id: "shiro_turbo",
        mass_kg: 1300.8,
        drag_cd_a_m2: 0.531,
        peak_torque_nm: 276.0,
        redline_rpm: 7200.0,
        max_lat_g: 0.91847473,
        brake_g: 1.0567981,
        accel_mps2: 4.3431053,
        top_speed_mps: 72.155846,
    },
    CapRow {
        id: "sperber_32",
        mass_kg: 1601.5,
        drag_cd_a_m2: 0.655,
        peak_torque_nm: 365.0,
        redline_rpm: 7900.0,
        max_lat_g: 0.9197548,
        brake_g: 1.0578725,
        accel_mps2: 4.4953923,
        top_speed_mps: 74.23798,
    },
    CapRow {
        id: "stoat_16v",
        mass_kg: 1144.5,
        drag_cd_a_m2: 0.678,
        peak_torque_nm: 181.0,
        redline_rpm: 6800.0,
        max_lat_g: 0.870206,
        brake_g: 1.002008,
        accel_mps2: 3.0964544,
        top_speed_mps: 55.44373,
    },
    CapRow {
        id: "talus_20t",
        mass_kg: 1386.4,
        drag_cd_a_m2: 0.718,
        peak_torque_nm: 373.0,
        redline_rpm: 7000.0,
        max_lat_g: 0.92977756,
        brake_g: 1.0562881,
        accel_mps2: 5.0774307,
        top_speed_mps: 66.68715,
    },
    CapRow {
        id: "tansy_13",
        mass_kg: 1058.2,
        drag_cd_a_m2: 0.694,
        peak_torque_nm: 121.0,
        redline_rpm: 6200.0,
        max_lat_g: 0.81932473,
        brake_g: 0.99432635,
        accel_mps2: 2.009848,
        top_speed_mps: 45.125137,
    },
    CapRow {
        id: "tumbril_20",
        mass_kg: 1845.2,
        drag_cd_a_m2: 1.554,
        peak_torque_nm: 180.0,
        redline_rpm: 6000.0,
        max_lat_g: 0.7180753,
        brake_g: 0.8624256,
        accel_mps2: 1.5945148,
        top_speed_mps: 39.009422,
    },
    CapRow {
        id: "vireo_xs",
        mass_kg: 1108.2,
        drag_cd_a_m2: 0.644,
        peak_torque_nm: 152.0,
        redline_rpm: 7600.0,
        max_lat_g: 0.8827453,
        brake_g: 1.0419533,
        accel_mps2: 2.830856,
        top_speed_mps: 52.010956,
    },
];
