# Mechanical visual geometry: engines, suspension, brakes, wheels

Wave-1 research for the `mechanical` wave-2 agent (`tools/assets/src/mechanical.rs`, `tools/assets/src/wheels.rs`).
Everything here is generic. There are no makers, models or brand names: "4-piston fixed caliper" rather than a
brand, and "big-block pushrod V8" rather than a named engine family. All sizes are in metres. Real dimensions are
rounded to what reads correctly at game camera distance.

## 0. Conventions and what the repo gives us

- **Vehicle space:** +X right, +Y forward, +Z up. `y_fa` / `y_ra` are the front/rear axle Y (`wheels[i].position_m[1]`).
  Ground is z = 0. The hub centre is at `z = radius_m` (loaded radius).
- **Wheel space** (as in `mesh.rs::wheel_parts`): origin at the hub centre, axle along X, Y/Z in the wheel plane. The
  current wheel is symmetric. Wave 2 must pass an **outboard sign** `s = sign(position_m[0])` (fl/rl are -X, fr/rr
  are +X) so that dish, spoke face, caliper bulk and lug nuts face the right way.
- **Clock angle θ** is measured in the wheel's YZ plane: θ = 0° points forward (+Y), 90° up, 180° rearward, 270° down.
- **Current state:** `WHEEL_SEGMENTS = 20`. The tire is 3 quads per segment (inner sidewall, tread, outer sidewall),
  so 120 tris. The rim is a flat 10-point star at `r_in = 0.64·R`, at ±0.6·hw on both faces, 40 tris. That makes
  **160 tris per wheel, 640 per car**. `TRI_BUDGET = (3000, 6000)` in `mesh.rs:16` (foundation will raise it to 8–20k / 4–10k).
- **The fixed `r_in = 0.64·R` is wrong for most of the roster.** Use the real bead radius instead:
  `rim_r = rim_in · 0.0127`. The real rim/R ratio ranges from 0.55 (70-series truck and muscle tires) to 0.74
  (40-series on 18"). See the table in §4.3.
- **Brake and tire sizes already exist in `data/cars/<id>.spec.md`** (tire size string, disc diameter, piston sizes,
  drum diameter). They are copied into the §3 and §4 tables. `mechanical.rs` should hard-code them per car id
  rather than parse the markdown.
- **Part kinds** (PLAN foundation §5): `engine` (rigid, one node), `suspension` (body-fixed), `brake_disc` (wheel space,
  spins), `caliper` (wheel space, steers, does not spin).
- **Recommendation:** treat the **upright/knuckle + lower damper half** as caliper-type parts (steer, no spin). Treat
  arms, springs and the upper damper body as body-fixed. Wheels move ±`travel_m` (0.12–0.26), and this split stops
  a visible gap opening at the arm/upright joint. Each body-fixed arm should end about 0.04 short of the upright,
  inside the wheel-arch shadow.

### How racing games present these parts (what to copy)

- **Sim showroom games:** the brake disc and caliper behind the spokes are the most-seen mechanical
  detail. Caliper colour is a customisation knob. Spokes are deliberately concave on big-brake cars, and discs show a
  lighter swept band with a dark vent gap at the edge. The engine bay is a separate mesh seen only with the hood open.
- **Arcade tuner games:** wheel customisation is mostly rim style, rim size, tire profile, lip depth and
  "stretch". At distance, the silhouette of the tire sidewall (tall vs. thin) does most of the work.
- **Demolition games:** engine, radiator and suspension are simplified solid shapes. They show
  when the hood or bumper comes off or a wheel detaches, and a dark block plus a coloured cam cover is enough. The
  damage model's visible parts fall off piece by piece.
- **Soft-body sim:** the rim and tire are separate flexbodies on hub/tire node groups. Brake torque
  is reacted between a disc-centre node and a caliper-arm node, which is the same split as our `brake_disc`
  (spins) / `caliper` (steers). Suspension arms are real meshes driven by beams.
- **For WreckPursuit:** wheels and brakes are always visible, so they get the detail. Suspension is visible in the
  arch gap and on wheel loss, so mid detail. The engine is seen only through scoops or glass, or once the hood is
  gone, so low-mid detail, but it needs a strong silhouette and colour.

---

## 1. Engines

### 1.1 Shared building blocks

| Primitive | Geometry | Tris |
|---|---|---|
| Block (per bank or inline) | beveled box | 44 |
| Head | beveled box sitting on the block, 0.02 wider each side | 44 |
| Cam/valve cover | beveled box, top bevel 0.015, in the cover colour | 44 |
| Coil pack / plug lead row | small box 0.05×0.04×0.07 per cylinder | 12 each |
| Oil pan | tapered box (sump deeper at the front for FR, at the rear of a transverse unit) | 12–24 |
| Round air cleaner | lathe, 16 seg × 4 pts | 96 |
| Throttle body / pipe | cylinder, 8–10 sides | 30–40 |
| Pipe run (exhaust, charge) | tube swept along 3–6 points, 6–8 sides | 12·sides·segs |
| Turbo snail | compressor: flattened cylinder plus a tangent outlet tube; turbine likewise; CHRA short cylinder | 150–220 per turbo |
| Radiator + fan shroud | box 0.03 deep plus a shallow ring | 24 + 48 |
| Intercooler (FMIC) | box plus 2 end-tank wedges plus 2 pipes | ~110 |

Materials: `engine` (dark metal) for the block, heads and pan. The cover colour comes from `caliper`-like tint or
`chrome` (see the per-engine notes). `exhaust` for manifolds and pipes, `chrome` for tips. Plastic covers and
airboxes use `trim`.

Heights below are absolute above ground for the roster's ride heights. Scale with `wheels[0].radius_m / 0.33` if a
car differs.

### 1.2 Engine types

**A. Big-block pushrod V8, 7.0–7.5 L, 90° V, carbureted** (`ironside_440`)
- Bare block 0.66 L(Y) × 0.70 W across the heads. Headers stick out to 0.80. Crank centre z = 0.40. Pan bottom z = 0.18.
- Banks: two boxes 0.62 long, 0.22 wide, 0.26 tall, rolled ±45° about Y, meeting at the crank axis.
- Valve covers: 0.58 × 0.13 × 0.07, chrome or a bright engine colour (orange-red, metallic blue or black).
  Offset 0.21 outboard of centre, z ≈ 0.72.
- Intake: a wedge 0.48 × 0.30 × 0.10 in the valley. On it, a 4-barrel carb box 0.15 × 0.15 × 0.09.
- **Air cleaner:** a round flat can 0.36 Ø × 0.075 tall with a chrome lid. The lid top sits at z ≈ 0.90 and fits
  under or through a hood scoop. Hero option: a domed lid and a centre wing nut (cylinder 0.03 Ø).
- Alternator, water pump and fan: water pump nose cylinder 0.10 Ø at the front face, a 4-blade fan disc 0.45 Ø ×
  0.03, and a radiator 0.65 W × 0.48 H at y_fa + 0.55.
- Exhaust: cast-log manifolds, 0.50 long × 0.06 Ø tubes per side at z ≈ 0.50, each turning down at the rear into a
  0.064 Ø (2.5") pipe. The pipes run dual, one per side, along the frame rails to transverse-oval mufflers
  (0.45 × 0.22 × 0.12) ahead of the rear axle, then kick over the axle and exit as **two round chrome tips 0.076 Ø
  turned down under the rear valance**, or as rectangular tips in the valance (hero option).
- Placement: longitudinal front-mid. Block front face at y_fa + 0.12, rear face at y_fa − 0.54. Bellhousing and
  gearbox continue back to y_fa − 1.35 (tapered box 0.30 → 0.18 W).
- Tris: hero ~1,300. Traffic/LOD ~350 (2 banks + 2 covers + cleaner + pan).

**B. Small-block pushrod V8, 4.8–5.7 L, 90° V** (`sentinel_cruiser`, `sentinel_bastion`, `hauler_1500`,
`drayman_cargo`, `sentinel_warden`)
- Block 0.62 L × 0.62 W (0.72 with manifolds) × height crank→intake top 0.50. Crank z = 0.40 (sedan) or 0.55
  (trucks/SUV).
- **Modern (2010s):** a composite intake manifold, low and wide (0.55 × 0.40 × 0.12), with the throttle body
  (0.09 Ø cylinder) facing forward and an air tube 0.10 Ø to an airbox box (0.35 × 0.25 × 0.20) in the front-left
  corner. Coil packs: 4 boxes per bank on the valve covers. Valve covers are black/dark grey. Optional plastic
  beauty cover over the intake (0.55 × 0.45 × 0.04, `trim`).
- **Old (`sentinel_warden`, 80s/90s):** throttle-body-injection base with a round air cleaner 0.33 Ø × 0.08, black
  or silver lid. Valve covers in a dull engine colour. Plug leads are a row of thin cylinders.
- Exhaust: a short cast manifold per side → Y-pipe or dual pipes 0.064 Ø.
  - Cruiser: dual 0.064 pipes to two mufflers and dual round tips 0.064 Ø turned down at the rear bumper.
  - Bastion/hauler: single 0.076 pipe, muffler 0.55 × 0.25 × 0.18 under the floor on the right side, single chrome
    tip 0.10 Ø exiting behind the right rear wheel or under the bumper.
  - Drayman: single pipe exiting before the right rear wheel, plain steel, no tip.
- Placement: longitudinal front-mid. Sedan front face at y_fa + 0.10, trucks/van at y_fa + 0.05.
  Drayman: engine partly under the cab floor (cab-forward van "doghouse"), front face at y_fa + 0.25, top z ≈ 1.00.
- Tris: hero ~1,000. Traffic ~250.

**C. Modern DOHC V8, 5.0 L, 90° V, 4 cams** (`rampart_50`, `sentinel_harrier`)
- Wider than a pushrod V8 because of the 4-valve heads: 0.70 L × 0.78 W × 0.66 H. Crank z = 0.36.
- Cam covers: 0.60 × 0.20 × 0.10, black crinkle or silver with a lighter rib. Each carries 4 coil-pack boxes
  (0.05 × 0.05 × 0.08) in a line.
- **Intake plenum:** a box 0.48 × 0.32 × 0.13 over the valley with 8 runner bumps on its sides. Front throttle body
  0.085 Ø. A long air snorkel 0.10 Ø leads forward-left to an airbox (0.32 × 0.22 × 0.18) behind the headlight.
- Hero: a plastic engine cover with two raised ribs (no lettering).
- Exhaust: tubular **4-into-1 headers per side** (four 0.042 Ø primaries, 0.30 long, merging into a 0.064 collector).
  An X- or H-crossover sits under the gearbox. Dual 0.064 pipes run to two rear mufflers in front of the bumper.
  - Rampart: **quad tips**, two each side, trapezoid/rounded-rectangle 0.12 × 0.065 in the diffuser.
  - Harrier: dual round tips 0.09 Ø, plain steel/black.
- Placement: longitudinal, front face at y_fa + 0.08, rear face at y_fa − 0.62.
- Tris: hero ~1,600 (headers are expensive; use 6-sided primaries). LOD ~400.

**D. Inline-4 NA, 1.3–1.6 L, transverse** (`vireo_xs`, `tansy_13`, `junkyard_beater`)
- Block **length along the crank (X) 0.46–0.52**, width (Y) 0.30, height to the cam cover top 0.58. Tilted rearward
  10–15° (cylinder axis leans toward the firewall). Crank z = 0.30.
- Gearbox/transaxle sits on −X: a lump 0.35 × 0.30 × 0.30 butting onto the block end. The timing-belt cover is a
  thin tall box on +X (0.04 × 0.25 × 0.45). Engine mounts are under the right/+X side.
- Cam cover 0.42 × 0.18 × 0.05.
  - **Vireo (two-stage cam, revvy): red wrinkle cam cover.**
  - Tansy: black plastic cover.
  - Junkyard: dull grey with a rust-orange tint.
- Intake on the firewall side: 4 curved runners (8-sided tubes 0.04 Ø), a plenum box and an airbox 0.30 × 0.20 ×
  0.15 in the corner.
- Exhaust on the radiator side: a 4-into-1 header with a heat-shield box. A single 0.045–0.050 pipe runs down,
  rearward under the floor tunnel, to a small round muffler (0.30 long × 0.18 Ø).
  - Single tip 0.06 Ø (Vireo: slightly larger, chrome, 0.08 Ø).
  - Junkyard: plain bent pipe, no tip, slightly sagging.
- Placement: transverse FF. Crank axis at y = y_fa + 0.05, wholly ahead of the firewall. Radiator at y_fa + 0.45.
- Tris: hero (Vireo) ~700. Traffic ~200.

**E. Inline-4 turbo, 2.0 L** (`kestrel_ts`, `lindwurm_4t`, `debug_sedan` transverse; `shiro_turbo` longitudinal)
- Same block as D, scaled: length 0.52–0.55, height 0.62.
- **Turbo snail:**
  - Compressor housing: flattened cylinder 0.14 Ø × 0.08 thick, axis along the crank. Inlet 0.065 Ø faces the
    airbox; outlet tube 0.055 Ø leaves tangentially.
  - Turbine housing: 0.12 Ø × 0.07 on the exhaust side.
  - Centre section: 0.06 Ø × 0.06. The whole unit hangs on the exhaust manifold at about cam-cover height − 0.15.
  - Material: turbine `exhaust` (heat-blued dark), compressor `metal`.
- **FMIC:** core 0.55 W × 0.18 H × 0.065 D centred behind the lower grille at z ≈ 0.30–0.48 and y ≈ front bumper −
  0.10. End tanks are wedges 0.05 each side. Charge pipes 0.057–0.063 Ø run from the compressor down through the
  bumper area to one tank, and from the other tank back up to the throttle body. Each pipe is ~5 swept points, with
  a blue/black silicone coupler band (`trim`). The visible FMIC is a strong tuner cue: always show it on Kestrel and
  Shiro (mesh visible through the bumper opening).
- **Blow-off/top-mount alternatives:** Lindwurm hides its intercooler (side-mount, a box in the right bumper corner)
  and uses a big plastic engine cover 0.50 × 0.35 × 0.05. Debug sedan: cover only.
- Shiro (longitudinal FR):
  - Engine block front at y_fa + 0.02. Tilted 0°; crank z = 0.30. The cam cover is red or black crinkle.
  - Turbo on the right/+X side, low. The downpipe runs back along the tunnel to the gearbox (y_fa − 0.40 →
    y_fa − 1.20).
- Exhaust, turbo cars: a 0.064–0.076 "downpipe" → mid resonator (cylinder 0.30 × 0.10 Ø) → rear muffler box
  (0.40 × 0.22 × 0.15).
  - Kestrel: single big tip, 0.10 Ø rolled-edge (tuner cue).
  - Shiro: dual 0.09 Ø oval tips.
  - Lindwurm: dual tips, one each side, 0.09 Ø, chrome.
  - Debug sedan: hidden tip.
- Tris: hero ~1,100 incl. FMIC and piping. Traffic (debug_sedan) ~250.

**F. Inline-6, 3.0–3.2 L, longitudinal** (`sperber_32` NA, `hakone_gt` twin-turbo, `debug_coupe` turbo)
- **The long head is the signature:**
  - Block 0.78 L × 0.30 W, height 0.66.
  - Cam cover 0.68 × 0.20 × 0.08 with 6 plug-coil bumps in a line (or a centre spine with 6 small boxes).
  - Canted 20–30° toward the intake side: Sperber 30° to the right, Hakone and debug coupe ~10°.
- Crank z = 0.33. Front face at y_fa + 0.18. Rear face at y_fa − 0.60, the engine reaching behind the front axle.
- Sperber (high-revving NA): **6 individual throttle bodies** on the left side, as 6 short cylinders 0.05 Ø × 0.06
  feeding a long airbox 0.62 × 0.18 × 0.14. Cam cover silver/grey with a coloured stripe in the `caliper` tint.
- Hakone (sequential twin-turbo):
  - Two snails on the right (exhaust) side, one in front of the other at y_fa + 0.05 and y_fa − 0.25, each
    compressor 0.12 Ø.
  - A big FMIC 0.62 × 0.20 × 0.075 with a crossover pipe.
  - A long intake plenum 0.60 × 0.16 × 0.14 on the left. Cam covers dark red or black.
- Debug coupe: single turbo variant of Hakone; FMIC hidden; plastic engine cover.
- Exhaust: 6-into-2 cast manifolds / tubular (hero: 3+3 primaries 0.038 Ø), dual 0.064 pipes, two rear mufflers.
  - Sperber: **quad round tips 0.08 Ø**, two per side.
  - Hakone: dual round 0.09 Ø on one side (left) or one each side.
  - Debug coupe: dual round 0.09 Ø.
- Tris: hero ~1,300. LOD ~350.

**G. Twin-turbo V6, 3.5 L, 60° V, transverse** (`sentinel_wolfhound`)
- Engine 0.52 along X (crank) × 0.62 Y × 0.62 H. Crank z = 0.38. Front bank leans toward the radiator, rear bank
  toward the firewall.
- Turbos: one per bank, hung low (z ≈ 0.30) at the bank ends, mostly hidden.
- The top is dominated by a black plastic cover 0.50 × 0.42 × 0.06 over the intake, plus an intercooler pipe to the
  front-right corner.
- Exhaust: single pipe → dual muffler, **dual round tips 0.076 Ø, plain steel, one each side** (police: no chrome).
- Tris: ~600 (police unit; the hood rarely comes off at a scale where detail matters).

**H. Flat-plane V8, 3.5 L, 90° V, mid-engine longitudinal with a transverse gearbox behind** (`lucciola_350`)
- Engine 0.60 L × 0.72 W × 0.52 H, dry sump so it sits low. Crank z = 0.28.
- Front face (to the cabin firewall) at y_ra + 0.95, rear face at y_ra + 0.35.
- Transverse gearbox behind the axle: a box 0.55 W × 0.30 L × 0.32 H at y_ra − 0.05 to y_ra − 0.35, with drive
  shafts 0.045 Ø out to the hubs.
- **Visible through louvres / rear glass:**
  - **Red wrinkle cam covers** 0.50 × 0.18 × 0.07 on each bank.
  - A central "inlet plenum" pair: two long boxes 0.45 × 0.12 × 0.10 with 4 runner bumps each, between the cam
    covers. Silver/aluminium.
  - Air intakes route from the side scoops ahead of the rear wheels via 0.12 Ø tubes.
- Exhaust: 4-into-1 per bank. Headers wrap down at the outer sides → a **transverse muffler box 0.80 W × 0.22 ×
  0.22** behind the gearbox (exposed, under the tail), and 2 or 4 round tips 0.08 Ø in the centre or at the two
  corners of the rear valance.
- Tris: hero ~1,400.

**I. V10, 5.2 L, 90° V, mid-engine longitudinal, AWD** (`fulmine_v10` NEW)
- Engine 0.69 L × 0.75 W × 0.55 H, dry sump. Crank z = 0.27. Front face at y_ra + 1.05, rear face at y_ra + 0.36.
- Longitudinal dual-clutch gearbox behind the axle: 0.50 L × 0.35 W × 0.30 H. A front driveshaft 0.06 Ø runs
  forward along the left side of the sump to a front diff box (0.20³) at y_fa.
- **Intake trumpets / airbox (signature):**
  - A carbon airbox shell over the valley (0.55 × 0.36 × 0.12, `carbon`) with two oval inlets.
  - Hero alternative with the lid off: **10 velocity stacks** (lathe 8 seg × 4 pts, 0.06 Ø mouth flaring to 0.075,
    0.08 tall) in two rows of 5 at the valley, z ≈ 0.80.
- Cam covers: 0.62 × 0.17 × 0.07, red, orange or black crinkle with a carbon centre strip. Everything shows under a
  glass engine cover with the bay lit, so give the bay a dark `trim` tub (box minus top).
- Exhaust: 5-into-1 per bank → twin rear silencers (boxes 0.35 × 0.20 × 0.20) at the rear corners → **4 large
  round/hex tips 0.10 Ø**, or 2 large trapezoid outlets 0.18 × 0.08, high in a mesh rear panel.
- Tris: hero ~1,800 (stacks ~800 of it; use the airbox lid as default and stacks as a mod).

### 1.3 Roster → engine map

| Car | Engine type | Layout | Cover colour cue | Exhaust exit |
|---|---|---|---|---|
| `ironside_440` | A big-block pushrod V8 + round chrome air cleaner | long. front-mid | orange-red or chrome covers | dual round, turned down, rear |
| `rampart_50` | C DOHC V8, plenum, coil packs | long. front-mid | black crinkle | quad trapezoid in diffuser |
| `sentinel_harrier` NEW | C DOHC V8 (no cover) | long. front-mid | black | dual round, plain |
| `sentinel_cruiser` | B small-block pushrod V8 (modern) | long. front-mid | black | dual round, turned down |
| `sentinel_warden` NEW | B small-block pushrod V8 (old, round cleaner) | long. front-mid | dull blue/black | single or dual turned down |
| `sentinel_bastion` | B small-block pushrod V8 (modern) | long. front-mid, high | black | single, behind right rear wheel |
| `hauler_1500` | B small-block pushrod V8 (modern) | long. front-mid, high | black | single chrome 0.10, right rear |
| `drayman_cargo` | B small-block pushrod V8 | long., partly under cab | black | single, no tip, before right rear wheel |
| `sentinel_wolfhound` | G twin-turbo V6 | transverse | black plastic cover | dual round, one each side |
| `vireo_xs` | D I4 NA | transverse FF | **red wrinkle cam cover** | single 0.08 chrome |
| `tansy_13` | D I4 NA | transverse FF | black plastic | single small, hidden |
| `junkyard_beater` | D I4 NA | transverse FF | grey, rust tint | bare pipe |
| `furrow_24` | D I4 NA, **longitudinal** (rotate 90°; SOHC, 2.4 L: length 0.55) | long. front-mid | silver/grey | single, before right rear wheel |
| `kestrel_ts` | E I4 turbo + **FMIC**, charge piping | transverse FF | red crinkle | single big 0.10 rolled tip |
| `lindwurm_4t` | E I4 turbo, hidden IC, big cover | transverse (AWD) | black cover | dual, one each side |
| `debug_sedan` | E I4 turbo, cover only | transverse FF | black | hidden |
| `shiro_turbo` | E I4 turbo + **FMIC** | long. FR | red/black crinkle | dual oval |
| `sperber_32` | F I6 NA, 6 ITBs, canted | long. front-mid | silver + tint stripe | quad round |
| `hakone_gt` | F I6 twin-turbo + FMIC | long. front-mid | dark red | dual round |
| `debug_coupe` | F I6 single turbo | long. front-mid | black cover | dual round |
| `lucciola_350` | H flat-plane V8 | mid long. + transverse box | **red wrinkle covers** | transverse muffler, centre tips |
| `fulmine_v10` NEW | I V10 + airbox/trumpets | mid long., AWD | carbon airbox, red covers | 4 large tips high |

---

## 2. Suspension

### 2.1 Primitives and sizes

All offsets are relative to the hub centre H = (x_h, y_h, z_h = radius_m). "In" = toward the car centre, i.e.
−s·Δx. Arm tubes are 6-sided, 0.025–0.035 Ø, or flat beveled boxes 0.06 × 0.025 for stamped arms.

| Element | Geometry | Size |
|---|---|---|
| Damper body | cylinder 8–10 sides | 0.045–0.055 Ø (strut 0.050–0.055; shock 0.045; race coilover 0.046 body / 0.05 threaded sleeve) |
| Damper rod | cylinder 6 sides, chrome | 0.018–0.022 Ø |
| Road coil spring | helix tube | mean Ø 0.13–0.16 (strut/OEM), wire 0.011–0.014, **4.5–6 active coils**, free length 0.25–0.33 |
| Coilover spring | helix tube | ID 0.060–0.065 (OD ~0.08–0.09), wire 0.010–0.013, **6–8 coils**, length 0.15–0.25, coloured (`caliper` tint or red/yellow) |
| Truck/SUV coil | helix | mean Ø 0.13–0.15, wire 0.016–0.018, 4–5 coils, length 0.30–0.40 |
| Top mount | cylinder disc | 0.13 Ø × 0.03 |
| Spring seats | thin discs | spring OD + 0.015 |
| Anti-roll bar | tube swept along 5–7 pts | 0.020–0.025 Ø, runs across the car at axle height ± 0.10, end links 0.012 Ø × 0.15 |
| Leaf spring | stack of curved flat strips | width **0.064 (2.5")**, leaf thickness 0.006–0.008, **4–6 leaves** (truck 3–5 + overload), main leaf length 1.40–1.60, arch 0.06–0.10 |
| Shackle | two flat plates | 0.07–0.11 long, 0.02 wide |
| Live axle tube | cylinder 8 sides | 0.075 Ø (car), 0.085 (½-ton truck), 0.095 (van) |
| Diff housing | sphere-ish lathe 10×5 + cover disc | 0.26–0.30 Ø, 0.20 deep, centre (or offset 0.10 to −X) |
| Torsion beam | U/box section | 0.09 × 0.06, spans the track minus 0.35 |
| Torsion bar (front, longitudinal) | 6-sided rod | 0.025 Ø × 0.90–1.10 along the frame rail, from the lower arm pivot rearward |
| Hub carrier / knuckle | beveled box | 0.10 × 0.06 × 0.22 (tall for SLA) |

Helix cost: `coils × segs_per_coil × tube_sides × 2`. A hero road spring is 5 × 10 × 4 × 2 = 400; prefer 3 sides →
**300**. Traffic: a single 8-sided cylinder in spring colour (≈30) with a darker inner cylinder. A cheap middle option
is a "ribbon" helix (two-sided flat strip, 5 × 10 × 2 = 100).

### 2.2 Layout templates (front = steerable axle)

**MacPherson strut (front)**
- Lower control arm (L-shape): outer ball joint at H + (in 0.10, 0, −0.10). Inner bushes at (in 0.42, ±0.15 Y,
  −0.12); arm length 0.32–0.38.
- Strut: damper body from the knuckle at H + (in 0.09, −0.02 Y, +0.08) up to the top mount at H + (in 0.17, −0.04 Y,
  +0.58), giving the ~10–13° KPI tilt and slight rearward caster.
- Coil spring: centred on the strut between z_h + 0.24 and z_h + 0.52, mean Ø 0.14, 5 coils. Top mount disc 0.13 Ø
  under the strut tower.
- Tie rod: 0.018 Ø from the knuckle arm at the back of the hub (H + (in 0.10, −0.12, 0)) inward 0.35.
- Rear strut variant (junkyard rear option): same, no tie rod; a trailing link instead.

**Double wishbone / SLA (front or rear)**
- Upper arm: short, 0.24–0.30, outer joint at H + (in 0.09, 0, +0.20…+0.26). Inner pivots at (in 0.34, ±0.12,
  +0.22).
- Lower arm: long, 0.40–0.48, outer joint at H + (in 0.08, 0, −0.11). Inner pivots at (in 0.52, ±0.18, −0.12).
- Upright: tall knuckle 0.36 from the lower to the upper joint.
- Spring/damper options:
  - **Coilover** from the lower arm at 0.6–0.7 of its length out, up to the frame at z_h + 0.38. Length 0.32; tilt
    the top inboard 10–20°. Typical of sports cars, exotics and the supercar.
  - **Coil between the arms** (old sedans/trucks): coil mean Ø 0.13, 4–5 coils, sitting on the lower arm at in 0.30,
    with a separate shock inside it.
  - **Torsion bar**: no front coil; a bar along the frame from the lower arm inner pivot (70s muscle, 90s compact
    pickup).
- Rear double wishbone: same with no steering; the toe link replaces the tie rod.

**Multilink rear**
- 4–5 links of 0.022–0.028 Ø, each 0.28–0.45 long, from a tall carrier (0.08 × 0.06 × 0.24):
  - upper lateral link at z_h + 0.18
  - lower lateral link at z_h − 0.09
  - trailing link from the carrier forward 0.40 at z_h − 0.04
  - toe link at z_h − 0.02, behind the axle
- Spring separate from the damper: a short barrel coil (mean Ø 0.12, 4 coils, 0.20 tall) on the lower link at
  in 0.25. Damper behind the hub at (in 0.08, −0.10, from z_h − 0.08 up to z_h + 0.36).
- Subframe: a flat box frame 0.08 × 0.05 tubes across the car at y_ra ± 0.20.
- A simplified 3-link version (lateral pair + trailing) is fine for mid-detail.

**Torsion beam rear (FF economy)**
- Trailing arms 0.45 long from a pivot at (in 0.12, y_h + 0.45, z_h + 0.02) back to the hub.
- Beam across at y_h + 0.22, z_h − 0.02, section 0.09 × 0.06, connecting both arms.
- Coil (mean Ø 0.11, 4–5 coils, 0.20 tall) on the arm just behind the beam. Damper behind the hub, slanted forward
  15°.

**Solid live axle, leaf springs**
- Axle tube along X through both hubs. Diff housing at the centre (z_h, y_ra), pinion nose pointing forward
  0.12 → driveshaft 0.075 Ø to the gearbox.
- Leaves under (car) or over (truck) the axle at x = ±(track/2 − 0.24):
  - Front eye at y_ra + 0.70.
  - Rear eye at y_ra − 0.70, with a shackle 0.09 hanging from the frame at z_h + 0.10.
  - Arch: the centre is 0.06–0.08 lower than the eyes.
  - Two U-bolts (0.012 Ø) clamp the leaves to a pad.
- Shocks: **staggered**, left ahead of the axle and right behind (or both forward-leaning ~20°). Body 0.045 Ø × 0.30.
- Muscle-car extra: keep the pure leaf layout; add traction bars (0.03 × 0.05 box, 0.40 long, under each leaf) only on the drag kit.

**Solid live axle, coil + links** (`sentinel_warden`, `sentinel_bastion`)
- Two lower trailing arms 0.50 long at z_h − 0.10 run forward to the frame.
- Two short upper arms (0.35) angled inward to the diff top, or a panhard bar (0.03 Ø × 1.2) across.
- Coils: mean Ø 0.14, 4.5 coils, 0.30 tall, sitting on the axle at x = ±(track/2 − 0.30). Shocks beside the coils.

### 2.3 Roster → suspension map

Spring/damper rates and travel are already in each JSON. Visible spring compression can stay static.

| Car | Front | Rear | Visible cue |
|---|---|---|---|
| `ironside_440` | SLA + longitudinal torsion bars | **live axle, leaf (5 leaves, 1.45 m), staggered shocks** | big diff pumpkin, rear drums |
| `rampart_50` | strut (twin lower links) | multilink IRS | big 6-piston caliper, wide carrier |
| `sentinel_harrier` NEW | strut | multilink IRS | as rampart, black parts |
| `vireo_xs` | double wishbone (90s FF) | double wishbone | red coil springs (tint) |
| `lindwurm_4t` | strut | multilink + rear driveshafts 0.04 Ø | AWD shafts visible |
| `kestrel_ts` | strut | multilink | coilover look on the AWD mod |
| `shiro_turbo` | strut | multilink | coloured coilovers (tuner, 0.062 ID spring, 7 coils) |
| `hakone_gt` | double wishbone | double wishbone | forged-look arms, silver |
| `sperber_32` | strut | multilink (5-link) | — |
| `debug_coupe` | strut | multilink | — |
| `lucciola_350` | double wishbone, coilovers | double wishbone, coilovers (2 per side optional) | exposed through the rear grille |
| `fulmine_v10` NEW | double wishbone, coilovers | double wishbone, coilovers | yellow/orange springs, carbon-look arms |
| `debug_sedan` | strut | multilink (simplified 3-link) | — |
| `junkyard_beater` | strut | torsion beam | rusty springs (dark brown tint) |
| `tansy_13` | strut | torsion beam | — |
| `furrow_24` | SLA + torsion bars | **live axle, leaf (4 leaves, 1.30 m), over-axle** | leaf hangers |
| `hauler_1500` | SLA, coil-over-shock | **live axle, leaf (3 + 1 overload, 1.50 m)** | tall ride, visible frame rails (box 0.08 × 0.18) |
| `drayman_cargo` | SLA, coil between the arms | **live axle, leaf (4 leaves, 1.55 m)** | heavy axle 0.095 Ø |
| `sentinel_cruiser` | SLA / strut hybrid (draw SLA) | multilink | — |
| `sentinel_warden` NEW | SLA, coil between the arms | live axle, coil + 4-link | frame rails, big diff |
| `sentinel_wolfhound` | strut | multilink + AWD shafts | — |
| `sentinel_bastion` | SLA coil-over-shock | live axle, coil + 5-link + panhard | frame rails |

Frame rails on body-on-frame cars (hauler, drayman, bastion, warden, furrow, ironside sub-frame): two boxes 0.07 W
× 0.15 H along x = ±(track/2 − 0.30) from y_fa + 0.6 to y_ra − 0.8. That is 24 tris and a lot of credibility when
seen under the car or after a wheel is lost.

---

## 3. Brakes

### 3.1 Disc geometry (wheel space, part `brake_disc`, spins)

- **Diameter:** from the spec table (§3.3). Fallback rule: **disc OD ≈ rim bead Ø − 0.09** for a fixed multi-piston
  caliper, or rim bead Ø − 0.07 for a floating one. Do not exceed this, or the caliper pokes through the barrel.
- **Plane:** disc centre at x = s·(hw − face_depth − 0.035), where face_depth is the wheel's spoke-face recess
  (§4.2). The disc must sit **≥ 0.015 inboard of the spoke back face** at the caliper radius.
- **Thickness:**

  | Case | Thickness |
  |---|---|
  | Front vented, economy | 0.020–0.022 |
  | Front vented, sport | 0.026–0.030 |
  | Front vented, big brake / supercar | 0.032–0.038 |
  | Rear solid | 0.010–0.012 |
  | Rear vented (sport) | 0.020–0.024 |
  | Carbon-ceramic look | 0.032–0.038 F, 0.028–0.032 R |

- **Top hat:** a lathe cylinder of Ø 0.16–0.19, protruding 0.02–0.04 toward outboard. The face carries lug studs
  (§4.2), darker `metal`.
- **Swept band:** the outer 0.055–0.065 radially, in bright `metal`. The inner ring up to the hat is darker.
- **Vented look:** the disc rim is drawn as two plates (each 0.3·t) with a **dark gap (0.4·t)**. In practice, an
  inner ring of `trim` at the edge band. +48 tris at N = 24.
- **Drilled:** 3 staggered rings of 6–12 dark dots (tiny quads in `trim`, 0.002 proud, 2 tris each) on the swept
  band. 36 dots ≈ 72 tris. Hero only.
- **Slotted:** 6–12 thin curved dark strips (2–4 quads each) across the swept band, angled 30° to radial ≈ 48–96 tris.
- **Carbon-ceramic:** dark charcoal disc (`carbon` slot or a very dark `metal`), lighter grey hat, cross-drilled
  dots. Large OD.
- **Drum (old rear axles):** a closed cylinder of Ø (drum Ø + 0.02) × 0.06–0.08 deep, with an outer cooling-fin ring
  (8 flat fins or a ribbed lathe) in dark cast `metal`. Backing plate: a disc of Ø + 0.03 inboard. The drum spins,
  the backing plate steers/is fixed like a caliper. With a closed steel wheel there is no caliper; with an open mag
  wheel (ironside) the drum face shows through.
- Disc cost (N = 24): 2 faces × 24 × 2 = 96 + edge 48 + vent gap 48 + hat 40 ≈ **230**. Traffic (N = 16, no
  vent/hat): **64**.

### 3.2 Calipers (wheel space, part `caliper`, steers, does not spin)

Shape: an arc-swept beveled box hugging the disc edge. Inner radius = disc_r − pad_depth, outer = disc_r + 0.012.

| Type | Arc span | Radial depth | Axial width | Look |
|---|---|---|---|---|
| Single-piston floating | 50–60° | 0.065 | t + 0.055 (bulk inboard: 70% of width on the inboard side) | plain cast, bracket visible |
| Twin-piston floating (trucks/police) | 60–70° | 0.075 | t + 0.07 | heavy, boxy |
| 2-piston fixed (opposed, 1+1) | 50–55° | 0.06 | t + 0.05, symmetric | small, rounded |
| 4-piston fixed (2+2) | 65–75° | 0.07 | t + 0.06 symmetric | smooth, rounded ends, painted |
| 6-piston fixed (3+3) | 85–95° | 0.08 | t + 0.065 | long, painted, outer face flat |
| 8/10-piston monoblock (supercar) | 100–115° | 0.085 | t + 0.07 | very long, curved, bridge "window" over the pad |

- Geometry: 3–5 arc segments × beveled section. ~60 tris (simple) to ~140 (hero with a pad window and 2 bleed-nipple
  bumps). A floating caliper adds a bracket plate (box, 12 tris) on the opposite side of the disc.
- **Clock position:**
  - Front: **trailing, θ = 160–200°**, behind the hub, centred slightly above horizontal (θ ≈ 165°). This is the
    usual RWD/sports and most-common modern layout and reads best in side views.
  - FF economy (tansy, junkyard): **leading, θ ≈ 15–30°** (front of the hub, slightly up), which varies the look.
  - Rear calipers: θ ≈ 200–220° (behind, slightly below) or leading at θ ≈ 330°.
  - Handbrake cable: optional 0.008 Ø line, skip.
- **Colours by genre:**
  - Muscle modern: red or silver-grey.
  - Tuner: red, yellow, gold or blue.
  - German sport sedan: blue or silver.
  - Exotic/supercar: **yellow, red or black, with a big carbon-ceramic disc**.
  - Hot hatch: red.
  - Traffic: unpainted dark grey (`metal` darkened).
  - Police: **plain black or unpainted**.
  - Trucks/vans: dark grey, black dust shield.
  - 70s muscle: front cast grey, rear drum dark.

  Use the `caliper` material slot with a per-car tint.

### 3.3 Roster → brake map (sizes from `data/cars/<id>.spec.md`)

| Car | Front disc OD × t, caliper | Rear | Caliper colour / pos |
|---|---|---|---|
| `ironside_440` | 0.279 × 0.025 vented, **single-piston floating** (70 mm) | **drum 0.279** | grey, trailing; rear drum |
| `rampart_50` | 0.380 × 0.034 vented slotted, **6-piston fixed** (46/40/36) | 0.330 × 0.025 vented, single floating | red/silver, F trailing |
| `sentinel_harrier` NEW | ~0.355 × 0.032, 4-piston fixed | ~0.330 solid/vented, floating | black |
| `vireo_xs` | 0.262 × 0.022 vented, single floating | 0.239 × 0.010 solid, single floating | red, trailing |
| `lindwurm_4t` | 0.340 × 0.030 vented, single floating (large 60 mm) | 0.310 × 0.022 vented | red, trailing |
| `kestrel_ts` | 0.300 × 0.026 vented, single floating | 0.280 × 0.010 solid | silver/red, trailing |
| `shiro_turbo` | 0.296 × 0.026 vented, **4-piston fixed** (42+42) | 0.297 × 0.018 vented, single | yellow/red, trailing |
| `hakone_gt` | 0.323 × 0.030 vented, **4-piston fixed** | 0.324 × 0.022 vented, **2-piston fixed** (36+36) | gold/red |
| `sperber_32` | 0.325 × 0.028 vented, single floating (60 mm) | 0.328 × 0.020 vented | blue/silver |
| `debug_coupe` | ~0.348 × 0.030 vented, 4-piston fixed | ~0.330 × 0.024 | red |
| `lucciola_350` | 0.330 × 0.030 vented drilled, **4-piston fixed** (40+40) | 0.310 × 0.028 drilled, 4-piston fixed | red or black, trailing |
| `fulmine_v10` NEW | **0.380–0.400 × 0.038 carbon-ceramic, 8-piston monoblock** | 0.356–0.380 × 0.032 CC, 4-piston | yellow/black/red |
| `debug_sedan` | ~0.300 × 0.025 vented, single floating | ~0.280 × 0.010 solid | plain |
| `junkyard_beater` | 0.240 × 0.018 vented (or solid 0.013), single floating | **drum 0.200** | rusty grey, leading |
| `tansy_13` | 0.256 × 0.020 vented, single floating | **drum 0.203** | plain, leading |
| `furrow_24` | 0.274 × 0.022 vented, single floating | **drum 0.254** | plain |
| `hauler_1500` | 0.330 × 0.030 vented, **twin-piston floating** (54+54) | 0.345 × 0.020, single floating | dark grey |
| `drayman_cargo` | 0.325 × 0.030 vented, twin-piston floating (57+57) | 0.330 × 0.022, single floating | dark grey |
| `sentinel_cruiser` | 0.350 × 0.032 vented, twin-piston floating | 0.345 × 0.022 vented, single | **plain black** |
| `sentinel_wolfhound` | 0.350 × 0.032 vented, twin-piston floating | 0.345 × 0.022, single | **plain black** |
| `sentinel_bastion` | 0.345 × 0.030 vented, twin-piston floating | 0.345 × 0.020, single | **plain black** |
| `sentinel_warden` NEW | ~0.300 × 0.028 vented, single-piston floating | **drum ~0.280** | plain/unpainted |

---

## 4. Wheels and tires

### 4.1 Wheel parameter struct (proposed `WheelStyle`)

```
rim_in            bead seat diameter in inches (13..21); rim_r = rim_in·0.0127
rim_width         ≈ tire width − 0.02 (sport) | −0.03 (truck/vintage)
face_depth        recess of the spoke face from the outer lip plane: 0.005 (flush) .. 0.07 (deep dish)
lip_width         radial width of the visible outer lip/flange: 0.010 .. 0.045 (stepped lip, tuner)
lip_style         Flat | Stepped | Rolled
spokes            count (0 = disc/steelie)
spoke_shape       Straight | Tapered | YSplit(gap) | Twin(gap) | Mesh(cross) | Fan(twist_deg) | Slot(window)
spoke_w_hub, spoke_w_rim   tangential widths (m)
spoke_concave     axial drop of the spoke from hub to rim (0 .. 0.04)
hub_r             0.065 .. 0.09 (mounting pad radius)
cap               None | Small(r 0.03) | Centre(r 0.045) | Hubcap(full face, r = rim_r) | DogDish(r 0.12) | CentreLock(hex 0.06)
lugs              count (4|5|6|8|1) and pcd_r (bolt-circle radius)
lug_style         Nut(hex 0.019) | Spline | Hidden
finish            material + tint (silver, gunmetal, black, gold, chrome, machined face)
```

**Lug patterns (generic):**

| Car type | Pattern (count × PCD) |
|---|---|
| Small FF | 4 × 0.100 |
| Japanese/US sport and modern muscle | 5 × 0.1143 |
| European sedan | 5 × 0.112 / 5 × 0.120 |
| 70s muscle | 5 × 0.1143 |
| Pickup/SUV/police SUV | 6 × 0.1397 |
| Heavy van | 8 × 0.1651 |
| Supercar | 5 × 0.112, or **centre-lock** (single 0.065 hex nut + 6–10 spline pins) |

Lug nut = 6-sided prism 0.019 Ø × 0.02, 12–16 tris each.

### 4.2 Rim styles (≥ 10)

Common base for every style:
- **Barrel/lip lathe:** inner barrel at rim_r − 0.010, out to a flange at rim_r + 0.012 (radial lip_width), lip face,
  then the spoke-face ring.
- Lathe profile 5–7 pts × N segs. Hero N = 24 (10-spoke style: N = 30, aligned with the spokes), traffic N = 16.
- Spokes are extruded quads swept along 2–3 points from hub_r to the inner face ring, given thickness 0.015–0.03
  (2 extra side walls).
- Simple spoke: 4 sides × 2 segments × 2 + 2 end caps ≈ 20 tris. Y/twin spoke ≈ 36. Fan ≈ 28.

| # | Style | Params | Tris (rim only, hero) |
|---|---|---|---|
| 1 | **Classic 5-spoke** (star) | 5 Tapered, w_hub 0.07 → w_rim 0.045, concave 0.01, face_depth 0.02, lip 0.012 flat, centre cap 0.045, 5 lugs | 240 lathe + 100 spokes + 30 cap + 70 lugs ≈ **440** |
| 2 | **Deep-dish mesh / cross-spoke** | 2 rings of 8–10 thin spokes (w 0.012) crossing at 30° offset, face_depth 0.06, **stepped lip 0.04** polished, centre cap + exposed bolts (12–16 tiny bolt bumps around the lip, hero only) | 280 + 360 + 30 + 70 ≈ **740** (+ 96 lip bolts) |
| 3 | **Turbine / fan** | 12–16 flat blades twisted 25–35° about the radial axis, w 0.035, closed outer ring, face_depth 0.015, cap 0.05 | 240 + 400 + 30 + 70 ≈ **740** |
| 4 | **Multi-spoke 10×** | 10 Straight spokes w 0.022 (constant), concave 0.02, face_depth 0.02, N = 30 | 300 + 200 + 30 + 70 ≈ **600** |
| 5 | **Split 5-spoke Y** | 5 YSplit: single stem from the hub to 55% radius, then two 0.022 arms, gap 0.012; concave 0.03 (clears big calipers); machined face option | 240 + 180 + 30 + 70 ≈ **520** |
| 6 | **6-spoke tuner** | 6 Straight w 0.045 constant, flat/slightly concave 0.012, face_depth 0.025, lip 0.02 stepped, colour gold/bronze/white/black | 260 + 120 + 30 + 70 ≈ **480** |
| 7 | **Steelie + full hubcap** | Steel disc: lathe 6 pts (barrel, bead hump, flat face, 4–8 oval vent holes as dark quads). Hubcap = shallow dome lathe (16 × 4) covering to rim_r + 0.005, plus 6–10 raised "spoke" ribs (boxes) or a plain disc. Silver plastic. Option: **hubcap missing** on 1 of 4 wheels (traffic variety) | traffic 16 seg: 160 + 96 + 50 ≈ **300** |
| 8 | **Steelie + centre cap (police)** | Black or silver steel, flat face with **5–6 round vent holes** (dark discs, 8 tris each) at r = 0.6·rim_r, small chrome/black centre cap 0.045, exposed lug nuts | 180 + 48 + 30 + 70 ≈ **330** |
| 9 | **Steelie + trim ring + dog-dish** (old police / `sentinel_warden`) | Steel body as 8, chrome trim ring (thin lathe at rim_r − 0.01…rim_r + 0.012) + small chrome dish r 0.12 | ≈ **380** |
| 10 | **Slotted mag (muscle)** | Cast disc face with **5 (or 6) elongated slots** (rounded rectangles, inner r 0.45·rim_r to outer 0.85·rim_r, angular width 30°) cut as dark recessed quads; polished lip 0.015; chrome lug nuts + small cap; face_depth 0.03; finish grey with polished ring | 240 + 5 × 30 + 30 + 70 ≈ **490** |
| 11 | **Monoblock forged (supercar)** | 5 or 7 YSplit/Twin spokes, thin (w 0.018 per arm), **concave 0.04**, spokes run right to the flange (no visible face ring), lip 0.008, centre cap 0.04 or centre-lock | 260 + 250 + 30 + 70 ≈ **610** |
| 12 | **Centre-lock** | Any 5–10 spoke + hub `CentreLock`: hex nut prism 0.065 across flats × 0.035 + 6–10 drive pins (8 tris each); coloured nut (red/blue/black) | +100 over base |
| 13 | **Truck 6-spoke / 6-window** | 6 broad spokes w 0.06 with dark windows between, deep offset (face_depth 0.05 outboard, centre "hub" bulge 0.04), chrome or machined, 6 lugs, large cap 0.06 | ≈ **520** |
| 14 | **Basic 7-spoke OEM alloy** (traffic) | 7 Tapered w 0.04 → 0.03, flat, silver, small cap, 4/5 lugs | traffic (16 seg) ≈ **300** |

Current star rim (40 tris) → **retire it**. Keep the flat 10-point star only as a far-LOD fallback.

### 4.3 Tire profile from aspect ratio

Use `wheels[i].radius_m` (loaded R) and `width_m` (W), plus `rim_in` from §4.4.
- **Sidewall height** h = R − rim_r (loaded). The nominal aspect ratio `a` (e.g. 45) is only needed to pick the
  profile character: low profile is a < 50, tall is a ≥ 60.
- Profile in the (x, r) half-plane, outboard half (mirror for inboard). Pts:
  1. bead (x = 0.48·W_rim, r = rim_r)
  2. rim-protector/lower sidewall (x = 0.50·W + bulge, r = rim_r + 0.25·h)
  3. **max section width** (x = 0.50·W + bulge, r = rim_r + 0.50·h)
  4. upper sidewall (x = 0.47·W, r = R − shoulder_r)
  5. shoulder (x = 0.5·W − shoulder_r·0.3, r = R − shoulder_r·0.3)
  6. tread edge (x = 0.5·tread_w, r = R)
  7. tread centre (x = 0, r = R + crown)
- Values by type:

  | Type | bulge | shoulder_r | tread_w | crown |
  |---|---|---|---|---|
  | Low profile (a ≤ 45) | 0.004 | 0.10·W | 0.86·W | 0.002 |
  | Mid (a 50–60) | 0.008 | 0.13·W | 0.82·W | 0.003 |
  | Tall (a ≥ 65, trucks, all-terrain) | 0.012–0.015 | 0.16·W | 0.80·W, squarer | 0.004 |
  | **vintage_bias** | 0.015 | **0.24·W** (rounded crown) | 0.72·W | 0.008 |
  | drag_radial | 0.018 (soft wrinkle) | 0.20·W | 0.85·W | 0 |
  | semi_slick | 0.004 | 0.08·W (sharp) | 0.90·W | 0 |

- Point budget: hero 10–12 profile pts (7 above, mirrored, sharing the centre). Traffic 6 pts: bead, max width,
  shoulder, tread centre, mirrored.
- **Tread grooves as geometry: only circumferential grooves**, as profile notches (4 extra pts per groove, depth
  0.006, width 0.012). Each costs 2·N·4 tris.

  | Compound | Grooves |
  |---|---|
  | `sport` / `street` / `police_pursuit` | 2–3 |
  | `wet_biased` | 4 |
  | `semi_slick` | 2 shallow |
  | `drag_radial` | 0 |
  | `vintage_bias` | 3 narrow |

  **Never model tread blocks**, except `all_terrain`: 2 offset rows of shoulder lugs (N/2 boxes per side, 0.025
  proud, 12 tris each), ≈ 2 × 12 × 12 = 288 tris. That is the one tire where the block pattern reads at gameplay
  distance.
- Sidewall colour band: `vintage_bias` gets an optional white/red stripe ring (an extra profile point pair at the
  max-width radius ± 0.008, in a separate material). There is no lettering anywhere.
- Cost: `2·N·(P−1)`. Hero N = 24, P = 11 → 480, plus 2 grooves (+192) ≈ **670**. Traffic N = 16, P = 6 → **160**.

### 4.4 Roster → wheel/tire map

rim/R uses the loaded radius. Fulmine, warden, harrier and the debug cars are proposals (no spec file yet).

| Car | Tire (spec) | rim_in | rim/R | Sidewall h | Style (§4.2) | Finish | Lugs |
|---|---|---|---|---|---|---|---|
| `ironside_440` | 235/70R15 vintage_bias | 15 | 0.55 | 0.153 | 10 slotted mag (alt: 9 steelie + dog-dish) | grey + polished lip | 5 × 0.1143 chrome |
| `rampart_50` | 255/40R19 F, 275/40R19 R | 19 | 0.72 | 0.092/0.100 | 5 split Y, concave | machined/gunmetal | 5 × 0.1143 |
| `sentinel_harrier` NEW | ~255/45R18 police_pursuit | 18 | ~0.68 | ~0.11 | 6 tuner-style 6-spoke, black | black | 5 × 0.1143 |
| `vireo_xs` | 195/55R15 | 15 | 0.66 | 0.098 | 1 classic 5-spoke (or 3 turbine as variant) | white/silver | 4 × 0.100 |
| `lindwurm_4t` | 225/40R18 | 18 | 0.74 | 0.080 | 3 turbine/fan | dark silver | 5 × 0.112 |
| `kestrel_ts` | 225/45R17 | 17 | 0.70 | 0.092 | 6 6-spoke tuner | gold/bronze | 5 × 0.1143 |
| `shiro_turbo` | 215/45R17 F, 225/45R17 R | 17 | 0.70 | 0.087/0.092 | 2 deep-dish mesh, stepped lip | silver + polished lip | 5 × 0.1143 |
| `hakone_gt` | 235/45R17 F, 255/40R17 R | 17 | 0.69 | 0.096/0.092 | 1 classic 5-spoke | silver | 5 × 0.1143 |
| `sperber_32` | 225/45R18 F, 255/40R18 R | 18 | 0.71 | 0.091 | 4 multi-spoke 10× (staggered rear lip 0.03) | silver | 5 × 0.120 |
| `debug_coupe` | (r 0.33, 245/275) → 19 | 19 | 0.73 | ~0.09 | 5 split Y | gunmetal | 5 × 0.112 |
| `lucciola_350` | 225/40R18 F, 265/40R18 R | 18 | 0.74/0.70 | 0.080/0.096 | 1 classic 5-spoke star, flat face (90s exotic) | silver/magnesium-grey | 5 × 0.108 |
| `fulmine_v10` NEW | ~245/30R20 F, ~305/30R20 R semi_slick-look sport | 20 | ~0.77 | ~0.073/0.091 | 11 monoblock forged (12 centre-lock as mod) | black/gunmetal | 5 × 0.112 or CL |
| `debug_sedan` | (r 0.325, 215) → 215/55R17 | 17 | 0.66 | ~0.11 | 14 basic 7-spoke | silver | 5 × 0.1143 |
| `junkyard_beater` | 175/70R13 | 13 | 0.59 | 0.114 | 7 steelie + hubcap (**one cap missing, random-free: fixed on rl**) | grey steel, faded cap | 4 × 0.100 |
| `tansy_13` | 175/65R14 | 14 | 0.63 | 0.105 | 7 steelie + hubcap | silver plastic | 4 × 0.100 |
| `furrow_24` | 215/70R14 | 14 | 0.56 | 0.137 | 8 steelie + centre cap (white/silver painted) | white steel | 6 × 0.1397 |
| `hauler_1500` | 265/70R17 all_terrain | 17 | 0.56 | 0.169 | 13 truck 6-spoke | chrome/machined | 6 × 0.1397 |
| `drayman_cargo` | 245/75R16 | 16 | 0.55 | 0.168 | 8 steelie + small black cap | black steel | 8 × 0.1651 |
| `sentinel_cruiser` | 225/60R18 police_pursuit | 18 | 0.65 | 0.124 | 8 police steelie | **black, chrome cap** | 5 × 0.1143 |
| `sentinel_warden` NEW | ~215/70R15 police_pursuit | 15 | ~0.56 | ~0.15 | 9 steelie + trim ring + dog-dish | black/silver + chrome | 5 × 0.1143 |
| `sentinel_wolfhound` | 245/55R18 police_pursuit | 18 | 0.65 | 0.123 | 8 police steelie | **black, small cap** | 5 × 0.1143 |
| `sentinel_bastion` | 265/60R17 police_pursuit | 17 | 0.60 | 0.144 | 8 police steelie (6-lug) | silver or black | 6 × 0.1397 |

Every car lists the same rim on all 4 corners, except staggered rears, which keep the same style with a wider barrel.

---

## 5. Triangle budgets

### 5.1 Per-wheel cost (one corner, everything in wheel space)

| Item | Hero (N = 24) | Traffic (N = 16) | Far LOD (N = 12) |
|---|---|---|---|
| Tire (P = 11 + 2 grooves) | 480–670 | 160 | 72 |
| Rim (style dependent) | 440–740 (+100 centre-lock) | 300 | 60 (star/disc) |
| Brake disc + hat | 200–260 (+70 drilled) | 64 | 0 |
| Caliper | 60–140 | 12 (box) | 0 |
| Upright/knuckle (steers) | 44 | 0 | 0 |
| **Corner total** | **≈ 1,250–1,500** | **≈ 400–550** | **≈ 130** |

The 400–1500 range per wheel maps to: 400 = traffic steelie, 900 = mid (N = 20, no grooves, simple 5-spoke),
1,500 = hero mesh/turbine with a big drilled brake.

### 5.2 Per-subsystem budgets

| Subsystem | Hero/perf/police (8–20k total) | Traffic (4–10k total) |
|---|---|---|
| Body shell + details (loft, flares, scoops, glass, lights) | 5,000–11,000 | 2,800–6,500 |
| Wheels ×4 (tire + rim) | 3,600–5,600 | 1,200–1,900 |
| Brakes ×4 (disc + caliper; drums cheaper) | 1,000–1,600 | 250–350 |
| Suspension (4 corners + ARBs + axle/leaf) | 700–1,600 (helix springs 300 each ×4 = biggest item; use ribbon at 100) | 150–400 (cylinder dampers, flat arms, beam/axle box) |
| Engine + radiator (+ FMIC) | 800–1,800 (exotic V10 with stacks ≤ 2,000) | 150–350 (block + cover + pan) |
| Exhaust (pipes + mufflers + tips) | 250–600 | 40–120 (one pipe + tip) |
| Frame rails / subframes / driveshaft | 50–150 | 24–60 |
| Interior (existing minimal shell) | unchanged | unchanged |
| Police extras (light bar, push bar; owned by style-police) | 600–1,500 | — |

### 5.3 Rules of thumb

1. **Spend on what is always visible:**
   - wheels and tires (silhouette and sidewall)
   - caliper colour
   - exhaust tips
   - FMIC through the bumper on tuners
   - engine top through scoops/glass on muscle, exotics and the supercar

   The underbody can be 1/5 the density.
2. **Engines are rigid single-node parts** and are hidden most of the time. Recommendation to the integration lead:
   an `engine_detail` LOD flag, so the high-detail engine only goes in the mesh for cars whose engine is visible
   through glass or scoops (lucciola, fulmine, ironside, rampart). Other cars get the ~300-tri version, which still
   reads correctly once the hood is torn off.
3. **Segment counts must match across tire, rim lathe, disc and caliper arc** (24 for hero, 16 for traffic) so the
   silhouettes nest without gaps. Use 30 for the 10-spoke style to align spokes with segments.
4. All geometry deterministic: angles via `wp_math::sin_cos`, no RNG. Traffic variety (missing hubcap, colour)
   comes from fixed per-id choices or the renderer's instance tint, never from random generation.
5. A **quick win** with the current code:
   - Replace `r_in = 0.64·R` with the per-car `rim_r`.
   - Add the 7-point profile and an outboard-sign dish.

   That alone turns 160 → ~600 tris per wheel and fixes the "every car has 0.64 rim" look before any rim style
   exists.

### Sources consulted (named generically per the naming rule)

- Online encyclopedia articles and forum threads on 1960s–70s big-block pushrod V8s: deck height 9.8–10.2", bore
  spacing 4.84", roughly 0.69 W × 0.81 L overall.
- Encyclopedia articles on two compact racing DOHC V8s: 0.49–0.55 L × 0.51–0.53 W × 0.44–0.53 H, scaled up for road V8s.
- Spring-maker catalogues: coilover springs at 60 mm / 2.5" ID, free lengths 0.12–0.30 m.
- Owner-forum and parts-retailer rotor specs: front vented 30 mm (28 mm wear limit), rear solid 12 mm (10 mm wear
  limit), 28 mm vented rears exist.
- Parts-retailer listings for sports-car carbon-ceramic replacement discs: 390 × 32 mm.
- Leaf-spring retailers: 2.5" wide springs with 5 leaves for 60s–70s coupes; shackles 2.5–4.3" centre to centre.
- Aftermarket front-mount intercooler listings: cores 550 × 180–230 × 65 mm with 63 mm ports; 22 × 9 × 3" for
  300–500 hp.
- Game-art forum threads and asset-store listings: car LOD chain 7.2k → 3.7k → 1.7k → 0.4k tris; arcade/mobile cars
  2.6–3.3k; wheels count ×4.
- Soft-body sim modding documentation: rim/tire flexbodies on hub/tire node groups; brake torque reacted between a
  disc-centre node and a caliper-arm node.
- Press coverage of a demolition-racing game's damage model: parts detach piece by piece.
- Repo: `data/cars/*.json` and `*.spec.md` (tire sizes, disc/drum sizes, piston counts), `data/tires/README.md`,
  `tools/assets/src/mesh.rs` (`wheel_parts`, `WHEEL_SEGMENTS = 20`, `TRI_BUDGET`).
