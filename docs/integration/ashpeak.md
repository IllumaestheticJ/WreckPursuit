# Ashpeak County integration notes

- **Workstream:** Ashpeak County map overhaul (owner request 2026-10-22)
- **Branch:** `ws/ashpeak`
- **Crates / folders owned:** `crates/wp_world/src/vesper/ashpeak/`, `data/city/ashpeak.*`,
  `tools/map-render/`
- **Contract version built against:** 0.3.0 (no contract changes)

## Exposes
- City id `ashpeak`: Port Vesper plus Ashpeak County as one map. `wp_world::ashpeak::generate`
  / `generate_with_report` / `default_config`, `World::ashpeak`, simcli `CitySpec::ashpeak()`,
  server `load_city("ashpeak")`, wasm `scenario::city_spec("ashpeak")`, `gen_city --preset
  ashpeak`, `just city-ashpeak`.
- `vesper::Region` trait: hook for adding regions around Port Vesper without changing
  `vesper_v2` (which stays byte-identical).
- Road format v6: `RoadSurface::material` (`RoadMaterial::{Asphalt, Gravel, Dirt}`),
  `CityMap::road_material` / `lane_material`; `surface_at` returns Gravel / Dirt on unpaved lanes.
  New `BuildingKind::{Farm, Silo, Landmark, Construction}` and `DecorKind::{PineTree, Crane,
  JerseyBarrier, SiteFence, HayBale, Billboard}`.
- `wp_race::spots` (`RaceSpot`, `HonkTrigger`): honk-to-start race spots on both maps; wasm
  `Session::observe_horn`, testbed `race_spots.ts`.
- `acso` county sheriff livery, used by units spawned on the Ashpeak side.

## Needs
Nothing.

## Contract change requests
None.

## Changes outside my crates
- `wp_opponent`: speed profile grip scales with the surface under each corridor sample
  (paved courses are bit-identical; gravel / dirt corners are slower).
- `wp_police::Unit::spawn_pos`, `wp_sim/physics.rs` and `tools/city-import` (new decor kinds),
  simcli / server / wasm / testbed wiring for the `ashpeak` city id.

## Goldens
Eight new race goldens in `apps/simcli/golden/` (`race_ashpeak_downtown_checkpoint`,
`race_ashpeak_metro_circuit`, `race_crowfoot_ascent`, `race_crowfoot_descent`,
`race_grizzly_grade_sprint`, `race_harnell_dirt_run`, `race_route7_tunnel_sprint`,
`race_waterfront_drag`). No existing golden changed.

## Known gaps
- Harnell Flats farmland uses `DistrictId::Suburb` (no farm district in the contract).
- The greybox (`data/city/ashpeak.greybox.glb`) is about 25 MB.
