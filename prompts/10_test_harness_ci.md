# PROMPT 10 — Headless simulation harness, determinism & regression testing, benchmarks, CI (`apps/simcli`, `wp_sim` test surface, `.github/`)

Read `CLAUDE.md`, `docs/INTEGRATION.md`, `docs/CONTRACT_CHANGELOG.md`. You own `apps/simcli`, `tools/bench/`, `.github/workflows/`, and the **test surface** of `wp_sim` (not its game logic — add hooks, don't implement systems). You may read everything. Branch `ws/harness`.

## Goal
Make it cheap for every other workstream to prove their work: a scenario runner that composes whatever crates exist, deterministic hashing, golden-replay regression, performance gates, and CI that blocks contract drift. This is the glue that makes parallel development converge.

## Work
**Scenario format** (`.ron`, documented in `apps/simcli/SCENARIOS.md`): seed, city (procedural params or file), time of day + ratio, damage mode, assist profile, entities to spawn (cars with ids/poses, traffic density override, police presence override), scripted inputs per entity (timeline of input keyframes; also `follow_route(route_id, target_speed)` and `ai_driver(target)` once available), duration, sample rate for metrics, `expect:` block with ranges (speed at t, max |lateral g|, crush depth, components failed, pursuit started before t, etc.), and `hash_every: ticks`.

**Runner**: loads via `wp_contracts` loaders, builds `wp_sim::Sim` with feature-gated systems (`--systems vehicle,damage,world,police,net` so a crate that's still a stub can be excluded), runs headless at max speed, writes `out/<scenario>/{metrics.csv, events.jsonl, hashes.txt, summary.json, replay.bin}`; exits non-zero when expectations fail. `--compare golden/<scenario>/hashes.txt` for regression; `--bless` to regenerate goldens (requires a commit message tag `[bless]` in CI).

**Determinism suite**: run each scenario twice in-process and once in a fresh process; hashes must match. Cross-target: build the WASM and run the same scenario in Node with the same inputs → compare hashes with native (this catches f32/transcendental drift early — if it fails, document which functions and route through `wp_math` lookup/software implementations). Add a `wp_math` audit test that flags use of `f64` or `std::f32::consts` fast paths in sim crates (simple grep-based lint in `tools/lint-determinism`).

**Golden replays**: a small library of replays (`golden/*.replay.bin`) recorded in the testbed for "feel" cases (FWD lift-off, RWD power slide, 56 km/h frontal, T-bone, spike strip, night pursuit escape). CI replays them and compares hashes; a physics change that alters them fails until blessed, with a diff report (first divergent tick, which entity, magnitude).

**Benchmarks** (`tools/bench`, criterion): 1/8/24/64 vehicles sim-only, 24 vehicles + 400 traffic, lattice solver alone, snapshot encode/decode; results appended to `docs/PERF.md` with machine fingerprint; CI fails on > 20% regression against the last blessed numbers (compare within the same runner class only).

**CI** (`.github/workflows/`): `ci.yml` (fmt, clippy -D warnings, test, wasm build, determinism suite, golden replays, schema validation of all `data/**`), `nightly.yml` (benchmarks, 10-minute soak of the procedural city with 32 bots via `apps/server --bots` once prompt 07 lands), `contract-guard.yml` (fails any PR that changes `wp_contracts` or `schemas/` without a `docs/CONTRACT_CHANGELOG.md` entry and version bump).

**Fuzz/soak**: `simcli --fuzz --minutes 5` — random inputs, random collisions, asserts no NaN, no node escaping chassis bounds, no vehicle below ground, no event floods (> 1000 events/tick).

## Deliver
Everything above, `apps/simcli/README.md`, `docs/TESTING.md` (how each workstream should add scenarios/expectations, how to bless), `docs/integration/harness.md` (hooks added to `wp_sim`: system registry, event drain, hash function spec — the hash must be stable: hash quantised state in a fixed entity order).
