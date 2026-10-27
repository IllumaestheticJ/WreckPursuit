# car-shapes phase 2 + pass 3: hatch group

- **Workstream:** car-shapes phase 2, group `hatch` (brief: `docs/integration/car-shapes-phase2-brief.md`),
  then pass 3 on the phase-3 generator (`ws/car-shapes` d8e0a524, merged in).
- **Branch:** `ws/car-shapes-hatch` (from `origin/ws/car-shapes`)
- **Cars:** `vireo_xs`, `stoat_16v`, `marten_20`, `lindwurm_4t`
- **Files changed:** `tools/assets/src/styles/performance/hatch.rs` (style file, the only
  generator file touched), the four cars' meshes in `data/panels/meshes/` and
  `data/panels/meshes/mid/`, their rows in `TRIS_BEFORE_INTEGRATED_SIGNALS`
  (`tools/assets/tests/assets.rs`), shots in `docs/media/car-shapes/hatch/`.
- **Not changed:** shared generator code, `data/cars`, `data/lattices`, panel specs, sim crates.
- **Before / after:** `docs/media/car-shapes/hatch/hatch_before_after.jpg` (all four), and
  `<car>_before_after.jpg` per car (front 3/4, rear 3/4, side, top; "before" = the phase-1
  meshes, "after" = pass 3).

## What was wrong before
All four used one body recipe and (three of them) one kit: a long flat roof running back to an
almost vertical hatch with a third side window, so they read as small estates; slab sides and a
rectangular plan view; a tall slab nose with box lamps; the stoat and marten were recolours of
the vireo; a floating skirt plank; wheels small in big arch gaps.

## Shared changes (all four, render only)
- **Greenhouse:** short roof (`roof_len_trim` −0.10…−0.18) with a slight fall (`roof_arc`
  0.10…0.40) to a raked hatch; on the vireo, marten and lindwurm this also removes the quarter
  glass, giving the thick body-colour C-pillar of a hatch (`c_pillar_w_m` 0.20…0.28).
- **Section and plan (pass 3):** each car has its own plan-width curve (local `plan_taper`:
  full width between the wheels, easing to a nose / tail scale over the overhangs, optional door
  waist), side bulge, rocker tuck, `tumblehome_m` and fender rise; the lindwurm adds side
  sculpts. Values per car below.
- **Stance:** `arch_gap` 0.07 → 0.05 (wheels fill the arches), `overhang_trim` (−0.05, −0.02):
  the visual nose ends at the bumper node row (front overhang 0.82–0.90 m, was 0.87–0.95 m).
- **Lamps (pass 3):** head and tail lamps are projected clusters (`Kit::proj_cluster`), the turn
  signal cut out of the same lens; reverse lamps, DRLs and light bars are extra projected lenses
  (local `proj_lens`, on `crate::lamp::lens`). The local `face_poly` prism is kept only for the
  flat intake / duct inserts.
- **Local helpers in `hatch.rs` (no shared code):** blank badges, body-side strips split per
  panel (fender / front door / rear door / quarter, so each piece rides and detaches with its
  panel), centre high brake lamp under the roof spoiler, plate on a recess, rear reflectors, rear
  wiper.

## Per car

### vireo_xs
- **Reference:** late-90s Japanese front-drive hot hatch, about 4.2 m long, 1.7 m wide, 1.37 m
  tall, 2.6 m wheelbase, 15-inch wheels. Rounded body: soft curved sides, a strongly tapered
  rounded-wedge nose with teardrop lamps swept back along the fender tops, slim grille slot, big
  lower mouth, raked windshield (≈60°), short roof falling to a raked hatch over a short
  tailgate, small body-colour roof spoiler, tall corner tail clusters, a belt rising rearward.
- **Shape:** nose height 0.60 m, rake 32°; belt +5 cm rising 5 cm to the tail; plan taper to
  0.87 at the nose and 0.93 at the tail; side bulge 0.035 m peaking at 60 % of the side, rocker
  tuck 0.045 m, tumblehome 0.17 m, fender tops 1.5 cm over the hood; no skirt (the sill tucks
  under).
- **Detail:** teardrop head lamps projected onto the nose top with the amber signal at the outer
  tip and a black bezel; grille slot with badge; trapezoid lower mouth with two bars, round fogs,
  red accent line and lip. Rear: tall red-over-amber clusters with a clear reverse lens under
  them, cast round onto the quarters (yaw 25°), black hatch garnish with badge, high brake lamp,
  plate, reflectors, single tip.
- **Triangles:** 18,039 full (phase 1: 17,065), 4,699 mid. Hero budget 22k.

### stoat_16v
- **Reference:** early-90s European front-drive 16-valve hot hatch, about 4.05 m long, 1.7 m
  wide, 1.40 m tall, 2.48 m wheelbase, 15-inch wheels. Upright and boxy with crisp edges: a
  near-vertical nose with a full-width black band of square twin-lamp housings and a slatted
  grille, black bumpers wrapping round to the arches, black rubbing strips, an upright greenhouse
  with a very thick C-pillar, wide ribbed horizontal tail lamps, small black roof spoiler.
- **Shape:** kept the squarest on purpose: nose rake 10°, square plan corners, plan taper only
  to 0.96 / 0.98, small side bulge (0.012 m) high on the side, rocker tuck 0.02 m, tumblehome
  0.10 m, flat bonnet between fender tops 2 cm higher, 56° windshield, 0.28 m C-pillar. It keeps
  a small rear quarter window (typical for the type; sets it apart from the other 90s cars).
- **Detail:** black front band with square lamp lenses, twin round reflectors each, amber
  corner signals, three-slat grille with badge; black front and rear bumper bands with side wraps
  to the arches; rectangular fogs; black rubbing strips with a yellow insert line along the
  doors; tail: wide lenses cast round the corners (yaw 20°) with amber outer ends, black ribs and
  clear inner strips, either side of a black plate panel on the tailgate; black roof spoiler with
  high brake lamp; black mirror caps.
- **Triangles:** 18,312 full (phase 1: 16,894), 5,174 mid.

### marten_20
- **Reference:** early-90s European front-drive 2.0 hot hatch, about 4.05 m long, 1.7 m wide,
  1.40 m tall, 2.47 m wheelbase, 15-inch wheels. Wedge: a low, pointed nose (in plan too) with
  slim lamps that almost meet over a grille-less nose, a body-colour bumper with a wide intake,
  sides leaning in above a black moulding, a rising belt line, a full-width red tail band, twin
  tips.
- **Shape:** nose height 0.58 m, rake 36°, 61° windshield, belt rising 7 cm to the tail, roof
  arc 0.40; plan taper to 0.84 at the nose (the most pointed) and 0.92 at the tail; side bulge
  0.022 m low on the side (42 %), rocker tuck 0.035 m, tumblehome 0.19 m.
- **Detail:** slim wedge head lamps cast round onto the fenders (yaw 25°) with amber outer
  ends and a thin bezel, black slot and badge between them; wide trapezoid intake split by a
  body-colour bar with rectangular fogs at its ends, black chin splitter; black side mouldings
  and skirts; tail: one red band of lamps (amber inboard sections, small reverse lenses) cast
  round the corners (yaw 30°) and a red reflector panel with badge across the tailgate; black
  lower valance, twin tips, body-colour roof spoiler with high brake lamp; filler on the left.
- **Triangles:** 18,242 full (phase 1: 17,056), 4,614 mid.

### lindwurm_4t
- **Reference:** 2010s European all-wheel-drive hot hatch, 5-door, about 4.28 m long, 1.80 m
  wide, 1.44 m tall, 2.63 m wheelbase, 18-inch wheels. Wide and planted: fenders wider than the
  doors, a sharp shoulder line with the lower door drawn in under it, haunches over the rear
  wheels, a high belt and lower greenhouse, slim swept LED head lamps with a DRL brow and grille
  bars running toward them, a big honeycomb lower intake with side ducts, a large roof spoiler
  with end plates, horizontal two-piece LED tail lamps, diffuser and quad tips.
- **Shape:** belt +6 cm, 62° windshield with 10 cm wrap, short roof, 0.26 m C-pillar, box
  arches; plan taper to 0.90 / 0.93 with a 0.965 waist at the doors; side bulge 0.02 m high
  (70 %), rocker tuck 0.035 m, tumblehome 0.19 m; front / rear fender rise 2.5 / 1.5 cm; side
  sculpts: +2 cm shoulder line 8 cm under the belt the whole length, −2.5 cm lower doors between
  the arches, +2 cm rear haunch.
- **Detail:** slim grille with two bars and badge; swept five-point LED lamps cast round onto
  the fenders (yaw 30°) with amber outer ends and a bezel, DRL brow lens along their top edge;
  trapezoid honeycomb intake with angled side ducts whose blades carry the red accent; tail:
  lamps cast round the corners (yaw 30°) with amber inboard sections and a light-bar lens along
  the top, black hatch garnish with badge, diffuser, quad tips, end-plated roof spoiler with high
  brake lamp, plate, reflectors; silver mirror caps with repeaters (mirrors 10 cm further back on
  the door than in phase 2, see checks); split-spoke wheels.
- **Triangles:** 18,638 full (phase 1: 17,474), 4,959 mid.

## Checks run (pass 3)
`cargo fmt`, `cargo clippy -p car-assets --all-targets -- -D warnings`, `cargo test -p
car-assets` (36 passed), `validate-car-asset` (39 cars ok), `gen-car-mesh --check`; `git status`
after a full `gen-car-mesh` shows only the four cars' meshes changed.
- `phase3_knobs_keep_panels_on_their_regions` (extreme knobs on top of each car's style) first
  failed on lindwurm_4t: mesh width 2.432 m against the +35 % limit of 2.430 m, from the door
  mirrors following the stress-test shoulder out. Moving the mirrors 10 cm back on the door
  (`Kit::mirrors(0.20, …)`) gives 2.348 m; the stock car is unaffected.
- Mod coverage, phase 2 (full run, 136 mods): 1 of 136 shows nothing on each car,
  `drivetrain_shifter_short` (interior, on the known list in `mod-visuals.md`). Pass 3 spot
  check: see below.

## Notes for the lead
- **Door count:** `stoat_16v`'s spec sheet (and the old style comment for `vireo_xs`) says
  three-door, but all four lattices have detachable front and rear doors. Doors must keep
  detaching per lattice region, so the meshes are 5-door hatches. A 3-door look needs a lattice
  change (physics), out of scope here.
- Pre-existing, shared: the black arch liner leaves a gap at the top of the rear arches through
  which the rear spring shows from low side angles (shared `trim.rs` code, not changed here).

## Brand cues (rule 9)
Per the lead (2026-10-27), rule 9 covers maker-identifying cues too. The coloured grille
pinstripes on stoat_16v and lindwurm_4t (a well-known single-maker hot-hatch cue) were removed;
the accent moved to the stoat's rubbing strips and the lindwurm's side-duct blades. Badges are
blank shapes; lamp graphics are generic.

## Generator requests
Sent to the lead 2026-10-27. Done in phase 3: plan-width curve, section curvature (bulge /
rocker / tumblehome / sculpts), projected lamps that wrap the corners. Still open (phase 3b,
announced):
1. **Nose / tail face rake and bow:** the nose and tail faces are still flat vertical planes
   from sill to nose height. A raked / undercut bumper face would let the vireo and marten noses
   read as rounded and pointed in side view too.
2. **Hatch tail:** hatch glass runs from the roof to `y_deck`, then a vertical tail face. Wanted
   a tailgate lean and a kick / lip at the glass base (all four).
3. **Quarter glass:** an explicit knob to suppress or size the quarter window (now only reachable
   by shortening the roof).

## Pass 3b plan (touch-up when phase 3b lands)
- vireo / marten: face rake 15–25° with an undercut chin; marten nose height down to ~0.55 m.
- all four: tailgate lean ~10° below the glass with a 1–2 cm lip; stoat stays near-vertical.
- stoat: quarter-glass knob to size its small window explicitly (keep it); the others none.

## Contract change requests
None.
