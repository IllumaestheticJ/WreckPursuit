# Car paint match integration notes

- **Workstream / prompt:** lead — owner report on v0.14.4: "the car is white in garage and
  orange in freeroam" (the junkyard beater, Free Roam, parked at Dockside Lock-up).
- **Branch:** `lead/car-paint-match`
- **Crates / folders touched:** `wp_economy` (ledger), `wp_wasm` (career, lots, lib), `apps/simcli`
  (scenario `EntitySpec`), `apps/testbed-web` (vehicle view, panel body, sim types, e2e).
- **Contract version built against:** unchanged (no `wp_contracts` edit).

## Root cause
A career car's colour never travelled with its session. The garage view drew
`CareerCarView::paint` (the car's `paint`, or for `paint: None` — the beater, dealer cars, old
saves — `FACTORY_PAINTS[uid % 12]`; uid 6 is white), while the Free Roam view was created with
the per-entity default (`COLOURS[id - 1]`, low-poly `PALETTE.carPaints[0]` = amber/orange) and
only repainted afterwards by `applyCareerLook` → `VehicleView.setPaint`. Any view built without
that call kept the default: the instant crash replay rebuilds every view on the way in and out
(`swapSessionViews`), so after a crash replay the career car stayed orange for the rest of the
run. Greybox also ignored `setPaint` on the panel mesh (it restored the mesh's construction
colour).

## Fix: one source
- `wp_economy::OwnedCar::body_paint()` — `paint` if set, else one fixed factory colour by uid.
  `wp_wasm::career::car_paint` (garage view) delegates to it.
- `simcli::scenario::EntitySpec::paint: Option<u32>` (presentation only, skipped when `None`, so
  every existing scenario / golden / replay is unchanged). `Career::car_session` (runs,
  races, touge, the workshop showroom) sets the player entity's paint to `car_paint`; a lot
  visit sets the own car's and each listing's.
- `vehicle_info().paint` exposes it; `VehicleView` takes it as its own paint at construction
  (all art styles, mesh factory tint ignored), so every view of the entity — crash replay,
  saved replay — draws it. The paint shop's preview / respray still call `setPaint`; the next
  session carries the saved colour.
- `PanelBody.applyStyle(lowpoly, paint, ownPaint)`: own paint is drawn in greybox too.
- Lot cars already stored their listing colour on purchase (`wp_economy::lots` `listing_car`).

## Tests
- `crates/wp_wasm/src/paint_tests.rs`: lot-bought car, street beater, resprayed car, cars with
  `paint: None` (dealer car, old save) — garage view == workshop bay session == Free Roam
  session; lot visit own car + listings.
- `apps/testbed-web/e2e/paint_match.mjs`: the beater in the garage bay, Free Roam (low-poly and
  greybox), through a crash replay and back, and after a respray; checks the materials and the
  body pixels' hue. Shots in `docs/media/paint-match/` (`00_before_fix_crash_replay_orange.png`
  is the bug as found).

## Changes outside my crates
`apps/simcli` struct literals (`fuzz.rs`, `police_bench.rs`, `examples/city_frame_cost.rs`) and
`wp_wasm::scenario::entity` gained `paint: None`.

## Known gaps
- Street-racer AI and touge NPC cars keep their own colour paths (level colour, roster paint).
