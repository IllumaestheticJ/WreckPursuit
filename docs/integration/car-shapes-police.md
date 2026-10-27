# car-shapes phase 2: police group

- **Branch:** `ws/car-shapes-police` (from `origin/ws/car-shapes`)
- **Cars:** `sentinel_cruiser`, `sentinel_wolfhound`, `sentinel_bastion`, `sentinel_warden`,
  `sentinel_vigil`
- **Files:** `tools/assets/src/styles/police.rs` (body knobs, wiring), new
  `tools/assets/src/styles/police/body.rs` (per-car body kits), `tools/assets/src/styles/police/kit.rs`
  (equipment tweaks), regenerated `data/panels/meshes/{,mid/}sentinel_*.json`, the five police
  rows of `TRIS_BEFORE_INTEGRATED_SIGNALS` in `tools/assets/tests/assets.rs`.
- **Shared generator:** untouched. The body kits use the performance `Kit` helpers as they are
  (read-only). No other car's mesh changed (`git status` checked after every regeneration).
- **Physics:** untouched (no change to `data/cars`, `data/lattices`, panel specs or sim code).

## What changed for every police car
- Lamps are shaped clusters, not boxes: `body.rs` clips a convex outline into the lamp and
  its turn-signal segment (`shaped_cluster`, `face_poly`, local helpers) with a dark housing
  behind it.
- Each car now has its own body kit (`body.rs`, one `body_kit` detail pushed before the police
  equipment): grille, head lamps with the front turn signal cut out of the same lens, tail lamps
  with the rear signal (amber or red-on-red), bumpers / valances, licence plates, reverse lamps,
  its own mirrors, handles and fuel filler. `Kit::new` drops the foundation's box lamps and
  generic mirrors first.
- BodyStyle knobs retuned per car (proportions, pillar widths, glass rakes, plan rounding, belt
  and roof heights, trim finishes).
- Police kit (`kit.rs`):
  - `grille_leds` no longer draws its own black box over the nose (it hid the new grilles); the
    LED modules now sit on two small black mounting plates, proud of the car's grille.
  - Spotlights sit closer to the A-pillar on a thicker bracket (they read as floating before).
  - The cruiser's trunk whip is 0.66 m (was 0.75 m): with the higher deck it broke the
    mesh-size check.
  - The vigil's pedestal wing is now a low decklid wing (about 6 cm up, the stock
    performance-pack look); its red / blue LED strip is kept.
- Kept and checked: light bars (LED, rotators, slicktop hidden LEDs), push bars / bumper guards,
  spotlights, antennas, partitions, decal carriers (`decal_unit_l/_r`, `decal_roof`,
  `decal_trunk`), city and county liveries (see "Checks").

## Per car

### `sentinel_cruiser` (police "sedan")
- **Reference:** 2010s American full-size RWD police sedan with muscle-car styling, about
  5.1 × 1.9 × 1.48 m on a 3.05 m wheelbase: long hood, high belt, short chopped greenhouse with
  a thick sloping C-pillar, wide crosshair grille, narrow slit head lamps with an LED eyebrow,
  one full-width "racetrack" tail lamp, dual exhaust.
- **Changed:** cabin 20 cm back (long hood, short deck), roof 6 cm lower and belt 8 cm higher
  (chopped, slit-window look), 62° windshield / 63° fast rear glass, shorter roof, 20 cm
  C-pillars, a 7 cm belt kick at the C-pillar, tighter plan rounding. Kit: wide mesh
  grille with a bright crosshair and surround, swept slit lamps (shaped lens, pointed inboard
  end, amber outboard end) in black housings that run back over the nose top, LED eyebrows, lower intake, fog lamps in black pods,
  chin lip; full-width racetrack tail lamp (a lit ring round a dark core, red-on-red outer signals) on a
  dark band, reverse
  lamps, rear plate, black valance, small body-colour decklid lip; black rockers.
- **Triangles:** 20287 full (was 18497), 5300 mid (was 4728).

### `sentinel_wolfhound` (police "patrol")
- **Reference:** 2010s American AWD police pursuit sedan built on a large FWD-platform saloon,
  about 5.05 × 1.94 × 1.55 m on a 2.87 m wheelbase: tall body, very high belt and a short
  greenhouse, big trapezoid grille with three wide bright bars, swept head lamps, round fogs in
  lower corner intakes, wide tail lamps joined by a bright garnish over the plate.
- **Changed:** cabin 10 cm forward (shorter hood) with a 12 cm longer roof, nose up to 0.86 m,
  belt 8 cm higher and roof 7 cm lower (tall body, short glass), 58° windshield, upright 50°
  rear glass over a tall deck with a 4 cm ducktail, 22 cm C-pillars. Kit: trapezoid grille (narrows downward) with three chrome bars
  and a dark frame, swept pentagon lamps rising from the grille corner to the fender (shaped lens, amber outboard
  end, black housing over the nose top), fog pods, lower intake, front plate; corner-wrapping tail lamps, wider at the top, with amber outboard
  ends in dark housings, chrome garnish between them, reverse lamps, plate, rear reflectors, valance; black
  rockers.
- **Triangles:** 20226 full (was 18626), 5667 mid (was 5183).

### `sentinel_bastion` (police "SUV")
- **Reference:** 2010s American full-size body-on-frame SUV, about 5.18 × 2.04 × 1.89 m on a
  2.95 m wheelbase, 0.22 m ground clearance: tall upright nose and nearly flat hood, two-tier
  grille split by a heavy bright bar at lamp height, stacked rectangular head lamps with
  projectors, near-vertical tailgate with tall vertical tail lamps on the corners, running
  boards, black lower cladding, big mirrors, hitch receiver.
- **Changed:** nose raised to the lattice hood height (the hood is now nearly flat), 57°
  windshield with a near-upright A-pillar, near-vertical rear glass (12°), squarer plan (8 %
  rounding), boxier greenhouse (less tumblehome), bigger box flares, squarer tail, 14 cm
  D-pillars. Kit: two-tier mesh grille with a chrome cross bar and frame, stacked lamps (two
  projector rings over an amber strip, DRL line above), dark front valance with fogs and a skid
  plate, front plate; vertical tail lamps (amber lower section, reverse lamp inset), black
  bumper step, tailgate handle, plate, hitch; running boards, black rocker cladding, large
  black-capped mirrors.
- **Triangles:** 20441 full (was 18932), 6190 mid (was 5346).

### `sentinel_warden` (police "old sedan")
- **Reference:** late-80s American full-size body-on-frame RWD sedan, about 5.4 × 1.98 × 1.43 m
  on a 2.9 m wheelbase: flat slab sides, square corners, formal upright rear glass with wide
  C-pillars, upright chrome egg-crate grille, quad rectangular sealed-beam head lamps in a
  chrome bezel, amber corner lamps wrapping round the fenders, chrome bumpers with black rubber
  strips, wide ribbed tail lamps beside a chrome plate panel, chrome drip rails and handles.
- **Changed:** near-flat nose face (1° rake, 4 % plan rounding front and rear), 54° windshield,
  formal 32° rear glass, 24 cm C-pillars, thin 6.5 cm A-pillars, crisper shoulders and tail,
  chrome drip rail, smaller arch lips. Kit: chrome front / rear bumper blades wrapping round the
  corners with rubber strips (the police bumper guards sit on them), black lower valance, chrome
  egg-crate grille with frame, quad lamps in chrome bezels, amber wraparound corner lamps (face
  and fender side, one lens part), chrome header strip, front plate; wide tail lamps with chrome
  ribs and red-on-red outer signals, reverse lamps, chrome plate panel, rear side markers;
  chrome handles, chrome-capped mirrors.
- **Triangles:** 20707 full (was 19111), 5965 mid (was 5575).

### `sentinel_vigil` (police "sports patrol")
- **Reference:** 2010s American muscle coupe, about 4.8 × 1.93 × 1.37 m on a 2.75 m wheelbase:
  long hood with a power dome and two heat-extractor vents, cabin set well back, low
  "gun-slit" greenhouse with wide C-pillars, slim upper grille slot in line with slim head
  lamps, big lower intake with a body-colour bar, brake-cooling inlets, twin horizontal tail
  lamps per side in a dark band, quad tips, side skirts.
- **Changed:** cabin 28 cm back (long hood), nose down to 0.64 m, roof 10 cm lower and belt
  5 cm higher (gun-slit side glass), 64° windshield, 22 cm C-pillars, big rear haunches, 0.2° nose-down stance; the
  generic hood vent boxes are replaced by the kit's vents. Kit: slim upper slot grille, slim
  "angry" lamps (shaped lens, top edge dropping toward the slot, amber inboard segment) in
  dark housings, LED blades, brake ducts, big lower
  intake with a body-colour bar; two hood extractor vents; dark tail band with an inner lamp
  (red-on-red signal) and an outer lamp ring with a dark core per side, centre reverse lamps,
  plate, dark diffuser valance; side skirts, faux quarter vents, mirrors. Police kit: low decklid
  wing.
- **Triangles:** 19472 full (was 19100), 5469 mid (was 4994).

All five are inside the hero budget (22k) and the mid budget (7k).

## Checks
- `cargo fmt`, `cargo clippy -p car-assets --all-targets -- -D warnings`, `cargo test -p
  car-assets` (incl. `every_car_has_turn_signals_integrated_into_its_lamps`, police lens split
  and slicktop tests), `validate-car-asset`, `gen-car-mesh --check`: pass.
- `npm run wasm` (built here; wasm-pack installed into this container) and
  `node e2e/mod_coverage.mjs` on the five cars: see "Mod coverage" below.
- Liveries in the testbed (headless, `startFreeDrive`, left / right / front / rear / top):
  `?police_skin=city` (all five schemes), `?police_skin=county` (ACSO sheriff), `?livery=grid`
  (atlas debug texture). Paint splits follow the doors, lettering and side / roof / trunk unit
  numbers land on their carriers, no page errors.
- `tools/assets/mesh_shots.mjs` rest-pose shots (front / rear / side / top / cabin) checked for
  gaps, floating parts, glass through pillars and wheels in arches.

## Shots
`docs/media/car-shapes/police/`:
- `compare_front_rear.jpg`: before / after, front 3/4 and rear 3/4 close-ups, all five cars.
- `compare_side_top.jpg`: before / after, side and top.
- `livery_city.jpg`, `livery_county.jpg`, `livery_grid.jpg`: testbed renders of the liveries on
  the new bodies.

## Notes for the lead
- `tools/assets/tests/assets.rs`: only the five `sentinel_*` rows of
  `TRIS_BEFORE_INTEGRATED_SIGNALS` changed (rebased to the new counts, as the phase-1 doc asks).
  Other groups edit neighbouring rows of the same table, so expect trivial merge conflicts there.
- `tools/assets/README.md`'s triangle tables still show the phase-1 police counts; the new
  counts are above.
- The police test's triangle range in `police.rs` is now 8k–22k (the hero budget).

## Generator requests
Sent to the lead (none blocking; this pass works inside the current generator):
1. Nose height above the lattice hood-front node: `profile.rs` clamps `z_nose` to `z_hood.1`, so
   the SUV's nose still falls ~9 cm (a knob allowing up to +0.10 m over it would give the flat,
   tall truck hood).
2. A face-following lens / panel primitive: a polygon outline projected onto the nose / tail face
   (respecting nose rake and plan rounding), like `surface_patch` for the end faces.
3. Greenhouse plan taper toward the rear, independent of tumblehome.

## Known gaps
- The hood of the SUV still falls about 9 cm from the cowl to the nose: the nose height is
  clamped to the lattice's hood-front node height.
- Lamp housings and grilles are boxes on the nose / tail face (no lamp follows a curved
  face); lamp shapes come from splitting and stacking boxes.
- The push bars' positions are unchanged (they still sit where the research table put them).
