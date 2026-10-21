# wp_wasm

**Owned by workstream 04 (browser testbed).**

wasm-bindgen surface of the simulation for `apps/testbed-web`. A thin `lib.rs` over
`session::Session` (plain Rust, unit-tested natively). A session is a **simcli scenario (format
v2)**: the sim is built by `simcli::run::build_sim`, scripted entities use
`simcli::run::driver_input`, scenario results come from `simcli::run`, and recordings are
canonical `replay.bin` files that `simcli replay` verifies.

| Module | What |
|---|---|
| `lib.rs` | exported functions (full ABI table: `docs/integration/testbed.md`) |
| `session.rs` | `Session`: fixed-step loop, live / scripted / playback, op + input log, per-tick hashes, drift check |
| `scenario.rs` | free-drive scenario generator, `.ron` parsing, cruise-speed override (all simcli v2) |
| `pack.rs` | packed typed-array layouts, beam strain, region energy |
| `street.rs` | NPC street racers (lead): roaming `wp_opponent` drivers in 5 levels, horn challenge, A→B race to a player-picked finish, wagers through the run's ledger; tests in `street_tests.rs` |
| `run.rs` | the player's economy (lead): drained events → G's `EconomySession`, end-of-run `RunReport`, "Repair & continue"; tests in `run_tests.rs` |
| `police.rs` / `garage.rs` / `audio.rs` | police read-outs + siren rule, live repair bill, audio director glue |
| `data.rs` + `build.rs` | every file in `data/{cars,lattices,tires,materials,modes}` embedded at build time, also as a simcli `MemData` bundle |

## Build / test

```sh
cargo test -p wp_wasm                                   # native unit tests (replay round trip, scenarios, layouts)
cargo build --target wasm32-unknown-unknown -p wp_wasm  # wasm check
cd apps/testbed-web && npm run wasm                     # wasm-pack release package into src/wasm/pkg
```

Dependencies are wasm32-clean: `simcli` (lib), `wp_world`, `postcard` (no_std + alloc), `serde_json`, `wasm-bindgen`.

## Perf
Real vehicle + damage, headless Chromium (SwiftShader), release wasm: 0.09 / 0.62 / 3.0 ms of
sim per 60 Hz frame at 1 / 8 / 24 cars; 0.27 ms in the 12×12 city with ~190 traffic agents. Street racers
(7 roaming AI cars, `street.rs`) add ~1.8 ms per 60 Hz frame natively in Port Vesper (AI 0.16 ms,
the rest is their physics). See `docs/integration/testbed.md`.

## Known gaps
- The end-of-run economy (`run.rs`) keeps the account in memory only (lost on reload); the lattice blob in `DamageReport` is empty (the live sim keeps the dents).
- Region energy / strain are proxies until contract request TB-2 lands; `intents()` is empty (TB-5).
- `simcli replay` cannot verify city recordings until the CLI turns on `RunOptions::load_city`.
- Street racers: accepting / declining a challenge is not in the replay log (a recording drifts from there); wagers are `Adjustment` ledger lines (see `docs/integration/testbed.md`, SR-1…3).
- `cargo test -p wp_wasm` takes ~2 min in debug (real physics).
