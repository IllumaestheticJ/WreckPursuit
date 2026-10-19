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
cargo test -p car-assets                             # every car valid + meshes up to date
```

## Mesh generator (`src/profile.rs`, `src/mesh.rs`)
- **Body profile** from the `VehicleDef` (width, wheels → arches) anchored to the lattice: hood
  y-range and heights, firewall, door hinge and latch stations, roof, rear glass, deck, quarter
  and bumper stations. So every panel's mesh covers exactly its lattice region's zone, and hinged
  panels start at their hinge line (doors and trunk at their front edge, the hood at the
  windshield) — `tests/assets.rs` checks this for every car.
- **Loft**: cross-section rings every 6 cm along +Y (plus every zone boundary) — chamfered sill,
  side, shoulder, greenhouse (tumblehome to the roof width), crowned top — rounded in plan and
  profile at the nose and tail; side points under a wheel arch are lifted onto the arch circle.
- **Parts**: each loft face is classified by station and band (lower side / greenhouse / top)
  into a body region: bumpers, hood, fenders (to the front-door hinge), doors (front door to the
  rear-door hinge; door windows ride with their door), rear doors / 2-door side strips, sills
  (`FrameLeft/Right`), roof, windshield, rear glass (or a cab-back wall), trunk lid or deck,
  quarters, pillars (`Cabin`). Plus head/tail lights (on the fenders / quarters, so a hanging
  bumper leaves them), door mirrors (on the front doors), a minimal interior, the underbody and
  per-wheel tire + star rim parts (hub space, not skinned).
- 3.5k–4.6k triangles per car (budget 3–6k, prompt 11). Metres, +Y forward, +Z up, origin = the
  chassis origin. Deterministic (no RNG; `wp_math::sin_cos`).

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

## Known gaps
- Placeholder shapes: one body profile family (chamfered box + greenhouse); pickups get a closed
  bed cover, hatches a steep rear glass. Hand-made meshes in the same format replace these per car
  (`bind-skin` binds them).
- No per-region convex hulls for debris (the sim uses the lattice hull of the detached nodes).
- Wheel wells are open (no inner arch liner); fine with the dark interior colour.
