# Landmark buildings integration notes

- **Workstream:** lead — landmark buildings for the drive-to places (owner, 2026-10-29: "the
  dealership icons look nothing like dealerships. They should be put on the map with an actual
  dealership - same with police stations, other landmarks, and garages.")
- **Branch:** `lead/landmark-buildings`
- **Crates / folders touched:** `wp_world` (`places`, two appended enum families in
  `network`, the hook at the end of `vesper::generate_region`), `tools/city-import`
  (materials, tests), `wp_wasm` (map icon field, `place_layouts_json`), `apps/testbed-web`
  (sign text, string flags, palette), `apps/ui-kit` (map icons, legend), `data/city`
  (regenerated greyboxes), `docs/media/landmarks`.
- **Physics:** untouched. The buildings are ordinary `Building`s and `Structure`s, which
  `wp_sim` already turns into convex-hull colliders.

## What each place looks like

| Place | Built from | Pieces |
|---|---|---|
| Dealership (Budget Motors, Harbourline Auto, Meridian Prestige) | generated | glass showroom (lit front window and side returns, name fascia, entrance canopy), four display cars angled to the road, three flag poles, pylon sign with the name (both faces) |
| Used-car lot (Lucky Lou's, Northgate Certified Pre-Owned) | generated + props | two rows of four cars, sales cabin with lit window and "SALES" board, price board with the name; string flags between six poles (testbed) |
| Junkyard (Dockside Salvage, Ridgeback Scrap & Pull) | generated | 2.6 m corrugated fence with a 13 m gate, crushed wrecks stacked 1–3 high, crane with boom and magnet, yard office, name board |
| Repair shop (5 `Garage` POIs) | generated | workshop with three bays: two closed roll-up doors, the middle bay open and lit with a car on a two-post lift, fascia sign with the shop name, door lamps |
| Police station (6 `PoliceStation` POIs) | generated | two-storey civic block with window bands, portico on pillars, "POLICE" on the portico and the façade, lit entrance, blue lamp, flagpole, four cruisers (white, red / blue light bar) nose-in out front |
| Safehouse (9 `SafehouseCandidate` POIs) | generated | an unmarked warehouse with one roll-up door and a dim lamp |
| Dockside Lock-up | generated | three lock-up units, the middle one open and lit, name over it |
| Spire Basement Garage | generated | 44 m tower over a ramp portal (wing walls, roof slab, dark ramp floor, lit mouth, raised barrier arm, name sign) |
| Harnell Barn | generated | gabled red barn, pale barn doors to the road, lamp, sign on posts |

Sign text (made-up names, or the generic word "POLICE") and string flags are drawn by the
testbed (`render/place_signs.ts`) from `place_layouts_json()`; everything else is greybox
geometry with colliders. No real names, logos or chain architecture.

Other named landmarks: every named landmark of the world data already stands as geometry
(Lantern Tower, Vesper Point Lighthouse, Hollins Observatory, Calder Mill Chimney, Gantry Cranes
1–3, Larkspur Water Tower: their structures; Iron Bridge and Hollins Tunnel are roads; Ashpeak's
Meridian Spire, stadium and Halcyon Observatory are landmark buildings), checked by
`landmark_places.rs`. Rest stops have their car parks and helipads sit with their precinct.
**No other landmark buildings were added.**

## Map icons
`places_json()` entries carry `icon`. The kit's `drawPoi` draws: a wrench on amber for repair
shops (was a "G"), a blue shield with a white star for police stations (was a "P"; now also on
the minimap), a car under a showroom roof on pink for dealers, a car with a "$" on yellow for
used lots, a stack of crushed cars on rust for junkyards, a roll-up garage door for the
player's garages (lots and homes were pink "$" / white "H" dots). Legend rows added.

## Placement
`wp_world::places::add_place_buildings` (last step of Port Vesper v2 and Ashpeak generation):
the ring is the POI, or for lots / homes the lane point nearest the anchor
(`DRIVE_TO_SPOTS`, the same anchors as `wp_wasm::garage_visit::{WORLD_LOTS, HOME_GARAGES}`,
checked by a test). The plot sits beside the ring's road (the ring lane's side first), centred
on the ring when it can (else offsets up to ±25 m), clear of every road + sidewalk, junction,
car park, water, protected building and other plot; the pieces are pushed back until none is
within 15.5 m of the ring, so the ring stays the forecourt you drive into. Generic buildings
and decor on the plot are removed. Ashpeak keeps Port Vesper's pieces where the plot is the
same, so they ship once (shared part).

## Greybox sizes (glb)
| File | Before | After |
|---|---|---|
| vesper_v2.shared | 4.83 MB | 5.00 MB |
| vesper_v2.own | 7.67 MB | 7.67 MB |
| ashpeak.county0 | 6.20 MB | 6.24 MB |
| ashpeak.county1 | 6.43 MB | 6.45 MB |
| ashpeak.county2 | 6.38 MB | 6.41 MB |
| ashpeak.county3 | 6.43 MB | 6.48 MB |
| ashpeak (monolithic, native only) | 30.21 MB | 30.48 MB |
| vesper_v2 (monolithic, native only) | 12.47 MB | 12.64 MB (test cap 12.5 → 12.75 MB) |

Ashpeak browser download (shared + counties): +0.31 MB. Every county part stays under the
9 MB cap. About 1 030 place pieces in Ashpeak (≈ 12 triangles each).

## Tests
- `tools/city-import/tests/landmark_places.rs` (Port Vesper v2 and Ashpeak): every place laid
  out; its building in the network within 45 m of the ring; no piece on a lane, sidewalk,
  junction or inside any ring; collider counts match and a ray down onto every building and
  piece hits it; place materials in the greybox.
- `wp_world::places` unit tests: pieces inside their plots, signs where expected.
- `wp_wasm::spawn_tests::every_drive_to_place_has_its_building_by_its_ring`: anchors / names /
  kinds match the career data, every lot, home, shop and safehouse ring of a session has its
  building near it.

## Known gaps
- Display cars, cruisers and wrecks are low-poly greybox boxes (solid, static), not the real
  car models.
- The Spire basement ramp is a lit portal over a dark floor; the ramp does not go down into
  the ground.
- The sign text is a flat label (canvas texture); small signs are hard to read at range in the
  low-resolution SwiftShader screenshots.
- Procedural / OSM / touge cities get no place buildings.
