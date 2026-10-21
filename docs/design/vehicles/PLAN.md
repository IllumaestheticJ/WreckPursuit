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

## Foundation API (for wave 2)

Built by the wave-1 foundation agent. Crate `car-assets` (`tools/assets`). Read
`tools/assets/README.md` (generator overview, budgets, perf) and `data/panels/README.md` (mesh
format 2, part kinds, materials, livery atlas). Every cargo command below assumes
`export CARGO_TARGET_DIR=/home/user/.cargo-target-shared CARGO_INCREMENTAL=0
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`.

### Entry points (exact signatures)

```rust
// tools/assets/src/styles/{performance,traffic,police}.rs — one per wave-2 style agent.
// (The contract above said `style(id)`; it takes the whole def so styles can read sizes.)
pub fn style(def: &VehicleDef) -> Option<CarStyle>;          // Some only for the ids you own

// tools/assets/src/style.rs (shared types; do not edit, ask the lead)
pub fn style_for(def: &VehicleDef) -> CarStyle;  // performance → traffic → police → class_default
pub fn class_default(def: &VehicleDef) -> CarStyle;
impl CarStyle { pub fn with_body(body: BodyStyle) -> CarStyle }
impl BodyStyle { pub fn preset(silhouette: Silhouette) -> BodyStyle }   // Default = original look
impl Detail {
    pub fn new(name: &str, region: BodyRegion, material: MaterialSlot, shape: Shape) -> Detail;
    pub fn mirrored(self) -> Detail; pub fn rigid(self) -> Detail; pub fn kind(self, k: PartKind) -> Detail;
}
pub type DetailFn = fn(&DetailCtx, &PartKey, &mut Builder);     // Shape::Custom(f)
pub struct DetailCtx<'a> { pub def: &'a VehicleDef, pub profile: &'a BodyProfile }

// tools/assets/src/mechanical.rs + wheels.rs — wave-2 mechanical agent
pub fn mechanical::parts(def: &VehicleDef, p: &BodyProfile, m: &MechStyle, b: &mut Builder);
pub fn mechanical::brake_parts(g: &WheelGeom, m: &MechStyle, w: &WheelStyle) -> Vec<MeshPart>;
pub fn wheels::wheel_parts(g: &WheelGeom, style: &WheelStyle) -> Vec<MeshPart>;
pub struct WheelGeom { index, outboard /* −1 left, +1 right */, radius, width, rim_radius,
                       tire_aspect, steerable, front, hub }   // g.name("tire") == "wheel_fl_tire"

// tools/assets/src/builder.rs — primitives (chassis space; wheel parts: hub space, axle +X)
PartKey::body(name, region, kind, material) / PartKey::wheel(name, wheel, kind, material) / .rigid() / .mirrored()
b.cuboid(&key, lo, hi)                      // the PLAN's `box`
b.beveled_box(&key, lo, hi, bevel)
b.oriented_box(&key, center, [ax, ay, az], half)
b.cylinder(&key, a, b, r_a, r_b, segments, caps)
b.torus(&key, center, axis, major, minor, seg_major, seg_minor)
b.lathe(&key, center, axis, &[[radius, along], ..], segments)
b.extrude(&key, &profile_2d_ccw, &path, up, caps)
b.surface_patch(&key, profile, &Patch { y0, y1, band: PatchBand::Side { z0, z1 } | PatchBand::Top { x0, x1 },
                                         offset_m, thickness_m, steps: (0, 0) })
b.poly(&key, &[p0, p1, p2, ..]) / b.poly_facing(&key, &pts, outward) / b.mirror_part("from", &to_key)

// BodyProfile queries for details (all values already styled)
p.y_nose, p.y_tail, p.y_fw, p.y_bf, p.roof /* (rear y, front y, z, _) */, p.y_deck, p.z_deck,
p.z_belt, p.z_bumper_top, p.arches /* (y, z, r) */, p.bed, p.half_width(y), p.belt(y), p.top(y),
p.sill(y), p.ring(y), p.side_point(y, z) -> (pt, normal), p.top_point(y, x) -> (pt, normal)
```

`BodyStyle` knobs (all in `style.rs` with units and ranges): `silhouette` (Notchback, Fastback,
Hatch, Wagon, Wedge, CabForward, Pickup, Van, Suv), `cabin_shift_m`, `hood_length`, `nose_h`,
`nose_rake_deg`, `nose_plan_round`, `tail_plan_round`, `hood_crown`, `hood_bulge_h`,
`hood_bulge_w`, `power_bulge_h`, `hood_scoop` (+`hood_scoop_h`), `cowl_h`,
`windshield_rake_deg`, `a_pillar_rake_deg`, `rear_glass_rake_deg`, `roof_len_trim`, `roof_dz`,
`roof_crown`, `tumblehome`, `glass_inset`, `rear_glass_bow`, `belt_dz`, `belt_rake`,
`belt_kick`, `crease_z`, `crease_depth`, `sill_tuck`, `shoulder_round`, `haunch`,
`front_haunch`, `flare`, `flare_shape` (Round / Box), `arch_gap`, `ride_dz`,
`stance_pitch_deg`, `deck_drop`, `ducktail_h`, `tail_round`, `tail_drop`, `overhang_trim`,
`bed_depth`. Names follow `research_performance.md` §0 where it proposed one. `WheelStyle`
(rim kind, spokes, rim radius, face depth, lip, concave, segments, cap, lugs, PCD, rim material,
tire aspect, `front_camber_deg` / `rear_camber_deg`) and `MechStyle` (engine kind, layout,
induction, `show_engine_detail`, front / rear suspension kind, front / rear `BrakeStyle`,
`caliper_rgb`, `spring_rgb`, exhaust kind + tip radius) are defined for the mechanical agent to
consume; the foundation only draws placeholders from them.

### Add a style for one car id
In your module (`styles/performance.rs` has a complete `debug_coupe` example):
```rust
pub fn style(def: &VehicleDef) -> Option<CarStyle> {
    match def.id.as_str() {
        "ironside_440" => Some(ironside()),
        _ => None,
    }
}
fn ironside() -> CarStyle {
    let mut s = CarStyle::with_body(BodyStyle {
        cabin_shift_m: -0.35, haunch: 0.04, power_bulge_h: 0.05, hood_scoop: HoodScoop::Cowl,
        ..BodyStyle::preset(Silhouette::Fastback)
    });
    s.mechanical.engine = EngineKind::V8;
    s
}
```
`style_for` sets `tier` (performance / police → Hero 8k–20k, traffic → Traffic 4k–10k).

### Add a detail
Push to `CarStyle.details`. Either a declarative shape:
```rust
s.details.push(Detail::new("skirt_l", BodyRegion::FrameLeft, MaterialSlot::Trim,
    Shape::Patch(Patch { y0: -0.8, y1: 0.6, band: PatchBand::Side { z0: 0.17, z1: 0.27 },
                         offset_m: 0.0, thickness_m: 0.02, steps: (0, 0) })).mirrored());
```
or a function that reads the profile (positions relative to the real body):
```rust
s.details.push(Detail::new("spoiler", BodyRegion::Trunk, MaterialSlot::Carbon, Shape::Custom(spoiler)));
fn spoiler(ctx: &DetailCtx, key: &PartKey, b: &mut Builder) {
    let p = ctx.profile;
    let y = p.y_tail + 0.22;
    let deck = p.top_point(y, 0.0).0[2];
    b.beveled_box(key, [-0.7, y - 0.12, deck + 0.10], [0.7, y + 0.06, deck + 0.125], 0.006);
}
```
A custom function may emit extra keys (e.g. a grille with its own material). Light bars: kind
`PartKind::Lightbar`, lens materials `LightbarRed/Blue/Clear`. The renderer gives parts left of
x = 0 and right of it independent materials (`PanelBody.lightbarMaterials.{left,right}.{red,blue,clear}`)
and exposes their centres (`lightbarAnchors.{left,right}`) for two point lights. Unit-number
decals: kind `DecalCarrier`, a `Patch` with `offset_m: 0.002`. Livery: `PanelBody.setLivery(tex)`,
`liveryMaterial`, `liveryPx(atlas, side, a, b, W, H)` in `render/panels.ts`.

### Add a wheel / mechanical variant
Edit `wheels.rs` (`wheel_parts`) and `mechanical.rs` (`parts`, `brake_parts`). The enums live in
`style.rs` and already list the research variants; ask the lead before adding more. Hub space:
origin at the hub, axle +X, outboard face at `x = g.outboard · g.width / 2`. Kinds: `Tire`,
`Rim`, `BrakeDisc` spin; `Caliper`, `Upright` steer and follow compression (wheel pivot);
body-fixed hardware is `Engine` / `Suspension` with `PartKey::rigid()` (one node, weight 255,
moved by matrix, never re-skinned). Gate the expensive engine with
`MechStyle.show_engine_detail`. Per-car caliper colour: `MechStyle.caliper_rgb` → mesh
`tints.caliper`.

### Regenerate / validate one car
```sh
cargo run -p car-assets --bin gen-car-mesh -- ironside_440      # writes data/panels/meshes/ironside_440.json
cargo run -p car-assets --bin validate-car-asset -- ironside_440
cargo run -p car-assets --example profile_dump -- --ring 1.5 ironside_440   # anchors + one cross-section
cargo test -p car-assets       # all cars: valid, up to date, hinge lines, primitive winding
```
Screenshots: `node tools/assets/panels_shots.mjs /tmp/out` (needs `apps/testbed-web/node_modules`
and the wasm pkg). `?livery=grid` in the testbed URL shows the atlas debug texture on every car.

### Gotchas
- Commit only the meshes of cars you own. A change to shared code (`style.rs`, `builder.rs`, …)
  makes every mesh stale and `cargo test` will say so; the lead regenerates everything at the end.
- Part names must be unique lower_snake_case. `.mirrored()` swaps the `_l`/`_r`, `fl`/`fr`,
  `rl`/`rr` tokens (a name with no side token gets `_m`), so name left-side details `..._l`.
- Use `PartKind::Detail` (the default) for add-ons, never `Panel`: the hinge test treats a region's
  first `Panel` part as the hinged panel mesh.
- The region decides what a detail does in a crash: on `Hood` / `Trunk` / doors it swings with the
  panel, on bumpers it hangs / falls with them, on `Roof` / `Cabin` it stays. Use `FrameFront` /
  `FrameRear` for hardware. Regions with < 4 nodes borrow from fallbacks (`skin::fallback_regions`).
- `cabin_shift_m` / `hood_length` move the visual hood rear edge / trunk front edge off the lattice
  hinge (≤ 0.6 m, test-checked). The hood still pivots on its lattice hinge nodes, so keep shifts
  modest (about ±0.35 m) or a swinging hood looks detached from the cowl.
- Budgets are hard limits in `validate-car-asset`; `gen-car-mesh` prints `tri_count`. Foundation
  output is 8.7k–10.1k (hero) and 5.5k–7.0k (traffic) before your details.
- `MaterialSlot::Livery` is for anything that should carry a livery; `Paint` for body-colour parts
  that should not (mirrors, a plain spoiler).
- Stay deterministic: `wp_math::sin_cos` / `builder::turn`, no RNG, no HashMap iteration.
- `apps/testbed-web/src/vehicle_view.ts` got one minimal foundation hook: calipers / uprights go in
  the wheel pivot, visual camber on the spin group, and pivot-part visibility. The lead's lamp
  markers there can now go, because the head / tail light parts sit on the outer surface.
