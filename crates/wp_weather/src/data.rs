//! Data tables from `data/weather/*.toml` and their loaders.
//!
//! The three files are embedded at compile time ([`WeatherData::embedded`]) so the browser build
//! needs no file access; [`WeatherData::load`] reads them from a data root instead (tests check
//! both agree).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Deserialize;
use wp_contracts::{DayPhase, SurfaceType};

use crate::kind::{Season, WeatherKind};

/// A `[at intensity 0, at intensity 1]` pair.
pub type Range2 = [f32; 2];

/// Linear interpolation inside a [`Range2`] (`t` is clamped to 0..1).
pub fn lerp2(r: Range2, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    r[0] + (r[1] - r[0]) * t
}

/// Error loading or validating a weather table.
#[derive(Debug, thiserror::Error)]
pub enum WeatherDataError {
    /// File could not be read.
    #[error("{path}: {source}")]
    Io {
        /// File path.
        path: PathBuf,
        /// Cause.
        source: std::io::Error,
    },
    /// TOML did not parse into the table type.
    #[error("{file}: {source}")]
    Parse {
        /// File name.
        file: String,
        /// Cause.
        source: toml::de::Error,
    },
    /// Parsed but semantically invalid.
    #[error("{file}: {msg}")]
    Invalid {
        /// File name.
        file: String,
        /// What is wrong.
        msg: String,
    },
}

fn invalid(file: &str, msg: impl Into<String>) -> WeatherDataError {
    WeatherDataError::Invalid {
        file: file.to_string(),
        msg: msg.into(),
    }
}

// ---------------------------------------------------------------------------------------------
// climate.toml
// ---------------------------------------------------------------------------------------------

/// Parameters of one regime (`[kinds.<Kind>]` in `climate.toml`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KindParams {
    /// Intensity range drawn uniformly when the regime is entered.
    pub intensity: Range2,
    /// Rain rate, mm/h.
    pub rain_mm_per_h: Range2,
    /// Fog extinction coefficient, 1/m.
    pub fog_per_m: Range2,
    /// Cloud cover 0..1.
    pub cloud_cover: Range2,
    /// Wind speed, m/s.
    pub wind_mps: Range2,
    /// Air temperature offset vs the season curve, °C.
    pub temp_offset_c: f32,
    /// Minimum time the regime is held before the next roll, in-game s.
    pub min_duration_game_s: f32,
}

/// One season (`[seasons.<Season>]`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeasonParams {
    /// Daily mean air temperature, °C.
    pub mean_temp_c: f32,
    /// Half the daily min–max swing, °C (peak at 15:00, minimum at 03:00).
    pub diurnal_amplitude_c: f32,
    /// Markov rows: from-regime → (to-regime → weight). Weights need not sum to 1.
    pub transitions: BTreeMap<WeatherKind, BTreeMap<WeatherKind, f32>>,
    /// Optional season-specific bias on the target regime per day phase (multiplies the global).
    #[serde(default)]
    pub phase_bias: BTreeMap<DayPhase, BTreeMap<WeatherKind, f32>>,
}

/// `data/weather/climate.toml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClimateTable {
    /// Must be `"climate"`.
    pub id: String,
    /// Time between schedule rolls, in-game s.
    pub roll_interval_game_s: f32,
    /// Blend time between regimes, in-game s.
    pub blend_game_s: f32,
    /// Max wind heading change per roll, rad.
    pub wind_heading_step_rad: f32,
    /// Daytime road heating in sun, °C.
    pub road_sun_gain_c: f32,
    /// Night-time radiative road cooling under clear skies, °C.
    pub road_sky_cooling_c: f32,
    /// Per-regime parameters (every regime required).
    pub kinds: BTreeMap<WeatherKind, KindParams>,
    /// Global bias on the target regime per day phase. Missing entries = 1.0.
    #[serde(default)]
    pub phase_bias: BTreeMap<DayPhase, BTreeMap<WeatherKind, f32>>,
    /// Per-season rows (every season required).
    pub seasons: BTreeMap<Season, SeasonParams>,
}

impl ClimateTable {
    /// Parameters of a regime.
    pub fn kind(&self, k: WeatherKind) -> &KindParams {
        &self.kinds[&k]
    }

    /// Normalised transition probabilities from `from`, in [`WeatherKind::ALL`] order, for the
    /// given season and day phase.
    pub fn transition_row(&self, season: Season, phase: DayPhase, from: WeatherKind) -> [f32; 5] {
        let s = &self.seasons[&season];
        let row = &s.transitions[&from];
        let bias = |m: &BTreeMap<DayPhase, BTreeMap<WeatherKind, f32>>, to: WeatherKind| {
            m.get(&phase)
                .and_then(|b| b.get(&to))
                .copied()
                .unwrap_or(1.0)
        };
        let mut w = [0.0f32; 5];
        let mut sum = 0.0;
        for (i, to) in WeatherKind::ALL.iter().enumerate() {
            let base = row.get(to).copied().unwrap_or(0.0);
            w[i] = base * bias(&self.phase_bias, *to) * bias(&s.phase_bias, *to);
            sum += w[i];
        }
        if sum <= 0.0 {
            // Validation guarantees a positive base row; a zeroing bias falls back to "stay".
            let mut stay = [0.0; 5];
            stay[from.code() as usize] = 1.0;
            return stay;
        }
        for x in &mut w {
            *x /= sum;
        }
        w
    }

    fn validate(&self) -> Result<(), WeatherDataError> {
        let f = "climate.toml";
        if self.id != "climate" {
            return Err(invalid(f, "id must be \"climate\""));
        }
        if self.roll_interval_game_s <= 0.0 || self.blend_game_s <= 0.0 {
            return Err(invalid(
                f,
                "roll_interval_game_s and blend_game_s must be > 0",
            ));
        }
        for k in WeatherKind::ALL {
            let p = self
                .kinds
                .get(&k)
                .ok_or_else(|| invalid(f, format!("missing kinds.{k:?}")))?;
            let ok01 = |r: Range2| r.iter().all(|x| (0.0..=1.0).contains(x));
            let pos = |r: Range2| r.iter().all(|x| *x >= 0.0);
            if !ok01(p.intensity) || p.intensity[0] > p.intensity[1] || !ok01(p.cloud_cover) {
                return Err(invalid(
                    f,
                    format!("kinds.{k:?}: intensity/cloud must be 0..1"),
                ));
            }
            if !pos(p.rain_mm_per_h) || !pos(p.fog_per_m) || !pos(p.wind_mps) {
                return Err(invalid(f, format!("kinds.{k:?}: rates must be >= 0")));
            }
        }
        for s in Season::ALL {
            let sp = self
                .seasons
                .get(&s)
                .ok_or_else(|| invalid(f, format!("missing seasons.{s:?}")))?;
            for k in WeatherKind::ALL {
                let row = sp
                    .transitions
                    .get(&k)
                    .ok_or_else(|| invalid(f, format!("seasons.{s:?}: missing row {k:?}")))?;
                if row.values().any(|w| *w < 0.0) || row.values().sum::<f32>() <= 0.0 {
                    return Err(invalid(
                        f,
                        format!("seasons.{s:?}.{k:?}: weights must be >= 0 with a positive sum"),
                    ));
                }
            }
        }
        let biases = self
            .phase_bias
            .values()
            .chain(self.seasons.values().flat_map(|s| s.phase_bias.values()));
        for b in biases {
            if b.values().any(|x| *x < 0.0) {
                return Err(invalid(f, "phase_bias values must be >= 0"));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// surfaces.toml
// ---------------------------------------------------------------------------------------------

/// Hydrology and wet grip of one surface (`[surfaces.<SurfaceType>]`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceParams {
    /// Weather grip multiplier on a fully wet film (reference street tire).
    pub wet_factor: f32,
    /// Max water held by a normal (non-puddle) cell, mm; excess runs off.
    pub film_capacity_mm: f32,
    /// Evaporation at reference conditions, mm/h.
    pub dry_mm_per_h: f32,
    /// Drainage of standing water into the ground, mm/h.
    pub infiltration_mm_per_h: f32,
    /// Relative chance that a cell of this surface is puddle-prone.
    pub puddle_weight: f32,
}

/// Aquaplaning parameters (`[aquaplane]`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AquaplaneParams {
    /// No aquaplaning below this standing-water depth, mm.
    pub min_depth_mm: f32,
    /// Depth the onset speeds are given at, mm.
    pub ref_depth_mm: f32,
    /// Tire width the onset speeds are given at, m.
    pub ref_width_m: f32,
    /// Onset speed for a treaded tire at the reference depth and width, m/s.
    pub onset_treaded_mps: f32,
    /// Onset speed for a slick at the reference depth and width, m/s.
    pub onset_slick_mps: f32,
    /// Onset clamp, m/s.
    pub onset_min_mps: f32,
    /// Onset clamp, m/s.
    pub onset_max_mps: f32,
    /// Full loss reached at `onset · full_at_onset_ratio`.
    pub full_at_onset_ratio: f32,
    /// Grip lost when fully aquaplaning, 0..1.
    pub max_grip_loss: f32,
    /// Depth at which the loss reaches `max_grip_loss`, mm.
    pub full_loss_depth_mm: f32,
}

/// `data/weather/surfaces.toml`: hydrology, wet grip, temperature grip and aquaplaning.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceTable {
    /// Must be `"surfaces"`.
    pub id: String,
    /// WetAsphalt/Asphalt ratio of the reference tire the `wet_factor`s were written for.
    pub reference_wet_ratio: f32,
    /// Water depth at which wetness reaches 1, mm.
    pub film_mm: f32,
    /// Absolute cap on any cell's water, mm.
    pub max_water_mm: f32,
    /// Interim adapter: Asphalt is reported as WetAsphalt at or above this wetness.
    pub interim_wet_threshold: f32,
    /// Evaporation reference temperature, °C.
    pub dry_temp_ref_c: f32,
    /// Evaporation change per °C from the reference.
    pub dry_temp_gain_per_c: f32,
    /// Lower clamp of the temperature evaporation factor.
    pub dry_temp_min: f32,
    /// Evaporation gain per m/s of wind.
    pub dry_wind_gain_per_mps: f32,
    /// Evaporation gain in full sun.
    pub dry_sun_gain: f32,
    /// Evaporation factor by regime (humidity).
    pub dry_humidity: BTreeMap<WeatherKind, f32>,
    /// Drying gain per vehicle of cell occupancy.
    pub traffic_drying_gain: f32,
    /// Occupancy clamp for the drying gain.
    pub traffic_occupancy_cap: f32,
    /// Occupancy moving-average time constant, in-game s.
    pub traffic_tau_game_s: f32,
    /// Base chance a cell is puddle-prone.
    pub puddle_cell_fraction: f32,
    /// Capacity range of puddle-prone cells, mm.
    pub puddle_capacity_mm: Range2,
    /// Road temperature below which cold-grip loss starts, °C.
    pub cold_ref_c: f32,
    /// Grip loss per °C below `cold_ref_c`.
    pub cold_loss_per_c: f32,
    /// Lower clamp of the cold factor.
    pub cold_min: f32,
    /// Road temperature at which a wet road is fully iced, °C.
    pub ice_temp_c: f32,
    /// Ice blends in over `ice_temp_c .. ice_temp_c + ice_band_c`.
    pub ice_band_c: f32,
    /// Grip multiplier on full ice.
    pub ice_factor: f32,
    /// Minimum wetness for ice to form.
    pub ice_min_wetness: f32,
    /// Painted marking multiplier when dry.
    pub marking_dry_factor: f32,
    /// Painted marking multiplier on a fully wet film.
    pub marking_wet_factor: f32,
    /// Aquaplaning.
    pub aquaplane: AquaplaneParams,
    /// Per-surface parameters (every [`SurfaceType`] required).
    pub surfaces: BTreeMap<SurfaceType, SurfaceParams>,
}

/// Every [`SurfaceType`] (the contract enum has no `ALL`).
pub const ALL_SURFACES: [SurfaceType; 8] = [
    SurfaceType::Asphalt,
    SurfaceType::WetAsphalt,
    SurfaceType::Concrete,
    SurfaceType::Gravel,
    SurfaceType::Grass,
    SurfaceType::Dirt,
    SurfaceType::Curb,
    SurfaceType::Metal,
];

impl SurfaceTable {
    /// Parameters of a surface.
    /// Surfaces missing from the table (`PaintedLine` until `surfaces.toml` lists it, contract
    /// v0.3.0 W-3) use the asphalt parameters.
    pub fn surface(&self, s: SurfaceType) -> &SurfaceParams {
        self.surfaces
            .get(&s)
            .unwrap_or(&self.surfaces[&SurfaceType::Asphalt])
    }

    /// The embedded table (`data/weather/surfaces.toml` at build time). Convenience for crates
    /// that need grip numbers without holding a [`WeatherData`].
    pub fn embedded() -> &'static SurfaceTable {
        &WeatherData::embedded_ref().surfaces
    }

    fn validate(&self) -> Result<(), WeatherDataError> {
        let f = "surfaces.toml";
        if self.id != "surfaces" {
            return Err(invalid(f, "id must be \"surfaces\""));
        }
        if self.film_mm <= 0.0 || self.max_water_mm < self.film_mm {
            return Err(invalid(f, "need 0 < film_mm <= max_water_mm"));
        }
        if !(0.0..1.0).contains(&self.reference_wet_ratio) {
            return Err(invalid(f, "reference_wet_ratio must be in 0..1"));
        }
        if self.traffic_tau_game_s <= 0.0 || self.dry_temp_min <= 0.0 {
            return Err(invalid(
                f,
                "traffic_tau_game_s and dry_temp_min must be > 0",
            ));
        }
        for k in WeatherKind::ALL {
            match self.dry_humidity.get(&k) {
                Some(x) if *x > 0.0 => {}
                _ => return Err(invalid(f, format!("dry_humidity.{k:?} must be > 0"))),
            }
        }
        let pc = self.puddle_capacity_mm;
        if pc[0] > pc[1] || pc[1] > self.max_water_mm || pc[0] < 0.0 {
            return Err(invalid(
                f,
                "puddle_capacity_mm must be ordered and <= max_water_mm",
            ));
        }
        let a = &self.aquaplane;
        if a.ref_depth_mm <= 0.0
            || a.ref_width_m <= 0.0
            || a.full_at_onset_ratio <= 1.0
            || a.onset_min_mps <= 0.0
            || a.onset_min_mps > a.onset_max_mps
            || a.full_loss_depth_mm <= a.min_depth_mm
            || !(0.0..=1.0).contains(&a.max_grip_loss)
        {
            return Err(invalid(f, "aquaplane parameters out of range"));
        }
        for s in ALL_SURFACES {
            let p = self
                .surfaces
                .get(&s)
                .ok_or_else(|| invalid(f, format!("missing surfaces.{s:?}")))?;
            if !(0.0..=1.0).contains(&p.wet_factor)
                || p.film_capacity_mm < 0.0
                || p.film_capacity_mm > self.max_water_mm
                || p.dry_mm_per_h <= 0.0
                || p.infiltration_mm_per_h < 0.0
                || p.puddle_weight < 0.0
            {
                return Err(invalid(f, format!("surfaces.{s:?} out of range")));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// effects.toml
// ---------------------------------------------------------------------------------------------

/// Visibility parameters (`[visibility]`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VisibilityParams {
    /// Visibility at which the AI sight multiplier is 1, m.
    pub reference_sight_m: f32,
    /// Extinction floor (clean air), 1/m.
    pub clear_air_per_m: f32,
    /// Lower clamp of the sight multiplier.
    pub min_sight_mult: f32,
    /// Sight loss per mm/h of rain.
    pub rain_sight_loss_per_mm_h: f32,
    /// Max sight loss from rain.
    pub max_rain_sight_loss: f32,
}

/// One modifier row: every value is a `[at intensity 0, at intensity 1]` multiplier pair.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModifierRow {
    /// Patrol speed cap multiplier (wp_police).
    pub patrol_speed_cap: Range2,
    /// Pursuit speed cap multiplier (wp_police).
    pub pursuit_speed_cap: Range2,
    /// Crash / incident rate multiplier (wp_police dispatch, wp_world traffic incidents).
    pub crash_incident_rate: Range2,
    /// Race payout multiplier (wp_economy).
    pub payout: Range2,
    /// Reputation multiplier (wp_economy).
    pub rep: Range2,
    /// Traffic desired-speed multiplier (wp_world).
    pub traffic_speed: Range2,
    /// Traffic time-headway multiplier (wp_world).
    pub traffic_headway: Range2,
    /// Civilian density multiplier (wp_world).
    pub traffic_density: Range2,
}

/// `data/weather/effects.toml`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectsTable {
    /// Must be `"effects"`.
    pub id: String,
    /// Visibility model.
    pub visibility: VisibilityParams,
    /// Rows per regime (every regime required).
    pub kinds: BTreeMap<WeatherKind, ModifierRow>,
    /// Row for wet roads without rain, lerped by mean wetness.
    pub wet_road: ModifierRow,
}

impl EffectsTable {
    fn validate(&self) -> Result<(), WeatherDataError> {
        let f = "effects.toml";
        if self.id != "effects" {
            return Err(invalid(f, "id must be \"effects\""));
        }
        let v = &self.visibility;
        if v.reference_sight_m <= 0.0
            || v.clear_air_per_m <= 0.0
            || !(0.0..=1.0).contains(&v.min_sight_mult)
            || !(0.0..=1.0).contains(&v.max_rain_sight_loss)
        {
            return Err(invalid(f, "visibility parameters out of range"));
        }
        for k in WeatherKind::ALL {
            let r = self
                .kinds
                .get(&k)
                .ok_or_else(|| invalid(f, format!("missing kinds.{k:?}")))?;
            if !row_positive(r) {
                return Err(invalid(f, format!("kinds.{k:?}: multipliers must be > 0")));
            }
        }
        if !row_positive(&self.wet_road) {
            return Err(invalid(f, "wet_road: multipliers must be > 0"));
        }
        Ok(())
    }
}

fn row_positive(r: &ModifierRow) -> bool {
    [
        r.patrol_speed_cap,
        r.pursuit_speed_cap,
        r.crash_incident_rate,
        r.payout,
        r.rep,
        r.traffic_speed,
        r.traffic_headway,
        r.traffic_density,
    ]
    .iter()
    .all(|p| p[0] > 0.0 && p[1] > 0.0)
}

// ---------------------------------------------------------------------------------------------
// All three
// ---------------------------------------------------------------------------------------------

/// All weather tables.
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherData {
    /// `climate.toml`.
    pub climate: ClimateTable,
    /// `surfaces.toml`.
    pub surfaces: SurfaceTable,
    /// `effects.toml`.
    pub effects: EffectsTable,
}

const CLIMATE_SRC: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/weather/climate.toml"
));
const SURFACES_SRC: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/weather/surfaces.toml"
));
const EFFECTS_SRC: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../data/weather/effects.toml"
));

fn parse<T: for<'de> Deserialize<'de>>(file: &str, text: &str) -> Result<T, WeatherDataError> {
    toml::from_str(text).map_err(|source| WeatherDataError::Parse {
        file: file.to_string(),
        source,
    })
}

impl WeatherData {
    /// Parse and validate the three tables from their TOML text.
    pub fn parse(climate: &str, surfaces: &str, effects: &str) -> Result<Self, WeatherDataError> {
        let d = WeatherData {
            climate: parse("climate.toml", climate)?,
            surfaces: parse("surfaces.toml", surfaces)?,
            effects: parse("effects.toml", effects)?,
        };
        d.climate.validate()?;
        d.surfaces.validate()?;
        d.effects.validate()?;
        Ok(d)
    }

    /// Load from a data root (the folder containing `weather/`).
    pub fn load(data_root: &Path) -> Result<Self, WeatherDataError> {
        let read = |name: &str| {
            let path = data_root.join("weather").join(name);
            std::fs::read_to_string(&path).map_err(|source| WeatherDataError::Io { path, source })
        };
        Self::parse(
            &read("climate.toml")?,
            &read("surfaces.toml")?,
            &read("effects.toml")?,
        )
    }

    /// The tables embedded at build time (panics only if the repo's own data is invalid, which
    /// the crate's tests catch).
    pub fn embedded() -> Self {
        Self::embedded_ref().clone()
    }

    /// Shared reference to the embedded tables.
    pub fn embedded_ref() -> &'static WeatherData {
        static DATA: OnceLock<WeatherData> = OnceLock::new();
        DATA.get_or_init(|| {
            WeatherData::parse(CLIMATE_SRC, SURFACES_SRC, EFFECTS_SRC)
                .expect("embedded data/weather tables are valid")
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_tables_parse_and_rows_normalise() {
        let d = WeatherData::embedded();
        for s in Season::ALL {
            for p in [
                DayPhase::Dawn,
                DayPhase::Day,
                DayPhase::Dusk,
                DayPhase::Night,
            ] {
                for k in WeatherKind::ALL {
                    let row = d.climate.transition_row(s, p, k);
                    let sum: f32 = row.iter().sum();
                    assert!((sum - 1.0).abs() < 1e-5, "{s:?} {p:?} {k:?}: {sum}");
                    assert!(row.iter().all(|x| *x >= 0.0));
                }
            }
        }
    }

    #[test]
    fn dawn_biases_toward_fog() {
        let c = &WeatherData::embedded().climate;
        let dawn = c.transition_row(Season::Autumn, DayPhase::Dawn, WeatherKind::Clear);
        let day = c.transition_row(Season::Autumn, DayPhase::Day, WeatherKind::Clear);
        let fog = WeatherKind::Fog.code() as usize;
        assert!(dawn[fog] > 3.0 * day[fog]);
    }

    #[test]
    fn invalid_tables_are_rejected() {
        let bad = CLIMATE_SRC.replace("id = \"climate\"", "id = \"nope\"");
        assert!(WeatherData::parse(&bad, SURFACES_SRC, EFFECTS_SRC).is_err());
        let bad = SURFACES_SRC.replace("[surfaces.Metal]", "[surfaces.Metalx]");
        assert!(WeatherData::parse(CLIMATE_SRC, &bad, EFFECTS_SRC).is_err());
        let bad = EFFECTS_SRC.replace("payout = [1.20, 1.35]", "payout = [0.0, 1.35]");
        assert!(WeatherData::parse(CLIMATE_SRC, SURFACES_SRC, &bad).is_err());
    }
}
