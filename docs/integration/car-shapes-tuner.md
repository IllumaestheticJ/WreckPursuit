# car-shapes phase 2: `tuner` group

- **Workstream:** car-shapes phase 2, group `tuner` (brief: `docs/integration/car-shapes-phase2-brief.md`)
- **Branch:** `ws/car-shapes-tuner` (from `origin/ws/car-shapes`)
- **Cars:** `kestrel_ts`, `arashi_t4`, `talus_20t`, `shiro_turbo`, `hakone_gt`, `sperber_32`
  (`debug_coupe` untouched; it lives in the same style file and its mesh is unchanged)
- **Files changed:** `tools/assets/src/styles/performance/tuner.rs` (style file),
  the six cars' `data/panels/meshes/<id>.json` + `meshes/mid/<id>.json`, the parity copy of
  kestrel (see below), the six rows of the turn-signal triangle table in
  `tools/assets/tests/assets.rs`, shots in `docs/media/car-shapes/tuner/`.
- **Render meshes only.** No change to `data/cars`, `data/lattices`, panel specs, the sim, or any
  shared generator file (style.rs, profile.rs, mesh.rs, builder.rs, wheels.rs, mechanical, trim,
  kit.rs). Wheel positions / sizes come from the car defs; every panel stays on its lattice region.

## What changed for every car in the group
- **Own body and kit per car.** `arashi_t4` was a recolour of the kestrel (same body and kit);
  it now has its own body knobs and kit. Each car has its own lamp shapes, grille, intake and
  tail pattern.
- **Shaped lamps and grilles (local helpers in `tuner.rs`, no shared change):**
  `face_poly` (convex outline lens / grille / intake on the nose or tail face, instead of boxes),
  `top_quad` (thin shell following the top surface over any plan quad) and `swept_top_lamp`
  (a head lamp on the nose top whose outboard edge runs back into the fender, its rear edge a
  diagonal, with the amber turn-signal segment at the inboard end and a dark housing round it),
  `blank_badge` (octagonal chrome badge with no lettering), `rear_reflectors`.
- **Sedan proportions.** The four-door cars had a ~0.7 m roof that read as a coupe. Roofs are now
  0.84–0.92 m long (`roof_len_trim` +0.15, steeper rear glass) and the cowl sits a little higher
  (`cowl_h`), so the windshield is shorter and the cabin reads as a saloon.
- **Stance.** `arch_gap` 0.07 → 0.045 on all six: the tyres fill the arches (was a big black gap
  over each wheel).
- **Proportion pass** (after the lead's 2026-10-27 heads-up): shorter visual overhangs
  (`overhang_trim`, front −0.08…+0.02, rear −0.04…−0.08), more sill tuck-under and shoulder
  rounding (less slab-sided sections), bigger haunches on talus / hakone / sperber, lower noses
  on kestrel / shiro / hakone, lower roofs on shiro / hakone. The shortened ends drop the
  bumper-split seam lines on some cars (the split station now sits at the mesh end).
- **Greenhouse knobs** per car (pillar widths, glass wrap, B-pillar width on the coupes).
- Generic door handles and fuel doors (phase 1 trim) are used instead of the old kit boxes,
  with the finish and side set per car (`door_handles`, `fuel_door`). Kit mirrors stay.

## Per car

References are generic descriptions of the real-world body type the analog stands for
(dimensions from the car's spec sheet, `data/cars/<id>.spec.md`; cues from period photos of the
class).

### `kestrel_ts`: mid-2000s Japanese turbo sport sedan, rally-bred look
- **Reference:** a mid-2000s Japanese compact four-door sport sedan with a turbo four (a rally
  homologation family): 4.60 × 1.77 × 1.45 m, wheelbase 2.64 m, overhangs ~1.0 m each end, high
  belt line (glass ≈ a third of the height), ~60° windshield. Cues: swept lamps that run back
  into the front fenders, hexagonal upper grille with a bright bar, wide trapezoid lower mouth
  with fog pods in the corners, top-mount intercooler scoop, tall pedestal boot wing, round
  tail elements, roof vent, mud flaps, gold multi-spoke wheels.
- **Changes:** swept surface-following head lamps (amber inboard segment) on dark housings
  replace the rectangle + projector boxes; hexagonal grille with chrome bar and blank badge;
  trapezoid lower mouth with mesh; fog pods; bigger tail clusters with chrome-ringed round
  elements; rear reflectors; boot badge. Body: belt +3 cm, 60° windshield, slight roof arc,
  longer roof, wider C-pillar, higher cowl, rounder nose corners.

### `arashi_t4`: mid-90s Japanese AWD rally-homologation saloon
- **Reference:** a mid-90s compact Japanese AWD turbo saloon: 4.33 × 1.69 × 1.42 m, wheelbase
  2.51 m; boxy upright three-box body, tall greenhouse with thin pillars, near-vertical nose,
  ~56° windshield. Cues: square rectangular head lamps with amber corner lenses wrapping round
  the fender, a slim slot grille, a deep bumper with a central intake and big round driving
  lamps, bonnet scoop, tall boot wing on pedestals, wide rectangular tails joined by a red
  reflector panel across the boot, mud flaps, six-spoke wheels.
- **Changes:** its own body (upright 10° nose, 56° / 48° glass, boxier shoulder and tail, thin
  A-pillars, longer roof) and its own kit (`arashi_kit`): square lamps with a divider and amber
  corners that wrap onto the fenders, slot grille with blank badge, central intake with slats,
  big round driving lamps, offset tow hook, pedestal wing, full-width red tail band (two ribbed
  tails + reflector garnish), plate recess, reflectors. Silver six-spoke wheels (was the
  kestrel's multi-spoke).

### `talus_20t`: late-90s Japanese AWD rally-homologation saloon
- **Reference:** a late-90s Japanese four-door rally homologation saloon: 4.35 × 1.77 × 1.42 m,
  wheelbase 2.51 m; upright late-90s greenhouse, near-vertical nose, round blistered arches.
  Cues: big angular clear head lamps, a front bumper that is mostly intercooler mesh with fog
  lamps in its outer corners, flat bonnet with twin vents and a central duct, a big two-plane
  boot wing on tall stands, tall rectangular tails with clear reverse lamps inboard.
- **Changes:** the quad round lamps on a black panel (an earlier-era look) become big angular
  clear lamps (inner lower corner cut, amber outer corner, twin reflector bowls); a narrow grille
  with blank badge; a six-sided intercooler mouth with metal mesh and fog pods; bonnet duct;
  two-plane wing; tails with lower amber strip and clear reverse lamps, black centre panel with
  badge. Body: belt +3 cm, longer roof, higher cowl.

### `shiro_turbo`: early-2000s Japanese FR turbo coupe in drift trim
- **Reference:** an early-2000s Japanese FR 2+2 coupe with a turbo four: 4.45 × 1.70 × 1.29 m,
  wheelbase 2.53 m; long cabin and short boot, fast ~62° windshield and rear glass, wedge belt.
  Cues: sharp swept lamps reaching far back along the fender tops, wide lower mouth, horizontal
  tails with clear sections and round elements. Drift-scene trim: vented carbon bonnet, lip with
  canards, bolt-on box flares, GT wing on swan necks, deep-dish mesh wheels, vinyl band.
- **Changes:** sharp swept lamps (longest sweep of the group); a wide six-sided lower mouth with
  mesh and a slim upper slot; tails taper down toward the plate (chrome housing and clear lens
  as shaped outlines, round red and amber elements); smaller swan-neck wing; boot badge and
  plate recess. Body: belt +5 cm with a 3 cm wedge, rounder roof arc, wider C-pillar, narrow
  B-pillar (coupe quarter glass), more wrap on both glasses.

### `hakone_gt`: 90s Japanese twin-turbo FR grand tourer liftback
- **Reference:** a 90s Japanese twin-turbo straight-six GT liftback: 4.52 × 1.81 × 1.28 m,
  wheelbase 2.55 m; long low nose, cabin set well back under a bubble roof that narrows into a
  teardrop canopy, a big glass hatch to a high tail, soft rounded flanks, fat rear haunches.
  Cues: triple projector lamps under long clear covers, a big rounded central mouth with a
  body-colour bar, a hoop wing over the hatch, quad round tails in a body-wide smoked panel.
- **Changes:** long swept lamp covers over the triple projectors; octagonal central mouth with
  mesh, body-colour bar and badge; shaped brake ducts; rear reflectors; plate recess. Body:
  bubble roof (`roof_arc` 0.55), C-pillar plan lean (teardrop canopy), belt +3.5 cm, more glass
  wrap, rounder ends.

### `sperber_32`: 2000s German FR sport sedan with a high-revving six
- **Reference:** a 2000s German compact executive sport sedan with a naturally aspirated straight
  six: 4.49 × 1.78 × 1.37 m, wheelbase 2.73 m; long bonnet with the cabin set back (long
  dash-to-axle), crisp notchback, short boot with an integrated lip, sharp shoulder line, wide
  C-pillars. Cues: a split twin grille, twin round lamps with light rings under one cover,
  three-part front intake, fender gills, L-shaped tails wrapping onto the quarters, power dome,
  quad tips.
- **Changes:** rounded chrome-framed twin grille halves with vertical slats; one shaped lamp
  cover with twin round lamps and light rings (torus, in the headlight part); shaped outer
  intakes; trapezoid centre intake; round badge on the bonnet; boot badge; chrome door handles
  (generic trim). Body: power dome instead of a flat bulge, cabin set further back, longer
  roof (0.92 m), wider C-pillar, belt +3.5 cm, higher cowl; the separate boot-lip part is
  replaced by the integrated ducktail.

## Triangle counts

Hero budget 22k (mid LOD 7k); all six within it.

| car | full before → after | mid before → after |
|---|---|---|
| kestrel_ts | 18914 → 19074 | 5051 → 5459 |
| arashi_t4 | 18292 → 18022 | 4962 → 4624 |
| talus_20t | 17376 → 17680 | 4464 → 4646 |
| shiro_turbo | 18992 → 18956 | 5040 → 5006 |
| hakone_gt | 18873 → 19215 | 4826 → 5154 |
| sperber_32 | 19112 → 19572 | 4942 → 5130 |

The rows of `every_car_has_turn_signals_integrated_into_its_lamps`'s table for these six cars
were rebased to the new counts (the phase-1 doc asks per-car passes above +2 % to do this).

## Checks run
`cargo fmt`, `cargo clippy -p car-assets --all-targets -- -D warnings`, `cargo test -p
car-assets` (18 + 11 pass, incl. `lights_sit_outside_the_shell`), `validate-car-asset` (all cars
ok), `gen-car-mesh --check` (no other car's mesh changed). `npm ci && npm run wasm && node
e2e/mod_coverage.mjs <the six cars>`: 7 car/mod pairs show nothing, all no-visual mods already on the known list (`drivetrain_shifter_short` on kestrel / arashi / talus / sperber, `tires_sport` on talus / shiro / hakone); bumpers, lips, wings, bull bars and vinyls show.

Shots were checked from front / rear 3/4, side, top and nose / tail close-ups for gaps, floating
parts, glass through pillars and lens faces under the body.

## Shots
`docs/media/car-shapes/tuner/<car>_before_after.jpg`: per car, before (left) and after (right)
for front 3/4, nose close-up, rear 3/4, tail close-up, side, top.
`group_before_front.jpg` / `group_after_front.jpg`: the six cars side by side.

## Changes outside my style file
- `data/panels/meshes/parity_fwd_c.json` and `meshes/mid/parity_fwd_c.json`: refreshed with
  `tools/parity/build_parity_cars.py`'s `copy_panels` (its template is `kestrel_ts`; the copy
  would otherwise show the old kestrel). Parity car physics untouched.
- `tools/assets/tests/assets.rs`: the six rows of the turn-signal triangle table (data only).
  Other groups edit neighbouring rows of the same table, so expect a trivial merge conflict there.

## Generator requests
Sent to the lead (2026-10-27). With the knobs alone the side silhouettes stay close to the old
ones; these are what limit the "obvious from a thumbnail" bar for this group:
1. **Hood and deck heights independent of the lattice** (`hood_dz` at cowl / nose, `deck_dz`
   vs the belt): today the hood line comes from lattice nodes and the deck sits at the belt,
   which makes the thick, flat nose and tail blocks in every side view.
2. **Curved side sections**: an outward bow between rocker and shoulder (`side_bow_m`) and a
   lower-body tuck (`rocker_in_m`), so flanks are not vertical slabs.
3. **Curved nose / tail faces in side view**: bumper bow and a set-back chin / lower valance,
   instead of one raked plane per end.
4. **Plan-view taper over a length** (`nose_taper_m` / `tail_taper_m` + length), not only corner
   rounding.
5. **Side-window (DLO) shape**: rear side window kink, window top line independent of the roof.
6. **Shaped lens helpers in `kit.rs`**: `top_quad` (surface-following shell over any plan quad),
   `swept_top_lamp` and `face_poly` from `tuner.rs` can move to the shared kit as they are.

## Known gaps
- The side views still show tall flat flanks on the saloons: the belt / sill heights and the
  overhangs come from the lattice and the shared loft; only knobs were used.
- `sperber_32` has no C-pillar window kink (see generator requests).
- Headlamp projector bowls / rings are on the nose face; on the steep hakone nose they read as
  separate round lamps under the cover rather than inside it.
