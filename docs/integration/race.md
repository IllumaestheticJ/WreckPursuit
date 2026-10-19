# Race events, crews & territory integration notes

- **Workstream / prompt:** 15 — race types, event scheduler, crews & territory (machine L)
- **Branch:** `ws/race`
- **Crates / folders owned:** `crates/wp_race`, `data/events/`, `apps/simcli/scenarios/race/`
- **Contract version built against:** 0.1.0 (codes toward the v0.2.0 plan; see requests)

Design write-up: `crates/wp_race/README.md`. Data: `data/events/README.md`.

## Exposes

### Lifecycle `wp_net` / the server drives

```rust
use wp_race::*;

let data = RaceData::embedded();                       // or RaceData::load_dir("data/events")

// 1. Calendar (once per in-game day; same on every machine for the same seed).
let venues = bridge::venues_from_map(world.map());     // RaceStartGrid POIs → Venue
let sched = Scheduler { rules: &data.schedule, stakes: &data.stakes, seed: world_seed };
let cal: DayCalendar = sched.generate_day(day, &venues, &meet_heat);
for ev in cal.lobbies_opening(prev_abs_s, now_abs_s) { /* broadcast lobby, RaceSignal::LobbyOpened */ }

// 2. Course + judge when the lobby opens.
let route = world.plan_route_between_pois(a, b)        // or plan_circuit / plan_route
    .unwrap();
world.set_active_routes(vec![bridge::active_route(route.clone(), ev)]);  // closed for sanctioned
let course = bridge::course_from_route(&route, 3.0);
let mut judge = RaceJudge::new(ev.id, data.rules.for_kind(ev.kind), course);
judge.set_police(&police_entities);

// 3. Entries (after the lobby checks below and wp_economy::enter_race).
data.progression.check_entry(&profile, class_index, &ev.requirements)?;
let slot: GridSlot = judge.enter(RacerEntry { racer: entity, redline_rpm })?;
sim.place_vehicle(entity, slot.pos, yaw_of(slot.dir));
// judge.withdraw(entity) while still in the lobby.

// 4. Start + every tick (physics or net tick; both judge the same).
let green_tick = judge.start(sim.tick())?;
meet_heat.record_race(&venue, abs_s, &data.schedule.meet_heat); // → maybe PatrolAttracted
judge.observe(sim.tick(), &samples, &tick_events);   // samples: RacerSample::from_state(id, &state, throttle)
judge.dnf(entity, DnfReason::Captured, tick);        // from wp_police capture
judge.dnf(entity, DnfReason::Disconnected, tick);    // from wp_net
judge.relocate(entity, pos, tick);                   // server reset/respawn (not a teleport DQ)
for s in judge.drain_signals() { /* fan out, see below */ }

// 5. Results.
if judge.is_finished() { let results: Vec<RacerResult> = judge.results(); }
```

Other entry points: `Course::{straight, closed, new, grid, marshal_spots, project, point_at}`,
`StakeTable::stake(kind, phase, bracket, legality)`, `CrewRegistry` (create/join/leave,
`set_bank_cut`, `withdraw`, `credit_race`, `issue_challenge`, `answer_challenge`,
`resolve_challenge`), `Territory` (`award_race`, `award_challenge`, `advance_to_day`,
`heat_decay_multiplier`, `safehouse_for`, `controlled_by`), `ProgressionTable`
(`bracket_for`, `licence_for`, `check_entry`), `trace::simulate` (scripted races for tests/tools).

### Signals (`RaceSignal`) and who consumes them

| Signal | `wp_net` (HUD/lobby) | `wp_economy` | `wp_police` | territory |
|---|---|---|---|---|
| `LobbyOpened { pos, kind, legality, police }` | lobby UI | — | apply `police` presence near `pos` (`Max` = illegal daytime) | — |
| `Countdown`, `Green` | lights | — | — | — |
| `CheckpointPassed`, `LapCompleted`, `LeadChanged` | HUD | — | — | — |
| `RacerFinished`, `RacerDnf`, `RacerDq` | HUD | — | — | — |
| `RaceFinished { results }` | results screen | settle: `bridge::economy_result(r, wanted)` + `bridge::economy_setup(..)` → `Economy::settle_race`; crew cut via `CrewRegistry::credit_race`; `cargo_damage_cost` charge; pink slip from `ChallengeOutcome` | add `Stake::heat` to illegal racers | `Territory::award_race(district, legality, ..)` |
| `PatrolAttracted { venue, pos, heat }` | — | — | dispatch a patrol to `pos` | — |
| `TerritoryChanged { district, crew }` | map overlay | — | `heat_decay_multiplier` changes | — |

## Needs

| From | What | Why | Workaround now |
|---|---|---|---|
| 07 `wp_net` (F) | call the lifecycle above from the server; `RaceCommand` messages (R6) | lobby transport is theirs | none; `trace::simulate` + simcli tests stand in |
| 06 `wp_police` (E) | presence override API, capture notification, heat add for `Stake::heat`, patrol dispatch for `PatrolAttracted`, heat-decay multiplier input | race stakes beyond cash | signals carry everything; nothing consumes them yet |
| 08 `wp_economy` (G) | accept `wp_race` kinds (R5) or a purse from `Stake`; charge `cargo_damage_cost`; apply `CrewCut` | 3 of 8 kinds map onto Sprint | `bridge::economy_race_type` maps Outrun→Touge, TimeAttack→Circuit, Checkpoint/Cannonball/Delivery→Sprint |
| 02 `wp_vehicle` (A) | extra payload mass hook (`set_payload(kg, chassis_offset)`) | Delivery cargo mass | cargo is scored but not felt |
| 02 `wp_vehicle` (A) | FYI: `debug_sedan` pulls ~12 m right over 402 m at full throttle with zero steer (see `race_drag_coupe_vs_sedan`, which now dabs left twice) | torque steer or asymmetry? | scenario corrects it |
| 05 `wp_world` (D) | a marshal prop kind (`PropKind::Marshal`) or OK to spawn them as `Sim::add_static_box` | sanctioned events | `Course::marshal_spots` returns poses only |
| 10 harness (I) | add `wp_race` to `lint-determinism::SIM_CRATES`; scenario v2 race block (`race: (kind, course, …)`, `expect: race_position(entity, pos)`, `race_status(entity, Dq(LeftCorridor))`) | judge results inside simcli itself | `crates/wp_race/tests/simcli_races.rs` judges the runs via `run_with_hook`; `tests/determinism_lint.rs` self-lints |
| 17 opponent AI | AI entrants for meets (`ScheduledEvent::spawn_s`) | populated meets | — |

## Contract change requests

| # | Change (type / field / variant) | Why | Who is affected | Status |
|---|---|---|---|---|
| R1 | `SimEvent::Race(RaceEvent)` with `RaceEvent` = the `wp_race::RaceSignal` variants (`LobbyOpened`, `Countdown`, `Green`, `CheckpointPassed`, `LapCompleted`, `LeadChanged`, `RacerFinished`, `RacerDnf`, `RacerDq`, `RaceFinished { results: Vec<RaceResultRow> }`, `PatrolAttracted`, `TerritoryChanged`), plus the small types they need moved into contracts: `RaceEventId(u32)`, `RaceKind`, `Legality`, `ClassBracket`, `DnfReason`, `DqReason`, `RacerStatus`, `RaceResultRow { racer, position, status, elapsed_subticks, best_lap_subticks, clean, contacts, civilian_hits, cargo_damage_cost }` | economy, police, net and the renderer all react to race events; same bump as typed `PoliceEvent`/`EconomyEvent` | 07, 08, 06, 04, 10 | requested |
| R2 | `PolicePresence { None, Normal, Elevated, Max }` in contracts | `police_presence_override: max` for illegal daytime races must reach `wp_police` | 06, 07 | requested |
| R3 | `DistrictId(u32)` = zone index (joins deferred 08 #6) | territory is keyed by district | 05, 08, 06 | requested (wp_race uses a local `DistrictId`) |
| R4 | Durable account id (accepted F NET-5) also used by crews/licences | crews outlive instances | 07, 08 | accepted upstream; wp_race uses a local `RacerAccount(u64)` until it lands |
| R5 | `wp_economy::RaceType` keyed by the contract `RaceKind` (or `RacePayout` carries the purse from `wp_race::Stake`) | Outrun, Checkpoint, Cannonball, Delivery, TimeAttack have no economy type; legality is lost | 08 | requested |
| R6 | `ClientToServer::RaceCommand { Enter { event }, Withdraw { event }, Ready }`, `ServerToClient::Race(RaceEvent)` (inside F's NET-1 message list) | lobby join/leave from clients | 07, 04 | requested |

## Changes outside my crates

| File | Change | Why |
|---|---|---|
| `Cargo.toml` (root) | `crates/wp_race` workspace member + `wp_race` workspace dependency | new crate |
| `apps/simcli/scenarios/race/*.ron` | two new scenarios (simcli v2, no new fields) | scenario tests live there per `CLAUDE.md` |
| `apps/simcli/golden/race_*/hashes.txt` | goldens for those scenarios (`[bless]` commit) | `goldens_match` requires one per scenario |

No edits to `wp_contracts`, `wp_world`, `wp_economy`, `wp_sim` or simcli code.

## Perf numbers

| Measurement | Machine | Native | Browser | Budget |
|---|---|---|---|---|
| Judge, 12 cars, every physics tick (`--example judge_bench`) | cloud container | 0.195 µs/car/tick ≈ 0.0008 ms/car/frame | not measured | none set (police: 0.5 ms) |
| Calendar generation, 24 venues | cloud container | 2.0 µs/day | not measured | — |

## Known gaps

See `crates/wp_race/README.md` → Known gaps. Main ones: nothing consumes the signals yet (needs
F/E/G call sites), Outrun gap is straight-line, Delivery cargo mass not applied to the car,
crews/territory not persisted, no AI entrants.

## Lead feedback
(Written by the integration lead.)
