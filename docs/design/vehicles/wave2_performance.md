# Wave 2 — style-performance

Owner of `tools/assets/src/styles/performance/` (it used to be `performance.rs`, now split into
`mod.rs` for dispatch and shared wheel setup, `kit.rs` for the body-kit geometry library, and
`muscle.rs`, `hatch.rs`, `tuner.rs`, `exotic.rs` for the cars). Spec:
`research_performance.md`. I regenerated only the 11 performance meshes.

## How it works
- Each car is a `CarStyle`:
  - research body knobs (`BodyStyle`);
  - one `Shape::Custom` "body kit" detail;
  - `WheelStyle` / `MechStyle` settings for the mechanical agent;
  - a paint tint (`tints.paint`).
- `kit::Kit::new` empties the foundation's generic `headlight_*`, `taillight_*` and `mirror_*`
  parts, using `Builder::set_geometry`. The kit then re-emits the car's own lamp and mirror shapes
  into the same keys, so part names, regions, kinds and materials stay the same.
- Helpers are positioned from the styled `BodyProfile`:
  - nose and tail faces;
  - `side_point` and `top_point` surfaces;
  - arches and roof edge.
- What the helpers make:
  - lamps: round, box or top-surface lamps, with bezels;
  - grilles and mesh grilles;
  - aero: splitter, canards, skirts, diffuser with fins;
  - wings: pedestal, swan-neck, hoop, roof spoiler, lip;
  - top and side vents, louvres, stripes and decals (matte `trim` sheets);
  - small parts: mirrors with caps, door handles, fuel filler, wipers, fog lamps, mud flaps.
- Sided parts use `_l` / `_r` names with mirrored regions.

## Per car (tris = whole mesh incl. today's placeholder wheels / mechanical)

All 11 cars fall between 9.6k and 12.4k triangles.

| Car | Tris | Tail-lamp signature | Key cues |
|---|---|---|---|
| ironside_440 | 12431 | 3-segment wide lamps per side in chrome bezels inside a full-width black panel, twin reverse lamps | long hood (cabin −0.35), nose-down rake, cowl scoop, hood pins, chrome bumper blades with wraps, full-width chrome grille with quad round lamps, vinyl roof, black side stripe + hip kick, chrome rocker, ducktail, plum |
| rampart_50 | 11690 | three vertical bars per side in a black panel | shark-nose mesh grille, lower mouth, carbon splitter, slim lamps + DRL, hood bulge + slatted heat vents, twin over-the-top stripes (hood / roof / deck), lip spoiler, carbon diffuser (4 fins), carbon skirts, red |
| vireo_xs | 9569 | tall vertical clusters (red / amber / clear) at the corners | slim grille slot, clear lamps with reflectors, big lower mouth, fogs, chin lip with red accent stripes (caliper slot), small roof spoiler, rear wiper, white |
| lindwurm_4t | 9770 | horizontal wrap-around lamps with a bright inner bar | honeycomb grille, blue accent bar through the lamps, slim LED lamps + DRL, 3-mouth intake with blades, big roof spoiler with end plates, diffuser, silver mirror caps, box flares, blue |
| kestrel_ts | 11138 | rect lamps with two round elements each | forward hood scoop, blistered box flares, tall pedestal wing with end plates, big fogs, roof vent scoop, mud flaps, tow hook, gold wheels (metal tint), rally blue |
| shiro_turbo | 10592 | chrome-housed clear clusters with two round red inserts | carbon hood with vents, carbon GT wing on swan necks + gurney, wide lip with fences + canards, bolt-on box flares, −0.045 drop, 3° rear camber, vinyl band + gold slash, carbon mirror caps, pearl white |
| hakone_gt | 10973 | quad round with chrome rings in a body-wide black panel + centre garnish | bubble fastback, hoop wing, triple-projector lamps, big framed mouth, brake ducts, fender vents, deep purple |
| sperber_32 | 11354 | L-shaped lamps wrapping onto the quarters | split chrome-framed grille, twin round lamps, power dome, fender gills, boot lip, carbon roof, diffuser, dark blue |
| debug_coupe | 9797 | full-width thin light bar with thick ends | slim lamps + DRL, mesh mouth, carbon splitter, hood vents, ducktail, carbon diffuser / skirts / mirror caps, gunmetal |
| lucciola_350 | 10190 | quad round in a black slatted panel; outer lamps amber-ringed | knife wedge, closed pop-up pods with seams, slatted nose intake, side intake with 5 strakes, 8-slat louvred deck, Kamm tail, red |
| fulmine_v10 | 10706 | thin C-shaped strokes + centre strip over a black mesh fascia | cab-forward (cabin +0.20), slim LED lamps with angular DRL strokes, 3-mouth black front, carbon splitter with fences, hood vents, giant side intakes with scallop wall + blade, carbon skirts, glass engine cover between louvre fields, haunch scoops, fixed carbon wing, 6-fin diffuser, lime |

Screenshots (front / rear 3/4) are `perf_<car>_{front,rear}.png` in the session scratchpad.

## Requests for the lead
1. **Foundation panic in `BodyProfile::styled`.** `clamp(min, max)` panics when min > max:
   - `r1.clamp(r0 + 0.25, v_fw - 0.10)` hits it when `rear_glass_rake_deg` is large (≥ 74° on
     fulmine, ≥ 80° on lucciola);
   - the `hood_length` clamp `(r0 + 0.6, y_bf - 0.30)` hits it on the mid-engine lattices.

   Clamp robustly, e.g. `x.max(lo).min(hi)`. I worked around it with rakes of 60–64° and no
   `hood_length`.
2. **True cab-forward lattices for `lucciola_350` / `fulmine_v10`.** `cabin_shift_m` is capped at
   +0.20, so the roof centre sits at about 52 % L instead of the 42–46 % in the research. A
   lattice with a forward cabin (or a larger shift cap) would finish the mid-engine look.
3. **Material slots.** I need these:
   - a per-car **rim tint** (or `rim` slot). Kestrel's gold wheels tint the whole `metal` slot,
     which also colours discs and hardware;
   - a **white rim** option (vireo should have white 5-spokes);
   - an **underglow / accent emissive** slot (e.g. cyan) for the tuner neon in research §1.3;
   - a free **accent paint** slot. Vireo's red stripe and lindwurm's blue bar borrow `caliper`,
     which is fine only because the calipers share the colour.
4. **Exhaust placement (mechanical).** `ExhaustKind` has no "centre quad" (lucciola) and no
   "high centre" exit (fulmine: 4 tips high in the mesh fascia). I set `Centre` / `Quad`.
   Shiro's single cannon and kestrel's 0.11 m tip are set with `exhaust_tip_r_m`.
5. **Strut placeholders poke through low hoods.** The mechanical placeholder struts go up to
   z ≈ 0.78 and show through the hood on ironside and the two exotics. The mechanical agent's
   real suspension should stay under `p.z_hood`.
6. **Tail-light colour in daylight.** `tailMat` `0x550000` reads almost black in daylight. A
   brighter base (≈ `0x8a0a0a`) would make every tail-lamp pattern readable from the chase cam.

Reported earlier and already fixed by the lead: the environment map (chrome looked black) and the
lamp materials (no `flatShading`).
