# WreckPursuit — Integration Guide

Read this and `docs/CONTRACT_CHANGELOG.md` at the start of every session (see `CLAUDE.md`).
Contract version: **0.3.0**.

## Crate map & ownership

| Path | What | Owner (prompt) | Branch |
|---|---|---|---|
| `crates/wp_contracts` | Shared types, schemas, events, traits, constants, loaders | Integration lead (01 → 12) | `main` |
| `crates/wp_math` | Fixed step, seeded RNG, stable hash, deterministic trig, interpolation | Integration lead | `main` |
| `crates/wp_vehicle` | Vehicle dynamics (`VehicleSim`) | 02 vehicle | `ws/vehicle` |
| `crates/wp_damage` | Lattice damage (`DamageSim`), `gen-box-lattice` | 03 damage | `ws/damage` |
| `crates/wp_wasm`, `apps/testbed-web` | Browser surface + Three.js testbed | 04 testbed | `ws/testbed` |
| `crates/wp_world`, `data/city` | Roads, signals, traffic, clock (`WorldQuery`) | 05 world | `ws/world` |
| `crates/wp_police`, `data/police` | Heat, dispatch, pursuit, captain | 06 police | `ws/police` |
| `crates/wp_net`, `apps/server` | Netcode + server | 07 net | `ws/net` |
| `crates/wp_economy`, `data/mods`, `data/economy` | Money, repair, impound, mods | 08 economy | `ws/economy` |
| `data/cars`, `data/lattices` (car files) | Car roster | 09 cars | `ws/cars` |
| `apps/simcli`, `.github/workflows` | Scenario runner, determinism, CI | 10 harness | `ws/harness` |
| `crates/wp_sim` | Game loop composition | Integration lead (12) | `main` |
| `schemas/` | Generated JSON Schemas (never hand-edit) | Integration lead | `main` |
| `data/tires` | 02 · `data/materials`, `data/modes` → 03 | | |

Dependency graph (arrows = "depends on"):

```
wp_contracts ◄── every crate
wp_math ◄── wp_vehicle, wp_damage, wp_world, wp_police, wp_sim
wp_sim ──► wp_vehicle, wp_damage, wp_world, wp_police, wp_economy, rapier3d
wp_wasm ──► wp_sim, simcli (lib: scenario format v2)   (must stay wasm32-clean)
apps/simcli ──► wp_sim, wp_net, wp_economy (entity `mods`: apply_mods, wasm-clean)
wp_economy ──► wp_police (WantedLevel, until v0.2.0); rusqlite only behind feature `native` (server)
apps/server ──► wp_net[native] (tokio, tungstenite — never reachable from wp_wasm)
```

## Data flow (one physics tick, `PHYSICS_DT = 1/240 s`)

```
inputs (VehicleInput per entity, from keyboard / net / AI)
   │
   ▼
wp_world.step(dt)            clock, signals, traffic            ──► SimEvent::TrafficViolation
   │  WorldQuery (surface, speed limit, signal ahead, time of day)
   ▼
VehicleSim.step(input, assists, world, dt)   × each vehicle (2 substeps inside)
   │
   ▼
Rapier step (gravity -Z)  ──► contact manifolds ──► ContactImpulse (chassis space)
   │                                                     │
   │                                                     ▼
   │                         DamageSim.apply_contacts(contacts, ModeTable, dt)
   │                              ──► PartDetached / ComponentDamaged / WheelDetached / TotalLoss
   │                                                     │
   ◄──────────── VehicleSim.apply_damage_effects(HandlingEffects) ◄──┘
   ▼
wp_police.step(world, racers, events, dt)   ──► SimEvent::Police(PoliceEvent)
   ▼
events ──► wp_economy (billing, payouts) · wp_net (snapshots @ 60 Hz) · renderer (via wp_wasm)
```

`HandlingEffects` is the *entire* damage → handling interface. Vehicle state flows out as
`VehicleState`; the renderer additionally reads lattice nodes via `DamageSim::node_positions`.

## Conventions recap
- SI units; +Z up, +Y forward, +X right; quaternions `[x, y, z, w]`; wheel arrays FL, FR, RL, RR.
- No wall-clock time, `HashMap` iteration, unseeded randomness or `f32::sin/cos` in sim code
  (use `wp_math::sin_cos`). Determinism is checked by hashing (`Sim::state_hash`).
- Every file in `data/` has `id` == file stem and must validate against `schemas/`.
- **One scenario format:** simcli v2 (`apps/simcli/SCENARIOS.md`), owned by 10. Other workstreams
  ask 10 for new fields instead of inventing a dialect. Replays: simcli `replay.bin` v1.
- Goldens and perf baselines only change in a commit tagged `[bless]` (CI guard).

## How to run everything

| Task | Command |
|---|---|
| All tests (incl. data/schema validation, scenario determinism) | `just test` / `cargo test --workspace` |
| CI-equivalent check | `just check` |
| WASM build check | `cargo build --target wasm32-unknown-unknown -p wp_wasm` |
| Browser testbed | `just web` → http://localhost:5173 |
| Headless server | `just server 8080` |
| Scenario | `just sim apps/simcli/scenarios/smoke/straight_line.ron` |
| All scenarios / goldens / re-bless | `just scenarios` · `just goldens` · `just bless` (commit with `[bless]`) |
| Determinism (in-process, fresh process, native vs wasm) | `just determinism` · `just lint-det` |
| Browser end-to-end (headless Chromium) | `just web-e2e` |
| Benchmarks + perf gate | `just bench` (see `docs/PERF.md`) |
| Everything CI runs | `just ci` |
| Regenerate schemas (lead) | `just schemas` |
| Regenerate debug lattices | `just lattices` |

Tooling: Rust 1.97.0 (pinned in `rust-toolchain.toml`, adds `wasm32-unknown-unknown`),
`just`, `wasm-pack`, Node 22.

## Contract change process
1. Write the request in `docs/integration/<you>.md` → "Contract change requests" (what, why, who
   is affected). Do not edit `wp_contracts`/`schemas/` on your branch.
2. The lead applies it on `main`, bumps `CONTRACT_VERSION`, regenerates schemas, updates example
   data and logs it in `docs/CONTRACT_CHANGELOG.md`.
3. Rebase/merge `main` into your branch.

## Workstream docs
Each workstream keeps `docs/integration/<name>.md` from `docs/integration/_TEMPLATE.md`.
