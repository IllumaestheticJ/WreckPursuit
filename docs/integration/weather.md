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
- `WithWeather::new(&dyn WorldQuery, &WeatherSim)` — a `WorldQuery` (see step 1): wet asphalt
  → `WetAsphalt`; `line_of_sight(a, b)` = `|b − a| ≤ sight_range_m() && world.line_of_sight(a, b)`
  (`weather_los = false` for the geometric test alone). `Weathered<W>` is the owning variant
  (e.g. `wp_vehicle::rig::Rig<Weathered<FlatWorld>>`).
- `WeatherSim::sight_range_m() -> f32` — 800 m × sight multiplier (800 clear, ~130 thick fog).
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

### 1b. simcli (lead, on behalf of I) — scenario field W-7
`apps/simcli/src/scenario.rs`, `Scenario` gains
```rust
/// Weather (workstream 14). None = no weather system (dry, clear: today's behaviour).
#[serde(default, skip_serializing_if = "Option::is_none")]
pub weather: Option<WeatherSpec>,

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WeatherSpec {
    pub kind: wp_weather::WeatherKind,           // Clear | Overcast | LightRain | HeavyRain | Fog
    #[serde(default = "one")] pub intensity: f32, // 0..=1, default 1
    #[serde(default)] pub water_mm: f32,          // start water on every cell (0 = dry road)
    #[serde(default = "yes")] pub locked: bool,   // default true: scenarios want fixed weather
    #[serde(default)] pub season: wp_weather::Season, // default Autumn
    #[serde(default)] pub seed: Option<u64>,      // default: the scenario seed
}
```
(`WeatherKind` / `Season` derive `Serialize`/`Deserialize` as unit variants.) In
`apps/simcli/src/run.rs::build_sim`, after the `has_city` block (so the grid covers the city):
```rust
if let Some(w) = &sc.weather {
    sim.set_weather(wp_weather::WeatherConfig {
        seed: w.seed.unwrap_or(sc.seed),
        season: w.season,
        start: wp_weather::Regime::new(w.kind, w.intensity),
        start_water_mm: w.water_mm,
        locked: w.locked,
        ..Default::default()
    });
}
```
`apps/simcli/Cargo.toml`: `wp_weather.workspace = true`. Two `Scenario { .. }` struct literals
need `weather: None`: `apps/simcli/src/fuzz.rs` (`gen_case`, after `barriers,`) and
`crates/wp_wasm/src/scenario.rs` (`free_drive`, after `barriers: Vec::new(),`). Replays need
nothing extra (they embed the scenario RON). SCENARIOS.md top-level block:
`weather: (kind: HeavyRain, intensity: 1.0, water_mm: 1.5, locked: true), // optional; see wp_weather`.
Then **rename `apps/simcli/scenarios/weather/wet_brake_100_0.ron.pending` →
`wet_brake_100_0.ron`** and bless only it:
`cd apps/simcli && cargo run --release -p simcli -- run scenarios/weather/wet_brake_100_0.ron --bless`
(commit with `[bless]`). In my patched copy every existing golden still matched; the only
`goldens_match` failures were the two new weather scenarios' missing goldens.

**Verified (2026-10-19, machine K):** I applied exactly steps 1 + 1b to a throwaway copy of
the repo (never committed): it compiles, and simcli gives `weather_dry_brake_100_0` PASS
(−0.05 m/s at 17.2 s) and `weather_wet_brake_100_0` PASS (**4.60 m/s at 17.2 s**, −0.05 m/s
at 18.0 s). `wp_weather`'s `pending_wet_scenario_ranges_hold_on_the_real_vehicle` checks the
same ranges on the rig on every `cargo test` (dry −0.05, wet 4.90 m/s).

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
| W-1 | Move `LocalConditions` (as in `wp_weather::conditions`, plain data) and `WeatherKind` into contracts; add `WorldQuery::weather_at(&self, p: [f32; 3]) -> LocalConditions` **with a default impl returning `LocalConditions::DRY`** | Vehicles need local water/temperature per wheel, police/traffic need local sight; a default method keeps every existing implementor compiling (additive → patch) | lead, A (calls it), D (forwards), E (LOS) | proposed for v0.3.0 |
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
- Nothing is wired in the game yet: `wp_sim` (steps 1/1b), police caps (4), world (3), economy (5).
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
