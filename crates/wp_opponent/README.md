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
  `civilian_margin_m` at 15 m/s+), crossing traffic extrapolated 3 s, **junction awareness**
  (stay able to stop before a junction box while a civilian is heading into or is inside it).
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
| stops for red lights (open roads) | risk < 0.35 | risk |
| `perception_ahead_m` | 60 → 130 m | awareness |
| `mirror_range_m` | 12 → 45 m | awareness |
| junction watch radius | 25 → 45 m | awareness |

Field-level: `RaceConfig { laps, decision_ticks (4 = 60 Hz), allow_abandon,
civilian_contact_dq, cooldown_speed_mps, seed, line: LineParams, corridor: CorridorOptions,
signals (default on; off for closed courses), draft (default off; on only if the sim applies
slipstream) }`, `AI_CONTACT_IMPULSE_NS = 400` (racer–racer collision threshold),
`YIELD_RADIUS_M`, `YIELD_AFTER_S`, `RED_LIGHT_RISK`.

### Slipstream model (`draft`)
`draft::drag_scales(&[DraftBody], &DraftParams) -> Vec<f32>`: a drag multiplier per vehicle
for the sim loop to apply (`effects.drag_scale *= scale` before `apply_damage_effects`).
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
  abandon and drive ≥ 250 m of a flight route ending > 300 m from the police, no contacts.
- Open roads, 25 % traffic, 90 s: no civilian contact, no racer collision, all progress, nobody
  stuck (< 8 s standing with nothing in the way), no queue longer than 90 s.
- **Live police** (`live_police_chase_ends_in_escape_or_capture_without_stuck_cars`): open
  roads, traffic, the sim's own physical police cars (`sentinel_cruiser`), the AI reading
  `SimPolice`. Racer at L3 runs; racer at L1 never runs (yields → booked, or races on); booked
  cars stay put; no stuck cars; no civilian contact; no AI collision. (Seed 4 today: both
  wanted racers are caught, the clean one races on.)
- Signal unit tests: careful driver stops for a red / short green, a risk-taker doesn't; a
  lasting green ignores cross traffic held at its red but not a car moving inside the box.
- Draft unit tests (wake geometry) and, with `--features sim-draft-hook` (needs
  `Sim::set_drag_scale`, requested from the lead), `draft_lets_a_faster_driver_in_the_same_car_pass`:
  same sedan, a 0.8-skill driver behind three 0.35s, 4 laps, seeds 1–4 — with slipstream it
  gains places in every seed (finishes 1st/2nd/3rd/2nd), without it finishes last every time;
  zero AI contacts.
- Ignored sweep `sweep_open_roads_progress` (8 seeds): reading signals raised total open-road
  progress from 15.9 km to 18.0 km (+13 %), no civilian contact, no AI collision.

Debugging: `WP_TRACE=<grid index>` prints that racer 4× per second (position, speed, speed
target, what limits it, tactic, lateral position / command, gap ahead).
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
- **Same-car passes need the slipstream hook in `wp_sim`** (`Sim::set_drag_scale`, one line;
  patch in `docs/integration/opponent.md`). Until it lands `RaceConfig::draft` must stay off and
  near-equal cars rarely pass. Even with it, two drivers within ~0.1 skill in the same car
  don't catch each other (the pace difference is smaller than the gap). Late-braking passes
  (the other real-world lever) are not implemented.
- Open-road racing: racers wait out long reds (the arterial corner's red is ~45 s) — correct,
  but it caps pace; progress ≈ 0.5–0.8 km per 90 s with 25 % traffic. Rare low-speed nudges in
  jams.
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
