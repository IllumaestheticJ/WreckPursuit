# testbed-web

**Owned by workstream 04 (browser testbed).**

Vite + TypeScript + Three.js testbed over `wp_wasm`: drivable cars, debug layers, HUD,
telemetry plots, scenario menu and deterministic replay. Full reference (WASM ABI, hotkeys, how
to add layers/scenarios, perf): `docs/integration/testbed.md`.

```sh
just web            # npm install + release wasm + vite → http://localhost:5173
just web-e2e        # headless Chromium end-to-end check
npm run typecheck
```

| File | What |
|---|---|
| `src/main.ts` | session/scenario/replay glue, frame loop, controls panel, `window.__wp` automation hook |
| `src/wasm/sim.ts`, `src/wasm/layout.ts` | typed wrapper + packed layouts (mirror `crates/wp_wasm/src/pack.rs`) |
| `src/scene.ts` | renderer, sky from time of day, ground + 10 m grid, `data/city/*.glb`, barriers, contacts, intents |
| `src/vehicle_view.ts` | body box / skinned body, wheels (steer + spin), lattice, stress, tire forces, suspension, CG + velocity |
| `src/skin.ts` | skin binding blend; synthesised hull binding for grid lattices |
| `src/cameras.ts` | near / far chase (car-relative spring), hood, bumper, cockpit, side tracking, TV / cinematic, orbit; camera cycle / look-back are rebindable actions; right stick looks around |
| `src/hud.ts` | speed/rpm/gear, badges, perf, damage silhouette, components, region energy, input + rack torque, help |
| `src/telemetry.ts` | rolling 10 s canvas plots |
| `src/input.ts` | keyboard ramps + Gamepad API (deadzone + steering curve, analogue pass-through), read through the bindings; release on blur / pad disconnect |
| `src/bindings.ts` | rebindable actions, defaults, `wp.bindings.v1` load/save, display names, F1 help lines |
| `src/ui/controls.ts` | controls panel (F9 / “controls” button): rebind keys and pad controls, deadzone / curve, reset to defaults; per-device input shaping sliders + live test bars (same values as the game menu's Settings → Controls) |
| `src/ui/carStats.ts` | stat bars under the car picker (acceleration, top speed, cornering) from `sim.carStats()` (`data/stats/cars.json`, measured on the physics rig); options carry the numbers as tooltips |
| `src/ui/modShop.ts` | "mods" button: install mods on the car (catalog from `sim.modCatalog`, checked by `sim.buildVisuals`); each change respawns with the build as the player's scenario `mods`; builds saved per car (`wp.builds`), `?mods=a,b` |
| `src/render/mod_parts.ts` | draws a build's parts (`entity_visuals_json`): wings, splitters, kits, cages, bars, rims, tread, brakes, paint finish, vinyls; fitted by ray-probing the panel mesh and skinned to the mount region's lattice nodes (dent / hinge / detach with it); body pieces join the car's per-material batches (`PanelBody.addModParts`), so a build adds one draw per mod material |
| `src/settings.ts` | debug layer table (hotkeys, `localStorage`) |
| `src/render/fx/` | crash effects: sparks, smoke / steam / tyre smoke, glass shards, skid marks, debris chips; `?fx=low\|med\|high`, `__wp.fx.setQuality()` (`docs/integration/crash-fx.md`) |
| `src/photo.ts` | photo mode (F7, live or replay): freeze, free camera, filters, save / copy PNG |
| `vite.config.ts` | serves `apps/simcli/scenarios` (simcli format v2) at `/scenarios/` and `data/` at `/data/` (+ listings) |
| `e2e/photo.mjs` | photo mode check (`npm run e2e:photo`): freeze, free camera, filters, save, clipboard, replay |
| `e2e/run.mjs` | Playwright check; `SHOTS=dir` writes screenshots, `BENCH=1` measures 1/8/24 cars |
| `e2e/crash_fx.mjs` | crash effects check + `FX_SHOTS=dir` screenshots (run by `e2e/run.mjs` too) |
| `e2e/controls_settings.mjs` | `npm run e2e:controls`: per-device input shaping — keyboard defaults, AssistProfile speed sensitivity per device (keyboard 0.75 / gamepad 0.70 / wheel 0), persistence, test bars + test mode, saturation / dead zone effect, panel ↔ menu, preset highlight |
| `e2e/presets.mjs` | `npm run e2e:presets`: Rookie / Street / Sim buttons set damage mode + assists in one click, a hand-ticked box shows "custom", the preset survives a reload |
| `e2e/race_spots.mjs` | `npm run e2e:race-spots`: honk-to-start race spots (`src/race_spots.ts`): spots listed and drawn, prompts, honk while stopped starts the race on the same map (`CITY=ashpeak` for the combined map) |
| `e2e/gamepad.mjs` | fake-gamepad + rebinding check (`npm run e2e:gamepad`; run by `e2e/run.mjs` too) |
| `e2e/mods_shots.mjs` | modified-car screenshots (stock / street / widebody / protection builds; `node e2e/mods_shots.mjs [dir]`, `ONLY=street`, `CAR=…`) |
| `e2e/signals.mjs` | `npm run e2e:signals`: traffic turn signals — a car whose draw record signals right / left blinks that side's corner lamps (lit and unlit frames, read back from the fleet instance or VehicleView), never the other side; hazards light both; the player's cosmetic stalk (`-` / `=`) |
| `e2e/city_v2_shots.mjs` | Port Vesper v2 screenshots → `docs/media/city-v2/` (`node e2e/city_v2_shots.mjs [dir]`) |

Cities: the "city" box (free drive) and the city chase use the city selector — Port Vesper v2
(`vesper_v2`, hills, default) or the flat grid (`procedural_12x12`); URL `?city=vesper_v2` or
`?city=procedural_12x12` opens free drive in that city (ticks the box), `?chase=1` the chase.
The greybox is `data/city/<sim.cityId()>.greybox.glb` (base64 fallback `.greybox.b64.txt` on the
published build; a failed load is retried on the next session in that city). Automation: `__wp.lookAt(from, to)` places a fixed
orbit camera in sim coordinates, `__wp.setClip(near, far)`.
| `e2e/playtest_chase.mjs` | city chase playtest: screenshots, overlapping HUD panels, layer + camera keys, F1 help (`node e2e/playtest_chase.mjs [outDir] [WxH]`) |
| `e2e/chase_perf.mjs` | city chase perf (sim / views / render ms, draw calls); `PANELS=0` A/B for panel meshes, `VIEWPORT=WxH` |

Assist presets: the **preset** row (Rookie / Street / Sim, `wp_vehicle::AssistPreset`) sets the
damage mode and the assists in one click and is remembered per viewer (`wp.assistPreset`). Any
assist box ticked afterwards turns it into a custom setup, which is remembered as before
(`wp.assistOverrides`). With no preset chosen, the owner's defaults still apply (TCS, ESC and auto
gears off).

Sim space is +Z up / +Y forward; the Three.js scene is +Y up, so all sim-space objects live under
a `world` group rotated −90° about X.

Requires Node 22+, `wasm-pack` (`cargo install wasm-pack`) and the `wasm32-unknown-unknown`
target (installed by `rust-toolchain.toml`). `src/wasm/pkg/` is generated and gitignored.
The e2e script uses `playwright-core` with the Chromium at `$CHROMIUM` (default: the
preinstalled `/opt/pw-browsers/chromium-1194`).

## Input shaping (Settings → Controls)

Per-device profiles (keyboard, gamepad, wheel — a pad whose id mentions "wheel") from the game
menu's `GameSettings.controls` (ui-kit `src/menu/controls.ts`). `readInput` applies the active
device's profile after its own processing: keyboard → after the key ramps; pad → after the
stick deadzone + steering curve. Steering goes through `shapeSteer` (dead zone, saturation,
sensitivity exponent), each pedal through `shapePedal` (dead zone, saturation).

- `steer_speed_sensitivity` of the player's AssistProfile = the device's speed sensitivity while
  Steering help is on, 0 while it is off, in both paths of `applyAssistPreset` (named preset and
  per-box overrides) and in `choosePreset`; re-applied when the settings change or the device
  switches. Touch keeps `TOUCH_ASSISTS`.
- Preset highlight (Rookie / Street / Sim): `activePreset()` in main.ts replaces
  `sim.assistPresetOf` — the same exact comparison of damage mode + AssistProfile, except that
  `steer_speed_sensitivity` only has to agree on on / off (> 0). Its value is a per-device control
  setting, not part of a preset, so moving the slider keeps the preset lit.
- `probeInput()` (input.ts) computes the same processed values from what is physically held now
  (a raw key set tracked before any menu / panel swallows the keydown, plus the pads), with its own
  ramps, firing nothing — the test bars of both the menu and the controls panel use it.
- Automation: `__wp.inputProbe(nowMs?)` (pass a time for a deterministic ramp), `__wp.app.input`
  (the processed DriverInput sent to the sim), `__wp.deviceControls()`, `__wp.activeDevice()`.

## Perf (headless Chromium, SwiftShader, real vehicle + damage)
sim 0.09 / 0.62 / 3.0 ms per 60 Hz frame and render 0.7 / 3.4 / 9.2 ms at 1 / 8 / 24 cars;
city + ~190 traffic agents 0.27 ms sim. City chase after warm-up: grid 1.5 ms, Port Vesper v2
2.8 ms sim per 60 Hz frame (its 6.8 MB greybox parses in ≈ 2 s under SwiftShader). Re-measure in
desktop Chrome with a GPU.

Browser frame budget (city chase, low-poly dark): `npm run profile` (`e2e/frame_profile.mjs`)
prints JS ms per frame-loop section, draw calls / triangles / raster ms per render feature
(`tagFeature` roots in `src/perf.ts`), steady vs one-off allocation and the top allocators;
`npm run e2e:perf` (`e2e/perf_check.mjs`) gates the desktop chase / rain budgets and prints the
phone (touch) profile's numbers for information only. PC is the target platform; no phone
support is planned; performance work is still welcome everywhere (owner, 2026-10-22, playtest
row 145; `docs/INTEGRATION.md` "Perf budgets").
Numbers and history: `docs/integration/browser-perf.md`.

## Known gaps
See `docs/integration/testbed.md` → Known gaps.
## Multiplayer page (workstream 07)
`net.html` + `src/net/` is the multiplayer client: start `just server` (or
`just server-bots`), then open `http://localhost:5173/net.html?server=ws://localhost:8080&name=me`.
It runs `wp_net::client::Client` through the `net_*` functions in `crates/wp_wasm/src/net.rs`
(prediction of the own car, 100 ms interpolation of the rest), with a role picker, race lobby,
captain map (2D canvas: units, incidents, heat, right-click command radial) and a debug overlay
(RTT, mispredictions, correction size, bandwidth, lattice hashes). Keys: arrows/WASD drive,
Space handbrake, L lights, Q siren, H horn. Kept separate from the testbed page on purpose so
the two evolve independently.
