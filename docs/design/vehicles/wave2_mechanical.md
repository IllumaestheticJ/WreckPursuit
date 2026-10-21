# Wave 2: mechanical (wheels, brakes, suspension, engines, exhaust)

Owner files: `tools/assets/src/mechanical/` (`mod.rs`, `spec.rs`, `prim.rs`, `brakes.rs`,
`suspension.rs`, `engine.rs`, `exhaust.rs`) and `tools/assets/src/wheels/` (`mod.rs`, `tire.rs`,
`rim.rs`). The public entry points are unchanged: `mechanical::parts`, `mechanical::brake_parts`,
`wheels::wheel_parts` and `wheels::WheelGeom::new`.

## How a car's mechanical look is resolved

1. `spec::table(id)` is a per-car table covering all 22 roster ids, including `fulmine_v10`,
   `sentinel_warden` and `sentinel_vigil`. Its values come from `research_mechanical.md` §1.3,
   §2.3, §3.3 and §4.4: rim design and size, lugs and PCD, finish, disc or drum sizes, caliper
   type and clock position, engine type and mount, cover finish, suspension per axle, frame
   rails, tinted or rusty springs, AWD, and exhaust layout. An unknown id falls back to its class
   (`spec::class_spec`).
2. **Style fields override the table.** A `MechStyle` or `WheelStyle` field counts as set when it
   differs from both `Default` and `class_default(def)`. `show_engine_detail` is ORed with the
   table. Some car-specific exhaust layouts survive a generic `ExhaustKind` from the same family:
   the lucciola centre quad and the fulmine high-centre quad both survive `Centre` or `Quad`.
   `Induction::Blower` on a V8 adds a roots blower.
3. The tier sets the density: hero is 24 tire segments and 20 rim/disc segments, traffic is 16
   and 13. The tier comes from `style_for(def).tier`.
4. `mechanical::parts`, which runs first, records the car in a thread-local
   (`spec::set_current`). `WheelGeom::new` only receives a `WheelDef`, so it looks the car up by
   hub position and radius. If the lookup fails, it falls back to a generic spec chosen by the
   tire id.

## What is drawn

- **Tire.** One lathe from bead to bead, using the profile family set by the tire id and aspect
  ratio (bulge, shoulder radius, tread width and crown). It has 2–3 V-groove circumferential
  grooves (hero only) and offset shoulder lugs for all-terrain tires. Traffic tires get the
  outboard lug row only.
- **Rim.** A lathed barrel with a flange, lip face, dish wall and inner barrel. The lip is
  polished on the mesh, slotted-mag and dog-dish rims. It has 14 designs: 5-spoke, deep-dish
  cross mesh, turbine (twisted blades plus a ring), 10-spoke, split-Y, 6-spoke, steel plus
  hubcap (one missing on `junkyard_beater` RL), steel with vent holes plus centre cap, dog-dish
  plus trim ring, slotted mag, monoblock (arms run to the flange), truck 6-window, OEM 7-spoke,
  and centre-lock (`CapKind::CentreLock`: hex nut plus pins, caliper tint). Hex lug nuts and
  domed caps face outboard.
- **Brakes (spin).** The disc annulus uses the `exhaust` slot, or `carbon` for carbon-ceramic.
  It has a dark top hat, a vent gap, drilled dots (3 rings) or curved slots, and sits inboard of
  the spokes' back face with room for the caliper. Drum brakes get finned drums plus backing
  plates. Closed-face wheels get a cheap disc.
- **Calipers (pivot).** Arc-swept sections by type: single or twin floating with an outboard
  bracket plate; fixed 2-, 4- and 6-piston; and an 8-piston monoblock with a dark pad window. They
  sit at the per-car clock angle: trailing 165° front and 205° rear, leading 20° on FF economy
  cars.
- **Pivot hardware.** Knuckle or carrier (tall for SLA and coilover), hub boss and steering arm.
  On struts, the strut lower body and spring seat move with the wheel, so the body-fixed rod and
  spring never gap. Live axles get an axle-end bell and half tube to the diff.
- **Suspension (body-fixed, rigid).** Strut, SLA coil-over-shock, SLA with coil between the arms,
  SLA with torsion bar, wishbone coilover, multilink, torsion beam, leaf live axle (under- or
  over-axle pack, hanger, shackle, U-bolts, staggered shocks) and coil live axle (trailing arms,
  coils, panhard). Also diff pumpkin or IRS diff box, half shafts, front anti-roll bar (hero),
  driveshaft, AWD front diff and shaft, and frame rails plus crossmember. Arms end about 4 cm
  short of the knuckle. All heights are capped under the hood or deck surface above each corner,
  and strut tops compress to fit.
- **Engines.** A big-block V8 with chrome air cleaner, small-block V8 (modern plenum and airbox,
  or old TBI cleaner), DOHC V8 (plenum, snorkel, coil packs), I4 NA and turbo (runners, headers,
  snail, FMIC and charge pipes), I6 (ITBs, twin turbo), twin-turbo V6 (cover), flat-plane V8
  (twin plenums, transverse box, side-scoop tubes), V10/V12 (carbon airbox or stacks, DCT, bay
  tub), and boxer, rotary or electric. Each engine is built in its own frame, rotated for its
  mount, then lowered and squashed in z until every vertex clears the hood or deck by 3 cm,
  keeping the sump at least 10 cm off the ground. Front engines also get a radiator, and a fan
  shroud on hero cars. Tuners get a front-mount intercooler. The detailed engine only appears
  when `engine_detail` is set; otherwise a cheap block of 130–400 tris is drawn.
- **Exhaust.** Runs go from the engine's exits (headers, logs or turbos) to under the floor, then
  to one or two mufflers. Hero turbo cars with long runs get a resonator. Tip styles: round,
  oval, trapezoid, turned-down, quad, dual-left, centre, centre quad, high centre, high corners
  with corner silencers, exit behind the right rear wheel, bare pipe before the right rear wheel,
  bare bent pipe, hidden, and side pipes. Tips are chrome, except on police cars and bare pipes.
  Mouths sit 2 cm proud of the tail.

**Materials.** `metal` is now used only by rims and hubcaps. Discs, links, shafts, intakes and
pipes use `exhaust`, so a gold or bronze `metal` tint only colours the wheels. Cam covers use the
caliper tint for the "tint" finish, `chrome`, `trim` (black), `exhaust` (silver) or `engine`
(dull). Coilovers and painted springs use the caliper tint.

## Triangle cost (2026-10-21, with the merged performance and police styles)

| | Hero (perf / police) | Traffic |
|---|---|---|
| Corner (tire + rim + brake + caliper + upright) | 1,260–1,720 (tire 624, rim 440–660, disc 40–240, caliper 10–110, upright 70–130) | 540–660 (tire 192, +108 all-terrain lugs) |
| Suspension + driveline + frame | 1,060–1,610 | 280–580 |
| Engine + radiator (+ FMIC) | 220–880 (detailed 600–880) | 130–330 |
| Exhaust | 230–700 | 70–140 |

Whole-car totals: hero cars are 15.7k–19.0k (ironside_440 at 18,971 is the highest, with a
10,983-tri styled body), and traffic cars are 6.9k–9.4k (hauler_1500 at 9,403 leaves about 600
for traffic details).

## Requests for the lead

1. **`rim` material slot** (renderer and `MaterialSlot`). Rims share `metal` with hubcaps only
   now, but a separate slot would let wheel tints such as gold or bronze sit beside a silver
   hubcap or bright hardware. Also wanted: a `spring` slot tinted by `MechStyle.spring_rgb`. That
   field is not written to the mesh today, so tinted springs reuse the caliper tint.
2. **Pass the car into the per-wheel calls.** `WheelGeom::new(index, &WheelDef, &WheelStyle)` and
   `brake_parts` cannot see `VehicleDef` or the tier, so `spec.rs` finds the car through a
   thread-local set by `mechanical::parts` (matched on hub position). Better:
   `WheelGeom::new(def, index, style, tier)`, or have `mesh.rs` pass the resolved
   `spec::CarSpec`.
3. **Underbody plate in the engine bay** (`mesh.rs::underbody`). The plate uses ring point 0,
   which is lifted onto the wheel-arch floor. Under the front arches it spans the whole width at
   arch height (z ≈ 0.66 on ironside_440), so it covers the engine when the hood is open or gone.
   It should stay at floor height between the arches, or leave out the engine-bay span. My
   engine-bay screenshots hide `underbody`.
4. **New `ExhaustKind` variants** to match `spec::Tips`: `CentreQuad`, `HighCentre`, `DualDown`
   (turned down), `SideExit` (behind the right rear wheel) and `Bare`. Until they exist, these are
   chosen per car id in the table and survive a style's `Centre`, `Quad`, `Dual` or `Single`.
5. **Optional `WheelStyle` fields:** `lip_polished: bool`, `missing_cap: Option<u8>` and
   `spoke_twist_deg`. `MechStyle`: `cover: CoverFinish` (tint / chrome / black / silver / dull),
   `caliper_clock_deg`, and `frame_rails: bool`. These are table-only today.
6. **Wave-2 style agents:** set `caliper_rgb` for red cam covers too (vireo, kestrel, shiro,
   hakone, lucciola, fulmine, ironside). The "tint" finish shares that colour. Traffic and police
   calipers stay dark when it is unset.
7. **Budget:** `hauler_1500` is at 9.4k of 10k. If traffic details push it over, drop its tire
   lugs (`TireKind::AllTerrain` → `Street`, −432) or lower `WheelStyle.segments` to 14.
8. **`tools/assets/README.md` "Known gaps"** still says the wheels and mechanical parts are
   placeholders. Please point it at this file. Calipers and uprights still do not get visual
   camber (renderer).
9. **Shared target-dir hazard** (reported earlier): `repo_root()` is
   `env!("CARGO_MANIFEST_DIR")`, so with one shared `CARGO_TARGET_DIR` a binary built in another
   worktree writes into that worktree's `data/`.
