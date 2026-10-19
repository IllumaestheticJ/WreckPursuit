# Status

_Last updated: lead integration run 1, 2026-10-19. Contract v0.1.0 (v0.2.0 planned, see
`docs/CONTRACT_CHANGELOG.md`)._

**Integration branch:** `claude/laughing-newton-0xqr6q` holds this run (5 workstreams merged, all
checks green). It lands on `main` once the owner OKs pushing to `main` (or merges the PR).

## Workstreams

| # | Workstream | Branch | Merged at | What works end-to-end now | Blocked on / next |
|---|---|---|---|---|---|
| 02 | Vehicle | `ws/vehicle` | `83eb297` | Real tire/suspension/drivetrain model drives in browser and simcli; debug cars inside their 0-100 / braking / skidpad bands | tire compounds for 08/09; v0.2.0 `GroundQuery` |
| 03 | Damage | `ws/damage` | — not landed | (stub: no deformation) | **critical path for Checkpoint 1** |
| 04 | Testbed | `ws/testbed` | `c0f78d9` | Debug layers, 5 cameras, telemetry, replay record/reload/scrub, scenario menu, native stub | adopt scenario v2 natively, call `add_static_box`, write simcli replay format |
| 05 | World | `ws/world` | — not landed | (stub: flat world, clock) | — |
| 06 | Police | `ws/police` | — not landed | (stub) | — |
| 07 | Net | `ws/net` | — not landed | (stub echo server) | — |
| 08 | Economy | `ws/economy` | `f75248f` | Ledger, garage, 16 mod categories, repair quotes, payouts, capture/impound; SQLite store (server feature) | call sites need 06/07; real estimates need 03 |
| 09 | Cars | `ws/cars` | `7dbf8f3` | 13 roster cars load, validate, run deterministically on the real model; nameplate check clean | switch to new tire files; fill VALIDATION sim columns |
| 10 | Harness | `ws/harness` | `297b6b8` | simcli v2, 17 scenarios, golden hashes + replays, determinism lint, native-vs-wasm compare, benches + perf gate, CI workflows | scenario `barriers` + start speed; CI lines for 08/09 |
| 12 | Lead | this branch | `ca11446`, `9b14716`, `cef7f7f` | Rapier contacts → `ContactImpulse` → damage + `SimEvent::Contact`; static boxes; `place_vehicle`; `set_mode_table` | v0.2.0 bump after 03 + 06 |

Prompts 13-20 not started.

## What you can play right now
`just web` → http://localhost:5173: drive the FWD debug sedan (or pick any of the 15 cars) with
real handling; cars collide with each other. No visible damage yet (needs 03).

## Checks on the integrated build (2026-10-19, cloud container)
- `cargo fmt --check`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test --workspace`: green.
- `cargo build --target wasm32-unknown-unknown -p wp_wasm`: green; no native-only deps reach it
  (`rusqlite` only behind `wp_economy/native`).
- Determinism lint: 0 errors. `tools/wasm-determinism/compare.sh`: native == wasm on all scenarios.
- `just web-e2e` (headless Chromium): all checks pass.
- `tools/specsheet/nameplate_check.py`: 0 problems. `smoke_sim.py`: 13 cars × 2 modes deterministic.
- GitHub CI workflows from 10 have not run on `main` yet (nothing pushed to `main`).

## Perf (native, release, cloud Xeon 2.1 GHz; full log in `docs/PERF.md`)
| What | Measured | Budget |
|---|---|---|
| vehicle + Rapier + contacts, 24 cars (`sim_only/24`) | 0.029 ms/car/frame | 0.40 ms/car (vehicle 0.15 + lattice 0.25) |
| vehicle model only (02's bench) | 0.016 ms/car/frame | 0.15 ms/car |
| lattice | not measured (stub) | 0.25 ms/car |
| browser, 24 cars (04, **stub physics**) | 0.054 ms/frame sim | 2× native — re-measure |

## Still stubbed
`wp_damage` (03), `wp_world` traffic/city (05), `wp_police` (06), `wp_net` beyond echo (07).

## Milestone checklist
- [ ] **Checkpoint 1 (Debug)**
  - [x] FWD + RWD debug cars drivable in the browser (real model, e2e-tested)
  - [ ] deforming lattice — needs 03
  - [ ] barrier tests pass in Arcade and Simulation — contacts + static boxes ready in `wp_sim`;
        needs 03 (damage) and 10 (`barriers` in scenarios)
  - [x] replay round-trips (browser: 26/26 hashes after reload; simcli golden replays)
  - [~] determinism suite green — green locally incl. native vs wasm; not yet run in GitHub CI
- [ ] Checkpoint 2 (Greybox): procedural city + traffic obeying signals; police patrol/pursue with
      day/night tables; heat/escape/capture loop; repair billing; 4-player server test.
- [ ] Checkpoint 3 (Low-poly playable): 13 cars validated, mod tree via garage, captain seat,
      32-bot soak green, style B, external playtest build.

## Decisions / actions needed from the owner
1. OK to push merges to `main` (lead sessions are told to push only to their session branch).
2. Enable branch protection on `main`; delete stale `claude/youthful-mayer-73fdu7`.
3. Resume prompts with Lead feedback waiting: 02, 04, 09, 10 (08 optional).
