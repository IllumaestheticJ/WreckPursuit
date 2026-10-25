# wp_render — our own renderer (engine spike)

The first piece of a custom engine (option 4 in `docs/ENGINE_DECISION.md`). It draws what the
browser testbed draws, natively: the city greybox (roads, junctions, sidewalks, buildings,
terrain, sea, bridges) with traffic, every car from its per-panel mesh (`data/panels/meshes`),
skinned to the damage lattice so dents, swinging doors and hanging bumpers show; spinning and
steering wheels; scenario barriers; a ground grid; a chase camera. Built on
[wgpu](https://wgpu.rs) (Vulkan / Metal / DX12 today; WebGPU later, so one renderer could
eventually serve the browser too).

| Module | What | Needs a GPU |
|---|---|---|
| `car_mesh` | Reader for mesh formats 1–4 (`tools/assets` writes 4; buckling, tints and livery are ignored), on-demand `CarMeshLibrary` | no |
| `city` | `CityMesh::from_glb` / `load_repo`: reads `data/city/<id>.greybox.glb` (the file the browser draws, written by `tools/city-import`) into one world-space mesh per decal layer | no |
| `skin` | `SkinnedBody`: lattice → vertices (port of `testbed-web/src/render/panels.ts`, incl. rigid-frame fit for free panels); wheel meshes | no |
| `scene` | `Scene::sync(&Sim, dt)`: one `CarView` per vehicle (body, wheel poses, chassis transform), boxes for abstract traffic, static boxes; `Scene::set_city` | no |
| `camera` | `ChaseCamera` (lagged heading, speed pull-back, orbit), `CameraView::view_proj` | no |
| `lighting` | `Lighting::for_time`: sun or moon, sky, fog and a night factor from the world clock; `SpotLight` headlights | no |
| `gpu` (feature `gpu`) | `Renderer` (one WGSL shader; opaque pipelines per decal layer + blended glass; 4× MSAA; reversed-Z float depth), `Offscreen` + `write_png` | yes |

Rendering only **reads** the sim. Nothing flows back, so frame time and floating-point on this
side cannot affect determinism. Body vertex buffers are re-uploaded only when a car's lattice,
panels or glass changed.

## Run it

```sh
# 3D window (keyboard: arrows/WASD, Space handbrake, R reset, C orbit camera, Esc quit)
cargo run --release -p testbed-native --features gpu -- --gpu
# ... in a city (free drive on its first race grid, with traffic): vesper_v2 or procedural_12x12
cargo run --release -p testbed-native --features gpu -- --gpu --city vesper_v2
# ... at night (any hour 0..24; the clock then runs at 10× real time as usual)
cargo run --release -p testbed-native --features gpu -- --gpu --city vesper_v2 --hour 23
# add a wheel:      --features gpu,ffb

# Headless screenshot (any adapter; in a container install mesa-vulkan-drivers for lavapipe)
cargo run --release -p testbed-native --features gpu -- \
  --shot out.png --scenario apps/simcli/scenarios/crash/head_on.ron --seconds 4 --orbit 70
# City overview
cargo run --release -p testbed-native --features gpu -- \
  --shot city.png --city vesper_v2 --seconds 4 --height 80 --distance 90 --orbit 150
```

`--shot OUT [--scenario X.ron | --city ID --hour H] [--seconds S] [--size WxH] [--orbit DEG]
[--height M] [--distance M]` runs the scenario (or a free drive with light throttle) at 60 fps,
follows the first car and writes one PNG. It also prints the average scene-sync cost.

### How the city is drawn

A session with a city (`Session::has_city`) loads `data/city/<network id>.greybox.glb`, the
same file the browser loads, so both testbeds show identical geometry. glTF is +Y up; the
reader converts to world +Z up. Materials that share a plane (parking and bus lanes on
asphalt, asphalt and sidewalks on zone ground) go to decal layers 2 / 1 / 0, drawn by
pipelines whose depth bias pulls them toward the camera, the same table the browser uses for
polygon offset. Depth is reversed Z in a 32-bit float buffer, so roads 5 cm above the zone
ground stay separate across the whole city. Terrain carries smooth normals; everything else is
flat-shaded. Lit colour goes through a fixed filmic curve so pale concrete in full sun does not
clip. Under a city the ground plane drops to −0.2 m, as in the browser.

Traffic agents promoted to physics vehicles are sim vehicles and draw as full skinned cars.
The rest (abstract agents) draw as 1.8 × 4.4 × 1.4 m boxes, grey, red when braking, blue when
yielding to a siren (the browser's colours), rebuilt every `sync`.

Screenshots: `docs/media/wp_render/city_procedural.png` (12×12 grid from above, traffic),
`city_vesper_street.png` (Port Vesper street level), `city_vesper_air.png` (Vesper hills and
terrain from above).

### Time of day

`Scene::sync` reads the world clock (`sim.world().clock`) and builds `Lighting::for_time`:

- **Key light:** the sun follows `wp_world::sun_direction`, warming toward the horizon. Below
  it a dim blue moon, opposite the sun, takes over.
- **Sky, ambient and fog** fade from day to night colours with the sun's height, with an
  orange tint around sunset and sunrise. The clear colour is the fog colour, so the horizon
  matches.
- **Headlights:** every car with `VehicleInput::headlights` set has two spot lights at the
  front corners of its lattice. Police set it all day. At night (night factor > 0.5) every car
  and every abstract traffic box has them on, the same rule the traffic AI uses. The shader
  takes the 32 lights nearest the camera and scales them down in daylight. The lamp meshes
  themselves are emissive and always glow.
- **Buildings:** walls get a procedural window grid (3.2 m × 3.5 m panes, darker glass). At
  night about 40 % glow warm, chosen by an integer hash per pane and wall. Walls carry exact
  face normals from the loader so the pattern stays stable at city-scale coordinates.

Screenshots: `night_vesper_street.png`, `night_vesper_headlights.png` (from above: the beams on
the road), `night_procedural_air.png` (grid city at 01:00, traffic headlights),
`dusk_vesper.png` (18:12).

## Tests

```sh
cargo test -p wp_render            # CPU only: mesh files parse, skinning, free-panel rigidity, scene sync, camera,
                                   # both city greyboxes load and cover their road networks, traffic boxes, reversed Z,
                                   # day/dusk/night lighting, headlights on at night and with driver input, light cap
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
| City size | 12×12 grid: 21,654 triangles; Port Vesper v2: 372,386 triangles | whole city drawn every frame, one draw call per decal layer |
| `CityMesh::load_repo`, debug build | 22 ms (grid), 416 ms (Vesper, 6.8 MB) | once per session |
| First offscreen frame with a city, lavapipe | 150–570 ms | includes uploading the city; varies run to run. Not a real GPU number |
| `Scene::sync` in the cities (1 player car, traffic boxes rebuilt) | 0.006–0.008 ms/car/frame | free drive, 4 s |

The vehicle budget in `CLAUDE.md` (0.15 ms/car) is for the sim. A renderer budget still has to
be set. Real-GPU frame times wait for a machine with a GPU.

## Known gaps (next steps, roughly in order)

- **City details:** the greybox has no lane markings, signal heads, street lights or street
  furniture. Parked cars and cones (sim props) are not drawn: `wp_sim` does not expose their
  poses. Abstract traffic is boxes. The greybox is a prebuilt file; a city generated with a
  different seed than its committed `.glb` would not match (sessions only use the committed
  seeds today).
- **City cost:** no frustum culling or LOD. All of Vesper (372k triangles) is drawn every
  frame. Fine for a desktop GPU in principle, but not measured on one yet. The far plane is
  1.5 km and fog hides it.
- **Interpolation:** draws the latest sim state, not the interpolated one between ticks
  (the browser does interpolate). Fine at 240 Hz sim / 60 Hz frames, visible on high-refresh
  screens.
- **Detached panels and debris:** detached regions just vanish (the browser throws them as
  visual debris; Rapier debris bodies aren't exposed to renderers yet).
- **Lighting:** no shadows, no street lights, no wet roads. Only the 32 headlights nearest
  the camera light the scene. Headlights don't change with damage (a smashed front still
  lights the road). Night scenarios in cities without a committed greybox (e.g.
  `golden/night_pursuit_escape.ron`, an 8×8 city) show cars and grid only.
- **No HUD or text.** Police light bars, traffic, signals and the browser's debug layers are
  not drawn either.
- **Wheel height** uses the browser's approximation (static hub height + compression about
  mid-travel).
- **Web:** the crate builds for `wasm32-unknown-unknown` with `--features gpu`, but nothing
  drives it in a browser yet. Meshes load from the filesystem; the web would call
  `CarMeshLibrary::insert`.
