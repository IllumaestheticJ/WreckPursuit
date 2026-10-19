# CLAUDE.md — WreckPursuit repo conventions (applies to every Claude Code session)

## What this project is
A physics-driven street-racing game with Wreckfest-style sim-cade handling, semi-soft-body damage (rigid chassis + deformable lattice), a living city with traffic and police, and asymmetric multiplayer (racers vs. player/AI police). Phase 1 target: browser-testable prototype (WASM) + native headless server. Physics and damage are the highest priorities.

## Architecture (do not deviate without updating docs/CONTRACT_CHANGELOG.md)
- Rust workspace at repo root. Core logic has **no renderer dependency** and must compile for `x86_64-unknown-linux-gnu` and `wasm32-unknown-unknown`.
- Crates:
  - `wp_contracts` — shared types, schemas, events, constants. **Owned by the integration lead. Other sessions may read it, never edit it; propose changes in `docs/integration/<your-workstream>.md` under "Contract change requests".**
  - `wp_math` — fixed-step helpers, deterministic RNG (`wp_math::Rng`, xoshiro256**, seeded), interpolation.
  - `wp_vehicle` — chassis, suspension, tires, drivetrain, input assists, FFB output.
  - `wp_damage` — lattice deformation, materials, mechanical damage, part detachment, mode tables, repair cost.
  - `wp_world` — road network, signals, traffic AI, civilian density, day/night clock.
  - `wp_police` — heat/bounty, dispatch, patrol/pursuit AI, captain tools.
  - `wp_economy` — money/rep, repair billing, impound, mod tree, garage state.
  - `wp_net` — server + client prediction, snapshots, roles, interest management.
  - `wp_sim` — the game loop that composes the above (owned by lead; stub exists after prompt 01).
  - `wp_wasm` — wasm-bindgen surface for the browser testbed.
  - `apps/testbed-web` — TypeScript + Three.js + Vite.
  - `apps/server` — headless authoritative server binary.
  - `apps/simcli` — headless scenario runner for tests/benchmarks.
  - `data/` — `cars/*.json`, `lattices/*.json`, `tires/*.json`, `materials/*.json`, `modes/{arcade,sim}.toml`, `mods/*.json`, `city/*`.
- Simulation: fixed timestep `wp_contracts::PHYSICS_DT = 1/240 s`. Vehicle + lattice sub-step inside that; netcode snapshots at 60 Hz; rendering interpolates. **No wall-clock time inside simulation code.** All randomness via seeded `wp_math::Rng` passed in explicitly.
- Units: SI everywhere (m, kg, s, N, rad). +Z up, +Y forward in vehicle space, right-handed. Rapier world uses the same convention (configure gravity on -Z).
- Determinism rules: no `HashMap` iteration order in sim logic (use `BTreeMap`/`IndexMap`), no `f32::sin` fast-math variants, no threads inside a sim step unless results are reduced in a fixed order, no platform-dependent `f64`→`f32` differences in hot paths (keep f32).

## Working rules for each session
1. Read `docs/INTEGRATION.md` and `docs/CONTRACT_CHANGELOG.md` at session start.
2. Work only in your assigned crate(s) and `data/` subfolder, on branch `ws/<name>`. If you must touch another crate to compile, make the minimal change and list it in your integration doc.
3. Every public type/function gets a doc comment. Every crate gets a `README.md` (what it does, how to run its tests, known gaps).
4. Tests: unit tests in-crate; scenario tests go in `apps/simcli/scenarios/*.ron` with expected ranges, not exact numbers (except determinism tests, which compare hashes).
5. Commit often, small messages prefixed with the crate: `wp_damage: plastic beam yield + tests`.
6. Before ending a session: `cargo fmt`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`, `cargo build --target wasm32-unknown-unknown -p wp_wasm` (if your crate is reachable from it), update `docs/integration/<name>.md`, push.
7. Do not add dependencies with native-only build steps to any crate reachable from `wp_wasm`. Check with `cargo tree` if unsure.
8. Performance budgets (per frame @60 Hz, native, release): vehicle sim ≤ 0.15 ms/car, lattice ≤ 0.25 ms/car (24 cars), traffic ≤ 2 ms for 400 agents, police ≤ 0.5 ms. Browser targets are 2× those. Record measurements in your README.
9. No licensed car names, badges, or trademarked model names anywhere in code, data, or comments. Analog names only (see `data/cars/README.md`).
10. Prefer boring, legible code over clever code. This is a long-lived project with many parallel contributors.

## Mode semantics (shared vocabulary)
- `DamageMode::Arcade` — punishing but survivable; one hard hit degrades, several end the car.
- `DamageMode::Simulation` — real-world-ish; one hard hit at speed is a total loss.
- Both come from `data/modes/*.toml` tables consumed by `wp_damage::ModeTable`. Same code path.
- `AssistProfile` (ABS/TCS/ESC/steer-help/throttle-filter) is independent of damage mode and lives in `wp_vehicle`.
