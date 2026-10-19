//! [`WithWeather`]: wrap any [`WorldQuery`] so weather reaches the vehicles **today**, before
//! the contract has `weather_at` (request W-1).
//!
//! Interim behaviour: `surface_at` reports `Asphalt` as `WetAsphalt` where the road wetness is
//! at or above `surfaces.toml: interim_wet_threshold`. Every tire already has a `WetAsphalt`
//! multiplier, so wet roads cost grip with no change in `wp_vehicle`. Everything else is
//! forwarded unchanged. Once W-1 lands, `WithWeather` also answers `weather_at` and the
//! surface remapping is switched off (`interim_surface = false`), because `wp_vehicle` then
//! composes the full multiplier itself.
//!
//! `line_of_sight` adds the weather range on top of the world's occluder test: false beyond
//! [`WeatherSim::visibility_m`] at the observer `a` (= [`WeatherSim::sight_range_m`] while the
//! sky is uniform) (800 m × the fog/rain sight multiplier; 800 m in clear air, so
//! longer than any police range and behaviour is unchanged). Turn it off with
//! `weather_los = false` to get the geometric test alone.
//!
//! [`Weathered`] is the owning variant (`Weathered<W>` holds the world and the weather), for
//! code that needs a `WorldQuery` by value, e.g. `wp_vehicle::rig::Rig<W>` in tests and tools.

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
    /// Limit `line_of_sight` to the weather sight range.
    pub weather_los: bool,
}

impl<'a> WithWeather<'a> {
    /// Wrap `world` with interim surface remapping on.
    pub fn new(world: &'a dyn WorldQuery, weather: &'a WeatherSim) -> Self {
        Self {
            world,
            weather,
            interim_surface: true,
            weather_los: true,
        }
    }

    /// Weather at `p` (what `WorldQuery::weather_at` will return).
    pub fn weather_at(&self, p: [f32; 3]) -> LocalConditions {
        self.weather.conditions_at(p)
    }

    /// Visibility from `p`, m ([`WeatherSim::visibility_m`]): the cap `line_of_sight` applies.
    pub fn visibility_m(&self, p: [f32; 3]) -> f32 {
        self.weather.visibility_m(p)
    }
}

/// The interim surface rule shared by both adapters.
fn weathered_surface(
    base: SurfaceType,
    weather: &WeatherSim,
    interim: bool,
    p: [f32; 3],
) -> SurfaceType {
    if interim
        && base == SurfaceType::Asphalt
        && weather.conditions_at(p).wetness >= weather.data().surfaces.interim_wet_threshold
    {
        SurfaceType::WetAsphalt
    } else {
        base
    }
}

/// The weather-limited line of sight shared by both adapters (range first: cheap).
fn weathered_los(
    world: &dyn WorldQuery,
    weather: &WeatherSim,
    limit: bool,
    a: [f32; 3],
    b: [f32; 3],
) -> bool {
    if limit {
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let r = weather.visibility_m(a);
        if d[0] * d[0] + d[1] * d[1] + d[2] * d[2] > r * r {
            return false;
        }
    }
    world.line_of_sight(a, b)
}

impl WorldQuery for WithWeather<'_> {
    fn surface_at(&self, p: [f32; 3]) -> SurfaceType {
        weathered_surface(
            self.world.surface_at(p),
            self.weather,
            self.interim_surface,
            p,
        )
    }

    fn speed_limit_at(&self, p: [f32; 3]) -> Option<f32> {
        self.world.speed_limit_at(p)
    }

    fn signal_state_ahead(&self, p: [f32; 3], heading: [f32; 3]) -> Option<SignalState> {
        self.world.signal_state_ahead(p, heading)
    }

    fn line_of_sight(&self, a: [f32; 3], b: [f32; 3]) -> bool {
        weathered_los(self.world, self.weather, self.weather_los, a, b)
    }

    fn time_of_day(&self) -> TimeOfDay {
        self.world.time_of_day()
    }
}

/// Owning variant of [`WithWeather`]: holds the world and the weather by value.
#[derive(Debug, Clone)]
pub struct Weathered<W: WorldQuery> {
    /// The underlying world.
    pub world: W,
    /// The weather.
    pub weather: WeatherSim,
    /// Report wet asphalt as [`SurfaceType::WetAsphalt`] (interim; see the module docs).
    pub interim_surface: bool,
    /// Limit `line_of_sight` to the weather sight range.
    pub weather_los: bool,
}

impl<W: WorldQuery> Weathered<W> {
    /// Wrap `world` with interim surface remapping on.
    pub fn new(world: W, weather: WeatherSim) -> Self {
        Self {
            world,
            weather,
            interim_surface: true,
            weather_los: true,
        }
    }

    /// Weather at `p` (what `WorldQuery::weather_at` will return).
    pub fn weather_at(&self, p: [f32; 3]) -> LocalConditions {
        self.weather.conditions_at(p)
    }

    /// Visibility from `p`, m ([`WeatherSim::visibility_m`]): the cap `line_of_sight` applies.
    pub fn visibility_m(&self, p: [f32; 3]) -> f32 {
        self.weather.visibility_m(p)
    }
}

impl<W: WorldQuery> WorldQuery for Weathered<W> {
    fn surface_at(&self, p: [f32; 3]) -> SurfaceType {
        weathered_surface(
            self.world.surface_at(p),
            &self.weather,
            self.interim_surface,
            p,
        )
    }

    fn speed_limit_at(&self, p: [f32; 3]) -> Option<f32> {
        self.world.speed_limit_at(p)
    }

    fn signal_state_ahead(&self, p: [f32; 3], heading: [f32; 3]) -> Option<SignalState> {
        self.world.signal_state_ahead(p, heading)
    }

    fn line_of_sight(&self, a: [f32; 3], b: [f32; 3]) -> bool {
        weathered_los(&self.world, &self.weather, self.weather_los, a, b)
    }

    fn time_of_day(&self) -> TimeOfDay {
        self.world.time_of_day()
    }
}
