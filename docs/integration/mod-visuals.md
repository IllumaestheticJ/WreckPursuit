# Mod visuals integration notes

- **Workstream / prompt:** mods you can see (economy 08 + testbed 04 + harness 10 touch points)
- **Branch:** `claude/funny-hypatia-wq2ojr`
- **Crates / folders touched:** `wp_economy` (`visuals.rs`, `ModDef::visuals`), `data/mods`,
  `apps/simcli` (`EntitySpec::mods`), `wp_wasm` (`mod_visuals.rs`, session per-entity cars,
  `mods` embedded), `apps/testbed-web` (`render/mod_parts.ts`, `ui/modShop.ts`)
- **Contract version built against:** 0.3.0 (no contract change)

Installing a mod now changes both halves of the car:

1. **Physics** — a scenario entity with `mods: [...]` (simcli format v2) spawns as the modified
   car: `simcli::data::load_modded_car` = `wp_economy::mods::apply_mods` (validated set, only
   mods active in the scenario's mode), resolved against the tires/materials the mods reference.
   `BuiltSim::modded` / `Session::car(id)` return it, so `vehicle_info`, lattice calls and audio
   see the build. Replays carry it (the scenario is the replay header).
2. **Looks** — `wp_economy::visuals::BuildVisuals` lists the parts to draw, each with the body
   regions it is mounted on; the testbed fits procedural pieces to the car's panel mesh and adds
   them to `PanelBody` as ordinary skinned parts, so they dent, swing with hinged panels and fly
   off as debris with their region (front bumper → splitter, lip, bull bar; trunk → wing).

## Exposes

| Item | Signature / format | For |
|---|---|---|
| `wp_economy::visuals` | `PartKind` (28 kinds, `regions()`, `styles()`), `VisualPart`, `PartVisual`, `WheelVisual`, `BuildVisuals`, `mod_parts(&ModDef)`, `build_visuals(base, applied, mods)` | renderers, UI kit |
| `Economy::build_visuals` | `(&VehicleDef, &LatticeDef, &[String]) -> Result<BuildVisuals, EconomyError>` | any client |
| `ModDef::visuals` | `"visuals": [{"part": "RearWing", "style": "gt"}]` in `data/mods/*.json` (optional) | data authors |
| simcli `EntitySpec::mods` | `mods: ["tires_semi_slick", ...]` (default empty, skipped when empty) | scenarios, testbed, server later |
| `simcli::data::{load_mod_catalog, load_modded_car}` | catalog from `mods/<pack>.json`; modified `LoadedCar` | harness, tools |
| wasm | `mod_catalog_json(car)`, `build_visuals_json(car, mods_json)`, `entity_visuals_json(entity)` | testbed |
| testbed | `render/mod_parts.ts` `fitBuild(panels, build, wheels, lamps)`; `PanelBody.addPart / hubParts / bodyParts`; `__wp.runScenarioJson(json)`, `__wp.modShop` | testbed, e2e |

## Needs
- UI kit (M): the garage `ModTree` could show `parts` per mod (`mod_catalog_json` already
  returns them) and a 3D preview reusing `render/mod_parts.ts`.
- Net (F): `EntitySpec::mods` for networked players (the server spawns from the player's
  `OwnedCar.mods`); the client needs the same list to draw the parts.
- Cars / panels (R): real modelled parts could replace the procedural ones per car; the
  `PartKind` + region contract stays the same.

## Contract change requests
None (everything lives in `wp_economy`, simcli and the testbed).

## Changes outside my crates
- `apps/simcli`: `EntitySpec::mods`, `BuiltSim::modded`, `load_modded_car` (+ `wp_economy` dep,
  wasm-clean, no `native` feature). Owner 10 — please review.
- `crates/wp_wasm`: `build.rs` embeds `data/mods`; `Session` keeps per-entity modded cars.
- `apps/testbed-web/src/render/panels.ts`: `addPart` factored out of `build`, public
  `mesh/regions/restNodes/nodeRegions/mats/hubParts`, extra materials; a body disposed before its
  mesh loaded no longer builds (it skinned itself to the next session's lattice → NaN warnings).

## Perf numbers
Fitting a build is one-off at spawn (ray probes + nearest-node skinning, a few ms in SwiftShader).
Mod parts add a few hundred vertices per car to the per-frame panel re-skin.

## Known gaps
- Pieces are procedural low-poly shapes sized from each car's mesh, not modelled per car.
- Ride height is drawn by moving the body relative to the wheels (the sim models lowering as a
  lower CG and less travel; wheel hub positions do not change).
- Vinyls are a position-based shader overlay (no UV livery); detached panels drop their vinyl.
- Only scenario entities carry mods (police / traffic cars the sim spawns are stock).

## Every mod on every car (2026-10-26)
Owner: every mod should have a visual on every car where it is reasonable. `apps/testbed-web/e2e/
mod_coverage.mjs` checks it: for each car model (debug / parity models skipped) it renders the
stock car, then each visible mod of `data/cosmetics/visuals.json` alone (its prerequisites
installed on both sides, class-limited mods skipped), the way a career car gets it (sim build
parts + cosmetic look), from eight views (front, rear, side, top, low rear, front wheel, and two
with the body shell taken off for the engine bay and cabin), and reports the mods that change no
pixels. `VIEWS=` / `SHOTS_ALL=1` / `SHOTS=dir` make screenshot sheets for a look check.

Fixed from the first run (37 cars x 136 mods):
- Rear wings stood on the trunk box, which on an upright tailgate (hatchbacks, estates, vans,
  SUVs, the Erg Hamada) is the back face: the wing ended up inside the spare-wheel cage or the
  bodywork. Both renderers now put it on the roof's back edge there (on a pickup: the tailgate
  top), and hide the stock roof / hatch / lip spoilers it replaces.
- The "Reinforced Front Bumper" bull bar was a thin tube 6 cm in front of the bumper centre,
  hidden on most noses (corners, lower grilles, skid plates stand further out). It is now a steel
  beam with a nudge hoop placed in front of the most forward point of the nose (`frontMost`, which
  also counts grilles, lamps and skid plates). Push bar, radiator guard, fog lamps and fake front
  plates use the same.
- Push / bull bars replace the stock police push bar instead of sitting inside it, and are steel
  or gunmetal instead of black so they read on black police cars; the push bar mod is a heavier
  bar with headlight hoops so fitting it changes the car.
- Body kits (front / side / rear) on the career car are drawn by the cosmetic look (a reshaped
  bumper with intakes, blade and canards) instead of the build's thin lip strip
  (`LOOK_DRAWS` in vehicle_view.ts).
- Exhaust tips follow the stock outlet: side exits (rally raider, vans, pickups) get side-pointing
  tips, cars with tips on both sides keep both; the build's exhaust, diffuser and rear bar stand
  behind diffuser fins and step bumpers (`rearMost`). A bought diffuser replaces the stock one.
- Fake plates on cars modelled without plates (on the bumper face); bluer xenon lenses; skid
  plate lip shows under the nose; cabin pieces (shifter, handbrake, radar box, nitrous) stand on
  top of the one-block stock interiors instead of inside them.

Known gaps: brake kits are hidden behind closed stock rims (steelies, turbofans); sway bars and
dampers are under the car; fog lamps are small. Runs take ~2 h for all cars in SwiftShader (three
workers on 4 cores).
