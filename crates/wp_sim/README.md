# wp_sim

**Owned by workstream 12 (integration lead).**

The fixed-step game loop. `Sim::new(SimConfig)`, `spawn_vehicle(&LoadedCar, pos, yaw)`, `set_input`, `set_assists`, `reset_vehicle`, `place_vehicle(id, pos, yaw, speed)`, `add_static_box(pos, size, yaw)`, `set_mode_table`, `step_fixed()` (one `PHYSICS_DT` tick), `advance(frame_dt) -> alpha`, `vehicle_states()`, `drain_events()`, `state_hash()`.

Per tick: world clock → vehicles (`wp_vehicle` wheel probes against Rapier, forces written to
the chassis bodies) → Rapier step (gravity -Z, static ground slab with top at z = 0) → contact
extraction (`contacts.rs`) → damage contacts → `HandlingEffects` back to vehicles → police →
events.

Contacts: after the Rapier step every chassis collider's contact pairs become
`ContactImpulse`s in that vehicle's chassis space (normal into the vehicle, closing speed from
pre-step velocities, friction direction rebuilt from the sliding velocity). One
`SimEvent::Contact` per touching pair per tick (emitted by the lower entity id) when the pair's
summed normal impulse is at least `CONTACT_EVENT_MIN_IMPULSE_NS` (50 N·s); damage always gets
every contact. Disabling the vehicle system (`set_systems`) also disables the chassis bodies.

`state_hash` follows hash spec v1 (`HASH_SPEC_VERSION`, full spec in `docs/integration/harness.md`): FNV-1a over version, tick, clock, vehicle count and every `entity_hashes()` value in ascending entity order; each entity hash covers quantised pose, velocities, rpm, gear, health, wheels and lattice nodes.

## Test surface (workstream 10)
- `Systems` / `SystemId` registry and `Sim::set_systems` — run a subset of systems (`Systems::parse("vehicle,world")`); default `Systems::ALL`.
- `Sim::last_tick_event_count`, `Sim::node_positions(id)`, `Sim::entity_hashes`.
- Scenario-level tests live in `apps/simcli` (see `docs/TESTING.md`).

## City: police and traffic as vehicles (lead run 5)
- `set_world(World)` also rebuilds police on the city (`PoliceSystem::for_world`).
- `set_police_car(car)`: AI police car units become `VehicleKind::Police` vehicles driven by
  `wp_police` (`police_link.rs`). `set_traffic_car(car)` (after `set_world`): traffic promoted to
  physical LOD becomes `VehicleKind::Traffic` vehicles driven by `wp_world` (`traffic_link.rs`).
- Slot reserves: police stop spawning at 64 − 16 vehicles, traffic at 64 − 24.
- `police()` exposes the police system; `examples/police_probe.rs` prints a chase timeline.

## Weather (lead, step 2)
- `set_weather(WeatherConfig)` turns on `wp_weather` (seeded, sim-time only; `clear_weather`,
  `weather()`, `weather_mut()`, `force_weather`). Stepped every tick after the world; vehicles
  and police see the world through `wp_weather::WithWeather` (interim surface remap off:
  `wp_vehicle` reads `weather_at` per wheel). Police line of sight is cut at `visibility_m`.
  Regime changes emit `SimEvent::WeatherChanged`. `set_world` rebuilds the 8 m wetness grid
  over the new road network.
- `state_hash` includes `WeatherSim::state_hash()` (cached per `WeatherSim::revision`) only when
  weather is on: weather-less hashes and goldens are unchanged.
- Cost (native, release): ≈ 0.04–0.08 ms per 60 Hz frame in the 12×12 city (golden city chase
  layout, 1.01–1.07 s → 1.09–1.32 s for 1200 frames); browser 0.1 ms per frame (flat world).
- Tests: `cargo test -p wp_sim weather`; scenarios in `apps/simcli/scenarios/weather/`.
## City ground: sidewalks and kerbs (playtest #20)
- `PhysicsWorld::set_city_ground` runs for every city (`Sim::set_world`): sidewalk tops and
  kerb faces (`wp_world::surface::sidewalk_mesh`, the triangles the greybox draws) as
  triangle-mesh tiles of 256 m in `SIDEWALK_GROUP`; a flat city keeps its slab under them, a
  city with terrain also gets terrain, road, building and deck-rail colliders.
  `CityColliders::sidewalk_triangles` counts them.
- A car upright on its wheels (≥ 2 wheel contacts, up.z > 0.7) has a chassis collider that
  ignores `SIDEWALK_GROUP` (switched per tick, only on change): the wheels climb the kerb and
  carry the car. The chassis box's floor sits at kerb height (0.15 m), so while straddling a
  kerb it otherwise scraped the kerb edge and snagged on internal triangle edges (a dead stop
  from 15 km/h). Rolled, airborne or wheel-less cars collide with sidewalks again.
  (Parry's `FIX_INTERNAL_EDGES` would also cure the snag, but its pseudo-normals use
  `f32::acos`, which can differ between native libm and wasm: not used.)
- `place_on_ground` lifts a car onto a sidewalk top just above the road/terrain height under
  it. `static_ground_z(xy, from_z, max_m)` raycasts the static world (diagnostics, tests).
- Tests: `tests/sidewalk.rs` drives 96 kerb mounts (15 / 40 km/h, two angles, every Port
  Vesper district, fill/deck approaches, the grid) and requires every tire on the visual
  surface within 3 cm (worst today: −2.3 cm Vesper, −0.6 cm grid; −34 cm / −15 cm without the
  colliders). simcli `city/sidewalk_mount_{15,40}kmh`.

## Parked-car damage (owner playtest 2026-10-21, `parked.rs`)
City parked cars (and `Sim::add_parked_car`) are props: a fixed box until something comes
near, then a dynamic one. One that takes a hit the lattice would feel (≥ 50 N·s from a moving
body at ≥ 1 m/s in one tick) is promoted: it gets the `Sim::set_parked_car` model's
`DamageModel` (simcli: the traffic car) and the session's mode table, its own contacts each
tick it is touched or its lattice is awake, a collider rebuilt from the dented hull (flat
4-corner floor), and debris for torn parts. Untouched parked cars cost nothing; at most
`MAX_PARKED_WRECKS` (48) per session. Its damage events stay out of the event stream (police,
economy and net key events by vehicle); read `parked_wrecks()` / `parked_stats()`. A dented
car's lattice answers `node_positions(PARKED_ENTITY_BASE + prop)`. A shoved prop wakes its
neighbours like a vehicle. Perf and scenarios: `docs/integration/parked-signals.md`
(`cargo run --release -p wp_sim --example parked_damage_cost`).

## Dormant vehicles (SR-3, lead, 2026-10-22)
`Sim::set_dormant(id, true)` puts a car out of play to sleep: its chassis becomes a fixed
Rapier body where it stands (at rest), it gets no tyre / suspension / drivetrain step, no
forces and no damage chassis motion, but keeps its collider, so it is hit like a parked car.
It wakes by itself, deterministically: when a moving vehicle (> `DORMANT_WAKE_MIN_SPEED_MPS`)
comes within `DORMANT_WAKE_M` (40 m, before any contact at road speeds), when anything touches
it, and when it is placed or reset. Replays log the sleep as `simcli` `OpKind::Dormant` (kind 6);
the wakes need no op. Cost (`cost_probe`, 13 `hakone_gt` on the slab): 0.10 ms per car per
60 Hz frame idle, 0.12 driving, 0.004 dormant. Street racers off shift sleep in their lockup
(`wp_wasm::street`). Test: `dormant_car_sleeps_until_a_car_comes_near`.

## Known gaps
- Traffic AI does not slow in rain or fog yet (needs a `wp_world` hook, request W-D1 in
  `docs/integration/world.md`); police speed caps ignore weather (E, weather.md step 4).
- Contacts reach `wp_damage`, but damage is still the prompt-01 stub until workstream 03 merges.
- Static geometry is boxes only (`add_static_box`, TB-3); simcli/testbed scenarios do not create them yet.
- Chassis collider is the box from `wp_vehicle::build_body`; a lattice hull comes with 03.
- Economy is not wired in.

## Tests

```sh
cargo test -p wp_sim
```
