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

## Phase 3 knobs (shared generator, 2026-10-27)

Phase 3 (branch `ws/car-shapes`, brief in the lead's session prompt) adds the shapes the
phase-2 per-car sessions could not make: non-rectangular plan views, sculpted and curved sides,
fender tops above the hood, short overhangs and lamps that follow the surface. **Every knob's
default changes nothing**: with all of them at their defaults the generated meshes were
byte-identical to the phase-1 meshes (`gen-car-mesh --check`) before the separate item-6 commit
(see "Smoother bodies" below). Render meshes only: no `data/cars`, `data/lattices` or sim
change.

### Knob list (`car_assets::style::BodyStyle`, all soft-clamped)

| knob | unit | range | default | what |
|---|---|---|---|---|
| `plan_width` | `Vec<[y_m, scale]>` | scale 0.80…1.05 (`PLAN_SCALE_RANGE`) | empty | half-width scale along the car (plan view) |
| `side_sculpt` | `Vec<SideSculpt>` | `dx_m` −0.08…+0.06 (`SCULPT_DX_RANGE`), summed too | empty | local side-wall offsets over a y × z box with soft edges |
| `side_bulge_m` | m | 0…0.06 | 0 | convex side section (outward bow between sill and belt) |
| `side_bulge_z` | fraction | 0.2…0.8 | 0.55 | height of the bulge peak, sill (0) → belt (1) |
| `rocker_in_m` | m | 0…0.08 | 0 | rocker / sill corner tucked in under the side |
| `tumblehome_m` | m | 0.03…0.36 (`TUMBLEHOME_M_RANGE`) | `None` | wider-range tumblehome; `Some` overrides `tumblehome` |
| `front_fender_rise_m` | m | 0…0.12 | 0 | front fender tops above the hood centre, wheel → nose |
| `rear_fender_rise_m` | m | 0…0.12 | 0 | rear fender / haunch tops above the deck, wheel → tail |
| `fender_peak_w` | fraction | 0.5…0.95 | 0.80 | where the fender peak starts (of the body half width) |
| `overhang_trim` (range widened) | m | −0.35…+0.25 each (`OVERHANG_RANGE`) | (0, 0) | was −0.15…+0.10 |

New type `SideSculpt { y0_m, y1_m, z0_m, z1_m, dx_m, falloff_m }` (chassis metres;
`SideSculpt::dx_at(y, z)` gives its offset). New module `car_assets::lamp` (projected lamps) and
`Kit::proj_cluster` in `styles/performance/kit.rs`. New `BodyProfile` helpers: `plan_scale(y)`,
`fender_rise(y)`, `fender_top(y, x)`, field `tire_outer_x`. `mesh::loft_rings(p, base, fine)`
exposes the loft's stations and rings.

### How to use each (for the per-car sessions)

Station `y` values are chassis metres (+ forward). Take wheel stations from your def
(`def.wheels[i].position_m[1]`, radius `radius_m`) or from `BodyProfile` (`p.arches`, `p.y_nose`,
`p.y_tail`, `p.sill(y)`, `p.belt(y)`); build the profile in your style function with
`BodyProfile::styled(def, lattice, &body)` only if you need it, or compute from the def.

- **Plan-width curve (`plan_width`).** Control points `[y, scale]`; smoothstep between points,
  held past the ends. Narrow nose: `[[front_wheel_y + r, 1.0], [y_nose, 0.88]]`. Tapered tail /
  boat-tail: the same at the rear (`0.85…0.92`). Coke-bottle waist: `1.0` over both arches,
  `0.92…0.95` at the doors (wide flares hide a small waist). Pickup fenders wider than the cab and bed: `1.0` over the arches, `0.93`
  from 0.45 m behind the front arch to 0.45 m ahead of the rear one. Limits: 0.80…1.05; over a
  wheel the body never comes inside the tire's outer face + 1 cm (tested), so arches stay
  skinned. The hood, deck, end faces and every kit anchor that reads `half_width` (`Kit::wn`,
  `wt`, generic lamps) follow; the greenhouse keeps its own width. Kit parts placed from fixed
  x values (side steps, skirts) do not follow: re-place them from `half_width(y)`.
- **Side sculpts (`side_sculpt`).** Each moves every side ring point (underside edge to
  shoulder) in the `y0..y1 × z0..z1` box by `dx_m`, easing out over `falloff_m`. Side pod stepping
  in behind the front wheel: `y` from the front arch's rear edge back 0.8 m, `z` sill → 70 % of
  the side, `dx −0.04…−0.07`, falloff 0.15. Door cove / scallop: short `y`, mid-height `z`,
  `dx −0.03…−0.045`, falloff 0.10–0.12. Pontoon fender standing proud: `y` over the arch ± 0.3,
  `z` from mid-side to the belt, `dx +0.04`, falloff 0.2 (pair with a fender rise). Shoulder /
  character line: a 3 cm `z` band 7–10 cm under the belt, the whole length, `dx +0.015…+0.025`,
  falloff 0.03–0.04. The shoulder never moves inside the glass base. The loft has fixed ring
  points (sill corner, rocker, three crease points, two shoulder points), so features thinner
  than the gaps between them (≈ 5–15 cm) blur: use the crease (`crease_z`) to put ring points
  where you want a sharp line.
- **Section shape.** `side_bulge_m` 0.02–0.04 turns slab sides into a curved section (peak at
  `side_bulge_z`; 0.6 for a high, rounded 90s side). `rocker_in_m` 0.03–0.05 tucks the sill under.
  `tumblehome_m` replaces `tumblehome` with a 0.03…0.36 range: vans / SUVs / pickups with
  near-vertical glass use 0.04–0.06 with `glass_inset` 0.04; a prototype canopy 0.30–0.36.
- **Fender tops (`front_fender_rise_m`, `rear_fender_rise_m`, `fender_peak_w`).** Raises the
  outer band of the hood / deck and the shoulder by the rise, from the wheel's arch to the nose
  (front) or tail (rear), fading in over 0.3 m on the cabin side; the nose fall still applies.
  Use it instead of `hood_valley` for peaks over a low hood (prototype, GT, long-hood classics):
  the hood centre and the nose stay put, so nothing rises in front of kit head lamps
  (`fender_rise_keeps_kit_lamps_visible` runs nyx / morrigan / ironside / kestrel at 0.12 with
  valleys at 0). Keep lamps either fully on the peak's plateau (|x| ≥ `fender_peak_w` × half
  width) or fully on the hood: a coarse patch (e.g. a `(2, 2)` `top_lamp`) straddling the band's
  bend cuts under it. The band is measured on the body's full half width, so it never grows
  toward the nose at a fixed x (no lip in front of lamps). `hood_valley` / `deck_valley` are
  unchanged (engine-cover valleys).
- **Overhangs (`overhang_trim`).** Now −0.35…+0.25 m per end. The nose / tail stop 6 cm short of
  the tires; past the lattice hood front (quarter end) the visual hood front (quarter end) moves
  back with them, so the bumper panels always keep a 10 cm strip on their region (their lattice
  nodes then sit a little ahead of / behind the mesh, which skins fine). A pre-runner pickup can
  use −0.30…−0.35 at the front. Both ends at +0.25 on a car that is already long can exceed
  `validate`'s ±15 % length sanity check.
- **Projected lamps (`car_assets::lamp`, `Kit::proj_cluster`).** Draw a lamp as a 2D outline in
  a view and it is cast onto the body triangles already built (the loft runs before style
  details), so the lens sits exactly `proud_m` (8 mm) over the drawn surface:
  - `LampView::Front` / `Rear`: outline `[x, z]` (signed x); `yaw_deg` (0…60) swings the cast
    toward the outline's side, so a lens near the corner wraps round onto the fender / quarter
    (20–35° for wraparound lamps). `LampView::Top`: `[x, y]`, lies on the hood / fender top
    (swept lamps). `LampView::Side { right }`: `[y, z]` (side markers, wraparound tail ends).
  - `Kit::proj_cluster(sx, front, view, left_outline, Some(seg), rear_mat, bezel_w, yaw_deg)`
    puts the lens into `headlight_*` / `taillight_*` and cuts the turn signal out of it as
    `indicator_front_*` / `indicator_rear_*` (the renderer's lamp slots and the turn-signal
    contract keep working; `Seg::Outboard / Inboard / Lower / Upper` as in `front_cluster`; on a
    top view `Lower` is the leading strip). `bezel_w > 0` adds a flush black bezel
    (`headlamp_bezel_*` / `taillamp_bezel_*`, `FrameFront` / `FrameRear`, trim).
  - Lower level: `lamp::lens(b, p, key, &Lamp)`, `lamp::bezel(...)`, `split_outline` /
    `clip_outline`, `ellipse` for round / oval lamps. Outlines must be star-shaped round their
    centroid (convex, ovals, teardrops, strips); make C / boomerang shapes from two lenses.
    Points past the silhouette are pulled in until they hit.
  - Rules that keep lenses visible from ahead (`lights_sit_outside_the_shell`): keep a
    front-view outline inside the nose half width (`Kit::wn()`; let `yaw_deg` do the wrap) and
    under the top edge of the nose face (an outline across a sharp edge lands on two surfaces and
    the lens chords cut it); give the signal segment some width on the face (≥ 8–10 cm when it is
    the outboard end); on a top-view lamp put the signal at the outer end or the rear (a leading
    strip shadows the lens from ahead). Faces that would end up under the body (a chord across a
    rounded corner) are pushed out automatically.
  - Cost: a lens ≈ 2 × points × rings + 2 × points triangles (8…32 points, 1…3 rings, ≈ 100–250);
    a bezel 4 × points. Replacing all four kit lamps with projected lenses and bezels cost
    +470…+560 triangles in the demo (kestrel, vireo, nyx).

### Demo and sheet
`cargo run --release -p car-assets --example phase3_demo -- <dir>` writes demo variants (not
the cars' styles) of kestrel_ts, vireo_xs, hauler_1500, nyx_proto and ironside_440;
`MESH_DIR=<dir> node tools/assets/mesh_shots.mjs <out> <car>__<variant> …` renders them
(`mesh_shots.mjs` now takes `MESH_DIR`; an id `<car>__<variant>` uses `<car>`'s def). Sheets:
`docs/media/car-shapes/phase3/{plan_width,sculpt_section,fender_rise,overhang,lamps_front,
lamps_rear}.jpg` (base on the left, knob on the right).

### Tests added
`style_knob_extremes_never_panic` sweeps every phase-3 knob far past both ends (NaN control
points included); `phase3_knobs_keep_panels_on_their_regions` (every car, strong in-range
settings, both overhang extremes: bind-skin + `validate`, budgets aside);
`plan_width_keeps_the_body_outside_the_tires`; `fender_rise_keeps_kit_lamps_visible`;
`projected_lamps_sit_outside_the_shell_and_keep_the_signal_contract` (front-view wrap with yaw,
top-view swept lamps, wraparound tails, on a sedan, a hatch, the prototype, the pickup and the
police sedan: lights check, turn-signal contract, end-lamp binding, bezels); `lamp.rs` unit
tests (split / clip, resample, a lens proud of a box).

### Not covered here (phase-2 requests outside this brief)
Nose / tail face rake in side view and a plan bow of the face, tailgate lean, a quarter-glass
knob (hatch group); a forward-leaning shark nose (muscle); a C-pillar window kink (tuner); nose
height above the lattice hood line (traffic, police SUV); a shared `face_poly` / `top_poly`
(several groups; `lamp.rs` covers lamp shapes, not general panels).

## Lead feedback
