# Police integration notes

- **Workstream / prompt:** 06 — police & pursuit AI
- **Branch:** `ws/police`
- **Crates / folders owned:** `crates/wp_police`, `data/police/`, `crates/wp_police/scenarios/`
- **Contract version built against:** 0.2.0

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
- Roads come from D's `wp_world::roads::RoadGraphView`:
  - **`RoadGraph::from_view(&RoadGraphView)` / `RoadGraph::from_city(&CityMap)`** map it 1:1:
    node ids are kept, and `RoadSegment::edge` holds the source edge id.
  - `wp_police::city::procedural(&ProcGenConfig) -> (RoadGraph, CityMap)` builds both for tests and tools.
  - `RoadGraph` keeps only police algorithms: routing, prediction, sight distance and nearest road.
- Driving uses D's **`wp_world::drive::DriveController`**, one per unit:
  - **`bind_unit_entity(unit, entity, SteerGeometry { wheelbase_m, max_steer_rad })`**;
  - **`unit_input(&mut self, unit, dt) -> Option<VehicleInput>`**: the controller's
    pure-pursuit steer plus PID pedals, called every physics tick after `sync_unit`;
  - `sync_unit(unit, UnitSync { pos, vel, heading, health, effects })`. A unit whose effects show a bent wheel pulls over.
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
- Data: `data/police/{heat,presence,pursuit,tools}.toml`. Scenarios: `crates/wp_police/scenarios/*.ron`
  on D's procedural city (`city: (nx, ny, seed)`, default `ProcGenConfig::small()`).

## Needs
| From | What | Workaround now |
|---|---|---|
| `wp_sim` (12) | Wiring for the city chase demo (see below) | Units are driven internally, and `wp_sim` uses `PoliceSystem::new()` |
| `wp_world` (05) / lead | `RoadGraphView` as the shared road-network trait (v0.3.0) | Police build from the struct directly |
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
| 5 | Shared road-network trait = D's `RoadGraphView` (+ `RoadEdgeId`) | One road model | 05, 06 | Police now consume it; ready to promote |
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

`RoadEdgeId(u32)` is an index into `wp_world::roads::RoadGraphView::edges`.

The reply is `Result<(), CaptainError>` with:
- `UnknownUnit`, `UnknownTarget`, `UnknownIncident`, `NotPursued`, `Unavailable`, `UnitUnavailable`, `Malformed`;
- `Locked { need_level: u8 }`, `Cooldown { remaining_s: f32 }`, `Budget`;
- `Site(SiteError { NoSite, NotOnPath, BlindCorner, Highway })`.

## Changes outside my crates
- **Root `Cargo.toml`**: `[profile.dev.package.wp_police] opt-level = 2` (kept by the lead).
- `wp_police` depends on `wp_world`. There's no cycle, and it is wasm32-clean.
- **`apps/ui-kit/tools/record-police`** (standalone tool), two edits:
  - it now uses `wp_police::city::procedural(&ProcGenConfig::small())` instead of the removed grid city;
  - `PursuitRecord::end` replaces `end_s` (the tool was already broken by the `SimTime` change).

  Its recorded fixture (`apps/ui-kit/fixtures/runs/police_pursuit`) was not regenerated.

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| `PoliceSystem::step` per 60 Hz frame, 13 patrols + reinforcements, 4 wanted racers, D's small city with exact building LOS (`police_bench`) | Cloud Xeon 2.1 GHz, release | 0.055 ms | not measured | 0.5 ms / 1.0 ms |

## Known gaps
- The headless kinematic body and traffic-hazard model stand in until units are physical (12).
  It runs wide more often on D's ring road and diagonals, mostly at low speed: the self-wreck
  rate is 1.3% over 20 Simulation pursuits.
- Scripted racers don't react to PIT or box-ins; physics resolves those.
- Night pursuit start varies per seed (4 of 8 runs under 60 s, mean 114 s); the tests assert means and ratios.

## Response to lead feedback
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
