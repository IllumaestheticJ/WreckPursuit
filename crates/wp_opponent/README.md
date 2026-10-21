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
| Speed profile | `line::SpeedProfile` | `v ≤ √(a_lat/|κ|)`, forward pass (`line::drive_accel`: traction-limited to 22 m/s, then power-limited minus drag sized to the top speed; friction ellipse; road grade), backward pass (braking, friction ellipse, grade). Circuits wrap. |
| Capability | `capability::CarCapability` | Measured with `wp_vehicle::metrics` (skidpad 30 m, 100–0 braking, 0–100, top speed); `for_car` reads the pre-measured `capability_table` (every car in `data/cars`, fingerprinted; falls back to `measure`) — use it in race glue, `geometry_only` is for unit tests. `damaged()` scales it by `HandlingEffects`. |
| Driver | `profile` | `DriverProfile { skill, aggression, consistency, risk, awareness }` → `DriverParams` (table below). |
| Race | `field::Field` | Per tick: observe (corridor position, laps, damage, heat, civilian DQ) → perceive (racers, other vehicles, civilians as obstacles in corridor coordinates) → decide (tactics, lateral target with hard side constraints, speed target) → track with `wp_world::drive::DriveController` (machine D's pure pursuit + PID, shared with physical traffic). Decisions at 60 Hz. |

### Driving (playtest row 99, `docs/integration/opponent-pace.md`)
- **Steering**: pure pursuit from the pose 0.2 s ahead (position + velocity, heading + yaw
  rate: response lag), lookahead `lookahead_per_mps` + 0.3 s per m/s above 25 m/s, and a
  curvature feedforward that aims κ·L²/8 inside in tight corners (cars settle wide of a
  pursuit target there).
- **Speed**: P control on the target plus the profile's own slope (`v·dv/ds`) as braking
  feedforward, so braking starts at full strength at the braking point.
- **Assists**: AI racers drive with `AssistProfile::ai()` (ABS, TCS, ESC, no filtering, 1:1
  steering — untouched by the keyboard steering helpers). Measured: ESC off makes them spin
  (Hollins 70 → 115 s, 18 wall hits), TCS off is slower.
- **Rejoin** only when off the *road* (opposing lanes included) or facing > 60° off, never on
  the grid's first 30 m; 7 m/s when misaligned up to 17 m/s when pointing along the road.
- **Recovery**: stuck 2 s → reverse 1.6 s; nose to nose with a standing car in the opposing
  lanes for 2 s → reverse 3 s; reversing is capped at 3 m/s and stops when anything is close
  behind; a reversing racer counts as unstable (others give it room). Finished cars of a
  point-to-point race park 25 m before the corridor ends.

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
  out. With `RaceConfig::draft` (only when the sim loop applies `draft::drag_scales`, see
  below): 0.3 s behind racers outside braking zones (so the corner exit doesn't open a gap the
  wake can't bridge), flat out in the wake down to the minimum gap on straights (the
  braking-distance cap still holds), and a tow-built run (> 0.8 m/s closing within 12 m) or a
  gap under 8 m counts as a pass chance.
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
  `civilian_margin_m` at 15 m/s+ — a car in the next lane is not in my way), crossing traffic
  extrapolated 3 s, passed like anyone (planned 3.5 s ahead at the closing speed, the next
  lane is enough). Physical traffic and police cars are passed with the same margins.
  Anything standing still for 8 s counts as **parked** (passable on the opposing lanes where
  the corridor has them; a car stuck in a traffic deadlock stays put for minutes). A sim
  vehicle (a police unit, another player) still counts as standing while it shuffles within
  2.5 m of where it stopped below 1 m/s, and a parked one is squeezed past at a crawl with
  0.2 m clearance (normal clearance by 15 m/s), 0.15 m nearer the road edge. Perception
  covers the whole road, opposing lanes included. **Junction awareness**: my swept strip
  through the box (my lane band × box length + my car) against each civilian's straight
  course through it; a civilian blocks only if its window in the strip overlaps mine (from
  reaching the box, accelerating, to clearing it) within ±0.6 × `civilian_headway_s`. Cars
  waiting at their own stop line, oncoming cars going straight on (no turn signal) and
  crossings that are over before I arrive no longer hold racers; a car signalling a turn gets
  a lane more of strip.
  Touching a civilian while driving into it (throttle on, moving toward it) = `Disqualified`
  (the car then drives on at cool-down pace — a car parked on the road would gridlock
  traffic). A car shoved into a civilian (by police) is not at fault.
- **Signals** (`RaceConfig::signals`, open roads): the corridor records the lane each junction
  is entered from (`Corridor::junction_lanes`); the field reads `CityMap::lane_signal` for the
  next one. A green lasting until I'm through = cross traffic is held, so only civilians moving
  inside the box count (no more yielding to cars waiting at their red). Red on arrival (or a
  yellow / short green I can stop for): drivers with `risk < RED_LIGHT_RISK` (0.35) stop at
  the line (`SpeedLimit::Signal`); the rest, and anyone fleeing police, go only when nobody is
  heading in. **Stand-off breaker**: after 3 s held by civilians that are standing still in the
  box (each waiting for the other), stop counting them and creep through.
- **Pulled-over cars** (retired, busted, yielding racers; civilians stopped while yielding to a
  siren or with hazards on) are fixed obstacles: anyone may pass them, also in the start phase,
  using the opposing lanes where the corridor allows overtaking there, needing room only until
  past them; nobody backs away from a stopped car. A racer already stopped right behind one
  creeps (≤ 2.5 m/s, stopping 1 m short) while steering out, since a stopped car cannot move
  sideways. Cars told to
  stop hold on the handbrake (the shared controller releases the brake pedal near a standstill
  because a held brake selects reverse; without this they idle forward at ~0.6 m/s).
- **Damage**: profile rebuilt from `CarCapability::damaged` when effects change by > 5 %;
  overheating caps speed at `top × (1 − 0.35·overheating)`; `steer_pull` compensated; dead
  engine / lost wheel / health < 0.12 → `Retired`, pull to the edge and stop.
- **Police** (`PoliceQuery`; use `SimPolice(sim.police())` for live data, `EventHeat` when only
  events are available). Spawn racers as `VehicleKind::AiRacer`.
  - wanted ≥ `flee_wanted_level` and `RaceConfig::allow_abandon` → `Fled`: plans a legal route
    to the POI farthest from the police (`police_positions()`: live ground units + roadblocks
    with `SimPolice`; falls back down the list until a route plans) and drives it with the same
    perception and civilian rules, ignoring red lights. **Re-plans** when police wait within
    35 m of the next 160 m of the route (units ahead only — a chaser behind is beaten by
    driving on; at most every 6 s) or the route is about to end while still wanted.
  - wanted but below the flee level → keeps racing; if a chasing unit stays within
    `YIELD_RADIUS_M` (30 m) for `YIELD_AFTER_S` (2 s) → `Tactic::Yield`: rolls on at ≤ 6 m/s
    to the kerb, clear of junctions, then stops (`Yielded` event); released back into the race
    if the pursuit ends without a capture.
  - `PursuitEnded { Captured | CapturedTotalLoss }` for a racer (from the tick's events) →
    `RacerStatus::Busted`, parked (`Busted` event). A fleeing racer no longer wanted or chased
    → `Escaped` event, cool-down pace to the end of its route, parks.
- **Mistakes**: each braking zone samples a braking-point error (seconds of travel) and a line
  error; both scale with `1 + pressure × pressure_sensitivity` (pressure = a car right behind
  or alongside).

## Tuning parameters (`DriverProfile` → `DriverParams`, all linear)

| Param | 0 → 1 | Knob |
|---|---|---|
| `usage.lateral` | 0.60 → 0.95 of measured lateral g | skill |
| `usage.braking` | 0.55 → 0.95 of measured braking | skill |
| `usage.accel` | 0.80 → 1.0 | skill |
| `early_brake_s` | 0.35 → 0 s earlier than the braking point | skill |
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
| `civilian_margin_m` | 1.0 → 0.6 m (at speed; 0.4 m creeping) | risk |
| `civilian_headway_s` | 1.6 → 0.9 s | risk |
| `flee_wanted_level` | 1 → 4 | risk |
| uses opposing lanes to pass (open roads) | risk ≥ 0.5 | risk |
| stops for red lights (open roads) | risk < 0.35 | risk |
| `perception_ahead_m` | 60 → 130 m, never under 1.3× the braking distance + 30 m | awareness |
| `mirror_range_m` | 12 → 45 m | awareness |
| junction watch radius | 25 → 45 m | awareness |

Field-level: `RaceConfig { laps, decision_ticks (4 = 60 Hz), allow_abandon,
civilian_contact_dq, cooldown_speed_mps, seed, line: LineParams, corridor: CorridorOptions,
signals (default on; off for closed courses), draft (default off; on only if the sim applies
slipstream) }`, `AI_CONTACT_IMPULSE_NS = 400` (racer–racer collision threshold),
`YIELD_RADIUS_M`, `YIELD_AFTER_S`, `RED_LIGHT_RISK`.

### Slipstream model (`draft`)
`draft::drag_scales(&[DraftBody], &DraftParams) -> Vec<f32>`: a drag multiplier per vehicle;
`draft::slipstream(&[VehicleObs], &DraftParams) -> Vec<(EntityId, f32)>` pairs them with
entities for `wp_sim::Sim::set_drag_scale`, which the race loop calls every tick.
35 % less drag bumper to bumper, linear to nothing at 25 m behind the leader's rear bumper,
fading quadratically to the edge of a wake one leader-width wide (+50 % over the range);
leader ≥ 10 m/s, follower travelling within ~25° of it; strongest leader wins, no stacking.
O(n²), deterministic. Physical size of the effect: at 25–35 m/s a sedan (CdA 0.66 m², 1.4 t)
gains 0.06–0.13 m/s² — a few metres per straight; it matters because it removes the gap the
corners open, not as a straight-line rocket.

## Using it
```rust
let field = Field::from_route(&world, &route, &specs, RaceConfig { laps: 5, ..Default::default() });
let grid = field.grid_slots(specs.len(), car_len, car_width, GridLayout::TwoWide); // spawn there
// every physics tick:
let inputs = field.tick(sim.world(), &vehicle_obs, &tick_events, &SimPolice(sim.police()));
for (entity, input) in inputs { sim.set_input(*entity, *input)?; }
```
`VehicleObs` = entity, `VehicleState`, `HandlingEffects` (`DamageSim::handling_effects`) and
body size for every sim vehicle. Events: `drain_events()` (`LapCompleted`, `Finished`,
`Overtook`… see `OpponentEvent`); `standings()`, `debug_of(k)` (speed target, what limits it,
lateral command, pass-decision counters), `state_hash()`.

## Evader bot (`evader`, police benchmark)
`Evader` is a scripted escapee for `simcli police-bench` (`docs/integration/police-bench.md`).
It is not a race opponent: it perceives police cars it can see (line of sight within its sense
range) or hear (close by), acts on what it saw `reaction_s` ago, plans over the lane graph
(Dijkstra out to `foresight_m`, roads near known units cost more, ends far from every unit
score best, bonuses for tunnels / covered roads and turns, seeded noise), keeps the next
max(40 m, 2.5 s) of its route unless a unit is on it, drives it with the shared
`PathFollower` near the car's limit (wet roads: `set_grip_scale`), changes lane round stopped
cars and past slower traffic, reverses out of dead stops and turns round when reversing twice
in one place. Skills (`EvaderSkill::{Casual, Good, Expert}`):

| | reaction s | replan s | foresight m | sense m | lateral m/s² | top m/s | min target m/s | steer noise | overtakes |
|---|---|---|---|---|---|---|---|---|---|
| casual | 1.2 | 3.0 | 300 | 150 | 5.5 | 30 | 16 | 0.05 | no (only round stopped cars) |
| good | 0.6 | 1.5 | 600 | 250 | 6.8 | 40 | 22 | 0.025 | yes |
| expert | 0.25 | 0.75 | 1000 | 350 | 7.8 | 48 | 28 | 0.01 | yes |

It reads only positions it could see — never police assignments or plans.

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
- Open roads + traffic: a wanted-level event read from the sim's event stream makes a racer
  abandon and drive ≥ 250 m in the minute after the call (≥ 150 m when the sim's patrols
  catch it first) on a flight route ending > 300 m from the police, no contacts. The others
  may earn heat of their own at race pace past the live patrols and flee for it.
- Junction gap acceptance unit test: cross traffic arriving together with me holds me; a
  crossing already through, one arriving well after me, and an oncoming car going straight on
  do not; one signalling a turn does.
- `tests/capability_table.rs`: every car in `data/cars` has a table row with a matching
  fingerprint (debug and release); release: rows within 2 % of a fresh `measure`
  (`WP_WRITE_CAPS=1` regenerates).
- Open roads, 25 % traffic, 90 s: no civilian contact, no racer collision, all progress, nobody
  stuck (< 8 s standing with nothing in the way), no queue longer than 90 s.
- **Live police** (`live_police_chase_ends_in_escape_or_capture_without_stuck_cars`): open
  roads, traffic, the sim's own physical police cars (`sentinel_cruiser`), the AI reading
  `SimPolice`. Racer at L3 runs; racer at L1 never runs (yields → booked, or races on); booked
  cars stay put; no stuck cars; no civilian contact; no AI collision. (Seed 4 today: both
  wanted racers are caught, the clean one races on.)
- Signal unit tests: careful driver stops for a red / short green, a risk-taker doesn't; a
  lasting green ignores cross traffic held at its red but not a car moving inside the box.
- Draft unit tests (wake geometry) and `draft_lets_a_faster_driver_in_the_same_car_pass`
  (slipstream applied through `Sim::set_drag_scale`):
  same sedan, a 0.8-skill driver behind three 0.35s, 4 laps, seeds 1–4 — with slipstream it
  gains places in every seed (finishes 1st/2nd/3rd/2nd), without it finishes last every time;
  zero AI contacts.
- Ignored sweep `sweep_open_roads_progress` (8 seeds; `WP_SWEEP_SEED=n` for one, with its
  contacts/DQs): reading signals raised total open-road progress from 15.9 km to 18.0 km
  (+13 %); row 99 (junction gap acceptance, traffic passing, parked-after-8 s) from 16.9 km
  to 26.3 km (+55 %), longest wait 66 → 31 s; no civilian contact, no AI collision, nobody
  stuck in any seed.
- Pace: `crates/wp_race/tests/opponent_pace.rs` (`pace_sweep`, ignored: every picker race ×
  skill vs the car's ideal time with loss attribution; `pace_scenarios_hold_their_ranges`,
  release: the `apps/simcli/scenarios/opponent/pace_*.ron` ranges).

Debugging: `WP_TRACE=<grid index>` prints that racer 4× per second (position, speed, speed
target, what limits it, tactic, lateral position / command, gap ahead).
- `determinism_lint`: the workspace lint over this crate.

Scenarios (simcli v2): `apps/simcli/scenarios/opponent/*.ron` — simcli cannot drive AI
racers, so their race checks are `expect_pending`; `pace_*.ron` are raced and checked by
`wp_race`'s `opponent_pace.rs` (see above).

## Perf (native, release, cloud container 4 vCPU)
| What | Measured | Budget |
|---|---|---|
| `Field::tick`, 8 racers, closed roads | 0.014 ms per 60 Hz frame (0.002 ms/racer) | — (police: 0.5 ms) |
| same, open roads with 25 % traffic | 0.11 ms per frame | — |
| `CarCapability::measure` | ~0.3 s per car (`for_car` reads the table instead: free) | load time |
| Whole sim, 8 cars + city + AI | 0.09 ms per physics tick | — |

## Known gaps
- Slipstream only works where the race loop applies it (`draft::slipstream` →
  `Sim::set_drag_scale` every tick, as `wp_race::AiEntrants::slipstream` does); with
  `RaceConfig::draft` on and no sim hook the AI chases a tow that isn't there. Even with it, two drivers within ~0.1 skill in the same car
  don't catch each other (the pace difference is smaller than the gap). Late-braking passes
  (the other real-world lever) are not implemented.
- Open-road racing: careful drivers (risk < 0.35) wait out long reds; traffic still dominates
  open-road times (see the pace doc: a civilian rolling road block over both lanes of a
  divided road cannot be passed legally). Rolling road blocks and world deadlocks (three
  civilians waiting on each other at an unsignalised junction; a responding police unit
  standing across a hairpin) still hold racers — reported to D / E.
- The "ideal" pace reference uses the profile's own acceleration model (mean 0–100 km/h
  acceleration up to 22 m/s, power-limited above); it is optimistic for slow-revving launches
  (grid sprint: the AI sits ~0 % over it, i.e. the model, not the AI, is the bound there).
- A police car parked with its siren on freezes nearby junctions (civilians within 60 m never
  enter them). Racers now pass the pulled-over queue on the opposing lanes, but cross traffic
  stuck inside the box can still hold them for tens of seconds (reported to D / E).
- Two-wide grids favour the inside column into the first corner.
- Live police: the sim's patrols are effective — in the test seeds most wanted racers are
  caught rather than escaping (they are boxed in at junction queues). Escape is implemented
  (route re-planning, no red lights) but not yet demonstrated in a test seed. A busted car can
  be nudged by the police car that caught it (it holds on the handbrake).
- Lap timing here is the AI's own bookkeeping; official judging belongs to `wp_race::judge`
  (adapter not written yet — see `docs/integration/opponent.md`).
- Line optimiser leaves some width unused in tight junction corners.
- Clockwise square on the procedural city: `plan_circuit` detours over the diagonals (corner
  junctions lack that right turn) — tests use counter-clockwise.
