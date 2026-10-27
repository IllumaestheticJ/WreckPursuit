# car-shapes phase 2: exotic group

- **Workstream:** car-shapes phase 2, group `exotic` (brief: `docs/integration/car-shapes-phase2-brief.md`)
- **Branch:** `ws/car-shapes-exotic` (from `origin/ws/car-shapes` at `626e7a65`)
- **Cars:** `lucciola_350`, `fulmine_v10`, `halberd_40`, `ombra_v12`, `ostwind_6t`, `kagero_hx`
- **Files edited:** `tools/assets/src/styles/performance/{exotic,supercar}.rs` (style files),
  `data/panels/meshes/{,mid/}<car>.json` for the six cars (regenerated),
  `tools/assets/tests/assets.rs` (one row of `TRIS_BEFORE_INTEGRATED_SIGNALS`, see below),
  `docs/media/car-shapes/exotic/`.
- Render meshes only: no change to `data/cars`, `data/lattices`, panel specs, the sim or any
  shared generator file (style.rs, profile.rs, mesh.rs, builder.rs, wheels.rs, mechanical.rs,
  trim.rs, kit.rs). A full `gen-car-mesh` run changed only these six cars' meshes.

## Common changes (all six)
- **Greenhouse height.** The lattice-derived belt sat at ≈ 0.67 m on cars 1.14–1.30 m tall, so
  the side glass was ≈ 43 % of the body height (the real class is ≈ 28–33 %), which made every
  car read as a tall-cabin coupe. `belt_dz` +0.07…+0.08 (Ostwind +0.04: that class has
  taller glass) and `cowl_h` +0.05…+0.16 (the hood rises toward the windshield, the mid-engine
  wedge) fix the proportion. Noses lowered to 0.45–0.50 m.
- **Shaped panels instead of boxes.** New helpers local to `supercar.rs` (`pub(super)`, also used
  by `exotic.rs`): `top_poly` (outline draped over the hood / deck), `side_poly` (outline on the
  body side or greenhouse), `end_poly` (outline on the nose / tail face, following the
  plan-rounded corners via `face_y`), `flip_x`, and `quarter_window` (glass + seal behind the door
  glass where the loft paints that greenhouse panel). Lamps, intakes, scoops and vents now
  have their real outlines.
- **Turn signals placed by hand** in each lamp (front ones lifted above their lens so the
  `lights_sit_outside_the_shell` ray test sees them); `signals_beside_lamps` only fills in what a
  kit has not built.

## Per car

### lucciola_350
Reference: a mid-90s Italian mid-engine V8 berlinetta. 4.25 × 1.90 × 1.17 m, 2.45 m wheelbase,
cab-forward cabin, rounded low nose with pop-up lamps, an oval slatted centre intake, a single
smooth teardrop scoop in each rear quarter, a near-upright rear window between flying
buttresses, a louvred engine lid and twin round tail lamps per side in a black mesh panel.
Changes: belt +0.07, cowl +0.05, nose 0.47 m; the five horizontal side strakes (a cue from an
earlier 80s flagship, not this car) replaced by one teardrop scoop with a body-colour lip; the
full-width slatted nose replaced by a squared-oval intake with four slats and corner ducts;
hood vents removed (the reference hood is clean).
Triangles: 17998 full (was 18170), 4672 mid.

### fulmine_v10
Reference: a 2010s Italian mid-engine V10 AWD supercar. 4.46 × 1.95 × 1.17 m, 2.62–2.65 m
wheelbase, the hood running in one straight line from the nose lip to the windshield,
hexagon-themed black lower front, slim angular head lamps with a Y-shaped DRL, a sharp diagonal
line from the front wheel into the big quarter intake, louvred engine cover, thin Y tail strokes,
twin exhaust tips each side low; no fixed wing on the base car.
Changes: wedge hood (`cowl_h` 0.14, nose 0.45 m) and belt +0.08; angular lens + Y DRL +
projector + amber tip; two trapezoid front intakes with a slanted body-colour blade, centre
slot; side diagonal groove + leaning quarter intake with a blade; the fixed carbon wing replaced
by an integrated lip spoiler; black fascia limited to the lower half of the tail, diagonal
honeycomb on it; Y tail strokes with an amber outer tip; `ExhaustKind::Quad`; no door handles or
fuel door (flush in the reference).
Triangles: 21056 full (was 19884; `TRIS_BEFORE_INTEGRATED_SIGNALS` row updated, +5.9 % from the
detail pass), 6296 mid.

### halberd_40
Reference: a 2020s British carbon-tub twin-turbo V8 supercar. 4.54 × 1.93 × 1.20 m, 2.67 m
wheelbase, teardrop glass canopy, deep black "eye socket" lamps that wrap over the nose
corners and carry an intake, dihedral doors with a deep air channel, a double-skin rear deck
with flying buttresses over a clear engine cover, a full-width active airbrake that lies flush
on the deck, hair-line tail lamps.
Changes: belt +0.08, cowl +0.12, nose 0.45 m; the box lamps replaced by teardrop sockets (top
and face parts), an LED eyebrow, a lens and a vertical slot + carbon vane; trapezoid hood duct;
the spiky carbon deck fins replaced by a glass engine cover between flying buttresses with dark
channels; the wing on tall stands replaced by a flush full-width airbrake; the tail hair-line
folds down at its outer ends, amber outer tip.
Triangles: 20594 full (was 19804), 6014 mid.

### ombra_v12
Reference: a 2010s Italian V12 flagship with scissor doors. 4.78 × 2.03 × 1.14 m, 2.70 m
wheelbase: the lowest, widest and flattest of the group, sharp Y motifs everywhere, long narrow
angular lamps with a Y DRL running back along the fenders, huge corner intakes split by a Y
blade, two hood creases converging into a V at the nose, a diagonal side groove into a giant
intake, shoulder air ears, louvred glass engine cover, Y tails over an exposed black rear,
hexagonal centre exhaust.
Changes: belt +0.07, cowl +0.16, nose 0.45 m; new lamps (housing, lens, Y DRL, projector, amber
tip); corner intakes rebuilt as trapezoids with a body-colour Y blade and mesh bars that follow
the slanted edge; V hood creases and tapered flush ducts in place of the spine; side groove.
Paint changed from dark navy (which hid every detail) to yellow, distinct from the other five.
Triangles: 20602 full (was 19286), 6099 mid.

### ostwind_6t
Reference: a 2020s German rear-engine flat-six flagship coupe (2+2). 4.54 × 1.90 × 1.30 m,
2.45 m wheelbase; the defining cue is the roofline falling in one continuous line from the
windshield header to the tail lip, with a small D-shaped rear quarter window behind the door,
upright oval lamps on high front fenders either side of a low front lid, wide hips with intakes
ahead of the rear wheels, a deployable wing on the engine lid, a full-width light bar.
Changes: belt +0.04; `roof_arc` 1.0 → 0.55, `rear_glass_bow` 0.05, `roof_len_trim` −0.18 and
`deck_drop` 0 so the fastback line meets the belt at the tail instead of dropping steeply onto a
flat deck; a kit quarter window (glass on a black seal) behind the door glass; the round lamps
now face forward (normal tilted 40°) instead of lying flat on the fender tops.
Triangles: 19252 full (was 18306), 5824 mid.

### kagero_hx
Reference: a 2010s Japanese mid-engine hybrid V6 supercar. 4.49 × 1.94 × 1.21 m, 2.63 m
wheelbase, slim "jewel" head lamps with a row of small LED elements and a dagger DRL, a narrow
black upper grille over a silver blade that sweeps up under the lamps, tall corner intakes, one
big side scoop that opens from the door into the quarter intake under a floating C-pillar,
black roof, thin full-width tail with Z ends, quad centre exhaust.
Changes: belt +0.07, cowl +0.08, nose 0.50 m; new lamps (housing, dagger DRL, four jewel
elements, amber inner end); upper grille + silver blade; corner intakes with one slanted blade
(the X-shaped blades are gone); hood vents removed; the stack of door / quarter rectangles
replaced by one continuous scoop with folded lips and a blue hybrid line; the black roof sheet
no longer z-fights (denser patch, 1 cm off the paint; `roof_sheet` is local to `supercar.rs`).
Triangles: 20077 full (was 18949), 6243 mid.

All full meshes are inside the hero budget (≤ 22k), all mid meshes inside 7k.

## Checks
`cargo fmt`, `cargo clippy -p car-assets --all-targets -- -D warnings`, `cargo test -p
car-assets`, `validate-car-asset` (all cars ok), `gen-car-mesh --check`. mod coverage: see below.

## Shots
`docs/media/car-shapes/exotic/<car>.jpg`: before (left) / after (right), front 3/4, rear 3/4,
side, top. `docs/media/car-shapes/exotic/overview.jpg`: front 3/4 and side, before / after, all
six.

## Generator requests
None needed. Nice to have later: a shared `top_poly` / `side_poly` / `end_poly` in `kit.rs`
(other groups would likely use them; kept local here per the brief).

## Known gaps
- Shared-generator facets on the black A-pillar sail (seen on every car before this pass).
- The tail corners of the sharp-plan cars (Ombra, Fulmine) show small facets from the generic
  end rounding.
- Mid-engine doors (scissor / dihedral) are drawn as closed panels only, as before.
