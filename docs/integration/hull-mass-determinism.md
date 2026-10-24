# Hull mass determinism integration notes

- **Workstream / prompt:** fix — native vs wasm split on dented parked cars and torn-off parts
- **Branch:** `claude/hull-mass-determinism` (PR into `lead/round12`)
- **Crates / folders touched:** `wp_damage` (`hull.rs`, `debris.rs`), `wp_sim` (`parked.rs`)
- **Contract version built against:** unchanged (no contract change)

## What was wrong
`tools/wasm-determinism/compare.sh` failed on `jam/pursuit_jam_grid.ron` (native and wasm hashes
split from tick 109920; red on `lead/round12` since cae4f20f). Exact-bit dumps on both targets
traced it to one parked car:

1. Tick 109722: a police car had dented the parked car, so `parked.rs` swapped its box collider
   for the dented convex hull (`col.set_shape`).
2. The collider was built with `.mass(m)`, so Rapier recomputed its mass properties from the
   hull. The centre of mass and mass came out bit-identical; the principal inertia and its
   frame did not (1 ULP). A hull's inertia tensor is not diagonal, and Rapier gets its principal
   axes from `glamx` 0.3.1 `SymmetricEigen3` (`src/eigen3.rs`), which calls `f32::acos` and
   `f32::cos`: the platform libm natively, Rust's libm on wasm32. `glamx` has no `libm`
   feature. Diagonal tensors (boxes, cylinders) skip that branch, which is why only hulls hit it.
3. Tick 109723 the parked car moved differently; tick 109725 the police car pushing it
   (contact impulse), then its unit, then the state hash.

Row 158 did not cause it; it changed police spawns so a police car ended up pushing a dented
parked car.

## Fix
`wp_damage::hull::box_mass_properties(center, half_extents, mass)` (and
`bounding_box_mass_properties(points, mass)`) build axis-aligned box mass properties with a
diagonal inertia, so Rapier never runs the eigen-decomposition:

- `wp_sim/src/parked.rs`: a dented parked car's hull collider keeps the box's mass properties
  (its mass, centred, `PARKED_CAR_HALF_EXTENTS` inertia) via `set_mass_properties`.
- `wp_damage/src/debris.rs`: a torn-off panel's hull collider gets the box mass properties of
  its hull points' bounding box instead of `.mass(m)`.

Vehicle chassis hulls (`Sim` hull swap in `lib.rs`) are not affected: the chassis collider has
density 0 (the body carries explicit mass properties), and a zero tensor skips the trig branch.

## Changes outside my crates
All of it: `wp_damage` (damage workstream) and `wp_sim` (lead). Behaviour change: dented parked
cars keep box inertia (centred) instead of the hull's, and loose panels get bounding-box inertia;
goldens with those collisions were re-blessed (`[bless]` commit).

## Known gaps
Any future dynamic collider whose mass properties Rapier derives from a non-box shape (hull,
trimesh, compound) reintroduces this. Give such colliders explicit mass properties.
