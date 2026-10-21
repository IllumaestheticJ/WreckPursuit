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
| `props_static` / `props_moved` | – | `Float32Array` 11 × props / 8 × moved | City props (parked cars, cones; `props.rs`, lead helper `lead/draw-props`): static list once per session `[kind, x, y, z, qx, qy, qz, qw, hx, hy, hz]`, then per frame only props whose pose changed `[index, x, y, z, qx, qy, qz, qw]`. Drawn by `render/props.ts`. See `docs/integration/props.md`. |
| `traffic_draw` | – | `Float32Array` 10 × agents | Every agent at any LOD (`lod.rs`): `[agent id, entity (0 = none), x, y, z, qx, qy, qz, qw, flags]`, flags as `traffic_agents`. Embodied agents report their vehicle's state; kinematic ones are seated on the ground exactly as the sim places a promoted car (`ground_pose`) and carried forward between 60 Hz traffic steps. `lod_draw_stride()` = 10. |
| `police_draw` | – | `Float32Array` 10 × units | Every police unit, bound or not: `[unit id, entity (0 = none), x, y, z, qx, qy, qz, qw, flags]`, flags 1 helicopter, 2 pursuing. |
| `session_car_models` | – | JSON | `{ cars, traffic, police }`: every car model the session can show (scenario cars, physical traffic, police) for preloading. |
| `car_model_info` | `car_id` | JSON | `{ car_id, body_size_m, wheels }` of a model without a vehicle (builds the instanced far-LOD mesh). |
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
| `crash_take`, `crash_replay_start(seq)`, `crash_replay_stop`, `crash_replay_active`, `crash_tuning_json` / `_set`, `crash_detect_enable`, `crash_buffer_ticks` | | | Instant crash replay (lead): see `docs/integration/crash-replay.md`. |
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

**Player actions are rebindable** (round 12b): the **controls** button or F9 opens a panel with
two keyboard and two gamepad slots per action; saved in `localStorage` `wp.bindings.v1`; the F1
help lists the live bindings. Defaults (`src/bindings.ts`):

| Action | Keyboard | Gamepad (standard mapping) |
|---|---|---|
| steer left / right | A / D, ← / → (ramped) | left stick X (analogue; deadzone 12 %, curve 1.0, both adjustable) |
| throttle / brake | W / S, ↑ / ↓ (ramped) | RT / LT (analogue, 5 % deadzone) |
| clutch (used with **clutch: manual**) | unbound — bind any key (ramped in 0.1 s / out 0.3 s) | unbound — bind a button, trigger, stick direction or a **wheel pedal axis** (full range) |
| handbrake | Space | A or X |
| shift up / down | Shift / Ctrl | RB / LB |
| horn | Q | L3 |
| look back (hold) | V | R3 |
| next camera | C (remembered in `wp.testbed.camera.v1`) | View |
| reset car | R | Y |
| headlights | O | d-pad ↓ |
| follow the next car | ` | – |
| city map | M or Tab | d-pad → |
| pause (replay: play/pause) | P | Menu |
| help | F1 or ? | – |
| controls panel | F9 | – |
| audio mute | **F8** (was V, which also held look-back) | – |
| repair bill | F | – |
| crash replay: play / skip (lead, `docs/integration/crash-replay.md`) | Enter | d-pad ↑ (R2 / RT is the throttle) |

Fixed keys:

| Key | Action |
|---|---|
| 1–8 | camera: near chase (default), far chase, hood, bumper, cockpit, side tracking, TV / cinematic, orbit / free (debug) |
| mouse drag | look around (springs back on release; right stick on a pad, not rebindable) |
| . / , | replay frame step forward / back (4 ticks) |
| [ / ] | replay speed down / up |
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
| 9 | traffic signals (was 6 until round 8: 6 is the side camera): mast-arm head at each stop line + far-side pedestal head, white stop line; lit lamp unlit-bright with an additive glow that strengthens at night (`signals_view.ts`, 6 InstancedMeshes for the whole city) |
| 0 | panel meshes (machine R, `src/render/panels.ts`; on by default) — off shows the lattice view |
| I | traffic / police intent arrows |
| K | component health panel |
| E | region energy panel |
| F4 | event feed — was V, now look-back — (component failures, parts, glass, region damage, total loss, violations) |
| J | telemetry plots |
| N | input display + rack-torque bar |

Gamepad: the first connected pad, read every frame. Its values go to the sim as read (deadzone +
steering curve, no keyboard-style ramps), so the sim's gamepad `AssistProfile` preset is the only
filter. The source switches to the device used last and main applies
`AssistProfile::for_source` (+ the owner's assist-box overrides) on every switch, logged as an
assists op. A pad disconnect mid-drive releases everything and falls back to the keyboard preset
(like keyboard blur). Bound actions win over debug-layer keys.

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
vehicles, or instanced from `police_draw()` for internally driven units) are white with a
red/blue light bar that strobes while their siren is on (plus a point light); traffic keeps one
of 8 liveries (`variant`) at every LOD (instanced far / kinematic, full view once physical, see
"LOD pop-in" below); every car has head/tail lights (headlights: O). The wanted
level / heat readout is M's UI-kit chase HUD (`apps/ui-kit/src/live`, mounted by
`src/ui/overlay.ts`), fed by `police_status()` mapped onto the kit's `PoliceReadout`
(`src/police.ts`). A centre banner shows **PURSUIT** / **SEARCHING** (escape %) while chased and
**ESCAPED** / **CAPTURED** for 6 s after a pursuit ends (`last_outcome`, `outcome_age_s`). A
capture, or the end of a scripted scenario, pops up the repair bill; a bill that popped up
closes on the next session, one the user opened (F) stays.

### Weather (lead, `lead/weather-in-game`)
**weather** selector (`scenario` = the scenario's own weather, off, clear, overcast, rain, heavy
rain, fog, dynamic) restarts the running session with that weather written into its scenario
(`scenario_with_weather`), so replays and `simcli replay` carry it; **rainy night** = the city
chase in a downpour on a soaked road (`city_chase_rain_json`; `?chase=rain`, `?weather=<id>`).
`src/render/weather.ts`: instanced rain streaks animated in the vertex shader (one draw, ≤ 5000
lines), wet road materials (`asphalt`, `highway_concrete`, `parking`, `sidewalk`, `kerb`, `bridge`
and the flat ground: darker, glossier, sky-dome reflections, puddle sheen) driven by the 8 m
water grid (`weather_grid()`, RG8 texture refreshed at 1 Hz), spray points behind cars
(`weather_spray()`), `FogExp2` density 3 / visibility, cloud-dimmed sun, wipers in hood /
bumper / cockpit cameras, and a HUD line (`#weather-hud`) with regime, rain rate, visibility,
temperature, road state and an aquaplaning warning (player car's `Telemetry::aquaplane`).
Exports (`crates/wp_wasm/src/weather.rs`): `weather_choices`, `scenario_with_weather`,
`scenario_weather_choice`, `city_chase_rain_json`, `weather_info` (18 floats, `WX` in
`render/weather.ts`), `weather_grid_meta`, `weather_grid`, `weather_spray`. Check:
`npm run e2e:weather` (also run by `npm run e2e`); screenshots in `docs/media/weather/`.

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

## Note from M (UI kit, round 9, 2026-10-19): city map HUD

Owner request "we need a game map HUD with police". Small glue in your crate/app, please review
(details in `docs/integration/ui.md` → "City map HUD"): `crates/wp_wasm/src/map.rs`
(`map_static_json()`, `map_dynamic(player)`, read-only), `src/ui/map.ts` mounting the kit's
`mountMapHud`, `main.ts` wiring (M / Tab toggle the full map, neutral input while it is open),
two layers (`'` minimap, `;` minimap signals), `e2e/run.mjs` map checks, `e2e/map_shots.mjs`.
**Key change:** audio mute is now **V** (was M) and "follow the next car" is **backquote** (was
Tab). The minimap sits in the left column under the input panel (top 312 px) because your
bottom-right holds the event feed and the audio panel; move it with `minimapPlacement`.

## Lead feedback (round 12b, 2026-10-19) — gamepad + rebindable controls (owner request)
Base: `origin/lead/round12` (round 12's merges; `main` catches up when its gate is green), not main.
1. **Gamepad** via the browser Gamepad API in `apps/testbed-web/src/input.ts`: analogue steer
   (left stick, with a small deadzone + optional curve), throttle/brake on the triggers (RT/LT),
   handbrake, shift up/down, camera cycle/look-back, horn, reset. Standard mapping first; handle
   connect/disconnect mid-drive (release everything on disconnect, as keyboard does on blur).
   Feed the real analogue values to the sim (no keyboard-style ramps) so the existing gamepad
   assist profile applies; check `AssistProfile` selection follows the active device.
2. **Rebindable controls**: a small settings panel (keyboard + gamepad) that lists every action,
   lets the player press a key/button to rebind, resets to defaults, and saves to localStorage
   (`wp.bindings.v1`, try/catch). Show the current bindings in the F1 help.
3. e2e: a fake gamepad (override `navigator.getGamepads`) drives forward and steers; a rebind
   survives a reload. Measure input latency stays one frame.
## Lead helper note (`lead/fix-lod-popin`, 2026-10-19) — LOD pop-in (playtest row 25)
Owner: "some cars load LOD too slow": cars pop in late or look wrong for a while. Reproduced on
`main` in headless Chromium (`e2e/lod_shots.mjs`, `docs/media/lod/before_*`); all four suspects
applied:
1. **Panel meshes loaded late.** `PanelBody` fetched `data/panels/meshes/<car>.json` the first
   time a model appeared: the traffic model at the first promotion, the police cruiser at the
   first police spawn. Until the file arrived (and always for one frame, since the build ran in a
   promise callback even when cached) the car was the lattice hull / debug box. Locally that is
   5–25 ms; on the published link it is a 180–230 KB fetch per model.
2. **Promotion swap.** Kinematic agents were 1.8 × 4.4 × 1.4 m boxes (grey; red when braking,
   blue when yielding, yellow when physical without a vehicle); a promoted agent became the
   `debug_sedan` panel mesh in `#7a8a99` with tire-force arrows and a CG marker on the same frame
   (`before_promotion_strip.png`). Physical agents were hidden whenever *any* physical traffic car
   existed, so one whose sim vehicle could not be spawned (vehicle limit) vanished.
3. **Draw distance.** The instanced boxes were never culled, but `wp_world` spawned / despawned
   agents 150 m from the player: cars materialised in plain view (closest measured 226 m in a 5 s
   window of the chase; the rule allows 150 m). Fixed in `wp_world` (see
   `docs/integration/world.md`, "Lead helper: LOD pop-in"): 320 m.
4. **Police.** Same mesh-on-first-spawn delay for the cruiser; unbound units were boxes.
### What changed
| File | Change |
|---|---|
| `crates/wp_wasm/src/lod.rs` (new), `lib.rs` | `traffic_draw()`, `police_draw()`, `lod_draw_stride()`, `session_car_models()`, `car_model_info()` (table above). Read-only: hashes and goldens unchanged by these. Native tests: quaternion maths, model list, and **draw poses stay continuous through 15 real promotions/demotions in 12 s of city chase (worst swap 0.06 m, worst tick 0.08 m)**. |
| `src/render/fleet.ts` (new) | Instanced cars: per model a full mesh (the model's real panel mesh merged at rest: paint with per-instance livery, detail, head/tail lamp markers where `VehicleView` puts them, police light bar) for cars < 150 m from the camera (192 per model; promotions happen at 80 m, so the swap is always full mesh → full mesh), a ~130-triangle proxy beyond (and while a mesh is loading). Shared 8-colour `TRAFFIC_PALETTE` by `variant`. |
| `src/render/lod.ts` (new) | `LodDirector`: per frame, each traffic agent / police unit is drawn by exactly one of its `VehicleView` (once the sim spawned it **and** its mesh is ready) or the fleet, same model / livery / pose — so promotions and demotions swap the drawer on one frame with no gap. `LodLog` records per-frame draw states (missing-mesh frames, swap pose jumps with ±3 frames, appear / vanish distances, placeholder time). |
| `src/render/panels.ts` | `preloadCarMeshes()`, `carMeshNow()`, `meshTimings`; a preloaded mesh builds in the `PanelBody` constructor (no placeholder frame). |
| `src/vehicle_view.ts` | `drawable` (mesh ready or panels layer off); while loading, a proxy in the car's livery instead of the debug box / lattice hull; sim-spawned traffic / police (`background`) show per-car debug overlays (tire forces, slip colours, suspension, CG / velocity) only when focused (Tab), so a promotion does not sprout arrows. |
| `src/main.ts` | Session start: `session_car_models()` → preload (first frame of the page waits ≤ 5 s for them); promoted traffic views get their agent's livery; `lod.update()` after the camera moves; `__wp.lod`, `__wp.meshTimings`. |
| `src/scene.ts` | Agent boxes removed; `updateHelicopters(police_draw())` keeps the helicopter boxes. |
| `e2e/run.mjs` | LOD check (fresh city chase, ≥ 12 s sim, ≥ 2 swaps): no car without its mesh after the first second; swap pose jump < 0.5 m; traffic appears / vanishes ≥ 300 m from the player. |
| `e2e/lod_shots.mjs` (new) | Before/after strips + frame logs into `docs/media/lod/` (runs against an older testbed too: `node e2e/lod_shots.mjs <out> before <main-worktree>/apps/testbed-web`). |
### Measurements (headless Chromium, SwiftShader, cloud 4 vCPU)
| | `main` | this branch |
|---|---|---|
| Closest traffic pop-in / pop-out from the player (`lod_shots.mjs`, chase) | 226 m / 226 m | 359 m / 462 m |
| Promotion: drawer change | grey/red box → `#7a8a99` debug_sedan + debug arrows | same model, livery and lamps (`*_promotion_strip.png`) |
| Frames with a car drawn without its mesh (after 1 s) | n/a (placeholder until fetched) | 0 |
| Car mesh fetch (local) | on first appearance of a model (5–25 ms + 1 frame) | at session start, before the first frame |
| Swap jump: drawn vs sim pose (e2e) / sim pose tick by tick (wasm test) | — | 0.000 m over 7 swaps / 0.06 m over 15 swaps |
| City chase frame (`chase_perf.mjs`, 20 s) | 899 ms, render 13.8 ms, 541k tris, 622 calls | 920 ms, render 26.6 ms, 627k tris, 470 calls |
SwiftShader frame time is dominated by rasterising the city and is unchanged; renderer CPU time
roughly doubles (instance buffers of the full-mesh fleet: up to 192 instances × ~4k triangles per
model within 250 m). `FULL_LOD_M` was then lowered to 150 m (owner's call, 2026-10-20); see the
follow-up measurement below.
Follow-up 2026-10-20: with the 320 m sightline the city no longer reached its traffic target
(`world_traffic_soak_observer`); fixed in `wp_world` without moving the spawn ring (see
`docs/integration/world.md`, "Follow-up"). `npm run e2e` LOD checks on that build: 0 frames
without a mesh, swap jump 0.000 m, traffic appears ≥ 322 m / vanishes ≥ 342 m from the player.
### Known gaps
- Proxy → full instanced mesh switches at 150 m from the camera (a small far-LOD change, by design;
  a 4.5 m car is ~20 px tall there at 1280×720).
- Damage done while physical is lost on demotion (the instanced mesh is undented); traffic
  demotes 100 m+ away, so it is rarely visible.
- Kinematic poses are carried forward between traffic steps, not interpolated by the frame's
  alpha (≤ 1 physics tick, 0.125 m at 30 m/s).
- Bench / car-selector cars outside the session list still load on demand (with the proxy).
### Gate (2026-10-19)
fmt, clippy `-D warnings`, `cargo test --workspace` (goldens blessed in the `[bless]` commit), wasm
build, tsc, `just web-e2e`: all checks pass (LOD: 0 frames without a mesh, 7 swaps with 0.000 m
hand-over jump, traffic appears ≥ 327 m / vanishes ≥ 353 m). One earlier run failed the capture /
"Repair & continue" checks and logged an audio `setValueCurveAtTime` overlap page error; `main`
fails the same three checks in this container, so they are pre-existing and flaky here (J / lead).
### Response — round 12b (2026-10-20, workstream 04, base `origin/lead/round12` @ `05f92f8`)
1. **Gamepad** (`src/input.ts`): the first connected pad is read every frame through the
   bindings. Left-stick steer (deadzone 12 %, optional response curve `sign·|v|^k`, k 1.0–2.5),
   RT/LT throttle/brake (5 % deadzone), A/X handbrake, RB/LB shifts, View camera cycle, R3 look
   back, L3 horn, Y reset, Menu pause, d-pad ↓ headlights, d-pad → map; right stick still looks
   around (`cameras.ts`, not rebindable; its old hard-coded View/R3 handling moved to the
   bindings). Values reach the sim **as read** (no ramps), so the sim's gamepad assist preset is
   the only filter. Source = device used last; main applies `AssistProfile::for_source` (+ owner
   overrides) on every switch — e2e checks the sim's profile matches the gamepad preset on the
   fields that differ (`steer_speed_sensitivity`, `countersteer_help`, `throttle_filter`,
   `brake_filter`) and returns to the keyboard preset. `gamepaddisconnected` (or the pad
   vanishing from `getGamepads()`) releases everything and falls back to the keyboard.
2. **Rebindable controls**: `src/bindings.ts` (19 actions, two keyboard + two pad slots each,
   defaults, `wp.bindings.v1` load/save with try/catch, validation of saved data) and
   `src/ui/controls.ts` (panel: **controls** button or F9; click a slot then press a key / pad
   button / move a stick or trigger; Esc cancels, Delete clears; a key taken from another action
   is moved, not duplicated; deadzone + curve sliders; reset to defaults). Driving input is
   neutral while the panel is open, and keys go only to the panel. The F1 help lists the live
   bindings (`hud.setControlsHelp`). A bound action wins over a debug-layer key.
   **Key change:** audio mute **V → F8** — since round 9 V both muted and held look-back.
   Look-back stays on V (e2e/cameras.mjs relies on it). Screenshot: `docs/media/controls_panel.png`.
3. **e2e** `e2e/gamepad.mjs` (run by `npm run e2e` at its end; alone `npm run e2e:gamepad`),
   16 checks, all pass: fake standard pad (`navigator.getGamepads` override) drives
   (9.3 m/s after 2.5 s) and steers (applied steer 0.55); analogue pass-through exact
   (stick 0.5 → 0.432, RT 0.4 → 0.368); **latency: pad change → `app.input` 1 frame,
   → the sim's applied throttle (telemetry `in_throttle`) 1 frame**; View cycles the camera,
   R3 holds look-back; disconnect with throttle + full lock + handbrake held → all 0, keyboard
   preset; panel blocks input; key + pad rebind saved, survives a reload, rebound key drives and
   the old one does not, F1 shows it; reset to defaults clears `wp.bindings.v1`.
Gates: `cargo fmt --check`, `cargo clippy --workspace -D warnings`, wasm build, `tsc`,
`just web-e2e` (64 PASS incl. weather + gamepad) and `e2e/cameras.mjs` all green.
`cargo test --workspace --no-fail-fast`: 873 pass, **3 fail on the round-12 base** (no Rust
changed in this round): simcli `goldens_match` (city_blocked_parked_player_{grid,vesper},
golden_city_chase, golden_night_pursuit_escape, panels_frontal_56_hood,
police_sideswipe_patrol_day, weather_aquaplane_{control,puddle}, … hash mismatches),
simcli `all_scenarios_pass…` (`blocked_parked_player_vesper`: `traffic_turnarounds` 0, want
1–30), and wp_net `four_clients_three_minutes_in_port_vesper` (p0 `Escaped`, expected
`Captured`). Lead: probably needs re-blessed goldens / re-tuned expectations after round 12's
merges.
Changes outside my folders: none.

### Response — round 12c clutch (2026-10-20, lead follow-up, playtest row 59)
Base: merged `origin/lead/round12` @ `6b9fa40`.
1. **`clutch` action** (`src/bindings.ts`, analogue): shown in the F9 panel and the F1 help,
   saved in `wp.bindings.v1`. Default **unbound** on keyboard and pad: every letter key is already
   a game or debug-layer key (E is the region-energy layer) and the pad's triggers / shoulders
   are taken. Older saves migrate in place (missing actions take their defaults; e2e checks an
   old save keeps its other bindings). Wheels: the reader now takes **every connected
   pad/wheel** (pedals can be a separate device), and the panel binds an axis that rests at one
   end as a **full-range pedal axis** (`{ axis, dir, full: true }`, 0 at rest → 1 at full press).
2. `DriverInput.clutch` gets it (keyboard ramp 10/s in, 3.5/s out; pad/pedal raw, 5 % deadzone);
   the HUD's `clt` bar shows it.
3. **clutch: automatic / manual** select next to the assist boxes = `AssistProfile.auto_clutch`.
   Default automatic (today's auto-clutch). Manual uses the player's clutch directly. The choice is
   remembered with the other assist overrides (`wp.assistOverrides`), applied over every device
   preset. A HUD badge shows `A-CLU` / `M-CLU`.
4. e2e (`e2e/gamepad.mjs`, now 23 checks): old save migrates; clutch bound to a key and a pedal
   axis, survives a reload; manual survives a reload with `auto_clutch` false; pedal half travel →
   0.47 (5 % deadzone); clutch held + full throttle from standstill → `clt` 1, engine 6 680 rpm,
   car 1.39 → 1.23 m/s (coasting: drive decoupled); clutch released → 13.7 m/s; automatic restores
   `auto_clutch`.

**Request to A (vehicle, `docs/integration/vehicle.md`), not edited by me:** the testbed now sends
`DriverInput.clutch` with `auto_clutch: false` when the player picks manual. Today manual has
anti-stall slip and no stalling: in 1st gear with the clutch out the car idle-creeps
(~1.4 m/s), which e2e tolerates. When stall + restart lands, please say how restart is triggered
(input bit or key), and I will bind it as an action. The testbed needs no other change for
stalling.

Gates: fmt, clippy `-D warnings`, wasm build, `tsc`, `just web-e2e` (117 PASS incl. weather,
gamepad, race) all green. `cargo test --workspace --no-fail-fast`: 936 pass, **6 fail, all in Rust
identical to `origin/lead/round12`** (this round changes no Rust): simcli `goldens_match`,
wp_race `grid_highway_sprint`, wp_sim `vesper_v2_cars_drive_up_the_kerb_and_along_the_sidewalk`,
wp_vehicle `front_drive_cars_understeer_mildly`, and the lead's race glue tests in wp_wasm
`race_tests::{short_sprint_with_ai_runs_to_results…, a_capture_mid_race_is_a_dnf…}` (player
DNF `FinishWindow`; "never captured"). The vehicle handling test failing too suggests a round-12
physics change behind most of them. Lead: please take these at the gate; I have not touched them.

### Response — round 12c reset in place (2026-10-20, lead follow-up, playtest row 65)
**R (and its pad binding, default Y) now resets in place** instead of sending the car to its
spawn:
- `crates/wp_wasm/src/reset.rs` (pure, deterministic) picks the pose: the nearest drivable
  lane within **15 m**, heading along it in the car's direction of travel (velocity when moving,
  else chassis forward; lanes running against it cost +4 m and the car still faces its own
  way). It uses the first spot along that lane (0, ±6, ±12, ±18 m) whose footprint (2.4 × 5.6 m)
  is clear of other sim vehicles, kinematic traffic agents, static props (parked cars, cones)
  and scenario barriers (oriented-box separating-axis test). With no clear lane spot (open
  ground, no city), the car is set upright where it is, or up to 8 m along its heading if that
  spot is blocked.
- `Session::reset_in_place(id, repair)` / wasm `reset_in_place(entity)` → JSON
  `{ x, y, yaw, on_lane, repaired }`. Upright at rest via `Sim::place_on_ground` (pitched to the
  road on hilly cities; `place_body` zeroes velocity and resets the vehicle). The car is repaired
  in free drive, city chase, races and scenarios, **but not in a career run**
  (`career_run_active()`; repairs are bought in the garage). Police heat, pursuit and the
  economy run are untouched. A race is told the car moved on purpose (`relocate_racer`, as for
  the old reset), so position and lap carry on. The event feed notes
  "reset in place — repaired / career: …".
- `reset_vehicle` (back to spawn, pristine) stays for scenario restarts.

**Change outside my crates (minimal, for replays): `apps/simcli`** — new replay op
`OpKind::ResetAt { xy, yaw, near_z, repair }` (byte tag 4: f32 x, y, yaw, near_z, u8 repair) →
`Sim::place_on_ground(id, xy, yaw, 0, Some(near_z))` then `repair_vehicle` if `repair`. It is in
`apply_op` and the encode/decode round-trip test, with the format notes updated. The op carries
the chosen pose, so playback (browser and `simcli replay`) needs no lane search. Old replays
decode unchanged; goldens unchanged. Owner of simcli (I) / lead: please review.

~~Request to the lead (wp_sim): ghost window~~ — **done, see "Ghost window" below** (the owner
asked for it directly). Note also:
`place_on_ground` on terrain cities overwrites the slot's `spawn`, so a later spawn reset goes
to the last in-place reset spot.

Tests: native `reset::tests` (box overlap, travel direction, open-ground fallback);
`tests::reset_in_place_repairs_near_the_crash_and_replays` (barrier crash → R → within 15 m,
clear of the barrier, upright, at rest, health 1.0; with `repair = false` the damage stays;
`ResetAt` in the replay, `simcli` verify matches); `tests::reset_in_place_in_the_city_snaps_to_
a_lane_and_keeps_heat`. e2e `e2e/reset.mjs` (run by `npm run e2e`, alone `npm run e2e:reset`):
barrier crash (health 85 %) → R → upright, 4.0 m from the crash, 0 m/s, health 100 %, the feed
notes it; the recording with the reset plays back with 0 drift; city chase → lane 0.2 m away,
heat 0.6 → 0.6; career run → `repaired: false`.
## Note from M (UI kit, round 12, 2026-10-20): HUD layout + game menu — for C to review
Owner rows 67 / 68, details in `docs/integration/ui.md` → "Round 12". Small glue in your app:
- **Layout:** minimap now bottom-right; the **audio panel moved bottom-left above `#controls`**;
  race HUD stacks under the speedo; the right column caps itself above the minimap. Run-time
  sizes by `src/ui/layout.ts` (writes `top` / `max-height` / `left` on `.wp-race-hud`,
  `#controls`, `#audio-panel`, `canvas.telemetry`, `.right-col`, `#weather-hud`). If you add a HUD
  element, add its selector to `e2e/hud_layout.mjs` so the overlap check covers it.
- **Game menu** (kit): `main.ts` mounts it. **Key change:** the rebindable "pause" action (P / pad
  Menu) now opens the pause menu while driving (replays keep play/pause), and **Esc** opens it when
  nothing else (map, controls panel, garage, run-end panel, race picker) is on screen. Its
  Controls button calls your `controls.open()` unchanged (your clutch / reset-in-place work should
  merge cleanly: `controls.ts` and `bindings.ts` are untouched).
- **Player mode** (`body.wp-player`, Settings → Developer off; default with `?menu=1` and the
  demo): hides the developer panels and suppresses the 3D debug layers via the new
  `layers.suppress(ids)` in `settings.ts`.
- `HudFrame.units` in `hud.ts` (km/h / mph).
- `e2e/menu.mjs` runs at the end of `npm run e2e` (36 checks).

### Ghost window after a reset in place (2026-10-20, owner request, playtest row 65)
The owner asked for the ghost window directly, so I added it in wp_sim (lead-owned crate; small
and additive, **lead please review**). Base: merged `origin/lead/round12` @ `608a50a`.
- **`wp_sim::physics::GhostHooks`** (Rapier `PhysicsHooks::filter_contact_pair`, now passed to
  `PhysicsWorld::step` instead of `&()`). A ghost chassis keeps contacts with static world
  geometry: colliders without a body or on a fixed body, i.e. ground slab, roads, terrain,
  sidewalks, buildings, rails, tunnel boxes and scenario barriers. It passes through everything
  that moves (other cars, debris) and through props even while they rest as fixed bodies (props
  now carry `user_data` `PROP_USER_BIT`). Ghosts are flagged with `user_data` `GHOST_USER_BIT`
  plus `ActiveHooks::FILTER_CONTACT_PAIRS`, so only pairs involving a ghost reach the hook. Every
  other pair keeps Rapier's default handling. No contact means no damage either way.
- **Wheel rays:** a ghost's suspension rays use `QueryFilter::exclude_dynamic()` plus a predicate
  skipping props (else its wheels "drive up" onto the car it overlaps), and every other car's
  rays skip ghost chassis.
- **`Sim::set_ghost(id, ticks)`** / **`Sim::ghost_ticks(id)`**: countdown in `step_fixed`
  (`update_ghosts`, before the physics step, vehicle order). If the window ends while the chassis
  AABB still overlaps another car's, the ghost is extended tick by tick until clear, capped at
  `GHOST_MAX_OVERSTAY_TICKS` (3 s).
- **simcli:** new replay op `OpKind::Ghost { ticks: u16 }` (byte tag 5) → `Sim::set_ghost`,
  in the round-trip test. `ResetAt` (tag 4) is unchanged, since it is already merged on several
  branches; the session logs `Ghost { 240 }` right after it.
- **wp_wasm:** `reset_in_place` ghosts the car for `reset::GHOST_TICKS` = 240 (1 s); its JSON
  gains `ghost_ticks`; new wasm `ghost_ticks(entity)`. **Testbed:** the player's car blinks at
  8 Hz while it is a ghost (`root.visible`, so no shared materials are touched).
- **Determinism / goldens:** with no ghosts the hook is never called and the wheel-ray
  predicate accepts every collider. The simcli golden report (every FAIL line and every first
  mismatch hash) is **bit-identical with and without this change** on the merged base (20
  goldens fail on the base itself, see below).

Tests: wp_sim `ghost_passes_through_cars_but_not_the_world` (a ghost at 10 m/s drives through a
parked sedan: no car–car contact, no damage, the parked car does not move, the ghost stays on
the ground, deterministic hash; without the ghost the same run crashes; a ghost still stops at a
static barrier) and `ghost_overstays_while_overlapping`. wp_wasm reset test checks the 240-tick
window, the `Ghost` op in the recording, solid afterwards, `simcli` verify. `e2e/reset.mjs`:
city-chase reset → `ghost_ticks` 240, the car blinks, then solid and drawn again; a recording
with the reset plays back with 0 drift.

Known gaps: traffic and police AI react to car positions, not physics rays, so they still
brake or steer around a ghost (no physics query besides wheel rays and the ground-height probe
exists in wp_sim/wp_world/wp_police). The overlap check at the end of the window looks at other
cars only, not props or debris.

Gates (ghost window): fmt, clippy `-D warnings`, wasm build, `tsc` green. `cargo test
--workspace --no-fail-fast`: 1003 pass, 5 fail, **all failing identically on the pre-ghost
code** (same panic text / race results / golden hashes): simcli `goldens_match` (20 scenarios),
wp_opponent `live_police_chase_ends_in_escape_or_capture_without_stuck_cars`, wp_race
`hollins_hill_sprint`, `loop9_circuit`, `lantern_checkpoint_draws_the_police`. (wp_wasm's
race tests and wp_vehicle's handling test now pass on the merged base.) `just web-e2e`: new
suites pass (gamepad, reset incl. ghost), 3 top-level failures that also fail on the pre-ghost
code: `e2e/perf_check.mjs` (software-GL frame-time / allocation / material-version budgets),
`e2e/touch.mjs` (layout 640×360: chase × banner overlap), and `e2e/crash_fx.mjs` (effects
1.18 KB/frame allocation — passes when run alone, so load-sensitive).
### Follow-up (2026-10-20): `FULL_LOD_M` 250 → 150 m
Owner's call after the render-cost note above. A/B on the same container (`chase_perf.mjs`, 20 s
city chase, two runs each; absolute numbers ran higher than the day before, compare within the
table only):
| `FULL_LOD_M` | triangles | render ms | frame ms |
|---|---|---|---|
| 250 m | 667k / 667k | 195 / 179 | 875 / 860 |
| 150 m | 591k / 603k | 170 / 141 | 828 / 787 |
≈ 10 % fewer triangles, ≈ 5 % faster frames headless. Promotions happen 80 m from the player, so
the hand-over is still full instanced mesh → full VehicleView.
Gate at 150 m (rebased on the lead's density follow-up): wasm build, tsc, `npm run e2e` all pass
(LOD: 0 frames without a mesh, 3 swaps with 0.000 m hand-over jump, traffic appears ≥ 321 m /
vanishes ≥ 373 m from the player; weather and race e2e also pass).
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
