# Status

_Last updated: prompt 01 (architecture & contracts), contract v0.1.0._

## What works end-to-end
- Cargo workspace (Rust 1.97.0 pinned) with all crates and apps; `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -D warnings` and
  `cargo build --target wasm32-unknown-unknown -p wp_wasm` are green.
- `wp_contracts` fully specified; JSON Schemas generated into `schemas/`; every file under
  `data/` validates against the schemas and the typed loaders (test-enforced, including a
  stale-schema check).
- Example data: `debug_sedan` (FWD, 1400 kg, 150 kW), `debug_coupe` (RWD, 1500 kg, 300 kW),
  `street` tire, 5 materials, 2 generated 120-node box lattices, Arcade/Sim mode tables.
- `just web`: browser testbed shows the debug sedan as a box that drives with arrows/WASD
  (kinematic stub), chase camera, HUD. Verified in headless Chromium.
- `just sim <scenario>`: headless runs print a state hash; scenarios are deterministic
  (test-enforced).
- `just server`: websocket server streams a 60 Hz tick counter and echoes text (test-enforced).

## What is stubbed
| Crate / app | Stub behaviour | Real work in |
|---|---|---|
| `wp_vehicle` | Kinematic bicycle model, no tires/suspension/weight transfer, unrealistically quick acceleration | 02 |
| `wp_damage` | No deformation; neutral `HandlingEffects`; empty repair estimate | 03 |
| `wp_wasm` / testbed | JSON API, one hard-coded car, boxes only | 04 |
| `wp_world` | Flat infinite asphalt, no limits/signals/traffic (clock works) | 05 |
| `wp_police` | Does nothing | 06 |
| `wp_net` / server | JSON codec, tick-echo websocket | 07 |
| `wp_economy` | Account struct + in-memory store | 08 |
| cars | Two debug cars only | 09 |
| `simcli` / CI | Single-car scenarios, no golden hashes, basic CI | 10 |
| `wp_sim` | Rapier world has only the ground; vehicles are not bodies yet → no contacts reach damage | 12 after 02/03 |
| `data/modes/*.toml` | Placeholder numbers (structure final) | 03 |

## Perf (stub, for reference only)
`simcli` smoke_straight_line: 3360 ticks with 1 stub car in ≈ 5 ms total wall time (release,
cloud container) → ≈ 1.5 µs/tick. Meaningless until 02/03 land.

## Milestone checklist
- [ ] Checkpoint 1 (Debug): drivable FWD + RWD debug cars in browser with deforming lattice,
      barrier tests pass in both modes, replay works, determinism suite green.
- [ ] Checkpoint 2 (Greybox): procedural city + traffic obeying signals; police patrol/pursue with
      day/night tables; heat/escape/capture loop; repair billing; 4-player server test.
- [ ] Checkpoint 3 (Low-poly playable): 13 cars validated, mod tree via garage, captain seat,
      32-bot soak green, style B, external playtest build.
