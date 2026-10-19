# wp_wasm

**Owned by workstream 04 (browser testbed).**

wasm-bindgen surface of the simulation for `apps/testbed-web`. A thin `lib.rs` over
`session::Session` (plain Rust, unit-tested natively), which drives one `wp_sim::Sim` tick by
tick and logs every mutation for deterministic replay.

| Module | What |
|---|---|
| `lib.rs` | exported functions (full ABI table: `docs/integration/testbed.md`) |
| `session.rs` | `Session`: fixed-step loop, op log, scenario scripts/metrics, replay playback + drift check |
| `replay.rs` | replay format (`WPR\x01` + postcard): header, tick-stamped ops, hash every 60 ticks |
| `scenario.rs` | simcli-compatible `.ron` scenarios + testbed extensions, expectation checks |
| `pack.rs` | packed typed-array layouts, tire-force estimate, beam strain, region energy |
| `data.rs` + `build.rs` | every file in `data/{cars,lattices,tires,materials,modes}` embedded at build time |

Everything reaches vehicle/damage through `wp_sim` and the `wp_contracts` traits, so real
`wp_vehicle` / `wp_damage` drop in without changes here.

## Build / test

```sh
cargo test -p wp_wasm                                   # native unit tests (replay round trip, scenarios, layouts)
cargo build --target wasm32-unknown-unknown -p wp_wasm  # wasm check
cd apps/testbed-web && npm run wasm                     # wasm-pack release package into src/wasm/pkg
```

Dependencies are wasm32-clean: `postcard` (no_std + alloc), `ron`, `serde_json`, `wasm-bindgen`.

## Perf
Testbed overhead with stub physics: 0.015 ms (1 car) … 0.054 ms (24 cars) per 60 Hz frame in
headless Chromium; see `docs/integration/testbed.md`.

## Known gaps
- Tire forces / region energy / strain are proxies until contract requests TB-1/TB-2.
- `set_damage_mode` restarts the session (TB-4); `intents()` is an empty placeholder (TB-5).
- `Sim::state_hash` ignores lattice state, so replay drift in damage is invisible (TB-6).
