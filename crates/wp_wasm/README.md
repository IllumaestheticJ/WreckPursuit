# wp_wasm

**Owned by workstream 04 (browser testbed).**

wasm-bindgen surface for the browser. Current JSON API (see `src/lib.rs` table): `init()`, `step(dt) -> alpha`, `get_vehicle_state_json()`, `get_vehicle_info_json()`, `set_input_json(json)`, `reset_vehicle()`. Car data is embedded with `include_str!`.

Build: `cargo build --target wasm32-unknown-unknown -p wp_wasm` (check) or `cd apps/testbed-web && npm run wasm` (wasm-pack package into `apps/testbed-web/src/wasm/pkg`).

## Known gaps
- Single hard-coded car; JSON instead of packed arrays; no replay. Prompt 04 replaces the API.

## Tests

```sh
cargo test -p wp_wasm
```
