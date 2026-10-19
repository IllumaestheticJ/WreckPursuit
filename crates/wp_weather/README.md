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
| Visibility | `effects.rs`, `effects.toml` | Koschmieder visibility from fog, rain sight loss → AI sight multiplier; `RenderParams` |
| Police / traffic / economy multipliers | `effects.rs`, `effects.toml` | patrol + pursuit speed caps, incident rate, traffic speed/headway/density, payout/rep (tables only — owners apply) |
| Puddles + spray for the renderer | `grid.rs`, `effects.rs` | `puddle_cells(min_mm)`, `wetness_u8()` texture, `spray_intensity(water, speed)` |
| `LocalConditions` / `weather_at(p)` | `conditions.rs` | payload of the requested `WorldQuery::weather_at` |
| `WithWeather` adapter | `adapter.rs` | wraps any `WorldQuery`; reports wet `Asphalt` as `WetAsphalt` so rain costs grip **now**, before any contract change |
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
```

Unit tests (in each module) + `tests/weather.rs`: same seed ⇒ identical hashes over 72 in-game
hours, different seed ⇒ different; per-tick stepping deterministic; water/wetness/intensity/sight
never leave their bounds (12 seeds × 48 h, all seasons); drying is monotone whenever the rain
rate is zero (even with the schedule free to roll); normal cells dry within 3 h, puddles last
longer; aquaplaning amount is non-decreasing in water depth and speed, onset falls with depth,
slicks and wide tires plane earlier; heavy rain makes puddles; dawn favours fog; locked
schedules never change; regime blends are smooth; winter clear nights freeze wet roads; police /
economy / traffic multipliers move the right way; adapter remaps only wet asphalt; snapshot
round trip; disk tables == embedded tables; invalid tables are rejected.

## Performance (native, release, cloud container, 2026-10-19)

| Measurement | Value | Budget |
|---|---|---|
| One internal update, 40 000 cells (1.6 × 1.6 km @ 8 m), 400 vehicles | 0.20 ms (every 15th frame) | — |
| … amortised per 60 Hz frame (4 Hz updates) | 0.013 ms | no budget set; police is 0.5 ms |
| `conditions_at` + `wheel_grip_multiplier` per wheel query | 32 ns | — |
| … 24 cars × 4 wheels × 4 ticks × 2 substeps per frame | 0.025 ms | vehicle 0.15 ms/car |

Browser: not measured yet (expected ≤ 2× native; the update is one linear pass over `f32`s).

## Known gaps

- No vehicle-level wet test yet: `wp_vehicle::rig::Rig` hard-codes its flat world, and simcli
  scenarios cannot set weather (request W-7). Grip is tested as multipliers only.
- Painted markings have no `SurfaceType` (W-3); `marking_grip_multiplier` exists but nothing
  calls it with `on_marking = true` yet.
- Tread flag is inferred from the compound's wet ratio (W-4).
- Fog and rain are uniform over the map (no fog banks or moving rain cells); the grid makes the
  road state local, not the sky.
- No snow, no tire temperature (out of scope), no wetness from splashes between cells.
- Cells are sampled at their centre: an 8 m cell straddling road and verge takes one surface.
- Clients would need the snapshot replicated (W-6); `wp_net` does not carry it yet.
