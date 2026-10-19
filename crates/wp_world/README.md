# wp_world

**Owned by workstream 05 (city & traffic).** Branch `ws/world`.

Road network, signals, civilian traffic, density, violations, race routes and the day/night
clock. Implements `wp_contracts::WorldQuery`. No renderer dependency; compiles for native and
`wasm32-unknown-unknown` (deps: `serde`, `serde_json`, `postcard`, `thiserror`, `wp_vehicle`).

| Module | What |
|---|---|
| `network` | The road file format (`.road.bin` / `.road.json`), validation |
| `builder` | Abstract road graph → lanes, connections, junction control, signal plans |
| `procgen` | Procedural fallback city (grid + ring + highway spine + 2 diagonals) |
| `map` | Runtime map: connector curves, conflict sets, uniform-grid index, `WorldQuery` (incl. building line of sight) |
| `signals` | Fixed-time controller evaluation |
| `traffic` | Civilian agents: IDM, MOBIL, junction permits, hazards, spawning, LOD |
| `density` | Density model (`f_time`, `f_distance`, district bases) |
| `violations` | Speeding / red light / wrong way / reckless / collision detection |
| `routes` | Lane-legal race routes, circuits, checkpoints, closures |
| `props` | Parked cars and cones (static colliders) |
| `clock` | `WorldClock`, `sun_direction` |
| `geom` | Deterministic geometry (polylines, OBB, Bézier; re-exports `wp_math::{atan, atan2}`) |
| `roads` | Road-level graph view (`RoadGraphView`) for police routing / minimaps |

City data lives in `data/city/` (see its README); importer and generator in
`tools/city-import/`.

## Quick start

```rust
let mut world = World::load("data/city/procedural_12x12.road.bin", WorldConfig::default())?;
world.set_traffic_car(&wp_contracts::load_car("data/cars/debug_sedan.json")?); // near LOD
// every physics tick:
world.set_tracked_vehicles(&[TrackedVehicle::from_state(id, &state, siren, true)]);
world.step(PHYSICS_DT, &mut events);           // emits SimEvent::TrafficViolation
for change in world.drain_lod_changes() { /* create / remove rigid bodies */ }
for agent in world.agents() { /* render / replicate AgentView */ }
```

## Tests

```sh
cargo test -p wp_world                                   # unit + integration (~15 s debug)
cargo run -p wp_world --example load_profile             # city build/load timings
cargo test -p wp_world --release -- --ignored --nocapture  # 10-min soak ×2 + perf budget
cargo run --release -p wp_world --example traffic_bench -- 10 400 12 1 0.05
                                         # minutes agents hour seed aggressive_fraction
```

Integration tests (`tests/world.rs`): signal plans never give conflicting protected
movements or crossing approaches green together, and every group's green time per cycle equals
its plan; road format round trip; shipped `data/city` files equal a fresh generation (stale
check); `WorldQuery` surfaces/limits/signals; 400 agents for 1 min (10 min ignored) with zero
civilian–civilian overlaps; aggressive drivers enabled stay under 1 % (recorded: **0** in all
runs, see Perf); density per district within 15 % of target after warm-up; noon core ≥ 5×
03:00 core (targets and spawned counts); sightline/active-area spawning; determinism hash
(same seed equal, different seed differs, physical LOD exercised); stopping behind a stationary
car and yielding to sirens; head-on emergency braking; speeding, wrong-way, red-light,
collision violations; seamless LOD promotion/demotion (pose jump < 0.6 m, speed continuous,
physical cars stay on the road); routes, circuits and closures; parked cars.

## Road format (`data/city/<city>.road.bin` + `.road.json`)

Binary: `b"WPRD"`, little-endian `u32` format version (`ROAD_FORMAT_VERSION` = 1), then the
[postcard](https://docs.rs/postcard) encoding of `RoadNetwork`. The `.road.json` debug dump is
the `serde_json` encoding of the same struct and loads identically (`RoadNetwork::load` picks by
extension). All coordinates are metres, +X east, +Y north, +Z up. Ids are `u32` indices.

- `id`, `source`, `attribution` (must be credited when present — OSM data is ODbL),
  `bounds_min/max`, `core` (density falloff centre).
- `nodes[]`: `pos`, `intersection` (index if the node is a junction).
- `lanes[]`: directed, **right-hand traffic**. `polyline` runs stop line → stop line in the
  travel direction (≥ 2 points); `from_node`/`to_node`; `road` (both directions of one source
  road share it); `kind` = `Through | TurnLeft | TurnRight | Bus | Parking | Alley | Highway`
  (`Bus` and `Parking` are not driven by civilians); `index_from_left` (0 = next to the centre
  line); `width_m`; `speed_limit_mps`; `left`/`right` same-direction neighbours (lane-change
  targets); `edge_left`/`edge_right` (the side is the edge of the paved ribbon → curb);
  `connections[]` = allowed movements at the far end (`to` lane, `turn` =
  `Straight | Left | Right | UTurn`). Empty connections = the lane leaves the map.
- `intersections[]`: `node`, `control` = `Signal{controller} | AllWayStop | Priority |
  Uncontrolled`, `incoming[]` (`lane`, `signal_group`, `major` for priority junctions),
  `allow_rtor`, `radius_m` (junction box).
- `signals[]`: fixed-time plans: `intersection`, `groups`, `offset_s` (green waves),
  `phases[]` (`green_groups`, `green_s`, `yellow_s`, `all_red_s`); cycle = Σ phase durations.
  A group green in consecutive phases stays green through the transition.
- `zones[]`: `kind` = `Core | Commercial | Residential | Industrial | Port | Suburb`,
  `density_base` (peak vehicles per lane-km), `polygon`.
- `pois[]`: `kind` = `SafehouseCandidate | Garage | RaceStartGrid | PoliceStation | Helipad`,
  `name`, `pos`, `heading_rad` (yaw, 0 = +Y), nearest `lane`.
- `buildings[]`: `footprint` polygon + `height_m`.

Derived at load (`CityMap::new`), not stored: junction connector curves (cubic Bézier, tangent
continuous), per-connector conflict sets (same target lane or swept paths closer than 2.4 m),
sibling divergence distances, approach info per lane, drivable lane-km per zone and a 32 m
uniform grid over segments, junction boxes and zones for `WorldQuery` lookups.

### Builder rules (procgen and OSM share them)
- Junction setback per road end from both road half-widths and the junction angle
  (`d = (h_j + h_i cos θ) / sin θ` + 2 m crosswalk), so skewed (diagonal) junctions get larger
  boxes; degree-2 nodes are continuations, degree-1 nodes get U-turns.
- Default lane use (no `turn:lanes`): leftmost lane Left+Straight, rightmost Right+Straight,
  middle Straight; bus lanes Straight+Right. Left turns map left-aligned, others right-aligned.
- Control: signals when every road is collector or above, at 5+-way junctions, or when tagged;
  priority (higher class has right of way) when classes differ; all-way stop otherwise.
- Signal plan: opposite approaches share a phase; green 14–30 s by class; yellow
  `clamp(1 + v / 6.1, 3, 5)` s (ITE); all-red 1.5 s (2 s above 20 m/s).

## Procedural city (`gen-city --grid 12x12 --seed N`)
Grid of streets at 110 m centred on the origin. Rows/columns 2 and 9 are 2+2 arterials
(40 mph), the two central rows/columns collectors (30 mph, parking), the rest local streets
(25 mph, parking). A 3+3 highway spine (55 mph) runs north–south between the central columns
and crosses only arterial/collector rows at signals; other rows end at it. Two 3+3 diagonal
arterials with kerbside bus lanes (35 mph) run corner to corner through grid nodes (6-way
signalised junctions) and stop at the highway block. An octagonal 2+2 ring road (45 mph)
collects every street end (priority T-junctions with local streets). Mid-block alleys
(15 mph) in core blocks. Zones per block by radius (Core < 200 m, Commercial < 380 m, else
Residential, Industrial in the south-east) plus outer bands (Industrial south, Port east,
Suburb north/west). Buildings on lots (Core 5–8 storeys, Commercial 2–4, Residential 1–2 with
courtyards, warehouses in the bands). Green-wave offsets along arterial rows (eastbound),
columns (northbound) and diagonals at 90 % of the limit. POIs: 2 police stations + helipad,
3 garages, 4 safehouse candidates, 3 race grids (highway, arterial, ring). The 12×12 city has
1734 lanes, 224 junctions (58 signalised), 1144 buildings, ~104 drivable lane-km.

## Traffic AI

**Agents** live on segments (lanes or junction connectors) with arc length `s` (car centre),
speed `v`, lateral offset and a two-segment plan. Kinematic agents integrate at 60 Hz
(`TRAFFIC_DT`) inside `World::step` (called at 240 Hz).

**Personalities** (sampled per spawn): 70 % law-abiding (0.93–1.0× limit, headway 1.6–2.0 s,
s₀ 2 m, comfortable decel 2.5 m/s², politeness 0.5, accepted gap 5 s), 25 % casual
(1.07–1.12× limit, 1.4–1.6 s, 3.0 m/s², 4.5 s), 5 % aggressive (1.15–1.22×, 1.2–1.3 s,
s₀ 1.2 m, runs yellows, politeness 0.05, 3.5 s gaps).

**Longitudinal (IDM)** against the nearest of: car ahead on the own segment or the planned
ones (incl. a car on a diverging sibling connector whose rear has not cleared the shared
section), the old-lane leader during a lane change, tracked vehicles (players/police) in the
path, the stop line when the agent has no junction permit, debris it cannot swerve around.
Desired speed = min(limit × factor, curve speed of the current and next connector — lateral
2.8 m/s² — reached with comfortable braking). Above the desired speed the IIDM free term is used
(never harsher than comfortable).

**Junctions**: an agent may enter a connector only with a **permit**, requested once it is
first in its lane and within comfortable stopping distance of the line. Granting requires:
- the control rule — green (left turns permissive), yellow (law-abiding go only if they cannot
  stop at 2.5–3 m/s², aggressive go if they reach the line before red, a left-turner waiting at
  the line goes on yellow = "sneaker"), red (right-on-red after a 1 s full stop; a car that
  cannot stop at 7 m/s² goes), all-way stop (full stop, then first-stopped first, ties by id),
  priority (major goes; minor stops then yields);
- every conflicting connector empty, unreserved and not occupied by a tracked vehicle;
- room on the target lane (don't block the box);
- lower-ranked movements (permissive left < protected; minor < major; right-on-red < green)
  accept a gap: no higher-ranked conflicting car arrives within the driver's accepted gap
  (upstream search 2 segments deep; cars that will stop for yellow/red are ignored).
The permit reserves the connector until the agent leaves it. Permits are revoked if the light
changes and the agent can still stop comfortably, a siren appears, or a tracked vehicle blocks
the box.

**Lane changes (MOBIL)** every 0.5 s per agent on multi-lane roads, ≥ 15 m into the lane and
≥ 60 m before its end: safety (new follower ≥ −4 m/s², gaps ≥ s₀), incentive
`Δa_self + p(Δa_new + Δa_old) > 0.25` with a 0.3 m/s² keep-right bias. The car slides over at
1.3 m/s and keeps a ghost in its old lane until clear.

**Hazards**: emergency braking (−8 m/s², hazard lights) when a tracked vehicle in front or
alongside has time-to-closest-approach < 3 s with miss distance < 3 m at > 5 m/s relative
speed (head-on or crossing); sirens within 60 m → pull 1 m right, slow to 3 m/s at up to
4 m/s², stop if the siren is within 25 m behind, never enter a junction; debris ahead → swerve
up to 1.5 m and slow to 60 % of the limit, or stop if it cannot be passed. Civilians never chase
or block players on purpose.

**LOD**: within 80 m of a player or an active race route (max 16), agents become physical:
a `wp_vehicle::Vehicle` with `AssistProfile::ai()` stepped every physics tick, driven by pure
pursuit (look-ahead 5–25 m) for steering and a PID on the IDM-commanded speed profile.
Promotion copies pose and speed; demotion (beyond 100 m) projects the vehicle back onto its
path. A physical agent that leaves the road (> 6 m or facing away) or is wrecked
(`apply_agent_damage` with a dead engine / lost wheel) is disabled (stopped, hazards) and is
removed once out of sight. The sim loop gets `LodChange::{Promoted, Demoted, Despawned}`.

**Density & spawning**: target per zone = Σ over its lane samples (10 m) of
`lane_km · density_base · f_distance(d_core) · f_time(time, district)` × `density_scale`,
scaled down so the active area never exceeds `max_agents` (400). `f_distance =
max(0.2, 1 / (1 + (d/600 m)²))`; `f_time` = hourly piecewise-linear curves with commute
bumps (07–09, 16–18) for Residential/Commercial/Suburb, shift peaks for Industrial, a deep
01:00–05:00 trough, and Core nightlife until ~02:00 (noon/03:00 = 10×). The active area is
400 m around each player and around active route samples; with no players and no routes the
whole map is active (headless tests). Every 0.25 s: despawn out-of-area, closed-lane, stuck
(> 120 s) and surplus agents (district total > target + 4 % or zone > 1.5× target), spawn into
zones with deficits on free lane spots (≥ 12 m gaps). **Sightline rule (prototype)**: nothing
spawns or despawns within 150 m of a player's current position or of where it will be in 1 s.

**Violations** (tracked vehicles, 60 Hz): speeding = over the limit by more than
max(2.24 m/s, 10 %) for > 1 s (re-emitted every 5 s); red light = crossing the stop line of the
lane you were on while it is red; wrong way = on a lane pointing the other way (and no lane
your way) above 3 m/s for > 1.5 s; reckless = 3 near misses (boxes within 1.5 m at > 8 m/s
relative) within 10 s; collision = box overlap with a civilian. Emitted as
`SimEvent::TrafficViolation`.

**Line of sight** (`WorldQuery::line_of_sight`, contract v0.2.0): buildings are footprints
extruded from the ground to `height_m`; a sight line is blocked if any part of it inside a
footprint is below the roof (checked at both ends of the inside span — height is linear along
the line). Buildings are indexed in the 32 m grid and only cells the line crosses (2D DDA) are
tested. Roads, parked cars and traffic do not occlude.

**Road graph** (`World::road_graph()`, `roads::RoadGraphView`): one edge per road with class,
limit, lanes each way, district; nodes with district and signal flag; POIs; building bounds.

**Routes**: `plan_route` / `plan_route_between_pois` (A* over lanes and connectors, travel
time at the limit, start snapped to a lane facing the POI heading), `plan_circuit` (visits a
polygon's vertices in order and closes the loop), checkpoints at every turn and every 200 m
(gate width = carriageway), `closure_for_route` (closes both directions of every road on the
route and puts 3 cones across every side-street stop line). `World::set_active_routes` makes
routes LOD/spawn focus areas and applies closures.

**Determinism**: no wall clock, no `f64`, `BTreeMap`/`Vec` only, a single seeded
`wp_math::Rng` used in a fixed order, polynomial `atan`/`sin_cos`, IEEE `sqrt` only. Time is
integer ticks: the world counts physics ticks (`World::tick`, signals use it via
`signals::group_state(sc, group, tick)`), traffic and violations count 60 Hz traffic steps
(`TICKS_PER_TRAFFIC_STEP` = 4). `World::state_hash` covers clock, tick and every agent (pinned
by `determinism_hash`).

## Perf
Release, cloud container (4 vCPU), procedural 12×12, 400 agents at saturation
(`density_scale` 4), noon, aggressive 5 %:

| Measurement | Native | Browser | Budget |
|---|---|---|---|
| Traffic + signals + violations per 60 Hz frame (4 × `World::step`) | avg 0.06 ms, p99 0.15 ms, p99.9 0.3 ms, max ≈ 1.4 ms (noise) | not measured | 2 ms (browser 4 ms) |
| Same with civilian-collision diagnostics on | avg 0.20 ms | — | — |
| `line_of_sight`, random 300 m sight lines at eye height | ≈ 1.1 µs/query | — | — |
| `World::load` of `procedural_12x12.road.bin` (decode + `CityMap::new` + parked cars) | ≈ 17 ms (debug build ≈ 0.2 s; `ProcGenConfig::small()` ≈ 5 ms / 0.06 s) | — | — |
| 10-min soak, 9 seeds × {0 %, 5 %} aggressive | 0 civilian–civilian collisions; 0–11 agents recycled after being stuck > 120 s (starved permissive lefts / minor approaches in saturated flow) | — | < 1 % |

Physical agents use the `wp_vehicle` stub (negligible); re-measure when prompt 02 lands.

## Known gaps
- Physical agents are `wp_vehicle` instances, but `wp_sim` does not yet create rigid bodies /
  damage models for them (needs `LodChange` handling in the sim loop, see
  `docs/integration/world.md`). Until then physical traffic cannot be hit.
- Under saturated flow a few left-turners / minor-road cars can starve for > 2 min; they are
  recycled when unseen. No protected-left phases yet.
- Spawning uses one traffic car definition (no per-district vehicle mix yet).
- Agents do not react to other civilians' hazards (only tracked vehicles and debris).
- Sightline rule is distance-only (no building occlusion).
- Lane change across a solid line / bus lane rules not modelled; buses not simulated.
- Elevation: kinematic agents follow lane polylines' z, the stub vehicle keeps its spawn z.
