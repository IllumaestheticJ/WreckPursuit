//! Car stat bars for the menus: the `data/stats/cars.json` table.
//!
//! Every car is measured with [`wp_vehicle::stats::CarStats::measure`] (0–100, top speed,
//! skidpad grip on the `wp_vehicle` rig) and placed on the fixed scales of
//! [`wp_vehicle::stats`]. `simcli car-stats` writes the table; `simcli car-stats --check` and the
//! `car_stats_table_is_current` test fail when the physics or a car file moved a stat by more
//! than [`TOLERANCE`]. The browser reads the embedded table (`wp_wasm::car_stats_json`).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use wp_vehicle::stats::{self, CarStats};

use crate::data::{load_car, DataSource};
use crate::RunError;

/// Path of the table under the data root.
pub const TABLE_PATH: &str = "stats/cars.json";

/// Largest drift between the table and a fresh measurement before `--check` fails:
/// 0–100 s, top speed km/h, cornering g.
pub const TOLERANCE: (f32, f32, f32) = (0.1, 2.0, 0.01);

/// The whole table (`data/stats/cars.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CarStatsTable {
    /// How the stats were measured and where the bar scales start and end.
    pub scales: Scales,
    /// Stats by car id, sorted.
    pub cars: BTreeMap<String, CarStatsEntry>,
}

/// Bar scale ends (copied from [`wp_vehicle::stats`] so the table explains itself).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scales {
    /// 0–100 km/h time for an empty / full acceleration bar, s.
    pub zero_to_100_s: [f32; 2],
    /// Top speed for an empty / full bar, km/h.
    pub top_speed_kmh: [f32; 2],
    /// Skidpad lateral g for an empty / full cornering bar.
    pub cornering_g: [f32; 2],
    /// Skidpad radius of the cornering measurement, m.
    pub skidpad_radius_m: f32,
}

impl Scales {
    /// The scales `wp_vehicle::stats` uses now.
    pub fn current() -> Self {
        Self {
            zero_to_100_s: [stats::ACCEL_EMPTY_S, stats::ACCEL_FULL_S],
            top_speed_kmh: [stats::TOP_SPEED_EMPTY_KMH, stats::TOP_SPEED_FULL_KMH],
            cornering_g: [stats::CORNERING_EMPTY_G, stats::CORNERING_FULL_G],
            skidpad_radius_m: stats::CORNERING_SKIDPAD_RADIUS_M,
        }
    }
}

/// One car's row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CarStatsEntry {
    /// Name shown in menus (`VehicleDef::display_name`).
    pub display_name: String,
    /// 0–100 km/h, s (`null` if not reached within 40 s).
    pub zero_to_100_s: Option<f32>,
    /// Top speed, km/h.
    pub top_speed_kmh: f32,
    /// Skidpad lateral grip, g.
    pub cornering_g: f32,
    /// Bars, 0–1.
    pub bars: Bars,
}

/// The three bars, 0 (empty) – 1 (full).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Bars {
    /// From the 0–100 km/h time.
    pub acceleration: f32,
    /// From the top speed.
    pub top_speed: f32,
    /// From the skidpad grip.
    pub cornering: f32,
}

fn round(v: f32, places: i32) -> f32 {
    let k = 10f32.powi(places);
    (v * k).round() / k
}

impl CarStatsEntry {
    /// A row from a measurement, rounded for the table (values 2 decimals, bars 3).
    pub fn from_stats(display_name: &str, s: &CarStats) -> Self {
        let b = s.bars();
        Self {
            display_name: display_name.to_string(),
            zero_to_100_s: s.zero_to_100_s.map(|t| round(t, 2)),
            top_speed_kmh: round(s.top_speed_kmh, 1),
            cornering_g: round(s.cornering_g, 3),
            bars: Bars {
                acceleration: round(b.acceleration, 3),
                top_speed: round(b.top_speed, 3),
                cornering: round(b.cornering, 3),
            },
        }
    }
}

/// Measure one car from a data source.
pub fn measure_car(src: &dyn DataSource, car_id: &str) -> Result<CarStatsEntry, RunError> {
    let car = load_car(src, car_id)?;
    Ok(CarStatsEntry::from_stats(
        &car.def.display_name,
        &CarStats::measure(&car),
    ))
}

/// Build a table from already-measured rows.
pub fn table(cars: BTreeMap<String, CarStatsEntry>) -> CarStatsTable {
    CarStatsTable {
        scales: Scales::current(),
        cars,
    }
}

/// Parse a table (`data/stats/cars.json`).
pub fn parse(text: &str) -> Result<CarStatsTable, String> {
    serde_json::from_str(text).map_err(|e| format!("{TABLE_PATH}: {e}"))
}

/// Pretty JSON with a trailing newline, as written to `data/stats/cars.json`.
pub fn to_json(t: &CarStatsTable) -> String {
    serde_json::to_string_pretty(t).expect("table serializes") + "\n"
}

/// Differences between a stored table and a fresh one beyond [`TOLERANCE`]
/// (missing or extra cars, changed scales, drifted stats). Empty = current.
pub fn compare(stored: &CarStatsTable, fresh: &CarStatsTable) -> Vec<String> {
    let mut out = Vec::new();
    if stored.scales != fresh.scales {
        out.push(format!(
            "scales changed: table {:?}, wp_vehicle {:?}",
            stored.scales, fresh.scales
        ));
    }
    for id in stored.cars.keys().filter(|k| !fresh.cars.contains_key(*k)) {
        out.push(format!("{id}: in the table but not in data/cars"));
    }
    for (id, f) in &fresh.cars {
        let Some(s) = stored.cars.get(id) else {
            out.push(format!("{id}: missing from the table"));
            continue;
        };
        let (dt, dv, dg) = TOLERANCE;
        let accel_ok = match (s.zero_to_100_s, f.zero_to_100_s) {
            (Some(a), Some(b)) => (a - b).abs() <= dt,
            (None, None) => true,
            _ => false,
        };
        if !accel_ok {
            out.push(format!(
                "{id}: 0-100 {:?} s in the table, {:?} s measured",
                s.zero_to_100_s, f.zero_to_100_s
            ));
        }
        if (s.top_speed_kmh - f.top_speed_kmh).abs() > dv {
            out.push(format!(
                "{id}: top speed {} km/h in the table, {} km/h measured",
                s.top_speed_kmh, f.top_speed_kmh
            ));
        }
        if (s.cornering_g - f.cornering_g).abs() > dg {
            out.push(format!(
                "{id}: cornering {} g in the table, {} g measured",
                s.cornering_g, f.cornering_g
            ));
        }
        if s.display_name != f.display_name {
            out.push(format!(
                "{id}: name {:?} in the table, {:?} in the car file",
                s.display_name, f.display_name
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(t: f32) -> CarStatsEntry {
        CarStatsEntry::from_stats(
            "Test",
            &CarStats {
                zero_to_100_s: Some(t),
                top_speed_kmh: 200.0,
                cornering_g: 0.9,
            },
        )
    }

    #[test]
    fn compare_flags_drift_missing_and_extra_cars() {
        let stored = table([("a".to_string(), entry(8.0)), ("b".to_string(), entry(9.0))].into());
        let same = table(
            [
                ("a".to_string(), entry(8.05)),
                ("b".to_string(), entry(9.0)),
            ]
            .into(),
        );
        assert!(compare(&stored, &same).is_empty());
        let fresh = table([("a".to_string(), entry(8.5)), ("c".to_string(), entry(9.0))].into());
        let diff = compare(&stored, &fresh);
        assert_eq!(diff.len(), 3, "{diff:?}");
    }

    #[test]
    fn table_round_trips() {
        let t = table([("a".to_string(), entry(8.0))].into());
        assert_eq!(parse(&to_json(&t)).unwrap(), t);
    }
}
