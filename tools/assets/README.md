# car-assets (`tools/assets`) — car models and panels pipeline

Owned by workstream R (prompt 21, `ws/panels`). Produces, for every car in `data/cars`, a
panel spec and a procedural low-poly mesh whose parts are skinned to the car's damage lattice,
and checks them. See `data/panels/README.md` for the file formats and
`docs/integration/panels.md` for how the pieces fit together.

```sh
cargo run -p car-assets --bin gen-panels            # data/panels/<car>.json (first pass from the lattice)
cargo run -p car-assets --bin gen-panels -- --check # exit 1 if a spec differs from the derived one
cargo run -p car-assets --bin gen-car-mesh          # data/panels/meshes/<car>.json (+ bind-skin) and meshes/mid/<car>.json
cargo run -p car-assets --bin gen-car-mesh -- --check  # exit 1 if a mesh is stale
cargo run -p car-assets --bin bind-skin -- data/panels/meshes/<car>.json [data/lattices/<id>.json]
cargo run -p car-assets --bin validate-car-asset [car ...]
cargo run --release -p car-assets --bin panels-bench # perf, 24 cars
cargo run --release -p car-assets --example panel_probe -- apps/simcli/scenarios/panels/side_swipe_door.ron
cargo run -p car-assets --example profile_dump -- [--ring <y>] <car>  # styled anchors, regions, a cross-section
cargo run --release -p car-assets --example phase3_demo -- <dir>  # phase 3 knob demo meshes (<car>__<variant>.json)
cargo test -p car-assets                             # every car valid + meshes up to date + turn-signal contract
cargo run --release -p car-assets --example crumple_matrix -- Arcade [frame ticks]  # hood/fender/bumper crumple, 30/50/70 km/h
node tools/assets/panels_shots.mjs                   # browser checks + screenshots (docs/media/panels)
node tools/assets/mesh_shots.mjs <dir> <car> ...      # rest-pose shots: front / rear / side / top / cabin / cabin_rear
                                                     #   (+ SHOT_CAM=x,y,z SHOT_AT=x,y,z → <car>_custom.png;
                                                     #    MESH_DIR=<dir> renders <dir>/<car>.json)
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

## Mesh generator (`src/{style,profile,mesh,trim,builder,wheels,mechanical}.rs`, `src/styles/`)
- **Style** (`style.rs`): `style_for(def)` asks `styles::{performance, traffic, police}` (first
  `Some` wins) else `class_default` (Truck → Pickup / Van, PoliceSuv → Suv, others the default
  notchback; engine layout / exhaust by class). A `CarStyle` = `BodyStyle` (≈45 shape knobs,
  documented with units and ranges; `Default` = the original look) + `details` + `WheelStyle` +
  `MechStyle` + optional `LiveryHint` + `tints` + `accent_rgb` / `underglow_rgb` + budget `Tier`.
  (≈70 knobs since the car-shapes pass; their defaults give every car framed glass and trim.)
  Styles live in `src/styles/{performance/,traffic.rs,police.rs + police/}`; every knob clamps
  softly (`profile::clamp_soft`), so extreme values never panic (`tests/assets.rs` sweeps them).
- **Body profile** (`profile.rs`) from the `VehicleDef` (width, wheels → arches) anchored to the
  lattice (hood y-range and heights, firewall, door hinge and latch stations, roof, rear glass,
  deck, quarters, bumpers), then styled (cabin shift / hood length, roof edges from glass rakes,
  heights, overhangs, open pickup bed). Panel zones stay on lattice zone boundaries, so every
  panel mesh covers its lattice region's zone; doors and the trunk start at their hinge line, the
  hood at the windshield base. `cabin_shift_m` / `hood_length` move the visual hood rear edge and
  the rear-glass base on purpose (the panel still pivots on its lattice hinge nodes; at most 0.6 m
  away, checked by `tests/assets.rs`). Greenhouse / top shape: `roof_arc` (0 = straight
  windshield, flat roof, straight rear glass; 1 = one smooth arc), `c_pillar_rake_deg` (teardrop
  canopy), `hood_valley` / `deck_valley` (centre sunk between fender / haunch tops). `cabin_shift_m` spans −0.6…+0.6 m: the mid-engine cars use
  +0.40 / +0.45 for a cab-forward roof (centre ≈ 46 % of the length from the nose).
- **Loft** (`mesh.rs`): cross-section rings of 18 points per side (sill tuck, rocker, character
  crease, shoulder, belt ledge, then either the greenhouse — side glass 8→10, roof rail /
  A- / C-pillar band 10→13, roof or glass 13→17 — or a flat top / open bed) at adaptive
  stations: hero 7 cm (3.6 cm near the nose, tail and arches), traffic 11 cm (6.5 cm), plus every
  zone boundary, window and pillar edge and the cowl edge. The flat top uses the greenhouse's x
  positions and a ring blends from flat to greenhouse over its first 5 cm of cabin height
  (`BodyProfile::greenhouse_weight`), so the windshield base and the deck joins have no fans of
  thin faces. Side points under a wheel arch are lifted onto the arch (round or box) with the
  flare lip; haunches swell the sides around the arches; the first and last rings are set in by
  `end_round_m` (rounded nose / tail face edges, no extra triangles).
- **Greenhouse** (car-shapes, 2026-10-27; knobs in `BodyStyle` "Greenhouse framing"): body-colour
  A-pillars `a_pillar_w_m` wide seen from the front following the windshield rake, C/D-pillars
  `c_pillar_w_m`; the windshield wraps back at the corners (`windshield_wrap_m`, as a corner drop
  on the raked glass, capped at `profile::WRAP_DROP_MAX_M`) above a black cowl `cowl_len_m` long;
  front door glass runs forward under the A-pillar to a black sail (`a_sail_len_m`, `a_sail`) that
  carries the mirror; B-pillar `b_pillar_w_m` wide in black or body colour (`b_pillar`); roof rail
  drop `roof_rail_drop_m`. Every glass panel sits `glass_recess_m` behind the surface and
  `glass_frame_m` inside its opening, with a black `<glass>_seal` part closing the step.
- **Common trim** (`trim.rs`; knobs in `BodyStyle` "Body details"): mirrors on the sail
  (`mirrors`), door handles (`door_handles`), panel seams (`panel_seams`, `seam_w_m`: door cut
  lines, hood / trunk shut lines, bumper splits), fuel door (`fuel_door`), arch lips
  (`arch_lip_m`) and black arch liners (`arch_liners`), drip-rail line (`drip_rail`), wipers
  (`wipers`). Style kits that draw their own pieces remove the generic ones first
  (`trim::remove_generic` with `GENERIC_MIRRORS` / `GENERIC_HANDLES` / `GENERIC_FUEL_DOOR` /
  `GENERIC_DRIP_RAIL` / `GENERIC_WIPERS`; `Kit::new`, `Kit::door_handles`, `Kit::fuel_filler`,
  traffic `handles` / `drip_rails` / `black_mirrors` / `tall_mirrors` do). Kit and traffic mirrors
  are placed from the sail (`BodyProfile::sail`) too.
- **Phase 3 shape knobs** (car-shapes phase 3, 2026-10-27; `BodyStyle` "Phase 3 shape knobs",
  defaults change nothing): `plan_width` (plan-view half-width curve, never inside a tire),
  `side_sculpt` (`SideSculpt` boxes: pods, coves, pontoons, shoulder lines), `side_bulge_m` /
  `side_bulge_z` (convex section), `rocker_in_m` (sill tuck-in), `tumblehome_m` (0.03…0.36),
  `front_fender_rise_m` / `rear_fender_rise_m` / `fender_peak_w` (fender tops above the hood
  centre), `overhang_trim` widened to −0.35…+0.25 (short noses move the visual hood front back,
  keeping a 10 cm bumper strip). How-to per knob: `docs/integration/car-shapes.md` "Phase 3 knobs".
- **Projected lamps** (`src/lamp.rs`, `Kit::proj_cluster`): a 2D outline in a front / rear /
  side / top view cast onto the body triangles already built, so the lens sits `proud_m` over the
  drawn surface (wraps round corners with `yaw_deg`, lies on curved noses and fender tops); the
  turn signal is cut out of the same outline; optional flush bezel. Scratch demo:
  `cargo run --release -p car-assets --example phase3_demo -- <dir>`, render with
  `MESH_DIR=<dir> node tools/assets/mesh_shots.mjs <out> <car>__<variant>`.
- **Parts**: each loft face is classified by station and segment (lower side / belt ledge /
  greenhouse / top / bed) into a body region: bumpers, hood, fenders, doors (door windows ride with
  their door), quarter glass, sills, roof, windshield, rear glass (or cab-back wall), trunk or
  deck, quarters, pillars, open bed floor (`bed_floor`, FrameRear). Plus head / tail lights on the
  nose / tail faces (`LIGHT_PROUD_M` proud of the surface; region `FrameFront` / `FrameRear`,
  bound to their end of the car so they crush with it), each with its turn signal cut out of
  the same lens (`indicator_front_*` / `indicator_rear_*`, optional `indicator_side_*` fender or
  mirror repeaters; contract and per-family layouts in `data/panels/README.md` "Turn signals";
  helpers `Kit::{front_cluster, rear_cluster, top_lamp_split, fender_repeaters,
  mirror_repeaters}`, traffic `reshape_indicators`), door mirrors, hood scoop / vents, a minimal interior, the underbody, style
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
Hero (performance + police, and classes Sport / Muscle / Exotic / Police*): 8k–22k. Traffic
(traffic module; classes Beater / Compact / Truck): 4k–11k. Raised from 20k / 10k on 2026-10-27
by the car-shapes pass (framed inset glass with seals, pillars, cowl, common trim: +0.8k–1.5k per
car; the heaviest hero cars were already at 19.8k of 20k). Measured render cost (desktop chase,
1280×720, low-poly dark, headless SwiftShader, `e2e/frame_profile.mjs chase 12`): old meshes
340k triangles / 409 draw calls / 227 ms per frame / RTF 0.91; new meshes 347k–350k / 413–417 /
233–252 ms / RTF 0.87–0.90 (gate: triangles ≤ 400k, draw calls < 600). Seals, seams and trim
are batched with the other `trim` / paint parts, so they add almost no draw calls.
`every_car_has_turn_signals_integrated_into_its_lamps` keeps the +2 % cap on the counts below.
Current output (2026-10-27, in brackets the change from the 2026-10-26 meshes):

| car | tris (vs 10-26) | car | tris (vs 10-26) | car | tris (vs 10-26) |
|---|---|---|---|---|---|
| arashi_t4 | 18292 (+1112) | junkyard_beater | 8836 (+868) | satchel_20 | 9221 (+859) |
| brigand_400 | 20986 (+1214) | kagero_hx | 18949 (+950) | sentinel_bastion | 18932 (+1501) |
| bulwark_400 | 20579 (+1393) | kestrel_ts | 18914 (+1102) | sentinel_cruiser | 18497 (+1294) |
| debug_coupe | 17927 (+1126) | lindwurm_4t | 17474 (+1044) | sentinel_vigil | 19100 (+1145) |
| debug_sedan | 9605 (+1151) | lucciola_350 | 18170 (+898) | sentinel_warden | 19111 (+1101) |
| drayman_cargo | 10488 (+1021) | marten_20 | 17056 (+1140) | sentinel_wolfhound | 18626 (+1249) |
| erg_hamada | 19872 (+1426) | merlo_20t | 19317 (+1065) | shiro_turbo | 18992 (+1010) |
| fulmine_v10 | 19884 (+1098) | mesquite_pr | 20223 (+882) | sperber_32 | 19112 (+1076) |
| furrow_24 | 9414 (+916) | morrigan_55 | 20454 (+1073) | stoat_16v | 16894 (+1164) |
| hakone_gt | 18873 (+1100) | nyx_proto | 19674 (+1126) | talus_20t | 17376 (+1044) |
| halberd_40 | 19804 (+1010) | ombra_v12 | 19286 (+926) | tansy_13 | 8214 (+994) |
| hauler_1500 | 10625 (+812) | ostwind_6t | 18306 (+934) | tumbril_20 | 9418 (+883) |
| ironside_440 | 19809 (+1110) | rampart_50 | 19954 (+1088) | vireo_xs | 17065 (+1214) |

Headroom: hero cars sit at 16.9k–21.0k of 22k, traffic cars at 8.2k–10.6k of 11k.

### Mid LOD (`Lod::Mid`, `data/panels/meshes/mid/<car>.json`)
`gen-car-mesh` writes a second, unskinned mesh per car for the browser's instanced far cars
(fleet near tier, nearest parked cars): `mesh::generate_mid` = `generate_lod(.., Lod::Mid)`, the
same style at `MID_LOFT_STEPS` (0.20 m / 0.12 m), `MID_WHEEL_SEGMENTS` (12) wheels,
`MID_PATCH_STEP_M` (16 cm) surface patches and traffic-tier mechanical detail, filtered by
`keep_in_mid` (exterior panels, glass, lights, mirrors, light bars, tires, rims, exhaust tips,
details ≥ `MID_MIN_DETAIL_M` = 0.2 m); details, rims, lamps, light bars and mirrors are then
decimated by vertex clustering (`cluster_part`, `MID_CLUSTER_M` = 3.5 cm grid, vertices snap
to cell centres so touching parts stay closed; atlas seams kept apart by a UV bucket). Atlas,
livery hint, tints and atlas UVs equal the full mesh's. Budget `MID_TRI_BUDGET` 2k–7k (6.5k until
2026-10-27); checked
by `every_car_has_an_up_to_date_mid_lod` (also: up to date, a subset of the full mesh's parts
with the same kinds / materials, every panel / glass / light / tire kept). Format: see
`data/panels/README.md` "Mid LOD". Current output:

| car | mid (full) tris | car | mid (full) tris | car | mid (full) tris |
|---|---|---|---|---|---|
| arashi_t4 | 4962 (18292) | junkyard_beater | 4473 (8836) | satchel_20 | 4650 (9221) |
| brigand_400 | 6042 (20986) | kagero_hx | 5131 (18949) | sentinel_bastion | 5346 (18932) |
| bulwark_400 | 5621 (20579) | kestrel_ts | 5051 (18914) | sentinel_cruiser | 4728 (18497) |
| debug_coupe | 4793 (17927) | lindwurm_4t | 4680 (17474) | sentinel_vigil | 4994 (19100) |
| debug_sedan | 4407 (9605) | lucciola_350 | 4674 (18170) | sentinel_warden | 5575 (19111) |
| drayman_cargo | 5248 (10488) | marten_20 | 4160 (17056) | sentinel_wolfhound | 5183 (18626) |
| erg_hamada | 5912 (19872) | merlo_20t | 5151 (19317) | shiro_turbo | 5040 (18992) |
| fulmine_v10 | 5530 (19884) | mesquite_pr | 6530 (20223) | sperber_32 | 4942 (19112) |
| furrow_24 | 4496 (9414) | morrigan_55 | 5648 (20454) | stoat_16v | 3938 (16894) |
| hakone_gt | 4826 (18873) | nyx_proto | 6025 (19674) | talus_20t | 4464 (17376) |
| halberd_40 | 5111 (19804) | ombra_v12 | 5268 (19286) | tansy_13 | 4002 (8214) |
| hauler_1500 | 5411 (10625) | ostwind_6t | 5506 (18306) | tumbril_20 | 4709 (9418) |
| ironside_440 | 5451 (19809) | rampart_50 | 5380 (19954) | vireo_xs | 3921 (17065) |

Mid LODs: 3921–6530 triangles (full meshes 8214–20986). The mid LOD's glass is flush (no seals) and it
skips seams, handles, the fuel door, arch lips, the drip rail and wipers. The four `parity_*` cars' mid files are copies of their template car's
(`tools/parity/build_parity_cars.py`; after a regeneration refresh the copies with its
`copy_panels(spec)` for each `CARS` entry, or rerun the script).

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
- Wheel wells: black arch liners close the openings (`arch_liners`); there is no full inner well.
- Per-car greenhouse styling is still to do: every car uses the default pillar widths, wrap and
  trim finishes; some style kits' own overlays (black A-pillar strokes, roof sheets on
  `kagero_hx` / `nyx_proto` / supercars) were tuned for the old frameless glass.
- Where the cabin meets the deck (C-pillar foot) the side faces still show a few small facets.
