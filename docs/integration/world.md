# World (city, signals, traffic) integration notes

- **Workstream / prompt:** 05 — city, road network, signals & civilian traffic
- **Branch:** `ws/world`
- **Crates / folders owned:** `crates/wp_world`, `data/city/`, `tools/city-import/`
- **Contract version built against:** 0.1.0

## Exposes
All in `wp_world` (details and format spec: `crates/wp_world/README.md`).

**World lifecycle**
- `World::load(path, WorldConfig) -> Result<World, RoadError>` (`.road.bin` or `.road.json`),
  `World::from_bin(&[u8], cfg)` (browser/embedded), `World::procedural(&ProcGenConfig, cfg)`,
  `World::flat(time, ratio)` (unchanged stub behaviour: empty map, asphalt, no traffic).
- `WorldConfig { seed, start_time, time_ratio (default 10), traffic: TrafficConfig,
  parked_cars }`.
- `World::step(dt, &mut Vec<SimEvent>)` — call once per physics tick with `PHYSICS_DT`. Runs
  clock and signals every call, physical traffic vehicles every call, traffic AI + violation
  detection at 60 Hz. Emits `SimEvent::TrafficViolation`.
- `World::populate()` — warm start (fill the active area to its density targets).
- `World::state_hash() -> u64` — clock, signal time, every agent.

**Queries**
- `impl WorldQuery for World` (and for `CityMap`): surface (lanes asphalt, highway concrete,
  curbs at ribbon edges, off-road concrete/grass/dirt by district, junction boxes asphalt),
  speed limit (lane or junction connector under the point), next signal ahead along the
  heading (≤ 150 m, following straight continuations), time of day.
- `World::map() -> &CityMap` (`locate`, `zone_at`, `district_at`, `intersection_at`,
  `lane_signal`, `lane_control`, segments with polylines), `World::network() -> &RoadNetwork`
  (nodes, lanes, junctions, signal plans, zones, POIs, buildings).
- `wp_world::clock::{WorldClock, sun_direction}`; `World::clock` (`phase()`,
  `night_factor()`, `sun_direction()`).

**Agents (renderer / netcode)**
- `World::agents() -> impl Iterator<Item = AgentView>`: `id, pos, yaw, speed_mps, lod,
  personality, brake_lights, hazard_lights, yielding, turn_signal, variant`, ordered by id.
- `World::physical_agents_mut() -> (AgentId, &mut PhysicalAgent)` (`vehicle: wp_vehicle::Vehicle`).
- `World::drain_lod_changes() -> Vec<LodChange>` (`Promoted(id) | Demoted(id) | Despawned(id)`).
- `World::set_agent_entity(id, Option<EntityId>)`, `World::apply_agent_damage(id, &HandlingEffects)`.
- `World::static_props()` — parked cars (`PropKind::ParkedCar`, 1400 kg boxes along parking
  lanes) and closure cones (`PropKind::Cone`, 3 kg).
- `World::traffic_stats()`, `World::zone_density()` (debug overlay: targets vs counts).

**Race routes** (`wp_world::routes`, also as `World` methods)
- `plan_route_between_pois(start, finish)`, `plan_route(start, heading, finish)`,
  `plan_circuit(polygon)` → `Route { segs, lanes, polyline, checkpoints, length_m, closed_loop }`.
- `closure_for_route(&map, &route) -> RouteClosure { closed_lanes, cones }`.
- `World::set_active_routes(Vec<ActiveRoute { route, closed }>)` — focus area for spawning and
  physical LOD; `closed` suppresses traffic on the route's roads and adds cones.

**Data & tools**
- `data/city/procedural_12x12.{road.bin,road.json,greybox.glb}` (`just city`).
- `tools/city-import`: `gen-city --grid WxH --seed N`, `city-import extract.osm.pbf --id X`.
- `wp_world::geom::{atan, atan2}` — deterministic polynomial versions (proposed for `wp_math`).

## Needs
From the sim loop (`wp_sim`, integration lead) — none of these are wired yet:
1. **Load a city**: `SimConfig` needs a city source (path or bytes) and `Sim::new` should use
   `World::load` instead of `World::flat`; also `world.set_traffic_car(&LoadedCar)` with a
   civilian car from the roster (09). Workaround: tests use `World::procedural`.
2. **Tracked vehicles every tick**: `world.set_tracked_vehicles(&[TrackedVehicle])` for every
   player, police unit and AI racer (`TrackedVehicle::from_state(entity, &state, siren,
   is_human_player)`; `siren` from `VehicleInput::siren`). Without it there are no violations,
   no siren/hazard reactions and the whole map counts as "active".
3. **Physical traffic bodies**: on `LodChange::Promoted(id)` create a Rapier body + `DamageModel`
   for that agent's `PhysicalAgent::vehicle` (and `set_agent_entity`); feed its contacts to
   damage and call `apply_agent_damage`; remove on `Demoted` / `Despawned`. Physical agents
   count against `MAX_VEHICLES` (max 16 by default, `TrafficConfig::max_physical`).
4. **Static colliders** for `static_props()` (re-read after `set_active_routes`).
5. **Debris hazards**: `world.set_hazards(&[Hazard { pos, radius_m }])` from detached parts (03).
6. **Race events** (08/12): `set_active_routes` when an event starts/ends.
7. **Testbed** (04): load `data/city/procedural_12x12.greybox.glb` and draw `AgentView`s; it
   needs a `wp_wasm` export for agents (suggest a flat `Float32Array` of `x, y, z, yaw, flags`
   per agent).
8. **simcli scenarios** (10): a traffic soak scenario once the sim loads cities (expected
   ranges: 0 civilian collisions, ≥ 380 agents, ≥ 50 % moving).

## Contract change requests
| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| 1 | `VehicleSim::teleport(&mut self, pos, rot, lin_vel, ang_vel)` (or keep `wp_vehicle::Vehicle::set_pose_and_speed`) | Seamless kinematic → physical traffic promotion needs to set velocity; the stub only had `reset` (zero speed) | 02 (implements), 05/06 (use it for traffic and police spawns) | proposed; interim inherent method added to `wp_vehicle` |
| 2 | `SimEvent::TrafficViolation { …, pos: [f32; 3] }` | Police dispatch needs where the violation happened (camera / witness position) | 06, 07 (replication) | proposed |
| 3 | `TrafficAgentSnapshot { id: u32, pos: [f32; 3], yaw: f32, speed: f32, flags: u8, variant: u8 }` in `wp_contracts::net` (+ `ServerToClient` snapshot field) | Replicate kinematic traffic cheaply; physical agents replicate as vehicles | 07, 04 | proposed |
| 4 | `wp_math::atan2` (move `wp_world::geom::{atan, atan2}`) | Deterministic `atan2` needed by 02/06 too (wp_math README lists it as a gap) | 12 | proposed |
| 5 | `SimConfig::city: Option<CitySource>` | Choose the city per instance | 12, 07 | proposed |

## Changes outside my crates
- `Cargo.toml` (workspace): added member `tools/city-import`.
- `Cargo.lock`: `postcard` (wp_world), `osmpbf` (city-import only).
- `justfile`: added `city` and `traffic-bench` recipes.
- `crates/wp_vehicle/src/lib.rs`: added `Vehicle::set_pose_and_speed(pos, yaw, speed_mps)`
  (Lead: the stub this went into was replaced by 02's real model; the method now lives in `wp_vehicle/src/vehicle.rs`, same signature.)
  (7 lines, `reset` + speed) for LOD promotion — see contract request 1.

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| 400 agents, traffic + signals + violations per 60 Hz frame (release) | cloud container, 4 vCPU | avg 0.06 ms, p99 0.15 ms, max ≈ 1.4 ms | not measured | 2 ms (browser 4 ms) |
| Same with collision diagnostics | same | avg 0.20 ms | — | — |
| `World::load` procedural_12x12 | same | ≈ 60 ms | — | — |
| 10-min soak × 18 (9 seeds × 0 / 5 % aggressive) | same | 0 civilian–civilian collisions | — | < 1 % with aggressive |

## Known gaps
- No rigid bodies for physical traffic until the sim loop handles `LodChange` (Needs 3).
- OSM importer verified on synthetic data only (download hosts blocked here); see
  `tools/city-import/README.md`.
- In saturated flow a few permissive left / minor-road cars starve > 2 min and get recycled
  when unseen (0–9 per 10 min with 400 agents); no protected-left phases yet.
- One civilian car model; no buses; sightline rule is distance-only.
- Kinematic traffic is not replicated yet (contract request 3).

## Lead feedback
(Written by the integration lead.)
