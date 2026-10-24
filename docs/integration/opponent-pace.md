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

Capability table regenerated on main's handling (lead's request): only `brake_g` moved, 1–8 %
lower on every car (main's front brake bias); lateral grip, acceleration and top speed unchanged.
Merged lead/round12 66904248 (no table change). Final seeds 1–8 table on this tip (after = with
the new table; the earlier "after" rows above are without it):

| race | skill | run | finishers | mean s | opp % | passes/km (field, per seed) | walls | rejoin+rev | nose-to-nose | of which opposing |
|---|---|---|---|---|---|---|---|---|---|---|
| lantern_checkpoint | 0.5 | before | 25/32 | 80.3 | 11 | 0.2 | 0 | 188 | 20 | 12 |
| lantern_checkpoint | 0.5 | after | 28/32 | 80.3 | 13 | 0.0 | 0 | 72 | 19 | 1 |
| lantern_checkpoint | 0.95 | before | 25/32 | 85.1 | 10 | 0.0 | 7 | 175 | 20 | 10 |
| lantern_checkpoint | 0.95 | after | 30/32 | 75.8 | 14 | 0.2 | 0 | 53 | 14 | 3 |
| grid_highway_sprint | 0.5 | before | 32/32 | 44.4 | 0 | 2.7 | 0 | 0 | 0 | 0 |
| grid_highway_sprint | 0.5 | after | 32/32 | 43.9 | 0 | 2.1 | 0 | 0 | 0 | 0 |
| grid_highway_sprint | 0.95 | before | 32/32 | 43.9 | 0 | 3.9 | 0 | 0 | 0 | 0 |
| grid_highway_sprint | 0.95 | after | 32/32 | 43.9 | 0 | 3.9 | 0 | 0 | 0 | 0 |
| hollins_sprint | 0.5 | before | 32/32 | 67.0 | 47 | 0.0 | 16 | 0 | 0 | 0 |
| hollins_sprint | 0.5 | after | 32/32 | 66.8 | 49 | 0.0 | 0 | 0 | 0 | 0 |
| hollins_sprint | 0.95 | before | 32/32 | 62.3 | 46 | 0.0 | 8 | 8 | 0 | 0 |
| hollins_sprint | 0.95 | after | 32/32 | 62.3 | 45 | 0.0 | 8 | 8 | 0 | 0 |
| city_sprint | 0.5 | before | 32/32 | 11.0 | 0 | 0.6 | 0 | 0 | 0 | 0 |
| city_sprint | 0.5 | after | 32/32 | 11.0 | 0 | 0.6 | 0 | 0 | 0 | 0 |
| city_sprint | 0.95 | before | 32/32 | 11.0 | 0 | 0.6 | 0 | 0 | 0 | 0 |
| city_sprint | 0.95 | after | 32/32 | 10.9 | 0 | 0.6 | 0 | 0 | 0 | 0 |
| loop9_circuit | 0.5 | before | 48/48 | 205.3 | 0 | 0.1 | 0 | 0 | 0 | 0 |
| loop9_circuit | 0.5 | after | 48/48 | 205.0 | 0 | 0.1 | 0 | 0 | 0 | 0 |
| loop9_circuit | 0.95 | before | 48/48 | 201.1 | 0 | 0.1 | 2 | 1 | 0 | 0 |
| loop9_circuit | 0.95 | after | 48/48 | 199.8 | 0 | 0.1 | 0 | 0 | 0 | 0 |

## Racers keep racing at L1–L2 (2026-10-21)

Street racers do not pull over for a single unit mid-race. A racer still in the race ignores the
pull-over at L1, and at L2 unless it is careful (personality `risk` < `RED_LIGHT_RISK`), and
yields from L3 — or once pinned: crawling under 1.5 m/s for 3 s with a chasing unit within 10 m.
Finished / disqualified racers pull over as before; abandoning the race (`flee_wanted_level`)
is unchanged. No police-side change.

race_picker with police, seeds 1–8 (`WP_RACE_SEED`), before = 4cddb03d, after = this tip:

| race | run | cars | finished | captured | fled | other | test passes |
|---|---|---|---|---|---|---|---|
| hollins_hill_sprint | before | 32 | 1 | 27 | 1 | 3 (2 running, 1 window) | 1/8 |
| hollins_hill_sprint | after | 32 | 11 | 21 | 0 | 0 | 7/8 |
| loop9_circuit | before | 48 | 22 | 11 | 13 | 2 (window) | 8/8 |
| loop9_circuit | after | 48 | 35 | 0 | 13 | 0 | 8/8 |
| lantern_checkpoint_draws_the_police | before | 32 | 0 | 20 | 11 | 1 (running) | 7/8 |
| lantern_checkpoint_draws_the_police | after | 32 | 2 | 16 | 13 | 1 (window) | 7/8 |

## Racers never give up (owner, 2026-10-21) — replaces the L1–L2 plan above

In a race a pursued racer never abandons it or pulls over, at any wanted level, whatever its skill
or risk: it keeps racing and evading until the police actually capture it (police-side capture
rules) or it is wrecked / DNF'd. Racers already out of the race (finished, disqualified) still pull
over. `RaceConfig::allow_abandon` now means "may give up" and defaults to off; the flee machinery
stays (its tests turn it on). race_picker::hollins_hill_sprint requires a winner again.

race_picker with police, seeds 1–8, before = lead/round12 e6165f7f (pursued racers yield / flee),
after = this tip:

| race | run | cars | finished | captured | fled | other | test passes |
|---|---|---|---|---|---|---|---|
| hollins_hill_sprint | before | 32 | 5 | 24 | 2 | 1 running | 7/8 (relaxed check) |
| hollins_hill_sprint | after | 32 | 3 | 27 | 0 | 2 running | 3/8 (winner required) |
| loop9_circuit | before | 48 | 23 | 11 | 12 | 2 window | 8/8 |
| loop9_circuit | after | 48 | 48 | 0 | 0 | 0 | 8/8 |
| lantern_checkpoint_draws_the_police | before | 32 | 0 | 18 | 13 | 1 running | 7/8 |
| lantern_checkpoint_draws_the_police | after | 32 | 5 | 27 | 0 | 0 | 8/8 |

Hollins per seed (after): one finisher on seeds 3, 4, 7 (the default seed passes); the police
genuinely capture all four on seeds 1, 2, 6; on seeds 5 and 8 the last racer is still racing
(23 m/s, 69 m from the line on seed 8) when the test's 200 s run limit ends it — held up by box-ins
in the first hairpin earlier, not a deadlock.

## Lane-splitting (2026-10-21)

A racer with no clear side to pass on goes between the target and a car abreast of it
(`plan_split` in `field.rs`) when all of these hold:

- Skill ≥ 0.75 (`SPLIT_MIN_SKILL`).
- The gap is at least my car's width + 0.8 m (`SPLIT_ROOM_M`, from `cap.width_m`; 2.6 m for a 1.8 m car).
- Closing on the slower of the two is ≤ 5 m/s × traffic risk (risk clamped 0.3..1, so 1.5..5 m/s).
- The split stays in my own lanes for the whole pass.
- Neither car is drifting toward the gap (> 0.3 m/s).
- Nothing else is in that slot (ahead, oncoming, standing, or closing from behind).

While splitting:

- The pair gets a fixed 0.25 m clearance (`SPLIT_MARGIN_M`) in the side constraints and the obstacle caps.
- The aim tracks the middle of the gap.
- The pass completes only when I am past both cars.
- I tuck back in while either car is still ahead if it drifts toward the gap or the gap narrows below width + 0.5 m.

Unit test: `skilled_driver_splits_a_wide_gap_closing_gently_and_nobody_else_does`.

Before is 9679494e plus the contact metric; after is this commit. Seeds 1–8, opponent_pace sweep:

| race | skill | run | finishers | mean s | walls | passes/km (field, per seed) | civ contacts | all contacts | splits |
|---|---|---|---|---|---|---|---|---|---|
| grid_highway_sprint | 0.5 | before | 32/32 | 43.9 | 0 | 2.14 | 0 | 0 | 0 |
| grid_highway_sprint | 0.5 | after | 32/32 | 43.9 | 0 | 2.14 | 0 | 0 | 0 |
| grid_highway_sprint | 0.95 | before | 32/32 | 43.9 | 0 | 3.94 | 0 | 0 | 0 |
| grid_highway_sprint | 0.95 | after | 32/32 | 43.9 | 0 | 3.94 | 0 | 0 | 0 |
| lantern_checkpoint | 0.5 | before | 28/32 | 80.3 | 0 | 0.00 | 0 | 0 | 0 |
| lantern_checkpoint | 0.5 | after | 28/32 | 80.3 | 0 | 0.00 | 0 | 0 | 0 |
| lantern_checkpoint | 0.95 | before | 30/32 | 75.8 | 0 | 0.19 | 0 | 0 | 0 |
| lantern_checkpoint | 0.95 | after | 30/32 | 75.8 | 0 | 0.19 | 0 | 0 | 0 |
| loop9_circuit | 0.5 | before | 48/48 | 205.0 | 0 | 0.09 | 0 | 0 | 0 |
| loop9_circuit | 0.5 | after | 48/48 | 205.0 | 0 | 0.09 | 0 | 0 | 0 |
| loop9_circuit | 0.95 | before | 48/48 | 199.8 | 0 | 0.11 | 0 | 0 | 0 |
| loop9_circuit | 0.95 | after | 48/48 | 199.8 | 0 | 0.11 | 0 | 0 | 0 |

**No splits happened on these maps, so nothing moved:** times, passes/km and contacts are
identical, and civilian contacts did not rise. No re-lock is needed.

Seed-1 trace on grid highway s95, 93 candidate ticks:

- **Closing limit:** every candidate was capped at 1.5 m/s. The skilled drivers there have traffic risk ≤ 0.3.
- **Gap:** civilians abreast in adjacent lanes leave 2.4–2.55 m, under the 2.58 m needed.
- **Wide gaps:** the 3.8–4.1 m gaps came with 12–13 m/s closing.
- **With the full 5 m/s:** 13 ticks (one episode, gap 2.6–2.7 m, closing 2.2–3.2 m/s) would have qualified.

Loosening the risk scaling is a lead decision.

### Floor at 0.6 risk (lead, 2026-10-21)

The closing limit's risk now counts as at least 0.6 (`SPLIT_MIN_RISK`), so a skilled, careful
driver closes at up to 3 m/s. The gap rule stays at width + 0.8 m. Split aborts are now counted
(`splitab` in the sweep). Before is 9679494e plus the metric; after is the floor at 0.6. Grid
highway gets 8 extra seeds (9–16):

| race | skill | seeds | run | finishers | mean s | walls | passes/km | civ contacts | all contacts | splits | aborts |
|---|---|---|---|---|---|---|---|---|---|---|---|
| grid_highway_sprint | 0.5 | 1-8 | before | 32/32 | 43.9 | 0 | 2.14 | 0 | 0 | 0 | 0 |
| grid_highway_sprint | 0.5 | 1-8 | after | 32/32 | 43.9 | 0 | 2.14 | 0 | 0 | 0 | 0 |
| grid_highway_sprint | 0.5 | 9-16 | before | 32/32 | 44.6 | 0 | 2.48 | 0 | 0 | 0 | 0 |
| grid_highway_sprint | 0.5 | 9-16 | after | 32/32 | 44.6 | 0 | 2.48 | 0 | 0 | 0 | 0 |
| grid_highway_sprint | 0.95 | 1-8 | before | 32/32 | 43.9 | 0 | 3.94 | 0 | 0 | 0 | 0 |
| grid_highway_sprint | 0.95 | 1-8 | after | 32/32 | 43.9 | 0 | 3.94 | 0 | 0 | 0 | 0 |
| grid_highway_sprint | 0.95 | 9-16 | before | 32/32 | 43.5 | 0 | 2.81 | 0 | 0 | 0 | 0 |
| grid_highway_sprint | 0.95 | 9-16 | after | 32/32 | 43.6 | 0 | 2.81 | 0 | 0 | 0 | 1 |
| lantern_checkpoint | 0.5 | 1-8 | before | 28/32 | 80.3 | 0 | 0.00 | 0 | 0 | 0 | 0 |
| lantern_checkpoint | 0.5 | 1-8 | after | 28/32 | 80.3 | 0 | 0.00 | 0 | 0 | 0 | 0 |
| lantern_checkpoint | 0.95 | 1-8 | before | 30/32 | 75.8 | 0 | 0.19 | 0 | 0 | 0 | 0 |
| lantern_checkpoint | 0.95 | 1-8 | after | 30/32 | 75.8 | 0 | 0.19 | 0 | 0 | 0 | 0 |
| loop9_circuit | 0.5 | 1-8 | before | 48/48 | 205.0 | 0 | 0.09 | 0 | 0 | 0 | 0 |
| loop9_circuit | 0.5 | 1-8 | after | 48/48 | 205.0 | 0 | 0.09 | 0 | 0 | 0 | 0 |
| loop9_circuit | 0.95 | 1-8 | before | 48/48 | 199.8 | 0 | 0.11 | 0 | 0 | 0 | 0 |
| loop9_circuit | 0.95 | 1-8 | after | 48/48 | 199.8 | 0 | 0.11 | 0 | 0 | 0 | 0 |

**Kept by the lead's rule:** across all 16 grid seeds, civilian contacts are 0 before and after,
and walls are 0.

- **Splits:** still none completed on these maps.
- **Aborts:** one split was started and given up (grid s95 seed 15, lindwurm_4t). It tucked back in (one of the pair drifted toward the gap, or the gap closed; the reason is not logged), and its time went from 52.1 s to 55.7 s.
- **Everything else:** identical.
- **Pace ranges:** unchanged.

### Split regression test (2026-10-21)

The test is `skilled_racer_splits_two_civilians_abreast_on_a_three_lane_road`, a closed-loop decision test in `field.rs`:

- **Road:** 3 lanes, 11 m wide, own direction only.
- **Traffic:** two civilians abreast in the outer lanes at 15 m/s, with a 3.0 m gap between them.
- **Racer:** skill 0.9 and risk 0.3, so its closing limit is 3 m/s from the 0.6 floor. It closes at 2.5 m/s, and its motion is simple kinematics.

What it checks:

- The skill-0.9 racer makes exactly 1 split with 0 aborts.
- Its clearance to both civilians stays above 0.2 m; it measured 0.60 m.
- A skill-0.5 racer makes no split.
- The test fails when splitting is switched off.

## Loop 9 late-night traffic (owner row 139, 2026-10-22)

**Owner:** "there should be SOME traffic in the loop9 race".

### Why there was none

- Civilians spawn only within 440 m of a player car.
- At 01:00 with density 0.3, the night curve (0.20 of peak) times the loop's districts' night multipliers (suburb, industrial, port about 0.6) rounds the target near the player down to zero. In a browser-like run the world held 0 civilians for the whole race.
- The pace harness marked every seat `AiRacer`, which spreads traffic over the whole city (about 56 cars, none near the field). So the tests never showed what the owner saw.

### Harness

- `WP_PACE_PLAYER=1` keeps the player seat a Player car (AI-driven), as in the browser.
- `PLAYER_SEAT_RACES` makes this the default for Loop 9 now. All picker races follow in the next commit (lead).
- With no traffic, the player seat drives identically to an AI seat.
- race_picker already raced with seat 0 as a Player car.
- New `WP_PACE_TOD` overrides the time of day.
- New metric `civnear`: civilians within 300 m of each car, in my direction and opposing, sampled at 1 Hz.

### Setting: 23:00, traffic density 2.0

Seed 1, skill 0.95, player seat. `civnear` is the mean number of civilians within 300 m in my direction (a 600 m window).

| time | density | civnear at the player seat | spacing |
|---|---|---|---|
| 01:00 | 0.3 | 0 | none |
| 01:00 | 1.0 | 0.56 | about 1 km |
| 01:00 | 3.0 | 2.2 | about 275 m |
| 23:00 | 1.0 | 0.6 | about 950 m |
| 23:00 | 2.0 | 1.5–1.7 | about 360 m |
| 23:00 | 3.0 | 2.2 | about 275 m |

- The lead asked for 250–300 m spacing (density 2–3).
- At 2.5–3 the field loses 30–55 % and several cars do not finish.
- 2.0 is the densest setting where the field still finishes 95 of 96 runs.

### AI fixes for traffic at speed

Four fixes were made; three are kept:

1. **Blockers use the closing-speed margin.** Braking added up to 0.5 m of clearance (1.0 m for racers) with closing speed, but "who blocks me" used the plain margin. A racer closing at 20 m/s on a civilian 2.6 m to one side braked to its speed from 110 m out and never tried to pass. Where a pass goes keeps the plain margin: using the larger one there cost Lantern s95 seed 8 two cars.
2. **"Never pass a car that is passing" applies only on roads under 10 m.** Ahead of the player seat, cars were always mid-pass, so this refused 254–912 passes a race. On 3+ lanes the lane checks decide.
3. **An overtake held below pace by a car other than its target for 1 s ends and re-plans.** One sat 11 s behind a civilian at 24 m/s with a pace of 67 m/s.

Dropped:

- **"No patience behind a civilian."** It cost Lantern s50 seed 10 two cars.
- **The gap planner at highway speed (4 s look-ahead)**, also tried. With no traffic, racers weaved round each other at 60 m/s: one car did not finish and two hit walls.

The five races (default harness, seeds 1–8), before = 4b3d690e, after = the kept fixes:

| race | skill | seeds | run | finishers | mean s | walls | passes/km | civ contacts | all contacts | splits | aborts |
|---|---|---|---|---|---|---|---|---|---|---|---|
| city_sprint | 0.5 | 1-8 | before | 32/32 | 11.0 | 0 | 0.60 | 0 | 0 | 0 | 0 |
| city_sprint | 0.5 | 1-8 | after | 32/32 | 11.0 | 0 | 0.60 | 0 | 0 | 0 | 0 |
| city_sprint | 0.95 | 1-8 | before | 32/32 | 10.9 | 0 | 0.60 | 0 | 0 | 0 | 0 |
| city_sprint | 0.95 | 1-8 | after | 32/32 | 11.0 | 0 | 0.60 | 0 | 0 | 0 | 0 |
| grid_highway_sprint | 0.5 | 1-8 | before | 32/32 | 42.8 | 0 | 1.91 | 0 | 0 | 0 | 0 |
| grid_highway_sprint | 0.5 | 1-8 | after | 32/32 | 44.1 | 0 | 2.02 | 0 | 0 | 0 | 0 |
| grid_highway_sprint | 0.95 | 1-8 | before | 32/32 | 42.9 | 0 | 3.71 | 0 | 0 | 0 | 0 |
| grid_highway_sprint | 0.95 | 1-8 | after | 32/32 | 42.7 | 0 | 1.80 | 0 | 0 | 0 | 0 |
| hollins_sprint | 0.5 | 1-8 | before | 30/32 | 69.0 | 0 | 0.00 | 0 | 0 | 0 | 0 |
| hollins_sprint | 0.5 | 1-8 | after | 29/32 | 68.0 | 0 | 0.12 | 0 | 0 | 0 | 0 |
| hollins_sprint | 0.95 | 1-8 | before | 30/32 | 63.7 | 8 | 0.25 | 0 | 8 | 0 | 0 |
| hollins_sprint | 0.95 | 1-8 | after | 30/32 | 64.5 | 12 | 0.25 | 0 | 9 | 0 | 0 |
| lantern_checkpoint | 0.5 | 1-8 | before | 31/32 | 83.1 | 10 | 0.19 | 0 | 7 | 0 | 0 |
| lantern_checkpoint | 0.5 | 1-8 | after | 31/32 | 83.8 | 9 | 0.56 | 0 | 4 | 0 | 0 |
| lantern_checkpoint | 0.95 | 1-8 | before | 32/32 | 75.1 | 0 | 0.00 | 0 | 0 | 0 | 0 |
| lantern_checkpoint | 0.95 | 1-8 | after | 32/32 | 76.7 | 0 | 0.56 | 0 | 0 | 0 | 0 |
| loop9_circuit | 0.5 | 1-8 | before | 48/48 | 207.5 | 1 | 0.11 | 0 | 0 | 0 | 0 |
| loop9_circuit | 0.5 | 1-8 | after | 48/48 | 208.6 | 3 | 0.17 | 0 | 1 | 0 | 0 |
| loop9_circuit | 0.95 | 1-8 | before | 48/48 | 203.7 | 0 | 0.21 | 0 | 0 | 0 | 0 |
| loop9_circuit | 0.95 | 1-8 | after | 48/48 | 203.4 | 0 | 0.23 | 0 | 0 | 0 | 0 |
| lantern_checkpoint | 0.5 | 9-16 | before | 32/32 | 79.2 | 0 | 0.00 | 0 | 0 | 0 | 0 |
| lantern_checkpoint | 0.5 | 9-16 | after | 32/32 | 78.3 | 0 | 1.50 | 0 | 0 | 0 | 0 |
| lantern_checkpoint | 0.95 | 9-16 | before | 32/32 | 75.9 | 0 | 0.38 | 0 | 0 | 0 | 0 |
| lantern_checkpoint | 0.95 | 9-16 | after | 32/32 | 75.6 | 0 | 1.31 | 0 | 0 | 0 | 0 |

- Lantern, grid, city and Loop 9 hold.
- Hollins: seed 7 at s50, a known jam, has one more non-finisher (3 vs 2). Seed 6 at s95 has one car with 4 walls. Seeds 1–8 at s95 walls total 12 vs 8.

Loop 9 with the player seat at 23:00, old AI (4b3d690e) vs new AI, seeds 1–8:

| density | skill | AI | finishers | mean s | walls | passes/km | civ contacts | all contacts | civnear at the player seat |
|---|---|---|---|---|---|---|---|---|---|
| 1.0 | 0.5 | old | 48/48 | 219.4 | 0 | 0.46 | 0 | 0 | 0.53 |
| 1.0 | 0.5 | new | 48/48 | 226.5 | 2 | 0.60 | 1 | 0 | 0.61 |
| 1.0 | 0.95 | old | 48/48 | 232.2 | 0 | 0.79 | 2 | 0 | 0.95 |
| 1.0 | 0.95 | new | 48/48 | 224.0 | 3 | 0.80 | 0 | 2 | 0.65 |
| 2.0 | 0.5 | old | 47/48 | 244.9 | 0 | 0.94 | 0 | 0 | 1.70 |
| 2.0 | 0.5 | new | 48/48 | 260.9 | 0 | 1.09 | 1 | 0 | 1.66 |
| 2.0 | 0.95 | old | 42/48 | 236.7 | 2 | 1.14 | 0 | 0 | 1.73 |
| 2.0 | 0.95 | new | 47/48 | 233.5 | 0 | 0.96 | 0 | 0 | 1.45 |

### Loop 9 before (01:00, 0.3, old AI) vs after (23:00, 2.0, new AI)

Player seat, seeds 1–8:

| police | skill | run | finishers | mean s | walls | passes/km | civ contacts | all contacts |
|---|---|---|---|---|---|---|---|---|
| off | 0.5 | before | 48/48 | 202.6 | 0 | 0.00 | 0 | 0  |
| off | 0.5 | after | 48/48 | 260.9 | 0 | 1.09 | 1 | 0  |
| off | 0.95 | before | 48/48 | 196.7 | 0 | 0.00 | 0 | 0  |
| off | 0.95 | after | 47/48 | 233.5 | 0 | 0.96 | 0 | 0  |
| on | 0.5 | before | 8/48 | 191.9 | 1235 | 0.10 | 18 | 208  |
| on | 0.5 | after | 32/48 | 251.5 | 213 | 1.12 | 14 | 47  |
| on | 0.95 | before | 48/48 | 196.7 | 0 | 0.00 | 0 | 0  |
| on | 0.95 | after | 19/48 | 221.7 | 377 | 1.11 | 14 | 76  |

Without police:

- Traffic costs the field 37–58 s a lap. Cars are held behind civilians, and the heavy cars re-accelerate slowly from 25 m/s.
- One car in 96 runs did not finish, with one civilian contact.

With police:

- The police decide the result (owner: captures are fine). Fewer racers take the flag (32/48 and 19/48), and rams and PITs put many cars into walls.
- The old setup's police numbers look odd: s95 is identical to its no-police run, and s50 has 1235 walls. Worth a look by the police workstream.

### Re-lock

- **Pace scenarios:** `pace_loop9_circuit_s50/s95` were re-locked by the rule, with reasons in the RON. Best ratio is now at most 1.27 / 1.21. Times are each car's today −15 % .. +15 % (+20 % at s50), with the s95 slow cars now 265–268 s.
- **race_picker:** `loop9_circuit` (9 pursuits, 6/6 finish) and `loop9_circuit_pace_without_police` pass within their existing windows.

### Known gap: weaving at highway speed

At a 250–300 m spacing the field cannot yet weave through traffic. The gap planner is off above 35 m/s and looks only 100 m ahead. Emergency swerves round a civilian changing lanes at 50 m/s can spin a car (seed 2, s95, density 1.5). This is a follow-up task.

## round12 67dafbfa reds + all races on the player seat (2026-10-22)

### Lantern police test (`lantern_checkpoint_draws_the_police`)

Default seed 31 after E's police, D's traffic and the new steering:

- Two racers end `Dnf(FinishWindow)`. Their police status is `Clear` throughout.
- Both sat about 25 s behind a civilian standing in their lane on the narrow downtown street (t = 50–75 s). They then crawled behind it with no room to pass: 171 and 2231 passes refused, for no room or lane ahead.

Over seeds 1–6 and 31:

- Seeds 1 and 2 end at the flag or in captures.
- Seeds 3, 4 and 6 end in captures.
- Seed 31 has the two finish-window cars.
- Seed 5 never gets judged: the player car is wedged beside two captured racers that stand in the street. This is an AI follow-up, not on the default seed.

The test now allows `FinishWindow`, since racers never give up and a racer held up past the window ends there. It must still end, nobody must wreck, and everybody must leave the grid.

### Pace re-locks

- **Lantern s50, best_ratio 2.27 → 2.44 (2.393):** already moved on 4b3d690e, before the traffic-passing AI fixes. It came with D's ws/world traffic merge; police are off in the pace scenarios, so it is not E's police. Superseded by the player-seat re-lock below.
- **Loop 9 s50, finishers 5..6:** after this merge the back of the skill-0.50 field packs up behind racers held by the 23:00 traffic. sperber_32 was 300 m short when the finish window closed (the density-2.0 sweep: 1 car in 96 runs).
- **The pace check** now skips `race_time_s` for a car that did not finish when the scenario's `finishers` range allows it. The other cars' times are still checked.

### All picker races on the player seat (lead)

The pace harness keeps seat 0 a `VehicleKind::Player` car, driven by the AI, in every picker race. This is the browser setup: traffic spawns round the player car. race_picker already did this. `WP_PACE_PLAYER` and the Loop 9-only list are gone.

| scenario | finishers before / after | best ratio before / after | mean time before / after (s) | walls before / after |
|---|---|---|---|---|
| grid_highway_sprint_s50 | 4/4 / 4/4 | 1.299 / 1.543 | 39.2 / 46.4 | 0 / 0 |
| grid_highway_sprint_s95 | 4/4 / 4/4 | 1.381 / 1.407 | 41.0 / 44.3 | 0 / 0 |
| hollins_sprint_s50 | 4/4 / 4/4 | 1.141 / 1.134 | 67.4 / 66.8 | 0 / 0 |
| hollins_sprint_s95 | 4/4 / 4/4 | 1.075 / 1.071 | 63.6 / 62.4 | 1 / 1 |
| lantern_checkpoint_s50 | 4/4 / 4/4 | 2.393 / 1.484 | 88.1 / 60.5 | 0 / 0 |
| lantern_checkpoint_s95 | 4/4 / 4/4 | 2.248 / 1.311 | 83.8 / 55.2 | 0 / 0 |

- **Lantern is much faster.** Downtown traffic used to spread over the whole city: a seed-8 trace counted about 12 civilians within 300 m of each racer. It is now round the player car, so the field meets less of it.
- **Grid highway is slower.** This is measured; the cause was not traced.
- **Hollins** is unchanged.
- **Loop 9** already ran this way.
- **city_sprint** is not a picker race.

Grid s50/s95 and Lantern s50/s95 were re-locked by the rule, with reasons in each RON. Hollins still holds its ranges.

## Race setup never panics on a route with nothing to drive (row 154 hardening, 2026-10-22)

- **The bug:** `Field::from_route` panics ("route with no length") on a route with no length, or on a one-segment route whose end lies behind its start. In the browser a wasm panic froze the game; branch-triage b6bae7d6 fixed the street-race path in `wp_wasm` street.rs.
- **The same path in wp_race:** `AiEntrants::new`, reached from `LiveRace::new` and the picker's `race_start`.
- **The fix:**
  - New `wp_opponent::Field::try_from_route` returns `None` instead of panicking. `from_route` keeps its panic, documented.
  - `AiEntrants::new` and `LiveRace::new` now return `Result<_, RaceSetupError>`. `RaceSetupError::NoDrivableRoute` refuses the race.
  - `wp_wasm` `race_start` turns the error into its usual `Err(String)` (minimal change outside my crates).
  - Callers in the tests unwrap with "a drivable route".
- **Test:** `entrants::tests::a_route_with_nothing_to_drive_refuses_the_race` covers a backwards one-segment loop route and an empty route; without the fix it panics in `Field::from_route`. A sprint gets a run-off leg past its finish, which can still give the AI a corridor on a degenerate judged route; a loop gets none.

## Queued follow-up (a): reverse-and-replan when boxed in by standing cars

This is the race_picker Lantern police test, seed 5. It is not the default seed (31). Queued by the lead 2026-10-22, behind highway weaving.

**Situation:** the player car (AI-driven) is boxed in on the narrow downtown street by police captures.

- Captured racer 3 stands 0.5 m ahead.
- Captured racer 4 stands alongside, overlapping, 2.3 m to the right.
- The left lane is closed 12 m ahead by parked cars, leaving 1.7 m for a 1.8 m car.

The race is never judged: the player stays `Running`.

**What the AI does:**

- Passes towards racer 4 are refused because it is alongside (`alongside_towards`). It is parked, but it overlaps, so a "parked car behind cannot be cut off" exception does not apply. That exception was tried and reverted.
- The nosed-up reverse fires (`nosed_up`, 3 s). That backs out about 3 m, which is still beside racer 4, so the car creeps back into the same box.

**Proposed fix:** when boxed by standing cars, keep reversing until clear of the one alongside (its rear plus a car length), then plan the pass from there, around the side away from the parked cars. It needs its own police sweep (Lantern, Hollins, Loop 9).

## Physical traffic cars seen once, as traffic + Loop 9 finishers over 8 seeds (2026-10-22)

**The fix.** Every physical traffic car near the player was perceived twice: as a civilian (its world agent) and as a plain `Who::Vehicle` (its sim body at the same spot). The plain copy set the limit with a 0.5 s drift horizon, racer margins and racer headway. A non-racer sim vehicle within 1 m of a Physical-LOD agent is now skipped (`TRAFFIC_BODY_MATCH_M`).

**Sweep, vs 0e84a9f7.**

| case | result |
|---|---|
| no traffic | identical |
| city, Hollins | identical |
| grid, mean | +0.4 / +0.1 s |
| Lantern s50, mean | 80.2 → 74.6 s |
| Lantern s95, finishers | 30 → 31 /32 |
| Loop 9 s50, seeds 1–16 | 91 → 96 /96 |
| Loop 9 s95, seeds 1–16 | 89 → 90 /96 |
| civilian contacts | 0 → 1 in 192 runs |

**Loop 9 finishers check.** At 23:00 with density 2.0, one seed's finishers swing by ±3 cars on millimetre grid differences. The pace RONs (seed 9) went from passing to 3/6 and 4/6, while seed 9 in the sweep finishes 6/6.

- The Loop 9 pace RONs no longer have `finishers`. The times of the cars that finish are still checked.
- The finishers gate is `loop9_traffic_field_finishes`: at least 44 of 48 per skill over seeds 1–8. It is release-only and ignored, takes about 20 min, runs via `just pace-loop9`, and is listed in the lead's main-track gate (docs/LEAD_AUTOMATION.md). Measured: 48/48 at s50, 47/48 at s95.
- grid s50 `best_ratio` was re-locked 1.59 → 1.64 (today 1.593).

**TEMPORARY: `lantern_checkpoint_draws_the_police` runs on seed 1.** With this fix, seed 31 (default) and seed 5 end with the last racer boxed in between captured cars, so the race never finishes and is never judged. The other seeds:

- 1 and 4: everyone finishes.
- 2, 3, 6, 7 and 8: everyone is captured.

Seed 31 comes back with follow-up (a).

**Next, in order (lead):**

1. Follow-up (a): the multi-point reverse-and-turn. Restore seed 31 and check seed 5.
2. A judge safety net: a race in which every remaining racer has made no progress for 3 min ends, and they are DNF Stuck (logged).
3. Weaving v2 aimed at the pack.

## Follow-up (a) done: escape from a box-in by standing cars (2026-10-22)

When a racer is nosed up to a car standing for good (parked, or out of the race and standing) — or has stood still for 3 s behind one with no pass that fits — it picks the side with more room beside that car and keeps working towards it. The racer:

- reverses with the nose swinging to that side;
- creeps forward round the car;
- repeats until past it, giving up after 30 s.

Reversing keeps 1 m to a car standing behind for good (4 m to anything else). The creep logic treats a standing out-of-race car like a parked one. The constants are `ESCAPE_CLEAR_M`, `ESCAPE_AFTER_S`, `ESCAPE_MAX_S` and `REVERSE_STANDING_GAP_M`.

`lantern_checkpoint_draws_the_police` is back on the catalog seed 31.

**Lantern police test, seeds 1–8 and 31:**

- Before: seeds 5 and 31 never finish (a racer stays Running).
- After: all 9 seeds end.

**Sweeps, seeds 1–8, before df9a2cb4 → after.** With police:

| race | skill | seeds | run | finishers | mean s | walls | passes/km | civ contacts | all contacts 
|---|---|---|---|---|---|---|---|---|---
| hollins_sprint | 0.5 | 1-8 | before | 1/32 | 116.8 | 0 | 0.25 | 30 | 0 
| hollins_sprint | 0.5 | 1-8 | after | 0/32 | 0.0 | 0 | 0.12 | 22 | 0 
| hollins_sprint | 0.95 | 1-8 | before | 0/32 | 0.0 | 24 | 1.50 | 54 | 12 
| hollins_sprint | 0.95 | 1-8 | after | 0/32 | 0.0 | 23 | 1.38 | 38 | 12 
| lantern_checkpoint | 0.5 | 1-8 | before | 18/32 | 73.8 | 4 | 2.25 | 24 | 0 
| lantern_checkpoint | 0.5 | 1-8 | after | 18/32 | 71.3 | 2 | 2.44 | 18 | 0 
| lantern_checkpoint | 0.95 | 1-8 | before | 9/32 | 59.5 | 0 | 0.75 | 2 | 0 
| lantern_checkpoint | 0.95 | 1-8 | after | 11/32 | 73.8 | 0 | 0.75 | 4 | 0 
| loop9_circuit | 0.5 | 1-8 | before | 39/48 | 256.1 | 85 | 1.23 | 2 | 15 
| loop9_circuit | 0.5 | 1-8 | after | 39/48 | 256.1 | 85 | 1.23 | 2 | 15 
| loop9_circuit | 0.95 | 1-8 | before | 27/48 | 228.6 | 336 | 1.10 | 13 | 65 
| loop9_circuit | 0.95 | 1-8 | after | 27/48 | 228.6 | 336 | 1.10 | 12 | 65 

Without police:

| race | skill | seeds | run | finishers | mean s | walls | passes/km | civ contacts | all contacts 
|---|---|---|---|---|---|---|---|---|---
| city_sprint | 0.5 | 1-8 | before | 32/32 | 11.0 | 0 | 0.60 | 0 | 0 
| city_sprint | 0.5 | 1-8 | after | 32/32 | 11.0 | 0 | 0.60 | 0 | 0 
| city_sprint | 0.95 | 1-8 | before | 32/32 | 11.0 | 0 | 0.60 | 0 | 0 
| city_sprint | 0.95 | 1-8 | after | 32/32 | 11.0 | 0 | 0.60 | 0 | 0 
| grid_highway_sprint | 0.5 | 1-8 | before | 32/32 | 43.4 | 0 | 0.68 | 0 | 0 
| grid_highway_sprint | 0.5 | 1-8 | after | 32/32 | 43.4 | 0 | 0.68 | 0 | 0 
| grid_highway_sprint | 0.95 | 1-8 | before | 32/32 | 43.5 | 0 | 1.46 | 0 | 0 
| grid_highway_sprint | 0.95 | 1-8 | after | 32/32 | 43.5 | 0 | 1.46 | 0 | 0 
| hollins_sprint | 0.5 | 1-8 | before | 32/32 | 66.8 | 0 | 0.00 | 0 | 0 
| hollins_sprint | 0.5 | 1-8 | after | 32/32 | 66.8 | 0 | 0.00 | 0 | 0 
| hollins_sprint | 0.95 | 1-8 | before | 32/32 | 62.5 | 8 | 0.00 | 0 | 8 
| hollins_sprint | 0.95 | 1-8 | after | 32/32 | 62.5 | 8 | 0.00 | 0 | 8 
| lantern_checkpoint | 0.5 | 1-8 | before | 32/32 | 74.6 | 0 | 0.38 | 1 | 0 
| lantern_checkpoint | 0.5 | 1-8 | after | 32/32 | 72.6 | 0 | 0.38 | 1 | 0 
| lantern_checkpoint | 0.95 | 1-8 | before | 31/32 | 67.3 | 0 | 0.94 | 0 | 0 
| lantern_checkpoint | 0.95 | 1-8 | after | 31/32 | 69.9 | 2 | 0.75 | 0 | 2 
| loop9_circuit | 0.5 | 1-8 | before | 48/48 | 255.0 | 0 | 1.21 | 0 | 0 
| loop9_circuit | 0.5 | 1-8 | after | 48/48 | 255.0 | 0 | 1.21 | 0 | 0 
| loop9_circuit | 0.95 | 1-8 | before | 47/48 | 243.3 | 0 | 1.20 | 0 | 0 
| loop9_circuit | 0.95 | 1-8 | after | 47/48 | 243.3 | 0 | 1.20 | 0 | 0 

- With police, the police decide the result (Hollins: nearly everyone is captured either way). Lantern s95 finishers go 9 → 11, and Lantern s50 civilian contacts go 24 → 18.
- Without police, every race is identical except Lantern: s50 is 2 s faster, and s95 is 2.6 s slower with 2 walls in 32 runs.

## Contract change requests

- **`wp_contracts::DnfReason::Stuck`** (2026-10-22, for the lead's judge safety net (iii)): "made no progress for `stuck_limit_s`". `wp_race::RaceJudge` would end a race once every racer still running has made no course progress for 3 min (generous; logged as a signal), marking them `Dnf(Stuck)`. This is not a racer giving up (owner rule): the judge calls it. Until the variant exists there is no good existing reason: `TimeLimit` and `FinishWindow` mean something else.

## Judge safety net: `Dnf(Stuck)` (2026-10-22)

The lead added `wp_contracts::DnfReason::Stuck` (0.3.1, round12 6e0639e1).

- **The rule:** `RaceJudge::check_limits` ends a race once every racer still running has made no course progress for `STUCK_LIMIT_S` = 180 s. Progress counts in steps of `STUCK_PROGRESS_M` = 5 m, timed from green. Each of them is `Dnf(Stuck)`, and gets a `RacerDnf` signal, which is the log.
- **Scope:**
  - Outrun keeps its own rule.
  - The time limit and the finish window still take precedence.
  - Every kind already has a time limit (420–1800 s), so this only shortens a race that would otherwise hang until it.
- **Not a racer giving up:** the judge calls it.
- **Constants, not rules.toml:** they are constants in `judge.rs`; say if you want them in `data/events/rules.toml`.
- **UI:** the testbed race results show reason `Stuck` as "stuck", lowercase like the other labels ("busted", "too slow").
- **Tests:**
  - `a_race_where_everyone_left_is_stuck_ends_after_the_stuck_limit`: a lone stopped racer ends at 182 s as Stuck; with one racer still crawling, nobody is Stuck.
  - `time_limit_and_finish_window` now uses a crawling racer for the time-limit case. A stopped one is now Stuck.

## Weaving v2, step A: tried, dropped (2026-10-22)

This was "no defensive move with a civilian within 100 m ahead". On Loop 9, 16 seeds, against 9070a6ca:

| skill | finishers before → after | mean time |
|---|---|---|
| s50 | 96 → 95 /96 | 2.8 s faster |
| s95 | 90 → 86 /96 | 4.6 s faster |

That is fewer finishers, so it is not kept.

## Loop 9 s95 slowdown, bisected (2026-10-22, lead)

The s95 re-lock on round12 6e0639e1 (best ratio 1.164 → 1.354, the field 20–55 s slower than
at 9070a6ca) is **not a regression**. It is this one scenario seed meeting less lucky traffic
after ws/world's Loop 9 rest stops.

**Bisect** (`pace_loop9_circuit_s95`, seed 9). Times in s, in the order vic / mara / dez / juno /
tito / player:

| commit | times |
|---|---|
| 9070a6ca (opponent-pace) | 209.2 / 191.8 / 211.5 / DNF / DNF / 272.0 |
| 12a721ca (round12, opponent-pace 0e84a9f7 merged) | 209.3 / 191.8 / 211.7 / 261.9 / 262.0 / 253.6 |
| **6799200b** (ws/world 5558ac04: rest stops, row 153) | 241.8 / 218.3 / 221.9 / DNF / 307.6 / 299.0 |
| e473c597, c81ecd99 | identical to 6799200b |
| b20e07b2 (opponent-pace merge), f5a9cd38, 41a7e401, 6e0639e1 | 263.9 / 222.8 / 231.9 / 265.0 / 266.8 / 251.6 |

The lead's three suspects (c81ecd99, f5a9cd38, 41a7e401) change nothing. The ws/world range in
6799200b is only the rest-stop commits: six more gores on Loop 9, so lane ids and the grid moved
and traffic spawns and merges differently.

**Traffic off** (`WP_PACE_DENSITY=0`, 12a721ca vs today's tip): every car within 1 s (kestrel_ts
196.2 vs 196.1). The rest stops do not change the road the racers drive.

**Seeds 1–8 with traffic** (`pace_sweep`, 12a721ca vs 6799200b, the same AI and harness):

| skill | finishers before / after | mean time before / after (s) |
|---|---|---|
| 0.50 | 49/54 / 54/54 | 253.4 / 240.4 |
| 0.95 | 51/54 / 49/54 | 242.2 / 224.6 |

On average the field is 13–18 s faster with the rest stops. A single seed swings by 30–80 s
either way, both before and after (for example seed 9, s95: vic 205 → 234 s, but dez 216 → 197 s).
Seed 3 at s95 has 4 cars that do not finish; the 8-seed gate (`just pace-loop9`) covers that.

**Decision.** Keep the s95 re-lock; its RON comment now names the cause. Nothing goes to ws/world.
The b20e07b2 step on this seed (the front three 5–22 s slower, but all six finish) is the
traffic-body fix and the box-in escape. They were measured over several seeds when they landed.

## Loop 9 pace lock on seeds 1–8 (2026-10-22, lead)

- **8-seed mean time.** `loop9_traffic_field_finishes` (`just pace-loop9`) now also checks the
  mean time of the cars that finish, per skill, over seeds 1–8 against `LOOP9_MEAN_TIME_S`. The
  range is the pace RON rule applied to the mean: today −15 % .. +15 % (+20 % at s50).
- **Measured on round12 6e0639e1 + 55058973:** s50 48/48 finished, mean 242.0 s; s95 44/48
  finished, mean 220.0 s. Caveat: s95 sits exactly on the 44/48 floor. It was 47/48 on 9070a6ca,
  before the round12 merges, and the rest stops reshuffle the traffic draw per seed.
- **Single-seed RONs widened.** `pace_loop9_circuit_s50/s95` now allow today's times −40 % .. +40 %
  and best_ratio up to today's × 1.4. They only catch gross breakage; the RON comments point at
  the 8-seed check.

## Weaving v2, step 1: no phantom drift from a civilian holding its lane (2026-10-23)

**Cause of most s95 Loop 9 DNFs.** On seed 1 at s95, three cars ran out of the finish window
(`Dnf(FinishWindow)`), each losing 33–74 s behind civilians. A trace showed the cause. A
civilian keeping its lane reads a few tenths of a m/s across the corridor (±0.37 m/s at
27 m/s): the corridor's sampled direction differs from its lane's by about 0.8°. The 3 s
civilian drift look-ahead in `overlaps` turned that into about a metre of drift into the next
lane. A racer one lane over, even at the road edge (n −7.7 against a civilian at −3.6), was
held at the civilian's speed for 50 s, and its overtakes never cleared it.

**Fix.** A civilian whose speed across the corridor is below 2.5 % of its speed along it
(`CIV_DRIFT_DEADBAND`, about 1.4°) gets no drift look-ahead. A fixed 0.6 m/s deadband was
tried first and dropped: it also hid slow city cars pulling across, and Lantern s95 got 17 s
slower on its pace seed. Test: `a_civilian_keeping_its_lane_next_to_mine_does_not_hold_me_up`
fails without the fix.

**Measured on round12 984bc3e4.**

| race | skill | seeds | finishers before → after | mean time before → after (s) | walls |
|---|---|---|---|---|---|
| Loop 9 | 0.50 | 1–16 | 96/96 → 96/96 | 239.1 → 238.7 | 16 → 16 |
| Loop 9 | 0.95 | 1–16 | 90/96 → 95/96 | 229.4 → 228.9 | 18 → 39 |
| Loop 9 (`just pace-loop9`) | 0.50 / 0.95 | 1–8 | 48/48, 44/48 → 48/48, 48/48 | 242.0 / 220.0 → 245.2 / 229.5 | |

City sprint, Grid highway, Hollins and Lantern, seeds 1–8 at s50 and s95: unchanged (Lantern
s50 within 0.1 s). The s95 floor criterion (≥ 46/48 over seeds 1–8) is met: 48/48.

**Caveat: walls.** With the fixed deadband, the extra walls were one car, sperber_32 on seed 13
at s95. A racer hit it (`hit 2`) at about 40 m/s, it spun off to the right (n +16) and never
rejoined: 30 rejoin and reverse cycles until the window closed, 36 wall contacts. The speed-scaled
run has the same total; its per-car split was not checked. This is a rejoin problem off the road
after a spin, not this fix's code. It is a follow-up.

## Stranded AI racers are put back on the course (2026-10-23, lead)

**The case.** On Loop 9 at s95, seed 13, sperber_32 was hit by a racer at about 40 m/s near
s 1490 on the Hollins hill. It spun off the right edge of the embankment and fell about 29 m
down the bank (z 57 → 28). For the rest of the race it drove straight back at the road, stalled
on a slope of about 25° 24 m short of it, and rolled 80 m back down: 30 rejoins.

Its 36 walls were 19 during the crash and 17 bumps at the foot of the bank. Over 16 seeds of
Loop 9 at both skills, every other car had at most 1 rejoin.

**Tried and dropped (local only).** A climb planner: on a stalled rejoin, aim for the road-edge
point (300 m back to 300 m ahead) with the gentlest straight-line climb, from the world's ground
heights. No point along those 600 m was under 0.21 grade; the road sits on a steep embankment
there. The car still made 36 rejoins.

**Fix (lead's option (b)).** The judge's off-course reset (round 12, row 127: `CourseReset` to the
last on-course point) now also covers the AI entrants: `RaceJudge::set_stranded_reset`. An AI
racer outside the corridor (half-width + margin) for `STRANDED_GRACE_S` = 20 s is reset, even
with resets off. Until then it keeps trying to rejoin.

- **Who:** picker races (`LiveRace::new`) list the AI entrants only. The player's car keeps the
  owner's picker-race rule: take any line, never a reset, autopilot or not.
- **Browser:** the session already moves every `CourseReset` with `reset_onto_course` (a clear
  lane spot, `ResetAt` + `Ghost` ops, damage kept). `LiveRace::relocate` now also tells the AI
  (`AiEntrants::relocate` → `Field::relocate`: re-projected within 1.5 km, progress adjusted,
  back to Follow from rest).
- **Harnesses:** the pace and race_picker harnesses place the car on the ground at the reset
  point and ghost it for 1 s.
- **Test:** `a_stranded_ai_racer_is_put_back_after_its_grace_and_the_player_is_not`.

**Measured on round12 984bc3e4 + this branch.**

| | before | after |
|---|---|---|
| Loop 9 s95, 16 seeds: finishers | 95/96 | 96/96 |
| Loop 9 s95, 16 seeds: walls | 39 | 22 |
| Loop 9 s95, 16 seeds: mean time | 228.9 s | 230.0 s |
| Loop 9 s50, 16 seeds | 96/96, 238.7 s, 16 walls | identical |
| most rejoins by one car, Loop 9, 32 races | 30 | 2 |
| `just pace-loop9` | 48/48, 48/48 | 48/48, 48/48 (means 245.2 / 229.5 s) |

Exactly one reset fired in Loop 9 (seed 13, at t = 70.8 s). sperber_32 now finishes in 278.8 s
with 19 walls (the crash) and 1 rejoin. City sprint, Grid highway, Hollins and Lantern over
seeds 1–8: identical, with no resets. Goldens: all 115 unchanged (no reset fires in them),
so there is nothing to bless.

**Open, not this change.** On Hollins s95 seed 3, shiro_turbo has 17 rejoins and lindwurm_4t
has 15, and neither finishes. This was already so before; they stay inside the corridor, so no
reset fires. It is a follow-up.

## Hollins rejoin loops: no run-off past the finish (2026-10-23, lead)

**Cause.** Hollins was the only picker race with no AI run-off. Its route ended at the Knob
Observatory Lot, 8 m before the end of a dead-end lane. `run_off_route` found nothing straight on,
and nothing round a corner within 200 m. wp_opponent parks a finisher in the last 25 m of its
corridor, so the winner stopped on the finish line itself, in the lot entrance where parked cars
with hazard lights stand.

The cars behind stopped 0.5–2 m short of the line, looped through rejoin and reverse cycles
(14–17 each) and were DNF on the finish window. This hit s95 seeds 3 and 12, and s50 seed 12.

**Fix 1: the layout (lead-approved, PLAYTEST_LOG "lead 2026-10-23").** The judged Hollins finish is
now 60 m short of the lot (`CourseSpec::PoiRoute { finish_back_m: 60 }`; the course is 941 m, was
1001 m). The road into the lot is the AI run-off (86 m), as every other race has. The race scenario
file is unchanged (same grid). `length_hint_m` is 941. The Hollins pace RONs are re-locked by the
rule (times about 3–4 s shorter, best_ratio unchanged).

**Fix 2: safety net (wp_opponent, 610f5b0d).** A finisher whose corridor ends at the finish line
(no run-off) rolls on 15 m past the end at up to 5 m/s, towards the kerb, before pulling over. It
does not "rejoin" back to the corridor. Test:
`a_finisher_rolls_on_past_a_corridor_that_ends_at_the_line_before_parking`.

**Tried and dropped.** Giving a finisher a run-off round a corner (no short one exists; a 2.2 km
detour broke the race). Letting a finisher go round anything standing (worse).

**Measured** (Hollins, 16 seeds per skill, against f46ea8be):

| skill | finishers before → after | mean time before → after (s) | walls | most rejoins by one car |
|---|---|---|---|---|
| 0.50 | 61/64 → 64/64 | 66.9 → 64.7 | 0 → 2 | 18 → ≤ 3 |
| 0.95 | 59/64 → 64/64 | 63.4 → 61.2 | 19 → 16 | 17 → 4 |

The times are shorter because the course is 60 m shorter. City sprint, Grid highway, Lantern and
Loop 9 over seeds 1–8 are identical. Goldens: all 115 unchanged (the race scenario file only holds
the grid), so there is nothing to bless. Gate clean.

## Candidate G (2026-10-23/24, lead staging on `lead/opponent-pace`)

The owner's new handling (G: yaw inertia ×1.35, softer ride, lateral grip −0–6 %; frozen by
the physics lead). The retune is in `wp_opponent` / `wp_race` only. This branch also merges
`lead/round12` 81fa22c0 (D's world v7, E's row 157 heat halving) and `lead/pr-ports` dd8b69c8
(row 165: spike strips only with their car).

### AI changes for G (`wp_opponent::field`)
- Steering-lag compensation (`STEER_LAG_S` 0.25 s). Look-ahead stretched 0.3 s over 25 m/s,
  plus 0.6 s over 35 m/s (split variant: of three look-ahead variants over 32 seeds, it
  traded fewest walls for time).
- Edge margins: 0.5 m to the lane edge while driving (`EDGE_MARGIN_M`, was 0.25 m), 0.25 m when
  parking or yielding (`PARK_EDGE_MARGIN_M`), and 0.25 m while passing (`PASS_EDGE_MARGIN_M`).
  Without these, the softer car ran wide into the Hollins kerbs (walls 2 → 124 at s50).
- Civilian drift dead band (0.025 × |v_s|) in `overlaps`: a civilian holding its lane beside
  me does not hold me up.
- Finisher roll-on past a course that ends at the line (`FINISH_CLEAR_M` 15 m at 5 m/s), then
  park. `wp_race` catalogue: Hollins `finish_back_m` 60, so there is no run-off into the
  rejoin loops.
- Stranded AI racer put back after 20 s (`RaceJudge::set_stranded_reset`; AI only, never the
  player) via `Field::relocate`.
- Pull-over (lead, row 165): a car pulling over (yielding, out of the race) that has not
  reached the kerb after 8 s stops short of it once clear of junctions and tight bends; after
  30 s it stops wherever it is. A first cut that stopped it after 8 s anywhere left a racer
  captured mid-hairpin blocking the whole field on Hollins (`hollins_hill_sprint`).
- First look at a point-to-point course: a car within 8 m of the start is placed at the start.
  A street racer's Loop 9 lap from a rest stop back into it had otherwise been projected onto
  its end and finished before it moved (`wp_wasm`
  `night_loop_race_meets_at_a_rest_stop`; it passed on the base only because the projection
  tie went the other way).
- `wp_race::catalog::widen_street_gates`: gates span the corridor plus margins, so a racer
  passing wide still clears its gate (`race_vesper`).

### 16 seeds: pre-G (round12 cae4f20f) vs G retuned

| race | skill | pre-G finishers / mean / walls | G finishers / mean / walls |
|---|---|---|---|
| Hollins | 0.5 | 64/64, 64.7 s, 2 | 64/64, 64.7 s, 41 |
| Hollins | 0.75 | 61.3 s, 32 | 61.8 s, 0 |
| Hollins | 0.95 | 61.2 s, 16 | 59.8 s, 0 |
| Lantern | 0.5 | 64/64, 75.1 s, 0 | 59/64, 72.7 s, 11 |
| Lantern | 0.75 | 64/64, 74.2 s | 64/64, 77.2 s |
| Lantern | 0.95 | 63/64, 74.9 s, 6 | 62/64, 72.0 s, 0 |
| Loop 9 | 0.5 | 96/96, 240.7 s, 5 | 96/96, 235.7 s, 21 |
| Loop 9 | 0.95 | 96/96, 228.9 s, 21 | 95/96, 233.2 s, 5 |

The grid races are +0.5 % / +1.8 %, and the city is unchanged. The Loop 9 s95 DNF is world
geometry (the off-ramp ridge, fixed by D in 9f156be3). On the merged tip, the Loop 9 gate
(seeds 1–8) gives 48/48 at 247.5 s for s50 and 47/48 at 233.5 s for s95.

### Known gap: Lantern s50 pack queueing
Lantern s50 loses about 8 s per car queueing behind racers and civilians. This comes from G
itself (the pass-margin fix recovers part of it). D sees the same pattern on Loop 9 s50 with
the deck-floor fix (ws/world 3affce58: 42/48 on seed 4, cars following one obstacle at a time
on the three-lane carriageways). Next: pass planning for packs on top of main once world v7 is
in. Target: Lantern s50 ≥ 63/64 with no new walls, then weaving v2.

### Re-locks (rule: best ratio ≤ today + 0.05, never under 0.97; times −15 %..+15 %)
- `pace_lantern_checkpoint_s50` / `_s95`: times and best ratio re-locked for G (values and
  reasons in the RONs).
- `pace_hollins_*`: re-locked for the finish move (`finish_back_m`).
- `pace_loop9_*`: widened to ±40 % (traffic chaos; the seeds 1–8 mean-time gate is the
  regression check).
- `sprint_off_route.ron` keyframes 11.2 / 12.6.
- Goldens: one [bless] commit on the final tip (reasons in its message).

## Lantern queueing under G, done (2026-10-24, lead)

- On lead/round13 (D's 3affce58 included), the targets hold:
  - Lantern s50: 64/64, mean 80.2 s, 0 walls (16 seeds).
  - Loop 9 s50: 48/48 on seeds 1–8.
- What's left is time. Per car, Lantern s50 loses about 31 s queued behind racers and 15 s
  behind civilians.
  - `opponent_pace` used to count time behind a racer as "veh": `Field` debug ids racers as
    100 + grid index, not by entity id. Fixed in 59c510fa.
  - Root case, seed 16: a careful leader (traffic risk 0.35) behind a civilian crawling at
    8–9 m/s on a two-lane downtown street, with oncoming traffic, and the field queued
    behind the leader.
- Pass reach (a car ahead counts for a pass up to the follow gap + 8 m, not a flat 30 m):
  no change on Lantern, three cars lost on Loop 9 seed 3. Dropped.
- Crawler patience 4 → 2 s (`CRAWLER_PATIENCE_S`):
  - Lantern s95: 61 → 64/64 finishers, walls 11 → 0.
  - Lantern s50: 80.2 → 80.1 s.
  - Loop 9 s50: identical.
  - Kept.
- `pass_checks` NoRoom counts only the last candidate tried (two lanes out), not why the main
  candidate failed. Read it with that in mind.

## Ashpeak metro circuit: course tuning (2026-10-24, lead)

`race_picker` ashpeak_metro_circuit, police off, finishers per seed 1–4:

| run | seed 1 | seed 2 | seed 3 | seed 4 |
|---|---|---|---|---|
| G tip (cd73812e), traffic 0.3, window 90 s | 3/6 | 4/6 | 5/6 | 4/6 |
| traffic 0.15 | 3/6 | 6/6 | 6/6 | 5/6 |
| traffic 0.15 + finish window 150 s | 6/6 | 6/6 | 6/6 | 6/6 |

- Pre-G main on the catalog seed got 4/6, and its pace sweep 2/6, so the course is marginal
  on its own.
- The losses are civilians, lights and racer trains on the one-lane downtown blocks, over
  two laps: the field spreads out over more than the circuit kind's 90 s.
- Course-side fix, as agreed with the lead:
  - traffic density 0.3 → 0.15 (catalog and `pick_ashpeak_metro_circuit.ron`);
  - a per-race finish window override, `RaceDef::finish_window_s` = 150 s, applied by
    `RaceDef::rules()` in `LiveRace` and in `opponent_pace`.
- Every other race keeps its kind's window (`None`).

## Unafraid of traffic (owner, 2026-10-24)

Owner: "make AI completely unafraid of tagging/hitting pedestrian vehicles in a race — they are
still far too slow and cautious around other traffic and player vehicles. Make it extreme."

- New `RaceConfig::ram_traffic` (default **on**; `wp_opponent::roam` and `wp_wasm`'s
  `roam_config` set it off). Details in the `wp_opponent` README ("Unafraid of traffic").
- While racing, every non-racer (civilian, physical traffic, player, police) is fair game: no
  following gap (brake only down to a `TAG_CLOSING_MPS` = 15 m/s hit), no junction yielding,
  reds run by everyone, no side clearance (except head on), no patience, opposing lanes
  always allowed for passes, 0.1 m clearance.
- Racer–racer racecraft untouched; racers out of the race (fled/busted/retired) are not rammed.
- Full sim, open roads, 25 % traffic, 4 racers, 60 s: 4768 m vs 2672 m careful (+78 %), zero
  racer–racer hard contacts.
- Cross-crate touch: `crates/wp_wasm/src/street.rs` `roam_config` (one field).
- Tuning knob: `TAG_CLOSING_MPS` (lower = softer tags; `f32::INFINITY` = never brakes for traffic).
