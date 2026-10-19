# Police integration notes

- **Workstream / prompt:** 06 — police & pursuit AI
- **Branch:** `ws/police`
- **Crates / folders owned:** `crates/wp_police`, `data/police/`, `crates/wp_police/scenarios/`
- **Contract version built against:** 0.3.0

## Exposes
- `PoliceSystem::step(world: &dyn WorldQuery, racers: &[RacerView], events: &[SimEvent], dt, out)`.
  - `world.line_of_sight` is used for every unit, camera and helicopter sighting (eye height
    `EYE_HEIGHT_M` = 1.3 m; the helicopter looks down from its altitude).
  - `TrafficViolation::pos` places civilian reports.
- Construction:
  - **`PoliceSystem::for_world(PoliceConfig, &wp_world::World, PoliceSetup)`**: police on D's
    city. Call `step` with the same `World` as `world`.
  - `PoliceSystem::new()`: built-in tables, no road network, heat only. This is what `wp_sim` uses today.
  - `PoliceSystem::with_city(cfg, RoadGraph, setup)`.
- Roads come from the contract road network **`wp_contracts::RoadNetworkQuery`** (v0.3.0; D's
  `wp_world::RoadNetworkIndex`):
  - **`RoadGraph::from_network(&dyn RoadNetworkQuery, signalised: &[u32])` / `RoadGraph::from_city(&CityMap)`**
    map it 1:1: node ids are kept, and `RoadSegment::edge` holds the edge id (`RoadEdgeId`).
    `RoadGraph` is only the police routing index over it, rebuilt from the trait, never edited.
  - `wp_police::city::procedural(&ProcGenConfig) -> (RoadGraph, CityMap)` builds both for tests and tools.
  - `RoadGraph` keeps only police algorithms: routing, prediction, sight distance and nearest road.
- Driving uses D's **`wp_world::drive::DriveController`**, one per unit:
  - **`bind_unit_entity(unit, entity, SteerGeometry { wheelbase_m, max_steer_rad })`**;
  - **`unit_input(&mut self, unit, dt) -> Option<VehicleInput>`**: the controller's
    pure-pursuit steer plus PID pedals, called every physics tick after `sync_unit`;
  - `sync_unit(unit, UnitSync { pos, vel, heading, health, effects })`. A unit whose effects show a bent wheel pulls over.
  - **`set_obstacles(&[ObstacleView])`** (round 6): every vehicle units must not drive into, once per
    tick before `unit_input`. `ObstacleView::car(entity, pos, vel, forward)` builds a car-sized one.
    Units follow, wait behind or pass them, and spawn clear of them (`wp_police::avoid`).
- Unit lifecycle events: `UnitSpawned` / `UnitDespawned`.
- Queries: `wanted_level`, `racer`, `units`, `incidents`, `pool_count`, `tire_puncture(entity) -> [f32; 4]`,
  `static_colliders()`, `spike_strips()`, `roadblocks()`, `helicopters()`, `now() -> SimTime`, `state_hash()`, `stats()`.
- Bounty (for prompt 08): `bounty`, `bounty_record`, `set_bounty_record`, `record_race_won`, `payout_scale`.
- Player chase units (for prompt 07): `add_player_unit(player, entity, pos, heading)` and `unit_order(unit)` (suggestion only).
- Captain API (for prompt 07): `captain_command(CaptainCommand) -> Result<(), CaptainError>`,
  `captain_view()`, `set_human_captain(bool)`. Roads are named by **`RoadEdgeId`** (D's edge index).
  `CaptainCommand::{to_bytes, from_bytes}` stays until v0.3.0 types it.
- Racer gadgets (for prompts 07/08): `radar(pos, tier)`, `scanner(tier, since: Option<SimTime>)`, `radio_log()`.
- Crash scenes (for prompt 05): `report_crash_scene(pos)`; civilian–civilian `SimEvent::Contact`s also create them.
- `SimEvent::PoliceEvent { kind, payload }` until v0.3.0: `kind` = `wp_police::events::kind::*`,
  `payload` = JSON of `wp_police::PoliceEvent` (`PoliceEvent::from_sim_event`).
- **Radio pacing** (lead helper `lead/playtest-radio`, playtest row 6): `PoliceEvent::Radio` is no
  longer emitted for every assignment. Dispatch logic *raises* a `wp_police::radio::RadioCue`
  (tier `Critical | Tactical | Routine` + optional collapse key) through `PoliceSystem::radio`,
  and `RadioDirector` decides at the end of each `step` what is spoken (event + scanner log).
  Critical (pursuit start "10-80", wanted level up, roadblock/spikes set, suspect lost, capture,
  captain orders) always plays; tactical/routine lines wait for `global_gap_s` (8 s, someone
  wanted) / `global_gap_calm_s` (20 s), `unit_gap_s` (30 s), merge duplicates within
  `collapse_window_s` ("Units 4 and 5 resuming patrol."), drop after `stale_s`; routine chatter
  with nobody wanted needs a unit within `earshot_m` of a racer. Tables: `data/police/radio.toml`
  (`PoliceConfig::radio`; `PoliceConfig::load` uses the built-in one if the directory has none).
  Sim time only, no RNG, not hashed. `radio_stats()` counts raised / spoken / collapsed / dropped.
  **When adding a radio line, pick a tier and a collapse key** (see `dispatch.rs::radio_text`).
  Budget scenarios: `radio_city_chase.ron`, `radio_calm_patrol.ron` (metrics
  `radio_per_min_pursuit`, `radio_per_min_calm`); `cargo run --release -p wp_police --example
  radio_log -- <scenario>` prints the spoken lines.
- Data: `data/police/{heat,presence,pursuit,tools,radio}.toml`. Scenarios: `crates/wp_police/scenarios/*.ron`
  on D's procedural city (`city: (nx, ny, seed)`, default `ProcGenConfig::small()`).

## Needs
| From | What | Workaround now |
|---|---|---|
| Harness (I) | Promote the `soak/traffic_soak_observer` gate: it now runs with `police_cars: true` and meets its pending `civilian_collisions = 0` (round 8) | — |
| Economy (G) | Remove `#[ignore]` from `wp_economy/tests/session.rs::live_police_run_settles_like_the_recording`: it passes with `--ignored` since round 8 (see "Response to round 8"). Re-record the fixtures if you want them from today's police | Test still ignored |
| Specsheet (H) | `tools/specsheet/ram_probe` hard-codes `DISENGAGE_HEALTH = 0.4`; it is now per mode (Arcade 0.15, Simulation 0.4) | Its "off" column is wrong for Arcade |
| Harness (I) | Re-aim `golden/city_chase` (see "Response to round 6") | Collision-free, but no pursuit within 20 s |
| `wp_world` (05) | Optional: parked cars / cones near a point (spatial query), for police obstacle awareness | Units see vehicles only |
| `wp_world` (05) | Kinematic traffic should treat tracked vehicles (police cars) as leaders: it drives into stopped police cars (3 of 4 patrol overlaps, 8 of 11 in pursuit) | None on my side |
| Lead | Contract request #7: signalised flag on `RoadNetworkQuery` nodes | `from_city` reads it from D's map |
| `wp_world` (05) | Optional: per-node turn restrictions in `RoadGraphView`; traffic cameras as a `PoiKind` | Any legal outgoing road may be taken; cameras at every third signalised node |
| `wp_net` (07) | Player roles (captain / chase seats) → `set_human_captain`, `add_player_unit`; replicate `CaptainView` | None |
| ui-kit (`apps/ui-kit`) | Rename `segment` → `road` (a `RoadEdgeId`) in `RequestRoadblock` / `RequestSpikes` | `wp_police` still decodes `segment` as an alias |

`wp_sim` wiring for the city chase demo, once a city is set:
1. `PoliceSystem::for_world(cfg, &world, setup)`, and pass the world to `step`.
2. On `UnitSpawned`, spawn a police car, then `bind_unit_entity(unit, entity, geometry)`.
   Each tick: `sync_unit`, `unit_input(unit, PHYSICS_DT)`; despawn on `UnitDespawned`.
3. Merge `tire_puncture(entity)` into `HandlingEffects::tire_puncture`.
4. Insert `static_colliders()` (roadblock barriers) and `spike_strips()` (line colliders).
5. Pass `TrackedVehicle::siren` from police inputs, so traffic yields.

## Contract change requests
| # | Change | Why | Who is affected | Status |
|---|---|---|---|---|
| 1 | `HandlingEffects.tire_puncture` | Spike strips | — | Already in v0.1.0 |
| 2 | Typed `PoliceEvent` (final list below) | Replaces the `{kind, payload}` bytes | 07, 08, 10, 04, J (audio) | **Final list** for v0.3.0 |
| 3 | Typed `CaptainCommand` + `CaptainError` (final lists below) | Captain client | 07, ui-kit | **Final list** for v0.3.0 |
| 4 | `WorldQuery::line_of_sight` | Police LOS on the real city | — | Landed in v0.2.0; used |
| 5 | Shared road-network trait = D's `RoadGraphView` (+ `RoadEdgeId`) | One road model | 05, 06 | Landed in v0.3.0 (`RoadNetworkQuery`); police build from it (round 8) |
| 7 | `RoadNetworkQuery::node_signalised(n: u32) -> bool` (additive) | Police stop at red lights only at signalised junctions and must know which ones are; the trait lacks it, so `RoadGraph::from_network` takes the list separately | 05 (implements, already has `RoadNode::signalised`), 06 | Proposed (round 8) |
| 6 | JSON Schema for `data/police/*.toml` | Data validation parity | Lead | Optional, later |

### Final `PoliceEvent` variant list (contract v0.3.0)
Kind codes are stable (`wp_police::events::kind`). `UnitId(u32)`, `ToolId(u32)` and
`IncidentId(u32)` are police-allocated ids. Positions are `[f32; 3]` world metres.
| Code | Variant | Fields |
|---|---|---|
| 1 | `Radio` | `unit: Option<UnitId>, text: String` |
| 2 | `WantedLevelChanged` | `racer: EntityId, level: WantedLevel` |
| 3 | `PursuitStarted` | `racer: EntityId, unit: Option<UnitId>` |
| 4 | `PursuitEnded` | `racer: EntityId, outcome: PursuitOutcome { Escaped, Safehouse, Captured, CapturedTotalLoss, CalledOff }` |
| 5 | `UnitSpawned` | `unit: UnitId, kind: UnitKind { Car, Helicopter }, pos: [f32; 3], heading: [f32; 2]` |
| 6 | `UnitDespawned` | `unit: UnitId` |
| 7 | `UnitDisabled` | `unit: UnitId, cause: DisableCause { SelfCrash, RacerContact, Damaged, BentWheel }` |
| 8 | `RoadblockPlaced` | `tool: ToolId, racer: EntityId, pos: [f32; 3], sight_m: f32` |
| 9 | `SpikesPlaced` | `tool: ToolId, racer: EntityId, a: [f32; 3], b: [f32; 3], sight_m: f32` |
| 10 | `ToolRemoved` | `tool: ToolId` |
| 11 | `SpikeHit` | `vehicle: EntityId, wheels: [bool; 4]` |
| 12 | `HeloLaunched` | `unit: UnitId, target: EntityId` |
| 13 | `PitAttempt` | `unit: UnitId, target: EntityId` |

### Final `CaptainCommand` list (contract v0.3.0)
| Variant | Fields |
|---|---|
| `AssignUnit` | `unit: UnitId, order: UnitOrder { Patrol, Pursue { target: EntityId }, Intercept { target: EntityId }, Respond { incident: IncidentId }, Disengage }` |
| `RequestRoadblock` | `target: EntityId, road: Option<RoadEdgeId>` (`None` = automatic placement) |
| `RequestSpikes` | `target: EntityId, road: Option<RoadEdgeId>` |
| `RequestHelicopter` | `target: EntityId` |
| `SetPriorityTarget` | `target: Option<EntityId>` |
| `CallOff` | `target: EntityId` |

`RoadEdgeId(u32)` is an edge index of `wp_contracts::RoadNetworkQuery` (= `wp_world::roads::RoadGraphView::edges`).

The reply is `Result<(), CaptainError>` with:
- `UnknownUnit`, `UnknownTarget`, `UnknownIncident`, `NotPursued`, `Unavailable`, `UnitUnavailable`, `Malformed`;
- `Locked { need_level: u8 }`, `Cooldown { remaining_s: f32 }`, `Budget`;
- `Site(SiteError { NoSite, NotOnPath, BlindCorner, Highway })`.

## Changes outside my crates
- **`apps/simcli/scenarios/soak/traffic_soak_observer.ron`** (round 8, at the lead's request):
  `police_cars: true` and its comment. Nothing else changed.
- **`apps/simcli/examples/police_traffic_contacts.rs`** (round 6): new measurement example
  (police↔civilian contacts, patrol and hot pursuit). Read-only use of simcli and `wp_sim`.
- **`crates/wp_sim/src/police_link.rs`** (round 6, minimal): `drive_police_units` first calls
  `police.set_obstacles(..)` with every vehicle slot (`forward_of(rot)`, `lin_vel`) and every
  kinematic traffic agent (`world.agents()` with `Lod::Kinematic`). ≈ 20 lines; the lead may move it.
- **Root `Cargo.toml`**: `[profile.dev.package.wp_police] opt-level = 2` (kept by the lead).
- `wp_police` depends on `wp_world`. There's no cycle, and it is wasm32-clean.
- **`apps/ui-kit/tools/record-police`** (standalone tool), two edits:
  - it now uses `wp_police::city::procedural(&ProcGenConfig::small())` instead of the removed grid city;
  - `PursuitRecord::end` replaces `end_s` (the tool was already broken by the `SimTime` change).

  Its recorded fixture (`apps/ui-kit/fixtures/runs/police_pursuit`) was not regenerated.

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| `PoliceSystem::step` per 60 Hz frame, 13 patrols + reinforcements, 4 wanted racers, D's small city with exact building LOS, obstacle awareness on (`police_bench`) | Cloud Xeon 2.1 GHz, release | 0.074 ms (0.055 before round 6) | not measured | 0.5 ms / 1.0 ms |

## Known gaps
- The headless kinematic body and traffic-hazard model stand in until units are physical (12).
  It runs wide more often on D's ring road and diagonals, mostly at low speed: the self-wreck
  rate is 0% over 20 Simulation pursuits.
- Obstacle awareness sees vehicles only, not parked cars or cones (`wp_world` static props).
- The light used is the one `signal_state_ahead` reports for the unit's current lane (straight
  ahead), so a unit turning at a junction with separate turn phases may read the wrong one.
- In the police-on soak two police cars touch at 4.5 m/s right after spawn (t = 3.9 s).
- Scripted racers don't react to PIT or box-ins; physics resolves those.
- Night pursuit start varies per seed (4 of 8 runs under 60 s, mean 114 s); the tests assert means and ratios.

## Response to lead feedback
**Round 8, all done:**
1. **`RoadNetworkQuery`:** `RoadGraph::from_network(&dyn RoadNetworkQuery, signalised)` builds the
   police routing index from the v0.3.0 trait (D's `RoadNetworkIndex`); `from_view(&RoadGraphView)`
   is gone. Districts come from `district_at`, cameras / safehouses / helipads / stations from
   `pois()`. The trait has no signalised-junction flag, so `from_city` reads those from D's map
   (contract request #7). The graph is identical to before: every golden hash and police test is
   unchanged. A test builds a graph from a hand-made `RoadNetworkQuery` (not D's).
2. **Arcade `disengage_health`:** now per mode, `{ arcade = 0.15, simulation = 0.4 }` in
   `data/police/pursuit.toml`. H measured Arcade health levelling off at 0.2–0.3 and bottoming out
   at 0.10–0.23, so Arcade units stay until properly wrecked; Simulation is unchanged. Test
   `disengage_health_depends_on_damage_mode` (Arcade 0.3 stays, Arcade 0.1 and Simulation 0.3 leave).
   H's `ram_probe` still hard-codes 0.4 (request above).
3. **Playtest row 11, 5-minute city chase** (`cargo run --release -p simcli --example
   police_traffic_contacts -- patrol|pursuit 300`, 12×12 city at noon, density 4, physical police,
   round-8 `main`):

   | Case | Police↔traffic contacts (physical) | Overlaps with kinematic traffic, police moving | Traffic into a stopped police car | Target |
   |---|---|---|---|---|
   | Patrol, 300 s, nobody wanted | **0** | 1 | 6 | ≤ 1 per 5 min: **met** |
   | Hot pursuit, 338 s pursued (seeds 101–105) | **3**, all lights on, ≤ 0.5 m/s (touches) | 6, all lights off (units not in the pursuit), 2–7 m/s | 6 | ≤ 3 per 5 min: **2.7 contacts met; with moving overlaps 8.0 per 5 min, not met** |

   No police contact above 0.5 m/s anywhere (row 11 was 20+ at up to 25 m/s). Compared with round
   6 (0 contacts, 3 moving overlaps per 362 s), pursuit is slightly worse on today's `main`; the
   remaining moving overlaps are lights-off units at low speed against kinematic traffic, which
   only becomes physical near a player. Traffic driving into stopped police cars is D's request
   (kinematic traffic does not treat tracked vehicles as leaders).
4. **`soak/traffic_soak_observer`** now runs with `police_cars: true` and passes: observer
   `distance_m` 0.008, `civilian_collisions` 0 (its pending zero target is met too),
   `traffic_agents_min` 362 (pending 380 still D's). **I: please promote the gate.**
5. **Parked test (G):** not intended; it was a bug. The stationary racer's violations sat inside
   a traffic camera's radius, so they counted as *witnessed*: full heat, but no report incident,
   and only a report sends a unit when nobody has seen the racer. A racer that never moves was
   never found. Now any violation no unit saw opens a report (camera or civilian;
   radio "traffic camera flagged a vehicle"), so a unit is sent after the report delay. The
   parked racer is pursued at 123 s and captured at 127 s (`cargo run --release -p wp_police
   --example parked_racer`). `live_police_run_settles_like_the_recording` **passes with
   `--ignored`**; G can drop the `#[ignore]` (I didn't edit `wp_economy`). New test
   `camera_flagged_parked_racer_is_found_and_captured`.

**Run 2, all done:**
- **Integer ticks:** police time is `SimTime` (u64 µs); there are no allow-list entries.
- **D's road network:** units drive, route, predict and place tools on D's roads. `RoadGraph` is
  built 1:1 from D's `RoadGraphView`. My grid city (`procedural_city`, `CityParams`) and my lane
  converter are gone, so every test and scenario runs on D's procedural city. Line of sight is D's
  building footprints.
- **D's controller:** every unit uses `wp_world::drive::DriveController`, for physical units
  (`unit_input`) and the headless body (`step_ground`). My `drive_input` is gone.
- **Real violations:** `TrafficViolation::pos` places reports.
- **Typed events:** final `PoliceEvent` / `CaptainCommand` / `CaptainError` lists are above. Captain
  commands now name roads by D's edge id (`RoadEdgeId`).

**Round 6, done:**
- **Police cars no longer drive into stopped cars.** New `wp_police::avoid`: every tick a unit scans
  its lane corridor ahead on its own track (plus a short bumper box straight ahead) for any
  vehicle, and caps its speed to stop 3 m behind it or follow it. At a zero cap physical units hold
  full brake (D's controller releases the brake below 0.6 m/s, which made them creep into the car
  ahead). Behind a stopped vehicle a unit waits, then passes in the other lane when it is clear
  (1.5 s with lights, 6 s without; no pass with oncoming traffic). PIT / box-in units ignore their own
  target. `wp_sim` feeds every vehicle and kinematic traffic car via `set_obstacles` (my small
  `police_link.rs` hook, see "Changes outside my crates"). Tuning: `[avoid]` in `data/police/pursuit.toml`.
- **Spawns clear of other vehicles:** pool units spawn in the right-hand lane, ≥ 25 m from any
  vehicle and outside any moving vehicle's next 5 s of travel (in its lane), and face their first
  patrol route (turning around from standstill drove physical cars over the kerb into parked cars).
  Reinforcement nodes use the same clearance.
- **Also fixed:** route tracks started at the unit's body, so a unit already in its lane got the
  lane offset twice (≈ 3.5 m right of the centreline) and scraped parked cars. They now start on
  the centreline.
- **`soak/traffic_soak_observer` with `police_cars: true` passes** (I didn't edit the file; flip it):
  observer `distance_m` 0, `civilian_collisions` 0 (its pending target passes too). Before: the
  observer was shoved 52 m, and units had 20+ contacts with traffic and each other at up to 24 m/s;
  now none above 1.5 m/s. Two units still touch a parked car / kerb at ≤ 6 m/s (known gap).
- **`golden/city_chase`: no collision, but no pursuit.** All pending checks pass except
  `pursuit_started_before(20 s)`. The unit that used to start 5 m ahead of the racer now spawns
  elsewhere (exactly the case the clearance rule exists for), and at 23:00 the 5 night patrols are
  ≥ 199 m from the racer's street (`probe_police`: units at (273, 73) S, (277, −139) N, (186, 167) W,
  (497, 30) N, (703, 603) E at t = 1 s). The scenario needs re-aiming (I's file). What I tried: the
  racer overtaking unit 2 in the oncoming lane hits oncoming traffic head-on; a modest speeder just
  ahead of unit 2 doesn't register a violation. A pursuit needs the racer wanted *and* a unit within
  40 m for 4 s, so the reliable shape is a racer overtaking a patrol on a 2-lanes-per-direction
  arterial (x or y = ±385) — which needs a patrol there, e.g. day time or starting heat. The cruise
  driver can't overtake by itself, so a racer catching a patrol in a single lane will always rear-end
  it.
- **Follow-up (owner playtest, PLAYTEST_LOG row 11, "cop cars have no regard to traffic"):**
  - *Patrol drives like traffic*: follows the vehicle ahead (gap model above), stops at red lights
    (and yellows with room), gives way to cross traffic at unsignalised junctions (waits while
    anything reaches the junction within 2.5 s of it, max 8 s), turns at 3.5 m/s, no overtaking
    within 25 m of a junction. Lights from `WorldQuery::signal_state_ahead` plus my graph's
    `signalised` node flag. I did not use a D leader query: police already get every vehicle via
    `set_obstacles` and run the same gap logic (request below if D wants one shared query).
  - *Lights on*: may run reds and exceed limits, but crosses junctions at ≤ 11 m/s (≤ 6 m/s against a
    red, giving way up to 3 s; turns ≤ 6 m/s); follows, waits behind or passes civilians as above.
    Civilians yielding to lights and siren is already D's (`TrackedVehicle::siren`).
  - *Contact*: only `Pit` / `BoxIn` units ignore their target; nothing ignores civilians.
  - *Also fixed*: parked units (spike strip / roadblock, no track) used to idle-creep into queued
    traffic; at a standstill full brake engages `wp_vehicle`'s auto-reverse, which sent one unit
    backwards at 25 m/s through traffic. Units now hold with brake 0.4 + handbrake. Police-vs-police
    gridlock at junctions is broken by unit id. Interceptors drive all the way to a stopped racer.
  - **Measured** (`cargo run --release -p simcli --example police_traffic_contacts -- patrol|pursuit 300`,
    12×12 city at noon, traffic density 4, physical police cars):

    | Case | Police↔traffic contacts (physical) | Overlaps with kinematic traffic, police moving | Target |
    |---|---|---|---|
    | Patrol, 300 s, nobody wanted | **0** | 1 (+3 where traffic drove into a stopped police car) | ≤ 1 per 5 min |
    | Hot pursuit, 362 s pursued (4 runs, racer at 20 m/s round the arterial ring) | **0** | 3 in total, 0 by units with lights on (+8 traffic into stopped police cars, mostly parked tool units) | ≤ 3 per 5 min |

    Traffic is physical only near a player, so the example also checks footprint overlaps with
    kinematic traffic. Before this round the same patrol run had 20+ contacts at up to 25 m/s.
- **Typed `PoliceEvent` / `CaptainCommand` lists:** unchanged, final, in "Contract change requests".
- Test change: `risk_budget_and_caps_keep_cops_alive` compared wrecks + traffic collisions only;
  with units keeping distance those are single digits for both configs. Harm now also counts
  run-wide crashes and near misses.

## Lead feedback (2026-10-19, run 2)

**Merged (machine E).** All your tests pass on the integrated build (real vehicle A, damage B, world D);
native == wasm; browser e2e green.

Lead changes in your area (pull them):
- Your scenarios used a third format inside `apps/simcli/scenarios/` (simcli only runs format v2), so they
  moved to **`crates/wp_police/scenarios/`** and the test to **`crates/wp_police/tests/police_scenarios.rs`**.
  Your simcli `main.rs` routing and the `scenarios.rs` skip were dropped (simcli has changed a lot since
  your base). New command: `cargo run --release -p wp_police --bin police-scenario -- <file>`.
- Your many `f64` timestamps are allowed by file-level entries in `tools/lint-determinism/allow.txt`
  (exact IEEE ops, identical on wasm). **Please move them to integer tick counts** and delete the entries.
- Your `opt-level = 2` dev-profile trick is kept and also applied to `wp_world`.

Your `wp_sim` needs — next lead run: spawn vehicles for units + `bind_unit_entity`/`unit_input`/`sync_unit`,
merge `tire_puncture`, insert `static_colliders`, build `with_city` from D's world. Until then police run
internally in `wp_sim` as before.

Most important for you now:
1. **Use machine D's road network** (`wp_world::RoadNetwork` / `CityMap`) instead of your own `RoadGraph` +
   `procedural_city()`: two road models will diverge. Agree a converter or a shared read-only trait with D
   (your contract request #5) — D's network is on `main` now.
2. **Use D's AI driving controller** for units instead of `unit::drive_input` (machine N, opponent AI, is
   told to reuse D's too) — one controller for traffic, police and racers.
3. Real traffic violations: D's `SimEvent::TrafficViolation` now flows from `wp_sim`.

Contract requests: #1 nothing to do; #2/#3 **rejected as JSON payloads** — v0.2.0 makes `PoliceEvent` and
`CaptainCommand` typed enums in contracts (also gives machine I stable kind codes); please send the final
variant list from `wp_police::PoliceEvent`. #4 `WorldQuery::line_of_sight` **accepted** (D implements on the
real city). #5 shared road-network trait **accepted in principle**, shape from D + you. #6 police TOML schema:
optional, later.

**v0.2.0 contract bump is unblocked** — it is the next lead run's first job.

### Lead run 5 (2026-10-19)
- **Your `wp_sim` needs are done:** `Sim::set_world` builds `PoliceSystem::for_world` on the city;
  `Sim::set_police_car(car)` (simcli city runs use `sentinel_cruiser`) gives every AI car unit a real
  vehicle (`VehicleKind::Police`), bound with `bind_unit_entity`, driven by `unit_input`, reported with
  `sync_unit`; retired units are despawned; `tire_puncture` merges into handling. Up to 64 − 16 slots;
  beyond that units keep your kinematic driving. `crates/wp_sim/src/police_link.rs`.
- Racers reported to you are now only players and AI racers (police and traffic were included before).
- Timeline with `cargo run --release -p wp_sim --example police_probe` (small city, 22:00, speeding coupe):
  13 cars on patrol, 11 violations by 5 s, pursuit at ≈ 10 s, searching at ≈ 20 s.
- Not yet: `static_colliders()` (roadblock barriers) as physics boxes — next lead run.

## Lead feedback (round 6, 2026-10-19)

**Merged.** Units on D's roads and D's `DriveController` work in the sim (your new `bind_unit_entity(unit, id, geometry)` /
`unit_input(unit, dt)` are wired in `crates/wp_sim/src/police_link.rs`).

**Top priority — police cars ram stopped cars.** Since run 5 AI police cars are real Rapier vehicles. In
`soak/traffic_soak_observer` (player car parked at the city core, 12:00) a patrol car drives into it at 10.8 m/s at
35.5 s and another at 47 s (`Contact` a=1, b=10 / b=31). With `police_cars: false` the same run has no hits, so it is
the units, not traffic. Units need obstacle awareness on their track: slow / stop behind any vehicle ahead (players,
traffic, other units — `wp_sim` vehicle states, or ask D for a leader query in the shared controller), and pass or
wait sensibly. Also: spawn units clear of other vehicles (in `golden/city_chase` a unit sits 5 m ahead of the racer).
Done = `soak/traffic_soak_observer` passes with `police_cars: true` (lead will flip it back), and the
`golden/city_chase` pursuit happens without a collision.

Also: send your final typed `PoliceEvent` + `CaptainCommand` variant lists for contract v0.3.0 (if not already in
this doc's contract section).


## Lead note (2026-10-19, from machine H)
In Arcade, health levels off around 0.2–0.3 under repeated hits and never reaches total loss (0.10–0.23), so `disengage_health` (0.4) is what decides when a cop leaves the chase. If playtests still find cops fragile, tune that knob, maybe lower in Arcade only. The cruiser police package (H) now stays in the pursuit until its 2nd hard hit.


## Lead feedback (round 8, 2026-10-19)

Contract **v0.3.0 is on `main`** (typed police/race/economy events, damage snapshot/summary, road-network trait, weather `LocalConditions`; see `docs/CONTRACT_CHANGELOG.md`). Workspace tests now take about 3.5 minutes (`lead/test-speed`). Owner playtest items are tracked in `docs/PLAYTEST_LOG.md`.

1. Move onto `RoadNetworkQuery` (v0.3.0 shared road trait) instead of your own `RoadGraph` copy.
2. Arcade `disengage_health`: see H's note above. Make it mode-dependent so cops stay in an Arcade chase until they're properly wrecked.
3. Verify playtest row 11 in a 5-minute city chase (civilian contacts by police on patrol and in pursuit) and record the numbers.
4. Flip `scenarios/soak/traffic_soak_observer.ron` back to `police_cars: true` if your fix stops patrols ramming stopped cars, then tell I to promote the gate.

5. **Parked test (lead, round 8):** see economy.md item 5. A stationary wanted racer, 3 am, `internal_driving: true`, is never found or captured with your round-6 changes (it was on ws/economy alone). Decide whether that is intended (no report → no search) and tell G.
