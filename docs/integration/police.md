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
- **Assaulting an officer** (playtest row 24): a `SimEvent::Contact` between a racer and a bound
  unit's entity that passes `[heat.assault]` in `data/police/heat.toml` raises heat to ≥ L2 and
  starts the pursuit in the same `step` (events: `WantedLevelChanged`, `PursuitStarted { unit }`,
  `Radio` "Unit N: I've been hit! … assault on an officer."). `stats().assaults` counts them.
  Nothing new is needed from `wp_sim`: it already passes contacts and binds unit entities.
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
| Harness (I) | A negative police expectation for simcli, e.g. `no_pursuit_before(t_s)` (or `pursuit_started_before` with `expected: false`), so a 5 km/h parking-bump city scenario can assert "no pursuit" (row 24) | Covered by `wp_sim/tests/police_assault.rs` and `crates/wp_police/scenarios/parking_bump.ron` |
| Lead (`wp_wasm`) | A captured / wrecked car is held with brake 1.0. At a standstill that engages `wp_vehicle`'s auto-reverse (brake > 0.5 for 0.25 s), and in reverse the pedals swap, so a held car can drive itself backwards. Hold with brake 0.4 + handbrake, as police units do. Found via the capture test above, where it is masked because the racer is pinned against the police car | — |
| ui-kit / lead (`wp_wasm`) | Optional: the HUD banner sub-line could say why a pursuit started ("assault on an officer"); `police_status()` would need the last offence. The PURSUIT banner and the radio line in the event feed already show it | — |
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
- **Playtest row 24** (this round):
  - `crates/wp_sim/tests/police_assault.rs`: new test file only (no `wp_sim` code changed; the
    contact → `on_contact` → unit-entity path already worked).
  - `apps/simcli/scenarios/police/sideswipe_patrol_{day,night}.ron`: new city scenarios, with
    their goldens in `apps/simcli/golden/` (`[bless]` commit).
  - `crates/wp_wasm/src/run_tests.rs` (lead's): `capture_bills_holds_the_car_and_repair_continue_restores_it`
    asserted the capture at exactly L2. Its "parked" racer holds full brake, which at a standstill
    engages `wp_vehicle`'s auto-reverse, so it backs into the police car at about 27 km/h (6,800 N·s):
    now an assault, L3. The assertion is `>= L2` with a comment; nothing else changed.
  - `apps/simcli/golden/race_city_sprint/` re-blessed: at 15.2 s the coupe, already pursued for
    speeding, rams police unit 3 (8,600 N·s, 8.7 m/s). That is now an assault (L1 → L2), so the
    hashes change from tick 3840. Its expectations still pass.
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
- Assault isn't judged by fault: a moving racer that a patrol car drives into (rare with obstacle
  awareness) also counts; overflow kinematic units in `wp_sim` (beyond the physical slots) can't
  be hit, and the headless proximity stand-in never counts as an assault.
- `presence.toml` unlocks spikes at L2 (`spikes_level = 2`); the doctrine table says L3+. Unchanged
  here (it would move goldens); flagging it for a tuning pass.
- The headless kinematic body and traffic-hazard model stand in until units are physical (12).
  It runs wide more often on D's ring road and diagonals, mostly at low speed: the self-wreck
  rate is 0% over 20 Simulation pursuits.
- Obstacle awareness sees vehicles only, not parked cars or cones (`wp_world` static props).
- The light used is the one `signal_state_ahead` reports for the unit's current lane (straight
  ahead), so a unit turning at a junction with separate turn phases may read the wrong one.
- In the police-on soak two police cars touch at 4.5 m/s right after spawn (t = 3.9 s).
- Scripted racers don't react to PIT or box-ins; physics resolves those.
- Night pursuit start varies per seed (4 of 8 runs under 60 s, mean 114 s); the tests assert means and ratios.

## Response to owner playtest row 24 (2026-10-19): "I sideswiped a cop and a chase didn't start"
**Fixed.** Cause: `wp_sim` delivered the contact and `wp_police` mapped it to the hit unit, but
`unit_racer_contact` only charged the unit's risk budget: hitting a cop added no heat, and a
pursuit still needed a wanted racer kept in recognition range for `recognition_time_s` (4 s
within 40 m at night). Checked the whole path:
1. `SimEvent::Contact` (player ↔ police car) reaches `PoliceSystem::step` in the same tick, and
   `unit_index_by_entity` finds the unit (physical units since round 5). No glue was needed.
2. New offence, assaulting an officer (`[heat.assault]` in `data/police/heat.toml`): heat
   +20, to at least L2 (doctrine table: assault ≥ L2).
3. The hit unit recognises the racer at once and the pursuit starts in the contact's tick, day
   or night (nearby units with line of sight act if the hit unit is out of action).
4. HUD: the PURSUIT banner follows `police_status().status`, and the Critical radio line
   "Unit N: I've been hit! racer #… rammed my unit, assault on an officer." appears in the event
   feed and the audio scanner.

Telling a hit from a nudge (measured with physical cars, `wp_sim` and simcli):

| Contact | Summed impulse | Normal closing | Relative speed | Counts? |
|---|---|---|---|---|
| Glancing flank sideswipe, 30-50 km/h | 220-1,500 N·s | 0.7-2.1 m/s | 8.3-13.9 m/s | yes |
| Harder sideswipe / quarter hit, 30-50 km/h | 2,500-12,500 N·s | 2.7-13.8 m/s | ≥ 8 m/s | yes |
| Mirror-to-mirror with an oncoming patrol car, 30 km/h | 1,300-11,000 N·s | 4-13 m/s | ≈ 14 m/s | yes |
| 5 km/h parking bump | 860-1,460 N·s | 0.9-1.3 m/s | 1.4 m/s | no |

A glancing swipe has little normal closing speed, so the rule uses the cars' relative speed
(`min_rel_speed_mps` = 2.5) plus a small impulse floor (`min_impulse_ns` = 150, feather touches)
and racer speed ≥ 2.5 m/s (a standing car is the one being hit). Speeds are the racer's just
before the episode's first contact: a head-on hit stops the racer within the tick that reports
it (reading the speed after the hit first hid a 10,700 N·s hit), and a patrol car nudging a
parked racer shoves it (which must not make the parked racer the attacker). The unit's own PIT / box-in / rolling-block contact on its target is not an assault.

Tests:
- `wp_sim/tests/police_assault.rs`: real Rapier cars, patrol car paused at its post. Glancing
  and rear-quarter sideswipes at 30 and 50 km/h → pursuit within 1 s of the contact, wanted ≥ L2,
  radio line. Grid at 12:00 and 23:00, Port Vesper v2 at 12:00 and 23:00. A 5 km/h bump into
  the tail (both cities, day and night): nothing. Fails without the fix.
- `wp_police/tests/system.rs`: thresholds day/night, episodes, repeat cooldown, PIT exclusion,
  ramming a second unit escalates.
- Police scenarios `assault_sideswipe_{day_30,day_50,night_30,night_50}.ron` and
  `parking_bump.ron` (new `hit_unit_at_s` / `hit_kmh` racer script; metrics `hits`, `assaults`,
  `hit_to_pursuit_max_s`): pursuit in 0.01 s every run; the bump does nothing.
- simcli `police/sideswipe_patrol_day.ron` (glancing swipe of a patrol car waiting at a junction)
  and `police/sideswipe_patrol_night.ron` (flank swipe of an oncoming patrol car; re-timed in round 12): wanted
  change and pursuit in the contact's tick. Both fail with the rule off (day: pursuit only at
  9.2 s from L1 reports; night: never).

## Response to lead feedback
**Round 12 (on `lead/round12`), done:**
1. **`police/sideswipe_patrol_night.ron` passes again.** Cause: a near miss, not a police change.
   `probe_police` on `lead/round12` shows unit 3 on the same path and timing as before (south down
   x = -276.8, 5-8 m/s after y = 165); the racer, with A's diffs, passes it 2.8 m apart at 12 s
   instead of touching. The old steer (-0.3 at 11.4 s) was at the edge of the hit window. I swept
   the steer: 11.2 / 11.3 / 11.4 s × -0.3 / -0.35 / -0.4 / -0.5 all hit except the old one, so the
   scenario now steers -0.4 at 11.3 s (the centre). Contact at 12.0 s, 2,500 N·s, normal sideways
   (a flank swipe), and wanted L2 + pursuit + "Unit 3: I've been hit!" in that tick. I re-timed
   rather than adding a target-seeking driver because the simcli drivers are I's code (request
   below if I wants one). Assault → pursuit behaviour is unchanged (no `wp_police` code changed).
   The day variant passes unchanged (its hashes don't move). `[bless]` commit: only
   `golden/police_sideswipe_patrol_night` moves.
   *Request to I (optional):* a `seek` driver (steer at an entity) would make police contact
   scenarios independent of vehicle tuning.

**Round 12 gate** (`ws/police` = `lead/round12` + the scenario re-time + its `[bless]`):
fmt and clippy `-D warnings` clean, wasm build OK. `cargo test --workspace --no-fail-fast`: 873
passed, 3 failed, 16 ignored. None of the 3 are police code, and all 3 fail the same way on
`lead/round12` (my diff only touches the night scenario, its golden and this doc):
- `simcli --test scenarios` / `all_scenarios_pass…`: `city/blocked_parked_player_vesper`
  `traffic_turnarounds` = 0 (D's round-12 item). `simcli run apps/simcli/scenarios` (release):
  54 pass, that one fails.
- `simcli --test scenarios` / `goldens_match`: 18 goldens drift on `lead/round12` itself
  (`simcli goldens`, release): city/blocked_parked_player_{grid,vesper}, golden_city_chase,
  golden_night_pursuit_escape, opponent_{flee_police,open_roads_traffic}, panels_frontal_56_hood,
  police_sideswipe_patrol_day, race_city_sprint_police, the four traffic soaks and five weather
  scenarios. Their expectations pass; the hashes look like A's diff rework and D's traffic merge
  landing without a re-bless. I did not re-bless them (not moved by my change), including my
  own `police_sideswipe_patrol_day` (first mismatch at tick 360, still passes). The lead can
  bless them with the merge.
- `wp_net --test vesper_e2e` (`Escaped` instead of `Captured`, F's round-12 item). If F traces
  it to a police rule (assault heat or capture/search), I'll take it.

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

## Lead note: weather in the game (2026-10-19, `lead/weather-in-game`)
`wp_sim` now passes `PoliceSystem::step` a weathered `WorldQuery` when weather is on: every
`line_of_sight` is false beyond `visibility_m` at the observer (≈ 130 m thick fog, ≈ 480 m
downpour, 800 m clear: no change in clear air). Measured with `scenarios/weather/fog_pursuit*`:
the noon city-chase speeder is recognised at 1.9 s in fog vs 0.3 s clear. `weather_at(p)` also
answers `visibility_m` if you want the explicit cap on `witness_range_m` /
`recognition_range_m` (K's step 4). Still yours: the speed-cap / incident-rate setter (K's
step 4). Observation: in the golden city-chase layout with heavy rain on a soaked road no
pursuit starts within 20 s (patrols on the wet road miss the night 40 m / 4 s window); worth a
look when tuning wet-road pursuit.

## Lead feedback (round 12, 2026-10-19)

**Base for this round: `lead/round12`, not `main`.** Round 12 merged ten branches (J, J2, A, D, E, R, M,
weather, G, F) onto `lead/round12`; everything passes except the item below, so `main` stays
where it was until it is fixed. Merge `origin/lead/round12` into your branch (not main), fix,
run the full gate there, push your `ws/*` branch, and the lead merges it.

1. **`apps/simcli/scenarios/police/sideswipe_patrol_night.ron` fails after the merge**: the racer
   never touches the patrol car (`Contact` count 0), so no assault / pursuit before 13.1 s. It
   passed on `ws/police`; on `lead/round12` it fails from the first merge that contains A's
   differential rework (`ws/vehicle`) and D's traffic changes. The scripted racer path probably
   no longer meets the cruiser (different launch / cornering with real diffs, or the cruiser's
   path changed). Make the scenario robust: steer the racer *at* the cruiser (target-seeking
   rather than a fixed script) or re-time it, and check the day variant still passes. Keep the
   assault → pursuit behaviour itself unchanged. Re-bless only what your change moves, in a
   `[bless]` commit with the reason.

## Lead feedback (round 12b, 2026-10-20) — owner playtest: police spawn too far away
Base: `origin/lead/round12` (not main). Owner: "the police spawn too far — we should spawn them in
when there's public safety calls for reckless driving."
1. Dispatch-driven spawning: when a 911-style call / traffic violation / reckless-driving report
   raises heat (the "Dispatch: caller reports reckless driver" line already exists), dispatch
   assigns a responding unit and, if no idle unit is within a response radius, **spawns** one
   just out of the player's sight (behind a corner / over a crest, never in view, never closer
   than ~120 m, typically 150–350 m away along the road graph) heading toward the call. Use
   `line_of_sight` and the road network; no pop-in in view.
2. Response time should feel like seconds, not minutes: first unit on scene within ~10–25 s of
   a reckless-driving call in the core districts (district-dependent), backup on escalation.
3. Keep the patrol population (wandering units) but cap total units by heat level; despawn
   units far out of sight when they are not needed (same never-in-view rule).
4. Scenario(s) with expected ranges: time from first reckless-driving call to first unit within
   100 m; no spawn inside the player's view cone or < 120 m. Goldens in a [bless] commit.
5. Coordinate with the 4-player test (`crates/wp_net/tests/vesper_e2e.rs` must still pass) and
   the LOD hand-over (spawned police use the fleet → full view hand-over in render/lod.ts).

