# Render integration notes

- **Workstream / prompt:** engine spike: own renderer (follows `docs/ENGINE_DECISION.md`)
- **Branch:** `claude/trusting-mendel-ta85bb`
- **Crates / folders owned:** `crates/wp_render`
- **Contract version built against:** 0.3.0

## Exposes
- `wp_render::Scene::{new, sync(&Sim, frame_dt), set_static_boxes, set_city, city,
  traffic_mesh, lighting, lights, lights_near, cars, car, wheel_meshes}`: renderer-side copy of the sim. Read-only on the sim.
- `wp_render::{SkinnedBody, CarMesh, CarMeshLibrary, CityMesh, ChaseCamera, CameraView,
  StaticBox, Vertex, Lighting, SpotLight}`. `CityMesh` reads the `data/city/<id>.greybox.glb` files `tools/city-import`
  writes (format: `data/city/README.md`).
- Feature `gpu`: `wp_render::gpu::{Renderer, Offscreen, write_png}` and a re-export of the
  `wgpu` it was built with. The renderer takes its lighting from the scene (`Lighting` moved
  from `gpu` to `wp_render::lighting`).
- `testbed-native --features gpu`: `--gpu [--city ID] [--hour H]` (3D window) and `--shot`
  (headless PNG, `--city`, `--hour`, `--height`, `--distance`). See
  `crates/wp_render/README.md`.

## Needs
- Debris bodies (`DetachedPart` → Rapier) exposed through `wp_sim`, so detached panels can be
  drawn where the physics puts them instead of vanishing. Workaround: hidden.
- Read-only poses of the sim's city props (parked cars, cones), which become Rapier bodies in
  `Sim::set_world` and move when hit. Workaround: not drawn.
- A read-only accessor for static boxes added with `Sim::add_static_box`. Workaround: barriers
  come from the scenario.
- City geometry comes from the committed greybox `.glb`, not from the sim. If the greybox
  format changes (`tools/city-import/src/glb.rs`), `wp_render::city` must follow; its tests
  load both committed cities.
- The previous-tick vehicle state (as `wp_wasm::vehicle_states_prev`), for interpolation.

## Contract change requests
None. Everything used is already public in `wp_sim` / `wp_damage` / `wp_contracts`.

## Changes outside my crates
- Root `Cargo.toml`: `crates/wp_render` added to the workspace.
- `apps/testbed-native` (workstream 04): new optional feature `gpu` (`src/gpu_view.rs`, a
  `main.rs` hook, Cargo deps). Default and existing features are untouched, so CI still builds
  only the headless smoke run. Later: `--city ID` for the window and `--shot`, `--height` /
  `--distance` for `--shot`; `--hour` for both (`gpu_view::FreeDrive`).

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| `Scene::sync` (skin + wheels), head-on scenario, avg | cloud container, 4 vCPU | 0.012 ms/car/frame | — | not set yet |
| `Scene::sync` in a city (traffic boxes rebuilt), free drive, avg | cloud container, 4 vCPU | 0.006–0.008 ms/car/frame | — | not set yet |
| City greybox triangles drawn per frame | — | 21,654 (12×12 grid), 372,386 (Vesper) | — | not set yet |

## Known gaps
See `crates/wp_render/README.md` → Known gaps.
