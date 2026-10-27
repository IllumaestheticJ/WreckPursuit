# car-shapes phase 2: traffic group

- **Workstream:** car-shapes phase 2, group `traffic` (brief: `docs/integration/car-shapes-phase2-brief.md`)
- **Branch:** `ws/car-shapes-traffic` (from `origin/ws/car-shapes`)
- **Files changed:** `tools/assets/src/styles/traffic.rs` (my style file), my seven cars' meshes
  (`data/panels/meshes/<id>.json`, `meshes/mid/<id>.json`), my seven rows of
  `TRIS_BEFORE_INTEGRATED_SIGNALS` in `tools/assets/tests/assets.rs` (the phase 1 doc says a
  per-car pass updates its row), shots in `docs/media/car-shapes/traffic/`.
- **Not changed:** shared generator code, `debug_sedan` (its mesh is byte-identical; the
  helpers it uses in `traffic.rs` are untouched, new helpers were added below them), physics,
  lattices, car defs. `gen-car-mesh --check` passes for every car.

## New local helpers (in `traffic.rs`, used by my cars only)
- `van_box(b, p, windows)`: the generator keeps the deck start ≥ 0.25 m ahead of the tail, so
  every van had a stepped upper rear (and floating hinges / roof markers: the known gap from
  phase 1). The greenhouse section at the roof's rear edge is now extruded back to the tail:
  `rear_corner_l/_r` (quarters), `roof_extension` (roof), `rear_door_skin` (trunk = barn doors,
  painted, livery UVs), with the barn seam, hinges, handle, high brake lamp and (European vans)
  two rear-door windows (`rear_door_glass`, glass kind / material, its own seal) on the flat
  face.
- `cab_back_window`: pickup cab-back glass with seal (and a sliding centre pane).
- `door_handles_at`, `door_cut`: crew-cab handles on recesses and the rear door's cut lines
  (the lattice has no hinged rear door on `hauler_1500`).
- `conform_to_faces` (pass 3b): moves kit detail / light parts near an end back by the measured
  set-back of a raked / chinned end face (projected lamps and body parts untouched).
- `head_lamp_bowls` (pass 3b): projector bowls + chrome reflector rings via `lamp::element`.
- `proj_lamps`, `proj_signals`, `rect` (pass 3): head / tail lamps and turn signals as lenses
  projected onto the body with the phase 3 `crate::lamp` module (any outline, wrapped round
  the corner by a yaw, optional bezel), mirrored per side. Used on every car except the hauler,
  whose lamps stay on `lens_poly` (a polygon lens on the end face, far cheaper): it is at
  10,999 of 11k triangles.
- `frame_front`, `plate_on_face` (a plate on a bumper bar that stands proud), and
  `sliding_door_track_at`.

## Pass 3 (phase 3 generator knobs, 2026-10-27)
Merged `ws/car-shapes` (phase 3) and reworked all seven again:
- Projected lenses with wrap: tansy's teardrop head lamps (20°) and D-pillar tail lamps (18°)
  in black bezels, the beater's and furrow's amber corner lamps wrapping onto the fender,
  satchel's swept lamps (25°) in a bezel, tumbril's square lamps in a bezel, the drayman's
  stacked lamps; all van tail lamps now sit on the flush rear face (they stood proud of the
  rounded corner before) and the lower barn hinges moved inboard of them.
- Van / truck sections: `tumblehome_m` 0.045–0.08 with a 4–6 cm glass ledge (drayman,
  satchel, tumbril, hauler, furrow); the local `square_up_sides` warp is gone. A slight side
  bow (`side_bulge_m` 0.01–0.03) and tucked rockers on the vans, tansy and the hauler.
- Plan views: tansy's nose and tail taper (0.93 / 0.95), the European vans' noses narrow toward
  the bumper (0.92 satchel, 0.95 tumbril), the hauler's nose 0.96.
- tansy: shorter overhangs (−8 cm each end), rounder section (bulge 0.03 at 60 %, rockers in).
- hauler: crisp full-length shoulder line (`side_sculpt`, +2 cm band under the belt).
- Phase 3b part 1 (merged `b10e5e14`): tansy gets a raked bumper face (22°), a slightly bowed nose in plan, a roof narrowing to the rear
  (`roof_plan_taper` 0.08) and a tucked lower body; the European vans get raked plastic
  bumpers (satchel 16°, tumbril 8°). Chins (tansy 5 cm, satchel 6 cm, tumbril 4 cm) and tansy's
  12° tailgate lean were tried and are off until the generator's face-bottom clipping fix (the
  lead's spike warning); a low-angle check of all seven (front and rear 3/4 at 0.3 m) shows no
  spikes with them off. A local `conform_to_faces`
  slides the kit's end-mounted parts (grilles, plates, intakes, fog lamps, lips) back onto the
  raked face. Projector bowls with chrome reflector rings (`lamp::element`) in tansy's and
  satchel's lamps (one / two) and a big round reflector in tumbril's square lamps.
- The drayman and furrow keep upright faces (the drayman for budget, the furrow because a 90s
  steel-bumper truck has none).
- Phase 3 addenda (merged `d64c04b7`): the hauler's and drayman's tall noses now come from
  `hood_dz` (front +0.12 / +0.10 m, `nose_h` 1.21) and the local nose-lift vertex warp is gone;
  the three vans use `deck_min_m` 0.04, so their rear glass / doors reach the tail and
  `van_box` only adds the painted door skin over the glass (solid doors on the drayman, two
  windows on the European vans) plus seam, hinges, handle and brake lamp; the European vans get
  black arch lips (`arch_lip_finish`) and black lower cladding (`cladding_z_m` 0.36 satchel /
  0.42 tumbril); tansy's window line kicks up at the rear (`dlo_kink_m` 0.06).

## Per car
Headline counts are pass 2; final pass 3 counts (full / mid): see the table under "Final
counts" at the end (all within traffic 11k / mid 7k; drayman on 14 wheel segments, hauler on 12).

Reference descriptions are generic (no real names). Triangle counts: full (phase 1) / mid
(phase 1). Budgets: traffic 4k–11k, mid 2k–7k.

### hauler_1500: 10999 (10625) / 5869 (5411)
**Reference:** 2010s American full-size half-ton crew-cab pickup with a ~1.7–1.8 m bed: about
5.9 m long, 2.05 m wide, 1.9 m tall; tall flat hood front ~1.25 m up, belt ~1.3 m, near-upright
cab back, long four-door cab (~2.4 m windshield base to cab back), big box flares, a tall
rectangular grille with heavy horizontal bars between tall headlamps, chrome bumper with fog
lamps and tow hooks, towing mirrors, cab clearance lamps, running boards.
**Before:** read as a low regular-cab long-bed sport truck (one door, 0.65 m roof, small grille).
**Changed:** cab moved back (`cabin_shift_m` −0.50) and the windshield base forward
(`hood_length` 1.62), so the cab is ~2.4 m long with a rear-door window and the bed ~1.9 m;
belt +8 cm, nose lifted 12 cm (pass 2 `lift_front`, now `hood_dz`) into a tall flat front; upright cab back
(`rear_glass_rake_deg` 6) with a roof run back to it; new kit: three-section chrome grille with
two heavy bars and a blank badge, tall lamps (shaped: inner lower corner cut toward the grille bar) in chrome bezels with an amber LED strip (signal),
chrome bumper with two intakes, fog lamps, air dam, red tow hooks, centre plate; crew-cab chrome
handles on recesses and rear-door cut lines; cab-back sliding window; running boards on
brackets; tailgate spoiler cap, badge, tail lamps with a white reverse section; chrome step
bumper with hitch. Wheels 14 → 12 segments to stay under 11k.

### furrow_24: 9788 (9414) / 4866 (4496)
**Reference:** 90s compact regular-cab pickup: ~4.75 m long, 1.69 m wide, 1.6 m tall, short
upright cab with a flat roof and vertical cab back, flat hood, square sealed lamps with
wraparound amber corners, chrome bar grille and bumper, two-tone tape stripe, 6-ft bed,
vertical tail lamps, step bumper.
**Changed:** squared-off section and ends (flat hood, small plan rounding, sharp crease, tighter shoulder, wider belt ledge); upright cab back (`rear_glass_rake_deg` 5) with the roof run back to it, slightly
thinner A-pillars; kit: chrome lamp bezel round head + corner lamps, grille centre post and
blank badge, black rubber strip on the chrome bumper, slotted valance, cab-back sliding window,
tail lamps with reverse section, recessed (blank) lettering panel on the tailgate, red tape
stripe (accent tint) along the body.

### tansy_13: 8424 (8214) / 4206 (4002)
**Reference:** 2000s European/Japanese five-door supermini: ~3.85 m long, 1.67 m wide, 1.5 m tall,
short sloping bonnet (~0.85 m) running into a fast windshield, tall softly arched roof, rising
belt, near-vertical tailgate with tall tail lamps, roof spoiler with the high brake lamp,
body-colour bumpers with a small upper slot and a wide lower intake, side repeaters.
**Before:** long flat bonnet and a flat box roof (read as a small MPV / old estate), black strip
bumpers, a big floating flat spoiler plate.
**Changed:** `hood_length` 0.86 (windshield base 16 cm forward), raised cowl, `roof_arc` 0.45,
more crown, rounder section (more tumblehome, shoulder and sill rounding), rounder plan at both
ends, front and rear haunches, steeper belt rake, plan-view A- and C-pillar lean, wider D-pillar, fuel door left; kit: shaped teardrop
lamps (high and wide at the fender, narrowing and dropping toward the grille) in a black bezel
with an amber signal under them, tall tail lamps whose inner edge follows the D-pillar, slot grille with chrome bar and
blank badge, wide lower intake with fog lamps, black lower lip and rear diffuser, rear
reflectors, chrome tailgate strip, wedge roof spoiler with third brake lamp, door rubbing
strips, side repeaters (replace US-style side markers).

### junkyard_beater: 9078 (8836) / 4441 (4473)
**Reference:** late-80s compact front-drive three-box sedan: ~4.3 m, 1.66 m wide, 1.38 m tall,
upright glasshouse with thin pillars and flat glass, sealed-beam lamps with amber corners,
black 5-mph bumpers, rub strips, chrome drip rails; high mileage.
**Changed:** taller flat-glass cabin (roof +4 cm, upright rear glass), flat panels with a
sharp waist crease, square ends; thin pillars (A 6 cm, C 9 cm, B 10 cm), almost no glass wrap, small sail, sharper
ends, fuel door left; personality: the front-left fender is a replacement still in red-oxide
primer (accent-tinted patches on the fender) and a primer spot low on the left rear door; the rear bumper hangs 4 cm low on its left end (a
broken bracket); the cracked left tail lamp is taped with two crossed strips of silver tape.

### drayman_cargo: 10365 (10488) / 5211 (5248)
**Reference:** 2000s American full-size cargo van: ~5.7 m long, 2.0 m wide, 2.1 m tall, short
V8 nose (~1.2 m) under a tall windshield, belt ~1.3 m, slab-sided windowless box, flush barn
doors, stacked square lamps, wide bar grille, black bumpers, big two-glass mirrors, tall
vertical tail lamps in the rear corners.
**Before:** long low station-wagon nose, box tucked in above the belt, upper rear stepped
25 cm in, hinges and roof markers floating behind it.
**Changed:** `hood_length` 1.22, belt +6 cm, nose lifted 10 cm, box flares, black mirrors /
handles; `square_up_sides` 6 cm (pass 2; now `tumblehome_m`); `van_box` flush rear (solid barn doors); kit: stacked lamps
(head over amber signal) in a black surround, grille with chrome top bar and badge, tall corner
tail lamps, high brake lamp. Roof marker lamps removed (not fitted on vans this width).

### satchel_20: 9258 (9221) / 4733 (4650)
**Reference:** 2000s European front-drive high-roof panel van, the rounder kind: ~4.85 m long,
1.9 m wide, 2.25 m tall, short sloping bonnet that flows into a raked windshield, high belt,
slab sides, flush barn doors with windows, big swept lamps, slim grille, plastic bumpers.
**Changed:** separated from tumbril (they shared one style): `hood_length` 0.92 with a raised
cowl, rounder nose, `square_up_sides` 6.5 cm (pass 2; now `tumblehome_m`), `van_box` with rear-door windows; kit: shaped swept
lamps (tall at the fender, tapering in and down to the grille) in a black bezel with the amber
signal along their outer bottom, two-slat grille with badge, black lower
bumper and intake, rubbing strip, side repeaters, tall corner tail lamps (red / amber / red).

### tumbril_20: 9681 (9418) / 4772 (4709)
**Reference:** 2000s European front-drive high-roof panel van, the squarer kind with a sliding
side door: ~4.9 m long, 1.92 m wide, 2.3 m tall, upright square nose with a black plastic
front panel, square lamps, deep black bumper with fog lamps, sliding-door track.
**Changed:** steeper screen and more upright nose than satchel, `hood_length` 1.0; kit: square
lamps with amber outer columns, full black front panel with a three-slat grille and badge, fog
lamps and lower intake in the bumper, right-side sliding-door track at the belt, plus the shared
European-van rear and sides.

## Checks run
`cargo fmt`, `cargo clippy -p car-assets --all-targets -- -D warnings`, `cargo test -p
car-assets` (all pass; seven `TRIS_BEFORE_INTEGRATED_SIGNALS` rows rebased), `validate-car-asset`
(all cars ok), `gen-car-mesh --check` (no stale mesh; only my seven cars' files changed).
Mod coverage: see "Mod coverage" below.

## Mod coverage
`node e2e/mod_coverage.mjs <my 7 cars>` on the final meshes (rerun after pass 3 and the phase 3
addenda, same result): 19 car/mod pairs show nothing, all
of kinds already on the known list in `mod-visuals.md` (and the same as phase 1's hauler run):
short / sequential shifter and bucket seats behind van / truck cabins (drayman, satchel, tumbril,
and the short shifter on every car), the radar (drayman, hauler), sway bars under the car
(drayman, hauler, satchel, tansy, tumbril). Wings, lips, skirts, bumpers, bull bars and vinyls
show on all seven; a look check (`SHOTS_ALL`, front / rear) of the wing, ram bull bar, side
skirts and stripes on satchel_20, hauler_1500 and tansy_13 has them sitting on the new bodies
(the van wing on the roof's rear edge above the flush barn doors, the hauler's on the tailgate).

## Rule 9 check (brand cues)
No real names anywhere; grilles are generic bar / slat / mesh patterns, badges are blank
rectangles, lamp outlines are generic (square, stacked, teardrop, swept trapezoid); nothing is
a maker's signature shape.

## Shots
`docs/media/car-shapes/traffic/<car>_before_after.jpg` (front 3/4, rear 3/4, side; before row
over after row) and `overview_front.jpg` (all seven, before / after).

## Known gaps
- `tools/assets/README.md`'s triangle tables still list the phase 1 numbers for these cars (the
  numbers above are current); left for the lead's merge to avoid conflicts with other groups.
- The van rear corners are an extrusion of the roof-rear section, so the side panel there is a
  separate part (`rear_corner_*`, on the quarter region); it dents with the quarter, not with
  the rear doors.
- Projected lenses cost far more than boxes (≈ +1.2k on tansy); the hauler keeps its
  polygon lenses for budget.

## Generator requests
(Sent to the lead on 2026-10-27; phase 3 covered the lamp, section and tumblehome items.)
- A shared shaped-lens helper with bezel and inner elements (projector bowls, reflector rings,
  LED strips); `lens_poly` here is a local stand-in.
- Bodyside bow (convex side between sill and shoulder) and a per-station greenhouse plan taper.
- More arch shapes and a lower cladding band in the ring.
- (Done in phase 3 / addenda: tumblehome range, van deck near the tail, hood height, arch
  shapes and cladding.) Open: a painted rear-door panel mode for vans (glass only where a
  window is drawn), so `van_box`'s door skin could go.

## Final counts (after the phase 3 addenda)
| car | full | mid |
|---|---|---|
| drayman_cargo | 10929 | 5657 |
| furrow_24 | 10006 | 4954 |
| hauler_1500 | 10999 | 5909 |
| junkyard_beater | 9136 | 4471 |
| satchel_20 | 10627 | 5177 |
| tansy_13 | 10106 | 4336 |
| tumbril_20 | 10640 | 5125 |
