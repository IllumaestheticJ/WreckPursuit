# Testbed integration notes

- **Workstream / prompt:** 04 — browser testbed, debug visualization, telemetry & replay
- **Branch:** `ws/testbed`
- **Crates / folders owned:** `crates/wp_wasm`, `apps/testbed-web`, `apps/testbed-native`
- **Contract version built against:** 0.2.0

```sh
just web          # wasm-pack (release) + Vite → http://localhost:5173
just web-e2e      # headless Chromium end-to-end check (see "e2e" below)
just native       # native window stub (winit + softbuffer); add `--features ffb` for an SDL2 wheel
cargo test -p wp_wasm
```

Screenshots: `docs/media/` (`drive_chase.png`, `debug_layers_orbit.png`, `camera_hood.png`,
`replay.png`, `scenario_barrier.png`, `scenario_head_on.png`, `city_traffic.png`,
`bench_24.png`, `native_testbed.png`). Regenerate with `SHOTS=../../docs/media npm run e2e`.

## How it fits together

```
apps/testbed-web (TS + Three.js)
   │  typed arrays per frame, JSON for rare calls
   ▼
crates/wp_wasm  lib.rs (wasm-bindgen, thin)  ──►  session.rs  Session (pure Rust, native-tested)
                                                    │  simcli::Scenario (format v2) + live/scripted flag
                                                    │  log = simcli::replay::Replay (inputs, ops, hash per tick)
                                                    ▼
          simcli::run::{build_sim, driver_input, apply_op}  ──►  wp_sim::Sim (+ wp_world city/traffic)
```

**One code path with the harness.** A session *is* a simcli v2 scenario:

- The sim is built by `simcli::run::build_sim` (systems, city, barriers, entities + assists in
  order) and scripted entities are driven by `simcli::run::driver_input`, so a scripted session
  ticks exactly like `simcli run`. When the script ends, the scenario is re-run headless with
  `simcli::run` (inside wasm) for the full expectation report (every expectation kind), and the
  live end-of-script hash is compared with simcli's (`matches_simcli`).
- Free drive is a generated v2 scenario (`scenario::free_drive`: entity `"player"`, keyboard
  assists, optional procedural 12×12 city with the player on its race start grid).
- Everything the session does is logged into a `simcli::replay::Replay`: input changes, ops
  (reset, assists, mode switch) and the state hash after **every** tick. `record_stop()` returns
  canonical `replay.bin` (format v2) that `simcli replay <file>` verifies headlessly; browser
  playback re-simulates it and compares every tick hash. simcli golden replays (v1) play back in
  the browser too.
- Spawns are only allowed before the first tick (they are appended to the scenario's entities),
  so every recording stays a pure "scenario + inputs + ops" file.

## Exposes — WASM ABI

All functions throw a JS `Error("wp_wasm: …")` on failure. `entity` is `EntityId.0` (u32).
Car data (`data/{cars,lattices,tires,materials,modes}`) is embedded at build time by
`crates/wp_wasm/build.rs` and exposed to simcli as a `MemData` bundle.

| Function | Args | Returns | Notes |
|---|---|---|---|
| `init` | `seed: u32`, `scenario_json`, `scripted: bool` | `Uint32Array` entity ids | Fresh live session from a v2 scenario as JSON. `seed` 0 = scenario's. `scripted` = drive entities by their scenario drivers until `duration_s`, then entity 1 is live. |
| `free_drive_json` | `car`, `mode`, `city: bool` | scenario JSON | Free-drive scenario (city = procedural 12×12 with traffic). |
| `city_chase_json` | `car`, `mode` | scenario JSON | "City chase" preset (live): procedural city at 23:00, `police_presence: 1.0`, traffic, player starts on the race grid at 30 m/s (`EntitySpec::speed_mps`). Seed 23. |
| `scenario_from_ron` | `text` | scenario JSON | simcli format v2 (`apps/simcli/SCENARIOS.md`). |
| `scenario_with_speed` | `json`, `target_mps` | scenario JSON | Overrides every `cruise` driver's target (the speed box). |
| `step` | `dt_seconds: f32` | `f32` alpha | Runs the whole 240 Hz ticks covered (max 60/call); leftover interpolation alpha between `vehicle_states_prev()` and `vehicle_states()`. Replays scale `dt` by the playback speed. |
| `set_damage_mode` | `"Arcade"`\|`"Simulation"` | – | `Sim::set_mode_table`, no restart, existing damage kept; logged op. |
| `car_ids` | – | JSON `string[]` | Embedded cars. |
| `contract_version` | – | string | |
| `spawn_vehicle` | `car_id`, `pos: Float32Array[3]`, `yaw` | `u32` entity | **Before the first tick only**; becomes scenario entity `car<N>` with the scenario's assists. |
| `set_input` | `entity`, `Float32Array` (layout **I**) | `bool` | `false` = ignored (unchanged, or a scripted scenario is running). Logged. `gear_up/down` are one-tick edges (the clear is logged). |
| `set_assist_profile` | `entity`, `AssistProfile` JSON | – | Logged op. |
| `assist_preset` | `"Keyboard"`\|`"Gamepad"`\|`"Wheel"` | JSON | `AssistProfile::for_source`. |
| `assist_profile` | `entity` | JSON | Current assists. |
| `reset_vehicle` | `entity` | – | Spawn pose, pristine. Logged op. |
| `entities` | – | `Uint32Array` | Scenario entities, scenario order (vehicles the sim spawns itself — police units, promoted traffic — are not listed; see `vehicle_meta`). |
| `state_stride` | – | `50` | |
| `vehicle_states` | – | `Float32Array` (layout **S**) × vehicles | **Every** sim vehicle, ascending entity (scenario cars, then sim-spawned police / traffic as they appear and disappear). |
| `vehicle_meta` | – | `Float32Array` 3 × vehicles | Same order as `vehicle_states`: `[entity, kind (0 player, 1 AI racer, 2 police, 3 physical traffic), flags]`, flags 1 siren, 2 headlights, 4 braking, 8 horn (from the input the vehicle drives with). |
| `vehicle_states_prev` | – | same | State before the last tick of the latest `step()`. |
| `vehicle_info` | `entity` | JSON | `kind`, scenario `name` (sim-spawned: `police7`, `traffic12`), `body_size_m`, wheels, `cg_m`, redline, lattice sizes, name tables `regions` / `components` / `surfaces`, `mounted_components`, `spawn_pos`. |
| `world_info` | – | `Float32Array[8]` | `[tick, time_of_day_h, night_factor, mode (0/1 Sim), replay, recording, city, scripted]`. |
| `state_hash` | – | 16 hex digits | `Sim::state_hash()` (hash spec v1: includes lattice nodes). |
| `police_status` | `entity` | JSON or `null` | From `Sim::police()`: `{level ("None"/"L1".."L5"), heat, heat_in_level 0..1, status (Clear/Wanted/Pursuit/Searching), cooldown 0..1 (escape progress while Searching), units_in_pursuit, bounty, units_total}`; `null` when the police system is off. Mapped onto the UI kit's `HeatHud`. |
| `police_units` | – | `Float32Array` 6 × units | Police units that are **not** sim vehicles (internally driven cars, helicopters): `[x, y, z, yaw, speed_mps, flags]`, flags 1 helicopter, 2 pursuing/intercepting. |
| `signal_heads` | – | `Float32Array` 10 × heads | Static, fetch once per session. One head per signalised junction approach (signal group): `[x, y, z, yaw, left_m, right_m, radius_m, centre_z, intersection, group]` — stop-line centre (z from the lane polyline, so heads follow elevation), travel yaw (lamps face `yaw + π`), stop-line lateral extent along travel-right, junction radius and node height (far-side heads). Empty on the flat world. `signal_head_stride()` = 10. |
| `signal_states` | – | `Uint8Array` 1 × heads | Per frame, `signal_heads` order: 0 off, 1 red, 2 yellow, 3 green, from the world's signal clock (`wp_world::signals::group_state`), so pauses/replays show the simulated state. No turn arrows: the city has no protected turn phases. |
| `traffic_agents` | – | `Float32Array` 5 × agents | `[x, y, z, yaw, flags]`; flags 1 physical LOD, 2 brake lights, 4 hazards, 8 yielding to siren, 16 / 32 turn signal L / R, 64 aggressive, `variant << 8`. Empty on the flat world. |
| `lattice_nodes` | `entity` | `Float32Array` xyz × nodes | Current, chassis space (`Sim::node_positions`). |
| `lattice_rest` | `entity` | `Float32Array` xyz × nodes | Rest pose. |
| `lattice_beams` | `entity` | `Uint32Array` `[a, b]` × beams | |
| `lattice_node_regions` | `entity` | `Uint8Array` × nodes | Region index. |
| `lattice_strain` | `entity` | `Float32Array` × nodes | Max \|engineering strain\| of incident beams (stress proxy, TB-2). |
| `region_energy` | `entity` | `Float32Array[20]` J | Elastic `½·(EA/L0)·ΔL²` proxy split over regions (TB-2). |
| `skin_binding_indices` / `skin_binding_weights` | `entity` | `Uint32Array` 5 × binds / `Float32Array` 4 × binds | Empty for debug cars. |
| `events_drain` | – | `Uint8Array` | postcard `Vec<SimEvent>` (empty = 1 byte). Buffer capped at 4096. |
| `events_decode` | bytes | JSON `SimEvent[]` | |
| `telemetry` | `entity` | `Float32Array[74]` (layout **T**) | |
| `telemetry_names` | – | JSON `string[74]` | Also the CSV header. |
| `ffb` | `entity` | `Float32Array[3]` | `[rack_torque_nm, road_rumble, damper]`; the browser shows rack torque as a bar. |
| `intents` | – | `Float32Array` 8 × arrows | Placeholder hook (TB-5 deferred). |
| `record_start` | – | – | Marks recording active (the log always covers the session from tick 0). |
| `record_stop` | – | `Uint8Array` | Canonical `replay.bin` (simcli format v2) of the whole session. |
| `recording` | – | bool | |
| `replay_load` | `replay.bin` bytes (v1 or v2) | entity ids | Playback; live control refused until `init`. |
| `replay_seek` | `tick: f64` | – | Backwards = re-simulate from tick 0. |
| `replay_play` | `speed` | – | 0 = pause, max 16. |
| `replay_status` | – | JSON or `null` | `{tick, end_tick, speed, verified_ticks, drift_count, drift_ticks[≤64], state_hash, recorded_final_hash, scenario, mode, contract_version, hash_spec}`. |
| `replay_export_csv` | bytes, `entity`, `every_n_ticks` | CSV | Headless re-simulation; header `tick,t_s,<telemetry_names>`. |
| `scenario_json` | – | JSON | The session's v2 scenario (entities incl. tick-0 spawns, barriers). |
| `scenario_result` | – | JSON or `null` | Scripted sessions: `{name, done, tick, end_tick, pass, live_hash, matches_simcli, summary}`; `summary` is simcli's `RunSummary` (`expectations` / `pending`: `{expect, actual, passed}`). |

### Layout **S** — `vehicle_states()` record (50 × f32)

| Offset | Field |
|---|---|
| 0 | entity id |
| 1–3 | pos (world, m) |
| 4–7 | rot quaternion `[x, y, z, w]` |
| 8–10 | lin_vel (world, m/s) |
| 11–13 | ang_vel (world, rad/s) |
| 14 | speed_mps (signed, chassis +Y) |
| 15 | engine_rpm |
| 16 | gear (-1 R, 0 N, 1..) |
| 17 | health 0..1 |
| 18 + 8·w | wheel w (FL, FR, RL, RR): `omega, steer_rad, compression_m, slip_angle, slip_ratio, fz_n, contact (0/1), detached (0/1)` |

### Layout **I** — `set_input()` (7 × f32)
`[steer -1..1, throttle, brake, handbrake, clutch, flags, tool_use]`; flags bits:
1 gear_up, 2 gear_down, 4 horn, 8 headlights, 16 siren. Missing trailing values = 0.

### Layout **T** — `telemetry()` (74 × f32)

| Offset | Field |
|---|---|
| 0–4 | speed_mps, engine_rpm, gear, steer_angle_rad (mean of fronts), health |
| 5–7 | accel long / lat / vert, g (chassis space, over the last tick, gravity excluded) |
| 8 | yaw_rate_rad_s |
| 9–12 | applied input: throttle, brake, steer, handbrake |
| 13 + 10·w | wheel w: `slip_angle_rad, slip_ratio, load_n, surface (index into vehicle_info().surfaces), compression_m, contact, detached, omega_rad_s, fx_n, fy_left_n` |
| 53–73 | component health, in `vehicle_info().components` order |

`fx_n` / `fy_left_n` are the contract's `WheelState::{fx_n, fy_n}` (v0.2.0): forces applied last
tick in the tire frame, Fx + forward along the wheel heading, Fy + toward the wheel's left.

### Scenarios
Only simcli format v2 (`apps/simcli/SCENARIOS.md`). The menu lists `apps/simcli/scenarios/**`
(served at `/scenarios/`); the speed box overrides every `cruise` target. The testbed crash
tests live in `apps/simcli/scenarios/crash/` and gate in `cargo test` like any other scenario.

### Replay format
simcli `replay.bin` (`apps/simcli/src/replay.rs`), format **v2** = v1 + an ops section
(`reset` / `assists` / `mode`, applied before the same tick's inputs). The header scenario's
`duration_s` is the recorded length. Typical size: ~15 KB for 7 s of keyboard driving (8 bytes of
hash per tick dominate). `simcli replay file.replay.bin` verifies a browser recording.

## Browser testbed

### Hotkeys (layer toggles persist in `localStorage`)

| Key | Action |
|---|---|
| W/S, ↑/↓ | throttle / brake (ramped) |
| A/D, ←/→ | steer (ramped) |
| Space | handbrake |
| Shift / Ctrl | gear up / down |
| R | reset player car |
| O / Q | headlights / horn |
| C | next camera (remembered per viewer in localStorage `wp.testbed.camera.v1`) |
| 1–8 | camera: near chase (default), far chase, hood, bumper, cockpit, side tracking, TV / cinematic, orbit / free (debug) |
| V (hold) | look back |
| mouse drag | look around (springs back on release; right stick on a pad) |
| Tab | cycle focused vehicle (camera, HUD, telemetry) |
| P | pause (replay: play/pause) |
| . / , | replay frame step forward / back (4 ticks) |
| [ / ] | replay speed down / up |
| F1 or ? | help overlay |
| B | body box / skinned body |
| L | lattice wireframe |
| H | lattice stress heat-map |
| T | tire force arrows (applied Fx green, Fy red) |
| U | suspension travel bars |
| Y | slip-angle wheel colouring |
| F2 | contact points & impulses (fading) — was C, now the camera key |
| G | CG + velocity vector |
| X | civilian traffic agents (grey; red = braking, yellow = physical LOD, blue = yielding); promoted agents are drawn as sim vehicles instead |
| Z | chase readout (UI kit `ChaseHud`: wanted / heat / bounty / radio) + pursuit banner |
| F | repair bill (UI kit `RepairBill`, priced by `wp_economy`); opens by itself on capture or scenario end |
| 9 | traffic signals (was 6 until round 8: 6 is the side camera): mast-arm head at each stop line + far-side pedestal head, white stop line; lit lamp unlit-bright with an additive glow that strengthens at night (`signals_view.ts`, 6 InstancedMeshes for the whole city) |
| 0 | panel meshes (machine R, `src/render/panels.ts`; on by default) — off shows the lattice view |
| I | traffic / police intent arrows |
| K | component health panel |
| E | region energy panel |
| F4 | event feed — was V, now look-back — (component failures, parts, glass, region damage, total loss, violations) |
| J | telemetry plots |
| N | input display + rack-torque bar |

Gamepad (standard mapping): left stick steer, RT/LT throttle/brake, A or X handbrake, RB/LB
gears, Y horn, View/Back next camera, right stick look around, R3 (hold) look back; deadzones 0.12 stick / 0.05 trigger. Switching device logs an assists op with
`AssistProfile::gamepad_default` / `keyboard_default`.

Controls panel: scenario menu + speed box, **city chase** preset, damage mode (live switch), car, **city** (free drive
in the procedural 12×12 city: greybox `data/city/procedural_12x12.greybox.glb` + traffic),
bench 1/8/24, assists, replay (record / stop / load last / file / download `.replay.bin` / CSV /
exit), timeline scrubber + speed + frame step.

URL parameters: `?scenario=crash/head_on.ron&speed=60`, `?bench=24`, `?cam=orbit`,
`?replay=last`. `window.__wp` exposes the sim wrapper, the scene and helpers for automation
(`startFreeDrive(car, mode, city, hour?)`, `lookFrom(eye, target)` pins the orbit camera to a
sim-space spot).

### How to add a debug layer
1. Add `{ id, key, label, default }` to `LAYERS` in `apps/testbed-web/src/settings.ts` — the
   hotkey, persistence and help overlay come for free.
2. Draw it where the data lives: per-vehicle in `VehicleView.update`
   (`vehicle_view.ts`), world-level in `SceneView.update` (`scene.ts`), panels in `hud.ts`; guard
   with `layers.on("<id>")` and skip the wasm call when off.
3. If it needs new sim data, add a packed getter in `crates/wp_wasm/src/lib.rs` (+ layout in
   `pack.rs`, `src/wasm/layout.ts`, and this doc), and a native test in `pack.rs`/`tests.rs`.

### How to add a scenario
1. Write a v2 `.ron` under `apps/simcli/scenarios/<group>/` (see `SCENARIOS.md`;
   `crash/barrier_frontal.ron` is a compact crash example with `barriers` and a `cruise` driver).
2. It appears in the testbed's menu immediately; the browser's result panel is simcli's own
   report, plus whether the live run matched simcli's hash.
3. `cargo test -p simcli` needs its golden hashes: `cargo run -p simcli -- goldens --bless`,
   commit only the new `apps/simcli/golden/<name>/` folder with `[bless]` in the message.

### e2e (`just web-e2e` / `npm run e2e`)
Headless Chromium: keyboard drive; all layers + 5 cameras; record (with a live mode switch,
gear shift, handbrake, reset) → `simcli replay` verifies the file → page reload → playback
matches every tick hash and the final hash; scrub back/forward; CSV; the three crash scenarios
and a smoke scenario pass with `matches_simcli` and draw contact markers; city free drive shows
traffic; signals: the junction nearest the player framed at 12:00 and 22:00
(`docs/media/signals/`), every head drawn and lit, lamps change within 30 s and match
`signal_states()`, 9 hides the layer and F1 lists it; city chase waits (≤ 240 s, no fixed sleep) until it is
23:00, a `TrafficViolation` was counted (`__wp.app.eventCounts`) and exactly one heat panel is mounted. `BENCH=1` adds perf, `SHOTS=dir` screenshots.

### City chase
**city chase** (button, `__wp.startCityChase()`): the procedural 12×12 city at 23:00 with
traffic and full police presence; the player starts on the race grid already doing ~110 km/h,
so speeding violations (event feed) start the heat. Police cars (`VehicleKind::Police` sim
vehicles, or `police_units()` for internally driven units) are white with a red/blue light bar
that strobes while their siren is on (plus a point light); physical traffic
(`VehicleKind::Traffic`) is grey; every car has head/tail lights (headlights: O). The wanted
level / heat readout is M's UI-kit chase HUD (`apps/ui-kit/src/live`, mounted by
`src/ui/overlay.ts`), fed by `police_status()` mapped onto the kit's `PoliceReadout`
(`src/police.ts`). A centre banner shows **PURSUIT** / **SEARCHING** (escape %) while chased and
**ESCAPED** / **CAPTURED** for 6 s after a pursuit ends (`last_outcome`, `outcome_age_s`). A
capture, or the end of a scripted scenario, pops up the repair bill; a bill that popped up
closes on the next session, one the user opened (F) stays.

## Native testbed stub (`apps/testbed-native`)
Default build = headless smoke run printing FFB (the only thing CI builds); it now runs the
real vehicle and prints non-zero rack torque. `--features window` opens a winit + softbuffer
window (top-down car box, 10 m grid, rack-torque bar; arrows/WASD, Space, R, Esc).
`--features ffb` opens SDL2 haptic device 0, runs one constant-force effect updated from
`rack_torque_nm / 20 N·m`, and reads joystick axis 0 as steering. Runtime needs
`libxkbcommon-x11` (X11) and, for `ffb`, `libSDL2` at link time.

## Needs
| From | What | Workaround now |
|---|---|---|
| 10 harness | Turn on `RunOptions::load_city` in the CLI (re-bless `golden_night_pursuit_escape`) | Browser city sessions play back drift-free in the browser, but `simcli replay` runs them on the flat world (diverges) |
| 10 harness | Review the simcli changes listed below (barriers, `build_sim`, replay v2 ops) | — |
| 12 lead | Physical traffic agents as Rapier bodies (`Sim::set_world` makes parked cars/cones bodies; moving agents are kinematic in the render) | Agents are drawn only; cars pass through them |
| 05 world | — (greybox + `World::agents()` consumed) | |

## Contract change requests
| # | Change | Status |
|---|---|---|
| TB-1 | `WheelState { fx_n, fy_n }` | **Landed in v0.2.0**; telemetry and force arrows read it (no estimate, no `wp_vehicle` internals). |
| TB-2 | `DamageSim::region_energy_j` / node strain | Accepted in principle; still proxies here. |
| TB-3 | static boxes | Done (`Sim::add_static_box`); used through scenario `barriers`. |
| TB-4 | mode switch | Done (`Sim::set_mode_table`); used live, logged as a replay op. |
| TB-5 | debug intents | Deferred. |
| TB-6 | lattice nodes in hash | Done (hash spec v1). |

## Changes outside my crates
Run 3 (2026-10-19), all in workstream 10's `apps/simcli` — minimal and test-covered, please
review:
- `Scenario.barriers: Vec<BarrierSpec { pos, size, yaw }>` (serde default, skipped when empty,
  so existing scenario RON / replay headers are unchanged), validated, added with
  `Sim::add_static_box` before any spawn. `SCENARIOS.md` documents it.
- `run.rs`: setup extracted from `run_with_hook` into `pub fn build_sim` (+ `build_city`,
  `apply_op`); `driver_input` made `pub`. `RunOptions` gained `op_track` and `load_city`
  (default off → every existing golden unchanged; `cargo test -p simcli` green).
- `replay.rs`: format **v2** = v1 + ops section (`OpEvent { tick, entity, op: Reset | Assists |
  Mode }`); `verify` applies ops; v1 files still decode (golden `.replay.bin` files untouched).
- `fuzz.rs`: `barriers: Vec::new()` in the generated scenario.
- New scenarios `apps/simcli/scenarios/crash/{barrier_frontal,barrier_frontal_sim,head_on}.ron`
  (moved back from `apps/testbed-web/scenarios/`, converted to v2) and their new goldens
  `apps/simcli/golden/crash_*/hashes.txt` (commit tagged `[bless]`; no existing golden changed).
- `simcli` depends on `wp_world` (for `build_city`).

Run 5 (2026-10-19): `wp_sim`: read-only `Sim::police() -> &PoliceSystem` (one accessor; lead,
replace with yours). `apps/testbed-web` imports M's `apps/ui-kit` sources (`HeatPanel`, theme)
and depends on `preact` 10.27.2 (same version as the kit; Vite `dedupe`).

Earlier: workspace member `apps/testbed-native`; `justfile` recipes `web-e2e`, `native`.

## Perf numbers
Machine: cloud container, 4 vCPU Xeon 2.1 GHz, headless Chromium 141 with SwiftShader (CPU
raster, no GPU) — render and fps numbers are not representative of desktop Chrome. Real vehicle
(02) and damage (03) models, release wasm. "sim-only" = 240 × `step(1/60)` timed inside the
page (includes the session's per-tick state hash and input logging).

| Measurement | Browser (this machine) | Budget (browser = 2× native) |
|---|---|---|
| sim-only, 1 car | 0.09 ms / 60 Hz frame | vehicle + lattice 0.8 ms/car |
| sim-only, 8 cars | 0.69 ms | 6.4 ms |
| sim-only, 24 cars | 2.4 ms | 19.2 ms |
| sim-only, city + 192 traffic agents, 1 car | 0.31 ms | traffic 4 ms |
| render (JS submit), 1 / 8 / 24 cars | 0.7 / 2.9 / 8.7 ms | — |
| fps (SwiftShader) 1 / 8 / 24 cars | 13 / 12 / 9 | — |

(Re-measured 2026-10-19 on contract v0.2.0 / main `502f72e` physics.)

Rendering dominates; 24 cars with every per-car layer (lattice, stress, skinned body) would want
instancing before Checkpoint 3. The first seconds of a city session are slower (traffic
populating around the player).

### Panel meshes in the city chase (round 8, `e2e/chase_perf.mjs`, A/B with `PANELS=0`)
City chase, default car, 22 vehicles (player + police + physical traffic, 20 with panel meshes),
20 s holding W, same SwiftShader container as above. HUD smoothed ms per frame.

| Viewport | Panels | Frame (wall) | sim | views | render submit | draw calls | triangles |
|---|---|---|---|---|---|---|---|
| 1280×720 | on | 625–633 ms | 42–56 | 11.5–17.0 | 17.8–20.2 | ~985 | 436 k |
| 1280×720 | off | 591–645 ms | 46–55 | 7.3–9.8 | 7.7 (one 140 outlier) | ~95 | 351 k |
| 640×360 | on | 451 ms | 67 | 11.5 | 27.2 | 1147 | 447 k |
| 640×360 | off | 405 ms | 71 | 6.8 | 11.1 | 416 | 360 k |

Panels cost about +5 ms of view updates and +10–16 ms of JS render submission for 20 cars
(~45 draw calls per car), i.e. ~0.75 ms/car of CPU, and add ~85 k triangles. In this container the
wall frame time is set by SwiftShader rasterising the ~350 k-triangle night city (barely changes
from 1280×720 to 640×360), so panels change it by 0–10 %. On a real GPU the per-car draw calls are
the part that scales: request for R below.

## Known gaps
- Signals: no turn arrows or pedestrian heads (the city has no protected-turn or walk phases);
  the far-side head is placed by junction radius, not by the outgoing road's real curb.
- e2e "city chase" check fails in the cloud container on `main` too (2026-10-19): headless
  SwiftShader runs the sim at ~150–250 ticks per 5 s wall-clock, so the player never earns heat.
  Same root cause as playtest item 3 (browser sim falls behind real time).
- Region energy / stress are elastic-strain proxies (TB-2).
- Recordings always cover the whole session from tick 0 (`record_start` does not trim; the
  replay format has no view-start field).
- Moving traffic agents are not collidable (lead's wiring); `intents()` is empty (TB-5).
- `simcli replay` cannot verify city recordings until the CLI loads cities (see Needs).
- Native stub: FFB not tested against a physical wheel.
- `cargo test -p wp_wasm` takes ~2 min in debug (real physics: crash scenarios + their simcli
  re-runs + a city session).

## Lead feedback (2026-10-19)

**Merged** (`c0f78d9`). `just web-e2e` passes on the integrated build with the real vehicle model:
drive, all layers + 5 cameras, replay after reload 26/26 hashes, CSV, three scenarios, no page errors.

Lead changes in your area (pull them):
- **Scenario format clash with 10.** simcli's format v2 is canonical (prompt 10 owns the format).
  Your three crash files (`car`/`others`/`barriers`/`cruise_kmh` dialect) broke simcli, so they
  moved to `apps/testbed-web/scenarios/crash/`. `vite.config.ts` serves both folders under
  `/scenarios/`. `wp_wasm::Scenario::from_ron` now parses v2 first via the `simcli` lib
  (wasm-clean, new dependency of `wp_wasm`) and converts it with `Scenario::from_simcli`, then
  falls back to your dialect. Approximations in the adapter: entity 1 spawns at the origin; a
  `cruise` driver holds for the whole run (no `from_s`/`until_s`); per-entity assists ignored;
  only `metric(...)` expectations are kept.

Next for you (priority order):
1. Adopt v2 natively: build sessions straight from `simcli::scenario::Scenario` (entity poses,
   cruise windows, assists, all expectation kinds), convert `crash/*.ron` to v2 and move them back
   to `apps/simcli/scenarios/crash/`; then delete the adapter and the old dialect.
2. Barriers: `wp_sim::Sim::add_static_box(pos, size, yaw)` exists (TB-3 done) — call it for
   barriers so crash scenarios really crash. 10 is adding `barriers` to v2.
3. Mode switch: `Sim::set_mode_table(ModeTable)` exists (TB-4 done) — switch without restarting and
   log it as an op so replays survive the switch.
4. Replays: one canonical format. simcli `replay.bin` v1 (10) is it, so `simcli replay` can verify
   browser recordings headlessly. Write that format from the browser (keep reading WPR\x01 for old
   files if you like).
5. Tire forces: `sim.vehicles()[i].vehicle.telemetry()` already has the applied per-wheel Fx/Fy
   (02's README), so the force-arrow estimate can go now without waiting for TB-1.
6. Contact layer: `SimEvent::Contact` events now flow (one per touching pair per tick above
   50 N·s); check the C layer shows them.

Contract requests:
| # | Decision |
|---|---|
| TB-1 `WheelState { fx_n, fy_n }` | Accepted for v0.2.0 (net snapshot cost is small). |
| TB-2 region energy / node strain | Accepted in principle; final shape agreed with 03 when it lands. |
| TB-3 static boxes | **Done** in `wp_sim` (no contract change). |
| TB-4 mode switch | **Done** in `wp_sim` (`set_mode_table`). |
| TB-5 debug intents | Deferred until 05/06 merge. |
| TB-6 lattice nodes in hash | **Done** by 10's hash spec v1 (nodes are hashed). |

Perf: please re-measure `?bench=24` now that the real model runs (your numbers were stub physics).

### Run 2 (2026-10-19)
- Real damage (B) is live: the lattice/stress layers now show deformation; head-on scenario passes 3/4
  pending damage checks. `e2e` all green.
- Machine D asks for an agents export (`Float32Array` of `x, y, z, yaw, flags` per traffic agent) and
  the greybox city (`data/city/procedural_12x12.greybox.glb`); `Sim::set_world` exists to load a city.
- Machine F's `net.html` will land in your app when F re-lands; keep `src/net/` theirs.

### Response — run 3 (2026-10-19, workstream 04)
1. **v2 natively — done.** Sessions are `simcli::Scenario`s built by `simcli::run::build_sim`
   and driven by `driver_input`; all expectation kinds come from simcli itself. The crash files
   are v2 in `apps/simcli/scenarios/crash/` with goldens; the adapter, the old dialect and
   `apps/testbed-web/scenarios/` are gone.
2. **Barriers — done.** Added `barriers` to v2 (minimal, see "Changes outside my crates"; 10 please
   reconcile with your version). Crash tests really crash: Arcade frontal survives at ~0.7 health,
   Simulation frontal is a total loss, head-on deforms both cars; all gate in `cargo test`.
3. **Mode switch — done.** `set_damage_mode` → `Sim::set_mode_table`, logged as a replay op.
4. **One replay format — done.** The browser writes simcli `replay.bin` (v2 = v1 + ops); `e2e`
   verifies a browser recording with `simcli replay`. WPR\x01 support dropped.
5. **Tire forces — done.** From `wp_vehicle::Telemetry::fx/fy`.
6. **Contact layer — verified.** e2e counts contact markers in all three crash scenarios.
- Run 2 asks: `traffic_agents()` export (`[x, y, z, yaw, flags]`) and the procedural city greybox
  + traffic in free drive (**city** box) — done. Perf re-measured with the real models (table above).

### Response — run 4 (2026-10-19, workstream 04, contract v0.2.0)
- Merged `main` (`502f72e`, contract v0.2.0). Conflicts only in `testbed-web/src/{main,scene}.ts`
  (main's relative URLs for the static build) — kept run-3 code with relative URLs everywhere.
- v0.2.0 items for 04: tire forces now come from `WheelState::{fx_n, fy_n}` (TB-1 landed; the
  `wp_vehicle::Telemetry` dependency is gone). New `GlassShattered` / `RegionDamaged` (and every
  other non-contact event) appear in a new **event feed** panel (hotkey V); the wasm/TS code has
  no exhaustive `SimEvent` matches, so nothing else needed changing.
- Crash goldens re-blessed (`[bless]`, only `apps/simcli/golden/crash_*/hashes.txt`): main's
  physics changes (tire data, shared ground collider) moved their hashes from tick 180 on; all
  expectations still pass unchanged.
- Checks: fmt, clippy `-D warnings` (incl. `testbed-native --features window,ffb`),
  `cargo test --workspace`, wasm32 build, `npm run build`, `npm run e2e` (+ bench) all green.
  Run-3 items (v2 natively, barriers, mode switch, replay.bin, tire forces, contacts, agents
  export, city) are unchanged and still verified by e2e.

## Lead feedback (round 6, 2026-10-19)

**Merged** — city chase button, police light bars, physical traffic drawing, heat panel. Great work.

Lead changes in your area:
- Machine A also implemented a city chase in your files (prompt mix-up). Your version won; A's testbed/`wp_wasm`
  changes were reverted wholesale.
- `session.rs`: your all-vehicle extras kept; scripted driving and replay checkpoints use scenario entities only
  (`simcli::run::scenario_states`) — city replays broke otherwise ("trailing bytes").
- `main.ts`: both your `PoliceHud` and M's `UiOverlay` are constructed. **Consolidate with M**: one heat / wanted /
  pursuit readout (M's UI kit components inside your page), plus M's repair-bill screen.
- You added `Sim::police()` to `wp_sim`; it already existed in `police_link.rs`, so yours was removed.

Next: show the **repair bill** after a session / capture (G's economy is merged; ask for a `wasm` call if needed),
and a visible "PURSUIT / SEARCHING / ESCAPED / CAPTURED" banner from `police_status()`.


## Lead note (2026-10-19): new machine R (panels)
Machine R (`ws/panels`, prompt 21) owns `crates/wp_damage/src/panels.rs` (hinged panel swing / unlatch / hang states), `data/panels/`, `tools/assets/` and `apps/testbed-web/src/render/`. Expect small hooks from R in your files; coordinate through `docs/integration/panels.md`.


## Lead note (round 6 gate, 2026-10-19)
- `e2e/run.mjs` city chase check is timing-flaky: it holds W for 5 s of wall time and needs a speeding violation in that window (failed once and passed on rerun, same build). Make it wait for a condition, e.g. poll up to 15 s for `TrafficViolation` in the feed, instead of a fixed sleep.
- Lead removed a duplicate `policeStatus` key in `src/wasm/sim.ts` (M had added a second one returning null, which overrode yours, so the heat panel showed "null"). Keep one accessor.
### Response — round 6 (2026-10-19, workstream 04)
- Merged `origin/main` + lead branch. One conflict in `wp_wasm/src/session.rs`, resolved like the
  lead (all-vehicle extras, scenario-only scripting / checkpoints).
- **One police readout.** `PoliceHud` (`police_hud.tsx`) deleted; M's chase HUD in `ui/overlay.ts`
  is the only heat / wanted / pursuit readout. Fixed on the way: (a) `sim.ts` had two
  `policeStatus` entries — M's looked for a non-existent `police_status_json` export, shadowed the
  real one and broke `tsc`; (b) the overlay cast wasm JSON straight to `PoliceReadout`, so units
  and escape cooldown were undefined — now mapped in `src/police.ts`; (c) layers `police` and
  `chase` both used Z — `police` removed.
- **Pursuit banner** PURSUIT / SEARCHING / ESCAPED / CAPTURED. `police_status()` gained
  `staleness_s`, `escape_s`, `last_outcome`, `outcome_age_s` (from `PoliceSystem::stats()`;
  unit test drives a total-loss capture).
- **Repair bill** after capture and at scripted-scenario end (M's `RepairBill`, G's pricing via
  the existing `repair_quote_json`; no new wasm call needed). F still toggles it.
- e2e: bill auto-opens after a scenario, exactly one heat panel in the city chase, and the banner
  sequence + capture→bill (stubbed `policeStatus`, since a real capture takes minutes).
- Request (lead / E): a test-only way to force a capture in a `wp_sim` session
  (e.g. `Sim::police_mut()` or a `PoliceSystem::set_heat` passthrough) so e2e can run the real
  capture path instead of a stub.
- Checks (this container): fmt, clippy `-D warnings`, wasm32 build, `tsc`, `npm run e2e` 21/21
  (city chase now waits on sim time: SwiftShader ran the city at ~0.3× real time and the old
  fixed 5 s wait saw no violation), crash scenarios pass and match their goldens.
  `cargo test --workspace`: simcli green (release), everything else 546 passed, **2 failed, both
  already on the lead branch** (code identical, not in my crates) — lead / owners please look:
  - `wp_economy` `tests/pursuit_wiring.rs:130` `real_police_capture_settles_through_the_economy`
    ("level at capture, not after the reset");
  - `wp_race` `tests/simcli_races.rs:28` `city_sprint_on_real_roads` (index out of bounds,
    len 2 index 2 — probably counts sim vehicles now that city runs add police/traffic).
## Lead helper note (2026-10-19): cameras (`lead/playtest-camera`, playtest row 7)
Owner feedback: "the camera zooms way too far out", "need more angles". Changes, all in
`src/cameras.ts` plus small wiring (`main.ts` keys/pursuer/HUD label, `input.ts` `isHeld`,
`settings.ts` key moves, `hud.ts` help line, `e2e/run.mjs` camera list, new `e2e/cameras.mjs`):
- **Why it zoomed out:** the old chase cam sprung the camera's *world* position (ω = 5), which
  trails a moving car by ≈ 2v/ω — about 12 m extra at the 30 m/s city-chase start speed, on top
  of a 6–8.4 m base. The city chase preset itself set `chase` (not far / orbit).
- **New chase:** car-relative offset from a smoothed heading; only the offset is sprung, so speed
  never adds lag. Near chase 5.6 m back / 1.8 m up (+≤1.2 m pull-back toward 50 m/s, FOV 62→70
  for speed feel); far chase 8.5 m / 2.9 m (+≤1.5 m). Look target leads along the real body
  heading (into turns) and blends toward the travel direction in slides. Springs use the exact
  critically-damped step (stable at any frame time; the Euler step diverged at SwiftShader frame
  times). Measured in city chase at ~115 km/h: 6.4 m back, 1.78–1.81 m up.
- **Angles:** near chase, far chase, hood, bumper, cockpit (own car hidden via Three layer 1, a
  placeholder dash / A-pillars / wheel), side tracking, TV / cinematic (roadside spots planted
  ahead of the car, cut once it passes; every third cut a heli or low follow shot; long lens;
  frames the nearest police car within 45 m), orbit / free. Hold V to look back.
- Presets (city chase) return to the viewer's chosen angle; replays still switch chase → TV.
- `npm run e2e:cameras` checks distance/height, C cycling, V look-back and persistence, and writes
  `docs/media/cameras/*.png`.
- Gaps: no camera collision with buildings (the near chase rarely clips; far/side can at
  corners); cockpit dash is a placeholder until real car interiors (R).
- `just web-e2e` on this branch: all checks pass except "city chase … speeding violations in the
  feed" (heat stays 0.00, no TrafficViolation in the feed). It fails the same way on unmodified
  `origin/main` (612e036) in this container, so it is pre-existing and not camera-related.


## Lead feedback (round 8, 2026-10-19)

Contract **v0.3.0 is on `main`** (typed police/race/economy events, damage snapshot/summary, road-network trait, weather `LocalConditions`; see `docs/CONTRACT_CHANGELOG.md`). Workspace tests now take about 3.5 minutes (`lead/test-speed`). Owner playtest items are tracked in `docs/PLAYTEST_LOG.md`.

1. The city chase now runs with merged physical police (E's obstacle awareness), D's traffic variety, H's cruiser package, and the lead helpers' camera rig, signals and RWD slide control. Play it in headless Chromium, and fix anything visibly broken (overlapping panels, layer toggles, help text).
2. Make the city chase e2e check robust (wait for a condition, not a fixed 5 s).
3. M will port its chase-end detector on top of your HUD; review it.
4. Machine R's panel meshes are on by default (key 0). Check perf in the city chase with 6+ cars and record it.
## Lead helper note (`lead/playtest-feel`, 2026-10-19) — perf + feel
- HUD: real-time factor (`src/rtf.ts`) with an amber slow-motion warning below 0.95×; frame
  breakdown sim / views / render, also on `window.__wp.app.perf`.
- Frame catch-up cap 0.1 → 0.25 s (= `MAX_TICKS_PER_STEP`); slow frames no longer drop sim time.
- `VehicleView` rebuilds the lattice wireframe / skin only when the nodes change (allocation-free,
  fixed bounding sphere); skin normals on every rebuild.
- Default car `lindwurm_4t` (free drive + city chase); keyboard throttle ramp 0.2 → 0.08 s.
- New tools: `e2e/chase_perf.mjs` (sim ms + RTF in headless Chromium; `VIEWPORT=480x270`),
  `e2e/cpu_profile.mjs`, `cargo run --release -p wp_wasm --example chase_bench`.
Numbers: `docs/PLAYTEST_LOG.md` rows 3–4.
### Note from M (UI kit, round 8 item 3) — for C to review
`src/ui/overlay.ts`: the capture → bill trigger now uses the kit's `ChaseEndDetector`
(`apps/ui-kit/src/live/chaseEnd.ts`). It reads the typed `PoliceEvent::PursuitEnded` from the
drained events (exact outcome and level before the reset), falling back to the status turning
clear plus a fresh (< 1 s) `last_outcome` from your `police_status`; it replaces the
`outcome_age_s` → end-tick arithmetic. Your behaviour is kept (banner unchanged; bill on capture,
and now also on a total loss mid-chase; not on escape). `ui.lastChaseEnd` = `{ how, outcome,
level, tick }`. `e2e/run.mjs` stubbed flow: one extra pursuit frame before the capture, plus checks
on `lastChaseEnd`. Nothing else in your files changed.
### Response — round 8 (2026-10-19, workstream 04)
Merged `origin/main` (d3effad, fast-forward). Changes are all in `apps/testbed-web/`.
**1. City chase playtest** (headless Chromium, 1400×860 and 1280×720; new
`node e2e/playtest_chase.mjs [outDir] [WxH]` drives the chase 12 s of sim time, screenshots it,
checks HUD boxes for overlaps, that every layer key flips its layer, camera keys 1–8 and F1 help).
Fixed:
- **Key 6 did two things.** The signals helper put the signals layer on 6 before the camera
  helper made 1–8 camera angles; layer keys win, so 6 never reached the side camera and F1 listed
  6 twice. Signals layer → **9**; `settings.ts` notes that 1–8 and M are taken.
- **Telemetry plots covered the car.** The 360×280 strip at bottom-centre hid the lower third of
  the player car in the near chase camera; now 340×192 and clear of the car at 720 and 860 px tall.
- **Event feed printed raw JSON** for police and violation events
  (`Police Radio={"unit":1,"text":…}`, `kind={"Speeding":{"over_mps":15.097}}`, unit positions).
  Enum variants are unwrapped: `Police Radio unit=1 text=…`, `TrafficViolation #1 Speeding
  over_mps=15.1`, `UnitSpawned unit=5 kind=Car` (`hud.ts` `feedDetail`).
- **F1 help** was 60 % transparent over the HUD and unreadable in the chase; now opaque, scrolls if
  the window is short, and lists Tab (follow next car). Stale duplicate Z row in the hotkey
  table above replaced with the missing 0 (panels) row.
- Checked and fine: no overlapping panels at either size (chase readout sits between the HUD and
  the damage column), all 18 layer toggles, all 8 cameras, no page errors.
**2. City chase e2e check** now waits (≤ 240 s, polling) for the whole condition it checks —
23:00, a counted `TrafficViolation`, a police status, exactly one mounted heat panel — instead of
a fixed sleep or reading the 12-line feed (violations can scroll out of it). The count is
`__wp.app.eventCounts` (sim events by kind, reset per session). A timeout now fails the check and
says so instead of being swallowed.
**3. Review of M's chase-end detector** (`apps/ui-kit/src/live/chaseEnd.ts` is on main; the
`overlay.ts` integration on `ws/ui` was held back by the lead). For M's port onto my HUD:
- **Capture is reported as "escaped".** Pursuit → Clear is how both an escape and a capture look
  in `status`. The detector must read `police_status().last_outcome` (`Escaped` / `Captured`,
  with `outcome_age_s`), which the HUD already uses (`src/police.ts` `isCapture`). Suggest
  `update(status, totalLoss, lastOutcome)` returning `"escaped" | "captured" | "wrecked"`.
- **One trigger, not two.** `UiOverlay.frame` already opens the bill on a fresh capture
  (`outcomeTick` guard, once per outcome). Replace that block with the detector rather than adding
  a second path, or the bill logic runs twice on a capture.
- **Use `autoBill()`, not `setBill(true)`.** An auto-opened bill closes on the next session; one
  opened with `setBill(true)` counts as user-opened and stays open after a restart.
- **Escape opening the bill is a product call.** Today ESCAPED shows the banner only (e2e asserts
  `!escaped.bill`); M's version opens the bill on escape. Fine by me if the lead wants that — then
  update that e2e assertion in the same change.
- Keep `lastChaseEnd` (useful for automation), reset it in `reset()`, and gate on
  `layers.on("chase")` the same way the banner is.
- Detector logic and its vitest cases otherwise look right (fires once, wreck mid-chase,
  reset forgets).
**4. Panel meshes perf** with 22 cars: table in "Perf numbers" above. ~0.75 ms/car of CPU
(views + draw submission), ~45 draw calls/car; not the bottleneck in SwiftShader.
**Requests**
- **R (panels):** ~45 draw calls per car (985 vs 95 for 20 cars). Merging each car's static
  panels into one geometry per material (keep swinging/detached panels separate) would cut that
  to a handful and matters most on real GPUs with 24 cars.
- **Lead (city):** the night city is ~350 k triangles and sets the frame time in headless
  Chromium (~0.4–0.6 s/frame, sim at ~0.4× real time); LOD or distance culling for buildings
  would help the published link on weak laptops.
- Still open from round 6: a test-only way to force a capture in a `wp_sim` session, so e2e can
  run the real capture → bill path.
**Checks (this container):** `cargo fmt --check`, `clippy --workspace -D warnings`, `cargo test
--workspace` (717 passed, 15 ignored, 0 failed), wasm32 build of `wp_wasm`, `tsc`, `just web-e2e`
25/25 (1 min 18 s; city chase ready after 1.0 s of sim time — that run counted 2 violations
while the feed, redrawn every 6th frame, did not show them yet: the race the old check had),
`playtest_chase.mjs` at 1400×860 and 1280×720 all pass.

## Lead helper note (`lead/demo-endrun`, 2026-10-19) — end of run

New wasm exports (`crates/wp_wasm/src/run.rs`, `lib.rs`; native tests in `run_tests.rs`):

| Export | Returns |
|---|---|
| `run_result_json()` | `RunReport` or `null`: `seq` (one per settlement), `outcome` (`Escaped` / `Safehouse` / `Captured` / `CapturedTotalLoss` / `TotalLoss`), `run_over`, `level`, `mode`, `bounty_payout`, `rep_delta`, `fines`, `bail`, `towing`, `car_status`, `impound_fee`, `impound_fee_per_day`, `days_until_auction`, `repair` (G's `RepairQuote`), `repair_cost`, `health`, `total_loss`, `cash_before`, `cash_after`, `continue_cost`, `can_continue`, `ledger` |
| `run_ledger_json()` | every ledger line of the player's account (`seq, day, reason, detail, cash_delta, rep_delta, bounty_delta, cash_after`) |
| `run_account_json()` | `{ cash, rep, bounty, debt }` |
| `run_repair_continue()` | pays impound + `RepairAll` from the ledger, repairs the car in place (replay op `Repair`), releases the brakes; throws when refused |
| `debug_set_heat(entity, heat)`, `debug_park_near_police(entity, ahead_m)` | e2e / debug only, not in replays |

Behaviour: live sessions with police get a `PlayerRun` (not replays or scripted scenarios). It reads
the drained events after each tick (outside the sim step: hashes and goldens unchanged). A capture
or a wreck holds the player's car on full brake (logged inputs, so recordings replay) until
"Repair & continue" or a restart; `set_input` returns `false` meanwhile. The account carries over
`init()` restarts. `vehicle_meta` flag 1 and the audio siren of police cars follow the unit's
assignment (`police::siren_on`).

Testbed: `src/ui/runEnd.tsx` (`RunEndView`, kit `Panel` / `Button` / `Money` / `Stat` / `Badge`) in
`#ui-run-end`: on a capture or wreck it opens next to the bill with the result and
**Repair & continue** / **Restart**; an escape shows a 5 s bounty toast. `main.ts`: construct it,
`runEnd.update(sim.runResult(), now)` each frame after `ui.frame`, `runEnd.reset()` on rebuild,
`restartRun()`, both on `window.__wp`. `e2e/run.mjs`: a real capture (player parked in front of a
patrol car at heat 30) → panel + bill → click **Repair & continue** → health 1.0, cash = after −
cost, panel closed. C: the stubbed banner check and `ChaseEndDetector` are untouched.

## Lead helper note (2026-10-20) — car stat bars on the picker

Acceleration, top speed and cornering bars under the car `<select>`, computed from the physics:

- `wp_vehicle::stats` (new module, A's crate): `CarStats::measure(car)` runs the existing metric
  rigs (`zero_to_100`, `top_speed_kmh`, `skidpad` at 50 m) and `bars()` places them on **fixed**
  scales (0–100 16 → 3.5 s, 140 → 310 km/h, 0.55 → 1.05 g), so a new car never moves the others.
- `simcli::car_stats` + `simcli car-stats [--check]` (`just car-stats`, ~2 s release) write
  `data/stats/cars.json`; `apps/simcli/tests/car_stats.rs` fails when the table drifts from the
  physics (0.1 s / 2 km/h / 0.01 g) or the roster changes. **A, H: after a handling or car-file
  change, run `just car-stats` and commit the table.** simcli now depends on `wp_vehicle`.
- `wp_wasm`: `build.rs` embeds `data/stats/*.json`; new export `car_stats_json()` (no session
  needed; a car missing from the table is measured in the browser once, ~1 s).
- Testbed: `src/ui/carStats.ts` (`CarStatBars`) in `#car-stats` under the car row; updates on
  picker change and on every rebuild; option tooltips carry the numbers. `e2e/run.mjs` step 1b.
- Not yet in `apps/ui-kit` (garage / market lists use mock listings); the same table can feed
  them once G's garage picks real car ids.

### Follow-up (2026-10-20): stats of a modded build

`wp_wasm::build_stats` (new module, `wp_wasm` now depends on `wp_vehicle`): export
`car_stats_build_json(car_id, mods_json, mode)` validates and applies the mods with
`wp_economy::Economy::preview` (same path as installing), resolves the modified car against every
embedded tire / material (`data::resolve_with`), runs `CarStats::measure`, and caches per (car,
sorted mods, mode). ~0.3 s native, ~1 s in the browser per new build; an empty set returns the
stock row. Illegal sets (missing requirement, unknown id, class) are refused. Not called by the
testbed yet; it is what a live garage provider fills `GarageCar.stats` from (`docs/integration/ui.md`).

## Lead helper note (2026-10-21) — garages open repairs

Until now the map's `Garage` POIs did nothing (police skip them, the testbed did not draw them, and
the economy's "garage" was only the end-of-run screen). New module `crates/wp_wasm/src/garage_visit.rs`
plus `Session::check_garage` (native tests in `run_tests.rs`):

| Export | Returns |
|---|---|
| `garages_json()` | `GarageSite[]`: `index, name, pos, heading_rad, radius_m` (15 m, like `PoiInfo`), `kind` (`Legit` / `Crooked`), `district` |
| `garage_visit_json()` | `GarageOffer` or `null`: `seq, tick, site, wanted, health, quote` (G's `RepairQuote` at that shop, or `null`), `note` (`"nothing to repair"` / refusal), `cash, can_repair` |
| `garage_repair()` | pays the shop's `RepairAll` (damage recorded first, then G's `repair`), repairs the car in place (replay op `Repair`), releases the brakes; `GarageRepairReport` (`shop, paid, insurance_covers, reported_to_police, cash_after, ledger`); throws when refused |
| `garage_leave()` | closes the visit without repairing |
| `debug_place(entity, x, y, yaw, speed)` | e2e / debug teleport, not in replays |

Behaviour (sessions with a `PlayerRun`, i.e. live with police, not replays): after each live tick,
the player's car stopped (< 3 m/s) inside a garage radius with no pursuit (`Pursuit` / `Searching`)
opens a visit. The car is held on the brakes (logged inputs, same path as the end-of-run hold) until
**Repair** or **Leave**; a pursuit starting or the run ending closes it. The car has to leave every
garage radius before the next visit (no re-trigger while parked). Pricing uses the shop's kind,
its district's labour rate and the car's current wanted level, so crooked shops mark up and legit
ones flag `reports_to_police` at L2+. Outside the sim step: hashes and goldens unchanged.

Shop kind: `wp_world`'s POIs have no legit / crooked field yet (`docs/design/CITY.md` C3, open for
05 / 08). `garage_visit::shop_kind` uses the CITY.md §5.2 list by name (Vesper: Meridian
Motorworks legit; Sal's Paint & Panel and Gantry Garage crooked) and falls back to the district
(industrial / port crooked) for the procedural city's `Garage N`. Drop it when C3 lands.

Testbed: `src/ui/garageVisit.tsx` (`GarageVisitView`, kit primitives) in `#ui-garage`: shop name,
kind, district, condition, priced lines, labour / insurance / police badges, cash before → after,
**Repair (cost)** / **Leave**, and a 4 s "Repaired at …" toast. `scene.setGarages()` draws a ring
at the radius and a 40 m light beam per shop (blue legit, amber crooked). `main.ts`: construct,
`garageView.update(sim.garageVisit(), now)` per frame (not in replays), reset on rebuild, on
`window.__wp`. `e2e/run.mjs`: free drive → markers = shops → stop in the first shop → panel, input
held, "Nothing to repair" → **Leave** → input live.

Gaps: police do not act on a legit shop's report (would need a logged sim op that adds heat);
only `RepairAll` (no per-item repair, drive-damaged or scrap at the counter); no garage test lot
(backlog B07).
