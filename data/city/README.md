# data/city — city road networks

Owned by workstream 05. Per city `<id>`:

| File | What |
|---|---|
| `<id>.road.bin` | Road network (`WPRD` magic + format version + postcard). Load with `wp_world::World::load` / `RoadNetwork::load`. |
| `<id>.road.json` | Same data as JSON (debugging, diffing, tooling). |
| `<id>.greybox.glb` | Greybox mesh (roads, junction boxes, sidewalks, district ground, buildings), glTF 2.0, +Y up, metres. World (x, y, z) = glTF (x, −z, y). |

Format spec: `crates/wp_world/README.md`. Coordinates: metres, +X east, +Y north, +Z up.

- `procedural_12x12.*` — procedural city, regenerate with `just city`
  (`gen-city --grid 12x12 --seed 1`). A test fails if the files are stale.

Imported OSM cities carry an ODbL attribution in the file; see
`tools/city-import/README.md` before committing or shipping one.
