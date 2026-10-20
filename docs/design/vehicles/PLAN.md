# Detailed vehicle models — multi-agent plan

Goal: replace the single "chamfered box + greenhouse" procedural body with archetype-specific,
much more detailed car models (muscle, hot hatch, tuner, supercar, exotic), varied traffic, and
detailed police cars (two-tone liveries, lettering, light bars) in several versions — each with
basic visible engine, suspension, brake and wheel geometry.

All naming rules in `CLAUDE.md` / `data/cars/README.md` apply: **no real makers, models, badges,
real police agencies or trademarked liveries.** Invented names only (police lettering uses the
fictional city in this repo or a generic word such as "POLICE" / "HIGHWAY PATROL" / "STATE
TROOPER" / "SHERIFF" — no real city or state names).

## Waves

| Wave | Agent | Owns (only edit these) | Output |
|---|---|---|---|
| 1 | research-performance | `docs/design/vehicles/research_performance.md` | archetype specs for muscle / hot hatch / tuner / supercar / exotic |
| 1 | research-traffic | `docs/design/vehicles/research_traffic.md` | traffic body types, colour distributions, variety tricks |
| 1 | research-police | `docs/design/vehicles/research_police.md` | police versions, two-tone schemes, lettering, light bars, equipment |
| 1 | research-mechanical | `docs/design/vehicles/research_mechanical.md` | engine / suspension / brake / wheel visual geometry per archetype |
| 1 | foundation | `tools/assets/src/{profile,mesh,style,builder,validate,lib}.rs`, `apps/testbed-web/src/render/panels.ts`, `data/panels/README.md` | parameterised body-style engine + part API + new material/part kinds (see below) |
| 1 | roster | `tools/specsheet/*`, `data/cars/*`, `data/lattices/*`, `data/panels/<car>.json`, `crates/wp_sim/src/{police_link,traffic_link}.rs`, `crates/wp_world/src/{lib,traffic}.rs` (multi-model hooks only), `crates/wp_wasm/*`, `apps/simcli/src/run.rs`, `crates/wp_net/src/testkit.rs` | 3 new cars + multi-model police/traffic selection |
| 2 | style-performance | `tools/assets/src/styles/performance.rs` | per-car body styles + details for the performance roster |
| 2 | style-traffic | `tools/assets/src/styles/traffic.rs`, `apps/testbed-web/src/render/traffic.ts` (+ minimal hook in `scene.ts` `updateAgents`) | civilian styles + instanced traffic variety |
| 2 | style-police | `tools/assets/src/styles/police.rs`, `apps/testbed-web/src/render/livery.ts`, `apps/testbed-web/src/render/police_units.ts` (+ minimal hooks in `vehicle_view.ts` / `scene.ts` `updatePoliceUnits`) | police styles, lettering, two-tone, light bars, push bars |
| 2 | mechanical | `tools/assets/src/mechanical.rs`, `tools/assets/src/wheels.rs` | engine bay, suspension, brakes, rim/tire styles |

Wave 2 starts from the merged wave-1 result. The integration lead (main session) merges,
regenerates every `data/panels/meshes/*.json`, runs the gates and takes screenshots.

**Generated files:** wave-2 agents commit *source*, and may regenerate meshes only for the cars
they own, so merges do not fight over generated JSON. The lead regenerates everything at the end.

## Roster → archetype map (wave-1 roster agent adds the three `NEW` cars)

| Car id | Archetype | Owner (wave 2) |
|---|---|---|
| `ironside_440` | muscle (early-70s big-block coupe) | performance |
| `rampart_50` | muscle (2010s modern muscle coupe) | performance |
| `vireo_xs` | hot hatch (late-90s FF) | performance |
| `lindwurm_4t` | hot hatch (2010s AWD) | performance |
| `kestrel_ts` | tuner (2000s AWD-able sport sedan) | performance |
| `shiro_turbo` | tuner (2000s FR coupe) | performance |
| `hakone_gt` | tuner / GT (90s FR twin-turbo) | performance |
| `sperber_32` | tuner / sport sedan (2000s German I6) | performance |
| `debug_coupe` | sport coupe | performance |
| `lucciola_350` | exotic (90s mid-engine wedge) | performance |
| `fulmine_v10` NEW | supercar (2010s mid-engine V10 AWD) | performance |
| `debug_sedan` | traffic: modern family sedan | traffic |
| `junkyard_beater` | traffic: 80s compact sedan | traffic |
| `tansy_13` | traffic: supermini 5-door | traffic |
| `furrow_24` | traffic: 90s compact pickup | traffic |
| `hauler_1500` | traffic: full-size crew-cab pickup | traffic |
| `drayman_cargo` | traffic: cargo van | traffic |
| `sentinel_cruiser` | police: full-size sedan ("sedan") | police |
| `sentinel_warden` NEW | police: 80s/90s boxy body-on-frame V8 sedan ("old sedan") | police |
| `sentinel_wolfhound` | police: AWD pursuit sedan, highway-patrol livery ("patrol") | police |
| `sentinel_bastion` | police: full-size SUV ("SUV") | police |
| `sentinel_harrier` NEW | police: muscle-coupe pursuit car ("sports patrol") | police |

## Foundation contract (wave 1 builds it, wave 2 uses it)

1. **`BodyStyle`** (`tools/assets/src/style.rs`): a plain struct of shape knobs consumed by
   `BodyProfile`/the loft, with `BodyStyle::default()` reproducing today's shape. Knobs at least:
   silhouette (`Notchback | Fastback | Hatch | Wagon | Wedge | CabForward | Pickup | Van | Suv`),
   nose height + rake, nose plan rounding, hood crown/bulge height + width, hood power-bulge /
   scoop, windshield rake, A/C-pillar rake, roof length / height offset, tumblehome, side
   sculpt (body-line crease height, depth), shoulder/haunch (rear fender swell), wheel-arch
   flare (amount, shape: round/box), ride-height visual offset, tail kamm / ducktail,
   overhang trims, beltline kick-up. Every knob documented with units and sane range.
2. **Style registry**: `fn style_for(def: &VehicleDef) -> CarStyle` dispatching by car id to
   `styles::{performance, traffic, police}::style(id)` (each returns `Option<CarStyle>`), falling
   back to a class-based default. `CarStyle { body: BodyStyle, details: Vec<Detail>, wheel:
   WheelStyle, mechanical: MechStyle, livery: Option<LiveryHint> }`. The foundation ships stub
   modules returning `None` so wave-2 agents only fill in their own file.
3. **Detail/part API** (`tools/assets/src/builder.rs`): helpers that emit skinned parts tied to a
   body region: `box`, `beveled_box`, `extrude(profile_2d, path)`, `cylinder`, `torus`,
   `lathe`, `surface_patch(y0..y1, band)` (a thin shell offset from the loft surface — for
   scoops, vents, spoilers, splitters, diffusers, skirts, flares, grilles, badges-free trim,
   push bars, light bars). Each detail declares region + material, so it dents/detaches with its
   panel.
4. **New material slots** (renderer + schema doc): `chrome`, `carbon`, `caliper` (tinted per
   car), `engine` (dark metal), `livery` (paint that samples the livery canvas texture),
   `lightbar_red`, `lightbar_blue`, `lightbar_clear`, `emissive_amber`, `grille`, `exhaust`.
5. **New part kinds**: `engine` (under the hood, only seen when hood is open/gone or through
   scoops), `suspension` (body-fixed), `brake_disc` (wheel space, spins), `caliper` (wheel space,
   steers but does **not** spin — renderer parents it to the wheel pivot, not the spin group),
   `lightbar`, `decal_carrier`.
6. **Livery UV atlas**: deterministic UVs so a 2D canvas can paint two-tone + lettering on paint
   parts: left side → u∈[0,0.25), right side → u∈[0.25,0.5) (both laid out so text drawn
   left-to-right reads correctly from outside), top (hood/roof/trunk) → v-band of u∈[0,0.5) etc.
   Document the exact layout in `data/panels/README.md`. U 0.5–1 stays for the existing use.
7. **Triangle budget**: hero/performance + police 8k–20k, traffic 4k–10k (per class); update
   `validate.rs`. Keep JS skinning cheap: only re-skin a part when lattice nodes changed, skip
   rigid parts (engine/suspension use one node, weight 255).
8. Keep everything deterministic (no RNG, `wp_math::sin_cos`) and all existing tests passing.
