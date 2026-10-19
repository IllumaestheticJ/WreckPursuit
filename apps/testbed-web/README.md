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
| `src/cameras.ts` | chase (spring-damped), hood, bumper, orbit, cinematic |
| `src/hud.ts` | speed/rpm/gear, badges, perf, damage silhouette, components, region energy, input + rack torque, help |
| `src/telemetry.ts` | rolling 10 s canvas plots |
| `src/input.ts` | keyboard ramps + Gamepad API with deadzones |
| `src/settings.ts` | debug layer table (hotkeys, `localStorage`) |
| `vite.config.ts` | serves `apps/simcli/scenarios` (simcli format v2) at `/scenarios/` and `data/` at `/data/` (+ listings) |
| `e2e/run.mjs` | Playwright check; `SHOTS=dir` writes screenshots, `BENCH=1` measures 1/8/24 cars |
| `e2e/city_v2_shots.mjs` | Port Vesper v2 screenshots → `docs/media/city-v2/` (`node e2e/city_v2_shots.mjs [dir]`) |

Cities: the "city" box (free drive) and the city chase use the city selector — Port Vesper v2
(`vesper_v2`, hills, default) or the flat grid (`procedural_12x12`); URL `?city=procedural_12x12`,
`?chase=1`. The greybox is `data/city/<sim.cityId()>.greybox.glb` (base64 fallback
`.greybox.b64.txt` on the published build). Automation: `__wp.lookAt(from, to)` places a fixed
orbit camera in sim coordinates, `__wp.setClip(near, far)`.

Sim space is +Z up / +Y forward; the Three.js scene is +Y up, so all sim-space objects live under
a `world` group rotated −90° about X.

Requires Node 22+, `wasm-pack` (`cargo install wasm-pack`) and the `wasm32-unknown-unknown`
target (installed by `rust-toolchain.toml`). `src/wasm/pkg/` is generated and gitignored.
The e2e script uses `playwright-core` with the Chromium at `$CHROMIUM` (default: the
preinstalled `/opt/pw-browsers/chromium-1194`).

## Perf (headless Chromium, SwiftShader, real vehicle + damage)
sim 0.09 / 0.62 / 3.0 ms per 60 Hz frame and render 0.7 / 3.4 / 9.2 ms at 1 / 8 / 24 cars;
city + ~190 traffic agents 0.27 ms sim. City chase after warm-up: grid 1.5 ms, Port Vesper v2
2.8 ms sim per 60 Hz frame (its 6.8 MB greybox parses in ≈ 2 s under SwiftShader). Re-measure in
desktop Chrome with a GPU.

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
