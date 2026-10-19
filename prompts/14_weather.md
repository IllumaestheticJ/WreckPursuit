# PROMPT 14 — Weather & road conditions (`wp_weather`)

Read `CLAUDE.md`, `docs/INTEGRATION.md`, `docs/CONTRACT_CHANGELOG.md`. You own a new crate `wp_weather` and `data/weather/`. Branch `ws/weather`. You may read `wp_world`, `wp_vehicle`, `wp_police`; do not edit them — expose functions they will call and list the hook points in your integration doc.

## Goal
Deterministic weather that changes grip, visibility, police behaviour and race stakes: clear, overcast, rain (light/heavy), wet-drying roads, fog, and night temperature effects. Everything data-driven and seeded.

## Work
- `WeatherState { kind, intensity, wetness_by_cell (grid), fog_density, temperature_c, wind }` evolving on a seeded Markov schedule (`data/weather/climate.toml`, per season and per `DayPhase`), with rain accumulation and drying rates by surface and by traffic volume (busy lanes dry first).
- Grip: `surface_grip_multiplier(SurfaceType, wetness, temperature)` tables (wet asphalt ~0.7, standing water aquaplaning onset above a speed as a function of tire width and tread flag, painted lines/metal plates much lower when wet) + a hook spec for `wp_vehicle` tire `surface_multipliers` composition. Tire temperature is out of scope.
- Visibility: fog/rain → render params + an AI sight-distance multiplier (hook spec for `wp_world` traffic hazard reactions and `wp_police` LOS/roadblock sight checks).
- Police/econ hooks: rain reduces patrol speed caps and pursuit speed caps, raises crash incident rate; payouts +X% in rain (table only; `wp_economy` applies).
- Spray/puddle placement data for the renderer (puddle cells list).
- Tests: determinism; wetness never exceeds bounds; drying monotone when rain stops; aquaplaning onset increases with wetness and speed.

## Deliver
Crate, data, README, `docs/integration/weather.md` with exact hook signatures each owning crate should call and the `WorldQuery` extension request (`weather_at(p) -> LocalConditions`).
