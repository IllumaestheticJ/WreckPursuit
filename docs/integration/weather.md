# Weather integration notes

- **Workstream / prompt:** 14 — weather & road conditions (machine K)
- **Branch:** `ws/weather`
- **Crates / folders owned:** `crates/wp_weather`, `data/weather/`
- **Contract version built against:** 0.2.0 (main merged 2026-10-19, after lead run 3)

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
- `WithWeather::new(&dyn WorldQuery, &WeatherSim)` — a `WorldQuery` (see step 1); forwards
  `line_of_sight` unchanged (LOS stays geometric).
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

### 1. `wp_sim` (lead) — works today, no contract change
```rust
// SimConfig gains `weather: Option<wp_weather::WeatherConfig>` (None = no weather, today's behaviour).
// After the world is built / set_world():
let grid = GridSpec::covering([min_x - 50.0, min_y - 50.0], [max_x + 50.0, max_y + 50.0], 8.0); // road network bbox
self.weather = cfg.weather.map(|c| WeatherSim::from_world(WeatherData::embedded(), c, grid, &self.world));

// step_fixed(), right after `self.world.step(dt, &mut out)`:
if let Some(w) = &mut self.weather {
    self.scratch_positions.clear();
    self.scratch_positions.extend(self.vehicles.iter().map(|v| v.vehicle.state().pos));
    self.scratch_positions.extend(self.world.agents().map(|a| a.pos));
    w.step(dt, self.world.time_of_day(), self.world.clock.ratio, &self.scratch_positions);
}
// vehicle loop: pass the adapter instead of &self.world
let q: &dyn WorldQuery = match &self.weather {
    Some(w) => &WithWeather::new(&self.world, w),   // interim_surface = true until W-1 + A's hook
    None => &self.world,
};
slot.vehicle.step_with_ground(&slot.input, &slot.assists, q, &ground, dt, &mut out);
// Sim::state_hash(): `if let Some(w) = &self.weather { w.hash_into(&mut h) }`
// (adds to the hash only when weather is configured, so existing goldens are untouched).
```
The traffic agents' own `wp_vehicle` steps inside `wp_world` keep the dry world until D
forwards weather (step 3). Cost: ≈ 0.04 ms per 60 Hz frame for a 1.6 km city and 24 cars
(README → Performance).

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

### 4. `wp_police` (E) — E has no branch yet; spec for when it lands
- `Police::set_weather(&mut self, m: &wp_weather::PoliceWeatherModifiers)` once per tick:
  - patrol speed cap: `cap_mps = base_cap_mps * m.patrol_speed_cap_mult`
  - pursuit speed cap: `cap_mps = base_cap_mps * m.pursuit_speed_cap_mult`
  - incident/dispatch call rate: `rate *= m.crash_incident_rate_mult`
  - LOS and roadblock sight checks (v0.2.0 has `WorldQuery::line_of_sight`): call
    `weather.can_see(world, observer, target, base_sight_m * night_mult)` instead of
    `dist <= base_sight_m * night_mult && world.line_of_sight(observer, target)`. Without access
    to the `WeatherSim`, the same result is `dist <= base * night_mult * m.sight_multiplier &&
    world.line_of_sight(..)` (weather does **not** include night; police keeps its own factor).
  - `wp_sim` already hands police `&self.world` (`police.step(&self.world, …)`); passing
    `&WithWeather::new(&self.world, w)` there instead is enough for wet-grip police cars too.
- Police cars are vehicles; they get wet grip through the sim's adapter like everyone else.

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
1. Lead: step 1 wiring in `wp_sim` (+ a `SimConfig::weather` field). Workaround: none needed
   for this crate's tests.
2. I (10): scenario field for weather (W-7 below) so wet/dry pairs can be compared in simcli.
3. A, D, E, G: the hooks above, after their branches are in.

## Contract change requests
Status after v0.2.0 (applied 2026-10-19): none of W-1…W-6 were in the batch (filed too late);
**re-filed for v0.3.0**, unchanged. W-1 is the one that matters: without it the full grip
model cannot reach `wp_vehicle` and the interim `WetAsphalt` remap stays in use.

| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| W-1 | Move `LocalConditions` (as in `wp_weather::conditions`, plain data) and `WeatherKind` into contracts; add `WorldQuery::weather_at(&self, p: [f32; 3]) -> LocalConditions` **with a default impl returning `LocalConditions::DRY`** | Vehicles need local water/temperature per wheel, police/traffic need local sight; a default method keeps every existing implementor compiling (additive → patch) | lead, A (calls it), D (forwards), E (LOS) | proposed for v0.3.0 |
| W-2 | `SimEvent::WeatherChanged { kind: WeatherKind, intensity: f32, forced: bool }` | Audio, HUD, replays and economy (race stakes) need to see regime changes | lead, F (codec), C, 13, 16 | proposed for v0.3.0 |
| W-3 | `SurfaceType::PaintedLine` (lane markings, zebra crossings; tires default 1.0) | Painted lines are the slipperiest common wet surface; `marking_grip_multiplier` is ready but nothing can report a marking | D (report it), A (`ffb` roughness match), H (tire data), lead | proposed for v0.3.0 (adding a variant breaks exhaustive matches → minor) |
| W-4 | `TireDef::treaded: Option<bool>` (default `true`; `semi_slick.json` → `false`) | Slicks aquaplane much earlier; today inferred from `WetAsphalt/Asphalt < 0.6` | H, A | proposed for v0.3.0 |
| W-5 | `ServerToClient` weather snapshot message / baseline field: `{ kind: u8, intensity: f32, prev_kind: u8, prev_intensity: f32, blend: f32, wind_heading_rad: f32, water_q8: Vec<u8> }` | Clients need the same wet roads for prediction and rendering | F, lead | proposed for v0.3.0 (fold into F's NET-1 list) |
| W-6 | `Season` into contracts | Only if 08/15 want season-dependent stakes or events; otherwise keep in `wp_weather` | lead, G, 15 | low priority |

**Not a contract — scenario format request to I (10), W-7:** optional top-level field
```ron
weather: (kind: HeavyRain, intensity: 0.8, water_mm: 1.5, locked: true, season: Autumn, seed: 3),
```
(`seed` defaults to the scenario seed; `water_mm` = `set_all_water_mm` before tick 0). Plus,
once W-1 + A's hook land, a metric `min_grip_mult`. First scenarios I would add under
`apps/simcli/scenarios/weather/`:
```ron
(
    name: "weather_wet_brake_100_0",
    seed: 1,
    assists: ai,
    weather: (kind: HeavyRain, intensity: 1.0, water_mm: 1.5, locked: true),
    entities: [(id: "car", car: "debug_sedan", driver: cruise(target_mps: 27.8, until_s: 14.0),
                inputs: [(t_s: 14.0, input: (brake: 1.0)), (t_s: 19.0, input: ())])],
    duration_s: 20.0,
    // dry twin (vehicle_brake_100_0) stops within 3 s; wet asphalt (0.70) needs ~1.4× the distance
    expect: [speed_at(t_s: 17.0, min: 3.0, max: 15.0), speed_at(t_s: 19.0, min: 0.0, max: 2.0)],
)
```
and `weather_puddle_aquaplane` (one car at 30 m/s through a soaked straight: lateral grip loss),
`weather_fog_pursuit` (police sight, once E lands). I did not add scenario files: the v2 parser
rejects unknown fields, and a dry copy of `vehicle/sedan_brake.ron` would add nothing.

## Changes outside my crates
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
- Nothing is wired: no `wp_sim`, vehicle, world, police or economy call sites yet (hooks above).
- No vehicle-level wet test: `wp_vehicle::rig::Rig` uses a fixed `FlatWorld`, and simcli
  cannot set weather (W-7).
- Sky state is uniform over the map (no fog banks, no moving rain cells); only road water is
  local. No snow. Tire temperature out of scope.
- Grid cells take the surface at their centre (8 m cells); kerbs/markings are sub-cell.
- Season is fixed per session (`WeatherConfig::season`).
- Tuning numbers (drying rates, puddle fraction, payout +%) are first guesses; feel-test once
  wired.

## Lead feedback
(Written by the integration lead.)
