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
- **Proportions:** each car's BodyStyle was pushed hard enough to read from a thumbnail: cabin
  placement, belt and roof heights (greenhouse height), glass rakes, roof length, pillar widths,
  plan rounding, haunches, flares, nose height, tail shape.
- **One fictional maker identity (rule 9, brand cues too):** the whole fleet shares an invented
  face: a hexagonal "shield" grille (`shield_grille`) and a blank shield badge (`badge`), in a
  different proportion and fill on each car (wide and low with slats, tall with fine vertical
  bars, big egg-crate, a low wide "mouth" with mesh). No real maker's signature grille, bar,
  tail-lamp pattern or badge shape is used; the 80s warden keeps the period-generic upright
  chrome grille with quad rectangular lamps that every US maker of the era used.
- **Lamps are shaped clusters, not boxes:** `shaped_cluster` clips a convex outline (|x|, z)
  into the lamp and its turn-signal segment (seam between them) with a dark housing behind;
  `face_poly` draws any convex lens / panel on the nose or tail face.
- **Own body kit per car** (`body.rs`, one `body_kit` detail pushed before the police
  equipment): grille, head lamps with the front signal cut out of the same lens, tail lamps with
  the rear signal (amber or red-on-red), bumpers / valances, plates, reverse lamps, mirrors,
  handles, fuel filler. `Kit::new` drops the foundation's box lamps and generic mirrors first.
- **Police kit (`kit.rs`):**
  - `grille_leds` no longer draws a black box over the nose (it hid the new grilles); the LED
    modules sit on two small black plates standing proud of the shield grille's bars.
  - Spotlights sit closer to the A-pillar on a thicker bracket (they read as floating).
  - The vigil's pedestal wing is a low decklid wing (~6 cm up); its red / blue LED strip stays.
  - The cruiser's trunk whip is 0.66 m (was 0.75 m): with the higher deck it broke the
    mesh-size check.
- **Kept and checked:** light bars (LED, rotators, slicktop hidden LEDs), push bars / bumper
  guards, spotlights, antennas, partitions, decal carriers (`decal_unit_l/_r`, `decal_roof`,
  `decal_trunk`), city and county liveries (see "Checks").

## Per car
References are body types only; no real model is reproduced.

### `sentinel_cruiser` (police "sedan")
- **Reference:** 2010s American full-size RWD police sedan, about 5.1 × 1.9 × 1.48 m on a
  3.05 m wheelbase: long hood, short deck, high belt with a chopped greenhouse, thick sloping
  C-pillar, slim head lamps, wide tail lamps, dual exhaust.
- **Changed:** cabin 20 cm back (long hood, short deck), roof 6 cm lower and belt 8 cm higher
  (slit side windows), 62° windshield / 63° fast rear glass, shorter roof, 20 cm C-pillars,
  7 cm belt kick at the C-pillar, tighter plan rounding. Kit: wide low shield grille with four
  slats and a chrome frame, badge; swept slit lamps (pointed inboard end, amber outboard end) in
  black housings that run back over the nose top, LED eyebrows; lower intake, fogs in black
  pods, chin lip. Rear: boomerang tail lamps (a long bar along the deck edge, the outer drop is
  the red-on-red signal) on a dark band, badge between them, reverse lamps, plate, valance,
  small body-colour lip; black rockers.
- **Triangles:** 19783 full (was 18497), 5220 mid (was 4728).

### `sentinel_wolfhound` (police "patrol")
- **Reference:** 2010s American AWD police pursuit sedan on a large FWD-platform saloon, about
  5.05 × 1.94 × 1.55 m on a 2.87 m wheelbase: tall body, very high belt, short glass, shorter
  hood and a tall upright tail, big grille, swept lamps, corner tail lamps.
- **Changed:** cabin 10 cm forward with a 12 cm longer roof, nose up to 0.86 m, belt 8 cm
  higher and roof 7 cm lower, 58° windshield, upright 50° rear glass over a tall deck with a
  4 cm ducktail, 22 cm C-pillars. Kit: tall shield grille with eleven fine vertical bars, badge;
  swept pentagon lamps rising from the grille corner to the fender (amber outboard end); fog
  pods, lower intake, plate. Rear: corner-wrapping tail lamps, wider at the top, amber outboard
  ends, in dark housings; badge, reverse lamps, plate, reflectors, valance; black rockers.
- **Triangles:** 19410 full (was 18626), 5535 mid (was 5183).

### `sentinel_bastion` (police "SUV")
- **Reference:** 2010s American full-size body-on-frame SUV, about 5.18 × 2.04 × 1.89 m on a
  2.95 m wheelbase, 0.22 m ground clearance: tall upright nose, nearly flat hood, near-vertical
  tailgate, stacked head lamps, tall vertical tail lamps, running boards, big mirrors, hitch.
- **Changed:** nose raised to the lattice hood height, 57° windshield with a near-upright
  A-pillar, near-vertical rear glass (12°), squarer plan (8 % rounding), boxier greenhouse,
  bigger box flares, squarer tail, 14 cm D-pillars. Kit: the biggest shield grille, coarse
  egg-crate fill, chrome frame, badge; stacked lamps (two projector rings over an amber strip,
  DRL line above); dark valance with fogs and a skid plate, plate. Rear: vertical tail lamps
  (amber lower section, reverse inset), bumper step, tailgate handle, plate, hitch; running
  boards, rocker cladding, large black-capped mirrors.
- **Triangles:** 20221 full (was 18932), 6186 mid (was 5346).

### `sentinel_warden` (police "old sedan")
- **Reference:** late-80s American full-size body-on-frame RWD sedan, about 5.4 × 1.98 × 1.43 m
  on a 2.9 m wheelbase: flat slab sides, square corners, formal upright rear glass with wide
  C-pillars, period upright chrome grille with quad rectangular sealed beams, amber corner
  lamps, chrome bumpers with rubber strips, wide ribbed tail lamps.
- **Changed:** near-flat nose face (1° rake, 4 % plan rounding), 54° windshield, formal 32° rear
  glass, 24 cm C-pillars, thin A-pillars, crisper shoulders and tail, chrome drip rail. Kit:
  chrome bumper blades wrapping round the corners with rubber strips (the police guards sit on
  them), black valance, chrome egg-crate grille, quad lamps in chrome bezels, amber wraparound
  corner lamps (face and fender side, one lens part), chrome header, plate; wide tail lamps with
  chrome ribs and red-on-red outer signals, reverse lamps, chrome plate panel, side markers;
  chrome handles, chrome-capped mirrors.
- **Triangles:** 20707 full (was 19111), 5965 mid (was 5575).

### `sentinel_vigil` (police "sports patrol")
- **Reference:** 2010s American muscle coupe, about 4.8 × 1.93 × 1.37 m on a 2.75 m wheelbase:
  long hood with a power dome and extractor vents, cabin well back, very low greenhouse with
  wide C-pillars, big rear haunches, slim lamps, one large front opening, quad tips.
- **Changed:** cabin 28 cm back, nose down to 0.64 m, roof 10 cm lower and belt 5 cm higher
  ("gun-slit" side glass), 64° windshield, 22 cm C-pillars, big rear haunches, slight
  nose-down stance; generic hood boxes replaced by the kit's vents. Kit: slim "angry" lamps
  (top edge dropping inboard, amber inboard segment) high on the corners over one big
  shield-shaped mouth (narrow top, wide lower half) with a mesh fill, badge; brake ducts; two
  hood extractor vents. Rear: dark band with one swept lamp per side (outer end sweeps up,
  inboard end is the red-on-red signal), centre reverse lamps and badge, plate, dark diffuser
  valance; skirts, faux quarter vents, mirrors; police low decklid wing.
- **Triangles:** 19184 full (was 19100), 5389 mid (was 4994).

All five are inside the hero budget (22k) and the mid budget (7k).

## Checks
- `cargo fmt`, `cargo clippy -p car-assets --all-targets -- -D warnings`, `cargo test -p
  car-assets` (incl. `every_car_has_turn_signals_integrated_into_its_lamps`, police lens split
  and slicktop tests), `validate-car-asset`, `gen-car-mesh --check`: pass.
- `npm run wasm` (wasm-pack installed into this container), then the testbed liveries
  (headless, `startFreeDrive`, left / right / front / rear / top): `?police_skin=city` (all five
  schemes), `?police_skin=county` (sheriff), `?livery=grid` (atlas debug texture). Paint splits
  follow the doors, lettering and side / roof / trunk unit numbers land on their carriers, no
  page errors.
- `node e2e/mod_coverage.mjs` on the five cars: see "Mod coverage".
- `tools/assets/mesh_shots.mjs` rest-pose shots (front / rear / side / top / cabin) checked for
  gaps, floating parts, glass through pillars and wheels in arches.

## Mod coverage
`node e2e/mod_coverage.mjs sentinel_cruiser sentinel_wolfhound sentinel_bastion sentinel_warden
sentinel_vigil` on the final meshes: 136 mods per car, 7 car/mod pairs show nothing, all on the
known list from phase 1 (no outside view): `drivetrain_shifter_short` on all five,
`elec_radar_1` on the warden and the vigil. Body kits, wings, lips, vinyls and bull bars show and
sit on the new bodies (stock and mod shots checked).

## Shots
`docs/media/car-shapes/police/`:
- `compare_front_rear.jpg`: before / after, front 3/4 and rear 3/4 close-ups, all five cars.
- `compare_side_top.jpg`: before / after, side and top.
- `livery_city.jpg`, `livery_county.jpg`, `livery_grid.jpg`: testbed renders of the liveries on
  the new bodies.

## Notes for the lead
- `tools/assets/tests/assets.rs`: only the five `sentinel_*` rows of
  `TRIS_BEFORE_INTEGRATED_SIGNALS` changed (rebased to the new counts). Other groups edit
  neighbouring rows, so expect trivial merge conflicts there.
- `tools/assets/README.md`'s triangle tables still show the phase-1 police counts.
- The police test's triangle range in `police.rs` is now 8k–22k (the hero budget).

## Generator requests
Sent to the lead (none blocking; this pass works inside the current generator):
1. Nose height above the lattice hood-front node: `profile.rs` clamps `z_nose` to `z_hood.1`, so
   the SUV's nose still falls ~9 cm (a knob allowing up to +0.10 m would give the flat, tall
   truck hood).
2. A face-following lens / panel primitive: a polygon outline projected onto the nose / tail
   face (respecting nose rake and plan rounding), like `surface_patch` for the end faces.
3. Greenhouse plan taper toward the rear, independent of tumblehome.

## Known gaps
- The SUV's hood still falls ~9 cm from the cowl to the nose (request 1).
- Lenses and grilles are flat prisms on the nose / tail face; they do not follow its curve.
- The push bars' positions are unchanged (they sit where the research table put them) and hide
  part of each grille, as on the real cars.
