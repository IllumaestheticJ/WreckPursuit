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
```

Wave-2 style / mechanical authors: see `docs/design/vehicles/PLAN.md` "Foundation API (for wave 2)".

## Mesh generator (`src/{style,profile,mesh,builder,wheels,mechanical}.rs`, `src/styles/`)
- **Style** (`style.rs`): `style_for(def)` asks `styles::{performance, traffic, police}` (first
  `Some` wins) else `class_default` (Truck → Pickup / Van, PoliceSuv → Suv, others the default
  notchback; engine layout / exhaust by class). A `CarStyle` = `BodyStyle` (≈45 shape knobs,
  documented with units and ranges; `Default` = the original look) + `details` + `WheelStyle` +
  `MechStyle` + optional `LiveryHint` + `tints` + budget `Tier`.
- **Body profile** (`profile.rs`) from the `VehicleDef` (width, wheels → arches) anchored to the
  lattice (hood y-range and heights, firewall, door hinge and latch stations, roof, rear glass,
  deck, quarters, bumpers), then styled (cabin shift / hood length, roof edges from glass rakes,
  heights, overhangs, open pickup bed). Panel zones stay on lattice zone boundaries, so every
  panel mesh covers its lattice region's zone; doors and the trunk start at their hinge line, the
  hood at the windshield base. `cabin_shift_m` / `hood_length` move the visual hood rear edge and
  the rear-glass base on purpose (the panel still pivots on its lattice hinge nodes; at most 0.6 m
  away, checked by `tests/assets.rs`).
- **Loft** (`mesh.rs`): cross-section rings of 18 points per side (sill tuck, rocker, character
  crease, shoulder, belt ledge, 3-point greenhouse, 7-point crowned top / open bed) at adaptive
  stations: hero 7 cm (3.6 cm near the nose, tail and arches), traffic 11 cm (6.5 cm), plus every
  zone boundary and window edge. Side points under a wheel arch are lifted onto the arch (round or
  box) with the flare lip; haunches swell the sides around the arches.
- **Parts**: each loft face is classified by station and segment (lower side / belt ledge /
  greenhouse / top / bed) into a body region: bumpers, hood, fenders, doors (door windows ride with
  their door), quarter glass, sills, roof, windshield, rear glass (or cab-back wall), trunk or
  deck, quarters, pillars, open bed floor (`bed_floor`, FrameRear). Plus head / tail lights on the
  nose / tail faces (proud of the surface; bound to the fenders / quarters so a hanging bumper
  leaves them), door mirrors, hood scoop / vents, a minimal interior, the underbody, style
  details, mechanical parts (`mechanical.rs`) and per-wheel tire / rim (`wheels.rs`) / brake disc /
  caliper / upright parts. Paint parts get livery-atlas UVs (`data/panels/README.md`).
- **Builder** (`builder.rs`): `PartKey` + `Builder` with `cuboid`, `beveled_box`, `oriented_box`,
  `cylinder`, `torus`, `lathe`, `extrude`, `surface_patch`, `poly`, `mirror_part`; drops
  degenerate triangles; unit tests check every closed primitive faces outward.
- Deterministic (no RNG; `wp_math::sin_cos`). Metres, +Y forward, +Z up, origin = the chassis
  origin.

### Triangle budgets (`Tier::tri_budget`, checked by `validate-car-asset`)
Hero (performance + police, and classes Sport / Muscle / Exotic / Police*): 8k–20k. Traffic
(traffic module; classes Beater / Compact / Truck): 4k–10k. Foundation output (2026-10-21, before
wave-2 styles):

| car | tris | car | tris | car | tris |
|---|---|---|---|---|---|
| debug_coupe (styled example) | 9413 | ironside_440 | 9524 | sentinel_bastion | 10092 |
| debug_sedan | 6178 | junkyard_beater | 6070 | sentinel_cruiser | 9308 |
| drayman_cargo | 6590 | kestrel_ts | 9078 | sentinel_wolfhound | 9561 |
| furrow_24 | 6280 | lindwurm_4t | 8698 | shiro_turbo | 8906 |
| hakone_gt | 8661 | lucciola_350 | 8654 | sperber_32 | 8934 |
| hauler_1500 | 7045 | rampart_50 | 9270 | tansy_13 | 5548 |
| | | | | vireo_xs | 5671 |

## bind-skin (`src/skin.rs`)
4 nearest nodes of the part's own region, weights ∝ 1/(d² + 1e-4), quantised to 1/255 summing
to 255. Region-aware: a hood vertex never follows a fender node. Regions with fewer than four
nodes (glass, 2-door side strips) borrow from their fallback regions (roof/cabin, quarter).
Rigid parts (`rigid: true`) bind every vertex to the candidate node nearest the part centroid
(weight 255). Writes into the mesh file (not `LatticeDef::skin_binding`, which would bloat the lattices; the
contract field stays available for hand-made meshes).

## validate-car-asset (`src/validate.rs`)
ids and lattice match, format, per-tier triangle budget, rigid parts bound to one node (255),
wheel-space kinds (`tire rim brake_disc caliper upright`) unskinned with a wheel index, unique lower_snake_case names, index ranges,
every skinned vertex bound to allowed nodes with weights summing to 255, four wheels with tire and
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
- Shapes are parametric (`BodyStyle`), but the lattice still fixes the panel zones; very different
  cabins (mid-engine) use the visual `cabin_shift_m`. Hand-made meshes in the same format still
  work per car (`bind-skin` binds them).
- Foundation wheels / mechanical parts are placeholders (star rim, block engine, one strut per
  corner) for the wave-2 mechanical agent to replace.
- Calipers / uprights are not cambered with `wheel_camber_deg` (only the spin group is).
- No per-region convex hulls for debris (the sim uses the lattice hull of the detached nodes).
- Wheel wells are open (no inner arch liner); fine with the dark interior colour.
