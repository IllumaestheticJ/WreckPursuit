# car-assets (`tools/assets`) — car models and panels pipeline

Owned by workstream R (prompt 21, `ws/panels`). Produces, for every car in `data/cars`, a
panel spec and a procedural low-poly mesh whose parts are skinned to the car's damage lattice,
and checks them. See `data/panels/README.md` for the file formats and
`docs/integration/panels.md` for how the pieces fit together.

```sh
cargo run -p car-assets --bin gen-panels            # data/panels/<car>.json (first pass from the lattice)
cargo run -p car-assets --bin gen-panels -- --check # exit 1 if a spec differs from the derived one
cargo run -p car-assets --bin gen-car-mesh          # data/panels/meshes/<car>.json (+ bind-skin)
cargo run -p car-assets --bin gen-car-mesh -- --check  # exit 1 if a mesh is stale
cargo run -p car-assets --bin bind-skin -- data/panels/meshes/<car>.json [data/lattices/<id>.json]
cargo run -p car-assets --bin validate-car-asset [car ...]
cargo run --release -p car-assets --bin panels-bench # perf, 24 cars
cargo run --release -p car-assets --example panel_probe -- apps/simcli/scenarios/panels/side_swipe_door.ron
cargo run -p car-assets --example profile_dump -- [--ring <y>] <car>  # styled anchors, regions, a cross-section
cargo test -p car-assets                             # every car valid + meshes up to date
cargo run --release -p car-assets --example crumple_matrix -- Arcade [frame ticks]  # hood/fender/bumper crumple, 30/50/70 km/h
node tools/assets/panels_shots.mjs                   # browser checks + screenshots (docs/media/panels)
node tools/assets/crumple_shots.mjs after            # hood crumple shots at 8 m / 30 m
```

Wave-2 style / mechanical authors: see `docs/design/vehicles/PLAN.md` "Foundation API (for wave 2)".
Per-archetype notes: `docs/design/vehicles/wave2_{performance,traffic,police,mechanical}.md`.

### Regenerating after a change
Any change under `src/` or `data/cars` / `data/lattices` makes the committed meshes stale (the
`assets` test fails). Regenerate everything, validate, test:

```sh
export CARGO_TARGET_DIR=<a target dir of this checkout only>   # see below
cargo run -p car-assets --bin gen-car-mesh && cargo run -p car-assets --bin validate-car-asset
cargo test -p car-assets
```

`repo_root()` is `env!("CARGO_MANIFEST_DIR")`, fixed at compile time: a `gen-car-mesh` binary
built in another worktree with a shared `CARGO_TARGET_DIR` writes into *that* worktree's
`data/`. Use one target dir per checkout (or none).

## Mesh generator (`src/{style,profile,mesh,builder,wheels,mechanical}.rs`, `src/styles/`)
- **Style** (`style.rs`): `style_for(def)` asks `styles::{performance, traffic, police}` (first
  `Some` wins) else `class_default` (Truck → Pickup / Van, PoliceSuv → Suv, others the default
  notchback; engine layout / exhaust by class). A `CarStyle` = `BodyStyle` (≈45 shape knobs,
  documented with units and ranges; `Default` = the original look) + `details` + `WheelStyle` +
  `MechStyle` + optional `LiveryHint` + `tints` + `accent_rgb` / `underglow_rgb` + budget `Tier`.
  Styles live in `src/styles/{performance/,traffic.rs,police.rs + police/}`; every knob clamps
  softly (`profile::clamp_soft`), so extreme values never panic (`tests/assets.rs` sweeps them).
- **Body profile** (`profile.rs`) from the `VehicleDef` (width, wheels → arches) anchored to the
  lattice (hood y-range and heights, firewall, door hinge and latch stations, roof, rear glass,
  deck, quarters, bumpers), then styled (cabin shift / hood length, roof edges from glass rakes,
  heights, overhangs, open pickup bed). Panel zones stay on lattice zone boundaries, so every
  panel mesh covers its lattice region's zone; doors and the trunk start at their hinge line, the
  hood at the windshield base. `cabin_shift_m` / `hood_length` move the visual hood rear edge and
  the rear-glass base on purpose (the panel still pivots on its lattice hinge nodes; at most 0.6 m
  away, checked by `tests/assets.rs`). `cabin_shift_m` spans −0.6…+0.6 m: the mid-engine cars use
  +0.40 / +0.45 for a cab-forward roof (centre ≈ 46 % of the length from the nose).
- **Loft** (`mesh.rs`): cross-section rings of 18 points per side (sill tuck, rocker, character
  crease, shoulder, belt ledge, 3-point greenhouse, 7-point crowned top / open bed) at adaptive
  stations: hero 7 cm (3.6 cm near the nose, tail and arches), traffic 11 cm (6.5 cm), plus every
  zone boundary and window edge. Side points under a wheel arch are lifted onto the arch (round or
  box) with the flare lip; haunches swell the sides around the arches.
- **Parts**: each loft face is classified by station and segment (lower side / belt ledge /
  greenhouse / top / bed) into a body region: bumpers, hood, fenders, doors (door windows ride with
  their door), quarter glass, sills, roof, windshield, rear glass (or cab-back wall), trunk or
  deck, quarters, pillars, open bed floor (`bed_floor`, FrameRear). Plus head / tail lights on the
  nose / tail faces (`LIGHT_PROUD_M` proud of the surface; region `FrameFront` / `FrameRear`,
  bound to their end of the car so they crush with it), door mirrors, hood scoop / vents, a minimal interior, the underbody, style
  details, mechanical parts (`mechanical/`: engine bay, suspension, exhaust, frame) and per-wheel
  tire / rim (`wheels/`) / brake disc / caliper / upright parts, all from the resolved
  `mechanical::car_spec` (per-id table + style overrides). The underbody plate stays at floor
  height between the wheels, so the engine bay is open from above. Paint parts get livery-atlas
  UVs; material slots incl. `rim`, `spring`, `accent`, `underglow` are listed in
  `data/panels/README.md`.
- **Builder** (`builder.rs`): `PartKey` + `Builder` with `cuboid`, `beveled_box`, `oriented_box`,
  `cylinder`, `torus`, `lathe`, `extrude`, `surface_patch`, `poly`, `mirror_part`; drops
  degenerate triangles; unit tests check every closed primitive faces outward.
- Deterministic (no RNG; `wp_math::sin_cos`). Metres, +Y forward, +Z up, origin = the chassis
  origin.

### Triangle budgets (`Tier::tri_budget`, checked by `validate-car-asset`)
Hero (performance + police, and classes Sport / Muscle / Exotic / Police*): 8k–20k. Traffic
(traffic module; classes Beater / Compact / Truck): 4k–10k. Current output (2026-10-21, mesh
format 4 after the round-12 × detailed-car-models merge; the fold bindings add no triangles):

| car | tris | car | tris | car | tris |
|---|---|---|---|---|---|
| arashi_t4 | 16916 | ironside_440 | 18699 | sentinel_vigil | 17715 |
| brigand_400 | 19772 | junkyard_beater | 7960 | sentinel_warden | 17770 |
| bulwark_400 | 19186 | kestrel_ts | 17548 | sentinel_wolfhound | 17137 |
| debug_coupe | 16617 | lindwurm_4t | 16246 | shiro_turbo | 17918 |
| debug_sedan | 8422 | lucciola_350 | 17242 | sperber_32 | 17772 |
| drayman_cargo | 9435 | marten_20 | 15796 | stoat_16v | 15610 |
| fulmine_v10 | 18738 | rampart_50 | 18826 | talus_20t | 17062 |
| furrow_24 | 8490 | satchel_20 | 8330 | tansy_13 | 7192 |
| hakone_gt | 17829 | sentinel_bastion | 17191 | tumbril_20 | 8503 |
| hauler_1500 | 9781 | sentinel_cruiser | 16963 | vireo_xs | 15731 |

Headroom: hero cars sit at 15.6k–19.8k of 20k (`brigand_400` 19.8k with 22 wheel segments),
`hauler_1500` at 9.8k of 10k (drop its tire lugs
or wheel segments first if traffic details push it over; see `wave2_mechanical.md`).

### Mesh format 4
`MESH_FORMAT = 4` = `claude/detailed-car-models`' format 2 (part kinds / materials, `rigid`,
`tints`, `livery`, `atlas`, `wheel_camber_deg`, livery atlas UVs) + `lead/round12`'s format 3
(`MeshPart::fold` crumple binding on hood / trunk / front fenders / rear quarters, head / tail
lamps in `FrameFront` / `FrameRear` bound to their end of the car). Field names are those of the
two sides; `data/panels/README.md` documents every field. Validation and tests accept format 4
only.

### Styles of the round-12 roster cars
Variants of an existing family (same body kit, so the same lamp shapes / tail pattern as the
family car; their own body knobs, paint, wheels and mechanical spec):

| car | archetype (`analog_of`) | style |
|---|---|---|
| `stoat_16v` | early-90s FF 3-door hot hatch | `hatch::stoat` (vireo family) |
| `marten_20` | early-90s FF hot hatch | `hatch::marten` (vireo family) |
| `arashi_t4` | mid-90s AWD rally saloon | `tuner::arashi` (kestrel family, AWD spec) |
| `talus_20t` | late-90s AWD rally saloon | `tuner::talus` (kestrel family, AWD spec) |
| `bulwark_400` | early-70s US four-door muscle sedan | `muscle::bulwark` (ironside family) |
| `brigand_400` | mid-70s US four-door sedan | `muscle::brigand` (ironside family, dog-dish wheels) |
| `satchel_20` / `tumbril_20` | 2000s FWD high-roof panel van | `traffic::euro_panel_van` (drayman van kit, I4 / strut / torsion beam) |

## bind-skin (`src/skin.rs`)
4 nearest nodes of the part's own region, weights ∝ 1/(d² + 1e-4), quantised to 1/255 summing
to 255. Region-aware: a hood vertex never follows a fender node. Regions with fewer than four
nodes (glass, 2-door side strips) borrow from their fallback regions (roof/cabin, quarter).
Head / tail lamps (light parts in `FrameFront` / `FrameRear`, `skin::is_end_lamp`) bind to the 4
nearest nodes of their end of the car (bumper, hood/trunk, same-side fender/quarter, frame), so
they crush and move with the bumper and hood; other lamps (fog lamps, DRLs, light bars, markers)
bind like any part of their region. Hood and trunk lids, the front fenders and the rear quarters
also get a crumple binding (`bind_fold`, the `fold` field): fixed-row → leading-row node chords,
front-structure lead nodes per chord, a visual gain and per-vertex crease, lift and follow
weights (see `data/panels/README.md`). `skin::deform` / `skin::deform_detail` and
`crumple::render_part` are the Rust reference of the browser skinning (tests use them).
Rigid parts (`rigid: true`) bind every vertex to the candidate node nearest the part centroid
(weight 255). Writes into the mesh file (not `LatticeDef::skin_binding`, which would bloat the
lattices; the contract field stays available for hand-made meshes).

## validate-car-asset (`src/validate.rs`)
ids and lattice match, format (4 only), per-tier triangle budget, rigid parts bound to one node
(255), wheel-space kinds (`tire rim brake_disc caliper upright`) unskinned with a wheel index,
unique lower_snake_case names, index ranges, every skinned vertex bound to allowed nodes with
weights summing to 255, fold chords inside their region with lead nodes outside it and 3 weights
per vertex, four wheels with tire and
rim, every exterior region with lattice nodes has a part, every panel in the spec has a painted
part, glass parts use glass, mesh size within the declared `body_size_m` (pivot/scale check).

## Perf
Sim side: see `docs/integration/panels.md` (`panels-bench`, release, 24 cars).

Browser skinning (`render/panels.ts` `PanelBody.update`, per car per frame, includes
`panel_states`; headless Chromium + SwiftShader, cloud 4 vCPU; script: time `update()` 300×
with unchanged nodes / hood nodes moving / every node moving):

| | verts | nodes unchanged | hood dented | all nodes moved |
|---|---|---|---|---|
| before (format 1 meshes, always re-skin) debug_sedan | 2614 | 0.26 ms | 0.27 ms | 0.13 ms |
| before, sentinel_cruiser / hauler_1500 | 2890 / 3090 | 0.13 / 0.15 ms | 0.16 / 0.15 ms | 0.14 / 0.14 ms |
| after (format 2, change-only re-skin, rigid parts by matrix) debug_sedan | 4282 | 0.003 ms | 0.03–0.08 ms | 0.18–0.26 ms |
| after, debug_coupe | 7179 | 0.008 ms | 0.10 ms | 0.33 ms |
| after, sentinel_cruiser / hauler_1500 | 6326 / 4971 | 0.003 / 0.003 ms | 0.05 / 0.05 ms | 0.28 / 0.23 ms |

An undamaged or settled car now costs ~nothing; only parts touching moved nodes are re-skinned.
The worst case (every node moving every frame, e.g. a car being crushed) scales with the ~2×
vertex count. `panels_shots.mjs` reports 0.14 ms per car per frame during its crash scenarios.

## Known gaps
- The eight round-12 roster cars reuse their family's body kit, so each shares the family car's
  lamp shapes and tail-lamp pattern (the "every car has a distinct tail pattern" rule does not
  hold for them yet).
- Shapes are parametric (`BodyStyle`), but the lattice still fixes the panel zones; very different
  cabins (mid-engine) use the visual `cabin_shift_m`, so on lucciola / fulmine the windshield sits
  over lattice hood nodes and the hood lid is short of its hinge line. Hand-made meshes in the
  same format still work per car (`bind-skin` binds them).
- Wheels, brakes, engines and suspension are the wave-2 mechanical parts (not placeholders); their
  open items are in `docs/design/vehicles/wave2_mechanical.md` ("Requests for the lead").
  Optional table-only fields (`lip_polished`, `missing_cap`, cover finish, `frame_rails`) are not
  `WheelStyle` / `MechStyle` knobs yet.
- No per-region convex hulls for debris (the sim uses the lattice hull of the detached nodes).
- Wheel wells are open (no inner arch liner); fine with the dark interior colour.
