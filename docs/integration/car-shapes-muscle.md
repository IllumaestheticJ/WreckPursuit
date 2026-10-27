# car-shapes phase 2: muscle group

- **Workstream:** car-shapes phase 2, group `muscle` (brief: `docs/integration/car-shapes-phase2-brief.md`)
- **Branch:** `ws/car-shapes-muscle` (from `origin/ws/car-shapes`)
- **Cars:** `ironside_440`, `rampart_50`, `bulwark_400`, `brigand_400`
- **Files changed:** `tools/assets/src/styles/performance/muscle.rs` (the only style file edited),
  my four cars' meshes (`data/panels/meshes/<id>.json`, `meshes/mid/<id>.json`), my four rows of
  `TRIS_BEFORE_INTEGRATED_SIGNALS` in `tools/assets/tests/assets.rs` (the phase-1 doc asks a
  per-car detail pass to update its car's row), shots in `docs/media/car-shapes/muscle/`.
- No shared generator code, car defs, lattices or sim code changed. A full `gen-car-mesh` run
  changes only these four cars' meshes.

## Pass 3 (phase-3 generator knobs, merged from ws/car-shapes d8e0a524)
- **ironside_440:** coke-bottle plan (1.0 over both wheels, 0.935 at the doors, 0.95–0.96 at
  the ends); front fender tops 5 cm above the hood (replaces `hood_valley`), rear haunch tops
  3.5 cm above the deck; pontoon hips (+3.5 cm over the rear wheel), a shoulder line 8 cm under
  the belt, 2 cm side bulge, 3 cm rocker tuck.
- **rampart_50:** narrow shark-nose plan (0.90 at the nose), waist 0.95, tail 0.94; fender
  peaks 3.5 / 5 cm; pontoon hips, a shoulder line and a 3.5 cm cove low on the doors; rounded
  section (bulge 3 cm high on the side), 4.5 cm rocker tuck, 26 cm tumblehome. Projected
  lamps replace the local prism stop-gaps: angular head lamps wrapping onto the fenders (yaw
  25°, black bezel, amber outer segment, running-light lens), wedge tail lamps wrapping onto
  the quarters (bezel, red-on-red outer segment); the black tail panel now only fills the centre.
- **bulwark_400:** gentle waist (0.955) and tapered ends, fender tops 3 cm / haunches 2.5 cm,
  pontoon hips, shoulder line, 1.5 cm bulge.
- **brigand_400:** slab-straight sides with barely tapered ends, squared front fender tops 4 cm
  above the hood, one crisp mid-side character line; the corner tail lamps are now projected
  lenses swung 35° round onto the quarters (bottom strip = signal).
- The 70s cars use 18 wheel segments (was 20) to pay for the extra loft stations.
- Waiting on phase 3b (nose / tail rake and chin, hood / deck dz, roof-arc exponent, DLO
  kink, arch shapes, smooth shading) for a touch-up pass.

## Second pass (after the lead's "be bold" and "no brand cues" notes)
- **Proportions:** ironside hood +7 cm longer (cabin −0.42 m), roof −3 cm, coke-bottle hips
  (haunch 7 cm) with fender peaks either side of a sunken hood (front haunch 3.5 cm, hood valley
  2.5 cm), full nose-down rake (0.8°), longer overhangs (+5 / +6 cm). Rampart roof chopped
  (−7 cm), belt at the top of its range, rounder shoulders and deeper sill tuck, lower nose,
  shorter overhangs (−6 / −8 cm), fender peaks. Bulwark +9 / +8 cm overhangs, lower roof,
  square shoulders. Brigand +10 / +10 cm overhangs, taller formal roof, near-vertical sides
  (tumblehome 12 cm), square shoulders, taller nose.
- **Shaped lamps:** a local `face_poly` helper (convex outline extruded off the nose / tail
  face) gives the rampart an angular headlamp (black housing whose inner end drops toward the
  grille, projector, lens strip, running-light blade along the sloped lower edge, amber outer
  segment) and one split wedge tail lamp per side.
- **No maker cues (rule 9):** removed the vertical tri-bar tail lamps and tri-bar running
  lights (rampart), the body-colour split-grille divider and the hockey-stick stripe
  (ironside, now a straight body-side stripe). Badges are blank discs.
- Generator requests sent to the lead (see below).

## Common to the group
- **Higher belt** (`belt_dz` +0.05…+0.07): tall slab body sides under a short greenhouse, which
  is the muscle-car look. Before, the belt sat at ~0.78 m on 1.33 m roofs and the greenhouse was
  as tall as the body side, so the cars read as 80s sports cars.
- **Longer roofs, less raked glass**: the old ironside had a 0.62 m roof and a windshield +
  rear glass so long the cabin read as a fastback wedge; the roof is now ~1.07 m.
- **Every car has its own body knobs and kit.** The two sedans no longer reuse the coupe's
  style with a different paint; they share one kit (`sedan70_kit`) with an early / mid-decade
  variant.
- Local helpers in `muscle.rs` (built from kit primitives, no generator change): `hood_scoop`
  (forward-facing scoop with a dark mouth), `side_markers` (period amber front / red rear side
  markers, chrome bezels), `side_moulding`, `face_badge` (blank chrome disc, no lettering).
- 70s cars use 20 wheel segments (`SEG_70S`) instead of 24: tall-sidewall tyres still read round,
  and the saving pays for the chrome trim within the 22k hero budget.

## ironside_440 — early-70s big-block hardtop coupe
**Reference (generic):** an early-70s American intermediate two-door hardtop with a big-block V8:
about 5.2 × 1.95 × 1.33 m, 2.95 m wheelbase; long hood (~37 % of length), set-back cabin,
semi-fastback roof with wide sail panels into a short deck; no B-pillar (pillarless hardtop);
windshield ~57°; tall slab sides with a hip kick over the rear wheel; full-width grille with
quad round lamps behind a chrome blade; twin hood scoops; full-width tail lamps in a black
panel; chrome drip rails, vinyl roof, body-side stripe; 14–15" slotted mags on tall tyres.

**Changed:** belt +6 cm; windshield 59°→57°, rear glass 63°→61°, roof +5 cm longer (now ~1.07 m
vs 0.62 m); hardtop B-pillar (5 cm seal only); wider C-pillar sail (16 cm); stronger hips
(haunch 3.5→5 cm, small front haunch); arch gap 7→6 cm; the flat "brick" cowl scoop replaced by
twin forward-facing hood scoops on a power bulge; blank grille badge; chrome drip rail, chrome bullet mirrors; chrome-bezelled side markers; reverse lamps and
a plate recess moved into the rear blade (the centre lamps no longer read as a letter).

## rampart_50 — 2010s muscle coupe
**Reference (generic):** a 2010s American V8 muscle coupe: about 4.8 × 1.92 × 1.38 m, 2.72 m
wheelbase; long hood with a power dome and heat extractors, chopped-looking greenhouse on a high
belt, fastback roof into a short ducktail deck; wide rear hips; large trapezoid mesh grille with a
centre emblem; slim angular headlamps with LED running lights; splitter; fake scoop ahead of the rear
wheel; slim tail lamps in a black panel, diffuser and quad tips; 19–20" wheels on
low-profile tyres; over-the-top stripes.

**Changed:** belt +4 cm net (high belt, small side glass), roof arc 0.5 (smooth fastback),
windshield 63°→62°, roof −3 cm; sharper shoulder crease; rounder nose and tail plan, softer tail
(tail_round 0.18→0.24); arch gap 7→6 cm (4.5 cm hid the sport-tyre mod); black mirrors; grille frame now a trapezoid with a blank
badge; angular shaped headlamps and split wedge tail lamps (second pass); quarter scoops; blank tail
badge.

## bulwark_400 — early-70s full-size four-door muscle sedan
**Reference (generic):** an early-70s American full-size four-door with a high-output V8:
about 5.5 × 2.0 × 1.38 m, 3.07 m wheelbase; long flat hood and deck, pillared four-door
greenhouse; horizontal egg-crate grille spanning the face with quad round lamps at its ends in
one chrome surround; slim chrome blades; full-width tail lamps; bright belt and rocker
mouldings; hood-centre spear; full vinyl roof; turbine wheel covers.

**Changed:** own body (was the coupe's with the cabin moved): belt +5 cm, windshield 58°, rear
glass 57°, longer roof, 18 cm C-pillars, 9 cm black B-pillar, flatter stance; new kit:
full-width egg-crate grille with a chrome frame and quad round lamps, amber corner wrap
lenses, hood spear, two-row full-width tail lamps with a chrome bar and red outer signals,
plate recess, chrome belt moulding along doors and quarters, side markers, turbine wheels.

## brigand_400 — mid-70s full-size four-door sedan
**Reference (generic):** a mid-70s American full-size four-door "land yacht": about 5.55 ×
2.02 × 1.38 m, 3.07 m wheelbase; formal upright roof with wide C-pillars and a landau half-vinyl
top with a chrome band; upright chrome waterfall grille standing proud of the hood, stacked
square quad lamps, wrap-around amber corner lenses; stand-up hood ornament; big 5-mph bumpers
with rubber strips and guards; vertical wrap-around tail lamps at the corners with a ribbed
centre panel; rubber body-side moulding; dog-dish steel wheels.

**Changed:** own body: belt +5 cm, upright rear glass (55°→48°), 26 cm formal C-pillars,
body-colour B-pillar, level stance, square nose; new kit: waterfall grille + frame, stacked
square lamps in chrome surrounds, corner amber wraps, hood ornament, 5-mph bumpers front and
rear with rubber strips, guards and pads, tall corner tail lamps that wrap onto the quarters,
ribbed reflector panel, landau roof with chrome band, rubber side strip, chrome belt moulding.

## Triangle counts (hero budget 22k, mid 7k)
| car | full before | full now | mid before | mid now |
|---|---|---|---|---|
| ironside_440 | 19809 | 20318 | 5451 | 5822 |
| rampart_50 | 19954 | 20910 | 5380 | 5910 |
| bulwark_400 | 20579 | 20825 | 5621 | 6171 |
| brigand_400 | 20986 | 21363 | 6042 | 6375 |

## Checks
`cargo fmt`, `cargo clippy -p car-assets --all-targets -- -D warnings`, `cargo test -p
car-assets` (incl. the turn-signal contract on full and mid meshes), `validate-car-asset` (all
cars ok), `gen-car-mesh --check`. Mod coverage: see below.

`node e2e/mod_coverage.mjs <car>` on the final meshes (after `npm ci && npm run wasm`): each
car shows 134 of 136 mods (4 not for Muscle); the two misses are the known non-visual ones from
`mod-visuals.md` (`drivetrain_shifter_short` on all four, `elec_radar_1` on ironside, bulwark,
brigand). On rampart, `tires_sport` was hidden by a 4.5 cm arch gap; at 6 cm it shows again
(`MODS=tires_sport` re-run: 0 missing).

## Shots
`docs/media/car-shapes/muscle/{before,after}/<car>_{front,rear,side,top,cabin,cabin_rear}.jpg`;
per-car before (left) / after (right) sheets `docs/media/car-shapes/muscle/<car>_before_after.jpg`
(rows: front 3/4, rear 3/4, side, top) and all four side by side in
`docs/media/car-shapes/muscle/before_after.jpg`.

## Known gaps
- The rampart's nose cannot lean forward (shark nose): `nose_rake_deg` only leans back. It is
  upright with a rounder plan instead.
- The rear-arch flare on the rampart still shows a few spiky facets where the haunch meets the
  arch lip (generator loft; reduced by a smaller haunch / flare).
- No opera windows on the brigand's C-pillar: side patches can't follow the greenhouse surface.

## Generator requests (sent to the lead 2026-10-27)
1. Per-station side section: a waist pinch at the doors swelling over the arches in plan
   (coke-bottle line), e.g. `waist_pinch_m`.
2. Forward-leaning nose: negative `nose_rake_deg` (to about −15°) for modern shark noses.
3. Shaped lamp lens primitive: polygon outline extruded off the face that follows the
   plan-round corners, with an inner bezel inset.
4. Side patch that follows the greenhouse surface (opera windows, C-pillar badges).
5. Lower-body tumblehome (boat-tail section) as its own knob.

## Contract change requests
None.
