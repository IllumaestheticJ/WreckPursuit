//! Civilian density model.
//!
//! `target(cell) = Σ_zone lane_km(cell) · density_base(zone) · f_distance(d_core) ·
//! f_time(time, district_kind)`, then scaled so the whole active area never exceeds the agent
//! cap. `f_time` is a piecewise-linear hourly curve per district kind (commute bumps for
//! Residential/Commercial, shift peaks for Industrial, a deep 01:00–05:00 trough everywhere,
//! Core keeps nightlife density until ~02:00).

use wp_contracts::TimeOfDay;

use crate::network::DistrictKind;

/// Hourly relative density (00:00 … 23:00) per district kind, peak ≈ 1.
fn curve(kind: DistrictKind) -> &'static [f32; 24] {
    match kind {
        DistrictKind::Core => &[
            0.55, 0.45, 0.35, 0.10, 0.08, 0.12, 0.30, 0.60, 0.85, 0.80, 0.80, 0.90, 1.00, 0.95,
            0.85, 0.85, 0.90, 1.00, 0.95, 0.85, 0.80, 0.75, 0.70, 0.65,
        ],
        DistrictKind::Commercial => &[
            0.20, 0.12, 0.08, 0.06, 0.06, 0.10, 0.30, 0.70, 1.00, 0.85, 0.75, 0.80, 0.90, 0.85,
            0.75, 0.80, 0.90, 1.00, 0.80, 0.60, 0.45, 0.35, 0.30, 0.25,
        ],
        DistrictKind::Residential => &[
            0.15, 0.08, 0.05, 0.04, 0.05, 0.12, 0.45, 0.90, 1.00, 0.60, 0.45, 0.45, 0.50, 0.45,
            0.45, 0.60, 0.85, 1.00, 0.90, 0.60, 0.45, 0.35, 0.28, 0.20,
        ],
        DistrictKind::Industrial => &[
            0.20, 0.15, 0.12, 0.10, 0.12, 0.50, 0.90, 0.80, 0.60, 0.60, 0.60, 0.60, 0.60, 0.80,
            0.90, 0.60, 0.55, 0.50, 0.40, 0.35, 0.40, 0.70, 0.60, 0.30,
        ],
        DistrictKind::Port => &[
            0.30, 0.25, 0.20, 0.20, 0.25, 0.40, 0.70, 0.85, 0.90, 0.90, 0.85, 0.85, 0.85, 0.85,
            0.85, 0.85, 0.85, 0.80, 0.70, 0.60, 0.50, 0.45, 0.40, 0.35,
        ],
        DistrictKind::Suburb => &[
            0.12, 0.06, 0.04, 0.03, 0.05, 0.15, 0.50, 0.95, 1.00, 0.55, 0.40, 0.40, 0.45, 0.40,
            0.40, 0.55, 0.85, 1.00, 0.85, 0.55, 0.40, 0.30, 0.22, 0.16,
        ],
    }
}

/// Time-of-day factor for a district kind (piecewise linear between hourly samples).
pub fn f_time(t: TimeOfDay, kind: DistrictKind) -> f32 {
    let h = t.hours();
    let i = (h.floor() as usize) % 24;
    let frac = h - h.floor();
    let c = curve(kind);
    c[i] + (c[(i + 1) % 24] - c[i]) * frac
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

    #[test]
    fn noon_core_vs_3am_core() {
        let noon = f_time(TimeOfDay::from_hours(12.0), DistrictKind::Core);
        let night = f_time(TimeOfDay::from_hours(3.0), DistrictKind::Core);
        assert!(noon / night >= 5.0, "{noon} / {night}");
    }

    #[test]
    fn commute_bumps_and_trough() {
        let r = |h| f_time(TimeOfDay::from_hours(h), DistrictKind::Residential);
        assert!(r(8.0) > r(11.0) * 1.5 && r(17.0) > r(11.0) * 1.5);
        for k in DistrictKind::ALL {
            for h in [1.5f32, 3.0, 4.5] {
                let v = f_time(TimeOfDay::from_hours(h), k);
                let day = f_time(TimeOfDay::from_hours(12.0), k);
                assert!(v < day * 0.6, "{k:?} at {h}");
            }
        }
        // Core nightlife holds until ~02:00.
        let c = |h| f_time(TimeOfDay::from_hours(h), DistrictKind::Core);
        assert!(c(1.0) > 0.4 && c(3.5) < 0.1);
    }

    #[test]
    fn distance_falloff() {
        assert_eq!(f_distance(0.0, 600.0), 1.0);
        assert!(f_distance(600.0, 600.0) - 0.5 < 1e-6);
        assert_eq!(f_distance(1e5, 600.0), 0.2);
    }
}
