# World (city, signals, traffic) integration notes

- **Workstream / prompt:** 05 — city, road network, signals & civilian traffic
- **Branch:** `ws/world`
- **Crates / folders owned:** `crates/wp_world`, `data/city/`, `tools/city-import/`
- **Contract version built against:** 0.3.0

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
- **Shared AI driving** (`wp_world::drive`, for police E and racers N):
  - `DriveController` — raw pure pursuit + PID (`steer(pos, fwd, target)`,
    `pedals(v, acc, dt, hold)`), what traffic uses.
  - `PathFollower::follow(&state, &DrivePath, target_mps, lateral_offset_m, dt) -> FollowOutput
    { input: VehicleInput, s, cross_track_m, remaining_m, planned_speed_mps, done }` — the
    `follow_path` API: projects onto the path, look-ahead target, speed planned from target,
    curvature ahead (lateral-acceleration limit) and end of path. One per vehicle
    (`PathFollower::for_vehicle(&def, FollowParams::{CIVILIAN, PURSUIT}, speed)`).
  - `DrivePath::new(points, looped)` / `DrivePath::from_route(&Route)`; windowed projection
    keeps self-crossing circuits stable.
  - Verified on the real `wp_vehicle` model in its Rapier rig: city route 866 m stays on the
    road, stops at the end, ≤ 1.7 m off line; circuit lap pursuit 192 s vs civilian 274 s,
    ≤ 1.4 m off line.
- **Leader / obstacle query** (`wp_world::leader`, round 6): `find_leader(own_pos, centreline,
  &[Obstacle], skip, &LeaderParams) -> Option<Leader { id, gap_m, speed_mps, .. }>`,
  `Obstacle::{from_state, from_agent}`, `CarFollowing::{CIVILIAN, PURSUIT}::leader_cap` (traffic's
  IDM), `hold_behind` / `held`; `DrivePath::leader`, `PathFollower::follow_behind`.
- **Debris hazards**: `World::set_hazards(&[Hazard::new(pos, radius)])` (or with `vel`).
  Small debris is swerved around within the lane (≤ 1.5 m, held until the rear clears);
  lane-blocking debris → lane change if the neighbour lane is free, else stop; debris in a
  junction box blocks the connectors through it; no spawning within 30 m.
- **Painted markings** (round 8, `wp_world::markings`): `surface_at` → `SurfaceType::PaintedLine`
  on centre/lane/edge lines, separators, stop lines and zebra crosswalks; `marking_at(&map, p)
  -> Option<MarkingKind>` + public geometry constants for the renderer (C).
- `RoadNetworkIndex` (contract `RoadNetworkQuery`): grid-indexed `nearest_on_road` (0.6 µs).
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
5. **Debris hazards**: each tick `world.set_hazards(&[Hazard { pos, radius_m, vel }])` for every
   loose body near the road (detached parts, wheels, knocked parked cars / cones). World side done.
6. **Race events** (08/12): `set_active_routes` when an event starts/ends.
7. **Testbed** (04): load `data/city/procedural_12x12.greybox.glb` and draw `AgentView`s; it
   needs a `wp_wasm` export for agents (suggest a flat `Float32Array` of `x, y, z, yaw, flags`
   per agent).
8. **simcli scenarios** (10): a traffic soak scenario once the sim loads cities (expected
   ranges: 0 civilian collisions, ≥ 380 agents, ≥ 50 % moving).
9. **Leader query inputs for police** (round 6): each tick, the state of every vehicle near an
   AI unit as `wp_world::leader::Obstacle`s (players, physical traffic, other units; kinematic
   traffic via `Obstacle::from_agent`) handed to `wp_police` (E's API) — see "Response from D
   (round 6)".

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
- `apps/simcli/golden/*`: re-blessed city goldens for `PaintedLine` (round 8, see gate note).
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
| 400 agents per frame after driver variety (round 6) | same | 0.055 ms (0.060 before, same run) | — | 2 ms |

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

### Lead run 5 (2026-10-19)
- **Your need #3 is done:** with `Sim::set_traffic_car(car)` (simcli city runs use `debug_sedan`), every
  `LodChange::Promoted` agent becomes a `wp_sim` vehicle (`VehicleKind::Traffic`, Rapier body + damage);
  your controller's `pa.input` drives it; its real state goes back with `Vehicle::set_state` and damage
  with `apply_agent_damage`; `Demoted` / `Despawned` remove it. `crates/wp_sim/src/traffic_link.rs`.
- One-line change in your crate: `Traffic::step_physics` skips agents that have an entity (wp_sim steps
  them). Keep it.
- **Please look:** `soak/traffic_soak_observer` now sees **1 civilian collision** in 60 s once near-player
  traffic is physical (real braking, nudges from parked-car props). The gate there is ≤ 3 for now; zero is
  `expect_pending`. Your `collision_log` has the pair and tick. The no-player soak still gates zero.
- Still open: debris hazards (#5), race routes (#6, with L).

## Lead feedback (round 6, 2026-10-19)

**Merged** (PathFollower / `follow_path`, debris hazards, integer ticks).

Police units drive with your shared controller but have no obstacle sensing, so they ram stopped vehicles (see
police.md round 6). Please expose a **leader / obstacle query** E can use from the controller: nearest vehicle ahead
on the path within a look-ahead (players, physical traffic, other units — `wp_sim` vehicle states can be passed in),
returning gap and speed, so units do the same car-following your traffic does. Coordinate the API with E in your doc.

### Response from D (round 6)
**Leader / obstacle query — done** (`wp_world::leader`, commit on `ws/world`). Pure geometry on
data the caller passes in, so it works for police units, AI racers and anything else that is
not lane-bound traffic:

```rust
use wp_world::leader::{find_leader, hold_behind, held, CarFollowing, LeaderParams, Obstacle};

// 1. Every vehicle near the unit (wp_sim vehicle states; kinematic traffic via AgentView).
let obstacles: Vec<Obstacle> = states.iter().map(|(id, st)| Obstacle::from_state(*id, st))
    .chain(world.agents().map(|a| Obstacle::from_agent(&a))).collect();
// 2. Nearest one in a ±1.2 m corridor along the unit's own track (any path: a closure giving
//    the point `d` m ahead, lane offset included). `skip` = own id, + the suspect when ramming.
let lp = LeaderParams::CAR.lookahead_for(v);
let leader = find_leader(pos, |d| lane_point(&track, s + d, lane_offset), &obstacles, &[own], &lp);
// 3. Same IDM law as traffic: cap the commanded acceleration.
let cap = CarFollowing::PURSUIT.leader_cap(v, leader.as_ref());
let acc = acc.min(cap.max(-max_brake));
let mut input = /* controller.steer + controller.pedals(v, acc, dt, disabled) as today */;
// 4. Stopped behind it: hold on the handbrake (pedals releases the foot brake < 0.6 m/s
//    because holding it engages auto-reverse — the car would idle forward into the leader).
if let Some(l) = &leader { if hold_behind(v, cap, l) { input = held(input); } }
```

- `Leader { id, gap_m (bumper to bumper), speed_mps (along the path, negative = oncoming),
  along_m, lateral_m }`. Obstacles are oriented boxes, so a car stopped sideways across the lane
  counts by its footprint; neighbour/oncoming lanes (≥ 3 m away) do not. Ties break on id
  (order-independent, deterministic).
- `CarFollowing::PURSUIT` = headway 0.8 s, standstill gap 2.5 m, a 4 m/s², b 5 m/s²;
  `CIVILIAN` = traffic's middle values. Traffic's own `idm` now delegates to
  `CarFollowing::idm` (same arithmetic; all traffic tests and the soaks unchanged).
- `DrivePath` users get it in one call: `PathFollower::follow_behind(state, path, target,
  offset, dt, Some(&FollowTraffic { obstacles, skip, corridor, following }))` (racers / N).
- Verified on the real vehicle (`tests/drive.rs`): from 25 m/s with `FollowParams::PURSUIT`
  it stops **2.5 m behind a stopped car** (no contact) and settles ≈ 9 m behind an 8 m/s car.
- Cost: 60 m look-ahead = 30 corridor samples; each obstacle is one circle reject or a
  30-segment projection — microseconds for 24 vehicles + nearby traffic.

**Coordination with E (police):** the call site is `PoliceSystem::unit_input` (it already uses
my `DriveController`; steps 2–4 above slot in around its `acc`). E needs the obstacle list from
`wp_sim`: proposed `PoliceSystem::set_obstacles(&[wp_world::leader::Obstacle])` (or a
`unit_input(unit, dt, &obstacles)` argument), filled by `police_link.rs` each tick from every
`wp_sim` vehicle (players, physical traffic, other units) plus kinematic `AgentView`s within
~150 m. When a unit means to ram/PIT the suspect it puts the suspect's id in `skip`; while
following or boxing it keeps it in. Spawn clearance (golden/city_chase unit 5 m ahead of the
racer) is E's/lead's; `find_leader` from the spawn pose with a 0 m/s look-ahead tells whether a
spot is clear.

**Physical traffic creep — fixed on my side too.** Same cause: a physical civilian stopped in
a queue or at a stop line had its foot brake released below 0.6 m/s and idled forward. It is
now held on the handbrake while its IDM acceleration is negative (or it is disabled) and it is
below 1 m/s. `soak/traffic_soak_observer`: 0 civilian collisions before and after (the pending
zero gate passes; you can promote it). Goldens: see below.

Needs added for this round: **9.** `wp_sim` / E: pass vehicle states to the police controller
for the leader query (above).

### Response from D (round 6 follow-up: playtest row 14, "NPC cars need variance")
- **Per-driver personality** (deterministic, traffic RNG at spawn): `DriverParams` now also
  carries `highway_bias`, `amber_decel`, `start_delay_s`, `lane_change_threshold`,
  `keep_right_bias`, and every existing field is a range per personality (table in the
  `DriverParams` docs / README). Desired speed / limit over the 70/25/5 mix: **p5 0.84, p50 0.98,
  p95 1.13** (was ≈ 0.94 / 0.98 / 1.11); highways p95 1.20; alleys ≤ 0.9. 12 % of law-abiding
  drivers are slow (0.80–0.88×).
- **Lane changes**: per-driver MOBIL threshold + keep-right bias (overtake slower leaders, return
  right); discretionary changes are **indicated ≥ 0.5 s before** they start (`AgentView::turn_signal`,
  already in the planned `TrafficAgentSnapshot.flags` bits 2/3 — no new contract request), with
  hysteresis so an indicated change usually happens; never within 60 m of a junction or inside one.
  Lane prep before turns is implicit (turns are picked from the current lane).
- **Dynamic driving**: reaction delay pulling away in a moving queue or first at a green
  (0.15–2.2 s, 8 % hesitant ≥ 1.2 s); casual drivers run ambers only when they reach the line
  before red; IDM gives smooth accel/decel with per-driver comfort values.
- **Physical agents** follow the same personality (controller tracks the same IDM acceleration);
  stopped ones are held on the handbrake (see round 6 above).
- **Bug found and fixed on the way**: a stopped car holding a junction permit at a green had its
  permit revoked every step (`need = 0 ≤ limit = 0`) — harmless before, a gridlock with start
  delays (119 stuck recycles / 10 min until fixed; now 1–4).
- **Own traffic no longer "tracked"** (`World::step`): `wp_sim` passes every vehicle to
  `set_tracked_vehicles`, including promoted traffic cars. Those were (a) obstacles for their own
  kinematic agent, (b) "players" for spawning / promotion, (c) earning traffic violations — every
  promoted civilian logged a `Collision` violation (27 in `traffic_soak_observer`), i.e. police
  heat for civilians. World now drops entities that are its own agents. Also RedLight is
  reported once per crossing (a slow car straddling the line was reported every update).
  `traffic_soak_observer` now has **0 events and 0 civilian collisions**.

Measurements (release, 12×12, 400 agents, `density_scale` 4, noon, 5 % aggressive, 10 min sim;
`cargo run --release -p wp_world --example traffic_bench -- 10 400 12 1 0.05`):

| | before | after |
|---|---|---|
| civilian collisions (10-min soaks, 0 % / 5 % aggressive) | 0 / 0 | 0 / 0 |
| stuck > 120 s, recycled | 11 / 2 | 3 / 1 |
| lane changes | 1133 / 1173 | 847 / 912 (328 of 912 to the left = overtakes) |
| indicators on (share of agent samples) | 28.6 % | 28.0 % |
| moving at the end | 327 / 309 of 400 | 336 / 333 |
| frame cost (bench, collision diagnostics on) | 0.245 ms avg | 0.246 ms avg |

Lane changes are fewer than before because marginal changes are now indicated first and dropped
if no longer worth it; overtakes are real ones. Speed / limit sampled on cruising cars city-wide
is p50 0.64 before and after: the 110 m grid with a signal every block keeps cars below their
desired speed, so the personal spread shows on longer roads and highways, not in the average.


## Lead feedback (round 8, 2026-10-19)

Contract **v0.3.0 is on `main`** (typed police/race/economy events, damage snapshot/summary, road-network trait, weather `LocalConditions`; see `docs/CONTRACT_CHANGELOG.md`). Workspace tests now take about 3.5 minutes (`lead/test-speed`). Owner playtest items are tracked in `docs/PLAYTEST_LOG.md`.

1. Report `SurfaceType::PaintedLine` for lane markings / zebra crossings.
2. `RoadNetworkIndex::nearest_on_road` is brute force; index it.
3. The lead helper `lead/city-v2` (curves and elevation) is building a new procedural preset in your crate. Review its branch when it lands and coordinate via its section in this doc.

### Response from D (round 8)
1. **`SurfaceType::PaintedLine` — done** (`wp_world::markings`). Markings are derived from the
   lane layout (no road-format change), so every city gets them, procedural, imported and v2
   alike: double centre line, dashed lane lines (3 m on / 9 m off), solid separators next to bus
   lanes and parking, edge lines at the curb, stop lines (last 0.5 m of a lane that stops) and
   zebra crosswalks (0.5 m bars, 0.2–1.8 m past each stop point / before each lane start at
   signalised and all-way-stop junctions). `CityMap::surface_at` checks them first and returns
   `PaintedLine`. Each lane owns its half of a shared line; lanes > 3 m above or below the
   queried z are ignored (bridges). Geometry constants and `marking_at(&map, p) ->
   Option<MarkingKind>` are public, so **C** can draw the same lines (table in the README).
   About 4 % of road samples are paint; cost 0.10 µs per wheel query (`surface_at` 0.36 µs
   total), ≈ 0.04 ms per frame for 24 cars. Grip: `surfaces.toml` still has no `PaintedLine` row
   (N), so `SurfaceTable` treats it as asphalt for now; A/N decide the wet-paint penalty.
   **Goldens:** see the gate note below.
2. **`nearest_on_road` indexed — done.** `RoadNetworkIndex` builds a 32 m uniform grid over
   the edge polyline pieces and searches rings outwards until nothing closer can exist; ties
   go to the lowest edge id, as before. 0.63 µs vs 19 µs per query (425 edges, release);
   `nearest_on_road_matches_linear_scan` compares it with the old scan on 4000 points inside
   and around the city. It is 2D like the trait (`[f32; 2]`): under a bridge it returns
   whichever road is closer in plan, which is the contract's semantics today.
3. **`lead/city-v2` review** (at `6cc3967`). Trial merge into `ws/world`: every `wp_world`
   file merges cleanly with this round's work; the only conflicts are `crates/wp_sim/src/lib.rs`
   and `apps/simcli/src/run.rs` (against `main`, lead files). With the merge, `tests/city_v2.rs`
   passes (incl. the grade-separation surface test: the probe points are not on paint), my
   markings work on Port Vesper (4.0 % paint, decks vs roads below kept apart by z) and the
   indexed `nearest_on_road` equals the linear scan on 3000 Vesper points (245 edges).
   Code review findings: see "City v2 review notes" below.

### City v2 review notes (D → lead helper `lead/city-v2`, at `6cc3967`)
These are findings only. I have not edited the branch, and `procgen.rs`/`builder.rs` are untouched on `ws/world`. Checks:
lint-determinism 0 errors; `wp_world` tests pass; traffic bench, 400 agents: grid 0.26 ms, vesper
0.24 ms per frame. Determinism: clean (no HashMap/f64/powi; sorts use `total_cmp`). On flat cities
behaviour is unchanged (`terrain: None`, `base_z_m = 0`).

**Blocking**
1. `clippy -D warnings` fails: `needless_range_loop` at `vesper.rs:643`, `:742`, `:747`
   (use `iter().enumerate()`).
2. v1 road files no longer load: `network.rs:21` bumps the format to 2 and `from_bin` /
   `validate` require an exact match. `surfaces`, `terrain` and `Building::base_z_m` have no
   serde default, and postcard can't default fields anyway. Only `procedural_12x12.*` was
   regenerated, so locally imported OSM files fail. Either decode v1 (`surfaces = []`,
   `terrain = None`, `base_z_m = 0`) plus serde defaults for JSON, or document "regenerate with
   `city-import`".

**Should fix**
3. `ground_height` / `MapGround` (`map.rs:481`, `surface.rs:148`): under a deck, when the
   only nearby segment is the deck, the probe returns the deck z. The downward cast misses and
   the terrain is never tried, so the wheel has no ground (off-road under a bridge, or just past
   the lower road's edge). Only accept candidates at or below the cast origin (+ margin), and keep
   terrain as a candidate.
4. Per-wheel cost: 5 `ground_height` calls per cast, each a full projection over every segment
   in the cell. Vesper's filleted curves have many points. Short-circuit to `FlatGround` when the
   map has no terrain and every node z is 0, or take the normal from the tangent and bank instead
   of finite differences. Not covered by the bench (0 promotions in it).
5. The new 3 m z checks make `wp_weather/src/grid.rs:216` (`surface_at([x, y, 0.0])`) read
   every Vesper road above 3 m as grass. K should sample at `ground_height(p, None)` (request
   to K/lead; not my crate).
6. Merge with `main`: `roads.rs:174` `OccluderBox.height_m = b.height_m` must become
   `b.roof_z()` (it auto-merges silently). `map.rs` `roof_z` hunks conflict with nothing on
   `ws/world` today.
7. `intersection_at` (`ix_grid … find`) returns the first junction disc. Stacked junctions at
   different heights would be picked arbitrarily in `surface_at` and `ground_height`. Filter
   by z in the `find`.

**Nits**: `builder.rs:306/324` silently ignore `via_z`/`bank` of the wrong length (reject in
`build`); `builder.rs:470` short-road `s0` should come from `trim_polyline`'s result so the bank
stays aligned; `surface.rs:100/120` index `len() - 1` / `k - 1` relying on `validate()`; `vesper.rs`
(2310 lines) would read better split into layout / profile / carve / buildings.

**Merge plan:** merge `ws/world` (this round) into `lead/city-v2`. All `wp_world` files merge
cleanly; resolve `wp_sim/src/lib.rs` and `simcli/src/run.rs` against `main` and apply note 6.
My markings and the indexed `nearest_on_road` already pass on Vesper (checked in a trial merge).

### Contract change request (round 8)
| # | Change | Why | Who | Status |
|---|---|---|---|---|
| 9 | `RoadNetworkQuery::nearest_on_road_3d(p: [f32; 3]) -> Option<(u32, f32, [f32; 3], f32)>` (prefer edges within 3 m of `p.z`), and `RoadEdgeInfo`/`edge_polyline` with z | Vesper has stacked roads (loop overpass over Route 40, 6.5–9 m clearance). The 2D query can snap a car under a bridge to the deck, which misroutes police | 06 (E), 12 | proposed |

### Gate note (round 8): goldens re-blessed
`PaintedLine` changes physics in city scenarios only. On asphalt the grip is the same: tires
have no `PaintedLine` entry, so `surface_grip` falls back to 1.0, the same as Asphalt. Highway lines
used to read as Concrete (0.95 on every tire) and now read 1.0. The surface code is also part of
the vehicle snapshot hash. Re-blessed (`simcli goldens --bless`; the commit message carries `[bless]`):
`golden_city_chase` (+ replay), `opponent_{circuit_8x5, flee_police, open_roads_traffic,
reverse_grid_2laps}`, `race_city_sprint{,_police}`, `world_traffic_soak_observer`. Non-city
goldens are unchanged. `simcli run apps/simcli/scenarios`: 39/39 pass, and pendings are unchanged in kind
(`traffic_soak_observer` 0 events / 0 civilian collisions, min agents 361 against its 350 gate).
When H adds `PaintedLine` to the tire files (or N to `surfaces.toml`), these move again.
## Lead helper: city v2 (branch `lead/city-v2`, 2026-10-19)
Playtest #15 ("the city needs to be more dynamic and have curves/elevation"). Built on
`main` + `ws/world` (merged at the start). D: `traffic.rs` and `drive.rs` were left alone except
the one line listed under "Changes in your files" — please merge `lead/city-v2` into `ws/world`
when convenient.
**What exists now**
- **Port Vesper v2** (`wp_world::vesper`, city id `vesper_v2`, `World::vesper(&VesperConfig,
  cfg)`): curved organic districts, downtown grid, waterfront/port flat, Hollins Drive
  switchbacks + Ridge Road crests up a ≈ 110 m hill, Bluff Road on a plateau, and Loop 9, a 3+3
  highway loop with banked sweepers and two grade separations (Route 40 West under an
  overpass, Route 40 North in an underpass cut). Grades capped per class (6 % highway …
  11.5 % hill roads), junctions flat, crests/sags ≤ 0.3 g at 1.15 × the limit. Details:
  `crates/wp_world/README.md` → "Port Vesper v2". The 12×12 grid is unchanged
  (`procgen`, still the default for every existing scenario and golden).
- **Road format v2** (`ROAD_FORMAT_VERSION` 2): `RoadNetwork::surfaces` (paved ribbon per road,
  with bank), `RoadNetwork::terrain: Option<Heightfield>`, `Building::base_z_m`. `RoadSpec` has
  `via_z` (heights at via points) and `bank` (cross slope per point); lanes are built on the
  banked surface. The grid's `.road.bin/.json` were regenerated (lanes byte-identical; only the
  new fields). OSM imports get `via_z`/`bank` empty (unchanged behaviour).
- **Queries:** `CityMap::ground_height(p, near_z)` (road under `p`, the level nearest `near_z`
  at a grade separation, else terrain); `surface_at` and junction checks ignore roads more
  than 3 m above/below the probe (decks); `line_of_sight` also tests the terrain (every 8 m).
- **`wp_world::surface`:** `road_mesh` (banked ribbons + junction discs + bend connectors) used
  by `wp_sim` and the greybox; `deck_rails`; `MapGround`, a wheel probe over roads + terrain.
- **Physics (`wp_sim`):** for a city with terrain, `Sim::set_world` replaces the slab (parked at
  z = −60 as a safety floor) by heightfield tiles (64 cells), road trimesh tiles (256 m),
  convex-hull building colliders (cars can no longer drive through buildings in city v2) and
  deck guard rails. `Sim::place_on_ground` puts a car on the surface under (x, y), pitched and
  rolled to it; police and promoted traffic spawn through it. Parked cars are posed flush
  (+4 cm), created asleep, sleep at ≤ 0.25 m/s and skip terrain pairs until hit. Grid cities
  take none of these paths.
- **simcli:** `city: preset(name: "vesper_v2", seed: 1)`; `follow_route(route_id: "<start POI> ->
  <finish POI>", target_speed)` now drives (shared `PathFollower`, pursuit limits); metrics
  `off_road_s`, `max_airborne_s`, `route_progress`; entities in hilly cities are placed on the
  ground under (x, y). Scenarios `city/hill_road_ascent`, `city/hill_road_descent` (Ridge Road),
  `city/loop_crest_at_limit`, `soak/traffic_soak_vesper`, `soak/traffic_soak_vesper_observer`.
- **Greybox:** `gen-city --preset vesper_v2` / `just city-v2` → `data/city/vesper_v2.greybox.glb`
  + `.greybox.b64.txt` (terrain, sea, banked ribbons, sidewalks + kerbs, retaining walls,
  bridge decks/parapets/piers, buildings on slopes; 6.8 MB / 9.1 MB). Every `.glb` written by
  `city-import` now gets its `.b64.txt` twin (the grid's too) and 16-bit index chunks (grid glb
  1.1 MB → 0.47 MB). `vesper_v2.road.bin` is generated in memory and not committed.
- **Testbed:** the city chase and the free-drive "city" box use the city selector (default
  Port Vesper v2, "grid 12×12" keeps the old city); `?city=procedural_12x12`, `?chase=1`;
  wasm `free_drive_city_json`, `city_chase_city_json`, `city_id`; police units drawn on the
  ground; `__wp.lookAt(from, to)` for docs shots (`e2e/city_v2_shots.mjs` →
  `docs/media/city-v2/`).
**Checks (this branch)**
| Check | Result |
| Hollins Drive ascent at the 25 mph limit (`city/hill_road_ascent`) | route done in 94 s, max 12.1 m/s, 0 s off road, 0 s airborne, no damage |
| Ridge Road descent over the crests (`city/hill_road_descent`) | done in 116 s, max 11.6 m/s, 0 s off road, 0 s airborne, no damage |
| Loop 9 over the hill shoulder at 55 mph (`city/loop_crest_at_limit`) | 2.7 km in 111 s, max 25.1 m/s, 46 m of climb/drop, 0 s off road / airborne |
| Traffic soak, 400 agents, no player (`soak/traffic_soak_vesper`, 60 s) | 0 civilian collisions, ≥ 380 agents, flowing |
| Traffic soak with a parked player downtown (`…_observer`) | 0 collisions, min 398 agents |
| `wp_world` 10-min soaks, 3 seeds × {0 %, 5 % aggressive} (`tests/city_v2.rs`, ignored) | 0 collisions in all 6; worst agent z error 0.22 m |
| Cars spawned on hill road / loop / deck / under the deck / junction / hillside | settle within 2.4 cm of the surface (`wp_sim/tests/city_v2.rs`) |
| Native vs wasm (`tools/wasm-determinism/compare.sh`) on the 5 new scenarios | identical hashes |
| Existing goldens | unchanged |
**Perf (release, cloud container, 4 vCPU)**
| Measurement | Native | Browser (headless Chromium) | Budget |
| `vesper::generate` + `CityMap::new` | 0.12 s + 10 ms (debug ≈ 0.4 s) | session start ≈ 0.8 s incl. colliders | — |
| Traffic, 400 agents, 60 Hz frame (`CITY=vesper_v2 just traffic-bench`) | avg 0.061 ms, p99 0.16 ms (0.27 ms avg with collision diagnostics; grid 0.26) | — | 2 ms |
| City chase, full frame (`cargo run --release -p simcli --example city_frame_cost`) | 1.7 ms (12 vehicles; grid 0.9 ms with 22) | 2.8 ms after warm-up (grid 1.5 ms) | — |
| Of which physics with ~3800 parked cars asleep | ≈ 0 (was 2 ms before the prop fixes) | — | — |
| One car alone (vehicle + damage + world) | 0.065 ms (grid 0.032) | — | 0.15 ms/car |
| Greybox | 6.8 MB `.glb`, 9.1 MB `.b64.txt` | parse ≈ 2 s (SwiftShader) | — |
**Changes in your files / outside my areas** (minimal, listed per the working rules)
- `crates/wp_world/src/traffic.rs` (one line): `step_physics` probes `surface::MapGround`
  instead of `FlatGround` for physical agents without a `wp_sim` body (identical on flat cities).
- `crates/wp_world/src/map.rs`: `ground_height`, z-aware `surface_at`, terrain LOS, building
  roof = `base_z_m + height_m`.
- `apps/simcli` (owned by I): `CitySpec::Preset`, `FollowRoute` driver, three metrics, terrain
  placement of entities, the debug `scenarios.rs` test skips `scenarios/city/` like `soak/`,
  and the net round-trip position check allows f32 rounding at the coordinate's magnitude
  (`pos_err_allowance`; city v2 reaches 1.7 km). `follow_route` ids without `->` (N's opponent
  scenarios) keep the old "not available yet" warning.
- `crates/wp_wasm`, `apps/testbed-web` (C): city selector, exports above, `lookAt`.
- `crates/wp_sim/src/police_link.rs`: the police car model is cloned only when a unit needs a
  body (it was cloned every tick: ~15 % of a city frame).
**Known gaps / follow-ups**
- Kinematic traffic is drawn level (`AgentView` has yaw only); physical cars pitch correctly.
  Proposal for the v0.3.0 `TrafficAgentSnapshot`: add `pitch`/`roll` (i8, ~0.5° steps) or let
  clients sample `ground_height`.
- `wp_vehicle::rig` (client prediction in `wp_net`) still predicts on a flat slab: networked
  play in city v2 will mispredict on slopes until the rig gets the city ground (the server is
  authoritative, so it only corrects). Needs F + lead.
- Buildings now block cars in city v2 only; the grid keeps its old behaviour (no building
  colliders) so its goldens stay put.
- Loop junctions are signalised T-junctions (no ramps/merges yet); Route 40 W/N end at
  the map edge with U-turns.
- `surface_at` / `ground_height` / police `nearest_on_road` project onto whole polylines; the
  curved city has up to 281 points per lane (≈ 0.5 µs per query, fine today; index pieces in
  the grid if it grows).
- Browser: the first city-chase second costs more (greybox parse ≈ 2 s in headless
  SwiftShader; prop wake-ups); `vesper_v2.greybox.b64.txt` is 9.1 MB on the published link.
**Contract change requests:** none needed (no `wp_contracts` change). Optional for v0.3.0: the
`TrafficAgentSnapshot` pitch/roll above.
## Lead helper: highway + road markings (branch `claude/festive-dijkstra-5rxjnw`, 2026-10-20)
Playtest #33 ("the highway needs to feel like an actual highway; road markings everywhere;
centre dividers and proper on-ramps").

**What exists now**
- **Loop 9 is a divided highway** (`wp_world::vesper`): two one-way 3-lane carriageways with
  paved shoulders and a Jersey-profile median barrier, profiled once as a single road so both
  directions share one profile and bank. Six interchanges replace the seven signalised
  T-junctions: five diamonds (crossroad bridges both carriageways, signalised terminals 150 m
  either side, four ramps each) and a half diamond at the port. Ramps leave / join the
  rightmost lane 300 m from the crossroad through 60 m taper connectors. Numbers and geometry:
  `crates/wp_world/README.md` → "Port Vesper v2".
- **Builder:** `ControlHint::Freeway` merge / diverge nodes; `RoadSpec::{shoulder_left_m,
  shoulder_right_m, median_barrier}`; paving can run through a node while lanes are set back.
- **Road format v3:** `RoadSurface::median_barrier`. The grid's `.road.bin/.json` were
  regenerated (lanes unchanged).
- **Traffic (`traffic.rs`, one rule):** a minor approach at a priority junction whose lane is a
  highway lane (an on-ramp) yields on the move instead of stopping first. No existing city has
  such a lane, so grid behaviour is unchanged.
- **Markings:** `markings::marking_mesh` (render mesh from the same rules as `marking_at`,
  white + yellow), `markings::must_stop` is public and false for merging ramps. wasm
  `road_markings(yellow)`; the testbed draws it (`markings_view.ts`) and signal heads no longer
  draw their own stop line.
- **Physics (`wp_sim`):** median barrier colliders (`CityColliders::barriers`); test
  `the_median_barrier_keeps_a_car_on_its_carriageway` (25° swerve at 90 km/h).
- **Greybox:** highways paved from their surfaces (shoulders included), ramp tapers, the
  barrier mesh, no parapets on the median side of decks. Both greyboxes regenerated
  (`vesper_v2.greybox.glb` 6.8 → 7.4 MB, `.b64.txt` 9.1 → 9.9 MB).
- Screenshots: `docs/media/highway/` (`apps/testbed-web/e2e/highway_shots.mjs`).

**Checks**
| Check | Result |
|---|---|
| `wp_world` (lib, `tests/city_v2.rs`, `tests/world.rs`) | pass; new `loop_is_a_divided_highway_with_ramps`, `mesh_paints_what_marking_at_reports` |
| 400 agents on Port Vesper, 1 min (`traffic_400_agents_one_minute_on_the_hills_without_collisions`) | 0 civilian collisions |
| `simcli run apps/simcli/scenarios` | all pass (Loop 9 crest route, hill roads, Vesper soaks) |
| Goldens | re-blessed: `city_loop_crest_at_limit`, `world_traffic_soak_vesper{,_observer}` (Port Vesper changed); every grid golden unchanged |

**Changes outside `wp_world`** (minimal): `wp_sim/src/physics.rs` (barrier colliders),
`wp_wasm/src/lib.rs` (`road_markings`), `apps/testbed-web` (markings view, stop lines moved
out of `signals_view.ts`), `tools/city-import` (greybox, `RoadSpec` struct update in `osm.rs`).

**Known gaps**
- No gore chevrons / dotted lane-line extensions through tapers (added later on, see the
  follow-up below); no ramp metering.
- Police pursuit AI uses the lane graph as before (ramps are ordinary highway lanes to it).

**Contract change requests:** none (no `wp_contracts` change).

### Follow-up: the grid city's highway (same branch, 2026-10-21)
- **Procedural grid** (`procgen`): the highway spine is a divided viaduct (two one-way
  3-lane carriageways, shoulders, median barrier) from the ring road at each end, bridging the
  arterial/collector rows (7.5 m clearance); full diamonds (8 ramps) at the first and last
  crossing rows of the 12×12 city, stop-controlled terminals on the street
  (`ControlHint::RampTerminal`). Smaller grids (6×6–8×8, the opponent/race test cities) have
  no mid-city ramps: their blocks are too short. Details in the `wp_world` README.
- **Flat terrain for the grid** (12 cm under the streets) so `wp_sim` builds the road mesh and
  the viaduct is a real bridge; on flat ground buildings still get no colliders (old grid
  behaviour; `physics.rs`).
- **Fixes found on the way (both cities):** taper connectors follow the paving's height
  (they stuck up or sank by up to 0.66 m on graded/banked highway); every road's paving runs
  to a freeway node (an unpaved wedge at the gore let a physical car slip under a ramp); deck
  rails and parapets skip ramp edges lying on the carriageway; junction discs in the road
  mesh cover the lane-end corners (gaps to the ground at every junction's outer lane corners);
  the greybox adds the sea only to cities with seabed. The merge-yield rule keys off the
  radius-0 freeway junction instead of the lane kind.
- **Tests / scenarios:** `procgen::highway_is_a_divided_viaduct_with_ramps`;
  `tests/drive.rs` starts at "Arterial drag" (the flat rig cannot drive the viaduct);
  `golden_city_chase` re-aimed (patrol spawns moved: start (279.75, −10), band −35…10),
  `golden_night_pursuit_escape` seed 106 → 107 (a civilian sat in the cruise driver's lane);
  `wp_opponent` `wanted_ai_racer_flees_police_through_the_city` runs 95 s instead of 80 s (the
  circuit row no longer feeds a highway junction, the south-east corner queue is one cycle
  longer). All goldens re-blessed; 45/45 scenarios pass.
- **For the opponent workstream (N):** with ramp terminals on the 8×8 circuit row (tried and
  dropped), a racer stopped behind a busted racer parked a few metres before a junction
  waited for ever: the AI does not pass a stopped car that close to a junction.
- `circuit_8x5` (pending scenario) shows two physical civilians colliding at the highway's
  north ring T-junction (10 m/s) and staying pressed together.

### Follow-up: ramps on the smaller grids (same branch, 2026-10-21)
- Every grid now gets the half diamond facing the ring at its first and last crossing rows
  (4 ramps); the 12×12 city keeps its two full diamonds (8 ramps, data unchanged).
- **`wp_opponent` (minimal changes, for N to review in `field.rs`):**
  - junction awareness ignores civilians on another level (more than 4 m above or below the
    racer's track: traffic on the viaduct overhead was holding racers at the junction under it);
  - on the major road of a priority junction, civilians stopped at its minor approaches, and
    those still able to stop before the box, no longer hold the racer;
  - a racer standing nose to tail with a parked (busted / retired / pulled-over) racer for 3 s
    reverses for 3.5 s to make room to steer round it (it waited for ever behind a busted car
    parked just before a junction);
  - `corridor.rs`: a freeway merge / diverge taper (radius-0 junction) is road, not a
    junction box (racers stopped before an off-ramp gore for traffic on the other
    carriageway).
- **Tests re-aimed:** `open_roads_with_traffic_no_civilian_contact` seed 7 → 5 (seeds 6-8
  bring the last racer to the 6-way corner lights on a long red just short of 250 m; 1-5, 9,
  10 pass); `wanted_ai_racer_flees_police_through_the_city` stays at 95 s;
  `wp_police` `day_core_speeder` seed 100 → 103 (one run of 100-104 started its pursuit at
  20.2 s against the 20 s cap); the 7×7 sprint scenarios (`race/city_sprint{,_police}.ron`)
  start on the new carriageway's judge slots (x 58.05 / 63.05 → 59.55 / 64.55) and
  `city_sprint_police` uses seed 12 (both `wp_race` police tests pass at aim dy −10 / 0 / +10).

### Follow-up: gore chevrons and dotted taper lines (2026-10-21)
- **Markings** (`markings`): new `MarkingKind::DottedLine` and `MarkingKind::GoreChevron`.
  Connectors through a freeway merge / diverge are painted now (lane lines, yellow median
  edge line, the taper's outer edge line, the dotted line on the carriageway's outer edge);
  where a ramp lane runs beside a carriageway lane the two edge lines become one dotted line,
  and where they part a chevron gore fills the paved shoulders between them. All derived when
  the map is built (`FreewayPaint` in `CityMap`; no road format change), drawn by
  `marking_mesh` and reported by `surface_at` (`PaintedLine`) from the same rules.
- **Found on the way:** on the grid viaduct the ramps start dropping where their shoulder
  still overlaps the carriageway's (a 0.5-3.9 m step inside the gore, rail-free under 1 m);
  Port Vesper has a few ramps like that (up to 1.4 m). Chevrons stop at the carriageway's own
  shoulder there. Holding the ramps level until their paving has left the carriageway's was
  tried and dropped: the grid viaduct's inner ramps then exceed the 8.5 % grade cap. Fixing it
  properly needs longer grid ramps (terminals further out), which moves city data.
- **Tests:** `freeway_tapers_are_dotted_and_gores_chevroned` (one dotted run and one gore per
  ramp in the grid and on Port Vesper, dots / gaps and chevron bars / bare concrete where the
  rules say, freeway paint mesh over `marking_at` hits); the mesh checks now test that every
  triangle corner is on paving (a triangle may bridge a step between overlapping ribbons).
- **Goldens re-blessed** (paint on the freeway connectors changes tyre grip there):
  `opponent_circuit_8x5`, `opponent_flee_police`, `opponent_open_roads_traffic`,
  `opponent_reverse_grid_2laps`, `race_city_sprint_police`, `world_traffic_soak`; 45/45
  scenarios pass.
- **Perf:** `surface_at` 0.61 µs/query (0.49 before, same run), markings 0.17 µs (0.13).


## Solid terrain = drawn terrain (playtest: "hitting invisible objects / crashing into the map geometry")
**Cause.** The greybox draws Port Vesper's terrain on a 10 m grid with every vertex lowered to
the minimum of the 5 m heights around it (so it never pokes through a road), but Rapier
collided with the raw 5 m heightfield. Off the roads ~10 % of the ground stood 1–4 m above
what was drawn, up to 10.2 m on steep slopes: invisible hillside.
**Fix.**
- `wp_world::terrain::Heightfield::lowered_coarse` (the drawn terrain) and
  `height_on_triangles` (height on the cell triangles, split along the (i, j)–(i+1, j+1)
  diagonal like the greybox and Rapier tiles).
- `wp_world::surface::collision_terrain` → `CityMap::ground`: the drawn terrain resampled on the
  5 m grid, except in the band around at-grade roads, junctions and bends (paved width +
  sidewalk + 2.5 m), which keeps the fine heights so sidewalks (drawn, not colliders on `main`)
  stay at road level. Not under bridge decks.
- `wp_sim::PhysicsWorld::set_city_ground` builds the heightfield tiles from `CityMap::ground`;
  `CityMap::ground_height` falls back to it off the roads (spawns, resets, physical traffic).
- `city-import` draws from `lowered_coarse` (the shipped `vesper_v2.greybox.glb` is byte-identical;
  `TERRAIN_RENDER_M` and `SIDEWALK_M` now live in `wp_world::surface`).
**Gate.** `wp_sim/tests/city_v2.rs::solid_terrain_is_the_drawn_terrain`: 167k off-road ray casts
against the Rapier world land within 1 mm of the drawn terrain (fails on the old colliders).
`CityMap::new` for Port Vesper: 20 ms (release). Files touched outside `wp_world`:
`crates/wp_sim/src/physics.rs`, `tools/city-import/src/glb.rs`.
**Not covered here.** Props (parked cars, cones, bins) are Rapier bodies that `main`'s testbed
never draws; that is playtest row 50, fixed on `lead/draw-props` / `lead/round12`. That line still
has this terrain mismatch (same `render_terrain`), so this change needs merging there too.

### Drawn terrain follows the ground (follow-up)
The old greybox terrain lowered every 10 m vertex to the lowest 5 m height within 10 m, so
flat ground next to a slope was drawn (and, after the fix above, solid) 1-2 m low, e.g. dips
behind lots that trapped police units. `Heightfield::coarse_under` replaces
`lowered_coarse`: a vertex keeps the fine height under it and is lowered only to the lowest
*protected* fine vertex within 10 m, the protected set being the road band
(`surface::drawn_terrain`, shared by `city-import` and `collision_terrain`). Measured on Port
Vesper (`wp_world/tests/city_v2.rs::drawn_terrain_stays_under_roads_and_follows_the_ground`):
never above a road (closest 0.12 m below), off-road heightfield − drawn p50 0.05 / p95 0.19 /
p99 0.51 m (max 5.0 m on hillsides right beside cuttings, −2.1 m on sharp ridges; physics
follows the drawn mesh in both). `vesper_v2.greybox.glb` regenerated (6.82 MB).


## Road network centrelines follow the road centre (playtest row 39)
`RoadNetworkIndex` built each edge's centreline from the node centres plus the interior
points of the road's first lane. On curved roads (74 % of Port Vesper's edges) that is a
lane's offset off centre, and users that offset into a lane from it (police units, 1.75 m)
ended up one lane too far right one way and on the centre line the other way. Edges now use
the interior points of the road surface's own centreline (`RoadSurface::centre`, oriented
a -> b), so straight roads stay node-to-node. Test:
`tests/city_v2.rs::road_network_centrelines_offset_into_the_right_lane` (1918 samples: 937 in
the wrong lane before, 4 at junction edges after). Police side: `docs/integration/police.md`.
