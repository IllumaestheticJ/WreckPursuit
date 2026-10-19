//! # wp_weather
//!
//! Deterministic weather and road conditions (workstream 14).
//!
//! - [`kind`]: [`WeatherKind`] regimes (clear, overcast, light/heavy rain, fog) and [`Season`].
//! - [`data`]: the `data/weather/{climate,surfaces,effects}.toml` tables (embedded + loader).
//! - [`grid`]: [`WetnessGrid`], road water per cell: rain, runoff, infiltration, drying by
//!   surface, temperature, wind, sun and traffic (busy lanes dry first), puddle-prone cells.
//! - [`grip`]: wet / cold / icy / painted-marking grip and aquaplaning, the multiplier
//!   `wp_vehicle` composes onto the tire's `surface_multipliers`.
//! - [`effects`]: AI sight distance, render params, police / traffic / economy multipliers.
//! - [`conditions`]: [`LocalConditions`], the answer to the requested `weather_at(p)`.
//! - [`adapter`]: [`WithWeather`], a `WorldQuery` wrapper that makes rain cost grip today.
//!
//! [`WeatherSim`] composes them. The sim loop calls [`WeatherSim::step`] once per physics tick
//! with the sim `dt`, the world clock and vehicle positions; the expensive work (schedule roll,
//! grid update) runs every [`WeatherConfig::update_interval_s`] of sim time inside. Weather
//! evolves in **in-game** time (`dt × time_ratio`), like the day/night clock.
//!
//! Determinism: all randomness comes from one `wp_math::Rng` seeded by
//! [`WeatherConfig::seed`]; no wall-clock, no `HashMap`, trig via `wp_math::sin_cos`.

#![warn(missing_docs)]

pub mod adapter;
pub mod conditions;
pub mod data;
pub mod effects;
pub mod grid;
pub mod grip;
pub mod kind;

pub use adapter::{Weathered, WithWeather};
pub use conditions::{LocalConditions, CLEAR_VISIBILITY_M};
pub use data::{ClimateTable, EffectsTable, SurfaceTable, WeatherData, WeatherDataError};
pub use effects::{
    spray_intensity, EconomyWeatherModifiers, PoliceWeatherModifiers, RenderParams,
    TrafficWeatherModifiers, WeatherModifiers,
};
pub use grid::{GridSpec, HydroInputs, PuddleCell, WetnessGrid};
pub use grip::TireProfile;
pub use kind::{Season, WeatherKind};

use data::lerp2;

use std::f32::consts::PI;
use wp_contracts::{TimeOfDay, WorldQuery};
use wp_math::{sin_cos, Rng, StateHasher};

/// A regime and its intensity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Regime {
    /// Regime.
    pub kind: WeatherKind,
    /// Intensity 0..1.
    pub intensity: f32,
}

impl Regime {
    /// Construct (intensity clamped to 0..1).
    pub fn new(kind: WeatherKind, intensity: f32) -> Self {
        Self {
            kind,
            intensity: intensity.clamp(0.0, 1.0),
        }
    }
}

/// Weather construction parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherConfig {
    /// Seed for the schedule and the puddle map.
    pub seed: u64,
    /// Season (fixed for a session).
    pub season: Season,
    /// Starting regime.
    pub start: Regime,
    /// Starting water on every cell, mm (e.g. "it rained an hour ago").
    pub start_water_mm: f32,
    /// Sim seconds between internal updates (schedule + grid). Default 0.25 s (4 Hz).
    pub update_interval_s: f32,
    /// Hold the regime: the schedule never rolls (races with fixed weather, tests).
    pub locked: bool,
}

impl Default for WeatherConfig {
    fn default() -> Self {
        Self {
            seed: 1,
            season: Season::default(),
            start: Regime::new(WeatherKind::Clear, 0.6),
            start_water_mm: 0.0,
            update_interval_s: 0.25,
            locked: false,
        }
    }
}

/// A schedule change, for logs / UI and `SimEvent::WeatherChanged` (contract v0.3.0, W-2;
/// see [`WeatherChange::to_sim_event`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WeatherChange {
    /// Regime before.
    pub from: Regime,
    /// Regime after (blends in over `blend_game_s`).
    pub to: Regime,
    /// Whether the change was forced (scenario / captain / debug) rather than rolled.
    pub forced: bool,
}

impl WeatherChange {
    /// The contract event (v0.3.0, W-2) for whoever drains [`WeatherSim::drain_changes`].
    pub fn to_sim_event(&self) -> wp_contracts::SimEvent {
        wp_contracts::SimEvent::WeatherChanged {
            kind: self.to.kind,
            intensity: self.to.intensity,
            forced: self.forced,
        }
    }
}

/// The full weather state.
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherState {
    /// Current regime.
    pub kind: WeatherKind,
    /// Current intensity 0..1.
    pub intensity: f32,
    /// Regime being blended away from.
    pub prev: Regime,
    /// Blend progress prev → current, 0..1.
    pub blend: f32,
    /// Road water per cell.
    pub wetness_by_cell: WetnessGrid,
    /// Fog extinction coefficient, 1/m.
    pub fog_density: f32,
    /// Rain rate, mm/h.
    pub rain_mm_per_h: f32,
    /// Cloud cover 0..1.
    pub cloud_cover: f32,
    /// Air temperature, °C.
    pub temperature_c: f32,
    /// Road surface temperature, °C.
    pub road_temp_c: f32,
    /// Wind velocity in world XY, m/s.
    pub wind: [f32; 2],
    /// Wind heading, rad (0 = toward +Y, clockwise toward +X).
    pub wind_heading_rad: f32,
    /// Mean road wetness 0..1.
    pub mean_road_wetness: f32,
    /// In-game seconds since the last schedule roll.
    pub since_roll_game_s: f32,
    /// In-game seconds in the current regime.
    pub in_kind_game_s: f32,
}

/// Deterministic weather simulation. See the crate docs.
#[derive(Debug, Clone)]
pub struct WeatherSim {
    data: WeatherData,
    cfg: WeatherConfig,
    rng: Rng,
    state: WeatherState,
    pending_s: f32,
    last_time: TimeOfDay,
    modifiers: WeatherModifiers,
    changes: Vec<WeatherChange>,
    revision: u64,
}

impl WeatherSim {
    /// Create with explicit tables and grid. Puddle cells are drawn from a fork of the seed.
    pub fn new(
        data: WeatherData,
        cfg: WeatherConfig,
        grid: GridSpec,
        surfaces: Vec<wp_contracts::SurfaceType>,
    ) -> Self {
        let mut rng = Rng::new(cfg.seed);
        let mut grid_rng = rng.fork();
        let wetness = WetnessGrid::new(grid, surfaces, &data.surfaces, &mut grid_rng);
        Self::assemble(data, cfg, rng, wetness)
    }

    /// Create over a uniform-surface grid (flat test worlds).
    pub fn uniform(
        data: WeatherData,
        cfg: WeatherConfig,
        grid: GridSpec,
        surface: wp_contracts::SurfaceType,
    ) -> Self {
        Self::new(data, cfg, grid, vec![surface; grid.len()])
    }

    /// Create by sampling `world.surface_at` at every cell centre.
    pub fn from_world(
        data: WeatherData,
        cfg: WeatherConfig,
        grid: GridSpec,
        world: &dyn WorldQuery,
    ) -> Self {
        let mut rng = Rng::new(cfg.seed);
        let mut grid_rng = rng.fork();
        let wetness = WetnessGrid::from_world(grid, world, &data.surfaces, &mut grid_rng);
        Self::assemble(data, cfg, rng, wetness)
    }

    fn assemble(
        data: WeatherData,
        cfg: WeatherConfig,
        mut rng: Rng,
        mut grid: WetnessGrid,
    ) -> Self {
        grid.fill(cfg.start_water_mm);
        let wind_heading_rad = rng.range_f32(-PI, PI);
        let start = cfg.start;
        let state = WeatherState {
            kind: start.kind,
            intensity: start.intensity,
            prev: start,
            blend: 1.0,
            wetness_by_cell: grid,
            fog_density: 0.0,
            rain_mm_per_h: 0.0,
            cloud_cover: 0.0,
            temperature_c: 15.0,
            road_temp_c: 15.0,
            wind: [0.0, 0.0],
            wind_heading_rad,
            mean_road_wetness: 0.0,
            since_roll_game_s: 0.0,
            in_kind_game_s: 0.0,
        };
        let mut sim = Self {
            data,
            cfg,
            rng,
            state,
            pending_s: 0.0,
            last_time: TimeOfDay::from_hours(12.0),
            modifiers: WeatherModifiers::NEUTRAL,
            changes: Vec::new(),
            revision: 0,
        };
        sim.refresh_atmosphere(sim.last_time);
        sim
    }

    /// Advance by `dt_s` sim seconds. `time` is the world clock now, `time_ratio` its in-game
    /// seconds per sim second, `vehicles` the world positions of moving vehicles (for traffic
    /// drying; may be empty). Internal updates run every `update_interval_s`.
    pub fn step(&mut self, dt_s: f32, time: TimeOfDay, time_ratio: f32, vehicles: &[[f32; 3]]) {
        self.pending_s += dt_s.max(0.0);
        let interval = self.cfg.update_interval_s.max(1e-3);
        while self.pending_s >= interval {
            self.pending_s -= interval;
            self.update(interval * time_ratio.max(0.0), time, vehicles);
        }
    }

    /// One internal update of `game_dt` in-game seconds (public for tools and tests that want
    /// to skip hours quickly).
    pub fn update(&mut self, game_dt: f32, time: TimeOfDay, vehicles: &[[f32; 3]]) {
        self.revision += 1;
        self.last_time = time;
        self.advance_schedule(game_dt, time);
        self.refresh_atmosphere(time);
        let s = &self.data.surfaces;
        let st = &mut self.state;
        let night = time.night_factor();
        let temp_f =
            (1.0 + s.dry_temp_gain_per_c * (st.road_temp_c - s.dry_temp_ref_c)).max(s.dry_temp_min);
        let wind_speed = (st.wind[0] * st.wind[0] + st.wind[1] * st.wind[1]).sqrt();
        let wind_f = 1.0 + s.dry_wind_gain_per_mps * wind_speed;
        let sun_f = 1.0 + s.dry_sun_gain * (1.0 - st.cloud_cover) * (1.0 - night);
        let hum = |k: WeatherKind| s.dry_humidity[&k];
        let humidity = hum(st.prev.kind) + (hum(st.kind) - hum(st.prev.kind)) * st.blend;
        let inp = HydroInputs {
            rain_mm_per_h: st.rain_mm_per_h,
            evaporation_factor: temp_f * wind_f * sun_f * humidity,
        };
        st.wetness_by_cell.step(game_dt, &inp, vehicles, s);
        st.mean_road_wetness = st.wetness_by_cell.mean_road_wetness(s);
        self.refresh_modifiers();
    }

    fn advance_schedule(&mut self, game_dt: f32, time: TimeOfDay) {
        let c = &self.data.climate;
        let st = &mut self.state;
        st.blend = (st.blend + game_dt / c.blend_game_s).min(1.0);
        st.in_kind_game_s += game_dt;
        st.since_roll_game_s += game_dt;
        if st.since_roll_game_s < c.roll_interval_game_s {
            return;
        }
        st.since_roll_game_s -= c.roll_interval_game_s;
        // Very large steps (tools skipping hours) roll at most once per update.
        st.since_roll_game_s = st.since_roll_game_s.min(c.roll_interval_game_s);
        // Draw every roll's numbers unconditionally so the RNG stream does not depend on
        // whether a roll was skipped.
        let pick = self.rng.next_f32();
        let new_int = self.rng.next_f32();
        let drift = self.rng.range_f32(-0.2, 0.2);
        let heading = self
            .rng
            .range_f32(-c.wind_heading_step_rad, c.wind_heading_step_rad);
        st.wind_heading_rad = wp_math::wrap_angle(st.wind_heading_rad + heading);
        if self.cfg.locked
            || st.blend < 1.0
            || st.in_kind_game_s < c.kind(st.kind).min_duration_game_s
        {
            return;
        }
        let row = c.transition_row(self.cfg.season, time.phase(), st.kind);
        let mut to = st.kind;
        let mut acc = 0.0;
        for (i, k) in WeatherKind::ALL.iter().enumerate() {
            acc += row[i];
            if pick < acc {
                to = *k;
                break;
            }
        }
        let range = c.kind(to).intensity;
        let intensity = if to == st.kind {
            (st.intensity + drift).clamp(range[0], range[1])
        } else {
            lerp2(range, new_int)
        };
        let from = Regime::new(st.kind, st.intensity);
        let next = Regime::new(to, intensity);
        if next != from {
            self.begin_transition(next, false, false);
        }
    }

    fn begin_transition(&mut self, to: Regime, immediate: bool, forced: bool) {
        let st = &mut self.state;
        let from = Regime::new(st.kind, st.intensity);
        st.prev = if immediate { to } else { from };
        st.blend = if immediate { 1.0 } else { 0.0 };
        if to.kind != st.kind {
            st.in_kind_game_s = 0.0;
        }
        st.kind = to.kind;
        st.intensity = to.intensity;
        self.changes.push(WeatherChange { from, to, forced });
    }

    /// Recompute rain, fog, cloud, wind and temperatures from the regime blend and `time`.
    fn refresh_atmosphere(&mut self, time: TimeOfDay) {
        let c = &self.data.climate;
        let st = &mut self.state;
        let (pk, ck) = (c.kind(st.prev.kind), c.kind(st.kind));
        let (pi, ci, b) = (st.prev.intensity, st.intensity, st.blend);
        let mix = |p: data::Range2, q: data::Range2| {
            let a = lerp2(p, pi);
            a + (lerp2(q, ci) - a) * b
        };
        st.rain_mm_per_h = mix(pk.rain_mm_per_h, ck.rain_mm_per_h).max(0.0);
        st.fog_density = mix(pk.fog_per_m, ck.fog_per_m).max(0.0);
        st.cloud_cover = mix(pk.cloud_cover, ck.cloud_cover).clamp(0.0, 1.0);
        let wind_speed = mix(pk.wind_mps, ck.wind_mps).max(0.0);
        let (s, co) = sin_cos(st.wind_heading_rad);
        st.wind = [wind_speed * s, wind_speed * co];

        let season = &c.seasons[&self.cfg.season];
        // Diurnal curve: peak at 15:00, minimum at 03:00; clouds damp the swing.
        let (_, diurnal) = sin_cos((time.hours() - 15.0) * (2.0 * PI / 24.0));
        let offset = pk.temp_offset_c + (ck.temp_offset_c - pk.temp_offset_c) * b;
        st.temperature_c = season.mean_temp_c
            + season.diurnal_amplitude_c * diurnal * (1.0 - 0.5 * st.cloud_cover)
            + offset;
        let night = time.night_factor();
        let clear = 1.0 - st.cloud_cover;
        st.road_temp_c = st.temperature_c + c.road_sun_gain_c * clear * (1.0 - night)
            - c.road_sky_cooling_c * clear * night;
    }

    fn refresh_modifiers(&mut self) {
        let st = &self.state;
        let sight = effects::sight_multiplier(
            &self.data.effects.visibility,
            st.fog_density,
            st.rain_mm_per_h,
        );
        self.modifiers = effects::blend_modifiers(
            &self.data.effects,
            (st.prev.kind, st.prev.intensity),
            (st.kind, st.intensity),
            st.blend,
            st.mean_road_wetness,
            sight,
        );
    }

    /// Force a regime (scenario setup, debug UI, future captain tool). `immediate` skips the
    /// blend. Does not lock the schedule; see [`WeatherSim::set_locked`].
    pub fn force(&mut self, to: Regime, immediate: bool) {
        self.revision += 1;
        self.begin_transition(to, immediate, true);
        self.refresh_atmosphere(self.last_time);
        self.refresh_modifiers();
    }

    /// Hold (or release) the current regime.
    pub fn set_locked(&mut self, locked: bool) {
        self.cfg.locked = locked;
    }

    /// Set every cell's water (scenario setup: "the road is soaked").
    pub fn set_all_water_mm(&mut self, water_mm: f32) {
        self.revision += 1;
        self.state.wetness_by_cell.fill(water_mm);
        self.state.mean_road_wetness = self
            .state
            .wetness_by_cell
            .mean_road_wetness(&self.data.surfaces);
        self.refresh_modifiers();
    }

    /// Flood the cells whose centres lie in the XY box `min..max` with `water_mm` of
    /// standing water (scenario setup: a puddle to aquaplane on; see
    /// [`WetnessGrid::flood_rect`]). Returns the number of cells flooded.
    pub fn flood_rect(&mut self, min: [f32; 2], max: [f32; 2], water_mm: f32) -> usize {
        self.revision += 1;
        let n = self
            .state
            .wetness_by_cell
            .flood_rect(min, max, water_mm, &self.data.surfaces);
        self.state.mean_road_wetness = self
            .state
            .wetness_by_cell
            .mean_road_wetness(&self.data.surfaces);
        self.refresh_modifiers();
        n
    }

    /// The full state.
    pub fn state(&self) -> &WeatherState {
        &self.state
    }

    /// The tables in use.
    pub fn data(&self) -> &WeatherData {
        &self.data
    }

    /// The configuration in use.
    pub fn config(&self) -> &WeatherConfig {
        &self.cfg
    }

    /// Weather at a world point (the requested `WorldQuery::weather_at`).
    pub fn conditions_at(&self, p: [f32; 3]) -> LocalConditions {
        let st = &self.state;
        let s = &self.data.surfaces;
        let i = st.wetness_by_cell.index(p);
        let water = st.wetness_by_cell.water_at_cell(i);
        LocalConditions {
            kind: st.kind,
            intensity: st.intensity,
            rain_mm_per_h: st.rain_mm_per_h,
            water_mm: water,
            wetness: WetnessGrid::wetness_of(water, s),
            standing_water_mm: WetnessGrid::standing_of(water, s),
            air_temp_c: st.temperature_c,
            road_temp_c: st.road_temp_c,
            fog_per_m: st.fog_density,
            sight_multiplier: self.modifiers.sight_multiplier,
            visibility_m: self.visibility_m(p),
            wind_mps: st.wind,
        }
    }

    /// AI sight-distance multiplier (fog + rain), 0..1.
    pub fn sight_multiplier(&self) -> f32 {
        self.modifiers.sight_multiplier
    }

    /// How far anyone can see in this weather, m: `visibility.reference_sight_m` (800) ×
    /// [`WeatherSim::sight_multiplier`]. 800 m in clear air, ~130 m in thick fog, ~480 m in a
    /// 16 mm/h downpour. The adapters' `line_of_sight` is false beyond it.
    pub fn sight_range_m(&self) -> f32 {
        self.data.effects.visibility.reference_sight_m * self.modifiers.sight_multiplier
    }

    /// Visibility at a world point, m: how far an observer standing at `p` can see through fog
    /// and rain (night not included). Today the sky is uniform, so this equals
    /// [`WeatherSim::sight_range_m`] everywhere; it takes a point so callers do not change when
    /// fog banks arrive. Police use it as a cap on every sight range:
    /// `range = phase_range.min(weather.visibility_m(observer))` (and after contract W-1 the same
    /// value arrives as `world.weather_at(observer).visibility_m`).
    pub fn visibility_m(&self, p: [f32; 3]) -> f32 {
        let _ = p; // uniform sky: no spatial variation yet
        self.sight_range_m()
    }

    /// Weather-limited sight check for police / AI: true if `b` is within
    /// `base_sight_m × sight_multiplier` of `a` **and** `world.line_of_sight(a, b)` (v0.2.0)
    /// is clear. The distance test runs first, so fog skips the occluder query for far targets.
    /// Night is not included; the caller folds its own night factor into `base_sight_m`.
    pub fn can_see(
        &self,
        world: &dyn WorldQuery,
        a: [f32; 3],
        b: [f32; 3],
        base_sight_m: f32,
    ) -> bool {
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let dist2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        let range = base_sight_m.max(0.0) * self.modifiers.sight_multiplier;
        dist2 <= range * range && world.line_of_sight(a, b)
    }

    /// All blended modifiers.
    pub fn modifiers(&self) -> &WeatherModifiers {
        &self.modifiers
    }

    /// Police multipliers (patrol/pursuit speed caps, crash rate, sight).
    pub fn police_modifiers(&self) -> PoliceWeatherModifiers {
        self.modifiers.police()
    }

    /// Traffic multipliers (speed, headway, density, hazard sight, incidents).
    pub fn traffic_modifiers(&self) -> TrafficWeatherModifiers {
        self.modifiers.traffic()
    }

    /// Economy multipliers (payout, rep).
    pub fn economy_modifiers(&self) -> EconomyWeatherModifiers {
        self.modifiers.economy()
    }

    /// Renderer parameters.
    pub fn render_params(&self) -> RenderParams {
        let st = &self.state;
        RenderParams {
            kind: st.kind,
            intensity: st.intensity,
            rain_mm_per_h: st.rain_mm_per_h,
            fog_per_m: st.fog_density,
            visibility_m: effects::visibility_m(&self.data.effects.visibility, st.fog_density),
            cloud_cover: st.cloud_cover,
            sun_dimming: 1.0 - 0.7 * st.cloud_cover,
            wind_mps: st.wind,
            mean_road_wetness: st.mean_road_wetness,
            air_temp_c: st.temperature_c,
            road_temp_c: st.road_temp_c,
        }
    }

    /// Cells with at least `min_depth_mm` of standing water (renderer puddles).
    pub fn puddle_cells(&self, min_depth_mm: f32) -> Vec<PuddleCell> {
        self.state
            .wetness_by_cell
            .puddle_cells(min_depth_mm, &self.data.surfaces)
    }

    /// Drain the schedule changes since the last call.
    pub fn drain_changes(&mut self) -> Vec<WeatherChange> {
        std::mem::take(&mut self.changes)
    }

    /// Network / save snapshot of the replicated state (clients hold the schedule locked and
    /// apply these at a low rate; see request W-6).
    pub fn snapshot(&self) -> WeatherSnapshot {
        let st = &self.state;
        WeatherSnapshot {
            kind: st.kind.code(),
            intensity: st.intensity,
            prev_kind: st.prev.kind.code(),
            prev_intensity: st.prev.intensity,
            blend: st.blend,
            wind_heading_rad: st.wind_heading_rad,
            water_q8: st.wetness_by_cell.water_q8(),
        }
    }

    /// Apply a snapshot from [`WeatherSim::snapshot`]. Returns false (nothing changed) if the
    /// codes or grid size do not match.
    pub fn apply_snapshot(&mut self, snap: &WeatherSnapshot) -> bool {
        let (Some(kind), Some(prev)) = (
            WeatherKind::from_code(snap.kind),
            WeatherKind::from_code(snap.prev_kind),
        ) else {
            return false;
        };
        if !self.state.wetness_by_cell.apply_water_q8(&snap.water_q8) {
            return false;
        }
        self.revision += 1;
        let st = &mut self.state;
        st.kind = kind;
        st.intensity = snap.intensity.clamp(0.0, 1.0);
        st.prev = Regime::new(prev, snap.prev_intensity);
        st.blend = snap.blend.clamp(0.0, 1.0);
        st.wind_heading_rad = snap.wind_heading_rad;
        st.mean_road_wetness = st.wetness_by_cell.mean_road_wetness(&self.data.surfaces);
        self.refresh_atmosphere(self.last_time);
        self.refresh_modifiers();
        true
    }

    /// Counts every change to the hashed state except the sub-interval timer: internal updates
    /// ([`WeatherSim::update`]), `force`, `set_all_water_mm`, `flood_rect`, `apply_snapshot`.
    /// Between two revisions only the pending-time accumulator moves, so callers that hash every
    /// tick (the browser replay log) can cache [`WeatherSim::state_hash`] per revision.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Mix the weather state into a determinism hash.
    pub fn hash_into(&self, h: &mut StateHasher) {
        let st = &self.state;
        h.write_u32(st.kind.code() as u32);
        h.write_u32(st.prev.kind.code() as u32);
        for v in [
            st.intensity,
            st.prev.intensity,
            st.blend,
            st.since_roll_game_s,
            st.in_kind_game_s,
            st.wind_heading_rad,
            st.temperature_c,
            self.pending_s,
        ] {
            h.write_f32_q(v, 1e4);
        }
        st.wetness_by_cell.hash_into(h);
    }

    /// Stand-alone state hash.
    pub fn state_hash(&self) -> u64 {
        let mut h = StateHasher::new();
        self.hash_into(&mut h);
        h.finish()
    }
}

/// Replicated weather state (see [`WeatherSim::snapshot`]).
#[derive(Debug, Clone, PartialEq)]
pub struct WeatherSnapshot {
    /// [`WeatherKind::code`] of the current regime.
    pub kind: u8,
    /// Current intensity.
    pub intensity: f32,
    /// [`WeatherKind::code`] of the regime being blended from.
    pub prev_kind: u8,
    /// Its intensity.
    pub prev_intensity: f32,
    /// Blend progress.
    pub blend: f32,
    /// Wind heading, rad.
    pub wind_heading_rad: f32,
    /// [`WetnessGrid::water_q8`].
    pub water_q8: Vec<u8>,
}
