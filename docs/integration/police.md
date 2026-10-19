# Police integration notes

- **Workstream / prompt:** 06 — police & pursuit AI
- **Branch:** `ws/police`
- **Crates / folders owned:** `crates/wp_police`, `data/police/`, `crates/wp_police/scenarios/`
- **Contract version built against:** 0.2.0

## Exposes
- `PoliceSystem::step(world: &dyn WorldQuery, racers: &[RacerView], events: &[SimEvent], dt, out)`.
  `world.line_of_sight` is used for every unit, camera and helicopter sighting (eye height
  `EYE_HEIGHT_M` = 1.3 m; the helicopter looks down from its altitude). `TrafficViolation::pos`
  places civilian reports.
- Construction:
  - `PoliceSystem::new()`: built-in tables, no road network, so heat only.
  - **`PoliceSystem::for_world(PoliceConfig, &wp_world::World, PoliceSetup)`**: police on D's
    city. Call `step` with the same `World` as `world`.
  - `PoliceSystem::with_city(PoliceConfig, RoadGraph, PoliceSetup { seed, mode, internal_driving, hazards })`.
- **`RoadGraph::from_city(&wp_world::CityMap)`**: converts D's network into the police graph. The
  mapping is in `crates/wp_police/src/city.rs`:
  - network node ids are kept; curved lanes add interior nodes;
  - straight two-way roads merge into one segment, everything else stays one-way;
  - districts: Core/Commercial → Core, Residential/Suburb → Suburb, Industrial/Port → Industrial;
  - POIs: safehouse candidates, helipad and stations are kept, and every third signalised junction gets a traffic camera.
- Queries: `wanted_level`, `racer`, `units`, `incidents`, `pool_count`, `tire_puncture(entity) -> [f32; 4]`,
  `static_colliders()`, `spike_strips()`, `roadblocks()`, `helicopters()`, `now() -> SimTime`, `state_hash()`, `stats()`.
- Bounty (for prompt 08): `bounty`, `bounty_record`, `set_bounty_record`, `record_race_won`, `payout_scale`.
- Physical units (for prompt 12):
  - the `UnitSpawned` / `UnitDespawned` events;
  - `bind_unit_entity(unit, entity)`;
  - `unit_input(unit, SteerGeometry { wheelbase_m, max_steer_rad }) -> Option<VehicleInput>`;
  - `sync_unit(unit, UnitSync { pos, vel, heading, health, effects })`. A unit whose effects show a bent wheel pulls over.
- Player chase units (for prompt 07): `add_player_unit(player, entity, pos, heading)` and `unit_order(unit)` (suggestion only).
- Captain API (for prompt 07): `captain_command(CaptainCommand) -> Result<(), CaptainError>`,
  `captain_view() -> CaptainView`, `set_human_captain(bool)`. `CaptainCommand::{to_bytes, from_bytes}`
  stays until v0.3.0 types it.
- Racer gadgets (for prompts 07/08 to gate by tier): `radar(pos, tier) -> Vec<RadarBlip>`,
  `scanner(tier, since: Option<SimTime>) -> Vec<RadioLine>`, `radio_log()`.
- Crash scenes (for prompt 05): `report_crash_scene(pos)`. Civilian–civilian `SimEvent::Contact`s also create them.
- `SimEvent::PoliceEvent { kind, payload }` until v0.3.0: `kind` = `wp_police::events::kind::*`,
  `payload` = JSON of `wp_police::PoliceEvent`. Decode with `PoliceEvent::from_sim_event`.
- Data: `data/police/{heat,presence,pursuit,tools}.toml` (meanings are in the crate README).
- Headless scenarios: `crates/wp_police/scenarios/*.ron` (`cargo run -p wp_police --bin police-scenario -- <file>`).
  `world_city: true` runs them on D's `ProcGenConfig::small()` city.

## Needs
| From | What | Workaround now |
|---|---|---|
| `wp_world` (05) | **A public driving controller**, e.g. `wp_world::traffic::follow_path(state, path, s, v_ref, wheelbase, max_steer) -> VehicleInput`. `drive_physical` is private, so police and opponent AI (N) can't reuse it | `unit::drive_input` ports the same steering law (look-ahead `4 + 0.6·v` clamped 5–25 m, `δ = atan(L·κ)`), with stateless speed control |
| `wp_world` (05) / lead | Shared read-only road-network trait (v0.3.0) | `RoadGraph::from_city` converter |
| `wp_world` (05) | Optional: turn restrictions per node, and traffic cameras as a `PoiKind` | Any legal outgoing road may be taken; cameras at every third signalised junction |
| `wp_sim` (12) | `PoliceSystem::for_world(cfg, &world, setup)` once a city is set; spawn vehicles for units plus `bind_unit_entity`, `unit_input` and `sync_unit`; merge `tire_puncture`; insert `static_colliders` | Units are driven internally |
| `wp_net` (07) | Player roles (captain / chase seats) → `set_human_captain`, `add_player_unit`; replicate `CaptainView` | None |

## Contract change requests
| # | Change | Why | Who is affected | Status |
|---|---|---|---|---|
| 1 | `HandlingEffects.tire_puncture` | Spike strips | — | Already in v0.1.0 |
| 2 | Typed `PoliceEvent` (final list below) | Replaces the `{kind, payload}` bytes | 07, 08, 10, 04 | **Variant list sent** for v0.3.0 |
| 3 | Typed `CaptainCommand` (final list below) | Captain client | 07 | **Variant list sent** for v0.3.0 |
| 4 | `WorldQuery::line_of_sight` | Police LOS on the real city | — | Landed in v0.2.0; used |
| 5 | Shared road-network trait | One road model | 05, 06 | Accepted in principle; `RoadGraph::from_city` bridges until then |
| 6 | JSON Schema for `data/police/*.toml` | Data validation parity | Lead | Optional, later |

### Final `PoliceEvent` variant list (for contract v0.3.0)
These are the stable kind codes from `wp_police::events::kind`. Ids are `UnitId(u32)` and
`ToolId(u32)`, and positions are `[f32; 3]`. They can live in contracts as-is.
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

### Final `CaptainCommand` list (for contract v0.3.0)
| Variant | Fields |
|---|---|
| `AssignUnit` | `unit: UnitId, order: UnitOrder { Patrol, Pursue { target }, Intercept { target }, Respond { incident: IncidentId }, Disengage }` |
| `RequestRoadblock` | `target: EntityId, segment: Option<SegmentId>` |
| `RequestSpikes` | `target: EntityId, segment: Option<SegmentId>` |
| `RequestHelicopter` | `target: EntityId` |
| `SetPriorityTarget` | `target: Option<EntityId>` |
| `CallOff` | `target: EntityId` |

The reply type would be `CaptainError { UnknownUnit, UnknownTarget, UnknownIncident, NotPursued,
Locked { need_level: u8 }, Cooldown { remaining_s: f32 }, Budget, Site(SiteError { NoSite,
NotOnPath, BlindCorner, Highway }), Unavailable, UnitUnavailable, Malformed }`. `SegmentId` is a
police graph id. Once the shared road-network trait lands it should become that trait's segment id.

## Changes outside my crates
- **Root `Cargo.toml`**: `[profile.dev.package.wp_police] opt-level = 2` (kept by the lead).
- `wp_police` now depends on `wp_world`. There's no cycle: `wp_world` doesn't depend on police, and it is wasm32-clean.
- `tools/lint-determinism/allow.txt`: police entries deleted (integer `SimTime`).

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| `PoliceSystem::step` per 60 Hz frame, 13 patrols + reinforcements, 4 wanted racers (`police_bench`) | Cloud Xeon 2.1 GHz, release | 0.030 ms | not measured | 0.5 ms / 1.0 ms |

## Known gaps
- The built-in kinematic driver and traffic-hazard model stand in until units are physical (12).
- On D's city the built-in driver runs wide more often, on the ring road and diagonals. It's
  mostly at low speed: the self-wreck rate is 2% over 20 Simulation pursuits.
- `wp_sim` still uses `PoliceSystem::new()`; `for_world` is ready for it.
- Scripted racers don't react to PIT or box-ins; physics resolves those.
- Night pursuit start varies per seed; the tests assert means and ratios.

## Response to lead feedback
**Run 2:**
- **Integer ticks: done.** Police time is `SimTime` (u64 µs), and the allow-list entries are deleted.
- **Use D's road network: done**, via the converter `RoadGraph::from_city` and `PoliceSystem::for_world`:
  - Line of sight comes from `WorldQuery::line_of_sight` (D's building footprints).
  - Scenarios can run on D's city (`world_city: true`). `world_day_core_speeder`,
    `world_night_core_speeder` and `world_self_wreck_sim` pass with the same tables:

    | Measure | Day | Night |
    |---|---|---|
    | Pursuit start, mean | 9.5 s | 240 s |
    | Reinforcement delay | 30 s | 101 s |

    Self-wreck rate is 2%. A test asserts the night/day ratios.
  - Own `procedural_city()` stays for the original tests.
- **Use D's AI controller: partly.** It is private (`Traffic::drive_physical`), so `drive_input`
  now ports the same steering law and takes `SteerGeometry`. I've asked D for a public function (see Needs).
- **Real violations:** `TrafficViolation::pos` is used for report positions; heat already came from the event stream.
- **Typed `PoliceEvent` / `CaptainCommand`:** final variant lists are above.

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
