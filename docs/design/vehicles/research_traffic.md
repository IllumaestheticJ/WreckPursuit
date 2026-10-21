# Traffic variety — research + spec (wave 1, research-traffic)

Input for wave-2 `style-traffic` (`tools/assets/src/styles/traffic.rs`, `apps/testbed-web/src/render/traffic.ts`).
Archetypes only; no makers, models, badges, real taxi companies, courier brands or real cities.

---

## 0. What exists today (repo facts)

- `scene.ts::updateAgents` draws **every** civilian agent as one `InstancedMesh(BoxGeometry(1,1,1))`
  scaled to `AGENT_SCALE = 1.8 × 4.4 × 1.4 m`, colour = debug state (yielding blue / brake red /
  physical yellow / else grey `#b8bcc4`). `MAX_AGENTS = 1024`, `frustumCulled = false`.
  It allocates a `new THREE.Vector3` per agent per frame (fix while in there).
- `wp_wasm::traffic_agents()` emits 5 floats per agent `[x, y, z, yaw, flags]`; `flags` bits:
  0 physical, 1 brake, 2 hazard, 3 yielding, 4/5 turn L/R, 6 aggressive, **8–15 `variant` (u8)**.
  `wp_world::traffic` sets `variant = rng.below(8)` at spawn — a stable per-agent visual seed is
  already available; it is just not used by the renderer. Agent `id` (stable, never reused) is not exported.
- Three.js is `0.180.0` → `InstancedMesh.setColorAt`, `BatchedMesh` (with per-instance colour
  and `setGeometryIdAt`) and `InstancedBufferAttribute` are all available.
- `tools/assets/src/profile.rs::BodyProfile` measures anchors from the lattice
  (`y_fw` firewall, `y_bf` hood front, `z_hood`, `z_belt`, `roof (y0,y1,z,half_w)`, `y_deck`,
  `z_deck`, `trunk_lid`, `rear_glass`, quarters, doors, arches). Fixed constants today:
  `END_ROUND_M = 0.35`, plan rounding 0.08 m, end belt drop 0.10 m, sill chamfer 0.06 m, glass
  slopes linear from roof edge to belt. `BodyStyle` knobs must modulate these, not replace the
  lattice anchors (panels must stay on their lattice regions).

Traffic cars and their data (`data/cars/*.json`, `body_size_m` = W × L × H):

| id | class | analog_of (abridged) | W × L × H m | WB m | doors |
|---|---|---|---|---|---|
| debug_sedan | Compact | 2010s mid-size FF family sedan | 1.80 × 4.70 × 1.45 | 2.70 | 4 |
| junkyard_beater | Beater | late-80s compact FF sedan, high mileage | 1.66 × 4.30 × 1.38 | 2.50 | 4 |
| tansy_13 | Compact | 2000s FF supermini 5-door | 1.67 × 3.85 × 1.50 | 2.46 | 5 |
| furrow_24 | Truck | 90s compact RWD pickup, regular cab | 1.69 × 4.75 × 1.60 | 2.85 | 2 |
| hauler_1500 | Truck | 2010s full-size half-ton crew cab | 2.03 × 5.85 × 1.88 | 3.645 | 4 |
| drayman_cargo | Truck | 2000s full-size RWD cargo van | 2.01 × 5.69 × 2.08 | 3.43 | 2 + sliders/rear |

---

## 1. What the reference games do (cheap-variety techniques)

| Game | What they do | Take-away for us |
|---|---|---|
| GTA V | ~250 vehicles total but each zone streams a small "popgroup" (VEH_POOR / VEH_MID / VEH_TAXI…); in practice 5+ identical cars at one junction is a common complaint. Colour from per-model `carcols`/`carvariations` lists (1–4 paint slots + pearl + livery + extras such as roof racks); liveries for taxi/delivery/rental are the *same mesh* with a decal set; per-instance dirt level; mods "fix" repetition by de-duplicating nearby spawns. | Per-model **allowed colour list** (not global random) + **extras toggles** + **dirt scalar**. Add a renderer-side "don't repeat the same model+colour within N neighbours" rule. |
| Burnout Paradise | Fictional traffic set of ~30–40 body types (hatchback, classic sedan, contemporary sedan, city SUV, rural SUV, MPV, pickup, farm truck, station wagon, taxi, city/rural bus, box/tow/tanker/garbage/cement trucks…). One van body covers 6 traffic versions (normal, rescue, repair, TV broadcast, camper, sponsor) via livery + roof props. | A small body set × livery/prop variants reads as a big roster. Heavy vehicles (bus, box truck) give the skyline of traffic its rhythm. |
| NFS MW 2005 / 2012, Heat, Unbound | Licensed hero cars, but traffic is mostly a dozen-odd generic/unbranded bodies (sedan, hatch, SUV, pickup, box van, bus, semi, taxi, cement truck) with modest colour sets; night cities sell variety through **lights** (head/tail, cab-sign glow) and wet reflections rather than geometry. Traffic is low-poly and fully crushable at rigid-body level. | At night the silhouette + lamp layout is what players read. Spend triangles on lamps & glass outline, not on panel detail. |
| Forza Horizon 4/5 | Traffic pool drawn from the licensed roster in muted "factory" colours; density zone-based (urban vs rural mix: more pickups/vans rural). | Make the mix **zone-weighted** (downtown vs industrial vs suburb). |
| Midnight Club LA (RAGE) | Licensed traffic, density by time of day (light at night, heaviest evening); taxis and police share one body. | Density curve by clock; reuse police sedan body for taxi archetype (we use `debug_sedan`). |
| Driver San Francisco | Very dense licensed traffic of mostly unexciting sedans/vans in period colours, LOD'd aggressively since the camera sweeps the city in Shift mode. | Far LOD can be very crude (box + lamp quads) if lamp positions are right. |

Common recipe: **~6–12 base bodies, 2–6 visual variants each, per-model colour tables, livery
decals, roof props, dirt/rust scalar, 3–4 LODs, lights carry the night.**

---

## 2. Real-world composition (US-style city) and recommended mix

Sources (rounded): US new sales 2024 ≈ SUV/crossover 58 %, pickup 18 %, car 19 %, van ≈ 5 %;
trucks+SUVs ≈ 75 %. But the **fleet on the road is older** (average age 12.6–12.8 yr; cars average
14.5 yr, light trucks 11.9 yr; cars still ≈ 1/3 of vehicles in operation), and a night downtown
skews toward sedans, taxis/ride-hail, and delivery vans. Grey-scale colours dominate trucks (≈ 83 %).

Recommended **night city (downtown/arterial)** body-type weights — sums to 100:

| Type | % | Source body | Notes |
|---|---|---|---|
| Mid-size sedan (family / fleet / rental) | 19 | debug_sedan | backbone of night traffic |
| Taxi / ride-hail-marked sedan | 7 | debug_sedan | downtown only; 2 % in suburbs |
| Compact crossover / small SUV | 16 | tansy_13 (raised) | most common new body type |
| Supermini / hatchback 5-door | 8 | tansy_13 | |
| Old compact sedan (beater) | 6 | junkyard_beater | 10 % in poor districts |
| Wagon / estate | 3 | debug_sedan (wagon roof) | |
| Minivan / MPV | 5 | drayman_cargo (short, windows) or tansy_13 stretched | see §3 caveat |
| Full-size pickup | 10 | hauler_1500 | 18 % suburbs/industrial |
| Compact pickup (old) | 4 | furrow_24 | |
| Full-size SUV | 6 | hauler_1500 (closed bed → SUV variant) | |
| Cargo / delivery van | 9 | drayman_cargo | 15 % industrial, double-parked downtown |
| Passenger van (windowed) | 2 | drayman_cargo | shuttle / church / hotel van |
| Box truck / bus | 5 | **needs new body** (not in roster) | strongly recommended for skyline variety; until then fold into van |
| **Total** | 100 | | |

Zone modifiers (multiply then renormalise): industrial ×2 vans/pickups/box, ×0 taxi;
suburb ×1.5 crossover/minivan/pickup, ×0.3 taxi; poor district ×2 beater/compact pickup, ×0.5 new crossover;
late night (01–05 h) ×2 taxi, ×0.5 box/bus.

**Per-model physics mapping** (what the sim picks, render variant chosen inside): debug_sedan 29 %,
tansy_13 26 %, hauler_1500 16 %, drayman_cargo 16 %, junkyard_beater 6 %, furrow_24 4 %, (box/bus 3 % pending).
Anti-repetition: when choosing a variant/colour, reject the pick if the same `(model, variant, colour)`
is among the last 6 spawns (renderer can enforce it cheaply with a ring buffer keyed by spawn order).

---

## 3. BodyStyle knob values per traffic car

Knob units assumed (wave-1 foundation may rename; keep the meaning):
heights/lengths **m**; angles **deg**; `*_frac` 0–1; "rake" = degrees back from vertical
(windshield 0° = vertical bus screen, 60° = supercar). Tumblehome = inward lean of side glass from
vertical, deg. Default = today's shape. Positions in detail lists: **x % = from front bumper
(0) to rear bumper (100) of body length L**, **z % = from ground (0) to roof top (100) of H**,
**lateral** in m from centreline (±). Sizes W × L × H m (W across car, L along car).

### 3.1 debug_sedan — 2010s mid-size FF family sedan (L 4.70, H 1.45)

| Knob | Value |
|---|---|
| silhouette | Notchback |
| nose_height / nose_rake | 0.68 m / 28° (soft wedge, pedestrian-safety nose) |
| nose_plan_round | 0.45 m (rounded corners) |
| hood_crown h / w | 0.03 m / 1.2 m; power_bulge none; scoop none |
| windshield_rake | 62° (fast, modern) |
| a_pillar_rake / c_pillar_rake | 62° / 58° (semi-fastback C-pillar flowing onto short deck) |
| roof_len / roof_height_offset | 1.25 m / 0 |
| tumblehome | 14° |
| crease height / depth | 0.88 m (shoulder line) / 0.012 m; lower door scallop 0.45 m / 0.008 m |
| haunch | 0.015 m |
| arch flare / shape | 0.01 m / round |
| ride_height_offset | 0 |
| tail_kamm / ducktail | 0.02 m integrated lip |
| overhang trim F / R | 0 / 0 |
| beltline_kick | 0.04 m rise from B- to C-pillar |

Details (all `beveled_box`/`surface_patch` unless noted):
| Detail | x % | z % | Size W × L × H m | Material |
|---|---|---|---|---|
| Grille upper slot | 1 | 48 | 0.85 × 0.04 × 0.10 | grille |
| Lower intake | 1 | 25 | 1.10 × 0.05 × 0.16 | grille |
| Headlamps (×2, swept) | 3 | 50 | 0.42 × 0.30 × 0.12, at ±0.62 | emissive white/glass |
| Fog lamps (×2) | 2 | 22 | 0.12 × 0.03 × 0.06, ±0.65 | emissive (off) |
| Mirrors (×2) | 33 | 66 | 0.08 × 0.20 × 0.12, ±0.95 | paint |
| Door handles (×4) | 42 / 60 | 60 | 0.02 × 0.14 × 0.03 | paint/chrome |
| Chrome window surround | 35–76 | 70 | 0.015 strip | chrome |
| Taillamps (×2, wrap) | 98 | 63 | 0.45 × 0.25 × 0.12, ±0.62 | emissive red |
| Third brake lamp | 84 | 78 | 0.25 × 0.02 × 0.03 | emissive red |
| Plate recess F / R | 0.5 / 99.5 | 32 / 45 | 0.52 × 0.01 × 0.12 | decal_carrier |
| Exhaust tip | 99 | 15 | Ø0.07 × 0.10, at −0.45 | exhaust |

### 3.2 junkyard_beater — late-80s compact FF sedan (L 4.30, H 1.38)

| Knob | Value |
|---|---|
| silhouette | Notchback (boxy three-box) |
| nose_height / nose_rake | 0.72 m / 10° (blunt, near-vertical grille) |
| nose_plan_round | 0.10 m (square corners) |
| hood_crown | 0.01 m (flat) |
| windshield_rake | 54° |
| a / c pillar rake | 54° / 30° (upright C-pillar, formal roof) |
| roof_len / height_offset | 1.40 m / +0.02 m |
| tumblehome | 8° |
| crease | 0.80 m / 0.006 m (single flat waistline) + rub strip (detail) |
| haunch | 0 |
| arch flare / shape | 0 / box-ish (shape = box, amount 0) |
| ride_height_offset | −0.02 m rear (sagging springs; −0.01 front) |
| tail_kamm / ducktail | 0 |
| overhang trim F / R | +0.03 / +0.05 m (5-mph bumpers stick out) |
| beltline_kick | 0 |

Details:
| Detail | x % | z % | Size m | Material |
|---|---|---|---|---|
| Rectangular grille | 0.5 | 50 | 0.70 × 0.03 × 0.14 | grille |
| Square sealed-beam lamps (×2) | 0.5 | 50 | 0.30 × 0.04 × 0.14, ±0.52 | emissive white |
| Amber corner lamps (×2) | 1.5 | 50 | 0.05 × 0.10 × 0.14, ±0.76 | emissive_amber |
| Black bumper bars F/R | 0 / 100 | 30 | 1.62 × 0.12 × 0.15 | rubber/plastic (dark) |
| Side rub strip | 15–88 | 42 | 0.02 × L-span × 0.04 | dark plastic |
| Drip rails | 35–75 | 92 | 0.015 strip | chrome (dull) |
| Mirrors (small) | 34 | 64 | 0.06 × 0.12 × 0.09 | black |
| Taillamps (horizontal band) | 99.5 | 60 | 0.55 × 0.03 × 0.14, ±0.50 | emissive red |
| Hubcaps / steel wheels | — | — | WheelStyle steel+cap | — |
| Antenna (whip) | 30 | 72→+0.6 m | Ø0.006 × 0.6 | chrome |

### 3.3 tansy_13 — 2000s supermini 5-door (L 3.85, H 1.50)

| Knob | Value |
|---|---|
| silhouette | Hatch |
| nose_height / rake | 0.70 m / 30° |
| nose_plan_round | 0.40 m |
| hood_crown | 0.02 m; hood short, sloping |
| windshield_rake | 60° (one-box-ish line from hood) |
| a / c pillar rake | 60° / 12° (near-vertical hatch) |
| roof_len / height_offset | 1.55 m / 0 |
| tumblehome | 12° |
| crease | 0.85 m / 0.008 m rising to tail lamps |
| haunch | 0.01 m |
| arch flare / shape | 0.005 m / round |
| ride_height_offset | 0 |
| tail_kamm | 0.03 m (hatch spoiler lip above glass) |
| overhang trim | 0 / −0.02 m |
| beltline_kick | 0.05 m at rear-door window |

Details:
| Detail | x % | z % | Size m | Material |
|---|---|---|---|---|
| Swept teardrop headlamps | 4 | 52 | 0.38 × 0.30 × 0.15, ±0.55 | emissive white |
| Smile lower grille | 1 | 28 | 0.80 × 0.04 × 0.12 | grille |
| Mirrors | 30 | 64 | 0.07 × 0.16 × 0.10, ±0.88 | paint |
| Tall vertical taillamps | 99 | 64 | 0.08 × 0.18 × 0.30, ±0.70 | emissive red |
| Hatch roof spoiler | 96 | 97 | 1.00 × 0.18 × 0.04 | paint |
| Rear wiper | 99 | 78 | 0.02 × 0.02 × 0.35 | black |
| Body-colour/black bumper strip | 0 / 100 | 32 | 1.60 × 0.10 × 0.06 | dark plastic |

### 3.4 furrow_24 — 90s compact RWD pickup, regular cab (L 4.75, H 1.60)

| Knob | Value |
|---|---|
| silhouette | Pickup |
| nose_height / rake | 0.92 m / 8° |
| nose_plan_round | 0.12 m |
| hood_crown | 0.015 m; flat hood, square edges |
| windshield_rake | 55° |
| a pillar / cab back | 55° / 0° (vertical cab-back wall, small rear glass) |
| roof_len / height_offset | 1.00 m / 0 |
| tumblehome | 6° |
| crease | 0.95 m / 0.010 m (full-length box line incl. bed) |
| haunch | 0 |
| arch flare / shape | 0.015 m / box (squared arches) |
| ride_height_offset | +0.02 m rear (unladen leaf rake) |
| tail_kamm | 0 |
| overhang trim | 0 / 0 |
| beltline_kick | 0 |

Details:
| Detail | x % | z % | Size m | Material |
|---|---|---|---|---|
| Chrome/black grille | 0.5 | 58 | 0.95 × 0.04 × 0.20 | grille |
| Square headlamps | 0.5 | 58 | 0.22 × 0.04 × 0.14, ±0.62 | emissive white |
| Steel front bumper | 0 | 35 | 1.66 × 0.14 × 0.16 | chrome |
| Bed rails (×2) | 52–98 | 66 | 0.06 × 2.05 × 0.03, ±0.80 | paint |
| Tailgate + handle | 100 | 50–66 | 1.55 × 0.04 × 0.42 | paint |
| Vertical taillamps | 99.5 | 58 | 0.04 × 0.08 × 0.24, ±0.80 | emissive red |
| Rear step bumper | 100 | 30 | 1.60 × 0.14 × 0.14 | chrome |
| Mirrors (tall rectangles) | 31 | 68 | 0.04 × 0.10 × 0.17, ±0.92 | black |
| Mud flaps (×2 rear) | 82 | 15 | 0.02 × 0.01 × 0.30 | rubber |

### 3.5 hauler_1500 — 2010s full-size crew cab (L 5.85, H 1.88)

| Knob | Value |
|---|---|
| silhouette | Pickup |
| nose_height / rake | 1.20 m / 5° (tall wall-face) |
| nose_plan_round | 0.15 m |
| hood_crown / bulge | 0.04 m / 1.4 m wide raised centre section, 0.03 m step |
| windshield_rake | 58° |
| a pillar / cab back | 58° / 2° |
| roof_len / height_offset | 1.75 m / 0 |
| tumblehome | 10° |
| crease | 1.05 m / 0.015 m sharp shoulder + lower 0.55 m line |
| haunch | 0.02 m bed-side bulge over rear arch |
| arch flare / shape | 0.03 m / box (squared, flat-topped) |
| ride_height_offset | +0.03 m rear |
| tail_kamm | 0.015 m tailgate spoiler lip |
| overhang trim | 0 / 0 |
| beltline_kick | 0 (flat, tall belt at 1.25 m) |

Details:
| Detail | x % | z % | Size m | Material |
|---|---|---|---|---|
| Huge grille | 0.5 | 55 | 1.30 × 0.05 × 0.45 | grille (chrome or black by variant) |
| Headlamps (C-shape) | 1 | 60 | 0.30 × 0.10 × 0.22, ±0.78 | emissive white |
| Chrome bumper | 0 | 30 | 1.95 × 0.20 × 0.24 | chrome |
| Tow hooks | 0 | 24 | 0.04 × 0.06 × 0.06, ±0.50 | caliper (red) |
| Running boards | 22–60 | 17 | 0.12 × 2.2 × 0.04, ±1.02 | black plastic |
| Towing mirrors | 26 | 66 | 0.06 × 0.25 × 0.22, ±1.15 | black |
| Bed rails / caps | 66–98 | 68 | 0.08 × 1.75 × 0.04, ±0.96 | black |
| Tailgate | 100 | 45–68 | 1.70 × 0.04 × 0.52 | paint |
| Tall taillamps | 99.5 | 55 | 0.05 × 0.10 × 0.40, ±0.95 | emissive red |
| Cab clearance lamps (×3) | 30 | 99 | 0.06 × 0.03 × 0.02 at −0.2/0/+0.2 | emissive_amber |
| Rear step bumper w/ hitch | 100 | 26 | 1.90 × 0.18 × 0.18 + Ø0.05 ball | chrome |
| Exhaust (side exit) | 82 | 14 | Ø0.09 × 0.15, at +0.85 | exhaust |

### 3.6 drayman_cargo — 2000s full-size cargo van (L 5.69, H 2.08)

| Knob | Value |
|---|---|
| silhouette | Van |
| nose_height / rake | 1.10 m / 15° (short "dog-house" hood) |
| nose_plan_round | 0.20 m |
| hood_crown | 0.03 m; hood length only ~0.9 m |
| windshield_rake | 48° |
| a pillar / rear | 48° / 0° (vertical barn/rear doors) |
| roof_len / height_offset | 3.90 m / 0 (box runs from B-pillar to tail) |
| tumblehome | 4° (cab glass only; box sides vertical) |
| crease | 1.05 m / 0.010 m; second line 0.60 m |
| haunch | 0 |
| arch flare / shape | 0.01 m / round |
| ride_height_offset | +0.02 m rear |
| tail_kamm | 0 |
| overhang trim | 0 / +0.03 m (rear bumper step) |
| beltline_kick | 0 |

Details:
| Detail | x % | z % | Size m | Material |
|---|---|---|---|---|
| Grille (wide) | 0.5 | 48 | 1.10 × 0.04 × 0.30 | grille |
| Headlamps | 1 | 50 | 0.28 × 0.08 × 0.18, ±0.75 | emissive white |
| Bumper (black) | 0 | 28 | 1.98 × 0.18 × 0.22 | black plastic |
| Side rub strip | 20–95 | 40 | 0.02 × 4.3 × 0.08 | black plastic |
| Sliding-door track | 35–60 | 72 | 0.02 × 1.4 × 0.03 (right side only) | black |
| Rear barn-door seam + hinges | 100 | 10–90 | vertical seam 0.01; 4 hinges 0.03 × 0.05 × 0.10 | black |
| Tall vertical taillamps | 99.5 | 45 | 0.05 × 0.10 × 0.45, ±0.95 | emissive red |
| Roof marker lamps | 99.5 | 98 | 3 × 0.05 × 0.02 × 0.03 | emissive red |
| Mirrors (big, two-glass) | 18 | 62 | 0.05 × 0.22 × 0.30, ±1.12 | black |
| Rear step bumper | 100 | 22 | 1.40 × 0.25 × 0.10 | black |

---

## 4. Visual-only variants (no new physics cars)

Variant is chosen from the existing `variant` byte (bits 8–15 of flags). Proposed encoding when
the sim side can widen it (roster agent owns `traffic_link`/`wp_world` hooks): **bits 8–11
variant (0–15), bits 12–15 colour slot seed**; until then use `variant % n_variants` and derive the
colour from `hash(variant, model, spawn index)`. Better: export agent `id` as a 6th float so
colour = `hash(id)` is stable across frames and despawn/respawn of other agents.

Each variant = a `CarStyle` override (`details` add/remove, `livery`, colour table) on top of the
base style. Geometry changes must stay within the body's lattice envelope (props on the roof are
attached to the Roof region and may detach; they do not change the collider).

| # | Variant | Base | What to change | Colour rule |
|---|---|---|---|---|
| S1 | Family sedan | debug_sedan | base | full civilian palette |
| S2 | Taxi | debug_sedan | roof sign box 0.80 × 0.28 × 0.22 m at x 45 %, emissive top text "TAXI" (lightbar_clear+emissive_amber); checker band (livery) at z 45 %, 0.10 m tall, 8-cell check, both sides; door number decal 0.25 × 0.12 m on front door; black steel wheels + caps | body yellow `#e8b923` 70 % / white 20 % / green-white two-tone 10 %; city-wide fixed per district |
| S3 | Ride-hail / livery car | debug_sedan | small windshield placard emissive (0.20 × 0.01 × 0.08 at x 30 %, z 72 %) | black/grey/white only |
| S4 | Rental / fleet plain | debug_sedan, tansy_13 | delete chrome trim (→ black), steel wheels + plastic covers, fleet number sticker 0.15 × 0.06 m on rear quarter | white 55 %, silver 30 %, grey 15 %; never metallic flake beyond 0.4 |
| S5 | Wagon / estate | debug_sedan | silhouette Wagon: extend roof to x 92 %, c-pillar 15°, extra D-pillar window, roof rails 0.03 × 1.6 × 0.05 at ±0.62, z 100 % | civilian palette, +brown/green muted |
| S6 | Unmarked-ish ex-fleet sedan | debug_sedan | black steelies, spotlight stub removed (hole cap 0.05 m), dark tint glass | white/black/dark blue; pairs with police archetypes for "is that a cop?" tension |
| B1 | Beater stock | junkyard_beater | base, dirt 0.5 | faded palette (§5) |
| B2 | Mismatched panels | junkyard_beater | per-panel colour override: pick 1–3 of {hood, one door, one fender/quarter, trunk} from the primer/faded list, rest body colour | body faded; panels primer grey `#7d7f7c`, primer red-oxide `#7a3b2a`, or another faded colour |
| B3 | Rusty | junkyard_beater, furrow_24 | rust decal patches on livery canvas: arch lips (0.25 m crescents above each arch), rocker bottom 0.08 m band x 25–80 %, trunk/tailgate lower edge; dirt 0.8; one hubcap missing (randomly) | faded palette |
| B4 | Tape & bodged | junkyard_beater | taped tail lamp (decal over lamp, amber-ish emissive 0.5×), bungee-tied trunk (2 cords Ø0.01), missing mirror one side | faded |
| H1 | Supermini | tansy_13 | base | civilian + bright accents (red/blue/yellow/green allowed ×2 weight) |
| H2 | Compact crossover | tansy_13 | ride_height_offset +0.08 m, arch flare 0.025 m box with black cladding (surface_patch around arches 0.06 m wide + rocker 0.12 m), roof rails, roof_height_offset +0.06 m, lower front skid plate (silver plastic 0.9 × 0.05 × 0.08 at x 0.5 %, z 15 %); wheel radius visual +0.02 | civilian palette, grey/blue up-weighted |
| H3 | City delivery hatch | tansy_13 | white, rear side windows blanked (paint-coloured panel), roof box 1.0 × 0.8 × 0.35 m (pizza/food delivery), emissive top sign | white 80 %, red 20 %; invented "WP Eats"-style generic word only ("DELIVERY", "PIZZA") |
| P1 | Old compact pickup | furrow_24 | base | faded + earth tones (tan/maroon/dark green) |
| P2 | Work pickup | furrow_24, hauler_1500 | ladder rack (2 hoops Ø0.04 tube over bed at x 60 % and 95 %, 0.45 m above rails, + 2 longitudinal bars; one aluminium ladder 0.45 × 3.0 × 0.08 on top), cross-bed toolbox 1.55 × 0.45 × 0.40 at x 63 % (diamond-plate chrome), amber roof beacon Ø0.12 × 0.10 (emissive_amber, off unless hazard flag), steel wheels | white 60 %, silver 20 %, red/yellow municipal 20 % |
| P3 | Lifted / off-road | furrow_24 | ride_height_offset +0.10 m, arch flare 0.04 m, light bar on cab roof 1.0 × 0.08 × 0.08, tube bumper | black/red/dark green |
| F1 | Crew-cab pickup | hauler_1500 | base | civilian, grey-scale 80 % |
| F2 | Chrome trim level | hauler_1500 | chrome grille + mirror caps + door handles, polished wheels | black/white/dark red/metallic blue |
| F3 | Full-size SUV | hauler_1500 | silhouette Suv: extend roof + glass to x 96 %, D-pillar 8°, close bed (tailgate becomes liftgate), rear quarter windows, roof rails; taillamps move to D-pillar | black 40 %, white 25 %, silver/grey 30 %, dark blue 5 % (livery "black car service") |
| F4 | Tonneau / bed cover | hauler_1500 | flat bed cover surface_patch at z 68 % over x 66–98 % | any |
| V1 | Cargo van plain | drayman_cargo | base | white 75 %, silver 10 %, other 15 % |
| V2 | Delivery livery | drayman_cargo | livery band: coloured lower 40 % of box sides + large generic word ("PARCEL", "EXPRESS", "FRESH") 0.35 m tall, roof ladder/rack optional | white body, accent band red/blue/green/brown; one invented carrier name per district |
| V3 | Trade van | drayman_cargo | roof rack (2 rails + 3 bars) with ladder, side decal "PLUMBING / ELECTRIC / LOCKSMITH" + fake phone number "555-01xx", dirt 0.4 | white 85 % |
| V4 | Passenger van / minivan stand-in | drayman_cargo | side windows full length (glass replaces box panel z 62–88 %), chrome bumpers, roof_height_offset −0.12 m (lower roof), rear windows in barn doors | white/silver/maroon/tan; shuttle livery ("AIRPORT SHUTTLE") 30 % |
| V5 | Ambulance-ish / utility | drayman_cargo | boxy roof module 1.9 × 3.2 × 0.30, amber/red marker lamps | white + red/orange stripe (non-emergency "medical transport", no star-of-life) |

Minivan caveat: a true minivan (one-box, 1.75 m tall, 5.1 m) sits between tansy_13 and drayman_cargo;
V4 is acceptable at night distance. If a 7th traffic physics car is ever added, make it the
minivan or a **box truck/city bus** (bigger visual payoff).

Damage: variants inherit the base lattice; props (roof sign, rack, ladder, toolbox, roof box)
should be detachables on the Roof/Trunk region with low break force so they fly off on impact —
cheap spectacle, the Burnout trick. Instanced (non-physical) agents carry no damage; when an agent
becomes `physical` the full mesh + lattice replaces the instance (existing LOD handoff), so the
variant and colour must be reproducible from `(model, variant, id)` on both paths.

---

## 5. Colour palette

Real-world anchors (new cars, global Axalta 68th report): white 38 %, black 19 %, grey 15 %,
silver 9 %, blue ≈ 7 %, red ≈ 6 %, others small. North America (BASF/iSeeCars style): white 25–29 %,
black 20–23 %, grey 20–23 %, silver 8–11 %, blue ≈ 9 %, red ≈ 7 %, rest ≈ 5 % (green, brown/beige,
orange, yellow, gold). Grey has been rising for a decade; silver falling — older (beater-era) fleet
was more silver/red/maroon/green/gold. Fleet/commercial vehicles are majority white (cheap single-stage
paint, lettering contrast, heat).

Base civilian palette (weights per 100; metal = MeshStandardMaterial `metalness`, rough = `roughness`
for the clear-coated base; add `clearcoat 0.6` on MeshPhysicalMaterial at LOD0 only if budget allows):

| Name | Hex | Weight | metal | rough |
|---|---|---|---|---|
| Glacier white (solid) | `#e9ebec` | 17 | 0.0 | 0.35 |
| Pearl white | `#f1efe8` | 8 | 0.15 | 0.28 |
| Black (solid) | `#0e0f11` | 12 | 0.0 | 0.30 |
| Black metallic | `#1a1c20` | 8 | 0.6 | 0.30 |
| Gunmetal grey | `#4a4e54` | 10 | 0.6 | 0.35 |
| Mid grey (solid, "cement") | `#8a8d8f` | 7 | 0.05 | 0.40 |
| Dark grey metallic | `#33363b` | 5 | 0.6 | 0.32 |
| Silver | `#b4b8bc` | 9 | 0.75 | 0.30 |
| Navy blue | `#1d2a48` | 5 | 0.5 | 0.32 |
| Bright blue | `#2456a8` | 3 | 0.5 | 0.30 |
| Red (solid) | `#a3141c` | 4 | 0.0 | 0.32 |
| Dark red / maroon | `#5a1418` | 3 | 0.5 | 0.33 |
| Dark green | `#1f3a2c` | 2 | 0.4 | 0.35 |
| Beige / champagne | `#b8a888` | 2 | 0.55 | 0.35 |
| Brown | `#4b3628` | 1.5 | 0.45 | 0.38 |
| Orange / yellow accent | `#d8741c` / `#e0b81c` | 1 / 0.5 | 0.2 | 0.32 |
| Teal / lime / purple accent | `#1f7a7a` / `#7fae2a` / `#4a2c6a` | 1 / 0.5 / 0.5 | 0.4 | 0.30 |
| **Total** | | 100 | | |

Faded / beater palette (junkyard_beater always, furrow_24 60 %, else 0): roughness +0.25 (clamp 0.85),
metalness ×0.3, saturation −35 %, value −10 %; plus specific fades:

| Name | Hex | Weight | metal | rough |
|---|---|---|---|---|
| Sun-faded red (oxidised) | `#9a4a44` | 16 | 0.05 | 0.70 |
| Faded maroon | `#5e3434` | 12 | 0.15 | 0.65 |
| Dull silver | `#9a9c9a` | 16 | 0.3 | 0.60 |
| Off-white / cream | `#d8d2c0` | 14 | 0.0 | 0.60 |
| Faded navy | `#3a4258` | 10 | 0.15 | 0.62 |
| Tan / gold-brown | `#9c8660` | 10 | 0.25 | 0.62 |
| Forest green faded | `#3c5040` | 8 | 0.15 | 0.65 |
| Powder blue | `#8aa2b8` | 6 | 0.2 | 0.60 |
| Dull black | `#252525` | 8 | 0.0 | 0.75 |
| Primer grey (panels only) | `#7d7f7c` | — | 0.0 | 0.90 |
| Red-oxide primer (panels only) | `#7a3b2a` | — | 0.0 | 0.90 |

Rules:
1. Each **variant** has its own colour table (see §4) referencing the palette names; fall back to the base table.
2. **Vans**: white ≥ 75 %. **Work pickups**: white 60 %. **Pickups/SUVs** generally: grey-scale (white/black/grey/silver) ≈ 80 %.
3. **Taxi** colour is fixed per district (players learn the city).
4. **Superminis/crossovers** may use accents ×2 weight; full-size SUVs/black-car service never use accents.
5. **Beaters** use the faded table, dirt 0.5–0.9, 35 % get mismatched panels, 30 % rust patches.
6. Jitter per instance: hue ±2°, value ±4 %, roughness ±0.04 — kills exact-duplicate look for free.
7. Night readability: no paint darker than `#0e0f11` (keeps silhouettes against black asphalt with
   reflections); cap `metalness` at 0.75 or cars vanish without an env map — make sure an
   env map (PMREM of the night sky + street-light probe) is set, else metallic paints render black.
8. Glass: dark tint `#0b0e12`, metal 0, rough 0.05; tyres `#151515` rough 0.9; black trim `#121314` rough 0.6;
   chrome `#d9dde0` metal 1.0 rough 0.12.

---

## 6. Instanced rendering for ~400 agents (Three.js 0.180)

Goal: ≤ 2× native traffic budget ⇒ render side should be < 1.5 ms CPU and ≈ 20–40 draw calls for
all traffic.

**Structure**
- One bucket per `(model, LOD)` — 6 models × 3 mesh LODs = 18 `InstancedMesh`es (+1 impostor) —
  or one `BatchedMesh` per LOD holding all model geometries (`addGeometry` once, `addInstance` per
  slot, `setGeometryIdAt` on LOD switch, `setColorAt`, `setMatrixAt`) → 3–4 draw calls total.
  BatchedMesh is the better fit once models × variants grow; start with InstancedMesh (simpler).
- **One material per bucket.** Merge all parts of a LOD into one geometry; bake a vertex attribute
  `aMat` (paint / glass / trim / chrome / tyre / lamp-front / lamp-rear / livery) and resolve
  colour/rough/metal in the shader (`onBeforeCompile`): paint = `instanceColor`, others = constants.
  Variant props (roof sign, rack, toolbox, roof box) live in the merged geometry with an
  `aVariantMask` bit; the vertex shader collapses masked-off vertices to zero (scale 0) based on a
  per-instance `iVariantBits` attribute → one geometry serves all variants of a body.
  Bodies whose silhouette changes (wagon S5, SUV F3, crossover H2, passenger van V4) are separate
  geometries (= separate bucket / BatchedMesh geometry id).
- Per-instance `InstancedBufferAttribute` (`vec4 iParams`): `x` variant bits, `y` dirt/rust 0–1,
  `z` lamp state (bit-packed brake / hazard / turn L / turn R / taxi sign on), `w` roughness/metal
  jitter. Lamps light from `iParams.z` + a global blink uniform; brake = rear lamp emissive ×4.
- Livery (taxi checker, delivery band, rust): one shared **atlas texture** (e.g. 2048², 16 cells),
  per-instance cell index in `iParams`; UV layout per §"Livery UV atlas" of PLAN.md. Rust/dirt =
  a tileable grime mask × `iParams.y`, no per-car texture.
- Keep buffers fixed-capacity (`MAX_AGENTS` per bucket, `count` = live). No allocations in the
  per-frame loop (reuse `Vector3/Quaternion/Matrix4`; current code news a `Vector3` per agent).
  Update only `instanceMatrix` every frame; `iParams`/colour only when an agent spawns or its lamp
  bits change (mark `addUpdateRange` for dirty ranges).
- Sort is not needed (opaque). Do a cheap CPU cull per agent (distance + view-cone dot) before
  writing it to a bucket, keep `frustumCulled = false` on the mesh (instance bounds are unknown).
- Wheels: at LOD0/1 put wheels in their own InstancedMesh keyed by wheel style (4 per agent,
  spin by speed estimate from position delta). At LOD2+ wheels are baked into the body.
- Lights at night: never add `THREE.PointLight`s per traffic car. Head-lamp pools = one additive
  decal quad on the road per car (instanced, LOD0–1 only, ≤ 60 m); far cars = emissive lamp quads
  + bloom. That is what sells a night city.

**LOD distances / triangle targets (per car, traffic budget 4k–10k at LOD0 per PLAN item 7)**

| LOD | Distance (m) | Triangles | Content |
|---|---|---|---|
| LOD0 | 0–35 | 4k–8k (hauler/van up to 10k) | full loft, all details, separate wheels w/ hubs, interior silhouette block |
| LOD1 | 35–100 | 1.2k–2k | loft at ½ stations, details > 0.1 m only, lamps as flat quads, 8-sided wheels |
| LOD2 | 100–250 | 250–500 | ~12-station hull with greenhouse, lamps as quads, wheels baked as 6-sided discs |
| LOD3 | 250–fog (≈ 600) | 12–40 | box + greenhouse wedge + 4 lamp quads (or camera-facing impostor card) |

Use hysteresis ±10 % on switch distances and pick LOD by **projected size** (distance × FOV
scale) so zoomed cameras keep detail. Expected counts with 400 agents in a city grid: ~15 at
LOD0, ~60 LOD1, ~150 LOD2, rest LOD3 ⇒ ≈ 15×6k + 60×1.6k + 150×400 + 175×30 ≈ 0.25 M triangles,
fine for WebGL2.

**Physical handoff**: agents flagged `physical` already get skipped (`skipPhysical`) and drawn by
the full `vehicle_view` path; that path must build the same style + variant + colour from the
same `(model, variant, id)` seed so a car does not change colour when it becomes physical.

---

## Sources

- Axalta 68th Global Automotive Color Popularity Report (white 38 %, black 19 %, grey 15 %, silver 9 %): https://www.tomorrowstechnician.com/axalta-releases-68th-global-automotive-color-popularity-report/
- Car colour popularity overview: https://en.wikipedia.org/wiki/Car_colour_popularity
- North America colour shares (BASF-based): https://www.insurance.com/auto-insurance/vehicle-shopping/most-popular-car-colors-of-2012.html , https://autosphere.ca/collision/2025/10/29/the-most-popular-car-colours/
- iSeeCars registrations / grey-scale on trucks: https://www.autoguide.com/auto/auto-news/ranking-the-most-popular-car-colors-in-america-44634832 , https://www.carpro.com/blog/the-most-popular-car-colors-in-america
- US sales by body type 2012–2024: https://datawrapper.dwcdn.net/cwlpE/1/ ; trucks+SUVs 75 %: https://theshopmag.com/news/suvs-trucks-sales-record-2024/
- Fleet age (S&P Global Mobility 2024 / 2025): https://press.spglobal.com/2024-05-22-Average-Age-of-Vehicles-in-the-US-Continues-to-Rise-12-6-years-in-2024%2C-according-to-S-P-Global-Mobility , https://press.spglobal.com/2025-05-21-U-S-Vehicle-Age-Rises-Again-to-12-8-Years-in-2025,-According-to-S-P-Global-Mobility
- Why fleet vehicles are white: https://www.knapheide.com/blog/why-most-commercial-vehicles-are-white/
- GTA V popgroups / duplicate traffic: https://el.gta5-mods.com/misc/v-popgroups-rewriten , https://www.gta5-mods.com/scripts/dynamic-traffic-colors , https://www.guinnessworldrecords.com/world-records/384857-most-vehicles-in-a-gta-videogame
- Burnout Paradise traffic set & van variants: https://steamcommunity.com/sharedfiles/filedetails/changelog/3027669849 , https://burnout.fandom.com/wiki/Traffic_(Burnout_Paradise)/Van
- Midnight Club LA traffic by time of day: https://en.wikipedia.org/wiki/Midnight_Club:_Los_Angeles
- NFS/FH/Driver SF notes are from general knowledge of the games (no primary source found for traffic model counts); treat as qualitative.
