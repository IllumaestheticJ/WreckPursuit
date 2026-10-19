//! Weather composed with `wp_sim` from the outside: the code the lead moves into `wp_sim` once
//! contract v0.3.0 (request W-1, `WorldQuery::weather_at`) lands.
//!
//! Run: `cargo run --release -p wp_weather --example weather_in_sim`
//! (also run on every `cargo test` by `tests/weather_in_sim.rs`, which checks the numbers).
//!
//! What it does:
//! 1. [`WeatherLink`] is everything `wp_sim::Sim` needs: build the wetness grid over the world,
//!    step the weather once per physics tick (after `World::step`, before the vehicles), hand
//!    vehicles and police a weathered [`WorldQuery`], hash the weather, and sample the
//!    conditions under each wheel.
//! 2. A real `wp_sim::Sim` brakes the `debug_sedan` from 100 km/h while a `WeatherLink` (heavy
//!    rain, soaked road) is stepped at every sim tick from `sim.world()` /
//!    `sim.vehicle_states()`, and the conditions at each wheel are sampled. `Sim` cannot take
//!    the weathered query from outside yet, so that car still brakes on a dry road.
//! 3. The grip effect: the same car, inputs and tick sequence on `wp_vehicle::rig::Rig` (the
//!    same Rapier + `RapierGround` + `Vehicle::step_with_ground` path `Sim::step_fixed` runs),
//!    over a real `wp_world::World`, once dry and once through the weathered query.
//!    The dry rig matches the dry `Sim` run; the wet rig shows what `Sim` does after the move.
//! 4. Police sight: `visibility_m` per regime (the cap on every police sight range).

use wp_contracts::{
    load_car, AssistProfile, LoadedCar, SignalState, SimEvent, SurfaceType, TimeOfDay, VehicleDef,
    VehicleInput, VehicleState, WorldQuery, PHYSICS_DT,
};
use wp_vehicle::rig::Rig;
use wp_vehicle::VehicleExtras;
use wp_weather::{
    GridSpec, LocalConditions, Regime, SurfaceTable, TireProfile, WeatherConfig, WeatherData,
    WeatherKind, WeatherSim, WithWeather,
};
use wp_world::World;

// ------------------------------------------------------------------------------------------
// The part that moves into wp_sim.
// ------------------------------------------------------------------------------------------

/// Weather as `wp_sim::Sim` will own it: `Sim { weather: Option<WeatherLink>, .. }`.
///
/// Mapping onto `Sim` (see `docs/integration/weather.md`, step 1):
/// - `Sim::set_weather(cfg)` → [`WeatherLink::new`]; `Sim::set_world` → [`WeatherLink::rebuild`].
/// - `Sim::step_fixed`, right after `self.world.step(dt, &mut out)` → [`WeatherLink::step`].
/// - vehicle loop and `self.police.step(..)` take [`WeatherLink::query`] instead of `&self.world`.
/// - `Sim::state_hash` → [`WeatherLink::hash_into`] after the world hash.
pub struct WeatherLink {
    cfg: WeatherConfig,
    weather: WeatherSim,
    /// Scratch for the vehicle + traffic positions passed to the weather each tick (busy lanes
    /// dry first); reused so the tick does not allocate.
    positions: Vec<[f32; 3]>,
}

impl WeatherLink {
    /// Weather over `world`: an 8 m wetness grid covering the road network plus 50 m (a 2 km
    /// square around the origin for a world without roads, e.g. `World::flat`).
    pub fn new(cfg: WeatherConfig, world: &World) -> Self {
        let weather = Self::build(&cfg, world);
        Self {
            cfg,
            weather,
            positions: Vec::new(),
        }
    }

    fn build(cfg: &WeatherConfig, world: &World) -> WeatherSim {
        let grid = GridSpec::around(world.network().nodes.iter().map(|n| &n.pos), 50.0, 8.0);
        WeatherSim::from_world(WeatherData::embedded(), cfg.clone(), grid, world)
    }

    /// Rebuild the grid for a new world with the same config (`Sim::set_world`).
    pub fn rebuild(&mut self, world: &World) {
        self.weather = Self::build(&self.cfg, world);
    }

    /// One physics tick. Call after `World::step` (so the clock is current) and before the
    /// vehicles step. `vehicles` are the sim vehicles' positions; traffic agents are added here.
    pub fn step(&mut self, world: &World, vehicles: impl IntoIterator<Item = [f32; 3]>) {
        self.positions.clear();
        self.positions.extend(vehicles);
        self.positions.extend(world.agents().map(|a| a.pos));
        self.weather.step(
            PHYSICS_DT,
            world.time_of_day(),
            world.clock.ratio,
            &self.positions,
        );
    }

    /// The world as vehicles and police must see it this tick: wet asphalt reaches the tires
    /// (interim `WetAsphalt` remap until W-1) and `line_of_sight` is cut at `visibility_m`.
    pub fn query<'a>(&'a self, world: &'a dyn WorldQuery) -> WithWeather<'a> {
        WithWeather::new(world, &self.weather)
    }

    /// The weather (renderer, HUD, economy, police modifiers).
    pub fn weather(&self) -> &WeatherSim {
        &self.weather
    }

    /// Mutable weather (scenario setup, debug UI: `force`, `set_all_water_mm`).
    pub fn weather_mut(&mut self) -> &mut WeatherSim {
        &mut self.weather
    }

    /// Fold the weather into the sim state hash.
    pub fn hash_into(&self, h: &mut wp_math::StateHasher) {
        self.weather.hash_into(h);
    }

    /// Conditions under each wheel of a car (FL, FR, RL, RR) and the grip multipliers they give.
    /// `world` is the **dry** world (its `surface_at` is the base surface).
    pub fn wheels(
        &self,
        world: &dyn WorldQuery,
        def: &VehicleDef,
        tires: &[TireProfile; 4],
        state: &VehicleState,
    ) -> [WheelWeather; 4] {
        let v = state.lin_vel;
        let speed = (v[0] * v[0] + v[1] * v[1]).sqrt();
        let wet_q = self.query(world);
        std::array::from_fn(|i| {
            let p = add(state.pos, rotate(state.rot, def.wheels[i].position_m));
            let conditions = self.weather.conditions_at(p);
            let base_surface = world.surface_at(p);
            WheelWeather {
                pos: p,
                conditions,
                base_surface,
                interim_surface: wet_q.surface_at(p),
                full_grip_mult: SurfaceTable::embedded().wheel_grip_multiplier(
                    &conditions,
                    base_surface,
                    false,
                    speed,
                    &tires[i],
                ),
            }
        })
    }
}

/// What the weather does under one wheel.
#[derive(Debug, Clone, Copy)]
pub struct WheelWeather {
    /// Wheel mount position in world space, m (Z ignored by the weather grid).
    pub pos: [f32; 3],
    /// Weather at the wheel (the W-1 `weather_at` payload).
    pub conditions: LocalConditions,
    /// The world's own (dry) surface.
    pub base_surface: SurfaceType,
    /// What the tires get today through [`WeatherLink::query`] (`WetAsphalt` when soaked).
    pub interim_surface: SurfaceType,
    /// The full model's multiplier (wetness, ice, aquaplaning) that `wp_vehicle` applies after
    /// W-1 (step 2 in the integration doc); 1.0 on a dry road.
    pub full_grip_mult: f32,
}

/// Per-wheel aquaplaning/tread profile (cache once per car, like `wp_vehicle` will).
pub fn tire_profiles(car: &LoadedCar) -> [TireProfile; 4] {
    std::array::from_fn(|i| {
        let w = &car.def.wheels[i];
        TireProfile::from_tire_def(&car.tires[&w.tire_id], w.width_m)
    })
}

/// Rotate `v` by the unit quaternion `q = [x, y, z, w]`.
fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q;
    // t = 2 (q.xyz × v); v' = v + w t + q.xyz × t
    let t = [
        2.0 * (y * v[2] - z * v[1]),
        2.0 * (z * v[0] - x * v[2]),
        2.0 * (x * v[1] - y * v[0]),
    ];
    [
        v[0] + w * t[0] + (y * t[2] - z * t[1]),
        v[1] + w * t[1] + (z * t[0] - x * t[2]),
        v[2] + w * t[2] + (x * t[1] - y * t[0]),
    ]
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

// ------------------------------------------------------------------------------------------
// The demonstration.
// ------------------------------------------------------------------------------------------

/// Brake from this speed (100 km/h), with the AI assists (ABS on), as the simcli scenarios do.
pub const V0_MPS: f32 = 27.8;
/// Speed below which the car counts as stopped.
const STOP_MPS: f32 = 0.5;
/// Ticks to settle on the springs before the launch.
const SETTLE_TICKS: u32 = 240;
/// Give up after this many braking ticks (20 s).
const MAX_BRAKE_TICKS: u32 = 240 * 20;

/// The wet case: a downpour held fixed on a road already carrying 1.5 mm of water (the same
/// weather as `apps/simcli/scenarios/weather/wet_brake_100_0.ron.pending`).
pub fn soaked() -> WeatherConfig {
    WeatherConfig {
        seed: 1,
        locked: true,
        start: Regime::new(WeatherKind::HeavyRain, 1.0),
        start_water_mm: 1.5,
        ..WeatherConfig::default()
    }
}

/// One braking run.
#[derive(Debug, Clone, Copy)]
pub struct Stop {
    /// Distance from brake application to `STOP_MPS`, m.
    pub distance_m: f32,
    /// Time from brake application to `STOP_MPS`, s.
    pub time_s: f32,
}

/// Everything the example measures.
#[derive(Debug, Clone)]
pub struct Report {
    /// `wp_sim::Sim`, dry (what the game does today).
    pub sim_dry: Stop,
    /// Conditions under the `Sim` car's wheels at brake application, from the `WeatherLink`
    /// stepped alongside the `Sim`.
    pub sim_wheels: [WheelWeather; 4],
    /// Weather state hash after the `Sim` run (determinism check).
    pub sim_weather_hash: u64,
    /// Rig on a `wp_world::World`, no weather (must match `sim_dry`).
    pub rig_dry: Stop,
    /// Rig on the same world through `WeatherLink::query`, clear weather (must equal `rig_dry`).
    pub rig_clear: Stop,
    /// Rig through `WeatherLink::query`, soaked road (what `Sim` does after the move).
    pub rig_wet: Stop,
    /// Police sight cap per regime, m.
    pub visibility_m: Vec<(WeatherKind, f32)>,
}

fn car() -> LoadedCar {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    load_car(root.join("cars/debug_sedan.json")).expect("debug_sedan loads")
}

fn noon() -> TimeOfDay {
    TimeOfDay::from_hours(12.0)
}

fn horizontal_speed(s: &VehicleState) -> f32 {
    (s.lin_vel[0] * s.lin_vel[0] + s.lin_vel[1] * s.lin_vel[1]).sqrt()
}

fn dist_xy(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

fn full_brake() -> VehicleInput {
    VehicleInput {
        brake: 1.0,
        ..VehicleInput::default()
    }
}

/// Part 2: the real `Sim`, with a `WeatherLink` stepped at every sim tick beside it.
fn sim_run(car: &LoadedCar) -> (Stop, [WheelWeather; 4], u64) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let mode = wp_contracts::load_mode_table(root.join("modes/arcade.toml")).expect("mode");
    let mut sim = wp_sim::Sim::new(wp_sim::SimConfig {
        seed: 1,
        mode,
        time_of_day: noon(),
        time_ratio: 1.0,
    });
    let id = sim.spawn_vehicle(car, [0.0; 3], 0.0).expect("spawn");
    sim.set_assists(id, AssistProfile::ai()).expect("assists");
    let mut link = WeatherLink::new(soaked(), sim.world());
    let tires = tire_profiles(car);

    // One tick of the composed loop: Sim steps (world → vehicles → physics → police), then the
    // weather steps from the sim's own world and vehicles. Inside wp_sim the weather step sits
    // between `World::step` and the vehicles; from outside, one tick later is the closest.
    let tick = |sim: &mut wp_sim::Sim, link: &mut WeatherLink| {
        sim.step_fixed();
        let positions = sim.vehicle_states().into_iter().map(|(_, s)| s.pos);
        link.step(sim.world(), positions.collect::<Vec<_>>());
    };

    for _ in 0..SETTLE_TICKS {
        tick(&mut sim, &mut link);
    }
    let z = sim.vehicle_states()[0].1.pos[2];
    sim.place_vehicle(id, [0.0, 0.0, z], 0.0, V0_MPS)
        .expect("place");
    sim.set_input(id, full_brake()).expect("input");
    let start = sim.vehicle_states()[0].1.clone();
    let wheels = link.wheels(sim.world(), &car.def, &tires, &start);
    let mut ticks = 0;
    loop {
        tick(&mut sim, &mut link);
        ticks += 1;
        if horizontal_speed(&sim.vehicle_states()[0].1) < STOP_MPS || ticks >= MAX_BRAKE_TICKS {
            break;
        }
    }
    let end = sim.vehicle_states()[0].1.pos;
    let stop = Stop {
        distance_m: dist_xy(start.pos, end),
        time_s: ticks as f32 * PHYSICS_DT,
    };
    // `Sim::state_hash` folds the weather in after the world hash, like this.
    let mut h = wp_math::StateHasher::new();
    link.hash_into(&mut h);
    (stop, wheels, h.finish())
}

/// A `wp_world::World` with weather, as one `WorldQuery` value (what `Rig<W>` needs). Inside
/// `wp_sim` this is just `link.query(&self.world)`; here the two have to live in one struct.
struct WorldWithWeather {
    world: World,
    link: Option<WeatherLink>,
}

impl WorldWithWeather {
    fn new(cfg: Option<WeatherConfig>) -> Self {
        let world = World::flat(noon(), 1.0);
        let link = cfg.map(|c| WeatherLink::new(c, &world));
        Self { world, link }
    }

    /// The `Sim::step_fixed` order: world, then weather, then (by the caller) the vehicles.
    fn step(&mut self, vehicle_pos: [f32; 3]) {
        let mut out: Vec<SimEvent> = Vec::new();
        self.world.step(PHYSICS_DT, &mut out);
        if let Some(link) = &mut self.link {
            link.step(&self.world, [vehicle_pos]);
        }
    }

    /// Run `f` on the world as the vehicles see it (weathered when weather is on).
    fn with<R>(&self, f: impl FnOnce(&dyn WorldQuery) -> R) -> R {
        match &self.link {
            Some(l) => f(&l.query(&self.world)),
            None => f(&self.world),
        }
    }
}

impl WorldQuery for WorldWithWeather {
    fn surface_at(&self, p: [f32; 3]) -> SurfaceType {
        self.with(|q| q.surface_at(p))
    }
    fn speed_limit_at(&self, p: [f32; 3]) -> Option<f32> {
        self.with(|q| q.speed_limit_at(p))
    }
    fn signal_state_ahead(&self, p: [f32; 3], heading: [f32; 3]) -> Option<SignalState> {
        self.with(|q| q.signal_state_ahead(p, heading))
    }
    fn line_of_sight(&self, a: [f32; 3], b: [f32; 3]) -> bool {
        self.with(|q| q.line_of_sight(a, b))
    }
    fn time_of_day(&self) -> TimeOfDay {
        self.with(|q| q.time_of_day())
    }
}

/// Part 3: the same car on the rig (Sim's vehicle path), dry or weathered.
fn rig_run(car: &LoadedCar, weather: Option<WeatherConfig>) -> Stop {
    let world = WorldWithWeather::new(weather);
    let mut rig = Rig::with_world(car, VehicleExtras::default(), world, [0.0; 3], 0.0);
    let assists = AssistProfile::ai();
    let tick = |rig: &mut Rig<WorldWithWeather>, input: &VehicleInput| {
        let pos = rig.state().pos;
        rig.world.step(pos);
        rig.step(input, &assists);
    };
    for _ in 0..SETTLE_TICKS {
        tick(&mut rig, &VehicleInput::default());
    }
    rig.teleport([0.0, 0.0, rig.state().pos[2]], 0.0, V0_MPS, 0.0);
    let start = rig.state().pos;
    let mut ticks = 0;
    loop {
        tick(&mut rig, &full_brake());
        ticks += 1;
        if horizontal_speed(&rig.state()) < STOP_MPS || ticks >= MAX_BRAKE_TICKS {
            break;
        }
    }
    Stop {
        distance_m: dist_xy(start, rig.state().pos),
        time_s: ticks as f32 * PHYSICS_DT,
    }
}

/// Part 4: police sight cap per regime.
fn visibility() -> Vec<(WeatherKind, f32)> {
    let world = World::flat(noon(), 1.0);
    let cfg = WeatherConfig {
        locked: true,
        ..WeatherConfig::default()
    };
    let mut link = WeatherLink::new(cfg, &world);
    // `Sim::set_world` after `Sim::set_weather`: same config, grid rebuilt over the new world.
    link.rebuild(&World::flat(noon(), 1.0));
    [
        WeatherKind::Clear,
        WeatherKind::Overcast,
        WeatherKind::LightRain,
        WeatherKind::HeavyRain,
        WeatherKind::Fog,
    ]
    .into_iter()
    .map(|k| {
        link.weather_mut().force(Regime::new(k, 1.0), true);
        // A quarter second of ticks: one internal weather update.
        for _ in 0..60 {
            link.step(&world, []);
        }
        (k, link.weather().visibility_m([0.0, 0.0, 1.0]))
    })
    .collect()
}

/// Run every part and return the measurements.
pub fn run() -> Report {
    let car = car();
    let (sim_dry, sim_wheels, sim_weather_hash) = sim_run(&car);
    let clear = WeatherConfig {
        locked: true,
        ..WeatherConfig::default()
    };
    Report {
        sim_dry,
        sim_wheels,
        sim_weather_hash,
        rig_dry: rig_run(&car, None),
        rig_clear: rig_run(&car, Some(clear)),
        rig_wet: rig_run(&car, Some(soaked())),
        visibility_m: visibility(),
    }
}

#[allow(dead_code)] // the test harness includes this file as a module and calls `run` only
fn main() {
    let r = run();
    println!("debug_sedan, 100→0 km/h, AI assists (ABS):");
    let line = |name: &str, s: &Stop| {
        println!("  {name:<38} {:>5.1} m  {:>4.2} s", s.distance_m, s.time_s)
    };
    line("wp_sim::Sim (dry, today)", &r.sim_dry);
    line("rig, wp_world::World, no weather", &r.rig_dry);
    line("rig, weather on, clear", &r.rig_clear);
    line("rig, weather on, soaked (= Sim after)", &r.rig_wet);
    println!(
        "  wet / dry = {:.2}×",
        r.rig_wet.distance_m / r.rig_dry.distance_m
    );
    println!("wheels of the Sim car at brake application (WeatherLink stepped with the Sim):");
    for (name, w) in ["FL", "FR", "RL", "RR"].iter().zip(&r.sim_wheels) {
        println!(
            "  {name} at ({:6.2}, {:6.2}): water {:.2} mm, wetness {:.2}, standing {:.2} mm, \
             road {:.1} °C, surface {:?} → {:?}, full-model grip ×{:.2}",
            w.pos[0],
            w.pos[1],
            w.conditions.water_mm,
            w.conditions.wetness,
            w.conditions.standing_water_mm,
            w.conditions.road_temp_c,
            w.base_surface,
            w.interim_surface,
            w.full_grip_mult,
        );
    }
    println!(
        "weather state hash after the Sim run: {:#018x}",
        r.sim_weather_hash
    );
    println!("police sight cap (visibility_m):");
    for (k, v) in &r.visibility_m {
        println!("  {k:?}: {v:.0} m");
    }
}
