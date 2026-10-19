# testbed-web

**Owned by workstream 04 (browser testbed).**

Vite + TypeScript + Three.js shell that loads `wp_wasm` (built with wasm-pack) and renders a ground
plane and one box per vehicle from `get_vehicle_state_json()`.

```sh
just web            # = npm install && npm run wasm && npm run dev → http://localhost:5173
```

Controls: arrows / WASD drive, Space handbrake, R reset.

Sim space is +Z up / +Y forward; the Three.js scene is +Y up, so all sim-space objects live under
a `world` group rotated −90° about X.

Requires Node 22+, `wasm-pack` (`cargo install wasm-pack`) and the `wasm32-unknown-unknown`
target (installed by `rust-toolchain.toml`). `src/wasm/pkg/` is generated and gitignored.

## Known gaps
Debug layers, cameras, HUD, telemetry, replay, scenario menu, gamepad: all prompt 04.
