# Render integration notes

- **Workstream / prompt:** engine spike: own renderer (follows `docs/ENGINE_DECISION.md`)
- **Branch:** `claude/trusting-mendel-ta85bb`
- **Crates / folders owned:** `crates/wp_render`
- **Contract version built against:** 0.3.0

## Exposes
- `wp_render::Scene::{new, sync(&Sim, frame_dt), set_static_boxes, cars, car, wheel_meshes}`:
  renderer-side copy of the sim. Read-only on the sim.
- `wp_render::{SkinnedBody, CarMesh, CarMeshLibrary, ChaseCamera, CameraView, StaticBox, Vertex}`.
- Feature `gpu`: `wp_render::gpu::{Renderer, Offscreen, Lighting, write_png}` and a re-export
  of the `wgpu` it was built with.
- `testbed-native --features gpu`: `--gpu` (3D window) and `--shot` (headless PNG). See
  `crates/wp_render/README.md`.

## Needs
- Debris bodies (`DetachedPart` → Rapier) exposed through `wp_sim`, so detached panels can be
  drawn where the physics puts them instead of vanishing. Workaround: hidden.
- A read-only accessor for the city geometry the sim builds (roads, buildings, rails), and for
  static boxes added with `Sim::add_static_box`. Workaround: barriers come from the scenario.
- The previous-tick vehicle state (as `wp_wasm::vehicle_states_prev`), for interpolation.

## Contract change requests
None. Everything used is already public in `wp_sim` / `wp_damage` / `wp_contracts`.

## Changes outside my crates
- Root `Cargo.toml`: `crates/wp_render` added to the workspace.
- `apps/testbed-native` (workstream 04): new optional feature `gpu` (`src/gpu_view.rs`, a
  `main.rs` hook, Cargo deps). Default and existing features are untouched, so CI still builds
  only the headless smoke run.

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| `Scene::sync` (skin + wheels), head-on scenario, avg | cloud container, 4 vCPU | 0.012 ms/car/frame | — | not set yet |

## Known gaps
See `crates/wp_render/README.md` → Known gaps.
