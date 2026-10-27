# car-shapes phase 2: hatch group

- **Workstream:** car-shapes phase 2, group `hatch` (brief: `docs/integration/car-shapes-phase2-brief.md`)
- **Branch:** `ws/car-shapes-hatch` (from `origin/ws/car-shapes`)
- **Cars:** `vireo_xs`, `stoat_16v`, `marten_20`, `lindwurm_4t`
- **Files changed:** `tools/assets/src/styles/performance/hatch.rs` (style file, the only
  generator file touched), the four cars' meshes in `data/panels/meshes/` and
  `data/panels/meshes/mid/`, their rows in `TRIS_BEFORE_INTEGRATED_SIGNALS`
  (`tools/assets/tests/assets.rs`), shots in `docs/media/car-shapes/hatch/`.
- **Not changed:** shared generator code, `data/cars`, `data/lattices`, panel specs, sim crates.
- **Before / after:** `docs/media/car-shapes/hatch/hatch_before_after.jpg` (all four), and
  `<car>_before_after.jpg` per car (front 3/4, rear 3/4, side).

## What was wrong before
All four used one body recipe and (three of them) one kit: a long flat roof running back to an
almost vertical hatch with a third side window, so they read as small estates; a tall slab nose
with box lamps; the stoat and marten were recolours of the vireo; a floating skirt plank; wheels
small in big arch gaps.

## Shared changes (all four, render only)
- Short roof (`roof_len_trim` −0.10…−0.18) with a slight fall (`roof_arc` 0.10…0.40) to a raked
  hatch; on the vireo, marten and lindwurm this also removes the quarter glass, giving the thick
  body-colour C-pillar of a hatch (`c_pillar_w_m` 0.20…0.28).
- `arch_gap` 0.07 → 0.05 (wheels fill the arches), `overhang_trim` (−0.05, −0.02): the visual
  nose ends at the bumper node row (front overhang 0.82–0.90 m instead of 0.87–0.95 m).
- Local helpers in `hatch.rs` (no shared code): shaped face lenses (`face_poly`, convex
  outline swept off the nose / tail face), dark lamp housings, blank badges, body-side strips
  split per panel (fender / front door / rear door / quarter, so each piece rides and detaches
  with its panel), tail-lamp wraps onto the quarters, centre high brake lamp under the roof
  spoiler, plate on a recess, rear reflectors, rear wiper.

## Per car

### vireo_xs
- **Reference:** late-90s Japanese front-drive hot hatch, about 4.2 m long, 1.7 m wide, 1.37 m
  tall, 2.6 m wheelbase, 15-inch wheels. Low, rounded nose with swept clear lamps wrapping onto
  the nose top, slim grille slot, a big lower mouth, a raked windshield (≈60°), short roof
  falling to a raked hatch over a short tailgate, small body-colour roof spoiler, tall corner
  tail clusters, a rising belt toward the rear.
- **Changes:** nose height 0.66 → 0.60 m and rake 25° → 32° (lower, softer nose); belt +5 cm
  with a 5 cm rise toward the tail; short falling roof, no quarter glass, 0.22 m C-pillar;
  windshield 60° with more wrap. Lamps: trapezoid clear lenses with the amber signal at the
  outer corner, continued onto the nose top, on a dark housing that runs over the nose edge,
  twin projector reflectors. Trapezoid lower mouth with two bars, round fogs, red accent line and
  lip, badge in the grille slot. Rear: tri-colour corner clusters (red / amber / clear) moved
  in from the corners and wrapping onto the quarters, black hatch garnish with badge, high brake
  lamp, plate, reflectors. Skirt made smaller.
- **Triangles:** 17,147 full (was 17,065), 4,185 mid. Hero budget 22k.

### stoat_16v
- **Reference:** early-90s European front-drive 16-valve hot hatch, about 4.05 m long, 1.7 m
  wide, 1.40 m tall, 2.48 m wheelbase, 15-inch wheels. Upright and boxy with crisp edges: a
  near-vertical nose with a full-width black band of square twin-lamp housings and a slatted
  grille (a coloured pinstripe), black bumpers wrapping round to the arches, black rubbing
  strips, an upright greenhouse with a very thick C-pillar, wide ribbed horizontal tail lamps,
  small black roof spoiler.
- **Changes:** own body (was a vireo recolour): nose rake 10°, square plan corners, flat hood,
  56° windshield, crisper shoulders and sill, 0.28 m C-pillar. Own kit: black front band, square
  lamp lenses each with twin round reflectors, amber signals at the corners, three-slat grille
  with badge, yellow pinstripe; black front and rear bumper bands with side wraps to the arches;
  rectangular fogs; black rubbing strips continuing the bumper line along the doors; ribbed tail
  lamps with clear inner strips either side of a black plate panel on the tailgate; black
  roof spoiler with high brake lamp; black mirror caps. It keeps a small rear quarter window
  (typical for the type, and it sets it apart from the other two 90s cars).
- **Triangles:** 17,326 full (was 16,894), 4,446 mid.

### marten_20
- **Reference:** early-90s European front-drive 2.0 hot hatch, about 4.05 m long, 1.7 m wide,
  1.40 m tall, 2.47 m wheelbase, 15-inch wheels. Wedge profile: a low sloping nose with slim
  lamps that almost meet over a grille-less nose, a body-colour bumper with a wide intake, a
  rising belt line, black body-side mouldings, a full-width red tail band, twin tips.
- **Changes:** own body: nose height 0.58 m, rake 36°, 61° windshield, belt rising 7 cm to the
  tail, falling roof (`roof_arc` 0.40), no quarter glass. Own kit: slim wedge lenses with amber
  signals under their outer ends, dark housings, small reflectors, black slot and badge between
  the lamps; wide trapezoid intake split by a body-colour bar with rectangular fogs at its ends,
  black chin splitter; black side mouldings and skirts; tail: lamps with amber inboard sections
  and a red reflector panel across the tailgate between them (one red band), badge, black lower
  valance, twin tips, body-colour roof spoiler with high brake lamp. Fuel filler moved to the
  left.
- **Triangles:** 17,520 full (was 17,056), 4,188 mid.

### lindwurm_4t
- **Reference:** 2010s European all-wheel-drive hot hatch, 5-door, about 4.28 m long, 1.80 m
  wide, 1.44 m tall, 2.63 m wheelbase, 18-inch wheels. Wide and planted, a sharp shoulder
  ("tornado") line, a high belt and lower greenhouse, slim LED head lamps with a DRL signature and
  horizontal grille bars running into them, a big honeycomb lower intake with side ducts, a large
  roof spoiler with end plates, horizontal two-piece LED tail lamps, diffuser and quad tips.
- **Changes:** belt +6 cm, shoulder line at 0.66 H, 62° windshield with 10 cm wrap, short roof,
  0.26 m C-pillar, box arches. Kit: slim grille with two bars running across into the lamps and a
  red pinstripe (accent changed from blue-on-blue to red), badge; nose-top LED lenses plus a face
  lens with an L-shaped DRL brow and a projector; trapezoid honeycomb intake with angled bladed
  side ducts; tail lamps moved in from the corners with an L light-bar signature, black hatch
  garnish with badge, high brake lamp under the end-plated spoiler, plate, reflectors;
  split-spoke wheels.
- **Triangles:** 18,152 full (was 17,474), 4,670 mid.

## Checks run
`cargo fmt`, `cargo clippy -p car-assets --all-targets -- -D warnings`, `cargo test -p
car-assets` (29 passed), `validate-car-asset` (39 cars ok), `gen-car-mesh --check`; `git status`
after a full `gen-car-mesh` shows only the four cars' meshes changed.
`npm run wasm && node e2e/mod_coverage.mjs vireo_xs stoat_16v marten_20 lindwurm_4t`: 1 of 136
mods shows nothing on each car, `drivetrain_shifter_short` (interior part, already on the known
list in `mod-visuals.md`). With `SHOTS_ALL` I looked at the wing, front lip, side skirts,
splitter, bull bar and stripes vinyl on stoat_16v and lindwurm_4t: all sit on the new bodies,
none float or hide inside the shell.

## Notes for the lead
- **Door count:** `stoat_16v`'s spec sheet (and the old style comment for `vireo_xs`) says
  three-door, but all four lattices have detachable front and rear doors. Doors must keep
  detaching per lattice region, so the meshes are 5-door hatches. A 3-door look needs a lattice
  change (physics), out of scope here.
- Pre-existing, shared: the black arch liner leaves a gap at the top of the rear arches through
  which the rear spring shows from low side angles (seen on these cars; not changed here since
  it is shared `trim.rs` code).

## Generator requests
None needed. Nice to have: a public `Kit` helper for a face lens of any convex outline (what
`face_poly` does locally here), so other styles can stop using boxes for lamps.

## Contract change requests
None.
