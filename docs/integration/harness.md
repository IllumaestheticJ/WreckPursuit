# Harness integration notes

- **Workstream / prompt:** 10 — headless harness, determinism & regression testing, benchmarks, CI
- **Branch:** `ws/harness`
- **Crates / folders owned:** `apps/simcli`, `tools/bench`, `tools/lint-determinism`,
  `tools/wasm-determinism`, `tools/ci`, `.github/workflows/`, the test surface of `wp_sim`
- **Contract version built against:** 0.3.0

## Exposes

### Hooks added to `wp_sim` (test surface only — no game logic)

| item | purpose |
|---|---|
| `wp_sim::Systems { vehicle, damage, world, police }`, `SystemId`, `Systems::parse("vehicle,world")`, `Display` | **system registry**: which subsystems `step_fixed` runs |
| `Sim::set_systems(Systems)` / `Sim::systems()` | default `Systems::ALL` (games and the testbed are unaffected). A disabled system's state is frozen: no vehicle step, no damage contacts/effects, frozen clock, no police step |
| `Sim::last_tick_event_count()` | events emitted by the last tick, without draining (flood detection) |
| `Sim::drain_events()` (existing) | **event drain**: the harness drains after every tick, so events are tick-tagged |
| `Sim::node_positions(EntityId)` | lattice nodes in chassis space (crush depth, fuzz bounds) |
| `Sim::entity_hashes() -> Vec<(EntityId, u64)>` | per-entity hash, ascending entity order (names the entity that diverged) |
| `Sim::state_hash()` (changed) + `wp_sim::HASH_SPEC_VERSION = 1` | hash spec below |

### Hash function spec (v4)

All hashing is FNV-1a 64 (`wp_math::StateHasher`, little-endian integer encoding). Floats are
**quantised** before hashing: `round(v as f64 × scale) as i64` (`wp_math::quantize`; NaN →
`i64::MIN`), so the hash compares behaviour, not bit noise, and f64 multiply/round is exact and
identical on every target.

```
state_hash = FNV( u32 HASH_SPEC_VERSION, u64 tick,
                  q(clock seconds_since_midnight, 1e3),
                  u64 World::state_hash()        // v2: signals + traffic (wp_world's own spec)
                  u64 PoliceSystem::state_hash() // v3: units, heat, pursuits (wp_police's own spec)
                  u32 debris_count, q(debris body translation, 1e-3 m) in spawn order,   // v2
                  u32 barrier_count, for each roadblock barrier body in sync order:     // v4
                      u32 tool id, q(body translation, 1e-3 m)
                  u32 vehicle_count,
                  u64 entity_hash(v) for v in vehicles ascending by EntityId )

entity_hash(v) = FNV( u32 entity id,
                      q(pos[0..3], 1e4)          // 0.1 mm
                      q(rot[0..4], 1e5)
                      q(lin_vel[0..3], 1e4), q(ang_vel[0..3], 1e4)
                      q(engine_rpm, 1), i64 gear, q(health, 1e4),   // health from damage
                      for wheel in FL, FR, RL, RR:
                          q(omega, 1e3), q(steer_rad, 1e5), q(compression_m, 1e5), u8 detached
                      u32 node_count, q(node.xyz, 1e4) for every damage node in id order )
```

v1 → v2 (run 3): added the world/traffic hash and debris bodies. v2 → v3 (run 5): added the
police hash, because police units are not vehicles and would otherwise drift unnoticed. v3 → v4
(run 9): added roadblock barrier bodies, which cars can now shove. Every golden was re-blessed
each time.
City props (parked cars, cones) are physics bodies but are not hashed yet (the sim keeps only
their count); a prop knocked differently shows up through the vehicles it touches.

Stability rules: fixed entity order (ascending id, ids never reused), fixed field order, no
maps iterated. Any change to the hashed fields bumps `HASH_SPEC_VERSION` and requires
re-blessing every golden; the replay verifier reports a hash-spec mismatch explicitly.
Systems should not add fields to the hash themselves — ask here, so the spec stays in one place.

### Files and formats

- Scenario format: `apps/simcli/SCENARIOS.md`. Outputs: `out/<scenario>/{metrics.csv,
  events.jsonl, hashes.txt, summary.json, replay.bin}`.
- `replay.bin` v1 (`apps/simcli/src/replay.rs`): `"WPREPLAY"`, format version, contract version,
  hash spec, systems, scenario RON (setup), tick count, input events
  `(u64 tick, u16 entity index, 5×f32 axes, u8 flags, u8 tool)`, a u64 state hash per tick,
  checkpoints `(u64 tick, per entity: u64 entity hash, pos, lin_vel, health)`.
  **Testbed (04):** please record replays in this format (setup scenario + applied inputs);
  `simcli replay` then verifies them headlessly.
- Golden layout: `apps/simcli/golden/<name>/hashes.txt` (every scenario),
  `apps/simcli/golden/<name>.replay.bin` (`scenarios/golden/*`).
- WASM runner C ABI (`tools/wasm-determinism`): `wp_alloc`, `wp_free`, `wp_run`,
  `wp_result_ptr`, `wp_result_len`. No host imports.
- Perf: `docs/PERF.md` (append-only log), `tools/bench/baselines/<runner class>.json`.

## Needs

| from | what | workaround now |
|---|---|---|
| 05 (world) | routes for `follow_route`; physical (rigid-body) traffic via `LodChange` | `follow_route` warns; traffic stays kinematic in scenarios |
| 05 (world) | population with a player present: the soak dips to ≈ 360 agents (≥ 380 without players) | `world_traffic_soak_observer` floor 350, 380 in `expect_pending` |
| 06 (police) | presence override; police that react in the night-pursuit / spike goldens | presence warns; pursuit/spike targets in `expect_pending` (kinds from `wp_police::events::kind`) |
| 07 (net) | `wp-server --bots N --minutes M` | nightly soak falls back to `simcli fuzz --minutes 10 --max-vehicles 32` (auto-switches when `--bots` appears in `apps/server/src/main.rs`) |
| 04 (testbed) | record replays in `replay.bin` v1 | golden replays recorded from scripted scenarios |
| 03 (damage) | T-bone victim: crush 0.12 m and no component failures at 50 km/h side impact | targets stay in `expect_pending` |
| 07 (net) | F's snapshot codec | net round trip and snapshot benches use `wp_net::{encode,decode}_server` (still JSON) |

## Contract change requests

| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| H1 | `wp_contracts`: `pub const POLICE_PURSUIT_STARTED: u16` (and other `PoliceEvent` kind codes), or a typed police event | `pursuit_started_before` expectation needs a stable kind code | 06, 10 | accepted (typed enum in v0.2.0) |
| H2 | Consider moving `Systems` into `wp_contracts` if `wp_net`/server need to toggle systems too | keep one registry | 07, 12 | rejected for now |

## Changes outside my crates
**By the lead in `apps/simcli` (`lead/demo-endrun`, 2026-10-19):** replay op `OpKind::Repair` (byte tag 3) → `Sim::repair_vehicle` (in-place full repair after a paid bill), plus its case in the encode/decode round-trip test. Old replays decode unchanged; goldens unchanged.


- `crates/wp_sim` (test surface, which this workstream owns per prompt 10): new
  `src/systems.rs`; `Sim` gains `set_systems`, `systems`, `last_tick_event_count`,
  `node_positions`, `entity_hashes`, `HASH_SPEC_VERSION`; `state_hash` now follows spec v1
  (adds clock, wheels, lattice nodes; previous hashes are not comparable). `step_fixed` checks
  the system flags; with `Systems::ALL` (the default) behaviour is unchanged. README updated.
- `Cargo.toml` (workspace members): `tools/wasm-determinism`, `tools/lint-determinism`, `tools/bench`.
- `justfile`: harness recipes appended (`scenarios`, `goldens`, `bless`, `determinism`,
  `lint-det`, `fuzz`, `bench`, `ci`).
- `.github/workflows/ci.yml` replaced (owned), `nightly.yml`, `contract-guard.yml` added.
- `docs/PERF.md`, `docs/TESTING.md` added. `docs/INTEGRATION.md` "How to run" table not edited
  (lead): suggest adding `just ci`, `just goldens`, `just bless`, `just fuzz`, `just bench`.

## Perf numbers

Cloud container, Intel Xeon @ 2.10 GHz, 4 cores, release, real vehicle + damage + world
(run 3, from `docs/PERF.md`):

| Measurement | Native | Browser | Budget |
|---|---|---|---|
| `sim_only/24` (one tick, 24 cars) | 383 µs → 0.064 ms/car/frame | not measured | 0.40 ms/car |
| `sim_only/64` | 2.77 ms/tick → 0.17 ms/car/frame | — | 0.40 ms/car |
| `lattice/solver_8_contacts` | 145 µs → 0.58 ms/car/frame | — | 0.25 ms/car |
| `snapshot/encode_24` / `decode_24` (JSON codec) | 40 µs / 53 µs | — | — |
| `world_traffic_soak` (60 s, 400 agents, 3344 prop bodies) | 3 s | — | — |
| `cargo test -p simcli` (debug, soaks skipped) | 130 s → 6.9 s `tests/scenarios.rs` (round 8: lead's opt-levels + two more `slow`) | — | — |
| native vs wasm compare, all scenarios (incl. builds) | ≈ 2 min | — | — |

## Known gaps

- Physics/damage/police expectations are pending until those systems land (see Needs).
- The perf gate only gates when runner class and CPU model match the baseline; GitHub's shared
  runners change CPUs, so expect some nights to report "not gating". A self-hosted runner class
  would make the gate strict.
- No `tools/bench` numbers in the browser yet (needs a wasm bench harness; the
  `wasm-determinism` module could host one).
- The lint is line-based: it can miss a transcendental called through a function pointer or a
  macro, and it stops at the first `#[cfg(test)]` in a file.

## Run 9b — lead/round12 merged (2026-10-20, lead helper)

Merged `origin/lead/round12` into `ws/harness` (merge commit). Conflict choices:
- `Sim::state_hash` keeps v4 (roadblock barrier bodies) **and** round12's weather hash
  (barriers first, then weather, then debris).
- `wp_wasm::Session::run_tick`: the crash-replay snapshot at tick 0 is taken first, then the
  scenario's police orders are applied (crash replays rerun `run_tick`, so orders reapply).
- simcli: `police_orders` and `weather` scenario fields both kept; summary metrics
  `max_spike_puncture`, `spiked_wheels`, `max_los_lost_s` (23 total); world metrics 17.
- Testbed: round12 already draws spike strips (`render/police_tools.ts`, from
  `map_dynamic()`), so `Scene.updatePoliceTools` now draws only the physical roadblock
  barriers. Flat tyres scale the wheel `spin` group, so the panel-mesh wheels deflate too.
- Goldens re-blessed `[bless]`. Before blessing, hashing as v3 (no barrier section) matched
  every round12 golden bit-for-bit: the merge changes no round12 behaviour.
  `golden_roadblock` / `golden_spike_strip` changed with round12's police and still pass.

Gate: fmt ✓, `clippy --workspace -D warnings` ✓, wasm32 build ✓, lint-determinism 0 errors,
`tsc` ✓, `npm run e2e:police` 7/7 (police are sim vehicles now; barriers still get shoved,
tyres still go flat). `cargo test --workspace`: 1115 passed, 5 failed, all also failing on
clean `origin/lead/round12` (not caused by this merge):
`wp_opponent race::wanted_ai_racer_flees_police_through_the_city`,
`wp_race race_picker::grid_highway_sprint` (never finishes),
`wp_wasm race_tests::short_sprint_with_ai_…` (race not over in 120 s), and simcli
`all_scenarios_pass…` / `goldens_match` through `city/siren_traffic_no_siren`
(`civilian_collisions` 1, want 0). `clippy --all-targets` also flags
`wp_audio/src/siren.rs:437` (`inconsistent_digit_grouping`, test code from round12).

## Run 9 — roadblocks and spike strips you can see and hit (2026-10-20)

Request: "police barricades you can actually see and hit, and spikes that burst your tyres".
wp_police already planned roadblocks and spike strips and punctured tyres; the barriers had no
body (cars drove through them) and the testbed drew neither tool.

1. **Solid barriers** (`crates/wp_sim/src/police_tools.rs`, new). Each barrier of an active
   roadblock is a dynamic Rapier box (size and 400 kg mass from `data/police/tools.toml`,
   friction 0.7, restitution 0.05) yawed across the road. It is synced every tick after
   police units and removed when the roadblock expires. Contacts reach damage like debris.
   `Sim::roadblock_barriers()` / `spike_strips()` / `spike_puncture(id)` are the read side.
   Barrier translations are in the state hash (**spec v4**). Every golden was re-blessed in
   `7c22ed8 [bless]`; nothing but the version word changed in goldens without police tools.
2. **Scenario `police_orders`**: `roadblock` / `spikes` captain orders (checked by wp_police
   rules) and `roadblock_at` / `spikes_at` direct placement (flat world, no pursuit needed)
   via new `PoliceSystem::place_roadblock_at` / `place_spikes_at`. Results go to
   `summary.json` → `police_orders`. `simcli::run::apply_police_orders` is shared with the
   WASM runner, so browser runs of these scenarios match simcli.
3. **Metrics**: `max_spike_puncture` and `spiked_wheels` (per entity), plus
   `roadblock_barriers_max`, `spike_strips_max` and `barrier_displaced_max_m` (world; these
   work on the flat world too).
4. **Goldens**:
   - `golden_roadblock`: a sedan at 56 km/h hits a 3-barrier roadblock. It shoves a barrier
     about 7.6 m, gets about 0.3 m of front crush and loses health.
   - `golden_spike_strip`: a sedan at 90 km/h crosses a strip and all four tyres deflate.
     A hard brake at 3 s leaves it at about 8.2 m/s at 5 s, against 6.0 m/s for an unspiked
     control.
5. **Testbed**:
   - `police_tools()` returns 11 floats per tool (`[kind, x, y, z, qx, qy, qz, qw, hx, hy, hz]`).
   - `tire_punctures()` returns 4 floats per vehicle: the max of spike and crash puncture.
   - Barriers are drawn as white boxes with red bands and a flashing amber lamp. Spike strips
     are a dark strip with two rows of steel studs.
   - Punctured tyres are drawn squashed, at radius × (1 − 0.3p), which is the sim's
     effective rolling radius.
   - `npm run e2e:police` runs both goldens in the browser and checks:
     - barriers are drawn;
     - a barrier was shoved more than 0.3 m;
     - the strip is drawn;
     - all four tyres read above 0.5 deflated;
     - both runs pass and match simcli;
     - there are no page errors.
     It passes. Screenshots are written to `$SHOTS`.

Changes outside my crates (minimal, listed for the owners):
- **wp_sim (lead):** the `police_tools` module, the `tool_bodies` field, the
  `sync_police_tools()` call in the step, and `set_police` clearing the old bodies. Hash spec v4.
- **wp_police (E):** `place_roadblock_at` / `place_spikes_at`. These bypass the dispatch rules
  and are meant for scenarios and tests.
- **wp_wasm / testbed (04):**
  - wasm exports `police_tools`, `tire_punctures` and `police_tool_stride`;
  - `Session` applies `police_orders`;
  - testbed: `scene.ts` tool meshes, `vehicle_view.ts` `setPunctures`, the `main.ts` calls,
    `layout.ts` / `sim.ts` bindings, and `e2e/police_tools.mjs`.

Known gaps:
- Spike strips have no body. The police hash counts strips, and punctures show up in the hash
  through the wheel state of the vehicle.
- Barriers never break apart.
- The AI dispatcher places roadblocks on real roads only in a city. Captain orders on the
  flat world are refused (`NotPursued`) because there are no units to start a pursuit.

Checks: `cargo fmt`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test
--workspace --no-fail-fast` (784 passed, 0 failed, 15 ignored), wasm build (`npm run wasm`),
`npm run typecheck`, `npm run e2e:police` (7/7), `simcli run apps/simcli/scenarios` (all pass),
`simcli goldens` (all match), `simcli determinism apps/simcli/scenarios` (all deterministic),
`tools/wasm-determinism/compare.sh` (native == wasm).

## Run 8 — response to lead feedback round 8 (2026-10-19)

Merged `origin/main` (contract v0.3.0, `lead/test-speed`, round 8 docs) into `ws/harness`; no
conflicts.

1. **`crush_depth` → `DamageModel::structural_crush_m()`** (B's request). The scenario metric
   (`crush_depth_m` in `summary.json` / `metrics.csv`, the `crush_depth` check) now skips glass,
   bumper covers and detached nodes. `simcli::run::crush_depth` (any-node max) stays as a
   helper because the tuning-dash probe (Q) calls it. Measured, structural vs old any-node:
   | Scenario / entity | structural | any-node | range now |
   |---|---|---|---|
   | `golden_tbone_sim` victim / striker | 0.35 / 0.19 | 0.35 / 0.42 | unchanged (0.25–0.8 / 0.1–0.7) |
   | `golden_tbone` victim | 0.18 | 0.18 | unchanged (0.15–0.6) |
   | `golden_frontal_56kmh` car | 0.21 | 0.45 | 0.25–0.8 → **0.15–0.5** |
   | `golden_barrier_56kmh` / `_arcade` | 0.66 / 0.31 | 0.93 / 0.59 | unchanged |
   | `crash_head_on` sedan | 0.28 | 0.53 | 0.3–1.2 → **0.2–0.8** |
   | `crash_barrier_frontal` / `_sim` | 0.32 / 0.69 | 0.58 / 0.88 | **0.2–0.8** / **0.45–1.5** |
   No re-bless: crush is a metric, not part of the state hash; every golden matches. (B's
   "0.58 vs 0.39 m in golden_tbone_sim" no longer reproduces: the victim reads 0.35 both ways
   now; the striker's front is where glass/bumper inflated it.)
2. **Observer soak gate promoted**: `civilian_collisions` max 3 → **0**, removed from
   `expect_pending`. 0 on seed 5 and on seeds 6–9 (measured). **Police stay off** in this soak:
   with `police_cars: true`, seeds 5, 7, 8, 9 give 0 collisions, seed 6 gives **3** (police
   cause them: 0 on seed 6 with police off). For E: flip it to `true` once units avoid traffic
   on seed 6 too (`sed 's/seed: 5,/seed: 6,/'` + `police_cars: true` reproduces it).
3. **Slow city scenarios in release CI**: `opponent/flee_police` (15.6 s per run in the test
   profile) and `race/city_sprint_police` (5.1 s) are now `slow: true`. The test pass ran each
   three times, so they were most of `tests/scenarios.rs` (lead: 117–123 s → now **6.9 s**).
   The release jobs run every scenario regardless (`determinism`, `goldens`, wasm compare), and
   the `goldens` job's "run all scenarios" step is where `slow`/`soak` expectations are gated
   (step renamed to say so).
4. **Harness report** (new `ci.yml` job `harness-report`, `tools/ci/harness_report.py`,
   `just report`): writes to the job summary every `#[ignore]`d Rust test with its reason and
   every release-only scenario (`soak/`, `slow: true`). Never fails (`--strict` exists to fail
   on bare `#[ignore]`, not on in CI). Today: 12 ignored tests, 4 without a reason, 7
   release-only scenarios. `docs/TESTING.md` asks for `#[ignore = "<who> (<round>): why"]`.

Requests (not my crates):
- **J (audio)** `crates/wp_audio/tests/perf.rs::director_frame_cost_24_cars`, **O (meta)**
  `apps/meta/tests/perf.rs::matchmaking_and_results_throughput`, **G (economy)**
  `crates/wp_economy/tests/perf.rs::perf`, **N (opponent)**
  `crates/wp_opponent/tests/race.rs::sweep_open_roads_progress`: bare `#[ignore]`; please give
  each a reason (`#[ignore = "perf: run with --release"]` or similar) so the report shows why.
- **E (police)**: seed 6 observer soak with police on (item 2).

Checks: `cargo fmt`, `cargo clippy --workspace --all-targets -D warnings`, `cargo test
--workspace --no-fail-fast` (717 passed, 0 failed, 15 ignored = the 12 listed + 3 ```` ```ignore ````
doc examples; 6 min wall), wasm build of `wp_wasm`, `simcli run apps/simcli/scenarios` (all pass),
`simcli goldens` (all match), `simcli determinism` (39/39 deterministic, slow + soak included), `tools/wasm-determinism/compare.sh` (native == wasm on all 39 scenarios).

## Run 6 — response to lead feedback round 6 (2026-10-19)

Merged `main` and the lead branch (run 6 + round 6/6b docs) into `ws/harness`. The lead's
round 6 was written against the pre-5b harness, so items 1 and 2 overlapped with run 5b:

1. **`build_sim` police order** — already `set_world` first, then `set_police` (run 5b). Merge
   conflict resolved to the lead's version: `police_cars: false` now means presence 0 with heat
   and wanted levels kept (run 5b used `PoliceSystem::new()`). `smoke/city_opt_out` still passes
   (0 units).
2. **`golden_city_chase` re-aimed (again) and checks back in `expect`.** Run 5b's aim (outer
   northbound lane of x = 55 from y = −100) no longer pursues after the run-6 merges: unit 1 now
   starts near (55, 33), pulls away north and turns west at y ≈ 57 by 5 s, before the racer
   arrives. The racer now starts at (59.75, −20), 16.5 m/s, `police_heat: 15`: pursuit at
   4.8 s, no damage. Robustness sweep with seed 108: every start from y = −40 to y = 25 at
   16 / 16.5 / 18 m/s passes (pursuit 4.0–6.9 s, health ≥ 0.95); y = −60 is too late; 15 m/s
   is under the violation threshold (no `TrafficViolation`). Other seeds spawn patrols
   elsewhere and are not expected to pass (this golden is aimed at seed 108's layout).
   Re-blessed (`[bless]`, only this golden changed; every other golden matched the lead's).
3. **`soak/traffic_soak_observer`** keeps `police_cars: false`. E's obstacle awareness is not on
   `main` yet; flip it back to `true` when it lands.

Not done / blocked:
- **W-7 (weather scenario field, request from K).** The spec in `weather.md` step 1b calls
  `Sim::set_weather`, which does not exist in `wp_sim` yet (needs W-1, after v0.3.0). The
  parser half is small and ready to add with it; `wet_brake_100_0.ron.pending` stays pending.
- Test-build speed is the lead helper's (`lead/test-speed`); nothing changed here for it.
- **For the lead / 08:** `cargo test --workspace` fails one test outside my crates,
  `wp_economy` `tests/pursuit_wiring.rs::real_police_capture_settles_through_the_economy`
  (line 130, "level at capture, not after the reset": the `PursuitEnded` resolution reports
  `WantedLevel::None`). Fails alone too. It comes in with the lead branch (my `crates/` diff
  against it is one doc comment in `wp_sim`); likely the run-6 police changes resetting the
  level before the capture event is read. Not fixed here (not my crate).
- **For the lead / race workstream:** `wp_race` `tests/simcli_races.rs::city_sprint_on_real_roads`
  panics at line 28 (`index out of bounds: the len is 2 but the index is 2`). The tick hook
  enumerates `v.sim.vehicle_states()`, which since lead run 5 also holds the city's police and
  traffic cars, and indexes `sc.entities[i]` with it. Scenario entities get the lowest ids, so
  the one-line fix in the hook is `.iter().take(sc.entities.len())` before `.enumerate()`
  (or filter by the ids, as `simcli::run::scenario_states` does). Not fixed here (not my crate).
  Every other test in the workspace passes (`cargo test --workspace --no-fail-fast`).

Checks: `cargo fmt`, `cargo clippy --workspace -D warnings`, `cargo test --workspace` (two upstream failures, below),
`simcli run apps/simcli/scenarios` (35/35 pass), `simcli goldens` (all match), wasm build of
`wp_wasm`.

## Run 5b — lead run 5 merged (2026-10-19)

- **Reconciled with the lead's police/traffic vehicles.** `Sim::set_world` now builds police
  itself, so simcli calls `set_police(police_for_city(..))` *after* `set_world`. That carries
  `police_presence` and `police_heat`. With neither set it builds exactly what `set_world`
  does (same seed salt, mode and driving), so default goldens match the lead's. My duplicate
  `Sim::police()` was dropped in favour of `police_link.rs`'s. The opt-outs gate run 5's cars:
  `police_cars: false` → `PoliceSystem::new()` and no `set_police_car`; `traffic_cars: false` →
  no `set_traffic_car`.
- **`golden_city_chase` re-aimed.** With real police cars, the racer rear-ended the patrol it
  was aimed at. It now passes in the outer northbound lane at 16.5 m/s, as a known speeder
  (`police_heat: 15`, L1). Pursuit starts at 6.7 s, no damage. All 12 nearby lane/speed/start
  variants pursue in 5–9 s. Without starting heat a clean pass is never recognised at
  night: heat only builds once the patrol has witnessed the speeding.
- **Debug test time.** After run 5, `cargo test -p simcli` passed the 1 h mark: the three
  `opponent/` races (90–330 s in a full city, now with physical police and traffic) take
  190–300+ s each in a debug build, and the suite runs each scenario about 5 times. New
  scenario flag `slow: true`: debug `cargo test` skips it, like `soak/`. I set it on the three
  opponent scenarios (one line each, for workstream 17). The release CI jobs still run them.
- **For the lead (run-5 police driver):** unit 1 (starts at (55, −21) on the x = 55 collector)
  drives a slow circle about 14 m across at 3.8 m/s around (40, −17) for 12+ s instead of
  patrolling (`examples/probe_police.rs` on `golden_city_chase` with the racer moved away shows
  it). The loop cuts into the southbound lanes, which is where my racer kept hitting it. It
  looks like `unit_input` steering never converging on its target.

## Run 5 — city chase, opt-outs, F's codec (2026-10-19)

- **Police in cities.** `wp_sim` gained `Sim::set_police(PoliceSystem)` / `Sim::police()`
  (test-surface hook, like `set_world`). With a city, simcli installs
  `PoliceSystem::for_world(cfg, &world, setup)` with E's kinematic unit driver, seeded from the
  scenario. Until now `wp_sim` always used `PoliceSystem::new()`, which has no road graph and
  so no units. **For the lead:** if run 5 wires police into `Sim::set_world` itself, simcli's
  call becomes redundant and can go.
- **`police_presence` is applied:** it scales `presence.toml`'s patrol counts (district ×
  phase). New per-entity `police_heat`; new world metrics `police_units_max` / `police_units_final`.
- **Opt-outs:** `traffic_cars`, `parked_cars`, `police_cars` (all default `true`).
  `smoke/city_opt_out.ron` checks all three; without them the same city has 198 agents, 1322
  parked cars and 5 units.
- **`golden_city_chase`:** 23:00, 12×12 city, a speeder at 22 m/s (limit 13.4 m/s) comes up
  behind the patrol that starts on the x = 55 collector. Pursuit starts at 5.0 s (expect ≤ 20 s),
  with no starting heat. **For E:** night recognition (40 m for 4 s) means a speeder passing
  a patrol head-on, or on a crossing street, is never recognised. In my probes a unit never
  converged on an unseen L2/L3 racer within 25 s, even at 3× presence. That matches
  "a glimpse at night is not enough", but it makes night chases depend on where the racer
  meets a patrol, so a scenario has to be aimed (`examples/probe_police.rs`).
- **F's codec.** The simcli net round trip and the `snapshot/*` benches use `wp_net`'s wire
  codec (`QVehicle` quantise → `DeltaVehicle` vs the previous net tick, absolute every 60 net
  ticks → `ServerMsg::Snapshot` with the first vehicle as `OwnState` → encode → decode →
  re-apply). Checks: re-encode equality, exact delta reconstruction, position error ≤ 5.1 mm.
  Typical size: 130–150 B per snapshot for 1–2 cars, 420 B for 24.
  **For F:** `DeltaVehicle::encode` keeps `rot`/`misc` values its mask omits, while `read`
  leaves them at their defaults. So `decode(encode(m)) != m` field-wise, though the wire
  meaning is identical. simcli compares re-encoded bytes instead; zeroing omitted fields in
  `encode` would make the struct round-trip exact.
- **Hash spec v3:** the state hash now includes `PoliceSystem::state_hash()`. Every golden was
  re-blessed (`[bless]`).
- **Perf:** re-blessed `claude-cloud.json` and appended to `docs/PERF.md`. `snapshot/encode_24` went
  from 40 to 3.3 µs and `decode_24` from 53 to 1.6 µs (F's binary codec vs JSON). The sim benches
  were 22–71 % slower than the run-3 baseline, but noisy: `sim_only/64` read 3.38 ms, then
  2.46 ms on a re-run. None of the harness changes touch the step these benches measure. **For
  the lead:** the remaining rise most likely comes from the run-4 merges (A's torque converter
  and AWD coupling, B's new damage events, F's contact filter); worth a look if the nightly
  gate on `gha-ubuntu-latest` agrees.
- Minimal edits outside my crates: `crates/wp_sim` (`set_police`, `police`), and
  `crates/wp_wasm/src/scenario.rs` (new `Scenario`/`EntitySpec` fields in its literals).

## Run 4 — contract v0.2.0 merge (2026-10-19)

- Merged `main` (contract 0.2.0, machines A/D/E and the run-3 merges). simcli already had the lead's
  `GlassShattered` / `RegionDamaged` arms. No other v0.2.0 item touches the harness: the typed
  `PoliceEvent` is deferred to v0.3.0.
- `PURSUIT_STARTED_KIND` is now `wp_police::events::kind::PURSUIT_STARTED` (3), not the provisional 1.
  The golden placeholders now use E's real codes: night pursuit escape → `PURSUIT_ENDED` (4),
  spike strip → `SPIKES_PLACED` (9). simcli depends on `wp_police` for the constants.
- Goldens re-blessed (`[bless]`): `main`'s physics changes (tire grip, shared ground collider)
  under hash spec v2. Merge conflicts in `vehicle_sedan_{brake,reverse}/hashes.txt` were
  resolved by regenerating them.
- **For E:** with police merged, the night-pursuit golden emits 13 `TrafficViolation`s (the
  racer speeding at 35 m/s) but no `PoliceEvent` at all. The parked "patrol" sedan is only a
  vehicle, and no police unit spawns in 12 s. Is there a scenario
  hook to spawn a police unit or set presence? simcli would wire it to `police_presence`.

## Run 3 — response to lead feedback (2026-10-19)

Done, on `ws/harness` (restarted from `main` after PR #1 merged):

1. **Scenario v2 additions.** `barriers: [(pos, size, yaw)]` → `Sim::add_static_box`; per-entity
   `speed_mps` → `Sim::place_vehicle`. Goldens retimed to start at speed (no run-ups):
   frontal 4 s, fwd_liftoff 8 s, rwd_power_slide 6 s, spike_strip 9 s, night_pursuit 12 s,
   tbone 4 s. Re-blessed (`[bless]`).
2. **CI.** New `workstream-checks` job: `python3 tools/specsheet/nameplate_check.py` (09) and
   `cargo test -p wp_economy --features native` (08).
3. **Perf.** The gate disagreed: the lead's baseline predates real damage (B) and world (D):
   `lattice/solver_8_contacts` 0.9 ns → 145 µs, `sim_only/24` 174 → 383 µs, `sim_only/64`
   0.52 → 2.77 ms, `city/24v_400traffic` 167 → 392 µs. Re-blessed `claude-cloud.json` (`[bless]`)
   and appended the entry to `docs/PERF.md`. **For B:** the lattice bench (8 frontal contacts
   every tick, a sustained-crash load) is 0.58 ms/car/frame against the 0.25 ms budget.
   `sim_only/64` is 0.17 ms/car/frame, inside the 0.40 combined budget.
4. **Cities are real.** `city: procedural(...)` and `file("city/<id>.road.json" | ".road.bin")`
   build a `wp_world::World` (seeded from the scenario seed; traffic `density_scale` from
   `traffic_density`; collision diagnostics on), pre-populate it around the spawned cars and
   hand it to `Sim::set_world`. No more city/traffic warnings. `procedural(seed: 1, 12, 12,
   110)` hashes identically to `file("city/procedural_12x12.road.json")`. New
   `world_metric(name, min, max)` expectation and a `world` block in `summary.json`. Scenarios
   may now have `entities: []`.
5. **Traffic soak (05).** `scenarios/soak/traffic_soak.ron`: 12×12 city, 4× density, no
   players, 60 s: 0 civilian collisions, ≥ 380 agents (measured: never below 400 after the
   10 s warm-up), ≥ 50 % moving (measured: min 0.64, final 0.76). **Finding for 05:** with one player car parked at the core
   (`traffic_soak_observer.ron`) the population dips to 358–364 for every position and seed I
   tried (collisions stay 0, flow ≥ 55 %), so 380 sits in that scenario's `expect_pending`.
   Soaks live in `scenarios/soak/`: debug `cargo test` skips them (~200× slower unoptimised:
   10 s of wall time per simulated second with 3344 parked-car bodies); CI's release
   `goldens`/`determinism`/wasm jobs run them.
6. **Rigid-barrier golden + B's question.** New `golden_barrier_56kmh` (static wall, Simulation
   mode): total loss, crush 0.86 m, 4 components failed, all now in `expect`. Answer to B:
   `golden_frontal_56kmh` into a *parked sedan* should **not** be a total loss. The parked car
   absorbs half the closing Δv for equal masses (about 28 km/h each, barrier-equivalent), and the
   model agrees: health 0.66, crush 0.62 m, 1 component failed. It now expects
   `total_loss(expected: false)`; its other pending targets now pass and moved into `expect`.

Also: hash spec v2 (world + debris, above). Fuzz rammers now start at speed and 30 % of cases get
a rigid wall. The night pursuit racer now starts in the northbound lane of the x = 60 collector:
on the city's centre line it ploughed into parked cars at 35 m/s.

Not done: the "switch to F's snapshot codec" item. F hasn't landed on `main` (`wp_net` still has
`{encode,decode}_server`), so nothing to switch to yet.

## Lead feedback (2026-10-19)

**Merged** (`297b6b8` + `2b5f063 [bless]`). Everything green on the integrated build: workspace tests,
determinism lint (0 errors), goldens, and `tools/wasm-determinism/compare.sh` (native == wasm on every
scenario).

Lead changes in your area (pull them; the reasons are physics landing, not bugs in your work):
- `smoke/straight_line.ron`: merged with 02's timing (brake until 8 s, then handbrake, because a held
  brake at standstill now engages auto-reverse) and 02's tighter ranges.
- 02's `vehicle/*.ron` converted to v2.
- Goldens retimed for real acceleration (the stub hit 100 km/h in ~3 s; real cars need 5-10 s):
  `fwd_liftoff` (+8 s run-up, custom assists: aids off but auto gears, since `off` = manual gearbox),
  `rwd_power_slide` (+2 s), `spike_strip` (+9 s, police moved to y = 400), `night_pursuit_escape`
  (+3.5-5 s, patrol at y = 200). `SCENARIOS.md` example retimed the same way.
- Cars are rigid bodies now and collide: `frontal_56kmh` barrier-car displacement moved to
  `expect_pending` (0.5-25 m); `grid_24` lets `c10` be nudged (≤ 2 m/s) and `s0` ≥ 5 m/s.
- Goldens re-blessed in a `[bless]` commit.
- Testbed's dialect crash files moved out of `apps/simcli/scenarios/` (see testbed.md).

New `wp_sim` API you can use: `Sim::add_static_box(pos, size, yaw)`, `Sim::place_vehicle(id, pos,
yaw, speed_mps)`, `Sim::set_mode_table`. `SimEvent::Contact` is now emitted (one per touching pair per
tick, summed impulse ≥ `wp_sim::CONTACT_EVENT_MIN_IMPULSE_NS` = 50 N·s), so `event_count(Contact)`
expectations work; `golden_frontal_56kmh` gets 1, `golden_tbone` 2, `grid_24` 119.

Next for you (priority order):
1. Scenario v2: add `barriers: [(pos, size, yaw)]` (→ `add_static_box`) and per-entity
   `speed_mps` (→ `place_vehicle`, so crash goldens can start at speed instead of long run-ups).
   Then retime the goldens back to short runs if you prefer, and re-bless.
2. CI additions requested by others: `python3 tools/specsheet/nameplate_check.py` (09) and
   `cargo test -p wp_economy --features native` (08).
3. Perf baselines were blessed against stub physics; the lead re-measured with the real model (see
   `docs/PERF.md`) — re-bless the baseline on your side if the nightly gate disagrees.

Contract requests: H1 (police kind codes) **accepted** — will be a typed `PoliceEvent` enum designed
with 06 in v0.2.0, so the provisional `PURSUIT_STARTED_KIND` goes away. H2 (`Systems` in contracts)
**rejected for now**: only `wp_sim`/simcli use it; revisit when 07 needs it.

### Run 2 (2026-10-19)
- Goldens re-blessed again (`[bless]`) for real damage (B) and world (D). All scenarios pass.
- B's barrier files moved to `crates/wp_damage/scenarios/`; your skip of `damage/` was dropped.
- Lint: `.log()` with no argument is no longer flagged; `allow.txt` gained file-level `f64` entries for
  machine D's time accumulators (temporary).
- simcli `net` round trip and `tools/bench` snapshot benches now JSON-encode `ServerToClient` themselves
  (F removed `wp_net::{encode,decode}_server`). Switch them to F's snapshot codec once F re-lands.
- New (run 3): `Sim::despawn_vehicle`, `set_vehicle_kind`, contact filter + `drain_applied_contacts`.
- New: `Sim::set_world(World)` → please make `city: procedural(...)` / `file(...)` real (no warning)
  and add D's traffic soak scenario (0 civilian collisions, ≥ 380 agents, ≥ 50 % moving).
- Question from B's tuning: should `golden_frontal_56kmh` (into a *parked sedan*) be a TotalLoss? A
  parked car absorbs energy; consider adding a rigid-barrier golden (static box via `barriers`).

### Lead run 5 (2026-10-19)
- `build_sim` city runs now set `POLICE_CAR` / `TRAFFIC_CAR` (`run.rs` consts), so city scenarios contain
  extra police / traffic vehicles. Metrics and expectations use `scenario_states(&sim, &ids)` (scenario
  entities only); fuzz names extra vehicles "city vehicle". City goldens re-blessed.
- `soak/traffic_soak_observer`: `civilian_collisions` gate 0 → ≤ 3, zero moved to `expect_pending`
  (physical traffic; D is investigating). Consider scenario fields to opt out of police / traffic.

## Lead feedback (round 6, 2026-10-19)

**Merged** (city chase golden, police/traffic/parked opt-outs, starting heat, F's snapshot codec in the bench).

Lead fixes in your area this round:
- `build_sim` installed your `police_for_city` **before** `set_world`, which rebuilds police on the city and threw
  your config / starting heat away. Now: `set_world` first, then `set_police(police_for_city(..))`; police car only
  with `police_cars`, physical traffic only with `traffic_cars`; `police_cars: false` = presence 0, heat kept.
- `golden/city_chase`: police cars are solid now and the racer starts 5 m behind a stopped patrol car at 22 m/s, so it
  rear-ends it at t = 0.11 s. Its checks moved to `expect_pending`. **Re-aim it** (start further back / adjacent
  lane, or wait for E's obstacle-aware units) and move the checks back to `expect`.
- `soak/traffic_soak_observer`: `police_cars: false` for now (police ram the parked observer; E's task). Flip it back
  once E lands obstacle awareness.


## Lead note (2026-10-19, from machine B)
- `simcli::run::crush_depth` should use `DamageModel::structural_crush_m()`. The any-node max counts shattered glass (0.58 vs 0.39 m in golden_tbone_sim). Please switch it, and re-bless only if expectations move, in a `[bless]` commit.
## Lead helper: test speed (2026-10-19, branch `lead/test-speed`)
Goal: `cargo test --workspace` under 10 min on a cloud container, same coverage, same hashes.
Machine: cloud container, 4 cores, 15 GB. All timings with `CARGO_INCREMENTAL=0
CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`, clean `target/`.
| | before | opt-level only | after (both) |
|---|---|---|---|
| `cargo test --workspace --no-run` (clean build) | 112 s | — | 333 s |
| `cargo test --workspace` (run) | **4206 s (70 min)** | — | **210 s (3.5 min)** |
| of which `simcli` `tests/scenarios.rs` | 3667 s | 465 s | 117–123 s |
| `wp_opponent` `tests/race.rs` | 154 s | — | 15 s |
| `wp_wasm` unit tests | 104 s | — | 3 s |
| `wp_net` `tests/bandwidth.rs` | 96 s | — | 24 s |
| `wp_sim` unit tests | 49 s | — | 2 s |
Clean build + full test run: ≈ 72 min → ≈ 9 min. With a warm cache (CI's rust-cache, local
re-runs) the extra build time for optimised dependencies is paid once.
What changed:
1. **`Cargo.toml` dev profile** (`build:`): `[profile.dev.package."*"] opt-level = 3` (rapier3d,
   parry3d, glam, nalgebra, serde, …) and `opt-level = 2` for `wp_math`, `wp_vehicle`,
   `wp_damage`, `wp_world`, `wp_police`, `wp_weather`, `wp_opponent`, `wp_race`, `wp_sim`,
   `simcli`. Debug assertions and overflow checks stay on (opt-level does not touch them), so
   `cfg!(debug_assertions)` code paths and the debug-only soak skip are unchanged. Other
   workspace crates stay at opt-level 0 for quick edit-compile cycles.
2. **Shared scenario pass** (`simcli:`): `all_scenarios_pass_and_are_deterministic_in_process`,
   `fresh_process_matches_in_process` and `goldens_match` used to simulate each scenario
   3 + 3 + 1–2 times (a plain run plus `check_in_process` twice over, then `check_scenario`).
   Now one `OnceLock` pass per test binary runs, for every scenario: two recorded in-process
   runs, one fresh `simcli hashes` child, and the golden replay verify (golden scenarios). The
   three tests read those results. What each test checks is unchanged: expectations (from
   recorded run 0; `RunSummary` does not depend on `record`), run 0 == run 1 via
   `determinism::compare_runs` (tick hashes, checkpoints, summary, replay), child stdout ==
   run 0's `hashes.txt`, and `golden::check_run` on run 0 plus the replay verify. Failures
   are now collected for all scenarios instead of stopping at the first.
   New library helpers (behaviour of the old entry points unchanged):
   `determinism::compare_runs`, `golden::check_replay`, `golden::check_run`, `golden::ReplayCheck`.
3. **Parallel jobs**: the pass spreads those jobs over `available_parallelism()` threads,
   costliest first. Each job is one whole single-threaded sim, so threading cannot change a
   hash; the two in-process runs of a scenario now run concurrently, which would also expose
   any shared global state.
4. `bless_is_refused_in_ci_without_tag` blesses only `scenarios/golden/tbone.ron` (was every
   golden scenario, twice, in a debug child); it checks the same guard and the same files.
5. **CI** (`ci.yml`): workflow env `CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=0`,
   `CARGO_PROFILE_TEST_DEBUG=0`. The profile itself lives in `Cargo.toml`, so CI, `just test`
   and local `cargo test` use the same opt-levels. The release jobs are untouched.
Determinism: no hash changed. `goldens_match` (debug, all non-soak scenarios) passes with the
optimised profile, `simcli goldens` (release, incl. soaks) passes, and
`tools/wasm-determinism/compare.sh` reports native == wasm on all 35 scenarios.
**Already failing on `claude/laughing-newton-0xqr6q` before this change** (same result before
and after, unrelated to speed; not fixed here):
- `wp_economy` `tests/pursuit_wiring.rs::real_police_capture_settles_through_the_economy`:
  assertion "level at capture, not after the reset" (line 130).
- `wp_race` `tests/simcli_races.rs::city_sprint_on_real_roads`: index out of bounds, len 2
  index 2 (line 28) — the test indexes a third entity the scenario no longer has.


## Lead feedback (round 8, 2026-10-19)

Contract **v0.3.0 is on `main`** (typed police/race/economy events, damage snapshot/summary, road-network trait, weather `LocalConditions`; see `docs/CONTRACT_CHANGELOG.md`). Workspace tests now take about 3.5 minutes (`lead/test-speed`). Owner playtest items are tracked in `docs/PLAYTEST_LOG.md`.

1. `crush_depth` → `DamageModel::structural_crush_m()` (B's request).
2. Promote the observer soak civilian-collision gate (D says it passes; E may flip police back on).
3. Run the slow city scenarios in the release CI jobs, per `lead/test-speed`.

## Lead helper: debug vs release hashes (2026-10-20, branch `lead/det-debug-release`)
Report (machine H, and `docs/integration/cars.md` "Lead follow-up round 12b"): on `lead/round12`
every scenario with steering (`vehicle/coupe_slalom`, `smoke/coupe_circle`,
`cars/beater_hard_turn`, …) hashed differently in a debug simcli than in the release simcli; the
goldens matched debug; scenarios without steering (`cars/beater_accel`, `beater_top_speed`) matched
both.

**Root cause: a stale release binary, not a profile difference.** `cargo test` / `cargo run`
rebuild simcli; `./target/release/simcli …` runs whatever was last built. A release simcli built
from `05f92f8` (`lead/round12` before the handling-balance merge `f6e9ecb`, i.e. before
`caa4797` slip-targeted keyboard/gamepad steering + ESC reference) and run against today's data and
goldens reproduces the report exactly: `coupe_slalom` / `coupe_circle` differ from tick 960,
`beater_hard_turn` from tick 720, while `beater_accel`, `beater_top_speed`, `sedan_brake`,
`sedan_reverse` match. That steering change only moves scenarios with steering input, which is
the whole pattern. (`tools/wasm-determinism/compare.sh` rebuilds simcli first, which is why
native == wasm still held.)

Fresh builds of `900261e` agree everywhere (4-core cloud container, AVX-512 + FMA, rustc 1.97.0):
- `simcli goldens` output (every scenario, incl. slow/soak; the "now" hashes of the pre-existing
  failures too) is byte-identical from the dev-profile and the release binary; `cargo test`'s
  `goldens_match` reports the same failures (minus the slow scenarios it skips).
- `simcli hashes` / `run --no-write` on the steering scenarios is identical across: dev profile
  (the opt-level 2/3 overrides), dev with incremental + full debuginfo, dev with **every** crate at
  opt-level 0, release, release with `-C target-cpu=native`, release with fat LTO +
  `codegen-units=1`.
- Not address- or memory-dependent: same hashes with ASLR off (`setarch -R`), `MALLOC_PERTURB_`,
  tcache off / low mmap threshold.
- Not CPU-dispatch dependent: `matrixmultiply` (runtime FMA/AVX dispatch) is not linked into
  simcli; forcing glibc onto its non-FMA/AVX variants (`GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2,-FMA,…`)
  leaves the hashes unchanged.
- Our sim crates have no `mul_add`, `cfg(debug_assertions)`, `HashMap`/`HashSet`, env reads or
  std transcendental calls in sim code. Rapier/parry use deterministic hashers (`IndexMap` + Fx)
  under `enhanced-determinism`; their `cfg(debug_assertions)` blocks are assertion-only.

So the Cargo.toml claim holds (rustc does not reorder or contract float ops at any opt-level)
and nothing was changed in the simulation; **no goldens move** (no `[bless]` commit).
`cars_beater_hard_turn`, blessed "with the debug binary" in `dba4862`, is identical in release.

**Guard:** `tools/profile-determinism/compare.sh [scenario.ron|dir …]` (`just det-profiles`, also
part of `just determinism` and a step in CI's determinism job) **rebuilds** simcli in both
profiles, then requires byte-identical `simcli hashes` output. Default set: `vehicle/`, `smoke/`,
`cars/` and `opponent/flee_police` (keyboard/gamepad steering, AI steering + ESC, traffic,
police; 15 scenarios, ~15 s after the build). Rule of thumb for everyone: never compare or bless
with a `target/*/simcli` you did not just build — use `cargo run [--release] -p simcli -- …` or
run `cargo build [--release] -p simcli` first.

Cross-machine note (not a profile issue, not changed here): rapier's joint-constraint builder,
soft-constraint plasticity and parry's `TriMesh::compute_pseudo_normals` call the platform libm
(`sincosf`, `powf`, `expf`, `acosf`) despite `enhanced-determinism`; glibc chooses the FMA or
non-FMA variant per CPU. The vehicle scenarios above don't depend on it (the forced non-FMA run
matched), and native == wasm holds, but if a future cross-machine mismatch involves joints or
trimesh setup, look there first.
