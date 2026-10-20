# Weather integration notes

- **Workstream / prompt:** 14 — weather & road conditions (machine K)
- **Branch:** `ws/weather`
- **Crates / folders owned:** `crates/wp_weather`, `data/weather/`
- **Contract version built against:** 0.2.0 (`main` + lead branch `claude/laughing-newton-0xqr6q`
  merged 2026-10-19, round 6b); builds so it drops onto v0.3.0 (W-1).

Summary: `wp_weather` is self-contained (deps: `wp_contracts`, `wp_math`, serde/toml). Nothing
calls it yet. The quickest path to "rain makes cars slide" is **step 1 below (lead, `wp_sim`
only)**. It needs no contract change, because the `WithWeather` adapter reports wet asphalt as
`SurfaceType::WetAsphalt`, which every tire already has a multiplier for. The full model
(per-surface wet grip, ice, markings, aquaplaning) arrives with W-1 + the `wp_vehicle` hook.

## Exposes

All in `wp_weather` (details: `crates/wp_weather/README.md`).

**Lifecycle**
- `WeatherData::embedded() -> WeatherData` (the three TOMLs compiled in; wasm-friendly) ·
  `WeatherData::load(data_root: &Path) -> Result<WeatherData, WeatherDataError>`.
- `WeatherConfig { seed: u64, season: Season, start: Regime, start_water_mm: f32,
  update_interval_s: f32 /* 0.25 */, locked: bool }`.
- `GridSpec::covering(min: [f32; 2], max: [f32; 2], cell_m: f32) -> GridSpec` (use 8 m).
- `WeatherSim::from_world(data, cfg, grid, &dyn WorldQuery) -> WeatherSim` (samples
  `surface_at` per cell centre once) · `WeatherSim::uniform(data, cfg, grid, SurfaceType)`.
- `WeatherSim::step(&mut self, dt_s: f32, time: TimeOfDay, time_ratio: f32, vehicles: &[[f32; 3]])`
  — once per physics tick; internal update every `update_interval_s` of sim time.
- `WeatherSim::force(Regime, immediate: bool)`, `set_locked(bool)`, `set_all_water_mm(f32)`
  (scenario setup / debug UI / a future captain tool).
- `WeatherSim::hash_into(&mut StateHasher)`, `state_hash() -> u64`.
- `WeatherSim::drain_changes() -> Vec<WeatherChange { from: Regime, to: Regime, forced: bool }>`.
- `WeatherSim::snapshot() -> WeatherSnapshot` / `apply_snapshot(&WeatherSnapshot) -> bool`.

**Queries**
- `WeatherSim::conditions_at(p: [f32; 3]) -> LocalConditions` — `kind, intensity,
  rain_mm_per_h, water_mm, wetness (0..1), standing_water_mm, air_temp_c, road_temp_c,
  fog_per_m, sight_multiplier (0..1), wind_mps: [f32; 2]`. This is the proposed
  `WorldQuery::weather_at` payload (W-1).
- `WeatherSim::state() -> &WeatherState` — `kind, intensity, prev, blend, wetness_by_cell:
  WetnessGrid, fog_density, rain_mm_per_h, cloud_cover, temperature_c, road_temp_c, wind, …`.
- `WeatherSim::{police_modifiers, traffic_modifiers, economy_modifiers, modifiers,
  sight_multiplier, render_params, puddle_cells(min_depth_mm)}`.
- `WithWeather::new(&dyn WorldQuery, &WeatherSim)` — a `WorldQuery` (see step 1): wet asphalt
  → `WetAsphalt`; `line_of_sight(a, b)` = `|b − a| ≤ sight_range_m() && world.line_of_sight(a, b)`
  (`weather_los = false` for the geometric test alone). `Weathered<W>` is the owning variant
  (e.g. `wp_vehicle::rig::Rig<Weathered<FlatWorld>>`).
- `WeatherSim::sight_range_m() -> f32` — 800 m × sight multiplier (800 clear, ~130 thick fog).
- **`WeatherSim::visibility_m(p: [f32; 3]) -> f32`** (round 6b) — how far an observer at `p`
  can see through fog/rain, m: 800 clear/overcast, 750 light rain, 480 heavy rain, 130 thick
  fog (night not included). Same value as `LocalConditions::visibility_m` (new field, so it
  travels with W-1's `weather_at`), `WithWeather::visibility_m(p)` / `Weathered::visibility_m(p)`,
  and the cut the adapters' `line_of_sight` applies (at the observer `a`). Uniform sky today;
  takes a point so callers do not change when fog banks arrive. `CLEAR_VISIBILITY_M = 800.0`
  (= `LocalConditions::DRY.visibility_m`).
- `GridSpec::around(points, margin_m, cell_m)` — grid over a point set's bbox (road nodes).
- `WeatherSim::can_see(&self, world: &dyn WorldQuery, a, b, base_sight_m) -> bool` —
  `|b − a| ≤ base_sight_m × sight_multiplier && world.line_of_sight(a, b)` (distance first, so
  fog skips the occluder query for far targets). Night not included.

**Grip (pure functions on `SurfaceTable`; `SurfaceTable::embedded()` is a `&'static`)**
- `surface_grip_multiplier(surface: SurfaceType, wetness: f32, road_temp_c: f32) -> f32`
- `surface_grip_multiplier_for(surface, wetness, road_temp_c, tire_wet_ratio: f32) -> f32`
- `marking_grip_multiplier(wetness, road_temp_c) -> f32`
- `aquaplane_onset_mps(standing_mm, &TireProfile) -> f32` ·
  `aquaplane_amount(standing_mm, speed_mps, &TireProfile) -> f32 (0..1)` ·
  `aquaplane_grip_multiplier(standing_mm, speed_mps, &TireProfile) -> f32`
- `wheel_grip_multiplier(&LocalConditions, surface, on_marking: bool, speed_mps, &TireProfile) -> f32`
  — the single per-wheel call; result is 1.0 dry, multiplies the tire's own
  `surface_multipliers[surface]`.
- `TireProfile { width_m, treaded, wet_ratio }`, `TireProfile::from_tire_def(&TireDef, wheel_width_m)`.
- `wp_weather::spray_intensity(water_mm, speed_mps) -> f32 (0..1)` (renderer, audio 13).

**Data** — `data/weather/climate.toml` (Markov rows per season, phase bias per `DayPhase`,
regime parameters), `surfaces.toml` (hydrology + grip + aquaplaning per `SurfaceType`),
`effects.toml` (visibility, police/traffic/economy multipliers). Every file has `id` == stem;
the loader validates ranges (`deny_unknown_fields`). No JSON Schema is generated for them (they
are not contract types); the crate's tests check disk == embedded and that bad values are
rejected.

## Hook points (exact calls each owner should make)

### 1. `wp_sim` (lead) — makes weather reach tires and police end to end; no contract change
**Ready-to-move code (round 6b):** `crates/wp_weather/examples/weather_in_sim.rs` →
`WeatherLink` is exactly what `Sim` owns (`new` = `set_weather`, `rebuild` = `set_world`,
`step` = after `World::step`, `query` = what vehicles and police get, `hash_into` = state hash,
`wheels` = conditions under each wheel). The example drives the real `wp_sim::Sim` with a
`WeatherLink` stepped at every sim tick and samples each wheel, then shows the grip effect on
`Rig` (the same Rapier/`step_with_ground` path `Sim` runs) over a real `wp_world::World`:

| debug_sedan, 100→0 km/h, ABS | distance | time |
|---|---|---|
| `wp_sim::Sim`, dry (today) | 42.9 m | 3.09 s |
| rig on `wp_world::World`, no weather | 42.9 m | 3.09 s |
| rig, weather on, clear | 42.9 m (bit-identical) | 3.09 s |
| rig, weather on, soaked (= `Sim` after the move) | **54.1 m** | 3.93 s |

Wheels of the `Sim` car on the soaked road: water 1.5 mm, wetness 1.0, surface `Asphalt` →
`WetAsphalt` (interim), full-model multiplier ×0.70 (after W-1). `tests/weather_in_sim.rs`
runs the example on every `cargo test` and checks all of it. Run it:
`cargo run --release -p wp_weather --example weather_in_sim`. The inline version:
What it gets you (measured on the real `debug_sedan` through `wp_vehicle::rig::Rig`, the same
model `wp_sim` steps — `crates/wp_weather/tests/vehicle_grip.rs`): 100→0 km/h **42.9 m / 3.09 s
dry vs 54.1 m / 3.93 s on a soaked road**; dry weather is bit-for-bit the old dry world. Police
sight: thick fog cuts `line_of_sight` at ~130 m (800 m in clear air, longer than any police
range, so clear-weather behaviour is unchanged).

`crates/wp_sim/Cargo.toml`: `wp_weather.workspace = true` (wasm-clean: contracts, math, serde, toml).

`crates/wp_sim/src/lib.rs`:
```rust
use wp_contracts::WorldQuery; // not imported in lib.rs today
use wp_weather::{GridSpec, WeatherConfig, WeatherData, WeatherSim, WithWeather};

// struct Sim — two new fields:
weather: Option<(WeatherConfig, WeatherSim)>,   // None = no weather (today's behaviour)
weather_positions: Vec<[f32; 3]>,               // scratch, reused every tick

/// Turn weather on (or reconfigure it). Builds the wetness grid over the current world's road
/// network; `set_world` rebuilds it with the same config, so call order does not matter.
pub fn set_weather(&mut self, cfg: WeatherConfig) {
    let grid = GridSpec::around(self.world.network().nodes.iter().map(|n| &n.pos), 50.0, 8.0);
    let w = WeatherSim::from_world(WeatherData::embedded(), cfg.clone(), grid, &self.world);
    self.weather = Some((cfg, w));
}
/// The weather, if enabled (renderer, HUD, economy read it).
pub fn weather(&self) -> Option<&WeatherSim> { self.weather.as_ref().map(|(_, w)| w) }
/// Mutable weather (debug UI / scenario setup: `force`, `set_all_water_mm`).
pub fn weather_mut(&mut self) -> Option<&mut WeatherSim> { self.weather.as_mut().map(|(_, w)| w) }

// set_world(), at the end:
if let Some((cfg, _)) = self.weather.take() { self.set_weather(cfg); }

// step_fixed(), right after the `if sys.world { … self.world.step(dt, &mut out); }` block:
if let Some((_, w)) = &mut self.weather {
    self.weather_positions.clear();
    self.weather_positions.extend(self.vehicles.iter().map(|v| v.vehicle.state().pos));
    self.weather_positions.extend(self.world.agents().map(|a| a.pos));
    w.step(dt, self.world.time_of_day(), self.world.clock.ratio, &self.weather_positions);
}

// step_fixed(), vehicle loop (`if sys.vehicle`): build the query once, before `for slot in …`
let wx = self.weather.as_ref().map(|(_, w)| WithWeather::new(&self.world, w));
let world_q: &dyn WorldQuery = match &wx { Some(q) => q, None => &self.world };
//   …and pass `world_q` instead of `&self.world` to `slot.vehicle.step_with_ground(..)`.
//   (Disjoint field borrows: self.world / self.weather shared, self.vehicles mutable — compiles.)

// step_fixed(), police (`if sys.police`): same adapter so LOS gets the fog/rain range
let wx = self.weather.as_ref().map(|(_, w)| WithWeather::new(&self.world, w));
let world_q: &dyn WorldQuery = match &wx { Some(q) => q, None => &self.world };
self.police.step(world_q, &racers, &tick_events, dt, &mut out);

// state_hash(): after the world hash
if let Some((_, w)) = &self.weather { w.hash_into(&mut h); }
```
Hashes change only for sims that call `set_weather`, so no existing golden moves. Traffic
agents' own vehicles inside `wp_world` stay on the dry world until D forwards weather (step 3).
Cost ≈ 0.04 ms per 60 Hz frame (README → Performance). Interim limits: wetness below
`interim_wet_threshold` (0.5) does not reach the tires, and ice / markings / aquaplaning need
W-1 + step 2.

### 1b. simcli (I) — scenario field W-7: exact patch
Two parts. **Part A is I's and can merge now** (parser only, no behaviour change: a scenario
with `weather:` runs dry and gets a warning). **Part B is one line** and lands with the lead's
`Sim::set_weather` (step 1); only then rename the pending wet scenario.

**Part A** — `git apply` against the current lead branch (checked with `git apply --check`;
`Cargo.lock` hunk left out, cargo regenerates it). Touches `apps/simcli/{Cargo.toml,
SCENARIOS.md, src/{scenario,run,fuzz,lib}.rs}` and one struct literal in
`crates/wp_wasm/src/scenario.rs` (`weather: None`, C's file). `wp_weather` is wasm-clean
(contracts, math, serde, toml, thiserror), so `wp_wasm → simcli → wp_weather` is fine. Adds
`WeatherSpec` (+ `WeatherSpec::config(scenario_seed) -> wp_weather::WeatherConfig`),
validation (`intensity` 0..=1, `water_mm` ≥ 0, unknown fields rejected) and a parser test.
Extract: `awk '/^```diff$/{f=1;next} /^```$/{f=0} f' docs/integration/weather.md > w7.patch`
(the only `diff` block in this file).

```diff
diff --git a/apps/simcli/Cargo.toml b/apps/simcli/Cargo.toml
index 78affaa..6e14f70 100644
--- a/apps/simcli/Cargo.toml
+++ b/apps/simcli/Cargo.toml
@@ -13,6 +13,7 @@ wp_net.workspace = true
 wp_sim.workspace = true
 wp_police.workspace = true
 wp_world.workspace = true
+wp_weather.workspace = true
 serde.workspace = true
 serde_json.workspace = true
 ron.workspace = true
diff --git a/apps/simcli/SCENARIOS.md b/apps/simcli/SCENARIOS.md
index 9fa2637..e6d3cde 100644
--- a/apps/simcli/SCENARIOS.md
+++ b/apps/simcli/SCENARIOS.md
@@ -26,6 +26,7 @@ Parsing uses RON with `implicit_some`, so optional fields are written plainly
     police_cars: true,               // AI police units patrolling the city (default true)
     systems: "vehicle,damage,world", // optional; default all. --systems overrides. "net" adds the snapshot round trip
     barriers: [(pos: (0.0, 20.0, 0.75), size: (8.0, 1.0, 1.5), yaw: 0.0)],  // static boxes (rigid walls)
+    weather: (kind: HeavyRain, intensity: 1.0, water_mm: 1.5, locked: true), // optional; see below
     entities: [ … ],                 // spawn order = entity id order (1, 2, 3, …); may be empty
     duration_s: 8.0,
     sample_hz: 60.0,                 // metrics.csv rate (default 60)
@@ -64,6 +65,14 @@ wanted levels.
 `barriers` are static boxes (`Sim::add_static_box`): centre `pos`, full `size` `[x, y, z]`,
 `yaw` about +Z. Contacts with them reach damage as static world geometry.
 
+`weather` (optional; omitted = no weather system, dry and clear, as before) turns on
+`wp_weather`: `kind` is `Clear | Overcast | LightRain | HeavyRain | Fog`, `intensity` 0..=1
+(default 1), `water_mm` the water film on every road cell at the first tick (default 0 = dry;
+1.5 = soaked), `locked` holds the regime for the whole run (default true), `season`
+(`Spring | Summer | Autumn | Winter`, default `Autumn`) and `seed` (default: the scenario seed)
+drive the schedule when unlocked. Wet roads cost grip and fog/rain shorten police sight.
+Examples: `scenarios/weather/`.
+
 Features a crate does not support yet (police presence override, `follow_route`,
 `ai_driver`) run anyway and add a **warning** to the summary; `--strict` turns warnings into
 failures.
diff --git a/apps/simcli/src/fuzz.rs b/apps/simcli/src/fuzz.rs
index 3c2cd61..df91a53 100644
--- a/apps/simcli/src/fuzz.rs
+++ b/apps/simcli/src/fuzz.rs
@@ -130,6 +130,7 @@ pub fn gen_case(cfg: &FuzzConfig, index: u64) -> Scenario {
         parked_cars: true,
         police_cars: true,
         systems: None,
+        weather: None,
         entities,
         duration_s: cfg.case_seconds,
         sample_hz: 10.0,
diff --git a/apps/simcli/src/lib.rs b/apps/simcli/src/lib.rs
index eaa5308..b7c077c 100644
--- a/apps/simcli/src/lib.rs
+++ b/apps/simcli/src/lib.rs
@@ -36,7 +36,7 @@ pub use run::{
     run, run_with_hook, RunOptions, RunOutput, RunSummary, Selection, TickView,
     PURSUIT_STARTED_KIND,
 };
-pub use scenario::Scenario;
+pub use scenario::{Scenario, WeatherSpec};
 
 /// Runner errors.
 #[derive(Debug, Error)]
diff --git a/apps/simcli/src/run.rs b/apps/simcli/src/run.rs
index cf63470..67ed4e4 100644
--- a/apps/simcli/src/run.rs
+++ b/apps/simcli/src/run.rs
@@ -266,6 +266,12 @@ pub fn build_sim(
         }
         None => false,
     };
+    // After the city (the wetness grid covers its road network). W-7 part B (lands with the
+    // lead's `Sim::set_weather`) replaces the warning with:
+    //     if let Some(w) = &sc.weather { sim.set_weather(w.config(sc.seed)); }
+    if sc.weather.is_some() {
+        warnings.push("weather ignored: wp_sim has no weather yet (weather.md step 1)".into());
+    }
     Ok(BuiltSim {
         sim,
         ids,
diff --git a/apps/simcli/src/scenario.rs b/apps/simcli/src/scenario.rs
index 94cd9fe..9c168e0 100644
--- a/apps/simcli/src/scenario.rs
+++ b/apps/simcli/src/scenario.rs
@@ -52,6 +52,10 @@ pub struct Scenario {
     /// Static boxes (barriers, walls) added before the first tick.
     #[serde(default, skip_serializing_if = "Vec::is_empty")]
     pub barriers: Vec<Barrier>,
+    /// Weather (workstream 14, `wp_weather`). `None` = no weather system: dry road, clear air
+    /// (every scenario written before weather existed; hashes unchanged).
+    #[serde(default, skip_serializing_if = "Option::is_none")]
+    pub weather: Option<WeatherSpec>,
     /// Vehicles to spawn, in spawn order (entity ids are allocated in this order).
     pub entities: Vec<EntitySpec>,
     /// Simulated duration, s.
@@ -75,6 +79,49 @@ fn yes() -> bool {
     true
 }
 
+fn one() -> f32 {
+    1.0
+}
+
+/// Scenario weather: a regime held from the first tick (`locked`, the default) or a seeded
+/// schedule starting from it. Maps onto `wp_weather::WeatherConfig` ([`WeatherSpec::config`]).
+#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
+#[serde(deny_unknown_fields)]
+pub struct WeatherSpec {
+    /// Starting regime: `Clear`, `Overcast`, `LightRain`, `HeavyRain` or `Fog`.
+    pub kind: wp_weather::WeatherKind,
+    /// Regime intensity, 0..=1 (default 1).
+    #[serde(default = "one")]
+    pub intensity: f32,
+    /// Water film on every road cell at the first tick, mm (default 0 = dry road; 1.5 = soaked).
+    #[serde(default)]
+    pub water_mm: f32,
+    /// Hold the regime for the whole run (default true: scenarios want fixed weather). `false`
+    /// lets the seeded schedule change it.
+    #[serde(default = "yes", skip_serializing_if = "is_true")]
+    pub locked: bool,
+    /// Season for the schedule and temperatures (default `Autumn`).
+    #[serde(default)]
+    pub season: wp_weather::Season,
+    /// Weather RNG seed; `None` = the scenario seed.
+    #[serde(default, skip_serializing_if = "Option::is_none")]
+    pub seed: Option<u64>,
+}
+
+impl WeatherSpec {
+    /// The `wp_weather` config for a scenario with master seed `scenario_seed`.
+    pub fn config(&self, scenario_seed: u64) -> wp_weather::WeatherConfig {
+        wp_weather::WeatherConfig {
+            seed: self.seed.unwrap_or(scenario_seed),
+            season: self.season,
+            start: wp_weather::Regime::new(self.kind, self.intensity),
+            start_water_mm: self.water_mm,
+            locked: self.locked,
+            ..Default::default()
+        }
+    }
+}
+
 fn is_true(v: &bool) -> bool {
     *v
 }
@@ -405,6 +452,17 @@ impl Scenario {
                 return Err(format!("barrier size {:?} must be positive", b.size));
             }
         }
+        if let Some(w) = &self.weather {
+            if !(w.intensity.is_finite() && (0.0..=1.0).contains(&w.intensity)) {
+                return Err(format!(
+                    "weather.intensity = {} must be in 0..=1",
+                    w.intensity
+                ));
+            }
+            if !(w.water_mm.is_finite() && w.water_mm >= 0.0) {
+                return Err(format!("weather.water_mm = {} must be >= 0", w.water_mm));
+            }
+        }
         if let Some(p) = self.police_presence {
             if !(p.is_finite() && p >= 0.0) {
                 return Err(format!("police_presence = {p} must be >= 0"));
@@ -532,6 +590,31 @@ mod tests {
         expect: [speed_at(entity: "a", t_s: 0.5, min: 0.0, max: 100.0)],
     )"#;
 
+    #[test]
+    fn weather_field_parses_defaults_round_trips_and_validates() {
+        let sc = Scenario::from_ron("t", MINIMAL).unwrap();
+        assert_eq!(sc.weather, None);
+        assert!(
+            !sc.to_ron().contains("weather"),
+            "no weather → field omitted"
+        );
+        let wet = MINIMAL.replace(
+            "seed: 1,",
+            "seed: 7, weather: (kind: HeavyRain, water_mm: 1.5),",
+        );
+        let sc = Scenario::from_ron("t", &wet).unwrap();
+        let w = sc.weather.clone().unwrap();
+        assert_eq!(w.kind, wp_weather::WeatherKind::HeavyRain);
+        assert_eq!((w.intensity, w.water_mm, w.locked), (1.0, 1.5, true));
+        let cfg = w.config(sc.seed);
+        assert_eq!((cfg.seed, cfg.locked, cfg.start_water_mm), (7, true, 1.5));
+        assert_eq!(Scenario::from_ron("t", &sc.to_ron()).unwrap(), sc);
+        let bad = MINIMAL.replace("seed: 1,", "seed: 1, weather: (kind: Fog, intensity: 2.0),");
+        assert!(Scenario::from_ron("t", &bad).is_err());
+        let typo = MINIMAL.replace("seed: 1,", "seed: 1, weather: (kind: Fog, wet: 1.0),");
+        assert!(Scenario::from_ron("t", &typo).is_err());
+    }
+
     #[test]
     fn minimal_scenario_parses_with_defaults() {
         let sc = Scenario::from_ron("t", MINIMAL).unwrap();
diff --git a/crates/wp_wasm/src/scenario.rs b/crates/wp_wasm/src/scenario.rs
index 03badbe..a637787 100644
--- a/crates/wp_wasm/src/scenario.rs
+++ b/crates/wp_wasm/src/scenario.rs
@@ -43,6 +43,7 @@ pub fn free_drive(car: &str, mode: DamageMode, city: bool) -> Scenario {
         police_presence: None,
         systems: None,
         barriers: Vec::new(),
+        weather: None,
         entities: vec![entity("player", car, [0.0; 3], 0.0)],
         duration_s: FREE_DRIVE_DURATION_S,
         sample_hz: 60.0,
```

**Part B** (after the lead's step 1 adds `Sim::set_weather`), in `apps/simcli/src/run.rs`
replace the warning block with:
```rust
    if let Some(w) = &sc.weather {
        sim.set_weather(w.config(sc.seed));
    }
```
then `git mv apps/simcli/scenarios/weather/wet_brake_100_0.ron.pending
apps/simcli/scenarios/weather/wet_brake_100_0.ron` and bless only it:
`cd apps/simcli && cargo run --release -p simcli -- run scenarios/weather/wet_brake_100_0.ron --bless`
(commit tagged `[bless]`).

**Verified 2026-10-19 (round 6b, machine K)** on a throwaway worktree of this branch with Part A
+ Part B + the step-1 `wp_sim` code (never committed): fmt clean, `simcli` lib tests pass,
`weather_dry_brake_100_0` PASS (27.39 m/s at 13.9 s, −0.05 at 17.2 s) and
`weather_wet_brake_100_0` PASS (27.37 at 13.9 s, **4.60 m/s at 17.2 s**, −0.05 at 18.0 s);
`simcli goldens`: every existing golden (incl. `weather_dry_brake_100_0`) still matches; the only
failure is the new `weather_wet_brake_100_0`'s missing golden (expected until Part B blesses it).

### 2. `wp_vehicle` (A) — after W-1 lands
- Cache per wheel at construction: `TireProfile::from_tire_def(&tire_def, wheel_def.width_m)`.
- In `Vehicle::step`, where `w.grip` is computed (`vehicle.rs`, `w.grip = surface_grip(&c.tire, w.surface) * …`):
  ```rust
  let local = world.weather_at(contact_point_world);
  w.grip = surface_grip(&c.tire, w.surface)
      * wp_weather::SurfaceTable::embedded()
          .wheel_grip_multiplier(&local, w.surface, /*on_marking*/ false, w.vx.abs(), &c.weather_profile)
      * eff.grip_scale[i].max(0.0) * (1.0 - 0.5 * p) * camber_grip;
  ```
  then the lead sets `WithWeather::interim_surface = false` in the same merge (otherwise wet
  loss would be counted twice).
- Optional: FFB — scale the aligning torque by `1 − aquaplane_amount(local.standing_water_mm, speed, &profile)`
  (the wheel goes light when planing). Spray: expose `spray_intensity(local.water_mm, speed)`
  per wheel in `WheelState` or let the renderer compute it from `weather_at`.
- `wp_vehicle` would add `wp_weather` as a dependency (wasm-clean, no native deps).

### 3. `wp_world` (D)
- New `World::set_weather_modifiers(&mut self, m: &wp_weather::TrafficWeatherModifiers)`, called
  by `wp_sim` once per tick before `World::step`. Apply inside traffic AI:
  - IDM desired speed `v0 *= m.desired_speed_mult`
  - IDM time headway `T *= m.time_headway_mult`
  - zone density targets `*= m.density_mult` (`density.rs` / `zone_targets`)
  - hazard reaction / sight distances `*= m.hazard_sight_multiplier`
  - civilian incident rate (when one exists) `*= m.incident_rate_mult`
- Physical traffic vehicles: step them with the same `WithWeather` adapter (or, after W-1,
  forward `weather_at` from the world the sim hands in).
- Keep `surface_at` returning the **dry base** surface — never `WetAsphalt` — weather owns
  wetness. (Today it never does; please keep it that way.)
- Nice to have: `CityMap::bounds() -> ([f32; 2], [f32; 2])` for the grid; `marking_at(p) -> bool`
  (lane markings, zebras) until W-3.
- Day/night temperature uses `TimeOfDay::night_factor()` and `phase()` only; no clock change.

### 4. `wp_police` (E)
- **After v0.3.0 (W-1), the explicit value:** `world.weather_at(observer).visibility_m`
  (until then `WeatherSim::visibility_m(observer)` / `WithWeather::visibility_m`). Use it as a cap
  on each phase range: `los_range = phase.los_range_m.min(vis)`, same for `witness_range_m`,
  `recognition_range_m` and roadblock sight checks. Clear air gives 800 m (longer than every
  police range today), so clear-weather behaviour does not move.
- **Sight / LOS — nothing to change.** Once `wp_sim` passes `WithWeather` (step 1), every
  `world.line_of_sight(..)` in `pursuit.rs` (cars at line 87 via `sees`, helicopter at line 102,
  cameras) is false beyond `weather.sight_range_m()`: ≈ 130 m in thick fog, ≈ 480 m in a
  16 mm/h downpour, 800 m clear (no change). The phase-keyed ranges at `pursuit.rs:64–67`
  (`los_range_m`, `witness_range_m`, `recognition_range_m`) stay as they are; weather caps them
  from outside.
- **Speed caps and incident rate** need a setter (E's code):
  ```rust
  // PoliceSystem
  weather: wp_weather::PoliceWeatherModifiers,   // default: all 1.0
  pub fn set_weather(&mut self, m: wp_weather::PoliceWeatherModifiers) { self.weather = m; }
  // pursuit.rs:457  self.units[i].cmd.speed_mps = lim * p.patrol_speed_factor * self.weather.patrol_speed_cap_mult;
  // pursuit.rs:623  pass `cap * self.weather.pursuit_speed_cap_mult` as pursuit_speed_cap's `cap` argument
  // dispatch: multiply the incident / crash call rate by self.weather.crash_incident_rate_mult
  ```
  and `wp_sim` calls `self.police.set_weather(w.police_modifiers())` right before
  `self.police.step(..)`. (`wp_police` → `wp_weather` dependency has no cycle; or copy the four
  `f32`s into a police-owned struct.)
- `WeatherSim::can_see(world, a, b, base_sight_m)` remains for callers that hold the
  `WeatherSim` directly (AI racers, tools).

### 5. `wp_economy` (G) — table only
- `RaceSetup` gains `weather_payout_mult: f32` and `weather_rep_mult: f32` (default 1.0), filled
  from `weather.economy_modifiers()` **when the race is announced** (the purse shown to the
  player must not change mid-race). Apply in `Economy::base_purse` (× `payout_mult`) and in the
  rep line of `race_payout` (× `rep_mult`). Do not apply to `entry_fee`.
- Current values: light rain +10–15 %, heavy rain +20–35 %, fog +10–20 %, wet road after rain
  up to +5 % (`data/weather/effects.toml`; G may retune the numbers there).

### 6. Renderer (C, `wp_wasm` / testbed) — via `wp_sim` accessors once the lead adds them
- Per frame: `weather.render_params()` → fog (`FogExp2.density ≈ fog_per_m`), rain particles
  (`rain_mm_per_h`, `wind_mps` slant), sky/sun (`cloud_cover`, `sun_dimming`), global road
  gloss (`mean_road_wetness`).
- At ≤ 1 Hz: `state().wetness_by_cell.wetness_u8(&data.surfaces)` as an `nx × ny` R8 texture
  over `spec().origin / cell_m` for per-cell road gloss; `puddle_cells(1.0)` for puddle decals
  (`center`, `size_m`, `depth_mm`).
- Spray: `spray_intensity(conditions_at(wheel).water_mm, speed)`.

### 7. Net (F)
- Server: send `weather.snapshot()` in the baseline and at 1 Hz (≈ 40 kB raw for a 1.6 km city —
  needs RLE/delta; most cells are equal). Client: `WeatherSim` with `locked = true`, call
  `apply_snapshot` on receipt and keep calling `step` for blending (see W-5).

### 8. Audio (13), races (15), UI (16)
- `drain_changes()` (→ `SimEvent::WeatherChanged`, W-2) for rain loops, thunder, HUD forecast.
- Races can lock weather: `WeatherConfig { locked: true, start: Regime::new(kind, i) }`.

## Needs
1. Lead: step 1 (`wp_sim`) and step 1b (simcli `weather:` field + rename the pending wet
   scenario). Both are exact above; nothing else is required for weather to change grip and
   police sight in the game.
2. v0.3.0: W-1 (`WorldQuery::weather_at`) + A's step 2 for the full grip model.
3. E: the speed-cap / incident setter (step 4). D, G: steps 3 and 5.

## Contract change requests
Status after v0.2.0 (applied 2026-10-19): none of W-1…W-6 were in the batch (filed too late);
**re-filed for v0.3.0**, unchanged. W-1 is the one that matters: without it the full grip
model cannot reach `wp_vehicle` and the interim `WetAsphalt` remap stays in use.

| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| W-1 | Move `LocalConditions` (as in `wp_weather::conditions`, plain data — **now incl. `visibility_m: f32`**, round 6b; `DRY.visibility_m = 800.0`) and `WeatherKind` into contracts; add `WorldQuery::weather_at(&self, p: [f32; 3]) -> LocalConditions` **with a default impl returning `LocalConditions::DRY`** | Vehicles need local water/temperature per wheel, police/traffic need local sight; a default method keeps every existing implementor compiling (additive → patch) | lead, A (calls it), D (forwards), E (LOS) | proposed for v0.3.0 |
| W-2 | `SimEvent::WeatherChanged { kind: WeatherKind, intensity: f32, forced: bool }` | Audio, HUD, replays and economy (race stakes) need to see regime changes | lead, F (codec), C, 13, 16 | proposed for v0.3.0 |
| W-3 | `SurfaceType::PaintedLine` (lane markings, zebra crossings; tires default 1.0) | Painted lines are the slipperiest common wet surface; `marking_grip_multiplier` is ready but nothing can report a marking | D (report it), A (`ffb` roughness match), H (tire data), lead | proposed for v0.3.0 (adding a variant breaks exhaustive matches → minor) |
| W-4 | `TireDef::treaded: Option<bool>` (default `true`; `semi_slick.json` → `false`) | Slicks aquaplane much earlier; today inferred from `WetAsphalt/Asphalt < 0.6` | H, A | proposed for v0.3.0 |
| W-5 | `ServerToClient` weather snapshot message / baseline field: `{ kind: u8, intensity: f32, prev_kind: u8, prev_intensity: f32, blend: f32, wind_heading_rad: f32, water_q8: Vec<u8> }` | Clients need the same wet roads for prediction and rendering | F, lead | proposed for v0.3.0 (fold into F's NET-1 list) |
| W-6 | `Season` into contracts | Only if 08/15 want season-dependent stakes or events; otherwise keep in `wp_weather` | lead, G, 15 | low priority |

**Not a contract — scenario field W-7:** exact spec in step 1b above. Scenario files (owned
by me, `apps/simcli/scenarios/weather/`):
- `dry_brake_100_0.ron` — active control: `debug_sedan`, cruise to 27.8 m/s, full brake
  14.0–18.0 s (released at 18.0 because a brake held at standstill selects reverse); expects
  ≤ 1.0 m/s at 17.2 s (passes: stops at ~17.1 s).
- `wet_brake_100_0.ron.pending` — the same car, inputs and timing with
  `weather: (kind: HeavyRain, intensity: 1.0, water_mm: 1.5, locked: true)`; expects 3–12 m/s
  at 17.2 s and −1…1.5 m/s at 18.0 s. Non-`.ron` extension so today's parser (unknown fields are
  errors) skips it; rename when W-7 lands.
Later: `weather_puddle_aquaplane` (needs W-1 + A's hook) and `weather_fog_pursuit` (pursuit
starts later / is lost sooner in fog).

## Changes outside my crates
- Round 6b: none. (`wp_weather` gained **dev**-dependencies on `wp_sim` and `wp_world` for the
  example/test; normal builds are unaffected, and a later `wp_sim → wp_weather` dependency is a
  dev-only cycle, which cargo allows. `Cargo.lock` updated accordingly.) The W-7 patch above is
  filed, not applied.
- `apps/simcli/scenarios/weather/` (new folder, scenario files only — allowed by the lead):
  `dry_brake_100_0.ron`, `wet_brake_100_0.ron.pending`. No simcli code touched.
- `apps/simcli/golden/weather_dry_brake_100_0/hashes.txt` — new golden for the new dry
  scenario (`goldens_match` requires one per scenario), blessed alone with
  `simcli run scenarios/weather/dry_brake_100_0.ron --bless`; commit tagged `[bless]`. No
  existing golden changed.
- `Cargo.toml` (workspace): added `crates/wp_weather` to `members` and `wp_weather` to
  `[workspace.dependencies]`.
- Lead (run 3, contract v0.2.0) added `line_of_sight` to `WithWeather` (forwards) and to the
  test `FlatWorld`; I kept both and added tests for them.
- `tools/lint-determinism/src/lib.rs`: added `"wp_weather"` to `SIM_CRATES` (it runs inside the
  sim step). Lint result: 0 errors; one `f32-consts` warning (`std::f32::consts::PI` used with
  `wp_math::sin_cos`).

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| Internal update (schedule + 40 000-cell grid + 400 vehicles) | K, cloud container | 0.20 ms, every 15th frame | not measured | — |
| … amortised per 60 Hz frame | K | 0.013 ms | — | — |
| `conditions_at` + `wheel_grip_multiplier` | K | 32 ns / query; 0.025 ms / frame for 24 cars | — | inside vehicle 0.15 ms/car |

`cargo run --release -p wp_weather --example weather_bench`.

## Known gaps
- Nothing is wired in the game yet: `wp_sim` (step 1, ready in `examples/weather_in_sim.rs`),
  simcli W-7 (patch in 1b), police caps (4), world (3), economy (5).
- The wet simcli scenario is pending on step 1/1b; the effect is proven today on the real
  vehicle via `Rig<Weathered<FlatWorld>>` (`tests/vehicle_grip.rs`).
- Interim grip path: only wetness ≥ 0.5 reaches tires (as `WetAsphalt`); damp roads, ice,
  painted lines and aquaplaning wait for W-1 + A's hook.
- Sky state is uniform over the map (no fog banks, no moving rain cells); only road water is
  local. No snow. Tire temperature out of scope.
- Grid cells take the surface at their centre (8 m cells); kerbs/markings are sub-cell.
- Season is fixed per session (`WeatherConfig::season`).
- Tuning numbers (drying rates, puddle fraction, payout +%) are first guesses; feel-test once
  wired.

## Lead feedback
(Written by the integration lead.)


## Lead feedback (round 6b, 2026-10-19) — head-start tasks

Contract **v0.3.0** is being applied in parallel by the lead (branch `lead/contract-v0.3.0`). Do not edit `wp_contracts`; build your work so it drops onto v0.3.0 (inherent methods / local types now, trait wiring after). Merge `origin/claude/laughing-newton-0xqr6q` (the lead branch, ahead of `main` until today's gate finishes) before starting. Goal of the project right now: the **single-player greybox demo**: drive in the city, raise heat, get chased by physical police, escape or get caught, and pay the repair bill.

1. A ready-to-merge integration example: `crates/wp_weather/examples/weather_in_sim.rs` composes `wp_sim` and `wp_weather` from the outside. Step the weather at the sim tick, sample conditions at each wheel, and show the grip effect on a braking test. This is the code the lead will move into `wp_sim` after v0.3.0 (W-1).
2. Keep `wet_brake_100_0.ron.pending` ready. W-7 (the scenario weather field) is I's parser: file the exact patch in your doc for I.
3. Fog/sight value for police line of sight: expose `visibility_m(p)` so E can use it after v0.3.0.

### Response (machine K, 2026-10-19)
1. **Done.** `crates/wp_weather/examples/weather_in_sim.rs`: `WeatherLink` (the code to move
   into `wp_sim`) stepped at every tick of a real `wp_sim::Sim`, conditions sampled under each
   wheel, braking test dry 42.9 m (Sim = rig, identical) vs soaked 54.1 m. Checked on every
   `cargo test` by `tests/weather_in_sim.rs`. See step 1.
2. **Done.** `wet_brake_100_0.ron.pending` unchanged and still in range; exact W-7 patch for I
   in step 1b (Part A merges now, Part B is one line after `Sim::set_weather`), verified end to
   end on a scratch worktree.
3. **Done.** `WeatherSim::visibility_m(p)`, `LocalConditions::visibility_m` (rides on W-1's
   `weather_at`), adapter accessors; `line_of_sight` cuts at it. E's use: step 4.

Checks on `ws/weather` (round 6b): fmt, clippy `--workspace --all-targets -D warnings`, wasm build
of `wp_wasm` pass; `cargo test --workspace --no-fail-fast`: 572 passed, 2 failed — both also fail
on the lead branch `claude/laughing-newton-0xqr6q` without any weather change (not mine, for the
lead): `wp_economy` `pursuit_wiring::real_police_capture_settles_through_the_economy` ("level at
capture, not after the reset", line 130) and `wp_race` `simcli_races::city_sprint_on_real_roads`
(index out of bounds, line 28). `weather_dry_brake_100_0` PASS, hashes match its golden.

## Lead integration: weather in the game (branch `lead/weather-in-game`, 2026-10-19)
Steps 1, 1b and 6 of the hook list are done by the lead; K's `weather_in_sim.rs` `WeatherLink`
moved into `crates/wp_sim/src/weather_link.rs` almost unchanged.

- **`wp_sim`**: `Sim::set_weather(WeatherConfig)` / `clear_weather` / `weather()` /
  `weather_mut()` / `force_weather(regime, immediate, locked)`. Stepped every tick after the
  world (sim vehicles + traffic agents dry their lanes); `set_world` rebuilds the 8 m grid over
  the new road network (+50 m). Vehicles and police get `WithWeather` with
  **`interim_surface = false`**: A's hook reads `weather_at` per wheel (water film + standing
  water → `water_depth_mm`, road temperature), painted lines come from D's `surface_at`
  (`PaintedLine`), so the full wet / cold / ice / paint / aquaplaning model is live. Police
  `line_of_sight` is cut at `visibility_m`. `drain_changes()` → `SimEvent::WeatherChanged`.
  The weather enters `Sim::state_hash` only when enabled, so weather-less hashes are unchanged.
- **simcli (W-7)**: Part A + B applied (`weather: (kind, intensity, water_mm, locked, season,
  seed)`), plus `puddles: [(min, max, water_mm)]` (standing-water boxes) and a
  `travel_between(from_s, to_s, min, max)` expectation (braking distance).
- **Scenarios** (`apps/simcli/scenarios/weather/`): `wet_brake_100_0` un-pended (51.6 m / 3.80 s
  vs 40.0 m / 2.93 s dry, ranges 46–62 m vs 35–45 m); `aquaplane_puddle` / `aquaplane_control`
  (33 m/s into 6 mm of water while steering: yaw rate 0.21 vs 0.37 rad/s, lateral offset 2.4 vs
  11.6 m); `fog_pursuit` / `fog_pursuit_clear` (noon city chase layout: first pursuit at 1.9 s in
  thick fog vs 0.3 s clear). `apps/simcli/tests/scenarios.rs` compares the twins.
- **Browser** (`wp_wasm::weather`, `apps/testbed-web/src/render/weather.ts`): selector, rain,
  wet roads + puddle sheen from the water grid, spray (`spray_intensity`), fog from visibility,
  wipers, HUD; "rainy night" city chase. Weather is part of the scenario, so replays carry it.

### Changes in `wp_weather` (K's crate; minimal glue, please keep or replace)
- `WetnessGrid::flood_rect(min, max, water_mm, &SurfaceTable)` and `WeatherSim::flood_rect`:
  flood the cells whose centres are in an XY box (raises their capacity to hold it; scenario
  puddles for the aquaplaning test).
- `WeatherSim::revision()`: bumped by `update`, `force`, `set_all_water_mm`, `flood_rect` and
  `apply_snapshot`. The browser hashes the sim every tick and streaming the whole grid into
  each hash cost 7 ms per frame in wasm; `wp_sim` now hashes `state_hash()` once per revision
  (0.24 ms per frame in rain vs 0.14 ms dry). If you add a state change, bump `revision`.

### Still open (owners)
- D: traffic slows in rain/fog — `wp_world` has no weather or global speed-factor hook yet;
  request filed in `docs/integration/world.md` (lead request W-D1). Traffic agents' grip on
  physical LOD is already weathered (they are `wp_sim` vehicles).
- E: speed caps / incident rate setter (step 4) and the explicit `visibility_m` cap on each
  phase range; LOS is already cut by the adapter.
- G: race purse multipliers (step 5). F: weather snapshot on the wire (W-5 message exists).
- Observation for K/E: the golden city chase layout with `HeavyRain` + 1.5 mm does not start a
  pursuit within 20 s (patrol AI on the wet road misses the night 40 m / 4 s window). Not a
  weather bug; worth a look when tuning police on wet roads.
