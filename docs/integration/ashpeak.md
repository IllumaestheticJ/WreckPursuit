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

## Fixes 2026-10-26 (owner: raised road blocking a direction, dividers, tunnel)
- **Meridian Plaza plinth over the streets.** `metro::landmarks` sized the plaza's 4 m paved
  plinth with `interior` marches that ran up Meridian Avenue / Grand Promenade into the T
  junction mouths (no road surface near), so it stopped 2 m from the street centrelines and
  covered the plaza-side lane of 5th St, 7th St, Ember St and Crowfoot Ave. Now `fit_rect`
  (clear of paved edges and junction discs): ~140 m square.
- **Freeway fan ends.** `divided.rs` fanned both carriageways right onto the line over
  `FAN_M`, so at Route 7 / Route 12's at-grade ends (Northline Parkway; Uptown Connector and
  Lantern Way in Port Vesper) opposing lanes overlapped for ~100 m and the forward median
  barrier crossed the backward lanes. Carriageways now close only until their lanes meet on
  the line; `surface::median_barrier_lines` drops any stretch of median edge lying on other
  paving (`inside_other_paving`), so the barrier ends where the carriageways close up.
- Talon Tunnel: a physics sweep (rays along every lane at 0.45 m / 1 m against all static
  colliders) found nothing solid in any tunnel lane.
- Regression tests in `vesper::ashpeak::tests`: no opposing lanes overlap, no median barrier
  on a lane, no building on a lane. Greyboxes regenerated (`vesper_v2` unchanged).

## Fixes 2026-10-27 (owner: "the ends of the highway the barriers are totally wonky")
- **Edge barriers at at-grade ends.** `surface::edge_barriers` widened every opening by
  `GORE_BARRIER_GAP_M` (40 m), junctions included, and measured junctions by their paved
  circle, which reaches far up a wide carriageway end. So the last 40–70 m before every
  freeway end on a street (Route 7 at Northline Parkway and Uptown Connector, Route 12 at
  Northline and Lantern Way) was unwalled, with a lone ~20 m stub of wall between the ramp
  taper and the end. Now a junction with a street opens only its node patch plus
  `JUNCTION_BARRIER_GAP_M`; ramp tapers, gores and highway-to-highway splits keep the 40 m.
- **Deck ends.** The barrier was cut on a whole surface-vertex step (up to ~10 m) where a
  bridge deck's rails end. The edge is now sampled every 2 m and the barrier starts where the
  rail stops (output lines still only have the surface's own vertices plus the run ends).
- **Stubs.** A piece shorter than `MIN_EDGE_BARRIER_M` (30 m) between two openings is
  dropped, unless it carries a deck rail or tunnel wall on.
- **Median.** `divided::FAN_M` 140 → 60 m (the carriageways only move in 1.5 m), so the
  median barrier runs to ~30 m from the junction instead of ~100 m.
- Test `vesper::ashpeak::tests::freeway_ends_are_walled_up_to_the_junction`. Greyboxes
  regenerated (both cities).

## Known gaps
- Harnell Flats farmland uses `DistrictId::Suburb` (no farm district in the contract).
- The greybox (`data/city/ashpeak.greybox.glb`) is about 25 MB.
