# data/city — city road networks

Owned by workstream 05. Per city `<id>`:

| File | What |
|---|---|
| `<id>.road.bin` | Road network (`WPRD` magic + format version + postcard). Load with `wp_world::World::load` / `RoadNetwork::load`. |
| `<id>.road.json` | Same data as JSON (debugging, diffing, tooling). |
| `<id>.greybox.glb` | Greybox mesh (roads, junction boxes, sidewalks + kerbs, district ground or terrain, buildings; city v2 adds sea, retaining walls, bridge decks/piers, ramp tapers and the highway median barrier; painted markings are drawn by the testbed from `wp_world::markings::marking_mesh`), glTF 2.0, +Y up, metres, 16-bit indices. World (x, y, z) = glTF (x, −z, y). |
| `<id>.greybox.b64.txt` | The same `.glb` bytes base64-encoded: the published testbed build cannot serve binary files and falls back to this (`apps/testbed-web/src/scene.ts`). Written together with the `.glb`. |

Format spec: `crates/wp_world/README.md`. Coordinates: metres, +X east, +Y north, +Z up.

- `procedural_12x12.*` — procedural grid city (flat), regenerate with `just city`
  (`gen-city --grid 12x12 --seed 1`). A test fails if the files are stale.
- `vesper_v2.greybox.{glb,b64.txt}` — Port Vesper v2 (curves, hills, terrain, highway loop;
  `wp_world::vesper`), regenerate with `just city-v2` (`gen-city --preset vesper_v2`). Its
  road network is generated in memory by `World::vesper` / scenario `city: preset(name:
  "vesper_v2", seed: 1)`, so `vesper_v2.road.bin` (2.3 MB, mostly terrain) is written locally
  but not committed. A test fails if the greybox is stale.

- `vesper_v3.greybox.{glb,b64.txt}` — Port Vesper v3 (in development; the full 8 × 7 km city
  with the river, highway network, airport, docks, rail and scenery; `wp_world::metro`),
  regenerate with `just city-v3` (`gen-city --preset vesper_v3`). Generated in memory by
  `World::metro` / scenario `city: preset(name: "vesper_v3", seed: 1)`; `vesper_v3.road.bin`
  (≈ 13 MB) is written locally but not committed. Box and cylinder features (trees, lamps,
  containers, parked cars, ...) are not in the greybox: the testbed draws them instanced
  from wasm `city_features()`. No stale-data test while v3 is in development: regenerate
  the greybox when the city changes (it is ≈ 29 MB + 38 MB base64).

Imported OSM cities carry an ODbL attribution in the file; see
`tools/city-import/README.md` before committing or shipping one.
