//! Per-wheel road conditions under the contact patch (weather hook, K's W-1).
//!
//! [`SurfaceConditions`] come from one of two places ([`ConditionsSource`]):
//! - **World** (the default): every step, each wheel in contact reads
//!   `WorldQuery::weather_at(contact point)` (contract v0.3.0, W-1) and converts it with
//!   [`SurfaceConditions::from_local`]. Worlds without weather answer `LocalConditions::DRY`,
//!   which converts to a neutral dry road, so they stay bit-for-bit as before.
//! - **Manual**: whoever owns the vehicle calls `Vehicle::set_surface_conditions` (tests,
//!   benches, tuning tools); the world's weather is then ignored until
//!   `Vehicle::use_world_weather`.
//!
//! **Painted lines**: a contact on [`SurfaceType::PaintedLine`] (or conditions with
//! `painted_line` set) gets the paint factor below. Its dry tire-vs-surface grip is the tire's
//! `PaintedLine` entry if it has one, else its `Asphalt` entry (`crate::tire::surface_grip`).
//!
//! The tire's grip on a wheel becomes
//!
//! ```text
//! grip = tire.surface_multipliers[surface]            (dry tire-vs-surface, as before)
//!      × conditions_grip(..)                          (this module: wet, cold/ice, paint, aquaplaning)
//!      × damage / puncture / camber terms              (as before)
//! ```
//!
//! **Wet grip** uses the tire's own `WetAsphalt / Asphalt` ratio, so a wet-biased compound keeps
//! its advantage. A water film reaches full wet loss at [`FULL_WET_DEPTH_MM`]; each surface scales
//! the loss ([`wet_loss_scale`]: smooth metal and paint lose more, gravel less).
//! If the base surface already reads `WetAsphalt` (an interim weather adapter), the wet term is
//! skipped so the loss is not counted twice; cold, paint and aquaplaning still apply.
//!
//! **Aquaplaning** starts once standing water is deeper than the tread can clear
//! ([`AQUAPLANE_MIN_DEPTH_MM`]) and the wheel's forward speed passes an onset speed that falls
//! with depth and tire width (≈ 90 km/h for a 205 mm treaded tire in 3 mm of water; slicks ≈ 35 %
//! earlier). Above the onset the tire rides on the water and keeps only a fraction of its grip.

use wp_contracts::{LocalConditions, SurfaceType, TireDef};
use wp_math::smoothstep;

/// Road conditions under one wheel. `Default` = dry, 20 °C, no paint (no effect on grip).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceConditions {
    /// Water on the road surface, mm. ~0.1 mm = damp, 0.5 mm = fully wet,
    /// above [`AQUAPLANE_MIN_DEPTH_MM`] = standing water that can aquaplane a tire.
    pub water_depth_mm: f32,
    /// Road-surface temperature, °C. Below [`COLD_REF_C`] grip falls slowly; at or below
    /// freezing a wet road turns to ice.
    pub temperature_c: f32,
    /// The contact patch is on a painted lane marking / zebra crossing (slippery when wet).
    pub painted_line: bool,
}

impl SurfaceConditions {
    /// Dry road at 20 °C, no paint.
    pub const DRY: Self = Self {
        water_depth_mm: 0.0,
        temperature_c: 20.0,
        painted_line: false,
    };

    /// True when these conditions cannot change grip (fast path; keeps dry runs bit-identical).
    pub fn is_neutral(&self) -> bool {
        self.water_depth_mm <= 0.0 && self.temperature_c >= COLD_REF_C && !self.painted_line
    }

    /// Wetness 0 (dry) … 1 (fully wet) from the water depth.
    pub fn wetness(&self) -> f32 {
        (self.water_depth_mm / FULL_WET_DEPTH_MM).clamp(0.0, 1.0)
    }
}

impl SurfaceConditions {
    /// Convert the world's weather at a contact point (`WorldQuery::weather_at`): water film +
    /// standing water → `water_depth_mm`, road temperature → `temperature_c`. Paint comes from
    /// the contact's surface type, not from the weather, so `painted_line` is false.
    /// `LocalConditions::DRY` converts to a neutral road ([`SurfaceConditions::is_neutral`]).
    pub fn from_local(local: &LocalConditions) -> Self {
        Self {
            water_depth_mm: local.water_mm.max(0.0),
            temperature_c: local.road_temp_c,
            painted_line: false,
        }
    }
}

/// Where a vehicle's [`SurfaceConditions`] come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ConditionsSource {
    /// Read `WorldQuery::weather_at` at each wheel's contact point every step (default).
    #[default]
    World,
    /// Use the values last given to `Vehicle::set_surface_conditions` / `set_wheel_conditions`.
    Manual,
}

impl Default for SurfaceConditions {
    fn default() -> Self {
        Self::DRY
    }
}

/// Water depth at which a surface counts as fully wet, mm.
pub const FULL_WET_DEPTH_MM: f32 = 0.5;
/// Below this road temperature grip starts to fall (cold rubber, cold road), °C.
pub const COLD_REF_C: f32 = 7.0;
/// Grip lost per °C below [`COLD_REF_C`].
pub const COLD_LOSS_PER_C: f32 = 0.006;
/// Floor of the cold-road factor (dry, very cold).
pub const COLD_MIN: f32 = 0.9;
/// Ice is fully formed at this road temperature, °C (none at +1 °C).
pub const ICE_FULL_C: f32 = -1.0;
/// Grip factor on full ice (studless tire).
pub const ICE_FACTOR: f32 = 0.15;
/// Painted line, dry: grip factor relative to the road.
pub const PAINT_DRY_FACTOR: f32 = 0.95;
/// Painted line, fully wet: grip factor relative to the *wet* road.
pub const PAINT_WET_FACTOR: f32 = 0.65;
/// Standing water shallower than this is cleared by the tread: no aquaplaning, mm.
pub const AQUAPLANE_MIN_DEPTH_MM: f32 = 1.0;
/// Reference depth for [`AQUAPLANE_ONSET_TREADED_MPS`], mm.
pub const AQUAPLANE_REF_DEPTH_MM: f32 = 3.0;
/// Reference tire width, m.
pub const AQUAPLANE_REF_WIDTH_M: f32 = 0.205;
/// Onset speed for a treaded 205 mm tire in 3 mm of water, m/s (≈ 90 km/h).
pub const AQUAPLANE_ONSET_TREADED_MPS: f32 = 25.0;
/// Onset speed for a slick / semi-slick at the reference depth and width, m/s (≈ 58 km/h).
pub const AQUAPLANE_ONSET_SLICK_MPS: f32 = 16.0;
/// Fully planing at this multiple of the onset speed.
pub const AQUAPLANE_FULL_RATIO: f32 = 1.3;
/// Depth at which planing reaches its full strength, mm.
pub const AQUAPLANE_FULL_DEPTH_MM: f32 = 6.0;
/// Grip lost when fully planing (some contact remains at the patch edges).
pub const AQUAPLANE_MAX_LOSS: f32 = 0.85;
/// Lower clamp on the conditions multiplier (never frictionless).
pub const MIN_CONDITIONS_GRIP: f32 = 0.05;

/// Tire properties the conditions model needs, cached per wheel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TireWetProfile {
    /// Tread width, m (`WheelDef::width_m`); wider tires plane earlier.
    pub width_m: f32,
    /// The compound's `WetAsphalt / Asphalt` ratio (0..1).
    pub wet_ratio: f32,
    /// Has drainage grooves. Inferred as `wet_ratio >= 0.6` (semi-slicks fall below) until the
    /// contract has a tread field.
    pub treaded: bool,
}

impl TireWetProfile {
    /// Derive from the tire definition and the wheel's width.
    pub fn new(def: &TireDef, width_m: f32) -> Self {
        let get = |s| def.surface_multipliers.get(&s).copied().unwrap_or(1.0);
        let wet_ratio =
            (get(SurfaceType::WetAsphalt) / get(SurfaceType::Asphalt).max(1e-3)).clamp(0.0, 1.0);
        Self {
            width_m,
            wet_ratio,
            treaded: wet_ratio >= 0.6,
        }
    }
}

/// How strongly a surface's grip reacts to water, relative to asphalt (the tire's own wet ratio
/// is measured on asphalt).
pub fn wet_loss_scale(surface: SurfaceType) -> f32 {
    match surface {
        SurfaceType::Asphalt | SurfaceType::WetAsphalt | SurfaceType::PaintedLine => 1.0,
        SurfaceType::Concrete => 1.1,
        SurfaceType::Curb => 1.3,
        SurfaceType::Metal => 1.6,
        SurfaceType::Gravel => 0.4,
        SurfaceType::Dirt => 1.2,
        SurfaceType::Grass => 1.3,
        #[allow(unreachable_patterns)]
        _ => 1.0,
    }
}

/// Fast-rising wetness response: a thin damp film already costs most of the wet loss.
fn wet_shape(wetness: f32) -> f32 {
    let w = wetness.clamp(0.0, 1.0);
    w * (2.0 - w)
}

/// Cold-road and ice factor (1.0 at or above [`COLD_REF_C`]). Ice needs water on the road.
pub fn temperature_factor(temperature_c: f32, wetness: f32) -> f32 {
    if temperature_c >= COLD_REF_C {
        return 1.0;
    }
    let cold = (1.0 - COLD_LOSS_PER_C * (COLD_REF_C - temperature_c)).max(COLD_MIN);
    let freeze = 1.0 - smoothstep(ICE_FULL_C, ICE_FULL_C + 2.0, temperature_c);
    let ice = freeze * smoothstep(0.05, 0.5, wetness);
    cold * (1.0 + (ICE_FACTOR - 1.0) * ice)
}

/// Aquaplaning onset speed, m/s, for standing water `depth_mm` and a tire; `f32::INFINITY` when
/// the water is too shallow to plane. Falls with depth and width.
pub fn aquaplane_onset_mps(depth_mm: f32, tire: &TireWetProfile) -> f32 {
    if depth_mm < AQUAPLANE_MIN_DEPTH_MM {
        return f32::INFINITY;
    }
    let base = if tire.treaded {
        AQUAPLANE_ONSET_TREADED_MPS
    } else {
        AQUAPLANE_ONSET_SLICK_MPS
    };
    let depth = (AQUAPLANE_REF_DEPTH_MM / depth_mm).sqrt();
    let width = (AQUAPLANE_REF_WIDTH_M / tire.width_m.max(0.05)).sqrt();
    (base * depth * width).clamp(10.0, 60.0)
}

/// How far into aquaplaning a wheel is: 0 (full contact) … 1 (fully planing). Non-decreasing in
/// depth and speed.
pub fn aquaplane_amount(depth_mm: f32, speed_mps: f32, tire: &TireWetProfile) -> f32 {
    let onset = aquaplane_onset_mps(depth_mm, tire);
    if !onset.is_finite() {
        return 0.0;
    }
    let speed = smoothstep(onset, onset * AQUAPLANE_FULL_RATIO, speed_mps.abs());
    let depth = smoothstep(AQUAPLANE_MIN_DEPTH_MM, AQUAPLANE_FULL_DEPTH_MM, depth_mm);
    speed * (0.25 + 0.75 * depth)
}

/// The per-wheel conditions grip multiplier and the aquaplaning amount `(grip, planing)`.
/// Exactly `(1.0, 0.0)` for neutral conditions off paint. `speed_mps` = the wheel's forward
/// speed. Paint applies when `cond.painted_line` is set or `surface` is
/// [`SurfaceType::PaintedLine`].
pub fn conditions_grip(
    cond: &SurfaceConditions,
    surface: SurfaceType,
    speed_mps: f32,
    tire: &TireWetProfile,
) -> (f32, f32) {
    let on_paint = cond.painted_line || surface == SurfaceType::PaintedLine;
    if cond.is_neutral() && !on_paint {
        return (1.0, 0.0);
    }
    let wetness = cond.wetness();
    let shape = wet_shape(wetness);
    let wet = if surface == SurfaceType::WetAsphalt {
        // The tire's surface table already applied its wet multiplier.
        1.0
    } else {
        let loss = ((1.0 - tire.wet_ratio) * wet_loss_scale(surface)).clamp(0.0, 0.95);
        1.0 - loss * shape
    };
    let paint = if on_paint {
        wp_math::lerp(PAINT_DRY_FACTOR, PAINT_WET_FACTOR, shape)
    } else {
        1.0
    };
    let planing = aquaplane_amount(cond.water_depth_mm, speed_mps, tire);
    let grip = wet
        * paint
        * temperature_factor(cond.temperature_c, wetness)
        * (1.0 - AQUAPLANE_MAX_LOSS * planing);
    (grip.clamp(MIN_CONDITIONS_GRIP, 1.0), planing)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn street() -> TireWetProfile {
        TireWetProfile {
            width_m: 0.205,
            wet_ratio: 0.7,
            treaded: true,
        }
    }

    fn wet(depth: f32) -> SurfaceConditions {
        SurfaceConditions {
            water_depth_mm: depth,
            ..SurfaceConditions::DRY
        }
    }

    #[test]
    fn dry_default_is_exactly_neutral() {
        let c = SurfaceConditions::default();
        assert!(c.is_neutral());
        for s in [
            SurfaceType::Asphalt,
            SurfaceType::Gravel,
            SurfaceType::Metal,
        ] {
            assert_eq!(conditions_grip(&c, s, 80.0, &street()), (1.0, 0.0));
        }
    }

    #[test]
    fn wet_asphalt_matches_the_tires_wet_ratio() {
        let (g, p) = conditions_grip(&wet(0.8), SurfaceType::Asphalt, 20.0, &street());
        assert!((g - 0.7).abs() < 1e-4, "{g}");
        assert_eq!(p, 0.0);
        // Damp is between dry and wet, and already most of the way.
        let (damp, _) = conditions_grip(&wet(0.15), SurfaceType::Asphalt, 20.0, &street());
        assert!(damp > 0.7 && damp < 0.85, "{damp}");
        // A wet-biased tire loses less.
        let rain = TireWetProfile {
            wet_ratio: 0.85,
            ..street()
        };
        assert!(conditions_grip(&wet(0.8), SurfaceType::Asphalt, 20.0, &rain).0 > g);
        // No double counting on a surface that already reads wet.
        let (g2, _) = conditions_grip(&wet(0.8), SurfaceType::WetAsphalt, 20.0, &street());
        assert_eq!(g2, 1.0);
    }

    #[test]
    fn paint_is_slippery_when_wet() {
        let line = |d| SurfaceConditions {
            painted_line: true,
            ..wet(d)
        };
        let dry_line = conditions_grip(&line(0.0), SurfaceType::Asphalt, 20.0, &street()).0;
        assert!((dry_line - PAINT_DRY_FACTOR).abs() < 1e-6);
        let wet_road = conditions_grip(&wet(0.8), SurfaceType::Asphalt, 20.0, &street()).0;
        let wet_line = conditions_grip(&line(0.8), SurfaceType::Asphalt, 20.0, &street()).0;
        assert!((wet_line / wet_road - PAINT_WET_FACTOR).abs() < 1e-4);
    }

    #[test]
    fn painted_line_surface_counts_as_paint() {
        let t = street();
        let dry = conditions_grip(&SurfaceConditions::DRY, SurfaceType::PaintedLine, 20.0, &t).0;
        assert!((dry - PAINT_DRY_FACTOR).abs() < 1e-6, "{dry}");
        // Wet paint surface == wet asphalt with the painted flag (no double paint factor).
        let flagged = SurfaceConditions {
            painted_line: true,
            ..wet(0.8)
        };
        let a = conditions_grip(&wet(0.8), SurfaceType::PaintedLine, 20.0, &t).0;
        let b = conditions_grip(&flagged, SurfaceType::Asphalt, 20.0, &t).0;
        let c = conditions_grip(&flagged, SurfaceType::PaintedLine, 20.0, &t).0;
        assert!((a - b).abs() < 1e-6 && (a - c).abs() < 1e-6, "{a} {b} {c}");
        assert!(a < 0.5, "wet paint is slippery: {a}");
    }

    #[test]
    fn from_local_dry_is_neutral_and_maps_water_and_temperature() {
        assert!(SurfaceConditions::from_local(&LocalConditions::DRY).is_neutral());
        let rain = LocalConditions {
            water_mm: 2.0,
            wetness: 1.0,
            standing_water_mm: 1.5,
            road_temp_c: 4.0,
            ..LocalConditions::DRY
        };
        let c = SurfaceConditions::from_local(&rain);
        assert_eq!(c.water_depth_mm, 2.0);
        assert_eq!(c.temperature_c, 4.0);
        assert!(!c.painted_line);
        assert_eq!(c.wetness(), 1.0);
    }

    #[test]
    fn cold_and_ice() {
        let cold_dry = SurfaceConditions {
            temperature_c: -10.0,
            ..SurfaceConditions::DRY
        };
        let g = conditions_grip(&cold_dry, SurfaceType::Asphalt, 20.0, &street()).0;
        assert!((g - COLD_MIN).abs() < 1e-6, "dry cold road, no ice: {g}");
        let icy = SurfaceConditions {
            temperature_c: -3.0,
            ..wet(0.6)
        };
        let g = conditions_grip(&icy, SurfaceType::Asphalt, 20.0, &street()).0;
        assert!(g < 0.15, "black ice: {g}");
        let thaw = SurfaceConditions {
            temperature_c: 3.0,
            ..wet(0.6)
        };
        assert!(conditions_grip(&thaw, SurfaceType::Asphalt, 20.0, &street()).0 > 0.6);
    }

    #[test]
    fn aquaplaning_onset_depends_on_depth_width_and_tread() {
        let t = street();
        assert!(aquaplane_onset_mps(0.8, &t).is_infinite());
        let v3 = aquaplane_onset_mps(3.0, &t);
        assert!((v3 * 3.6 - 90.0).abs() < 1.0, "{} km/h", v3 * 3.6);
        assert!(aquaplane_onset_mps(6.0, &t) < v3);
        let wide = TireWetProfile {
            width_m: 0.285,
            ..t
        };
        assert!(aquaplane_onset_mps(3.0, &wide) < v3);
        let slick = TireWetProfile {
            wet_ratio: 0.5,
            treaded: false,
            ..t
        };
        assert!(aquaplane_onset_mps(3.0, &slick) < v3);
        // Below onset: no planing; well above: most of the grip gone.
        assert_eq!(aquaplane_amount(3.0, 0.9 * v3, &t), 0.0);
        let (g, p) = conditions_grip(&wet(6.0), SurfaceType::Asphalt, 40.0, &t);
        assert!(p > 0.99 && g < 0.15, "{g} {p}");
        // Monotonic in speed.
        let mut last = 0.0;
        for i in 0..60 {
            let a = aquaplane_amount(4.0, i as f32, &t);
            assert!(a >= last);
            last = a;
        }
    }

    #[test]
    fn profile_from_tire_def() {
        let p = concat!(env!("CARGO_MANIFEST_DIR"), "/../../data/tires/street.json");
        let def = wp_contracts::load_tire_def(p).unwrap();
        let prof = TireWetProfile::new(&def, 0.205);
        assert!((prof.wet_ratio - 0.7).abs() < 1e-6 && prof.treaded);
    }
}
