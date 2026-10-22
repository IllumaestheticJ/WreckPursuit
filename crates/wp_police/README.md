# wp_police

**Owned by workstream 06 (police & pursuit AI).** Data: `data/police/*.toml`.

Heat/bounty, day/night presence and dispatch, patrol and pursuit AI, police tools (roadblocks,
spike strips, helicopter), racer gadgets (radar detector, scanner) and the captain interface.
Pure Rust, no renderer, builds for `wasm32-unknown-unknown`.

## Design pillars
- Chases are **slow, calculated and lethal to both sides**. Pursuers are speed-capped, plan
  corners conservatively and spend a **risk budget**; a cop that drives like an arcade cop wrecks.
- Police win by **containment and attrition**: shadowing, box-ins, PIT at low speed, rolling
  blocks, spikes/roadblocks ahead of the predicted path, a helicopter keeping visual.
- Racers win by **breaking line of sight** for the escape time, reaching a **safehouse**, or
  disabling pursuers (which raises heat and bounty).
- **Day vs night is tables, not code**: every phase-dependent number is a `[x.dawn/day/dusk/night]`
  table in `data/police/`.

## How it works (one tick)
`PoliceSystem::step(world, racers, events, dt, out)` is called once per physics tick by `wp_sim`.

1. **Racers** are synced (position/velocity); spike strip crossings are checked per wheel.
2. **Events**: `TrafficViolation` adds heat (full if a unit/camera *witnessed* it, 20% otherwise
   as a civilian *report*). If no unit saw it (only a camera, or nobody), a report incident opens
   at the violation and the nearest free unit is sent after the report delay; `TotalLoss` of a wanted racer is a capture; `Contact` between a unit
   and a racer marks the unit, civilian–civilian contacts above a speed create crash incidents.
3. **Driving** (every tick): internally driven units integrate with the built-in kinematic driver.
4. **Think** (`think_hz`, default 10 Hz): line of sight → rules (heat decay, escape, capture,
   safehouse) → health check (disengage/pull over/wreck) → patrol pool → incidents →
   reinforcements → helicopter → AI captain tools → tactics and drive commands.

### Heat & wanted levels (`heat.rs`)
- Heat 0–100, levels L1..L5 at `level_thresholds`.
- Decays only when unseen and not pursued, after `decay_grace_s`, at
  `decay_per_s[phase] × decay_district_scale[district]` (faster at night, Suburb, Industrial).
- While pursued and seen: `pursuit_heat_per_s` (long chases escalate). Disabling a cop: `cop_disabled_heat`.
- **Pursuit start**: a wanted racer kept within `recognition_range_m[phase]` with line of sight
  for `recognition_time_s[phase]` (a glimpse at night is not enough).
- **Escape**: no unit, camera or helicopter has line of sight for
  `escape.base_s[level] × escape.phase_scale[phase]`.
- **Safehouse**: inside a safehouse POI for `dwell_s` with no unit within `clear_radius_m`;
  heat is multiplied by `heat_scale_on_enter`.
- **Capture**: speed < `stop_speed_mps` for `stop_time_s` with a unit within `unit_radius_m`, or
  `TotalLoss` while wanted. Heat resets to 0.
- **Bounty** = `per_heat_second × ∫heat/100 dt + per_race_won_wanted × races + per_cop_disabled × cops`.
  `BountyRecord` is persisted by prompt 08 (`bounty_record` / `set_bounty_record`). It sets
  reinforcement priority (up to `priority_max_reduction` faster response). Night stakes: `payout_scale()`.

### Presence & dispatch (`dispatch.rs`)
- Pool units per district per phase from `presence.toml [patrols.*]`. Shortfalls are spawned;
  surplus idle units retire when no racer is within 300 m.
- **Incidents**: civilian reports (dispatched after `report_dispatch_delay_s[phase]`) and crash
  scenes (`report_crash_scene`, or automatically from contacts), which hold a unit for `crash_scene_s[phase]`.
- **Reinforcements**: each pursued racer wants `units_per_level[level]` ground units. A deficit
  creates a request due after `response_s[phase]` (Day 20–40 s, Night 60–150 s). When it comes due,
  an idle unit within 700 m is committed, or a new unit spawns ≥ `spawn_min_distance_m` away, out of sight.
  Backup for a moving racer **waits ahead** (`Assignment::Intercept { ambush: true }`), whether it
  is a committed patrol or a new spawn: it drives in from a side road to a point 25 m past a
  junction on the racer's predicted path (≤ 900 m ahead, ≥ 6 s of the racer's travel away), stops
  there facing the racer's direction of travel, and joins the chase as the racer comes by (no
  turn-around behind it). A new spawn starts on a hidden side-road node; a patrol routes from where
  it is, unless it is on the racer's road itself. Only planned where the padded route time plus 3 s
  beats the racer there; one unit per wait point. It joins only once past the junction and facing
  within ~45° of the racer's line. If the racer stops or leaves the predicted path, it falls back
  to a plain intercept. With no such spot, or a stopped racer, a patrol intercepts as before and a
  spawn appears at the nearest hidden node facing the racer.
- **Helicopter**: `count`, `launch_s`, `unavailable_chance` per phase. It requests from `min_level`.
  It is kinematic, gives line of sight to every unit, and is **not** a weapon.
- Every assignment change raises a radio line; `radio::RadioDirector` paces what is spoken as
  `PoliceEvent::Radio` (tiers, sim-time cooldowns, duplicate collapse, earshot;
  `data/police/radio.toml`). Budget: <= 4 lines/min with nobody wanted, <= 10/min in a pursuit
  (`scenarios/radio_*.ron`).

Assignments: `Patrol`, `Respond(incident)`, `Pursue(target)`, `Intercept(point)`, `RoadblockAt`,
`SpikeAt`, `HeloTrack`, `OutOfService`.

### Pursuit (`pursuit.rs`, `unit.rs`)
- Pursuers follow the racer's **breadcrumb trail**, which is recorded only while the racer is
  seen, extended by its predicted path. Losing sight means losing the scent.
- Speed = `min(max(target × (1 + target_speed_margin), 10), pursuit_cap_mps[phase])`. Corners are
  planned at `pursuit_cornering_g`, or `shadow_cornering_g` once the budget is spent.
- **Risk budget**: time above `risk.lateral_g`, near misses and collisions cost budget. A unit
  with no budget only shadows, at ≤ 1.3× the limit.
- **Tactics**, picked closest pursuer first:
  - `BoxIn`: racer < `boxin_max_kmh` and ≥ 2 budgeted units within 60 m; rear, front-left and front-right slots.
  - `Pit`: racer < `pit_max_kmh[mode]` (55 Sim / 70 Arcade), ≥ `pit_min_straight_m` of straight
    road ahead, unit has budget and is off cooldown.
  - `RollingBlock`: a unit ahead of the racer slows in front of it.
  - `Shadow`: hold 25–40 m back, 12 m more per extra unit; close to 8 m when the racer stops.
  - `Intercept`: units off the trail or far behind take the lane-legal fastest route to the
    racer's predicted position 10–20 s out.
- **Disengage**: health < `disengage_health` (per mode: Arcade 0.15, Simulation 0.4), a bent wheel (`|toe| > bent_wheel_toe_rad`), a flat
  or a lost wheel, or a dead engine → pull over. Total loss at `total_loss_health[mode]`.

**Driving** uses `wp_world::drive::DriveController`, the controller civilian traffic and opponent
racers share. Each unit owns one, created with its `SteerGeometry` (wheelbase, full-lock angle):
- **Physical units** (`bind_unit_entity(unit, entity, geometry)`): `unit_input(unit, dt)` drives
  with the unit's **pro driver** (`pro.rs`, below) on top of that controller.
- **Headless units**: `step_ground` steers with the same controller, then moves a kinematic bicycle
  body. The body keeps right (`LANE_OFFSET_M`), saturates at a grip limit, crashes when it runs
  wide and has a background traffic-hazard model. Track progress only moves forward
  (`Track::project_ahead`), so hairpins and U-turns don't snap back.

Police keep only the speed planning (corner speeds, caps, tactics).

### Pro driving (`pro.rs`, lead P8)
Physical AI units with lights on (pursuit, intercept, response) drive like trained pursuit
drivers: within ~5 % of the best race AI and (almost) never into a wall, dry or wet. Without
lights (patrol, returning) units keep the calm lane driver they always had (pure pursuit on the
lane, `patrol_cornering_g`, `unit.brake_mps2`). `ProDriver::drive(ctx, track, room, dt)`:
- **Racing line** (lights on, no other vehicle within `traffic_clear_m` ahead): the next
  `window_m` of the track, resampled every `step_m`, its polyline corners rounded
  (`corner_round_passes`), then a lateral offset per sample minimising the summed squared
  curvature (projected Gauss–Seidel, strides 4 → 1) inside the **drivable room**. The window
  starts at the car's own lateral position (no jump on a re-plan, every `replan_s`). While
  passing an obstacle, with traffic ahead or while turning round the unit tracks its lane
  (traffic look-ahead) and only the speed profile is the pro driver's.
- **Room** (`RoadRoom`, `RoomQuery`): built from the city's paved surfaces and lanes
  (`RoadRoom::from_city`; the contract road network's polylines follow a lane, so they are
  not used for widths): own-direction lanes (bus lanes yes, parking no) plus `opposing_m`
  over the centreline, and the whole box of a junction (stop line to stop line). Kept in a
  25 m grid. `RoadRoom::new(&RoadGraph)` is the fallback for hand-built graphs.
- **Speed profile at the grip limit**: `v ≤ √(a_lat/|κ|)` on the line, then a backward pass
  braking inside the friction ellipse. `a_lat = lat_g × lat_usage`, braking
  `brake_g × brake_usage`, both × the **road grip** the sim reports (`sync_unit_feel`: wet,
  cold, paint, aquaplaning via `wp_vehicle::Vehicle::conditions_grip`) × damage grip ×
  **confidence** (drops `confidence_drop` per slide or run-wide, floor `confidence_min`,
  regains `confidence_regain_per_s`). Shadowing (risk budget spent) keeps
  `shadow_cornering_g` as a cap; a pursuit or a response uses the limit. Obstacle and junction
  caps (`avoid`, e.g. `siren_junction_mps`) still apply on top.
- **Late braking**: the pedal command adds the profile's own deceleration a few metres ahead
  (feed-forward), read `lead_s` ahead in time.
- **Slide control** (above 5 m/s; body slip less the rolling slip `½·wheelbase·yaw/v`, which
  is large in a tight turn at a crawl): slip past `slip_deadband_rad` is countersteered
  (`countersteer_gain`, front wheels toward the velocity); past `slide_slip_rad` the driver
  lifts to `slide_throttle`. On the way out of a corner the throttle is capped by the friction
  ellipse from the measured lateral load (`exit_throttle_min` at full load), so the rear stays
  planted. Running `run_wide_m` outside the line in a corner lifts too.
- **Recovery**: facing away from the lane ahead → turn round at `turn_around_mps`; stuck
  (throttle on, not moving for `stuck_s`) → reverse at opposite lock until the nose points
  at the lane (`reverse_min_s`..`reverse_s`).
- **Car limits**: `set_unit_limits(unit, CarLimits)` (`wp_sim::Sim::set_police_car_limits`);
  default `[pro] default_limits` = the Sentinel Cruiser measured with `wp_vehicle::metrics`.

Measured (`cargo test --release -p wp_opponent --test police_pace -- --nocapture`; Sentinel
Cruiser, same car and measured capability for both, best race-AI profile = skill 1, both in
their own-direction lanes, full `wp_sim` loop):

| Course | Race AI | Police (pro) | Wall hits | Max body slip |
|---|---|---|---|---|
| Arterial square, best flying lap | 53.30 s | 52.73 s (−1.1 %) | 0 | 0.044 rad |
| Hollins Drive hill sprint (hairpins, 11 %) | 59.70 s | 61.08 s (+2.3 %) | 0 | 0.049 rad |
| Arterial square, fully wet (0.5 mm) | — | 59.22 s (+12.3 % vs dry) | 0 | 0.039 rad |
| Hollins Drive, fully wet | — | 71.78 s (+17.5 % vs dry) | 0 | 0.033 rad |

The pre-P8 driver (pure pursuit on the lane, corners planned at `pursuit_cornering_g` 0.65 g,
braking at 6.5 m/s², emulated in the same test): arterial square 60.51 s (+13.5 %), Hollins
Drive not finished (6 hits, stuck at a hairpin).

In the city with traffic (`cargo run --release -p simcli --example police_traffic_contacts --
pursuit 900`, 12×12 city at noon, now also counting police↔world hits), per 5 min of pursuit,
lights on: wall hits 5.7 (`main` before P8) → 2.1; traffic contacts 0.3 → 2.4, of which 7 of 8
are one low-speed stand-off (0.3–0.6 m/s, a held unit and a traffic car at a junction) counted
every few seconds. Patrol (lights off) is unchanged by construction (`patrol 300`: 3 wall hits,
as before).

### Obstacle awareness (`avoid.rs`)
Every tick each ground unit (physical or headless) looks ahead on its own track:
- **Lane corridor**: any vehicle whose box overlaps the unit's lane (unit half width + its
  projected half extent + `lateral_margin_m`) within a speed-dependent horizon
  (`min_horizon_m`..`max_horizon_m`). The nearest one caps the speed so the unit can stop
  `standstill_gap_m` behind it (braking at `decel_mps2` after `reaction_s`); a moving leader is
  followed at its own speed. A **bumper box** (`bumper_m` straight ahead of the body) catches
  what the corridor misses while the body is off its track (turning, cutting corners).
- **Hold**: at a zero cap, physical units get full brake (the shared controller would otherwise
  release it below its hold speed and creep).
- **Passing**: behind a *stopped* vehicle closer than `pass_gap_m` for `pass_wait_s` (6 s;
  `pass_wait_siren_s` 1.5 s with lights), the unit pulls `pass_shift_m` left (or
  `pass_extra_shift_m` further, for a car straddling the centreline) when that lane is clear past
  the blocker and free of oncoming vehicles for `oncoming_clear_m`. It passes at ≤
  `pass_speed_mps` and returns to its lane after the blocker. With lights a vehicle creeping
  slower than `pass_crawl_mps` (a queue at a light) counts as stopped.
- **Passing moving traffic** (lights on): a vehicle going the unit's way at ≤
  `pass_moving_max_mps`, at least `pass_moving_margin_mps` slower than the unit wants, within
  `pass_moving_gap_m`, not a police unit and not the target, is passed at its speed +
  `pass_moving_dv_mps`. The pass length follows from that closing speed; the other lane must be
  clear for it (oncoming cars counted at 15 m/s over the pass) and it must end
  `pass_moving_junction_m` before the next junction. The unit rejoins only once the passed car
  is clear behind (out at most `pass_moving_max_m`). In the chase report (12 seeds, race-AI
  racer fleeing through traffic) this took captures from 11/12 to 12/12, mean pursuit 62 → 56 s,
  nearest pursuer 43 → 37 m; police↔traffic incidents with lights on unchanged
  (`police_traffic_contacts -- pursuit 900`: 1 contact, 6 overlaps, 7 wall hits, as before).
- **Exceptions**: a unit in a `Pit` or `BoxIn` tactic ignores its own target (contact is the point).
  No unit ever overtakes its own target (a stopped target is an arrest).
- **Traffic rules** (next junction on the track, refreshed at think rate; light from
  `WorldQuery::signal_state_ahead`):
  - *No lights (patrol)*: stop `stop_line_back_m` before a red, and before a yellow when there is
    room; slow to `yield_mps` through unsignalised junctions and **give way** to cross traffic that
    gets there within `yield_gap_s` of the unit (or is in the junction box), waiting at most
    `yield_max_s`; turns at `turn_mps`; no overtaking within `no_pass_near_junction_m` of a
    junction (red-light queues).
  - *Lights on*: may run reds and exceed limits, but crosses junctions at `siren_junction_mps`
    (`siren_red_mps` against a red, giving way to cross traffic for up to `yield_max_siren_s`) and
    turns at `siren_turn_mps`. A unit in an active pursuit (`Pursue`) goes straight through a
    green or unsignalised junction at `pursuit_junction_mps` (20 m/s) instead; reds and turns
    are capped as above.
- **Holding still**: at a zero cap, and when parked (tool units, no track), a physical unit holds
  brake 0.4 plus the handbrake. Full pedal at a standstill would engage `wp_vehicle`'s
  automatic reverse.
- **Gridlock**: a unit stopped behind another stopped police unit for `gridlock_s` creeps on at
  `pass_creep_mps` past stopped higher-id units that cross or face it (they keep waiting).
- **Spawning**: pool units appear in the right-hand lane, ≥ `spawn_clear_m` from every vehicle
  and outside every moving vehicle's next `spawn_path_s` of travel, already facing their first
  patrol route (a physical car turning around from standstill runs over the kerb). Reinforcement
  nodes use the same clearance.

Obstacles: `wp_sim` reports every vehicle (players, racers, physical and kinematic traffic,
police cars) with `set_obstacles` each tick. Headless, units see the racers and each other.

### Tools & gadgets (`tools.rs`)
- `plan_site` puts a tool on the predicted path between `min_ahead_m` and `max_ahead_m`, never
  within 15 m of an intersection, and with approach sight distance ≥ `min_sight_m` (120 m).
  Sight distance is the minimum over **every** legal approach, so a site just after a turn-in
  point is a blind corner.
- **Roadblock**: 2–3 parked units plus barrier `StaticCollider`s with mass. Never on a highway
  when the level is above `highway_max_level`.
- **Spike strip**: a line collider. Wheels that cross it deflate linearly over `deflate_s` (5 s)
  → `tire_puncture(entity)` for `HandlingEffects::tire_puncture`.
- **Radar detector** `radar(pos, tier)`: units in Patrol/Pursue state within `radar_range_m[tier]`.
- **Scanner** `scanner(tier, since: Option<SimTime>)`: radio lines delayed by `scanner_delay_s[tier]` (10/6/3 s).

### Captain (`captain.rs`)
`CaptainCommand` (assign unit, roadblock, spikes, helicopter, priority target, call off) is
validated against unlocks (`presence.toml [unlocks]`), cooldowns, budgets and the placement
rules, and returns `CaptainError` on refusal. Use `to_bytes`/`from_bytes` for the payload of
`ClientToServer::CaptainCommand`. `captain_view()` returns units, incidents, heat by district,
racers' last-known positions with staleness, tools and readiness. Without a human captain
(`set_human_captain(false)`), the dispatcher places tools and requests the helicopter itself.
Player chase units get `unit_order()` **suggestions** only.

## Tuning tables (`data/police/`)
| File | Contents |
|---|---|
| `heat.toml` | Level thresholds; violation weights; witness, recognition, decay and escape tables per phase; capture, safehouse and bounty rules |
| `presence.toml` | Patrols per district × phase, reinforcement response times, report delays, helicopter availability, tool/tactic unlock levels, crash scenes |
| `pursuit.toml` | Think rate, speed factors and caps, cornering g, LOS range, unit dynamics and crash severity per mode, risk budget, traffic hazards, tactic parameters |
| `tools.toml` | Roadblock, spike strip and helicopter parameters, radar/scanner tiers, captain hold time |

The tables are compiled in (`PoliceConfig::builtin()`) and can be loaded from disk with
`PoliceConfig::load(dir)`. A test checks that both copies match.

## Roads
Roads come from the contract road network (`wp_contracts::RoadNetworkQuery`, v0.3.0; D's
`wp_world::RoadNetworkIndex`). Build police with `PoliceSystem::for_world(cfg, &world, setup)`, or
with `RoadGraph::from_network(&dyn RoadNetworkQuery, &signalised_nodes)` / `from_city(&CityMap)`
(`city.rs`). `RoadGraph` is the police routing index over that network, not a second road model.
The mapping is 1:1, with one segment (or a chain of them on curved roads) per edge:
- **Ids:** node ids are kept. `RoadSegment::edge` remembers D's edge id, and captains name roads
  by it (`RoadEdgeId`).
- **Classes:** Collector/Local/Alley → Local.
- **Districts:** Core/Commercial → Core, Residential/Suburb → Suburb, Industrial/Port → Industrial.
- **Line of sight:** `WorldQuery::line_of_sight`, which uses D's building footprints.

`RoadGraph` is only the police index on top: legal routing, predicted paths, approach sight
distance and nearest road. Tests, scenarios and tools use `city::procedural(&ProcGenConfig)`.

## Measured behaviour (`crates/wp_police/scenarios`, D's `ProcGenConfig::small()` city)
| Scenario | Result |
|---|---|
| Patrol presence (4 phases) | Exact table counts after warm-up |
| Day speeder, Core, 2× limit (5 runs) | Pursuit at 8.8 s mean (≤ 12.8 s every run); reinforcement 29 s |
| Night speeder, 03:00 (8 runs) | Pursuit at 126 s mean; reinforcement 115 s |
| Cop self-wreck, Simulation (20 pursuits) | 0% of engaged units; a reckless config (no budget, high caps) has more risk events (run-wide crashes, near misses, wrecks, traffic hits) |
| Tool placement (5 runs, L4) | 35 tools, minimum sight 120.1 m |
| Escape (dusk, below helicopter level) | 2 of 3 runs escape |
| Capture / safehouse (12×12 city) / total loss | Each rule fires in its scripted case |

| City traffic, physical cars (`cargo run --release -p simcli --example police_traffic_contacts -- patrol\|pursuit 300`) | Patrol 300 s: 0 police↔traffic contacts, 1 overlap with kinematic traffic caused by a moving unit. Hot pursuit 362 s: 0 contacts, 0 overlaps with lights on, 3 caused by moving units in total |

## Perf
`cargo run --release -p wp_police --example police_bench`: 13 patrols + reinforcements, 4 wanted
racers on D's small city, with exact building line of sight and obstacle awareness → **0.074 ms per 60 Hz frame** (0.055 before round 6)
(native, release, cloud Xeon 2.1 GHz). Budget 0.5 ms native / 1.0 ms browser. Browser not
measured yet.

Pro driver (physical units with lights on): 2.2–3.1 µs per physics tick per unit including
the line re-plans (`police_pace` test, release, same machine) → ≈ 12 µs per unit per 60 Hz
frame, ≈ 0.1 ms for 8 chasing units.

## Determinism
Absolute time is integer `SimTime` microseconds (`src/time.rs`); durations are `f32` seconds.
The crate uses no `f64`, no `HashMap` iteration and no libm transcendentals (only `wp_math::sin_cos`
and repeated multiplication). The RNG is seeded via `PoliceSetup::seed`. `lint-determinism` reports
no errors for this crate and needs no allow-list entries.

## Tests
```sh
cargo test -p wp_police                          # unit + system tests
cargo test -p wp_police --test police_scenarios  # scenarios, ratios, determinism
cargo run --release -p wp_police --bin police-scenario -- crates/wp_police/scenarios/day_core_speeder.ron
cargo run --release -p wp_police --example police_scenario -- <file.ron> [--tables <dir>]
```

## Known gaps
- The police graph ignores per-lane turn restrictions. Cameras come from the network's POIs (D
  places one at every third signalised junction). `RoadNetworkQuery` has no signalised-junction
  flag yet, so `from_network` takes the list separately (contract request #7).
- The headless kinematic body and its traffic-hazard model stand in for physical cars and real traffic.
- Pro driving (P8): the racing line only sees vehicles within `traffic_clear_m` ahead; a car
  alongside in the next own-direction lane is not in the line's room. Junction caps
  still hold pursuits back: 20 m/s straight on (`pursuit_junction_mps`), 6 m/s through turns
  and reds. Turning round in a
  junction is a full-lock crawl, which can touch a kerb on narrow roads.
- Obstacle awareness sees vehicles only: parked cars and cones (`wp_world` static props) are not
  in the list, so a unit off its lane can still clip one. Passing assumes the other lane exists.
- `PoliceSystem::new()` (used by `wp_sim` today) has no road network, so it tracks heat but spawns no units.
- Scripted racers don't react to PIT or box-ins; physics resolves those in the real game.
- Captain-command events are emitted on the next `step`.
- No JSON Schema for `data/police/*.toml`; validation lives in `PoliceConfig::validate` plus serde `deny_unknown_fields`.
