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
#[derive(Clone)]
pub(crate) struct WeatherLink {
    cfg: WeatherConfig,
    weather: WeatherSim,
    /// Scratch for vehicle + traffic positions, reused every tick.
    positions: Vec<[f32; 3]>,
    /// `(WeatherSim::revision, WeatherSim::state_hash)` at the last change: hashing the whole
    /// wetness grid every tick costs milliseconds (the browser hashes every tick).
    hash_cache: (u64, u64),
}

impl WeatherLink {
    pub(crate) fn new(cfg: WeatherConfig, world: &World) -> Self {
        let weather = Self::build(&cfg, world);
        let hash_cache = (weather.revision(), weather.state_hash());
        Self {
            cfg,
            weather,
            positions: Vec::new(),
            hash_cache,
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

    /// The weather's state hash: cached per revision (the grid only changes on internal updates,
    /// every `update_interval_s`, and on explicit edits); recomputed if the cache is stale.
    fn state_hash(&self) -> u64 {
        if self.hash_cache.0 == self.weather.revision() {
            self.hash_cache.1
        } else {
            self.weather.state_hash()
        }
    }

    /// Refresh the hash cache after the weather changed.
    fn refresh_hash(&mut self) {
        if self.hash_cache.0 != self.weather.revision() {
            self.hash_cache = (self.weather.revision(), self.weather.state_hash());
        }
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
            link.hash_cache = (link.weather.revision(), link.weather.state_hash());
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
        link.refresh_hash();
    }

    /// Fold the weather into the state hash (only when enabled, so weather-less hashes stay):
    /// `WeatherSim::state_hash` as of its latest revision (see `WeatherSim::revision`).
    pub(crate) fn hash_weather(&self, h: &mut StateHasher) {
        if let Some(link) = &self.weather {
            h.write_u64(link.state_hash());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SimConfig;
    use wp_contracts::{TimeOfDay, VehicleInput};
    use wp_weather::WeatherKind;

    fn data(path: &str) -> String {
        format!("{}/../../data/{path}", env!("CARGO_MANIFEST_DIR"))
    }

    fn sim() -> Sim {
        let mode = wp_contracts::load_mode_table(data("modes/arcade.toml")).unwrap();
        let mut sim = Sim::new(SimConfig {
            seed: 5,
            mode,
            time_of_day: TimeOfDay::from_hours(12.0),
            time_ratio: 10.0,
        });
        let car = wp_contracts::load_car(data("cars/debug_sedan.json")).unwrap();
        let id = sim.spawn_vehicle(&car, [0.0; 3], 0.0).unwrap();
        sim.set_input(
            id,
            VehicleInput {
                throttle: 0.6,
                ..Default::default()
            },
        )
        .unwrap();
        sim
    }

    fn rain() -> WeatherConfig {
        WeatherConfig {
            seed: 9,
            start: Regime::new(WeatherKind::HeavyRain, 1.0),
            start_water_mm: 1.5,
            locked: true,
            ..Default::default()
        }
    }

    #[test]
    fn weather_is_deterministic_and_off_by_default() {
        let run = |weather: bool| {
            let mut s = sim();
            if weather {
                s.set_weather(rain());
            }
            for _ in 0..240 {
                s.step_fixed();
            }
            s.state_hash()
        };
        assert!(sim().weather().is_none());
        assert_eq!(run(true), run(true));
        assert_ne!(run(true), run(false));
    }

    #[test]
    fn forced_changes_emit_weather_changed() {
        let mut s = sim();
        s.set_weather(rain());
        s.step_fixed();
        s.drain_events();
        s.force_weather(Regime::new(WeatherKind::Fog, 1.0), true, true);
        s.step_fixed();
        let ev: Vec<_> = s
            .drain_events()
            .into_iter()
            .filter(|e| matches!(e, SimEvent::WeatherChanged { .. }))
            .collect();
        assert_eq!(
            ev,
            vec![SimEvent::WeatherChanged {
                kind: WeatherKind::Fog,
                intensity: 1.0,
                forced: true
            }]
        );
    }

    #[test]
    fn cached_weather_hash_matches_a_full_hash() {
        let mut s = sim();
        s.set_weather(rain());
        for _ in 0..130 {
            s.step_fixed();
            let link = s.weather.as_ref().unwrap();
            assert_eq!(link.hash_cache.0, link.weather.revision());
        }
        // An edit between ticks (stale cache) still hashes the new state.
        let before = s.state_hash();
        s.weather_mut().unwrap().set_all_water_mm(0.0);
        assert_ne!(s.state_hash(), before);
    }

    #[test]
    fn fog_caps_line_of_sight_and_wet_roads_reach_weather_at() {
        let mut s = sim();
        s.set_weather(rain());
        let link = s.weather.as_ref().unwrap();
        let q = link.query(&s.world);
        let wet = q.weather_at([0.0, 0.0, 0.0]);
        assert!(wet.water_mm > 1.0 && wet.wetness > 0.99, "{wet:?}");
        // Interim remap off: the base surface reaches the tires, wp_vehicle applies the water.
        assert_eq!(q.surface_at([0.0; 3]), s.world.surface_at([0.0; 3]));
        assert!(q.line_of_sight([0.0, 0.0, 1.2], [0.0, 300.0, 1.2]));
        s.force_weather(Regime::new(WeatherKind::Fog, 1.0), true, true);
        let link = s.weather.as_ref().unwrap();
        let q = link.query(&s.world);
        let vis = q.visibility_m([0.0; 3]);
        assert!(vis < 200.0, "{vis}");
        assert!(!q.line_of_sight([0.0, 0.0, 1.2], [0.0, 300.0, 1.2]));
        assert!(q.line_of_sight([0.0, 0.0, 1.2], [0.0, vis * 0.5, 1.2]));
    }
}
