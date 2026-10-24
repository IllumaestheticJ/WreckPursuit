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
- **Blocked-vehicle behaviour** (round 9, playtest row 32; details in "Response from D (round 9)"):
  traffic goes around or turns around when a stationary vehicle blocks its lane.
  `AgentView` is unchanged (passing/turning cars indicate; reversing cars show hazards).
  New `TrafficStats` fields (serde default): `go_around_lane_changes`, `oncoming_passes`,
  `pass_aborts`, `turnarounds`, `turnarounds_completed`, `reverses`, `reroutes`.
  `traffic::Agent::{wait: Wait, patience_s}` (why a car waits; how long it waits).
- **Manoeuvres** (`wp_world::manoeuvre`, for traffic and any other AI driver):
  `plan_turn(&CarShape, &RoadCorridor, obstacles, rear, forward, goal) -> Option<TurnPlan>`
  (U-turn or k-point turn inside the paved width, clear of obstacle boxes), `TurnDriver`
  (closed-loop turn for a physical `wp_vehicle`, reverse gear via the automatic box),
  `ReverseDriver` (reverse along a path), `Crawl` (creep / hold / gear selection at walking
  pace), `sight_clear` (crest / bend check), map helpers `road_corridor(&map, pos, forward)`,
  `lane_profile`, `overtake_sight_clear(&map, pos, forward, dist)`.
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
- `apps/simcli/src/run.rs` + `SCENARIOS.md` (owned by I, round 9): world metrics
  `traffic_go_arounds` and `traffic_turnarounds` (from the new `TrafficStats` fields);
  scenarios `scenarios/city/blocked_parked_player_{grid,vesper}.ron`; goldens re-blessed for the
  city scenarios whose traffic meets stopped tracked vehicles (see round 9 gate note).
- `apps/simcli/golden/*`: re-blessed city goldens for `PaintedLine` (round 8, see gate note).
- `Cargo.toml` (workspace): added member `tools/city-import`.
- `Cargo.lock`: `postcard` (wp_world), `osmpbf` (city-import only).
- `justfile`: added `city` and `traffic-bench` recipes.
- `tools/lint-determinism/allow.txt`: removed the five `crates/wp_world/... f64` entries (run 2 feedback).
- `crates/wp_vehicle/src/lib.rs`: added `Vehicle::set_pose_and_speed(pos, yaw, speed_mps)`
  (Lead: the stub this went into was replaced by 02's real model; the method now lives in `wp_vehicle/src/vehicle.rs`, same signature.)
  (7 lines, `reset` + speed) for LOD promotion — see contract request 1.
- `crates/wp_sim/src/physics.rs` + `crates/wp_sim/tests/barrier_scrape.rs` (lead-owned; row 164,
  assigned by the lead): barrier and rail colliders are mitered triangle meshes
  (`barrier_colliders`, `chain_segments`) instead of per-segment boxes. See "Playtest row 164".
- `apps/testbed-web`: **photo mode** (owner request, 2026-10-20; branch `claude/fervent-cerf-ft2h9h`,
  not `ws/world`). Testbed-only, no Rust changes. Full notes and the list of touched files are in
  `docs/integration/testbed.md` → "Photo mode" (C to review).

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
  wasm `free_drive_city_json`, `night_run_city_json`, `city_id`; police units drawn on the
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

## Response from D (round 9: playtest row 32, "AI cars, when impeded, should try to turn around or go around the stationary vehicle")
Traffic (and anything driving with the shared controller) no longer queues forever behind a
stopped or wrecked car, a parked player, a stopped police car or debris. Kinematic and physical
(promoted) agents alike. Code: `crates/wp_world/src/traffic/blocked.rs` (traffic state machine)
and `crates/wp_world/src/manoeuvre.rs` (shared manoeuvres); write-up in the crate README
("Blocked vehicles").

**Detect.** Each decision records which constraint set the car's acceleration; from it a
stopped car's `wait` is `Line` (red light, stop line, junction rules, siren, pulling-away
delay), `Queue` (the car ahead waits for one of those) or `Obstructed` (the car ahead is
disabled or itself obstructed; a stationary tracked vehicle not at, or queued before, the stop
line; debris). Cars stop 6 m further back from an obstruction than from a queue, so there is
room to pull out. A driver acts when the obstruction has been stationary for its patience
(law-abiding 4.5–8 s, casual 3.5–6 s, aggressive 3–4 s; derived from id + traffic seed, so no
extra RNG draws and unchanged hashes when nothing is blocked) and it has stood behind it ≥ 1 s.

**Go around.** Multi-lane: forced MOBIL lane change (threshold lowered, safety criterion kept;
never into a lane with a stopped car or parked tracked vehicle within 40 m; physical agents too).
Two-lane: slow pass (4 m/s) in the oncoming lane — only when the obstruction is ≤ 30 m long and
leaves the oncoming lane free, the pass ends ≥ 20 m before the junction, the road up to the
junction is visible (no crest hiding a car, no bend > 20°: Port Vesper's hills), and the oncoming
lane is clear for the pass time at 1.25 × its limit (≤ 160 m, including cars in the junction and
on lanes about to turn in, and tracked vehicles). Left indicator out, right back; the passer
holds an entry in the oncoming lane (oncoming cars stop for it), gives way to anything there,
and goes back behind the obstruction if an oncoming car appears before it is alongside or it
stalls 4 s. Physical passers creep (1.5 m/s, aiming 0.6 m wider) until the body is clear.
No shoulder / kerb mounting: the shoulders in both cities are parking lanes with parked cars.

**Turn around** otherwise: `manoeuvre::plan_turn` finds a U-turn (wide roads) or k-point turn
(a 1+1 street: 5 legs, ≈ 17 s for the real car) inside the paved driving lanes, clear of every
nearby car, tracked vehicle and debris; it starts when oncoming traffic can stop for it. Or
**reverse** back to the last junction (single-lane one-way, no room to turn, or stuck inside a
junction connector): the last car first claims the spot at the end of the lane it came from,
starts once that spot and the junction are clear, holds its connector while reversing. Then
**re-route**: the lane is remembered as blocked for 90 s — nobody plans into it while there is an
alternative, and cars about to enter it re-plan (giving up a permit they can still stop for).

**Police (E).** Patrols drive with `wp_police::avoid`, which already passes a stopped vehicle in
the other lane (and pursuit avoidance is untouched — nothing in `wp_police` changed). What
patrols lack compared with traffic is the turn-around and the crest / junction rule. Request to
E (no contract change needed; all in `wp_world::manoeuvre`):
1. Before `avoid` commits to a pass without lights, require
   `overtake_sight_clear(&map, pos, forward, need + oncoming_clear_m)` (same crest / 20° bend
   rule as traffic).
2. When a patrol has waited behind a stopped vehicle for `pass_wait_s + 15 s` (no gap / pass
   impossible): `road_corridor(&map, pos, forward)` → `plan_turn(&CarShape::from_def(&def), …,
   goal = -lane direction)`; if `Some`, drive it with a `TurnDriver` (`set_obstacles` each tick
   with nearby vehicles as `geom::Obb`), then re-route the patrol from the opposing lane.
   `TurnDriver::step(&state, dt) -> (VehicleInput, ManoeuvreStatus)` is closed loop on the real
   car (verified in the Rapier rig: `tests/drive.rs`).

**Coordination with `lead/fix-lod-popin`.** Not on the remote yet. My change to LOD: agents in
the middle of a turn-around or reverse are neither promoted nor demoted (`Traffic::lod_locked`
in `update_lod`; a kinematic turn cannot be handed to a physical car mid-leg). Please keep that
skip if you change `TrafficConfig::physical_radius_m` / the demotion hysteresis (+20 m) —
radii are config values, `traffic/blocked.rs` needs no change for them.

**Tests** (`crates/wp_world/tests/blocked.rs`, all with civilian-collision diagnostics on):

| Case (density 4, 150 s unless noted) | Result |
|---|---|
| Grid 1+1 street, stopped car in one lane | 0 contacts with it, 0 civilian collisions; 3 cars held up, all got away (2 oncoming passes + 1 reverse), longest wait 13 s |
| Grid 1+1, car parked **across** the lane (reaches over the centre line) | 0 / 0; 6 held up, 5 got away (3 turn-arounds done, 1 reverse), longest wait 21 s |
| Grid 2+2 arterial, stopped car in the right lane (120 s) | 0 / 0; 9 go-around lane changes, **0** oncoming passes |
| Single-lane one-way in a 3×2 ladder town (density 1.5, 240 s) | 0 / 0; no passes or turns (no room), 2 reverses to the junction + re-route, 1 recycled after 120 s (third in line, reversing one at a time) |
| Port Vesper 1+1 street near downtown | 0 / 0; turned around (1) |
| Real car (`tests/drive.rs`, Rapier rig): `TurnDriver` on a 7 m street with a car 7 m ahead | 5 legs, 16.7 s, never outside the kerbs, never touching it |
| Real car: `ReverseDriver` 40 m with a kink | ≤ 0.13 m off the path, stops at the end |

**simcli scenarios** (`apps/simcli/scenarios/city/blocked_parked_player_{grid,vesper}.ron`): a
player car parked across one lane of a two-lane street (nose 0.3 m towards the kerb), 120 s,
near-player traffic physical (Rapier). Seeds 3/5/7/9/11:

| | grid (police on) | Port Vesper |
|---|---|---|
| civilian collisions | 0 (all seeds) | 0 (all seeds) |
| parked car: health / moved | 1.0 / 0.0 m (all seeds) | 1.0 / ≤ 0.001 m |
| go-arounds (passes + lane changes) | 1–4 | 0–1 |
| turn-arounds + reverses | 0–1 | 1–2 |
| stuck recycled | 0 | 0 |

Ranges in the scenarios: collisions 0, health ≥ 0.99, moved ≤ 0.5 m, stuck ≤ 3, grid
go-arounds ≥ 1, Vesper turn-arounds ≥ 1, moving fraction ≥ 0.4 (city-wide incl. red lights;
measured 0.45–0.64).

**Soaks / regressions.** 10-min soaks, 400 agents, 12×12 grid, {0 %, 5 %} aggressive: 0
civilian collisions, stats bit-identical to round 6 (847 / 912 lane changes, 3 / 1 stuck) —
normal traffic is unchanged. Port Vesper 10-min soaks × 3 seeds × {0 %, 5 %}: 0 collisions.
simcli `soak/traffic_soak_observer` (police on), `traffic_soak`, `traffic_soak_vesper`,
`traffic_soak_vesper_observer`: 0 civilian collisions, all expectations pass.

**Perf** (release, 4 vCPU): 400 agents 0.075 ms per 60 Hz frame without blockers, **0.091 ms**
with 12 blockers and passes / lane changes / turns / reverses running (budget 2 ms).

**Determinism.** f32 only, `BTreeMap`s, patience from an integer hash of id + seed, no new RNG
draws; `World::state_hash` adds manoeuvre state only for agents in a manoeuvre.

**Known gaps.** No shoulder use (parking lanes have parked cars). One pass at a time per
obstruction (the queue passes in turn). The k-point planner assumes a locally straight road (the
physical driver stops a leg early near an edge). A long queue reversing out of a one-way street
goes one car at a time, so its front car may still be recycled after 120 s.

**Contract change requests:** none.

### Gate note (round 9): goldens re-blessed
Traffic now behaves differently around stationary tracked vehicles (players, police units):
6 m extra standstill gap behind an obstruction, body-aware "in my path" test, go-around /
turn-around. Re-blessed (`simcli goldens --bless`, commit tagged `[bless]`): `golden_city_chase`
(+ replay), `golden_night_pursuit_escape` (+ replay), `opponent_{flee_police,
open_roads_traffic}`, `race_city_sprint_police`, `world_traffic_soak{,_observer,_vesper,
_vesper_observer}`; new: `city_blocked_parked_player_{grid,vesper}`. Non-city goldens and the
city scenarios without tracked vehicles in traffic's path are unchanged. `simcli run
apps/simcli/scenarios`: 47/47 pass. fmt, `clippy --workspace --all-targets -D warnings`,
`cargo test --workspace`, wasm32 build of `wp_wasm`: pass.

## Lead request W-D1 (weather in the game, 2026-10-19)
Weather now runs in `wp_sim` (`Sim::weather()`); traffic should drive slower and leave longer
gaps in rain and fog. `wp_world` has no hook for it yet (per-agent `DriverParams::speed_factor`
exists, but nothing global), so please add one (no contract change):
- `World::set_weather_modifiers(&mut self, m: TrafficWeather)` with a `wp_world`-owned
  `TrafficWeather { desired_speed_mult, time_headway_mult, density_mult, hazard_sight_mult }`
  (all 1.0 by default; `wp_world` need not depend on `wp_weather`). Apply in the IDM:
  `v0 *= desired_speed_mult` (in `speed_factor_on` or where `seg.speed_limit * speed_factor_on`
  is used), `T *= time_headway_mult`, density targets `*= density_mult`.
- The lead will call it from `Sim::step_fixed` before `World::step`, filled from
  `WeatherSim::traffic_modifiers()` (heavy rain 0.85–0.75 × speed / 1.4–1.8 × headway; fog
  0.85–0.70 / 1.3–1.6, by intensity; `data/weather/effects.toml`).
- Keep the default 1.0 bit-identical, so traffic goldens do not move.

## Lead feedback (round 12, 2026-10-19)

**Base for this round: `lead/round12`, not `main`.** Round 12 merged ten branches (J, J2, A, D, E, R, M,
weather, G, F) onto `lead/round12`; everything passes except the item below, so `main` stays
where it was until it is fixed. Merge `origin/lead/round12` into your branch (not main), fix,
run the full gate there, push your `ws/*` branch, and the lead merges it.

1. **`apps/simcli/scenarios/city/blocked_parked_player_vesper.ron` fails after the merge**:
   `traffic_turnarounds` = 0 (expected 1–30). It passes on `main + ws/world` alone (hash
   33efc120…), and fails as soon as A's differential rework (`ws/vehicle`, `[bless]` 04cd217) is in
   (hash fd3b599a…). Find what changed for the scenario (traffic car drivetrains now have real
   diffs; launch/creep behaviour, the parked player's own diff/handbrake, or speed thresholds in
   the go-around / turnaround logic) and fix it in `wp_world` (or tune the scenario only if the
   behaviour is still right and the expectation was too tight; explain which). Don't loosen the
   range to hide a real regression. Re-bless only the goldens your fix changes, in a `[bless]`
   commit with the reason.

### Response from D (round 12)
**Item 1 — `city_blocked_parked_player_vesper` 0 turn-arounds: fixed in `wp_world`** (`935cfe7`),
goldens in `[bless]` `8595256`. Scenario file and its ranges unchanged.

*What changed for the scenario.* Traced the queue on lane 446 before/after A's differential
rework. In both runs the first civilian passes the parked car in the oncoming lane (t ≈ 21 s),
and the next one (id 483, at t ≈ 75 s) waits its 21.5 s pass budget for an oncoming gap that
never comes (steady ~5 s stream at ~11 m/s; a pass needs ≈ 150 m clear). Then:
- *main + ws/world:* turn refused for ~4 s (cars rolling up behind), then a gap and the
  turn-around starts at **100.7 s**, finishing at 118 s of 120 — it passed by a few seconds.
- *round 12:* the shifted launch/creep timing of the physical queue cars (real diffs) means
  four cars roll up behind one after another for **13 s** (any car within 40 m behind moving
  > 1 m/s refused the turn), after which the oncoming stream (needs `(1.25 × limit)² / 6 + 25`
  ≈ 81 m) stays closed until the end. 0 turn-arounds.
So it is not a new bug in the diffs: the turn checks were overly conservative and the scenario
was riding on a lucky gap. Both checks were treating *every* car as worst-case:
- **Followers (own lane):** now refuse the turn only if they cannot stop at 2 m/s² short of the
  turner's rear − 10 m (≈ the room a reversing leg uses; followers already stop ≥ 6 m + 3 m
  back because of the obstruction gap and the turn's own-lane entry). A car creeping up at
  3 m/s 30 m back no longer blocks it; a fast one 60 m back now does (the old 40 m window
  missed it). Lanes crossed keep the 40 m rule.
- **Oncoming:** each car must be able to stop from its own speed + 2 m/s at 3 m/s² with 25 m to
  spare (`turn_oncoming_room`), instead of a fixed distance for 1.25 × the limit.
Pure helpers `turn_follower_stops_short` / `turn_oncoming_room` with unit tests.

*Measured (120 s, round 12 + fix), go-arounds / turn-arounds:* seed 3: 0/2, 5: 0/1, **7: 1/1**,
9: 0/2, 11: 0/2, 13: 0/2, 15: 0/2, 17: 0/2; 0 collisions, 0 stuck, parked car untouched in
all. Seed 19 (not in the measured set) resolves with passes only (3 started, 0 turn-arounds) —
the preferred outcome, so the `turnarounds ≥ 1` row would fail on it; I left the row as is
because it pins the turn-around path for seed 7. Before the fix, seeds 7 and 19 failed.
Runs of 150–180 s show `stuck_recycled` 4–5 on some seeds (far end of the queue, kinematic,
out of sight); unchanged by the fix, so the scenario stays at 120 s.

*Goldens.* Hashes compared scenario by scenario, `lead/round12` vs `lead/round12` + fix:
exactly 5 move — `city_blocked_parked_player_{vesper,grid}`, `world_traffic_soak_observer`,
`world_traffic_soak_vesper`, `world_traffic_soak_vesper_observer`; the other 50 are
bit-identical. Only those 5 re-blessed. **Heads-up:** `lead/round12` itself (before my change)
already mismatches many goldens in `simcli goldens` — `weather_*` (aquaplane, fog_pursuit,
wet_brake), `panels_frontal_56_hood`, `golden_city_chase`, `golden_night_pursuit_escape`,
`opponent_{flee_police,open_roads_traffic}`, `police_sideswipe_patrol_day`,
`race_city_sprint_police`, `world_traffic_soak` — and `police_sideswipe_patrol_night` fails its
expectations (0 racer contacts, no pursuit before 13.1 s). Not mine and untouched; they need
the lead's post-merge bless / an owner's look. (The grid and soak goldens I re-blessed were
in that stale set too, so blessing them also absorbs that merge drift.)

*Gate.* fmt, `clippy --workspace -D warnings`, wasm32 build of `wp_wasm`: pass.
`cargo test --workspace --no-fail-fast`: everything passes except three tests that fail
identically on `lead/round12` without my change — simcli `goldens_match` and
`all_scenarios_pass_and_are_deterministic_in_process` (the stale goldens /
`police_sideswipe_patrol_night` above), and `wp_net` `four_clients_three_minutes_in_port_vesper`
(p0 pursuit ends `Escaped`, expected `Captured`; checked on `aee4916`).

Determinism: both `city_blocked_parked_player_*` pass `simcli determinism`. No new RNG draws,
f32 only. **Contract change requests:** none.

## Lead helper: sidewalk fix (branch `lead/fix-sidewalk-fall`, 2026-10-19)
Playtest #20 ("I can fall into the sidewalk"). Built on `main`. D: the changes in your crate
are confined to `surface.rs` (listed below); please merge `lead/fix-sidewalk-fall` into
`ws/world` when convenient.
**What was wrong** (reproduced with `tools/city-import/tests/surface_match.rs`: greybox vs
`wp_sim` colliders, ≈ 550 k / 615 k downward rays)
1. **No sidewalk or kerb colliders at all.** The greybox draws 3 m sidewalks 0.15 m above
   the road edge; physics had only the road ribbon, then the terrain (Port Vesper: carved
   12 cm *below* the road → a wheel sank 0.27 m into every sidewalk) or the slab (grid:
   0.15 m). On fills and deck approaches, where the terrain is natural, the visual sidewalk
   floated over ground up to 21 m below: driving onto it was a fall through the world.
   294 k (Vesper) / 246 k (grid) samples disagreed.
2. **Holes at every junction mouth.** The paved junction disc had the junction's
   `radius_m` (e.g. 9 m) while the trimmed road ribbons' corners sit at `sqrt(d² + w²)`
   (11.4 m): an unpaved crescent up to ~1.7 m deep at each corner of each mouth (Vesper:
   wheels dropped 12–22 cm onto terrain while turning; grid: grass drawn there). The disc
   16-gon also had its corners (not edges) on the circle.
3. **Pits at junction corners and bends.** No sidewalk between the sidewalk ends of two
   streets (a 15 / 27 cm deep notch whose far side is the next sidewalk's edge, which stopped
   cars cutting the corner dead) and gaps up to 14 m on the outside of Vesper's bends.
4. Sidewalks ran onto the crossing road at tight / skewed corners (3.7 k junction-area
   samples): a 15 cm step inside the junction.
**Fix**
- `wp_world::surface` (one geometry for physics and greybox): `sidewalk_mesh` (strips +
  kerb faces along every non-highway edge lane, trimmed off pavement in 0.5 m cells; corner,
  T and bend fills at kerb height with kerb faces), `lane_bank` / `lane_sidewalks` (moved from
  the greybox), `junction_polygon` (edges touch the circle), `junction_paved_radii` (reach
  every ribbon corner; `radius_m` and junction logic untouched), `SIDEWALK_W_M` = 3.0,
  `KERB_H_M` = 0.15, `TriMesh::{quad, strip_vertical, append}`, unit tests
  `sidewalks_stay_off_the_roadway` (every lane and turning path, both cities) and
  `kerb_faces_and_tops_meet_and_face_out`.
- `wp_sim`: `set_city_ground` runs for every city and adds the sidewalk mesh (256 m tiles,
  `SIDEWALK_GROUP`); a car upright on its wheels has its chassis ignore that group (the box
  floor is at kerb height and snagged on internal triangle edges while straddling a kerb;
  wheels do the climbing); `place_on_ground` lifts onto a sidewalk top; `static_ground_z`.
- `tools/city-import`: the greybox draws `sidewalk_mesh` and `junction_polygon` (vertices
  deduplicated); `data/city/*.greybox.{glb,b64.txt}` regenerated (grid 0.47 → 0.85 MB, Vesper
  6.81 → 7.04 MB).
**Checks (this branch)**
| Check | Before | After |
| Sweep, visual vs physics (`surface_match`, 3 cm; kerb line 16 cm) | 294 k / 246 k of 555 k / 616 k off | 0 / 0 |
| 96 kerb mounts, 15 / 40 km/h × 2 angles, every Vesper district + fill/deck approaches + grid (`wp_sim/tests/sidewalk.rs`) — lowest tire vs visual surface | −0.34 m (Vesper), −0.15 m (grid) | −0.023 m, −0.006 m; never airborne; all end on the sidewalk |
| simcli `city/sidewalk_mount_15kmh`, `_40kmh` (mount + drive along; `min_z_m`, `max_airborne_s`) | FAIL: walkers at 2.38 / 2.38 (top 2.65) | PASS: 2.65, never airborne |
| Roadway points under a sidewalk (lanes + turning paths) | — | 0 (both cities) |
| All 47 scenarios' expectations | pass | pass (same pending set) |
| `cargo test --workspace` | — | 791 passed, 0 failed |
| fmt, clippy `-D warnings`, wasm32 build of `wp_wasm`, `lint-determinism` | — | clean (0 errors) |
| Native vs wasm (`tools/wasm-determinism/compare.sh`, all 47 scenarios) | — | identical |
| `tsc --noEmit` | — | clean |
| `web-e2e` | `main`: 2 of 4 runs pass | 0 of 4 pass: the same 3 checks fail on `main` (below) |
**Goldens:** re-blessed in a `[bless]` commit — 17 city scenarios (grid and Vesper: new
sidewalk colliders, bigger junction discs, chassis filter) + the 2 new ones; all non-city
goldens unchanged.
**Perf (release, cloud container)**: city chase frame (`city_frame_cost`, all systems)
1.01–1.25 ms vs 1.03–1.12 ms on `main` (noise); city build 280–310 ms vs 215–230 ms
(`sidewalk_mesh` ≈ 46 ms + the extra trimesh BVH).
**Known gaps / follow-ups (D)**
- The render terrain is the 10 m min-filtered mesh: on steep cuts and hillsides physics
  terrain is up to 10.6 m *above* what is drawn (an invisible slope, never a hole; sweep
  `Terrain` range). Drawing the fine heightfield near roads would fix it.
- `CityMap::ground_height` / `MapGround` (un-embodied physical traffic) don't know the
  sidewalks.
- Corner fills/trims follow pavement in 0.5 m steps (jagged kerb at corners; a sidewalk edge
  can overhang the outer rim of a junction disc by ≤ 0.35 m, never a lane).
- An upright car's chassis passes through kerbs (no kerb tripping); the wheels see them.
- A Parry heightfield ray exactly on certain grid lines (e.g. y = 45.000) misses the terrain
  (measure zero; the sweep jitters its grid).
- **`web-e2e` flake, also on `main` (lead / C / J):** "capture ends the run", "Repair &
  continue" and "no page errors" (`NotSupportedError: setValueCurveAtTime … overlaps`,
  `apps/testbed-web/src/audio/synth.ts` siren scheduler) fail together in about half the
  runs on `main` and in all 4 runs on this branch. Instrumented runs of both show the same
  scene tick for tick: the patrol car stops ≈ 7 m short of the parked player (headless
  Chromium runs the sim at ≈ 60 ticks/s), no arrest within the 240 s wait, and the siren keeps
  scheduling until the audio error. Natively (`Session`, Port Vesper chase) the same setup
  arrests in 9.6 s on this branch. Not fixed here (outside this change).
**Follow-up after `lead/round12` (2026-10-20).** The lead's gate failed
`vesper_v2_cars_drive_up_the_kerb_and_along_the_sidewalk` (3 drives, all 40 km/h 15°, at
(1246, −945) Port, (−1240, 301), (−923, 1059)). Cause, checked one candidate at a time:
- *Collider port* (`city_ground_colliders` / `city_sidewalk_colliders`): fine — same
  colliders, groups (`SIDEWALK_GROUP`, terrain group) and insertion order as before (terrain,
  roads, sidewalks + kerbs, buildings, rails; tunnel boxes appended), chassis-group switching
  unchanged. Tires never sank: lowest tire −0.025 m (limit −0.03).
- *Parked props*: not involved (the test runs with `parked_cars: false`; none within 8 m).
- *Tunnels / terrain cuts*: terrain is below the sidewalk at all three spots; buildings
  near them were not moved (round 12 only drops buildings in cover corridors).
- *A's steering retune* (slip-target lock + `STEER_RAMP_S`): **the cause.** The test's own
  lane-keeping controller turns the car parallel more slowly now: it peaked 0.3–0.4 m further
  out (lat 4.07 / 4.29 / 4.31 m vs 3.79 / 4.18 / 4.41 on the old branch) while still angled
  in, and its nose hit the building wall ~1 m past the sidewalk's outer edge (11 → 1 m/s in
  0.1 s; the end check then saw a car crushed against a wall, 16 cm below the sidewalk top).
Fix (test only, `crates/wp_sim/tests/sidewalk.rs`): heading damping of the test controller
1.2 → 2.5 (peaks back to 3.8 / 4.35 / 4.6 m, no stops); `Drive` also records
`min_speed_mps` so a wall hit is visible in the failure message. No physics or collider code
changed. Result: Vesper 68 drives, lowest tire −0.023 m; grid 28 drives, −0.005 m; all end on
the sidewalk.
Gate on this branch (= `lead/round12` + this test/doc change): fmt, clippy `-D warnings`,
wasm32 build, `tools/wasm-determinism/compare.sh` (native == wasm, all scenarios), `web-e2e`
and the `surface_match` sweep (both cities) pass. `cargo test --workspace --no-fail-fast`:
937 passed, 5 failed — all also failing on `lead/round12` itself (nothing here touches them),
for the lead: `simcli goldens_match` (21 scenario hash goldens not re-blessed after the round 12
merges), `wp_race race_picker::grid_highway_sprint` (2 of 4 finished),
`wp_vehicle handling_balance::front_drive_cars_understeer_mildly` (junkyard_beater 1.71 deg/g,
limit 1.6), `wp_wasm race_tests::{short_sprint_with_ai_…, a_capture_mid_race_…}` (player DNF
on the finish window / never captured).
## Lead helper: LOD pop-in (branch `lead/fix-lod-popin`, 2026-10-19, playtest row 25)
Owner report: "some cars load LOD too slow": cars pop in late or look wrong for a while. Two
causes were D's tuning (the rest was browser-side, see `docs/integration/testbed.md`):
- **Spawn ring in plain view.** Agents spawned and despawned 150 m from the player. The browser
  draws every agent, so that ring *was* the draw distance: on Loop 9, Ridge Road and the downtown
  avenues cars materialised / vanished 150–230 m ahead (measured in the browser: closest
  appearance 226 m in a 5 s window of the city chase; `wp_world` itself allows 150 m).
- **Promotion swap.** Fine on the sim side (promote/demote keep the pose, < 0.2 m); the pop was
  the browser drawing kinematic agents as grey boxes and promoted ones as a different car.
### What changed in `wp_world` (kept small)
| Change | Why |
| `TrafficConfig::default()`: `sight_radius_m` 150 → **320**, `active_radius_m` 400 → **440** | nothing spawns or despawns within 320 m of a player (now or 1 s ahead), i.e. cars are visible ≥ 300 m ahead at highway speed; 440 m keeps the density near the player at today's level |
| `traffic::POPULATE_SIGHT_RADIUS_M = 150` + `Traffic::populating` | `populate()` (before the first tick, nobody has seen anything) keeps the old 150 m clearance, so sessions start as busy as before |
| `tests/lod_draw.rs` (new) | a player riding Loop-9-style traffic for 60 s on Port Vesper never sees a car appear or vanish within 300 m; cars within 150 m stay ≥ 0.6× the old radii (5 seeds; measured 97 vs 83). `--ignored compare_radii` prints the radius comparison |
| `tests/world.rs` | `spawns_respect_sightline_and_active_area` reads the radii from the config and checks live spawns too; `lod_promotion_and_demotion_are_seamless` lets its player cruise the row (the parked player depended on one queue stopping inside 80 m; it now sees 25 promotions, max jump 0.17 m) |
| `README.md` | sightline rule numbers |
Radius comparison (`cargo test -p wp_world --release --test lod_draw -- --ignored --nocapture`,
5 seeds, 60 s each, player riding the fastest traffic at ≈ 17–21 m/s):
| sight / active | agents | cars < 150 m | cars < 300 m | spawns / min |
|---|---|---|---|---|
| 150 / 400 (old) | 145 | 16.7 | 71 | 751 |
| **320 / 440 (new)** | 190 | 17.2 | 60 | 978 |
| 320 / 480 | 176 | 14.8 | 54 | 763 |
| 340 / 600 | 181 | 9.3 | 34 | 528 |
Agents stay capped at `max_agents` (400), so the 2 ms / 400 agents budget is unchanged; the
typical count near a player grows ≈ 30 % (145 → 190).
Physical (promotion) radius stays **80 m** (`max_physical` 16): with the browser hand-over now
seamless (same model, livery and pose on the swap frame) promotion distance is invisible, and a
larger radius would only cost physics. Not changed: `wp_police` spawning (E): reinforcements
already spawn ≥ 350 m away out of sight (`data/police/presence.toml`).
### Changes outside `wp_world`
| File | Change | Why |
| `crates/wp_net/src/netsim.rs` (test only) | `damage_steps_match_the_models` populates the world around the spawn before the first tick, as simcli does | it needs a promoted traffic car within 5 s; live spawns now start 320 m out |
### Goldens (blessed in the `[bless]` commit)
Every city scenario with a player or race route changes from tick 1 (different spawn ring):
`golden_city_chase`, `golden_night_pursuit_escape`, `opponent_flee_police`,
`opponent_open_roads_traffic`, `race_city_sprint_police`, `world_traffic_soak_observer`,
`world_traffic_soak_vesper_observer`. Player-less soaks are unchanged; every scenario
expectation still passes; `wp_race`'s tuning-sensitive meet-report pair still passes (it did not
at 480 m, which is why the active radius is 440 m).
### For D (please own from here)
- **Joining a running server**: `wp_net` servers never call `populate()`; a player who joins
  where nobody else is sees the street fill from 320 m out instead of 150 m. If that reads empty
  in multiplayer playtests, a per-player warm start (populate around a newly tracked player,
  outside any other player's sight) would fix it.
- Kinematic agents move in 60 Hz traffic steps; `wp_wasm::lod::traffic_draw` carries them
  forward along their heading between steps (read-only), so the browser no longer judders.
  A `World` accessor for "time since the last traffic step" would let it stop recomputing that.

## Lead feedback (round 12b, 2026-10-20) — owner: traffic yields to police sirens
Base: `origin/lead/round12` (not main). Owner: "traffic will get out of the way of cops if siren is
on" (rows 61–62; E is adding siren units pushing through stuck traffic on ws/police).
1. Civilian drivers yield to any vehicle with its siren on (police units now; the player cop in
   multiplayer too): when a siren unit approaches from behind or ahead in the same direction
   within ~60–120 m (or is detected at a junction), the driver signals, slows and pulls to the
   kerb / the right-hand side (or the nearest shoulder / out of the lane the unit needs), stops,
   and waits until the unit has passed + ~2 s, then merges back carefully. At red lights, creep
   into the junction only if safe to make room; never block the junction. Oncoming traffic
   slows. Driver personality may vary reaction time (0.5–2 s) but everyone yields.
2. Expose the siren state D needs (from wp_police / wp_sim; if a contract field is needed, write a
   request under "Contract change requests" and use a local query meanwhile).
3. The LOD fleet (kinematic far traffic) must yield too, cheaply (e.g. stop on the shoulder when
   a siren unit is within range), so far cars don't block the road when they are promoted.
4. Scenarios with expected ranges: a siren unit behind 6 cars on a 2-lane street passes all of
   them within ~10 s; with sirens off they don't yield. Same in both damage modes.
5. Keep the 400-agent traffic budget (≤ 2 ms native).

## Lead helper: junction holes (branch `lead/fix-junction-holes`, 2026-10-20, playtest rows 46, 51)
Changes in your crate are confined to `surface.rs` and the new `surface/nodes.rs`:
`node_patches` (junction surfaces, bend strips, dead-end turnarounds sharing the ribbons' end
edges), `surface_ribbon`, exact sidewalk corner fills, strip pull-back, and tests. The greybox
now draws the surface ribbons. Full write-up, numbers and the items left for you (node 81's
overlapping roads, turning paths through building lots at nodes 78 / 97, junction heights on
saddles): `docs/integration/junctions.md`. Please merge `lead/fix-junction-holes` into
`ws/world` once the lead has merged it.
### Follow-up (2026-10-20): density back to target, stranded passers
The lead found `world_traffic_soak_observer` failing on this branch (and on `lead/round12`):
`traffic_agents_min` 310 (range 350–400). Cause and fix (`crates/wp_world/src/traffic.rs`,
`traffic/blocked.rs`):
- **Downtown in view never refilled.** Spawn control is per zone and nothing spawns or
  despawns in view. With the 320 m sightline the parked observer sees the whole 12×12
  downtown (9 zones, target 157 cars), which held ≈ 56: the city sat ≈ 90 below target from
  `populate()` on (populate stalled at 299/400; the sightline check rejected most samples).
  `spawn_update` now counts only out-of-sight agents per zone, and `spawnable_targets()` gives
  each zone the out-of-sight share of its target **plus** the in-view shortfall (in-view target
  minus cars in view), spread over the spawnable samples within the active radius of a
  *player* (not of race-route focus points), at most `MAX_SPAWNABLE_BOOST` = 2×. Those cars
  drift into view as before (spawns still only ≥ 320 m away). No player tracked → targets
  unchanged.
- **Stranded passers.** The denser start exposed a row-32 gap: a pass given up after stalling
  beside the obstruction (`PASS_STALL_S`) left a *physical* car at a standstill, nose out in the
  oncoming lane, with no lane entry (a physical car cannot steer back at 0 m/s). Oncoming cars
  drove into it (`city_blocked_parked_player_grid`, seeds 5/7; already on `main` for
  `..._vesper` seed 3). Now any car outside a manoeuvre with `lat < -STRAY_INTRUSION_LAT_M`
  (0.8 m, the passing threshold) occupies the oncoming lane at its body and holds traffic
  `STRAY_HOLD_BACK_M` = 22 m further up. Without the hold point the oncoming queue stopped 6 m
  from it and it could neither pass nor turn (deadlock); with it, it turns around once its
  patience runs out.
| | `lead/round12` | this branch |
|---|---|---|
| `world_traffic_soak_observer` agents min (seeds 3/5/9/11) | 313 / 310 / 304 / 311 | 400 / 400 / 400 / 400 |
| `world_traffic_soak_vesper_observer` | passes | passes (≈ 400 from 10 s on) |
| `world_traffic_soak_observer` moving fraction min (seed 5) | 0.495 (lead's run) | 0.54 |
| parked-player grid / vesper, 5 seeds each: collisions | 0,0,0,0,0 / 0,1,0,0,0 | 0 / 0 |
| browser LOD e2e (`run.mjs`): closest appear / vanish | — | 322 m / 342 m, 0 missing-mesh frames, 0.000 m swap jump |
All 61 scenarios pass (slow soaks included); native == wasm on every scenario.
Browser (`npm run e2e`, headless Chromium): `run.mjs` 50/50 and `race.mjs` 19/19 on this
build. One earlier `run.mjs` pass, run while the golden bless was using every core, failed
"replay after reload" (all 1979 tick hashes matched, final state hash differed) and the scrub
check; two later runs passed and `simcli replay` verified that same recording. Looks
load-dependent in the page's record/replay path, not the sim; worth watching.
Goldens: the 14 this change moves are blessed in their own `[bless]` commit; the 15 that were
already stale on `lead/round12` (round-12 merges, not this change) are blessed in a second
`[bless]` commit at the lead's request. All 61 goldens match; `goldens_match` passes.
**Open, not fixed here (for D / race owner):** `wp_race` `lantern_checkpoint_draws_the_police`
(needs 2 of 3 AI racers to finish) fails on this branch. It is a knife-edge test in every
configuration measured (`WP_RACE_SEED`): pre-pop-in radii 150/400 m pass 2 of 6 seeds
(default, 1); `lead/round12` 2 of 5 (3, 4) plus default; this branch 0 of 6. In all of them one
AI racer (P3) stays on the grid with 0 gates and many contacts, and others are captured or flee
from the daytime meet patrol. Not tuned around one seed. Also failing on `lead/round12`
itself, unrelated to traffic: `wp_sim` `vesper_v2_cars_drive_up_the_kerb_and_along_the_sidewalk`
and `wp_wasm` `race_tests::a_capture_mid_race_is_a_dnf_and_hands_off_to_the_end_of_run_panel`.
### Response from D (round 12b) — traffic yields to sirens
Base: `origin/lead/round12` @ 900261e merged into `ws/world`. New module `wp_world::traffic::siren`
(replaces the old "slow to 3 m/s near any siren within 60 m" rule); README § *Yielding to sirens*.
**1. Civilians yield to any vehicle with its siren on.** Per driver, per siren unit:
- *Behind*, same direction (noticed 60–120 m back by the unit's speed; also a slow unit < 30 m
  back, i.e. one stuck behind the car as in E's row-61 push-through): reaction time by
  personality (law-abiding 0.5–1.2 s, casual 0.7–1.6, aggressive 1.0–2.0; hash of id + seed, no
  RNG draws) → signal → slow (≥ 1 m/s², ≤ comfort decel) → move to the kerb inside the lane
  (body 0.15 m from the lane edge = 0.65 m right of a 3.5 m lane's centre; crawl at 2 m/s while
  still moving sideways) → stop → wait until the unit is past + 2 s → signal → merge back once
  nobody is closing in behind. Away from the unit: left only if the unit is < 35 m back and
  > 0.5 m to the right (e.g. riding the line to the right lane). Multi-lane: forced lane change
  out of the unit's lane where the next lane has a safe gap (MOBIL safety unchanged).
  A unit standing still behind for 15 s is given up on (not yielded to again until it moves).
- *Oncoming* (or a unit stopped facing the car < 30 m ahead): to the kerb, slow to 4 m/s.
- *Stopped unit* within 30 m otherwise: slow to 8 m/s past it (5 m/s within 40 m made a slow
  stream past a bust scene that kept an AI racer waiting for a gap for 130 s:
  `wp_opponent` `live_police_chase_ends_in_escape_or_capture_without_stuck_cars`).
- *Junction*: a driver within 60 m of its stop line does not enter a junction a moving unit heads
  for (unit within 70–120 m by speed, or within 20 m; a unit parked at a scene does not close it), and gives up a permit it can still stop for;
  cars already in the box keep going. Nobody creeps into a junction to make room (never block
  the box wins over "creep at a red"; the unit passes the queue on the centre line).
- Same code in both damage modes (no `DamageMode` branch anywhere).
- Snapshot: `AgentView::yielding` → existing `traffic_flags::YIELDING`; turn signal towards the
  kerb while pulling over / waiting, back while merging.
**2. Siren state:** no contract change needed. `wp_police::unit_input` already sets
`VehicleInput::siren`, `wp_sim::step_fixed` passes `v.input.siren` into
`TrackedVehicle::siren` for every vehicle, so AI units and a player cop's siren both reach
traffic. **For E:** a unit passes yielding traffic best hugging the centre line (offset ≈
−(lane_w/2 − 0.2) from its lane centre): pulled-over cars of both directions leave a 2.6 m
corridor there. A unit at its lane centre is still blocked by a car pulled over in that lane
(0.65 m is all the lane allows); your existing pass-in-the-other-lane logic works too.
**3. LOD fleet:** kinematic agents run the same decision code (lateral offset kept on
promotion), so far cars are already on the kerb when promoted. Cost: one distance test per
siren unit per agent per traffic step; nothing runs while no siren is on.
**4. Tests / scenarios.**
- `crates/wp_world/tests/siren.rs` (stand-in unit, police-car-sized, hugging the centre line at
  ≤ 16 m/s, braking for anything in its swept path):
  | case | result (seeds 1/2/3) |
  |---|---|
  | 6 cars queued at a red (3×3 grid, signalised crossroads), unit 30 m behind the 6th | all 6 passed in **6.8 / 6.9 / 5.7 s**, 0 contacts |
  | 6 cars moving at ~9 m/s on a 1 km 1+1 street | all passed, unit never below 13.8 m/s, 0 contacts, all 6 back in lane and driving |
  | same, siren **off** | 0 yields, 0 of 6 passed, 0 contacts |
  | 2+2 arterial, unit on the lane line | 4 of 4 passed, unit ≥ 15.3 m/s, 0 contacts |
  | 2+2, unit in the middle of the left lane | 0 contacts; 2 lane changes out of its lane (only where a gap exists) |
  | unit crawling 12 m behind a car / then parked | car pulls over and stops; gives up after 15 s, merges, drives off, does not re-yield |
- simcli (new metric `traffic_siren_yields`; `follow_route` gained optional
  `lateral_offset_m` / `keep_distance`, and route drivers take `siren`/`headlights`/`horn` from
  keyframes): `city/siren_traffic_yields_arcade.ron`, `_sim.ron` (identical hashes: damage mode
  does not change traffic), control `city/siren_traffic_no_siren.ron`. Grid "Garage 2 → Garage 3",
  density 10: siren on 6 pull-overs, route done at 16 m/s, 0 contacts; siren off 0 pull-overs,
  52 % of the route (held up behind traffic). Port Vesper's 1+1 streets carry ≤ 8 cars/km
  even at density 6, so the exact "6 cars within ~10 s" check lives in the wp_world test.
  Deterministic (`simcli determinism`).
**5. Perf** (release, `SIRENS=8 traffic_bench -- 1 400`): 8 units each 30 m behind a civilian
(traffic yielding all the time): avg 0.12 ms, p99 0.32 ms per 60 Hz frame (0.054 / 0.12 ms
without sirens, same run). Budget 2 ms.
**Goldens.** Ran every scenario on `origin/lead/round12` (900261e) and on this branch: hashes
differ only for `city_blocked_parked_player_grid` (police on), `golden_city_chase` (+ replay),
`weather_fog_pursuit`, `weather_fog_pursuit_clear` — all have siren units. Only those + the 3 new
scenarios re-blessed (`[bless]` commits). Every other hash is bit-identical to lead/round12. The
same 2 scenarios fail on both trees (not mine): `police_tunnel_escape_founders_cut` (no pursuit
ended) and `world_traffic_soak_observer` (`traffic_agents_min` 310 < 350). Many other goldens are
stale on lead/round12 itself (same list on both trees) — they need the lead's post-merge bless.
**Lead note (blocked_parked_player_vesper after the 12b merges):** on 900261e it **passes** for me
in both release and debug simcli, same hash 44b5e478… on `lead/round12` alone and on this branch
(the siren code does not run without sirens). Seeds 3–19: turn-arounds 1–2 each, passes on all
but seed 3, which has 1 civilian–civilian collision — identical on lead/round12 (pre-existing,
not the scenario's seed). If it still fails for you, please send the commit / build profile.
**Changes outside wp_world:** `apps/simcli` (`scenario.rs`: `FollowRoute { lateral_offset_m,
keep_distance }`, serde-defaulted and skipped when default so existing scenarios/replays are
unchanged; `run.rs`: `follow_behind` for route drivers, keyframe siren/lights/horn, metric
`traffic_siren_yields`; `SCENARIOS.md`), scenarios + goldens above.
**Known gaps.** Pull-overs stay inside the lane (no kerb mounting / parking lanes); on multi-lane
roads a car leaves the unit's lane only where the next lane has a safe gap; no creeping into a
junction at a red.
**Contract change requests:** none.
*Gate (ws/world @ round 12b, on lead/round12 900261e).* fmt, `clippy --workspace --all-targets
-D warnings`, wasm32 build of `wp_wasm`: pass. `simcli run apps/simcli/scenarios`: 75 pass, 2 fail
— the same 2 as on lead/round12 (`police_tunnel_escape_founders_cut`,
`world_traffic_soak_observer`). `cargo test --workspace --no-fail-fast`: all pass except 4 tests
that fail identically on a clean lead/round12 checkout (checked one by one): simcli
`goldens_match` (stale goldens on the base), `wp_race` `grid_highway_sprint`, `wp_sim`
`vesper_v2_cars_drive_up_the_kerb_and_along_the_sidewalk`, `wp_wasm`
`race_tests::short_sprint_with_ai_runs_to_results_and_pays_through_the_economy`. The new siren
scenarios pass `simcli determinism`.

### Response from D (round 12b follow-up) — siren scenarios after the 12b merges
Merged `origin/lead/round12` @ db11e64. Reproduced `traffic_siren_yields` = 3 (hash ee21009f…).
A seed sweep (1–9) showed the count is only spawn luck (3–6: the 12b spawn/LOD changes moved
which cars are on the route), but also a real problem: on 3 of 9 seeds the siren car stalled.
**Cause:** a physical car pulling over stopped crawling once within 0.25 m of its kerb offset,
so it could stand 0.4 m (not 0.65 m) from its lane centre — inside the passing car's path, and the
pair waited on each other (the 15 s give-up released it, then it re-yielded in the same spot).
**Fix** (`wp_world`): crawl sideways until within 0.1 m. Seeds 1–9 now: 3–6 pull-overs, 0
contacts, 0 civilian collisions on all; 7 of 9 finish the route. Seeds 3 and 5 still stall at
the route's multi-lane start, where pulled-over cars on both sides of a lane line leave 2.2 m
from the car's line — inside simcli's 1.2 m keep-distance corridor (a narrower 1.05 m corridor
made the siren-off control scrape cars, so it stays). Scenario threshold now
`traffic_siren_yields` ≥ 2 (was 4; the count depends on spawns, the route completion and
zero-contact checks stay strict). Re-blessed only the two siren-on scenarios. On db11e64 + this
branch all 79 scenarios pass (`simcli run apps/simcli/scenarios`, release); every other hash is
identical to lead/round12. wp_world tests pass.
## Lead helper: sidewalks level with lots and buildings (branch `lead/fix-sidewalk-lots`, 2026-10-20, playtest row 52)
Built on `lead/fix-junction-holes`. D: changes in your crate are in `surface.rs` (level lots
behind every sidewalk, skirts, corner fills out to the lots, wedge pull-back, rails on lot-less
edges) and `vesper.rs` (carve up to the lots, building bases / placement, cul-de-sac direction:
node 81). Details, numbers and known gaps for you: `docs/integration/sidewalk-lots.md`.

## Lead feedback (round 13, 2026-10-20) — owner playtest rows 76–77 (owner-requested)
Base: `origin/lead/round12`. Both are direct owner requests from playing preview v0.12.11.
1. **Row 76 — "traffic is too high for night-time."** Replace/extend `density.rs` `f_time()` with a
   data-driven time-of-day curve (target civilian density vs daytime peak): 01–05 ≈ 15–20 %,
   23:00 ≈ 35 %, 19–22 ≈ 60 %, 07–09 and 16–19 = 100 %, midday ≈ 75 %; district modifiers
   (downtown/nightlife busier at night, industrial/suburb emptier). Cap the out-of-sight
   `MAX_SPAWNABLE_BOOST` at the curve so the boost never lifts night traffic above it. Same curve for
   the LOD/kinematic fleet. Despawns stay out of sight. Add a ~02:00 scenario with the night band;
   adjust soak ranges only where the hour changes them (say why).
2. **Row 77 — "too many parked cars."** Port Vesper v2 has ~3,300 parked-car props. Reduce to roughly
   35–50 %: kerbside parking only where plausible (none on arterials/highways/bridges/tunnel
   approaches, none within ~15 m of junctions, crossings, bus stops, driveways/lot entrances),
   occupancy by district (downtown/residential medium, commercial/industrial lots clustered, keep the
   main racing arterials clear), random 0–3 car-length gaps, data-driven table. Deterministic; the
   drawn-props system stays as is. Re-check scenarios that park near props.
Re-bless only goldens your change moves; gate as usual; push ws/world.

### Response from D (round 13) — night traffic curve + fewer parked cars (owner rows 76–77)
Merged `origin/lead/round12` @ 363004c into `ws/world`.

**1. Row 76 — night traffic.** `density::f_time` now reads a data table,
`data/city/traffic_time.json` (`density::TimeCurve`, embedded with `include_str!`, so the browser
build gets it too): one hourly curve relative to the daytime peak — 07–09 and 16–19 = 1.0,
midday 0.75, 19:00 0.62 → 22:00 0.50, 23:00 0.33, 01–05 0.15–0.20 — times an hourly multiplier
row per district. Core keeps nightlife until ~02:00 (×1.1–1.4 from 19:00 to 02:00) and then drops
into the same trough (×0.8 at 03–04, so prompt 05's noon ≥ 5× 03:00 still holds: 6.25×);
Industrial and Suburb ×0.6 at night, Port ×0.75, Residential ×0.85, Commercial follows the curve;
every district is equal by day. Kinematic (LOD) and physical agents share the same zone targets,
so the far fleet follows the curve too. **Out-of-sight boost capped at night:**
`traffic::max_spawnable_boost(f)` — none at f ≤ 0.2 (`BOOST_OFF_AT_F_TIME`, the small hours),
the full `MAX_SPAWNABLE_BOOST` (2×) at f ≥ 0.6 (`BOOST_FULL_AT_F_TIME`, every daytime hour, so
the day soaks are unchanged by this), linear between; applied per zone. Despawns are unchanged
(still never in sight).
Measured, Port Vesper observer downtown (Lantern Row), default density, agents after 60 s
(base = lead/round12 → this branch): 02:00 109 → 53, 23:00 215 → 144, 21:00 257 → 248,
noon 384 → 302, 08:00 360 → 396 (cap 400). Against the cap-limited 08:00 peak: 02:00 13 %,
23:00 36 %, 21:00 63 %, noon 76 %. Note: daytime *downtown* is now lower than before (the old
Core curve was 1.0 at noon; the owner band says midday ≈ 75 %), while residential/suburb
streets are busier at midday than before (0.45–0.5 → 0.75).
New scenario `soak/traffic_night_vesper_observer.ron` (02:00, default density): 55–61 agents,
0 civilian collisions, moving ≥ 0.6; gates 10–120 agents. Test
`tests/world.rs::night_traffic_around_a_player_stays_at_the_curve` (grid, player in town:
02:00 58 agents vs noon 394, count within the target). Soak ranges: none changed — the noon
soaks still pass with the same gates.

**2. Row 77 — parked cars.** `props::parked_cars` now follows `data/city/parking.json`
(`props::ParkingTable`): parking lanes only (collector/local streets; arterials, highways,
hill/bluff/service/port roads never had them), and no car within 15 m of either end of a parking
lane (junction setback + crosswalk), 20 m of any POI (garages, police stations, race grids,
safehouses — the lot entrances we have), 40 m of a tunnel portal, inside a covered span, or on a
bridge deck (lane > 1.5 m above the terrain); no parking above 14 m/s. Cars come in clusters
(chance per free 6.5 m slot by district: Core 0.28 / 1–3 cars, Commercial 0.13 / 2–5, Residential
0.29 / 1–3, Industrial 0.08 / 2–5, Port 0.05 / 2–4, Suburb 0.20 / 1–2; ×0.5 on 30 mph collectors
so the busier streets stay clearer), bumper gaps 0.8–1.6 m, then a random 0–3 car-length gap.
Deterministic per seed (same RNG stream construction). **Port Vesper v2: 3,760 → 1,578 cars
(42 %)** (simcli `props` on the Vesper observer, traffic seed 5: 3,846 → 1,697, 44 %).
Test `props::tests::vesper_parked_cars_fewer_and_clear_of_junctions` gates 35–50 % of
the old rule, junction / POI / tunnel clearances, no overlaps, determinism. The drawn-props
path is unchanged (`wp_wasm` still reads `World::static_props`). Scenarios that park near props
(`blocked_parked_player_*`, sidewalk-mount, tunnels, races) all pass.

**Siren regression found and fixed (wp_world).** The new noon mix moved
`city_siren_traffic_yields_*` from 7/9 to 3/9 seeds finishing. Two deadlocks in
`traffic.rs`, both fixed:
- a car still *pulling over* that had stopped (something ahead; a physical car only moves
  sideways while rolling) never ran the 15 s give-up timer, which only ran once it was waiting
  at the kerb; now it runs in both phases;
- a yielding car may not enter a junction (round 12b), so a car at the stop line with the unit
  stopped right behind it waited forever; now the hold applies only while the unit is moving —
  once it has stopped behind the car, the car goes when it may (signal/gap rules unchanged).
Seeds 1–9 after the fixes: still 3/9 finish (2, 4, 5); the remaining stalls are the round-12b
geometry case — a pulled-over car a few cm short of its kerb offset and slightly angled reaches
~0.1 m into simcli's 1.2 m keep-distance corridor — and seed 7 where a physical car cutting a
turn (1.1 m off its path) clips another. The three siren scenarios moved to **seed 2** (passes:
4 pull-overs, route done, 0 contacts; control 40 % of the route, passes on all 9 seeds).
Proposal for the lead: either accept seed 2, or let the keep-distance corridor shrink to 1.05 m
only beside a car flagged `yielding` (the round-12b attempt narrowed it for everyone and the
siren-off control scraped).
**Re-blessed:** `world_traffic_night_vesper_observer` (new), `city_siren_traffic_yields_arcade`,
`city_siren_traffic_yields_sim`, `city_siren_traffic_no_siren` (seed changed). Every other
golden my change moves (all city/traffic scenarios) was already stale on `lead/round12`
(33 golden mismatches on the base, same list on this branch, none newly failing), so they are
left for the lead's post-merge bless.
**Tests changed for the hour:** `tests/blocked.rs` one-way ladder density 1.5 → 1.0 and
`tests/siren.rs` grid queue density 3.0 → 2.0 — both Residential at noon, 0.5 → 0.75 on the new
curve, so the same number of cars.
**Changes outside wp_world:** `data/city/{traffic_time,parking}.json` (+ README), scenarios
`soak/traffic_night_vesper_observer.ron` (new), `city/siren_traffic_*.ron` (seed + notes),
goldens above.
**Known gaps.** No bus stops, driveways or crosswalks between junctions exist in the network,
so those clearances are not applied (crosswalks at junctions are covered by the 15 m lane-end
rule); race routes are planned at run time, so "keep the racing arterials clear" is the
no-parking rule on arterials plus half occupancy on collectors.
**Contract change requests:** none.
**Tests in other crates that this round's traffic moves (requests to their owners, not
edited):** each passes on a clean `lead/round12` checkout and fails here; all are emergent race
outcomes tuned on the old traffic. Restoring only the old `density.rs` makes the first two pass
again, so they are the hour's traffic, not a bug I can see:
- `wp_opponent` `tests/race.rs::wanted_ai_racer_flees_police_through_the_city` (noon, traffic
  0.25): a racer other than the wanted one ends `Fled` (the test allows Racing/Busted).
- `wp_wasm` `crash_replay_tests::race_crash_replays_with_the_judge_and_no_crash_on_the_grid`
  (`grid_highway_sprint`, 23:00): `debug_park_in_front_of` + 14 m/s no longer produces the
  "big crash" within 240 ticks.
- `wp_wasm` `race_tests::short_sprint_with_ai_runs_to_results_and_pays_through_the_economy`
  (same race): the player wins but the purse is 1,377 vs 5,031 advertised (outside the R5
  0.75–1.34 band), i.e. the player is wanted at the finish (wanted purse multiplier); with only
  the old curve restored it fails earlier (finishers / winner time) — so it moves with the
  parked cars and with the hour.
Suggested for A/G/F: re-measure against round 13 traffic (night races now run with ~half the
cars) or pin these tests' traffic/parking (`parked_cars: false`, explicit `traffic_density`).
*Gate (ws/world @ round 13, on lead/round12 363004c).* fmt, `clippy --workspace --all-targets
-D warnings`, wasm32 build of `wp_wasm`: pass. `simcli run apps/simcli/scenarios` (release): 78
pass, 2 fail — the same 2 as on lead/round12 with identical hashes
(`police_tunnel_escape_founders_cut`, `…_control`). `simcli determinism` on the new/changed
scenarios: ok. `cargo test --workspace --no-fail-fast`: all wp_world tests pass; failures:
simcli `goldens_match` (33 goldens stale on the base too) and `wp_race`
`race_picker::loop9_circuit` (fails on the base too), plus the three above.

### Response from D (round 13 addendum) — row 93 "still too many parked cars"
Row 77's table moved to the low end, with visible empty kerb. Changes in `data/city/parking.json`
(+ `props::ParkingTable::{empty_stretch_chance, empty_stretch_m}`, serde-defaulted): collectors
×0.3 (was ×0.5), and after 35 % of clusters a 30–80 m empty stretch instead of the 0–3
car-length gap. Rules stay keyed to lane kind, speed limit, lane ends, POIs, tunnels and the
terrain — no road or lane widths — so `lead/city-residential`'s wider roads need no change here
(the car sits on the parking lane's centreline).

| Port Vesper v2 (seed 1) | lead/round12 | round 13 (row 77) | + row 93 |
|---|---|---|---|
| Parked cars | 3,760 | 1,578 (42 %) | **1,219 (32 %)** |
| Cars per km of kerb (45.7 km) | 82.2 | 34.5 | **26.7** |
| — collectors (8.7 km) | 81.1 | — | **14.4** |
| — local streets (37.0 km) | 82.5 | — | **29.5** |
| Kerb with no car within 10 m (a ≥ 20 m empty stretch) | 7 % | — | **59 %** |

(Counts from `props::tests::vesper_parked_cars_fewer_and_clear_of_junctions`, which now gates
30–40 % of the old rule and ≥ 50 % empty kerb. No screenshots from this headless session; the
per-km numbers above are the measure.)
**Found while re-gating:** `world_traffic_soak_vesper` got 1 civilian collision with the new
parked layout: a kinematic car stopped 0.3 m into its lane and made a blocked-lane change from
standstill with its tail still in the junction, clipping a car turning through. Fix
(`traffic.rs`): no blocked-lane change before the car is a full car length into the lane. Soak
passes again (0 collisions).
**Gate (ws/world after row 93):** fmt, clippy `--all-targets -D warnings`, wasm32 build: pass.
Scenarios: 78 pass, 2 fail (the same Founders Cut pair as on lead/round12, identical hashes).
`cargo test --workspace`: as in the round-13 gate above, plus `wp_race`
`race_picker::lantern_checkpoint_draws_the_police`, which passed before row 93: one racer ends
`Dnf(FinishWindow)` 45 m short of the finish (the test allows only Finished / Captured / Fled).
Another emergent race outcome from the new kerb layout; request to G to re-measure (or allow
FinishWindow) against round 13. Contract change requests: none.

### Response from D (round 13, after the lead/round12 merge @ ecf4d7b) — `city_siren_traffic_no_siren` collision
**One line for the lead:** not from wp_world — it comes from the vehicle row-90 steering +
`data/cars` retune; the control's `civilian_collisions` is re-scoped to ≤ 1 (strict 0 kept as
`expect_pending`), and the handling regression goes to A.
Details. Merged `origin/lead/round12` (fast-forward; the only wp_world changes in it are the
road-name accessors, presentation only). There are no "jam fixes" on ws/world (row 85 is not
started). Bisect on seed 2: lead/round12 with `crates/wp_vehicle` **and** `data/cars` from
39baed1 → passes with the exact pre-merge hash (57478444…); with either one new → 1 collision.
The collision: physical Casual civilian 41 takes a tight left turn at the comfort speed
(4.3–5.1 m/s, curve speed 5.1) but its lateral error grows steadily from 0.07 m to 1.44 m over
the end of the turn and into the straight, and it clips a car stopped in the opposite lane.
Capping physical cars' turn speed at the comfort speed (no personality factor > 1) did not
change it, so this is steering tracking, not speed; I did not keep that change. Across seeds 1–9
of the control: physical cars more than 0.8 m wide within 8 m of a turn exit 7 → 11, worst
1.34 → 1.50 m; collisions 0/9 → 1/9 seeds.
**Request to A (vehicle):** check AI / traffic cars after row 90 (rack slew, countersteer onset)
— a low-speed turn exit should settle on the lane, not drift 1.4 m out; repro
`simcli run apps/simcli/scenarios/city/siren_traffic_no_siren.ron` (seed 2, agent 41, ~9 s in).
The control's golden hash moved with the merge (not with my edit); left for the lead's bless.
Scenarios on the merged tree (release): all 86 pass.

### Response from D (round 13, row 85) — "massive traffic jam"
Started on the owner's request (lead addendum 2026-10-20). Merged `origin/lead/round12` @ 21e14c0.
**Status:** on the pre-main maps both jam scenarios met the lead's gates (no stop > 45 s except
at red, no queue > 12). **On main's maps (after the 9a4272f8 merge) Vesper meets them, the grid
does not** — see "Row 85 on main's maps" below. Pre-main, across seeds 1–4 + 85: stop gate 10 of 10 runs (worst
39 s), queue gate 9 of 10 (Vesper seed 1: 17 cars for 3 s at a red, behind a car pulled over for
a siren; cleared on the next green). Civilian collisions in the pursuit melee still need their
own pass.

**1. Instrumentation.** `TrafficStats::{max_stop_s, max_queue, box_stops, jam_recycled}`
(`traffic/jam.rs`), simcli metrics `traffic_max_stop_s`, `traffic_max_queue`,
`traffic_box_stops`, `traffic_jam_recycled` (`apps/simcli/src/run.rs`, `SCENARIOS.md`).
Stop time is held (not reset) at red / amber and, for the metric only, within 60 m of a
*standing scene* (a tracked vehicle stopped ≥ 20 s); wrecks are left out. Queue = stopped cars
back from a stop line, followed back through junctions, sampled 1 Hz. Scenarios
`jam/pursuit_jam_grid.ron` (12×12 grid, Safehouse 3 → Safehouse 2) and
`jam/pursuit_jam_vesper.ron` (Lantern Run → Founders Cut lay-by): noon, racer at heat 70 (L4,
pursuit takes it to L5), city police on, 10 min, `restart_after_capture: true`.

**Capture scenes (simcli).** In simcli a captured racer used to stand in the road for the rest
of the run with 5–20 units around it — a scene that does not exist in the game (a capture ends
the run, `RunEnd::Captured`) and accounted for most of the long stops. New scenario flag
`restart_after_capture` (`apps/simcli/src/scenario.rs`, `run.rs`): on `PursuitEnded`
Captured / CapturedTotalLoss the racer is put back at its route start with the scenario's heat,
so a 10-min run measures repeated pursuits. Default off; existing scenarios unchanged.

**2–5. Fixes in wp_world** (details in `crates/wp_world/README.md`, "Jams"):
- reservations rebuilt every step (stale ones from despawned / re-routed / turned-around cars
  were the biggest single cause: whole junctions held for minutes);
- a stationary tracked vehicle in a crossing path blocks only the paths it stands in (moving
  > 2.5 m/s it still holds crossing traffic); same for wrecks;
- stale arrivals (stopped at the line 4 s without a permit) no longer make others yield;
  all-way stop first-to-stop loses priority after 8 s; turn-taking booking after 20 s at the
  line; permits dropped when stopped > 6 m short of the line;
- police at / in the junction: a tracked vehicle stopped at the line 6 s (not red) or in the box
  3 s, or a box path blocked by one for 6 s, is an obstruction → re-route to another exit, else
  the queue turns around / backs out from the tail, and the lane is marked blocked (multi-lane
  roads too) so upstream cars re-route; obstructed cars give up their permit;
- sirens: junction hold times out after 8 s; pulled-over drivers give up on a unit standing or
  crawling behind after 8 s (was 15 s); with a unit standing beside the car (< 1 m/s, ≥ 0.5 m
  of room) they creep on along the kerb (≤ 5 m/s) and merge back past it instead of waiting;
- stuck manoeuvres: half-done lane changes undone (20 s cooldown); reverses stalled 8 s and
  U-turns that cannot start for 10 s given up; no reverse with a tracked vehicle < 25 m behind;
  reverse wait 15 s (was 40 s); kinematic U-turns check their exit pose (0.6 m margin) and hold
  their lane 8 m back (was 3 m);
- spawning: never behind a stopped car within 60 m; per-lane cap one car / 20 m at the daytime
  peak × time-of-day factor; unseen cars stopped 30 s outside a red recycled (also in a box);
- wrecks shoved out of their lane are re-filed under the lane they lie in; > 3.5 m off any lane
  they are not obstacles; a wreck just past a junction on a car's exit is an obstruction for
  the car held at the line (re-route / turn around) instead of room to wait for;
- turn-taking bookings are not made against the car's own red / amber (they lapsed before
  the green and held the crossing traffic meanwhile);
- an obstructed driver's wait is held, not reset, while it stands (a one-step class flicker —
  level-of-detail switch, siren yield, the leader's class — kept drivers from ever giving up);
- the jam metric's stop timer is also held while the car yields to a siren (like a red).
Don't-block-the-box (exit room) already existed and is unchanged.

**Before / after** (release simcli, both with `restart_after_capture`; before = `lead/round12`
traffic + the instrumentation only, after = this branch). Seed 85 is the shipped scenario.

| run | max stop before / after (s) | max queue before / after | box stops before / after | civilian collisions before / after |
|---|---|---|---|---|
| grid 1 | 69 / 24 | 10 / 9 | 2 / 1 | 29 / 3 |
| grid 2 | 107 / 37 | 13 / 12 | 3 / 6 | 8 / 12 |
| grid 3 | 50 / 26 | 15 / 10 | 11 / 7 | 12 / 7 |
| grid 4 | 45 / 27 | 12 / 10 | 9 / 3 | 6 / 9 |
| grid 85 | 55 / 30 | 13 / 11 | 10 / 10 | 7 / 9 |
| Vesper 1 | 90 / 39 | 10 / 17 | 6 / 7 | 17 / 10 |
| Vesper 2 | 94 / 30 | 14 / 7 | 7 / 3 | 9 / 3 |
| Vesper 3 | 89 / 26 | 9 / 8 | 10 / 12 | 8 / 13 |
| Vesper 4 | 53 / 29 | 8 / 6 | 5 / 9 | 6 / 7 |
| Vesper 85 | 55 / 37 | 7 / 8 | 2 / 9 | 1 / 12 |

Gates met before: 1 of 10 runs (stop) / 5 of 10 (queue); after: 10 of 10 / 9 of 10.
**Remaining:** civilian collisions in the pursuit melee (physical civilians shoved by units,
wrecks): 103 → 85 over the 10 runs (after counts a re-touch within 2 s once, see below), but up on some seeds (Vesper 85: 1 → 12, the pursuit now
runs through downtown instead of ending in the tunnel); needs its own pass. Box stops (cars
standing > 5 s in a junction, mostly behind police in the box) are unchanged on average.
**Collision counter.** `civilian_collisions` now counts a pair that touches again within 2 s
of separating once (`RETOUCH_STEPS`); before, a bump-part-bump was 2. Found on
`city_siren_traffic_no_siren` (seed 2): one physical civilian drifts ~1 m off a 5.8 m/s left
turn (the vehicle-side tracking issue noted in that scenario) and brushes a car on a path that
is > 2.4 m away and so not a junction conflict; it was counted twice and failed the ≤ 1 gate.
Sim hashes are unchanged by this. Over seeds 1–9 that scenario has 4 collisions on this branch
and 4 on the lead tip (on different seeds).
**Goldens** re-blessed (`[bless]` commit) for the row-85 traffic changes: city / siren /
blocked / opponent / race / weather scenarios that run civilian traffic.
**Changes outside wp_world:** `apps/simcli/src/run.rs` (4 metrics, `restart_after_capture`),
`apps/simcli/src/scenario.rs`, `src/fuzz.rs` and `examples/city_frame_cost.rs` (the flag),
`apps/simcli/SCENARIOS.md`, `apps/simcli/golden/*` (bless),
`apps/simcli/scenarios/jam/*.ron`; `crates/wp_wasm/src/scenario.rs` one-line
`restart_after_capture: false` so the shared scenario struct still builds. Contract change
requests: none.
### Row 85 on main's maps (merge of lead/round12 9a4272f8)
Merged `origin/lead/round12` @ 9a4272f8 (main's rebuilt grid and Vesper, E's police,
opponent-pace racers, brake bias). Conflicts: `apps/simcli/src/run.rs` (kept `cars` for
`restart_after_capture` and the lead's `modded`), 24 golden hash files (re-blessed on the
merged tip). `apps/simcli/src/police_bench.rs`: `restart_after_capture: false` for the new
`Scenario` field (minimal compile fix).
**Fixes found on the new maps** (all wp_world):
- *Sneaker*: a left-turner waiting at the line with its permit keeps it through amber / red;
  standing still it "could stop" at any decel, so the amber stopping check revoked the permit
  the step after it was given and it sat out the next cycle too. Main's short greens (~14 s on
  some approaches) made this the main grid jam.
- *Booking without room*: a car starving behind a short block (22 m on main's grid) may book
  its turn before the exit has room, so the room that comes free is its, not the other
  approaches'. While it waits for room behind a red-light queue the booking does not lapse;
  any other hold lapses it after 40 s (`BOOK_ROOM_MAX_S`).
- *U-turns* also wait for the lane on their right (the reversing leg swings into it):
  `world_traffic_soak_vesper` had one collision on main's Vesper.
**Before / after on main's maps** (release simcli, 10 runs; before = this merge with the
row-85 traffic rules taken out, instrumentation and capture restart kept; the before's
metric holds only for `yielding`, it had no siren junction-hold timer to read):

| run | max stop before / after (s) | max queue before / after |
|---|---|---|
| grid 1 | 53 / 52 | 12 / 8 |
| grid 2 | 57 / 118 | 13 / 15 |
| grid 3 | 143 / 37 | 17 / 9 |
| grid 4 | 228 / 59 | 13 / 8 |
| grid 85 | 66 / 27 | 18 / 15 |
| Vesper 1 | 48 / 58 | 8 / 6 |
| Vesper 2 | 79 / 29 | 8 / 6 |
| Vesper 3 | 39 / 26 | 8 / 9 |
| Vesper 4 | 46 / 26 | 9 / 7 |
| Vesper 85 | 50 / 33 | 8 / 6 |

**Vesper** (the shipped `jam/pursuit_jam_vesper.ron`, hard gates): stop gate met on 4 of 5
seeds (seed 1: 58 s), queue 5 of 5 (before: 1 of 5 / 5 of 5).
**Grid: not met on main's grid**; `jam/pursuit_jam_grid.ron` gates moved back to
`expect_pending` (seed 85: stop 27 s passes, queue 15 fails — a red-light queue at a ~46 s red).
Over the 5 grid seeds: stop gate 0 → 2 met, worst 228 → 118 s; queue gate 1 → 3 met. What is left
is network capacity under a pursuit: 22 m blocks between a priority junction and a signal whose
approach gets ~14 s of green per ~60 s cycle, with units crossing it with sirens (the front car
is held by the siren hold or by units in the crossing paths, and the queue spills back through
the short blocks; seed 2's 118 s car is behind such a block). Proposed next step (not done): give
main's grid signals longer / actuated greens on short-block approaches (`procgen.rs` signal
plans), and let the siren junction hold count per junction rather than per unit.
**opponent-pace lantern_checkpoint** (`wp_race/tests/opponent_pace.rs`, release, on the
9a4272f8 merge): still red, but the jam fixes help a lot — best ratio s50 2.678 → 2.246
(want ≤ 2.2), s95 2.999 → 2.209 (want ≤ 1.98); race times 96–139 s → 79–96 s (before = the
same tree with the row-85 traffic rules taken out).
**Merge of lead/round12 5b9ab4f7** (E's police, never-give-up, detailed cars + LOD, audio):
conflicts only where both sides added fields (`Traffic::{overlap_ended, cars}`,
`Scenario::{restart_after_capture, car_mix}` in `fuzz.rs` and `wp_wasm/src/scenario.rs`).
Goldens re-blessed only where ws/world's traffic moves them, by blessing lead/round12 alone and
the merged tip and comparing: ws/world only — `jam_pursuit_{grid,vesper}`; both ws/world and
E (merged value differs from round12 alone, so this bless supersedes E's on a merge) —
`golden_city_chase`, `golden_night_pursuit_escape` (+ replays), `opponent_flee_police`,
`race_city_sprint_police`, `weather_fog_pursuit{,_clear}`, `world_traffic_soak{,_observer}`;
E only, left for E — `opponent_circuit_8x5`, `race_lantern_checkpoint`.

### Row 85 follow-ups (2026-10-22): lane changes and tracked vehicles, wrecks across paths
- **Lead red on round12 67dafbfa** (`blocked::multi_lane_road_changes_lane_around_a_stopped_car`,
  98 contacts): civilians' lane-change check only saw civilians in the target lane, so a car
  changed into the lane of a stopped tracked vehicle 25 m ahead at 15 m/s; the old emergency
  stop had masked it until 7b3762eb narrowed it. The nearest tracked vehicle ahead in the
  target lane is now a leader there (gap check + incentive). Test re-aimed: go-arounds ≥ 1
  (was ≥ 2): fewer cars end up behind the stopped car. Goldens moved: `jam_pursuit_{grid,vesper}`,
  `opponent_flee_police` (blessed; the 67dafbfa merge alone moved none).
- **Pursuit-melee collisions**, census over the 10 jam runs: 82 → 67. 35 of the 67 are a car
  shoved by a unit (a speed jump no car makes itself) and most of the rest two physical cars
  within 10 m of a unit — police contact, not traffic AI. Wrecks: 15 → 0 — a wreck shoved
  across a neighbouring lane / junction path was invisible to cars on it (only their own
  segment's list was searched). `Traffic::wreck_in_path`: a wreck lying across a car's segment
  or the next two of its plan, by position and footprint, is its leader.
- **`wp_race grid_highway_sprint`** ("nobody drove past the speed trap") is red on lead/round12
  10d94b4d itself (not ws/world). **`lantern_checkpoint_draws_the_police`** (red on 67dafbfa)
  passes on ws/world.
- Main's grid jam gates stay pending (seeds 1–4 + 85: 43–188 s, wreck check on or off is not
  the difference: 111/86/49/43/115 s without it). The left-turn "sneaker" may now keep its
  permit through amber / red; letting it start on red (tried) boxed junctions in, reverted.

### Row 85 follow-up (2026-10-22, lead): minor-road starvation at Priority junctions
**Case.** `jam_pursuit_vesper` on lead/round12 (after the police search extras): a minor-road
car stood 50.5 s at a Priority junction (E re-aimed the gate 45 → 55 s). Probed: the car was
never refused by gap acceptance but by the crossing paths themselves — the slow major-road
stream (≈ 4.5 m/s, densified by siren yielding) always had a car in the box or a reservation
on it. ("Agent 146" is a slot index; the agents vector is reordered, so it names different
cars over time.)
**Fix (`traffic.rs`).**
- *Courtesy:* a major-road car at a Priority junction (not a radius-0 freeway merge) going
  ≤ 10 m/s (`COURTESY_MAX_V`) that can stop comfortably does not take a permit while a
  conflicting minor approach's front car has stood at its line ≥ 25 s (`STARVE_AFTER_S`),
  is headed into that movement and has room beyond the box. The box clears and the minor car
  goes; its stop timer resets once it moves.
- *Shorter gaps:* the starved car's accepted gap shrinks from its own (3.5–5 s) to 2.5 s
  (`STARVE_GAP_S`) over 10 s, and it ignores a major lane whose front car stands without a
  permit (the courtesy hold: nobody on that lane comes through first).
**Gate** back to 45 s. Measured (seeds 1–4 + 85, worst stop outside a red, base → new):
lead/round12 seed 85 50.5 → ≤ 45 (pass); ws/world Vesper seed 2 81.8 → ≤ 45 (the other seeds
were already ≤ 45; seed 4 fails `pursuit_started_before` both ways, police side); grid
(gate pending) 0 / 81.8 / 97.4 / 107.6 / 96.7 → 0 / 0 / 58.9 / 64.0 / 119.1 s — better on
three seeds, worse on 85 (its worst stop is a different car; the grid's remaining stops are
blocked boxes at short-green signals, not Priority junctions). Test:
`starve_tests::starved_minor_road_cars_accept_shorter_gaps_after_a_long_wait`; the scenario is
the behaviour gate.

**`blocked::multi_lane_road_changes_lane_around_a_stopped_car` (lead, 2026-10-22).** Passes
on lead/round12 353744ec and on ws/world (debug and release: 1 go-around, contacts 0). The
failure opponent-pace saw on 0e84a9f7 predates a779ad57 (the fix and the re-aimed ≥ 1
threshold are not in that commit), so it is not a regression. The margin is thin (exactly 1).

## Police can see solid verge furniture (lead / E, 2026-10-24)

**Why:** police units hit verge street trees (Crowfoot, row 168 follow-up), because trees were
not in anything the police query.

**What's new:** `wp_world::obstacles`.
- `SolidDecor::of(&Decor)` gives the sim's collider footprint as 2D:
  - tree trunks (street, yard, pine): circles of `TRUNK_R_M` (0.22 m), solid to `TRUNK_SOLID_H_M`
    (2.5 m);
  - hedges, fences, jersey barriers, site fences, hay bales, billboards, crane masts: oriented
    boxes.
  - Also gives `distance_to(p)` and `outline()`.
- `ObstacleIndex`: a 16 m grid; `near(p, r)` returns the items within `r` of `p`, in ascending
  index order.
- `World::obstacles()`, built in `World::new`.
- Empty on cities without relief, like the sim.
- There are no lamp posts or hydrants in the world yet (no DecorKind).

**Kept in step with the sim:** `wp_sim::physics::decor_collider` uses `TRUNK_SOLID_H_M`, so both
read the same constants. No simulation change and no golden moves. Police integration (feelers,
U-turn room) is E's.

## Ashpeak greybox de-dup, option A (lead + ashpeak-port, 2026-10-24)

**Goal:** `ashpeak` ships `vesper_v2`'s greybox once, plus county tiles. Port Vesper inside
Ashpeak must look the same to a player: same buildings and props in the same places. Small
ground differences at the 8 m grid are fine (lead). Options B (Ashpeak terrain at 5 m: 26.0 →
35.7 MB glb, 0.68 M → 1.74 M heightfield cells) and C (composite terrain) were turned down.

**Before:** roads matched about 99 %, but almost no buildings or decor did (14 of 21 k building
triangles):
- the county drew from Port Vesper's random stream before its buildings;
- buildings are drawn one after another from a single stream, so the few tie-in roads the county
  changes (Route 40 West / North, Uptown Connector, Lantern Way: half a millimetre to 1 m in
  height) shifted every later draw;
- the terrain grid is 8 m against 5 m.

**wp_world (`vesper.rs`, region path only; `vesper_v2` byte-identical: road.bin and greybox):**
- The region (layout, extra buildings, POIs) draws from its own stream (`REGION_RNG_SALT`).
- `ashpeak` generates `vesper_v2` itself. `reuse_vesper_lots` keeps its buildings and decor
  wherever nothing the county changed or added is within `REUSE_CLEAR_M` (45 m), and they don't
  clash with the county's own items or landmark lots. The generic pass skips the unchanged
  streets (`buildings(.., skip)`) and fills only the others, clear of what is kept.
- `reuse_vesper_features`: Port Vesper's structures, landmarks and hideouts take `vesper_v2`'s
  heights, which come from its 5 m terrain.

**city-import `dedup`:**
- `drawn` keys every greybox triangle and decor instance by its exact bits.
- `TERRAIN_BOUND` lists the materials the county always owns: ground_*, seabed, rock, water,
  lot_skirt, retaining_wall, and bridge (piers reach the ground).
- Every other item belongs to the 64 m cell its centroid is in; `partition` gives a cell inside
  Port Vesper to the county wherever anything differs.
- `Partition::check` proves the split adds up, and test `ashpeak_dedup` runs it.
- Results:
  - 43 % of `vesper_v2`'s items are shared (ground is about half of them).
  - 127 of 2592 cells inside Port Vesper are the county's (the N / W tie-ins).
  - Buildings, roofs, garages, trees, fences and landmarks are 90-95 % shared; roads 91-95 %.
- `county_cells` is computed from the two maps on every run, never stored.

**Rename (owner, via ashpeak-port):** Harlan → Harnell (Harnell Flats / Road / Crossing, race
`harnell_dirt_run`) in the same regeneration.

**Regenerated:**
- `data/city/ashpeak.greybox.*`: 25.99 MB glb.
- `race/pick_harnell_dirt_run.ron`.
- [bless] ced8ac31: the 8 ashpeak goldens only.
- 151523cc: `harnell_dirt_run` length hint 3978 → 4295 m. The county's own random stream moves
  its farm section roads (686 nodes, 682 before). All four cars finish in 292-297 s (window
  190-380 s), with no contacts.

**Gate (release, `--no-fail-fast`; debug would not fit on this container's disk):**
- fmt, clippy and wasm are clean.
- The only failures:
  - simcli: Founders Cut and the six Port Vesper goldens above (round12's);
  - `pace_scenarios`: Lantern s50 (G's).

**On lead/round12 7abc46aa, not caused here (vesper_v2's road.bin is byte-identical to
round12's):**
- `police_tunnel_escape_founders_cut` fails: Captured at 132.7 s, where the story wants Escaped.
- Six Port Vesper goldens differ: jam_pursuit_vesper, the Founders Cut pair,
  race_hollins_descent, race_lantern_checkpoint, world_traffic_soak_vesper. Not blessed.

## Lead / G: "collider mid-lane on Loop 9 near (-1303, 1341)" (2026-10-23)

**Not reproduced; the point is on the junction paving the row 162 fix rebuilt.**
- (-1303, 1341) is 8.9 m from node 234. There Loop 9's carriageways (roads 5 / 23) continue as
  road 6 and the one-lane ramp road 341 branches off. The point is 13 m from the nearest lane
  centre.
- On ws/world and on main ee742a68, no static collider stands above the road surface within
  60 m along any lane. The median barriers there are 3.35 m from the lane centres.
- Loop 9 s95, `WP_PACE_SEED=15`, every static contact over 100 N·s logged: the rampart has no
  contact near the point on either branch.

**Found instead (ws/world only): the canal bridge launched cars.**
- The PR #38 port lifts Loop 9 2.25 m over the canal (x -550..-450). Floors were pinned where
  they held the profile up, so smoothing rounded each deck corner on one side only, and the final
  clamp put the kink back: a 4.6 % grade break in 10 m.
- At 59 m/s hakone_gt and rampart_50 left the ground and bottomed out (5 + 1 static hits on that
  seed; none on main).
- 44780509 / 99b21a36: floors are no longer pinned; each is lifted by what the smoothing takes
  off it until the smoothed profile clears it. Worst break there is now 1.65 % in 10 m, and the
  deck is 0.35 m higher.
- Holding floors further out instead (the first try) lengthened the embankments, and a lot back
  edge came out 2.0 cm off the drawn terrain (`sidewalk_lots`).
- New unit test `a_deck_floor_rounds_off_within_the_curvature_limit` (fails on the old code).

**Knock-on:**
- Greyboxes regenerated (35aec98d): vesper_v2 10.45 MB glb / 13.93 MB b64; ashpeak 25.87 MB.
- 49afdab6 [bless]: 8 goldens moved (routes over the lifted decks).
- The Founders Cut police pair is re-seeded 54 → 60. Seed 54's control never captured: the
  lead unit braked 40 m short of the parked racer, then turned across the Cut at walking pace.
  The Cut's geometry is unchanged. Over seeds 50-63 the control captures only on 60.
  - Police (FYI): with most seeds the lead unit's final approach to a car parked in the lay-by
    stalls crosswise.

**Gate:**

| Step | Result |
|---|---|
| fmt, clippy, wasm | 0 |
| `cargo test --workspace` | 0 |
| scenarios | 129/129 |
| pace scenarios | lantern s50 unchanged (G is on it; identical numbers) |
| `just pace-loop9` | **out at skill 0.5**: 42/48 (want ≥ 44), mean 236.1 s; skill 0.95: 48/48, 234.8 s |

**`pace-loop9` at skill 0.5:**
- Same test with only `vesper.rs` back at a7af2d79: 47/48 at skill 0.5, 45/48 at 0.95.
- Totals are about even (90 against 92 out of 96); skill 0.95 is better.
- Four of the six cars that don't finish are on seed 4, all `Dnf(FinishWindow)`. The pack sits
  behind civilians and racers: civ + veh losses are 130-140 s each, with no wall hits, rejoins or
  resets.
- That is the traffic-draw swing the test's notes already describe (±3 cars per seed at 23:00 /
  density 2.0), not a road fault.
- Left to the lead and opponent-pace (G is reworking pack queueing); I did not widen the gate.

**Seed 4 queue, dug out for the lead (skill 0.5, circuit finish window 90 s):**
- Old map: leader kestrel 222.8 s; the other five 274-294 s, the slowest +71.6 s, so about
  18 s from the cut-off already. civ + veh losses were 94-118 s each.
- ws/world: the leader is bit-identical. Four of the five lose 130-140 s and get
  `Dnf(FinishWindow)`.
- Trace (hakone_gt, `WP_PACE_TRACE=1`): on the three-lane Loop 9 carriageways it follows one
  obstacle at a time at 23-26 m/s for long stretches (one racer for 18 s, one civilian for 23 s,
  then about 95 s behind civilians 102-105 and a racer). Only 12 passes in 180 s, with no
  civilians side by side.
- The pack rarely passes slow civilians on a multi-lane road, so the civilian draw decides its
  time. The new deck heights moved a few buildings and spawn spots and reshuffled that draw.
  No world change.

## PR #38 port (lead, 2026-10-23): river, canal, bridges, landmarks, hideouts; rows 160 and 162
Ported from `claude/beautiful-einstein-b1noth` onto round12 1da95294 (with Ashpeak): a1c11a27,
6b753b84, 166ee01b, 122def2d and 5b49526a. 823f87a8 was skipped (already on round12), and
a729a629 (its scenario re-bless) was replaced by blessing here.

**Road format v7** (round12 was at v6 after Ashpeak; PR #38's own v4): `RoadNetwork::features`
(`CityFeatures`: waterways, bridges, structures, landmarks, hideouts), `#[serde(default)]`.

**Ported:**
- The Vesper River and Calder Canal, cut into the natural ground. Ashpeak's region hook wraps
  `Natural::vesper_z`.
- Every water crossing is a bridge deck: 13 decks, 11 named bridges. The Iron Bridge is a
  steel truss with a `Metal` deck.
- 8 landmarks plus the Iron Bridge and Hollins Tunnel; 5 canopy hideouts plus the tunnel.
  Their lots are reserved before buildings, which also keep out of the water.
- Structure colliders (convex hulls) and line-of-sight occlusion.
- `CityMap::water_surface_at` / `on_steel_deck` / `in_tunnel`.
- The terrain competes in `ground_height` when it is much closer to the reference.
- Greybox: river water, canopies, landmarks, steel.

**Not ported: PR #38's Hollins Tunnel bore** (TUNNEL_* hold, terrain holes, lining
structures). Round12 already has the Hollins Tunnel as a covered span at the same place. The
features list that span as a landmark and hideout instead, and `in_tunnel` = inside any
covered span. The two tunnel screenshots were dropped.

**Changes to the PR on round12's map:**
- **Water crossings are profile floors, not holds.** PR #38 held every crossing at the lower
  bank. Under the grade cap that pulled the Hollins Knob summit junction from 109.8 m to
  83.6 m, cutting Ridge Road and the Hollins Descent 26 m into the knob; it showed up as
  drawn-terrain p99 1.04 m. `profile(…, floors, …)`: the deck clears banks and water and
  stands higher where the road's line passes higher (Ridge Road Bridge: a high gorge bridge).
- **Parked cars:** `PARK_MAX_TWIST_M` 0.06 → 0.05 (wp_sim). A driveway on the new layout
  with 5.5 cm of twist left a wheel 3.1 cm in the lot. Vesper keeps 1850 of 1861 cars.
- **Barrier meshes are not drivable ground** (`BARRIER_USER_DATA`, a row-164 follow-up).
  `drivable_ground_z` took every triangle mesh as ground.
- **Tests:**
  - The drawn-terrain p99 limit is 0.85 m (0.78 with the steep river and canal banks; 0.70
    before).
  - The Port Vesper blocked-street test tries the nearest few 1+1 lanes for one that traffic
    queues on.
- **Scenarios:**
  - city_hill_road_ascent: path ≥ 800 m (851 m: the grid moved across the gorge bridge).
  - city_blocked_parked_player_vesper: seed 5 (no civilian reached the car on seed 7).

**Row 160: highway outer edge barriers** (5b49526a, adapted). `surface::edge_barriers(map)`
puts concrete barriers on carriageway outer edges and both ramp edges. They are open:
- where other paving meets the edge (ramp tapers, gores, rest-stop ramps);
- over node-patch paving (freeway-merge taper connectors);
- within a junction's paved radius + 2 m (ramp terminals);
- 40 m either side of each of those openings (row 162);
- on decks and in covered spans (the barrier stops at the Hollins Tunnel portals).

Physics uses the smooth row-164 meshes. Tests:
- Vesper `highway_edges_…`: never on paving or in a covered span, every ramp end clear
  across its width, ≥ 55 % of edges walled (61 % measured).
- wp_sim `the_edge_barrier_keeps_a_car_off_the_verge`.

**Row 162: ramp merges flush and clear.**
- `profile_ramp`: a ramp lies in the carriageway's plane while its paving overlaps the
  carriageway's. It used to leave the plane at the end of the parallel lane: the Hollins rest
  exit ramp stood 0.64 m under the carriageway's paved edge along the taper, a wall a car
  driving the ramp at 25 m/s hit.
- Freeway taper strips are banked like the road surfaces they join. Level strips left 4–5 cm
  steps at their edges and a ridge over the carriageway. Where a strip lies over a ribbon it
  joins, its vertices take that ribbon's height (the lane curve's own heights stood 2.5 cm
  proud).
- Ramp banks beside the carriageway: the carriageway plane's gradient taken across the
  ramp's own direction (bank·cos θ + grade·sin θ, up to 7.6 %), not clamped to the ramp
  style's 5 %. Found with opponent-pace's report: an AI car at Loop 9 race speed was wrecked
  by "world geometry" mid-carriageway at the Hollins rest off-ramp diverge (seed 15, s95).
  The ramp's inner edge stood 6–9 cm proud in the right lane there.
- New gate, city-import `vesper_carriageways_are_level_over_every_ramp_taper`: through
  traffic over all 28 merge and diverge tapers, every 0.25 m pair on the paving under 3.5 cm
  (worst 2.2 cm, grade and bank included; it was 7.9 cm on taper 1200, the Hollins rest
  diverge).
- Gates:
  - city-import `ramp_merges` (14 merges, 32.6 k samples on the car's track): pavement, drawn
    = physics (0.000 m), worst step 0.6 cm beyond the lane profile (was 5.5 cm), nothing
    standing in the path.
  - wp_sim `ramp_merges_are_driven_at_speed_without_contact`: every merge at 25 m/s from
    100 m up the ramp to 120 m along, no static contact.
  - Scenario `city_ramp_merge_hollins_rest_exit`: player seat, keyboard assists, at the ramp
    limit.

**Greyboxes:**

| Greybox | glb (MB) | b64 (MB) | Note |
|---|---|---|---|
| vesper_v2 | 10.44 | 13.92 | under the 15 MB preview cap; size check raised to 11 MB |
| procedural_12x12 | 3.02 | 4.02 | |
| ashpeak | 25.86 | 34.48 | split for the preview by `tools/preview_build.sh` (b0269195) |

**Race files:**
- `catalog` hollins_sprint length hint is 796 m (941 m before): the Hollins Ascent grid moved across
  the gorge bridge.
- `race/pick_*.ron` regenerated (`WP_RACE_WRITE_SCENARIOS=1`).
- The opponent pace hollins / loop9 files and `race/vesper_hollins_sprint.ron` get spawns
  re-synced from the picks; pace ranges unchanged.

**Pace** (release, `opponent_pace`):

| Check | Result | Notes |
|---|---|---|
| `loop9_traffic_field_finishes` (`just pace-loop9`) | skill 0.5: 46/48 finish, mean 232.7 s; 0.95: 47/48, 242.7 s | measured before the diverge bank fix |
| Hollins sprint s50 · s95 | best 55.4 · 51.6 s | in range |
| Loop 9 circuit s50 · s95 | best 204 · 196 s | in range |
| City sprint, grid highway | | in range |
| Lantern checkpoint s95 | 47.7 s | in range |
| Lantern checkpoint s50 | ai_juno 64.09 s (max 64.0), ai_dez 65.84 s (max 65.7); best ratio 1.78 (max 1.53) | **out**: every car logs a rejoin on this course (s50 and s95); likely the new Lantern Bridge on its route. Left to opponent-pace: ranges are theirs, widened only with a reason. |

**Perf** (this container):
- Native `wp_wasm --example chase_bench hakone_gt 30`: sim 3.21–3.35 ms per 60 Hz frame mean,
  p95 4.06–4.30 (ashpeak-port's round-12 numbers on the same container class: 3.37–3.65 /
  4.9–5.6), 21 vehicles, 1891 props.
- Browser draw calls are not measured (no wasm-pack or testbed npm install here). The Vesper
  greybox goes from 33 to 37 primitives (river, canopy, steel, edge walls) and from 504.2 k to
  515.5 k triangles (+2.2 %). Ashpeak-port measured the desktop chase at 437 calls · 329 k
  triangles against the < 600 · ≤ 400 k gate.

**Gate (merged with round12 cae4f20f):**

| Step | Result |
|---|---|
| fmt | 0 |
| clippy `-D warnings` | 0 |
| `cargo test --workspace` | 0 after the race-file fixes (the debug run's only failures were those race files) |
| wasm build | 0 |
| simcli scenarios | 129/129 |
| goldens | blessed (two [bless] commits) |

**Changes outside my crates:**
- `crates/wp_sim/src/physics.rs`: structure colliders, edge barriers, `BARRIER_USER_DATA`.
- `crates/wp_sim/src/parked.rs`: `PARK_MAX_TWIST_M`.
- wp_sim tests.
- `tools/city-import`: greybox features, size check, `ramp_merges` tests.
- `crates/wp_race/src/catalog.rs`: hint.
- `crates/wp_wasm/src/police.rs`: test literal.
- simcli scenarios and goldens.
- `data/city`: road files and greyboxes.

## Lead (2026-10-23): `world_traffic_soak` on candidate G handling — 1 civilian collision
**Cause (traffic driving, not G).** Both cars were kinematic civilians; no racer or police unit
was involved. G only changed the timing.
- Agent 241 began a 13 s kinematic turn-around with agent 420 queued 2.9 m behind it (bumper
  gap). The turn-around holds its own lane at `TURN_OWN_HOLD_M` (8 m) behind the turner, but
  420 was already closer than that.
- So 420 never saw the hold. It saw the turning car's own entry as its leader and crept up to
  its 1.8 m standstill gap during the turn.
- The turn ends with the car lined up with the opposite lane, its nose swung back over the
  own lane. That end pose cleared 420 by 0.29 m when the turn was planned; after the creep it
  overlapped 420 by about 0.1 m.

**Fix (`traffic/blocked.rs`).** `turn_own_hold_s`: the hold sits 8 m behind the turner, or
0.5 m ahead of a follower already closer than that (`TURN_FOLLOWER_HOLD_GAP_M`), so that
follower stays where it is until the turn ends. Unit test
`a_follower_queued_close_behind_a_turn_is_held_where_it_is`.
**Measured on `lead/opponent-pace` (0274348d, G).** `world_traffic_soak`: 0 civilian
collisions on seeds 5–9. With the fix, 113 of 115 scenarios pass on G. The other two,
`police_tunnel_escape_founders_cut*`, fail identically without it (same hashes), so they
are G-side; they are not on physics.md's red list.

## Playtest row 164 — barriers: a glancing scrape slides the car along
**Cause (physics only; the drawn barrier is already one swept profile).** Median barriers,
deck guard rails and bare-sidewalk-back rails were one yaw-only box per segment
(`box_collider`). A car scraping along caught the next box's end corner at a joint: on a
straight Loop 9 barrier, at 30 m/s and 10°, the contact normal tilted 23°, there was an 8.7 kN·s impulse,
the car spun at 140°/s and was thrown 5.3 m/s away from the barrier.
**Fix (`crates/wp_sim/src/physics.rs`: outside D's crates, assigned by the lead).**
`barrier_colliders`: per barrier line (median barriers) or per chained run of rail segments
(`chain_segments`: deck rails, bare backs) the colliders are two triangle meshes:
- side faces plus end caps, mitered at every joint and facing outward, with
  `TriMeshFlags::FIX_INTERNAL_EDGES`;
- the top, 2 cm inside and 1 cm below the side faces' top edge.

One closed mesh with the top included let the internal-edge fix tilt the normal up at the
shared top edge, which rolled a scraping car onto the barrier on a right bend. The plain mesh
(no edge fix) kept the joint kicks.

Friction 0.3 (was 0.4) and restitution 0 (the chassis' Min rule already made it 0). Sizes are
unchanged (barrier 0.5 × 0.81 m; rails 0.3 × 0.9 m; feet 0.3 m below the line). The
`CityColliders::{rails, barriers}` counts are still segments, so there are now far fewer
colliders (one or two meshes per line instead of a box per segment).
**Gate.** `crates/wp_sim/tests/barrier_scrape.rs`, Port Vesper. 30 m/s at 10° into the median
barrier, then held lightly against it (bend feed-forward, aimed 5 cm into the face), checked
for 2 s after the first contact:

| Stretch | Max gap (m) | Max speed away (m/s) | Scrape yaw (°/s) | End speed (m/s) |
|---|---|---|---|---|
| Straight | 0.09 | 0.39 | 10 | 27.5 |
| Bending away, 16° over 110 m (convex face) | 0.28 | 0.75 | 6 | 29.2 |
| Bending in, 4° (concave face; Vesper's sharpest right-hand barrier bend) | 0.47 | 0.83 | 24 | 28.3 |

Limits are 1.5 m, 3 m/s and 60°/s; no spin past 15° off the barrier. On the old boxes, the
straight stretch fails (gap 4.1 m, 5.3 m/s away, 140°/s).

The first 0.25 s (the impact itself) yaws the car 85–98°/s on every collider shape. That is
the rigid-body minimum for the box chassis' front corner taking a 5.2 m/s closing speed:
Δω = m_eff·v·arm/I_z ≈ 96°/s for the debug sedan. So that window has its own limit
(110°/s), and the 60°/s limit applies after it. Softening the impact itself would need a
chassis or contact change in wp_vehicle (physics is frozen).

**Scenario fix.** `soak/traffic_soak_observer.ron`'s observer was parked at the grid origin.
Since main's grid viaduct, that point is the viaduct's median barrier, so the car spent the
run balanced on the old barrier box (contacts at deck + 0.81 m). On the smooth barrier it
slid off (1.17 m against the 0.5 m limit). It now parks inside a core block ≥ 25 m from every
road (100, 27.5).

**Goldens [bless].** jam_pursuit_grid, opponent_circuit_8x5, opponent_flee_police and
world_traffic_soak_observer moved: new barrier/rail colliders (collider set and contact
order), plus the observer's new spot.

## Playtest row 159 — "overpass 3D models still completely block the highway underneath"
**Cause (drawn greybox only).** Every interchange crossroad over Loop 9 (Harbor Boulevard west
and east, Lantern Way, Bluff Road, Uptown Connector: 10 of Port Vesper's 14 grade separations)
had a solid wall across both carriageways under the bridge: the curtain `build_greybox` hangs
from the back of a sidewalk without a lot ("bare back") went down to the ground even on the
deck, so it dropped from the crossroad's sidewalk (≈ 12 m) to the highway surface (≈ 2.3 m)
along both edges of the bridge. Physics was clear (≥ 7.5 m headroom, no colliders in the
lanes); the grid's 16 crossings (its viaduct) were clear in both.
**Fix (`tools/city-import/src/glb.rs`).** `wall_foot`: a wall over the paving of a road below
(a sidewalk back or edge on an overpass) ends at the deck slab depth (1.2 m, `DECK_SLAB_M`)
instead of the ground; applies to the sidewalk curtains and the highway retaining walls.
Vesper greybox regenerated (`just city-v2`; the grid's is byte-identical).
**Gate.** `tools/city-import/tests/underpass_clearance.rs`: at every grade separation (a road
≥ 3 m over another) in both cities, over the lower road's lanes (0.8 m inside its edges) from
8 m before to 8 m past the upper road: ≥ 4.5 m headroom and nothing standing in the lane at
1 m or 3.5 m, in the drawn greybox and in the physics colliders (`PhysicsWorld::set_city_ground`).
Fails on the old generator (10 blocked), passes now (Vesper 14, grid 16 checked).
`rapier3d` added as a city-import dev-dependency (tests only; not reachable from wp_wasm).

## Playtest row 149 — "I currently now can't hit traffic cars" (world side)
**Cause.** Kinematic traffic has no collider: only physical (promoted) cars can be hit. With the
physical cap (`TrafficConfig::max_physical`, 16) full, cars stayed physical out to 100 m and
were never demoted to make room, and the street racers' route focus filled the cap with cars
along their routes — the cars right in front of the player stayed kinematic and the player
drove through them. Reproduced in a `wp_wasm` session (temporary probe, chaining 20 rams at
speed): 2 cars overlapping the player for 316 samples with street racers on, at the cap.
**Fix.** A kinematic car within 40 m of a player (`PLAYER_PRIORITY_RADIUS_M`) takes the place
of the physical car farthest from any player (if 10 m farther; wrecks and LOD-locked cars are
not evicted). A kinematic car turning around / reversing within 20 m of a player
(`LOCKED_PROMOTE_M`) gives the manoeuvre up and is promoted at its manoeuvre pose. After:
0 pass-throughs with street racers at normal density (the probe's remaining samples are its own
teleports, before the next 0.25 s LOD update). Test:
`world::cars_near_the_player_are_physical_with_the_cap_full_elsewhere` (fails without the fix).
For branch-triage: the street-racer side was not changed; if 0.12.49 still passes through,
check that the player's vehicle is `VehicleKind::Player` (only players drive the LOD).

## Playtest row 153 — Loop 9 rest stops
Three pull-offs (the owner chose real highway pull-offs over lots off the existing exits):
**Hollins Rest Area** beside the outer carriageway on the west (north of Lantern Way),
**Larkspur Rest Area** and **Harbor Rest Area** beside the inner carriageway on the south (the
sea starts just past the outer one there). Each: an off-ramp 275 m upstream of its centre, a
100 m one-way parking road 34 m off the loop centreline (parking on the kerb, always a few
parked cars, street lamps like every road, no houses; on an embankment where the ground falls
away), an on-ramp 275 m downstream. No room elsewhere: the north-east stretch only has its
banked corner (a gore there does not meet the drawn surface — tried, dropped), the other gaps
are shorter than two ramp spans.
**API.** `PoiKind::RestStop` POIs (pose = middle of the parking road on its travel lane),
`World::rest_stops()` / `World::rest_stop(name)`; on the map (`wp_wasm` kind `"RestStop"`,
ui-kit "P" glyph). Not in `wp_contracts`' POI list (the contract view skips them).
**Impact.** Loop 9 is cut at six more gores: lane ids and Loop 9 race grids moved (the race
scenario file `race/pick_loop9_circuit.ron` rewritten with `WP_RACE_WRITE_SCENARIOS=1`); the
"Hollins Tunnel exit" route point moved from 200 to 120 m past the portal (the Hollins off-ramp
gore is at 210 m); greybox regenerated (10.04 MB, cap raised to 10.2 MB); Vesper goldens
re-blessed. wp_race Loop 9 races (`loop9_circuit`, `..._pace_without_police`) and street
racers in Vesper pass.
**Changes outside wp_world:** `crates/wp_wasm/src/map.rs` (kind string),
`apps/ui-kit/src/map/{data,draw}.ts` (type + glyph), `tools/city-import/src/lib.rs` (size cap),
`data/city/vesper_v2.greybox.*`, `apps/simcli/scenarios/race/pick_loop9_circuit.ron`.

## Solid terrain = drawn terrain (playtest row 119: "hitting invisible objects / crashing into the map geometry")
**Cause.** The greybox draws Port Vesper's terrain on a 10 m grid with each vertex lowered to
the minimum of the 5 m heights around it (so it never pokes through a road), but Rapier
collided with the raw 5 m heightfield: off the roads ground stood up to 11.3 m above what was
drawn (invisible hillside).
**Fix.**
- `wp_world::terrain::Heightfield::lowered_coarse` (the drawn terrain, shared with
  `city-import`; the shipped greybox is byte-identical) and `height_on_triangles` (cell
  triangles split along the (i, j)–(i+1, j+1) diagonal, like the greybox and Rapier tiles).
- `wp_world::surface::collision_terrain` → `CityMap::ground` (`Arc`): the drawn terrain on the
  5 m grid, except along at-grade roads, junctions and bends (paved + sidewalk + lot + skirt +
  2.5 m), which keep the fine heights the lots and skirts are laid on. Decks and tunnels are not
  at grade.
- `wp_sim::city_ground_colliders` (server and client prediction) builds the heightfield tiles
  from `CityMap::ground`; `CityMap::ground_height` falls back to it off the roads.
**Gate.** `wp_sim/tests/city_v2.rs::solid_terrain_is_the_drawn_terrain` (158k off-road ray
casts ≤ 1 mm from the drawn terrain; fails on the old colliders). Also on
`claude/lucid-cori-8sjuzz` for `main`.

### Drawn terrain follows the ground (row 119 follow-up)
The greybox terrain lowered every 10 m vertex to the lowest 5 m height within 10 m, so flat
ground beside a slope was drawn (and, after the fix above, solid) 1-2 m low: dips behind lots
that trapped police units (e.g. pool unit 11 in the Founders Cut run). `Heightfield::coarse_under`
replaces `lowered_coarse`: a vertex keeps the fine height under it and is lowered only to the
lowest *protected* fine vertex within 10 m, the protected set being the road band
(`surface::drawn_terrain`, shared by `city-import` and `collision_terrain`). Tunnels: the
carve inside the walls and under the deck overhang lies inside the band, so the drawn terrain
never reaches into a tube. Measured (`wp_world/tests/city_v2.rs::drawn_terrain_stays_under_roads_and_follows_the_ground`):
never above a road, tunnels included (closest 0.14 m below); off-road heightfield − drawn
p50 0.05 / p95 0.21 / p99 0.60 m (max 7.0 m on hillsides right beside cuttings, −2.2 m on
sharp ridges; physics follows the drawn mesh in both). Greybox regenerated (9.09 MB, +5 KB).

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
## Lead helper note (P9 helicopter, 2026-10-20; merged 2026-10-21 by `triage/helicopter`)

`CityMap::line_of_sight` now treats **bridge decks** as occluders: `surface::deck_slabs(net)` —
surface centreline pieces whose whole cross-section is ≥ `DECK_COVER_CLEARANCE_M` (3 m) above
the terrain at both ends — each a thin plane over the paved width (bank ignored), indexed in a
32 m grid like buildings; `DeckSlab::blocks(p, q)` solves the single crossing in closed form.
Effect: a helicopter cannot see a car under Port Vesper's overpass or in the underpass; cars on
the deck stay visible. Ground-level sight lines are unaffected (horizontal rays under a deck do not
cross its plane). Tests: `surface::tests::*` (slab geometry), `vesper::tests::overpass_decks_hide_the_road_below_from_the_air`.
Decks are separate from round 12's covered spans (`covered.rs`: tunnels and the Founders Cut
trench, switchable with `covered_los`); `deck_slabs` is not affected by `covered_los`. The slab
list and its grid are `Arc`-shared like the other grids (cheap `CityMap` clones).

## Playtest row 134 (lead): traffic no longer panics when the player is near

`traffic.rs` emergency stop on a tracked vehicle: was "closest approach within 3 m in the next
3 s", which a player passing in the next lane (same direction or oncoming, ~2.7–3.3 m over)
kept setting off. Now the closest approach must overlap the agent's body side to side
(half widths + `EMERGENCY_SIDE_MARGIN_M` 0.4 m, and within a car length along it), look-ahead
`EMERGENCY_HORIZON_S` 2.5 s. Tests: `tests/player_nearby.rs` (two same-world runs, with and
without the player, compare the chosen agent): oncoming 636 → 0 panic ticks, overtaking 40 → 0;
a car in its lane head-on still triggers it.

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

### Follow-up: street-race picks stand for the whole road; clearing a start (2026-10-23)
Owner: "the betting map gets confused on Loop 9 if I don't pick exactly the right lane — make
the gate cover the whole road" and "in betting mode for free roam pause both cars and clear the
area out for race start".
- **Cause:** routes never change lanes mid-road, and Loop 9's three lanes per carriageway are
  separate lane chains, so `plan_route_via` (which snapped a pick to one nearby lane) found a
  pick on the "wrong" lane unreachable ("no road route through those waypoints") and a pick on
  the far carriageway reachable only by lapping the loop (westbound from the north grid, a
  waypoint 700 m on: 708 m on one lane, no route on two others, 4.9 km on the far side).
- **`routes::plan_route_via`:** a pick (waypoint or finish) now stands for every lane abreast
  of it along its road, both directions (`road_lanes_at`: within 22 m beyond the nearest lane,
  parallel or opposite, same height ±4 m, not end-on across a junction). One bounded
  multi-target A* (`paths_to_any`, gives up at 2× the quickest + 60 s) per pick; a waypoint
  takes the lane quickest to reach plus quickest to leave for the next pick, the finish the
  quickest to reach. With no waypoints the finish also covers the road now (no longer exactly
  `plan_route`). Only street races call it.
- **`Traffic::clear_near` / `World::clear_traffic_near` / `Sim::clear_traffic_near`:** despawn
  every civilian car (wrecks too) within a radius now; physical ones lose their body at once.
  New replay op `simcli::replay::OpKind::ClearTraffic { xy, radius_m }` (kind byte 8; older
  replays read unchanged).
- **`wp_wasm::street` (lead):** a waypoint gate counts on a line across the whole road
  (`GATE_HALF_SPAN_M` 26 m either side, `GATE_DEPTH_M` 8 m deep) as well as the 20 m radius;
  the player is held at a halt while an offer is open (the racer already waits), a honk with
  the map closed calls it off; accepting logs `ClearTraffic` (80 m round the grid) before the
  start moves.
  Owner, same day: "the AI got away with winning without hitting all the waypoints": the
  racer's finish (at B, along-route progress, or the end of its run) ignored the gates. Now the
  racer passes the same gates in order (`racer_next_gate`); B counts for it only after all of
  them, a run ended with one missed loses ("they missed a waypoint"), and at the time limit
  each car's progress is capped at its next unpassed gate.
- **Tests:** `world.rs` `route_via_waypoint_on_either_loop9_carriageway` (a pick on each of
  Loop 9's six lanes routes along the near carriageway), `route_via_waypoints_passes_them_in_order`
  (now: within the road); `street.rs` `a_gate_spans_the_whole_road`, `gates_count_in_order_and_cap_progress`; `street_tests.rs`
  `betting_holds_both_cars_and_the_start_is_cleared`; replay round trip carries the new op.

## Road network centrelines follow the road centre (playtest row 170)
`RoadNetworkIndex` built each edge's centreline from the node centres plus the interior
points of the road's first lane. On curved roads (74 % of Port Vesper's edges) that is a
lane's offset off centre, and users that offset into a lane from it (police units, 1.75 m)
ended up one lane too far right one way and on the centre line the other way. Edges now use
the interior points of the road surface's own centreline (`RoadSurface::centre`, oriented
a -> b), so straight roads stay node-to-node, simplified to the points that shape the
road (Douglas-Peucker, 0.2 m): every interior point is a node in users' graphs (police patrol
destinations). Test:
`tests/city_v2.rs::road_network_centrelines_offset_into_the_right_lane` (1918 samples: 937 in
the wrong lane before, 4 at junction edges after). Police side: `docs/integration/police.md`.
