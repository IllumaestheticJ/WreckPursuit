//! Weather in the game loop (lead, step 2 of the roadmap; machine K's `wp_weather`).
//!
//! [`Sim::set_weather`] turns weather on. From then on, every tick:
//! 1. after the world step (so the clock is current) the [`WeatherSim`] advances by
//!    `PHYSICS_DT` with the world clock, its time ratio and every moving vehicle (sim vehicles
//!    and traffic agents: busy lanes dry first);
//! 2. vehicles and police get the world through a [`WithWeather`] adapter: `weather_at` answers
//!    per-wheel water, standing water and road temperature (painted lines come from the
//!    world's `surface_at`, `SurfaceType::PaintedLine`), and `line_of_sight` is cut at
//!    `visibility_m`;
//! 3. regime changes become `SimEvent::WeatherChanged`.
//!
//! The adapter's interim `WetAsphalt` remap is **off**: `wp_vehicle` reads `weather_at` per
//! wheel itself (A's hook), so the wet loss is counted once. Without `set_weather` nothing here
//! runs and the sim is exactly as before (same hashes).

use wp_contracts::{SimEvent, WorldQuery, PHYSICS_DT};
use wp_math::StateHasher;
use wp_weather::{GridSpec, Regime, WeatherConfig, WeatherData, WeatherSim, WithWeather};
use wp_world::World;

use crate::Sim;

/// Wetness grid cell size, m (K's recommendation).
pub const WEATHER_CELL_M: f32 = 8.0;
/// Margin around the road network covered by the wetness grid, m.
pub const WEATHER_GRID_MARGIN_M: f32 = 50.0;

/// Weather owned by the sim.
pub(crate) struct WeatherLink {
    cfg: WeatherConfig,
    weather: WeatherSim,
    /// Scratch for vehicle + traffic positions, reused every tick.
    positions: Vec<[f32; 3]>,
}

impl WeatherLink {
    pub(crate) fn new(cfg: WeatherConfig, world: &World) -> Self {
        let weather = Self::build(&cfg, world);
        Self {
            cfg,
            weather,
            positions: Vec::new(),
        }
    }

    /// The wetness grid covers the road network plus a margin (a 2 km square around the
    /// origin on the flat world, which has no roads).
    fn build(cfg: &WeatherConfig, world: &World) -> WeatherSim {
        let grid = GridSpec::around(
            world.network().nodes.iter().map(|n| &n.pos),
            WEATHER_GRID_MARGIN_M,
            WEATHER_CELL_M,
        );
        WeatherSim::from_world(WeatherData::embedded(), cfg.clone(), grid, world)
    }

    /// The world as vehicles and police see it this tick.
    pub(crate) fn query<'a>(&'a self, world: &'a dyn WorldQuery) -> WithWeather<'a> {
        let mut q = WithWeather::new(world, &self.weather);
        // wp_vehicle composes the full wet/ice/paint/aquaplaning model from `weather_at`.
        q.interim_surface = false;
        q
    }
}

impl Sim {
    /// Turn weather on (or reconfigure it): a seeded, deterministic [`WeatherSim`] over the
    /// current world's road network, stepped every tick. [`Sim::set_world`] rebuilds the grid
    /// for the new city with the same config, so call order does not matter. Scenario setup:
    /// call before the first tick so replays match.
    pub fn set_weather(&mut self, cfg: WeatherConfig) {
        self.weather = Some(WeatherLink::new(cfg, &self.world));
    }

    /// Turn weather off (dry road, clear air: the behaviour before weather existed).
    pub fn clear_weather(&mut self) {
        self.weather = None;
    }

    /// The weather, if enabled (renderer, HUD, economy).
    pub fn weather(&self) -> Option<&WeatherSim> {
        self.weather.as_ref().map(|w| &w.weather)
    }

    /// Mutable weather (scenario setup, debug UI: `force`, `set_all_water_mm`, `flood_rect`).
    /// Changing it mid-run is an input as far as replays are concerned.
    pub fn weather_mut(&mut self) -> Option<&mut WeatherSim> {
        self.weather.as_mut().map(|w| &mut w.weather)
    }

    /// Force a regime now (`immediate` skips the blend) and hold or release the schedule.
    /// No-op without weather. Emits `SimEvent::WeatherChanged { forced: true }` on the next tick.
    pub fn force_weather(&mut self, to: Regime, immediate: bool, locked: bool) {
        if let Some(w) = self.weather_mut() {
            w.force(to, immediate);
            w.set_locked(locked);
        }
    }

    /// `set_world` hook: same config, new grid.
    pub(crate) fn rebuild_weather(&mut self) {
        if let Some(link) = &mut self.weather {
            link.weather = WeatherLink::build(&link.cfg, &self.world);
        }
    }

    /// One weather tick (after the world step) + its events.
    pub(crate) fn step_weather(&mut self, out: &mut Vec<SimEvent>) {
        let Some(link) = &mut self.weather else {
            return;
        };
        link.positions.clear();
        let bodies = &self.physics.bodies;
        link.positions.extend(
            self.vehicles
                .iter()
                .map(|v| bodies[v.body.body].translation().to_array()),
        );
        link.positions.extend(self.world.agents().map(|a| a.pos));
        link.weather.step(
            PHYSICS_DT,
            self.world.time_of_day(),
            self.world.clock.ratio,
            &link.positions,
        );
        out.extend(
            link.weather
                .drain_changes()
                .iter()
                .map(|c| c.to_sim_event()),
        );
    }

    /// Fold the weather into the state hash (only when enabled, so weather-less hashes stay).
    pub(crate) fn hash_weather(&self, h: &mut StateHasher) {
        if let Some(link) = &self.weather {
            link.weather.hash_into(h);
        }
    }
}
