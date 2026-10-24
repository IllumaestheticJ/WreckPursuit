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
- Unit lifecycle events: `UnitSpawned` / `UnitDespawned`. Since round 12b units also spawn on
  reckless-driving calls (`UnitRole::Responder`), out of every racer's sight and ≥ 120 m away;
  despawns are out of sight too. `wp_sim` handles them like any unit (no change needed).
- Stats (round 12b): `stats().calls` (`CallRecord`: racer, time, place, responder spawned),
  `stats().spawns` (`SpawnRecord`: every ground unit after the session start, distance to the
  nearest racer, in sight), `redispatches`, `pursuit_harm`, `pursuit_high_g_s`, `pursuit_no_risk_s`,
  `stuck` (`stuck::StuckRecord`), `responses` (`ResponseRecord`), `blatant`, `swings`.
- Parked cars / cones: `set_props(Vec<stuck::PropBox>)` (`for_world` does it from
  `World::static_props`). `set_unstick_recovery(bool)` (measurements only).
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
- **Pro driving** (lead helper `lead/police-driver`, P8; module `wp_police::pro`, README
  "Pro driving"): with lights on, `unit_input` drives each physical unit with a
  `pro::ProDriver` (racing line in its own-direction lanes and junction boxes, speed profile at
  the grip limit, late braking, slide control, stuck recovery). Patrol keeps the old lane
  driver. New API:
  - `sync_unit_feel(unit, pro::UnitFeel { yaw_rate, road_grip })` after `sync_unit` (wp_sim
    sends `ang_vel[2]` and `wp_vehicle::Vehicle::conditions_grip()`, so wet roads slow units);
  - `set_unit_limits(unit, pro::CarLimits)` (wp_sim: `Sim::set_police_car_limits`); default
    `[pro] default_limits` = the Sentinel Cruiser measured;
  - `pro::RoadRoom::from_city(&CityMap, &ProParams)` (built by `for_world`), `pro::RoomQuery`;
  - `RoadSegment::{lanes_fwd, lanes_back, lane_width_m}` (serde default 0 = unknown);
  - tuning `[pro]` in `data/police/pursuit.toml` (`PursuitFile::pro`, every key spelled out; a
    test keeps it equal to `ProParams::default`).
  Measured: `cargo test --release -p wp_opponent --test police_pace -- --nocapture` (police vs
  the best race-AI profile, same car: arterial square −1.0 %, Hollins Drive +2.3 %, 0 wall
  hits dry and wet; the pre-P8 driver +13.5 % and did not finish Hollins). City with traffic:
  README "Pro driving".
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
- **Round 12c:** `crates/wp_sim/src/police_link.rs` (minimal): `reconcile_police_units` applies
  `PoliceSystem::take_unit_resets` (place the car on the ground at the reset pose, `set_ghost`
  for `ghost_s`) before syncing units back. `crates/wp_sim/tests/police_passby.rs`: the
  row-80 pass-by matrix test. `crates/wp_wasm/src/audio.rs`: siren test window 30 → 60 s.
  `crates/wp_wasm/src/career_tests.rs`: `brake_until_settled` holds the brake at 0.5 (above
  0.5 at a standstill engages auto-reverse). `apps/simcli/scenarios/police/tunnel_escape_founders_cut*.ron`:
  re-seeded 56 → 51. `crates/wp_net/tests/vesper_e2e.rs`: p1 may miss racer 0's capture when
  it lands in p1's deliberate link-down window (it now does, at step ≈ 2746 of 2700–2880;
  events are not replayed on reconnect). Goldens re-blessed in `[bless]` commits.
- **Round 13:** `apps/simcli/examples/police_chase.rs` (new measurement example: full sim,
  Port Vesper, scripted fleeing racer, before/after tables for rows 61 / 63 / 64 / 66; read-only
  use of simcli / `wp_sim`). `apps/simcli/scenarios/police/tunnel_escape_founders_cut*.ron`:
  comments, and the control's capture window starts at 115 s (was 130 s). Goldens re-blessed for
  the scenarios this round moves (`[bless]` commit).
- **Round 12b:** `apps/simcli/examples/police_response.rs` (new measurement example: full sim,
  physical police, call → first police car within 100 m, pop-in audit from the player's eye and
  a chase camera). Read-only use of simcli / `wp_sim`. Goldens re-blessed for the scenarios this
  change moves (see "Response to lead feedback", round 12b).
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

### Assist presets (backlog B10, done from this session at the owner's request)
One-click **Rookie / Street / Sim** (GDD §6.1), with damage mode and assists still separate axes:
- **`crates/wp_vehicle/src/presets.rs`** (new; owner: A/02): `AssistPreset { Rookie, Street, Sim }`
  with `damage_mode()`, `assists(InputSource)`, `matching(..)` and `from_name`; re-exported as
  `wp_vehicle::AssistPreset`. Keyboard keeps its pedal filters and speed-sensitive steering in
  every preset (GDD §6.2). No serde derive, because `wp_vehicle` has no serde dependency.
- **`crates/wp_wasm`** (owner: C/04): adds a direct `wp_vehicle` dependency; new exports
  `assist_presets()`, `assist_preset_named(name, source)` → `{mode, assists}` and
  `assist_preset_of(entity, source)`. There is a native session test.
- **`apps/testbed-web`** (owner: C/04): a preset row above the assist boxes, wrappers in
  `src/wasm/sim.ts`, and the `e2e/presets.mjs` check (`npm run e2e:presets`).
- **`apps/ui-kit`** (owner: M/16): presets renamed to `rookie | street | sim | custom`, with values
  equal to `wp_vehicle`'s keyboard presets. A preset sets the damage mode too, `presetOf(assists, mode)`
  checks both, and the default is Street.
- **Contract request (lead):** `AssistProfile::esc_strength: f32`. The GDD's Street preset wants
  "ESC low", but `esc` is a switch, so Street leaves ESC off for now.
- Not done: the GDD's "suggest Rookie after three crashes in five minutes" (needs a crash
  counter in the run). simcli scenarios could gain `assists: Preset(..)` (owner: I).
- **Lead `lead/police-driver` (P8)**, outside `wp_police`:
  - `crates/wp_vehicle/src/vehicle.rs`: `Vehicle::conditions_grip()` (read-only, additive).
  - `crates/wp_sim`: `Sim::set_surface_conditions`, `Sim::set_police_car_limits`, and
    `police_link` sends `sync_unit_feel` / `set_unit_limits`.
  - `crates/wp_opponent/tests/police_pace.rs`: new full-sim test (police vs race AI), reusing
    the crate's `tests/common` harness; no change to `wp_opponent` itself.
  - `apps/simcli/examples/police_traffic_contacts.rs`: also counts police↔world hits;
    `WP_TRACE_COP=<seed>,<entity>` traces one police car.
  - Goldens re-blessed (`[bless]` commit): every scenario with physical police cars.

## Perf numbers
| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| `PoliceSystem::step` per 60 Hz frame, 13 patrols + reinforcements, 4 wanted racers, D's small city with exact building LOS, obstacle awareness on (`police_bench`) | Cloud Xeon 2.1 GHz, release | 0.097 ms (round 13: up to 8 units per racer, backup, search; 0.055 ms in round 12b, 0.074 ms in round 6) | not measured | 0.5 ms / 1.0 ms |
| `PoliceSystem::step` per 60 Hz frame, 13 patrols + reinforcements, 4 wanted racers, D's small city with exact building LOS, obstacle awareness on (`police_bench`) | Cloud Xeon 2.1 GHz, release | 0.074 ms (0.055 before round 6) | not measured | 0.5 ms / 1.0 ms |
| Pro driver (`ProDriver::drive`, lead P8) per unit per 60 Hz frame (4 physics ticks, re-plans included; `police_pace` test) | Cloud container, release | ≈ 0.012 ms (2.2–3.1 µs per tick) | not measured | (part of the 0.5 ms) |

## Known gaps
- Assault isn't judged by fault: a moving racer that a patrol car drives into (rare with obstacle
  awareness) also counts; overflow kinematic units in `wp_sim` (beyond the physical slots) can't
  be hit, and the headless proximity stand-in never counts as an assault.
- `presence.toml` unlocks spikes at L2 (`spikes_level = 2`); the doctrine table says L3+. Unchanged
  here (it would move goldens); flagging it for a tuning pass.
- Round 13 (owner rows 63/64) makes the AI more aggressive than `POLICE_DOCTRINE.md` (P1/P2/P4,
  §3.2–3.3); the `[chase]` tables are the truth until P updates the doctrine.
- Police hit buildings more often at L2/L3 than the old slow police (row 66 table); PITs line up
  and touch but rarely spin the scripted racer.
- The headless kinematic body and traffic-hazard model stand in until units are physical (12).
  It runs wide more often on D's ring road and diagonals, mostly at low speed: the self-wreck
  rate is 0% over 20 Simulation pursuits.
- Obstacle awareness sees vehicles only, not parked cars or cones (`wp_world` static props).
- The light used is the one `signal_state_ahead` reports for the unit's current lane (straight
  ahead), so a unit turning at a junction with separate turn phases may read the wrong one.
- In the police-on soak two police cars touch at 4.5 m/s right after spawn (t = 3.9 s).
- Never-in-view covers patrol refills, responders, reinforcements and every despawn. Roadblock
  and spike units still appear where the tool rules put them (≥ 300 m ahead, ≥ 120 m approach
  sight), so a racer can see them appear. Out-of-service helicopters vanish after 15 s.
- The view check takes the spot at the racer's own height, so on a steep hill a spot below a
  crest can count as visible (conservative) and one above it as hidden.
- Night response has a long tail when the racer keeps turning away (worst 38-55 s headless);
  the re-dispatch rule bounds it but a racer that turns at every junction can outrun the
  prediction. Physical responders also slow for junctions like every unit.
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
**Round 13 (on `lead/round12`): owner playtest rows 61 / 63 / 64 / 66, the four "Not done" items
of round 12b.** All four are in; the before/after tables are below. Row 66 is better than the same
aggressive driving without the new avoidance, but police still touch buildings more often than
the old, slow police did (details under row 66).

*How it was measured.* New example `apps/simcli/examples/police_chase.rs`: full sim, Port Vesper
v2 at 12:00, traffic and parked cars, physical police. A scripted racer (`debug_coupe`, wp_world's
path follower) is placed 30 m ahead of a patrolling cruiser and flees along random routes at
24 m/s (86 km/h); its heat is held in the middle of the level (L1 20, L2 40, L3 60) and the
pursuit started with the new `PoliceSystem::begin_pursuit`. It backs out when blocked and, after
three tries with no police car within 20 m, is put 15 m further along its route (as a player
would steer round a jam); it never stops on purpose. 8 seeds × 180 s per level; a run ends at
the first capture or escape. "Before" = `lead/round12` + only the measurement hooks
(`begin_pursuit`, `PoliceStats::static_hits`), same seeds, same harness. "After" = commit
`e4f8b56` (the later tunnel-portal tweak only changes searches for a racer lost in a tunnel).
`cargo run --release -p simcli --example police_chase -- vesper 12 8 180 24 123`.

*Row 64, backup converging (done).*
- When a pursuit is on, idle and patrolling units within `backup_radius_m[level]` of legal
  driving (L1 450 m … L5 1,500 m; only the racer's own district below L2) join **at once** and
  drive to the racer's predicted position. The level's cap (`units_per_level`) is now
  **L1 2, L2 4, L3 6, L4+ 8**; whatever nearby units don't fill spawns out of sight ahead of /
  around the racer after `response_s` (day 4–10 s, dawn 8–16, dusk 6–14, night 12–30; was
  20–40 / 60–150). Units farther than the radius + 250 m don't count toward the cap (so closer
  ones come) and are stood down after 20 s.
- Interceptors on one racer aim at different points of its predicted path (no queue for one
  junction) and keep their route at speed instead of flipping to one that starts with a U-turn
  (that flip had them crawling at 4 m/s).
- **Search**: with the racer out of sight, units that reach the last known position and the
  interceptors spread over the junctions around it (nearest, the next ones on the path it was
  taking, the roads out of the first), moving on every 10 s. Prediction while unseen uses the
  velocity at the last sighting (it used the racer's real velocity). A racer lost **inside a
  tunnel** is searched for at its portals, each unit at the one nearer to it (doctrine §5), so
  tunnels still break contact (Founders Cut below).

Units in service within 300 m of the racer (mean over runs still going), 24 m/s racer:

| t (s) | 0 | 10 | 20 | 30 | 40 | 50 | 60 | 70 | 80 | 90 | 100 | 110 | 120 |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| L1 before | 1.8 | 1.2 | 0.9 | 1.2 | 1.2 | 1.2 | 1.5 | 1.5 | 1.5 | 2.0 | 2.0 | 2.0 | 2.0 |
| L1 after | 1.8 | 1.5 | 1.5 | 1.4 | 2.0 | 2.0 | – | – | – | – | – | – | – |
| L2 before | 1.8 | 1.2 | 0.9 | 1.1 | 1.3 | 1.4 | 1.7 | 2.0 | 2.0 | 3.0 | 2.5 | 3.0 | 3.0 |
| L2 after | 1.8 | 2.4 | 2.1 | 1.7 | 2.3 | 2.5 | 2.0 | 3.0 | 3.5 | 3.0 | 5.0 | 4.0 | 4.0 |
| L3 before | 1.8 | 1.2 | 0.9 | 1.2 | 1.4 | 1.4 | 1.6 | 2.0 | 2.1 | 1.8 | 2.5 | 2.3 | 2.3 |
| L3 after | 1.8 | 3.4 | 3.3 | 3.2 | 3.7 | 3.4 | 3.0 | 4.0 | 5.0 | 2.5 | 2.5 | 2.5 | 4.0 |

Before, the racer usually escaped by 40–80 s with one or two cars anywhere near; after, L3 has
3–5 cars within 300 m from 10 s, L2 2–5 (all L1 runs had ended by 60 s: 5 captures, 3 escapes). Late columns average only the few runs
still going (most end in a capture), so they are noisy. Pursuits that start with nobody but the
first cruiser nearby (seed-dependent) fill slowest. Reckless-driving response (row 44/55) is not
worse: `police_response` noon, 10 runs: call → car within 100 m mean 12.5 s (was 13.8 s),
6 of 7 within 25 s both.

*Row 63, aggression (done; "noticeably harder").* `[chase]` in `data/police/pursuit.toml`,
by wanted level, same in both damage modes (owner rule):
- **Driving at the limit**: corners planned at 0.82 g (was 0.65), braking at 8.5 m/s² (was
  6.5), closing at the racer's speed + 25 % up to the phase cap × [1.0, 1.1, 1.2, 1.25, 1.3];
  through junctions at 18 m/s and reds at 13 m/s (was 11 / 6), 0.6 s look for cross traffic
  (was 3 s), passes after 0.5 s; interceptors at up to 2.2 × the limit. Chasing units squeeze
  past a car that only clips the lane edge and, more than 150 m from every racer, drive through
  kinematic traffic (it has no collider there).
- **Close range**: tail L1 10–18 m, L2 6–12 m, L3+ 4–8 m (was 25–40 m at every level).
- **Tactics by level**: L1 box-in/stop (box-in unlocked at L1); L2 PIT whenever the racer's
  road is straight for 40 m (speed limit 100 km/h at L2, 130 at L3; was 60 km/h on a 300 m
  radius) and rear-quarter rams otherwise; L3+ hard rams and side slams; the second unit flanks
  alongside the racer from L2; L3+ rolling blocks (interceptors that get ahead of the racer on
  its line join as rolling blocks) and **junction blocks** (an interceptor that reaches a
  junction on the racer's path first parks across it). Re-engage after 2.5 s (PIT cooldown was
  8 s). A deliberate contact costs 1 risk point (a collision 4), and the budget grows with the
  level ×[1.5, 2.5, 4, 6, 10]. Civilians within [25, 10, 6, 3, 0] m of the racer hold back
  deliberate contact.
- **Capture** also when **pinned**: within 4 m of one spot for 6 s with a unit within 15 m
  (a boxed racer rocking back and forth never met "below 2 m/s for 4 s"). Units closing on a
  stopped racer ignore other stopped police cars (they deadlocked 20 m short of the 15 m radius).
- A pursuing unit that drives into its own target is no longer counted as the racer
  "assaulting" it.

Racer at 24 m/s (86 km/h), 8 runs × 180 s per level:

| Level | | First contact, mean s (runs with one) | Contacts / pursuit-min | PIT attempts | Rams | Captured in 3 min | Escaped |
|---|---|---|---|---|---|---|---|
| L1 | before | – (0 of 8) | 0.00 | 0 | 0 | **1 of 8** (83 s) | 6 |
| L1 | after | 10.2 (2 of 8) | 1.70 | 0 | 0 | **5 of 8** (33 s) | 3 |
| L2 | before | 29.2 (1 of 8) | 0.09 | 0 | 0 | **1 of 8** (83 s) | 6 |
| L2 | after | 52.0 (4 of 8) | 1.10 | 0 | 0 | **6 of 8** (60 s) | 2 |
| L3 | before | 110.3 (3 of 8) | 0.31 | 0 | 0 | **2 of 8** (112 s) | 1 |
| L3 | after | 19.4 (6 of 8) | 1.32 | 0 | 0 | **6 of 8** (38 s) | **0** |

Racer at 16 m/s (58 km/h), same seeds:

| Level | | First contact | Contacts / pursuit-min | PIT attempts | Rams | Captured in 3 min | Escaped |
|---|---|---|---|---|---|---|---|
| L1 | before | – (0 of 8) | 0.00 | 0 | 0 | 1 of 8 (59 s) | 7 |
| L1 | after | 77.9 (3 of 8) | 0.56 | 0 | 0 | 4 of 8 (38 s) | 3 |
| L2 | before | 80.9 (2 of 8) | 0.63 | 0 | 0 | 1 of 8 (59 s) | 6 |
| L2 | after | 33.4 (3 of 8) | 1.86 | 1 | 1 | 6 of 8 (49 s) | 2 |
| L3 | before | 68.5 (3 of 8) | 0.49 | 0 | 0 | 2 of 8 (85 s) | 1 |
| L3 | after | 18.6 (3 of 8) | 1.02 | 0 | 0 | **8 of 8** (74 s) | 0 |

PIT success (racer turned > 60° or below 4 m/s within 3 s of the contact) was 0 of 1 here (0 of
7 in an earlier build): the PIT lines up and touches, but the scripted racer's follower
straightens out. Most captures are box-ins,
junction blocks and pins after the racer is slowed by traffic or a corner; a 24 m/s racer on
Port Vesper's curvy roads rarely lets the lead cruiser within 35 m on a straight. Capture times
are from the pursuit start. Contacts count episodes between the racer and any police car (L1
contacts are tailing bumps and box-ins).

*Row 61, push-through (done).* A unit with lights on, stopped behind a civilian or parked car
(not the racer, not another unit) for 1.5 s with no way past (the pass check failed or the pass
is pinned), shoves it at 3.5 m/s (13 km/h) aiming 0.8 m left, for up to 3 s; if it moved less
than 1.5 m it backs out / re-routes / respawns like any stuck unit. The shove aims straight
ahead (aiming at the unit's freshly re-planned route swung responders across the road and
stalled `wp_race` `grid_highway_sprint` and `wp_wasm` `short_sprint…` behind them). Push contacts are plain
low-speed contacts: no heat for the racer, no crash-scene incident (below 8 m/s), no assault
(the racer isn't involved). `PoliceStats::pushes` counts them.
Units with lights on stopped behind a civilian or parked car, same chase runs (24 m/s racer):

| Level | Longest single wait, before → after | Mean of each run's longest wait | Pushes (8 runs, after) |
|---|---|---|---|
| L1 | 34.0 s → **12.6 s** | 11.6 s → 7.3 s | 18 |
| L2 | 80.0 s → **14.0 s** | 27.6 s → 9.0 s | 64 |
| L3 | 155.9 s → **42.9 s** | 84.5 s → 12.3 s | 79 |

Before, a cruiser could sit behind a parked car or a stopped civilian for minutes. The total
seconds spent waiting per pursuit-minute went up at L1/L2 (15 / 39 / 93 → 27 / 50 / 57 unit-s)
because 3–6 cars now chase through traffic instead of 1–2, but waits are short. The 42.9 s one
is push → back out → re-route cycles behind a car that won't move (wedged at a kerb).

*Row 66, routing round buildings (done, partly effective).*
- Pursuers drive the racer's own line only within 70 m **with line of sight**; otherwise they
  follow the **road graph** to the junction the racer heads for, then its predicted path
  (`chase_route`), re-planned every 1.5 s or when the racer heads for another junction. A racer
  cutting through a plaza or lot is therefore met on the roads.
- **Static feelers** (`wp_police::walls`): three rays from the front bumper (ahead, ±23°)
  against every building footprint and tunnel wall, as long as the stopping distance + 6 m; a
  wall ahead caps speed to stop 2.5 m short, a wall closing in at the side shifts the line
  away and slows to ≥ 5 m/s. Parked cars and cones were already obstacles (round 12b).
- Turn-rounds (U-turn / reverse swing) only where the turning circle clears the buildings;
  otherwise stop and three-point, or carry on to a junction. Reverse manoeuvres capped at 3 m/s.
- **Bug fixed**: a physical unit's speed was synced unsigned. A unit rolling backwards "too
  fast" braked, and in reverse gear `wp_vehicle` swaps the pedals, so the brake drove it
  backwards: units ran backwards at up to 21 m/s into buildings. Speed is now negative when
  rolling backwards faster than 1 m/s (fully signed, it changed standstill creeping and a patrol
  stopped across the race highway in `grid_highway_sprint`).

Building / tunnel-wall contacts (a tall static contact within 1.5 m of a footprint or tunnel
wall) per pursuit-minute, all pursuing units, at ≥ 3 m/s (all speeds in brackets):

| Level | Before (slow police) | After, feelers **off** | After |
|---|---|---|---|
| L1 | 0.12 (1.74) | 0.62 (0.83) | **0.42 (0.42)** |
| L2 | 0.28 (1.77) | 0.88 (1.08) | **0.49 (0.73)** |
| L3 | 0.05 (1.50) | 2.70 (4.26) | **1.93 (4.37)** |

All-speed building contacts are down at L1/L2 (1.74 → 0.42, 1.77 → 0.73) and the feelers cut
fast hits by 30–45 % against the same aggressive driving, but fast hits at L2/L3 are still above
the old police, which drove at 0.65 g, were 25–40 m back, and mostly lost the racer within a
minute (shorter, slower pursuits with 1–2 cars against longer ones with 3–6 at the limit). The
remaining fast hits are corners taken at 9–15 m/s in a slide (the rays follow the heading, not
the slide) and pile-ups at the racer. Next steps if the owner wants fewer: cast the centre
feeler along the velocity too, and lower chase cornering for units more than 100 m behind.
Parked-car contacts went up (0.0–0.2 → 4–9 per pursuit-minute): that is the push-through and
the closer squeeze past parked cars at the kerb.

*Founders Cut tunnel scenarios (re-aimed, lead request).* With faster, converging police the
racer reaches L4 (30 s escape time) just before the Cut and searching units drove into the Cut
and found him (captured 138.6 s). With units now holding a tunnel's portals when the racer is
lost inside (each unit the portal nearer to it: sent to the far one, a unit drove through the
tunnel and found him), both scenarios pass: `tunnel_escape_founders_cut` lost at ≈ 101 s,
**Escaped 131.2 s** (nearest unit ≈ 130 m away at a portal); the control (`covered_los: false`)
is **Captured 129.3 s** with the helicopter overhead the whole time. The control's window now starts
at 115 s (was 130 s; police arrive sooner). Comments updated in both files.

*Notes for other workstreams.*
- **L (race)**: police are faster and converge, so AI racers that get wanted flee or get caught
  sooner. Expect `wp_race --test race_picker` `loop9_circuit` and
  `lantern_checkpoint_draws_the_police` (already failing after round 12b) to see fewer finishers;
  please re-tune the finisher counts / flee behaviour for the new police, or tell me a
  race-specific rule you want (e.g. no PIT/rams on race routes below L2).
- **P (design docs)**: `POLICE_DOCTRINE.md` §2 P1/P2/P4 and §3.3 ("contain, don't race", PIT only
  below 55/70 km/h, shadow 25–40 m) no longer match the AI after the owner's row 63 request. The
  tables are the truth now (`[chase]`): please update the doctrine (and §3.2 max units: 2/4/6/8).
- **Lead**: `PoliceStats` gained `static_hits`, `pushes`, `backup_joins`, `rams`,
  `junction_blocks`; `Tactic::Ram(RamKind)` is a new variant (captain view shows "ram");
  `PoliceSystem::begin_pursuit`, `racers_iter`, `set_walls`, `set_feelers` are new. `UnitBody::speed`
  is negative only when the unit is really reversing (below −1 m/s along its heading); otherwise
  it is the speed magnitude, as before (a fully signed speed broke `wp_race` `grid_highway_sprint`).

*Driving controller vs `lead/police-driver` (lead heads-up, round 12c).* Round 13 touched the
low-level path→controls driving only where the tactics needed it, all in `unit_input` /
`avoid.rs` and tuned from `[chase]` / `[avoid]` in `data/police/pursuit.toml`: chase cornering
speed (`cornering_g`, `brake_mps2`, `speed_margin`, `cap_scale`), chase speeds at junctions / reds /
turns (`chase_*_mps`), wall feelers (`wall_*`, `walls.rs`), the push-through phase
(`push_*`, `stuck.rs` `Phase::Push`) and reverse caps (`REVERSE_MAX_MPS` 3, `SWING_REVERSE_MAX_MPS`
5). The DriveController itself (pure pursuit + PID) is unchanged. The driver session can replace
the speed planning and feelers; the tactic layer only needs `unit_input` to keep honouring the
`Tactic` target, `AvoidState`, the shove phase and `PoliceStats::static_hits`.

*Merged and re-checked on `lead/round12` 9409249 (2026-10-20).* The tables above were measured on
the earlier base (commit `e4f8b56`); later commits: three-point turns reverse at up to 5 m/s
(`67f6de5`), the wall index shared in an `Arc` (`e6b4ab4`), and the merges of the lead's car-data
retune, testbed presets, audio impacts and touch perf pass. After the car-data retune (stiffer
dampers on every car) the Founders Cut racer is caught and PIT'd on the Uptown Connector (≈ 71 s,
L5 at ≈ 77 s, 40 s escape time); he is still lost inside the Cut (≈ 112.5 s) with the units
holding the portals, so `tunnel_escape_founders_cut` now runs 170 s (escape window to 170 s) and
escapes at ≈ 152 s. The control is still captured.

*Gate (merged branch, `3ed3415` + this doc).*
- `cargo fmt --check`, `cargo clippy --workspace -- -D warnings`, `cargo build --target
  wasm32-unknown-unknown -p wp_wasm`, `lint-determinism` (0 errors): clean.
- `cargo test --workspace`: everything passes except failures that also fail on
  `lead/round12` 9409249 without this branch: `wp_wasm`
  `crash_replay_tests::race_crash_replays_with_the_judge_and_no_crash_on_the_grid` and
  `tests::simcli_golden_replays_play_back_in_the_browser_core` (`golden_tbone` replay drift);
  `wp_race --test race_picker` `loop9_circuit` / `lantern_checkpoint_draws_the_police` (note for L
  above); and `simcli --test scenarios goldens_match`, only for goldens that drift on the base too
  (identical hashes with and without this branch: audio_sideswipe_scrape, the city hill / sidewalk
  / siren / blocked-parked-vesper scenes and the other car-data-retune drift; for the lead to
  bless), plus the two Founders Cut scenarios, which have no golden on the base either.
- All 82 scenarios in `apps/simcli/scenarios` pass their expectations (including both Founders
  Cut ones and the `city_siren_traffic_yields_*` ones that failed in round 12b).
- `crates/wp_net/tests/vesper_e2e.rs`: pass (104 s).
- Goldens: generated on `lead/round12` 9409249 and on this branch; the 21 hash files and 2 replays
  that differ were blessed in `3ed3415` (all have physical police in them).
- `just web-e2e`: 198 checks pass. Failing: `perf_check` on software GL (chase 18 frames over 250 ms, rain 21 frames and 2738 KB/frame; the perf check also failed on the base in the earlier round-13 run, and this container has no GPU — not re-run on 9409249 alone), and `menu.mjs` "countersteer slider disabled until the wasm setter exists": the lead's menu now enables the slider (the setter exists), this branch's testbed diff against the lead does not touch it, so the e2e check needs updating on `lead/round12`. (`just` is not installed here; ran `npm run wasm` + `npm run e2e`.)

*Re-check after the lead's round-12b rollup (`lead/round12` 2e40a9d0, which already merged this
branch at `3ed3415`).* After merging it, this branch's code is identical to the lead tip (only this
doc differs). fmt / clippy clean, `wp_police` tests pass, 85 of 86 scenarios pass; the one failure,
`city_siren_traffic_no_siren` (1 civilian collision, expected 0), comes with the lead tip itself.
`simcli goldens` reports 44 stale goldens on that tip (damage, panels, crash, siren, soak, race,
police, tunnel); none of them is moved by this branch, so none is re-blessed here — they need a
bless on `lead/round12`.

*Lead bug (r13-test-fixes §4): a responder's turn-round loop stalled the grid highway sprint.*
In `wp_race` `grid_highway_sprint` and `wp_wasm` `short_sprint_with_ai_…` (seed 7) unit 7 was
answering a reckless-driving call and following the reported racer at the finish junction. It
turned round toward him for about 80 s (U-turn, back out, back out, …) with its siren on, and the
civilians ahead of the racers stopped for the siren, so the race never ended. Fix:
- Turn-round tries (U-turn, reverse swing, backing out from a stopped racer) are counted per spot
  (`stuck::MAX_TURNS` 4 within 20 m). The 4th holds turning for 30 s (`TURN_HOLD_S`). A unit on a
  call hands it back (Idle, siren off) and then, like any unit that gave up, despawns out of sight
  (`PoliceStats::turn_loops`). A unit that gave up is no longer `available()` for new calls.
- A responder following the reported racer in sight keeps its siren quiet (`Unit::quiet`). Its
  driving doesn't change (a first version also dropped the lights-and-siren driving, and in
  `golden_city_chase` the follower lost the racer and no pursuit started).

| seed 7, `grid_highway_sprint` | winner | finishers | race over |
|---|---|---|---|
| before (lead/round12 1b53f4e4) | — (never judged; 125.9 s on the helper's run) | 3–4 | never |
| after | 62.0 s | 4 of 4 (62.0 / 62.6 / 99.9 / 109.2 s) | yes |
| police presence 0 (same tip) | 59.5 s | 4 of 4 | yes |

Both tests still fail their 15–60 s winner check, by about 2 s. The race is close to that limit even
without police (59.5 s), and police on the road cost a little time. P3 and P4 are held about 30 s
longer: unit 7, on another call with the siren on, turns in the finish junction from 72 to 88 s,
and after it has gone two civilians stay stopped at the stop line until about 100 s with no police
near (the traffic junction deadlock the helper sent to D). Other seeds, winner with / without
police: 3: 56.7 / 52.9 s, 11: 59.5 / 51.3 s, 23: 48.0 / 64.4 s; seed 19 never ends either way (a
captured racer parks in the lane, the helper's N(1) / E pinned-capture item). Whether to widen the
winner window or keep police off race routes is for L and the lead. Scenarios: 85 of 86 pass (the
known `city_siren_traffic_no_siren` fails on the lead tip too); the only golden moved is
`golden_city_chase` (+ replay, `[bless]`); `damage_arcade_*` and `city_siren_traffic_no_siren` fail
on the lead tip with the same hashes.

*Lead bug (opponent-pace): responders standing across the Hollins hairpins.* A responding
unit stopped angled across the hill road (pulled over for the oncoming racer mid-bend, or
turning round just after the leader passed), nose to nose with the next racer, each waiting for
the other, for 18–40 s. Fix:
- Look before turning: a unit not chasing anyone doesn't start a U-turn or reverse swing while
  another car would reach it within 4 s. Pursuers still turn at once, as `wp_sim` `police_passby`
  requires.
- A responder yielding to the oncoming racer stops only once lined up with its lane (within 15°);
  until then it creeps on at 2 m/s.
- Give way: standing still for 2 s with a slow (< 3 m/s) racer it isn't after in its way (or with
  one within 15 m while blocked by anything), the unit backs straight out 6 m. Then it waits up to
  8 s while that racer is still in front of it within 25 m (`PoliceStats::give_ways`). Each one
  counts toward the turn-round cap (4 per spot). Backing out with steering swung units across the
  road; driving on straight after backing out put them back nose to nose.

| Hollins hill sprint | seed 1 | 2 | 3 | 4 | 5 | default |
|---|---|---|---|---|---|---|
| longest stand next to a racer (≤ 20 m), lead tip 174a0dfb | — | 18 s | 21 s | 0 s | 3 s | — |
| … after | — | 3 s | 5 s | 4 s | 4 s | — |
| P2 finish, lead tip | 117 s | DNF | 118 s | 82 s | 89 s | 83 s |
| … after | 85 s | 89 s | 81 s | 77 s | 93 s | 92 s |

P3 and P4 are DNF in every run, with or without police: on the lead tip, racers 1 and 3 never leave
the grid even at police presence 0 (for N / opponent-pace). `grid_highway_sprint` now passes
(winner 43.8 s, 4 of 4). `wp_wasm --lib`: 86 of 86. **New red:** `race_picker::loop9_circuit`
(green on the lead tip): racer 4 is `Dnf(Retired)` (too damaged) at ≈ 113 s after one hard
contact, at speed, with no police unit within 60 m in the 3 s samples before. The test accepts
only the flag, the police or a racing incident. That looks like a knock-on effect in this long
police race, not a police stall, but it is red on this branch: for L / the lead to decide
whether a wrecked racer counts as a racing incident there. I didn't change L's test. Scenarios: 99 of 101 pass; `city_siren_traffic_yields_arcade/_sim` fail
with the same hash on the lead tip. Goldens moved and re-blessed: golden_city_chase (+replay),
opponent_circuit_8x5, opponent_flee_police, opponent_open_roads_traffic,
police_tunnel_escape_founders_cut_control, world_traffic_soak_observer.

*Lead items (opponent-pace / round 12 vehicle merge): Loop 9 DQ, Hollins deadlocks, passby turn.*
- **Loop 9 LeftCorridor DQ.** A unit was given a racer to chase while that racer was coming at
  it. Its new route lay behind it, so it arced across the road at walking pace into the field,
  the racer hit it and left the corridor. Now a unit whose way lies behind it holds straight in
  its lane while a car in front of it closes within 4 s (for a pursuer: a racer). It doesn't
  start a turn-round then either. A pursuer whose target comes straight at it waits for it to go
  by.
- **Hollins deadlocks.** (1) A unit without lights never passed a stopped car near a junction
  (the red-light-queue rule), even a car that was waiting for it. After 12 s it passes anyway
  (`avoid::QUEUE_DEADLOCK_S`). (2) Last resort: a unit that gave up and has stood within 3 m of
  one spot for 30 s with a racer within 20 m despawns even in view
  (`PoliceStats::deadlock_despawns`; owner row 78 allows a quick visible police reset, spawns
  still never happen in view).
- **Holding still in reverse.** The new waits braked with the brake pedal, which in reverse
  drives the car backwards: a unit backing up while it held accelerated away (`wp_wasm` capture
  tests). `hold_pedals` brakes with the throttle when rolling backwards. Other police cars no
  longer count as closing traffic.
- **`wp_sim police_passby` (Vesper, parked cop: facing the player 5.4 s after the pass, bound
  4 s).** Since round 12's vehicle changes, reverse takes about 1 s to engage from a standstill,
  and the full-lock reverse arc turned only 0.9 rad in 3 s. A standing unit with room for the
  turning circle now U-turns forward. The three-point reverse swing stays for narrow streets.
  Passes (3 of 3).

| | lead/round12 bcc57094 | this branch on bcc57094 | lead/round12 0bc27d29 | this branch on 0bc27d29 |
|---|---|---|---|---|
| Loop 9 seeds 1–6, LeftCorridor DQs | 6 (one per seed, opponent-pace's report) | 0 | — | 1 (seed 4; also on 0bc27d29) |
| Hollins seeds 1–8 finishers / races that never end | 19 / 2 (opponent-pace's report) | 24 / 1 | 20 / 1 | 18 / 1 |

0bc27d29 brought in opponent-pace's new capability table and pace changes, and they move these
seed-by-seed results a lot. On that tip this branch and the lead tip are about level on Hollins.
Seed 6 never ending is a fled racer parked in the first hairpin with the field behind it (police
are only at the tail of that queue). The default `loop9_circuit` seed panicked in `wp_opponent/src/corridor.rs:243`; fixed below. Gate on 0bc27d29 + this
branch: fmt / clippy clean; `wp_police`, `wp_sim`, `wp_wasm --lib` (86) pass; `wp_race` 6 of 7
(`loop9_circuit`, above); 109 of 109 scenarios; goldens re-blessed (14 files, listed in the
`[bless]` commit).

*Loop 9 panic in `wp_opponent` (lead, after merging 6e0ac42a).* `loop9_circuit` (default
seed) panicked at `crates/wp_opponent/src/corridor.rs:243` (`pieces.last().unwrap()`). A fleeing
racer's `plan_flight` asked `wp_world::World::plan_route` for a destination back down the same
2.8 km segment. `plan_route` returned a one-segment route whose end (s = 40 m) projects behind its
start (s = 1535 m), so every corridor piece was empty. No police behaviour hands the racer AI a
route: the police only decide which racer flees from where. The round-13 hold-in-lane change put
a racer on that segment at that moment, and the empty-route case was latent in `wp_opponent`
and `wp_world`. **Change outside my crates (`wp_opponent`, minimal):** `Corridor::try_from_route`
returns `None` for a route with no length (no segments, or every piece empty), and
`plan_flight` uses it and tries the next destination. `Corridor::from_route` keeps its contract
(`expect`). New unit test `corridor::tests::a_route_with_no_length_builds_no_corridor`. For D:
`plan_route` to a point behind the start on the same one-way segment should loop round, not
return that segment alone.

After the fix, on lead/round12 + this branch:

| | default | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 |
|---|---|---|---|---|---|---|---|---|---|
| `loop9_circuit` | pass | pass | pass | pass | pass (1 DQ) | pass | pass | pass | pass |
| `hollins_hill_sprint` finishers | 4 | 4 | 4 | 1 | 1 | 1 | 0 (never ends) | 4 | 3 |

`wp_race` (release) 7 of 7 race_picker tests and all other suites pass; `wp_opponent` (release)
passes; clippy and fmt are clean; `simcli goldens` 0 mismatches. Hollins seeds 3–6 fail the
test's "at least 3 of 4" check (the default seed passes). Seed 6 is the fled-racer hairpin
block described above.

*Hollins police seeds 3–6 (lead, after preview 0.12.22).* On lead/round12 0bc27d29 (before
this branch's last merges) seeds 3–6 already failed the "≥ 3 of 4" check: 0 (never ends) / 1 /
1 / 2 finishers. The common case: a racer stands nose to nose with a pursuing unit in the first
hairpin, the pinned capture busts it, and it then stays where it stopped, mid-hairpin, with the
field queued behind it. **Changes outside my crates (`wp_opponent`, minimal):**
- A parked or yielding car that wants to roll on (to the kerb, clear of a junction) could never
  pull away from rest: `Park`/`Yield` always held the handbrake with zero throttle below
  1.5 m/s. It now holds only when its target speed is a crawl.
- A car out of the race (busted, retired, fled) doesn't stop in a bend tighter than 35 m radius
  (`PARK_MIN_RADIUS_M`). It rolls on at 6 m/s to a straighter bit first.

Police side: the deadlock last resort treated a unit that kept backing out of a racer's way
(6 m) and nosing back in as moving. "Standing" now means within 8 m of where it stopped
(`stuck::STAND_M`).

| Hollins finishers | default | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 |
|---|---|---|---|---|---|---|---|---|---|
| lead/round12 0bc27d29 | 4 | 4 | 4 | 0 (never ends) | 1 | 1 | 2 | 4 | 4 |
| before this fix (9899d87c) | 4 | 4 | 4 | 1 | 1 | 1 | 0 (never ends) | 4 | 3 |
| after | 4 | 4 | 4 | 3 | 3 | 3 | 0 (never ends) | 4 | 3 |

Seed 6 still never ends. The busted racer now rolls on, but every bend there is tight, so it
crawls ahead of the field for about a minute. Later two more racers are captured, and the last
one is left behind a captured car at (−770, 1063). For N: a field should be able to pass a slow
out-of-race car, or a busted racer should leave the road (verge, lay-by). Gate on this commit:
fmt / clippy clean; `wp_race` (release, all suites incl. race_picker 7/7) and `wp_opponent`
pass; Loop 9 default + seeds 1–8 pass (1 DQ on seed 4, as on 0bc27d29); `wp_police`, `wp_sim`,
`wp_wasm --lib` (86) pass; 109 of 109 scenarios; goldens: `opponent_circuit_8x5` and
`police_tunnel_escape_founders_cut_control` re-blessed, 0 other mismatches.

*City-chase vehicle count, browser smoke (lead: 11 at 8c353f75 vs 17 at 9899d87c).* Not a
behaviour change. Native `wp_wasm` Session, Vesper city chase, throttle held: the vehicle list at
12 s of sim time is identical with and without the two changes (17 vehicles: player, 6 police,
10 traffic; 0 deadlock despawns, 0 give-ways). The count grows with sim time (6 at 4 s, 10 at 8 s,
16 at 10 s, 17 at 12 s) as traffic is promoted near the player. In the browser, `step()` runs at
most 60 ticks (0.25 s) per frame; under SwiftShader the page ran about 3 fps (36 frames in 13 s,
24 of them over 250 ms), so 12 s of wall time is only about 8 s of sim time. Two runs at
8c353f75 gave 15 and 13 vehicles. A smoke check that counts vehicles should wait for a sim time,
not a wall time.

*Lead requests received during the gate, not started (round 12c addenda).* Rows 75 (upside-down
spawn / safe spawns), 78 (un-wrecked units reset themselves), 80 (a witnessed violation starts the
pursuit; blatant at +20 km/h within 90 m), 84 (search reinforcements and sweep), 86 (no active unit
parked in a jam; stopped-while-active watchdog), 89 (symmetric line-of-sight range, 350–400 m by
day, ≥ 250 m at night) and 96 (oncoming units block the road). They arrived after the round-13
scope was set; they are listed here so the next police session picks them up.

**Round 12b (on `lead/round12`): owner playtest rows 44 / 55 / 60, plus the lead addenda on
un-sticking and mode-keyed behaviour.**

*Row 44, dispatch-driven spawning (done).*
1. A violation no unit saw is a reckless-driving call (civilian or traffic camera); later calls
   move it to the racer's latest position, and a report within 200 m of an open call joins it
   (a racing pack is one call). After the call delay dispatch sends an available unit within
   the district's radius *by road* that can reach a junction on the racer's predicted path first.
   Otherwise a **responder spawns** (`UnitRole::Responder`): 150–350 m of legal driving away
   (+ 6 s of a fast racer's travel), never closer than 120 m, out of every racer's sight, clear
   of other vehicles and parked cars, facing the racer; spots that meet the racer on its
   predicted path come first. Tables: `data/police/presence.toml [response]`.
2. **Never in view**: no line of sight from the racer's eye or a chase camera (behind along the
   velocity and on every side, since police know velocity, not facing), now or 1 s on, to a car
   body at the spot. Applies to responders, reinforcements, patrol refills and every despawn.
3. **Responders** drive with lights at 1.6 × limit to the meeting junction, **hold short** of it
   while the racer is coming, **follow** the racer once in sight within 150 m (so it can be
   recognised), pull over for a head-on racer and back out of a nose-to-nose stand-off. A unit
   still > 150 m away 6 s after taking the call is released and the call re-dispatched.
4. **Population**: patrols unchanged; responders and reinforcements only while ground units <
   [16, 18, 20, 22, 24, 24] for L0..L5, none where the patrol table is empty (simcli
   `police_cars: false`). Idle responders/reinforcements, surplus patrols and out-of-service
   units despawn only out of sight and ≥ 150 m away.
5. One radio line per call: "Dispatch: caller reports reckless driver, Core. Unit 6 responding."

*Row 55, measured first, then fixed (done).* Full sim, physical cars, Port Vesper v2, player at
25 m/s on random routes (`cargo run --release -p simcli --example police_response -- vesper
<hour> 20 60 25`, per-trip log in `stats().responses`):

| | 23:00 | 12:00 |
|---|---|---|
| Calls reached (police car within 100 m) within 25 s | 10 of 14 (71 %), mean 19.8 s | 11 of 13 (85 %), mean 13.8 s |
| Trips that arrived, mean | 17: start 144 m, road 176 m, 6.2 s en route | 13: 148 m, 177 m, 6.0 s |
| Trips that fell behind (racer turned away; re-dispatched) | 17: 189 m / 229 m, 7.6 s | 6: 167 m / 236 m, 8.1 s |
| Trips lost to a stuck unit | 1 (kerb) | 0 |
| Spawns / despawns visible to the player | 0 of 37 / 0 of 23 | 0 of 46 responder/patrol spawns (1 roadblock unit, a known gap) / 0 of 35 |

Grid, 22 m/s, 10 runs: 23:00 6 of 7 within 25 s (mean 21.2 s), 12:00 4 of 7 (17.4 s, 2 never).
Headless (`response_*.ron`, 8 runs, racer at 2× limit): Core noon 3.2 s mean / 7.6 s worst,
Suburb noon 9.3 / 16.8, Core 23:00 7.1 / 17.6, Suburb 23:00 19.3 s mean (6 of 8 within 25 s).
**Target ≥ 90 % within 25 s: met headless and nearly by day in Port Vesper (85 %), not at
night (71 %).** The misses are trips falling behind a player at 90 km/h who keeps turning
(responders meet the predicted path; a random route beats the prediction).

*Lead addendum, un-sticking (done).* `crates/wp_police/src/stuck.rs`: a physical unit that wants
to move but stays < 1 m/s for 2 s (or waits 20 s behind a stopped racer or police car) is
logged with a cause from its last contacts (`Damage`, `Vehicle`, `ParkedProp` via the city's prop
footprints, `JunctionStep` / `Kerb`, `Wall`, `Terrain`, `NoContact`), backs out 4 m with
opposite lock if there is room behind, re-routes around the segment on a repeat (never across
a queue), and despawns out of sight after 8 s (its call goes out again). Parked cars and cones
are now obstacles (follow / wait / pass). Port Vesper, 20 runs × 60 s, stuck events:

| | 23:00 | 12:00 |
|---|---|---|
| First measurement (no prop avoidance, no recovery; 10 runs at 22 m/s, doubled) | 1,066 (props 642, terrain 224, no-contact 104, wall 56, kerb 38) | 2,344 (terrain 918, props 902, wall 250, no-contact 140, kerb 112) |
| Prop avoidance, recovery off | 329 (terrain 165, wall 85, kerb 44, no-contact 29, vehicle 6) | 712 (terrain 411, wall 143, no-contact 101, kerb 29, vehicle 28) |
| Prop avoidance + recovery (final) | 48 (terrain 22, wall 13, kerb 11, vehicle 2) | 105 (terrain 51, wall 31, kerb 11, vehicle 12) |

Not mine and still there: terrain (high-centring / steep grades, mostly on the hill road) and
junction-step / kerb contacts (`lead/fix-junction-holes`, row 51). Durability is B's (row 48).
`set_unstick_recovery(false)` keeps the logging for these measurements.

*Lead addendum, behaviour keyed by damage mode (done).* `pit_max_kmh` is one value, 60 km/h
(between the old 70 Arcade / 55 Simulation and close to real PIT policy limits). The rest of
`wp_police` keyed by mode is damage only: crash / contact health per (km/s)², total-loss health
and `disengage_health` (the health at which a damaged unit pulls over), as the lead allowed.

*Row 60, "I sped right by a cop with no chase" / "the cop doesn't turn round" (done).* Cause:
at night a pursuit needed the racer within 40 m for 4 s; a car passing at 150 km/h is inside
40 m for ~2 s. New `heat.blatant`: speeding ≥ 40 km/h over within 60 m of an in-service unit
with line of sight is recognised at once (L1 at least, pursuit by that unit, radio "just blew
past me at 56 over, I'm on it"); busy, parked and responding units react. Physical units with
lights on whose target is behind them turn round: a forward U-turn at full lock from speed
(handbrake pull), a reverse swing from a standstill (brake held engages reverse). Shadowing
units far behind close at the phase's pursuit cap instead of the racer's speed + 10 % (a cop
200 m behind a stopped racer crawled at 10 m/s). `crates/wp_sim/tests/police_passby.rs`: racer
passes a patrolling or parked cop at 120 km/h in the next lane, going the other way, grid and
Port Vesper, 12:00 and 23:00: pursuit within 3 s of the pass, the cop facing the racer within
4 s, within 150 m of it 15 s after the pass; a 40 km/h pass starts nothing (unless the player
hits a police car, which is an assault).

*Not done this round* (done in round 13, above): row 61
pushing civilians aside, row 63 pursuit aggression tuning (only the gap-closing fix above),
row 64 backup converging on a pursuit, row 66 static-geometry avoidance and routing round
buildings.

*Effects on other workstreams' tests (requests).*
- **L (race)**: city races now draw police within seconds and AI racers flee or get caught.
  `wp_race --test race_picker`: `loop9_circuit` (1 of 6 finish, 5 pursuits) and
  `lantern_checkpoint_draws_the_police` (1 of 4 finish, an AI racer fled) fail;
  `hollins_hill_sprint` and `grid_highway_sprint` pass (the latter fails on `lead/round12`).
  Re-tune the finisher counts / AI flee behaviour for faster police, or tell me a race-specific
  police rule you want.
- **Lead (`police/tunnel_escape_founders_cut*.ron`)**: ground units now reach the racer parked
  in the Cut sooner: the escape case is captured at 133 s (fails on `lead/round12` too, "no
  pursuit ended"), the control is captured at 128.8 s, just before its 130–180 s window.
  Re-aim with the probe.
- Unchanged failures that are on `lead/round12` itself: `soak/traffic_soak_observer`
  `traffic_agents_min` (313 vs 310 on the base), `wp_sim --test sidewalk` (Vesper),
  `wp_wasm` race tests (2), and the goldens that already drift there.

Changed tests (mine): `night_is_slower_than_day_…` night floor 60 → 25 s (mean 126 → 33 s, still
≥ 3× day); `risk_budget_and_caps_keep_cops_alive` now checks the self-wreck bar and that the risk
budget is spent (the old harm comparison was within noise: 462 vs 475 on `lead/round12`).
**Round 12b gate** (`ws/police` = `lead/round12` (edeebdc) + this work + two `[bless]` commits):
fmt clean, clippy `-D warnings` clean, wasm build OK, `cargo test --workspace --no-fail-fast`:
945 passed, 6 failed, 19 ignored. Mine: none. The 6:
- `wp_race --test race_picker`: `loop9_circuit`, `lantern_checkpoint_draws_the_police`
  (faster police; request to L above).
- `simcli --test scenarios` / `goldens_match`: goldens that already drift on `lead/round12`
  (the 17 this work moves are re-blessed; a run of `lead/round12` itself was the reference).
- `wp_sim --test sidewalk` (Vesper), `wp_vehicle --test handling_balance`
  (`front_drive_cars_understeer_mildly`), `wp_wasm` `race_tests::a_capture_mid_race_…`: all
  fail the same way on `lead/round12`.
- `wp_net --test vesper_e2e` (4 players) passes. `simcli run apps/simcli/scenarios` (release):
  71 pass; fails: the two Founders Cut scenarios (above) and `soak/traffic_soak_observer`'s
  agent count (as on the base).
- `police_bench`: 0.055 ms per 60 Hz frame (was 0.074 ms; budget 0.5 ms).

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


### Assist presets merged with `lead/round12` (2026-10-20)
`ws/police` carries `origin/lead/round12` (the lead had already merged the presets there; its
`main.ts` resolution is kept) plus two testbed fixes on top:
- Touch looks presets up as a gamepad (`presetSource`), which fixes round 12's three `tsc` errors.
- A box tick or preset click pushes its state into the game menu without the menu rebuilding the
  assists from device defaults (`tellMenu`). Before, Street + ticking ESC rebuilt to exactly Rookie
  (keyboard default countersteer 0.6), so `e2e:presets` failed. An assist changed in the menu itself
  still rebuilds and makes the preset "custom"; a preset click updates the menu's toggles.
- `cargo fmt` (module order in `wp_vehicle/src/lib.rs`, from round 12).

Checks: `e2e:presets`, `e2e:touch`, `tsc`, fmt, clippy, wasm build pass. Fail identically on the
`lead/round12` tip (not this merge): `e2e:menu` "countersteer slider disabled until the wasm setter
exists" (the setter exists now); `wp_vehicle` `roster_diff_hashes_are_pinned`,
`front_drive_cars_understeer_mildly`; `wp_wasm` `race_crash_replays_with_the_judge_and_no_crash_on_the_grid`,
`simcli_golden_replays_play_back_in_the_browser_core`; 52 simcli goldens on the round-12 tip of 2026-10-20 (same set with and without this merge; `ws/police` before it: 19; none re-blessed here).


## Round 12c (lead rows 75 / 78 / 80 / 84 / 86 / 89 / 96) and the `origin/main` merge — in progress
Base: `origin/lead/round12` @ `d6baeeec` + `origin/lead/main-sync` (`origin/main` @ `3dc962b`).

### Merge of `origin/main` (P8 pro driver)
`ws/police` merged `origin/lead/main-sync`, so it contains `origin/main` @ `3dc962b`
(#18 P8 and the rest). Every non-police file is the helper's resolution. The wp_police
resolution replaces the provisional one:
- `unit_input` runs round 12's manoeuvres for every unit, lights on or off. Only the plain drive
  at the end differs: lights on → `pro::ProDriver` (`PoliceSystem::pro_drive`), lights off → the
  lane driver. In the provisional merge the siren U-turn / swing / push-through block sat in the
  lights-off path and was dead code.
- The pro driver gets the command speed capped by obstacle awareness and the static feelers;
  a pass or a wall shift is driven as an exact offset.
- The pro driver's own stuck reverse is off (`stuck_s` = ∞ at creation): `stuck.rs` owns
  recovery. Its "facing away → crawl round" stays.

Checks on the merge: fmt, clippy, wp_police, wp_sim, wp_race (`hollins_hill_sprint`,
`loop9_circuit`) and wp_opponent (`live_police_chase…`, P8's `police_pace`) all pass. simcli
runs 113 PASS. The six goldens `lead/main-sync` left open and the four round-12c police goldens
were re-blessed. The one red test is `wp_race` `pace_scenarios_hold_their_ranges`:
`opponent_pace_lantern_checkpoint_s50` never finishes (57 rejoins). That scenario has the police
off, so it comes from combining the newer `lead/round12` with `main-sync`'s world / opponent
changes, not from wp_police. Same for `wp_wasm`
`race_tests::short_sprint_with_ai_runs_to_results_and_pays_through_the_economy`: two AI cars
stop for good at (10.8, 355.3) on `grid_highway_sprint` from 60 s on, neither wanted nor near a
police unit.
`origin/main` has moved on since `3dc962b` (#19–#27: world viaducts and ramps, vehicle catch
steering, Loop 9 greybox). Of those changes only `crates/wp_police/scenarios/day_core_speeder.ron`
touches police. Those commits are left to the main-sync helper.

### Rows 80 + 89: witnessed violations, symmetric sight (done)
- A unit that watches a violation that makes the racer wanted starts the pursuit at once, day or
  night. The blatant pass-by threshold is 20 km/h over within 90 m (was 40 km/h within 60 m).
  A unit already on another racer's pursuit keeps it; backup comes for the new racer.
- Sight: 380 m by day, 320 m at dawn and dusk, 260 m at night, capped by weather visibility. A
  200° forward view plus 60 m all round. Recognising a wanted car takes 0.5–1.0 s within 150 m
  and up to 2 s more at the edge. Out of sight the timer drains (0.5/s) instead of resetting.
- Pass-by matrix (`wp_sim` `speeding_past_a_cop_at_20_40_80_over_starts_a_chase_within_1_5_s`;
  +20/+40/+80 km/h, stationary and oncoming unit, day and night, grid and Vesper):

  | | Before | After |
  |---|---|---|
  | Cases failing | 12 of 24 | 0 of 24 |
  | +20 km/h over | never pursued (8 cases) | −1.9 to +0.14 s from the pass |
  | +40 km/h over, Vesper | pursued 3.3–3.6 s after the pass | within 1.5 s |

- Night `pursuit_start_s` metric:

  | Scenario | Before | After |
  |---|---|---|
  | `night_core_speeder` | 33.0 s | 14.6 s |
  | `day_core_speeder` | 8.8 s | 8.4 s |
  | Night / day ratio | 3.75 | 1.74 |

  Expectations re-aimed to night > 10 s and ratio ≥ 1.5.
- A racer who stays out of sight still gets away. `escape_break_contact_night.ron` (new, at
  02:00): before, no unit even spotted it; after, one pursuit, escaped.
- Fixed on the way: a far pursuer flipped between Pursue and Intercept every think step (two
  track projections disagreed), so it never moved (capture_stop seed 401).
- Gaps: there is no racer headlight state yet, so a lights-off racer is not harder to see at
  night. The HUD is unchanged.

### Row 78: reset in place (first part, done)
An un-wrecked AI unit that stands stuck while it wants to drive resets in place.
- Stuck time: 8 s on a pursuit, 15 s otherwise, 40 s behind civilian traffic.
- Never: at a red light, queued within 25 m of a junction (unless the stuck detector flagged
  it), holding a block, or over a stopped racer it is arresting.
- It snaps to the nearest clear lane spot within 15 m along its route, upright, damage kept, and
  is a 1 s ghost in `wp_sim`. If a racer can see it, it waits 2 s more first.
- No clear spot: it despawns out of sight, or in view after 30 s wedged next to a racer.
- Logged in `PoliceStats::resets`: 0–9 per simcli scenario, all short hops.
- Test: `stuck_unit_resets_in_place_within_15_m`.
- To do: count per pursuit-minute and the scenario metric.

The reset fixed the Hollins race deadlock this round's sight change exposed: a column of units
and two captured racers were jammed in a hairpin.

### `lead/main-sync` merge: police reds fixed (2026-10-21)
Merged `origin/lead/main-sync` (main's rebuilt grid: divided viaduct at x = 0, diamond ramps
with terminals at x = ±22) twice (51902403, then its new tip in ead38c0f). Police fixes:
- **Nose-to-nose unit deadlock** (radio_chatter capture never happened): two units stood face
  to face at a ramp terminal for a minute. After 3 s the less busy one (not chasing; else the
  higher id) resets, past the other unit only. Facing decides, not `blocked_by` (the other's
  passing attempt flips it on and off). Headless units now count standing time too.
- **Patrol nose to nose with a stopped racer** (wp_race hollins_hill_sprint, all captured): a
  patrol met the racers' queue head-on on a hairpin. After 3 s it turns round: reset into the
  other lane facing the other way (up to 15 m back, on the road, clear) with a new patrol.
- **Wide U-turn rejoin** (wp_sim police_passby Vesper, 174 m > 150 m): the U-turn ended 9 m off
  the centreline and the chase route began with a sideways leg to the junction beside the car,
  so it looped across the road. A physically driven unit past the road's edge (lanes × width
  + 1.5 m) now joins at the first route node that is not beside it (cos < 0.5) within 2× its
  offset; the pro driver's in-lane look-ahead is at least 2× the distance off the lane. Headless
  units keep the old route start (with it `self_wreck_sim` rose 17 → 20 wrecks).
- First patrol of a shift is silent (no 13 × "resuming patrol"); lost-visual sustain 4.5 s;
  callsign lists over 4 say "N units".
- Re-aimed: `escape_break_contact`, `response_*` (wp_police scenarios); simcli
  `police/sideswipe_patrol_{day,night}` (a front-corner clip then flank scrape with a patrol
  driving at the racer; the assault rule still starts the pursuit in the contact tick).
  `night_is_slower` pursuit ratio ≥ 1.25 (1.39 on the new grid).
- Tests adjusted: `blocked_siren…` frees the other units; `presence_matches_table…` expects a
  quiet shift start; wp_wasm siren-audio test starts the player wanted (the full-throttle player
  now stops at a ramp terminal 20 s in; test-only `Session::sim_mut`).
- [bless] `police_sideswipe_patrol_{day,night}`, `police_tunnel_escape_founders_cut{,_control}`.

Red on plain `lead/main-sync` too, not police (left to main-sync):
- `wp_net vesper_e2e`: racer 0 falls through the ground at its spawn (z 2 → −60 m at step
  ≈ 1000, (491, −782)); the cop can't follow, so the pursuit ends Escaped.
- `wp_race opponent_pace` grid_highway_sprint s50/s95: 0 finishers; police are off in those
  scenarios (start grid on the rebuilt highway).
- `simcli city/tunnel_founders_cut_drive_through` path length 299 m (< 380).
- web e2e `run.mjs` perf budgets under software GL (409k triangles > 400k, 323 draw calls > 300
  on phone, 2.3 fps): main's heavier city geometry. `siren.mjs` audio frame 5–7 ms with sirens
  on this container (1.6 ms in another run): load-dependent. The siren mode-change check was a
  test bug (forcing the pattern the unit already played) and is fixed here.
- After merging main-sync's newer tip (ead38c0f), also red on plain main-sync d1a3f64f:
  wp_race `hollins_hill_sprint` (a responding unit stops head-on in the hairpin to let its
  target pass; the queue behind it is arrested while stopped), wp_wasm `run_tests` ×2 (the
  "parked" player reverses away with the brake held, so nothing is captured) and
  `race_tests::short_sprint…`, wp_opponent `capability_table` (car data re-measure).
- 44 goldens mismatch identically on main-sync (same list with or without this branch); only
  the four police ones above are blessed here.

### Keep-up round 2: corner exits and turn-round (2026-10-21)
16 seeds × 12 cells each:

| | lead m | pace | capture | capture s | escape | crash/min | civ/min | stuck/min |
|---|---|---|---|---|---|---|---|---|
| baseline d3c7408c | 62 | 0.68 | 79 % | 54 | 14 % | 1.00 | 0.29 | 3.59 |
| + turning-round fix (6f5b0f24) | 60 | 0.78 | 82 % | 61 | 14 % | 1.05 | 0.34 | 3.33 |
| + turn_around_mps 4 -> 6 (not kept) | 62 | 0.76 | 81 % | 56 | 13 % | 1.26 | 0.37 | 3.44 |

- **Kept:** the pro driver's 4 m/s "turning round" crawl fired whenever the lane point 8 m on
  was > 70° off the nose, i.e. at the apex of every right-angle corner; it now also needs the
  car to point away from the road where it is. Pace 0.68 → 0.78.
- **Not kept:** a faster turn-round crawl (6 m/s): no gain, +20 % crashes.
- Traces (`WP_BENCH_TRACE`, new `branch` / `pro_dbg` tags): the nearest pursuer far behind
  (> 40 m) is slow mostly in the plain drive with no limit in force: genuine turn-rounds at
  the 4 m/s crawl (≈ 40 % of those), corner speeds and acceleration lag; push-throughs and
  U-turns make up most of the rest. Lead gap stays ≈ 60 m: the next lever is the
  turn-round itself (a quicker manoeuvre, not a higher crawl speed) and prediction.

### Lead's main-sync blockers (2026-10-21, on main-sync cd6cd320)
- **wp_wasm `run_tests` ×2** (nothing settled in 60 s): not police routing. The test held a
  full brake on the "parked" racer; at a standstill that auto-reverses, and on main's grid the
  racer backed away down a diagonal avenue at 17 m/s (the units on diagonal headings were
  chasing it). The tests now hold half brake, like `career_tests::brake_until_settled`.
- **wp_race `hollins_hill_sprint`** (all four `Dnf(Captured)`): partly police, partly
  legitimate. Police side: a responding unit held head-on in the single-lane hairpin for its
  target; the target stopped in front of it and was arrested, then the same unit arrested
  the racers queued behind, one after another (the 30 m pinned rule took them too). Fixed:
  the capturer is busy booking for `ARREST_BUSY_S` (30 s; no other capture, no new
  pursuit/intercept) and the pinned rule needs a unit chasing that racer. Legitimate side:
  with opponent-pace phase 3 every racer speeds and runs reds past the hill's patrols in the
  first 20 s (3 + 3 witnessed violations, L1) and a pursued L1 AI racer yields at 6 m/s, so
  the police can still take out the whole field. The test now accepts that (race ended,
  nobody wrecked); other police races still need a winner.

### Keep-up and predict (owner: "cops can't keep up", "don't predict fast enough") — 2026-10-21
Measured with `simcli police-bench`, which now has two keep-up columns (`lead m`: distance
to the nearest chasing unit while police see the evader; `pace`: that unit's speed ÷ the
evader's while the evader is above 90 km/h). 16 seeds × 12 cells = 192 runs each:

| | lead m | pace | capture | capture s | escape | crash/min | civ/min | stuck/min |
|---|---|---|---|---|---|---|---|---|
| before (a0678db4) | 61 | 0.68 | 85 % | 60 | 10 % | 1.06 | 0.34 | 3.27 |
| after (this push) | 62 | 0.66 | 81 % | 53 | 12 % | 1.00 | 0.28 | 3.03 |

By cell: rain pace 0.39 → 0.67, day 0.41 → 0.36; everything else within noise (8-seed runs
of the same config swing pace by ±0.1). **Keep-up did not move.** What changed:
- **PR #33 ported** (backup waits past a junction on the racer's path, facing its way, and
  joins as it comes level: no 180). Gated to a racer police can see, one new ambush per racer
  per 45 s (back-to-back ambushes kept a fast racer in sight forever), joins once level.
- **Chase junction speeds** 18 → 24 m/s straight, 13 → 17 at reds (28/20 doubled headless
  self-wrecks in `self_wreck_sim`, the < 10 % spec bar).
- **Row 86 (first part):** a chasing unit held under 3 m/s behind civilian traffic for 3 s
  routes around that road segment (test `chasing_unit_stuck_in_a_civilian_queue_routes_around_it`).
  A right-shoulder pass was tried and dropped: it drove a unit into a wall.
- Pursuit caps 40/36/32 → 52/48/46 m/s: no measurable effect, not kept.
- Headless run-wide crash only when over the grip (units "crashed" at 5 m/s at main's ramp
  terminals); a patrol in a nose-to-nose unit deadlock may turn round.
- Night/day test now compares the first backup per pursuit (`first_backup_s`): the old mean
  over every join mostly measured how late into long chases units still joined.

Where keep-up is lost (traces with `WP_BENCH_TRACE`): when the evader is fast the nearest
chaser is usually coming out of a turn-round, a ram or a turn at 3–10 m/s; rain queues held
interceptors 10–20 s (row 86 above). Next: corner-exit acceleration and turn-round time
for physical units (pro driver), and predicting where the racer goes (the ambush only covers
backup).

### Rows 131, 133, 140 and spike deflate (2026-10-22, on round12 9b686f76)

**Row 131 — no spawn where a racer will see it.** Every spawn (pool, responder, backup,
ambush, roadblock and spike cars) is rejected where a racer sees the spot now, from its chase
camera, 1 s on, or from points on its predicted road over the next `ROUTE_VIEW_S` = 3 s
(every 15 m; the horizon grows with speed, above 5 m/s). Spawns fall back to the next
candidate. Roadblock and spike cars wait (`PendingToolCar`) until their spot is hidden: the
tool is placed at once, the car appears later (on a long straight possibly only after the
racer has passed). Despawns and resets keep the plain check. Test:
`no_unit_spawns_in_a_fast_racers_view_now_or_3_s_ahead` (40 m/s racer; seeds 1, 7, 14 each
had 1-4 such spawns before).

**Row 133 — tools on the racer's carriageway.** `RoadGraph::carriageway_m`: on a one-way
segment lane 0 is centred on the line and the lanes extend right, so the carriageway is
[-w/2 - 0.5, (n - 0.5)·w] right of the line (0.5 m slack: 16 of 530 grid and 58 of 3044
Vesper one-way segments put lane 0 at -0.92 m). The spike car parks 2.5 m beyond the right
edge facing the carriageway's flow. Before, it parked 7.5 m left, which is on the far
carriageway. The strip spans the carriageway only, and roadblock barriers and cars are
centred on it. Two-way roads: the strip is unchanged (7 m, centred) and the spike car parks
on the right shoulder. Test: `spike_car_and_strip_stay_on_the_racers_carriageway_on_divided_highways`
(grid 4 strips + 3 roadblocks, Vesper 3 + 3; heading within 30 deg, racer's side of the
median, strip width = carriageway; the old layout fails it).

**Row 140 — nothing on the far carriageway.** A racer's corridor is its predicted road
900 m ahead plus its legal road 500 m back. A one-way road within 30 m of the corridor and
running the other way is its far carriageway.
- Responder, backup and ambush spawn spots there are rejected.
- Backup spawned at a node on a one-way road faces its flow.
- A unit already on the far carriageway is scored for backup by its legal drive (round by
  the next interchange or turning point), not the straight line.
- It is never "close" to a racer across the median (`opposed_carriageways`), so it routes
  round instead of shadowing from the other side.

Test: `no_pursuit_spawns_on_the_far_carriageway_and_routes_stay_legal`, a 40 m/s racer chased
60 s from 12 starts per city, with one patrol per district so backup must spawn (grid 78
pursuit spawns and 3507 routes, Vesper 153 and 2662). It asserts no pursuit spawn on the far
carriageway and that every pursuing unit's route never runs the wrong way along a one-way
road or hops carriageways away from a junction. With the old spike-car layout, grid starts 3
and 6 fail it. With rows 131 and 133 in, the far-carriageway spawn rule rarely fires in these
runs: the far carriageway is in the racer's view.

**Row 132 — spikes deflate in 1 s** (`data/police/tools.toml` `deflate_s` 5 → 1). The spike
system test now times the partial deflation from the hit.

**Bench** (`simcli police-bench --seeds 16`, 192 runs; "before" = this tree with the
look-ahead off, tool cars spawning at once, the old spike-car side and the far-carriageway rule
off):

| | capture | escape | pace | lead m | crash/min | pop-in/min | far-cw/min |
|---|---:|---:|---:|---:|---:|---:|---:|
| before, all | 82 % | 11 % | 0.45 | 63 | 1.17 | 0.82 | 0.00 |
| after, all | 83 % | 12 % | 0.70 | 63 | 1.03 | **0.06** | 0.00 (1 spawn) |
| before / after, grid | 73 / 78 % | 11 / 14 % | 0.59 / 0.80 | 68 / 70 | 1.34 / 1.17 | 1.95 / 0.12 | 0 / 0 |
| before / after, Vesper | 87 / 86 % | 12 / 12 % | 0.38 / 0.42 | 59 / 57 | 1.08 / 0.95 | 0.17 / 0.03 | 0 / 0.01 |

`pop-in/min` counts police cars (tool cars included) that appeared in a racer's line of sight
or whose spot a racer saw within 3 s (`SpawnRecord::seen_after_s`). `far-cw/min` counts pursuit
cars that appeared on the far carriageway (`SpawnRecord::far_carriageway`). The one remaining
far-carriageway spawn is in "vesper night clear L3 good". Rerunning that cell alone does not
reproduce it, so it is still open. The 6 % of pop-ins left are spawns hidden at the time
whose spot came into view within 3 s from somewhere the look-ahead does not sample (off the
predicted road).

**Ranges re-aimed, all approved by the lead:**
- `escape_break_contact_night`: escapes max 3 → 4 (chases diverge once tool cars stop popping
  in: 25 → 0 in-view tool-car spawns over 10 runs).
- `response_day_suburb`: call→unit cap 25 → 28 s (responders spawn past the 3 s look-ahead:
  23.4 → 26.6 s over 3 calls).
- `golden_spike_strip`: speed at 5 s 7–11 → 13–20 m/s (on its rims when it brakes; 16.3 m/s).
- `police_bench_light`: "not captured in every run" became "median capture ≥ 15 s after the
  pursuit start". The one run that used to time out still in pursuit (not an escape) is now
  captured, at 25–78 s.

**Goldens** (blessed in their own commits): `tunnel_escape_founders_cut` (+ control) on round12
f850d6b4. On 9b686f76: `golden_spike_strip`, which moves with `deflate_s` only (its hash is
unchanged at 5 s), and `jam_pursuit_grid`, from 20 s of its L4–L5 pursuit (spawn and tool
rules). Every other golden passes on the merged tree.

### Turn-round, chase-unit map marker, far-carriageway repro (2026-10-22)

**Where turn-round time goes.** Bench columns `turns/min` and `turn s` (3d1daa45) time each turn-round of a unit after the evader: the pro driver facing more than ~73 deg from its line, a reverse swing or U-turn, or backing out. Baseline: about 51 unit-seconds of turning per pursuit minute, most of it in turns of 3 s or more. With the racer moving, a 3-seed tally split the time into:
- U-turns while rolling: about 370 s.
- The pro driver's crawl round at its 4 m/s cap: about 240 s.
- Standing, blocked by traffic or another unit: about 250 s.

**Change:** the crawl cap goes from 4 to 5.5 m/s (`data/police/pursuit.toml` `turn_around_mps`, and the matching code default). Tried and not kept (6 seeds each):
- U-turn 6 → 7.5 m/s: turning 52.1 → 49.9 s, no gain.
- Swing/U-turn from 90 deg instead of 107 deg: 54.6 s, stuck 3.58 → 4.15/min.

| 192 runs | lead m | pace | capture | escape | crash/min | stuck/min | turning s per pursuit min | turns ≥ 3 s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| before (3d1daa45) | 65 | 0.75 | 82 % | 13 % | 1.18 | 3.89 | 50.7 | 1586 |
| after (crawl 5.5 m/s) | 62 | 0.53 | 83 % | 14 % | 1.20 | 2.86 | 46.6 | 1279 |

Pace is the median over different, smaller sample sets (588 → 371 samples) and moves opposite ways by city (Vesper 0.46 → 0.54, grid 0.78 → 0.50), so it is within its known noise. Turning time, long turns and stuck events clearly drop.

Goldens blessed: jam_pursuit_grid, jam_pursuit_vesper, weather_fog_pursuit, and police_tunnel_escape_founders_cut with its control. Founders Cut's helicopter now launches at 90.2 s; its bound moved from 80 to 95 s (still before the Cut).

**Chase-unit map marker (row 129).** A chasing unit routing round by the roads now shows where its route meets the racer's path: the junction the racer heads for (`Unit::chase_route`). It is drawn in policeB with a dotted line and a diamond, while an interceptor keeps its dashed line and circle. It is shown only while the unit routes; a unit right behind the racer shows none. This is minimal glue in `wp_wasm/src/map.rs` and `apps/ui-kit/src/map/draw.ts`. ui-kit's `data.test` top-speed failure is pre-existing.

**Far-carriageway spawn repro (row 140).** It reproduced with the 192-run sequence on 36a31b1f, vesper night L3: a spike car that had waited for its spot to be hidden spawned 285 s in, with the racer 360 m away on another road. From the racer's road at that moment, the spot was its far carriageway. A waiting tool car whose spot is on the racer's far carriageway is now dropped. On the current base the full 192-run sequence has no such spawn.

### Row 84: searching (2026-10-22)

Owner: "Searching doesn't work well enough: more cops should spawn and go to the site."

- **Work the search from the last known spot.** While a racer is lost (Searching), units on it are counted, and far ones stood down, relative to its last known position (`search_centre`). Before, they were measured against where the hidden racer really was, so units sent to search were released as "far".
- **More units.** `search_extra_units` = [1, 2, 2, 3, 3] more per level L1..L5 while lost (`data/police/presence.toml`).
- **Search backup spawns around the last known spot**, along the way it was heading (`response_spawn_point`), and is still never in the racer's real view.
- **A growing sweep.** Search points start 150 m from the last known spot and grow by 0.5 × its last speed (at least 12 m/s) × the time unseen, stepped every 10 s, up to 700 m. Its predicted exits come first, then junctions nearest first, up to 16 points handed out to units in turn.
- **Re-acquire** stays as it was: any unit (or the helicopter) seeing the racer resumes the pursuit at once.

Bench columns: `re-acquire` (search episodes ending with the racer seen again) and `search units` (cars within 300 m of the last known spot while searching).

| 192 runs | capture | escape | re-acquire | search units | crash/min | stuck/min | night escape |
|---|---:|---:|---:|---:|---:|---:|---:|
| before (a981ad2d) | 79 % | 12 % | 95 % | 3.8 | 1.47 | 4.27 | 17 % |
| after | 82 % | 9 % | 96 % | 3.8 | 1.39 | 3.82 | 12 % |

`search units` does not move: searches are short (95 % end re-acquired) and the extra units take their 4–16 s response time to spawn and drive in. Escapes drop by a quarter.

Re-aims:
- `response_day_suburb` max call→unit 35 → 38 s. It went 34.2 → 37.1 s, the worst of 3 calls, and is 34.6 s with the extras off. Reserving 2 cap slots for calls did not change it, so the cause is the extra requests' RNG draws, not a cap squeeze.
- Founders Cut escape + control re-seeded 51 → 53. The extras moved seed 51's loss to 64.5 s, and he escaped before the helicopter. Over seeds 50–63 the story now holds on 12 of 14, both scenarios on 7.
- Goldens blessed: the jam pair and the Founders Cut pair.

### Row 86: active units standing still — audit and two levers tried (2026-10-23)

Owner: "cop not moving (stuck in the row-85 jam during an L5 pursuit)". The first part (a chasing unit held behind civilian traffic re-routes round that segment) shipped earlier.

**Audit (19dbe120, kept).** `PoliceStats::active_stops` logs each episode in which an active unit (pursue, intercept, respond) stood under 1 m/s for over 5 s with no reason to. Reasons that excuse a stop: booking an arrest, blocking a junction, or its racer stopped within 40 m. Each record has what held the unit; the bench shows `stops>5s/min` and `stop s`. Baseline on round12 4d086b65, 192 runs: 1.55 stops per pursuit minute, median 6.9 s, 3231 s in all.

| held by | stops | time |
|---|---:|---:|
| civilian traffic (kinematic) | 162 | 1216 s |
| another police unit | 112 | 959 s |
| nothing recorded | 96 | 728 s |
| a vehicle | 35 | 323 s |

Traces show these are recovery episodes, not units parked for good. Typical cases:
- Two units nose to nose. Both back out and re-route; the existing deadlock reset follows after 3 s, plus 2 s if the racer can see them.
- The queue behind such a pair.
- Push-through stalls behind traffic (10–17 s).
- Queues behind traffic even after re-routing.

**Tried and reverted (192 runs each):**

| | stops>5s/min | stop s | crash/min | capture |
|---|---:|---:|---:|---:|
| baseline | 1.55 | 6.9 | 1.20 | 82 % |
| head-on pairs: lower id keeps its route, the other backs out | 1.58 | 6.9 | 1.19 | 82 % |
| + active-unit reset 8 → 5 s, responders included | 1.31 | 6.2 | 1.54 | 79 % |
| queue pass: pass a stopped queue (≤ 8 cars, 120 m, cars ≤ 9 m apart) in one go | 1.70 | 6.9 | 1.34 | 78 % |
| queue pass + no push-through into a queue (un-stick at once instead) | 1.46 | 6.6 | 1.25 | 78 % |

The head-on rule changes nothing: the existing deadlock reset already resolves those pairs. The 5 s watchdog cuts long stops by 15 %, but police crashes rise by 28 % and captures drop, so it is not worth it.

The queue pass (2026-10-23, round12 314c01d1) passes the whole queue in the oncoming lane instead of one car at a time. Alone it is worse on every count: more time in the oncoming lane means more head-ons with traffic coming the other way, and those stops are traffic-held (201 vs 162). With no push-through into a queue, stops fall 6 % (traffic-held time 1216 → 1130 s), which is not clearly lower. Capture drops 4 points and crashes rise. Both were reverted (never pushed).

Still open in row 86:
- red lights as a yield under siren: already in place (`avoid::junction_speed`: with lights a unit slows to `siren_red_mps`, gives way to cross traffic for at most `yield_max_siren_s`, then goes; chasing units use `chase_red_mps` / `chase_yield_max_s`). The audit does not yet say how many "nothing recorded" stops are at lights;
- using the shoulder when queued (an earlier shoulder pass drove a unit into a wall); the opposite lane is tried above;
- making push-through recover faster behind traffic.

### Row 75: a cop spawned upside down in view (2026-10-23)

Owner: "a cop car spawned upside down in the road, in view". Spawn spots (pool, responder, backup, ambush and tool cars) were checked against moving and standing vehicles, but not against the world's parked cars and cones. A unit could be placed on a parked car and thrown over by the physics.

- **Spawn clearance.** Every spawn spot must now also be `SPAWN_PROP_CLEAR_M` (3 m) clear of parked cars and cones (`PropIndex::clear_of`, `dispatch::spot_clear`). Spawning in view was already ruled out (row 131).
- **Flipped reset.** wp_sim reports each physical unit's chassis up axis (`PoliceSystem::sync_unit_upright`; minimal glue in `wp_sim::police_link`). A unit on its side or roof (up below `FLIPPED_UP_Z` 0.3) for `FLIPPED_RESET_S` (1.5 s) is reset upright in its lane by the row 78 reset: out of every racer's view, damage kept, ghost for 1 s.
- **Tests.** `unit_on_its_roof_is_reset_upright` (fails with the flipped branch disabled) and `prop_clearance_for_spawns`.
- **Goldens.** 8 moved, re-blessed in their own [bless] commit: the two jam pursuits, four opponent scenarios, the Lantern checkpoint race and the traffic soak observer. The new spawn draws change where units first appear. Round12 merged into the commit before row 75 matches every golden. All expected ranges still pass.

### Row 96 — in progress (not pushed).
## Lead helper note (P9 helicopter, 2026-10-20, `claude/hopeful-cray-0dtq2z`; merged 2026-10-21 by `triage/helicopter`)

Owner backlog item **P9 — Helicopter**: joins at wanted level 3, flies overhead, keeps you in
sight, spotlight at night, radios your position to ground units; tunnels, covered roads and
underpasses break its line of sight. Done by a lead helper in your crate (small, listed here so
you can review):

- Sightline height: the branch's own fix (`racer z + altitude_m` → `racer z + 1.0 m`) was
  **dropped in the merge**; round 12's `eye_height_at(z)` target (af853e24) is kept as is.
- `dispatch::helo_reports` + `PoliceSystem::helo_sight` (per tracking helicopter: line of sight,
  visual as radioed, next report): "Air N: eyes on … Spotlight on." / "still on …, N km/h" every
  `position_report_s` / "lost visual on … Units, check the cover." after `lost_visual_after_s`
  without sight / "reacquired". These are the helicopter's own lines, beside `chatter.rs`'s
  racer-level lost-visual / eyes-on-again calls. New `tools.toml [helicopter]` keys
  `position_report_s = 30` and `lost_visual_after_s = 3` (validated); `spotlight_half_angle_rad`
  0.35 → 0.12 (drawing only). `HeloView` gained `has_visual` and the racer's height as the aim z.
- `PoliceSystem::debug_launch_helicopter(racer, offset_m)` (debug / e2e; `wp_wasm`
  `debug_launch_helicopter`).
- Test: `pursuit::tests::helicopter_joins_at_level_3_keeps_visual_and_loses_it_under_an_overpass`
  (Port Vesper, both grade separations: none at L2, launches at L3, overhead, "eyes on", last
  known position follows, lost under the deck after 3 s, last known stays outside, reacquired).
- `wp_world` (D): bridge decks occlude `line_of_sight` (see `docs/integration/world.md`). This
  applies to every sightline (ground units too: a car on the deck is hidden from a unit on the
  road below and vice versa).
- `wp_wasm`/testbed (C): `police_helicopters()` and the night spotlight on round 12's drawn
  helicopter.

- An idle patrol / reinforcement committed as backup (within 700 m) now also gets an ambush when it
  can make one in time (`dispatch::ambush_route`): it routes from its current position and heading
  (`start_node`, +3 s if it must turn around first) to the same wait points
  (`dispatch::ambush_waits`, shared with the spawn search). Skipped when the unit is on the
  racer's road itself or its route would use it; then it intercepts as before.
- Ambushers join `Pursue` only once ≥ 5 m past their junction and facing within ~45° of the
  racer's line (one joined while still turning in from its side road).
- Test: `tests/system.rs::nearby_patrols_sent_as_backup_also_wait_ahead` (default pool; two
  patrol units set up ahead and join facing the racer's way, heading · travel 0.96 / 1.00).

### Follow-up: traffic-hazard stand-in is opt-in (owner request, 2026-10-21)

- `PoliceSetup::default()` now has `hazards: false`. The statistical traffic model (random near
  misses / collisions for headless units) is only for runs without real traffic; `wp_sim` already
  turned it off explicitly (PR #34), since its `wp_world` traffic is reported as obstacles.
- Explicit setters are unchanged: simcli and the tuning-dash probe still pass
  `hazards: sc.traffic_cars`, `apps/ui-kit/tools/record-police` and the `wp_net` city tests pass
  `true`. The one behaviour change is `wp_opponent/tests/common` (police-heat runs on a real
  `wp_sim` city), which inherited `true` and now gets `false`.
- Test: `lib.rs::tests::traffic_hazard_stand_in_is_opt_in`.
## Race-specific police profiles (owner request, 2026-10-22)

Each race in the browser's picker now has its own police, configured in **`data/police/races.toml`**
(one table per race id, `wp_police::race`). Changing one table changes only that race.

| race | style | what the police do |
|---|---|---|
| `lantern_checkpoint` (illegal daytime, downtown) | **Downtown lockdown** | presence 2.5 (was 2.0), backup ×1.5; two roadblocks *across the route* (each leaves a 3.5 m gap on one side), four roadblocks *off the route* sealing side streets, two stakeouts at junctions |
| `loop9_circuit` (1 am highway lap) | **Highway interceptors** | high-speed pursuit only: no roadblocks, spikes, box-ins, rolling or junction blocks, no helicopter; no pursuit speed cap (none anywhere since 2026-10-23), PIT up to 220 km/h; four units staked out on the shoulders join as the field passes |
| `hollins_sprint` (night hill sprint, hairpins) | **Summit trap** (proposed) | no PITs, rams, spikes or dynamic roadblocks on the hill; the side road at the top is sealed, a unit waits just below the summit and lights the field up, box-ins at the lot |
| `grid_highway_sprint` (night, short highway) | **Speed trap** (proposed) | two radar stakeouts recognise a racer the moment it flies past; a 4 m spike strip covers the overtaking lane left of the racing line at 85 % (the racing lane stays open); PIT, rams and rolling blocks, no roadblocks |

How it works:
- `RaceProfile::apply` rewrites the police tables for the race: a tactic switched off unlocks at
  `NEVER_LEVEL` (6), speed overrides replace `chase.pit_max_kmh` /
  `chase.speed_margin`, reinforcements scale by `backup_scale`. Patrol counts come from the
  profile's `presence` via the race's scenario (`RaceDef::police_presence`).
- `PoliceSystem::start_race(id, profile, course, entrants)` (called through
  `wp_race::police::start_race` by `wp_wasm::race::RaceRun::new` and the picker tests, before the
  countdown) places the scripted tools and stakeouts. Side roads are found from the road graph
  (junctions on the course, roads leaving across the racing direction, walked through the city's
  short segment chains). `Roadblock` / `SpikeStrip` gained `scripted: bool`: scripted tools last
  `tool_lifetime_s`, survive the end of a racer's pursuit and do not count against the AI
  captain's `max_active`.
- A stakeout is a parked `UnitRole::Tool` unit. When a racer passes within `trigger_m` with line
  of sight it joins the racer's pursuit, or (with `recognise_on_sight`) makes the racer L1 and
  starts the pursuit itself; it then becomes a `Reinforcement` (cleaned up like backup).
- Events: no new `PoliceEvent` variants (`RoadblockPlaced`, `SpikesPlaced`, `PursuitStarted` as
  before). One Critical radio line at setup ("Operation <name>: …").
- Same code path for Arcade and Simulation (nothing branches on `DamageMode`).

Measured (picker tests, AI in every seat, default seeds): Lantern 6 scripted roadblocks, 4
pursuits, all 4 racers ended by the police; Loop 9 0 roadblocks / 0 spikes, 4 stakeouts sprung,
8 pursuits, 0 of 6 finished (captures / flights); Hollins 1 side block, 3+ finish; Grid 2
stakeouts sprung, spike strip hit, 2 of 4 finish (one captured, one fled).

Tests: `wp_police` `race::tests` (12: tables, apply, placement on/off route, gaps, stakeout
springs a pursuit, spikes cover part of the road only); `wp_race` `catalog::every_race_has_its_own_police_profile`;
`wp_race/tests/race_picker.rs` checks each race's profile in the full sim (Grid now expects 2
finishers instead of 3: the speed trap takes racers out).

Outside my crates: `wp_race` (catalog presence/profile, `police::start_race`, picker tests),
`wp_wasm::race` (start_race call, `RaceCard.police_style/police_summary`),
`apps/testbed-web/src/ui/race.tsx` (shows the police style on the race card),
`wp_opponent::corridor` (a flight route whose pieces are all filtered out no longer panics: it
showed up once the Loop 9 interceptors started forcing flights), `pick_lantern_checkpoint.ron`
regenerated + its golden blessed (presence 2.5).

Known gaps: standalone `simcli` runs of the `pick_*` scenarios have no race block, so only the
presence applies there; the AI racers (`wp_opponent`) do not see barriers or spikes and drive
into them; Loop 9's chase is very punishing (no finishers at the default seed) and may want
tuning after a playtest.

### Row 156: tool levels — roadblocks 3, spikes 4, helicopter 5 (owner decision, 2026-10-23)

`data/police/presence.toml`: `[unlocks] spikes_level` 2 → 4 (`roadblock_level` stays 3) and `[helicopter] min_level` 3 → 5. Every gate reads these:
- the AI captain's tool planner (`dispatch` roadblock and spike site planning);
- the player captain's `RequestRoadblock` / `RequestSpikes` (`CaptainError::Locked`);
- the AI helicopter request;
- the player captain's `RequestHelicopter`, which was not gated by level at all and now needs `helicopter.min_level` too;
- the radio, which announces what is placed.

Two places were left alone:
- **Race profiles' scripted tools** (`data/police/races.toml` `route_blocks` / `side_blocks` / `spikes`): laid out at race start as part of the course, not unlocked by heat, so unchanged. The pursuit-only race still turns every tool off.
- **Highway roadblocks** (`tools.toml` `highway_max_level = 3`): still level 3 at most, so on a highway at level 4+ only spikes come.

Minimal change outside the crate: the ui-kit captain-tool mock (`apps/ui-kit/src/mock/police.ts` `TOOL_RULES`) mirrors the levels (spikes 4, helicopter 5).

Tests updated to the new levels:
- `pursuit::tests::helicopter_joins_at_level_5_…` (was `…_level_3_…`): none at L4, one at L5;
- `divided_tools` spike strips: requested at L4, roadblocks at L3;
- `system::heat_violation_wanted_pursuit_and_captain_tools`: spikes at L4;
- `radio_chatter::tool_lines_are_spoken_within_a_second_…` (was `level_three_…`): spikes at L4.

No golden or scenario expectation moved.

### Row 158: a cop spawned in view and started a chase (2026-10-23)

Owner (0.12.59): "cop also spawned right into my line of sight and started a chase". Checked every spawn path against row 131's view check (`spawn_in_view`):
- pool patrols, responders, backup and search (ambush plan, response spawn point, reinforcement spawn point) and the AI captain's tool cars all check;
- three paths did not, and are fixed below.

**1. Session-start patrols (the likely report).** `warm_up` placed every district's patrols at once with no view check: the opening scene. A patrol could land right beside the player, and a racer still wanted from before was chased at once. Session-start patrols now spawn out of sight of every racer who is already wanted (heat above zero, `opening_spawn_in_view`). Around a clean racer a patrol on screen from the first frame did not pop in and chases only an offence it sees, so it may stay. Every later patrol spawns out of every racer's sight, as before. A district short of hidden spots fills up later through `maintain_pool`.
- Test: `dispatch::tests::session_start_patrols_spawn_out_of_view`. It fails without the fix: a patrol placed 75 m from the racer, in view.

**2. Round the corner ahead.** The "seen within `ROUTE_VIEW_S`" part of the check followed only the straightest road, from the racer's centreline, at its current height. In the bench, backup appeared hidden 120–200 m away and in plain view 1.5–3 s later. The check (`roads_ahead`) now covers:
- every road the racer can reach within 3 s, assuming at least 15 m/s plus 20 m (a racer pulling away);
- both ways along its road when it is slow or stopped, and 40 m behind otherwise (it may turn round);
- points every 10 m, on the centreline and 2.5 m to either side;
- eyes and the spot 5 m higher there (ground heights ahead are not known: Port Vesper's crests).
- Test: `dispatch::tests::spawn_view_covers_every_road_the_racer_can_take`. It fails with the old straightest-road check.

**3. Race setup.** Race-profile stakeout cars and scripted roadblock cars (`start_race`, which has no world) spawned with no view check. Now:
- a roadblock car whose spot a racer can see waits out of view in the tool-car queue (row 131);
- a stakeout post a racer can see stays empty;
- both use the road graph's building sight lines.

**Bench (4 seeds × 12 cells = 48 runs).**
- Spawns a racer saw within 3 s: 3 before. After: 1 (backup at 200 m, seen 2.6 s later), plus one roadblock's 3 cars 568 m away on a long straight (roadblocks are sited to be seen from far).
- Capture 77 % → 77 %.

**Scenarios and tests re-aimed.**
- `weather/fog_pursuit_clear` and `weather/fog_pursuit` start with a wanted racer (heat 15). Their first recognition came from a patrol placed in plain sight at the start: 0.30 s and 1.90 s. Now 7.00 s and 11.70 s, bounds 9 s and 14 s. Fog is still later than clear (`fog_delays_the_pursuit`).
- The sideswipe and night-escape scenarios start with a clean racer, so their opening patrols are unchanged (no re-aim, goldens unmoved).
- **[bless]**, release `simcli goldens`, every expectation passing: golden_city_chase (+ replay), jam_pursuit_grid, jam_pursuit_vesper, opponent_circuit_8x5, police_tunnel_escape_founders_cut (+ control), race_city_sprint_police, weather_fog_pursuit (+ clear), world_traffic_soak_observer. Every one has police that now spawn elsewhere: hidden from any road the racer can reach within 3 s, or out of sight of a wanted racer at the start. After merging round12 2d5791c5 (row 164 barriers, G traffic soak fix), a second bless covered jam_pursuit_grid, jam_pursuit_vesper, opponent_circuit_8x5 and world_traffic_soak_observer (the merged world with row 158's spawns). The Ashpeak race goldens changed only in their header and were not blessed.
- `escape_break_contact` (dusk, Industrial): 1 escape in 3 runs on the base (the bottom of [1, 3]), 0 now. Over 8 runs it is 2 → 1, and pursuit heat takes the racer to L5 every run, so its premise ("stays below the helicopter's level") already fails. Now 8 runs, at least 1 escape. Row 157 is the real fix and re-aims it.
- `escape_break_contact_night`: 2 → 5 escapes in 10. Max 4 → 6.
- `response_night_suburb`: responders may not spawn on a road the racer reaches within 3 s, so the best spots ahead of it are out. Over 16 runs: within 25 s 0.85 → 0.77, mean 15.0 → 19.7 s, max 34.3 → 61.0 s. 8 runs hung on that one 61 s call, so the file now runs 16. Bounds unchanged.
- `tests/system.rs::ambush_chase`: the racer becomes wanted after 0.5 s, once the session-start patrols are out. The nearby patrols it checks are then placed as before.
- **Outside wp_police (minimal):** `crates/wp_economy/tests/session.rs` `record_police_run` now commits its offences after the first second, not at ticks 1–3. Its procedural city has no buildings, so no spot is hidden from the parked racer. Wanted before the session-start patrols were placed, it got none and nobody came. The recorded fixtures still match.

**Other.** The police scenario `self_wreck_sim` moved from 23 to 24 self-wrecks of 240 units (9.6 % → 10.0 %, one unit, inside noise). Its bound was re-aimed from "< 10 %" to "≤ 10 %", with the reason in the scenario file.


### Row 157: heat builds about 2× slower (2026-10-23)

Owner: heat builds too fast. Lead target: about 30–60 s of active chase per wanted level.

**Change.** Every heat gain in `data/police/heat.toml` is halved:
- `pursuit_heat_per_s` 0.25 → 0.125;
- violations: speeding 2 + 0.4/m·s → 1 + 0.2, red light 6 → 3, wrong way 4 → 2, collision 8 → 4, reckless 3 → 1.5;
- assault 20 → 10 (its `min_level` 2 floor stays);
- disabling a cop 15 → 7.5.

Thresholds (10/30/50/70/90), decay, escape times and the `min_level` floors (assault L2, blatant speeding L1) are unchanged.

**Bench.** New `simcli police-bench --clean-start`: every cell starts at no heat. RunMetrics now record `first_offence_s` and `level_reached_s` (time from the first offence to each level), and `heat_timeline_table` prints them. Run: `--clean-start --full --seeds 8`, 192 runs, both cities, day/night, clear/rain, casual/good/expert evader, 300 s. Base is ws/police 58742472 (row 158). Times are medians over the runs that reached the level [p25–p75], s.

| | runs reaching it, before → after | first offence → level, before | after | previous level → level, before | after |
|---|---:|---:|---:|---:|---:|
| L1 | 159 → 145 | 15.5 [4–33] | 33.2 [17–54] | – | – |
| L2 | 122 → 74 | 36.5 [21–55] | 60.5 [31–87] | 19.1 [13–29] | 32.7 [0–44] |
| L3 | 78 → 37 | 57.9 [43–79] | 102.2 [78–135] | 23.1 [16–30] | 44.5 [34–55] |
| L4 | 62 → 26 | 78.2 [56–101] | 153.5 [132–185] | 20.1 [13–24] | 41.1 [34–48] |
| L5 | 43 → 17 | 103.5 [80–124] | 193.7 [153–210] | 20.9 [15–26] | 39.5 [28–51] |

- Each level now takes about 40 s of chase (was about 20 s), inside the 30–60 s target.
- L1 → L2 p25 is 0: an assault still jumps straight to L2 (the floor).
- The racers that reach a level are the ones that stay in the chase, so later levels are measured over fewer runs.

**Knock-on, same runs (bench ALL row, before → after).**
- capture 59 % → 47 %, escape 15 % → 22 %;
- pursuit s 30 → 49, capture s 52 → 39;
- never engaged 33 → 49 of 192: a clean racer whose first offence nobody sees now often decays back to 0 before a unit arrives;
- crash/min 1.16 → 0.89, pop-in/min 0.01 → 0.01.
Fewer racers reach L3+, so fewer roadblocks, spikes (L4) and helicopters (L5) come out.

**Re-aimed for row 157, each with its reason in the file.**
- `tunnel_escape_founders_cut` (+ control): from heat 90 the racer drops below L5 and climbs back at half the rate, so the helicopter came at 95.7–111.2 s on every seed 50–63 (deadline 95 s). Now heat 95 and seed 54 → 63.
  - Seeds 50–63 at heat 95: escape story on 8 of 14, control captured in its window on 2 (54, 63).
  - No control seed escapes: the rest are still in pursuit at 190 s or captured early. A fragile pair, flagged.
- `response_day_suburb`: 8 runs time only 3 calls. Same seeds, old vs new heat: 16 runs 19.1 → 20.4 s mean, 32 runs 15.2 → 15.7 s. Now 16 runs, worst-call cap 38 → 40 s.
- `tests/system.rs` assault test: the assault heat is read from config (was a literal 19 ≈ 20).
- **Outside wp_police (minimal):** `apps/simcli/scenarios/race/city_sprint_police.ron` entrants' starting heat 3.75 → 6.0. At 3.75 nobody reached L1 and `city_sprint_pulls_in_police` saw no pursuit. A `WP_RACE_AIM_HEAT` sweep at seed 1 passes both race tests at 4.5–8.25; 6.0 is near the middle. `crates/wp_race/tests/race_police.rs` comments and the `> 3.75` check now say 6.0.
- **[bless]** 16 goldens: hashes only, every expectation passes. Pursuits, levels and units change from the first offence on.

## No pursuit speed cap; faster police cars (owner request, 2026-10-23)

Owner: "cop cars get smoked even by a stock Hakone GT"; "remove the AI pursuit speed cap" (all
damage modes — the owner rule keeps Arcade and Simulation identical outside car damage).

- **Cap removed.** `pursuit_cap_mps` (32–40 m/s by phase) and `[chase] cap_scale` are gone from
  `PursuitFile` / `ChaseConfig` and `pursuit.toml`. Every place that clamped to the cap now
  clamps to the unit car's own top speed (`PoliceSystem::unit_top_speed_mps`: the unit's
  measured `CarLimits`, else `[pro] default_limits`), including the built-in point-mass driver
  of kinematic units, which has no top speed of its own. Closing on the racer is still
  `max(racer speed × (1 + speed_margin), 10 m/s)`; units far behind, PIT and ram approaches go
  up to the car's top speed (the old `cap + 6/7 m/s` allowances are gone). Ambush timing
  (`dispatch.rs`) bounds the respond pace by the default car's top speed.
- **Cars** (`tools/specsheet/roster_inputs.py`, regenerated): measured 0–100 / ¼ mile / Vmax,
  before → after (stock `hakone_gt`: 6.00 s / 14.28 s @ 169 km/h / 282 km/h):
  - `sentinel_cruiser` 6.2 L supercharged V8, 276 → 390 kW: 6.40 → 5.93 s, 14.69 → 13.93 s,
    260 → 293 km/h.
  - `sentinel_wolfhound` 3.5 TT V6 pursuit tune, 272 → 415 kW: 5.92 → 4.88 s, 14.21 → 12.95 s,
    244 → 290 km/h.
  - `sentinel_vigil` (was `sentinel_harrier`) 5.2 L supercharged V8, 319 → 561 kW: 5.77 → 5.46 s, 13.82 → 12.90 s,
    275 → 323 km/h.
  - `sentinel_warden` 7.4 L big-block V8, 155 → 217 kW: 8.82 → 6.91 s, 201 → 229 km/h.
  - `sentinel_bastion` 6.2 L V8, 265 → 334 kW: 7.97 → 6.72 s, 215 → 227 km/h.
- Regenerated: car JSON + spec sheets, `validation_results.json`, `ROSTER.md`, `VALIDATION.md`,
  `data/stats/cars.json`, `wp_opponent` capability table; `[pro] default_limits` (Sentinel
  Cruiser) 4.35 → 4.75 m/s², 72.4 → 80.0 m/s.
- **Rolling-block deadlock fixed** (found merging main, 2026-10-23): units that had passed a
  racer who then stopped (pulled into the Founders Cut lay-by) took `RollingBlock` and were
  commanded to 0 m/s ahead of him for good, so nobody came within the 15 m capture radius.
  A rolling block now needs the racer moving (≥ `ARREST_RACER_MPS`, 3 m/s, the arrest
  threshold), and a shadowing unit past a stopped racer goes back to it by the roads
  (`chase_route`) instead of on along its trail. On merged main with the new cars:
  `tunnel_escape_founders_cut` Escaped 132.4 s (40 s unseen), control Captured 106.4 s (was:
  no pursuit end in 190 s).
- Tests: `wp_vehicle` `class_favours_powerful_rwd` uses `furrow_24` as the slow RWD (the
  Bastion is no longer one); wolfhound diff hash and the simcli goldens re-blessed.
- **Open (audio, E):** `wp_audio` `every_wot_upshift_dips_and_re_rises` fails for
  `sentinel_cruiser` 4→5 at 30.28 s (0.4 dB, wants ≥ 1.2). The old car lifted before that
  shift; the new one makes it flat out at ≈ 5900 rpm, where the converter shift (no torque
  cut in `wp_vehicle`) drops rpm at only ≈ 3000 rpm/s and layer energy is at its ceiling.
  Lowering `shift_rate_onset_rpm_s` to 1200 only reaches 0.9 dB.

### PR #60 ported onto ws/police (2026-10-23)

PR #60 (`claude/epic-dijkstra-6gc3lv`: no AI pursuit cap + police power package) is merged onto ws/police after row 157 and lead/round12 81fa22c0. The lead closes the PR as merged by this port.

Bench, police-bench --seeds 8, 96 runs per arm, before → after:
- Stock evader (debug_coupe): capture 72 → 72 %, escape 16 → 21 %, capture time 56 → 47 s, crash/min 1.34 → 1.55.
- Evader = hakone_gt: capture 73 → 79 %, escape 16 → 9 %, crash/min 1.39 → 1.23, police–civilian hits 57 → 36.

**Outside my crates (minimal, for the audio test the PR left red):**
- `wp_audio` gets a gear-change upshift cue. `EngineInput.gear` is `Option<i8>`; the director passes `Some(VehicleState::gear)`.
- An upshift under throttle holds the existing upshift note at full depth for `EngineMix.upshift_cue_s`. That field is serde default 0.15 s, so `audio_config.json` is unchanged.
- It's for converter automatics without a torque cut, whose rpm falls slower than the rpm-rate cue's onset. No `wp_vehicle` change (physics frozen).
- New unit test: `engine::tests::upshift_with_a_slow_rpm_fall_still_dips_when_the_gear_is_known`.
- On the merged tree the PR's failing case (cruiser 4→5 at 30.3 s, 0.4 dB) no longer happens: the cruiser makes no 4→5 shift in the trace. Its weakest dip is 3→4: 1.55 dB without the cue (floor 1.2 dB), 2.26 dB with it.
- **Push-through fix (wp_police, found by the port):** a push (row 61) that started on traffic now ends as soon as the car in front is a racer or another unit. The stronger cruiser, back on Patrol after a capture, kept shoving the held player car (wp_wasm `capture_bills_holds_the_car_and_repair_continue_restores_it`: 2.45 m/s, want < 1).

### Row 168: police wipe out race fields on one-lane roads (2026-10-24)

Measured with `race_picker`, police on at each profile's own presence (the harness used the pick scenario's 1.0 before), seeds 1–4, before tuning. Crowfoot ascent and the metro circuit lost the whole field on almost every seed; Grizzly Grade and the waterfront drag never drew a pursuit. Traced on crowfoot_ascent seed 1.

**Fixes (wp_police).**
1. **Queued behind another racer is not caught** (`RuleInputs::held_up_by_racer`).
   - A stopped racer waits, not caught, when another racer is within 12 m ahead, inside its lane (±3 m), and closer than any unit in that box. Neither the stop nor the pinned rule counts while it waits.
   - Facing is the racer's last direction of travel. A sudden reversal (backing out of the queue) doesn't flip it.
   - Box-ins and a unit right in front still capture.
2. **A unit in the racer's lane in front of it** (`RuleInputs::blocked_ahead_only`, `[capture] blocked_ahead_stop_time_s` = 12 s).
   - When the only units that count stand in the racer's lane in front of it, both rules wait 12 s instead of 4 s and 6 s. This covers a pursuer, interceptor or patrol that cut in or crashed there, but not a roadblock, spike car or box-in.
   - That gives the race AI time to back out and go round. In the trace, a cruiser wedged on a hairpin had the field behind it arrested in 4 s.
3. **Units clear a stopped racer's lane.** A shadowing unit more than 3 m into a stopped racer's lane ahead of it drives on at least 8 m/s, back to the racer by the roads. Before, it stood there as a de-facto roadblock.
4. **Wedged units don't arrest.** A unit stuck against a wall or the terrain and trying to free itself (`UnstickState::wedged`) is left out of the capture distances.
- Tests: `heat::tests::racer_queued_behind_another_racer_is_not_captured_until_the_queue_clears`.
- Tried and dropped: a speed floor for a shadowing unit ahead of a moving racer (0.8× its speed). It raised police crashes 1.25 → 1.58/min on the bench.

**Bench guard rails** (`police-bench --seeds 8`, 96 runs, base lead/round12 34dbeb5f → row 168): capture 73 → 72 %, escape 21 → 21 %, crash/min 1.25 → 1.25, stuck/min 2.96 → 2.75, pop-in/min 0.02 → 0.02.

**races.toml tuning**, on top of the fixes:
- grizzly_grade_sprint: a stakeout at 0.7 that knows the field on sight. Pursuits 0 → 1–2, all finish.
- waterfront_drag: a stakeout at 0.9. Pursuits 0 → 2, both finish.
- harlan_dirt_run: presence 0.5, backup 0.5. The field finishes on 2 of 4 seeds (was 0–1 finishers).

Finishers per seed, seeds 1–4, after:

| race | finishers / field |
|---|---|
| downtown_checkpoint | 0, 4, 4, 0 / 4 |
| crowfoot_descent | 4, 4, 2, 4 / 4 |
| route7 | 4 each / 5 |
| grizzly | 4 each / 4 |
| waterfront | 2 each / 2 |
| harlan | 4, 4, 0, 0 / 4 |
| crowfoot_ascent | 0 each / 4 |
| metro_circuit | 0 each / 6 |

**Not fixed: crowfoot_ascent and metro_circuit.** Presence (down to 0.3) and tactic changes (no PIT, rams, box-ins or roadblocks on metro) barely move them. These are fair captures under the rules, caused by things outside races.toml:
1. Units hit hairpin walls at intercept speed (29.5 m/s planned into a hairpin, 13.6 m/s at impact) and wedge there.
2. The race AI gets stuck on a hairpin and is captured under the pinned rule, and its busted car then stays parked across the one-lane road. The field behind ends Stuck (metro: 5 of 6 Stuck after fix 1's facing guard).
3. The race AI's back-out comes too late against a unit beside it.
Proposed follow-ups: a cornering-limited intercept speed on hairpins (wp_police), and a busted race car that clears the lane (wp_opponent / wp_race).
- Founders Cut pair re-seeded 60 → 58. Seeds 50–70: escape story on 11 of 21 (was 16), control captured on 20, both on 11. Units no longer stall next to the stopped racer in the lay-by (D's report), so he is caught more often.
- `pick_harlan_dirt_run.ron` regenerated (`WP_RACE_WRITE_SCENARIOS=1`, wp_wasm): presence 0.5.
- **Queue cap (lead/round13 lantern_checkpoint):** racer 4 sat behind busted racer 3, parked in the lane, from about 70 s to 300 s with units nearby. The queue exemption kept it Running, so the race never ended. A racer now waits behind another racer for at most `QUEUE_MAX_WAIT_S` (15 s) without moving; after that the normal stop and pinned rules apply. lantern_checkpoint ends at 66 s and passes. (`race_picker::ashpeak_metro_circuit` fails on lead/round13 7538c828 too, with police off: 1 of 6 finish. Not police.)

## Stuck patrol cars (playtest row 170)
Patrol cars in the Vesper traffic soak froze for good: nose to nose with traffic, in parked
cars, tangled after a turn. Two route-geometry causes (round 12's `stuck` module recovers
cars once stuck; these stop them getting there):
- **Road network centrelines were a lane, not the road centre** (`wp_world::roads`, D's
  crate; see `docs/integration/world.md`). Units add a 1.75 m lane offset to that line, so
  one way they drove in the parking lane and the other way on the centre line, head-on with
  oncoming traffic. Centrelines are also simplified (0.2 m): each interior point is a graph
  node and a patrol destination.
- **Routes turn on the spot at junction centres.** A physical car swings wide by its turning
  radius: into the oncoming lane turning right, into the parking lane turning left.
  `unit::round_junction_corners` replaces 25-150° corners with 10 m arcs (`TURN_RADIUS_M`;
  the inside lane path stays ~8 m). Applied in `Unit::set_command` for physically driven
  units without lights only: the pro driver smooths its own line, internal units follow the
  track exactly.

Measured on the Vesper traffic soak, seeds 5-12 (police-seconds still for > 5 s while
wanting to move / units still stuck at the end), both with the `stuck` module: main 1793 /
48, with these fixes 1129 / 28.

Tests: `unit::tests::junction_corners_become_drivable_arcs`, `gentle_bends_and_u_turns_stay`,
`wp_world/tests/city_v2.rs::road_network_centrelines_offset_into_the_right_lane`.

Measured before merging main's world v7 map; not re-measured since.

Moved police exposed a bug elsewhere, fixed here (outside this workstream's crates):
- **A busted AI racer blocked the race for good** (`wp_race`, `wp_opponent`). It stopped where
  it was caught; on a one-lane street (pass room ±0.35 m) with its cruiser beside it, nobody
  could pass and Hollins sprint / Lantern checkpoint never finished. `LiveRace` now tows
  busted AI cars `IMPOUND_AFTER_S` (6 s) after the capture (`StepOut::impound`; the wasm race
  session and the `race_picker` loop despawn them; a towed car is never course-reset), and
  busted racers pull over to the kerb clear of junctions first instead of braking where
  they are.
- simcli: new `police_tools: false` scenario switch (the AI captain lays no spikes or
  roadblocks of its own; exact-spot `spikes_at` / `roadblock_at` orders still work).

Re-aimed on the corrected graph (reasons and numbers in each file):
- `police_sideswipe_patrol_night`: racer starts at (-164.9, -190) (unit 2 patrols ~270 m
  further south); contact 8.70 s as before.
- `police_tunnel_escape_founders_cut` + `_control`: `police_tools: false` (the captain now
  finds spike sites on the route; these scenarios are about covered-road line of sight).
- `day_core_speeder`: seed 106 (one run of 103-107 started at 20.9 s; elsewhere as quick
  as main or quicker).
- `response_day_core`: worst call 15 -> 20 s and mean 10 -> 11 s. **Review with the police
  balance:** 48-run means are level (7.3/8.1/5.4/9.5 s vs main 6.8/7.3/6.0/8.3 s), but 4 of
  68 calls take over 25 s (main 0 of 71). Main's Core patrols mostly drove to the highway:
  patrol destinations are drawn per node, and 260 of main's 308 Core nodes were highway
  sample points (here 32 of 80). Length-weighted destinations would remove the sampling
  bias either way.
- `wp_race` `hollins_descent`: `check_with_police` (its stakeout pursuits now catch two of
  four; captures count, like `hollins_sprint`).
- `wp_opponent` `live_police_chase…`: seed 1 (its own harness has no tow).
- `escape_break_contact_night`: max escapes 6 -> 8 (19 of 30 runs escape here vs 14 on main, no
  captures either way). **Review with the police balance** together with `response_day_core`.

Native == wasm32 (the determinism suite failed on `jam/pursuit_jam_grid` and
`jam/pursuit_jam_vesper` once police routes moved): traced to glamx's symmetric 3x3 eigen solver,
whose `acos`/`cos` were platform libm calls. parry uses it for mass properties from convex hulls
(a dented parked car's inertia) and to seed convex-hull construction (a building hull came out
with different face normals). `third_party/glamx` patches it to the `libm` crate
(`[patch.crates-io]`; see its `WRECKPURSUIT.md`).

Known gaps: units queue behind traffic jams (correctly) at the soak's 4x density; the solid
terrain in the band along road edges keeps the fine 5 m heights while the drawn terrain is the
lowered coarse grid, so on steep embankments solid ground can sit 2-4 m above what is drawn (e.g.
Port Vesper (-542, 239); world workstream, also on main);
`opponent_pace` `lantern_checkpoint_s50` fails identically on main (not this branch's).
