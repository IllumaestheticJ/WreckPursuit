# car-shapes phase 2: exotic group

- **Workstream:** car-shapes phase 2, group `exotic` (brief: `docs/integration/car-shapes-phase2-brief.md`)
- **Branch:** `ws/car-shapes-exotic` (from `origin/ws/car-shapes` at `626e7a65`)
- **Cars:** `lucciola_350`, `fulmine_v10`, `halberd_40`, `ombra_v12`, `ostwind_6t`, `kagero_hx`
- **Files edited:** `tools/assets/src/styles/performance/{exotic,supercar}.rs` (style files),
  `data/panels/meshes/{,mid/}<car>.json` for the six cars (regenerated),
  `tools/assets/tests/assets.rs` (the `fulmine_v10` row of `TRIS_BEFORE_INTEGRATED_SIGNALS`,
  see below), `docs/media/car-shapes/exotic/`.
- Render meshes only: no change to `data/cars`, `data/lattices`, panel specs, the sim or any
  shared generator file (style.rs, profile.rs, mesh.rs, builder.rs, wheels.rs, mechanical.rs,
  trim.rs, kit.rs). A full `gen-car-mesh` run changed only these six cars' meshes.

## Common changes (all six)
- **Greenhouse height.** The lattice-derived belt sat at ≈ 0.67 m on cars 1.14–1.30 m tall, so
  the side glass was ≈ 43 % of the body height (the real class is ≈ 28–33 %), which made every
  car read as a tall-cabin coupe. `belt_dz` +0.07…+0.08 (Ostwind +0.04: that class has
  taller glass) and `cowl_h` +0.05…+0.16 (the hood rises toward the windshield, the mid-engine
  wedge) fix the proportion. Noses lowered to 0.45–0.50 m.
- **Body section and stance.** Haunches 0.07–0.08 and front haunches 0.045–0.07 (swollen
  fenders over narrower doors), sill tuck 0.07–0.11 (less slab side), more tumblehome on the
  canopy cars, the body dropped a further 1–2 cm on its wheels.
- **Shaped panels instead of boxes.** New helpers local to `supercar.rs` (`pub(super)`, also used
  by `exotic.rs`): `top_poly` (outline draped over the hood / deck), `side_poly` (outline on the
  body side or greenhouse), `end_poly` (outline on the nose / tail face, following the
  plan-rounded corners via `face_y`), `flip_x`, `stadium` (pill outline) and `quarter_window`
  (glass + seal behind the door glass where the loft paints that greenhouse panel). Lamps,
  intakes, scoops and vents now have their real outlines.
- **Own lamp signatures (rule 9).** No maker-identifying graphics: no Y / hexagon motifs, eye
  sockets, four-dot lamps, jewel elements or round quad tails. Each car carries its own
  fictional maker's signature, which is also its night identifier:
  Lucciola pill tails, Fulmine lightning bolt, Halberd hooked blade, Ombra double chevron,
  Ostwind triple bars + stepped dashes, Kagero twin parallel bars. `signatures.jpg` shows them.
- **Turn signals placed by hand** in each lamp (front ones lifted above their lens so the
  `lights_sit_outside_the_shell` ray test sees them); `signals_beside_lamps` only fills in what a
  kit has not built.

## Pass 3 (phase 3 generator, merged from `ws/car-shapes` at `d8e0a524`)
`shape3` (local to `supercar.rs`) sets the new knobs from the wheel stations for all six:

| car | nose / tail / waist plan scale | scallop ahead of rear wheel | shoulder | bulge | rocker in | fender rise F / R |
|---|---|---|---|---|---|---|
| lucciola_350 | 0.86 / 0.92 / 0.95 | −0.05 | – | 0.03 | 0.04 | 0.02 / 0.03 |
| fulmine_v10 | 0.88 / 0.95 / 0.96 | −0.06 | – | 0.015 | 0.04 | 0 / 0.03 |
| halberd_40 | 0.84 / 0.90 / 0.94 | −0.07 | – | 0.035 | 0.05 | 0.03 / 0.04 |
| ombra_v12 | 0.86 / 0.97 / 0.95 | −0.07 | +0.02 | 0.01 | 0.04 | 0 / 0.02 |
| ostwind_6t | 0.90 / 0.94 / 0.94 | – | – | 0.035 | 0.04 | 0.06 / 0.05 (hood valley 0.06 → 0.02) |
| kagero_hx | 0.87 / 0.94 / 0.95 | −0.06 | +0.015 | 0.02 | 0.04 | 0.02 / 0.03 |

The dark side intakes now sit in real recesses (the scallop) instead of being painted on a flat
side. The panel helpers drape their face over the surface (concentric rings) and side panels
sit 1 cm out (`SIDE_LIFT_M`) because the loft's flat facets on sculpted sides stand up to that
far off the smooth surface. Lucciola tails moved to `Kit::proj_cluster` (cast pill lens, inner
amber segment, bezel); the other lamps keep their kit shapes because their signature strokes
need a dark lens under a bright graphic. Ostwind side marker enlarged to ≥ 4 cm (the mid LOD's
3.5 cm clustering dropped it). Waiting on phase 3b for the raked end faces, the roof-arc
exponent and the sail / tail-corner fans.

## Pass 3b part 1 (merged from `ws/car-shapes` at `b10e5e14`)
`Shape3` also sets the end faces, plan bows and lower lean-in:

| car | nose rake / chin | tail rake / chin | nose bow / tip | tail bow / tip | lean-in |
|---|---|---|---|---|---|
| lucciola_350 | 35° / 0.06 | 10° / 0.06 | 0.30 / 0.60 | 0.25 / 0.70 | 0.03 |
| fulmine_v10 | 30° / 0.05 | 8° / 0.08 | 0.12 / 0.90 | 0.08 / 0.92 | 0.02 |
| halberd_40 | 30° / 0.06 | 12° / 0.08 | 0.25 / 0.60 | 0.12 / 0.85 | 0.04 |
| ombra_v12 | 35° / 0.04 | 5° / 0.08 | 0.12 / 0.90 | 0.08 / 0.92 | 0.02 |
| ostwind_6t | 25° / 0.05 | 15° / 0.06 | 0.30 / 0.55 | 0.30 / 0.65 | 0.03 |
| kagero_hx | 28° / 0.05 | 10° / 0.07 | 0.20 / 0.70 | 0.10 / 0.88 | 0.03 |

(The nose plan-width scales in `shape3` went back up to 0.90–0.94 since the bows now do most
of the nose taper.) Kit parts follow the new faces: `shear_to_faces` (local) moves every
detail / light part the kit built on the old vertical end plane back onto the raked face,
bowed corners and chin, sampling `face_y` (which now reads the ring's real height range), and
`end_poly` builds on the plane and lets the shear place it. Parts the generator built before
the kit, and projected lamps, are left alone. Top-surface lamps are anchored where the hood
starts behind the face (`k.yn - p.face_depth(true)`). Lucciola's front parking / signal strips
are now projected (`LampView::Front`, wrapping with a 25° yaw). Tail bows on the sharp-cornered
cars were kept small: larger ones make the generator's tail-corner fans (phase 3b item 5)
stand out as flaps.

## Per car
References are body types and eras only; no single real car is reproduced.

### lucciola_350
Reference: a mid-90s Italian mid-engine V8 berlinetta. 4.25 × 1.90 × 1.17 m, 2.45 m wheelbase,
cab-forward cabin, rounded low nose with pop-up lamps, a slatted centre intake, one smooth
teardrop scoop in each rear quarter, a near-upright rear window between flying buttresses, a
louvred engine lid.
Changes: belt +0.07, cowl +0.05, nose 0.47 m, rounder plan corners, bigger haunches; five
horizontal side strakes replaced by one teardrop scoop with a body-colour lip; full-width nose
slats replaced by a squared-oval intake with four slats and corner ducts; hood vents removed;
quad round tails replaced by one wide pill lamp per side in a chrome surround (amber inner end).
Triangles: 18898 full (was 18170; table row updated), 5014 mid.

### fulmine_v10
Reference: a 2010s Italian mid-engine V10 AWD supercar. 4.46 × 1.95 × 1.17 m, ≈ 2.62 m
wheelbase, the hood running in one straight line from the nose lip to the windshield, black
lower front with big corner intakes, slim angular head lamps, a sharp diagonal line from the
front wheel into a big quarter intake, louvred engine cover, twin exhaust tips each side low; no
fixed wing on the base car.
Changes: wedge hood (`cowl_h` 0.14, nose 0.45 m), belt +0.08; angular lens with a
lightning-bolt DRL, projector and amber tip; two trapezoid front intakes with a slanted blade,
centre slot; side diagonal groove + leaning quarter intake with a blade; the fixed carbon wing
replaced by an integrated lip spoiler; slatted black fascia on the lower half of the tail;
bolt-shaped tail strokes with an amber corner bar; `ExhaustKind::Quad`; no door handles or fuel
door (flush on the type).
Triangles: 21876 full (was 19884; table row updated), 6665 mid.

### halberd_40
Reference: a 2020s British carbon-tub twin-turbo V8 supercar. 4.54 × 1.93 × 1.20 m, 2.67 m
wheelbase, teardrop glass canopy, dihedral doors with a deep air channel, a double-skin rear
deck with flying buttresses over a clear engine cover, a full-width active airbrake lying flush
on the deck, hair-line tail lamps.
Changes: belt +0.08, cowl +0.12, nose 0.45 m; box lamps replaced by hooked blade lamps (black
blade along each nose-top corner, LED stroke tracing it and its hook, lens in the hook) and a
separate vertical corner intake with a carbon vane; trapezoid hood duct; spiky carbon deck fins
replaced by a glass engine cover between flying buttresses with dark channels; the wing on tall
stands replaced by a flush full-width airbrake; the tail hair-line folds down at its outer ends,
amber outer tip.
Triangles: 20886 full (was 19804), 6039 mid.

### ombra_v12
Reference: a 2010s Italian V12 flagship with scissor doors. 4.78 × 2.03 × 1.14 m, 2.70 m
wheelbase: the lowest, widest and flattest of the group, long narrow angular lamps running back
along the fenders, huge corner intakes, hood creases converging at the nose, a diagonal side
groove into a giant intake, shoulder air ears, louvred glass engine cover, an exposed black
rear.
Changes: belt +0.07, cowl +0.16, nose 0.45 m, haunch 0.08, lower stance; new lamps (housing,
lens, double-chevron DRL, projector, amber tip); trapezoid corner intakes split by two
body-colour bars into three slots, mesh bars that follow the slanted edge; V hood creases and
tapered flush ducts; side groove; double-chevron tails; centre exhaust in a wide pill surround.
Paint: factory navy (kept, per the lead; it hides detail in the greybox shots).
Triangles: 21552 full (was 19286), 6539 mid.

### ostwind_6t
Reference: a 2020s German rear-engine flat-six flagship coupe (2+2). 4.54 × 1.90 × 1.30 m,
2.45 m wheelbase; the defining cue is the roofline falling in one continuous line from the
windshield header to the tail lip, with a small rear quarter window behind the door, upright
oval lamps on high front fenders either side of a low front lid, wide hips, a deployable wing on
the engine lid, a full-width light bar.
Changes: belt +0.04; `roof_arc` 1.0 → 0.55, `rear_glass_bow` 0.05, `roof_len_trim` −0.18 and
`deck_drop` 0 so the fastback line meets the belt at the tail instead of dropping steeply onto a
flat deck; a kit quarter window (glass on a black seal); the oval lamps now face forward (normal
tilted 40°) with three horizontal light bars (the lower one the amber signal) instead of four
dots; three stepped dashes per side in the tail bar instead of four brake dots.
Triangles: 19854 full (was 18306), 6164 mid.

### kagero_hx
Reference: a 2010s Japanese mid-engine hybrid V6 supercar. 4.49 × 1.94 × 1.21 m, 2.63 m
wheelbase, slim angular head lamps, a narrow black upper grille, tall corner intakes, one big
side scoop that opens from the door into the quarter intake under a floating C-pillar, black
roof, thin full-width tail with folded ends, quad centre exhaust.
Changes: belt +0.07, cowl +0.08, nose 0.50 m, bigger haunches; new lamps (housing, twin
parallel light bars, projector, amber inner end); upper grille over a straight silver line;
corner intakes with one slanted blade (the X-shaped blades are gone); hood vents removed; the
stack of door / quarter rectangles replaced by one continuous scoop with folded lips and a blue
hybrid line; the black roof sheet no longer z-fights (denser patch, 1 cm off the paint).
Triangles: 21074 full (was 18949), 6772 mid.

All full meshes are inside the hero budget (≤ 22k), all mid meshes inside 7k.

## Checks
`cargo fmt`, `cargo clippy -p car-assets --all-targets -- -D warnings`, `cargo test -p
car-assets`, `validate-car-asset` (all cars ok), `gen-car-mesh --check`.
Mod coverage (`e2e/mod_coverage.mjs` on the six cars, 136 mods each, 6 not for Exotic):
12 car/mod pairs show nothing, all on the known list in `mod-visuals.md`: the short shifter
(all six), the fuel rail under the mid-engine covers (lucciola, fulmine) and sport tyres on the
low-profile exotics (lucciola, fulmine, ombra, ostwind). The Ombra lamp halo, a known miss
before, now shows. Body kits, lips, wings, vinyls, bull bars and diffusers all show on the new
bodies.

## Shots
`docs/media/car-shapes/exotic/<car>.jpg`: before (left) / after (right), front 3/4, rear 3/4,
side, top. `overview.jpg`: front 3/4 and side, before / after, all six. `signatures.jpg`:
close-ups of each car's lamp signature.

## Generator requests (sent to the lead)
1. Raked nose / tail face: the end face is a vertical plane (nose_rake only drops the top);
   supercars need the face leaning back 30–60° with an undercut chin.
2. Plan-view taper: half width constant except the corner rounding; needs a nose taper and a
   waist pinch between the arches.
3. Side section curvature: a body-side bulge knob and per-station scallops, so intakes can be
   recesses in the loft rather than dark panels on a flat side.
4. Roof arc profile: the parabola stays flat then drops; needs a bias knob for an even fall.
5. Pre-existing facets on the black A-pillar sail and on the tail corners of sharp-plan cars.
Nice to have: shared `top_poly` / `side_poly` / `end_poly` in `kit.rs`.

## Known gaps
- Items 1–5 above.
- Mid-engine doors (scissor / dihedral) are drawn as closed panels only, as before.
