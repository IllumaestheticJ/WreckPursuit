# Port Vesper v3 (city overhaul) integration notes

- **Workstream:** city v3 — the full city after `docs/design/CITY.md`, built on the side.
- **Branch:** `claude/sharp-goodall-y8gbuc` (module branches merge into it).
- **Crates / folders:** `crates/wp_world/src/metro/` (new), `data/city/vesper_v3.*`, plus
  the feature plumbing in `wp_sim` (colliders), `tools/city-import` (greybox), `wp_wasm` and
  `apps/testbed-web` (instanced feature rendering).
- **Contract version built against:** 0.3.0 (no `wp_contracts` change needed).

Port Vesper v2 (`vesper_v2`) stays the default city until v3 is ready. v3 is selectable as
city preset `vesper_v3` (`World::metro`, simcli `city: preset(name: "vesper_v3", seed: 1)`,
`gen-city --preset vesper_v3`, testbed `?city=vesper_v3`).

## Layout

8 × 7 km (`x −4400..3600`, `y −3600..3400`), 6 m terrain cells. Everything is in
`metro/plan.rs`: the Vesper River and the Sluice Canal, Calder Bay and the south sound, 24
district zones, six highways (Loop 9, Route 40 West, Crosstown Expressway, Route 40 North,
Airport Expressway, Port Connector) with 14 interchanges (cloverleaf, stack, trumpets,
diamonds, parclo), 18 arterials, 14 bridges / tunnels / viaducts, three rail lines, ~35
reserved sites (airport, airfield, container terminal, quays, piers, rail yard, station,
parking structures and lots, construction sites, landmarks, parks) and the CITY.md POIs.

## Pipeline and ownership (`metro/mod.rs`)

| Step | File | Owner |
|---|---|---|
| layout | `plan.rs` | lead (read-only for everyone else) |
| natural ground, earthworks | `ground.rs` + each module's `earthworks()` | lead / modules |
| roads, bridges, tunnels, highways, carving | `roads.rs` (may become `roads/`) | roads |
| shared placement context | `ctx.rs` | lead |
| airport + airfield | `airport.rs` | airport |
| port, waterfront | `docks.rs` | docks |
| rail | `rail.rs` | rail |
| parking structures, lots | `parking.rs` | parking |
| construction sites | `construction.rs` | construction |
| buildings, landmarks | `buildings.rs` | buildings |
| scenery, boundaries | `scenery.rs` | scenery |
| feature format | `network.rs` (`Feature`, format v4) | lead |

Modules after roads run in the order above on one `Ctx`; each sees what earlier ones placed
(occupancy grid: road, verge, water, building, feature, site). Each draws randomness from its
own `ctx.rng(SALT)`.

## Features (road format v4)

`RoadNetwork::features: Vec<Feature { kind, material, shape, solid }>`; shapes are `Box`
(centre, half extents, yaw, pitch), `Cylinder` and `Prism` (convex if solid). Solid features
are static colliders in `wp_sim`; drivable decks / ramps are just solid boxes with a flat top.
Boxes and cylinders are drawn instanced by the testbed (wasm export), prisms go into the
greybox. Water is terrain below `plan::WATER_Z` (one sea plane in the greybox).

Making areas inaccessible: buildings (convex colliders), walls / fences / hedges / Jersey
barriers along lot lines, water edges, cliff edges, rail corridors, airport and port
perimeters, plus map-edge walls. Gates in the plan are the only ways into fenced sites.

## Status (2026-10-21)
All modules are merged and every workspace test passes (`cargo test --workspace`, clippy
`-D warnings`, determinism lint, wasm build). The city is playable in the testbed
(`?city=vesper_v3`) and in simcli; v2 stays the default.

| Seed 1 | Count |
|---|---|
| Lanes / junctions / signalised | 6818 / 1198 / 430 |
| Highway | 23.1 km, 56 ramps, 134 grade separations; cloverleaf, stack, 3 trumpets, 7 diamonds, parclo, half diamond |
| Structures | 1131 piers, 3.4 km of covered tunnel, 24 sign gantries |
| Buildings | 12 722 |
| Features | 99 557 (largest: trees ≈ 21 k incl. canopies, fences ≈ 19 k pieces, lamps ≈ 7.5 k, containers ≈ 6.2 k, rail track ≈ 5.5 k) |

| Module (owner file) | Highlights |
|---|---|
| roads (`metro/roads/`) | all district street patterns, Hollins Drive (4 km, 11 hairpins), Skyline Drive + Westridge Tunnel, river bridges, Calloway Tunnel, six divided highways and their interchanges |
| airport (`metro/airport/`) | two drivable runways + taxiways, terminal, 3 piers / 18 jet bridges, tower, hangars, 20+ aircraft; Northfield Airfield with 7 hangars (SH13) |
| docks (`metro/docks/`) | ≈ 400 container blocks (Gantry Maze), 13 gantry cranes, 3 container ships + a bulk carrier, quay walls with kerbs, 4 Old Harbor piers, ferry terminal, marinas |
| rail (`metro/rail/`) | graded freight + passenger lines (≤ 1.5 %), swing bridge, 16-siding yard with ≈ 500 cars, roundhouse + turntable (SH05), elevated Vesper Central, ≈ 13 level crossings with gates, fenced corridors |
| parking + construction | 5 drivable multi-storey garages (gate → roof, ≤ 1:7 ramps), 4 surface lots, Starlite Drive-In; Lantern Tower, the Unfinished Exchange jump (22 m gap), Canal Flats off-road earthworks, Eastgate block |
| buildings (`metro/buildings/`) | district-specific lots (8 downtown landmark towers 150–220 m on podiums), lot walls / fences / hedges, Vesper Bowl (drive onto the pitch), City Hall, Calder Power Station, steel mill, observatory, malls, lighthouse, water towers, churches, gas stations |
| scenery (`metro/scenery/`) | water-edge barriers, bluff crest wall, highway guard rails / sound walls / fences, map-edge walls and Route 40 road-closed portals, forests with solid undergrowth edges, parks, street trees / lamps / poles / furniture kept ≥ 1.8 m off every lane and turning path |
| plumbing | feature colliders (compound per 256 m tile), prisms in the greybox, wasm `city_features()`, instanced rendering in the testbed (`;` toggles, 1.5 km draw distance, `?featureDist=`) |

## Visual clearance
`tools/city-import/tests/greybox_clearance.rs` drives a ray along every lane at 1.2 m and
3 m and fails on any blocking render geometry: greybox walls, bridge decks / piers, buildings,
feature prisms and (v3) every box / cylinder feature. Exceptions: tree canopies and the
road-closed portals within 30 m of the map edge. Bridge piers are never placed on paving or
junction boxes under a deck (they move to the next clear centreline point), and rail track
on a road at its own height is laid flush in the paving (street running).

## How to run
- Tests: `cargo test -p wp_world` (v3 suites: `metro::*` lib tests, `tests/city_v3_{roads,rail,parking}.rs`).
- Drive checks (not in the golden library while v3 changes):
  `cargo build --release -p simcli && ./target/release/simcli run apps/simcli/scenarios-dev/v3/`.
- Greybox: `just city-v3`; screenshots: `cd apps/testbed-web && node e2e/city_v3_shots.mjs`
  (writes `docs/media/city-v3/`).

## Perf (native release, cloud container)
- Generation (`metro::generate`, whole pipeline) ≈ 4 s; `CityMap::new` ≈ 0.1 s. Browser free
  drive starts in ≈ 13 s (swiftshader headless), greybox loads ≈ 10 s later.
- A 7-min sim session including world build ≈ 35 s; 50 k solid feature boxes cost ≈ 57 ms to
  build and ≈ 5 µs per physics step.

## Contract change requests
None (v3 lives entirely in `wp_world` and the renderer/physics plumbing).

## Known gaps
- `apps/simcli/scenarios-dev/v3/traffic_observer.ron` logs 1 civilian–civilian collision in
  120 s with 400 agents (traffic AI, no city cause found); police AI still clips kerbside
  parked cars (as in v2).
- Greybox is ≈ 29 MB (+ 38 MB base64) — mostly road paving; no stale-data test while v3 changes.
- Not yet wired: police doctrine for the river chokepoints / tunnel LOS, train / drawbridge /
  airport traffic, interior navigation for AI in garages, race routes using the new landmarks.
- Lot side/rear fences are trimmed to the buildings budget, so some short stretches behind
  houses stay open; the swing bridge rail deck is drivable from its level crossings.
- Port Vesper v2 (not v3): one lot's building at (-1125..-1089, -896..-865) stands over two
  lanes of roads 173 / 177; found by `tools/city-import/tests/greybox_clearance.rs`, which
  therefore checks only bridges and walls for v2.
- POI snapping: `SH06 Knob Observatory Lot` snaps to the westbound lot lane, so routes ending
  there may loop the lot.
