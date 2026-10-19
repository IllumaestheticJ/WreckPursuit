//! Weather grip: wet roads, cold and icy roads, painted markings and aquaplaning.
//!
//! All functions return a **weather multiplier** that is 1.0 on a dry road at mild temperature.
//! `wp_vehicle` multiplies it onto the tire's own dry `surface_multipliers[surface]`:
//!
//! ```text
//! grip = tire.surface_multipliers[surface]                        (dry tire-vs-surface)
//!      × table.wheel_grip_multiplier(&local, surface, on_marking, speed_mps, &profile)
//! ```
//!
//! The per-surface `wet_factor`s are for the reference street tire. A tire whose own
//! `WetAsphalt/Asphalt` ratio is better or worse scales the wet *loss* proportionally, so a
//! wet-biased compound keeps its advantage on every wet surface, not only asphalt.

use wp_contracts::{SurfaceType, TireDef};
use wp_math::smoothstep;

use crate::conditions::LocalConditions;
use crate::data::SurfaceTable;

/// Tire properties the weather model needs. Width comes from `WheelDef::width_m`; the tread
/// flag is inferred from the [`TireDef`] until the contract has one (request W-4 in
/// `docs/integration/weather.md`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TireProfile {
    /// Tread width, m (wider tires aquaplane earlier).
    pub width_m: f32,
    /// Has drainage grooves (slicks aquaplane much earlier).
    pub treaded: bool,
    /// This compound's `WetAsphalt / Asphalt` grip ratio (0..1).
    pub wet_ratio: f32,
}

impl Default for TireProfile {
    /// A 205 mm treaded street tire.
    fn default() -> Self {
        Self {
            width_m: 0.205,
            treaded: true,
            wet_ratio: 0.70,
        }
    }
}

impl TireProfile {
    /// Derive a profile from a tire definition and the wheel's `WheelDef::width_m`. `wet_ratio` comes from the tire's surface table
    /// (`WetAsphalt / Asphalt`, missing entries = 1.0 as in `wp_vehicle`); `treaded` is inferred
    /// as `wet_ratio >= 0.6` (semi-slicks and slicks fall below) until the contract has a tread
    /// field.
    pub fn from_tire_def(def: &TireDef, width_m: f32) -> Self {
        let get = |s| def.surface_multipliers.get(&s).copied().unwrap_or(1.0);
        let dry = get(SurfaceType::Asphalt).max(1e-3);
        let wet_ratio = (get(SurfaceType::WetAsphalt) / dry).clamp(0.0, 1.0);
        Self {
            width_m,
            treaded: wet_ratio >= 0.6,
            wet_ratio,
        }
    }
}

/// Lower clamp on any weather grip multiplier (never a frictionless road).
pub const MIN_GRIP_MULT: f32 = 0.05;

/// Fast-rising wetness response: a thin damp film already costs most of the grip.
fn wet_shape(wetness: f32) -> f32 {
    let w = wetness.clamp(0.0, 1.0);
    w * (2.0 - w)
}

impl SurfaceTable {
    /// Weather grip multiplier for a surface at a wetness (0..1) and road temperature (°C), for
    /// the reference street tire. Includes the cold-road and black-ice terms; excludes
    /// aquaplaning (speed dependent, see [`SurfaceTable::aquaplane_grip_multiplier`]).
    pub fn surface_grip_multiplier(
        &self,
        surface: SurfaceType,
        wetness: f32,
        road_temp_c: f32,
    ) -> f32 {
        self.surface_grip_multiplier_for(surface, wetness, road_temp_c, self.reference_wet_ratio)
    }

    /// [`SurfaceTable::surface_grip_multiplier`] for a tire with its own `wet_ratio`.
    pub fn surface_grip_multiplier_for(
        &self,
        surface: SurfaceType,
        wetness: f32,
        road_temp_c: f32,
        tire_wet_ratio: f32,
    ) -> f32 {
        let wet_factor = self.surface(surface).wet_factor;
        self.compose(wet_factor, 1.0, wetness, road_temp_c, tire_wet_ratio)
    }

    /// Weather grip multiplier on a painted lane marking / zebra crossing (no `SurfaceType` for
    /// it yet, request W-3).
    pub fn marking_grip_multiplier(&self, wetness: f32, road_temp_c: f32) -> f32 {
        self.compose(
            self.marking_wet_factor,
            self.marking_dry_factor,
            wetness,
            road_temp_c,
            self.reference_wet_ratio,
        )
    }

    fn compose(
        &self,
        wet_factor: f32,
        dry_factor: f32,
        wetness: f32,
        road_temp_c: f32,
        tire_wet_ratio: f32,
    ) -> f32 {
        // Scale the wet loss by how this tire compares with the reference tire.
        let ref_loss = (1.0 - self.reference_wet_ratio).max(1e-3);
        let tire_scale = (1.0 - tire_wet_ratio.clamp(0.0, 1.0)) / ref_loss;
        let wet_loss = ((1.0 - wet_factor) * tire_scale).clamp(0.0, 1.0);
        let wet = dry_factor * (1.0 - wet_loss * wet_shape(wetness));
        (wet * self.temperature_factor(wetness, road_temp_c)).clamp(MIN_GRIP_MULT, 1.0)
    }

    /// Cold-road and black-ice multiplier (1.0 at or above `cold_ref_c`).
    pub fn temperature_factor(&self, wetness: f32, road_temp_c: f32) -> f32 {
        let cold = (1.0 - self.cold_loss_per_c * (self.cold_ref_c - road_temp_c).max(0.0))
            .max(self.cold_min);
        // Ice: full at ice_temp_c, none at ice_temp_c + ice_band_c; needs a wet road.
        let freeze = 1.0
            - smoothstep(
                self.ice_temp_c,
                self.ice_temp_c + self.ice_band_c.max(1e-3),
                road_temp_c,
            );
        let wet_enough = smoothstep(self.ice_min_wetness, 1.0, wetness);
        let ice = freeze * wet_enough;
        cold * (1.0 + (self.ice_factor - 1.0) * ice)
    }

    /// Aquaplaning onset speed, m/s, for a standing-water depth (mm) and tire. `f32::INFINITY`
    /// below `aquaplane.min_depth_mm`. Decreases with depth and with tire width.
    pub fn aquaplane_onset_mps(&self, standing_mm: f32, tire: &TireProfile) -> f32 {
        let a = &self.aquaplane;
        if standing_mm < a.min_depth_mm {
            return f32::INFINITY;
        }
        let base = if tire.treaded {
            a.onset_treaded_mps
        } else {
            a.onset_slick_mps
        };
        let depth = (a.ref_depth_mm / standing_mm).sqrt();
        let width = (a.ref_width_m / tire.width_m.max(0.05)).sqrt();
        (base * depth * width).clamp(a.onset_min_mps, a.onset_max_mps)
    }

    /// How far into aquaplaning a tire is, 0 (full contact) … 1 (fully planing). Non-decreasing
    /// in both water depth and speed.
    pub fn aquaplane_amount(&self, standing_mm: f32, speed_mps: f32, tire: &TireProfile) -> f32 {
        let a = &self.aquaplane;
        let onset = self.aquaplane_onset_mps(standing_mm, tire);
        if !onset.is_finite() {
            return 0.0;
        }
        let speed = smoothstep(onset, onset * a.full_at_onset_ratio, speed_mps.abs());
        let depth = smoothstep(a.min_depth_mm, a.full_loss_depth_mm, standing_mm);
        // A just-over-threshold film still planes a little once fast enough.
        speed * (0.25 + 0.75 * depth)
    }

    /// Grip multiplier from aquaplaning alone (1.0 = no effect).
    pub fn aquaplane_grip_multiplier(
        &self,
        standing_mm: f32,
        speed_mps: f32,
        tire: &TireProfile,
    ) -> f32 {
        1.0 - self.aquaplane.max_grip_loss * self.aquaplane_amount(standing_mm, speed_mps, tire)
    }

    /// The full per-wheel weather multiplier: wet + temperature + marking + aquaplaning. This is
    /// the one call `wp_vehicle` makes per wheel contact (see the module docs).
    pub fn wheel_grip_multiplier(
        &self,
        local: &LocalConditions,
        surface: SurfaceType,
        on_marking: bool,
        speed_mps: f32,
        tire: &TireProfile,
    ) -> f32 {
        let base = if on_marking {
            let m = self.marking_grip_multiplier(local.wetness, local.road_temp_c);
            // Markings are reference-tire numbers; apply the tire's relative wet behaviour via
            // the asphalt ratio.
            let ref_asphalt = self.surface_grip_multiplier(
                SurfaceType::Asphalt,
                local.wetness,
                local.road_temp_c,
            );
            let tire_asphalt = self.surface_grip_multiplier_for(
                SurfaceType::Asphalt,
                local.wetness,
                local.road_temp_c,
                tire.wet_ratio,
            );
            m * tire_asphalt / ref_asphalt.max(MIN_GRIP_MULT)
        } else {
            self.surface_grip_multiplier_for(
                surface,
                local.wetness,
                local.road_temp_c,
                tire.wet_ratio,
            )
        };
        let aqua = self.aquaplane_grip_multiplier(local.standing_water_mm, speed_mps, tire);
        (base * aqua).clamp(MIN_GRIP_MULT, 1.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::{WeatherData, ALL_SURFACES};

    fn t() -> SurfaceTable {
        WeatherData::embedded().surfaces
    }

    #[test]
    fn dry_mild_road_is_neutral() {
        let t = t();
        for s in ALL_SURFACES {
            assert_eq!(t.surface_grip_multiplier(s, 0.0, 20.0), 1.0, "{s:?}");
        }
    }

    #[test]
    fn wet_asphalt_is_about_070_and_markings_metal_are_much_lower() {
        let t = t();
        let asphalt = t.surface_grip_multiplier(SurfaceType::Asphalt, 1.0, 20.0);
        assert!((asphalt - 0.70).abs() < 1e-4, "{asphalt}");
        let metal = t.surface_grip_multiplier(SurfaceType::Metal, 1.0, 20.0);
        let marking = t.marking_grip_multiplier(1.0, 20.0);
        assert!(metal < asphalt - 0.2, "{metal}");
        assert!(marking < asphalt - 0.15, "{marking}");
        // Surfaces already wet in the tire table get no extra wet loss.
        assert_eq!(
            t.surface_grip_multiplier(SurfaceType::WetAsphalt, 1.0, 20.0),
            1.0
        );
    }

    #[test]
    fn grip_falls_monotonically_with_wetness() {
        let t = t();
        for s in ALL_SURFACES {
            let mut prev = f32::INFINITY;
            for i in 0..=20 {
                let g = t.surface_grip_multiplier(s, i as f32 / 20.0, 15.0);
                assert!(g <= prev + 1e-6, "{s:?}");
                prev = g;
            }
        }
    }

    #[test]
    fn freezing_wet_road_is_ice_and_freezing_dry_road_is_only_cold() {
        let t = t();
        let ice = t.surface_grip_multiplier(SurfaceType::Asphalt, 1.0, -2.0);
        let cold_dry = t.surface_grip_multiplier(SurfaceType::Asphalt, 0.0, -2.0);
        assert!(ice < 0.2, "{ice}");
        assert!((0.9..1.0).contains(&cold_dry), "{cold_dry}");
        let wet_warm = t.surface_grip_multiplier(SurfaceType::Asphalt, 1.0, 12.0);
        assert!(ice < wet_warm);
    }

    #[test]
    fn wet_compound_ratio_scales_loss() {
        let t = t();
        let street = t.surface_grip_multiplier_for(SurfaceType::Metal, 1.0, 20.0, 0.70);
        let wet = t.surface_grip_multiplier_for(SurfaceType::Metal, 1.0, 20.0, 0.86);
        let slick = t.surface_grip_multiplier_for(SurfaceType::Metal, 1.0, 20.0, 0.50);
        assert!(slick < street && street < wet);
        let a = t.surface_grip_multiplier_for(SurfaceType::Asphalt, 1.0, 20.0, 0.86);
        assert!((a - 0.86).abs() < 1e-4);
    }

    #[test]
    fn aquaplaning_increases_with_water_and_speed() {
        let t = t();
        let tire = TireProfile::default();
        // Monotone in depth at fixed speed and in speed at fixed depth.
        for v in [10.0, 20.0, 25.0, 30.0, 40.0] {
            let mut prev = 0.0;
            for i in 0..=40 {
                let d = i as f32 * 0.25;
                let a = t.aquaplane_amount(d, v, &tire);
                assert!(a + 1e-6 >= prev, "depth {d} speed {v}");
                prev = a;
            }
        }
        for d in [0.6, 1.0, 3.0, 6.0, 9.0] {
            let mut prev = 0.0;
            for i in 0..=60 {
                let v = i as f32;
                let a = t.aquaplane_amount(d, v, &tire);
                assert!(a + 1e-6 >= prev, "depth {d} speed {v}");
                prev = a;
            }
        }
        // Onset speed falls as water deepens.
        let mut prev = f32::INFINITY;
        for i in 2..=40 {
            let d = i as f32 * 0.25;
            let o = t.aquaplane_onset_mps(d, &tire);
            assert!(o <= prev);
            prev = o;
        }
        // No aquaplaning on a damp film or at walking pace.
        assert_eq!(t.aquaplane_amount(0.3, 50.0, &tire), 0.0);
        assert_eq!(t.aquaplane_amount(6.0, 5.0, &tire), 0.0);
        // Deep water at motorway speed planes heavily.
        assert!(t.aquaplane_amount(6.0, 33.0, &tire) > 0.9);
    }

    #[test]
    fn slicks_and_wide_tires_plane_earlier() {
        let t = t();
        let street = TireProfile::default();
        let slick = TireProfile {
            treaded: false,
            ..street
        };
        let wide = TireProfile {
            width_m: 0.305,
            ..street
        };
        let o = t.aquaplane_onset_mps(3.0, &street);
        assert!((o - 24.0).abs() < 1e-3, "{o}");
        assert!(t.aquaplane_onset_mps(3.0, &slick) < o);
        assert!(t.aquaplane_onset_mps(3.0, &wide) < o);
    }

    #[test]
    fn wheel_multiplier_composes_and_is_bounded() {
        let t = t();
        let tire = TireProfile::default();
        let mut c = LocalConditions::DRY;
        assert_eq!(
            t.wheel_grip_multiplier(&c, SurfaceType::Asphalt, false, 30.0, &tire),
            1.0
        );
        c.wetness = 1.0;
        c.standing_water_mm = 6.0;
        let slow = t.wheel_grip_multiplier(&c, SurfaceType::Asphalt, false, 5.0, &tire);
        let fast = t.wheel_grip_multiplier(&c, SurfaceType::Asphalt, false, 35.0, &tire);
        assert!((slow - 0.70).abs() < 1e-4);
        assert!((MIN_GRIP_MULT..0.2).contains(&fast));
        let line = t.wheel_grip_multiplier(&c, SurfaceType::Asphalt, true, 5.0, &tire);
        assert!(line < slow);
    }
}
