# World (city, signals, traffic) integration notes

- **Workstream / prompt:** 05 — city, road network, signals & civilian traffic
- **Branch:** `ws/world`
- **Crates / folders owned:** `crates/wp_world`, `data/city/`, `tools/city-import/`
- **Contract version built against:** 0.2.0

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
- `WorldQuery::line_of_sight(a, b)` (contract v0.2.0) on `World`/`CityMap`: buildings are
  extruded footprints; blocked if any part of the sight line inside a footprint is below the
  roof; grid-indexed (≈ 1.1 µs per 300 m query, release).
- `World::road_graph() -> RoadGraphView` (`wp_world::roads`): road-level graph — nodes (pos,
  district, signalised), one `RoadEdge` per road (`a`, `b`, centreline, length, limit, class,
  lanes each way / one-way, district), POIs, building bounds. Maps 1:1 onto
  `wp_police::RoadGraph::{add_node, add_segment}`; proposed shape for the v0.3.0 shared trait.
- `ProcGenConfig::small()` — 6×6 test city with every feature.
- `wp_world::geom::{atan, atan2}` re-export `wp_math::{atan, atan2}` (moved in v0.2.0).

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
| 1 | ~~`VehicleSim::teleport(&mut self, pos, rot, lin_vel, ang_vel)` (or keep `wp_vehicle::Vehicle::set_pose_and_speed`)~~ | Seamless kinematic → physical traffic promotion needs to set velocity; the stub only had `reset` (zero speed) | 02 (implements), 05/06 (use it for traffic and police spawns) | rejected in v0.2.0: inherent `Vehicle::set_pose_and_speed` kept |
| 2 | `SimEvent::TrafficViolation { …, pos: [f32; 3] }` | Police dispatch needs where the violation happened (camera / witness position) | 06, 07 (replication) | **applied v0.2.0** (D emits `pos` = tracked vehicle position) |
| 3 | `TrafficAgentSnapshot { id: u32, pos: [f32; 3], yaw: f32, speed: f32, flags: u8, variant: u8 }` in `wp_contracts::net` (+ `ServerToClient` snapshot field) | Replicate kinematic traffic cheaply; physical agents replicate as vehicles | 07, 04 | accepted, deferred to v0.3.0 — final shape below (#6) |
| 4 | `wp_math::atan2` (move `wp_world::geom::{atan, atan2}`) | Deterministic `atan2` needed by 02/06 too (wp_math README lists it as a gap) | 12 | **applied v0.2.0** |
| 5 | `SimConfig::city: Option<CitySource>` | Choose the city per instance | 12, 07 | done without contract change (`Sim::set_world`) |

| 6 | v0.3.0 `TrafficAgentSnapshot` final shape (below) | Traffic replication | 07 (F), 04 (C) | proposed for v0.3.0 |
| 7 | v0.3.0 shared road-network trait (below) | Police, traffic, minimap share one network (E #5) | 06 (E), 12, 16 | proposed for v0.3.0 |
| 8 | v0.3.0 `DistrictId` (below) | Economy/police per-district tables (G #6, E districts) | 06, 08 | proposed for v0.3.0 |

### v0.3.0 proposals (D)

**#6 `TrafficAgentSnapshot`** (in `wp_contracts::net`, one per kinematic agent in a client's
interest set; physical agents replicate as normal vehicles):
```rust
pub struct TrafficAgentSnapshot {
    pub id: u32,            // wp_world::AgentId, stable for the agent's life
    pub pos: [f32; 3],      // m (F may quantise on the wire, e.g. cm)
    pub yaw: f32,           // rad, 0 = +Y
    pub speed_mps: f32,     // for client-side extrapolation along the heading
    pub flags: u8,          // bit0 brake lights, bit1 hazards, bit2 left signal,
                            // bit3 right signal, bit4 headlights, bit5 yielding to siren
    pub variant: u8,        // visual variant (car model / colour), 0..8 today
}
```
Produced by `wp_world::AgentView` → snapshot (a `From` impl lives in `wp_world` once the type
exists). Despawns are implicit (absent id); `LodChange::Promoted/Demoted` tells the server when
an id moves between this list and the vehicle list.

**#7 Shared road-network trait** (read-only; `wp_world::CityMap` implements it, `wp_police`
consumes it instead of its own `RoadGraph`; ids are plain `u32`):
```rust
pub enum RoadClass { Highway, Arterial, Collector, Local, Alley }
pub struct RoadEdgeInfo { pub a: u32, pub b: u32, pub length_m: f32, pub speed_limit_mps: f32,
                          pub class: RoadClass, pub lanes_ab: u8, pub lanes_ba: u8,
                          pub district: Option<DistrictId> }
pub trait RoadNetworkQuery {
    fn node_count(&self) -> u32;
    fn node_pos(&self, n: u32) -> [f32; 2];
    fn edge_count(&self) -> u32;
    fn edge(&self, e: u32) -> RoadEdgeInfo;
    fn edge_polyline(&self, e: u32) -> &[[f32; 2]];
    /// Edges leaving `n` that may be driven away from `n` (respects one-way), ascending ids.
    fn edges_from(&self, n: u32) -> &[u32];
    /// Closest point on any edge: (edge, fraction from `a`, point, distance).
    fn nearest_on_road(&self, p: [f32; 2]) -> Option<(u32, f32, [f32; 2], f32)>;
    fn district_at(&self, p: [f32; 2]) -> Option<DistrictId>;
    fn pois(&self) -> &[PoiInfo];   // kind (Safehouse, Garage, RaceStartGrid, PoliceStation,
                                    // Helipad, Camera), pos, heading, radius
}
```
Line of sight stays on `WorldQuery`. E's `RoadGraph` keeps its routing/prediction code on top
of the trait. Today the same data is available as `wp_world::World::road_graph()`
(`RoadGraphView`), so `wp_sim` can already build E's graph with `add_node`/`add_segment`
(district mapping: Core/Commercial → Core, Residential/Suburb → Suburb, Industrial/Port →
Industrial).

**#8 `DistrictId`**: `pub enum DistrictId { Core, Commercial, Residential, Industrial, Port,
Suburb }` — exactly `wp_world::network::DistrictKind` (moved to contracts, re-exported by
`wp_world`). Per-zone identity is not needed by G or E; if it becomes needed, add
`ZoneId(u32)` alongside.

## Changes outside my crates
- `Cargo.toml` (workspace): added member `tools/city-import`.
- `Cargo.lock`: `postcard` (wp_world), `osmpbf` (city-import only).
- `justfile`: added `city` and `traffic-bench` recipes.
- `tools/lint-determinism/allow.txt`: removed the five `crates/wp_world/... f64` entries (run 2 feedback).
- `crates/wp_vehicle/src/lib.rs`: added `Vehicle::set_pose_and_speed(pos, yaw, speed_mps)`
  (Lead: the stub this went into was replaced by 02's real model; the method now lives in `wp_vehicle/src/vehicle.rs`, same signature.)
  (7 lines, `reset` + speed) for LOD promotion — see contract request 1.

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| 400 agents, traffic + signals + violations per 60 Hz frame (release) | cloud container, 4 vCPU | avg 0.06 ms, p99 0.15 ms, max ≈ 1.4 ms | not measured | 2 ms (browser 4 ms) |
| Same with collision diagnostics | same | avg 0.20 ms | — | — |
| `World::load` procedural_12x12 | same | ≈ 17 ms (debug ≈ 0.2 s) | — | — |
| `World::procedural(ProcGenConfig::small())` (6×6, 650 lanes, 521 parked cars) | same | ≈ 5 ms (debug ≈ 0.06 s) | — | — |
| `line_of_sight`, random 300 m sight lines at 1.5 m, procedural_12x12 | same | 1.1 µs/query | — | — |
| 10-min soak × 18 (9 seeds × 0 / 5 % aggressive) | same | 0 civilian–civilian collisions | — | < 1 % with aggressive |

## Known gaps
- No rigid bodies for physical traffic until the sim loop handles `LodChange` (Needs 3).
- OSM importer verified on synthetic data only (download hosts blocked here); see
  `tools/city-import/README.md`.
- In saturated flow a few permissive left / minor-road cars starve > 2 min and get recycled
  when unseen (0–11 per 10 min with 400 agents); no protected-left phases yet.
- One civilian car model; no buses; sightline rule is distance-only (LOS exists now; spawning
  could use it — kept distance-only for determinism of spawn tests).
- Kinematic traffic is not replicated yet (contract request 3).

## Lead feedback (2026-10-19, run 2)

**Merged (machine D)** — `b418ac5`. Tests pass; the procedural city loads and steps
deterministically inside `wp_sim` (new test).

Lead changes in your area (pull them):
- Your `Vehicle::set_pose_and_speed` went into the old stub `wp_vehicle/src/lib.rs`, which A replaced
  with the real model. It is re-added on the real `Vehicle` (`wp_vehicle/src/vehicle.rs`), same signature.
- `traffic.rs` IDM: `powi(4)` → multiplies (the determinism lint rejects `powi`).
- Your `f64` time accumulators are allowed by a file-level entry in `tools/lint-determinism/allow.txt`
  (exact IEEE ops, identical on wasm). **Please move them to integer tick counts** (`u64` ticks ×
  `PHYSICS_DT`) and delete those allow lines.

Your `wp_sim` needs — status:
| # | Status |
|---|---|
| 1 load a city | **Done**: `Sim::set_world(World)` (call before spawning). `SimConfig::city` not added (non-breaking path chosen). |
| 2 tracked vehicles | **Done**: every vehicle each tick, `player = true` for all until AI/police flags exist. |
| 3 physical traffic bodies | **Not yet** — next lead run (body + DamageModel on `Promoted`, removal on `Demoted`). Traffic stays kinematic. |
| 4 static props | **Done**: every `static_props()` entry becomes a dynamic Rapier box with its mass. |
| 5 debris hazards | Not yet (next run, with 3). |
| 6 race routes | Waits on prompt 15. |
| 7 testbed agents export | Asked of machine C. |
| 8 traffic soak scenario | Asked of machine I (simcli can call `set_world` now). |

Perf note: `World::procedural(default)` takes ~50 s in a debug build (wp_sim test) — please check
whether procgen can be cheaper or offer a small test config.

Contract requests: #1 teleport — **kept as the inherent method** (no trait change); #2 violation
`pos` **accepted**; #3 `TrafficAgentSnapshot` accepted, designed with F; #4 `wp_math::atan2`
**accepted** (lead moves it); #5 city source — done in `wp_sim` without a contract change.

### Response from D (run 2 feedback)
- **Integer ticks — done.** `wp_world` has no `f64` left: `World` counts physics ticks
  (`World::tick()`, `sim_time_s()` now returns `f32` derived from it), `CityMap::signal_tick: u64`
  replaces `signal_time_s`, `signals::group_state(sc, group, tick: u64)` works in exact integer
  ticks (`signals::ticks_from_secs`), traffic and violations use the 60 Hz traffic step count
  (`Traffic::step_count()`, `steps_to_s`). The five `wp_world` lines are deleted from
  `tools/lint-determinism/allow.txt`; `lint-determinism` reports 0 errors. Behaviour is unchanged
  (soaks still 0 collisions; simcli goldens pass without re-blessing).
- **Debug load time — done on my side.** `CityMap::new` conflict detection now uses bounding
  boxes + an early-exit distance test: 1.44 s → 0.17 s in debug (14 ms release), identical
  conflict sets. Added `ProcGenConfig::small()` (6×6 grid with every feature: highway, diagonals,
  bus lanes, alleys, signals, ring). The remaining ~50 s of `procedural_city_loads_props_and_steps`
  is not the world: the default city has **3190 parked cars**, which `Sim::set_world` turns into
  dynamic Rapier bodies stepped in a debug build. Measured here (not committed, your crate):
  the test takes 8.9 s with `ProcGenConfig::small()` and 0.3 s with `parked_cars: false` (the
  `prop_count() > 0` assert then fails, as expected). Suggestions for `wp_sim`: use
  `ProcGenConfig::small()` in that test, and create parked-car bodies asleep
  (`RigidBodyBuilder::dynamic().sleeping(true)`) or fixed until hit, which also matters in
  release with ~3000 props.

### Response from D (contract v0.2.0, lead run 3)
- Merged `main` (v0.2.0) into `ws/world`; the lead's first-cut changes in `wp_world` compile and
  pass (`GroundQuery` → `FlatGround` for physical traffic until `wp_sim` gives it the Rapier
  ground; violation `pos`; `atan`/`atan2` from `wp_math`).
- **Line of sight — taken over.** Building grid + 2D DDA over the cells the sight line crosses
  (only those buildings are tested; 1.1 µs per 300 m query, release). Fixed a case in the first
  cut: it tested the height only where the line *enters* a footprint, so a line from above the
  roof down to a point behind/inside the building (helicopter → car) was wrongly clear; now the
  lowest point of the line inside the footprint is checked (both ends of the span, since height
  is linear). Tests: hand cases (through, beside, over, descending into, endpoint inside,
  corner clip) and 3000 random lines against brute force.
- **v0.3.0 shapes** for `TrafficAgentSnapshot`, the shared road-network trait and `DistrictId`
  are above (#6–#8). As a no-contract step, `World::road_graph()` exposes the road-level graph
  E needs now.

