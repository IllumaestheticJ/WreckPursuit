# wp_opponent — AI street racers (workstream 17)

AI racers that race cleanly and believably where contact is expensive: a racing line from the
route geometry, braking points from the car's measured capability, overtakes only with margin,
one defensive move, no ramming, mistakes that grow with pressure, civilians avoided at all
costs, damage and police awareness. **No rubber-banding**: nothing reads race position or the
gap to the player — pace is car capability × how much of it the driver dares to use.

## How it works

| Stage | Module | What |
|---|---|---|
| Corridor | `corridor` | A `wp_world::Route` resampled every 2 m with drivable left/right widths from the lane's same-direction neighbours (parking lanes never), blended through junctions. `OpposingLanes::{Never, ForOvertaking, Always}` decides whether the other direction is usable. |
| Racing line | `line::RacingLine` | Minimum discrete curvature inside the corridor (half car width + 0.4 m from the edges), projected Gauss–Seidel run coarse-to-fine (strides 16→1). Out-in-out falls out. |
| Speed profile | `line::SpeedProfile` | `v ≤ √(a_lat/|κ|)`, forward pass (power, falling off to top speed, friction ellipse), backward pass (braking, friction ellipse). Circuits wrap. |
| Capability | `capability::CarCapability` | Measured once per car with `wp_vehicle::metrics` (skidpad 30 m, 100–0 braking, 0–100, top speed); `damaged()` scales it by `HandlingEffects`. |
| Driver | `profile` | `DriverProfile { skill, aggression, consistency, risk, awareness }` → `DriverParams` (table below). |
| Race | `field::Field` | Per tick: observe (corridor position, laps, damage, heat, civilian DQ) → perceive (racers, other vehicles, civilians as obstacles in corridor coordinates) → decide (tactics, lateral target with hard side constraints, speed target) → track with `wp_world::drive::DriveController` (machine D's pure pursuit + PID, shared with physical traffic). Decisions at 60 Hz. |

### Tactics
- **Follow**: racing line + this braking zone's line noise; time-gap following (gap opened at
  most 3 m/s slower than the car ahead), hard braking limit, deceleration demand when a car
  ahead limits me.
- **Overtake** — only with margin. Needs evidence of being held up (`overtake_patience_s`) or a
  real run (closing > 2.5 m/s). Plans a side (inside of the next corner first) that stays in the
  corridor for the whole pass, is not slower on that line through a corner, is free ahead and
  behind, and never makes it three-wide (no passing a car that is passing; no starting a pass
  while someone is alongside). Aborts when not alongside by the braking point, when side by
  side into the corner longer than `side_by_side_s` while behind, or when oncoming traffic
  appears in the lane (open roads).
- **Slipstream/tow**: held up on a straight → tuck in behind at half the time gap before pulling
  out (positioning only — `wp_vehicle` has no draft model yet).
- **Defend**: one move per straight toward an attacker's side, never into a car alongside,
  never in braking zones or the start phase.
- **Side constraints** (always on): never command a lateral position toward a car that overlaps
  me longitudinally or is arriving from behind within 1.5 s; lateral closing speed adds to the
  clearance; squeezed cars hold their line and the car behind backs out.
- **Yield when losing the wheel**: body slip > ~11° = unstable (lift); drifting > 1.5 m wide of
  the command in a corner = running wide (lift). Both are broadcast: others give such cars 1.5 m
  more room and 1.6× the time gap.
- **Start discipline**: hold the grid lane, no passing, for the first 150 m.
- **Rejoin** after an off (outside the corridor by 1 m or facing > 60° off): 7 m/s cap, waits for
  cars arriving from behind. **Reverse** when stuck against something for 2 s.
- **Civilians**: obstacles with a margin that grows with my speed (0.4 m at a crawl, full
  `civilian_margin_m` at 15 m/s+), crossing traffic extrapolated 3 s, **junction awareness**
  (stay able to stop before a junction box while a civilian is heading into or is inside it).
  Touching a civilian while moving into it = `Disqualified` (the car then drives on at
  cool-down pace — a car parked on the road would gridlock traffic).
- **Damage**: profile rebuilt from `CarCapability::damaged` when effects change by > 5 %;
  overheating caps speed at `top × (1 − 0.35·overheating)`; `steer_pull` compensated; dead
  engine / lost wheel / health < 0.12 → `Retired`, pull to the edge and stop.
- **Police**: `PoliceQuery::wanted_level(entity) ≥ flee_wanted_level` and
  `RaceConfig::allow_abandon` → `Fled`: plans a route to the farthest POI and drives it.
- **Mistakes**: each braking zone samples a braking-point error (seconds of travel) and a line
  error; both scale with `1 + pressure × pressure_sensitivity` (pressure = a car right behind
  or alongside).

## Tuning parameters (`DriverProfile` → `DriverParams`, all linear)

| Param | 0 → 1 | Knob |
|---|---|---|
| `usage.lateral` | 0.68 → 0.93 of measured lateral g | skill |
| `usage.braking` | 0.68 → 0.92 of measured braking | skill |
| `usage.accel` | 0.85 → 1.0 | skill |
| `lookahead_per_mps` | 0.45 → 0.35 s (pure-pursuit, 5–30 m) | skill |
| `throttle_rate_per_s` | 2 → 8 | skill |
| `brake_error_s` (σ) | 0.15 → 0 s (consistency 0.12→0 + skill 0.03→0) | consistency, skill |
| `line_noise_m` (σ) | 0.6 → 0.05 m | consistency |
| `pressure_sensitivity` | 1.5 → 0.2 | consistency |
| `headway_s` | 0.9 → 0.45 s | aggression |
| `overtake_margin_m` | 1.6 → 0.9 m extra beside the car passed | aggression |
| `side_by_side_s` | 0.5 → 3.0 s into a braking zone | aggression |
| `defends` | aggression ≥ 0.3 | aggression |
| `overtake_patience_s` | 2.5 → 0.6 s | aggression |
| `civilian_margin_m` | 1.8 → 1.1 m (at speed) | risk |
| `civilian_headway_s` | 1.6 → 0.9 s | risk |
| `flee_wanted_level` | 1 → 4 | risk |
| uses opposing lanes to pass (open roads) | risk ≥ 0.5 | risk |
| `perception_ahead_m` | 60 → 130 m | awareness |
| `mirror_range_m` | 12 → 45 m | awareness |
| junction watch radius | 25 → 45 m | awareness |

Field-level: `RaceConfig { laps, decision_ticks (4 = 60 Hz), allow_abandon,
civilian_contact_dq, cooldown_speed_mps, seed, line: LineParams, corridor: CorridorOptions }`,
`AI_CONTACT_IMPULSE_NS = 400` (racer–racer collision threshold).

## Using it
```rust
let field = Field::from_route(&world, &route, &specs, RaceConfig { laps: 5, ..Default::default() });
let grid = field.grid_slots(specs.len(), car_len, car_width, GridLayout::TwoWide); // spawn there
// every physics tick:
let inputs = field.tick(sim.world(), &vehicle_obs, &tick_events, &police);
for (entity, input) in inputs { sim.set_input(*entity, *input)?; }
```
`VehicleObs` = entity, `VehicleState`, `HandlingEffects` (`DamageSim::handling_effects`) and
body size for every sim vehicle. Events: `drain_events()` (`LapCompleted`, `Finished`,
`Overtook`… see `OpponentEvent`); `standings()`, `debug_of(k)` (speed target, what limits it,
lateral command, pass-decision counters), `state_hash()`.

## Tests
```sh
cargo test -p wp_opponent                       # unit + full-sim races (~3 min debug, ~12 s release)
cargo test --release -p wp_opponent             # fast
cargo run --release -p wp_opponent --example race -- 5 8 0 0 7   # laps racers traffic reverse seed
WP_SINGLE=1 WP_DUMP=out/ WP_CARS=debug_sedan,lucciola_350 ...   # single-file grid, CSV dump, cars
```
Full-sim tests (`tests/race.rs`, procedural 8×8 city, counter-clockwise arterial square, 1.29 km):
- 8 racers × 5 laps, default aggression, single-file grid: all finish, **zero racer–racer
  contacts ≥ 400 N·s**, best laps in skill order (≤ 0.15 s tolerance), spread 2.5–8 s.
  Measured over seeds 1–8: zero contacts on every seed; best laps strictly in skill order on
  all but the two slowest drivers (0.1 skill apart) on 2 seeds.
- Reverse two-wide grid, 2 laps: no collisions (7/8 seeds clean in the sweep; seed 3 in the test).
- Faster car (sports car) from the back of three sedans: gets past with > 2 m centre distance.
- Same seed ⇒ same sim + field hash; different seed ⇒ different field hash.
- Wanted racer abandons and flees; others keep racing.
- Open roads, 25 % traffic, 90 s: no civilian contact, no racer collision, all progress.
- `determinism_lint`: the workspace lint over this crate.

Scenarios (simcli v2): `apps/simcli/scenarios/opponent/*.ron` — run today with warnings
(simcli has no city loading / `follow_route` yet); their race checks are `expect_pending`.

## Perf (native, release, cloud container 4 vCPU)
| What | Measured | Budget |
|---|---|---|
| `Field::tick`, 8 racers, closed roads | 0.014 ms per 60 Hz frame (0.002 ms/racer) | — (police: 0.5 ms) |
| same, open roads with 25 % traffic | 0.11 ms per frame | — |
| `CarCapability::measure` | ~0.3 s per car (cache it) | load time |
| Whole sim, 8 cars + city + AI | 0.09 ms per physics tick | — |

## Known gaps
- **Passing between near-equal cars is rare**: without a slipstream model a same-car pass
  needs a corner-speed advantage; a faster car passes reliably. Draft physics requested from 02.
- Open-road racing in traffic is safe but slow (racers yield at every busy junction; no signal
  timing; no "run the red when clear" logic). Rare low-speed racer nudges in jams.
- Two-wide grids favour the inside column into the first corner.
- Lap timing here is the AI's own bookkeeping; official judging belongs to `wp_race::judge`
  (adapter not written yet — see `docs/integration/opponent.md`).
- Line optimiser leaves some width unused in tight junction corners.
- Clockwise square on the procedural city: `plan_circuit` detours over the diagonals (corner
  junctions lack that right turn) — tests use counter-clockwise.
