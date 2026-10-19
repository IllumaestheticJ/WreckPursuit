# PROMPT 21 — Car models with destructible body panels and hinges (machine R; `ws/panels`)

Read `CLAUDE.md`, `docs/INTEGRATION.md`, `docs/CONTRACT_CHANGELOG.md`, `docs/STATUS.md`,
`crates/wp_damage/README.md` (Detachables section), `prompts/03_damage_model.md`,
`prompts/11_art_direction_pipeline.md` (car mesh pipeline part only) and `data/cars/README.md`.

## Goal
Cars look like cars and come apart panel by panel. Every body panel is its own mesh and dents
with the lattice. Hinged panels (doors, hood, trunk/tailgate) unlatch in hits, then **swing on
their hinges** (open while driving, flap, bounce on their stops). They tear off when the hinge
fails. Bumpers sag and dangle from one side before they fall. All of this is deterministic and
cheap (it sits inside the lattice budget, ≤ 0.25 ms/car native).

## Ownership
You own:
- `crates/wp_damage/src/panels.rs` plus its tests (a new module; B owns the rest of
  `wp_damage`, so keep your hooks into `solver.rs`/`lib.rs` minimal and list them in your doc)
- `data/panels/*.json` (per-car panel specs)
- `tools/assets/` (mesh generator, `bind-skin`, `validate-car-asset`)
- `apps/testbed-web/src/render/` (car panel rendering; C owns the rest of the testbed)
- `docs/integration/panels.md`

Never edit `wp_contracts`. Contract v0.3.0 is being applied right now (`lead/contract-v0.3.0`).
File what you need as "Contract change requests", and use local types or `data/panels` files
in the meantime.

## Work
1. **Panel state machine** (`panels.rs`): per detachable group, the states are
   `Attached → Unlatched (swinging on hinge) → HangingByOne (one hinge broken; bumpers, hoods) → Detached`.
   - Latch failure comes from the existing hinge-impulse integrator and from intrusion at the
     latch nodes.
   - The swing is a 1-DOF hinge angle integrated at the lattice sub-step. It is driven by
     chassis acceleration (inertial), contact impulses on the panel, gravity and aero at speed,
     with angle limits and a restitution "stop". It is kinematic, not Rapier joints, so the cost
     stays tiny and determinism stays easy.
   - The panel's lattice nodes follow the hinge rotation, so the collider and contacts stay right.
   - Detached panels become the existing `DetachedPart` debris, carrying their swing velocity.
   - Use the seeded RNG only if needed, passed in; no wall-clock.
2. **Panel data** (`data/panels/<car>.json`): for every car in `data/cars`, each panel gets a
   region, hinge axis nodes, latch nodes, swing limits, mass and a damping value. Generate the
   first pass from the lattice detachables and add rear doors, fenders and mirrors as new
   detachables where the lattice allows. Validate on load.
3. **Procedural low-poly car meshes** (`tools/assets`, per prompt 11's mesh spec):
   - Generate them from the `VehicleDef` dimensions plus a body profile (chamfered box,
     greenhouse, wheel arches).
   - One mesh per panel/region, plus separate glass, wheels and lights.
   - Units are metres, +Y forward, +Z up; 3–6k tris.
   - Output JSON or binary buffers that the browser can load. `.glb` is not servable by the
     published artifact, so prefer JSON or `.bin` under `data/`.
   - Run `bind-skin` (4 nearest lattice nodes, region-aware weights) so dents show on every
     panel. Every car in the roster gets a mesh. Analog names only.
4. **Browser rendering** (`apps/testbed-web/src/render/`):
   - Draw the panel meshes skinned to lattice nodes, with hinged panels rotating with their
     angle and detached panels as debris.
   - Add the smallest `wp_wasm` accessor you need, for example
     `panel_states(id) -> [region, state, angle]`. List it as a change outside your crates and
     tell C in your doc.
   - Add a debug toggle: "panel meshes / lattice view".
5. **Tests and scenarios**:
   - Unit tests: swing period and stop bounce, unlatch thresholds, a door opening under hard
     braking only once it's unlatched, deterministic hashes.
   - A simcli scenario (`apps/simcli/scenarios/panels/`) with a side swipe that unlatches a
     door that then swings, and a frontal hit that drops the bumper to `HangingByOne`.
     Expectations are ranges.
   - Show the native == wasm hash.
6. Record perf (native release, 24 cars) in your README and doc. Add screenshots in
   `docs/media/panels/` (Playwright with Chromium is available).

## Gate before every push
`cargo fmt`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`,
`cargo build --target wasm32-unknown-unknown -p wp_wasm`, the determinism lint, `npx tsc --noEmit`
in `apps/testbed-web`, `just web-e2e` if it touches the testbed. Use
`CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0` to save disk.
Commit often (`wp_damage/panels: …`, `tools/assets: …`, `testbed/render: …`) and push `ws/panels`.
