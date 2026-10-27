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
  `swept_top_lamp` (a head lamp on the nose top whose outboard edge runs back into the fender,
  its rear edge a diagonal; since pass 3 drawn with the shared `Kit::proj_cluster`),
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
  into the front fenders, a slatted upper slot, wide trapezoid lower mouth
  with fog pods in the corners, top-mount intercooler scoop, tall pedestal boot wing, round
  tail elements, roof vent, mud flaps, gold multi-spoke wheels.
- **Changes:** swept surface-following head lamps (amber inboard segment) on dark housings
  replace the rectangle + projector boxes; trapezoid slot grille with two slats and blank badge;
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
  body-colour bar, a hoop wing over the hatch, wide tails in a body-wide smoked panel.
- **Changes:** long swept lamp covers over the triple projectors; octagonal central mouth with
  mesh, body-colour bar and badge; shaped brake ducts; the quad round tails (a well-known
  brand signature) became one wide two-tier lens per side with rounded outer corners and an
  amber inboard section; rear reflectors; plate recess. Body:
  bubble roof (`roof_arc` 0.55), C-pillar plan lean (teardrop canopy), belt +3.5 cm, more glass
  wrap, rounder ends.

### `sperber_32`: 2000s German FR sport sedan with a high-revving six
- **Reference:** a 2000s German compact executive sport sedan with a naturally aspirated straight
  six: 4.49 × 1.78 × 1.37 m, wheelbase 2.73 m; long bonnet with the cabin set back (long
  dash-to-axle), crisp notchback, short boot with an integrated lip, sharp shoulder line, wide
  C-pillars. Cues: a low full-width grille between the lamps, twin round lamps under one
  cover, three-part front intake, fender gills, horizontal tails wrapping onto the quarters,
  power dome, quad tips.
- **Changes:** one full-width rounded grille with a dark mesh and a body-colour split bar
  (replaces the twin split grille, a carmaker's protected trade dress; the lamp light rings,
  the L-shaped tails and the bonnet roundel went for the same reason); one shaped lamp cover
  with twin round lamps; horizontal tails tapering toward the plate with an amber lower strip,
  wrapping onto the quarters; shaped outer intakes; trapezoid centre intake; boot badge;
  dark fender gills with body-colour slats; chrome door handles (generic trim). Body: power dome instead of a flat bulge, cabin set further back, longer
  roof (0.92 m), wider C-pillar, belt +3.5 cm, higher cowl; the separate boot-lip part is
  replaced by the integrated ducktail.

## Brand cues removed (CLAUDE.md rule 9, lead review 2026-10-27)
Rule 9 covers trade dress as well as names. Removed: sperber's twin split grille, lamp light
rings, L-shaped tails and bonnet roundel; hakone's quad round tails; kestrel's hexagon grille
with a bright wing bar. What remains are generic era / body-type cues (swept lamps, square
lamps with amber corners, intercooler mesh, pedestal wings, hoop wing, horizontal tails).

## Pass 3 (phase 3 generator, merged from `ws/car-shapes` @ d8e0a524)
Side profiles, sections and plan views, using the phase 3 knobs (values in `tuner.rs`; wheel
stations from the car def via `axle()`, belt ≈ 0.59 × body height):

| car | plan view (`plan_width`) | section | sculpts / fender rises | overhang trim (front, rear) |
|---|---|---|---|---|
| kestrel_ts | nose 0.93, tail 0.95 | bulge 0.025, rocker in 0.04 | shoulder line +2 cm, arch flares +2.5 / +3 cm, rear rise 2 cm | −0.10, −0.12 |
| arashi_t4 | nose 0.96, tail 0.97 (square) | bulge 0.012, rocker in 0.03 | shoulder +1.2 cm, box flares +2.2 cm, front rise 1.5 cm | −0.08, −0.10 |
| talus_20t | nose 0.95, waist 0.95, tail 0.96 | bulge 0.02, rocker in 0.045 | round blisters +4 / +4.5 cm, shoulder, rises 3 / 2.5 cm | −0.08, −0.12 |
| shiro_turbo | nose 0.89, waist 0.96, tail 0.92 | bulge 0.03 high, rocker in 0.045 | shoulder +1.5 cm, rear haunch +3 cm, rear rise 3.5 cm | +0.02, −0.16 |
| hakone_gt | pointed nose 0.86, waist 0.95, tail 0.90 | bulge 0.04, rocker in 0.05, tumblehome 0.26 | fat rear haunch +4 cm, front +2 cm, rear rise 5 cm | +0.05, −0.12 |
| sperber_32 | nose 0.94, tail 0.95 | bulge 0.02, rocker in 0.035 | sharp shoulder +2.2 cm, lower door line −2 cm, rear +2 cm, front rise 2 cm | −0.14, −0.06 |

Lamps moved to the shared projected clusters (`Kit::proj_cluster`): the swept top lamps
(kestrel, shiro, hakone; `swept_top_lamp` now draws a top-view outline, amber at the outer
end, flush bezel), arashi's and talus's head lamps (front view, 22° / 15° yaw so the amber
corner wraps onto the fender), arashi's and talus's tails (rear view, 30° wrap). The local
`top_quad` helper is gone. Kestrel's swept lamps sit outboard (|x| ≥ 0.56 m) and its
`fender_peak_w` is 0.95, so they stay on one side of the fender-peak band
(`fender_rise_keeps_kit_lamps_visible`). The swept lamp keeps a 15 cm minimum lens width when
a narrow nose pulls its outer edge in (`phase3_knobs_keep_panels_on_their_regions` sweeps
`plan_width` 0.80 at the nose). Sheets: `group_side_before_after.jpg`,
`group_top_before_after.jpg` and the per-car sheets (regenerated).

Still waiting on the rest of phase 3b (hood / deck dz, DLO top line and kink, nose height,
roof-arc exponent, smooth shading) for the touch-up pass.

### Pass 3b part 1 (merged from `ws/car-shapes` @ b10e5e14)
End faces and plan:

| car | nose face rake / chin | nose plan bow (tip) | tail face rake | roof plan taper | lower lean-in |
|---|---|---|---|---|---|
| kestrel_ts | 22° / 0.06 | 0.25 (0.75) | 10° | 0.08 | – |
| arashi_t4 | – / 0.03 | – | – | 0.04 | – |
| talus_20t | – / – | – | – | 0.05 | – |
| shiro_turbo | 30° / 0.06 | 0.25 (0.7) | 15° | 0.12 | 0.03 |
| hakone_gt | 35° / 0.05 | 0.30 (0.7) | 20° | 0.15 | 0.04 |
| sperber_32 | 12° / 0.05 | 0.15 (0.8) | 8° | 0.06 | – |

- **Kit parts on raked faces** (`follow_faces` in `tuner.rs`): grilles, intakes, fogs, face
  lamps, badges, plates, valances and diffusers built on the old vertical end plane are moved
  onto the real face: rays on a 3 × 4 grid over each part's x / z span find the body shell, and
  each vertex moves by the bilinear blend (rays that pass over or under the face and hit the
  roof or floor are ignored). Exact whatever knobs shape the face (rake, chin, plan bow, the old
  rounding knobs). Projected lamps (which already follow the surface) are skipped. Swept top
  lamps shift back by `face_depth` so they lie on the nose top, not down the face.
- **No tail plan bow**: kit parts placed from `Kit::wt()` collapse inward when a bow narrows the
  tail tip; the `plan_width` taper already narrows the tails.
- **Generator bug (reported):** with `nose_face_rake_deg` / `tail_face_rake_deg` ≠ 0,
  front- / rear-view projected lamps (`Kit::proj_cluster` with `LampView::Front` / `Rear`) land
  ~5 cm inside the face (talus measured: lens at y 2.01 behind a hood surface at 2.06 at the same
  x / z; with the rake at 0 the lens sits 8 mm proud). Top-view lamps and chins are fine. So
  arashi and talus (projected front and rear lamps) keep vertical faces for now; arashi keeps a
  chin. On talus a chin also cut under the outboard signal, and the shared lamp code's 3b
  outline handling put part of the old cut-corner signal under the bumper, so its head lamps
  are now a plain trapezoid 6 cm inside the nose edge with an 18° wrap.
- arashi's and talus's separate lamp housings were replaced by the projected cluster's flush
  bezel (arashi 25 mm, so the white lenses read on the white body).

### Phase 3 addenda (merged from `ws/car-shapes` @ d64c04b7)
- Hood / deck heights: `hood_dz.1` (front) −0.05 kestrel / sperber, −0.03 arashi / talus,
  −0.06 hakone; `deck_dz` −0.04 kestrel / sperber, −0.03 arashi / talus / shiro. The thick
  flat nose and tail blocks in side view are thinner. shiro keeps its hood front (−0.03 still
  left its front fender 3.0 cm off the crushed front in `front_crumples_and_follows_the_crushed_front`,
  limit 3.0 cm; the fender fold binds to the lattice hood line).
- DLO top line: `dlo_top_drop_m` 0.03 shiro, 0.04 hakone (teardrop side glass), 0.02 sperber.
  No C-pillar kink on sperber: it is a strong brand cue for that body type (rule 9).
- `roof_arc_rear_exp` 1.4 on hakone (the hatch falls in one curve from the header).
- `face_poly` / `on_side` now come from the shared kit (same signatures; the local copies are
  gone). The local `swept_top_lamp` stays: it reads the outer edge from the body's plan width at
  each corner (bowed noses) and shifts back by `face_depth`, which the shared one does not.
- The projected-lamp / face-rake issue is still there after the addenda: talus with an 8° nose
  rake puts 69 of 84 head-lamp lens faces under the hood (`lights_sit_outside_the_shell`).
  Repro: add `b.nose_face_rake_deg = 8.0;` in `tuner::talus`, regenerate, run that test.

## Triangle counts

Hero budget 22k (mid LOD 7k); all six within it.

| car | full before → after | mid before → after |
|---|---|---|
| kestrel_ts | 18914 → 19654 | 5051 → 5463 |
| arashi_t4 | 18292 → 18820 | 4962 → 4854 |
| talus_20t | 17376 → 19464 | 4464 → 5080 |
| shiro_turbo | 18992 → 19454 | 5040 → 5282 |
| hakone_gt | 18873 → 19844 | 4826 → 4614 |
| sperber_32 | 19112 → 18880 | 4942 → 5062 |

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
6. **Shaped lens helpers in `kit.rs`**: done in phase 3 as `car_assets::lamp` /
   `Kit::proj_cluster` (items 2–4 also landed in phase 3; 1 and 5 are in phase 3b).

## Known gaps
- Hood and deck heights still come from the lattice (thick nose / tail blocks in side view on
  the saloons) until phase 3b's hood / deck dz.
- `sperber_32` has no C-pillar window kink (see generator requests).
- Headlamp projector bowls / rings are on the nose face; on the steep hakone nose they read as
  separate round lamps under the cover rather than inside it.
