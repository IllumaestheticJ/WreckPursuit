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

## Contract change requests
None (v3 lives entirely in `wp_world` and the renderer/physics plumbing).

## Known gaps
- Work in progress: see the module reports below as they land.
