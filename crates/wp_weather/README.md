# wp_weather

Deterministic weather and road conditions (workstream 14, machine K). Weather changes grip,
visibility, police behaviour and race stakes; everything is data-driven
(`data/weather/*.toml`) and seeded.

Depends only on `wp_contracts`, `wp_math`, `serde`, `toml`, `thiserror` (wasm32-clean). It does
not depend on `wp_world`, `wp_vehicle` or `wp_police`; those crates call into it (hook
signatures: `docs/integration/weather.md`).

## What it does

| Piece | Where | Notes |
|---|---|---|
| Regimes `Clear, Overcast, LightRain, HeavyRain, Fog` + intensity 0..1 | `kind.rs` | `Season` picks the Markov rows and temperature curve |
| Markov schedule per season × `DayPhase` | `lib.rs`, `climate.toml` | rolls every 15 in-game min, min hold per regime, linear blend between regimes (10 min) |
| Temperature (air + road), wind, cloud, fog | `lib.rs` | diurnal curve via `wp_math::sin_cos`; clear nights cool the road (black ice in winter) |
| `WetnessGrid` — water per 8 m cell | `grid.rs`, `surfaces.toml` | rain in, evaporation by surface / temperature / wind / sun / humidity / **traffic** (busy lanes dry first), infiltration of standing water, runoff to a per-cell cap; seeded puddle-prone cells |
| Grip | `grip.rs`, `surfaces.toml` | `surface_grip_multiplier(surface, wetness, road_temp_c)` (wet asphalt 0.70, metal 0.40, painted lines 0.50 for the reference tire), cold + ice, tire-compound scaling, aquaplaning onset vs depth / speed / width / tread |
| Visibility | `effects.rs`, `effects.toml`, `lib.rs` | Koschmieder visibility from fog, rain sight loss → AI sight multiplier; `can_see(world, a, b, base_m)` = weather range + v0.2.0 `line_of_sight`; `RenderParams` |
| Police / traffic / economy multipliers | `effects.rs`, `effects.toml` | patrol + pursuit speed caps, incident rate, traffic speed/headway/density, payout/rep (tables only — owners apply) |
| Puddles + spray for the renderer | `grid.rs`, `effects.rs` | `puddle_cells(min_mm)`, `wetness_u8()` texture, `spray_intensity(water, speed)` |
| `LocalConditions` / `weather_at(p)` | `conditions.rs` | payload of the requested `WorldQuery::weather_at` |
| `WithWeather` / `Weathered<W>` adapters | `adapter.rs` | wrap any `WorldQuery`: wet `Asphalt` → `WetAsphalt` so rain costs grip **now**; `line_of_sight` cut at the observer's `visibility_m` |
| Police sight cap | `lib.rs`, `conditions.rs` | `visibility_m(p)` (also `LocalConditions::visibility_m`): 800 m clear, 750 light rain, 480 heavy rain, 130 thick fog |
| `wp_sim` integration (ready to move) | `examples/weather_in_sim.rs` | `WeatherLink`: build/rebuild grid, step per tick, weathered query, hash, per-wheel conditions |
| Snapshot | `lib.rs` | `snapshot()` / `apply_snapshot()`: regime + `u8` water per cell (0.05 mm steps) |

Time: weather runs in **in-game seconds** (`dt × time_ratio`), like the clock. At the default
ratio 10 a 30-minute shower is 3 real minutes and a wet road dries in ~6–20 real minutes.
`WeatherSim::step` is called every physics tick; the schedule and grid update every
`update_interval_s` (default 0.25 s sim, 4 Hz).

## Usage

```rust
use wp_weather::*;
let mut w = WeatherSim::from_world(
    WeatherData::embedded(),
    WeatherConfig { seed: 7, season: Season::Autumn, ..Default::default() },
    GridSpec::covering([-800.0, -800.0], [800.0, 800.0], 8.0),
    &world,
);
// each physics tick:
w.step(PHYSICS_DT, world.time_of_day(), world.clock.ratio, &vehicle_positions);
let local = w.conditions_at(wheel_contact_point);
let grip = SurfaceTable::embedded()
    .wheel_grip_multiplier(&local, surface, false, speed_mps, &TireProfile::default());
```

## Tests

```
cargo test -p wp_weather
cargo run --release -p wp_weather --example weather_bench
cargo run --release -p wp_weather --example weather_in_sim   # weather + wp_sim, braking dry vs wet
```

Unit tests (in each module) + `tests/weather.rs`: same seed ⇒ identical hashes over 72 in-game
hours, different seed ⇒ different; per-tick stepping deterministic; water/wetness/intensity/sight
never leave their bounds (12 seeds × 48 h, all seasons); drying is monotone whenever the rain
rate is zero (even with the schedule free to roll); normal cells dry within 3 h, puddles last
longer; aquaplaning amount is non-decreasing in water depth and speed, onset falls with depth,
slicks and wide tires plane earlier; heavy rain makes puddles; dawn favours fog; locked
schedules never change; regime blends are smooth; winter clear nights freeze wet roads; police /
economy / traffic multipliers move the right way; adapter remaps only wet asphalt and forwards `line_of_sight`; fog shortens `can_see` and walls still block; snapshot
round trip; disk tables == embedded tables; invalid tables are rejected.

## End-to-end result (real vehicle)

`tests/vehicle_grip.rs` drives `debug_sedan` through `wp_vehicle::rig::Rig<Weathered<FlatWorld>>`:
100→0 km/h with ABS **42.9 m / 3.09 s dry, 54.1 m / 3.93 s soaked** (1.26×); a dry road under
the adapter is bit-for-bit the plain dry world. simcli pair: `apps/simcli/scenarios/weather/`
(`dry_brake_100_0.ron` active; `wet_brake_100_0.ron.pending` until the `weather:` field lands).

`examples/weather_in_sim.rs` (checked by `tests/weather_in_sim.rs`) steps a `WeatherLink`
beside a real `wp_sim::Sim` and repeats the brake test on `Rig` over a real `wp_world::World`:
dry `Sim` 42.9 m = dry rig 42.9 m = weather-on-but-clear 42.9 m (bit-identical); soaked 54.1 m.

## Performance (native, release, cloud container, 2026-10-19)

| Measurement | Value | Budget |
|---|---|---|
| One internal update, 40 000 cells (1.6 × 1.6 km @ 8 m), 400 vehicles | 0.20 ms (every 15th frame) | — |
| … amortised per 60 Hz frame (4 Hz updates) | 0.013 ms | no budget set; police is 0.5 ms |
| `conditions_at` + `wheel_grip_multiplier` per wheel query | 32 ns | — |
| … 24 cars × 4 wheels × 4 ticks × 2 substeps per frame | 0.025 ms | vehicle 0.15 ms/car |

Browser: not measured yet (expected ≤ 2× native; the update is one linear pass over `f32`s).

## In the game (lead, 2026-10-19)
`wp_sim` owns a `WeatherSim` (`Sim::set_weather`), simcli has the `weather:` scenario field and
the browser renders rain, wet roads, spray and fog — see `docs/integration/weather.md` → "Lead
integration". Lead glue in this crate: `WetnessGrid::flood_rect` / `WeatherSim::flood_rect`
(scenario puddles) and `WeatherSim::revision` (hash caching; bump it on any new state change).

## Known gaps

- The wet simcli scenario waits for `wp_sim`/simcli wiring (integration doc steps 1/1b); only
  wetness ≥ 0.5 reaches tires until W-1 (no damp-road loss, ice, markings or aquaplaning in game).
- Painted markings have no `SurfaceType` (W-3); `marking_grip_multiplier` exists but nothing
  calls it with `on_marking = true` yet.
- Tread flag is inferred from the compound's wet ratio (W-4).
- Fog and rain are uniform over the map (no fog banks or moving rain cells); the grid makes the
  road state local, not the sky.
- No snow, no tire temperature (out of scope), no wetness from splashes between cells.
- Cells are sampled at their centre: an 8 m cell straddling road and verge takes one surface.
- Clients would need the snapshot replicated (W-6); `wp_net` does not carry it yet.
