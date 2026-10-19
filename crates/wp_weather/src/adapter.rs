//! [`WithWeather`]: wrap any [`WorldQuery`] so weather reaches the vehicles **today**, before
//! the contract has `weather_at` (request W-1).
//!
//! Interim behaviour: `surface_at` reports `Asphalt` as `WetAsphalt` where the road wetness is
//! at or above `surfaces.toml: interim_wet_threshold`. Every tire already has a `WetAsphalt`
//! multiplier, so wet roads cost grip with no change in `wp_vehicle`. Everything else is
//! forwarded unchanged. Once W-1 lands, `WithWeather` also answers `weather_at` and the
//! surface remapping is switched off (`interim_surface = false`), because `wp_vehicle` then
//! composes the full multiplier itself.

use wp_contracts::{SignalState, SurfaceType, TimeOfDay, WorldQuery};

use crate::conditions::LocalConditions;
use crate::WeatherSim;

/// A [`WorldQuery`] that layers weather over another one.
pub struct WithWeather<'a> {
    /// The underlying world (normally `wp_world::World`).
    pub world: &'a dyn WorldQuery,
    /// The weather.
    pub weather: &'a WeatherSim,
    /// Report wet asphalt as [`SurfaceType::WetAsphalt`] (interim; see the module docs).
    pub interim_surface: bool,
}

impl<'a> WithWeather<'a> {
    /// Wrap `world` with interim surface remapping on.
    pub fn new(world: &'a dyn WorldQuery, weather: &'a WeatherSim) -> Self {
        Self {
            world,
            weather,
            interim_surface: true,
        }
    }

    /// Weather at `p` (what `WorldQuery::weather_at` will return).
    pub fn weather_at(&self, p: [f32; 3]) -> LocalConditions {
        self.weather.conditions_at(p)
    }
}

impl WorldQuery for WithWeather<'_> {
    fn surface_at(&self, p: [f32; 3]) -> SurfaceType {
        let s = self.world.surface_at(p);
        if self.interim_surface
            && s == SurfaceType::Asphalt
            && self.weather.conditions_at(p).wetness
                >= self.weather.data().surfaces.interim_wet_threshold
        {
            SurfaceType::WetAsphalt
        } else {
            s
        }
    }

    fn speed_limit_at(&self, p: [f32; 3]) -> Option<f32> {
        self.world.speed_limit_at(p)
    }

    fn signal_state_ahead(&self, p: [f32; 3], heading: [f32; 3]) -> Option<SignalState> {
        self.world.signal_state_ahead(p, heading)
    }

    fn time_of_day(&self) -> TimeOfDay {
        self.world.time_of_day()
    }
}
