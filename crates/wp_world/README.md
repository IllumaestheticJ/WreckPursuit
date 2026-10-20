# wp_world

**Owned by workstream 05 (city & traffic).** Branch `ws/world`.

Road network, signals, civilian traffic, density, violations, race routes and the day/night
clock. Implements `wp_contracts::WorldQuery`. No renderer dependency; compiles for native and
`wasm32-unknown-unknown` (deps: `serde`, `serde_json`, `postcard`, `thiserror`, `wp_vehicle`).

| Module | What |
|---|---|
| `network` | The road file format (`.road.bin` / `.road.json`), validation |
| `builder` | Abstract road graph → lanes, connections, junction control, signal plans |
| `procgen` | Procedural fallback city (grid + ring + highway spine + 2 diagonals), flat |
| `vesper` | Port Vesper v2 (`vesper_v2`): curved streets, hills, terrain, banked highway loop, grade separations |
| `terrain` | `Heightfield` (stored with the network), value noise, bumps |
| `surface` | Road trimesh, raised sidewalks + kerbs (`sidewalk_mesh`), junction discs, deck rails — one geometry for physics and the greybox; `MapGround` wheel probe |
| `map` | Runtime map: connector curves, conflict sets, uniform-grid index, `WorldQuery` (incl. building line of sight) |
| `signals` | Fixed-time controller evaluation |
| `traffic` | Civilian agents: IDM, MOBIL, junction permits, hazards, spawning, LOD |
| `density` | Density model (`f_time`, `f_distance`, district bases) |
| `violations` | Speeding / red light / wrong way / reckless / collision detection |
| `routes` | Lane-legal race routes, circuits, checkpoints, closures |
| `props` | Parked cars and cones (static colliders) |
| `clock` | `WorldClock`, `sun_direction` |
| `geom` | Deterministic geometry (polylines, OBB, Bézier; re-exports `wp_math::{atan, atan2}`) |
| `drive` | Shared AI driving: `DriveController` (pure pursuit + PID) and `PathFollower` (`follow_path` along any `DrivePath`) for traffic, police and racers |
| `leader` | Leader / obstacle query (`find_leader`) + shared IDM (`CarFollowing`) so police and racers follow and stop behind vehicles like traffic does |
| `roads` | Road-level graph view (`RoadGraphView`) for police routing / minimaps; `RoadNetworkIndex` (contract `RoadNetworkQuery`, grid-indexed `nearest_on_road`) |
| `manoeuvre` | Low-speed manoeuvres: k-point turn planner (`plan_turn`), closed-loop `TurnDriver` / `ReverseDriver` for physical cars (reverse gear through the automatic box), `sight_clear` (crest / bend check), map helpers `road_corridor` / `overtake_sight_clear` |
| `markings` | Painted markings derived from the lane layout (`marking_at` / `painted_at`): `WorldQuery::surface_at` reports `SurfaceType::PaintedLine` on them |

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

Blocked vehicles (`tests/blocked.rs`, playtest row 32): a stopped car in one lane of a 1+1
street is passed in the oncoming lane (or turned away from) with no contact; a car parked across
the lane; a 2+2 arterial (lane changes, never the oncoming lane); a single-lane one-way street
(followers reverse to the junction and re-route); a Port Vesper street; the map helpers; an
ignored perf run with 12 blockers. `tests/drive.rs` drives the real car (Rapier rig) through a
5-leg turn on a 7 m street and a 40 m reverse.

## Road format (`data/city/<city>.road.bin` + `.road.json`)

Binary: `b"WPRD"`, little-endian `u32` format version (`ROAD_FORMAT_VERSION` = 2), then the
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
- `buildings[]`: `footprint` polygon, `base_z_m` (lowest ground under it), `height_m` (roof =
  `base_z_m + height_m`).
- `surfaces[]` (v2): the paved ribbon of each source road between its junction setbacks:
  `road`, trimmed `centre` polyline (3D), `bank[]` per point (cross slope, dz per metre to the
  right; lanes lie on `centre.z + bank · lateral`), `left_m`/`right_m` paved widths,
  `highway`, `sidewalk`. Physics and the greybox draw these.
- `terrain` (v2): `Option<Heightfield>` — `origin`, `cell_m`, `nx`, `ny`, row-major heights
  (bilinear between vertices). `None` = flat ground at z = 0 (the procedural grid, imports).

Derived at load (`CityMap::new`), not stored: junction connector curves (cubic Bézier, tangent
continuous), per-connector conflict sets (same target lane or swept paths closer than 2.4 m),
sibling divergence distances, approach info per lane, drivable lane-km per zone and a 32 m
uniform grid over segments, junction boxes and zones for `WorldQuery` lookups.

### Painted markings (`markings`)
Not stored in the road file; derived from the lanes, so every city gets them and a renderer
can draw them from the same constants (`wp_world::markings::*`). `surface_at` returns
`PaintedLine` on them before any other rule.

| Marking | Where | Size / pattern |
|---|---|---|
| Centre line | left edge of the leftmost lane of a two-way road | double solid, 2 × 0.12 m, 0.12 m gap |
| Lane line | between same-direction lanes | 0.12 m, dashed 3 m on / 9 m off from the lane start |
| Separator | next to a bus lane or parking strip | 0.20 m solid |
| Edge line | inside the curb edge | 0.15 m solid |
| Stop line | last 0.5 m of a lane that stops (signal, all-way stop, minor approach) | full lane width |
| Crosswalk | 0.2–1.8 m past each lane end / before each lane start at signalised and all-way-stop junctions (inside the 2 m crosswalk setback) | zebra bars 0.5 m on / 0.5 m off across the lane |

Alleys and parking strips are unmarked; highways get centre, lane and edge lines but no stop
lines or crosswalks. Each lane owns its half of a shared line, so answers do not depend on
query order. Lanes more than 3 m above or below the queried z are ignored (grade
separation). About 4 % of road samples are paint on Port Vesper v2 (`painted_share_is_plausible` gates 2–15 % on the 6×6 city).

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

## Port Vesper v2 (`vesper::generate`, `World::vesper`, `gen-city --preset vesper_v2`)
After `docs/design/CITY.md`, scaled to ≈ 3.3 × 2.9 km (`MAP_LO..MAP_HI`), 5 m terrain cells.
- **Layout:** Lantern Row downtown (7×7 grid at 100 m, arterials/collectors, signals);
  Harbor Boulevard curving along the flat south shore; Gantry Row port grid + Terminal Avenue on
  the quay; Canal Flats industrial grid; organic districts as warped grids with curved edges,
  dropped edges and cul-de-sacs (Larkspur west, Mercer north, Eastgate east, Bayview on the
  north-east plateau); Hollins Drive (6 switchbacks up Hollins Knob, ≈ 110 m), Ridge Road back
  down over the crests, the Knob lot; Bluff Road along the plateau edge; Loop 9, a 3+3
  highway loop (filleted control polygon, radius ≥ 420 m sweepers) with 7 signalised
  junctions; Route 40 West (dead-ends at the map edge, passes **under** the loop: overpass)
  and Route 40 North (cut **under** the loop: underpass).
- **Terrain:** natural ground = northward rise + authored hill (`bump`) + plateau + two
  octaves of seeded value noise (damped downtown, none on the port flat) + sea at the south and
  east edges.
- **Profiles:** node heights from the ground, relaxed until every road can meet its grade
  cap; per road, samples every ~4 m follow the (smoothed) ground inside the cones of the
  anchors (both junction approaches flat, deck heights at grade separations), clamped to the
  grade cap (highway 6 %, arterials 8 %, streets 10 %, hill roads 11.5 %) by averaged
  forward/backward passes, then binomially smoothed so a full grade reversal bends no tighter
  than 0.3 g of vertical acceleration at 1.15 × the limit. Junction boxes are pinned flat.
  Banking (highway ≤ 6 %, Bluff Road ≤ 3 %) from curvature, faded out near junctions.
- **Carving:** the heightfield is carved 12 cm under every at-grade surface (full strength
  7.5 m beyond the paved edge, smoothstep blend over 15–22 m) and junction discs; never above
  any road; left natural under bridge decks (explicit crossing decks, or any stretch whose
  whole cross-section is > 3 m above the ground) and kept ≥ 0.5 m under decks.
- **Lots:** buildings along every street side (district-sized lots), clear of all roads and
  dead-end turnarounds, skipped on > 8 m of slope or water; walls start at the lowest ground
  under the footprint.
- **POIs:** race grids `Lantern Run` (first: free drive / city chase spawn), `Loop 9 sprint`,
  `Hollins Ascent`, `Hollins Descent`, `Ridge Road`, `Harbor Boulevard drag`, `Bluff Road`,
  `Loop 9 north`, `Loop 9 west`; 3 precincts + helipad, 3 garages, 4 safehouses (incl.
  `Knob Observatory Lot`).

950 lanes, 133 junctions (27 signalised), 3346 buildings, 2 grade separations, terrain
681 × 611 vertices (−3 … 111 m); generation ≈ 0.12 s release (0.4 s debug), `CityMap::new`
≈ 10 ms. `cargo run --release -p wp_world --example vesper_stats` prints grades, crest/sag g
at the limit and banks per road.

### Sidewalks, kerbs and junction discs (`surface`, playtest #20)
Physics (`wp_sim`) and the greybox (`tools/city-import`) build these from the same functions,
so what you see is what you drive on (`tools/city-import/tests/surface_match.rs` checks it):
- **Sidewalks** (`sidewalk_mesh`): along every edge lane that is not a highway
  (`lane_sidewalks`), from the kerb line out to `SIDEWALK_W_M` = 3 m, `KERB_H_M` = 0.15 m above
  the road edge, continuing the road's bank; vertical kerb faces from the road edge up,
  facing the road. Where a strip would run onto pavement (the crossing road or junction disc
  at a tight/skewed corner) it is trimmed in `TRIM_CELL_M` = 0.5 m cells.
- **Corner and bend fills:** the gaps between sidewalk ends at a node — junction corners, the
  top of a T, the outside of a bend — are filled at kerb height (the area between the two
  strips' kerb and outer lines, extended to where they meet; a least-squares plane through
  the four end points), minus pavement and what the strips cover, with kerb faces towards
  the pavement. Corner kerbs follow the junction disc in 0.5 m steps.
- **Junction discs** (`junction_polygon`, `junction_paved_radii`): a 16-gon whose edges touch
  the circle (not its corners), with a paved radius large enough to reach the corners of
  every road ribbon trimmed at the junction (`sqrt(d² + w²)`). `Intersection::radius_m`
  (lane trimming, junction logic) is unchanged.
- Sizes: Port Vesper 28 k top + 28 k kerb triangles (≈ 46 ms to build, release), the grid
  9.5 k + 11 k.

## Traffic AI

**Agents** live on segments (lanes or junction connectors) with arc length `s` (car centre),
speed `v`, lateral offset and a two-segment plan. Kinematic agents integrate at 60 Hz
(`TRAFFIC_DT`) inside `World::step` (called at 240 Hz).

**Personalities** (sampled per spawn from the traffic RNG; every driver differs within its
personality — owner playtest row 14): 70 % law-abiding (12 % slow drivers at 0.80–0.88× the
limit, the rest 0.88–1.03×; headway 1.6–2.2 s, s₀ 2 m, accel 1.1–1.7, comfortable decel
2.0–2.8 m/s², politeness 0.4–0.7, accepted gap 5 s), 25 % casual (1.03–1.13×, 1.3–1.6 s,
1.5–2.1 / 2.6–3.3 m/s², 4.5 s), 5 % aggressive (1.13–1.25×, 1.1–1.3 s, s₀ 1.2 m, runs yellows,
politeness 0–0.1, 3.5 s gaps). Per road class: highways add −4…+12 % (by personality), alleys
cap at 0.9×. Resulting desired speed / limit over the mix: p5 0.84, p50 0.98, p95 1.13 (highway
p95 1.20). Full table: `DriverParams` docs.

**Dynamic driving**: reaction delay when pulling away — in a queue that starts moving, or first
in line at a green going straight/right — 0.5–1.0 s law-abiding (10 % hesitant: 1.2–2.2 s),
0.35–0.75 s casual, 0.15–0.4 s aggressive (not for gap-accepting moves: the gap would close).
Ambers: law-abiding stop whenever they can at their comfortable decel; casual drivers go when
stopping would need more than 55–90 % of it *and* they reach the line before red; aggressive go
whenever they reach the line. Physical (near-LOD) cars follow the same personality: the
controller tracks the same IDM acceleration.

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
changes and the agent can still stop comfortably (never at a green: a stopped, hesitating car
keeps its permit), a siren appears, or a tracked vehicle blocks
the box.

**Lane changes (MOBIL)** every 0.5 s per agent on multi-lane roads, ≥ 15 m into the lane and
≥ 60 m before its end: safety (new follower ≥ −4 m/s², gaps ≥ s₀), incentive
`Δa_self + p(Δa_new + Δa_old) > threshold` with a keep-right bias, both per driver (threshold
0.15–0.35 / 0.15–0.3 / 0.08–0.15 m/s², bias 0.2–0.4 / 0.15–0.3 / 0.05–0.15 for law-abiding /
casual / aggressive). Discretionary changes are **indicated first** (`AgentView::turn_signal`)
and happen at the next check ≥ 0.5 s later if still chosen (half the threshold then) and still
safe; debris-forced changes go at once. The car slides over at 1.3 m/s and keeps a ghost in its
old lane until clear. Lane choice before turns is implicit: an agent picks its turn from the
lane it is in. `TrafficStats::{lane_changes, lane_changes_left}`.

**Hazards**: emergency braking (−8 m/s², hazard lights) when a tracked vehicle in front or
alongside has time-to-closest-approach < 3 s with miss distance < 3 m at > 5 m/s relative
speed (head-on or crossing); sirens within 60 m → pull 1 m right, slow to 3 m/s at up to
4 m/s², stop if the siren is within 25 m behind, never enter a junction; debris ahead (`Hazard`,
extrapolated along its velocity up to 2 s) → swerve within the lane by up to 1.5 m (planned in
the lane frame, held until the rear bumper is past) at 60 % of the limit; debris that blocks the
lane → change to a free neighbour lane (forced MOBIL change, safety still checked) or stop;
debris in a junction box blocks every connector through it. Civilians never chase
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

**Leader query** (`leader`, for police units and AI racers): `find_leader(own_pos,
centreline, obstacles, skip, params)` samples the caller's path ahead (any path type — a
closure `d → point d m ahead`) from the car's own position out to the look-ahead
(`LeaderParams::lookahead_for(v)`: stopping distance at 6 m/s² + 3 s + 20 m, 40–150 m), projects
every `Obstacle` (oriented box from a `VehicleState` or `AgentView`) onto it and returns the one
whose box overlaps a ±1.2 m strip with the smallest bumper gap, with its speed along the path
(negative = oncoming). `CarFollowing::{CIVILIAN, PURSUIT}::leader_cap(v, leader)` is the IDM
interaction term (the exact law traffic uses — traffic's `idm` delegates to it); combine as
`acc.min(cap)`. `hold_behind` / `held` keep a stopped car on the handbrake: `DriveController::pedals`
releases the foot brake below 0.6 m/s (holding it engages auto-reverse), so without a hold a car
idles forward into the one it stopped behind. `PathFollower::follow_behind` does all of this
for `DrivePath` users. Real-vehicle tests (`tests/drive.rs`): from 25 m/s with pursuit
settings it stops 2.5 m behind a stopped car and settles ≈ 9 m behind an 8 m/s car. Physical
traffic uses the same hold when stopped.

**Blocked vehicles** (owner playtest row 32; `traffic/blocked.rs`, `manoeuvre.rs`). Every
decision records which constraint set the acceleration and from that why a stopped car waits
(`Agent::wait`): `Line` (stop line, signal, siren, junction rules, pulling-away delay), `Queue`
(the car ahead waits for one of those) or `Obstructed` (the car ahead is disabled or itself
obstructed; a stationary tracked vehicle — parked player, stopped police car — that is not at
or queued before the stop line; debris). Obstructions are stopped behind with 6 m extra room
(`OBSTACLE_GAP_M`). A driver acts once the obstruction has stood still for its **patience**
(law-abiding 4.5–8 s, casual 3.5–6 s, aggressive 3–4 s; derived from id + seed, no RNG draw)
and it has stood behind it ≥ 1 s:
- **Multi-lane road**: a forced MOBIL lane change (incentive threshold lowered, safety
  criterion unchanged; the target lane must not hold a stopped car or a parked tracked vehicle
  within 40 m). Physical agents may lane-change for this too. After patience + 8 s without a
  change the alternatives below are tried.
- **Two-lane road — pass in the oncoming lane** (first car behind the obstruction only): the
  obstruction (plus stopped cars right in front of it) ≤ 30 m long, its box clear of the
  oncoming lane, the pass ends ≥ 20 m before the junction, the road up to the junction visible
  (`sight_clear`: no crest hiding a 1.1 m object from a 1.1 m eye, no bend > 20°), and the
  oncoming lane clear for `pass + v_limit × 1.25 × t_pass + 20 m` (≤ 160 m) including cars in the
  junction / on lanes about to turn into it, and no tracked vehicle coming. The pass is slow
  (4 m/s), indicated (left out, right back), keeps clear of the obstruction's box (creeps at
  1.5 m/s aiming 0.6 m wider until the body is clear; stops if it would touch), gives way to
  anything in the oncoming lane (IDM against it) and goes back behind the obstruction if an
  oncoming car appears before it is alongside, or if it stalls 4 s. While in the oncoming lane
  it has an entry in that lane's list, so oncoming traffic stops for it like for any car.
- **Turn around** otherwise (two-way road, ≥ 6 m from the junction ends): `plan_turn` finds a
  U-turn or k-point turn within the paved driving lanes (parking lanes excluded) clear of every
  nearby car / tracked vehicle / debris; it starts when every oncoming car can stop for it
  (`(v + 2)² / 6 + 25 m` from its own speed `v`), followers in its own lane can stop at
  2 m/s² at least 10 m short of its rear (room for the reversing legs), and the lanes crossed
  have nobody arriving within 40 m. Kinematic agents follow the
  planned legs at ≤ 1.2 m/s (0.7 s between legs, re-checking each leg's sweep); physical agents
  run `TurnDriver` (re-plans before every leg from the measured pose, engages reverse through the
  automatic box). The car holds entries in its own lane (3 m behind it) and the oncoming lane
  while turning, then drives off in the opposing lane.
- **Reverse to the last junction** (one-way street, no room to turn, or the car is stuck in a
  junction connector): only the last car in the lane; it first claims the spot at the end of the
  lane it came from (an entry there makes approaching cars hold back), starts once that spot,
  the junction across its path and the diverging connectors are clear (one car per spot; gives
  up after 40 s), holds its connector while backing through it at ≤ 2.5 m/s, and stops at the
  previous lane's stop line, from where it re-plans.
- **Re-route**: a lane someone turned around in, reversed out of, or found impassable is
  remembered as blocked for 90 s: `choose_next` avoids it while alternatives exist, and cars
  planning into it re-plan (giving up a permit they can still comfortably stop for).

Police patrols (`wp_police::avoid`, workstream 06) pass stopped vehicles with their own logic;
`manoeuvre::{road_corridor, plan_turn, TurnDriver, overtake_sight_clear}` let them add the
turn-around and the crest / bend rule (request in `docs/integration/world.md`).

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
| Same after driver variety (round 6 follow-up, 2026-10-19) | perf test 0.055 ms (0.060 before, same run); bench avg 0.068 ms, p99 0.27 ms, p99.9 0.76 ms, single max 4.5 ms (container noise; before: 3.7–6.6 ms) | — | 2 ms |
| `leader::find_leader`, 60 m look-ahead, ~30 obstacles | a few µs per call | — | — |
| Same with civilian-collision diagnostics on | avg 0.20 ms | — | — |
| `line_of_sight`, random 300 m sight lines at eye height | ≈ 1.1 µs/query | — | — |
| `surface_at` (round 8, incl. markings) | 0.36 µs/query, of which markings 0.10 µs (`surface_timing`) | — | — |
| `RoadNetworkIndex::nearest_on_road` (425 edges) | 0.63 µs/query with the edge grid; linear scan was 19 µs (`nearest_on_road_timing`) | — | — |
| `World::load` of `procedural_12x12.road.bin` (decode + `CityMap::new` + parked cars) | ≈ 17 ms (debug build ≈ 0.2 s; `ProcGenConfig::small()` ≈ 5 ms / 0.06 s) | — | — |
| 10-min soak, 9 seeds × {0 %, 5 %} aggressive | 0 civilian–civilian collisions; 0–11 agents recycled after being stuck > 120 s (starved permissive lefts / minor approaches in saturated flow) | — | < 1 % |

Physical agents use the `wp_vehicle` stub (negligible); re-measure when prompt 02 lands.

Blocked vehicles (round 9, playtest row 32): the 400-agent perf test 0.075 ms per frame (no
blocker; the 10-min soaks give bit-identical stats to before: normal traffic is unchanged);
**0.091 ms** with 12 blockers (8 two-lane, 4 arterial) and passes / lane changes / turns /
reverses running (`perf_400_agents_with_blockers_under_budget`, ignored). Per decision the
added work is the binding-constraint bookkeeping; manoeuvre checks run only for obstructed cars
(once per second) and for the few cars in a manoeuvre.

Port Vesper v2, same settings (`CITY=vesper_v2 just traffic-bench`): avg 0.061 ms, p99 0.16 ms
per 60 Hz frame (0.27 ms avg with collision diagnostics); 10-min soaks × 3 seeds × {0 %, 5 %}
aggressive: 0 civilian–civilian collisions (`tests/city_v2.rs`, ignored).

## Known gaps
- Physical agents are `wp_vehicle` instances, but `wp_sim` does not yet create rigid bodies /
  damage models for them (needs `LodChange` handling in the sim loop, see
  `docs/integration/world.md`). Until then physical traffic cannot be hit.
- Under saturated flow a few left-turners / minor-road cars can starve for > 2 min; they are
  recycled when unseen. No protected-left phases yet.
- Spawning uses one traffic car definition (no per-district vehicle mix yet).
- Agents do not react to other civilians' hazards (only tracked vehicles and debris).
- Blocked vehicles: no use of shoulders / parking lanes (parked cars live there); passes are
  one car at a time (followers queue up and pass in turn); a k-point turn assumes the road is
  locally straight (it plans in a straight corridor; the physical driver stops a leg early if
  the body nears an edge); several cars reversing out of a long one-way street go one after
  another, so the last of a long queue may still be recycled as stuck after 120 s.
- Sightline rule is distance-only (no building occlusion).
- Lane change across a solid line / bus lane rules not modelled; buses not simulated.
- Elevation: kinematic agents follow the lanes' 3D polylines; un-embodied physical agents
  probe `surface::MapGround` (roads + terrain). Agent views carry yaw only (no pitch/roll on
  slopes; the testbed draws kinematic cars level).
- City v2: `WorldQuery::surface_at` and `CityMap::ground_height` project onto whole lane
  polylines (≈ 0.5 µs on the curved city); terrain LOS samples every 8 m.
- `CityMap::ground_height` / `MapGround` do not know the raised sidewalks (roads, junctions,
  terrain only); `wp_sim` lifts spawns onto a sidewalk top under them. Corner fills and trims
  follow pavement in 0.5 m steps (a sidewalk edge can overhang the outer rim of a junction
  disc by up to ~0.35 m; never a lane or turning path — tested).
