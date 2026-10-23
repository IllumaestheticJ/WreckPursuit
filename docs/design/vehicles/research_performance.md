# Research: performance-car archetypes (muscle / hot hatch / tuner / supercar / exotic)

Wave-1 research for `style-performance` (owner of `tools/assets/src/styles/performance.rs`).
Naming rule: archetypes only. No maker, model, badge or trademark names in anything derived
from this file. Games are cited only as references for how they *convey* a genre.

## 0. Conventions used in this document

- **Units**: m and degrees. Body size `[W, L, H]` matches `body_size_m` in `data/cars/*.json`.
- **Positions on the body**: `y%` = percent of body length **measured from the nose** (0 % = front
  bumper face, 100 % = rear bumper face). `z%` = percent of body height H from the ground.
  `x` = metres off the centreline (± means mirrored pair). Detail sizes are `l × w × h` (m),
  meaning along the car (y) × across (x) × up (z).
- **Glass rake** is measured **from vertical** (0° = upright glass, 90° = flat). Typical
  family-sedan windshield is ~58–62° from vertical; low-drag cars go past 62°; very
  large rakes (~65°+) are rare outside sports cars (SAE / patent sources below).
- **Knob names** below are *proposals* matching PLAN.md "Foundation contract" item 1. The
  foundation agent picks the real field names; map 1:1.

| Knob (proposed) | Unit | Meaning | Sane range | Default (today) |
|---|---|---|---|---|
| `silhouette` | enum | Notchback / Fastback / Hatch / Wagon / Wedge / … | – | Notchback |
| `nose_h` | m | height of the nose leading edge (bumper top / lamp line) | 0.45–0.80 | from lattice `z_bumper_top.0` |
| `nose_rake_deg` | ° from vertical | lean-back of the nose face | 0–60 | ~15 |
| `nose_plan_round` | m | plan-view corner rounding of the nose | 0.05–0.45 | 0.35 (`END_ROUND_M`) |
| `hood_crown` | m | centre rise of the hood over its edges | 0–0.05 | 0.015 |
| `hood_bulge_h`, `hood_bulge_w` | m, frac of hood width | raised power bulge | 0–0.08, 0.2–0.7 | 0, – |
| `hood_scoop` | enum + size | None / Bulge / Scoop (forward-open) / Cowl (rear-open) / Shaker / Vents | – | None |
| `windshield_rake_deg` | ° from vertical | windshield angle | 52–72 | ~58 (measured) |
| `a_pillar_rake_deg` | ° | plan-view inward lean of A-pillar (greenhouse taper) | 0–25 | 8 |
| `rear_glass_rake_deg` | ° from vertical | backlight / hatch glass | 20–85 | ~55 |
| `roof_len_trim` | m | lengthen (+) / shorten (−) flat roof vs lattice | −0.25…+0.15 | 0 |
| `roof_dz` | m | visual roof height offset (− = "chopped") | −0.06…+0.03 | 0 |
| `tumblehome` | m | per-side inset of roof edge vs belt edge | 0.10–0.30 | 0.12 |
| `crease_z`, `crease_depth` | frac of H, m | body-side character line | 0.45–0.70, 0–0.025 | –, 0 |
| `haunch` | m | outward swell of rear fender over rear arch | 0–0.06 | 0 |
| `flare`, `flare_shape` | m, Round/Box | arch lip outward offset | 0–0.08 | 0, Round |
| `ride_dz` | m | visual body drop (− = lower), wheels fixed | −0.05…+0.03 | 0 |
| `stance_pitch_deg` | ° | extra knob suggested: + = nose-down rake | −0.6…+0.8 | 0 |
| `ducktail_h` | m | tail lip rise at deck end (Kamm cut if negative) | −0.03…+0.06 | 0 |
| `overhang_trim` | (m, m) | visual front / rear overhang trim (− shortens) | −0.15…+0.10 | (0, 0) |
| `belt_kick` | m | rise of belt line under the C-pillar | 0–0.08 | 0 |

### 0.1 What the generator measures today (`tools/assets/src/profile.rs`)

`BodyProfile::measure` takes all heights / stations from the **lattice** (regions FrontBumper,
Hood, LeftDoorF, Roof, Trunk, RearGlass, RearLeftQuarter, Cabin) and wheel `(y, z, r+0.07)` arches
from the `VehicleDef`. Measured now (script run over `data/lattices/*.json`):

| car | hood (nose→WS base) %L | roof len %L | roof centre y% | deck %L | WS rake | rear glass | belt/H | greenhouse/H | floor z |
|---|---|---|---|---|---|---|---|---|---|
| ironside_440 | 30 | 16 | 56 | 22 | 58 | 53 | .58 | .42 | .15 |
| rampart_50 | 28 | 16 | 56 | 18 | 59 | 57 | .58 | .42 | .13 |
| vireo_xs | 27 | 45 | 67 | 6 | 52 | 24 | .58 | .42 | .14 |
| lindwurm_4t | 28 | 45 | 67 | 5 | 50 | 24 | .58 | .42 | .13 |
| kestrel_ts | 28 | 22 | 57 | 23 | 54 | 42 | .60 | .40 | .14 |
| shiro_turbo | 28 | 16 | 56 | 18 | 58 | 57 | .58 | .42 | .13 |
| hakone_gt | 29 | 16 | 56 | 17 | 58 | 58 | .58 | .42 | .13 |
| sperber_32 | 27 | 22 | 57 | 25 | 56 | 39 | .60 | .40 | .13 |
| debug_coupe | 29 | 16 | 56 | 22 | 58 | 53 | .58 | .42 | .15 |
| lucciola_350 | 28 | 16 | 56 | 17 | 60 | 59 | .58 | .42 | .11 |

**Key finding:** every lattice comes from one template (belt 0.58 H, greenhouse 0.42 H, hood 28 %,
roof centre 56 %). So genre has to come from `BodyStyle` + details, and two things the template
can't do need a decision:
1. **Mid-engine cab-forward** (lucciola_350, fulmine_v10): the roof centre should sit at 42–46 % L,
   the windshield base at 20–24 % L, and the rear deck should be a long, nearly flat engine
   cover (30–36 % L). The current lattice puts a front-engine coupe cabin on the exotic. Options:
   (a) the roster agent regenerates those two lattices with a forward cabin, or (b) the foundation
   adds a visual `cabin_shift` knob (−0.40…+0.20 m) that moves windshield base / roof / rear glass
   stations and lets hood/trunk skin weights follow. (a) is cleaner; (b) is fine at ±0.3 m.
2. **Muscle long hood**: target 38–42 % L vs today's 28–30 %. Needs `cabin_shift ≈ −0.30…−0.45 m` on
   ironside_440 and −0.25 m on rampart_50, or a lattice update.
3. The belt/greenhouse split (0.58/0.42) is right for tuners. Muscle and modern supercar want a
   higher belt (0.62–0.64) / thinner glass (0.36–0.38): use `roof_dz` −0.03…−0.05 plus belt raised
   0.03 m in the style (visual only; the door panel's top edge follows the loft).

### 0.2 Camera geometry (sets the minimum feature size)

Chase cam 6 m behind, 2 m up, aimed at ~0.6 m height: about 13° down. You see the **rear face,
deck, roof and rear 3/4 sides** of your own car, and mostly the **rear** of opponents. At
1080p with ~90° horizontal FOV, a 1.9 m wide car is ~300 px wide at 6 m and ~90 px at 20 m.
- Geometry under **~0.04 m** (≈6 px at 6 m) won't read. Put it in texture / material, or leave it out.
- At 20 m only the **silhouette, colour, tail-lamp pattern and wing** read.
- Rear cues get the bulk of the detail budget. Front cues matter in mirrors, replays,
  the garage and for oncoming police / traffic.

---

## 1. Genotypes

### 1.1 MUSCLE

**Silhouette.** Long hood, short deck, set-back cabin, a wide flat body slab. Early-70s
(ironside_440): long, low and flat-sided. Hard horizontal belt with a rising "hip" kick over
the rear wheel, a chrome bumper blade each end, a recessed full-width grille, and a semi-fastback or
notch roof. 2010s (rampart_50): the same proportions, shrunk and muscled up. High belt, chopped
greenhouse, fastback, shark-nose grille that leans *forward* at the top, big haunches, a short tail.
Every reference game (NFS, Forza Horizon, GTA, Midnight Club, Wreckfest) sells muscle with three things:
**hood length, a rear-up nose-down stance and a hood bulge/scoop**, plus fat rear tyres and loud
twin exhausts.

| Ratio | 70s big-block coupe | 2010s muscle coupe |
|---|---|---|
| L / W | 2.65–2.70 | 2.45–2.55 |
| H / L | 0.25–0.27 | 0.28–0.29 |
| Hood (nose → WS base) | 0.38–0.42 L | 0.36–0.40 L |
| Roof flat length | 0.15–0.18 L | 0.13–0.16 L |
| Roof centre (from nose) | 0.58–0.62 L | 0.58–0.61 L |
| Deck length (rear glass base → tail) | 0.20–0.24 L | 0.13–0.16 L |
| Windshield rake | 58–62° | 62–64° |
| Rear glass rake | 62–70° (semi-fastback) / 50–55° (notch) | 72–76° (fastback) |
| Belt height | 0.58–0.60 H | 0.62–0.64 H |
| Greenhouse (roof − belt) | 0.40–0.42 H | 0.36–0.38 H |
| Front / rear overhang | 0.19 / 0.24 L | 0.20 / 0.23 L |
| Wheel Ø (tyre) / H | 0.50–0.52 | 0.48–0.50 |
| Track / W | 0.76–0.80 (wheels tucked) | 0.83–0.86 |
| Ride height (floor) | 0.15–0.17 m, rear +0.03 | 0.12–0.13 m |

**Signature features.**
- 70s: power bulge or cowl scoop (rear-opening) or a shaker scoop through the hood. Twin hood pins.
  Chrome bumper blades. Recessed full-width grille with **quad round headlamps** (2 per side) or hidden
  lamps. Hip kick-up. Optional pedestal decklid spoiler or small ducktail. Wide horizontal tail lamps
  in a recessed panel. **Dual exhaust, one round tip per side** under the bumper. Side-exit pipes are
  an option. Vinyl roof, stripes (hockey-stick / bumblebee / twin over-the-top).
- 2010s: trapezoid upper grille plus lower mouth, slim headlamps with an LED signature, twin hood
  heat-extractor vents beside a raised centre bulge, front splitter, short side skirt, small
  lip/pedestal spoiler, **vertical segmented tail lamps or a full-width bar**, black diffuser panel,
  **quad tips (2 per side)**, contrast black roof, over-the-top twin stripes.

**Wheels.** 70s: 15″ rim on a tall tyre (rim ≈ 0.55 of tyre Ø), 5-slot/5-spoke chrome or a steel wheel with
a dog-dish cap, raised-white-letter tyre sidewall (texture), rears 1 size wider. 2010s: 20″ (rim ≈
0.76 of tyre Ø), 10-spoke or split 5-spoke, dark/graphite, staggered widths, large calipers.

**Colours.** Loud solid colours: plum `#5B1E5A`, lime `#7CC242`, orange `#E5631E`, orange-red
`#D9381E`, school-bus yellow `#F2B90F`, black `#111111`, white with black stripes. 2010s: bright red
`#C1121C`, vivid blue `#1F7AE0`, gunmetal `#3A3F45`, satin black stripes.

### 1.2 HOT HATCH

**Silhouette.** A short, tall 2-box: short hood, upright-ish windshield, long flat roof that
runs to a near-vertical hatch, tiny rear overhang, wheels pushed to the corners. The car
reads as a compact *made aggressive*: a lower stance, bigger wheels in barely flared arches, a roof
spoiler. A 90s FF hatch (vireo_xs) is boxy and light, with thin pillars, clear-lens lamps, a slim grille
slot and a big lower mouth. A 2010s AWD hatch (lindwurm_4t) is chunkier: honeycomb mesh grille,
slim LED lamps, a big roof spoiler with end plates, diffuser and quad tips. Forza Horizon and NFS use
the roof spoiler, the centre or twin exhaust and an accent colour (red stripe or lip) as the hot hatch tells.

| Ratio | late-90s FF 3-door | 2010s AWD 5-door |
|---|---|---|
| L / W | 2.45–2.50 | 2.35–2.40 |
| H / L | 0.32–0.33 | 0.33–0.34 |
| Hood (nose → WS base) | 0.24–0.27 L | 0.25–0.28 L |
| Roof flat length | 0.40–0.45 L | 0.40–0.44 L |
| Roof centre | 0.62–0.66 L | 0.62–0.66 L |
| Deck | 0–0.04 L (tailgate) | 0–0.04 L |
| Windshield rake | 56–59° | 59–62° |
| Hatch glass rake | 35–45° | 48–56° |
| Belt | 0.56–0.58 H | 0.59–0.61 H |
| Greenhouse | 0.40–0.43 H | 0.37–0.39 H |
| Front / rear overhang | 0.20 / 0.17 L | 0.21 / 0.17 L |
| Wheel Ø / H | 0.42–0.44 | 0.44–0.46 |
| Track / W | 0.86–0.88 | 0.84–0.86 |
| Ride height | 0.12–0.13 m (−0.02 vs base car) | 0.12–0.13 m |

**Signature features.** Roof spoiler at the top of the hatch (90s: small lip, 2010s: large with end
plates and a centre gap). Front chin lip / splitter. Lower mesh intake bigger than the upper
grille. Fog lamps in the bumper corners. Short side skirts. 90s: **one large round tip** offset to one side, or
a centre tip. 2010s: **centre twin tips** or **quad (2 per side) in a diffuser**. Accent-colour lip or
grille stripe (red on white/black). Mirror caps in contrast (silver / carbon). Wide-ish box-flare
lips only on rally-homologation variants.

**Wheels.** 90s: 15–16″, 5-spoke or 6-spoke, white or silver (rim ≈ 0.66 of tyre Ø). 2010s: 18–19″, 5-
twin-spoke or turbine-fan, graphite or matte silver, coloured calipers (rim ≈ 0.74).

**Colours.** 90s: competition white `#F4F3EE` (with a red accent stripe), bright yellow `#F4C80F`,
signal red `#C8102E`, black. 2010s: bright blue `#1E5BD6`, bright white `#F2F2F2`,
flat grey `#8A8D8F`, deep blue `#253E8F`, matte black trim.

### 1.3 TUNER

**Silhouette.** A mid-size Japanese or European coupe or sedan: front engine, moderate hood (0.28–0.33 L),
upright greenhouse with thin pillars, short deck. Genre comes from **modification**, not factory
proportions. Lowered (−0.03 to −0.05 m), wheels flush with or poking out of the arches, negative-camber look, an aftermarket
**wing on tall stands**, wide front lip, side skirts, a big single "cannon" exhaust, a vented or
carbon hood, stickers/vinyl. This is the NFS Underground / Midnight Club / Forza Horizon tuner
grammar: body kit, wing, wheels, neon, vinyl, with every upgrade visible.
Sub-flavours on the roster: rally sedan (kestrel_ts: hood scoop, tall wing, fender bulges, gold
wheels), FR drift coupe (shiro_turbo: wide lip, GT wing, deep-dish wheels), 90s twin-turbo GT
(hakone_gt: bubbly fastback, hoop wing, round tails), German sport sedan (sperber_32: crisp
notchback, power dome, quad tips, fender gills).

| Ratio | 2000s FR coupe | 90s FR GT | 2000s sport sedan |
|---|---|---|---|
| L / W | 2.60–2.65 | 2.45–2.55 | 2.50–2.60 |
| H / L | 0.29 | 0.28 | 0.30–0.32 |
| Hood | 0.30–0.33 L | 0.32–0.35 L | 0.29–0.32 L |
| Roof flat | 0.15–0.18 L | 0.12–0.15 L | 0.20–0.24 L |
| Roof centre | 0.53–0.57 L | 0.55–0.58 L | 0.52–0.56 L |
| Deck | 0.17–0.20 L | 0.10–0.14 L (liftback) | 0.20–0.24 L |
| Windshield rake | 59–62° | 62–65° | 58–61° |
| Rear glass rake | 64–70° | 72–78° | 55–62° |
| Belt | 0.58–0.60 H | 0.58–0.60 H | 0.60–0.62 H |
| Greenhouse | 0.40–0.42 H | 0.38–0.40 H | 0.38–0.40 H |
| Overhang F / R | 0.20 / 0.23 L | 0.21 / 0.22 L | 0.19 / 0.21 L |
| Wheel Ø / H | 0.47–0.50 | 0.48–0.50 | 0.44–0.47 |
| Track / W | 0.86–0.88 (flush) | 0.84–0.86 | 0.85–0.86 |
| Ride height | 0.10–0.11 m | 0.11–0.12 m | 0.12 m |

**Signature features.** Big rear wing: 2000s-tuner GT wing at 0.20–0.28 m above the deck, swan-neck or
plate stands. 90s GT hoop wing spanning the deck. Rally-sedan pedestal wing at 0.15 m. Wide front lip
/ splitter 0.04–0.06 m below the bumper, side skirts, tow hook (front, angled), canards
optional. Hood vents or a top-mount scoop. Box or bolt-on flares on drift builds. Fender gills
(sedan). Tails: quad round (90s GT), clear-lens chrome-housing clusters (2000s aftermarket),
slim horizontal (stock). Exhaust: **single large tip** 0.10–0.12 m (tuner), quad (German sedan).
Underglow neon (emissive strip under the sills).

**Wheels.** 17–18″ (rim ≈ 0.70–0.76 of tyre Ø), 6-spoke deep-dish, mesh, or a bronze/gold rally
12-spoke. Visual camber 1.5–3° on the rears. Bright calipers.

**Colours.** Pearl white `#EEEDE6`, deep purple `#3B2350`, rally blue `#1D3F8C` + gold wheels
`#B8963E`, silver `#B7BABE`, black `#121212`, orange `#F26A1B`, lime `#9BD32B`, two-tone vinyl
(livery hint). German sedan: dark metallic blue `#24407A`, silver-grey metallic `#8E9399`,
carbon roof.

### 1.4 SUPERCAR (modern, 2010s+, mid-engine)

**Silhouette.** A very low, wide **wedge-teardrop**. Short pointed nose, **cab-forward** canopy
(windshield base over the front axle), a long, almost flat engine deck behind the cabin. Huge side
intakes ahead of the rear wheels, a vertical-ish tail cut with a big diffuser. Width dominates: from the
chase cam it's a flat black-and-paint slab wider than it is tall by about 1.65×. NFS, Forza Horizon, GTA
and The Crew all sell it with **slim LED light signatures, giant side intakes, diffuser plus wing,
and big wheels**.

| Ratio | 2010s mid-engine V10 AWD |
|---|---|
| L / W | 2.28–2.35 |
| H / L | 0.26 |
| Nose → WS base | 0.20–0.24 L (WS base 0.05–0.15 m behind front axle) |
| Roof flat | 0.12–0.15 L |
| Roof centre | 0.42–0.46 L |
| Engine deck (rear glass base → tail) | 0.30–0.36 L |
| Windshield rake | 66–70° |
| Rear glass / engine cover | 78–84° (nearly flat) |
| Belt | 0.52–0.56 H (but hips at 0.62–0.66 H over rear wheel) |
| Greenhouse | 0.42–0.46 H, strong tumblehome (canopy) |
| Overhang F / R | 0.22 / 0.21 L |
| Wheel Ø / H | 0.56–0.60 |
| Track / W | 0.85–0.88 |
| Ride height | 0.10–0.11 m |

**Signature features.** Front splitter, 3-mouth front (big outer ducts + centre slot) in black.
Slim angular LED headlamps (single line DRL). Bonnet with two shallow vents. Big side intakes with
a strake. Rear haunch swell 0.05–0.06. Engine cover with louvres or a glass panel. Fixed or active
rear wing (or a pop-up lip). Full-width or Y/C-shaped thin **light bar** tail lamps. Large
multi-fin diffuser. **2 or 4 large exhaust tips set high in the rear centre** (hex or round), black
mesh rear fascia. Side skirts/blades in carbon.

**Wheels.** 20″ front / 20–21″ rear (rim ≈ 0.76–0.78 of tyre Ø), forged 5- or 10-spoke, centre-lock
look, carbon-ceramic size discs (0.38 m Ø), caliper colour (yellow/orange/red/black).

**Colours.** Lime `#8DC63F`, bright orange `#F57E20`, yellow `#F6C700`, matte grey
`#5C6066`, pearl white `#F1F0EC`, satin black with carbon. Black lower body (sills/splitter/diffuser)
always.

### 1.5 EXOTIC (classic, 70s–90s mid-engine wedge)

**Silhouette.** A pure **doorstop wedge**: knife nose 0.50–0.58 m high, an unbroken straight line from
nose to windshield, flat cab-forward canopy, rear deck covered in louvres, chopped Kamm tail.
Very wide rear track, wide rear tyres. Flatter, more angular and more "drawn with a ruler" than the
modern supercar. Exotic reads as **rare and dramatic** more than efficient. Pop-up headlamps, side
strakes over intakes, quad round tails, and a big wing as an option (Burnout / Midnight Club / NFS
classic-exotic tier).

| Ratio | 90s mid-engine wedge |
|---|---|
| L / W | 2.20–2.30 |
| H / L | 0.27–0.28 |
| Nose → WS base | 0.22–0.26 L |
| Roof flat | 0.13–0.16 L |
| Roof centre | 0.43–0.47 L |
| Engine deck | 0.30–0.34 L |
| Windshield rake | 64–68° (in line with hood plane ±3°) |
| Rear glass / buttress | 75–82° (flat louvred deck) |
| Belt | 0.54–0.57 H |
| Greenhouse | 0.42–0.45 H |
| Overhang F / R | 0.20 / 0.22 L |
| Wheel Ø / H | 0.52–0.56 (rear tyre bigger) |
| Track / W | front 0.79–0.81, rear 0.85–0.87 |
| Ride height | 0.10–0.11 m |

**Signature features.** Pop-up or flush rectangular lamps. Black slatted nose intake plus small
side ducts. Door strakes over the side intake. Rear-deck louvres (transverse, 6–10 slats). Flying
buttresses optional. Black rear grille / mesh between the lamps. **Quad round tail lamps** or a full-width
black panel with rectangular lamps. 2 or 4 tips centre-rear. Optional big wing on high stands.

**Wheels.** 17–18″ (rim ≈ 0.70 of tyre Ø), 5-spoke star or "telephone-dial", silver; front 0.23 m
wide, rear 0.27 m wide.

**Colours.** Racing red `#C4161C`, yellow `#F4C300`, black `#0E0E0E`, white `#F4F4F0`, metallic
silver `#C9CBCE`, tan/cream interior hint.

---

## 2. Per-car recommendations

Current `[W, L, H]`, wheelbase and wheel radius come from `data/cars/*.json`. **fulmine_v10 (new):**
suggested def for the roster agent: `body_size_m [1.95, 4.46, 1.17]`, wheelbase 2.65, front
wheels r 0.330 / w 0.245 at x ±0.835, rear r 0.345 / w 0.305 at x ±0.820. Analog text:
"2010s mid-engine supercar, 5.2 L naturally aspirated V10, permanent AWD, dual-clutch".

### 2.1 Body knobs. Muscle + hot hatch

| Knob | ironside_440 | rampart_50 | vireo_xs | lindwurm_4t |
|---|---|---|---|---|
| size W/L/H | 1.95/5.20/1.35 | 1.92/4.78/1.38 | 1.695/4.18/1.37 | 1.80/4.28/1.44 |
| silhouette | Notchback (semi-fast) | Fastback | Hatch | Hatch |
| cabin_shift (see 0.1) | −0.40 | −0.25 | 0 | 0 |
| nose_h | 0.72 | 0.70 | 0.66 | 0.70 |
| nose_rake_deg | −5 (leans forward) | −8 (shark nose) | 25 | 20 |
| nose_plan_round | 0.08 | 0.22 | 0.30 | 0.25 |
| hood_crown | 0.010 | 0.020 | 0.015 | 0.020 |
| hood_bulge_h / w | 0.045 / 0.40 | 0.030 / 0.30 | 0 | 0.010 / 0.50 (twin creases) |
| hood_scoop | Cowl (rear-open) | Vents ×2 | None | None |
| windshield_rake_deg | 60 | 63 | 57 | 60 |
| a_pillar_rake_deg | 6 | 10 | 8 | 10 |
| rear_glass_rake_deg | 66 | 74 | 40 | 52 |
| roof_len_trim | +0.05 | −0.10 | +0.05 | 0 |
| roof_dz | −0.02 | −0.05 | 0 | −0.02 |
| tumblehome | 0.16 | 0.20 | 0.12 | 0.15 |
| crease_z / depth | 0.56 / 0.012 | 0.60 / 0.015 | 0.50 / 0.006 | 0.62 / 0.012 |
| haunch | 0.035 | 0.050 | 0.010 | 0.020 |
| flare / shape | 0.010 Round | 0.020 Round | 0.010 Round | 0.015 Box |
| ride_dz | 0 | −0.01 | −0.02 | −0.015 |
| stance_pitch_deg | +0.6 (rear up) | +0.2 | 0 | 0 |
| ducktail_h | 0.030 | 0.025 | 0 | 0 |
| overhang_trim F / R | 0 / 0 | 0 / −0.03 | 0 / 0 | 0 / 0 |
| belt_kick | 0.06 (hip) | 0.04 | 0 | 0.02 |
| wheel (rim″, style) | 15″ 5-slot chrome, RWL tyre | 20″ split 5-spoke graphite | 15″ 5-spoke white | 19″ 5-twin graphite |
| rim Ø / tyre Ø | 0.55 | 0.76 | 0.66 | 0.75 |
| caliper | none visible (drum look) | red | none/silver | blue |
| paint | plum `#5B1E5A` | bright red `#C1121C` | white `#F4F3EE` | blue `#1E5BD6` |

### 2.2 Body knobs. Tuners + sport coupe

| Knob | kestrel_ts | shiro_turbo | hakone_gt | sperber_32 | debug_coupe |
|---|---|---|---|---|---|
| size W/L/H | 1.77/4.60/1.45 | 1.695/4.445/1.285 | 1.81/4.515/1.275 | 1.78/4.49/1.37 | 1.85/4.50/1.32 |
| silhouette | Notchback 4-door | Notchback 2-door | Fastback (liftback) | Notchback 4-door | Fastback |
| cabin_shift | 0 | −0.08 | −0.12 | −0.10 | −0.10 |
| nose_h | 0.68 | 0.62 | 0.62 | 0.68 | 0.64 |
| nose_rake_deg | 20 | 28 | 35 | 12 | 25 |
| nose_plan_round | 0.25 | 0.30 | 0.40 | 0.18 | 0.30 |
| hood_crown | 0.015 | 0.015 | 0.025 | 0.015 | 0.020 |
| hood_bulge_h / w | 0.020 / 0.45 | 0 | 0.015 / 0.60 | 0.020 / 0.35 (dome) | 0.015 / 0.40 |
| hood_scoop | Scoop (fwd-open) | Vents ×2 (carbon) | None | None | Vents ×2 |
| windshield_rake_deg | 59 | 61 | 63 | 60 | 63 |
| a_pillar_rake_deg | 8 | 9 | 12 | 8 | 10 |
| rear_glass_rake_deg | 58 | 66 | 76 | 60 | 72 |
| roof_len_trim | 0 | 0 | −0.08 | 0 | −0.05 |
| roof_dz | 0 | 0 | 0 | −0.01 | −0.02 |
| tumblehome | 0.13 | 0.14 | 0.20 | 0.15 | 0.18 |
| crease_z / depth | 0.60 / 0.010 | 0.58 / 0.010 | 0 / 0 (smooth) | 0.62 / 0.015 | 0.60 / 0.012 |
| haunch | 0.015 | 0.020 | 0.045 | 0.020 | 0.035 |
| flare / shape | 0.030 Box (front blistered) | 0.040 Box (bolt-on) | 0.015 Round | 0.015 Round | 0.020 Round |
| ride_dz | −0.02 | −0.045 | −0.02 | −0.015 | −0.01 |
| stance_pitch_deg | 0 | +0.2 | 0 | 0 | 0 |
| ducktail_h | 0.015 | 0.010 | 0 | 0.020 | 0.035 |
| overhang_trim F / R | 0 / 0 | +0.04 / 0 (lip) | 0 / 0 | 0 / 0 | 0 / 0 |
| belt_kick | 0.01 | 0.02 | 0.03 | 0.03 (C-pillar kink) | 0.03 |
| wheel | 17″ 12-spoke gold | 18″ 6-spoke deep-dish bronze | 17″ 5-spoke silver | 19″ double-spoke silver | 19″ 5-spoke black |
| rim Ø / tyre Ø | 0.70 | 0.75 | 0.70 | 0.76 | 0.74 |
| visual camber (rear) | 1.0° | 3.0° | 1.0° | 0.5° | 0.5° |
| caliper | red | gold | red | grey | red |
| paint | rally blue `#1D3F8C` | pearl white `#EEEDE6` | deep purple `#3B2350` | dark metallic blue `#24407A` | gunmetal `#3A3F45` |

### 2.3 Body knobs. Exotic + supercar

| Knob | lucciola_350 | fulmine_v10 (new) |
|---|---|---|
| size W/L/H | 1.90/4.25/1.17 | 1.95/4.46/1.17 |
| silhouette | Wedge | Wedge |
| cabin_shift (see 0.1) | −0.50 (roof centre to 45 % L) | −0.55 (roof centre to 44 % L) |
| nose_h | 0.55 | 0.58 |
| nose_rake_deg | 55 (knife edge) | 45 |
| nose_plan_round | 0.15 | 0.20 |
| hood_crown | 0.010 | 0.020 |
| hood_bulge_h / w | 0 | 0 |
| hood_scoop | Vents ×2 (small) | Vents ×2 |
| windshield_rake_deg | 66 | 68 |
| a_pillar_rake_deg | 14 | 18 |
| rear_glass_rake_deg | 80 (louvred deck) | 82 (glass engine cover) |
| roof_len_trim | −0.05 | −0.08 |
| roof_dz | 0 | 0 |
| tumblehome | 0.24 | 0.28 |
| crease_z / depth | 0.50 / 0.020 (wedge line) | 0.45 / 0.025 (scallop into intake) |
| haunch | 0.040 | 0.060 |
| flare / shape | 0.015 Box (angular) | 0.020 Round |
| ride_dz | −0.01 | −0.01 |
| stance_pitch_deg | +0.3 | +0.2 |
| ducktail_h | −0.02 (Kamm) | 0.030 (active lip at rest) |
| overhang_trim F / R | 0 / −0.03 | 0 / 0 |
| belt_kick | 0.06 (rising to rear hips) | 0.08 |
| wheel | 17″ 5-spoke star silver | 20″ 10-spoke forged, centre-lock |
| rim Ø / tyre Ø | 0.70 | 0.77 |
| caliper | red | yellow |
| paint | red `#C4161C` | lime `#8DC63F` |

### 2.4 Detail lists

Format: **detail**: `kind` @ y% / z% / x m, size l × w × h (m), material, region.
Regions follow `BodyRegion` names. "pair" = mirrored ±x.

**ironside_440** (L 5.20, H 1.35)
- Cowl-induction bulge: `surface_patch` @ y 8–34 % / on hood / x 0, 1.35 × 0.78 × 0.045, rear edge open
  0.55 × 0.035 slot with `grille`, paint, Hood.
- Hood pins pair: `cylinder` @ y 3 % / hood / ±0.55, Ø0.035 × 0.012, chrome, Hood.
- Front bumper blade: `extrude` @ y 0–2 % / z 30–37 % / full width, 0.10 × 1.95 × 0.09, chrome, FrontBumper.
- Recessed grille: `surface_patch` (inset −0.04) @ y 0–1 % / z 40–55 %, – × 1.55 × 0.20, grille, FrontBumper.
- Quad headlamps: 2 `cylinder` per side @ y 0.5 % / z 47 % / ±0.48 and ±0.70, Ø0.165 × 0.03,
  `lightbar_clear` + chrome bezel ring `torus` r 0.09, FrontBumper.
- Rear bumper blade: `extrude` @ y 98–100 % / z 28–36 %, 0.10 × 1.95 × 0.09, chrome, RearBumper.
- Tail lamps pair: `beveled_box` inset in a 0.03-deep black recessed panel @ y 99 % / z 44–53 %, inboard edge x ±0.18,
  0.03 × 0.62 × 0.11, emissive red, RearBumper/Trunk.
- Ducktail lip: `surface_patch` @ y 95–98 % / deck edge, 0.12 × 1.70 × 0.03, paint, Trunk.
- Dual exhaust pair: `cylinder` @ y 100 % (+0.04 protrude) / z 18 % / ±0.55, Ø0.075 × 0.15, chrome, RearBumper.
- Rocker chrome strip pair: `box` @ y 30–68 % / z 33 %, 1.95 × 0.01 × 0.035, chrome, doors/sills.
- Vinyl top + "hockey-stick" stripe: livery hint (roof matte black `#141414`; stripe y 20–80 % at crease).

**rampart_50** (L 4.78, H 1.38)
- Shark-nose upper grille: `surface_patch` inset −0.03 @ y 0–2 % / z 42–56 %, – × 1.05 × 0.19 trapezoid, grille, FrontBumper.
- Lower mouth + splitter: grille patch @ z 16–26 %, 1.30 wide; splitter `box` @ y 0–4 % / z 9 %, 0.18 × 1.75 × 0.025, carbon.
- Slim headlamps pair: `beveled_box` @ y 1–5 % / z 52 % / ±0.62, 0.20 × 0.40 × 0.07, clear + DRL strip
  emissive (0.30 × 0.015), FrontBumper.
- Hood vents pair: `surface_patch` inset @ y 14–22 % / hood / ±0.33, 0.30 × 0.16 × −0.015, grille, Hood.
- Centre bulge: covered by hood_bulge knob.
- Side skirt pair: `extrude` @ y 28–72 % / z 11 %, 2.1 × 0.04 × 0.06, carbon/black, sills.
- Rear diffuser: `surface_patch` @ y 97–100 % / z 10–22 %, 0.20 × 1.30 × 0.14 with 4 fins (0.12 × 0.01 × 0.10), carbon, RearBumper.
- Quad tips: 2 `cylinder` per side @ y 100 % / z 15 % / ±0.48, ±0.60, Ø0.10 × 0.12, exhaust/chrome.
- Tail lamps pair: 3 vertical `beveled_box` bars each @ y 99 % / z 54–64 % / x ±0.42 to ±0.66 step 0.12,
  0.03 × 0.07 × 0.14, emissive red, set in black panel 0.02 × 1.55 × 0.16.
- Decklid lip spoiler: `extrude` @ y 95–98 % / deck, 0.10 × 1.40 × 0.035, paint, Trunk.
- Twin over-the-top stripes: livery hint, 2 × 0.25 m at x ±0.20, black `#151515`.

**vireo_xs** (L 4.18, H 1.37)
- Slim grille slot: grille patch @ y 0–1 % / z 50–55 %, – × 0.70 × 0.07, FrontBumper.
- Big lower intake: grille patch @ y 0 % / z 18–32 %, – × 1.10 × 0.19, FrontBumper.
- Chin lip: `extrude` @ y 0–3 % / z 10 %, 0.08 × 1.50 × 0.025, black.
- Headlamps pair (clear, elongated): `beveled_box` @ y 1–6 % / z 55 % / ±0.55, 0.22 × 0.42 × 0.10, clear.
- Fog lamps pair: `cylinder` @ y 0 % / z 25 % / ±0.62, Ø0.08, clear.
- Roof spoiler (small): `extrude` at roof rear edge @ y 84–88 % / z 98 %, 0.18 × 1.20 × 0.035, paint, Trunk (tailgate).
- Side skirt pair: `extrude` @ y 28–70 % / z 11 %, 1.75 × 0.025 × 0.05, paint.
- Single exhaust: `cylinder` @ y 100 % / z 15 % / x −0.50 (left), Ø0.095 × 0.12, chrome.
- Tail lamps pair: `beveled_box` vertical clusters @ y 99 % / z 55–75 % / ±0.68, 0.04 × 0.20 × 0.28, red + amber/clear split.
- Red accent stripe on front lip/grille: livery hint `#C8102E`, 0.02 m (texture).

**lindwurm_4t** (L 4.28, H 1.44)
- Honeycomb upper grille + horizontal accent bar through headlamps: grille patch @ z 52–58 %, 1.05 wide;
  bar `box` 0.01 × 1.55 × 0.012 accent blue `#1E5BD6` (texture if <0.04).
- Large lower 3-mouth intake: grille patch @ z 14–34 %, 1.40 wide, with 2 vertical blades.
- Slim LED headlamps pair: `beveled_box` @ y 1–6 % / z 57 % / ±0.60, 0.22 × 0.45 × 0.07, clear + DRL emissive.
- Splitter: `box` @ y 0–3 % / z 8 %, 0.10 × 1.60 × 0.02, black.
- Big roof spoiler with end plates: `extrude` @ y 83–90 % / z 99 %, 0.28 × 1.30 × 0.04 + 2 plates 0.25 × 0.015 × 0.08, paint/black, Trunk.
- Mirror caps pair: `box` @ y 35 % / z 70 % / ±0.98, 0.12 × 0.18 × 0.10, matte silver `#9EA2A6`.
- Diffuser: `surface_patch` @ y 97–100 % / z 9–20 %, 0.15 × 1.20 × 0.15, black, 3 fins.
- Quad tips: 2 per side @ y 100 % / z 14 % / ±0.52, ±0.62, Ø0.090 × 0.10, exhaust (polished).
- Tail lamps pair: horizontal `beveled_box` @ y 98 % / z 63–70 % / ±0.55, 0.08 × 0.48 × 0.10, red with internal bar.
- Side skirts pair: `extrude` @ y 28–72 % / z 10 %, 1.85 × 0.035 × 0.06, black.

**kestrel_ts** (L 4.60, H 1.45)
- Top-mount intercooler scoop: `surface_patch` @ y 14–24 % / hood / x 0, 0.45 × 0.48 × 0.07, forward
  mouth 0.40 × 0.05 grille, paint, Hood.
- Blistered front fenders: covered by flare 0.03 Box (front arches only; rear 0.015).
- Tall pedestal wing: `extrude` aerofoil @ y 93–98 % / deck + 0.15, 0.22 × 1.40 × 0.035 on 2
  pedestals (0.10 × 0.03 × 0.15) at ±0.50, paint, Trunk.
- Fog lamp pair: `cylinder` @ y 0 % / z 25 % / ±0.62, Ø0.11, clear.
- Front lip: `extrude` @ y 0–3 % / z 10 %, 0.06 × 1.55 × 0.03, black.
- Lower grille: grille patch @ z 16–30 %, 1.20 wide.
- Single large tip: `cylinder` @ y 100 % / z 16 % / x +0.50, Ø0.11 × 0.12, exhaust (blued).
- Tail lamps pair: `beveled_box` @ y 99 % / z 60–68 % / ±0.58, 0.06 × 0.40 × 0.11, red, with round inner element (texture).
- Mud flaps optional (rally): `box` behind each wheel @ z 3–20 %, 0.005 × 0.24 × 0.25, black.

**shiro_turbo** (L 4.445, H 1.285)
- GT wing: `extrude` aerofoil chord 0.28 @ y 93–99 % / deck + 0.24, 0.28 × 1.50 × 0.03, carbon; end plates
  0.30 × 0.008 × 0.15; swan-neck stands 2 × (0.18 × 0.012 × 0.26) at ±0.45, Trunk.
- Wide front lip: `extrude` @ y −1–3 % / z 7 %, 0.10 × 1.65 × 0.03, black (adds overhang_trim +0.04).
- Carbon vented hood: hood material carbon `#1A1A1A` + 2 vent patches @ y 18–26 % / ±0.28, 0.25 × 0.18.
- Bolt-on flares: flare 0.04 Box, rivets as texture.
- Side skirts pair: `extrude` @ y 28–70 % / z 9 %, 1.85 × 0.04 × 0.07, paint.
- Tow hook: `box` @ y 0 % / z 22 % / x +0.45, 0.05 × 0.03 × 0.06, `caliper` red (angled 15°).
- Single cannon exhaust: `cylinder` @ y 100 % (+0.06) / z 14 % / x +0.48, Ø0.12 × 0.20, exhaust (burnt titanium).
- Clear aftermarket tail clusters pair: `beveled_box` @ y 99 % / z 55–64 % / ±0.55, 0.04 × 0.42 × 0.12,
  chrome housing + 2 round red inserts Ø0.08.
- Underglow: emissive strip under sills `box` @ y 25–75 % / z 7 %, 2.2 × 0.02 × 0.01, emissive (cyan
  `#29E0FF` default; toggled by renderer at night).
- Livery hint: vinyl side graphic band z 20–45 %, sticker squares on rear quarter glass.

**hakone_gt** (L 4.515, H 1.275)
- Hoop wing: `extrude` @ y 92–96 % / deck + 0.12 spanning between deck corners, chord 0.18 × 1.62,
  legs integrated into the rear quarters (2 × 0.12 × 0.10 × 0.12), paint, Trunk.
- Large central lower mouth: grille patch @ z 15–32 %, 1.00 wide, rounded.
- Headlamps pair: rounded triple-projector clusters `beveled_box` @ y 2–8 % / z 50 % / ±0.62, 0.28 × 0.38 × 0.08.
- Quad round tail lamps: 2 `cylinder` per side @ y 99 % / z 55 % / ±0.48, ±0.70, Ø0.13 × 0.03 (2 rings; inner
  emissive red, outer red/amber), set in black body-wide panel 0.02 × 1.55 × 0.16.
- Twin large tips: `cylinder` @ y 100 % / z 15 % / +0.50, +0.60, Ø0.09, chrome (or 1 oval 0.16 × 0.08).
- Side intake vent pair (behind front wheel): grille patch @ y 26–30 % / z 40 %, 0.14 × – × 0.08.
- Smooth bubble roof: no crease, tumblehome 0.20, liftback glass seam at y 66 %.

**sperber_32** (L 4.49, H 1.37)
- Power dome: hood_bulge 0.02 × 0.35 (flat-topped).
- Wide low grille (split by a vertical body-colour bar): grille patch @ y 0–1 % / z 48–56 %, 0.60 wide, in 2 halves
  of 0.28 (generic split, not any maker's shape).
- Lower 3-part intake: grille patch @ z 15–30 %, centre 0.80 wide + side ducts 0.20.
- Fender gills pair: `surface_patch` inset @ y 27–31 % / z 50 %, 0.14 × – × 0.06 with 2 slats, chrome/black.
- Bootlid lip: `extrude` @ y 96–99 % / deck, 0.05 × 1.30 × 0.02, paint.
- Quad tips: 2 per side @ y 100 % / z 14 % / ±0.50, ±0.60, Ø0.085, chrome (stacked pair is also fine).
- Diffuser: black `surface_patch` @ z 8–18 %, 1.10 wide, 4 fins.
- Tail lamps pair: L-shaped clusters @ y 98 % / z 57–66 % / ±0.60 wrapping 0.12 onto the quarter, 0.06 × 0.40 × 0.12.
- Carbon roof: livery/material hint (roof region carbon).
- C-pillar reverse kink: `belt_kick` 0.03 + side-window trailing edge kink (window graphic).

**debug_coupe** (L 4.50, H 1.32)
- Front splitter: `box` @ y 0–3 % / z 9 %, 0.10 × 1.70 × 0.02, black.
- Lower mouth: grille patch @ z 14–30 %, 1.20 wide.
- Slim headlamps pair: @ y 1–6 % / z 52 % / ±0.60, 0.22 × 0.42 × 0.07, DRL emissive.
- Hood vents pair: @ y 15–23 % / ±0.30, 0.28 × 0.14, grille.
- Integrated ducktail: knob 0.035.
- Rear light bar: `beveled_box` @ y 99 % / z 62 % / full width −0.20, 0.03 × 1.45 × 0.045, emissive red,
  thicker ends 0.30 × 0.09 at ±0.60.
- Twin tips per side: @ y 100 % / z 14 % / ±0.55, ±0.64, Ø0.09, exhaust.
- Diffuser: black, z 9–19 %, 1.10 wide, 3 fins.

**lucciola_350** (L 4.25, H 1.17)
- Pop-up headlamp pods pair (closed state = flush panel lines; open state optional): `beveled_box` @
  y 6–13 % / z 56 % / ±0.55, 0.28 × 0.34 × 0.03 (closed) / 0.12 rise (open), paint + 2 clear lenses Ø0.10.
- Nose slat intake: grille patch @ y 0 % / z 18–30 %, – × 1.20 × 0.14, 4 horizontal slats black.
- Turn/side lamps in bumper corners pair: `box` 0.03 × 0.20 × 0.05 @ z 30 %, amber.
- Side intake + strakes pair: inset patch @ y 48–62 % / z 32–55 % (door rear + quarter), 0.60 × – × 0.27,
  5 horizontal strakes `box` 0.60 × 0.015 × 0.02 spaced 0.05, paint, LeftDoorF/RearLeftQuarter.
- Engine-deck louvres: 8 transverse slats `box` @ y 64–86 % / on deck, 0.025 × 1.10 × 0.012 spaced 0.11, black, Trunk.
- Black rear mesh panel: grille patch @ y 100 % / z 40–60 %, – × 1.30 × 0.23, RearBumper.
- Quad round tail lamps: 2 per side @ y 100 % / z 52 % / ±0.55, ±0.75, Ø0.12 × 0.03, emissive red (outer
  ring amber ok).
- Quad centre tips: `cylinder` @ y 100 % / z 18 % / ±0.08, ±0.20, Ø0.075, chrome.
- Optional high wing (variant): `extrude` @ y 92–97 % / deck + 0.22, 0.24 × 1.55 × 0.03 on 2 struts.
- Side mirrors on the door "sail": `beveled_box` @ y 33 % / z 72 % / ±1.00, 0.14 × 0.16 × 0.08.

**fulmine_v10** (L 4.46, H 1.17)
- Front splitter: `box` @ y −1–4 % / z 7 %, 0.22 × 1.85 × 0.025, carbon.
- 3-mouth front: outer ducts pair grille patch @ y 0–3 % / z 12–35 % / ±0.62, 0.30 wide (angular
  hex); centre slot @ z 15–22 %, 0.70 wide; black, FrontBumper.
- Slim LED headlamps pair: `beveled_box` @ y 3–12 % / z 52 % / ±0.62, 0.38 × 0.30 × 0.05, clear + angular
  DRL (two emissive strokes 0.20 × 0.012), FrontBumper/Hood.
- Hood vents pair: inset patch @ y 10–20 % / ±0.25, 0.40 × 0.16 × −0.012, grille.
- Side intakes pair (biggest cue): inset patch @ y 50–62 % / z 28–58 %, 0.55 × – × 0.35, depth −0.08,
  black mesh, with horizontal strake blade 0.50 × 0.03 × 0.02 paint, RearLeftQuarter (+ door rear).
- Side skirt blades pair: `extrude` @ y 26–60 % / z 8 %, 1.5 × 0.05 × 0.04, carbon.
- Engine cover glass + louvres: glass panel @ y 62–82 %, 0.85 × 0.80, with 6 `box` louvres
  0.03 × 0.80 × 0.015, `grille` dark.
- Rear wing (fixed variant) or active lip: `extrude` @ y 90–96 % / deck + 0.12, 0.26 × 1.55 × 0.03 on 2 stands; lip
  variant = ducktail 0.03.
- Light-bar tails: `beveled_box` thin C/Y strokes @ y 96 % / z 66 % / ±0.55, 0.04 × 0.45 × 0.035 each plus
  full-width centre strip 0.02 × 0.90 × 0.02, emissive red.
- Black rear mesh fascia: grille patch @ y 99–100 % / z 25–60 %, – × 1.65 × 0.40.
- Diffuser: `surface_patch` @ y 92–100 % / z 6–22 %, 0.35 × 1.40 × 0.18, carbon, 6 fins (0.30 × 0.012 × 0.15).
- Exhaust: 2 hex tips high-centre @ y 100 % / z 40 % / ±0.13, 0.12 × 0.10 hex `lathe`, exhaust (or 4 round Ø0.09 in diffuser).
- Haunch intake scoops on rear fender tops pair: inset patch @ y 66–72 % / z 75 % / ±0.88, 0.18 × 0.10 × −0.03, black.

---

## 3. What makes each genre read at chase-cam distance (top 5 per genre)

Ordered by impact per triangle. Triangle figures are a rough share of the 8–20 k hero budget
(PLAN item 7).

| Typical budget | Tris |
|---|---|
| Body loft (paint shell, 40–60 stations × 24–32 ring verts) | 4.0–7.0 k |
| 4 wheels (rim 600–1 000 + tyre 300–500 each) | 3.6–6.0 k |
| Glass + lamps | 0.5–1.0 k |
| Genre details (this file) | 1.5–4.0 k |
| Mechanical (engine/suspension/brakes, separate agent) | 1.5–3.0 k |

**MUSCLE**
1. **Hood length + set-back cabin** (roof centre ≥ 0.58 L). Pure proportion, 0 extra tris, needs `cabin_shift`.
2. **Rear-up / nose-down rake + fat rear tyres** (`stance_pitch` +0.4–0.6°, rear tyre +0.03 width).
3. **Hood bulge / scoop**: visible over the roof from a raised cam and from every replay; ~150 tris.
4. **Wide horizontal tail lamps + twin round tips** (or quad tips on the modern car). This is the rear signature
   you chase. ~200 tris.
5. **Chrome bumper blades / contrast stripes** (70s) or **black roof + twin stripes** (modern). Material and
   livery, ~50 tris.

**HOT HATCH**
1. **Two-box profile**: tall flat roof running to a near-vertical hatch, tiny rear overhang (0.17 L).
2. **Roof spoiler** at the top of the hatch: the #1 rear cue from a 13° downward cam. ~120–250 tris.
3. **Wheels filling the arches** (Ø/H ≥ 0.43, track/W ≥ 0.85) + −0.02 m drop.
4. **Exhaust signature**: single big tip (90s) or centre/quad tips in a black diffuser (2010s). ~150 tris.
5. **Accent colour**: red lip/stripe on white, or contrast mirror caps. Material only.

**TUNER**
1. **Big rear wing on tall stands** (GT wing / hoop / pedestal). The single strongest tuner cue from behind.
   ~250–400 tris.
2. **Stance**: −0.03 to −0.05 m drop, flush/poke wheels (track/W ≥ 0.86), visible rear camber.
3. **Aftermarket wheels**: deep dish, gold/bronze or mesh, rim ≥ 0.70 of tyre Ø (wheel agent).
4. **Body kit**: wide front lip + side skirts + flares (box flares on drift builds). ~300 tris.
5. **Livery/vinyl + underglow + single cannon exhaust**: night-race readability (emissive).

**SUPERCAR**
1. **Low, wide wedge with cab-forward canopy** (roof centre 0.42–0.46 L, H/W ≈ 0.60).
2. **Giant black side intakes** ahead of the rear wheels. They read even at 20 m as a dark notch. ~200 tris.
3. **Thin light-bar / C-shape tail lamps + black rear mesh** (emissive at night). ~150 tris.
4. **Big wheels** (Ø/H ≥ 0.56) with bright calipers in a tight arch gap.
5. **Diffuser fins + high-centre exhausts + wing/lip**. ~400 tris.

**EXOTIC (classic wedge)**
1. **Straight knife-edge wedge**: nose ≤ 0.58 m, nose plane ≈ windshield plane.
2. **Louvred flat engine deck**: reads from a raised cam as stripes. ~150 tris (boxes), or a texture.
3. **Quad round tail lamps in a black rear panel**. ~200 tris.
4. **Side strakes over the intakes**. Horizontal lines give a strong side read at 6–20 m. ~120 tris.
5. **Pop-up headlamp pods + wide rear track** (rear track/W ≈ 0.86 vs front 0.80, rear tyre +0.04 width).

**Cross-genre rules (low-poly)**
- Silhouette first: the loft knobs carry 60–70 % of genre identity. Spend stations where curvature
  is (nose, windshield base, roof ends, deck edge, arches). Keep flat panels to 2–3 stations.
- Minimum geometric detail ~0.04 m. Below that use texture/material (stripes, rivets, small vents,
  lettering, tyre letters).
- Black is a shape. Intakes, mesh, diffusers and window surrounds in dark `grille`/`carbon` give the
  readable dark notches games use instead of deep geometry. Inset 0.02–0.08 m is enough.
- Rear > sides > top > front for chase-cam priority; front cues for mirrors / police / replays.
- Night: tail-lamp shape (emissive) is the opponent identifier at range, so give every car a
  **distinct lamp pattern**: muscle wide bars / vertical bars, hatch vertical clusters, tuner clear
  round inserts, GT quad round, supercar thin bar, exotic quad round in black panel. hakone_gt and
  lucciola_350 both use quad rounds. Separate them by spacing (0.22 vs 0.20 m) and panel height
  (z 55 % vs 52 %), or give lucciola rectangular lamps if they look too alike in play-test.

## Sources

- Dash-to-axle and cab-rearward proportion: [J.D. Power](https://www.jdpower.com/cars/shopping-guides/what-is-a-dash-to-axle-ratio-in-car-design),
  [Jalopnik](https://jalopnik.com/2114994/car-design-dash-axle-proportions/),
  [Hagerty](https://hagerty.com/media/?p=181035),
  [TTAC Vellum Venom](https://www.thetruthaboutcars.com/2017/11/vellum-venom-dash-axle-defined/?print=1),
  [Vizcom: sports car design](https://vizcom.com/blog/sports-car-design).
- Windshield rake definitions and typical angles: [SlashGear](https://www.slashgear.com/1869096/cars-slanting-windshields-reason/),
  [Justia patent 20240009969](https://patents.justia.com/patent/20240009969),
  [SAE 1999-01-0803](https://saemobilus.sae.org/papers/windshield-spray-performance-high-vehicle-speeds-1999-01-0803),
  [UPV CFD study](https://riunet.upv.es/bitstreams/64c1b920-eeae-4712-89b4-fbcbecdf766f/download).
- Hot hatch cues (chin spoiler, honeycomb, roof spoiler, centre exhaust, diffuser, swollen arches):
  [BusinessLive](https://www.businesslive.co.za/bd/life/motoring/2022-05-09-seven-all-time-hot-hatch-styling-greats/),
  [TimesLive](https://www.timeslive.co.za/motoring/features/2022-05-12-seven-of-the-best-looking-hot-hatches-to-ever-hit-the-streets/),
  [eBay Motors blog](https://ebay.com/motors/blog/whats-the-formula-for-making-a-hot-hatchback),
  Auto123 (2000s subcompact hot-hatch show-car report; maker name withheld, CLAUDE.md rule 9),
  [GarageDreams](https://garagedreams.net/car-facts/what-is-a-hot-hatchback-a-quick-guide-for-car-enthusiasts).
- Tuner customisation grammar (wide body, wings, skirts, roof scoops, neon, vinyl):
  [Team-BHP NFS U2 thread](https://www.team-bhp.com/forum/shifting-gears/1297-nfs-underground-2-a.html),
  [Need for Speed Underground 2 listing](https://moveonup.csd.auth.gr/products/need-for-speed-underground-2/218715887/),
  [PressReader / Play UK](https://www.pressreader.com/australia/play-uk-1/20201020/283403424236322).
- Fictional cars with real-world cues in games: [Burnout Paradise (Wikipedia)](https://en.wikipedia.org/wiki/Burnout_Paradise),
  [Jalopnik: video game cars](https://jalopnik.com/lets-appreciate-these-cool-video-game-cars-inspired-by-1845747852),
  [Giant Bomb: fictional car brands](https://giantbomb.com/wiki/Concepts/Fictional_Car_Brands).
- Low-poly silhouette-first practice: [StudyRaid PS2 low-poly guide](https://app.studyraid.com/en/read/99670/4471908/creating-low-poly-models-for-ps2-hardware-constraints).
- Measured repo values: `data/cars/*.json`, `data/lattices/*.json`, `tools/assets/src/profile.rs`.
  The remaining numeric ranges are design judgement, calibrated against those measured
  dimensions and published body sizes for each archetype.
