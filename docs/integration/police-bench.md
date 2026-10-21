# Police benchmark integration notes

- **Workstream / prompt:** P7 — police competency benchmark (`prompts/extra/P7_police_benchmark.md`)
- **Branch:** `claude/dazzling-mayer-ovag76` (the cloud session's branch; the prompt's name for it is `lead/police-bench`)
- **Crates / folders owned:** `crates/wp_opponent/src/evader.rs`, `apps/simcli/src/police_bench.rs`,
  `apps/simcli/tests/police_bench.rs`, the `police-bench` subcommand in `apps/simcli/src/main.rs`,
  `just police-bench`, `docs/integration/police-bench*.md`
- **Contract version built against:** 0.3.0

## Summary (plain language)

**What changed.** There is now a repeatable, numeric test of how good the police are. A
scripted getaway driver (the "evader bot") starts somewhere central, already wanted (level 1, 2
or 3), and tries to get away. The police find, chase and try to catch it the same way they deal
with a player in the game: no shortcuts, no staging. Each chase ends with a capture, an escape,
or after 5 minutes. The benchmark runs it across both cities, day and night, clear and rain,
wanted levels 1–3 and three driver skills (casual, good, expert), with 8 different random seeds
each, and prints a table: how fast the police get close, how often they make contact, how much
of the chase they can see the car, how many cars are near it, how often they catch it and how
quickly, how often it escapes, and how often police cars crash, hit civilians or get stuck.

**Police behaviour is unchanged.** This branch only measures. Its one reading of police state
that is not already in `PoliceStats` is a unit's current tactic (to count PIT attempts by
physical police cars, which `PoliceStats::pit_attempts` does not count).

**Numbers before / after** (default set: 12 cells × 8 seeds = 96 chases, ≤ 5 min each):

| | `lead/round12` (before) | + `ws/police` @ 759d850 (now) |
|---|---:|---:|
| chases where a pursuit started | 88 / 96 | 91 / 96 |
| first police car within 100 m (median) | 12 s | 12 s |
| pursuit starts (median) | 23 s | 23 s |
| first contact with the evader (median, runs with contact) | 82 s | 83 s |
| contacts per pursuit minute | 0.21 | 0.73 |
| PIT attempts (successful) | 1 (1) | 9 (4) |
| share of pursuit time with police line of sight | 84 % | 84 % |
| police cars within 300 m during a pursuit (mean) | 2.2 | 2.9 |
| **capture rate** | **18 %** | **66 %** |
| capture time after the pursuit starts (median) | 45 s | 45 s |
| **escape rate** | **53 %** | **25 %** |
| police crashes into buildings / walls / parked cars per minute | 1.32 | 2.62 |
| police collisions with civilian cars per minute | 0.04 | 0.36 |
| police stuck records per minute | 1.64 | 5.26 |
| police–police collisions (all runs) | 27 | 61 |

In words: machine E's round 12c work (tactics, backup, push-through, building avoidance) makes
the police **far more effective** (captures ×3.7, escapes halved, the expert evader now caught
in 3 of 8 chases instead of never), at the cost of **twice the crashes, nine times the civilian
hits and three times the stuck records**. The worst spot for that is Port Vesper by day
(≈ 5 crashes and 11–14 stuck records per minute). That is the obvious next target. Full tables:
[`police-bench-results.md`](police-bench-results.md) (now) and
[`police-bench-results-round12.md`](police-bench-results-round12.md) (before).

**How to see it.** `just police-bench` (or `cargo run --release -p simcli -- police-bench`),
≈ 7–10 minutes on 4 cores; it prints the tables and rewrites `police-bench-results.md`. A single
cell, quickly: `just police-bench --city vesper --phase night --weather clear --level 2 --skill
expert --seeds 4 --no-write`. Watch one chase in numbers: `WP_BENCH_TRACE=240` before the same
command prints the evader once per second (position, speed, what it knows, police status).
Not in the browser yet (see Known gaps).

## Exposes

- **`simcli police-bench`** (`apps/simcli/src/main.rs`) and `just police-bench *args`:
  - `--full` (all 72 cells), `--seeds N` (default 8), `--duration S` (default 300),
    `--threads N` (default: all cores);
  - filters `--city vesper,grid`, `--phase day,night`, `--weather clear,rain`, `--level 1,2,3`,
    `--skill casual,good,expert`; `--mode arcade|simulation` (default Arcade; police AI is the
    same in both, only damage differs);
  - `--label TEXT` (report title), `--out FILE.md` (default
    `docs/integration/police-bench-results.md`) or `--no-write`, `--json FILE` (every run's
    `RunMetrics`).
- **`simcli::police_bench`** (library, for tests and other tools): `BenchCell` (city, night,
  rain, level, skill; `matrix()`, `slow()`, `scenario()`), `BenchOptions`, `run_bench`,
  `run_one`, `RunMetrics`, `RunEnd`, `summarize`, `summaries`, `table`, `markdown_table`,
  `markdown_report`.
- **`wp_opponent::Evader`** (`crates/wp_opponent/src/evader.rs`): `Evader::new(params, def, seed,
  speed)`, `step(map, state, police_cars, obstacles, dt) -> VehicleInput`, `set_grip_scale`,
  `stats`, `debug`, `known_units`, `path`; `EvaderSkill::{Casual, Good, Expert}` →
  `EvaderParams`; `PoliceCar` (what the world shows of a police car). Skill table in
  `crates/wp_opponent/README.md`.

### The matrix

Full: 2 cities (Port Vesper v2, 12×12 grid) × day (12:00) / night (23:00) × clear / heavy rain
on a wet road (0.8 intensity, 1 mm water) × start L1 / L2 / L3 (heat 15 / 35 / 55) × casual /
good / expert = **72 cells**. Default (not `slow`): **12 cells**:
- environment sweep: the good evader at L2 in all 8 city × phase × weather combinations;
- skill sweep: casual and expert at L2, Port Vesper night clear (the testbed's city chase);
- level sweep: the good evader at L1 and L3, Port Vesper night clear.

Every run: the scenario the game would build (`simcli::run::build_sim`): traffic and parked
cars on, police cars on (`sentinel_cruiser`) at the built-in presence, AI assists on the
evader's `debug_coupe`. The evader starts on a ≥ 80 m, non-highway, non-tunnel lane in the core
or commercial districts picked by the seed, at 15 m/s. Nothing else is set up.

### Metrics (per run → per cell)

| Column | Per run | Per cell |
|---|---|---|
| `engaged` | a pursuit started | runs |
| `near100 s` | first time any in-service police car is within 100 m | median |
| `never near` | no police car ever within 100 m | runs |
| `pursuit s` | `PursuitStarted` for the evader | median |
| `contact s` | first physical evader–police contact | median of runs with contact |
| `contacts/min` | contact episodes (per unit, merged within 1 s) | per pursuit minute |
| `PIT ok/att` | contacts by a unit in its `Pit` tactic (+ kinematic units' `pit_attempts`); ok = evader spun (> 60° off its travel) or stopped within 3 s | sums |
| `LOS` | time with status `Pursuit` / time in `Pursuit` or `Searching` | pooled |
| `units≤300m` | in-service police cars within 300 m, 1 Hz during the pursuit (also at +15/30/60/120 s in the JSON) | mean |
| `lead m` | keep-up: distance to the nearest unit chasing the evader (`Pursue` / `Intercept`), 1 Hz while police see it | median of all samples |
| `pace` | keep-up: that unit's speed ÷ the evader's, 1 Hz while police see it and it drives faster than 25 m/s (~90 km/h); 1.0 = matching it | median of all samples |
| `capture` | pursuit ended `Captured` / `CapturedTotalLoss` | share of runs |
| `capture s` | capture time after the pursuit start | median |
| `escape` | pursuit ended `Escaped` / `Safehouse` | share of runs |
| `crash/min` | police car hits the static world (buildings, walls, parked cars, cones) at ≥ 4 m/s, merged per car within 2 s | per simulated minute |
| `civ/min` | police car hits a physical civilian car at ≥ 2 m/s (+ the kinematic driver's traffic collisions) | per simulated minute |
| `stuck/min` | `PoliceStats::stuck` records (one per unstick step, so a long stuck counts several) | per simulated minute |

Also in the JSON: police–police collisions, self / contact wrecks, units that pursued, calls,
highest wanted level, evader health, evader civilian hits, evader plans / reversals. A run ends at
the first pursuit end, or after `--duration` (`TimeoutInPursuit` / `NeverEngaged`).

## Needs
| From | What | Workaround now |
|---|---|---|
| Police (E) | Count PIT attempts by physical units in `PoliceStats::pit_attempts` (today only the kinematic driver's proximity contacts count) | The bench counts contacts by a unit whose tactic is `Pit` |
| Police (E) | Optional: a `PoliceStats` counter for unit crashes into static geometry, so the game (not only this bench) can report it | The bench classifies `SimEvent::Contact`s itself |
| Lead / testbed | Optional: an "evader" button in the testbed to watch one benchmark chase (`wp_wasm` drives `Evader` for an AI car) | `WP_BENCH_TRACE` numbers only |

## Contract change requests
| # | Change | Why | Who is affected | Status |
|---|---|---|---|---|
| — | none | | | |

## Changes outside my crates
- `crates/wp_opponent/src/lib.rs`: `pub mod evader;` and re-exports (wp_opponent is machine N's
  crate; the evader is a new module and touches nothing else there). `crates/wp_opponent/README.md`:
  evader section.
- `apps/simcli` (machine I's app): new module, subcommand, test, README section; `wp_opponent`
  added to its dependencies (pure Rust, wasm32-clean).
- `justfile`: `police-bench` recipe.
- No edits in `crates/wp_police` (the merge of `origin/ws/police` is E's work as-is; the 5
  conflicting goldens were taken from `ws/police` and pass `simcli goldens`).

## Perf numbers
| Measurement | Machine | Native (release) | Budget |
|---|---|---|---|
| `police-bench` default (96 runs) | cloud container, 4 vCPU | 9 m 40 s on `lead/round12`, 7 m 14 s with `ws/police` (more runs end early) | < 15 min |
| simulation speed in a benchmark run | same, per core | ≈ 5× real time on Port Vesper with traffic + parked cars | — |
| evader controller | same | not timed separately; benchmark runs simulate at ≈ 5.3× real time, the same Port Vesper setup with a parked player car at ≈ 4.7×, so the evader is not the bottleneck | — |
| CI-light test (`apps/simcli/tests/police_bench.rs`, 3 cells × 2 seeds × 120 s + determinism) | same, debug profile | ≈ 50 s | — |
| `--full` (576 runs) | same | ≈ 1 hour (estimate from the default set's run lengths; not run yet) | — |

## Known gaps
- **Sample size.** 8 seeds per cell: a single cell's capture or escape rate is ±15–17 points
  (1 σ). Use the roll-ups (by skill, level, city, phase, weather) or `--seeds 16` before reading
  much into one cell. Example: at the reference spot the casual evader escaped more often than
  the good one in both baselines. That is within the noise, and evader skill calibration is
  not finished.
- **The evader is lane-legal.** It follows the lane graph (no wrong-way driving, no U-turns
  except after being blocked twice in one place, no short cuts through parks or car parks), and
  it changes lanes only into a real lane. It is a decent driver, not a great one: in
  `lead/round12` it hit civilians 2.5 times per run on average and reversed out of a stop ~20
  times per 5-minute run. Traffic queues it cannot pass (both neighbouring lanes busy, e.g.
  civilians stopped for sirens) are where most stops happen; a player could do better there.
- **The start is cold** (wanted, unseen, central). Response time is therefore measured from
  "already wanted somewhere in the core" and not from a specific offence.
- **One car each way** (evader `debug_coupe`, police `sentinel_cruiser`), Arcade damage by
  default. `--mode simulation` exists but was not baselined.
- **`stuck/min` counts unstick records**, not stuck episodes; compare it only with itself.
- **Pre-existing failure, not from this branch:** on `lead/round12`,
  `apps/simcli/scenarios/police/tunnel_escape_founders_cut.ron` fails (`Captured at 136.4 s`
  instead of `Escaped` at 105–150 s). See the gate results below for its state after the
  `ws/police` merge.
- Not drawn in the browser (no testbed hook); numbers only.

## Gate (2026-10-20, cloud container, 4 vCPU)

Branch head = `lead/round12` (a3f2534) + this work + `origin/ws/police` (759d850) merged.

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | pass |
| `cargo build --target wasm32-unknown-unknown -p wp_wasm`, `npm run wasm` (wasm-pack 0.13.1) | pass |
| `crates/wp_net/tests/vesper_e2e.rs` | pass |
| this branch's tests: `wp_opponent` evader unit tests (4), `simcli --test police_bench` (3) | pass |
| `simcli run apps/simcli/scenarios --no-write` (release) | 78 / 79; fails `police/tunnel_escape_founders_cut` (Captured at 138.6 s; also fails on plain `lead/round12`) |
| `cargo test --workspace --no-fail-fast` | 4 targets fail, **none from this work** (below) |
| `npm run e2e` (`just web-e2e`) | every functional check passes; `perf_check.mjs` (software GL) misses frame-time limits: 4 checks under full CPU load, 1 when re-run alone (rain: 19 frames > 250 ms, limit 12). No browser code or wasm export changed here |

Failing workspace tests, and where they come from (each reproduced without this work):

| Test | Also fails on `lead/round12` a3f2534 | Also fails on `origin/ws/police` 759d850 |
|---|---|---|
| `simcli --test scenarios goldens_match` (19 goldens on the branch head) | yes (`simcli goldens`: 30) | yes (30) |
| `wp_police --test system contact_tactics_escalate_with_the_wanted_level` | n/a (test is new on ws/police) | yes |
| `wp_race --test race_picker` (`loop9_circuit`, `lantern_checkpoint_draws_the_police`) | yes | yes |
| `wp_wasm --lib race_tests::short_sprint_with_ai_runs_to_results_and_pays_through_the_economy` | no: appears with the ws/police merge | not checked |
| `wp_opponent --test race live_police_chase_ends_in_escape_or_capture_without_stuck_cars` | yes; **passes** after the ws/police merge | — |

No goldens were re-blessed: this branch moves none (the bench adds new modules only and no
scenario runs through them). The moved goldens belong to ws/police's in-progress work and to
`lead/round12`'s current state; re-bless them where that work lands.
