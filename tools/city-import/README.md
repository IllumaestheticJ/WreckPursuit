# city-import

**Owned by workstream 05 (city & traffic).** Native-only tools (never a dependency of
`wp_wasm`): OSM importer, procedural city generator, greybox `.glb` writer.

## Usage

```sh
# Procedural fallback (no extract needed) — what `just city` runs:
cargo run --release -p city-import --bin gen-city -- --grid 12x12 --seed 1 --out data/city
# OpenStreetMap extract:
cargo run --release -p city-import --bin city-import -- extract.osm.pbf --id mycity \
    [--out data/city] [--origin LAT,LON] [--core LAT,LON] [--seed N]
```

Both write `<out>/<id>.road.bin`, `<id>.road.json` (debug dump), `<id>.greybox.glb` and
`<id>.greybox.b64.txt` (the same bytes base64-encoded, for the published testbed).
Port Vesper v2 (hills, terrain): `gen-city --preset vesper_v2 [--seed 1] [--json]` (`just city-v2`;
the JSON dump only with `--json`).

Greybox: one primitive per material (16-bit index chunks): `asphalt`, `highway_concrete`,
`parking`, `bus_lane` (lane ribbons, banked like the road surface), `sidewalk` + `kerb`,
`building` (prisms from `base_z_m`), `ground_<district>` (zone polygons on flat cities; on
terrain cities a 10 m terrain mesh, each vertex the minimum of the fine heights around it so it
never rises above a road, smooth normals), `rock`, `seabed`, `water` (sea plane at 0.25 m),
`retaining_wall` (road edges above the ground) and `bridge` (deck sides, parapets, piers).
Flat faces carry no normals (the testbed flat-shades them).
Prism features (format v4, solid or not) become `feature_<material>` primitives (one per
`Material`, colours from `wp_world::palette::material_rgba`, re-exported as
`glb::material_rgba`); box and cylinder features are drawn instanced by the testbed instead.
Big maps (terrain over 25 km², i.e. Port Vesper v3) draw render-terrain 2 × 2 blocks with no road
within 60 m as one quad (edge midpoints kept where a block meets full-resolution cells, so there
are no cracks); smaller maps are unchanged byte for byte. v3 with stub roads: 15.3 → 6.6 MB glb.
Format: see `crates/wp_world/README.md`.

## OSM mapping
| OSM | City data |
|---|---|
| `highway=motorway/trunk(+_link)` | `Highway` class (motorway/links one-way), 65/55/45 mph default |
| `highway=primary/secondary(+_link)` | `Arterial`, 45/35 mph default |
| `highway=tertiary/unclassified` | `Collector`, 35/30 mph default |
| `highway=residential/living_street` | `Local`, 25/15 mph default |
| `highway=service` + `service=alley` | `Alley`, 15 mph (other service roads, footways, paths, tracks are ignored) |
| `lanes`, `lanes:forward`, `lanes:backward`, `oneway` (`yes`/`-1`) | lane counts per direction (two-way `lanes=N` → ⌈N/2⌉ + ⌊N/2⌋) |
| `maxspeed` (`"25 mph"`, `"50"`, `"50 km/h"`) | speed limit; US defaults above when missing/unparseable |
| `turn:lanes`, `turn:lanes:forward/backward` | per-lane movements (`left`, `through`, `right`, slight/sharp variants) |
| `highway=traffic_signals` node at or within 25 m of a junction | signalised junction |
| `bridge`, `tunnel`, `layer` | node elevation ±6 m per layer (nodes shared across layers stay at 0) |
| `building=*` closed ways | extruded footprint: `height`, else `building:levels` × 3.2 m, else 1–8 storeys by type (seeded) |
| `landuse=residential/commercial/retail/industrial/railway/port/harbour` | district of the 200 m zone tiles they cover (otherwise by distance from the core) |
| `amenity=police`, `shop=car_repair`, `aeroway=helipad` nodes | POIs; plus 4 seeded safehouse candidates and race grids on the 3 longest arterial lanes |

Projection: equirectangular around the centre of the drivable nodes (or `--origin`), metres,
+X east, +Y north. Junction control, lane wiring and signal plans come from the shared
`wp_world::builder` (same rules as the procedural city).

## Attribution (required before shipping imported data)
OpenStreetMap data is © OpenStreetMap contributors and licensed under the
[Open Database License (ODbL) 1.0](https://opendatacommons.org/licenses/odbl/1-0/).
- Every imported network stores `attribution = "Map data © OpenStreetMap contributors,
  available under the Open Database License (ODbL) 1.0"`. The game must display it (credits
  screen and anywhere the map is shown, e.g. the captain's map view).
- The `.road.bin/.road.json` files are a **derivative database**: if we distribute them (or a
  game build containing them) we must make that database available under ODbL (or provide the
  means to recreate it — this importer plus the extract id/date) and keep the attribution.
  Rendered greybox meshes and gameplay built on them are "produced works": attribution only.
- Record the extract source and date in `RoadNetwork::source` (the importer stores
  `osm:<path>`; keep the original file name, e.g. Geofabrik's dated name).
- Do not mix in data from sources with incompatible licences (e.g. Google Maps).

## Tests

```sh
cargo test -p city-import
```
Covers `maxspeed`/`turn:lanes` parsing, US defaults, a synthetic signalised crossing extract
(lanes, speed, signal detection, one-way, building height, police POI, projection), ear-clipping
triangulation and the GLB container layout. The greybox for `procedural_12x12` was also checked
visually in Three.js (`GLTFLoader`).

## Known gaps
- **Not yet run on a real extract** (download hosts were unreachable from the build
  container); expect clean-up work on the first real city.
- Dual carriageways and complex junction clusters become several close junctions (no node
  merging yet); very short OSM segments get shrunk setbacks.
- Turn restriction relations, `access`, `junction=roundabout` semantics and parking tags are
  ignored (no parking lanes from OSM yet).
- Whole extract is held in memory (fine for city-size extracts).
