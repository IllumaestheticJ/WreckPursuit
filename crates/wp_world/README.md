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
| `vesper` | Port Vesper v2 (`vesper_v2`): curved streets, hills, terrain, banked highway loop, grade separations, the Vesper River and Calder Canal with their bridges, the Hollins Tunnel, landmarks, hideouts |
| `terrain` | `Heightfield` (stored with the network), value noise, bumps |
| `surface` | Road trimesh for physics/greybox, deck rails, median and highway edge barriers, `MapGround` wheel probe |
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
  right; lanes lie on `centre.z + bank · lateral`), `left_m`/`right_m` paved widths (lanes +
  parking + shoulders), `highway`, `sidewalk`, and (v3) `median_barrier`: a concrete barrier
  runs along the left paved edge (`surface::median_barriers`). Physics and the greybox draw
  these.
- `terrain` (v2): `Option<Heightfield>` — `origin`, `cell_m`, `nx`, `ny`, row-major heights
  (bilinear between vertices), and (v4) `holes`: removed cell ids (`j * (nx − 1) + i`, sorted)
  that physics and the greybox leave out (tunnel mouths; structures close them). `None` = flat
  ground at z = 0 (the procedural grid, imports).
- `features` (v4, `CityFeatures`; empty for the grid city and imports):
  - `waterways[]`: `name`, `kind` (`River` / `Canal`), `centre` (z = water surface, source →
    mouth), `half_width_m[]` per point, `depth_m`. `network::SEA_LEVEL_M` (0.25 m) is the sea.
  - `bridges[]`: named deck stretches over water — `name`, `kind` (`Beam`, `Truss` = steel-grate
    deck, `Viaduct`), `surface` (index into `surfaces`), deck `centre`, `half_width_m`.
  - `tunnels[]`: `name`, floor `centre` + `bank[]`, `half_width_m`, `height_m`;
    `Tunnel::contains(p)`.
  - `structures[]`: solid prisms that are not ordinary buildings, possibly floating — `kind`
    (`TunnelWall`, `TunnelCeiling`, `TunnelPortal`, `Canopy`, `Pillar`, `Landmark`, `Truss`),
    convex `footprint`, `z0_m`..`z1_m`. They collide (`wp_sim`), render and block line of sight.
  - `landmarks[]`: `name`, `kind` (`Tower`, `Lighthouse`, `Observatory`, `Chimney`, `Crane`,
    `WaterTower`, `Bridge`, `Tunnel`), base `pos`, `height_m`, its `structures`.
  - `hideouts[]`: `name`, `kind` (`Tunnel`, `Canopy`), `pos` (ground level), `radius_m`: places
    hidden from the air.

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

`marking_mesh(&map)` turns the same rules into a render mesh (white + yellow paint, 2 cm above
the banked surface); the testbed draws it through wasm `road_markings(yellow)` (Port Vesper
v2: ≈ 91 k triangles, two draw calls). Centre lines and the left (median-side) edge line of a
one-way highway are yellow; merging freeway ramps get no stop line.
`mesh_paints_what_marking_at_reports` checks that > 98 % of the mesh's triangles sit over a
`marking_at` hit of the right colour.

### Builder rules (procgen and OSM share them)
- Junction setback per road end from both road half-widths and the junction angle
  (`d = (h_j + h_i cos θ) / sin θ` + 2 m crosswalk), so skewed (diagonal) junctions get larger
  boxes; degree-2 nodes are continuations, degree-1 nodes get U-turns.
- Default lane use (no `turn:lanes`): leftmost lane Left+Straight, rightmost Right+Straight,
  middle Straight; bus lanes Straight+Right. Left turns map left-aligned, others right-aligned.
- Control: signals when every road is collector or above, at 5+-way junctions, or when tagged;
  priority (higher class has right of way) when classes differ; all-way stop otherwise.
- Freeway merge / diverge (`ControlHint::Freeway`, three one-way roads): lanes set back
  `FREEWAY_SETBACK_M` (30 m) so the ramp lane tapers along a 60 m connector, the main road's
  paving runs through (no junction disc: radius 0), main lanes continue left-aligned and the
  ramp joins / leaves the rightmost lane (a diverge's rightmost lane is an option lane).
  Control is priority with the main road major; a minor *highway* lane (an on-ramp) yields on
  the move in `traffic.rs` instead of stopping first.
- Signal plan: opposite approaches share a phase; green 14–30 s by class; yellow
  `clamp(1 + v / 6.1, 3, 5)` s (ITE); all-red 1.5 s (2 s above 20 m/s).

## Procedural city (`gen-city --grid 12x12 --seed N`)
Grid of streets at 110 m centred on the origin. Rows/columns 2 and 9 are 2+2 arterials
(40 mph), the two central rows/columns collectors (30 mph, parking), the rest local streets
(25 mph, parking). The highway spine (55 mph) runs north–south between the central columns:
a divided viaduct (two one-way 3-lane carriageways, shoulders, median barrier) that leaves
the ring road at grade at each end (signalised T), climbs over 200 m (≤ 6 %) after a 115 m
flat run and bridges the arterial/collector rows with 7.5 m clearance; other rows end at the
columns beside it. The first and last crossing rows get ramps: 45 mph ramps that leave /
join the carriageways 280 m away and meet the row at stop-controlled terminals 22 m either
side of the centreline (`ControlHint::RampTerminal`: the street keeps priority, the off-ramp
stops and yields). Every grid gets the half diamond facing the ring (an exit for traffic
heading away from the ring, an entry towards it: 4 ramps); where the next block in is long
enough as well (the 12×12 city) each row gets a full diamond (8 ramps). The ground is
a flat heightfield 12 cm under the streets, so `wp_sim` builds the road mesh (the viaduct is a
real bridge) and the greybox draws piers and parapets; buildings still do not block cars on
flat ground. Two 3+3 diagonal
arterials with kerbside bus lanes (35 mph) run corner to corner through grid nodes (6-way
signalised junctions) and stop at the highway block. An octagonal 2+2 ring road (45 mph)
collects every street end (priority T-junctions with local streets). Mid-block alleys
(15 mph) in core blocks. Zones per block by radius (Core < 200 m, Commercial < 380 m, else
Residential, Industrial in the south-east) plus outer bands (Industrial south, Port east,
Suburb north/west). Buildings on lots (Core 5–8 storeys, Commercial 2–4, Residential 1–2 with
courtyards, warehouses in the bands). Green-wave offsets along arterial rows (eastbound),
columns (northbound) and diagonals at 90 % of the limit. POIs: 2 police stations + helipad,
3 garages, 4 safehouse candidates, 3 race grids (highway, arterial, ring). The 12×12 city has
1742 lanes, 232 junctions (54 signalised, 8 freeway merges / diverges), 1068 buildings.

## Port Vesper v2 (`vesper::generate`, `World::vesper`, `gen-city --preset vesper_v2`)
After `docs/design/CITY.md`, scaled to ≈ 3.3 × 2.9 km (`MAP_LO..MAP_HI`), 5 m terrain cells.
- **Layout:** Lantern Row downtown (7×7 grid at 100 m, arterials/collectors, signals);
  Harbor Boulevard curving along the flat south shore; Gantry Row port grid + Terminal Avenue on
  the quay; Canal Flats industrial grid; organic districts as warped grids with curved edges,
  dropped edges and cul-de-sacs (Larkspur west, Mercer north, Eastgate east, Bayview on the
  north-east plateau); Hollins Drive (6 switchbacks up Hollins Knob, ≈ 110 m), Ridge Road back
  down over the crests, the Knob lot; Bluff Road along the plateau edge; Loop 9, a divided
  highway loop (filleted control polygon, radius ≥ 420 m sweepers; see below) with six
  interchanges and no at-grade junction; Route 40 West (dead-ends at the map edge, passes
  **under** the loop: overpass) and Route 40 North (cut **under** the loop: underpass).
- **Loop 9:** profiled as one road (so both directions share one vertical profile and bank),
  then split into two one-way 3-lane carriageways 7.05 m either side of the centreline: 1.5 m
  paved median shoulders up to a Jersey-profile concrete barrier on the centreline (0.6 m base,
  0.81 m tall; `wp_sim` builds 0.5 × 0.81 m box colliders along it), 2.5 m outer shoulders.
  **Edge barriers** (`surface::edge_barriers`, both cities): the same concrete wall runs
  along every carriageway's outer edge and both edges of every ramp, its inner face on the
  paved edge, so a car cannot leave the highway for the verge. It opens wherever another
  road's paving meets the edge (ramp tapers and gores) and gives way to guard rails on decks
  and to the lining in the tunnel; ≈ 36 km on Port Vesper, ≈ 1.8 km on the grid city's
  at-grade highway ends.
  **Interchanges:** five diamonds (Harbor Boulevard west and east, Lantern Way, Bluff Road,
  Uptown Connector) and a half diamond at the port (inner carriageway only). Each diamond's
  crossroad (2+2, 10 % grade cap) bridges both carriageways with 7.5 m clearance between two
  signalised terminals 150 m either side; four one-way 45 mph ramps per diamond leave / join
  the carriageways 300 m up / downstream of the crossroad. A ramp shares the carriageway's
  plane for its first 90 m (30 m taper + parallel lane beside the shoulder), then S-curves to
  its terminal under an 8 % cap; a second profiling pass holds every terminal within reach of
  its ramps' gores. Interchange crossroads take the old junctions' local roads (Terminal
  Avenue no longer reaches the loop; the port is served by the half diamond).
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
- **Vesper River** (`features.waterways[0]`): north edge to the sea along x ≈ −505, between
  Larkspur and downtown; 32–60 m of water, 2.5 m deep, 12 m banks, cut into the natural ground
  (`Natural::z`). The surface follows the ground 3 m down and never rises towards the mouth
  (19.5 m at the north edge → sea level by Harbor Boulevard), so it runs through a gorge past
  Hollins Heights. **Calder Canal**: a level 20 m cut west from the river through Canal Flats
  (between its two rows; the grid moved west of the river) to a 60 m turning basin, 3 m deep
  with steep quay banks.
- **Bridges:** every road that crosses a channel is held at deck height across it (water +
  banks, anchors like a grade separation; deck just over the lower bank and ≥ 3 m over the
  water, 4.5 m for Loop 9, 2.2 m over the canal), left uncarved and gets deck rails and piers
  like any deck. Eleven named bridges: Vesper Viaduct and Mouth Viaduct (Loop 9 north and
  south), Ridge Road Bridge, Hollins Gorge Bridge, Lantern Bridge, Larkspur Avenue Bridge, the
  **Iron Bridge** (Canal Street, 1+1, steel truss over a steel-grate deck: `surface_at` reads
  `Metal`), Harbor Boulevard Bridge, and the Basin, Mill and Lock bridges over the canal. Loop
  9 is never cut into pieces over the river or in the tunnel.
- **Hollins Tunnel** (`features.tunnels[0]`, ≈ 430 m): Loop 9's north leg is held at 33 m under
  Hollins Heights' northern shoulder (it used to crest at ≈ 55 m); the bore is the stretch with
  ≥ 9 m of natural ground over its full 30 m width. The bore is not carved (the hill stays over
  it, raised to clear the 7 m ceiling where the approach cuts reach in). At each mouth the
  terrain cells whose surface would dip across the road become `holes`; a portal block (roof
  + side walls, 10 m), an apron (9 m) and the lining (walls on the paved edges, ceiling slabs
  every ~10 m) close them. Inside, the terrain does not block line of sight
  (`CityMap::in_tunnel`), so a unit in the bore sees along it while the helicopter is blind.
- **Landmarks** (`features.landmarks`; lots found by an outward search for dry, level ground
  clear of roads, reserved before buildings are placed): Lantern Tower (230 m, downtown),
  Vesper Point Lighthouse (south-east shore), Hollins Observatory (the knob), Calder Mill
  Chimney (Canal Flats), three Gantry Cranes on the quay (booms over the shore highway), the
  Larkspur Water Tower; plus the Iron Bridge and the Hollins Tunnel.
- **Hideouts** (`features.hideouts`): the Hollins Tunnel and five canopies — roof slabs 4.2 m
  over a level lot next to a road, on pillars, open at the sides (Calder Mill Shed, Gantry Row
  Reefer Dock, Larkspur Carports, Eastgate Parking Deck, Lantern Row Arcade). A car under one is
  out of the helicopter's sight; ground units see in from the side.
- **POIs:** race grids `Lantern Run` (first: free drive / city chase spawn), `Loop 9 sprint`,
  `Hollins Ascent` (just across the Hollins Gorge Bridge), `Hollins Descent`, `Ridge Road`, `Harbor Boulevard drag`, `Bluff Road`,
  `Loop 9 north`, `Loop 9 west`; 3 precincts + helipad, 3 garages, 4 safehouses (incl.
  `Knob Observatory Lot`).

1012 lanes, 159 junctions (31 signalised, 22 freeway merges / diverges), 3218 buildings,
14 grade separations (Route 40 West / North and five crossroads, each over or under both
carriageways), 22 ramps, ≈ 10 km of median barrier, 11 bridges over water, one 434 m tunnel,
287 structures, 10 landmarks, 6 hideouts, terrain 681 × 611 vertices (−3 … 111 m) with 22
holes; generation ≈ 0.21 s release, `CityMap::new` ≈ 10 ms. `cargo run --release -p wp_world --example vesper_stats` prints grades, crest/sag g
at the limit and banks per road.

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

Port Vesper v2, same settings (`CITY=vesper_v2 just traffic-bench`): avg 0.061 ms, p99 0.16 ms
per 60 Hz frame (0.27 ms avg with collision diagnostics); 10-min soaks × 3 seeds × {0 %, 5 %}
aggressive: 0 civilian–civilian collisions (`tests/city_v2.rs`, ignored).

## Known gaps
- Water is scenery: a car that leaves a bridge drives on the river bed (no buoyancy, drag,
  drowning or engine flooding yet). `CityMap::water_surface_at` is there for whoever adds it.
- Only Loop 9 has a tunnel; tunnels are built from Loop 9's ring profile, so a tunnel on an
  ordinary road needs the same treatment from its own surface. Ceiling slabs step by the
  road's grade between 10 m pieces (greybox).
- The Kettle Street drawbridge, rail line and Westline viaduct from `docs/design/CITY.md` are
  not built; the river runs north–south here (the design's runs east–west).
- Only the tunnel, canopies and other structures hide a car from the air: road decks do not
  block line of sight (a car under a viaduct is still seen by the helicopter).
- Grid cities smaller than 12×12 have half diamonds only (their inner blocks are too short
  for the other half): from the city you can join the highway towards the ring and leave it
  coming from the ring, plus the ring junctions at both ends.
- The single-car rig (`wp_vehicle::rig`, client prediction and `tests/drive.rs`) drives on a
  flat slab, so it does not see the grid's viaduct either (as on Port Vesper's hills).
- No painted gore chevrons or dotted lane-line extensions across freeway tapers; the taper
  connectors themselves read as `Concrete` in `surface_at` (other connectors read `Asphalt`).
  Taper connectors follow the paving's height (graded, banked) rather than a straight line.
- Physical agents are `wp_vehicle` instances, but `wp_sim` does not yet create rigid bodies /
  damage models for them (needs `LodChange` handling in the sim loop, see
  `docs/integration/world.md`). Until then physical traffic cannot be hit.
- Under saturated flow a few left-turners / minor-road cars can starve for > 2 min; they are
  recycled when unseen. No protected-left phases yet.
- Spawning uses one traffic car definition (no per-district vehicle mix yet).
- Agents do not react to other civilians' hazards (only tracked vehicles and debris).
- Sightline rule is distance-only (no building occlusion).
- Lane change across a solid line / bus lane rules not modelled; buses not simulated.
- Elevation: kinematic agents follow the lanes' 3D polylines; un-embodied physical agents
  probe `surface::MapGround` (roads + terrain). Agent views carry yaw only (no pitch/roll on
  slopes; the testbed draws kinematic cars level).
- City v2: `WorldQuery::surface_at` and `CityMap::ground_height` project onto whole lane
  polylines (≈ 0.5 µs on the curved city); terrain LOS samples every 8 m.
