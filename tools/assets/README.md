# car-assets (`tools/assets`) — car models and panels pipeline

Owned by workstream R (prompt 21, `ws/panels`). Produces, for every car in `data/cars`, a
panel spec and a procedural low-poly mesh whose parts are skinned to the car's damage lattice,
and checks them. See `data/panels/README.md` for the file formats and
`docs/integration/panels.md` for how the pieces fit together.

```sh
cargo run -p car-assets --bin gen-panels            # data/panels/<car>.json (first pass from the lattice)
cargo run -p car-assets --bin gen-panels -- --check # exit 1 if a spec differs from the derived one
cargo run -p car-assets --bin gen-car-mesh          # data/panels/meshes/<car>.json + RENDER_LIST.md (+ bind-skin)
cargo run -p car-assets --bin gen-car-mesh -- --check  # exit 1 if a mesh or the render list is stale
cargo run -p car-assets --bin car-profile [car ...] # anchors + lamp windows to author a style sheet against
cargo run -p car-assets --bin bind-skin -- data/panels/meshes/<car>.json [data/lattices/<id>.json]
cargo run -p car-assets --bin validate-car-asset [car ...]
cargo run --release -p car-assets --bin panels-bench # perf, 24 cars
cargo run --release -p car-assets --example panel_probe -- apps/simcli/scenarios/panels/side_swipe_door.ron
cargo run -p car-assets --example ring_dump -- <car> <y> ...  # debug: cross-sections at stations
node tools/assets/gallery.mjs [--night] [--views front34,rear34,side,front,rear,top] [car ...]
                                                     # headless renders (needs apps/testbed-web node_modules)
cargo test -p car-assets                             # every car valid + meshes and render list up to date
```

## Making a car look like itself
Every car has a **style sheet** `data/panels/styles/<car>.toml` (`src/style.rs` documents every
key; every key has a default, so a sheet lists only what differs). Workflow:

1. `car-profile <car>` prints the lattice anchors (nose/tail y, hood, belt, roof, deck, doors,
   bumper tops, wheels) and, per lateral offset, the **lamp window**: the z range the nose / tail
   face covers 6 cm in. Lamps and grilles inside the window sit flat on the face.
2. Write `[identity]` (tagline, personality, silhouette, signatures, lighting — this is the render
   list text), `[palette]`, `[body]` shape knobs, `[[lamp]]` entries, `[parts]` and `[wheels]`.
3. `gen-car-mesh <car>`; it warns when a lamp outline slides off the surface (clamped) or misses
   the body. `node tools/assets/gallery.mjs <car>` renders it; iterate.
4. Run `gen-car-mesh` with no arguments before committing (it rewrites `RENDER_LIST.md`).

## Mesh generator (`src/profile.rs`, `src/shell.rs`, `src/mesh.rs`, `src/parts.rs`, `src/wheels.rs`)
- **Anchors** from the `VehicleDef` (width, wheels → arches) and the lattice: hood y-range and
  heights, firewall, door hinge and latch stations, roof, rear glass, deck, quarter and bumper
  stations. Every panel's mesh covers exactly its lattice region's zone and hinged panels start at
  their hinge line (`tests/assets.rs` checks this for every car). Style rakes move only the
  *visible* roof ends (the glass takes over), never a door or lid edge.
- **Shape** (`BodyProfile` + style `[body]`): nose/tail rounding length, plan loss, drop, lift and
  sharpness; deck kick; tumblehome; roof width, crown, bow; windscreen / rear-glass bow and rake;
  character crease; shoulder and rocker chamfers; front/rear flares; arch gap and outline (round,
  flat-top, square); hood bulge; belt rise and kick; beds (`open`, `box`, `rails`, `tipper`)
  with gap, height, half width and a raised floor; blind rear (no side glass); body-colour or
  black pillars.
- **Loft** (`Shell::ring`): 11-point half cross-sections every 7–13 cm (coarser on long vehicles)
  plus every zone boundary: rocker, lower side, crease, shoulder, ledge, greenhouse side, roof
  edge, crown, centre line. Faces are classified by station and band into regions: bumpers, hood,
  fenders, doors, door glass (rides with its door), fixed quarter glass, pillars, sills, roof,
  windscreen, rear glass / cab back, trunk lid or deck, quarters, bed / box / rails / tipper
  parts. End caps close nose and tail (tailgates and box doors are the trunk region).
- **Surface queries** (`Shell::{front_y, rear_y, side_x, top_z, end_path}`): every lamp, plate,
  grille, handle and bumper bar is projected onto the face it sits on (lens fan + skirt, optional
  bezel); points that would slide more than 8 cm along the surface are clamped and reported.
- **Parts** (`src/parts.rs`): lamps by kind → material (`headlight`, `taillight`, `indicator`,
  `lens`, `beacon_red`, `beacon_blue`), each skinned to the region it is bolted to (front lamps to
  the fenders so a hanging bumper leaves them; below the bumper top to the bumper; police push
  bars to `Accessory` when the lattice has it). Details are kind `detail`.
- **Wheels** (`src/wheels.rs`): tire with a rounded shoulder; rim designs `star5`, `spoke10`,
  `mesh`, `six`, `turbine`, `dish`, `steel`, `hubcap`, `truck` in a paint / chrome / metal /
  black finish; a dark backing disc; dual rear tires; visual extra axles (`hub_offset_mm`) that
  the renderer spins with the physics wheel; raised white letters.
- Budgets: 3k–9k tris for cars, up to 12k for vehicles longer than 5.6 m. Metres, +Y forward,
  +Z up, origin = the chassis origin. Deterministic (no RNG; `wp_math::sin_cos`).

## bind-skin (`src/skin.rs`)
4 nearest nodes of the part's own region, weights ∝ 1/(d² + 1e-4), quantised to 1/255 summing
to 255. Region-aware: a hood vertex never follows a fender node. Regions with fewer than four
nodes (glass, 2-door side strips) borrow from their fallback regions (roof/cabin, quarter).
Writes into the mesh file (not `LatticeDef::skin_binding`, which would bloat the lattices; the
contract field stays available for hand-made meshes).

## validate-car-asset (`src/validate.rs`)
ids and lattice match, format, triangle budget, unique lower_snake_case names, index ranges,
every skinned vertex bound to allowed nodes with weights summing to 255, four wheels with tire and
rim, every exterior region with lattice nodes has a part, every panel in the spec has a painted
part, glass parts use glass, mesh size within the declared `body_size_m` (pivot/scale check).

## Perf
See `docs/integration/panels.md` (measured with `panels-bench`, release, 24 cars).

## Render list (`src/render_list.rs`)
`data/panels/RENDER_LIST.md`: per car the style sheet's identity (tagline, personality,
silhouette, lighting, signatures), palette and wheels, and the generated mesh's parts grouped
(body panels, glass, lamps, mirrors, bolt-on details, structure & trim, interior, wheels) with
part and triangle counts.

## Known gaps
- Shapes are procedural lofts: a style sheet shapes the body between the lattice anchors but
  cannot move them; the lattice generator's fixed profile fractions decide where the cabin sits.
  Hand-made meshes in the same format still replace these per car (`bind-skin` binds them).
- No per-region convex hulls for debris (the sim uses the lattice hull of the detached nodes).
- Wheel wells are open (no inner arch liner); fine with the dark interior colour.
- No trailer (needs a hitch joint in the sim); the semi tractor runs bobtail.
