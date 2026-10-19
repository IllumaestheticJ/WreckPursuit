# PROMPT 04 — Browser testbed, debug visualization, telemetry & replay (`apps/testbed-web`, `wp_wasm`)

Read `CLAUDE.md`, `docs/INTEGRATION.md`, `docs/CONTRACT_CHANGELOG.md`. You own `apps/testbed-web`, `wp_wasm`, and `apps/testbed-native` (new, optional stub). You may read all crates. Branch `ws/testbed`.

## Goal
The tool everyone tunes physics and damage in. Must run from `just web` in Chrome/Firefox with no install beyond Node + wasm-pack. It is also the first playable. Prioritise debug visibility and replay over looks — Checkpoint 1 is "boxes, wireframes, arrows, numbers".

## Work
**WASM surface (`wp_wasm`)** — thin, JSON-free where it matters:
- `init(seed, scenario_json)`, `step(dt_seconds)` (runs N fixed steps and returns the leftover alpha for interpolation), `set_input(entity, Float32Array)`, `set_assist_profile(json)`, `set_damage_mode(str)`, `spawn_vehicle(car_id, pos, yaw) -> entity`, `reset_vehicle(entity)`, `vehicle_states() -> Float32Array` (packed, document the layout), `lattice_nodes(entity) -> Float32Array`, `skin_bindings(entity) -> Uint32Array/Float32Array`, `events_drain() -> Uint8Array` (bincode/postcard of `Vec<SimEvent>`), `telemetry(entity) -> Float32Array` (speed, rpm, gear, per-wheel slip/load/surface, g-forces, steer angle, health, component health list), `ffb(entity) -> Float32Array`, `record_start()`, `record_stop() -> Uint8Array`, `replay_load(bytes)`, `replay_seek(tick)`, `replay_play(speed)`. Replays store **inputs + seed + spawn script**, and a state hash every 60 ticks for drift detection; deterministic re-simulation is the playback.
- Even if `wp_vehicle`/`wp_damage` are still stubs on your branch, wire everything through the traits so merging is a one-line change.

**Renderer (Three.js, TypeScript)**:
- Scene: flat ground with a 10 m grid, optional loaded `data/city/*.glb` greybox when present (prompt 05 will produce one), skybox colour driven by `TimeOfDay`.
- Vehicle: oriented box from `VehicleDef` dims; wheels as cylinders with steer/spin; lattice wireframe overlay; **skinned deformable body** when `skin_bindings` are non-empty (update `BufferGeometry` positions from `lattice_nodes` each frame; recompute normals every 4 frames).
- Debug layers (hotkeys, persisted in `localStorage`): tire force arrows (Fx/Fy per wheel, scaled), suspension travel bars, slip-angle colouring, contact points & impulses (fading), lattice node stress heat-map, component health panel, region energy panel, CG + velocity vector, traffic/police intent arrows (placeholder hooks).
- Cameras: chase (spring-damped), hood, bumper, free orbit, replay cinematic.
- Input: keyboard (WASD/arrows, space handbrake, shift/ctrl gears, R reset, 1–4 cameras), Gamepad API with deadzones and the gamepad assist preset; input display overlay. **Browser has no FFB** — show rack torque as a bar.
- HUD: speed, rpm bar, gear, damage silhouette (regions coloured by energy), component list, mode badge (Arcade/Sim), assist badges, FPS + sim ms/frame.
- Telemetry: rolling 10 s plots (Chart-free, hand-drawn canvas is fine) for speed, slip angles, loads, yaw rate; CSV export of the recording.
- Scenario menu: load any `apps/simcli/scenarios/**/*.ron` (serve the folder), including barrier crash tests at chosen speeds and a "two-car head-on" test; show pass/fail of the scenario's expected ranges after the run.
- Replay UI: timeline scrubber, speed control, free camera, frame step.

**Native testbed stub (`apps/testbed-native`)**: minimal winit + wgpu or Bevy window that steps the same core and sends `FfbOutput` to a DirectInput/SDL FFB wheel via `sdl2` if present. Keep this to "window + car box + FFB out" — it exists so wheel feel can be judged. Behind a cargo feature; must not be required for CI.

## Deliver
- `just web` → drivable car, every debug layer working, replay round-trips (record → reload page → replay matches hash).
- `docs/integration/testbed.md`: WASM ABI table (function, args, packed layouts), hotkey list, how to add a debug layer, how to add a scenario, perf numbers (sim ms + render ms at 1 / 8 / 24 cars in Chrome).
- Short GIF or screenshots in `docs/media/` for the status report.
