# Opponent AI pace (playtest row 99)

- **Helper branch:** `lead/opponent-pace` (from `lead/round12`, with `lead/r13-test-fixes` and the
  `lead/round12` tip merged in)
- **Owner report (preview v0.12.12):** "the AI in the AI races are quite dumb and slow."
- **Crates touched:** `wp_opponent` (all behaviour), `wp_race` (glue + tests), `wp_wasm`
  (one line: real car limits for the picker AI). No contract change. No `wp_vehicle` change
  (steering is machine A's; the AI keeps `AssistProfile::ai()`, see below).
- Design rules kept: **no rubber-banding** (pace reads nothing about positions or the
  player), same physics in Arcade and Simulation, deterministic (new state in `BTreeMap`s and
  per-racer fields; polynomial trig).

## Method

`crates/wp_race/tests/opponent_pace.rs::pace_sweep` (ignored; `cargo test --release -p wp_race
--test opponent_pace pace_sweep -- --ignored --nocapture`) races every picker race (Loop 9
circuit, Hollins sprint, Lantern checkpoint, grid highway sprint) and the procedural city
sprint in the full `wp_sim` loop with `wp_opponent` in every seat (the player's too), every
seat at one skill (25/50/75/95 %), the seat's own picker personality otherwise, police off,
traffic as in the race. Knobs: `WP_PACE_SOLO=1` (pole car only), `WP_PACE_DENSITY=0`,
`WP_PACE_GEOM=1` (the old geometry-only limits), `WP_PACE_TRACE=<seat>` (2 Hz trace with what
limits the car), `WP_PACE_POLICE=1`, `WP_PACE_SEED`.

- **Ideal time** (per car): its own racing line, its speed profile at **100 %** of the car's
  *measured* limits (`CarCapability::measure`: skidpad, 100–0, 0–100, top speed), standing
  start, the profile's acceleration model. A bound, not a lap anyone drives: it assumes
  perfect tracking and no traffic.
- **"Good human" reference**: the solo, traffic-free skill-0.95 run (a clean line driver at
  95 % of the grip; the scripted fast-line driver of the brief). No human telemetry exists in
  the repo; a casual keyboard player was not measured (estimate: 15–30 % over ideal on
  these courses).
- **Loss attribution** per tick, `dt·(1 − v/v_ideal(s))`, booked to what set the car's speed
  target: `usage` (driver's own profile below ideal — the intended skill gap), `track` (below
  its own target: lag, traction, braking early), `racer` / `civ` / `veh` (a car ahead:
  racer, civilian agent, physical traffic car), `backout`, `unstable`, `mode` (off-line
  path caps, rejoin caps), `signal`, `rejoin` / `reverse` (recovering). The climb back to
  the profile after a slowdown is booked to the cause of the slowdown.

## What was slow and dumb (before, measured)

| # | Finding | Where | Cost |
|---|---|---|---|
| 1 | **Every AI car drove like a generic street car**: race glue (browser picker, `LiveRace`, picker tests) built `CarCapability::geometry_only` (0.85 g, 0.95 g braking, 4 m/s², **55 m/s top**) instead of the car's measured limits. On Loop 9 the 55 m/s cap made every skill level the same pace (≈ 200 s own profile vs 161–172 s ideal). | all races | 15–35 s per car on Loop 9, skill irrelevant |
| 2 | **Rejoin for leaving the lane, not the road**: `off` used the lane limits; on one-lane-each-way roads (Hollins) the car may move ±0.3 m, so a 1.3 m drift in a hairpin, or a grid slot beside the lane, dropped it to 7 m/s "rejoin" pace. | Hollins | 4–20 s per car |
| 3 | **Civilians one lane over counted as in my path** (civilian margin 1.1–1.8 m + up to 1 m for closing speed > a lane's spare width), and passing a civilian demanded the racer-vs-racer overtake margin on top (`NoRoom`): cars braked from 32 to 11 m/s behind a civilian on a multi-lane road. Physical traffic cars were passed with racer margins too. | grid sprint, Loop 9, Lantern | 10–40 s |
| 4 | **Passes were planned only within 30 m** (1.5 s at highway closing speeds) and lane changes ran at ~2 m/s lateral. | highways | several s per pass |
| 5 | **Junctions: any civilian "heading in" within 4 s blocked entry**, including cars waiting at their own stop line, oncoming cars going straight on, and crossings long over before arrival: 20–28 s standing at single junctions. | grid sprint, Lantern | up to 30 s per junction |
| 6 | **Perception blind spot**: civilians/vehicles more than 4–6 m outside the *own-direction* road were dropped, so a racer passing on the opposing lanes could not see a car standing there (drove at it → civilian-contact DQ in the open-road sweep). | open roads | DQs |
| 7 | **No speed-scaled perception**: a stalled car on Loop 9 was noticed ~120 m out at 64 m/s (needs ~215 m to stop) → late swerve, 23 m/s into the wall, engine dead. | Loop 9 | DNF |
| 8 | **High-speed weave**: pure pursuit with a 0.45 s lookahead and no lag compensation oscillated with growing amplitude in lane changes above ~35 m/s (off the road, DQ `LeftCorridor`). Hidden before by the 55 m/s cap. | Loop 9 | DNF/DQ |
| 9 | **Running wide in tight corners**: the car settled ~1.3 m wide of the pursuit target in junction turns (tyre slip, lag) → rejoin; braking lagged the profile by ~3 m/s (P-only speed loop) → hot entries. | Lantern | 4–6 s + wall hits |
| 10 | Speed profile ignored **road grade** (11 % on Hollins) and over-estimated acceleration at speed (`a0·(1−(v/top)²)`). | Hollins, highways | dishonest targets |
| 11 | Skill spread too small at the bottom, too timid at the top: usage 0.68–0.93 lateral, 0.68–0.92 braking. | all | — |
| 12 | Stand-offs: nose to nose with a standing car in the opposing lanes (or a civilian turning round across a hairpin) — nobody moved, forever. Long-standing cars (traffic deadlocks, a crawler) were never passable for careful drivers. Finished cars drove into the car park wall past the finish (Hollins), passed parked cars on the run-off in front of patrols (pursuit). | Lantern, Hollins, city sprint | DNFs |

Assists: the AI already drives with `AssistProfile::ai()` (ABS, TCS, ESC, no filtering,
`steer_speed_sensitivity` 0, `countersteer_help` 0) — machine A's keyboard/gamepad steering
helpers are inactive for it. Measured alternatives (Hollins, skill 0.95, solo): ESC off →
spins, 70.5 → 114.8 s, 18 wall hits; TCS off → 72.7 s; both off → 161 s. So no AI-specific
profile was needed; the AI keeps `ai()`.

## Fixes (all in `wp_opponent` unless noted)

1. `CarCapability::for_car` + generated `capability_table` (every car in `data/cars`,
   fingerprinted on mass / drag area / peak torque / redline, falls back to `measure`);
   `tests/capability_table.rs` fails when a row drifts > 2 % from a fresh measurement (it
   caught 9 cars after the round-12b physics merge; regenerated). Race glue (`wp_wasm`
   picker, picker/police/Vesper tests) uses it.
2. Speed profile: road grade in both passes; `drive_accel` (flat to 22 m/s, then
   power-limited minus drag sized to the measured top speed). Skill usage: lateral
   0.60 → 0.95, braking 0.55 → 0.95, accel 0.80 → 1.0; beginners brake up to 0.35 s early
   (`early_brake_s`).
3. Steering: pure pursuit from the pose 0.2 s ahead (velocity + yaw rate), lookahead +0.3 s
   per m/s above 25 m/s, curvature feedforward κ·L²/8 toward the inside in tight corners.
   Speed: braking feedforward from the profile slope (`v·dv/ds`).
4. Rejoin only off the *road* (opposing lanes included), never in the first 30 m from the
   grid, 7 → 17 m/s depending on heading.
5. Traffic: civilian margin 1.0 → 0.6 m (risk; 0.4 m creeping), closing-speed extra ≤ 0.5 m
   for civilians, pass margin 0.3 m for non-racers, pass planning 3.5 s at the closing speed,
   lane-change rate ×1.6 max tapering above 30 m/s, perception over the whole road and at
   least 1.3× the braking distance + 30 m, anything standing 8 s counts as parked, non-racers
   crawling under 70 % of my pace may be passed on the opposing lanes by careful drivers too,
   off-line paths at 85 % of the line's grip.
6. Junctions: time-window gap acceptance in my swept strip (see README).
7. Recovery: head-on reverse (3 s, opposing lanes or facing a standing car), reverse capped
   at 3 m/s and aborted when anything is close behind, reversing racers flagged unstable,
   finished cars park before the corridor ends, done racing past `RaceConfig::finish_m`.
8. Parked police: a sim vehicle standing within 2.5 m of where it stopped (below 1 m/s) for
   8 s counts as parked and is squeezed past at a crawl with 0.2 m clearance (normal by
   15 m/s), 0.15 m nearer the road edge (Hollins: a responding unit stopped on the hairpin
   held three racers for the rest of the race; follow-up after the round-12 tip merge).
9. `wp_race`: `AiEntrants` sets `finish_m` (judged finish on the run-off) and
   `max_half_width_m` (judged corridor + rule margin − 0.5 m: a risk-taker passing on the far
   side of Loop 9 was DQ'd `LeftCorridor`).

Open-road safety sweep (`wp_opponent/tests/race.rs::sweep_open_roads_progress`, 8 seeds, 25 %
traffic, 90 s): total progress **16.9 → 26.3 km (+55 %)**, longest wait 66 → 29 s; zero
civilian contacts, zero racer contacts ≥ 400 N·s, nobody stuck, in every seed (the sweep
caught and drove the fixes for #6, reversing into a car, and creeping overshoot).

## Before / after (merged physics: `lead/r13-test-fixes` + `lead/round12` @ 981bc56a)

"Before" = the same tree without this branch's changes (geometry-only limits, as the game
used them); "after" = this branch. Same harness, seeds, personalities. The ideal is the car's
own profile at 100 % of its measured limits (after's model; the `ideal` column is the best car
of the field), so ratios compare like with like within a row. Times in seconds; "finished" =
judged finishers / cars (DNFs before: finish window behind queues, crashes, `LeftCorridor`).

#### Full field, picker traffic, police off (every seat at the skill; times in s)

| race | skill | ideal (best car) | before: best / mean, finished | after: best / mean, finished | after best ÷ ideal | after: rejoins / reverses / wall hits | biggest remaining losses (after, best car) |
|---|---|---|---|---|---|---|---|
| loop9_circuit | 0.25 | 171.0 s | 221.6 / 236.4 s, 5/6 | 219.5 / 233.2 s, 6/6 | 1.284 (hakone_gt) | 0 / 0 / 0 | usage 8.1, track 0.7, civ 2.3, veh 25.8, backout 3.9, mode 7.2 |
| loop9_circuit | 0.50 | 171.0 s | 236.7 / 240.0 s, 4/6 | 215.8 / 227.1 s, 6/6 | 1.202 (kestrel_ts) | 0 / 0 / 0 | usage 10.3, track 5.0, civ 12.6, mode 8.0 |
| loop9_circuit | 0.75 | 171.0 s | 216.7 / 232.4 s, 6/6 | 209.1 / 220.2 s, 6/6 | 1.164 (kestrel_ts) | 0 / 0 / 0 | usage 5.5, track 5.9, civ 8.3, mode 9.4 |
| loop9_circuit | 0.95 | 171.0 s | 214.0 / 233.0 s, 5/6 | 205.7 / 215.7 s, 6/6 | 1.145 (kestrel_ts) | 0 / 0 / 0 | usage 1.8, track 5.7, civ 9.1, mode 9.2 |
| hollins_sprint | 0.25 | 54.7 s | 68.8 / 74.7 s, 3/4 | 101.6 / 114.4 s, 4/4 | 1.821 (hakone_gt) | 10 / 0 / 0 | usage 9.6, track 5.0, civ 33.9, mode 1.7, rejoin 0.6 |
| hollins_sprint | 0.50 | 54.7 s | 66.6 / 72.5 s, 3/4 | 65.8 / 88.7 s, 4/4 | 1.179 (hakone_gt) | 9 / 2 / 1 | usage 6.6, track 5.2, civ 2.7, mode 0.1 |
| hollins_sprint | 0.75 | 54.7 s | 64.8 / 78.5 s, 4/4 | 61.9 / 66.5 s, 4/4 | 1.109 (hakone_gt) | 2 / 0 / 1 | usage 3.7, track 5.6, civ 1.3, mode 0.1 |
| hollins_sprint | 0.95 | 54.7 s | 64.1 / 70.0 s, 3/4 | 60.1 / 64.8 s, 4/4 | 1.077 (hakone_gt) | 2 / 0 / 0 | usage 1.6, track 6.1, civ 0.3, unstable 0.6 |
| lantern_checkpoint | 0.25 | 34.7 s | — / —, 0/4 | 54.0 / 67.1 s, 3/4 | 1.500 (shiro_turbo) | 10 / 6 / 0 | usage 2.8, track 2.3, civ 16.2, mode 0.3, signal 3.1, rejoin 0.3 |
| lantern_checkpoint | 0.50 | 34.7 s | 83.4 / 101.3 s, 3/4 | 58.3 / 60.9 s, 4/4 | 1.619 (shiro_turbo) | 3 / 0 / 0 | usage 2.1, track 2.6, civ 17.9, mode 0.1, signal 2.5, rejoin 4.1 |
| lantern_checkpoint | 0.75 | 34.7 s | 76.8 / 76.8 s, 1/4 | 46.6 / 56.6 s, 4/4 | 1.294 (shiro_turbo) | 4 / 0 / 0 | usage 1.2, track 2.9, civ 11.2, mode 0.1, signal 2.1, rejoin 0.1 |
| lantern_checkpoint | 0.95 | 34.7 s | 91.9 / 121.4 s, 4/4 | 50.1 / 60.6 s, 4/4 | 1.392 (shiro_turbo) | 4 / 0 / 0 | usage 0.4, track 2.6, civ 16.3, signal 1.7, rejoin 0.1 |
| grid_highway_sprint | 0.25 | 20.6 s | 58.2 / 64.2 s, 4/4 | 38.6 / 46.0 s, 4/4 | 1.686 (kestrel_ts) | 1 / 0 / 0 | track 5.8, civ 21.7 |
| grid_highway_sprint | 0.50 | 20.6 s | 59.6 / 60.8 s, 4/4 | 38.6 / 45.1 s, 4/4 | 1.686 (kestrel_ts) | 1 / 0 / 0 | track 5.7, civ 21.8 |
| grid_highway_sprint | 0.75 | 20.6 s | 51.3 / 57.6 s, 4/4 | 38.5 / 49.8 s, 4/4 | 1.681 (kestrel_ts) | 1 / 0 / 0 | track 5.7, civ 21.7 |
| grid_highway_sprint | 0.95 | 20.6 s | 51.3 / 57.7 s, 4/4 | 38.4 / 44.8 s, 4/4 | 1.677 (kestrel_ts) | 1 / 0 / 0 | track 5.7, civ 21.5 |
| city_sprint | 0.25 | 9.1 s | 9.9 / 12.1 s, 4/4 | 10.0 / 12.2 s, 4/4 | 1.099 (debug_coupe) | 0 / 0 / 0 | civ 7.4 |
| city_sprint | 0.50 | 9.1 s | 9.7 / 11.9 s, 4/4 | 9.8 / 12.0 s, 4/4 | 1.077 (debug_coupe) | 0 / 0 / 0 | civ 7.2 |
| city_sprint | 0.75 | 9.1 s | 9.7 / 11.9 s, 4/4 | 9.7 / 11.8 s, 4/4 | 1.066 (debug_coupe) | 0 / 0 / 0 | civ 7.1 |
| city_sprint | 0.95 | 9.1 s | 9.7 / 12.1 s, 4/4 | 9.6 / 11.8 s, 4/4 | 1.055 (debug_coupe) | 0 / 0 / 0 | civ 7.0 |

#### Solo (pole car only), no traffic: the AI's own driving

| race | skill | ideal (best car) | before: best / mean, finished | after: best / mean, finished | after best ÷ ideal | after: rejoins / reverses / wall hits | biggest remaining losses (after, best car) |
|---|---|---|---|---|---|---|---|
| loop9_circuit | 0.25 | 179.6 s | 213.5 / 213.5 s, 1/1 | 209.8 / 209.8 s, 1/1 | 1.168 (kestrel_ts) | 0 / 0 / 0 | usage 17.3, track 4.2, backout 0.7, mode 7.7 |
| loop9_circuit | 0.50 | 179.6 s | 210.0 / 210.0 s, 1/1 | 203.0 / 203.0 s, 1/1 | 1.130 (kestrel_ts) | 0 / 0 / 0 | usage 10.6, track 4.7, backout 0.4, mode 7.4 |
| loop9_circuit | 0.75 | 179.6 s | — / —, 0/1 | 198.1 / 198.1 s, 1/1 | 1.103 (kestrel_ts) | 0 / 0 / 0 | usage 5.6, track 6.1, veh 0.1, backout 0.1, mode 6.4 |
| loop9_circuit | 0.95 | 179.6 s | — / —, 0/1 | 194.1 / 194.1 s, 1/1 | 1.081 (kestrel_ts) | 0 / 0 / 0 | usage 1.7, track 5.7, veh 0.1, backout 0.1, mode 6.6 |
| hollins_sprint | 0.25 | 55.8 s | 67.1 / 67.1 s, 1/1 | 66.8 / 66.8 s, 1/1 | 1.197 (hakone_gt) | 0 / 0 / 1 | usage 10.4, track 5.0, mode 0.2 |
| hollins_sprint | 0.50 | 55.8 s | 65.0 / 65.0 s, 1/1 | 63.7 / 63.7 s, 1/1 | 1.142 (hakone_gt) | 0 / 0 / 2 | usage 6.9, track 5.5, mode 0.1 |
| hollins_sprint | 0.75 | 55.8 s | 64.3 / 64.3 s, 1/1 | 60.8 / 60.8 s, 1/1 | 1.090 (hakone_gt) | 0 / 0 / 0 | usage 3.9, track 5.5, mode 0.1 |
| hollins_sprint | 0.95 | 55.8 s | 61.8 / 61.8 s, 1/1 | 59.5 / 59.5 s, 1/1 | 1.066 (hakone_gt) | 0 / 0 / 0 | usage 1.6, track 5.9, unstable 0.4 |
| lantern_checkpoint | 0.25 | 36.0 s | 41.3 / 41.3 s, 1/1 | 43.8 / 43.8 s, 1/1 | 1.217 (shiro_turbo) | 1 / 0 / 0 | usage 4.0, track 4.0, mode 0.3, signal 6.2, rejoin 0.3 |
| lantern_checkpoint | 0.50 | 36.0 s | 40.8 / 40.8 s, 1/1 | 41.4 / 41.4 s, 1/1 | 1.150 (shiro_turbo) | 1 / 0 / 0 | usage 2.6, track 4.2, mode 0.2, signal 5.2, rejoin 0.2 |
| lantern_checkpoint | 0.75 | 36.0 s | 39.3 / 39.3 s, 1/1 | 39.8 / 39.8 s, 1/1 | 1.106 (shiro_turbo) | 1 / 0 / 0 | usage 1.4, track 4.5, mode 0.1, signal 4.7, rejoin 0.1 |
| lantern_checkpoint | 0.95 | 36.0 s | 38.4 / 38.4 s, 1/1 | 38.7 / 38.7 s, 1/1 | 1.075 (shiro_turbo) | 1 / 0 / 0 | usage 0.5, track 4.7, signal 4.3, rejoin 0.1 |
| grid_highway_sprint | 0.25 | 22.9 s | 23.0 / 23.0 s, 1/1 | 23.0 / 23.0 s, 1/1 | 1.004 (kestrel_ts) | 0 / 0 / 0 | track 11.9 |
| grid_highway_sprint | 0.50 | 22.9 s | 22.9 / 22.9 s, 1/1 | 22.9 / 22.9 s, 1/1 | 1.000 (kestrel_ts) | 0 / 0 / 0 | track 11.8 |
| grid_highway_sprint | 0.75 | 22.9 s | 22.9 / 22.9 s, 1/1 | 22.9 / 22.9 s, 1/1 | 1.000 (kestrel_ts) | 0 / 0 / 0 | track 11.8 |
| grid_highway_sprint | 0.95 | 22.9 s | 22.9 / 22.9 s, 1/1 | 22.9 / 22.9 s, 1/1 | 1.000 (kestrel_ts) | 0 / 0 / 0 | track 11.8 |
| city_sprint | 0.25 | 9.1 s | 9.5 / 9.5 s, 1/1 | 9.4 / 9.4 s, 1/1 | 1.033 (debug_coupe) | 0 / 0 / 0 | track 6.8 |
| city_sprint | 0.50 | 9.1 s | 9.4 / 9.4 s, 1/1 | 9.4 / 9.4 s, 1/1 | 1.033 (debug_coupe) | 0 / 0 / 0 | track 6.7 |
| city_sprint | 0.75 | 9.1 s | 9.4 / 9.4 s, 1/1 | 9.4 / 9.4 s, 1/1 | 1.033 (debug_coupe) | 0 / 0 / 0 | track 6.7 |
| city_sprint | 0.95 | 9.1 s | 9.4 / 9.4 s, 1/1 | 9.4 / 9.4 s, 1/1 | 1.033 (debug_coupe) | 0 / 0 / 0 | track 6.7 |

Reading the tables:
- **With traffic every race is faster and everyone finishes at skill ≥ 0.50** (before: 9 of 20
  race × skill cells had DNFs, Lantern lost 30–90 s per car to junctions; after: one DNF left,
  Lantern skill 0.25). Loop 9 at 0.95: best 214.0 → 205.7 s, mean 233.0 → 215.7 s. Grid
  sprint: 51–60 → 38.4 s best (the rest is a rolling civilian road block, see gaps). Lantern
  at 0.95: 91.9 → 50.1 s best.
- Hollins at 0.25 with traffic got slower (68.8 → 101.6 s best; all four now finish instead
  of three): the beginner field now meets a civilian jam (34 s) the old timing missed. Traffic
  outcomes are chaotic; the solo row is the driving: 67.1 → 66.8 s.
- Solo Lantern is 0.3–2.5 s slower than before: Juno (risk 0.25) stops at a red the old
  timing caught green (`signal` 4–6 s); the driving itself is faster (`usage`/`track`).
- Skill now spreads pace properly. Solo, no traffic, vs the ideal: Loop 9 1.168 / 1.130 /
  1.103 / 1.081, Hollins 1.197 / 1.142 / 1.090 / 1.066, Lantern 1.217 / 1.150 / 1.106 / 1.075
  (skill 0.25 / 0.50 / 0.75 / 0.95). Before, Loop 9 was 1.24 / 1.22 and the 55 m/s cap made
  0.75 / 0.95 crash (DNF), Hollins 1.22 → 1.13 with 6.5 s of grid rejoin in every run.

## Skill targets (brief: skill 95 within ~3–5 % of ideal, clearly beats a casual player; 50 a decent driver; 25 a beginner)

- **Skill 0.95: 6.6–8.1 % over the ideal on the technical courses, ~0 % on the drags** —
  above the 3–5 % asked, against a deliberately strict ideal (perfect tracking at 100 % of
  the *measured* grip, braking and an acceleration model that is optimistic uphill and at
  launch). What is left at 0.95 (solo): `usage` 0.5–1.7 s (the 5 % grip the driver leaves),
  `track` 4.7–6.4 s (exits and climbs below the profile: acceleration model optimism plus
  TCS/ESC), Loop 9 `mode` 6.6 s (the start-discipline lane hold over the first 150 m and
  lane changes), Lantern `signal` 4.3 s (a careful driver at a red). Pushing usage past 95 %
  spun cars on the Hollins hairpins (35 wall hits at 98 %), so 95 % is the stable ceiling with
  today's ESC.
- **Casual player**: not measured (no human telemetry in the repo). The owner's preview
  complaint was against the old AI, which at skill 0.75–0.95 ran 1.24–1.43 × ideal on Loop 9
  with traffic; the new AI at 0.95 runs 1.145 with traffic (1.081 solo), i.e. 8–20 % faster.
- **Skill 0.50** runs 1.13–1.15 × ideal solo (a decent driver), **skill 0.25** 1.17–1.22
  (brakes up to 0.35 s early, 60 % lateral grip at skill 0).

## Lock-in

`apps/simcli/scenarios/opponent/pace_<race>_s{50,95}.ron` (10 files): the race's scenario
with police off; `expect_pending` holds `finishers` (all), `best_ratio` (≤ today's + 0.05,
≥ 0.97) and per-car `race_time_s` (today −15 % … +15 %, +20 % at skill 0.50).
`cargo test --release -p wp_race --test opponent_pace pace_scenarios` races and checks them
(~2.5 min; ignored in debug). simcli runs them standalone (cars on the grid, pending metrics
unknown to it); goldens blessed (`[bless]` commit 0cd5995f).

### Re-lock 2026-10-21 (ws/harness, after `lead/city-residential`)

The lock-in went red when `lead/city-residential` (7955480b) met round 12b: 26 checks out of
range. Its new layout (arterial / highway medians, residential traffic and parked cars, verge
trees) moves every open-road race. Fixed first, all in `wp_opponent`:
1. **Cars beyond the ends of an open course** (a grid behind the start line, a flight route that
   starts at the racer) were clamped onto the end, so they read as level with the racer
   (`Corridor::unclamped_s` / `project_unclamped`, `Racer::s_rel`; harness.md Runs 9c, 9d).
2. **Head-on stand-off with a civilian** (Lantern, the junction at (0, −45)). The racing line
   hugs the centre line, so a civilian coming the other way brushed the racer's lateral margin.
   The racer stopped for it, the civilian stopped for the racer in its path, and both waited for
   the rest of the race. A civilian facing the racer that it meets within 1.5 s now enters the
   side constraints (as cars alongside do): the racer keeps to its side and drives on. Police
   and racers keep the old rules; a box of standing units at Loop 9 is squeezed past as before.
3. **Reverse blocked by the racer behind** (Lantern s95, a civilian half-way through a left turn
   with a racer nose-in to it). The stuck racer's reverse ended at once because the next racer
   stood right behind it, and that one never backed off. A racer whose reverse is cut short now
   asks for room (`Obstacle::wants_room`), and a stopped racer right behind it backs off 1.6 s.

Lantern now finishes 4 of 4 at both skills (s50: 1 of 4 before; s95: 2 of 4). Then every
scenario was re-measured with the same rule (`finishers` all, `best_ratio` ≤ today + 0.05,
`race_time_s` −15 % … +15 %, +20 % at skill 0.50). Today's time per car (s) and best ratio,
before → after:

| pace scenario | best ratio | race time per car |
|---|---|---|
| city_sprint_s50 | 1.08 → 1.01 | ai_coupe 10 → 9, ai_sedan 12 → 11, ai_coupe_2 12 → 10, ai_sedan_2 14 → 12 |
| city_sprint_s95 | 1.00 → 1.00 | ai_coupe 10 → 9, ai_sedan 11 → 11, ai_coupe_2 11 → 10, ai_sedan_2 15 → 15 |
| grid_highway_sprint_s50 | 1.69 → 2.60 | ai_vic 39 → 77, ai_juno 40 → 96, ai_tito 54 → 55, player 48 → 97 |
| grid_highway_sprint_s95 | 1.68 → 2.83 | ai_vic 38 → 76, ai_juno 40 → 108, ai_tito 49 → 60, player 52 → 106 |
| hollins_sprint_s50 | 1.18 → 1.15 | ai_mara 66 → 64, ai_vic 70 → 69, ai_juno 104 → 65, player 114 → 69 |
| hollins_sprint_s95 | 1.08 → 1.07 | ai_mara 60 → 59, ai_vic 63 → 64, ai_juno 67 → 61, player 68 → 64 |
| lantern_checkpoint_s50 | 1.62 → 1.65 | ai_juno 58 → 70, ai_dez 60 → 59, ai_tito 62 → 71, player 63 → 76 |
| lantern_checkpoint_s95 | 1.39 → 1.80 | ai_juno 50 → 67, ai_dez 60 → 64, ai_tito 71 → 110, player 61 → 111 |
| loop9_circuit_s50 | 1.20 → 1.18 | ai_vic 216 → 200, ai_mara 217 → 190, ai_dez 227 → 201, ai_juno 238 → 204, ai_tito 239 → 207, player 225 → 206 |
| loop9_circuit_s95 | 1.15 → 1.13 | ai_vic 206 → 192, ai_mara 207 → 182, ai_dez 212 → 193, ai_juno 234 → 194, ai_tito 214 → 197, player 221 → 196 |

Why each moved:
- **grid_highway_sprint**: roughly twice as slow. The medians leave no opposing lanes, and two
  civilians side by side on both lanes (already reported to D) now hold the field for most of
  the run. The pace sweep at skill 0.95 books 17–44 s per car to `civ` and up to 30 s to
  `veh`, against 0.1–0.4 s `track`. Traffic and layout, not the AI's driving (Known gaps).
- **lantern_checkpoint**: everybody finishes now. s95 `ai_tito` / `player` take about 110 s:
  a 15 s red, a 10 s wait while racer 2 backs out for a civilian turning across it, and 14 s
  behind a civilian standing crosswise in the lane at (189, −152) waiting to turn right.
- **hollins_sprint_s50**: `ai_juno` / `player` were held up when the ranges were recorded
  (104 / 114 s); they are not now (65 / 69 s).
- **loop9_circuit, city_sprint**: 1–8 % faster (round-12b physics and traffic).

## Tests and gate (release unless noted)

- `wp_opponent`: unit, capability table (fingerprints in debug too), `race.rs` full-sim tests,
  open-road sweep — green.
- `wp_race`: unit, picker races (6/6), police race (3/3), Vesper (2/2), simcli races, pace
  lock-in — green. Changed expectations, with reasons: picker Lantern "left the grid" (see
  `opponent.md`); `wp_opponent/tests/race.rs` flee test (see `opponent.md`).
- `wp_wasm` lib: `crash_replay_tests::race_crash_replays_with_the_judge_and_no_crash_on_the_grid`
  and `tests::simcli_golden_replays_play_back_in_the_browser_core` fail identically on the
  baseline (pre-existing, release build).
- Goldens: 6 unrelated goldens drifted from the `lead/round12` tip merge (identical on the
  merged baseline); blessed separately in b911f82e with that reason.

## Known gaps

- Since city-residential the grid highway sprint runs 2.6–2.9 × ideal at the front (civ / veh
  losses, see Re-lock 2026-10-21); the lock-in holds today's times, not a target.
- Traffic still dominates open-road race times: two civilians side by side on both lanes of
  the grid highway (≈ 20 s of every grid-sprint run) cannot be passed legally; world stand-offs
  (the Lantern 4-way junction, a responding police unit angled across a Hollins hairpin with no
  room either side — seeds 3–5 on the round-12 tip, ~40 s) hold racers until they clear.
  Reported to D / E in `opponent.md`.
- The ideal's acceleration model (mean 0–100 km/h acceleration up to 22 m/s, power-limited
  above) is optimistic for launches and climbs; a measured acceleration curve per car in the
  capability table would make `track` losses honest.
- Late-braking / trail-braking passes are not modelled; overtakes still need a run or patience.
- No casual-player reference: record a few owner laps of the picker races (testbed replay) to
  calibrate the skill scale against people.

## Round-12 tip re-measure (2026-10-20, after the police give-way / grid fit / residential merges)

The release-only lock-in (`opponent_pace::pace_scenarios_hold_their_ranges`) was red on
`lead/round12` before this branch's follow-up (identical failures with and without it; a
debug `cargo test` skips it). Causes and what was done:

| Race | Symptom on the tip | Cause | Done |
|---|---|---|---|
| all but Lantern | cars started off today's grid | pace `.ron` files copied the picker grids before the grid fit (Hollins) / median shift (city, grid sprints) / Loop 9 −1.5 m | entity poses copied from today's `race/pick_*.ron` / `city_sprint_police.ron`; 8 goldens `[bless]` |
| Hollins s50/s95 | 2 of 4 finish | right-column car started on the verge, hit it at s ≈ 5 m, field stuck behind | (grid) 4 of 4, 59.8–68.7 s |
| Lantern s50 | 1 of 4 finish | a racer braking behind a junction queue aimed round the corner, drifted into the oncoming lane nose to nose with a stopped civilian; cars behind blocked the reverse | lookahead capped at the stopping distance while slowing for a car / light: 4 of 4 |
| Grid highway | 2.4–3.2 × ideal | a red light at y ≈ −75 on the new grid layout (both lanes held by civilians) | ranges re-measured |
| Hollins picker (police) | default seed never finished | a racer busted on the hairpin stood in the lane (the standing timer skipped racers; busted cars stopped mid-lane; 0.25 throttle could not pull away uphill) | busted / fleeing / retired racers are ordinary cars, pull over to the kerb, creep from rest at 0.45 throttle: `WP_RACE_SEED` 1–8 15 → 19 of 32 finish; new police-free test (4/4) |
| (table) | 3 cars > 2 % | brake-dive merge | only those rows updated; a full regeneration moves every row < 2 % and that alone flips race outcomes |

Re-measured ranges (best_ratio cap = today + 0.05), after merging the auto-clutch fix and the
handbrake pivot aid: city 1.13 / 1.10, grid highway 2.41 / 2.47, Hollins 1.19 / 1.12,
Lantern 1.71 / 1.80, Loop 9 1.23 / 1.18 (s50 / s95). Lantern s95 > s50 is traffic at its
junctions (who reaches a queue first), not skill.

Handbrake pivot aid (A, 89af45cd): `AssistProfile::ai` has `steer_speed_sensitivity` 0, so AI
cars never get it. The browser autopilot drives the player's car with the player's assists,
where it is live: measured, a stopped car on handbrake + full steer turns ~59° in 3 s with
keyboard assists (0° with `ai`). The AI now holds the wheels straight while standing (< 0.3
m/s) on the handbrake. (Straightening below 1.5 m/s cost Lantern s95 3 of 4 finishers: a car
creeping to a stop must keep steering.)

Fleeing cars: a fled racer that has to stop (end of its flight route, no route) pulls over to
the kerb clear of junctions first, like a yielding or busted one; standing still it is a parked
car to the field, which passes it (on the opposing lanes where the corridor has them) after
8 s. `wp_race grid_highway_sprint` and `wp_wasm short_sprint…` (red on lead/assist-levels)
pass on this branch.

Loop 9 with police (report to E / race): every seed 1–6 has one `LeftCorridor` DQ: a racer
yielding to an L1 pursuit at ~45 m/s near (−300, 1386) ends up ~20 m off the corridor after a
contact with the pursuing unit; the judge disqualifies a car pulled over for the police. Grid highway and
Lantern are dominated by the world (a red light, junction queues); the solo pace numbers
above are unchanged in method. Remaining gaps: Hollins with police still loses ~40 % of the
field to police stand-offs (reported to E); a racer that ends up in the opposing lane facing
a standing civilian with racers queued behind it cannot reverse out (Hollins police-free seed
3, 8 m from the flag).


### ws/police 3bccc796 (main sync) — 2026-10-21

Grid highway `short_sprint` (cars 3/4 DNF FinishWindow, "stopped at (10.8, 355.3)"): not the
viaduct. At the first signal (y ≈ −84) a racer sat ~14 s behind a civilian with the next lane
empty: the car in line on its bumper counted as alongside in the hard side constraints (both
sides closed), and the no-three-wide rule refused a pass to the right while a rival sat on the
left. Fixed (a car in line behind bounds neither side; a car alongside only blocks a pass
towards it). Before the fix the two then met the ramp-terminal light at y ≈ 368 as it went red
behind three civilians across all lanes of a divided road (no opposing lane, 1.2 m strip): a
32 s red plus a patrol reversing in the box (two units within 10–20 m, not pursuing). Now all
three AI cars finish by 50 s.

Pace lock-in: only `lantern_checkpoint_s95` moved (best ratio 1.71 → 1.86, already 1.94 on
fc1b388e before the fix above): the whole field waits ~24 s behind a civilian turning at a
give-way junction and then for each crossing car (main's turn-signal / give-way timing).
Re-ranged (today + 0.05, times ±15 %). Taking gaps in cross traffic is the first item of the
street-racing junction risk check.

### Re-lock on ws/police 3bccc796 + ws/harness + lead/round12 — 2026-10-21

Base: this branch merged with ws/police 3bccc796 (main sync), ws/harness 2de604ec / f7abeb95
(oncoming-civilian side constraint, `wants_room`; their re-recorded ranges replaced by this
re-lock) and lead/round12 4367d42e (one-version handling, rear ×0.45). Capability table: rows
added for brigand_400, marten_20, talus_20t, tumbril_20; existing rows unchanged (all within
2 % on the new handling). Rule as before: best ratio ≤ today + 0.05, race times −15 % .. +15 %
(+20 % at skill 0.50).

| scenario | best ratio max | finishers |
|---|---|---|
| city_sprint_s50 | 1.06 | 4 |
| city_sprint_s95 | 1.04 | 4 |
| grid_highway_sprint_s50 | 2.55 | 4 |
| grid_highway_sprint_s95 | 2.05 | 4 |
| hollins_sprint_s50 | 1.19 | 4 |
| hollins_sprint_s95 | 1.12 | 4 |
| lantern_checkpoint_s50 | 2.64 | 4 |
| lantern_checkpoint_s95 | 1.98 | 4 |
| loop9_circuit_s50 | 1.23 | 6 |
| loop9_circuit_s95 | 1.2 | 5 |

Why things moved: grid highway s50 / Lantern are dominated by main's give-way timing at the
Lantern junction s≈270 (whole field waits for a civilian turning and the cross traffic; at
skill 0.50 cars back out / rejoin there 8–18 times). Plain round12 (no main traffic) runs
Lantern s50 in 75–94 s but loses 3 of 4 cars at s95; this tip finishes 4/4 at both. Each of
the boxed-in fix and both harness changes is needed there (removing any one: the whole field
DNFs at s50).

Open: loop9_circuit_s95 — hakone_gt never finishes on the one-version handling (reproduces on
plain round12): at 55–60 m/s it cannot hold its line's lateral g (the 30 m skidpad figure, 0.89
g, over-asks at speed), drifts > 1 m wide, the off-line cap brakes mid-bend and the loose rear
lets go. Needs a speed-dependent lateral capability (next item). Lock-in until then:
`finishers` min 5, no ai_mara time (lead's call, 2026-10-21); restore min 6 with the fix. If the
owner picks rear ×0.55 over ×0.45, re-measure this case on it.

### Loop 9 s95 hakone_gt — fixed (2026-10-21)

Not a capability over-ask: a constant-speed steer-ramp probe (AI assists) shows steady lateral
grip *rising* with speed for the aero cars (hakone_gt 0.90 g at 20 m/s, 1.29 g at 50 m/s;
kestrel / lindwurm flat ~0.94 g). The trace at 10 Hz: a 2 m drift off the line at 60 m/s made
the off-line cap ask for 48 m/s, the speed controller (`2.5 · Δv`, clamp −12) went to full brake,
the car yawed *against* its steering under the full brake, the steering correction built too
slowly and it spun. Fix: slowing for the off-line cap trail-brakes (at most 40 % of the car's
brake decel) instead of braking flat out; every other limit is unchanged. Loop 9 s95: 6/6
finish, hakone_gt fastest (186.0 s, ratio 1.141, no walls); the other races move by under 1 s
(Loop 9 s95 −3–4 s per car). The lock-in for loop9_circuit_s95 is back to finishers 6 with an
ai_mara time.

## Street racing — before numbers (2026-10-21, lead/opponent-pace 03c03fe6)

`WP_PACE_SKILLS=0.5,0.95 cargo test --release -p wp_race --test opponent_pace pace_sweep -- --ignored`
(police off, traffic on). Per race and skill: best ratio, mean opposing-lane time, mean lateral
spread (m), completed overtakes per km summed over the field (racers and civilians), rejoins +
reverses, walls, and time lost behind traffic (civ = civilians, veh = sim vehicles, s summed over
the field).

| race | skill | best ratio | opp % | lat sd | passes/km | rej+rev | walls | civ / veh s |
|---|---|---|---|---|---|---|---|---|
| loop9_circuit | 0.50 | 1.168 | 0 | 1.85 | 0.1 | 0 | 0 | 10 / 110 |
| loop9_circuit | 0.95 | 1.131 | 0 | 1.82 | 0.0 | 0 | 0 | 0 / 106 |
| hollins_sprint | 0.50 | 1.144 | 18 | 0.82 | 0.0 | 0 | 0 | 0 / 32 |
| hollins_sprint | 0.95 | 1.069 | 20 | 0.72 | 0.0 | 0 | 2 | 0 / 24 |
| lantern_checkpoint | 0.50 | 2.592 | 12 | 0.56 | 0.0 | 88 | 0 | 120 / 105 |
| lantern_checkpoint | 0.95 | 1.931 | 10 | 0.41 | 0.0 | 5 | 0 | 54 / 101 |
| grid_highway_sprint | 0.50 | 2.501 | 0 | 1.70 | 7.9 | 0 | 0 | 106 / 73 |
| grid_highway_sprint | 0.95 | 2.001 | 0 | 2.10 | 5.3 | 0 | 0 | 112 / 48 |
| city_sprint | 0.50 | 1.097 | 2 | 0.70 | 0.0 | 0 | 0 | 9 / 17 |
| city_sprint | 0.95 | 1.098 | 2 | 0.73 | 0.0 | 0 | 0 | 9 / 17 |

Reading: the field hardly uses the road it has and almost never completes a pass (≤ 8 /km
summed over the whole field, on the grid highway only). Opposing lanes are used only on Hollins
(hairpins) and where blocked; on the grid highway and Lantern the field loses 30–70 s per car
behind civilians and each other. `passes/km` now counts every completed overtake
(`Field::passes_by`), not only racer position changes.

## Street racing phase 2 — whole-road line (2026-10-21)

Per car width a second line is optimised over the whole road (own lanes + opposing, 0.6 m more
from the far edge) with its own speed profile. A racer in Follow takes it while
`wide_line_clear` finds the opposing side clear out past the next braking zone (oncoming cars
by where they will be), never within 30 m of a junction (side-road traffic is not in view until
it is in the way), and drops it as soon as a car or a light ahead sets its speed. Passes also
check for an opposing-side junction 40 m past the pass.

Seeds 1–8 (32 cars per row) vs the pre-street-racing tip:

| race | skill | finishers | mean s | opposing | walls |
|---|---|---|---|---|---|
| lantern_checkpoint | 0.50 | 30 → 28 | 84.9 → 78.4 | 9 → 9 % | 0 → 0 |
| lantern_checkpoint | 0.95 | 32 → 32 | 78.0 → 74.9 | 10 → 9 % | 0 → 0 |
| hollins_sprint | 0.50 | 32 → 32 | 67.1 → 67.7 | 18 → 41 % | 4 → 14 (taps) |
| hollins_sprint | 0.95 | 32 → 32 | 64.1 → 62.5 | 20 → 41 % | 17 → 8 |

Lantern s50 DNFs are the give-way junction (the same seeds lose cars with the line forced off).
Tried and dropped on the way (each made a seed worse): following the line into a queue (stopped
on the opposing side → gridlock), edge margin on both edges (Hollins −0.5–1.3 s per car), the
line through empty junctions (a civilian turning in met a racer nose to nose). Lock-in:
lantern_checkpoint_s50 re-ranged (faster, rejoins 8–18 → 1–2 per car, no walls).
Next: weaving through traffic and the skill / risk spread.

## Street racing phase 3 — weaving + skill / risk spread (2026-10-21)

- A civilian ahead is gone round as soon as I close on it (no patience timer).
- A car behind in the target lane blocks a pass only if it is closing on me or within 3 m.
- Pass positions: right beside the target (preferred side, then the other), then one and two
  lane widths further out (a car abreast of the target in the next lane no longer ends it).
- `DriverParams::traffic_risk = max(risk, 0.7 · (1 − skill))`: weak drivers use the opposing lanes
  to pass and accept tighter civilian gaps; skilled drivers keep their personality's risk.
  Police choices (running, red lights) keep the profile's `risk`.
- Backing out: a standing out-of-race racer counts like a parked car (race_picker loop9 police
  deadlock).

Seeds 1–8 (32 cars per row), before = cf00e02f (phase 2), after = this tip:

| race | skill | run | finishers | mean s | opp % | passes/km (field, per seed) | walls | rejoin+rev |
|---|---|---|---|---|---|---|---|---|
| lantern_checkpoint | 0.5 | p3_before | 28/32 | 78.4 | 9 | 0.2 | 0 | 80 |
| lantern_checkpoint | 0.5 | p3_after2 | 31/32 | 82.9 | 9 | 0.2 | 0 | 84 |
| lantern_checkpoint | 0.95 | p3_before | 32/32 | 74.9 | 9 | 0.2 | 0 | 54 |
| lantern_checkpoint | 0.95 | p3_after2 | 32/32 | 74.9 | 9 | 0.2 | 0 | 54 |
| hollins_sprint | 0.5 | p3_before | 32/32 | 67.7 | 41 | 0.0 | 14 | 0 |
| hollins_sprint | 0.5 | p3_after2 | 32/32 | 67.7 | 41 | 0.0 | 14 | 0 |
| hollins_sprint | 0.95 | p3_before | 32/32 | 62.5 | 41 | 0.0 | 8 | 8 |
| hollins_sprint | 0.95 | p3_after2 | 32/32 | 62.5 | 41 | 0.0 | 8 | 8 |
| grid_highway_sprint | 0.5 | p3_before | 32/32 | 55.9 | 0 | 4.0 | 0 | 0 |
| grid_highway_sprint | 0.5 | p3_after2 | 32/32 | 54.2 | 0 | 3.3 | 0 | 0 |
| grid_highway_sprint | 0.95 | p3_before | 32/32 | 54.6 | 0 | 4.1 | 0 | 0 |
| grid_highway_sprint | 0.95 | p3_after2 | 32/32 | 52.6 | 0 | 4.3 | 0 | 0 |

Passes/km barely move: on the grid highway nearly every rejected pass is a car abreast in the
next lane or a queue standing at a light inside the pass distance (lane ahead), or a rival
racer alongside on that side (target busy) — the highway is saturated. Real weaving through
dense traffic needs a gap planner (longitudinal gaps per lane over ~100 m) — next candidate,
with the junction-arm look-ahead for cutting empty junctions.

## Street racing — gap planner + junction-arm look-ahead (2026-10-21)

**Junction-arm look-ahead.** The whole-road line may now put a racer on the opposing side within
30 m of a junction when its arms are empty: no civilian within 60 m of it heading to it, none
standing within 25 m, no non-racer vehicle at it. It uses the civilians already gathered near
the racers (`Field::civilians_near`, 160 m) — **no new wp_world query, no wp_contracts change.**
New metric: `Debug::head_on` (standing nose to nose with a standing car in my path); the pace
sweep prints `nose <n> <s>` and `opp-nose <n>` (those out on the opposing lanes) per car.

**Gap planner.** In traffic, in Follow, on a straight (next braking zone > 2.5 s plus the time to
get back to my line away), below 35 m/s and only where my own direction has more than one lane
(over the next 4 s of travel): compare my lanes over 100 m (room = gap to the next car + 3 s of
its speed against mine) and move early into one with 20 m more room than mine, when moving over
is safe (nothing alongside on the way, nothing closing from behind). Tried and narrowed on the
way: on two-lane roads the "other lane" is the opposing one (Lantern opposing stand-offs 1 → 6,
Hollins walls 8 → 15); at circuit speed a weave into a lane that ended a second later, and a
weave ending into a bend 5 m off the line, each lost a car on Loop 9.

Seeds 1–8; before = c1a68c48 (phase 3) with the metric added and the old "never near a
junction" rule; after = this tip (Lantern / Hollins rows measured before the last two gap-planner
restrictions, which only narrow it; Loop 9 / grid rows on the tip).

| race | skill | run | finishers | mean s | opp % | passes/km (field, per seed) | walls | rejoin+rev | nose-to-nose | of which opposing |
|---|---|---|---|---|---|---|---|---|---|---|
| grid_highway_sprint | 0.5 | before | 32/32 | 54.2 | 0 | 3.3 | 0 | 0 | 0 | 0 |
| grid_highway_sprint | 0.5 | after | 32/32 | 52.7 | 0 | 3.5 | 0 | 0 | 0 | 0 |
| grid_highway_sprint | 0.95 | before | 32/32 | 52.6 | 0 | 4.3 | 0 | 0 | 0 | 0 |
| grid_highway_sprint | 0.95 | after | 32/32 | 50.0 | 0 | 4.0 | 0 | 0 | 0 | 0 |
| loop9_circuit | 0.5 | before | 48/48 | 200.4 | 1 | 0.1 | 0 | 0 | 0 | 0 |
| loop9_circuit | 0.5 | after | 48/48 | 200.8 | 1 | 0.1 | 0 | 0 | 0 | 0 |
| loop9_circuit | 0.95 | before | 48/48 | 195.0 | 1 | 0.1 | 0 | 0 | 0 | 0 |
| loop9_circuit | 0.95 | after | 48/48 | 194.7 | 1 | 0.0 | 0 | 0 | 0 | 0 |


Lantern / Hollins (same metric without the opposing split):

| race | skill | run | finishers | mean s | opp % | passes/km (field, per seed) | walls | rejoin+rev | nose-to-nose |
|---|---|---|---|---|---|---|---|---|---|
| lantern_checkpoint | 0.5 | before | 31/32 | 82.9 | 9 | 0.2 | 0 | 84 | 11 |
| lantern_checkpoint | 0.5 | after | 31/32 | 80.3 | 12 | 0.2 | 0 | 61 | 7 |
| lantern_checkpoint | 0.95 | before | 32/32 | 74.9 | 9 | 0.2 | 0 | 54 | 13 |
| lantern_checkpoint | 0.95 | after | 32/32 | 72.8 | 11 | 0.2 | 0 | 58 | 18 |
| hollins_sprint | 0.5 | before | 32/32 | 67.7 | 41 | 0.0 | 14 | 0 | 0 |
| hollins_sprint | 0.5 | after | 32/32 | 67.4 | 45 | 0.0 | 14 | 0 | 0 |
| hollins_sprint | 0.95 | before | 32/32 | 62.5 | 41 | 0.0 | 8 | 8 | 0 |
| hollins_sprint | 0.95 | after | 32/32 | 62.2 | 45 | 0.0 | 8 | 8 | 0 |

Opposing-side stand-offs over seeds 1–8, Lantern only (none elsewhere), measured with the
`opp-nose` metric: before 4, after 4 — the same seeds and cars (s50 seeds 3/6/8, s95 seed 8),
each a brief (≤ 0.6 s) meeting at the give-way junction with a racer whose own line sits ~0.4 m
over the centre line there; none involves the whole-road line or the look-ahead. The total
nose-to-nose count at Lantern s95 rose 13 → 18: all in the racer's own lane (a civilian turning
across the junction box stops facing it; the racer backs out within ~5 s) — more cars reach those
junctions sooner, none is a side-road car meeting a racer on the wrong side. Passes/km:
grid highway 3.3–4.3 per seed for the field, Lantern 0.2 (the planner moves cars before a pass
is needed; completed passes stay rare).

## Main's maps (lead/main-sync bb2208ed) — 2026-10-21

**Divided highways.** On main's grid highway and Loop 9 the corridor has no passing room beyond
my own carriageway anywhere (`left_pass_m == left_m` on every sample; opposing lanes are only the
same road's, and only when my leftmost lane has no median / kerb on its left). So the whole-road
line, opposing-side passes and the gap planner never cross a median or use the far carriageway.
**Ramps / merges.** The gap planner only takes a lane that exists for the next 4 s of travel and
only below 35 m/s; grid highway / Loop 9 results are identical to main-sync (table).

**Lantern downtown (new red).** Two behaviour fixes:
- A civilian stopped in my lane (at a light) is gone round as usual during start discipline, and
  any standing non-racer that holds me 1 s lets me use the opposing lanes (pass checks permitting).
  The junction look-ahead became a gap check (a civilian heading in closes the junction only if
  it arrives within my time to reach and clear it, accelerating as I would, + 1.5 s; standing at
  its own line does not; in the box does; the cars I am passing do not).
- Never stand at a red on the opposing side: no opposing-side pass into a junction whose light
  is red (or turning red) on arrival; out there and still behind with a red coming → tuck back in.
  (Seed 7: the leader stopped for a red mid-pass on the opposing side, nose to nose with the
  oncoming queue, for 2 minutes — no car finished.)

What is left over the old lock is red-light queues with no safe way past. At the first light
(leader, s95): pass rejected 702× on the junction gap (cross traffic arriving in 1.4–5 s against
~4.5 s needed) and 168× on the oncoming queue standing 36 m ahead in the opposing lane.
Lock-in re-ranged for lantern_checkpoint_s50 / s95 by the rule after the fixes.

Seeds 1–8, before = lead/main-sync bb2208ed, after = this tip:

| race | skill | run | finishers | mean s | opp % | passes/km (field, per seed) | walls | rejoin+rev | nose-to-nose | of which opposing |
|---|---|---|---|---|---|---|---|---|---|---|
| lantern_checkpoint | 0.5 | before | 25/32 | 80.3 | 11 | 0.2 | 0 | 188 | 20 | 12 |
| lantern_checkpoint | 0.5 | after | 29/32 | 82.0 | 14 | 0.0 | 0 | 101 | 25 | 5 |
| lantern_checkpoint | 0.95 | before | 25/32 | 85.1 | 10 | 0.0 | 7 | 175 | 20 | 10 |
| lantern_checkpoint | 0.95 | after | 30/32 | 76.1 | 15 | 0.0 | 0 | 52 | 17 | 7 |
| grid_highway_sprint | 0.5 | before | 32/32 | 44.4 | 0 | 2.7 | 0 | 0 | 0 | 0 |
| grid_highway_sprint | 0.5 | after | 32/32 | 44.4 | 0 | 2.7 | 0 | 0 | 0 | 0 |
| grid_highway_sprint | 0.95 | before | 32/32 | 43.9 | 0 | 3.9 | 0 | 0 | 0 | 0 |
| grid_highway_sprint | 0.95 | after | 32/32 | 43.9 | 0 | 3.9 | 0 | 0 | 0 | 0 |
| hollins_sprint | 0.5 | before | 32/32 | 67.0 | 47 | 0.0 | 16 | 0 | 0 | 0 |
| hollins_sprint | 0.5 | after | 32/32 | 67.0 | 47 | 0.0 | 16 | 0 | 0 | 0 |
| hollins_sprint | 0.95 | before | 32/32 | 62.3 | 46 | 0.0 | 8 | 8 | 0 | 0 |
| hollins_sprint | 0.95 | after | 32/32 | 62.4 | 45 | 0.0 | 8 | 8 | 0 | 0 |
| city_sprint | 0.5 | before | 32/32 | 11.0 | 0 | 0.6 | 0 | 0 | 0 | 0 |
| city_sprint | 0.5 | after | 32/32 | 11.0 | 0 | 0.6 | 0 | 0 | 0 | 0 |
| city_sprint | 0.95 | before | 32/32 | 11.0 | 0 | 0.6 | 0 | 0 | 0 | 0 |
| city_sprint | 0.95 | after | 32/32 | 10.9 | 0 | 0.6 | 0 | 0 | 0 | 0 |
| loop9_circuit | 0.5 | before | 48/48 | 205.3 | 0 | 0.1 | 0 | 0 | 0 | 0 |
| loop9_circuit | 0.5 | after | 48/48 | 205.3 | 0 | 0.1 | 0 | 0 | 0 | 0 |
| loop9_circuit | 0.95 | before | 48/48 | 201.1 | 0 | 0.1 | 2 | 1 | 0 | 0 |
| loop9_circuit | 0.95 | after | 48/48 | 201.1 | 0 | 0.1 | 2 | 1 | 0 | 0 |
