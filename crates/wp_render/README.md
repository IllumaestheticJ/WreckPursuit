# wp_render — our own renderer (engine spike)

The first piece of a custom engine (option 4 in `docs/ENGINE_DECISION.md`). It draws what the
browser testbed draws, natively: every car from its per-panel mesh (`data/panels/meshes`),
skinned to the damage lattice so dents, swinging doors and hanging bumpers show; spinning and
steering wheels; scenario barriers; a ground grid; a chase camera. Built on
[wgpu](https://wgpu.rs) (Vulkan / Metal / DX12 today; WebGPU later, so one renderer could
eventually serve the browser too).

| Module | What | Needs a GPU |
|---|---|---|
| `car_mesh` | Reader for mesh format 1 (`tools/assets` writes it), on-demand `CarMeshLibrary` | no |
| `skin` | `SkinnedBody`: lattice → vertices (port of `testbed-web/src/render/panels.ts`, incl. rigid-frame fit for free panels); wheel meshes | no |
| `scene` | `Scene::sync(&Sim, dt)`: one `CarView` per vehicle (body, wheel poses, chassis transform), static boxes | no |
| `camera` | `ChaseCamera` (lagged heading, speed pull-back, orbit), `CameraView::view_proj` | no |
| `gpu` (feature `gpu`) | `Renderer` (one WGSL shader, opaque + blended-glass pipelines, 4× MSAA, depth), `Offscreen` + `write_png` | yes |

Rendering only **reads** the sim. Nothing flows back, so frame time and floating-point on this
side cannot affect determinism. Body vertex buffers are re-uploaded only when a car's lattice,
panels or glass changed.

## Run it

```sh
# 3D window (keyboard: arrows/WASD, Space handbrake, R reset, C orbit camera, Esc quit)
cargo run --release -p testbed-native --features gpu -- --gpu
# add a wheel:      --features gpu,ffb

# Headless screenshot (any adapter; in a container install mesa-vulkan-drivers for lavapipe)
cargo run --release -p testbed-native --features gpu -- \
  --shot out.png --scenario apps/simcli/scenarios/crash/head_on.ron --seconds 4 --orbit 70
```

`--shot OUT [--scenario X.ron] [--seconds S] [--size WxH] [--orbit DEG]` runs the scenario at
60 fps, follows the first car and writes one PNG. It also prints the average scene-sync cost.

## Tests

```sh
cargo test -p wp_render            # CPU only: mesh files parse, skinning, free-panel rigidity, scene sync, camera
cargo clippy -p wp_render --features gpu -- -D warnings
```

The GPU path has no automated test yet. It is checked by rendering `--shot` images:
`docs/media/wp_render/headon.png` (head-on, coupe's hood crumpled), `bumper.png` (barrier:
hood popped, fender hanging) and `door.png` (side swipe: both left doors swung open).

## Measurements (2026-10-21, cloud container, 4 vCPU, release)

| What | Measured | Note |
|---|---|---|
| `Scene::sync`, head-on crash scenario (2 cars, 4 s) | 0.012 ms/car/frame average | includes frames skipped because nothing deformed; worst-case per-frame skin of a deforming car not measured yet |
| First offscreen frame, 1280×720, lavapipe (software Vulkan) | ≈ 110 ms | CPU rasteriser plus all buffer creation. Not a real GPU number |

The vehicle budget in `CLAUDE.md` (0.15 ms/car) is for the sim. A renderer budget still has to
be set. Real-GPU frame times wait for a machine with a GPU.

## Known gaps (next steps, roughly in order)

- **City:** roads, buildings and props from `wp_world` are not drawn. Only scenario barriers
  and a flat grid. Terrain cities render as flat.
- **Interpolation:** draws the latest sim state, not the interpolated one between ticks
  (the browser does interpolate). Fine at 240 Hz sim / 60 Hz frames, visible on high-refresh
  screens.
- **Detached panels and debris:** detached regions just vanish (the browser throws them as
  visual debris; Rapier debris bodies aren't exposed to renderers yet).
- **Lighting:** one sun, hemisphere ambient, fog. No shadows, no headlight or streetlight
  lights, no night, no wet roads.
- **No HUD or text.** Police light bars, traffic, signals and the browser's debug layers are
  not drawn either.
- **Wheel height** uses the browser's approximation (static hub height + compression about
  mid-travel).
- **Web:** the crate builds for `wasm32-unknown-unknown` with `--features gpu`, but nothing
  drives it in a browser yet. Meshes load from the filesystem; the web would call
  `CarMeshLibrary::insert`.
