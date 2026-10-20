//! Civilian density model.
//!
//! `target(cell) = Σ_zone lane_km(cell) · density_base(zone) · f_distance(d_core) ·
//! f_time(time, district_kind)`, then scaled so the whole active area never exceeds the agent
//! cap. `f_time` comes from the data table `data/city/traffic_time.json` ([`TimeCurve`],
//! embedded at build time): one hourly curve relative to the daytime peak (07–09 and 16–19 =
//! 1, midday ≈ 0.75, 19–22 ≈ 0.5–0.6, 23:00 ≈ 0.33, 01–05 ≈ 0.15–0.2) and an hourly multiplier
//! per district (downtown nightlife until ~02:00, industrial / port / suburbs emptier at night,
//! all districts equal by day). Owner playtest row 76: the old per-district curves kept too
//! much traffic at night.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use serde::Deserialize;
use wp_contracts::TimeOfDay;

use crate::network::DistrictKind;

/// The built-in time-of-day table (`data/city/traffic_time.json`).
pub const BUILTIN_TIME_CURVE: &str = include_str!("../../../data/city/traffic_time.json");

/// Time-of-day density table: see the module docs and `data/city/traffic_time.json`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TimeCurve {
    /// Density at each full hour (00:00 … 23:00) relative to the daytime peak.
    pub hourly: [f32; 24],
    /// Multiplier on `hourly` per district kind at each full hour (1 = follows the curve).
    /// Missing kinds use 1.
    pub district_mul: BTreeMap<DistrictKind, [f32; 24]>,
}

impl TimeCurve {
    /// Parse a table from JSON (unknown keys such as `_doc` are ignored).
    pub fn from_json(s: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(s)
    }

    /// The built-in table (parsed once).
    pub fn builtin() -> &'static TimeCurve {
        static CURVE: OnceLock<TimeCurve> = OnceLock::new();
        CURVE.get_or_init(|| {
            TimeCurve::from_json(BUILTIN_TIME_CURVE).expect("data/city/traffic_time.json")
        })
    }

    /// Density factor at `t` for a district kind, 0..1 (linear between hourly samples).
    pub fn at(&self, t: TimeOfDay, kind: DistrictKind) -> f32 {
        let h = t.hours();
        let i = (h.floor() as usize) % 24;
        let j = (i + 1) % 24;
        let frac = h - h.floor();
        let lerp = |c: &[f32; 24]| c[i] + (c[j] - c[i]) * frac;
        let mul = self.district_mul.get(&kind).map_or(1.0, lerp);
        let v = lerp(&self.hourly) * mul;
        v.clamp(0.0, 1.0)
    }
}

/// Time-of-day factor for a district kind from the built-in [`TimeCurve`].
pub fn f_time(t: TimeOfDay, kind: DistrictKind) -> f32 {
    TimeCurve::builtin().at(t, kind)
}

/// Distance-from-core factor: `max(0.2, 1 / (1 + (d / r)²))` with `r` the falloff radius.
pub fn f_distance(d_core_m: f32, falloff_m: f32) -> f32 {
    let x = d_core_m / falloff_m.max(1.0);
    (1.0 / (1.0 + x * x)).max(0.2)
}

/// Default peak density for a district kind, vehicles per lane-km.
pub fn default_density_base(kind: DistrictKind) -> f32 {
    match kind {
        DistrictKind::Core => 30.0,
        DistrictKind::Commercial => 22.0,
        DistrictKind::Residential => 12.0,
        DistrictKind::Industrial => 8.0,
        DistrictKind::Port => 6.0,
        DistrictKind::Suburb => 5.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(h: f32, k: DistrictKind) -> f32 {
        f_time(TimeOfDay::from_hours(h), k)
    }

    #[test]
    fn builtin_table_parses() {
        let c = TimeCurve::builtin();
        assert_eq!(c.district_mul.len(), DistrictKind::ALL.len());
    }

    /// Owner row 76 bands (relative to the daytime peak) for a district that follows the curve.
    #[test]
    fn owner_bands() {
        let k = DistrictKind::Commercial;
        for h in [1.0f32, 2.0, 3.0, 4.0, 5.0] {
            let v = at(h, k);
            assert!((0.14..=0.21).contains(&v), "{h}: {v}");
        }
        assert!((0.3..=0.4).contains(&at(23.0, k)));
        for h in [19.5f32, 20.0, 21.0, 22.0] {
            let v = at(h, k);
            assert!((0.5..=0.7).contains(&v), "{h}: {v}");
        }
        for h in [7.0f32, 8.0, 9.0, 16.0, 17.0, 18.0] {
            assert_eq!(at(h, k), 1.0, "{h}");
        }
        assert!((0.7..=0.8).contains(&at(12.0, k)));
    }

    #[test]
    fn districts_at_night() {
        use DistrictKind::*;
        for h in [0.0f32, 1.0, 2.0, 22.0, 23.0] {
            assert!(at(h, Core) > at(h, Commercial), "{h}");
        }
        for h in [0.0f32, 2.0, 3.5, 23.0] {
            assert!(at(h, Industrial) < at(h, Commercial), "{h}");
            assert!(at(h, Suburb) < at(h, Commercial), "{h}");
        }
        // By day every district follows the curve.
        for k in DistrictKind::ALL {
            assert_eq!(at(12.0, k), at(12.0, Commercial));
        }
        // Downtown nightlife holds until ~02:00, then even downtown drops into the trough
        // (prompt 05: noon core ≥ 5× 03:00 core).
        assert!(at(1.0, Core) >= 0.25);
        assert!(at(12.0, Core) >= 5.0 * at(3.0, Core));
        for k in DistrictKind::ALL {
            assert!(at(3.0, k) <= at(12.0, k) * 0.2, "{k:?}");
        }
    }

    #[test]
    fn distance_falloff() {
        assert_eq!(f_distance(0.0, 600.0), 1.0);
        assert!(f_distance(600.0, 600.0) - 0.5 < 1e-6);
        assert_eq!(f_distance(1e5, 600.0), 0.2);
    }
}
