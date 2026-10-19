# PROMPT 05 — City, road network, signals & civilian traffic (`wp_world`)

Read `CLAUDE.md`, `docs/INTEGRATION.md`, `docs/CONTRACT_CHANGELOG.md`. You own `wp_world`, `data/city/`, and `tools/city-import/`. You may read `wp_contracts`, `wp_math`, `wp_vehicle` (for the traffic vehicle "lite" model), `apps/testbed-web` (to emit a greybox GLB it can load). Branch `ws/world`.

## Goal
A real, working city traffic system: lane-level road graph, signalised intersections, speed limits, civilian AI that obeys lights and limits (and reacts to racers and sirens), density that falls off with distance from city centre and changes with time of day, and a day/night clock. Plus a greybox city mesh so the testbed and police AI have somewhere to exist. Deterministic; ≤ 2 ms per 60 Hz frame for 400 agents native.

## Work
**Road network format** (`data/city/<city>.road.bin` + `.road.json` debug dump): nodes, directed lane segments (centerline polyline, width, speed limit m/s, lane type: Through|TurnLeft|TurnRight|Bus|Parking|Alley|Highway, allowed turn connections at the far end), intersections (incoming lanes, signal group id or priority/stop rule), signal controllers (fixed-time phase plans with all-red + yellow, `offset` for green waves on arterials), zones (polygons with `district_kind: Core|Commercial|Residential|Industrial|Port|Suburb` and `density_base`), POIs (safehouse candidates, garages, race start grids, police stations, helipad). Spatial index (uniform grid) for `WorldQuery` lookups.

**Importer (`tools/city-import`)**: reads an OpenStreetMap `.osm.pbf` extract (use `osmpbf` crate), builds the lane graph from `highway=*`, `lanes`, `oneway`, `maxspeed`, `turn:lanes`, `highway=traffic_signals`, default speed limits per road class when missing (US: residential 25 mph, collector 30–35, arterial 35–45, highway 55–65). Emits the binary + a `greybox.glb` (road ribbons, sidewalks, simple block extrusions from `building=*` footprints with 1–8 storey heights). Document attribution requirements. Also include a **procedural fallback** (`gen-city --grid 12x12 --seed N`) so nobody is blocked on an extract: grid + a ring road + a highway spine + 2 diagonal arterials, zones by radius.

**Day/night clock**: `TimeOfDay` advancing at a configurable ratio (default 1 real minute = 10 game minutes → 2.4 h day), `DayPhase`, `night_factor()`, sun direction helper for the renderer.

**Density model**: target agents in a cell = `density_base(zone) * f_distance(d_to_core) * f_time(TimeOfDay, district_kind)` where `f_time` has morning/evening commute bumps for Residential↔Commercial flows and a deep trough 01:00–05:00 (Core keeps nightlife density until ~02:00). Spawn/despawn ring around each human player and around active race routes; never spawn within sightline of a player (use distance + a 1 s delayed-visibility rule for the prototype).

**Traffic agent AI** (`traffic.rs`): lane-following with IDM car-following (desired speed = min(limit × driver_factor, curve speed), time headway 1.2–2.0 s per driver personality), MOBIL-style lane changes on multi-lane roads, intersection logic (signal state, stop lines, yield on left turn, right on red if `allow_rtor`), **hazard reactions**: brake hard for a vehicle approaching head-on or crossing at speed, pull right and slow for a siren within 60 m (and stop at intersections), swerve lightly to avoid debris. Personalities: 70% law-abiding, 25% casual (limit +10%), 5% aggressive (runs yellows, tailgates). Civilians **do not** chase or block players intentionally.

**Two LODs for agents**: far = kinematic (pose integrated along lane), near (within 80 m of a player or race route) = physical: a `wp_vehicle` instance with `AssistProfile::ai()` driven by a steering/throttle controller tracking the lane path (pure pursuit + PID speed). Promotion/demotion must be seamless (match pose and velocity). Physical agents can be hit and will deform via the normal damage path (prompt 03). Parked cars along `Parking` lanes as static colliders with mass (density by zone).

**`WorldQuery` implementation**: surface type from lane/zone (asphalt default, curbs at ribbon edges, grass/dirt off-road from zone), speed limit, signal state ahead, time of day. Plus `TrafficViolation` detection for any tracked vehicle (speeding by > X over limit for > 1 s, red-light running via stop-line crossing during red, wrong-way via lane direction, reckless = near-miss count) — emit `SimEvent::TrafficViolation`; prompt 06 turns them into heat.

**Race route helper**: `routes.rs` — given start/finish POIs produce a lane-legal path and a checkpoint list; given a circuit polygon produce a closed loop; emit closure markers for "controlled day events" (traffic suppressed on the route, cones as static colliders).

## Tests
- Signal controller: no conflicting greens; cycle lengths honoured.
- Traffic: 400 agents for 10 min of sim with zero civilian-civilian collisions on the procedural city (allow < 1% with aggressive personalities enabled and record it).
- Density: counts per zone track target within 15% after warm-up; time curves produce expected ratios (noon core vs 03:00 core ≥ 5×).
- Determinism hash.
- Perf benchmark.

## Deliver
`wp_world`, `tools/city-import`, `data/city/procedural_12x12.*` + `greybox.glb`, tests, README (format spec + AI write-up), `docs/integration/world.md` (exposes: `World::load`, `World::step`, `WorldQuery`, agent iteration for the renderer/netcode, route helper; needs: player positions, siren flags, race-route closures from the sim loop; contract requests).
