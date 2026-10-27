# car-shapes integration notes

- **Workstream / prompt:** car-shapes, phase 1 (shared generator), owner complaint 2026-10-27:
  "very weird front windshields, no A-pillars, … improve accuracy / detail / personality".
- **Branch:** `ws/car-shapes` (from `main` at `58e83e32`)
- **Crates / folders owned:** `tools/assets` (car-assets), `data/panels/meshes` (regenerated),
  `docs/media/car-shapes`
- **Contract version built against:** 0.3.0 (no contract change)

## What changed (every car, render meshes only)
Physics is untouched: no change to `data/cars`, `data/lattices`, panel specs or any sim crate.

1. **Greenhouse cross-section** (`profile.rs`): the ring now has explicit roof-rail / pillar
   points (side glass 8→10, pillar or roof rail 10→13, glass or roof 13→17). The flat top (hood,
   deck) uses the same x positions, and a ring blends from flat to greenhouse over its first 5 cm
   of cabin height, so the windshield base and the deck joins no longer make fans of thin faces.
2. **A-pillars and C/D-pillars** in body colour, following the windshield / rear-glass rake up to
   the roof, 7.5 cm / 10 cm wide by default.
3. **Windshield**: inset glass with a black seal, a black cowl / wiper panel at its base, two
   parked wipers, a plan-view wrap (corners set back; drawn as a corner drop on the raked glass,
   capped so steep glass doesn't droop).
4. **Side glass**: every window inset with a black seal; the front door glass now runs forward
   under the A-pillar to a black sail (mirror patch); black B-pillar (knob: body colour); drip-rail
   line along the roof edge and down the pillars.
5. **Rear glass** framed and inset the same way.
6. **Common trim** (`trim.rs`): mirrors on the sail (body-colour housing, black base and arm,
   mirror glass), door handles on black recesses, panel seams (door cut lines and bottoms, hood and
   trunk shut lines, bumper splits), fuel filler door, body-colour arch lips, black arch liners
   (wheel wells no longer open), rounded nose / tail face edges.
7. Style kits that draw their own mirrors / handles / fuel fillers / drip rails remove the generic
   ones first; kit and traffic mirrors now sit on the sail too. Kit and traffic wipers were
   removed (the generic ones fit the cowl and the inset glass). `supercar.rs`'s own door seams
   were removed (generic seams cover them).

Contracts kept: every new part is skinned to the region of the panel it sits on (door glass,
seals, handles and mirrors ride with the doors; seals stay when glass shatters); glass parts keep
the `glass` material and kind (shatter hides them by region as before); painted parts get livery
atlas UVs; fold bindings unchanged; `cargo test -p car-assets` (incl. the soft-clamp sweep, now
with every new knob at both extremes) and `validate-car-asset` pass.

## New style knobs (`car_assets::style::BodyStyle`)
All soft-clamped like the existing knobs; defaults apply to every car.

| knob | unit | range | default | what |
|---|---|---|---|---|
| `a_pillar_w_m` | m | 0.04…0.14 | 0.075 | A-pillar width seen from the front |
| `c_pillar_w_m` | m | 0.04…0.30 | 0.10 | C/D-pillar width seen from behind (blends to the A-pillar along the roof) |
| `b_pillar_w_m` | m | 0.05…0.25 | 0.12 | B-pillar width in side view |
| `b_pillar` | `TrimFinish` | `Black` / `Body` | `Black` | B-pillar finish |
| `a_sail_len_m` | m | 0.04…0.40 | 0.14 | sail at the A-pillar foot, ahead of the front door glass |
| `a_sail` | `TrimFinish` | `Black` / `Body` | `Black` | sail finish |
| `roof_rail_drop_m` | m | 0.02…0.10 | 0.05 | roof top → side-glass top edge |
| `glass_recess_m` | m | 0…0.03 | 0.012 | glass inset behind the surface |
| `glass_frame_m` | m | 0…0.04 | 0.018 | black seal width round each glass |
| `windshield_wrap_m` | m | 0…0.20 | 0.06 | windshield corners set back in plan |
| `rear_glass_wrap_m` | m | 0…0.20 | 0.04 | rear-glass corners set forward in plan |
| `cowl_len_m` | m | 0…0.20 | 0.07 | black cowl panel length (0 = none) |
| `wipers` | count | 0…2 | 2 | parked wiper blades |
| `drip_rail` | `TrimFinish` | any | `Black` | drip-rail line (`None` = off) |
| `door_handles` | `TrimFinish` | any | `Body` | handle bar finish (`None` = off) |
| `mirrors` | `TrimFinish` | any | `Body` | mirror housing finish (`None` = no generic mirrors) |
| `panel_seams` | bool | | true | seam lines |
| `seam_w_m` | m | 0.003…0.012 | 0.006 | seam line width |
| `fuel_door` | `FuelDoor` | `None` / `Left` / `Right` | `Right` | fuel filler side |
| `arch_lip_m` | m | 0…0.04 | 0.016 | arch lip radial width (0 = none) |
| `arch_liners` | bool | | true | black liners inside the arches |
| `end_round_m` | m | 0…0.05 | 0.03 | nose / tail face edge rounding |

New types: `TrimFinish { None, Body, Black, Chrome }`, `FuelDoor { None, Left, Right }`.
Helpers for per-car styles: `BodyProfile::{front_window, sail, pillar_w, greenhouse_weight,
greenhouse_crown}`, ring index constants `RING_GLASS_BASE`, `RING_GLASS_TOP`, `RING_PILLAR_IN`,
`trim::remove_generic` + `GENERIC_*` part-name lists. Part names: `data/panels/README.md`
"Greenhouse and common trim".

## Budgets and render cost
Triangles +0.8k–1.5k per full mesh (seals ~0.4k, arch lips ~0.2k, seams ~0.25k, the rest small);
the mid LOD barely changes (flat glass, no seams or small trim there). Budgets raised, with
measured numbers in `tools/assets/README.md`:

| | before | now |
|---|---|---|
| Hero tier | 8k–20k | 8k–22k (cars at 16.9k–21.0k) |
| Traffic tier | 4k–10k | 4k–11k (cars at 8.2k–10.6k) |
| Mid LOD | 2k–6.5k | 2k–7k (cars at 3.9k–6.5k) |
| `TRI_BUDGET` hard range | 4k–20k | 4k–22k |

Desktop chase (`e2e/frame_profile.mjs chase 12`, 1280×720, low-poly dark, SwiftShader, this
box): old meshes 340k triangles, 409 draw calls, 227 ms / frame, RTF 0.91; new meshes 347k–350k,
413–417, 233–252 ms, RTF 0.87–0.90 (two runs). Gates: triangles ≤ 400k, draw calls < 600 hold;
RTF ≥ 0.9 is machine-dependent and this box sits at the line in both builds.

`every_car_has_turn_signals_integrated_into_its_lamps` caps growth at +2 % of a per-car table;
the table was rebased to the new counts. A per-car detail pass that adds more than 2 % updates
its car's row.

## Checks run
`cargo fmt`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test -p car-assets
-p wp_economy`, `cargo build --target wasm32-unknown-unknown -p wp_wasm`, `npm run wasm && npx tsc
--noEmit -p .`, `validate-car-asset` (39 cars ok), `gen-car-mesh --check`.
`node apps/testbed-web/e2e/mod_coverage.mjs kestrel_ts ombra_v12 hauler_1500`: 8 car/mod pairs
show nothing, all on the known list in `mod-visuals.md` (short / sequential shifter, radar, sport
tyres on the exotic, the Ombra halo, front sway bar on the truck); bumpers, lips, wings and
vinyls still show.

## Shots
`docs/media/car-shapes/{before,after}/<car>_{front,rear,side,top,cabin,cabin_rear}.jpg` for
tansy_13 (hatch), debug_sedan and kestrel_ts (sedans), shiro_turbo (coupe), ombra_v12 (mid-engine
exotic), ironside_440 (muscle), hauler_1500 (pickup), drayman_cargo (van), sentinel_bastion
(SUV), sentinel_cruiser (police); side-by-side sheets in `docs/media/car-shapes/compare/
{cabin,front,side}.jpg`. `tools/assets/mesh_shots.mjs` now also writes the cabin close-ups and a
custom view (`SHOT_CAM`, `SHOT_AT`).

## Changes outside my crates
- `data/panels/meshes/**` regenerated; the six `parity_*` copies refreshed with
  `tools/parity/build_parity_cars.py`'s `copy_panels` only (parity car physics files untouched).
- `data/panels/README.md`: new section "Greenhouse and common trim".

## Known gaps / next (per-car sessions)
- Every car uses the default pillar widths, wrap, sail and trim finishes. Per-car sessions should
  tune them (e.g. chrome handles and body-colour B-pillars on 70s cars, black mirrors on
  trucks, long sails on mid-engine cars whose windshield base is far ahead of the door hinge).
- A few style overlays were tuned for the old frameless glass and should be revisited: the black
  A-pillar strokes / roof sheets on supercars, `kagero_hx` and `nyx_proto`.
- The windshield's plan wrap is a corner drop on the raked glass, not a true plan curve of the
  cowl line (the loft is station-based).
- The C-pillar foot (where the cabin meets the deck) still shows a few small facets.
- Pre-existing, not from this pass: z-fighting stripes from the police decal patches in the
  greybox shot renderer, a floating trim piece behind the drayman van's rear corner.

## Lead feedback
