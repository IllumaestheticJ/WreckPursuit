# car-shapes phase 2: race-offroad

- **Workstream:** car-shapes phase 2, group `race-offroad` (brief: `docs/integration/car-shapes-phase2-brief.md`)
- **Branch:** `ws/car-shapes-race-offroad` (from `origin/ws/car-shapes`)
- **Cars:** `morrigan_55`, `nyx_proto`, `merlo_20t`, `mesquite_pr`, `erg_hamada`
- **Files edited:** `tools/assets/src/styles/performance/race.rs`, `.../offroad.rs`; regenerated
  `data/panels/meshes/{,mid/}<car>.json` for the five cars only (full regen left every other mesh
  byte-identical).
- **Contract version:** unchanged. No physics, lattice, car def or shared generator change.
- **Before / after sheets:** `docs/media/car-shapes/race-offroad/<car>_before_after.jpg`
  (front 3/4, rear 3/4, side, cabin close-up; greybox renderer, `tools/assets/mesh_shots.mjs`).

## Per car

Triangles: full mesh (hero budget 22k) / mid LOD (7k).

### `erg_hamada` (rally-raid prototype)
- **Reference:** 2020s desert rally-raid prototype (top class): tubular spaceframe under a
  composite two-door coupe body, 2.3 m wide on 37-inch tyres, ~350 mm suspension travel, big
  pontoon fenders with box flares over exposed wheels, short nose with a lamp pod, low and narrow
  raked cab set mid-car, roof air scoop, sloping rear cover over a spare-wheel bay, tails on the
  rear frame.
- **Before:** a tall four-door SUV / minivan greenhouse (upright glass all round).
- **Changed:** cab chopped 12 cm and narrowed (tumblehome 0.28, A/C-pillar plan rake), 62° raked
  windshield and rear glass with a rounded roof arc, deeper fender swell; body-colour B-pillar.
  Two-door look: the rear-door and quarter side glass are covered with painted panels
  (`glass_cover`, a local helper that skins the loft's side-glass rings; regions stay
  `LeftDoorR` / `RearLeftQuarter`, so they dent and detach with them) carrying a dark cooling
  intake with body-colour slats and an accent lip. Lower rear glass covered by a painted
  spare-bay cover with a louvred outlet (`RearGlass` region). Tails moved off the (now absent)
  glass onto the tail frame uprights. The amber strip under the slim lamps is now the front turn
  signal and the slim lamps are part of the head-lamp part. One wiper, black handles, no fuel door.
- **Tris:** 19 872 → 20 054 / mid 5 912 → 6 135.

### `mesquite_pr` (pre-runner pickup)
- **Reference:** 2010s American full-size off-road performance pickup: tall slab sides and a high,
  flat hood, near-upright nose with a wide grille between the lamps, heavy crossbar and badge,
  tall lamp clusters with LED running-light graphics, fibreglass box flares over 35-inch
  all-terrains, raked windshield, sliding cab-back window.
- **Before:** read as a small 80s pickup (low belt, small square lamps, small grille, 46°
  upright windshield).
- **Changed:** belt +8 cm (taller slab sides), nose raised to the lattice hood-front height
  (1.20 m, the physical limit) and stood upright, flatter hood with a centre bulge, 57°
  windshield, side character line; new front: tall lamp clusters (projector lens + outboard
  amber signal + twin running-light blades), wide mesh grille with frame, body-colour crossbar and blank
  badge; sliding rear window on the cab-back wall; black mirrors and handles, fuel door left.
- **Tris:** 20 223 → 20 811 / mid 6 530 → 6 964.

### `morrigan_55` (front-engine GT endurance racer)
- **Reference:** 2010s front-mid-engine GT endurance car on a production coupe: long hood with
  raised fender peaks and a power bulge, louvred extractors over the front wheel wells, side
  exit vents behind the front wheels, cab set back with a fastback roof, swollen round haunches,
  slim swept lamps, huge splitter, swan-neck wing, deep diffuser.
- **Changed:** hood valley (fender peaks), power bulge, more raked nose and windshield, rounder
  roof arc and tumblehome, round flared arches with bigger haunches, small ducktail; slim lamps
  swept back along the fender peaks; hood louvres over the wheel wells only (the clashing
  fender-top vents are gone); new fender exit vents with carbon blades; body-colour B-pillar,
  wider C-pillar, one wiper, no handles or fuel door (race car).
- **Tris:** 20 454 → 20 682 / mid 5 648 → 6 002.

### `nyx_proto` (closed-cockpit sports prototype)
- **Reference:** 2010s closed-cockpit endurance prototype (≈4.75 × 1.9 × 1.05 m, 1 m front
  overhang): narrow canopy between high fender pods, no rear window — a body-colour engine cover
  with radiator louvres and a shark fin behind the cockpit.
- **Before:** a huge raked rear window behind the canopy made it read like a glass-backed coupe.
- **Changed:** rear glass covered by a painted engine cover (`RearGlass` region) with louvres
  either side of the fin; deeper engine-cover valley; slimmer A-pillars, body-colour B-pillar,
  one wiper, no handles / fuel door. `hood_valley` stays at 0.06: deeper hides part of the head
  lamps (`lights_sit_outside_the_shell`).
- **Tris:** 19 674 → 19 446 / mid 6 025 → 6 114.

### `merlo_20t` (touring-car racer)
- **Reference:** 2010s European touring-car racer on a current four-door sedan: 1.95 m wide on
  18-inch wheels with bolt-on arch extensions, raked windshield and fast rear glass, slim
  angular lamps wrapping onto the fenders, big grille, slim tails joined by a light bar.
- **Before:** an upright 80s three-box sedan greenhouse and box lamps.
- **Changed:** 61° windshield, 64° rear glass, roof arc 0.55, A/C-pillar plan rake, side
  character line, small ducktail; slim lamps on the nose top plus a face lens with the amber
  signal cut out of it and an LED brow; slimmer two-piece tails with a corner hook and a
  full-width red light bar on the lid; one wiper.
- **Tris:** 19 317 → 20 162 / mid 5 151 → 5 234.

All five race cars share `sun_strip` (race.rs): its patch now has a finer grid across (2 × 10),
because the coarse one cut under the crowned windshield and flickered.

## Second pass (lead feedback 2026-10-27: "bolder, change proportions")
- **All:** stance and overhangs pushed to the knob limits. Race cars: `ride_dz` −0.05, roofs
  4–6 cm lower, rounder section (`sill_tuck` / `shoulder_round` ~0.10), wider flares and haunches,
  longer splitter and tail overhangs (Morrigan +0.10 / +0.06 m, Nyx +0.07 / +0.05 m, Merlo +0.06 /
  +0.03 m). Off-road: `ride_dz` +0.08 / +0.06 and `arch_gap` 0.15 / 0.13 (long-travel gap over
  the tyres), overhangs trimmed (Mesquite −0.15 / −0.12 m, Erg −0.10 / −0.06 m).
- **Shaped lamps:** the race cars' rectangular `top_lamp` lenses are replaced by convex polygon
  slab lenses that follow the nose-top surface (`top_lens` / `shaped_lamp` in race.rs, with a
  dark housing grown around each outline): Morrigan a long swept blade, Nyx a swept main unit
  plus a short lower blade, Merlo an angular slit.
- **Mesquite:** boxed fibreglass fenders 17–18 cm out (black, with a flat fender top at the
  front and orange lips round the openings), black lower cladding over the door and quarter so
  the body reads lifted high over the tyres, exposed bed cage (stays from the hoop to the tail,
  tail hoop) over the spare and a fuel cell. The black hood stripes and side bands are gone
  (the cladding takes over as the dark accent; also needed to keep the mid LOD under 7k).
- **Erg:** body-colour pontoon fenders (boxed, navy lips) 14 cm out; taller stance.
- **Morrigan:** carbon sill pods between the wheels.
- **Rule 9 (brand cues, lead note):** Morrigan's yellow-with-black livery was dropped (a yellow
  front-engine V8 GT racer points at one real factory team): now slate teal with lime accents.
  Mesquite's C-shaped running lights (one maker's signature) are now twin horizontal blades.
  Grilles are generic mesh with blank badges.
- Not done on purpose: a cab-forward canopy on Morrigan (front-engine GT, long hood per its
  spec) and Merlo (four-door touring sedan). Only Nyx has the canopy.

## Final pass (phase 3 knobs, merged ws/car-shapes @ d8e0a524)
- **Morrigan:** plan taper (nose 0.90, tail 0.93), front / rear fender rise 0.08 / 0.06 over a
  flat hood (`hood_valley` dropped, `fender_peak_w` 0.65), a cove stepping in 4.5 cm behind the
  front wheel (exit-vent area), rear haunches standing 3.5 cm proud, side bulge 0.025, rocker in
  0.04. Lamps: projected top-view swept blades on the fender peaks (amber outer tip) and slim
  projected tail blades wrapping round the corners (30° yaw, amber inboard). No bezels (budget).
- **Nyx:** wheel pods via fender rise 0.11 / 0.09 (the old painted dome patches and sidepod
  patch are gone; louvres sit on the pods), canopy tumblehome 0.34, sidepods stepping in 7 cm
  between the wheels, nose and tail tapered (0.90 / 0.95). Lamps: projected angular units on
  the pods with a wide (8 cm) amber outer tip (a narrow tip or a rear strip fails
  `lights_sit_outside_the_shell` under the hood's shadow).
- **Merlo:** arch extensions standing 3.5 cm proud over both axles (sedan doors read pinched
  between them), nose taper 0.95, side bulge 0.02, rocker in 0.035, small fender rise. Lamps:
  slim angular head lamps drawn in the front view and projected round onto the fenders (25° yaw,
  10 cm amber outboard segment, black bezel); tails wrap round the corners, light bar kept.
- **Mesquite:** plan width 0.93 between the arches (fender boxes wider than cab and bed), front
  overhang −0.32 m (stubby pre-runner nose), upright glass (`tumblehome_m` 0.05, `glass_inset`
  0.05), rocker in 0.05.
- **Erg:** cabin pinched to 0.90 between pontoon fenders (sculpt +0.04 over each axle), fender
  humps 0.08 / 0.06 above the hood and deck, canopy tumblehome 0.32, overhangs −0.22 / −0.08.
- **Shared test fix (one line, `tools/assets/tests/assets.rs`):**
  `plan_width_keeps_the_body_outside_the_tires` built its baseline from the car's own style, so
  any car whose style pinches the waist at y = 0 (Erg, Mesquite) failed `≤ 0.81 × baseline`.
  The baseline now clears the style's `plan_width` first; the test's intent is unchanged.
- Triangles (full / mid): morrigan 21 704 / 6 428, nyx 19 892 / 6 128, merlo 20 652 / 5 404,
  mesquite 21 017 / 6 964, erg 19 552 / 6 190 (budgets 22k / 7k).
- Sheets (`<car>_before_after.jpg`) now show front 3/4, rear 3/4, side and top (the top view
  shows the plan taper and waists). Mod spot-check (8 kit / wing / bull-bar / vinyl mods on all
  five): nothing missing.

## Touch-up (merged ws/car-shapes @ 86693a0d: smooth shading, raked faces, all-cars regen)
- Merge: the five mesh conflicts were taken from this branch and regenerated (no hand-merged
  JSON). The all-cars regeneration's denser loft put morrigan at 23.4k, merlo at 22.3k and the
  mesquite mid LOD at 7 058; trimmed back:
  - **Morrigan:** 6-spoke rims (10 → 6; 5 spokes cost more), hood extractor vents instead of
    the louvre field, no hood pins or quick-fill couplings, coarser mouth mesh, hood stripe grid
    and diffuser (5 fins), and the projected lamps built with a coarser lens grid (`lean_cluster`
    in race.rs: `Kit::proj_cluster` with `Lamp::step_m` 0.07 instead of 0.04). The engine
    detail cannot be turned off from the style (the mechanical table forces it).
  - **Merlo:** coarse hood / trunk stripe grids, no head-lamp bezel.
  - **Mesquite:** grille 2 × 4, two bypass shocks per side instead of three.
- New knobs: nose face rake + chin and a tail chin on the race cars (Morrigan 20° / 0.08 /
  0.06, Nyx 25° / 0.10 / 0.08, Merlo 12° / 0.06 / 0.05); Mesquite `hood_dz` (0.06, 0.12) with
  `nose_h` 1.30 for the tall flat truck hood. `tail_face_rake_deg` stays 0.
- Smooth shading in the real renderer (`e2e/smooth_shots.mjs`): no seams or flipped faces on the
  five cars. One fix it and the low views exposed: door number panels cut under the bulged door
  side (now 3 rows, 8 mm off).
- Low rear 3/4 and low side views: no spikes under the faces.
- Triangles (full / mid): morrigan 21 906 / 6 294, nyx 21 714 / 6 425, merlo 21 915 / 5 607,
  mesquite 21 457 / 6 996, erg 21 126 / 6 398 (budgets 22k / 7k).
- Final sheet: `docs/media/car-shapes/race-offroad/final.jpg` (each row one car: original `main`
  mesh and final mesh, front / rear / side / top). Per-car `<car>_before_after.jpg` refreshed
  (before = the ws/car-shapes phase-1 meshes).
- Full mod coverage (136 mods × 5 cars, after the touch-up): 10 pairs show nothing. Known list:
  short shifter (morrigan, merlo, mesquite, erg), sequential shifter + radar + turbo 1 on the
  Mesquite, sway bars under the car (nyx front, erg rear). Erg turbo 1 was also reported, but on
  its own (`MODS=fi_turbo_1`) it shows in the open-bay views: Erg's render noise is 14 px, so it
  sits at the threshold.

## Checks
`cargo fmt`, `cargo clippy -p car-assets --all-targets -- -D warnings`, `cargo test -p car-assets`
(incl. the turn-signal +2 % table and `lights_sit_outside_the_shell`), `validate-car-asset`
(all cars ok), `gen-car-mesh --check`, `npm run wasm`, `node e2e/mod_coverage.mjs <cars>` (see
below). `wasm-pack` was not installed in this container; installed with `cargo install wasm-pack`.

## Mod coverage
`node e2e/mod_coverage.mjs morrigan_55 nyx_proto merlo_20t mesquite_pr erg_hamada` (136 mods):
7 car/mod pairs show nothing, all on the known list in `mod-visuals.md`: short shifter (nyx
excepted: merlo, mesquite, erg), sequential shifter + radar + turbo 1 on the Mesquite (truck
cabin / known), front sway bar on the Nyx (under the car). Look check (`SHOTS_ALL=1`, wing 2,
side skirts, tube bull bar, stripes vinyl on Erg and Mesquite): the wing sits on the roof edge,
the bull bar in front of the new noses, skirts and vinyls follow the new panels.
After the second pass, wings (2, swan neck), front lip, side skirts, rear bumper, tube bull bar,
stripes and race-number vinyls show on all five cars (`MODS=...`, 0 missing).

## Generator requests
All six delivered by phase 3 (see "Final pass"); kept for the record. Sent to the lead on
2026-10-27. Each item is a limit the per-car styles cannot get past:
1. **Plan-view width curve:** the loft's half-width is the lattice width everywhere; only
   `nose_plan_round` / `tail_plan_round` round the corners. Wanted: a per-station width scale
   (e.g. `plan_width: Vec<(y, scale)>`) for a nose narrower than the body, a Coke-bottle waist,
   or pickup fenders wider than the cab.
2. **Section shape per station:** the ring's side wall is fixed (sill tuck, crease, shoulder);
   the only per-station swell is `haunch` / `front_haunch` at the arches (max 0.08). Wanted: a
   side offset over a y-range and z-band (dx) for sidepods that step in behind the front wheels,
   door scallops, and pontoon fenders standing proud of the cabin. Today these are bolt-on
   detail boxes (`skirts`, `box_flare`).
3. **Fender height independent of the hood centre:** `hood_valley` is capped at 0.08, and
   deeper hides kit head lamps (`lights_sit_outside_the_shell`; Nyx stays at 0.06). Wanted:
   separate front / rear fender-top heights (prototype wheel pods, GT fender peaks).
4. **Overhangs:** `overhang_trim` is clamped to −0.15…+0.10 off the bumper nodes; the pickup's
   front overhang cannot get shorter.
5. **Lens outlines on the curved loft:** the kit has box, round and top-patch lenses. The local
   `top_lens` lays a polygon slab corner by corner on the top surface, which is fine for small
   convex lamps. A builder helper for a `surface_patch` with a polygon outline (top or face)
   would allow concave and wrapped lamp graphics.
6. Wheel size and track come from the car def (physics, frozen); only the arch gap and flares
   change how much tyre shows.
Also useful: a shared "cover glass with paint" helper (the local `glass_cover` / painted top
patches here) for two-door bodies on four-door lattices and closed engine covers.

## Known gaps
- Erg keeps a rear door seam and handle on the painted rear side panel (generic trim follows the
  lattice doors).
- The loft's flat shading shows a stipple on the big curved haunches (pre-existing, every car).
- Mesquite's hood front cannot rise above the lattice hood-front height, so the nose is a little
  lower than a real full-size truck's; its mid LOD sits at 6 964 of 7 000.
