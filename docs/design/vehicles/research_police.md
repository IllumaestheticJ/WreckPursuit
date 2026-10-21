# Police vehicles: research and spec (wave 1, research-police)

**Consumers:** wave-2 `style-police` (`tools/assets/src/styles/police.rs`,
`apps/testbed-web/src/render/{livery,police_units}.ts`), `roster` (new `sentinel_warden`,
`sentinel_harrier`), `mechanical` (wheels), foundation (UV atlas, `lightbar`/`decal_carrier`).

**Naming rule (hard):** no real maker or model names, and no real agency names, crests, seals or
trademarked liveries. Every scheme below is a mix of generic conventions. Lettering only uses
the fictional **Port Vesper Police Department (PVPD)** (`docs/design/POLICE_DOCTRINE.md`), its
precincts (Central, Harbor, North, South, West, Loop = Highway Patrol Post 9) and the generic
words POLICE / HIGHWAY PATROL / TRAFFIC / K-9 / DIAL 911 / unit numbers. The emblem is invented:
a **lighthouse inside a ring** (see §4.4). Do not use stars, shields or seals that resemble a
real department's.

Conventions in this doc:
- **Position along the car:** `%L` = fraction of body length measured from the front bumper
  (0 %) to the rear bumper (100 %). To convert to chassis Y (+Y forward, origin at mid-wheelbase):
  `y = y_front_bumper − %L·L`. Front-bumper Y = `wheelbase/2 + front_overhang`.
- **Height:** `z` in metres above the ground at nominal ride height. **Lateral:** `x` in metres
  from the centreline (+X = right).
- Sizes are in metres. Colours are sRGB hex.
- BodyStyle knob values use the knob names from PLAN "Foundation contract" item 1. `style.rs` does
  not exist yet, so these units are assumed: heights and offsets in m, rakes in degrees from
  vertical (windshield) or horizontal (nose), and amounts as 0–1 fractions where 0 means today's
  default shape. The foundation agent should rescale if it picks different units.

---

## 0. What the games do (takeaways only)

| Source | What reads as "police" at speed | Takeaway for us |
|---|---|---|
| Criterion HP-2010 / Rivals (county PD on supercars) | Black-and-white split that follows the body shape, not the doors. Big flat white panels. A thin low LED bar or none (grille, mirror and window strobes). A blue accent stripe on the split line. | The sports patrol (`harrier`) gets a diagonal split and a slicktop. The silhouette does the work. |
| NFS Most Wanted 2005 / Heat | Classic full-size sedans with black body and white doors + roof. The bar is the brightest object at night. The bar sits forward on the roof. | The `cruiser` uses the textbook B/W. The bar's emissive must out-glow headlights. |
| GTA V (fictional city/county agencies) | Fictional city names + a generic "POLICE". A sheriff scheme in tan/green/brown. A highway scheme with "STATE"-style wording. Unit numbers on the roof and trunk. | Wording works: an invented city name + a generic word is enough. Tan/brown is our highway scheme. |
| Burnout (Paradise/Revenge) | Exaggerated push bars and huge bars, read as a silhouette at 200 km/h. | Push bars ~10 % oversize vs. real are fine. |
| Driver (70s/90s period) | Boxy body-on-frame sedans, steel wheels with dog-dish caps, a single dome or twin rotators, an A-pillar spotlight. | `warden` = period piece: rotators, a spotlight, steelies, a whip antenna. |
| Forza Horizon (festival police-livery cars) | Mostly vinyls. Livery alone on a sports car looks like "cosplay" unless equipment is added (push bar, spotlight, antennas). | Always pair livery with at least 3 equipment items. |

Real-world generic conventions used throughout:
- **Agency name on each side and on the back.** One state code sets letters ≥ 4 in (0.10 m). We
  use 0.12–0.22 m cap height for primary words and 0.06–0.09 m for secondary lines.
- **Battenburg:** two rows of alternating light/dark blocks (min. block ~600 × 300 mm in the UK
  guidance). The light colour is fluorescent (day) and the dark one retroreflective (night).
- **Aerial roof markings** (unit IDs readable from aircraft): a German guide recommends ≥ 0.40 m
  glyphs. We use 0.40–0.50 m.
- **Light bars:** 1940s–50s single rotating beacon ("gumball"). 1970s roof-width bar with two
  rotators + mirrors + flashers. 80s–90s halogen rotator bars, later with strobes. 2000s–2010s
  LED, low-profile bars ~2–2.25 in (0.05–0.057 m) tall, 44–70 in (1.12–1.78 m) long, ~12 in
  (0.30 m) deep. Mini bars are 17–23 in. Lower front tier = clear **takedown** lights. Side-facing
  clear = **alley** lights. Rear = amber **traffic-advisor** arrow stick.
- Push bars ("ram/nudge bars") protect the nose for pushes/PIT. One or two A-pillar spotlights are
  aimed from inside. Steel wheels with small chrome centre caps are cheap and dent-proof. A
  partition (metal + polycarbonate) sits behind the front seats. The MDT laptop sits on a pedestal
  mount at the centre console, angled at the driver.

---

## 1. The five versions

Shared equipment kit (all lengths in m; parts emit through the PLAN item 3 part API, with region
in brackets):

| Item | Geometry | Material | Region |
|---|---|---|---|
| `pushbar` | 2 vertical uprights Ø0.05 tube + 2 horizontal rails Ø0.05 (z 0.40 and 0.78 for sedans), width 0.95–1.25. A rubber pad strip 0.06 × 0.03 on the top rail. Stands 0.10 ahead of the bumper (matches the lattice Accessory nodes). | `grille` (black powder-coat), pads `trim` | `Accessory` (detachable group 8) |
| `spotlight` | Housing Ø0.13 × 0.14 cylinder + lens disc `lightbar_clear` + a Ø0.025 × 0.20 handle shaft through the A-pillar. Centre sits 0.06 outboard of the A-pillar, 0.10 below the roof edge. | `chrome` (80s) / black (modern) | `A-pillar / door front` |
| `antenna_whip` | Ø0.006 × 0.75 cylinder + Ø0.03 base | black | roof or trunk |
| `antenna_puck` | Low dome 0.09 × 0.06 × 0.02 (modern GPS/LTE pucks) ×2–4 | dark grey | roof |
| `partition` | Plane 1.35 × 0.55 at the B-pillar, upper half `glass` with a dark tint, lower half black mesh look (`grille`) | | `Cabin` |
| `mdt` | Box 0.32 × 0.23 × 0.03 lid at 60° on a Ø0.03 × 0.30 pedestal, centre console, x = +0.15 | dark grey, screen `emissive` dim blue-white | `Cabin` |
| `steel_wheel` | See `mechanical`: flat dish, 5 round 30 mm "vent" holes, Ø0.17 chrome dog-dish cap (80s) / Ø0.08 black plastic cap (modern), satin black | | wheel |
| `traffic_advisor` | Rear-deck bar inside the rear glass: 0.90 × 0.05 × 0.04, 8 amber cells | `emissive_amber` | `Cabin` |
| `grille_leds` | 2–4 modules 0.12 × 0.03 × 0.02 in the grille opening, z ≈ grille centre | red/blue | `Front` |
| `visor_bar` | Inside the windshield top: 0.80 × 0.04 × 0.04, split red | blue, with a black back-plate (prevents interior glare) | `Cabin` |
| `mirror_leds` | 0.06 × 0.015 pod under each mirror shell | red (L) / blue (R) | `Door FL/FR` |
| `rear_window_bar` | Below the rear glass top: 0.90 × 0.035 × 0.035, red | amber | blue | `Cabin` |
| `wigwag` | Headlamp + tail-lamp flash. Not geometry: a renderer pattern on the existing lamp materials. | | |

### 1.1 `sentinel_cruiser`: "sedan" (2010s full-size RWD V8 sedan, everyday patrol)

Dims (existing): 1.91 W × 5.08 L × 1.48 H, wheelbase 3.05, wheel r 0.353 (225/60R18). Wheel
centres sit at ~19.5 %L and ~79.5 %L (front overhang ≈ 0.99).

**BodyStyle**
| Knob | Value | Note |
|---|---|---|
| silhouette | `Notchback` | three-box, formal roofline |
| nose height / rake | 0.78 m / 8° | blunt, upright, tall grille |
| nose plan rounding | 0.35 | moderately square corners |
| hood crown / bulge | 0.025 m crown, no bulge (0, 0) | flat hood (the livery reads well on it) |
| hood power-bulge / scoop | 0 / none | |
| windshield rake | 60° from vertical | |
| A / C pillar rake | 58° / 52° | C-pillar formal and steep |
| roof length / height offset | 1.70 m / 0.00 | long roof for rear-seat room |
| tumblehome | 0.12 | |
| side sculpt crease | z 0.86 m, depth 0.012 m | one body line at the door-handle height, which also bounds the lettering zone |
| shoulder / haunch | 0.02 | little rear swell |
| wheel-arch flare | 0.01, `Round` | |
| ride-height visual offset | +0.01 m | pursuit package sits level |
| tail kamm / ducktail | 0 / 0.015 m | small integrated lip |
| overhang trims | front 0, rear +0.03 m | long trunk |
| beltline kick-up | 0.03 m at C-pillar | |

**Equipment**
| Item | Position | Size |
|---|---|---|
| Light bar: modern LED low-profile (L-2010) | Centre at 44 %L (just behind the B-pillar), roof top + 0.03 feet | 1.22 × 0.30 × 0.055 |
| Push bar | Front, z 0.35–0.80, width 1.10 | §1 kit |
| Spotlight | Driver side (left, x −0.78), z 1.12, at the A-pillar base ≈ 34 %L | Ø0.13 |
| Antenna pucks | 3 on the roof at 58–66 %L, x −0.25 / 0 / +0.25 | |
| Antenna whip | Trunk lid, 90 %L, x +0.35 | 0.75 |
| Partition, MDT | Interior | |
| Traffic advisor | Rear parcel shelf, 72 %L, z 1.08 | |
| Grille LEDs | 2 modules at x ±0.22, z 0.62, 1 %L | |
| Wig-wag | Headlamps (white) + tail lamps (red) | |
| Wheels | `steel_wheel`, modern: satin black, Ø0.08 black cap, 18 in | |

**Livery `pvpd_bw`: classic two-tone black-and-white**
- Base: gloss black `#111317`.
- Second tone, white `#F3F4F6`: **front doors, rear doors, roof panel** (windshield header to
  backlight top), and **C-pillars**. Door white runs from the rocker (z 0.30) up to the beltline
  (z 0.98). The front and rear vertical edges follow the door shut-lines (front door 32–52 %L, rear
  door 52–69 %L). Hood, fenders, quarters, trunk and bumpers stay black.
- Stripe: none (the hard tone split *is* the graphic). A 0.008 m silver pinline separates the
  tones on the door edges (optional, LOD-0 only).
- Lettering (all white areas carry black text, black areas carry white text):

| String | Where | Cap height / font | Position |
|---|---|---|---|
| `POLICE` | Front door, both sides | 0.16 m, **bold condensed sans**, black, tracking +4 % | Centred on the front door (42 %L), baseline z 0.56 |
| `PORT VESPER` | Front door, above POLICE | 0.06 m, bold condensed sans, black | Baseline z 0.76 |
| Emblem (lighthouse ring) | Front door, behind POLICE | Ø0.30 ring, 0.012 m line, black | Centre at 42 %L, z 0.63, drawn *under* the text at 25 % opacity, or omitted at LOD-1 |
| `DIAL 911` | Rear door, both sides | 0.07 m bold condensed, black | 60 %L, baseline z 0.48 |
| `C-12` (unit) | Rear quarter (black), both sides | 0.11 m bold condensed, white | 80 %L (above the rear wheel), baseline z 0.82 |
| `POLICE` | Trunk lid (black) | 0.10 m bold condensed, white | Centred, 0.05 below the lid edge |
| `C-12` | Rear bumper / trunk, right of the plate | 0.07 m, white | |
| `C12` | **Roof** (white), aerial | 0.45 m cap, bold condensed, black, 0.04 stroke weight | Centred on the roof at 60 %L, reading upright from behind (glyph tops toward the front) |
| `POLICE` | Front bumper / hood front (optional) | 0.07 m white | |

- Unit number format: `<precinct letter>-<2 digits>` (Central-12 → `C-12`). Precinct letters:
  C Central, H Harbor, N North, S South, W West, L Loop. The roof uses the same string without
  the hyphen. The doctrine number blocks apply (01–19 day, 20–39 evening, 40–59 night).

### 1.2 `sentinel_warden`: "old sedan" (NEW: 80s/90s boxy body-on-frame RWD V8 sedan)

Roster suggestion (for the `roster` agent): 1.98 W × 5.40 L × 1.43 H, wheelbase 2.90, front
overhang 1.05, 225/70R15 (unloaded r 0.348, loaded 0.335), ~1 850 kg, 5.7 L pushrod V8, 4-speed
auto, live rear axle, perimeter frame, `durability_class` 4. Role: the old fleet still on night
shift (call-signs 40–59), slow but tough. It is the car that goes to the auction (`ex_fleet_bw`
theme in `CARS_LORE.md`).

**BodyStyle**
| Knob | Value | Note |
|---|---|---|
| silhouette | `Notchback` | |
| nose height / rake | 0.80 m / 2° | flat, vertical grille face, near-vertical |
| nose plan rounding | 0.05 | square corners |
| hood crown / bulge | 0.005 m / (0, 0) | dead flat, long |
| hood power-bulge / scoop | 0 / none | |
| windshield rake | 52° | upright for the era |
| A / C pillar rake | 50° / 25° | formal near-vertical C-pillar, sometimes opera window |
| roof length / height offset | 1.62 m / −0.02 | |
| tumblehome | 0.06 | slab-sided |
| side sculpt crease | z 0.80 m, depth 0.006 m | plus a 0.03 m-high chrome/black rub strip at z 0.55 |
| shoulder / haunch | 0 | |
| wheel-arch flare | 0, `Box` (squared arch cut-outs, rear arch partially skirted) | |
| ride-height visual offset | +0.03 m | soft springs, nose-up stance |
| tail kamm / ducktail | 0 / 0 | flat trunk |
| overhang trims | front +0.05, rear +0.08 | long overhangs, 5 mph bumpers stick out 0.10 |
| beltline kick-up | 0 | dead straight beltline |

**Equipment**
| Item | Position | Size |
|---|---|---|
| Light bar: twin-rotator full-width (L-80s) | Centre at 45 %L, roof + 0.04 feet | Base 1.22 × 0.30 × 0.06 + 2 dome lenses per side, each 0.28 × 0.26 × 0.13 (red L outer, blue R outer, or red/red per era), clear centre box 0.36 × 0.20 × 0.09 with 2 takedown lamps forward + 1 alley lamp each end |
| Alt bar (variant `warden_70s`) | Single dome "gumball" centred at 45 %L | Ø0.20 × 0.22 dome on Ø0.24 × 0.04 base, red |
| Push bar | None stock. Optional `bumper_guard`: 2 rubber-faced vertical posts 0.12 × 0.25 at x ±0.35 | |
| Spotlights | **Both** A-pillars (x ±0.80), z 1.08, at 33 %L, chrome | Ø0.15 (larger, older) |
| Antenna whip | Trunk lid at 92 %L, x 0, + a second on the right rear fender | 0.80 / 0.60 |
| Partition | Interior, wire-mesh look | |
| MDT | None. Dash radio box 0.20 × 0.18 × 0.06 under the dash, centre | |
| Siren speaker | Behind the grille or a roof-bar centre, black | 0.15 × 0.15 × 0.12 |
| Wheels | `steel_wheel`, 80s: body colour or black, Ø0.17 chrome dog-dish cap, 15 in, tall sidewall | |

**Livery `pvpd_bw_classic`, default** (same family as the cruiser, so the city reads as one fleet):
- Base black `#16171A`. White `#EEEEE8` (slightly warm, aged) on **front + rear doors and roof**.
  The white door panel is a **pure rectangle**, from rocker to beltline, square to the ground. This
  is the era look: the paint edge does not follow the shut-line curve.
- Rub strip chrome at z 0.55 (geometry, not livery).
- Lettering font: **bold extended (wide) sans, upright**, gives an 80s feel. Implementation:
  draw condensed and set `ctx.scale(1.25, 1)`, or use a wide face.

| String | Where | Height / style | Position |
|---|---|---|---|
| `POLICE` | Front door | 0.13 m, extended sans, black | 42 %L, baseline z 0.62 (above the rub strip) |
| Emblem ring | Front door, *above* POLICE | Ø0.26 solid black disc with white lighthouse | 42 %L, centre z 0.82 |
| `PORT VESPER` | Rear door | 0.06 m, extended sans, black | 61 %L, baseline z 0.66 |
| `C-44` | Front fender (black), both sides, behind the wheel | 0.10 m, white | 22–26 %L, baseline z 0.70 |
| `C44` | Roof (white) | 0.40 m, black | 60 %L |
| `POLICE` + `C-44` | Trunk lid | 0.09 m, white | |

**Variant `warden_white_blue`** (another precinct, or a period flashback): white base `#F0F0EC`.
A **medium-blue `#1E4FA8` beltline band** 0.10 m tall from z 0.74 to 0.84, full length nose to
tail, interrupted by nothing. A 0.015 m gap and a 0.012 m pinstripe below it. Text in blue
`#1E4FA8`, `POLICE` 0.13 m extended sans on the front door below the band (baseline z 0.52).
Twin rotators both red for this variant.

### 1.3 `sentinel_wolfhound`: "patrol" (AWD twin-turbo V6 pursuit sedan, highway-patrol style)

Dims (existing): 1.94 W × 5.05 L × 1.55 H, wheelbase 2.87, r 0.352 (245/55R18). Wheel centres at
~21 %L / ~78 %L. Role: Loop 9 / Highway Patrol Post 9.

**BodyStyle**
| Knob | Value | Note |
|---|---|---|
| silhouette | `Notchback` (short deck, fast C) | modern pursuit sedan with a tall beltline |
| nose height / rake | 0.82 m / 14° | tall, aggressive face |
| nose plan rounding | 0.55 | |
| hood crown / bulge | 0.035 m / (0.015 m, 0.70 width) | a raised centre section |
| hood power-bulge / scoop | 0.01 / none | |
| windshield rake | 62° | |
| A / C pillar rake | 62° / 60° | fast C-pillar |
| roof length / height offset | 1.45 m / +0.03 | taller cabin |
| tumblehome | 0.15 | |
| side sculpt crease | z 0.92 m, depth 0.018 m | strong shoulder crease that rises toward the rear |
| shoulder / haunch | 0.05 | |
| wheel-arch flare | 0.02, `Round` | |
| ride-height visual offset | +0.02 m | |
| tail kamm / ducktail | 0.1 / 0.02 m | |
| overhang trims | front 0, rear −0.03 | |
| beltline kick-up | 0.06 m at the C-pillar | high, rising beltline |

**Equipment**
| Item | Position | Size |
|---|---|---|
| Light bar: modern LED low-profile (L-2010), full width | 43 %L, roof + 0.03 | 1.35 × 0.30 × 0.055 |
| Push bar | Front, z 0.35–0.80, width 1.15, plus 2 forward LED cubes on the top rail at x ±0.40 | |
| Spotlight | Driver (left) A-pillar, black LED housing, z 1.18 | Ø0.12 |
| Antenna pucks | 4 on the roof, 2 rows at 58 %L and 64 %L, x ±0.22 | |
| Visor bar | Windshield top inside, z roof − 0.08 | |
| Rear window bar | Rear glass top inside | red/amber/blue |
| Traffic advisor | In the rear window bar (amber centre segment) | |
| Partition, MDT | | |
| Mirror LEDs | Both | |
| Rear spoiler | Small decklid lip 0.04 tall, body colour | |
| Wheels | `steel_wheel`, modern black, 18 in, Ø0.08 black cap. Variant: black 5-spoke alloy | |

**Livery `pvpd_highway`: tan-over-dark two-tone, gold accents**
- Upper tone tan `#CDBB8E`: **roof, pillars, and the body above the shoulder crease (z ≥ 0.92)**
  along the full length, including the decklid top surface and the hood. The split follows the
  crease, so it kicks up at the C-pillar.
- Lower tone dark umber `#2B241C`: everything below the crease, plus bumpers.
- Stripe: a **gold pinstripe `#C99A2E`**, 0.018 m tall, on the tone split, plus a second 0.006 m
  stripe 0.012 below it. At the front fender the pair sweeps down 0.06 m in a 0.40 m radius toward
  the headlamp (a "hockey-stick" end).
- Lettering font: **bold condensed sans, outlined**. Gold fill `#D9AA3A` + 0.006 m black outline
  on the dark lower body. On tan areas use umber fill with no outline.

| String | Where | Height / style | Position |
|---|---|---|---|
| `HIGHWAY PATROL` | Front + rear doors (one line spanning both, or on the front door only in 2 lines `HIGHWAY` / `PATROL`) | 0.12 m, gold, outlined | Single line: centred on 50 %L, baseline z 0.60. Two-line: 41 %L, baselines z 0.70 / 0.55 |
| Emblem (lighthouse ring, gold) | Front fender, behind the front wheel | Ø0.22, gold line art | 26 %L, centre z 0.72 |
| `PORT VESPER` | Rear door, below the main line | 0.055 m, gold | 60 %L, baseline z 0.46 |
| `L-L1` | Rear quarter | 0.10 m gold | 82 %L, baseline z 0.75 |
| `HIGHWAY PATROL` | Decklid face (vertical rear panel) | 0.08 m umber on tan | Centred |
| `LL1` | Roof (tan) | 0.45 m, umber | 60 %L |

- Unit format: `<precinct>-L<n>` (Loop Lima-1 → `L-L1`; Central Lima-2 → `C-L2`).
- **Variant `wolfhound_ghost`** (slicktop "ghost" traffic car): solid charcoal `#2E3136` metallic,
  **no roof bar** (visor bar + grille + mirror + rear-window LEDs only). Lettering `POLICE` /
  `TRAFFIC` in reflective silver-grey `#5A5F66` (≈ 12 % lighter than the base, so it is barely
  visible by day). In the night shader, use the livery mask to boost the lettering
  roughness/emissive by 0.15 when lit by headlights, or simply raise its albedo to `#8A9098` at
  night. Same positions as above. Steel wheels black. Driver spotlight kept (the giveaway).

### 1.4 `sentinel_bastion`: "SUV" (full-size body-on-frame SUV, roadblock / port units)

Dims (existing): 2.04 W × 5.18 L × 1.89 H, wheelbase 2.95, r 0.36 (265/60R17). Wheel centres at
~18 %L / ~75 %L. Front overhang 0.95.

**BodyStyle**
| Knob | Value | Note |
|---|---|---|
| silhouette | `Suv` | two-box, upright tailgate |
| nose height / rake | 1.08 m / 4° | tall flat face |
| nose plan rounding | 0.25 | |
| hood crown / bulge | 0.03 m / (0.02 m, 0.55) | |
| hood power-bulge / scoop | 0 / none | |
| windshield rake | 58° | |
| A / C (D) pillar rake | 57° / 8° | near-vertical D-pillar |
| roof length / height offset | 2.70 m / 0.00 | full-length roof to the tailgate |
| tumblehome | 0.08 | |
| side sculpt crease | z 1.12 m, depth 0.012 m; lower cladding crease at z 0.55 | |
| shoulder / haunch | 0.02 | |
| wheel-arch flare | 0.03, `Box` (squared truck arches with black cladding) | |
| ride-height visual offset | +0.04 m | |
| tail kamm / ducktail | 0 / 0 | |
| overhang trims | 0 / 0 | |
| beltline kick-up | 0.02 at the D-pillar | |

**Equipment**
| Item | Position | Size |
|---|---|---|
| Light bar: modern LED low-profile, wide | 33 %L (over the front seats, an SUV bar sits forward), roof + 0.04 | 1.37 × 0.30 × 0.06 |
| Push bar | Heavy: 4 uprights, top rail z 1.00, lower z 0.48, width 1.30, plus 2 LED cubes on the top rail | |
| Spotlight | Driver A-pillar, z 1.55 | Ø0.12 |
| Roof rack / cargo cage (optional variant `bastion_k9`) | Rear 55–90 %L, 0.06 tall rails | |
| Antenna pucks | 4, at 50–60 %L | |
| Rear window bar | Top of the tailgate glass, interior | red/amber/blue |
| Traffic advisor | Rear window bar centre, amber | |
| Rear cargo partition | At the 2nd/3rd row, mesh | |
| Tow hooks | 2 red-painted Ø0.04 hooks below the bumper, x ±0.55 | |
| Wheels | `steel_wheel` modern, 17 in, 6-lug truck pattern, black, Ø0.10 chrome cap | |

**Livery `pvpd_suv`: white with dark band and reflective stripe** (modern "white + colour" scheme)
- Base white `#F2F4F5`.
- Second tone navy `#14223F`: **lower body from rocker to the cladding crease (z 0.30–0.55)**
  full length, plus the **bumpers** and the **hood top surface** (a navy hood cuts glare and reads
  from above). Doors, roof and pillars stay white.
- Stripe: a **reflective cyan-blue band `#1E7FD0`**, 0.10 m tall, at z 0.60–0.70 full length. It
  breaks into **forward-raked chevrons** (0.10 wide, 45°, 0.08 gap) along the last 0.60 m of the
  rear quarter. A thin 0.012 m green `#2FA15A` pinline sits under it (the "blue/green" signature
  of the SUV fleet).
- Lettering: bold condensed sans, navy `#14223F` on white.

| String | Where | Height | Position |
|---|---|---|---|
| `POLICE` | Front + rear doors (one word across both) | 0.22 m | Centred on 44 %L, baseline z 0.80 |
| `PORT VESPER` | Front fender above the arch | 0.07 m | 18 %L, baseline z 1.02 |
| Emblem ring | Rear door/C-pillar area | Ø0.30, navy | 58 %L, centre z 1.05 |
| `S-B3` | Rear quarter, above the band | 0.12 m | 82 %L, baseline z 0.82 |
| `POLICE` | Tailgate below the glass | 0.12 m navy | |
| `DIAL 911` | Rear bumper top (navy area) | 0.06 m white | |
| `SB3` | Roof | 0.50 m navy | 55 %L |

- Unit format: `<precinct>-B<n>` (South Bravo-3 → `S-B3`).
- **Variant `bastion_port`, European "battenburg" flavour** (Harbor/Port units only; a nod to the
  container terminal, not a copy of any national scheme):
  - Base white. Sides carry **two rows of alternating blocks** from the front wheel arch to the rear
    lamp. Block 0.50 × 0.25 m (scaled from the ~600 × 300 guidance to fit a 5 m car), rows at
    z 0.42–0.67 and 0.67–0.92, offset by one block (checker). Colours: **fluorescent yellow-green
    `#D4F21A`** + **blue `#1A3FB8`**. Use the same colours as a single row on the hood front edge.
  - Rear: a full tailgate / bumper **chevron field**, alternating `#D4F21A` / `#1A3FB8` 0.12 m
    bands at 45°, pointing up (inverted V).
  - `POLICE` 0.18 m bold condensed in blue on a **white rounded panel** 1.10 × 0.26 placed above
    the checker (z 1.00). Roof `HB2` 0.50 m black on white.
  - Light bar: **blue-only** LED bar (both halves blue; red reserved for the rear-facing stop
    message cells). This is the European colour convention.
  - **K-9 sub-variant:** `K-9` 0.14 m on the rear quarter glass/panel.

### 1.5 `sentinel_harrier`: "sports patrol" (NEW: modern muscle-coupe pursuit car)

Roster suggestion: 1.93 W × 4.82 L × 1.37 H (body), wheelbase 2.75, 275/40R20 (unloaded r 0.364,
loaded 0.353), ~1 800 kg, 6.2 L supercharged V8 or a 5.0 L DOHC V8 (the `rampart_50` class
engine), RWD + LSD, `durability_class` 3. Role: high-heat pursuit unit for sports cars and
exotics (L4–L5). Proposed call sign `<Precinct> Hotel-<n>` (body `C-H1`). This is **new and needs
sign-off from the doctrine owner**: add a row to `POLICE_DOCTRINE.md` §8.1.

**BodyStyle**
| Knob | Value | Note |
|---|---|---|
| silhouette | `Fastback` | |
| nose height / rake | 0.70 m / 12° | shark-nose, forward-leaning grille |
| nose plan rounding | 0.40 | |
| hood crown / bulge | 0.03 m / (0.035 m, 0.45 width) | |
| hood power-bulge / scoop | 0.04 m / `Vent` pair (heat extractors) at 25–32 %L | |
| windshield rake | 64° | |
| A / C pillar rake | 64° / 72° | long fast C into a short decklid |
| roof length / height offset | 1.15 m / −0.06 | low roof |
| tumblehome | 0.18 | |
| side sculpt crease | z 0.78 m, depth 0.02 m | crease runs into the rear haunch |
| shoulder / haunch | 0.10 | strong rear fender swell |
| wheel-arch flare | 0.04, `Round` | |
| ride-height visual offset | −0.02 m | |
| tail kamm / ducktail | 0.5 / 0.03 m | |
| overhang trims | front −0.02, rear −0.05 | |
| beltline kick-up | 0.05 at the quarter window | |

**Equipment**
| Item | Position | Size |
|---|---|---|
| Light bar: **slicktop** (L-SLICK) by default. Variant `harrier_bar`: aero mini-bar | Slicktop: visor bar inside the windshield top, grille LEDs, mirror LEDs, rear-window bar, decklid-spoiler LEDs. Mini-bar: 0.95 × 0.25 × 0.05 at 45 %L | |
| Push bar | Compact "PIT bumper": a single horizontal rail z 0.38, plus 2 short uprights to z 0.62, width 0.95 | |
| Spotlight | Driver A-pillar, black low-profile LED | Ø0.10 |
| Rear spoiler | Decklid pedestal wing: blade 1.40 × 0.20 × 0.02 at 0.12 above the decklid, 2 pedestals. Carries a 0.80 × 0.025 red/blue LED strip on the trailing edge | `carbon` or body colour |
| Front splitter | 0.06 protrusion, `carbon` | |
| Antenna pucks | 2 low pucks on the roof rear, 70 %L | |
| Partition | Light (two-seat cell, cage bar behind the front seats) | |
| MDT | | |
| Wig-wag | Headlamps + tails | |
| Wheels | **Not steel.** Black 5-split-spoke 20 in alloy, brake calipers `caliper` tinted blue `#2050C0` | |

**Livery `pvpd_pursuit`: diagonal B/W split + blue accent (game-style)**
- Base gloss black `#0E0F12`.
- Second tone white `#F4F5F7`: a **forward-raked diagonal panel**. The split line starts at the
  front-door leading edge at the rocker (31 %L, z 0.30), rises rearward at 18° to reach the
  beltline (z 0.88) at 64 %L, then runs along the beltline to the tail. Everything **below/behind
  the line** is white: lower front door, whole rear quarter, tail panel. Roof stays black. The
  hood is black with a **white 0.30 m-wide centre stripe pair** (two 0.13 stripes, 0.04 gap) nose
  to tail over the roof and decklid. That is the muscle-car signature and reads from the helicopter.
- Accent stripe: **electric blue `#1C6CFF`**, 0.035 m, offset 0.02 m onto the black side of the
  diagonal split, full length of the split.
- Lettering: **bold condensed italic sans** (8–10° slant, forward-leaning on both sides; see §3.3).

| String | Where | Height / colour | Position |
|---|---|---|---|
| `POLICE` | Door, on the white diagonal, black text | 0.20 m, italic condensed | Centred at 50 %L, baseline z 0.40, angled +0° (text stays horizontal, panel is diagonal) |
| `PURSUIT` | Rear quarter above POLICE, black | 0.07 m | 70 %L, baseline z 0.72 |
| `PORT VESPER` | Front fender (black), white | 0.06 m | 20 %L, baseline z 0.72 |
| `C-H1` | Rear quarter near the tail lamp | 0.09 m black | 90 %L, baseline z 0.55 |
| `POLICE` | Rear tail panel / below the spoiler | 0.10 m black on white | |
| `CH1` | Roof (black) | 0.45 m white | 62 %L |
| `DIAL 911` | Rear bumper, white on black diffuser | 0.05 m | |

---

## 2. Flash patterns

Engine for every pattern: `on(channel, t) = any(t mod period ∈ [t0, t1))` over a list of on-windows.
Express each pattern as data so `police_units.ts` and `vehicle_view.ts` share it:

```ts
type Pattern = { period: number; ch: Record<string, [number, number][]> };
```

Real rates sit roughly in 60–260 flashes/min per lamp (one state code range). **Accessibility:**
WCAG 2.3.1 limits full-field flashing to ≤ 3 flashes/s. Keep the **scene point/spot lights** that
light the world on a slower envelope (≤ 3 Hz, soft attack/decay ~40 ms), and use fast strobing
only on the small emissive lens meshes. Add a "reduced flashing" option that swaps every pattern
for `P-SLOW` below.

| Id | Era / use | Period | Channel on-windows (seconds within period) | Notes |
|---|---|---|---|---|
| `P-GUMBALL` | 70s single dome (`warden_70s`) | 0.75 s (80 rpm) | Rotating: not windowed. Lens emissive = `0.25 + 0.75·max(0, cos(θ − φ_view))^6`, θ = 2π·t/0.75 | Add a rotating SpotLight (cone 25°) for the sweep on walls. One-bulb reflector gives 80 flashes/min. |
| `P-ROTATOR` | 80s–90s twin rotators (`warden`) | Each rotator 0.60 s (100 rpm). L and R start 180° apart. | Rotating as above per dome. Centre clear `flasher`: on [0, 0.15) of a 0.6 s cycle. Takedowns steady when stopped. | The classic "sweep". Mirrors double apparent flashes: put 2 lobes per rev (`cos(2θ)` with 2 SpotLights back-to-back) |
| `P-STROBE` | late-90s/2000s strobe add-on | 0.80 s | red: [0, 0.04), [0.10, 0.14); blue: [0.40, 0.44), [0.50, 0.54) | Xenon double-flash. Very white-hot core: lens colour lerps to `#FFFFFF` at 60 %. |
| `P-LED-QUAD` | 2010s LED primary (pursuit) | 0.50 s | red (left half): [0.000, 0.040), [0.060, 0.100), [0.120, 0.160), [0.180, 0.220); blue (right half): same +0.25 | Quad flash side to side, 120 cycles/min. The scene light uses envelope red [0, 0.22), blue [0.25, 0.47). |
| `P-LED-WIGWAG` | 2010s LED cruise/response | 0.70 s | red: [0, 0.30); blue: [0.35, 0.65) | Slow alternation, used below L3 or for "lit up for a stop" (L1). |
| `P-LED-SIM` | 2010s "all-flash" for roadblocks (parked) | 1.00 s | red + blue together: [0, 0.08), [0.15, 0.23); white takedowns: [0.50, 0.58), [0.65, 0.73) | Parked blocks look different from moving units, which helps readability. |
| `P-HEADLAMP-WW` | Wig-wag headlamps (all modern + warden) | 0.70 s | left headlamp: [0, 0.35); right: [0.35, 0.70). Tail lamps same phase, mirrored | Run only while pursuing. The base headlamp emissive is replaced, not added. |
| `P-SLOW` | Reduced-flashing option | 1.00 s | red: [0, 0.45); blue: [0.50, 0.95), 60 ms ramps | ≤ 2 flashes/s per colour. |

Amber/white auxiliary lights:

| Light | Era | Behaviour |
|---|---|---|
| Takedown (front-facing clear, in the bar centre) | 80s+ | **Steady** when the unit is stopped behind a target (Capture / BoxIn). Flash white in `P-LED-SIM` and in modern pursuit at [0.22, 0.25) and [0.47, 0.50) of `P-LED-QUAD` (a "white pop" between colours). |
| Alley (side-facing clear, bar ends) | 80s+ | Steady on when parked at a roadblock (side facing the road). Off while moving. |
| Traffic advisor (rear amber, 8 cells) | 90s+ | **Sequential**, 80 ms per step: `left` = cells 7→0 fill, then all off 160 ms (period 0.80 s). `centre-out` = cells (3,4)→(0,7), 4 steps + 160 ms off (period 0.48 s). `warn` = alternate odd/even cells at 0.5 s. Use `left/right` when a roadblock is ahead in that lane, `warn` otherwise. |
| Rear amber/red in the rear-window bar | 2010s | While pursuing, amber cells flash with blue timing and red with red. When parked: amber only, `warn` pattern. |
| Gumball/rotator era | 70s–80s | No amber. A rotator's clear "takedown" is a steady sealed-beam pair. |

Renderer notes vs. the current code:
- `vehicle_view.ts` builds the bar as two 0.32 W × 0.25 × 0.12 boxes at `y = −0.1, z = h + 0.08`,
  with a 2.5 Hz double-strobe and one PointLight (intensity 25, range 18). `scene.ts`
  `updatePoliceUnits` duplicates this logic on box proxies with a per-unit phase offset
  `i·0.17`. Replace both with the shared `Pattern` table. Keep the per-unit phase offset (de-syncs a
  convoy) but derive it from a unit id hash, not the array index, so it is stable when the list
  changes.
- A 0.12 m-tall bar is right for `P-ROTATOR` (0.13 domes) but 2× too tall for LED bars (0.055).
- Use 2 lights (red L, blue R at bar ends, distance 20, decay 2) instead of one colour-switching
  light, so red/blue cast from the correct side. Drop to 1 light for cars beyond 60 m and 0 beyond
  120 m (emissive-only plus a bloom sprite).
- Light lens segments: modern LED bar = 6 modules per half (each 0.09 × 0.05), for `P-LED-QUAD`
  to flash outer 3 then inner 3 ("split") at alternate cycles if wanted. The center 2 modules are
  clear (takedowns). The ends are angled 45° (alley/corner).

---

## 3. Canvas-texture lettering for the side-projected livery atlas

Atlas (PLAN item 6): left side `u ∈ [0, 0.25)`, right side `u ∈ [0.25, 0.5)`, top
(hood/roof/trunk) in its own v-band of `u ∈ [0, 0.5)`, and `u ∈ [0.5, 1)` keeps its existing use.
The UV layout must be fixed in `data/panels/README.md` by the foundation. Assumed here: within
each side band, **u increases in the direction a reader outside the car sees as left → right**
(left side: front → rear; right side: rear → front), and v increases with height (z). Top band:
u along the car width, v from the front bumper to the rear.

### 3.1 Canvas size and mapping
- One `HTMLCanvasElement` per **livery variant** (not per unit), 4096 × 2048, covering only
  `u ∈ [0, 0.5)`. Set `texture.repeat.set(2, 1)` with `offset 0` on the `livery` material, or
  remap UVs (`u' = 2u`) in the mesh loader, so the whole canvas maps onto the livery half. That
  gives 2048 px per side over ~5.1 m = **~400 px/m along the length**. A 0.06 m secondary line =
  24 px cap height, readable. A 0.16 m `POLICE` = 64 px.
- Rows: side band `v ∈ [0, 0.5)` → canvas rows 1024–2047 (bottom half; `flipY` true: canvas
  top = v 1). Height ~1.0–1.9 m over 1024 px. Top band `v ∈ [0.5, 1)` → rows 0–1023.
- Lower-spec / traffic LOD: 2048 × 1024 (200 px/m). Text ≥ 0.10 m stays legible.
- `THREE.CanvasTexture`: `colorSpace = SRGBColorSpace`, `generateMipmaps = true`,
  `anisotropy = renderer.capabilities.getMaxAnisotropy()` (lettering is viewed at grazing angles),
  `minFilter = LinearMipmapLinearFilter`. GPU cost 4096×2048 RGBA ≈ 32 MB + mips ≈ 43 MB per
  variant, so cap live variants at 3 (cache by `variantId`). Unit numbers do **not** go into this
  canvas (see 3.4).

Helper (car-space → canvas px), the only place mirroring is handled:

```ts
// side: "L" | "R"; pctL: 0 = front bumper, 1 = rear bumper; z: metres above ground
function sidePx(side: "L" | "R", pctL: number, z: number, W = 4096, H = 2048) {
  const bandW = W / 2;                         // one side = half the canvas width
  const x0 = side === "L" ? 0 : bandW;
  const t = side === "L" ? pctL : 1 - pctL;    // right side: reader's left is the car's rear
  const zMin = 0.15, zMax = CAR_H;             // must match foundation's v mapping
  const y = H - (H / 2) * ((z - zMin) / (zMax - zMin)); // bottom half, canvas y down
  return { x: x0 + t * bandW, y };
}
```

### 3.2 Draw order per variant
1. Fill both side bands and the top band with the **base colour**.
2. **Second-tone regions** as polygons in car space (`%L`, z) pushed through `sidePx` per point.
   Door rectangles, the diagonal split and the crease split are just point lists. Draw each region
   once per side **using the same car-space polygon**. The mapping flips it geometrically, which is
   correct (the white door is always at 32–52 %L).
3. Stripes: polylines in car space, stroked with `lineWidth = height_m · px_per_m_v`. Chevrons:
   polygons. Battenburg: loop over blocks in car space.
4. Emblem (vector path in a unit circle, drawn at an anchor with a uniform scale and **no flip**).
5. **Text** last (§3.3). A 0.006 m outline = `strokeText` first (`lineJoin = "round"`,
   `lineWidth = 2·outline_px`), then `fillText`.
6. Top band: hood/roof/trunk tones and stripes (e.g. harrier twin stripes) in (x, %L) space.
7. **Bleed:** extend every region 8 px past the band edges and past region boundaries that meet
   UV seams. Pad 16 px between atlas bands with the base colour so mip levels don't bleed the
   neighbouring band in.

### 3.3 Avoiding mirrored text
- Text is anchored, not mapped: compute the anchor with `sidePx(side, pctL, zBaseline)`, then
  `ctx.save(); ctx.translate(x, y); ctx.textAlign = "center"; ctx.textBaseline = "alphabetic";
  ctx.fillText(str, 0, 0); ctx.restore();`. **Never** apply `scale(−1, 1)` to text. Because the
  atlas convention already makes left→right correct from outside on both sides, plain `fillText`
  reads correctly on both doors.
- Only *directional graphics* depend on the car's direction: hockey-stick stripe ends, chevrons
  that point forward, italic slant. Build the graphics in car space so the mapping flips them. For
  **italic text** (harrier), lean the glyph tops **toward the car's front** on both sides so it
  looks fast. Apply `ctx.transform(1, 0, c, 1, 0, 0)` after translating to the anchor (canvas
  y points down, so tops move by −c·h). Left side (front = reader's left): `c = +0.16`, tops lean
  left. Right side (front = reader's right): `c = −0.16`, ordinary italic. Use a skew, not
  `font-style: italic`, so both directions are possible.
- Glyph height in px is `cap_m × px_per_m_v`. Glyph width is controlled independently by
  `ctx.scale(sx, 1)` (condensed 0.8, extended 1.25), since the band's px/m differs between u and v.
  Correct for the anisotropy: `sx_total = sx_style · (px_per_m_u / px_per_m_v)` (e.g. 400 / 600
  → 0.67) with font size = `cap_m · px_per_m_v / 0.72` (cap-height ≈ 0.72 em). Otherwise the
  letters stretch on the car.
- Roof aerial numbers: draw in the top band rotated so glyph tops point to the car's front (reads
  from behind). Rear/trunk text: the trunk-face UVs belong to the top band or a rear band. Draw
  upright as seen from behind.
- Verify with a test livery: write `L FRONT →` / `R FRONT →` arrows plus the string `ABC123` at
  10 %L on both sides, and screenshot both sides once the foundation lands.

### 3.4 Fonts and per-unit strings
- Font: the browser default stacks differ per OS. Bundle **one OFL-licensed condensed sans** (e.g.
  any SIL-OFL condensed grotesk) as `woff2` under `apps/testbed-web/public/fonts/`, load it with
  `await document.fonts.load("700 64px PVPDCondensed")` **before** drawing, and fall back to
  `"Arial Narrow", "Roboto Condensed", sans-serif` with `scale(0.82, 1)`. Weight 700–800, tracking
  +3–5 % (`ctx.letterSpacing = "0.04em"` where supported, else draw per-glyph).
- Unit numbers (`C-12`, roof `C12`) vary per car, so do not bake them into the shared canvas.
  Option A (preferred): 3 small `decal_carrier` quads per car (rear quarter L/R + roof) sampling a
  shared **number atlas** canvas 1024 × 1024 with a 16 × 8 grid of pre-rendered unit strings,
  cached by string. Option B: a per-car 1024 × 512 canvas only for numbers if decal carriers
  aren't available. A decal carrier is a `surface_patch` 0.002 m off the loft, so it dents with its
  panel.
- Damage: scorch/scrape masks multiply over the livery in the shader, so the canvas never needs
  redrawing on damage. Detached panels keep their UVs, so lettering stays on the fallen door.
- Ex-fleet / auction theme (`ex_fleet_bw`, `decal_ghost`): redraw the same variant with text alpha
  0.12 and darker by 6 %, the "ghosted" outline where decals were peeled.

---

## 4. Quick reference

### 4.1 Colour tokens
`bw_black #111317`, `bw_white #F3F4F6`, `warden_white #EEEEE8`, `hp_tan #CDBB8E`,
`hp_umber #2B241C`, `hp_gold #D9AA3A`, `suv_navy #14223F`, `suv_cyan #1E7FD0`,
`suv_green #2FA15A`, `batt_yellow #D4F21A`, `batt_blue #1A3FB8`, `pursuit_blue #1C6CFF`,
`ghost_grey #2E3136` / text `#5A5F66`. Light lenses: red off `#5A0E14` / on `#FF2040`, blue off
`#0E1C5A` / on `#2060FF`, amber on `#FFA010`, clear on `#FFF6E0`.

### 4.2 Light-bar types
| Id | Era | Dimensions (L × D × H) | Content |
|---|---|---|---|
| L-GUMBALL | 60s–70s | Ø0.20 × 0.22 dome on Ø0.24 × 0.04 base | 1 rotator |
| L-80s | 80s–90s | 1.22 × 0.30 × 0.17 total (domes 0.13) | 2 domed rotators + clear centre takedowns + end alley lamps |
| L-2010 | 2010s | 1.22–1.37 × 0.30 × 0.055 (+0.03 feet) | 12 LED modules + 2 takedowns + 2 alley/corner + optional rear amber |
| L-MINI | 2010s | 0.95 × 0.25 × 0.05 | 8 modules, sports variant |
| L-SLICK | 2010s | none on the roof | visor + grille + mirror + rear-window + spoiler LEDs |

### 4.3 Unit-number formats
| Car | Body string | Roof | Doctrine call sign |
|---|---|---|---|
| cruiser | `C-12` | `C12` | Central-12 |
| warden | `C-44` (40–59 night blocks) | `C44` | Central-44 |
| wolfhound | `L-L1` | `LL1` | Loop Lima-1 |
| bastion | `S-B3` | `SB3` | South Bravo-3 |
| harrier | `C-H1` | `CH1` | Central Hotel-1 (**proposed**) |
| helicopter | `SKYWATCH 1` on the tail boom, 0.40 m; belly `PVPD` | | Skywatch One |

### 4.4 Emblem
A lighthouse silhouette (tapered tower 3 bands + lamp room + 3 light rays) inside a ring, with
`PORT VESPER` around the ring's top arc at 0.12 × ring diameter and `PVPD` along the bottom.
One colour line art only, no star, shield or eagle, so it can never be mistaken for a real seal.

---

## Sources
- [Emergency vehicle lighting (Wikipedia)](https://en.wikipedia.org/wiki/Emergency_vehicle_lighting): beacon history, takedown/alley/traffic advisor, wig-wag, colour conventions, 10–20 Hz photosensitivity note
- [Police car (Wikipedia)](https://en.wikipedia.org/wiki/Police_car): slicktop/ghost, push bars, spotlights, partitions, MDT
- [Aerial roof markings (Wikipedia)](https://en.wikipedia.org/wiki/Aerial_roof_markings): roof/hood placement, ≥ 40 cm glyphs (German guide)
- [Jalopnik: when did police cars get lights](https://www.jalopnik.com/2269297/when-did-police-cars-get-lights/): rotator → 70s roof-width bar with 2 rotators + mirrors
- [Full-size LED light-bar spec sheet (Dana Safety Supply)](https://danasafetysupply.com/products/14840/view): LED bar dimensions (2–2.25 in tall, 44–70 in long, 12 in deep)
- [67 Pa. Code § 173.3](https://law.cornell.edu/regulations/pennsylvania/67-Pa-Code-SS-173-3): 60–260 flashes/min
- [Lakeside: buyer's guide to battenburg](https://www.lakesidegroup.co.uk/blog-a-buyers-guide-to-battenburg-livery-markings/) and [NRSPP: High Conspicuity Livery for Police Vehicles](https://www.nrspp.org.au/resources/high-conspicuity-livery-police-vehicles): two-row blocks, fluorescent + retroreflective, ~600 × 300 mm
- [WMAZ: white-on-white lettering](https://www.13wmaz.com/article/news/local/verify-is-it-legal-to-have-white-on-white-lettering-on-cruisers/93-20412500-b18c-45a7-9f43-b4a6feca62d4) (agency name each side + back, ≥ 4 in) and [Ohio Admin. Code 311-3](https://codes.ohio.gov/ohio-administrative-code/chapter-311-3) (sheriff markings, reflective, trunk letters 3 in)
- [NFS wiki: Hot Pursuit (2010) career](https://nfs.fandom.com/wiki/Need_for_Speed:_Hot_Pursuit_(2010)/Career): fictional county PD framing in racing games
- WCAG 2.3.1 (three flashes or below threshold): accessibility cap for scene-light flashing
